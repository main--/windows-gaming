use std::ffi::OsStr;

use config::UsbBus;

pub fn parse_hex(s: &OsStr) -> Option<u16> {
    let s = match s.to_str() {
        Some(s) => s,
        None => return None
    };
    if s.starts_with("0x") {
        u16::from_str_radix(&s[2..], 16).ok()
    } else {
        u16::from_str_radix(s, 16).ok()
    }
}

pub fn usable_ports(bus: UsbBus) -> usize {
    match bus {
        UsbBus::Ohci => 15,
        UsbBus::Uhci => 2,
        UsbBus::Ehci => 6,
        UsbBus::Xhci => 15,
    }
}
