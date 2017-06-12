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

    macro_rules! please {
        (create a test function named $name:ident, which creates and passes BytesMut containing $data:expr, asserts the result $expected:expr, and a remaining length of $len:expr) => (
            #[test]
            fn $name() {
                let mut bytes = BytesMut::new();
                bytes.extend($data);
                assert_eq!(Codec.decode(&mut bytes).unwrap(), $expected);
                assert_eq!(bytes.len(), $len);
            }
        )
    }

    please!(create a test function named none, which creates and passes BytesMut containing &[], asserts the result None, and a remaining length of 0);
    please!(create a test function named invalid, which creates and passes BytesMut containing &[0], asserts the result None, and a remaining length of 1);
    please!(create a test function named io_entry, which creates and passes BytesMut containing &[1], asserts the result Some(ControlCmdIn::IoEntry), and a remaining length of 0);
    please!(create a test function named shutdown, which creates and passes BytesMut containing &[2], asserts the result Some(ControlCmdIn::Shutdown), and a remaining length of 0);
    please!(create a test function named force_io_entry, which creates and passes BytesMut containing &[3], asserts the result Some(ControlCmdIn::ForceIoEntry), and a remaining length of 0);
    please!(create a test function named io_exit, which creates and passes BytesMut containing &[4], asserts the result Some(ControlCmdIn::IoExit), and a remaining length of 0);
    please!(create a test function named suspend, which creates and passes BytesMut containing &[5], asserts the result Some(ControlCmdIn::Suspend), and a remaining length of 0);

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
