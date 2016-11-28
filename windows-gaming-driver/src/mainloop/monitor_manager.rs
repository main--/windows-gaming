use std::cell::RefCell;
use std::rc::Rc;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::os::unix::io::{AsRawFd, RawFd};

use mainloop::*;

pub struct MonitorManager {
    stream: UnixStream,
    io_attached: bool,
}

impl MonitorManager {
    pub fn new(stream: UnixStream) -> MonitorManager {
        MonitorManager {
            stream: stream,
            io_attached: false,
        }
    }

    fn writemon(&mut self, command: &str) {
        writeln!(self.stream, "{}", command).expect("Failed too write to monitor");
    }

    pub fn set_io_attached(&mut self, state: bool) {
        if state != self.io_attached {
            if state {
                // attach
                self.writemon("device_add usb-host,vendorid=0x1532,productid=0x0024,id=mouse");
                self.writemon("device_add usb-host,vendorid=0x1532,productid=0x011a,id=kbd");
            } else {
                // detach
                self.writemon("device_del kbd");
                self.writemon("device_del mouse");
            }
            self.io_attached = state;
        }
    }

    pub fn shutdown(&mut self) {
        self.writemon("system_powerdown");
    }
}

impl Pollable for Rc<RefCell<MonitorManager>> {
    fn fd(&self) -> RawFd {
        self.borrow().stream.as_raw_fd()
    }
    fn run(&mut self) -> PollableResult {
        drain_stdout(&mut self.borrow_mut().stream)
    }
    fn is_critical(&self) -> bool {
        true
    }
}
