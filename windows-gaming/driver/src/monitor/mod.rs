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

use std::collections::{HashSet, HashMap};
use std::io::Error;
use std::rc::Rc;
use std::cell::{RefCell, Cell};

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
    send2: Option<Send>,
    recv: Option<mpsc::UnboundedReceiver<QmpCommand>>,
    qapi: Option<QapiStream<QmpStreamTokio<ReadHalf<UnixStream>>, QmpStreamTokio<WriteHalf<UnixStream>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueryCpusFast {}
impl qapi::Command for QueryCpusFast {
	const NAME: &'static str = "query-cpus-fast";
	const ALLOW_OOB: bool = false;

	type Ok = Vec<CpuInfoFast>;
}
impl qapi::qmp::QmpCommand for QueryCpusFast {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "target")]
pub enum CpuInfoFast {
    #[serde(rename = "x86_64")]
    X86_64 {
        #[serde(flatten)]
        base: qapi::qmp::CpuInfoFastBase,
    },
}

impl Monitor {
    pub async fn new(stream: UnixStream) -> Monitor {
        let (r, w) = tokio::io::split(stream);
        let nego = QmpStreamTokio::open_split(r, w).await.unwrap();
        let mut qapi = nego.negotiate().await.unwrap();

        let resp = qapi.execute(QueryCpusFast {}).await.unwrap();
        for c in resp {
            let (cpu, tid) = match c {
                CpuInfoFast::X86_64 { base } => (base.cpu_index, base.thread_id),
            };
            unsafe {
                let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
                libc::CPU_ZERO(&mut cpuset);
                libc::CPU_SET(cpu as usize, &mut cpuset);
                libc::sched_setaffinity(tid as i32, std::mem::size_of_val(&cpuset), &cpuset);
            }
        }

        let (send, recv) = mpsc::unbounded();

        Monitor {
            send2: Some(send.clone()),
            send: Some(send),
            recv: Some(recv),
            qapi: Some(qapi),
        }
    }

    pub fn take_send(&mut self) -> Send {
        self.send.take().unwrap()
    }

    pub fn take_handler(&mut self, controller: Rc<RefCell<Controller>>) -> Handler {
        let send_to_myself = self.send2.take().unwrap();

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
                    qmp::Event::SUSPEND { .. } => {
                        controller.borrow_mut().qemu_suspended();
                    }
                    qmp::Event::BLOCK_JOB_READY { data: qmp::BLOCK_JOB_READY { device, .. }, .. } => {
                        let _ = send_to_myself.unbounded_send(QmpCommand::JobReady(device));
                    }
                    _ => (),
                }
            }
        };
        let mut commands = self.recv.take().unwrap().compat();
        let pending_disk_commits = Rc::new(RefCell::new(HashMap::new()));
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
                    QmpCommand::TakeSnapshot { disk_id, snap_file, ack } => {
                        let res = qapi.execute(qmp::blockdev_snapshot_sync(qmp::BlockdevSnapshotSync {
                            node_name: Some(format!("disk{disk_id}")),
                            snapshot_file: snap_file,
                            snapshot_node_name: Some(format!("disk{disk_id}_snap")),
                            device: None,
                            format: Some("qcow2".to_owned()),
                            mode: None,
                        })).await;
                        if res.is_ok() {
                            let _ = ack.send(());
                        }
                        res
                    }
                    QmpCommand::CommitSnapshot { disk_id, snap_file, ack } => {
                        let jobid = format!("disk{disk_id}");
                        pending_disk_commits.borrow_mut().insert(jobid.clone(), (snap_file, ack));
                        #[allow(deprecated)]
                        qapi.execute(qmp::block_commit {
                            job_id: Some(jobid),
                            device: format!("disk{disk_id}_snap"),
                            base_node: None,

                            base: None,
                            top_node: None,
                            top: None,
                            backing_file: None,
                            speed: None,
                            on_error: None,
                            filter_node_name: None,
                            auto_finalize: None,
                            auto_dismiss: None,
                        }).await
                    }
                    QmpCommand::JobReady(device) => {
                        debug!("committing block job {device}");
                        let (snap_file, ack) = pending_disk_commits.borrow_mut().remove(&device).unwrap();
                        let res = qapi.execute(&qmp::block_job_complete { device: device.clone() }).await;
                        if res.is_ok() {
                            // if the snapshot was created in the same session, qemu will (for some reason I don't fully understand)
                            // automatically remove the blockdev node, causing this to fail
                            // so we just ignore the result and that's it
                            let _ = qapi.execute(&qmp::blockdev_del { node_name: format!("{device}_snap") }).await;

                            tokio::fs::remove_file(snap_file).await.expect("failed to remove snapshot file after applying, please fix manually");
                            let _ = ack.send(());
                        }
                        res
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
