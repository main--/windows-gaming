use std::io::Write;

use libudev::{Result, Context, Enumerator};

use util::parse_hex;
use config::{MachineConfig, DeviceId, UsbBinding, UsbDevice};
use hwid;
use setup::ask;
use setup::wizard;

#[derive(Clone, Copy)]
enum HidKind {
    Mouse,
    Keyboard,
}

pub fn select(machine: &mut MachineConfig) -> bool {
    println!("Step 2: Select USB Devices");
    println!();
    if !machine.usb_devices.is_empty() {
        println!("You have currently selected the following usb devices: ");
        for dev in machine.usb_devices.iter() {
            match dev.binding {
                UsbBinding::ById(DeviceId { vendor, product }) => {
                    let name = match hwid::hwid_resolve_usb(vendor, product) {
                        Err(_) | Ok(None)=> "Unknown vendor Unknown product".to_string(),
                        Ok(Some((vendor, None))) => format!("{} Unknown product", vendor),
                        Ok(Some((vendor, Some(product)))) => format!("{} {}", vendor, product)
                    };
                    println!("\t{} [{:04x}:{:04x}]", name, vendor, product);
                }
                UsbBinding::ByPort { bus, port } =>
                    println!("\tBus {} Port {}", bus, port), // TODO: improve this
            }
        }
        if ask::yesno("Do you want to remove them before proceeding?") {
            machine.usb_devices.clear();
            println!("Removed.");
        }
    }
    let mouse = pick(Some(HidKind::Mouse), &machine.usb_devices, true)
        .expect("Failed to select Mouse");
    let keyboard = pick(Some(HidKind::Keyboard), &machine.usb_devices, true)
        .expect("Failed to select Keyboard");
    if let Some(id) = mouse {
        machine.usb_devices.insert(0, UsbDevice::from_binding(UsbBinding::ById(id)));
    } else {
        println!("No mouse selected. Please select your mouse from this complete list of connected devices:");
        let mouse = pick(None, &machine.usb_devices, !machine.usb_devices.is_empty())
            .expect("Failed to select mouse from complete list");
        if let Some(id) = mouse {
            machine.usb_devices.insert(0, UsbDevice::from_binding(UsbBinding::ById(id)));
        }
    }
    if let Some(id) = keyboard {
        machine.usb_devices.push(UsbDevice::from_binding(UsbBinding::ById(id)));
    } else {
        println!("No keyboard selected. Please select your keyboard from this complete list of connected devices:");
        let keyboard = pick(None, &machine.usb_devices, true)
            .expect("Failed to select keyboard from complete list");
        if let Some(id) = keyboard {
            machine.usb_devices.push(UsbDevice::from_binding(UsbBinding::ById(id)));
        }
    }
    if !ask::yesno("Done?") {
        println!("Aborted.");
        return false;
    }
    // add udev rule to add selected devices to vfio group
    wizard::sudo_write_file("/etc/udev/rules.d/80-vfio-usb.rules", |mut w| {
        for dev in machine.usb_devices.iter() {
            match dev.binding {
                UsbBinding::ById(DeviceId { vendor, product }) =>
                    writeln!(w, r#"SUBSYSTEM=="usb", ATTR{{idVendor}}=="{:04x}", ATTR{{idProduct}}=="{:04x}", ACTION=="add", RUN+="/usr/bin/setfacl -m g:vfio:rw- $devnode""#, vendor, product)?,
                UsbBinding::ByPort { bus, port } =>
                    writeln!(w, r#"SUBSYSTEM=="usb", ATTR{{busnum}}=="{}", ATTR{{devpath}}=="{}", ACTION=="add", RUN+="/usr/bin/setfacl -m g:vfio:rw- $devnode""#, bus, port)?,
            }
        }
        Ok(())
    }).expect("Cannot write udev rules");
    println!();
    true
}

fn pick(special: Option<HidKind>,
                 blacklist: &[UsbDevice], allow_abort: bool) -> Result<Option<DeviceId>> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_subsystem("usb")?;
    let devtype = if special.is_some() { "usb_interface" } else { "usb_device" };
    iter.match_property("DEVTYPE", devtype)?;

    if let Some(h) = special {
        iter.match_attribute("bInterfaceClass", "03")?; // HID
        iter.match_attribute("bInterfaceSubClass", "01")?; // boot interface (?)
        let protocol = match h {
            HidKind::Mouse => "02",
            HidKind::Keyboard => "01",
        };
        iter.match_attribute("bInterfaceProtocol", protocol)?;
    }

    let mut devs = Vec::new();

    for dev in iter.scan_devices()? {
        let dev = if special.is_some() {
            dev.parent().unwrap()
        } else {
            dev
        };

        let mut vendor = None;
        let mut product = None;
        let mut vendor_name = "Unknown vendor".to_owned();
        let mut product_name = "Unknown product".to_owned();

        for attr in dev.properties() {
            if let Some(val) = attr.value().to_str() {
                if attr.name() == "ID_VENDOR_ID" {
                    vendor = parse_hex(val);
                } else if attr.name() == "ID_MODEL_ID" {
                    product = parse_hex(val);
                } else if attr.name() == "ID_VENDOR_FROM_DATABASE" {
                    vendor_name = val.to_owned();
                } else if attr.name() == "ID_MODEL_FROM_DATABASE" {
                    product_name = val.to_owned();
                }
            }
        }

        let id = (vendor.unwrap(), product.unwrap()).into();
        if !blacklist.iter().any(|dev| dev.binding == UsbBinding::ById(id)) {
            println!("[{}]\t{} {} [{:04x}:{:04x}]", devs.len(), vendor_name, product_name,
                     id.vendor, id.product);
            devs.push(Some(id));
        }
    }

    if allow_abort {
        println!("[{}]\tNone of the above", devs.len());
        devs.push(None);
    }

    let k = match special {
        None => "usb device",
        Some(HidKind::Mouse) => "mouse",
        Some(HidKind::Keyboard) => "keyboard",
    };
    let selection = ask::numeric(&format!("Please select the {} you would like to pass through", k),
                                 0..devs.len());
    Ok(devs[selection])
}

