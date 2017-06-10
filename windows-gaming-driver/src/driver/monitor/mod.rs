mod codec;

pub use self::codec::QmpCommand;

use std::os::unix::net::{UnixStream as StdUnixStream};
use std::io::{Error, ErrorKind};

use futures::unsync::mpsc::{self, UnboundedSender};
use futures::{Stream, Sink, Future};
use tokio_core::reactor::Handle;
use tokio_io::AsyncRead;
use tokio_uds::UnixStream as TokioUnixStream;

use self::codec::Codec;

type Send = UnboundedSender<QmpCommand>;
type Sender = Box<Future<Item=(), Error=Error>>;
type Read = Box<Stream<Item=String, Error=Error>>;
type Handler = Box<Future<Item=(), Error=Error>>;

pub struct Monitor {
    send: Option<Send>,
    sender: Option<Sender>,
    read: Option<Read>,
}

impl Monitor {
    pub fn new(stream: StdUnixStream, handle: &Handle) -> Monitor {
        let stream = TokioUnixStream::from_stream(stream, handle).unwrap();
        let (write, read) = stream.framed(Codec).split();
        let (send, recv) = mpsc::unbounded();
        let recv = recv.map_err(|()| Error::new(ErrorKind::Other, "Failed to write to monitor"));
        let sender = write.send_all(recv).map(|_| ());

        Monitor {
            send: Some(send),
            sender: Some(Box::new(sender)),
            read: Some(Box::new(read)),
        }
    }

    pub fn take_send(&mut self) -> Send {
        self.send.take().unwrap()
    }

    pub fn take_sender(&mut self) -> Sender {
        self.sender.take().unwrap()
    }

    pub fn take_handler(&mut self) -> Handler {
        let handler = self.read.take().unwrap().for_each(|line| { println!("{}", line); Ok(()) });
        Box::new(handler)
    }
}
