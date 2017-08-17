use num_cpus;

use std::env;
use std::path::Path;

use common::config::{MachineConfig, SetupConfig, StorageDevice};
use driver::qemu;

// TODO: completely restructure this
/*
pub fn setup(setup: &mut SetupConfig, machine: &mut MachineConfig, datadir: &Path) -> bool {
    println!("Step 7: VM setup");
    println!("Looks like everything is working fine so far! Time to configure your VM!");

    let logical_cores = num_cpus::get();
    let physical_cores = num_cpus::get_physical();

    if machine.cores == 0 {
        machine.cores = physical_cores;
        if logical_cores == physical_cores * 2 {
            // hyperthreading detected
            machine.threads = Some(2);
        } else if logical_cores != physical_cores {
            println!("Warning: You have {} logical on {} physical cores. Only using physical cores for now.", logical_cores, physical_cores);
        }
    }

    if !memory(machine) {
        return false;
    }

    if qemu::has_gtk_support() && env::var("DISPLAY").is_ok() {
        println!("It seems you're running this setup in a graphical environment. This can make things a lot easier!");
        println!("While our objective is of course VGA passthrough, running a virtual display during setup is very convenient for many reasons. We strongly recommend using this.");
        if ask::yesno("Would you like to enable virtual graphics (only during setup)?") {
            setup.gui = true;
        }
    }
    if !setup.gui {
        // TODO: Here we would hook them up with mouse-only passthrough so they can
        // do the setup without losing control over the machine.
        // Not implemented because the whole USB thing is still missing.
        unimplemented!();
    }

    println!("Configuring VM root hard disk. Only raw disks are supported for now (sorry).");
    println!("WARNING: ALL DATA ON THE BLOCK DEVICE YOU SELECT HERE WILL BE DELETED!");
    machine.storage.push(StorageDevice {
        cache: "none".to_owned(),
        format: "raw".to_owned(),
        path: ask::file("Please enter the path to the VM root block device"),
    });
    // TODO: Support multiple storage devices
    // TODO: Support qcow2 images

    // Stage 1: First boot and Windows setup

    // TODO: if installed, call windows10-get-download-link, wget to a temporary location and go on
    setup.cdrom = Some(ask::file("Please enter the path to your Windows ISO"));
    setup.floppy = Some(datadir.join("virtio-win.vfd").to_str().unwrap().to_owned());
    true
}

pub fn memory(machine: &mut MachineConfig) -> bool {
    // TODO: hugepages part 2
    if machine.memory == "" {
        // FIXME: validate this
        machine.memory = ask::anything("How much memory would you like to assign to it?",
                                       "be careful no validation LUL", |x| Some(x.to_owned()));
    }
    true
}
*/
