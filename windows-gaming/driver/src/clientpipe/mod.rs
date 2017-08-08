mod codec;

pub use self::codec::{GaCmdOut, ClipboardMessage, ClipboardType, ClipboardTypes, RegisterHotKey, Point};

use std::os::unix::net::{UnixStream as StdUnixStream};
use std::io::{Error, ErrorKind};
use std::rc::Rc;
use std::cell::RefCell;
use std::time::Duration;

use futures::unsync::mpsc::{self, UnboundedSender};
use futures::{Stream, Sink, Future};
use tokio_core::reactor::Handle;
use tokio_io::AsyncRead;
use tokio_uds::UnixStream as TokioUnixStream;
use tokio_timer::Timer;

use controller::Controller;
use self::codec::{Codec, GaCmdIn};

type Send = UnboundedSender<GaCmdOut>;
type Sender = Box<Future<Item=(), Error=Error>>;
type Read = Box<Stream<Item=GaCmdIn, Error=Error>>;
type Handler<'a> = Box<Future<Item=(), Error=Error> + 'a>;

pub struct Clientpipe {
    send: Option<Send>,
    sender: Option<Sender>,
    read: Option<Read>,
}

impl Clientpipe {
    pub fn new(stream: StdUnixStream, handle: &Handle) -> Clientpipe {
        let stream = TokioUnixStream::from_stream(stream, &handle).unwrap();
        let (write, read) = stream.framed(Codec).split();
        let (send, recv) = mpsc::unbounded();
        let recv = recv.map_err(|()| Error::new(ErrorKind::Other, "Failed to write to clientpipe"));
        let sender = write.send_all(recv).map(|_| ());

        Clientpipe {
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

    pub fn take_handler<'a>(&mut self, controller_rc: Rc<RefCell<Controller>>, handle: &'a Handle) -> Handler<'a> {
        let handler = self.read.take().unwrap().for_each(move |cmd| {
            trace!("GA sent message: {:?}", cmd);
            let mut controller = controller_rc.borrow_mut();

            match cmd {
                GaCmdIn::ReportBoot(version) => {
                    info!("client is now alive!");

                    if controller.ga_hello(version) {
                        let controller = controller_rc.clone();
                        let timer = Timer::default().interval(Duration::new(5, 0))
                            .map_err(|_| ())
                            .for_each(move |()| match controller.borrow_mut().ga_ping() {
                                true => Ok(()),
                                false => Err(()),
                            });
                        handle.spawn(timer);
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
                    Some(ClipboardMessage::GrabClipboard(())) => controller.grab_x11_clipboard(),
                    Some(ClipboardMessage::RequestClipboardContents(kind)) => match ClipboardType::from_i32(kind) {
                        Some(kind) => controller.read_x11_clipboard(kind),
                        None => error!("Windows requested an invalid clipboard type??"),
                    },
                    Some(ClipboardMessage::ContentTypes(types)) => controller.respond_x11_types(types.types().collect()),
                    Some(ClipboardMessage::ClipboardContents(buf)) => controller.respond_x11_clipboard(buf),
                    None => error!("Windows sent an empty clipboard message??"),
                },
                GaCmdIn::MouseEdged(Point { x, y }) => controller.mouse_edged(x, y),
            }
            Ok(())
        });
        Box::new(handler)
    }
}
