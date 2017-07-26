use std::io;
use std::str;
use bytes::BytesMut;
use tokio_io::codec::{Encoder, Decoder};

#[derive(Debug, PartialEq, Eq)]
pub enum ControlCmdOut {
    // none yet
}

#[derive(Debug, PartialEq, Eq)]
pub enum ControlCmdIn {
    IoEntry,
    TryIoEntry,
    LightEntry,
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
        let ret = match buf.get(0).cloned() {
            Some(1) => ControlCmdIn::IoEntry,
            Some(2) => ControlCmdIn::Shutdown,
            Some(3) => ControlCmdIn::ForceIoEntry,
            Some(4) => ControlCmdIn::IoExit,
            Some(5) => ControlCmdIn::Suspend,
            Some(6) => ControlCmdIn::TryIoEntry,
            Some(7) => ControlCmdIn::LightEntry,
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

#[cfg(test)]
mod test {
    use super::*;
    use bytes::BytesMut;
    use tokio_io::codec::Decoder;

    fn verify(data: &[u8], expected: Option<ControlCmdIn>, remaining: usize) {
        let mut bytes = BytesMut::new();
        bytes.extend(data);
        assert_eq!(Codec.decode(&mut bytes).unwrap(), expected);
        assert_eq!(bytes.len(), remaining);
    }

    #[test] fn none() { verify(&[], None, 0); }
    #[test] fn invalid() { verify(&[0], None, 1); }
    #[test] fn io_entry() { verify(&[1], Some(ControlCmdIn::IoEntry), 0); }
    #[test] fn shutdown() { verify(&[2], Some(ControlCmdIn::Shutdown), 0); }
    #[test] fn force_io_entry() { verify(&[3], Some(ControlCmdIn::ForceIoEntry), 0); }
    #[test] fn io_exit() { verify(&[4], Some(ControlCmdIn::IoExit), 0); }
    #[test] fn suspend() { verify(&[5], Some(ControlCmdIn::Suspend), 0); }

    #[test]
    fn multiple() {
        let mut bytes = BytesMut::new();
        bytes.extend(&[1,2]);
        assert_eq!(Codec.decode(&mut bytes).unwrap(), Some(ControlCmdIn::IoEntry));
        assert_eq!(bytes.len(), 1);
        assert_eq!(Codec.decode(&mut bytes).unwrap(), Some(ControlCmdIn::Shutdown));
        assert_eq!(bytes.len(), 0);
    }
}
