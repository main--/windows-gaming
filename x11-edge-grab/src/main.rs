extern crate xcb;
extern crate byteorder;

use std::os::unix::net::UnixStream;
use std::io::Write;
use std::thread;
use std::process::Command;
use std::sync::{Arc, Mutex};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use xcb::Connection;
use xcb::xproto::{self, Window};

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
    let mut writer = UnixStream::connect("/run/user/1000/windows-gaming-driver/control.sock").unwrap();
    let mut reader = writer.try_clone().unwrap();
    let (con, _) = Connection::connect(None).unwrap();
    let root = con.get_setup().roots().take(1).next().unwrap().root();
    register(&con, root);
    let state = Arc::new(Mutex::new(State::Detached));

    let state_r = state.clone();
    thread::spawn(move || {
        loop {
            match unwrap!(reader.read_u8()) {
                1 => {
                    let x = unwrap!(reader.read_i32::<LittleEndian>());
                    let y = unwrap!(reader.read_i32::<LittleEndian>());
                    if *unwrap!(state_r.lock()) != State::Attached {
                        continue;
                    }
                    println!("{}:{}", x, y);
                    if x <= 0 && y >= 540 && y < 1080+540
                        || x >= 3839 && y >= 540 && y < 1080+540 {
                        println!("exit");
                        let x = x + 1920;
                        unwrap!(reader.write_u8(4));
                        println!("written");
                        unwrap!(reader.flush());
                        *unwrap!(state_r.lock()) = State::TryDetach;
                        println!("flushed");
                        let mut cmd = Command::new("xdotool");
                        cmd.arg("mousemove")
                            .arg(x.to_string())
                            .arg(y.to_string());
                        if !unwrap!(cmd.status()).success() {
                            println!("Got nonzero exit status");
                            ::std::process::exit(1);
                        }
                    }
                }
                2 => {
                    *unwrap!(state_r.lock()) = State::Attached;
                    println!("Attached");
                },
                3 => {
                    *unwrap!(state_r.lock()) = State::Detached;
                    println!("Detached");
                },
                _ => {
                    println!("Got unknown Packet");
                    ::std::process::exit(1);
                }
            }
        }
    });
    while let Some(evt) = con.wait_for_event() {
        match evt.response_type() {
            xproto::MOTION_NOTIFY => {
                if *state.lock().unwrap() != State::Detached {
                    continue;
                }
                let query = xproto::query_pointer(&con, root);
                let reply = query.get_reply().unwrap();
                let x = reply.root_x();
                let y = reply.root_y();
                if x >= 1920 && x < 1920+3840 {
                    println!("entry: {}:{}", x, y);
                    writer.write_u8(8).unwrap();
                    writer.write_i32::<LittleEndian>(x as i32 - 1920).unwrap();
                    writer.write_i32::<LittleEndian>(y as i32 + 540).unwrap();
                    *state.lock().unwrap() = State::TryAttach;
                }
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
