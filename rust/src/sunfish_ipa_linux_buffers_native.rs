//! Exclusive uncached CPU mapping for the experimental IPA buffer owner.
//! This backend does not perform DMA or provide an SMMU/IPA Ready token.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::sunfish_ipa_linux_buffers::{Memory, MemoryError, PAGE_BYTES, POOL_BYTES};
use core::marker::PhantomData;

pub struct NativeRam<'a> {
    pointer: *mut u32,
    physical: [u64; 32],
    _exclusive: PhantomData<&'a mut [u32]>,
}
impl<'a> NativeRam<'a> {
    /// # Safety
    /// Retain the sole uncached RW/NX mapping of exactly these 32 private
    /// allocator frames, contiguous in this virtual span, for `'a`. Actual
    /// CapDL must establish their ownership and absence of aliases/reserved
    /// overlaps. The retained installed domain must map them, in this order,
    /// at IOVA 0x80000000, and no DMA may use them during CPU preparation.
    /// No other CPU writer or context/route retargeting is allowed. This
    /// constructor validates geometry; it does not establish those rights.
    pub unsafe fn new(pointer: *mut u32, physical: &[u64]) -> Result<Self, MemoryError> {
        if pointer.is_null()
            || pointer as usize % PAGE_BYTES != 0
            || (pointer as usize).checked_add(POOL_BYTES).is_none()
            || physical.len() != 32
        {
            return Err(MemoryError::Layout);
        }
        let mut pages = [0; 32];
        for (i, &page) in physical.iter().enumerate() {
            if !(0x80000000..1u64 << 40).contains(&page)
                || page % PAGE_BYTES as u64 != 0
                || page
                    != physical[0]
                        .checked_add(i as u64 * PAGE_BYTES as u64)
                        .ok_or(MemoryError::Layout)?
            {
                return Err(MemoryError::Layout);
            }
            pages[i] = page;
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
    fn checked(&self, offset: usize) -> Result<*mut u32, MemoryError> {
        if offset % 4 != 0 || offset >= POOL_BYTES {
            return Err(MemoryError::Bounds);
        }
        // SAFETY: the constructor's caller retains the full exclusive span;
        // the checked aligned offset selects exactly one 32-bit word in it.
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
impl Memory for NativeRam<'_> {
    fn bytes(&self) -> usize {
        POOL_BYTES
    }
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        let pointer = self.checked(offset)?;
        // SAFETY: checked() establishes bounds and alignment; ownership is
        // retained by this non-cloneable backend and its constructor contract.
        let value = unsafe { pointer.read_volatile() };
        Self::barrier();
        Ok(u32::from_le(value))
    }
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        let pointer = self.checked(offset)?;
        Self::barrier();
        // SAFETY: as above. Scalar volatile stores avoid SIMD memset/copy on
        // the Device mapping; preparation occurs before any DMA submission.
        unsafe {
            pointer.write_volatile(value.to_le());
        }
        Self::barrier();
        Ok(())
    }
}
