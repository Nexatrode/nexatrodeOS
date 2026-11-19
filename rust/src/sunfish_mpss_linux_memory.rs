//! Exclusive scalar CPU mappings for the selected Linux PAS4 loader.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::sunfish_mpss_linux_loader::{BASE, BYTES, Error, METADATA_BYTES, Memory};
use core::marker::PhantomData;

pub struct Window<'a> {
    metadata: *mut u8,
    target: *mut u8,
    physical: u64,
    _exclusive: PhantomData<&'a mut [u8]>,
}
impl<'a> Window<'a> {
    /// # Safety
    /// Provide exclusive, complete uncached RW/NX CPU mappings of three fresh
    /// contiguous allocator pages and the exact reserved MPSS region. Their
    /// real capabilities must exclude all other live mappings and reserved
    /// aliases. Metadata must not be in an existing TZMem/SHM bridge. The cold
    /// boot chain must authorize host staging of MPSS. Retain both mappings
    /// across PAS protection, uncertain completion and running firmware; no
    /// other CPU/device may access them during staging. Numerical checks here
    /// establish none of that authority. The loader seals CPU access before
    /// INIT (metadata) and AUTH (target); the platform must preserve the seal.
    pub unsafe fn new(
        metadata: *mut u8,
        pages: [u64; 3],
        target: *mut u8,
        target_physical: u64,
        target_bytes: usize,
    ) -> Result<Self, Error> {
        let a = metadata as usize;
        let b = target as usize;
        let ae = a.checked_add(METADATA_BYTES).ok_or(Error::Geometry)?;
        let be = b.checked_add(BYTES).ok_or(Error::Geometry)?;
        let end = pages[0]
            .checked_add(METADATA_BYTES as u64)
            .ok_or(Error::Geometry)?;
        if a == 0
            || b == 0
            || a % 4096 != 0
            || b % 4096 != 0
            || (a < be && b < ae)
            || target_physical != BASE
            || target_bytes != BYTES
            || pages[0] < 0x80000000
            || pages[0] % 4096 != 0
            || pages[1] != pages[0] + 4096
            || pages[2] != pages[0] + 8192
            || end > 1 << 40
        {
            return Err(Error::Geometry);
        }
        for (base, bytes) in [
            (0x86000000, 0x200000),
            (BASE, BYTES as u64),
            (0x98f80000, 0x10000),
            (0x146a8000, 0x2000),
            (0xfde01000, 0x200000),
        ] {
            if pages[0] < base + bytes && base < end {
                return Err(Error::Geometry);
            }
        }
        Ok(Self {
            metadata,
            target,
            physical: pages[0],
            _exclusive: PhantomData,
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
    fn pointer(base: *mut u8, at: usize, width: usize, limit: usize) -> Result<*mut u8, Error> {
        if at % width != 0 || at.checked_add(width).is_none_or(|end| end > limit) {
            return Err(Error::Geometry);
        }
        // SAFETY: constructor owns complete mappings; the entire scalar is in range.
        Ok(unsafe { base.add(at) })
    }
    fn read8(base: *mut u8, at: usize, limit: usize) -> Result<u8, Error> {
        let p = Self::pointer(base, at, 1, limit)?;
        Self::barrier();
        let v = unsafe { p.read_volatile() };
        Self::barrier();
        Ok(v)
    }
    fn write8(base: *mut u8, at: usize, limit: usize, v: u8) -> Result<(), Error> {
        let p = Self::pointer(base, at, 1, limit)?;
        Self::barrier();
        unsafe { p.write_volatile(v) };
        Self::barrier();
        Ok(())
    }
}
impl Memory for Window<'_> {
    fn metadata_physical(&self) -> u64 {
        self.physical
    }
    fn metadata_read(&mut self, at: usize) -> Result<u8, Error> {
        Self::read8(self.metadata, at, METADATA_BYTES)
    }
    fn metadata_write(&mut self, at: usize, v: u8) -> Result<(), Error> {
        Self::write8(self.metadata, at, METADATA_BYTES, v)
    }
    fn target_read8(&mut self, at: usize) -> Result<u8, Error> {
        Self::read8(self.target, at, BYTES)
    }
    fn target_write8(&mut self, at: usize, v: u8) -> Result<(), Error> {
        Self::write8(self.target, at, BYTES, v)
    }
    fn target_read32(&mut self, at: usize) -> Result<u32, Error> {
        let p = Self::pointer(self.target, at, 4, BYTES)?.cast::<u32>();
        Self::barrier();
        let v = unsafe { p.read_volatile() };
        Self::barrier();
        Ok(u32::from_le(v))
    }
    fn target_write32(&mut self, at: usize, v: u32) -> Result<(), Error> {
        let p = Self::pointer(self.target, at, 4, BYTES)?.cast::<u32>();
        Self::barrier();
        unsafe { p.write_volatile(v.to_le()) };
        Self::barrier();
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        Self::barrier();
        Ok(())
    }
}
