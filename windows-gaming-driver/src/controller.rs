use std::os::unix::net::UnixStream;
use std::io::prelude::*;
use serde_json;

pub struct Controller {
    usb_devs: Vec<(u16, u16)>,

    ga_up: bool,
    ga_pong_expected: bool,

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

            ga_up: false,
            ga_pong_expected: false,

            io_attached: false,

            monitor,
            clientpipe: clientpipe.try_clone().unwrap(),
        }
    }

    pub fn ga_ping(&mut self) -> bool {
        // the idea is that someone else (timer) calls this periodically
        assert!(self.ga_up);

        if self.ga_pong_expected {
            // the last ping wasn't even answered
            // we conclude that the ga has died
            self.ga_up = false;
            self.set_io_attached(false, true);
            false
        } else {
            self.ga_pong_expected = true;
            self.write_ga(GaCmd::Ping);
            true
        }
    }

    pub fn ga_pong(&mut self) {
        self.ga_pong_expected = false;
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

        self.ga_pong_expected = false;

        if self.ga_up {
            return false;
        }

        self.ga_up = true;
        true
    }

    pub fn set_io_attached(&mut self, state: bool, force: bool) {
        if self.ga_up || force {
            match (self.io_attached, state) {
                (false, true) => {
                    // attach
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
                }
                (true, false) => {
                    // detach
                    for i in 0..self.usb_devs.len() {
                        writemon(&mut self.monitor, &QmpCommand::DeviceDel { id: format!("usb{}", i) });
                    }
                }
                _ => (),
            }
            self.io_attached = state;
        }
    }

    pub fn shutdown(&mut self) {
        writemon(&mut self.monitor, &QmpCommand::SystemPowerdown);
    }
}
