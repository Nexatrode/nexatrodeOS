#![allow(dead_code)]

#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_rx_ind.rs"]
mod sunfish_qcacld_rx_ind;

use qcom_htc::{
    HTC_FLAG_RECV_1MORE_BLOCK, HTC_FLAG_RECV_TRAILER, HTC_RECORD_CREDITS, HtcCreditLedger,
    HtcError, HtcReady, encode_wmi_command_frame,
};
use sunfish_qcacld_htc::{HTT_DATA_SERVICE_ID, parse_connect_response, parse_data_frame};
use sunfish_qcacld_rx_ind::{
    Error as RxIndError, FrameError, HttRxIndication, SUNFISH_HTT_RX_IND_HEADER_BYTES,
    SequenceRange, decode_htc_data_frame,
};

fn endpoint() -> sunfish_qcacld_htc::HttEndpoint {
    let response = [
        3, 0, 0, 3, // connect response, HTT service
        0, 5, 0, 2, // success, endpoint 5, 512-byte max message
        0, 0, 0, 0, // no metadata or options
    ];
    parse_connect_response(
        &response,
        HtcReady {
            credit_count: 4,
            credit_size: 64,
            max_endpoints: 9,
            htc_version: 1,
            max_messages_per_bundle: 1,
            alternate_data_credit_size: 0,
        },
    )
    .unwrap()
}

fn put_word(bytes: &mut [u8], offset: usize, word: u32) {
    bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn rx_indication() -> Vec<u8> {
    // Fixed 48-byte header, five firmware descriptor bytes, three alignment
    // bytes, two MPDU ranges, and two trailing bytes accepted by QCACLD.
    let mut bytes = vec![0; 66];
    let first = 1 | (7 << 8) | (1 << 13) | (1 << 14) | (0x1234 << 16);
    put_word(&mut bytes, 0, first);
    put_word(
        &mut bytes,
        4,
        (2 << 24) | 1 | (2 << 6) | (3 << 12) | (4 << 18),
    );
    put_word(&mut bytes, 44, 5);
    bytes[48..53].copy_from_slice(b"desc!");
    bytes[53..56].copy_from_slice(&[0x55, 0x66, 0x77]);
    put_word(&mut bytes, 56, 2 | (1 << 8));
    put_word(&mut bytes, 60, 1 | (3 << 8));
    bytes[64..66].copy_from_slice(&[0xaa, 0xbb]);
    bytes
}

fn frame_for(payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; 8 + payload.len()];
    frame[0] = 5;
    frame[1] = 0;
    frame[2..4].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    frame[4] = 0x5a; // ControlBytes0 is opaque when CRC checking is disabled.
    frame[5] = 0xc3; // ControlBytes1 is the endpoint sequence number.
    frame[8..].copy_from_slice(payload);
    frame
}

#[test]
fn negotiated_htc_data_frame_decodes_into_bounded_rx_indication() {
    assert_eq!(HTT_DATA_SERVICE_ID, 0x0300);
    assert_eq!(SUNFISH_HTT_RX_IND_HEADER_BYTES, 48);
    let message = rx_indication();
    let frame = frame_for(&message);
    let (sequence, credit_returns, indication) = decode_htc_data_frame(endpoint(), &frame).unwrap();
    assert_eq!(sequence, 0xc3);
    assert_eq!(credit_returns.for_endpoint(5), 0);
    assert_eq!(indication.peer_id(), 0x1234);
    assert_eq!(indication.extended_tid(), 7);
    assert!(indication.flush_valid());
    assert!(indication.release_valid());
    assert_eq!(
        indication.flush_sequence_range(),
        SequenceRange { start: 1, end: 2 }
    );
    assert_eq!(
        indication.release_sequence_range(),
        SequenceRange { start: 3, end: 4 }
    );
    assert_eq!(indication.firmware_descriptor_bytes(), b"desc!");
    assert_eq!(indication.alignment_padding(), &[0x55, 0x66, 0x77]);
    assert_eq!(indication.mpdu_range_count(), 2);
    assert_eq!(
        indication.mpdu_ranges().collect::<Vec<_>>(),
        [
            sunfish_qcacld_rx_ind::MpduRange {
                mpdu_count: 2,
                status: 1,
            },
            sunfish_qcacld_rx_ind::MpduRange {
                mpdu_count: 1,
                status: 3,
            },
        ]
    );
    assert_eq!(
        indication
            .mpdus()
            .map(|mpdu| (
                mpdu.range_index,
                mpdu.index_in_range,
                mpdu.status.raw(),
                mpdu.status.is_ok(),
            ))
            .collect::<Vec<_>>(),
        [(0, 0, 1, true), (0, 1, 1, true), (1, 0, 3, false)]
    );
    assert_eq!(indication.trailing_bytes(), &[0xaa, 0xbb]);
}

#[test]
fn data_receive_trailer_preserves_sequence_and_reports_wmi_credits() {
    let message = rx_indication();
    let mut frame = frame_for(&message);
    let trailer = [HTC_RECORD_CREDITS, 4, 0, 0, 2, 1, 0, 0];
    let payload_len = message.len() + trailer.len();
    frame[1] = HTC_FLAG_RECV_TRAILER;
    frame[2..4].copy_from_slice(&(payload_len as u16).to_le_bytes());
    frame[4] = trailer.len() as u8;
    frame.extend_from_slice(&trailer);

    let (sequence, credit_returns, indication) = decode_htc_data_frame(endpoint(), &frame).unwrap();
    assert_eq!(sequence, 0xc3);
    assert_eq!(credit_returns.for_endpoint(2), 1);
    assert_eq!(credit_returns.for_endpoint(5), 0);
    assert_eq!(indication.peer_id(), 0x1234);

    let mut credits = HtcCreditLedger::new(7, 2, 4, 64, 512).unwrap();
    let mut outgoing = [0u8; 16];
    let plan = encode_wmi_command_frame(2, 512, 64, 4, 1, &[0xaa], &mut outgoing).unwrap();
    credits.reserve(7, plan).unwrap();
    assert_eq!(credits.available(), 3);
    assert_eq!(credits.apply_credit_returns(7, credit_returns).unwrap(), 1);
    assert_eq!(credits.available(), 4);
    assert_eq!(credits.in_flight(), 0);
}

#[test]
fn receive_bundle_flag_is_not_misread_as_a_credit_request() {
    let message = rx_indication();
    let mut frame = frame_for(&message);
    frame[1] = HTC_FLAG_RECV_1MORE_BLOCK; // RX bundling is unsupported.
    assert_eq!(
        decode_htc_data_frame(endpoint(), &frame),
        Err(FrameError::Htc(HtcError::InvalidFrameFlags {
            observed: HTC_FLAG_RECV_1MORE_BLOCK
        }))
    );
}

#[test]
fn data_frame_requires_the_negotiated_endpoint_and_supported_header_mode() {
    let payload = rx_indication();
    let mut frame = frame_for(&payload);
    frame[0] = 4;
    assert_eq!(
        parse_data_frame(endpoint(), &frame),
        Err(HtcError::InvalidFrameEndpoint { observed: 4 })
    );

    frame[0] = 5;
    frame[1] = HTC_FLAG_RECV_1MORE_BLOCK; // HTC receive bundling is unsupported.
    assert_eq!(
        parse_data_frame(endpoint(), &frame),
        Err(HtcError::InvalidFrameFlags {
            observed: HTC_FLAG_RECV_1MORE_BLOCK
        })
    );

    frame[1] = 0;
    frame[6] = 1;
    assert_eq!(
        parse_data_frame(endpoint(), &frame),
        Err(HtcError::BadReservedBits)
    );

    frame[6] = 0;
    frame[2..4].copy_from_slice(&513u16.to_le_bytes());
    assert_eq!(
        parse_data_frame(endpoint(), &frame),
        Err(HtcError::MessageTooLarge {
            limit: 512,
            observed: 513,
        })
    );

    let mut message = rx_indication();
    message[0] = 2;
    let frame = frame_for(&message);
    assert_eq!(
        decode_htc_data_frame(endpoint(), &frame),
        Err(FrameError::Htt(RxIndError::WrongMessageType {
            observed: 2
        }))
    );
}

#[test]
fn indication_rejects_invalid_header_and_unbounded_variable_spans() {
    let mut bytes = rx_indication();
    assert_eq!(
        HttRxIndication::decode(&bytes[..47]),
        Err(RxIndError::TruncatedHeader)
    );

    put_word(&mut bytes, 0, 2);
    assert_eq!(
        HttRxIndication::decode(&bytes),
        Err(RxIndError::WrongMessageType { observed: 2 })
    );

    put_word(&mut bytes, 0, 1 | (1 << 15));
    assert_eq!(
        HttRxIndication::decode(&bytes),
        Err(RxIndError::ReservedHeaderBits)
    );

    put_word(&mut bytes, 0, 1 | (19 << 8));
    assert_eq!(
        HttRxIndication::decode(&bytes),
        Err(RxIndError::InvalidExtendedTid { observed: 19 })
    );

    put_word(&mut bytes, 0, 1);
    put_word(&mut bytes, 44, 1 << 16);
    assert_eq!(
        HttRxIndication::decode(&bytes),
        Err(RxIndError::ReservedSuffixBits)
    );

    put_word(&mut bytes, 44, 0xffff);
    assert_eq!(
        HttRxIndication::decode(&bytes),
        Err(RxIndError::DescriptorSpanOutOfRange)
    );

    put_word(&mut bytes, 44, 0);
    put_word(&mut bytes, 4, 0xff00_0000);
    assert_eq!(
        HttRxIndication::decode(&bytes),
        Err(RxIndError::RangeTableOutOfRange)
    );
}

#[test]
fn zero_range_release_indication_is_valid_and_trailing_data_is_empty() {
    let mut bytes = vec![0; SUNFISH_HTT_RX_IND_HEADER_BYTES];
    put_word(&mut bytes, 0, 1 | (1 << 14) | (0x40 << 16));
    let indication = HttRxIndication::decode(&bytes).unwrap();
    assert_eq!(indication.peer_id(), 0x40);
    assert!(!indication.flush_valid());
    assert!(indication.release_valid());
    assert_eq!(indication.mpdu_range_count(), 0);
    assert_eq!(indication.mpdu_ranges().len(), 0);
    assert_eq!(indication.mpdus().count(), 0);
    assert!(indication.trailing_bytes().is_empty());
}

#[test]
fn mpdu_iterator_skips_empty_ranges_and_preserves_unknown_status() {
    let mut bytes = vec![0; SUNFISH_HTT_RX_IND_HEADER_BYTES + 4 * 4];
    put_word(&mut bytes, 0, 1 | (2 << 8));
    put_word(&mut bytes, 4, 4 << 24);
    put_word(&mut bytes, 48, 0 | (1 << 8)); // Empty range, ignored.
    put_word(&mut bytes, 52, 2 | (1 << 8)); // Two successful MPDUs.
    put_word(&mut bytes, 56, 1 | (0xfe << 8)); // Unknown status is retained.
    put_word(&mut bytes, 60, 1); // Status zero is UNKNOWN, not success.

    let indication = HttRxIndication::decode(&bytes).unwrap();
    assert_eq!(
        indication
            .mpdus()
            .map(|mpdu| (
                mpdu.range_index,
                mpdu.index_in_range,
                mpdu.status.raw(),
                mpdu.status.is_ok(),
            ))
            .collect::<Vec<_>>(),
        [
            (1, 0, 1, true),
            (1, 1, 1, true),
            (2, 0, 0xfe, false),
            (3, 0, 0, false),
        ]
    );
}
