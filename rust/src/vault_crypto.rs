//! Crypto-operation boundary for vault records.
//!
//! Record code asks a provider to encrypt or authenticate/decrypt bytes. It
//! never receives a key. A device implementation can keep key material behind
//! a trusted service boundary, while the QEMU-only development implementation
//! still holds its fixed test key in process memory.

use crate::record_crypto::{self, KEY_SIZE, NONCE_SIZE, TAG_SIZE};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoProviderError {
    Unavailable,
}

/// Performs AES-256-GCM-SIV operations for the current vault record format.
/// `Ok(false)` from `decrypt` means authentication rejected the record;
/// `Err` means the provider itself could not complete the operation.
pub trait VaultCryptoProvider {
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        plaintext: &mut [u8],
    ) -> Result<[u8; TAG_SIZE], CryptoProviderError>;

    fn decrypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        ciphertext: &mut [u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<bool, CryptoProviderError>;
}

/// Software crypto for QEMU, UTM, and host backup.
///
/// The key is private to this type and is not returned to record or vault
/// callers. This does not isolate it from the QEMU vault process: a future
/// device provider must keep key handling inside its trusted service.
#[cfg(feature = "development-crypto")]
pub struct DevelopmentCryptoProvider {
    key: [u8; KEY_SIZE],
}

#[cfg(feature = "development-crypto")]
const DEVELOPMENT_KEY: [u8; KEY_SIZE] = [
    0x91, 0x2e, 0x7a, 0x04, 0xc3, 0x58, 0xd1, 0x6f, 0x88, 0x33, 0xb7, 0x2a, 0x45, 0xe0, 0x19, 0xad,
    0x6c, 0x52, 0xf8, 0x0b, 0x97, 0x31, 0x44, 0xaa, 0x0e, 0xd5, 0x63, 0x7f, 0x28, 0xb1, 0x9c, 0xe2,
];

#[cfg(feature = "development-crypto")]
impl DevelopmentCryptoProvider {
    pub const fn new() -> Self {
        Self::with_key(DEVELOPMENT_KEY)
    }

    pub const fn with_key(key: [u8; KEY_SIZE]) -> Self {
        Self { key }
    }

    #[cfg(feature = "utm-wrapped-vault-experiment")]
    pub fn replace_key(&mut self, key: &[u8; KEY_SIZE]) {
        use zeroize::Zeroize;
        self.key.zeroize();
        self.key.copy_from_slice(key);
    }

    #[cfg(feature = "utm-wrapped-vault-experiment")]
    pub fn clear_key(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
    }

    /// No key bytes leave the provider; used by the disposable lifecycle's
    /// synchronous eviction acknowledgement, including its failure control.
    #[cfg(any(feature = "utm-dynamic-vault-lock",feature="utm-maps-vault-key"))]
    pub fn key_is_cleared(&self) -> bool { self.key.iter().all(|byte| *byte == 0) }

    /// Fixed-purpose child key. Neither caller-provided contexts nor the root
    /// key are exposed to an application.
    #[cfg(feature = "utm-maps-vault-key")]
    pub fn maps_key(&self) -> Option<zeroize::Zeroizing<[u8;32]>> {
        use hmac::{Hmac,Mac};
        if self.key_is_cleared() { return None; }
        let mut mac=Hmac::<sha2::Sha256>::new_from_slice(&self.key).ok()?;
        mac.update(b"NexatrodeOS/app/maps/saved-places/v1");
        Some(zeroize::Zeroizing::new(mac.finalize().into_bytes().into()))
    }
}

#[cfg(all(test,feature="utm-maps-vault-key"))]
mod maps_tests {
    use super::*;
    #[test] fn maps_child_key_is_stable_distinct_and_evicted() {
        let mut crypto=DevelopmentCryptoProvider::with_key([7;32]);
        let first=crypto.maps_key().unwrap();
        assert_ne!(*first,[7;32]);assert_eq!(*first,*crypto.maps_key().unwrap());
        crypto.replace_key(&[8;32]);assert_ne!(*first,*crypto.maps_key().unwrap());
        crypto.clear_key();assert!(crypto.maps_key().is_none());
        crypto.replace_key(&[7;32]);assert_eq!(*first,*crypto.maps_key().unwrap());
    }
}

#[cfg(feature = "development-crypto")]
impl VaultCryptoProvider for DevelopmentCryptoProvider {
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        plaintext: &mut [u8],
    ) -> Result<[u8; TAG_SIZE], CryptoProviderError> {
        record_crypto::encrypt(&self.key, nonce, associated_data, plaintext)
            .ok_or(CryptoProviderError::Unavailable)
    }

    fn decrypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        ciphertext: &mut [u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<bool, CryptoProviderError> {
        Ok(record_crypto::decrypt(
            &self.key,
            nonce,
            associated_data,
            ciphertext,
            tag,
        ))
    }
}

#[cfg(test)]
pub(crate) struct TestCryptoProvider {
    key: [u8; KEY_SIZE],
}

#[cfg(test)]
impl TestCryptoProvider {
    pub(crate) const fn new(key: [u8; KEY_SIZE]) -> Self {
        Self { key }
    }
}

#[cfg(test)]
impl VaultCryptoProvider for TestCryptoProvider {
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        plaintext: &mut [u8],
    ) -> Result<[u8; TAG_SIZE], CryptoProviderError> {
        record_crypto::encrypt(&self.key, nonce, associated_data, plaintext)
            .ok_or(CryptoProviderError::Unavailable)
    }

    fn decrypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        ciphertext: &mut [u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<bool, CryptoProviderError> {
        Ok(record_crypto::decrypt(
            &self.key,
            nonce,
            associated_data,
            ciphertext,
            tag,
        ))
    }
}

#[cfg(test)]
pub(crate) struct UnavailableCryptoProvider;

#[cfg(test)]
impl VaultCryptoProvider for UnavailableCryptoProvider {
    fn encrypt(
        &self,
        _: &[u8; NONCE_SIZE],
        _: &[u8],
        _: &mut [u8],
    ) -> Result<[u8; TAG_SIZE], CryptoProviderError> {
        Err(CryptoProviderError::Unavailable)
    }

    fn decrypt(
        &self,
        _: &[u8; NONCE_SIZE],
        _: &[u8],
        _: &mut [u8],
        _: &[u8; TAG_SIZE],
    ) -> Result<bool, CryptoProviderError> {
        Err(CryptoProviderError::Unavailable)
    }
}
