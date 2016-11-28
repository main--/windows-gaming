use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::UnixStream;
use std::os::unix::io::{AsRawFd, RawFd};

use mainloop::*;

pub struct ClientpipeHandler {
    monitor: MonitorRef,
    clientpipe: Rc<RefCell<UnixStream>>,
}


impl ClientpipeHandler {
    pub fn new(monitor: MonitorRef, clientpipe: Rc<RefCell<UnixStream>>) -> ClientpipeHandler {
        ClientpipeHandler {
            monitor: monitor,
            clientpipe: clientpipe,
        }
    }
}

impl Pollable for ClientpipeHandler {
    fn fd(&self) -> RawFd {
        self.clientpipe.borrow().as_raw_fd()
    }
    fn run(&mut self) -> PollableResult {
        match read_byte(&mut *self.clientpipe.borrow_mut()).expect("clientpipe read failed") {
            Some(1) => {
                println!("client is now alive!");
                ::sd_notify::notify_systemd(true, "Ready");
            }
            Some(2) => {
                println!("client requests IO exit");
                self.monitor.borrow_mut().set_io_attached(false);
            }
            Some(x) => println!("client sent invalid request {}", x),
            None => return PollableResult::Death,
        }
        PollableResult::Ok
    }

    fn is_critical(&self) -> bool {
        true
    }
}
