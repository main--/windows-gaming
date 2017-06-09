use std::os::unix::io::AsRawFd;
use std::time::Duration;

use timerfd::{TimerFd, TimerState};
use my_io::MyIo;
use mainloop::*;

pub struct Pinger {
    timerfd: TimerFd,
    mio: MyIo,
    controller: ControllerRef,
}

impl Pinger {
    pub fn new(interval: Duration, controller: ControllerRef) -> Pinger {
        let mut tfd = TimerFd::new().unwrap();
        tfd.set_state(TimerState::Periodic { current: interval, interval: interval });
        Pinger {
            mio: MyIo { fd: tfd.as_raw_fd() },
            timerfd: tfd,
            controller: controller,
        }
    }
}

impl Pollable for Pinger {
    fn evented(&self) -> &::mio::Evented {
        &self.mio
    }

    fn run(&mut self) -> PollableResult {
        let ret = self.controller.borrow_mut().ga_ping();

        self.timerfd.read();

        match ret {
            true => PollableResult::Ok,
            false => PollableResult::Death,
        }
    }
}
