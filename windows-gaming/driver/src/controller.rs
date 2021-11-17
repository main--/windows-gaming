use std::mem;
use std::rc::Rc;
use std::ffi::OsStr;
use std::cell::RefCell;
use std::borrow::Cow;

use itertools::Itertools;
use libudev::{Result as UdevResult, Context, Enumerator};
use futures::unsync::mpsc::UnboundedSender;
use futures::unsync::oneshot::{self, Sender};
use futures::Future;
use futures::future;

use common::config::{Action, HooksConfig, HotKeyAction, MachineConfig, UsbBinding, UsbId, UsbPort};
use common::util;
use tokio::process::Command;
use crate::clientpipe::{GaCmdOut, ClipboardMessage, ClipboardType, RegisterHotKey, Point};
use crate::control::ControlCmdOut;
use crate::monitor::QmpCommand;
use crate::sd_notify;
use crate::libinput::Input;
use crate::clipboard::{ClipboardRequestEvent, ClipboardRequestResponse};
use crate::release_all_keys::EVENTS as RELEASE_ALL_KEYS;


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

#[derive(Clone)]
enum IoState {
    Detached,
    LightEntry,
    TemporaryLightEntry(UnboundedSender<ControlCmdOut>),
    AwaitingUpgrade,
    FullEntry,
}

pub struct Controller {
    machine_config: MachineConfig,
    hooks_config: HooksConfig,

    ga: State,
    io_state: IoState,
    // senders to be sent to when windows finished suspending
    suspend_senders: Vec<Sender<()>>,

    input: Rc<RefCell<Input>>,

    x11_clipboard: UnboundedSender<ClipboardRequestResponse>,
    x11_clipboard_grabber: UnboundedSender<()>,
    x11_clipboard_reader: UnboundedSender<ClipboardType>,
    win_clipboard_request: Option<ClipboardRequestEvent>,

    // write-only
    monitor: UnboundedSender<QmpCommand>,
    clientpipe: UnboundedSender<GaCmdOut>,
}

impl Controller {
    fn write_ga<C: Into<GaCmdOut>>(&mut self, cmd: C) {
        (&self.clientpipe).unbounded_send(cmd.into()).unwrap();
    }

    pub fn new(machine_config: MachineConfig,
               hooks_config: HooksConfig,
               monitor: UnboundedSender<QmpCommand>,
               clientpipe: UnboundedSender<GaCmdOut>,
               input: Rc<RefCell<Input>>,
               x11_clipboard: UnboundedSender<ClipboardRequestResponse>,
               x11_clipboard_grabber: UnboundedSender<()>,
               x11_clipboard_reader: UnboundedSender<ClipboardType>) -> Controller {
        Controller {
            machine_config,
            hooks_config,

            ga: State::Down,
            io_state: IoState::Detached,
            suspend_senders: Vec::new(),

            monitor,
            clientpipe,
            input,

            x11_clipboard,
            x11_clipboard_grabber,
            x11_clipboard_reader,
            win_clipboard_request: None,
        }
    }

    pub fn ga_ping(&mut self) -> bool {
        // the idea is that someone else (timer) calls this periodically
        match self.ga {
            State::Pinging => {
                // the last ping wasn't even answered
                // we conclude that the ga has died
                self.ga = State::Down;
                match self.io_state {
                    IoState::FullEntry => self.io_detach(),
                    IoState::TemporaryLightEntry(_) => self.temporary_exit(),
                    _ => ()
                }
                false
            }
            State::Up => {
                self.ga = State::Pinging;
                self.write_ga(GaCmdOut::Ping(()));
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
            let (modifiers, key) = hotkey.key.to_windows();
            self.write_ga(RegisterHotKey { id: i as u32, modifiers, key });
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

        if let IoState::AwaitingUpgrade = self.io_state {
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
            Some(HotKeyAction::Action(action)) => {
                if let IoState::TemporaryLightEntry(_) = self.io_state {
                    info!("Got action-hotkey while in temporary light entry. Ignoring.");
                    return;
                }
                match action {
                    Action::IoUpgrade => self.io_attach(),
                    Action::IoEntryForced => self.io_force_attach(),
                    Action::IoExit => self.io_detach(),
                }
            }
            Some(HotKeyAction::Exec(cmd)) => {
                Command::new("/bin/sh").arg("-c").arg(&cmd).spawn().unwrap();
            }
        }
    }

    /// Attaches all configured devices if GA is up and wakes the host up if it's suspended
    pub fn io_attach(&mut self) {
        match self.ga {
            State::Resuming | State::Suspending => (),
            State::Suspended => {
                // make them wake up
                (&self.monitor).unbounded_send(QmpCommand::SystemWakeup).unwrap();
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

    pub fn temporary_entry(&mut self, sender: UnboundedSender<ControlCmdOut>, x: i32, y: i32) -> bool {
        match self.ga {
            State::Up | State::Pinging => match self.io_state {
                IoState::Detached => {
                    self.write_ga(GaCmdOut::SetMousePosition(Point { x, y }));
                    self.light_attach();
                    self.io_state = IoState::TemporaryLightEntry(sender);
                    true
                }
                _ => false
            },
            _ => false
        }
    }

    pub fn temporary_exit(&mut self) {
        // only detach if we are not already detached
        if let IoState::TemporaryLightEntry(_) = self.io_state {
            self.io_detach();
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
            IoState::LightEntry | IoState::FullEntry | IoState::TemporaryLightEntry(_) => (),
        }
    }

    /// Attaches all configured devices regardless of GA state
    pub fn io_force_attach(&mut self) {
        debug!("full entry");

        // release light entry first so we don't mess things up
        match self.io_state {
            IoState::Detached => (),
            IoState::AwaitingUpgrade | IoState::LightEntry => self.input.borrow_mut().suspend(),
            IoState::TemporaryLightEntry(ref mut sender) => {
                self.input.borrow_mut().suspend();
                sender.unbounded_send(ControlCmdOut::TemporaryLightDetached).unwrap();
            }
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
                (&self.monitor).unbounded_send(QmpCommand::DeviceAdd {
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
        self.write_ga(GaCmdOut::ReleaseModifiers(()));
        // launch entry hook
        run_hook(&self.hooks_config.attach);
    }

    /// Suspends Windows
    pub fn suspend(&mut self) -> Box<dyn Future<Item=(), Error=()>> {
        if self.ga == State::Suspended {
            // we are already suspended, return a resolved future
            return Box::new(future::ok(()));
        }

        if self.ga != State::Suspending {
            // only need to write suspend command to qemu if the system is not already suspending
            self.write_ga(GaCmdOut::Suspend(()));
        }

        let (sender, receiver) = oneshot::channel();
        self.suspend_senders.push(sender);
        Box::new(receiver.map_err(|_| ()))
    }

    /// Detaches all configured devices
    pub fn io_detach(&mut self) {
        assert!(self.ga != State::Suspending, "trying to exit from a suspending vm?");
        assert!(self.ga != State::Suspended, "trying to exit from a suspended vm?");

        match self.io_state {
            IoState::Detached => (),
            IoState::AwaitingUpgrade | IoState::LightEntry | IoState::TemporaryLightEntry(_) => {
                debug!("detaching light entry");
                self.input.borrow_mut().suspend();
                (&self.monitor).unbounded_send(QmpCommand::InputSendEvent {
                    events: Cow::from(RELEASE_ALL_KEYS),
                }).unwrap();
            },
            IoState::FullEntry => {
                debug!("detaching full entry");
                for i in self.machine_config.usb_devices.iter().enumerate()
                        .filter(|&(_, dev)| !dev.permanent).map(|(i, _)| i) {
                    (&self.monitor).unbounded_send(QmpCommand::DeviceDel { id: format!("usb{}", i) }).unwrap();
                }
            }
        }

        self.io_state = IoState::Detached;
        run_hook(&self.hooks_config.detach);
    }

    pub fn shutdown(&mut self) {
        // if GA is up, use that to shut down instead of sending the ACPI message through qemu
        match self.ga {
            State::Up | State::Pinging => self.write_ga(GaCmdOut::Shutdown(())),
            _ => (&self.monitor).unbounded_send(QmpCommand::SystemPowerdown).unwrap(),
        }
    }

    /// Windows told us to grab the keyboard
    pub fn grab_x11_clipboard(&mut self) {
        (&self.x11_clipboard_grabber).unbounded_send(()).unwrap();
    }

    /// Paste on Windows, so we have to request contents
    pub fn read_x11_clipboard(&mut self, kind: ClipboardType) {
        (&self.x11_clipboard_reader).unbounded_send(kind).unwrap();
    }

    /// Windows asked what kind of data our clipboard has, X11 responded
    pub fn respond_x11_types(&mut self, types: Vec<ClipboardType>) {
        if let Some(event) = self.win_clipboard_request.take() {
            (&self.x11_clipboard).unbounded_send(event.reply_types(types)).unwrap();
        }
    }

    /// Pasting on Linux, Windows responded with contents
    pub fn respond_x11_clipboard(&mut self, buf: Vec<u8>) {
        if let Some(event) = self.win_clipboard_request.take() {
            (&self.x11_clipboard).unbounded_send(event.reply_data(buf)).unwrap();
        }
    }

    /// We lost the X11 clipboard, so we grab the Windows keyboard
    pub fn grab_win_clipboard(&mut self) {
        self.write_ga(ClipboardMessage::GrabClipboard(()));
    }

    /// Paste on Linux, so we have to request contents
    pub fn read_win_clipboard(&mut self, event: ClipboardRequestEvent) {
        self.write_ga(ClipboardMessage::RequestClipboardContents(event.desired_type().into()));
        self.win_clipboard_request = Some(event);
    }

    /*
    Unused right now, clipboard protocol will change to avoid the two-step dance.

    // Linux asked what kind of data our clipboard has, Windows responded
    pub fn respond_win_types(&mut self, types: Vec<ClipboardType>) {
        let types = types.into_iter().map(Into::into).collect();
        self.write_ga(ClipboardMessage::ContentTypes(ClipboardTypes { types }));
    }
    */

    /// Pasting on Windows, X11 responded with contents
    pub fn respond_win_clipboard(&mut self, buf: Vec<u8>) {
        self.write_ga(ClipboardMessage::ClipboardContents(buf));
    }

    pub fn mouse_edged(&mut self, x: i32, y: i32) {
        if let IoState::TemporaryLightEntry(ref mut sender) = self.io_state {
            sender.unbounded_send(ControlCmdOut::MouseEdged { x, y }).unwrap();
        }
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

fn run_hook(hook: &Option<String>) {
    if let Some(hook) = hook.as_ref() {
        match Command::new("/bin/sh").arg("-c").arg(&hook).spawn() {
            Ok(mut child) => {
                tokio::spawn(async move {
                    if let Err(e) = child.wait().await {
                        warn!("Error waiting for hook to return: {:?}", e);
                    }
                });
            }
            Err(e) => warn!("Error spawning hook: {:?}", e),
        }
    }
}
