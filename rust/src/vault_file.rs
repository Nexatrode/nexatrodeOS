//! Fixed, app-scoped database and rollback-journal extents for the vault.
//!
//! The format deliberately starts with one SimpleX agent database and its
//! SQLite rollback journal. It is not a general filesystem: file IDs map to
//! fixed extents, and lengths are authenticated metadata records committed
//! with the same two-slot protocol as other vault blobs.

use crate::vault_blob_record::{self, DecodedBlob, SLOT_SIZE};
use crate::vault_crypto::{CryptoProviderError, VaultCryptoProvider};

pub const BLOCK_SIZE: usize = 4096;
pub const METADATA_FIRST_BLOCK: usize = 10;
pub const DATABASE_FIRST_BLOCK: usize = 14;
pub const FILE_BLOCKS: usize = 1024;
pub const FILE_CAPACITY: usize = FILE_BLOCKS * BLOCK_SIZE;
pub const JOURNAL_FIRST_BLOCK: usize = DATABASE_FIRST_BLOCK + FILE_BLOCKS;
pub const REQUIRED_BLOCKS: usize = JOURNAL_FIRST_BLOCK + FILE_BLOCKS;
pub const METADATA_SLOT_COUNT: usize = 2;
pub const FILE_COUNT: usize = 2;
pub const METADATA_NAMESPACE_BASE: usize = 0x20;
pub const FILE_METADATA_SIZE: usize = 16;
pub const FILE_TRANSFER_MAX: usize = vault_blob_record::MAX_PAYLOAD_SIZE;

const FILE_METADATA_MAGIC: [u8; 8] = *b"PVLTFM1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VaultFileId {
    AgentDatabase = 0,
    AgentJournal = 1,
}

impl VaultFileId {
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::AgentDatabase),
            1 => Some(Self::AgentJournal),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn metadata_namespace(self) -> usize {
        METADATA_NAMESPACE_BASE + self as usize
    }

    pub const fn first_block(self) -> usize {
        match self {
            Self::AgentDatabase => DATABASE_FIRST_BLOCK,
            Self::AgentJournal => JOURNAL_FIRST_BLOCK,
        }
    }

    pub fn physical_block(self, logical_block: usize) -> Option<usize> {
        (logical_block < FILE_BLOCKS).then_some(self.first_block() + logical_block)
    }

    pub const fn metadata_block(self, slot: usize) -> Option<usize> {
        if slot < METADATA_SLOT_COUNT {
            Some(METADATA_FIRST_BLOCK + self.index() * METADATA_SLOT_COUNT + slot)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileMetadata {
    pub generation: u64,
    pub length: u32,
    pub exists: bool,
}

impl FileMetadata {
    pub fn new(generation: u64, length: usize) -> Option<Self> {
        if length > FILE_CAPACITY {
            return None;
        }
        Some(Self {
            generation,
            length: length as u32,
            exists: true,
        })
    }

    pub const fn length(self) -> usize {
        self.length as usize
    }
}

/// Encode file length metadata into one authenticated vault block.
pub fn encode_metadata(
    crypto: &impl VaultCryptoProvider,
    file: VaultFileId,
    metadata: FileMetadata,
    committed: bool,
) -> Result<Option<[u8; SLOT_SIZE]>, CryptoProviderError> {
    if metadata.length() > FILE_CAPACITY {
        return Ok(None);
    }
    let mut payload = [0; FILE_METADATA_SIZE];
    payload[..8].copy_from_slice(&FILE_METADATA_MAGIC);
    payload[8..12].copy_from_slice(&(file as u32).to_le_bytes());
    let encoded_length = metadata.length | (u32::from(metadata.exists) << 31);
    payload[12..16].copy_from_slice(&encoded_length.to_le_bytes());
    vault_blob_record::encode(
        crypto,
        file.metadata_namespace(),
        metadata.generation,
        &payload,
        committed,
    )
}

/// Decode an authenticated file-length metadata block.
pub fn decode_metadata(
    crypto: &impl VaultCryptoProvider,
    file: VaultFileId,
    block: &mut [u8; SLOT_SIZE],
) -> Result<Option<FileMetadata>, CryptoProviderError> {
    let decoded = vault_blob_record::decode(crypto, file.metadata_namespace(), block)?;
    let Some(DecodedBlob {
        generation,
        payload_length,
    }) = decoded
    else {
        return Ok(None);
    };
    if payload_length != FILE_METADATA_SIZE {
        return Ok(None);
    }
    let payload = &block[vault_blob_record::CIPHERTEXT_OFFSET
        ..vault_blob_record::CIPHERTEXT_OFFSET + FILE_METADATA_SIZE];
    if payload[..8] != FILE_METADATA_MAGIC
        || u32::from_le_bytes(payload[8..12].try_into().unwrap()) != file as u32
    {
        return Ok(None);
    }
    let encoded_length = u32::from_le_bytes(payload[12..16].try_into().unwrap());
    let length = encoded_length & 0x7fff_ffff;
    if length as usize > FILE_CAPACITY {
        return Ok(None);
    }
    Ok(Some(FileMetadata {
        generation,
        length,
        exists: encoded_length & 0x8000_0000 != 0,
    }))
}

/// Update a staged metadata block's commit marker after its body is durable.
pub fn set_metadata_committed(block: &mut [u8; SLOT_SIZE]) -> bool {
    vault_blob_record::set_committed(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_crypto::TestCryptoProvider;
    use crate::vault_record::TEST_KEY;

    fn crypto() -> TestCryptoProvider {
        TestCryptoProvider::new(TEST_KEY)
    }

    #[test]
    fn database_and_journal_extents_are_disjoint_and_bounded() {
        let db = VaultFileId::AgentDatabase;
        let journal = VaultFileId::AgentJournal;
        assert_eq!(db.physical_block(0), Some(14));
        assert_eq!(db.physical_block(FILE_BLOCKS - 1), Some(1037));
        assert_eq!(journal.physical_block(0), Some(1038));
        assert_eq!(journal.physical_block(FILE_BLOCKS - 1), Some(2061));
        assert_eq!(db.physical_block(FILE_BLOCKS), None);
        assert!(db.first_block() + FILE_BLOCKS <= journal.first_block());
        assert_eq!(db.metadata_block(0), Some(10));
        assert_eq!(db.metadata_block(1), Some(11));
        assert_eq!(journal.metadata_block(0), Some(12));
        assert_eq!(journal.metadata_block(1), Some(13));
        assert_eq!(VaultFileId::from_u32(2), None);
    }

    #[test]
    fn metadata_round_trips_and_rejects_uncommitted_or_wrong_file_records() {
        let crypto = crypto();
        let db_metadata = FileMetadata::new(7, 8193).unwrap();
        let mut staged = encode_metadata(&crypto, VaultFileId::AgentDatabase, db_metadata, false)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_metadata(&crypto, VaultFileId::AgentDatabase, &mut staged).unwrap(),
            None
        );
        assert!(set_metadata_committed(&mut staged));
        assert_eq!(
            decode_metadata(&crypto, VaultFileId::AgentDatabase, &mut staged).unwrap(),
            Some(db_metadata)
        );
        assert_eq!(
            decode_metadata(&crypto, VaultFileId::AgentJournal, &mut staged).unwrap(),
            None
        );
        assert_eq!(FileMetadata::new(1, FILE_CAPACITY + 1), None);
    }

    #[test]
    fn metadata_is_authenticated_and_file_namespaces_are_separate() {
        let crypto = crypto();
        let metadata = FileMetadata::new(2, 4096).unwrap();
        let mut db = encode_metadata(&crypto, VaultFileId::AgentDatabase, metadata, true)
            .unwrap()
            .unwrap();
        let journal = encode_metadata(&crypto, VaultFileId::AgentJournal, metadata, true)
            .unwrap()
            .unwrap();
        assert_ne!(db, journal);
        db[vault_blob_record::CIPHERTEXT_OFFSET] ^= 1;
        assert_eq!(
            decode_metadata(&crypto, VaultFileId::AgentDatabase, &mut db).unwrap(),
            None
        );
    }
}
