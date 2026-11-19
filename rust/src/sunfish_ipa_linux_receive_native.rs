//! Exclusive uncached CPU backend for the full IPA exception-RX allocation.
//! Geometry checks do not grant physical frames, SMMU, DMA or IRQ authority.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::sunfish_ipa_linux_buffers::{Memory, MemoryError, PAGE_BYTES};
use crate::sunfish_ipa_linux_receive::{DATA_RESERVED_BYTES, DATA_USED_PAGES};
use core::marker::PhantomData;

pub const RESERVED_PAGES: usize = DATA_RESERVED_BYTES / PAGE_BYTES;

pub struct NativeReceiveRam<'a> {
    pointer: *mut u32,
    physical: [u64; RESERVED_PAGES],
    _exclusive: PhantomData<&'a mut [u32]>,
}
impl<'a> NativeReceiveRam<'a> {
    /// # Safety
    /// Retain the sole uncached RW/NX mapping of these 512 private allocator
    /// frames, contiguous in this CPU virtual span, for `'a`. The actual
    /// CapDL must establish ownership and absence of aliases/reserved overlaps.
    /// Before RX publication, a retained verified IPA domain must map the
    /// first 502 pages in order at the receive plan's DATA_IOVA. The ten tail
    /// pages are CPU canary storage; do not expose them to device DMA.
    /// No DMA may run during CPU preparation, and no other CPU writer or
    /// route/context retargeting may exist while any buffer is published.
    /// This constructor validates geometry only, not any of these rights.
    pub unsafe fn new(pointer: *mut u32, physical: &[u64]) -> Result<Self, MemoryError> {
        if pointer.is_null()
            || pointer as usize % PAGE_BYTES != 0
            || (pointer as usize)
                .checked_add(DATA_RESERVED_BYTES)
                .is_none()
            || physical.len() != RESERVED_PAGES
        {
            return Err(MemoryError::Layout);
        }
        let mut pages = [0; RESERVED_PAGES];
        for (n, &page) in physical.iter().enumerate() {
            if !(0x80000000..1u64 << 40).contains(&page)
                || page % PAGE_BYTES as u64 != 0
                || page
                    != physical[0]
                        .checked_add((n * PAGE_BYTES) as u64)
                        .ok_or(MemoryError::Layout)?
            {
                return Err(MemoryError::Layout);
            }
            pages[n] = page;
        }
        Ok(Self {
            pointer,
            physical: pages,
            _exclusive: PhantomData,
        })
    }
    pub const fn physical_base(&self) -> u64 {
        self.physical[0]
    }
    /// Metadata for the retained SMMU owner. This is not another RAM backend
    /// or a grant to dereference the numerical virtual span.
    #[allow(dead_code)] // The standalone CPU-backend suite omits the SMMU owner.
    pub(crate) fn identity(&self) -> ([u64; RESERVED_PAGES], (usize, usize)) {
        (self.physical, (self.pointer as usize, DATA_RESERVED_BYTES))
    }
    /// Diagnostics for a future retained domain guard, not a mapping receipt.
    pub fn dma_page(&self, n: usize) -> Option<u64> {
        if n < DATA_USED_PAGES {
            Some(self.physical[n])
        } else {
            None
        }
    }
    fn checked(&self, offset: usize) -> Result<*mut u32, MemoryError> {
        if offset % 4 != 0 || offset >= DATA_RESERVED_BYTES {
            return Err(MemoryError::Bounds);
        }
        // SAFETY: the retained constructor contract covers this whole span;
        // the checked aligned offset selects exactly one 32-bit word.
        Ok(unsafe { self.pointer.add(offset / 4) })
    }
    fn barrier() {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        #[cfg(not(target_arch = "aarch64"))]
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}
impl Memory for NativeReceiveRam<'_> {
    fn bytes(&self) -> usize {
        DATA_RESERVED_BYTES
    }
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        let pointer = self.checked(offset)?;
        // SAFETY: checked bounds, sole retained mapping and constructor's
        // ownership contract. The caller must also enforce buffer ownership.
        let value = unsafe { pointer.read_volatile() };
        Self::barrier();
        Ok(u32::from_le(value))
    }
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        let pointer = self.checked(offset)?;
        Self::barrier();
        // SAFETY: as above. Scalar volatile stores avoid implicit SIMD
        // memset/copy operations on the uncached/device CPU mapping.
        unsafe { pointer.write_volatile(value.to_le()) };
        Self::barrier();
        Ok(())
    }
}
