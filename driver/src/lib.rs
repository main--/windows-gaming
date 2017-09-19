extern crate nix;
extern crate users;
extern crate libudev;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate libc;
#[macro_use]
extern crate lazy_static;
extern crate itertools;
#[macro_use]
extern crate log;
extern crate mio;
extern crate bytes;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_uds;
extern crate tokio_timer;
extern crate tokio_process;
extern crate tokio_signal;
extern crate dbus as libdbus;
extern crate input;
extern crate xcb;
extern crate prost;
extern crate common;

pub mod qemu;
pub use control::ControlCmdIn;

mod control;
mod monitor;
mod clientpipe;
mod controller;
mod my_io;
mod sd_notify;
mod samba;
mod dbus;
mod sleep_inhibitor;
mod libinput;
mod clipboard;
mod release_all_keys;

use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener as StdUnixListener};
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use tokio_core::reactor::Core;
use tokio_signal::unix::{Signal, SIGINT, SIGTERM};
use futures::{Future, Stream, future};
use futures::unsync::mpsc;

use common::config::Config;

use controller::Controller;
use monitor::Monitor;
use clientpipe::Clientpipe;
use libinput::Input;
use clipboard::X11Clipboard;

pub fn run(cfg: &Config, tmp: &Path, data: &Path, enable_gui: bool) {
    let _ = fs::remove_dir_all(tmp); // may fail - we dont care
    fs::create_dir(tmp).expect("Failed to create TMP_FOLDER"); // may not fail - has to be new
    trace!("created tmp dir");

    let monitor_socket_file = tmp.join("monitor.sock");
    let monitor_socket = StdUnixListener::bind(&monitor_socket_file)
        .expect("Failed to create monitor socket");
    debug!("Started Monitor");

    let clientpipe_socket_file = tmp.join("clientpipe.sock");
    let clientpipe_socket = StdUnixListener::bind(&clientpipe_socket_file)
        .expect("Failed to create clientpipe socket");
    debug!("Started Clientpipe");

    let control_socket_file = tmp.join("control.sock");
    let control_socket = StdUnixListener::bind(&control_socket_file)
        .expect("Failed to create control socket");
    fs::set_permissions(control_socket_file, Permissions::from_mode(0o777))
        .expect("Failed to set permissions on control socket");
    debug!("Started Control socket");

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let qemu = qemu::run(cfg, tmp, data, &clientpipe_socket_file, &monitor_socket_file, &handle, enable_gui)
        .map(|code| {
            if !code.success() {
                warn!("QEMU returned with an error code: {}", code);
            }
        });

    let (monitor_stream, _) = monitor_socket.accept().expect("Failed to get monitor");
    drop(monitor_socket);

    let (clientpipe_stream, _) = clientpipe_socket.accept().expect("Failed to get clientpipe");
    drop(clientpipe_socket);

    sd_notify::notify_systemd(false, "Booting ...");
    debug!("Windows is starting");

    let mut monitor = Monitor::new(monitor_stream, &handle);
    let mut clientpipe = Clientpipe::new(clientpipe_stream, &handle);

    let (mut input, input_events) = Input::new(&handle, cfg.machine.clone());
    input.suspend();
    let input = Rc::new(RefCell::new(input));

    let monitor_sender = monitor.take_send();
    let (clipgrab_send, clipgrab_recv) = mpsc::unbounded();
    let (clipread_send, clipread_recv) = mpsc::unbounded();
    let (resp_send, resp_recv) = mpsc::unbounded();

    let ctrl = Controller::new(cfg.machine.clone(), monitor_sender.clone(), clientpipe.take_send(), input.clone(),
                               resp_send, clipgrab_send, clipread_send);

    let controller = Rc::new(RefCell::new(ctrl));

    let clipboard = X11Clipboard::open().expect("Failed to open X11 clipboard!");
    let clipboard_listener = clipboard.run(controller.clone(), resp_recv, &handle);

    let clipboard_grabber = clipgrab_recv.for_each(|()| {
        clipboard.grab_clipboard();
        Ok(())
    }).then(|_| Ok(()));

    let clipboard_reader = clipread_recv.for_each(|kind| {
        clipboard.read_clipboard(kind);
        Ok(())
    }).then(|_| Ok(()));

    let sysbus = sleep_inhibitor::system_dbus();
    let ctrl = controller.clone();
    let inhibitor = sleep_inhibitor::sleep_inhibitor(&sysbus, move || ctrl.borrow_mut().suspend(), &handle);

    let ref input_ref = *input;
    let input_listener = libinput::InputListener(input_ref);
    let hotkey_bindings: Vec<_> = cfg.machine.hotkeys.iter().map(|x| x.key.clone()).collect();
    let input_handler = libinput::create_handler(input_events, &hotkey_bindings, controller.clone(),
                                                 monitor_sender);

    let control_handler = control::create(control_socket, &handle, controller.clone());

    let sigint = Signal::new(SIGINT, &handle).flatten_stream();
    let sigterm = Signal::new(SIGTERM, &handle).flatten_stream();
    let signals = sigint.merge(sigterm);
    let catch_sigterm = signals.for_each(|_| {
        controller.borrow_mut().shutdown();
        Ok(())
    }).then(|_| Ok(()));

    let joined = future::join_all(vec![
        inhibitor,
        clientpipe.take_handler(controller.clone(), &handle),
        clientpipe.take_sender(),
        control_handler,
        monitor.take_handler(controller.clone()),
        monitor.take_sender(),
        Box::new(catch_sigterm),
        Box::new(input_listener),
        input_handler,
        clipboard_listener,
        Box::new(clipboard_grabber),
        Box::new(clipboard_reader),
    ]).map(|_| ());

    core.run(qemu.select2(joined).then(|x| {
        match x {
            Ok(future::Either::A((_, _))) => info!("qemu down first, all ok"),
            Err(future::Either::A((e, _))) => return future::err(e).boxed(),
            Ok(future::Either::B((_, _))) => unreachable!(), // we never return cleanly
            Err(future::Either::B((e, a))) => {
                error!("We errored: {}", e);
                return a.boxed(); // we errored first, wait for qemu to exit
            }
        }
        future::ok(()).boxed()
    })).expect("Waiting for qemu errored");

    info!("unbinding resettable vfio-things");
    
    for dev in cfg.machine.pci_devices.iter().filter(|x| x.resettable) {
        let mut child = Command::new(data.join("vfio-ubind")).arg(&dev.slot).arg("-r").spawn().expect("failed to run vfio-ubind");
        match child.wait() {
            Ok(status) if status.success() => (), // all is well
            Ok(status) => error!("vfio-ubind failed with {}! The device might still be bound to the vfio-driver!", status),
            Err(err) => error!("failed to wait on child. Got: {}", err)
        }
    }
    info!("windows-gaming-driver down.");
}
