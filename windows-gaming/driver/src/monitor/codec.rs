use std::io;
use std::str;
use std::borrow::Cow;
use bytes::BytesMut;
use tokio_util::codec::{Encoder, Decoder};
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
    #[serde(rename = "input-send-event")]
    InputSendEvent {
        events: Cow<'static, [InputEvent]>,
    },
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum InputEvent {
    Rel {
        axis: &'static str,
        value: u32,
    },
    Btn {
        button: InputButton,
        down: bool,
    },
    Key {
        key: KeyValue,
        down: bool,
    },
}

#[derive(Serialize, Clone, Copy)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum KeyValue {
    // Number(u32),
    Qcode(&'static str),
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum InputButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    Side,
    Extra,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Message {
    Init { #[serde(rename = "QMP")] qmp: Qmp },
    Return { #[serde(rename = "return")] ret: Ret },
    Event(Event),
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct Ret {}

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "event")]
pub enum Event {
    #[serde(rename = "POWERDOWN")] Powerdown {
        timestamp: Timestamp,
    },
    #[serde(rename = "SUSPEND")] Suspend {
        timestamp: Timestamp,
    },
    #[serde(rename = "WAKEUP")] Wakeup {
        timestamp: Timestamp,
    },
    #[serde(rename = "RESET")] Reset {
        timestamp: Timestamp,
    },
    #[serde(rename = "DEVICE_DELETED")] DeviceDeleted {
        timestamp: Timestamp,
        data: DeviceDeleted,
    },
    #[serde(rename = "RTC_CHANGE")] RtcChange {
        timestamp: Timestamp,
        data: RtcChange,
    },
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct DeviceDeleted {
    device: String,
    path: String,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct RtcChange {
    offset: i32,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct Timestamp {
    seconds: u64,
    microseconds: u32,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct Qmp {
    version: QmpVersion,
    capabilities: Vec<String>,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct QmpVersion {
    qemu: Version,
    package: String,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
pub struct Version {
    micro: u32,
    minor: u32,
    major: u32,
}

pub struct Codec;

impl Decoder for Codec {
    type Item = Message;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<Message>> {
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            // remove the serialized frame from the buffer.
            let line = buf.split_to(i);

            // Also remove the '\n'
            buf.split_to(1);

            // Turn this data into a UTF string and return it in a Frame.
            match str::from_utf8(&line) {
                Ok(s) => {
                    match serde_json::from_str(&s) {
                        Ok(m) => Ok(m),
                        Err(e) => {
                            warn!("Error deserializing {:?}: {:?}. Ignoring.", s, e);
                            Ok(None)
                        },
                    }
                },
                Err(_) => Err(io::Error::new(io::ErrorKind::Other,
                                             "invalid UTF-8")),
            }
        } else {
            Ok(None)
        }
    }
}

impl Encoder<QmpCommand> for Codec {
    type Error = io::Error;

    fn encode(&mut self, cmd: QmpCommand, buf: &mut BytesMut) -> io::Result<()> {
        buf.extend(serde_json::to_string(&cmd).unwrap().bytes());
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json;

    #[test]
    fn qmp_init() {
        let str = r#"{"QMP": {"version": {"qemu": {"micro": 0, "minor": 9, "major": 2}, "package": ""}, "capabilities": []}}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Init {
            qmp: Qmp {
                version: QmpVersion {
                    qemu: Version { micro: 0, minor: 9, major: 2 },
                    package: "".to_string(),
                },
                capabilities: Vec::new(),
            }
        };
        assert_eq!(ser, expected);
    }

    #[test]
    fn ret() {
        let str = r#"{"return": {}}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Return { ret: Ret {} };
        assert_eq!(ser, expected);
    }

    #[test]
    fn powerdown() {
        let str = r#"{"timestamp": {"seconds": 1497035586, "microseconds": 395911}, "event": "POWERDOWN"}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Event(Event::Powerdown {
            timestamp: Timestamp {
                seconds: 1497035586,
                microseconds: 395911,
            }
        });
        assert_eq!(ser, expected);
    }

    #[test]
    fn suspend() {
        let str = r#"{"timestamp": {"seconds": 1497008371, "microseconds": 653091}, "event": "SUSPEND"}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Event(Event::Suspend {
            timestamp: Timestamp {
                seconds: 1497008371,
                microseconds: 653091,
            }
        });
        assert_eq!(ser, expected);
    }

    #[test]
    fn wakeup() {
        let str = r#"{"timestamp": {"seconds": 1497008392, "microseconds": 419210}, "event": "WAKEUP"}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Event(Event::Wakeup {
            timestamp: Timestamp {
                seconds: 1497008392,
                microseconds: 419210,
            }
        });
        assert_eq!(ser, expected);
    }

    #[test]
    fn reset() {
        let str = r#"{"timestamp": {"seconds": 1497009553, "microseconds": 256981}, "event": "RESET"}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Event(Event::Reset {
            timestamp: Timestamp {
                seconds: 1497009553,
                microseconds: 256981,
            }
        });
        assert_eq!(ser, expected);
    }

    #[test]
    fn device_deleted() {
        let str = r#"{"timestamp": {"seconds": 1497008409, "microseconds": 508154}, "event": "DEVICE_DELETED", "data": {"device": "usb0", "path": "/machine/peripheral/usb0"}}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Event(Event::DeviceDeleted {
            timestamp: Timestamp {
                seconds: 1497008409,
                microseconds: 508154,
            },
            data: DeviceDeleted {
                device: "usb0".to_string(),
                path: "/machine/peripheral/usb0".to_string(),
            },
        });
        assert_eq!(ser, expected);
    }

    #[test]
    fn rtc_offset() {
        let str = r#"{"timestamp": {"seconds": 1497009700, "microseconds": 514}, "event": "RTC_CHANGE", "data": {"offset": -2}}"#;
        let ser: Message = serde_json::from_str(str).unwrap();
        let expected = Message::Event(Event::RtcChange {
            timestamp: Timestamp {
                seconds: 1497009700,
                microseconds: 514,
            },
            data: RtcChange {
                offset: -2,
            },
        });
        assert_eq!(ser, expected);
    }
}
