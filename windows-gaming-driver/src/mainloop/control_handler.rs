use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::io::{AsRawFd, RawFd};

use mainloop::*;

pub struct ControlServerHandler {
    controller: ControllerRef,
    controlserver: UnixListener,
}

impl ControlServerHandler {
    pub fn new(controller: ControllerRef, controlserver: UnixListener) -> ControlServerHandler {
        ControlServerHandler {
            controller: controller,
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
            controller: self.controller.clone(),
            client: client,
        }))
    }
}

struct ControlClientHandler {
    controller: ControllerRef,
    client: UnixStream,
}

impl Pollable for ControlClientHandler {
    fn fd(&self) -> RawFd {
        self.client.as_raw_fd()
    }
    fn run(&mut self) -> PollableResult {
        match read_byte(&mut self.client).expect("control channel read failed") {
            Some(1) => {
                info!("IO entry requested!");
                self.controller.borrow_mut().io_attach();
            }
            Some(2) => {
                info!("Shutdown requested");
                self.controller.borrow_mut().shutdown();
            }
            Some(3) => {
                info!("IO entry FORCED!");
                self.controller.borrow_mut().io_force_attach();
            }
            Some(4) => {
                info!("IO exit!");
                self.controller.borrow_mut().io_detach();
            }
            Some(x) => warn!("control sent invalid request {}", x),
            None => return PollableResult::Death,
        }
        PollableResult::Ok
    }
}
