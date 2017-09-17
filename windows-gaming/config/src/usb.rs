use std::io::{Write, Result as IoResult};

use libudev::{Result as UdevResult, Context, Enumerator};

use common::config::{self, MachineConfig, UsbId, UsbPort, UsbBinding, UsbDevice};
use common::usb_device::{UsbDevice as UsbDeviceInfo, Binding};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidKind {
    Mouse,
    Keyboard,
}

pub const UDEV_RULES_FILE: &'static str = "/etc/udev/rules.d/80-vfio-usb.rules";

pub fn write_udev_rules<W: Write>(usb_devices: &[UsbDevice], mut w: W) -> IoResult<()> {
    // add udev rule to add selected devices to vfio group
    for dev in usb_devices.iter() {
        match dev.binding {
            UsbBinding::ById(UsbId { vendor, product }) =>
                writeln!(w, r#"SUBSYSTEM=="usb", ATTR{{idVendor}}=="{:04x}", ATTR{{idProduct}}=="{:04x}", ACTION=="add", RUN+="/usr/bin/setfacl -m g:vfio:rw- $devnode""#, vendor, product)?,
            UsbBinding::ByPort(UsbPort { bus, ref port }) =>
                writeln!(w, r#"SUBSYSTEM=="usb", ATTR{{busnum}}=="{}", ATTR{{devpath}}=="{}", ACTION=="add", RUN+="/usr/bin/setfacl -m g:vfio:rw- $devnode""#, bus, port)?,
        }
    }
    Ok(())
}

/// Retrieve additional info for all attached devices
pub fn get_devices_info(devices: &[UsbDevice]) -> Vec<Result<UsbDeviceInfo, UsbDevice>> {
    let infos = list_devices(None).expect("Can't read connected USB devices");

    devices.iter().map(|dev| infos.iter().cloned().find(|info| info.id().unwrap() == dev.binding || *info.port().unwrap() == dev.binding).ok_or(dev.clone())).collect()
}

pub fn list_devices(kind: Option<HidKind>) -> UdevResult<Vec<UsbDeviceInfo>> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_subsystem("usb")?;
    let devtype = if kind.is_some() { "usb_interface" } else { "usb_device" };
    iter.match_property("DEVTYPE", devtype)?;

    if let Some(kind) = kind {
        iter.match_attribute("bInterfaceClass", "03")?; // HID
        iter.match_attribute("bInterfaceSubClass", "01")?; // boot interface (?)
        let protocol = match kind {
            HidKind::Mouse => "02",
            HidKind::Keyboard => "01",
        };
        iter.match_attribute("bInterfaceProtocol", protocol)?;
    }

    let mut devs = Vec::new();

    for dev in iter.scan_devices()? {
        let dev = if kind.is_some() {
            dev.parent().unwrap()
        } else {
            dev
        };
        devs.push(UsbDeviceInfo::from_udev_device(dev));
    }
    Ok(devs)
}
