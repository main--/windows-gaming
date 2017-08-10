extern crate clientpipe_proto as proto;

use bytes::{IntoBuf, Buf, BytesMut};
use prost::Message;

pub use self::proto::{RegisterHotKey, ClipboardType, ClipboardTypes, Point};
pub use self::proto::ga_cmd_in::Message as GaCmdIn;
pub use self::proto::ga_cmd_out::Message as GaCmdOut;
pub use self::proto::clipboard_message::Message as ClipboardMessage;

pub fn decode(buf: BytesMut) -> Option<GaCmdIn> {
    let len = buf.len();
    match proto::GaCmdIn::decode(buf.into_buf().take(len)) {
        Ok(msg) => msg.message,
        Err(e) => { error!("Error decoding message, skipping over: {}", e); None },
    }
}

pub fn encode(cmd: GaCmdOut) -> BytesMut {
    let cmd = proto::GaCmdOut { message: Some(cmd) };
    let mut buf = BytesMut::with_capacity(cmd.encoded_len());
    cmd.encode(&mut buf).unwrap();
    buf
}
