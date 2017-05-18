extern crate systemd;
extern crate nix;
extern crate users;
extern crate toml;
extern crate timerfd;
extern crate libudev;
extern crate num_cpus;
extern crate xdg;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

mod mainloop;
mod config;
mod sd_notify;
mod samba;
mod controller;
mod pci_device;
mod setup;
mod hwid;
mod qemu;
mod util;

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

    let xdg_dirs = xdg::BaseDirectories::with_prefix("windows-gaming-driver").unwrap();

    let config_path = match config_path {
        Some(x) => Path::new(&x).to_path_buf(),
        None => {
            match mode {
                RunMode::System => Path::new("/etc/windows-gaming-driver.toml").to_path_buf(),
                RunMode::User => xdg_dirs.place_config_file("config.toml").expect("Failed to create config directory."),
            }
        }
    };

    let workdir_path = match mode {
        RunMode::System => Path::new("/run/windows-gaming-driver").to_path_buf(),
        RunMode::User => xdg_dirs.create_runtime_directory("").expect("Failed to create runtime directory."),
    };

    let cfg = if config_path.exists() {
        Some(Config::load(&config_path))
    } else {
        None
    };

    match cfg {
        Some(ref cfg) if cfg.setup.is_none() => qemu::run(cfg, &workdir_path, Path::new(DATA_FOLDER)),
        cfg => setup::run(cfg, &config_path, &workdir_path, Path::new(DATA_FOLDER)),
    }
}
