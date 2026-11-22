mod common;
use common::*;
use modem_storage_host::{
    qcom_glink_fifo::{self as fifo, Adapter, Barrier},
    qcom_glink_qmi::{Config, Session},
    qcom_modem_handlers as h,
    qcom_modem_owner::{self as owner_core, Interrupts, Owner},
    qcom_modem_services::Service,
    qcom_qmi::{Kind, Message},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Ring(Rc<RefCell<RingState>>);
struct RingState {
    descriptors: [u32; 8],
    rx: [u8; fifo::FIFO_BYTES],
    tx: [u8; fifo::FIFO_BYTES],
    notifications: usize,
}
impl Ring {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(RingState {
            descriptors: [0; 8],
            rx: [0; fifo::FIFO_BYTES],
            tx: [0; fifo::FIFO_BYTES],
            notifications: 0,
        })))
    }
    fn push_rx(&self, record: &[u8]) {
        assert_eq!(record.len() % 8, 0);
        let mut state = self.0.borrow_mut();
        let head = state.descriptors[3] as usize;
        let tail = state.descriptors[2] as usize;
        let free = fifo::FIFO_BYTES - ((head + fifo::FIFO_BYTES - tail) % fifo::FIFO_BYTES);
        assert!(record.len() < free.saturating_sub(fifo::FULL_RESERVE));
        for (index, byte) in record.iter().enumerate() {
            state.rx[(head + index) % fifo::FIFO_BYTES] = *byte;
        }
        state.descriptors[3] = ((head + record.len()) % fifo::FIFO_BYTES) as u32;
    }
}

// The wire cursor words are aligned and never wrap in this small test. This
// helper still reads modulo the ring so the parser matches production rings.
fn read_word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(std::array::from_fn(|i| bytes[(at + i) % bytes.len()]))
}

#[derive(Clone, Copy)]
struct OwnedMessage {
    kind: Kind,
    transaction: u16,
    message_id: u16,
    result: [u8; 4],
    has_result: bool,
}
impl From<Message<'_>> for OwnedMessage {
    fn from(message: Message<'_>) -> Self {
        let mut result = [0; 4];
        let has_result = message.required(2).is_ok_and(|value| {
            value.len() == 4 && {
                result.copy_from_slice(value);
                true
            }
        });
        Self {
            kind: message.kind,
            transaction: message.transaction,
            message_id: message.message_id,
            result,
            has_result,
        }
    }
}
impl Ring {
    fn transmitted_summaries(&self) -> Vec<OwnedMessage> {
        // Parse once more while retaining only the scalar fields we assert.
        let state = self.0.borrow();
        let mut at = state.descriptors[0] as usize;
        let end = state.descriptors[1] as usize;
        let mut messages = Vec::new();
        while at != end {
            let command = u16::from_le_bytes([state.tx[at], state.tx[(at + 1) % fifo::FIFO_BYTES]]);
            let parameter = read_word(&state.tx, (at + 4) % fifo::FIFO_BYTES);
            let size = match command {
                5 => 8 + parameter as usize * 8,
                6 | 7 | 13 | 14 => 8,
                9 | 12 => 16 + read_word(&state.tx, (at + 8) % fifo::FIFO_BYTES) as usize,
                _ => panic!("unexpected GLINK command {command}"),
            };
            if matches!(command, 9 | 12) {
                let chunk = read_word(&state.tx, (at + 8) % fifo::FIFO_BYTES) as usize;
                let remaining = read_word(&state.tx, (at + 12) % fifo::FIFO_BYTES) as usize;
                if remaining == 0 && chunk >= 32 {
                    let datagram: Vec<u8> = (0..chunk)
                        .map(|index| state.tx[(at + 16 + index) % fifo::FIFO_BYTES])
                        .collect();
                    if read_word(&datagram, 0) == 1
                        && read_word(&datagram, 4) == 1
                        && datagram.len() >= 32 + read_word(&datagram, 20) as usize
                    {
                        let body = read_word(&datagram, 20) as usize;
                        if let Ok(message) = Message::decode(&datagram[32..32 + body]) {
                            messages.push(OwnedMessage::from(message));
                        }
                    }
                }
            }
            at = (at + ((size + 7) & !7)) % fifo::FIFO_BYTES;
        }
        messages
    }
}

impl fifo::Io for Ring {
    fn observe(&mut self) -> Result<fifo::Observation, fifo::Fault> {
        Ok(fifo::Observation {
            generation: 1,
            identity: 7,
            ready: true,
            fatal: false,
        })
    }
    fn descriptor(&mut self, word: usize) -> Result<u32, fifo::Fault> {
        Ok(self.0.borrow().descriptors[word])
    }
    fn read_rx(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), fifo::Fault> {
        let state = self.0.borrow();
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = state.rx[(offset + index) % fifo::FIFO_BYTES];
        }
        Ok(())
    }
    fn read_tx(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), fifo::Fault> {
        let state = self.0.borrow();
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = state.tx[(offset + index) % fifo::FIFO_BYTES];
        }
        Ok(())
    }
    fn write_tx(&mut self, offset: usize, bytes: &[u8]) -> Result<(), fifo::Fault> {
        let mut state = self.0.borrow_mut();
        for (index, byte) in bytes.iter().enumerate() {
            let at = (offset + index) % fifo::FIFO_BYTES;
            state.tx[at] = *byte;
        }
        Ok(())
    }
    fn store_tx_head(&mut self, value: u32) -> Result<(), fifo::Fault> {
        self.0.borrow_mut().descriptors[1] = value;
        Ok(())
    }
    fn store_rx_tail(&mut self, value: u32) -> Result<(), fifo::Fault> {
        self.0.borrow_mut().descriptors[2] = value;
        Ok(())
    }
    fn barrier(&mut self, _: Barrier) -> Result<(), fifo::Fault> {
        Ok(())
    }
    fn notify_peer(&mut self) -> Result<(), fifo::Fault> {
        self.0.borrow_mut().notifications += 1;
        Ok(())
    }
}

#[derive(Default)]
struct FakeInterrupts {
    acked: Vec<u8>,
    schedules: usize,
}
impl Interrupts for FakeInterrupts {
    fn acknowledge(&mut self, channel: u8) -> Result<(), owner_core::Error> {
        self.acked.push(channel);
        Ok(())
    }
    fn schedule(&mut self) -> Result<(), owner_core::Error> {
        self.schedules += 1;
        Ok(())
    }
}

fn word_bytes(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}
fn intent_record() -> Vec<u8> {
    let mut record = Vec::new();
    record.extend(word_bytes(0x0002_0005));
    record.extend(word_bytes(8));
    for id in 1..=8 {
        record.extend(word_bytes(4096));
        record.extend(word_bytes(id));
    }
    record
}
fn data_record(intent: u32, datagram: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    for word in [0x0002_0009, intent, datagram.len() as u32, 0] {
        record.extend(word_bytes(word));
    }
    record.extend(datagram);
    record.resize((record.len() + 7) & !7, 0);
    record
}
fn hello() -> Vec<u8> {
    [1, 2, 0, 0xffff_fffe, 0, 20, 1, 0xffff_fffe, 2, 0, 0, 0, 0]
        .into_iter()
        .flat_map(word_bytes)
        .collect()
}
fn open_request() -> Vec<u8> {
    let path = b"/boot/modem_fs1";
    let mut tlv = vec![1];
    tlv.extend((path.len() as u16).to_le_bytes());
    tlv.extend(path);
    let mut qmi = vec![0, 0x34, 0x12];
    qmi.extend(1u16.to_le_bytes());
    qmi.extend((tlv.len() as u16).to_le_bytes());
    qmi.extend(&tlv);
    let mut packet = Vec::new();
    for word in [1, 1, 0, 9, 0, qmi.len() as u32, 1, 0x4000] {
        packet.extend(word_bytes(word));
    }
    packet.extend(qmi);
    packet.resize((packet.len() + 3) & !3, 0);
    packet
}

fn attach_owner(ring: Ring) -> Owner<Ring> {
    let mut session = Session::new(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap();
    session.register(1, Service::RemoteStorage, 0x4000).unwrap();
    session.register(1, Service::HeapMemory, 0x4001).unwrap();
    Owner::attach(Adapter::attach(ring, session, 1).unwrap(), 1).unwrap()
}

#[test]
fn owner_fifo_qrtr_hello_open_and_reply_use_validated_backend() {
    let ring = Ring::new();
    let pool = pool();
    let disk = disk();
    let backend = combined(&pool, &disk);
    let mut handlers = h::Handlers::attach(
        h::Profile::validate(handler_config(), &[]).unwrap(),
        backend,
    );
    assert_eq!(handlers.error(), None);
    let mut owner = attach_owner(ring.clone());
    owner.start_control().unwrap();
    assert_eq!(owner.offer_receive().unwrap(), 1);
    assert_eq!(owner.offer_receive().unwrap(), 2);
    let mut interrupts = FakeInterrupts::default();

    owner
        .activate_handlers(&mut handlers, &mut interrupts)
        .unwrap();
    assert_eq!(owner.control_status().ready_services, 0b11);
    assert_eq!(owner.control_status().announced_services, 0);

    ring.push_rx(&intent_record());
    ring.push_rx(&data_record(1, &hello()));
    owner
        .notified_with_handlers(
            1 << owner_core::GLINK_CHANNEL,
            &mut handlers,
            &mut interrupts,
        )
        .unwrap();
    for _ in 0..32 {
        if owner.control_status().announced_services == 0b11 {
            break;
        }
        owner
            .notified_with_handlers(
                1 << owner_core::CONTINUE_RECEIVE,
                &mut handlers,
                &mut interrupts,
            )
            .unwrap();
    }

    ring.push_rx(&data_record(2, &open_request()));
    owner
        .notified_with_handlers(
            1 << owner_core::GLINK_CHANNEL,
            &mut handlers,
            &mut interrupts,
        )
        .unwrap();
    for _ in 0..32 {
        if ring.transmitted_summaries().iter().any(|message| {
            message.kind == Kind::Response
                && message.transaction == 0x1234
                && message.message_id == 1
        }) {
            break;
        }
        owner
            .notified_with_handlers(
                1 << owner_core::CONTINUE_RECEIVE,
                &mut handlers,
                &mut interrupts,
            )
            .unwrap();
    }

    assert_eq!(owner.control_status().peer_hello, true);
    assert_eq!(owner.control_status().announced_services, 0b11);
    let replies = ring.transmitted_summaries();
    let reply = replies
        .iter()
        .find(|message| message.kind == Kind::Response && message.message_id == 1)
        .expect("OPEN response traversed owner and FIFO");
    assert_eq!(reply.transaction, 0x1234);
    assert!(reply.has_result);
    assert_eq!(reply.result, [0, 0, 0, 0]);
    assert_eq!(disk.count(Tag::Read), 0);
    assert_eq!(disk.count(Tag::Write), 0);
    assert_eq!(pool.count(Tag::Invoke), 0);
    assert!(interrupts.acked.contains(&owner_core::GLINK_CHANNEL));
}

#[test]
fn owner_does_not_publish_services_when_pool_authority_is_missing() {
    let ring = Ring::new();
    let pool = pool();
    pool.0.borrow_mut().observation.contract = false;
    let backend = combined(&pool, &disk());
    let mut handlers = h::Handlers::attach(
        h::Profile::validate(handler_config(), &[]).unwrap(),
        backend,
    );
    let mut owner = attach_owner(ring);
    owner.start_control().unwrap();
    owner.offer_receive().unwrap();
    owner.offer_receive().unwrap();
    let mut interrupts = FakeInterrupts::default();

    assert!(
        owner
            .activate_handlers(&mut handlers, &mut interrupts)
            .is_err()
    );
    assert_eq!(owner.control_status().ready_services, 0);
    assert_eq!(owner.control_status().announced_services, 0);
    assert_eq!(pool.count(Tag::Invoke), 0);
}
