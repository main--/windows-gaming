use std::process::{Command, Stdio};
use std::path::{Path};
use std::iter::Iterator;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt as UnixCommandExt;

use itertools::Itertools;
use tokio_core::reactor::Handle;
use tokio_process::{CommandExt, Child};
use libc;

use common::config::{Config, SoundBackend, AlsaUnit, UsbBus};
use controller;
use sd_notify::notify_systemd;
use samba;
use common::util;

const QEMU: &str = "/usr/bin/qemu-system-x86_64";

fn supports_display(kind: &str) -> bool {
    Command::new(QEMU).args(&["-display", kind, "-version"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .status().unwrap().success()
}

pub fn has_gtk_support() -> bool {
    supports_display("gtk")
}

pub fn run(cfg: &Config, tmp: &Path, data: &Path, clientpipe_path: &Path, monitor_path: &Path,
           handle: &Handle, enable_gui: bool) -> Child {
    trace!("qemu::run");
    let machine = &cfg.machine;

    let efivars_file = tmp.join("efivars.fd");
    fs::copy(data.join("ovmf-vars.fd"), &efivars_file).expect("Failed to copy efivars image");
    trace!("copied efivars file");

    let mut usernet = format!("user,id=unet,restrict=on,guestfwd=tcp:10.0.2.1:31337-unix:{}",
                              clientpipe_path.display());

    if let Some(ref samba) = cfg.samba {
        trace!("setting up samba");
        samba::setup(&tmp, samba, &mut usernet);
        debug!("Samba started");
    }

    let ga_iso = data.join("windows-gaming-ga.iso");
    assert!(ga_iso.exists());

    notify_systemd(false, "Starting qemu ...");
    trace!("starting qemu setup");
    let mut qemu = Command::new(QEMU);
    qemu.args(&["-enable-kvm",
                "-machine",
                "pc-q35-6.1",
                "-cpu",
                "host,kvm=off,hv_time,hv_relaxed,hv_vapic,hv_spinlocks=0x1fff,\
                 hv_vendor_id=NvidiaFuckU",
                "-rtc",
                "base=localtime",
                "-nodefaults",
                "-net",
                "none",
                "-display", "none", "-vga", "none",
                "-qmp",
                &format!("unix:{}", monitor_path.display()),
                "-drive",
                &format!("if=pflash,format=raw,readonly,file={}",
                         data.join("ovmf-code.fd").display()),
                "-drive",
                &format!("if=pflash,format=raw,file={}", efivars_file.display()),
                "-object", "iothread,id=ioth0",
                "-device", "virtio-scsi-pci,id=scsi,iothread=ioth0",
                "-drive", &format!("if=none,id=iso,media=cdrom,file={}", ga_iso.display()),
                "-device", "scsi-cd,id=cdrom,drive=iso",
    ]);
    // TODO: make root ports in the VM for PCIe devices to ensure it's all natural to the guest OS and drivers

    if enable_gui {
        qemu.args(&["-display", "gtk", "-vga", "qxl"]);
        debug!("Applied gtk to qemu");
    }

    if let Some(ref setup) = cfg.setup {
        if let Some(ref cdrom) = setup.cdrom {
            qemu.arg("-cdrom").arg(cdrom);
            debug!("Forward cdrom {:?}", cdrom);
        }

        if let Some(ref floppy) = setup.floppy {
            qemu.arg("-drive").arg(format!("file={},index=0,if=floppy,readonly", floppy));
            debug!("Forward floppy {:?}", floppy);
        }
    }


    if machine.hugepages.unwrap_or(false) {
        qemu.args(&["-mem-path", "/dev/hugepages_vfio_1G/", "-mem-prealloc"]);
        debug!("Enabled hugepages");
    }

    trace!("Memory: {}", machine.memory);
    qemu.args(&["-m", &machine.memory]);
    trace!("Threads: {}, {}", machine.cores, machine.threads.unwrap_or(1));
    qemu.args(&["-smp",
                &format!("cores={},threads={}",
                         machine.cores,
                         machine.threads.unwrap_or(1))]);
    trace!("use hda sound hardware");
    qemu.args(&["-soundhw", "hda"]);

    for (idx, bridge) in machine.network.iter().flat_map(|x| x.bridges.iter()).enumerate() {
        trace!("setup bridge {}", bridge);
        qemu.args(&["-netdev",
                    &format!("bridge,id=bridge{},br={}", idx, bridge),
                    "-device",
                    &format!("e1000,netdev=bridge{}", idx)]);
    }
    trace!("setup usernet");
    qemu.args(&["-netdev", &usernet, "-device", "e1000,netdev=unet"]);

    // TODO: Check if the configured device is in the configured slot
    for device in cfg.machine.pci_devices.iter() {
        if device.resettable {
            let mut child = Command::new(data.join("vfio-ubind")).arg(&device.slot).spawn().expect("failed to run vfio-ubind");
            match child.wait() {
                Ok(status) if !status.success() =>
                    panic!("vfio-ubind failed with {}! The device might not be bound to the \
                            vfio-driver and therefore not function correctly", status),
                Err(err) => panic!("failed to wait on child. Got: {}", err),
                _ => (),
            }
        }

        qemu.args(&["-device", &format!("vfio-pci,host={},multifunction=on", device.slot)]);
        debug!("Passed through {}", device.slot);
    }

    // create usb buses
    {
        let mut create_usb_buses = |name, typ, ports| {
            let mut count = cfg.machine.usb_devices.iter().filter(|dev| dev.bus == typ).count();

            if typ == UsbBus::Xhci {
                // account for lighthouse usb-mouse and usb-kbd
                count += 2;
            }

            let usable_ports = util::usable_ports(typ);
            let num = (count + usable_ports - 1) / usable_ports;
            debug!("Setup {} {:?} bus(es)", num, typ);
            for i in 0..num {
                let device = format!("{},id={}{}{}", name, typ, i, ports);
                trace!("Bus: {}", device);
                qemu.args(&["-device", &device]);
            }
        };
        create_usb_buses("pci-ohci", UsbBus::Ohci, ",num-ports=15");
        create_usb_buses("ich9-usb-uhci1", UsbBus::Uhci, "");
        create_usb_buses("ich9-usb-ehci1", UsbBus::Ehci, "");
        create_usb_buses("qemu-xhci", UsbBus::Xhci, ",p2=15,p3=15");
    }

    let sorted = cfg.machine.usb_devices.iter().sorted_by(|a, b| a.bus.cmp(&b.bus));
    let groups = sorted.iter().group_by(|dev| dev.bus);
    for (bus, devices) in &groups {
        let usable_ports = util::usable_ports(bus);
        let mut i = 0;
        for dev in devices {
            let port = i;
            i += 1;
            if dev.permanent {
            if let Some((hostbus, hostaddr)) = controller::resolve_binding(&dev.binding)
                    .expect("Failed to resolve usb binding")
                {
                    qemu.args(&["-device", &format!(
                        "usb-host,hostbus={},hostaddr={},bus={}{}.0,port={}", hostbus, hostaddr,
                        bus, port / usable_ports, (port % usable_ports) + 1)]);
                    debug!("Connected {:?} ({}:{}) to bus {}", dev.binding, hostbus, hostaddr, bus);
                }
            }
        }

        if bus == UsbBus::Xhci {
            // add lighthouse usb-mouse
            let port = i;
            qemu.args(&["-device", &format!("usb-mouse,bus=xhci{}.0,port={}",
                                            port / usable_ports, (port % usable_ports) + 1)]);
            debug!("usb-mouse at xhci{}.0p{}", port / usable_ports, (port % usable_ports) + 1);
            // add lighthouse usb-kbd
            let port = i + 1;
            qemu.args(&["-device", &format!("usb-kbd,bus=xhci{}.0,port={}",
                                            port / usable_ports, (port % usable_ports) + 1)]);
            debug!("usb-kbd at xhci{}.0p{}", port / usable_ports, (port % usable_ports) + 1);
        }
    }

    for (idx, drive) in machine.storage.iter().enumerate() {
        qemu.args(&["-drive",
                    &format!("file={},id=disk{},format={},if=none,cache={},aio=native",
                             drive.path,
                             idx,
                             drive.format,
                             drive.cache),
                    "-device",
                    &format!("scsi-hd,drive=disk{}", idx)]);
        debug!("Passed through {}", drive.path);
    }

    {
        trace!("Applying sound config");
        let sound = &cfg.sound;

        qemu.env("QEMU_AUDIO_TIMER_PERIOD", sound.timer_period.to_string());

        qemu.env("QEMU_AUDIO_DAC_VOICES", sound.output.voices.to_string());
        qemu.env("QEMU_AUDIO_DAC_TRY_POLL", if sound.output.use_polling { "1" } else { "0" });

        match &sound.output.fixed {
            &None => {
                qemu.env("QEMU_AUDIO_DAC_FIXED_SETTINGS", "0");
            }
            &Some(ref x) => {
                qemu.env("QEMU_AUDIO_DAC_FIXED_SETTINGS", "1");
                qemu.env("QEMU_AUDIO_DAC_FIXED_FREQ", x.frequency.to_string());
                qemu.env("QEMU_AUDIO_DAC_FIXED_FMT", &x.format);
                qemu.env("QEMU_AUDIO_DAC_FIXED_CHANNELS", x.channels.to_string());
            }
        }

        qemu.env("QEMU_AUDIO_ADC_VOICES", sound.input.voices.to_string());
        qemu.env("QEMU_AUDIO_ADC_TRY_POLL", if sound.output.use_polling { "1" } else { "0" });

        match &sound.input.fixed {
            &None => {
                qemu.env("QEMU_AUDIO_ADC_FIXED_SETTINGS", "0");
            }
            &Some(ref x) => {
                qemu.env("QEMU_AUDIO_ADC_FIXED_SETTINGS", "1");
                qemu.env("QEMU_AUDIO_ADC_FIXED_FREQ", x.frequency.to_string());
                qemu.env("QEMU_AUDIO_ADC_FIXED_FMT", &x.format);
                qemu.env("QEMU_AUDIO_ADC_FIXED_CHANNELS", x.channels.to_string());
            }
        }

        match &sound.backend {
            &SoundBackend::None => {
                qemu.env("QEMU_AUDIO_DRV", "none");
            }
            &SoundBackend::Alsa { ref sink, ref source } => {
                qemu.env("QEMU_AUDIO_DRV", "alsa");

                qemu.env("QEMU_ALSA_DAC_DEV", &sink.name);
                qemu.env("QEMU_ALSA_DAC_SIZE_IN_USEC",
                         if sink.unit == AlsaUnit::MicroSeconds { "1" } else { "0" });
                qemu.env("QEMU_ALSA_DAC_BUFFER_SIZE", sink.buffer_size.to_string());
                qemu.env("QEMU_ALSA_DAC_PERIOD_SIZE", sink.period_size.to_string());

                qemu.env("QEMU_ALSA_ADC_DEV", &source.name);
                qemu.env("QEMU_ALSA_ADC_SIZE_IN_USEC",
                         if source.unit == AlsaUnit::MicroSeconds { "1" } else { "0" });
                qemu.env("QEMU_ALSA_ADC_BUFFER_SIZE", source.buffer_size.to_string());
                qemu.env("QEMU_ALSA_ADC_PERIOD_SIZE", source.period_size.to_string());
            }
            &SoundBackend::PulseAudio {
                buffer_samples,
                ref server,
                ref sink_name,
                ref source_name,
            } => {
                qemu.env("QEMU_AUDIO_DRV", "pa");

                qemu.env("QEMU_PA_SAMPLES", buffer_samples.to_string());
                option2env(&mut qemu, "QEMU_PA_SERVER", server);
                option2env(&mut qemu, "QEMU_PA_SINK", sink_name);
                option2env(&mut qemu, "QEMU_PA_SOURCE", source_name);
            }
        }
    }

    if let Some(ref cmd) = cfg.additional_qemu_cmdline {
        qemu.args(cmd.split(' '));
    }

    qemu.stdin(Stdio::null());
    debug!("qemu: {:?}", qemu);

    // try to detach qemu from process group to enable better Ctrl+C support
    qemu.before_exec(|| unsafe {
        if libc::setpgid(0, 0) < 0 {
            warn!("Can't setpgid: {}", io::Error::last_os_error());
        } else {
            debug!("Detached qemu process");
        }
        Ok(())
    });
    let qemu = qemu.spawn_async(handle).expect("Failed to start qemu");
    trace!("qemu spawned");
    return qemu;
}

fn option2env(cmd: &mut Command, name: &str, val: &Option<String>) {
    match val {
        &None => cmd.env_remove(name),
        &Some(ref x) => cmd.env(name, &x),
    };
}
