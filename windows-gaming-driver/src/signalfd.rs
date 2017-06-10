use std::os::unix::io::AsRawFd;
use nix::sys::signalfd::{SigSet, SignalFd as Underlying, SFD_CLOEXEC, SFD_NONBLOCK};
pub use nix::sys::signalfd::siginfo;
pub use nix::sys::signal;
use my_io::MyIo;
use futures::{Async, Poll, Stream};
use tokio_core::reactor::{Handle, PollEvented};

pub struct SignalFd {
    underlying: Underlying,
    poll_evented: PollEvented<MyIo>,
}

impl SignalFd {
    pub fn new<I: IntoIterator<Item = signal::Signal>>(iter: I, handle: &Handle) -> SignalFd {
        let mut sigset = SigSet::empty();
        for sig in iter {
            sigset.add(sig);
        }
        sigset.thread_block().unwrap();
        let underlying = Underlying::with_flags(&sigset, SFD_NONBLOCK | SFD_CLOEXEC).expect("Failed to create signalfd");
        SignalFd {
            poll_evented: PollEvented::new(MyIo { fd: underlying.as_raw_fd() }, handle).unwrap(),
            underlying,
        }
    }
}

impl Stream for SignalFd {
    type Item = siginfo;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<siginfo>, ()> {
        if let Async::NotReady = self.poll_evented.poll_read() {
            return Ok(Async::NotReady);
        }

        if let Some(sig) = self.underlying.read_signal().unwrap() {
            Ok(Async::Ready(Some(sig)))
        } else {
            Ok(Async::NotReady) // wtf we got scammed???
        }
    }
}
