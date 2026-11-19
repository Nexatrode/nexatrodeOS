//! PIN-encrypted wallet identity envelope. Storage and UI integration must
//! supply fresh entropy and commit this envelope before retiring old records.

use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use zeroize::Zeroizing;
use crate::record_crypto;

const MAGIC: &[u8; 8] = b"NXPIN001";
const ROUNDS: u32 = 600_000;
const SALT: usize = 16;
const NONCE: usize = 12;
const IDENTITY: usize = 74;
const HEADER: usize = 8 + SALT + NONCE;
pub const ENVELOPE_SIZE: usize = HEADER + IDENTITY + 16;

fn valid_pin(pin: &[u8]) -> bool {
    (6..=12).contains(&pin.len()) && pin.iter().all(u8::is_ascii_digit)
}

fn derive(pin: &[u8], salt: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0; 32]);
    pbkdf2_hmac::<Sha256>(pin, salt, ROUNDS, &mut *key);
    key
}

/// Caller must provide a fresh, cryptographically random salt and nonce.
pub fn seal(pin: &[u8], entropy: &[u8; SALT + NONCE], identity: &[u8; IDENTITY]) -> Option<[u8; ENVELOPE_SIZE]> {
    if !valid_pin(pin) { return None; }
    let mut out = [0; ENVELOPE_SIZE];
    out[..8].copy_from_slice(MAGIC);
    out[8..HEADER].copy_from_slice(entropy);
    let key = derive(pin, &entropy[..SALT]);
    let nonce: &[u8; NONCE] = entropy[SALT..].try_into().ok()?;
    let mut data = Zeroizing::new(*identity);
    let tag = record_crypto::encrypt(&key, nonce, &out[..HEADER], &mut *data)?;
    out[HEADER..HEADER + IDENTITY].copy_from_slice(&data[..]);
    out[HEADER + IDENTITY..].copy_from_slice(&tag);
    Some(out)
}

pub fn open(pin: &[u8], envelope: &[u8]) -> Option<Zeroizing<[u8; IDENTITY]>> {
    if !valid_pin(pin) || envelope.len() != ENVELOPE_SIZE || &envelope[..8] != MAGIC { return None; }
    let key = derive(pin, &envelope[8..8 + SALT]);
    let nonce: &[u8; NONCE] = envelope[8 + SALT..HEADER].try_into().ok()?;
    let tag: &[u8; 16] = envelope[HEADER + IDENTITY..].try_into().ok()?;
    let mut plain = Zeroizing::new([0; IDENTITY]);
    plain.copy_from_slice(&envelope[HEADER..HEADER + IDENTITY]);
    record_crypto::decrypt(&key, nonce, &envelope[..HEADER], &mut *plain, tag).then_some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_authentication() {
        let identity = [42; IDENTITY];
        let envelope = seal(b"123456", &[7; SALT + NONCE], &identity).unwrap();
        assert_eq!(&*open(b"123456", &envelope).unwrap(), &identity);
        assert!(open(b"123457", &envelope).is_none());
        for offset in [8, HEADER, ENVELOPE_SIZE - 1] {
            let mut altered = envelope;
            altered[offset] ^= 1;
            assert!(open(b"123456", &altered).is_none());
        }
        assert!(seal(b"12345", &[7; SALT + NONCE], &identity).is_none());
    }
}
