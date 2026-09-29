//! Encrypted, variable-length app records occupying one vault block.
//!
//! The small PVLTREC1 format remains the format used by the current runtime.
//! This separate codec is the storage foundation for richer app data; it does
//! not change the cipher suite, key handling, or existing record decoder.

use crate::record_crypto::{NONCE_SIZE, TAG_SIZE};
use crate::vault_crypto::{CryptoProviderError, VaultCryptoProvider};

pub const SLOT_SIZE: usize = 4096;
pub const MAGIC: [u8; 8] = *b"PVLTBLB1";
pub const FORMAT_VERSION: u32 = 1;
pub const COMMIT_SALT: u64 = 0x3c6e_f372_fe94_f82b;
pub const MAX_PAYLOAD_SIZE: usize = SLOT_SIZE - CIPHERTEXT_OFFSET - TAG_SIZE - COMMIT_SIZE;

const VERSION_OFFSET: usize = 8;
const GENERATION_OFFSET: usize = 12;
const LENGTH_OFFSET: usize = 20;
const NONCE_OFFSET: usize = 24;
pub const CIPHERTEXT_OFFSET: usize = 36;
const COMMIT_SIZE: usize = 8;
const AAD_MAGIC: [u8; 8] = *b"PVLTBL1A";
const AAD_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedBlob {
    pub generation: u64,
    pub payload_length: usize,
}

/// Encode a payload into one authenticated 4 KiB slot.
///
/// `committed = false` creates a staged slot that recovery must ignore. The
/// caller commits it only after the staged body has been durably flushed.
pub fn encode(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    generation: u64,
    payload: &[u8],
    committed: bool,
) -> Result<Option<[u8; SLOT_SIZE]>, CryptoProviderError> {
    if payload.len() > MAX_PAYLOAD_SIZE {
        return Ok(None);
    }
    let Ok(namespace) = u32::try_from(namespace) else {
        return Ok(None);
    };
    let Ok(nonce_namespace) = u8::try_from(namespace) else {
        return Ok(None);
    };

    let length = payload.len() as u32;
    let nonce = nonce(nonce_namespace, generation);
    let aad = associated_data(namespace, generation, length);
    let mut slot = [0; SLOT_SIZE];
    slot[..VERSION_OFFSET].copy_from_slice(&MAGIC);
    slot[VERSION_OFFSET..GENERATION_OFFSET].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    slot[GENERATION_OFFSET..LENGTH_OFFSET].copy_from_slice(&generation.to_le_bytes());
    slot[LENGTH_OFFSET..NONCE_OFFSET].copy_from_slice(&length.to_le_bytes());
    slot[NONCE_OFFSET..CIPHERTEXT_OFFSET].copy_from_slice(&nonce);

    let end = CIPHERTEXT_OFFSET + payload.len();
    slot[CIPHERTEXT_OFFSET..end].copy_from_slice(payload);
    let tag = crypto.encrypt(&nonce, &aad, &mut slot[CIPHERTEXT_OFFSET..end])?;
    let tag_end = end + TAG_SIZE;
    slot[end..tag_end].copy_from_slice(&tag);
    let marker = if committed {
        generation ^ COMMIT_SALT
    } else {
        (generation ^ COMMIT_SALT) ^ 1
    };
    slot[tag_end..tag_end + COMMIT_SIZE].copy_from_slice(&marker.to_le_bytes());
    Ok(Some(slot))
}

/// Authenticate and decrypt an occupied slot in place.
///
/// On success, the plaintext payload is at bytes `36..36 + payload_length`;
/// bytes after it retain the encoded header/tag/commit marker and zero padding.
pub fn decode(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    slot: &mut [u8; SLOT_SIZE],
) -> Result<Option<DecodedBlob>, CryptoProviderError> {
    if slot[..VERSION_OFFSET] != MAGIC {
        return Ok(None);
    }
    let version = u32::from_le_bytes(slot[VERSION_OFFSET..GENERATION_OFFSET].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Ok(None);
    }
    let generation = u64::from_le_bytes(slot[GENERATION_OFFSET..LENGTH_OFFSET].try_into().unwrap());
    let length = u32::from_le_bytes(slot[LENGTH_OFFSET..NONCE_OFFSET].try_into().unwrap()) as usize;
    if length > MAX_PAYLOAD_SIZE {
        return Ok(None);
    }
    let Ok(namespace) = u32::try_from(namespace) else {
        return Ok(None);
    };
    let Ok(nonce_namespace) = u8::try_from(namespace) else {
        return Ok(None);
    };

    let end = CIPHERTEXT_OFFSET + length;
    let tag_end = end + TAG_SIZE;
    let marker_end = tag_end + COMMIT_SIZE;
    let marker = u64::from_le_bytes(slot[tag_end..marker_end].try_into().unwrap());
    if marker != generation ^ COMMIT_SALT || slot[marker_end..].iter().any(|byte| *byte != 0) {
        return Ok(None);
    }

    let expected_nonce = nonce(nonce_namespace, generation);
    if slot[NONCE_OFFSET..CIPHERTEXT_OFFSET] != expected_nonce {
        return Ok(None);
    }
    let aad = associated_data(namespace, generation, length as u32);
    let mut tag = [0; TAG_SIZE];
    tag.copy_from_slice(&slot[end..tag_end]);
    if !crypto.decrypt(
        &expected_nonce,
        &aad,
        &mut slot[CIPHERTEXT_OFFSET..end],
        &tag,
    )? {
        return Ok(None);
    }
    Ok(Some(DecodedBlob {
        generation,
        payload_length: length,
    }))
}

/// Replace a staged record's marker after its encrypted body has been flushed.
pub fn set_committed(slot: &mut [u8; SLOT_SIZE]) -> bool {
    if slot[..VERSION_OFFSET] != MAGIC
        || u32::from_le_bytes(slot[VERSION_OFFSET..GENERATION_OFFSET].try_into().unwrap())
            != FORMAT_VERSION
    {
        return false;
    }
    let generation = u64::from_le_bytes(slot[GENERATION_OFFSET..LENGTH_OFFSET].try_into().unwrap());
    let length = u32::from_le_bytes(slot[LENGTH_OFFSET..NONCE_OFFSET].try_into().unwrap()) as usize;
    if length > MAX_PAYLOAD_SIZE {
        return false;
    }
    let marker_offset = CIPHERTEXT_OFFSET + length + TAG_SIZE;
    slot[marker_offset..marker_offset + COMMIT_SIZE]
        .copy_from_slice(&(generation ^ COMMIT_SALT).to_le_bytes());
    true
}

fn nonce(namespace: u8, generation: u64) -> [u8; NONCE_SIZE] {
    let mut result = [0; NONCE_SIZE];
    result[..3].copy_from_slice(b"PB1");
    result[3] = namespace;
    result[4..].copy_from_slice(&generation.to_le_bytes());
    result
}

fn associated_data(namespace: u32, generation: u64, length: u32) -> [u8; AAD_SIZE] {
    let mut result = [0; AAD_SIZE];
    result[..8].copy_from_slice(&AAD_MAGIC);
    result[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    result[12..20].copy_from_slice(&u64::from(namespace).to_le_bytes());
    result[20..28].copy_from_slice(&generation.to_le_bytes());
    result[28..32].copy_from_slice(&length.to_le_bytes());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_crypto::{TestCryptoProvider, UnavailableCryptoProvider};
    use crate::vault_record::TEST_KEY;

    fn crypto() -> TestCryptoProvider {
        TestCryptoProvider::new(TEST_KEY)
    }

    #[test]
    fn encrypted_blob_round_trips_empty_multiline_and_maximum_payloads() {
        for payload in [
            b"".as_slice(),
            b"first line\nsecond line",
            &[0x5a; MAX_PAYLOAD_SIZE],
        ] {
            let slot = encode(&crypto(), 1, 9, payload, true).unwrap().unwrap();
            if payload.len() >= 8 {
                assert!(!slot.windows(8).any(|part| part == &payload[..8]));
            }
            let mut opened = slot;
            let decoded = decode(&crypto(), 1, &mut opened).unwrap().unwrap();
            assert_eq!(decoded.generation, 9);
            assert_eq!(decoded.payload_length, payload.len());
            assert_eq!(
                &opened[CIPHERTEXT_OFFSET..CIPHERTEXT_OFFSET + payload.len()],
                payload
            );
        }
    }

    #[test]
    fn blob_binds_namespace_generation_length_and_ciphertext() {
        let slot = encode(&crypto(), 1, 9, b"private note", true)
            .unwrap()
            .unwrap();

        let mut wrong_namespace = slot;
        assert_eq!(decode(&crypto(), 2, &mut wrong_namespace), Ok(None));

        let mut wrong_generation = slot;
        wrong_generation[GENERATION_OFFSET] ^= 1;
        assert_eq!(decode(&crypto(), 1, &mut wrong_generation), Ok(None));

        let mut wrong_length = slot;
        wrong_length[LENGTH_OFFSET] ^= 1;
        assert_eq!(decode(&crypto(), 1, &mut wrong_length), Ok(None));

        let mut changed_ciphertext = slot;
        changed_ciphertext[CIPHERTEXT_OFFSET] ^= 1;
        assert_eq!(decode(&crypto(), 1, &mut changed_ciphertext), Ok(None));

        let mut changed_tag = slot;
        let tag_start = CIPHERTEXT_OFFSET + b"private note".len();
        changed_tag[tag_start] ^= 1;
        assert_eq!(decode(&crypto(), 1, &mut changed_tag), Ok(None));

        let mut changed_commit = slot;
        let tag_end = CIPHERTEXT_OFFSET + b"private note".len() + TAG_SIZE;
        changed_commit[tag_end] ^= 1;
        assert_eq!(decode(&crypto(), 1, &mut changed_commit), Ok(None));

        let mut dirty_padding = slot;
        dirty_padding[SLOT_SIZE - 1] = 1;
        assert_eq!(decode(&crypto(), 1, &mut dirty_padding), Ok(None));
    }

    #[test]
    fn staged_and_oversized_records_are_not_accepted_or_written() {
        let staged = encode(&crypto(), 1, 9, b"staged", false).unwrap().unwrap();
        let mut staged = staged;
        assert_eq!(decode(&crypto(), 1, &mut staged), Ok(None));

        let zero_commit_marker = encode(&crypto(), 1, COMMIT_SALT, b"staged", false)
            .unwrap()
            .unwrap();
        let mut zero_commit_marker = zero_commit_marker;
        assert_eq!(decode(&crypto(), 1, &mut zero_commit_marker), Ok(None));

        let mut committed = encode(&crypto(), 1, 9, b"staged", false).unwrap().unwrap();
        assert!(set_committed(&mut committed));
        assert_eq!(
            decode(&crypto(), 1, &mut committed)
                .unwrap()
                .unwrap()
                .generation,
            9
        );
        assert!(
            encode(&crypto(), 1, 9, &[0; MAX_PAYLOAD_SIZE + 1], true)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            encode(&UnavailableCryptoProvider, 1, 9, b"note", true),
            Err(CryptoProviderError::Unavailable)
        );
    }
}
