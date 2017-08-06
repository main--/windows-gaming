extern crate nix;
extern crate xdg;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate time;
extern crate common;
extern crate driver;
extern crate wizard;

mod logger;

use std::iter::Iterator;
use std::path::Path;
use std::env;

use nix::unistd;

use common::config::Config;

enum RunMode {
    System,
    User,
}

const DATA_FOLDER: &'static str = "/usr/lib/windows-gaming";
fn main() {
    logger::init().expect("Error initializing env_logger");

    let mut args = env::args().skip(1);
    let config_path = args.next();
    if args.next().is_some() {
        println!("Usage: windows-gaming-driver [conf]");
    }

    let mode = if unistd::getuid() == 0 {
        debug!("Running in system mode");
        RunMode::System
    } else {
        debug!("Running in user mode");
        RunMode::User
    };

    let xdg_dirs = xdg::BaseDirectories::with_prefix("windows-gaming-driver").unwrap();

    let config_path = match config_path {
        Some(x) => Path::new(&x).to_path_buf(),
        None => {
            match mode {
                RunMode::System => Path::new("/etc/windows-gaming-driver.toml").to_path_buf(),
                RunMode::User => xdg_dirs.place_config_file("config").expect("Failed to create config directory."),
            }
        }
    };
    debug!("Using config file {:?}", config_path);

    let workdir_path = match mode {
        RunMode::System => Path::new("/run/windows-gaming-driver").to_path_buf(),
        RunMode::User => xdg_dirs.create_runtime_directory("").expect("Failed to create runtime directory."),
    };
    debug!("Working directory is {:?}", workdir_path);

    let cfg = Config::load(&config_path);
    trace!("Successfully loaded configuration file.");

    match cfg {
        Some(ref cfg) if cfg.setup.is_none() => driver::run(cfg, &workdir_path, Path::new(DATA_FOLDER)),
        cfg => wizard::run(cfg, &config_path, &workdir_path, Path::new(DATA_FOLDER)),
    }
}
