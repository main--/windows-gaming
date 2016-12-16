extern crate systemd;
extern crate nix;
extern crate users;
extern crate toml;
extern crate rustc_serialize;
extern crate timerfd;
extern crate libudev;
extern crate num_cpus;

mod mainloop;
mod config;
mod sd_notify;
mod samba;
mod controller;
mod pci_device;
mod setup_wizard;
mod hwid;
mod qemu;

use std::iter::Iterator;
use std::path::Path;
use std::env;

use nix::unistd;

use config::Config;

enum RunMode {
    System,
    User,
}

const DATA_FOLDER: &'static str = "/usr/lib/windows-gaming";
fn main() {
    let mut args = env::args().skip(1);
    let config_path = args.next();
    if args.next().is_some() {
        println!("Usage: windows-gaming-driver [conf]");
    }

    let mode = if unistd::getuid() == 0 {
        RunMode::System
    } else {
        RunMode::User
    };

    let config_path = match config_path {
        Some(x) => Path::new(&x).to_path_buf(),
        None => {
            match mode {
                RunMode::System => Path::new("/etc/windows-gaming-driver.toml").to_path_buf(),
                RunMode::User => Path::new(&env::var("XDG_CONFIG_HOME").expect("Failed to get XDG_CONFIG_HOME"))
                    .join("windows-gaming-driver").join("config.toml"),
            }
        }
    };

    let workdir_path = match mode {
        RunMode::System => Path::new("/run/windows-gaming-driver").to_path_buf(),
        RunMode::User => Path::new(&env::var("XDG_RUNTIME_DIR").expect("Failed to get XDG_RUNTIME_DIR")).to_path_buf(),
    }.join("windows-gaming-driver");

    if config_path.exists() {
        let cfg = Config::load(config_path);
        qemu::run(&cfg, &workdir_path, Path::new(DATA_FOLDER));
    } else {
        setup_wizard::run(&config_path, &workdir_path, Path::new(DATA_FOLDER));
    }
}
