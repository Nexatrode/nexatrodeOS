//! Bounded decoder for the Sunfish QCACLD low-latency HTT RX indication.
//!
//! The decoder follows the pinned `htt_rx_ind_hdr_t` wire layout and returns
//! borrowed firmware-descriptor and MPDU-range spans. It does not consume the
//! separately DMA-filled MSDU address ring, reorder MPDUs, or deliver packets.
#![forbid(unsafe_code)]

use crate::qcom_htc::{HtcCreditReportTotals, HtcError};
use crate::sunfish_qcacld_htc::{HttEndpoint, parse_data_frame};

pub const SUNFISH_HTT_RX_IND_HEADER_BYTES: usize = 48;
pub const SUNFISH_HTT_RX_IND_MAX_EXTENDED_TIDS: u8 = 19;

pub const HTT_T2H_MSG_TYPE_RX_IND: u8 = 0x01;
const RX_IND_RESERVED_BIT: u32 = 1 << 15;
const RX_IND_FLUSH_VALID_BIT: u32 = 1 << 13;
const RX_IND_RELEASE_VALID_BIT: u32 = 1 << 14;
const RX_IND_PEER_ID_SHIFT: u32 = 16;
const RX_IND_RANGE_COUNT_SHIFT: u32 = 24;
const RX_IND_SEQ_NUM_MASK: u32 = 0x3f;
const RX_IND_FLUSH_END_SHIFT: u32 = 6;
const RX_IND_RELEASE_START_SHIFT: u32 = 12;
const RX_IND_RELEASE_END_SHIFT: u32 = 18;
const RX_IND_DESCRIPTOR_BYTES_MASK: u32 = 0x0000_ffff;
const RX_IND_RESERVED_SUFFIX_MASK: u32 = 0xffff_0000;
const RX_IND_RANGE_BYTES: usize = 4;
pub const HTT_RX_IND_MPDU_STATUS_OK: u8 = 1;
pub const HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER: u8 = 6;
const ETHERNET_HEADER_BYTES: usize = 14;
const EAPOL_ETHERTYPE: u16 = 0x888e;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    TruncatedHeader,
    WrongMessageType { observed: u8 },
    ReservedHeaderBits,
    InvalidExtendedTid { observed: u8 },
    ReservedSuffixBits,
    DescriptorSpanOverflow,
    DescriptorSpanOutOfRange,
    RangeTableOverflow,
    RangeTableOutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    Htc(HtcError),
    Htt(Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceRange {
    pub start: u8,
    pub end: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MpduRange {
    pub mpdu_count: u8,
    pub status: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MpduStatus(u8);

impl MpduStatus {
    pub const fn raw(self) -> u8 {
        self.0
    }

    pub const fn is_ok(self) -> bool {
        self.0 == HTT_RX_IND_MPDU_STATUS_OK
    }

    pub const fn is_unauthenticated_peer(self) -> bool {
        self.0 == HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER
    }

    /// QCACLD permits unauthenticated-peer traffic only when the Ethernet
    /// frame carries EAPOL. Other MPDU statuses are not ordinary data frames.
    pub const fn permits_ethernet_frame(self, frame: &[u8]) -> bool {
        if self.is_ok() {
            return true;
        }
        if self.0 != HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER || frame.len() < ETHERNET_HEADER_BYTES {
            return false;
        }
        u16::from_be_bytes([frame[12], frame[13]]) == EAPOL_ETHERTYPE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MpduIndication {
    pub range_index: usize,
    pub index_in_range: u8,
    pub status: MpduStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MpduRangeIter<'a> {
    bytes: &'a [u8],
    remaining: usize,
}

impl Iterator for MpduRangeIter<'_> {
    type Item = MpduRange;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let word = u32::from_le_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]]);
        self.bytes = &self.bytes[RX_IND_RANGE_BYTES..];
        self.remaining -= 1;
        Some(MpduRange {
            mpdu_count: word as u8,
            status: (word >> 8) as u8,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for MpduRangeIter<'_> {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MpduIter<'a> {
    ranges: MpduRangeIter<'a>,
    next_range_index: usize,
    current_range_index: usize,
    current_status: u8,
    current_count: u8,
    remaining_in_range: u8,
}

impl Iterator for MpduIter<'_> {
    type Item = MpduIndication;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.remaining_in_range != 0 {
                let index_in_range = self.current_count - self.remaining_in_range;
                self.remaining_in_range -= 1;
                return Some(MpduIndication {
                    range_index: self.current_range_index,
                    index_in_range,
                    status: MpduStatus(self.current_status),
                });
            }

            let range = self.ranges.next()?;
            self.current_range_index = self.next_range_index;
            self.next_range_index += 1;
            self.current_status = range.status;
            self.current_count = range.mpdu_count;
            self.remaining_in_range = range.mpdu_count;
        }
    }
}

/// A validated view into one ordinary, non-full-reorder `HTT_RX_IND`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttRxIndication<'a> {
    message: &'a [u8],
    peer_id: u16,
    extended_tid: u8,
    flush_valid: bool,
    release_valid: bool,
    flush_sequence_range: SequenceRange,
    release_sequence_range: SequenceRange,
    firmware_descriptor_bytes: &'a [u8],
    alignment_padding: &'a [u8],
    mpdu_range_bytes: &'a [u8],
    mpdu_range_count: usize,
    trailing_start: usize,
}

impl<'a> HttRxIndication<'a> {
    /// Decode the HTT payload after its negotiated HTC data-frame header.
    /// Trailing bytes are retained as outside metadata because the pinned C
    /// handler accepts a message whose calculated range table ends before the
    /// HTC payload length.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < SUNFISH_HTT_RX_IND_HEADER_BYTES {
            return Err(Error::TruncatedHeader);
        }

        let first = read_word(bytes, 0);
        let message_type = first as u8;
        if message_type != HTT_T2H_MSG_TYPE_RX_IND {
            return Err(Error::WrongMessageType {
                observed: message_type,
            });
        }
        if first & RX_IND_RESERVED_BIT != 0 {
            return Err(Error::ReservedHeaderBits);
        }
        let extended_tid = ((first >> 8) & 0x1f) as u8;
        if extended_tid >= SUNFISH_HTT_RX_IND_MAX_EXTENDED_TIDS {
            return Err(Error::InvalidExtendedTid {
                observed: extended_tid,
            });
        }

        let range_count = ((read_word(bytes, 4) >> RX_IND_RANGE_COUNT_SHIFT) & 0xff) as usize;
        let sequence_word = read_word(bytes, 4);
        let suffix = read_word(bytes, SUNFISH_HTT_RX_IND_HEADER_BYTES - 4);
        if suffix & RX_IND_RESERVED_SUFFIX_MASK != 0 {
            return Err(Error::ReservedSuffixBits);
        }
        let descriptor_bytes = (suffix & RX_IND_DESCRIPTOR_BYTES_MASK) as usize;
        let descriptor_end = SUNFISH_HTT_RX_IND_HEADER_BYTES
            .checked_add(descriptor_bytes)
            .ok_or(Error::DescriptorSpanOverflow)?;
        if descriptor_end > bytes.len() {
            return Err(Error::DescriptorSpanOutOfRange);
        }
        let range_offset = descriptor_end
            .checked_add(3)
            .ok_or(Error::RangeTableOverflow)?
            & !3;
        let range_bytes = range_count
            .checked_mul(RX_IND_RANGE_BYTES)
            .ok_or(Error::RangeTableOverflow)?;
        let range_end = range_offset
            .checked_add(range_bytes)
            .ok_or(Error::RangeTableOverflow)?;
        if range_end > bytes.len() {
            return Err(Error::RangeTableOutOfRange);
        }

        Ok(Self {
            message: bytes,
            peer_id: (first >> RX_IND_PEER_ID_SHIFT) as u16,
            extended_tid,
            flush_valid: first & RX_IND_FLUSH_VALID_BIT != 0,
            release_valid: first & RX_IND_RELEASE_VALID_BIT != 0,
            flush_sequence_range: SequenceRange {
                start: (sequence_word & RX_IND_SEQ_NUM_MASK) as u8,
                end: ((sequence_word >> RX_IND_FLUSH_END_SHIFT) & RX_IND_SEQ_NUM_MASK) as u8,
            },
            release_sequence_range: SequenceRange {
                start: ((sequence_word >> RX_IND_RELEASE_START_SHIFT) & RX_IND_SEQ_NUM_MASK) as u8,
                end: ((sequence_word >> RX_IND_RELEASE_END_SHIFT) & RX_IND_SEQ_NUM_MASK) as u8,
            },
            firmware_descriptor_bytes: &bytes[SUNFISH_HTT_RX_IND_HEADER_BYTES..descriptor_end],
            alignment_padding: &bytes[descriptor_end..range_offset],
            mpdu_range_bytes: &bytes[range_offset..range_end],
            mpdu_range_count: range_count,
            trailing_start: range_end,
        })
    }

    pub const fn peer_id(self) -> u16 {
        self.peer_id
    }

    pub const fn extended_tid(self) -> u8 {
        self.extended_tid
    }

    pub const fn flush_valid(self) -> bool {
        self.flush_valid
    }

    pub const fn release_valid(self) -> bool {
        self.release_valid
    }

    pub const fn flush_sequence_range(self) -> SequenceRange {
        self.flush_sequence_range
    }

    pub const fn release_sequence_range(self) -> SequenceRange {
        self.release_sequence_range
    }

    pub const fn firmware_descriptor_bytes(self) -> &'a [u8] {
        self.firmware_descriptor_bytes
    }

    pub const fn alignment_padding(self) -> &'a [u8] {
        self.alignment_padding
    }

    pub const fn mpdu_range_count(self) -> usize {
        self.mpdu_range_count
    }

    pub fn trailing_bytes(self) -> &'a [u8] {
        &self.message[self.trailing_start..]
    }

    pub fn mpdu_ranges(self) -> MpduRangeIter<'a> {
        MpduRangeIter {
            bytes: self.mpdu_range_bytes,
            remaining: self.mpdu_range_count,
        }
    }

    /// Expand the range table into MPDU entries without consuming DMA buffers.
    /// Each range contributes `mpdu_count` MPDUs with its status; zero-count
    /// ranges are skipped. Firmware may provide unknown status values, which
    /// are preserved in `MpduStatus::raw()` and are never treated as success.
    pub fn mpdus(self) -> MpduIter<'a> {
        MpduIter {
            ranges: self.mpdu_ranges(),
            next_range_index: 0,
            current_range_index: 0,
            current_status: 0,
            current_count: 0,
            remaining_in_range: 0,
        }
    }
}

/// Compose dynamic HTC endpoint validation with the HTT receive-indication
/// decoder used by the CE1 completion path.
pub fn decode_htc_data_frame<'a>(
    endpoint: HttEndpoint,
    bytes: &'a [u8],
) -> Result<(u8, HtcCreditReportTotals, HttRxIndication<'a>), FrameError> {
    let frame = parse_data_frame(endpoint, bytes).map_err(FrameError::Htc)?;
    let indication = HttRxIndication::decode(frame.htt_payload).map_err(FrameError::Htt)?;
    Ok((frame.sequence, frame.credit_returns, indication))
}

const fn read_word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
