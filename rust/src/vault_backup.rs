//! Versioned, allocation-free container for a vault's encrypted namespace slots.
//!
//! This defines the bytes to be moved by a future guest/host backup channel. It
//! deliberately stores the already-encrypted record slots without decrypting
//! them. Authentication and namespace binding are checked whenever a backup is
//! created or opened.

use crate::vault_blob_record;
use crate::vault_crypto::{CryptoProviderError, VaultCryptoProvider};
use crate::vault_record::{self, RecordFormat, SLOT_SIZE};

pub const NAMESPACE_COUNT: usize = 3;
pub const MAGIC: [u8; 8] = *b"PVLTBAK1";
pub const VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 16;
pub const BACKUP_SIZE: usize = HEADER_SIZE + NAMESPACE_COUNT * SLOT_SIZE;
pub const FULL_SLOT_SIZE: usize = vault_blob_record::SLOT_SIZE;
pub const MAGIC_V2: [u8; 8] = *b"PVLTBAK2";
pub const VERSION_V2: u32 = 2;
pub const BACKUP_V2_SIZE: usize = HEADER_SIZE + NAMESPACE_COUNT * FULL_SLOT_SIZE;

const VERSION_OFFSET: usize = 8;
const COUNT_OFFSET: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupError {
    InvalidLength,
    InvalidHeader,
    InvalidRecord,
    LegacyRecord,
    CryptoUnavailable,
}

fn map_crypto_error(_: CryptoProviderError) -> BackupError {
    BackupError::CryptoUnavailable
}

/// Package one authenticated, encrypted record for each fixed namespace.
///
/// Records must be ordered as system, notes, contacts (namespace IDs 0, 1, 2).
/// Legacy plaintext slots must first be migrated by the vault.
pub fn create(
    crypto: &impl VaultCryptoProvider,
    records: &[[u8; SLOT_SIZE]; NAMESPACE_COUNT],
) -> Result<[u8; BACKUP_SIZE], BackupError> {
    let mut backup = [0; BACKUP_SIZE];
    backup[..VERSION_OFFSET].copy_from_slice(&MAGIC);
    backup[VERSION_OFFSET..COUNT_OFFSET].copy_from_slice(&VERSION.to_le_bytes());
    backup[COUNT_OFFSET..HEADER_SIZE].copy_from_slice(&(NAMESPACE_COUNT as u32).to_le_bytes());

    for (namespace, record) in records.iter().enumerate() {
        validate_record(crypto, namespace, record)?;
        let start = HEADER_SIZE + namespace * SLOT_SIZE;
        backup[start..start + SLOT_SIZE].copy_from_slice(record);
    }
    Ok(backup)
}

/// Validate a backup and return its encrypted records in namespace order.
pub fn open(
    crypto: &impl VaultCryptoProvider,
    bytes: &[u8],
) -> Result<[[u8; SLOT_SIZE]; NAMESPACE_COUNT], BackupError> {
    if bytes.len() != BACKUP_SIZE {
        return Err(BackupError::InvalidLength);
    }
    if bytes[..VERSION_OFFSET] != MAGIC
        || u32::from_le_bytes(bytes[VERSION_OFFSET..COUNT_OFFSET].try_into().unwrap()) != VERSION
        || u32::from_le_bytes(bytes[COUNT_OFFSET..HEADER_SIZE].try_into().unwrap())
            != NAMESPACE_COUNT as u32
    {
        return Err(BackupError::InvalidHeader);
    }

    let mut records = [[0; SLOT_SIZE]; NAMESPACE_COUNT];
    for (namespace, record) in records.iter_mut().enumerate() {
        let start = HEADER_SIZE + namespace * SLOT_SIZE;
        record.copy_from_slice(&bytes[start..start + SLOT_SIZE]);
        validate_record(crypto, namespace, record)?;
    }
    Ok(records)
}

/// Package the active encrypted record block for each namespace. This archive
/// version supports both the original 72-byte records and PVLTBLB1 full-block
/// records while preserving the bytes opaquely.
pub fn create_v2(
    crypto: &impl VaultCryptoProvider,
    records: &[[u8; FULL_SLOT_SIZE]; NAMESPACE_COUNT],
) -> Result<[u8; BACKUP_V2_SIZE], BackupError> {
    let mut backup = [0; BACKUP_V2_SIZE];
    backup[..VERSION_OFFSET].copy_from_slice(&MAGIC_V2);
    backup[VERSION_OFFSET..COUNT_OFFSET].copy_from_slice(&VERSION_V2.to_le_bytes());
    backup[COUNT_OFFSET..HEADER_SIZE].copy_from_slice(&(NAMESPACE_COUNT as u32).to_le_bytes());

    for (namespace, record) in records.iter().enumerate() {
        let mut checked = *record;
        validate_full_record(crypto, namespace, &mut checked)?;
        let start = HEADER_SIZE + namespace * FULL_SLOT_SIZE;
        backup[start..start + FULL_SLOT_SIZE].copy_from_slice(record);
    }
    Ok(backup)
}

/// Validate a PVLTBAK2 archive and return its opaque full-block records.
pub fn open_v2(
    crypto: &impl VaultCryptoProvider,
    bytes: &[u8],
) -> Result<[[u8; FULL_SLOT_SIZE]; NAMESPACE_COUNT], BackupError> {
    if bytes.len() != BACKUP_V2_SIZE {
        return Err(BackupError::InvalidLength);
    }
    if bytes[..VERSION_OFFSET] != MAGIC_V2
        || u32::from_le_bytes(bytes[VERSION_OFFSET..COUNT_OFFSET].try_into().unwrap()) != VERSION_V2
        || u32::from_le_bytes(bytes[COUNT_OFFSET..HEADER_SIZE].try_into().unwrap())
            != NAMESPACE_COUNT as u32
    {
        return Err(BackupError::InvalidHeader);
    }

    let mut records = [[0; FULL_SLOT_SIZE]; NAMESPACE_COUNT];
    for (namespace, record) in records.iter_mut().enumerate() {
        let start = HEADER_SIZE + namespace * FULL_SLOT_SIZE;
        record.copy_from_slice(&bytes[start..start + FULL_SLOT_SIZE]);
        let mut checked = *record;
        validate_full_record(crypto, namespace, &mut checked)?;
    }
    Ok(records)
}

/// Authenticate one full disk block and return its generation.
pub fn full_record_generation(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    record: &[u8; FULL_SLOT_SIZE],
) -> Result<Option<u64>, BackupError> {
    let mut checked = *record;
    validate_full_record(crypto, namespace, &mut checked)
}

/// Apply the authenticated-generation rollback check to full-block records.
pub fn rollback_full_records(
    crypto: &impl VaultCryptoProvider,
    current_generations: &[Option<u64>; NAMESPACE_COUNT],
    records: &[[u8; FULL_SLOT_SIZE]; NAMESPACE_COUNT],
) -> Result<[bool; NAMESPACE_COUNT], BackupError> {
    let mut rollback = [false; NAMESPACE_COUNT];
    for (namespace, record) in records.iter().enumerate() {
        let mut checked = *record;
        let generation = validate_full_record(crypto, namespace, &mut checked)?
            .ok_or(BackupError::InvalidRecord)?;
        rollback[namespace] =
            current_generations[namespace].is_some_and(|current| generation < current);
    }
    Ok(rollback)
}

/// Return which records in a validated restore are older than the source disk.
/// A caller must obtain `current_generations` from its selected source image.
pub fn rollback_namespaces(
    crypto: &impl VaultCryptoProvider,
    current_generations: &[Option<u64>; NAMESPACE_COUNT],
    records: &[[u8; SLOT_SIZE]; NAMESPACE_COUNT],
) -> Result<[bool; NAMESPACE_COUNT], BackupError> {
    let mut rollback = [false; NAMESPACE_COUNT];
    for (namespace, record) in records.iter().enumerate() {
        let decoded = vault_record::decode(crypto, namespace, record)
            .map_err(map_crypto_error)?
            .ok_or(BackupError::InvalidRecord)?;
        if decoded.format != RecordFormat::Encrypted {
            return Err(BackupError::LegacyRecord);
        }
        rollback[namespace] =
            current_generations[namespace].is_some_and(|current| decoded.generation < current);
    }
    Ok(rollback)
}

fn validate_record(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    record: &[u8; SLOT_SIZE],
) -> Result<(), BackupError> {
    match vault_record::decode(crypto, namespace, record).map_err(map_crypto_error)? {
        Some(decoded) if decoded.format == RecordFormat::Encrypted => Ok(()),
        Some(_) => Err(BackupError::LegacyRecord),
        None => Err(BackupError::InvalidRecord),
    }
}

fn validate_full_record(
    crypto: &impl VaultCryptoProvider,
    namespace: usize,
    record: &mut [u8; FULL_SLOT_SIZE],
) -> Result<Option<u64>, BackupError> {
    if record[..8] == vault_blob_record::MAGIC {
        return vault_blob_record::decode(crypto, namespace, record)
            .map_err(map_crypto_error)
            .map(|decoded| decoded.map(|record| record.generation));
    }
    if record[SLOT_SIZE..].iter().any(|byte| *byte != 0) {
        return Err(BackupError::InvalidRecord);
    }
    let mut small = [0; SLOT_SIZE];
    small.copy_from_slice(&record[..SLOT_SIZE]);
    match vault_record::decode(crypto, namespace, &small).map_err(map_crypto_error)? {
        Some(decoded) if decoded.format == RecordFormat::Encrypted => Ok(Some(decoded.generation)),
        Some(_) => Err(BackupError::LegacyRecord),
        None => Err(BackupError::InvalidRecord),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_crypto::{TestCryptoProvider, UnavailableCryptoProvider};
    use crate::vault_record::{self, TEST_KEY};

    fn test_crypto(key: [u8; crate::record_crypto::KEY_SIZE]) -> TestCryptoProvider {
        TestCryptoProvider::new(key)
    }

    fn sample_records() -> [[u8; SLOT_SIZE]; NAMESPACE_COUNT] {
        core::array::from_fn(|namespace| {
            vault_record::encode(
                &test_crypto(TEST_KEY),
                namespace,
                17 + namespace as u64,
                0x1234_0000 + namespace as u64,
                true,
            )
            .unwrap()
            .unwrap()
        })
    }

    fn sample_full_records() -> [[u8; FULL_SLOT_SIZE]; NAMESPACE_COUNT] {
        core::array::from_fn(|namespace| {
            let mut block = [0; FULL_SLOT_SIZE];
            if namespace == 1 {
                block = vault_blob_record::encode(
                    &test_crypto(TEST_KEY),
                    namespace,
                    21,
                    b"private\nnotes",
                    true,
                )
                .unwrap()
                .unwrap();
            } else {
                let small = vault_record::encode(
                    &test_crypto(TEST_KEY),
                    namespace,
                    17 + namespace as u64,
                    0x1234_0000 + namespace as u64,
                    true,
                )
                .unwrap()
                .unwrap();
                block[..SLOT_SIZE].copy_from_slice(&small);
            }
            block
        })
    }

    #[test]
    fn encrypted_backup_round_trips_all_namespaces() {
        let records = sample_records();
        let crypto = test_crypto(TEST_KEY);
        let backup = create(&crypto, &records).unwrap();
        assert_eq!(backup.len(), BACKUP_SIZE);
        assert!(
            !backup
                .windows(8)
                .any(|window| window == 0x1234_0001u64.to_le_bytes())
        );
        assert_eq!(open(&crypto, &backup).unwrap(), records);
    }

    #[test]
    fn full_block_backup_preserves_scalar_and_blob_records() {
        let records = sample_full_records();
        let crypto = test_crypto(TEST_KEY);
        let backup = create_v2(&crypto, &records).unwrap();
        assert_eq!(backup.len(), BACKUP_V2_SIZE);
        assert_eq!(open_v2(&crypto, &backup).unwrap(), records);
        assert_eq!(
            rollback_full_records(&crypto, &[Some(18), Some(22), None], &records).unwrap(),
            [true, true, false]
        );
    }

    #[test]
    fn full_block_backup_rejects_bad_headers_tampering_and_noncanonical_padding() {
        let records = sample_full_records();
        let crypto = test_crypto(TEST_KEY);
        let mut backup = create_v2(&crypto, &records).unwrap();
        backup[HEADER_SIZE + SLOT_SIZE + 100] ^= 1;
        assert_eq!(open_v2(&crypto, &backup), Err(BackupError::InvalidRecord));

        let mut bad_header = create_v2(&crypto, &records).unwrap();
        bad_header[VERSION_OFFSET] ^= 1;
        assert_eq!(
            open_v2(&crypto, &bad_header),
            Err(BackupError::InvalidHeader)
        );

        let mut bad_padding = sample_full_records();
        bad_padding[0][FULL_SLOT_SIZE - 1] = 1;
        assert_eq!(
            create_v2(&crypto, &bad_padding),
            Err(BackupError::InvalidRecord)
        );
    }

    #[test]
    fn restore_guard_detects_only_authenticated_generation_rollbacks() {
        let records = sample_records();
        let crypto = test_crypto(TEST_KEY);
        let current = [Some(18), Some(18), None];
        assert_eq!(
            rollback_namespaces(&crypto, &current, &records).unwrap(),
            [true, false, false]
        );
    }

    #[test]
    fn backup_rejects_wrong_namespace_and_authenticated_data_tampering() {
        let records = sample_records();
        let crypto = test_crypto(TEST_KEY);
        let mut reordered = records;
        reordered.swap(0, 1);
        assert_eq!(create(&crypto, &reordered), Err(BackupError::InvalidRecord));

        let mut backup = create(&crypto, &records).unwrap();
        backup[HEADER_SIZE + SLOT_SIZE + 40] ^= 1;
        assert_eq!(open(&crypto, &backup), Err(BackupError::InvalidRecord));
    }

    #[test]
    fn backup_rejects_bad_header_and_noncanonical_lengths() {
        let records = sample_records();
        let crypto = test_crypto(TEST_KEY);
        let backup = create(&crypto, &records).unwrap();
        assert_eq!(
            open(&crypto, &backup[..BACKUP_SIZE - 1]),
            Err(BackupError::InvalidLength)
        );
        let mut extended = backup.to_vec();
        extended.push(0);
        assert_eq!(open(&crypto, &extended), Err(BackupError::InvalidLength));

        let mut bad_version = backup;
        bad_version[VERSION_OFFSET] ^= 1;
        assert_eq!(open(&crypto, &bad_version), Err(BackupError::InvalidHeader));
    }

    #[test]
    fn backup_creation_requires_authenticated_encrypted_records() {
        let mut records = sample_records();
        let crypto = test_crypto(TEST_KEY);
        records[2][24] ^= 1;
        assert_eq!(create(&crypto, &records), Err(BackupError::InvalidRecord));
    }

    #[test]
    fn backup_rejects_a_different_key_and_legacy_plaintext() {
        let records = sample_records();
        let crypto = test_crypto(TEST_KEY);
        let backup = create(&crypto, &records).unwrap();
        let wrong_key = [0x5a; crate::record_crypto::KEY_SIZE];
        assert_eq!(
            open(&TestCryptoProvider::new(wrong_key), &backup),
            Err(BackupError::InvalidRecord)
        );

        let generation = 29u64;
        let value = 0xfeed_beefu64;
        let mut legacy = [0; SLOT_SIZE];
        legacy[0..8].copy_from_slice(&generation.to_le_bytes());
        legacy[8..16].copy_from_slice(&value.to_le_bytes());
        legacy[16..24].copy_from_slice(
            &(generation.rotate_left(17) ^ value.rotate_right(9) ^ 0xbb67_ae85_84ca_a73b)
                .to_le_bytes(),
        );
        legacy[24..32].copy_from_slice(&(generation ^ 0x6a09_e667_f3bc_c909).to_le_bytes());
        let mut records = sample_records();
        records[1] = legacy;
        assert_eq!(create(&crypto, &records), Err(BackupError::LegacyRecord));
    }

    #[test]
    fn backup_reports_provider_outage_separately_from_bad_records() {
        let crypto = test_crypto(TEST_KEY);
        let backup = create(&crypto, &sample_records()).unwrap();
        assert_eq!(
            open(&UnavailableCryptoProvider, &backup),
            Err(BackupError::CryptoUnavailable)
        );
    }
}
