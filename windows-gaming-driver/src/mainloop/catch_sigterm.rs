use std::os::unix::io::AsRawFd;
use nix::sys::signalfd::{SigSet, SignalFd, SFD_CLOEXEC};
use nix::sys::signal;

use mainloop::*;
use my_io::MyIo;

pub struct CatchSigterm {
    sigfd: SignalFd,
    mio: MyIo,
    controller: ControllerRef,
}

impl CatchSigterm {
    pub fn new(controller: ControllerRef) -> CatchSigterm {
        let mut sigset = SigSet::empty();
        sigset.add(signal::SIGTERM);
        sigset.add(signal::SIGINT);
        sigset.thread_block().unwrap();
        let sigfd = SignalFd::with_flags(&sigset, SFD_CLOEXEC).expect("Failed to create signalfd");
        CatchSigterm {
            mio: MyIo { fd: sigfd.as_raw_fd() },
            sigfd,
            controller: controller,
        }
    }
}

impl Pollable for CatchSigterm {
    fn evented(&self) -> &::mio::Evented {
        &self.mio
    }

    fn run(&mut self) -> PollableResult {
        self.sigfd.read_signal().expect("Failed to read signalfd").unwrap();

        // sigterm/sigint -> shutdown
        self.controller.borrow_mut().shutdown();

        PollableResult::Ok
    }
}
