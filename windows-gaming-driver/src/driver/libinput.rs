use std::io;
use std::collections::VecDeque;
use std::cell::RefCell;

use tokio_core::reactor::{Handle, PollEvented};
use futures::{Async, Poll, Future, Stream};
use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver, unbounded};
use input::{Libinput, LibinputInterface};
use input::event::Event;
use input::event::pointer::PointerEvent;
use libc::{open, close, ioctl, c_char, c_int, c_ulong, c_void};
use mio::Ready;

use super::my_io::MyIo;

const EVIOCGRAB: c_ulong = 1074021776;

unsafe extern "C" fn do_open(x: *const c_char, y: c_int, z: *mut c_void) -> c_int {
    let fd = open(x, y);
    ioctl(fd, EVIOCGRAB, 1);
    fd
}

unsafe extern "C" fn do_close(fd: c_int, y: *mut c_void) {
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
