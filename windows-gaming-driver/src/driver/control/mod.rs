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
            let mut controller = controller.borrow_mut();
            info!("Control request: {:?}", req);
            match req {
                ControlCmdIn::IoEntry => controller.io_attach(),
                ControlCmdIn::Shutdown => controller.shutdown(),
                ControlCmdIn::ForceIoEntry => controller.io_force_attach(),
                ControlCmdIn::IoExit => controller.io_detach(),
                ControlCmdIn::Suspend => { controller.suspend(); }
                ControlCmdIn::TryIoEntry => controller.try_attach(),
                ControlCmdIn::LightEntry => controller.light_attach(),
            }
            Ok(())
        }).then(|_| Ok(())));
        Ok(())
    });
    Box::new(handler)
}
