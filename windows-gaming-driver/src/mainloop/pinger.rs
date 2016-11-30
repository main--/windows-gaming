use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use timerfd::{TimerFd, TimerState};

use mainloop::*;

pub struct Pinger {
    timerfd: TimerFd,
    controller: ControllerRef,
}

impl Pinger {
    pub fn new(interval: Duration, controller: ControllerRef) -> Pinger {
        let mut tfd = TimerFd::new().unwrap();
        tfd.set_state(TimerState::Periodic { current: interval, interval: interval });
        Pinger {
            timerfd: tfd,
            controller: controller,
        }
    }
}

impl Pollable for Pinger {
    fn fd(&self) -> RawFd {
        self.timerfd.as_raw_fd()
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
