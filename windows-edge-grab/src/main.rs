extern crate xdg;
extern crate xcb;
extern crate byteorder;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;

mod config;

use std::os::unix::net::UnixStream;
use std::io::Write;
use std::thread;
use std::process::Command;
use std::sync::{Arc, Mutex};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use xcb::Connection;
use xcb::xproto::{self, Window};

use config::Config;

macro_rules! unwrap {
    ($e:expr) => {
        match $e {
            Ok(val) => val,
            Err(e) => {
                println!("Got error: {}", e);
                ::std::process::exit(1);
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    TryAttach,
    Attached,
    TryDetach,
    Detached,
}

fn main() {
    let xdg_dir = xdg::BaseDirectories::with_prefix("windows-gaming-driver").unwrap();
    let cfg_file = xdg_dir.place_config_file("windows-edge-grab.yml").unwrap();
    let config = Config::load(cfg_file).unwrap();
    let control_socket = xdg_dir.place_runtime_file("control.sock").unwrap();
    let mut writer = UnixStream::connect(control_socket).unwrap();
    let mut reader = writer.try_clone().unwrap();
    let (con, _) = Connection::connect(None).unwrap();
    let root = con.get_setup().roots().take(1).next().unwrap().root();
    register(&con, root);
    let state = Arc::new(Mutex::new(State::Detached));

    let state_r = state.clone();
    let cfg = config.clone();
    thread::spawn(move || {
        loop {
            match unwrap!(reader.read_u8()) {
                1 => {
                    let xwin = unwrap!(reader.read_i32::<LittleEndian>());
                    let ywin = unwrap!(reader.read_i32::<LittleEndian>());
                    if *unwrap!(state_r.lock()) != State::Attached {
                        continue;
                    }
                    // for now we only support one windows monitor
                    let windows_monitor = cfg.monitors.iter().find(|m| m.is_windows).unwrap();
                    // transform to linux coordinates
                    let right = xwin >= windows_monitor.bounds.width - 1;
                    let down = ywin >= windows_monitor.bounds.height - 1;
                    let xlin = if right && !windows_monitor.connected {
                        xwin + windows_monitor.bounds.x - windows_monitor.bounds.width
                    } else {
                        xwin + windows_monitor.bounds.x
                    };
                    let ylin = if down && !windows_monitor.connected {
                        ywin + windows_monitor.bounds.y - windows_monitor.bounds.height
                    } else {
                        ywin + windows_monitor.bounds.y
                    };
                                        
                    if cfg.monitors.iter().any(|m| m.vbounds.contains((xwin, ywin)) && !m.is_windows) {
                        unwrap!(reader.write_u8(4));
                        unwrap!(reader.flush());
                        *unwrap!(state_r.lock()) = State::TryDetach;
                        let mut cmd = Command::new("xdotool");
                        cmd.arg("mousemove")
                            .arg(xlin.to_string())
                            .arg(ylin.to_string());
                        if !unwrap!(cmd.status()).success() {
                            println!("Got nonzero exit status");
                            ::std::process::exit(1);
                        }
                    }
                }
                2 => *unwrap!(state_r.lock()) = State::Attached,
                3 => *unwrap!(state_r.lock()) = State::Detached,
                _ => {
                    println!("Got unknown Packet");
                    ::std::process::exit(1);
                }
            }
        }
    });
    // initialize with first non-windows monitor to avoid bugs
    let mut old_monitor = config.monitors.iter().find(|m| !m.is_windows).unwrap();
    while let Some(evt) = con.wait_for_event() {
        match evt.response_type() {
            xproto::MOTION_NOTIFY => {
                if *state.lock().unwrap() != State::Detached {
                    continue;
                }
                let query = xproto::query_pointer(&con, root);
                let reply = query.get_reply().unwrap();
                let x = reply.root_x() as i32;
                let y = reply.root_y() as i32;
                let curr_monitor = config.monitors.iter().find(|m| !m.is_windows && m.bounds.contains((x,y)))
                .expect("The mouse is not on a monitor???");;
                let vx = x + curr_monitor.vbounds.x;
                let vy = y + curr_monitor.vbounds.y;
                let monitor = config.monitors.iter().find(|m| m.vbounds.contains((vx,vy)))
                    .expect("The mouse is not on a monitor???");
                if monitor.is_windows && monitor != old_monitor {
                    let right = old_monitor.bounds.x > monitor.bounds.x + monitor.bounds.width - 1;
                    let x = if right && !monitor.connected {
                        x - monitor.vbounds.x + monitor.vbounds.width - 1
                    } else {
                        x - monitor.vbounds.x
                    } + curr_monitor.vbounds.x;
                    let y = y + monitor.vbounds.y + curr_monitor.vbounds.y;
                    writer.write_u8(8).unwrap();
                    writer.write_i32::<LittleEndian>(x).unwrap();
                    writer.write_i32::<LittleEndian>(y).unwrap();
                    *state.lock().unwrap() = State::TryAttach;
                }
                old_monitor = monitor;
            }
            xproto::CREATE_NOTIFY => {
                register(&con, root);
            }
            _ => ()
        }
    }
}

fn register(con: &Connection, window: Window) {
    xproto::grab_server(&con).request_check().unwrap();
    let values = &[(xproto::CW_EVENT_MASK, xproto::EVENT_MASK_POINTER_MOTION | xproto::EVENT_MASK_SUBSTRUCTURE_NOTIFY)];
    let _ = xproto::change_window_attributes_checked(&con, window, values).request_check();
    xproto::ungrab_server(&con).request_check().unwrap();
    if let Ok(reply) = xproto::query_tree(con, window).get_reply() {
        let children = reply.children();
        for child in children {
            register(con, *child);
        }
    }
}
