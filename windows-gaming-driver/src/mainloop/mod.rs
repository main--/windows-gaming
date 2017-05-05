use std::io::prelude::*;
use std::os::unix::prelude::*;

use std::io::Result as IoResult;
use std::io::ErrorKind;

use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener, UnixStream};

use controller::Controller;
use config::Config;

pub mod clientpipe_handler;
pub mod monitor_handler;
pub mod control_handler;
pub mod catch_sigterm;
pub mod pinger;

pub type ControllerRef = Rc<RefCell<Controller>>;

pub enum PollableResult {
    Child(Box<Pollable>),
    Ok,
    Death,
}

pub trait Pollable {
    fn fd(&self) -> RawFd;
    fn run(&mut self) -> PollableResult;
    fn is_critical(&self) -> bool {
        false // eventloop will run until all critical ones are gone
    }
}

pub fn read_byte<T: Read>(thing: &mut T) -> IoResult<Option<u8>> {
    let mut buf = [0u8; 1];
    match thing.read(&mut buf) {
        Ok(0) => Ok(None),
        Ok(1) => Ok(Some(buf[0])),
        Ok(_) => unreachable!(),
        Err(ref e) if e.kind() == ErrorKind::ConnectionReset => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn drain_stdout<T: Read>(thing: &mut T) -> PollableResult {
    let mut buf = [0u8; 4096];
    let count = thing.read(&mut buf).expect("read failed");
    print!("{}", String::from_utf8_lossy(&buf[0..count]));

    if count == 0 {
        PollableResult::Death
    } else {
        PollableResult::Ok
    }
}



fn poll_core<'a>(mut components: Vec<Box<Pollable>>) {
    while components.iter().any(|x| x.is_critical()) {
        use nix::poll::*;

        let mut deathlist = Vec::new();
        let mut newchildren = Vec::new();
        {

            let mut pollfds: Vec<_> = components.iter()
                .map(|x| PollFd::new(x.fd(), POLLIN, EventFlags::empty()))
                .collect();
            let poll_count = poll(&mut pollfds, -1).expect("poll failed");
            assert!(poll_count > 0);


            for (idx, (pollable, pollfd)) in components.iter_mut().zip(pollfds).enumerate() {
                let mut bits = pollfd.revents().unwrap();
                if bits.intersects(POLLIN) {
                    match pollable.run() {
                        PollableResult::Death => deathlist.push(idx),
                        PollableResult::Child(c) => newchildren.push(c),
                        PollableResult::Ok => (),
                    }
                }
                bits.remove(POLLIN);
                // assert!(bits.is_empty());
            }
        }

        // remove in reverse order so we don't mess up each subsequent index
        for &i in deathlist.iter().rev() {
            components.remove(i);
        }

        for c in newchildren {
            components.push(c);
        }

    }
}

pub fn run(cfg: &Config, monitor_stream: UnixStream, clientpipe_stream: UnixStream, control_socket: UnixListener) {
    let ctrl = Controller::new(cfg.machine.usb_devices.clone(), &monitor_stream, &clientpipe_stream);
    let ctrl = Rc::new(RefCell::new(ctrl));

    poll_core(vec![
        Box::new(monitor_handler::MonitorHandler::new(monitor_stream)),
        Box::new(clientpipe_handler::ClientpipeHandler::new(ctrl.clone(), clientpipe_stream)),
        Box::new(control_handler::ControlServerHandler::new(ctrl.clone(), control_socket)),
        Box::new(catch_sigterm::CatchSigterm::new(ctrl.clone())),
    ]);
}
