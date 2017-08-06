use std::borrow::Cow;
use std::path::Path;
use std::fs::File;
use std::io::{BufReader, BufRead, Write, Result};
use libudev::{Context, Enumerator};

use common::config::{MachineConfig, VfioDevice};
use common::pci_device::PciDevice;
use ask;
use wizard;

const KERNEL_MODULES: &'static str = "vfio vfio_iommu_type1 vfio_pci vfio_virqfd";

pub fn setup(machine: &mut MachineConfig) -> bool {
    println!("Step 3: Setting up the vfio driver");

    if !machine.pci_devices.is_empty() {
        println!();
        println!("{}", "Troubleshooting (since you apparently already did this):");
        println!("Just like Step 1, this requires a reboot to activate. If you already did that, the most likely cause \
                  is that things were misconfigured somehow. Are the kernel modules really in the initramfs now? \
                  Are they loaded? Are they loaded BEFORE any graphics drivers? Is the module configuration applied \
                  correctly? Note that vfio-pci only exists since Linux 4.1. Earlier versions are not supported by \
                  this tool but you can still make it work with pci-stub. You're on your own there but if you need this \
                  and figure it out remember that contributions are always appreciated!");
        println!();
    }

    select(machine).expect("Failed to select PCI Devices");
    println!("Success!");
    println!();

    let mut has_modconf = false;
    let mut skip_ask = false;
    if autoconfigure_mkinitcpio(&mut has_modconf).unwrap_or(false) {
        println!("Success!");
        println!();
        if !has_modconf {
            println!("However, it looks like your mkinitcpio is using a nonstandard configuration that does not use the 'modconf' hook.");
            println!("This hook inserts a config file that tells the vfio driver what PCI devices it should bind to, so things won't work without it.");
            println!("If our detection just bugged and you actually have the hook enabled, things are obviously fine.");
            println!("Alternatively, you have to make sure that our configuration at /etc/modprobe.d/vfio.conf (creating right now) is properly applied.");
            if !ask::yesno("Done?") {
                println!("Aborted.");
                return false;
            }
        } else {
            skip_ask = true;
        }
    } else {
        println!("Falling back to manual mode.");
        println!();
        println!("Please configure your initramfs generator to load these kernel modules: {}", KERNEL_MODULES);
        println!("Make sure that they are loaded *before* any graphics drivers!");
        println!("For mkinitcpio users, adding them at the *start* of your MODULES line in /etc/mkinitcpio.conf will take care of this.");
        println!();
        if !ask::yesno("Done?") {
            println!("Aborted.");
            return false;
        }

        println!("Now that the vfio is added, it needs to know what PCI devices it should bind to.");
        println!("We configure this in /etc/modprobe.d/vfio.conf (creating right now) but your initramfs generator needs to understand it.");
        if has_modconf {
            println!("Looks like your mkinitcpio contains the hook that does this ('modconf') but perhaps you'd like to double-check.");
        } else {
            println!("Since you're either not using mkinitcpio at all or heavily customized your configuration, you're on your own here. Good luck.");
            println!("(Feel free to contribute support for other initramfs generators.)");
        }
    }

    write_vfio_modconf(machine);

    if !skip_ask {
        if !ask::yesno("Done?") {
            println!("Aborted.");
            return false;
        }
    }
    true
}

fn select(machine: &mut MachineConfig) -> Result<()> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_subsystem("pci")?;
    let pci_devs: Vec<_> = iter.scan_devices()?.map(PciDevice::new).collect();


    // filter to the display controller class (0x03XXXX, udev drops the leading zero)
    select_device(machine, "Which graphics card would you like to pass through?", &pci_devs,
                  |x| x.pci_class.starts_with("3") && x.pci_class.len() == 5)?;
    if ask::yesno("Would you like to pass aditional PCI devices?") {
        println!();
        println!("Please choose aditional pci devices. These will be passed through to the VM on its boot.");
        println!("Note: For this to work properly, the device has to support resetting.");
        println!("The devices will be bound to the vfio-pci driver on VM startup and unbound when it quits.");
        println!("You will not be able to use them on your host system while Windows is running.");
        println!();

        while select_device(machine, "Which device woud you like to pass through?", &pci_devs,
                            |x| x.resettable)? { }

        println!("Note that non-resettable passthrough is only known to work for GPUs.");
        println!("With everything else you're completely on your own.");
        println!("You will not be able to use these devices on your host system at all.");
        println!();
        if ask::yesno("Would you like to pass any non-resettable PCI devices?") {
            // all non-resettable devices but no PCI bridges (because arch wiki says so)
            while select_device(machine, "Which device do you want to pass through?", &pci_devs,
                                |x| !x.resettable && x.pci_class != "60400")? { }
        }
    }
    Ok(())
}

fn select_device<P>(machine: &mut MachineConfig, question: &str, devices: &[PciDevice],
                    predicate: P) -> Result<bool>
                    where P: Fn(&PciDevice) -> bool {
    let askable_devices: Vec<_> = devices.iter()
        .filter(|x| predicate(x) && !machine.pci_devices.iter().any(|y| y.id == x.id)).collect();

    for (i, dev) in askable_devices.iter().enumerate() {
        println!("[{}]\t{}", i, dev);
    }
    println!("[{}]\tNone of the above.", askable_devices.len());

    let selection = ask::numeric(question, 0..askable_devices.len()+1);

    if let Some(selected_slot) = askable_devices.get(selection).map(|x| x.pci_slot.clone()) {
        for device in devices.iter()
                .filter(|x| x.pci_slot[..x.pci_slot.len()-1] == selected_slot[..selected_slot.len()-1]) {
            let vfio_device = VfioDevice {
                resettable: device.resettable,
                slot: device.pci_slot.clone(),
                id: device.id,
            };

            machine.pci_devices.push(vfio_device);
        }
        Ok(true)
    } else {
        Ok(false)
    }
}



fn autoconfigure_mkinitcpio(has_modconf: &mut bool) -> Result<bool> {
    const MKINITCPIO_CONF: &'static str = "/etc/mkinitcpio.conf";

    // File::open works on symlinks but sudo -e does not.
    // So we dereference.
    let mut path = Cow::from(Path::new(MKINITCPIO_CONF));
    while let Ok(p) = path.read_link() {
        path = Cow::from(p);
    }

    assert!(path.is_file());

    if let Ok(f) = File::open(MKINITCPIO_CONF) {
        println!("It seems you are using mkinitcpio. (If you aren't, select NO here!)");

        if ask::yesno("Would you like me to try to edit the config file for you?") {
            let mut hooks_added = false;
            let mut already_added = false;

            let mut mkic_conf = Vec::new();
            for line in BufReader::new(f).lines().flat_map(|x| x.ok()) {
                const MODULES_PREFIX: &'static str = "MODULES=\"";
                const HOOKS_PREFIX: &'static str = "HOOKS=\"";
                if line.starts_with(MODULES_PREFIX) {
                    if line.contains(KERNEL_MODULES) {
                        // Already added.
                        // Now this might still be at a different location (not at the start)
                        // but those users probably know what they're doing anyways.
                        already_added = true;
                        hooks_added = true;
                    } else if line.contains("vfio") || hooks_added {
                        // bail out if there's already vfio stuff in the file
                        // or if there's two MODULES lines for some reason
                        return Ok(false);
                    } else {
                        mkic_conf.push(MODULES_PREFIX.to_owned() + KERNEL_MODULES + " " + &line[MODULES_PREFIX.len()..]);
                        hooks_added = true;
                    }
                } else {
                    if line.starts_with(HOOKS_PREFIX) && line.contains("modconf") {
                        *has_modconf = true;
                    }
                    mkic_conf.push(line);
                }
            }

            if already_added {
                return Ok(true);
            }

            if hooks_added {
                return wizard::sudo_write_file(MKINITCPIO_CONF, |writer| {
                    for line in mkic_conf {
                        writeln!(writer, "{}", line)?;
                    }
                    Ok(())
                });
            }
        }
    }

    Ok(false)
}

fn write_vfio_modconf(machine: &MachineConfig) {
    let vfio_params = machine.pci_devices.iter().filter(|x| !x.resettable)
        .fold(String::new(), |s, i| s + &format!("{:04x}:{:04x},", i.id.vendor, i.id.device));
    assert!(wizard::sudo_write_file("/etc/modprobe.d/vfio.conf", |x| {
        writeln!(x, "options vfio-pci ids={}", vfio_params)
    }).unwrap_or(false), "Failed to write modconf");
}
