use std::os::unix::net::UnixStream;
use std::io::prelude::*;

pub struct Controller {
    ga_up: bool,
    ga_pong_expected: bool,

    io_attached: bool,

    // write-only
    monitor: UnixStream,
    clientpipe: UnixStream,
}

enum GaCmd {
    Ping = 0x01,
}

impl Controller {
    fn write_ga(&mut self, cmd: GaCmd) {
        self.clientpipe.write_all(&[cmd as u8]).expect("Failed to write to clientpipe");
    }

    fn writemon(&mut self, command: &str) {
        writeln!(self.monitor, "{}", command).expect("Failed to write to monitor");
    }

    pub fn new(monitor: &UnixStream, clientpipe: &UnixStream) -> Controller {
        Controller {
            ga_up: false,
            ga_pong_expected: false,

            io_attached: false,

            monitor: monitor.try_clone().unwrap(),
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
                    self.writemon("device_add usb-host,vendorid=0x1532,productid=0x0024,id=mouse");
                    self.writemon("device_add usb-host,vendorid=0x1532,productid=0x011a,id=kbd");
                }
                (true, false) => {
                    // detach
                    self.writemon("device_del kbd");
                    self.writemon("device_del mouse");
                }
                _ => (),
            }
            self.io_attached = state;
        }
    }

    pub fn shutdown(&mut self) {
        self.writemon("system_powerdown");
    }
}
