use std::ffi::CString;
use std::io;
use std::iter;
use std::os::unix::prelude::{AsRawFd, OsStrExt};
use std::rc::Rc;
use std::cell::RefCell;
use std::borrow::Cow;
use std::os::unix::io::RawFd;

use futures::{Future, Stream, Sink};
use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver, self};
use input::{Libinput, LibinputInterface, Device, AccelProfile};
use input::event::{Event, KeyboardEvent, PointerEvent};
use input::event::pointer::{Axis, ButtonState, PointerScrollEvent};
use input::event::keyboard::{KeyState, KeyboardEventTrait};
use libc::{self, c_char, c_int, c_ulong, pollfd};
use libudev::{Result as UdevResult, Context, Enumerator};

use common::config::{UsbBinding, UsbPort, UsbId, MachineConfig};
use crate::controller::Controller;
use common::hotkeys::{KeyboardState, KeyResolution, KeyBinding};
use crate::monitor::{QmpCommand, InputEvent, InputButton, KeyValue};
use tokio::io::unix::AsyncFd;

const EVIOCGRAB: c_ulong = 1074021776;

unsafe extern "C" fn do_open(path: *const c_char, mode: c_int) -> c_int {
    let fd = libc::open(path, mode);
    grab_fd(fd, true);
    fd
}

unsafe extern "C" fn do_close(fd: c_int) {
    grab_fd(fd, false);
    libc::close(fd);
}

unsafe fn grab_fd(fd: RawFd, grap: bool) {
    let res = libc::ioctl(fd, EVIOCGRAB, grap as usize);
    if res < 0 {
        error!("Could not exclusively grap input device");
    }
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;

pub struct Input {
    machine: MachineConfig,
    li: Libinput,
    device_handles: Vec<Device>,
    io: AsyncFd<RawFd>,
    sender: UnboundedSender<Event>,
}

// could do advanced things here to avoid the need for udev rules that give us permissions to
// the respective input devices
// however, you need udev rules for full entry anyway, sooo...
struct DumbLibinputInterface;
impl LibinputInterface for DumbLibinputInterface {
    fn open_restricted(&mut self, path: &std::path::Path, flags: i32) -> Result<RawFd, i32> {
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        unsafe {
            let fd = do_open(path.as_ptr(), flags);
            if fd < 0 {
                Err(*libc::__errno_location())
            } else {
                Ok(fd)
            }
        }
    }

    fn close_restricted(&mut self, fd: RawFd) {
        unsafe { do_close(fd) }
    }
}

impl Input {
    pub fn new(machine: MachineConfig) -> (Input, UnboundedReceiver<Event>) {
        let li = Libinput::new_from_path(DumbLibinputInterface);
        let (send, recv) = mpsc::unbounded();
        (Input {
            machine,
            io: AsyncFd::new(li.as_raw_fd()).unwrap(),
            li,
            device_handles: Vec::new(),
            sender: send,
        }, recv)
    }

    pub fn suspend(&mut self) {
        for handle in self.device_handles.drain(..) {
            self.li.path_remove_device(handle);
        }
    }

    pub fn resume(&mut self) {
        self.resume_inner().expect("Failed to open libinput devices");
    }

    pub fn resume_inner(&mut self) -> UdevResult<()> {
        let ctx = Context::new()?;

        for dev in self.machine.usb_devices.iter().filter(|x| !x.permanent) {
            // FIXME this is copy-pasta from controller's udev resolver
            let mut iter = Enumerator::new(&ctx).unwrap();

            iter.match_subsystem("usb")?;
            iter.match_property("DEVTYPE", "usb_device")?;

            match dev.binding {
                UsbBinding::ById(UsbId { vendor, product }) => {
                    iter.match_attribute("idVendor", format!("{:04x}", vendor))?;
                    iter.match_attribute("idProduct", format!("{:04x}", product))?;
                }
                UsbBinding::ByPort(UsbPort { bus, ref port }) => {
                    iter.match_attribute("busnum", bus.to_string())?;
                    iter.match_attribute("devpath", port.to_string())?;
                }
            }

            for usbdev in iter.scan_devices()? {
                trace!("usbdev {:?} {:?}", dev, usbdev.sysname());

                let mut iter = Enumerator::new(&ctx).unwrap();
                iter.match_subsystem("input")?;
                iter.match_parent(&usbdev)?;
                iter.match_property("LIBINPUT_DEVICE_GROUP", "?*")?;
                for dev in iter.scan_devices()? {
                    trace!("input {:?} {:?}", dev.sysname(), dev.devnode());

                    let dev_node = dev.devnode().expect("libinput device is missing a devnode");
                    let handle  = self.li.path_add_device(dev_node.as_os_str().to_str().unwrap()); // FIXME utf8???
                    match handle {
                        Some(mut h) => {
                            // only set acceleration for devices which support it
                            if h.config_accel_profiles().contains(&AccelProfile::Flat) {
                                h.config_accel_set_profile(AccelProfile::Flat)
                                    .expect("Error setting acceleration profile to flat");
                                h.config_accel_set_speed(self.machine.light_mouse_speed)
                                    .expect("Error setting acceleration speed");
                            }
                            self.device_handles.push(h);
                        }
                        None => error!("Failed to open libinput device ({:?})!", dev.syspath()),
                    }
                }
            }
        }

        Ok(())
    }
}

pub struct InputListener<'a>(pub &'a RefCell<Input>);

// while we could just have a stream here, libinput gets mad if we process their events too slowly
// hence, we buffer through an UnboundedSender and have this future which, whenever the fd is ready,
// lets libinput do its thing and sends the events to our channel for deferred processing
// it will (basically) never complete
impl<'a> futures03::Future for InputListener<'a> {
    type Output = Result<(), io::Error>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let mut p = self.0.borrow_mut();
        let p = &mut *p;
        loop {
            match p.io.poll_read_ready(cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(guard) => {
                    let mut guard = guard?;

                    p.li.dispatch()?;

                    // recheck to figure out whether libinput lied to us
                    let mut fd = pollfd { fd: p.li.as_raw_fd(), events: libc::POLLIN, revents: 0  };
                    let is_really_finished = unsafe { libc::poll(&mut fd, 1, 0) } == 0;
                    if is_really_finished {
                        guard.clear_ready();
                    }

                    // lmao what a quality api
                    while let Some(e) = p.li.next() {
                        (&p.sender).unbounded_send(e).unwrap();
                    }
                }
            }
        }
    }
}

pub fn create_handler<'a>(input_events: UnboundedReceiver<Event>, hotkey_bindings: &'a [KeyBinding],
                          controller: Rc<RefCell<Controller>>, monitor_sender: UnboundedSender<QmpCommand>)
            -> Box<dyn Future<Item = (), Error = io::Error> + 'a> {
    let mut keyboard_state = KeyboardState::new(hotkey_bindings);
    let input_handler = input_events.filter_map(move |event| {
        Some(match event {
            Event::Pointer(PointerEvent::Motion(m)) =>
                QmpCommand::InputSendEvent {
                    events: Cow::from(vec![
                        InputEvent::Rel { axis: "x", value: m.dx() as i64 },
                        InputEvent::Rel { axis: "y", value: m.dy() as i64 },
                    ])
                },
            Event::Pointer(PointerEvent::Button(b)) =>
                QmpCommand::InputSendEvent {
                    events: Cow::from(vec![InputEvent::Btn {
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
                    }])
                },
            Event::Pointer(PointerEvent::ScrollWheel(ref e)) if e.has_axis(Axis::Vertical) => {
                let steps = (e.scroll_value_v120(Axis::Vertical) / 120.) as i32;
                if steps == 0 {
                    // stop event, ignore
                    return None;
                }

                let direction = if steps > 0 {
                    InputButton::WheelDown
                } else {
                    InputButton::WheelUp
                };

                let events: Vec<_> = iter::repeat(direction).take(steps.abs() as usize).flat_map(|b| vec![
                    InputEvent::Btn { down: true, button: b },
                    InputEvent::Btn { down: false, button: b },
                ]).collect();

                QmpCommand::InputSendEvent { events: Cow::from(events) }
            },
            #[allow(deprecated)] // we're specifically ignoring this because it's deprecated
            Event::Pointer(PointerEvent::Axis(_)) => return None,
            Event::Keyboard(KeyboardEvent::Key(k)) => {
                let down = k.key_state() == KeyState::Pressed;
                let KeyResolution { hotkeys, qcode } = match keyboard_state.input_linux(k.key(), down) {
                    Some(x) => x,
                    None => return None,
                };

                for &hk in &hotkeys {
                    controller.borrow_mut().ga_hotkey(hk as u32);
                }
                if !hotkeys.is_empty() {
                    // If this was an IoExit hotkey, we just released all keys.
                    // To avoid hung keys, do not forward keypresses that trigger hotkeys.
                    return None;
                }

                match qcode {
                    Some(qcode) => QmpCommand::InputSendEvent {
                        events: Cow::from(vec![InputEvent::Key {
                            down,
                            key: KeyValue::Qcode(qcode),
                        }])
                    },
                    None => return None
                }
            }
            event => {
                info!("Unhandled input event {:?}", event);
                return None;
            }
        })
    }).forward(monitor_sender.sink_map_err(|_| ())).then(|_| Ok(()));
    Box::new(input_handler)
}

/*
For key event testing:

pub fn main() {
    use tokio_core::reactor::*;
    use input::event::*;
    use input::event::keyboard::KeyboardEventTrait;
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let (input, input_events) = Input::new(&handle);
    let input = RefCell::new(input);
    let input_listener = InputListener(&input);
    let input_handler = input_events.for_each(|event| {
        match event {
            Event::Keyboard(k) => println!("key {} {:?}", k.key(), k.key_state()),
            Event::Pointer(m) => unreachable!(),
            _ => (),
        }
        Ok(())
    }).then(|_| Ok(()));

    core.run(input_listener.join(input_handler)).unwrap();
}
 */
