use std::io::{self, Write, BufRead, BufReader, StdinLock, Result as IoResult};
use std::process::{Command, Stdio, ChildStdin};
use std::fs::{read_dir, File};
use std::path::Path;
use std::iter::Iterator;
use std::env;

use libudev::{Result as UdevResult, Context, Enumerator};
use num_cpus;
use config::{Config, MachineConfig, StorageDevice, SetupConfig};
use pci_device::PciDevice;
use qemu;
use setup::ask;

struct Wizard<'a> {
    stdin: StdinLock<'a>,
    udev: Context,
}

impl<'a> Wizard<'a> {
    fn udev_select_gpu(&mut self) -> UdevResult<Option<Vec<(u16, u16)>>> {
        let mut iter = Enumerator::new(&self.udev)?;
        iter.match_subsystem("pci")?;
        let pci_devs: Vec<_> = iter.scan_devices()?.map(PciDevice::new).collect();

        // filter to the display controller class (0x03XXXX, udev drops the leading zero)
        let display_controllers: Vec<_> = pci_devs.iter().filter(|x| x.pci_class.starts_with("3") && x.pci_class.len() == 5).collect();

        println!("");
        for (i, dev) in display_controllers.iter().enumerate() {
            println!("[{}]\t{}", i, dev);
        }

        let selection = ask::numeric(&mut self.stdin, "Please select the graphics device you would like to pass through", 0..display_controllers.len());
        let selected = display_controllers[selection];

        let mut related_devices: Vec<_> = pci_devs.iter().filter(|x| x.pci_device() == selected.pci_device()).map(|x| x.id).collect();
        related_devices.sort();
        Ok(Some(related_devices))
    }

    fn check_iommu_grouping(&mut self, cfg: &SetupConfig) -> IoResult<bool> {
        let first_id = cfg.vfio_devs[0];
        let mut iter = Enumerator::new(&self.udev)?;
        iter.match_subsystem("pci")?;
        let mut iter = iter.scan_devices()?.map(PciDevice::new).filter(|x| x.id == first_id);
        let selected = iter.next().expect("PCI device is gone now?");
        assert!(iter.next().is_none());


        let iommu_dir = selected.dev.syspath().join("iommu_group").join("devices");
        assert!(iommu_dir.is_dir());

        let mut unrelated_devices = Vec::new();
        let mut related_devices = Vec::new();
        for entry in read_dir(&iommu_dir)? {
            let dev = PciDevice::new(self.udev.device_from_syspath(&entry?.path())?);
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

            if !ask::yesno(&mut self.stdin,
                          "Ignore this and carry on? (not recommended unless you know exactly what you're doing)") {
                println!("Aborted.");
                return Ok(false);
            }
        }

        related_devices.sort();
        related_devices.dedup();

        assert!(cfg.vfio_devs == related_devices);
        Ok(true)
    }

    fn get_passthrough_devs(&self) -> UdevResult<Vec<(u16, u16)>> {
        let mut iter = Enumerator::new(&self.udev)?;
        iter.match_property("DRIVER", "vfio-pci")?;

        Ok(iter.scan_devices()?.map(PciDevice::new).map(|x| x.id).collect())
    }

    fn autoconfigure_mkinitcpio(&mut self, has_modconf: &mut bool) -> IoResult<bool> {
        static MKINITCPIO_CONF: &'static str = "/etc/mkinitcpio.conf";

        // File::open works on symlinks but sudo -e does not.
        if !Path::new(MKINITCPIO_CONF).is_file() {
            return Ok(false);
        }

        if let Ok(f) = File::open(MKINITCPIO_CONF) {
            println!("It seems you are using mkinitcpio. (If you aren't, select NO here!)");

            if ask::yesno(&mut self.stdin, "Would you like me to try to edit the config file for you?") {
                let mut hooks_added = false;
                let mut already_added = false;

                let mut mkic_conf = Vec::new();
                for line in BufReader::new(f).lines().flat_map(|x| x.ok()) {
                    static MODULES_PREFIX: &'static str = "MODULES=\"";
                    static HOOKS_PREFIX: &'static str = "HOOKS=\"";
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
                    return sudo_write_file(MKINITCPIO_CONF, |writer| {
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

    fn write_vfio_modconf(&self, setup: &SetupConfig) {
        let vfio_params = setup.vfio_devs.iter().fold(String::new(), |s, &(v, d)| s + &format!("{:04x}:{:04x},", v, d));
        assert!(sudo_write_file("/etc/modprobe.d/vfio.conf", |x| {
            writeln!(x, "options vfio-pci ids={}", vfio_params)
        }).unwrap_or(false), "Failed to write modconf");
    }

    fn run(&mut self, cfg: Option<Config>, target: &Path, workdir: &Path, datadir: &Path) {
        static TROUBLESHOOTING: &'static str = "Troubleshooting (since you apparently already did this):";

        let mut machine = MachineConfig {
            memory: "".to_owned(),
            cores: 0,
            network: None,
            storage: Vec::new(),

            hugepages: None,
            threads: None,
        };

        let mut setup = SetupConfig {
            cdrom: None,
            floppy: None,
            gui: false,

            iommu_commanded: false,
            reboot_commanded: false,
            vfio_devs: Vec::new(),
        };

        if let Some(cfg) = cfg {
            machine = cfg.machine;
            if let Some(cfg_setup) = cfg.setup {
                setup = cfg_setup;
            }
        }

        fn make_config(machine: &MachineConfig, setup: &SetupConfig) -> Config {
            Config {
                machine: machine.clone(),
                setup: Some(setup.clone()),
                samba: None,
            }
        }


        println!("Welcome!");
        println!("This wizard will help you configure a PCI passthrough setup!");
        println!("Some steps are automated, others have to be done manually. Note that answering no ('n') to those will abort the wizard.");
        println!("You can abort the wizard at any point without risking corruptions except where it specifically tells you not to.");
        println!("");
        if !ask::yesno(&mut self.stdin, "Understood?") {
            println!("Aborted.");
            return;
        }

        println!("");

        // TODO: Add a step 0 here where we add the current user to the vfio group

        if !is_iommu_enabled() {
            println!("Step 1: Enable IOMMU");
            println!("It's as simple as adding 'intel_iommu=on' or 'amd_iommu=on' to your kernel command line.");
            println!("Do this now, then continue here. Don't reboot yet, there's more we need to configure.");
            println!("");
            if setup.iommu_commanded {
                println!("{}", TROUBLESHOOTING);
                println!("This is a kernel parameter, so it won't be active before you reboot. But if you already did that, \
                          the kernel fails to enable it for some reason. IOMMU (aka VT-d) is disabled by default on many \
                          mainboards, please check your firmware settings to make sure it's enabled. If that doesn't work \
                          it's possible that your hardware just doesn't support it. If that's the reason you're out of luck \
                          though. IOMMU is a critical component of this setup and there's no way it can work without that. Sorry.");
            }

            if !ask::yesno(&mut self.stdin, "Done?") {
                println!("Aborted.");
                return;
            }
            setup.iommu_commanded = true;
            make_config(&machine, &setup).save(target);
            println!("");
        }

        // TODO: Add a udev picker step here (just like the one we have for gpu) that selects USB keyboard and mouse.
        // Then save that to the config and create a udev rule file that runs setfacl so the vfio group can use them.

        let passthrough_devs = self.get_passthrough_devs().expect("Failed to check gpu passthrough with udev");
        if passthrough_devs.is_empty() {
            println!("Step 2: Setting up the vfio driver");

            if !setup.vfio_devs.is_empty() {
                println!("");
                println!("{}", TROUBLESHOOTING);
                println!("Just like Step 1, this requires a reboot to activate. If you already did that, the most likely cause \
                          is that things were misconfigured somehow. Are the kernel modules really in the initramfs now? \
                          Are they loaded? Are they loaded BEFORE any graphics drivers? Is the module configuration applied \
                          correctly? Note that vfio-pci only exists since Linux 4.1. Earlier versions are not supported by \
                          this tool but you can still make it work with pci-stub. You're on your own there but if you need this \
                          and figure it out remember that contributions are always appreciated!");
                println!("");
            }

            setup.vfio_devs = match self.udev_select_gpu().expect("Failed to select GPU") {
                Some(x) => x,
                None => return,
            };
            println!("Success!");
            println!("");

            let mut has_modconf = false;
            let mut skip_ask = false;
            if self.autoconfigure_mkinitcpio(&mut has_modconf).unwrap_or(false) {
                println!("Success!");
                println!("");
                if !has_modconf {
                    println!("However, it looks like your mkinitcpio is using a nonstandard configuration that does not use the 'modconf' hook.");
                    println!("This hook inserts a config file that tells the vfio driver what PCI devices it should bind to, so things won't work without it.");
                    println!("If our detection just bugged and you actually have the hook enabled, things are obviously fine.");
                    println!("Alternatively, you have to make sure that our configuration at /etc/modprobe.d/vfio.conf (creating right now) is properly applied.");
                    if !ask::yesno(&mut self.stdin, "Done?") {
                        println!("Aborted.");
                        return;
                    }
                } else {
                    skip_ask = true;
                }
            } else {
                println!("Falling back to manual mode.");
                println!("");
                println!("Please configure your initramfs generator to load these kernel modules: {}", KERNEL_MODULES);
                println!("Make sure that they are loaded *before* any graphics drivers!");
                println!("For mkinitcpio users, adding them at the *start* of your MODULES line in /etc/mkinitcpio.conf will take care of this.");
                println!("");
                if !ask::yesno(&mut self.stdin, "Done?") {
                    println!("Aborted.");
                    return;
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

            self.write_vfio_modconf(&setup);

            if !skip_ask {
                if !ask::yesno(&mut self.stdin, "Done?") {
                    println!("Aborted.");
                    return;
                }
            }

            println!("");
            println!("Step 3: Update initramfs");
            let mut skip_ask = false;
            if ask::yesno(&mut self.stdin, "Are you using the default kernel ('linux')?") {
                let status = Command::new("/usr/bin/sudo").arg("/usr/bin/mkinitcpio")
                    .arg("-p").arg("linux").status().expect("Failed to run mkinitcpio");
                if !status.success() {
                    println!("Got an error from mkinitcpio. Sorry, but you have to fix this on your own.");
                } else {
                    skip_ask = true;
                }
            } else {
                println!("Please run your initramfs generator now and verify that everything works.");
            }

            if !skip_ask {
                if !ask::yesno(&mut self.stdin, "Done?") {
                    println!("Aborted.");
                    return;
                }
            }

            // TODO: hugepages part 1 (as you have to reboot)

            setup.reboot_commanded = true;
            make_config(&machine, &setup).save(target);

            println!("");
            println!("Step 4: Reboot");
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
            println!("");
            println!("With that out of the way, your next step after the reboot is simply to launch this wizard again and we can move on!");
        } else { // if something is actually passed through properly
            println!("Step 5: Check IOMMU grouping");
            if !self.check_iommu_grouping(&setup).expect("Failed to check IOMMU grouping") {
                return;
            }
            println!("");

            println!("Step 6: VM setup");
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

            // TODO: hugepages part 2
            if machine.memory == "" {
                // FIXME: validate this
                machine.memory = ask::anything(&mut self.stdin, "How much memory would you like to assign to it?",
                                              "be careful no validation LUL", |x| Some(x.to_owned()));
            }

            {
                if env::var("DISPLAY").is_ok() {
                    println!("It seems you're running this setup in a graphical environment. This can make things a lot easier!");
                    println!("While our objective is of course VGA passthrough, running a virtual display during setup is very convenient for many reasons. We strongly recommend using this.");
                    if ask::yesno(&mut self.stdin, "Would you like to enable virtual graphics (only during setup)?") {
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
                    path: ask::file(&mut self.stdin, "Please enter the path to the VM root block device"),
                });
                // TODO: Support multiple storage devices
                // TODO: Support qcow2 images

                make_config(&machine, &setup).save(target);

                // Stage 1: First boot and Windows setup

                // TODO: if installed, call windows10-get-download-link, wget to a temporary location and go on
                setup.cdrom = Some(ask::file(&mut self.stdin, "Please enter the path to your Windows ISO"));
                setup.floppy = Some(datadir.join("virtio-win.vfd").to_str().unwrap().to_owned());
            }
            println!("Your VM is going to boot now.");
            println!("Just install Windows and shut it down cleanly as soon as that's done so we can continue.");
            println!("");
            println!("Note: Windows probably won't pick up the virtio-scsi storage device right away. You can load the drivers from the attached floppy drive.");
            if !ask::yesno(&mut self.stdin, "Ready?") {
                println!("Aborted.");
                return;
            }

            qemu::run(&make_config(&machine, &setup), workdir, datadir);

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

static KERNEL_MODULES: &'static str = "vfio vfio_iommu_type1 vfio_pci vfio_virqfd";

fn is_iommu_enabled() -> bool {
    read_dir("/sys/devices/virtual/iommu/").ok().and_then(|mut x| x.next()).is_some()
}

fn sudo_write_file<P: AsRef<Path>, F: FnOnce(&mut ChildStdin) -> IoResult<()>>(path: P, write: F) -> IoResult<bool> {
    let mut writer_child = Command::new("/usr/bin/sudo").env("SUDO_EDITOR", "/usr/bin/tee")
        .arg("-e").arg(path.as_ref().to_str().unwrap()).stdin(Stdio::piped()).stdout(Stdio::null()).spawn()?;
    {
        let mut writer = writer_child.stdin.as_mut().unwrap();
        write(writer)?;
    }
    Ok(writer_child.wait()?.success())
}

pub fn run(cfg: Option<Config>, target: &Path, workdir: &Path, datadir: &Path) {
    let stdin = io::stdin();
    Wizard {
        stdin: stdin.lock(),
        udev: Context::new().expect("Failed to create udev context"),
    }.run(cfg, target, workdir, datadir);
}
