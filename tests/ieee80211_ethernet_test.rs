#![forbid(unsafe_code)]

#[path = "../rust/src/ieee80211_ethernet.rs"]
mod ieee80211_ethernet;

use ieee80211_ethernet::{
    encapsulate_ethernet_ii, Error, LLC_SNAP_BYTES, MAX_ETHERNET_FRAME_BYTES,
    MAX_NATIVE_FRAME_BYTES,
};

const STATION: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
const BSSID: [u8; 6] = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60];
const DESTINATION: [u8; 6] = [0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb];

fn ethernet_frame(ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; 14 + payload.len()];
    frame[..6].copy_from_slice(&DESTINATION);
    frame[6..12].copy_from_slice(&STATION);
    frame[12..14].copy_from_slice(&ethertype.to_be_bytes());
    frame[14..].copy_from_slice(payload);
    frame
}

#[test]
fn builds_non_qos_to_ds_rfc1042_data_frame() {
    let ethernet = ethernet_frame(0x0800, &[0x45, 0, 0, 20, 1, 2, 3, 4]);
    let mut output = [0xa5; 64];
    let frame_bytes =
        encapsulate_ethernet_ii(&ethernet, STATION, BSSID, 0x123, &mut output).unwrap();

    assert_eq!(frame_bytes, 24 + LLC_SNAP_BYTES + 8);
    assert_eq!(&output[..2], &0x0108u16.to_le_bytes());
    assert_eq!(&output[4..10], &BSSID);
    assert_eq!(&output[10..16], &STATION);
    assert_eq!(&output[16..22], &DESTINATION);
    assert_eq!(&output[22..24], &(0x1230u16).to_le_bytes());
    assert_eq!(&output[24..32], &[0xaa, 0xaa, 0x03, 0, 0, 0, 0x08, 0]);
    assert_eq!(&output[32..frame_bytes], &ethernet[14..]);
    assert!(output[frame_bytes..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn uses_bridge_tunnel_oui_for_aarp_and_ipx() {
    for ethertype in [0x80f3, 0x8137] {
        let ethernet = ethernet_frame(ethertype, &[0x11]);
        let mut output = [0; 64];
        encapsulate_ethernet_ii(&ethernet, STATION, BSSID, 0, &mut output).unwrap();
        assert_eq!(&output[27..30], &[0, 0, 0xf8]);
        assert_eq!(&output[30..32], &ethertype.to_be_bytes());
    }
}

#[test]
fn supports_vlan_as_an_ethernet_ii_protocol() {
    let ethernet = ethernet_frame(0x8100, &[0, 7, 0x08, 0x00, 0x45]);
    let mut output = [0; 64];
    let frame_bytes = encapsulate_ethernet_ii(&ethernet, STATION, BSSID, 1, &mut output).unwrap();
    assert_eq!(&output[30..32], &0x8100u16.to_be_bytes());
    assert_eq!(&output[32..frame_bytes], &ethernet[14..]);
}

#[test]
fn rejects_invalid_frames_without_changing_output() {
    let mut output = [0x5a; 64];
    assert_eq!(
        encapsulate_ethernet_ii(&[0; 13], STATION, BSSID, 0, &mut output),
        Err(Error::EthernetFrameTooShort)
    );
    assert_eq!(
        encapsulate_ethernet_ii(&ethernet_frame(1500, &[]), STATION, BSSID, 0, &mut output),
        Err(Error::Unsupported8023Length { observed: 1500 })
    );
    assert_eq!(
        encapsulate_ethernet_ii(&ethernet_frame(0x0800, &[]), [1; 6], BSSID, 0, &mut output),
        Err(Error::InvalidStationAddress)
    );
    assert_eq!(
        encapsulate_ethernet_ii(
            &ethernet_frame(0x0800, &[]),
            STATION,
            [0; 6],
            0,
            &mut output
        ),
        Err(Error::InvalidBssid)
    );
    let mut wrong_source = ethernet_frame(0x0800, &[]);
    wrong_source[6] ^= 2;
    assert_eq!(
        encapsulate_ethernet_ii(&wrong_source, STATION, BSSID, 0, &mut output),
        Err(Error::EthernetSourceMismatch)
    );
    assert_eq!(
        encapsulate_ethernet_ii(
            &ethernet_frame(0x0800, &[]),
            STATION,
            BSSID,
            0x1000,
            &mut output
        ),
        Err(Error::InvalidSequenceNumber { observed: 0x1000 })
    );
    assert_eq!(output, [0x5a; 64]);
}

#[test]
fn rejects_undersized_output_and_oversized_sddf_frame() {
    let ethernet = ethernet_frame(0x0800, &[1, 2, 3]);
    let mut short = [0xa5; 34];
    assert_eq!(
        encapsulate_ethernet_ii(&ethernet, STATION, BSSID, 0, &mut short),
        Err(Error::OutputTooSmall {
            required: 24 + 8 + 3,
            available: 34,
        })
    );
    assert_eq!(short, [0xa5; 34]);

    let oversized = vec![0; MAX_ETHERNET_FRAME_BYTES + 1];
    assert_eq!(
        encapsulate_ethernet_ii(
            &oversized,
            STATION,
            BSSID,
            0,
            &mut [0; MAX_NATIVE_FRAME_BYTES]
        ),
        Err(Error::EthernetFrameTooLong {
            observed: MAX_ETHERNET_FRAME_BYTES + 1
        })
    );
}

#[test]
fn accepts_the_full_sddf_buffer_at_the_exact_native_frame_bound() {
    let payload = vec![0x3c; MAX_ETHERNET_FRAME_BYTES - 14];
    let ethernet = ethernet_frame(0x0800, &payload);
    let mut output = [0xa5; MAX_NATIVE_FRAME_BYTES];
    let frame_bytes =
        encapsulate_ethernet_ii(&ethernet, STATION, BSSID, 0xfff, &mut output).unwrap();
    assert_eq!(frame_bytes, MAX_NATIVE_FRAME_BYTES);
    assert_eq!(
        &output[frame_bytes - 1..frame_bytes],
        &ethernet[ethernet.len() - 1..]
    );
}
