#![allow(dead_code)]

#[path = "../rust/src/ath10k_htt.rs"]
mod ath10k_htt;
#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_htt_event.rs"]
mod sunfish_qcacld_htt_event;
#[path = "../rust/src/sunfish_qcacld_rx_ind.rs"]
mod sunfish_qcacld_rx_ind;

use ath10k_htt::{HttSecurityIndication, HttSecurityIndicationError};
use qcom_ce_v3::RxCompletion;
use qcom_htc::HtcReady;
use sunfish_qcacld_htc::{HTT_DATA_SERVICE_ID, parse_connect_response};
use sunfish_qcacld_htt_event::{
    ConsumeError, DecodeError, Event, consume_and_dispatch, decode_data_frame,
};

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

fn completion(nbytes: usize) -> RxCompletion {
    RxCompletion {
        address: 0x9000,
        nbytes: nbytes as u16,
        transfer_id: 7,
        byte_swapped: false,
    }
}

fn security_frame() -> [u8; 36] {
    let mut bytes = [0xa5; 36];
    bytes[0] = 5;
    bytes[1] = 0;
    bytes[2..4].copy_from_slice(&28u16.to_le_bytes());
    bytes[5] = 0x71;
    bytes[6] = 0;
    bytes[7] = 0;
    bytes[8] = 0x0b;
    bytes[9] = 6 | 0x80; // CCMP pairwise
    bytes[10..12].copy_from_slice(&0x1234u16.to_le_bytes());
    bytes
}

fn rx_indication_frame() -> [u8; 56] {
    let mut bytes = [0; 56];
    bytes[0] = 5;
    bytes[2..4].copy_from_slice(&48u16.to_le_bytes());
    bytes[5] = 0x42;
    bytes[8..12].copy_from_slice(&(1u32 | (2 << 8) | (0x1234 << 16)).to_le_bytes());
    bytes
}

#[test]
fn dispatches_sec_ind_from_the_dynamic_htt_endpoint_without_copying_key_material() {
    assert_eq!(HTT_DATA_SERVICE_ID, 0x0300);
    let packet = security_frame();
    let decoded = decode_data_frame(endpoint(), &packet).unwrap();

    assert_eq!(decoded.sequence, 0x71);
    assert_eq!(decoded.credit_returns.for_endpoint(5), 0);
    assert_eq!(
        decoded.event,
        Event::SecurityIndication(HttSecurityIndication {
            peer_id: 0x1234,
            security_type: 6,
            is_unicast: true,
        })
    );
}

#[test]
fn dispatches_rx_ind_on_the_same_endpoint() {
    let packet = rx_indication_frame();
    let decoded = decode_data_frame(endpoint(), &packet).unwrap();
    let Event::RxIndication(indication) = decoded.event else {
        panic!("RX_IND was dispatched as another event");
    };

    assert_eq!(decoded.sequence, 0x42);
    assert_eq!(indication.peer_id(), 0x1234);
    assert_eq!(indication.extended_tid(), 2);
}

#[test]
fn sec_ind_rejects_truncation_and_dispatch_rejects_unknown_messages() {
    let mut truncated = security_frame();
    truncated[2..4].copy_from_slice(&27u16.to_le_bytes());
    assert_eq!(
        decode_data_frame(endpoint(), &truncated[..35]),
        Err(DecodeError::SecurityIndication(
            HttSecurityIndicationError::Truncated {
                expected: 28,
                observed: 27,
            }
        ))
    );

    let mut unknown = security_frame();
    unknown[8] = 0x7f;
    assert_eq!(
        decode_data_frame(endpoint(), &unknown),
        Err(DecodeError::UnsupportedMessageType(0x7f))
    );
}

#[test]
fn ce1_completion_bounds_dispatch_and_preserves_transport_errors() {
    let packet = security_frame();
    let mut output = [0xee; 48];
    let completed = consume_and_dispatch(endpoint(), &mut output, |destination| {
        destination[..packet.len()].copy_from_slice(&packet);
        Ok::<_, ()>(completion(packet.len()))
    })
    .unwrap();
    assert_eq!(completed.completion, completion(36));
    assert!(matches!(
        completed.decoded.event,
        Event::SecurityIndication(HttSecurityIndication {
            peer_id: 0x1234,
            security_type: 6,
            is_unicast: true,
        })
    ));

    assert_eq!(
        consume_and_dispatch(endpoint(), &mut output, |_| { Ok::<_, ()>(completion(0)) }),
        Err(ConsumeError::EmptyCompletion)
    );
    assert_eq!(
        consume_and_dispatch(endpoint(), &mut output, |_| { Ok::<_, ()>(completion(49)) }),
        Err(ConsumeError::CompletionOutOfRange {
            frame_bytes: 49,
            available: 48,
        })
    );
    assert_eq!(
        consume_and_dispatch(endpoint(), &mut output, |_| Err::<RxCompletion, _>("ce1")),
        Err(ConsumeError::Consume("ce1"))
    );
}
