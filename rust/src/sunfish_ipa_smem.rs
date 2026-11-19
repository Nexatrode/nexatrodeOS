//! One-attempt owner for the cold Sunfish IPA SMEM allocation.
//!
//! This retains its serialized metadata/lock view through success or failure.
//! It accepts only the empty host-1 partition measured on this device, not an
//! arbitrary allocator or an existing allocation. Four fixed metadata words
//! are published in the pinned Linux order. Payload bytes are never accessed.
//! Diagnostics from this module grant no SMMU, DMA or firmware-ready authority.

use crate::qcom_modem_status::{self as directory, Directory, Read};

pub const SMEM_PHYSICAL: u64 = 0x8600_0000;
pub const SMEM_BYTES: usize = 0x20_0000;
pub const LOCK_PHYSICAL: u64 = 0x01f4_3000;
pub const ITEM_ID: u16 = 497;
pub const ITEM_BYTES: usize = 0x2000;
pub const PARTITION_INDEX: u32 = 7;
pub const PARTITION_OFFSET: usize = 0x11d000;
pub const PARTITION_BYTES: usize = 0x7c000;
pub const METADATA_PHYSICAL: u64 = SMEM_PHYSICAL + PARTITION_OFFSET as u64;
pub const LOCK_ATTEMPT_LIMIT: u32 = 65536;
const TABLE: usize = SMEM_BYTES - 4096;
const TABLE_COUNT: usize = 11;
const HEADER: usize = PARTITION_OFFSET + 32;
const FREE_AFTER: usize = 32 + 16 + ITEM_BYTES;
pub(crate) const STORES: [(usize, u32); 4] = [
    (HEADER, ((ITEM_ID as u32) << 16) | 0xa5a5),
    (HEADER + 4, ITEM_BYTES as u32),
    (HEADER + 8, 0),
    (PARTITION_OFFSET + 12, FREE_AFTER as u32),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreError {
    Rejected,
    Uncertain,
}

/// Serialized uncached metadata and the actual lock-3 register.
///
/// # Safety
/// Implementations must retain the exact SMEM/metadata/lock mappings and
/// exclude other AP clients for their entire lifetime. Reads are metadata
/// only, bounded to SMEM; stores admit only the four fixed metadata words.
/// `barrier` orders hardware accesses, not just compiler operations. Lock
/// methods must implement the real APPS-owner-1 hardware arbitration; a
/// software flag cannot establish hardware ownership. Failed/uncertain stores
/// must not be retried or undone. A host model may simulate these contracts.
pub unsafe trait Io: Read {
    fn barrier(&mut self);
    fn lock_write(&mut self, value: u32);
    fn lock_read(&mut self) -> u32;
    fn store_metadata(&mut self, offset: usize, value: u32) -> Result<(), StoreError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    LockBusy,
    LockLost,
    Directory(directory::Error),
    Unstable,
    Target,
    ItemLimit,
    TableDescriptor,
    ForeignRange,
    ForeignOverlap,
    Store { step: u8, source: StoreError },
    Readback { step: u8 },
    Lookup,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub consumed: bool,
    pub lock_attempts: u32,
    pub pre_scans: u8,
    pub post_scans: u8,
    pub submitted: u8,
    pub completed: u8,
    pub published: bool,
    pub release_sent: bool,
    pub item_limit: u16,
}

/// Address diagnostics only. This is neither an exclusive page lease nor a
/// DMA mapping capability: page rounding includes partition/item metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemInfo {
    physical: u64,
    bytes: usize,
}
impl ItemInfo {
    pub const fn physical(&self) -> u64 {
        self.physical
    }
    pub const fn bytes(&self) -> usize {
        self.bytes
    }
    pub const fn page_span(&self) -> (u64, usize) {
        let first = self.physical & !4095;
        let bytes = (self.bytes + (self.physical - first) as usize + 4095) & !4095;
        (first, bytes)
    }
}

/// No reset, retry, free, rollback or extraction of the retained IO view.
/// Dropping this owner performs no hardware operations and cannot free SMEM.
#[must_use]
pub struct Owner<I: Io> {
    io: I,
    report: Report,
    item: Option<ItemInfo>,
    modem_claimed: bool,
}
impl<I: Io> Owner<I> {
    pub fn new(io: I) -> Self {
        Self {
            io,
            report: Report::default(),
            item: None,
            modem_claimed: false,
        }
    }
    pub fn report(&self) -> Report {
        self.report
    }
    pub fn retained_item(&self) -> Option<&ItemInfo> {
        self.item.as_ref()
    }
    // Only the composed modem owner can borrow this same lock/metadata view.
    // Keep the actual successful IPA allocation owner, never just its address.
    pub(crate) fn claim_modem_extension(&mut self) -> Result<(), Error> {
        if self.modem_claimed
            || self.item.is_none()
            || !self.report.published
            || !self.report.release_sent
            || self.report.completed != 15
            || self.report.post_scans != 2
        {
            return Err(Error::Consumed);
        }
        self.modem_claimed = true;
        Ok(())
    }
    pub(crate) fn modem_io(&mut self) -> Result<&mut I, Error> {
        if !self.modem_claimed {
            return Err(Error::Consumed);
        }
        Ok(&mut self.io)
    }
    pub(crate) fn retained_io(&self) -> Result<&I, Error> {
        if self.item.is_none() || !self.report.release_sent || self.report.completed != 15 {
            return Err(Error::Consumed);
        }
        Ok(&self.io)
    }
    fn owned(&mut self) -> bool {
        self.io.barrier();
        self.io.lock_read() == 1
    }
    fn scan(&self) -> Result<Directory, Error> {
        directory::allocation_directory(&self.io, ITEM_ID).map_err(Error::Directory)
    }
    fn read16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.io.read8(offset), self.io.read8(offset + 1)])
    }
    fn read32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(core::array::from_fn(|i| self.io.read8(offset + i)))
    }
    fn guard_target(&mut self, d: &Directory) -> Result<(), Error> {
        if d.route.index != PARTITION_INDEX
            || d.route.offset != PARTITION_OFFSET
            || d.route.size != PARTITION_BYTES
            || d.route.cacheline != 32
            || d.free_uncached != 32
            || d.free_cached != PARTITION_BYTES
            || d.item.is_some()
        {
            return Err(Error::Target);
        }
        let count = self.read32(TABLE + 8) as usize;
        if count != TABLE_COUNT
            || self.read32(TABLE + 32) as usize != TABLE
            || self.read32(TABLE + 36) != 4096
            || self.read16(TABLE + 44) != 0xffff
            || self.read16(TABLE + 46) != 0xffff
        {
            return Err(Error::TableDescriptor);
        }
        let info = TABLE + 32 + TABLE_COUNT * 48;
        let limit = if self.read32(info) == u32::from_le_bytes(*b"SIII") {
            self.read16(info + 16)
        } else {
            512
        };
        self.report.item_limit = limit;
        if ITEM_ID < 8 || ITEM_ID >= limit {
            return Err(Error::ItemLimit);
        }
        for i in 1..TABLE_COUNT {
            if i == PARTITION_INDEX as usize {
                continue;
            }
            let at = TABLE + 32 + i * 48;
            let offset = self.read32(at) as usize;
            let bytes = self.read32(at + 4) as usize;
            // Pinned Linux skips either zero field, including partial slots.
            if offset == 0 || bytes == 0 {
                continue;
            }
            if offset > TABLE || bytes > TABLE - offset {
                return Err(Error::ForeignRange);
            }
            if offset < PARTITION_OFFSET + PARTITION_BYTES && PARTITION_OFFSET < offset + bytes {
                return Err(Error::ForeignOverlap);
            }
        }
        Ok(())
    }
    fn store(&mut self, step: usize) -> Result<(), Error> {
        if !self.owned() {
            return Err(Error::LockLost);
        }
        let (offset, value) = STORES[step];
        // A failed call may already have reached hardware. Consume first.
        self.report.submitted |= 1 << step;
        self.io
            .store_metadata(offset, value)
            .map_err(|source| Error::Store {
                step: step as u8,
                source,
            })?;
        self.io.barrier();
        if !self.owned() {
            return Err(Error::LockLost);
        }
        if self.read32(offset) != value {
            return Err(Error::Readback { step: step as u8 });
        }
        self.report.completed |= 1 << step;
        Ok(())
    }
    fn locked_allocate(&mut self) -> Result<ItemInfo, Error> {
        let first = self.scan();
        self.report.pre_scans = 1;
        let first = first?;
        if !self.owned() {
            return Err(Error::LockLost);
        }
        let second = self.scan();
        self.report.pre_scans = 2;
        let second = second?;
        if first != second {
            return Err(Error::Unstable);
        }
        self.guard_target(&second)?;
        for step in 0..4 {
            if step == 3 {
                // Linux wmb() before offset_free_uncached publication.
                self.io.barrier();
            }
            self.store(step)?;
        }
        self.report.published = true;
        let first = self.scan();
        self.report.post_scans = 1;
        let first = first?;
        let expected = directory::Item {
            offset: HEADER + 16,
            size: ITEM_BYTES,
        };
        if first.route != second.route
            || first.free_uncached != FREE_AFTER
            || first.free_cached != PARTITION_BYTES
            || first.item != Some(expected)
        {
            return Err(Error::Lookup);
        }
        if !self.owned() {
            return Err(Error::LockLost);
        }
        let second = self.scan();
        self.report.post_scans = 2;
        if first != second? {
            return Err(Error::Unstable);
        }
        Ok(ItemInfo {
            physical: SMEM_PHYSICAL + expected.offset as u64,
            bytes: expected.size,
        })
    }
    pub fn allocate_once(&mut self) -> Result<&ItemInfo, Error> {
        if self.report.consumed {
            return Err(Error::Consumed);
        }
        self.report.consumed = true;
        let mut acquired = false;
        for attempt in 1..=LOCK_ATTEMPT_LIMIT {
            self.report.lock_attempts = attempt;
            self.io.lock_write(1);
            if self.owned() {
                acquired = true;
                break;
            }
        }
        if !acquired {
            return Err(Error::LockBusy);
        }
        let result = self.locked_allocate();
        // Never release a lock whose owner is no longer APPS. In particular,
        // successful lookup cannot override a lost-lock error at this point.
        if !self.owned() {
            return Err(Error::LockLost);
        }
        self.io.lock_write(0);
        self.io.barrier();
        self.report.release_sent = true;
        self.item = Some(result?);
        Ok(self.item.as_ref().unwrap())
    }
}
