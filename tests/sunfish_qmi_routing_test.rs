//! Offline, source-derived wire fixtures; no ADB, MMIO or firmware execution.
#[path = "../rust/src/qcom_modem_services.rs"]
mod qcom_modem_services;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_qmi.rs"]
mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
mod qcom_qmi_router;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
mod qcom_ipa_service;

use qcom_modem_services::{
    Direction, MemoryRequest, Partition, Processor, Request, Service, StorageRequest,
};
use qcom_qmi::{Error as WireError, Kind, Message};
use qcom_qmi_router::{Address, Error, MAX_DATAGRAM_BYTES, MAX_IN_FLIGHT, Router};

const STORAGE_PORT: u32 = 0x4000;
const MEMORY_PORT: u32 = 0x4001;

fn qmi(kind: u8, txn: u16, id: u16, fields: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = vec![kind];
    bytes.extend(txn.to_le_bytes());
    bytes.extend(id.to_le_bytes());
    bytes.extend([0, 0]);
    for (kind, value) in fields {
        bytes.push(*kind);
        bytes.extend(u16::try_from(value.len()).unwrap().to_le_bytes());
        bytes.extend(value);
    }
    let len = u16::try_from(bytes.len() - 7).unwrap();
    bytes[5..7].copy_from_slice(&len.to_le_bytes());
    bytes
}

fn u32_field(kind: u8, value: u32) -> (u8, Vec<u8>) {
    (kind, value.to_le_bytes().to_vec())
}

fn frame(port: u32, message: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [1, 1, 0, 9, 0, message.len() as u32, 1, port] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(message);
    bytes
}

fn set_word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn router() -> Router {
    let mut router = Router::new(1, 0, 1).unwrap();
    router
        .register(Service::RemoteStorage, STORAGE_PORT)
        .unwrap();
    router.register(Service::HeapMemory, MEMORY_PORT).unwrap();
    router
}

fn query(txn: u16) -> Vec<u8> {
    frame(MEMORY_PORT, &qmi(0, txn, 0x24, &[u32_field(1, 1)]))
}

fn allocation_fields() -> Vec<(u8, Vec<u8>)> {
    vec![
        u32_field(1, 0x500000),
        u32_field(2, 1),
        u32_field(3, 0),
        u32_field(4, 0x11223344),
    ]
}

fn decode(service: Service, bytes: &[u8]) -> Result<Request<'_>, WireError> {
    qcom_modem_services::decode(service, &Message::decode(bytes)?)
}

fn iovecs(entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut value = vec![u8::try_from(entries.len()).unwrap()];
    for &(sector, address, count) in entries {
        value.extend(sector.to_le_bytes());
        value.extend(address.to_le_bytes());
        value.extend(count.to_le_bytes());
    }
    value
}

fn io_fields(value: Vec<u8>) -> Vec<(u8, Vec<u8>)> {
    vec![u32_field(1, 7), (2, vec![0]), (3, value), (4, vec![1])]
}

fn heap_blocks(entries: &[(u64, u32)]) -> Vec<u8> {
    let mut value = vec![u8::try_from(entries.len()).unwrap()];
    for &(address, size) in entries {
        value.extend(address.to_le_bytes());
        value.extend(size.to_le_bytes());
    }
    value
}

#[test]
fn literal_storage_open_decodes_without_a_round_trip_encoder() {
    // Independent fixture: request, txn 0x1234, OPEN 1, payload 18;
    // TLV 1 has the 15 raw string bytes, with no nested length or NUL.
    let bytes = b"\x00\x34\x12\x01\x00\x12\x00\x01\x0f\x00/boot/modem_fs1";
    let original = bytes.to_vec();
    let message = Message::decode(bytes).unwrap();
    assert_eq!(
        (message.kind, message.transaction, message.message_id),
        (Kind::Request, 0x1234, 1)
    );
    assert_eq!(message.tlv_count(), 1);
    assert_eq!(
        qcom_modem_services::decode(Service::RemoteStorage, &message),
        Ok(Request::Storage(StorageRequest::Open(Partition::ModemFs1)))
    );
    assert_eq!(bytes, original.as_slice());
}

#[test]
fn servreg_endpoint_routes_only_bounded_get_domain_list_queries() {
    let mut router = Router::new(1, 0, 1).unwrap();
    router.register(Service::RemoteStorage, STORAGE_PORT).unwrap();
    router.register(Service::HeapMemory, MEMORY_PORT).unwrap();
    let endpoint = router
        .register(Service::ServiceRegistry, qcom_servreg::LOCAL_PORT)
        .unwrap();
    assert_eq!(
        (Service::ServiceRegistry.descriptor(), endpoint.address.port),
        ((0x40, 1, 1), 0x4003)
    );
    let request = qmi(0, 0x1234, 0x21, &[(1, b"tms/servreg".to_vec())]);
    let bytes = frame(qcom_servreg::LOCAL_PORT, &request);
    let routed = router.route(1, &bytes).unwrap();
    assert_eq!(
        routed.request,
        Request::ServiceRegistry(qcom_servreg::Query {
            service_name: b"tms/servreg",
            domain_offset: 0,
        })
    );
    assert_eq!(routed.context.service, Service::ServiceRegistry);
    assert_eq!(routed.context.transaction, 0x1234);
    assert_eq!(router.complete(routed.ticket).unwrap().service, Service::ServiceRegistry);
}

#[test]
fn ipa_ap_endpoint_uses_instance_one_and_routes_only_valid_server_requests() {
    const IPA_PORT: u32 = 0x4004;
    assert_eq!(Service::IpaServer.descriptor(), (0x31, 1, 1));
    let mut router = Router::new(1, 0, 1).unwrap();
    router.register(Service::RemoteStorage, STORAGE_PORT).unwrap();
    router.register(Service::HeapMemory, MEMORY_PORT).unwrap();
    router
        .register(Service::ServiceRegistry, qcom_servreg::LOCAL_PORT)
        .unwrap();
    let endpoint = router.register(Service::IpaServer, IPA_PORT).unwrap();
    assert_eq!(endpoint.address.port, IPA_PORT);

    let request = qmi(0, 0x4567, 0x20, &[(0x10, vec![1]), (0x12, vec![0])]);
    let request_frame = frame(IPA_PORT, &request);
    let routed = router.route(1, &request_frame).unwrap();
    assert_eq!(routed.context.service, Service::IpaServer);
    assert_eq!(routed.context.peer, Address { node: 0, port: 9 });
    assert_eq!(routed.context.transaction, 0x4567);
    assert_eq!(
        routed.request,
        Request::IpaServer { message_id: 0x20 }
    );
    assert_eq!(router.complete(routed.ticket).unwrap().service, Service::IpaServer);

    let invalid = qmi(0, 2, 0x20, &[(0x10, vec![2])]);
    assert_eq!(
        router.route(1, &frame(IPA_PORT, &invalid)).unwrap_err(),
        Error::Wire(WireError::InvalidValue)
    );
}

#[test]
fn literal_67_byte_allocation_routes_and_preserves_reply_identity() {
    // Source-derived minimum allocation: QRTR v1 + QMI + four mandatory u32 TLVs.
    let bytes: [u8; 67] = [
        1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 1, 0, 0, 0, 35, 0, 0, 0, 1, 0, 0, 0, 1, 64,
        0, 0, 0, 255, 255, 34, 0, 28, 0, 1, 4, 0, 0, 0, 80, 0, 2, 4, 0, 1, 0, 0, 0, 3, 4, 0, 0, 0,
        0, 0, 4, 4, 0, 68, 51, 34, 17,
    ];
    let mut router = router();
    let mut routed = router.route(1, &bytes).unwrap();
    assert_eq!(
        routed.request,
        Request::Memory(MemoryRequest::Allocate {
            bytes: 0x500000,
            client: 1,
            processor: Processor::Modem,
            sequence: 0x11223344,
            contiguous: None,
            alignment: None,
        })
    );
    assert!(routed.context.confirm_rx);
    // Public diagnostic metadata cannot redirect the private transaction receipt.
    routed.context.peer = Address { node: 23, port: 12 };
    routed.context.transaction = 5;
    let reply = router.complete(routed.ticket).unwrap();
    assert_eq!(
        reply.source,
        Address {
            node: 1,
            port: MEMORY_PORT
        }
    );
    assert_eq!(reply.destination, Address { node: 0, port: 9 });
    assert_eq!(
        (reply.generation, reply.transaction, reply.message_id),
        (1, 65535, 0x22)
    );
    assert_eq!(reply.service, Service::HeapMemory);
    assert_eq!(router.in_flight(), 0);
}

#[test]
fn storage_path_allowlist_is_exact() {
    for (path, partition) in [
        (b"/boot/modem_fs1".as_slice(), Partition::ModemFs1),
        (b"/boot/modem_fs2", Partition::ModemFs2),
        (b"/boot/modem_fsg", Partition::ModemFsg),
        (b"/boot/modem_fsc", Partition::ModemFsc),
    ] {
        let bytes = qmi(0, 1, 1, &[(1, path.to_vec())]);
        assert_eq!(
            decode(Service::RemoteStorage, &bytes),
            Ok(Request::Storage(StorageRequest::Open(partition)))
        );
    }
    for path in [
        b"".as_slice(),
        b"/boot/../modem_fs1",
        b"/boot/modem_fs1\0",
        b"/boot/modem_fs10",
        b"/boot/modem_study",
        b"\xff",
    ] {
        assert_eq!(
            decode(Service::RemoteStorage, &qmi(0, 1, 1, &[(1, path.to_vec())])),
            Err(WireError::InvalidValue)
        );
    }
}

#[test]
fn storage_handles_are_decoded_as_data_not_authority() {
    for id in [2, 5] {
        let bytes = qmi(0, 0, id, &[u32_field(1, u32::MAX)]);
        let expected = if id == 2 {
            StorageRequest::Close { handle: u32::MAX }
        } else {
            StorageRequest::DeviceError { handle: u32::MAX }
        };
        assert_eq!(
            decode(Service::RemoteStorage, &bytes),
            Ok(Request::Storage(expected))
        );
    }
    assert_eq!(
        decode(Service::RemoteStorage, &qmi(0, 1, 6, &[])),
        Err(WireError::UnsupportedMessage)
    );
}

#[test]
fn storage_allocations_enforce_the_initial_pool_ceiling() {
    for size in [1, 0x200000] {
        let bytes = qmi(0, 1, 4, &[u32_field(1, 7), u32_field(2, size)]);
        assert_eq!(
            decode(Service::RemoteStorage, &bytes),
            Ok(Request::Storage(StorageRequest::Allocate {
                handle: 7,
                bytes: size
            }))
        );
    }
    for size in [0, 0x200001, u32::MAX] {
        assert_eq!(
            decode(
                Service::RemoteStorage,
                &qmi(0, 1, 4, &[u32_field(1, 7), u32_field(2, size)])
            ),
            Err(WireError::InvalidValue)
        );
    }
}

#[test]
fn storage_iovecs_use_wide_disk_offsets_and_borrowed_entries() {
    let bytes = qmi(
        0,
        1,
        3,
        &io_fields(iovecs(&[(u32::MAX, 0xf4601000, 1), (2, 0xf4601200, 4095)])),
    );
    let Request::Storage(StorageRequest::ReadWrite {
        handle,
        direction,
        iovecs,
        force_sync,
    }) = decode(Service::RemoteStorage, &bytes).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (handle, direction, iovecs.len(), force_sync),
        (7, Direction::Read, 2, true)
    );
    let entries: Vec<_> = iovecs.iter().collect();
    assert_eq!(entries[0].disk_offset(), u64::from(u32::MAX) * 512);
    assert_eq!(entries[1].byte_count(), 4095 * 512);
    assert!(!iovecs.is_empty());
    let mut fields = io_fields(vec![0]);
    fields[1].1 = vec![1];
    fields[3].1 = vec![0];
    let bytes = qmi(0, 1, 3, &fields);
    let Request::Storage(StorageRequest::ReadWrite {
        direction,
        iovecs,
        force_sync,
        ..
    }) = decode(Service::RemoteStorage, &bytes).unwrap()
    else {
        panic!()
    };
    assert_eq!(direction, Direction::Write);
    assert!(iovecs.is_empty());
    assert!(!force_sync);
}

#[test]
fn storage_array_counts_transfer_limits_and_physical_wrap_are_checked() {
    let many = vec![(0, 0x100000, 1); 255];
    let bytes = qmi(0, 1, 3, &io_fields(iovecs(&many)));
    let Request::Storage(StorageRequest::ReadWrite { iovecs: parsed, .. }) =
        decode(Service::RemoteStorage, &bytes).unwrap()
    else {
        panic!()
    };
    assert_eq!(parsed.len(), 255);
    for value in [vec![], vec![1], vec![0, 0], {
        let mut v = iovecs_bytes();
        v[0] = 2;
        v
    }] {
        assert!(decode(Service::RemoteStorage, &qmi(0, 1, 3, &io_fields(value))).is_err());
    }
    let too_much = iovecs(&[(0, 0x100000, 4096), (0, 0x400000, 1)]);
    assert_eq!(
        decode(Service::RemoteStorage, &qmi(0, 1, 3, &io_fields(too_much))),
        Err(WireError::InvalidValue)
    );
    let wrap = iovecs(&[(0, 0xffffff00, 1)]);
    assert_eq!(
        decode(Service::RemoteStorage, &qmi(0, 1, 3, &io_fields(wrap))),
        Err(WireError::RangeOverflow)
    );
    // An exclusive endpoint exactly at 2^32 is representable with wide arithmetic.
    assert!(
        decode(
            Service::RemoteStorage,
            &qmi(0, 1, 3, &io_fields(iovecs(&[(0, 0xfffffe00, 1)])))
        )
        .is_ok()
    );
}

fn iovecs_bytes() -> Vec<u8> {
    iovecs(&[(0, 0x100000, 1)])
}

#[test]
fn storage_missing_fields_scalar_lengths_and_boolean_domains_fail() {
    let fields = io_fields(iovecs_bytes());
    for index in 0..fields.len() {
        let mut missing = fields.clone();
        let kind = missing.remove(index).0;
        assert_eq!(
            decode(Service::RemoteStorage, &qmi(0, 1, 3, &missing)),
            Err(WireError::MissingTlv(kind))
        );
    }
    for index in [1, 3] {
        let mut bad = fields.clone();
        bad[index].1 = vec![2];
        assert_eq!(
            decode(Service::RemoteStorage, &qmi(0, 1, 3, &bad)),
            Err(WireError::InvalidValue)
        );
    }
    let mut bad = fields;
    bad[0].1.pop();
    assert_eq!(
        decode(Service::RemoteStorage, &qmi(0, 1, 3, &bad)),
        Err(WireError::InvalidTlvLength(1))
    );
}

#[test]
fn memory_query_preserves_optional_processor_presence() {
    let bytes = qmi(0, 0, 0x24, &[u32_field(1, 1)]);
    assert_eq!(
        decode(Service::HeapMemory, &bytes),
        Ok(Request::Memory(MemoryRequest::QuerySize {
            client: 1,
            processor: None
        }))
    );
    for (value, processor) in [
        (0, Processor::Modem),
        (1, Processor::Adsp),
        (2, Processor::Wcnss),
    ] {
        let bytes = qmi(0, 1, 0x24, &[u32_field(1, 1), u32_field(0x10, value)]);
        assert_eq!(
            decode(Service::HeapMemory, &bytes),
            Ok(Request::Memory(MemoryRequest::QuerySize {
                client: 1,
                processor: Some(processor)
            }))
        );
    }
    assert_eq!(
        decode(
            Service::HeapMemory,
            &qmi(0, 1, 0x24, &[u32_field(1, 1), u32_field(0x10, 3)])
        ),
        Err(WireError::InvalidValue)
    );
    assert_eq!(
        decode(
            Service::HeapMemory,
            &qmi(0, 1, 0x24, &[u32_field(1, 1), (0x10, vec![0])])
        ),
        Err(WireError::InvalidTlvLength(0x10))
    );
}

#[test]
fn memory_allocate_optional_fields_and_limits_are_checked() {
    for align in [0, 11] {
        for contiguous in [0, 1] {
            let mut fields = allocation_fields();
            fields.push((0x10, vec![contiguous]));
            fields.push(u32_field(0x11, align));
            let bytes = qmi(0, 1, 0x22, &fields);
            let Request::Memory(MemoryRequest::Allocate {
                bytes,
                contiguous: flag,
                alignment,
                ..
            }) = decode(Service::HeapMemory, &bytes).unwrap()
            else {
                panic!()
            };
            assert_eq!(bytes, 0x500000);
            assert_eq!(flag, Some(contiguous == 1));
            assert_eq!(alignment, Some(align as u8));
        }
    }
    for optional in [
        (0x10, vec![2]),
        u32_field(0x11, 12),
        u32_field(0x11, u32::MAX),
        (0x10, vec![]),
    ] {
        let mut fields = allocation_fields();
        fields.push(optional);
        assert!(decode(Service::HeapMemory, &qmi(0, 1, 0x22, &fields)).is_err());
    }
    for size in [0, 0x500001, u32::MAX] {
        let mut fields = allocation_fields();
        fields[0] = u32_field(1, size);
        assert_eq!(
            decode(Service::HeapMemory, &qmi(0, 1, 0x22, &fields)),
            Err(WireError::InvalidValue)
        );
    }
}

#[test]
fn every_mandatory_memory_allocation_field_is_required_and_sized() {
    for index in 0..4 {
        let mut fields = allocation_fields();
        let kind = fields.remove(index).0;
        assert_eq!(
            decode(Service::HeapMemory, &qmi(0, 1, 0x22, &fields)),
            Err(WireError::MissingTlv(kind))
        );
        let mut fields = allocation_fields();
        fields[index].1.push(0);
        assert_eq!(
            decode(Service::HeapMemory, &qmi(0, 1, 0x22, &fields)),
            Err(WireError::InvalidTlvLength(kind))
        );
    }
    let mut fields = allocation_fields();
    fields[2] = u32_field(3, 99);
    assert_eq!(
        decode(Service::HeapMemory, &qmi(0, 1, 0x22, &fields)),
        Err(WireError::InvalidValue)
    );
}

#[test]
fn memory_free_decodes_64_bit_addresses_without_dereferencing_them() {
    let bytes = qmi(
        0,
        1,
        0x23,
        &[
            (1, heap_blocks(&[(0x110000000, 4096)])),
            u32_field(0x10, 1),
            u32_field(0x11, 0),
        ],
    );
    let Request::Memory(MemoryRequest::Free {
        blocks,
        client,
        processor,
    }) = decode(Service::HeapMemory, &bytes).unwrap()
    else {
        panic!()
    };
    assert_eq!((client, processor), (Some(1), Some(Processor::Modem)));
    assert_eq!(blocks.len(), 1);
    assert!(!blocks.is_empty());
    assert_eq!(blocks.iter().next().unwrap().physical_address, 0x110000000);
    let bytes = qmi(0, 1, 0x23, &[(1, vec![0])]);
    let Request::Memory(MemoryRequest::Free {
        blocks,
        client,
        processor,
    }) = decode(Service::HeapMemory, &bytes).unwrap()
    else {
        panic!()
    };
    assert!(blocks.is_empty());
    assert_eq!((client, processor), (None, None));
}

#[test]
fn memory_free_array_limits_lengths_and_ranges_fail_closed() {
    let entries = vec![(0x100000, 4096); 64];
    let bytes = qmi(0, 1, 0x23, &[(1, heap_blocks(&entries))]);
    let Request::Memory(MemoryRequest::Free { blocks, .. }) =
        decode(Service::HeapMemory, &bytes).unwrap()
    else {
        panic!()
    };
    assert_eq!(blocks.len(), 64);
    for value in [
        heap_blocks(&vec![(0x100000, 4096); 65]),
        vec![],
        vec![1],
        vec![0, 0],
    ] {
        assert!(decode(Service::HeapMemory, &qmi(0, 1, 0x23, &[(1, value)])).is_err());
    }
    for entries in [
        vec![(u64::MAX, 1)],
        vec![(0x100000, 0)],
        vec![(0x100000, 0x500001)],
        vec![(0x100000, 0x500000), (0x900000, 1)],
    ] {
        assert!(
            decode(
                Service::HeapMemory,
                &qmi(0, 1, 0x23, &[(1, heap_blocks(&entries))])
            )
            .is_err()
        );
    }
}

#[test]
fn duplicate_tlvs_are_rejected_even_for_ignored_optional_fields() {
    for kind in [1, 0x10, 0xe0] {
        let bytes = qmi(0, 1, 1, &[(kind, vec![]), (kind, vec![])]);
        assert_eq!(Message::decode(&bytes), Err(WireError::DuplicateTlv(kind)));
    }
}

#[test]
fn unknown_optional_fields_are_bounded_and_mandatory_fields_are_rejected() {
    let mut fields = vec![(1, b"/boot/modem_fs1".to_vec())];
    fields.extend((0x80..0x8f).map(|kind| (kind, vec![])));
    let bytes = qmi(0, 1, 1, &fields);
    assert_eq!(Message::decode(&bytes).unwrap().tlv_count(), 16);
    assert!(decode(Service::RemoteStorage, &bytes).is_ok());
    fields.push((0xf0, vec![]));
    assert_eq!(
        Message::decode(&qmi(0, 1, 1, &fields)),
        Err(WireError::TooManyTlvs)
    );
    let bytes = qmi(0, 1, 1, &[(1, b"/boot/modem_fs1".to_vec()), (9, vec![])]);
    assert_eq!(
        decode(Service::RemoteStorage, &bytes),
        Err(WireError::UnknownMandatoryTlv(9))
    );
}

#[test]
fn incomplete_tlv_headers_values_and_envelope_lengths_are_rejected() {
    for payload in [vec![1], vec![1, 0], vec![1, 1, 0], vec![1, 255, 255, 0]] {
        let mut bytes = vec![0, 1, 0, 1, 0];
        bytes.extend((payload.len() as u16).to_le_bytes());
        bytes.extend(payload);
        assert_eq!(Message::decode(&bytes), Err(WireError::TruncatedTlv));
    }
    let mut bytes = qmi(0, 1, 1, &[]);
    bytes[5] = 1;
    assert_eq!(Message::decode(&bytes), Err(WireError::LengthMismatch));
    for size in 0..7 {
        assert_eq!(
            Message::decode(&[0; 7][..size]),
            Err(WireError::ShortHeader)
        );
    }
}

#[test]
fn response_indication_and_reserved_flags_never_enter_request_handlers() {
    for (flag, kind) in [
        (0, Kind::Request),
        (2, Kind::Response),
        (4, Kind::Indication),
    ] {
        let bytes = qmi(flag, 1, 0x24, &[u32_field(1, 1)]);
        assert_eq!(Message::decode(&bytes).unwrap().kind, kind);
        if flag != 0 {
            assert_eq!(
                router().route(1, &frame(MEMORY_PORT, &bytes)).unwrap_err(),
                Error::NotRequest
            );
            assert_eq!(
                decode(Service::HeapMemory, &bytes),
                Err(WireError::UnsupportedKind)
            );
        }
    }
    for flag in [1, 3, 5, 6, 255] {
        assert_eq!(
            Message::decode(&qmi(flag, 1, 0x24, &[])),
            Err(WireError::UnsupportedKind)
        );
    }
}

#[test]
fn router_rejects_wrong_headers_peers_ports_and_lengths_without_reserving_slots() {
    let good = query(1);
    for (offset, value, error) in [
        (0, 2, Error::UnsupportedVersion),
        (4, 4, Error::UnsupportedPacket),
        (8, 5, Error::WrongPeer),
        (12, 0, Error::InvalidPort),
        (12, 0xfffffffe, Error::InvalidPort),
        (12, 0xffffffff, Error::InvalidPort),
        (16, 2, Error::InvalidConfirmRx),
        (20, 0xffffffff, Error::Wire(WireError::LengthMismatch)),
        (24, 5, Error::WrongDestination),
        (28, 0x4002, Error::NoEndpoint),
        (28, 0, Error::InvalidPort),
        (28, 0xfffffffe, Error::InvalidPort),
    ] {
        let mut bytes = good.clone();
        set_word(&mut bytes, offset, value);
        let mut router = router();
        assert_eq!(
            router.route(1, &bytes).unwrap_err(),
            error,
            "word at {offset}"
        );
        assert_eq!(router.in_flight(), 0);
    }
    let mut trailing = good;
    trailing.push(0);
    assert_eq!(
        router().route(1, &trailing).unwrap_err(),
        Error::Wire(WireError::LengthMismatch)
    );
}

#[test]
fn router_configuration_rejects_ambiguous_endpoints_and_reserved_addresses() {
    let configured = router();
    assert_eq!(
        configured
            .endpoint(Service::HeapMemory)
            .unwrap()
            .address
            .port,
        MEMORY_PORT
    );
    assert_eq!(Router::new(1, 0, 0).err(), Some(Error::InvalidGeneration));
    for (local, peer) in [(0, 0), (1, 1), (0xfffffffe, 0), (1, 0xffffffff)] {
        assert_eq!(Router::new(local, peer, 1).err(), Some(Error::InvalidNode));
    }
    for port in [0, 0xfffffffe, 0xffffffff] {
        assert_eq!(
            Router::new(1, 0, 1)
                .unwrap()
                .register(Service::HeapMemory, port),
            Err(Error::InvalidPort)
        );
    }
    let mut router = Router::new(1, 0, 1).unwrap();
    router
        .register(Service::RemoteStorage, STORAGE_PORT)
        .unwrap();
    assert_eq!(
        router.register(Service::RemoteStorage, 0x5000),
        Err(Error::EndpointBusy)
    );
    assert_eq!(
        router.register(Service::HeapMemory, STORAGE_PORT),
        Err(Error::EndpointBusy)
    );
    assert_eq!(Service::RemoteStorage.descriptor(), (14, 1, 0));
    assert_eq!(Service::HeapMemory.descriptor(), (52, 1, 1));
}

#[test]
fn pending_transaction_capacity_is_fixed_and_cancel_releases_one_slot() {
    let packets: Vec<_> = (0..=MAX_IN_FLIGHT).map(|txn| query(txn as u16)).collect();
    let mut router = router();
    let mut tickets = Vec::new();
    for bytes in &packets[..MAX_IN_FLIGHT] {
        tickets.push(router.route(1, bytes).unwrap().ticket);
    }
    assert_eq!(router.in_flight(), MAX_IN_FLIGHT);
    assert_eq!(
        router.route(1, &packets[MAX_IN_FLIGHT]).unwrap_err(),
        Error::InFlightFull
    );
    // Malformed inputs are still validated and cannot disturb a full queue.
    assert_eq!(
        router.route(1, &[]).unwrap_err(),
        Error::Wire(WireError::ShortHeader)
    );
    assert_eq!(router.in_flight(), MAX_IN_FLIGHT);
    router.cancel(tickets.pop().unwrap()).unwrap();
    let routed = router.route(1, &packets[MAX_IN_FLIGHT]).unwrap();
    assert_eq!(router.in_flight(), MAX_IN_FLIGHT);
    router.complete(routed.ticket).unwrap();
    for ticket in tickets {
        router.cancel(ticket).unwrap();
    }
    assert_eq!(router.in_flight(), 0);
}

#[test]
fn service_selection_comes_from_the_registered_destination_not_the_message_id() {
    let memory_request = qmi(0, 1, 0x22, &allocation_fields());
    let storage_request = qmi(0, 2, 1, &[(1, b"/boot/modem_fs1".to_vec())]);
    let mut router = router();
    for bytes in [
        frame(STORAGE_PORT, &memory_request),
        frame(MEMORY_PORT, &storage_request),
    ] {
        assert_eq!(
            router.route(1, &bytes).unwrap_err(),
            Error::Wire(WireError::UnsupportedMessage)
        );
        assert_eq!(router.in_flight(), 0);
    }
    for bytes in [
        frame(MEMORY_PORT, &memory_request),
        frame(STORAGE_PORT, &storage_request),
    ] {
        let routed = router.route(1, &bytes).unwrap();
        router.complete(routed.ticket).unwrap();
    }
}

#[test]
fn qrtr_and_qmi_lengths_are_validated_independently() {
    let mut bytes = query(1);
    bytes[32 + 5] = 0xff;
    bytes[32 + 6] = 0xff;
    let mut router = router();
    assert_eq!(
        router.route(1, &bytes).unwrap_err(),
        Error::Wire(WireError::LengthMismatch)
    );
    assert_eq!(router.in_flight(), 0);
}

#[test]
fn duplicate_transaction_keys_include_peer_port_and_endpoint_but_not_message_id() {
    let mut router = router();
    let first = query(0x1234);
    let a = router.route(1, &first).unwrap();
    assert_eq!(
        router.route(1, &first).unwrap_err(),
        Error::DuplicateTransaction
    );
    let different_id = frame(MEMORY_PORT, &qmi(0, 0x1234, 0x22, &allocation_fields()));
    assert_eq!(
        router.route(1, &different_id).unwrap_err(),
        Error::DuplicateTransaction
    );
    let mut other_peer_port = first.clone();
    set_word(&mut other_peer_port, 12, 10);
    let b = router.route(1, &other_peer_port).unwrap();
    let other_endpoint = frame(STORAGE_PORT, &qmi(0, 0x1234, 2, &[u32_field(1, 0)]));
    let c = router.route(1, &other_endpoint).unwrap();
    assert_eq!(router.in_flight(), 3);
    for ticket in [a.ticket, b.ticket, c.ticket] {
        router.complete(ticket).unwrap();
    }
}

#[test]
fn a_new_generation_invalidates_old_packets_tickets_and_registrations() {
    let bytes = query(1);
    let mut router = router();
    let old = router.route(1, &bytes).unwrap();
    assert_eq!(router.begin_generation(1), Err(Error::InvalidGeneration));
    assert_eq!(router.begin_generation(0), Err(Error::InvalidGeneration));
    assert_eq!(router.in_flight(), 1);
    router.begin_generation(2).unwrap();
    assert_eq!(router.in_flight(), 0);
    assert_eq!(router.complete(old.ticket), Err(Error::StaleGeneration));
    assert_eq!(router.route(1, &bytes).unwrap_err(), Error::StaleGeneration);
    assert_eq!(router.route(2, &bytes).unwrap_err(), Error::NoEndpoint);
    router.register(Service::HeapMemory, MEMORY_PORT).unwrap();
    let new = router.route(2, &bytes).unwrap();
    assert_eq!(router.complete(new.ticket).unwrap().generation, 2);
}

#[test]
fn tickets_from_another_router_cannot_retire_a_same_numbered_slot() {
    let bytes = query(1);
    let mut a = router();
    let mut b = router();
    let first = a.route(1, &bytes).unwrap();
    let second = b.route(1, &bytes).unwrap();
    assert_eq!(b.complete(first.ticket), Err(Error::InvalidTicket));
    assert_eq!(a.in_flight(), 1);
    assert_eq!(b.in_flight(), 1);
    b.complete(second.ticket).unwrap();
    a.begin_generation(2).unwrap();
}

#[test]
fn the_complete_4096_byte_datagram_fits_but_an_extra_byte_does_not() {
    let fields = [(1, b"/boot/modem_fs1".to_vec()), (0x80, vec![0x5a; 4036])];
    let bytes = frame(STORAGE_PORT, &qmi(0, 1, 1, &fields));
    assert_eq!(bytes.len(), MAX_DATAGRAM_BYTES);
    let mut router = router();
    let routed = router.route(1, &bytes).unwrap();
    router.complete(routed.ticket).unwrap();
    let mut oversized = bytes;
    oversized.push(0);
    assert_eq!(
        router.route(1, &oversized).unwrap_err(),
        Error::Wire(WireError::TooLarge)
    );
    let message = qmi(0, 1, 1, &[(0x80, vec![0; 4086])]);
    assert_eq!(message.len(), qcom_qmi::MAX_MESSAGE_BYTES);
    assert!(Message::decode(&message).is_ok());
    let mut oversized = message;
    oversized.push(0);
    assert_eq!(Message::decode(&oversized), Err(WireError::TooLarge));
}

#[test]
fn every_truncation_of_a_valid_allocation_fails_without_input_or_state_changes() {
    let bytes = frame(MEMORY_PORT, &qmi(0, 1, 0x22, &allocation_fields()));
    assert_eq!(bytes.len(), 67);
    for cut in 0..bytes.len() {
        let input = bytes[..cut].to_vec();
        let original = input.clone();
        let mut router = router();
        assert!(router.route(1, &input).is_err(), "accepted cut {cut}");
        assert_eq!(router.in_flight(), 0);
        assert_eq!(input, original);
    }
}

#[test]
fn deterministic_wire_mutations_never_panic_or_leave_partial_reservations() {
    let bytes = frame(MEMORY_PORT, &qmi(0, 1, 0x22, &allocation_fields()));
    for at in 0..bytes.len() {
        for value in [0, 1, 0x7f, 0x80, 0xff] {
            let mut input = bytes.clone();
            input[at] = value;
            let original = input.clone();
            let mut router = router();
            match router.route(1, &input) {
                Ok(routed) => {
                    router.complete(routed.ticket).unwrap();
                }
                Err(_) => assert_eq!(router.in_flight(), 0),
            }
            assert_eq!(router.in_flight(), 0);
            assert_eq!(input, original);
        }
    }
}

#[test]
fn arbitrary_tlv_tails_remain_bounded_and_never_panic() {
    let mut seed = 0x5a171912u32;
    for len in 0..512 {
        let mut payload = vec![0; len];
        for byte in &mut payload {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        let mut bytes = vec![0, 1, 0, 0x22, 0];
        bytes.extend((payload.len() as u16).to_le_bytes());
        bytes.extend(payload);
        if let Ok(message) = Message::decode(&bytes) {
            assert!(message.tlv_count() <= 16);
            let _ = qcom_modem_services::decode(Service::HeapMemory, &message);
        }
    }
}

#[test]
fn inspecting_reply_route_preserves_pending_key_until_completion() {
    let mut router = router();
    let bytes = query(1);
    let routed = router.route(1, &bytes).unwrap();
    let route = router.reply_route(&routed.ticket).unwrap();
    assert_eq!(router.reply_route(&routed.ticket).unwrap(), route);
    assert_eq!(router.in_flight(), 1);
    assert!(matches!(
        router.route(1, &bytes),
        Err(Error::DuplicateTransaction)
    ));
    assert_eq!(router.complete(routed.ticket).unwrap(), route);
    assert_eq!(router.in_flight(), 0);
}
