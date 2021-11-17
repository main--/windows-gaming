#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate log;
extern crate dbus as libdbus;

use tokio::runtime::{Builder, Runtime};

pub mod qemu;
pub use crate::control::ControlCmdIn;
use futures03::{FutureExt, StreamExt, TryFutureExt, TryStreamExt};
use futures03::compat::Future01CompatExt;
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::LocalSet;
use tokio_stream::wrappers::SignalStream;

mod control;
mod monitor;
mod clientpipe;
mod controller;
mod sd_notify;
mod samba;
mod dbus;
mod sleep_inhibitor;
mod libinput;
mod clipboard;
mod release_all_keys;

use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream};
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::io::ErrorKind;

use futures::{Future, Stream, future};
use futures::unsync::mpsc;

use common::config::Config;

use crate::controller::Controller;
use crate::monitor::Monitor;
use crate::clientpipe::Clientpipe;
use crate::libinput::Input;
use crate::clipboard::X11Clipboard;

pub fn run(cfg: &Config, tmp: &Path, data: &Path, enable_gui: bool) {
    let rt1 = Builder::new_current_thread().enable_all().build().unwrap();
    let _g = rt1.enter();
    run_inner(cfg, tmp, data, enable_gui, &rt1);
}
pub fn run_inner(cfg: &Config, tmp: &Path, data: &Path, enable_gui: bool, rt1: &Runtime) {
    let control_socket_file = tmp.join("control.sock");
    // first check for running sessions
    match UnixStream::connect(&control_socket_file) {
        Err(e) => match e.kind() {
            ErrorKind::ConnectionRefused => (), // previous instance existed but is down now
            ErrorKind::NotFound => (), // no previous instance
            _ => warn!("Error while checking for running instances: {:?}", e), // ??? (but continue anyway)
        },
        Ok(_) => {
            error!("An instance of windows-gaming is already running in this runtime directory.");
            error!("Either quit that or select a different runtime directory.");
            return;
        }
    }

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

    let control_socket = StdUnixListener::bind(&control_socket_file)
        .expect("Failed to create control socket");
    fs::set_permissions(control_socket_file, Permissions::from_mode(0o777))
        .expect("Failed to set permissions on control socket");
    debug!("Started Control socket");

    let qemu_child = qemu::run(cfg, tmp, data, &clientpipe_socket_file, &monitor_socket_file, enable_gui);
    let qemu = qemu_child.wait_with_output().boxed().compat().map(|code| {
            if !code.status.success() {
                warn!("QEMU returned with an error code: {}", code.status);
            }
        });

    let (monitor_stream, _) = monitor_socket.accept().expect("Failed to get monitor");
    drop(monitor_socket);

    let (clientpipe_stream, _) = clientpipe_socket.accept().expect("Failed to get clientpipe");
    drop(clientpipe_socket);

    sd_notify::notify_systemd(false, "Booting ...");
    debug!("Windows is starting");

    let mut monitor = Monitor::new(monitor_stream);
    let mut clientpipe = Clientpipe::new(clientpipe_stream);

    let (mut input, input_events) = Input::new(cfg.machine.clone());
    input.suspend();
    let input = Rc::new(RefCell::new(input));

    let monitor_sender = monitor.take_send();
    let (clipgrab_send, clipgrab_recv) = mpsc::unbounded();
    let (clipread_send, clipread_recv) = mpsc::unbounded();
    let (resp_send, resp_recv) = mpsc::unbounded();

    let ctrl = Controller::new(cfg.machine.clone(), monitor_sender.clone(), clientpipe.take_send(), input.clone(),
                               resp_send, clipgrab_send, clipread_send);

    let controller = Rc::new(RefCell::new(ctrl));


    let clipboard = /*core.run*/rt1.block_on(X11Clipboard::open().compat()).expect("Failed to open X11 clipboard!");
    let clipboard_listener = clipboard.run(controller.clone(), resp_recv);

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
    let inhibitor = sleep_inhibitor::sleep_inhibitor(&sysbus, move || ctrl.borrow_mut().suspend());

    let ref input_ref = *input;
    let input_listener = libinput::InputListener(input_ref);
    let hotkey_bindings: Vec<_> = cfg.machine.hotkeys.iter().map(|x| x.key.clone()).collect();
    let input_handler = libinput::create_handler(input_events, &hotkey_bindings, controller.clone(),
                                                 monitor_sender);

    let control_handler = control::create(control_socket, controller.clone());

    let sigint = SignalStream::new(signal(SignalKind::interrupt()).unwrap());
    let sigterm = SignalStream::new(signal(SignalKind::terminate()).unwrap());
    let signals = tokio_stream::StreamExt::merge(sigint, sigterm).map(Ok).compat();
    let catch_sigterm = signals.for_each(|_| {
        controller.borrow_mut().shutdown();
        Ok::<(), ()>(())
    }).then(|_| Ok(()));

    let joined = future::join_all(vec![
        inhibitor,
        clientpipe.take_handler(controller.clone()),
        clientpipe.take_sender(),
        control_handler,
        monitor.take_handler(controller.clone()),
        monitor.take_sender(),
        Box::new(catch_sigterm),
        Box::new(input_listener.compat()),
        input_handler,
        clipboard_listener,
        Box::new(clipboard_grabber),
        Box::new(clipboard_reader),
    ]).map(|_| ());

    let ls = LocalSet::new();
    ls.block_on(&rt1, qemu.select2(joined).then(|x| -> Box<dyn Future<Item=(), Error=std::io::Error>> {
        match x {
            Ok(future::Either::A((_, _))) => info!("qemu down first, all ok"),
            Err(future::Either::A((e, _))) => return Box::new(future::err(e)),
            Ok(future::Either::B((_, _))) => unreachable!(), // we never return cleanly
            Err(future::Either::B((e, a))) => {
                error!("We errored: {}", e);
                return Box::new(a); // we errored first, wait for qemu to exit
            }
        }
        Box::new(future::ok(()))
    }).compat()).expect("Waiting for qemu errored");

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
