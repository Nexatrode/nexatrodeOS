//! Bounded Ethernet-II to station-mode Native-WiFi data encapsulation.
//!
//! This builds an unprotected To-DS 802.11 frame with RFC 1042 LLC/SNAP. It
//! does not classify QoS, assign a peer, protect the frame, or authorize TX;
//! those decisions belong to the station/security owner and QCACLD adapter.
#![forbid(unsafe_code)]

pub const ETHERNET_HEADER_BYTES: usize = 14;
pub const IEEE80211_DATA_HEADER_BYTES: usize = 24;
pub const LLC_SNAP_BYTES: usize = 8;
pub const MAX_ETHERNET_FRAME_BYTES: usize = 2048;
pub const MAX_NATIVE_FRAME_BYTES: usize =
    IEEE80211_DATA_HEADER_BYTES + LLC_SNAP_BYTES + MAX_ETHERNET_FRAME_BYTES - ETHERNET_HEADER_BYTES;
pub const SEQUENCE_NUMBER_MAX: u16 = 0x0fff;

const ETHERTYPE_AARP: u16 = 0x80f3;
const ETHERTYPE_IPX: u16 = 0x8137;
const FC_DATA_TO_DS: u16 = 0x0108;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EthernetFrameTooShort,
    EthernetFrameTooLong { observed: usize },
    Unsupported8023Length { observed: u16 },
    InvalidStationAddress,
    InvalidBssid,
    EthernetSourceMismatch,
    InvalidSequenceNumber { observed: u16 },
    OutputTooSmall { required: usize, available: usize },
}

/// Encapsulate one Ethernet-II frame in a non-QoS, unprotected To-DS data
/// frame. The station address must match the Ethernet source; both it and
/// `bssid` must identify the selected associated link.
/// The caller must apply CCMP after this operation for protected data traffic;
/// EAPOL and controlled-port policy remain with the station owner.
pub fn encapsulate_ethernet_ii(
    ethernet: &[u8],
    station: [u8; 6],
    bssid: [u8; 6],
    sequence: u16,
    output: &mut [u8],
) -> Result<usize, Error> {
    if ethernet.len() < ETHERNET_HEADER_BYTES {
        return Err(Error::EthernetFrameTooShort);
    }
    if ethernet.len() > MAX_ETHERNET_FRAME_BYTES {
        return Err(Error::EthernetFrameTooLong {
            observed: ethernet.len(),
        });
    }
    let ethertype = u16::from_be_bytes([ethernet[12], ethernet[13]]);
    if ethertype <= 1500 {
        return Err(Error::Unsupported8023Length {
            observed: ethertype,
        });
    }
    if !valid_unicast_address(station) {
        return Err(Error::InvalidStationAddress);
    }
    if !valid_unicast_address(bssid) {
        return Err(Error::InvalidBssid);
    }
    if ethernet[6..12] != station {
        return Err(Error::EthernetSourceMismatch);
    }
    if sequence > SEQUENCE_NUMBER_MAX {
        return Err(Error::InvalidSequenceNumber { observed: sequence });
    }

    let frame_bytes =
        IEEE80211_DATA_HEADER_BYTES + LLC_SNAP_BYTES + ethernet.len() - ETHERNET_HEADER_BYTES;
    if output.len() < frame_bytes {
        return Err(Error::OutputTooSmall {
            required: frame_bytes,
            available: output.len(),
        });
    }

    output[..IEEE80211_DATA_HEADER_BYTES].fill(0);
    output[..2].copy_from_slice(&FC_DATA_TO_DS.to_le_bytes());
    output[4..10].copy_from_slice(&bssid);
    output[10..16].copy_from_slice(&station);
    output[16..22].copy_from_slice(&ethernet[..6]);
    output[22..24].copy_from_slice(&(sequence << 4).to_le_bytes());

    let oui = match ethertype {
        ETHERTYPE_AARP | ETHERTYPE_IPX => [0x00, 0x00, 0xf8],
        _ => [0x00, 0x00, 0x00],
    };
    let snap =
        &mut output[IEEE80211_DATA_HEADER_BYTES..IEEE80211_DATA_HEADER_BYTES + LLC_SNAP_BYTES];
    snap[..3].copy_from_slice(&[0xaa, 0xaa, 0x03]);
    snap[3..6].copy_from_slice(&oui);
    snap[6..8].copy_from_slice(&ethertype.to_be_bytes());
    output[IEEE80211_DATA_HEADER_BYTES + LLC_SNAP_BYTES..frame_bytes]
        .copy_from_slice(&ethernet[ETHERNET_HEADER_BYTES..]);
    Ok(frame_bytes)
}

fn valid_unicast_address(address: [u8; 6]) -> bool {
    address.iter().any(|byte| *byte != 0) && address[0] & 1 == 0
}
