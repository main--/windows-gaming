use std::os::unix::net::UnixStream;
use std::io::prelude::*;
use std::mem;
use serde_json;

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
    usb_devs: Vec<(u16, u16)>,

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
    DeviceAdd { driver: &'static str, id: String, vendorid: u16, productid: u16, bus: &'static str, port: usize },
    DeviceDel { id: String },
    SystemPowerdown,
    SystemWakeup,
}

enum GaCmd {
    Ping = 0x01,
}

fn writemon(monitor: &mut UnixStream, command: &QmpCommand) {
    let cmd = serde_json::to_string(command).unwrap();
    writeln!(monitor, "{}", cmd).expect("Failed to write to monitor");
}

impl Controller {
    fn write_ga(&mut self, cmd: GaCmd) {
        self.clientpipe.write_all(&[cmd as u8]).expect("Failed to write to clientpipe");
    }

    pub fn new(usb_devs: Vec<(u16, u16)>,
               monitor: &UnixStream,
               clientpipe: &UnixStream) -> Controller {
        let mut monitor = monitor.try_clone().unwrap();
        writemon(&mut monitor, &QmpCommand::QmpCapabilities);
        Controller {
            usb_devs,

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
        for (i, &(vendor, product)) in self.usb_devs.iter().enumerate() {
            writemon(&mut self.monitor, &QmpCommand::DeviceAdd {
                driver: "usb-host",
                bus: "xhci.0",
                port: i + 1,
                id: format!("usb{}", i),
                vendorid: vendor,
                productid: product,
            });
        }
        self.io_attached = true;
    }

    /// Detaches all configured devices
    pub fn io_detach(&mut self) {
        assert!(self.ga != State::Suspending, "trying to exit from a suspended vm?");
        if !self.io_attached {
            return;
        }
        for i in 0..self.usb_devs.len() {
            writemon(&mut self.monitor, &QmpCommand::DeviceDel { id: format!("usb{}", i) });
        }
        self.io_attached = false;
    }

    pub fn shutdown(&mut self) {
        writemon(&mut self.monitor, &QmpCommand::SystemPowerdown);
    }
}
