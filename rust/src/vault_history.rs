//! Append-only chat history grows from the end of the vault partition toward
//! the existing photo extent. Both allocators check the other high-water mark.

pub const RECORD_NAMESPACE: usize = 0x40;
pub const SEQUENCE_NAMESPACE: usize = 0x41;
pub const OWNER_SIZE: usize = core::mem::size_of::<u64>();
pub const MAX_EVENT_SIZE: usize = crate::vault_blob_record::MAX_PAYLOAD_SIZE - OWNER_SIZE;
/// Bound the in-memory index; usable records are additionally limited by the
/// partition capacity and the media extent. Existing partitions retain their
/// original size because history addresses are relative to their end.
pub const MAX_INDEXED_RECORDS: usize = 16_384;

pub fn sequence_block(partition_blocks: usize, slot: usize) -> Option<usize> {
    (slot < 2).then_some(())?;
    partition_blocks.checked_sub(2 - slot)
}

/// IDs start at one. `photo_end` is the first block after reserved photos.
pub fn record_block(partition_blocks: usize, id: u64, photo_end: usize) -> Option<usize> {
    let id = usize::try_from(id).ok()?;
    if id == 0 {
        return None;
    }
    let block = sequence_block(partition_blocks, 0)?.checked_sub(id)?;
    (block >= photo_end).then_some(block)
}

/// Exclusive end of the media extent when history already owns `last_id`.
pub fn photo_limit(partition_blocks: usize, last_id: u64) -> Option<usize> {
    sequence_block(partition_blocks, 0)?.checked_sub(usize::try_from(last_id).ok()?)
}

pub fn split_event(payload: &[u8]) -> Option<(u64, &[u8])> {
    let (owner, event) = payload.split_at_checked(OWNER_SIZE)?;
    let owner = u64::from_le_bytes(owner.try_into().ok()?);
    (owner != 0 && !event.is_empty()).then_some((owner, event))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growing_history_and_photos_never_claim_the_same_block() {
        assert_eq!(record_block(100, 1, 95), Some(97));
        assert_eq!(record_block(100, 3, 95), Some(95));
        assert_eq!(record_block(100, 4, 95), None);
        assert_eq!(photo_limit(100, 3), Some(95));
        assert_eq!(photo_limit(100, 0), Some(98));
    }

    #[test]
    fn larger_new_partition_supports_more_than_the_old_index_limit() {
        // The 64 MiB UTM GPT image has a 62 MiB data partition.
        let blocks = 15_872;
        let media_end = 2_100;
        assert!(MAX_INDEXED_RECORDS > 4_096);
        assert_eq!(record_block(blocks, 4_097, media_end), Some(11_773));
        let capacity = (blocks - 2 - media_end) as u64;
        assert_eq!(record_block(blocks, capacity, media_end), Some(media_end));
        assert_eq!(record_block(blocks, capacity + 1, media_end), None);
        assert_eq!(photo_limit(blocks, capacity), Some(media_end));
    }

    #[test]
    fn growing_an_existing_partition_moves_its_tail_records() {
        // The current format anchors history at the end of the partition.
        // A larger replacement image must migrate these records explicitly;
        // simply extending an existing disk would make them unreachable.
        assert_eq!(sequence_block(4096, 0), Some(4094));
        assert_eq!(record_block(4096, 1, 2100), Some(4093));
        assert_eq!(sequence_block(15_872, 0), Some(15_870));
        assert_eq!(record_block(15_872, 1, 2100), Some(15_869));
    }

    #[test]
    fn invalid_ids_and_small_partitions_fail_closed() {
        assert_eq!(record_block(100, 0, 0), None);
        assert_eq!(record_block(100, u64::MAX, 0), None);
        assert_eq!(record_block(2, 1, 0), None);
        assert_eq!(photo_limit(1, 0), None);
    }

    #[test]
    fn row_round_trip_preserves_batch_identity_and_body() {
        let key = MessageKey {
            source: 9,
            number: 73,
            part: 1,
        };
        let line = "MacHost: batch part two ✓";
        let mut bytes = [0; 128];
        let len = encode_row(key, line, &mut bytes).unwrap();
        match decode_event(&bytes[..len]) {
            Some(Event::Row(restored, text)) => {
                assert_eq!(restored, key);
                assert_eq!(text, line);
            }
            _ => panic!("expected a history row"),
        }
        assert!(encode_row(key, "two\nrows", &mut bytes).is_none());
    }

    #[test]
    fn event_carries_a_nonzero_chat_owner() {
        let mut bytes = [0; OWNER_SIZE + 2];
        assert_eq!(split_event(&bytes), None);
        bytes[..OWNER_SIZE].copy_from_slice(&13u64.to_le_bytes());
        bytes[OWNER_SIZE..].copy_from_slice(b"hi");
        assert_eq!(split_event(&bytes), Some((13, &b"hi"[..])));
        assert_eq!(split_event(&bytes[..OWNER_SIZE]), None);
    }
}

/// The owner is stored by the vault. This identity distinguishes an agent
/// delivery from an imported row in that same chat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageKey {
    pub source: u64,
    pub number: u64,
    pub part: u16,
}

impl MessageKey {
    pub const LOCAL: Self = Self {
        source: 0,
        number: 0,
        part: 0,
    };
}

pub const ROW_HEADER: usize = 19;
pub const MIGRATED: &[u8] = b"\x02";
pub const CLEARED: &[u8] = b"\x03";

pub enum Event<'a> {
    Row(MessageKey, &'a str),
    Migrated,
    Cleared,
}

pub fn encode_row(key: MessageKey, line: &str, out: &mut [u8]) -> Option<usize> {
    let len = ROW_HEADER.checked_add(line.len())?;
    if line.is_empty() || line.contains('\n') || len > MAX_EVENT_SIZE {
        return None;
    }
    let out = out.get_mut(..len)?;
    out[0] = 1;
    out[1..9].copy_from_slice(&key.source.to_le_bytes());
    out[9..17].copy_from_slice(&key.number.to_le_bytes());
    out[17..19].copy_from_slice(&key.part.to_le_bytes());
    out[ROW_HEADER..].copy_from_slice(line.as_bytes());
    Some(len)
}

pub fn decode_event(bytes: &[u8]) -> Option<Event<'_>> {
    match bytes {
        MIGRATED => Some(Event::Migrated),
        CLEARED => Some(Event::Cleared),
        [1, ..] => {
            let line = core::str::from_utf8(bytes.get(ROW_HEADER..)?).ok()?;
            if line.is_empty() || line.contains('\n') {
                return None;
            }
            let source = u64::from_le_bytes(bytes.get(1..9)?.try_into().ok()?);
            let number = u64::from_le_bytes(bytes.get(9..17)?.try_into().ok()?);
            let part = u16::from_le_bytes(bytes.get(17..19)?.try_into().ok()?);
            Some(Event::Row(
                MessageKey {
                    source,
                    number,
                    part,
                },
                line,
            ))
        }
        _ => None,
    }
}
