// Offline exchange fixtures. No native FIFO, firmware, ADB, or Fastboot.
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
pub mod sunfish_ipa_linux_config;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
pub mod sunfish_ipa_linux_memory;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/sunfish_ipa_linux_qmi.rs"]
pub mod sunfish_ipa_linux_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_ipa_service.rs"]
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
#[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
#[path = "../rust/src/qcom_qrtr_flow.rs"]
pub mod qcom_qrtr_flow;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[cfg(feature = "sunfish-native-tftp-server-check")]
#[path = "../rust/src/qcom_tftp_server_check.rs"]
pub mod qcom_tftp_server_check;
#[path = "../rust/src/qcom_wlfw_bdf.rs"]
pub mod qcom_wlfw_bdf;
#[path = "../rust/src/qcom_wlfw_client.rs"]
pub mod qcom_wlfw_client;
#[allow(dead_code)] // This standalone test only consumes the power permit API.
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;

use qcom_glink_qmi::{Config, Error, Event, Published, Reply, RequestHandle, Session, WriteError};
use qcom_modem_services::{MemoryRequest, Partition, Request, Service, StorageRequest};
use qcom_qmi::{Kind, Message};
use qcom_qmi_router::Error as RouteError;

const STORAGE: u32 = 0x4000;
const MEMORY: u32 = 0x4001;

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn config() -> Config {
    Config {
        local_node: 1,
        peer_node: 0,
        local_channel: 1,
        remote_channel: 2,
    }
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn post_auth_runtime_session_has_no_early_publication_or_tftp() {
    let session = Session::native_runtime_profile(config(), 1).unwrap();
    let report = session.runtime_profile_report();
    assert!(report.remote_storage_registered);
    assert!(!report.remote_storage_ready);
    assert!(!report.service_registry_registered);
    assert!(!report.ipa_server_registered);
    assert!(report.linux_ipa_client_enabled);
    assert_eq!(report.tftp_port, None);
    assert!(!report.tftp_server_prepared);
    assert_eq!(session.endpoint(Service::RemoteStorage).unwrap().address.port, STORAGE);
    assert!(session.transport_is_pristine());
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn post_auth_runtime_profile_does_not_change_the_auth_only_session() {
    let mut session = Session::new(config(), 1).unwrap();
    for (service, port) in [
        (Service::RemoteStorage, STORAGE),
        (Service::ServiceRegistry, 0x4003),
        (Service::IpaServer, 0x4002),
    ] {
        session.register(1, service, port).unwrap();
        session.prepare_service_available(1, service).unwrap();
    }
    session.enable_linux_ipa(1, 0x4004).unwrap();
    session.prepare_tftp_server(1, 0x5000).unwrap();
    assert!(session.endpoint(Service::ServiceRegistry).is_some());
    assert!(session.endpoint(Service::IpaServer).is_some());
    assert!(session.tftp_server_prepared(1, 0x5000));
    assert!(session.runtime_profile_report().remote_storage_ready);
}
fn session() -> Session {
    let mut session = Session::new(config(), 1).unwrap();
    session
        .register(1, Service::RemoteStorage, STORAGE)
        .unwrap();
    session.register(1, Service::HeapMemory, MEMORY).unwrap();
    session
}
fn publish(session: &mut Session) -> Option<(Published, Vec<u8>)> {
    let mut bytes = Vec::new();
    session
        .publish_next(1, |record| {
            bytes.extend(record);
            Ok(())
        })
        .unwrap()
        .map(|event| (event, bytes))
}
fn publish_until_data(session: &mut Session) -> (Published, Vec<u8>) {
    for _ in 0..8 {
        let (event, bytes) = publish(session).expect("bounded session should emit queued data");
        if matches!(event, Published::Data { .. }) {
            return (event, bytes);
        }
        assert!(matches!(
            event,
            Published::ReceiveIntent(_) | Published::ReceiveDone(_)
        ));
    }
    panic!("data was not published within the fixed receive queue bound");
}
fn offer(session: &mut Session) -> u32 {
    let id = session.offer_receive(1).unwrap();
    let (event, bytes) = publish(session).unwrap();
    assert_eq!(event, Published::ReceiveIntent(id));
    assert_eq!(&bytes[..4], &[5, 0, 1, 0]);
    assert_eq!(word(&bytes, 8), 4096);
    assert_eq!(word(&bytes, 12), id);
    id
}
fn control(command: u16, parameter: u32) -> Vec<u8> {
    let mut bytes = vec![0; 8];
    bytes[..2].copy_from_slice(&command.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    put(&mut bytes, 4, parameter);
    bytes
}
fn intents(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = control(5, entries.len() as u32);
    for &(id, capacity) in entries {
        bytes.extend(capacity.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    bytes
}
fn data(command: u16, intent: u32, payload: &[u8], remaining: usize) -> Vec<u8> {
    let mut bytes = control(command, intent);
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend((remaining as u32).to_le_bytes());
    bytes.extend(payload);
    bytes.resize((bytes.len() + 7) & !7, 0xa5); // Incoming GLINK padding is unspecified.
    bytes
}
fn qrtr(kind: u32, peer_port: u32, local_port: u32, confirm: bool, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [
        1,
        kind,
        0,
        peer_port,
        u32::from(confirm),
        payload.len() as u32,
        1,
        local_port,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(payload);
    bytes.resize((bytes.len() + 3) & !3, 0x5a); // Independent QRTR padding.
    bytes
}
fn qrtr_control(kind: u32, fields: [u32; 4]) -> Vec<u8> {
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
    .flat_map(|word| word.to_le_bytes())
    .collect()
}
fn servreg_state_response(transaction: u16, state: i32) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend([0x21, 0, 14, 0]);
    qmi.extend([0x02, 4, 0, 0, 0, 0, 0]);
    qmi.extend([0x10, 4, 0]);
    qmi.extend(state.to_le_bytes());
    qmi
}
fn servreg_restart_response(transaction: u16) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend([0x24, 0, 7, 0]);
    qmi.extend([0x02, 4, 0, 0, 0, 0, 0]);
    qmi
}
fn servreg_listener_response(transaction: u16, state: i32) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend([0x20, 0, 14, 0]);
    qmi.extend([0x02, 4, 0, 0, 0, 0, 0]);
    qmi.extend([0x10, 4, 0]);
    qmi.extend(state.to_le_bytes());
    qmi
}
fn servreg_state_updated_indication(
    transaction: u16,
    indication_transaction: u16,
    state: i32,
) -> Vec<u8> {
    let mut qmi = vec![4];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend([0x22, 0, 0, 0]);
    qmi.extend([0x01, 4, 0]);
    qmi.extend(state.to_le_bytes());
    qmi.extend([0x02, 17, 0]);
    qmi.extend(b"msm/modem/wlan_pd");
    qmi.extend([0x03, 2, 0]);
    qmi.extend(indication_transaction.to_le_bytes());
    let payload_len = (qmi.len() - 7) as u16;
    qmi[5..7].copy_from_slice(&payload_len.to_le_bytes());
    qmi
}
fn servreg_set_ack_response(transaction: u16) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend([0x23, 0, 7, 0]);
    qmi.extend([0x02, 4, 0, 0, 0, 0, 0]);
    qmi
}
fn ipa_init_response(transaction: u16, result: u16, error: u16) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend(transaction.to_le_bytes());
    qmi.extend([0x21, 0, 7, 0]);
    qmi.extend([0x02, 4, 0]);
    qmi.extend(result.to_le_bytes());
    qmi.extend(error.to_le_bytes());
    qmi
}

struct FakeWlanPowerIo;

impl qcom_wlan_power::Io for FakeWlanPowerIo {
    type Snapshot = u8;
    type Lease = ();

    fn acquire(&mut self, _generation: u64) -> Result<Self::Lease, qcom_wlan_power::IoError> {
        Ok(())
    }

    fn snapshot(
        &mut self,
        _lease: &Self::Lease,
        _resource: qcom_wlan_power::Resource,
    ) -> Result<Self::Snapshot, qcom_wlan_power::IoError> {
        Ok(0)
    }

    fn apply(
        &mut self,
        _lease: &Self::Lease,
        _action: qcom_wlan_power::Action,
    ) -> Result<(), qcom_wlan_power::IoError> {
        Ok(())
    }

    fn restore(
        &mut self,
        _lease: &Self::Lease,
        _resource: qcom_wlan_power::Resource,
        _snapshot: Self::Snapshot,
    ) -> Result<(), qcom_wlan_power::IoError> {
        Ok(())
    }
}

fn fake_power_ready_permit(generation: u64) -> qcom_wlan_power::PowerReadyPermit {
    let profile = qcom_wlan_power::VerifiedIcNssProfile::test_fixture(
        generation,
        qcom_wlan_power::Config::new(qcom_wlan_power::ALL_RAILS_MASK, false),
    );
    let plan = qcom_wlan_power::verified_sunfish_plan(&profile).unwrap();
    let mut sequencer = qcom_wlan_power::Sequencer::<u8>::default();
    sequencer
        .start(&mut FakeWlanPowerIo, generation, plan)
        .unwrap();
    sequencer.power_ready_permit(generation).unwrap()
}

#[test]
fn production_wlfw_client_waits_for_generation_matched_icnss_power_permit() {
    let mut session = Session::new(config(), 1).unwrap();
    session
        .enable_wlfw_client_power_gated(1, 0x4100)
        .unwrap();
    session.start_control(1).unwrap();
    session
        .ingest(
            1,
            &intents(&[(7, 4096), (8, 4096), (9, 4096), (10, 4096)]),
        )
        .unwrap();

    let hello_intent = offer(&mut session);
    let hello = qrtr_control(2, [0; 4]);
    assert!(matches!(
        session.ingest(1, &data(9, hello_intent, &hello, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))
    ));

    let (server_offer, _) = publish(&mut session).expect("receive pool refills");
    let server_intent = match server_offer {
        Published::ReceiveIntent(id) => id,
        other => panic!("expected the refilled receive intent, got {other:?}"),
    };
    let server = qrtr_control(
        4,
        [
            qcom_qrtr_control::WLFW_SERVICE_ID,
            qcom_qrtr_control::WLFW_VERSION_INSTANCE,
            0,
            0x980,
        ],
    );
    assert!(matches!(
        session.ingest(1, &data(9, server_intent, &server, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Server {
            added: true,
            ..
        }))
    ));
    assert_eq!(
        session.wlfw_state(),
        Some(qcom_wlfw_client::State::ReadyToRegister)
    );

    // Publishing and direct higher-level requests cannot bypass ICNSS power.
    for _ in 0..qcom_glink_qmi::RX_SLOTS {
        let _ = publish(&mut session);
        assert_eq!(
            session.wlfw_state(),
            Some(qcom_wlfw_client::State::ReadyToRegister)
        );
    }
    assert_eq!(
        session.begin_wlfw_capability_request(1),
        Err(Error::Wlfw(qcom_wlfw_client::Error::PowerPrerequisite))
    );
    assert_eq!(
        session.wlfw_state(),
        Some(qcom_wlfw_client::State::ReadyToRegister)
    );

    session
        .authorize_wlfw_power(1, fake_power_ready_permit(1))
        .unwrap();
    let _ = publish(&mut session);
    assert_eq!(
        session.wlfw_state(),
        Some(qcom_wlfw_client::State::RegistrationPending)
    );
}

fn ipa_server_request(transaction: u16, message_id: u16, fields: &[(u8, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend(transaction.to_le_bytes());
    bytes.extend(message_id.to_le_bytes());
    bytes.extend([0, 0]);
    for (tag, value) in fields {
        bytes.push(*tag);
        bytes.extend((value.len() as u16).to_le_bytes());
        bytes.extend(*value);
    }
    let payload_len = u16::try_from(bytes.len() - 7).unwrap();
    bytes[5..7].copy_from_slice(&payload_len.to_le_bytes());
    bytes
}

#[test]
fn ipa_ap_service_responds_to_indication_registration_only_after_publish_commit() {
    let peer = qcom_qmi_router::Address { node: 0, port: 27 };
    let mut server = qcom_ipa_service::Server::new(peer, 3).unwrap();
    let request = ipa_server_request(0x1234, 0x0020, &[(0x10, &[1]), (0x12, &[0])]);

    assert_eq!(
        server.prepare_response(3, peer, &request, qcom_ipa_service::QMI_MESSAGE_BYTES - 1),
        Err(qcom_ipa_service::Error::OutputTooSmall)
    );
    assert_eq!(server.registration(), None);

    let reply = server
        .prepare_response(3, peer, &request, qcom_ipa_service::QMI_MESSAGE_BYTES)
        .unwrap();
    assert_eq!(reply.bytes().len(), 14);
    let decoded = Message::decode(reply.bytes()).unwrap();
    assert_eq!(decoded.kind, Kind::Response);
    assert_eq!(decoded.transaction, 0x1234);
    assert_eq!(decoded.message_id, 0x0020);
    assert_eq!(decoded.required(0x02).unwrap(), &[0, 0, 0, 0]);
    assert_eq!(server.registration(), None);

    server.commit_response(3, &reply).unwrap();
    assert_eq!(
        server.registration(),
        Some(qcom_ipa_service::Registration {
            master_driver_init_complete: Some(true),
            data_usage_quota_reached: None,
            mhi_ready: Some(false),
            endpoint_desc: None,
            bandwidth_change: None,
        })
    );
    assert!(!server.indication_sent());
}

#[test]
fn ipa_ap_service_matches_source_service_identity_and_bounds_messages() {
    assert_eq!(qcom_ipa_service::SERVICE_ID, 0x31);
    assert_eq!(qcom_ipa_service::QRTR_VERSION_INSTANCE, 0x0000_0101);
    assert_eq!(qcom_ipa_service::INDICATION_REGISTER_MESSAGE_ID, 0x0020);
    assert_eq!(
        qcom_ipa_service::MASTER_DRIVER_INIT_COMPLETE_MESSAGE_ID,
        0x0022
    );
    assert_eq!(
        qcom_ipa_service::INIT_MODEM_DRIVER_COMPLETE_MESSAGE_ID,
        0x0035
    );

    let peer = qcom_qmi_router::Address { node: 0, port: 27 };
    let mut server = qcom_ipa_service::Server::new(peer, 5).unwrap();
    let oversized_completion = ipa_server_request(1, 0x0035, &[(0x01, &[1]), (0x10, &[0])]);
    assert_eq!(
        server.prepare_response(5, peer, &oversized_completion, 14),
        Err(qcom_ipa_service::Error::InputTooLarge)
    );
    assert_eq!(server.modem_init_status(), None);
}

#[test]
fn ipa_ap_service_acks_modem_completion_and_sends_master_indication_after_both_gates() {
    let peer = qcom_qmi_router::Address { node: 0, port: 27 };
    let mut server = qcom_ipa_service::Server::new(peer, 7).unwrap();
    assert_eq!(
        server
            .prepare_master_init_indication(7, qcom_ipa_service::QMI_MESSAGE_BYTES)
            .unwrap(),
        None
    );

    let completion = ipa_server_request(9, 0x0035, &[(0x01, &[1])]);
    let reply = server
        .prepare_response(7, peer, &completion, qcom_ipa_service::QMI_MESSAGE_BYTES)
        .unwrap();
    assert_eq!(server.modem_init_status(), None);
    server.commit_response(7, &reply).unwrap();
    assert_eq!(server.modem_init_status(), Some(1));

    let stale_readiness = qcom_ipa_service::ApReadyPermit::test_after_post_init(6);
    assert_eq!(
        server.note_ap_init_complete(7, &stale_readiness),
        Err(qcom_ipa_service::Error::StaleGeneration)
    );
    let readiness = qcom_ipa_service::ApReadyPermit::test_after_post_init(7);
    server.note_ap_init_complete(7, &readiness).unwrap();
    assert_eq!(
        server
            .prepare_master_init_indication(7, qcom_ipa_service::QMI_MESSAGE_BYTES)
            .unwrap(),
        None,
        "the modem still must register for indications"
    );

    let registration = ipa_server_request(10, 0x0020, &[]);
    let reply = server
        .prepare_response(7, peer, &registration, qcom_ipa_service::QMI_MESSAGE_BYTES)
        .unwrap();
    server.commit_response(7, &reply).unwrap();
    let indication = server
        .prepare_master_init_indication(7, qcom_ipa_service::QMI_MESSAGE_BYTES)
        .unwrap()
        .unwrap();
    let decoded = Message::decode(indication.bytes()).unwrap();
    assert_eq!(decoded.kind, Kind::Indication);
    assert_eq!(decoded.transaction, 0);
    assert_eq!(decoded.message_id, 0x0022);
    assert_eq!(decoded.required(0x02).unwrap(), &[0, 0, 0, 0]);
    assert!(!server.indication_sent());
    server
        .commit_master_init_indication(7, &indication)
        .unwrap();
    assert!(server.indication_sent());
    assert_eq!(
        server
            .prepare_master_init_indication(7, qcom_ipa_service::QMI_MESSAGE_BYTES)
            .unwrap(),
        None
    );
}

#[test]
fn ipa_master_init_indication_waits_for_both_glink_and_qrtr_completion() {
    const IPA_SERVER_PORT: u32 = 0x4003;
    const MODEM_IPA_PORT: u32 = 0x27;
    const MODEM_IPA_INSTANCE: u32 = (2 << 8) | 1;
    let modem = qcom_qmi_router::Address {
        node: 0,
        port: MODEM_IPA_PORT,
    };
    let mut session = Session::new(config(), 1).unwrap();
    session
        .register(1, Service::IpaServer, IPA_SERVER_PORT)
        .unwrap();
    session
        .prepare_service_available(1, Service::IpaServer)
        .unwrap();
    session.start_control(1).unwrap();
    session
        .ingest(
            1,
            &intents(&[(7, 4096), (8, 4096), (9, 4096), (10, 4096), (11, 4096)]),
        )
        .unwrap();

    let hello_intent = offer(&mut session);
    assert!(matches!(
        session.ingest(1, &data(9, hello_intent, &qrtr_control(2, [0; 4]), 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))
    ));
    let (hello_event, _) = publish_until_data(&mut session);
    assert!(matches!(
        hello_event,
        Published::Data {
            remote_intent: 7,
            ..
        }
    ));
    session.ingest(1, &control(6, 7)).unwrap();

    let (service_event, service_frame) = publish_until_data(&mut session);
    assert!(matches!(
        service_event,
        Published::Data {
            remote_intent: 8,
            last_fragment: true
        }
    ));
    let service_datagram = &service_frame[16..16 + word(&service_frame, 8) as usize];
    assert_eq!(word(service_datagram, 36), qcom_ipa_service::SERVICE_ID);
    assert_eq!(
        word(service_datagram, 40),
        qcom_ipa_service::QRTR_VERSION_INSTANCE
    );
    assert_eq!(word(service_datagram, 48), IPA_SERVER_PORT);
    session.ingest(1, &control(6, 8)).unwrap();

    let server_intent = offer(&mut session);
    let remote_ipa = qrtr_control(
        4,
        [
            qcom_ipa_service::SERVICE_ID,
            MODEM_IPA_INSTANCE,
            0,
            MODEM_IPA_PORT,
        ],
    );
    assert!(matches!(
        session.ingest(1, &data(9, server_intent, &remote_ipa, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Server {
            added: true,
            ..
        }))
    ));

    let mut server = qcom_ipa_service::Server::new(modem, 1).unwrap();
    let registration = ipa_server_request(1, 0x0020, &[]);
    let prepared_reply = server
        .prepare_response(1, modem, &registration, qcom_ipa_service::QMI_MESSAGE_BYTES)
        .unwrap();
    server.commit_response(1, &prepared_reply).unwrap();
    let readiness = qcom_ipa_service::ApReadyPermit::test_after_post_init(1);
    server.note_ap_init_complete(1, &readiness).unwrap();
    let indication = server
        .prepare_master_init_indication(1, qcom_ipa_service::QMI_MESSAGE_BYTES)
        .unwrap()
        .unwrap();
    assert_eq!(indication.peer(), modem);

    session
        .queue_ipa_master_init_indication(1, &indication)
        .unwrap();
    let (indication_event, indication_frame) = publish_until_data(&mut session);
    let Published::Data { remote_intent, .. } = indication_event else {
        unreachable!();
    };
    let datagram = &indication_frame[16..16 + word(&indication_frame, 8) as usize];
    assert_eq!(word(datagram, 0), 1);
    assert_eq!(word(datagram, 4), 1);
    assert_eq!(word(datagram, 8), 1);
    assert_eq!(word(datagram, 12), IPA_SERVER_PORT);
    assert_eq!(word(datagram, 16), 1, "request QRTR RESUME_TX");
    assert_eq!(
        word(datagram, 20),
        qcom_ipa_service::QMI_MESSAGE_BYTES as u32
    );
    assert_eq!(word(datagram, 24), modem.node);
    assert_eq!(word(datagram, 28), modem.port);
    let decoded = Message::decode(&datagram[32..32 + qcom_ipa_service::QMI_MESSAGE_BYTES]).unwrap();
    assert_eq!(decoded.kind, Kind::Indication);
    assert_eq!(decoded.message_id, 0x0022);
    assert_eq!(decoded.required(0x02).unwrap(), &[0, 0, 0, 0]);

    assert_eq!(session.take_ipa_indication_completion(1).unwrap(), None);
    session.ingest(1, &control(6, remote_intent)).unwrap();
    assert_eq!(session.take_ipa_indication_completion(1).unwrap(), None);

    let credit_intent = offer(&mut session);
    let resume = resume_route(MODEM_IPA_PORT, IPA_SERVER_PORT);
    assert_eq!(
        session.ingest(1, &data(9, credit_intent, &resume, 0)),
        Ok(Event::CreditReceived)
    );
    let token = session
        .take_ipa_indication_completion(1)
        .unwrap()
        .expect("both transport acknowledgements complete the indication");
    assert_eq!(token, indication.token());
    server
        .commit_master_init_indication(1, &indication)
        .unwrap();
    assert!(server.indication_sent());
}

#[test]
fn ipa_ap_service_rejects_bad_peer_boolean_and_stale_response_without_advancing() {
    let peer = qcom_qmi_router::Address { node: 0, port: 27 };
    let mut server = qcom_ipa_service::Server::new(peer, 11).unwrap();
    let bad_boolean = ipa_server_request(1, 0x0020, &[(0x10, &[2])]);
    assert_eq!(
        server.prepare_response(11, peer, &bad_boolean, 14),
        Err(qcom_ipa_service::Error::InvalidBoolean(2))
    );
    let completion = ipa_server_request(2, 0x0035, &[(0x01, &[1])]);
    assert_eq!(
        server.prepare_response(
            11,
            qcom_qmi_router::Address { node: 0, port: 28 },
            &completion,
            14,
        ),
        Err(qcom_ipa_service::Error::WrongPeer)
    );
    let reply = server.prepare_response(11, peer, &completion, 14).unwrap();
    assert_eq!(
        server.commit_response(12, &reply),
        Err(qcom_ipa_service::Error::StaleGeneration)
    );
    assert_eq!(server.modem_init_status(), None);
    server.commit_response(11, &reply).unwrap();
    assert_eq!(server.modem_init_status(), Some(1));
}

#[test]
fn ipa_ap_service_quarantines_uncertain_response_publication() {
    let peer = qcom_qmi_router::Address { node: 0, port: 27 };
    let mut server = qcom_ipa_service::Server::new(peer, 13).unwrap();
    let request = ipa_server_request(4, 0x0035, &[(0x01, &[0])]);
    let reply = server.prepare_response(13, peer, &request, 14).unwrap();
    server.mark_response_uncertain(13, &reply).unwrap();
    assert_eq!(
        server.commit_response(13, &reply),
        Err(qcom_ipa_service::Error::Indeterminate)
    );
    assert_eq!(server.modem_init_status(), None);
}

#[test]
fn ipa_ap_service_response_commits_only_after_glink_and_qrtr_completion() {
    const IPA_SERVER_PORT: u32 = 0x4003;
    let mut session = session();
    session
        .register(1, Service::IpaServer, IPA_SERVER_PORT)
        .unwrap();
    session
        .prepare_service_available(1, Service::IpaServer)
        .unwrap();
    session.start_control(1).unwrap();
    session
        .ingest(
            1,
            &intents(&[(7, 4096), (8, 4096), (9, 4096), (10, 4096), (11, 4096)]),
        )
        .unwrap();

    let hello_intent = offer(&mut session);
    assert!(matches!(
        session.ingest(1, &data(9, hello_intent, &qrtr_control(2, [0; 4]), 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))
    ));
    let (hello_event, _) = publish_until_data(&mut session);
    assert!(matches!(
        hello_event,
        Published::Data {
            remote_intent: 7,
            ..
        }
    ));
    session.ingest(1, &control(6, 7)).unwrap();

    let (service_event, service_frame) = publish_until_data(&mut session);
    assert!(matches!(
        service_event,
        Published::Data {
            remote_intent: 8,
            last_fragment: true
        }
    ));
    let service_datagram = &service_frame[16..16 + word(&service_frame, 8) as usize];
    assert_eq!(word(service_datagram, 4), 4);
    assert_eq!(word(service_datagram, 36), qcom_ipa_service::SERVICE_ID);
    assert_eq!(
        word(service_datagram, 40),
        qcom_ipa_service::QRTR_VERSION_INSTANCE
    );
    assert_eq!(word(service_datagram, 44), 1);
    assert_eq!(word(service_datagram, 48), IPA_SERVER_PORT);
    session.ingest(1, &control(6, 8)).unwrap();

    // Bind the service handler to the exact modem IPA server announcement.
    let announce_intent = offer(&mut session);
    let modem_ipa = qrtr_control(4, [0x31, 0x201, 0, 27]);
    assert!(matches!(
        session.ingest(1, &data(9, announce_intent, &modem_ipa, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Server {
            added: true,
            ..
        }))
    ));

    let qmi_request = ipa_server_request(0x1234, 0x0020, &[(0x10, &[1]), (0x12, &[0])]);
    let request_datagram = qrtr(1, 27, IPA_SERVER_PORT, true, &qmi_request);
    let request_intent = offer(&mut session);
    let request_handle = receive(&mut session, request_intent, &request_datagram);
    let (context, decoded) = session.request(&request_handle).unwrap();
    assert_eq!(context.service, Service::IpaServer);
    assert_eq!(
        decoded,
        Request::IpaServer {
            message_id: qcom_ipa_service::INDICATION_REGISTER_MESSAGE_ID,
        }
    );

    let mut server = qcom_ipa_service::Server::new(context.peer, 1).unwrap();
    let prepared = server
        .prepare_response(
            1,
            context.peer,
            &qmi_request,
            qcom_ipa_service::QMI_MESSAGE_BYTES,
        )
        .unwrap();
    assert_eq!(server.registration(), None);
    session
        .queue_ipa_service_reply(1, &request_handle, &prepared)
        .unwrap();
    assert_eq!(session.take_ipa_service_response_completion(1), Ok(None));

    let (credit_event, credit_frame) = publish_until_data(&mut session);
    assert!(matches!(
        credit_event,
        Published::Data {
            remote_intent: 9,
            last_fragment: true
        }
    ));
    let credit_datagram = &credit_frame[16..16 + word(&credit_frame, 8) as usize];
    assert_eq!(word(credit_datagram, 4), 7);
    session.ingest(1, &control(6, 9)).unwrap();

    let (reply_event, reply_frame) = publish_until_data(&mut session);
    assert!(matches!(
        reply_event,
        Published::Data {
            remote_intent: 10,
            last_fragment: true
        }
    ));
    let reply_datagram = &reply_frame[16..16 + word(&reply_frame, 8) as usize];
    assert_eq!(word(reply_datagram, 8), 1);
    assert_eq!(word(reply_datagram, 12), IPA_SERVER_PORT);
    assert_eq!(word(reply_datagram, 16), 1);
    assert_eq!(word(reply_datagram, 24), context.peer.node);
    assert_eq!(word(reply_datagram, 28), context.peer.port);
    let reply_bytes = &reply_datagram[32..32 + word(reply_datagram, 20) as usize];
    assert_eq!(reply_bytes, prepared.bytes());

    // GLINK RX_DONE alone does not commit the QMI server state.
    session.ingest(1, &control(6, 10)).unwrap();
    assert_eq!(session.take_ipa_service_response_completion(1), Ok(None));
    assert_eq!(server.registration(), None);

    // The matching QRTR RESUME_TX completes the second transport half.
    let credit_intent = offer(&mut session);
    let resume = resume_route(27, IPA_SERVER_PORT);
    assert_eq!(
        session.ingest(1, &data(9, credit_intent, &resume, 0)),
        Ok(Event::ReplyQrtrResume(request_handle))
    );
    assert_eq!(
        session.take_ipa_service_response_completion(1),
        Ok(Some(prepared.token()))
    );
    server.commit_response(1, &prepared).unwrap();
    assert_eq!(
        server.registration(),
        Some(qcom_ipa_service::Registration {
            master_driver_init_complete: Some(true),
            data_usage_quota_reached: None,
            mhi_ready: Some(false),
            endpoint_desc: None,
            bandwidth_change: None,
        })
    );

    // Exercise the opposite acknowledgement order with the modem's
    // INIT_MODEM_DRIVER_COMPLETE request: QRTR RESUME_TX may arrive before
    // GLINK RX_DONE, but the server state still waits for both.
    let completion_request = ipa_server_request(0x2345, 0x0035, &[(0x01, &[1])]);
    let completion_datagram = qrtr(1, 27, IPA_SERVER_PORT, true, &completion_request);
    let completion_intent = offer(&mut session);
    let completion_handle = receive(&mut session, completion_intent, &completion_datagram);
    let (completion_context, raw_completion) = session.request_qmi(&completion_handle).unwrap();
    assert_eq!(raw_completion, completion_request);
    let prepared_completion = server
        .prepare_response(
            1,
            completion_context.peer,
            raw_completion,
            qcom_ipa_service::QMI_MESSAGE_BYTES,
        )
        .unwrap();
    session
        .queue_ipa_service_reply(1, &completion_handle, &prepared_completion)
        .unwrap();
    let (completion_credit, completion_credit_frame) = publish_until_data(&mut session);
    let Published::Data {
        remote_intent: completion_credit_intent,
        ..
    } = completion_credit
    else {
        unreachable!();
    };
    assert_eq!(word(&completion_credit_frame[16..], 4), 7);
    session
        .ingest(1, &control(6, completion_credit_intent))
        .unwrap();
    session.ingest(1, &intents(&[(12, 4096)])).unwrap();
    let (completion_event, completion_frame) = publish_until_data(&mut session);
    assert!(matches!(
        completion_event,
        Published::Data {
            remote_intent: 12,
            ..
        }
    ));
    let completion_datagram = &completion_frame[16..16 + word(&completion_frame, 8) as usize];
    assert_eq!(word(completion_datagram, 4), 1);
    assert_eq!(word(completion_datagram, 12), IPA_SERVER_PORT);
    assert_eq!(word(completion_datagram, 16), 1);

    let credit_intent = offer(&mut session);
    let resume = resume_route(27, IPA_SERVER_PORT);
    assert_eq!(
        session.ingest(1, &data(9, credit_intent, &resume, 0)),
        Ok(Event::ReplyQrtrResume(completion_handle))
    );
    assert_eq!(session.take_ipa_service_response_completion(1), Ok(None));
    assert_eq!(server.modem_init_status(), None);
    session.ingest(1, &control(6, 12)).unwrap();
    assert_eq!(
        session.take_ipa_service_response_completion(1),
        Ok(Some(prepared_completion.token()))
    );
    server.commit_response(1, &prepared_completion).unwrap();
    assert_eq!(server.modem_init_status(), Some(1));
}
fn servreg_trace(
    transaction: u16,
    message_id: u16,
    request: &[u8],
    response: &[u8],
) -> qcom_servreg_notif_client::WireTrace {
    let mut trace = qcom_servreg_notif_client::WireTrace {
        source: qcom_qmi_router::Address {
            node: 1,
            port: 0x4002,
        },
        destination: qcom_qmi_router::Address { node: 0, port: 3 },
        transaction,
        message_id,
        request: [0; qcom_servreg_notif_client::MAX_TRACE_BYTES],
        request_len: request.len() as u8,
        response: [0; qcom_servreg_notif_client::MAX_TRACE_BYTES],
        response_len: response.len() as u8,
    };
    trace.request[..request.len()].copy_from_slice(request);
    trace.response[..response.len()].copy_from_slice(response);
    trace
}
fn query(txn: u16, confirm: bool) -> Vec<u8> {
    let mut qmi = vec![0];
    qmi.extend(txn.to_le_bytes());
    qmi.extend([0x24, 0, 7, 0, 1, 4, 0, 1, 0, 0, 0]);
    qrtr(1, 9, MEMORY, confirm, &qmi)
}

#[test]
fn ipa_init_client_uses_discovered_server_and_one_explicit_live_profile() {
    const IPA_PORT: u32 = 0x4003;
    const IPA_SERVER_PORT: u32 = 3;

    let fields = sunfish_ipa_profile::init_request_fields(sunfish_ipa_profile::RuntimeSnapshot {
        shared_mem_baddr_units: 0x10,
        shared_mem_size_units: 0x400,
        apps_wan_cons_endpoint: Some(sunfish_ipa_profile::APPS_WAN_CONS_ENDPOINT),
        ipa_uc_loaded: false,
    })
    .unwrap();

    let mut session = session();
    session.enable_ipa_client(1, IPA_PORT).unwrap();
    assert_eq!(session.ipa_state(), Some(qcom_ipa_client::State::Searching));
    assert_eq!(
        session.begin_ipa_init_modem_driver(1, &fields),
        Err(Error::Control(qcom_qrtr_control::Error::PeerNotReady))
    );
    session.start_control(1).unwrap();
    session
        .ingest(
            1,
            &intents(&[
                (7, 4096),
                (8, 4096),
                (9, 4096),
                (10, 4096),
                (11, 4096),
                (12, 4096),
                (13, 4096),
            ]),
        )
        .unwrap();

    let hello_intent = offer(&mut session);
    assert!(matches!(
        session.ingest(1, &data(9, hello_intent, &qrtr_control(2, [0; 4]), 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))
    ));
    let (hello_event, _) = publish_until_data(&mut session);
    assert!(matches!(
        hello_event,
        Published::Data {
            remote_intent: 7,
            ..
        }
    ));
    session.ingest(1, &control(6, 7)).unwrap();

    assert_eq!(
        session.begin_ipa_init_modem_driver(1, &fields),
        Err(Error::IpaClient(qcom_ipa_client::Error::NoEndpoint))
    );

    let server_intent = offer(&mut session);
    let ipa_server = qrtr_control(
        4,
        [
            qcom_ipa::QRTR_SERVICE_ID,
            qcom_ipa::QRTR_VERSION_INSTANCE,
            0,
            IPA_SERVER_PORT,
        ],
    );
    assert!(matches!(
        session.ingest(1, &data(9, server_intent, &ipa_server, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Server {
            added: true,
            ..
        }))
    ));
    assert_eq!(session.ipa_state(), Some(qcom_ipa_client::State::Ready));

    session.begin_ipa_init_modem_driver(1, &fields).unwrap();
    let (request_event, request_frame) = publish_until_data(&mut session);
    assert!(matches!(
        request_event,
        Published::Data {
            remote_intent: 8,
            last_fragment: true
        }
    ));
    let request_len = word(&request_frame, 8) as usize;
    let request = &request_frame[16..16 + request_len];
    assert_eq!(word(request, 0), 1);
    assert_eq!(word(request, 4), 1);
    assert_eq!(word(request, 8), 1);
    assert_eq!(word(request, 12), IPA_PORT);
    assert_eq!(word(request, 16), 0);
    assert_eq!(word(request, 20), 118);
    assert_eq!(word(request, 24), 0);
    assert_eq!(word(request, 28), IPA_SERVER_PORT);
    let qmi = Message::decode(&request[32..32 + word(request, 20) as usize]).unwrap();
    assert_eq!(qmi.kind, Kind::Request);
    assert_eq!(qmi.transaction, 1);
    assert_eq!(qmi.message_id, qcom_ipa::INIT_MODEM_DRIVER_MESSAGE_ID);
    session.ingest(1, &control(6, 8)).unwrap();

    let response_intent = offer(&mut session);
    let payload = ipa_init_response(1, 0, 0);
    let mut response = qrtr(1, IPA_SERVER_PORT, IPA_PORT, true, &payload);
    response[32 + payload.len()..].fill(0);
    assert_eq!(
        session.ingest(1, &data(9, response_intent, &response, 0)),
        Ok(Event::IpaInitModemDriver(
            qcom_ipa_client::ResponseOutcome {
                response: qcom_ipa::InitResponse {
                    result: 0,
                    error: 0,
                    control_endpoint: None,
                    default_endpoint: None,
                    modem_driver_init_pending: None,
                },
                confirm_rx: true,
            }
        ))
    );
    assert_eq!(
        session.ipa_state(),
        Some(qcom_ipa_client::State::ResponseReceived)
    );
    let (resume_event, resume_frame) = publish_until_data(&mut session);
    assert!(matches!(resume_event, Published::Data { .. }));
    assert_eq!(word(&resume_frame, 20), 7);
    assert_eq!(
        session.begin_ipa_init_modem_driver(1, &fields),
        Err(Error::IpaClient(qcom_ipa_client::Error::AlreadyRequested))
    );
}

#[test]
fn servreg_notif_query_runs_through_qrtr_and_glink_and_returns_wlan_fw_state() {
    let mut session = session();
    session.enable_servreg_notif_client(1, 0x4002).unwrap();
    session.start_control(1).unwrap();
    session
        .ingest(
            1,
            &intents(&[
                (7, 4096),
                (8, 4096),
                (9, 4096),
                (10, 4096),
                (11, 4096),
                (12, 4096),
                (13, 4096),
                (14, 4096),
                (15, 4096),
            ]),
        )
        .unwrap();

    let hello_intent = offer(&mut session);
    let hello = qrtr_control(2, [0; 4]);
    assert!(matches!(
        session.ingest(1, &data(9, hello_intent, &hello, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))
    ));
    let (hello_event, hello_frame) = publish_until_data(&mut session);
    assert!(matches!(
        hello_event,
        Published::Data {
            remote_intent: 7,
            ..
        }
    ));
    assert_eq!(word(&hello_frame, 20), 2);
    session.ingest(1, &control(6, 7)).unwrap();

    let server_intent = offer(&mut session);
    let server = qrtr_control(4, [0x42, 0xb401, 0, 3]);
    assert!(matches!(
        session.ingest(1, &data(9, server_intent, &server, 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Server {
            added: true,
            ..
        }))
    ));
    assert_eq!(
        session.servreg_notif_state(),
        Some(qcom_servreg_notif_client::State::Ready)
    );
    session.begin_servreg_notif_query(1).unwrap();

    let (query_event, query_frame) = publish_until_data(&mut session);
    assert!(matches!(
        query_event,
        Published::Data {
            remote_intent: 8,
            last_fragment: true
        }
    ));
    let query_datagram = &query_frame[16..16 + word(&query_frame, 8) as usize];
    assert_eq!(word(query_datagram, 4), 1);
    assert_eq!(word(query_datagram, 20), 27);
    assert_eq!(word(query_datagram, 24), 0);
    assert_eq!(word(query_datagram, 28), 3);
    assert_eq!(
        &query_datagram[32..59],
        b"\x00\x01\x00\x21\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd"
    );
    session.ingest(1, &control(6, 8)).unwrap();

    let response_intent = offer(&mut session);
    let payload = servreg_state_response(1, 0x7fff_ffff);
    let mut response = qrtr(1, 3, 0x4002, true, &payload);
    response[53..56].fill(0); // This client requires canonical zero QRTR padding.
    assert_eq!(
        session.ingest(1, &data(9, response_intent, &response, 0)),
        Ok(Event::ServregNotifState(
            qcom_servreg_notif_client::Outcome {
                query: Some(qcom_servreg_notif::QueryStateResponse {
                    result: 0,
                    error: 0,
                    state: Some(qcom_servreg_notif::ServiceState::Uninit),
                }),
                listener: None,
                restart: None,
                state_update: None,
                set_ack: None,
                confirm_rx: true,
                trace: servreg_trace(
                    1,
                    qcom_servreg_notif::QUERY_STATE_MESSAGE_ID,
                    b"\x00\x01\x00\x21\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd",
                    &payload,
                ),
            }
        ))
    );
    let (resume_event, resume_frame) = publish_until_data(&mut session);
    assert!(matches!(
        resume_event,
        Published::Data {
            remote_intent: 9,
            last_fragment: true
        }
    ));
    assert_eq!(word(&resume_frame, 20), 7);
    session.ingest(1, &control(6, 9)).unwrap();
    assert_eq!(
        session.servreg_notif_state(),
        Some(qcom_servreg_notif_client::State::Complete)
    );
    assert_eq!(
        session.servreg_notif_outcome().unwrap().current_state(),
        Some(qcom_servreg_notif::ServiceState::Uninit)
    );

    session.begin_servreg_notif_listener(1).unwrap();
    let (listener_event, listener_frame) = publish_until_data(&mut session);
    assert!(matches!(
        listener_event,
        Published::Data {
            remote_intent: 10,
            last_fragment: true
        }
    ));
    let listener_datagram = &listener_frame[16..16 + word(&listener_frame, 8) as usize];
    assert_eq!(word(listener_datagram, 20), 31);
    assert_eq!(
        &listener_datagram[32..63],
        b"\x00\x02\x00\x20\x00\x18\x00\x01\x01\x00\x01\x02\x11\x00msm/modem/wlan_pd"
    );
    session.ingest(1, &control(6, 10)).unwrap();

    let listener_response_intent = offer(&mut session);
    let payload = servreg_listener_response(2, 0x7fff_ffff);
    let mut response = qrtr(1, 3, 0x4002, true, &payload);
    response[53..56].fill(0);
    assert_eq!(
        session.ingest(1, &data(9, listener_response_intent, &response, 0)),
        Ok(Event::ServregNotifState(
            qcom_servreg_notif_client::Outcome {
                query: Some(qcom_servreg_notif::QueryStateResponse {
                    result: 0,
                    error: 0,
                    state: Some(qcom_servreg_notif::ServiceState::Uninit),
                }),
                listener: Some(qcom_servreg_notif::RegisterListenerResponse {
                    result: 0,
                    error: 0,
                    state: Some(qcom_servreg_notif::ServiceState::Uninit),
                }),
                restart: None,
                state_update: None,
                set_ack: None,
                confirm_rx: true,
                trace: servreg_trace(
                    2,
                    qcom_servreg_notif::REGISTER_LISTENER_MESSAGE_ID,
                    b"\x00\x02\x00\x20\x00\x18\x00\x01\x01\x00\x01\x02\x11\x00msm/modem/wlan_pd",
                    &payload,
                ),
            }
        ))
    );
    let (listener_resume_event, listener_resume_frame) = publish_until_data(&mut session);
    assert!(matches!(
        listener_resume_event,
        Published::Data {
            remote_intent: 11,
            last_fragment: true
        }
    ));
    assert_eq!(word(&listener_resume_frame, 20), 7);
    session.ingest(1, &control(6, 11)).unwrap();
    assert_eq!(
        session.servreg_notif_state(),
        Some(qcom_servreg_notif_client::State::ListenerComplete)
    );

    let payload = servreg_state_updated_indication(0x1234, 0x5678, 0x7fff_ffff);
    let mut indication = qrtr(1, 3, 0x4002, true, &payload);
    indication[32 + payload.len()..].fill(0);
    let indication_intent = offer(&mut session);
    let update = match session.ingest(1, &data(9, indication_intent, &indication, 0)) {
        Ok(Event::ServregNotifState(outcome)) => {
            assert_eq!(
                outcome.current_state(),
                Some(qcom_servreg_notif::ServiceState::Uninit)
            );
            assert_eq!(
                outcome.state_update,
                Some(qcom_servreg_notif_client::StateUpdate {
                    qmi_transaction: 0x1234,
                    state: qcom_servreg_notif::ServiceState::Uninit,
                    indication_transaction: 0x5678,
                    confirm_rx: true,
                })
            );
            assert_eq!(
                outcome.trace.message_id,
                qcom_servreg_notif::STATE_UPDATED_INDICATION_MESSAGE_ID
            );
            outcome
        }
        event => panic!("expected SERVREG state update, got {event:?}"),
    };
    assert_eq!(
        session.servreg_notif_state(),
        Some(qcom_servreg_notif_client::State::AckPending)
    );
    assert_eq!(update.set_ack, None);

    let (indication_resume_event, indication_resume_frame) = publish_until_data(&mut session);
    assert!(matches!(
        indication_resume_event,
        Published::Data {
            remote_intent: 12,
            last_fragment: true
        }
    ));
    let resume_datagram =
        &indication_resume_frame[16..16 + word(&indication_resume_frame, 8) as usize];
    assert_eq!(word(resume_datagram, 4), 7);
    session.ingest(1, &control(6, 12)).unwrap();

    let (ack_event, ack_frame) = publish_until_data(&mut session);
    assert!(matches!(
        ack_event,
        Published::Data {
            remote_intent: 13,
            last_fragment: true
        }
    ));
    let ack_datagram = &ack_frame[16..16 + word(&ack_frame, 8) as usize];
    assert_eq!(
        word(ack_datagram, 20),
        qcom_servreg_notif::WLAN_PD_SET_ACK_REQUEST_BYTES as u32
    );
    let ack_message = Message::decode(&ack_datagram[32..]).unwrap();
    assert_eq!(ack_message.kind, Kind::Request);
    assert_eq!(ack_message.transaction, 3);
    assert_eq!(
        ack_message.message_id,
        qcom_servreg_notif::SET_ACK_MESSAGE_ID
    );
    assert_eq!(ack_message.required(0x01).unwrap(), b"msm/modem/wlan_pd");
    assert_eq!(
        ack_message.required(0x02).unwrap(),
        &0x5678u16.to_le_bytes()
    );
    session.ingest(1, &control(6, 13)).unwrap();

    let ack_response_intent = offer(&mut session);
    let payload = servreg_set_ack_response(3);
    let mut response = qrtr(1, 3, 0x4002, false, &payload);
    response[46..48].fill(0);
    let outcome = match session.ingest(1, &data(9, ack_response_intent, &response, 0)) {
        Ok(Event::ServregNotifState(outcome)) => outcome,
        event => panic!("expected SET_ACK response, got {event:?}"),
    };
    assert!(outcome.state_update_acknowledged());
    assert_eq!(
        session.servreg_notif_state(),
        Some(qcom_servreg_notif_client::State::ListenerComplete)
    );

    session.begin_servreg_notif_restart_pd(1).unwrap();
    let (restart_event, restart_frame) = publish_until_data(&mut session);
    assert!(matches!(
        restart_event,
        Published::Data {
            remote_intent: 14,
            last_fragment: true
        }
    ));
    let restart_datagram = &restart_frame[16..16 + word(&restart_frame, 8) as usize];
    assert_eq!(word(restart_datagram, 20), 27);
    assert_eq!(
        &restart_datagram[32..59],
        b"\x00\x04\x00\x24\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd"
    );
    session.ingest(1, &control(6, 14)).unwrap();

    let restart_response_intent = offer(&mut session);
    let payload = servreg_restart_response(4);
    let mut response = qrtr(1, 3, 0x4002, true, &payload);
    response[46..48].fill(0); // This client requires canonical zero QRTR padding.
    assert_eq!(
        session.ingest(1, &data(9, restart_response_intent, &response, 0)),
        Ok(Event::ServregNotifState(
            qcom_servreg_notif_client::Outcome {
                query: Some(qcom_servreg_notif::QueryStateResponse {
                    result: 0,
                    error: 0,
                    state: Some(qcom_servreg_notif::ServiceState::Uninit),
                }),
                listener: Some(qcom_servreg_notif::RegisterListenerResponse {
                    result: 0,
                    error: 0,
                    state: Some(qcom_servreg_notif::ServiceState::Uninit),
                }),
                restart: Some(qcom_servreg_notif::RestartPdResponse {
                    result: 0,
                    error: 0
                }),
                state_update: Some(qcom_servreg_notif_client::StateUpdate {
                    qmi_transaction: 0x1234,
                    state: qcom_servreg_notif::ServiceState::Uninit,
                    indication_transaction: 0x5678,
                    confirm_rx: true,
                }),
                set_ack: Some(qcom_servreg_notif::SetAckResponse {
                    result: 0,
                    error: 0,
                }),
                confirm_rx: true,
                trace: servreg_trace(
                    4,
                    qcom_servreg_notif::RESTART_PD_MESSAGE_ID,
                    b"\x00\x04\x00\x24\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd",
                    &payload,
                ),
            }
        ))
    );
    let (restart_resume_event, restart_resume_frame) = publish_until_data(&mut session);
    assert!(matches!(
        restart_resume_event,
        Published::Data {
            remote_intent: 15,
            last_fragment: true
        }
    ));
    assert_eq!(word(&restart_resume_frame, 20), 7);
    session.ingest(1, &control(6, 15)).unwrap();
    assert_eq!(
        session.servreg_notif_state(),
        Some(qcom_servreg_notif_client::State::RestartComplete)
    );
}
fn open(txn: u16) -> Vec<u8> {
    let mut qmi = vec![0];
    qmi.extend(txn.to_le_bytes());
    qmi.extend([1, 0, 18, 0, 1, 15, 0]);
    qmi.extend(b"/boot/modem_fs1");
    qrtr(1, 9, STORAGE, false, &qmi)
}
fn receive(session: &mut Session, id: u32, bytes: &[u8]) -> RequestHandle {
    match session.ingest(1, &data(9, id, bytes, 0)).unwrap() {
        Event::Request(handle) => handle,
        event => panic!("expected request, got {event:?}"),
    }
}
fn request(session: &mut Session, txn: u16) -> RequestHandle {
    let id = offer(session);
    receive(session, id, &query(txn, false))
}
fn resume(local_port: u32) -> Vec<u8> {
    resume_route(9, local_port)
}
fn resume_route(peer_port: u32, local_port: u32) -> Vec<u8> {
    let mut body = vec![0; 20];
    put(&mut body, 0, 7);
    put(&mut body, 4, 0);
    put(&mut body, 8, peer_port);
    qrtr(7, peer_port, local_port, false, &body)
}
fn credit(session: &mut Session, local_port: u32) -> u32 {
    let id = offer(session);
    assert!(matches!(
        session.ingest(1, &data(9, id, &resume(local_port), 0)),
        Ok(Event::CreditReceived | Event::ReplyQrtrResume(_))
    ));
    id
}
fn queued_reply(session: &mut Session) -> RequestHandle {
    let handle = request(session, 0x1234);
    session.ingest(1, &intents(&[(7, 4096)])).unwrap();
    session
        .queue_reply(&handle, Reply::Failure { error: 3 })
        .unwrap();
    assert!(matches!(
        publish(session).unwrap().0,
        Published::ReceiveDone(_)
    ));
    handle
}

#[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
#[test]
fn experimental_rmtfs_flow_uses_linux_watermarks_and_waits_for_glink() {
    let mut s = session();
    for count in 1..=10u16 {
        let id = offer(&mut s);
        let handle = receive(&mut s, id, &open(count));
        let remote = 100 + u32::from(count);
        s.ingest(1, &intents(&[(remote, 4096)])).unwrap();
        s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
        assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
        let (published, frame) = publish_until_data(&mut s);
        assert!(matches!(published, Published::Data { last_fragment: true, .. }));
        assert_eq!(word(&frame, 32), u32::from(count == 5));
        assert_eq!(s.in_flight(), 1, "buffer remains live until GLINK completion");
        assert_eq!(s.ingest(1, &control(6, remote)), Ok(Event::ReplyRxDone(handle)));
        assert_eq!(s.in_flight(), 0);
    }
    let id = offer(&mut s);
    let handle = receive(&mut s, id, &open(11));
    assert_eq!(s.queue_reply(&handle, Reply::Failure { error: 3 }), Err(Error::FlowBusy));
    // A flow resume replenishes credit; it is not a receipt for the blocked reply.
    let credit_id = offer(&mut s);
    assert_eq!(s.ingest(1, &data(9, credit_id, &resume(STORAGE), 0)), Ok(Event::CreditReceived));
    assert!(s.queue_reply(&handle, Reply::Failure { error: 3 }).is_ok());
}

#[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
#[test]
fn experimental_rmtfs_would_block_retries_same_packet_but_uncertain_stops() {
    fn setup() -> (Session, RequestHandle) {
        let mut session = session();
        let id = offer(&mut session);
        let handle = receive(&mut session, id, &open(1));
        session.ingest(1, &intents(&[(7, 4096)])).unwrap();
        session.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
        assert!(matches!(publish(&mut session).unwrap().0, Published::ReceiveDone(_)));
        (session, handle)
    }
    let (mut s, handle) = setup();
    let mut blocked = Vec::new();
    assert_eq!(s.publish_next(1, |bytes| { blocked.extend(bytes); Err(WriteError::WouldBlock) }), Err(Error::WouldBlock));
    assert_eq!(s.in_flight(), 1);
    let (_, written) = publish(&mut s).unwrap();
    assert_eq!(written, blocked);
    assert_eq!(word(&written, 32), 0);
    assert_eq!(s.ingest(1, &control(6, 7)), Ok(Event::ReplyRxDone(handle)));
    assert_eq!(s.in_flight(), 0);

    let (mut s, _) = setup();
    assert_eq!(s.publish_next(1, |_| Err(WriteError::Uncertain)), Err(Error::PublicationUncertain));
    assert_eq!(s.publish_next(1, |_| Ok(())), Err(Error::Quarantined));
}

#[test]
fn closed_rmtfs_client_retires_fully_published_glink_completed_reply() {
    let mut s = session();
    s.prepare_service_available(1, Service::RemoteStorage).unwrap();
    s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))));
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 7, .. }));
    s.ingest(1, &control(6, 7)).unwrap();
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 8, .. }));
    s.ingest(1, &control(6, 8)).unwrap();
    let intent = offer(&mut s);
    let handle = receive(&mut s, intent, &open(1));
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 9, last_fragment: true }));
    assert_eq!(s.ingest(1, &control(6, 9)), Ok(Event::ReplyRxDone(handle)));

    // No QRTR RESUME has arrived; the published reply still owns its slot.
    let intent = offer(&mut s);
    let close = qrtr_control(6, [0, 9, 0, 0]);
    assert!(matches!(s.ingest(1, &data(9, intent, &close, 0)),
        Ok(Event::QrtrControl(_))));
    assert_eq!(s.in_flight(), 0);
    assert!(s.client_gone_diagnostic().is_none());
    assert_eq!(s.retired_rmtfs_peer_count(), 1);
    let late = offer(&mut s);
    assert_eq!(s.ingest(1, &data(9, late, &resume(STORAGE), 0)), Ok(Event::CreditReceived));
    let reused = offer(&mut s);
    assert_eq!(s.ingest(1, &data(9, reused, &open(2), 0)), Err(Error::FlowBusy));
    assert_eq!(s.in_flight(), 0);
}
#[test]
fn closed_rmtfs_client_with_uncompleted_glink_reply_still_stops() {
    let mut s = session();
    s.prepare_service_available(1, Service::RemoteStorage).unwrap();
    s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))));
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 7, .. }));
    s.ingest(1, &control(6, 7)).unwrap();
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 8, .. }));
    s.ingest(1, &control(6, 8)).unwrap();
    let intent = offer(&mut s);
    let handle = receive(&mut s, intent, &open(1));
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 9, last_fragment: true }));
    // The peer has not returned GLINK RX_DONE.

    // No QRTR RESUME has arrived; the published reply still owns its slot.
    let intent = offer(&mut s);
    let close = qrtr_control(6, [0, 9, 0, 0]);
    assert_eq!(s.ingest(1, &data(9, intent, &close, 0)),
        Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
    let detail = s.client_gone_diagnostic().unwrap();
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    assert_eq!(s.deferred_close_diagnostic(), None, "routed close must not be reported as deferred");
    assert_eq!(detail.reply_sent, detail.reply_len);
    assert!(!detail.reply_glink_done);
    assert_eq!(s.in_flight(), 1);
}

#[test]
fn query_routes_through_fragment_assembly_and_private_borrow() {
    let mut s = session();
    let id = offer(&mut s);
    let bytes = query(0x1234, false);
    assert_eq!(
        s.ingest(1, &data(9, id, &bytes[..19], bytes.len() - 19)),
        Ok(Event::Fragment)
    );
    let handle = match s.ingest(1, &data(12, id, &bytes[19..], 0)).unwrap() {
        Event::Request(handle) => handle,
        _ => unreachable!(),
    };
    let (context, decoded) = s.request(&handle).unwrap();
    assert_eq!(context.transaction, 0x1234);
    assert_eq!(context.local.port, MEMORY);
    assert_eq!(
        decoded,
        Request::Memory(MemoryRequest::QuerySize {
            client: 1,
            processor: None
        })
    );
    assert_eq!(s.in_flight(), 1);
}

#[test]
fn storage_and_heap_are_routed_by_registered_endpoint() {
    let mut s = session();
    let id = offer(&mut s);
    let handle = receive(&mut s, id, &open(7));
    assert_eq!(
        s.request(&handle).unwrap().1,
        Request::Storage(StorageRequest::Open(Partition::ModemFs1))
    );
    assert_eq!(Service::RemoteStorage.descriptor(), (14, 1, 0));
    let heap = request(&mut s, 7);
    assert_eq!(s.request(&heap).unwrap().0.service, Service::HeapMemory);
}

#[test]
fn response_identity_result_and_padding_are_exact() {
    let mut s = session();
    let handle = queued_reply(&mut s);
    let (event, bytes) = publish(&mut s).unwrap();
    assert_eq!(
        event,
        Published::Data {
            remote_intent: 7,
            last_fragment: true
        }
    );
    assert_eq!(&bytes[..4], &[9, 0, 1, 0]);
    assert_eq!(word(&bytes, 8), 48);
    assert_eq!(word(&bytes, 12), 0);
    let qrtr = &bytes[16..64];
    assert_eq!((word(qrtr, 8), word(qrtr, 12)), (1, MEMORY));
    assert_eq!((word(qrtr, 24), word(qrtr, 28)), (0, 9));
    assert_eq!(word(qrtr, 16), 1);
    assert_eq!(word(qrtr, 20), 14);
    let message = Message::decode(&qrtr[32..46]).unwrap();
    assert_eq!(
        (message.kind, message.transaction, message.message_id),
        (Kind::Response, 0x1234, 0x24)
    );
    assert_eq!(message.required(2).unwrap(), &[1, 0, 3, 0]);
    assert_eq!(&qrtr[46..], &[0, 0]);
    assert_eq!(s.request(&handle), Err(Error::InvalidHandle));
}

#[test]
fn glink_completion_alone_does_not_release_transaction() {
    let mut s = session();
    let handle = queued_reply(&mut s);
    publish(&mut s).unwrap();
    assert_eq!(s.ingest(1, &control(6, 7)), Ok(Event::ReplyRxDone(handle)));
    assert_eq!(s.in_flight(), 1);
    let id = offer(&mut s);
    assert_eq!(
        s.ingest(1, &data(9, id, &query(0x1234, false), 0)),
        Err(Error::Routing(RouteError::DuplicateTransaction))
    );
    credit(&mut s, MEMORY);
    assert_eq!(s.in_flight(), 0);
    assert_eq!(
        s.queue_reply(&handle, Reply::Failure { error: 3 }),
        Err(Error::InvalidHandle)
    );
}

#[test]
fn qrtr_credit_alone_does_not_release_glink_buffer() {
    let mut s = session();
    queued_reply(&mut s);
    publish(&mut s).unwrap();
    credit(&mut s, MEMORY);
    assert_eq!(s.in_flight(), 1);
    s.ingest(1, &control(6, 7)).unwrap();
    assert_eq!(s.in_flight(), 0);
    assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::InvalidCompletion));
}

#[test]
fn confirm_rx_generates_separate_resume_packet_not_qmi_success() {
    let mut s = session();
    let id = offer(&mut s);
    let h = receive(&mut s, id, &query(2, true));
    assert!(s.request(&h).unwrap().0.confirm_rx);
    s.ingest(1, &intents(&[(0, 52)])).unwrap(); // Remote intent ID zero is legal.
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    let (_, bytes) = publish(&mut s).unwrap();
    let qrtr = &bytes[16..68];
    assert_eq!(word(qrtr, 4), 7);
    assert_eq!(word(qrtr, 16), 0);
    assert_eq!(
        (word(qrtr, 32), word(qrtr, 36), word(qrtr, 40)),
        (7, 1, MEMORY)
    );
    assert_eq!(s.in_flight(), 1);
    s.ingest(1, &control(6, 0)).unwrap();
    assert_eq!(s.in_flight(), 1);
}

#[test]
fn incoming_intent_is_unusable_until_advertisement_publishes() {
    let mut s = session();
    let id = s.offer_receive(1).unwrap();
    assert_eq!(
        s.ingest(1, &data(9, id, &query(1, false), 0)),
        Err(Error::FragmentOrder)
    );
    assert_eq!(
        s.publish_next(1, |_| Err(WriteError::WouldBlock)),
        Err(Error::WouldBlock)
    );
    assert_eq!(
        s.ingest(1, &data(9, id, &query(1, false), 0)),
        Err(Error::FragmentOrder)
    );
    publish(&mut s).unwrap();
    receive(&mut s, id, &query(1, false));
}

#[test]
fn one_shot_receive_ids_are_not_reused_after_done() {
    let mut s = session();
    queued_reply(&mut s);
    publish(&mut s).unwrap();
    let id = credit(&mut s, MEMORY);
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    s.ingest(1, &control(6, 7)).unwrap();
    let new = offer(&mut s);
    assert!(new > id);
    assert_eq!(
        s.ingest(1, &data(9, id, &query(9, false), 0)),
        Err(Error::InvalidIntent)
    );
}

#[test]
fn interleaved_fragments_stay_in_their_intent_buffers() {
    let mut s = session();
    let a = offer(&mut s);
    let b = offer(&mut s);
    let first = query(1, false);
    let second = open(2);
    s.ingest(1, &data(9, a, &first[..1], first.len() - 1))
        .unwrap();
    s.ingest(1, &data(9, b, &second[..11], second.len() - 11))
        .unwrap();
    let one = match s.ingest(1, &data(12, a, &first[1..], 0)).unwrap() {
        Event::Request(h) => h,
        _ => unreachable!(),
    };
    let two = match s.ingest(1, &data(12, b, &second[11..], 0)).unwrap() {
        Event::Request(h) => h,
        _ => unreachable!(),
    };
    assert_eq!(s.request(&one).unwrap().0.transaction, 1);
    assert_eq!(s.request(&two).unwrap().0.transaction, 2);
}

#[test]
fn malformed_fragment_is_rejected_without_advancing_assembly() {
    let mut s = session();
    let id = offer(&mut s);
    let bytes = query(1, false);
    assert_eq!(
        s.ingest(1, &data(12, id, &bytes, 0)),
        Err(Error::FragmentOrder)
    );
    s.ingest(1, &data(9, id, &bytes[..10], bytes.len() - 10))
        .unwrap();
    assert_eq!(
        s.ingest(1, &data(9, id, &bytes, 0)),
        Err(Error::FragmentOrder)
    );
    assert_eq!(
        s.ingest(1, &data(12, id, &bytes[10..], 1)),
        Err(Error::FragmentOrder)
    );
    assert!(matches!(
        s.ingest(1, &data(12, id, &bytes[10..], 0)),
        Ok(Event::Request(_))
    ));
}

#[test]
fn fragment_bounds_and_empty_chunks_are_enforced() {
    let mut s = session();
    let id = offer(&mut s);
    for (chunk, remaining) in [(0, 0), (1, 4096), (4097, 0)] {
        assert_eq!(
            s.ingest(1, &data(9, id, &vec![0; chunk], remaining)),
            Err(Error::FragmentLength)
        );
    }
    let mut record = data(9, id, &[1], 0);
    put(&mut record, 8, u32::MAX);
    assert_eq!(s.ingest(1, &record), Err(Error::FragmentLength));
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn fragment_count_is_bounded_independently_of_total_size() {
    let mut s = session();
    let id = offer(&mut s);
    let payload = vec![0; 65];
    for index in 0..64 {
        assert_eq!(
            s.ingest(
                1,
                &data(
                    if index == 0 { 9 } else { 12 },
                    id,
                    &payload[index..index + 1],
                    64 - index
                )
            ),
            Ok(Event::Fragment)
        );
    }
    assert_eq!(
        s.ingest(1, &data(12, id, &payload[64..], 0)),
        Err(Error::FragmentLimit)
    );
}

#[test]
fn qrtr_and_glink_padding_are_separate_and_exact() {
    let mut s = session();
    let id = offer(&mut s);
    let bytes = query(1, false);
    assert_eq!(bytes.len(), 48); // Declared QRTR length is 46.
    let mut record = data(9, id, &bytes, 0);
    record.push(0);
    assert_eq!(s.ingest(1, &record), Err(Error::RecordLength));
    let mut wrong = bytes.clone();
    wrong.truncate(46);
    assert_eq!(
        s.ingest(1, &data(9, id, &wrong, 0)),
        Err(Error::Wire(qcom_qmi::Error::LengthMismatch))
    );
    assert!(publish(&mut s).is_none()); // Held message sends no RX_DONE.
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn malformed_qmi_is_held_and_does_not_receive_done_or_reply() {
    let mut s = session();
    let id = offer(&mut s);
    let mut bytes = query(1, true);
    bytes[32] = 4;
    assert_eq!(
        s.ingest(1, &data(9, id, &bytes, 0)),
        Err(Error::Routing(RouteError::NotRequest))
    );
    assert!(publish(&mut s).is_none());
    assert_eq!(s.in_flight(), 0);
    assert_eq!(
        s.ingest(1, &data(9, id, &query(1, false), 0)),
        Err(Error::FragmentOrder)
    );
}

#[test]
fn remote_intent_batch_validation_is_atomic() {
    let mut s = session();
    assert_eq!(
        s.ingest(1, &intents(&[(2, 4096), (2, 4096)])),
        Err(Error::IntentAlreadyActive)
    );
    assert_eq!(
        s.ingest(1, &intents(&[(2, 4096), (3, 0)])),
        Err(Error::IntentCapacity)
    );
    assert_eq!(
        s.ingest(1, &intents(&[(3, 4096), (2, 4096)])),
        Ok(Event::RemoteIntents)
    );
    assert_eq!(
        s.ingest(1, &intents(&[(3, 4096)])),
        Err(Error::IntentAlreadyActive)
    );
    assert_eq!(
        s.ingest(1, &intents(&[(4, 4096); 33])),
        Err(Error::IntentCapacity)
    );
}

#[test]
#[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
fn missing_remote_capacity_applies_backpressure_without_consuming_request() {
    let mut s = session();
    let handle = request(&mut s, 1);
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    publish(&mut s).unwrap(); // RX_DONE.
    assert_eq!(
        s.publish_next(1, |_| panic!("must not publish")),
        Err(Error::NoRemoteIntent)
    );
    s.ingest(1, &intents(&[(1, 47)])).unwrap();
    assert_eq!(
        s.publish_next(1, |_| panic!("undersized intent")),
        Err(Error::NoRemoteIntent)
    );
    s.ingest(1, &intents(&[(2, 48)])).unwrap();
    assert_eq!(
        publish(&mut s).unwrap().0,
        Published::Data {
            remote_intent: 2,
            last_fragment: true
        }
    );
    assert_eq!(s.in_flight(), 1);
}

#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[test]
fn missing_intent_request_gets_a_turn_ahead_of_receive_completion() {
    let mut s = session();
    let handle = request(&mut s, 1);
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::RemoteIntentRequest { size: 48 }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)), "the deferred receive completion must still be published");
}

#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[test]
fn existing_suitable_intent_keeps_normal_receive_completion_priority() {
    let mut s = session();
    let handle = request(&mut s, 1);
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    s.ingest(1, &intents(&[(9, 48)])).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    assert_eq!(s.outbound_intent_attempt_diagnostic(), (0, 0));
    let (calls, pending, mask) = s.intent_gate_diagnostic();
    assert!(calls >= pending && pending > 0);
    assert_ne!(mask & 1, 0, "sendable transmission recorded");
    let (combinations, last) = s.intent_gate_combinations();
    assert_ne!(combinations & (1 << last), 0, "last combination was observed");
    assert_ne!(last & 1, 0, "last turn selected a sendable transmission");
    assert_eq!(s.intent_gate_queue_snapshot(), (0, 3, 1));
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { remote_intent: 9, .. }));
}

#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[test]
fn missing_remote_capacity_requests_one_intent_and_waits_for_grant_and_buffer() {
    for advertisement_first in [false, true] {
        let mut s = session();
        let handle = request(&mut s, 1);
        s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
        let (event, bytes) = publish(&mut s).unwrap();
        assert_eq!(event, Published::RemoteIntentRequest { size: 48 });
        assert_eq!(s.outbound_intent_diagnostic(), (1, 48));
        assert_eq!(&bytes[..], &[7, 0, 1, 0, 48, 0, 0, 0]);
        assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
        if advertisement_first {
            s.ingest(1, &intents(&[(9, 48)])).unwrap();
            assert_eq!(s.publish_next(1, |_| panic!("grant required")), Err(Error::NoRemoteIntent));
        }
        assert_eq!(s.ingest(1, &control(8, 1)), Ok(Event::RemoteIntentRequestAck { granted: true }));
        assert_eq!(s.outbound_intent_diagnostic(), (2, 48));
        if !advertisement_first {
            assert_eq!(s.publish_next(1, |_| panic!("buffer required")), Err(Error::NoRemoteIntent));
            s.ingest(1, &intents(&[(9, 48)])).unwrap();
        }
        assert!(matches!(publish(&mut s).unwrap().0, Published::Data { remote_intent: 9, .. }));
        assert_eq!(s.outbound_intent_diagnostic(), (4, 48));
        assert_eq!(s.in_flight(), 1, "buffer advertisement is not a GLINK completion");
    }
}

#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[test]
fn denied_intent_request_is_latched_and_never_retried() {
    let mut s = session(); let handle = request(&mut s, 1);
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    assert_eq!(s.ingest(1, &control(8, 1)), Err(Error::InvalidIntent));
    assert!(matches!(publish(&mut s).unwrap().0, Published::RemoteIntentRequest { .. }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    assert_eq!(s.ingest(1, &control(8, 2)), Err(Error::InvalidIntent));
    assert_eq!(s.ingest(1, &control(8, 0)), Ok(Event::RemoteIntentRequestAck { granted: false }));
    assert_eq!(s.outbound_intent_diagnostic(), (3, 48));
    assert_eq!(s.publish_next(1, |_| panic!("denial must not retry")), Err(Error::NoRemoteIntent));
    assert_eq!(s.ingest(1, &control(8, 1)), Err(Error::InvalidIntent));
}

#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[test]
fn intent_request_publication_obeys_would_block_and_uncertain_guards() {
    let mut s = session(); let handle = request(&mut s, 1);
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(s.outbound_intent_attempt_diagnostic(), (1, 1));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::Uncertain)), Err(Error::PublicationUncertain));
    assert_eq!(s.outbound_intent_attempt_diagnostic(), (2, 1));
    assert_eq!(s.publish_next(1, |_| panic!("uncertain writes cannot be retried")), Err(Error::Quarantined));
}

#[test]
fn failed_publication_is_retryable_with_identical_bytes() {
    let mut s = session();
    queued_reply(&mut s);
    let mut attempt = Vec::new();
    assert_eq!(
        s.publish_next(1, |bytes| {
            attempt.extend(bytes);
            Err(WriteError::WouldBlock)
        }),
        Err(Error::WouldBlock)
    );
    assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::InvalidCompletion));
    assert_eq!(publish(&mut s).unwrap().1, attempt);
}

#[test]
fn uncertain_publication_quarantines_all_future_work_until_transport_reset() {
    let mut s = session();
    let handle = queued_reply(&mut s);
    assert_eq!(
        s.publish_next(1, |_| Err(WriteError::Uncertain)),
        Err(Error::PublicationUncertain)
    );
    assert_eq!(
        s.publish_next(1, |_| panic!("quarantined")),
        Err(Error::Quarantined)
    );
    assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::Quarantined));
    assert_eq!(s.offer_receive(1), Err(Error::Quarantined));
    s.reset_after_transport_reset(2).unwrap();
    assert_eq!(s.in_flight(), 0);
    assert_eq!(
        s.queue_reply(&handle, Reply::Failure { error: 1 }),
        Err(Error::StaleGeneration)
    );
    assert_eq!(s.offer_receive(1), Err(Error::StaleGeneration));
    assert!(s.offer_receive(2).is_ok());
}

#[test]
fn reset_discards_pending_writes_and_registrations_and_stale_handles() {
    let mut s = session();
    let handle = queued_reply(&mut s);
    s.reset_after_transport_reset(2).unwrap();
    assert_eq!(
        s.publish_next(2, |_| panic!("old queue survived reset")),
        Ok(None)
    );
    assert_eq!(
        s.publish_next(1, |_| panic!("stale generation")),
        Err(Error::StaleGeneration)
    );
    assert_eq!(s.request(&handle), Err(Error::StaleGeneration));
    assert_eq!(
        s.reset_after_transport_reset(2),
        Err(Error::Routing(RouteError::InvalidGeneration))
    );
    assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::StaleGeneration));
    let id = s.offer_receive(2).unwrap();
    s.publish_next(2, |_| Ok(())).unwrap();
    assert_eq!(
        s.ingest(2, &data(9, id, &query(1, false), 0)),
        Err(Error::Routing(RouteError::NoEndpoint))
    );
}

#[test]
fn request_handle_cannot_cross_sessions() {
    let mut first = session();
    let handle = request(&mut first, 1);
    let mut second = session();
    request(&mut second, 1);
    assert_eq!(second.request(&handle), Err(Error::InvalidHandle));
    assert_eq!(
        second.queue_reply(&handle, Reply::Failure { error: 3 }),
        Err(Error::InvalidHandle)
    );
}

#[test]
fn completion_must_match_busy_intent_and_fully_published_message() {
    let mut s = session();
    queued_reply(&mut s);
    assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::InvalidCompletion));
    assert_eq!(
        s.last_completion_error().unwrap().failure,
        qcom_glink_qmi::CompletionFailure::NoBusyRemoteIntent
    );
    publish(&mut s).unwrap();
    assert_eq!(s.ingest(1, &control(6, 8)), Err(Error::InvalidCompletion));
    assert_eq!(
        s.last_completion_error().unwrap().intent_id,
        8,
        "unknown peer intent ID is retained for the native diagnostic"
    );
    assert_eq!(s.ingest(1, &control(14, 8)), Err(Error::InvalidCompletion));
    let invalid_reuse = s.last_completion_error().unwrap();
    assert_eq!(invalid_reuse.intent_id, 8);
    assert!(invalid_reuse.reuse);
    assert_eq!(
        invalid_reuse.failure,
        qcom_glink_qmi::CompletionFailure::NoBusyRemoteIntent
    );
    s.ingest(1, &control(14, 7)).unwrap();
    assert_eq!(
        s.ingest(1, &intents(&[(7, 4096)])),
        Err(Error::IntentAlreadyActive)
    );
    assert!(s.ingest(1, &intents(&[(8, 4096)])).is_ok());
}

#[test]
fn remote_intent_id_can_be_reused_after_nonreusable_completion() {
    let mut s = session();
    let handle = queued_reply(&mut s); // Installs peer intent 7 and queues its reply.
    assert!(matches!(
        publish_until_data(&mut s).0,
        Published::Data {
            remote_intent: 7,
            last_fragment: true
        }
    ));
    assert_eq!(s.ingest(1, &control(6, 7)), Ok(Event::ReplyRxDone(handle)));
    assert_eq!(
        s.ingest(1, &intents(&[(7, 4096)])),
        Ok(Event::RemoteIntents)
    );
    assert_eq!(
        s.ingest(1, &intents(&[(7, 4096)])),
        Err(Error::IntentAlreadyActive),
        "a cyclic ID cannot collide with an intent that is still live"
    );
}

#[test]
fn remote_reuse_completion_releases_intent_for_a_later_transmit() {
    let mut s = Session::new(config(), 1).unwrap();
    s.register(1, Service::HeapMemory, MEMORY).unwrap();
    let first_reply_handle = queued_reply(&mut s);
    publish(&mut s).unwrap();
    assert_eq!(
        s.ingest(1, &control(14, 7)),
        Ok(Event::ReplyRxDone(first_reply_handle))
    );
    credit(&mut s, MEMORY);
    let handle = request(&mut s, 2);
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    assert!(matches!(
        publish(&mut s).unwrap().0,
        Published::ReceiveDone(_)
    ));
    // A previous credit's RX_DONE can precede the new request's RX_DONE.
    while let Some((event, _)) = publish(&mut s) {
        if matches!(
            event,
            Published::Data {
                remote_intent: 7,
                ..
            }
        ) {
            return;
        }
    }
    panic!("reusable remote intent not available");
}

#[test]
fn malformed_credit_route_payload_or_timing_is_held() {
    for modification in 0..7 {
        let mut s = session();
        queued_reply(&mut s);
        if modification != 5 {
            publish(&mut s).unwrap();
        }
        let id = offer(&mut s);
        let mut bytes = resume(MEMORY);
        match modification {
            0 => put(&mut bytes, 8, 2),
            1 => put(&mut bytes, 28, STORAGE),
            2 => put(&mut bytes, 40, 10),
            3 => put(&mut bytes, 16, 1),
            4 => bytes[44] = 1,
            6 => put(&mut bytes, 36, 1),
            _ => {}
        }
        let result = s.ingest(1, &data(9, id, &bytes, 0));
        if matches!(modification, 1 | 2 | 6) {
            // Linux QRTR uses the body client address as a flow lookup key;
            // an absent key is ignored, and the header destination port does
            // not select a local socket for RESUME_TX.
            assert_eq!(result, Ok(Event::CreditReceived));
            assert_eq!(s.in_flight(), 1, "unmatched credit cannot retire a reply");
            assert_eq!(s.last_credit_diagnostic(), None);
            continue;
        }
        assert_eq!(result, Err(Error::InvalidCredit));
        assert_eq!(s.in_flight(), 1);
        let diagnostic = s.last_credit_diagnostic().expect("bounded credit detail");
        assert_eq!(diagnostic.packet_len, 52);
        assert_ne!(diagnostic.bad_fields, 0);
        match modification {
            0 => assert_eq!(diagnostic.words[2], 2),
            3 => assert_eq!(diagnostic.words[4], 1),
            4 => assert_eq!(diagnostic.words[11], 1),
            5 => assert_ne!(diagnostic.reply_state & 4, 0),
            _ => unreachable!(),
        }
    }
}

#[test]
fn valid_unmatched_resume_is_ignored_without_releasing_a_transaction() {
    let mut s = session();
    queued_reply(&mut s);
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { .. }));
    // Retire the QRTR reply and its GLINK slot, then replay its valid resume.
    s.ingest(1, &control(6, 7)).unwrap();
    credit(&mut s, MEMORY);
    assert_eq!(s.in_flight(), 0);
    let id = offer(&mut s);
    assert_eq!(
        s.ingest(1, &data(9, id, &resume(MEMORY), 0)),
        Ok(Event::CreditReceived)
    );
    assert_eq!(s.last_credit_diagnostic(), None);
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn valid_resume_for_an_untracked_flow_is_consumed_as_a_noop() {
    let mut s = session();
    queued_reply(&mut s);
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { .. }));
    s.ingest(1, &control(6, 7)).unwrap();
    assert_eq!(s.in_flight(), 1);

    let id = offer(&mut s);
    assert_eq!(
        s.ingest(1, &data(9, id, &resume_route(0xdead_beef, MEMORY), 0)),
        Ok(Event::CreditReceived)
    );
    assert_eq!(s.last_credit_diagnostic(), None);
    assert_eq!(
        s.in_flight(),
        1,
        "unknown credit cannot retire another flow"
    );

    credit(&mut s, MEMORY);
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn repeated_untracked_resumes_reuse_the_receive_intent_after_rx_done() {
    let mut s = session();
    for index in 0..12 {
        let id = offer(&mut s);
        let peer_port = 0xdead_0000 + index;
        assert_eq!(
            s.ingest(1, &data(9, id, &resume_route(peer_port, STORAGE), 0)),
            Ok(Event::CreditReceived)
        );
        assert_eq!(s.last_credit_diagnostic(), None);
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
        assert_eq!(s.in_flight(), 0);
    }
}

#[test]
fn same_destination_flow_has_one_reply_waiting_for_credit() {
    let mut s = session();
    let first = request(&mut s, 1);
    let second = request(&mut s, 2);
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    s.queue_reply(&first, Reply::Failure { error: 3 }).unwrap();
    assert_eq!(
        s.queue_reply(&second, Reply::Failure { error: 3 }),
        Err(Error::FlowBusy)
    );
    assert_eq!(s.request(&second).unwrap().0.transaction, 2);
    while publish(&mut s).is_some() {}
    s.ingest(1, &control(6, 7)).unwrap();
    let credit_id = offer(&mut s);
    assert_eq!(
        s.ingest(1, &data(9, credit_id, &resume(MEMORY), 0)),
        Ok(Event::ReplyQrtrResume(first))
    );
    assert!(s.queue_reply(&second, Reply::Failure { error: 3 }).is_ok());
}

#[test]
fn invalid_reply_tlvs_do_not_consume_request_or_queue_output() {
    let mut s = session();
    let handle = request(&mut s, 1);
    publish(&mut s).unwrap();
    assert_eq!(
        s.queue_reply(&handle, Reply::Failure { error: 0 }),
        Err(Error::InvalidReply)
    );
    for fields in [
        &[0x10, 1][..],
        &[0x10, 2, 0, 1][..],
        &[0x10, 0, 0, 0x10, 0, 0][..],
        &[1, 0, 0][..],
    ] {
        assert!(s
            .queue_reply(
                &handle,
                Reply::Success {
                    optional_tlvs: fields
                }
            )
            .is_err());
        assert_eq!(s.in_flight(), 1);
        assert!(s.request(&handle).is_ok());
        assert!(publish(&mut s).is_none());
    }
    let too_large = vec![0; 4096];
    assert!(s
        .queue_reply(
            &handle,
            Reply::Success {
                optional_tlvs: &too_large
            }
        )
        .is_err());
}

#[test]
fn long_success_reply_fragments_and_holds_intent_until_last_frame() {
    let mut s = session();
    let handle = request(&mut s, 1);
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    let mut fields = vec![0x10, 0, 0];
    fields[1..3].copy_from_slice(&700u16.to_le_bytes());
    fields.extend((0..700).map(|index| index as u8));
    s.queue_reply(
        &handle,
        Reply::Success {
            optional_tlvs: &fields,
        },
    )
    .unwrap();
    publish(&mut s).unwrap(); // RX_DONE.
    let mut assembled = Vec::new();
    let mut frames = 0;
    while let Some((event, frame)) = publish(&mut s) {
        let Published::Data {
            remote_intent,
            last_fragment,
        } = event
        else {
            panic!()
        };
        assert_eq!(remote_intent, 7);
        assert_eq!(
            u16::from_le_bytes(frame[..2].try_into().unwrap()),
            if frames == 0 { 9 } else { 12 }
        );
        assert!(frame.len() <= qcom_glink_qmi::MAX_TX_RECORD_BYTES);
        let chunk = word(&frame, 8) as usize;
        assembled.extend(&frame[16..16 + chunk]);
        if !last_fragment {
            assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::InvalidCompletion));
            let diagnostic = s.last_completion_error().unwrap();
            assert_eq!(diagnostic.intent_id, 7);
            assert_eq!(
                diagnostic.failure,
                qcom_glink_qmi::CompletionFailure::NoFullyPublishedTransmit
            );
            assert_eq!(diagnostic.busy_remote_intents, 1);
            assert_eq!(diagnostic.incomplete_transmits, 1);
            assert_eq!(diagnostic.completed_transmits, 0);
        }
        frames += 1;
    }
    assert_eq!(frames, 3);
    let len = 32 + word(&assembled, 20) as usize;
    let decoded = Message::decode(&assembled[32..len]).unwrap();
    assert_eq!(decoded.required(2).unwrap(), &[0, 0, 0, 0]);
    assert_eq!(decoded.required(0x10).unwrap(), &fields[3..]);
    s.ingest(1, &control(6, 7)).unwrap();
    credit(&mut s, MEMORY);
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn exact_maximum_reply_and_receive_sizes_work() {
    let mut s = session();
    let id = offer(&mut s);
    let mut bytes = query(1, false);
    bytes.truncate(46);
    bytes.extend([0x11, 0xcf, 0x0f]); // 4047 unknown optional bytes.
    bytes.extend(vec![0; 4047]);
    put(&mut bytes, 20, 4064);
    bytes[37..39].copy_from_slice(&4057u16.to_le_bytes());
    assert_eq!(bytes.len(), 4096);
    let h = receive(&mut s, id, &bytes);
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    let mut fields = vec![0x10];
    fields.extend(4047u16.to_le_bytes());
    fields.extend(vec![0; 4047]);
    s.queue_reply(
        &h,
        Reply::Success {
            optional_tlvs: &fields,
        },
    )
    .unwrap();
    publish(&mut s).unwrap();
    let mut total = 0;
    let mut count = 0;
    while let Some((event, bytes)) = publish(&mut s) {
        assert!(matches!(event, Published::Data { .. }));
        total += word(&bytes, 8) as usize;
        count += 1;
    }
    assert_eq!((total, count), (4096, 16));
}

#[test]
fn fixed_receive_request_and_remote_tables_apply_backpressure() {
    let mut s = session();
    for txn in 0..4 {
        request(&mut s, txn);
    }
    let id = offer(&mut s);
    #[cfg(not(feature = "sunfish-qrtr-deferred-requests"))]
    assert_eq!(
        s.ingest(1, &data(9, id, &query(5, false), 0)),
        Err(Error::NoCapacity)
    );
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    assert_eq!(s.ingest(1, &data(9, id, &query(5, false), 0)), Ok(Event::RequestDeferred));
    assert_eq!(s.in_flight(), 4);
    for _ in 0..3 {
        offer(&mut s);
    }
    assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
    let entries: Vec<_> = (0..32).map(|id| (id, id + 1)).collect();
    s.ingest(1, &intents(&entries)).unwrap();
    assert_eq!(s.ingest(1, &intents(&[(32, 4096)])), Err(Error::NoCapacity));
}

#[cfg(feature = "sunfish-qrtr-deferred-requests")]
#[test]
fn deferred_requests_reenter_in_arrival_order_only_after_capacity_returns() {
    let mut s = session();
    let mut handles = Vec::new();
    for txn in 1..=4 {
        let id = offer(&mut s);
        handles.push(receive(&mut s, id, &open(txn)));
    }
    for txn in 5..=6 {
        let id = offer(&mut s);
        assert_eq!(s.ingest(1, &data(9, id, &open(txn), 0)), Ok(Event::RequestDeferred));
    }
    let snapshot = s.receive_snapshot();
    assert_eq!((snapshot.states >> 16) & 0xff, 0xaa);
    assert_eq!((snapshot.services >> 16) & 0xff, 0x11);
    assert!(!s.deferred_ready());
    for (index, remote) in [(0, 7), (1, 8)] {
        s.ingest(1, &intents(&[(remote, 4096)])).unwrap();
        s.queue_reply(&handles[index], Reply::Failure { error: 3 }).unwrap();
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        assert_eq!(s.ingest(1, &control(6, remote)), Ok(Event::ReplyRxDone(handles[index])));
        let handle = match s.retry_deferred(1).unwrap() {
            Some(Event::Request(handle)) => handle,
            other => panic!("expected deferred request: {other:?}"),
        };
        assert_eq!(s.request(&handle).unwrap().0.transaction, 5 + index as u16);
        assert_eq!(s.in_flight(), 4);
        assert!(!s.deferred_ready());
    }
}

#[cfg(all(feature = "sunfish-qrtr-deferred-requests", feature = "sunfish-native-tftp-server-check", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn tftp_confirmed_packet_waits_for_full_tx_pool_and_resumes_once() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap();
    s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let incoming = offer(&mut s);
    for n in 0..8 {
        s.queue_tftp_response(1, TftpReplyRoute {
            source: Address { node: 1, port: 0x5001 + n },
            destination: Address { node: 0, port: 0x60 + n },
        }, qcom_tftp_server_check::OACK).unwrap();
    }
    #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
    {
        let route = TftpReplyRoute {
            source: Address { node: 1, port: 0x5001 },
            destination: Address { node: 0, port: 0x60 },
        };
        let oack = qcom_tftp_server_check::OACK;
        assert_eq!(s.queue_tftp_response(1, route, oack), Err(Error::FlowBusy));
        let block = s.tftp_queue_block();
        assert_eq!((block.check, block.kind, block.opcode, block.flags & 3), (1, 6, 6, 0));
        let other = TftpReplyRoute { destination: Address { node: 0, port: 0x70 }, ..route };
        assert_eq!(s.queue_tftp_response(1, other, oack), Err(Error::NoCapacity));
        assert_eq!(s.tftp_queue_block().check, 2);
    }
    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    {
        assert_eq!(s.tftp_tx_occupied(), 0xff);
        let snapshot = s.tftp_tx_slots();
        assert!(snapshot.iter().enumerate().all(|(index, slot)|
            (slot.kind, slot.opcode, slot.flags & 3, slot.peer_port, slot.remote_intent)
                == (6, 6, 0, 0x60 + index as u32, u32::MAX)));
    }
    let packet = qrtr(1, 0x70, 0x5000, true, qcom_tftp_server_check::REQUEST);
    assert_eq!(s.ingest(1, &data(9, incoming, &packet, 0)), Ok(Event::RequestDeferred));
    assert!(!s.deferred_ready());
    s.ingest(1, &intents(&[(30, 4096)])).unwrap();
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 30, .. }));
    #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
    {
        let route = TftpReplyRoute {
            source: Address { node: 1, port: 0x5001 },
            destination: Address { node: 0, port: 0x60 },
        };
        assert_eq!(s.queue_tftp_response(1, route, qcom_tftp_server_check::OACK), Err(Error::FlowBusy));
        let block = s.tftp_queue_block();
        assert_eq!((block.check, block.kind, block.opcode, block.flags & 7), (1, 6, 6, 3));
    }
    s.ingest(1, &control(6, 30)).unwrap();
    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    assert_eq!(!s.tftp_tx_occupied(), 1);

    let handle = match s.retry_deferred(1).unwrap() {
        Some(Event::TftpRequest(h)) => h,
        other => panic!("expected one TFTP request: {other:?}"),
    };
    assert_eq!(s.tftp_datagram(&handle).unwrap().1, qcom_tftp_server_check::REQUEST);
    assert_eq!(s.retry_deferred(1).unwrap(), None);
}

#[cfg(all(feature = "sunfish-tftp-endpoint-observer", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn first_oack_endpoint_trace_records_qrtr_confirmation_and_glink_completion() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let route = TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } };
    for _ in 0..4 {
        s.queue_tftp_response(1, route, &[0, 4, 0, 1]).unwrap();
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("TFTP data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    assert!(s.tftp_endpoint_trace().is_none(), "non-OACK replies must not occupy the trace");
    s.queue_tftp_response(1, route, qcom_tftp_server_check::OACK).unwrap();
    let queued = s.tftp_endpoint_trace().unwrap();
    assert_eq!(queued.flags, 17, "fifth packet requests QRTR confirmation");
    assert_eq!(queued.remote_intent, u32::MAX);
    let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("OACK data expected") };
    assert_eq!(s.tftp_endpoint_trace().unwrap().flags, 23);
    assert_eq!(s.tftp_endpoint_trace().unwrap().remote_intent, remote_intent);
    s.ingest(1, &control(14, remote_intent)).unwrap();
    assert_eq!(s.tftp_endpoint_trace().unwrap().flags, 31);
}

#[cfg(all(feature = "sunfish-tftp-endpoint-observer", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn first_oack_trace_ignores_other_peer_completion_even_when_it_arrives_first() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap();
    s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let first = TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x31 } };
    let second = TftpReplyRoute { source: Address { node: 1, port: 0x5002 }, destination: Address { node: 0, port: 0x32 } };
    s.queue_tftp_response(1, first, qcom_tftp_server_check::OACK).unwrap();
    let (Published::Data { remote_intent: first_id, .. }, _) = publish_until_data(&mut s) else { panic!("first OACK expected") };
    s.queue_tftp_response(1, second, qcom_tftp_server_check::OACK).unwrap();
    let (Published::Data { remote_intent: second_id, .. }, _) = publish_until_data(&mut s) else { panic!("second OACK expected") };
    assert_ne!(first_id, second_id);
    s.ingest(1, &control(14, second_id)).unwrap();
    let trace = s.tftp_endpoint_trace().unwrap();
    assert_eq!(trace.destination, first.destination);
    assert_eq!(trace.remote_intent, first_id);
    assert_eq!(trace.flags & 8, 0, "other peer cannot complete first OACK");
    s.ingest(1, &control(14, first_id)).unwrap();
    assert_ne!(s.tftp_endpoint_trace().unwrap().flags & 8, 0);
}

#[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
#[test]
fn ready_tftp_oack_gets_a_turn_between_other_peer_fragments() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = session();
    s.prepare_service_available(1, Service::HeapMemory).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap();
    s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..3 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let handle = request(&mut s, 42);
    let mut fields = vec![0x10];
    fields.extend(700u16.to_le_bytes());
    fields.extend(vec![0; 700]);
    s.queue_reply(&handle, Reply::Success { optional_tlvs: &fields }).unwrap();
    let (first, _) = publish_until_data(&mut s);
    assert!(matches!(first, Published::Data { last_fragment: false, .. }));
    let route = TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } };
    s.queue_tftp_response(1, route, qcom_tftp_server_check::OACK).unwrap();
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    let (_, frame) = publish_until_data(&mut s);
    assert_eq!(word(&frame, 28), 0x5001, "the ready OACK must get a turn before the unrelated transfer's next fragment");
    let trace = s.tftp_schedule_trace();
    assert!(trace.calls >= 2);
    assert_eq!(trace.mask & 0x83, 0x83, "a suitable intent, selected OACK and blocked writer were observed");
    let _ = publish_until_data(&mut s);
    assert_eq!(s.tftp_schedule_trace(), trace, "the snapshot stops accumulating after first OACK publication");
}

#[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
#[test]
fn repeated_low_slot_tftp_does_not_starve_other_ready_data() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = session();
    s.prepare_service_available(1, Service::HeapMemory).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..3 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let handle = request(&mut s, 43);
    let route = |local, remote| TftpReplyRoute { source: Address { node: 1, port: local }, destination: Address { node: 0, port: remote } };
    s.queue_tftp_response(1, route(0x5001, 0x56), qcom_tftp_server_check::OACK).unwrap();
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    let (first, frame) = publish_until_data(&mut s);
    assert_eq!(word(&frame, 28), 0x5001);
    let Published::Data { remote_intent, .. } = first else { panic!("TFTP data expected") };
    s.ingest(1, &control(14, remote_intent)).unwrap();
    s.queue_tftp_response(1, route(0x5002, 0x57), qcom_tftp_server_check::OACK).unwrap();
    let (_, frame) = publish_until_data(&mut s);
    assert_eq!(word(&frame, 28), MEMORY, "other ready traffic must run before a second TFTP reply");
}

#[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
#[test]
fn unsendable_tftp_does_not_block_a_smaller_ready_reply() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = session();
    s.prepare_service_available(1, Service::HeapMemory).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..3 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(6, remote_intent)).unwrap();
    }
    let handle = request(&mut s, 44);
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } }, qcom_tftp_server_check::OACK).unwrap();
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    s.ingest(1, &intents(&[(10, 52)])).unwrap();
    let (event, frame) = publish_until_data(&mut s);
    assert!(matches!(event, Published::Data { remote_intent: 10, .. }));
    assert_eq!(word(&frame, 28), MEMORY, "an OACK without a large enough intent must not block a smaller reply");
    let trace = s.tftp_schedule_trace();
    assert!(trace.calls > 0);
    assert_eq!(trace.mask & 1, 0, "no suitable OACK intent was available");
    assert_ne!(trace.mask & 4, 0, "a different ready transfer was selected");
    assert_ne!(trace.mask & 64, 0, "a receive completion was also pending");
}

#[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
#[test]
fn ready_tftp_and_qrtr_credit_both_get_bounded_turns() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = session();
    s.prepare_service_available(1, Service::HeapMemory).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..3 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let incoming = offer(&mut s);
    let _request = receive(&mut s, incoming, &query(45, true));
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } }, qcom_tftp_server_check::OACK).unwrap();
    let mut kinds = Vec::new();
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, frame) = publish_until_data(&mut s) else { panic!("data expected") };
        kinds.push(word(&frame, 20));
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    kinds.sort_unstable();
    assert_eq!(kinds, [1, 7], "both the OACK and QRTR credit must publish");
}

#[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
#[test]
fn ready_tftp_gets_a_turn_during_receive_intent_advertisements() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    for _ in 0..3 { s.offer_receive(1).unwrap(); }
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } }, qcom_tftp_server_check::OACK).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
    let (event, frame) = publish(&mut s).unwrap();
    assert!(matches!(event, Published::Data { .. }), "a ready OACK must not wait for all receive advertisements");
    assert_eq!(word(&frame, 28), 0x5001);
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)), "advertisements must continue after the OACK turn");
}

#[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
#[test]
fn tftp_turn_preserves_backing_advertisement_before_granted_intent_ack() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } }, qcom_tftp_server_check::OACK).unwrap();
    assert_eq!(s.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)), "backing intent is advertised first");
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    let (event, frame) = publish(&mut s).unwrap();
    assert!(matches!(event, Published::Data { .. }));
    assert_eq!(word(&frame, 28), 0x5001);
    #[cfg(not(feature = "sunfish-wlan-intent-ack-turn"))]
    {
        let mut ack = false;
        for _ in 0..qcom_glink_qmi::RX_SLOTS + 2 {
            match publish(&mut s).unwrap().0 {
                Published::ReceiveIntent(_) | Published::ReceiveDone(_) => {},
                Published::IntentRequestAck { granted: true } => { ack = true; break; },
                other => panic!("unexpected publication before intent ACK: {other:?}"),
            }
        }
        assert!(ack, "the intent ACK must still get a bounded turn");
    }
}

#[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
#[test]
fn uncertain_priority_tftp_publication_quarantines_the_session() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control data expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    s.offer_receive(1).unwrap();
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 }, destination: Address { node: 0, port: 0x56 } }, qcom_tftp_server_check::OACK).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::Uncertain)), Err(Error::PublicationUncertain));
    assert_eq!(s.publish_next(1, |_| Ok(())), Err(Error::Quarantined));
}

#[cfg(all(feature = "sunfish-qrtr-deferred-requests", feature = "sunfish-native-tftp-server-check", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn closing_peer_with_deferred_tftp_preserves_branch_and_header_evidence() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let incoming = offer(&mut s);
    let close_intent = offer(&mut s);
    for n in 0..8 {
        s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 + n },
            destination: Address { node: 0, port: 0x60 + n } }, qcom_tftp_server_check::OACK).unwrap();
    }
    let packet = qrtr(1, 0x70, 0x5000, true, qcom_tftp_server_check::REQUEST);
    assert_eq!(s.ingest(1, &data(9, incoming, &packet, 0)), Ok(Event::RequestDeferred));
    let close = qrtr_control(6, [0, 0x70, 0, 0]);
    assert_eq!(s.ingest(1, &data(9, close_intent, &close, 0)), Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
    let detail = s.deferred_close_diagnostic().unwrap();
    assert_eq!((detail.command, detail.peer_port, detail.destination_port, detail.opcode, detail.matching), (6, 0x70, 0x5000, 2, 1));
    assert!(s.client_gone_diagnostic().is_none());
    assert_eq!(s.retry_deferred(1), Err(Error::Quarantined));
}

#[cfg(all(feature = "sunfish-tftp-deferred-close-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
fn deferred_tftp_close_fixture(busy: bool, conflicting_tx: bool, packet: &[u8]) -> (Session, u32, u32) {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let incoming = offer(&mut s); let close_intent = offer(&mut s);
    for n in 0..8 {
        let peer = if conflicting_tx && n == 0 { 0x70 } else { 0x60 + n };
        s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5001 + n },
            destination: Address { node: 0, port: peer } }, qcom_tftp_server_check::OACK).unwrap();
    }
    let incoming_packet = qrtr(1, 0x70, 0x5000, true, packet);
    assert_eq!(s.ingest(1, &data(9, incoming, &incoming_packet, 0)), Ok(Event::RequestDeferred));
    #[cfg(not(feature = "sunfish-tftp-peer-close-experiment"))]
    s.set_tftp_owner_busy(1, busy).unwrap();
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    {
        let mut peers = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
        if busy { peers[0] = Some(Address { node: 0, port: 0x70 }); }
        s.set_tftp_owner_peers(1, peers).unwrap();
    }
    (s, incoming, close_intent)
}

#[cfg(all(feature = "sunfish-tftp-deferred-close-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn closed_peer_exact_deferred_wrq_is_fenced_and_returned_without_processing() {
    let (mut s, deferred, close_intent) = deferred_tftp_close_fixture(false, false, qcom_tftp_server_check::REQUEST);
    let close = qrtr_control(6, [0, 0x70, 0, 0]);
    assert!(matches!(s.ingest(1, &data(9, close_intent, &close, 0)), Ok(Event::QrtrControl(_))));
    assert!(s.deferred_close_diagnostic().is_none());
    assert_eq!(s.retry_deferred(1).unwrap(), None, "closed request is never dispatched");
    let mut returned = Vec::new();
    // Fair scheduling may interleave outgoing data with receive completions.
    // Both closed receive intents must still be returned within a bounded turn.
    for _ in 0..8 {
        if let Some((Published::ReceiveDone(id), _)) = publish(&mut s) { returned.push(id); }
        if returned.contains(&deferred) && returned.contains(&close_intent) { break; }
    }
    assert!(returned.contains(&deferred) && returned.contains(&close_intent));
    let late = offer(&mut s);
    let packet = qrtr(1, 0x70, 0x5000, true, qcom_tftp_server_check::REQUEST);
    assert_eq!(s.ingest(1, &data(9, late, &packet, 0)), Err(Error::FlowBusy), "closed port remains fenced");
}

#[cfg(all(feature = "sunfish-tftp-deferred-close-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn deferred_close_still_stops_for_owner_work_outstanding_tx_or_other_packets() {
    for (busy, conflict, packet, expected_bit) in [(true, false, qcom_tftp_server_check::REQUEST, 2),
        (false, true, qcom_tftp_server_check::REQUEST, 8), (false, false, b"\0\x02/other\0octet\0" as &[u8], 1)] {
        let (mut s, _, close_intent) = deferred_tftp_close_fixture(busy, conflict, packet);
        let close = qrtr_control(6, [0, 0x70, 0, 0]);
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        if busy {
            assert!(matches!(s.ingest(1, &data(9, close_intent, &close, 0)), Ok(Event::TftpClosePending { .. })));
            assert_eq!(s.deferred_close_diagnostic().unwrap().block_mask, 2);
            assert_eq!(s.offer_receive(1), Err(Error::FlowBusy));
            let mut wrote = false;
            assert_eq!(s.publish_next(1, |_| { wrote = true; Ok(()) }), Err(Error::FlowBusy));
            assert!(!wrote, "no receive completion or other record may publish while pending");
            assert_eq!(s.ingest(1, &control(6, 7)), Err(Error::FlowBusy));
            let peer = qcom_qmi_router::Address { node: 0, port: 0x70 };
            assert_eq!(s.reject_tftp_close(2, peer), Err(Error::StaleGeneration));
            assert_eq!(s.reject_tftp_close(1, qcom_qmi_router::Address { node: 0, port: 0x71 }), Err(Error::InvalidHandle));
            assert_eq!(s.reject_tftp_close(1, peer), Ok(()));
            assert_eq!(s.offer_receive(1), Err(Error::Quarantined));
            continue;
        }
        assert_eq!(s.ingest(1, &data(9, close_intent, &close, 0)), Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
        assert_ne!(s.deferred_close_diagnostic().unwrap().block_mask & expected_bit, 0);
    }
}

#[cfg(all(feature = "sunfish-tftp-peer-close-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn deferred_close_allows_unrelated_owner_peer_but_keeps_closed_peer_fenced() {
    use qcom_qmi_router::Address;
    let (mut s, _, close_intent) = deferred_tftp_close_fixture(false, false, qcom_tftp_server_check::REQUEST);
    let mut peers = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
    peers[0] = Some(Address { node: 0, port: 0x99 });
    s.set_tftp_owner_peers(1, peers).unwrap();
    let close = qrtr_control(6, [0, 0x70, 0, 0]);
    assert!(matches!(s.ingest(1, &data(9, close_intent, &close, 0)), Ok(Event::QrtrControl(_))));
    assert_eq!(s.retired_rmtfs_peer_count(), 1);
}

#[cfg(all(feature = "sunfish-tftp-close-owner-fence", not(feature = "sunfish-tftp-wlan-close-handoff")))]
#[test]
fn client_close_with_owner_reply_but_no_deferred_request_quarantines_before_publish() {
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap();
    s.start_control(1).unwrap();
    let hello = offer(&mut s);
    let closing = s.offer_receive(1).unwrap();
    let mut advertised = false;
    for _ in 0..8 {
        if matches!(publish(&mut s), Some((Published::ReceiveIntent(id), _)) if id == closing) {
            advertised = true;
            break;
        }
    }
    assert!(advertised);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    let mut peers = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
    peers[0] = Some(Address { node: 0, port: 0x70 });
    s.set_tftp_owner_peers(1, peers).unwrap();
    let mut firmware = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
    firmware[0] = Some(Address { node: 0, port: 0x70 });
    s.set_tftp_firmware_peers(1, firmware).unwrap();
    assert_eq!(s.ingest(1, &data(9, closing, &qrtr_control(6, [0, 0x70, 0, 0]), 0)),
        Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
    let mut wrote = false;
    assert_eq!(s.publish_next(1, |_| { wrote = true; Ok(()) }), Err(Error::Quarantined));
    assert!(!wrote);
}

#[cfg(feature = "sunfish-tftp-close-owner-fence")]
#[test]
fn client_close_with_transport_tftp_reply_and_empty_owner_snapshot_quarantines() {
    use qcom_qmi_router::Address;
    use qcom_glink_qmi::TftpReplyRoute;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap();
    s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let closing = offer(&mut s);
    s.set_tftp_owner_peers(1, [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS]).unwrap();
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5100 },
        destination: Address { node: 0, port: 0x70 } }, b"\0\x06x\0").unwrap();
    assert_eq!(s.ingest(1, &data(9, closing, &qrtr_control(6, [0, 0x70, 0, 0]), 0)),
        Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
    let mut wrote = false;
    assert_eq!(s.publish_next(1, |_| { wrote = true; Ok(()) }), Err(Error::Quarantined));
    assert!(!wrote);
}

#[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
#[test]
fn pending_firmware_close_blocks_publication_until_exact_commit_and_fences_peer() {
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    let hello = offer(&mut s); let closing = s.offer_receive(1).unwrap();
    let mut advertised = false;
    for _ in 0..8 {
        if matches!(publish(&mut s), Some((Published::ReceiveIntent(id), _)) if id == closing) { advertised = true; break; }
    }
    assert!(advertised);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    let peer = Address { node: 0, port: 0x70 };
    let mut firmware = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS]; firmware[0] = Some(peer);
    s.set_tftp_firmware_peers(1, firmware).unwrap();
    assert_eq!(s.ingest(1, &data(9, closing, &qrtr_control(6, [0, peer.port, 0, 0]), 0)),
        Ok(Event::FirmwareClosePending { peer }));
    let mut wrote = false;
    assert_eq!(s.publish_next(1, |_| { wrote = true; Ok(()) }), Err(Error::FlowBusy)); assert!(!wrote);
    assert_eq!(s.commit_firmware_close(2, peer), Err(Error::StaleGeneration));
    assert_eq!(s.commit_firmware_close(1, Address { node: 0, port: 0x71 }), Err(Error::InvalidHandle));
    assert_eq!(s.commit_firmware_close(1, peer), Ok(()));
    assert_eq!(s.commit_firmware_close(1, peer), Err(Error::InvalidHandle));
    assert_eq!(s.retired_rmtfs_peer_count(), 1);
}

#[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
#[test]
fn firmware_close_preserves_published_transmit_until_real_glink_completion() {
    use qcom_qmi_router::Address;
    use qcom_glink_qmi::TftpReplyRoute;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let peer = Address { node: 0, port: 0x70 };
    let mut firmware = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS]; firmware[0] = Some(peer);
    s.set_tftp_firmware_peers(1, firmware).unwrap();
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5100 }, destination: peer }, b"\0\x03\0\x01data").unwrap();
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 9, last_fragment: true }));
    let closing = offer(&mut s);
    assert_eq!(s.ingest(1, &data(9, closing, &qrtr_control(6, [0, peer.port, 0, 0]), 0)), Ok(Event::FirmwareClosePending { peer }));
    assert_eq!(s.commit_firmware_close(1, peer), Ok(()));
    assert!(matches!(s.ingest(1, &control(6, 9)), Ok(Event::TransmitDone)));
    let late = offer(&mut s);
    assert_eq!(s.ingest(1, &data(9, late, &qrtr(1, peer.port, 0x5100, false, &[0, 4, 0, 1]), 0)), Err(Error::FlowBusy));
}

#[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
#[test]
fn firmware_close_cancels_only_provably_unpublished_transport_reply() {
    use qcom_qmi_router::Address;
    use qcom_glink_qmi::TftpReplyRoute;
    let mut s = Session::new(config(), 1).unwrap(); s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let closing = offer(&mut s);
    let peer = Address { node: 0, port: 0x70 };
    let mut firmware = [None; qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS]; firmware[0] = Some(peer);
    s.set_tftp_firmware_peers(1, firmware).unwrap();
    s.queue_tftp_response(1, TftpReplyRoute { source: Address { node: 1, port: 0x5100 }, destination: peer }, b"\0\x03\0\x01data").unwrap();
    assert_eq!(s.ingest(1, &data(9, closing, &qrtr_control(6, [0, peer.port, 0, 0]), 0)), Ok(Event::FirmwareClosePending { peer }));
    assert_eq!(s.commit_firmware_close(1, peer), Ok(()));
    for _ in 0..8 { assert!(!matches!(publish(&mut s), Some((Published::Data { .. }, _)))); }
    assert_eq!(s.retired_rmtfs_peer_count(), 1);
}

#[cfg(all(feature = "sunfish-tftp-wlan-mux", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn firmware_data_retires_on_glink_completion_without_individual_qrtr_resume() {
    use qcom_qmi_router::Address;
    use qcom_glink_qmi::TftpReplyRoute;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)), Ok(Event::QrtrControl(_))));
    for remote in [7, 8] {
        assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent, .. } if remote_intent == remote));
        s.ingest(1, &control(6, remote)).unwrap();
    }
    let route = TftpReplyRoute { source: Address { node: 1, port: 0x5100 }, destination: Address { node: 0, port: 0x70 } };
    s.queue_tftp_response(1, route, b"\0\x03\0\x01test").unwrap();
    let (event, wire) = publish_until_data(&mut s);
    assert!(matches!(event, Published::Data { remote_intent: 9, .. }));
    assert_eq!(word(&wire, 16 + 16), 0, "first data packet must not force QRTR confirm");
    s.ingest(1, &control(6, 9)).unwrap();
    assert_eq!(s.queue_tftp_response(1, route, b"\0\x03\0\x02test"), Ok(()));
}

#[cfg(all(feature = "sunfish-tftp-close-handoff-prototype", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn pending_close_commit_fences_peer_and_releases_both_receive_slots() {
    use qcom_qmi_router::Address;
    let (mut s, deferred, closing) = deferred_tftp_close_fixture(true, false, qcom_tftp_server_check::REQUEST);
    let peer = Address { node: 0, port: 0x70 };
    let close = qrtr_control(6, [0, peer.port, 0, 0]);
    assert_eq!(s.ingest(1, &data(9, closing, &close, 0)), Ok(Event::TftpClosePending { peer }));
    assert_eq!(s.commit_tftp_close(2, peer), Err(Error::StaleGeneration));
    assert_eq!(s.commit_tftp_close(1, Address { node: 0, port: 0x71 }), Err(Error::InvalidHandle));
    assert_eq!(s.commit_tftp_close(1, peer), Ok(qcom_qrtr_control::Event::ClientRemoved(peer.port)));
    assert_eq!(s.commit_tftp_close(1, peer), Err(Error::InvalidHandle));
    let mut returned = Vec::new();
    // Fair scheduling may interleave outgoing data with receive completions.
    for _ in 0..8 {
        if let Some((Published::ReceiveDone(id), _)) = publish(&mut s) { returned.push(id); }
        if returned.contains(&deferred) && returned.contains(&closing) { break; }
    }
    assert!(returned.contains(&deferred) && returned.contains(&closing));
    let late = offer(&mut s);
    let packet = qrtr(1, peer.port, 0x5000, true, qcom_tftp_server_check::REQUEST);
    assert_eq!(s.ingest(1, &data(9, late, &packet, 0)), Err(Error::FlowBusy));
}

#[cfg(all(feature = "sunfish-tftp-deferred-close-experiment", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn deferred_close_stops_when_same_peer_has_another_deferred_or_held_request() {
    for confirm in [true, false] {
        let (mut s, _, close_intent) = deferred_tftp_close_fixture(false, false, qcom_tftp_server_check::REQUEST);
        let another = offer(&mut s);
        let packet = qrtr(1, 0x70, 0x5000, confirm, qcom_tftp_server_check::REQUEST);
        let event = s.ingest(1, &data(9, another, &packet, 0)).unwrap();
        if confirm { assert_eq!(event, Event::RequestDeferred); }
        else { assert!(matches!(event, Event::TftpRequest(_))); }
        let close = qrtr_control(6, [0, 0x70, 0, 0]);
        assert_eq!(s.ingest(1, &data(9, close_intent, &close, 0)), Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
    }
}

#[test]
fn wrong_channels_generations_and_commands_cannot_route_or_publish() {
    let mut s = session();
    assert_eq!(
        Session::new(
            Config {
                local_channel: 0,
                ..config()
            },
            1
        )
        .err(),
        Some(Error::InvalidChannel)
    );
    let id = offer(&mut s);
    let mut bytes = data(9, id, &query(1, false), 0);
    bytes[2] = 1;
    assert_eq!(s.ingest(1, &bytes), Err(Error::WrongChannel));
    bytes[2] = 2;
    assert_eq!(s.ingest(2, &bytes), Err(Error::StaleGeneration));
    assert_eq!(s.ingest(1, &control(1, 0)), Err(Error::UnsupportedCommand));
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn every_record_truncation_leaves_intent_available_for_valid_retry() {
    let frame = data(9, 1, &query(1, false), 0);
    for end in 0..frame.len() {
        let mut s = session();
        let id = offer(&mut s);
        assert_eq!(id, 1);
        assert!(
            s.ingest(1, &frame[..end]).is_err(),
            "accepted truncation {end}"
        );
        assert_eq!(s.in_flight(), 0);
        receive(&mut s, id, &query(1, false));
    }
}

#[test]
fn deterministic_record_mutations_never_panic_or_exceed_bounds() {
    let frame = data(9, 1, &query(1, false), 0);
    for index in 0..frame.len() {
        for mask in [1, 0x80, 0xff] {
            let mut s = session();
            offer(&mut s);
            let mut changed = frame.clone();
            changed[index] ^= mask;
            let _ = s.ingest(1, &changed);
            assert!(s.in_flight() <= 1);
        }
    }
}

#[test]
fn pending_control_completion_is_not_starved_by_peer_intent_requests() {
    let mut s = session();
    s.start_control(1).unwrap();
    let id = offer(&mut s);
    assert!(matches!(s.ingest(1, &data(9, id, &qrtr_control(2, [0; 4]), 0)),
        Ok(Event::QrtrControl(qcom_qrtr_control::Event::Hello { .. }))));
    assert_eq!(s.ingest(1, &control(7, 52)),
        Ok(Event::ReceiveIntentRequest { granted: true }));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    {
        assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
        assert_eq!(s.ingest(1, &control(7, 0)), Ok(Event::ReceiveIntentRequest { granted: false }));
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
    }
    #[cfg(not(feature = "sunfish-wlan-intent-ack-turn"))]
    {
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
        assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
        let mut ack = false;
        for _ in 0..qcom_glink_qmi::RX_SLOTS {
            match publish(&mut s).unwrap().0 {
                Published::ReceiveIntent(_) => {},
                Published::IntentRequestAck { granted: true } => { ack = true; break; },
                other => panic!("unexpected publication before intent ACK: {other:?}"),
            }
        }
        assert!(ack);
    }
}

#[test]
fn remote_intent_request_advertises_backing_before_acknowledging() {
    let mut s = session();
    assert_eq!(
        s.ingest(1, &control(7, 67)),
        Ok(Event::ReceiveIntentRequest { granted: true })
    );
    assert_eq!(s.ingest(1, &control(7, 67)), Err(Error::NoCapacity));
    let (event, bytes) = publish(&mut s).unwrap();
    assert_eq!(event, Published::ReceiveIntent(1));
    assert_eq!(word(&bytes, 8), 4096);
    let mut blocked = Vec::new();
    assert_eq!(
        s.publish_next(1, |record| {
            blocked.extend(record);
            Err(WriteError::WouldBlock)
        }),
        Err(Error::WouldBlock)
    );
    assert_eq!(blocked, vec![8, 0, 1, 0, 1, 0, 0, 0]);
    assert_eq!(
        publish(&mut s).unwrap(),
        (Published::IntentRequestAck { granted: true }, blocked)
    );
    receive(&mut s, 1, &query(1, false));
}

#[test]
fn intent_request_denial_is_bounded_and_creates_no_receive_buffer() {
    let mut s = session();
    for size in [0, 4097, u32::MAX] {
        assert_eq!(
            s.ingest(1, &control(7, size)),
            Ok(Event::ReceiveIntentRequest { granted: false })
        );
        assert_eq!(
            publish(&mut s).unwrap(),
            (
                Published::IntentRequestAck { granted: false },
                vec![8, 0, 1, 0, 0, 0, 0, 0]
            )
        );
    }
    for _ in 0..qcom_glink_qmi::RX_SLOTS {
        offer(&mut s);
    }
    assert_eq!(
        s.ingest(1, &control(7, 52)),
        Ok(Event::ReceiveIntentRequest { granted: false })
    );
    assert_eq!(
        publish(&mut s).unwrap().0,
        Published::IntentRequestAck { granted: false }
    );
    assert!(publish(&mut s).is_none());
}

#[test]
fn intent_denial_retains_size_and_each_receive_state() {
    let mut invalid = session();
    assert_eq!(invalid.ingest(1, &control(7, 4097)),
        Ok(Event::ReceiveIntentRequest { granted: false }));
    let detail = invalid.intent_denial_diagnostic().unwrap();
    assert_eq!((detail.requested, detail.states), (4097, 0));

    let mut full = session();
    for _ in 0..qcom_glink_qmi::RX_SLOTS { offer(&mut full); }
    assert_eq!(full.ingest(1, &control(7, 52)),
        Ok(Event::ReceiveIntentRequest { granted: false }));
    let detail = full.intent_denial_diagnostic().unwrap();
    assert_eq!((detail.requested, detail.states), (52, 0x2222_2222));
}

#[test]
fn receive_snapshot_tracks_local_slot_service_and_reply() {
    let mut s = session();
    assert_eq!(s.receive_snapshot(), qcom_glink_qmi::ReceiveSnapshot { states: 0, services: 0, replies: 0 });
    let id = offer(&mut s);
    assert_eq!(s.receive_snapshot().states & 15, 2);
    let handle = receive(&mut s, id, &open(1));
    let snapshot = s.receive_snapshot();
    assert_eq!((snapshot.states & 15, snapshot.services & 15, snapshot.replies & 15), (4, 1, 0));
    s.queue_reply(&handle, Reply::Failure { error: 3 }).unwrap();
    let snapshot = s.receive_snapshot();
    assert_eq!(snapshot.states & 15, 5);
    assert_eq!(snapshot.services & 15, 1);
    assert_eq!(snapshot.replies & 1, 1);
}

#[test]
fn waiting_large_reply_does_not_block_small_qrtr_credit() {
    let mut s = session();
    let first = request(&mut s, 1);
    let mut fields = vec![0x10];
    fields.extend(500u16.to_le_bytes());
    fields.extend(vec![0; 500]);
    s.queue_reply(
        &first,
        Reply::Success {
            optional_tlvs: &fields,
        },
    )
    .unwrap();
    let id = offer(&mut s);
    receive(&mut s, id, &query(2, true));
    s.ingest(1, &intents(&[(1, 52)])).unwrap();
    #[cfg(not(feature = "sunfish-glink-tx-fairness-experiment"))]
    for _ in 0..2 {
        assert!(matches!(
            publish(&mut s).unwrap().0,
            Published::ReceiveDone(_)
        ));
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        assert_eq!(s.outbound_intent_attempt_diagnostic(), (0, 0),
            "a sendable small credit keeps a larger intent request ineligible while receive completions run");
    }
    #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    let (_, frame) = publish(&mut s).unwrap();
    assert_eq!(word(&frame, 20), 7); // QRTR type at GLINK payload + 4.
    #[cfg(all(feature = "sunfish-glink-tx-fairness-experiment", feature = "sunfish-glink-intent-request-experiment"))]
    assert!(matches!(publish(&mut s).unwrap().0, Published::RemoteIntentRequest { .. }));
    #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
    assert_eq!(
        s.publish_next(1, |_| panic!("large intent still absent")),
        Err(Error::NoRemoteIntent)
    );
    #[cfg(all(feature = "sunfish-glink-intent-request-experiment", not(feature = "sunfish-glink-tx-fairness-experiment")))]
    assert!(matches!(publish(&mut s).unwrap().0, Published::RemoteIntentRequest { .. }));
    s.ingest(1, &control(6, 1)).unwrap();
    s.ingest(1, &intents(&[(2, 4096)])).unwrap();
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    s.ingest(1, &control(8, 1)).unwrap();
    assert_eq!(
        publish(&mut s).unwrap().0,
        Published::Data {
            remote_intent: 2,
            last_fragment: false
        }
    );
}

#[test]
#[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
fn ready_credit_gets_a_turn_among_receive_completions() {
    let mut s = session();
    let first = request(&mut s, 1);
    let mut fields = vec![0x10];
    fields.extend(500u16.to_le_bytes());
    fields.extend(vec![0; 500]);
    s.queue_reply(&first, Reply::Success { optional_tlvs: &fields }).unwrap();
    let id = offer(&mut s);
    receive(&mut s, id, &query(2, true));
    s.ingest(1, &intents(&[(1, 52)])).unwrap();
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { remote_intent: 1, .. }));
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    assert!(matches!(publish(&mut s).unwrap().0, Published::RemoteIntentRequest { .. }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveDone(_)),
        "transmit must not suppress the remaining receive completion");
}

#[test]
fn literal_allocation_survives_both_wire_padding_layers() {
    let mut s = session();
    let id = offer(&mut s);
    // Same independent 67-byte allocation fixture as the core tests, padded
    // to 68 by QRTR. Neither its addresses nor its size cause an allocation.
    let mut bytes = vec![
        1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 1, 0, 0, 0, 35, 0, 0, 0, 1, 0, 0, 0, 1, 64,
        0, 0, 0, 255, 255, 34, 0, 28, 0, 1, 4, 0, 0, 0, 80, 0, 2, 4, 0, 1, 0, 0, 0, 3, 4, 0, 0, 0,
        0, 0, 4, 4, 0, 68, 51, 34, 17,
    ];
    bytes.push(0x5a);
    let handle = receive(&mut s, id, &bytes);
    let (context, decoded) = s.request(&handle).unwrap();
    assert_eq!((context.transaction, context.message_id), (0xffff, 0x22));
    assert!(matches!(
        decoded,
        Request::Memory(MemoryRequest::Allocate {
            bytes: 0x500000,
            sequence: 0x11223344,
            ..
        })
    ));
}

#[test]
fn mock_query_result_completes_full_request_reply_and_both_acknowledgements() {
    let mut s = session();
    let handle = request(&mut s, 17);
    assert!(matches!(
        s.request(&handle).unwrap().1,
        Request::Memory(MemoryRequest::QuerySize { client: 1, .. })
    ));
    // Mock backend result only. This test grants no physical memory ownership.
    let size_field = [0x10, 4, 0, 0, 0, 0x50, 0];
    s.queue_reply(
        &handle,
        Reply::Success {
            optional_tlvs: &size_field,
        },
    )
    .unwrap();
    s.ingest(1, &intents(&[(1, 128)])).unwrap();
    publish(&mut s).unwrap();
    let (_, frame) = publish(&mut s).unwrap();
    let len = word(&frame, 36) as usize; // QRTR body length in outer payload.
    let decoded = Message::decode(&frame[48..48 + len]).unwrap();
    assert_eq!(decoded.transaction, 17);
    assert_eq!(decoded.u32(0x10).unwrap(), 5 * 1024 * 1024);
    s.ingest(1, &control(6, 1)).unwrap();
    let id = credit(&mut s, MEMORY);
    assert_eq!(s.in_flight(), 0);
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    assert!(publish(&mut s).is_none());
    // The transaction ID can now be used by a later request.
    request(&mut s, 17);
}

#[test]
fn observed_thirty_intent_inventory_fits_and_selects_large_peer_buffer() {
    let mut s = session();
    let handle = request(&mut s, 1);
    // Counts/capacities from the user's IPCRTR INTENT DETAILS screen.
    let mut entries = Vec::new();
    for (count, capacity) in [(16, 0x80), (8, 0x200), (5, 0x480), (1, 0x2080)] {
        for _ in 0..count {
            entries.push((entries.len() as u32, capacity));
        }
    }
    assert_eq!(entries.len(), 30);
    assert_eq!(s.ingest(1, &intents(&entries)), Ok(Event::RemoteIntents));
    let mut fields = vec![0x10];
    fields.extend(1600u16.to_le_bytes());
    fields.extend(vec![0; 1600]);
    s.queue_reply(
        &handle,
        Reply::Success {
            optional_tlvs: &fields,
        },
    )
    .unwrap();
    publish(&mut s).unwrap();
    assert_eq!(
        publish(&mut s).unwrap().0,
        Published::Data {
            remote_intent: 29,
            last_fragment: false
        }
    );
}

#[test]
fn unwinding_writer_leaves_publication_quarantined() {
    let mut s = session();
    queued_reply(&mut s);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = s.publish_next(1, |_| panic!("writer lost publication outcome"));
    }));
    assert!(unwind.is_err());
    assert_eq!(
        s.publish_next(1, |_| panic!("must remain stopped")),
        Err(Error::Quarantined)
    );
}


#[cfg(all(feature = "sunfish-tftp-final-ack-observer", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn final_ack_trace_matches_route_block_and_exact_transmit_lifetime() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)).unwrap();
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let route = TftpReplyRoute { source: Address { node: 1, port: 0x5002 }, destination: Address { node: 0, port: 0x56 } };
    let other = TftpReplyRoute { destination: Address { port: 0x57, ..route.destination }, ..route };
    let ack = [0, 4, 0, 1];
    s.observe_tftp_final_ack(route, &[0, 6, 0, 1]);
    s.observe_tftp_final_ack(route, &[0, 4, 0]);
    assert!(s.tftp_final_ack_trace().is_none());
    s.observe_tftp_final_ack(route, &ack);
    for (r, packet) in [(other, &ack[..]), (TftpReplyRoute { source: Address { port: 0x5003, ..route.source }, ..route }, &ack[..]), (route, &[0, 4, 0, 2][..]), (route, qcom_tftp_server_check::OACK)] {
        s.queue_tftp_response(1, r, packet).unwrap();
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("reply expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
        assert_eq!(s.tftp_final_ack_trace().unwrap().flags, 1, "unrelated route, block or opcode must not advance trace");
    }
    s.queue_tftp_response(1, route, &ack).unwrap();
    assert_eq!(s.tftp_final_ack_trace().unwrap().flags, 3);
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(s.tftp_final_ack_trace().unwrap().flags, 3);
    let (Published::Data { remote_intent: target, .. }, _) = publish_until_data(&mut s) else { panic!("final ACK expected") };
    assert_eq!(s.tftp_final_ack_trace().unwrap().flags, 15);
    s.queue_tftp_response(1, other, &ack).unwrap();
    let (Published::Data { remote_intent: unrelated, .. }, _) = publish_until_data(&mut s) else { panic!("other ACK expected") };
    s.ingest(1, &control(14, unrelated)).unwrap();
    assert_eq!(s.tftp_final_ack_trace().unwrap().flags, 15, "other peer's completion is not our ACK");
    s.ingest(1, &control(14, target)).unwrap();
    let saved = s.tftp_final_ack_trace().unwrap();
    assert_eq!((saved.flags, saved.remote_intent), (31, target));
    s.observe_tftp_final_ack(other, &[0, 4, 0, 2]);
    s.queue_tftp_response(1, other, &ack).unwrap();
    let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("reused slot expected") };
    s.ingest(1, &control(14, remote_intent)).unwrap();
    assert_eq!(s.tftp_final_ack_trace(), Some(saved), "slot reuse and later transfers cannot overwrite the trace");
    s.reset_after_transport_reset(2).unwrap();
    assert_eq!(s.tftp_final_ack_trace(), None, "old generation trace must be cleared");
}
#[cfg(all(feature = "sunfish-tftp-missing-temp-replies", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
#[test]
fn missing_reply_trace_matches_route_bytes_and_exact_transmit_lifetime() {
    use qcom_glink_qmi::TftpReplyRoute;
    use qcom_qmi_router::Address;
    let mut s = Session::new(config(), 1).unwrap();
    s.prepare_tftp_server(1, 0x5000).unwrap(); s.start_control(1).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096), (9, 4096)])).unwrap();
    let hello = offer(&mut s);
    s.ingest(1, &data(9, hello, &qrtr_control(2, [0; 4]), 0)).unwrap();
    for _ in 0..2 {
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("control expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
    }
    let route = TftpReplyRoute { source: Address { node: 1, port: 0x5002 }, destination: Address { node: 0, port: 0x56 } };
    let other = TftpReplyRoute { destination: Address { port: 0x57, ..route.destination }, ..route };
    let ack = *b"\0\x05\0\x01file not found\0";
    s.observe_tftp_missing_reply(route);
    for (r, packet) in [(other, &ack[..]), (TftpReplyRoute { source: Address { port: 0x5003, ..route.source }, ..route }, &ack[..]), (route, &[0, 4, 0, 2][..]), (route, qcom_tftp_server_check::OACK)] {
        s.queue_tftp_response(1, r, packet).unwrap();
        let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("reply expected") };
        s.ingest(1, &control(14, remote_intent)).unwrap();
        assert_eq!(s.tftp_missing_reply_trace().unwrap().flags, 1, "unrelated route, block or opcode must not advance trace");
    }
    s.queue_tftp_response(1, route, &ack).unwrap();
    assert_eq!(s.tftp_missing_reply_trace().unwrap().flags, 3);
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(s.tftp_missing_reply_trace().unwrap().flags, 3);
    let (Published::Data { remote_intent: target, .. }, _) = publish_until_data(&mut s) else { panic!("final ACK expected") };
    assert_eq!(s.tftp_missing_reply_trace().unwrap().flags, 15);
    s.queue_tftp_response(1, other, &ack).unwrap();
    let (Published::Data { remote_intent: unrelated, .. }, _) = publish_until_data(&mut s) else { panic!("other ACK expected") };
    s.ingest(1, &control(14, unrelated)).unwrap();
    assert_eq!(s.tftp_missing_reply_trace().unwrap().flags, 15, "other peer's completion is not our ACK");
    s.ingest(1, &control(14, target)).unwrap();
    let saved = s.tftp_missing_reply_trace().unwrap();
    assert_eq!((saved.flags, saved.remote_intent), (31, target));
    s.observe_tftp_missing_reply(other);
    s.queue_tftp_response(1, other, &ack).unwrap();
    let (Published::Data { remote_intent, .. }, _) = publish_until_data(&mut s) else { panic!("reused slot expected") };
    s.ingest(1, &control(14, remote_intent)).unwrap();
    assert_eq!(s.tftp_missing_reply_trace(), Some(saved), "slot reuse and later transfers cannot overwrite the trace");
    s.reset_after_transport_reset(2).unwrap();
    assert_eq!(s.tftp_missing_reply_trace(), None, "old generation trace must be cleared");
}

#[cfg(all(
    feature = "sunfish-wlan-elf-publish-trace",
    feature = "sunfish-glink-intent-request-experiment",
))]
#[test]
#[cfg(not(feature = "sunfish-wlan-registry-second-reply"))]
fn registry_blocker_trace_separates_intent_wait_and_fifo_backpressure() {
    let mut session = session();
    let port = crate::qcom_servreg::LOCAL_PORT;
    session.register(1, Service::ServiceRegistry, port).unwrap();
    session.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    let first_intent = offer(&mut session);
    let second_intent = offer(&mut session);
    let request = |session: &mut Session, intent, transaction: u8| {
        let payload = [0, transaction, 0, 0x21, 0, 6, 0, 1, 3, 0, b'f', b'o', b'o'];
        match session.ingest(1, &data(9, intent, &qrtr(1, 9, port, false, &payload), 0)).unwrap() {
            Event::Request(handle) => handle,
            event => panic!("expected registry request, got {event:?}"),
        }
    };
    let first = request(&mut session, first_intent, 0x34);
    let second = request(&mut session, second_intent, 0x35);
    let reply = || Reply::Failure { error: 3 };
    session.queue_reply(&first, reply()).unwrap();
    let sample = |session: &mut Session| {
        assert_eq!(session.queue_reply(&second, reply()), Err(Error::FlowBusy));
        session.registry_queue_pressure(&second).unwrap().peer_publication.unwrap()
    };
    let queued = sample(&mut session);
    assert_eq!((queued.transaction, queued.bytes, queued.calls, queued.mask), (0x34, 48, 0, 0));
    assert!(matches!(publish(&mut session).unwrap().0, Published::RemoteIntentRequest { size: 48 }));
    assert!(matches!(publish(&mut session).unwrap().0, Published::ReceiveDone(_)));
    assert!(matches!(publish(&mut session).unwrap().0, Published::ReceiveDone(_)));
    session.ingest(1, &intents(&[(9, 48)])).unwrap();
    assert_eq!(session.publish_next(1, |_| panic!("grant is still pending")), Err(Error::NoRemoteIntent));
    let waiting = sample(&mut session);
    assert_eq!((waiting.calls, waiting.mask), (4, 0x47));

    session.ingest(1, &control(8, 1)).unwrap();
    assert_eq!(session.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    let blocked = sample(&mut session);
    assert_eq!((blocked.calls, blocked.mask), (5, 0x57));

    assert!(matches!(publish(&mut session).unwrap().0, Published::Data { remote_intent: 9, .. }));
    let published = sample(&mut session);
    let priority = if cfg!(feature = "sunfish-wlan-registry-tx-turn") { 0x80 } else { 0 };
    assert_eq!((published.calls, published.mask), (6, 0x77 | priority));
    session.ingest(1, &control(6, 9)).unwrap();
    session.queue_reply(&second, reply()).unwrap();
}

#[cfg(feature = "sunfish-wlan-registry-tx-turn")]
fn registry_turn_request(s: &mut Session, intent: u32, peer: u32, txn: u8) -> RequestHandle {
    let payload = [0, txn, 0, 0x21, 0, 6, 0, 1, 3, 0, b'f', b'o', b'o'];
    let packet = qrtr(1, peer, qcom_servreg::LOCAL_PORT, false, &payload);
    match s.ingest(1, &data(9, intent, &packet, 0)).unwrap() {
        Event::Request(handle) => handle,
        event => panic!("expected registry request, got {event:?}"),
    }
}

#[cfg(feature = "sunfish-wlan-registry-tx-turn")]
#[test]
#[cfg(not(feature = "sunfish-wlan-registry-second-reply"))]
fn registry_priority_is_bounded_and_preserves_same_peer_completion() {
    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let first_id = offer(&mut s);
    let second_id = offer(&mut s);
    let later_id = offer(&mut s);
    let first = registry_turn_request(&mut s, first_id, 9, 4);
    let second = registry_turn_request(&mut s, second_id, 10, 4);
    let later = registry_turn_request(&mut s, later_id, 9, 5);
    for id in [first_id, second_id, later_id] {
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    }
    let advertising = s.offer_receive(1).unwrap();
    let reply = || Reply::Failure { error: 3 };
    s.queue_reply(&first, reply()).unwrap();
    s.queue_reply(&second, reply()).unwrap();
    assert_eq!(s.queue_reply(&later, reply()), Err(Error::FlowBusy));
    assert_eq!(s.registry_queue_pressure(&later).unwrap().peer_request, Some(first));

    let mut blocked_frame = Vec::new();
    assert_eq!(s.publish_next(1, |record| {
        blocked_frame.extend_from_slice(record);
        Err(WriteError::WouldBlock)
    }), Err(Error::WouldBlock));
    assert!(!s.reply_fully_published(&first));
    let (event, frame) = publish(&mut s).unwrap();
    assert!(matches!(event, Published::Data { remote_intent: 7, .. }));
    assert_eq!(frame, blocked_frame);
    assert_eq!(word(&frame, 44), 9);
    assert_eq!(s.queue_reply(&later, reply()), Err(Error::FlowBusy));
    let first_trace = s.registry_queue_pressure(&later).unwrap().peer_publication.unwrap();
    assert_eq!(first_trace.mask & 0xb0, 0xb0);
    #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
    assert_eq!(first_trace.turn_gates, [0, 0, 0, 0, 0, 0, 0, 2]);
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(advertising));
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { remote_intent: 8, .. }));
    assert_eq!(s.ingest(1, &control(6, 7)), Ok(Event::ReplyRxDone(first)));
    s.queue_reply(&later, reply()).unwrap();
}

#[cfg(feature = "sunfish-wlan-registry-tx-turn")]
#[test]
fn registry_priority_preserves_receive_done_and_pending_intent_ack() {
    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    let id = offer(&mut s);
    let request = registry_turn_request(&mut s, id, 9, 4);
    s.queue_reply(&request, Reply::Failure { error: 3 }).unwrap();
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    assert_eq!(s.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { remote_intent: 7, .. }));
}

#[cfg(all(feature = "sunfish-wlan-registry-second-reply", feature = "sunfish-wlan-registry-tx-turn"))]
#[test]
fn bounded_registry_second_reply_preserves_order_receipts_and_credit() {
    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
    let first_id = offer(&mut s);
    let second_id = offer(&mut s);
    let third_id = offer(&mut s);
    let first = registry_turn_request(&mut s, first_id, 9, 1);
    let second = registry_turn_request(&mut s, second_id, 9, 2);
    let third = registry_turn_request(&mut s, third_id, 9, 3);
    let reply = || Reply::Failure { error: 3 };

    s.queue_reply(&first, reply()).unwrap();
    assert_eq!(s.queue_reply(&second, reply()), Err(Error::FlowBusy));
    let (sent, frame) = publish_until_data(&mut s);
    assert!(matches!(sent, Published::Data { remote_intent: 7, last_fragment: true }));
    assert_eq!(&frame[49..51], &1u16.to_le_bytes());
    s.queue_reply(&second, reply()).unwrap();
    assert_eq!(s.queue_reply(&third, reply()), Err(Error::FlowBusy));
    let (sent, frame) = publish_until_data(&mut s);
    assert!(matches!(sent, Published::Data { remote_intent: 8, last_fragment: true }));
    assert_eq!(&frame[49..51], &2u16.to_le_bytes());
    assert!(s.reply_fully_published(&first));
    assert!(s.reply_fully_published(&second));

    assert_eq!(s.ingest(1, &control(14, 8)), Ok(Event::ReplyRxDone(second)));
    assert!(s.reply_fully_published(&first));
    assert!(!s.reply_fully_published(&second));
    s.queue_reply(&third, reply()).unwrap();
    assert!(matches!(publish_until_data(&mut s).0, Published::Data { remote_intent: 8, .. }));
    assert_eq!(s.ingest(1, &control(14, 7)), Ok(Event::ReplyRxDone(first)));
    assert_eq!(s.ingest(1, &control(14, 8)), Ok(Event::ReplyRxDone(third)));

    for txn in 4..=10 {
        let id = offer(&mut s);
        let request = registry_turn_request(&mut s, id, 9, txn);
        s.queue_reply(&request, reply()).unwrap();
        let (sent, frame) = publish_until_data(&mut s);
        assert!(matches!(sent, Published::Data { remote_intent: 7, .. }));
        assert_eq!(word(&frame, 32), u32::from(txn == 5));
        if txn < 10 {
            assert_eq!(s.ingest(1, &control(14, 7)), Ok(Event::ReplyRxDone(request)));
        }
    }
    let id = offer(&mut s);
    let next = registry_turn_request(&mut s, id, 9, 11);
    assert_eq!(s.queue_reply(&next, reply()), Err(Error::FlowBusy));
    assert_eq!(s.registry_queue_pressure(&next).unwrap().reason, 4);
}

#[cfg(all(feature = "sunfish-wlan-registry-ack-trace", not(feature = "sunfish-wlan-intent-ack-turn")))]
#[test]
fn registry_ack_trace_counts_completed_and_held_acks_separately() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(0);

    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.set_registry_ack_clock(|| CLOCK.load(Relaxed));
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    let first_id = offer(&mut s);
    let later_id = offer(&mut s);
    let first = registry_turn_request(&mut s, first_id, 9, 3);
    let later = registry_turn_request(&mut s, later_id, 9, 4);
    let reply = || Reply::Failure { error: 3 };
    s.queue_reply(&first, reply()).unwrap();

    CLOCK.store(100, Relaxed);
    assert_eq!(s.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
    for id in [first_id, later_id] {
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    }
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
    CLOCK.store(160, Relaxed);
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    let sample = |s: &mut Session| {
        assert_eq!(s.queue_reply(&later, reply()), Err(Error::FlowBusy));
        s.registry_queue_pressure(&later).unwrap()
    };
    let held = sample(&mut s);
    assert_eq!(held.pending_ack_age, Some(60));
    let trace = held.peer_publication.unwrap().ack;
    assert_eq!((trace.sent, trace.blocked, trace.done_ahead, trace.ads_ahead, trace.max_age),
               (0, 1, 2, 1, None));

    CLOCK.store(190, Relaxed);
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    assert_eq!(s.ingest(1, &control(7, 68)), Ok(Event::ReceiveIntentRequest { granted: true }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::ReceiveIntent(_)));
    CLOCK.store(220, Relaxed);
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    let sent = sample(&mut s);
    assert_eq!(sent.pending_ack_age, None);
    let trace = sent.peer_publication.unwrap().ack;
    assert_eq!((trace.sent, trace.blocked, trace.done_ahead, trace.ads_ahead, trace.max_age),
               (2, 1, 2, 2, Some(90)));
}

#[cfg(feature = "sunfish-wlan-intent-ack-turn")]
#[test]
fn intent_ack_turn_sends_exact_backing_and_yields_to_ordinary_work() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(0);

    let mut s = session();
    s.set_registry_ack_clock(|| CLOCK.load(Relaxed));
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    let id = offer(&mut s);
    let ordinary = s.offer_receive(1).unwrap();
    let request = registry_turn_request(&mut s, id, 9, 3);
    s.queue_reply(&request, Reply::Failure { error: 3 }).unwrap();

    CLOCK.store(100, Relaxed);
    assert_eq!(s.ingest(1, &control(7, 67)), Ok(Event::ReceiveIntentRequest { granted: true }));
    let mut attempted = Vec::new();
    assert_eq!(s.publish_next(1, |record| {
        attempted.extend_from_slice(record);
        Err(WriteError::WouldBlock)
    }), Err(Error::WouldBlock));
    assert_eq!(s.intent_ack_summary(), (0, None));
    assert_eq!(publish(&mut s).unwrap(), (Published::ReceiveIntent(ordinary + 1), attempted));

    CLOCK.store(110, Relaxed);
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    assert_eq!(s.intent_ack_summary(), (1, Some(10)));
    assert_eq!(s.ingest(1, &control(7, 0)), Ok(Event::ReceiveIntentRequest { granted: false }));
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
    assert_eq!(s.ingest(1, &control(7, 0)), Ok(Event::ReceiveIntentRequest { granted: false }));
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(ordinary));
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
    assert_eq!(s.ingest(1, &control(7, 0)), Ok(Event::ReceiveIntentRequest { granted: false }));
    assert!(matches!(publish(&mut s).unwrap().0, Published::Data { remote_intent: 7, .. }));
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
    assert_eq!(s.intent_ack_summary(), (4, Some(10)));
}

#[cfg(feature = "sunfish-wlan-intent-ack-turn")]
#[test]
fn replay_full_52_byte_request_then_receive_done() {
    for block_done in [false, true] {
        let mut s = session();
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        s.set_registry_ack_clock(|| 17);
        for _ in 1..qcom_glink_qmi::RX_SLOTS { offer(&mut s); }
        assert_eq!(s.ingest(1, &control(7, 52)), Ok(Event::ReceiveIntentRequest { granted: true }));
        let Published::ReceiveIntent(id) = publish(&mut s).unwrap().0 else { panic!() };
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
        assert_eq!(s.ingest(1, &data(9, id, &resume_route(0xdead_0000, STORAGE), 0)), Ok(Event::CreditReceived));
        assert_eq!(s.ingest(1, &control(7, 52)), Ok(Event::ReceiveIntentRequest { granted: false }));
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        {
            assert_eq!(s.grant_backing.requested, 52);
            assert!(s.grant_backing.intent.is_none() && s.grant_backing.reserved_at.is_none());
            s.set_registry_ack_clock(|| 31);
        }
        if block_done {
            assert_eq!(s.publish_next(1, |bytes| {
                assert_eq!(word(bytes, 0) & 0xffff, 6);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
        }
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        assert!(s.grant_backing.reserved_at.is_none());
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        {
            assert_eq!(s.grant_backing.intent, Some(id + 1));
            assert_eq!(s.grant_backing.reserved_at, Some(31));
            s.set_registry_ack_clock(|| 46);
        }
        if cfg!(feature = "sunfish-glink-retry-full-intent") {
            assert_eq!(s.publish_next(1, |bytes| {
                assert_eq!(word(bytes, 0) & 0xffff, 5);
                assert_eq!(word(bytes, 12), id + 1);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            #[cfg(feature = "sunfish-grant-ad-writer-trace")]
            assert!(s.grant_backing.writer.is_none() && s.grant_backing.advertised_at.is_none());
            assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(id + 1));
            #[cfg(feature = "sunfish-grant-ad-writer-trace")]
            {
                assert_eq!(s.grant_backing.writer, Some(qcom_glink_qmi::GrantAdWriter::Ordinary));
                assert_eq!(s.grant_backing.advertised_at, Some(46));
            }
        }
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: cfg!(feature = "sunfish-glink-retry-full-intent") });
        assert!(publish(&mut s).is_none());
    }
}

#[cfg(all(feature = "sunfish-wlan-registry-tx-turn", feature = "sunfish-glink-retry-full-intent"))]
#[test]
fn replay_registry_slot_reused_before_reply_admission() {
    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();

    for _ in 1..qcom_glink_qmi::RX_SLOTS { offer(&mut s); }
    assert_eq!(s.ingest(1, &control(7, 56)), Ok(Event::ReceiveIntentRequest { granted: true }));
    let Published::ReceiveIntent(id) = publish(&mut s).unwrap().0 else { panic!() };
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    let original = registry_turn_request(&mut s, id, 9, 4);
    assert_eq!(s.ingest(1, &control(7, 52)), Ok(Event::ReceiveIntentRequest { granted: false }));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));

    let replacement = if cfg!(feature = "sunfish-glink-registry-early-release") {
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(id + 1));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
        Some(registry_turn_request(&mut s, id + 1, 10, 5))
    } else {
        assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
        None
    };
    let (context, request) = s.request(&original).unwrap();
    assert_eq!(context.transaction, 4);
    assert!(matches!(request, Request::ServiceRegistry(query) if query.service_name == b"foo"));
    s.queue_reply(&original, Reply::Failure { error: 3 }).unwrap();
    let (Published::Data { remote_intent, .. }, bytes) = publish_until_data(&mut s) else { panic!() };
    assert_eq!(u16::from_le_bytes([bytes[49], bytes[50]]), 4);
    assert_eq!(s.ingest(1, &control(6, remote_intent)), Ok(Event::ReplyRxDone(original)));
    if let Some(replacement) = replacement {
        assert_eq!(s.request(&replacement).unwrap().0.transaction, 5);
    }
}

#[cfg(all(feature = "sunfish-wlan-registry-tx-turn", feature = "sunfish-glink-retry-full-intent"))]
#[test]
fn replay_registry_reply_completes_after_receive_slot_is_reused() {
    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096)])).unwrap();
    for _ in 1..qcom_glink_qmi::RX_SLOTS { offer(&mut s); }
    assert_eq!(s.ingest(1, &control(7, 56)), Ok(Event::ReceiveIntentRequest { granted: true }));
    let Published::ReceiveIntent(id) = publish(&mut s).unwrap().0 else { panic!() };
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    let original = registry_turn_request(&mut s, id, 9, 4);
    s.queue_reply(&original, Reply::Failure { error: 3 }).unwrap();
    assert_eq!(s.ingest(1, &control(7, 52)), Ok(Event::ReceiveIntentRequest { granted: false }));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));

    let early_reply = if cfg!(feature = "sunfish-registry-converted-grant-turn") {
        Some(publish(&mut s).unwrap())
    } else { None };
    let replacement = if cfg!(feature = "sunfish-glink-registry-early-release") {
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(id + 1));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
        Some(registry_turn_request(&mut s, id + 1, 10, 5))
    } else {
        assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
        None
    };
    let (event, bytes) = early_reply.unwrap_or_else(|| publish_until_data(&mut s));
    assert!(matches!(event, Published::Data { remote_intent: 7, .. }));
    assert_eq!(u16::from_le_bytes([bytes[49], bytes[50]]), 4);
    assert_eq!(s.ingest(1, &control(6, 7)), Ok(Event::ReplyRxDone(original)));
    if let Some(replacement) = replacement {
        assert_eq!(s.request(&replacement).unwrap().0.transaction, 5);
    }
}

#[cfg(all(feature = "sunfish-glink-registry-early-release", feature = "sunfish-wlan-registry-tx-turn"))]
#[test]
fn replay_registry_move_entry_occupied() {
    let mut s = session();
    s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
    s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();

    // Queue an older reply to the same peer to force FlowBusy for transaction 4.
    let older_id = offer(&mut s);
    for _ in 1..qcom_glink_qmi::RX_SLOTS { offer(&mut s); }
    let older = registry_turn_request(&mut s, older_id, 9, 3);
    // Keep an unpublished predecessor for the FlowBusy/reused-slot replay.
    // The queue-order-independent child now arms this order too; spend its
    // one-shot with a blocked write, then exercise the normal grant fallback.
    if cfg!(feature="sunfish-registry-ready-handoffs"){assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(older_id));}
    s.queue_reply(&older, Reply::Failure { error: 3 }).unwrap();
    if !cfg!(feature="sunfish-registry-ready-handoffs"){assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(older_id));}
    assert_eq!(s.ingest(1, &control(7, 56)), Ok(Event::ReceiveIntentRequest { granted: true }));
    #[cfg(feature="sunfish-registry-elf-pressure")]
    assert_eq!(s.publish_next(1, |b| {
        assert_eq!(word(b, 0) & 0xffff, 9);
        assert_eq!(&b[49..51], &3u16.to_le_bytes());
        Err(WriteError::WouldBlock)
    }), Err(Error::WouldBlock));
    let Published::ReceiveIntent(id) = publish(&mut s).unwrap().0 else { panic!() };
    assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
    let original = registry_turn_request(&mut s, id, 9, 4);
    assert_eq!(s.queue_reply(&original, Reply::Failure { error: 3 }), Err(Error::FlowBusy));
    assert_eq!(s.ingest(1, &control(7, 52)), Ok(Event::ReceiveIntentRequest { granted: false }));
    assert_eq!(s.publish_next(1, |_| Err(WriteError::WouldBlock)), Err(Error::WouldBlock));
    assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
    assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));

    let early_older = if cfg!(feature = "sunfish-registry-converted-grant-turn")
        && !cfg!(feature = "sunfish-registry-elf-pressure") {
        let (Published::Data { remote_intent, .. }, b) = publish(&mut s).unwrap() else { panic!("converted grant reply expected") };
        assert_eq!(&b[49..51], &3u16.to_le_bytes()); Some(remote_intent)
    } else { None };
    let replacement = if cfg!(feature = "sunfish-glink-registry-second-host-entry") {
        assert_eq!(s.publish_next(1, |bytes| {
            assert_eq!(word(bytes, 0) & 0xffff, 5);
            assert_eq!(word(bytes, 12), id + 1);
            Err(WriteError::WouldBlock)
        }), Err(Error::WouldBlock));
        assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(id + 1));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
        Some(registry_turn_request(&mut s, id + 1, 10, 5))
    } else {
        assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
        None
    };
    let (context, request) = s.request(&original).unwrap();
    assert_eq!(context.transaction, 4);
    assert!(matches!(request, Request::ServiceRegistry(query) if query.service_name == b"foo"));
    if let Some(older_remote) = early_older {
        s.queue_reply(&original, Reply::Failure { error: 3 }).unwrap();
        let (Published::Data { remote_intent, .. }, b) = publish_until_data(&mut s) else { panic!() };
        assert_eq!(&b[49..51], &4u16.to_le_bytes());
        assert_eq!(s.ingest(1, &control(6, remote_intent)), Ok(Event::ReplyRxDone(original)));
        assert_eq!(s.ingest(1, &control(6, older_remote)), Ok(Event::ReplyRxDone(older)));
    } else {
    assert_eq!(s.queue_reply(&original, Reply::Failure { error: 3 }), Err(Error::FlowBusy));
    let (Published::Data { remote_intent, .. }, older_bytes) = publish_until_data(&mut s) else { panic!() };
    assert_eq!(u16::from_le_bytes([older_bytes[49], older_bytes[50]]), 3);
    assert_eq!(s.ingest(1, &control(6, remote_intent)), Ok(Event::ReplyRxDone(older)));
    s.queue_reply(&original, Reply::Failure { error: 3 }).unwrap();
    let (Published::Data { remote_intent, .. }, bytes) = publish_until_data(&mut s) else { panic!() };
    assert_eq!(u16::from_le_bytes([bytes[49], bytes[50]]), 4);
    assert_eq!(s.ingest(1, &control(6, remote_intent)), Ok(Event::ReplyRxDone(original)));
    }
    if let Some(replacement) = replacement {
        assert_eq!(s.request(&replacement).unwrap().0.transaction, 5);
    } else {
        assert!(s.offer_receive(1).is_ok());
    }
}

#[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
#[test]
fn preferred_negative_ack_can_borrow_one_slot_releasing_done() {
    let candidate = cfg!(feature = "sunfish-glink-denial-rxdone-turn");
    for block_done in [false, true] {
        let mut s = session();
        for _ in 0..qcom_glink_qmi::RX_SLOTS - 2 {
            offer(&mut s);
        }
        assert_eq!(s.ingest(1, &control(7, 52)),
            Ok(Event::ReceiveIntentRequest { granted: true }));
        let Published::ReceiveIntent(id) = publish(&mut s).unwrap().0
        else { panic!("expected backing advertisement"); };
        assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });

        // A successful non-ACK write clears the ACK's yield turn.
        let last = offer(&mut s);
        assert_eq!(s.ingest(1, &data(9, id, &resume_route(0xdead_0000, STORAGE), 0)),
            Ok(Event::CreditReceived));
        assert_eq!(s.ingest(1, &control(7, 108)),
            Ok(Event::ReceiveIntentRequest { granted: false }));

        if candidate && !block_done {
            assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
            assert_eq!(s.publish_next(1, |bytes| {
                assert_eq!(word(bytes, 0) & 0xffff, 5);
                assert_eq!(word(bytes, 12), last + 1);
                Err(WriteError::WouldBlock)
            }), Err(Error::WouldBlock));
            assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveIntent(last + 1));
            assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: true });
        } else {
            if candidate {
                assert_eq!(s.publish_next(1, |bytes| {
                    assert_eq!(word(bytes, 0) & 0xffff, 6);
                    assert_eq!(word(bytes, 4), id);
                    Err(WriteError::WouldBlock)
                }), Err(Error::WouldBlock));
                assert_eq!(s.offer_receive(1), Err(Error::NoCapacity));
            }
            assert_eq!(publish(&mut s).unwrap().0, Published::IntentRequestAck { granted: false });
            assert_eq!(publish(&mut s).unwrap().0, Published::ReceiveDone(id));
        }
        assert!(publish(&mut s).is_none());
    }
}

#[cfg(all(feature="sunfish-wlan-registry-second-reply",feature="sunfish-wlan-registry-tx-turn"))]
#[test]
fn registry_two_reply_window_retries_exact_write_and_retains_closed_peer_buffers(){
    for (reverse,close_peer) in [(false,false),(true,false),(false,true),(true,true)] {
        let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
        s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        s.start_control(1).unwrap();
        s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();
        let drain_control=|s:&mut Session|{for _ in 0..16 {
            let next=s.publish_next(1,|_|Ok(())).unwrap();
            if let Some(Published::Data{remote_intent,last_fragment:true})=next {s.ingest(1,&control(14,remote_intent)).unwrap();}
            if next.is_none(){return;}
        }panic!("control handshake did not drain")};
        drain_control(&mut s);
        let hello=offer(&mut s);s.ingest(1,&data(9,hello,&qrtr_control(2,[0;4]),0)).unwrap();
        drain_control(&mut s);
        let rx3=offer(&mut s);let rx4=offer(&mut s);
        let first=registry_turn_request(&mut s,rx3,9,3);
        let second=registry_turn_request(&mut s,rx4,9,4);
        s.queue_reply(&first,Reply::Failure{error:3}).unwrap();
        let (one,_)=publish_until_data(&mut s);
        let Published::Data{remote_intent:a,..}=one else{panic!("first response")};
        s.ingest(1,&control(7,52)).unwrap();
        s.queue_reply(&second,Reply::Failure{error:3}).unwrap();
        #[cfg(feature="sunfish-registry-window-trace")]
        {
            let window=s.registry_window_trace();assert_eq!(window.admissions,1);
            let pair=window.target.unwrap();assert_eq!((pair.older_transaction,pair.newer_transaction),(3,4));
            assert_eq!(pair.published_before_done,None);
        }
        let mut grant_advertised=false;let mut grant_acked=false;
        let mut blocked=None;
        for _ in 0..16 {
            let result=s.publish_next(1,|bytes|{
                if (word(bytes,0)&0xffff)==9 && bytes.get(49..51)==Some(&4u16.to_le_bytes()) {
                    blocked=Some(bytes.to_vec());Err(WriteError::WouldBlock)
                }else{Ok(())}
            });
            if blocked.is_some(){assert_eq!(result,Err(Error::WouldBlock));break;}
            match result.unwrap() {
                Some(Published::ReceiveIntent(_))=>grant_advertised=true,
                Some(Published::IntentRequestAck{granted:true})=>{assert!(grant_advertised);grant_acked=true;},
                _=>{}
            }
        }
        assert!(grant_acked,"positive grant after its advertisement must keep its turn");
        let blocked=blocked.expect("must block the actual second reply record");
        assert!(!s.reply_fully_published(&second));assert!(s.reply_fully_published(&first));
        let retry=s.publish_next(1,|bytes|{assert_eq!(bytes,blocked.as_slice());Ok(())}).unwrap();
        let Some(Published::Data{remote_intent:b,..})=retry else{panic!("second response")};
        assert_ne!(a,b);assert!(s.reply_fully_published(&first));assert!(s.reply_fully_published(&second));
        #[cfg(feature="sunfish-registry-window-trace")]
        assert_eq!(s.registry_window_trace().target.unwrap().published_before_done,Some(true),"proof is independent of the absent clock");
        if close_peer {
            let close=offer(&mut s);
            assert_eq!(s.ingest(1,&data(9,close,&qrtr_control(6,[0,9,0,0]),0)),Err(Error::Control(qcom_qrtr_control::Error::ClientGone)));
            assert!(s.reply_fully_published(&first));assert!(s.reply_fully_published(&second));
            let mut called=false;
            assert_eq!(s.publish_next(1,|_|{called=true;Ok(())}),Err(Error::Quarantined));assert!(!called);
            assert_eq!(s.ingest(1,&control(14,a)),Err(Error::Quarantined));
            continue;
        }
        assert!(s.reply_fully_published(&first));assert!(s.reply_fully_published(&second));
        let completions=if reverse{[(b,second,first),(a,first,second)]}else{[(a,first,second),(b,second,first)]};
        for (index,(intent,expected,other)) in completions.into_iter().enumerate(){
            assert_eq!(s.ingest(1,&control(14,intent)),Ok(Event::ReplyRxDone(expected)));
            assert!(!s.reply_fully_published(&expected));
            if index==0{assert!(s.reply_fully_published(&other),"close/other completion cannot free its intent");}
        }
    }
}

#[cfg(feature = "sunfish-registry-window-trace")]
#[test]
fn registry_window_trace_matches_receipts_without_relying_on_clock_or_slot_index() {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    for clocked in [false, true] {
        for complete_first in [false, true] {
            let mut s = session();
            if clocked { s.set_registry_ack_clock(|| CLOCK.load(Relaxed)); }
            s.register(1, Service::ServiceRegistry, qcom_servreg::LOCAL_PORT).unwrap();
            s.prepare_service_available(1, Service::ServiceRegistry).unwrap();
            s.ingest(1, &intents(&[(7, 4096), (8, 4096)])).unwrap();
            let rx3 = offer(&mut s); let rx4 = offer(&mut s);
            let one = registry_turn_request(&mut s, rx3, 9, 3);
            let two = registry_turn_request(&mut s, rx4, 9, 4);
            s.queue_reply(&one, Reply::Failure { error: 3 }).unwrap();
            assert_eq!(s.queue_reply(&two, Reply::Failure { error: 3 }), Err(Error::FlowBusy));
            assert_eq!(s.registry_window_trace().admissions, 0);
            let (Published::Data { remote_intent: a, .. }, _) = publish_until_data(&mut s) else { panic!() };
            assert_eq!(s.queue_reply(&two, Reply::Failure { error: 0 }), Err(Error::InvalidReply));
            assert_eq!(s.registry_window_trace().admissions, 0, "failed admission must not be counted");
            CLOCK.store(100, Relaxed);
            s.queue_reply(&two, Reply::Failure { error: 3 }).unwrap();
            assert_eq!(s.registry_window_trace().admissions, 1);
            assert_eq!(s.registry_window_trace().target.unwrap().queued_at, clocked.then_some(100));
            if complete_first {
                CLOCK.store(120, Relaxed);
                assert_eq!(s.ingest(1, &control(14, a)), Ok(Event::ReplyRxDone(one)));
            }
            CLOCK.store(140, Relaxed);
            let (Published::Data { remote_intent: b, .. }, _) = publish_until_data(&mut s) else { panic!() };
            let pair = s.registry_window_trace().target.unwrap();
            assert_eq!(pair.newer_published_at, clocked.then_some(140));
            assert_eq!(pair.published_before_done, Some(!complete_first));
            #[cfg(feature="sunfish-registry-post-reply-trace")]
            { assert!(pair.publication_frozen); assert_eq!(pair.publication.unwrap().transaction,4);
              assert!(pair.publication.unwrap().calls>0); }
            #[cfg(feature="sunfish-registry-post-reply-trace")]
            let frozen_publication = pair.publication;
            CLOCK.store(160, Relaxed);
            assert_eq!(s.ingest(1, &control(14, b)), Ok(Event::ReplyRxDone(two)));
            assert_eq!(s.registry_window_trace().target.unwrap().older_done_at, if clocked && complete_first {Some(120)} else {None}, "newer completion is not the older receipt");
            if !complete_first {
                CLOCK.store(180, Relaxed);
                assert_eq!(s.ingest(1, &control(14, a)), Ok(Event::ReplyRxDone(one)));
            }
            let pair = s.registry_window_trace().target.unwrap();
            assert_eq!(pair.older_done_at, clocked.then_some(if complete_first {120} else {180}));
            assert_eq!(pair.published_before_done, Some(!complete_first), "later completion cannot rewrite overlap proof");
            #[cfg(feature="sunfish-registry-post-reply-trace")]
            { assert_eq!(pair.publication,frozen_publication);
              assert!(s.registry_window_trace().target.unwrap().publication_frozen); }

            s.reset_after_transport_reset(2).unwrap();
            assert_eq!(s.registry_window_trace(), Default::default());
        }
    }
}

#[cfg(feature="sunfish-registry-handoff-turn")]
#[test]
fn registry_handoff_live_metadata_excludes_wrong_identity_route_and_completed_slot(){
    use crate::qcom_qmi_router::Address;
    for clocked in [false,true] {
        let mut s=session();if clocked{s.set_registry_ack_clock(||77);}
        s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();
        let a=offer(&mut s);let b=offer(&mut s);let c=offer(&mut s);
        let one=registry_turn_request(&mut s,a,9,3);let two=registry_turn_request(&mut s,b,9,4);let other=registry_turn_request(&mut s,c,10,4);
        s.queue_reply(&one,Reply::Failure{error:3}).unwrap();
        assert_eq!(s.registry_handoff_pair(&one,&two),Some((3,4,Address{node:0,port:9},false)));
        assert_eq!(s.registry_handoff_pair(&one,&other),None);assert_eq!(s.registry_handoff_pair(&two,&one),None);
        assert_eq!(session().registry_handoff_pair(&one,&two),None);
        assert_eq!(s.registry_window_trace().predecessor.unwrap().queued_at,clocked.then_some(77));
        assert_eq!(s.registry_window_trace().predecessor.unwrap().published_at,None);
        let(Published::Data{remote_intent:first,..},_)=publish_until_data(&mut s)else{panic!()};
        assert_eq!(s.registry_handoff_pair(&one,&two),Some((3,4,Address{node:0,port:9},true)));
        assert_eq!(s.registry_window_trace().predecessor.unwrap().published_at,clocked.then_some(77));
        s.queue_reply(&two,Reply::Failure{error:3}).unwrap();assert_eq!(s.registry_handoff_pair(&one,&two),None);
        assert_eq!(s.ingest(1,&control(14,first)),Ok(Event::ReplyRxDone(one)));
        assert_eq!(s.registry_handoff_pair(&one,&other),None);
        assert_eq!(s.registry_window_trace().predecessor.unwrap().done_at,clocked.then_some(77));
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().predecessor,None);
    }
}


#[cfg(feature="sunfish-registry-predecessor-pressure")]
#[test]
fn registry_predecessor_pressure_freezes_exact_publication_without_clock_and_survives_retirement(){
    for clocked in [false,true]{
        let mut s=session();if clocked{s.set_registry_ack_clock(||77);}
        s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();let a=offer(&mut s);let b=offer(&mut s);
        let one=registry_turn_request(&mut s,a,9,3);let two=registry_turn_request(&mut s,b,9,4);
        s.queue_reply(&one,Reply::Failure{error:3}).unwrap();
        let before=s.registry_window_trace().predecessor.unwrap();assert!(!before.publication_frozen);assert_eq!(before.publication.unwrap().transaction,3);
        let mut blocked=None;
        for _ in 0..16{
            let r=s.publish_next(1,|bytes|{if word(bytes,0)&0xffff==9 && bytes.get(49..51)==Some(&3u16.to_le_bytes()){blocked=Some(bytes.to_vec());Err(WriteError::WouldBlock)}else{Ok(())}});
            if blocked.is_some(){assert_eq!(r,Err(Error::WouldBlock));break;}r.unwrap();
        }
        let blocked=blocked.unwrap();let live=s.registry_window_trace().predecessor.unwrap();assert!(!live.publication_frozen);assert!(live.publication.unwrap().mask&16!=0);
        let event=s.publish_next(1,|bytes|{assert_eq!(bytes,blocked);Ok(())}).unwrap().unwrap();
        let Published::Data{remote_intent:id,..}=event else{panic!()};
        let frozen=s.registry_window_trace().predecessor.unwrap();assert!(frozen.publication_frozen);assert_eq!(frozen.published_at,clocked.then_some(77));
        assert!(frozen.publication.unwrap().mask&32!=0);assert!(frozen.publication.unwrap().calls>0);
        s.queue_reply(&two,Reply::Failure{error:3}).unwrap();
        let(Published::Data{remote_intent:other,..},_)=publish_until_data(&mut s)else{panic!()};
        s.ingest(1,&control(14,other)).unwrap();assert_eq!(s.registry_window_trace().predecessor.unwrap().publication,frozen.publication);
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(one)));
        assert_eq!(s.registry_window_trace().predecessor.unwrap().publication,frozen.publication,"retirement cannot erase or overwrite the capture");
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().predecessor,None);
    }
}


#[cfg(feature="sunfish-registry-between-grants")]
mod registry_between_grants {
    use super::*;

    // A real completed ACK precedes a second, still-unadvertised grant.
    // No simulated clock or diagnostic counter controls this window.
    fn prepared(return_request: bool, confirm: bool, capacity: u32) -> (Session, RequestHandle) {
        let mut s=session();
        s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
        s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        s.ingest(1,&intents(&[(7,capacity),(8,capacity),(9,capacity)])).unwrap();
        let id=offer(&mut s);
        let payload=[0,3,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o'];
        let packet=qrtr(1,9,qcom_servreg::LOCAL_PORT,confirm,&payload);
        let Event::Request(request)=s.ingest(1,&data(9,id,&packet,0)).unwrap() else {panic!()};
        if return_request {assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));}
        s.ingest(1,&control(7,52)).unwrap();
        assert!(matches!(publish(&mut s).unwrap().0,Published::ReceiveIntent(_)));
        assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});
        s.queue_reply(&request,Reply::Failure{error:3}).unwrap();
        s.ingest(1,&control(7,108)).unwrap();
        (s,request)
    }
    fn reply(s:&mut Session)->(u32,Vec<u8>){
        let(event,bytes)=publish(s).unwrap();
        let Published::Data{remote_intent,last_fragment:true}=event else{panic!("expected registry reply: {event:?}")};
        assert_eq!(word(&bytes,44),9);assert_eq!(&bytes[49..51],&3u16.to_le_bytes());
        (remote_intent,bytes)
    }
    fn grant(s:&mut Session){
        assert!(matches!(publish(s).unwrap().0,Published::ReceiveIntent(_)));
        assert_eq!(publish(s).unwrap().0,Published::IntentRequestAck{granted:true});
    }
    #[test]
    fn committed_ack_lends_exact_reply_one_turn_before_next_advertisement(){
        let(mut s,request)=prepared(true,false,4096);
        let(id,_)=reply(&mut s);
        let p=s.registry_window_trace().predecessor.unwrap().publication.unwrap();
        assert_eq!(p.mask&0x300,0x300,"attempt and actual successful borrowed DATA");
        grant(&mut s);
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn blocked_borrow_consumes_opportunity_then_grant_and_ordinary_retry_preserve_bytes(){
        let(mut s,request)=prepared(true,false,4096);let mut blocked=Vec::new();
        assert_eq!(s.publish_next(1,|b|{blocked.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
        let p=s.registry_window_trace().predecessor.unwrap().publication.unwrap();
        assert_eq!(p.mask&0x300,0x100);assert!(!s.reply_fully_published(&request));
        grant(&mut s); // no second borrowed attempt despite the still-pending exact reply
        let(id,bytes)=reply(&mut s);assert_eq!(bytes,blocked);
        assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,0x100);
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn uncertain_borrow_quarantines_without_grant_or_data_retry(){
        let(mut s,_)=prepared(true,false,4096);
        assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));
        let mut called=false;assert_eq!(s.publish_next(1,|_|{called=true;Ok(())}),Err(Error::Quarantined));assert!(!called);
        s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());
    }
    #[test]
    fn own_request_return_and_ready_qrtr_credit_keep_their_turns(){
        for confirm in [false,true]{
            let(mut s,request)=prepared(false,confirm,4096);
            assert!(matches!(publish(&mut s).unwrap().0,Published::ReceiveDone(_)));
            let early=if cfg!(feature="sunfish-registry-ready-handoffs") && !confirm {Some(reply(&mut s))}else{None};
            grant(&mut s);
            if confirm {let(e,b)=publish(&mut s).unwrap();assert!(matches!(e,Published::Data{..}));assert_eq!(word(&b,20),7);}
            let(id,_)=early.unwrap_or_else(||reply(&mut s));
            assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,if cfg!(feature="sunfish-registry-ready-handoffs")&&!confirm{0x300}else{0});
            assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
        }
    }
    #[test]
    fn ready_credit_and_missing_fit_do_not_spend_the_borrow() {
        let(mut credit,request)=prepared(true,true,4096);
        grant(&mut credit); // a ready RESUME excludes registry priority
        let(e,b)=publish(&mut credit).unwrap();assert!(matches!(e,Published::Data{..}));assert_eq!(word(&b,20),7);
        let(id,_)=reply(&mut credit);
        assert_eq!(credit.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,0);
        assert_eq!(credit.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
        let(mut nofit,request)=prepared(true,false,32);
        grant(&mut nofit); // no fitting modem intent, so finish the original grant
        assert_eq!(nofit.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,0);
        nofit.ingest(1,&intents(&[(10,4096)])).unwrap();
        nofit.ingest(1,&control(7,108)).unwrap();
        let(id,_)=reply(&mut nofit); // missing fit did not consume the later opportunity
        assert_eq!(nofit.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,0x300);
        assert_eq!(nofit.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn advertised_grant_ack_is_never_overtaken_by_new_reply(){
        let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
        s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,4096)])).unwrap();
        let id=offer(&mut s);let request=registry_turn_request(&mut s,id,9,3);
        assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));
        s.ingest(1,&control(7,52)).unwrap();grant(&mut s);
        s.ingest(1,&control(7,108)).unwrap();
        assert!(matches!(publish(&mut s).unwrap().0,Published::ReceiveIntent(_)));
        // Queue only after the backing advertisement. The new reply has never borrowed.
        s.queue_reply(&request,Reply::Failure{error:3}).unwrap();
        assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});
        let(id,_)=reply(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
        assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,0);
    }
    #[test]
    fn reset_reuses_slots_without_reusing_a_consumed_borrow_or_old_receipt(){
        let(mut s,old)=prepared(true,false,4096);
        assert_eq!(s.publish_next(1,|_|Err(WriteError::WouldBlock)),Err(Error::WouldBlock));
        s.reset_after_transport_reset(2).unwrap();
        s.register(2,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(2,Service::ServiceRegistry).unwrap();
        s.ingest(2,&intents(&[(7,4096)])).unwrap();let id=s.offer_receive(2).unwrap();
        let next=|s:&mut Session|{let mut b=Vec::new();let e=s.publish_next(2,|v|{b.extend_from_slice(v);Ok(())}).unwrap().unwrap();(e,b)};
        assert_eq!(next(&mut s).0,Published::ReceiveIntent(id));
        let payload=[0,3,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o'];
        let Event::Request(new)=s.ingest(2,&data(9,id,&qrtr(1,9,qcom_servreg::LOCAL_PORT,false,&payload),0)).unwrap() else {panic!()};
        assert_ne!(old,new);assert_eq!(s.queue_reply(&old,Reply::Failure{error:3}),Err(Error::StaleGeneration));
        assert_eq!(next(&mut s).0,Published::ReceiveDone(id));
        s.ingest(2,&control(7,52)).unwrap();assert!(matches!(next(&mut s).0,Published::ReceiveIntent(_)));
        assert_eq!(next(&mut s).0,Published::IntentRequestAck{granted:true});
        s.queue_reply(&new,Reply::Failure{error:3}).unwrap();s.ingest(2,&control(7,108)).unwrap();
        let(e,b)=next(&mut s);let Published::Data{remote_intent:id,..}=e else {panic!("reset latch survived: {e:?}")};
        assert_eq!(&b[49..51],&3u16.to_le_bytes());
        assert_eq!(s.ingest(2,&control(14,id)),Ok(Event::ReplyRxDone(new)));
        assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x300,0x300);
    }

    #[cfg(feature="sunfish-registry-grant-eligibility")]
    #[test]
    fn eligibility_distinguishes_return_expired_yield_advertised_ack_and_borrow(){
        for mode in 0..3 {
            let(mut s,request)=prepared(mode!=0,false,4096);
            let expected=match mode {
                0 => { assert!(matches!(publish(&mut s).unwrap().0,Published::ReceiveDone(_)));if cfg!(feature="sunfish-registry-ready-handoffs"){reply(&mut s);grant(&mut s);[0,0,0,0,0,0,0,1,0,0,0,1]}else{grant(&mut s);reply(&mut s);[1,0,1,1,0,0,0,1,0,0,0,0]} },
                1 => {reply(&mut s);[0,0,0,0,0,0,0,0,0,0,0,1]},
                _ => {assert_eq!(s.publish_next(1,|_|Err(WriteError::WouldBlock)),Err(Error::WouldBlock));grant(&mut s);reply(&mut s);[1,0,1,0,0,0,0,0,1,0,0,1]},
            };
            assert!(s.reply_fully_published(&request));
            let frozen=s.registry_window_trace().predecessor.unwrap();
            assert!(frozen.publication_frozen);
            assert_eq!(frozen.publication.unwrap().grant_gates,expected,"mode{mode}");
            assert_eq!(expected.iter().sum::<u32>(),frozen.publication.unwrap().calls);
            assert_eq!(s.ingest(1,&control(14,7)),Ok(Event::ReplyRxDone(request)));
            assert_eq!(s.registry_window_trace().predecessor.unwrap().publication,frozen.publication);
        }
    }
    #[cfg(feature="sunfish-registry-grant-eligibility")]
    #[test]
    fn eligibility_distinguishes_negative_ack_and_trace_reads_do_not_sample_clock(){
        use core::sync::atomic::{AtomicU32,Ordering::Relaxed};
        static READS:AtomicU32=AtomicU32::new(0);
        for clocked in [false,true]{
            let(mut s,request)=prepared(true,false,4096);
            if clocked{s.set_registry_ack_clock(||{READS.fetch_add(1,Relaxed);77});}
            // Consume the positive-grant borrow without publication, then finish
            // its advertisement and ACK. A later invalid-size request is denied.
            assert_eq!(s.publish_next(1,|_|Err(WriteError::WouldBlock)),Err(Error::WouldBlock));grant(&mut s);
            assert_eq!(s.ingest(1,&control(7,4097)),Ok(Event::ReceiveIntentRequest{granted:false}));
            for _ in 0..4{if s.reply_fully_published(&request){break;}publish(&mut s).unwrap();}
            assert!(s.reply_fully_published(&request));
            let before=READS.load(Relaxed);let frozen=s.registry_window_trace().predecessor.unwrap();
            assert_eq!(frozen.publication.unwrap().grant_gates[1],1);
            for _ in 0..4{assert_eq!(s.registry_window_trace().predecessor.unwrap().publication,frozen.publication);}
            assert_eq!(READS.load(Relaxed),before,"trace reads must not add clock operations");
            assert_eq!(s.ingest(1,&control(14,7)),Ok(Event::ReplyRxDone(request)));
            assert_eq!(s.registry_window_trace().predecessor.unwrap().publication,frozen.publication);
            s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().predecessor,None);
        }
    }
}

#[cfg(feature="sunfish-registry-converted-grant-turn")]
mod registry_converted_grant_turn {
    use super::*;
    fn next_at(s:&mut Session,g:u64)->(Published,Vec<u8>){
        let mut b=Vec::new();let e=s.publish_next(g,|v|{b.extend_from_slice(v);Ok(())}).unwrap().unwrap();(e,b)
    }
    fn offer_at(s:&mut Session,g:u64)->u32{let id=s.offer_receive(g).unwrap();assert_eq!(next_at(s,g).0,Published::ReceiveIntent(id));id}
    fn denied_in(s:Session,g:u64,confirm:bool,fit:bool)->(Session,RequestHandle,u32,u32){
        denied_reply(s,g,confirm,fit,Reply::Failure{error:3})
    }
    fn denied_reply(mut s:Session,g:u64,confirm:bool,fit:bool,reply:Reply<'_>)->(Session,RequestHandle,u32,u32){
        for(service,port) in [(Service::RemoteStorage,STORAGE),(Service::HeapMemory,MEMORY),(Service::ServiceRegistry,qcom_servreg::LOCAL_PORT)]{
            if s.endpoint(service).is_none(){s.register(g,service,port).unwrap();}
        }
        s.prepare_service_available(g,Service::ServiceRegistry).unwrap();
        s.ingest(g,&intents(&[(1,if confirm{4096}else{32})])).unwrap();
        let ids:Vec<_>=(0..qcom_glink_qmi::RX_SLOTS).map(|_|offer_at(&mut s,g)).collect();
        let payload=[0,3,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o'];
        let Event::Request(request)=s.ingest(g,&data(9,ids[0],&qrtr(1,9,qcom_servreg::LOCAL_PORT,confirm,&payload),0)).unwrap()else{panic!()};
        assert_eq!(next_at(&mut s,g).0,Published::ReceiveDone(ids[0]));
        let last=offer_at(&mut s,g);assert_eq!(s.offer_receive(g),Err(Error::NoCapacity));
        if fit{s.ingest(g,&intents(&[(7,4096),(8,4096),(9,4096)])).unwrap();}
        s.queue_reply(&request,reply).unwrap();
        assert_eq!(s.ingest(g,&data(9,ids[1],&resume_route(0xdead_0000,STORAGE),0)),Ok(Event::CreditReceived));
        assert_eq!(s.ingest(g,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));
        (s,request,ids[1],last+1)
    }
    fn denied(confirm:bool,fit:bool)->(Session,RequestHandle,u32,u32){denied_in(session(),1,confirm,fit)}
    fn exact_reply(s:&mut Session)->(u32,Vec<u8>){
        let(e,b)=publish(s).unwrap();let Published::Data{remote_intent,last_fragment:true}=e else{panic!("converted grant must lend exact registry reply, got {e:?}")};
        assert_eq!(word(&b,44),9);assert_eq!(&b[49..51],&3u16.to_le_bytes());(remote_intent,b)
    }
    fn backing_and_ack(s:&mut Session,id:u32){
        assert_eq!(publish(s).unwrap().0,Published::ReceiveIntent(id));
        assert_eq!(publish(s).unwrap().0,Published::IntentRequestAck{granted:true});
    }
    fn converted(s:&mut Session,release:u32){assert_eq!(publish(s).unwrap().0,Published::ReceiveDone(release));}
    fn mask(s:&Session)->u32{s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask}
    #[cfg(feature="sunfish-registry-post-reply-trace")]
    #[test]
    fn registry_post_reply_converted_grant_observation_survives_token_and_frozen_publication(){
        use core::sync::atomic::{AtomicU64,Ordering::Relaxed};
        static NOW:AtomicU64=AtomicU64::new(100);
        for clocked in [false,true] {
            let(mut s,request,release,backing)=denied(false,true);
            if clocked{s.set_registry_ack_clock(||NOW.load(Relaxed));}
            converted(&mut s,release);
            let g=s.registry_window_trace().predecessor.unwrap().grant.unwrap();
            assert_eq!(g.intent,backing);assert_eq!(g.converted_at,clocked.then_some(100));
            let(id,_)=exact_reply(&mut s);
            let publication=s.registry_window_trace().predecessor.unwrap().publication;
            NOW.store(200,Relaxed);
            assert_eq!(s.publish_next(1,|_|Err(WriteError::WouldBlock)),Err(Error::WouldBlock));
            assert_eq!(s.registry_window_trace().predecessor.unwrap().grant.unwrap().advertised_at,None);
            assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveIntent(backing));
            NOW.store(300,Relaxed);
            assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});
            let prior=s.registry_window_trace().predecessor.unwrap();let grant=prior.grant.unwrap();
            assert_eq!(grant.advertised_at,clocked.then_some(200));assert_eq!(grant.ack_at,clocked.then_some(300));
            assert_eq!(prior.publication,publication,"later matching grant must not rewrite pre-publication pressure");
            assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
            s.ingest(1,&control(7,52)).unwrap();
            assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:false});
            assert_eq!(s.registry_window_trace().predecessor.unwrap().grant,Some(grant));
            s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().predecessor,None);
            NOW.store(100,Relaxed);
        }
    }

    #[test]
    fn full_slot_denial_return_then_exact_reply_precedes_backing_and_positive_ack(){
        let(mut s,request,release,backing)=denied(false,true);converted(&mut s,release);
        assert_eq!(mask(&s)&0x1c00,0x400);
        let(id,_)=exact_reply(&mut s);assert_eq!(mask(&s)&0x1f00,0x1f00);
        backing_and_ack(&mut s,backing);
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn blocked_reply_spends_conversion_before_checked_writer_and_preserves_fallback_bytes(){
        let(mut s,request,release,backing)=denied(false,true);converted(&mut s,release);
        let mut blocked=Vec::new();assert_eq!(s.publish_next(1,|b|{blocked.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
        assert_eq!(mask(&s)&0x1f00,0xd00);
        let mut blocked_ad=Vec::new();assert_eq!(s.publish_next(1,|b|{assert_eq!(word(b,0)&0xffff,5);blocked_ad.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
        let(e,b)=publish(&mut s).unwrap();assert_eq!(e,Published::ReceiveIntent(backing));assert_eq!(b,blocked_ad);
        assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});
        let(id,b)=exact_reply(&mut s);assert_eq!(b,blocked);assert_eq!(mask(&s)&0x1f00,0xd00);
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn blocked_or_uncertain_receive_return_cannot_arm_a_positive_grant(){
        for uncertain in [false,true]{
            let(mut s,request,release,_)=denied(false,true);
            let result=s.publish_next(1,|b|{assert_eq!(word(b,0)&0xffff,6);assert_eq!(word(b,4),release);Err(if uncertain{WriteError::Uncertain}else{WriteError::WouldBlock})});
            assert_eq!(result,Err(if uncertain{Error::PublicationUncertain}else{Error::WouldBlock}));assert_eq!(mask(&s)&0x1f00,0);
            if uncertain{let mut called=false;assert_eq!(s.publish_next(1,|_|{called=true;Ok(())}),Err(Error::Quarantined));assert!(!called);}
            else{assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:false});let(id,_)=exact_reply(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));assert_eq!(mask(&s)&0x1f00,0);}
        }
    }
    #[test]
    fn uncertain_reply_quarantines_and_generation_reset_drops_exact_conversion_identity(){
        let(mut s,old,release,_)=denied(false,true);converted(&mut s,release);
        assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));assert_eq!(mask(&s)&0x1f00,0xd00);
        assert_eq!(s.publish_next(1,|_|panic!("uncertain publication must never retry")),Err(Error::Quarantined));
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().predecessor,None);
        let(mut s,new,release,backing)=denied_in(s,2,false,true);assert_ne!(old,new);
        assert_eq!(s.queue_reply(&old,Reply::Failure{error:3}),Err(Error::StaleGeneration));
        assert_eq!(next_at(&mut s,2).0,Published::ReceiveDone(release));
        let(e,b)=next_at(&mut s,2);let Published::Data{remote_intent:id,..}=e else{panic!("new generation did not get its own turn: {e:?}")};assert_eq!(&b[49..51],&3u16.to_le_bytes());
        assert_eq!(next_at(&mut s,2).0,Published::ReceiveIntent(backing));assert_eq!(next_at(&mut s,2).0,Published::IntentRequestAck{granted:true});
        assert_eq!(s.ingest(2,&control(14,id)),Ok(Event::ReplyRxDone(new)));assert_eq!(mask(&s)&0x1f00,0x1f00);
    }
    #[test]
    fn ready_qrtr_credit_expires_conversion_at_advertisement_and_keeps_its_data_turn(){
        let(mut s,request,release,backing)=denied(true,true);converted(&mut s,release);
        assert_eq!(mask(&s)&0x1f00,0x400);backing_and_ack(&mut s,backing);
        let(e,b)=publish(&mut s).unwrap();let Published::Data{remote_intent:credit,..}=e else{panic!()};assert_eq!(word(&b,20),7);
        let(id,_)=exact_reply(&mut s);assert_ne!(id,credit);assert_eq!(mask(&s)&0x1f00,0x400);
        s.ingest(1,&control(14,credit)).unwrap();assert!(s.reply_fully_published(&request));
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn absent_fitting_modem_intent_cannot_arm_or_inherit_a_conversion_turn(){
        let(mut s,request,release,backing)=denied(false,false);converted(&mut s,release);assert_eq!(mask(&s)&0x1f00,0);assert_eq!(mask(&s)&0xe000,0xa000);
        backing_and_ack(&mut s,backing);s.ingest(1,&intents(&[(7,4096)])).unwrap();
        let(id,_)=exact_reply(&mut s);assert_eq!(mask(&s)&0x1f00,0);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn another_conversion_cannot_rearm_the_same_blocked_reply(){
        let(mut s,request,release,backing)=denied(false,true);converted(&mut s,release);
        assert_eq!(s.publish_next(1,|_|Err(WriteError::WouldBlock)),Err(Error::WouldBlock));backing_and_ack(&mut s,backing);
        s.ingest(1,&data(9,3,&resume_route(0xbeef_0000,STORAGE),0)).unwrap();
        assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));converted(&mut s,3);
        assert_ne!(mask(&s)&0x10000,0);backing_and_ack(&mut s,backing+1);let(id,_)=exact_reply(&mut s);assert_eq!(mask(&s)&0x1f00,0xd00);
        assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
    #[test]
    fn second_reply_admission_and_reverse_receipts_keep_original_transactions(){
        let(mut s,one,release,backing)=denied(false,true);converted(&mut s,release);
        let(first,_)=exact_reply(&mut s);backing_and_ack(&mut s,backing);
        let two=registry_turn_request(&mut s,backing,9,4);s.queue_reply(&two,Reply::Failure{error:3}).unwrap();
        let(Published::Data{remote_intent:second,..},b)=publish_until_data(&mut s)else{panic!()};assert_eq!(&b[49..51],&4u16.to_le_bytes());assert_ne!(first,second);
        assert_eq!(s.registry_window_trace().target.unwrap().published_before_done,Some(true));
        assert_eq!(s.ingest(1,&control(14,second)),Ok(Event::ReplyRxDone(two)));assert!(s.reply_fully_published(&one));
        assert_eq!(s.ingest(1,&control(14,first)),Ok(Event::ReplyRxDone(one)));
    }

    #[test]
    fn ready_tftp_oack_keeps_its_turn_and_expires_the_conversion_before_registry_data(){
        use qcom_glink_qmi::TftpReplyRoute;use qcom_qmi_router::Address;
        let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();s.ingest(1,&intents(&[(17,4096),(18,4096),(19,4096)])).unwrap();
        let hello=offer(&mut s);let mut available=vec![hello];
        s.ingest(1,&data(9,hello,&qrtr_control(2,[0;4]),0)).unwrap();
        for _ in 0..64{let Some((e,_))=publish(&mut s)else{break};match e{
            Published::ReceiveIntent(id)=>available.push(id),Published::ReceiveDone(id)=>available.retain(|&old|old!=id),
            Published::Data{remote_intent,..}=>{s.ingest(1,&control(14,remote_intent)).unwrap();},other=>panic!("{other:?}")}}
        assert!(publish(&mut s).is_none());
        while available.len()<qcom_glink_qmi::RX_SLOTS{available.push(offer(&mut s));}
        assert_eq!(s.offer_receive(1),Err(Error::NoCapacity));
        let request=registry_turn_request(&mut s,available[0],9,3);
        assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(available[0]));let last=offer(&mut s);
        s.queue_reply(&request,Reply::Failure{error:3}).unwrap();
        s.ingest(1,&data(9,available[1],&resume_route(0xdead_0000,STORAGE),0)).unwrap();
        let route=TftpReplyRoute{source:Address{node:1,port:0x5001},destination:Address{node:0,port:0x56}};
        s.queue_tftp_response(1,route,qcom_tftp_server_check::OACK).unwrap();
        assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));converted(&mut s,available[1]);
        assert_eq!(mask(&s)&0x1f00,0x400);backing_and_ack(&mut s,last+1);
        let(e,b)=publish(&mut s).unwrap();let Published::Data{remote_intent:oack,..}=e else{panic!("protected OACK expected: {e:?}")};assert_eq!(word(&b,44),0x56);assert_eq!(&b[48..50],&[0,6]);
        let(id,_)=exact_reply(&mut s);assert_eq!(mask(&s)&0x1f00,0x400);
        s.ingest(1,&control(14,oack)).unwrap();assert!(s.reply_fully_published(&request));assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }

    #[test]
    fn a_large_reply_borrows_one_fragment_then_the_grant_pair_keeps_its_turn(){
        let mut fields=vec![0x10];fields.extend(700u16.to_le_bytes());fields.extend(vec![0;700]);
        let(mut s,request,release,backing)=denied_reply(session(),1,false,true,Reply::Success{optional_tlvs:&fields});
        converted(&mut s,release);let(e,b)=publish(&mut s).unwrap();
        let Published::Data{remote_intent:id,last_fragment:false}=e else{panic!("one bounded first fragment expected: {e:?}")};
        assert_eq!(word(&b,8),qcom_glink_qmi::TX_CHUNK_BYTES as u32);assert!(!s.reply_fully_published(&request));
        let mut payload=b[16..16+word(&b,8)as usize].to_vec();let mut remaining=word(&b,12);
        backing_and_ack(&mut s,backing); // no continuation can replace this grant pair
        for _ in 0..16{
            let(e,b)=publish(&mut s).unwrap();let Published::Data{remote_intent,last_fragment}=e else{panic!("{e:?}")};
            assert_eq!(remote_intent,id);assert_eq!(word(&b,0)&0xffff,12);
            let chunk=word(&b,8);assert_eq!(chunk+word(&b,12),remaining);remaining=word(&b,12);
            payload.extend_from_slice(&b[16..16+chunk as usize]);
            if last_fragment{assert_eq!(remaining,0);break;}
        }
        assert!(s.reply_fully_published(&request));assert_eq!(word(&payload,28),9);assert_eq!(&payload[33..35],&3u16.to_le_bytes());
        assert_eq!(mask(&s)&0x1f00,0x1f00);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(request)));
    }
}

#[cfg(feature="sunfish-registry-post-reply-trace")]
#[test]
fn registry_post_reply_tx4_pressure_is_live_then_frozen_against_original_identity(){
    for no_fit in [false,true] {
        let mut s=session();
        s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
        s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
        s.ingest(1,&intents(&[(7,4096)])).unwrap();
        let rx3=offer(&mut s);let rx4=offer(&mut s);
        let one=registry_turn_request(&mut s,rx3,9,3);let two=registry_turn_request(&mut s,rx4,9,4);
        s.queue_reply(&one,Reply::Failure{error:3}).unwrap();
        let(Published::Data{remote_intent:a,..},_)=publish_until_data(&mut s)else{panic!()};
        s.queue_reply(&two,Reply::Failure{error:3}).unwrap();
        let live=s.registry_window_trace().target.unwrap();
        assert!(!live.publication_frozen);assert_eq!(live.publication.unwrap().transaction,4);
        if no_fit {
            // The only modem intent still belongs to transaction 3. Normal
            // missing-fit handling must be observed, never bypassed by tracing.
            for _ in 0..3 {let _=s.publish_next(1,|_|Ok(()));}
            let p=s.registry_window_trace().target.unwrap().publication.unwrap();
            assert!(p.mask&1!=0);assert!(!s.reply_fully_published(&two));
            s.ingest(1,&intents(&[(8,4096)])).unwrap();
            // The normal outbound-intent request ACK is needed, if requested.
            let _=s.ingest(1,&control(8,1));
        }else{s.ingest(1,&intents(&[(8,4096)])).unwrap();}
        let mut attempted=false;
        for _ in 0..16 {
            let r=s.publish_next(1,|bytes|{
                if word(bytes,0)&0xffff==9 && bytes.get(49..51)==Some(&4u16.to_le_bytes()){
                    attempted=true;Err(WriteError::WouldBlock)
                }else{Ok(())}
            });
            if attempted{assert_eq!(r,Err(Error::WouldBlock));break;}
        }
        assert!(attempted);let blocked=s.registry_window_trace().target.unwrap();
        assert!(!blocked.publication_frozen);assert!(blocked.publication.unwrap().mask&16!=0);
        let(Published::Data{remote_intent:b,..},_)=publish_until_data(&mut s)else{panic!()};
        let frozen=s.registry_window_trace().target.unwrap();assert!(frozen.publication_frozen);
        assert!(frozen.publication.unwrap().mask&(1<<5)!=0);
        assert_eq!(s.ingest(1,&control(14,b)),Ok(Event::ReplyRxDone(two)));
        assert_eq!(s.registry_window_trace().target.unwrap().publication,frozen.publication);
        assert_eq!(s.ingest(1,&control(14,a)),Ok(Event::ReplyRxDone(one)));
        assert_eq!(s.registry_window_trace().target.unwrap().publication,frozen.publication);
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().target,None);
    }
}

#[cfg(feature="sunfish-registry-conversion-lifetime-trace")]
#[path="sunfish_registry_conversion_lifetime_fixture.rs"]
mod registry_conversion_lifetime_fixture;

#[cfg(feature = "sunfish-glink-announcement-reuse")]
#[path = "sunfish_announcement_reuse_test.rs"]
mod announcement_reuse;

#[cfg(feature="sunfish-elf-queued-handoff")]
mod elf_window_cases {include!("sunfish_elf_window_cases.rs");}

#[cfg(feature="sunfish-registry-ready-handoffs")]
#[path="sunfish_registry_ready_core_cases.rs"]
mod registry_ready_core_cases;
