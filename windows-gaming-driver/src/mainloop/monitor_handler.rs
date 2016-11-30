use std::os::unix::net::UnixStream;
use std::os::unix::io::{AsRawFd, RawFd};

use mainloop::*;

pub struct MonitorHandler {
    stream: UnixStream,
}

impl MonitorHandler {
    pub fn new(stream: UnixStream) -> MonitorHandler {
        MonitorHandler {
            stream: stream,
        }
    }
}

impl Pollable for MonitorHandler {
    fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    fn run(&mut self) -> PollableResult {
        drain_stdout(&mut self.stream)
    }

    fn is_critical(&self) -> bool {
        true
    }
}
