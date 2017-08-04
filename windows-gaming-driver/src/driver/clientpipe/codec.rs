extern crate clientpipe_proto as proto;

use std::io;
use bytes::{IntoBuf, Buf, BytesMut};
use tokio_io::codec::{Encoder, Decoder};
use prost::{encoding, Message};

pub use self::proto::RegisterHotKey;
pub use self::proto::ga_cmd_in::Message as GaCmdIn;
pub use self::proto::ga_cmd_out::Message as GaCmdOut;
pub use self::proto::clipboard_message::Message as ClipboardMessage;
use self::proto::Unit;

/// Unit because protobuf is weird
pub const O: Unit = Unit {};



pub struct Codec;

impl Decoder for Codec {
    type Item = GaCmdIn;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<GaCmdIn>> {
        let mut res = Ok(None);
        let mut consumed = 0;
        {
            let mut rbuf = (&*buf).into_buf();
            // prost's length delimiting functions are astonishingly useless because they return io::Error
            if let Ok(len) = encoding::decode_varint(&mut rbuf) {
                if rbuf.remaining() as u64 >= len {
                    consumed = len as usize + encoding::encoded_len_varint(len);
                    // Even if it's an error don't return early, as we want to
                    // skip over the message.
                    res = proto::GaCmdIn::decode(&mut rbuf.take(len as usize))
                        .map(|x| x.message);
                }
            }
        }
        if let Err(e) = res.as_ref() {
            warn!("Unknown / invalid request ({}), skipping over: {:?}", e, &buf[..consumed]);
        }
        buf.split_to(consumed);
        res
    }
}

impl Encoder for Codec {
    type Item = GaCmdOut;
    type Error = io::Error;

    fn encode(&mut self, cmd: GaCmdOut, buf: &mut BytesMut) -> io::Result<()> {
        let cmd = proto::GaCmdOut { message: Some(cmd) };

        let len = cmd.encoded_len();
        buf.reserve(len + encoding::encoded_len_varint(len as u64));
        cmd.encode_length_delimited(buf)
    }
}
