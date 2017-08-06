#[macro_use] extern crate prost_derive;

include!(concat!(env!("OUT_DIR"), "/clientpipe_protocol.rs"));

impl From<clipboard_message::Message> for ga_cmd_out::Message {
    fn from(msg: clipboard_message::Message) -> Self {
        ga_cmd_out::Message::Clipboard(ClipboardMessage { message: Some(msg) })
    }
}

impl From<RegisterHotKey> for ga_cmd_out::Message {
    fn from(msg: RegisterHotKey) -> Self {
        ga_cmd_out::Message::RegisterHotKey(msg)
    }
}
