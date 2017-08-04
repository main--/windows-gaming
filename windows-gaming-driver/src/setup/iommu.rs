use std::io::Result;
use std::fs;

use libudev::{Context, Enumerator};

use config::{SetupConfig, MachineConfig};
use setup::ask;
use pci_device::PciDevice;

pub fn enable(setup: &mut SetupConfig) -> bool {
    println!("Step 1: Enable IOMMU");
    println!("It's as simple as adding 'intel_iommu=on' or 'amd_iommu=on' to your kernel command line.");
    println!("Do this now, then continue here. Don't reboot yet, there's more we need to configure.");
    println!();
    if is_enabled() {
        if ask::yesno("IOMMU is already enabled. Do you want to skip this step?") {
            return true;
        }
    }
    if setup.iommu_commanded {
        println!("Troubleshooting (since you apparently already did this):");
        println!("This is a kernel parameter, so it won't be active before you reboot. But if you already did that, \
                  the kernel fails to enable it for some reason. IOMMU (aka VT-d) is disabled by default on many \
                  mainboards, please check your firmware settings to make sure it's enabled. If that doesn't work \
                  it's possible that your hardware just doesn't support it. If that's the reason, you're out of luck \
                  though. IOMMU is a critical component of this setup and there's no way it can work without that. Sorry.");
    }

    if !ask::yesno("Done?") {
        println!("Aborted.");
        return false;
    }
    setup.iommu_commanded = true;
    println!();
    true
}

pub fn is_enabled() -> bool {
    fs::read_dir("/sys/devices/virtual/iommu/").ok().and_then(|mut x| x.next()).is_some()
}

pub fn check_grouping(machine: &MachineConfig) -> Result<bool> {
    let udev = Context::new().expect("Failed to create udev context");
    let first_id = machine.pci_devices[0].device; // FIXME
    let mut iter = Enumerator::new(&udev)?;
    iter.match_subsystem("pci")?;
    let mut iter = iter.scan_devices()?.map(PciDevice::new).filter(|x| x.id == first_id);
    let selected = iter.next().expect("PCI device is gone now?");
    assert!(iter.next().is_none());


    let iommu_dir = selected.dev.syspath().join("iommu_group").join("devices");
    assert!(iommu_dir.is_dir());

    let mut unrelated_devices = Vec::new();
    let mut related_devices = Vec::new();
    for entry in fs::read_dir(&iommu_dir)? {
        let dev = PciDevice::new(udev.device_from_syspath(&entry?.path())?);
        if dev.pci_device() == selected.pci_device() {
            // these are ours
            related_devices.push(dev.id);
        } else if dev.pci_class == "60400"/*pci bridge*/ {
            // According to the Arch wiki (https://wiki.archlinux.org/index.php/PCI_passthrough_via_OVMF)
            // passing a PCI bridge is fine, so we just ignore those.
            // TODO: This check probably doesn't catch everything so maybe improve it one day.
        } else {
            // now the rest is actually unrelated and therefore problematic
            unrelated_devices.push(dev);
        }
    }

    if unrelated_devices.len() > 0 {
        println!("Warning: There are unrelated devices grouped with the selected GPU:");
        for dev in unrelated_devices {
            println!("\t{}", dev);
        }

        println!("This is a problem as it means that the GPU is not properly isolated - you can only pass entire groups to a VM. All or nothing.");
        println!("While there ARE fixes for this issue, it's not supported by this tool (yet), so you're on your own.");
        // FIXME: Instead of telling them that they're screwed, we should just inform them about the all-or-nothing,
        // ask if that's alright and then configure things that way instead (requiring another reboot to bind the remaining vfio-pci devices).
        // (Untested but according to the wiki that's how it should work)

        if !ask::yesno("Ignore this and carry on? (not recommended unless you know exactly what you're doing)") {
            println!("Aborted.");
            return Ok(false);
        }
    }

    related_devices.sort();
    related_devices.dedup();

    // FIXME assert!(setup.vfio_devs.iter().cloned().eq(related_devices.iter().cloned()));
    Ok(true)
}

