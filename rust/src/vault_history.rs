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
    fn persisted_peer_revision_and_utf8_fragments_reopen_after_restart() {
        // Peer row, source 9, original agent message 2 (part 1), edited by
        // message 4 (part 0), live, shared ID "xyz", one tail record at 3.
        let mut head = vec![5];
        head.extend_from_slice(&9_u64.to_le_bytes());
        head.extend_from_slice(&2_u64.to_le_bytes());
        head.extend_from_slice(&1_u16.to_le_bytes());
        head.extend_from_slice(&4_u64.to_le_bytes());
        head.extend_from_slice(&0_u16.to_le_bytes());
        head.extend_from_slice(&[0, 3, 1]);
        head.extend_from_slice(b"xyz");
        head.extend_from_slice(&3_u16.to_le_bytes());
        head.extend_from_slice("Peer: こんにちは".as_bytes());
        assert!(decode_event(&head).is_some(), "peer identity and revision were lost on reopen");
        let tail = [b"\x04".as_slice(), "🍎".repeat(750).as_bytes()].concat();
        assert!(decode_event(&tail).is_some(), "long-row text was lost on reopen");
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
    Fragment(&'a str),
    Peer(PeerRow<'a>),
    Migrated,
    Cleared,
}

/// A direct text row becomes visible only when this head record commits. Its
/// immutable tail fragments can safely be left unreferenced by a power loss.
pub const DIRECT_TEXT_LIMIT: usize = 65_536;
pub const DIRECT_LINE_LIMIT: usize = 3 * DIRECT_TEXT_LIMIT + 512;
const FRAGMENT_SIZE: usize = 3000;
const MAX_FRAGMENTS: usize = DIRECT_LINE_LIMIT / (FRAGMENT_SIZE - 3) + 1;

#[derive(Clone, Copy)]
pub struct PeerVersion<'a> {
    pub key: MessageKey,
    pub shared_id: &'a str,
    pub revision: (u64, u16),
    pub deleted: bool,
}

impl PeerVersion<'_> {
    pub fn accepts(&self, delivery: MessageKey, id: &str) -> bool {
        self.key.source == delivery.source && !id.is_empty() && self.shared_id == id
            && !self.deleted && (delivery.number, delivery.part) > self.revision
    }
}

pub struct PeerRow<'a> {
    pub version: PeerVersion<'a>,
    pub first: &'a str,
    fragments: &'a [u8],
}

impl PeerRow<'_> {
    pub fn fragment_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.fragments.chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]) as u64)
    }
}

/// Persist all fragments before the single row commit. The writer returns
/// the actual vault record ID, including any gaps from interrupted writes.
pub fn store_peer_line(
    version: PeerVersion<'_>, line: &str, mut append: impl FnMut(&[u8]) -> Result<u64, ()>,
) -> Result<(), ()> {
    if line.is_empty() || line.len() > DIRECT_LINE_LIMIT || line.contains('\n')
        || !valid_shared_id(version.shared_id) {
        return Err(());
    }
    let split = |text: &str| {
        let mut at = FRAGMENT_SIZE.min(text.len());
        while !text.is_char_boundary(at) { at -= 1; }
        at
    };
    let (first, mut tail) = line.split_at(split(line));
    let mut ids = [0u16; MAX_FRAGMENTS];
    let mut count = 0;
    let mut buf = [0; MAX_EVENT_SIZE];
    while !tail.is_empty() {
        let (fragment, rest) = tail.split_at(split(tail));
        buf[0] = 4;
        buf[1..1 + fragment.len()].copy_from_slice(fragment.as_bytes());
        ids[count] = u16::try_from(append(&buf[..1 + fragment.len()])?).map_err(|_| ())?;
        count += 1;
        tail = rest;
    }
    buf[0] = 5;
    buf[1..9].copy_from_slice(&version.key.source.to_le_bytes());
    buf[9..17].copy_from_slice(&version.key.number.to_le_bytes());
    buf[17..19].copy_from_slice(&version.key.part.to_le_bytes());
    buf[19..27].copy_from_slice(&version.revision.0.to_le_bytes());
    buf[27..29].copy_from_slice(&version.revision.1.to_le_bytes());
    buf[29] = u8::from(version.deleted);
    buf[30] = version.shared_id.len() as u8;
    buf[31] = count as u8;
    let mut pos = 32 + version.shared_id.len();
    buf[32..pos].copy_from_slice(version.shared_id.as_bytes());
    for id in &ids[..count] {
        buf[pos..pos + 2].copy_from_slice(&id.to_le_bytes());
        pos += 2;
    }
    buf[pos..pos + first.len()].copy_from_slice(first.as_bytes());
    append(&buf[..pos + first.len()])?;
    Ok(())
}

pub fn valid_shared_id(id: &str) -> bool {
    id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=_-".contains(&b))
}

fn decode_peer(bytes: &[u8]) -> Option<PeerRow<'_>> {
    let header = bytes.get(..32)?;
    let key = MessageKey {
        source: u64::from_le_bytes(header[1..9].try_into().ok()?),
        number: u64::from_le_bytes(header[9..17].try_into().ok()?),
        part: u16::from_le_bytes(header[17..19].try_into().ok()?),
    };
    let revision = (u64::from_le_bytes(header[19..27].try_into().ok()?),
        u16::from_le_bytes(header[27..29].try_into().ok()?));
    let deleted = match header[29] { 0 => false, 1 => true, _ => return None };
    let (id, rest) = bytes[32..].split_at_checked(header[30] as usize)?;
    let shared_id = core::str::from_utf8(id).ok()?;
    let (fragments, first) = rest.split_at_checked(2 * header[31] as usize)?;
    let first = core::str::from_utf8(first).ok()?;
    if key.source == 0 || key.number == 0 || revision < (key.number, key.part)
        || !valid_shared_id(shared_id) || first.is_empty() || first.contains('\n')
        || first.len() > FRAGMENT_SIZE || header[31] as usize > MAX_FRAGMENTS {
        return None;
    }
    Some(PeerRow { version: PeerVersion { key, shared_id, revision, deleted }, first, fragments })
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
        [4, rest @ ..] => {
            let text = core::str::from_utf8(rest).ok()?;
            (!text.is_empty() && text.len() <= FRAGMENT_SIZE && !text.contains('\n')).then_some(Event::Fragment(text))
        }
        [5, ..] => decode_peer(bytes).map(Event::Peer),
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
