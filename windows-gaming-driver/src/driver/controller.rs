use std::mem;
use std::rc::Rc;
use std::ffi::OsStr;
use std::cell::RefCell;
use std::process::Command;

use itertools::Itertools;
use libudev::{Result as UdevResult, Context, Enumerator};
use futures::unsync::mpsc::UnboundedSender;
use futures::unsync::oneshot::{self, Sender};
use futures::Future;
use futures::future;
use clipboard::{ClipboardContext, ClipboardProvider};

use config::{UsbId, UsbPort, UsbBinding, MachineConfig, HotKeyAction, Action};
use util;
use driver::clientpipe::GaCmdOut as GaCmd;
use driver::monitor::QmpCommand;
use driver::sd_notify;
use driver::libinput::Input;

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
    /// Windows is suspended
    Suspended,
    /// Windows is currently waking up from suspend
    Resuming,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum IoState {
    Detached,
    LightEntry,
    AwaitingUpgrade,
    FullEntry,
}

pub struct Controller {
    machine_config: MachineConfig,

    ga: State,
    io_state: IoState,
    suspend_senders: Vec<Sender<()>>,

    input: Rc<RefCell<Input>>,

    // write-only
    monitor: UnboundedSender<QmpCommand>,
    clientpipe: UnboundedSender<GaCmd>,

    clipboard: ClipboardContext,
}

impl Controller {
    fn write_ga(&mut self, cmd: GaCmd) {
        (&self.clientpipe).send(cmd).unwrap();
    }

    pub fn new(machine_config: MachineConfig,
               monitor: UnboundedSender<QmpCommand>,
               clientpipe: UnboundedSender<GaCmd>,
               input: Rc<RefCell<Input>>,
               clipboard: ClipboardContext) -> Controller {
        (&monitor).send(QmpCommand::QmpCapabilities).unwrap();
        Controller {
            machine_config,

            ga: State::Down,
            io_state: IoState::Detached,
            suspend_senders: Vec::new(),

            monitor,
            clientpipe,
            input,

            clipboard,
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
        sd_notify::notify_systemd(true, "Ready");

        // send GA all hotkeys we want to register
        for (i, hotkey) in self.machine_config.hotkeys.clone().into_iter().enumerate() {
            self.write_ga(GaCmd::RegisterHotKey {
                id: i as u32,
                key: hotkey.key.to_windows(),
            });
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

        if self.io_state == IoState::AwaitingUpgrade {
            self.io_attach();
        }

        match ga {
            State::Pinging | State::Up => false,
            State::Down | State::Suspended => true,
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

    pub fn qemu_suspended(&mut self) {
        info!("Windows is now suspended");
        self.ga = State::Suspended;
        for sender in self.suspend_senders.drain(..) {
            let _ = sender.send(());
        }
    }

    pub fn ga_hotkey(&mut self, index: u32) {
        match self.machine_config.hotkeys.get(index as usize).map(|h| h.action.clone()) {
            None => warn!("Client sent invalid hotkey id"),
            Some(HotKeyAction::Action(action)) => self.action(action),
            Some(HotKeyAction::Exec(cmd)) => {
                Command::new("/bin/sh").arg("-c").arg(&cmd).spawn().unwrap();
            }
        }
    }

    /// Executes given action
    pub fn action(&mut self, action: Action) {
        match action {
            Action::IoUpgrade => self.io_attach(),
            Action::IoEntryForced => self.io_force_attach(),
            Action::IoExit => self.io_detach(),
        }
    }

    /// Attaches all configured devices if GA is up and wakes the host up if it's suspended
    pub fn io_attach(&mut self) {
        match self.ga {
            State::Resuming | State::Suspending => (),
            State::Suspended => {
                // make them wake up
                (&self.monitor).send(QmpCommand::SystemWakeup).unwrap();
                // can't enter now - gotta wait for GA to get ready
                self.ga = State::Resuming;
            },
            State::Down => {
                self.light_attach();
                self.io_state = IoState::AwaitingUpgrade;
            }
            State::Up | State::Pinging => self.io_force_attach(),
        }
    }

    pub fn try_attach(&mut self) {
        match self.ga {
            State::Up | State::Pinging => self.io_force_attach(),
            _ => (),
        }
    }

    pub fn light_attach(&mut self) {
        debug!("light entry");
        match self.io_state {
            IoState::Detached => {
                self.prepare_entry();
                self.input.borrow_mut().resume();
                self.io_state = IoState::LightEntry;
            }
            IoState::AwaitingUpgrade => self.io_state = IoState::LightEntry,
            IoState::LightEntry | IoState::FullEntry => (),
        }
    }

    /// Attaches all configured devices regardless of GA state
    pub fn io_force_attach(&mut self) {
        debug!("full entry");

        // release light entry first so we don't mess things up
        match self.io_state {
            IoState::Detached => (),
            IoState::AwaitingUpgrade | IoState::LightEntry => self.input.borrow_mut().suspend(),
            IoState::FullEntry => return,
        }

        self.prepare_entry();

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
                (&self.monitor).send(QmpCommand::DeviceAdd {
                    driver: "usb-host",
                    bus: format!("{}{}.0", bus, port / usable_ports),
                    port: (port % usable_ports) + 1,
                    id: format!("usb{}", i),
                    hostbus: hostbus,
                    hostaddr: hostaddr,
                }).unwrap();
            }
        }

        self.io_state = IoState::FullEntry;
    }

    pub fn prepare_entry(&mut self) {
        // release modifiers
        self.write_ga(GaCmd::ReleaseModifiers);

        // send clipboard
        match self.clipboard.get_contents() {
            Ok(s) => {
                let s = s.replace('\n', "\r\n");
                debug!("send clipboard to GA: {}", s.chars().take(100).collect::<String>());
                self.write_ga(GaCmd::SetClipboardText(s));
            },
            Err(e) => error!("Could not access clipboard: {}", e.description())
        }
    }

    /// Asks Windows to provide us with its clipboard contents
    pub fn ask_clipboard(&mut self) {
        self.write_ga(GaCmd::GetClipboard);
    }

    /// Sets Linux's clipboard to the provided value
    pub fn set_clipboard(&mut self, s: String) {
        let s = s.replace("\r\n", "\n");
        match self.clipboard.set_contents(s) {
            Ok(()) => (),
            Err(e) => error!("Could not set clipboard: {}", e.description())
        }
    }

    /// Suspends Windows
    pub fn suspend(&mut self) -> Box<Future<Item=(), Error=()>> {
        if self.ga == State::Suspended {
            // we are already suspended, return a resolved future
            return Box::new(future::ok(()));
        }

        if self.ga != State::Suspending {
            // only need to write suspend command to qemu if the system is not already suspending
            self.write_ga(GaCmd::Suspend);
        }

        let (sender, receiver) = oneshot::channel();
        self.suspend_senders.push(sender);
        Box::new(receiver.map_err(|_| ()))
    }

    /// Detaches all configured devices
    pub fn io_detach(&mut self) {
        assert!(self.ga != State::Suspending, "trying to exit from a suspending vm?");
        assert!(self.ga != State::Suspended, "trying to exit from a suspended vm?");
        self.ask_clipboard();

        match self.io_state {
            IoState::Detached => (),
            IoState::AwaitingUpgrade | IoState::LightEntry => {
                debug!("detaching light entry");
                self.input.borrow_mut().suspend()
            },
            IoState::FullEntry => {
                debug!("detaching full entry");
                for i in self.machine_config.usb_devices.iter().enumerate()
                        .filter(|&(_, dev)| !dev.permanent).map(|(i, _)| i) {
                    (&self.monitor).send(QmpCommand::DeviceDel { id: format!("usb{}", i) }).unwrap();
                }
            }
        }

        self.io_state = IoState::Detached;
    }

    pub fn shutdown(&mut self) {
        (&self.monitor).send(QmpCommand::SystemPowerdown).unwrap();
    }
}

/// Resolves a `UsbBinding` to a (bus, addr) tuple.
pub fn udev_resolve_binding(udev: &Context, binding: &UsbBinding)
                        -> UdevResult<Option<(String, String)>> {
    let mut iter = Enumerator::new(udev).unwrap();

    iter.match_subsystem("usb")?;
    iter.match_property("DEVTYPE", "usb_device")?;

    match binding {
        &UsbBinding::ById(UsbId { vendor, product }) => {
            iter.match_attribute("idVendor", format!("{:04x}", vendor))?;
            iter.match_attribute("idProduct", format!("{:04x}", product))?;
        }
        &UsbBinding::ByPort(UsbPort { bus, ref port }) => {
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
                warn!("Multiple matches for {:?} found. Binding to the first one we see,\
                 just like qemu would.", binding);
            }

            Ok(Some((bus.unwrap(), addr.unwrap())))
        }
        None => {
            warn!("Didn't find any devices for {:?}", binding);
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
