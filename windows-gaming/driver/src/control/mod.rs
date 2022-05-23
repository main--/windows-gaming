mod codec;

pub use self::codec::{ControlCmdOut, ControlCmdIn};

use std::io::Error;
use std::rc::Rc;
use std::cell::RefCell;

use futures::{future, Stream, Future, Sink};
use futures::unsync::mpsc;
use futures03::{SinkExt, StreamExt, TryStreamExt};
use futures03::compat::Future01CompatExt;

use crate::controller::Controller;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::codec::Decoder;
use self::codec::Codec;

type Handler<'a> = Box<dyn Future<Item=(), Error=Error> + 'a>;

pub fn create<'a>(socket: UnixListener, controller: Rc<RefCell<Controller>>) -> Handler<'a> {
    let handler = UnixListenerStream::new(socket).compat().for_each(move |socket| {
        let (writer, reader) = Codec.framed(socket).split();
        let (sender, recv) = mpsc::unbounded();
        let sender = Rc::new(RefCell::new(sender));
        let writer = writer.compat().sink_map_err(|_| ()).send_all(recv).map_err(|_| ()).map(|_| ());
        let controller_rc = controller.clone();
        let mut temp_entry = false;
        let reader = reader.compat().map_err(|_| ()).for_each(move |req| {
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
                        return Box::new(future::err(())) as Box<dyn Future<Item=_, Error=_>>;
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
                    (&*sender.borrow()).unbounded_send(ControlCmdOut::TemporaryLightAttached).unwrap();
                    temp_entry = true;
                    let receiver = receiver.map_err(|_| ());
                    let controller = controller_rc.clone();
                    let sender = sender.clone();
                    let sender2 = sender.clone();
                    tokio::task::spawn_local(receiver.for_each(move |data| (&*sender.borrow()).unbounded_send(data).map_err(|_| ()))
                        .then(move |_| {
                            controller.borrow_mut().temporary_exit();
                            let _ = (&*sender2.borrow()).unbounded_send(ControlCmdOut::TemporaryLightDetached);
                            Ok::<(), ()>(())
                        }).compat());
                }
                ControlCmdIn::EnterBackupMode => controller.enter_backup_mode(send_ack_when_ready(sender.clone())),
                ControlCmdIn::LeaveBackupMode => controller.leave_backup_mode(send_ack_when_ready(sender.clone())),
            }
            Box::new(future::ok(()))
        }).then(|_| Ok(()));

        tokio::task::spawn_local(writer.select(reader).then(|_| Ok::<(), ()>(())).compat());
        Ok(())
    });
    Box::new(handler)
}
fn send_ack_when_ready(sender: Rc<RefCell<futures::unsync::mpsc::UnboundedSender<ControlCmdOut>>>) -> tokio::sync::oneshot::Sender<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_local(async move {
        if rx.await.is_ok() {
            let _ = sender.borrow().unbounded_send(ControlCmdOut::Ack);
        }
    });
    tx
}
