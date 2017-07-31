use std::io;
use std::str;
use bytes::{BytesMut, BufMut, LittleEndian, IntoBuf, Buf};
use tokio_io::codec::{Encoder, Decoder};

#[derive(Debug, PartialEq, Eq)]
pub enum ControlCmdOut {
    MouseEdged {
        x: i32,
        y: i32,
    },
    TemporaryLightAttach,
    TemporaryLightDetach,
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
    TemporaryLightEntry {
        x: i32,
        y: i32,
    },
}

pub struct Codec;

impl Decoder for Codec {
    type Item = ControlCmdIn;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<ControlCmdIn>> {
        let mut size = 1;
        let ret = match buf.get(0).cloned() {
            Some(1) => ControlCmdIn::IoEntry,
            Some(2) => ControlCmdIn::Shutdown,
            Some(3) => ControlCmdIn::ForceIoEntry,
            Some(4) => ControlCmdIn::IoExit,
            Some(5) => ControlCmdIn::Suspend,
            Some(6) => ControlCmdIn::TryIoEntry,
            Some(7) => ControlCmdIn::LightEntry,
            Some(8) => {
                let mut bbuf = (&*buf).into_buf();
                bbuf.advance(1); // skip cmd
                let x = bbuf.get_i32::<LittleEndian>();
                let y = bbuf.get_i32::<LittleEndian>();
                size += 8;
                ControlCmdIn::TemporaryLightEntry { x, y }
            }
            Some(x) => {
                warn!("control sent invalid request {}", x);
                // no idea how to proceed as the request might have payload
                // this essentially just hangs the connection forever
                return Ok(None);
            }
            None => return Ok(None),
        };
        buf.split_to(size);
        Ok(Some(ret))
    }
}

impl Encoder for Codec {
    type Item = ControlCmdOut;
    type Error = io::Error;

    fn encode(&mut self, cmd: ControlCmdOut, buf: &mut BytesMut) -> io::Result<()> {
        buf.reserve(1);
        match cmd {
            ControlCmdOut::MouseEdged { x, y } => {
                buf.put_u8(1);
                buf.reserve(8);
                buf.put_i32::<LittleEndian>(x);
                buf.put_i32::<LittleEndian>(y);
            }
            ControlCmdOut::TemporaryLightAttach => buf.put_u8(2),
            ControlCmdOut::TemporaryLightDetach => buf.put_u8(3),
        }
        Ok(())
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
