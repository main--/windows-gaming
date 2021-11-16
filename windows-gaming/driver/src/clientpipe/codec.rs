extern crate clientpipe_proto as proto;

use std::io;
use prost::bytes::{Buf, BytesMut};
use tokio_util::codec::{Encoder, Decoder};
use prost::{encoding, Message};

pub use self::proto::{RegisterHotKey, ClipboardType, ClipboardTypes, Point};
pub use self::proto::ga_cmd_in::Message as GaCmdIn;
pub use self::proto::ga_cmd_out::Message as GaCmdOut;
pub use self::proto::clipboard_message::Message as ClipboardMessage;

pub struct Codec;

impl Decoder for Codec {
    type Item = GaCmdIn;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut bytes::BytesMut) -> io::Result<Option<GaCmdIn>> {
        let mut res = Ok(None);
        let mut consumed = 0;
        {
            let mut rbuf = (&*buf).as_ref();
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

        let skipped = buf.split_to(consumed);
        res.or_else(|e| {
            warn!("Unknown / invalid request ({}), skipping over: {:?}", e, skipped);
            Ok(None)
        })
    }
}

impl Encoder<GaCmdOut> for Codec {
    type Error = io::Error;

    fn encode(&mut self, cmd: GaCmdOut, buf: &mut bytes::BytesMut) -> io::Result<()> {
        let cmd = proto::GaCmdOut { message: Some(cmd) };

        let len = cmd.encoded_len();
        let cap = len + encoding::encoded_len_varint(len as u64);

        let mut tmp = prost::bytes::BytesMut::with_capacity(cap);
        cmd.encode_length_delimited(&mut tmp)?;

        buf.extend_from_slice(tmp.as_ref());
        Ok(())
    }
}
