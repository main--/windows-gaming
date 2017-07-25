pub mod qemu;

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
pub mod hotkeys;

use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener as StdUnixListener};
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio_core::reactor::Core;
use tokio_signal::unix::{Signal, SIGINT, SIGTERM};
use futures::{Future, Stream, future};
use clipboard::{ClipboardContext, ClipboardProvider};

use driver::controller::Controller;
use config::Config;
use self::monitor::Monitor;
use self::clientpipe::Clientpipe;
use self::libinput::Input;

pub fn run(cfg: &Config, tmp: &Path, data: &Path) {
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

    let clipboard = ClipboardContext::new().expect("Failed to access clipboard");
    debug!("Clipboard created");

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let qemu = qemu::run(cfg, tmp, data, &clientpipe_socket_file, &monitor_socket_file, &handle)
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
    let ctrl = Controller::new(cfg.machine.clone(), monitor_sender.clone(), clientpipe.take_send(),
                               input.clone(), clipboard);
    let controller = Rc::new(RefCell::new(ctrl));

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
    ]).map(|_| ());

    match core.run(qemu.select(joined)) {
        Ok(((), _)) => (), // one (*hopefully* qemu) is done, so the other is too
        Err((e, _)) => panic!("Unexpected error: {}", e),
    }

    info!("windows-gaming-driver down.");
}
