use std::io;
use std::cell::RefCell;

use tokio_core::reactor::{Handle, PollEvented};
use futures::{Async, Poll, Future, Stream};
use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver, unbounded};
use input::{Libinput, LibinputInterface};
use input::event::Event;
use libc::{open, close, ioctl, c_char, c_int, c_ulong, c_void};

use super::my_io::MyIo;

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
    li: Libinput,
    io: PollEvented<MyIo>,
    sender: UnboundedSender<Event>,
}

impl Input {
    pub fn new(handle: &Handle) -> (Input, UnboundedReceiver<Event>) {
        let mut li = Libinput::new_from_path(LibinputInterface {
            open_restricted: Some(do_open),
            close_restricted: Some(do_close),
        }, Some(()));
        li.path_add_device("/dev/input/by-id/usb-Logitech_USB_Receiver-if01-event-mouse").unwrap();
        li.path_add_device("/dev/input/by-id/usb-Razer_Razer_BlackWidow_Ultimate_2013-event-kbd").unwrap();
        let (send, recv) = unbounded();
        (Input {
            io: PollEvented::new(MyIo { fd: unsafe { li.fd() } }, handle).unwrap(),
            li,
            sender: send,
        }, recv)
    }

    pub fn suspend(&mut self) {
        self.li.suspend();
    }

    pub fn resume(&mut self) {
        self.li.resume().unwrap();
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
