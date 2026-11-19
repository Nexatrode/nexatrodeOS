#![allow(dead_code)]

#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_rx_ind.rs"]
mod sunfish_qcacld_rx_ind;
#[path = "../rust/src/sunfish_qcacld_rx_transport.rs"]
mod sunfish_qcacld_rx_transport;

use qcom_ce_v3::RxCompletion;
use qcom_htc::HtcReady;
use sunfish_qcacld_htc::{HTT_DATA_SERVICE_ID, parse_connect_response};
use sunfish_qcacld_rx_ind::{FrameError, HttRxIndication};
use sunfish_qcacld_rx_transport::{Error, consume_and_decode};

fn endpoint() -> sunfish_qcacld_htc::HttEndpoint {
    let response = [
        3, 0, 0, 3, // connect response, HTT service
        0, 5, 0, 2, // success, endpoint 5, 512-byte max message
        0, 0, 0, 0,
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

fn frame() -> [u8; 56] {
    let mut bytes = [0; 56];
    bytes[0] = 5;
    bytes[2..4].copy_from_slice(&48u16.to_le_bytes());
    bytes[5] = 0xa4;
    bytes[8..12].copy_from_slice(&(1u32 | (2 << 8) | (0x1234 << 16)).to_le_bytes());
    bytes
}

fn completion(nbytes: u16) -> RxCompletion {
    RxCompletion {
        address: 0x9000,
        nbytes,
        transfer_id: 7,
        byte_swapped: false,
    }
}

#[test]
fn ce1_completion_is_bounded_to_its_reported_length_before_htc_htt_decode() {
    assert_eq!(HTT_DATA_SERVICE_ID, 0x0300);
    let packet = frame();
    let mut output = [0xee; 80];
    let completed = consume_and_decode(endpoint(), &mut output, |destination| {
        destination[..packet.len()].copy_from_slice(&packet);
        Ok::<_, ()>(completion(packet.len() as u16))
    })
    .unwrap();

    assert_eq!(completed.completion, completion(56));
    assert_eq!(completed.sequence, 0xa4);
    assert_eq!(completed.credit_returns.for_endpoint(5), 0);
    assert_eq!(completed.indication.peer_id(), 0x1234);
    assert_eq!(completed.indication.extended_tid(), 2);
    assert_eq!(completed.indication.mpdu_range_count(), 0);
    assert_eq!(
        completed.indication,
        HttRxIndication::decode(&packet[8..]).unwrap()
    );
}

#[test]
fn invalid_completion_lengths_fail_before_parsing_scratch_bytes() {
    let mut output = [0xee; 56];
    assert_eq!(
        consume_and_decode(endpoint(), &mut output, |_| Ok::<_, ()>(completion(0))),
        Err(Error::EmptyCompletion)
    );
    assert_eq!(
        consume_and_decode(endpoint(), &mut output, |_| Ok::<_, ()>(completion(57))),
        Err(Error::CompletionOutOfRange {
            frame_bytes: 57,
            available: 56,
        })
    );
}

#[test]
fn consume_and_decode_propagates_transport_and_frame_errors() {
    let mut output = [0; 56];
    assert_eq!(
        consume_and_decode(endpoint(), &mut output, |_| Err::<RxCompletion, _>("ce1")),
        Err(Error::Consume("ce1"))
    );

    let mut packet = frame();
    packet[0] = 4;
    assert_eq!(
        consume_and_decode(endpoint(), &mut output, |destination| {
            destination.copy_from_slice(&packet);
            Ok::<_, ()>(completion(56))
        }),
        Err(Error::Frame(FrameError::Htc(
            qcom_htc::HtcError::InvalidFrameEndpoint { observed: 4 }
        )))
    );
}
