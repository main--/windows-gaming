extern crate libudev;
extern crate num_cpus;
extern crate common;
extern crate driver;

pub mod iommu;
pub mod usb;
pub mod vfio;
pub mod initramfs;
pub mod vm;

use libudev::{Result as UdevResult, Context, Enumerator};
use common::pci_device::PciDevice;
use common::config::PciId;

pub fn get_vfio_bound_devs() -> UdevResult<Vec<PciId>> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_property("DRIVER", "vfio-pci")?;

    Ok(iter.scan_devices()?.map(PciDevice::new).map(|x| x.id).collect())
}
