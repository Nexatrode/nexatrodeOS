//! Experimental passphrase-wrapped vault-key primitive.
//!
//! The optional disposable UTM experiment connects this to a lock and vault;
//! this module itself supplies no storage, RNG, migration, or retry policy.

use crate::record_crypto;
use pbkdf2::pbkdf2_hmac;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"NXVKP001";
const KDF_ID: u8 = 1; // PBKDF2-HMAC-SHA256, for this prototype only.
const CIPHER_ID: u8 = 1; // AES-256-GCM-SIV.
const ROUNDS: u32 = 600_000;
const HEADER_SIZE: usize = 44;
pub const ENVELOPE_SIZE: usize = HEADER_SIZE + 32 + 16;

fn valid_password(password: &[u8]) -> bool {
    (8..=128).contains(&password.len())
}

fn derive(password: &[u8], salt: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0; 32]);
    pbkdf2_hmac::<Sha256>(password, salt, ROUNDS, &mut *key);
    key
}

/// Fingerprint of a uniformly random vault key, used only to reject an
/// incorrect key at the experimental vault boundary.
pub fn key_hash(key: &[u8; 32]) -> [u8; 32] {
    Sha256::digest(key).into()
}

/// Wrap an existing random vault key. The caller must obtain a fresh 16-byte
/// salt and 12-byte nonce from a cryptographically secure random source for
/// every envelope, and generate the vault key independently using that source.
pub fn seal(
    password: &[u8],
    entropy: &[u8; 28],
    vault_key: &[u8; 32],
) -> Option<[u8; ENVELOPE_SIZE]> {
    if !valid_password(password) {
        return None;
    }
    let mut envelope = [0; ENVELOPE_SIZE];
    envelope[..8].copy_from_slice(MAGIC);
    envelope[8] = KDF_ID;
    envelope[9] = CIPHER_ID;
    envelope[12..16].copy_from_slice(&ROUNDS.to_le_bytes());
    envelope[16..HEADER_SIZE].copy_from_slice(entropy);
    let key = derive(password, &entropy[..16]);
    let nonce: &[u8; 12] = entropy[16..].try_into().ok()?;
    let mut ciphertext = Zeroizing::new(*vault_key);
    let tag = record_crypto::encrypt(&key, nonce, &envelope[..HEADER_SIZE], &mut *ciphertext)?;
    envelope[HEADER_SIZE..HEADER_SIZE + 32].copy_from_slice(&*ciphertext);
    envelope[HEADER_SIZE + 32..].copy_from_slice(&tag);
    Some(envelope)
}

/// Reject unsupported parameters before running the KDF, and never return
/// plaintext unless the entire authenticated envelope has verified.
pub fn open(password: &[u8], envelope: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    if !valid_password(password) || !supported_envelope(envelope) {
        return None;
    }
    let key = derive(password, &envelope[16..32]);
    let nonce: &[u8; 12] = envelope[32..HEADER_SIZE].try_into().ok()?;
    let tag: &[u8; 16] = envelope[HEADER_SIZE + 32..].try_into().ok()?;
    let mut plaintext = Zeroizing::new([0; 32]);
    plaintext.copy_from_slice(&envelope[HEADER_SIZE..HEADER_SIZE + 32]);
    record_crypto::decrypt(&key, nonce, &envelope[..HEADER_SIZE], &mut *plaintext, tag)
        .then_some(plaintext)
}

pub fn supported_envelope(envelope: &[u8]) -> bool {
    !(envelope.len() != ENVELOPE_SIZE
        || &envelope[..8] != MAGIC
        || envelope[8] != KDF_ID
        || envelope[9] != CIPHER_ID
        || envelope[10..12] != [0; 2]
        || envelope[12..16] != ROUNDS.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &[u8] = b"disposable example passphrase";
    const KEY: [u8; 32] = [42; 32];
    const ENTROPY: [u8; 28] = [7; 28];

    #[test]
    fn wrapping_requires_the_right_passphrase() {
        let envelope = seal(PASSWORD, &ENTROPY, &KEY).unwrap();
        assert_eq!(&*open(PASSWORD, &envelope).unwrap(), &KEY);
        assert!(open(b"incorrect passphrase", &envelope).is_none());
        assert!(!envelope.windows(KEY.len()).any(|bytes| bytes == KEY));
    }

    #[test]
    fn matches_independent_python_cryptography_vector() {
        // Generated using hashlib.pbkdf2_hmac and cryptography's AESGCMSIV.
        let envelope = seal(PASSWORD, &ENTROPY, &KEY).unwrap();
        let actual: String = envelope.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            actual,
            concat!(
                "4e58564b5030303101010000c027090007070707070707070707070707070707",
                "0707070707070707070707072698d2cd6e416f33a75881ea470eaec95e7a3cab",
                "d9b1bfa448d488ec92f84fec677f3fcb4a4c1ba33db093d5e66754fe"
            )
        );
    }

    #[test]
    fn rejects_corrupted_or_unsupported_records() {
        let envelope = seal(PASSWORD, &ENTROPY, &KEY).unwrap();
        for offset in [0, 8, 9, 12, 16, 32, 44, ENVELOPE_SIZE - 1] {
            let mut corrupted = envelope;
            corrupted[offset] ^= 1;
            assert!(open(PASSWORD, &corrupted).is_none(), "offset {offset}");
        }
        assert!(open(PASSWORD, &envelope[..ENVELOPE_SIZE - 1]).is_none());
    }

    #[test]
    fn rejects_invalid_passwords_and_separates_salts() {
        assert!(seal(b"short", &ENTROPY, &KEY).is_none());
        assert!(seal(&[b'a'; 129], &ENTROPY, &KEY).is_none());
        let a = seal(PASSWORD, &ENTROPY, &KEY).unwrap();
        let b = seal(PASSWORD, &[8; 28], &KEY).unwrap();
        assert_ne!(a, b);
        assert_eq!(&*open(PASSWORD, &b).unwrap(), &KEY);
    }
}
