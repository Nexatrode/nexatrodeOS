include!("sunfish_modem_owner_test.rs");
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
pub mod sunfish_ipa_linux_config;
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
pub mod sunfish_ipa_linux_memory;
#[path = "../rust/src/sunfish_ipa_linux_qmi.rs"]
pub mod sunfish_ipa_linux_qmi;
use qcom_modem_owner::{LinuxIpaError as LE, LinuxIpaProgress as LP};
const LO_SERVER: u32 = 0x4002;
const LO_CLIENT: u32 = 0x4004;
const LO_MODEM: u32 = 27;
const LO_PEER_CLIENT: u32 = 29;
fn lo_word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn lo_packet(kind: u32, peer: u32, local: u32, confirm: bool, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    for n in [
        1,
        kind,
        0,
        peer,
        u32::from(confirm),
        payload.len() as u32,
        1,
        local,
    ] {
        b.extend(n.to_le_bytes());
    }
    b.extend(payload);
    b.resize((b.len() + 3) & !3, 0);
    b
}
fn lo_message(kind: u8, id: u16, txn: u16, fields: &[(u8, &[u8])]) -> Vec<u8> {
    let mut b = vec![kind];
    b.extend(txn.to_le_bytes());
    b.extend(id.to_le_bytes());
    let size = fields.iter().map(|(_, v)| 3 + v.len()).sum::<usize>();
    b.extend((size as u16).to_le_bytes());
    for (tag, value) in fields {
        b.push(*tag);
        b.extend((value.len() as u16).to_le_bytes());
        b.extend_from_slice(value);
    }
    b
}
fn lo_session() -> Session {
    let mut s = session();
    s.register(1, Service::RemoteStorage, 0x4000).unwrap();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT)
        .unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry)
        .unwrap();
    s.register(1, Service::IpaServer, LO_SERVER).unwrap();
    s.prepare_service_available(1, Service::IpaServer).unwrap();
    s.enable_linux_ipa(1, LO_CLIENT).unwrap();
    s
}
struct LoRig {
    peer: Fake,
    owner: Owner<Fake>,
    irqs: Irqs,
    now: u64,
    available: Vec<u32>,
    sent: Vec<(u32, Vec<u8>)>,
    host: Vec<Vec<u8>>,
    hold_host: bool,
    recycle_intents: bool,
    held_host: Vec<(u32, Vec<u8>)>,
    intent_acks: Vec<bool>,
    receive_done: Vec<u32>,
    handlers: Option<qcom_modem_handlers::Handlers<services_fixture::FakeBackend>>,
}
impl LoRig {
    fn new() -> Self {
        Self::at(0, false)
    }
    fn at(now: u64, handlers: bool) -> Self {
        Self::from_session(now, handlers, lo_session())
    }
    fn from_session(now: u64, handlers: bool, session: Session) -> Self {
        Self::from_session_with_ready_ipa(now, handlers, session, false)
    }
    fn from_session_with_ready_ipa(now: u64, handlers: bool, session: Session, ready_ipa: bool) -> Self {
        Self::from_session_with_modem(now, handlers, session, ready_ipa, true)
    }
    fn from_session_with_modem(
        now: u64, handlers: bool, session: Session, ready_ipa: bool, announce_modem: bool,
    ) -> Self {
        Self::from_session_with_intents(now, handlers, session, ready_ipa, announce_modem, 4096)
    }
    fn from_session_with_intents(
        now: u64, handlers: bool, session: Session, ready_ipa: bool, announce_modem: bool,
        intent_capacity: u32,
    ) -> Self {
        let peer = Fake::new(0, 0);
        let adapter = Adapter::attach(peer.clone(), session, 1).unwrap();
        let mut owner = Owner::attach_linux_ipa(adapter, 1, 0x415, 1000).unwrap();
        let mut irqs = Irqs::default();
        if ready_ipa {
            owner.prepare_ipa_server_endpoint(LO_SERVER).unwrap();
        }
        owner.start_control().unwrap();
        if ready_ipa {
            let permit = qcom_ipa_service::ApReadyPermit::test_after_post_init(1);
            owner.enable_ipa_server(&permit, &mut irqs).unwrap();
        }
        let handlers = if handlers {
            let c = services_fixture::config(8192, 8192, 16384);
            let backend = services_fixture::backend(c);
            let mut h = qcom_modem_handlers::Handlers::attach(
                qcom_modem_handlers::Profile::validate(c, &[]).unwrap(),
                backend,
            );
            owner.activate_handlers(&mut h, &mut irqs).unwrap();
            Some(h)
        } else {
            None
        };
        let mut intents = control(5, 2, 32);
        for id in 7u32..39 {
            intents.extend(intent_capacity.to_le_bytes());
            intents.extend(id.to_le_bytes());
        }
        peer.push(&intents);
        let mut r = Self {
            peer,
            owner,
            irqs,
            now,
            available: Vec::new(),
            sent: Vec::new(),
            host: Vec::new(),
            hold_host: false,
            recycle_intents: false,
            held_host: Vec::new(),
            intent_acks: Vec::new(),
            receive_done: Vec::new(),
            handlers,
        };
        r.drive().unwrap();
        r.input(&owner_control_frame(2, [0; 4])).unwrap();
        if announce_modem {
            r.input(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM])).unwrap();
        }
        r
    }
    fn enqueue(&mut self, packet: &[u8]) {
        let id = self.available.remove(0);
        self.peer.push(&data(id, packet));
    }
    fn input(&mut self, packet: &[u8]) -> Result<(), OwnerError> {
        self.drive()?;
        self.enqueue(packet);
        self.drive()
    }
    fn drain(&mut self) -> bool {
        let bytes = self.peer.take_tx();
        let mut at = 0;
        let changed = !bytes.is_empty();
        while at < bytes.len() {
            let cmd = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
            let id = lo_word(&bytes, at + 4);
            let size = match cmd {
                5 => {
                    assert_eq!(id, 1);
                    self.available.push(lo_word(&bytes, at + 12));
                    16
                }
                6 | 14 => { self.receive_done.push(id); 8 },
                #[cfg(feature = "sunfish-glink-intent-request-experiment")]
                7 => 8,
                13 => 8,
                8 => { self.intent_acks.push(id != 0); 8 },
                9 | 12 => {
                    assert_eq!(cmd, 9);
                    let n = lo_word(&bytes, at + 8) as usize;
                    assert_eq!(lo_word(&bytes, at + 12), 0);
                    let packet = bytes[at + 16..at + 16 + n].to_vec();
                    if lo_word(&packet, 4) == 1
                        && [LO_SERVER, LO_CLIENT].contains(&lo_word(&packet, 12))
                    {
                        self.sent.push((id, packet));
                    } else {
                        if lo_word(&packet, 4) == 1 {
                            self.host.push(packet.clone());
                        }
                        if self.hold_host && lo_word(&packet, 4) == 1 {
                            self.held_host.push((id, packet.clone()));
                        } else {
                            self.peer.push(&control(6, 2, id));
                            if self.recycle_intents {
                                let mut offer = control(5, 2, 1);
                                offer.extend(4096u32.to_le_bytes());
                                offer.extend(id.to_le_bytes());
                                self.peer.push(&offer);
                            }
                            if lo_word(&packet, 16) == 1 {
                                let p = self.credit(&packet);
                                self.enqueue(&p);
                            }
                        }
                    }
                    (16 + n + 7) & !7
                }
                _ => panic!("unexpected output command {cmd}"),
            };
            at += size;
        }
        assert_eq!(at, bytes.len());
        changed
    }
    fn drive(&mut self) -> Result<(), OwnerError> {
        for _ in 0..96 {
            let (p, service) = if let Some(h) = self.handlers.as_mut() {
                let combined = self.owner.notified_with_handlers(8, h, &mut self.irqs)?;
                (combined.transport, combined.service)
            } else {
                (
                    self.owner.notified(8, &mut self.irqs)?,
                    qcom_modem_owner::ServiceProgress::Idle,
                )
            };
            let registry = self.owner.dispatch_service_registry_step(&mut self.irqs)?;
            let qmi = self
                .owner
                .service_linux_ipa_step(self.now, &mut self.irqs)?;
            let output = self.drain();
            let busy = matches!(
                qmi,
                LP::Discovered | LP::Prepared | LP::Queued | LP::Committed | LP::ResponseAccepted
            ) || matches!(
                service,
                qcom_modem_owner::ServiceProgress::Progress
                    | qcom_modem_owner::ServiceProgress::ReplyQueued
            ) || matches!(
                registry,
                qcom_modem_owner::ServiceProgress::Progress
                    | qcom_modem_owner::ServiceProgress::ReplyQueued
            );
            if p.received == 0 && p.published == 0 && !busy && !output {
                return Ok(());
            }
        }
        panic!("owner failed to yield within bound");
    }
    fn credit(&self, packet: &[u8]) -> Vec<u8> {
        let peer = lo_word(packet, 28);
        let local = lo_word(packet, 12);
        let body = [7, 0, peer, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        lo_packet(7, peer, local, false, &body)
    }
    fn ack_one(&mut self, record: &(u32, Vec<u8>), glink: bool) -> Result<(), OwnerError> {
        if glink {
            self.peer.push(&control(6, 2, record.0));
            if self.recycle_intents {
                let mut offer = control(5, 2, 1);
                offer.extend(4096u32.to_le_bytes());
                offer.extend(record.0.to_le_bytes());
                self.peer.push(&offer);
            }
            self.drive()
        } else {
            self.input(&self.credit(&record.1))
        }
    }
    fn next(&mut self) -> (u32, Vec<u8>) {
        self.drive().unwrap();
        assert_eq!(self.sent.len(), 1);
        self.sent.remove(0)
    }
    fn ack(&mut self, record: &(u32, Vec<u8>), first: bool) {
        let committed = self
            .owner
            .linux_ipa_report()
            .unwrap()
            .publications_committed;
        self.ack_one(record, first).unwrap();
        assert_eq!(
            self.owner
                .linux_ipa_report()
                .unwrap()
                .publications_committed,
            committed + u32::from(cfg!(feature = "sunfish-qrtr-ipa-flow-experiment") && first)
        );
        self.ack_one(record, !first).unwrap();
        assert_eq!(
            self.owner
                .linux_ipa_report()
                .unwrap()
                .publications_committed,
            committed + 1
        );
    }
    fn request(&mut self, id: u16, txn: u16) {
        let b = if id == 0x35 {
            lo_message(0, id, txn, &[(1, &[0xfe])])
        } else {
            lo_message(0, id, txn, &[(0x10, &[0xfe])])
        };
        self.input(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &b))
            .unwrap();
    }
    fn init_response(&mut self, txn: u16, result: u16, error: u16) -> Result<(), OwnerError> {
        let value = result
            .to_le_bytes()
            .into_iter()
            .chain(error.to_le_bytes())
            .collect::<Vec<_>>();
        self.input(&lo_packet(
            1,
            LO_MODEM,
            LO_CLIENT,
            true,
            &lo_message(2, 0x21, txn, &[(2, &value)]),
        ))
    }
}

#[cfg(feature = "sunfish-qrtr-servreg-flow-experiment")]
#[test]
fn servreg_reference_releases_reply_after_glink_without_qrtr_resume() {
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    r.hold_host = true;
    let query = |txn| lo_message(0, 0x21, txn, &[(1, b"wlan/fw"), (0x10, &0u32.to_le_bytes())]);
    r.input(&lo_packet(1, LO_PEER_CLIENT, qcom_servreg::LOCAL_PORT, true, &query(1))).unwrap();
    assert_eq!(r.held_host.len(), 1);
    let first = r.held_host.remove(0);
    assert_eq!(lo_word(&first.1, 16), 0, "first reply must not force QRTR confirmation");
    r.input(&lo_packet(1, LO_PEER_CLIENT, qcom_servreg::LOCAL_PORT, true, &query(2))).unwrap();
    assert_eq!(r.held_host.len(), 0, "second reply must wait for real GLINK completion");
    r.ack_one(&first, true).unwrap();
    assert_eq!(r.held_host.len(), 1, "same-destination reply must proceed without QRTR resume");
    assert_eq!(lo_word(&r.held_host[0].1, 16), 0);
}

#[cfg(feature = "sunfish-ipa-ignore-config-reference")]
#[test]
fn reference_ipa_config_is_consumed_without_fabricating_a_qmi_response() {
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    let value = [0u8; 4];
    let fields = [0x10, 0x15, 0x11, 0x12, 0x13, 0x14, 0x16, 0x17]
        .map(|tag| (tag, value.as_slice()));
    let message = lo_message(0, 0x27, 3, &fields);
    assert_eq!(message.len(), 63);
    let id = r.available[0];
    r.input(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &message)).unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.ignored_config_requests, 1);
    assert_eq!((report.registration_requests, report.driver_requests), (0, 0));
    assert!(!report.protocol_complete);
    assert!(r.receive_done.contains(&id), "a consumed request needs a real RX_DONE");
    assert!(r.sent.is_empty(), "ignored config must not manufacture a QMI response");
}

#[cfg(feature = "sunfish-ipa-ignore-config-reference")]
#[test]
fn reference_ipa_config_rejects_bad_headers_and_duplicate_tlvs() {
    for (port, message) in [
        (0, lo_message(0, 0x27, 1, &[])),
        (LO_PEER_CLIENT, lo_message(2, 0x27, 1, &[])),
        (LO_PEER_CLIENT, lo_message(0, 0x27, 1, &[(0x10, &[1]), (0x10, &[2])])),
    ] {
        let mut r = LoRig::new();
        let _ipa_init = r.next();
        assert!(r.input(&lo_packet(1, port, LO_SERVER, true, &message)).is_err());
        assert_eq!(r.owner.linux_ipa_report().unwrap().ignored_config_requests, 0);
    }
}

#[cfg(feature = "sunfish-ipa-ignore-unhandled-reference")]
#[test]
fn reference_unhandled_filter_request_is_consumed_without_success_reply() {
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    let filter_data = vec![0u8; 0x8ca];
    let message = lo_message(0, 0x23, 4, &[(0x15, &filter_data)]);
    assert_eq!(message.len(), 0x8d4);
    let id = r.available[0];
    r.input(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &message)).unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!((report.ignored_unhandled_requests, report.last_ignored_unhandled_id), (1, Some(0x23)));
    assert_eq!((report.registration_requests, report.driver_requests), (0, 0));
    assert!(!report.protocol_complete);
    assert!(r.receive_done.contains(&id));
    assert!(r.sent.is_empty(), "unknown filter request must not produce a success reply");
}

#[cfg(feature = "sunfish-ipa-ignore-unhandled-reference")]
#[test]
fn reference_unhandled_request_still_rejects_malformed_envelopes() {
    for (port, message) in [
        (0, lo_message(0, 0x23, 1, &[])),
        (LO_PEER_CLIENT, lo_message(2, 0x23, 1, &[])),
        (LO_PEER_CLIENT, lo_message(0, 0x23, 1, &[(0x15, &[1]), (0x15, &[2])])),
    ] {
        let mut r = LoRig::new();
        let _ipa_init = r.next();
        assert!(r.input(&lo_packet(1, port, LO_SERVER, true, &message)).is_err());
        assert_eq!(r.owner.linux_ipa_report().unwrap().ignored_unhandled_requests, 0);
    }
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    let msg = lo_message(0, 0x23, 1, &[]);
    assert!(r.input(&lo_packet(1, LO_PEER_CLIENT, qcom_servreg::LOCAL_PORT, true, &msg)).is_err());
    assert_eq!(r.owner.linux_ipa_report().unwrap().ignored_unhandled_requests, 0);
}

#[test]
fn rejected_oversized_ipa_request_retains_only_routing_header_metadata() {
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    let bytes = lo_message(0, 0x35, 7, &[(1, &[1]), (0x10, &[2])]);
    assert!(r.input(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &bytes)).is_err());
    let diagnostic = r.owner.routing_diagnostic().expect("rejected route metadata retained");
    assert_eq!(diagnostic.service, qcom_ipa_service::SERVICE_ID);
    assert_eq!(diagnostic.body_bytes, bytes.len() as u32);
    assert_eq!((diagnostic.message_id, diagnostic.transaction), (0x35, 7));
    assert_eq!((diagnostic.destination, diagnostic.kind), (LO_SERVER, 0));
    assert_eq!(diagnostic.tlvs, [0x0001_0001, 0x0010_0001]);
}

#[test]
fn rejected_ipa_client_reply_retains_its_header_after_owner_stop() {
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    let bytes = lo_message(2, 0x77, 8, &[(2, &[0, 0, 0, 0])]);
    assert!(r.input(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &bytes)).is_err());
    let diagnostic = r.owner.routing_diagnostic().expect("failed receive metadata retained");
    assert_eq!((diagnostic.source, diagnostic.destination, diagnostic.kind), (LO_MODEM, LO_CLIENT, 2));
    assert_eq!((diagnostic.message_id, diagnostic.transaction), (0x77, 8));
    assert_eq!(diagnostic.tlvs[0], 0x0002_0004);
    let failure = r.owner.session_failure_diagnostic().expect("failed transport stage retained");
    assert_eq!((failure.stage, failure.command), (3, 9));
}

#[cfg(feature = "sunfish-qrtr-servreg-flow-experiment")]
#[test]
fn servreg_reference_applies_watermarks_and_resumes_destination() {
    let mut r = LoRig::new();
    let _ipa_init = r.next();
    r.hold_host = true;
    let query = |txn| lo_message(0, 0x21, txn, &[(1, b"wlan/fw"), (0x10, &0u32.to_le_bytes())]);
    let mut fifth = None;
    for txn in 1..=10 {
        r.input(&lo_packet(1, LO_PEER_CLIENT, qcom_servreg::LOCAL_PORT, true, &query(txn))).unwrap();
        assert_eq!(r.held_host.len(), 1);
        let reply = r.held_host.remove(0);
        assert_eq!(lo_word(&reply.1, 16), u32::from(txn == 5));
        if txn == 5 { fifth = Some(reply.clone()); }
        r.ack_one(&reply, true).unwrap();
    }
    r.input(&lo_packet(1, LO_PEER_CLIENT, qcom_servreg::LOCAL_PORT, true, &query(11))).unwrap();
    assert!(r.held_host.is_empty(), "high watermark must block the eleventh reply");
    let fifth = fifth.unwrap();
    r.ack_one(&fifth, false).unwrap();
    assert_eq!(r.held_host.len(), 1, "destination resume must restore credit");
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn native_runtime_profile_can_start_ipa_after_explicit_test_readiness() {
    let session = Session::native_runtime_profile(Config {
        local_node: 1, peer_node: 0, local_channel: 1, remote_channel: 2,
    }, 1).unwrap();
    // A synthetic permit is valid only in this host test, not on the device.
    let mut r = LoRig::from_session_with_ready_ipa(0, false, session, true);
    assert_ne!(r.owner.control_status().ready_services & (1 << 4), 0);
    assert_ne!(r.owner.control_status().announced_services & (1 << 4), 0);
    let record = r.next();
    assert_eq!(lo_word(&record.1, 28), LO_MODEM);
    assert_eq!(u16::from_le_bytes(record.1[35..37].try_into().unwrap()), 0x21);
    assert!(!r.owner.linux_ipa_report().unwrap().failed);
}

#[test]
fn linuxowner_cold_handshake_crosses_real_fifo_and_both_completion_orders() {
    for first in [false, true] {
        let mut r = LoRig::new();
        let initial = r.next();
        assert_eq!(
            (lo_word(&initial.1, 12), lo_word(&initial.1, 28)),
            (LO_CLIENT, LO_MODEM)
        );
        let m = qcom_qmi::Message::decode(&initial.1[32..32 + lo_word(&initial.1, 20) as usize])
            .unwrap();
        assert_eq!(m.transaction, 0);
        assert_eq!(m.u8(0x18), Ok(0));
        r.ack(&initial, first);
        r.init_response(0, 0, 0).unwrap();
        for (id, txn) in [(0x20, 3), (0x35, 4)] {
            r.request(id, txn);
            let record = r.next();
            assert_eq!(
                (lo_word(&record.1, 12), lo_word(&record.1, 28)),
                (LO_SERVER, LO_PEER_CLIENT)
            );
            r.ack(&record, first);
        }
        let indication = r.next();
        assert!(!r.owner.linux_ipa_report().unwrap().protocol_complete);
        r.ack(&indication, first);
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.protocol_complete);
        assert_eq!(report.publications_committed, 4);
        assert_eq!(report.responses_accepted, 1);
        assert_eq!(report.next_deadline, None);
    }
}

#[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
fn lo_published_init_indication() -> (LoRig, (u32, Vec<u8>)) {
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.init_response(0, 0, 0).unwrap();
    for (id, txn) in [(0x20, 3), (0x35, 4)] {
        r.request(id, txn);
        let record = r.next();
        r.ack(&record, true);
    }
    let indication = r.next();
    assert_eq!((indication.1[32], u16::from_le_bytes([indication.1[35], indication.1[36]])), (4, 0x22));
    (r, indication)
}

#[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
#[test]
fn reference_ipa_init_response_is_ignored_without_completing_indication() {
    let (mut r, indication) = lo_published_init_indication();
    let response = lo_message(2, 0x22, 0, &[(2, &[0, 0, 0, 0])]);
    let id = r.available[0];
    r.input(&lo_packet(1, LO_MODEM, LO_SERVER, true, &response)).unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.ignored_init_responses, 1);
    assert!(!report.protocol_complete);
    assert!(!report.protocol.unwrap().indication_sent, "a response is not a GLINK receipt");
    assert!(r.receive_done.contains(&id));
    r.ack(&indication, true);
    assert!(r.owner.linux_ipa_report().unwrap().protocol_complete);
    r.input(&lo_packet(1, LO_MODEM, LO_SERVER, true, &response)).unwrap();
    assert_eq!(r.owner.linux_ipa_report().unwrap().ignored_init_responses, 2);
}

#[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
#[test]
fn reference_ipa_init_response_rejects_unpublished_wrong_peer_or_transaction() {
    let mut early = LoRig::new();
    let _initial = early.next();
    let response = lo_message(2, 0x22, 0, &[(2, &[0, 0, 0, 0])]);
    assert!(early.input(&lo_packet(1, LO_MODEM, LO_SERVER, true, &response)).is_err());
    for (peer, txn) in [(LO_PEER_CLIENT, 0), (LO_MODEM, 1)] {
        let (mut r, _indication) = lo_published_init_indication();
        let msg = lo_message(2, 0x22, txn, &[(2, &[0, 0, 0, 0])]);
        assert!(r.input(&lo_packet(1, peer, LO_SERVER, true, &msg)).is_err());
        assert_eq!(r.owner.linux_ipa_report().unwrap().ignored_init_responses, 0);
    }
    for msg in [lo_message(2, 0x24, 0, &[(2, &[0, 0, 0, 0])]),
                lo_message(2, 0x22, 0, &[(2, &[0, 0, 0, 0]), (2, &[0, 0, 0, 0])])] {
        let (mut r, _indication) = lo_published_init_indication();
        assert!(r.input(&lo_packet(1, LO_MODEM, LO_SERVER, true, &msg)).is_err());
        assert_eq!(r.owner.linux_ipa_report().unwrap().ignored_init_responses, 0);
    }
}
#[test]
fn linuxowner_all_six_handshake_orders_are_serialized_without_manual_protocol_commits() {
    for order in [b"IRU", b"IUR", b"RIU", b"RUI", b"UIR", b"URI"] {
        let mut r = LoRig::new();
        let initial = r.next();
        r.ack(&initial, true);
        for e in order {
            if *e == b'I' {
                r.init_response(0, 0, 0).unwrap();
            } else {
                r.request(if *e == b'R' { 0x20 } else { 0x35 }, 1);
                let record = r.next();
                r.ack(&record, false);
            }
        }
        let indication = r.next();
        r.ack(&indication, true);
        assert!(r.owner.linux_ipa_report().unwrap().protocol_complete);
    }
}
// The independent profile has its own paired baseline/candidate test.
#[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
#[test]
fn linuxowner_holds_early_response_and_reply_requests_while_initial_record_is_retained() {
    let mut r = LoRig::new();
    let initial = r.next();
    r.init_response(0, 0, 0).unwrap();
    r.request(0x20, 1);
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.held_response);
    assert!(report.record_queued);
    assert_eq!(report.registration_requests, 1);
    assert_eq!(report.driver_requests, 0);
    assert!(!report.protocol.unwrap().modem_ready);
    assert!(r.sent.is_empty());
    r.ack(&initial, false);
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(!report.held_response);
    assert!(report.protocol.unwrap().modem_ready);
    let response = r.next();
    r.ack(&response, true);
    assert!(
        r.owner
            .linux_ipa_report()
            .unwrap()
            .protocol
            .unwrap()
            .indication_requested
    );
}

#[test]
fn linuxowner_counts_validated_registration_and_driver_requests_while_queued() {
    let mut r = LoRig::new();
    let _initial = r.next();
    r.request(0x20, 1);
    r.request(0x35, 2);
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!((report.registration_requests, report.driver_requests), (1, 1));
    assert!(report.record_queued);
    assert!(!report.protocol.unwrap().qmi_uc_ready);
}

#[cfg(feature = "sunfish-ipa-registration-trace")]
#[test]
fn registration_trace_follows_only_its_own_glink_completion() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let mut rig = LoRig::new();
    rig.owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
    let init = rig.next();
    rig.init_response(0, 0, 0).unwrap();
    rig.request(0x20, 0x78);
    let waiting = rig.owner.ipa_registration_trace().unwrap();
    assert_eq!(waiting.transaction, 0x78);
    assert_eq!(waiting.peer_port, LO_PEER_CLIENT);
    assert_eq!(waiting.local_port, LO_SERVER);
    assert_eq!(waiting.prior_message, Some(0x21));
    assert!(waiting.received_at.unwrap() < waiting.routed_at.unwrap());
    assert_eq!(waiting.prepared_at.is_some(), cfg!(feature = "sunfish-ipa-independent-server-experiment"));
    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    assert!(!rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap());

    rig.ack(&init, false);
    let registration = rig.next();
    assert_eq!(registration.1.len(), 48);
    assert_eq!(&registration.1[32..37], &[2, 0x78, 0, 0x20, 0]);
    let sent = rig.owner.ipa_registration_trace().unwrap();
    assert!(sent.routed_at.unwrap() < sent.prepared_at.unwrap());
    assert!(sent.prepared_at.unwrap() < sent.queued_at.unwrap());
    assert!(sent.queued_at.unwrap() < sent.published_at.unwrap());
    assert_eq!(sent.remote_intent, Some(registration.0));
    assert_eq!(sent.completed_at, None);
    rig.ack(&registration, false);
    let completed = rig.owner.ipa_registration_trace().unwrap();
    assert!(completed.published_at.unwrap() < completed.completed_at.unwrap());

    rig.request(0x20, 0x79);
    let later = rig.next();
    rig.ack(&later, false);
    assert_eq!(rig.owner.ipa_registration_trace(), Some(completed));
}

#[cfg(feature = "sunfish-ipa-registration-early-turn")]
#[test]
fn first_ipa_registration_yields_and_takes_one_step_per_early_turn() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let mut rig = LoRig::new();
    rig.owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
    let init = rig.next();
    rig.ack(&init, false);
    rig.init_response(0, 0, 0).unwrap();
    let registration = lo_message(0, 0x20, 0x78, &[(0x10, &[0xfe])]);
    let driver = lo_message(0, 0x35, 0x79, &[(1, &[0xfe])]);
    rig.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &registration));
    rig.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &driver));

    let glink = 1 << qcom_modem_owner::GLINK_CHANNEL;
    let progress = rig.owner.notified(glink, &mut rig.irqs).unwrap();
    assert_eq!(progress.received, 1);
    assert_eq!(progress.acknowledged & glink, 0);
    let routed = rig.owner.ipa_registration_trace().unwrap();
    let [yielded, early] = rig.owner.ipa_registration_turn_times();
    assert!(routed.routed_at.unwrap() < yielded.unwrap());
    assert_eq!(early, None);
    assert_eq!(routed.prepared_at, None);

    rig.now = CLOCK.fetch_add(1, Relaxed);
    assert!(rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap());
    assert_eq!(rig.owner.ipa_registration_turn_times(), [yielded, Some(rig.now)]);
    let prepared = rig.owner.ipa_registration_trace().unwrap();
    assert!(prepared.prepared_at.unwrap() > rig.now);
    assert_eq!(prepared.first_queue_attempt_at, None);
    assert_eq!(prepared.queued_at, None);

    rig.now = CLOCK.fetch_add(1, Relaxed);
    assert!(rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap());
    let admitted = rig.owner.ipa_registration_trace().unwrap();
    let attempt = admitted.first_queue_attempt_at.unwrap();
    assert!(prepared.prepared_at.unwrap() < attempt && attempt < admitted.queued_at.unwrap());
    assert_eq!((admitted.flow_waits, admitted.capacity_waits), (0, 0));
    assert!(!rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap());

    let reply = rig.next();
    assert!(rig.irqs.calls.contains(&qcom_modem_owner::GLINK_CHANNEL));
    assert_eq!(&reply.1[32..37], &[2, 0x78, 0, 0x20, 0]);
    assert!(rig.owner.ipa_registration_trace().unwrap().completed_at.is_none());
    rig.ack(&reply, false);
    assert!(rig.owner.ipa_registration_trace().unwrap().completed_at.is_some());
}

#[cfg(all(feature = "sunfish-ipa-registration-owner-trace", feature = "sunfish-qrtr-ipa-flow-experiment"))]
#[test]
fn registration_owner_trace_distinguishes_uncommitted_init_and_held_response() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    for (held_response, route_flags, pending, expected) in [
        (false, 0x5f, Some(0x21), LP::Committed),
        (true, 0x63, None, LP::ResponseAccepted),
    ] {
        let independent_early = cfg!(feature = "sunfish-ipa-independent-server-experiment") && !held_response;
        let route_flags = route_flags | if independent_early { 0x80 } else { 0 };
        let expected = if independent_early { LP::Prepared } else { expected };
        let mut rig = LoRig::new();
        rig.owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
        let init = rig.next();
        if held_response {
            rig.ack(&init, true);
            let response = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
            rig.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &response));
        } else {
            rig.peer.push(&control(6, 2, init.0));
        }
        let registration = lo_message(0, 0x20, 1, &[(0x10, &[1])]);
        rig.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &registration));
        rig.owner.notified(8, &mut rig.irqs).unwrap();
        assert_eq!(rig.owner.ipa_registration_trace().unwrap().prior_message, None);
        let trace = rig.owner.ipa_registration_owner_trace().unwrap();
        assert_eq!((trace.route_flags, trace.route_pending_id), (route_flags, pending));
        assert_eq!(trace.first_step, None);
        let turns = rig.owner.ipa_registration_turn_times();
        assert_eq!(turns[0].is_some(), independent_early);
        assert_eq!(turns[1], None);

        rig.now = CLOCK.fetch_add(1, Relaxed);
        assert_eq!(rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap(), independent_early);
        if !independent_early {
            assert_eq!(rig.owner.service_linux_ipa_step(rig.now, &mut rig.irqs).unwrap(), expected);
        }
        let first_step = rig.owner.ipa_registration_owner_trace().unwrap().first_step.unwrap();
        assert_eq!(first_step.started_at, rig.now);
        assert!(first_step.ended_at.unwrap() > rig.now);
        assert_eq!(first_step.progress, Some(expected));

        if independent_early {
            rig.now = CLOCK.fetch_add(1, Relaxed);
            assert_eq!(rig.owner.service_linux_ipa_step(rig.now, &mut rig.irqs).unwrap(), LP::Committed);
        }
        rig.now = CLOCK.fetch_add(1, Relaxed);
        assert!(rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap());
        assert!(rig.owner.ipa_registration_trace().unwrap().prepared_at.is_some());
        let settled = rig.owner.ipa_registration_owner_trace().unwrap();
        assert_eq!((settled.route_flags, settled.route_pending_id), (route_flags, pending));
        assert_eq!(settled.first_step, Some(first_step));
    }
}

// The composed profile tests overlapping discovery/early turns and both
// completion orders in independent_runtime_tests below.
#[cfg(all(feature = "sunfish-ipa-init-continuation", not(feature = "sunfish-ipa-independent-server-experiment")))]
#[test]
fn registration_before_ipa_core_runs_bounded_init_and_waits_for_real_receipt() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    for registration_waiting in [false, true] {
        let mut rig = LoRig::from_session_with_modem(0, false, lo_session(), false, false);
        rig.owner.set_rmtfs_event_clock(clock);
        rig.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
        if registration_waiting {
            let request = lo_message(0, 0x20, 1, &[(0x10, &[1])]);
            rig.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request));
        }
        rig.owner.notified(8, &mut rig.irqs).unwrap();
        let discovered = rig.owner.service_linux_ipa_step(clock(), &mut rig.irqs).unwrap();
        assert_eq!(discovered, LP::Discovered);
        rig.owner.continue_ipa_init_after_discovery(discovered, clock, &mut rig.irqs).unwrap();
        let (owner, transport) = rig.owner.ipa_init_trace();
        let owner = owner.unwrap();
        assert_eq!(owner.extra_steps, if registration_waiting { 2 } else { 0 });
        if !registration_waiting {
            assert!(owner.prepared_at.is_none() && transport.is_none());
            continue;
        }
        assert_eq!(rig.owner.ipa_registration_owner_trace().unwrap().route_flags, 0x40);
        let queued = transport.unwrap();
        assert!(owner.discovered_at < owner.prepared_at && owner.prepared_at < queued.queued_at);
        assert!(queued.queued_at.is_some() && queued.published_at.is_none());
        assert!(owner.committed_at.is_none() && queued.retired_at.is_none());
        assert_eq!(owner.queue_waits, 0);
        rig.now = clock();
        assert_eq!(rig.owner.service_linux_ipa_step(rig.now, &mut rig.irqs).unwrap(), LP::WaitingCompletion);
        assert!(rig.owner.ipa_registration_trace().unwrap().prepared_at.is_none());
        let init = rig.next();
        assert_eq!(&init.1[32..37], &[0, 0, 0, 0x21, 0]);
        let published = rig.owner.ipa_init_trace().1.unwrap();
        assert!(published.queued_at < published.published_at && published.retired_at.is_none());
        #[cfg(feature = "sunfish-ipa-init-tx-trace")]
        {
            assert!(published.queued_at < published.tx.router_at);
            assert!(published.tx.router_at < published.tx.first_pass_at);
            assert!(published.tx.first_pass_at < published.tx.first_writer_at);
            assert!(published.tx.first_writer_at <= published.tx.selected_at);
            assert!(published.tx.selected_at < published.published_at);
        }
        rig.peer.push(&control(6, 2, init.0));
        rig.owner.notified(8, &mut rig.irqs).unwrap();
        let (owner, transport) = rig.owner.ipa_init_trace();
        let retired = transport.unwrap();
        assert!(retired.published_at < retired.retired_at);
        assert!(owner.unwrap().committed_at.is_none());
        rig.now = clock();
        assert_eq!(rig.owner.service_linux_ipa_step(rig.now, &mut rig.irqs).unwrap(), LP::Committed);
        let committed = rig.owner.ipa_init_trace().0.unwrap();
        assert!(retired.retired_at < committed.committed_at);
        rig.now = clock();
        assert!(rig.owner.service_ipa_registration_early(rig.now, &mut rig.irqs).unwrap());
        assert!(committed.committed_at < rig.owner.ipa_registration_trace().unwrap().prepared_at);
    }
}

#[cfg(feature = "sunfish-ipa-init-tx-trace")]
#[test]
fn ipa_init_trace_separates_small_buffers_and_competing_publications() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    let mut rig = LoRig::from_session_with_intents(0, false, lo_session(), false, true, 64);
    rig.owner.set_rmtfs_event_clock(clock);
    let report = rig.owner.linux_ipa_report().unwrap();
    assert!(report.record_queued);
    assert_eq!(report.pending_message_id, Some(0x21));
    // Supply a peer intent request and a confirmed registration. The existing
    // control work and its smaller QRTR credit can fit while INIT_DRIVER waits.
    rig.peer.push(&control(7, 2, u32::MAX));
    let request = lo_message(0, 0x20, 1, &[(0x10, &[1])]);
    rig.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request));
    rig.drive().unwrap();
    let waiting = rig.owner.ipa_init_trace().1.unwrap();
    assert!(waiting.tx.control_ahead > 0 && waiting.tx.data_ahead > 0);
    assert!(waiting.tx.no_fit > 0 && waiting.tx.first_writer_at.is_some());
    assert!(waiting.tx.selected_at.is_none() && waiting.published_at.is_none());
    assert_eq!(waiting.tx.target_blocked, 0);
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    {
        // Independent admission does not bypass the shared transport's intent
        // request gate. Both records remain queued until that gate clears.
        assert!(rig.sent.is_empty());
        let report = rig.owner.linux_ipa_report().unwrap();
        assert!(report.client_record_queued && report.server_record_queued);
        assert!(rig.owner.ipa_init_trace().0.unwrap().committed_at.is_none());
        assert!(rig.owner.ipa_registration_trace().unwrap().published_at.is_none());
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        {
            let tx = rig.owner.ipa_registration_trace().unwrap().tx;
            assert!(tx.passes > 0 && tx.gates[0] > 0, "global intent request is observed");
            assert_eq!(tx.gates[1], 0, "a fitting free intent exists for the small reply");
            assert_eq!(tx.gates[3], 0, "global request gate still makes it ineligible");
            assert!(tx.idle > 0 && !tx.clock_invalid);
        }
    }

    let mut offer = control(5, 2, 1);
    offer.extend(4096u32.to_le_bytes());
    offer.extend(0u32.to_le_bytes());
    rig.peer.push(&offer);
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    rig.peer.push(&control(8, 2, 1));
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    let packet = {
        rig.drive().unwrap();
        assert_eq!(rig.sent.len(), 2);
        let server = rig.sent.iter().position(|(_, bytes)| bytes[32] == 2).unwrap();
        let registration = rig.sent.remove(server);
        assert_eq!(&registration.1[32..37], &[2, 1, 0, 0x20, 0]);
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        let published_tx = rig.owner.ipa_registration_trace().unwrap().tx;
        rig.ack(&registration, true);
        assert!(rig.owner.ipa_registration_trace().unwrap().completed_at.is_some());
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        {
            assert_eq!(rig.owner.ipa_registration_trace().unwrap().tx, published_tx,
                "counts and timing freeze when this exact reply is published");
            assert!(published_tx.gates[3] > 0 && published_tx.selected_at.is_some());
            assert!(published_tx.active_ticks >= published_tx.writer_ticks && published_tx.writer_ticks > 0);
        }
        assert!(rig.owner.ipa_init_trace().0.unwrap().committed_at.is_none());
        rig.sent.remove(0)
    };
    #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
    let packet = rig.next();
    assert_eq!(&packet.1[32..37], &[0, 0, 0, 0x21, 0]);
    let published = rig.owner.ipa_init_trace().1.unwrap();
    assert!(published.tx.first_writer_at < published.tx.selected_at);
    assert!(published.tx.selected_at < published.published_at);
    assert!(published.retired_at.is_none());
    let frozen_tx = published.tx;
    rig.ack(&packet, true);
    let retired = rig.owner.ipa_init_trace().1.unwrap();
    assert!(retired.published_at < retired.retired_at);
    assert_eq!(retired.tx, frozen_tx);
}

#[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
#[test]
fn linuxowner_reference_order_coalesced_records_complete_after_real_glink_receipts() {
    let mut r = LoRig::new();
    let initial = r.next();
    let reg = lo_message(0, 0x20, 3, &[(0x10, &[0xfe])]);
    let init = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    let driver = lo_message(0, 0x35, 4, &[(1, &[0xfe])]);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &reg));
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &init));
    r.peer.push(&control(6, 2, initial.0));
    r.drive().unwrap();
    // The actual profile initially advertises only two receive intents. The
    // third packet must use a newly advertised intent after those records.
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &driver));
    r.drive().unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!((report.registration_requests, report.driver_requests), (1, 1));
    assert_eq!(report.responses_accepted, 1);
    assert!(!report.protocol_complete);
    for _ in 0..2 {
        let reply = r.next();
        r.ack(&reply, true);
    }
    let indication = r.next();
    assert!(!r.owner.linux_ipa_report().unwrap().protocol_complete);
    r.ack(&indication, true);
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.protocol_complete);
    assert_eq!(report.publications_committed, 4);
}

#[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
#[test]
fn linuxowner_offers_requested_intent_while_two_ipa_receives_are_held() {
    let mut r = LoRig::new();
    let initial = r.next();
    let reg = lo_message(0, 0x20, 3, &[(0x10, &[0xfe])]);
    let init = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    let driver = lo_message(0, 0x35, 4, &[(1, &[0xfe])]);
    assert_eq!(r.available.len(), 2);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &reg));
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &init));
    r.peer.push(&control(7, 2, 1024));
    r.drive().unwrap();
    assert!(r.intent_acks.contains(&true));
    assert!(!r.available.is_empty());
    let intent_report = r.owner.linux_ipa_report().unwrap();
    assert_eq!((intent_report.peer_intent_requests, intent_report.peer_intent_denied), (1, 0));
    assert!(r.owner.linux_ipa_receive_available() > 0);
    r.peer.push(&control(7, 2, u32::MAX));
    r.drive().unwrap();
    let intent_report = r.owner.linux_ipa_report().unwrap();
    assert_eq!((intent_report.peer_intent_requests, intent_report.peer_intent_denied), (2, 1));
    assert!(r.intent_acks.contains(&false));
    assert!(r.owner.linux_ipa_report().unwrap().held_response);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &driver));
    r.drive().unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!((report.registration_requests, report.driver_requests), (1, 1));
    assert!(!report.protocol_complete);
    r.ack(&initial, true);
    for _ in 0..2 { let reply = r.next(); r.ack(&reply, true); }
    let indication = r.next();
    r.ack(&indication, true);
    assert!(r.owner.linux_ipa_report().unwrap().protocol_complete);
}
#[test]
fn linuxowner_timeout_keeps_record_and_rejects_retry_before_any_io() {
    let mut r = LoRig::new();
    let _initial = r.next();
    let schedules = r.irqs.schedules;
    for _ in 0..4 {
        r.drive().unwrap();
    }
    assert_eq!(r.irqs.schedules, schedules);
    assert_eq!(
        r.owner.linux_ipa_report().unwrap().next_deadline,
        Some(60_000)
    );
    r.now = 59_999;
    r.drive().unwrap();
    r.now = 60_000;
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::LinuxIpa(LE::Timeout))
    );
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record);
    assert_eq!(report.timeout_causes, 3);
    assert_eq!(report.pending_message_id, Some(0x21));
    assert!(report.protocol.unwrap().failed);
    assert!(!report.protocol_complete);
    r.peer.clear();
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::Stopped)
    );
    assert!(r.peer.log().is_empty());
}
#[test]
fn linuxowner_timeout_retains_diagnostic_without_accepting_early_response() {
    let mut r = LoRig::new();
    let _initial = r.next();
    r.init_response(0, 0, 0).unwrap();
    r.now = 60_000;
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::LinuxIpa(LE::Timeout))
    );
    let snapshot = r.owner.linux_ipa_tx_diagnostic().unwrap();
    assert_ne!(snapshot.flags & 0x10, 0);
    assert_eq!(snapshot.tx_transaction, 0);
    assert_eq!(snapshot.response_transaction, 0);
    assert_eq!(snapshot.response_result, 0);
    assert_eq!(snapshot.response_error, 0);
    assert!(!r.owner.linux_ipa_report().unwrap().protocol_complete);
    r.peer.clear();
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::Stopped));
    assert!(r.peer.log().is_empty());
}
#[cfg(feature = "sunfish-linux-rmtfs")]
#[test]
fn linuxowner_records_host_response_age_without_accepting_or_changing_deadline() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RESPONSE_CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 { RESPONSE_CLOCK.load(Ordering::Relaxed) }
    let mut r = LoRig::new();
    let _initial = r.next();
    r.owner.set_rmtfs_event_clock(clock);
    RESPONSE_CLOCK.store(12_345, Ordering::Relaxed);
    r.now = 12_345;
    r.init_response(0, 0, 0).unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.response_observed_age, Some(12_345));
    assert_eq!(report.timeout_ticks, 60_000);
    assert!(report.held_response && report.pending_record);
    r.now = 60_000;
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::LinuxIpa(LE::Timeout)));
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.response_observed_age, Some(12_345));
    assert_eq!(report.responses_accepted, 0);
}
#[cfg(feature = "sunfish-linux-rmtfs")]
#[test]
fn linuxowner_records_response_observed_at_deadline_as_not_early() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RESPONSE_CLOCK: AtomicU64 = AtomicU64::new(60_000);
    fn clock() -> u64 { RESPONSE_CLOCK.load(Ordering::Relaxed) }
    let mut r = LoRig::new();
    let _initial = r.next();
    r.owner.set_rmtfs_event_clock(clock);
    let response = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &response));
    r.owner.notified(8, &mut r.irqs).unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.response_observed_age, Some(report.timeout_ticks));
    assert!(report.held_response && report.pending_record);
    assert_eq!(report.responses_accepted, 0);
}
#[cfg(feature = "sunfish-ipa-independent-response-experiment")]
#[test]
fn timely_init_response_is_accepted_while_registration_reply_remains_uncommitted() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RESPONSE_CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 { RESPONSE_CLOCK.load(Ordering::Relaxed) }
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.owner.set_rmtfs_event_clock(clock);
    r.now = 30_000;
    r.request(0x20, 1);
    let registration = r.next();
    assert_eq!(r.owner.linux_ipa_report().unwrap().publications_committed, 1);
    RESPONSE_CLOCK.store(35_775, Ordering::Relaxed);
    r.now = 35_775;
    r.init_response(0, 0, 0).unwrap();
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.protocol.unwrap().modem_ready);
    assert_eq!(report.responses_accepted, 1);
    assert!(report.pending_record && report.record_queued);
    assert_eq!(report.publications_committed, 1, "registration has no receipt yet");
    r.ack(&registration, true);
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.publications_committed, 2);
    assert!(report.protocol.unwrap().indication_requested);
}

#[cfg(feature = "sunfish-ipa-independent-response-experiment")]
#[test]
fn response_observed_at_deadline_is_not_accepted() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RESPONSE_CLOCK: AtomicU64 = AtomicU64::new(60_000);
    fn clock() -> u64 { RESPONSE_CLOCK.load(Ordering::Relaxed) }
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.owner.set_rmtfs_event_clock(clock);
    let response = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &response));
    r.owner.notified(8, &mut r.irqs).unwrap();
    r.now = 60_000;
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::LinuxIpa(LE::Timeout)));
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.responses_accepted, 0);
    assert!(report.held_response && report.protocol.unwrap().failed);
}
#[cfg(feature = "sunfish-ipa-independent-response-experiment")]
#[test]
fn response_observed_before_deadline_survives_late_owner_service() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RESPONSE_CLOCK: AtomicU64 = AtomicU64::new(35_775);
    fn clock() -> u64 { RESPONSE_CLOCK.load(Ordering::Relaxed) }
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.owner.set_rmtfs_event_clock(clock);
    r.now = 30_000;
    r.request(0x20, 1);
    let registration = r.next();
    let response = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &response));
    r.owner.notified(8, &mut r.irqs).unwrap();
    assert!(r.owner.linux_ipa_report().unwrap().held_response);
    r.now = 60_000;
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Ok(LP::ResponseAccepted));
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.protocol.unwrap().modem_ready && report.pending_record);
    assert_eq!(report.publications_committed, 1);
    r.ack(&registration, true);
    assert_eq!(r.owner.linux_ipa_report().unwrap().publications_committed, 2);
}
#[cfg(feature = "sunfish-ipa-independent-response-experiment")]
#[test]
fn independent_response_still_rejects_wrong_transaction_and_remote_failure() {
    for (txn, result, error) in [(1, 0, 0), (0, 1, 3)] {
        let mut r = LoRig::new();
        let initial = r.next();
        r.ack(&initial, true);
        r.request(0x20, 1);
        let _registration = r.next();
        assert!(r.init_response(txn, result, error).is_err());
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.failed && report.pending_record && report.held_response);
        assert_eq!(report.publications_committed, 1);
        assert_eq!(report.responses_accepted, 0);
    }
}
#[cfg(feature = "sunfish-ipa-independent-response-experiment")]
#[test]
fn independent_response_does_not_bypass_expired_registration_deadline() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RESPONSE_CLOCK: AtomicU64 = AtomicU64::new(35_775);
    fn clock() -> u64 { RESPONSE_CLOCK.load(Ordering::Relaxed) }
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.owner.set_rmtfs_event_clock(clock);
    r.now = 30_000;
    r.request(0x20, 1);
    let _registration = r.next();
    let response = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &response));
    r.owner.notified(8, &mut r.irqs).unwrap();
    r.now = 90_000;
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::LinuxIpa(LE::Timeout)));
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.timeout_causes, 3);
    assert_eq!(report.responses_accepted, 0);
}
#[cfg(feature = "sunfish-ipa-independent-response-experiment")]
#[test]
fn response_with_no_observation_clock_cannot_extend_deadline() {
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    let response = lo_message(2, 0x21, 0, &[(2, &[0, 0, 0, 0])]);
    r.enqueue(&lo_packet(1, LO_MODEM, LO_CLIENT, true, &response));
    r.owner.notified(8, &mut r.irqs).unwrap();
    r.now = 60_000;
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::LinuxIpa(LE::Timeout)));
    let report = r.owner.linux_ipa_report().unwrap();
    assert_eq!(report.response_observed_age, None);
    assert_eq!(report.responses_accepted, 0);
}
#[test]
fn linuxowner_init_response_deadline_survives_outgoing_record_completion() {
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    assert!(!r.owner.linux_ipa_report().unwrap().pending_record);
    r.now = 60_000;
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::LinuxIpa(LE::Timeout))
    );
    assert_eq!(r.owner.linux_ipa_report().unwrap().timeout_causes, 2);
}
#[test]
fn linuxowner_counter_wrap_is_valid_but_counter_regression_is_terminal() {
    let mut r = LoRig::at(u64::MAX - 100, false);
    let _ = r.next();
    r.now = 20;
    r.drive().unwrap();
    assert!(!r.owner.linux_ipa_report().unwrap().failed);
    r.now = 19;
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::LinuxIpa(LE::Clock))
    );
}
#[test]
fn linuxowner_wrong_transaction_and_negative_result_stop_without_a_second_request() {
    for (txn, result, error) in [(1, 0, 0), (0, 1, 3), (0, 0, 3)] {
        let mut r = LoRig::new();
        let initial = r.next();
        r.ack(&initial, true);
        assert!(r.init_response(txn, result, error).is_err());
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.failed);
        assert!(report.held_response);
        assert!(!report.protocol_complete);
        assert!(r.sent.is_empty());
    }
}
#[test]
fn linuxowner_duplicate_held_response_is_terminal_and_does_not_replace_the_first() {
    let mut r = LoRig::new();
    let _ = r.next();
    r.init_response(0, 0, 0).unwrap();
    assert_eq!(
        r.init_response(0, 0, 0),
        Err(OwnerError::LinuxIpa(LE::UnexpectedResponse))
    );
    assert!(r.owner.linux_ipa_report().unwrap().held_response);
    assert!(r.owner.linux_ipa_report().unwrap().failed);
}
#[test]
fn linuxowner_live_gate_loss_masks_progress_and_preserves_protocol_ownership() {
    for fatal in [false, true] {
        let mut r = LoRig::new();
        let _ = r.next();
        {
            let mut m = r.peer.0.borrow_mut();
            if fatal {
                m.observation.fatal = true;
            } else {
                m.observation.identity += 1;
            }
        }
        assert!(r.owner.service_linux_ipa_step(1, &mut r.irqs).is_err());
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.failed && report.pending_record);
        r.peer.clear();
        assert_eq!(r.owner.notified(1, &mut r.irqs), Err(OwnerError::Stopped));
        assert!(r.peer.log().is_empty());
    }
}
#[test]
fn linuxowner_constructor_requires_selected_pristine_profile_and_valid_counter() {
    let (_, a) = adapter(0, 0);
    assert!(matches!(
        Owner::attach_linux_ipa(a, 1, 0x415, 1000),
        Err(OwnerError::LinuxIpa(LE::Configuration))
    ));
    for hz in [0, u64::MAX] {
        let peer = Fake::new(0, 0);
        let a = Adapter::attach(peer, lo_session(), 1).unwrap();
        assert!(matches!(
            Owner::attach_linux_ipa(a, 1, 0x415, hz),
            Err(OwnerError::LinuxIpa(LE::Clock))
        ));
    }
}
#[test]
fn linuxowner_non_ipa_handlers_and_servreg_continue_during_pending_ipa_completion() {
    let mut r = LoRig::at(0, true);
    let _initial = r.next();
    let name = b"wlan/fw";
    let bytes = lo_message(0, 0x21, 9, &[(1, name), (0x10, &0u32.to_le_bytes())]);
    r.input(&lo_packet(1, 10, qcom_servreg::LOCAL_PORT, true, &bytes))
        .unwrap();
    let allocate = lo_message(0, 0x24, 10, &[(1, &1u32.to_le_bytes())]);
    r.input(&lo_packet(1, 11, 0x4001, true, &allocate)).unwrap();
    assert!(
        r.host
            .iter()
            .any(|p| lo_word(p, 12) == qcom_servreg::LOCAL_PORT)
    );
    assert!(r.host.iter().any(|p| lo_word(p, 12) == 0x4001));
    assert!(r.owner.linux_ipa_report().unwrap().pending_record);
    assert!(!r.owner.linux_ipa_report().unwrap().failed);
}

fn lo_registration_requested() -> LoRig {
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.init_response(0, 0, 0).unwrap();
    let b = lo_message(0, 0x20, 1, &[(0x10, &[1])]);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, false, &b));
    r.owner.notified(8, &mut r.irqs).unwrap();
    r.drain();
    r
}
fn lo_queued_registration() -> LoRig {
    let mut r = lo_registration_requested();
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Ok(LP::Prepared)
    );
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Ok(LP::Queued)
    );
    r
}
#[test]
fn linuxowner_uncertain_head_store_retains_the_record_without_replaying_io() {
    let mut r = lo_queued_registration();
    r.peer.0.borrow_mut().fail_after_store = true;
    assert!(r.owner.notified(8, &mut r.irqs).is_err());
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record && report.record_queued);
    assert!(!report.protocol.unwrap().indication_requested);
    assert!(!report.protocol_complete);
    assert!(r.peer.log().iter().any(|op| matches!(op, Op::Head(_))));
    r.peer.clear();
    assert_eq!(r.owner.notified(8, &mut r.irqs), Err(OwnerError::Stopped));
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::Stopped)
    );
    assert!(r.peer.log().is_empty());
}
#[test]
fn linuxowner_unwinding_doorbell_keeps_pending_protocol_and_owner_terminal() {
    let mut r = lo_queued_registration();
    r.peer.0.borrow_mut().panic_on_notify = true;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        r.owner.notified(8, &mut r.irqs)
    }));
    assert!(result.is_err());
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record);
    assert!(!report.protocol_complete);
    assert!(!report.protocol.unwrap().indication_requested);
    r.peer.clear();
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::Stopped)
    );
    assert!(r.peer.log().is_empty());
}

#[test]
fn linuxowner_reply_backpressure_retains_one_prepared_record_until_the_flow_clears() {
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.init_response(0, 0, 0).unwrap();
    r.hold_host = true;
    let bytes = lo_message(0, 0x21, 9, &[(1, b"wlan/fw"), (0x10, &0u32.to_le_bytes())]);
    r.input(&lo_packet(
        1,
        LO_PEER_CLIENT,
        qcom_servreg::LOCAL_PORT,
        true,
        &bytes,
    ))
    .unwrap();
    assert_eq!(r.held_host.len(), 1);
    r.request(0x20, 2);
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.pending_record && !report.record_queued && !report.failed);
    for _ in 0..4 {
        r.drive().unwrap();
    }
    assert!(r.sent.is_empty());
    assert!(
        !r.owner
            .linux_ipa_report()
            .unwrap()
            .protocol
            .unwrap()
            .indication_requested
    );
    let record = r.held_host.remove(0);
    r.hold_host = false;
    r.ack_one(&record, true).unwrap();
    r.ack_one(&record, false).unwrap();
    let reply = r.next();
    r.ack(&reply, true);
    assert!(
        r.owner
            .linux_ipa_report()
            .unwrap()
            .protocol
            .unwrap()
            .indication_requested
    );
    assert_eq!(r.owner.service_registry_trace().replies_queued, 1);
}
#[test]
fn linuxowner_failed_continuation_schedule_stops_with_prepared_record_retained() {
    let mut r = lo_registration_requested();
    // Consume the transport continuation before injecting a failure in the
    // IPA step's own scheduling call.
    r.owner.notified(8, &mut r.irqs).unwrap();
    r.irqs.fail_schedule = true;
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::Schedule)
    );
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record && !report.record_queued);
    assert!(report.protocol.unwrap().failed);
    r.peer.clear();
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Err(OwnerError::Stopped)
    );
    assert!(r.peer.log().is_empty());
}

#[test]
fn linuxowner_post_handshake_panic_never_reports_protocol_complete() {
    let mut r = LoRig::new();
    let initial = r.next();
    r.ack(&initial, true);
    r.init_response(0, 0, 0).unwrap();
    for id in [0x20, 0x35] {
        r.request(id, 1);
        let record = r.next();
        r.ack(&record, true);
    }
    let record = r.next();
    r.ack(&record, true);
    assert!(r.owner.linux_ipa_report().unwrap().protocol_complete);
    let bytes = lo_message(0, 0x20, 2, &[(0x10, &[1])]);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, false, &bytes));
    r.owner.notified(8, &mut r.irqs).unwrap();
    r.drain();
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Ok(LP::Prepared)
    );
    assert_eq!(
        r.owner.service_linux_ipa_step(r.now, &mut r.irqs),
        Ok(LP::Queued)
    );
    r.peer.0.borrow_mut().panic_on_notify = true;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r
            .owner
            .notified(8, &mut r.irqs)))
        .is_err()
    );
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record);
    assert!(!report.protocol_complete);
}

#[cfg(feature = "sunfish-ipa-independent-server-runtime")]
mod independent_runtime_tests {
    include!("sunfish_ipa_independent_runtime_cases.rs");
    include!("sunfish_ipa_independent_server_cases.rs");
}

#[cfg(feature = "sunfish-ipa-announcement-fifo")]
#[test]
fn modem_ipa_announcement_fifo_matches_first_accepted_record() {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(1);
    for buffered_wrq in [false, true] {
        let mut session = lo_session();
        let port = qcom_tftp_server_check::SERVICE_PORT;
        session.prepare_tftp_server(1, port).unwrap();
        let mut rig = LoRig::from_session_with_modem(0, false, session, false, false);
        while rig.available.len() < 4 {
            rig.owner.offer_receive().unwrap();
            rig.drive().unwrap();
        }
        rig.owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
        rig.drive().unwrap();
        let other = owner_control_frame(4, [0x31, 0x101, 0, LO_MODEM]);
        let announcement = owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]);
        let wrq = lo_packet(1, 0x1f, port, false, qcom_tftp_server_check::REQUEST);
        rig.input(&other).unwrap();
        assert_eq!(rig.owner.ipa_announcement_rx(), None);
        rig.enqueue(&other);
        rig.enqueue(&announcement);
        if buffered_wrq { rig.enqueue(&wrq); }
        rig.drive().unwrap();
        let snapshot = rig.owner.ipa_announcement_rx().unwrap();
        let visible = snapshot.first_visible.unwrap();
        assert!(snapshot.last_absent.unwrap() < visible);
        assert_eq!(snapshot.bytes_ahead, Some(data(1, &other).len() as u32));
        assert!(visible <= rig.owner.first_ipa_announcement().unwrap().1.unwrap());
        if !buffered_wrq { rig.input(&wrq).unwrap(); }
        let wrq_bounds = rig.owner.ipa_wrq_timing().1.unwrap();
        if buffered_wrq {
            assert_eq!(Some(visible), wrq_bounds.first_visible);
        } else {
            assert!(visible < wrq_bounds.last_absent.unwrap());
        }
        rig.input(&announcement).unwrap();
        assert_eq!(rig.owner.ipa_announcement_rx(), Some(snapshot));
    }
}

#[cfg(feature = "sunfish-ipa-announcement-fifo")]
#[test]
fn announcement_after_wrq_needs_the_complete_glink_record() {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(1);
    let mut session = lo_session();
    let port = qcom_tftp_server_check::SERVICE_PORT;
    session.prepare_tftp_server(1, port).unwrap();
    let mut rig = LoRig::from_session_with_modem(0, false, session, false, false);
    rig.owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
    rig.input(&lo_packet(1, 0x1f, port, false, qcom_tftp_server_check::REQUEST)).unwrap();
    let wrq = rig.owner.ipa_wrq_timing().1.unwrap();
    let announcement = data(rig.available.remove(0), &owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
    let (incomplete, rest) = announcement.split_at(announcement.len() - 8);
    rig.peer.push(incomplete);
    rig.drive().unwrap();
    assert_eq!(rig.owner.first_ipa_announcement(), None);
    assert_eq!(rig.owner.ipa_announcement_rx(), None);
    rig.peer.push(rest);
    rig.drive().unwrap();
    let bounds = rig.owner.ipa_announcement_rx().unwrap();
    assert!(wrq.first_visible.unwrap() < bounds.last_absent.unwrap());
    assert!(bounds.last_absent.unwrap() < bounds.first_visible.unwrap());
    assert_eq!(Some(0), bounds.bytes_ahead);
    assert_eq!(rig.owner.ipa_wrq_timing().1, Some(wrq));
}
