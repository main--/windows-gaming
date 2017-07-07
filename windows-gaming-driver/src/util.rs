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

pub trait PrettySplit<T>{
	fn into_two(self, &Fn(&T) -> (bool)) -> (Vec<T>, Vec<T>);
}

impl<T> PrettySplit<T> for Vec<T> {
	fn into_two(self, filter: &Fn(&T) -> (bool) ) -> (Vec<T>, Vec<T>){
		let mut matched = Vec::new();
		let mut remaining = Vec::new();
	
		for dev in self {
			if filter(&dev) {
				matched.push(dev);
			}
			else {
				remaining.push(dev);
			}
		}
		(matched, remaining)
	}
}
