use libudev::{Result, Context, Enumerator};

use config::{SetupConfig, MachineConfig};
use pci_device::PciDevice;
use setup::ask;

pub fn select(setup: &mut SetupConfig, machine: &mut MachineConfig) -> Result<()> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_subsystem("pci")?;
    let pci_devs: Vec<_> = iter.scan_devices()?.map(PciDevice::new).collect();

    // filter to the display controller class (0x03XXXX, udev drops the leading zero)
    let display_controllers: Vec<_> = pci_devs.iter().filter(|x| x.pci_class.starts_with("3") && x.pci_class.len() == 5).collect();

    println!();
    for (i, dev) in display_controllers.iter().enumerate() {
        println!("[{}]\t{}", i, dev);
    }

    let selection = ask::numeric("Please select the graphics device you would like to pass through", 0..display_controllers.len());
    let selected = display_controllers[selection];

    let mut related_devices: Vec<_> = pci_devs.iter().filter(|x| x.pci_device() == selected.pci_device()).collect();
    let gpu_index = related_devices.iter().position(|dev| dev == &selected).unwrap();
    related_devices.swap(0, gpu_index);
    setup.vfio_devs = related_devices.iter().map(|dev| dev.id).collect();
    machine.vfio_slots = related_devices.iter().map(|dev| dev.pci_slot.clone()).collect();
    Ok(())
}

