mod codec;

pub use self::codec::ControlCmdOut;

use std::os::unix::net::{UnixListener as StdUnixListener};
use std::io::{Error};
use std::rc::Rc;
use std::cell::RefCell;

use futures::{Stream, Future};
use tokio_core::reactor::Handle;
use tokio_io::AsyncRead;
use tokio_uds::UnixListener as TokioUnixListener;

use driver::controller::Controller;
use self::codec::{Codec, ControlCmdIn};

type Handler<'a> = Box<Future<Item=(), Error=Error> + 'a>;

pub fn create<'a>(socket: StdUnixListener, handle: &'a Handle, controller: Rc<RefCell<Controller>>) -> Handler<'a> {
    let socket = TokioUnixListener::from_listener(socket, &handle).unwrap();
    let handler = socket.incoming().for_each(move |(socket, _)| {
        let (_writer, reader) = socket.framed(Codec).split();

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
                ControlCmdIn::Suspend => {
                    info!("Suspending guest!");
                    controller.borrow_mut().suspend();
                }
            }
            Ok(())
        }).then(|_| Ok(())));
        Ok(())
    });
    Box::new(handler)
}
