use std::cell::RefCell;
use std::rc::Rc;
use std::os::unix::net::{UnixListener, UnixStream};

use controller::Controller;
use config::Config;

pub fn run(cfg: &Config, monitor_stream: UnixStream, clientpipe_stream: UnixStream, control_socket: UnixListener) {
    use tokio_core::reactor::Core;
    use tokio_io::AsyncRead;
    use tokio_uds::{UnixListener, UnixStream};
    use tokio_timer::Timer;
    use futures::{Future, Stream, Sink};
    use clientpipe_codec::*;
    use control_codec::*;
    use monitor_codec::*;
    use futures::unsync::mpsc;
    use std::io;
    use std::time::Duration;
    use signalfd::{SignalFd, signal};

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let monitor_stream = UnixStream::from_stream(monitor_stream, &handle).unwrap();
    let (monitor_write, monitor_read) = monitor_stream.framed(MonitorCodec).split();
    let (monitor_send, monitor_recv) = mpsc::unbounded();
    let monitor_sender = monitor_write.send_all(monitor_recv.map_err(|()| io::Error::new(io::ErrorKind::Other, "cslul")));

    let clientpipe_stream = UnixStream::from_stream(clientpipe_stream, &handle).unwrap();
    let (clientpipe_write, clientpipe_read) = clientpipe_stream.framed(ClientpipeCodec).split();
    let (clientpipe_send, clientpipe_recv) = mpsc::unbounded();
    let clientpipe_sender = clientpipe_write.send_all(clientpipe_recv.map_err(|()| io::Error::new(io::ErrorKind::Other, "cslul")));

    let ctrl = Controller::new(cfg.machine.clone(), monitor_send, clientpipe_send);
    let controller = Rc::new(RefCell::new(ctrl));

    let signals = SignalFd::new(vec![signal::SIGTERM, signal::SIGINT], &handle);
    let catch_sigterm = signals.for_each(|_| {
        controller.borrow_mut().shutdown();
        Ok(())
    }).then(|_| Ok(()));

    let monitor_handler = monitor_read.for_each(|line| { println!("{}", line); Ok(()) });

    let clientpipe_handler = clientpipe_read.for_each(|cmd| {
        match cmd {
            GaCmdIn::ReportBoot => {
                info!("client is now alive!");

                if controller.borrow_mut().ga_hello() {
                    let controller = controller.clone();
                    let timer = Timer::default().interval(Duration::new(1, 0));
                    handle.spawn(timer.take_while(move |&()| Ok(controller.borrow_mut().ga_ping()))
                                 .for_each(|()| Ok(())).then(|_| Ok(()))); // lmao
                }
            }
            GaCmdIn::Suspending => {
                info!("client says that it's suspending");
                controller.borrow_mut().ga_suspending();
            }
            GaCmdIn::Pong => {
                trace!("ga pong'ed");
                controller.borrow_mut().ga_pong();
            }
            GaCmdIn::HotKey(id) => {
                debug!("hotkey pressed: {}", id);
                controller.borrow_mut().ga_hotkey(id);
            }
            GaCmdIn::HotKeyBindingFailed(s) => {
                warn!("HotKeyBinding failed: {}", s);
            }
        }
        Ok(())
    });

    let control_socket = UnixListener::from_listener(control_socket, &handle).unwrap();
    let control_handler = control_socket.incoming().for_each(|(socket, _)| {
        let (_writer, reader) = socket.framed(ControlCodec).split();

        let controller = controller.clone();
        handle.spawn(reader.for_each(move |req| {
            match req {
                ControlCmdIn::IoEntry => {
                    info!("IO entry requested!");
                    controller.borrow_mut().io_attach();
                }
                ControlCmdIn::Shutdown => {
                    info!("Shutdown requested");
                    controller.borrow_mut().shutdown();
                }
                ControlCmdIn::ForceIoEntry => {
                    info!("IO entry FORCED!");
                    controller.borrow_mut().io_force_attach();
                }
                ControlCmdIn::IoExit => {
                    info!("IO exit!");
                    controller.borrow_mut().io_detach();
                }
            }
            Ok(())
        }).then(|_| Ok(())));
        Ok(())
    });

    core.run(monitor_handler.join(catch_sigterm).join5(clientpipe_handler, control_handler, monitor_sender, clientpipe_sender)).unwrap();
}
