use std::io;
use std::str;
use bytes::{IntoBuf, Buf, BufMut, BytesMut, LittleEndian};
use tokio_io::codec::{Encoder, Decoder};

pub enum GaCmdOut {
    Ping,
    RegisterHotKey {
        id: u32,
        key: String,
    },
    ReleaseModifiers,
    Suspend,
}

pub enum GaCmdIn {
    ReportBoot,
    Suspending,
    Pong,
    HotKey(u32),
    HotKeyBindingFailed(String),
}

pub struct Codec;

impl Decoder for Codec {
    type Item = GaCmdIn;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<GaCmdIn>> {
        let mut size = 1;
        let ret = match buf.get(0) {
            Some(&1) => GaCmdIn::ReportBoot,
            Some(&3) => GaCmdIn::Suspending,
            Some(&4) => GaCmdIn::Pong,
            Some(&5) if buf.len() >= 5 => {
                let mut buf = (&*buf).into_buf();
                buf.advance(1); // skip cmd
                let id = buf.get_u32::<LittleEndian>();
                size += 4;
                GaCmdIn::HotKey(id)
            }
            Some(&6) if buf.len() >= 5 => {
                let mut bbuf = (&*buf).into_buf();
                bbuf.advance(1); // skip cmd
                let len = bbuf.get_u32::<LittleEndian>() as usize;
                if buf.len() < len + 5 {
                    return Ok(None);
                }
                let s = String::from_utf8_lossy(&buf[5..5+len]).into_owned();
                size += 4 + len;
                GaCmdIn::HotKeyBindingFailed(s)
            }
            Some(x) => {
                warn!("client sent invalid request {}", x);
                // no idea how to proceed as the request might have payload
                // this essentially just hangs the connection forever
                return Ok(None);
            }
            _ => return Ok(None),
        };
        buf.split_to(size);
        Ok(Some(ret))
    }
}

impl Encoder for Codec {
    type Item = GaCmdOut;
    type Error = io::Error;

    fn encode(&mut self, cmd: GaCmdOut, buf: &mut BytesMut) -> io::Result<()> {
        buf.reserve(1);
        match cmd {
            GaCmdOut::Ping => buf.put_u8(0x01),
            GaCmdOut::RegisterHotKey { id, key } => {
                buf.put_u8(0x02);
                buf.reserve(4 + 4 + key.len());
                buf.put_u32::<LittleEndian>(id);
                buf.put_u32::<LittleEndian>(key.len() as u32);
                buf.extend(key.bytes());
            }
            GaCmdOut::ReleaseModifiers => buf.put_u8(0x03),
            GaCmdOut::Suspend => buf.put_u8(0x04),
        }
        Ok(())
    }
}
