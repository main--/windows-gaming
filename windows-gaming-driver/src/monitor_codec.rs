use std::io;
use std::str;
use bytes::BytesMut;
use tokio_io::codec::{Encoder, Decoder};
use serde_json;

#[derive(Serialize)]
#[serde(tag = "execute", content = "arguments", rename_all = "snake_case")]
pub enum QmpCommand {
    QmpCapabilities,
    DeviceAdd {
        driver: &'static str,
        id: String,
        bus: String,
        port: usize,
        hostbus: String,
        hostaddr: String,
    },
    DeviceDel { id: String },
    SystemPowerdown,
    SystemWakeup,
}

pub struct MonitorCodec;

impl Decoder for MonitorCodec {
    type Item = String;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<String>> {
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            // remove the serialized frame from the buffer.
            let line = buf.split_to(i);

            // Also remove the '\n'
            buf.split_to(1);

            // Turn this data into a UTF string and return it in a Frame.
            match str::from_utf8(&line) {
                Ok(s) => Ok(Some(s.to_string())),
                Err(_) => Err(io::Error::new(io::ErrorKind::Other,
                                             "invalid UTF-8")),
            }
        } else {
            Ok(None)
        }
    }
}

impl Encoder for MonitorCodec {
    type Item = QmpCommand;
    type Error = io::Error;

    fn encode(&mut self, cmd: QmpCommand, buf: &mut BytesMut) -> io::Result<()> {
        buf.extend(serde_json::to_string(&cmd).unwrap().bytes());
        Ok(())
    }
}
