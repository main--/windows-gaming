use std::ffi::OsStr;
use std::fmt::Display;
use std::cmp::Ordering;
use std::fmt::{Formatter, Error as FmtError};

use libudev::Device;

pub struct PciDevice<'a> {
    pub dev: Device<'a>,
    pub id: (u16, u16),

    pub vendor: Option<String>,
    pub model: Option<String>,
    pub pci_slot: String,
    pub pci_class: String,
}

impl<'a, 'b> PartialEq<PciDevice<'a>> for PciDevice<'b> {
    fn eq(&self, other: &PciDevice<'a>) -> bool {
        self.id == other.id
    }
}

impl<'a> Eq for PciDevice<'a> {}

impl<'a, 'b> PartialOrd<PciDevice<'a>> for PciDevice<'b> {
    fn partial_cmp(&self, other: &PciDevice<'a>) -> Option<Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl<'a> Ord for PciDevice<'a> {
    fn cmp(&self, other: &PciDevice<'a>) -> Ordering {
        self.id.cmp(&other.id)
    }
}

impl<'a> Display for PciDevice<'a> {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), FmtError> {
        write!(fmt, "{} {} [{:04x}:{:04x}]", self.vendor.as_ref().unwrap_or(&"Unknown vendor".to_owned()), self.model.as_ref().unwrap_or(&"Unknown model".to_owned()), self.id.0, self.id.1)
    }
}

impl<'a> PciDevice<'a> {
    pub fn new(dev: Device<'a>) -> PciDevice<'a> {
        let mut vendor_id = None;
        let mut model_id = None;

        for attr in dev.attributes() {
            if let Some(val) = attr.value().and_then(OsStr::to_str).and_then(parse_hex) {
                if attr.name() == "vendor" {
                    vendor_id = Some(val);
                } else if attr.name() == "device" {
                    model_id = Some(val);
                }
            }
        }

        let mut vendor = None;
        let mut model = None;
        let mut pci_slot = None;
        let mut pci_class = None;
        for prop in dev.properties() {
            if let Some(val) = prop.value().to_str() {
                let val = Some(val.to_owned());
                if prop.name() == "ID_VENDOR_FROM_DATABASE" {
                    vendor = val;
                } else if prop.name() == "ID_MODEL_FROM_DATABASE" {
                    model = val;
                } else if prop.name() == "PCI_SLOT_NAME" {
                    pci_slot = val;
                } else if prop.name() == "PCI_CLASS" {
                    pci_class = val;
                }
            }
        }

        PciDevice {
            dev: dev,
            id: (vendor_id.unwrap(), model_id.unwrap()),

            vendor: vendor,
            model: model,
            pci_slot: pci_slot.unwrap(),
            pci_class: pci_class.unwrap(),
        }
    }

    pub fn pci_device(&self) -> &str {
        &self.pci_slot[.. self.pci_slot.rfind('.').unwrap()]
    }
}

fn parse_hex(s: &str) -> Option<u16> {
    if s.starts_with("0x") {
        u16::from_str_radix(&s[2..], 16).ok()
    } else {
        None
    }
}
