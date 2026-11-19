#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/ieee80211_probe.rs"]
mod ieee80211_probe;

use ath10k_nwifi_rx::{FrameKind, inspect_frame};
use ieee80211_probe::{ProbeRequest, ProbeRequestError, encode_probe_request};

const STATION: [u8; 6] = [0x02, 0x10, 0x20, 0x30, 0x40, 0x50];

fn request<'a>(
    ssid: &'a [u8],
    supported_rates: &'a [u8],
    extended_supported_rates: &'a [u8],
    sequence_number: u16,
) -> ProbeRequest<'a> {
    ProbeRequest {
        transmitter: STATION,
        ssid,
        supported_rates,
        extended_supported_rates,
        sequence_number,
    }
}

#[test]
fn encodes_wildcard_probe_request_and_borrowed_rate_elements() {
    let rates = [0x82, 0x84, 0x8b, 0x96];
    let extended_rates = [0x0c, 0x12];
    let mut output = [0xa5; 40];
    let layout =
        encode_probe_request(request(&[], &rates, &extended_rates, 0x0123), &mut output).unwrap();

    assert_eq!(layout.frame_bytes, 36);
    assert_eq!(layout.information_element_bytes, 12);
    assert_eq!(&output[..2], &[0x40, 0x00]); // Management Probe Request.
    assert_eq!(&output[2..4], &[0, 0]);
    assert_eq!(&output[4..10], &[0xff; 6]);
    assert_eq!(&output[10..16], &STATION);
    assert_eq!(&output[16..22], &[0xff; 6]);
    assert_eq!(&output[22..24], &[0x30, 0x12]);
    assert_eq!(
        &output[24..36],
        &[0, 0, 1, 4, 0x82, 0x84, 0x8b, 0x96, 50, 2, 0x0c, 0x12]
    );
    assert!(output[36..].iter().all(|byte| *byte == 0xa5));

    let frame = inspect_frame(&output[..layout.frame_bytes]).unwrap();
    assert_eq!(frame.kind, FrameKind::Management);
    assert_eq!(frame.subtype, 4);
    assert_eq!(frame.header.len(), 24);
    assert_eq!(frame.body, &output[24..36]);
}

#[test]
fn encodes_specific_ssid_and_omits_empty_extended_rates() {
    let mut output = [0; 34];
    let layout = encode_probe_request(request(b"guest", &[0x82], &[], 0), &mut output).unwrap();
    assert_eq!(layout.frame_bytes, 34);
    assert_eq!(&output[24..31], &[0, 5, b'g', b'u', b'e', b's', b't']);
    assert_eq!(&output[31..34], &[1, 1, 0x82]);
}

#[test]
fn rejects_invalid_inputs_and_short_output_without_modifying_it() {
    let mut output = [0xa5; 32];
    let invalid_address = ProbeRequest {
        transmitter: [0xff; 6],
        ..request(b"", &[0x82], &[], 0)
    };
    assert_eq!(
        encode_probe_request(invalid_address, &mut output),
        Err(ProbeRequestError::InvalidTransmitterAddress)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_probe_request(request(b"", &[], &[], 0), &mut output),
        Err(ProbeRequestError::InvalidSupportedRatesLength)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_probe_request(request(&[b'x'; 33], &[0x82], &[], 0), &mut output),
        Err(ProbeRequestError::SsidTooLong)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_probe_request(request(b"", &[0x82], &[], 0x1000), &mut output),
        Err(ProbeRequestError::SequenceNumberOutOfRange)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    let mut too_small = [0xa5; 28];
    assert_eq!(
        encode_probe_request(request(b"guest", &[0x82], &[], 0), &mut too_small),
        Err(ProbeRequestError::OutputTooSmall)
    );
    assert!(too_small.iter().all(|byte| *byte == 0xa5));
}
