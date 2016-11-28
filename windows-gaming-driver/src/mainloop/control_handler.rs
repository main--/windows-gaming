use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::io::{AsRawFd, RawFd};

use mainloop::*;

pub struct ControlServerHandler {
    monitor: MonitorRef,
    clientpipe: Rc<RefCell<UnixStream>>,
    controlserver: UnixListener,
}

impl ControlServerHandler {
    pub fn new(monitor: MonitorRef, clientpipe: Rc<RefCell<UnixStream>>, controlserver: UnixListener) -> ControlServerHandler {
        ControlServerHandler {
            monitor: monitor,
            clientpipe: clientpipe,
            controlserver: controlserver,
        }
    }
}

impl Pollable for ControlServerHandler {
    fn fd(&self) -> RawFd {
        self.controlserver.as_raw_fd()
    }

    fn run(&mut self) -> PollableResult {
        let (client, _) = self.controlserver.accept().unwrap();
        PollableResult::Child(Box::new(ControlClientHandler {
            monitor: self.monitor.clone(),
            clientpipe: self.clientpipe.clone(),
            client: client,
        }))
    }
}

struct ControlClientHandler {
    monitor: Rc<RefCell<monitor_manager::MonitorManager>>,
    #[allow(dead_code)]
    clientpipe: Rc<RefCell<UnixStream>>,
    client: UnixStream,
}

impl Pollable for ControlClientHandler {
    fn fd(&self) -> RawFd {
        self.client.as_raw_fd()
    }
    fn run(&mut self) -> PollableResult {
        match read_byte(&mut self.client).expect("control channel read failed") {
            Some(1) => {
                println!("IO entry requested!");
                self.monitor.borrow_mut().set_io_attached(true);
            }
            Some(2) => {
                self.monitor.borrow_mut().shutdown();
            }
            Some(x) => println!("control sent invalid request {}", x),
            None => return PollableResult::Death,
        }
        PollableResult::Ok
    }
}
