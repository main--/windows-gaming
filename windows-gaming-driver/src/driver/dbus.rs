use std::collections::VecDeque;

use dbus::{Connection, ConnectionItem, Watch, WatchEvent};
use tokio_core::reactor::{Handle, PollEvented};
use mio::Ready;
use mio::unix::UnixReady;
use futures::{Async, Poll, Stream};

use super::my_io::MyIo;

pub struct DBusItems<'a, 'b> {
    conn: &'a Connection,
    handle: &'b Handle,
    fds: Vec<Fd>,
    pending_items: VecDeque<ConnectionItem>,
}

struct Fd {
    io: PollEvented<MyIo>,
    interest: Ready,
}

impl<'a, 'b> DBusItems<'a, 'b> {
    pub fn new(bus: &'a Connection, handle: &'b Handle) -> DBusItems<'a, 'b> {
        DBusItems {
            handle: handle,
            fds: bus.watch_fds().iter().map(|x| {
                Fd {
                    io: PollEvented::new(MyIo { fd: x.fd() }, &handle).unwrap(),
                    interest: to_ready(&x),
                }
            }).collect(),
            conn: bus,
            pending_items: VecDeque::new(),
        }
    }
}

impl<'a, 'b> Stream for DBusItems<'a, 'b> {
    type Item = ConnectionItem;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<ConnectionItem>, ()> {
        // if we still got data, don't touch the fds
        if let Some(x) = self.pending_items.pop_front() {
            return Ok(Async::Ready(Some(x)));
        }

        // query fds
        let results: Vec<_> = self.fds.iter().flat_map(|fd| match fd.io.poll_ready(fd.interest) {
            Async::Ready(x) => Some((fd.io.get_ref().fd, x)),
            Async::NotReady => None,
        }).collect();

        // borrow things to keep borrowck happy
        let fds = &mut self.fds;
        let conn = &self.conn;
        let handle = &self.handle;
        // make dbus process fds and collect the results
        self.pending_items.extend(
            results.into_iter().flat_map(|(fd, r)| conn.watch_handle(fd, from_ready(r)))
                .flat_map(|ci| match ci {
                    // eat WatchFd events
                    ConnectionItem::WatchFd(w) => {
                        let ready = to_ready(&w);
                        let pos = fds.iter().position(|x| x.io.get_ref().fd == w.fd());
                        if ready.is_empty() {
                            // removed
                            if let Some(i) = pos {
                                fds.remove(i);
                            }
                        } else {
                            // add or update
                            if let Some(i) = pos {
                                fds[i].interest = ready;
                            } else {
                                fds.push(Fd {
                                    io: PollEvented::new(MyIo { fd: w.fd() }, handle).unwrap(),
                                    interest: ready,
                                });
                            }
                        }
                        None
                    }
                    _ => Some(ci),
                }));

        // finally, return results (if any)
        if let Some(x) = self.pending_items.pop_front() {
            Ok(Async::Ready(Some(x)))
        } else {
            Ok(Async::NotReady)
        }
    }
}

// yuck
fn from_ready(r: Ready) -> u32 {
    let mut ret = 0;
    if r.is_readable() {
        ret |= WatchEvent::Readable as u32;
    }
    if r.is_writable() {
        ret |= WatchEvent::Writable as u32;
    }
    if UnixReady::from(r).is_hup() {
        ret |= WatchEvent::Hangup as u32;
    }
    if UnixReady::from(r).is_error() {
        ret |= WatchEvent::Error as u32;
    }
    ret
}

fn to_ready(w: &Watch) -> Ready {
    let mut r = Ready::empty();
    if w.readable() {
        r = r | Ready::readable();
    }
    if w.writable() {
        r = r | Ready::writable();
    }
    r
}
