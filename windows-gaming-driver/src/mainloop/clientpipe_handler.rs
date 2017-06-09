use mio_uds::UnixStream;
use std::time::Duration;

use byteorder::{ReadBytesExt, LittleEndian};

use super::pinger::Pinger;
use mainloop::*;

pub struct ClientpipeHandler {
    controller: ControllerRef,
    clientpipe: UnixStream,
}


impl ClientpipeHandler {
    pub fn new(controller: ControllerRef, clientpipe: UnixStream) -> ClientpipeHandler {
        ClientpipeHandler {
            controller: controller,
            clientpipe: clientpipe,
        }
    }
}

impl Pollable for ClientpipeHandler {
    fn evented(&self) -> &::mio::Evented {
        &self.clientpipe
    }

    fn run(&mut self) -> PollableResult {
        match read_byte(&mut self.clientpipe).expect("clientpipe read failed") {
            Some(1) => {
                info!("client is now alive!");
                if self.controller.borrow_mut().ga_hello() {
                    let pinger = Pinger::new(Duration::new(1, 0), self.controller.clone());
                    return PollableResult::Child(Box::new(pinger));
                }
            }
            Some(3) => {
                info!("client says that it's suspending");
                self.controller.borrow_mut().ga_suspending();
            }
            Some(4) => {
                trace!("ga pong'ed");
                self.controller.borrow_mut().ga_pong();
            }
            Some(5) => {
                let id = self.clientpipe.read_u32::<LittleEndian>().expect("clientpipe read failed");
                debug!("hotkey pressed: {}", id);
                self.controller.borrow_mut().ga_hotkey(id);
            }
            Some(6) => {
                let len = self.clientpipe.read_u32::<LittleEndian>().expect("clientpipe read failed");
                let mut vec = vec![0; len as usize];
                self.clientpipe.read_exact(&mut vec).expect("clientpipe read failed");
                let s = String::from_utf8_lossy(&vec);
                warn!("HotKeyBinding failed: {}", s);
            }
            Some(x) => warn!("client sent invalid request {}", x),
            None => return PollableResult::Death,
        }
        PollableResult::Ok
    }

    fn is_critical(&self) -> bool {
        true
    }
}
