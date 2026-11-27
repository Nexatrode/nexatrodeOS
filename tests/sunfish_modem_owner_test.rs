include!("sunfish_glink_fifo_test.rs");
#[path = "../rust/src/qcom_modem_handlers.rs"]
pub mod qcom_modem_handlers;
#[path = "../rust/src/qcom_modem_owner.rs"]
pub mod qcom_modem_owner;
#[cfg(feature = "sunfish-native-tftp-server-check")]
#[path = "../rust/src/qcom_tftp_server_check.rs"]
pub mod qcom_tftp_server_check;
#[path = "modem_handlers_fixture.rs"]
pub mod services_fixture;
use qcom_modem_owner::{Error as OwnerError, Interrupts, Layout, Owner, Prelude, WORK_BUDGET};

#[derive(Default)]
struct Irqs {
    calls: Vec<u8>,
    schedules: usize,
    fail_ack: bool,
    fail_schedule: bool,
    revoke: Option<Fake>,
}
impl Interrupts for Irqs {
    fn acknowledge(&mut self, channel: u8) -> Result<(), OwnerError> {
        self.calls.push(channel);
        if let Some(peer) = &self.revoke {
            peer.0.borrow_mut().observation.ready = false;
        }
        if self.fail_ack {
            Err(OwnerError::Ack)
        } else {
            Ok(())
        }
    }
    fn schedule(&mut self) -> Result<(), OwnerError> {
        self.schedules += 1;
        if self.fail_schedule {
            Err(OwnerError::Schedule)
        } else {
            Ok(())
        }
    }
}
fn owner() -> (Fake, Owner<Fake>) {
    let (peer, adapter) = adapter(0, 0);
    (peer, Owner::attach(adapter, 1).unwrap())
}
fn prelude() -> [u8; 88] {
    prelude_order(false)
}
fn prelude_order(ipcrtr_first: bool) -> [u8; 88] {
    let mut b = Vec::new();
    let first_two = if ipcrtr_first {
        [(1, b"IPCRTR".as_slice()), (2, b"glink_ssr".as_slice())]
    } else {
        [(1, b"glink_ssr".as_slice()), (2, b"IPCRTR".as_slice())]
    };
    for (id, name) in first_two
        .into_iter()
        .chain([(3, b"ssm_rtr_modem_apps".as_slice())])
    {
        b.extend(control(2, id, (name.len() + 1) as u32));
        b.extend(name);
        b.push(0);
        b.resize((b.len() + 7) & !7, 0xa5);
    }
    b.extend([1, 0, 1, 0, 0, 0, 0, 0, 4, 0, 1, 0, 0, 0, 0, 0]);
    b.try_into().unwrap()
}

#[test]
fn owner_layout_limits_writes_to_prepared_pages_and_disjoint_buffer_ranges() {
    let layout = Layout::validate((0x11d198, 32), (0x11d1c8, 16384), (0x180000, 16384)).unwrap();
    assert_eq!(layout.offsets(), (0x11d198, 0x11d1c8, 0x180000));
    assert!(Layout::validate((0x11d198, 32), (0x11d1c8, 16384), (0x1211d8, 16384)).is_ok());
    for (desc, tx, rx) in [
        ((0, 32), (0x11d1c8, 16384), (0x180000, 16384)),
        ((0x11d198, 32), (0x11d198, 16384), (0x180000, 16384)),
        ((0x11d198, 32), (0x11d1c8, 16384), (0x11d000, 16384)),
        ((0x11d198, 32), (0x11d1c8, 16384), (usize::MAX, 16384)),
        ((0x11d198, 32), (0x11d1c8, 16384), (0x1ff000, 16384)),
        ((0x11d198, 32), (0x11d1c8, 16384), (0x180000, 16376)),
    ] {
        assert_eq!(Layout::validate(desc, tx, rx), Err(OwnerError::Mapping));
    }
}
#[test]
fn owner_rejects_wrong_injected_mapping_symbols() {
    let good = [0x44000000, 0x4411d000, 0x4411e000, 0x40006000, 0x40005000];
    for index in 0..5 {
        let mut values = good;
        values[index] += 4096;
        assert_eq!(
            Layout::validate_symbols(values[0], values[1], values[2], values[3], values[4]),
            Err(OwnerError::Mapping)
        );
    }
    assert!(Layout::validate_symbols(good[0], good[1], good[2], good[3], good[4]).is_ok());
}
#[test]
fn owner_retains_secondary_open_requests_and_validates_handoff_exactly() {
    let bytes = prelude();
    assert_eq!(Prelude::validate(bytes).unwrap().retained_bytes(), &bytes);
    for index in [
        0, 2, 4, 17, 24, 26, 28, 32, 38, 40, 42, 44, 66, 72, 74, 80, 82,
    ] {
        let mut bad = bytes;
        bad[index] ^= 1;
        assert!(Prelude::validate(bad).is_err(), "byte {index}");
    }
}
#[test]
fn owner_accepts_both_valid_first_open_orders_but_rejects_bad_targets_and_ids() {
    let reversed = prelude_order(true);
    assert_eq!(
        Prelude::validate(reversed).unwrap().retained_bytes(),
        &reversed
    );

    // The peer's non-IPCRTR offer is a bounded opaque name, as in the earlier
    // native OPEN-plan check; changing its case must not reject the handoff.
    let mut opaque_offer = reversed;
    opaque_offer[24] ^= 0x20;
    assert!(Prelude::validate(opaque_offer).is_ok());

    let mut bad_target = reversed;
    bad_target[8] ^= 1;
    assert_eq!(
        Prelude::validate(bad_target).err(),
        Some(OwnerError::Prelude)
    );

    let mut bad_second_id = reversed;
    bad_second_id[18] ^= 1; // reversed ID1 record is 16 bytes; ID2 starts here
    assert_eq!(
        Prelude::validate(bad_second_id).err(),
        Some(OwnerError::Prelude)
    );

    let mut bad_third_name = reversed;
    bad_third_name[48] = 0; // embedded NUL before the declared terminator
    assert_eq!(
        Prelude::validate(bad_third_name).err(),
        Some(OwnerError::Prelude)
    );
}
#[test]
fn owner_status_is_rechecked_before_ack_and_status_irq_precedes_glink() {
    let (_, mut owner) = owner();
    let mut irqs = Irqs::default();
    let progress = owner.notified(3, &mut irqs).unwrap();
    assert_eq!(irqs.calls, [1, 0]);
    assert_eq!(progress.acknowledged, 3);
    assert_eq!(irqs.schedules, 1); // Post-rearm check, independent of a peer edge.
    owner.notified(8, &mut irqs).unwrap();
    assert_eq!(irqs.schedules, 1);
}
#[test]
fn owner_fatal_status_suppresses_all_transport_work_and_irq_rearm() {
    let (peer, mut owner) = owner();
    owner.offer_receive().unwrap();
    peer.clear();
    peer.0.borrow_mut().observation.fatal = true;
    let mut irqs = Irqs::default();
    assert!(owner.notified(3, &mut irqs).is_err());
    assert!(writes(&peer.log()).is_empty());
    assert!(irqs.calls.is_empty());
    assert_eq!(irqs.schedules, 0);
    peer.0.borrow_mut().observation.fatal = false;
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Stopped));
}
#[test]
fn owner_busy_irq_yields_with_irq_masked_then_rearms_after_drain() {
    let (peer, mut owner) = owner();
    for _ in 0..WORK_BUDGET + 2 {
        peer.push(&control(13, 0, 0));
    }
    let mut irqs = Irqs::default();
    let first = owner.notified(1, &mut irqs).unwrap();
    assert_eq!(first.received as usize, WORK_BUDGET);
    assert!(first.deferred);
    assert!(irqs.calls.is_empty());
    let second = owner.notified(8, &mut irqs).unwrap();
    assert_eq!(second.received, 2);
    assert_eq!(irqs.calls, [0]);
    assert!(second.deferred);
    owner.notified(8, &mut irqs).unwrap();
    assert_eq!(irqs.schedules, 2);
}
#[test]
fn owner_coalesces_busy_irq_notifications_into_one_pending_ack() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    for _ in 0..WORK_BUDGET * 2 + 1 {
        peer.push(&control(13, 0, 0));
    }
    owner.notified(1, &mut irqs).unwrap();
    owner.notified(1, &mut irqs).unwrap();
    assert!(irqs.calls.is_empty());
    assert_eq!(irqs.schedules, 1);
    owner.notified(8, &mut irqs).unwrap();
    assert_eq!(irqs.calls, [0]);
}
#[test]
fn owner_partial_record_rearms_and_checks_after_ack_without_consuming() {
    let (peer, mut owner) = owner();
    peer.push(&control(9, 2, 1));
    let mut irqs = Irqs::default();
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(peer.0.borrow().words[2], 0);
    assert_eq!(irqs.calls, [0]);
    assert_eq!(irqs.schedules, 1);
}
#[test]
fn owner_post_ack_continuation_finds_work_without_a_second_irq() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    owner.notified(1, &mut irqs).unwrap();
    peer.push(&control(13, 0, 0));
    assert_eq!(owner.notified(8, &mut irqs).unwrap().received, 1);
    assert_eq!(irqs.calls, [0]);
}
#[test]
fn owner_ready_loss_between_irq_acks_leaves_remaining_irq_masked() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs {
        revoke: Some(peer),
        ..Irqs::default()
    };
    assert!(owner.notified(3, &mut irqs).is_err());
    assert_eq!(irqs.calls, [1]);
    assert_eq!(irqs.schedules, 0);
}
#[test]
fn owner_uncertain_irq_ack_is_not_retried() {
    let (_, mut owner) = owner();
    let mut irqs = Irqs {
        fail_ack: true,
        ..Irqs::default()
    };
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Ack));
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Stopped));
    assert_eq!(irqs.calls, [0]);
}
#[test]
fn owner_failed_continuation_stops_instead_of_spinning() {
    let (_, mut owner) = owner();
    let mut irqs = Irqs {
        fail_schedule: true,
        ..Irqs::default()
    };
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Schedule));
    assert_eq!(irqs.schedules, 1);
    assert!(owner.error().is_some());
}
#[test]
fn owner_unrelated_notification_never_acks_a_capability() {
    let (_, mut owner) = owner();
    let mut irqs = Irqs::default();
    assert_eq!(
        owner.notified(4, &mut irqs),
        Err(OwnerError::UnexpectedNotification)
    );
    assert!(irqs.calls.is_empty());
    assert_eq!(irqs.schedules, 0);
}

#[test]
fn owner_external_quarantine_preserves_original_failure() {
    let (_, mut owner) = owner();
    let mut irqs = Irqs::default();
    assert_eq!(owner.notified(4, &mut irqs), Err(OwnerError::UnexpectedNotification));
    assert_eq!(owner.error(), Some(OwnerError::UnexpectedNotification));
    owner.fault();
    assert_eq!(owner.error(), Some(OwnerError::UnexpectedNotification));
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Stopped));
    assert!(irqs.calls.is_empty());
}
#[test]
fn owner_retains_typed_request_until_explicit_backend_reply() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    let id = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&query(id));
    owner.notified(1, &mut irqs).unwrap();
    let handle = owner.pending_request().unwrap();
    assert!(matches!(
        owner.request(&handle).unwrap().1,
        Request::Memory(MemoryRequest::QuerySize { .. })
    ));
    assert_eq!(owner.in_flight(), 1);
    owner
        .queue_reply(&handle, Reply::Failure { error: 3 }, &mut irqs)
        .unwrap();
    assert!(owner.pending_request().is_none());
    assert_eq!(owner.in_flight(), 1);
}
#[test]
fn owner_lifecycle_loss_rejects_backend_reply_and_retains_request() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    let id = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&query(id));
    owner.notified(1, &mut irqs).unwrap();
    let handle = owner.pending_request().unwrap();
    peer.0.borrow_mut().observation.ready = false;
    assert!(
        owner
            .queue_reply(&handle, Reply::Failure { error: 3 }, &mut irqs)
            .is_err()
    );
    assert_eq!(owner.pending_request(), Some(handle));
    assert_eq!(owner.in_flight(), 1);
    assert_eq!(owner.offer_receive(), Err(OwnerError::Stopped));
}
#[test]
fn owner_unwinding_io_cannot_resume_from_a_later_notification() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    owner.offer_receive().unwrap();
    peer.0.borrow_mut().panic_on_notify = true;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.notified(1, &mut irqs)
    }));
    assert!(result.is_err());
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Stopped));
}

#[test]
fn owner_handoff_checks_all_descriptor_words_before_tail_publication() {
    assert!(Prelude::validate_cursors([40, 40, 8, 120, 0, 0, 0, 0]).is_ok());
    for (index, value) in [
        (0, 48),
        (1, 48),
        (2, 96),
        (3, 95),
        (3, 0x4000),
        (3, u32::MAX),
        (4, 1),
        (7, 1),
    ] {
        let mut words = [40, 40, 8, 120, 0, 0, 0, 0];
        words[index] = value;
        assert_eq!(Prelude::validate_cursors(words), Err(OwnerError::Prelude));
    }
}
#[test]
fn owner_backend_reply_schedules_work_without_waiting_for_peer_irq() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    let id = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&query(id));
    owner.notified(1, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let handle = owner.pending_request().unwrap();
    let before = irqs.schedules;
    owner
        .queue_reply(&handle, Reply::Failure { error: 3 }, &mut irqs)
        .unwrap();
    assert_eq!(irqs.schedules, before + 1);
}

fn owner_control_frame(kind: u32, fields: [u32; 4]) -> Vec<u8> {
    [
        1,
        kind,
        0,
        0xffff_fffe,
        0,
        20,
        1,
        0xffff_fffe,
        kind,
        fields[0],
        fields[1],
        fields[2],
        fields[3],
    ]
    .iter()
    .flat_map(|w| w.to_le_bytes())
    .collect()
}
fn owner_qrtr_packet(
    kind: u32,
    peer_port: u32,
    local_port: u32,
    confirm: bool,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = Vec::new();
    for word in [
        1,
        kind,
        0,
        peer_port,
        u32::from(confirm),
        payload.len() as u32,
        1,
        local_port,
    ] {
        packet.extend(word.to_le_bytes());
    }
    packet.extend_from_slice(payload);
    packet.resize((packet.len() + 3) & !3, 0);
    packet
}
fn owner_control_start() -> (Fake, Owner<Fake>, Irqs) {
    let (peer, mut owner) = owner();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    (peer, owner, irqs)
}
#[test]
fn owner_stages_servreg_before_control_without_publishing_early() {
    let (peer, mut owner) = owner();
    let mut irqs = Irqs::default();
    owner.prepare_service_registry().unwrap();
    assert_eq!(owner.control_status().ready_services, 0b100);
    assert_eq!(owner.control_status().announced_services, 0);
    assert!(peer.take_tx().is_empty());

    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    owner.notified(8, &mut irqs).unwrap();
    let outgoing = owner_datagrams(&peer.take_tx());
    assert_eq!(outgoing.len(), 1);
    assert_eq!(u32::from_le_bytes(outgoing[0][4..8].try_into().unwrap()), 2);
    assert_eq!(owner.control_status().announced_services, 0);
}
fn owner_datagrams(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut at = 0;
    let mut output = Vec::new();
    while at < bytes.len() {
        let command = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let word =
            |offset| u32::from_le_bytes(bytes[at + offset..at + offset + 4].try_into().unwrap());
        at += match command {
            5 => 8 + word(4) as usize * 8,
            9 | 12 => {
                let chunk = word(8) as usize;
                output.push(bytes[at + 16..at + 16 + chunk].to_vec());
                (16 + chunk + 7) & !7
            }
            6 | 8 | 13 | 14 => 8,
            _ => panic!("unexpected outgoing command {command}"),
        };
    }
    assert_eq!(at, bytes.len());
    output
}
fn owner_complete_datagrams(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut at = 0;
    let mut current = std::collections::BTreeMap::<u32, Vec<u8>>::new();
    let mut output = Vec::new();
    while at < bytes.len() {
        let command = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let word =
            |offset| u32::from_le_bytes(bytes[at + offset..at + offset + 4].try_into().unwrap());
        at += match command {
            5 => 8 + word(4) as usize * 8,
            9 | 12 => {
                let intent = word(4);
                let chunk = word(8) as usize;
                if command == 9 {
                    assert!(current.insert(intent, Vec::new()).is_none(), "duplicate start for intent {intent}");
                }
                current.get_mut(&intent).expect("continuation without a start")
                    .extend_from_slice(&bytes[at + 16..at + 16 + chunk]);
                if word(12) == 0 {
                    output.push(current.remove(&intent).unwrap());
                }
                (16 + chunk + 7) & !7
            }
            6 | 8 | 13 | 14 => 8,
            _ => panic!("unexpected outgoing command {command}"),
        };
    }
    assert_eq!(at, bytes.len());
    assert!(current.is_empty());
    output
}
fn acknowledge_outgoing_glink(
    peer: &Fake,
    owner: &mut Owner<Fake>,
    irqs: &mut Irqs,
    records: &[u8],
) {
    let mut at = 0;
    let mut done = Vec::new();
    while at < records.len() {
        let command = u16::from_le_bytes([records[at], records[at + 1]]);
        let word =
            |offset| u32::from_le_bytes(records[at + offset..at + offset + 4].try_into().unwrap());
        at += match command {
            5 => 8 + word(4) as usize * 8,
            9 | 12 => {
                let chunk = word(8) as usize;
                if command == 9 {
                    done.extend(control(6, 2, word(4)));
                }
                (16 + chunk + 7) & !7
            }
            _ => 8,
        };
    }
    assert_eq!(at, records.len());
    if !done.is_empty() {
        peer.push(&done);
        owner.notified(1, irqs).unwrap();
    }
}
fn take_owner_datagrams(peer: &Fake, owner: &mut Owner<Fake>, irqs: &mut Irqs) -> Vec<Vec<u8>> {
    let records = peer.take_tx();
    let datagrams = owner_datagrams(&records);
    acknowledge_outgoing_glink(peer, owner, irqs, &records);
    datagrams
}
fn owner_control_peer_hello(peer: &Fake, owner: &mut Owner<Fake>, irqs: &mut Irqs) {
    peer.push(&data(1, &owner_control_frame(2, [0; 4])));
    let progress = owner.notified(1, irqs).unwrap();
    assert_eq!(progress.controls, 1);
    assert_eq!(progress.requests, 0);
    assert!(owner.control_status().peer_hello);
}

fn tftp_rrq(path: &[u8], options: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut packet = vec![0, 1];
    packet.extend_from_slice(path);
    packet.push(0);
    packet.extend_from_slice(b"octet\0");
    for (name, value) in options {
        packet.extend_from_slice(name);
        packet.push(0);
        packet.extend_from_slice(value);
        packet.push(0);
    }
    packet
}

#[cfg(feature = "sunfish-native-tftp-server-check")]
fn server_check_wrq_for_backpressure() -> Vec<u8> {
    let mut request = qcom_tftp_server_check::REQUEST.to_vec();
    if cfg!(feature = "sunfish-wlan-first-server-check-retry") {
        // The server accepts either case. Keep these fixtures on the ordinary
        // backpressure path by using a WRQ outside the exact retry experiment.
        request[qcom_tftp_server_check::REQUEST_PREFIX.len()] = b'O';
    }
    request
}

fn tftp_qrtr(packet: &[u8], peer_port: u32, local_port: u32) -> Vec<u8> {
    let mut datagram = Vec::new();
    for value in [1u32, 1, 0, peer_port, 1, packet.len() as u32, 1, local_port] {
        datagram.extend(value.to_le_bytes());
    }
    datagram.extend_from_slice(packet);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn owner_word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn owner_dispatches_held_tftp_rrq_to_bounded_server_and_queues_oack() {
    use qcom_modem_owner::TftpServiceProgress;
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};

    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x4300).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    let announced = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert!(announced.iter().any(|packet| {
        owner_word(packet, 36) == qcom_qrtr_control::TFTP_SERVICE_ID
            && owner_word(packet, 48) == 0x4300
    }));

    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    let rrq = tftp_rrq(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        &[(b"tsize", b"0")],
    );
    peer.push(&data(intent, &tftp_qrtr(&rrq, 0x6000, 0x4300)));
    owner.notified(1, &mut irqs).unwrap();
    let handle = owner.pending_tftp_request().unwrap();
    let (route, raw) = owner.tftp_datagram(&handle).unwrap();
    assert_eq!(route.source.port, 0x4300);
    assert_eq!(route.destination.port, 0x6000);
    assert_eq!(raw, rrq);

    let image = b"synthetic test image";
    let mut server = Server::new(
        1,
        0,
        0x4300,
        0x4400,
        &[],
        AuthorizedWlanImage::test_fixture(image),
    )
    .unwrap();
    assert_eq!(
        owner.service_tftp_step(&mut server, &mut irqs),
        Ok(TftpServiceProgress::ResponseQueued)
    );
    assert!(owner.pending_tftp_request().is_none());
    owner.notified(8, &mut irqs).unwrap();

    let records = peer.take_tx();
    let datagrams = owner_datagrams(&records);
    let reply = datagrams
        .iter()
        .find(|packet| owner_word(packet, 12) == 0x4400 && owner_word(packet, 28) == 0x6000)
        .unwrap();
    assert_eq!(owner_word(reply, 4), 1);
    assert_eq!(&reply[32..34], &qcom_tftp::OP_OACK.to_be_bytes());
    assert!(reply[32..].windows(6).any(|field| field == b"tsize\0"));
}

#[cfg(all(feature = "sunfish-tftp-close-owner-fence", not(feature = "sunfish-tftp-wlan-close-handoff")))]
#[test]
fn active_firmware_peer_is_visible_to_owner_close_fence() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, IncomingRoute, Peer, Server};
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut server = Server::new(1, 0, 0x5000, 0x5100, &[], AuthorizedWlanImage::test_fixture(b"firmware")).unwrap();
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"blksize", b"8")]);
    assert!(matches!(server.receive(IncomingRoute { destination_node: 1, destination_port: 0x5000,
        source: Peer { node: 0, port: 0x70 } }, &rrq, &mut output), qcom_qrtr_tftp::Dispatch::Send { .. }));
    owner.snapshot_tftp_firmware_peers(&server).unwrap();
    let closing = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(closing, &owner_control_frame(6, [0, 0x70, 0, 0])));
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Transport(Error::Session(
        qcom_glink_qmi::Error::Control(qcom_qrtr_control::Error::ClientGone)))));
}

#[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
#[test]
fn quiescent_firmware_close_retires_only_its_session_and_continues() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, IncomingRoute, Peer, Server};
    let (peer, mut owner) = owner(); owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents); let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs); take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut firmware = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[], AuthorizedWlanImage::test_fixture(b"firmware")).unwrap();
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"blksize", b"8")]);
    for port in [0x70, 0x71] {
        assert!(matches!(firmware.receive(IncomingRoute { destination_node: 1, destination_port: 0x5000,
            source: Peer { node: 0, port } }, &rrq, &mut output), qcom_qrtr_tftp::Dispatch::Send { .. }));
    }
    owner.snapshot_tftp_firmware_peers(&firmware).unwrap();
    let closing = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(closing, &owner_control_frame(6, [0, 0x70, 0, 0])));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.pending_firmware_close(), Some(qcom_qmi_router::Address { node: 0, port: 0x70 }));
    owner.resolve_firmware_close(&mut firmware).unwrap();
    assert_eq!(owner.pending_firmware_close(), None);
    assert_eq!(firmware.active_peers(), [None, Some(Peer { node: 0, port: 0x71 }), None, None]);
    owner.notified(8, &mut irqs).unwrap();
}

#[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
#[test]
fn firmware_close_validation_fault_retains_session_and_stops_owner() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, IncomingRoute, Peer, Server};
    let (peer, mut owner) = owner(); owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents); let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs); take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut firmware = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[], AuthorizedWlanImage::test_fixture(b"firmware")).unwrap();
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"blksize", b"8")]);
    let key = Peer { node: 0, port: 0x70 };
    assert!(matches!(firmware.receive(IncomingRoute { destination_node: 1, destination_port: 0x5000, source: key }, &rrq, &mut output), qcom_qrtr_tftp::Dispatch::Send { .. }));
    owner.snapshot_tftp_firmware_peers(&firmware).unwrap();
    let closing = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(closing, &owner_control_frame(6, [0, key.port, 0, 0]))); owner.notified(1, &mut irqs).unwrap();
    assert!(owner.pending_firmware_close().is_some());
    peer.clear(); peer.0.borrow_mut().fail = Some("observe");
    assert!(owner.resolve_firmware_close(&mut firmware).is_err());
    assert!(firmware.active_peers().contains(&Some(key)));
    assert!(owner.resolve_firmware_close(&mut firmware).is_err());
    assert!(writes(&peer.log()).is_empty());
}

#[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
#[test]
fn firmware_close_discards_only_its_exact_unpublished_retained_reply() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner(); owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents); let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs); take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut firmware = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[], AuthorizedWlanImage::test_fixture(b"firmware")).unwrap();
    let mut check = qcom_tftp_server_check::ServerCheck::new();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"blksize", b"8")]);
    for iteration in 0..2 {
        let incoming = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let mut packet = tftp_qrtr(&rrq, 0x6000, 0x5000); packet[16..20].copy_from_slice(&0u32.to_le_bytes());
        peer.push(&data(incoming, &packet)); owner.notified(1, &mut irqs).unwrap();
        let progress = owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs).unwrap();
        if iteration == 0 {
            assert_eq!(progress, TftpServiceProgress::ResponseQueued);
            owner.notified(8, &mut irqs).unwrap();
            // Keep the first response in flight while dispatching its retry.
            let records = peer.take_tx();
            let second = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            peer.push(&data(second, &packet)); owner.notified(1, &mut irqs).unwrap();
            assert_eq!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs), Ok(TftpServiceProgress::WaitingReply));
            acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records); peer.take_tx();
            break;
        }
    }
    let closing = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(closing, &owner_control_frame(6, [0, 0x6000, 0, 0])));
    owner.notified(1, &mut irqs).unwrap();
    assert!(owner.pending_firmware_close().is_some());
    owner.resolve_firmware_close(&mut firmware).unwrap();
    assert_eq!(firmware.active_transfers(), 0);
    assert!(!owner.tftp_queue_snapshot().1);
    owner.notified(8, &mut irqs).unwrap();
}

#[cfg(feature = "sunfish-tftp-wlan-mux")]
#[test]
fn combined_owner_routes_firmware_rrq_on_shared_server_port() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let intent = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"tsize", b"0")]);
    peer.push(&data(intent, &tftp_qrtr(&rrq, 0x6000, 0x5000))); owner.notified(1, &mut irqs).unwrap();
    let mut firmware = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[],
        AuthorizedWlanImage::test_fixture(b"firmware")).unwrap();
    let mut check = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs),
        Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
    assert_eq!(firmware.active_transfers(), 1);
    owner.notified(8, &mut irqs).unwrap();
    let packets = owner_datagrams(&peer.take_tx());
    assert!(packets.iter().any(|p| owner_word(p, 12) == 0x5100 && owner_word(p, 28) == 0x6000
        && p[32..34] == [0, 6]));
}

#[cfg(feature = "sunfish-tftp-wlan-mux")]
#[test]
fn combined_owner_completes_multiblock_firmware_read() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents); let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs); take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let image: Vec<u8> = (0..19).collect();
    let mut firmware = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[],
        AuthorizedWlanImage::test_fixture(&image)).unwrap();
    let mut check = qcom_tftp_server_check::ServerCheck::new();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"blksize", b"8"), (b"wsize", b"2")]);
    let receive = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut packet = tftp_qrtr(&rrq, 0x6000, 0x5000); packet[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(receive, &packet)); owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    owner.notified(8, &mut irqs).unwrap();
    let first_records = peer.take_tx();
    let packets = owner_complete_datagrams(&first_records);
    let oack = packets.iter().find(|p| owner_word(p, 12) == 0x5100 && owner_word(p, 28) == 0x6000).unwrap();
    assert!(oack[32..].windows(8).any(|v| v == b"wsize\x001\0"));
    let original_oack = oack[32..32 + owner_word(oack, 20) as usize].to_vec();
    // Retransmit while the first response still owns its GLINK intent: the
    // second response must be retained without creating another transfer.
    let receive = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut packet = tftp_qrtr(&rrq, 0x6000, 0x5000); packet[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(receive, &packet)); owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs), Ok(TftpServiceProgress::WaitingReply));
    assert_eq!(firmware.active_transfers(), 1);
    acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &first_records); peer.take_tx();
    assert_eq!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    owner.notified(8, &mut irqs).unwrap();
    let packets = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let repeat = packets.iter().find(|p| owner_word(p, 12) == 0x5100 && owner_word(p, 28) == 0x6000).unwrap();
    assert_eq!(&repeat[32..32 + owner_word(repeat, 20) as usize], original_oack);
    for block in 0..3u16 {
        let receive = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let [hi, lo] = block.to_be_bytes();
        let mut packet = tftp_qrtr(&[0, 4, hi, lo], 0x6000, 0x5100); packet[16..20].copy_from_slice(&0u32.to_le_bytes());
        peer.push(&data(receive, &packet));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
        owner.notified(8, &mut irqs).unwrap();
        let packets = take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let pkt = packets.iter().find(|p| owner_word(p, 12) == 0x5100 && owner_word(p, 28) == 0x6000).unwrap();
        let bytes = owner_word(pkt, 20) as usize;
        assert_eq!(&pkt[32..36], &[0, 3, 0, block as u8 + 1]);
        let start = usize::from(block) * 8;
        assert_eq!(&pkt[36..32 + bytes], &image[start..(start + 8).min(image.len())]);
    }
    let receive = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut packet = tftp_qrtr(&[0, 4, 0, 3], 0x6000, 0x5100); packet[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(receive, &packet)); owner.notified(1, &mut irqs).unwrap();
    assert!(matches!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs),
        Ok(TftpServiceProgress::Closed(qcom_qrtr_tftp::CloseReason::TransferComplete))));
    assert_eq!(firmware.active_transfers(), 0);
}

#[cfg(feature = "sunfish-tftp-wlan-mux")]
#[test]
fn combined_owner_firmware_dispatch_fault_does_not_create_a_transfer() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    let (peer, mut owner) = owner(); owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents); let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs); take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let receive = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"blksize", b"8")]);
    peer.push(&data(receive, &tftp_qrtr(&rrq, 0x6000, 0x5000))); owner.notified(1, &mut irqs).unwrap();
    let mut firmware = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[], AuthorizedWlanImage::test_fixture(b"firmware")).unwrap();
    let mut check = qcom_tftp_server_check::ServerCheck::new();
    peer.0.borrow_mut().fail = Some("observe");
    assert!(owner.service_combined_tftp_step(&mut check, &mut firmware, &mut irqs).is_err());
    assert_eq!(firmware.active_transfers(), 0);
    assert_eq!(firmware.next_transfer_port(), 0x5100);
    assert_eq!(owner.notified(8, &mut irqs), Err(OwnerError::Stopped));
}

#[cfg(feature = "sunfish-tftp-wlan-mux")]
#[test]
fn firmware_close_before_dispatch_stops_without_publishing_a_reply() {
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents); let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs); take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let request = owner.offer_receive().unwrap(); let closing = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[(b"tsize", b"0")]);
    peer.push(&data(request, &tftp_qrtr(&rrq, 0x6000, 0x5000))); owner.notified(1, &mut irqs).unwrap();
    assert!(owner.pending_tftp_request().is_some());
    peer.push(&data(closing, &owner_control_frame(6, [0, 0x6000, 0, 0])));
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Transport(Error::Session(
        qcom_glink_qmi::Error::Control(qcom_qrtr_control::Error::ClientGone)))));
    assert!(owner.pending_tftp_request().is_some());
}

#[cfg(feature = "sunfish-native-tftp-server-check")]
#[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_server_check_queues_real_oack_on_separate_transfer_port() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000)));
    owner.notified(1, &mut irqs).unwrap();
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    assert!(owner.pending_tftp_request().is_none());
    let (queued, retained) = owner.tftp_queue_snapshot();
    assert!(!retained); assert_eq!(queued.tx_count, 1); assert_ne!(queued.tx_flags & 1, 0);
    owner.notified(8, &mut irqs).unwrap();
    let records = peer.take_tx();
    let datagrams = owner_datagrams(&records);
    let reply = datagrams.iter().find(|p| owner_word(p, 12) == 0x5001 && owner_word(p, 28) == 0x56).unwrap();
    let body = &reply[32..32 + qcom_tftp_server_check::OACK.len()];
    assert_eq!(body, qcom_tftp_server_check::OACK);
    // Complete GLINK but withhold QRTR credit; the DATA acknowledgement must
    // be retained once, rather than replaying the RAM write under backpressure.
    acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
    peer.take_tx();
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &tftp_qrtr(b"\0\x03\0\x01ok", 0x56, 0x5001)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::WaitingReply));
    assert_eq!(server.contents(), b"ok");
    assert!(owner.pending_tftp_request().is_none());
    let credit_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let credit: Vec<u8> = [1u32, 7, 0, 0x56, 0, 20, 1, 0x5001, 7, 0, 0x56, 0, 0]
        .iter().flat_map(|word| word.to_le_bytes()).collect();
    peer.push(&data(credit_intent, &credit));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    owner.notified(8, &mut irqs).unwrap();
    let records = peer.take_tx();
    let reply = owner_complete_datagrams(&records).into_iter()
        .find(|p| owner_word(p, 12) == 0x5001 && owner_word(p, 28) == 0x56).unwrap();
    assert_eq!(&reply[32..36], &[0, 4, 0, 1]);
    assert_eq!(server.contents(), b"ok");
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn server_check_reference_flow_retires_oack_after_glink_and_accepts_data() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000)));
    owner.notified(1, &mut irqs).unwrap();
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    {
        let trace = owner.tftp_endpoint_trace().unwrap();
        assert_eq!((trace.source.node, trace.source.port, trace.destination.node, trace.destination.port), (1, 0x5001, 0, 0x56));
        assert_eq!((trace.payload_bytes, trace.flags, trace.remote_intent), (39, 1, u32::MAX));
    }
    assert_eq!(owner.tftp_tx_progress(), [0, 0, 0], "queueing alone is not publication");
    owner.notified(8, &mut irqs).unwrap();
    let records = peer.take_tx();
    let packets = owner_datagrams(&records);
    let reply = packets.iter().find(|p| owner_word(p, 12) == 0x5001 && owner_word(p, 28) == 0x56).unwrap();
    // The pinned Linux qrtr_hdr_v1 orders these eight little-endian words,
    // and qrtr_node_enqueue pads the body to a four-byte boundary.
    let header: Vec<u32> = (0..8).map(|i| owner_word(reply, i * 4)).collect();
    assert_eq!(header, [1, 1, 1, 0x5001, 0, qcom_tftp_server_check::OACK.len() as u32, 0, 0x56]);
    assert_eq!(reply.len(), (32 + qcom_tftp_server_check::OACK.len() + 3) & !3);
    assert!(reply[32 + qcom_tftp_server_check::OACK.len()..].iter().all(|b| *b == 0));
    assert_eq!(owner_word(reply, 16), 0, "first flow packet does not request QRTR confirmation");
    assert_eq!(&reply[32..32 + qcom_tftp_server_check::OACK.len()], qcom_tftp_server_check::OACK);
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    {
        let trace = owner.tftp_endpoint_trace().unwrap();
        assert_eq!(trace.flags, 7, "publication is separate from completion");
        assert_ne!(trace.remote_intent, u32::MAX);
    }
    assert_eq!(owner.tftp_tx_progress(), [1, 1, 0], "wire publication is not GLINK completion");
    assert_eq!(owner.tftp_queue_snapshot().0.tx_count, 1, "GLINK completion still required");
    acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
    assert_eq!(owner.tftp_tx_progress(), [1, 1, 1], "the matching peer completion is separately counted");
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    assert_eq!(owner.tftp_final_ack_trace(), None, "OACK is not a final DATA ACK");
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    let first_endpoint = { let trace = owner.tftp_endpoint_trace().unwrap(); assert_eq!(trace.flags, 15); trace };
    assert_eq!(owner.tftp_queue_snapshot().0.tx_count, 0);
    peer.take_tx();
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &tftp_qrtr(b"\0\x03\0\x01ok", 0x56, 0x5001)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    assert_eq!(owner.tftp_endpoint_trace(), Some(first_endpoint), "later DATA acknowledgement cannot overwrite the first OACK trace");
    assert_eq!(server.contents(), b"ok");
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    {
        assert_eq!(server.completed_count(), 1);
        let trace = owner.tftp_final_ack_trace().unwrap();
        assert_eq!((trace.source.node, trace.source.port, trace.destination.node, trace.destination.port), (1, 0x5001, 0, 0x56));
        assert_eq!((trace.block, trace.flags, trace.remote_intent), (1, 3, u32::MAX), "local DATA acceptance and queueing do not prove publication");
        owner.notified(8, &mut irqs).unwrap();
        let records = peer.take_tx();
        let packets = owner_datagrams(&records);
        let ack = packets.iter().find(|p| owner_word(p, 12) == 0x5001 && owner_word(p, 28) == 0x56).unwrap();
        assert_eq!(&ack[32..36], &[0, 4, 0, 1]);
        assert_eq!(owner.tftp_final_ack_trace().unwrap().flags, 15, "publication is not completion");
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
        assert_eq!(owner.tftp_final_ack_trace().unwrap().flags, 31);
        assert_eq!(owner.tftp_endpoint_trace(), Some(first_endpoint));
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        {
            let final_ack = owner.tftp_final_ack_trace();
            peer.take_tx();
            let intent = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            peer.push(&data(intent, &tftp_qrtr(b"\0\x01/readwrite/mcfg.tmp\0octet\0", 0x57, 0x5000)));
            owner.notified(1, &mut irqs).unwrap();
            assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
            let trace = owner.tftp_missing_reply_trace().unwrap();
            assert_eq!((trace.flags, trace.destination.port, trace.source.port, trace.payload_bytes), (3, 0x57, 0x5002, 19));
            assert_eq!((server.total_bytes(), server.completed_count(), server.missing_requests()), (2, 1, 1));
            owner.notified(8, &mut irqs).unwrap();
            let records = peer.take_tx();
            let packets = owner_datagrams(&records);
            let reply = packets.iter().find(|p| owner_word(p, 12) == 0x5002 && owner_word(p, 28) == 0x57).unwrap();
            assert_eq!(&reply[32..51], qcom_tftp_server_check::MISSING_REPLY);
            assert_eq!(owner.tftp_missing_reply_trace().unwrap().flags, 15);
            acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
            assert_eq!(owner.tftp_missing_reply_trace().unwrap().flags, 31);
            assert_eq!(owner.tftp_final_ack_trace(), final_ack, "error reply cannot change prior final ACK evidence");
        }
    }
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn two_server_check_peers_receive_separate_transfer_ports_through_owner() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    {
        use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
        static CLOCK: AtomicU64 = AtomicU64::new(1);
        owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
    }
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    for (remote, local) in [(0x56, 0x5001), (0x57, 0x5002)] {
        let slot = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let request = owner_qrtr_packet(1, remote, 0x5000,
            !cfg!(feature = "sunfish-wlan-tftp-oack-compare") || remote == 0x57,
            qcom_tftp_server_check::REQUEST);
        peer.push(&data(slot, &request));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
        owner.notified(8, &mut irqs).unwrap();
        let records = peer.take_tx();
        let packets = owner_datagrams(&records);
        let reply = packets.iter().find(|p| owner_word(p, 12) == local && owner_word(p, 28) == remote).unwrap();
        assert_eq!(owner_word(reply, 16), 0);
        assert_eq!(&reply[32..32 + qcom_tftp_server_check::OACK.len()], qcom_tftp_server_check::OACK);
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
        peer.take_tx();
    }
    assert_eq!(server.active_count(), 2);
    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    {
        let [Some(first), None] = owner.server_check_oacks() else { panic!("expected only the first WRQ"); };
        assert_eq!((first.peer.port, first.transfer_port), (0x56, 0x5001));
        assert!(!first.confirm_requested);
        assert_eq!(first.resume_at, None);
        assert!(first.ready_at_oack.is_some_and(|count| count > 0));
        assert!(first.wrq_at.unwrap() < first.wrq_done_at.unwrap());
        assert!(first.wrq_done_at.unwrap() < first.oack_at.unwrap());
        assert!(first.oack_at.unwrap() < first.oack_done_at.unwrap());

        for (port, expected) in [
            (0x5001, TftpServiceProgress::Ignored(qcom_qrtr_tftp::IgnoreReason::InvalidTransferPacket)),
            (0x5002, TftpServiceProgress::ResponseQueued),
        ] {
            let slot = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap();
            peer.take_tx();
            let packet = owner_qrtr_packet(1, 0x57, port, false, b"\x00\x03\x00\x01test\n");
            peer.push(&data(slot, &packet));
            owner.notified(1, &mut irqs).unwrap();
            assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(expected));
        }
        let [Some(original), Some(done)] = owner.server_check_oacks() else { panic!("expected both transfers"); };
        assert_eq!(original, first, "wrong-port DATA must not change the first transfer");
        assert_eq!((done.peer.port, done.transfer_port), (0x57, 0x5002));
        assert!(done.confirm_requested);
        assert!(done.resume_at.is_some_and(|at| at > done.wrq_at.unwrap()));
        assert!(done.ready_at_oack.is_some_and(|count| count > 0));
        assert!(done.oack_at.unwrap() < done.oack_done_at.unwrap());
        assert!(done.oack_done_at.unwrap() < done.first_data_at.unwrap());
        assert_eq!(server.completed_count(), 1);
    }
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn pending_server_reply_gets_a_turn_after_glink_completion_before_next_request() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut slots = Vec::new();
    for _ in 0..6 { slots.push(owner.offer_receive().unwrap()); owner.notified(8, &mut irqs).unwrap(); peer.take_tx(); }
    peer.push(&data(slots[0], &tftp_qrtr(&server_check_wrq_for_backpressure(), 0x56, 0x5000)));
    owner.notified(1, &mut irqs).unwrap();
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::ResponseQueued));
    owner.notified(8, &mut irqs).unwrap();
    let records = peer.take_tx();
    let mut at = 0;
    let mut completed = Vec::new();
    while at < records.len() {
        let cmd = u16::from_le_bytes([records[at], records[at + 1]]);
        let word = |offset| u32::from_le_bytes(records[at + offset..at + offset + 4].try_into().unwrap());
        at += match cmd {
            5 => 8 + word(4) as usize * 8,
            9 | 12 => { if cmd == 9 { completed.extend(control(6, 2, word(4))); } (16 + word(8) as usize + 7) & !7 },
            _ => 8,
        };
    }
    assert!(!completed.is_empty());
    peer.push(&data(slots[1], &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::WaitingReply));
    slots.push(owner.offer_receive().unwrap()); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    for (slot, port) in slots[2..6].iter().zip(0x57..0x5b) {
        peer.push(&data(*slot, &tftp_qrtr(qcom_tftp_server_check::REQUEST, port, 0x5000)));
        owner.notified(8, &mut irqs).unwrap();
    }
    peer.push(&completed);
    peer.push(&data(slots[6], &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x60, 0x5000)));
    let mut queued = false;
    for _ in 0..8 {
        owner.notified(8, &mut irqs).unwrap();
        assert_eq!(owner.queue_full_kind(), 0);
        if owner.service_tftp_server_check_step(&mut server, &mut irqs) == Ok(TftpServiceProgress::ResponseQueued) {
            queued = true;
            break;
        }
    }
    assert!(queued, "the retained reply must get a turn when its transmit completes");
}

#[cfg(feature = "sunfish-native-tftp-server-check")]
#[test]
fn server_check_burst_yields_to_service_before_filling_held_queue() {
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut slots = Vec::new();
    for _ in 0..5 { slots.push(owner.offer_receive().unwrap()); owner.notified(8, &mut irqs).unwrap(); peer.take_tx(); }
    for intent in slots { peer.push(&data(intent, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000))); }
    // The runtime calls service after notified returns. It must get that turn
    // before a fifth held request overflows the four-entry owner queue.
    let progress = owner.notified(1, &mut irqs).unwrap();
    assert!(owner.pending_tftp_request().is_some());
    assert!(progress.deferred);
    assert_eq!(progress.acknowledged, 0);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
}

#[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
#[test]
fn new_tftp_request_yields_once_before_publication_without_starving_tx() {
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let first = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let second = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(first, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000)));
    peer.push(&data(second, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000)));
    let first_turn = owner.notified(1, &mut irqs).unwrap();
    assert!(first_turn.deferred);
    assert_eq!(first_turn.acknowledged, 0);
    assert_eq!(first_turn.published, 0);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
    let second_turn = owner.notified(8, &mut irqs).unwrap();
    assert!(second_turn.published > 0, "a second request must not postpone publication again");
}

#[cfg(feature = "sunfish-native-tftp-server-check")]
#[test]
fn server_check_counts_incoming_requests_without_assuming_the_first_rejection() {
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    for (packet, port) in [
        (qcom_tftp_server_check::REQUEST, 0x5000),
        (b"\0\x01/readonly/firmware/image/wlanmdsp.mbn\0octet\0" as &[u8], 0x5000),
        (b"\0\x03\0\x01example" as &[u8], 0x5001),
    ] {
        let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        peer.push(&data(slot, &tftp_qrtr(packet, 0x56, port)));
        owner.notified(1, &mut irqs).unwrap();
    }
    assert_eq!(owner.tftp_wire_counts(), [1, 1, 1, 1]);
    #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
    {
        let first = owner.first_rrq().unwrap();
        assert_eq!(first.path_text(), "/readonly/firmware/image/wlanmdsp.mbn");
        let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let second = b"\0\x01/readwrite/modem_efs\0octet\0";
        peer.push(&data(slot, &tftp_qrtr(second, 0x57, 0x5000)));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.first_rrq(), Some(first), "later requests cannot overwrite the first RRQ");
        assert_eq!(owner.tftp_wire_counts(), [1, 2, 1, 1]);
    }
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", not(feature = "sunfish-tftp-per-peer-reply-experiment")))]
#[test]
fn server_check_duplicate_burst_does_not_overflow_while_reply_waits_for_credit() {
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut slots = Vec::new();
    for _ in 0..6 { slots.push(owner.offer_receive().unwrap()); owner.notified(8, &mut irqs).unwrap(); peer.take_tx(); }
    for intent in slots {
        let mut packet = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000);
        packet[16..20].copy_from_slice(&0u32.to_le_bytes());
        peer.push(&data(intent, &packet));
    }
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    for iteration in 0..6 {
        owner.notified(if iteration == 0 { 1 } else { 8 }, &mut irqs).unwrap();
        owner.service_tftp_server_check_step(&mut server, &mut irqs).unwrap();
    }
    let (reason, wait_tx, _wait_flows, wait_peer) = owner.tftp_wait_diagnostic();
    assert_eq!(reason, 1, "a blocked reply records FlowBusy");
    assert!(wait_tx > 0);
    assert_eq!(wait_peer, 0x56);
    let seventh = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut packet = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000);
    packet[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(seventh, &packet));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.queue_full_kind(), 0);
    assert!(owner.tftp_wrq_coalesced() > 0);
    let coalesced = owner.tftp_wrq_coalesced();
    let other = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut different = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000);
    different[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(other, &different));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.tftp_wrq_coalesced(), coalesced);
    assert!(owner.pending_tftp_request().is_some());
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs),
        Ok(qcom_modem_owner::TftpServiceProgress::WaitingReply),
        "the previously retained reply is retried before another peer can be dispatched");
    assert!(owner.pending_tftp_request().is_some(),
        "the unrelated peer's request stays held behind that reply");
    let duplicate = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut same_other = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000);
    same_other[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(duplicate, &same_other));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.tftp_wrq_coalesced(), coalesced + 1, "a matching held request may absorb its retransmission");
    let altered = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut bad_request = qcom_tftp_server_check::REQUEST.to_vec();
    *bad_request.last_mut().unwrap() = 1;
    let mut packet = tftp_qrtr(&bad_request, 0x56, 0x5000);
    packet[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(altered, &packet));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.tftp_wrq_coalesced(), coalesced + 1);
}

#[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
#[test]
fn blocked_peer_does_not_block_another_server_check_peer() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    for (iteration, source) in [0x56, 0x56, 0x57].into_iter().enumerate() {
        let slot = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let body = if iteration == 0 {
            server_check_wrq_for_backpressure()
        } else {
            qcom_tftp_server_check::REQUEST.to_vec()
        };
        let mut packet = tftp_qrtr(&body, source, 0x5000);
        packet[16..20].copy_from_slice(&0u32.to_le_bytes());
        peer.push(&data(slot, &packet));
        owner.notified(1, &mut irqs).unwrap();
        let result = owner.service_tftp_server_check_step(&mut server, &mut irqs).unwrap();
        match iteration {
            0 => assert_eq!(result, TftpServiceProgress::ResponseQueued),
            1 => assert_eq!(result, TftpServiceProgress::WaitingReply),
            2 => assert_eq!(result, TftpServiceProgress::ResponseQueued, "a distinct peer may progress"),
            _ => unreachable!(),
        }
        if iteration == 1 {
            let duplicate = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            let mut packet = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000);
            packet[16..20].copy_from_slice(&0u32.to_le_bytes());
            let before = owner.tftp_wrq_coalesced();
            peer.push(&data(duplicate, &packet));
            owner.notified(1, &mut irqs).unwrap();
            assert_eq!(owner.tftp_wrq_coalesced(), before + 1, "only the exact retained peer request is coalesced");
        }
    }
    assert_eq!(server.active_count(), 2);
    assert!(owner.tftp_queue_snapshot().1, "the blocked first peer's reply is retained");
}

#[cfg(feature = "sunfish-tftp-timing-observer")]
#[test]
fn first_wrq_and_queued_oack_are_correlated_through_the_real_owner() {
    use qcom_qmi_router::Address;
    use std::sync::atomic::{AtomicU64, Ordering};
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 { CLOCK.load(Ordering::Relaxed) }
    let (peer, mut owner) = owner();
    owner.set_rmtfs_event_clock(clock);
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut packet = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000);
    packet[16..20].copy_from_slice(&0u32.to_le_bytes());
    CLOCK.store(100, Ordering::Relaxed);
    peer.push(&data(slot, &packet)); owner.notified(1, &mut irqs).unwrap();
    let t = owner.tftp_timing();
    assert_eq!(t.peer, Some(Address { node: 0, port: 0x56 }));
    assert_eq!(t.request, Some(100));
    CLOCK.store(150, Ordering::Relaxed);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
    let t = owner.tftp_timing();
    assert_eq!(t.queued, Some(150));
    assert!(!t.different_peer);
    assert_eq!(owner.tftp_endpoint_trace().unwrap().destination, t.peer.unwrap());
    CLOCK.store(180, Ordering::Relaxed);
    owner.notified(8, &mut irqs).unwrap();
    assert_eq!(owner.tftp_timing().published, Some(180));
    #[cfg(feature = "sunfish-tftp-tail-observer")]
    let sent = {
        assert_eq!(owner.tftp_timing().peer_tail, None);
        let prefix = {
            let mut memory = peer.0.borrow_mut();
            let start = memory.words[0] as usize;
            let outstanding = (memory.words[1] as usize + FIFO_BYTES - start) % FIFO_BYTES;
            assert!(outstanding > 8);
            let prefix: Vec<u8> = (0..8).map(|i| memory.tx[(start + i) % FIFO_BYTES]).collect();
            memory.words[0] = ((start + 8) % FIFO_BYTES) as u32;
            prefix
        };
        CLOCK.store(185, Ordering::Relaxed);
        owner.notified(8, &mut irqs).unwrap();
        assert_eq!(owner.tftp_timing().peer_tail, None, "partial FIFO consumption must not cross the OACK boundary");
        let mut records = prefix;
        records.extend(peer.take_tx());
        let packets = owner_datagrams(&records);
        CLOCK.store(190, Ordering::Relaxed);
        owner.notified(8, &mut irqs).unwrap();
        assert_eq!(owner.tftp_timing().peer_tail, Some(190));
        assert_eq!(owner.tftp_timing().done, None, "cursor consumption is not GLINK completion");
        CLOCK.store(200, Ordering::Relaxed);
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
        packets
    };
    #[cfg(not(feature = "sunfish-tftp-tail-observer"))]
    let sent = {
    CLOCK.store(200, Ordering::Relaxed);
        take_owner_datagrams(&peer, &mut owner, &mut irqs)
    };
    assert!(sent.iter().any(|packet| packet[32..].starts_with(qcom_tftp_server_check::OACK)));
    assert_eq!(owner.tftp_timing().done, Some(200));
    #[cfg(feature = "sunfish-tftp-peer-close-observer")]
    {
        for (port, now) in [(0x57, 201), (0x56, 202)] {
            let slot = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            CLOCK.store(now, Ordering::Relaxed);
            peer.push(&data(slot, &owner_control_frame(6, [0, port, 0, 0])));
            owner.notified(1, &mut irqs).unwrap();
        }
        let t = owner.tftp_timing();
        assert_eq!((t.peer_close, t.peer_close_phase), (Some(202), 7));
    }
    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
    {
        let endpoint = owner.tftp_endpoint_trace().unwrap();
        for (remote, local, now) in [
            (0x57, endpoint.source.port, 210),
            (0x56, endpoint.source.port + 1, 220),
            (0x56, endpoint.source.port, 230),
        ] {
            let slot = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap();
            peer.take_tx();
            CLOCK.store(now, Ordering::Relaxed);
            peer.push(&data(slot, &tftp_qrtr(b"\0\x03\0\x01example", remote, local)));
            owner.notified(1, &mut irqs).unwrap();
        }
        let t = owner.tftp_timing();
        assert_eq!((t.matching_data, t.first_matching_data), (1, Some(230)));
    }
}

#[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
#[test]
fn per_peer_retained_reply_capacity_is_bounded_and_holds_next_request() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 16);
    for id in 0u32..16 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    for source in 0x56..0x5a {
        for duplicate in [false, true] {
            let slot = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            let mut packet = tftp_qrtr(&server_check_wrq_for_backpressure(), source, 0x5000);
            packet[16..20].copy_from_slice(&0u32.to_le_bytes());
            peer.push(&data(slot, &packet)); owner.notified(1, &mut irqs).unwrap();
            let result = owner.service_tftp_server_check_step(&mut server, &mut irqs).unwrap();
            assert_eq!(result, if duplicate { TftpServiceProgress::WaitingReply } else { TftpServiceProgress::ResponseQueued });
        }
    }
    assert_eq!(server.active_count(), 4);
    let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut fifth = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x70, 0x5000);
    fifth[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(slot, &fifth)); owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(TftpServiceProgress::WaitingReply));
    assert!(owner.pending_tftp_request().is_some(), "full retained slots leave the fifth packet held");
    assert_eq!(server.active_count(), 4);
}

#[cfg(feature = "sunfish-native-tftp-server-check")]
#[test]
fn tftp_full_queue_snapshot_survives_stop_and_identifies_held_peers() {
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut slots = Vec::new();
    for _ in 0..5 { slots.push(owner.offer_receive().unwrap()); owner.notified(8, &mut irqs).unwrap(); peer.take_tx(); }
    for (index, intent) in slots.into_iter().enumerate() {
        let mut packet = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56 + index as u32, 0x5000);
        packet[16..20].copy_from_slice(&0u32.to_le_bytes());
        peer.push(&data(intent, &packet));
    }
    for iteration in 0..4 { owner.notified(if iteration == 0 { 1 } else { 8 }, &mut irqs).unwrap(); }
    assert_eq!(owner.notified(8, &mut irqs), Err(OwnerError::QueueFull));
    assert_eq!(owner.queue_full_kind(), 2);
    let (snapshot, pending) = owner.tftp_queue_snapshot();
    assert!(!pending); assert_eq!(snapshot.total, 5); assert_eq!(snapshot.tx_count, 0);
    let mut peers = snapshot.peers; peers.sort_unstable();
    assert_eq!(peers, [0x56, 0x57, 0x58, 0x59, 0x5a]);
    for metadata in snapshot.metadata {
        assert_eq!(metadata & 0xff, 2);
        assert_ne!(metadata & (1 << 8), 0);
        assert_ne!(metadata & (1 << 10), 0);
        assert_eq!(metadata >> 16, qcom_tftp_server_check::REQUEST.len() as u32);
    }
}

#[test]
fn owner_retains_generated_tftp_reply_across_transport_backpressure() {
    use qcom_modem_owner::TftpServiceProgress;
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};

    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x4300).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);

    let image = vec![0x5a; 600];
    let mut server = Server::new(
        1,
        0,
        0x4300,
        0x4400,
        &[],
        AuthorizedWlanImage::test_fixture(&image),
    )
    .unwrap();
    let rrq_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[]);
    peer.push(&data(rrq_intent, &tftp_qrtr(&rrq, 0x6000, 0x4300)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.service_tftp_step(&mut server, &mut irqs),
        Ok(TftpServiceProgress::ResponseQueued)
    );
    owner.notified(8, &mut irqs).unwrap();
    let first_records = peer.take_tx();
    let first = owner_complete_datagrams(&first_records)
        .into_iter()
        .find(|packet| owner_word(packet, 12) == 0x4400 && owner_word(packet, 28) == 0x6000)
        .unwrap();
    assert_eq!(&first[32..34], &qcom_tftp::OP_DATA.to_be_bytes());
    assert_eq!(first[36..].len(), 512);
    // Complete GLINK delivery but deliberately withhold QRTR RESUME_TX.
    acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &first_records);
    peer.take_tx();

    let ack_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&data(
        ack_intent,
        &tftp_qrtr(&[0, qcom_tftp::OP_ACK as u8, 0, 1], 0x6000, 0x4400),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.service_tftp_step(&mut server, &mut irqs),
        Ok(TftpServiceProgress::WaitingReply)
    );
    assert!(owner.pending_tftp_request().is_none());

    let credit_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    let credit: Vec<u8> = [1u32, 7, 0, 0x6000, 0, 20, 1, 0x4400, 7, 0, 0x6000, 0, 0]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    peer.push(&data(credit_intent, &credit));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.service_tftp_step(&mut server, &mut irqs),
        Ok(TftpServiceProgress::ResponseQueued)
    );
    owner.notified(8, &mut irqs).unwrap();
    let second = owner_complete_datagrams(&peer.take_tx())
        .into_iter()
        .find(|packet| owner_word(packet, 12) == 0x4400 && owner_word(packet, 28) == 0x6000)
        .unwrap();
    assert_eq!(&second[32..34], &qcom_tftp::OP_DATA.to_be_bytes());
    assert_eq!(&second[34..36], &[0, 2]);
    assert_eq!(second[36..].len(), 88);
}

fn wlfw_registration_response(confirm_rx: bool) -> Vec<u8> {
    let mut qmi = vec![2, 1, 0, 0x20, 0, 7, 0, 2, 4, 0, 0, 0, 0, 0];
    let mut datagram = [
        1u32,
        1,
        0,
        0x5002,
        u32::from(confirm_rx),
        qmi.len() as u32,
        1,
        0x4100,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect::<Vec<_>>();
    datagram.append(&mut qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn ipa_init_response(confirm_rx: bool) -> Vec<u8> {
    let qmi = [2, 1, 0, 0x21, 0, 7, 0, 2, 4, 0, 0, 0, 0, 0];
    let mut datagram = [
        1u32,
        1,
        0,
        0x5009,
        u32::from(confirm_rx),
        qmi.len() as u32,
        1,
        0x4101,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect::<Vec<_>>();
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn wlfw_msa_info_response(confirm_rx: bool) -> Vec<u8> {
    let mut qmi = vec![2, 2, 0, 0x2d, 0, 37, 0];
    qmi.extend_from_slice(&[2, 4, 0, 0, 0, 0, 0]);
    qmi.extend_from_slice(&[3, 27, 0, 2]);
    for (address, size, secure) in [
        (0x98d00000u64, 0x4000u32, 1u8),
        (0x98d04000u64, 0x1fc000u32, 0u8),
    ] {
        qmi.extend_from_slice(&address.to_le_bytes());
        qmi.extend_from_slice(&size.to_le_bytes());
        qmi.push(secure);
    }
    let mut datagram = [
        1u32,
        1,
        0,
        0x5002,
        u32::from(confirm_rx),
        qmi.len() as u32,
        1,
        0x4100,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect::<Vec<_>>();
    datagram.append(&mut qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn wlfw_status_response(
    transaction_id: u8,
    message_id: u16,
    confirm_rx: bool,
    optional: &[u8],
) -> Vec<u8> {
    let mut qmi = vec![2, transaction_id, 0];
    qmi.extend_from_slice(&message_id.to_le_bytes());
    let mut payload = vec![2, 4, 0, 0, 0, 0, 0];
    payload.extend_from_slice(optional);
    qmi.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    qmi.extend_from_slice(&payload);
    let mut datagram = [
        1u32,
        1,
        0,
        0x5002,
        u32::from(confirm_rx),
        qmi.len() as u32,
        1,
        0x4100,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect::<Vec<_>>();
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn wlfw_startup_indication(message_id: u16, confirm_rx: bool) -> Vec<u8> {
    let mut qmi = vec![4, 0, 0];
    qmi.extend_from_slice(&message_id.to_le_bytes());
    qmi.extend_from_slice(&[0, 0]);
    let mut datagram = [
        1u32,
        1,
        0,
        0x5002,
        u32::from(confirm_rx),
        qmi.len() as u32,
        1,
        0x4100,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect::<Vec<_>>();
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn servreg_query_datagram_for(service_name: &[u8]) -> Vec<u8> {
    servreg_query_datagram_with_transaction(service_name, 0x1234)
}

fn servreg_query_datagram_with_transaction(service_name: &[u8], transaction: u16) -> Vec<u8> {
    let mut qmi = vec![
        0,
        transaction as u8,
        (transaction >> 8) as u8,
        0x21,
        0,
        (service_name.len() + 3) as u8,
        0,
        1,
        service_name.len() as u8,
        0,
    ];
    qmi.extend_from_slice(service_name);
    let mut datagram = [
        1u32,
        1,
        0,
        9,
        0,
        qmi.len() as u32,
        1,
        crate::qcom_servreg::LOCAL_PORT,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect::<Vec<_>>();
    datagram.append(&mut qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn servreg_query_datagram() -> Vec<u8> {
    servreg_query_datagram_for(b"tms/servreg")
}

#[test]
fn owner_announces_servreg_and_returns_bounded_domain_map_through_fifo() {
    use crate::qcom_modem_owner::ServiceProgress;

    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let announcements = owner_datagrams(&peer.take_tx());
    assert_eq!(announcements.len(), 1);
    assert_eq!(
        &announcements[0][32..],
        &[4u32, 64, 257, 1, crate::qcom_servreg::LOCAL_PORT]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>()
    );

    peer.push(&data(2, &servreg_query_datagram()));
    owner.notified(1, &mut irqs).unwrap();
    assert!(owner.pending_request().is_some());
    let trace = owner.service_registry_trace();
    assert_eq!(trace.requests, 1);
    assert_eq!(
        trace.name_counts[crate::qcom_servreg::NAME_COUNTER_TMS_SERVREG],
        1
    );
    assert_eq!(trace.replies_queued, 0);
    assert_eq!(
        trace.first_reply_name_code,
        crate::qcom_servreg::NAME_UNKNOWN
    );
    assert_eq!(trace.reply_waits, 0);
    assert_eq!(trace.last_name_code, crate::qcom_servreg::NAME_TMS_SERVREG);
    assert_eq!(trace.last_domain_offset, 0);
    let full_topology = cfg!(feature = "sunfish-servreg-sm7150-topology");
    let domains = if full_topology { 6 } else { 2 };
    assert_eq!(trace.last_total_domains, u32::from(domains));
    assert_eq!(trace.last_service_name_parts(), ("TMS SERVREG", ""));
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::ReplyQueued
    );
    let reply_trace = owner.service_registry_trace();
    assert_eq!(reply_trace.replies_queued, 1);
    assert_eq!(
        reply_trace.first_reply_name_code,
        crate::qcom_servreg::NAME_TMS_SERVREG
    );
    assert!(!reply_trace.first_reply_glink_rx_done);
    owner.notified(8, &mut irqs).unwrap();

    let replies = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert!(
        owner.service_registry_trace().first_reply_glink_rx_done,
        "peer RX_DONE should be attributed to the first queued SERVREG reply"
    );
    assert_eq!(replies.len(), 1);
    let reply = &replies[0];
    assert_eq!(u32::from_le_bytes(reply[4..8].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(reply[8..12].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(reply[12..16].try_into().unwrap()),
        crate::qcom_servreg::LOCAL_PORT
    );
    assert_eq!(u32::from_le_bytes(reply[24..28].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(reply[28..32].try_into().unwrap()), 9);
    let row_size: u16 = if full_topology {
        assert_eq!(reply.len(), 224);
        162
    } else if cfg!(feature = "sunfish-servreg-linux-string-length") {
        55
    } else {
        57
    };
    let payload_size = (row_size + 20).to_le_bytes();
    assert_eq!(
        &reply[32..39],
        &[2, 0x34, 0x12, 0x21, 0, payload_size[0], payload_size[1]]
    );
    assert_eq!(&reply[39..46], &[2, 4, 0, 0, 0, 0, 0]);
    assert_eq!(&reply[46..51], &[0x10, 2, 0, domains, 0]);
    assert_eq!(&reply[51..56], &[0x11, 2, 0, 1, 0]);
    assert_eq!(reply[56], 0x12);
    assert_eq!(u16::from_le_bytes(reply[57..59].try_into().unwrap()), row_size);
    assert_eq!(reply[59], domains);
    assert!(
        reply
            .windows(b"msm/modem/root_pd".len())
            .any(|w| w == b"msm/modem/root_pd")
    );
    assert!(
        reply
            .windows(b"msm/modem/wlan_pd".len())
            .any(|w| w == b"msm/modem/wlan_pd")
    );
    assert!(owner.pending_request().is_none());
    #[cfg(feature = "sunfish-wlan-startup-trace")]
    {
        #[cfg(feature = "sunfish-wlan-elf-timing")]
        owner.set_rmtfs_event_clock(|| {
            static CLOCK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(100);
            CLOCK.fetch_add(10, core::sync::atomic::Ordering::Relaxed)
        });
        let root = 1 << crate::qcom_servreg::NAME_COUNTER_TMS_SERVREG;
        let loader = 1 << crate::qcom_servreg::NAME_COUNTER_ELF_LOADER;
        assert_eq!(owner.service_registry_trace().completed_names, root);
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        peer.push(&data(intent, &servreg_query_datagram_with_transaction(b"kernel/elf_loader", 0x1235)));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.dispatch_service_registry_step(&mut irqs).unwrap(), ServiceProgress::ReplyQueued);
        assert_eq!(owner.service_registry_trace().queued_names, root | loader);
        assert_eq!(owner.service_registry_trace().completed_names, root);
        owner.notified(8, &mut irqs).unwrap();
        let replies = take_owner_datagrams(&peer, &mut owner, &mut irqs);
        assert_eq!(replies.len(), 1);
        assert_eq!(owner.service_registry_trace().completed_names, root | loader);
        #[cfg(feature = "sunfish-wlan-elf-timing")]
        {
            let trace = owner.service_registry_trace().elf.unwrap();
            assert_eq!((trace.peer_port, trace.domain_offset), (9, 0));
            assert_eq!((trace.total_domains, trace.returned_rows), (Some(1), Some(1)));
            #[cfg(not(feature = "sunfish-wlan-elf-dispatch-timing"))]
            assert_eq!((trace.received, trace.queued, trace.published, trace.done),
                       (Some(100), Some(110), Some(120), Some(130)));
            #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
            {
                // Optional observation clock calls advance this synthetic counter;
                // neither clock values nor this trace participate in protocol policy.
                let clocks = if cfg!(feature="sunfish-registry-receipt-fifo") {
                    // One arm sample and active ELF FIFO interval observations.
                    (Some(110), Some(160), Some(210), Some(390))
                } else if cfg!(feature="sunfish-elf-writer-trace") {
                    // Exact writer start/end add two optional clock observations.
                    (Some(110), Some(160), Some(200), Some(240))
                } else if cfg!(feature="sunfish-elf-window-trace") {
                    (Some(110), Some(160), Some(180), Some(220))
                } else if cfg!(feature="sunfish-post-elf-mcfg-trace") {
                    (Some(110), Some(150), Some(170), Some(200))
                } else {(Some(100), Some(140), Some(150), Some(160))};
                assert_eq!((trace.received, trace.queued, trace.published, trace.done), clocks);
                let extra = u64::from(cfg!(feature="sunfish-post-elf-mcfg-trace"))*10;
                assert_eq!(trace.dispatch, qcom_modem_owner::ElfDispatchTiming {
                    receive_pass_end: Some(110+extra),
                    registry_entered: Some(120+extra),
                    selected: Some(130+extra),
                    #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
                    turns: trace.dispatch.turns,
                });
                #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
                assert_eq!(trace.dispatch.turns.calls, 0);
            }
            let qmi = &replies[0][32..32 + owner_word(&replies[0], 20) as usize];
            let row = [
                &[1, 17][..], b"msm/modem/wlan_pd",
                &180u32.to_le_bytes(), &[0], &[0; 4],
            ].concat();
            assert_eq!(&qmi[7..20], &[2, 4, 0, 0, 0, 0, 0, 0x10, 2, 0, 1, 0, 0x11]);
            assert_eq!(&qmi[20..27], &[2, 0, 1, 0, 0x12, row.len() as u8, 0]);
            assert_eq!(&qmi[27..], row);
        }
    }
}

#[test]
#[cfg(not(feature = "sunfish-qrtr-servreg-flow-experiment"))]
fn owner_traces_servreg_qrtr_resume_and_splits_reply_wait_reasons() {
    use crate::qcom_modem_owner::ServiceProgress;

    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();

    peer.push(&data(2, &servreg_query_datagram_for(b"tms/pdr_enabled")));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::ReplyQueued
    );
    assert_eq!(
        owner.service_registry_trace().replies_queued,
        1,
        "first LOC reply should queue before the second request"
    );
    owner.notified(8, &mut irqs).unwrap();
    let replies = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(replies.len(), 1);
    assert!(owner.service_registry_trace().first_reply_glink_rx_done);
    assert!(!owner.service_registry_trace().first_reply_qrtr_resume);

    peer.push(&data(
        3,
        &servreg_query_datagram_with_transaction(b"tms/pddump_disabled", 0x1235),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::WaitingReply
    );
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::WaitingReply
    );
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::WaitingReply
    );
    let blocked = owner.service_registry_trace();
    assert_eq!(blocked.requests, 2);
    assert_eq!(blocked.replies_queued, 1);
    assert_eq!(blocked.reply_waits, 3);
    assert_eq!(blocked.reply_flow_busy, 3);
    assert_eq!(blocked.reply_no_capacity, 0);
    assert_eq!(
        blocked.reply_wait_name_counts[crate::qcom_servreg::NAME_COUNTER_TMS_PDR_ENABLED],
        0,
        "the queued pdr_enabled reply must not receive later wait attribution"
    );
    assert_eq!(
        blocked.reply_wait_name_counts[crate::qcom_servreg::NAME_COUNTER_TMS_PDDUMP_DISABLED],
        3,
        "each backpressured pddump_disabled reply attempt must be attributed"
    );
    assert_eq!(
        blocked.reply_wait_name_counts[crate::qcom_servreg::NAME_COUNTER_OTHER],
        0
    );
    assert!(!blocked.first_reply_qrtr_resume);

    let credit_id = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    let credit_body: Vec<u8> = [7u32, 0, 9, 0, 0]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    peer.push(&data(
        credit_id,
        &owner_qrtr_packet(7, 9, crate::qcom_servreg::LOCAL_PORT, false, &credit_body),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert!(owner.service_registry_trace().first_reply_qrtr_resume);
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::ReplyQueued
    );
    let resumed = owner.service_registry_trace();
    assert!(resumed.first_reply_qrtr_resume);
    assert_eq!(resumed.replies_queued, 2);
    assert_eq!(resumed.reply_waits, 3);
    assert_eq!(resumed.reply_flow_busy, 3);
    assert_eq!(resumed.reply_no_capacity, 0);
    assert_eq!(
        resumed.reply_wait_name_counts[crate::qcom_servreg::NAME_COUNTER_TMS_PDDUMP_DISABLED],
        3,
        "wait attribution remains cumulative after the reply is eventually queued"
    );
}

#[test]
fn owner_service_dispatchers_select_their_request_from_a_mixed_queue() {
    use crate::qcom_modem_owner::ServiceProgress;

    const IPA_SERVER_PORT: u32 = 0x4102;
    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    owner.prepare_ipa_server_endpoint(IPA_SERVER_PORT).unwrap();
    owner.enable_service_registry(&mut irqs).unwrap();
    owner
        .enable_ipa_server(
            &crate::qcom_ipa_service::ApReadyPermit::test_after_post_init(1),
            &mut irqs,
        )
        .unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();

    let ipa_request = vec![0, 0x34, 0x12, 0x20, 0, 4, 0, 0x10, 1, 0, 1];
    let ipa_datagram = owner_qrtr_packet(1, 27, IPA_SERVER_PORT, false, &ipa_request);
    peer.push(&data(2, &ipa_datagram));
    owner.notified(1, &mut irqs).unwrap();
    peer.push(&data(3, &servreg_query_datagram_for(b"tms/wlan_pd")));
    owner.notified(1, &mut irqs).unwrap();

    let ipa_handle = owner
        .pending_request_for_service(Service::IpaServer)
        .unwrap()
        .unwrap();
    let servreg_handle = owner
        .pending_request_for_service(Service::ServiceRegistry)
        .unwrap()
        .unwrap();
    assert_eq!(owner.pending_request(), Some(ipa_handle));
    assert_ne!(ipa_handle, servreg_handle);

    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::ReplyQueued,
        "SERVREG must progress even when IPA is the oldest queued request"
    );
    let (context, request) = owner.request_qmi(&ipa_handle).unwrap();
    assert_eq!(context.service, Service::IpaServer);
    assert_eq!(request, ipa_request);
}

#[test]
fn owner_preserves_unknown_servreg_name_in_trace_and_queues_empty_map_reply() {
    use crate::qcom_modem_owner::ServiceProgress;

    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();

    peer.push(&data(2, &servreg_query_datagram_for(b"unmapped/loc_probe")));
    owner.notified(1, &mut irqs).unwrap();

    let trace = owner.service_registry_trace();
    assert_eq!(trace.requests, 1);
    assert_eq!(
        trace.name_counts[crate::qcom_servreg::NAME_COUNTER_OTHER],
        1
    );
    assert_eq!(trace.last_name_code, crate::qcom_servreg::NAME_UNKNOWN);
    assert_eq!(trace.last_service_name_parts(), ("UNMAPPED LOC PROBE", ""));
    assert_eq!(trace.last_total_domains, 0);
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::ReplyQueued
    );
    let reply_trace = owner.service_registry_trace();
    assert_eq!(reply_trace.replies_queued, 1);
    assert_eq!(
        reply_trace.first_reply_name_code,
        crate::qcom_servreg::NAME_UNKNOWN
    );
    owner.notified(8, &mut irqs).unwrap();
    let replies = owner_datagrams(&peer.take_tx());
    assert_eq!(replies.len(), 1);
    assert_eq!(
        &replies[0][32..56],
        &[
            2, 0x34, 0x12, 0x21, 0, 17, 0, 2, 4, 0, 0, 0, 0, 0, 0x10, 2, 0, 0, 0, 0x11, 2, 0, 1, 0,
        ]
    );
    assert!(owner.pending_request().is_none());
}

#[test]
fn owner_answers_observed_pddump_disabled_lookup_with_empty_map_reply() {
    use crate::qcom_modem_owner::ServiceProgress;

    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();

    peer.push(&data(
        2,
        &servreg_query_datagram_for(b"tms/pddump_disabled"),
    ));
    owner.notified(1, &mut irqs).unwrap();

    let trace = owner.service_registry_trace();
    assert_eq!(trace.requests, 1);
    assert_eq!(
        trace.name_counts[crate::qcom_servreg::NAME_COUNTER_TMS_PDDUMP_DISABLED],
        1
    );
    assert_eq!(
        trace.last_name_code,
        crate::qcom_servreg::NAME_TMS_PDDUMP_DISABLED
    );
    assert_eq!(trace.last_service_name_parts(), ("TMS PDDUMP DISABLED", ""));
    assert_eq!(trace.last_total_domains, 0);
    assert_eq!(
        owner.dispatch_service_registry_step(&mut irqs).unwrap(),
        ServiceProgress::ReplyQueued
    );
    let reply_trace = owner.service_registry_trace();
    assert_eq!(reply_trace.replies_queued, 1);
    assert_eq!(
        reply_trace.first_reply_name_code,
        crate::qcom_servreg::NAME_TMS_PDDUMP_DISABLED
    );
    owner.notified(8, &mut irqs).unwrap();
    let replies = owner_datagrams(&peer.take_tx());
    assert_eq!(replies.len(), 1);
    assert_eq!(
        &replies[0][32..56],
        &[
            2, 0x34, 0x12, 0x21, 0, 17, 0, 2, 4, 0, 0, 0, 0, 0, 0x10, 2, 0, 0, 0, 0x11, 2, 0, 1, 0,
        ]
    );
    assert!(owner.pending_request().is_none());
}

#[test]
fn native_owner_routes_wlfw_registration_through_fifo_and_stops_at_msa_gate() {
    use crate::qcom_wlfw_client::{RegistrationOutcome, State};

    let (peer, mut owner) = owner();
    owner.enable_wlfw_client(0x4100).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::Searching));
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    peer.push(&data(2, &owner_control_frame(4, [0x45, 1, 0, 0x5002])));
    owner.notified(1, &mut irqs).unwrap();
    let registration = owner_datagrams(&peer.take_tx());
    assert_eq!(registration.len(), 1);
    assert_eq!(
        &registration[0][32 + 7 + 19..32 + 7 + 26],
        &[0x19, 4, 0, 1, 0, 0, 0],
        "native ICNSS registration subscribes to FW rejuvenation"
    );
    assert_eq!(
        u32::from_le_bytes(registration[0][12..16].try_into().unwrap()),
        0x4100
    );
    assert_eq!(
        u32::from_le_bytes(registration[0][28..32].try_into().unwrap()),
        0x5002
    );
    assert_eq!(owner.wlfw_state(), Some(State::RegistrationPending));

    peer.push(&data(3, &wlfw_registration_response(false)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::MsaInfoReady));
    assert_eq!(
        owner.wlfw_outcome(),
        Some(RegistrationOutcome::Accepted {
            firmware_status: None,
            confirm_rx: false,
        })
    );
    owner.notified(8, &mut irqs).unwrap();
    assert!(owner_datagrams(&peer.take_tx()).is_empty());
}

#[test]
fn native_owner_routes_explicit_ipa_init_through_fifo_and_correlates_response() {
    use crate::qcom_ipa_client::State;
    use crate::sunfish_ipa_profile::{self, APPS_WAN_CONS_ENDPOINT, RuntimeSnapshot};

    let (peer, mut owner) = owner();
    owner.enable_ipa_client(0x4101).unwrap();
    assert_eq!(owner.ipa_state(), Some(State::Searching));
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    // Discovery only binds the exact modem IPA service; it does not send the
    // request until the owner explicitly receives runtime-profile fields.
    peer.push(&data(2, &owner_control_frame(4, [0x31, 0x201, 0, 0x5009])));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.ipa_state(), Some(State::Ready));
    assert!(owner_datagrams(&peer.take_tx()).is_empty());

    let fields = sunfish_ipa_profile::init_request_fields(RuntimeSnapshot {
        shared_mem_baddr_units: 0x10,
        shared_mem_size_units: 0x400,
        apps_wan_cons_endpoint: Some(APPS_WAN_CONS_ENDPOINT),
        ipa_uc_loaded: false,
    })
    .unwrap();
    owner
        .begin_ipa_init_modem_driver(&fields, &mut irqs)
        .unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let requests = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(requests.len(), 1);
    assert_eq!(owner_word(&requests[0], 8), 1);
    assert_eq!(owner_word(&requests[0], 12), 0x4101);
    assert_eq!(owner_word(&requests[0], 24), 0);
    assert_eq!(owner_word(&requests[0], 28), 0x5009);
    assert_eq!(requests[0][32], 0); // QMI request.
    assert_eq!(
        u16::from_le_bytes(requests[0][35..37].try_into().unwrap()),
        crate::qcom_ipa::INIT_MODEM_DRIVER_MESSAGE_ID
    );
    assert_eq!(owner.ipa_state(), Some(State::Pending));

    peer.push(&data(3, &ipa_init_response(true)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.ipa_state(), Some(State::ResponseReceived));
    let outcomes = owner.ipa_outcome().unwrap();
    assert_eq!(outcomes.response.result, 0);
    assert_eq!(outcomes.response.error, 0);
    assert!(outcomes.confirm_rx);
    let confirms = owner_datagrams(&peer.take_tx());
    assert_eq!(confirms.len(), 1);
    assert_eq!(owner_word(&confirms[0], 32), 7); // QRTR RESUME_TX.
}

#[test]
fn native_owner_registers_ap_ipa_without_early_announcement_and_commits_after_transport() {
    const IPA_SERVER_PORT: u32 = 0x4102;
    let (peer, mut owner) = owner();
    owner.prepare_ipa_server_endpoint(IPA_SERVER_PORT).unwrap();
    assert_eq!(owner.control_status().ready_services, 0);
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    assert_eq!(owner.control_status().announced_services, 0);

    assert_eq!(
        owner.set_service_available(Service::IpaServer, true, &mut irqs),
        Err(OwnerError::IpaReadinessRequired),
        "generic service publication cannot bypass the IPA readiness gate"
    );
    let stale_readiness = crate::qcom_ipa_service::ApReadyPermit::test_after_post_init(2);
    assert_eq!(
        owner.enable_ipa_server(&stale_readiness, &mut irqs),
        Err(OwnerError::IpaReadinessRequired),
        "a readiness permit cannot cross owner generations"
    );
    let readiness = crate::qcom_ipa_service::ApReadyPermit::test_after_post_init(1);
    owner.enable_ipa_server(&readiness, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let advertised = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert!(advertised.iter().any(|datagram| {
        owner_word(datagram, 4) == 4
            && owner_word(datagram, 36) == crate::qcom_ipa_service::SERVICE_ID
            && owner_word(datagram, 40) == crate::qcom_ipa_service::QRTR_VERSION_INSTANCE
            && owner_word(datagram, 48) == IPA_SERVER_PORT
    }));

    // Bind the modem endpoint from its QRTR service announcement.
    let server_announcement = owner_control_frame(4, [0x31, 0x201, 0, 27]);
    let announce_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&data(announce_intent, &server_announcement));
    owner.notified(1, &mut irqs).unwrap();

    let mut request = vec![0];
    request.extend(0x1234u16.to_le_bytes());
    request.extend(0x0020u16.to_le_bytes());
    request.extend([4, 0, 0x10, 1, 0, 1]); // indication-register=true
    let request_datagram = owner_qrtr_packet(1, 27, IPA_SERVER_PORT, false, &request);
    let request_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&data(request_intent, &request_datagram));
    owner.notified(1, &mut irqs).unwrap();

    let handle = owner.pending_request().unwrap();
    let (context, request_bytes) = owner.request_qmi(&handle).unwrap();
    assert_eq!(context.service, Service::IpaServer);
    assert_eq!(request_bytes, request);
    let mut server = crate::qcom_ipa_service::Server::new(context.peer, 1).unwrap();
    let prepared = server
        .prepare_response(
            1,
            context.peer,
            request_bytes,
            crate::qcom_ipa_service::QMI_MESSAGE_BYTES,
        )
        .unwrap();
    owner
        .queue_ipa_service_reply(&handle, &prepared, &mut irqs)
        .unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let responses = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert!(responses.iter().any(|datagram| {
        datagram.len() >= 32 + crate::qcom_ipa_service::QMI_MESSAGE_BYTES
            && owner_word(datagram, 4) == 1
            && u16::from_le_bytes(datagram[35..37].try_into().unwrap()) == 0x0020
    }));
    assert_eq!(server.registration(), None);

    let mut resume_body = vec![0; 20];
    resume_body[0..4].copy_from_slice(&7u32.to_le_bytes());
    resume_body[8..12].copy_from_slice(&27u32.to_le_bytes());
    let resume = owner_qrtr_packet(7, 27, IPA_SERVER_PORT, false, &resume_body);
    let resume_intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&data(resume_intent, &resume));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.take_ipa_service_response_completion().unwrap(),
        Some(prepared.token())
    );
    server.commit_response(1, &prepared).unwrap();
    assert_eq!(
        server.registration().unwrap().master_driver_init_complete,
        Some(true)
    );
}

#[test]
fn native_owner_sends_qrtr_resume_tx_for_confirmed_wlfw_response() {
    use crate::qcom_wlfw_client::{RegistrationOutcome, State};

    let (peer, mut owner) = owner();
    owner.enable_wlfw_client(0x4100).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    peer.push(&data(2, &owner_control_frame(4, [0x45, 1, 0, 0x5002])));
    owner.notified(1, &mut irqs).unwrap();
    peer.take_tx();
    assert_eq!(owner.wlfw_state(), Some(State::RegistrationPending));

    peer.push(&data(3, &wlfw_registration_response(true)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::MsaInfoReady));
    assert_eq!(
        owner.wlfw_outcome(),
        Some(RegistrationOutcome::Accepted {
            firmware_status: None,
            confirm_rx: true,
        })
    );

    let datagrams = owner_datagrams(&peer.take_tx());
    let resume = datagrams
        .iter()
        .find(|bytes| u32::from_le_bytes(bytes[4..8].try_into().unwrap()) == 7)
        .expect("QRTR RESUME_TX response");
    assert_eq!(resume.len(), 52);
    assert_eq!(u32::from_le_bytes(resume[8..12].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(resume[12..16].try_into().unwrap()),
        0x4100
    );
    assert_eq!(u32::from_le_bytes(resume[16..20].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(resume[24..28].try_into().unwrap()), 0);
    assert_eq!(
        u32::from_le_bytes(resume[28..32].try_into().unwrap()),
        0x5002
    );
    assert_eq!(u32::from_le_bytes(resume[32..36].try_into().unwrap()), 7);
    assert_eq!(u32::from_le_bytes(resume[36..40].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(resume[40..44].try_into().unwrap()),
        0x4100
    );
    assert_eq!(&resume[44..], &[0; 8]);
    owner.notified(8, &mut irqs).unwrap();
    assert!(owner_datagrams(&peer.take_tx()).is_empty());
}

#[test]
fn native_owner_routes_msa_info_ready_and_capability_through_fifo_offline() {
    use crate::qcom_wlfw_client::State;

    let (peer, mut owner) = owner();
    owner.enable_wlfw_client(0x4100).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();

    peer.push(&data(2, &owner_control_frame(4, [0x45, 1, 0, 0x5002])));
    owner.notified(1, &mut irqs).unwrap();
    let registration = owner_datagrams(&peer.take_tx());
    assert_eq!(registration.len(), 1);
    peer.push(&data(3, &wlfw_registration_response(false)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::MsaInfoReady));
    assert!(take_owner_datagrams(&peer, &mut owner, &mut irqs).is_empty()); // Registration RX completion.

    let reservation = crate::qcom_msa::Reservation::validate(
        1,
        99,
        crate::qcom_msa::SUNFISH_MSA_BASE,
        crate::qcom_msa::SUNFISH_MSA_BYTES,
        true,
        true,
        &[],
    )
    .unwrap();
    owner.begin_wlfw_msa_info(reservation, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let requests = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.len(), 60);
    assert_eq!(u32::from_le_bytes(request[20..24].try_into().unwrap()), 25);
    assert_eq!(
        u16::from_le_bytes(request[35..37].try_into().unwrap()),
        0x002d
    );
    assert_eq!(
        u64::from_le_bytes(request[42..50].try_into().unwrap()),
        crate::qcom_msa::SUNFISH_MSA_BASE
    );
    assert_eq!(
        u32::from_le_bytes(request[53..57].try_into().unwrap()),
        crate::qcom_msa::SUNFISH_MSA_BYTES
    );
    assert_eq!(owner.wlfw_state(), Some(State::MsaInfoPending));

    peer.push(&data(4, &wlfw_msa_info_response(true)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::MsaOwnershipRequired));
    let outcome = owner.wlfw_msa_info_outcome().unwrap();
    assert_eq!(outcome.result, 0);
    assert_eq!(outcome.region_count, 2);
    assert!(outcome.confirm_rx);
    assert_eq!(outcome.regions[0].address, 0x98d00000);
    assert_eq!(outcome.regions[0].size, 0x4000);
    assert!(outcome.regions[0].secure);
    assert_eq!(outcome.regions[1].address, 0x98d04000);
    assert_eq!(outcome.regions[1].size, 0x1fc000);
    assert!(!outcome.regions[1].secure);
    let plan = outcome.handoff_plan.unwrap();
    assert_eq!((plan.generation(), plan.identity()), (1, 99));
    let transfers = plan.transfers();
    assert_eq!(
        transfers[0].region(),
        crate::qcom_msa::Range {
            base: 0x98d00000,
            bytes: 0x4000
        }
    );
    assert_eq!(transfers[0].source_vmids(), [crate::qcom_msa::VMID_HLOS]);
    assert_eq!(transfers[0].destination_count(), 2);
    assert_eq!(
        transfers[1].region(),
        crate::qcom_msa::Range {
            base: 0x98d04000,
            bytes: 0x1fc000
        }
    );
    assert_eq!(transfers[1].destination_count(), 3);

    let datagrams = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let resume = datagrams
        .iter()
        .find(|bytes| u32::from_le_bytes(bytes[4..8].try_into().unwrap()) == 7)
        .expect("QRTR RESUME_TX response");
    assert_eq!(
        u32::from_le_bytes(resume[40..44].try_into().unwrap()),
        0x4100
    );
    owner.notified(8, &mut irqs).unwrap();
    assert!(take_owner_datagrams(&peer, &mut owner, &mut irqs).is_empty());

    let mut permit = crate::qcom_msa::MsaReadyPermit::from_verified_handoff(1, 99);
    owner.begin_wlfw_msa_ready(&mut permit, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let requests = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].len(), 40);
    assert_eq!(
        u32::from_le_bytes(requests[0][20..24].try_into().unwrap()),
        7
    );
    assert_eq!(
        u16::from_le_bytes(requests[0][35..37].try_into().unwrap()),
        0x002e
    );
    assert_eq!(owner.wlfw_state(), Some(State::MsaReadyPending));

    peer.push(&data(5, &wlfw_status_response(3, 0x002e, true, &[])));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::CapabilityReady));
    owner.notified(8, &mut irqs).unwrap();
    let credit = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(credit.len(), 1);
    assert_eq!(u32::from_le_bytes(credit[0][4..8].try_into().unwrap()), 7);
    owner.notified(8, &mut irqs).unwrap();
    assert!(take_owner_datagrams(&peer, &mut owner, &mut irqs).is_empty());

    peer.push(&data(
        6,
        &wlfw_startup_indication(crate::wlfw_protocol::WLFW_MSA_READY_IND_MESSAGE_ID, true),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(
        owner.wlfw_last_indication(),
        Some(crate::qcom_wlfw_client::IndicationOutcome {
            indication: crate::wlfw_protocol::WlfwIndication::MsaReady,
            confirm_rx: true,
        })
    );
    assert_eq!(owner.wlfw_state(), Some(State::CapabilityReady));
    owner.notified(8, &mut irqs).unwrap();
    let resume = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(resume.len(), 1);
    assert_eq!(u32::from_le_bytes(resume[0][4..8].try_into().unwrap()), 7);
    owner.notified(8, &mut irqs).unwrap();
    assert!(take_owner_datagrams(&peer, &mut owner, &mut irqs).is_empty());

    owner.begin_wlfw_capability_request(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let requests = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].len(), 40);
    assert_eq!(
        u16::from_le_bytes(requests[0][35..37].try_into().unwrap()),
        0x0024
    );
    assert_eq!(owner.wlfw_state(), Some(State::CapabilityPending));

    // Keep a fresh peer-intent batch available for the source-ordered
    // best-effort ICNSS feature-mask request that follows CAP.
    let mut refreshed_intents = control(5, 2, 24);
    for id in 8u32..32 {
        refreshed_intents.extend(4096u32.to_le_bytes());
        refreshed_intents.extend(id.to_le_bytes());
    }
    peer.push(&refreshed_intents);
    owner.notified(1, &mut irqs).unwrap();
    assert!(owner_datagrams(&peer.take_tx()).is_empty());

    peer.push(&data(
        7,
        &wlfw_status_response(
            4,
            0x0024,
            false,
            &[
                0x10, 8, 0, 0x34, 0x12, 0, 0, 0x78, 0x56, 0, 0, 0x11, 4, 0, 0x02, 0x12, 0, 0,
            ],
        ),
    ));
    owner.notified(1, &mut irqs).unwrap();
    let capabilities = owner.wlfw_capability_summary().unwrap();
    assert_eq!(capabilities.result, 0);
    assert_eq!(capabilities.chip_info, Some((0x1234, 0x5678)));
    assert_eq!(capabilities.board_id, Some(0x1202));
    assert!(!capabilities.confirm_rx);
    assert_eq!(owner.wlfw_state(), Some(State::DynamicFeatureMaskPending));
    owner.notified(8, &mut irqs).unwrap();
    let dynamic_records = peer.take_tx();
    let dynamic_mask = owner_datagrams(&dynamic_records);
    assert_eq!(dynamic_mask.len(), 1, "records={dynamic_records:02x?}");
    acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &dynamic_records);
    assert_eq!(
        u16::from_le_bytes(dynamic_mask[0][35..37].try_into().unwrap()),
        crate::wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID
    );
    assert_eq!(
        &dynamic_mask[0][42..50],
        &1u64.to_le_bytes(),
        "request={:02x?}",
        dynamic_mask[0]
    );
    assert_eq!(owner.wlfw_state(), Some(State::DynamicFeatureMaskPending));

    peer.push(&data(
        8,
        &wlfw_status_response(
            5,
            crate::wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
            false,
            &[
                0x10, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x11, 8, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert!(owner.wlfw_dynamic_feature_mask_done());
    assert_eq!(owner.wlfw_state(), Some(State::CapabilitiesAccepted));
    owner.notified(8, &mut irqs).unwrap();
    assert!(take_owner_datagrams(&peer, &mut owner, &mut irqs).is_empty());

    let board_file = vec![0x5a; 5000];
    let image = crate::qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&board_file, 0x1202, 1, 99, 1);
    owner.begin_wlfw_bdf_download(&image, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let fragments = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(fragments.len(), 16);
    let first_request: Vec<u8> = fragments.concat();
    assert_eq!(first_request.len(), 4096);
    assert_eq!(
        u32::from_le_bytes(first_request[20..24].try_into().unwrap()),
        4064
    );
    assert_eq!(
        u16::from_le_bytes(first_request[35..37].try_into().unwrap()),
        crate::wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID
    );
    assert_eq!(
        u32::from_le_bytes(first_request[53..57].try_into().unwrap()),
        board_file.len() as u32
    );
    assert_eq!(
        u32::from_le_bytes(first_request[60..64].try_into().unwrap()),
        0
    );
    assert_eq!(&first_request[69..69 + 4023], &board_file[..4023]);
    assert_eq!(&first_request[4092..4096], &[0x14, 1, 0, 0]);
    assert_eq!(owner.wlfw_state(), Some(State::BdfPending));

    peer.push(&data(
        9,
        &wlfw_status_response(
            6,
            crate::wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
            false,
            &[],
        ),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::BdfReady));
    owner.notified(8, &mut irqs).unwrap();
    assert!(take_owner_datagrams(&peer, &mut owner, &mut irqs).is_empty());

    owner.continue_wlfw_bdf_download(&image, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let fragments = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let second_request: Vec<u8> = fragments.concat();
    assert_eq!(second_request.len(), 1052);
    assert_eq!(
        u32::from_le_bytes(second_request[20..24].try_into().unwrap()),
        1018
    );
    assert_eq!(
        u32::from_le_bytes(second_request[60..64].try_into().unwrap()),
        1
    );
    assert_eq!(&second_request[69..69 + 977], &board_file[4023..]);
    assert_eq!(&second_request[1046..1050], &[0x14, 1, 0, 1]);
    assert_eq!(owner.wlfw_state(), Some(State::BdfPending));
    peer.push(&data(
        10,
        &wlfw_status_response(
            7,
            crate::wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
            false,
            &[],
        ),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::BdfComplete));

    owner.begin_wlfw_empty_cal_report(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let fragments = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let request: Vec<u8> = fragments.concat();
    assert_eq!(request.len(), 44);
    assert_eq!(
        u16::from_le_bytes(request[35..37].try_into().unwrap()),
        crate::wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID
    );
    assert_eq!(&request[39..43], &[1, 1, 0, 0]);
    assert_eq!(owner.wlfw_state(), Some(State::CalReportPending));
    peer.push(&data(
        11,
        &wlfw_status_response(
            8,
            crate::wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID,
            false,
            &[],
        ),
    ));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.wlfw_state(), Some(State::CalReportAccepted));
}

#[test]
fn owner_control_handshake_and_explicit_readiness_publish_service_through_fifo() {
    let (peer, mut owner, mut irqs) = owner_control_start();
    let outgoing = owner_datagrams(&peer.take_tx());
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0][4], 2);
    assert_eq!(owner.control_status().announced_services, 0);
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    assert!(owner_datagrams(&peer.take_tx()).is_empty());
    let schedules = irqs.schedules;
    owner
        .set_service_available(Service::HeapMemory, true, &mut irqs)
        .unwrap();
    owner.notified(8, &mut irqs).unwrap();
    let outgoing = owner_datagrams(&peer.take_tx());
    assert_eq!(outgoing.len(), 1);
    assert_eq!(
        &outgoing[0][32..],
        &[4u32, 52, 257, 1, 0x4001]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert!(irqs.schedules >= schedules);
    assert_eq!(owner.control_status().announced_services, 2);
    assert!(owner.pending_request().is_none());
}
#[test]
fn owner_control_peer_client_loss_retains_request_and_holds_native_tail() {
    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    owner
        .set_service_available(Service::HeapMemory, true, &mut irqs)
        .unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&query(2));
    owner.notified(1, &mut irqs).unwrap();
    peer.take_tx();
    let handle = owner.pending_request().unwrap();
    assert_eq!(owner.in_flight(), 1);
    peer.push(&data(3, &owner_control_frame(6, [0, 9, 0, 0])));
    let before = peer.0.borrow().words;
    let acks = irqs.calls.len();
    peer.clear();
    assert_eq!(
        owner.notified(1, &mut irqs),
        Err(OwnerError::Transport(Error::Session(
            qcom_glink_qmi::Error::Control(qcom_qrtr_control::Error::ClientGone)
        )))
    );
    assert_eq!(peer.0.borrow().words, before);
    assert!(writes(&peer.log()).is_empty());
    assert_eq!(irqs.calls.len(), acks);
    assert_eq!(owner.pending_request(), Some(handle));
    assert_eq!(owner.in_flight(), 1);
    let detail = owner.client_gone_diagnostic().expect("withdrawal metadata retained");
    assert_eq!(detail.command, 6);
    assert_eq!(detail.port, 9);
    assert_eq!(detail.service, Service::HeapMemory);
    assert_eq!(detail.matching_requests, 1);
    assert_eq!(detail.receive_state, 4);
    assert_eq!(detail.reply_count, 0);
    assert_eq!((detail.reply_sent, detail.reply_len), (0, 0));
    assert!(!detail.reply_glink_done && !detail.reply_qrtr_done);
    owner.fault();
    assert_eq!(owner.client_gone_diagnostic(), Some(detail));
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-peer-close-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_retains_same_peer_request_when_close_meets_deferred_wrq() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let held = owner.offer_receive().unwrap();
    let deferred = owner.offer_receive().unwrap();
    let close = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut first = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000);
    first[16..20].copy_from_slice(&0u32.to_le_bytes());
    peer.push(&data(held, &first)); owner.notified(1, &mut irqs).unwrap();
    let request = owner.pending_tftp_request().expect("first request is owner-held");
    let mut saturated = false;
    for n in 0..16 {
        let result = owner.queue_tftp_response(TftpReplyRoute {
            source: Address { node: 1, port: 0x5001 + n },
            destination: Address { node: 0, port: 0x80 + n },
        }, qcom_tftp_server_check::OACK, &mut irqs);
        if result == Err(OwnerError::Transport(Error::Session(qcom_glink_qmi::Error::NoCapacity))) {
            saturated = true;
            break;
        }
        result.unwrap();
    }
    assert!(saturated, "transmit queue must be full before the deferred receive");
    peer.push(&data(deferred, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000)));
    owner.notified(1, &mut irqs).unwrap();
    peer.push(&data(close, &owner_control_frame(6, [0, 0x57, 0, 0])));
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    {
        assert!(owner.notified(1, &mut irqs).is_err());
        assert_eq!(owner.pending_tftp_request(), Some(request), "rejected cleanup retains the request");
        return;
    }
    #[cfg(not(feature = "sunfish-tftp-close-handoff-prototype"))]
    {
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Transport(Error::Session(
        qcom_glink_qmi::Error::Control(qcom_qrtr_control::Error::ClientGone)))));
    let detail = owner.deferred_close_diagnostic().unwrap();
    assert_eq!(detail.peer_port, 0x57);
    assert_eq!(detail.block_mask & 2, 2);
    assert_eq!(owner.deferred_close_owner_kinds(), Some(1));
    assert_eq!(owner.pending_tftp_request(), Some(request), "the held request is not dropped");
    }
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-peer-close-experiment", feature = "sunfish-tftp-per-peer-reply-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_retains_same_peer_reply_when_close_meets_deferred_wrq() {
    exercise_owner_close_reply(0);
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-close-handoff-prototype", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_close_rejects_if_transfer_gains_data_after_hold() {
    exercise_owner_close_reply(1);
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-close-handoff-prototype", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_close_gate_failure_keeps_reply_and_server_state() {
    exercise_owner_close_reply(2);
    exercise_owner_close_reply(3);
    exercise_owner_close_reply(4);
    exercise_owner_close_reply(5);
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-close-handoff-prototype", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_close_partial_transport_commit_is_terminal_and_retains_owner_state() {
    exercise_owner_close_reply(6);
    exercise_owner_close_reply(7);
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-close-handoff-prototype", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn owner_close_failures_between_commits_are_terminal_and_retain_reply() {
    exercise_owner_close_reply(8);
    exercise_owner_close_reply(9);
}

#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-peer-close-experiment", feature = "sunfish-tftp-per-peer-reply-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
fn exercise_owner_close_reply(mode: u8) {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    #[cfg(feature = "sunfish-tftp-peer-close-observer")]
    owner.set_rmtfs_event_clock(|| 42);
    owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut server = qcom_tftp_server_check::ServerCheck::new();
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    let mut first_records = Vec::new();
    for (iteration, expected) in [TftpServiceProgress::ResponseQueued, TftpServiceProgress::WaitingReply].into_iter().enumerate() {
        let slot = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let body = if iteration == 0 {
            server_check_wrq_for_backpressure()
        } else {
            qcom_tftp_server_check::REQUEST.to_vec()
        };
        let mut request = tftp_qrtr(&body, 0x57, 0x5000);
        request[16..20].copy_from_slice(&0u32.to_le_bytes());
        peer.push(&data(slot, &request)); owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(expected));
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        if iteration == 0 { owner.notified(8, &mut irqs).unwrap(); first_records = peer.take_tx(); }
        let _ = iteration;
    }
    assert!(owner.tftp_queue_snapshot().1, "second reply is retained by owner");
    assert!(owner.pending_tftp_request().is_none());
    assert!(owner.can_cancel_server_check_reply(Address { node: 0, port: 0x57 }, &server));
    assert!(!owner.can_cancel_server_check_reply(Address { node: 0, port: 0x58 }, &server));
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    {
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &first_records); peer.take_tx();
        let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let body: Vec<u8> = [7u32, 0, 0x57, 0, 0].iter().flat_map(|word| word.to_le_bytes()).collect();
        peer.push(&data(slot, &owner_qrtr_packet(7, 0x57, 0x5001, false, &body)));
        owner.notified(1, &mut irqs).unwrap(); peer.take_tx();
    }
    let deferred = owner.offer_receive().unwrap(); let close = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    let mut saturated = false;
    for n in 0..16 {
        let result = owner.queue_tftp_response(TftpReplyRoute {
            source: Address { node: 1, port: 0x5100 + n },
            destination: Address { node: 0, port: 0x80 + n },
        }, qcom_tftp_server_check::OACK, &mut irqs);
        if result == Err(OwnerError::Transport(Error::Session(qcom_glink_qmi::Error::NoCapacity))) {
            saturated = true; break;
        }
        result.unwrap();
    }
    assert!(saturated);
    peer.push(&data(deferred, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000)));
    owner.notified(1, &mut irqs).unwrap();
    peer.push(&data(close, &owner_control_frame(6, [0, 0x57, 0, 0])));
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    {
        let result = owner.notified(1, &mut irqs);
        assert!(result.is_ok(), "{result:?} {:?}", owner.deferred_close_diagnostic());
        assert_eq!(owner.pending_tftp_close(), Some(Address { node: 0, port: 0x57 }));
        #[cfg(feature = "sunfish-tftp-peer-close-observer")]
        assert_eq!(owner.tftp_timing().peer_close, Some(42), "handoff records the matching close before commit");
        if mode == 1 {
            let mut out = [0; 128];
            let route = qcom_tftp_server_check::Route {
                source: qcom_tftp_server_check::Address { node: 0, port: 0x57 },
                destination: qcom_tftp_server_check::Address { node: 1, port: 0x5001 },
            };
            server.receive(route, b"\0\x03\0\x01x", &mut out).unwrap();
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err());
            assert_eq!(owner.tftp_close_commits(), 0);
            assert!(owner.tftp_queue_snapshot().1, "rejected cleanup retains the reply");
            assert_eq!(server.contents(), b"x");
            return;
        }
        if mode == 8 || mode == 9 {
            owner.test_fail_close_owner_stage(mode - 7);
            peer.clear();
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err());
            assert_eq!(owner.tftp_close_commits(), 0);
            assert!(owner.tftp_queue_snapshot().1, "partial owner commit retains the reply");
            let state = server.can_abandon_empty_peer(qcom_tftp_server_check::Address { node: 0, port: 0x57 });
            if mode == 8 { assert_eq!(state, Ok(())); }
            else { assert_eq!(state, Err(qcom_tftp_server_check::Error::UnexpectedPacket)); }
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err());
            assert!(writes(&peer.log()).is_empty());
            return;
        }
        if mode == 6 || mode == 7 {
            owner.test_fail_close_stage(mode - 5);
            peer.clear();
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err());
            assert_eq!(owner.tftp_close_commits(), 0);
            assert!(owner.tftp_queue_snapshot().1, "partial transport commit retains the reply");
            assert_eq!(server.can_abandon_empty_peer(qcom_tftp_server_check::Address { node: 0, port: 0x57 }), Ok(()));
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err(), "partial commit cannot be retried");
            assert!(writes(&peer.log()).is_empty());
            return;
        }
        if mode >= 2 {
            match mode {
                2 => peer.0.borrow_mut().fail = Some("observe"),
                3 => peer.0.borrow_mut().fail = Some("descriptor"),
                4 => peer.0.borrow_mut().fail = Some("barrier"),
                5 => peer.0.borrow_mut().observation.ready = false,
                _ => panic!("unexpected failure mode"),
            }
            peer.clear();
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err());
            assert_eq!(owner.tftp_close_commits(), 0);
            assert!(owner.tftp_queue_snapshot().1, "gate failure retains the reply");
            assert_eq!(server.can_abandon_empty_peer(qcom_tftp_server_check::Address { node: 0, port: 0x57 }), Ok(()));
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err(), "a failed gate cannot be retried");
            assert!(writes(&peer.log()).is_empty(), "failed gate publishes no writes");
            return;
        }
        owner.resolve_pending_tftp_close(&mut server).unwrap();
        assert_eq!(owner.tftp_close_commits(), 1);
        assert_eq!(owner.pending_tftp_close(), None);
        assert!(!owner.tftp_queue_snapshot().1, "the unpublished reply is retired only after commit");
        assert_eq!(server.can_abandon_empty_peer(qcom_tftp_server_check::Address { node: 0, port: 0x57 }), Err(qcom_tftp_server_check::Error::UnexpectedPacket));
        return;
    }
    #[cfg(not(feature = "sunfish-tftp-close-handoff-prototype"))]
    {
    let _ = mode;
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Transport(Error::Session(
        qcom_glink_qmi::Error::Control(qcom_qrtr_control::Error::ClientGone)))));
    let detail = owner.deferred_close_diagnostic().unwrap();
    assert_eq!(detail.peer_port, 0x57);
    assert_ne!(detail.block_mask & 2, 0);
    assert_eq!(owner.deferred_close_owner_kinds(), Some(4));
    assert!(owner.tftp_queue_snapshot().1, "the retained reply remains owned after the stop");
    assert!(!owner.can_cancel_server_check_reply(Address { node: 0, port: 0x57 }, &server), "a stopped owner cannot authorize cancellation");
    }
}

#[test]
fn owner_control_server_withdrawal_records_matching_request_metadata() {
    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    owner.set_service_available(Service::HeapMemory, true, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&query(2));
    owner.notified(1, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&data(3, &owner_control_frame(5, [0x2b, 0x1202, 0, 9])));
    assert_eq!(owner.notified(1, &mut irqs), Err(OwnerError::Transport(Error::Session(
        qcom_glink_qmi::Error::Control(qcom_qrtr_control::Error::ClientGone)
    ))));
    let detail = owner.client_gone_diagnostic().unwrap();
    assert_eq!((detail.command, detail.port, detail.service), (5, 9, Service::HeapMemory));
    assert_eq!(detail.matching_requests, 1);
}
#[test]
fn owner_control_malformed_announcement_cannot_ack_consume_or_publish() {
    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    peer.push(&data(2, &owner_control_frame(4, [43, 0x1202, 1, 1])));
    let before = peer.0.borrow().words;
    let acks = irqs.calls.len();
    peer.clear();
    assert!(owner.notified(1, &mut irqs).is_err());
    assert_eq!(peer.0.borrow().words, before);
    assert!(writes(&peer.log()).is_empty());
    assert_eq!(irqs.calls.len(), acks);
    assert!(owner.remote_server(0).is_none());
}
#[test]
fn owner_control_ready_loss_suppresses_queued_service_announcement() {
    let (peer, mut owner, mut irqs) = owner_control_start();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    peer.take_tx();
    owner
        .set_service_available(Service::HeapMemory, true, &mut irqs)
        .unwrap();
    peer.0.borrow_mut().observation.ready = false;
    peer.clear();
    assert!(owner.notified(8, &mut irqs).is_err());
    assert_eq!(owner.control_status().announced_services, 0);
    assert!(writes(&peer.log()).is_empty());
}

mod handler_integration {
    use super::*;
    use qcom_modem_handlers::{Fault, Handlers, Pool, Profile};
    use qcom_modem_owner::ServiceProgress;
    use services_fixture::{self as fixture, FakeBackend, Tag};
    use std::collections::VecDeque;
    struct Rig {
        peer: Fake,
        owner: Owner<Fake>,
        worker: Handlers<FakeBackend>,
        backend: FakeBackend,
        irqs: Irqs,
        offered: VecDeque<u32>,
        sent: Vec<Vec<u8>>,
        held: Vec<(u32, u32)>,
        hold_credit: bool,
        transaction: u16,
    }
    fn word(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn field(kind: u8, value: &[u8]) -> Vec<u8> {
        let mut b = vec![kind];
        b.extend((value.len() as u16).to_le_bytes());
        b.extend(value);
        b
    }
    fn f32(kind: u8, value: u32) -> Vec<u8> {
        field(kind, &value.to_le_bytes())
    }
    impl Rig {
        fn new() -> Self {
            let peer = Fake::new(0, 0);
            let mut s = session();
            s.register(1, Service::RemoteStorage, 0x4000).unwrap();
            let mut owner = Owner::attach(Adapter::attach(peer.clone(), s, 1).unwrap(), 1).unwrap();
            owner.start_control().unwrap();
            let c = fixture::config(8192, 8192, 16384);
            let backend = fixture::backend(c);
            let mut worker = Handlers::attach(Profile::validate(c, &[]).unwrap(), backend.clone());
            let mut irqs = Irqs::default();
            owner.activate_handlers(&mut worker, &mut irqs).unwrap();
            let mut intents = control(5, 2, 32);
            for id in 0u32..32 {
                intents.extend(4096u32.to_le_bytes());
                intents.extend(id.to_le_bytes());
            }
            peer.push(&intents);
            let mut rig = Self {
                peer,
                owner,
                worker,
                backend,
                irqs,
                offered: VecDeque::new(),
                sent: Vec::new(),
                held: Vec::new(),
                hold_credit: false,
                transaction: 1,
            };
            rig.pump().unwrap();
            rig.inject(&owner_control_frame(2, [0; 4]));
            rig.pump().unwrap();
            assert_eq!(rig.owner.control_status().announced_services, 3);
            rig
        }
        fn inject(&mut self, b: &[u8]) {
            let id = self.offered.pop_front().expect("backed receive intent");
            self.peer.push(&data(id, b));
        }
        fn credit(&mut self, local: u32, peer: u32) {
            let b: Vec<u8> = [1u32, 7, 0, peer, 0, 20, 1, local, 7, 0, peer, 0, 0]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect();
            self.inject(&b);
        }
        fn collect(&mut self) {
            let bytes = self.peer.take_tx();
            let mut at = 0;
            while at < bytes.len() {
                let b = &bytes[at..];
                let command = u16::from_le_bytes([b[0], b[1]]);
                at += match command {
                    5 => {
                        assert_eq!(word(b, 4), 1);
                        self.offered.push_back(word(b, 12));
                        16
                    }
                    14 => {
                        self.offered.push_back(word(b, 4));
                        8
                    }
                    6 | 8 | 13 => 8,
                    9 => {
                        let length = word(b, 8) as usize;
                        assert_eq!(word(b, 12), 0);
                        let packet = b[16..16 + length].to_vec();
                        self.peer.push(&control(6, 2, word(b, 4)));
                        if word(&packet, 4) == 1 {
                            let local = word(&packet, 12);
                            let peer = word(&packet, 28);
                            if self.hold_credit {
                                self.held.push((local, peer));
                            } else {
                                self.credit(local, peer);
                            }
                        }
                        self.sent.push(packet);
                        (16 + length + 7) & !7
                    }
                    _ => panic!("unexpected outgoing command {command}"),
                };
            }
        }
        fn round(&mut self) -> Result<ServiceProgress, OwnerError> {
            let actions = self.backend.action_count();
            let progress =
                self.owner
                    .notified_with_handlers(8, &mut self.worker, &mut self.irqs)?;
            assert!(self.backend.action_count() - actions <= 1);
            self.collect();
            Ok(progress.service)
        }
        fn idle(&self) -> bool {
            let w = self.peer.0.borrow().words;
            self.worker.active_handle().is_none()
                && self.owner.pending_request().is_none()
                && w[0] == w[1]
                && w[2] == w[3]
        }
        fn pump(&mut self) -> Result<(), OwnerError> {
            let mut quiet = 0;
            for _ in 0..4096 {
                self.round()?;
                if self.idle() {
                    quiet += 1;
                    // An enqueued response is published by the next local
                    // continuation, even while both native cursors are empty.
                    if quiet == 2 {
                        return Ok(());
                    }
                } else {
                    quiet = 0;
                }
            }
            panic!("service did not become idle")
        }
        fn send(&mut self, service: Service, id: u16, fields: &[Vec<u8>]) -> u16 {
            let txn = self.transaction;
            self.transaction += 1;
            let payload: Vec<u8> = fields.iter().flatten().copied().collect();
            let mut q = vec![0];
            q.extend(txn.to_le_bytes());
            q.extend(id.to_le_bytes());
            q.extend((payload.len() as u16).to_le_bytes());
            q.extend(payload);
            let mut b: Vec<u8> = [
                1u32,
                1,
                0,
                9,
                0,
                q.len() as u32,
                1,
                if service == Service::RemoteStorage {
                    0x4000
                } else {
                    0x4001
                },
            ]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
            b.extend(q);
            b.resize((b.len() + 3) & !3, 0);
            self.inject(&b);
            txn
        }
        fn message(&self, txn: u16) -> qcom_qmi::Message<'_> {
            let b = self
                .sent
                .iter()
                .find(|b| {
                    word(b, 4) == 1 && u16::from_le_bytes(b[33..35].try_into().unwrap()) == txn
                })
                .expect("matching response");
            qcom_qmi::Message::decode(&b[32..32 + word(b, 20) as usize]).unwrap()
        }
        fn success(&self, txn: u16) {
            assert_eq!(self.message(txn).required(2).unwrap(), [0, 0, 0, 0]);
        }
        fn open(&mut self) -> u32 {
            let txn = self.send(Service::RemoteStorage, 1, &[field(1, b"/boot/modem_fs1")]);
            self.pump().unwrap();
            self.success(txn);
            self.message(txn).u32(0x10).unwrap()
        }
    }
    #[test]
    fn handlers_complete_storage_and_memory_actions_through_both_rings_and_qmi_credits() {
        let mut r = Rig::new();
        let handle = r.open();
        let txn = r.send(Service::RemoteStorage, 4, &[f32(1, handle), f32(2, 8192)]);
        r.pump().unwrap();
        r.success(txn);
        let base = u64::from_le_bytes(r.message(txn).required(0x10).unwrap().try_into().unwrap());
        assert_eq!(base, r.backend.0.borrow().config.storage.base);
        let mut spans = vec![1];
        spans.extend(0u32.to_le_bytes());
        spans.extend((base as u32).to_le_bytes());
        spans.extend(16u32.to_le_bytes());
        let txn = r.send(
            Service::RemoteStorage,
            3,
            &[
                f32(1, handle),
                field(2, &[0]),
                field(3, &spans),
                field(4, &[0]),
            ],
        );
        r.pump().unwrap();
        r.success(txn);
        assert_eq!(
            &r.backend.0.borrow().storage[..],
            &r.backend.0.borrow().disks[0][..8192]
        );
        let txn = r.send(Service::HeapMemory, 0x24, &[f32(1, 1)]);
        r.pump().unwrap();
        r.success(txn);
        assert_eq!(r.message(txn).u32(0x10).unwrap(), 8192);
        let txn = r.send(
            Service::HeapMemory,
            0x22,
            &[f32(1, 4096), f32(2, 1), f32(3, 0), f32(4, 19)],
        );
        r.pump().unwrap();
        r.success(txn);
        let block = r.message(txn).required(0x11).unwrap().to_vec();
        assert_eq!(block[0], 1);
        let txn = r.send(Service::HeapMemory, 0x23, &[field(1, &block)]);
        r.pump().unwrap();
        r.success(txn);
        assert!(!r.worker.leased(Pool::Heap));
        let txn = r.send(Service::RemoteStorage, 2, &[f32(1, handle)]);
        r.pump().unwrap();
        r.success(txn);
        assert!(!r.worker.leased(Pool::Storage));
        assert_eq!(r.owner.in_flight(), 0);
        assert_eq!(r.worker.open_handles(), 0);
    }
    #[test]
    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
    fn handlers_cache_completed_action_while_reply_flow_is_blocked() {
        let mut r = Rig::new();
        r.hold_credit = true;
        let first = r.open();
        assert_eq!(first, 1);
        assert_eq!(r.owner.in_flight(), 1);
        let txn = r.send(Service::RemoteStorage, 1, &[field(1, b"/boot/modem_fs1")]);
        for _ in 0..8 {
            assert_eq!(r.round().unwrap(), ServiceProgress::WaitingReply);
            assert_eq!(r.worker.open_handles(), 2);
        }
        let (local, peer) = r.held.remove(0);
        r.hold_credit = false;
        r.credit(local, peer);
        r.pump().unwrap();
        r.success(txn);
        assert_eq!(r.message(txn).u32(0x10).unwrap(), 2);
        assert_eq!(r.worker.open_handles(), 2);
        assert_eq!(r.owner.in_flight(), 0);
    }
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    #[test]
    fn handlers_reuse_storage_flow_after_glink_without_fabricated_credit() {
        let mut r = Rig::new();
        r.hold_credit = true;
        assert_eq!(r.open(), 1);
        assert_eq!(r.owner.in_flight(), 0);
        let txn = r.send(Service::RemoteStorage, 1, &[field(1, b"/boot/modem_fs1")]);
        r.pump().unwrap();
        r.success(txn);
        assert_eq!(r.message(txn).u32(0x10).unwrap(), 2);
        assert_eq!(r.worker.open_handles(), 2);
        assert_eq!(r.owner.in_flight(), 0);
    }
    #[test]
    fn handlers_uncertain_grant_stops_both_owners_without_address_reply_or_retry() {
        let mut r = Rig::new();
        let handle = r.open();
        r.backend.0.borrow_mut().fail = Some((Tag::Grant, Fault::Uncertain));
        let txn = r.send(Service::RemoteStorage, 4, &[f32(1, handle), f32(2, 4096)]);
        assert_eq!(
            r.pump(),
            Err(OwnerError::Service(
                qcom_modem_handlers::Error::BackendUncertain
            ))
        );
        let actions = r.backend.action_count();
        assert!(r.round().is_err());
        assert_eq!(r.backend.action_count(), actions);
        assert!(r.owner.pending_request().is_some());
        assert!(
            !r.sent.iter().any(
                |b| word(b, 4) == 1 && u16::from_le_bytes(b[33..35].try_into().unwrap()) == txn
            )
        );
    }
    #[test]
    fn handlers_transport_ready_loss_after_grant_retains_lease_and_holds_success_reply() {
        let mut r = Rig::new();
        let handle = r.open();
        let peer = r.peer.clone();
        r.backend.0.borrow_mut().after = Some(Box::new(move |tag| {
            if tag == Tag::Grant {
                peer.0.borrow_mut().observation.ready = false;
            }
        }));
        let txn = r.send(Service::RemoteStorage, 4, &[f32(1, handle), f32(2, 4096)]);
        assert!(r.pump().is_err());
        assert!(r.worker.leased(Pool::Storage));
        assert!(r.owner.pending_request().is_some());
        assert!(
            !r.sent.iter().any(
                |b| word(b, 4) == 1 && u16::from_le_bytes(b[33..35].try_into().unwrap()) == txn
            )
        );
    }
    #[test]
    fn handlers_instance_binding_and_paired_notification_path_cannot_be_bypassed() {
        let mut r = Rig::new();
        let c = fixture::config(8192, 8192, 16384);
        let mut replacement =
            Handlers::attach(Profile::validate(c, &[]).unwrap(), fixture::backend(c));
        assert_eq!(
            r.owner
                .notified_with_handlers(8, &mut replacement, &mut r.irqs),
            Err(OwnerError::Service(
                qcom_modem_handlers::Error::Configuration
            ))
        );
        assert!(replacement.error().is_some());
        assert!(r.round().is_err());
        assert!(r.worker.error().is_some());
        let mut r = Rig::new();
        assert_eq!(
            r.owner.notified(8, &mut r.irqs),
            Err(OwnerError::Service(
                qcom_modem_handlers::Error::InvalidRequest
            ))
        );
        assert!(r.round().is_err());
        assert!(r.worker.error().is_some());
    }
}

#[cfg(feature = "sunfish-wlan-state-query")]
fn wlan_query_owner() -> (Fake, Owner<Fake>, Irqs) {
    let (peer, mut owner) = owner();
    owner.enable_servreg_notif_client(0x4005).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 8);
    for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    (peer, owner, irqs)
}
#[cfg(feature = "sunfish-wlan-state-query")]
fn wlan_query_announce(peer: &Fake, owner: &mut Owner<Fake>, irqs: &mut Irqs, service: u32, instance: u32, port: u32) {
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &owner_control_frame(4, [service, instance, 0, port])));
    owner.notified(1, irqs).unwrap();
    take_owner_datagrams(peer, owner, irqs);
}
#[cfg(feature = "sunfish-wlan-state-query")]
#[test]
fn wlan_query_waits_for_exact_service_and_ipa_then_sends_only_one_read_query() {
    use qcom_modem_owner::WlanStateQuery;
    let (peer, mut owner, mut irqs) = wlan_query_owner();
    let mut probe = WlanStateQuery::default();
    probe.step(&mut owner, true, &mut irqs).unwrap();
    assert_eq!(probe.snapshot(&owner)[0], 1);
    wlan_query_announce(&peer, &mut owner, &mut irqs, 0x42, 0xb501, 4);
    wlan_query_announce(&peer, &mut owner, &mut irqs, 0x45, 2, 90);
    probe.step(&mut owner, true, &mut irqs).unwrap();
    assert_eq!((probe.snapshot(&owner)[4], probe.snapshot(&owner)[7]), (0, 0));
    wlan_query_announce(&peer, &mut owner, &mut irqs, 0x42, 0xb401, 3);
    wlan_query_announce(&peer, &mut owner, &mut irqs, 0x45, 1, 81);
    probe.step(&mut owner, false, &mut irqs).unwrap();
    assert_eq!(probe.snapshot(&owner), [2, u32::MAX, u32::MAX, u32::MAX, 1, 0, 3, 1, 0, 81]);
    assert!(peer.take_tx().is_empty());
    probe.step(&mut owner, true, &mut irqs).unwrap();
    for _ in 0..5 { probe.step(&mut owner, true, &mut irqs).unwrap(); }
    owner.notified(8, &mut irqs).unwrap();
    let packets = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let queries: Vec<_> = packets.iter().filter(|p| owner_word(p, 4) == 1).collect();
    assert_eq!(queries.len(), 1);
    let q = queries[0];
    assert_eq!((owner_word(q, 12), owner_word(q, 28)), (0x4005, 3));
    assert_eq!(&q[32..59], b"\x00\x01\x00\x21\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd");
    assert_eq!(probe.snapshot(&owner)[0], 3);
    // A confirmed successful UNINIT response is reported, never restarted.
    let payload = [2,1,0,0x21,0,14,0,2,4,0,0,0,0,0,0x10,4,0,0xff,0xff,0xff,0x7f];
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &owner_qrtr_packet(1, 3, 0x4005, false, &payload)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(&probe.snapshot(&owner)[..4], &[4, 0x7fff_ffff, 0, 0]);
    for _ in 0..5 { probe.step(&mut owner, true, &mut irqs).unwrap(); owner.notified(8, &mut irqs).unwrap(); }
    assert!(owner_datagrams(&peer.take_tx()).iter().all(|p| owner_word(p,4)!=1), "no listener, restart, WLFW or duplicate query");
    assert!(!owner.servreg_notif_outcome().unwrap().restart_succeeded());
}
#[cfg(feature = "sunfish-wlan-state-query")]
#[test]
fn wlan_query_does_not_misreport_failure_or_missing_state_as_down_or_retry() {
    for (result, error, raw, phase) in [(1u16, 3u16, Some(0x0fff_ffffu32), 5u32), (0, 0, None, 6), (0, 0, Some(0x1fff_ffff), 4), (0, 0, Some(0x12345678), 4)] {
        let (peer, mut owner, mut irqs) = wlan_query_owner();
        wlan_query_announce(&peer, &mut owner, &mut irqs, 0x42, 0xb401, 3);
        let mut probe = qcom_modem_owner::WlanStateQuery::default();
        probe.step(&mut owner, true, &mut irqs).unwrap();
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let mut payload = vec![2,1,0,0x21,0, if raw.is_some() {14} else {7},0,2,4,0];
        payload.extend(result.to_le_bytes()); payload.extend(error.to_le_bytes());
        if let Some(raw) = raw { payload.extend([0x10,4,0]);payload.extend(raw.to_le_bytes()); }
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        peer.push(&data(intent, &owner_qrtr_packet(1, 3, 0x4005, false, &payload)));
        owner.notified(1, &mut irqs).unwrap();
        let expected_raw = if result == 0 && error == 0 { raw.unwrap_or(u32::MAX) } else { u32::MAX };
        assert_eq!(&probe.snapshot(&owner)[..4], &[phase, expected_raw, result as u32, error as u32]);
        probe.step(&mut owner, true, &mut irqs).unwrap();
        owner.notified(8, &mut irqs).unwrap();
        assert!(owner_datagrams(&peer.take_tx()).iter().all(|p| owner_word(p,4)!=1));
    }
}
#[cfg(feature = "sunfish-wlan-state-query")]
#[test]
fn wlan_query_schedule_failure_consumes_the_one_attempt() {
    let (peer, mut owner, mut irqs) = wlan_query_owner();
    wlan_query_announce(&peer, &mut owner, &mut irqs, 0x42, 0xb401, 3);
    let mut probe = qcom_modem_owner::WlanStateQuery::default();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    irqs.fail_schedule = true;
    assert_eq!(probe.step(&mut owner, true, &mut irqs), Err(OwnerError::Schedule));
    let calls = irqs.schedules;
    irqs.fail_schedule = false;
    probe.step(&mut owner, true, &mut irqs).unwrap();
    assert_eq!(irqs.schedules, calls);
    assert!(peer.take_tx().is_empty());
    assert_eq!(probe.snapshot(&owner)[0], 3);
}

#[cfg(feature = "sunfish-wlan-state-listener")]
fn wlan_listener_response(peer: &Fake, owner: &mut Owner<Fake>, irqs: &mut Irqs, id: u8, txn: u8, result: u16, error: u16, state: Option<u32>) -> Vec<Vec<u8>> {
    let mut payload = vec![2,txn,0,id,0,if state.is_some() {14} else {7},0,2,4,0];
    payload.extend(result.to_le_bytes()); payload.extend(error.to_le_bytes());
    if let Some(raw) = state { payload.extend([0x10,4,0]); payload.extend(raw.to_le_bytes()); }
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &owner_qrtr_packet(1, 3, 0x4005, false, &payload)));
    owner.notified(1, irqs).unwrap();
    take_owner_datagrams(peer, owner, irqs)
}
#[cfg(feature = "sunfish-wlan-state-listener")]
fn wlan_listener_query_ready(result: u16, error: u16, state: Option<u32>) -> (Fake, Owner<Fake>, Irqs) {
    let (peer, mut owner, mut irqs) = wlan_query_owner();
    wlan_query_announce(&peer, &mut owner, &mut irqs, 0x42, 0xb401, 3);
    let mut query = qcom_modem_owner::WlanStateQuery::default();
    query.step(&mut owner, true, &mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    wlan_listener_response(&peer, &mut owner, &mut irqs, 0x21, 1, result, error, state);
    (peer, owner, irqs)
}
#[cfg(feature = "sunfish-wlan-state-listener")]
#[test]
fn wlan_listener_registers_once_and_acks_update_without_restart() {
    let (peer, mut owner, mut irqs) = wlan_listener_query_ready(0,0,Some(0x7fff_ffff));
    let mut listener = qcom_modem_owner::WlanStateListener::default();
    listener.step(&mut owner, false, &mut irqs).unwrap();
    assert_eq!(listener.snapshot(&owner)[0], 1);
    assert!(peer.take_tx().is_empty());
    for _ in 0..5 { listener.step(&mut owner, true, &mut irqs).unwrap(); }
    owner.notified(8, &mut irqs).unwrap();
    let packets = take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let requests: Vec<_> = packets.iter().filter(|p| owner_word(p,4)==1).collect();
    assert_eq!(requests.len(),1);
    assert_eq!(&requests[0][32..63], b"\x00\x02\x00\x20\x00\x18\x00\x01\x01\x00\x01\x02\x11\x00msm/modem/wlan_pd");
    assert_eq!((owner_word(requests[0],12),owner_word(requests[0],28)),(0x4005,3));
    assert_eq!(listener.snapshot(&owner)[0],3);
    wlan_listener_response(&peer, &mut owner, &mut irqs, 0x20, 2, 0,0,Some(0x7fff_ffff));
    assert_eq!(listener.snapshot(&owner),[4,0x7fff_ffff,0,0,u32::MAX,u32::MAX,u32::MAX,u32::MAX]);
    // A matching state update uses the existing protocol ACK, not restart.
    let mut indication=vec![4,0x34,0x12,0x22,0,32,0,1,4,0];
    indication.extend(0x1fff_ffffu32.to_le_bytes());
    indication.extend([2,17,0]);indication.extend(b"msm/modem/wlan_pd");
    indication.extend([3,2,0,0x78,0x56]);
    let intent=owner.offer_receive().unwrap();owner.notified(8,&mut irqs).unwrap();peer.take_tx();
    peer.push(&data(intent,&owner_qrtr_packet(1,3,0x4005,false,&indication)));
    owner.notified(1,&mut irqs).unwrap();
    let packets=take_owner_datagrams(&peer,&mut owner,&mut irqs);
    let requests:Vec<_>=packets.iter().filter(|p|owner_word(p,4)==1).collect();
    assert_eq!(requests.len(),1);
    assert_eq!(&requests[0][32..64],b"\x00\x03\x00\x23\x00\x19\x00\x01\x11\x00msm/modem/wlan_pd\x02\x02\x00\x78\x56");
    assert_eq!(&listener.snapshot(&owner)[4..],&[0x5678,0x1fff_ffff,u32::MAX,u32::MAX]);
    wlan_listener_response(&peer,&mut owner,&mut irqs,0x23,3,0,0,None);
    assert_eq!(&listener.snapshot(&owner)[4..],&[0x5678,0x1fff_ffff,0,0]);
    for _ in 0..5 {listener.step(&mut owner,true,&mut irqs).unwrap();owner.notified(8,&mut irqs).unwrap();}
    assert!(owner_datagrams(&peer.take_tx()).iter().all(|p|owner_word(p,4)!=1));
    assert!(owner.servreg_notif_outcome().unwrap().restart.is_none());
}
#[cfg(feature = "sunfish-wlan-state-listener")]
#[test]
fn wlan_listener_requires_fresh_successful_uninit_and_does_not_invent_state() {
    for (result,error,state) in [(1,3,Some(0x7fff_ffff)),(0,0,None),(0,0,Some(0x1fff_ffff)),(0,0,Some(0x12345678))] {
        let (peer,mut owner,mut irqs)=wlan_listener_query_ready(result,error,state);
        let mut listener=qcom_modem_owner::WlanStateListener::default();
        listener.step(&mut owner,true,&mut irqs).unwrap();
        assert_eq!(listener.snapshot(&owner),[2,u32::MAX,u32::MAX,u32::MAX,u32::MAX,u32::MAX,u32::MAX,u32::MAX]);
        assert!(peer.take_tx().is_empty());
    }
    for (result,error,state,phase,raw) in [(1,3,Some(0x7fff_ffff),5,u32::MAX),(0,0,None,6,u32::MAX),(0,0,Some(0x1fff_ffff),4,0x1fff_ffff)] {
        let (peer,mut owner,mut irqs)=wlan_listener_query_ready(0,0,Some(0x7fff_ffff));
        let mut listener=qcom_modem_owner::WlanStateListener::default();
        listener.step(&mut owner,true,&mut irqs).unwrap();owner.notified(8,&mut irqs).unwrap();
        take_owner_datagrams(&peer,&mut owner,&mut irqs);
        wlan_listener_response(&peer,&mut owner,&mut irqs,0x20,2,result,error,state);
        assert_eq!(&listener.snapshot(&owner)[..4],&[phase,raw,result as u32,error as u32]);
        listener.step(&mut owner,true,&mut irqs).unwrap();owner.notified(8,&mut irqs).unwrap();
        assert!(owner_datagrams(&peer.take_tx()).iter().all(|p|owner_word(p,4)!=1));
    }
}
#[cfg(feature = "sunfish-wlan-state-listener")]
#[test]
fn wlan_listener_schedule_failure_is_not_retried() {
    let (peer,mut owner,mut irqs)=wlan_listener_query_ready(0,0,Some(0x7fff_ffff));
    owner.notified(8,&mut irqs).unwrap();peer.take_tx();
    let mut listener=qcom_modem_owner::WlanStateListener::default();irqs.fail_schedule=true;
    assert_eq!(listener.step(&mut owner,true,&mut irqs),Err(OwnerError::Schedule));
    let calls=irqs.schedules;irqs.fail_schedule=false;
    listener.step(&mut owner,true,&mut irqs).unwrap();assert_eq!(irqs.schedules,calls);
    assert_eq!(listener.snapshot(&owner)[0],3);assert!(peer.take_tx().is_empty());
}

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
#[test]
fn mcfg_write_and_read_round_trip_through_glink_owner() {
    use qcom_modem_owner::TftpServiceProgress;
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 16);
    for id in 0u32..16 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    let mut server = qcom_tftp_server_check::ServerCheck::new();

    let exchanges: &[(&[u8], u32, u32, &[u8], u32)] = &[
        (b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0",
         0x54, 0x5000, b"\0\x06blksize\0512\0timeoutms\01000\0wsize\01\0", 0x5001),
        (b"\0\x03\0\x01mcfg", 0x54, 0x5001, b"\0\x04\0\x01", 0x5001),
        (b"\0\x01/readwrite/mcfg.tmp\0octet\0tsize\00\0", 0x55, 0x5000, b"\0\x06tsize\04\0", 0x5002),
        (b"\0\x04\0\0", 0x55, 0x5002, b"\0\x03\0\x01mcfg", 0x5002),
        (b"\0\x04\0\x01", 0x55, 0x5002, b"", 0x5002),
    ];
    for &(request, remote, local, expected, source) in exchanges {
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        peer.push(&data(intent, &tftp_qrtr(request, remote, local)));
        owner.notified(1, &mut irqs).unwrap();
        let progress = owner.service_tftp_server_check_step(&mut server, &mut irqs).unwrap();
        assert_eq!(progress, if expected.is_empty() { TftpServiceProgress::Idle } else { TftpServiceProgress::ResponseQueued });
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        if request.starts_with(&[0, 2]) {
            let trace = owner.mcfg_trace().0.unwrap();
            assert_eq!((trace.flags, trace.source.port, trace.destination.port), (3, 0x5001, 0x54));
        }
        owner.notified(8, &mut irqs).unwrap();
        let records = peer.take_tx();
        let packets = owner_datagrams(&records);
        let reply = packets.iter().find(|p| owner_word(p, 4) == 1 && owner_word(p, 12) == source && owner_word(p, 28) == remote);
        if expected.is_empty() {
            assert!(reply.is_none());
        } else {
            let reply = reply.unwrap();
            assert_eq!(owner_word(reply, 20) as usize, expected.len());
            assert_eq!(&reply[32..32 + expected.len()], expected);
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            if request.starts_with(&[0, 2]) {
                assert_eq!(owner.mcfg_trace().0.unwrap().flags, 15);
            }
            acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
        }
    }
    assert_eq!(server.mcfg_progress(), (1, Some(4), 1));
    assert_eq!(server.first_rejection(), None);
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    {
        let (oack, data) = owner.mcfg_trace();
        assert_eq!(oack.unwrap().flags, 31);
        let data = data.unwrap();
        assert_eq!((data.opcode, data.block_or_error, data.packet_bytes, data.local_port),
                   (3, Some(1), 8, 0x5001));
    }
}

#[cfg(feature = "sunfish-tftp-mcfg-trace")]
#[test]
fn mcfg_trace_distinguishes_competing_writer_from_invalid_options() {
    use qcom_tftp_server_check::{Address, Error, Route, ServerCheck};
    let mut server = ServerCheck::new();
    let mut reply = [0; 64];
    let write = b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0";
    let route = |port| Route {
        source: Address { node: 0, port },
        destination: Address { node: 1, port: 0x5000 },
    };
    server.receive(route(0x54), write, &mut reply).unwrap();
    assert_eq!(server.receive(route(0x56), &write[..60], &mut reply), Err(Error::McfgMalformed));
    assert_eq!(server.receive(route(0x56), write, &mut reply), Err(Error::McfgBusy));
    assert_eq!(server.mcfg_progress(), (1, None, 0));
}

#[cfg(feature = "sunfish-wlan-elf-queue-trace")]
#[test]
fn elf_queue_trace_distinguishes_same_peer_from_slot_exhaustion() {
    use qcom_glink_qmi::{TftpReplyRoute, TX_SLOTS};
    use qcom_modem_owner::ServiceProgress;
    use qcom_qmi_router::Address;

    for (peer_port, reason) in [(9, 1), (0x80, 2)] {
        let (peer, mut owner) = owner();
        owner.prepare_tftp_server(0x5000).unwrap();
        owner.prepare_service_registry().unwrap();
        owner.start_control().unwrap();
        let mut intents = control(5, 2, 8);
        for id in 0u32..8 {
            intents.extend(4096u32.to_le_bytes());
            intents.extend(id.to_le_bytes());
        }
        peer.push(&intents);
        let mut irqs = Irqs::default();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        peer.push(&data(intent, &servreg_query_datagram_for(b"kernel/elf_loader")));
        owner.notified(1, &mut irqs).unwrap();
        let occupied = owner.tftp_queue_snapshot().0.all_tx_used as usize;
        assert!(occupied < TX_SLOTS - 1);
        let slots = if reason == 1 { 1 } else { TX_SLOTS - occupied };

        for slot in 0..slots as u32 {
            owner.queue_tftp_response(TftpReplyRoute {
                source: Address { node: 1, port: 0x5001 + slot },
                destination: Address { node: 0, port: peer_port + slot },
            }, qcom_tftp_server_check::OACK, &mut irqs).unwrap();
        }
        assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::WaitingReply));
        let trace = owner.service_registry_trace().elf.unwrap();
        let mut expected = [0; 6];
        expected[reason] = 1;
        assert_eq!(trace.waits_by_reason, expected);
        let pressure = trace.last_pressure.unwrap();
        assert_eq!(usize::from(pressure.reason), reason);
        assert_eq!(usize::from(pressure.used_slots), occupied + slots);
        assert!(usize::from(pressure.unpublished_slots) >= slots);
        assert_ne!(pressure.kind_mask & (1 << 6), 0);
        assert_eq!(pressure.peer_kind, if reason == 1 { 6 } else { 0 });
        assert_eq!(pressure.peer_flags & 0b0111, 0, "the peer packet has not started publication");
    }
}

#[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
#[test]
#[cfg(not(feature = "sunfish-wlan-registry-second-reply"))]
fn registry_txn4_timing_follows_exact_reply_that_blocks_elf() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use qcom_modem_owner::ServiceProgress;
    static CLOCK: AtomicU64 = AtomicU64::new(0);

    let (peer, mut owner, mut irqs) = owner_control_start();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.set_rmtfs_event_clock(|| CLOCK.load(Relaxed));

    let receive = |owner: &mut Owner<Fake>, irqs: &mut Irqs, packet: &[u8]| {
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, irqs).unwrap();
        peer.take_tx();
        peer.push(&data(intent, packet));
        owner.notified(1, irqs).unwrap();
    };
    let txn4 = servreg_query_datagram_with_transaction(b"tms/servreg", 4);
    let mut unrelated = txn4.clone();
    unrelated[12..16].copy_from_slice(&10u32.to_le_bytes());
    receive(&mut owner, &mut irqs, &unrelated);
    assert!(owner.service_registry_trace().txn4.is_none());
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);

    CLOCK.store(100, Relaxed);
    receive(&mut owner, &mut irqs, &txn4);
    receive(&mut owner, &mut irqs, &servreg_query_datagram_with_transaction(b"kernel/elf_loader", 5));
    CLOCK.store(120, Relaxed);
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
    {
        let dispatch = owner.service_registry_trace().elf.unwrap().dispatch;
        assert_eq!(dispatch.receive_pass_end, Some(100));
        assert_eq!(dispatch.registry_entered, Some(120));
        assert_eq!(dispatch.selected, None);
    }
    CLOCK.store(125, Relaxed);
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::WaitingReply));
    let trace = owner.service_registry_trace().txn4.unwrap();
    assert_eq!(trace.name_code, qcom_servreg::NAME_TMS_SERVREG);
    assert_eq!((trace.received, trace.queued, trace.published, trace.done, trace.elf_waits),
               (Some(100), Some(120), None, None, 1));

    CLOCK.store(150, Relaxed);
    owner.notified(8, &mut irqs).unwrap();
    let trace = owner.service_registry_trace().txn4.unwrap();
    assert_eq!((trace.published, trace.done), (Some(150), None));
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::WaitingReply));

    CLOCK.store(300, Relaxed);
    assert_eq!(take_owner_datagrams(&peer, &mut owner, &mut irqs).len(), 1);
    let trace = owner.service_registry_trace().txn4.unwrap();
    assert_eq!((trace.published, trace.done, trace.elf_waits), (Some(150), Some(300), 2));
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
    {
        let elf = owner.service_registry_trace().elf.unwrap();
        assert_eq!(elf.dispatch, qcom_modem_owner::ElfDispatchTiming {
            receive_pass_end: Some(100),
            registry_entered: Some(120),
            selected: Some(125),
            #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
            turns: elf.dispatch.turns,
        });
        #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
        {
            let turns = elf.dispatch.turns;
            assert_eq!(turns.calls, 1);
            assert_eq!(turns.max_gap, 5);
            assert_eq!(turns.runs, [0x0004_1101, 0, 0, 0]);
            assert_eq!(turns.omitted, 0);
        }
        assert_eq!(elf.queued, Some(300));
    }
}

#[cfg(feature = "sunfish-wlan-tx4-pressure")]
#[test]
#[cfg(not(feature = "sunfish-wlan-registry-second-reply"))]
fn tx4_pressure_retains_exact_same_peer_blocker_after_success() {
    use qcom_modem_owner::ServiceProgress;
    let (peer, mut owner, mut irqs) = owner_control_start();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);

    for txn in [3, 4] {
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        peer.push(&data(intent, &servreg_query_datagram_with_transaction(b"tms/servreg", txn)));
        owner.notified(1, &mut irqs).unwrap();
    }
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::WaitingReply));
    let timing = owner.service_registry_trace().txn4.unwrap();
    assert_eq!(timing.waits_by_reason, [0, 1, 0, 0, 0, 0]);
    let blocker = timing.last_pressure.unwrap();
    assert_eq!(blocker.reason, 1);
    assert_eq!(blocker.peer_publication.unwrap().transaction, 3);
    owner.notified(8, &mut irqs).unwrap();
    #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
    let blocker = {
        assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::WaitingReply));
        let sample = owner.service_registry_trace().txn4.unwrap().last_pressure.unwrap();
        let trace = sample.peer_publication.unwrap();
        assert_eq!(trace.transaction, 3);
        assert_eq!(trace.turn_gates[7], 1);
        assert_eq!(trace.turn_gates.iter().sum::<u32>(), trace.calls);
        sample
    };
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    assert_eq!(owner.service_registry_trace().txn4.unwrap().last_pressure, Some(blocker));
}

#[cfg(feature = "sunfish-wlan-elf-phase-io")]
#[test]
fn elf_phase_io_tracks_first_request_without_an_intervening_registry_turn() {
    use qcom_modem_owner::ServiceProgress;
    let (peer, mut owner, mut irqs) = owner_control_start();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.set_rmtfs_event_clock(|| 100);
    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
    peer.push(&data(intent, &servreg_query_datagram_with_transaction(b"kernel/elf_loader", 5)));
    owner.notified(1, &mut irqs).unwrap();
    let first = owner.service_registry_trace().elf.unwrap();
    assert!(first.io.received.unwrap()[0] < first.io.receive_pass_end.unwrap()[0]);
    assert_eq!(first.io.queued, None); assert_eq!(first.io.published, None);
    let log_before = peer.log();
    let counters = owner.with_io(|io| io.elf_io_totals()).unwrap();
    assert_eq!(peer.log(), log_before, "snapshot performs no extra IO");
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    let queued = owner.service_registry_trace().elf.unwrap();
    assert_eq!(queued.dispatch.turns.calls, 0);
    assert_eq!(queued.io.received, first.io.received);
    assert_eq!(queued.io.receive_pass_end, first.io.receive_pass_end);
    assert_eq!(queued.io.queued, owner.with_io(|io| io.elf_io_totals()));
    assert!(queued.io.queued.unwrap()[0] >= counters[0]);
    owner.notified(8, &mut irqs).unwrap();
    let published = owner.service_registry_trace().elf.unwrap();
    assert!(published.io.published.unwrap()[0] > counters[0]);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.notified(8, &mut irqs).unwrap();
    let done = owner.service_registry_trace().elf.unwrap();
    assert_eq!(done.io, published.io, "later work must not overwrite first boundaries");
    assert!(done.done.is_some());
}

#[cfg(all(feature = "sunfish-wlan-elf-dispatch-timing",
          not(feature = "sunfish-wlan-registry-second-reply")))]
#[test]
fn immediate_second_registry_turn_only_queues_elf_when_existing_peer_gate_allows_it() {
    use qcom_modem_owner::ServiceProgress;
    for elf_peer in [9u32, 10] {
        let (peer, mut owner, mut irqs) = owner_control_start();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner.enable_service_registry(&mut irqs).unwrap();
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner.set_rmtfs_event_clock(|| 100);
        for (name, txn, port) in [
            (b"tms/servreg".as_slice(), 1, 9u32),
            (b"kernel/elf_loader".as_slice(), 2, elf_peer),
        ] {
            let id = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            let mut frame = servreg_query_datagram_with_transaction(name, txn);
            frame[12..16].copy_from_slice(&port.to_le_bytes());
            peer.push(&data(id, &frame));
            owner.notified(1, &mut irqs).unwrap();
        }
        assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
        let second = owner.dispatch_service_registry_step(&mut irqs).unwrap();
        let elf = owner.service_registry_trace().elf.unwrap();
        assert_eq!(elf.dispatch.selected, Some(100), "selection alone does not prove queueing");
        if elf_peer == 9 {
            assert_eq!(second, ServiceProgress::WaitingReply);
            assert_eq!(elf.queued, None);
            assert_eq!(owner.service_registry_trace().replies_queued, 1);
            owner.notified(8, &mut irqs).unwrap();
            // Publication alone still retains the existing same-peer gate.
            assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::WaitingReply));
            assert_eq!(owner.service_registry_trace().elf.unwrap().queued, None);
            take_owner_datagrams(&peer, &mut owner, &mut irqs);
            assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
        } else {
            assert_eq!(second, ServiceProgress::ReplyQueued);
            assert_eq!(elf.queued, Some(100));
            assert_eq!(owner.service_registry_trace().replies_queued, 2);
        }
        assert_eq!(owner.service_registry_trace().elf.unwrap().queued, Some(100));
        assert!(owner.pending_request().is_none());
    }
}

#[cfg(feature = "sunfish-servreg-bounded-second-turn")]
#[test]
fn bounded_registry_dispatch_keeps_two_call_limit_and_existing_backpressure_without_a_clock() {
    use qcom_modem_owner::ServiceProgress;
    for elf_port in [9u32, 10] {
        let (peer, mut owner, mut irqs) = owner_control_start();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner.enable_service_registry(&mut irqs).unwrap();
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        // Deliberately leave the diagnostic clock absent. Only actual pending
        // handles and queue progress may determine the additional dispatch.
        for (name, txn, port) in [
            (b"tms/servreg".as_slice(), 1, 9u32),
            (b"kernel/elf_loader".as_slice(), 2, elf_port),
            (b"tms/servreg".as_slice(), 3, 11),
        ] {
            let id = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            let mut frame = servreg_query_datagram_with_transaction(name, txn);
            frame[12..16].copy_from_slice(&port.to_le_bytes());
            peer.push(&data(id, &frame));
            owner.notified(1, &mut irqs).unwrap();
        }
        let (first, second) = owner.dispatch_service_registry_bounded(&mut irqs).unwrap();
        assert_eq!(first, ServiceProgress::ReplyQueued);
        assert_eq!(second, Some(if elf_port == 9 {
            ServiceProgress::WaitingReply
        } else { ServiceProgress::ReplyQueued }));
        let trace = owner.service_registry_trace();
        assert_eq!(trace.elf.unwrap().received, None);
        assert_eq!(trace.elf.unwrap().queued, None, "an absent clock stays unavailable");
        assert_eq!(trace.replies_queued, if elf_port == 9 { 1 } else { 2 });
        assert!(owner.pending_request().is_some(), "third request cannot be drained in the same burst");
        if elf_port == 9 {
            let schedules = irqs.schedules;
            assert_eq!(owner.dispatch_service_registry_bounded(&mut irqs),
                Ok((ServiceProgress::WaitingReply, None)));
            assert_eq!(irqs.schedules, schedules, "no repeated scheduling on backpressure");
            assert_eq!(owner.service_registry_trace().replies_queued, 1);
            owner.notified(8, &mut irqs).unwrap();
            take_owner_datagrams(&peer, &mut owner, &mut irqs);
            assert_eq!(owner.dispatch_service_registry_bounded(&mut irqs),
                Ok((ServiceProgress::ReplyQueued, None)), "ELF queue success ends the extra-turn scope");
            assert_eq!(owner.service_registry_trace().replies_queued, 2);
            assert!(owner.pending_request().is_some());
        }
        owner.fault();
        let log = peer.log();
        assert!(owner.dispatch_service_registry_bounded(&mut irqs).is_err());
        assert_eq!(peer.log(), log, "terminal fault permits no additional access");
    }
}

#[cfg(any(feature = "sunfish-wlan-elf-oack-router-turn",
          all(feature = "sunfish-wlan-elf-blocker-timing",
              feature = "sunfish-qrtr-servreg-flow-experiment",
              not(feature = "sunfish-wlan-registry-second-reply"))))]
#[path = "../rust/src/pixel_mpss_runtime_irq.rs"]
mod elf_blocker_irq;

#[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
#[test]
fn post_elf_oack_uses_one_early_router_turn_and_preserves_irq_ownership() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use elf_blocker_irq::{Kernel, Router};
    use qcom_modem_owner::{ServiceProgress, TftpServiceProgress};
    use qcom_tftp_server_check::{OACK, REQUEST, ServerCheck};

    #[derive(Default)]
    struct KernelLog(Rc<RefCell<Vec<u8>>>);
    impl Kernel for KernelLog {
        fn acknowledge(&mut self, channel: u8) -> Result<(), ()> {
            self.0.borrow_mut().push(channel);
            Ok(())
        }
    }

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    let (peer, mut owner) = owner();
    owner.prepare_tftp_server(0x5000).unwrap();
    owner.start_control().unwrap();
    let mut intents = control(5, 2, 12);
    for id in 0u32..12 {
        intents.extend(4096u32.to_le_bytes());
        intents.extend(id.to_le_bytes());
    }
    peer.push(&intents);
    let mut irqs = Irqs::default();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner_control_peer_hello(&peer, &mut owner, &mut irqs);
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.enable_service_registry(&mut irqs).unwrap();
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    owner.notified(8, &mut irqs).unwrap();
    assert!(peer.take_tx().is_empty());
    owner.set_rmtfs_event_clock(clock);

    let log = KernelLog::default();
    let acks = log.0.clone();
    let mut router = Router::new(log, 1);
    let mut server = ServerCheck::new();
    let receive_write = |owner: &mut Owner<Fake>, router: &mut Router<KernelLog>, server: &mut ServerCheck, port| {
        let intent = owner.offer_receive().unwrap();
        router.dispatch(owner).unwrap();
        peer.take_tx();
        peer.push(&data(intent, &tftp_qrtr(REQUEST, port, 0x5000)));
        router.capture(1 << 4).unwrap();
        router.dispatch(owner).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(server, router),
            Ok(TftpServiceProgress::ResponseQueued));
    };
    receive_write(&mut owner, &mut router, &mut server, 0x22);
    assert!(router.report().continuation_pending);
    #[cfg(not(feature = "sunfish-wlan-first-server-check-early-turn"))]
    {
        assert_eq!(router.dispatch_queued_server_check_oack(&mut owner, clock), Ok(false));
        router.dispatch(&mut owner).unwrap();
    }
    #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
    {
        let first = |owner: &Owner<Fake>| owner.service_registry_trace().first_server_check.unwrap();
        assert!(first(&owner).before_elf);
        assert_eq!(first(&owner).oack.phase, 2);
        assert_eq!(router.first_oack_turn(), None);
        assert_eq!(router.dispatch_queued_server_check_oack(&mut owner, clock), Ok(true));
        let (entry, end) = router.first_oack_turn().unwrap();
        assert!(first(&owner).timing.queued_at.unwrap() < entry);
        assert!((entry..end).contains(&first(&owner).oack.published_at.unwrap()));
        assert_eq!(first(&owner).oack.phase, 3);
        assert_eq!(router.dispatch_queued_server_check_oack(&mut owner, clock), Ok(false));
        assert_eq!(router.first_oack_turn(), Some((entry, end)));
    }
    assert_eq!(router.elf_oack_turn(), None);
    #[cfg(not(feature = "sunfish-wlan-first-server-check-tail"))]
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
    {
        use qcom_glink_fifo::ServerCheckTail;

        let snapshot = owner.service_registry_trace();
        let ServerCheckTail::Pending(remaining) = snapshot.first_server_check_tail
        else { panic!("first OACK cursor boundary should be pending") };
        assert!(remaining > 8);
        assert_eq!(snapshot.first_server_check.unwrap().oack.phase, 3);
        let tail = peer.0.borrow().words[0] as usize;
        let records = peer.take_tx();
        let head = peer.0.borrow().words[0];
        let through_oack = owner_complete_datagrams(&records[..remaining]);
        let oack = through_oack.last().unwrap();
        assert_eq!(&oack[28..32], &0x22u32.to_le_bytes());
        assert_eq!(owner_word(oack, 12), snapshot.first_server_check.unwrap().transfer_port.unwrap());
        assert_eq!(&oack[32..32 + OACK.len()], OACK);
        // Present partial progress before any adapter sample can see the end.
        peer.0.borrow_mut().words[0] = ((tail + 8) % qcom_glink_fifo::FIFO_BYTES) as u32;
        owner.notified(8, &mut irqs).unwrap();
        assert_eq!(owner.service_registry_trace().first_server_check_tail,
            ServerCheckTail::Pending(remaining - 8));
        peer.0.borrow_mut().words[0] = ((tail + remaining) % qcom_glink_fifo::FIFO_BYTES) as u32;
        owner.notified(8, &mut irqs).unwrap();
        let snapshot = owner.service_registry_trace();
        let ServerCheckTail::Consumed(Some(observed_at)) = snapshot.first_server_check_tail
        else { panic!("checked cursor sample should show OACK consumption") };
        assert_eq!(snapshot.first_server_check.unwrap().oack.phase, 3);
        peer.0.borrow_mut().words[0] = head;
        #[cfg(all(feature = "sunfish-wlan-first-server-check-repeat",
            not(feature = "sunfish-wlan-first-server-check-retry")))]
        {
            let intent = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap();
            peer.take_tx();
            peer.push(&data(intent, &tftp_qrtr(REQUEST, 0x22, 0x5000)));
            owner.notified(1, &mut irqs).unwrap();
            assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs),
                Ok(TftpServiceProgress::WaitingReply));
            let repeat = owner.service_registry_trace().first_server_check.unwrap().repeat.unwrap();
            assert!(!repeat.coalesced);
            assert!(repeat.dispatch_at.is_some());
            assert_eq!(repeat.result, Some(0));
            assert_eq!(repeat.oack.phase, 1);
            assert_eq!(repeat.flow_busy, 1);
            assert_eq!(repeat.no_capacity, 0);
            assert_eq!(repeat.queued_at, None);
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
        {
            // The original still owns its remote intent after the peer tail
            // crosses it. The retransmission gets its own intent and RXDONE.
            receive_write(&mut owner, &mut router, &mut server, 0x22);
            let repeat = |owner: &Owner<Fake>| owner.service_registry_trace().first_server_check.unwrap().repeat.unwrap();
            assert!(repeat(&owner).overlapped);
            assert_eq!(repeat(&owner).flow_busy, 0);
            assert_eq!(repeat(&owner).oack.phase, 2);
            assert_eq!(router.dispatch_queued_tftp_retry(&mut owner), Ok(true));
            assert!(router.retry_oack_turn());
            assert_eq!(repeat(&owner).oack.phase, 3);
            assert_eq!(router.dispatch_queued_tftp_retry(&mut owner), Ok(false));
            let retry_records = peer.take_tx();
            let replies: Vec<_> = owner_complete_datagrams(&retry_records).into_iter()
                .filter(|packet| owner_word(packet, 4) == 1 && owner_word(packet, 28) == 0x22)
                .collect();
            assert_eq!(replies.len(), 1);
            assert_eq!(owner_word(&replies[0], 12), owner_word(oack, 12));
            assert_eq!(owner_word(&replies[0], 28), owner_word(oack, 28));
            assert_eq!(&replies[0][32..32 + OACK.len()], OACK);
            acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &retry_records);
            assert_eq!(repeat(&owner).oack.phase, 4);
            assert_eq!(owner.service_registry_trace().first_server_check.unwrap().oack.phase, 3);
            assert!(repeat(&owner).oack.done_at > repeat(&owner).oack.published_at);
        }
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &records);
        let completed = owner.service_registry_trace();
        assert_eq!(completed.first_server_check_tail, ServerCheckTail::Consumed(Some(observed_at)));
        assert!(completed.first_server_check.unwrap().oack.done_at.unwrap() > observed_at);
        #[cfg(all(feature = "sunfish-wlan-first-server-check-repeat",
            not(feature = "sunfish-wlan-first-server-check-retry")))]
        {
            assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs),
                Ok(TftpServiceProgress::ResponseQueued));
            let repeat = owner.service_registry_trace().first_server_check.unwrap().repeat.unwrap();
            assert_eq!(repeat.oack.phase, 2);
            assert!(repeat.queued_at > repeat.dispatch_at);
            owner.notified(8, &mut irqs).unwrap();
            assert_eq!(owner.service_registry_trace().first_server_check.unwrap().repeat.unwrap().oack.phase, 3);
            take_owner_datagrams(&peer, &mut owner, &mut irqs);
            let repeat = owner.service_registry_trace().first_server_check.unwrap().repeat.unwrap();
            assert_eq!(repeat.oack.phase, 4);
            assert!(repeat.oack.done_at > repeat.oack.published_at);
            owner.notified(8, &mut irqs).unwrap();
            assert!(peer.take_tx().is_empty());
        }
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    let first_completed = {
        let first = owner.service_registry_trace().first_server_check.unwrap();
        assert!(first.before_elf);
        assert_eq!(first.peer.port, 0x22);
        assert_eq!(first.wrq_result, Some(0));
        assert_eq!(first.oack.phase, 4);
        assert!(owner.service_registry_trace().elf_exchange.is_none());
        let intent = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        let packet = tftp_qrtr(b"\x00\x03\x00\x01test\n", 0x22, first.transfer_port.unwrap());
        peer.push(&data(intent, &packet));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs),
            Ok(TftpServiceProgress::ResponseQueued));
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner.notified(8, &mut irqs).unwrap();
        assert!(peer.take_tx().is_empty());
        let completed = owner.service_registry_trace().first_server_check.unwrap();
        assert_eq!(completed.data.unwrap().result, Some(0));
        assert_eq!(completed.timing.peer_rx.data.count, 1);
        assert_eq!(completed.final_block, Some(1));
        assert_eq!(completed.ack.phase, 4);
        #[cfg(feature="sunfish-first-data-ack-elf-trace")]
        assert_eq!(owner.original_tftp_write_trace(), Some(completed));
        assert!(completed.ack.done_at > completed.oack.done_at);
        #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
        {
            let ack = owner.tftp_final_ack_trace().unwrap();
            assert_eq!(ack.destination.port, completed.peer.port);
            assert_eq!(Some(ack.source.port), completed.transfer_port);
            assert_eq!(ack.flags, 0x1f);
            assert!(ack.accepted_at.unwrap() < ack.published_at.unwrap());
            assert!(ack.published_at.unwrap() < ack.done_at.unwrap());
        }
        completed
    };

    let intent = owner.offer_receive().unwrap();
    owner.notified(8, &mut irqs).unwrap();
    peer.take_tx();
    peer.push(&data(intent, &servreg_query_datagram_with_transaction(b"kernel/elf_loader", 5)));
    owner.notified(1, &mut irqs).unwrap();
    assert_eq!(owner.dispatch_service_registry_step(&mut irqs), Ok(ServiceProgress::ReplyQueued));
    owner.notified(8, &mut irqs).unwrap();
    take_owner_datagrams(&peer, &mut owner, &mut irqs);
    assert!(owner.service_registry_trace().elf_exchange.unwrap().completed);
    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    {
        let elf = owner.service_registry_trace().elf.unwrap();
        assert!(owner.tftp_final_ack_trace().unwrap().done_at.unwrap() < elf.received.unwrap());
        assert!(elf.received.unwrap() < elf.published.unwrap());
        assert!(elf.published.unwrap() < elf.done.unwrap());
    }
    owner.notified(8, &mut irqs).unwrap();
    assert!(peer.take_tx().is_empty());

    receive_write(&mut owner, &mut router, &mut server, 0x21);
    let write = |owner: &Owner<Fake>| owner.service_registry_trace().elf_exchange.unwrap().server_check.unwrap();
    assert_eq!(write(&owner).oack.phase, 2);
    assert!(router.report().continuation_pending);
    assert_eq!(router.dispatch_queued_server_check_oack(&mut owner, clock), Ok(true));
    let (entry, end) = router.elf_oack_turn().unwrap();
    let published = write(&owner);
    assert!(published.timing.queued_at.unwrap() < entry);
    assert!((entry..end).contains(&published.timing.queue.first_at.unwrap()));
    assert!((entry..end).contains(&published.oack.published_at.unwrap()));
    assert_eq!(published.oack.phase, 3);
    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    {
        assert!(!published.before_elf);
        assert_eq!(owner.service_registry_trace().first_server_check, Some(first_completed));
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        assert!(matches!(owner.service_registry_trace().first_server_check_tail,
            qcom_glink_fifo::ServerCheckTail::Consumed(Some(_))));
    }
    let datagrams = owner_complete_datagrams(&peer.take_tx());
    let replies: Vec<_> = datagrams.iter()
        .filter(|packet| packet.get(4..8) == Some(&1u32.to_le_bytes())
            && packet.get(28..32) == Some(&0x21u32.to_le_bytes()))
        .collect();
    assert_eq!(replies.len(), 1);
    let body_len = u32::from_le_bytes(replies[0][20..24].try_into().unwrap()) as usize;
    assert_eq!(&replies[0][32..32 + body_len], OACK);
    assert!(replies[0][32 + body_len..].iter().all(|byte| *byte == 0));
    assert_eq!(router.dispatch_queued_server_check_oack(&mut owner, clock), Ok(false));
    assert_eq!(router.elf_oack_turn(), Some((entry, end)));
    for _ in 0..8 {
        if router.report().pending_modem == 0 { break; }
        router.dispatch(&mut owner).unwrap();
    }
    #[cfg(not(feature = "sunfish-wlan-first-server-check-retry"))]
    assert_eq!(&*acks.borrow(), &[4, 4]);
    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    assert_eq!(&*acks.borrow(), &[4, 4, 4]);

    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    {
        use qcom_qrtr_control::{WLFW_SERVICE_ID, WLFW_VERSION_INSTANCE};

        let elf_done = owner.service_registry_trace().elf.unwrap().done.unwrap();
        assert_eq!(owner.wlan_discovery_times(), (None, None));
        let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", &[]);
        let announcement = owner_control_frame(4, [WLFW_SERVICE_ID, WLFW_VERSION_INSTANCE, 0, 0x5b]);
        let packets = [tftp_qrtr(&rrq, 0x35, 0x5000), announcement];
        let mut first_discovery = None;
        for (index, packet) in packets.iter().cycle().take(4).enumerate() {
            let intent = owner.offer_receive().unwrap();
            owner.notified(8, &mut irqs).unwrap();
            acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &peer.take_tx());
            peer.push(&data(intent, packet));
            owner.notified(1, &mut irqs).unwrap();
            if index == 1 {
                first_discovery = Some(owner.wlan_discovery_times());
            }
        }
        let (rrq, service) = owner.wlan_discovery_times();
        assert_eq!(Some((rrq, service)), first_discovery);
        assert_eq!(owner.tftp_wire_counts()[2], 2);
        assert!(elf_done < rrq.unwrap());
        assert!(rrq.unwrap() < service.unwrap());
    }
}

#[cfg(feature = "sunfish-wlan-first-server-check-retry")]
#[test]
fn server_check_retry_close_cancels_only_unpublished_oack() {
    use qcom_modem_owner::TftpServiceProgress::{Idle, ResponseQueued, WaitingReply};
    use qcom_tftp_server_check::{REQUEST, ServerCheck};

    for consume_original in [false, true] {
        let (peer, mut owner) = owner();
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        static CLOCK: AtomicU64 = AtomicU64::new(100);
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        {
            CLOCK.store(100, Relaxed);
            owner.set_rmtfs_event_clock(|| CLOCK.load(Relaxed));
        }
        let mut irqs = Irqs::default();
        let mut server = ServerCheck::new();
        owner.prepare_tftp_server(0x5000).unwrap();
        owner.start_control().unwrap();
        let mut intents = control(5, 2, 12);
        for id in 0u32..12 {
            intents.extend(4096u32.to_le_bytes());
            intents.extend(id.to_le_bytes());
        }
        peer.push(&intents);
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let first = owner.offer_receive().unwrap();
        let repeat = owner.offer_receive().unwrap();
        let close = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();

        let request = tftp_qrtr(REQUEST, 0x1f, 0x5000);
        peer.push(&data(first, &request));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(ResponseQueued));
        owner.notified(8, &mut irqs).unwrap();
        let tail = peer.0.borrow().words[0];
        let original = peer.take_tx();
        if !consume_original { peer.0.borrow_mut().words[0] = tail; }
        peer.push(&data(repeat, &request));
        owner.notified(1, &mut irqs).unwrap();
        let expected = if consume_original { ResponseQueued } else { WaitingReply };
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(expected));
        let trace = |owner: &Owner<Fake>| owner.service_registry_trace().first_server_check.unwrap();
        assert_eq!(trace(&owner).repeat.unwrap().overlapped, consume_original);

        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        CLOCK.store(200, Relaxed);
        peer.push(&data(close, &owner_control_frame(6, [0, 0x1f, 0, 0])));
        owner.notified(1, &mut irqs).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(Idle));
        let write = trace(&owner);
        let retry = write.repeat.unwrap();
        assert!(retry.cancelled);
        assert_eq!(retry.close_phase, Some(if consume_original { 2 } else { 1 }));
        assert_eq!(retry.oack.published_at, None);
        assert_eq!(write.oack.phase, 3);
        assert_eq!(owner.tftp_queue_snapshot().0.tx_count, 1);
        assert!(!owner.tftp_queue_snapshot().1);

        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        {
            assert_eq!(owner.first_oack_rx_done(), None);
            CLOCK.store(300, Relaxed);
        }
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &original);
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        {
            let done = owner.first_oack_rx_done().unwrap();
            assert_eq!(done.last_absent, Some(200));
            assert_eq!(done.first_visible, Some(300));
            assert_eq!(done.visible_at_close, Some(false));
            assert!(done.bytes_ahead.is_some());
        }
        owner.notified(8, &mut irqs).unwrap();
        let emitted = peer.take_tx();
        let emitted = if consume_original { emitted.as_slice() } else { &emitted[original.len()..] };
        assert!(!owner_complete_datagrams(emitted).iter().any(|packet|
            owner_word(packet, 4) == 1 && owner_word(packet, 28) == 0x1f));
        assert_eq!(trace(&owner).oack.phase, 4);
        assert_eq!(trace(&owner).repeat.unwrap().oack.done_at, None);
        assert_eq!(owner.tftp_queue_snapshot().0.tx_count, 0);
    }
}

#[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
#[test]
fn server_check_retry_window_distinguishes_close_control_and_missing_intent() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use elf_blocker_irq::{Kernel, Router};
    use qcom_modem_owner::TftpServiceProgress::ResponseQueued;
    use qcom_tftp_server_check::{OACK, REQUEST, ServerCheck};

    struct TestKernel;
    impl Kernel for TestKernel {
        fn acknowledge(&mut self, channel: u8) -> Result<(), ()> {
            assert_eq!(channel, 4);
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Window { CloseFirst, ControlFirst, NoFittingIntent }

    static CLOCK: AtomicU64 = AtomicU64::new(0);
    const QUEUE_AT: u64 = 1_628_569;
    const CLOSE_AT: u64 = QUEUE_AT + 52_978;
    for scenario in [Window::CloseFirst, Window::ControlFirst, Window::NoFittingIntent] {
        let (peer, mut owner) = owner();
        let mut irqs = Irqs::default();
        let mut server = ServerCheck::new();
        owner.set_rmtfs_event_clock(|| CLOCK.load(Relaxed));
        owner.prepare_tftp_server(0x5000).unwrap();
        owner.start_control().unwrap();
        let mut intents = control(5, 2, 12);
        for id in 0u32..12 {
            // Control datagrams fit in 64 bytes. The original OACK uses the
            // sole larger intent in the no-fit case and holds its completion.
            let capacity = if scenario == Window::NoFittingIntent && id != 0 { 64u32 } else { 4096 };
            intents.extend(capacity.to_le_bytes());
            intents.extend(id.to_le_bytes());
        }
        peer.push(&intents);
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let first = owner.offer_receive().unwrap();
        let repeat = owner.offer_receive().unwrap();
        let close = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        let mut router = Router::new(TestKernel, 1);

        let request = owner_qrtr_packet(1, 0x1f, 0x5000, false, REQUEST);
        CLOCK.store(0, Relaxed);
        peer.push(&data(first, &request));
        router.capture(1 << 4).unwrap();
        router.dispatch(&mut owner).unwrap();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut router), Ok(ResponseQueued));
        CLOCK.store(361_071, Relaxed);
        router.dispatch(&mut owner).unwrap();
        assert_eq!(router.report().pending_modem, 0);
        let original = owner_complete_datagrams(&peer.take_tx());
        assert!(original.iter().any(|packet| packet.get(32..32 + OACK.len()) == Some(OACK)));
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        {
            let first = owner.server_check_oacks()[0].unwrap();
            assert_eq!(first.wrq_at, Some(0));
            assert_eq!(first.oack_at, Some(361_071));
        }

        CLOCK.store(1_490_039, Relaxed);
        peer.push(&data(repeat, &request));
        router.capture(1 << 4).unwrap();
        router.dispatch(&mut owner).unwrap();
        CLOCK.store(QUEUE_AT, Relaxed);
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut router), Ok(ResponseQueued));
        let trace = |owner: &Owner<Fake>| owner.service_registry_trace().first_server_check.unwrap();
        let retry = trace(&owner).repeat.unwrap();
        assert!(retry.overlapped);
        assert_eq!(retry.queued_at, Some(QUEUE_AT));
        assert_eq!(retry.tx.calls, 0);

        let close = data(close, &owner_control_frame(6, [0, 0x1f, 0, 0]));
        if scenario == Window::ControlFirst {
            // An oversize peer request creates a real GLINK denial ACK.
            peer.push(&control(7, 2, (qcom_glink_qmi::MAX_DATAGRAM_BYTES + 1) as u32));
        }
        let turn_at = if scenario == Window::NoFittingIntent {
            QUEUE_AT + 1
        } else {
            peer.push(&close);
            CLOSE_AT
        };
        CLOCK.store(turn_at, Relaxed);
        assert_eq!(router.dispatch_queued_tftp_retry(&mut owner), Ok(true), "{scenario:?}");
        let first_turn = trace(&owner).repeat.unwrap();
        assert_eq!(first_turn.tx.turn_at, Some(turn_at));
        assert_eq!(first_turn.tx.first_poll_end, Some(turn_at));
        match scenario {
            Window::CloseFirst => {
                assert_eq!(first_turn.tx.calls, 0);
                assert_eq!(first_turn.tx.first, None);
                assert_eq!(first_turn.tx.first_call, None);
            }
            Window::ControlFirst => {
                assert_eq!(first_turn.tx.calls, 1);
                assert_eq!(first_turn.tx.first, Some(4));
                assert_eq!(first_turn.tx.seen, 1 << 3);
            }
            Window::NoFittingIntent => {
                assert!(!first_turn.cancelled);
                assert!(first_turn.tx.calls > 0);
                assert_eq!(first_turn.tx.first_call, Some(turn_at));
                assert_eq!(first_turn.tx.first.unwrap() & 0x100, 0x100);
                assert_eq!(first_turn.tx.seen & (1 << 5), 0, "retry was never selected");
                CLOCK.store(CLOSE_AT, Relaxed);
                peer.push(&close);
                router.capture(1 << 4).unwrap();
                router.dispatch(&mut owner).unwrap();
            }
        }
        let write = trace(&owner);
        let retry = write.repeat.unwrap();
        assert_eq!(write.timing.first_close_at, Some(CLOSE_AT));
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        {
            let [Some(first), None] = owner.server_check_oacks() else { panic!("unexpected completed transfer"); };
            assert_eq!(first.first_repeat_at, Some(1_490_039));
            assert_eq!(first.first_close_at, Some(CLOSE_AT));
            assert_eq!(first.first_data_at, None);
            assert_eq!(first.oack_done_at, None);
        }
        assert_eq!(write.oack.phase, 3);
        assert!(retry.cancelled);
        assert_eq!(retry.close_phase, Some(2));
        assert_eq!(retry.oack.published_at, None);
        assert_eq!(retry.tx, first_turn.tx);
    }
}

#[cfg(all(feature = "sunfish-wlan-elf-blocker-timing",
          feature = "sunfish-qrtr-servreg-flow-experiment",
          not(feature = "sunfish-wlan-registry-second-reply")))]
#[test]
fn earlier_router_turn_publishes_exact_elf_blocker_and_preserves_its_completion_gate() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use elf_blocker_irq::{Kernel, Router};
    use qcom_modem_owner::ServiceProgress;

    struct KernelTrace<'a>(&'a mut Vec<u8>);
    impl Kernel for KernelTrace<'_> {
        fn acknowledge(&mut self, channel: u8) -> Result<(), ()> {
            self.0.push(channel);
            Ok(())
        }
    }

    static CLOCK: AtomicU64 = AtomicU64::new(0);
    let mut queue_times = Vec::new();
    for early in [false, true] {
        let (peer, mut owner, mut setup) = owner_control_start();
        take_owner_datagrams(&peer, &mut owner, &mut setup);
        owner_control_peer_hello(&peer, &mut owner, &mut setup);
        take_owner_datagrams(&peer, &mut owner, &mut setup);
        owner.enable_service_registry(&mut setup).unwrap();
        owner.notified(8, &mut setup).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut setup);
        // Consume the fixture's pending continuation before switching to Router.
        owner.notified(8, &mut setup).unwrap();
        assert!(peer.take_tx().is_empty());
        owner.set_rmtfs_event_clock(|| CLOCK.load(Relaxed));

        // Capture a single sent record and retain its remote-intent ID.
        let take_reply = |port: u32, transaction: u16| {
            let records = peer.take_tx();
            let datagrams = owner_datagrams(&records);
            assert_eq!(datagrams.len(), 1);
            let packet = &datagrams[0];
            assert_eq!(&packet[28..32], &port.to_le_bytes());
            assert_eq!(&packet[33..35], &transaction.to_le_bytes());
            let mut at = 0;
            while at < records.len() {
                let command = u16::from_le_bytes(records[at..at + 2].try_into().unwrap());
                let parameter = u32::from_le_bytes(records[at + 4..at + 8].try_into().unwrap());
                match command {
                    9 => return parameter,
                    5 => at += 8 + parameter as usize * 8,
                    6 | 8 | 13 => at += 8,
                    _ => panic!("unexpected GLINK command {command}"),
                }
            }
            panic!("missing reply data");
        };
        let mut acknowledgements = Vec::new();
        let mut router = Router::new(KernelTrace(&mut acknowledgements), 1);
        let receive = |owner: &mut Owner<Fake>,
                       router: &mut Router<KernelTrace<'_>>,
                       name: &[u8],
                       transaction,
                       port: u32| {
            let intent = owner.offer_receive().unwrap();
            router.dispatch(owner).unwrap();
            assert!(owner_datagrams(&peer.take_tx()).is_empty());
            let mut packet = servreg_query_datagram_with_transaction(name, transaction);
            packet[12..16].copy_from_slice(&port.to_le_bytes());
            peer.push(&data(intent, &packet));
            router.capture(1 << 4).unwrap();
            router.dispatch(owner).unwrap();
            assert!(owner_datagrams(&peer.take_tx()).is_empty());
        };

        CLOCK.store(50, Relaxed);
        receive(&mut owner, &mut router, b"tms/servreg", 3, 10);
        assert_eq!(owner.dispatch_service_registry_step(&mut router), Ok(ServiceProgress::ReplyQueued));
        router.dispatch(&mut owner).unwrap();
        let other_intent = take_reply(10, 3);
        receive(&mut owner, &mut router, b"tms/servreg", 3, 9);
        receive(&mut owner, &mut router, b"kernel/elf_loader", 5, 9);

        CLOCK.store(100, Relaxed);
        assert_eq!(owner.dispatch_service_registry_bounded(&mut router),
            Ok((ServiceProgress::ReplyQueued, Some(ServiceProgress::WaitingReply))));
        let initial = owner.service_registry_trace().elf.unwrap().blocker.unwrap();
        assert_eq!((initial.transaction, initial.flags_at_wait, initial.wait), (3, 8, Some(100)));
        assert_eq!((initial.published, initial.done), (None, None));
        assert!(router.report().continuation_pending);

        // Model a fixed interval spent on other services. Only the
        // location of the already scheduled router turn changes.
        let publish_time = if early { 110 } else { 310 };
        CLOCK.store(publish_time, Relaxed);
        router.dispatch(&mut owner).unwrap();
        let blocker_intent = take_reply(9, 3);
        assert_ne!(blocker_intent, other_intent);
        let published = owner.service_registry_trace().elf.unwrap().blocker.unwrap();
        assert_eq!((published.published, published.done), (Some(publish_time), None));
        assert_eq!(published.first_turn, Some(publish_time));
        assert_eq!(owner.dispatch_service_registry_step(&mut router), Ok(ServiceProgress::WaitingReply));

        // A valid completion with the same QMI transaction at a
        // different port cannot complete this blocker or admit ELF.
        let completion_with_backlog = |completed, advertised: u32| {
            let mut records = control(6, 2, completed).to_vec();
            records.extend(control(5, 2, 1));
            records.extend(4096u32.to_le_bytes());
            records.extend(advertised.to_le_bytes());
            records
        };
        CLOCK.store(publish_time + 10, Relaxed);
        peer.push(&completion_with_backlog(other_intent, 0x90));
        router.capture(1 << 4).unwrap();
        let unrelated = router.dispatch(&mut owner).unwrap();
        assert_eq!(unrelated.received, 2, "unrelated RXDONE does not yield");
        assert_eq!(owner.service_registry_trace().elf.unwrap().blocker, Some(published));
        assert_eq!(owner.dispatch_service_registry_step(&mut router), Ok(ServiceProgress::WaitingReply));

        CLOCK.store(publish_time + 20, Relaxed);
        peer.push(&completion_with_backlog(blocker_intent, 0x91));
        router.capture(1 << 4).unwrap();
        let acknowledgements_before = router.report().acknowledgements;
        let matching = router.dispatch(&mut owner).unwrap();
        if cfg!(feature = "sunfish-wlan-elf-completion-turn") {
            assert_eq!(matching.received, 1, "the next record stays in the FIFO");
            assert_eq!(matching.acknowledged, 0, "the physical IRQ stays owned");
            assert_eq!(router.report().pending_modem, 1 << 4);
            assert_eq!(router.report().acknowledgements, acknowledgements_before);
            assert!(router.report().continuation_pending);
        } else {
            assert_eq!(matching.received, 2, "the regular pass drains the backlog");
            assert_eq!(router.report().pending_modem, 0);
        }
        assert_eq!(owner.dispatch_service_registry_bounded(&mut router),
            Ok((ServiceProgress::ReplyQueued, None)));
        let elf = owner.service_registry_trace().elf.unwrap();
        let blocker = elf.blocker.unwrap();
        assert_eq!(blocker.done, Some(publish_time + 20));
        assert_eq!(elf.queued, blocker.done);
        #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
        {
            assert!(blocker.yielded);
            assert_eq!(blocker.yielded_at, blocker.done);
        }
        queue_times.push(elf.queued.unwrap());
        assert!(owner_datagrams(&peer.take_tx()).is_empty(), "registry queueing alone does not publish");
        let resumed = router.dispatch(&mut owner).unwrap();
        assert_eq!(resumed.received, u32::from(cfg!(feature = "sunfish-wlan-elf-completion-turn")));
        assert_eq!(router.report().pending_modem, 0);
        assert_eq!(router.report().acknowledgements, acknowledgements_before + 1);
        let _elf_intent = take_reply(9, 5);
        assert_eq!(owner.service_registry_trace().elf.unwrap().blocker, Some(blocker));
        assert!(router.report().error.is_none());
        let expected_acknowledgements = if cfg!(feature = "sunfish-wlan-elf-credit-trace") {
            14
        } else if cfg!(feature = "sunfish-wlan-elf-exchange-trace") {
            12
        } else {
            5
        };
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        {
            use qcom_glink_qmi::ElfQrtrHeader;
            use qcom_qmi_router::Address;

            let trace = owner.service_registry_trace().elf_exchange.unwrap();
            assert_eq!(trace.source, Address { node: 1, port: qcom_servreg::LOCAL_PORT });
            assert_eq!(trace.destination, Address { node: 0, port: 9 });
            assert_eq!(trace.transaction, 5);
            assert_eq!(trace.wire_bytes, 88);
            assert!(trace.qmi_matches_reference);
            assert!(!trace.completed);
            assert_eq!(trace.following, [None; 4]);
            #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
            {
                let host = trace.host_credit.unwrap();
                assert!(!host.requested);
                assert_eq!(host.before_reply, Some([false, false]));
                assert_eq!(host.remote_intent, None);
                assert!(!host.completed);
            }

            let receive_packet = |owner: &mut Owner<Fake>, router: &mut Router<KernelTrace<'_>>, packet: &[u8]| {
                let intent = owner.offer_receive().unwrap();
                router.dispatch(owner).unwrap();
                peer.take_tx();
                peer.push(&data(intent, packet));
                router.capture(1 << 4).unwrap();
                router.dispatch(owner).unwrap();
            };
            receive_packet(&mut owner, &mut router, &owner_control_frame(9, [0; 4]));
            assert_eq!(owner.service_registry_trace().elf_exchange, Some(trace),
                "datagrams before matching RXDONE are outside the capture");

            peer.push(&control(6, 2, _elf_intent));
            router.capture(1 << 4).unwrap();
            router.dispatch(&mut owner).unwrap();
            assert!(owner.service_registry_trace().elf_exchange.unwrap().completed);
            for kind in [8, 8, 9, 8, 9] {
                receive_packet(&mut owner, &mut router, &owner_control_frame(kind, [0; 4]));
            }
            let trace = owner.service_registry_trace().elf_exchange.unwrap();
            assert_eq!(trace.following, [8, 8, 9, 8].map(|kind| Some(ElfQrtrHeader {
                meta: 0x0100_0034 | kind << 16,
                source_port: qcom_qrtr_control::CONTROL_PORT,
                destination_port: qcom_qrtr_control::CONTROL_PORT,
            })));
            #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
            {
                assert!(trace.credit.first_data.is_none());
                receive_packet(&mut owner, &mut router, &owner_control_frame(4, [71, 1, 0, 57]));
                receive(&mut owner, &mut router, b"tms/servreg", 6, 11);
                let observed = owner.service_registry_trace().elf_exchange.unwrap();
                assert_eq!(observed.following, trace.following);
                assert_eq!(observed.credit.resumes, [0; 3]);
                assert_eq!(observed.credit.servers[0], Some(qcom_qrtr_control::Server {
                    service: 71,
                    instance: 1,
                    address: Address { node: 0, port: 57 },
                }));
                assert_eq!(observed.credit.first_data, Some(ElfQrtrHeader {
                    meta: 0x0101_0038,
                    source_port: 11,
                    destination_port: qcom_servreg::LOCAL_PORT,
                }));
            }
        }
        assert_eq!(acknowledgements.as_slice(), vec![4; expected_acknowledgements]);
    }
    assert_eq!(queue_times, [330, 130], "only the modeled service delay moved");
}


#[cfg(feature = "sunfish-wlan-elf-credit-trace")]
#[test]
fn elf_credit_trace_counts_only_its_flow_across_publication_and_completion() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use qcom_glink_qmi::ElfResumeObservation;

    static CLOCK: AtomicU64 = AtomicU64::new(0);
    let mut session = session();
    let locator = qcom_servreg::LOCAL_PORT;
    session.register(1, Service::ServiceRegistry, locator).unwrap();
    session.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    session.set_registry_ack_clock(|| CLOCK.load(Relaxed));
    let receive_intents: [u32; 7] = core::array::from_fn(|_| {
        let intent = session.offer_receive(1).unwrap();
        assert_eq!(session.publish_next(1, |_| Ok(())).unwrap(), Some(Published::ReceiveIntent(intent)));
        intent
    });
    let [elf, other, pre_pub, other_credit, pre_done, post_done, alternate_header] = receive_intents;
    let queue = |session: &mut Session, intent, peer: u32| {
        let mut request = servreg_query_datagram_with_transaction(b"kernel/elf_loader", 5);
        request[12..16].copy_from_slice(&peer.to_le_bytes());
        let Event::Request(handle) = session.ingest(1, &data(intent, &request)).unwrap()
        else { panic!("expected registry request") };
        let response = qcom_servreg::respond(qcom_servreg::Query {
            service_name: b"kernel/elf_loader",
            domain_offset: 0,
        });
        session.queue_reply(&handle, Reply::Success { optional_tlvs: response.optional_tlvs() }).unwrap();
        handle
    };
    let handle = queue(&mut session, elf, 9);
    session.observe_elf_reply(&handle);
    queue(&mut session, other, 10);

    let credit = |session: &mut Session, intent, source, local, peer| {
        let bytes = [1u32, 7, 0, source, 0, 20, 1, local, 7, 0, peer, 0, 0]
            .iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>();
        assert_eq!(session.ingest(1, &data(intent, &bytes)), Ok(Event::CreditReceived));
    };
    CLOCK.store(10, Relaxed);
    credit(&mut session, pre_pub, 9, qcom_qrtr_control::CONTROL_PORT, 9);
    let first = session.elf_exchange().unwrap();
    assert!(!first.credit.published);
    assert_eq!(first.credit.resumes, [1, 0, 0]);
    assert_eq!(first.credit.applied, 1);
    assert_eq!(first.credit.first_resume, Some(ElfResumeObservation {
        source_port: 9,
        local_port: qcom_qrtr_control::CONTROL_PORT,
        observed_at: Some(10),
    }));

    let mut advertisement = control(5, 2, 1);
    advertisement.extend(4096u32.to_le_bytes());
    advertisement.extend(7u32.to_le_bytes());
    assert_eq!(session.ingest(1, &advertisement), Ok(Event::RemoteIntents));
    for _ in 0..8 {
        if matches!(session.publish_next(1, |_| Ok(())).unwrap(), Some(Published::Data { .. })) {
            break;
        }
    }
    let published = session.elf_exchange().unwrap();
    assert!(published.credit.published);
    assert!(!published.completed);
    CLOCK.store(20, Relaxed);
    credit(&mut session, other_credit, 10, locator, 10);
    assert_eq!(session.elf_exchange(), Some(published));
    credit(&mut session, pre_done, 9, locator, 9);
    assert_eq!(session.elf_exchange().unwrap().credit.resumes, [1, 1, 0]);

    assert_eq!(session.ingest(1, &control(6, 2, 7)), Ok(Event::ReplyRxDone(handle)));
    CLOCK.store(30, Relaxed);
    credit(&mut session, post_done, 9, locator, 9);
    let control_port = qcom_qrtr_control::CONTROL_PORT;
    credit(&mut session, alternate_header, control_port, control_port, 9);
    let complete = session.elf_exchange().unwrap();
    assert!(complete.completed);
    assert_eq!(complete.credit.resumes, [1, 1, 2]);
    let control_resume_enabled = cfg!(feature = "sunfish-qrtr-control-port-resume");
    assert_eq!(complete.credit.applied, 3 + u32::from(control_resume_enabled));
    assert_eq!(complete.credit.first_resume, first.credit.first_resume);
}

#[cfg(feature = "sunfish-wlan-elf-credit-trace")]
#[test]
fn control_port_resume_restores_only_the_named_destination_send_capacity() {
    use qcom_glink_qmi::Error;
    use qcom_qrtr_flow::HIGH_WATERMARK;

    let mut session = session();
    session.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    session.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    let mut advertisement = control(5, 2, 1);
    advertisement.extend(4096u32.to_le_bytes());
    advertisement.extend(7u32.to_le_bytes());
    assert_eq!(session.ingest(1, &advertisement), Ok(Event::RemoteIntents));

    let offer = |session: &mut Session| {
        let intent = session.offer_receive(1).unwrap();
        assert_eq!(session.publish_next(1, |_| Ok(())), Ok(Some(Published::ReceiveIntent(intent))));
        intent
    };
    let receive_request = |session: &mut Session, port: u32, transaction| {
        let intent = offer(session);
        let mut request = servreg_query_datagram_with_transaction(b"kernel/elf_loader", transaction);
        request[12..16].copy_from_slice(&port.to_le_bytes());
        let Event::Request(handle) = session.ingest(1, &data(intent, &request)).unwrap()
        else { panic!("expected registry request") };
        assert_eq!(session.publish_next(1, |_| Ok(())), Ok(Some(Published::ReceiveDone(intent))));
        handle
    };
    let reply = qcom_servreg::respond(qcom_servreg::Query {
        service_name: b"kernel/elf_loader",
        domain_offset: 0,
    });
    let queue = |session: &mut Session, handle: &_| {
        session.queue_reply(handle, Reply::Success { optional_tlvs: reply.optional_tlvs() })
    };
    let publish = |session: &mut Session| {
        assert_eq!(session.publish_next(1, |_| Ok(())), Ok(Some(Published::Data {
            remote_intent: 7, last_fragment: true,
        })));
    };
    let done = control(14, 2, 7);
    for port in [10, 9] {
        for transaction in 1..=HIGH_WATERMARK {
            let handle = receive_request(&mut session, port, u16::from(transaction));
            queue(&mut session, &handle).unwrap();
            publish(&mut session);
            if port != 9 || transaction != HIGH_WATERMARK {
                assert_eq!(session.ingest(1, &done), Ok(Event::ReplyRxDone(handle)));
            }
        }
    }
    let next_elf = receive_request(&mut session, 9, 11);
    let next_other = receive_request(&mut session, 10, 11);
    assert_eq!(queue(&mut session, &next_elf), Err(Error::FlowBusy));
    assert_eq!(queue(&mut session, &next_other), Err(Error::FlowBusy));

    // The credit path reuses the RX intent only after publishing RXDONE.
    let credit = |session: &mut Session, frame: &[u8]| {
        let intent = offer(session);
        assert_eq!(session.ingest(1, &data(intent, frame)), Ok(Event::CreditReceived));
        assert_eq!(session.publish_next(1, |_| Ok(())), Ok(Some(Published::ReceiveDone(intent))));
    };
    let packet = owner_control_frame(7, [0, 9, 0, 0]);
    let mut wrong_header = packet.clone();
    wrong_header[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());
    credit(&mut session, &wrong_header);
    credit(&mut session, &owner_control_frame(7, [1, 9, 0, 0]));
    assert_eq!(queue(&mut session, &next_elf), Err(Error::FlowBusy));

    credit(&mut session, &packet);
    assert_eq!(session.in_flight(), 3);
    assert!(matches!(session.ingest(1, &done), Ok(Event::ReplyRxDone(_))));
    assert_eq!(session.in_flight(), 2);
    assert_eq!(queue(&mut session, &next_other), Err(Error::FlowBusy));
    if cfg!(feature = "sunfish-qrtr-control-port-resume") {
        assert_eq!(queue(&mut session, &next_elf), Ok(()));
        publish(&mut session);
        assert_eq!(session.ingest(1, &done), Ok(Event::ReplyRxDone(next_elf)));
    } else {
        assert_eq!(queue(&mut session, &next_elf), Err(Error::FlowBusy));
    }
}

#[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
#[test]
fn first_server_check_wrq_done_under_competing_publications() {
    check_first_server_check_wrq_done(false);
}

#[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
#[test]
fn first_server_check_uncertain_reuse_quarantines_the_session() {
    check_first_server_check_wrq_done(true);
}

#[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
fn check_first_server_check_wrq_done(_uncertain: bool) {
    use qcom_glink_qmi::TftpReplyRoute;
    #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
    use qcom_glink_qmi::{Error, WriteError};
    use qcom_qmi_router::Address;
    use qcom_tftp_server_check::{self as tftp, OACK, REQUEST, ServerCheck};

    let mut session = session();
    session.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
    session.start_control(1).unwrap();
    let mut advertisement = control(5, 2, 4);
    for id in 7u32..11 {
        advertisement.extend(4096u32.to_le_bytes());
        advertisement.extend(id.to_le_bytes());
    }
    assert_eq!(session.ingest(1, &advertisement), Ok(Event::RemoteIntents));
    let publish = |session: &mut Session| session.publish_next(1, |_| Ok(())).unwrap();
    let drain = |session: &mut Session| {
        let mut records = Vec::new();
        for _ in 0..16 {
            let Some(record) = publish(session) else { return records };
            if let Published::Data { remote_intent, last_fragment: true } = record {
                assert_eq!(session.ingest(1, &control(14, 2, remote_intent)), Ok(Event::TransmitDone));
            }
            records.push(record);
        }
        panic!("unexpected work after draining the test session");
    };
    let _ = drain(&mut session);
    let offer = |session: &mut Session| {
        let id = session.offer_receive(1).unwrap();
        assert_eq!(publish(session), Some(Published::ReceiveIntent(id)));
        id
    };
    let hello = offer(&mut session);
    assert!(matches!(session.ingest(1, &data(hello, &owner_control_frame(2, [0; 4]))),
        Ok(Event::QrtrControl(_))));
    let _ = drain(&mut session);
    let older = offer(&mut session);
    let first = offer(&mut session);
    let receive = |session: &mut Session, intent, peer, body: &[u8]| {
        let packet = owner_qrtr_packet(1, peer, tftp::SERVICE_PORT, false, body);
        let Event::TftpRequest(handle) = session.ingest(1, &data(intent, &packet)).unwrap()
        else { panic!("expected held TFTP request"); };
        handle
    };
    let wrq = receive(&mut session, first, 0x1f, REQUEST);
    let unrelated = receive(&mut session, older, 0x20, b"\x00\x01/readwrite/modem_efs\x00octet\x00");
    session.finish_tftp_request(&unrelated).unwrap();
    assert_eq!(publish(&mut session), Some(Published::ReceiveDone(older)));

    let older = offer(&mut session);
    let unrelated = receive(&mut session, older, 0x20, b"\x00\x01/readwrite/modem_efs\x00octet\x00");
    session.finish_tftp_request(&unrelated).unwrap();
    let host = Address { node: 1, port: tftp::SERVICE_PORT };
    let modem = Address { node: 0, port: 0x1f };
    let mut server = ServerCheck::new();
    let mut reply_bytes = [0; 64];
    let result = server.receive(tftp::Route {
        source: tftp::Address { node: modem.node, port: modem.port },
        destination: tftp::Address { node: host.node, port: host.port },
    }, REQUEST, &mut reply_bytes);
    let reply = result.unwrap();
    assert_eq!(&reply_bytes[..reply.len], OACK);
    session.observe_elf_server_check_dispatch(&wrq, TftpReplyRoute { source: host, destination: modem },
        true, result, &reply_bytes);
    session.finish_tftp_request(&wrq).unwrap();
    let transfer = Address { node: host.node, port: reply.route.source.port };
    session.queue_traced_tftp_response(1, &wrq, TftpReplyRoute { source: transfer, destination: modem },
        OACK, false).unwrap();
    let advertised = session.offer_receive(1).unwrap();

    #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
    {
        let intent_size = (qcom_glink_qmi::MAX_DATAGRAM_BYTES + 1) as u32;
        assert_eq!(session.ingest(1, &control(7, 2, intent_size)), Ok(Event::ReceiveIntentRequest { granted: false }));
        assert_eq!(publish(&mut session), Some(Published::IntentRequestAck { granted: false }));
        let command = if cfg!(feature = "sunfish-wlan-first-wrq-reuse") { 14 } else { 6 };
        let expected = control(command, 1, first);
        assert_eq!(session.publish_next(1, |record| {
            assert_eq!(record, expected);
            Err(WriteError::WouldBlock)
        }), Err(Error::WouldBlock));
        assert!(!session.server_check_oacks()[0].unwrap().wrq_priority_published);
        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
        let write_data = {
            let packet = owner_qrtr_packet(1, modem.port, transfer.port, false, b"\x00\x03\x00\x01check");
            let write_data = data(first, &packet);
            assert_eq!(session.ingest(1, &write_data), Err(Error::FragmentOrder));
            assert!(!session.server_check_oacks()[0].unwrap().wrq_reused);
            if _uncertain {
                assert_eq!(session.publish_next(1, |record| {
                    assert_eq!(record, expected);
                    Err(WriteError::Uncertain)
                }), Err(Error::PublicationUncertain));
                assert!(!session.server_check_oacks()[0].unwrap().wrq_reused);
                assert_eq!(session.ingest(1, &write_data), Err(Error::Quarantined));
                assert_eq!(session.publish_next(1, |_| panic!("quarantined writer")),
                    Err(Error::Quarantined));
                return;
            }
            write_data
        };
        assert_eq!(session.publish_next(1, |record| {
            assert_eq!(record, expected);
            Ok(())
        }), Ok(Some(Published::ReceiveDone(first))));
        assert!(session.server_check_oacks()[0].unwrap().wrq_priority_published);
        let rest = drain(&mut session);
        assert!(rest.contains(&Published::ReceiveIntent(advertised)));
        assert!(rest.contains(&Published::ReceiveDone(older)));
        assert!(rest.iter().any(|record| matches!(record, Published::Data { last_fragment: true, .. })));
        assert!(!rest.contains(&Published::ReceiveDone(first)));
        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
        {
            assert!(session.server_check_oacks()[0].unwrap().wrq_reused);
            let Event::TftpRequest(body) = session.ingest(1, &write_data).unwrap()
            else { panic!("expected DATA on the reused wire intent"); };
            assert_ne!(body, wrq);
            assert_eq!(session.tftp_datagram(&wrq), Err(Error::InvalidHandle));
            assert_eq!(session.finish_tftp_request(&wrq), Err(Error::InvalidHandle));
            let (route, packet) = session.tftp_datagram(&body).unwrap();
            assert_eq!(route, TftpReplyRoute { source: transfer, destination: modem });
            let reply = server.receive(tftp::Route {
                source: tftp::Address { node: modem.node, port: modem.port },
                destination: tftp::Address { node: transfer.node, port: transfer.port },
            }, packet, &mut reply_bytes).unwrap();
            assert!(reply.completed);
            assert_eq!(&reply_bytes[..reply.len], b"\x00\x04\x00\x01");
            session.finish_tftp_request(&body).unwrap();
            assert_eq!(session.publish_next(1, |record| {
                assert_eq!(record, control(6, 1, first));
                Ok(())
            }), Ok(Some(Published::ReceiveDone(first))));
            assert_eq!(session.ingest(1, &write_data), Err(Error::InvalidIntent));
        }
    }
    #[cfg(not(feature = "sunfish-wlan-first-wrq-rxdone-turn"))]
    {
        assert_eq!(publish(&mut session), Some(Published::ReceiveIntent(advertised)));
        assert!(matches!(publish(&mut session), Some(Published::Data { last_fragment: true, .. })));
        assert_eq!(publish(&mut session), Some(Published::ReceiveDone(older)));
        assert_eq!(publish(&mut session), Some(Published::ReceiveDone(first)));
    }
}

#[cfg(feature = "sunfish-wlan-first-wrq-window")]
#[test]
fn first_wrq_window_retains_neighbors_and_matches_only_published_completion() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use qcom_glink_qmi::{Error, TftpReplyRoute, WriteError};
    use qcom_qmi_router::Address;
    use qcom_tftp_server_check::{REQUEST, SERVICE_PORT};

    static CLOCK: AtomicU64 = AtomicU64::new(1);
    let mut session = session();
    session.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));
    session.prepare_tftp_server(1, SERVICE_PORT).unwrap();
    session.start_control(1).unwrap();
    let mut advertisement = control(5, 2, 3);
    for id in 7u32..10 {
        advertisement.extend(4096u32.to_le_bytes());
        advertisement.extend(id.to_le_bytes());
    }
    session.ingest(1, &advertisement).unwrap();
    let publish = |session: &mut Session| session.publish_next(1, |_| Ok(())).unwrap();
    let offer = |session: &mut Session| {
        let id = session.offer_receive(1).unwrap();
        assert_eq!(publish(session), Some(Published::ReceiveIntent(id)));
        id
    };
    let hello = offer(&mut session);
    session.ingest(1, &data(hello, &owner_control_frame(2, [0; 4]))).unwrap();
    while let Some(event) = publish(&mut session) {
        if let Published::Data { remote_intent, .. } = event {
            session.ingest(1, &control(14, 2, remote_intent)).unwrap();
        }
    }
    let receive = |session: &mut Session, peer, body: &[u8]| {
        let intent = offer(session);
        let packet = owner_qrtr_packet(1, peer, SERVICE_PORT, false, body);
        let Event::TftpRequest(handle) = session.ingest(1, &data(intent, &packet)).unwrap()
        else { panic!("expected held datagram"); };
        session.finish_tftp_request(&handle).unwrap();
        assert_eq!(publish(session), Some(Published::ReceiveDone(intent)));
    };
    let stage_reply = |session: &mut Session, peer| {
        let route = TftpReplyRoute {
            source: Address { node: 1, port: 0x5001 },
            destination: Address { node: 0, port: peer },
        };
        let qmi_response = b"\x02\x01\x00\x20\x00\x07\x00\x02\x04\x00\x00\x00\x00\x00";
        session.queue_tftp_response(1, route, qmi_response).unwrap();
    };
    let send_reply = |session: &mut Session| {
        let Some(Published::Data { remote_intent, last_fragment: true }) = publish(session)
        else { panic!("expected one complete datagram"); };
        remote_intent
    };
    let qmi_request = b"\x00\x01\x00\x20\x00\x04\x00\x10\x01\x00\x01";
    receive(&mut session, 0x2a, qmi_request);
    receive(&mut session, 0x2b, qmi_request);
    stage_reply(&mut session, 0x2a);
    let old = send_reply(&mut session);
    stage_reply(&mut session, 0x2b);
    let pre = send_reply(&mut session);
    assert_ne!(old, pre);
    session.ingest(1, &control(14, 2, old)).unwrap();
    let window = session.first_wrq_window();
    assert_eq!(window.rx_before.unwrap().source_port, 0x2b);
    assert_eq!(window.rx_before.unwrap().qmi_header, Some(0x20));
    assert_eq!(window.tx_before.unwrap().destination_port, 0x2b);
    assert_eq!(window.tx_before.unwrap().done_at, None);
    session.ingest(1, &control(14, 2, pre)).unwrap();
    let pre_done = session.first_wrq_window().tx_before.unwrap().done_at.unwrap();

    receive(&mut session, 0x1f, REQUEST);
    receive(&mut session, 0x2c, qmi_request);
    receive(&mut session, 0x2d, qmi_request);
    stage_reply(&mut session, 0x2c);
    assert_eq!(session.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(session.first_wrq_window().tx_after, None);
    let post = send_reply(&mut session);
    session.ingest(1, &control(14, 2, post)).unwrap();
    let original = session.first_wrq_window().tx_after.unwrap();
    assert_eq!(original.qmi_header, Some(0x0200_0020));
    assert_eq!(original.destination_port, 0x2c);
    assert_eq!(original.intent, post);
    assert!(pre_done < original.at.unwrap() && original.at < original.done_at);
    stage_reply(&mut session, 0x2d);
    let next = send_reply(&mut session);
    session.ingest(1, &control(14, 2, next)).unwrap();
    let window = session.first_wrq_window();
    assert_eq!(window.tx_after, Some(original));
    assert_eq!(window.rx_after.unwrap().source_port, 0x2c);
    assert_eq!((window.rx_after_count, window.tx_after_count), (2, 2));

    session.reset_after_transport_reset(2).unwrap();
    assert_eq!(session.first_wrq_window(), Default::default());
}

#[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
#[test]
fn elf_host_credit_and_first_tftp_request_follow_exact_transport_boundaries() {
    use qcom_glink_qmi::ElfTftpRequest;

    let cases: &[u8] = if cfg!(feature="sunfish-ack-elf-phase-trace") { &[0,1,2,3] } else { &[3] };
    for &ack_case in cases {
    let mut session = session();
    let locator = qcom_servreg::LOCAL_PORT;
    session.register(1, Service::ServiceRegistry, locator).unwrap();
    session.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    session.prepare_tftp_server(1, 0x5000).unwrap();
    session.start_control(1).unwrap();
    let mut advertisement = control(5, 2, 4);
    for intent in 7u32..11 {
        advertisement.extend(4096u32.to_le_bytes());
        advertisement.extend(intent.to_le_bytes());
    }
    assert_eq!(session.ingest(1, &advertisement), Ok(Event::RemoteIntents));

    let publish = |session: &mut Session| {
        let mut frame = Vec::new();
        let event = session.publish_next(1, |bytes| {
            frame.extend_from_slice(bytes);
            Ok(())
        }).unwrap();
        (event, frame)
    };
    let publish_data = |session: &mut Session| {
        for _ in 0..8 {
            if let (Some(Published::Data { remote_intent, last_fragment: true }), frame) =
                publish(session)
            {
                return (remote_intent, frame);
            }
        }
        panic!("expected a complete data record");
    };
    let offer = |session: &mut Session| {
        let id = session.offer_receive(1).unwrap();
        assert_eq!(publish(session).0, Some(Published::ReceiveIntent(id)));
        id
    };
    let complete = |session: &mut Session, intent| {
        session.ingest(1, &control(14, 2, intent)).unwrap()
    };

    let (hello, _) = publish_data(&mut session);
    assert_eq!(complete(&mut session, hello), Event::TransmitDone);
    let id = offer(&mut session);
    assert!(matches!(session.ingest(1, &data(id, &owner_control_frame(2, [0; 4]))),
        Ok(Event::QrtrControl(_))));
    while let (Some(event), _) = publish(&mut session) {
        if let Published::Data { remote_intent, last_fragment: true } = event {
            assert_eq!(complete(&mut session, remote_intent), Event::TransmitDone);
        }
    }

    let mut query = servreg_query_datagram_with_transaction(b"kernel/elf_loader", 5);
    query[16..20].copy_from_slice(&1u32.to_le_bytes());
    let id = offer(&mut session);
    let Event::Request(handle) = session.ingest(1, &data(id, &query)).unwrap()
    else { panic!("expected ELF lookup") };
    let (resume_intent, resume) = publish_data(&mut session);
    let expected: Vec<u8> = [1u32, 7, 1, locator, 0, 20, 0, 9, 7, 1, locator, 0, 0]
        .into_iter().flat_map(u32::to_le_bytes).collect();
    assert_eq!(&resume[16..68], expected);

    let response = qcom_servreg::respond(qcom_servreg::Query {
        service_name: b"kernel/elf_loader",
        domain_offset: 0,
    });
    session.queue_reply(&handle, Reply::Success { optional_tlvs: response.optional_tlvs() }).unwrap();
    session.observe_elf_reply(&handle);
    let credit = session.elf_exchange().unwrap().host_credit.unwrap();
    assert!(credit.requested);
    assert_eq!(credit.remote_intent, Some(resume_intent));
    assert!(!credit.completed);
    let (elf_intent, _) = publish_data(&mut session);
    assert_ne!(resume_intent, elf_intent);

    let tftp = |session: &mut Session, payload: &[u8]| {
        let id = offer(session);
        let mut packet = qrtr(1, payload);
        packet[12..16].copy_from_slice(&0x21u32.to_le_bytes());
        packet[28..32].copy_from_slice(&0x5000u32.to_le_bytes());
        let Event::TftpRequest(handle) = session.ingest(1, &data(id, &packet)).unwrap()
        else { panic!("expected TFTP packet") };
        session.finish_tftp_request(&handle).unwrap();
        assert_eq!(publish(session).0, Some(Published::ReceiveDone(id)));
    };
    let modem_efs = b"\x00\x01/readwrite/modem_efs\x00octet\x00";
    tftp(&mut session, modem_efs);
    assert!(session.elf_exchange().unwrap().first_tftp_request.is_none());
    assert_eq!(complete(&mut session, elf_intent), Event::ReplyRxDone(handle));
    assert!(!session.elf_exchange().unwrap().host_credit.unwrap().completed);
    assert_eq!(complete(&mut session, resume_intent), Event::TransmitDone);
    let credit = session.elf_exchange().unwrap().host_credit.unwrap();
    assert!(credit.completed);
    assert_eq!(credit.before_reply, Some([true, false]));

    tftp(&mut session, b"\x00\x03\x00\x01");
    assert!(session.elf_exchange().unwrap().first_tftp_request.is_none());
    tftp(&mut session, modem_efs);
    let first = Some(ElfTftpRequest {
        opcode: 1, path_kind: "MODEM EFS", body_bytes: modem_efs.len() as u32,
        source_port: 0x21, destination_port: 0x5000,
    });
    assert_eq!(session.elf_exchange().unwrap().first_tftp_request, first);
    tftp(&mut session, b"\x00\x02/readwrite/other.txt\x00octet\x00");
    assert_eq!(session.elf_exchange().unwrap().first_tftp_request, first);

    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    {
        use qcom_tftp_server_check::{Address, OACK, REQUEST, Route, ServerCheck};
        use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

        static CLOCK: AtomicU64 = AtomicU64::new(0);
        session.set_registry_ack_clock(|| CLOCK.load(Relaxed));
        let mut server = ServerCheck::new();
        let mut response = [0; 64];
        let trace = |session: &Session| session.elf_exchange().unwrap().server_check.unwrap();
        let receive_write = |session: &mut Session, peer, local, payload| {
            let id = offer(session);
            let packet = owner_qrtr_packet(1, peer, local, false, payload);
            let Event::TftpRequest(handle) = session.ingest(1, &data(id, &packet)).unwrap()
            else { panic!("expected a routed TFTP request") };
            handle
        };
        let dispatch = |session: &mut Session, server: &mut ServerCheck, handle, output: &mut [u8]| {
            let (route, payload) = session.tftp_datagram(&handle).unwrap();
            let wrq = payload.starts_with(qcom_tftp_server_check::REQUEST_PREFIX);
            let result = server.receive(Route {
                source: Address { node: route.destination.node, port: route.destination.port },
                destination: Address { node: route.source.node, port: route.source.port },
            }, payload, output);
            session.observe_elf_server_check_dispatch(&handle, route, wrq, result, output);
            session.finish_tftp_request(&handle).unwrap();
            result
        };
        CLOCK.store(10, Relaxed);
        let wrq = receive_write(&mut session, 0x21, 0x5000, REQUEST);
        assert_eq!(trace(&session).received_at, Some(10));
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        {
            CLOCK.store(11, Relaxed);
            for peer in [0x22, 0x21] {
                let repeat = receive_write(&mut session, peer, 0x5000, REQUEST);
                #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
                session.observe_elf_server_check_coalesced(&repeat);
                session.finish_tftp_request(&repeat).unwrap();
                assert!(matches!(publish(&mut session).0, Some(Published::ReceiveDone(_))));
            }
            assert_eq!(trace(&session).timing.repeats, 1);
            assert_eq!(trace(&session).timing.first_repeat_at, Some(11));
            #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
            {
                let repeat = session.first_server_check().unwrap().repeat.unwrap();
                assert!(repeat.coalesced);
                assert_eq!(repeat.dispatch_at, None);
                assert_eq!(repeat.oack.phase, 0);
            }
            CLOCK.store(12, Relaxed);
            session.observe_elf_server_check_dispatch_start(&wrq);
            CLOCK.store(13, Relaxed);
        }
        let reply = dispatch(&mut session, &mut server, wrq, &mut response).unwrap();
        assert_eq!(&response[..reply.len], OACK);
        let port = reply.route.source.port;
        let route = qcom_glink_qmi::TftpReplyRoute {
            source: qcom_qmi_router::Address { node: 1, port },
            destination: qcom_qmi_router::Address { node: 0, port: 0x21 },
        };
        assert_eq!(trace(&session).wrq_result, Some(0));
        assert_eq!(trace(&session).transfer_port, Some(port));
        assert_eq!(trace(&session).oack.phase, 1);
        #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
        {
            let early = receive_write(&mut session, 0x21, port + 1, b"\x00\x03\x00\x01");
            session.finish_tftp_request(&early).unwrap();
            assert!(matches!(publish(&mut session).0, Some(Published::ReceiveDone(_))));
            assert_eq!(trace(&session).timing.peer_rx.data.count, 0);
        }
        #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
        let competing_intents: [u32; 4] = core::array::from_fn(|index| {
            let competing = qcom_glink_qmi::TftpReplyRoute {
                source: route.source,
                destination: qcom_qmi_router::Address { node: 0, port: 0x70 + index as u32 },
            };
            session.queue_tftp_response(1, competing, &[0, 4, 0, 1]).unwrap();
            publish_data(&mut session).0
        });
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        {
            assert_eq!(trace(&session).timing.dispatch_at, Some(12));
            assert_eq!(trace(&session).timing.ready_at, Some(13));
            CLOCK.store(15, Relaxed);
        }
        session.queue_tftp_response(1, route, OACK).unwrap();
        assert_eq!(trace(&session).oack.phase, 2);
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        assert_eq!(trace(&session).timing.queued_at, Some(15));
        #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
        {
            use qcom_glink_qmi::{Error, WriteError};
            CLOCK.store(16, Relaxed);
            assert_eq!(publish(&mut session).0, Some(Published::RemoteIntentRequest { size: 72 }));
            CLOCK.store(17, Relaxed);
            assert_eq!(session.publish_next(1, |_| panic!("no available remote intent")),
                Err(Error::NoRemoteIntent));
            assert_eq!(session.ingest(1, &control(8, 2, 1)),
                Ok(Event::RemoteIntentRequestAck { granted: true }));
            for intent in competing_intents {
                assert_eq!(complete(&mut session, intent), Event::TransmitDone);
            }
            CLOCK.store(18, Relaxed);
            assert_eq!(session.publish_next(1, |record| {
                assert_eq!(&record[48..50], &[0, 6]);
                CLOCK.store(19, Relaxed);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            assert_eq!(trace(&session).oack.phase, 2);
        }
        CLOCK.store(20, Relaxed);
        let (intent, _) = publish_data(&mut session);
        assert_eq!(trace(&session).oack.phase, 3);
        assert_eq!(trace(&session).oack.published_at, Some(20));
        #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
        {
            let queue = trace(&session).timing.queue;
            assert_eq!(queue.first_at, Some(16));
            assert_eq!(queue.calls, 4);
            assert_eq!(queue.no_fit, 2);
            assert_eq!(queue.intent_wait, 1);
            assert_eq!(queue.intent_requests, 1);
            assert_eq!(queue.idle, 1);
            assert_eq!(queue.oack_blocked, 1);
            assert_eq!(queue.other_blocked, 0);
            assert_eq!(queue.max_gap_ticks, 1);
            assert_eq!(queue.active_ticks, 1);
            assert_eq!(queue.writer_ticks, 1);
            assert_eq!(queue.oack_writer_ticks, 1);
        }
        #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
        {
            CLOCK.store(21, Relaxed);
            let wrong_port = receive_write(&mut session, 0x21, port + 1, b"\x00\x03\x00\x02x");
            assert!(trace(&session).data.is_none());
            let observed = trace(&session).timing.peer_rx.data.first.unwrap();
            assert_eq!(observed.destination_port, port + 1);
            assert_eq!(observed.block_or_error, Some(2));
            assert_eq!(observed.body_bytes, 5);
            assert_eq!(observed.received_at, Some(21));
            session.finish_tftp_request(&wrong_port).unwrap();
            assert!(matches!(publish(&mut session).0, Some(Published::ReceiveDone(_))));
            CLOCK.store(22, Relaxed);
            let error = receive_write(&mut session, 0x21, port, b"\x00\x05\x00\x05bad port\0");
            let observed = trace(&session).timing.peer_rx.error.first.unwrap();
            assert_eq!(observed.destination_port, port);
            assert_eq!(observed.block_or_error, Some(5));
            assert_eq!(observed.received_at, Some(22));
            session.finish_tftp_request(&error).unwrap();
            assert!(matches!(publish(&mut session).0, Some(Published::ReceiveDone(_))));
        }
        CLOCK.store(30, Relaxed);
        assert_eq!(complete(&mut session, intent), Event::TransmitDone);
        assert_eq!(trace(&session).oack.phase, 4);
        assert_eq!(trace(&session).oack.done_at, Some(30));

        let payload = b"\x00\x03\x00\x01test\n";
        let other = receive_write(&mut session, 0x22, port, payload);
        assert!(dispatch(&mut session, &mut server, other, &mut response).is_err());
        assert!(matches!(publish(&mut session).0, Some(Published::ReceiveDone(_))));
        assert!(trace(&session).data.is_none());
        CLOCK.store(40, Relaxed);
        let target = receive_write(&mut session, 0x21, port, payload);
        assert_eq!(trace(&session).data.unwrap().received_at, Some(40));
        #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
        {
            let peer_rx = trace(&session).timing.peer_rx;
            assert_eq!(peer_rx.data.count, 2, "the other peer is excluded");
            assert_eq!(peer_rx.data.first.unwrap().destination_port, port + 1);
            assert_eq!(peer_rx.error.count, 1);
        }
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        {
            let stages = trace(&session).data.unwrap().stages;
            assert_eq!(stages, qcom_glink_qmi::FinalAckStages::default());
            CLOCK.store(41, Relaxed);
            session.observe_elf_server_check_dispatch_start(&target);
            CLOCK.store(42, Relaxed);
            session.observe_elf_server_check_dispatch_start(&target);
            assert_eq!(trace(&session).data.unwrap().stages.dispatch_at, Some(41));
        }
        let reply = dispatch(&mut session, &mut server, target, &mut response).unwrap();
        assert!(reply.completed);
        assert_eq!(trace(&session).data.unwrap().result, Some(0));
        assert_eq!(trace(&session).data.unwrap().body_bytes, 9);
        assert_eq!(trace(&session).final_block, Some(1));
        assert_eq!(trace(&session).ack.phase, 1);
        assert_eq!(server.completed_count(), 1);
        assert_eq!(server.total_bytes(), 5);
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        {
            assert_eq!(trace(&session).data.unwrap().stages.prepared_at, Some(42));
            assert_eq!(trace(&session).data.unwrap().stages.returned_at, None);
            CLOCK.store(44, Relaxed);
        }
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        if ack_case == 0 { session.queue_tftp_response(1, route, &response[..reply.len]).unwrap(); }
        else { session.queue_traced_tftp_response(1, if ack_case==1 {&other} else {&target}, route, &response[..reply.len], false).unwrap(); }
        #[cfg(not(feature = "sunfish-ack-elf-phase-trace"))]
        session.queue_tftp_response(1, route, &response[..reply.len]).unwrap();
        assert_eq!(trace(&session).ack.phase, 2);
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        {
            assert_eq!(trace(&session).data.unwrap().stages.queued_at, if ack_case>=2 {Some(44)} else {None});
            assert_eq!(session.publish_next(1, |_| Err(qcom_glink_qmi::WriteError::WouldBlock)),
                Err(qcom_glink_qmi::Error::WouldBlock));
            assert_eq!(trace(&session).data.unwrap().stages.returned_at, None,
                "a blocked publication cannot invent a receive buffer return");
        }
        CLOCK.store(50, Relaxed);
        let (intent, _) = publish_data(&mut session);
        assert_eq!(trace(&session).ack.phase, 3);
        assert_eq!(trace(&session).ack.published_at, Some(50));
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        {
            assert_eq!(trace(&session).data.unwrap().stages.returned_at, None);
            CLOCK.store(55, Relaxed);
            assert_eq!(publish(&mut session).0, Some(Published::ReceiveDone(target.diagnostic_id())));
            assert_eq!(trace(&session).data.unwrap().stages, qcom_glink_qmi::FinalAckStages {
                final_prepared: true, final_queued: ack_case>=2,
                dispatch_at: Some(41), prepared_at: Some(42), queued_at: if ack_case>=2 {Some(44)} else {None}, returned_at: Some(55),
            });
        }
        CLOCK.store(60, Relaxed);
        assert_eq!(complete(&mut session, intent), Event::TransmitDone);
        assert_eq!(trace(&session).ack.phase, 4);
        assert_eq!(trace(&session).ack.done_at, Some(60));
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        {
            CLOCK.store(65, Relaxed);
            let repeat = receive_write(&mut session, 0x21, 0x5000, REQUEST);
            session.finish_tftp_request(&repeat).unwrap();
            assert!(matches!(publish(&mut session).0, Some(Published::ReceiveDone(_))));
            assert_eq!(trace(&session).timing.repeats, 2);
            assert_eq!(trace(&session).timing.first_repeat_at, Some(11));

            for (peer, time) in [(0x22, 70), (0x21, 80)] {
                CLOCK.store(time, Relaxed);
                let intent = offer(&mut session);
                let close = owner_control_frame(6, [0, peer, 0, 0]);
                assert_eq!(session.ingest(1, &data(intent, &close)),
                    Ok(Event::QrtrControl(qcom_qrtr_control::Event::ClientRemoved(peer))));
                assert_eq!(publish(&mut session).0, Some(Published::ReceiveDone(intent)));
            }
            let timing = trace(&session).timing;
            assert_eq!(timing.closes, 1);
            assert_eq!(timing.first_close_at, Some(80));
            assert_eq!(timing.first_close_oack_phase, Some(4));
            #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
            {
                assert_eq!(timing.peer_rx.repeat.count, 1, "only the post-publication WRQ");
                let repeat = timing.peer_rx.repeat.first.unwrap();
                assert_eq!(repeat.destination_port, 0x5000);
                assert_eq!(repeat.received_at, Some(65));
                let late = receive_write(&mut session, 0x21, port, payload);
                session.finish_tftp_request(&late).unwrap();
                assert_eq!(trace(&session).timing.peer_rx, timing.peer_rx);
            }
            #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
            {
                let first = session.first_server_check().unwrap();
                let post = trace(&session);
                assert!(!first.before_elf);
                assert_eq!(first.peer, post.peer);
                assert_eq!(first.received_at, post.received_at);
                assert_eq!(first.oack, post.oack);
                assert_eq!(first.ack, post.ack);
                assert_eq!(first.final_block, post.final_block);
                assert_eq!(first.timing.first_close_at, post.timing.first_close_at);
            }
        }
    }
    }
}

#[cfg(feature = "sunfish-first-oack-spare-buffer")]
mod first_oack_spare_tests {
    use super::*;
    pub(crate) fn prepared(full: bool) -> (Fake, Owner<Fake>, Irqs, qcom_tftp_server_check::ServerCheck) {
        let peer = Fake::new(0, 0);
        let mut s = session();
        s.enable_ipa_before_first_oack();
        s.observe_first_tftp_without_oack_hold();
        let adapter = Adapter::attach(peer.clone(), s, 1).unwrap();
        let mut owner = Owner::attach(adapter, 1).unwrap();
        owner.set_rmtfs_event_clock(|| 100);
        let mut irqs = Irqs::default();
        owner.prepare_tftp_server(0x5000).unwrap();
        owner.start_control().unwrap();
        let mut intents = control(5, 2, 12);
        for id in 0u32..12 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
        peer.push(&intents);
        owner.notified(8, &mut irqs).unwrap();
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let first = owner.offer_receive().unwrap();
        if full {
            while (0..8).any(|slot| (owner.receive_snapshot().states >> (slot * 4)) & 15 == 0) {
                owner.offer_receive().unwrap();
            }
        }
        owner.notified(8, &mut irqs).unwrap();
        peer.take_tx();
        peer.push(&data(first, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x1f, 0x5000)));
        owner.notified(1, &mut irqs).unwrap();
        let mut server = qcom_tftp_server_check::ServerCheck::new();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs),
            Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
        (peer, owner, irqs, server)
    }
    #[test]
    fn fresh_advertisement_precedes_original_oack_once() {
        let (peer, mut owner, mut irqs, _) = prepared(false);
        owner.notified(8, &mut irqs).unwrap();
        let id = owner.oack_resources(0x5001).unwrap().primed_intent
            .expect("first OACK should advertise a fresh free buffer");
        let wire = peer.take_tx();
        let mut at = 0; let mut advertisement = None; let mut oack = None;
        while at < wire.len() {
            let cmd = owner_word(&wire, at) & 0xffff;
            let len = match cmd {
                5 => { if owner_word(&wire, at+12) == id { advertisement = Some(at); } 16 },
                9 | 12 => { let n = owner_word(&wire, at+8) as usize;
                    let body = &wire[at+16..at+16+n];
                    if n >= 34 && owner_word(body, 28) == 0x1f && body[32..34] == [0,6] { oack = Some(at); }
                    (16+n+7)&!7 }, _ => 8,
            }; at += len;
        }
        assert!(advertisement.unwrap() < oack.unwrap());
        owner.notified(8, &mut irqs).unwrap();
        assert_eq!(owner.oack_resources(0x5001).unwrap().primed_intent, Some(id));
        assert!(peer.take_tx().is_empty(), "follow-up must not prime repeatedly");
    }
    #[test]
    fn full_pool_does_not_steal_a_receive_slot() {
        let (peer, mut owner, mut irqs, _) = prepared(true);
        owner.notified(8, &mut irqs).unwrap();
        assert_eq!(owner.oack_resources(0x5001).unwrap().primed_intent, None);
        assert!(owner_complete_datagrams(&peer.take_tx()).iter().any(|packet|
            packet.len() >= 34 && owner_word(packet, 28) == 0x1f && packet[32..34] == [0,6]));
    }
    #[test]
    fn uncertain_advertisement_head_seals_without_followup() {
        for failure in ["intent_head_before", "intent_head_after"] {
            let (peer, mut owner, mut irqs, _) = prepared(false);
            peer.0.borrow_mut().fail = Some(failure);
            assert!(owner.notified(8, &mut irqs).is_err());
            assert!(owner.oack_grant_outcome().primed_offer.is_none());
            assert!(owner.server_check_oacks()[0].unwrap().oack_at.is_none());
            peer.0.borrow_mut().fail = None;
            let before = peer.0.borrow().words;
            assert!(owner.notified(8, &mut irqs).is_err());
            assert_eq!(peer.0.borrow().words, before);
        }
    }

}

#[cfg(feature = "sunfish-wrq-stage-costs")]
#[test]
fn wrq_stage_history_freezes_only_after_exact_checked_return() {
    let (peer,mut owner,mut irqs,_) = first_oack_spare_tests::prepared(false);
    use ::core::sync::atomic::{AtomicU64,Ordering};
    static CLOCK:AtomicU64=AtomicU64::new(1000);
    CLOCK.store(1000,Ordering::Relaxed);
    owner.set_rmtfs_event_clock(||CLOCK.fetch_add(1,Ordering::Relaxed));
    assert_eq!(owner.wrq_receive_work().finished,None);
    owner.notified(8,&mut irqs).unwrap();
    let at=owner.server_check_oacks()[0].unwrap().wrq_done_at.unwrap();
    let work=owner.wrq_receive_work();
    assert_eq!(work.finished,Some(at));
    let before:Vec<_>=work.records.records(0,u64::MAX).map(|r|(r.start,r.end,r.kind,r.cmd,r.detail,r.protocol)).collect();
    assert!(before.iter().any(|r|r.2==1 && r.3==14 && r.1>r.0 && r.1<=at));
    let cost=work.cost(100,at);
    assert!(cost.check_count>0 && cost.check_ticks>0 && cost.tx>0);
    #[cfg(feature = "sunfish-wrq-validation-path")]
    {
        let path=&work.validation;
        let head=owner.server_check_oacks()[0].unwrap().oack_at.unwrap();
        assert!(at<head && head<=path.finished.unwrap(),"new history freezes after OACK, not WRQ return");
        assert!(path.cost(100,at).count>0);
        let after_return=path.cost(at,head);
        assert!(after_return.count>0 && after_return.tx>0);
    }
    peer.take_tx();peer.push(&control(7,2,108));
    owner.notified(8,&mut irqs).unwrap();
    let after:Vec<_>=owner.wrq_receive_work().records.records(0,u64::MAX)
        .map(|r|(r.start,r.end,r.kind,r.cmd,r.detail,r.protocol)).collect();
    assert_eq!(before,after);
    assert_eq!(owner.wrq_receive_work().finished,Some(at));
    #[cfg(feature = "sunfish-wrq-validation-path")]
    {
        let path=&owner.wrq_receive_work().validation;
        let end=path.finished.unwrap();
        assert!(path.records.records(at,u64::MAX).all(|r|r.end<=end));
        assert!(!path.records.records(end+1,u64::MAX).any(|r|r.cmd==7));
    }
}

#[cfg(feature = "sunfish-tftp-peer-metadata-batch")]
#[path = "sunfish_tftp_peer_metadata_test.rs"]
mod peer_metadata_batch_tests;

#[cfg(feature="sunfish-post-elf-startup-panel")]
include!("sunfish_post_elf_mcfg_cases.rs");
