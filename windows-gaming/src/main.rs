extern crate nix;
extern crate xdg;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate time;
#[macro_use] extern crate clap;
extern crate common;
extern crate driver;

use std::fs;
use std::path::Path;
use std::os::unix::net::UnixStream;
use std::io::{self, Write, Read, ErrorKind};
use std::process::Command;

use clap::{Arg, App, SubCommand, AppSettings, ArgGroup, Shell};
use nix::unistd;

use common::config::Config;
use driver::ControlCmdIn;

enum RunMode {
    System,
    User,
}

const DATA_FOLDER: &'static str = "/usr/lib/windows-gaming";
fn main() {
    env_logger::init();

    let mut cli = App::new(crate_name!())
        .version(crate_version!())
        .about("Windows Gaming")
        .setting(AppSettings::DeriveDisplayOrder)
        .setting(AppSettings::GlobalVersion)
        .setting(AppSettings::InferSubcommands)
        .arg(Arg::with_name("config")
            .long("config")
            .alias("cfg")
            .value_name("CONFIG")
            .help("Config to use")
            .takes_value(true)
            .global(true)
        ).arg(Arg::with_name("generate-bash-completions")
            .long("generate-bash-completions")
            .hidden(true)
            .takes_value(false)
        ).subcommand(SubCommand::with_name("run")
            .about("Starts Windows")
            .visible_alias("start")
            .arg(Arg::with_name("virtual-gpu")
                .long("virtual-gpu")
                .help("Run QEMU with a virtual QXL GPU that draws to a GUI window (useful for troubleshooting)")
                .takes_value(false))
        ).subcommand(SubCommand::with_name("wizard")
            .about("Runs the wizard")
        ).subcommand(SubCommand::with_name("backup")
            .about("Support functionality for performing block-level backups of your Windows VM")
            .subcommand(SubCommand::with_name("start").about("Enter backup mode. Redirect disks to snapshot files where configured."))
            .subcommand(SubCommand::with_name("stop").about("Leave backup mode. Commit and then remove all active snapshot files."))
        ).subcommand(SubCommand::with_name("control")
            .about("Commands to interact with the driver")
            .subcommand(SubCommand::with_name("attach")
                .about("Attaches configured devices to Windows")
                .long_about("Attaches configured device to Windows. If the guest agent is down, a \
                light io entry will be performed, which will be upgraded to a full entry as soon as \
                the GA comes up. If the GA is running, a full entry will be performed.")
                .arg(Arg::with_name("try")
                    .long("try")
                    .help("Performs a full entry if GA is up")
                    .takes_value(false)
                ).arg(Arg::with_name("force")
                    .long("force")
                    .help("Perform a full entry regardless of the GA state")
                    .takes_value(false)
                ).arg(Arg::with_name("light")
                    .long("light")
                    .help("Performs a light entry")
                    .takes_value(false)
                ).group(ArgGroup::with_name("type")
                    .args(&["try", "force", "light"])
                )
            ).subcommand(SubCommand::with_name("detach")
                .about("Detaches configured attached devices")
            ).subcommand(SubCommand::with_name("shutdown")
                .about("Shuts down Windows, gracefully stopping execution of the driver")
            ).subcommand(SubCommand::with_name("suspend")
                .about("Suspends Windows")
            )
        );
    let matches = cli.clone().get_matches();

    if matches.is_present("generate-bash-completions") {
        cli.gen_completions_to(crate_name!(), Shell::Bash, &mut io::stdout());
        return;
    }

    let mode = if unistd::getuid() == 0 {
        debug!("Running in system mode");
        RunMode::System
    } else {
        debug!("Running in user mode");
        RunMode::User
    };

    let xdg_dirs = xdg::BaseDirectories::with_prefix("windows-gaming-driver").unwrap();

    let config_path = match matches.value_of("config") {
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

    let control_socket = xdg_dirs.place_runtime_file("control.sock").unwrap();

    let data_folder = Path::new(match cfg {
        Some(Config { data_directory_override: Some(ref x), .. }) => x.as_str(),
        _ => DATA_FOLDER,
    }).to_owned();

    let workdir_path = match cfg {
        Some(Config { runtime_directory_override: Some(ref x), .. }) => Path::new(x).to_path_buf(),
        _ => workdir_path,
    };

    match matches.subcommand() {
        ("run", cmd) => driver::run(cfg.as_ref().unwrap(), &workdir_path, &data_folder, cmd.unwrap().is_present("virtual-gpu")),
        ("wizard", _) => unimplemented!("wizard"),
        ("control", cmd) => {
            match cmd.unwrap().subcommand() {
                ("attach", cmd) => {
                    let cmd = cmd.unwrap();
                    if cmd.is_present("try") {
                        control_send(ControlCmdIn::TryIoEntry, &control_socket);
                    } else if cmd.is_present("force") {
                        control_send(ControlCmdIn::ForceIoEntry, &control_socket);
                    } else if cmd.is_present("light") {
                        control_send(ControlCmdIn::LightEntry, &control_socket);
                    } else {
                        control_send(ControlCmdIn::IoEntry, &control_socket);
                    }
                }
                ("detach", _) => control_send(ControlCmdIn::IoExit, &control_socket),
                ("shutdown", _) => control_send(ControlCmdIn::Shutdown, &control_socket),
                ("suspend", _) => control_send(ControlCmdIn::Suspend, &control_socket),
                _ => unreachable!()
            }
        }
        ("backup", cmd) => {
            match cmd.unwrap().subcommand() {
                ("start", _) => {
                    if !control_send_fallible(ControlCmdIn::EnterBackupMode, &control_socket) {
                        // qemu is down, so invoke qemu-img to do it
                        let todos = cfg.as_ref().unwrap().machine.storage.iter()
                            .filter_map(|d| d.snapshot_file.as_ref().map(|s| (s, &d.path, &d.format)))
                            .filter(|(s, _, _)| !Path::new(s).exists());
                        for (snap, path, format) in todos {
                            // qemu-img create -f qcow2 -b /dev/tank/windows -F raw qemu-snaps/windows.qcow2
                            let status = Command::new("qemu-img")
                                .args(&["create", "-f", "qcow2", "-b", path, "-F", format, snap])
                                .status().expect("Failed to call qemu-img");
                            if !status.success() {
                                panic!("qemu-img reported an error");
                            }
                        }
                    }
                }
                ("stop", _) => {
                    if !control_send_fallible(ControlCmdIn::LeaveBackupMode, &control_socket) {
                        // qemu is down, so invoke qemu-img to do it
                        let todos = cfg.as_ref().unwrap().machine.storage.iter()
                            .filter_map(|d| d.snapshot_file.as_ref())
                            .filter(|s| Path::new(s).exists());
                        for snap in todos {
                            let status = Command::new("qemu-img")
                                .args(&["commit", "-d", snap])
                                .status().expect("Failed to call qemu-img");
                            if !status.success() {
                                panic!("qemu-img reported an error");
                            }
                            fs::remove_file(snap).expect("Failed to delete old snapshot file");
                        }
                    }
                }
                _ => unreachable!()
            }
        }
        _ => match cfg {
            Some(ref cfg) if cfg.setup.is_none() => driver::run(cfg, &workdir_path, &data_folder, false),
            _cfg => unimplemented!("wizard"),
        }
    }
}

fn control_send<P: AsRef<Path>>(cmd: ControlCmdIn, socket_path: P) {
    if !control_send_fallible(cmd, socket_path) {
        panic!("Windows is down");
    }
}
fn control_send_fallible<P: AsRef<Path>>(cmd: ControlCmdIn, socket_path: P) -> bool {
    let mut writer = match UnixStream::connect(socket_path) {
        Err(e) if e.kind() == ErrorKind::ConnectionRefused => return false,
        x => x,
    }.unwrap();
    writer.write(&[match cmd {
        ControlCmdIn::IoEntry => 1,
        ControlCmdIn::TryIoEntry => 6,
        ControlCmdIn::LightEntry => 7,
        ControlCmdIn::Shutdown => 2,
        ControlCmdIn::ForceIoEntry => 3,
        ControlCmdIn::IoExit => 4,
        ControlCmdIn::Suspend => 5,
        ControlCmdIn::TemporaryLightEntry { .. } => unimplemented!(),
        ControlCmdIn::EnterBackupMode => 9,
        ControlCmdIn::LeaveBackupMode => 10,
    }]).unwrap();
    writer.flush().unwrap();

    match cmd {
        ControlCmdIn::EnterBackupMode | ControlCmdIn::LeaveBackupMode => {
            // wait for ack
            let mut v = [0];
            writer.read_exact(&mut v).unwrap();
            let [v] = v;
            assert_eq!(v, 4); // ack command
        }
        _ => (),
    }

    true
}
