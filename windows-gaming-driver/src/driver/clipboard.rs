use std::rc::Rc;
use std::cell::RefCell;
use std::os::unix::io::AsRawFd;

use futures::{Async, Stream, Future};
use futures::unsync::mpsc::UnboundedReceiver;
use tokio_core::reactor::{Handle, PollEvented};
use xcb::{self, Atom, Connection, ConnError, GenericEvent, GenericError, Window, Timestamp,
          SelectionRequestEvent, SelectionNotifyEvent,
          SELECTION_REQUEST, SELECTION_CLEAR, SELECTION_NOTIFY, PROP_MODE_REPLACE};

use super::my_io::MyIo;
use super::controller::Controller;


struct XcbEvents<'a> {
    connection: &'a Connection,
    my_io: PollEvented<MyIo>,
}

impl<'a> XcbEvents<'a> {
    fn new(connection: &'a Connection, handle: &Handle) -> XcbEvents<'a> {
        XcbEvents {
            connection,
            my_io: PollEvented::new(MyIo { fd: connection.as_raw_fd() }, handle).unwrap(),
        }
    }
}

impl<'a> Stream for XcbEvents<'a> {
    type Item = GenericEvent;
    type Error = ConnError;

    fn poll(&mut self) -> Result<Async<Option<GenericEvent>>, ConnError> {
        if let Some(event) = self.connection.poll_for_queued_event() {
            return Ok(Async::Ready(Some(event)));
        }

        match self.my_io.poll_read() {
            Async::Ready(()) => match self.connection.poll_for_event() {
                Some(event) => Ok(Async::Ready(Some(event))),
                None => self.connection.has_error().map(|()| {
                    self.my_io.need_read();
                    Async::NotReady
                }),
            },
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

fn get_atom(connection: &Connection, name: &str) -> Result<Atom, GenericError> {
    xcb::intern_atom(connection, false, name).get_reply().map(|reply| reply.atom())
}

struct Atoms {
    clipboard: Atom,
    targets: Atom,
    utf8_string: Atom,
    property: Atom,
}

pub struct X11Clipboard {
    connection: Connection,
    window: u32,
    atoms: Atoms,
}

impl X11Clipboard {
    pub fn open() -> Result<X11Clipboard, GenericError> {
        let (connection, screen) = Connection::connect(None).unwrap();

        let window = connection.generate_id();

        // borrowing a little from the x11-clipboard crate here
        {
            let screen = connection.get_setup().roots().nth(screen as usize)
                .expect("Invalid X11 screen!");

            xcb::create_window(
                &connection,
                xcb::COPY_FROM_PARENT as u8,
                window, screen.root(),
                0, 0, 1, 1,
                0,
                xcb::WINDOW_CLASS_INPUT_OUTPUT as u16,
                screen.root_visual(),
                &[(
                    xcb::CW_EVENT_MASK,
                    xcb::EVENT_MASK_STRUCTURE_NOTIFY | xcb::EVENT_MASK_PROPERTY_CHANGE
                )]
            );
            connection.flush();
        }

        let clipboard = get_atom(&connection, "CLIPBOARD")?;
        let property = get_atom(&connection, "THIS_CLIPBOARD_OUT")?;
        let targets = get_atom(&connection, "TARGETS")?;
        let utf8_string = get_atom(&connection, "UTF8_STRING")?;
        // let incr = get_atom(&connection, "INCR")?; // not (yet?) implemented

        Ok(X11Clipboard { connection, window, atoms: Atoms { clipboard, targets, utf8_string, property } })
    }

    pub fn grab_clipboard(&self) {
        xcb::set_selection_owner(&self.connection, self.window, self.atoms.clipboard, xcb::CURRENT_TIME);
        self.connection.flush();
    }

    pub fn read_clipboard(&self) {
        xcb::convert_selection(&self.connection, self.window, self.atoms.clipboard, self.atoms.utf8_string,
                               self.atoms.property, xcb::CURRENT_TIME);
        self.connection.flush();
    }

    pub fn run<'a>(&'a self,
                   controller: Rc<RefCell<Controller>>,
                   resp_recv: UnboundedReceiver<ClipboardRequestResponse>,
                   handle: &Handle) -> Box<Future<Item=(), Error=::std::io::Error> + 'a> {
        let xcb_listener = XcbEvents::new(&self.connection, handle).for_each(move |event| {
            debug!("XCB event {}", event.response_type());
            match event.response_type() & !0x80 {
                SELECTION_REQUEST => {
                    let event: &SelectionRequestEvent = unsafe { xcb::cast_event(&event) };
                    controller.borrow_mut().read_win_clipboard(ClipboardRequestEvent {
                        time: event.time(),
                        requestor: event.requestor(),
                        selection: event.selection(),
                        target: event.target(),
                        property: event.property(),
                    });
                }
                SELECTION_CLEAR => {
                    controller.borrow_mut().grab_win_clipboard();
                }
                SELECTION_NOTIFY => {
                    let event: &SelectionNotifyEvent = unsafe { xcb::cast_event(&event) };
                    if let Ok(reply) = xcb::get_property(
                        &self.connection, false, self.window,
                        event.property(), self.atoms.utf8_string, 0, ::std::u32::MAX // FIXME reasonable buffer size
                    ).get_reply() {
                        assert!(reply.type_() == self.atoms.utf8_string);

                        controller.borrow_mut().respond_win_clipboard(reply.value().to_vec());
                        xcb::delete_property(&self.connection, self.window, self.atoms.property);
                    } else {
                        controller.borrow_mut().respond_win_clipboard(Vec::new());
                    }
                }
                _ => info!("Unknown XCB event: {}", event.response_type()),//unimplemented!(),
            }

            Ok(())
        });

        let responder = resp_recv.for_each(move |response: ClipboardRequestResponse| {
            let event = &response.event;

            if event.target == self.atoms.targets {
                xcb::change_property(
                    &self.connection, PROP_MODE_REPLACE as u8,
                    event.requestor, event.property, xcb::ATOM_ATOM, 32,
                    &[self.atoms.targets, self.atoms.utf8_string],
                );

            } else if event.target == self.atoms.utf8_string {
                xcb::change_property(
                    &self.connection, PROP_MODE_REPLACE as u8,
                    event.requestor, event.property, event.target, 8,
                    &response.response
                );
            }
            // else do nothing: we don't set anything so they realize that this format is unsupported

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
    }
}

pub struct ClipboardRequestEvent {
    time: Timestamp,
    requestor: Window,
    selection: Atom,
    target: Atom,
    property: Atom,
}

impl ClipboardRequestEvent {
    pub fn reply(self, response: Vec<u8>) -> ClipboardRequestResponse {
        ClipboardRequestResponse {
            event: self,
            response,
        }
    }
}

pub struct ClipboardRequestResponse {
    event: ClipboardRequestEvent,
    response: Vec<u8>,
}
