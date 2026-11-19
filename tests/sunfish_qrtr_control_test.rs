//! Independent control-wire fixtures and bounded GLINK control exchanges.
#[path = "../rust/src/pixel_glink_control.rs"]
mod pixel_glink_control;
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
pub mod qcom_ipa_service;
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
#[allow(dead_code)] // The shared QMI module names the lifecycle permit type.
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
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
#[path = "../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;

use qcom_glink_qmi::{Config, Error, Event, Published, Session, WriteError};
use qcom_modem_services::Service;
use qcom_qrtr_control::{
    Command, Controller, Error as ControlError, Event as ControlEvent, MAX_SERVERS,
};
use std::collections::VecDeque;

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn wire(kind: u32, fields: [u32; 4]) -> Vec<u8> {
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
fn hello() -> Vec<u8> {
    // Captured Sunfish HELLO profile: peer0 -> broadcast, both control ports.
    [
        1u32,
        2,
        0,
        0xffff_fffe,
        0,
        20,
        0xffff_ffff,
        0xffff_fffe,
        2,
        0,
        0,
        0,
        0,
    ]
    .iter()
    .flat_map(|w| w.to_le_bytes())
    .collect()
}
fn raw_qrtr_data(body: &[u8], source_port: u32, destination_port: u32, confirm: u32) -> Vec<u8> {
    let mut bytes: Vec<u8> = [
        1u32,
        1,
        0,
        source_port,
        confirm,
        body.len() as u32,
        1,
        destination_port,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect();
    bytes.extend_from_slice(body);
    bytes.resize((bytes.len() + 3) & !3, 0);
    bytes
}
fn tftp_rrq(path: &[u8], mode: &[u8], options: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes.extend_from_slice(mode);
    bytes.push(0);
    for (name, value) in options {
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.extend_from_slice(value);
        bytes.push(0);
    }
    bytes
}
fn glink(command: u16, parameter: u32) -> Vec<u8> {
    let mut b = vec![command as u8, (command >> 8) as u8, 2, 0];
    b.extend(parameter.to_le_bytes());
    b
}
fn data(command: u16, id: u32, bytes: &[u8], left: usize) -> Vec<u8> {
    let mut b = glink(command, id);
    b.extend((bytes.len() as u32).to_le_bytes());
    b.extend((left as u32).to_le_bytes());
    b.extend(bytes);
    b.resize((b.len() + 7) & !7, 0xa5);
    b
}
fn new_controller() -> Controller {
    let mut c = Controller::new(1, 0);
    c.start().unwrap();
    c
}
fn accept(c: &mut Controller, b: &[u8]) -> Result<ControlEvent, ControlError> {
    c.receive(b)
}
struct Peer {
    session: Session,
    offered: VecDeque<u32>,
    sent: Vec<Vec<u8>>,
    used: Vec<u32>,
}
impl Peer {
    fn new(remote: bool) -> Self {
        Self::profile(remote, false, false, true)
    }
    fn with_wlfw_client(remote: bool) -> Self {
        Self::profile(remote, true, false, true)
    }
    fn with_prepared_registry() -> Self {
        Self::profile(false, false, true, false)
    }
    fn with_tftp_observer() -> Self {
        let mut peer = Self::profile(false, false, false, false);
        peer.session.prepare_tftp_observer(1, 0x4300).unwrap();
        peer.session.start_control(1).unwrap();
        peer.add_remote_intents();
        peer
    }
    fn with_tftp_server() -> Self {
        let mut peer = Self::profile(false, false, false, false);
        peer.session.prepare_tftp_server(1, 0x4300).unwrap();
        assert!(peer.session.tftp_server_prepared(1, 0x4300));
        assert!(!peer.session.tftp_server_prepared(1, 0x4301));
        peer.session.start_control(1).unwrap();
        peer.add_remote_intents();
        peer
    }
    fn profile(remote: bool, wlfw: bool, registry: bool, start_control: bool) -> Self {
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
        if registry {
            session
                .register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT)
                .unwrap();
            session
                .prepare_service_available(1, Service::ServiceRegistry)
                .unwrap();
        }
        if wlfw {
            // The live restart profile keeps its SERVREG_NOTIF state client
            // active alongside WLFW service discovery on distinct ports.
            session.enable_servreg_notif_client(1, 0x4200).unwrap();
            session.enable_wlfw_client(1, 0x4100).unwrap();
        }
        if start_control {
            session.start_control(1).unwrap();
        }
        let mut peer = Self {
            session,
            offered: VecDeque::new(),
            sent: Vec::new(),
            used: Vec::new(),
        };
        if remote {
            peer.add_remote_intents();
        }
        peer
    }
    fn add_remote_intents(&mut self) {
        let mut b = glink(5, 8);
        for id in 0u32..8 {
            b.extend(4096u32.to_le_bytes());
            b.extend(id.to_le_bytes());
        }
        self.session.ingest(1, &b).unwrap();
    }
    fn drain(&mut self) {
        for _ in 0..64 {
            let mut b = Vec::new();
            let result = self.session.publish_next(1, |record| {
                b.extend(record);
                Ok(())
            });
            match result {
                Ok(Some(Published::ReceiveIntent(id))) => self.offered.push_back(id),
                Ok(Some(Published::Data {
                    remote_intent,
                    last_fragment: true,
                })) => {
                    assert_eq!(word(&b, 12), 0);
                    self.sent.push(b[16..16 + word(&b, 8) as usize].to_vec());
                    self.session.ingest(1, &glink(6, remote_intent)).unwrap();
                }
                Ok(Some(Published::Data { .. })) => panic!("control must fit one chunk"),
                Ok(Some(_)) => {}
                Ok(None) | Err(Error::NoRemoteIntent) => return,
                other => panic!("{other:?}"),
            }
        }
        panic!("unbounded control output");
    }
    fn inject(&mut self, bytes: &[u8]) -> Result<Event, Error> {
        let id = self.offered.pop_front().unwrap();
        assert!(!self.used.contains(&id));
        self.used.push(id);
        self.session.ingest(1, &data(9, id, bytes, 0))
    }
    fn ready(&mut self) {
        self.session
            .set_service_available(1, Service::RemoteStorage, true)
            .unwrap();
        self.session
            .set_service_available(1, Service::HeapMemory, true)
            .unwrap();
        self.drain();
        self.inject(&hello()).unwrap();
        self.drain();
    }
}

#[test]
fn literal_broadcast_hello_validates_without_round_trip_encoding() {
    let mut c = new_controller();
    assert_eq!(c.decode(&hello()), Ok(Command::Hello));
    assert_eq!(
        accept(&mut c, &hello()),
        Ok(ControlEvent::Hello { first: true })
    );
    assert_eq!(
        accept(&mut c, &wire(2, [0; 4])),
        Ok(ControlEvent::Hello { first: false })
    );
    assert!(c.status().peer_hello);
}

#[test]
fn control_commands_ignore_union_words_the_linux_qrtr_handler_does_not_use() {
    let c = new_controller();
    for (kind, expected) in [
        (2, Command::Hello),
        (3, Command::Bye),
        (8, Command::Ignored(8)),
        (9, Command::Ignored(9)),
    ] {
        // QRTR's fixed control struct has a 16-byte union. These commands
        // consume only `cmd`; the remainder is not a reserved-zero field.
        let bytes = wire(kind, [0xdead_beef, 0x1234_5678, 0xa5a5_5a5a, u32::MAX]);
        assert_eq!(c.decode(&bytes), Ok(expected), "command {kind}");
    }

    let del_client = wire(6, [0, 0x4000, 0xcafe_babe, u32::MAX]);
    assert_eq!(
        c.decode(&del_client),
        Ok(Command::DelClient(0x4000)),
        "DEL_CLIENT uses only its node and port words"
    );
}

#[test]
fn all_control_truncations_and_header_mutations_are_side_effect_free() {
    let c = new_controller();
    let b = hello();
    for n in 0..52 {
        assert_eq!(c.decode(&b[..n]), Err(ControlError::Length));
    }
    let mut extended = b.clone();
    extended.push(0);
    assert_eq!(c.decode(&extended), Err(ControlError::Length));
    for (at, value) in [
        (0, 2),
        (4, 4),
        (8, 1),
        (12, 3),
        (16, 1),
        (20, 19),
        (24, 2),
        (28, 0x4000),
        (32, 9),
    ] {
        let mut bad = b.clone();
        put(&mut bad, at, value);
        assert!(c.decode(&bad).is_err(), "word {}", at / 4);
        assert!(!c.status().peer_hello);
        assert_eq!(c.status().servers, 0);
    }
}
#[test]
fn spoofed_server_nodes_ports_and_local_lookup_commands_are_rejected() {
    let c = new_controller();
    for fields in [
        [0, 1, 0, 1],
        [43, 1, 1, 1],
        [43, 1, 0, 0],
        [43, 1, 0, 0xffff_fffe],
        [43, 1, 0, u32::MAX],
    ] {
        assert_eq!(c.decode(&wire(4, fields)), Err(ControlError::Payload));
    }
    for kind in [0, 1, 7, 10, 11, 12, 14, u32::MAX] {
        assert_eq!(
            c.decode(&wire(kind, [0; 4])),
            Err(ControlError::Unsupported)
        );
    }
}
#[test]
fn announcements_require_hello_and_do_not_change_local_backend_readiness() {
    let mut c = new_controller();
    let b = wire(4, [43, 0x1202, 0, 1]);
    assert_eq!(accept(&mut c, &b), Err(ControlError::PeerNotReady));
    accept(&mut c, &hello()).unwrap();
    accept(&mut c, &b).unwrap();
    assert_eq!(c.status().servers, 1);
    assert_eq!(c.status().ready_services, 0);
    assert!(!c.accepts(Service::RemoteStorage));
}
#[test]
fn bounded_registry_coalesces_replaces_and_rejects_overflow_without_eviction() {
    let mut c = new_controller();
    accept(&mut c, &hello()).unwrap();
    for port in 1..=MAX_SERVERS as u32 {
        accept(&mut c, &wire(4, [43, 0x1202, 0, port])).unwrap();
    }
    let first = c.server(0);
    assert_eq!(
        accept(&mut c, &wire(4, [43, 1, 0, 65])),
        Err(ControlError::NoCapacity)
    );
    assert_eq!(c.status().servers, MAX_SERVERS);
    assert_eq!(c.server(0), first);
    assert!(matches!(
        accept(&mut c, &wire(4, [43, 0x1202, 0, 1])),
        Ok(ControlEvent::Server { changed: false, .. })
    ));
    assert!(matches!(
        accept(&mut c, &wire(4, [66, 1, 0, 1])),
        Ok(ControlEvent::Server { changed: true, .. })
    ));
    assert_eq!(c.server(0).unwrap().service, 66);
    assert_eq!(c.status().servers, MAX_SERVERS);
    assert!(c.server(usize::MAX).is_none());
}
#[test]
fn matching_deletes_are_idempotent_but_stale_deletes_do_not_remove_replacements() {
    let mut c = new_controller();
    accept(&mut c, &hello()).unwrap();
    accept(&mut c, &wire(4, [43, 0x1202, 0, 1])).unwrap();
    assert_eq!(
        accept(&mut c, &wire(5, [43, 0x1201, 0, 1])),
        Err(ControlError::ServerConflict)
    );
    assert_eq!(c.status().servers, 1);
    assert!(matches!(
        accept(&mut c, &wire(5, [43, 0x1202, 0, 1])),
        Ok(ControlEvent::Server { changed: true, .. })
    ));
    assert!(matches!(
        accept(&mut c, &wire(5, [43, 0x1202, 0, 1])),
        Ok(ControlEvent::Server { changed: false, .. })
    ));
}
#[test]
fn del_client_accepts_control_or_closed_source_port_and_rejects_spoofing() {
    let mut c = new_controller();
    accept(&mut c, &hello()).unwrap();
    accept(&mut c, &wire(4, [43, 1, 0, 9])).unwrap();
    let mut b = wire(6, [0, 9, 0, 0]);
    put(&mut b, 12, 9);
    assert_eq!(accept(&mut c, &b), Ok(ControlEvent::ClientRemoved(9)));
    assert_eq!(c.status().servers, 0);
    put(&mut b, 12, 10);
    assert!(c.decode(&b).is_err());
    put(&mut b, 12, 0xffff_fffe);
    assert!(c.decode(&b).is_ok());
    put(&mut b, 36, 1);
    assert!(c.decode(&b).is_err());
}
#[test]
fn bye_and_direct_del_proc_latch_peer_loss_without_restarting() {
    for bytes in [wire(3, [0; 4]), wire(13, [0xe111, 0, 0, 0])] {
        let mut c = new_controller();
        accept(&mut c, &hello()).unwrap();
        assert_eq!(accept(&mut c, &bytes), Err(ControlError::PeerGone));
        assert_eq!(c.decode(&hello()), Err(ControlError::PeerGone));
        assert_eq!(c.start(), Err(ControlError::PeerGone));
    }
    let c = new_controller();
    assert!(c.decode(&wire(13, [0xe111, 1, 0, 0])).is_err());
}
#[test]
fn ping_and_exit_have_no_echo_or_backend_side_effects() {
    let mut p = Peer::new(true);
    p.drain();
    p.inject(&hello()).unwrap();
    p.drain();
    assert_eq!(p.sent.len(), 1);
    for kind in [8, 9] {
        assert_eq!(
            p.inject(&wire(kind, [0; 4])),
            Ok(Event::QrtrControl(ControlEvent::Ignored(kind)))
        );
        p.drain();
    }
    assert_eq!(p.sent.len(), 1);
    assert_eq!(p.session.in_flight(), 0);
}
#[test]
fn hello_then_ready_services_emit_exact_descriptors_after_peer_hello() {
    let mut p = Peer::new(true);
    p.session
        .set_service_available(1, Service::RemoteStorage, true)
        .unwrap();
    p.session
        .set_service_available(1, Service::HeapMemory, true)
        .unwrap();
    p.drain();
    assert_eq!(p.sent.len(), 1);
    let expected: Vec<u8> = [
        1u32,
        2,
        1,
        0xffff_fffe,
        0,
        20,
        0,
        0xffff_fffe,
        2,
        0,
        0,
        0,
        0,
    ]
    .iter()
    .flat_map(|w| w.to_le_bytes())
    .collect();
    assert_eq!(p.sent[0], expected);
    p.inject(&hello()).unwrap();
    p.drain();
    assert_eq!(p.sent.len(), 3);
    assert_eq!(
        p.sent[1]
            .chunks_exact(4)
            .map(|b| word(b, 0))
            .collect::<Vec<_>>(),
        [
            1,
            4,
            1,
            0xffff_fffe,
            0,
            20,
            0,
            0xffff_fffe,
            4,
            14,
            1,
            1,
            0x4000
        ]
    );
    assert_eq!(
        p.sent[2]
            .chunks_exact(4)
            .map(|b| word(b, 0))
            .collect::<Vec<_>>(),
        [
            1,
            4,
            1,
            0xffff_fffe,
            0,
            20,
            0,
            0xffff_fffe,
            4,
            52,
            257,
            1,
            0x4001
        ]
    );
    assert_eq!(p.session.control_status().announced_services, 3);
}

#[test]
fn prepared_servreg_is_ready_before_control_and_announced_first_after_peer_hello() {
    let mut p = Peer::with_prepared_registry();
    assert_eq!(p.session.control_status().ready_services, 0b100);
    assert_eq!(p.session.control_status().announced_services, 0);
    p.session.start_control(1).unwrap();
    p.add_remote_intents();
    p.drain();
    assert_eq!(p.sent.len(), 1, "only HELLO before peer HELLO");

    p.session
        .set_service_available(1, Service::RemoteStorage, true)
        .unwrap();
    p.session
        .set_service_available(1, Service::HeapMemory, true)
        .unwrap();
    p.inject(&hello()).unwrap();
    p.drain();

    assert_eq!(p.sent.len(), 4);
    for (packet, service, instance, port) in [
        (&p.sent[1], 0x40, 0x101, qcom_servreg::LOCAL_PORT),
        (&p.sent[2], 14, 1, 0x4000),
        (&p.sent[3], 52, 0x101, 0x4001),
    ] {
        assert_eq!(word(packet, 4), 4, "NEW_SERVER command");
        assert_eq!(word(packet, 36), service);
        assert_eq!(word(packet, 40), instance);
        assert_eq!(word(packet, 44), 1, "local node");
        assert_eq!(word(packet, 48), port);
    }
    assert_eq!(p.session.control_status().announced_services, 0b111);
}

#[test]
fn prepared_local_service_must_be_configured_before_control_starts() {
    let mut c = Controller::new(1, 0);
    c.start().unwrap();
    assert_eq!(
        c.prepare_available(Service::ServiceRegistry, qcom_servreg::LOCAL_PORT),
        Err(ControlError::Busy)
    );
    assert_eq!(c.status().ready_services, 0);
}
#[test]
fn unready_native_profile_emits_hello_but_never_announces_services() {
    let mut p = Peer::new(true);
    p.drain();
    p.inject(&hello()).unwrap();
    p.drain();
    assert_eq!(p.sent.len(), 1);
    assert_eq!(p.session.control_status().announced_services, 0);
    let mut query = wire(1, [0; 4]);
    put(&mut query, 12, 9);
    put(&mut query, 28, 0x4001);
    assert_eq!(
        p.inject(&query),
        Err(Error::Control(ControlError::Unavailable))
    );
    assert_eq!(p.session.in_flight(), 0);
}
#[test]
fn duplicate_hellos_do_not_amplify_output_or_exhaust_receive_slots() {
    let mut p = Peer::new(true);
    p.ready();
    for _ in 0..80 {
        p.inject(&hello()).unwrap();
        p.drain();
    }
    assert_eq!(p.sent.len(), 3);
    assert_eq!(p.used.len(), 81);
    assert_eq!(p.session.control_status().announced_services, 3);
}
#[test]
fn fragmented_control_changes_state_only_after_its_final_fragment() {
    let mut p = Peer::new(true);
    p.drain();
    let id = p.offered.pop_front().unwrap();
    let b = hello();
    assert_eq!(
        p.session.ingest(1, &data(9, id, &b[..19], 33)),
        Ok(Event::Fragment)
    );
    assert!(!p.session.control_status().peer_hello);
    assert_eq!(
        p.session.ingest(1, &data(12, id, &b[19..], 0)),
        Ok(Event::QrtrControl(ControlEvent::Hello { first: true }))
    );
    p.drain();
    assert!(p.session.control_status().peer_hello);
}
#[test]
fn withdrawal_publishes_del_server_without_erasing_pending_transport_state() {
    let mut p = Peer::new(true);
    p.ready();
    p.session
        .set_service_available(1, Service::HeapMemory, false)
        .unwrap();
    p.drain();
    let b = p.sent.last().unwrap();
    assert_eq!(word(b, 4), 5);
    assert_eq!(
        &b[36..],
        &[52u32, 257, 1, 0x4001]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(p.session.control_status().announced_services, 1);
    p.session
        .set_service_available(1, Service::HeapMemory, true)
        .unwrap();
    p.drain();
    assert_eq!(p.session.control_status().announced_services, 3);
}
#[test]
fn remote_intent_backpressure_retains_one_pending_hello_until_capacity_arrives() {
    let mut p = Peer::new(false);
    p.drain();
    for _ in 0..20 {
        p.drain();
    }
    assert!(p.sent.is_empty());
    assert!(!p.session.control_status().hello_sent);
    let mut b = glink(5, 1);
    b.extend(128u32.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    p.session.ingest(1, &b).unwrap();
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    p.session.ingest(1, &glink(8, 1)).unwrap(); // Peer grants the earlier intent request.
    p.drain();
    assert_eq!(p.sent.len(), 1);
    assert!(p.session.control_status().hello_sent);
    p.drain();
    assert_eq!(p.sent.len(), 1);
}
#[test]
fn blocked_or_uncertain_publication_never_marks_a_control_message_sent() {
    for uncertain in [false, true] {
        let mut p = Peer::new(true);
        // Publish two backed receive intents, leaving the HELLO staged.
        for _ in 0..2 {
            assert!(matches!(
                p.session.publish_next(1, |_| Ok(())).unwrap(),
                Some(Published::ReceiveIntent(_))
            ));
        }
        assert_eq!(
            p.session.publish_next(1, |_| Err(if uncertain {
                WriteError::Uncertain
            } else {
                WriteError::WouldBlock
            })),
            Err(if uncertain {
                Error::PublicationUncertain
            } else {
                Error::WouldBlock
            })
        );
        assert!(!p.session.control_status().hello_sent);
        if uncertain {
            assert_eq!(
                p.session.publish_next(1, |_| panic!("must not retry")),
                Err(Error::Quarantined)
            );
        } else {
            p.drain();
            assert_eq!(p.sent.len(), 1);
        }
    }
}
#[test]
fn stale_generation_and_transport_reset_clear_control_readiness_and_inventory() {
    let mut p = Peer::new(true);
    p.ready();
    p.inject(&wire(4, [43, 0x1202, 0, 1])).unwrap();
    p.drain();
    assert!(p.session.remote_server(0).is_some());
    assert_eq!(
        p.session
            .set_service_available(2, Service::HeapMemory, false),
        Err(Error::StaleGeneration)
    );
    p.session.reset_after_transport_reset(2).unwrap();
    let status = p.session.control_status();
    assert!(!status.started && !status.peer_hello && !status.hello_sent);
    assert_eq!(
        (
            status.ready_services,
            status.announced_services,
            status.servers
        ),
        (0, 0, 0)
    );
    assert!(p.session.remote_server(0).is_none());
}
#[test]
fn ready_change_is_busy_until_queued_announcement_has_a_known_publication_outcome() {
    let mut p = Peer::new(true);
    p.drain();
    p.inject(&hello()).unwrap();
    p.drain();
    p.session
        .set_service_available(1, Service::HeapMemory, true)
        .unwrap();
    assert_eq!(
        p.session.publish_next(1, |_| Err(WriteError::WouldBlock)),
        Err(Error::WouldBlock)
    );
    assert_eq!(
        p.session
            .set_service_available(1, Service::HeapMemory, false),
        Err(Error::Control(ControlError::Busy))
    );
    assert_eq!(p.session.control_status().announced_services, 0);
    p.drain();
    assert_eq!(p.session.control_status().announced_services, 2);
}

#[test]
fn ready_qmi_exchange_and_remote_inventory_keep_distinct_routing_and_completion_state() {
    let mut p = Peer::new(true);
    p.ready();
    p.inject(&wire(4, [43, 0x1202, 0, 99])).unwrap();
    p.drain();
    let mut query: Vec<u8> = [1u32, 1, 0, 9, 0, 14, 1, 0x4001]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    query.extend([0, 0x34, 0x12, 0x24, 0, 7, 0, 1, 4, 0, 1, 0, 0, 0]);
    query.resize(48, 0x5a);
    let Event::Request(handle) = p.inject(&query).unwrap() else {
        panic!("not a request")
    };
    assert_eq!(p.session.request(&handle).unwrap().0.transaction, 0x1234);
    p.drain();
    p.session
        .queue_reply(&handle, qcom_glink_qmi::Reply::Failure { error: 3 })
        .unwrap();
    p.drain();
    let reply = p.sent.last().unwrap();
    assert_eq!(word(reply, 4), 1);
    assert_eq!(&reply[33..35], &0x1234u16.to_le_bytes());
    assert_eq!(p.session.in_flight(), 1); // GLINK completion alone did not retire QMI.
    let mut resume: Vec<u8> = [1u32, 7, 0, 9, 0, 20, 1, 0x4001, 7, 0, 9, 0, 0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    assert_eq!(p.inject(&resume), Ok(Event::ReplyQrtrResume(handle)));
    p.drain();
    assert_eq!(p.session.in_flight(), 0);
    assert_eq!(p.session.control_status().servers, 1);
    assert_eq!(p.session.remote_server(0).unwrap().address.port, 99);
    // Linux QRTR silently ignores a RESUME_TX whose body names no tracked
    // destination flow. The receive intent is reusable, but no state changes.
    put(&mut resume, 40, 10);
    assert_eq!(p.inject(&resume), Ok(Event::CreditReceived));
    p.drain();
    assert_eq!(p.session.in_flight(), 0);
    assert_eq!(p.session.control_status().servers, 1);
}

#[test]
fn wlfw_discovery_uses_only_unique_passive_peer_announcements() {
    let mut p = Peer::new(true);
    assert_eq!(p.session.wlfw_endpoint(), None);
    p.ready();
    let sent_before = p.sent.len();

    // WLFW service 0x45/version 1/instance 0 is QRTR's packed instance 1.
    // Other versions and services must not become a WLAN client endpoint.
    p.inject(&wire(4, [0x45, 2, 0, 0x5000])).unwrap();
    p.inject(&wire(4, [0x46, 1, 0, 0x5001])).unwrap();
    p.drain();
    assert_eq!(p.session.wlfw_endpoint(), None);

    p.inject(&wire(4, [0x45, 1, 0, 0x5002])).unwrap();
    p.drain();
    assert_eq!(
        p.session.wlfw_endpoint(),
        Some(qcom_qmi_router::Address {
            node: 0,
            port: 0x5002,
        })
    );

    // Multiple matching servers are ambiguous and fail closed. Removing one
    // exact address restores the unique endpoint without any outbound lookup.
    p.inject(&wire(4, [0x45, 1, 0, 0x5003])).unwrap();
    p.drain();
    assert_eq!(p.session.wlfw_endpoint(), None);
    p.inject(&wire(5, [0x45, 1, 0, 0x5002])).unwrap();
    p.drain();
    assert_eq!(
        p.session.wlfw_endpoint(),
        Some(qcom_qmi_router::Address {
            node: 0,
            port: 0x5003,
        })
    );
    assert_eq!(p.sent.len(), sent_before);
}

fn wlfw_registration_response(transaction: u16, result: u16, error: u16) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend(0x0020u16.to_le_bytes());
    let mut payload = vec![2, 4, 0];
    payload.extend(result.to_le_bytes());
    payload.extend(error.to_le_bytes());
    qmi.extend((payload.len() as u16).to_le_bytes());
    qmi.extend(payload);
    let mut datagram: Vec<u8> = [1u32, 1, 0, 0x5002, 0, qmi.len() as u32, 1, 0x4100]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .chain(qmi)
        .collect();
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

#[test]
fn native_session_sends_only_exact_wlfw_registration_and_stops_at_msa_gate() {
    use crate::qcom_servreg_notif_client::State as ServregState;
    use crate::qcom_wlfw_client::{RegistrationOutcome, State};

    let mut p = Peer::with_wlfw_client(true);
    p.ready();
    assert_eq!(p.session.wlfw_state(), Some(State::Searching));
    assert_eq!(
        p.session.servreg_notif_state(),
        Some(ServregState::Searching)
    );

    p.inject(&wire(4, [0x45, 1, 0, 0x5002])).unwrap();
    p.drain();
    assert_eq!(p.session.wlfw_state(), Some(State::RegistrationPending));
    let request = p.sent.last().unwrap();
    assert_eq!(word(request, 0), 1);
    assert_eq!(word(request, 4), 1);
    assert_eq!(word(request, 8), 1);
    assert_eq!(word(request, 12), 0x4100);
    assert_eq!(word(request, 16), 0);
    assert_eq!(word(request, 20), 33);
    assert_eq!(word(request, 24), 0);
    assert_eq!(word(request, 28), 0x5002);
    assert_eq!(request[32], 0);
    assert_eq!(u16::from_le_bytes([request[35], request[36]]), 0x20);
    assert_eq!(request.len(), 68);
    assert_eq!(
        &request[32 + 7 + 19..32 + 7 + 26],
        &[0x19, 4, 0, 1, 0, 0, 0]
    );
    assert_eq!(&request[65..68], &[0, 0, 0]);

    let response_id = p.offered.pop_front().unwrap();
    let response = wlfw_registration_response(1, 0, 0);
    assert_eq!(
        p.session
            .ingest(1, &data(9, response_id, &response, 0))
            .unwrap(),
        Event::WlfwRegistration(RegistrationOutcome::Accepted {
            firmware_status: None,
            confirm_rx: false,
        })
    );
    assert_eq!(p.session.wlfw_state(), Some(State::MsaInfoReady));
    assert_eq!(
        p.session.wlfw_outcome(),
        Some(RegistrationOutcome::Accepted {
            firmware_status: None,
            confirm_rx: false,
        })
    );
    let sent = p.sent.len();
    p.drain();
    assert_eq!(p.sent.len(), sent);
    assert_eq!(p.session.wlfw_state(), Some(State::MsaInfoReady));
}

#[test]
fn peer_loss_freezes_queued_output_and_does_not_publish_receive_done() {
    let mut p = Peer::new(true);
    p.ready();
    assert_eq!(
        p.inject(&wire(3, [0; 4])),
        Err(Error::Control(ControlError::PeerGone))
    );
    assert_eq!(
        p.session
            .publish_next(1, |_| panic!("peer loss must hold output")),
        Err(Error::Quarantined)
    );
}

#[test]
fn tftp_server_announcement_is_gated_and_precedes_modem_data_services() {
    let mut c = Controller::new(1, 0);
    c.prepare_available(Service::ServiceRegistry, 0x4201)
        .unwrap();
    c.prepare_tftp_available(0x4300).unwrap();
    c.prepare_available(Service::RemoteStorage, 0x4000).unwrap();
    c.prepare_available(Service::HeapMemory, 0x4001).unwrap();
    assert_eq!(c.tftp_port(), Some(0x4300));
    assert!(!c.accepts_tftp());
    assert_eq!(c.status().ready_services & 0b1111, 0b1111);
    assert_eq!(c.status().announced_services & 0b1111, 0);

    c.start().unwrap();
    let hello_token = c.next().unwrap();
    assert_eq!(hello_token, qcom_qrtr_control::Outbound::Hello);
    c.queued(hello_token);
    c.published(hello_token);
    assert_eq!(
        accept(&mut c, &hello()),
        Ok(ControlEvent::Hello { first: true })
    );

    let registry = c.next().unwrap();
    assert_eq!(
        registry,
        qcom_qrtr_control::Outbound::NewServer(Service::ServiceRegistry)
    );
    c.queued(registry);
    c.published(registry);
    assert!(!c.accepts_tftp());

    let tftp = c.next().unwrap();
    assert_eq!(tftp, qcom_qrtr_control::Outbound::NewTftpServer);
    let announcement = c.encode(tftp);
    assert_eq!(word(&announcement, 4), 4);
    assert_eq!(word(&announcement, 36), qcom_qrtr_control::TFTP_SERVICE_ID);
    assert_eq!(
        word(&announcement, 40),
        qcom_qrtr_control::TFTP_VERSION_INSTANCE
    );
    assert_eq!(word(&announcement, 44), 1);
    assert_eq!(word(&announcement, 48), 0x4300);
    c.queued(tftp);
    c.published(tftp);
    assert!(c.accepts_tftp());

    assert_eq!(
        c.next(),
        Some(qcom_qrtr_control::Outbound::NewServer(
            Service::RemoteStorage
        ))
    );
}

#[test]
fn tftp_withdrawal_disables_receive_before_its_qrtr_del_server_is_published() {
    let mut c = Controller::new(1, 0);
    c.prepare_tftp_available(0x4300).unwrap();
    c.start().unwrap();
    let hello_token = c.next().unwrap();
    c.queued(hello_token);
    c.published(hello_token);
    accept(&mut c, &hello()).unwrap();
    let tftp = c.next().unwrap();
    c.queued(tftp);
    c.published(tftp);
    assert!(c.accepts_tftp());

    c.set_tftp_available(0x4300, false).unwrap();
    assert!(!c.accepts_tftp());
    let withdrawal = c.next().unwrap();
    assert_eq!(withdrawal, qcom_qrtr_control::Outbound::DelTftpServer);
    c.queued(withdrawal);
    c.published(withdrawal);
    assert_eq!(c.status().announced_services & (1 << 3), 0);
}

#[test]
fn raw_tftp_rrq_is_observed_without_qmi_routing_or_tftp_response() {
    let mut p = Peer::with_tftp_observer();
    p.ready();
    assert_ne!(p.session.control_status().announced_services & (1 << 3), 0);
    assert!(p.sent.iter().any(|packet| {
        word(packet, 4) == 4
            && word(packet, 36) == qcom_qrtr_control::TFTP_SERVICE_ID
            && word(packet, 40) == qcom_qrtr_control::TFTP_VERSION_INSTANCE
            && word(packet, 48) == 0x4300
    }));
    assert_eq!(p.session.in_flight(), 0);

    let before = p.sent.len();
    let rrq = tftp_rrq(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"blksize", b"7680"), (b"tsize", b"0")],
    );
    let datagram = raw_qrtr_data(&rrq, 0x6000, 0x4300, 1);
    match p.inject(&datagram).unwrap() {
        Event::TftpObserved {
            peer,
            observation: Ok(observation),
        } => {
            assert_eq!(peer.node, 0);
            assert_eq!(peer.port, 0x6000);
            assert_eq!(observation.opcode, qcom_tftp::OP_RRQ);
            assert_eq!(
                observation.path_text(),
                "/readonly/firmware/image/wlanmdsp.mbn"
            );
            assert_eq!(observation.options.unwrap().block_size, 7680);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    p.drain();
    assert_eq!(p.session.in_flight(), 0);
    assert_eq!(p.sent.len(), before + 1);
    let credit = p.sent.last().unwrap();
    assert_eq!(word(credit, 4), 7);
    assert_eq!(word(credit, 8), 1);
    assert_eq!(word(credit, 12), 0x4300);
    assert_eq!(word(credit, 24), 0);
    assert_eq!(word(credit, 28), 0x6000);
}

#[test]
fn tftp_server_profile_holds_raw_datagram_until_bounded_dispatch_finishes() {
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Dispatch, IncomingRoute, Peer as TftpPeer, Server};

    let mut p = Peer::with_tftp_server();
    p.ready();
    let rrq = tftp_rrq(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"tsize", b"0")],
    );
    let handle = match p.inject(&raw_qrtr_data(&rrq, 0x6000, 0x4300, 1)).unwrap() {
        Event::TftpRequest(handle) => handle,
        other => panic!("unexpected event: {other:?}"),
    };
    let (reply_route, packet) = p.session.tftp_datagram(&handle).unwrap();
    assert_eq!(packet, rrq);
    assert_eq!(reply_route.source.node, 1);
    assert_eq!(reply_route.source.port, 0x4300);
    assert_eq!(reply_route.destination.node, 0);
    assert_eq!(reply_route.destination.port, 0x6000);

    let image = b"synthetic authorized-source stand-in";
    let mut server = Server::new(
        1,
        0,
        0x4300,
        0x4400,
        &[],
        AuthorizedWlanImage::test_fixture(image),
    )
    .unwrap();
    let incoming = IncomingRoute {
        destination_node: reply_route.source.node,
        destination_port: reply_route.source.port,
        source: TftpPeer {
            node: reply_route.destination.node,
            port: reply_route.destination.port,
        },
    };
    let mut response = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let dispatch = server.receive(incoming, packet, &mut response);
    let Dispatch::Send { route, bytes } = dispatch else {
        panic!("expected bounded TFTP response: {dispatch:?}");
    };
    assert_eq!(&response[..2], &qcom_tftp::OP_OACK.to_be_bytes());
    assert!(response[..bytes]
        .windows(6)
        .any(|field| field == b"tsize\0"));

    p.session
        .queue_tftp_response(
            1,
            qcom_glink_qmi::TftpReplyRoute {
                source: qcom_qmi_router::Address {
                    node: route.source_node,
                    port: route.source_port,
                },
                destination: qcom_qmi_router::Address {
                    node: route.destination.node,
                    port: route.destination.port,
                },
            },
            &response[..bytes],
        )
        .unwrap();
    p.session.finish_tftp_request(&handle).unwrap();
    assert!(p.session.tftp_datagram(&handle).is_err());
    p.drain();

    let reply = p
        .sent
        .iter()
        .find(|packet| {
            word(packet, 4) == 1 && word(packet, 12) == 0x4400 && word(packet, 28) == 0x6000
        })
        .unwrap();
    assert_eq!(&reply[32..34], &qcom_tftp::OP_OACK.to_be_bytes());
    assert_eq!(p.session.in_flight(), 0);
}

#[test]
#[cfg(feature = "sunfish-glink-credit-fairness-experiment")]
fn ready_tftp_reply_gets_a_turn_between_credits() {
    let mut p = Peer::with_tftp_server();
    p.ready();
    let rrq = tftp_rrq(b"/readonly/test", b"octet", &[]);
    assert!(matches!(p.inject(&raw_qrtr_data(&rrq, 0x6000, 0x4300, 1)), Ok(Event::TftpRequest(_))));
    assert!(matches!(p.inject(&raw_qrtr_data(&rrq, 0x6001, 0x4300, 1)), Ok(Event::TftpRequest(_))));
    p.session.queue_tftp_response(1, qcom_glink_qmi::TftpReplyRoute {
        source: qcom_qmi_router::Address { node: 1, port: 0x4301 },
        destination: qcom_qmi_router::Address { node: 0, port: 0x7000 },
    }, &[0, 4, 0, 1]).unwrap();
    let mut records = Vec::new();
    for _ in 0..32 {
        let mut wire = Vec::new();
        let event = p.session.publish_next(1, |bytes| { wire.extend_from_slice(bytes); Ok(()) }).unwrap();
        if matches!(event, Some(Published::Data { .. })) { records.push(word(&wire, 20)); }
        if records.len() == 3 { break; }
    }
    assert_eq!(records, [7, 1, 7], "credit, TFTP reply, credit");
}

#[test]
#[cfg(all(feature = "sunfish-glink-credit-fairness-experiment", feature = "sunfish-glink-intent-request-experiment"))]
fn pending_peer_intent_control_precedes_tftp_fairness_selection() {
    let mut p = Peer::with_tftp_server();
    p.ready();
    let rrq = tftp_rrq(b"/readonly/test", b"octet", &[]);
    assert!(matches!(p.inject(&raw_qrtr_data(&rrq, 0x6000, 0x4300, 1)), Ok(Event::TftpRequest(_))));
    p.session.queue_tftp_response(1, qcom_glink_qmi::TftpReplyRoute {
        source: qcom_qmi_router::Address { node: 1, port: 0x4301 },
        destination: qcom_qmi_router::Address { node: 0, port: 0x7000 },
    }, &[0, 4, 0, 1]).unwrap();
    assert!(matches!(p.session.ingest(1, &glink(7, 52)), Ok(Event::ReceiveIntentRequest { granted: true })));
    let mut wire = Vec::new();
    let event = p.session.publish_next(1, |bytes| { wire.extend_from_slice(bytes); Ok(()) }).unwrap();
    assert!(matches!(event, Some(Published::ReceiveIntent(_))), "the peer's backing advertisement retains priority");
    assert_eq!(p.session.intent_gate_combinations().1, 7, "ready data, pending ACK and advertisement coincide");
    assert_eq!(p.session.intent_gate_queue_snapshot().0, 1, "TFTP remains unsent on this turn");
    let mut ack_seen = false;
    let mut tftp_sent = false;
    for _ in 0..16 {
        let mut wire = Vec::new();
        let event = p.session.publish_next(1, |bytes| { wire.extend_from_slice(bytes); Ok(()) }).unwrap();
        if matches!(event, Some(Published::IntentRequestAck { granted: true })) { ack_seen = true; }
        if matches!(event, Some(Published::Data { .. })) && word(&wire, 20) == 1 {
            assert!(ack_seen, "the peer grant must be acknowledged before data in this scenario");
            tftp_sent = true;
            break;
        }
    }
    assert!(tftp_sent, "with a quiet peer, control priority alone must not permanently starve TFTP");
}

#[test]
fn tftp_reply_uses_its_transfer_tid_and_retires_after_both_transport_completions() {
    let mut p = Peer::with_tftp_observer();
    p.ready();
    let rrq = tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[]);
    assert!(matches!(
        p.inject(&raw_qrtr_data(&rrq, 0x6000, 0x4300, 1)),
        Ok(Event::TftpObserved { .. })
    ));
    p.drain();

    let tftp_data = [0, qcom_tftp::OP_DATA as u8, 0, 1, b'x'];
    let route = qcom_glink_qmi::TftpReplyRoute {
        source: qcom_qmi_router::Address {
            node: 1,
            port: 0x4400,
        },
        destination: qcom_qmi_router::Address {
            node: 0,
            port: 0x6000,
        },
    };
    p.session.queue_tftp_response(1, route, &tftp_data).unwrap();
    p.drain();
    let reply = p.sent.last().unwrap();
    assert_eq!(word(reply, 0), 1);
    assert_eq!(word(reply, 4), 1);
    assert_eq!(word(reply, 8), 1);
    assert_eq!(word(reply, 12), 0x4400);
    assert_eq!(word(reply, 16), 1);
    assert_eq!(word(reply, 20), 5);
    assert_eq!(word(reply, 24), 0);
    assert_eq!(word(reply, 28), 0x6000);
    assert_eq!(&reply[32..37], &tftp_data);
    assert_eq!(
        p.session.queue_tftp_response(1, route, &tftp_data),
        Err(Error::FlowBusy)
    );

    // GLINK completion alone is not enough; the QRTR peer must return the
    // matching RESUME_TX flow key before this transfer route can retire.
    let resume: Vec<u8> = [1u32, 7, 0, 0x6000, 0, 20, 1, 0x4400, 7, 0, 0x6000, 0, 0]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    assert_eq!(p.inject(&resume), Ok(Event::CreditReceived));
    p.drain();

    // Exercise more distinct transfer TIDs than the transport's bounded
    // historical flow table; retired TFTP flows must release their slots.
    for index in 0..10u32 {
        let mut remote_intent = glink(5, 1);
        remote_intent.extend(4096u32.to_le_bytes());
        remote_intent.extend((0x7000 + index).to_le_bytes());
        p.session.ingest(1, &remote_intent).unwrap();
        let route = qcom_glink_qmi::TftpReplyRoute {
            source: qcom_qmi_router::Address {
                node: 1,
                port: 0x4401 + index,
            },
            destination: qcom_qmi_router::Address {
                node: 0,
                port: 0x6100 + index,
            },
        };
        p.session.queue_tftp_response(1, route, &tftp_data).unwrap();
        p.drain();
        let resume: Vec<u8> = [
            1u32,
            7,
            0,
            route.destination.port,
            0,
            20,
            1,
            route.source.port,
            7,
            0,
            route.destination.port,
            0,
            0,
        ]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
        assert_eq!(
            p.inject(&resume),
            Ok(Event::CreditReceived),
            "transfer {index}, diagnostic {:?}",
            p.session.last_credit_diagnostic()
        );
        p.drain();
    }
}
