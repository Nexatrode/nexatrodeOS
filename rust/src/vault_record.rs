//! On-disk encoding for the small encrypted records used by the vault.

use crate::record_crypto::{NONCE_SIZE, TAG_SIZE};
use crate::vault_crypto::{CryptoProviderError, VaultCryptoProvider};

pub const SLOT_SIZE: usize = 72;
pub const MAGIC: [u8; 8] = *b"PVLTREC1";
pub const FORMAT_VERSION: u32 = 1;
pub const COMMIT_SALT: u64 = 0x6a09_e667_f3bc_c909;
const LEGACY_COMMIT_SALT: u64 = COMMIT_SALT;
const LEGACY_CHECKSUM_SALT: u64 = 0xbb67_ae85_84ca_a73b;
const AAD_MAGIC: [u8; 8] = *b"PVLTAD01";
const AAD_SIZE: usize = 28;

const MAGIC_OFFSET: usize = 0;
const VERSION_OFFSET: usize = 8;
const GENERATION_OFFSET: usize = 12;
const NONCE_OFFSET: usize = 20;
const CIPHERTEXT_OFFSET: usize = 32;
const TAG_OFFSET: usize = 40;
const COMMIT_OFFSET: usize = 56;
const RESERVED_OFFSET: usize = 64;
const LEGACY_SIZE: usize = 32;

#[cfg(test)]
pub(crate) const TEST_KEY: [u8; crate::record_crypto::KEY_SIZE] =
    [0x42; crate::record_crypto::KEY_SIZE];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordFormat {
    Encrypted,
    LegacyPlaintext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedRecord {
    pub generation: u64,
    pub value: u64,
    pub format: RecordFormat,
}

pub fn encode(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    generation: u64,
    value: u64,
    committed: bool,
) -> Result<Option<[u8; SLOT_SIZE]>, CryptoProviderError> {
    let Ok(namespace) = u32::try_from(namespace) else {
        return Ok(None);
    };
    let Ok(nonce_namespace) = u8::try_from(namespace) else {
        return Ok(None);
    };
    let nonce = nonce(nonce_namespace, generation);
    let aad = associated_data(namespace, generation);
    let mut slot = [0; SLOT_SIZE];
    slot[MAGIC_OFFSET..VERSION_OFFSET].copy_from_slice(&MAGIC);
    slot[VERSION_OFFSET..GENERATION_OFFSET].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    slot[GENERATION_OFFSET..NONCE_OFFSET].copy_from_slice(&generation.to_le_bytes());
    slot[NONCE_OFFSET..CIPHERTEXT_OFFSET].copy_from_slice(&nonce);
    slot[CIPHERTEXT_OFFSET..CIPHERTEXT_OFFSET + 8].copy_from_slice(&value.to_le_bytes());
    let ciphertext = &mut slot[CIPHERTEXT_OFFSET..CIPHERTEXT_OFFSET + 8];
    let ciphertext: &mut [u8; 8] = ciphertext.try_into().expect("fixed-size record field");
    let tag = crypto.encrypt(&nonce, &aad, ciphertext)?;
    slot[TAG_OFFSET..COMMIT_OFFSET].copy_from_slice(&tag);
    // Keep an uncommitted marker distinct from the committed marker for every
    // generation, including the (rare) generation whose committed marker is 0.
    let marker = if committed {
        generation ^ COMMIT_SALT
    } else {
        (generation ^ COMMIT_SALT) ^ 1
    };
    slot[COMMIT_OFFSET..RESERVED_OFFSET].copy_from_slice(&marker.to_le_bytes());
    Ok(Some(slot))
}

pub fn decode(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    slot: &[u8; SLOT_SIZE],
) -> Result<Option<DecodedRecord>, CryptoProviderError> {
    if slot[MAGIC_OFFSET..VERSION_OFFSET] == MAGIC {
        decode_encrypted(crypto, namespace, slot)
    } else {
        Ok(decode_legacy(slot))
    }
}

fn decode_encrypted(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    slot: &[u8; SLOT_SIZE],
) -> Result<Option<DecodedRecord>, CryptoProviderError> {
    let version = u32::from_le_bytes(
        slot[VERSION_OFFSET..GENERATION_OFFSET]
            .try_into()
            .expect("fixed-size record field"),
    );
    if version != FORMAT_VERSION || slot[RESERVED_OFFSET..].iter().any(|byte| *byte != 0) {
        return Ok(None);
    }
    let generation = u64::from_le_bytes(
        slot[GENERATION_OFFSET..NONCE_OFFSET]
            .try_into()
            .expect("fixed-size record field"),
    );
    let marker = u64::from_le_bytes(
        slot[COMMIT_OFFSET..RESERVED_OFFSET]
            .try_into()
            .expect("fixed-size record field"),
    );
    if marker != generation ^ COMMIT_SALT {
        return Ok(None);
    }

    let Ok(namespace) = u32::try_from(namespace) else {
        return Ok(None);
    };
    let Ok(nonce_namespace) = u8::try_from(namespace) else {
        return Ok(None);
    };
    let expected_nonce = nonce(nonce_namespace, generation);
    if slot[NONCE_OFFSET..CIPHERTEXT_OFFSET] != expected_nonce {
        return Ok(None);
    }
    let aad = associated_data(namespace, generation);
    let mut ciphertext = [0; 8];
    ciphertext.copy_from_slice(&slot[CIPHERTEXT_OFFSET..TAG_OFFSET]);
    let mut tag = [0; TAG_SIZE];
    tag.copy_from_slice(&slot[TAG_OFFSET..COMMIT_OFFSET]);
    if !crypto.decrypt(&expected_nonce, &aad, &mut ciphertext, &tag)? {
        return Ok(None);
    }
    Ok(Some(DecodedRecord {
        generation,
        value: u64::from_le_bytes(ciphertext),
        format: RecordFormat::Encrypted,
    }))
}

fn decode_legacy(slot: &[u8; SLOT_SIZE]) -> Option<DecodedRecord> {
    let generation = u64::from_le_bytes(slot[0..8].try_into().ok()?);
    let value = u64::from_le_bytes(slot[8..16].try_into().ok()?);
    let checksum = u64::from_le_bytes(slot[16..24].try_into().ok()?);
    let committed = u64::from_le_bytes(slot[24..LEGACY_SIZE].try_into().ok()?);
    if committed != (generation ^ LEGACY_COMMIT_SALT)
        || checksum != legacy_checksum(generation, value)
        || slot[LEGACY_SIZE..].iter().any(|byte| *byte != 0)
    {
        return None;
    }
    Some(DecodedRecord {
        generation,
        value,
        format: RecordFormat::LegacyPlaintext,
    })
}

fn nonce(namespace: u8, generation: u64) -> [u8; NONCE_SIZE] {
    let mut result = [0; NONCE_SIZE];
    result[..3].copy_from_slice(b"PV1");
    result[3] = namespace;
    result[4..].copy_from_slice(&generation.to_le_bytes());
    result
}

fn associated_data(namespace: u32, generation: u64) -> [u8; AAD_SIZE] {
    let mut result = [0; AAD_SIZE];
    result[..8].copy_from_slice(&AAD_MAGIC);
    result[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    result[12..20].copy_from_slice(&u64::from(namespace).to_le_bytes());
    result[20..28].copy_from_slice(&generation.to_le_bytes());
    result
}

fn legacy_checksum(generation: u64, value: u64) -> u64 {
    generation.rotate_left(17) ^ value.rotate_right(9) ^ LEGACY_CHECKSUM_SALT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_crypto::{TestCryptoProvider, UnavailableCryptoProvider};

    const TEST_VALUE: u64 = 0x4e4f_5445;

    fn crypto() -> TestCryptoProvider {
        TestCryptoProvider::new(TEST_KEY)
    }

    #[test]
    fn encrypted_slot_round_trips_and_hides_plaintext() {
        let crypto = crypto();
        let slot = encode(&crypto, 1, 42, TEST_VALUE, true).unwrap().unwrap();
        assert_eq!(slot.len(), SLOT_SIZE);
        assert!(
            !slot
                .windows(8)
                .any(|window| window == TEST_VALUE.to_le_bytes())
        );
        assert_eq!(
            decode(&crypto, 1, &slot),
            Ok(Some(DecodedRecord {
                generation: 42,
                value: TEST_VALUE,
                format: RecordFormat::Encrypted,
            }))
        );
    }

    #[test]
    fn encrypted_slot_rejects_tampering_and_wrong_namespace() {
        let crypto = crypto();
        let mut slot = encode(&crypto, 1, 42, TEST_VALUE, true).unwrap().unwrap();
        assert_eq!(decode(&crypto, 2, &slot), Ok(None));
        slot[TAG_OFFSET] ^= 1;
        assert_eq!(decode(&crypto, 1, &slot), Ok(None));
    }

    #[test]
    fn staged_and_uncommitted_slots_are_not_readable() {
        let crypto = crypto();
        let staged = encode(&crypto, 1, 42, TEST_VALUE, false).unwrap().unwrap();
        assert_eq!(decode(&crypto, 1, &staged), Ok(None));

        let zero_commit_marker = encode(&crypto, 1, COMMIT_SALT, TEST_VALUE, false)
            .unwrap()
            .unwrap();
        assert_eq!(decode(&crypto, 1, &zero_commit_marker), Ok(None));
    }

    #[test]
    fn legacy_record_is_recognized_for_one_time_migration() {
        let generation = 42u64;
        let value = TEST_VALUE;
        let mut legacy = [0; SLOT_SIZE];
        legacy[0..8].copy_from_slice(&generation.to_le_bytes());
        legacy[8..16].copy_from_slice(&value.to_le_bytes());
        legacy[16..24].copy_from_slice(&legacy_checksum(generation, value).to_le_bytes());
        legacy[24..32].copy_from_slice(&(generation ^ LEGACY_COMMIT_SALT).to_le_bytes());
        assert_eq!(
            decode(&crypto(), 1, &legacy),
            Ok(Some(DecodedRecord {
                generation,
                value,
                format: RecordFormat::LegacyPlaintext,
            }))
        );
    }

    #[test]
    fn provider_failure_stays_distinct_from_invalid_record() {
        let crypto = crypto();
        let slot = encode(&crypto, 1, 42, TEST_VALUE, true).unwrap().unwrap();
        let unavailable = UnavailableCryptoProvider;
        assert_eq!(
            decode(&unavailable, 1, &slot),
            Err(CryptoProviderError::Unavailable)
        );
        assert_eq!(
            encode(&unavailable, 1, 43, TEST_VALUE, true),
            Err(CryptoProviderError::Unavailable)
        );
    }
}
