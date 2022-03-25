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

use std::collections::HashSet;
use std::io::Error;
use std::rc::Rc;
use std::cell::RefCell;

use futures::unsync::mpsc::{self, UnboundedSender};
use futures::Future;
use futures03::compat::Stream01CompatExt;
use qapi::futures::{QapiStream, QmpStreamTokio};
use qapi::qmp;
use tokio::io::{ReadHalf, WriteHalf};

use crate::controller::Controller;
use futures03::{FutureExt, StreamExt, TryFutureExt};
use tokio::net::UnixStream;

type Send = UnboundedSender<QmpCommand>;
type Handler = Box<dyn Future<Item=(), Error=Error>>;

pub struct Monitor {
    send: Option<Send>,
    recv: Option<mpsc::UnboundedReceiver<QmpCommand>>,
    qapi: Option<QapiStream<QmpStreamTokio<ReadHalf<UnixStream>>, QmpStreamTokio<WriteHalf<UnixStream>>>>,
}

impl Monitor {
    pub async fn new(stream: UnixStream) -> Monitor {
        let (r, w) = tokio::io::split(stream);
        let nego = QmpStreamTokio::open_split(r, w).await.unwrap();
        let qapi = nego.negotiate().await.unwrap();

        let (send, recv) = mpsc::unbounded();

        Monitor {
            send: Some(send),
            recv: Some(recv),
            qapi: Some(qapi),
        }
    }

    pub fn take_send(&mut self) -> Send {
        self.send.take().unwrap()
    }

    pub fn take_handler(&mut self, controller: Rc<RefCell<Controller>>) -> Handler {
        let (qapi, mut events) = self.qapi.take().unwrap().into_parts();
        let event_handler = async move {
            while let Some(a) = events.next().await {
                let event = match a {
                    Err(e) => {
                        warn!("Error reading from QAPI: {:?}", e);
                        return;
                    }
                    Ok(e) => e,
                };

                info!("QAPI event: {:?}", event);
                match event {
                    qapi::qmp::Event::SUSPEND { .. } => {
                        controller.borrow_mut().qemu_suspended();
                    }
                    _ => (),
                }
            }
        };
        let mut commands = self.recv.take().unwrap().compat();
        let command_handler = async move {
            let mut held_keys = HashSet::new();
            while let Some(Ok(cmd)) = commands.next().await {
                let res = match cmd {
                    QmpCommand::DeviceAdd { driver, id, bus, port, hostbus, hostaddr } =>
                        qapi.execute(&qmp::device_add { id: Some(id), bus: Some(bus), driver: driver.to_owned(), arguments: vec![
                            ("port".to_owned(), port.into()),
                            ("hostbus".to_owned(), hostbus.into()),
                            ("hostaddr".to_owned(), hostaddr.into()),
                        ].into_iter().collect() }).await,
                    QmpCommand::DeviceDel { id } => qapi.execute(&qmp::device_del { id }).await,
                    QmpCommand::SystemPowerdown => qapi.execute(&qmp::system_powerdown {}).await,
                    QmpCommand::SystemWakeup => qapi.execute(&qmp::system_wakeup {}).await,
                    QmpCommand::InputSendEvent { events } => {
                        for e in events.as_ref() {
                            match e {
                                &InputEvent::Key { key, down: true } => { held_keys.insert(key); }
                                &InputEvent::Key { key, down: false } => { held_keys.remove(&key); }
                                _ => (),
                            }
                        }
                        let input_send_event = qmp::input_send_event { device: None, head: None, events: events.into_iter().map(|i| i.clone().into()).collect() };
                        qapi.execute(&input_send_event).await
                    }
                    QmpCommand::ReleaseAllKeys => {
                        let events = held_keys.drain().map(|key| InputEvent::Key { key, down: false });
                        let input_send_event = qmp::input_send_event { device: None, head: None, events: events.into_iter().map(|i| i.clone().into()).collect() };
                        qapi.execute(&input_send_event).await
                    }
                };

                if let Err(e) = res {
                    warn!("Error executing QMP command: {:?}", e);
                    if let qapi::ExecuteError::Io(_) = e {
                        // don't loop infinitely trying to read from a broken socket
                        break;
                    }
                }
            }
        };
        let handler = async move {
            tokio::join!(event_handler, command_handler);
            Ok(())
        };
        Box::new(handler.boxed_local().compat())
    }
}
