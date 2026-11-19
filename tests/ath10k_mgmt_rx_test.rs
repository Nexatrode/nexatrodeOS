#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_mgmt_rx.rs"]
mod ath10k_mgmt_rx;
#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;

use ath10k_mgmt_rx::{
    Error, ManagementRxEvent, WMI_10_4_MGMT_RX_HEADER_BYTES, WMI_RX_STATUS_EXT_INFO,
    parse_management_rx_event,
};

fn event(frame: &[u8], status: u32) -> Vec<u8> {
    let mut bytes = vec![0; WMI_10_4_MGMT_RX_HEADER_BYTES + frame.len()];
    bytes[0..4].copy_from_slice(&6u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&42u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&[0x80, 0x7f, 1, 2]);
    bytes[12..16].copy_from_slice(&540u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&(frame.len() as u32).to_le_bytes());
    bytes[24..28].copy_from_slice(&status.to_le_bytes());
    bytes[WMI_10_4_MGMT_RX_HEADER_BYTES..].copy_from_slice(frame);
    bytes
}

#[test]
fn parses_borrowed_management_frame_and_10_4_metadata() {
    let frame = [0x80, 0, 0xaa, 0xbb];
    let mut bytes = event(&frame, 0);
    bytes.extend_from_slice(&[0, 0, 0, 0]); // Trailing transport padding.

    let parsed = parse_management_rx_event(&bytes).unwrap();
    assert_eq!(
        parsed,
        ManagementRxEvent {
            channel_number: 6,
            snr: 42,
            rssi_control: [0x80, 0x7f, 1, 2],
            rate: 540,
            phy_mode: 3,
            status: 0,
            frame: &frame,
            rx_mac_timestamp: None,
        }
    );
}

#[test]
fn reads_optional_timestamp_after_four_byte_aligned_frame() {
    let mut bytes = event(&[0x50, 0, 0xaa], WMI_RX_STATUS_EXT_INFO);
    bytes.push(0xee); // The event places the timestamp after aligned frame bytes.
    bytes.extend_from_slice(&0x0123_4567_89ab_cdefu64.to_le_bytes());

    let parsed = parse_management_rx_event(&bytes).unwrap();
    assert_eq!(parsed.frame, &[0x50, 0, 0xaa]);
    assert_eq!(parsed.rx_mac_timestamp, Some(0x0123_4567_89ab_cdef));
}

#[test]
fn rejects_short_header_frame_and_extension_without_panicking() {
    let full = event(&[0x80, 0], 0);
    for length in 0..WMI_10_4_MGMT_RX_HEADER_BYTES {
        assert_eq!(
            parse_management_rx_event(&full[..length]),
            Err(Error::HeaderTooShort)
        );
    }

    assert_eq!(
        parse_management_rx_event(&event(&[], 0)),
        Err(Error::FrameTooShort)
    );
    assert_eq!(
        parse_management_rx_event(&event(&[0], 0)),
        Err(Error::FrameTooShort)
    );
    assert_eq!(
        parse_management_rx_event(&event(&[0x80], 0)),
        Err(Error::FrameTooShort)
    );
    assert_eq!(
        parse_management_rx_event(&event(&[0x80, 0], WMI_RX_STATUS_EXT_INFO)),
        Err(Error::ExtensionInfoTruncated)
    );

    let mut bad_length = event(&[0x80, 0], 0);
    bad_length[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        parse_management_rx_event(&bad_length),
        Err(Error::FrameLengthOutOfBounds)
    );
}
