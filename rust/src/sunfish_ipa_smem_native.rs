//! Exact uncached native view used by the bounded IPA SMEM owner.
//! Construction requires real retained mappings; no hardware is touched here.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::qcom_modem_status::Read;
use crate::sunfish_ipa_smem::{
    Io, LOCK_PHYSICAL, METADATA_PHYSICAL, PARTITION_OFFSET, SMEM_BYTES, SMEM_PHYSICAL, STORES,
    StoreError,
};
use core::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Mapping,
}

pub struct Window<'a> {
    smem: *const u8,
    metadata: *mut u32,
    lock: *mut u32,
    next_store: usize,
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    modem_pages: Option<*mut u32>,
    _retained: PhantomData<&'a mut [u8]>,
}
impl Window<'_> {
    pub(crate) fn virtual_spans(&self) -> [(usize, usize); 2] {
        [(self.smem as usize, SMEM_BYTES), (self.lock as usize, 4096)]
    }
    /// # Safety
    /// Retain SMEM at physical 0x86000000/2 MiB as uncached NX metadata reads.
    /// Only the page at 0x8611d000 is writable, at the corresponding address
    /// within that same contiguous CPU view (no writable/cached alias).
    /// `lock` is exclusively TCSR lock 3 at 0x01f43000, uncached RW/NX.
    /// These mappings must remain valid for the returned lifetime, excluding
    /// other AP clients, a concurrent allocator, reset and remapping. Remote
    /// writers must honor the actual shared lock. The capability manifest must
    /// establish physical identities/rights: numerical addresses alone do not.
    pub unsafe fn new(
        smem: *const u8,
        smem_physical: u64,
        metadata: *mut u32,
        metadata_physical: u64,
        lock: *mut u32,
        lock_physical: u64,
    ) -> Result<Self, Error> {
        Self::views(
            smem,
            smem_physical,
            metadata,
            metadata_physical,
            lock,
            lock_physical,
        )
    }
    fn views(
        smem: *const u8,
        smem_physical: u64,
        metadata: *mut u32,
        metadata_physical: u64,
        lock: *mut u32,
        lock_physical: u64,
    ) -> Result<Self, Error> {
        if smem_physical != SMEM_PHYSICAL
            || metadata_physical != METADATA_PHYSICAL
            || lock_physical != LOCK_PHYSICAL
            || smem.is_null()
            || metadata.is_null()
            || lock.is_null()
            || smem as usize % 4096 != 0
            || metadata as usize % 4096 != 0
            || lock as usize % 4096 != 0
        {
            return Err(Error::Mapping);
        }
        let first = smem as usize;
        let end = first.checked_add(SMEM_BYTES).ok_or(Error::Mapping)?;
        let lock_first = lock as usize;
        let lock_end = lock_first.checked_add(4096).ok_or(Error::Mapping)?;
        if first.checked_add(PARTITION_OFFSET) != Some(metadata as usize)
            || (first < lock_end && lock_first < end)
        {
            return Err(Error::Mapping);
        }
        Ok(Self {
            smem,
            metadata,
            lock,
            next_store: 0,
            #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
            modem_pages: None,
            _retained: PhantomData,
        })
    }
    /// # Safety
    /// Same retained single-owner SMEM/lock mapping contract as `new`, with
    /// the additional sole uncached RW/NX five-page subview at8611f000. The
    /// complete capability audit must establish those rights in the same
    /// contiguous CPU view. This object still admits only the IPA metadata
    /// sequence; the composed retained owner must exclusively delegate modem
    /// writes, never create a second AP client or independent lock owner.
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub unsafe fn new_with_modem_pages(
        smem: *const u8,
        smem_physical: u64,
        metadata: *mut u32,
        metadata_physical: u64,
        lock: *mut u32,
        lock_physical: u64,
        pages: *mut u32,
        pages_physical: u64,
    ) -> Result<Self, Error> {
        let mut view = Self::views(
            smem,
            smem_physical,
            metadata,
            metadata_physical,
            lock,
            lock_physical,
        )?;
        if pages_physical != 0x8611f000
            || (smem as usize).checked_add(0x11f000) != Some(pages as usize)
        {
            return Err(Error::Mapping);
        }
        view.modem_pages = Some(pages);
        Ok(view)
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub(crate) fn modem_pages(&self) -> Option<*mut u32> {
        self.modem_pages
    }
}
impl Read for Window<'_> {
    fn read8(&self, offset: usize) -> u8 {
        assert!(offset < SMEM_BYTES);
        // SAFETY: new's caller retains the exact entire read-only SMEM view.
        unsafe { self.smem.add(offset).read_volatile() }
    }
    fn read_le32(&self, offset: usize) -> u32 {
        assert!(offset <= SMEM_BYTES - 4);
        if offset % 4 != 0 {
            return u32::from_le_bytes(core::array::from_fn(|i| self.read8(offset + i)));
        }
        // SAFETY: construction requires a page-aligned retained SMEM view;
        // the bounds and offset alignment above make this a valid word read.
        u32::from_le(unsafe { self.smem.add(offset).cast::<u32>().read_volatile() })
    }
}
// SAFETY: unsafe construction retains the audited metadata/lock views. The
// private window has no extraction or payload writer and admits four stores.
unsafe impl Io for Window<'_> {
    fn barrier(&mut self) {
        #[cfg(target_arch = "aarch64")]
        // SAFETY: a barrier has no memory operands or firmware side effects.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        #[cfg(not(target_arch = "aarch64"))]
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
    fn lock_write(&mut self, value: u32) {
        assert!(value <= 1);
        // SAFETY: new's caller retains this exact hardware lock register.
        unsafe { self.lock.write_volatile(value) };
    }
    fn lock_read(&mut self) -> u32 {
        // SAFETY: new's caller retains this exact hardware lock register.
        unsafe { self.lock.read_volatile() }
    }
    fn store_metadata(&mut self, offset: usize, value: u32) -> Result<(), StoreError> {
        if STORES.get(self.next_store) != Some(&(offset, value)) {
            return Err(StoreError::Rejected);
        }
        self.barrier();
        if self.lock_read() != 1 {
            return Err(StoreError::Rejected);
        }
        self.next_store += 1;
        // SAFETY: the exact fixed sequence writes only the four aligned words
        // in the one retained metadata page. Consume before reaching hardware.
        unsafe {
            self.metadata
                .add((offset - PARTITION_OFFSET) / 4)
                .write_volatile(value);
        }
        Ok(())
    }
}
