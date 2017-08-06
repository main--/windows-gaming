use std::io::Result as IoResult;
use std::process::{Command, Stdio, ChildStdin};
use std::path::Path;
use std::iter::Iterator;

use libudev::{Result as UdevResult, Context, Enumerator};

use common::config::{Config, SetupConfig, PciId};
use common::pci_device::PciDevice;
use driver;

use ask;
use iommu;
use usb;
use vfio;
use initramfs;
use vm;

struct Wizard;

impl Wizard {
    fn run(&mut self, cfg: Option<Config>, cfg_path: &Path, workdir: &Path, datadir: &Path) {
        let mut config = cfg.unwrap_or_default();
        if config.setup.is_none() {
            config.setup = Some(SetupConfig::default());
        }

        println!("Welcome!");
        println!("This wizard will help you configure a PCI passthrough setup!");
        println!("Some steps are automated, others have to be done manually. Note that answering no ('n') to those will abort the wizard.");
        println!("You can abort the wizard at any point without risking corruptions except where it specifically tells you not to.");
        println!();
        println!("This setup assumes that you are currently running on the GPU you want Linux to run on.");
        println!("Make sure that you remove unnecessary drivers as they might interfere.");
        println!("You may also need to configure your display server's config files accordingly (e.g. xorg.conf for Xorg).");
        println!();

        if !ask::yesno("Understood?") {
            println!("Aborted.");
            return;
        }

        println!();

        // TODO: Add a step 0 here where we add the current user to the vfio group

        if !iommu::enable(config.setup.as_mut().unwrap()) {
            return;
        }
        config.save(cfg_path);

        if !usb::select(&mut config.machine) {
            return;
        }
        config.save(cfg_path);

        if !vfio::setup(&mut config.machine) {
            return;
        }
        config.save(cfg_path);

        let passthrough_devs = get_passthrough_devs().expect("Failed to check gpu passthrough with udev");
        if passthrough_devs.is_empty() {
            if !initramfs::rebuild() {
                return;
            }

            // TODO: hugepages part 1 (as you have to reboot)

            config.setup.as_mut().unwrap().reboot_commanded = true;
            config.save(cfg_path);

            println!();
            println!("Step 5: Reboot");
            println!("Now that everything is properly configured, the initramfs should load vfio which should then grab your graphics card.");
            println!("Before you boot into Linux again, please check your firmware/bios settings to make sure that the guest GPU is NOT your \
                      primary graphics device. If everything works correctly, the host's graphics drivers will no longer see the card so \
                      remember to plug your monitor to a different output (e.g. Intel onboard gfx).");
            println!("Some firmware implementations initialize a UEFI framebuffer on the card anyways. This is no problem for VFIO but \
                      it may cause your monitor to pick up a black image from the discrete graphics if it's already plugged into both. \
                      If you get no video output whatsoever, your system is probably not creating a virtual console and instead relies \
                      on the UEFI framebuffer entirely. This too can be fixed but that's a lot harder to do and pretty much impossible \
                      if you don't have an emergency/live OS you can boot into.");
            println!("If you don't have anything like that, now is the time to change your mind and undo these changes. (Removing the vfio \
                      module from your initramfs should almost certainly leave you with a perfectly working system.)");
            println!("You have been warned.");
            println!();
            println!("With that out of the way, your next step after the reboot is simply to launch this wizard again and we can move on!");
        } else { // if something is actually passed through properly
            println!("Step 6: Check IOMMU grouping");
            if !iommu::check_grouping(&config.machine).expect("Failed to check IOMMU grouping") {
                return;
            }
            println!();

            if !vm::setup(config.setup.as_mut().unwrap(), &mut config.machine, datadir) {
                return;
            }
            config.save(cfg_path);

            println!("Your VM is going to boot now.");
            println!("Just install Windows and shut it down cleanly as soon as that's done so we can continue.");
            println!();
            println!("Note: Windows probably won't pick up the virtio-scsi storage device right away. You can load the drivers from the attached floppy drive.");
            if !ask::yesno("Ready?") {
                println!("Aborted.");
                return;
            }

            driver::run(&config, workdir, datadir);

            // TODO:
            // * ask if it worked, offer to retry or abort
            // * record progress in the config file
            // * boot again with the guest-agent ISO and tell them to install that
            // * network
            // * if installed: configure samba if they want it
            // * at some point determine that everthing is properly set up
            // * after some closing remarks to the user just switch over to normal operation

            println!("Alright, so far so good!");
            unimplemented!();
        }
    }
}

fn get_passthrough_devs() -> UdevResult<Vec<PciId>> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_property("DRIVER", "vfio-pci")?;

    Ok(iter.scan_devices()?.map(PciDevice::new).map(|x| x.id).collect())
}


pub fn sudo_write_file<P: AsRef<Path>, F: FnOnce(&mut ChildStdin) -> IoResult<()>>(path: P, write: F) -> IoResult<bool> {
    let mut writer_child = Command::new("/usr/bin/sudo").env("SUDO_EDITOR", "/usr/bin/tee")
        .arg("-e").arg(path.as_ref().to_str().unwrap()).stdin(Stdio::piped()).stdout(Stdio::null()).spawn()?;
    {
        let mut writer = writer_child.stdin.as_mut().unwrap();
        write(writer)?;
    }
    Ok(writer_child.wait()?.success())
}

pub fn run(cfg: Option<Config>, target: &Path, workdir: &Path, datadir: &Path) {
    Wizard.run(cfg, target, workdir, datadir);
}
