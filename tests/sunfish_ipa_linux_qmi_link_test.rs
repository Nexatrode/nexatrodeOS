// Compose the real bounded transport, router and Linux handshake core. No phone.
include!("sunfish_glink_qmi_test.rs");
#[cfg(not(feature = "sunfish-linux-ipa-qmi"))]
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
pub mod sunfish_ipa_linux_config;
#[cfg(not(feature = "sunfish-linux-ipa-qmi"))]
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
pub mod sunfish_ipa_linux_memory;
#[cfg(not(feature = "sunfish-linux-ipa-qmi"))]
#[path = "../rust/src/sunfish_ipa_linux_qmi.rs"]
pub mod sunfish_ipa_linux_qmi;
use sunfish_ipa_linux_qmi as linux;

const LINUX_SERVER: u32 = 0x4003;
const LINUX_CLIENT: u32 = 0x4004;
const LINUX_MODEM: u32 = 27;
const LINUX_PEER_CLIENT: u32 = 29;
struct LinuxRig {
    link: Session,
    available: Vec<u32>,
    output: Vec<(u32, Vec<u8>)>,
    frames: Vec<Vec<u8>>,
}

#[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
#[test]
fn linuxlink_reference_flow_accepts_response_after_real_glink_completion() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let prepared = core.prepare_init_driver(1, 0, 0x415).unwrap();
    rig.link.queue_linux_ipa(1, &prepared, None).unwrap();
    rig.pump();
    let record = rig.output.remove(0);
    assert_eq!(word(&record.1, 16), 0, "first Linux flow packet does not request credit");
    let response = rig.response(0);
    assert_eq!(rig.link.linux_ipa_receipt_pending(), Some(false));
    assert!(rig.link.take_linux_ipa_receipt(1).unwrap().is_none());
    assert!(!core.state().modem_ready);
    rig.ack_one(&record, true);
    assert_eq!(rig.link.linux_ipa_receipt_pending(), Some(true));
    assert_eq!(rig.link.linux_ipa_receipt_pending(), Some(true), "observation must not consume receipt");
    let receipt = rig.link.take_linux_ipa_receipt(1).unwrap().unwrap();
    assert_eq!(rig.link.linux_ipa_receipt_pending(), Some(false));
    rig.link.commit_linux_ipa(1, &mut core, &prepared, receipt).unwrap();
    rig.accept_response(&mut core, response);
    assert!(core.state().modem_ready);
    rig.ack_one(&record, false);
    assert!(rig.link.take_linux_ipa_receipt(1).unwrap().is_none());
}

#[test]
#[cfg(not(feature = "sunfish-qrtr-ipa-flow-experiment"))]
fn linuxlink_diagnostic_distinguishes_missing_glink_and_qrtr_completion() {
    for glink_first in [true, false] {
        let mut rig = LinuxRig::new();
        let mut core = rig.core();
        let prepared = core.prepare_init_driver(1, 0, 0x415).unwrap();
        let record = rig.queued(&prepared, None);
        let before = rig.link.linux_ipa_tx_diagnostic().unwrap();
        assert_eq!(before.sent, before.len);
        assert_eq!(before.flags, 0x0c);
        assert_eq!(before.remote_intent, record.0);
        assert_eq!(before.tx_transaction, 0);
        assert_eq!(before.response_transaction, u32::MAX);
        let response = rig.response(0);
        let held = rig.link.linux_ipa_tx_diagnostic().unwrap();
        assert_eq!(held.flags, 0x1c);
        assert_eq!((held.response_transaction, held.response_result, held.response_error), (0, 0, 0));
        assert!(!core.state().modem_ready);
        rig.ack_one(&record, glink_first);
        let partial = rig.link.linux_ipa_tx_diagnostic().unwrap();
        assert_eq!(partial.flags, if glink_first { 0x15 } else { 0x1e });
        assert!(rig.link.take_linux_ipa_receipt(1).unwrap().is_none());
        rig.ack_one(&record, !glink_first);
        assert!(rig.link.linux_ipa_tx_diagnostic().is_none());
        let receipt = rig.link.take_linux_ipa_receipt(1).unwrap().unwrap();
        rig.link.commit_linux_ipa(1, &mut core, &prepared, receipt).unwrap();
        rig.accept_response(&mut core, response);
        assert!(core.state().modem_ready);
    }
}
impl LinuxRig {
    fn new() -> Self {
        Self::with_services(true, true)
    }
    fn with_services(available: bool, discovered: bool) -> Self {
        let mut link = session();
        link.register(1, Service::IpaServer, LINUX_SERVER).unwrap();
        link.enable_linux_ipa(1, LINUX_CLIENT).unwrap();
        if available {
            link.prepare_service_available(1, Service::IpaServer)
                .unwrap();
        }
        Self::from_prepared_link(link, discovered)
    }
    fn from_prepared_link(link: Session, discovered: bool) -> Self {
        Self::from_prepared_link_with_intents(link, discovered, 39)
    }
    fn from_prepared_link_with_intents(mut link: Session, discovered: bool, limit: u32) -> Self {
        link.start_control(1).unwrap();
        link.ingest(
            1,
            &intents(&(7..limit).map(|id| (id, 4096)).collect::<Vec<_>>()),
        )
        .unwrap();
        let mut rig = Self {
            link,
            available: Vec::new(),
            output: Vec::new(),
            frames: Vec::new(),
        };
        rig.input(&qrtr_control(2, [0; 4])).unwrap();
        rig.pump();
        assert!(rig.output.is_empty());
        if discovered {
            rig.input(&qrtr_control(4, [0x31, 0x201, 0, LINUX_MODEM]))
                .unwrap();
        }
        rig.pump();
        rig
    }
    fn pump(&mut self) {
        for _ in 0..64 {
            let mut bytes = Vec::new();
            match self
                .link
                .publish_next(1, |record| {
                    bytes.extend_from_slice(record);
                    Ok(())
                })
                .unwrap()
            {
                None => return,
                Some(Published::ReceiveIntent(id)) => self.available.push(id),
                Some(Published::Data {
                    remote_intent,
                    last_fragment,
                }) => {
                    assert!(last_fragment);
                    let packet = bytes[16..16 + word(&bytes, 8) as usize].to_vec();
                    if word(&packet, 4) == 1 {
                        self.frames.push(bytes.clone());
                        self.output.push((remote_intent, packet));
                    } else {
                        self.link.ingest(1, &control(6, remote_intent)).unwrap();
                    }
                }
                _ => {}
            }
        }
        panic!("transport did not become idle within bound");
    }
    fn input(&mut self, packet: &[u8]) -> Result<Event, Error> {
        self.pump();
        let id = self.available.remove(0);
        self.link.ingest(1, &data(9, id, packet, 0))
    }
    fn core(&self) -> linux::ColdSession {
        linux::ColdSession::new(1, self.link.linux_ipa_modem(1).unwrap().unwrap()).unwrap()
    }
    fn queued(&mut self, p: &linux::Prepared, handle: Option<&RequestHandle>) -> (u32, Vec<u8>) {
        self.link.queue_linux_ipa(1, p, handle).unwrap();
        self.pump();
        assert_eq!(self.output.len(), 1);
        let record = self.output.remove(0);
        let packet = &record.1;
        #[cfg(not(feature = "sunfish-qrtr-ipa-flow-experiment"))]
        assert_eq!(word(packet, 16), 1);
        #[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
        assert!(word(packet, 16) <= 1);
        assert_eq!(&packet[32..32 + word(packet, 20) as usize], p.bytes());
        assert!(self.link.take_linux_ipa_receipt(1).unwrap().is_none());
        record
    }
    fn ack_one(&mut self, record: &(u32, Vec<u8>), glink: bool) {
        if glink {
            self.link.ingest(1, &control(6, record.0)).unwrap();
        } else {
            self.input(&resume_route(word(&record.1, 28), word(&record.1, 12)))
                .unwrap();
        }
    }
    fn complete(
        &mut self,
        p: &linux::Prepared,
        core: &mut linux::ColdSession,
        record: &(u32, Vec<u8>),
        glink_first: bool,
    ) {
        let before = core.state();
        #[cfg(not(feature = "sunfish-qrtr-ipa-flow-experiment"))]
        {
            self.ack_one(record, glink_first);
            assert!(self.link.take_linux_ipa_receipt(1).unwrap().is_none());
            assert_eq!(core.state(), before);
            self.ack_one(record, !glink_first);
        }
        #[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
        {
            if !glink_first { self.ack_one(record, false); }
            assert!(self.link.take_linux_ipa_receipt(1).unwrap().is_none());
            assert_eq!(core.state(), before);
            self.ack_one(record, true);
            if glink_first { self.ack_one(record, false); }
        }
        let receipt = self.link.take_linux_ipa_receipt(1).unwrap().unwrap();
        self.link.commit_linux_ipa(1, core, p, receipt).unwrap();
        assert!(self.link.take_linux_ipa_receipt(1).unwrap().is_none());
    }
    fn request(&mut self, id: u16, txn: u16, status: u8) -> RequestHandle {
        let bytes = if id == 0x35 {
            ipa_server_request(txn, id, &[(1, &[status])])
        } else {
            ipa_server_request(txn, id, &[(0x10, &[status])])
        };
        match self
            .input(&qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, true, &bytes))
            .unwrap()
        {
            Event::Request(handle) => handle,
            _ => panic!(),
        }
    }
    fn prepare_reply(
        &self,
        core: &mut linux::ColdSession,
        handle: &RequestHandle,
    ) -> linux::Prepared {
        let (context, bytes) = self.link.request_qmi(handle).unwrap();
        assert_eq!(context.peer.port, LINUX_PEER_CLIENT);
        assert!(matches!(
            self.link.request(handle).unwrap().1,
            Request::IpaServer { .. }
        ));
        core.prepare_response(
            1,
            linux::Address {
                node: context.peer.node,
                port: context.peer.port,
            },
            bytes,
        )
        .unwrap()
    }
    fn response(&mut self, txn: u16) -> qcom_glink_qmi::LinuxIpaHandle {
        match self
            .input(&qrtr(
                1,
                LINUX_MODEM,
                LINUX_CLIENT,
                true,
                &ipa_init_response(txn, 0, 0),
            ))
            .unwrap()
        {
            Event::LinuxIpaResponse(handle) => handle,
            _ => panic!(),
        }
    }
    fn accept_response(
        &mut self,
        core: &mut linux::ColdSession,
        handle: qcom_glink_qmi::LinuxIpaHandle,
    ) {
        let (peer, bytes) = self.link.linux_ipa_response(&handle).unwrap();
        core.receive_init_response(1, peer, bytes).unwrap();
        self.link.finish_linux_ipa_response(&handle).unwrap();
        assert!(self.link.linux_ipa_response(&handle).is_err());
        self.pump();
    }
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn linuxlink_runtime_profile_cannot_send_init_without_host_ipa_server() {
    let link = Session::native_runtime_profile(Config {
        local_node: 1, peer_node: 0, local_channel: 1, remote_channel: 2,
    }, 1).unwrap();
    let mut rig = LinuxRig::from_prepared_link(link, true);
    assert!(rig.link.endpoint(Service::IpaServer).is_none());
    assert_eq!(rig.link.control_status().ready_services & (1 << 4), 0);
    let mut core = rig.core();
    let request = core.prepare_init_driver(1, 0, 0x415).unwrap();
    assert_eq!(rig.link.queue_linux_ipa(1, &request, None),
        Err(Error::Control(qcom_qrtr_control::Error::Unavailable)));
    rig.pump();
    assert!(rig.output.is_empty());
}

#[test]
fn linuxlink_requires_published_host_service_and_unique_modem_discovery() {
    for (available, discovered) in [(false, true), (true, false)] {
        let mut rig = LinuxRig::with_services(available, discovered);
        let mut core = linux::ColdSession::new(
            1,
            linux::Address {
                node: 0,
                port: LINUX_MODEM,
            },
        )
        .unwrap();
        let p = core.prepare_init_driver(1, 1, 0x415).unwrap();
        assert_eq!(
            rig.link.queue_linux_ipa(1, &p, None),
            Err(Error::Control(qcom_qrtr_control::Error::Unavailable))
        );
        rig.pump();
        assert!(rig.output.is_empty());
        assert!(!core.protocol_complete());
    }
}

#[test]
fn linuxlink_frame_matches_original_linux_glink_and_qrtr_header_assignments() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let p = core.prepare_init_driver(1, 0x1234, 0x415).unwrap();
    let record = rig.queued(&p, None);
    assert_eq!(record.0, 9);
    let text = include_str!(env!("SUNFISH_LINUX_QMI_LINK_C")).trim();
    let golden = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    // The pinned wire fixture forces confirmation; Linux flow control does
    // not request it on the first packet to a destination.
    #[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
    let golden = { let mut bytes = golden; bytes[32] = 0; bytes };
    assert_eq!(rig.frames.last().unwrap(), &golden);
}

#[test]
fn linuxlink_full_cold_exchange_reuses_actual_transport_in_both_completion_orders() {
    for glink_first in [false, true] {
        let mut rig = LinuxRig::new();
        let mut core = rig.core();
        let p = core.prepare_init_driver(1, 0, 0x415).unwrap();
        let record = rig.queued(&p, None);
        assert_eq!(
            (word(&record.1, 12), word(&record.1, 28)),
            (LINUX_CLIENT, LINUX_MODEM)
        );
        rig.complete(&p, &mut core, &record, glink_first);
        let response = rig.response(0);
        rig.accept_response(&mut core, response);
        assert!(core.state().modem_ready);
        assert!(!core.protocol_complete());
        for (id, txn) in [(0x20, 1), (0x35, 2)] {
            let handle = rig.request(id, txn, 0xfe);
            let p = rig.prepare_reply(&mut core, &handle);
            let record = rig.queued(&p, Some(&handle));
            assert_eq!(
                (word(&record.1, 12), word(&record.1, 28)),
                (LINUX_SERVER, LINUX_PEER_CLIENT)
            );
            assert_eq!(rig.link.in_flight(), 1);
            rig.complete(&p, &mut core, &record, glink_first);
            assert_eq!(rig.link.in_flight(), 0);
        }
        let p = core.prepare_indication(1, 0x2468).unwrap().unwrap();
        let record = rig.queued(&p, None);
        assert_eq!(Message::decode(p.bytes()).unwrap().transaction, 0x2468);
        assert_eq!(
            (word(&record.1, 12), word(&record.1, 28)),
            (LINUX_SERVER, LINUX_MODEM)
        );
        assert!(!core.protocol_complete());
        rig.complete(&p, &mut core, &record, glink_first);
        assert!(core.protocol_complete());
        assert_eq!(core.driver_status(), Some(0xfe));
        assert!(!rig.link.transport_is_pristine());
    }
}

#[test]
fn linuxlink_early_response_is_held_until_request_has_both_completions() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let p = core.prepare_init_driver(1, 0x1234, 0x415).unwrap();
    let record = rig.queued(&p, None);
    let handle = rig.response(0x1234);
    let (peer, bytes) = rig.link.linux_ipa_response(&handle).unwrap();
    assert_eq!(
        core.receive_init_response(1, peer, bytes),
        Err(linux::Error::NotPublished)
    );
    assert!(!core.state().modem_ready);
    rig.complete(&p, &mut core, &record, false);
    rig.accept_response(&mut core, handle);
    assert!(core.state().modem_ready);
}

#[test]
fn linuxlink_all_six_handshake_orders_use_real_request_tickets() {
    for order in [b"IRU", b"IUR", b"RIU", b"RUI", b"UIR", b"URI"] {
        let mut rig = LinuxRig::new();
        let mut core = rig.core();
        let p = core.prepare_init_driver(1, 3, 0x415).unwrap();
        let r = rig.queued(&p, None);
        rig.complete(&p, &mut core, &r, true);
        for (step, input) in order.iter().enumerate() {
            if *input == b'I' {
                let handle = rig.response(3);
                rig.accept_response(&mut core, handle);
            } else {
                let handle = rig.request(if *input == b'R' { 0x20 } else { 0x35 }, step as u16, 0);
                let p = rig.prepare_reply(&mut core, &handle);
                let r = rig.queued(&p, Some(&handle));
                rig.complete(&p, &mut core, &r, step % 2 == 0);
            }
            if let Some(p) = core.prepare_indication(1, 9).unwrap() {
                let r = rig.queued(&p, None);
                rig.complete(&p, &mut core, &r, false);
            }
            assert_eq!(core.protocol_complete(), step == 2);
        }
    }
}

#[test]
fn linuxlink_profiles_and_local_ports_cannot_overlap() {
    let mut s = session();
    s.enable_linux_ipa(1, 0x4004).unwrap();
    assert!(s.enable_ipa_client(1, 0x4005).is_err());
    assert!(s.enable_wlfw_client(1, 0x4004).is_err());
    assert!(s.enable_servreg_notif_client(1, 0x4004).is_err());
    assert!(s.prepare_tftp_server(1, 0x4004).is_err());
    assert!(s.register(1, Service::IpaServer, 0x4004).is_err());
    let mut s = session();
    s.enable_ipa_client(1, 0x4005).unwrap();
    assert!(s.enable_linux_ipa(1, 0x4004).is_err());
    let mut s = session();
    s.prepare_tftp_server(1, 0x4004).unwrap();
    assert!(s.enable_linux_ipa(1, 0x4004).is_err());
    let mut rig = LinuxRig::new();
    assert!(rig.link.enable_linux_ipa(1, 0x4005).is_err());
}

#[test]
fn linuxlink_record_replay_foreign_owner_and_wrong_request_are_rejected() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let handle = rig.request(0x20, 1, 1);
    let p = rig.prepare_reply(&mut core, &handle);
    assert_eq!(
        rig.link.queue_linux_ipa(1, &p, None),
        Err(Error::InvalidReply)
    );
    let other = rig.request(0x35, 2, 1);
    assert_eq!(
        rig.link.queue_linux_ipa(1, &p, Some(&other)),
        Err(Error::InvalidReply)
    );
    let record = rig.queued(&p, Some(&handle));
    assert!(rig.link.queue_linux_ipa(1, &p, Some(&handle)).is_err());
    rig.complete(&p, &mut core, &record, true);
    assert!(rig.link.queue_linux_ipa(1, &p, Some(&handle)).is_err());
    let mut foreign = rig.core();
    let p2 = foreign.prepare_init_driver(1, 1, 0x415).unwrap();
    assert_eq!(
        rig.link.queue_linux_ipa(1, &p2, None),
        Err(Error::InvalidReply)
    );
    assert!(!foreign.state().modem_ready);
}

#[test]
fn linuxlink_receipt_cannot_cross_sessions_records_or_generations() {
    let mut a = LinuxRig::new();
    let mut b = LinuxRig::new();
    let mut core = a.core();
    let p = core.prepare_init_driver(1, 1, 0x415).unwrap();
    let record = a.queued(&p, None);
    a.ack_one(&record, true);
    a.ack_one(&record, false);
    let receipt = a.link.take_linux_ipa_receipt(1).unwrap().unwrap();
    assert_eq!(
        b.link.commit_linux_ipa(1, &mut core, &p, receipt),
        Err(Error::InvalidReply)
    );
    assert!(!core.state().modem_ready);
    assert!(a.link.take_linux_ipa_receipt(1).unwrap().is_none());
    assert!(b.link.queue_linux_ipa(2, &p, None).is_err());
    let p2 = core.prepare_indication(1, 0).unwrap();
    assert!(p2.is_none());
}

#[test]
fn linuxlink_unknown_or_wrong_response_route_is_held_without_readiness() {
    for (peer, port, kind, id) in [
        (28, LINUX_CLIENT, 2, 0x21),
        (LINUX_MODEM, LINUX_CLIENT, 4, 0x21),
        (LINUX_MODEM, LINUX_CLIENT, 2, 0x22),
        (LINUX_MODEM, LINUX_SERVER, 2, 0x21),
    ] {
        let mut rig = LinuxRig::new();
        let mut bytes = ipa_init_response(1, 0, 0);
        bytes[0] = kind;
        bytes[3..5].copy_from_slice(&u16::to_le_bytes(id));
        assert!(rig.input(&qrtr(1, peer, port, true, &bytes)).is_err());
        assert!(rig.link.take_linux_ipa_receipt(1).unwrap().is_none());
    }
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let p = core.prepare_init_driver(1, 7, 0x415).unwrap();
    let record = rig.queued(&p, None);
    rig.complete(&p, &mut core, &record, true);
    let h = rig.response(8);
    let (peer, bytes) = rig.link.linux_ipa_response(&h).unwrap();
    assert_eq!(
        core.receive_init_response(1, peer, bytes),
        Err(linux::Error::WrongTransaction)
    );
    assert!(!core.state().modem_ready);
}

#[cfg(feature = "sunfish-ipa-init-tx-trace")]
#[test]
fn linuxlink_init_trace_records_blocked_target_and_exact_publication() {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let mut rig = LinuxRig::new();
    rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));
    let mut core = rig.core();
    let init = core.prepare_init_driver(1, 0, 0x415).unwrap();
    rig.link.queue_linux_ipa(1, &init, None).unwrap();
    let mut blocked_bytes = Vec::new();
    assert_eq!(rig.link.publish_next(1, |record| {
        blocked_bytes.extend_from_slice(record);
        Err(WriteError::WouldBlock)
    }), Err(Error::WouldBlock));
    let blocked = rig.link.ipa_init_transport_trace().unwrap();
    assert_eq!(blocked.tx.passes, 1);
    assert_eq!(blocked.tx.target_blocked, 1);
    assert_eq!(blocked.tx.no_fit, 0);
    assert_eq!(blocked.tx.selected_at, blocked.tx.first_writer_at);
    assert!(blocked.tx.selected_at.is_some());
    #[cfg(feature = "sunfish-ipa-init-writer-fit")]
    assert!(blocked.tx.first_fit_at.is_some() && blocked.tx.first_fit_at <= blocked.tx.selected_at);
    assert!(blocked.published_at.is_none() && blocked.retired_at.is_none());

    let published = rig.link.publish_next(1, |record| {
        assert_eq!(record, blocked_bytes);
        Ok(())
    }).unwrap();
    assert!(matches!(published, Some(Published::Data { last_fragment: true, .. })));
    let sent = rig.link.ipa_init_transport_trace().unwrap();
    assert_eq!(sent.tx.passes, 2);
    assert_eq!(sent.tx.target_blocked, 1);
    assert_eq!(sent.tx.selected_at, blocked.tx.selected_at);
    assert!(sent.tx.selected_at < sent.published_at);
    assert!(sent.retired_at.is_none());
}

#[cfg(all(feature = "sunfish-ipa-init-tx-turn", feature = "sunfish-wlan-intent-ack-turn"))]
#[test]
fn linuxlink_init_priority_preserves_grant_credit_and_falls_back_after_block() {
    for blocked in [false, true] {
        let mut rig = LinuxRig::new();
        let mut core = rig.core();
        let init = core.prepare_init_driver(1, 0, 0x415).unwrap();
        let request = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
        let intent = rig.available.remove(0);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, true, &request);
        assert!(matches!(rig.link.ingest(1, &data(9, intent, &packet, 0)), Ok(Event::Request(_))));
        rig.link.queue_linux_ipa(1, &init, None).unwrap();
        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));

        assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        let (event, credit) = publish_until_data(&mut rig.link);
        assert_eq!(word(&credit, 20), 7);
        let Published::Data { remote_intent, .. } = event else { unreachable!() };
        rig.link.ingest(1, &control(6, remote_intent)).unwrap();
        let advertising = rig.link.offer_receive(1).unwrap();
        assert_eq!(rig.link.ipa_init_transport_trace().unwrap().tx.priority_turn, 0);

        if blocked {
            assert_eq!(rig.link.publish_next(1, |record| {
                assert_eq!(&record[48..53], &[0, 0, 0, 0x21, 0]);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            let trace = rig.link.ipa_init_transport_trace().unwrap();
            assert_eq!(trace.tx.priority_turn, 1);
            assert_eq!(trace.tx.target_blocked, 1);
            assert!(trace.published_at.is_none());
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(advertising));
        }
        let (event, frame) = publish_until_data(&mut rig.link);
        assert_eq!(&frame[48..53], &[0, 0, 0, 0x21, 0]);
        let trace = rig.link.ipa_init_transport_trace().unwrap();
        assert_eq!(trace.tx.priority_turn, if blocked { 1 } else { 3 });
        assert_eq!(rig.link.linux_ipa_receipt_pending(), Some(false));
        #[cfg(feature = "sunfish-ipa-init-writer-counts")]
        {
            #[cfg(feature = "sunfish-ipa-init-writer-fit")]
            {
                assert_eq!(trace.tx.data_kinds[0], 1, "one QRTR credit ahead");
                assert_eq!(trace.tx.data_kinds.iter().sum::<u32>(), trace.tx.data_ahead);
                assert_eq!(trace.tx.no_fit, 0);
            }
            assert!(trace.tx.gates[0] >= 2, "grant advertisement and ACK both gate init");
            assert!(trace.tx.gates[4] > 0, "ready QRTR credit gates init");
            assert_eq!(trace.tx.gates[7], 1, "one priority attempt even if blocked");
            assert_eq!(trace.tx.gates[6] > 0, blocked);
            assert_eq!(trace.tx.commands[0], if blocked { 2 } else { 1 });
            assert_eq!(trace.tx.commands[3], 1);
            assert_eq!(trace.tx.commands.iter().sum::<u32>(), trace.tx.control_ahead);
        }
        if !blocked {
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(advertising));
        }
        let Published::Data { remote_intent, .. } = event else { unreachable!() };
        rig.link.ingest(1, &control(6, remote_intent)).unwrap();
        assert_eq!(rig.link.linux_ipa_receipt_pending(), Some(true));
        #[cfg(feature = "sunfish-ipa-init-writer-counts")]
        assert_eq!(rig.link.ipa_init_transport_trace().unwrap().tx, trace.tx,
            "later control work and completion cannot change the queued-init window");
    }
}

#[test]
fn linuxlink_wouldblock_preserves_bytes_and_uncertain_write_is_terminal() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let p = core.prepare_init_driver(1, 1, 0x415).unwrap();
    rig.link.queue_linux_ipa(1, &p, None).unwrap();
    let mut first = Vec::new();
    assert_eq!(
        rig.link.publish_next(1, |r| {
            first.extend_from_slice(r);
            Err(WriteError::WouldBlock)
        }),
        Err(Error::WouldBlock)
    );
    let mut second = Vec::new();
    assert_eq!(
        rig.link.publish_next(1, |r| {
            second.extend_from_slice(r);
            Err(WriteError::Uncertain)
        }),
        Err(Error::PublicationUncertain)
    );
    assert_eq!(first, second);
    assert_eq!(
        rig.link.take_linux_ipa_receipt(1).err(),
        Some(Error::Quarantined)
    );
    core.publication_uncertain(1, &p).unwrap();
    assert!(core.state().failed);
    assert!(!core.protocol_complete());
    assert_eq!(
        rig.link.publish_next(1, |_| panic!()).err(),
        Some(Error::Quarantined)
    );
}

#[test]
fn linuxlink_peer_loss_and_replacement_quarantine_the_retained_record() {
    for packet in [
        qrtr_control(5, [0x31, 0x201, 0, LINUX_MODEM]),
        qrtr_control(4, [0x31, 0x201, 0, 28]),
        qrtr_control(3, [0; 4]),
    ] {
        let mut rig = LinuxRig::new();
        let mut core = rig.core();
        let p = core.prepare_init_driver(1, 1, 0x415).unwrap();
        let _ = rig.queued(&p, None);
        assert!(rig.input(&packet).is_err());
        assert_eq!(
            rig.link.take_linux_ipa_receipt(1).err(),
            Some(Error::Quarantined)
        );
        core.peer_lost(1).unwrap();
        assert!(!core.protocol_complete());
    }
}

#[test]
fn linuxlink_completed_record_cannot_replay_or_overwrite_an_unconsumed_receipt() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let p = core.prepare_init_driver(1, 1, 0x415).unwrap();
    let record = rig.queued(&p, None);
    rig.ack_one(&record, true);
    rig.ack_one(&record, false);
    assert!(rig.link.queue_linux_ipa(1, &p, None).is_err());
    // Deliberately drive the public bookkeeping API without transport proof:
    // even this cannot overwrite the link's real, unconsumed receipt.
    core.commit_publication(1, &p).unwrap();
    let handle = rig.request(0x20, 2, 1);
    let next = rig.prepare_reply(&mut core, &handle);
    // Independent sockets may queue the server reply, but must preserve the
    // client receipt and reject using it to commit that reply below.
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    assert_eq!(rig.link.queue_linux_ipa(1, &next, Some(&handle)), Ok(()));
    #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
    assert_eq!(
        rig.link.queue_linux_ipa(1, &next, Some(&handle)),
        Err(Error::FlowBusy)
    );
    let receipt = rig.link.take_linux_ipa_receipt(1).unwrap().unwrap();
    assert!(
        rig.link
            .commit_linux_ipa(1, &mut core, &next, receipt)
            .is_err()
    );
    assert!(!core.state().indication_requested);
    assert!(rig.link.queue_linux_ipa(1, &p, None).is_err());
}

#[test]
fn linuxlink_owner_binding_and_handles_survive_neither_foreign_session_nor_reset() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let p = core.prepare_init_driver(1, 1, 0x415).unwrap();
    let record = rig.queued(&p, None);
    rig.complete(&p, &mut core, &record, true);
    let handle = rig.request(0x35, 2, 1);
    let mut foreign = rig.core();
    let first = foreign.prepare_init_driver(1, 1, 0x415).unwrap();
    foreign.commit_publication(1, &first).unwrap();
    let next = rig.prepare_reply(&mut foreign, &handle);
    assert_eq!(
        rig.link.queue_linux_ipa(1, &next, Some(&handle)),
        Err(Error::InvalidReply)
    );
    let mut stale = linux::ColdSession::new(
        2,
        linux::Address {
            node: 0,
            port: LINUX_MODEM,
        },
    )
    .unwrap();
    let stale_p = stale.prepare_init_driver(2, 1, 0x415).unwrap();
    assert_eq!(
        rig.link.queue_linux_ipa(1, &stale_p, None),
        Err(Error::InvalidReply)
    );
    let response = rig.response(1);
    let other = LinuxRig::new();
    assert_eq!(
        other.link.linux_ipa_response(&response).err(),
        Some(Error::InvalidHandle)
    );
    rig.link.reset_after_transport_reset(2).unwrap();
    assert_eq!(
        rig.link.linux_ipa_response(&response).err(),
        Some(Error::StaleGeneration)
    );
    assert!(rig.link.queue_linux_ipa(2, &p, None).is_err());
    assert!(rig.link.take_linux_ipa_receipt(2).unwrap().is_none());
}

#[cfg(feature = "sunfish-ipa-registration-tx-turn")]
#[test]
fn linuxlink_registration_turn_preserves_grant_and_credit_and_falls_back_after_block() {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    for blocked in [false, true] {
        let mut rig = LinuxRig::new();
        rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));
        let mut core = rig.core();
        let init = core.prepare_init_driver(1, 1, 0x415).unwrap();

        let request = ipa_server_request(2, 0x20, &[(0x10, &[1])]);
        let intent = rig.available.remove(0);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, true, &request);
        let Event::Request(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
        else { panic!("expected registration request"); };
        let reply = rig.prepare_reply(&mut core, &handle);
        rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();

        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        let (credit, frame) = publish_until_data(&mut rig.link);
        assert_eq!(word(&frame, 20), 7);
        let Published::Data { remote_intent, .. } = credit else { unreachable!() };
        rig.link.ingest(1, &control(6, remote_intent)).unwrap();

        #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
        if blocked {
            let other = rig.link.offer_receive(1).unwrap();
            assert_eq!(rig.link.publish_next(1, |record| {
                assert_eq!(word(record, 0) & 0xffff, 6);
                assert_eq!(word(record, 4), intent);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            let trace = rig.link.ipa_registration_trace().unwrap().tx;
            assert!(trace.request_done_attempted);
            assert!(trace.request_done_at.is_none());
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(other));
        }
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(intent));
        rig.link.queue_linux_ipa(1, &init, None).unwrap();
        let before = rig.link.ipa_registration_trace().unwrap().tx;
        assert!(before.request_done_at.is_some());
        assert_eq!(before.priority_deferrals[2], 2);
        assert_eq!(before.priority_deferrals[3], 1);
        #[cfg(not(feature = "sunfish-ipa-registration-rxdone-turn"))]
        assert_eq!(before.priority_deferrals[4], 1);
        #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
        {
            assert!(before.request_done_attempted);
            assert_eq!(before.priority_deferrals[4], if blocked { 3 } else { 1 });
        }
        assert!(!before.priority_selected);
        let advertising = rig.link.offer_receive(1).unwrap();
        let expected = if blocked { Err(WriteError::WouldBlock) } else { Ok(()) };
        let result = rig.link.publish_next(1, |record| {
            assert_eq!(&record[48..48 + reply.bytes().len()], reply.bytes());
            expected
        });
        assert!(rig.link.ipa_registration_trace().unwrap().tx.priority_selected);
        if blocked {
            assert_eq!(result, Err(Error::WouldBlock));
            let trace = rig.link.ipa_registration_trace().unwrap();
            assert_eq!(trace.tx.target_blocked, 1);
            assert!(trace.published_at.is_none());
            #[cfg(feature = "sunfish-reg-writer-timeline")]
            {
                assert!(trace.tx.selected_at.is_some());
                assert_eq!(trace.tx.committed_write_start, None);
            }
        } else {
            assert!(matches!(result, Ok(Some(Published::Data { last_fragment: true, .. }))));
        }
        let (_, frame) = publish_until_data(&mut rig.link);
        assert_eq!(&frame[48..48 + init.bytes().len()], init.bytes());
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(advertising));
        if blocked {
            let (_, frame) = publish_until_data(&mut rig.link);
            assert_eq!(&frame[48..48 + reply.bytes().len()], reply.bytes());
        }
        #[cfg(feature = "sunfish-reg-writer-timeline")]
        {
            let reg = rig.link.ipa_registration_trace().unwrap();
            let first = reg.tx.selected_at.unwrap();
            let committed = reg.tx.committed_write_start.unwrap();
            assert_eq!(first < committed, blocked);
            assert!(first <= committed && committed < reg.tx.last_end.unwrap());
            assert!(reg.tx.last_end.unwrap() < reg.published_at.unwrap());
        }
    }
}

#[cfg(feature = "sunfish-ipa-before-first-oack")]
#[test]
fn linuxlink_first_oack_waits_for_registration_receipt_or_cancels_on_close() {
    use crate::qcom_glink_qmi::TftpReplyRoute;
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    #[cfg(not(feature = "sunfish-tftp-oack-reserve"))]
    let cases = [(false, false, 0), (true, false, 0)];
    #[cfg(feature = "sunfish-tftp-oack-reserve")]
    let cases = [(false, false, 0), (true, false, 0), (true, true, 0),
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        (true, false, 1),
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        (true, false, 2),
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        (true, false, 3),
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        (true, false, 4),
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        (true, false, 5),
        #[cfg(all(feature = "sunfish-tftp-oack-credit-retry", not(feature = "sunfish-tftp-oack-credit-early")))]
        (true, false, 6),
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        (true, false, 7),
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        (true, false, 8),
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        (true, false, 9),
        #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
        (true, false, 10),
        #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
        (true, false, 11),
        #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
        (true, false, 12),
        #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
        (true, false, 13),
        #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
        (true, false, 14),
        #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
        (true, false, 15),
    ];
    for (close, _cancel_reserve, _early_case) in cases {
        let mut link = session();
        link.enable_ipa_before_first_oack();
        link.register(1, Service::IpaServer, LINUX_SERVER).unwrap();
        link.enable_linux_ipa(1, LINUX_CLIENT).unwrap();
        link.prepare_service_available(1, Service::IpaServer).unwrap();
        link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
        let mut rig = LinuxRig::from_prepared_link(link, true);
        rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));

        let peer = Address { node: 0, port: 0x1e };
        let service = Address { node: 1, port: tftp::SERVICE_PORT };
        let wrq = qrtr(1, peer.port, service.port, false, tftp::REQUEST);
        let Event::TftpRequest(handle) = rig.input(&wrq).unwrap()
        else { panic!("expected first WRQ"); };
        let mut bytes = [0; 64];
        let mut server = tftp::ServerCheck::new();
        let result = server.receive(tftp::Route {
            source: tftp::Address { node: peer.node, port: peer.port },
            destination: tftp::Address { node: service.node, port: service.port },
        }, tftp::REQUEST, &mut bytes);
        let reply = result.unwrap();
        rig.link.observe_elf_server_check_dispatch(&handle,
            TftpReplyRoute { source: service, destination: peer }, true, result, &bytes);
        rig.link.finish_tftp_request(&handle).unwrap();
        rig.link.queue_traced_tftp_response(1, &handle, TftpReplyRoute {
            source: Address { node: 1, port: reply.route.source.port }, destination: peer,
        }, &bytes[..reply.len], false).unwrap();

        let registration = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
        let intent = rig.available.remove(0);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &registration);
        let Event::Request(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
        else { panic!("expected registration request"); };
        let prepared = rig.prepare_reply(&mut rig.core(), &handle);
        rig.link.queue_linux_ipa(1, &prepared, Some(&handle)).unwrap();
        #[cfg(feature = "sunfish-ipa-reply-before-grant")]
        {
            assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(_)));
            assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(_)));
            assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
            let first = rig.link.publish_next(1, |frame| {
                assert_eq!(&frame[48..48 + prepared.bytes().len()], prepared.bytes());
                if close { Err(WriteError::WouldBlock) } else { Ok(()) }
            });
            if close {
                assert_eq!(first, Err(Error::WouldBlock));
            } else {
                let Ok(Some(Published::Data { remote_intent, .. })) = first else { panic!("{first:?}"); };
                assert_eq!(rig.link.ingest(1, &control(14, remote_intent)), Ok(Event::TransmitDone));
            }
            assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
            if !close {
                let (_, record) = publish_until_data(&mut rig.link);
                assert_eq!(&record[48..48 + tftp::OACK.len()], tftp::OACK);
                continue;
            }
        }
        let (published, record) = publish_until_data(&mut rig.link);
        assert_eq!(&record[48..48 + prepared.bytes().len()], prepared.bytes());
        let Published::Data { remote_intent, .. } = published else { unreachable!() };
        for _ in 0..32 {
            match rig.link.publish_next(1, |_| Ok(())) {
                Err(Error::NoRemoteIntent) => break,
                Ok(Some(Published::ReceiveDone(_) | Published::ReceiveIntent(_))) => {}
                other => panic!("unexpected publication before matching receipt: {other:?}"),
            }
        }
        assert!(rig.link.server_check_oacks()[0].unwrap().oack_at.is_none());

        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        if close {
            let replay_credit = _early_case >= 3;
            for offset in 0..(6 - u32::from(replay_credit)) {
                rig.link.queue_tftp_response(1, TftpReplyRoute {
                    source: Address { node: 1, port: 0x5003 + offset },
                    destination: Address { node: 0, port: 0x60 + offset },
                }, tftp::OACK).unwrap();
                let (_, frame) = publish_until_data(&mut rig.link);
                assert_eq!(word(&frame, 44), 0x60 + offset);
            }
            assert_eq!(rig.link.tftp_tx_occupied(), if replay_credit { 0x7f } else { 0xff });
            let route = TftpReplyRoute {
                source: Address { node: 1, port: 0x5002 },
                destination: Address { node: 0, port: 0x49 },
            };
            if !replay_credit {
                assert_eq!(rig.link.queue_tftp_response(1, route, tftp::OACK), Err(Error::NoCapacity));
            }
            let incoming = rig.link.offer_receive(1).unwrap();
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(incoming));
            let close_intent = if _cancel_reserve {
                let id = rig.link.offer_receive(1).unwrap();
                assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(id));
                Some(id)
            } else { None };
            let wrq = qrtr(1, 0x49, service.port, replay_credit, tftp::REQUEST);
            let Event::TftpRequest(later) = rig.link.ingest(1, &data(9, incoming, &wrq, 0)).unwrap()
            else { panic!("expected later WRQ"); };
            #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
            {
                let dispatch = server.receive(tftp::Route {
                    source: tftp::Address { node: 0, port: 0x49 },
                    destination: tftp::Address { node: 1, port: service.port },
                }, tftp::REQUEST, &mut bytes);
                assert_eq!(dispatch.unwrap().route.source.port, route.source.port);
                rig.link.observe_elf_server_check_dispatch(&later,
                    TftpReplyRoute { source: service, destination: route.destination }, true, dispatch, &bytes);
            }
            rig.link.finish_tftp_request(&later).unwrap();
            rig.link.queue_traced_tftp_response(1, &later, route, tftp::OACK, false).unwrap();
            let (reserved, _) = rig.link.reserved_oack().unwrap();
            assert_eq!(reserved.peer, route.destination);
            #[cfg(feature = "sunfish-tftp-server-inventory")]
            {
                let traces = rig.link.server_check_inventory();
                assert!(traces.iter().flatten().any(|trace|
                    trace.peer == route.destination && trace.transfer_port == route.source.port));
            }
            if let Some(intent) = close_intent {
                let packet = qrtr_control(6, [0, 0x49, 0, 0]);
                assert!(matches!(rig.link.ingest(1, &data(9, intent, &packet, 0)),
                    Ok(Event::QrtrControl(_))));
                for _ in 0..16 {
                    match rig.link.publish_next(1, |_| Ok(())) {
                        Ok(Some(Published::ReceiveDone(_) | Published::ReceiveIntent(_))) => {},
                        Ok(None) | Err(Error::NoRemoteIntent) => break,
                        other => panic!("unexpected publication after close: {other:?}"),
                    }
                }
                assert!(rig.link.reserved_oack().unwrap().0.published_at.is_none());
                continue;
            }
            assert_eq!(rig.link.tftp_tx_occupied(), 0xff);
            #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
            let sent = {
                use crate::qcom_glink_qmi::OackFollowup;
                assert!(rig.link.early_reserved_oack_pending());
                if _early_case >= 2 {
                    assert_eq!(rig.link.ingest(1, &control(7, 108)),
                        Ok(Event::ReceiveIntentRequest { granted: true }));
                }
                if replay_credit {
                    assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
                }
                let mut frame = Vec::new();
                let result = rig.link.publish_oack_followup(1, OackFollowup::EarlyReserve, |record| {
                    frame.extend_from_slice(record);
                    if _early_case == 1 { Err(WriteError::WouldBlock) } else { Ok(()) }
                });
                assert!(!rig.link.early_reserved_oack_pending());
                if replay_credit {
                    assert_eq!(result, Ok(None));
                    assert!(frame.is_empty());
                    let (oack, _) = rig.link.reserved_oack().unwrap();
                    assert_eq!((oack.early_status, oack.early_command, oack.early_guards),
                        (3, 8, 0x0012));
                    assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
                    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                    {
                        let oack = rig.link.reserved_oack().unwrap().0;
                        assert_eq!(oack.retry_state, 2);
                        assert!(oack.retry_ack_at.is_some());
                        assert!(oack.retry_credit_at.is_none());
                    }

                    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                    if _early_case == 5 {
                        assert_eq!(rig.link.ingest(1, &control(7, 108)),
                            Ok(Event::ReceiveIntentRequest { granted: true }));
                        let mut selected = Vec::new();
                        let turn = rig.link.publish_next(1, |record| {
                            selected.extend_from_slice(record);
                            Ok(())
                        }).unwrap().unwrap();
                        assert_eq!(turn, Published::ReceiveDone(incoming));
                        assert_eq!(word(&selected, 0) & 0xffff, 6);
                        let (turn, selected) = publish(&mut rig.link).unwrap();
                        assert!(matches!(turn, Published::ReceiveIntent(_)), "selected {turn:?}");
                        assert_eq!(word(&selected, 0) & 0xffff, 5);
                        assert_eq!(publish(&mut rig.link).unwrap().0,
                            Published::IntentRequestAck { granted: true });
                    }
                    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                    if _early_case >= 6 {
                        assert_eq!(rig.link.ingest(1, &control(7, 108)),
                            Ok(Event::ReceiveIntentRequest { granted: true }));
                        #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                        {
                            assert!(rig.link.take_oack_credit_turn());
                            assert!(!rig.link.take_oack_credit_turn());
                        }
                        let mut frame = Vec::new();
                        let credit = rig.link.publish_oack_followup(1, OackFollowup::CreditReplay, |record| {
                            frame.extend_from_slice(record);
                            if _early_case == 7 { Err(WriteError::WouldBlock) } else { Ok(()) }
                        });
                        assert_eq!(word(&frame, 20), 7);
                        if _early_case == 7 {
                            assert_eq!(credit, Err(Error::WouldBlock));
                            assert_eq!(rig.link.reserved_oack().unwrap().0.retry_state, 2);
                            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(incoming));
                            assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
                            assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
                        } else {
                            let Published::Data { remote_intent, .. } = credit.unwrap().unwrap() else { panic!() };
                            assert_eq!(rig.link.reserved_oack().unwrap().0.retry_state, 3);
                            #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                            if _early_case >= 10 {
                                let mut advertised = None;
                                for _ in 0..8 {
                                    match publish(&mut rig.link).unwrap().0 {
                                        Published::ReceiveDone(_) => {},
                                        Published::ReceiveIntent(id) => advertised = Some(id),
                                        Published::IntentRequestAck { granted: true } => break,
                                        other => panic!("unexpected grant turn {other:?}"),
                                    }
                                }
                                let id = advertised.expect("backing advertisement");
                                let packet = qrtr(1, 0x66, service.port, false, tftp::REQUEST);
                                let Event::TftpRequest(request) = rig.link.ingest(1, &data(9, id, &packet, 0)).unwrap()
                                    else { panic!("expected competing WRQ"); };
                                rig.link.finish_tftp_request(&request).unwrap();
                                rig.link.replay_wait_for_oack_done(id);
                                assert!(!rig.link.take_oack_after_done());
                                let mut blocked = false;
                                for _ in 0..8 {
                                    rig.link.replay_clear_tftp_writer_turns();
                                    rig.link.replay_allow_receive_done();
                                    let turn = rig.link.publish_next(1, |record| {
                                        assert_eq!(word(record, 0) & 0xffff, 6);
                                        if word(record, 4) == id {
                                            Err(WriteError::WouldBlock)
                                        } else {
                                            Ok(())
                                        }
                                    });
                                    assert_eq!(rig.link.reserved_oack().unwrap().0.after_done_state, 1);
                                    match turn {
                                        Err(Error::WouldBlock) => { blocked = true; break; }
                                        Ok(Some(Published::ReceiveDone(other))) => assert_ne!(other, id),
                                        other => panic!("unexpected completion turn {other:?}"),
                                    }
                                }
                                assert!(blocked);
                                rig.link.replay_clear_tftp_writer_turns();
                                rig.link.replay_allow_receive_done();
                                assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(id));
                                assert_eq!(rig.link.reserved_oack().unwrap().0.after_done_state, 2);
                                #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
                                if _early_case >= 12 {
                                    assert_eq!(rig.link.ingest(1, &control(7, 108)),
                                        Ok(Event::ReceiveIntentRequest { granted: true }));
                                    assert!(rig.link.take_oack_after_done());
                                    assert!(!rig.link.take_oack_after_done());
                                    let ad = rig.link.publish_oack_followup(1, OackFollowup::Advertise, |record| {
                                        assert_eq!(word(record, 0) & 0xffff, 5);
                                        if _early_case == 13 { Err(WriteError::WouldBlock) } else { Ok(()) }
                                    });
                                    if _early_case == 13 {
                                        assert_eq!(ad, Err(Error::WouldBlock));
                                        assert_eq!(rig.link.reserved_oack().unwrap().0.after_done_ad, 2);
                                        assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
                                        assert_eq!(publish(&mut rig.link).unwrap().0,
                                            Published::IntentRequestAck { granted: true });
                                        assert_eq!(word(&publish_until_data(&mut rig.link).1, 44), 0x49);
                                        continue;
                                    }
                                    assert!(matches!(ad, Ok(Some(Published::ReceiveIntent(_)))));
                                    let ack = rig.link.publish_oack_followup(1, OackFollowup::Grant, |record| {
                                        assert_eq!(word(record, 0) & 0xffff, 8);
                                        if _early_case == 14 { Err(WriteError::WouldBlock) } else { Ok(()) }
                                    });
                                    if _early_case == 14 {
                                        assert_eq!(ack, Err(Error::WouldBlock));
                                        let trace = rig.link.reserved_oack().unwrap().0;
                                        assert_eq!((trace.after_done_ad, trace.after_done_ack), (1, 2));
                                        assert!(trace.published_at.is_none());
                                        assert_eq!(publish(&mut rig.link).unwrap().0,
                                            Published::IntentRequestAck { granted: true });
                                        assert_eq!(word(&publish_until_data(&mut rig.link).1, 44), 0x49);
                                        continue;
                                    }
                                    assert_eq!(ack, Ok(Some(Published::IntentRequestAck { granted: true })));
                                    let mut frame = Vec::new();
                                    let reply = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                                        frame.extend_from_slice(record);
                                        if _early_case == 15 { Err(WriteError::WouldBlock) } else { Ok(()) }
                                    });
                                    assert_eq!(word(&frame, 44), 0x49);
                                    assert_eq!(&frame[48..48 + tftp::OACK.len()], tftp::OACK);
                                    let trace = rig.link.reserved_oack().unwrap().0;
                                    if _early_case == 15 {
                                        assert_eq!(reply, Err(Error::WouldBlock));
                                        assert_eq!((trace.after_done_ad, trace.after_done_ack, trace.after_done_result), (1, 1, 2));
                                        #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
                                        assert_eq!(trace.writer_trace.oack_blocked, 1);
                                        assert!(trace.published_at.is_none());
                                        assert_eq!(publish_until_data(&mut rig.link).1, frame);
                                    } else {
                                        assert!(matches!(reply, Ok(Some(Published::Data { .. }))));
                                        assert_eq!((trace.after_done_ad, trace.after_done_ack, trace.after_done_result), (1, 1, 1));
                                    }
                                    continue;
                                }
                                assert!(rig.link.take_oack_after_done());
                                assert!(!rig.link.take_oack_after_done());
                                let mut frame = Vec::new();
                                let retry = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                                    frame.extend_from_slice(record);
                                    if _early_case == 11 { Err(WriteError::WouldBlock) } else { Ok(()) }
                                });
                                assert_eq!(word(&frame, 44), 0x49);
                                assert_eq!(&frame[48..48 + tftp::OACK.len()], tftp::OACK);
                                let oack = rig.link.reserved_oack().unwrap().0;
                                assert!(oack.after_done_at < oack.after_done_retry_at);
                                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                                {
                                    assert_eq!(oack.after_done_origin, 1);
                                    assert_eq!(oack.after_done_choice, 4);
                                    assert_eq!(oack.after_done_choice_id, 0x49);
                                    assert!(oack.after_done_first_check.is_some());
                                }
                                if _early_case == 11 {
                                    assert_eq!(retry, Err(Error::WouldBlock));
                                    assert_eq!(oack.after_done_result, 2);
                                    assert!(oack.published_at.is_none());
                                    assert_eq!(publish_until_data(&mut rig.link).1, frame);
                                } else {
                                    assert!(matches!(retry, Ok(Some(Published::Data { .. }))));
                                    assert_eq!(oack.after_done_result, 1);
                                }
                                continue;
                            }
                            if _early_case >= 8 {
                                assert!(!rig.link.replay_take_oack_after_grant());
                                assert_eq!(rig.link.reserved_oack().unwrap().0.retry_state, 3);
                            } else {
                                assert!(rig.link.take_oack_credit_retry());
                                assert!(!rig.link.take_oack_credit_retry());
                            }
                            let mut retry_writes = 0;
                            let retry = rig.link.publish_oack_followup(1, OackFollowup::Reply, |_| {
                                retry_writes += 1;
                                Ok(())
                            });
                            assert_eq!(retry, Ok(None));
                            assert_eq!(retry_writes, 0);
                            assert_eq!(rig.link.ingest(1, &control(14, remote_intent)), Ok(Event::TransmitDone));
                            let mut advertised = false;
                            let mut acked = false;
                            for _ in 0..8 {
                                match publish(&mut rig.link).unwrap().0 {
                                    Published::ReceiveDone(_) => {},
                                    Published::ReceiveIntent(_) => advertised = true,
                                    Published::IntentRequestAck { granted: true } => {
                                        assert!(advertised);
                                        acked = true;
                                        break;
                                    }
                                    other => panic!("unexpected grant turn {other:?}"),
                                }
                            }
                            assert!(acked);
                            if _early_case >= 8 {
                                assert!(rig.link.replay_take_oack_after_grant());
                                assert!(!rig.link.replay_take_oack_after_grant());
                                let mut frame = Vec::new();
                                let retry = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                                    frame.extend_from_slice(record);
                                    if _early_case == 9 { Err(WriteError::WouldBlock) } else { Ok(()) }
                                });
                                assert_eq!(word(&frame, 44), 0x49);
                                assert_eq!(&frame[48..48 + tftp::OACK.len()], tftp::OACK);
                                if _early_case == 9 {
                                    assert_eq!(retry, Err(Error::WouldBlock));
                                    assert!(rig.link.reserved_oack().unwrap().0.published_at.is_none());
                                    let (_, later) = publish_until_data(&mut rig.link);
                                    assert_eq!(later, frame);
                                } else {
                                    assert!(matches!(retry, Ok(Some(Published::Data { .. }))));
                                }
                            } else {
                                let (_, frame) = publish_until_data(&mut rig.link);
                                assert_eq!(word(&frame, 44), 0x49);
                            }
                            continue;
                        }
                    }
                    // Keep the credit's turn across any required RX_DONE.
                    let (credit, frame) = (0..16).find_map(|_| {
                        #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
                        rig.link.replay_clear_tftp_writer_turns();
                        let (event, frame) = publish(&mut rig.link).unwrap();
                        match event {
                            Published::ReceiveDone(_) | Published::ReceiveIntent(_) => None,
                            Published::Data { .. } => Some((event, frame)),
                            other => panic!("unexpected publication before credit: {other:?}"),
                        }
                    }).expect("QRTR credit did not publish");
                    assert_eq!(word(&frame, 20), 7); // QRTR RESUME_TX
                    assert_eq!(word(&frame, 44), 0x49);
                    let Published::Data { remote_intent, .. } = credit else { unreachable!() };

                    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                    {
                        let oack = rig.link.reserved_oack().unwrap().0;
                        assert_eq!(oack.retry_state, 3);
                        assert!(oack.retry_ack_at < oack.retry_credit_at);
                        assert!(rig.link.take_oack_credit_retry());
                        assert!(!rig.link.take_oack_credit_retry());
                        let oack = rig.link.reserved_oack().unwrap().0;
                        assert!(oack.retry_credit_at < oack.retry_attempt_at);
                    }
                    // The extra attempt only admits the original, whole OACK.
                    let mut frame = Vec::new();
                    let retry = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                        assert_eq!(word(record, 44), 0x49);
                        assert_eq!(&record[48..48 + tftp::OACK.len()], tftp::OACK);
                        frame.extend_from_slice(record);
                        if _early_case == 4 { Err(WriteError::WouldBlock) } else { Ok(()) }
                    });
                    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                    assert_eq!(rig.link.reserved_oack().unwrap().0.retry_result,
                        if _early_case == 4 { 2 } else { 1 });
                    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                    {
                        let oack = rig.link.reserved_oack().unwrap().0;
                        assert_eq!(oack.retry_choice, 4);
                        assert_eq!(oack.retry_choice_id, 0x49);
                        assert_ne!(oack.retry_gates & 0x10, 0);
                    }
                    let sent = if _early_case == 4 {
                        assert_eq!(retry, Err(Error::WouldBlock));
                        assert!(rig.link.reserved_oack().unwrap().0.published_at.is_none());
                        let sent = publish_until_data(&mut rig.link);
                        assert_eq!(sent.1, frame);
                        sent
                    } else {
                        (retry.unwrap().unwrap(), frame)
                    };
                    assert_eq!(rig.link.ingest(1, &control(14, remote_intent)), Ok(Event::TransmitDone));
                    sent
                } else if _early_case == 2 {
                    assert_eq!(result, Ok(None));
                    assert!(frame.is_empty());
                    let (oack, _) = rig.link.reserved_oack().unwrap();
                    assert_eq!(oack.early_status, 3);
                    assert_eq!(oack.early_command, 5);
                    assert_ne!(oack.early_guards & 0x0002, 0);
                    assert_eq!(oack.early_guards & 0x1000, 0);
                    assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
                    assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
                    publish_until_data(&mut rig.link)
                } else if _early_case == 1 {
                    assert_eq!(result, Err(Error::WouldBlock));
                    let (oack, _) = rig.link.reserved_oack().unwrap();
                    assert_eq!(oack.early_status, 2);
                    assert!(oack.published_at.is_none());
                    publish_until_data(&mut rig.link)
                } else {
                    let (oack, _) = rig.link.reserved_oack().unwrap();
                    assert_eq!(oack.early_status, 1);
                    assert_eq!(oack.early_command, 9);
                    assert_eq!(oack.early_guards & 0x1fdf, 0x1800);
                    (result.unwrap().unwrap(), frame)
                }
            };
            #[cfg(not(feature = "sunfish-tftp-oack-early-reserve"))]
            let sent = publish_until_data(&mut rig.link);
            let (Published::Data { remote_intent: oack_intent, .. }, frame) = sent
                else { panic!("expected reserved OACK"); };
            assert_eq!(word(&frame, 44), 0x49);
            assert_ne!(oack_intent, remote_intent);
            #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
            {
                let (snapshot, _) = rig.link.reserved_oack().unwrap();
                assert_eq!(snapshot.wire_len as usize, frame.len() - 16);
                assert_eq!(&snapshot.wire_bytes[..snapshot.wire_len as usize], &frame[16..]);
                assert_eq!(snapshot.wire_intent, oack_intent);
                #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
                {
                    assert!(snapshot.writer_trace.calls > 0);
                    assert_eq!(snapshot.writer_trace.oack_selected, 1);
                    assert!(snapshot.writer_trace.first_at.is_some());
                }
            }
            assert_eq!(rig.link.ingest(1, &control(14, oack_intent)), Ok(Event::TransmitDone));
            let (reserved, _) = rig.link.reserved_oack().unwrap();
            assert!(reserved.queued_at < reserved.published_at && reserved.published_at < reserved.done_at);
            assert!(rig.link.ipa_registration_trace().unwrap().completed_at.is_none());
            assert!(rig.link.server_check_oacks()[0].unwrap().oack_at.is_none());
            assert_eq!(rig.link.tftp_tx_occupied(), if replay_credit { 0x7f } else { 0xff });
            continue;
        }
        if close {
            let intent = rig.link.offer_receive(1).unwrap();
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(intent));
            let packet = qrtr_control(6, [0, peer.port, 0, 0]);
            assert!(matches!(rig.link.ingest(1, &data(9, intent, &packet, 0)), Ok(Event::QrtrControl(_))));
        }
        assert_eq!(rig.link.ingest(1, &control(6, remote_intent)), Ok(Event::TransmitDone));
        if close {
            rig.link.queue_tftp_response(1, TftpReplyRoute {
                source: Address { node: 1, port: reply.route.source.port },
                destination: Address { node: 0, port: peer.port + 1 },
            }, tftp::OACK).unwrap();
            let (other, _) = publish_until_data(&mut rig.link);
            let Published::Data { remote_intent, .. } = other else { unreachable!() };
            assert_eq!(rig.link.ingest(1, &control(6, remote_intent)), Ok(Event::TransmitDone));
            let first = rig.link.first_server_check().unwrap();
            assert_eq!(first.timing.first_close_oack_phase, Some(2));
            assert_eq!(first.oack.phase, 2);
            assert!(first.oack.published_at.is_none() && first.oack.done_at.is_none());
            assert!(rig.link.server_check_oacks()[0].unwrap().oack_at.is_none());
        } else {
            let (_, record) = publish_until_data(&mut rig.link);
            assert_eq!(&record[48..48 + tftp::OACK.len()], tftp::OACK);
            assert!(rig.link.ipa_registration_trace().unwrap().completed_at.unwrap()
                < rig.link.server_check_oacks()[0].unwrap().oack_at.unwrap());
        }
    }
}

#[cfg(all(feature = "sunfish-ipa-registration-between-grants", not(feature = "sunfish-ipa-registration-rxdone-before-grant")))]
#[test]
fn registration_done_between_positive_grants() {
    for blocked in [false, true] {
        let mut rig = LinuxRig::new();
        #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
        {
            use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
            static CLOCK: AtomicU64 = AtomicU64::new(1);
            rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));
        }
        let mut core = rig.core();
        let handle = rig.request(0x20, 2, 1);
        let reply = rig.prepare_reply(&mut core, &handle);
        rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();
        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(_)));
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        let (credit, frame) = publish_until_data(&mut rig.link);
        assert_eq!(word(&frame, 20), 7);
        let Published::Data { remote_intent, .. } = credit else { unreachable!() };
        rig.link.ingest(1, &control(6, remote_intent)).unwrap();

        // Re-arm the ACK yield with the exact registration RX_DONE pending.
        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        let Published::ReceiveIntent(backing) = publish(&mut rig.link).unwrap().0
        else { panic!("expected backing advertisement"); };
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        let result = rig.link.publish_next(1, |record| {
            assert_eq!(word(record, 0) & 0xffff, 6);
            if blocked { Err(WriteError::WouldBlock) } else { Ok(()) }
        });
        if blocked { assert_eq!(result, Err(Error::WouldBlock)); }
        else { assert!(matches!(result, Ok(Some(Published::ReceiveDone(_))))); }
        assert!(rig.link.ipa_registration_trace().unwrap().tx.request_done_attempted);
        #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
        {
            let trace = rig.link.ipa_registration_trace().unwrap().tx.request_done;
            assert_eq!(trace.passes, 6);
            assert_eq!(trace.turns[0], 1 | (2 << 3) | (4 << 6) | (1 << 9) | (2 << 12) | (7 << 15));
            assert_eq!(trace.failed_turns, if blocked { 1 << 5 } else { 0 });
            assert_eq!(trace.finished, !blocked);
            assert!(trace.first_pass_at.unwrap() < trace.first_eligible_at.unwrap());
            assert!(trace.gates[2] > 0);
            assert!(trace.gates[3] > 0);
        }
        let mut advertised = false;
        for _ in 0..16 {
            match publish(&mut rig.link).unwrap().0 {
                Published::ReceiveIntent(id) => advertised |= id == backing + 1,
                Published::IntentRequestAck { granted: true } => {
                    assert!(advertised);
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(rig.link.intent_ack_summary().0, 3);
        #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
        if !blocked {
            assert_eq!(rig.link.ipa_registration_trace().unwrap().tx.request_done.passes, 6);
        }
    }
}

#[cfg(feature = "sunfish-ipa-registration-reply-between-grants")]
#[test]
fn registration_reply_between_positive_grants_after_its_receive_completion() {
    for blocked in [false, true] {
        let mut rig = LinuxRig::new();
        let mut core = rig.core();
        let handle = rig.request(0x20, 2, 1);
        let reply = rig.prepare_reply(&mut core, &handle);
        rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();

        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        let Published::ReceiveIntent(first) = publish(&mut rig.link).unwrap().0
        else { panic!("expected first grant advertisement"); };
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        let (credit, frame) = publish_until_data(&mut rig.link);
        assert_eq!(word(&frame, 20), 7);
        let Published::Data { remote_intent, .. } = credit else { unreachable!() };
        rig.link.ingest(1, &control(6, remote_intent)).unwrap();

        assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(_)));
        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(first + 1));
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });

        assert_eq!(rig.link.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
        let result = rig.link.publish_next(1, |record| {
            assert_eq!(word(record, 0) & 0xffff, 9);
            assert_eq!(&record[48..48 + reply.bytes().len()], reply.bytes());
            if blocked { Err(WriteError::WouldBlock) } else { Ok(()) }
        });
        if blocked { assert_eq!(result, Err(Error::WouldBlock)); }
        else { assert!(matches!(result, Ok(Some(Published::Data { .. })))); }
        assert!(rig.link.ipa_registration_trace().unwrap().tx.priority_selected);
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(first + 2));
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
    }
}

#[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
#[test]
fn replay_first_close_during_pending_grant() {
    use crate::qcom_glink_qmi::TftpReplyRoute;
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    for blocked in [false, true] {
        let mut link = session();
        link.enable_ipa_before_first_oack();
        link.register(1, Service::IpaServer, LINUX_SERVER).unwrap();
        link.enable_linux_ipa(1, LINUX_CLIENT).unwrap();
        link.prepare_service_available(1, Service::IpaServer).unwrap();
        link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
        let mut rig = LinuxRig::from_prepared_link(link, true);
        rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));

        let peer = Address { node: 0, port: 0x1e };
        let service = Address { node: 1, port: tftp::SERVICE_PORT };
        let wrq = qrtr(1, peer.port, service.port, false, tftp::REQUEST);
        let Event::TftpRequest(handle) = rig.input(&wrq).unwrap()
        else { panic!("expected first WRQ"); };
        let mut bytes = [0; 64];
        let result = tftp::ServerCheck::new().receive(tftp::Route {
            source: tftp::Address { node: peer.node, port: peer.port },
            destination: tftp::Address { node: service.node, port: service.port },
        }, tftp::REQUEST, &mut bytes);
        let reply = result.unwrap();
        rig.link.observe_elf_server_check_dispatch(&handle,
            TftpReplyRoute { source: service, destination: peer }, true, result, &bytes);
        rig.link.finish_tftp_request(&handle).unwrap();
        rig.link.queue_traced_tftp_response(1, &handle, TftpReplyRoute {
            source: Address { node: 1, port: reply.route.source.port }, destination: peer,
        }, &bytes[..reply.len], false).unwrap();

        assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(_)));
        let spare_intent = rig.link.offer_receive(1).unwrap();
        assert!((0..16).any(|_| publish(&mut rig.link).unwrap().0
            == Published::ReceiveIntent(spare_intent)));

        let registration = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
        let intent = rig.available.remove(0);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &registration);
        let Event::Request(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
        else { panic!("expected registration request"); };
        let prepared = rig.prepare_reply(&mut rig.core(), &handle);
        rig.link.queue_linux_ipa(1, &prepared, Some(&handle)).unwrap();
        assert_eq!(rig.link.ingest(1, &control(7, 67)),
            Ok(Event::ReceiveIntentRequest { granted: true }));
        let close_intent = spare_intent;
        let packet = qrtr_control(6, [0, peer.port, 0, 0]);
        let mut first_command = 0;
        let first = rig.link.publish_next(1, |record| {
            first_command = word(record, 0) & 0xffff;
            if blocked && first_command == 6 { Err(WriteError::WouldBlock) } else { Ok(()) }
        });
        let candidate = cfg!(feature = "sunfish-ipa-registration-rxdone-before-grant");
        if candidate {
            assert_eq!(first_command, 6);
            if blocked { assert_eq!(first, Err(Error::WouldBlock)); }
            else { assert_eq!(first, Ok(Some(Published::ReceiveDone(intent)))); }
        } else {
            assert_eq!(first_command, 5);
            assert_eq!(first, Ok(Some(Published::ReceiveIntent(spare_intent + 1))));
        }
        assert!(matches!(rig.link.ingest(1, &data(9, close_intent, &packet, 0)), Ok(Event::QrtrControl(_))));
        if candidate {
            let (advert, frame) = publish(&mut rig.link).unwrap();
            let Published::ReceiveIntent(id) = advert else { panic!("{advert:?}") };
            assert_eq!(id, spare_intent + 1);
            assert_eq!(word(&frame, 12), id);
        }
        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        if !candidate || blocked {
            let mut done = false;
            for _ in 0..16 {
                if publish(&mut rig.link).unwrap().0 == Published::ReceiveDone(intent) {
                    done = true;
                    break;
                }
            }
            assert!(done);
        }
        let trace = rig.link.ipa_registration_trace().unwrap().tx;
        assert!(trace.request_done.finished);
        if !candidate {
            assert_eq!(trace.request_done.passes, 3);
            assert_eq!(trace.request_done.turns[0], 0x1d1);
            assert_eq!(trace.request_done.passes_at_close, Some(1));
        } else if !blocked {
            assert_eq!(trace.request_done.passes, 1);
            assert_eq!(trace.request_done.turns[0], 7);
        }
        assert_eq!(trace.request_done.failures, u32::from(candidate && blocked));
        assert!(rig.link.server_check_oacks()[0].unwrap().oack_at.is_none());
    }
}


#[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
#[test]
fn reply_window_retains_latest_committed_negative_ack() {
    let mut rig = LinuxRig::new();
    rig.link.set_registry_ack_clock(|| 1);
    let payload = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
    let intent = rig.available.remove(0);
    let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &payload);
    let Event::Request(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
    else { panic!("expected registration request"); };
    let prepared = rig.prepare_reply(&mut rig.core(), &handle);
    rig.link.queue_linux_ipa(1, &prepared, Some(&handle)).unwrap();
    let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut rig.link)
    else { panic!("expected registration reply"); };

    while rig.link.offer_receive(1).is_ok() {}
    for requested in [52, 108] {
        assert_eq!(rig.link.ingest(1, &control(7, requested)),
            Ok(Event::ReceiveIntentRequest { granted: false }));
        let previous = rig.link.intent_denial_window();
        assert_eq!(rig.link.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
        assert_eq!(rig.link.intent_denial_window(), previous);
        assert!((0..=qcom_glink_qmi::RX_SLOTS).any(|_| {
            let event = publish(&mut rig.link).unwrap().0;
            if event == (Published::IntentRequestAck { granted: false }) {
                true
            } else {
                assert!(matches!(event, Published::ReceiveIntent(_)));
                assert_eq!(rig.link.intent_denial_window(), previous);
                false
            }
        }));
        let denial = rig.link.intent_denial_window().unwrap();
        assert_eq!(denial.requested, requested);
        assert_eq!(denial.request.slots.map(|(id, _)| id),
            denial.ack.slots.map(|(id, _)| id));
        assert!(denial.request.slots[..8].iter().all(|&(_, flags)| flags >> 12 != 0));
        assert_eq!(denial.request.slots[8..], [(0, 0), (0, 0)]);
    }
    assert_eq!(rig.link.ingest(1, &control(6, remote_intent)), Ok(Event::TransmitDone));
    assert_eq!(rig.link.intent_denial_window().unwrap().requested, 108);
}

#[cfg(feature = "sunfish-tftp-first-oack-unheld")]
#[test]
fn linuxlink_first_oack_eligible_without_registration_completion() {
    use crate::qcom_glink_qmi::TftpReplyRoute;
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    for hold in [false, true] {
        let mut link = session();
        link.enable_ipa_before_first_oack();
        if !hold { link.observe_first_tftp_without_oack_hold(); }
        link.register(1, Service::IpaServer, LINUX_SERVER).unwrap();
        link.enable_linux_ipa(1, LINUX_CLIENT).unwrap();
        link.prepare_service_available(1, Service::IpaServer).unwrap();
        link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
        let mut rig = LinuxRig::from_prepared_link(link, true);
        rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));

        let peer = Address { node: 0, port: 0x1e };
        let service = Address { node: 1, port: tftp::SERVICE_PORT };
        let wrq = qrtr(1, peer.port, service.port, false, tftp::REQUEST);
        let Event::TftpRequest(handle) = rig.input(&wrq).unwrap()
        else { panic!("expected first WRQ"); };
        let mut bytes = [0; 64];
        let result = tftp::ServerCheck::new().receive(tftp::Route {
            source: tftp::Address { node: peer.node, port: peer.port },
            destination: tftp::Address { node: service.node, port: service.port },
        }, tftp::REQUEST, &mut bytes);
        let reply = result.unwrap();
        rig.link.observe_elf_server_check_dispatch(&handle,
            TftpReplyRoute { source: service, destination: peer }, true, result, &bytes);
        rig.link.finish_tftp_request(&handle).unwrap();
        rig.link.queue_traced_tftp_response(1, &handle, TftpReplyRoute {
            source: Address { node: 1, port: reply.route.source.port }, destination: peer,
        }, &bytes[..reply.len], false).unwrap();

        let registration = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
        let intent = rig.available.remove(0);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &registration);
        let Event::Request(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
        else { panic!("expected registration request"); };
        let prepared = rig.prepare_reply(&mut rig.core(), &handle);
        rig.link.queue_linux_ipa(1, &prepared, Some(&handle)).unwrap();

        let mut oack = false;
        for _ in 0..32 {
            match rig.link.publish_next(1, |_| Ok(())) {
                Ok(Some(_)) if rig.link.server_check_oacks()[0].unwrap().oack_at.is_some() => {
                    oack = true;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(Error::NoRemoteIntent) => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(oack, !hold);
        #[cfg(all(feature = "sunfish-ipa-registration-rx-first", feature = "sunfish-wlan-elf-oack-queue-trace"))]
        {
            let trace = rig.link.first_server_check().unwrap().timing.queue;
            assert!(trace.calls > 0);
            assert!(trace.first_at.is_some());
        }
        assert!(rig.link.ipa_registration_trace().unwrap().completed_at.is_none());
    }
}

#[cfg(feature = "sunfish-tftp-oack-resources")]
#[test]
fn linuxlink_oack_resources_follow_each_peer_until_data_or_close() {
    use crate::qcom_glink_qmi::TftpReplyRoute;
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static CLOCK: AtomicU64 = AtomicU64::new(1);
    let mut link = session();
    link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
    let mut rig = LinuxRig::from_prepared_link(link, false);
    rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));
    let mut server = tftp::ServerCheck::new();
    let mut response = [0; 64];

    for peer_port in [0x49, 0x58] {
        let peer = Address { node: 0, port: peer_port };
        let listener = Address { node: 1, port: tftp::SERVICE_PORT };
        let wrq = qrtr(1, peer_port, listener.port, peer_port == 0x58, tftp::REQUEST);
        rig.pump();
        let wrq_intent = rig.available.remove(0);
        let Event::TftpRequest(handle) = rig.link.ingest(1, &data(9, wrq_intent, &wrq, 0)).unwrap()
            else { panic!("expected WRQ"); };
        let dispatch = server.receive(tftp::Route {
            source: tftp::Address { node: 0, port: peer_port },
            destination: tftp::Address { node: 1, port: listener.port },
        }, tftp::REQUEST, &mut response);
        let oack = dispatch.unwrap();
        rig.link.observe_elf_server_check_dispatch(&handle,
            TftpReplyRoute { source: listener, destination: peer }, true, dispatch, &response);
        rig.link.finish_tftp_request(&handle).unwrap();
        let transfer = oack.route.source.port;
        let route = TftpReplyRoute { source: Address { node: 1, port: transfer }, destination: peer };
        let prime = cfg!(feature = "sunfish-tftp-oack-prime-rx") && peer_port == 0x49;
        let initial_intent = if prime {
            rig.pump();
            #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
            if rig.link.server_check_49().unwrap().wrq_reused {
                rig.available.push(wrq_intent);
            }
            for (i, id) in core::mem::take(&mut rig.available).into_iter().enumerate() {
                let packet = qrtr(1, 0x60 + i as u32, listener.port, false, tftp::REQUEST);
                assert!(matches!(rig.link.ingest(1, &data(9, id, &packet, 0)),
                    Ok(Event::TftpRequest(_))));
            }
            rig.link.queue_traced_tftp_response(1, &handle, route, tftp::OACK, false).unwrap();
            let mut blocked_id = None;
            assert_eq!(rig.link.publish_next(1, |record| {
                assert_eq!(word(record, 0) & 0xffff, 5);
                blocked_id = Some(word(record, 12));
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            let trace = rig.link.server_check_49().unwrap();
            assert!(trace.oack_at.is_none());
            #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
            assert!(rig.link.oack_resources(transfer).unwrap().primed_intent.is_none());
            let (Published::ReceiveIntent(id), _) = publish(&mut rig.link).unwrap()
                else { panic!("expected initial advertisement"); };
            assert_eq!(blocked_id, Some(id));
            rig.available.push(id);
            id
        } else {
            let id = rig.link.offer_receive(1).unwrap();
            rig.pump();
            assert!(rig.available.contains(&id));
            rig.link.queue_traced_tftp_response(1, &handle, route, tftp::OACK, false).unwrap();
            id
        };
        let (Published::Data { remote_intent, .. }, frame) = publish_until_data(&mut rig.link)
            else { panic!("expected OACK"); };
        assert_eq!(word(&frame, 44), peer_port);
        rig.link.ingest(1, &control(14, remote_intent)).unwrap();

        let credit = qrtr_control(7, [0, peer_port, 0, 0]);
        let slot = rig.available.iter().position(|&id| id == initial_intent).unwrap();
        rig.available.remove(slot);
        assert_eq!(rig.link.ingest(1, &data(9, initial_intent, &credit, 0)), Ok(Event::CreditReceived));

        let intent = rig.link.offer_receive(1).unwrap();
        rig.pump();
        let slot = rig.available.iter().position(|&id| id == intent).unwrap();
        rig.available.remove(slot);
        if peer_port == 0x49 {
            let close = qrtr_control(6, [0, peer_port, 0, 0]);
            assert!(matches!(rig.link.ingest(1, &data(9, intent, &close, 0)),
                Ok(Event::QrtrControl(_))));
            let trace = rig.link.server_check_49().unwrap();
            let resources = rig.link.oack_resources(trace.transfer_port).unwrap();
            assert!(!trace.confirm_requested && !resources.oack_confirm);
            assert_eq!(resources.flow_pending, Some(1));
            assert_eq!((resources.peer_resumes, resources.applied_resumes), (1, 1));
            assert!(trace.oack_at < resources.peer_resume_at);
            assert!(resources.peer_resume_at < trace.first_close_at);
            let initial = resources.initial.iter().flatten()
                .find(|entry| entry.id == initial_intent).unwrap();
            assert!(trace.oack_at < initial.consumed_at);
            assert!(initial.consumed_at < resources.peer_resume_at);
            assert!(resources.initial.iter().flatten().all(|entry| entry.id != intent));
            let observed = resources.added.iter().flatten()
                .find(|entry| entry.id == intent).unwrap();
            assert!(trace.oack_at < observed.available_at && observed.available_at < observed.consumed_at);
            assert!(observed.consumed_at < trace.first_close_at);
            assert!(resources.data_intent.is_none());
            assert_eq!(resources.close_intent, Some(intent));
            #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
            assert_eq!(resources.primed_intent, Some(initial_intent));
        } else {
            let payload = [0, 3, 0, 1, 42];
            let packet = qrtr(1, peer_port, transfer, false, &payload);
            let Event::TftpRequest(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
                else { panic!("expected DATA"); };
            let dispatch = server.receive(tftp::Route {
                source: tftp::Address { node: 0, port: peer_port },
                destination: tftp::Address { node: 1, port: transfer },
            }, &payload, &mut response);
            assert!(dispatch.unwrap().completed);
            rig.link.observe_elf_server_check_dispatch(&handle, route, false, dispatch, &response);
            let trace = rig.link.server_check_oacks()[1].unwrap();
            let resources = rig.link.oack_resources(trace.transfer_port).unwrap();
            assert_eq!(trace.peer.port, 0x58);
            assert!(trace.confirm_requested);
            assert!(trace.resume_at.is_some());
            assert_eq!(resources.data_intent, Some(intent));
            assert!(trace.first_close_at.is_none());
        }
    }
}

#[cfg(feature = "sunfish-tftp-oack-prime-rx")]
#[test]
fn linuxlink_oack_prime_handles_pending_grant_and_full_receive_pool() {
    use crate::qcom_glink_qmi::{RX_SLOTS, TftpReplyRoute};
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;

    for (full, intervening_grant, replenish, _blocked_followup) in [
        (false, false, false, 0), (true, false, false, 0), (true, true, false, 0),
        (true, true, true, 0),
        #[cfg(feature = "sunfish-tftp-oack-followup")]
        (true, true, false, 1),
        #[cfg(feature = "sunfish-tftp-oack-followup")]
        (true, true, false, 2),
        #[cfg(feature = "sunfish-tftp-oack-followup")]
        (true, true, false, 3),
        #[cfg(feature = "sunfish-tftp-oack-ordinary-ad")]
        (true, true, false, 4),
        #[cfg(feature = "sunfish-tftp-oack-ordinary-ad")]
        (true, true, false, 5),
        #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
        (true, true, false, 6),
    ] {
        let mut followup_frame = None;
        let mut link = session();
        link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
        let mut rig = LinuxRig::from_prepared_link(link, false);
        rig.link.set_registry_ack_clock(|| 1);
        let peer = Address { node: 0, port: 0x49 };
        let listener = Address { node: 1, port: tftp::SERVICE_PORT };
        let wrq = qrtr(1, peer.port, listener.port, false, tftp::REQUEST);
        let wrq_intent = rig.available.remove(0);
        let Event::TftpRequest(handle) = rig.link.ingest(1, &data(9, wrq_intent, &wrq, 0)).unwrap()
            else { panic!("expected WRQ"); };
        let mut server = tftp::ServerCheck::new();
        let mut response = [0; 64];
        let dispatch = server.receive(tftp::Route {
            source: tftp::Address { node: 0, port: peer.port },
            destination: tftp::Address { node: 1, port: listener.port },
        }, tftp::REQUEST, &mut response);
        let transfer = dispatch.unwrap().route.source.port;
        rig.link.observe_elf_server_check_dispatch(&handle,
            TftpReplyRoute { source: listener, destination: peer }, true, dispatch, &response);
        rig.link.finish_tftp_request(&handle).unwrap();
        rig.pump();
        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
        if rig.link.server_check_49().unwrap().wrq_reused {
            rig.available.push(wrq_intent);
        }
        if !full {
            assert_eq!(rig.link.ingest(1, &control(7, 4097)),
                Ok(Event::ReceiveIntentRequest { granted: false }));
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: false });
        }

        let mut held = Vec::new();
        loop {
            for id in core::mem::take(&mut rig.available) {
                let packet = qrtr(1, 0x60 + held.len() as u32, listener.port, false, tftp::REQUEST);
                let Event::TftpRequest(request) = rig.link.ingest(1, &data(9, id, &packet, 0)).unwrap()
                    else { panic!("expected competing WRQ"); };
                held.push((id, request));
            }
            if !full || held.len() == RX_SLOTS {
                break;
            }
            rig.link.offer_receive(1).unwrap();
            rig.pump();
        }

        let route = TftpReplyRoute { source: Address { node: 1, port: transfer }, destination: peer };
        rig.link.queue_traced_tftp_response(1, &handle, route, tftp::OACK, false).unwrap();
        if full {
            let (id, releasable) = held[0];
            rig.link.finish_tftp_request(&releasable).unwrap();
            assert_eq!(rig.link.publish_next(1, |record| {
                assert_eq!(word(record, 0) & 0xffff, 6);
                assert_eq!(word(record, 4), id);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            assert!(rig.link.oack_resources(transfer).unwrap().prime_released.is_none());
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(id));
            assert_eq!(rig.link.oack_resources(transfer).unwrap().prime_released, Some(id));
        }
        if !full || intervening_grant {
            assert_eq!(rig.link.ingest(1, &control(7, 108)),
                Ok(Event::ReceiveIntentRequest { granted: true }));
        }
        let (Published::ReceiveIntent(id), _) = publish(&mut rig.link).unwrap()
            else { panic!("expected advertisement before OACK"); };
        let primed = if intervening_grant { None } else { Some(id) };
        assert_eq!(rig.link.oack_resources(transfer).unwrap().primed_intent, primed);
        if !full || intervening_grant {
            assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
        }
        let mut ready = Some(id);
        if intervening_grant {
            let packet = qrtr(1, peer.port, listener.port, false, tftp::REQUEST);
            #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
            {
                let trace = rig.link.oack_slot_follow().unwrap();
                assert_eq!(trace.released, held[0].0);
                assert_eq!(trace.next_intent, Some(id));
                assert_eq!(trace.availability_kind, 2);
            }
            let split = 16;
            assert_eq!(rig.link.ingest(1, &data(9, id, &packet[..split], packet.len() - split)),
                Ok(Event::Fragment));
            #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
            {
                let partial = rig.link.oack_slot_follow().unwrap();
                assert!(partial.started && partial.packet.is_none());
            }
            let Event::TftpRequest(repeat) = rig.link.ingest(1, &data(12, id, &packet[split..], 0)).unwrap()
                else { panic!("expected repeat WRQ"); };
            #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
            {
                let captured = rig.link.oack_slot_follow().unwrap().packet.unwrap();
                assert_eq!(captured.source, Some(peer));
                assert_eq!(captured.destination, Some(listener));
                assert_eq!((captured.version, captured.kind, captured.opcode), (1, Some(1), Some(2)));
                assert_eq!(captured.bytes as usize, packet.len());
            }
            let retry = server.receive(tftp::Route {
                source: tftp::Address { node: 0, port: peer.port },
                destination: tftp::Address { node: 1, port: listener.port },
            }, tftp::REQUEST, &mut response).unwrap();
            assert_eq!(retry.route.source.port, transfer);
            assert_eq!(&response[..retry.len], tftp::OACK);
            #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
            assert!(rig.link.coalesce_followed_wrq(&repeat));
            rig.link.finish_tftp_request(&repeat).unwrap();
            assert_eq!(rig.link.queue_traced_tftp_response(1, &repeat, route, tftp::OACK, false),
                Err(Error::FlowBusy));
            ready = None;
            #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
            if !replenish {
                assert_eq!(rig.link.publish_next(1, |record| {
                    assert_eq!(word(record, 0) & 0xffff, 6);
                    assert_eq!(word(record, 4), id);
                    Err(WriteError::WouldBlock)
                }), Err(Error::WouldBlock));
                assert!(rig.link.oack_slot_follow().unwrap().exact_repeat_done_at.is_none());
                assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(id));
                assert!(rig.link.oack_slot_follow().unwrap().exact_repeat_done_at.is_some());
                #[cfg(feature = "sunfish-tftp-oack-followup")]
                let replenishment = {
                    use crate::qcom_glink_qmi::OackFollowup;
                    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                    if matches!(_blocked_followup, 3 | 6) {
                        assert_eq!(rig.link.ingest(1, &control(7, 108)),
                            Ok(Event::ReceiveIntentRequest { granted: true }));
                        assert!(rig.link.oack_followup_grant_pending());
                        assert_eq!(rig.link.publish_oack_followup(1, OackFollowup::Grant, |_| {
                            panic!("ACK before the backing advertisement");
                        }), Ok(None));
                        let ad = rig.link.publish_oack_followup(1, OackFollowup::Advertise, |record| {
                            assert_eq!(word(record, 0) & 0xffff, 5);
                            Ok(())
                        }).unwrap();
                        let Some(Published::ReceiveIntent(id)) = ad
                            else { panic!("expected backing advertisement"); };
                        let trace = rig.link.oack_slot_follow().unwrap();
                        assert_eq!(trace.followup_selected, Some(id));
                        let guard = trace.advertisement_guard.unwrap();
                        assert!(guard.grant_pending && guard.allowed);
                        assert!(rig.link.oack_followup_grant_pending());
                        let ack = rig.link.publish_oack_followup(1, OackFollowup::Grant, |record| {
                            assert_eq!(word(record, 0) & 0xffff, 8);
                            assert_eq!(word(record, 4), 1);
                            if _blocked_followup == 6 { Err(WriteError::WouldBlock) } else { Ok(()) }
                        });
                        if _blocked_followup == 6 {
                            assert_eq!(ack, Err(Error::WouldBlock));
                            let trace = rig.link.oack_slot_follow().unwrap();
                            assert_eq!((trace.followup_ad, trace.followup_grant, trace.followup_reply), (1, 2, 0));
                            assert!(rig.link.oack_followup_grant_pending());
                            assert!(rig.link.server_check_49().unwrap().oack_at.is_none());
                            assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
                        } else {
                            assert_eq!(ack, Ok(Some(Published::IntentRequestAck { granted: true })));
                        }
                        assert!(!rig.link.oack_followup_grant_pending());
                        let mut frame = Vec::new();
                        let reply = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                            frame.extend_from_slice(record);
                            Ok(())
                        }).unwrap();
                        assert!(matches!(reply, Some(Published::Data { last_fragment: true, .. })));
                        assert_eq!(word(&frame, 44), peer.port);
                        let resources = rig.link.oack_resources(transfer).unwrap();
                        assert!(resources.initial.iter().flatten().any(|entry| entry.id == id));
                        continue;
                    }
                    if _blocked_followup == 3 {
                        assert_eq!(rig.link.ingest(1, &control(7, 108)),
                            Ok(Event::ReceiveIntentRequest { granted: true }));
                    }
                    #[cfg(feature = "sunfish-tftp-oack-ordinary-ad")]
                    if matches!(_blocked_followup, 4 | 5) {
                        rig.link.set_service_available(1, Service::RemoteStorage, true).unwrap();
                        // Start from the mapped flags in the phone's 03/00 trace.
                        rig.link.replay_clear_tftp_writer_turns();
                        let mut id = None;
                        let ad = rig.link.publish_oack_followup(1, OackFollowup::Advertise, |record| {
                            assert_eq!(word(record, 0) & 0xffff, 5);
                            id = Some(word(record, 12));
                            if _blocked_followup == 5 { Err(WriteError::WouldBlock) } else { Ok(()) }
                        });
                        let trace = rig.link.oack_slot_follow().unwrap();
                        assert_eq!(trace.followup_gates, 0x1800);
                        assert_eq!(trace.followup_choice, 1);
                        assert_eq!(trace.followup_candidate, None);
                        assert_eq!(trace.followup_selected, id);
                        #[cfg(feature = "sunfish-tftp-oack-guard")]
                        {
                            let guard = trace.advertisement_guard.unwrap();
                            let record = guard.first_record.unwrap();
                            assert_eq!((record.kind, record.len, record.sent), (6, 72, 0));
                            assert_eq!((guard.matching, guard.unsent, guard.ready_unsent), (1, 1, 1));
                            assert!(guard.oack_at.is_none());
                            assert!(guard.allowed && !guard.grant_pending && !guard.peer_closed);
                        }
                        if _blocked_followup == 5 {
                            assert_eq!(ad, Err(Error::WouldBlock));
                            assert_eq!((trace.followup_ad, trace.followup_reply), (2, 0));
                            assert!(rig.link.server_check_49().unwrap().oack_at.is_none());
                            assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveIntent(id.unwrap()));
                        } else {
                            assert_eq!(ad, Ok(Some(Published::ReceiveIntent(id.unwrap()))));
                        }

                        let mut frame = Vec::new();
                        if _blocked_followup == 5 {
                            frame = publish_until_data(&mut rig.link).1;
                        } else {
                            let reply = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                                frame.extend_from_slice(record);
                                Ok(())
                            }).unwrap();
                            assert!(matches!(reply, Some(Published::Data { last_fragment: true, .. })));
                            let trace = rig.link.oack_slot_follow().unwrap();
                            assert_eq!((trace.followup_ad, trace.followup_reply), (1, 1));
                            assert_eq!(trace.followup_selected, Some(peer.port));
                            #[cfg(feature = "sunfish-tftp-oack-guard")]
                            {
                                let guard = trace.advertisement_guard.unwrap();
                                assert!(guard.oack_at.is_none());
                                assert_eq!(guard.first_record.unwrap().sent, 0);
                                assert!(rig.link.server_check_49().unwrap().oack_at.is_some());
                            }
                        }
                        assert_eq!(word(&frame, 44), peer.port);
                        let resources = rig.link.oack_resources(transfer).unwrap();
                        assert!(resources.initial.iter().flatten().any(|entry| Some(entry.id) == id));
                        continue;
                    }
                    let ad = rig.link.publish_oack_followup(1, OackFollowup::Advertise, |record| {
                        assert_ne!(_blocked_followup, 3, "protected grant stays on the normal path");
                        assert_eq!(word(record, 0) & 0xffff, 5);
                        if _blocked_followup == 1 { Err(WriteError::WouldBlock) } else { Ok(()) }
                    });
                    if _blocked_followup == 3 {
                        assert_eq!(ad, Ok(None));
                        assert_eq!(rig.link.oack_slot_follow().unwrap().followup_ad, 3);
                        assert_eq!(rig.link.oack_slot_follow().unwrap().followup_reply, 0);
                        let ad = publish(&mut rig.link).unwrap().0;
                        #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
                        {
                            let Published::ReceiveIntent(grant_id) = ad
                                else { panic!("expected the protected grant's advertisement"); };
                            let trace = rig.link.oack_slot_follow().unwrap();
                            assert_eq!(trace.followup_choice, 1);
                            assert_eq!(trace.followup_selected, Some(grant_id));
                            #[cfg(feature = "sunfish-tftp-oack-guard")]
                            {
                                let guard = trace.advertisement_guard.unwrap();
                                assert!(guard.grant_pending && !guard.allowed);
                                assert_eq!(guard.ready_unsent, 1);
                            }
                            assert_eq!(trace.followup_gates & 0xd, 0xd); // Preferred positive grant with backing.
                            assert_eq!(trace.followup_gates & (1 << 9), 0); // No priming target.
                            assert_eq!(trace.followup_gates & 0x1800, 0x1800); // Target queued and ready.
                            assert_eq!((trace.followup_slots >> (4 * trace.slot)) & 0xf, 1); // Advertising.
                        }
                        assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
                        ad
                    } else if _blocked_followup == 1 {
                        assert_eq!(ad, Err(Error::WouldBlock));
                        assert_eq!(rig.link.oack_slot_follow().unwrap().followup_ad, 2);
                        assert_eq!(rig.link.oack_slot_follow().unwrap().followup_reply, 0);
                        publish(&mut rig.link).unwrap().0
                    } else {
                        let mut bytes = Vec::new();
                        let reply = rig.link.publish_oack_followup(1, OackFollowup::Reply, |record| {
                            assert_eq!(word(record, 44), peer.port);
                            bytes.extend_from_slice(record);
                            if _blocked_followup == 2 { Err(WriteError::WouldBlock) } else { Ok(()) }
                        });
                        assert_eq!(rig.link.oack_slot_follow().unwrap().followup_ad, 1);
                        if _blocked_followup == 2 {
                            assert_eq!(reply, Err(Error::WouldBlock));
                            assert_eq!(rig.link.oack_slot_follow().unwrap().followup_reply, 2);
                            assert!(rig.link.server_check_49().unwrap().oack_at.is_none());
                        } else {
                            let published = reply.unwrap().unwrap();
                            assert!(matches!(published, Published::Data { last_fragment: true, .. }));
                            assert_eq!(rig.link.oack_slot_follow().unwrap().followup_reply, 1);
                            followup_frame = Some((published, bytes));
                        }
                        ad.unwrap().unwrap()
                    }
                };
                #[cfg(not(feature = "sunfish-tftp-oack-followup"))]
                let replenishment = publish(&mut rig.link).unwrap().0;
                let Published::ReceiveIntent(new) = replenishment
                    else { panic!("expected replenishment before OACK"); };
                assert_eq!(rig.link.oack_resources(transfer).unwrap().primed_intent,
                    (_blocked_followup != 3).then_some(new));
                ready = Some(new);
            }
            #[cfg(feature = "sunfish-tftp-first-oack-unheld")]
            if replenish {
                // Hold the OACK in this replay to exercise the existing
                // release, advertise, ACK sequence before allowing data.
                rig.link.enable_ipa_before_first_oack();
                assert_eq!(rig.link.ingest(1, &control(7, 108)),
                    Ok(Event::ReceiveIntentRequest { granted: false }));
                assert_eq!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(id));
                let (Published::ReceiveIntent(new), _) = publish(&mut rig.link).unwrap()
                    else { panic!("expected grant advertisement"); };
                assert_eq!(publish(&mut rig.link).unwrap().0, Published::IntentRequestAck { granted: true });
                ready = Some(new);
                rig.link.observe_first_tftp_without_oack_hold();
            }
        }
        let (_, frame) = if let Some(result) = followup_frame {
            result
        } else if intervening_grant && ready.is_none() {
            let (event, frame) = publish(&mut rig.link).unwrap();
            assert!(matches!(event, Published::Data { .. }), "OACK bypasses the repeat RX_DONE");
            (event, frame)
        } else {
            publish_until_data(&mut rig.link)
        };
        assert_eq!(word(&frame, 44), peer.port);
        let resources = rig.link.oack_resources(transfer).unwrap();
        if let Some(ready) = ready {
            assert!(resources.initial.iter().flatten().any(|intent| intent.id == ready));
        } else {
            assert_eq!(rig.link.server_check_49().unwrap().ready_at_oack, Some(0));
        }
    }
}

#[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
#[test]
fn first_oack_borrows_one_fitting_intent_after_exact_ipa_publication() {
    use crate::qcom_glink_qmi::TftpReplyRoute;
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(1);
    for (scarce, blocked, close, close_before_pass) in [
        (false, false, false, false),
        (false, false, true, false),
        (false, true, false, false),
        (false, true, true, false),
        (true, false, false, false),
        (true, false, true, false),
        (false, false, true, true),
    ] {
        let mut link = session();
        link.enable_ipa_before_first_oack();
        link.enable_first_oack_after_ipa_publish();
        link.register(1, Service::IpaServer, LINUX_SERVER).unwrap();
        link.enable_linux_ipa(1, LINUX_CLIENT).unwrap();
        link.prepare_service_available(1, Service::IpaServer)
            .unwrap();
        link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
        let mut rig =
            LinuxRig::from_prepared_link_with_intents(link, true, if scarce { 11 } else { 39 });
        rig.link
            .set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));

        let peer = Address {
            node: 0,
            port: 0x1f,
        };
        let service = Address {
            node: 1,
            port: tftp::SERVICE_PORT,
        };
        let wrq = qrtr(1, peer.port, service.port, false, tftp::REQUEST);
        let Event::TftpRequest(handle) = rig.input(&wrq).unwrap() else {
            panic!("expected first WRQ");
        };
        let mut bytes = [0; 64];
        let result = tftp::ServerCheck::new().receive(
            tftp::Route {
                source: tftp::Address {
                    node: peer.node,
                    port: peer.port,
                },
                destination: tftp::Address {
                    node: service.node,
                    port: service.port,
                },
            },
            tftp::REQUEST,
            &mut bytes,
        );
        let reply = result.unwrap();
        rig.link.observe_elf_server_check_dispatch(
            &handle,
            TftpReplyRoute {
                source: service,
                destination: peer,
            },
            true,
            result,
            &bytes,
        );
        rig.link.finish_tftp_request(&handle).unwrap();
        rig.link
            .queue_traced_tftp_response(
                1,
                &handle,
                TftpReplyRoute {
                    source: Address {
                        node: 1,
                        port: reply.route.source.port,
                    },
                    destination: peer,
                },
                &bytes[..reply.len],
                false,
            )
            .unwrap();

        let close_intent = rig.link.offer_receive(1).unwrap();
        if close_before_pass {
            assert!((0..16).any(|_| publish(&mut rig.link).unwrap().0
                == Published::ReceiveIntent(close_intent)));
        }
        let registration = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
        let intent = rig.available.remove(0);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &registration);
        let Event::Request(handle) = rig.link.ingest(1, &data(9, intent, &packet, 0)).unwrap()
        else {
            panic!("expected registration request");
        };
        let prepared = rig.prepare_reply(&mut rig.core(), &handle);
        rig.link
            .queue_linux_ipa(1, &prepared, Some(&handle))
            .unwrap();

        let (
            Published::Data {
                remote_intent: ipa_id,
                last_fragment: true,
            },
            ipa_frame,
        ) = publish_until_data(&mut rig.link)
        else {
            panic!("missing IPA reply")
        };
        assert_eq!(
            &ipa_frame[48..48 + prepared.bytes().len()],
            prepared.bytes()
        );
        if close_before_pass {
            // IPA was published by the preceding turn. Closing immediately
            // distinguishes a missing writer opportunity from a missing intent.
            rig.link.ingest(1, &data(9, close_intent,
                &qrtr_control(6, [0, peer.port, 0, 0]), 0)).unwrap();
            let gates = rig.link.first_oack_eligibility();
            assert!(gates.passes > 0 && gates.queued_at.is_some());
            assert_eq!((gates.ipa_passes, gates.ready_passes, gates.close_mask), (0, 0, Some(0)));
            rig.link.ingest(1, &control(14, ipa_id)).unwrap();
            let _ = publish(&mut rig.link);
            assert_eq!(rig.link.first_oack_eligibility(), gates);
            continue;
        }
        if blocked {
            assert_eq!(
                rig.link.ingest(1, &control(7, 128)),
                Ok(Event::ReceiveIntentRequest { granted: true })
            );
            let (advertisement, advertised) = publish(&mut rig.link).unwrap();
            assert!(matches!(advertisement, Published::ReceiveIntent(_)));
            assert!(word(&advertised, 8) >= 128);
            assert_eq!(
                publish(&mut rig.link).unwrap().0,
                Published::IntentRequestAck { granted: true }
            );
            if !close {
                let confirmed = qrtr(1, 0x22, service.port, true, tftp::REQUEST);
                let Event::TftpRequest(other) = rig
                    .link
                    .ingest(1, &data(9, word(&advertised, 12), &confirmed, 0))
                    .unwrap()
                else {
                    panic!("expected independent confirmed WRQ")
                };
                rig.link.finish_tftp_request(&other).unwrap();
                let (_, credit) = publish_until_data(&mut rig.link);
                assert_eq!(word(&credit, 20), 7, "QRTR RESUME must keep its turn");
            }
        }
        let mut attempts = 0;
        let mut published_oack = None;
        for _ in 0..32 {
            let result = rig.link.publish_next(1, |frame| {
                assert_ne!(
                    word(frame, 0) & 0xffff,
                    7,
                    "held OACK requested a new peer intent"
                );
                if word(frame, 0) & 0xffff == 9 {
                    attempts += 1;
                    assert_ne!(word(frame, 4), ipa_id);
                    assert_eq!(word(frame, 28), reply.route.source.port);
                    assert_eq!(word(frame, 44), peer.port);
                    assert_eq!(&frame[48..48 + reply.len], &bytes[..reply.len]);
                    if blocked {
                        return Err(WriteError::WouldBlock);
                    }
                    published_oack = Some(word(frame, 4));
                }
                Ok(())
            });
            match result {
                Ok(Some(_)) | Err(Error::WouldBlock) => {}
                Ok(None) | Err(Error::NoRemoteIntent) => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(attempts, usize::from(!scarce));
        assert!(
            rig.link
                .ipa_registration_trace()
                .unwrap()
                .completed_at
                .is_none()
        );
        let gates = rig.link.first_oack_eligibility();
        assert!(gates.queued_at.is_some() && gates.ipa_passes > 0);
        assert_eq!(gates.ready_passes > 0, !scarce);
        assert_eq!(gates.common & 0x40 != 0, scarce);
        if blocked {
            assert_ne!(gates.seen & 0x04, 0, "pending grant recorded");
            if !close {
                assert_ne!(gates.seen & 0x08, 0, "queued RESUME recorded");
            }
        }
        if close {
            rig.link
                .ingest(
                    1,
                    &data(9, close_intent, &qrtr_control(6, [0, peer.port, 0, 0]), 0),
                )
                .unwrap();
        }
        let at_close = rig.link.first_oack_eligibility();
        assert_eq!(at_close.close_mask.is_some(), close);
        if close && scarce {
            assert_ne!(at_close.close_mask.unwrap() & 0x40, 0);
        }
        if let Some(id) = published_oack {
            assert!(
                rig.link.server_check_oacks()[0]
                    .unwrap()
                    .oack_done_at
                    .is_none()
            );
            rig.link.ingest(1, &control(6, id)).unwrap();
            assert!(
                rig.link.server_check_oacks()[0]
                    .unwrap()
                    .oack_done_at
                    .is_some()
            );
            assert!(
                rig.link
                    .ipa_registration_trace()
                    .unwrap()
                    .completed_at
                    .is_none()
            );
        }
        rig.link.ingest(1, &control(14, ipa_id)).unwrap();
        let mut fallback = 0;
        for _ in 0..32 {
            match rig.link.publish_next(1, |frame| {
                if word(frame, 0) & 0xffff == 9 {
                    assert_eq!(&frame[48..48 + reply.len], &bytes[..reply.len]);
                    fallback += 1;
                }
                Ok(())
            }) {
                Ok(Some(_)) => {}
                Ok(None) | Err(Error::NoRemoteIntent) => break,
                other => panic!("{other:?}"),
            }
        }
        if close {
            assert_eq!(rig.link.first_oack_eligibility(), at_close);
        }
        assert_eq!(fallback, usize::from((scarce || blocked) && !close));
    }
}

#[cfg(all(feature = "sunfish-ipa-registration-rxdone-turn", feature = "sunfish-qrtr-ipa-flow-experiment"))]
#[test]
fn linuxlink_prompt_reply_completion_preserves_blocked_registration_buffer_return() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let wire = rig.available.remove(0);
    let registration = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
    let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &registration);
    let handle = receive(&mut rig.link, wire, &packet);
    let reply = rig.prepare_reply(&mut core, &handle);
    rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();

    loop {
        let result = rig.link.publish_next(1, |frame| {
            if frame[0] == 6 {
                assert_eq!(word(frame, 4), wire);
                return Err(WriteError::WouldBlock);
            }
            assert_eq!(frame[0], 5);
            Ok(())
        });
        if result == Err(Error::WouldBlock) {
            break;
        }
        assert!(matches!(result, Ok(Some(Published::ReceiveIntent(_)))));
    }

    let mut returns = Vec::new();
    let mut completed = false;
    while let Some((event, frame)) = publish(&mut rig.link) {
        match event {
            Published::Data { remote_intent, .. } => {
                assert!(!completed);
                assert_eq!(&frame[48..48 + reply.bytes().len()], reply.bytes());
                rig.link.ingest(1, &control(14, remote_intent)).unwrap();
                let receipt = rig.link.take_linux_ipa_receipt(1).unwrap().unwrap();
                rig.link.commit_linux_ipa(1, &mut core, &reply, receipt).unwrap();
                completed = true;
            }
            Published::ReceiveDone(id) => returns.push(id),
            Published::ReceiveIntent(_) => {}
            other => panic!("unexpected publication {other:?}"),
        }
    }
    assert!(completed);
    assert_eq!(rig.link.in_flight(), 0);
    assert_eq!(returns, [wire], "the peer is still owed its buffer return");
}

#[cfg(feature = "sunfish-ipa-announcement-timeline")]
#[test]
fn ipa_announcement_latches_the_first_accepted_modem_endpoint_per_generation() {
    for timed in [false, true] {
        let mut rig = LinuxRig::with_services(true, false);
        if timed {
            rig.link.set_registry_ack_clock(|| 101);
        }
        rig.input(&qrtr_control(4, [0x31, 1, 0, LINUX_MODEM + 1])).unwrap();
        assert_eq!(rig.link.first_ipa_announcement(), None);
        let announce = qrtr_control(4, [0x31, 0x201, 0, LINUX_MODEM]);
        rig.input(&announce).unwrap();
        let expected = Some((LINUX_MODEM, timed.then_some(101)));
        assert_eq!(rig.link.first_ipa_announcement(), expected);
        rig.link.set_registry_ack_clock(|| 202);
        rig.input(&announce).unwrap();
        assert_eq!(rig.link.first_ipa_announcement(), expected);
        rig.link.reset_after_transport_reset(2).unwrap();
        assert_eq!(rig.link.first_ipa_announcement(), None);
    }
}

#[cfg(feature = "sunfish-ipa-announcement-timeline")]
#[test]
fn accepted_ipa_announcement_can_precede_the_local_hello() {
    let mut link = session();
    link.start_control(1).unwrap();
    link.set_registry_ack_clock(|| 73);
    let mut ids = Vec::new();
    for _ in 0..8 {
        if let Some((Published::ReceiveIntent(id), _)) = publish(&mut link) {
            ids.push(id);
        }
        if ids.len() == 2 { break; }
    }
    assert_eq!(ids.len(), 2);
    link.ingest(1, &data(9, ids[0], &qrtr_control(2, [0; 4]), 0)).unwrap();
    link.ingest(1, &data(9, ids[1], &qrtr_control(4, [0x31, 0x201, 0, LINUX_MODEM]), 0)).unwrap();
    assert!(!link.control_status().hello_sent);
    assert_eq!(link.first_ipa_announcement(), Some((LINUX_MODEM, Some(73))));
}

#[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
#[test]
fn endpoint_ready_requires_both_remote_discovery_and_local_publication() {
    let mut rig = LinuxRig::with_services(false, false);
    rig.link.set_registry_ack_clock(|| 101);
    let modem = qrtr_control(4,[0x31,0x201,0,LINUX_MODEM]);
    rig.input(&modem).unwrap();
    assert_eq!(rig.link.ipa_endpoint_ready_at(),None);
    rig.link.set_service_available(1,Service::IpaServer,true).unwrap();
    assert_eq!(rig.link.ipa_endpoint_ready_at(),None);
    rig.link.set_registry_ack_clock(|| 202);
    rig.pump();
    assert_eq!(rig.link.ipa_endpoint_ready_at(),Some(202));
    rig.link.set_registry_ack_clock(|| 303);
    rig.input(&modem).unwrap();
    assert_eq!(rig.link.ipa_endpoint_ready_at(),Some(202));
    rig.link.reset_after_transport_reset(2).unwrap();
    assert_eq!(rig.link.ipa_endpoint_ready_at(),None);
}

#[cfg(feature = "sunfish-ipa-init-open-writer")]
#[test]
fn replay_open_init_with_grant_pending_at_registration_publication() {
    use crate::qcom_glink_qmi::TftpReplyRoute;
    use crate::qcom_qmi_router::Address;
    use crate::qcom_tftp_server_check as tftp;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(1);
    let candidate = cfg!(feature = "sunfish-ipa-init-before-ad");
    for case in ["one_open_turn", "writer_blocked", "closed_first", "credit_arrives", "already_advertised", "uncertain"] {
        let mut link = session();
        link.enable_ipa_before_first_oack();
        link.register(1, Service::IpaServer, LINUX_SERVER).unwrap();
        link.enable_linux_ipa(1, LINUX_CLIENT).unwrap();
        link.prepare_service_available(1, Service::IpaServer).unwrap();
        link.prepare_tftp_server(1, tftp::SERVICE_PORT).unwrap();
        let mut rig = LinuxRig::from_prepared_link(link, true);
        rig.link.set_registry_ack_clock(|| CLOCK.fetch_add(1, Relaxed));
        while rig.available.len() < 4 {
            rig.link.offer_receive(1).unwrap();
            rig.pump();
        }
        let peer = Address { node: 0, port: 0x1f };
        let service = Address { node: 1, port: tftp::SERVICE_PORT };
        let wrq = qrtr(1, peer.port, service.port, false, tftp::REQUEST);
        let Event::TftpRequest(handle) = rig.input(&wrq).unwrap() else { panic!("expected WRQ"); };
        let mut bytes = [0; 64];
        let result = tftp::ServerCheck::new().receive(tftp::Route {
            source: tftp::Address { node: peer.node, port: peer.port },
            destination: tftp::Address { node: service.node, port: service.port },
        }, tftp::REQUEST, &mut bytes);
        let reply = result.unwrap();
        rig.link.observe_elf_server_check_dispatch(&handle,
            TftpReplyRoute { source: service, destination: peer }, true, result, &bytes);
        rig.link.finish_tftp_request(&handle).unwrap();
        rig.link.queue_traced_tftp_response(1, &handle, TftpReplyRoute {
            source: Address { node: 1, port: reply.route.source.port }, destination: peer,
        }, &bytes[..reply.len], false).unwrap();

        let request = ipa_server_request(1, 0x20, &[(0x10, &[1])]);
        let packet = qrtr(1, LINUX_PEER_CLIENT, LINUX_SERVER, false, &request);
        let Event::Request(handle) = rig.link.ingest(1,
            &data(9, rig.available.remove(0), &packet, 0)).unwrap() else { panic!("expected IPA"); };
        let mut core = rig.core();
        let reply = rig.prepare_reply(&mut core, &handle);
        rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();
        for _ in 0..2 {
            assert!(matches!(publish(&mut rig.link).unwrap().0, Published::ReceiveDone(_)));
        }
        assert_eq!(rig.link.ingest(1, &control(7, 52)), Ok(Event::ReceiveIntentRequest { granted: true }));
        let (registration, frame) = publish(&mut rig.link).unwrap();
        assert_eq!(&frame[48..48 + reply.bytes().len()], reply.bytes());
        let Published::Data { remote_intent: reg_id, .. } = registration else { panic!() };

        let mut records = Vec::new();
        if case == "already_advertised" {
            let ad = publish(&mut rig.link).unwrap();
            assert!(matches!(ad.0, Published::ReceiveIntent(_)));
            records.push(ad);
        }
        let init = core.prepare_init_driver(1, 0, 0x415).unwrap();
        rig.link.queue_linux_ipa(1, &init, None).unwrap();
        let at_queue = rig.link.ipa_init_transport_trace().unwrap().tx.queue_state;
        assert_eq!(at_queue, if case == "already_advertised" { 0x87 } else { 0xc7 });
        let close = data(9, rig.available.remove(0), &qrtr_control(6, [0,peer.port,0,0]), 0);
        if case == "credit_arrives" {
            let confirmed = qrtr(1, 0x51, service.port, true, tftp::REQUEST);
            assert!(matches!(rig.link.ingest(1, &data(9, rig.available.remove(0), &confirmed, 0)),
                Ok(Event::TftpRequest(_))));
        }
        if case == "uncertain" {
            assert_eq!(rig.link.publish_next(1, |frame| {
                if candidate { assert_eq!(&frame[48..48 + init.bytes().len()], init.bytes()); }
                else { assert_eq!(frame[0], 5); }
                Err(WriteError::Uncertain)
            }), Err(Error::PublicationUncertain));
            assert!(rig.link.ipa_init_transport_trace().unwrap().published_at.is_none());
            assert!(rig.link.publish_next(1, |_| panic!("uncertain writer must stay quarantined")).is_err());
            continue;
        }
        if case == "closed_first" {
            rig.link.ingest(1, &close).unwrap();
            assert_eq!(rig.link.original_1f_close_count(), Some(1));
        }
        if case == "writer_blocked" {
            assert_eq!(rig.link.publish_next(1, |frame| {
                if candidate { assert_eq!(&frame[48..48 + init.bytes().len()], init.bytes()); }
                else { assert_eq!(frame[0], 5); }
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            assert!(rig.link.ipa_init_transport_trace().unwrap().published_at.is_none());
        }
        let first = publish(&mut rig.link).unwrap();
        let borrowed = candidate && case == "one_open_turn";
        assert_eq!(first.1[0], if borrowed { 9 } else if case == "already_advertised" { 8 } else { 5 }, "{case}");
        records.push(first);
        if case != "closed_first" {
            rig.link.ingest(1, &close).unwrap();
            assert_eq!(rig.link.original_1f_close_count(), Some(1));
        }
        for _ in 0..12 {
            if records.iter().any(|r| r.1[0] == 8) && rig.link.ipa_init_transport_trace().unwrap().published_at.is_some() { break; }
            records.push(publish(&mut rig.link).expect("pending grant or INIT"));
        }
        let ad = records.iter().position(|r| matches!(r.0, Published::ReceiveIntent(_))).unwrap();
        let ack = records.iter().position(|r| r.0 == Published::IntentRequestAck { granted: true }).unwrap();
        let Published::ReceiveIntent(local_id) = records[ad].0 else { unreachable!() };
        assert_eq!(word(&records[ad].1, 4), 1);
        assert!(word(&records[ad].1, 8) >= 52);
        assert_eq!(word(&records[ad].1, 12), local_id);
        assert_eq!(word(&records[ack].1, 4), 1);
        assert!(ad < ack);
        let sent: Vec<_> = records.iter().enumerate().filter(|(_,r)|
            r.1.get(48..48+init.bytes().len()) == Some(init.bytes())).collect();
        assert_eq!(sent.len(), 1);
        let (pos, (Published::Data {remote_intent, last_fragment: true}, frame)) = sent[0] else { panic!() };
        assert_eq!(word(frame, 44), LINUX_MODEM);
        assert_eq!(pos == 0, borrowed);
        if case == "credit_arrives" {
            assert!(records[ack + 1..pos].iter().any(|(_, bytes)| bytes[0] == 9 && word(bytes, 20) == 7),
                "RESUME_TX must retain its turn before INIT");
        }
        assert_ne!(reg_id, *remote_intent);
        assert_eq!(rig.link.ingest(1, &control(14,*remote_intent)), Ok(Event::TransmitDone));
        assert!(rig.link.ipa_init_transport_trace().unwrap().retired_at.is_some());
        assert!(rig.link.ipa_registration_trace().unwrap().completed_at.is_none());
        assert_eq!(rig.link.ingest(1, &control(14,reg_id)), Ok(Event::TransmitDone));
        assert!(rig.link.ipa_registration_trace().unwrap().completed_at.is_some());
        println!("{case}: queue={at_queue:#x} INIT turn={} advertisement={} ACK={} local intent={local_id:#x}", pos+1, ad+1, ack+1);
    }
}
