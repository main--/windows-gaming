mod codec;

pub use self::codec::ControlCmdOut;

use std::os::unix::net::{UnixListener as StdUnixListener};
use std::io::Error;
use std::rc::Rc;
use std::cell::RefCell;

use futures::{future, Stream, Future, Sink};
use futures::unsync::mpsc;
use tokio_core::reactor::Handle;
use tokio_io::AsyncRead;
use tokio_uds::UnixListener as TokioUnixListener;

use driver::controller::Controller;
use self::codec::{Codec, ControlCmdIn};

type Handler<'a> = Box<Future<Item=(), Error=Error> + 'a>;

pub fn create<'a>(socket: StdUnixListener, handle: &'a Handle, controller: Rc<RefCell<Controller>>) -> Handler<'a> {
    let socket = TokioUnixListener::from_listener(socket, &handle).unwrap();
    let handle_inner = handle.clone();
    let handler = socket.incoming().for_each(move |(socket, _)| {
        let (writer, reader) = socket.framed(Codec).split();
        let (sender, recv) = mpsc::unbounded();
        let sender = Rc::new(RefCell::new(sender));
        let writer = writer.sink_map_err(|_| ()).send_all(recv).map_err(|_| ()).map(|_| ());
        let controller_rc = controller.clone();
        let mut temp_entry = false;
        let handle_inner = handle_inner.clone();
        let reader = reader.map_err(|_| ()).for_each(move |req| {
            let mut controller = controller_rc.borrow_mut();
            info!("Control request: {:?}", req);
            if temp_entry {
                match req {
                    ControlCmdIn::IoExit => {
                        controller.temporary_exit();
                        temp_entry = false;
                    }
                    _ => {
                        controller.temporary_exit();
                        return Box::new(future::err(())) as Box<Future<Item=_, Error=_>>;
                    }
                }
                return Box::new(future::ok(()));
            }
            match req {
                ControlCmdIn::IoEntry => controller.io_attach(),
                ControlCmdIn::Shutdown => controller.shutdown(),
                ControlCmdIn::ForceIoEntry => controller.io_force_attach(),
                ControlCmdIn::IoExit => controller.io_detach(),
                ControlCmdIn::Suspend => return controller.suspend(),
                ControlCmdIn::TryIoEntry => controller.try_attach(),
                ControlCmdIn::LightEntry => controller.light_attach(),
                ControlCmdIn::TemporaryLightEntry { x, y } => {
                    let (send, receiver) = mpsc::unbounded();
                    let res = controller.temporary_entry(send, x, y);
                    if !res {
                        warn!("Temporary entry failed, closing connection");
                        return Box::new(future::err(()));
                    }
                    (&*sender.borrow()).send(ControlCmdOut::TemporaryLightAttached).unwrap();
                    temp_entry = true;
                    let receiver = receiver.map_err(|_| ());
                    let controller = controller_rc.clone();
                    let sender = sender.clone();
                    let sender2 = sender.clone();
                    handle_inner.spawn(receiver.for_each(move |data| (&*sender.borrow()).send(data).map_err(|_| ()))
                        .then(move |_| {
                            controller.borrow_mut().temporary_exit();
                            let _ = (&*sender2.borrow()).send(ControlCmdOut::TemporaryLightDetached);
                            Ok(())
                        }));
                }
            }
            Box::new(future::ok(()))
        }).then(|_| Ok(()));

        handle.spawn(writer.select(reader).map(|_| ()).map_err(|_| ()));
        Ok(())
    });
    Box::new(handler)
}
