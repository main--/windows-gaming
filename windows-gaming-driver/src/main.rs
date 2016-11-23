extern crate systemd;
extern crate nix;
extern crate users;
extern crate toml;
extern crate rustc_serialize;

use std::cell::RefCell;
use std::rc::Rc;
use std::process::{Command, Stdio};
use std::fs::{copy, create_dir, remove_dir_all, set_permissions, File, Permissions};
use std::path::Path;
use std::io::{Write, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::iter::Iterator;
use std::fmt::Write as FmtWrite;

use users::get_user_by_name;

const TMP_FOLDER: &'static str = "/tmp/windows-gaming";
const DATA_FOLDER: &'static str = "/usr/lib/windows-gaming";

#[derive(RustcDecodable, Debug)]
struct Config {
    datadir: Option<String>,
    workdir: Option<String>,
    machine: MachineConfig,
    samba: Option<SambaConfig>,
}

#[derive(RustcDecodable, Debug)]
struct MachineConfig {
    memory: String,
    hugepages: Option<bool>,

    cores: u32,
    threads: Option<u32>,

    network: Option<NetworkConfig>,
    storage: Vec<StorageDevice>,
}

#[derive(RustcDecodable, Debug)]
struct StorageDevice {
    path: String,
    cache: String,
    format: String,
}

#[derive(RustcDecodable, Debug)]
struct NetworkConfig {
    bridges: Vec<String>, // TODO: custom usernet
}

#[derive(RustcDecodable, Debug)]
struct SambaConfig {
    user: String,
    path: String,
}

fn main() {
    println!("Hello, world!");

    let mut args = std::env::args().skip(1);
    let config_path = args.next();
    if args.next().is_some() {
        println!("Usage: windows-gaming-driver [conf]");
    }

    let mut config = String::new();
    {
        let mut config_file = File::open(config_path.as_ref()
                .map(|x| &x[..])
                .unwrap_or("/etc/windows-gaming-driver.toml"))
            .expect("Failed to open config file");
        config_file.read_to_string(&mut config).expect("Failed to read config file");
    }

    let mut parser = toml::Parser::new(&config);


    let parsed = match parser.parse() {
        Some(x) => x,
        None => {
            for e in parser.errors {
                println!("{}", e);
            }
            panic!("Failed to parse config");
        }
    };

    let cfg: Config = match rustc_serialize::Decodable::decode(
        &mut toml::Decoder::new(toml::Value::Table(parsed))) {
        Ok(x) => x,
        Err(e) => panic!("Failed to decode config: {}", e),
    };
    println!("{:?}", cfg);
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
        let user = get_user_by_name(&samba.user).unwrap();

        let samba_cfg = tmp.join("smbd.conf");
        let mut smbd_conf = File::create(&samba_cfg).expect("Failed to create smbd conf");
        let samba_folder = tmp.join("samba");
        write!(smbd_conf,
               r#"
[global]
private dir={0}
interfaces=127.0.0.1
bind interfaces only=yes
pid directory={0}
lock directory={0}
state directory={0}
cache directory={0}
ncalrpc dir={0}/ncalrpc
log file={0}/log.smbd
smb passwd file={0}/smbpasswd
security = user
map to guest = Bad User
load printers = no
printing = bsd
disable spoolss = yes
usershare max shares = 0
create mask = 0644
[qemu]
path={1}
read only=no
guest ok=yes
force user={2}
"#,
               samba_folder.display(),
               samba.path,
               samba.user)
            .expect("Failed to write smbd conf");

        create_dir(&samba_folder).expect("Failed to create samba folder");
        nix::unistd::chown(&samba_folder,
                           Some(user.uid()),
                           Some(user.primary_group_id()))
            .expect("Failed to chown samba folder");
        write!(usernet,
               ",guestfwd=tcp:10.0.2.1:445-cmd:sudo -u {} -- smbd --configfile {}",
               samba.user,
               samba_cfg.display())
            .unwrap();
    }

    notify_systemd(false, "Starting qemu ...");
    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.args(&["-enable-kvm",
                "-cpu",
                "host,kvm=off,hv_time,hv_relaxed,hv_vapic,hv_spinlocks=0x1fff,\
                 hv_vendor_id=NvidiaFuckU",
                "-rtc",
                "base=localtime",
                "-vga",
                "none",
                "-display",
                "none",
                "-serial",
                "none",
                "-parallel",
                "none",
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

    let csr = Rc::new(RefCell::new(clientpipe_stream));

    struct MonitorManager {
        stream: UnixStream,
        io_attached: bool,
    }

    impl MonitorManager {
        fn writemon(&mut self, command: &str) {
            writeln!(self.stream, "{}", command).expect("Failed too write to monitor");
        }

        fn set_io_attached(&mut self, state: bool) {
            if state != self.io_attached {
                if state {
                    // attach
                    self.writemon("device_add usb-host,vendorid=0x1532,productid=0x0024,id=mouse");
                    self.writemon("device_add usb-host,vendorid=0x1532,productid=0x011a,id=kbd");
                } else {
                    // detach
                    self.writemon("device_del kbd");
                    self.writemon("device_del mouse");
                }
                self.io_attached = state;
            }
        }

        fn shutdown(&mut self) {
            self.writemon("system_powerdown");
        }
    }

    impl Pollable for Rc<RefCell<MonitorManager>> {
        fn fd(&self) -> RawFd {
            self.borrow().stream.as_raw_fd()
        }
        fn run(&mut self) -> PollableResult {
            drain_stdout(&mut self.borrow_mut().stream)
        }
    }

    let mman = Rc::new(RefCell::new(MonitorManager {
        stream: monitor_stream,
        io_attached: false,
    }));

    struct ClientpipeHandler {
        monitor: Rc<RefCell<MonitorManager>>,
        clientpipe: Rc<RefCell<UnixStream>>,
    }

    impl Pollable for ClientpipeHandler {
        fn fd(&self) -> RawFd {
            self.clientpipe.borrow().as_raw_fd()
        }
        fn run(&mut self) -> PollableResult {
            match read_byte(&mut *self.clientpipe.borrow_mut()).expect("clientpipe read failed") {
                Some(1) => {
                    println!("client is now alive!");
                    notify_systemd(true, "Ready");
                }
                Some(2) => {
                    println!("client requests IO exit");
                    self.monitor.borrow_mut().set_io_attached(false);
                }
                Some(x) => println!("client sent invalid request {}", x),
                None => return PollableResult::Death,
            }
            PollableResult::Ok
        }
    }

    struct ControlServerHandler {
        monitor: Rc<RefCell<MonitorManager>>,
        clientpipe: Rc<RefCell<UnixStream>>,
        controlserver: UnixListener,
    }

    impl Pollable for ControlServerHandler {
        fn fd(&self) -> RawFd {
            self.controlserver.as_raw_fd()
        }

        fn run(&mut self) -> PollableResult {
            let (client, _) = self.controlserver.accept().unwrap();
            PollableResult::Child(Box::new(ControlClientHandler {
                monitor: self.monitor.clone(),
                clientpipe: self.clientpipe.clone(),
                client: client,
            }))
        }

        fn is_critical(&self) -> bool {
            false // this one will never shut down on its own
        }
    }

    struct ControlClientHandler {
        monitor: Rc<RefCell<MonitorManager>>,
        #[allow(dead_code)]
        clientpipe: Rc<RefCell<UnixStream>>,
        client: UnixStream,
    }

    impl Pollable for ControlClientHandler {
        fn fd(&self) -> RawFd {
            self.client.as_raw_fd()
        }
        fn run(&mut self) -> PollableResult {
            match read_byte(&mut self.client).expect("control channel read failed") {
                Some(1) => {
                    println!("IO entry requested!");
                    self.monitor.borrow_mut().set_io_attached(true);
                }
                Some(2) => {
                    self.monitor.borrow_mut().shutdown();
                }
                Some(x) => println!("control sent invalid request {}", x),
                None => return PollableResult::Death,
            }
            PollableResult::Ok
        }
    }

    poll_core(vec![Box::new(ClientpipeHandler {
                       monitor: mman.clone(),
                       clientpipe: csr.clone(),
                   }),
                   Box::new(ControlServerHandler {
                       monitor: mman.clone(),
                       clientpipe: csr.clone(),
                       controlserver: control_socket,
                   }),
                   Box::new(mman)]);

    qemu.wait().unwrap();
    println!("windows-gaming-driver down.");
}

enum PollableResult {
    Child(Box<Pollable>),
    Ok,
    Death,
}

trait Pollable {
    fn fd(&self) -> RawFd;
    fn run(&mut self) -> PollableResult;
    fn is_critical(&self) -> bool {
        true
    } // eventloop will run until all critical ones are gone
}

fn read_byte<T: Read>(thing: &mut T) -> std::io::Result<Option<u8>> {
    let mut buf = [0u8; 1];
    match thing.read(&mut buf)? {
        0 => Ok(None),
        1 => Ok(Some(buf[0])),
        _ => unreachable!(),
    }
}

fn poll_core<'a>(mut components: Vec<Box<Pollable>>) {
    while components.iter().any(|x| x.is_critical()) {
        use nix::poll::*;

        let mut deathlist = Vec::new();
        let mut newchildren = Vec::new();
        {

            let mut pollfds: Vec<_> = components.iter()
                .map(|x| PollFd::new(x.fd(), POLLIN, EventFlags::empty()))
                .collect();
            let poll_count = poll(&mut pollfds, -1).expect("poll failed");
            assert!(poll_count > 0);


            for (idx, (pollable, pollfd)) in components.iter_mut().zip(pollfds).enumerate() {
                let mut bits = pollfd.revents().unwrap();
                if bits.intersects(POLLIN) {
                    match pollable.run() {
                        PollableResult::Death => deathlist.push(idx),
                        PollableResult::Child(c) => newchildren.push(c),
                        PollableResult::Ok => (),
                    }
                }
                bits.remove(POLLIN);
                // assert!(bits.is_empty());
            }
        }

        // remove in reverse order so we don't mess up each subsequent index
        for &i in deathlist.iter().rev() {
            components.remove(i);
        }

        for c in newchildren {
            components.push(c);
        }

    }
}

fn drain_stdout<T: Read>(thing: &mut T) -> PollableResult {
    let mut buf = [0u8; 4096];
    let count = thing.read(&mut buf).expect("read failed");
    print!("{}", String::from_utf8_lossy(&buf[0..count]));

    if count == 0 {
        PollableResult::Death
    } else {
        PollableResult::Ok
    }
}

fn notify_systemd(ready: bool, status: &'static str) {
    use systemd::daemon::*;
    use std::collections::HashMap;

    let mut info = HashMap::new();
    info.insert(STATE_READY, if ready { "1" } else { "0" });
    info.insert(STATE_STATUS, status);

    // this returns false if we're not actually running inside systemd
    // we don't care about that though
    notify(false, info).expect("sd_notify failed");
}
