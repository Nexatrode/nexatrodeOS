//! Offline QCACLD native-Wi-Fi HTT TX descriptor encoding for Sunfish.
//!
//! This mirrors the pinned Sunfish QCACLD `HELIUMPLUS` / Wi-Fi IP 2.0 / 64-bit
//! HTT profile. It encodes one already-built contiguous 802.11 data frame and
//! its HTT descriptor plus one-entry fragment table. It does not allocate or
//! map DMA, clean caches, compose the outer HTC header, touch CE/MMIO, or start
//! WLAN. Callers must supply IOVAs established by an independently validated
//! DMA/SMMU owner before using this codec in a transmit path.
#![forbid(unsafe_code)]

use crate::ath10k_nwifi_rx::{FrameHeaderError, FrameKind, inspect_frame};

pub const HTT_TX_DESC_BYTES: usize = 20;
pub const HELIUMPLUS_TSO_PREFIX_BYTES: usize = 24;
pub const HELIUMPLUS_FRAGMENT_SLOTS: usize = 6;
pub const HELIUMPLUS_FRAGMENT_DESC_BYTES: usize =
    HELIUMPLUS_TSO_PREFIX_BYTES + HELIUMPLUS_FRAGMENT_SLOTS * 8;
pub const MAX_FRAME_IOVA: u64 = 1 << 37;
pub const QCACLD_WIFI_IP_VERSION: (u8, u8) = (2, 0);
/// QCACLD `HTT_DATA_MSG_SVC` (`HTT_SERVICE_GROUP:0`).
pub const HTT_DATA_SERVICE_ID: u16 = 0x0300;
/// Sunfish `target_service_to_ce_map_wlan`: HTT packet-data downlink.
pub const HTT_DATA_HOST_TO_TARGET_CE: u8 = 4;
/// Sunfish `target_service_to_ce_map_wlan`: HTT RX indication uplink.
pub const HTT_DATA_TARGET_TO_HOST_CE: u8 = 1;

const HTT_H2T_MSG_TYPE_TX_FRM: u32 = 1;
const HTT_PKT_TYPE_NATIVE_WIFI: u32 = 1;
const HTT_TX_DESC_NO_ENCRYPT: u32 = 1 << 10;
const HTT_TX_DESC_TX_COMP_REQ: u32 = 1 << 31;
const HTT_INVALID_PEER_ID: u16 = 0xffff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxMetadata {
    /// Coherent fragment-descriptor table IOVA. The matched driver stores this
    /// pointer in the low 32 bits of the 64-bit HTT descriptor field.
    pub fragment_table_iova: u64,
    /// DMA address of the complete contiguous 802.11 frame (37-bit QCACLD ABI).
    pub frame_iova: u64,
    pub msdu_id: u16,
    pub vdev_id: u8,
    /// QCACLD's already-classified extended TID (0..=31).
    pub ext_tid: u8,
    /// Whether firmware is expected to encrypt this frame.
    pub hardware_encrypt: bool,
    /// Set only when matching the high-latency profile's per-frame completion
    /// request. Low-latency QCACLD requires completions independently.
    pub request_completion: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedTx {
    /// Little-endian `htt_tx_msdu_desc64_t` bytes, excluding HTC header.
    pub htt_descriptor: [u8; HTT_TX_DESC_BYTES],
    /// Zeroed `msdu_ext_desc_t` with one fragment and a zero terminator.
    pub fragment_descriptor: [u8; HELIUMPLUS_FRAGMENT_DESC_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyFrame,
    FrameTooLong { observed: usize },
    InvalidFrame(FrameHeaderError),
    NotDataFrame,
    InvalidVdevId { observed: u8 },
    InvalidExtTid { observed: u8 },
    NullFragmentTableIova,
    UnalignedFragmentTableIova { observed: u64 },
    FragmentTableOutsideLow32 { observed: u64 },
    NullFrameIova,
    FrameIovaOutside37Bit { observed: u64, bytes: usize },
}

/// Encode one QCACLD native-Wi-Fi data frame for the matched Sunfish ABI.
///
/// `fragment_table_iova` must identify a 72-byte coherent region below 4 GiB;
/// `frame_iova` must identify a contiguous frame wholly inside the 37-bit
/// firmware-visible range. These are checked numerically only—the caller is
/// responsible for ensuring they are live mappings owned by its DMA gate.
pub fn encode_native_wifi_data(frame: &[u8], metadata: TxMetadata) -> Result<EncodedTx, Error> {
    if frame.is_empty() {
        return Err(Error::EmptyFrame);
    }
    if frame.len() > usize::from(u16::MAX) {
        return Err(Error::FrameTooLong {
            observed: frame.len(),
        });
    }
    let view = inspect_frame(frame).map_err(Error::InvalidFrame)?;
    if view.kind != FrameKind::Data {
        return Err(Error::NotDataFrame);
    }
    if metadata.vdev_id >= 64 {
        return Err(Error::InvalidVdevId {
            observed: metadata.vdev_id,
        });
    }
    if metadata.ext_tid >= 32 {
        return Err(Error::InvalidExtTid {
            observed: metadata.ext_tid,
        });
    }
    if metadata.fragment_table_iova == 0 {
        return Err(Error::NullFragmentTableIova);
    }
    if metadata.fragment_table_iova & 7 != 0 {
        return Err(Error::UnalignedFragmentTableIova {
            observed: metadata.fragment_table_iova,
        });
    }
    let table_end = metadata
        .fragment_table_iova
        .checked_add(HELIUMPLUS_FRAGMENT_DESC_BYTES as u64)
        .ok_or(Error::FragmentTableOutsideLow32 {
            observed: metadata.fragment_table_iova,
        })?;
    if table_end > (u64::from(u32::MAX) + 1) {
        return Err(Error::FragmentTableOutsideLow32 {
            observed: metadata.fragment_table_iova,
        });
    }
    if metadata.frame_iova == 0 {
        return Err(Error::NullFrameIova);
    }
    let frame_end = metadata.frame_iova.checked_add(frame.len() as u64).ok_or(
        Error::FrameIovaOutside37Bit {
            observed: metadata.frame_iova,
            bytes: frame.len(),
        },
    )?;
    if frame_end > MAX_FRAME_IOVA {
        return Err(Error::FrameIovaOutside37Bit {
            observed: metadata.frame_iova,
            bytes: frame.len(),
        });
    }

    let mut htt_descriptor = [0u8; HTT_TX_DESC_BYTES];
    let mut word0 = HTT_H2T_MSG_TYPE_TX_FRM
        | (HTT_PKT_TYPE_NATIVE_WIFI << 13)
        | (u32::from(metadata.vdev_id) << 16)
        | (u32::from(metadata.ext_tid) << 22);
    if !metadata.hardware_encrypt {
        word0 |= HTT_TX_DESC_NO_ENCRYPT;
    }
    if metadata.request_completion {
        word0 |= HTT_TX_DESC_TX_COMP_REQ;
    }
    htt_descriptor[0..4].copy_from_slice(&word0.to_le_bytes());

    let word1 = (frame.len() as u32) | (u32::from(metadata.msdu_id) << 16);
    htt_descriptor[4..8].copy_from_slice(&word1.to_le_bytes());
    // QCACLD's HELIUMPLUS pointer updater writes the fragment-table IOVA low
    // word and explicitly clears the high word for this descriptor ABI.
    htt_descriptor[8..12].copy_from_slice(&(metadata.fragment_table_iova as u32).to_le_bytes());
    htt_descriptor[12..16].copy_from_slice(&0u32.to_le_bytes());
    htt_descriptor[16..18].copy_from_slice(&HTT_INVALID_PEER_ID.to_le_bytes());
    // chanfreq is zero: request the vdev's home channel.

    let mut fragment_descriptor = [0u8; HELIUMPLUS_FRAGMENT_DESC_BYTES];
    let fragment_lo = metadata.frame_iova as u32;
    let fragment_hi_and_len =
        (((frame.len() as u32) & 0xffff) << 16) | ((metadata.frame_iova >> 32) as u32 & 0xffff);
    fragment_descriptor[HELIUMPLUS_TSO_PREFIX_BYTES..HELIUMPLUS_TSO_PREFIX_BYTES + 4]
        .copy_from_slice(&fragment_lo.to_le_bytes());
    fragment_descriptor[HELIUMPLUS_TSO_PREFIX_BYTES + 4..HELIUMPLUS_TSO_PREFIX_BYTES + 8]
        .copy_from_slice(&fragment_hi_and_len.to_le_bytes());
    // The next all-zero fragment entry terminates the list, as in
    // htt_tx_desc_num_frags(..., 1); remaining slots stay zero as well.

    Ok(EncodedTx {
        htt_descriptor,
        fragment_descriptor,
    })
}
