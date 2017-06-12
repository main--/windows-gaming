use std::io;
use std::str;
use bytes::BytesMut;
use tokio_io::codec::{Encoder, Decoder};

pub enum ControlCmdOut {
    // none yet
}

pub enum ControlCmdIn {
    IoEntry,
    Shutdown,
    ForceIoEntry,
    IoExit,
    Suspend,
}

pub struct Codec;

impl Decoder for Codec {
    type Item = ControlCmdIn;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<ControlCmdIn>> {
        let ret = match buf.get(0) {
            Some(&1) => ControlCmdIn::IoEntry,
            Some(&2) => ControlCmdIn::Shutdown,
            Some(&3) => ControlCmdIn::ForceIoEntry,
            Some(&4) => ControlCmdIn::IoExit,
            Some(&5) => ControlCmdIn::Suspend,
            Some(x) => {
                warn!("control sent invalid request {}", x);
                // no idea how to proceed as the request might have payload
                // this essentially just hangs the connection forever
                return Ok(None);
            }
            None => return Ok(None),
        };
        buf.split_to(1);
        Ok(Some(ret))
    }
}

impl Encoder for Codec {
    type Item = ControlCmdOut;
    type Error = io::Error;

    fn encode(&mut self, cmd: ControlCmdOut, _buf: &mut BytesMut) -> io::Result<()> {
        match cmd {
        }
    }
}
