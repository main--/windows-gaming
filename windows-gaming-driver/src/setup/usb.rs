use std::io::Write;

use libudev::{Result, Context, Enumerator};

use config::{self, MachineConfig, UsbId, UsbPort, UsbBinding, UsbDevice};
use setup::{ask, wizard};
use usb_device::{UsbDevice as UsbDeviceInfo, Binding};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidKind {
    Mouse,
    Keyboard,
}

pub fn select(machine: &mut MachineConfig) -> bool {
    println!("Step 2: Select USB Devices");
    println!();

    // check existing configured devices
    if !machine.usb_devices.is_empty() {
        remove(&mut machine.usb_devices);
    }

    // if no device is configured ask for mouse and keyboard
    if machine.usb_devices.is_empty() {
        let mut mouse = pick(Some(HidKind::Mouse), &[], false)
            .expect("Failed to select Mouse");
        // if mouse is not detected as such, ask again with all usb devices as choice
        if mouse.is_none() {
            println!("No mouse selected. Please select your mouse from this complete list of connected devices:");
            mouse = pick(None, &[], false)
                .expect("Failed to select mouse from complete list");
        }
        if let Some(id) = mouse {
            machine.usb_devices.push(id);
        }

        let mut keyboard = pick(Some(HidKind::Keyboard), &machine.usb_devices, false)
            .expect("Failed to select Keyboard");
        // if keyboard is not detected as such, ask again with all usb devices as choice
        if keyboard.is_none() {
            println!("No keyboard selected. Please select your keyboard from this complete list of connected devices:");
            keyboard = pick(None, &machine.usb_devices, false)
                .expect("Failed to select keyboard from complete list");
        }
        if let Some(id) = keyboard {
            machine.usb_devices.push(id);
        }
    }

    // additional devices
    while ask::yesno("Would you like to add additional devices?") {
        if let Ok(Some(dev)) = pick(None, &machine.usb_devices, true) {
            machine.usb_devices.push(dev);
        }
    }

    if !ask::yesno("Done?") {
        println!("Aborted.");
        return false;
    }
    println!("Writing udev rules");
    // add udev rule to add selected devices to vfio group
    wizard::sudo_write_file("/etc/udev/rules.d/80-vfio-usb.rules", |mut w| {
        for dev in machine.usb_devices.iter() {
            match dev.binding {
                UsbBinding::ById(UsbId { vendor, product }) =>
                    writeln!(w, r#"SUBSYSTEM=="usb", ATTR{{idVendor}}=="{:04x}", ATTR{{idProduct}}=="{:04x}", ACTION=="add", RUN+="/usr/bin/setfacl -m g:vfio:rw- $devnode""#, vendor, product)?,
                UsbBinding::ByPort(UsbPort { bus, ref port }) =>
                    writeln!(w, r#"SUBSYSTEM=="usb", ATTR{{busnum}}=="{}", ATTR{{devpath}}=="{}", ACTION=="add", RUN+="/usr/bin/setfacl -m g:vfio:rw- $devnode""#, bus, port)?,
            }
        }
        Ok(())
    }).expect("Cannot write udev rules");
    println!();
    true
}

/// Lets the user remove (some) of given devices
///
/// This modifies the passed list.
fn remove(usb_devices: &mut Vec<UsbDevice>) {
    let infos = list_devices(None).expect("Can't read connected USB devices");

    loop {
        println!("You have currently selected the following usb devices: ");
        let mut last = 0;
        for (i, dev) in usb_devices.iter().cloned().enumerate() {
            if let Some(pos) = infos.iter().position(|info| info.id().unwrap() == dev.binding
                    || *info.port().unwrap() == dev.binding) {
                // device is connected
                let mut info = infos[pos].clone();
                match dev.binding {
                    UsbBinding::ById(id) => {
                        // TODO: Find out HidKind
                        info.binding = Binding::Id(id);
                        println!("[{}]\t{}", i, info);
                    },
                    UsbBinding::ByPort(port) => {
                        info.binding = Binding::Port(port);
                        println!("[{}]\t{}", i, info);
                    }
                }
            } else {
                // device not connected
                match dev.binding {
                    UsbBinding::ById(id) => {
                        let info = UsbDeviceInfo::from_id(id);
                        println!("[{}]\t{} (Not Connected)", i, info);
                    },
                    UsbBinding::ByPort(port) => {
                        println!("[{}]\tBus {} (Not Connected)", i, port);
                    }
                }
            }
            last = i;
        }
        println!("[{}]\tNone", last + 1);
        let index = ask::numeric("Which device would you like to remove?", 0..(last + 2));
        if index == last + 1 {
            break;
        }
        usb_devices.remove(index);
    }
}

fn pick(special: Option<HidKind>, blacklist: &[UsbDevice], extended_questions: bool) -> Result<Option<UsbDevice>> {
    // TODO: let user choose between id and rt binding
    let infos = list_devices(special).expect("Can't read connected usb devices");
    let mut devs: Vec<_> = infos.into_iter()
        .filter(|info| !blacklist.iter().any(|dev| match dev.binding {
            UsbBinding::ById(id) => id == info.id().unwrap(),
            UsbBinding::ByPort(ref port) => port == info.port().unwrap(),
        }))
        .map(|info| Some(info))
        .collect();
    for (i, dev) in devs.iter().enumerate() {
        println!("[{}]\t{}", i, dev.as_ref().unwrap());
    }

    println!("[{}]\tNone of the above", devs.len());
    devs.push(None);

    let k = match special {
        None => "usb device",
        Some(HidKind::Mouse) => "mouse",
        Some(HidKind::Keyboard) => "keyboard",
    };
    let selection = ask::numeric(&format!("Please select the {} you would like to pass through", k),
                                 0..devs.len());
    if let Some(dev) = devs[selection].as_ref() {
        let mut permanent = false;
        let mut binding = UsbBinding::ById(dev.id().unwrap());
        if extended_questions {
            permanent = ask::yesno("Would you like this device to be attached permanently?");

            println!("[0] By Id: {}", dev.id().unwrap());
            println!("[1] By Bus/Port: {}", dev.port().unwrap());
            if ask::numeric("How would you like to bind this device?", 0..2) == 1 {
                binding = UsbBinding::ByPort(dev.port().unwrap().clone());
            }
        }
        Ok(Some(UsbDevice {
            binding: binding,
            permanent: permanent,
            bus: config::default_usbdevice_bus(),
        }))
    } else {
        Ok(None)
    }
}

pub fn list_devices(kind: Option<HidKind>) -> Result<Vec<UsbDeviceInfo>> {
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
