#![no_std]
#![no_main]

mod vault_protocol;

use core::{
    ptr,
    sync::atomic::{Ordering, fence},
};
use personal_os_rust_server::vault_blob_record::{self, DecodedBlob};
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use personal_os_rust_server::vault_crypto::DevelopmentCryptoProvider;
use personal_os_rust_server::vault_record::{self, DecodedRecord, RecordFormat, SLOT_SIZE};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, get_mr, memory_region_symbol, protection_domain, set_mr,
};
#[cfg(feature = "storage-test")]
use vault_protocol::REQUEST_TEST_ADVANCE;
use vault_protocol::{
    CALENDAR_APP_CHANNEL, CALENDAR_NAMESPACE, CONTACTS_APP_CHANNEL, CONTACTS_NAMESPACE,
    MESSAGES_APP_CHANNEL, MESSAGES_NAMESPACE, NAMESPACE_COUNT, NOTES_APP_CHANNEL,
    NOTES_NAMESPACE, REPLY_BAD_REQUEST, REPLY_CONFLICT, REPLY_CORRUPT, REPLY_DENIED,
    REPLY_NAMESPACE_TEST, REPLY_NO_BLOB, REPLY_OK, REPLY_STORAGE, REQUEST_NAMESPACE_TEST,
    REQUEST_READ, REQUEST_READ_BLOB, REQUEST_WRITE, REQUEST_WRITE_BLOB, SYSTEM_NAMESPACE,
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
const MAX_WAIT_ITERATIONS: usize = 1_000_000;

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
    loaded: u64,
}

#[protection_domain]
fn init() -> Vault {
    let config = unsafe { ptr::read_volatile(ptr::addr_of!(BLOCK_CONFIG)) };
    let crypto = DevelopmentCryptoProvider::new();
    let state = memory_region_symbol!(vault_private_start: *mut VaultState).as_ptr();
    let notes_transfer = memory_region_symbol!(notes_transfer: *mut BlobTransferPage).as_ptr();
    let contacts_transfer =
        memory_region_symbol!(contacts_transfer: *mut BlobTransferPage).as_ptr();
    let calendar_transfer =
        memory_region_symbol!(calendar_transfer: *mut BlobTransferPage).as_ptr();
    let messages_transfer =
        memory_region_symbol!(messages_transfer: *mut BlobTransferPage).as_ptr();
    unsafe {
        ptr::write_volatile(
            state,
            VaultState {
                slots: [[RecordSlot::INVALID; SLOT_COUNT]; NAMESPACE_COUNT],
                loaded: 0,
            },
        );
    }
    debug_println!("VAULT: disk-backed service initialized");
    Vault {
        state,
        block: BlockClient::new(config),
        crypto,
        blob_transfer: [
            ptr::null_mut(),
            notes_transfer,
            contacts_transfer,
            calendar_transfer,
            messages_transfer,
        ],
    }
}

struct Vault {
    state: *mut VaultState,
    block: BlockClient,
    crypto: DevelopmentCryptoProvider,
    blob_transfer: [*mut BlobTransferPage; NAMESPACE_COUNT],
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

    fn transfer_page(&self, namespace: usize) -> Result<*mut BlobTransferPage, MessageLabel> {
        let page = self
            .blob_transfer
            .get(namespace)
            .copied()
            .ok_or(REPLY_DENIED)?;
        if page.is_null() {
            return Err(REPLY_DENIED);
        }
        Ok(page)
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

    fn write_blob(
        &mut self,
        namespace: usize,
        expected_generation: MessageRegisterValue,
        payload: &[u8],
    ) -> Result<MessageRegisterValue, MessageLabel> {
        if namespace == SYSTEM_NAMESPACE || payload.len() > vault_blob_record::MAX_PAYLOAD_SIZE {
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
        let namespace = if channel == vault_protocol::OWNER_CHANNEL {
            SYSTEM_NAMESPACE
        } else if channel == NOTES_APP_CHANNEL {
            NOTES_NAMESPACE
        } else if channel == CONTACTS_APP_CHANNEL {
            CONTACTS_NAMESPACE
        } else if channel == CALENDAR_APP_CHANNEL {
            CALENDAR_NAMESPACE
        } else if channel == MESSAGES_APP_CHANNEL {
            MESSAGES_NAMESPACE
        } else {
            return Ok(MessageInfo::new(REPLY_DENIED, 0));
        };
        if let Err(label) = self.load_from_disk() {
            return Ok(MessageInfo::new(label, 0));
        }

        match (request.label(), request.count()) {
            (REQUEST_READ, 0) => match self.active_slot(namespace) {
                Some((_index, record)) => {
                    set_mr(0, record.generation);
                    set_mr(1, if record.is_blob { 0 } else { record.value });
                    Ok(MessageInfo::new(REPLY_OK, 2))
                }
                None => Ok(MessageInfo::new(REPLY_CORRUPT, 0)),
            },
            (REQUEST_WRITE, 2) => {
                let expected = get_mr(0);
                let value = get_mr(1);
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

    fn transfer(&mut self, code: u32, block_number: usize, count: u16) -> Result<(), MessageLabel> {
        self.wait_until_ready()?;
        if self.capacity == 0 {
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
                    io_or_offset: (block_number * BLOCK_SIZE) as u64,
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
