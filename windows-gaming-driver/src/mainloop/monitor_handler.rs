use mio_uds::UnixStream;

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
    fn evented(&self) -> &::mio::Evented {
        &self.stream
    }

    fn run(&mut self) -> PollableResult {
        drain_stdout(&mut self.stream)
    }

    fn is_critical(&self) -> bool {
        true
    }
}
