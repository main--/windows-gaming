mod codec;

pub use self::codec::{
    QmpCommand,
    InputEvent,
    Message,
    Event,
    Ret,
    DeviceDeleted,
    RtcChange,
    Timestamp,
    Qmp,
    QmpVersion,
    Version,
    KeyValue,
    InputButton,
};

use std::os::unix::net::{UnixStream as StdUnixStream};
use std::io::{Error, ErrorKind};
use std::rc::Rc;
use std::cell::RefCell;

use futures::unsync::mpsc::{self, UnboundedSender};
use futures::{Stream, Sink, Future};

use controller::Controller;
use futures03::{StreamExt, SinkExt, TryStreamExt};
use tokio1::net::UnixStream;
use tokio_util::codec::Decoder;
use self::codec::Codec;

type Send = UnboundedSender<QmpCommand>;
type Sender = Box<dyn Future<Item=(), Error=Error>>;
type Read = Box<dyn Stream<Item=Message, Error=Error>>;
type Handler = Box<dyn Future<Item=(), Error=Error>>;

pub struct Monitor {
    send: Option<Send>,
    sender: Option<Sender>,
    read: Option<Read>,
}

impl Monitor {
    pub fn new(stream: StdUnixStream) -> Monitor {
        stream.set_nonblocking(true).unwrap();
        let stream = UnixStream::from_std(stream).unwrap();
        let (write, read) = Codec.framed(stream).split();
        let (send, recv) = mpsc::unbounded();
        let recv = recv.map_err(|()| Error::new(ErrorKind::Other, "Failed to write to monitor"));
        let sender = recv.forward(write.compat()).map(|_| ());

        Monitor {
            send: Some(send),
            sender: Some(Box::new(sender)),
            read: Some(Box::new(read.compat())),
        }
    }

    pub fn take_send(&mut self) -> Send {
        self.send.take().unwrap()
    }

    pub fn take_sender(&mut self) -> Sender {
        self.sender.take().unwrap()
    }

    pub fn take_handler(&mut self, controller: Rc<RefCell<Controller>>) -> Handler {
        let handler = self.read.take().unwrap().for_each(move |msg| {
            if let Message::Return { .. } = msg {
                // do not print these, they are useless and spammy
            } else {
                info!("{:?}", msg);
            }

            if let Message::Event(Event::Suspend { .. }) = msg {
                controller.borrow_mut().qemu_suspended();
            }
            Ok(())
        });
        Box::new(handler)
    }
}
