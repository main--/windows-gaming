use std::io::Cursor;
use std::iter::once;

use std::rc::Rc;
use std::cell::RefCell;


use futures::{Stream, Future};
use futures::unsync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use futures03::{FutureExt, TryFutureExt, TryStreamExt, StreamExt};

use crate::controller::Controller;
use crate::clientpipe::ClipboardType;
use tokio_stream::wrappers::{UnboundedReceiverStream, WatchStream};

use zerocost_clipboard;

static OUR_MIME_MARKER: &'static str = "application/from-windows";

pub struct X11Clipboard {
    /*
    connection: Connection,
    window: u32,
    atoms: Atoms,
    */
    clipboard: zerocost_clipboard::WaylandClipboard,
    run: RefCell<Option<Box<dyn Future<Item=(), Error=anyhow::Error>>>>,

    cmd_tx: UnboundedSender<Cmd>,
    cmd_rx: RefCell<Option<UnboundedReceiver<Cmd>>>,
}

#[derive(Debug)]
enum Cmd {
    Grab,
    Read(ClipboardType),
}

impl X11Clipboard {
    pub fn open() -> Box<dyn Future<Item=X11Clipboard, Error=anyhow::Error>> {
        trace!("opening wayland clipboard");
        let task = zerocost_clipboard::WaylandClipboard::init().boxed().compat();
        Box::new(task.map(|(run, clipboard)| {
            let run: Box<dyn Future<Item=(), Error=anyhow::Error>> = Box::new(run.boxed_local().compat());
            let run = RefCell::new(Some(run));
            let (cmd_tx, cmd_rx) = mpsc::unbounded();
            trace!("opened wayland clipboard");
            X11Clipboard { run, clipboard, cmd_tx, cmd_rx: RefCell::new(Some(cmd_rx)) }
        }))
    }

    pub fn grab_clipboard(&self) {
        self.cmd_tx.unbounded_send(Cmd::Grab).unwrap();
    }

    pub fn read_clipboard(&self, kind: ClipboardType) {
        self.cmd_tx.unbounded_send(Cmd::Read(kind)).unwrap();
    }

    pub fn run<'a>(&'a self,
                   controller: Rc<RefCell<Controller>>,
                   resp_recv: UnboundedReceiver<ClipboardRequestResponse>,
                   ) -> Box<dyn Future<Item=(), Error=::std::io::Error> + 'a> {
        trace!("running wayland clipboard");
        let cmd_rx = self.cmd_rx.borrow_mut().take().unwrap();

        let clipboard = &self.clipboard;
        let run = self.run.borrow_mut().take().unwrap();
        Box::new(run.map_err(|_| unreachable!()).join(
        self.clipboard.subscribe().never_error().boxed().compat().and_then(move |clipboard_rx| {
            let clipboard_sub = WatchStream::new(clipboard_rx);
            let controller2 = controller.clone();
            let clipboard_listener = clipboard_sub.map(Ok).compat().for_each(move |offer| {
                debug!("got clipboard offer");
                if let Some(offer) = offer {
                    if !offer.mime_types().contains(OUR_MIME_MARKER) {
                        debug!("it is foreign, so we grab the clipboard");
                        // only react if it's not from us
                        controller2.borrow_mut().grab_win_clipboard();
                    } else {
                        debug!("but we reject it because it's ours");
                    }
                }
                Ok(())
            });
            let responder = resp_recv.for_each(move |response: ClipboardRequestResponse| {
                match response.response {
                    ClipboardResponse::Types(_kinds) => {
                        todo!();
                    }
                    ClipboardResponse::Data(buf) => {
                        debug!("responding to wayland with clipboard data");
                        let mut target = response.event.req.into_target();
                        tokio::spawn(async move {
                            let res = tokio::io::copy_buf(&mut Cursor::new(buf), &mut target).await;
                            if let Err(e) = res {
                                error!("Error responding to wayland clipboard: {:?}", e);
                            }
                        });
                    }
                }
                Ok(())
            });

            let (claim_tx, claim_rx) = mpsc::unbounded();
            let controller2 = controller.clone();
            let cmd_handler = cmd_rx.for_each(move |r| {
                match r {
                    Cmd::Grab => {
                        debug!("windows is grabbing the clipboard");
                        let claim_tx = claim_tx.clone();
                        clipboard.claim(zerocost_clipboard::PLAINTEXT_MIME_TYPES.iter().chain(once(&OUR_MIME_MARKER)).map(|&s| s.to_owned()))
                            .map(move |sender| claim_tx.unbounded_send(sender).unwrap())
                            .unit_error().boxed_local().compat()
                    }
                    Cmd::Read(_) => {
                        debug!("windows is reading the clipboard (i.e. pasting)");
                        let controller = controller2.clone();
                        clipboard.get().then(move |c| c.unwrap().receive_string()).map(move |s| {
                                let contents = s.unwrap().into_bytes();
                                controller.borrow_mut().respond_win_clipboard(contents);
                            })
                            .unit_error().boxed_local().compat()
                    }
                }
            });

            let controller = controller.clone();
            let claim_handler = claim_rx.for_each(move |claim| {
                let controller = controller.clone();
                UnboundedReceiverStream::new(claim).map(Ok).boxed().compat().for_each(move |req| {
                    controller.borrow_mut().read_win_clipboard(ClipboardRequestEvent { req });
                    Ok(())
                })
            });

            clipboard_listener
                .join(responder.map_err(|()| unreachable!()))
                .join(cmd_handler.map_err(|()| unreachable!()))
                .join(claim_handler.map_err(|()| unreachable!()))
                .then(|_| Ok(()))
        }).map_err(|_| std::io::Error::last_os_error())).map(|((), ())| ()))
        /*
        let clipboard_sub = WatchStream::new(self.clipboard.subscribe());
        //let clipboard_listener = clipboard_sub.
        todo!();
        */
        /*
        let xcb_listener = XcbEvents::new(&self.connection, handle).for_each(move |event| {
            trace!("XCB event {}", event.response_type());
            match event.response_type() & !0x80 {
                SELECTION_REQUEST => {
                    let event: &SelectionRequestEvent = unsafe { xcb::cast_event(&event) };
                    controller.borrow_mut().read_win_clipboard(ClipboardRequestEvent {
                        time: event.time(),
                        requestor: event.requestor(),
                        selection: event.selection(),
                        target: event.target(),
                        property: event.property(),
                        desired_type: self.atom_to_cliptype(event.target()).unwrap_or(ClipboardType::Text),
                    });
                }
                SELECTION_CLEAR => {
                    controller.borrow_mut().grab_win_clipboard();
                }
                SELECTION_NOTIFY => {
                    let event: &SelectionNotifyEvent = unsafe { xcb::cast_event(&event) };
                    let reply = xcb::get_property(
                        &self.connection, false, self.window,
                        event.property(), xcb::ATOM_ANY, 0, ::std::u32::MAX // FIXME reasonable buffer size
                    ).get_reply();

                    if event.target() == self.atoms.targets {
                        // this is type info (targets)
                        let formats = reply.as_ref().map(|reply| {
                            let formats: &[Atom] = reply.value();
                            let formats = formats.iter().filter_map(|&x| self.atom_to_cliptype(x)).collect();
                            formats
                        }).unwrap_or(Vec::new());

                        controller.borrow_mut().respond_win_types(formats);
                    } else {
                        let contents = reply.as_ref().map(|x| x.value().to_vec()).unwrap_or(Vec::new());
                        controller.borrow_mut().respond_win_clipboard(contents);
                    }

                    if reply.is_ok() {
                        xcb::delete_property(&self.connection, self.window, self.atoms.property);
                    }
                }
                PROPERTY_NOTIFY => trace!("Ignoring PROPERTY_NOTIFY event"),
                _ => warn!("Unknown XCB event: {}", event.response_type()),
            }

            Ok(())
        });

        let responder = resp_recv.for_each(move |response: ClipboardRequestResponse| {
            let event = &response.event;

            match response.response {
                ClipboardResponse::Types(kinds) => {
                    let mut kinds: Vec<_> = kinds.iter().filter_map(|&x| self.cliptype_to_atom(x)).collect();
                    kinds.push(self.atoms.targets);
                    xcb::change_property(&self.connection, PROP_MODE_REPLACE as u8,
                                         event.requestor, event.property, xcb::ATOM_ATOM, 32,
                                         &kinds);
                }
                ClipboardResponse::Data(buf) => {
                    xcb::change_property(&self.connection, PROP_MODE_REPLACE as u8,
                                         event.requestor, event.property, event.target, 8, &buf);
                }
            }
            // TODO: right now we default to requesting unknown formats as text
            // this is potentially bad
            // we should not set anything here for unsupported formats

            xcb::send_event(
                &self.connection, false, event.requestor, 0,
                &SelectionNotifyEvent::new(
                    event.time,
                    event.requestor,
                    event.selection,
                    event.target,
                    event.property
                )
            );
            self.connection.flush();

            Ok(())
        });

        Box::new(xcb_listener.join(responder.map_err(|()| unreachable!())).then(|_| Ok(())))
        */
    }
}

pub struct ClipboardRequestEvent {
    /*
    time: Timestamp,
    requestor: Window,
    selection: Atom,
    target: Atom,
    property: Atom,
    desired_type: ClipboardType,
    */
    req: zerocost_clipboard::ClipboardRequest,
}

impl ClipboardRequestEvent {
    pub fn reply_data(self, response: Vec<u8>) -> ClipboardRequestResponse {
        ClipboardRequestResponse {
            event: self,
            response: ClipboardResponse::Data(response),
        }
    }

    pub fn reply_types(self, types: Vec<ClipboardType>) -> ClipboardRequestResponse {
        ClipboardRequestResponse {
            event: self,
            response: ClipboardResponse::Types(types),
        }
    }

    pub fn desired_type(&self) -> ClipboardType {
        ClipboardType::Text
        //self.desired_type
    }
}

enum ClipboardResponse {
    Types(Vec<ClipboardType>),
    Data(Vec<u8>),
}

pub struct ClipboardRequestResponse {
    event: ClipboardRequestEvent,
    response: ClipboardResponse,
}