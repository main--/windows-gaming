pub mod qemu;

mod control;
mod monitor;
mod clientpipe;
mod controller;
mod my_io;
mod signalfd;
mod sd_notify;
mod samba;
mod dbus;
mod sleep_inhibitor;

use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener as StdUnixListener};
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio_core::reactor::Core;
use futures::{Future, Stream, future};

use driver::controller::Controller;
use config::Config;
use driver::signalfd::{SignalFd, signal};
use self::monitor::Monitor;
use self::clientpipe::Clientpipe;

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

    let mut qemu = qemu::run(cfg, tmp, data, &clientpipe_socket_file, &monitor_socket_file);

    let (monitor_stream, _) = monitor_socket.accept().expect("Failed to get monitor");
    drop(monitor_socket);

    let (clientpipe_stream, _) = clientpipe_socket.accept().expect("Failed to get clientpipe");
    drop(clientpipe_socket);

    sd_notify::notify_systemd(false, "Booting ...");
    debug!("Windows is starting");

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let mut monitor = Monitor::new(monitor_stream, &handle);
    let mut clientpipe = Clientpipe::new(clientpipe_stream, &handle);

    let ctrl = Controller::new(cfg.machine.clone(), monitor.take_send(), clientpipe.take_send());
    let controller = Rc::new(RefCell::new(ctrl));

    let sysbus = sleep_inhibitor::system_dbus();
    let ctrl = controller.clone();
    let inhibitor = sleep_inhibitor::sleep_inhibitor(&sysbus, move || ctrl.borrow_mut().suspend(), &handle);

    let control_handler = control::create(control_socket, &handle, controller.clone());

    let signals = SignalFd::new(vec![signal::SIGTERM, signal::SIGINT], &handle);
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
    ]);
    core.run(joined).unwrap();

    qemu.wait().unwrap();
    info!("windows-gaming-driver down.");
}
