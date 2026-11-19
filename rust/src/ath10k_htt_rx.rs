//! Bounded WCN3990 ath10k HTT receive-buffer decoding and raw-MSDU unchaining.
//!
//! The layout and chain rules follow the pinned Sunfish ath10k source. This
//! module only reads caller-owned snapshots and writes into a caller-provided
//! output slice. It does not own an RX ring, perform DMA/cache operations, or
//! access the device.

const RX_DESCRIPTOR_BYTES: usize = 360;
const RX_BUFFER_BYTES: usize = 2048;
const RX_PAYLOAD_OFFSET: usize = 360;
const RX_ATTENTION_OFFSET: usize = 4;
const RX_FRAG_INFO_OFFSET: usize = 8;
const RX_MSDU_START_INFO0_OFFSET: usize = 28;
const RX_MSDU_START_INFO1_OFFSET: usize = 36;
const RX_MSDU_END_INFO0_OFFSET: usize = 64;
const RX_IN_ORD_T2H_MESSAGE_TYPE: u8 = 0x12;
const RX_IN_ORD_FIXED_BYTES: usize = 8; // one-byte HTT header + seven-byte indication
const RX_IN_ORD_64_DESCRIPTOR_BYTES: usize = 12;
const RX_IN_ORD_MAX_DESCRIPTORS: u16 = 2047;

const ATTENTION_MSDU_LENGTH_ERROR: u32 = 1 << 17;
const ATTENTION_MPDU_LENGTH_ERROR: u32 = 1 << 27;
const ATTENTION_MSDU_DONE: u32 = 1 << 31;
const MSDU_LENGTH_MASK: u32 = 0x0000_3fff;
const DECAP_FORMAT_MASK: u32 = 0x0000_0300;
const DECAP_FORMAT_SHIFT: u32 = 8;
const FIRST_MSDU: u32 = 1 << 14;
const LAST_MSDU: u32 = 1 << 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    DescriptorTooShort,
    BufferTooShort,
    NotComplete,
    ChainedMsdu,
    LengthError,
    MsduLengthOutOfRange,
    ContinuationCountMismatch,
    OutputTooSmall,
    InOrderWrongMessageType,
    InOrderHeaderTooShort,
    InOrderCountExceedsRing,
    InOrderDescriptorsTooShort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropReason {
    NonRawDecapsulation,
    MoreThanOneLogicalMsdu,
    MissingLastMsdu,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnchainOutcome {
    NotChained,
    Drop(DropReason),
    Assembled { length: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxDescriptor {
    pub more_buffers: u8,
    pub msdu_length: u16,
    pub decap_format: u8,
    pub first_msdu: bool,
    pub last_msdu: bool,
}

/// One WCN3990 64-bit entry in ath10k's TLV in-order RX indication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxInOrderMsduDescriptor {
    pub buffer_iova: u64,
    pub msdu_length: u16,
    pub firmware_descriptor: u8,
    /// Retained verbatim: the monitor-mode path uses this as a fragment marker.
    pub reserved: u8,
}

/// Borrowed, bounded view of a TLV `RX_IN_ORD_PADDR_IND` packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxInOrderPaddrIndication<'a> {
    pub tid: u8,
    pub peer_id: u16,
    pub vdev_id: u8,
    pub offload: bool,
    pub fragmented: bool,
    pub reserved_info: u8,
    pub header_reserved: u8,
    descriptors: &'a [u8],
    count: u16,
    /// Any source-transport tail bytes are exposed rather than interpreted.
    pub trailing: &'a [u8],
}

impl RxInOrderPaddrIndication<'_> {
    pub const fn descriptor_count(self) -> u16 {
        self.count
    }

    /// Decode one entry from the source-sized 12-byte little-endian array.
    pub fn descriptor(self, index: u16) -> Option<RxInOrderMsduDescriptor> {
        if index >= self.count {
            return None;
        }
        let offset = usize::from(index).checked_mul(RX_IN_ORD_64_DESCRIPTOR_BYTES)?;
        let bytes = self
            .descriptors
            .get(offset..offset + RX_IN_ORD_64_DESCRIPTOR_BYTES)?;
        Some(RxInOrderMsduDescriptor {
            buffer_iova: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            msdu_length: u16::from_le_bytes([bytes[8], bytes[9]]),
            firmware_descriptor: bytes[10],
            reserved: bytes[11],
        })
    }
}

/// Parse the pinned ath10k TLV 64-bit in-order RX indication layout.
///
/// The message contains a one-byte HTT type, a seven-byte indication header,
/// then at most 2047 twelve-byte address/length entries. The 2047 bound is the
/// WCN3990 ring fill level. No address is dereferenced or treated as authority.
pub fn parse_in_order_paddr_indication(
    message: &[u8],
) -> Result<RxInOrderPaddrIndication<'_>, Error> {
    let message_type = *message.first().ok_or(Error::InOrderHeaderTooShort)?;
    if message_type != RX_IN_ORD_T2H_MESSAGE_TYPE {
        return Err(Error::InOrderWrongMessageType);
    }
    if message.len() < RX_IN_ORD_FIXED_BYTES {
        return Err(Error::InOrderHeaderTooShort);
    }

    let info = message[1];
    let peer_id = u16::from_le_bytes([message[2], message[3]]);
    let vdev_id = message[4];
    let header_reserved = message[5];
    let count = u16::from_le_bytes([message[6], message[7]]);
    if count > RX_IN_ORD_MAX_DESCRIPTORS {
        return Err(Error::InOrderCountExceedsRing);
    }

    let descriptor_bytes = usize::from(count)
        .checked_mul(RX_IN_ORD_64_DESCRIPTOR_BYTES)
        .ok_or(Error::InOrderDescriptorsTooShort)?;
    let descriptors_end = RX_IN_ORD_FIXED_BYTES
        .checked_add(descriptor_bytes)
        .ok_or(Error::InOrderDescriptorsTooShort)?;
    let descriptors = message
        .get(RX_IN_ORD_FIXED_BYTES..descriptors_end)
        .ok_or(Error::InOrderDescriptorsTooShort)?;

    Ok(RxInOrderPaddrIndication {
        tid: info & 0x1f,
        peer_id,
        vdev_id,
        offload: info & 0x20 != 0,
        fragmented: info & 0x40 != 0,
        reserved_info: info & 0x80,
        header_reserved,
        descriptors,
        count,
        trailing: &message[descriptors_end..],
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::DescriptorTooShort)?;
    let word = bytes.get(offset..end).ok_or(Error::DescriptorTooShort)?;
    Ok(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

impl RxDescriptor {
    /// Decode the WCN3990 `htt_rx_desc_v2` fields consumed by ath10k's
    /// low-latency `ath10k_htt_rx_amsdu_pop()` path.
    pub fn decode(buffer: &[u8]) -> Result<Self, Error> {
        if buffer.len() < RX_DESCRIPTOR_BYTES {
            return Err(Error::DescriptorTooShort);
        }

        let attention = read_u32(buffer, RX_ATTENTION_OFFSET)?;
        if attention & ATTENTION_MSDU_DONE == 0 {
            return Err(Error::NotComplete);
        }
        if attention & (ATTENTION_MSDU_LENGTH_ERROR | ATTENTION_MPDU_LENGTH_ERROR) != 0 {
            return Err(Error::LengthError);
        }

        let more_buffers = *buffer
            .get(RX_FRAG_INFO_OFFSET + 2)
            .ok_or(Error::DescriptorTooShort)?;
        let msdu_start_info0 = read_u32(buffer, RX_MSDU_START_INFO0_OFFSET)?;
        let msdu_start_info1 = read_u32(buffer, RX_MSDU_START_INFO1_OFFSET)?;
        let msdu_end_info0 = read_u32(buffer, RX_MSDU_END_INFO0_OFFSET)?;

        Ok(Self {
            more_buffers,
            msdu_length: (msdu_start_info0 & MSDU_LENGTH_MASK) as u16,
            decap_format: ((msdu_start_info1 & DECAP_FORMAT_MASK) >> DECAP_FORMAT_SHIFT) as u8,
            first_msdu: msdu_end_info0 & FIRST_MSDU != 0,
            last_msdu: msdu_end_info0 & LAST_MSDU != 0,
        })
    }

    /// Borrow the payload for an ordinary, single-buffer WCN3990 MSDU.
    pub fn single_buffer_payload<'a>(self, buffer: &'a [u8]) -> Result<&'a [u8], Error> {
        if self.more_buffers != 0 {
            return Err(Error::ChainedMsdu);
        }
        if buffer.len() < RX_BUFFER_BYTES {
            return Err(Error::BufferTooShort);
        }
        let end = RX_PAYLOAD_OFFSET
            .checked_add(usize::from(self.msdu_length))
            .ok_or(Error::MsduLengthOutOfRange)?;
        if end > RX_BUFFER_BYTES {
            return Err(Error::MsduLengthOutOfRange);
        }
        buffer
            .get(RX_PAYLOAD_OFFSET..end)
            .ok_or(Error::BufferTooShort)
    }
}

/// Apply ath10k's bounded chained-MSDU policy to one completed MPDU queue.
///
/// Upstream ath10k unchains only a single logical MSDU in RAW decapsulation
/// format. It drops chained aggregates with another decapsulation format or
/// with a queue length other than `1 + ring2_more_count`. The first buffer
/// contains the descriptor; continuation buffers contain payload only.
///
/// All counts, bounds, and output capacity are checked before the first copy,
/// so an error leaves `output` unchanged.
pub fn unchain_raw_msdu(
    descriptor: RxDescriptor,
    first_buffer: &[u8],
    continuation_buffers: &[&[u8]],
    queued_buffer_count: usize,
    output: &mut [u8],
) -> Result<UnchainOutcome, Error> {
    if descriptor.more_buffers == 0 {
        return Ok(UnchainOutcome::NotChained);
    }
    if continuation_buffers.len() != usize::from(descriptor.more_buffers) {
        return Err(Error::ContinuationCountMismatch);
    }
    if descriptor.decap_format != 0 {
        return Ok(UnchainOutcome::Drop(DropReason::NonRawDecapsulation));
    }
    let expected_queue_count = 1usize
        .checked_add(usize::from(descriptor.more_buffers))
        .ok_or(Error::ContinuationCountMismatch)?;
    if queued_buffer_count != expected_queue_count {
        return Ok(UnchainOutcome::Drop(DropReason::MoreThanOneLogicalMsdu));
    }
    if !descriptor.last_msdu {
        return Ok(UnchainOutcome::Drop(DropReason::MissingLastMsdu));
    }
    if first_buffer.len() < RX_BUFFER_BYTES
        || continuation_buffers
            .iter()
            .any(|buffer| buffer.len() < RX_BUFFER_BYTES)
    {
        return Err(Error::BufferTooShort);
    }

    let length = usize::from(descriptor.msdu_length);
    if output.len() < length {
        return Err(Error::OutputTooSmall);
    }

    let first_capacity = RX_BUFFER_BYTES - RX_PAYLOAD_OFFSET;
    let mut remaining = length;
    let first_length = remaining.min(first_capacity);
    remaining -= first_length;

    for buffer in continuation_buffers {
        let part_length = remaining.min(RX_BUFFER_BYTES);
        if part_length == 0 {
            return Err(Error::MsduLengthOutOfRange);
        }
        if buffer.len() < part_length {
            return Err(Error::BufferTooShort);
        }
        remaining -= part_length;
    }
    if remaining != 0 {
        return Err(Error::MsduLengthOutOfRange);
    }

    // Every segment must contribute data. The source's queue includes each
    // ring-2 continuation, so empty surplus segments indicate a bad descriptor.
    if first_length == 0 {
        return Err(Error::MsduLengthOutOfRange);
    }

    output[..first_length]
        .copy_from_slice(&first_buffer[RX_PAYLOAD_OFFSET..RX_PAYLOAD_OFFSET + first_length]);
    let mut output_offset = first_length;
    let mut remaining = length - first_length;
    for buffer in continuation_buffers {
        let part_length = remaining.min(RX_BUFFER_BYTES);
        let end = output_offset
            .checked_add(part_length)
            .ok_or(Error::MsduLengthOutOfRange)?;
        output[output_offset..end].copy_from_slice(&buffer[..part_length]);
        output_offset = end;
        remaining -= part_length;
    }

    Ok(UnchainOutcome::Assembled { length })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_wcn3990_low_latency_chain_fields() {
        let mut buffer = vec![0; RX_BUFFER_BYTES];
        buffer[RX_ATTENTION_OFFSET..RX_ATTENTION_OFFSET + 4]
            .copy_from_slice(&ATTENTION_MSDU_DONE.to_le_bytes());
        buffer[RX_FRAG_INFO_OFFSET + 2] = 2;
        buffer[RX_MSDU_START_INFO0_OFFSET..RX_MSDU_START_INFO0_OFFSET + 4]
            .copy_from_slice(&4000u32.to_le_bytes());
        buffer[RX_MSDU_START_INFO1_OFFSET..RX_MSDU_START_INFO1_OFFSET + 4]
            .copy_from_slice(&(1u32 << DECAP_FORMAT_SHIFT).to_le_bytes());
        buffer[RX_MSDU_END_INFO0_OFFSET..RX_MSDU_END_INFO0_OFFSET + 4]
            .copy_from_slice(&(FIRST_MSDU | LAST_MSDU).to_le_bytes());

        assert_eq!(
            RxDescriptor::decode(&buffer),
            Ok(RxDescriptor {
                more_buffers: 2,
                msdu_length: 4000,
                decap_format: 1,
                first_msdu: true,
                last_msdu: true,
            })
        );
    }

    #[test]
    fn borrows_only_bounded_single_buffer_payloads() {
        let mut buffer = vec![0; RX_BUFFER_BYTES];
        buffer[RX_ATTENTION_OFFSET..RX_ATTENTION_OFFSET + 4]
            .copy_from_slice(&ATTENTION_MSDU_DONE.to_le_bytes());
        buffer[RX_MSDU_START_INFO0_OFFSET..RX_MSDU_START_INFO0_OFFSET + 4]
            .copy_from_slice(&8u32.to_le_bytes());
        buffer[RX_PAYLOAD_OFFSET..RX_PAYLOAD_OFFSET + 8].copy_from_slice(b"sunfish!");

        let descriptor = RxDescriptor::decode(&buffer).unwrap();
        assert_eq!(
            descriptor.single_buffer_payload(&buffer),
            Ok(&b"sunfish!"[..])
        );
        assert_eq!(
            descriptor.single_buffer_payload(&buffer[..RX_BUFFER_BYTES - 1]),
            Err(Error::BufferTooShort)
        );
        assert_eq!(
            RxDescriptor {
                more_buffers: 1,
                ..descriptor
            }
            .single_buffer_payload(&buffer),
            Err(Error::ChainedMsdu)
        );

        buffer[RX_MSDU_START_INFO0_OFFSET..RX_MSDU_START_INFO0_OFFSET + 4]
            .copy_from_slice(&1700u32.to_le_bytes());
        let oversized = RxDescriptor::decode(&buffer).unwrap();
        assert_eq!(
            oversized.single_buffer_payload(&buffer),
            Err(Error::MsduLengthOutOfRange)
        );
    }

    #[test]
    fn unchains_raw_msdu_using_descriptor_then_payload_only_fragments() {
        let first = vec![0x11; RX_BUFFER_BYTES];
        let second = vec![0x22; RX_BUFFER_BYTES];
        let third = vec![0x33; RX_BUFFER_BYTES];
        let descriptor = RxDescriptor {
            more_buffers: 2,
            msdu_length: 4000,
            decap_format: 0,
            first_msdu: true,
            last_msdu: true,
        };
        let mut output = vec![0; 4000];
        assert_eq!(
            unchain_raw_msdu(descriptor, &first, &[&second, &third], 3, &mut output,),
            Ok(UnchainOutcome::Assembled { length: 4000 })
        );
        assert!(
            output[..RX_BUFFER_BYTES - RX_PAYLOAD_OFFSET]
                .iter()
                .all(|byte| *byte == 0x11)
        );
        assert!(
            output[RX_BUFFER_BYTES - RX_PAYLOAD_OFFSET..RX_BUFFER_BYTES - RX_PAYLOAD_OFFSET + 2048]
                .iter()
                .all(|byte| *byte == 0x22)
        );
        assert!(
            output[RX_BUFFER_BYTES - RX_PAYLOAD_OFFSET + 2048..]
                .iter()
                .all(|byte| *byte == 0x33)
        );
    }

    #[test]
    fn mirrors_linux_drop_conditions_for_nonraw_or_aggregate_chains() {
        let first = vec![0; RX_BUFFER_BYTES];
        let fragment = vec![0; RX_BUFFER_BYTES];
        let mut output = [0; 8];
        let nonraw = RxDescriptor {
            more_buffers: 1,
            msdu_length: 8,
            decap_format: 1,
            first_msdu: true,
            last_msdu: true,
        };
        assert_eq!(
            unchain_raw_msdu(nonraw, &first, &[&fragment], 2, &mut output),
            Ok(UnchainOutcome::Drop(DropReason::NonRawDecapsulation))
        );

        let raw = RxDescriptor {
            decap_format: 0,
            ..nonraw
        };
        assert_eq!(
            unchain_raw_msdu(raw, &first, &[&fragment], 3, &mut output),
            Ok(UnchainOutcome::Drop(DropReason::MoreThanOneLogicalMsdu))
        );
        assert_eq!(
            unchain_raw_msdu(
                RxDescriptor {
                    last_msdu: false,
                    ..raw
                },
                &first,
                &[&fragment],
                2,
                &mut output,
            ),
            Ok(UnchainOutcome::Drop(DropReason::MissingLastMsdu))
        );
    }

    #[test]
    fn rejects_incomplete_bad_length_and_insufficient_spans_without_partial_copy() {
        let mut first = vec![0x11; RX_BUFFER_BYTES];
        assert_eq!(
            RxDescriptor::decode(&first[..RX_DESCRIPTOR_BYTES - 1]),
            Err(Error::DescriptorTooShort)
        );
        assert_eq!(RxDescriptor::decode(&first), Err(Error::NotComplete));
        first[RX_ATTENTION_OFFSET..RX_ATTENTION_OFFSET + 4]
            .copy_from_slice(&(ATTENTION_MSDU_DONE | ATTENTION_MSDU_LENGTH_ERROR).to_le_bytes());
        assert_eq!(RxDescriptor::decode(&first), Err(Error::LengthError));

        let descriptor = RxDescriptor {
            more_buffers: 1,
            msdu_length: 1800,
            decap_format: 0,
            first_msdu: true,
            last_msdu: true,
        };
        let continuation = vec![0x22; RX_BUFFER_BYTES];
        let mut too_small = [0x5a; 1799];
        assert_eq!(
            unchain_raw_msdu(descriptor, &first, &[&continuation], 2, &mut too_small),
            Err(Error::OutputTooSmall)
        );
        assert!(too_small.iter().all(|byte| *byte == 0x5a));

        let mut malformed = vec![0x5a; 4000];
        assert_eq!(
            unchain_raw_msdu(
                RxDescriptor {
                    msdu_length: 4000,
                    ..descriptor
                },
                &first,
                &[&continuation],
                2,
                &mut malformed,
            ),
            Err(Error::MsduLengthOutOfRange)
        );
        assert!(malformed.iter().all(|byte| *byte == 0x5a));
    }

    #[test]
    fn parses_tlv_in_order_64_bit_addresses_and_control_fields() {
        let mut message = vec![0; RX_IN_ORD_FIXED_BYTES + 2 * RX_IN_ORD_64_DESCRIPTOR_BYTES + 1];
        message[0] = RX_IN_ORD_T2H_MESSAGE_TYPE;
        message[1] = 0xe5; // TID 5, offload, fragmented, reserved high bit
        message[2..4].copy_from_slice(&0x1234u16.to_le_bytes());
        message[4] = 7;
        message[5] = 0xa5;
        message[6..8].copy_from_slice(&2u16.to_le_bytes());
        message[8..16].copy_from_slice(&0x1_2345_6789u64.to_le_bytes());
        message[16..18].copy_from_slice(&1500u16.to_le_bytes());
        message[18] = 0x42;
        message[19] = 0x01;
        message[20..28].copy_from_slice(&0x9_abcdef01u64.to_le_bytes());
        message[28..30].copy_from_slice(&500u16.to_le_bytes());
        message[30] = 0x24;
        message[31] = 0x80;
        message[32] = 0xcc;

        let indication = parse_in_order_paddr_indication(&message).unwrap();
        assert_eq!(indication.tid, 5);
        assert_eq!(indication.peer_id, 0x1234);
        assert_eq!(indication.vdev_id, 7);
        assert!(indication.offload);
        assert!(indication.fragmented);
        assert_eq!(indication.reserved_info, 0x80);
        assert_eq!(indication.header_reserved, 0xa5);
        assert_eq!(indication.descriptor_count(), 2);
        assert_eq!(
            indication.descriptor(0),
            Some(RxInOrderMsduDescriptor {
                buffer_iova: 0x1_2345_6789,
                msdu_length: 1500,
                firmware_descriptor: 0x42,
                reserved: 1,
            })
        );
        assert_eq!(
            indication.descriptor(1),
            Some(RxInOrderMsduDescriptor {
                buffer_iova: 0x9_abcdef01,
                msdu_length: 500,
                firmware_descriptor: 0x24,
                reserved: 0x80,
            })
        );
        assert_eq!(indication.descriptor(2), None);
        assert_eq!(indication.trailing, &[0xcc]);
    }

    #[test]
    fn rejects_wrong_type_truncated_fixed_header_and_oversized_count() {
        assert_eq!(
            parse_in_order_paddr_indication(&[]),
            Err(Error::InOrderHeaderTooShort)
        );
        assert_eq!(
            parse_in_order_paddr_indication(&[0x11; RX_IN_ORD_FIXED_BYTES]),
            Err(Error::InOrderWrongMessageType)
        );
        for length in 1..RX_IN_ORD_FIXED_BYTES {
            let mut message = vec![0; length];
            message[0] = RX_IN_ORD_T2H_MESSAGE_TYPE;
            assert_eq!(
                parse_in_order_paddr_indication(&message),
                Err(Error::InOrderHeaderTooShort)
            );
        }

        let mut message = vec![0; RX_IN_ORD_FIXED_BYTES];
        message[0] = RX_IN_ORD_T2H_MESSAGE_TYPE;
        message[6..8].copy_from_slice(&2048u16.to_le_bytes());
        assert_eq!(
            parse_in_order_paddr_indication(&message),
            Err(Error::InOrderCountExceedsRing)
        );
    }

    #[test]
    fn every_descriptor_truncation_fails_and_zero_count_is_valid() {
        let mut message = vec![0; RX_IN_ORD_FIXED_BYTES + RX_IN_ORD_64_DESCRIPTOR_BYTES];
        message[0] = RX_IN_ORD_T2H_MESSAGE_TYPE;
        assert_eq!(
            parse_in_order_paddr_indication(&message)
                .unwrap()
                .descriptor_count(),
            0
        );

        message[6..8].copy_from_slice(&1u16.to_le_bytes());
        for length in RX_IN_ORD_FIXED_BYTES..RX_IN_ORD_FIXED_BYTES + RX_IN_ORD_64_DESCRIPTOR_BYTES {
            assert_eq!(
                parse_in_order_paddr_indication(&message[..length]),
                Err(Error::InOrderDescriptorsTooShort)
            );
        }
    }
}
