//! The UI requests one owner-scoped photo; the reader returns its bytes.
use crate::photo_mailbox::Mailbox;
pub const REQUEST_SIZE: usize = 16;
pub type PhotoReadMailbox = Mailbox<REQUEST_SIZE, { crate::photo_read::MAX_PHOTO }>;
const _: () = assert!(core::mem::size_of::<PhotoReadMailbox>() <= 0x200000);

pub fn request(owner: u64, id: u64) -> [u8; REQUEST_SIZE] {
    let mut bytes = [0; REQUEST_SIZE];
    bytes[..8].copy_from_slice(&owner.to_le_bytes());
    bytes[8..].copy_from_slice(&id.to_le_bytes());
    bytes
}

pub fn parse_request(bytes: &[u8]) -> Option<(u64, u64)> {
    if bytes.len() != REQUEST_SIZE {
        return None;
    }
    let owner = u64::from_le_bytes(bytes[..8].try_into().ok()?);
    let id = u64::from_le_bytes(bytes[8..].try_into().ok()?);
    (owner != 0 && owner != u64::MAX && id != 0).then_some((owner, id))
}
