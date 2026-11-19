#![allow(dead_code)]
#[path = "../rust/src/qcom_qmi.rs"]
mod qcom_qmi;
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
mod sunfish_ipa_linux_config;
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
mod sunfish_ipa_linux_memory;
#[path = "../rust/src/sunfish_ipa_linux_qmi.rs"]
mod sunfish_ipa_linux_qmi;

use qcom_qmi::{Kind, Message};
use sunfish_ipa_linux_qmi as ipa;
use sunfish_ipa_linux_qmi::{Address, ColdSession, Error, InitRequest, Prepared, Socket};

const GENERATION: u64 = 17;
const MODEM: Address = Address {
    node: 3,
    port: 0x40,
};
const CLIENT: Address = Address {
    node: 3,
    port: 0x80,
};
const TRANSACTION: u16 = 0x1234;
const INDICATION_TRANSACTION: u16 = 0x2468;
const GOLDEN: &str = include_str!(env!("SUNFISH_LINUX_QMI_C"));

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn golden(name: &str) -> Vec<u8> {
    let line = GOLDEN
        .lines()
        .find_map(|s| s.strip_prefix(&format!("{name} ")))
        .unwrap();
    (0..line.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
        .collect()
}
fn message(kind: u8, txn: u16, id: u16, tlvs: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend(txn.to_le_bytes());
    out.extend(id.to_le_bytes());
    let size: usize = tlvs.iter().map(|(_, b)| 3 + b.len()).sum();
    out.extend((size as u16).to_le_bytes());
    for (tag, bytes) in tlvs {
        out.push(*tag);
        out.extend((bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}
fn response(txn: u16, result: u16, error: u16) -> Vec<u8> {
    let value: Vec<u8> = result
        .to_le_bytes()
        .into_iter()
        .chain(error.to_le_bytes())
        .collect();
    message(2, txn, ipa::INIT_DRIVER, &[(2, &value)])
}
fn new() -> ColdSession {
    ColdSession::new(GENERATION, MODEM).unwrap()
}
fn published() -> ColdSession {
    let mut s = new();
    let p = s
        .prepare_init_driver(GENERATION, TRANSACTION, 0x415)
        .unwrap();
    s.commit_publication(GENERATION, &p).unwrap();
    s
}
fn prepare(s: &mut ColdSession, id: u16, txn: u16) -> Prepared {
    let bytes = if id == ipa::DRIVER_INIT_COMPLETE {
        message(0, txn, id, &[(1, &[0x67])])
    } else {
        message(0, txn, id, &[])
    };
    s.prepare_response(GENERATION, CLIENT, &bytes).unwrap()
}
fn event(s: &mut ColdSession, event: u8) {
    match event {
        b'I' => s
            .receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0))
            .unwrap(),
        b'R' | b'U' => {
            let p = prepare(
                s,
                if event == b'R' {
                    ipa::INDICATION_REGISTER
                } else {
                    ipa::DRIVER_INIT_COMPLETE
                },
                1,
            );
            s.commit_publication(GENERATION, &p).unwrap();
        }
        _ => panic!(),
    }
    if let Some(p) = s
        .prepare_indication(GENERATION, INDICATION_TRANSACTION)
        .unwrap()
    {
        s.commit_publication(GENERATION, &p).unwrap();
    }
}
fn ready_before_indication() -> ColdSession {
    let mut s = published();
    s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0))
        .unwrap();
    for id in [ipa::INDICATION_REGISTER, ipa::DRIVER_INIT_COMPLETE] {
        let p = prepare(&mut s, id, 1);
        s.commit_publication(GENERATION, &p).unwrap();
    }
    s
}

#[test]
fn qmi_exact_original_factory_and_encoder() {
    for (loaded, name) in [(false, "cold"), (true, "loaded")] {
        let request = InitRequest::new(TRANSACTION, 0x415, loaded).unwrap();
        assert_eq!(request.bytes().as_slice(), golden(name));
        assert_eq!(request.bytes().len(), 108);
        let m = Message::decode(request.bytes()).unwrap();
        assert_eq!(
            (m.kind, m.transaction, m.message_id, m.tlv_count()),
            (Kind::Request, TRANSACTION, ipa::INIT_DRIVER, 12)
        );
        assert_eq!(m.u8(0x18).unwrap(), u8::from(loaded));
        for tag in [0x12, 0x13] {
            assert_eq!(&m.get(tag).unwrap()[4..], 7u32.to_le_bytes());
        }
        assert_eq!(m.get(0x11).unwrap(), [0xa8, 4, 0, 0, 0xe7, 5, 0, 0]);
        assert_eq!(m.get(0x19).unwrap(), [0xf0, 5, 0, 0, 0xef, 7, 0, 0]);
        assert_eq!(m.u32(0x17).unwrap(), 9);
        assert_eq!(m.u32(0x1f).unwrap(), 0xa50);
        assert_eq!(m.u32(0x20).unwrap(), 0x60);
        for tag in [0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x21, 0x22] {
            assert!(m.get(tag).is_none());
        }
    }
    for bit in 0..32 {
        assert_eq!(
            InitRequest::new(1, 0x415 ^ (1 << bit), false).err(),
            Some(Error::Geometry)
        );
    }
    let zero = InitRequest::new(0, 0x415, false).unwrap();
    assert_eq!(Message::decode(zero.bytes()).unwrap().transaction, 0);
    assert_eq!(
        (
            ipa::SERVICE,
            ipa::HOST_VERSION_INSTANCE,
            ipa::MODEM_VERSION_INSTANCE,
            ipa::INIT_DRIVER_TIMEOUT_MS
        ),
        (0x31, 0x101, 0x201, 60_000)
    );
}

#[test]
fn qmi_six_handshake_orders_match_original_callbacks() {
    for order in ["IRU", "IUR", "RIU", "RUI", "UIR", "URI"] {
        let mut s = published();
        for (step, input) in order.bytes().enumerate() {
            event(&mut s, input);
            let st = s.state();
            let line = format!(
                "trace {order} {step} {} {} {} {} {}",
                u8::from(st.modem_ready),
                u8::from(st.qmi_uc_ready),
                u8::from(st.indication_requested),
                u8::from(st.indication_sent),
                u8::from(st.initial_boot)
            );
            assert!(GOLDEN.lines().any(|row| row == line), "{line}");
            assert_eq!(s.protocol_complete(), step == 2);
        }
        assert_eq!(s.driver_status(), Some(0x67));
        assert!(
            s.prepare_indication(GENERATION, INDICATION_TRANSACTION)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn qmi_publication_is_required_for_each_milestone() {
    let mut s = new();
    let p = s
        .prepare_init_driver(GENERATION, TRANSACTION, 0x415)
        .unwrap();
    assert_eq!(
        s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0)),
        Err(Error::NotPublished)
    );
    assert!(!s.state().modem_ready);
    assert_eq!(
        s.prepare_init_driver(GENERATION, TRANSACTION, 0x415).err(),
        Some(Error::AlreadyStarted)
    );
    #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
    assert_eq!(
        s.prepare_response(
            GENERATION,
            CLIENT,
            &message(0, 1, ipa::INDICATION_REGISTER, &[])
        )
        .err(),
        Some(Error::Busy)
    );
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    {
        let server = s.prepare_response(GENERATION, CLIENT,
            &message(0, 1, ipa::INDICATION_REGISTER, &[])).unwrap();
        assert!(!s.state().indication_requested);
        s.commit_publication(GENERATION, &server).unwrap();
        assert!(s.state().indication_requested && !s.state().modem_ready);
        assert_eq!(s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0)),
            Err(Error::NotPublished), "server commit is not INIT publication");
    }
    s.commit_publication(GENERATION, &p).unwrap();
    assert_eq!(p.socket(), Socket::Client);
    assert_eq!(p.peer(), MODEM);
    for id in [ipa::INDICATION_REGISTER, ipa::DRIVER_INIT_COMPLETE] {
        let before = s.state();
        let p = prepare(&mut s, id, 4);
        assert_eq!(s.state(), before);
        assert!(
            s.prepare_indication(GENERATION, INDICATION_TRANSACTION)
                .unwrap()
                .is_none()
        );
        s.commit_publication(GENERATION, &p).unwrap();
    }
    assert!(!s.protocol_complete());
    s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0))
        .unwrap();
    let p = s
        .prepare_indication(GENERATION, INDICATION_TRANSACTION)
        .unwrap()
        .unwrap();
    assert!(!s.protocol_complete());
    assert!(!s.state().indication_sent);
    assert_eq!(
        s.prepare_indication(GENERATION, INDICATION_TRANSACTION)
            .err(),
        Some(Error::Busy)
    );
    s.commit_publication(GENERATION, &p).unwrap();
    assert!(s.protocol_complete());
    assert_eq!(
        s.commit_publication(GENERATION, &p),
        Err(Error::NoPendingPublication)
    );
}

#[test]
fn qmi_original_success_records_and_two_local_handles() {
    let mut s = new();
    for (id, txn, label) in [
        (ipa::INDICATION_REGISTER, 0x5678, "register_response"),
        (ipa::DRIVER_INIT_COMPLETE, 0x9abc, "driver_response"),
    ] {
        let p = prepare(&mut s, id, txn);
        assert_eq!(p.bytes(), golden(label));
        assert_eq!(p.socket(), Socket::Server);
        assert_eq!(p.peer(), CLIENT); // not the modem service port
        s.commit_publication(GENERATION, &p).unwrap();
    }
    let p = s
        .prepare_init_driver(GENERATION, TRANSACTION, 0x415)
        .unwrap();
    s.commit_publication(GENERATION, &p).unwrap();
    s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0))
        .unwrap();
    let p = s
        .prepare_indication(GENERATION, INDICATION_TRANSACTION)
        .unwrap()
        .unwrap();
    assert_eq!(p.bytes(), golden("complete_indication"));
    assert_eq!(p.socket(), Socket::Server);
    assert_eq!(p.peer(), MODEM); // source sends to the discovered modem service
}

#[test]
fn qmi_owner_generation_and_sequence_isolation() {
    assert_eq!(
        ColdSession::new(0, MODEM).err(),
        Some(Error::InvalidGeneration)
    );
    for bad in [
        Address {
            node: u32::MAX,
            port: 3,
        },
        Address { node: 3, port: 0 },
        Address {
            node: 3,
            port: 0xffff_fffe,
        },
    ] {
        assert_eq!(ColdSession::new(1, bad).err(), Some(Error::InvalidPeer));
    }
    let mut a = new();
    let mut b = new();
    let pa = prepare(&mut a, ipa::INDICATION_REGISTER, 1);
    let pb = prepare(&mut b, ipa::INDICATION_REGISTER, 1);
    assert_eq!(
        a.commit_publication(GENERATION, &pb),
        Err(Error::WrongPublication)
    );
    assert_eq!(
        a.publication_uncertain(GENERATION, &pb),
        Err(Error::WrongPublication)
    );
    assert_eq!(
        a.commit_publication(GENERATION + 1, &pa),
        Err(Error::StaleGeneration)
    );
    assert_eq!(
        a.prepare_response(
            GENERATION + 1,
            CLIENT,
            &message(0, 1, ipa::INDICATION_REGISTER, &[])
        )
        .err(),
        Some(Error::StaleGeneration)
    );
    assert_eq!(
        a.prepare_indication(GENERATION + 1, INDICATION_TRANSACTION)
            .err(),
        Some(Error::StaleGeneration)
    );
    assert_eq!(a.fail(GENERATION + 1), Err(Error::StaleGeneration));
    a.commit_publication(GENERATION, &pa).unwrap();
    let next = prepare(&mut a, ipa::DRIVER_INIT_COMPLETE, 2);
    assert_eq!(
        a.commit_publication(GENERATION, &pa),
        Err(Error::WrongPublication)
    );
    assert!(!a.state().qmi_uc_ready);
    a.commit_publication(GENERATION, &next).unwrap();
}

#[test]
fn qmi_peer_and_transaction_correlation_precedes_remote_failure() {
    let mut s = published();
    let before = s.state();
    let failure = response(TRANSACTION + 1, 1, 3);
    assert_eq!(
        s.receive_init_response(GENERATION, MODEM, &failure),
        Err(Error::WrongTransaction)
    );
    assert_eq!(
        s.receive_init_response(GENERATION, CLIENT, &response(TRANSACTION, 1, 3)),
        Err(Error::WrongPeer)
    );
    assert_eq!(
        s.receive_init_response(GENERATION + 1, MODEM, &response(TRANSACTION, 1, 3)),
        Err(Error::StaleGeneration)
    );
    for peer in [
        Address {
            node: 4,
            port: 0x80,
        },
        Address { node: 3, port: 0 },
        Address {
            node: 3,
            port: 0xffff_fffe,
        },
    ] {
        assert_eq!(
            s.prepare_response(
                GENERATION,
                peer,
                &message(0, 1, ipa::INDICATION_REGISTER, &[])
            )
            .err(),
            Some(Error::WrongPeer)
        );
    }
    assert_eq!(s.state(), before);
    s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0))
        .unwrap();
    assert_eq!(
        s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0)),
        Err(Error::NotPublished)
    );
}

#[test]
fn qmi_remote_result_is_fail_closed() {
    for (result, error) in [(1, 3), (0, 3), (2, 0), (0xffff, 0xffff)] {
        let mut s = published();
        assert_eq!(
            s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, result, error)),
            Err(Error::RemoteFailure { result, error })
        );
        assert!(s.state().failed);
        assert!(!s.state().modem_ready);
        assert!(!s.protocol_complete());
        assert_eq!(
            s.prepare_indication(GENERATION, INDICATION_TRANSACTION)
                .err(),
            Some(Error::Failed)
        );
    }
}

#[test]
fn qmi_request_schema_is_bounded_and_source_bytes_are_uninterpreted() {
    for value in 0..=255 {
        let mut s = new();
        let reg = message(0, 7, ipa::INDICATION_REGISTER, &[(0x10, &[value])]);
        let p = s.prepare_response(GENERATION, CLIENT, &reg).unwrap();
        assert!(!s.state().indication_requested);
        s.commit_publication(GENERATION, &p).unwrap();
        assert!(s.state().indication_requested);
        let done = message(0, 8, ipa::DRIVER_INIT_COMPLETE, &[(1, &[value])]);
        let p = s.prepare_response(GENERATION, CLIENT, &done).unwrap();
        s.commit_publication(GENERATION, &p).unwrap();
        assert!(s.state().qmi_uc_ready);
        assert_eq!(s.driver_status(), Some(value));
    }
    let cases = [
        message(0, 1, ipa::INDICATION_REGISTER, &[(1, &[0])]),
        message(0, 1, ipa::INDICATION_REGISTER, &[(0x10, &[0, 1])]),
        message(
            0,
            1,
            ipa::INDICATION_REGISTER,
            &[(0x10, &[0]), (0x10, &[1])],
        ),
        message(0, 1, ipa::DRIVER_INIT_COMPLETE, &[]),
        message(0, 1, ipa::DRIVER_INIT_COMPLETE, &[(1, &[])]),
        message(0, 1, ipa::DRIVER_INIT_COMPLETE, &[(1, &[0, 1])]),
        message(2, 1, ipa::DRIVER_INIT_COMPLETE, &[(1, &[0])]),
        message(4, 1, ipa::INDICATION_REGISTER, &[]),
        message(0, 1, 0x36, &[(1, &[0])]),
        message(0, 1, ipa::DRIVER_INIT_COMPLETE, &[(1, &[0]), (0x10, &[0])]),
        message(0, 1, ipa::INDICATION_REGISTER, &[(0x15, &[0; 18])]),
    ];
    for bytes in cases {
        let mut s = new();
        let before = s.state();
        assert!(
            s.prepare_response(GENERATION, CLIENT, &bytes).is_err(),
            "{}",
            hex(&bytes)
        );
        assert_eq!(s.state(), before);
        assert!(
            s.prepare_indication(GENERATION, INDICATION_TRANSACTION)
                .unwrap()
                .is_none()
        );
    }
    let mut s = new();
    let unknown_optional = message(0, 1, ipa::INDICATION_REGISTER, &[(0x15, &[9])]);
    assert!(
        s.prepare_response(GENERATION, CLIENT, &unknown_optional)
            .is_ok()
    );
}

#[test]
fn qmi_wire_truncation_length_and_duplicate_defenses() {
    for bytes in [
        message(0, 1, ipa::DRIVER_INIT_COMPLETE, &[(1, &[0])]),
        message(0, 1, ipa::INDICATION_REGISTER, &[(0x10, &[1])]),
    ] {
        for cut in 0..bytes.len() {
            assert!(
                new()
                    .prepare_response(GENERATION, CLIENT, &bytes[..cut])
                    .is_err()
            );
        }
        for n in [0, 1, 0xffff] {
            let mut wrong = bytes.clone();
            wrong[5..7].copy_from_slice(&u16::to_le_bytes(n));
            assert!(new().prepare_response(GENERATION, CLIENT, &wrong).is_err());
        }
    }
    let bytes = response(TRANSACTION, 0, 0);
    for cut in 0..bytes.len() {
        let mut s = published();
        assert!(
            s.receive_init_response(GENERATION, MODEM, &bytes[..cut])
                .is_err()
        );
        assert!(!s.state().modem_ready);
    }
    for malformed in [
        message(2, TRANSACTION, ipa::INIT_DRIVER, &[]),
        message(2, TRANSACTION, ipa::INIT_DRIVER, &[(2, &[0; 3])]),
        message(
            2,
            TRANSACTION,
            ipa::INIT_DRIVER,
            &[(2, &[0; 4]), (2, &[0; 4])],
        ),
        message(2, TRANSACTION, ipa::INIT_DRIVER, &[(2, &[0; 4]), (1, &[0])]),
    ] {
        let mut s = published();
        assert!(
            s.receive_init_response(GENERATION, MODEM, &malformed)
                .is_err()
        );
        assert!(!s.state().modem_ready);
        assert!(s.response().is_none());
    }
}

#[test]
fn qmi_optional_response_fields_do_not_become_hardware_authority() {
    let bytes = message(
        2,
        TRANSACTION,
        ipa::INIT_DRIVER,
        &[
            (2, &[0; 4]),
            (0x10, &u32::MAX.to_le_bytes()),
            (0x11, &11u32.to_le_bytes()),
            (0x12, &[0xfe]),
        ],
    );
    assert_eq!(bytes.len(), 32);
    let mut s = published();
    s.receive_init_response(GENERATION, MODEM, &bytes).unwrap();
    assert_eq!(
        s.response(),
        Some(ipa::InitResponse {
            control_endpoint: Some(u32::MAX),
            default_endpoint: Some(11),
            driver_init_pending: Some(0xfe)
        })
    );
    assert!(!s.protocol_complete());
    assert!(!s.state().qmi_uc_ready);
    for tag in [0x10, 0x11, 0x12] {
        let malformed = message(
            2,
            TRANSACTION,
            ipa::INIT_DRIVER,
            &[(2, &[0; 4]), (tag, &[0; 2])],
        );
        let mut s = published();
        assert!(
            s.receive_init_response(GENERATION, MODEM, &malformed)
                .is_err()
        );
        assert!(!s.state().modem_ready);
    }
}

#[test]
fn qmi_uncertain_publications_and_loss_are_terminal() {
    assert!(GOLDEN.lines().any(|s| s == "response_failed 0 0 0"));
    assert!(GOLDEN.lines().any(|s| s == "indication_failed 1 1 0 1"));
    for kind in 0..4 {
        let mut s = if kind == 3 {
            ready_before_indication()
        } else {
            new()
        };
        let p = match kind {
            0 => s
                .prepare_init_driver(GENERATION, TRANSACTION, 0x415)
                .unwrap(),
            1 => prepare(&mut s, ipa::INDICATION_REGISTER, 1),
            2 => prepare(&mut s, ipa::DRIVER_INIT_COMPLETE, 1),
            3 => s
                .prepare_indication(GENERATION, INDICATION_TRANSACTION)
                .unwrap()
                .unwrap(),
            _ => unreachable!(),
        };
        let before = s.state();
        s.publication_uncertain(GENERATION, &p).unwrap();
        let mut expected = before;
        expected.failed = true;
        assert_eq!(s.state(), expected);
        assert_eq!(s.commit_publication(GENERATION, &p), Err(Error::Failed));
        assert!(!s.protocol_complete());
    }
    let mut s = ready_before_indication();
    s.peer_lost(GENERATION).unwrap();
    let st = s.state();
    assert_eq!(
        (
            st.modem_ready,
            st.qmi_uc_ready,
            st.indication_requested,
            st.indication_sent,
            st.initial_boot
        ),
        (false, true, false, false, true)
    );
    assert!(GOLDEN.lines().any(|s| s == "bye 0 1 0 0 1"));
    assert!(!s.protocol_complete());
    assert!(st.failed);
    let mut s = published();
    s.fail(GENERATION).unwrap();
    assert_eq!(
        s.receive_init_response(GENERATION, MODEM, &response(TRANSACTION, 0, 0)),
        Err(Error::Failed)
    );
}

#[test]
fn qmi_duplicate_registration_does_not_resend_the_final_indication() {
    let mut s = published();
    for e in b"IRU" {
        event(&mut s, *e);
    }
    assert!(s.protocol_complete());
    let p = prepare(&mut s, ipa::INDICATION_REGISTER, 2);
    s.commit_publication(GENERATION, &p).unwrap();
    assert!(s.protocol_complete());
    assert!(
        s.prepare_indication(GENERATION, INDICATION_TRANSACTION)
            .unwrap()
            .is_none()
    );
}

#[test]
fn qmi_original_send_helper_allocates_indication_ids_including_zero() {
    for (transaction, label) in [
        (0, "complete_indication_zero"),
        (0x2468, "complete_indication"),
    ] {
        let mut s = ready_before_indication();
        let p = s
            .prepare_indication(GENERATION, transaction)
            .unwrap()
            .unwrap();
        assert_eq!(p.bytes(), golden(label));
        assert_eq!(Message::decode(p.bytes()).unwrap().transaction, transaction);
        s.commit_publication(GENERATION, &p).unwrap();
        assert!(s.protocol_complete());
    }
    let mut s = new();
    let p = s.prepare_init_driver(GENERATION, 0, 0x415).unwrap();
    s.commit_publication(GENERATION, &p).unwrap();
    s.receive_init_response(GENERATION, MODEM, &response(0, 0, 0))
        .unwrap();
    assert!(s.state().modem_ready);
}
