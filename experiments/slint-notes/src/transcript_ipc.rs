//! Bounded worker exchange; the first two words identify the history snapshot.
use crate::photo_mailbox::Mailbox;

pub const MAX_TRANSCRIPT: usize = 1024 * 1024;
pub const HEADER: usize = 16;
pub type TranscriptMailbox = Mailbox<8, { HEADER + MAX_TRANSCRIPT }>;
const _: () = assert!(core::mem::size_of::<TranscriptMailbox>() <= 0x200000);

pub fn request(index: usize) -> Option<[u8; 8]> {
    (index < 16).then(|| (index as u64).to_le_bytes())
}

pub fn parse_request(bytes: &[u8]) -> Option<usize> {
    let index = u64::from_le_bytes(bytes.try_into().ok()?);
    (index < 16).then_some(index as usize)
}

pub fn parse_response(bytes: &[u8]) -> Option<(u64, u64, &str)> {
    let header = bytes.get(..HEADER)?;
    let owner = u64::from_le_bytes(header[..8].try_into().ok()?);
    let revision = u64::from_le_bytes(header[8..].try_into().ok()?);
    let text = core::str::from_utf8(&bytes[HEADER..]).ok()?;
    (owner != 0 && owner != u64::MAX).then_some((owner, revision, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_and_validates_messages() {
        assert_eq!(parse_request(&request(15).unwrap()), Some(15));
        assert!(request(16).is_none());
        assert!(parse_request(&[0; 7]).is_none());
        let mut bytes = [0u8; HEADER + 2];
        bytes[0] = 7;
        bytes[8] = 3;
        bytes[HEADER..].copy_from_slice(b"hi");
        assert_eq!(parse_response(&bytes), Some((7, 3, "hi")));
        bytes[HEADER] = 0xff;
        assert!(parse_response(&bytes).is_none());
    }
}
