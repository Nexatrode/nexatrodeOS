#![allow(dead_code)]

#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;

use qcom_htc::{HtcError, HtcReady};
use sunfish_qcacld_htc::{
    HTC_CONNECT_DISABLE_CREDIT_FLOW_CTRL, HTT_DATA_SERVICE_ID, HttStartupError, HttStartupPhase,
    HttStartupSession, encode_connect_request, encode_data_frame, parse_connect_response,
};

fn ready() -> HtcReady {
    HtcReady {
        credit_count: 4,
        credit_size: 64,
        max_endpoints: 9,
        htc_version: 1,
        max_messages_per_bundle: 1,
        alternate_data_credit_size: 0,
    }
}

fn connect_response(service_id: u16, endpoint_id: u8, max_message_size: u16) -> [u8; 12] {
    let endpoint_and_size = (u32::from(endpoint_id) << 8) | (u32::from(max_message_size) << 16);
    [
        3,
        0,
        service_id as u8,
        (service_id >> 8) as u8,
        endpoint_and_size as u8,
        (endpoint_and_size >> 8) as u8,
        (endpoint_and_size >> 16) as u8,
        (endpoint_and_size >> 24) as u8,
        0,
        0,
        0,
        0,
    ]
}

fn control_response(service_id: u16, endpoint_id: u8, max_message_size: u16) -> [u8; 20] {
    let payload = connect_response(service_id, endpoint_id, max_message_size);
    let mut frame = [0u8; 20];
    frame[2..4].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    frame[8..].copy_from_slice(&payload);
    frame
}

#[test]
fn htt_connect_requests_dynamic_endpoint_without_credit_flow_control() {
    assert_eq!(HTT_DATA_SERVICE_ID, 0x0300);
    assert_eq!(HTC_CONNECT_DISABLE_CREDIT_FLOW_CTRL, 0x0008);
    let mut bytes = [0xa5; 12];
    assert_eq!(encode_connect_request(&mut bytes), Ok(8));
    assert_eq!(&bytes[..8], &[2, 0, 0, 3, 8, 0, 0, 0]);
    assert_eq!(bytes[8], 0xa5);
}

#[test]
fn target_response_binds_only_a_valid_dynamic_htt_endpoint() {
    let response = connect_response(HTT_DATA_SERVICE_ID, 5, 512);
    let endpoint = parse_connect_response(&response, ready()).unwrap();
    assert_eq!(endpoint.endpoint_id(), 5);
    assert_eq!(endpoint.max_message_size(), 512);

    let wrong_service = connect_response(0x0100, 5, 512);
    assert_eq!(
        parse_connect_response(&wrong_service, ready()),
        Err(HtcError::WrongServiceId {
            expected: HTT_DATA_SERVICE_ID,
            observed: 0x0100,
        })
    );

    let endpoint_zero = connect_response(HTT_DATA_SERVICE_ID, 0, 512);
    assert_eq!(
        parse_connect_response(&endpoint_zero, ready()),
        Err(HtcError::InvalidEndpoint { observed: 0 })
    );

    let oversized = connect_response(HTT_DATA_SERVICE_ID, 5, 4089);
    assert_eq!(
        parse_connect_response(&oversized, ready()),
        Err(HtcError::InvalidMaxMessageSize { observed: 4089 })
    );
}

#[test]
fn htt_payload_is_framed_with_dynamic_endpoint_and_sequence() {
    let endpoint =
        parse_connect_response(&connect_response(HTT_DATA_SERVICE_ID, 5, 512), ready()).unwrap();
    let payload = [0x33; 23];
    let mut output = [0xa5; 40];
    let plan = encode_data_frame(endpoint, 0xab, &payload, &mut output).unwrap();
    assert_eq!(plan.frame_len, 31);
    assert_eq!(plan.payload_len, 23);
    assert_eq!(plan.sequence, 0xab);
    assert_eq!(&output[..8], &[5, 0, 23, 0, 0, 0xab, 0, 0]);
    assert_eq!(&output[8..31], &payload);
    assert_eq!(output[31], 0xa5);
}

#[test]
fn frame_encoder_fails_closed_on_empty_large_or_short_payloads() {
    let endpoint =
        parse_connect_response(&connect_response(HTT_DATA_SERVICE_ID, 5, 4), ready()).unwrap();
    let mut output = [0x5a; 11];
    assert_eq!(
        encode_data_frame(endpoint, 0, &[], &mut output),
        Err(HtcError::Truncated {
            expected: 1,
            observed: 0,
        })
    );
    assert_eq!(output, [0x5a; 11]);

    let too_large = [0u8; 5];
    assert_eq!(
        encode_data_frame(endpoint, 0, &too_large, &mut output),
        Err(HtcError::MessageTooLarge {
            limit: 4,
            observed: 5,
        })
    );
    assert_eq!(output, [0x5a; 11]);

    let fits_endpoint = [0u8; 4];
    assert_eq!(
        encode_data_frame(endpoint, 0, &fits_endpoint, &mut output),
        Err(HtcError::OutputTooSmall {
            required: 12,
            available: 11,
        })
    );
    assert_eq!(output, [0x5a; 11]);
}

#[test]
fn htt_startup_submits_control_request_then_binds_target_endpoint() {
    let mut session = HttStartupSession::new(7, ready()).unwrap();
    assert_eq!(session.phase(), HttStartupPhase::AwaitRequest);
    assert_eq!(session.endpoint(), None);

    let mut frame = [0xa5; 32];
    assert_eq!(session.stage_connect_request(7, &mut frame), Ok(16));
    assert_eq!(
        &frame[..16],
        &[0, 0, 8, 0, 0, 0, 0, 0, 2, 0, 0, 3, 8, 0, 0, 0]
    );
    assert_eq!(session.phase(), HttStartupPhase::RequestStaged);
    assert_eq!(session.endpoint(), None);

    session.request_tx_submitted(7).unwrap();
    assert_eq!(session.phase(), HttStartupPhase::AwaitResponse);
    let response = control_response(HTT_DATA_SERVICE_ID, 5, 512);
    let endpoint = session.accept_connect_response_frame(7, &response).unwrap();
    assert_eq!(endpoint.endpoint_id(), 5);
    assert_eq!(endpoint.max_message_size(), 512);
    assert_eq!(session.endpoint(), Some(endpoint));
    assert_eq!(session.phase(), HttStartupPhase::Connected);
}

#[test]
fn htt_startup_rejects_stale_or_out_of_order_transactions() {
    let mut session = HttStartupSession::new(7, ready()).unwrap();
    let mut frame = [0xa5; 16];
    assert_eq!(
        session.stage_connect_request(8, &mut frame),
        Err(HttStartupError::WrongGeneration {
            expected: 7,
            observed: 8,
        })
    );
    assert_eq!(
        session.accept_connect_response_frame(7, &control_response(HTT_DATA_SERVICE_ID, 5, 512)),
        Err(HttStartupError::InvalidState {
            expected: HttStartupPhase::AwaitResponse,
            observed: HttStartupPhase::AwaitRequest,
        })
    );
    assert_eq!(session.phase(), HttStartupPhase::AwaitRequest);

    assert_eq!(session.stage_connect_request(7, &mut frame), Ok(16));
    session.request_tx_failed(7).unwrap();
    assert_eq!(session.phase(), HttStartupPhase::AwaitRequest);
    assert_eq!(session.endpoint(), None);
}

#[test]
fn uncertain_control_submission_is_quarantined_without_retry() {
    let mut session = HttStartupSession::new(7, ready()).unwrap();
    let mut frame = [0u8; 16];
    session.stage_connect_request(7, &mut frame).unwrap();
    session.request_tx_uncertain(7).unwrap();

    assert_eq!(session.phase(), HttStartupPhase::SubmissionUncertain);
    assert_eq!(
        session.staged_connect_request_frame(7),
        Err(HttStartupError::InvalidState {
            expected: HttStartupPhase::RequestStaged,
            observed: HttStartupPhase::SubmissionUncertain,
        })
    );
    assert_eq!(
        session.request_tx_failed(7),
        Err(HttStartupError::InvalidState {
            expected: HttStartupPhase::RequestStaged,
            observed: HttStartupPhase::SubmissionUncertain,
        })
    );
    assert_eq!(
        session.stage_connect_request(7, &mut frame),
        Err(HttStartupError::InvalidState {
            expected: HttStartupPhase::AwaitRequest,
            observed: HttStartupPhase::SubmissionUncertain,
        })
    );
}

#[test]
fn htt_startup_does_not_commit_invalid_response_or_short_request() {
    let mut session = HttStartupSession::new(7, ready()).unwrap();
    let mut too_short = [0xa5; 15];
    assert_eq!(
        session.stage_connect_request(7, &mut too_short),
        Err(HttStartupError::Htc(HtcError::OutputTooSmall {
            required: 16,
            available: 15,
        }))
    );
    assert_eq!(session.phase(), HttStartupPhase::AwaitRequest);
    assert_eq!(too_short, [0xa5; 15]);

    let mut frame = [0xa5; 16];
    session.stage_connect_request(7, &mut frame).unwrap();
    session.request_tx_submitted(7).unwrap();
    let wrong_service = control_response(0x0100, 5, 512);
    assert_eq!(
        session.accept_connect_response_frame(7, &wrong_service),
        Err(HttStartupError::Htc(HtcError::WrongServiceId {
            expected: HTT_DATA_SERVICE_ID,
            observed: 0x0100,
        }))
    );
    assert_eq!(session.phase(), HttStartupPhase::AwaitResponse);
    assert_eq!(session.endpoint(), None);

    let invalid_endpoint = control_response(HTT_DATA_SERVICE_ID, 0, 512);
    assert_eq!(
        session.accept_connect_response_frame(7, &invalid_endpoint),
        Err(HttStartupError::Htc(HtcError::InvalidEndpoint {
            observed: 0
        }))
    );
    assert_eq!(session.phase(), HttStartupPhase::AwaitResponse);
}

#[test]
fn htt_startup_binds_only_a_valid_generation_and_parsed_htc_ready() {
    assert_eq!(
        HttStartupSession::new(0, ready()),
        Err(HttStartupError::InvalidGeneration)
    );

    let mut ready_payload = [0u8; 12];
    ready_payload[..4].copy_from_slice(&0x0004_0001u32.to_le_bytes());
    ready_payload[4..8].copy_from_slice(&0x0009_0040u32.to_le_bytes());
    ready_payload[8..12].copy_from_slice(&0x0000_0101u32.to_le_bytes());
    let session = HttStartupSession::from_ready_payload(9, &ready_payload).unwrap();
    assert_eq!(session.generation(), 9);
    assert_eq!(session.phase(), HttStartupPhase::AwaitRequest);

    assert_eq!(
        HttStartupSession::from_ready_payload(9, &ready_payload[..11]),
        Err(HttStartupError::Htc(HtcError::Truncated {
            expected: 12,
            observed: 11,
        }))
    );

    ready_payload[8] = 2;
    assert_eq!(
        HttStartupSession::from_ready_payload(9, &ready_payload),
        Err(HttStartupError::Htc(HtcError::UnsupportedHtcVersion {
            observed: 2
        }))
    );
}
