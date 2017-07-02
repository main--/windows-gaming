#[macro_use] extern crate log;

extern crate argparse;
extern crate env_logger;
extern crate regex;

use std::path::Path;
use regex::Regex;
use argparse::{ArgumentParser, StoreTrue, Store};
use std::fs::OpenOptions;
use std::fs::read_link;
use std::io::prelude::*;

fn pretty_write(path: &Path, content : &str, dryrun: bool) {
	if dryrun {
		println!("writing {} into {}", content, path.display());
	}
	else { 
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

	debug!("Checking with Regexes");
	let dbdf_regex = Regex::new(r"^[[:xdigit:]]{4}:[[:xdigit:]]{2}:[[:xdigit:]]{2}.[[:xdigit:]]$").unwrap();
	let bdf_regex = Regex::new(r"^[[:xdigit:]]{2}:[[:xdigit:]]{2}.[[:xdigit:]]$").unwrap();
	
	if !dbdf_regex.is_match(&device) {
		if !bdf_regex.is_match(&device) {
			println!("Please supply Domain:Bus:Device.Function of PCI device in form: dddd:bb:dd.f");
			return;
		}
		else {
			warn!("No PCI domain supplied, assuming PCI domain is 0000");
			device = "0000:".to_string() + &device;
		}
	}
	
	let dev_sysfs = "/sys/bus/pci/devices/".to_string() + &device;
	let dev_sysfs_path = Path::new(&dev_sysfs);
	let dev_iommu = dev_sysfs_path.join("iommu");
	
	if !dev_iommu.exists() {
		println!("No signs of an IOMMU. Check your hardware and/or linux cmdline parameters.");
		println!("Use intel_iommu=on or iommu=pt iommu=1");
		info!("File {} didn't exist", dev_iommu.display());
		return;
	}
	
	let dev_reset = dev_sysfs_path.join("reset");
	
	if ! dev_reset.exists() {
		error!("The device does no support resetting!");
		info!("File {} didn't exist", dev_reset.display());
		return;
	}
	
	let dev_driver_link = dev_sysfs_path.join("driver");
	let dev_driver = read_link(dev_driver_link);
	
	if let Ok(driver) = dev_driver {
		if driver.file_name().unwrap() == "vfio-pci" && !remove{
			println!("Device already bound to vfio-pci driver, nothing to do here");
			return;
		}
		info!("Device already has a driver, unbinding");
		if remove {
			pretty_write(&dev_sysfs_path.join("driver_override"), "\n", dryrun);
		}
		else{
			pretty_write(&dev_sysfs_path.join("driver_override"), "vfio-pci", dryrun);
		}
		pretty_write(&dev_sysfs_path.join("driver/unbind"), &device, dryrun);		
	}
	
	pretty_write(Path::new("/sys/bus/pci/drivers_probe"), &device, dryrun);
}
