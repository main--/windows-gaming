use std::io;
use std::iter;
use std::rc::Rc;
use std::cell::RefCell;

use tokio_core::reactor::{Handle, PollEvented};
use futures::{Async, Poll, Future, Stream, Sink};
use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver, self};
use input::{Libinput, LibinputInterface, Device};
use input::event::{Event, KeyboardEvent, PointerEvent};
use input::event::pointer::{Axis, ButtonState};
use input::event::keyboard::{KeyState, KeyboardEventTrait};
use libc::{self, c_char, c_int, c_ulong, c_void};
use libudev::{Result as UdevResult, Context, Enumerator};

use super::my_io::MyIo;
use config::{UsbDevice, UsbBinding, UsbPort, UsbId};
use driver::controller::Controller;
use driver::hotkeys::{KeyboardState, KeyResolution};
use driver::monitor::{QmpCommand, InputEvent, InputButton, KeyValue};
use driver::hotkeys::KeyBinding;

const EVIOCGRAB: c_ulong = 1074021776;

unsafe extern "C" fn do_open(path: *const c_char, mode: c_int, _: *mut c_void) -> c_int {
    let fd = libc::open(path, mode);
    libc::ioctl(fd, EVIOCGRAB, 1);
    fd
}

unsafe extern "C" fn do_close(fd: c_int, _: *mut c_void) {
    libc::ioctl(fd, EVIOCGRAB, 0);
    libc::close(fd);
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;

pub struct Input {
    usb_devs: Vec<UsbDevice>,
    li: Libinput,
    device_handles: Vec<Device>,
    io: PollEvented<MyIo>,
    sender: UnboundedSender<Event>,
}

impl Input {
    pub fn new(handle: &Handle, usb_devs: Vec<UsbDevice>) -> (Input, UnboundedReceiver<Event>) {
        let li = Libinput::new_from_path(LibinputInterface {
            open_restricted: Some(do_open),
            close_restricted: Some(do_close),
        }, Some(()));
        let (send, recv) = mpsc::unbounded();
        (Input {
            io: PollEvented::new(MyIo { fd: unsafe { li.fd() } }, handle).unwrap(),
            li, usb_devs,
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

        for dev in self.usb_devs.iter().filter(|x| !x.permanent) {
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
                        Some(h) => self.device_handles.push(h),
                        None => error!("Failed to open libinput device ({:?})!", dev.syspath()),
                    }
                }
            }
        }

        Ok(())
    }
}

pub struct InputListener<'a>(pub &'a RefCell<Input>);

impl<'a> Future for InputListener<'a> {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        if let Async::NotReady = self.0.borrow().io.poll_read() {
            return Ok(Async::NotReady);
        }

        let mut p = self.0.borrow_mut();
        p.li.dispatch()?;

        // lmao what a quality api
        while let Some(e) = p.li.next() {
            (&p.sender).send(e).unwrap();
        }

        p.io.need_read();
        Ok(Async::NotReady)
    }
}

pub fn create_handler<'a>(input_events: UnboundedReceiver<Event>, hotkey_bindings: &'a [KeyBinding],
                      controller: Rc<RefCell<Controller>>, monitor_sender: UnboundedSender<QmpCommand>)
            -> Box<Future<Item = (), Error = io::Error> + 'a> {
    let mut keyboard_state = KeyboardState::new(hotkey_bindings);
    let input_handler = input_events.filter_map(move |event| {
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
                    events: iter::repeat(direction).take(steps.abs() as usize).flat_map(|b| vec![
                        InputEvent::Btn { down: true, button: b },
                        InputEvent::Btn { down: false, button: b },
                    ]).collect(),
                }
            },
            Event::Keyboard(KeyboardEvent::Key(k)) => {
                let down = k.key_state() == KeyState::Pressed;
                let KeyResolution { hotkeys, qcode } = match keyboard_state.input_linux(k.key(), down) {
                    Some(x) => x,
                    None => return None,
                };

                for hk in hotkeys {
                    controller.borrow_mut().ga_hotkey(hk as u32);
                }

                match qcode {
                    Some(qcode) => QmpCommand::InputSendEvent {
                        events: vec![ InputEvent::Key {
                            down,
                            key: KeyValue::Qcode(qcode),
                        }]
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
