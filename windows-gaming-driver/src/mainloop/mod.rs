use std::io::prelude::*;

use std::io::Result as IoResult;
use std::io::ErrorKind;

use std::cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};

use mio::Evented;

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
    fn evented(&self) -> &Evented;
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



fn poll_core<'a>(components: Vec<Box<Pollable>>) {
    use mio::*;

    let mut token_alloc = 0;
    let mut next_token = || {
        let ret = token_alloc;
        token_alloc += 1;
        Token(ret)
    };

    let mut components: HashMap<_, _> = components.into_iter().map(|x| (next_token(), x)).collect();

    let poll = Poll::new().unwrap();
    for (&token, component) in &components {
        poll.register(component.evented(), token, Ready::readable(), PollOpt::level()).unwrap();
    }

    let mut events = Events::with_capacity(1024);
    while components.values().any(|x| x.is_critical()) {
        poll.poll(&mut events, None).unwrap();

        for token in events.iter().map(|x| x.token()) {
            match components.get_mut(&token).unwrap().run() {
                PollableResult::Death => {
                    let component = components.remove(&token).unwrap();
                    poll.deregister(component.evented()).unwrap();
                }
                PollableResult::Child(c) => {
                    let token = next_token();
                    poll.register(c.evented(), token, Ready::readable(), PollOpt::level()).unwrap();
                    components.insert(token, c);
                }
                PollableResult::Ok => (),
            }
        }
    }
}

pub fn run(cfg: &Config, monitor_stream: UnixStream, clientpipe_stream: UnixStream, control_socket: UnixListener) {
    use mio_uds::{UnixListener as MioListener, UnixStream as MioStream};

    let ctrl = Controller::new(cfg.machine.clone(), &monitor_stream, &clientpipe_stream);
    let ctrl = Rc::new(RefCell::new(ctrl));

    poll_core(vec![
        Box::new(monitor_handler::MonitorHandler::new(MioStream::from_stream(monitor_stream).unwrap())),
        Box::new(clientpipe_handler::ClientpipeHandler::new(ctrl.clone(), MioStream::from_stream(clientpipe_stream).unwrap())),
        Box::new(control_handler::ControlServerHandler::new(ctrl.clone(), MioListener::from_listener(control_socket).unwrap())),
        Box::new(catch_sigterm::CatchSigterm::new(ctrl.clone())),
    ]);
}
