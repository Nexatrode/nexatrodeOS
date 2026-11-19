#[path = "../rust/src/qcacld_wmi.rs"]
mod qcacld_wmi;
#[allow(dead_code)]
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;

use qcacld_wmi::{SUNFISH_WCN3998_FIRMWARE_BUILD, ServiceReadyView};
use qcom_htc::{
    ConnectServiceRequest, HTC_CONTROL_MESSAGE_MAX, HTC_CREDIT_REPORT_SIZE, HTC_FLAG_RECV_TRAILER,
    HTC_FRAME_HEADER_SIZE, HTC_MAX_FRAME_PAYLOAD, HTC_RECORD_CREDITS, HTC_SERVICE_METADATA_MAX,
    HTC_WMI_CONTROL_SERVICE_ID, HtcError, HtcPhase, HtcStartup, encode_connect_service_request,
    encode_control_frame, encode_setup_complete_ex, encode_wmi_command_frame,
    parse_connect_service_response, parse_plain_frame, parse_ready_ex, parse_wmi_frame,
    parse_wmi_tx_frame,
};

const READY: [u8; 12] = [1, 0, 4, 0, 64, 0, 9, 0, 1, 4, 0, 0];
const CONNECT_RESPONSE: [u8; 12] = [3, 0, 0, 1, 0, 2, 0xf8, 0x0f, 0, 0, 0, 0];

fn ready() -> qcom_htc::HtcReady {
    parse_ready_ex(&READY).unwrap()
}

fn connect_response_with(endpoint: u8, max_message_size: u16) -> [u8; 12] {
    let encoded = (u32::from(endpoint) << 8) | (u32::from(max_message_size) << 16);
    [
        3,
        0,
        0,
        1,
        encoded as u8,
        (encoded >> 8) as u8,
        (encoded >> 16) as u8,
        (encoded >> 24) as u8,
        0,
        0,
        0,
        0,
    ]
}

fn append_tlv(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(value);
}

fn sunfish_service_ready_fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut fixed = [0u8; 128];
    let abi = [0x0100_0000u32, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0];
    fixed[..4].copy_from_slice(&SUNFISH_WCN3998_FIRMWARE_BUILD.to_le_bytes());
    for (index, word) in abi.into_iter().enumerate() {
        let start = 4 + index * 4;
        fixed[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }
    append_tlv(&mut bytes, 32, &fixed); // WMI service-ready fixed parameters
    append_tlv(&mut bytes, 33, &[0; 36]); // HAL register capabilities
    append_tlv(&mut bytes, 16, &[0; 128]); // service bitmap
    append_tlv(&mut bytes, 18, &[]); // empty host-memory request array
    bytes
}

#[test]
fn ready_ex_parses_source_layout_and_clamps_bundle_count() {
    let parsed = ready();
    assert_eq!(parsed.credit_count, 4);
    assert_eq!(parsed.credit_size, 64);
    assert_eq!(parsed.max_endpoints, 9);
    assert_eq!(parsed.htc_version, 1);
    assert_eq!(parsed.max_messages_per_bundle, 4);
    assert_eq!(parsed.alternate_data_credit_size, 0);

    let mut no_bundle = READY;
    no_bundle[9] = 0;
    assert_eq!(
        parse_ready_ex(&no_bundle).unwrap().max_messages_per_bundle,
        1
    );
}

#[test]
fn ready_ex_rejects_every_truncation_and_trailing_data() {
    for length in 0..READY.len() {
        assert!(matches!(
            parse_ready_ex(&READY[..length]),
            Err(HtcError::Truncated { .. })
        ));
    }
    let mut extra = [0u8; 13];
    extra[..READY.len()].copy_from_slice(&READY);
    assert!(matches!(
        parse_ready_ex(&extra),
        Err(HtcError::UnexpectedLength { .. })
    ));
}

#[test]
fn ready_ex_rejects_invalid_ids_resources_versions_and_reserved_bits() {
    let mut bytes = READY;
    bytes[0] = 9;
    assert!(matches!(
        parse_ready_ex(&bytes),
        Err(HtcError::WrongMessageId { .. })
    ));

    let mut bytes = READY;
    bytes[2] = 0;
    bytes[3] = 0;
    assert_eq!(parse_ready_ex(&bytes), Err(HtcError::ZeroCreditCount));

    let mut bytes = READY;
    bytes[4] = 0;
    bytes[5] = 0;
    assert_eq!(parse_ready_ex(&bytes), Err(HtcError::ZeroCreditSize));

    let mut bytes = READY;
    bytes[7] = 1;
    assert_eq!(parse_ready_ex(&bytes), Err(HtcError::BadReservedBits));

    let mut bytes = READY;
    bytes[6] = 1;
    assert!(matches!(
        parse_ready_ex(&bytes),
        Err(HtcError::InvalidEndpointLimit { .. })
    ));

    let mut bytes = READY;
    bytes[8] = 2;
    assert!(matches!(
        parse_ready_ex(&bytes),
        Err(HtcError::UnsupportedHtcVersion { .. })
    ));

    let mut bytes = READY;
    bytes[11] = 0x10;
    assert_eq!(parse_ready_ex(&bytes), Err(HtcError::BadReservedBits));
}

#[test]
fn qcacld_one_wmi_endpoint_receives_all_initial_credits_without_narrowing() {
    assert_eq!(ready().initial_wmi_credit_allocation(), Ok(4));
    let mut too_many = READY;
    too_many[2] = 0;
    too_many[3] = 1;
    assert_eq!(
        parse_ready_ex(&too_many)
            .unwrap()
            .initial_wmi_credit_allocation(),
        Err(HtcError::CreditAllocationTooLarge { observed: 256 })
    );
}

#[test]
fn connect_request_encodes_wmi_control_service_and_little_endian_fields() {
    let mut output = [0xa5; 12];
    let written = encode_connect_service_request(
        ConnectServiceRequest {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            connection_flags: 0,
            look_ahead_v2: false,
            metadata: &[],
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(written, 8);
    assert_eq!(&output[..written], &[2, 0, 0, 1, 0, 0, 0, 0]);
    assert_eq!(output[8], 0xa5);
}

#[test]
fn connect_request_encodes_allowed_flags_lookahead_and_metadata() {
    let metadata = [0xaa, 0xbb, 0xcc];
    let mut output = [0u8; 16];
    let written = encode_connect_service_request(
        ConnectServiceRequest {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            connection_flags: 0x0105,
            look_ahead_v2: true,
            metadata: &metadata,
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(written, 11);
    assert_eq!(&output[..8], &[2, 0, 0, 1, 5, 1, 3, 1]);
    assert_eq!(&output[8..written], &metadata);
}

#[test]
fn connect_request_rejects_reserved_flags_zero_service_long_metadata_and_short_output() {
    let mut output = [0x5a; 140];
    let base = ConnectServiceRequest {
        service_id: HTC_WMI_CONTROL_SERVICE_ID,
        connection_flags: 0x20,
        look_ahead_v2: false,
        metadata: &[],
    };
    assert!(matches!(
        encode_connect_service_request(base, &mut output),
        Err(HtcError::InvalidConnectionFlags { .. })
    ));
    assert_eq!(output[0], 0x5a);

    let zero_service = ConnectServiceRequest {
        service_id: 0,
        ..base
    };
    assert!(matches!(
        encode_connect_service_request(zero_service, &mut output),
        Err(HtcError::WrongServiceId { .. })
    ));

    let too_much_metadata = [0u8; HTC_SERVICE_METADATA_MAX + 1];
    let long = ConnectServiceRequest {
        metadata: &too_much_metadata,
        connection_flags: 0,
        ..base
    };
    assert!(matches!(
        encode_connect_service_request(long, &mut output),
        Err(HtcError::MetadataTooLong { .. })
    ));

    let valid = ConnectServiceRequest {
        connection_flags: 0,
        ..base
    };
    assert!(matches!(
        encode_connect_service_request(valid, &mut output[..7]),
        Err(HtcError::OutputTooSmall { .. })
    ));
}

#[test]
fn connect_response_parses_and_borrows_optional_metadata() {
    let response =
        parse_connect_service_response(&CONNECT_RESPONSE, HTC_WMI_CONTROL_SERVICE_ID, ready())
            .unwrap();
    assert_eq!(response.service_id, HTC_WMI_CONTROL_SERVICE_ID);
    assert_eq!(response.endpoint_id, 2);
    assert_eq!(response.max_message_size, HTC_MAX_FRAME_PAYLOAD as u16);
    assert!(!response.look_ahead_v2);
    assert!(response.metadata.is_empty());

    let mut with_metadata = [0u8; 14];
    with_metadata[..CONNECT_RESPONSE.len()].copy_from_slice(&CONNECT_RESPONSE);
    with_metadata[8] = 2;
    with_metadata[12..].copy_from_slice(&[0x12, 0x34]);
    let response =
        parse_connect_service_response(&with_metadata, HTC_WMI_CONTROL_SERVICE_ID, ready())
            .unwrap();
    assert_eq!(response.metadata, &[0x12, 0x34]);
}

#[test]
fn connect_response_rejects_truncation_length_and_wrong_identifiers() {
    for length in 0..12 {
        assert!(matches!(
            parse_connect_service_response(
                &CONNECT_RESPONSE[..length],
                HTC_WMI_CONTROL_SERVICE_ID,
                ready()
            ),
            Err(HtcError::Truncated { .. })
        ));
    }
    let mut extra = [0u8; 13];
    extra[..12].copy_from_slice(&CONNECT_RESPONSE);
    assert!(matches!(
        parse_connect_service_response(&extra, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::UnexpectedLength { .. })
    ));

    let mut wrong_id = CONNECT_RESPONSE;
    wrong_id[0] = 2;
    assert!(matches!(
        parse_connect_service_response(&wrong_id, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::WrongMessageId { .. })
    ));
    assert!(matches!(
        parse_connect_service_response(&CONNECT_RESPONSE, 0x0300, ready()),
        Err(HtcError::WrongServiceId { .. })
    ));
}

#[test]
fn connect_response_rejects_status_reserved_metadata_endpoint_and_size_errors() {
    let mut rejected = CONNECT_RESPONSE;
    rejected[4] = 1;
    assert_eq!(
        parse_connect_service_response(&rejected, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::ServiceRejected { status: 1 })
    );

    let mut reserved = CONNECT_RESPONSE;
    reserved[9] = 2;
    assert_eq!(
        parse_connect_service_response(&reserved, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::BadReservedBits)
    );

    let mut too_much_metadata = [0u8; 141];
    too_much_metadata[..12].copy_from_slice(&CONNECT_RESPONSE);
    too_much_metadata[8] = 129;
    assert!(matches!(
        parse_connect_service_response(&too_much_metadata, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::MetadataTooLong { observed: 129 })
    ));

    let endpoint_zero = connect_response_with(0, 64);
    assert!(matches!(
        parse_connect_service_response(&endpoint_zero, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::InvalidEndpoint { observed: 0 })
    ));
    let endpoint_past_target = connect_response_with(9, 64);
    assert!(matches!(
        parse_connect_service_response(&endpoint_past_target, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::InvalidEndpoint { observed: 9 })
    ));
    let zero_size = connect_response_with(2, 0);
    assert!(matches!(
        parse_connect_service_response(&zero_size, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::InvalidMaxMessageSize { observed: 0 })
    ));
    let too_large = connect_response_with(2, 4089);
    assert!(matches!(
        parse_connect_service_response(&too_large, HTC_WMI_CONTROL_SERVICE_ID, ready()),
        Err(HtcError::InvalidMaxMessageSize { observed: 4089 })
    ));
}

#[test]
fn setup_complete_ex_matches_zero_initialized_pinned_driver_message() {
    let mut output = [0xa5; 12];
    assert_eq!(encode_setup_complete_ex(&mut output).unwrap(), 12);
    assert_eq!(output, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(
        encode_setup_complete_ex(&mut output[..11]),
        Err(HtcError::OutputTooSmall { .. })
    ));
}

#[test]
fn plain_control_frame_roundtrips_and_checks_header_length_and_reserved_fields() {
    let payload = [1, 2, 3, 4];
    let mut encoded = [0u8; HTC_FRAME_HEADER_SIZE + 4];
    assert_eq!(
        encode_control_frame(&payload, &mut encoded).unwrap(),
        encoded.len()
    );
    assert_eq!(&encoded[..8], &[0, 0, 4, 0, 0, 0, 0, 0]);
    let parsed = parse_plain_frame(&encoded).unwrap();
    assert_eq!(parsed.endpoint_id, 0);
    assert_eq!(parsed.flags, 0);
    assert_eq!(parsed.payload, &payload);

    assert!(matches!(
        parse_plain_frame(&encoded[..7]),
        Err(HtcError::Truncated { .. })
    ));
    let mut wrong_len = encoded;
    wrong_len[2] = 5;
    assert!(matches!(
        parse_plain_frame(&wrong_len),
        Err(HtcError::InvalidFramePayloadLength { .. })
    ));
    let mut flags = encoded;
    flags[1] = 1;
    assert_eq!(
        parse_plain_frame(&flags),
        Err(HtcError::InvalidFrameFlags { observed: 1 })
    );
    let mut reserved = encoded;
    reserved[4] = 1;
    assert_eq!(parse_plain_frame(&reserved), Err(HtcError::BadReservedBits));
    let mut invalid_endpoint = encoded;
    invalid_endpoint[0] = 9;
    assert_eq!(
        parse_plain_frame(&invalid_endpoint),
        Err(HtcError::InvalidFrameEndpoint { observed: 9 })
    );
}

#[test]
fn wmi_command_frame_contains_wmi_id_and_htc_header_and_charges_full_wire_length() {
    let body = [0xaa, 0xbb];
    let mut output = [0xa5; 20];
    let plan = encode_wmi_command_frame(2, 8, 8, 5, 1, &body, &mut output).unwrap();
    assert_eq!(plan.frame_len, 14);
    assert_eq!(plan.wmi_payload_len, 6);
    assert_eq!(plan.credits_required, 2);
    assert_eq!(plan.credits_remaining, 3);
    assert!(!plan.requested_credit_update);
    assert_eq!(
        &output[..14],
        &[2, 0, 6, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0xaa, 0xbb]
    );
    assert_eq!(output[14], 0xa5);
}

#[test]
fn wmi_frame_requests_credit_update_at_source_threshold() {
    let body = [0xaa, 0xbb];
    let mut output = [0u8; 14];
    let plan = encode_wmi_command_frame(2, 8, 8, 3, 1, &body, &mut output).unwrap();
    assert_eq!(plan.credits_remaining, 1);
    assert!(plan.requested_credit_update);
    assert_eq!(output[1], 1);
}

#[test]
fn wmi_receive_frame_parser_checks_endpoint_flags_lengths_and_command_header() {
    let mut frame = [0xa5; 16];
    let plan = encode_wmi_command_frame(2, 8, 8, 5, 0x1234, &[0xaa, 0xbb], &mut frame).unwrap();
    let tx = parse_wmi_tx_frame(&frame[..plan.frame_len], 2, 8).unwrap();
    assert!(!tx.credit_update_requested);
    let mut rx_frame = frame;
    rx_frame[1] = 0;
    let parsed = parse_wmi_frame(&rx_frame[..plan.frame_len], 2, 8).unwrap();
    assert_eq!(parsed.endpoint_id, 2);
    assert_eq!(parsed.command_id, 0x1234);
    assert_eq!(parsed.tlv_body, &[0xaa, 0xbb]);
    assert_eq!(parsed.credit_reports().count(), 0);

    let mut credit_update = frame;
    credit_update[1] = 1;
    assert!(
        parse_wmi_tx_frame(&credit_update[..plan.frame_len], 2, 8)
            .unwrap()
            .credit_update_requested
    );
    assert_eq!(
        parse_wmi_frame(&credit_update[..plan.frame_len], 2, 8),
        Err(HtcError::InvalidFrameFlags { observed: 1 })
    );

    assert_eq!(
        parse_wmi_frame(&rx_frame[..plan.frame_len], 3, 8),
        Err(HtcError::InvalidFrameEndpoint { observed: 2 })
    );
    assert_eq!(
        parse_wmi_frame(&rx_frame[..plan.frame_len], 2, 5),
        Err(HtcError::MessageTooLarge {
            limit: 5,
            observed: 6
        })
    );
    let mut unsupported_flags = rx_frame;
    unsupported_flags[1] = 4;
    assert_eq!(
        parse_wmi_frame(&unsupported_flags[..plan.frame_len], 2, 8),
        Err(HtcError::InvalidFrameFlags { observed: 4 })
    );
    let mut invalid_command = rx_frame;
    invalid_command[11] = 1;
    assert_eq!(
        parse_wmi_frame(&invalid_command[..plan.frame_len], 2, 8),
        Err(HtcError::InvalidWmiCommandId {
            observed: 0x0100_1234
        })
    );

    let empty_wmi_body = [2, 0, 4, 0, 0, 0, 0, 0, 1, 0, 0, 0];
    assert_eq!(
        parse_wmi_frame(&empty_wmi_body, 2, 8),
        Err(HtcError::EmptyWmiBody)
    );
    for end in 0..HTC_FRAME_HEADER_SIZE {
        assert!(parse_wmi_frame(&rx_frame[..end], 2, 8).is_err());
    }
}

#[test]
fn receive_trailer_returns_credits_only_for_the_bound_endpoint() {
    let mut frame = [0u8; 22];
    frame[0] = 2;
    frame[1] = HTC_FLAG_RECV_TRAILER;
    frame[2..4].copy_from_slice(&14u16.to_le_bytes());
    frame[4] = 8;
    frame[8..12].copy_from_slice(&0x1234u32.to_le_bytes());
    frame[12..14].copy_from_slice(&[0xaa, 0xbb]);
    frame[14..22].copy_from_slice(&[HTC_RECORD_CREDITS, 4, 0, 0, 2, 3, 0, 0]);

    let parsed = parse_wmi_frame(&frame, 2, 8).unwrap();
    assert_eq!(parsed.command_id, 0x1234);
    assert_eq!(parsed.tlv_body, &[0xaa, 0xbb]);
    assert_eq!(parsed.credit_returns().for_endpoint(2), 3);
    let reports = parsed.credit_reports().collect::<Vec<_>>();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].endpoint_id, 2);
    assert_eq!(reports[0].credits, 3);

    frame[4] = 7;
    assert!(matches!(
        parse_wmi_frame(&frame, 2, 8),
        Err(HtcError::InvalidTrailerRecord)
            | Err(HtcError::InvalidCreditReport)
            | Err(HtcError::BadReservedBits)
            | Err(HtcError::InvalidTrailerLength { .. })
    ));
    assert_eq!(HTC_CREDIT_REPORT_SIZE, 4);
}

#[test]
fn wmi_credit_ledger_tracks_publication_refunds_and_validated_reports() {
    let mut startup = HtcStartup::new();
    startup.accept_ready(&READY).unwrap();
    let mut message = [0u8; HTC_CONTROL_MESSAGE_MAX];
    startup.stage_wmi_connect(&mut message).unwrap();
    startup.control_tx_submitted().unwrap();
    startup.accept_connect_response(&CONNECT_RESPONSE).unwrap();
    startup.stage_setup_complete(&mut message).unwrap();
    startup.control_tx_submitted().unwrap();
    let mut ledger = startup.wmi_credit_ledger(9).unwrap();
    assert_eq!(ledger.endpoint_id(), 2);
    assert_eq!(ledger.available(), 4);
    assert_eq!(ledger.credit_size(), 64);

    let mut outgoing = [0u8; 32];
    let plan = encode_wmi_command_frame(2, 4088, 64, ledger.available(), 1, &[0xaa], &mut outgoing)
        .unwrap();
    ledger.reserve(9, plan).unwrap();
    assert_eq!(ledger.available(), 3);
    assert_eq!(ledger.in_flight(), 1);
    ledger.cancel_unpublished(9, plan.credits_required).unwrap();
    assert_eq!(ledger.available(), 4);
    assert_eq!(ledger.in_flight(), 0);

    ledger.reserve(9, plan).unwrap();
    let mut incoming = [0u8; 22];
    incoming[0] = 2;
    incoming[1] = HTC_FLAG_RECV_TRAILER;
    incoming[2..4].copy_from_slice(&14u16.to_le_bytes());
    incoming[4] = 8;
    incoming[8..12].copy_from_slice(&0x1234u32.to_le_bytes());
    incoming[12..14].copy_from_slice(&[0xaa, 0xbb]);
    incoming[14..22].copy_from_slice(&[HTC_RECORD_CREDITS, 4, 0, 0, 2, 1, 0, 0]);
    let parsed = parse_wmi_frame(&incoming, 2, 8).unwrap();
    assert_eq!(parsed.credit_returns().for_endpoint(2), 1);
    assert_eq!(ledger.apply_reports(9, parsed.credit_reports()).unwrap(), 1);
    assert_eq!(ledger.available(), 4);
    assert_eq!(ledger.in_flight(), 0);
}

#[test]
fn wmi_frame_rejects_invalid_lengths_and_credit_before_writing() {
    let body = [0xaa, 0xbb];
    let mut output = [0x5a; 14];
    assert_eq!(
        encode_wmi_command_frame(2, 8, 8, 1, 1, &body, &mut output),
        Err(HtcError::InsufficientCredits {
            required: 2,
            available: 1
        })
    );
    assert_eq!(output, [0x5a; 14]);

    assert!(matches!(
        encode_wmi_command_frame(0, 8, 8, 3, 1, &body, &mut output),
        Err(HtcError::InvalidEndpoint { observed: 0 })
    ));
    assert_eq!(
        encode_wmi_command_frame(2, 8, 0, 3, 1, &body, &mut output),
        Err(HtcError::ZeroCreditSize)
    );
    assert_eq!(
        encode_wmi_command_frame(2, 8, 8, 3, 0x0100_0000, &body, &mut output),
        Err(HtcError::InvalidWmiCommandId {
            observed: 0x0100_0000
        })
    );
    assert_eq!(
        encode_wmi_command_frame(2, 8, 8, 3, 1, &[], &mut output),
        Err(HtcError::EmptyWmiBody)
    );
    assert!(matches!(
        encode_wmi_command_frame(2, 5, 8, 3, 1, &body, &mut output),
        Err(HtcError::MessageTooLarge { .. })
    ));
    assert!(matches!(
        encode_wmi_command_frame(2, 8, 8, 3, 1, &body, &mut output[..13]),
        Err(HtcError::OutputTooSmall { .. })
    ));
    assert!(matches!(
        encode_wmi_command_frame(2, 4096, 8, u16::MAX, 1, &body, &mut output),
        Err(HtcError::InvalidMaxMessageSize { .. })
    ));
}

#[test]
fn htc_startup_enforces_ready_connect_response_setup_order_and_failure_rollback() {
    let mut startup = HtcStartup::new();
    assert_eq!(startup.phase(), HtcPhase::AwaitReady);
    let mut control = [0u8; HTC_CONTROL_MESSAGE_MAX];
    assert!(matches!(
        startup.stage_wmi_connect(&mut control),
        Err(HtcError::InvalidState { .. })
    ));

    let mut ready_frame = [0u8; HTC_FRAME_HEADER_SIZE + 12];
    ready_frame[..8].copy_from_slice(&[0, 0, 12, 0, 0, 0, 0, 0]);
    ready_frame[8..].copy_from_slice(&READY);
    startup.accept_ready_frame(&ready_frame).unwrap();
    assert_eq!(startup.phase(), HtcPhase::Ready);
    assert_eq!(startup.stage_wmi_connect(&mut control).unwrap(), 8);
    assert_eq!(&control[..8], &[2, 0, 0, 1, 0, 4, 0, 0]);
    assert_eq!(startup.phase(), HtcPhase::ConnectStaged);
    assert!(matches!(
        startup.accept_connect_response(&CONNECT_RESPONSE),
        Err(HtcError::InvalidState { .. })
    ));
    assert_eq!(startup.control_tx_failed().unwrap(), HtcPhase::Ready);

    startup.stage_wmi_connect(&mut control).unwrap();
    assert_eq!(
        startup.control_tx_submitted().unwrap(),
        HtcPhase::AwaitConnectResponse
    );
    startup.accept_connect_response(&CONNECT_RESPONSE).unwrap();
    assert_eq!(startup.phase(), HtcPhase::ServiceConnected);
    assert_eq!(startup.stage_setup_complete(&mut control).unwrap(), 12);
    assert_eq!(&control[..12], &[5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        startup.control_tx_failed().unwrap(),
        HtcPhase::ServiceConnected
    );
    startup.stage_setup_complete(&mut control).unwrap();
    assert_eq!(startup.control_tx_submitted().unwrap(), HtcPhase::Started);
}

#[test]
fn startup_only_frames_wmi_after_setup_and_uses_negotiated_endpoint_limit() {
    let mut startup = HtcStartup::new();
    let mut frame = [0u8; 14];
    assert!(matches!(
        startup.encode_wmi_command_frame(8, 5, 1, &[0xaa, 0xbb], &mut frame),
        Err(HtcError::InvalidState { .. })
    ));
    startup.accept_ready(&READY).unwrap();
    let mut control = [0u8; 12];
    startup.stage_wmi_connect(&mut control).unwrap();
    startup.control_tx_submitted().unwrap();
    startup.accept_connect_response(&CONNECT_RESPONSE).unwrap();
    startup.stage_setup_complete(&mut control).unwrap();
    startup.control_tx_submitted().unwrap();
    let plan = startup
        .encode_wmi_command_frame(
            8,
            u16::from(startup.initial_wmi_credit_allocation().unwrap()),
            1,
            &[0xaa, 0xbb],
            &mut frame,
        )
        .unwrap();
    assert_eq!(plan.credits_required, 2);
    assert_eq!(plan.credits_remaining, 2);
    assert!(plan.requested_credit_update);
    assert_eq!(frame[0], 2);
    assert_eq!(frame[1], 1);
    assert_eq!(frame[8..12], [1, 0, 0, 0]);
}

#[test]
fn qcacld_wmi_init_body_composes_into_one_bounded_htc_command_frame() {
    let service_ready = sunfish_service_ready_fixture();
    let wmi_startup = ServiceReadyView::parse(&service_ready)
        .unwrap()
        .require_sunfish_wmi_startup()
        .unwrap();
    let mut resource_config = [0u8; 276];
    resource_config[..4].copy_from_slice(&((75u32 << 16) | 272).to_le_bytes());
    let mut wmi_body = [0u8; 312];
    let body_len = wmi_startup
        .encode_init_body(&resource_config, None, &mut wmi_body)
        .unwrap();
    assert_eq!(body_len, 312);

    let mut htc = HtcStartup::new();
    htc.accept_ready(&READY).unwrap();
    let mut control = [0u8; 12];
    htc.stage_wmi_connect(&mut control).unwrap();
    htc.control_tx_submitted().unwrap();
    let response = connect_response_with(2, 512);
    htc.accept_connect_response(&response).unwrap();
    htc.stage_setup_complete(&mut control).unwrap();
    htc.control_tx_submitted().unwrap();

    let mut frame = [0u8; HTC_FRAME_HEADER_SIZE + 4 + 312];
    let plan = htc
        .encode_wmi_command_frame(
            256,
            u16::from(htc.initial_wmi_credit_allocation().unwrap()),
            1,
            &wmi_body[..body_len],
            &mut frame,
        )
        .unwrap();
    assert_eq!(plan.frame_len, 324);
    assert_eq!(plan.wmi_payload_len, 316);
    assert_eq!(plan.credits_required, 2);
    assert_eq!(plan.credits_remaining, 2);
    assert!(plan.requested_credit_update);
    assert_eq!(&frame[..8], &[2, 1, 0x3c, 1, 0, 0, 0, 0]);
    assert_eq!(&frame[8..12], &[1, 0, 0, 0]);
    assert_eq!(&frame[12..], &wmi_body[..body_len]);
}

#[test]
fn control_response_frame_must_use_endpoint_zero() {
    let mut startup = HtcStartup::new();
    startup.accept_ready(&READY).unwrap();
    let mut control = [0u8; 12];
    startup.stage_wmi_connect(&mut control).unwrap();
    startup.control_tx_submitted().unwrap();

    let mut frame = [0u8; HTC_FRAME_HEADER_SIZE + 12];
    frame[..8].copy_from_slice(&[0, 0, 12, 0, 0, 0, 0, 0]);
    frame[8..].copy_from_slice(&CONNECT_RESPONSE);
    assert_eq!(
        startup
            .accept_connect_response_frame(&frame)
            .unwrap()
            .endpoint_id,
        2
    );

    let mut wrong_endpoint = frame;
    wrong_endpoint[0] = 1;
    let mut second_startup = HtcStartup::new();
    second_startup.accept_ready(&READY).unwrap();
    second_startup.stage_wmi_connect(&mut control).unwrap();
    second_startup.control_tx_submitted().unwrap();
    assert_eq!(
        second_startup.accept_connect_response_frame(&wrong_endpoint),
        Err(HtcError::InvalidFrameEndpoint { observed: 1 })
    );
}
