#![no_std]
#![no_main]

mod vault_protocol;
#[cfg(feature = "utm-photo-reader")]
mod vault_photo_access;

use core::{
    ptr,
    sync::atomic::{Ordering, fence},
};
use personal_os_rust_server::vault_blob_record::{self, DecodedBlob};
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use personal_os_rust_server::vault_crypto::DevelopmentCryptoProvider;
use personal_os_rust_server::vault_file::{self, FileMetadata, VaultFileId};
use personal_os_rust_server::vault_history;
use personal_os_rust_server::vault_record::{self, DecodedRecord, RecordFormat, SLOT_SIZE};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, get_mr, memory_region_symbol, protection_domain, set_mr,
};
#[cfg(feature = "storage-test")]
use vault_protocol::REQUEST_TEST_ADVANCE;
use vault_protocol::{
    CALENDAR_APP_CHANNEL, CALENDAR_NAMESPACE, CONTACTS_APP_CHANNEL, CONTACTS_NAMESPACE,
    MESSAGES_APP_CHANNEL, MESSAGES_NAMESPACE, NAMESPACE_COUNT, NOTES_APP_CHANNEL, NOTES_NAMESPACE,
    REPLY_BAD_REQUEST, REPLY_CONFLICT, REPLY_CORRUPT, REPLY_DENIED, REPLY_NAMESPACE_TEST,
    REPLY_NO_BLOB, REPLY_OK, REPLY_STORAGE, REQUEST_FILE_DELETE, REQUEST_FILE_READ,
    REQUEST_FILE_SIZE, REQUEST_FILE_SYNC, REQUEST_FILE_TRUNCATE, REQUEST_FILE_WRITE,
    REQUEST_NAMESPACE_TEST, REQUEST_READ, REQUEST_READ_BLOB, REQUEST_WRITE, REQUEST_WRITE_BLOB,
    SIMPLEX_AGENT_CHANNEL, SYSTEM_NAMESPACE, WALLET_APP_CHANNEL,
};

const BLOCK_SIZE: usize = 4096;
const SLOT_COUNT: usize = 2;
// Keep the system namespace at blocks 0-1. Calendar reuses the previously
// unused prototype gap at blocks 2-3. Notes/Contacts remain at 4-5 and 6-7.
// Messages occupies blocks 8-9 (STORAGE_BLOCK_COUNT=10). Host backup archives
// still cover namespaces 0-2 only. RNG DMA is pinned at 0x5fdc0000 so the
// larger vault data window does not collide with it.
const NAMESPACE_FIRST_BLOCK: [usize; NAMESPACE_COUNT] = [0, 4, 6, 2, 8];
const STORAGE_BLOCK_COUNT: u16 = 10;
const BLOCK_READ: u32 = 0;
const BLOCK_WRITE: u32 = 1;
const BLOCK_FLUSH: u32 = 2;
const BLK_RESP_OK: u32 = 0;
// Two four-block banks beyond the SQLite and wallet extents.
// These blocks were reserved by the wallet file extent. The raw file
// protocol must never allocate into them.
const WALLET_IDENTITY_BLOCK: usize = vault_file::WALLET_FIRST_BLOCK + vault_file::WALLET_BLOCKS - 1;
const WALLET_IDENTITY_NAMESPACE: usize = 0x50;
const PHOTO_FIRST_BLOCK: usize = vault_file::WALLET_FIRST_BLOCK + vault_file::WALLET_BLOCKS;
const PHOTO_BLOCKS: usize = vault_protocol::PHOTO_SLOT_CHUNKS;
const PHOTO_NAMESPACE: usize = 0x30;
const PHOTO_HEADER: usize = 10;
const PHOTO_HEADER_LARGE: usize = 14;
const PHOTO_SEQUENCE_BLOCK: usize = PHOTO_FIRST_BLOCK + 2 * PHOTO_BLOCKS;
const PHOTO_SEQUENCE_NAMESPACE: usize = PHOTO_NAMESPACE + PHOTO_BLOCKS;
const MESSAGE_PHOTO_FIRST_BLOCK: usize = PHOTO_SEQUENCE_BLOCK + 2;
const MAX_WAIT_ITERATIONS: usize = 1_000_000;

// The vault is single-threaded. Keep the larger staging buffer in BSS rather
// than embedding it in the handler value, which otherwise exceeds its stack.
static mut PHOTO_STAGE: [u8; vault_protocol::PHOTO_MAX] = [0; vault_protocol::PHOTO_MAX];

// Length zero was invalid in the original format, so it introduces the
// extended header without changing the interpretation of existing files.
fn photo_header(bytes: &[u8]) -> Option<(u64, usize, usize)> {
    if bytes.len() < PHOTO_HEADER { return None; }
    let owner = u64::from_le_bytes(bytes[..8].try_into().ok()?);
    let short = u16::from_le_bytes(bytes[8..10].try_into().ok()?) as usize;
    if short != 0 { return Some((owner, short, PHOTO_HEADER)); }
    if bytes.len() < PHOTO_HEADER_LARGE { return None; }
    let length = u32::from_le_bytes(bytes[10..14].try_into().ok()?) as usize;
    if length <= u16::MAX as usize || length > vault_protocol::PHOTO_MAX { return None; }
    Some((owner, length, PHOTO_HEADER_LARGE))
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Region {
    vaddr: *mut u8,
    size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlockConnection {
    storage_info: Region,
    request_queue: Region,
    response_queue: Region,
    num_buffers: u16,
    id: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlockClientConfig {
    magic: [u8; 5],
    _padding: [u8; 3],
    virt: BlockConnection,
    data: Region,
}

// sDDF's build replaces this section with the generated client configuration.
#[used]
#[unsafe(link_section = ".blk_client_config")]
static mut BLOCK_CONFIG: BlockClientConfig = BlockClientConfig {
    magic: [b's', b'D', b'D', b'F', 2],
    _padding: [0; 3],
    virt: BlockConnection {
        storage_info: Region {
            vaddr: ptr::null_mut(),
            size: 0,
        },
        request_queue: Region {
            vaddr: ptr::null_mut(),
            size: 0,
        },
        response_queue: Region {
            vaddr: ptr::null_mut(),
            size: 0,
        },
        num_buffers: 0,
        id: 0,
    },
    data: Region {
        vaddr: ptr::null_mut(),
        size: 0,
    },
};

#[repr(C)]
#[derive(Clone, Copy)]
struct BlockRequest {
    code: u32,
    io_or_offset: u64,
    block_number: u64,
    count: u16,
    id: u32,
}

#[repr(C)]
struct RequestQueueHeader {
    head: u32,
    tail: u32,
    plugged: u8,
    _padding: [u8; 3],
    _buffer_alignment: [BlockRequest; 0],
}

#[repr(C)]
struct ResponseQueueHeader {
    head: u32,
    tail: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlockResponse {
    status: u32,
    success_count: u16,
    id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RecordSlot {
    generation: MessageRegisterValue,
    value: MessageRegisterValue,
    payload_length: u32,
    valid: bool,
    encrypted: bool,
    is_blob: bool,
}

#[derive(Clone, Copy)]
enum WriteMode {
    Complete,
    #[cfg(feature = "storage-test")]
    MissingCommit,
    #[cfg(feature = "storage-test")]
    PartialBody,
    #[cfg(feature = "storage-test")]
    BadTag,
    #[cfg(feature = "storage-test")]
    TornCommit,
    #[cfg(feature = "storage-test")]
    Interrupt(InterruptPoint),
}

#[cfg(feature = "storage-test")]
#[derive(Clone, Copy, Eq, PartialEq)]
enum InterruptPoint {
    BeforeStageFlush,
    AfterStageFlush,
    BeforeCommitFlush,
    AfterCommitFlush,
}

impl RecordSlot {
    const INVALID: Self = Self {
        generation: 0,
        value: 0,
        payload_length: 0,
        valid: false,
        encrypted: false,
        is_blob: false,
    };

    fn initial() -> Self {
        Self {
            generation: 0,
            value: vault_protocol::INITIAL_VALUE,
            payload_length: 0,
            valid: true,
            encrypted: true,
            is_blob: false,
        }
    }

    fn valid(self) -> bool {
        self.valid
    }

    fn from_decoded(record: DecodedRecord) -> Self {
        Self {
            generation: record.generation,
            value: record.value,
            payload_length: 0,
            valid: true,
            encrypted: record.format == RecordFormat::Encrypted,
            is_blob: false,
        }
    }

    fn from_blob(record: DecodedBlob) -> Self {
        Self {
            generation: record.generation,
            value: 0,
            payload_length: record.payload_length as u32,
            valid: true,
            encrypted: true,
            is_blob: true,
        }
    }
}

#[repr(C)]
struct VaultState {
    slots: [[RecordSlot; SLOT_COUNT]; NAMESPACE_COUNT],
    files: [PersistentFileState; vault_file::FILE_COUNT],
    loaded: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PersistentFileState {
    generation: u64,
    length: u32,
    active_slot: u32,
    exists: bool,
    loaded: bool,
    dirty: bool,
}

impl PersistentFileState {
    const UNLOADED: Self = Self {
        generation: 0,
        length: 0,
        active_slot: 1,
        exists: false,
        loaded: false,
        dirty: false,
    };
}

#[cfg(feature = "utm-installation-vault-key")]
#[used]
#[unsafe(link_section = ".utm_vault_key")]
static UTM_VAULT_KEY: [u8; 32] = [0; 32];

#[cfg(feature = "utm-installation-vault-key")]
fn installation_crypto() -> DevelopmentCryptoProvider {
    let key = unsafe { ptr::read_volatile(ptr::addr_of!(UTM_VAULT_KEY)) };
    assert!(key != [0; 32], "UTM vault key was not provisioned");
    DevelopmentCryptoProvider::with_key(key)
}

#[protection_domain]
fn init() -> Vault {
    let config = unsafe { ptr::read_volatile(ptr::addr_of!(BLOCK_CONFIG)) };
    #[cfg(feature = "utm-installation-vault-key")]
    let crypto = installation_crypto();
    #[cfg(not(feature = "utm-installation-vault-key"))]
    let crypto = DevelopmentCryptoProvider::new();
    let state = memory_region_symbol!(vault_private_start: *mut VaultState).as_ptr();
    let notes_transfer = memory_region_symbol!(notes_transfer: *mut BlobTransferPage).as_ptr();
    #[cfg(feature = "utm-device-lock")]
    let lock_transfer = memory_region_symbol!(device_lock_transfer: *mut BlobTransferPage).as_ptr();
    let contacts_transfer =
        memory_region_symbol!(contacts_transfer: *mut BlobTransferPage).as_ptr();
    let calendar_transfer =
        memory_region_symbol!(calendar_transfer: *mut BlobTransferPage).as_ptr();
    let messages_transfer =
        memory_region_symbol!(messages_transfer: *mut BlobTransferPage).as_ptr();
    let wallet_transfer = memory_region_symbol!(wallet_transfer: *mut BlobTransferPage).as_ptr();
    #[cfg(feature = "utm-photo-reader")]
    let photo_read_transfer = memory_region_symbol!(photo_read_transfer: *mut BlobTransferPage).as_ptr();
    let wallet_identity_transfer =
        *sel4_microkit::var!(wallet_identity_transfer: usize = 0) as *mut BlobTransferPage;
    #[cfg(feature = "simplex-agent-vault")]
    let simplex_agent_transfer =
        memory_region_symbol!(simplex_agent_transfer: *mut BlobTransferPage).as_ptr();
    unsafe {
        ptr::write_volatile(
            state,
            VaultState {
                slots: [[RecordSlot::INVALID; SLOT_COUNT]; NAMESPACE_COUNT],
                files: [PersistentFileState::UNLOADED; vault_file::FILE_COUNT],
                loaded: 0,
            },
        );
    }
    debug_println!("VAULT: disk-backed service initialized");
    Vault {
        state,
        photo_staged: 0,
        wallet_tx_stage: [0; vault_protocol::WALLET_TX_MAX],
        wallet_tx_staged: 0,
        block: BlockClient::new(config),
        crypto,
        blob_transfer: [
            {
                #[cfg(feature = "utm-device-lock")]
                { lock_transfer }
                #[cfg(not(feature = "utm-device-lock"))]
                { ptr::null_mut() }
            },
            notes_transfer,
            contacts_transfer,
            calendar_transfer,
            messages_transfer,
        ],
        wallet_transfer,
        #[cfg(feature = "utm-photo-reader")]
        photo_read_transfer,
        wallet_identity_transfer,
        #[cfg(feature = "simplex-agent-vault")]
        simplex_agent_transfer,
    }
}

struct Vault {
    photo_staged: usize,
    wallet_tx_stage: [u8; vault_protocol::WALLET_TX_MAX],
    wallet_tx_staged: usize,
    state: *mut VaultState,
    block: BlockClient,
    crypto: DevelopmentCryptoProvider,
    blob_transfer: [*mut BlobTransferPage; NAMESPACE_COUNT],
    wallet_transfer: *mut BlobTransferPage,
    #[cfg(feature = "utm-photo-reader")]
    photo_read_transfer: *mut BlobTransferPage,
    wallet_identity_transfer: *mut BlobTransferPage,
    #[cfg(feature = "simplex-agent-vault")]
    simplex_agent_transfer: *mut BlobTransferPage,
}

impl Vault {
    fn loaded(&self) -> bool {
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.state).loaded)) != 0 }
    }

    fn read_slot(&self, namespace: usize, index: usize) -> RecordSlot {
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.state).slots[namespace][index])) }
    }

    fn set_slot(&mut self, namespace: usize, index: usize, slot: RecordSlot) {
        unsafe {
            ptr::write_volatile(
                ptr::addr_of_mut!((*self.state).slots[namespace][index]),
                slot,
            )
        }
    }

    fn load_from_disk(&mut self) -> Result<(), MessageLabel> {
        if self.loaded() {
            return Ok(());
        }
        self.block.transfer(BLOCK_READ, 0, STORAGE_BLOCK_COUNT)?;
        let bytes = self.block.data;
        let mut valid_count = 0;
        for namespace in 0..NAMESPACE_COUNT {
            let mut namespace_all_zero = true;
            let mut namespace_valid = 0;
            for index in 0..SLOT_COUNT {
                let block_index = NAMESPACE_FIRST_BLOCK[namespace] + index;
                let block = unsafe { bytes.add(block_index * BLOCK_SIZE) };
                for offset in 0..BLOCK_SIZE {
                    if unsafe { ptr::read_volatile(block.add(offset)) } != 0 {
                        namespace_all_zero = false;
                        break;
                    }
                }
                let mut raw_block = unsafe { ptr::read_volatile(block.cast::<[u8; BLOCK_SIZE]>()) };
                if raw_block[..vault_blob_record::MAGIC.len()] == vault_blob_record::MAGIC {
                    match vault_blob_record::decode(&self.crypto, namespace, &mut raw_block)
                        .map_err(|_| REPLY_STORAGE)?
                    {
                        Some(decoded) => {
                            self.set_slot(namespace, index, RecordSlot::from_blob(decoded));
                            namespace_valid += 1;
                        }
                        None => self.set_slot(namespace, index, RecordSlot::INVALID),
                    }
                    continue;
                }
                let mut raw = [0; SLOT_SIZE];
                raw.copy_from_slice(&raw_block[..SLOT_SIZE]);
                let decoded = vault_record::decode(&self.crypto, namespace, &raw)
                    .map_err(|_| REPLY_STORAGE)?;
                if let Some(decoded) = decoded {
                    self.set_slot(namespace, index, RecordSlot::from_decoded(decoded));
                    namespace_valid += 1;
                } else {
                    self.set_slot(namespace, index, RecordSlot::INVALID);
                }
            }

            if namespace_valid == 0 {
                if !namespace_all_zero {
                    return Err(REPLY_CORRUPT);
                }
                self.set_slot(namespace, 0, RecordSlot::initial());
            }
            valid_count += namespace_valid;
        }

        // Validate the lock-bearing namespace before any legacy migration can
        // overwrite a corrupt slot. An unreadable lock must not become absent.
        #[cfg(feature = "utm-device-lock")]
        self.system_lock_state()?;

        // The previous prototype used a plain-value record with a simple
        // checksum. Preserve those demo records once, then leave only the
        // authenticated encrypted slot as the newest committed version.
        for namespace in 0..NAMESPACE_COUNT {
            if let Some((_index, active)) = self.active_slot(namespace) {
                if !active.encrypted {
                    debug_println!("VAULT: migrating legacy plaintext namespace {}", namespace);
                    self.write_record(
                        namespace,
                        active.generation,
                        active.value,
                        WriteMode::Complete,
                    )?;
                }
            }
        }

        if valid_count == 0 {
            debug_println!("VAULT: blank partition; using generation 0");
        } else {
            debug_println!("VAULT: recovered committed namespace records from disk");
        }
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.state).loaded), 1) };
        Ok(())
    }

    fn active_slot(&self, namespace: usize) -> Option<(usize, RecordSlot)> {
        let first = self.read_slot(namespace, 0);
        let second = self.read_slot(namespace, 1);
        match (first.valid(), second.valid()) {
            (true, true) if second.generation > first.generation => Some((1, second)),
            (true, _) => Some((0, first)),
            (false, true) => Some((1, second)),
            (false, false) => None,
        }
    }

    fn write_record(
        &mut self,
        namespace: usize,
        expected_generation: MessageRegisterValue,
        value: MessageRegisterValue,
        mode: WriteMode,
    ) -> Result<MessageRegisterValue, MessageLabel> {
        let Some((active_index, active)) = self.active_slot(namespace) else {
            return Err(REPLY_CORRUPT);
        };
        if active.is_blob {
            return Err(REPLY_BAD_REQUEST);
        }
        if active.generation != expected_generation {
            return Err(REPLY_CONFLICT);
        }
        let Some(next_generation) = active.generation.checked_add(1) else {
            return Err(REPLY_CONFLICT);
        };
        let target = 1 - active_index;
        let Some(staged) =
            vault_record::encode(&self.crypto, namespace, next_generation, value, false)
                .map_err(|_| REPLY_STORAGE)?
        else {
            return Err(REPLY_CORRUPT);
        };

        #[cfg(feature = "storage-test")]
        if matches!(mode, WriteMode::PartialBody) {
            self.write_partial_body(namespace, target, &staged)?;
            self.set_slot(namespace, target, RecordSlot::INVALID);
            return Ok(next_generation);
        }
        self.write_slot_unflushed(namespace, target, staged)?;
        #[cfg(feature = "storage-test")]
        self.interrupt_if_requested(mode, InterruptPoint::BeforeStageFlush);
        self.block.flush()?;
        #[cfg(feature = "storage-test")]
        self.interrupt_if_requested(mode, InterruptPoint::AfterStageFlush);

        #[cfg(feature = "storage-test")]
        if matches!(mode, WriteMode::MissingCommit) {
            self.set_slot(namespace, target, RecordSlot::INVALID);
            return Ok(next_generation);
        }

        let mut committed = match mode {
            #[cfg(feature = "storage-test")]
            WriteMode::BadTag => staged,
            #[cfg(feature = "storage-test")]
            WriteMode::TornCommit => staged,
            #[cfg(feature = "storage-test")]
            WriteMode::Interrupt(_) => staged,
            WriteMode::Complete => staged,
            #[cfg(feature = "storage-test")]
            WriteMode::MissingCommit | WriteMode::PartialBody => unreachable!(),
        };
        let marker = match mode {
            #[cfg(feature = "storage-test")]
            WriteMode::TornCommit => (next_generation ^ vault_record::COMMIT_SALT) ^ 2,
            _ => next_generation ^ vault_record::COMMIT_SALT,
        };
        committed[56..64].copy_from_slice(&marker.to_le_bytes());
        #[cfg(feature = "storage-test")]
        if matches!(mode, WriteMode::BadTag) {
            committed[40] ^= 1;
        }
        self.write_slot_unflushed(namespace, target, committed)?;
        #[cfg(feature = "storage-test")]
        self.interrupt_if_requested(mode, InterruptPoint::BeforeCommitFlush);
        self.block.flush()?;
        #[cfg(feature = "storage-test")]
        self.interrupt_if_requested(mode, InterruptPoint::AfterCommitFlush);
        if matches!(mode, WriteMode::Complete) {
            self.set_slot(
                namespace,
                target,
                RecordSlot {
                    generation: next_generation,
                    value,
                    payload_length: 0,
                    valid: true,
                    encrypted: true,
                    is_blob: false,
                },
            );
        } else {
            self.set_slot(namespace, target, RecordSlot::INVALID);
        }
        Ok(next_generation)
    }

    fn file_state(&self, file: VaultFileId) -> PersistentFileState {
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.state).files[file.index()])) }
    }

    fn set_file_state(&mut self, file: VaultFileId, state: PersistentFileState) {
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.state).files[file.index()]), state) }
    }

    fn check_file_region(&self) -> Result<(), MessageLabel> {
        if self.block.capacity_blocks()? < vault_file::REQUIRED_BLOCKS {
            return Err(REPLY_STORAGE);
        }
        Ok(())
    }

    fn load_file_state(&mut self, file: VaultFileId) -> Result<PersistentFileState, MessageLabel> {
        let cached = self.file_state(file);
        if cached.loaded {
            return Ok(cached);
        }
        self.check_file_region()?;
        let mut decoded: Option<(usize, FileMetadata)> = None;
        let mut any_nonzero = false;
        for slot in 0..vault_file::METADATA_SLOT_COUNT {
            let block = file.metadata_block(slot).ok_or(REPLY_BAD_REQUEST)?;
            let raw = self.read_disk_block(block)?;
            any_nonzero |= raw.iter().any(|byte| *byte != 0);
            let mut authenticated = raw;
            if let Some(metadata) =
                vault_file::decode_metadata(&self.crypto, file, &mut authenticated)
                    .map_err(|_| REPLY_STORAGE)?
            {
                if decoded.is_none_or(|(_, current)| metadata.generation > current.generation) {
                    decoded = Some((slot, metadata));
                }
            }
        }
        let state = match decoded {
            Some((active_slot, metadata)) => PersistentFileState {
                generation: metadata.generation,
                length: metadata.length,
                active_slot: active_slot as u32,
                exists: metadata.exists,
                loaded: true,
                dirty: false,
            },
            None if !any_nonzero => PersistentFileState {
                loaded: true,
                ..PersistentFileState::UNLOADED
            },
            None => return Err(REPLY_CORRUPT),
        };
        self.set_file_state(file, state);
        Ok(state)
    }

    fn read_disk_block(&mut self, block: usize) -> Result<[u8; BLOCK_SIZE], MessageLabel> {
        self.block.transfer_at(BLOCK_READ, block, 0, 1)?;
        let mut bytes = [0; BLOCK_SIZE];
        unsafe {
            bytes.copy_from_slice(&ptr::read_volatile(
                self.block.data.cast::<[u8; BLOCK_SIZE]>(),
            ));
        }
        Ok(bytes)
    }

    fn write_disk_block(
        &mut self,
        block: usize,
        bytes: &[u8; BLOCK_SIZE],
    ) -> Result<(), MessageLabel> {
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), self.block.data, BLOCK_SIZE);
        }
        fence(Ordering::Release);
        self.block.transfer_at(BLOCK_WRITE, block, 0, 1)
    }

    fn file_size(&mut self, file: VaultFileId) -> Result<PersistentFileState, MessageLabel> {
        self.load_file_state(file)
    }

    fn read_file(
        &mut self,
        file: VaultFileId,
        offset: usize,
        requested: usize,
        channel: Channel,
    ) -> Result<usize, MessageLabel> {
        if requested > vault_file::FILE_TRANSFER_MAX {
            return Err(REPLY_BAD_REQUEST);
        }
        let state = self.load_file_state(file)?;
        let page = self.file_transfer_page(channel)?;
        if (unsafe { &*page }).payload().is_none() {
            return Err(REPLY_BAD_REQUEST);
        }
        let length = if offset >= state.length as usize {
            0
        } else {
            requested.min(state.length as usize - offset)
        };
        let mut output = [0; vault_file::FILE_TRANSFER_MAX];
        let mut copied = 0;
        while copied < length {
            let position = offset + copied;
            let logical_block = position / BLOCK_SIZE;
            let within_block = position % BLOCK_SIZE;
            let count = (BLOCK_SIZE - within_block).min(length - copied);
            let physical = file
                .physical_block(logical_block)
                .ok_or(REPLY_BAD_REQUEST)?;
            let block = self.read_disk_block(physical)?;
            output[copied..copied + count]
                .copy_from_slice(&block[within_block..within_block + count]);
            copied += count;
        }
        if !(unsafe { &mut *page }).set_payload(&output[..length]) {
            return Err(REPLY_BAD_REQUEST);
        }
        Ok(length)
    }

    fn write_file_range(
        &mut self,
        file: VaultFileId,
        offset: usize,
        bytes: &[u8],
        visible_length: &mut usize,
    ) -> Result<(), MessageLabel> {
        let end = offset.checked_add(bytes.len()).ok_or(REPLY_BAD_REQUEST)?;
        if end > file.capacity() {
            return Err(REPLY_BAD_REQUEST);
        }
        // Existing wallet files remain readable in full. New writes cannot
        // overlap the node, transaction and identity records reserved in this extent.
        if file == VaultFileId::WalletState && end > BLOCK_SIZE
            && !(*visible_length > BLOCK_SIZE && end <= 5 * BLOCK_SIZE) {
            return Err(REPLY_CONFLICT);
        }
        let mut copied = 0;
        while copied < bytes.len() {
            let position = offset + copied;
            let logical_block = position / BLOCK_SIZE;
            let within_block = position % BLOCK_SIZE;
            let count = (BLOCK_SIZE - within_block).min(bytes.len() - copied);
            let physical = file
                .physical_block(logical_block)
                .ok_or(REPLY_BAD_REQUEST)?;
            let block_start = logical_block * BLOCK_SIZE;
            let mut block = if within_block == 0 && count == BLOCK_SIZE {
                [0; BLOCK_SIZE]
            } else if block_start < *visible_length {
                self.read_disk_block(physical)?
            } else {
                [0; BLOCK_SIZE]
            };
            if *visible_length > block_start && *visible_length < block_start + BLOCK_SIZE {
                block[*visible_length - block_start..].fill(0);
            }
            block[within_block..within_block + count]
                .copy_from_slice(&bytes[copied..copied + count]);
            self.write_disk_block(physical, &block)?;
            copied += count;
            *visible_length = (*visible_length).max(position + count);
        }
        Ok(())
    }

    fn write_file(
        &mut self,
        file: VaultFileId,
        offset: usize,
        length: usize,
        channel: Channel,
    ) -> Result<(), MessageLabel> {
        if length > vault_file::FILE_TRANSFER_MAX {
            return Err(REPLY_BAD_REQUEST);
        }
        let page = self.file_transfer_page(channel)?;
        let input = (unsafe { &*page }).payload().ok_or(REPLY_BAD_REQUEST)?;
        if input.len() != length {
            return Err(REPLY_BAD_REQUEST);
        }
        let mut bytes = [0; vault_file::FILE_TRANSFER_MAX];
        bytes[..length].copy_from_slice(input);
        let mut state = self.load_file_state(file)?;
        let mut visible_length = state.length as usize;
        if offset > visible_length {
            let zeros = [0; vault_file::FILE_TRANSFER_MAX];
            let mut gap = visible_length;
            while gap < offset {
                let count = (offset - gap).min(zeros.len());
                self.write_file_range(file, gap, &zeros[..count], &mut visible_length)?;
                gap += count;
            }
        }
        self.write_file_range(file, offset, &bytes[..length], &mut visible_length)?;
        state.length = visible_length as u32;
        state.exists = true;
        state.dirty = true;
        self.set_file_state(file, state);
        Ok(())
    }

    fn truncate_file(&mut self, file: VaultFileId, new_length: usize) -> Result<(), MessageLabel> {
        if new_length > file.capacity() {
            return Err(REPLY_BAD_REQUEST);
        }
        let mut state = self.load_file_state(file)?;
        if file == VaultFileId::WalletState && new_length > BLOCK_SIZE
            && !(state.exists && state.length as usize > BLOCK_SIZE && new_length <= 5 * BLOCK_SIZE) {
            return Err(REPLY_CONFLICT);
        }
        let mut visible_length = state.length as usize;
        if new_length > visible_length {
            let zeros = [0; vault_file::FILE_TRANSFER_MAX];
            let mut position = visible_length;
            while position < new_length {
                let count = (new_length - position).min(zeros.len());
                self.write_file_range(file, position, &zeros[..count], &mut visible_length)?;
                position += count;
            }
        }
        state.length = new_length as u32;
        state.exists = true;
        state.dirty = true;
        self.set_file_state(file, state);
        Ok(())
    }

    fn sync_file(&mut self, file: VaultFileId) -> Result<(), MessageLabel> {
        let mut state = self.load_file_state(file)?;
        self.block.flush()?;
        if !state.dirty {
            return Ok(());
        }
        let next_generation = state.generation.checked_add(1).ok_or(REPLY_CONFLICT)?;
        let target = 1 - state.active_slot as usize;
        let metadata = FileMetadata {
            generation: next_generation,
            length: state.length,
            exists: state.exists,
        };
        let mut staged = vault_file::encode_metadata(&self.crypto, file, metadata, false)
            .map_err(|_| REPLY_STORAGE)?
            .ok_or(REPLY_BAD_REQUEST)?;
        let block = file.metadata_block(target).ok_or(REPLY_BAD_REQUEST)?;
        self.write_disk_block(block, &staged)?;
        self.block.flush()?;
        if !vault_file::set_metadata_committed(&mut staged) {
            return Err(REPLY_CORRUPT);
        }
        self.write_disk_block(block, &staged)?;
        self.block.flush()?;
        state.generation = next_generation;
        state.active_slot = target as u32;
        state.dirty = false;
        self.set_file_state(file, state);
        Ok(())
    }

    fn delete_file(&mut self, file: VaultFileId) -> Result<(), MessageLabel> {
        if file != VaultFileId::AgentJournal {
            return Err(REPLY_DENIED);
        }
        let mut state = self.load_file_state(file)?;
        state.length = 0;
        state.exists = false;
        state.dirty = true;
        self.set_file_state(file, state);
        self.sync_file(file)
    }

    fn transfer_page(&self, namespace: usize) -> Result<*mut BlobTransferPage, MessageLabel> {
        let page = self
            .blob_transfer
            .get(namespace)
            .copied()
            .ok_or(REPLY_DENIED)?;
        if page.is_null() {
            debug_println!("VAULT|TRANSFER_PAGE_MISSING|namespace={}", namespace);
            return Err(REPLY_DENIED);
        }
        Ok(page)
    }

    fn file_transfer_page(&self, channel: Channel) -> Result<*mut BlobTransferPage, MessageLabel> {
        if channel == WALLET_APP_CHANNEL {
            if self.wallet_transfer.is_null() {
                debug_println!("VAULT|TRANSFER_PAGE_MISSING|wallet");
                return Err(REPLY_DENIED);
            }
            return Ok(self.wallet_transfer);
        }
        if channel == SIMPLEX_AGENT_CHANNEL {
            #[cfg(feature = "simplex-agent-vault")]
            {
                if self.simplex_agent_transfer.is_null() {
                    debug_println!("VAULT|TRANSFER_PAGE_MISSING|SimpleX Agent");
                    return Err(REPLY_DENIED);
                }
                return Ok(self.simplex_agent_transfer);
            }
            #[cfg(not(feature = "simplex-agent-vault"))]
            return Err(REPLY_DENIED);
        }
        self.transfer_page(MESSAGES_NAMESPACE)
    }

    fn read_blob_to_transfer(
        &mut self,
        namespace: usize,
        index: usize,
    ) -> Result<usize, MessageLabel> {
        let block_index = NAMESPACE_FIRST_BLOCK[namespace] + index;
        self.block.transfer(BLOCK_READ, block_index, 1)?;
        let block = unsafe { self.block.data.add(block_index * BLOCK_SIZE) };
        let mut raw = unsafe { ptr::read_volatile(block.cast::<[u8; BLOCK_SIZE]>()) };
        let decoded = vault_blob_record::decode(&self.crypto, namespace, &mut raw)
            .map_err(|_| REPLY_STORAGE)?
            .ok_or(REPLY_CORRUPT)?;
        let metadata = self.read_slot(namespace, index);
        if decoded.generation != metadata.generation
            || decoded.payload_length != metadata.payload_length as usize
        {
            return Err(REPLY_CORRUPT);
        }
        let page = unsafe { &mut *self.transfer_page(namespace)? };
        let payload_end = vault_blob_record::CIPHERTEXT_OFFSET + decoded.payload_length;
        if !page.set_payload(&raw[vault_blob_record::CIPHERTEXT_OFFSET..payload_end]) {
            return Err(REPLY_CORRUPT);
        }
        Ok(decoded.payload_length)
    }

    /// Read the legacy scalar or the lock-aware system payload. A nonblank
    /// invalid slot fails closed: falling back to an older unlocked record
    /// could otherwise silently remove an enrolled lock after corruption.
    #[cfg(feature = "utm-device-lock")]
    fn system_lock_state(&mut self) -> Result<(u64, u64, Option<[u8; personal_os_rust_server::device_lock::SETTINGS_SIZE]>), MessageLabel> {
        for slot in 0..SLOT_COUNT {
            let mut raw = self.read_disk_block(NAMESPACE_FIRST_BLOCK[SYSTEM_NAMESPACE] + slot)?;
            if raw.iter().all(|byte| *byte == 0) { continue; }
            if raw[..vault_blob_record::MAGIC.len()] == vault_blob_record::MAGIC {
                if vault_blob_record::decode(&self.crypto, SYSTEM_NAMESPACE, &mut raw)
                    .map_err(|_| REPLY_STORAGE)?.is_none() { return Err(REPLY_CORRUPT); }
            } else {
                let mut small = [0u8; SLOT_SIZE];
                small.copy_from_slice(&raw[..SLOT_SIZE]);
                if raw[SLOT_SIZE..].iter().any(|byte| *byte != 0)
                    || vault_record::decode(&self.crypto, SYSTEM_NAMESPACE, &small)
                    .map_err(|_| REPLY_STORAGE)?.is_none() { return Err(REPLY_CORRUPT); }
            }
        }
        let (index, slot) = self.active_slot(SYSTEM_NAMESPACE).ok_or(REPLY_CORRUPT)?;
        if !slot.is_blob { return Ok((slot.generation, slot.value, None)); }
        let mut raw = self.read_disk_block(NAMESPACE_FIRST_BLOCK[SYSTEM_NAMESPACE] + index)?;
        let decoded = vault_blob_record::decode(&self.crypto, SYSTEM_NAMESPACE, &mut raw)
            .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
        let start = vault_blob_record::CIPHERTEXT_OFFSET;
        let (value, settings) = personal_os_rust_server::system_lock_record::decode_system(
            &raw[start..start + decoded.payload_length]
        ).map_err(|_| REPLY_CORRUPT)?;
        Ok((decoded.generation, value, Some(settings)))
    }

    fn write_blob(
        &mut self,
        namespace: usize,
        expected_generation: MessageRegisterValue,
        payload: &[u8],
    ) -> Result<MessageRegisterValue, MessageLabel> {
        if payload.len() > vault_blob_record::MAX_PAYLOAD_SIZE {
            return Err(REPLY_BAD_REQUEST);
        }
        let Some((active_index, active)) = self.active_slot(namespace) else {
            return Err(REPLY_CORRUPT);
        };
        if active.generation != expected_generation {
            return Err(REPLY_CONFLICT);
        }
        let Some(next_generation) = active.generation.checked_add(1) else {
            return Err(REPLY_CONFLICT);
        };
        let target = 1 - active_index;
        let Some(mut staged) =
            vault_blob_record::encode(&self.crypto, namespace, next_generation, payload, false)
                .map_err(|_| REPLY_STORAGE)?
        else {
            return Err(REPLY_BAD_REQUEST);
        };

        self.write_blob_slot_unflushed(namespace, target, &staged)?;
        self.block.flush()?;
        if !vault_blob_record::set_committed(&mut staged) {
            return Err(REPLY_CORRUPT);
        }
        self.write_blob_slot_unflushed(namespace, target, &staged)?;
        self.block.flush()?;
        self.set_slot(
            namespace,
            target,
            RecordSlot::from_blob(DecodedBlob {
                generation: next_generation,
                payload_length: payload.len(),
            }),
        );
        Ok(next_generation)
    }

    fn run_namespace_test(
        &mut self,
        namespace: usize,
    ) -> Result<(MessageRegisterValue, MessageRegisterValue), MessageLabel> {
        if namespace == SYSTEM_NAMESPACE {
            return Err(REPLY_BAD_REQUEST);
        }
        let Some((active_index, active)) = self.active_slot(namespace) else {
            return Err(REPLY_CORRUPT);
        };
        if active.is_blob {
            let length = self.read_blob_to_transfer(namespace, active_index)?;
            let page = unsafe { &*self.transfer_page(namespace)? };
            let mut payload = [0; vault_blob_record::MAX_PAYLOAD_SIZE];
            payload[..length].copy_from_slice(&page.payload[..length]);
            let generation = self.write_blob(namespace, active.generation, &payload[..length])?;
            Ok((generation, 0))
        } else {
            let generation = self.write_record(
                namespace,
                active.generation,
                active.value,
                WriteMode::Complete,
            )?;
            Ok((generation, active.value))
        }
    }

    fn write_blob_slot_unflushed(
        &mut self,
        namespace: usize,
        index: usize,
        slot: &[u8; vault_blob_record::SLOT_SIZE],
    ) -> Result<(), MessageLabel> {
        let block_index = NAMESPACE_FIRST_BLOCK[namespace] + index;
        let destination = unsafe { self.block.data.add(block_index * BLOCK_SIZE) };
        unsafe {
            ptr::write_bytes(destination, 0, BLOCK_SIZE);
            ptr::copy_nonoverlapping(slot.as_ptr(), destination, vault_blob_record::SLOT_SIZE);
        }
        fence(Ordering::Release);
        self.block.transfer(BLOCK_WRITE, block_index, 1)
    }

    #[cfg(feature = "storage-test")]
    fn write_partial_body(
        &mut self,
        namespace: usize,
        index: usize,
        staged: &[u8; SLOT_SIZE],
    ) -> Result<(), MessageLabel> {
        let block_index = NAMESPACE_FIRST_BLOCK[namespace] + index;
        let destination = unsafe { self.block.data.add(block_index * BLOCK_SIZE) };
        unsafe {
            ptr::write_bytes(destination, 0, BLOCK_SIZE);
            ptr::copy_nonoverlapping(staged.as_ptr(), destination, 20);
        }
        fence(Ordering::Release);
        self.block.transfer(BLOCK_WRITE, block_index, 1)?;
        self.block.flush()
    }

    #[cfg(feature = "storage-test")]
    fn advance_test_interruption(
        &mut self,
    ) -> Result<(MessageRegisterValue, MessageRegisterValue), MessageLabel> {
        const TEST_VALUE: MessageRegisterValue = 0x5253_544f_5245;
        const STAGE_DONE: MessageRegisterValue = 5;

        // Other capabilities (including the app-scoped SQLite files) reuse
        // the block client's DMA staging area. Re-read the system slot here
        // instead of assuming the last unrelated transfer left it in memory.
        self.block
            .transfer(BLOCK_READ, NAMESPACE_FIRST_BLOCK[SYSTEM_NAMESPACE], 1)?;
        let raw = unsafe { ptr::read_volatile(self.block.data.cast::<[u8; SLOT_SIZE]>()) };
        let valid_staged =
            vault_record::encode(&self.crypto, SYSTEM_NAMESPACE, 2, TEST_VALUE, false)
                .map_err(|_| REPLY_STORAGE)?
                .ok_or(REPLY_CORRUPT)?;
        let mut partial = [0; SLOT_SIZE];
        partial[..20].copy_from_slice(&valid_staged[..20]);
        let mut bad_tag = vault_record::encode(&self.crypto, SYSTEM_NAMESPACE, 2, TEST_VALUE, true)
            .map_err(|_| REPLY_STORAGE)?
            .ok_or(REPLY_CORRUPT)?;
        bad_tag[40] ^= 1;
        let mut torn_commit =
            vault_record::encode(&self.crypto, SYSTEM_NAMESPACE, 2, TEST_VALUE, true)
                .map_err(|_| REPLY_STORAGE)?
                .ok_or(REPLY_CORRUPT)?;
        let marker = 2 ^ vault_record::COMMIT_SALT;
        torn_commit[56..64].copy_from_slice(&(marker ^ 2).to_le_bytes());
        let (stage, mode) = if raw.iter().all(|byte| *byte == 0) {
            (1, WriteMode::MissingCommit)
        } else if raw == valid_staged {
            (2, WriteMode::PartialBody)
        } else if raw == partial {
            (3, WriteMode::BadTag)
        } else if raw == bad_tag {
            (4, WriteMode::TornCommit)
        } else if raw == torn_commit {
            set_mr(0, STAGE_DONE);
            set_mr(1, 1);
            return Ok((STAGE_DONE, 1));
        } else {
            return Err(REPLY_CORRUPT);
        };

        let generation = self.write_record(SYSTEM_NAMESPACE, 1, TEST_VALUE, mode)?;
        set_mr(0, stage);
        set_mr(1, generation);
        Ok((stage, generation))
    }

    #[cfg(feature = "storage-test")]
    fn start_flush_interruption(
        &mut self,
        namespace: usize,
        stage: MessageRegisterValue,
    ) -> Result<MessageRegisterValue, MessageLabel> {
        let point = match stage {
            1 => InterruptPoint::BeforeStageFlush,
            2 => InterruptPoint::AfterStageFlush,
            3 => InterruptPoint::BeforeCommitFlush,
            4 => InterruptPoint::AfterCommitFlush,
            _ => return Err(REPLY_BAD_REQUEST),
        };
        if namespace == SYSTEM_NAMESPACE {
            return self.write_record(
                SYSTEM_NAMESPACE,
                1,
                0x5253_544f_5245,
                WriteMode::Interrupt(point),
            );
        }
        if namespace != NOTES_NAMESPACE {
            return Err(REPLY_DENIED);
        }

        let Some((_active_index, active)) = self.active_slot(namespace) else {
            return Err(REPLY_CORRUPT);
        };
        let mut payload = [0; vault_blob_record::MAX_PAYLOAD_SIZE];
        let length = {
            let page = unsafe { &*self.transfer_page(namespace)? };
            let Some(input) = page.payload() else {
                return Err(REPLY_BAD_REQUEST);
            };
            payload[..input.len()].copy_from_slice(input);
            input.len()
        };
        self.write_blob_with_interruption(namespace, active.generation, &payload[..length], point)
    }

    #[cfg(feature = "storage-test")]
    fn write_blob_with_interruption(
        &mut self,
        namespace: usize,
        expected_generation: MessageRegisterValue,
        payload: &[u8],
        point: InterruptPoint,
    ) -> Result<MessageRegisterValue, MessageLabel> {
        let Some((active_index, active)) = self.active_slot(namespace) else {
            return Err(REPLY_CORRUPT);
        };
        if active.generation != expected_generation {
            return Err(REPLY_CONFLICT);
        }
        let Some(next_generation) = active.generation.checked_add(1) else {
            return Err(REPLY_CONFLICT);
        };
        let target = 1 - active_index;
        let Some(mut staged) =
            vault_blob_record::encode(&self.crypto, namespace, next_generation, payload, false)
                .map_err(|_| REPLY_STORAGE)?
        else {
            return Err(REPLY_BAD_REQUEST);
        };

        self.write_blob_slot_unflushed(namespace, target, &staged)?;
        self.interrupt_if_requested(
            WriteMode::Interrupt(point),
            InterruptPoint::BeforeStageFlush,
        );
        self.block.flush()?;
        self.interrupt_if_requested(WriteMode::Interrupt(point), InterruptPoint::AfterStageFlush);
        if !vault_blob_record::set_committed(&mut staged) {
            return Err(REPLY_CORRUPT);
        }
        self.write_blob_slot_unflushed(namespace, target, &staged)?;
        self.interrupt_if_requested(
            WriteMode::Interrupt(point),
            InterruptPoint::BeforeCommitFlush,
        );
        self.block.flush()?;
        self.interrupt_if_requested(
            WriteMode::Interrupt(point),
            InterruptPoint::AfterCommitFlush,
        );
        self.set_slot(
            namespace,
            target,
            RecordSlot::from_blob(DecodedBlob {
                generation: next_generation,
                payload_length: payload.len(),
            }),
        );
        Ok(next_generation)
    }

    fn write_slot_unflushed(
        &mut self,
        namespace: usize,
        index: usize,
        slot: [u8; SLOT_SIZE],
    ) -> Result<(), MessageLabel> {
        let block_index = NAMESPACE_FIRST_BLOCK[namespace] + index;
        let destination = unsafe { self.block.data.add(block_index * BLOCK_SIZE) };
        unsafe {
            ptr::write_bytes(destination, 0, BLOCK_SIZE);
            ptr::copy_nonoverlapping(slot.as_ptr(), destination, SLOT_SIZE);
        }
        fence(Ordering::Release);
        self.block.transfer(BLOCK_WRITE, block_index, 1)
    }

    #[cfg(feature = "storage-test")]
    fn interrupt_if_requested(&self, mode: WriteMode, point: InterruptPoint) {
        let WriteMode::Interrupt(requested) = mode else {
            return;
        };
        if requested != point {
            return;
        }
        let name = match point {
            InterruptPoint::BeforeStageFlush => "BEFORE_STAGE_FLUSH",
            InterruptPoint::AfterStageFlush => "AFTER_STAGE_FLUSH",
            InterruptPoint::BeforeCommitFlush => "BEFORE_COMMIT_FLUSH",
            InterruptPoint::AfterCommitFlush => "AFTER_COMMIT_FLUSH",
        };
        debug_println!("STORAGE|RESET_POINT|{}", name);
        loop {
            sel4::r#yield();
        }
    }
}

impl Vault {
    fn read_photo_block(
        &mut self,
        first_block: usize,
        part: usize,
    ) -> Result<([u8; BLOCK_SIZE], Option<DecodedBlob>), MessageLabel> {
        let mut raw = self.read_disk_block(first_block + part)?;
        let record = vault_blob_record::decode(&self.crypto, PHOTO_NAMESPACE + part, &mut raw)
            .map_err(|_| REPLY_STORAGE)?;
        Ok((raw, record))
    }

    fn active_photo_bank(&mut self) -> Result<Option<(usize, u64)>, MessageLabel> {
        let mut active = None;
        for bank in 0..2 {
            if let (_, Some(record)) = self.read_photo_block(PHOTO_FIRST_BLOCK + bank * PHOTO_BLOCKS, 0)? {
                if active.is_none_or(|(_, generation)| record.generation > generation) {
                    active = Some((bank, record.generation));
                }
            }
        }
        Ok(active)
    }

    // Old sequence records used an empty payload and generation == slot.
    // New records reserve an independent generation and retain the physical
    // high-water mark even when a freed slot is reused.
    fn photo_sequence(&mut self) -> Result<(u64, u64), MessageLabel> {
        let legacy = self.active_photo_bank()?.map_or(0, |(_, generation)| generation);
        let mut latest = (legacy, legacy);
        for slot in 0..2 {
            let mut raw = self.read_disk_block(PHOTO_SEQUENCE_BLOCK + slot)?;
            if let Some(record) = vault_blob_record::decode(&self.crypto, PHOTO_SEQUENCE_NAMESPACE, &mut raw)
                .map_err(|_| REPLY_STORAGE)?
            {
                if record.generation < latest.0 {
                    continue;
                }
                let start = vault_blob_record::CIPHERTEXT_OFFSET;
                let slots = match record.payload_length {
                    0 => record.generation,
                    8 => u64::from_le_bytes(raw[start..start + 8].try_into().unwrap()),
                    _ => return Err(REPLY_CORRUPT),
                };
                latest = (record.generation, slots);
            }
        }
        Ok(latest)
    }

    fn reserve_photo(&mut self, needed: usize, reserve_library_entry: bool) -> Result<(u64, usize), MessageLabel> {
        let (generation, slots) = self.photo_sequence()?;
        if needed == 0 || needed > vault_protocol::PHOTO_MAX.div_ceil(PHOTO_BLOCKS * vault_protocol::PHOTO_CHUNK) {
            return Err(REPLY_BAD_REQUEST);
        }
        let next = generation.checked_add(1).ok_or(REPLY_CONFLICT)?;
        let last_generation = generation.checked_add(needed as u64).ok_or(REPLY_CONFLICT)?;
        u32::try_from(last_generation).map_err(|_| REPLY_CONFLICT)?;
        let blocks = self.block.capacity_blocks()?;
        let mut history_end = self.history_end(blocks)?;
        if reserve_library_entry {
            if history_end >= vault_history::MAX_INDEXED_RECORDS as u64 { return Err(REPLY_STORAGE); }
            history_end = history_end.checked_add(1).ok_or(REPLY_STORAGE)?;
        }
        let limit = vault_history::photo_limit(blocks, history_end).ok_or(REPLY_STORAGE)?;
        let mut slot = 1u64;
        loop {
            let last = slot.checked_add(needed as u64 - 1).ok_or(REPLY_STORAGE)?;
            let first = self.message_photo_block(slot)?;
            let end = first.checked_add(needed * PHOTO_BLOCKS).ok_or(REPLY_STORAGE)?;
            if end > limit { return Err(REPLY_STORAGE); }
            let mut occupied = false;
            for candidate in slot..=last.min(slots) {
                let (_, record) = self.read_photo_block(self.message_photo_block(candidate)?, 0)?;
                if record.is_some() {
                    slot = candidate.checked_add(1).ok_or(REPLY_STORAGE)?;
                    occupied = true;
                    break;
                }
            }
            if !occupied { break; }
        }
        let first = self.message_photo_block(slot)?;

        // The low 32 bits locate the slot. The high bits identify this
        // allocation so a stale reference cannot open a replacement photo.
        // Legacy IDs have zero high bits and still locate their old slot.
        let id = u64::from(u32::try_from(next).map_err(|_| REPLY_CONFLICT)?) << 32
            | u64::from(u32::try_from(slot).map_err(|_| REPLY_STORAGE)?);
        let encoded = vault_blob_record::encode(
            &self.crypto, PHOTO_SEQUENCE_NAMESPACE, last_generation,
            &slots.max(slot + needed as u64 - 1).to_le_bytes(), true,
        ).map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
        self.write_disk_block(PHOTO_SEQUENCE_BLOCK + (last_generation % 2) as usize, &encoded)?;
        self.block.flush()?;
        Ok((id, first))
    }

    fn purge_chat_photos(&mut self, handle: u64) -> Result<(), MessageLabel> {
        let (_, slots) = self.photo_sequence()?;
        let mut freed = 0;
        for first in (0..2).map(|bank| PHOTO_FIRST_BLOCK + bank * PHOTO_BLOCKS)
            .chain((0..slots as usize).map(|slot| MESSAGE_PHOTO_FIRST_BLOCK + slot * PHOTO_BLOCKS))
        {
            let (raw, record) = self.read_photo_block(first, 0)?;
            if record.is_some_and(|record| record.payload_length >= PHOTO_HEADER) {
                let start = vault_blob_record::CIPHERTEXT_OFFSET;
                let owner = u64::from_le_bytes(raw[start..start + 8].try_into().unwrap());
                if owner == handle {
                    // The header is the commit record. Invalidate it before
                    // offering this extent to another allocation.
                    self.write_disk_block(first, &[0; BLOCK_SIZE])?;
                    freed += 1;
                }
            }
        }
        self.block.flush()?;
        if freed != 0 {
            debug_println!("SIMPLEXIPC|VAULT|PHOTO_RECLAIMED|{}|{}", handle, freed);
        }
        Ok(())
    }

    fn message_photo_block(&self, id: u64) -> Result<usize, MessageLabel> {
        let slot = usize::try_from(id.checked_sub(1).ok_or(REPLY_BAD_REQUEST)?)
            .map_err(|_| REPLY_BAD_REQUEST)?;
        let first = slot.checked_mul(PHOTO_BLOCKS)
            .and_then(|offset| MESSAGE_PHOTO_FIRST_BLOCK.checked_add(offset))
            .ok_or(REPLY_BAD_REQUEST)?;
        if first.checked_add(PHOTO_BLOCKS).ok_or(REPLY_BAD_REQUEST)? > self.block.capacity_blocks()? {
            return Err(REPLY_STORAGE);
        }
        Ok(first)
    }

    fn photo_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        let page = self.transfer_page(MESSAGES_NAMESPACE)?;
        self.photo_request_on_page(request, page, None)
    }

    fn photo_request_on_page(&mut self, request: MessageInfo, page: *mut BlobTransferPage, expected_owner: Option<u64>) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{PHOTO_CHUNK, PHOTO_MAX, REQUEST_PHOTO_COMMIT, REQUEST_PHOTO_READ, REQUEST_PHOTO_STAGE};
        if self.block.capacity_blocks()? < PHOTO_SEQUENCE_BLOCK + 2 {
            return Err(REPLY_STORAGE);
        }
        match (request.label(), request.count()) {
            (vault_protocol::REQUEST_PHOTO_PURGE_CHAT, 1) => {
                let handle = get_mr(0);
                if handle == 0 {
                    return Err(REPLY_BAD_REQUEST);
                }
                self.purge_chat_photos(handle)?;
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            (REQUEST_PHOTO_STAGE, 1) => {
                let offset = get_mr(0) as usize;
                if offset == 0 {
                    unsafe { (&mut *ptr::addr_of_mut!(PHOTO_STAGE)).fill(0); }
                    self.photo_staged = 0;
                }
                let bytes = (unsafe { &*page }).payload().ok_or(REPLY_BAD_REQUEST)?;
                if offset != self.photo_staged || bytes.is_empty()
                    || bytes.len() > PHOTO_CHUNK || bytes.len() > PHOTO_MAX - offset
                {
                    return Err(REPLY_BAD_REQUEST);
                }
                unsafe { (&mut *ptr::addr_of_mut!(PHOTO_STAGE))[offset..offset + bytes.len()].copy_from_slice(bytes); }
                self.photo_staged += bytes.len();
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            (REQUEST_PHOTO_COMMIT | vault_protocol::REQUEST_ATTACHMENT_COMMIT, 2) => {
                let handle = get_mr(0);
                let length = get_mr(1) as usize;
                let photo = request.label() == REQUEST_PHOTO_COMMIT;
                if handle == 0 || !(1..=PHOTO_MAX).contains(&length)
                    || length != self.photo_staged
                    || (photo && (length < 4 || unsafe { (&*ptr::addr_of!(PHOTO_STAGE))[..2] != [0xff, 0xd8] }))
                {
                    return Err(REPLY_BAD_REQUEST);
                }
                let needed = length.div_ceil(PHOTO_BLOCKS * PHOTO_CHUNK);
                let (generation, first_block) = self.reserve_photo(needed, handle == vault_protocol::LIBRARY_OWNER)?;
                // The first block is the commit record. Flush the remaining
                // blocks before it becomes visible to recovery.
                for part in (0..length.div_ceil(PHOTO_CHUNK)).rev() {
                    let offset = part * PHOTO_CHUNK;
                    let bytes = unsafe { &(&*ptr::addr_of!(PHOTO_STAGE))[offset..length.min(offset + PHOTO_CHUNK)] };
                    let mut payload = [0; PHOTO_HEADER_LARGE + PHOTO_CHUNK];
                    let segment = part / PHOTO_BLOCKS;
                    let segment_part = part % PHOTO_BLOCKS;
                    let segment_generation = generation
                        .checked_add((segment as u64) << 32)
                        .and_then(|id| id.checked_add(segment as u64))
                        .ok_or(REPLY_STORAGE)?;
                    let size = if segment_part == 0 {
                        payload[..8].copy_from_slice(&handle.to_le_bytes());
                        let header = if length <= u16::MAX as usize {
                            payload[8..PHOTO_HEADER].copy_from_slice(&(length as u16).to_le_bytes());
                            PHOTO_HEADER
                        } else {
                            payload[8..PHOTO_HEADER].fill(0);
                            payload[PHOTO_HEADER..PHOTO_HEADER_LARGE].copy_from_slice(&(length as u32).to_le_bytes());
                            PHOTO_HEADER_LARGE
                        };
                        payload[header..header + bytes.len()].copy_from_slice(bytes);
                        self.block.flush()?;
                        header + bytes.len()
                    } else {
                        payload[..bytes.len()].copy_from_slice(bytes);
                        bytes.len()
                    };
                    let encoded = vault_blob_record::encode(
                        &self.crypto, PHOTO_NAMESPACE + segment_part, segment_generation, &payload[..size], true,
                    ).map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_BAD_REQUEST)?;
                    self.write_disk_block(first_block + part, &encoded)?;
                }
                self.block.flush()?;
                unsafe { (&mut *ptr::addr_of_mut!(PHOTO_STAGE)).fill(0); }
                self.photo_staged = 0;
                set_mr(0, generation);
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_PHOTO_READ, 2) => {
                let id = get_mr(0);
                let part = get_mr(1) as usize;
                if part >= PHOTO_MAX.div_ceil(PHOTO_CHUNK) {
                    return Err(REPLY_BAD_REQUEST);
                }
                // Every chunk is authorized by the committed first slot.
                // This prevents stale IDs from reading leftover continuation
                // blocks after a chat is purged or an allocation is reused.
                let (base, base_generation) = if id == 0 {
                    let (bank, generation) = self.active_photo_bank()?.ok_or(REPLY_NO_BLOB)?;
                    (PHOTO_FIRST_BLOCK + bank * PHOTO_BLOCKS, generation)
                } else {
                    (self.message_photo_block(id & u64::from(u32::MAX))?, id)
                };
                let (base_raw, base_record) = self.read_photo_block(base, 0)?;
                let base_record = base_record.filter(|record| record.generation == base_generation
                    && record.payload_length >= PHOTO_HEADER).ok_or(REPLY_CORRUPT)?;
                let base_start = vault_blob_record::CIPHERTEXT_OFFSET;
                let (owner, total, _) = photo_header(&base_raw[base_start..base_start + base_record.payload_length]).ok_or(REPLY_CORRUPT)?;
                if owner == 0 || total == 0 || total > PHOTO_MAX || part >= total.div_ceil(PHOTO_CHUNK) {
                    return Err(REPLY_CORRUPT);
                }
                #[cfg(feature = "utm-photo-reader")]
                if expected_owner.is_some_and(|expected| !vault_photo_access::permits(id, expected, owner)) {
                    return Err(REPLY_DENIED);
                }
                #[cfg(not(feature = "utm-photo-reader"))]
                let _ = expected_owner;
                let _ = base_record;
                let (first_block, generation) = if id == 0 {
                    if part >= PHOTO_BLOCKS { return Err(REPLY_BAD_REQUEST); }
                    let (bank, generation) = self.active_photo_bank()?.ok_or(REPLY_NO_BLOB)?;
                    (PHOTO_FIRST_BLOCK + bank * PHOTO_BLOCKS, generation)
                } else {
                    let segment = part / PHOTO_BLOCKS;
                    let slot = (id & u64::from(u32::MAX)).checked_add(segment as u64).ok_or(REPLY_BAD_REQUEST)?;
                    let generation = id.checked_add((segment as u64) << 32)
                        .and_then(|v| v.checked_add(segment as u64)).ok_or(REPLY_BAD_REQUEST)?;
                    (self.message_photo_block(slot)?, generation)
                };
                let segment_part = part % PHOTO_BLOCKS;
                let (raw, record) = self.read_photo_block(first_block, segment_part)?;
                let record = record.filter(|record| record.generation == generation).ok_or(REPLY_CORRUPT)?;
                let start = vault_blob_record::CIPHERTEXT_OFFSET;
                let mut bytes = &raw[start..start + record.payload_length];
                if segment_part == 0 {
                    if bytes.len() < PHOTO_HEADER {
                        return Err(REPLY_CORRUPT);
                    }
                    let (segment_owner, segment_total, header) = photo_header(bytes).ok_or(REPLY_CORRUPT)?;
                    if segment_owner != owner || segment_total != total { return Err(REPLY_CORRUPT); }
                    if part == 0 {
                        set_mr(0, segment_owner);
                        set_mr(1, segment_total as u64);
                    }
                    bytes = &bytes[header..];
                }
                let expected = (total - part * PHOTO_CHUNK).min(PHOTO_CHUNK);
                if bytes.len() != expected { return Err(REPLY_CORRUPT); }
                if !(unsafe { &mut *page }).set_payload(bytes) {
                    return Err(REPLY_CORRUPT);
                }
                Ok(MessageInfo::new(REPLY_OK, if part == 0 { 2 } else { 0 }))
            }
            _ => Err(REPLY_BAD_REQUEST),
        }
    }

    /// Store library metadata as authenticated history events and bytes in the
    /// existing encrypted attachment allocator. No existing extents move.
    fn library_commit_staged(&mut self, total: usize, filename: &[u8]) -> Result<u64, MessageLabel> {
        let page = self.transfer_page(MESSAGES_NAMESPACE)?;
        set_mr(0, vault_protocol::LIBRARY_OWNER);
        set_mr(1, total as u64);
        let reply = self.photo_request(MessageInfo::new(vault_protocol::REQUEST_ATTACHMENT_COMMIT, 2))?;
        if reply.label() != REPLY_OK || reply.count() != 1 { return Err(REPLY_STORAGE); }
        let file_id = get_mr(0);
        let mut entry = [0u8; 21 + 120];
        entry[..8].copy_from_slice(b"NXFILE2\0");
        entry[8..16].copy_from_slice(&file_id.to_le_bytes());
        entry[16..20].copy_from_slice(&(total as u32).to_le_bytes());
        entry[20] = filename.len() as u8;
        entry[21..21 + filename.len()].copy_from_slice(filename);
        if !(unsafe { &mut *page }).set_payload(&entry[..21 + filename.len()]) { return Err(REPLY_CORRUPT); }
        set_mr(0, vault_protocol::LIBRARY_OWNER);
        let reply = self.history_request(MessageInfo::new(vault_protocol::REQUEST_HISTORY_APPEND, 1))?;
        if reply.label() != REPLY_OK || reply.count() != 1 { return Err(REPLY_STORAGE); }
        Ok(file_id)
    }

    fn stage_attachment_copy(&mut self, source_id: u64, source_owner: u64) -> Result<usize, MessageLabel> {
        let page = self.transfer_page(MESSAGES_NAMESPACE)?;
        let mut total = 0usize;
        let mut offset = 0usize;
        loop {
            set_mr(0, source_id);
            set_mr(1, (offset / vault_protocol::PHOTO_CHUNK) as u64);
            let reply = self.photo_request(MessageInfo::new(vault_protocol::REQUEST_PHOTO_READ, 2))?;
            if reply.label() != REPLY_OK || reply.count() != if offset == 0 { 2 } else { 0 } { return Err(REPLY_CORRUPT); }
            if offset == 0 {
                if get_mr(0) != source_owner { return Err(REPLY_DENIED); }
                total = get_mr(1) as usize;
                if !(1..=vault_protocol::PHOTO_MAX).contains(&total) { return Err(REPLY_CORRUPT); }
            }
            let bytes = (unsafe { &*page }).payload().ok_or(REPLY_CORRUPT)?;
            let size = (total - offset).min(vault_protocol::PHOTO_CHUNK);
            if bytes.len() != size { return Err(REPLY_CORRUPT); }
            set_mr(0, offset as u64);
            self.photo_request(MessageInfo::new(vault_protocol::REQUEST_PHOTO_STAGE, 1))?;
            offset += size;
            if offset == total { return Ok(total); }
        }
    }

    fn library_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{LIBRARY_OWNER, PHOTO_CHUNK, PHOTO_MAX, REQUEST_ATTACHMENT_COMMIT,
            REQUEST_HISTORY_READ, REQUEST_LIBRARY_CLONE, REQUEST_LIBRARY_NEXT, REQUEST_LIBRARY_DELETE, REQUEST_LIBRARY_IMPORT, REQUEST_LIBRARY_COPY_TO_CHAT};
        let page = self.transfer_page(MESSAGES_NAMESPACE)?;
        match (request.label(), request.count()) {
            (REQUEST_LIBRARY_COPY_TO_CHAT, 2) => {
                let id = get_mr(0);
                let owner = get_mr(1);
                if id == 0 || owner == 0 || owner == LIBRARY_OWNER { return Err(REPLY_BAD_REQUEST); }
                let total = self.stage_attachment_copy(id, LIBRARY_OWNER)?;
                set_mr(0, owner);
                set_mr(1, total as u64);
                self.photo_request(MessageInfo::new(REQUEST_ATTACHMENT_COMMIT, 2))
            }
            (REQUEST_LIBRARY_IMPORT, 1) => {
                let total = get_mr(0) as usize;
                let name = (unsafe { &*page }).payload().ok_or(REPLY_BAD_REQUEST)?;
                let text = core::str::from_utf8(name).map_err(|_| REPLY_BAD_REQUEST)?;
                if !(1..=PHOTO_MAX).contains(&total) || name.is_empty() || name.len() > 120
                    || text == "." || text == ".."
                    || text.chars().any(|c| c.is_control() || c == '/' || c == '\\')
                { return Err(REPLY_BAD_REQUEST); }
                let mut filename = [0u8; 120];
                let length = name.len();
                filename[..length].copy_from_slice(name);
                let id = self.library_commit_staged(total, &filename[..length])?;
                set_mr(0, id);
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_LIBRARY_CLONE, 2) => {
                let source_id = get_mr(0);
                let source_owner = get_mr(1);
                let name = (unsafe { &*page }).payload().ok_or(REPLY_BAD_REQUEST)?;
                let Ok(text) = core::str::from_utf8(name) else { return Err(REPLY_BAD_REQUEST); };
                if source_id == 0 || source_owner == 0 || source_owner == LIBRARY_OWNER
                    || name.is_empty() || name.len() > 120
                    || text.chars().any(|c| c.is_control() || c == '/' || c == '\\')
                    || text == "." || text == ".."
                { return Err(REPLY_BAD_REQUEST); }
                let mut filename = [0u8; 120];
                let name_len = name.len();
                filename[..name_len].copy_from_slice(name);
                let total = self.stage_attachment_copy(source_id, source_owner)?;
                let file_id = self.library_commit_staged(total, &filename[..name_len])?;
                set_mr(0, file_id);
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_LIBRARY_NEXT, 1) => {
                let mut cursor = get_mr(0);
                let end = self.history_end(self.block.capacity_blocks()?)?;
                while cursor < end {
                    cursor += 1;
                    set_mr(0, cursor);
                    let reply = match self.history_request(MessageInfo::new(REQUEST_HISTORY_READ, 1)) {
                        Ok(reply) => reply,
                        Err(REPLY_NO_BLOB) => continue,
                        Err(err) => return Err(err),
                    };
                    if reply.label() != REPLY_OK || reply.count() != 2 { return Err(REPLY_CORRUPT); }
                    if get_mr(0) != LIBRARY_OWNER { continue; }
                    let entry = (unsafe { &*page }).payload().ok_or(REPLY_CORRUPT)?;
                    if entry.len() < 19 { return Err(REPLY_CORRUPT); }
                    let file_id = u64::from_le_bytes(entry[8..16].try_into().unwrap());
                    let (len, name_len, header) = if &entry[..8] == b"NXFILE1\0" {
                        (u16::from_le_bytes(entry[16..18].try_into().unwrap()) as usize, entry[18] as usize, 19)
                    } else if &entry[..8] == b"NXFILE2\0" && entry.len() >= 21 {
                        (u32::from_le_bytes(entry[16..20].try_into().unwrap()) as usize, entry[20] as usize, 21)
                    } else { return Err(REPLY_CORRUPT); };
                    if file_id == 0 || !(1..=PHOTO_MAX).contains(&len) || name_len == 0
                        || name_len > 120 || entry.len() != header + name_len
                        || core::str::from_utf8(&entry[header..]).is_err()
                    { return Err(REPLY_CORRUPT); }
                    // Deleted entries remain in the append-only index. Only
                    // expose an entry while its exact allocation is live.
                    let base = self.message_photo_block(file_id & u64::from(u32::MAX))?;
                    let (raw, record) = self.read_photo_block(base, 0)?;
                    let Some(record) = record else { continue; };
                    if record.generation != file_id { continue; }
                    if record.payload_length < PHOTO_HEADER { return Err(REPLY_CORRUPT); }
                    let start = vault_blob_record::CIPHERTEXT_OFFSET;
                    let (owner, stored_len, _) = photo_header(&raw[start..start + record.payload_length]).ok_or(REPLY_CORRUPT)?;
                    if owner != LIBRARY_OWNER || stored_len != len
                    { return Err(REPLY_CORRUPT); }
                    let mut name = [0u8; 120];
                    name[..name_len].copy_from_slice(&entry[header..]);
                    if !(unsafe { &mut *page }).set_payload(&name[..name_len]) { return Err(REPLY_CORRUPT); }
                    set_mr(0, cursor);
                    set_mr(1, file_id);
                    set_mr(2, len as u64);
                    return Ok(MessageInfo::new(REPLY_OK, 3));
                }
                Err(REPLY_NO_BLOB)
            }
            (REQUEST_LIBRARY_DELETE, 1) => {
                let id = get_mr(0);
                if id == 0 { return Err(REPLY_BAD_REQUEST); }
                let base = self.message_photo_block(id & u64::from(u32::MAX))?;
                let (raw, record) = self.read_photo_block(base, 0)?;
                let record = record.filter(|r| r.generation == id && r.payload_length >= PHOTO_HEADER)
                    .ok_or(REPLY_NO_BLOB)?;
                let start = vault_blob_record::CIPHERTEXT_OFFSET;
                let (owner, total, _) = photo_header(&raw[start..start + record.payload_length]).ok_or(REPLY_CORRUPT)?;
                if owner != LIBRARY_OWNER { return Err(REPLY_DENIED); }
                if !(1..=PHOTO_MAX).contains(&total) { return Err(REPLY_CORRUPT); }
                // Invalidate the commit record before reclaiming continuations.
                self.write_disk_block(base, &[0; BLOCK_SIZE])?;
                self.block.flush()?;
                for segment in 1..total.div_ceil(PHOTO_BLOCKS * PHOTO_CHUNK) {
                    self.write_disk_block(base + segment * PHOTO_BLOCKS, &[0; BLOCK_SIZE])?;
                }
                self.block.flush()?;
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            _ => Err(REPLY_BAD_REQUEST),
        }
    }

    fn history_end(&mut self, blocks: usize) -> Result<u64, MessageLabel> {
        let mut end = 0;
        for slot in 0..2 {
            let block = vault_history::sequence_block(blocks, slot).ok_or(REPLY_STORAGE)?;
            let mut raw = self.read_disk_block(block)?;
            if let Some(record) =
                vault_blob_record::decode(&self.crypto, vault_history::SEQUENCE_NAMESPACE, &mut raw)
                    .map_err(|_| REPLY_STORAGE)?
            {
                end = end.max(record.generation);
            }
        }
        Ok(end)
    }

    fn history_record_block(
        &self,
        blocks: usize,
        id: u64,
        photo_id: u64,
    ) -> Result<usize, MessageLabel> {
        let photo_end = if photo_id == 0 {
            MESSAGE_PHOTO_FIRST_BLOCK
        } else {
            self.message_photo_block(photo_id)? + PHOTO_BLOCKS
        };
        vault_history::record_block(blocks, id, photo_end).ok_or(REPLY_STORAGE)
    }

    fn history_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{REQUEST_HISTORY_APPEND, REQUEST_HISTORY_END, REQUEST_HISTORY_READ, REQUEST_HISTORY_READ_BATCH};
        let blocks = self.block.capacity_blocks()?;
        if blocks <= MESSAGE_PHOTO_FIRST_BLOCK + 2 {
            return Err(REPLY_STORAGE);
        }
        let input = get_mr(0);
        let end = self.history_end(blocks)?;
        match (request.label(), request.count()) {
            (REQUEST_HISTORY_END, 0) => {
                set_mr(0, end);
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_HISTORY_APPEND, 1) => {
                let page = self.transfer_page(MESSAGES_NAMESPACE)?;
                let event = (unsafe { &*page }).payload().ok_or(REPLY_BAD_REQUEST)?;
                if input == 0 || event.is_empty() || event.len() > vault_history::MAX_EVENT_SIZE {
                    return Err(REPLY_BAD_REQUEST);
                }
                let mut payload = [0; vault_blob_record::MAX_PAYLOAD_SIZE];
                let size = vault_history::OWNER_SIZE + event.len();
                payload[..vault_history::OWNER_SIZE].copy_from_slice(&input.to_le_bytes());
                payload[vault_history::OWNER_SIZE..size].copy_from_slice(event);
                let id = end.checked_add(1).ok_or(REPLY_CONFLICT)?;
                let (_, photo_end) = self.photo_sequence()?;
                let block = self.history_record_block(blocks, id, photo_end)?;

                // Persist the reservation before encrypting the event. A retry
                // can skip an ID after power loss without reusing its nonce.
                let sequence = vault_blob_record::encode(
                    &self.crypto,
                    vault_history::SEQUENCE_NAMESPACE,
                    id,
                    &[],
                    true,
                )
                .map_err(|_| REPLY_STORAGE)?
                .ok_or(REPLY_CORRUPT)?;
                let sequence_block = vault_history::sequence_block(blocks, (id % 2) as usize)
                    .ok_or(REPLY_STORAGE)?;
                self.write_disk_block(sequence_block, &sequence)?;
                self.block.flush()?;

                let record = vault_blob_record::encode(
                    &self.crypto,
                    vault_history::RECORD_NAMESPACE,
                    id,
                    &payload[..size],
                    true,
                )
                .map_err(|_| REPLY_STORAGE)?
                .ok_or(REPLY_CORRUPT)?;
                self.write_disk_block(block, &record)?;
                self.block.flush()?;
                set_mr(0, id);
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_HISTORY_READ, 1) => {
                if input == 0 || input > end {
                    return Err(REPLY_NO_BLOB);
                }
                let block = vault_history::record_block(blocks, input, MESSAGE_PHOTO_FIRST_BLOCK)
                    .ok_or(REPLY_CORRUPT)?;
                let mut raw = self.read_disk_block(block)?;
                // A reserved block without an authenticated record is an
                // interrupted append. Records never reuse a physical slot.
                let record = vault_blob_record::decode(
                    &self.crypto,
                    vault_history::RECORD_NAMESPACE,
                    &mut raw,
                )
                .map_err(|_| REPLY_STORAGE)?
                .ok_or(REPLY_NO_BLOB)?;
                if record.generation != input {
                    return Err(REPLY_CORRUPT);
                }
                let offset = vault_blob_record::CIPHERTEXT_OFFSET;
                let (owner, event) =
                    vault_history::split_event(&raw[offset..offset + record.payload_length])
                        .ok_or(REPLY_CORRUPT)?;
                let page = self.transfer_page(MESSAGES_NAMESPACE)?;
                if !(unsafe { &mut *page }).set_payload(event) {
                    return Err(REPLY_CORRUPT);
                }
                set_mr(0, owner);
                set_mr(1, event.len() as u64);
                Ok(MessageInfo::new(REPLY_OK, 2))
            }
            (REQUEST_HISTORY_READ_BATCH, 2) => {
                let requested_end = get_mr(1);
                if input == 0 || input > requested_end || requested_end > end {
                    return Err(REPLY_BAD_REQUEST);
                }
                let mut bytes = [0; vault_blob_record::MAX_PAYLOAD_SIZE];
                let mut writer = personal_os_rust_server::history_batch::Writer::new(&mut bytes);
                let last = requested_end.min(input.saturating_add(personal_os_rust_server::history_batch::MAX_ENTRIES as u64 - 1));
                for id in input..=last {
                    let block = vault_history::record_block(blocks, id, MESSAGE_PHOTO_FIRST_BLOCK).ok_or(REPLY_CORRUPT)?;
                    let mut raw = self.read_disk_block(block)?;
                    let record = vault_blob_record::decode(&self.crypto, vault_history::RECORD_NAMESPACE, &mut raw)
                        .map_err(|_| REPLY_STORAGE)?;
                    let fits = match record {
                        None => writer.push(0, None),
                        Some(record) => {
                            if record.generation != id { return Err(REPLY_CORRUPT); }
                            let offset = vault_blob_record::CIPHERTEXT_OFFSET;
                            let (owner, event) = vault_history::split_event(&raw[offset..offset + record.payload_length])
                                .ok_or(REPLY_CORRUPT)?;
                            if owner == 0 || event.is_empty() { return Err(REPLY_CORRUPT); }
                            writer.push(owner, Some(event))
                        }
                    };
                    if !fits { break; }
                }
                let count = writer.count();
                let payload = writer.bytes();
                let length = payload.len();
                let page = self.transfer_page(MESSAGES_NAMESPACE)?;
                if !(unsafe { &mut *page }).set_payload(payload) { return Err(REPLY_CORRUPT); }
                set_mr(0, count as u64);
                set_mr(1, length as u64);
                Ok(MessageInfo::new(REPLY_OK, 2))
            }
            _ => Err(REPLY_BAD_REQUEST),
        }
    }

    fn wallet_node_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{REQUEST_WALLET_NODE_READ, REQUEST_WALLET_NODE_WRITE};
        if !cfg!(any(feature = "utm-installation-vault-key", feature = "storage-test"))
            || self.wallet_identity_transfer.is_null() || request.count() != 0 {
            return Err(REPLY_DENIED);
        }
        let file = self.load_file_state(VaultFileId::WalletState)?;
        if file.exists && file.length as usize > BLOCK_SIZE { return Err(REPLY_CONFLICT); }
        let page = unsafe { &mut *self.wallet_identity_transfer };
        let mut payload = [0u8; 128];
        let length = if request.label() == REQUEST_WALLET_NODE_WRITE {
            let input = page.payload().filter(|input| !input.is_empty() && input.len() <= payload.len())
                .ok_or(REPLY_BAD_REQUEST)?;
            payload[..input.len()].copy_from_slice(input);
            input.len()
        } else { 0 };
        page.set_payload(&[]);
        let base = vault_file::WALLET_FIRST_BLOCK + 1;
        const DATA_NS: usize = 0x66;
        const SEQUENCE_NS: usize = 0x67;
        let mut active: Option<(usize, u64)> = None;
        let mut sequence = 0u64;
        for slot in 0..2 {
            let mut raw = self.read_disk_block(base + slot)?;
            if let Some(record) = vault_blob_record::decode(&self.crypto, DATA_NS, &mut raw)
                .map_err(|_| REPLY_STORAGE)? {
                if active.is_none_or(|(_, generation)| record.generation > generation) {
                    active = Some((slot, record.generation));
                }
            }
            let mut raw = self.read_disk_block(base + 2 + slot)?;
            if let Some(record) = vault_blob_record::decode(&self.crypto, SEQUENCE_NS, &mut raw)
                .map_err(|_| REPLY_STORAGE)? { sequence = sequence.max(record.generation); }
        }
        if active.is_some_and(|(_, generation)| generation > sequence) { return Err(REPLY_CORRUPT); }
        match request.label() {
            REQUEST_WALLET_NODE_READ => {
                let (slot, generation) = active.ok_or(REPLY_NO_BLOB)?;
                let mut raw = self.read_disk_block(base + slot)?;
                let record = vault_blob_record::decode(&self.crypto, DATA_NS, &mut raw)
                    .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
                if record.generation != generation || record.payload_length == 0 || record.payload_length > 128 {
                    return Err(REPLY_CORRUPT);
                }
                let start = vault_blob_record::CIPHERTEXT_OFFSET;
                if !page.set_payload(&raw[start..start + record.payload_length]) { return Err(REPLY_CORRUPT); }
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            REQUEST_WALLET_NODE_WRITE => {
                let next = sequence.checked_add(1).ok_or(REPLY_CONFLICT)?;
                let reserved = vault_blob_record::encode(&self.crypto, SEQUENCE_NS, next, &[], true)
                    .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
                self.write_disk_block(base + 2 + (next % 2) as usize, &reserved)?;
                self.block.flush()?;
                let encoded = vault_blob_record::encode(&self.crypto, DATA_NS, next, &payload[..length], true)
                    .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_BAD_REQUEST)?;
                self.write_disk_block(base + active.map_or(0, |(slot, _)| 1 - slot), &encoded)?;
                self.block.flush()?;
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            _ => Err(REPLY_BAD_REQUEST),
        }
    }

    fn wallet_tx_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{REQUEST_WALLET_TX_STAGE, REQUEST_WALLET_TX_COMMIT,
            REQUEST_WALLET_TX_READ, REQUEST_WALLET_TX_CLEAR, WALLET_TX_CHUNK, WALLET_TX_MAX};
        if !cfg!(any(feature = "utm-installation-vault-key", feature = "storage-test"))
            || self.wallet_identity_transfer.is_null()
        {
            return Err(REPLY_DENIED);
        }
        // Refuse to use this extent if an older raw wallet file occupies it.
        let file = self.load_file_state(VaultFileId::WalletState)?;
        if file.exists && file.length as usize > 5 * BLOCK_SIZE {
            return Err(REPLY_CONFLICT);
        }
        let page = self.wallet_identity_transfer;
        let base = vault_file::WALLET_FIRST_BLOCK + 5;
        let sequence_namespace = 0x65;
        let mut sequence = 0u64;
        for slot in 0..2 {
            let mut raw = self.read_disk_block(base + 8 + slot)?;
            if let Some(record) = vault_blob_record::decode(&self.crypto, sequence_namespace, &mut raw)
                .map_err(|_| REPLY_STORAGE)?
            {
                sequence = sequence.max(record.generation);
            }
        }
        match (request.label(), request.count()) {
            (REQUEST_WALLET_TX_STAGE, 1) => {
                let offset = get_mr(0) as usize;
                if offset == 0 {
                    self.wallet_tx_staged = 0;
                }
                let transfer = unsafe { &mut *page };
                let bytes = transfer.payload().ok_or(REPLY_BAD_REQUEST)?;
                if offset != self.wallet_tx_staged || bytes.is_empty()
                    || bytes.len() > WALLET_TX_CHUNK || bytes.len() > WALLET_TX_MAX - offset
                {
                    transfer.set_payload(&[]);
                    return Err(REPLY_BAD_REQUEST);
                }
                let length = bytes.len();
                self.wallet_tx_stage[offset..offset + length].copy_from_slice(bytes);
                self.wallet_tx_staged += length;
                transfer.set_payload(&[]);
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            (REQUEST_WALLET_TX_COMMIT, 1) | (REQUEST_WALLET_TX_CLEAR, 0)
            | (0x444, 1) if request.label() != 0x444 || cfg!(feature = "storage-test") => {
                let length = if request.label() == REQUEST_WALLET_TX_CLEAR { 0 } else { get_mr(0) as usize };
                if length > WALLET_TX_MAX || (length != 0 && length != self.wallet_tx_staged)
                    || (request.label() != REQUEST_WALLET_TX_CLEAR && length == 0)
                    || (request.label() == 0x444 && length <= WALLET_TX_CHUNK)
                {
                    return Err(REPLY_BAD_REQUEST);
                }
                let next = sequence.checked_add(1).ok_or(REPLY_CONFLICT)?;
                let reservation = vault_blob_record::encode(&self.crypto, sequence_namespace, next, &[], true)
                    .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
                self.write_disk_block(base + 8 + (next % 2) as usize, &reservation)?;
                self.block.flush()?;
                // A reservation can survive an interrupted write. Its parity
                // therefore does not identify the bank containing the last
                // committed transaction. Always write the other bank so any
                // number of failed attempts leave the committed record intact.
                let mut active: Option<(usize, u64)> = None;
                for bank in 0..2 {
                    let mut raw = self.read_disk_block(base + bank * 4)?;
                    if let Some(record) = vault_blob_record::decode(&self.crypto, 0x60, &mut raw)
                        .map_err(|_| REPLY_STORAGE)?
                    {
                        if active.is_none_or(|(_, generation)| record.generation > generation) {
                            active = Some((bank, record.generation));
                        }
                    }
                }
                let bank = base + active.map_or(0, |(bank, _)| 1 - bank) * 4;
                let parts = length.div_ceil(WALLET_TX_CHUNK).max(1);
                for part in (0..parts).rev() {
                    let offset = part * WALLET_TX_CHUNK;
                    let mut payload = [0u8; WALLET_TX_CHUNK + 2];
                    let data_len = length.saturating_sub(offset).min(WALLET_TX_CHUNK);
                    let used = if part == 0 {
                        payload[..2].copy_from_slice(&(length as u16).to_le_bytes());
                        payload[2..2 + data_len].copy_from_slice(&self.wallet_tx_stage[offset..offset + data_len]);
                        data_len + 2
                    } else {
                        payload[..data_len].copy_from_slice(&self.wallet_tx_stage[offset..offset + data_len]);
                        data_len
                    };
                    if part == 0 {
                        self.block.flush()?;
                    }
                    let encoded = vault_blob_record::encode(&self.crypto, 0x60 + part, next, &payload[..used], true)
                        .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
                    self.write_disk_block(bank + part, &encoded)?;
                    // Test-only power-loss simulation: stop after a non-commit
                    // chunk reaches disk, before the bank's commit marker.
                    if cfg!(feature = "storage-test") && request.label() == 0x444 {
                        self.block.flush()?;
                        return Ok(MessageInfo::new(REPLY_OK, 0));
                    }
                }
                self.block.flush()?;
                self.wallet_tx_stage.fill(0);
                self.wallet_tx_staged = 0;
                Ok(MessageInfo::new(REPLY_OK, 0))
            }
            (REQUEST_WALLET_TX_READ, 1) => {
                let part = get_mr(0) as usize;
                if part >= 4 {
                    return Err(REPLY_BAD_REQUEST);
                }
                let mut active = None;
                for bank in 0..2 {
                    let mut raw = self.read_disk_block(base + bank * 4)?;
                    if let Some(record) = vault_blob_record::decode(&self.crypto, 0x60, &mut raw)
                        .map_err(|_| REPLY_STORAGE)?
                    {
                        if active.is_none_or(|(_, generation)| record.generation > generation) {
                            active = Some((bank, record.generation));
                        }
                    }
                }
                let (bank, generation) = active.ok_or(REPLY_NO_BLOB)?;
                let mut raw = self.read_disk_block(base + bank * 4 + part)?;
                let record = vault_blob_record::decode(&self.crypto, 0x60 + part, &mut raw)
                    .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
                if record.generation != generation {
                    return Err(REPLY_CORRUPT);
                }
                let start = vault_blob_record::CIPHERTEXT_OFFSET;
                let mut payload = &raw[start..start + record.payload_length];
                if part == 0 {
                    if payload.len() < 2 {
                        return Err(REPLY_CORRUPT);
                    }
                    let length = u16::from_le_bytes(payload[..2].try_into().unwrap()) as usize;
                    if length == 0 {
                        return Err(REPLY_NO_BLOB);
                    }
                    set_mr(0, length as u64);
                    payload = &payload[2..];
                }
                if !unsafe { &mut *page }.set_payload(payload) {
                    return Err(REPLY_CORRUPT);
                }
                Ok(MessageInfo::new(REPLY_OK, if part == 0 { 1 } else { 0 }))
            }
            _ => Err(REPLY_BAD_REQUEST),
        }
    }

    fn wallet_identity_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{REQUEST_WALLET_IDENTITY_CREATE, REQUEST_WALLET_IDENTITY_READ, WALLET_IDENTITY_SIZE};
        use zeroize::Zeroizing;

        if !cfg!(any(feature = "utm-installation-vault-key", feature = "storage-test")) {
            return Err(REPLY_DENIED);
        }
        if self.wallet_identity_transfer.is_null() || request.count() != 0 {
            return Err(REPLY_BAD_REQUEST);
        }
        let file = self.load_file_state(VaultFileId::WalletState)?;
        if file.exists && file.length as usize > (vault_file::WALLET_BLOCKS - 1) * BLOCK_SIZE {
            return Err(REPLY_CONFLICT);
        }
        let transfer = unsafe { &mut *self.wallet_identity_transfer };
        let create = request.label() == REQUEST_WALLET_IDENTITY_CREATE;
        let mut identity = Zeroizing::new([0; WALLET_IDENTITY_SIZE]);
        let size_ok = if create {
            if let Some(input) = transfer.payload().filter(|input| input.len() == identity.len()) {
                identity.copy_from_slice(input);
                true
            } else {
                false
            }
        } else {
            true
        };
        transfer.set_payload(&[]);
        if !size_ok {
            return Err(REPLY_BAD_REQUEST);
        }
        let mut raw = Zeroizing::new(self.read_disk_block(WALLET_IDENTITY_BLOCK)?);
        let blank = raw.iter().all(|&byte| byte == 0);
        if create {
            if !blank {
                return Err(REPLY_CONFLICT);
            }
            let encoded = vault_blob_record::encode(
                &self.crypto, WALLET_IDENTITY_NAMESPACE, 1, &identity[..], true,
            ).map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_BAD_REQUEST)?;
            self.write_disk_block(WALLET_IDENTITY_BLOCK, &encoded)?;
            self.block.flush()?;
            return Ok(MessageInfo::new(REPLY_OK, 0));
        }
        if request.label() != REQUEST_WALLET_IDENTITY_READ {
            return Err(REPLY_BAD_REQUEST);
        }
        if blank {
            return Err(REPLY_NO_BLOB);
        }
        let record = vault_blob_record::decode(&self.crypto, WALLET_IDENTITY_NAMESPACE, &mut raw)
            .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
        if record.payload_length != WALLET_IDENTITY_SIZE || record.generation != 1 {
            return Err(REPLY_CORRUPT);
        }
        let offset = vault_blob_record::CIPHERTEXT_OFFSET;
        transfer.set_payload(&raw[offset..offset + WALLET_IDENTITY_SIZE]);
        Ok(MessageInfo::new(REPLY_OK, 0))
    }

    // Block zero is reserved for the PIN envelope only when the legacy wallet
    // file is absent. The old identity remains intact during this staging
    // step: removing it requires a separate, verified migration commit.
    fn wallet_pin_request(&mut self, request: MessageInfo) -> Result<MessageInfo, MessageLabel> {
        use vault_protocol::{REQUEST_WALLET_PIN_CREATE, REQUEST_WALLET_PIN_READ};
        use zeroize::Zeroizing;
        if !cfg!(any(feature = "utm-installation-vault-key", feature = "storage-test"))
            || self.wallet_identity_transfer.is_null() || request.count() != 0 {
            return Err(REPLY_DENIED);
        }
        let file = self.load_file_state(VaultFileId::WalletState)?;
        if file.exists { return Err(REPLY_CONFLICT); }
        let page = unsafe { &mut *self.wallet_identity_transfer };
        let mut payload = Zeroizing::new([0u8; personal_os_rust_server::wallet_pin::ENVELOPE_SIZE]);
        let create = request.label() == REQUEST_WALLET_PIN_CREATE;
        if create {
            let input = page.payload().filter(|p| p.len() == payload.len()).ok_or(REPLY_BAD_REQUEST)?;
            payload.copy_from_slice(input);
        }
        page.set_payload(&[]);
        let block = vault_file::WALLET_FIRST_BLOCK;
        const PIN_NS: usize = 0x68;
        let mut raw = Zeroizing::new(self.read_disk_block(block)?);
        if create {
            if raw.iter().any(|&b| b != 0) { return Err(REPLY_CONFLICT); }
            let encoded = vault_blob_record::encode(&self.crypto, PIN_NS, 1, &payload[..], true)
                .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_BAD_REQUEST)?;
            self.write_disk_block(block, &encoded)?;
            self.block.flush()?;
            return Ok(MessageInfo::new(REPLY_OK, 0));
        }
        if request.label() != REQUEST_WALLET_PIN_READ { return Err(REPLY_BAD_REQUEST); }
        if raw.iter().all(|&b| b == 0) { return Err(REPLY_NO_BLOB); }
        let record = vault_blob_record::decode(&self.crypto, PIN_NS, &mut raw)
            .map_err(|_| REPLY_STORAGE)?.ok_or(REPLY_CORRUPT)?;
        if record.generation != 1 || record.payload_length != payload.len() { return Err(REPLY_CORRUPT); }
        let offset = vault_blob_record::CIPHERTEXT_OFFSET;
        if !page.set_payload(&raw[offset..offset + payload.len()]) { return Err(REPLY_CORRUPT); }
        Ok(MessageInfo::new(REPLY_OK, 0))
    }

    fn handle_wallet_file_request(&mut self, request: MessageInfo) -> MessageInfo {
        let only_wallet = |file: VaultFileId| -> bool { file == VaultFileId::WalletState };
        match (request.label(), request.count()) {
            (REQUEST_FILE_READ, 3) => {
                let Some(file) = VaultFileId::from_u32(get_mr(0) as u32).filter(|&f| only_wallet(f))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                let (Ok(offset), Ok(length)) =
                    (usize::try_from(get_mr(1)), usize::try_from(get_mr(2)))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match self.read_file(file, offset, length, WALLET_APP_CHANNEL) {
                    Ok(length) => {
                        set_mr(0, length as MessageRegisterValue);
                        MessageInfo::new(REPLY_OK, 1)
                    }
                    Err(label) => MessageInfo::new(label, 0),
                }
            }
            (REQUEST_FILE_WRITE, 3) => {
                let Some(file) = VaultFileId::from_u32(get_mr(0) as u32).filter(|&f| only_wallet(f))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                let (Ok(offset), Ok(length)) =
                    (usize::try_from(get_mr(1)), usize::try_from(get_mr(2)))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match self.write_file(file, offset, length, WALLET_APP_CHANNEL) {
                    Ok(()) => MessageInfo::new(REPLY_OK, 0),
                    Err(label) => MessageInfo::new(label, 0),
                }
            }
            (REQUEST_FILE_TRUNCATE, 2) => {
                let Some(file) = VaultFileId::from_u32(get_mr(0) as u32).filter(|&f| only_wallet(f))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                let Ok(length) = usize::try_from(get_mr(1)) else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match self.truncate_file(file, length) {
                    Ok(()) => MessageInfo::new(REPLY_OK, 0),
                    Err(label) => MessageInfo::new(label, 0),
                }
            }
            (REQUEST_FILE_SYNC, 1) => {
                let Some(file) = VaultFileId::from_u32(get_mr(0) as u32).filter(|&f| only_wallet(f))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match self.sync_file(file) {
                    Ok(()) => MessageInfo::new(REPLY_OK, 0),
                    Err(label) => MessageInfo::new(label, 0),
                }
            }
            (REQUEST_FILE_SIZE, 1) => {
                let Some(file) = VaultFileId::from_u32(get_mr(0) as u32).filter(|&f| only_wallet(f))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match self.file_size(file) {
                    Ok(state) => {
                        set_mr(0, state.length as MessageRegisterValue);
                        set_mr(1, MessageRegisterValue::from(state.exists));
                        MessageInfo::new(REPLY_OK, 2)
                    }
                    Err(label) => MessageInfo::new(label, 0),
                }
            }
            (REQUEST_FILE_DELETE, 1) => {
                let Some(file) = VaultFileId::from_u32(get_mr(0) as u32).filter(|&f| only_wallet(f))
                else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match self.delete_file(file) {
                    Ok(()) => MessageInfo::new(REPLY_OK, 0),
                    Err(label) => MessageInfo::new(label, 0),
                }
            }
            _ => MessageInfo::new(REPLY_BAD_REQUEST, 0),
        }
    }
}

impl Handler for Vault {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if !channels.contains(self.block.channel) {
            debug_println!("FAIL: unexpected disk-vault notification");
        }
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        #[cfg(feature = "utm-device-lock")]
        if channel == vault_protocol::DEVICE_LOCK_CHANNEL {
            if let Err(label) = self.load_from_disk() { return Ok(MessageInfo::new(label, 0)); }
            let page = match self.transfer_page(SYSTEM_NAMESPACE) {
                Ok(page) => page,
                Err(label) => return Ok(MessageInfo::new(label, 0)),
            };
            let (generation, value, previous) = match self.system_lock_state() {
                Ok(state) => state,
                Err(label) => return Ok(MessageInfo::new(label, 0)),
            };
            if request.label() == vault_protocol::REQUEST_DEVICE_LOCK_READ && request.count() == 0 {
                unsafe { &mut *page }.set_payload(&[]);
                set_mr(0, generation);
                return match previous {
                    Some(settings) if unsafe { &mut *page }.set_payload(&settings) => Ok(MessageInfo::new(REPLY_OK, 1)),
                    Some(_) => Ok(MessageInfo::new(REPLY_CORRUPT, 0)),
                    None => Ok(MessageInfo::new(REPLY_NO_BLOB, 1)),
                };
            }
            if request.label() == vault_protocol::REQUEST_DEVICE_LOCK_WRITE && request.count() == 1 {
                let expected = get_mr(0);
                let mut settings = [0u8; personal_os_rust_server::device_lock::SETTINGS_SIZE];
                let input = unsafe { &*page }.payload();
                if input.is_none_or(|input| input.len() != settings.len()) { return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0)); }
                settings.copy_from_slice(input.unwrap());
                unsafe { &mut *page }.set_payload(&[]);
                let payload = match personal_os_rust_server::system_lock_record::encode_system(value, &settings) {
                    Ok(payload) => payload,
                    Err(_) => return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0)),
                };
                return match self.write_blob(SYSTEM_NAMESPACE, expected, &payload) {
                    Ok(next) => { set_mr(0, next); Ok(MessageInfo::new(REPLY_OK, 1)) }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                };
            }
            return Ok(MessageInfo::new(REPLY_DENIED, 0));
        }
        #[cfg(feature = "utm-photo-reader")]
        if channel == vault_protocol::PHOTO_READER_CHANNEL {
            if request.label() != vault_protocol::REQUEST_PHOTO_READ || request.count() != 3 {
                return Ok(MessageInfo::new(REPLY_DENIED, 0));
            }
            let id = get_mr(0);
            let part = get_mr(1);
            let owner = get_mr(2);
            if id == 0 || owner == 0 || owner == vault_protocol::LIBRARY_OWNER || self.photo_read_transfer.is_null() {
                return Ok(MessageInfo::new(REPLY_DENIED, 0));
            }
            if let Err(label) = self.load_from_disk() {
                return Ok(MessageInfo::new(label, 0));
            }
            unsafe { (&mut *self.photo_read_transfer).set_payload(&[]); }
            set_mr(0, id);
            set_mr(1, part);
            return Ok(self.photo_request_on_page(
                MessageInfo::new(vault_protocol::REQUEST_PHOTO_READ, 2), self.photo_read_transfer, Some(owner),
            ).unwrap_or_else(|label| MessageInfo::new(label, 0)));
        }
        if request.label() == REQUEST_FILE_READ {
            debug_println!("VAULT|FILE_READ|channel={:?} file={}", channel, get_mr(0));
        }
        if channel == WALLET_APP_CHANNEL {
            if let Err(label) = self.load_from_disk() {
                return Ok(MessageInfo::new(label, 0));
            }
            if matches!(request.label(), vault_protocol::REQUEST_WALLET_NODE_READ | vault_protocol::REQUEST_WALLET_NODE_WRITE) {
                return Ok(self.wallet_node_request(request).unwrap_or_else(|label| MessageInfo::new(label, 0)));
            }
            if matches!(request.label(), vault_protocol::REQUEST_WALLET_TX_STAGE
                | vault_protocol::REQUEST_WALLET_TX_COMMIT | vault_protocol::REQUEST_WALLET_TX_READ
                | vault_protocol::REQUEST_WALLET_TX_CLEAR)
                || (cfg!(feature = "storage-test") && request.label() == 0x444)
            {
                return Ok(self.wallet_tx_request(request)
                    .unwrap_or_else(|label| MessageInfo::new(label, 0)));
            }
            if matches!(request.label(), vault_protocol::REQUEST_WALLET_IDENTITY_READ
                | vault_protocol::REQUEST_WALLET_IDENTITY_CREATE)
            {
                return Ok(self.wallet_identity_request(request)
                    .unwrap_or_else(|label| MessageInfo::new(label, 0)));
            }
            if matches!(request.label(), vault_protocol::REQUEST_WALLET_PIN_READ
                | vault_protocol::REQUEST_WALLET_PIN_CREATE) {
                return Ok(self.wallet_pin_request(request)
                    .unwrap_or_else(|label| MessageInfo::new(label, 0)));
            }
            return Ok(self.handle_wallet_file_request(request));
        }
        let namespace = if channel == vault_protocol::OWNER_CHANNEL {
            SYSTEM_NAMESPACE
        } else if channel == NOTES_APP_CHANNEL {
            NOTES_NAMESPACE
        } else if channel == CONTACTS_APP_CHANNEL {
            CONTACTS_NAMESPACE
        } else if channel == CALENDAR_APP_CHANNEL {
            CALENDAR_NAMESPACE
        } else if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL {
            MESSAGES_NAMESPACE
        } else {
            return Ok(MessageInfo::new(REPLY_DENIED, 0));
        };
        if let Err(label) = self.load_from_disk() {
            return Ok(MessageInfo::new(label, 0));
        }

        if channel == MESSAGES_APP_CHANNEL
            && matches!(request.label(), vault_protocol::REQUEST_LIBRARY_CLONE | vault_protocol::REQUEST_LIBRARY_NEXT | vault_protocol::REQUEST_LIBRARY_DELETE | vault_protocol::REQUEST_LIBRARY_IMPORT | vault_protocol::REQUEST_LIBRARY_COPY_TO_CHAT)
        {
            return Ok(self.library_request(request).unwrap_or_else(|label| MessageInfo::new(label, 0)));
        }

        if channel == MESSAGES_APP_CHANNEL
            && matches!(request.label(), vault_protocol::REQUEST_HISTORY_APPEND
                | vault_protocol::REQUEST_HISTORY_READ | vault_protocol::REQUEST_HISTORY_END
                | vault_protocol::REQUEST_HISTORY_READ_BATCH)
        {
            if request.label() == vault_protocol::REQUEST_HISTORY_APPEND && get_mr(0) == vault_protocol::LIBRARY_OWNER {
                return Ok(MessageInfo::new(REPLY_DENIED, 0));
            }
            return Ok(self.history_request(request).unwrap_or_else(|label| MessageInfo::new(label, 0)));
        }

        if channel == MESSAGES_APP_CHANNEL
            && matches!(request.label(), vault_protocol::REQUEST_PHOTO_STAGE
                | vault_protocol::REQUEST_PHOTO_COMMIT | vault_protocol::REQUEST_PHOTO_READ
                | vault_protocol::REQUEST_PHOTO_PURGE_CHAT | vault_protocol::REQUEST_ATTACHMENT_COMMIT)
        {
            if matches!(request.label(), vault_protocol::REQUEST_PHOTO_COMMIT
                | vault_protocol::REQUEST_ATTACHMENT_COMMIT | vault_protocol::REQUEST_PHOTO_PURGE_CHAT)
                && get_mr(0) == vault_protocol::LIBRARY_OWNER
            {
                return Ok(MessageInfo::new(REPLY_DENIED, 0));
            }
            return Ok(self.photo_request(request).unwrap_or_else(|label| MessageInfo::new(label, 0)));
        }

        match (request.label(), request.count()) {
            (REQUEST_READ, 0) => match self.active_slot(namespace) {
                Some((_index, record)) => {
                    #[cfg(feature = "utm-device-lock")]
                    if namespace == SYSTEM_NAMESPACE {
                        return match self.system_lock_state() {
                            Ok((generation, value, _)) => { set_mr(0, generation); set_mr(1, value); Ok(MessageInfo::new(REPLY_OK, 2)) }
                            Err(label) => Ok(MessageInfo::new(label, 0)),
                        };
                    }
                    set_mr(0, record.generation);
                    set_mr(1, if record.is_blob { 0 } else { record.value });
                    Ok(MessageInfo::new(REPLY_OK, 2))
                }
                None => Ok(MessageInfo::new(REPLY_CORRUPT, 0)),
            },
            (REQUEST_WRITE, 2) => {
                let expected = get_mr(0);
                let value = get_mr(1);
                #[cfg(feature = "utm-device-lock")]
                if namespace == SYSTEM_NAMESPACE {
                    let settings = match self.system_lock_state() {
                        Ok((_, _, settings)) => settings,
                        Err(label) => return Ok(MessageInfo::new(label, 0)),
                    };
                    if let Some(settings) = settings {
                        let payload = match personal_os_rust_server::system_lock_record::encode_system(value, &settings) {
                            Ok(payload) => payload,
                            Err(_) => return Ok(MessageInfo::new(REPLY_CORRUPT, 0)),
                        };
                        return match self.write_blob(namespace, expected, &payload) {
                            Ok(generation) => { set_mr(0, generation); Ok(MessageInfo::new(REPLY_OK, 1)) }
                            Err(label) => Ok(MessageInfo::new(label, 0)),
                        };
                    }
                }
                match self.write_record(namespace, expected, value, WriteMode::Complete) {
                    Ok(generation) => {
                        set_mr(0, generation);
                        Ok(MessageInfo::new(REPLY_OK, 1))
                    }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_READ_BLOB, 0) => match self.active_slot(namespace) {
                Some((index, record)) if record.is_blob => {
                    match self.read_blob_to_transfer(namespace, index) {
                        Ok(length) => {
                            set_mr(0, record.generation);
                            set_mr(1, length as MessageRegisterValue);
                            Ok(MessageInfo::new(REPLY_OK, 2))
                        }
                        Err(label) => Ok(MessageInfo::new(label, 0)),
                    }
                }
                Some(_) => Ok(MessageInfo::new(REPLY_NO_BLOB, 0)),
                None => Ok(MessageInfo::new(REPLY_CORRUPT, 0)),
            },
            (REQUEST_WRITE_BLOB, 2) => {
                if namespace == SYSTEM_NAMESPACE { return Ok(MessageInfo::new(REPLY_DENIED, 0)); }
                let expected = get_mr(0);
                let length = get_mr(1) as usize;
                let page = match self.transfer_page(namespace) {
                    Ok(page) => unsafe { &*page },
                    Err(label) => return Ok(MessageInfo::new(label, 0)),
                };
                let Some(payload) = page.payload() else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                if payload.len() != length {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                }
                match self.write_blob(namespace, expected, payload) {
                    Ok(generation) => {
                        set_mr(0, generation);
                        Ok(MessageInfo::new(REPLY_OK, 1))
                    }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_FILE_READ, 3)
                if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL =>
            {
                let Some(file) = u32::try_from(get_mr(0)).ok().and_then(VaultFileId::from_u32)
                    .filter(|file| *file != VaultFileId::WalletState)
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                let (Ok(offset), Ok(length)) =
                    (usize::try_from(get_mr(1)), usize::try_from(get_mr(2)))
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                match self.read_file(file, offset, length, channel) {
                    Ok(length) => {
                        set_mr(0, length as MessageRegisterValue);
                        Ok(MessageInfo::new(REPLY_OK, 1))
                    }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_FILE_WRITE, 3)
                if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL =>
            {
                let Some(file) = u32::try_from(get_mr(0)).ok().and_then(VaultFileId::from_u32)
                    .filter(|file| *file != VaultFileId::WalletState)
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                let (Ok(offset), Ok(length)) =
                    (usize::try_from(get_mr(1)), usize::try_from(get_mr(2)))
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                match self.write_file(file, offset, length, channel) {
                    Ok(()) => Ok(MessageInfo::new(REPLY_OK, 0)),
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_FILE_TRUNCATE, 2)
                if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL =>
            {
                let Some(file) = u32::try_from(get_mr(0)).ok().and_then(VaultFileId::from_u32)
                    .filter(|file| *file != VaultFileId::WalletState)
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                let Ok(length) = usize::try_from(get_mr(1)) else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                match self.truncate_file(file, length) {
                    Ok(()) => Ok(MessageInfo::new(REPLY_OK, 0)),
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_FILE_SYNC, 1)
                if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL =>
            {
                let Some(file) = u32::try_from(get_mr(0)).ok().and_then(VaultFileId::from_u32)
                    .filter(|file| *file != VaultFileId::WalletState)
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                match self.sync_file(file) {
                    Ok(()) => Ok(MessageInfo::new(REPLY_OK, 0)),
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_FILE_SIZE, 1)
                if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL =>
            {
                let Some(file) = u32::try_from(get_mr(0)).ok().and_then(VaultFileId::from_u32)
                    .filter(|file| *file != VaultFileId::WalletState)
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                match self.file_size(file) {
                    Ok(state) => {
                        set_mr(0, state.length as MessageRegisterValue);
                        set_mr(1, MessageRegisterValue::from(state.exists));
                        Ok(MessageInfo::new(REPLY_OK, 2))
                    }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_FILE_DELETE, 1)
                if channel == MESSAGES_APP_CHANNEL || channel == SIMPLEX_AGENT_CHANNEL =>
            {
                let Some(file) = u32::try_from(get_mr(0)).ok().and_then(VaultFileId::from_u32)
                    .filter(|file| *file != VaultFileId::WalletState)
                else {
                    return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
                };
                match self.delete_file(file) {
                    Ok(()) => Ok(MessageInfo::new(REPLY_OK, 0)),
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            (REQUEST_NAMESPACE_TEST, 0) => match self.run_namespace_test(namespace) {
                Ok((generation, value)) => {
                    set_mr(0, generation);
                    set_mr(1, value);
                    Ok(MessageInfo::new(REPLY_NAMESPACE_TEST, 2))
                }
                Err(label) => Ok(MessageInfo::new(label, 0)),
            },
            #[cfg(feature = "storage-test")]
            (REQUEST_TEST_ADVANCE, 0) => match self.advance_test_interruption() {
                Ok(_) => Ok(MessageInfo::new(REPLY_OK, 2)),
                Err(label) => Ok(MessageInfo::new(label, 0)),
            },
            #[cfg(feature = "storage-test")]
            (REQUEST_TEST_ADVANCE, 1) => {
                match self.start_flush_interruption(namespace, get_mr(0)) {
                    Ok(generation) => {
                        set_mr(0, generation);
                        Ok(MessageInfo::new(REPLY_OK, 1))
                    }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            _ => Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0)),
        }
    }
}

struct BlockClient {
    storage_info: *mut u8,
    request_queue: *mut u8,
    response_queue: *mut u8,
    capacity: usize,
    data_size: usize,
    data: *mut u8,
    channel: Channel,
    next_id: u32,
}

impl BlockClient {
    fn new(config: BlockClientConfig) -> Self {
        Self {
            storage_info: config.virt.storage_info.vaddr,
            request_queue: config.virt.request_queue.vaddr,
            response_queue: config.virt.response_queue.vaddr,
            capacity: config.virt.num_buffers as usize,
            data_size: config.data.size as usize,
            data: config.data.vaddr,
            channel: Channel::new(config.virt.id as usize),
            next_id: 0,
        }
    }

    fn wait_until_ready(&self) -> Result<(), MessageLabel> {
        // blk_storage_info_t.ready follows serial_number[64] and read_only.
        let ready = unsafe { self.storage_info.add(65) };
        for _ in 0..MAX_WAIT_ITERATIONS {
            if unsafe { ptr::read_volatile(ready) } != 0 {
                fence(Ordering::Acquire);
                return Ok(());
            }
            sel4::r#yield();
        }
        Err(REPLY_STORAGE)
    }

    fn flush(&mut self) -> Result<(), MessageLabel> {
        self.transfer(BLOCK_FLUSH, 0, 0)
    }

    fn capacity_blocks(&self) -> Result<usize, MessageLabel> {
        self.wait_until_ready()?;
        if self.storage_info.is_null() {
            return Err(REPLY_STORAGE);
        }
        // blk_storage_info_t.capacity is an aligned u64 at byte offset 80 and
        // counts sDDF's 4 KiB transfer blocks.
        fence(Ordering::Acquire);
        let capacity = unsafe { ptr::read_volatile(self.storage_info.add(80).cast::<u64>()) };
        usize::try_from(capacity).map_err(|_| REPLY_STORAGE)
    }

    fn transfer(&mut self, code: u32, block_number: usize, count: u16) -> Result<(), MessageLabel> {
        let data_offset = block_number.checked_mul(BLOCK_SIZE).ok_or(REPLY_STORAGE)?;
        self.transfer_from(code, block_number, data_offset, count)
    }

    fn transfer_at(
        &mut self,
        code: u32,
        block_number: usize,
        data_offset: usize,
        count: u16,
    ) -> Result<(), MessageLabel> {
        self.transfer_from(code, block_number, data_offset, count)
    }

    fn transfer_from(
        &mut self,
        code: u32,
        block_number: usize,
        data_offset: usize,
        count: u16,
    ) -> Result<(), MessageLabel> {
        self.wait_until_ready()?;
        if self.capacity == 0 {
            return Err(REPLY_STORAGE);
        }
        let data_end = data_offset
            .checked_add(
                (count as usize)
                    .checked_mul(BLOCK_SIZE)
                    .ok_or(REPLY_STORAGE)?,
            )
            .ok_or(REPLY_STORAGE)?;
        if data_end > self.data_size {
            return Err(REPLY_STORAGE);
        }
        let request_header = self.request_queue.cast::<RequestQueueHeader>();
        let head = unsafe { ptr::read_volatile(ptr::addr_of!((*request_header).head)) };
        let tail = unsafe { ptr::read_volatile(ptr::addr_of!((*request_header).tail)) };
        if tail.wrapping_sub(head) >= self.capacity as u32 {
            return Err(REPLY_STORAGE);
        }

        let request_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let buffer_base = unsafe {
            self.request_queue
                .add(core::mem::size_of::<RequestQueueHeader>())
                .cast::<BlockRequest>()
        };
        let request = unsafe { buffer_base.add(tail as usize % self.capacity) };
        unsafe {
            ptr::write_volatile(
                request,
                BlockRequest {
                    code,
                    io_or_offset: data_offset as u64,
                    block_number: block_number as u64,
                    count,
                    id: request_id,
                },
            );
        }
        fence(Ordering::Release);
        unsafe {
            ptr::write_volatile(
                ptr::addr_of_mut!((*request_header).tail),
                tail.wrapping_add(1),
            );
        }
        fence(Ordering::Release);
        self.channel.notify();

        let response_header = self.response_queue.cast::<ResponseQueueHeader>();
        let response_base = unsafe {
            self.response_queue
                .add(core::mem::size_of::<ResponseQueueHeader>())
                .cast::<BlockResponse>()
        };
        for _ in 0..MAX_WAIT_ITERATIONS {
            let response_head =
                unsafe { ptr::read_volatile(ptr::addr_of!((*response_header).head)) };
            let response_tail =
                unsafe { ptr::read_volatile(ptr::addr_of!((*response_header).tail)) };
            if response_tail != response_head {
                fence(Ordering::Acquire);
                let response = unsafe {
                    ptr::read_volatile(response_base.add(response_head as usize % self.capacity))
                };
                unsafe {
                    ptr::write_volatile(
                        ptr::addr_of_mut!((*response_header).head),
                        response_head.wrapping_add(1),
                    );
                }
                if response.id != request_id
                    || response.status != BLK_RESP_OK
                    || response.success_count != count
                {
                    return Err(REPLY_STORAGE);
                }
                return Ok(());
            }
            sel4::r#yield();
        }
        Err(REPLY_STORAGE)
    }
}
