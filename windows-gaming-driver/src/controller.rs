use std::os::unix::net::UnixStream;
use std::io::prelude::*;
use std::mem;
use std::ffi::OsStr;
use std::process::Command;

use itertools::Itertools;
use serde_json;
use libudev::{Result as UdevResult, Context, Enumerator};
use byteorder::{WriteBytesExt, LittleEndian};

use config::{DeviceId, UsbBinding, MachineConfig, HotKeyAction, Action};
use util;

#[derive(PartialEq, Eq, Clone, Copy)]
/// States the state machine of this Controller can have
enum State {
    /// GA is down
    Down,
    /// GA is up
    Up,
    /// We wait for a ping response
    Pinging,
    /// Windows is currently suspending
    Suspending,
    /// Windows is currently waking up from suspend
    Resuming,
}

pub struct Controller {
    machine_config: MachineConfig,

    ga: State,
    io_attached: bool,

    // write-only
    monitor: UnixStream,
    clientpipe: UnixStream,
}

#[derive(Serialize)]
#[serde(tag = "execute", content = "arguments", rename_all = "snake_case")]
enum QmpCommand {
    QmpCapabilities,
    DeviceAdd {
        driver: &'static str,
        id: String,
        bus: String,
        port: usize,
        hostbus: String,
        hostaddr: String,
    },
    DeviceDel { id: String },
    SystemPowerdown,
    SystemWakeup,
}

enum GaCmd {
    Ping = 0x01,
    RegisterHotKey = 0x02,
}

fn writemon(monitor: &mut UnixStream, command: &QmpCommand) {
    let cmd = serde_json::to_string(command).unwrap();
    writeln!(monitor, "{}", cmd).expect("Failed to write to monitor");
}

impl Controller {
    fn write_ga(&mut self, cmd: GaCmd) {
        self.clientpipe.write_all(&[cmd as u8]).expect("Failed to write to clientpipe");
    }
    fn write_ga_buf(&mut self, cmd: GaCmd, buf: &[u8]) {
        self.write_ga(cmd);
        self.clientpipe.write_all(buf).expect("Failed to write to clientpipe");
    }

    pub fn new(machine_config: MachineConfig,
               monitor: &UnixStream,
               clientpipe: &UnixStream) -> Controller {
        let mut monitor = monitor.try_clone().unwrap();
        writemon(&mut monitor, &QmpCommand::QmpCapabilities);
        Controller {
            machine_config,

            ga: State::Down,
            io_attached: false,

            monitor,
            clientpipe: clientpipe.try_clone().unwrap(),
        }
    }

    pub fn ga_ping(&mut self) -> bool {
        // the idea is that someone else (timer) calls this periodically
        match self.ga {
            State::Pinging => {
                // the last ping wasn't even answered
                // we conclude that the ga has died
                self.ga = State::Down;
                self.io_detach();
                false
            }
            State::Up => {
                self.ga = State::Pinging;
                self.write_ga(GaCmd::Ping);
                true
            }
            _ => false,
        }
    }

    pub fn ga_pong(&mut self) {
        if self.ga == State::Pinging {
            self.ga = State::Up;
        }
    }

    pub fn ga_hello(&mut self) -> bool {
        ::sd_notify::notify_systemd(true, "Ready");

        // send GA all hotkeys we want to register
        for (i, &(ref key, _)) in self.machine_config.hotkeys.clone().iter().enumerate() {
            let mut buf = Vec::new();
            buf.write_u32::<LittleEndian>(i as u32).unwrap();
            buf.write_u32::<LittleEndian>(key.len() as u32).unwrap();
            buf.extend(key.bytes());
            self.write_ga_buf(GaCmd::RegisterHotKey, &buf);
        }

        // Whenever a ga_hello message arrives, we know that the GA just started.
        // Typically, it would be the initial launch after boot but it might also be
        // a restart. There is even the racy case where it restarts before responding
        // to our ping - if we're not careful we might end up timeouting the new GA
        // instance for missing a ping we sent before it even existed!
        //
        // To handle this, we just make sure that there's no lingering ping AND
        // we return false if we didn't notice the GA going down as the timer still
        // exists in that case so it would be a bug to create a second one.

        let ga = mem::replace(&mut self.ga, State::Up);
        match ga {
            State::Pinging | State::Up => false,
            State::Down => true,
            State::Resuming => {
                self.io_attach();
                true
            }
            State::Suspending => false, // wtf though
        }
    }

    pub fn ga_suspending(&mut self) {
        self.io_detach();
        self.ga = State::Suspending;
    }

    pub fn ga_hotkey(&mut self, index: u32) {
        match self.machine_config.hotkeys.get(index as usize).cloned() {
            None => println!("Client sent invalid hotkey id"),
            Some((_, HotKeyAction::Action(action))) => self.action(action),
            Some((_, HotKeyAction::Exec(cmd))) => {
                Command::new("/bin/sh").arg("-c").arg(&cmd).spawn().unwrap();
            }
        }
    }

    /// Executes given action
    pub fn action(&mut self, action: Action) {
        match action {
            Action::IoEntry => self.io_attach(),
            Action::IoEntryForced => self.io_force_attach(),
            Action::IoExit => self.io_detach(),
        }
    }

    /// Attaches all configured devices if GA is up and wakes the host up if it's suspended
    pub fn io_attach(&mut self) {
        match self.ga {
            State::Down | State::Resuming => (),
            State::Suspending => {
                // make them wake up
                writemon(&mut self.monitor, &QmpCommand::SystemWakeup);
                // can't enter now - gotta wait for GA to get ready
                self.ga = State::Resuming;
            },
            State::Up | State::Pinging => self.io_force_attach()
        }
    }

    /// Attaches all configured devices regardless of GA state
    pub fn io_force_attach(&mut self) {
        if self.io_attached {
            return;
        }

        let mut udev = Context::new().expect("Failed to create udev context");

        let mut sorted = self.machine_config.usb_devices.iter().enumerate()
            .sorted_by(|&(_, a), &(_, b)| a.bus.cmp(&b.bus));
        let groups = sorted.drain(..).group_by(|&(_, dev)| dev.bus);
        for (port, (i, dev)) in groups.into_iter().flat_map(|(_, group)| group.enumerate())
                .filter(|&(_, (_, ref dev))| !dev.permanent) {
            if let Some((hostbus, hostaddr)) = udev_resolve_binding(&mut udev, &dev.binding)
                    .expect("Failed to resolve usb binding") {
                let bus = dev.bus;
                let usable_ports = util::usable_ports(bus);
                writemon(&mut self.monitor, &QmpCommand::DeviceAdd {
                    driver: "usb-host",
                    bus: format!("{}{}.0", bus, port / usable_ports),
                    port: (port % usable_ports) + 1,
                    id: format!("usb{}", i),
                    hostbus: hostbus,
                    hostaddr: hostaddr,
                });
            }
        }
        self.io_attached = true;
    }

    /// Detaches all configured devices
    pub fn io_detach(&mut self) {
        assert!(self.ga != State::Suspending, "trying to exit from a suspended vm?");
        if !self.io_attached {
            return;
        }
        for i in self.machine_config.usb_devices.iter().enumerate()
            .filter(|&(_, dev)| !dev.permanent).map(|(i, _)| i) {
            writemon(&mut self.monitor, &QmpCommand::DeviceDel { id: format!("usb{}", i) });
        }
        self.io_attached = false;
    }

    pub fn shutdown(&mut self) {
        writemon(&mut self.monitor, &QmpCommand::SystemPowerdown);
    }
}

/// Resolves a `UsbBinding` to a (bus, addr) tuple.
pub fn udev_resolve_binding(udev: &Context, binding: &UsbBinding)
                        -> UdevResult<Option<(String, String)>> {
    let mut iter = Enumerator::new(udev).unwrap();

    iter.match_subsystem("usb")?;
    iter.match_property("DEVTYPE", "usb_device")?;

    match binding {
        &UsbBinding::ById(DeviceId { vendor, product }) => {
            iter.match_attribute("idVendor", format!("{:04x}", vendor))?;
            iter.match_attribute("idProduct", format!("{:04x}", product))?;
        }
        &UsbBinding::ByPort { bus, port } => {
            iter.match_attribute("busnum", bus.to_string())?;
            iter.match_attribute("devpath", port.to_string())?;
        }
    }

    let mut scanner = iter.scan_devices().unwrap();
    // FIXME: rust-lang/rust#42222
    return match scanner.next() {
        Some(dev) => {
            let mut bus = None;
            let mut addr = None;
            for attr in dev.attributes() {
                if let Some(val) = attr.value().and_then(OsStr::to_str) {
                    if attr.name() == "busnum" {
                        bus = Some(val.to_owned());
                    } else if attr.name() == "devnum" {
                        addr = Some(val.to_owned());
                    }
                }
            }

            if scanner.next().is_some() {
                println!("Warning: Multiple matches for {:?} found.", binding);
                println!("         Binding to the first one we see, just like qemu would.");
            }

            Ok(Some((bus.unwrap(), addr.unwrap())))
        }
        None => {
            println!("Warning: Didn't find any devices for {:?}", binding);
            Ok(None)
        }
    };
}

/// Resolves a `UsbBinding` to a (bus, addr) tuple.
///
/// This is just a wrapper around `udev_resolve_binding` creating a new udev context.
pub fn resolve_binding(binding: &UsbBinding) -> UdevResult<Option<(String, String)>> {
    let udev = Context::new().expect("Failed to create udev context");
    udev_resolve_binding(&udev, binding)
}
