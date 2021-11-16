use std::fmt::{Display, Formatter, Result as FmtResult};

use libudev::Device;
use config::{UsbId, UsbPort};
use util;
use hwid;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Binding {
    Id(UsbId),
    Port(UsbPort),
    IdAndPort(UsbId, UsbPort),
}

// TODO: Find out HidKind
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct UsbDevice {
    pub binding: Binding,
    pub names: Option<(String, Option<String>)>,
}

impl UsbDevice {
    pub fn from_id(id: UsbId) -> UsbDevice {
        let names = hwid::hwid_resolve_usb(id.vendor, id.product).ok().unwrap_or(None);
        UsbDevice {
            binding: Binding::Id(id),
            names: names,
        }
    }

    pub fn from_udev_device(dev: Device) -> UsbDevice {
        let vendor = dev.property_value("ID_VENDOR_ID").and_then(util::parse_hex).unwrap();
        let product = dev.property_value("ID_MODEL_ID").and_then(util::parse_hex).unwrap();
        let vendor_name = dev.property_value("ID_VENDOR_FROM_DATABASE")
            .map(|s| s.to_string_lossy().into_owned());
        let product_name = dev.property_value("ID_MODEL_FROM_DATABASE")
            .map(|s| s.to_string_lossy().into_owned());
        let busnum = dev.attribute_value("busnum")
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok())
            .unwrap();
        let devpath = dev.attribute_value("devpath")
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap();
        let id = UsbId { vendor, product };
        let port = UsbPort { bus: busnum, port: devpath };
        UsbDevice {
            binding: Binding::IdAndPort(id, port),
            names: vendor_name.map(|name| (name, product_name)),
        }
    }

    pub fn id(&self) -> Option<UsbId> {
        match self.binding {
            Binding::Id(id) | Binding::IdAndPort(id, _) => Some(id),
            Binding::Port(_) => None
        }
    }

    pub fn port(&self) -> Option<&UsbPort> {
        match self.binding {
            Binding::Port(ref port) | Binding::IdAndPort(_, ref port) => Some(port),
            Binding::Id(_) => None,
        }
    }
}

impl Display for UsbDevice {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self.names {
            None => f.write_str("Unknown vendor Unknown product ")?,
            Some((ref vendor, None)) => write!(f, "{} Unknown product ", vendor)?,
            Some((ref vendor, Some(ref product))) => write!(f, "{} {} ", vendor, product)?
        }
        match self.binding {
            Binding::Id(id) => write!(f, "[{}]", id),
            Binding::Port(ref port) => write!(f, "[Bus {}]", port),
            Binding::IdAndPort(id, ref port) => write!(f, "[{}] [Bus {}]", id, port)
        }
    }
}