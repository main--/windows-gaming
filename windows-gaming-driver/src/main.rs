extern crate systemd;
extern crate nix;
extern crate users;
extern crate toml;
extern crate rustc_serialize;
extern crate timerfd;

mod mainloop;
mod config;
mod sd_notify;
mod samba;
mod controller;

use std::process::{Command, Stdio};
use std::fs::{copy, create_dir, remove_dir_all, set_permissions, Permissions};
use std::path::Path;
use std::os::unix::net::UnixListener;
use std::os::unix::fs::PermissionsExt;
use std::iter::Iterator;
use config::Config;
use sd_notify::notify_systemd;

const TMP_FOLDER: &'static str = "/tmp/windows-gaming";
const DATA_FOLDER: &'static str = "/usr/lib/windows-gaming";

fn main() {
    let mut args = std::env::args().skip(1);
    let config_path = args.next();
    if args.next().is_some() {
        println!("Usage: windows-gaming-driver [conf]");
    }

    let cfg = Config::load(config_path.as_ref().map(|x| &x[..]).unwrap_or("/etc/windows-gaming-driver.toml"));

    let machine = cfg.machine;

    let tmp = Path::new(cfg.workdir.as_ref().map(|x| &x[..]).unwrap_or(TMP_FOLDER));
    let data = Path::new(cfg.datadir.as_ref().map(|x| &x[..]).unwrap_or(DATA_FOLDER));

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

    if let Some(samba) = cfg.samba {
        samba::setup(&tmp, &samba, &mut usernet);
    }

    notify_systemd(false, "Starting qemu ...");
    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.args(&["-enable-kvm",
                "-cpu",
                "host,kvm=off,hv_time,hv_relaxed,hv_vapic,hv_spinlocks=0x1fff,\
                 hv_vendor_id=NvidiaFuckU",
                "-rtc",
                "base=localtime",
                "-display",
                "none",
                "-nodefaults",
                "-usb",
                "-net",
                "none",
                "-monitor",
                &format!("unix:{}", monitor_socket_file.display()),
                "-drive",
                &format!("if=pflash,format=raw,readonly,file={}",
                         data.join("ovmf-code.fd").display()),
                "-drive",
                &format!("if=pflash,format=raw,file={}", efivars_file.display()),
                "-device",
                "virtio-scsi-pci,id=scsi"]);
    // "-monitor",
    // "telnet:127.0.0.1:31338,server,nowait"

    if machine.hugepages.unwrap_or(false) {
        qemu.args(&["-mem-path", "/dev/hugepages"]);
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

    qemu.args(&["-device",
                "vfio-pci,host=01:00.0,multifunction=on",
                "-device",
                "vfio-pci,host=01:00.1"]);

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

    mainloop::run(monitor_stream, clientpipe_stream, control_socket);

    qemu.wait().unwrap();
    println!("windows-gaming-driver down.");
}
