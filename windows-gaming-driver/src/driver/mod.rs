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
pub mod hotkeys;

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
    let hotkey_bindings: Vec<_> = cfg.machine.hotkeys.iter().map(|x| x.key.clone()).collect();
    let mut keyboard_state = self::hotkeys::KeyboardState::new(&hotkey_bindings);
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
            Event::Keyboard(KeyboardEvent::Key(k)) => {
                let down = k.key_state() == KeyState::Pressed;
                let (hotkeys, qcode) = match keyboard_state.input_linux(k.key(), down) {
                    Some(x) => x,
                    None => return None,
                };

                for hk in hotkeys {
                    controller.borrow_mut().ga_hotkey(hk as u32);
                }

                QmpCommand::InputSendEvent {
                    events: vec![ InputEvent::Key {
                        down,
                        key: KeyValue::Qcode(qcode),
                    }]
                }
            }
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
