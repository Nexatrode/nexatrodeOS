//! Framing for bounded, authenticated history reads.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub owner: u64,
    pub event: Option<&'a [u8]>,
}
pub const MAX_ENTRIES: usize = 32;
const HEADER: usize = 10;

pub fn scan_step_end(scanned: usize, end: usize, budget: usize) -> Option<usize> {
    if scanned > end || budget == 0 { return None; }
    Some(scanned.saturating_add(budget).min(end))
}
pub struct Writer<'a> {
    output: &'a mut [u8],
    used: usize,
    count: usize,
}
impl<'a> Writer<'a> {
    pub fn new(output: &'a mut [u8]) -> Self {
        Self {
            output,
            used: 0,
            count: 0,
        }
    }
    pub fn push(&mut self, owner: u64, event: Option<&[u8]>) -> bool {
        let data = match (owner, event) {
            (0, None) => &[][..],
            (owner, Some(data))
                if owner != 0 && !data.is_empty() && data.len() <= u16::MAX as usize =>
            {
                data
            }
            _ => return false,
        };
        let Some(end) = self
            .used
            .checked_add(HEADER)
            .and_then(|x| x.checked_add(data.len()))
        else {
            return false;
        };
        if self.count >= MAX_ENTRIES || end > self.output.len() {
            return false;
        }
        self.output[self.used..self.used + 8].copy_from_slice(&owner.to_le_bytes());
        self.output[self.used + 8..self.used + 10]
            .copy_from_slice(&(data.len() as u16).to_le_bytes());
        self.output[self.used + HEADER..end].copy_from_slice(data);
        self.used = end;
        self.count += 1;
        true
    }
    pub fn bytes(&self) -> &[u8] {
        &self.output[..self.used]
    }
    pub fn count(&self) -> usize {
        self.count
    }
}
pub fn decode(bytes: &[u8], count: usize) -> Option<impl Iterator<Item = Entry<'_>>> {
    if count > MAX_ENTRIES {
        return None;
    }
    let mut remaining = bytes;
    for _ in 0..count {
        let header = remaining.get(..HEADER)?;
        let owner = u64::from_le_bytes(header[..8].try_into().ok()?);
        let len = u16::from_le_bytes(header[8..10].try_into().ok()?) as usize;
        if (owner == 0) != (len == 0) {
            return None;
        }
        remaining = remaining.get(HEADER.checked_add(len)?..)?;
    }
    if !remaining.is_empty() {
        return None;
    }
    let mut input = bytes;
    Some(core::iter::from_fn(move || {
        if input.is_empty() {
            return None;
        }
        let owner = u64::from_le_bytes(input[..8].try_into().ok()?);
        let len = u16::from_le_bytes(input[8..10].try_into().ok()?) as usize;
        let event = (len != 0).then(|| &input[HEADER..HEADER + len]);
        input = &input[HEADER + len..];
        Some(Entry { owner, event })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_preserves_order_owners_and_reserved_gaps() {
        let mut buffer = [0u8; 100];
        let mut writer = Writer::new(&mut buffer);
        assert!(writer.push(10, Some(b"first")));
        assert!(writer.push(0, None));
        assert!(writer.push(20, Some(b"second")));
        let entries: Vec<_> = decode(writer.bytes(), writer.count()).unwrap().collect();
        assert_eq!(
            entries,
            vec![
                Entry {
                    owner: 10,
                    event: Some(b"first")
                },
                Entry {
                    owner: 0,
                    event: None
                },
                Entry {
                    owner: 20,
                    event: Some(b"second")
                }
            ]
        );
    }
    #[test]
    fn full_buffer_rejects_an_entry_without_losing_prior_records() {
        let mut buffer = [0u8; 15];
        let mut writer = Writer::new(&mut buffer);
        assert!(writer.push(1, Some(b"abc")));
        assert!(!writer.push(2, Some(b"d")));
        assert_eq!(writer.count(), 1);
        assert_eq!(decode(writer.bytes(), 1).unwrap().count(), 1);
    }
    #[test]
    fn malformed_or_ambiguous_frames_fail_closed() {
        assert!(decode(&[0; 9], 1).is_none());
        assert!(decode(&[0; 10], 1).unwrap().next().unwrap().event.is_none());
        assert!(decode(&[0; 10], 0).is_none());
        let mut bytes = [0u8; 11];
        bytes[8] = 1;
        bytes[10] = b'x';
        assert!(decode(&bytes, 1).is_none());
        bytes[0] = 1;
        assert!(decode(&bytes[..10], 1).is_none());
        assert!(decode(&bytes, 2).is_none());
    }
    #[test]
    fn rejects_invalid_owner_and_empty_event() {
        let mut buffer = [0u8; 40];
        let mut writer = Writer::new(&mut buffer);
        assert!(!writer.push(0, Some(b"x")));
        assert!(!writer.push(1, None));
        assert!(!writer.push(1, Some(b"")));
        assert_eq!(writer.count(), 0);
    }
    #[test]
    fn count_is_bounded_even_when_buffer_has_room() {
        let mut buffer = [0u8; 512];
        let mut writer = Writer::new(&mut buffer);
        for _ in 0..MAX_ENTRIES {
            assert!(writer.push(0, None));
        }
        assert!(!writer.push(0, None));
        assert_eq!(
            decode(writer.bytes(), MAX_ENTRIES).unwrap().count(),
            MAX_ENTRIES
        );
        assert!(decode(writer.bytes(), MAX_ENTRIES + 1).is_none());
    }
    #[test]
    fn scan_steps_are_bounded_and_never_pass_the_snapshot() {
        assert_eq!(super::scan_step_end(0, 30, 4), Some(4));
        assert_eq!(super::scan_step_end(28, 30, 4), Some(30));
        assert_eq!(super::scan_step_end(30, 30, 4), Some(30));
        assert_eq!(super::scan_step_end(31, 30, 4), None);
        assert_eq!(super::scan_step_end(0, 30, 0), None);
        assert_eq!(super::scan_step_end(usize::MAX - 1, usize::MAX, 4), Some(usize::MAX));
    }
}
