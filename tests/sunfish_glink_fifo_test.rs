// Ring/peer simulation plus the actual volatile backend on private host memory.
#[path = "../rust/src/qcom_glink_fifo.rs"]
pub mod qcom_glink_fifo;
#[path = "../rust/src/qcom_glink_native_io.rs"]
pub mod qcom_glink_native_io;
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
pub mod qcom_ipa_service;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[path = "../rust/src/qcom_msa.rs"]
pub mod qcom_msa;
#[path = "../rust/src/qcom_qmi.rs"]
pub mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
pub mod qcom_qmi_router;
#[path = "../rust/src/qcom_qrtr_control.rs"]
pub mod qcom_qrtr_control;
#[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
#[path = "../rust/src/qcom_qrtr_flow.rs"]
pub mod qcom_qrtr_flow;
#[path = "../rust/src/qcom_qrtr_tftp.rs"]
pub mod qcom_qrtr_tftp;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[path = "../rust/src/qcom_wlfw_bdf.rs"]
pub mod qcom_wlfw_bdf;
#[path = "../rust/src/qcom_wlfw_client.rs"]
pub mod qcom_wlfw_client;
#[allow(dead_code)]
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;

use qcom_glink_fifo::{
    Adapter, Barrier, Error, Fault, Io, Observation, Receive, Transmit, FIFO_BYTES,
};
use qcom_glink_native_io::{Gate, Mappings, NativeIo};
use qcom_glink_qmi::{Config, Event, Published, Reply, RequestHandle, Session};
use qcom_modem_services::{MemoryRequest, Request, Service};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug, PartialEq)]
enum Op {
    #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
    Snapshot,
    Observe,
    Desc(usize),
    ReadRx(usize, usize),
    ReadTx(usize, usize),
    Write(usize, Vec<u8>),
    Head(u32),
    Tail(u32),
    Barrier(Barrier),
    Notify,
}

struct Memory {
/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
    append_after_notify: Option<Vec<u8>>,
/* MCFG_IDLE_DETAIL_END */
    words: [u32; 8],
    tx: Vec<u8>,
    rx: Vec<u8>,
    observation: Observation,
    #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
    observation_after_barrier: Option<Observation>,
    #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
    late_barrier_observation: Option<(Barrier, usize, Observation)>,
    log: Vec<Op>,
    fail: Option<&'static str>,
    once: bool,
    corrupt_readback: bool,
    mutate_rx: bool,
    read_rx_calls: usize,
    revoke_after_write: bool,
    fail_after_store: bool,
    append_on_rx_read: bool,
    panic_on_notify: bool,
    #[cfg(feature = "sunfish-glink-announcement-reuse")]
    append_notification_on_write: bool,
    diagnostic: u32,
}
#[derive(Clone)]
struct Fake(Rc<RefCell<Memory>>);

#[cfg(feature = "sunfish-ipa-reg-close-fifo")]
include!("sunfish_ipa_close_fifo_fixture.rs");

impl Fake {
    fn new(tx: u32, rx: u32) -> Self {
        Self(Rc::new(RefCell::new(Memory {
            words: [tx, tx, rx, rx, 0x1234, 2, 3, 4],
/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
            append_after_notify: None,
/* MCFG_IDLE_DETAIL_END */
            tx: vec![0xa5; FIFO_BYTES],
            rx: vec![0x5a; FIFO_BYTES],
            observation: Observation {
                generation: 1,
                identity: 99,
                ready: true,
                fatal: false,
            },
            #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
            observation_after_barrier: None,
            #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
            late_barrier_observation: None,
            log: Vec::new(),
            fail: None,
            once: false,
            corrupt_readback: false,
            mutate_rx: false,
            read_rx_calls: 0,
            revoke_after_write: false,
            fail_after_store: false,
            append_on_rx_read: false,
            panic_on_notify: false,
            #[cfg(feature = "sunfish-glink-announcement-reuse")]
            append_notification_on_write: false,
            diagnostic: 0,
        })))
    }
    fn log(&self) -> Vec<Op> {
        self.0.borrow().log.clone()
    }
    fn clear(&self) {
        self.0.borrow_mut().log.clear();
    }
    fn set_diagnostic(&self, code: u32) {
        self.0.borrow_mut().diagnostic = code;
    }
    fn op(&self, name: &'static str, op: Op) -> Result<(), Fault> {
        #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
        inject_at_registration_write(self, name, &op);
        let mut m = self.0.borrow_mut();
        m.log.push(op);
        if m.fail == Some(name) {
            if m.once {
                m.fail = None;
            }
            return Err(Fault);
        }
        Ok(())
    }
    fn push(&self, bytes: &[u8]) {
        assert_eq!(bytes.len() % 8, 0);
        let mut m = self.0.borrow_mut();
        let start = m.words[3] as usize;
        for (index, value) in bytes.iter().enumerate() {
            m.rx[(start + index) % FIFO_BYTES] = *value;
        }
        m.words[3] = ((start + bytes.len()) % FIFO_BYTES) as u32;
    }
    fn take_tx(&self) -> Vec<u8> {
        let mut m = self.0.borrow_mut();
        let start = m.words[0] as usize;
        let count = (m.words[1] as usize + FIFO_BYTES - start) % FIFO_BYTES;
        let bytes = (0..count)
            .map(|index| m.tx[(start + index) % FIFO_BYTES])
            .collect();
        m.words[0] = m.words[1];
        bytes
    }
}

impl Io for Fake {
    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
    fn elf_io_totals(&self) -> Option<[u64; 12]> {
        // Synthetic monotonic cost while this fixture keeps its operation log.
        // Reading the diagnostic itself must not add an I/O operation.
        Some([self.0.borrow().log.len() as u64; 12])
    }
    fn diagnostic(&self) -> u32 {
        self.0.borrow().diagnostic
    }
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.op("observe", Op::Observe)?;
        Ok(self.0.borrow().observation)
    }
    fn descriptor(&mut self, word: usize) -> Result<u32, Fault> {
        self.op("descriptor", Op::Desc(word))?;
        Ok(self.0.borrow().words[word])
    }
    #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
    fn cursor_snapshot(&mut self) -> Result<(Option<usize>, [u32; 8]), Fault> {
        self.op("snapshot", Op::Snapshot)?;
        Ok((self.rx_length()?, self.descriptors()?))
    }
    fn read_rx(&mut self, offset: usize, out: &mut [u8]) -> Result<(), Fault> {
        assert!(offset + out.len() <= FIFO_BYTES);
        self.op("read_rx", Op::ReadRx(offset, out.len()))?;
        let mut m = self.0.borrow_mut();
        m.read_rx_calls += 1;
        out.copy_from_slice(&m.rx[offset..offset + out.len()]);
        if m.append_on_rx_read {
            m.append_on_rx_read = false;
            let head = m.words[3] as usize;
            for (i, byte) in [13, 0, 0, 0, 0, 0, 0, 0].into_iter().enumerate() {
                m.rx[(head + i) % FIFO_BYTES] = byte;
            }
            m.words[3] = ((head + 8) % FIFO_BYTES) as u32;
        }
        if m.mutate_rx && m.read_rx_calls > 1 && !out.is_empty() {
            out[0] ^= 1;
        }
        Ok(())
    }
    fn read_tx(&mut self, offset: usize, out: &mut [u8]) -> Result<(), Fault> {
        assert!(offset + out.len() <= FIFO_BYTES);
        self.op("read_tx", Op::ReadTx(offset, out.len()))?;
        let m = self.0.borrow();
        out.copy_from_slice(&m.tx[offset..offset + out.len()]);
        if m.corrupt_readback && !out.is_empty() {
            out[0] ^= 1;
        }
        Ok(())
    }
    fn write_tx(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Fault> {
        assert!(offset + bytes.len() <= FIFO_BYTES);
        self.op("write_tx", Op::Write(offset, bytes.to_vec()))?;
        let mut m = self.0.borrow_mut();
        #[cfg(feature = "sunfish-glink-announcement-reuse")]
        if m.append_notification_on_write {
            m.append_notification_on_write = false;
            let record = [13u8,0,0,0,0,0,0,0];
            let at = m.words[3] as usize;
            for (i,b) in record.into_iter().enumerate(){m.rx[(at+i)%FIFO_BYTES]=b;}
            m.words[3] = ((at+8)%FIFO_BYTES) as u32;
        }
        m.tx[offset..offset + bytes.len()].copy_from_slice(bytes);
        if m.revoke_after_write {
            m.observation.ready = false;
        }
        Ok(())
    }
    fn store_tx_head(&mut self, value: u32) -> Result<(), Fault> {
        self.op("head", Op::Head(value))?;
        let mut m = self.0.borrow_mut();
        #[cfg(feature = "sunfish-oack-grant-ack-turn")]
        let ack = m.tx[m.words[1] as usize] == 8;
        #[cfg(feature = "sunfish-oack-grant-ack-turn")]
        if ack && m.fail == Some("ack_head_before") { return Err(Fault); }
        #[cfg(feature = "sunfish-first-oack-spare-buffer")]
        let advertisement = m.tx[m.words[1] as usize] == 5;
        #[cfg(feature = "sunfish-first-oack-spare-buffer")]
        if advertisement && m.fail == Some("intent_head_before") { return Err(Fault); }
        m.words[1] = value;
        #[cfg(feature = "sunfish-first-oack-spare-buffer")]
        if advertisement && m.fail == Some("intent_head_after") { return Err(Fault); }
        #[cfg(feature = "sunfish-oack-grant-ack-turn")]
        if ack && m.fail == Some("ack_head_after") { return Err(Fault); }
        if m.fail_after_store {
            return Err(Fault);
        }
        Ok(())
    }
    fn store_rx_tail(&mut self, value: u32) -> Result<(), Fault> {
        self.op("tail", Op::Tail(value))?;
        let mut m = self.0.borrow_mut();
        m.words[2] = value;
        if m.fail_after_store {
            return Err(Fault);
        }
        Ok(())
    }
    fn barrier(&mut self, kind: Barrier) -> Result<(), Fault> {
        self.op("barrier", Op::Barrier(kind))?;
        #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
        {
            let mut m = self.0.borrow_mut();
            if let Some(observation) = m.observation_after_barrier.take() {
                m.observation = observation;
            }
            if let Some((target, nth, observation)) = m.late_barrier_observation {
                let count = m.log.iter().filter(|op| **op == Op::Barrier(target)).count();
                if kind == target && count == nth {
                    m.observation = observation;
                    m.late_barrier_observation = None;
                }
            }
        }
        Ok(())
    }
    fn notify_peer(&mut self) -> Result<(), Fault> {
        self.op("notify", Op::Notify)?;
        assert!(
            !self.0.borrow().panic_on_notify,
            "notification outcome lost"
        );
/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
        {let bytes=self.0.borrow_mut().append_after_notify.take();if let Some(bytes)=bytes{self.push(&bytes);}}
/* MCFG_IDLE_DETAIL_END */
        Ok(())
    }
}

fn session() -> Session {
    let mut s = Session::new(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap();
    s.register(1, Service::HeapMemory, 0x4001).unwrap();
    s
}
fn adapter(tx: u32, rx: u32) -> (Fake, Adapter<Fake>) {
    let io = Fake::new(tx, rx);
    let a = Adapter::attach(io.clone(), session(), 1).unwrap();
    io.clear();
    (io, a)
}

#[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
#[test]
fn rx_backlog_snapshot_uses_only_last_validated_cursor_sample() {
    let (peer, mut a) = adapter(0, 0);
    assert_eq!(a.tftp_rx_snapshot(), Some((0, 0)));
    peer.push(&[13, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(a.tftp_rx_snapshot(), Some((0, 0)), "unsampled peer changes remain unobserved");
    a.revalidate(1).unwrap();
    let before = peer.log();
    assert_eq!(a.tftp_rx_snapshot(), Some((8, 0)));
    assert_eq!(peer.log(), before, "snapshot must not perform device I/O");
    assert_eq!(a.poll_receive(1), Ok(Receive::ReadNotification));
    assert_eq!(a.tftp_rx_snapshot(), Some((0, 0)));
}

#[test]
fn msa_reservation_identity_must_match_the_live_glink_handoff() {
    let (_, mut adapter) = adapter(0, 0);
    let reservation = crate::qcom_msa::Reservation::validate(
        1,
        98,
        crate::qcom_msa::SUNFISH_MSA_BASE,
        crate::qcom_msa::SUNFISH_MSA_BYTES,
        true,
        true,
        &[],
    )
    .unwrap();
    assert_eq!(
        adapter.begin_wlfw_msa_info(1, reservation),
        Err(Error::IdentityChanged)
    );
    assert_eq!(adapter.wlfw_state(), None);
}

#[test]
fn adapter_preserves_backend_diagnostic_without_weakening_failed_gate() {
    let (io, mut adapter) = adapter(0, 0);
    io.set_diagnostic(0x1234);
    assert_eq!(adapter.io_diagnostic(), 0x1234);
    io.0.borrow_mut().observation.ready = false;
    assert_eq!(adapter.revalidate(1), Err(Error::NotReady));
    assert_eq!(adapter.io_diagnostic(), 0x1234);
}

fn control(command: u16, channel: u16, parameter: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(command.to_le_bytes());
    bytes.extend(channel.to_le_bytes());
    bytes.extend(parameter.to_le_bytes());
    bytes
}
fn data(intent: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = control(9, 2, intent);
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(payload);
    bytes.resize((bytes.len() + 7) & !7, 0);
    bytes
}
fn qrtr(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [1, kind, 0, 9, 0, payload.len() as u32, 1, 0x4001] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(payload);
    bytes.resize((bytes.len() + 3) & !3, 0);
    bytes
}
fn query(intent: u32) -> Vec<u8> {
    data(
        intent,
        &qrtr(1, &[0, 17, 0, 0x24, 0, 7, 0, 1, 4, 0, 1, 0, 0, 0]),
    )
}
fn advertised(a: &mut Adapter<Fake>, peer: &Fake) -> u32 {
    let id = a.offer_receive(1).unwrap();
    assert_eq!(
        a.publish_next(1),
        Ok(Transmit::Session(Published::ReceiveIntent(id)))
    );
    assert_eq!(
        peer.take_tx(),
        vec![
            5,
            0,
            1,
            0,
            1,
            0,
            0,
            0,
            0,
            16,
            0,
            0,
            id as u8,
            (id >> 8) as u8,
            (id >> 16) as u8,
            (id >> 24) as u8
        ]
    );
    id
}
fn receive_handle(a: &mut Adapter<Fake>) -> RequestHandle {
    match a.poll_receive(1).unwrap() {
        Receive::Session(Event::Request(handle)) => handle,
        event => panic!("{event:?}"),
    }
}
fn writes(log: &[Op]) -> Vec<Op> {
    log.iter()
        .filter(|op| matches!(op, Op::Write(..) | Op::Head(_) | Op::Tail(_) | Op::Notify))
        .cloned()
        .collect()
}

#[test]
fn attach_and_idle_do_not_initialize_or_modify_existing_rings() {
    let (peer, mut a) = adapter(0x3ff8, 0x3ff0);
    let before = peer.0.borrow().words;
    assert_eq!(a.poll_receive(1), Ok(Receive::Idle));
    assert_eq!(a.publish_next(1), Ok(Transmit::Idle));
    assert!(writes(&peer.log()).is_empty());
    assert_eq!(peer.0.borrow().words, before);
}

#[test]
fn capacity_diagnostic_retains_rejected_intent_batch_without_advancing_tail() {
    let (peer, mut adapter) = adapter(0, 0);
    let mut batch = control(5, 2, 32);
    for id in 0u32..32 { batch.extend(4096u32.to_le_bytes()); batch.extend(id.to_le_bytes()); }
    peer.push(&batch);
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RemoteIntents)));
    let mut extra = control(5, 2, 1);
    extra.extend(4096u32.to_le_bytes()); extra.extend(32u32.to_le_bytes());
    peer.push(&extra);
    let tail = peer.0.borrow().words[2];
    assert_eq!(adapter.poll_receive(1), Err(Error::Session(qcom_glink_qmi::Error::NoCapacity)));
    assert_eq!(peer.0.borrow().words[2], tail);
    let info = adapter.capacity_diagnostic().unwrap();
    assert_eq!((info.phase, info.command, info.parameter), (1, 5, 1));
    assert_eq!((info.remote_free, info.remote_busy), (0, 0));
    assert_eq!(info.rx_free, qcom_glink_qmi::RX_SLOTS as u32);
    assert_eq!(adapter.poll_receive(1), Err(Error::Quarantined));
}

#[cfg(feature = "sunfish-qrtr-deferred-requests")]
#[test]
fn full_qmi_queue_defers_request_but_drains_glink_completion_without_replay() {
    let (peer, mut adapter) = adapter(0, 0);
    let mut intent = control(5, 2, 1);
    intent.extend(4096u32.to_le_bytes()); intent.extend(7u32.to_le_bytes());
    peer.push(&intent);
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RemoteIntents)));
    let make_request = |id, txn: u16| {
        let mut qmi = vec![0, 0, 0, 0x24, 0, 7, 0, 1, 4, 0, 1, 0, 0, 0];
        qmi[1..3].copy_from_slice(&txn.to_le_bytes());
        data(id, &qrtr(1, &qmi))
    };
    let mut admitted = Vec::new();
    for txn in 1..=4 {
        let id = advertised(&mut adapter, &peer);
        peer.push(&make_request(id, txn));
        admitted.push(receive_handle(&mut adapter));
    }
    assert_eq!(adapter.in_flight(), 4);
    let fifth = advertised(&mut adapter, &peer);
    peer.push(&make_request(fifth, 5));
    let before = peer.0.borrow().words[2];
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RequestDeferred)));
    assert_ne!(peer.0.borrow().words[2], before, "copied record is consumed exactly once");
    assert_eq!(adapter.in_flight(), 4);

    adapter.queue_reply(1, &admitted[0], qcom_glink_qmi::Reply::Failure { error: 3 }).unwrap();
    let mut sent = false;
    for _ in 0..8 {
        match adapter.publish_next(1).unwrap() {
            Transmit::Session(Published::Data { remote_intent: 7, last_fragment: true }) => { sent = true; break; }
            Transmit::Session(Published::ReceiveDone(id)) => assert_ne!(id, fifth),
            other => panic!("unexpected publication: {other:?}"),
        }
    }
    assert!(sent);
    peer.push(&control(6, 2, 7));
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::ReplyRxDone(admitted[0]))));
    peer.take_tx();
    let credit_intent = advertised(&mut adapter, &peer);
    let mut body = vec![0; 20];
    body[0] = 7;
    body[8] = 9;
    peer.push(&data(credit_intent, &qrtr(7, &body)));
    assert!(matches!(adapter.poll_receive(1), Ok(Receive::Session(Event::ReplyQrtrResume(_)))));
    let tail = peer.0.borrow().words[2];
    let resumed = match adapter.poll_receive(1).unwrap() {
        Receive::Session(Event::Request(handle)) => handle,
        other => panic!("expected the deferred request, got {other:?}"),
    };
    assert_eq!(peer.0.borrow().words[2], tail, "retry must not reread the FIFO");
    assert_eq!(adapter.request(1, &resumed).unwrap().0.transaction, 5);
    assert_eq!(adapter.in_flight(), 4);
}

#[test]
fn tx_wrap_publishes_payload_then_one_head_then_doorbell() {
    let (peer, mut a) = adapter(0x3ff8, 0);
    a.offer_receive(1).unwrap();
    peer.clear();
    assert_eq!(
        a.publish_next(1),
        Ok(Transmit::Session(Published::ReceiveIntent(1)))
    );
    let ops = writes(&peer.log());
    assert_eq!(
        ops,
        vec![
            Op::Write(0x3ff8, vec![5, 0, 1, 0, 1, 0, 0, 0]),
            Op::Write(0, vec![0, 16, 0, 0, 1, 0, 0, 0]),
            Op::Head(8),
            Op::Notify
        ]
    );
    let log = peer.log();
    let head = log.iter().position(|op| matches!(op, Op::Head(_))).unwrap();
    let write = log
        .iter()
        .rposition(|op| matches!(op, Op::Write(..)))
        .unwrap();
    assert!(log[write + 1..head].contains(&Op::Barrier(Barrier::Release)));
    let notify = log.iter().position(|op| *op == Op::Notify).unwrap();
    assert!(log[head + 1..notify].contains(&Op::Barrier(Barrier::Complete)));
    assert_eq!(peer.0.borrow().words[4..], [0x1234, 2, 3, 4]);
}

#[test]
fn rx_wrap_copies_request_before_advancing_only_ap_tail() {
    let (peer, mut a) = adapter(0, 0x3ff8);
    let id = advertised(&mut a, &peer);
    peer.push(&query(id));
    peer.clear();
    let handle = receive_handle(&mut a);
    assert_eq!(
        a.request(1, &handle).unwrap().1,
        Request::Memory(MemoryRequest::QuerySize {
            client: 1,
            processor: None
        })
    );
    assert_eq!(writes(&peer.log()), vec![Op::Tail(56)]);
    assert_eq!(a.in_flight(), 1);
    let log = peer.log();
    let tail = log.iter().position(|op| matches!(op, Op::Tail(_))).unwrap();
    assert!(log[..tail].contains(&Op::Barrier(Barrier::Release)));
    assert!(log[..tail]
        .iter()
        .any(|op| matches!(op, Op::ReadRx(0x3ff8, 8))));
}

#[test]
fn partial_header_body_and_padding_leave_rx_cursor_and_core_unchanged() {
    for size in [8, 16, 24, 56] {
        let (peer, mut a) = adapter(0, 0x3ff8);
        let id = advertised(&mut a, &peer);
        let record = query(id);
        peer.push(&record[..size]);
        peer.clear();
        let result = a.poll_receive(1).unwrap();
        assert_eq!(
            result,
            Receive::Pending {
                available: size,
                needed: if size == 8 { 16 } else { 64 }
            }
        );
        assert_eq!(a.in_flight(), 0);
        assert!(writes(&peer.log()).is_empty());
        peer.push(&record[size..]);
        receive_handle(&mut a);
    }
}

#[cfg(feature = "sunfish-grant-rx-open")]
#[test]
fn grant_receive_turn_tracks_only_the_committed_positive_52_byte_ack() {
    for size in [0, 52, 53] {
        let (peer, mut adapter) = adapter(0, 0);
        peer.push(&control(7, 2, size));
        let granted = size != 0;
        assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::ReceiveIntentRequest { granted })));
        if granted {
            assert!(matches!(adapter.publish_next(1), Ok(Transmit::Session(Published::ReceiveIntent(_)))));
            assert_eq!(peer.take_tx()[0], 5);
        }
        assert_eq!(adapter.grant_receive_trace().ack_count, 0);
        assert_eq!(adapter.publish_next(1), Ok(Transmit::Session(Published::IntentRequestAck { granted })));
        assert_eq!(peer.take_tx()[0], 8);
        assert_eq!(adapter.grant_receive_trace().ack_count, u32::from(size == 52));
        if size == 52 {
            assert!(adapter.take_grant_receive_turn());
            assert!(!adapter.take_grant_receive_turn());
        }
    }
}

#[cfg(feature = "sunfish-wrq-grant-turns")]
#[test]
fn separate_startup_grants_each_get_one_opportunity_after_its_own_ack() {
    let (peer, mut adapter) = adapter(0, 0);
    for count in 1..=2 {
        peer.push(&control(7, 2, 52));
        assert!(matches!(adapter.poll_receive(1), Ok(Receive::Session(Event::ReceiveIntentRequest { granted: true }))));
        assert!(matches!(adapter.publish_next(1), Ok(Transmit::Session(Published::ReceiveIntent(_)))));
        assert!(!adapter.take_grant_receive_turn());
        assert_eq!(peer.take_tx()[0], 5);
        assert_eq!(adapter.publish_next(1), Ok(Transmit::Session(Published::IntentRequestAck { granted: true })));
        assert_eq!(peer.take_tx()[0], 8);
        assert_eq!(adapter.grant_receive_trace().ack_count, count);
        assert!(adapter.take_grant_receive_turn());
        assert!(!adapter.take_grant_receive_turn());
    }
}

#[test]
fn one_poll_consumes_only_one_record_in_a_queue() {
    let (peer, mut a) = adapter(0, 0);
    peer.push(&control(7, 2, 52));
    peer.push(&control(7, 2, 67));
    assert_eq!(
        a.poll_receive(1),
        Ok(Receive::Session(Event::ReceiveIntentRequest {
            granted: true
        }))
    );
    assert_eq!(peer.0.borrow().words[2], 8);
    // Publish the backing intent and its ACK before admitting another request.
    a.publish_next(1).unwrap();
    a.publish_next(1).unwrap();
    assert_eq!(
        a.poll_receive(1),
        Ok(Receive::Session(Event::ReceiveIntentRequest {
            granted: true
        }))
    );
    assert_eq!(peer.0.borrow().words[2], 16);
}

#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[test]
fn outbound_intent_request_and_peer_ack_cross_fifo_without_releasing_reply() {
    let (peer, mut adapter) = adapter(0, 0);
    let intent = advertised(&mut adapter, &peer);
    peer.push(&query(intent));
    let request = receive_handle(&mut adapter);
    adapter.queue_reply(1, &request, Reply::Failure { error: 3 }).unwrap();
    assert!(matches!(adapter.publish_next(1), Ok(Transmit::Session(Published::RemoteIntentRequest { size: 48 }))));
    assert!(matches!(adapter.publish_next(1), Ok(Transmit::Session(Published::ReceiveDone(_)))));
    peer.take_tx();
    peer.push(&control(8, 2, 1));
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RemoteIntentRequestAck { granted: true })));
    assert_eq!(adapter.in_flight(), 1);
    assert_eq!(adapter.publish_next(1), Ok(Transmit::AwaitingIntent));
    assert_eq!(adapter.outbound_intent_diagnostic(), (2, 48));
    adapter.suspend();
    assert_eq!(adapter.outbound_intent_diagnostic(), (2, 48), "quarantine must not erase the software status");
}

#[test]
fn read_notification_consumes_before_notifying_and_never_touches_tx() {
    let (peer, mut a) = adapter(0, 0x3ff8);
    peer.push(&control(13, 0, 0));
    assert_eq!(a.poll_receive(1), Ok(Receive::ReadNotification));
    assert_eq!(writes(&peer.log()), vec![Op::Tail(0), Op::Notify]);
}

#[test]
fn tx_reserves_sixteen_bytes_and_requests_space_once() {
    let peer = Fake::new(0, 0);
    peer.0.borrow_mut().words[1] = (FIFO_BYTES - 24) as u32;
    let mut a = Adapter::attach(peer.clone(), session(), 1).unwrap();
    a.offer_receive(1).unwrap();
    peer.clear();
    assert_eq!(
        a.publish_next(1),
        Ok(Transmit::Blocked {
            read_notification_sent: true
        })
    );
    assert_eq!(
        writes(&peer.log()),
        vec![
            Op::Write(FIFO_BYTES - 24, vec![13, 0, 0, 0, 0, 0, 0, 0]),
            Op::Head((FIFO_BYTES - 16) as u32),
            Op::Notify
        ]
    );
    peer.clear();
    assert_eq!(
        a.publish_next(1),
        Ok(Transmit::Blocked {
            read_notification_sent: false
        })
    );
    assert!(writes(&peer.log()).is_empty());
    peer.take_tx();
    peer.clear();
    assert_eq!(
        a.publish_next(1),
        Ok(Transmit::Session(Published::ReceiveIntent(1)))
    );
    assert_eq!(peer.take_tx().len(), 16);
}

#[test]
fn exact_capacity_boundary_accepts_record_and_preserves_reserve() {
    for free in [16, 24, 32] {
        let peer = Fake::new(0, 0);
        peer.0.borrow_mut().words[1] = (FIFO_BYTES - free) as u32;
        let mut a = Adapter::attach(peer.clone(), session(), 1).unwrap();
        a.offer_receive(1).unwrap();
        let result = a.publish_next(1).unwrap();
        assert_eq!(matches!(result, Transmit::Session(_)), free == 32);
        let m = peer.0.borrow();
        let remaining = (m.words[0] as usize + FIFO_BYTES - m.words[1] as usize) % FIFO_BYTES;
        assert!(remaining >= 8);
    }
}

#[test]
fn malformed_or_unsupported_records_halt_without_consumption() {
    for record in [
        control(0, 1, 0),
        control(13, 2, 0),
        control(5, 2, u32::MAX),
        {
            let mut v = control(9, 2, 1);
            v.extend(u32::MAX.to_le_bytes());
            v.extend(0u32.to_le_bytes());
            v
        },
    ] {
        let (peer, mut a) = adapter(0, 0);
        peer.push(&record);
        peer.clear();
        assert!(a.poll_receive(1).is_err());
        assert!(writes(&peer.log()).is_empty());
        assert_eq!(a.publish_next(1), Err(Error::Quarantined));
    }
}

#[test]
fn core_rejection_after_copy_holds_fifo_tail_and_stops_queued_replies() {
    let (peer, mut a) = adapter(0, 0);
    let id = advertised(&mut a, &peer);
    let mut bytes = query(id);
    bytes[48] = 4; // QMI indication cannot be a request.
    peer.push(&bytes);
    peer.clear();
    assert!(matches!(a.poll_receive(1), Err(Error::Session(_))));
    assert_eq!(a.in_flight(), 0);
    assert!(writes(&peer.log()).is_empty());
    assert_eq!(a.publish_next(1), Err(Error::Quarantined));
}

#[test]
fn changed_unconsumed_record_is_detected_before_core_or_cursor_write() {
    let (peer, mut a) = adapter(0, 0);
    peer.push(&control(7, 2, 52));
    peer.0.borrow_mut().mutate_rx = true;
    assert_eq!(a.poll_receive(1), Err(Error::ChangedRecord));
    assert!(writes(&peer.log()).is_empty());
    assert_eq!(peer.0.borrow().words[2], 0);
}

#[test]
fn invalid_descriptor_cursors_are_rejected_before_any_fifo_access() {
    for index in 0..4 {
        for value in [1, 7, 0x4000, u32::MAX] {
            let peer = Fake::new(0, 0);
            peer.0.borrow_mut().words[index] = value;
            assert_eq!(
                Adapter::attach(peer.clone(), session(), 1).err(),
                Some(Error::CursorBounds)
            );
            assert!(!peer
                .log()
                .iter()
                .any(|op| matches!(op, Op::ReadRx(..) | Op::ReadTx(..) | Op::Write(..))));
        }
    }
}

#[test]
fn external_ap_cursor_or_reserved_word_changes_stop_before_payload_access() {
    for index in [1, 2, 4, 7] {
        let (peer, mut a) = adapter(0, 0);
        peer.0.borrow_mut().words[index] += 8;
        let expected = if index < 4 {
            Error::OwnedCursorChanged
        } else {
            Error::ReservedChanged
        };
        assert_eq!(a.poll_receive(1), Err(expected));
        assert!(!peer.log().iter().any(|op| matches!(op, Op::ReadRx(..))));
        assert_eq!(a.offer_receive(1), Err(Error::Quarantined));
    }
}

#[test]
fn peer_cannot_consume_unpublished_bytes_or_overwrite_live_rx_bytes() {
    let (peer, mut a) = adapter(0, 0);
    peer.0.borrow_mut().words[0] = 8;
    assert_eq!(a.poll_receive(1), Err(Error::PeerOverrun));
    let (peer, mut a) = adapter(0, 0);
    peer.push(&[0; FIFO_BYTES - 8]);
    // Establish a legal almost-full head through a read-only gate.
    a.offer_receive(1).unwrap();
    peer.0.borrow_mut().words[3] = 0;
    assert_eq!(a.publish_next(1), Err(Error::PeerOverrun));
}

#[test]
fn stale_generation_is_rejected_without_even_observing_hardware() {
    let (peer, mut a) = adapter(0, 0);
    peer.clear();
    assert_eq!(a.poll_receive(2), Err(Error::StaleGeneration));
    assert_eq!(a.publish_next(2), Err(Error::StaleGeneration));
    assert_eq!(a.offer_receive(2), Err(Error::StaleGeneration));
    assert!(peer.log().is_empty());
}

#[test]
fn ready_fatal_generation_and_identity_are_checked_before_mutation() {
    for failure in 0..4 {
        let (peer, mut a) = adapter(0, 0);
        a.offer_receive(1).unwrap();
        let expected = {
            let mut m = peer.0.borrow_mut();
            match failure {
                0 => {
                    m.observation.ready = false;
                    Error::NotReady
                }
                1 => {
                    m.observation.fatal = true;
                    Error::Fatal
                }
                2 => {
                    m.observation.generation = 2;
                    Error::StaleGeneration
                }
                _ => {
                    m.observation.identity = 100;
                    Error::IdentityChanged
                }
            }
        };
        assert_eq!(a.publish_next(1), Err(expected));
        assert!(writes(&peer.log()).is_empty());
        assert_eq!(a.poll_receive(1), Err(Error::Quarantined));
    }
}

#[test]
fn ready_loss_after_tx_staging_never_publishes_head() {
    let (peer, mut a) = adapter(0, 0);
    a.offer_receive(1).unwrap();
    peer.0.borrow_mut().revoke_after_write = true;
    assert_eq!(a.publish_next(1), Err(Error::NotReady));
    assert_eq!(peer.0.borrow().words[1], 0);
    assert!(!writes(&peer.log())
        .iter()
        .any(|op| matches!(op, Op::Head(_) | Op::Notify)));
    assert_eq!(a.publish_next(1), Err(Error::Quarantined));
}

#[test]
fn tx_readback_mismatch_never_publishes_head_or_notifies() {
    let (peer, mut a) = adapter(0, 0);
    a.offer_receive(1).unwrap();
    peer.0.borrow_mut().corrupt_readback = true;
    assert_eq!(a.publish_next(1), Err(Error::Readback));
    assert_eq!(peer.0.borrow().words[1], 0);
    assert!(!peer
        .log()
        .iter()
        .any(|op| matches!(op, Op::Head(_) | Op::Notify)));
}

#[test]
fn every_tx_io_failure_stops_without_retrying_uncertain_publication() {
    for point in [
        "observe",
        "descriptor",
        "write_tx",
        "barrier",
        "read_tx",
        "head",
        "notify",
    ] {
        let (peer, mut a) = adapter(0, 0);
        a.offer_receive(1).unwrap();
        peer.0.borrow_mut().fail = Some(point);
        assert_eq!(a.publish_next(1), Err(Error::Access), "{point}");
        let log = peer.log();
        assert_eq!(a.publish_next(1), Err(Error::Quarantined));
        assert_eq!(peer.log(), log);
    }
}

#[test]
fn rx_tail_store_failure_after_effect_hides_request_and_halts_transmission() {
    let (peer, mut a) = adapter(0, 0);
    let id = advertised(&mut a, &peer);
    peer.push(&query(id));
    peer.0.borrow_mut().fail_after_store = true;
    assert_eq!(a.poll_receive(1), Err(Error::Access));
    assert_eq!(peer.0.borrow().words[2], 64);
    assert_eq!(a.in_flight(), 1); // Retained, never rerouted or auto-replied.
    assert_eq!(a.publish_next(1), Err(Error::Quarantined));
}

#[test]
fn tx_head_store_failure_after_effect_is_not_treated_as_would_block() {
    let (peer, mut a) = adapter(0, 0);
    a.offer_receive(1).unwrap();
    peer.0.borrow_mut().fail_after_store = true;
    assert_eq!(a.publish_next(1), Err(Error::Access));
    assert_eq!(peer.0.borrow().words[1], 16);
    assert_eq!(a.publish_next(1), Err(Error::Quarantined));
}

#[test]
fn reset_requires_new_trusted_generation_same_mapping_and_empty_rings() {
    let (peer, mut a) = adapter(0, 0);
    a.offer_receive(1).unwrap();
    a.publish_next(1).unwrap();
    assert_eq!(
        a.reset_after_transport_reset(1),
        Err(Error::StaleGeneration)
    );
    assert_eq!(
        a.reset_after_transport_reset(2),
        Err(Error::StaleGeneration)
    );
    peer.0.borrow_mut().observation.generation = 2;
    assert_eq!(a.reset_after_transport_reset(2), Err(Error::ResetNotEmpty));
    peer.take_tx();
    assert_eq!(a.reset_after_transport_reset(2), Ok(()));
    peer.clear();
    assert_eq!(a.publish_next(2), Ok(Transmit::Idle));
    assert!(writes(&peer.log()).is_empty());
    assert_eq!(a.offer_receive(1), Err(Error::StaleGeneration));
    a.register(2, Service::HeapMemory, 0x4001).unwrap();
    assert!(a.offer_receive(2).unwrap() > 1);
}

#[test]
fn reset_refuses_relocated_smem_identity_without_writing_old_mapping() {
    let (peer, mut a) = adapter(0, 0);
    peer.0.borrow_mut().observation = Observation {
        generation: 2,
        identity: 100,
        ready: true,
        fatal: false,
    };
    assert_eq!(
        a.reset_after_transport_reset(2),
        Err(Error::IdentityChanged)
    );
    assert!(writes(&peer.log()).is_empty());
}

#[test]
fn attaching_a_session_with_wire_state_is_rejected_before_io() {
    let peer = Fake::new(0, 0);
    let mut s = session();
    s.offer_receive(1).unwrap();
    assert_eq!(
        Adapter::attach(peer.clone(), s, 1).err(),
        Some(Error::SessionNotFresh)
    );
    assert!(peer.log().is_empty());
}

#[test]
fn complete_query_reply_exchange_crosses_native_rings_and_both_completions() {
    let (peer, mut a) = adapter(0x3ff8, 0x3ff8);
    let id = advertised(&mut a, &peer);
    peer.push(&query(id));
    let handle = receive_handle(&mut a);
    a.queue_reply(
        1,
        &handle,
        Reply::Success {
            optional_tlvs: &[0x10, 4, 0, 0, 0, 0x50, 0],
        },
    )
    .unwrap();
    let mut remote = control(5, 2, 1);
    remote.extend(128u32.to_le_bytes());
    remote.extend(7u32.to_le_bytes());
    peer.push(&remote);
    a.poll_receive(1).unwrap();
    a.publish_next(1).unwrap(); // RX_DONE for the request.
    a.publish_next(1).unwrap(); // Correlated response.
    let out = peer.take_tx();
    assert_eq!(&out[..8], &[6, 0, 1, 0, 1, 0, 0, 0]);
    let size = u32::from_le_bytes(out[44..48].try_into().unwrap()) as usize;
    let decoded = qcom_qmi::Message::decode(&out[56..56 + size]).unwrap();
    assert_eq!((decoded.transaction, decoded.message_id), (17, 0x24));
    assert_eq!(decoded.u32(0x10).unwrap(), 5 * 1024 * 1024);
    peer.push(&control(6, 2, 7));
    a.poll_receive(1).unwrap();
    assert_eq!(a.in_flight(), 1);
    let credit_intent = advertised(&mut a, &peer);
    let mut body = vec![0; 20];
    body[0] = 7;
    body[8] = 9;
    peer.push(&data(credit_intent, &qrtr(7, &body)));
    assert!(matches!(
        a.poll_receive(1),
        Ok(Receive::Session(Event::ReplyQrtrResume(_)))
    ));
    assert_eq!(a.in_flight(), 0);
    assert_eq!(
        a.publish_next(1),
        Ok(Transmit::Session(Published::ReceiveDone(credit_intent)))
    );
    assert_eq!(a.publish_next(1), Ok(Transmit::Idle));
}

#[test]
fn all_aligned_cursor_positions_support_wrapped_fixed_records() {
    for position in (0..FIFO_BYTES).step_by(8) {
        let (peer, mut a) = adapter(position as u32, position as u32);
        peer.push(&control(13, 0, 0));
        assert_eq!(a.poll_receive(1), Ok(Receive::ReadNotification));
        a.offer_receive(1).unwrap();
        a.publish_next(1).unwrap();
        assert_eq!(peer.take_tx().len(), 16);
    }
}

#[repr(align(8))]
struct RingMemory([u8; FIFO_BYTES]);
struct FixedGate(Observation);
impl Gate for FixedGate {
    fn observe(&mut self) -> Result<Observation, Fault> {
        Ok(self.0)
    }
}

#[test]
fn actual_volatile_backend_works_on_guarded_private_host_memory() {
    let mut descriptor = [0u32; 10];
    descriptor[0] = 0xabcd;
    descriptor[9] = 0xdcba;
    let mut tx = Box::new(RingMemory([0xa5; FIFO_BYTES]));
    let mut rx = Box::new(RingMemory([0x5a; FIFO_BYTES]));
    rx.0[..8].copy_from_slice(&[13, 0, 0, 0, 0, 0, 0, 0]);
    descriptor[4] = 8; // Peer published a READ_NOTIF before attachment.
    let mut doorbell = [0xaaaa, 0, 0xbbbb];
    let mappings = Mappings {
        descriptor: descriptor[1..].as_mut_ptr(),
        tx: tx.0.as_mut_ptr(),
        rx: rx.0.as_ptr(),
        doorbell: doorbell[1..].as_mut_ptr(),
    };
    // SAFETY: test-owned, aligned, disjoint, retained buffers simulate the maps;
    // there is no peer thread, device, aliasing access or concurrent writer.
    let io = unsafe {
        NativeIo::new(
            mappings,
            FixedGate(Observation {
                generation: 1,
                identity: 1,
                ready: true,
                fatal: false,
            }),
        )
    }
    .unwrap();
    let mut a = Adapter::attach(io, session(), 1).unwrap();
    assert_eq!(a.poll_receive(1), Ok(Receive::ReadNotification));
    a.offer_receive(1).unwrap();
    a.publish_next(1).unwrap();
    drop(a);
    assert_eq!(
        &tx.0[..16],
        &[5, 0, 1, 0, 1, 0, 0, 0, 0, 16, 0, 0, 1, 0, 0, 0]
    );
    assert!(tx.0[16..].iter().all(|byte| *byte == 0xa5));
    assert_eq!(&rx.0[..8], &[13, 0, 0, 0, 0, 0, 0, 0]);
    assert!(rx.0[8..].iter().all(|byte| *byte == 0x5a));
    assert_eq!(descriptor, [0xabcd, 0, 16, 8, 8, 0, 0, 0, 0, 0xdcba]);
    assert_eq!(doorbell, [0xaaaa, 1 << 12, 0xbbbb]);
}

#[test]
fn native_mapping_validation_rejects_null_unaligned_overflow_and_overlap() {
    let base = Mappings {
        descriptor: 0x1000 as *mut u32,
        tx: 0x2000 as *mut u8,
        rx: 0x10000 as *const u8,
        doorbell: 0x20000 as *mut u32,
    };
    let observation = Observation {
        generation: 1,
        identity: 1,
        ready: true,
        fatal: false,
    };
    for bad in [
        Mappings {
            descriptor: std::ptr::null_mut(),
            ..base
        },
        Mappings {
            tx: 0x2001 as *mut u8,
            ..base
        },
        Mappings {
            rx: 0x2008 as *const u8,
            ..base
        },
        Mappings {
            tx: (usize::MAX - 7) as *mut u8,
            ..base
        },
        Mappings {
            doorbell: 0x1000 as *mut u32,
            ..base
        },
    ] {
        // SAFETY: constructor validation rejects these before dereferencing.
        assert!(unsafe { NativeIo::new(bad, FixedGate(observation)) }.is_err());
    }
}

#[test]
fn largest_supported_record_wraps_without_truncation_or_shared_borrows() {
    let (peer, mut a) = adapter(0, 0x3ff8);
    let id = advertised(&mut a, &peer);
    let mut qmi = vec![0, 17, 0, 0x24, 0, 0xd9, 0x0f, 1, 4, 0, 1, 0, 0, 0];
    qmi.extend([0x11, 0xcf, 0x0f]);
    qmi.extend(vec![0; 4047]);
    let payload = qrtr(1, &qmi);
    assert_eq!(payload.len(), 4096);
    let record = data(id, &payload);
    assert_eq!(record.len(), qcom_glink_fifo::MAX_RX_RECORD);
    peer.push(&record);
    let handle = receive_handle(&mut a);
    // Peer can overwrite freed FIFO bytes; the request remains in private RAM.
    peer.0.borrow_mut().rx.fill(0xff);
    assert_eq!(
        a.request(1, &handle).unwrap().1,
        Request::Memory(MemoryRequest::QuerySize {
            client: 1,
            processor: None
        })
    );
    assert_eq!(peer.0.borrow().words[2], 4104);
}

#[test]
fn legal_peer_append_during_copy_does_not_require_freezing_peer_head() {
    let (peer, mut a) = adapter(0, 0x3fc0);
    let id = advertised(&mut a, &peer);
    peer.push(&query(id));
    peer.0.borrow_mut().append_on_rx_read = true;
    receive_handle(&mut a);
    assert_eq!(peer.0.borrow().words[2], 0);
    assert_eq!(a.poll_receive(1), Ok(Receive::ReadNotification));
    assert_eq!(peer.0.borrow().words[2], 8);
}

#[test]
fn receive_io_failures_never_return_an_event_or_retry_the_record() {
    for point in ["read_rx", "tail", "notify"] {
        let (peer, mut a) = adapter(0, 0);
        peer.push(&control(13, 0, 0));
        peer.0.borrow_mut().fail = Some(point);
        assert_eq!(a.poll_receive(1), Err(Error::Access));
        let log = peer.log();
        assert_eq!(a.poll_receive(1), Err(Error::Quarantined));
        assert_eq!(peer.log(), log);
    }
}

#[test]
fn unwinding_io_halts_both_transmit_and_receive_paths() {
    for transmit in [false, true] {
        let (peer, mut a) = adapter(0, 0);
        if transmit {
            a.offer_receive(1).unwrap();
        } else {
            peer.push(&control(13, 0, 0));
        }
        peer.0.borrow_mut().panic_on_notify = true;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if transmit {
                let _ = a.publish_next(1);
            } else {
                let _ = a.poll_receive(1);
            }
        }));
        assert!(result.is_err());
        assert_eq!(a.publish_next(1), Err(Error::Quarantined));
        assert_eq!(a.poll_receive(1), Err(Error::Quarantined));
    }
}

#[test]
fn native_backend_rejects_out_of_range_accesses_before_touching_memory() {
    let mut descriptor = [0u32; 8];
    let mut tx = Box::new(RingMemory([0xa5; FIFO_BYTES]));
    let rx = Box::new(RingMemory([0x5a; FIFO_BYTES]));
    let mut doorbell = 0u32;
    let observation = Observation {
        generation: 1,
        identity: 1,
        ready: true,
        fatal: false,
    };
    let mappings = Mappings {
        descriptor: descriptor.as_mut_ptr(),
        tx: tx.0.as_mut_ptr(),
        rx: rx.0.as_ptr(),
        doorbell: &mut doorbell,
    };
    // SAFETY: exclusively retained, aligned private memory simulates the maps.
    let mut io = unsafe { NativeIo::new(mappings, FixedGate(observation)) }.unwrap();
    let mut byte = [0];
    assert_eq!(io.descriptor(8), Err(Fault));
    for offset in [FIFO_BYTES, usize::MAX] {
        assert_eq!(io.read_rx(offset, &mut byte), Err(Fault));
        assert_eq!(io.read_tx(offset, &mut byte), Err(Fault));
        assert_eq!(io.write_tx(offset, &[1]), Err(Fault));
    }
    for value in [1, 7, 0x4000, u32::MAX] {
        assert_eq!(io.store_tx_head(value), Err(Fault));
        assert_eq!(io.store_rx_tail(value), Err(Fault));
    }
    drop(io);
    assert_eq!(descriptor, [0; 8]);
    assert!(tx.0.iter().all(|byte| *byte == 0xa5));
    assert_eq!(doorbell, 0);
}

#[cfg(feature = "sunfish-glink-combined-cursor-sample")]
#[test]
fn cursor_snapshot_failure_holds_cursors_and_quarantines_without_publication() {
    for transmit in [false, true] {
        let (peer, mut adapter) = adapter(0, 0);
        adapter.offer_receive(1).unwrap();
        peer.clear();
        peer.0.borrow_mut().fail = Some("snapshot");
        if transmit { assert_eq!(adapter.publish_next(1), Err(Error::Access)); }
        else { assert_eq!(adapter.poll_receive(1), Err(Error::Access)); }
        assert!(peer.log().contains(&Op::Snapshot));
        assert!(writes(&peer.log()).is_empty());
        let before = peer.log();
        assert_eq!(adapter.poll_receive(1), Err(Error::Quarantined));
        assert_eq!(adapter.publish_next(1), Err(Error::Quarantined));
        assert_eq!(peer.log(), before);
        assert_eq!(&peer.0.borrow().words[..4], &[0, 0, 0, 0]);
    }
}

#[cfg(feature = "sunfish-glink-barrier-observe-pair")]
#[test]
fn barrier_pair_preserves_order_and_stops_on_each_failed_half() {
    for barrier in [Barrier::Acquire, Barrier::Release, Barrier::Complete] {
        for fail in [None, Some("barrier"), Some("observe")] {
            let mut io = Fake::new(0, 0);
            io.0.borrow_mut().fail = fail;
            let result = io.barrier_observe(barrier);
            assert_eq!(result.is_err(), fail.is_some());
            assert_eq!(io.log(), if fail == Some("barrier") {
                vec![Op::Barrier(barrier)]
            } else { vec![Op::Barrier(barrier), Op::Observe] });
        }
    }
}

#[cfg(feature = "sunfish-glink-barrier-observe-pair")]
#[test]
fn barrier_pair_still_validates_post_barrier_observation_before_any_write() {
    for mode in 0..4 {
        let peer = Fake::new(0, 0);
        let mut adapter = Adapter::attach(peer.clone(), session(), 1).unwrap();
        peer.clear();
        let mut observation = peer.0.borrow().observation;
        let expected = match mode {
            0 => { observation.generation += 1; Error::StaleGeneration },
            1 => { observation.identity += 1; Error::IdentityChanged },
            2 => { observation.ready = false; Error::NotReady },
            _ => { observation.fatal = true; Error::Fatal },
        };
        peer.0.borrow_mut().observation_after_barrier = Some(observation);
        assert_eq!(adapter.poll_receive(1), Err(expected));
        let after = peer.log();
        assert!(!after.iter().any(|op| matches!(op, Op::Write(..) | Op::Head(_) | Op::Tail(_) | Op::Notify)));
        assert!(adapter.publish_next(1).is_err());
        assert_eq!(peer.log(), after);
    }
}

#[cfg(feature = "sunfish-glink-barrier-observe-pair")]
#[test]
fn adjacent_barriers_preserve_cursor_and_doorbell_order() {
    let (peer, mut adapter) = adapter(0, 0);
    peer.push(&[13, 0, 0, 0, 0, 0, 0, 0]);
    peer.clear();
    assert_eq!(adapter.poll_receive(1), Ok(Receive::ReadNotification));
    let log = peer.log();
    let tail = log.iter().position(|op| matches!(op, Op::Tail(_))).unwrap();
    assert_eq!(&log[tail - 2..tail], &[Op::Barrier(Barrier::Release), Op::Observe]);
    let notify = log.iter().position(|op| *op == Op::Notify).unwrap();
    assert_eq!(&log[notify - 2..notify], &[Op::Barrier(Barrier::Complete), Op::Observe]);
    assert_eq!(&log[notify + 1..notify + 3], &[Op::Barrier(Barrier::Complete), Op::Observe]);
    assert_eq!(log.iter().filter(|op| **op == Op::Notify).count(), 1);
    assert_eq!(peer.0.borrow().words[2], 8);
}

#[cfg(feature = "sunfish-glink-barrier-observe-pair")]
#[test]
fn late_barrier_status_loss_stops_before_cursor_or_doorbell_and_stays_quarantined() {
    for (barrier, nth, tail_committed, notices) in [
        (Barrier::Release, 1, false, 0),
        (Barrier::Complete, 2, true, 0),
        (Barrier::Complete, 3, true, 1),
    ] {
        for mode in 0..4 {
            let (peer, mut adapter) = adapter(0, 0);
            peer.push(&[13, 0, 0, 0, 0, 0, 0, 0]);
            peer.clear();
            let mut changed = peer.0.borrow().observation;
            let expected = match mode {
                0 => { changed.generation += 1; Error::StaleGeneration },
                1 => { changed.identity += 1; Error::IdentityChanged },
                2 => { changed.ready = false; Error::NotReady },
                _ => { changed.fatal = true; Error::Fatal },
            };
            peer.0.borrow_mut().late_barrier_observation = Some((barrier, nth, changed));
            assert_eq!(adapter.poll_receive(1), Err(expected), "{barrier:?} {nth} {mode}");
            assert!(peer.0.borrow().late_barrier_observation.is_none());
            assert_eq!(peer.0.borrow().words[2], if tail_committed { 8 } else { 0 });
            let log = peer.log();
            assert_eq!(log.iter().filter(|op| **op == Op::Notify).count(), notices);
            assert_eq!(adapter.poll_receive(1), Err(Error::Quarantined));
            assert_eq!(adapter.publish_next(1), Err(Error::Quarantined));
            assert_eq!(peer.log(), log, "a failed pair must not be retried");
        }
    }
}

#[cfg(feature = "sunfish-grant-buffer-reads")]
#[test]
fn grant_read_trace_requires_the_last_byte_of_the_wrapped_offer() {
    use core::sync::atomic::{AtomicU64, Ordering};
    static NOW: AtomicU64 = AtomicU64::new(0);
    let (peer, mut adapter) = adapter(0x3ff8, 0);
    adapter.set_registry_ack_clock(|| NOW.fetch_add(1, Ordering::Relaxed));
    adapter.track_first_wrq();
    let id = adapter.offer_receive(1).unwrap();
    assert_eq!(adapter.publish_next(1), Ok(Transmit::Session(Published::ReceiveIntent(id))));

    // Only the first eight bytes of the advertisement have crossed the peer tail.
    peer.0.borrow_mut().words[0] = 0;
    adapter.revalidate(1).unwrap();
    peer.push(&control(7, 2, 108));
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::ReceiveIntentRequest { granted: true })));
    adapter.revalidate(1).unwrap();
    let capture = adapter.grant_buffer_reads();
    let offer = capture.fits.iter().flatten().next().unwrap();
    assert_eq!((offer.id, offer.size, offer.command), (id, 4096, 5));
    assert_ne!(capture.backing_id, Some(id));
    assert!(capture.absent.is_some() && capture.visible.is_some());
    assert!(offer.first_read.is_none());
    assert_eq!(offer.order(capture.absent, capture.visible), 2);

    peer.0.borrow_mut().words[0] = 8;
    adapter.revalidate(1).unwrap();
    let capture = adapter.grant_buffer_reads();
    let offer = capture.fits.iter().flatten().next().unwrap();
    assert!(offer.first_read.unwrap() > capture.visible.unwrap());
    assert_eq!(offer.order(capture.absent, capture.visible), 2);
}

#[cfg(feature="sunfish-registry-post-reply-trace")]
#[test]
fn registry_post_reply_fifo_matches_exact_receipt_across_wrap_and_preserves_checked_io() {
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};
    static NOW:AtomicU64=AtomicU64::new(100);
    for clocked in [false,true] {
        let mut s=session();
        s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
        s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        let peer=Fake::new(0x3ff8,0x3ff8);
        let mut a=Adapter::attach(peer.clone(),s,1).unwrap();
        if clocked {a.set_registry_ack_clock(||NOW.fetch_add(1,Relaxed));}
        let mut remote=control(5,2,2);
        for id in [7u32,8] {remote.extend(4096u32.to_le_bytes());remote.extend(id.to_le_bytes());}
        peer.push(&remote);a.poll_receive(1).unwrap();
        let id=advertised(&mut a,&peer);
        let mut packet=qrtr(1,&[0,3,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);
        packet[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());
        peer.push(&data(id,&packet));
        let Ok(Receive::Session(Event::Request(request)))=a.poll_receive(1) else{panic!("registry request")};
        a.queue_reply(1,&request,Reply::Failure{error:3}).unwrap();
        let mut intent=None;
        for _ in 0..16 {
            let e=a.publish_next(1).unwrap();
            if let Transmit::Session(Published::Data{remote_intent,last_fragment:true})=e {intent=Some(remote_intent);break;}
            peer.take_tx();
        }
        let intent=intent.expect("exact reply published");
        let trace=a.registry_reply_fifo().expect("publication arms even without a clock");
        assert!(!trace.finished); assert_eq!(trace.completion,None);
        peer.take_tx();a.revalidate(1).unwrap();
        assert_eq!(a.registry_reply_fifo().unwrap().peer_tail_at.is_some(),clocked);
        // A read notification ahead of the exact receipt stays in FIFO order.
        peer.push(&control(13,0,0));peer.push(&control(14,2,intent));
        a.revalidate(1).unwrap();
        assert_eq!(a.poll_receive(1),Ok(Receive::ReadNotification));
        assert!(!a.registry_reply_fifo().unwrap().finished);
        assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(request))));
        let done=a.registry_reply_fifo().unwrap();assert!(done.finished);
        let bounds=done.completion.unwrap();assert_eq!(bounds.bytes_ahead,Some(8));
        assert_eq!(bounds.first_visible.is_some(),clocked);
        let log=peer.log();assert_eq!(a.registry_reply_fifo(),Some(done));assert_eq!(peer.log(),log,"getter adds no hardware reads");
        a.revalidate(1).unwrap();assert_eq!(a.registry_reply_fifo(),Some(done));
    }
}


#[cfg(feature="sunfish-registry-prearm-io-trace")]
#[test]
fn registry_prearm_adapter_observes_queue_first_sample_own_return_and_fault_without_extra_getter_io(){
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};
    static NOW:AtomicU64=AtomicU64::new(100);
    for clocked in [false,true] {for fail_sample in [false,true] {
        let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
        s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        let peer=Fake::new(0x3ff8,0x3ff8);let mut a=Adapter::attach(peer.clone(),s,1).unwrap();
        if clocked{a.set_registry_ack_clock(||NOW.fetch_add(1,Relaxed));}
        let mut remote=control(5,2,2);for id in [7u32,8]{remote.extend(4096u32.to_le_bytes());remote.extend(id.to_le_bytes());}
        peer.push(&remote);a.poll_receive(1).unwrap();
        for txn in [3u16,4]{
            let id=advertised(&mut a,&peer);let mut payload=vec![0];payload.extend(txn.to_le_bytes());payload.extend([0x21,0,6,0,1,3,0,b'f',b'o',b'o']);
            let mut packet=qrtr(1,&payload);packet[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());peer.push(&data(id,&packet));
            let Ok(Receive::Session(Event::Request(request)))=a.poll_receive(1)else{panic!("registry request")};
            a.queue_reply(1,&request,Reply::Failure{error:3}).unwrap();
            if txn==3{
                assert_eq!(a.registry_prearm_io(),None);
                for _ in 0..16{let event=a.publish_next(1).unwrap();peer.take_tx();if matches!(event,Transmit::Session(Published::Data{last_fragment:true,..})){break;}}
            }else{
                let queued=a.registry_prearm_io().expect("only exact transaction4 queues observation");assert!(queued.queued.is_some());assert_eq!(queued.queued_at.is_some(),clocked);assert_eq!(queued.first_enter,None);
                if fail_sample{peer.0.borrow_mut().fail=Some("observe");assert!(a.publish_next(1).is_err());}
                else{assert_eq!(a.publish_next(1),Ok(Transmit::Session(Published::ReceiveDone(id))));}
                let t=a.registry_prearm_io().unwrap();assert!(t.first_enter.is_some());assert_eq!(t.first_enter_at.is_some(),clocked);assert_eq!(t.first_end.is_some(),!fail_sample);assert_eq!(t.arm,None);
                let p=a.registry_window_trace().target.unwrap().prearm;assert_eq!(p.passes,u32::from(!fail_sample));assert_eq!(p.own_done_at.is_some(),clocked&&!fail_sample);
                if !fail_sample{assert_eq!(p.first_choice.unwrap().command,6);assert_eq!(p.first_choice.unwrap().parameter,id);assert_eq!(p.first_choice.unwrap().result,1);assert!(t.total(false).is_some());}
                let log=peer.log();assert_eq!(a.registry_prearm_io(),Some(t));assert_eq!(peer.log(),log,"diagnostic getter performs no I/O");
            }
        }
    }}
}

#[cfg(feature="sunfish-registry-tx3-wire-headers")]
#[test]
fn registry_wire_adapter_scopes_committed_headers_to_exact_predecessor_and_reset(){
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static NOW:AtomicU64=AtomicU64::new(100);
    for clocked in [false,true]{
        let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        let peer=Fake::new(0x3ff8,0x3ff8);let mut a=Adapter::attach(peer.clone(),s,1).unwrap();if clocked{a.set_registry_ack_clock(||NOW.fetch_add(1,Relaxed));}
        let mut remote=control(5,2,2);for id in [7u32,8]{remote.extend(4096u32.to_le_bytes());remote.extend(id.to_le_bytes());}peer.push(&remote);a.poll_receive(1).unwrap();
        let mut requests=std::vec::Vec::new();let mut intents=std::vec::Vec::new();
        for txn in [3u16,4]{let id=advertised(&mut a,&peer);let mut b=vec![0];b.extend(txn.to_le_bytes());b.extend([0x21,0,6,0,1,3,0,b'f',b'o',b'o']);let mut packet=qrtr(1,&b);packet[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());peer.push(&data(id,&packet));let Ok(Receive::Session(Event::Request(request)))=a.poll_receive(1)else{panic!("registry request")};
            a.queue_reply(1,&request,Reply::Failure{error:3}).unwrap();if txn==3{assert!(a.registry_wire_headers().is_none());}
            let mut published=None;for _ in 0..16{let event=a.publish_next(1).unwrap();peer.take_tx();if let Transmit::Session(Published::Data{remote_intent,last_fragment:true})=event{published=Some(remote_intent);break;}}
            intents.push(published.expect("reply published"));requests.push(request);
            if txn==3{let t=a.registry_wire_headers().unwrap();let h=t.headers[0].unwrap();assert_eq!(h.qmi,Some((2,3,0x21)));assert!(h.tx);assert_eq!(h.at.is_some(),clocked);assert!(!t.finished);}
        }
        peer.push(&control(14,2,intents[1]));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(requests[1]))));assert!(!a.registry_wire_headers().unwrap().finished,"transaction4 receipt cannot freeze3");
        peer.push(&control(13,0,0));peer.push(&control(14,2,intents[0]));assert_eq!(a.poll_receive(1),Ok(Receive::ReadNotification));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(requests[0]))));
        let t=a.registry_wire_headers().unwrap();assert!(t.finished);let h=t.completion.unwrap();assert_eq!((h.command,h.parameter),(14,intents[0]));assert_eq!(h.at.is_some(),clocked);assert!(t.headers.iter().flatten().any(|h|!h.tx&&h.command==13));
        let before=peer.log();assert_eq!(a.registry_wire_headers().unwrap().completion,t.completion);assert_eq!(peer.log(),before,"getter adds no device I/O");peer.push(&control(13,0,0));a.poll_receive(1).unwrap();assert_eq!(a.registry_wire_headers().unwrap().completion,t.completion);assert_eq!(a.registry_wire_headers().unwrap().omitted,t.omitted);
        peer.0.borrow_mut().observation.generation=2;a.reset_after_transport_reset(2).unwrap();assert!(a.registry_wire_headers().is_none());
    }
}

#[cfg(feature = "sunfish-glink-announcement-reuse")]
#[path = "sunfish_announcement_fixture.rs"]
mod announcement_fixture;
