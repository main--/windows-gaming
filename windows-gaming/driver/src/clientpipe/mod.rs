mod codec;

pub use self::codec::{GaCmdOut, ClipboardMessage, ClipboardType, ClipboardTypes, RegisterHotKey, Point};

use std::io::{Error, ErrorKind};
use std::rc::Rc;
use std::cell::RefCell;
use std::time::Duration;

use futures::unsync::mpsc::{self, UnboundedSender};
use futures::{Stream, Future};
use futures03::StreamExt;
use futures03::SinkExt;
use futures03::TryStreamExt;
use futures03::compat::Future01CompatExt;

use crate::controller::Controller;
use tokio::net::UnixStream;
use tokio::time;
use tokio_stream::wrappers::IntervalStream;
use tokio_util::codec::Decoder;
use self::codec::{Codec, GaCmdIn};

type Send = UnboundedSender<GaCmdOut>;
type Sender = Box<dyn Future<Item=(), Error=Error>>;
type Read = Box<dyn Stream<Item=GaCmdIn, Error=Error>>;
type Handler<'a> = Box<dyn Future<Item=(), Error=Error> + 'a>;

pub struct Clientpipe {
    pub send: Option<Send>,
    pub sender: Option<Sender>,
    read: Option<Read>,
}

impl Clientpipe {
    pub fn new(stream: UnixStream) -> Clientpipe {
        let (write, read) = Codec.framed(stream).split();
        let (send, recv) = mpsc::unbounded();
        let recv = recv.map_err(|()| Error::new(ErrorKind::Other, "Failed to write to clientpipe"));
        let sender = recv.forward(write.compat()).map(|_| ());

        let read = read.map_err(|_e| Error::new(ErrorKind::Other, "Failed to read from clientpipe"));

        Clientpipe {
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

    pub fn take_handler<'a>(&mut self, controller_rc: Rc<RefCell<Controller>>) -> Handler<'a> {
        let handler = self.read.take().unwrap().for_each(move |cmd| {
            trace!("GA sent message: {:?}", cmd);
            let mut controller = controller_rc.borrow_mut();

            match cmd {
                GaCmdIn::ReportBoot(()) => {
                    info!("client is now alive!");

                    if controller.ga_hello() {
                        let controller = controller_rc.clone();
                        let timer = IntervalStream::new(time::interval(Duration::new(5, 0)))
                            .map(|a| Ok::<_, ()>(a)).compat()
                            .for_each(move |_| match controller.borrow_mut().ga_ping() {
                                true => Ok(()),
                                false => Err(()),
                            });
                        tokio::task::spawn_local(timer.compat());
                    }
                }
                GaCmdIn::Suspending(()) => {
                    info!("client says that it's suspending");
                    controller.ga_suspending();
                }
                GaCmdIn::Pong(()) => controller.ga_pong(),
                GaCmdIn::HotKey(id) => controller.ga_hotkey(id),
                GaCmdIn::HotKeyBindingFailed(s) => warn!("HotKeyBinding failed: {}", s),
                GaCmdIn::Clipboard(c) => match c.message {
                    Some(ClipboardMessage::GrabClipboard(types)) => controller.grab_x11_clipboard(types),
                    Some(ClipboardMessage::RequestClipboardContents(kind)) => match ClipboardType::from_i32(kind) {
                        Some(kind) => controller.read_x11_clipboard(kind),
                        None => error!("Windows requested an invalid clipboard type??"),
                    },
                    Some(ClipboardMessage::ClipboardContents(buf)) => controller.respond_x11_clipboard(buf),
                    None => error!("Windows sent an empty clipboard message??"),
                },
                GaCmdIn::MouseEdged(Point { x, y }) => {
                    trace!("Mouse Edged: {}:{}", x, y);
                    controller.mouse_edged(x, y);
                }
            }
            Ok(())
        });
        Box::new(handler)
    }
}
