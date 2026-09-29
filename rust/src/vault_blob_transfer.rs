//! Shared one-page transfer area for a single app and the vault service.

use crate::vault_blob_record::MAX_PAYLOAD_SIZE;

pub const PAGE_SIZE: usize = 4096;

/// The system description maps one instance to its owning app and to the
/// vault. Sibling apps have distinct pages and cannot inspect each other's
/// staged or decrypted payloads.
#[repr(C)]
pub struct BlobTransferPage {
    pub length: u32,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl BlobTransferPage {
    pub const fn new() -> Self {
        Self {
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn set_payload(&mut self, payload: &[u8]) -> bool {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return false;
        }
        self.payload[..payload.len()].copy_from_slice(payload);
        self.payload[payload.len()..].fill(0);
        self.length = payload.len() as u32;
        true
    }

    pub fn payload(&self) -> Option<&[u8]> {
        let length = self.length as usize;
        if length <= MAX_PAYLOAD_SIZE {
            Some(&self.payload[..length])
        } else {
            None
        }
    }
}

const _: () = assert!(core::mem::size_of::<BlobTransferPage>() <= PAGE_SIZE);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_page_bounds_lengths_and_clears_stale_bytes() {
        let mut page = BlobTransferPage::new();
        assert!(page.set_payload(b"first\nsecond"));
        assert_eq!(page.payload(), Some(b"first\nsecond".as_slice()));
        assert_eq!(page.payload[12], 0);

        page.length = (MAX_PAYLOAD_SIZE + 1) as u32;
        assert_eq!(page.payload(), None);
        assert!(!page.set_payload(&[0; MAX_PAYLOAD_SIZE + 1]));
    }
}
