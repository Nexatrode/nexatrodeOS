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

/// Fixed-key software crypto for QEMU and host-backup development only.
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
        Self {
            key: DEVELOPMENT_KEY,
        }
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
