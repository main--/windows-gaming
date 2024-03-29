use std::io::Cursor;

use std::rc::Rc;
use std::cell::RefCell;


use clientpipe_proto::ClipboardTypes;
use futures::Future;
use futures::unsync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use futures03::compat::Stream01CompatExt;
use futures03::{FutureExt, TryFutureExt, StreamExt};

use crate::controller::Controller;
use crate::clientpipe::ClipboardType;
use tokio_stream::wrappers::WatchStream;

use zerocost_clipboard::{self, PLAINTEXT_MIME_TYPES};

static OUR_MIME_MARKER: &'static str = "application/from-windows";

pub struct X11Clipboard {
    clipboard: zerocost_clipboard::WaylandClipboard,
    run: RefCell<Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), anyhow::Error>>>>>>,
    cmd_tx: UnboundedSender<Cmd>,
    cmd_rx: RefCell<Option<UnboundedReceiver<Cmd>>>,
}

#[derive(Debug)]
enum Cmd {
    Grab(ClipboardTypes),
    Read(ClipboardType),
}

impl X11Clipboard {
    pub fn open() -> Box<dyn Future<Item=X11Clipboard, Error=anyhow::Error>> {
        trace!("opening wayland clipboard");
        let task = zerocost_clipboard::WaylandClipboard::init().boxed().compat();
        Box::new(task.map(|(run, clipboard)| {
            let run = run.boxed_local();
            let run = RefCell::new(Some(run));
            let (cmd_tx, cmd_rx) = mpsc::unbounded();
            trace!("opened wayland clipboard");
            X11Clipboard { run, clipboard, cmd_tx, cmd_rx: RefCell::new(Some(cmd_rx)) }
        }))
    }

    pub fn grab_clipboard(&self, types: ClipboardTypes) {
        self.cmd_tx.unbounded_send(Cmd::Grab(types)).unwrap();
    }

    pub fn read_clipboard(&self, kind: ClipboardType) {
        self.cmd_tx.unbounded_send(Cmd::Read(kind)).unwrap();
    }

    pub async fn run(&self, controller: Rc<RefCell<Controller>>, resp_recv: UnboundedReceiver<ClipboardRequestResponse>) {
        trace!("running wayland clipboard");
        let cmd_rx = self.cmd_rx.borrow_mut().take().unwrap();

        let clipboard = &self.clipboard;
        let run = self.run.borrow_mut().take().unwrap();
        tokio::task::spawn_local(run);

        let mut clipboard_rx = WatchStream::new(clipboard.subscribe().await);
        let clipboard_listener = async {
            while let Some(offer) = clipboard_rx.next().await {
                debug!("got clipboard offer");
                if let Some(offer) = offer {
                    if !offer.mime_types().contains(OUR_MIME_MARKER) {
                        debug!("it is foreign, so we grab the clipboard");
                        // only react if it's not from us

                        let mut types = ClipboardTypes::default();
                        if offer.mime_types().contains("text/plain;charset=utf-8") {
                            types.push_types(ClipboardType::Text);
                        }
                        if offer.mime_types().contains("image/png") {
                            types.push_types(ClipboardType::Image);
                        }

                        controller.borrow_mut().grab_win_clipboard(types);
                    } else {
                        debug!("but we reject it because it's ours");
                    }
                }
            }
        };
        let mut resp_recv = resp_recv.compat();
        let responder = async {
            while let Some(Ok(response)) = resp_recv.next().await {
                match response.response {
                    ClipboardResponse::Data(buf) => {
                        debug!("responding to wayland with clipboard data");
                        let mut target = response.event.req.into_target();

                        let res = tokio::io::copy_buf(&mut Cursor::new(buf), &mut target).await;
                        if let Err(e) = res {
                            error!("Error responding to wayland clipboard: {:?}", e);
                        }
                    }
                }
            }
        };

        let mut cmd_rx = cmd_rx.compat();
        let cmd_handler = async {
            while let Some(Ok(r)) = cmd_rx.next().await {
                match r {
                    Cmd::Grab(types) => {
                        debug!("windows is grabbing the clipboard: {:?}", types);
                        let mut mime_types = vec![OUR_MIME_MARKER.to_owned()];
                        for t in types.types() {
                            match t {
                                ClipboardType::Invalid => (),
                                ClipboardType::Text => mime_types.extend(PLAINTEXT_MIME_TYPES.iter().map(|&s| s.to_owned())),
                                ClipboardType::Image => mime_types.push(CLIPBOARD_MIME_PNG.to_owned()),
                            }
                        }
                        let mut claim = clipboard.claim(mime_types).await;
                        debug!("have a claim, sending it");
                        let controller = controller.clone();
                        tokio::task::spawn_local(async move {
                            while let Some(req) = claim.recv().await {
                                controller.borrow_mut().read_win_clipboard(ClipboardRequestEvent { req });
                            }
                        });
                    }
                    Cmd::Read(typ) => {
                        debug!("windows is reading the clipboard (i.e. pasting): {:?}", typ);
                        let contents = match clipboard.get().await {
                            Some(offer) => {
                                match typ {
                                    ClipboardType::Invalid => None,
                                    ClipboardType::Text => offer.receive_string().await.ok().map(|s| s.into_bytes()),
                                    ClipboardType::Image => offer.receive_bytes(CLIPBOARD_MIME_PNG).await.ok(),
                                }
                            }
                            _ => None,
                        };
                        controller.borrow_mut().respond_win_clipboard(contents.unwrap_or(Vec::new()));
                    }
                }
            }
            debug!("cmd handler going down");
        };

        tokio::join!(clipboard_listener, responder, cmd_handler);
    }
}

pub struct ClipboardRequestEvent {
    req: zerocost_clipboard::ClipboardRequest,
}

const CLIPBOARD_MIME_PNG: &str = "image/png";
impl ClipboardRequestEvent {
    pub fn reply_data(self, response: Vec<u8>) -> ClipboardRequestResponse {
        ClipboardRequestResponse {
            event: self,
            response: ClipboardResponse::Data(response),
        }
    }

    pub fn desired_type(&self) -> ClipboardType {
        if PLAINTEXT_MIME_TYPES.iter().any(|&m| m == self.req.mime_type()) {
            ClipboardType::Text
        } else if self.req.mime_type() == CLIPBOARD_MIME_PNG {
            ClipboardType::Image
        } else {
            // wayland protocol violation, but instead of crashing let's just give them text and see what happens
            ClipboardType::Text
        }
    }
}

enum ClipboardResponse {
    Data(Vec<u8>),
}

pub struct ClipboardRequestResponse {
    event: ClipboardRequestEvent,
    response: ClipboardResponse,
}