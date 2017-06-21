pub mod qemu;

mod control;
mod monitor;
mod clientpipe;
mod controller;
mod my_io;
mod signalfd;
mod sd_notify;
mod samba;
mod dbus;
mod sleep_inhibitor;
pub mod libinput;

use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener as StdUnixListener};
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio_core::reactor::Core;
use futures::{Future, Stream, Sink, future};

use driver::controller::Controller;
use config::Config;
use driver::signalfd::{SignalFd, signal};
use self::monitor::Monitor;
use self::clientpipe::Clientpipe;

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;

pub fn run(cfg: &Config, tmp: &Path, data: &Path) {
    let _ = fs::remove_dir_all(tmp); // may fail - we dont care
    fs::create_dir(tmp).expect("Failed to create TMP_FOLDER"); // may not fail - has to be new
    trace!("created tmp dir");

    let monitor_socket_file = tmp.join("monitor.sock");
    let monitor_socket = StdUnixListener::bind(&monitor_socket_file)
        .expect("Failed to create monitor socket");
    debug!("Started Monitor");

    let clientpipe_socket_file = tmp.join("clientpipe.sock");
    let clientpipe_socket = StdUnixListener::bind(&clientpipe_socket_file)
        .expect("Failed to create clientpipe socket");
    debug!("Started Clientpipe");

    let control_socket_file = tmp.join("control.sock");
    let control_socket = StdUnixListener::bind(&control_socket_file)
        .expect("Failed to create control socket");
    fs::set_permissions(control_socket_file, Permissions::from_mode(0o777))
        .expect("Failed to set permissions on control socket");
    debug!("Started Control socket");

    let mut qemu = qemu::run(cfg, tmp, data, &clientpipe_socket_file, &monitor_socket_file);

    let (monitor_stream, _) = monitor_socket.accept().expect("Failed to get monitor");
    drop(monitor_socket);

    let (clientpipe_stream, _) = clientpipe_socket.accept().expect("Failed to get clientpipe");
    drop(clientpipe_socket);

    sd_notify::notify_systemd(false, "Booting ...");
    debug!("Windows is starting");

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let mut monitor = Monitor::new(monitor_stream, &handle);
    let mut clientpipe = Clientpipe::new(clientpipe_stream, &handle);

    let (mut input, input_events) = libinput::Input::new(&handle);
    input.suspend();
    let input = Rc::new(RefCell::new(input));

    let monitor_sender = monitor.take_send();
    let ctrl = Controller::new(cfg.machine.clone(), monitor_sender.clone(), clientpipe.take_send(), input.clone());
    let controller = Rc::new(RefCell::new(ctrl));

    let sysbus = sleep_inhibitor::system_dbus();
    let ctrl = controller.clone();
    let inhibitor = sleep_inhibitor::sleep_inhibitor(&sysbus, move || ctrl.borrow_mut().suspend(), &handle);

    let ref input_ref = *input;
    let input_listener = libinput::InputListener(input_ref);
    let input_handler = input_events.filter_map(|event| {
        use self::monitor::*;
        use std::iter;
        use input::event::{Event, KeyboardEvent, PointerEvent};
        use input::event::pointer::{Axis, ButtonState};
        use input::event::keyboard::{KeyState, KeyboardEventTrait};
        Some(match event {
            Event::Pointer(PointerEvent::Motion(m)) =>
                QmpCommand::InputSendEvent { events: vec![
                    InputEvent::Rel { axis: "x", value: m.dx() as u32 },
                    InputEvent::Rel { axis: "y", value: m.dy() as u32 },
                ]},
            Event::Pointer(PointerEvent::Button(b)) =>
                QmpCommand::InputSendEvent {
                    events: vec![InputEvent::Btn {
                        down: b.button_state() == ButtonState::Pressed,
                        button: match b.button() {
                            BTN_LEFT => InputButton::Left,
                            BTN_RIGHT => InputButton::Right,
                            BTN_MIDDLE => InputButton::Middle,
                            BTN_SIDE => InputButton::Side,
                            BTN_EXTRA => InputButton::Extra,
                            b => {
                                warn!("Unknown mouse button {}", b);
                                return None;
                            }
                        }
                    }]
                },
            Event::Pointer(PointerEvent::Axis(ref a)) if a.has_axis(Axis::Vertical) => {
                let steps = a.axis_value_discrete(Axis::Vertical).map(|x| x as i32).unwrap_or(0);
                if steps == 0 {
                    // stop event, ignore
                    return None;
                }

                let direction = if steps > 0 {
                    InputButton::WheelDown
                } else {
                    InputButton::WheelUp
                };

                QmpCommand::InputSendEvent {
                    events: iter::repeat(direction).flat_map(|b| vec![
                        InputEvent::Btn { down: true, button: b },
                        InputEvent::Btn { down: false, button: b },
                    ]).take(steps.abs() as usize).collect(),
                }
            },
            Event::Keyboard(KeyboardEvent::Key(k)) =>
                QmpCommand::InputSendEvent {
                    events: vec![ InputEvent::Key {
                        down: k.key_state() == KeyState::Pressed,
                        key: KeyValue::Qcode(match key_convert(k.key()) { Some(x) => x, None => return None, }),//KeyValue::Number(k.key())
                    }]
                },
            event => {
                info!("Unhandled input event {:?}", event);
                return None;
            }
        })
    }).forward(monitor_sender.sink_map_err(|_| ())).then(|_| Ok(()));

    let control_handler = control::create(control_socket, &handle, controller.clone());

    let signals = SignalFd::new(vec![signal::SIGTERM, signal::SIGINT], &handle);
    let catch_sigterm = signals.for_each(|_| {
        controller.borrow_mut().shutdown();
        Ok(())
    }).then(|_| Ok(()));

    let joined = future::join_all(vec![
        inhibitor,
        clientpipe.take_handler(controller.clone(), &handle),
        clientpipe.take_sender(),
        control_handler,
        monitor.take_handler(controller.clone()),
        monitor.take_sender(),
        Box::new(catch_sigterm),
        Box::new(input_listener),
        Box::new(input_handler),
    ]);
    core.run(joined).unwrap();

    qemu.wait().unwrap();
    info!("windows-gaming-driver down.");
}


// http://elixir.free-electrons.com/linux/latest/source/include/uapi/linux/input-event-codes.h
// into
// sed 's/#define /const /' | sed -E 's/[\t]+/: u32 = /' | awk '{ print $0 ";"; }'
const KEY_ESC: u32 = 1;
const KEY_1: u32 = 2;
const KEY_2: u32 = 3;
const KEY_3: u32 = 4;
const KEY_4: u32 = 5;
const KEY_5: u32 = 6;
const KEY_6: u32 = 7;
const KEY_7: u32 = 8;
const KEY_8: u32 = 9;
const KEY_9: u32 = 10;
const KEY_0: u32 = 11;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_BACKSPACE: u32 = 14;
const KEY_TAB: u32 = 15;
const KEY_Q: u32 = 16;
const KEY_W: u32 = 17;
const KEY_E: u32 = 18;
const KEY_R: u32 = 19;
const KEY_T: u32 = 20;
const KEY_Y: u32 = 21;
const KEY_U: u32 = 22;
const KEY_I: u32 = 23;
const KEY_O: u32 = 24;
const KEY_P: u32 = 25;
const KEY_LEFTBRACE: u32 = 26;
const KEY_RIGHTBRACE: u32 = 27;
const KEY_ENTER: u32 = 28;
const KEY_LEFTCTRL: u32 = 29;
const KEY_A: u32 = 30;
const KEY_S: u32 = 31;
const KEY_D: u32 = 32;
const KEY_F: u32 = 33;
const KEY_G: u32 = 34;
const KEY_H: u32 = 35;
const KEY_J: u32 = 36;
const KEY_K: u32 = 37;
const KEY_L: u32 = 38;
const KEY_SEMICOLON: u32 = 39;
const KEY_APOSTROPHE: u32 = 40;
const KEY_GRAVE: u32 = 41;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_BACKSLASH: u32 = 43;
const KEY_Z: u32 = 44;
const KEY_X: u32 = 45;
const KEY_C: u32 = 46;
const KEY_V: u32 = 47;
const KEY_B: u32 = 48;
const KEY_N: u32 = 49;
const KEY_M: u32 = 50;
const KEY_COMMA: u32 = 51;
const KEY_DOT: u32 = 52;
const KEY_SLASH: u32 = 53;
const KEY_RIGHTSHIFT: u32 = 54;
const KEY_KPASTERISK: u32 = 55;
const KEY_LEFTALT: u32 = 56;
const KEY_SPACE: u32 = 57;
const KEY_CAPSLOCK: u32 = 58;
const KEY_F1: u32 = 59;
const KEY_F2: u32 = 60;
const KEY_F3: u32 = 61;
const KEY_F4: u32 = 62;
const KEY_F5: u32 = 63;
const KEY_F6: u32 = 64;
const KEY_F7: u32 = 65;
const KEY_F8: u32 = 66;
const KEY_F9: u32 = 67;
const KEY_F10: u32 = 68;
const KEY_NUMLOCK: u32 = 69;
const KEY_SCROLLLOCK: u32 = 70;
const KEY_KP7: u32 = 71;
const KEY_KP8: u32 = 72;
const KEY_KP9: u32 = 73;
const KEY_KPMINUS: u32 = 74;
const KEY_KP4: u32 = 75;
const KEY_KP5: u32 = 76;
const KEY_KP6: u32 = 77;
const KEY_KPPLUS: u32 = 78;
const KEY_KP1: u32 = 79;
const KEY_KP2: u32 = 80;
const KEY_KP3: u32 = 81;
const KEY_KP0: u32 = 82;
const KEY_KPDOT: u32 = 83;

// const KEY_ZENKAKUHANKAKU: u32 = 85;
const KEY_102ND: u32 = 86;
const KEY_F11: u32 = 87;
const KEY_F12: u32 = 88;
// const KEY_RO: u32 = 89;
// const KEY_KATAKANA: u32 = 90;
// const KEY_HIRAGANA: u32 = 91;
// const KEY_HENKAN: u32 = 92;
// const KEY_KATAKANAHIRAGANA: u32 = 93;
// const KEY_MUHENKAN: u32 = 94;
// const KEY_KPJPCOMMA: u32 = 95;
const KEY_KPENTER: u32 = 96;
const KEY_RIGHTCTRL: u32 = 97;
const KEY_KPSLASH: u32 = 98;
const KEY_SYSRQ: u32 = 99;
const KEY_RIGHTALT: u32 = 100;
// const KEY_LINEFEED: u32 = 101;
const KEY_HOME: u32 = 102;
const KEY_UP: u32 = 103;
const KEY_PAGEUP: u32 = 104;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_END: u32 = 107;
const KEY_DOWN: u32 = 108;
const KEY_PAGEDOWN: u32 = 109;
const KEY_INSERT: u32 = 110;
const KEY_DELETE: u32 = 111;
// const KEY_MACRO: u32 = 112;
// const KEY_MUTE: u32 = 113;
// const KEY_VOLUMEDOWN: u32 = 114;
// const KEY_VOLUMEUP: u32 = 115;
// const KEY_POWER: u32 = 116	/* SC System Power Down */;
const KEY_KPEQUAL: u32 = 117;
// const KEY_KPPLUSMINUS: u32 = 118;
const KEY_PAUSE: u32 = 119;
// const KEY_SCALE: u32 = 120	/* AL Compiz Scale (Expose) */;

const KEY_KPCOMMA: u32 = 121;
// const KEY_HANGEUL: u32 = 122;
// const KEY_HANGUEL: u32 = KEY_HANGEUL;
// const KEY_HANJA: u32 = 123;
// const KEY_YEN: u32 = 124;
const KEY_LEFTMETA: u32 = 125;
const KEY_RIGHTMETA: u32 = 126;
const KEY_COMPOSE: u32 = 127;

fn key_convert(code: u32) -> Option<&'static str> {
    Some(match code {
        KEY_LEFTSHIFT => "shift",
        KEY_RIGHTSHIFT => "shift_r",
        KEY_LEFTALT => "alt",
        KEY_RIGHTALT => "alt_r",
        KEY_LEFTCTRL => "ctrl",
        KEY_RIGHTCTRL => "ctrl_r",
        KEY_COMPOSE => "menu", // NOTE
        KEY_ESC => "esc",
        KEY_0 => "0",
        KEY_1 => "1",
        KEY_2 => "2",
        KEY_3 => "3",
        KEY_4 => "4",
        KEY_5 => "5",
        KEY_6 => "6",
        KEY_7 => "7",
        KEY_8 => "8",
        KEY_9 => "9",
        KEY_MINUS => "minus",
        KEY_EQUAL => "equal",
        KEY_BACKSPACE => "backspace",
        KEY_TAB => "tab",
        KEY_A => "a",
        KEY_B => "b",
        KEY_C => "c",
        KEY_D => "d",
        KEY_E => "e",
        KEY_F => "f",
        KEY_G => "g",
        KEY_H => "h",
        KEY_I => "i",
        KEY_J => "j",
        KEY_K => "k",
        KEY_L => "l",
        KEY_M => "m",
        KEY_N => "n",
        KEY_O => "o",
        KEY_P => "p",
        KEY_Q => "q",
        KEY_R => "r",
        KEY_S => "s",
        KEY_T => "t",
        KEY_U => "u",
        KEY_V => "v",
        KEY_W => "w",
        KEY_X => "x",
        KEY_Y => "y",
        KEY_Z => "z",
        KEY_ENTER => "ret",
        KEY_SEMICOLON => "semicolon",
        KEY_APOSTROPHE => "apostrophe",
        KEY_GRAVE => "grave_accent",
        KEY_BACKSLASH => "backslash",
        KEY_COMMA => "comma",
        KEY_DOT => "dot",
        KEY_SLASH => "slash",
        KEY_SPACE => "spc",
        KEY_CAPSLOCK => "caps_lock",
        KEY_F1 => "f1",
        KEY_F2 => "f2",
        KEY_F3 => "f3",
        KEY_F4 => "f4",
        KEY_F5 => "f5",
        KEY_F6 => "f6",
        KEY_F7 => "f7",
        KEY_F8 => "f8",
        KEY_F9 => "f9",
        KEY_F10 => "f10",
        KEY_F11 => "f11",
        KEY_F12 => "f12",
        KEY_NUMLOCK => "num_lock",
        KEY_SCROLLLOCK => "scroll_lock",
        KEY_KPSLASH => "kp_divide",
        KEY_KPASTERISK => "kp_multiply",
        KEY_KPMINUS => "kp_subtract",
        KEY_KPPLUS => "kp_add",
        KEY_KPENTER => "kp_enter",
        KEY_KPDOT => "kp_decimal",
        KEY_SYSRQ => "sysrq",
        KEY_KP0 => "kp_0",
        KEY_KP1 => "kp_1",
        KEY_KP2 => "kp_2",
        KEY_KP3 => "kp_3",
        KEY_KP4 => "kp_4",
        KEY_KP5 => "kp_5",
        KEY_KP6 => "kp_6",
        KEY_KP7 => "kp_7",
        KEY_KP8 => "kp_8",
        KEY_KP9 => "kp_9",
        KEY_102ND => "less",
        KEY_HOME => "home",
        KEY_PAGEUP => "pgup",
        KEY_PAGEDOWN => "pgdn",
        KEY_END => "end",
        KEY_LEFT => "left",
        KEY_UP => "up",
        KEY_DOWN => "down",
        KEY_RIGHT => "right",
        KEY_INSERT => "insert",
        KEY_DELETE => "delete",
        KEY_PAUSE => "pause",
        KEY_KPCOMMA => "kp_comma",
        KEY_KPEQUAL => "kp_equals",
        KEY_LEFTMETA => "meta_l",
        KEY_RIGHTMETA => "meta_r",
        // KEY_POWER => "power",
        KEY_LEFTBRACE => "bracket_left",
        KEY_RIGHTBRACE => "bracket_right",
        _ => return None,
    })
}
