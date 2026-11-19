//! Retained scalar CPU views for the Linux-reference IPA PAS loader.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::sunfish_ipa_fws::{REGION_BASE, REGION_BYTES};
use crate::sunfish_ipa_linux_loader::{Error, METADATA_BYTES, Memory};
use core::marker::PhantomData;

pub struct Window<'a> {
    metadata: *mut u8,
    target: *mut u8,
    physical: u64,
    _owned: PhantomData<&'a mut [u8]>,
}
impl<'a> Window<'a> {
    /// # Safety
    /// Retain exclusive uncached RW/NX CPU mappings of two fresh contiguous
    /// allocator pages and exactly the reserved IPA firmware region for `'a`.
    /// Actual CapDL must exclude all other mappings/allocated/reserved aliases
    /// and establish firmware staging authority. Metadata must not belong to
    /// a TZMem/SHM bridge. No other CPU/device may access either view, and
    /// neither mapping may be reused while PAS protection/completion is
    /// uncertain. Numerical validation does not establish these preconditions.
    pub unsafe fn new(
        metadata: *mut u8,
        pages: [u64; 2],
        target: *mut u8,
        target_physical: u64,
        target_bytes: usize,
    ) -> Result<Self, Error> {
        let a = metadata as usize;
        let b = target as usize;
        let ae = a.checked_add(METADATA_BYTES).ok_or(Error::Geometry)?;
        let be = b
            .checked_add(REGION_BYTES as usize)
            .ok_or(Error::Geometry)?;
        let end = pages[0]
            .checked_add(METADATA_BYTES as u64)
            .ok_or(Error::Geometry)?;
        if a == 0
            || b == 0
            || a % 4096 != 0
            || b % 4096 != 0
            || (a < be && b < ae)
            || target_physical != REGION_BASE
            || target_bytes != REGION_BYTES as usize
            || pages[0] < 0x80000000
            || pages[0] % 4096 != 0
            || pages[1] != pages[0] + 4096
            || end > 1 << 40
        {
            return Err(Error::Geometry);
        }
        for (base, bytes) in [
            (0x86000000, 0x200000),
            (0x8b000000, 0x09800000),
            (REGION_BASE, REGION_BYTES),
            (0x146a8000, 0x2000),
        ] {
            if pages[0] < base + bytes && base < end {
                return Err(Error::Geometry);
            }
        }
        Ok(Self {
            metadata,
            target,
            physical: pages[0],
            _owned: PhantomData,
        })
    }
    fn barrier() {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        #[cfg(not(target_arch = "aarch64"))]
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    fn byte(base: *mut u8, offset: usize, limit: usize) -> Result<*mut u8, Error> {
        if offset >= limit {
            return Err(Error::Geometry);
        }
        // SAFETY: constructor retains both complete views; bound checked here.
        Ok(unsafe { base.add(offset) })
    }
    fn read(base: *mut u8, offset: usize, limit: usize) -> Result<u8, Error> {
        let p = Self::byte(base, offset, limit)?;
        let value = unsafe { p.read_volatile() };
        Self::barrier();
        Ok(value)
    }
    fn write(base: *mut u8, offset: usize, limit: usize, value: u8) -> Result<(), Error> {
        let p = Self::byte(base, offset, limit)?;
        Self::barrier();
        unsafe {
            p.write_volatile(value);
        }
        Self::barrier();
        Ok(())
    }
}
impl Memory for Window<'_> {
    fn metadata_physical(&self) -> u64 {
        self.physical
    }
    fn metadata_read(&mut self, offset: usize) -> Result<u8, Error> {
        Self::read(self.metadata, offset, METADATA_BYTES)
    }
    fn metadata_write(&mut self, offset: usize, value: u8) -> Result<(), Error> {
        Self::write(self.metadata, offset, METADATA_BYTES, value)
    }
    fn target_read(&mut self, offset: usize) -> Result<u8, Error> {
        Self::read(self.target, offset, REGION_BYTES as usize)
    }
    fn target_write(&mut self, offset: usize, value: u8) -> Result<(), Error> {
        Self::write(self.target, offset, REGION_BYTES as usize, value)
    }
    fn sync(&mut self) -> Result<(), Error> {
        Self::barrier();
        Ok(())
    }
}
