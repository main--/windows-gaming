use std::io;
use std::cell::RefCell;

use tokio_core::reactor::{Handle, PollEvented};
use futures::{Async, Poll, Future};
use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver, unbounded};
use input::{Libinput, LibinputInterface, Device};
use input::event::Event;
use libc::{open, close, ioctl, c_char, c_int, c_ulong, c_void};
use libudev::{Result as UdevResult, Context, Enumerator};

use super::my_io::MyIo;
use config::{UsbDevice, UsbBinding, UsbPort, UsbId};

const EVIOCGRAB: c_ulong = 1074021776;

unsafe extern "C" fn do_open(path: *const c_char, mode: c_int, _: *mut c_void) -> c_int {
    let fd = open(path, mode);
    ioctl(fd, EVIOCGRAB, 1);
    fd
}

unsafe extern "C" fn do_close(fd: c_int, _: *mut c_void) {
    ioctl(fd, EVIOCGRAB, 0);
    close(fd);
}

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
        let (send, recv) = unbounded();
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
            p.sender.send(e).unwrap();
        }

        p.io.need_read();
        Ok(Async::NotReady)
    }
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
