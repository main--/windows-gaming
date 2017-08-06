use std::rc::Rc;
use std::cell::RefCell;
use std::io::Error;

use libdbus::{Connection, ConnectionItem, BusType, Message, OwnedFd};
use tokio_core::reactor::Handle;
use futures::{Stream, Future, IntoFuture};

use dbus::DBusItems;

pub fn system_dbus() -> Connection {
    Connection::get_private(BusType::System).unwrap()
}

fn inhibit(dbus: &Connection) -> OwnedFd {
    let mut ih = Message::new_method_call("org.freedesktop.login1",
                                          "/org/freedesktop/login1",
                                          "org.freedesktop.login1.Manager",
                                          "Inhibit").unwrap();
    ih.append_items(&["sleep".into(),
                      "windows-gaming-driver".into(),
                      "Suspend guest with the host".into(),
                      "delay".into()]);
    // TODO: make this async.
    // for now, it just blocks the entire eventloop *shrug*
    // whatever, this is rare
    let resp = dbus.send_with_reply_and_block(ih, 2000).unwrap();
    resp.get1().unwrap()
}

pub fn sleep_inhibitor<'a, R, F>(bus: &'a Connection, mut callback: F, handle: &'a Handle)
                                 -> Box<Future<Item = (), Error = Error> + 'a>
    where F : FnMut() -> R + 'a, R : IntoFuture<Item = (), Error = ()> + 'a
{
    let items = DBusItems::new(&bus, &handle);

    bus.add_match("interface='org.freedesktop.login1.Manager',member='PrepareForSleep'").unwrap();

    let fd = Rc::new(RefCell::new(Some(inhibit(&bus))));

    Box::new(items.for_each(move |ci| {
        match ci {
            ConnectionItem::Signal(ref s) if &*s.interface().unwrap() == "org.freedesktop.login1.Manager"
                    && &*s.member().unwrap() == "PrepareForSleep" => {
                let starting: bool = s.get1().unwrap();
                debug!("dbus reports PrepareForSleep");
                if starting {
                    // run callback, then drop
                    let myfd = fd.clone();
                    let b: Box<Future<Item=(), Error=()>> = Box::new(callback().into_future().map(move |()| {
                        *myfd.borrow_mut() = None;
                    }));
                    return b;
                } else {
                    // re-acquire
                    *fd.borrow_mut() = Some(inhibit(&bus));
                }
            }
            _ => (),
        }
        Box::new(Ok(()).into_future())
    }).then(|_| Ok(())))
}
