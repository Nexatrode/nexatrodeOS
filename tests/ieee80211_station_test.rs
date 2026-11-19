#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/ieee80211_station.rs"]
mod ieee80211_station;

use ieee80211_station::{
    AssociationRequest, StationFrameError, StationIdentity, encode_association_request,
    encode_open_authentication_request, parse_association_response,
    parse_open_authentication_response,
};

const STATION: [u8; 6] = [0x02, 0x10, 0x20, 0x30, 0x40, 0x50];
const BSSID: [u8; 6] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];

fn identity() -> StationIdentity {
    StationIdentity {
        station: STATION,
        bssid: BSSID,
        sequence_number: 0x123,
    }
}

fn response_frame(frame_control: u16, addresses: [[u8; 6]; 3], body: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; 24 + body.len()];
    frame[..2].copy_from_slice(&frame_control.to_le_bytes());
    frame[4..10].copy_from_slice(&addresses[0]);
    frame[10..16].copy_from_slice(&addresses[1]);
    frame[16..22].copy_from_slice(&addresses[2]);
    frame[24..].copy_from_slice(body);
    frame
}

fn auth_response(body: &[u8]) -> Vec<u8> {
    response_frame(0x00b0, [STATION, BSSID, BSSID], body)
}

fn assoc_response(body: &[u8]) -> Vec<u8> {
    response_frame(0x0010, [STATION, BSSID, BSSID], body)
}

#[test]
fn open_auth_request_has_exact_address_sequence_and_transaction_fields() {
    let mut output = [0xa5; 36];
    let encoded = encode_open_authentication_request(identity(), &mut output).unwrap();
    assert_eq!(encoded.frame_bytes, 30);
    assert_eq!(&output[..2], &[0xb0, 0x00]);
    assert_eq!(&output[4..10], &BSSID);
    assert_eq!(&output[10..16], &STATION);
    assert_eq!(&output[16..22], &BSSID);
    assert_eq!(&output[22..24], &[0x30, 0x12]);
    assert_eq!(&output[24..30], &[0, 0, 1, 0, 0, 0]);
    assert!(output[30..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn open_auth_response_requires_selected_ap_open_system_transaction_two() {
    assert_eq!(
        parse_open_authentication_response(&auth_response(&[0, 0, 2, 0, 0, 0]), identity()),
        Ok(ieee80211_station::AuthenticationResponse { status_code: 0 })
    );
    assert_eq!(
        parse_open_authentication_response(&auth_response(&[0, 0, 2, 0, 0x0b, 0]), identity()),
        Ok(ieee80211_station::AuthenticationResponse { status_code: 11 })
    );
    assert_eq!(
        parse_open_authentication_response(&auth_response(&[1, 0, 2, 0, 0, 0]), identity()),
        Err(StationFrameError::UnsupportedAuthenticationAlgorithm)
    );
    assert_eq!(
        parse_open_authentication_response(&auth_response(&[0, 0, 1, 0, 0, 0]), identity()),
        Err(StationFrameError::WrongAuthenticationTransaction)
    );
}

#[test]
fn auth_response_rejects_wrong_peer_subtype_protection_and_truncation() {
    let wrong_peer = response_frame(0x00b0, [[0x02; 6], BSSID, BSSID], &[0, 0, 2, 0, 0, 0]);
    assert_eq!(
        parse_open_authentication_response(&wrong_peer, identity()),
        Err(StationFrameError::WrongPeer)
    );
    assert_eq!(
        parse_open_authentication_response(&assoc_response(&[0; 6]), identity()),
        Err(StationFrameError::WrongFrameSubtype)
    );
    let protected = response_frame(0x40b0, [STATION, BSSID, BSSID], &[0, 0, 2, 0, 0, 0]);
    assert_eq!(
        parse_open_authentication_response(&protected, identity()),
        Err(StationFrameError::ProtectedManagementFrame)
    );
    assert_eq!(
        parse_open_authentication_response(&auth_response(&[0, 0, 2, 0, 0]), identity()),
        Err(StationFrameError::TruncatedBody)
    );
}

#[test]
fn association_request_serializes_core_ies_before_caller_ordered_ies() {
    let rsn = [1, 0, 0, 0x0f, 0xac, 4];
    let extra = [45, 1, 0xaa, 221, 1, 0xbb];
    let request = AssociationRequest {
        identity: identity(),
        capability_info: 0x0011, // ESS and privacy for the supplied RSN IE.
        listen_interval: 10,
        ssid: b"home",
        supported_rates: &[0x82, 0x84],
        extended_supported_rates: &[0x0c],
        rsn: Some(&rsn),
        additional_ies: &extra,
    };
    let mut output = [0xa5; 64];
    let encoded = encode_association_request(request, &mut output).unwrap();
    assert_eq!(encoded.frame_bytes, 55);
    assert_eq!(&output[..2], &[0, 0]); // Association Request.
    assert_eq!(&output[4..10], &BSSID);
    assert_eq!(&output[10..16], &STATION);
    assert_eq!(&output[22..24], &[0x30, 0x12]);
    assert_eq!(&output[24..28], &[0x11, 0, 10, 0]);
    assert_eq!(
        &output[28..55],
        &[
            0, 4, b'h', b'o', b'm', b'e', 1, 2, 0x82, 0x84, 50, 1, 0x0c, 48, 6, 1, 0, 0, 0x0f,
            0xac, 4, 45, 1, 0xaa, 221, 1, 0xbb,
        ]
    );
    assert!(output[55..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn open_association_omits_rsn_and_protected_data_is_not_implied() {
    let request = AssociationRequest {
        identity: identity(),
        capability_info: 1,
        listen_interval: 1,
        ssid: b"guest",
        supported_rates: &[0x82],
        extended_supported_rates: &[],
        rsn: None,
        additional_ies: &[],
    };
    let mut output = [0; 40];
    let encoded = encode_association_request(request, &mut output).unwrap();
    assert_eq!(encoded.frame_bytes, 38);
    assert_eq!(
        &output[28..38],
        &[0, 5, b'g', b'u', b'e', b's', b't', 1, 1, 0x82]
    );
}

#[test]
fn association_response_returns_only_successful_bounded_aid() {
    assert_eq!(
        parse_association_response(&assoc_response(&[0x11, 0, 0, 0, 0x23, 0xc1]), identity()),
        Ok(ieee80211_station::AssociationResponse {
            capability_info: 0x0011,
            status_code: 0,
            association_id: Some(0x0123),
        })
    );
    assert_eq!(
        parse_association_response(&assoc_response(&[0x11, 0, 10, 0, 0, 0]), identity()),
        Ok(ieee80211_station::AssociationResponse {
            capability_info: 0x0011,
            status_code: 10,
            association_id: None,
        })
    );
    assert_eq!(
        parse_association_response(&assoc_response(&[0x11, 0, 0, 0, 0, 0]), identity()),
        Err(StationFrameError::InvalidAssociationId)
    );
}

#[test]
fn invalid_requests_and_short_outputs_fail_without_partial_writes() {
    let valid = AssociationRequest {
        identity: identity(),
        capability_info: 1,
        listen_interval: 1,
        ssid: b"guest",
        supported_rates: &[0x82],
        extended_supported_rates: &[],
        rsn: None,
        additional_ies: &[],
    };
    let mut output = [0xa5; 64];
    assert_eq!(
        encode_open_authentication_request(
            StationIdentity {
                sequence_number: 0x1000,
                ..identity()
            },
            &mut output,
        ),
        Err(StationFrameError::SequenceNumberOutOfRange)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_association_request(
            AssociationRequest {
                capability_info: 0,
                ..valid
            },
            &mut output,
        ),
        Err(StationFrameError::InvalidCapability)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_association_request(
            AssociationRequest {
                capability_info: 0x0011,
                ..valid
            },
            &mut output,
        ),
        Err(StationFrameError::InvalidCapability)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_association_request(
            AssociationRequest {
                identity: StationIdentity {
                    station: [0xff; 6],
                    ..identity()
                },
                ..valid
            },
            &mut output,
        ),
        Err(StationFrameError::InvalidStationAddress)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_association_request(
            AssociationRequest {
                additional_ies: &[0, 0],
                ..valid
            },
            &mut output,
        ),
        Err(StationFrameError::DuplicateCoreElement(0))
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_association_request(
            AssociationRequest {
                additional_ies: &[45, 2, 0xaa],
                ..valid
            },
            &mut output,
        ),
        Err(StationFrameError::InvalidAdditionalElements)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    let mut too_small = [0xa5; 35];
    assert_eq!(
        encode_association_request(valid, &mut too_small),
        Err(StationFrameError::OutputTooSmall)
    );
    assert!(too_small.iter().all(|byte| *byte == 0xa5));
}
