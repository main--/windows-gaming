use std::process::{Command, Stdio};
use std::fs::{copy, create_dir, remove_dir_all, set_permissions, Permissions};
use std::path::{Path};
use std::os::unix::net::UnixListener;
use std::os::unix::fs::PermissionsExt;
use std::iter::Iterator;

use config::Config;
use sd_notify::notify_systemd;
use samba;
use mainloop;

const QEMU: &str = "/usr/bin/qemu-system-x86_64";

fn supports_display(kind: &str) -> bool {
    Command::new(QEMU).args(&["-display", kind, "-version"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .status().unwrap().success()
}

pub fn has_gtk_support() -> bool {
    supports_display("gtk")
}

pub fn run(cfg: &Config, tmp: &Path, data: &Path) {
    let machine = &cfg.machine;

    let _ = remove_dir_all(tmp); // may fail - we dont care
    create_dir(tmp).expect("Failed to create TMP_FOLDER"); // may not fail - has to be new

    let efivars_file = tmp.join("efivars.fd");
    copy(data.join("ovmf-vars.fd"), &efivars_file).expect("Failed to copy efivars image");

    let monitor_socket_file = tmp.join("monitor.sock");
    let monitor_socket = UnixListener::bind(&monitor_socket_file)
        .expect("Failed to create monitor socket");

    let clientpipe_socket_file = tmp.join("clientpipe.sock");
    let clientpipe_socket = UnixListener::bind(&clientpipe_socket_file)
        .expect("Failed to create clientpipe socket");

    let control_socket_file = tmp.join("control.sock");
    let control_socket = UnixListener::bind(&control_socket_file)
        .expect("Failed to create control socket");
    set_permissions(control_socket_file, Permissions::from_mode(0o777))
        .expect("Failed to set permissions on control socket");

    let mut usernet = format!("user,id=unet,restrict=on,guestfwd=tcp:10.0.2.1:31337-unix:{}",
                              clientpipe_socket_file.display());

    if let Some(ref samba) = cfg.samba {
        samba::setup(&tmp, samba, &mut usernet);
    }

    notify_systemd(false, "Starting qemu ...");
    let mut qemu = Command::new(QEMU);
    qemu.args(&["-enable-kvm",
                "-machine",
                "q35",
                "-cpu",
                "host,kvm=off,hv_time,hv_relaxed,hv_vapic,hv_spinlocks=0x1fff,\
                 hv_vendor_id=NvidiaFuckU",
                "-rtc",
                "base=localtime",
                "-nodefaults",
                "-usb",
                "-net",
                "none",
                "-display", "none", "-vga", "none",
                "-qmp",
                &format!("unix:{}", monitor_socket_file.display()),
                "-drive",
                &format!("if=pflash,format=raw,readonly,file={}",
                         data.join("ovmf-code.fd").display()),
                "-drive",
                &format!("if=pflash,format=raw,file={}", efivars_file.display()),
                "-device", "qemu-xhci,id=xhci",
                "-device",
                "virtio-scsi-pci,id=scsi"]);
    // "-monitor",
    // "telnet:127.0.0.1:31338,server,nowait"

    if let Some(ref setup) = cfg.setup {
        if setup.gui {
            qemu.args(&["-display", "gtk", "-vga", "qxl"]);
        }

        if let Some(ref cdrom) = setup.cdrom {
            qemu.arg("-cdrom").arg(cdrom);
        }

        if let Some(ref floppy) = setup.floppy {
            qemu.arg("-drive").arg(format!("file={},index=0,if=floppy,readonly", floppy));
        }
    }


    if machine.hugepages.unwrap_or(false) {
        qemu.args(&["-mem-path", "/dev/hugepages_vfio_1G/", "-mem-prealloc"]);
    }

    qemu.args(&["-m", &machine.memory]);
    qemu.args(&["-smp",
                &format!("cores={},threads={}",
                         machine.cores,
                         machine.threads.unwrap_or(1))]);
    qemu.args(&["-soundhw", "hda"]);

    for (idx, bridge) in machine.network.iter().flat_map(|x| x.bridges.iter()).enumerate() {
        qemu.args(&["-netdev",
                    &format!("bridge,id=bridge{},br={}", idx, bridge),
                    "-device",
                    &format!("e1000,netdev=bridge{}", idx)]);
    }
    qemu.args(&["-netdev", &usernet, "-device", "e1000,netdev=unet"]);

    for slot in cfg.machine.vfio_slots.iter() {
        qemu.args(&["-device", &format!("vfio-pci,host={},multifunction=on", slot)]);
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
    }

    qemu.stdin(Stdio::null());

    let mut qemu = qemu.spawn().expect("Failed to start qemu");

    let (monitor_stream, _) = monitor_socket.accept().expect("Failed to get monitor");
    drop(monitor_socket);

    let (clientpipe_stream, _) = clientpipe_socket.accept().expect("Failed to get clientpipe");
    drop(clientpipe_socket);

    notify_systemd(false, "Booting ...");

    mainloop::run(cfg, monitor_stream, clientpipe_stream, control_socket);

    qemu.wait().unwrap();
    println!("windows-gaming-driver down.");
}
