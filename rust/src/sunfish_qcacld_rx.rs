//! Bounded decoder for the Sunfish QCACLD low-latency RX descriptor.
//!
//! This decodes only the pinned LineageOS `htt_host_rx_desc_base` fields used
//! to decide whether a completed ring buffer contains one contiguous MSDU or
//! a chained MSDU. It does not touch DMA memory, reclaim ring entries, parse
//! 802.11 headers, or assert that firmware ownership is valid.
#![forbid(unsafe_code)]

use crate::sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use crate::sunfish_htt_rx_ring::SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS;

const WORD_BYTES: usize = 4;
const FW_DESC_BYTE_OFFSET: usize = 0;
const FW_DESC_DISCARD: u8 = 1 << 0;
const FW_DESC_FORWARD: u8 = 1 << 1;
const FW_DESC_ANY_ERROR: u8 = 1 << 2;
const FW_DESC_DUPLICATE_ERROR: u8 = 1 << 3;
const FW_DESC_IPA_INDICATION: u8 = 1 << 4;
const FW_DESC_INSPECT: u8 = 1 << 5;
const FW_DESC_EXTENSION_MASK: u8 = 0xc0;
const ATTENTION_MPDU_LENGTH_ERROR: u32 = 1 << 27;
const ATTENTION_MSDU_LENGTH_ERROR: u32 = 1 << 17;
const ATTENTION_MSDU_DONE: u32 = 1 << 31;
const FRAG_INFO_RING2_MORE_COUNT_MASK: u32 = 0x00ff_0000;
const FRAG_INFO_RING2_MORE_COUNT_SHIFT: u32 = 16;
const MSDU_START_LENGTH_MASK: u32 = 0x0000_3fff;
const MSDU_START_DECAP_FORMAT_OFFSET: usize = 2 * WORD_BYTES;
const MSDU_START_DECAP_FORMAT_MASK: u32 = 0x0000_0300;
const MSDU_START_DECAP_FORMAT_SHIFT: u32 = 8;
const MSDU_END_FIRST_MSDU: u32 = 1 << 14;
const MSDU_END_LAST_MSDU: u32 = 1 << 15;
const MSDU_END_L3_PADDING_MASK: u32 = 0x0000_1c00;
const MSDU_END_L3_PADDING_SHIFT: u32 = 10;
const MSDU_END_WORD4_INDEX: usize = 4;
const MSDU_END_L3_PADDING_WORD_INDEX: usize = 8;

/// Captured QCACLD WMI INIT uses `CFG_TGT_RX_DECAP_MODE = 2` for Sunfish.
/// The corresponding hardware RX descriptor value is the 802.3 format.
pub const SUNFISH_RX_DECAP_FORMAT_8023: u8 = 2;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParseOptions {
    /// The matched QCACLD path applies this padding only for the supported
    /// target IP revision. Keep that runtime predicate explicit at the call.
    pub l3_header_padding_active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareFlags {
    pub discard: bool,
    pub forward: bool,
    pub any_error: bool,
    pub duplicate_error: bool,
    pub ipa_indication: bool,
    pub inspect: bool,
    /// Nonzero values announce an extension the caller must interpret before
    /// consuming the payload as an ordinary MSDU.
    pub extension: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxDescriptor {
    pub firmware: FirmwareFlags,
    pub mpdu_length_error: bool,
    pub msdu_length_error: bool,
    pub more_buffers: u8,
    pub msdu_length: u16,
    pub decap_format: u8,
    pub first_msdu: bool,
    pub last_msdu: bool,
    pub l3_header_padding: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidProfile,
    DescriptorTooShort,
    BufferTooShort,
    NotComplete,
    ChainedMsdu,
    FirmwareExtension,
    MsduLengthOutOfRange,
    OffsetOverflow,
}

fn word_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset
        .checked_add(WORD_BYTES)
        .ok_or(Error::OffsetOverflow)?;
    let word = bytes.get(offset..end).ok_or(Error::DescriptorTooShort)?;
    Ok(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

fn word_offset(word_index: u16) -> Result<usize, Error> {
    usize::from(word_index)
        .checked_mul(WORD_BYTES)
        .ok_or(Error::OffsetOverflow)
}

impl RxDescriptor {
    /// Parse a complete hardware descriptor at the beginning of one HTT RX
    /// ring buffer. A descriptor without `MSDU_DONE` is not exposed to callers.
    pub fn decode(bytes: &[u8], options: ParseOptions) -> Result<Self, Error> {
        Self::decode_inner(bytes, options, None)
    }

    /// Parse the hardware descriptor while using the per-MSDU firmware status
    /// byte carried separately in `HTT_RX_IND`. QCACLD copies this byte into
    /// the ring descriptor before applying firmware discard/forward policy.
    pub fn decode_with_firmware_descriptor(
        bytes: &[u8],
        firmware_descriptor: u8,
        options: ParseOptions,
    ) -> Result<Self, Error> {
        Self::decode_inner(bytes, options, Some(firmware_descriptor))
    }

    fn decode_inner(
        bytes: &[u8],
        options: ParseOptions,
        firmware_descriptor: Option<u8>,
    ) -> Result<Self, Error> {
        let profile =
            plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).map_err(|_| Error::InvalidProfile)?;
        let descriptor_bytes = usize::try_from(SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes)
            .map_err(|_| Error::InvalidProfile)?;
        if bytes.len() < descriptor_bytes {
            return Err(Error::DescriptorTooShort);
        }

        let attention_offset = word_offset(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words)?;
        let frag_info_offset = word_offset(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.fragment_info_words)?;
        let msdu_start_offset = word_offset(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words)?;
        let msdu_end_offset = word_offset(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words)?;
        let attention = word_at(bytes, attention_offset)?;
        if attention & ATTENTION_MSDU_DONE == 0 {
            return Err(Error::NotComplete);
        }

        let frag_info = word_at(bytes, frag_info_offset)?;
        let msdu_start = word_at(bytes, msdu_start_offset)?;
        let msdu_start_decap = word_at(
            bytes,
            msdu_start_offset
                .checked_add(MSDU_START_DECAP_FORMAT_OFFSET)
                .ok_or(Error::OffsetOverflow)?,
        )?;
        let msdu_end_word4_offset = msdu_end_offset
            .checked_add(MSDU_END_WORD4_INDEX * WORD_BYTES)
            .ok_or(Error::OffsetOverflow)?;
        let msdu_end_word4 = word_at(bytes, msdu_end_word4_offset)?;
        let msdu_end_padding_offset = msdu_end_offset
            .checked_add(MSDU_END_L3_PADDING_WORD_INDEX * WORD_BYTES)
            .ok_or(Error::OffsetOverflow)?;
        let msdu_end_padding = word_at(bytes, msdu_end_padding_offset)?;
        let more_buffers = ((frag_info & FRAG_INFO_RING2_MORE_COUNT_MASK)
            >> FRAG_INFO_RING2_MORE_COUNT_SHIFT) as u8;
        let msdu_length = (msdu_start & MSDU_START_LENGTH_MASK) as u16;
        let l3_header_padding = if options.l3_header_padding_active {
            ((msdu_end_padding & MSDU_END_L3_PADDING_MASK) >> MSDU_END_L3_PADDING_SHIFT) as u8
        } else {
            0
        };

        // A non-chained MSDU must fit in the mapped ring buffer after both the
        // descriptor reservation and target-reported L3 padding.
        if more_buffers == 0 {
            let buffer_bytes =
                usize::try_from(profile.rx_buffer_bytes()).map_err(|_| Error::InvalidProfile)?;
            let payload_start = descriptor_bytes
                .checked_add(usize::from(l3_header_padding))
                .ok_or(Error::OffsetOverflow)?;
            let payload_end = payload_start
                .checked_add(usize::from(msdu_length))
                .ok_or(Error::OffsetOverflow)?;
            if payload_start > buffer_bytes || payload_end > buffer_bytes {
                return Err(Error::MsduLengthOutOfRange);
            }
        }

        let fw = match firmware_descriptor {
            Some(value) => value,
            None => *bytes
                .get(FW_DESC_BYTE_OFFSET)
                .ok_or(Error::DescriptorTooShort)?,
        };
        Ok(Self {
            firmware: FirmwareFlags {
                discard: fw & FW_DESC_DISCARD != 0,
                forward: fw & FW_DESC_FORWARD != 0,
                any_error: fw & FW_DESC_ANY_ERROR != 0,
                duplicate_error: fw & FW_DESC_DUPLICATE_ERROR != 0,
                ipa_indication: fw & FW_DESC_IPA_INDICATION != 0,
                inspect: fw & FW_DESC_INSPECT != 0,
                extension: (fw & FW_DESC_EXTENSION_MASK) >> 6,
            },
            mpdu_length_error: attention & ATTENTION_MPDU_LENGTH_ERROR != 0,
            msdu_length_error: attention & ATTENTION_MSDU_LENGTH_ERROR != 0,
            more_buffers,
            msdu_length,
            decap_format: ((msdu_start_decap & MSDU_START_DECAP_FORMAT_MASK)
                >> MSDU_START_DECAP_FORMAT_SHIFT) as u8,
            first_msdu: msdu_end_word4 & MSDU_END_FIRST_MSDU != 0,
            last_msdu: msdu_end_word4 & MSDU_END_LAST_MSDU != 0,
            l3_header_padding,
        })
    }
}

/// Decode and borrow a packet only when its descriptor and bytes belong to
/// this same single RX buffer. Chained buffers and firmware extensions need a
/// separate assembler/parser.
pub fn decode_single_buffer<'a>(
    buffer: &'a [u8],
    options: ParseOptions,
) -> Result<(RxDescriptor, &'a [u8]), Error> {
    decode_single_buffer_inner(buffer, None, options)
}

/// Single-buffer variant using the firmware status byte from the enclosing
/// `HTT_RX_IND` rather than trusting the buffer's first byte.
pub fn decode_single_buffer_with_firmware_descriptor<'a>(
    buffer: &'a [u8],
    firmware_descriptor: u8,
    options: ParseOptions,
) -> Result<(RxDescriptor, &'a [u8]), Error> {
    decode_single_buffer_inner(buffer, Some(firmware_descriptor), options)
}

fn decode_single_buffer_inner<'a>(
    buffer: &'a [u8],
    firmware_descriptor: Option<u8>,
    options: ParseOptions,
) -> Result<(RxDescriptor, &'a [u8]), Error> {
    let descriptor = match firmware_descriptor {
        Some(value) => RxDescriptor::decode_with_firmware_descriptor(buffer, value, options)?,
        None => RxDescriptor::decode(buffer, options)?,
    };
    if descriptor.more_buffers != 0 {
        return Err(Error::ChainedMsdu);
    }
    if descriptor.firmware.extension != 0 {
        return Err(Error::FirmwareExtension);
    }
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).map_err(|_| Error::InvalidProfile)?;
    let buffer_bytes =
        usize::try_from(profile.rx_buffer_bytes()).map_err(|_| Error::InvalidProfile)?;
    if buffer.len() < buffer_bytes {
        return Err(Error::BufferTooShort);
    }
    let descriptor_bytes = usize::try_from(SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes)
        .map_err(|_| Error::InvalidProfile)?;
    let start = descriptor_bytes
        .checked_add(usize::from(descriptor.l3_header_padding))
        .ok_or(Error::OffsetOverflow)?;
    let end = start
        .checked_add(usize::from(descriptor.msdu_length))
        .ok_or(Error::OffsetOverflow)?;
    if end > buffer_bytes {
        return Err(Error::MsduLengthOutOfRange);
    }
    let payload = buffer.get(start..end).ok_or(Error::BufferTooShort)?;
    Ok((descriptor, payload))
}
