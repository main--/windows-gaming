use std::borrow::Cow;
use std::path::Path;
use std::fs::File;
use std::io::{BufReader, BufRead, Write, Result};
use libudev::{Context, Enumerator};

use common::config::{MachineConfig, VfioDevice};
use common::pci_device::PciDevice;

const KERNEL_MODULES: &'static str = "vfio vfio_iommu_type1 vfio_pci vfio_virqfd";

pub fn get_devices<'a, 'b, P>(machine: &mut MachineConfig, question: &str, devices: &'b [PciDevice<'a>],
                      predicate: P) -> Vec<&'b PciDevice<'a>> where P: Fn(&PciDevice) -> bool {
    devices.iter().filter(|x| predicate(x) && !machine.pci_devices.iter().any(|y| y.id == x.id)).collect()
}


// TODO: this is hard
/*
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
*/

fn write_vfio_modconf<W: Write>(machine: &MachineConfig, mut w: W) -> Result<()> {
    let vfio_params = machine.pci_devices.iter().filter(|x| !x.resettable)
        .fold(String::new(), |s, i| s + &format!("{:04x}:{:04x},", i.id.vendor, i.id.device));

    writeln!(w, "options vfio-pci ids={}", vfio_params)
}
