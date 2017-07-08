#[macro_use] extern crate log;

extern crate argparse;
extern crate env_logger;
extern crate regex;
extern crate users;

use std::path::Path;
use regex::Regex;
use argparse::{ArgumentParser, StoreTrue, Store};
use std::fs::{OpenOptions, self};
use std::io::prelude::*;
use users::os::unix::GroupExt;

fn pretty_write(path: &Path, content: &str, dryrun: bool) {
    if dryrun {
        println!("writing {} into {}", content, path.display());
    } else { 
        info!("writing {} into {}", content, path.display());
    
        let mut file = OpenOptions::new().write(true).open(&path).expect(&format!("Failed to open {}", path.display()));
        if let Err(e) = write!(&mut file, "{}", content) {
            panic!("Failed to write {} into {}! Got: {}", content, path.display(), e)
        }
    }
}

fn main() {

    let mut dryrun = false;
    let mut remove = false;
    let mut device = "".to_string();
    
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("This tool allows you to bind the vfio driver to a specified resetable pci-device");
        ap.refer(&mut device).add_argument("PCI-Device", Store, "The PCI-Device to bind to").required();
        ap.refer(&mut dryrun).add_option(&["-d", "--dry-run"], StoreTrue, "Don't change anything");
        ap.refer(&mut remove).add_option(&["-r", "--remove"], StoreTrue, "Reattach the default Driver to the device");
        ap.parse_args_or_exit();
    }
    
    env_logger::init().unwrap();
    
    debug!("effective uid: {} current uid: {}", users::get_effective_uid(), users::get_current_uid());
    
    if users::get_effective_uid() != 0 {
        error!("This tool requires root permissions. If the setuid bit is not set, you need to execute this as root!");
    }
    if users::get_current_uid() != 0 {
        let vfio_group = users::get_group_by_name("vfio")
            .expect("Your system has no vfio group. You need to be part of it to run this tool!");
        let user = users::get_user_by_uid(users::get_current_uid()).unwrap();
        let user_name = user.name();
        
        if !vfio_group.members().contains(&user_name.to_owned()) {
            panic!("You're not part of the vfio group. Apply there and run this tool again!");
        }
        else {
            debug!("User is part of the vfio group...continuing");
        }
    }

    debug!("Checking with Regexes");
    let dbdf_regex = Regex::new(r"^[[:xdigit:]]{4}:[[:xdigit:]]{2}:[[:xdigit:]]{2}.[[:xdigit:]]$").unwrap();
    let bdf_regex = Regex::new(r"^[[:xdigit:]]{2}:[[:xdigit:]]{2}.[[:xdigit:]]$").unwrap();
    
    match (dbdf_regex.is_match(&device), bdf_regex.is_match(&device)) {
        (false, true) => { 
            warn!("No PCI domain supplied, assuming PCI domain is 0000");	
            device = "0000:".to_string() + &device; 
        },
        (false, false) => panic!("Please supply Domain:Bus:Device.Function of PCI device in form: dddd:bb:dd.f"),
        (true, _) => (),
    }
    
    let dev_sysfs = Path::new("/sys/bus/pci/devices/").join(&device);
    let dev_iommu = dev_sysfs.join("iommu");
    
    if !dev_iommu.exists() {
        info!("File {} didn't exist", dev_iommu.display());
        panic!("No signs of an IOMMU. \
                Check your hardware and/or linux cmdline parameters. \
                Use intel_iommu=on or iommu=pt iommu=1");
    }
    
    let dev_reset = dev_sysfs.join("reset");
    
    if !dev_reset.exists() {
        info!("File {} didn't exist", dev_reset.display());
        panic!("The device does not support resetting!");
    }
    
    let dev_driver_link = dev_sysfs.join("driver");
    let dev_driver = fs::read_link(dev_driver_link);
    
    if let Ok(driver) = dev_driver {
        if driver.file_name().unwrap() == "vfio-pci" && !remove {
            println!("Device already bound to vfio-pci driver, nothing to do here");
            return;
        }
        info!("Device already has a driver, unbinding");
        
        let driver = if remove {"\n"} else {"vfio-pci"};
        pretty_write(&dev_sysfs.join("driver_override"), driver, dryrun);
        pretty_write(&dev_sysfs.join("driver/unbind"), &device, dryrun);		
    } else if !remove {
        pretty_write(&dev_sysfs.join("driver_override"), "vfio-pci", dryrun);
    }
    
    pretty_write(Path::new("/sys/bus/pci/drivers_probe"), &device, dryrun);
}
