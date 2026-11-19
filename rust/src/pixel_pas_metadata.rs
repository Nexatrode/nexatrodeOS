//! Byte-only adapter for the dedicated owned MDT RAM mapping.
use personal_os_rust_server::qcom_pas_metadata::Memory;
pub struct Buffer {
    base: *mut u8,
}
impl Buffer {
    /// # Safety
    /// `base` must exclusively map five contiguous owned RAM pages RW, NX,
    /// uncached Device-nGnRnE for the lifetime of this adapter.
    pub unsafe fn new(base: *mut u8) -> Self {
        Self { base }
    }
}
impl Memory for Buffer {
    fn read_byte(&mut self, offset: usize) -> u8 {
        assert!(offset < personal_os_rust_server::qcom_pas_metadata::MAPPING_SIZE);
        unsafe { self.base.add(offset).read_volatile() }
    }
    fn write_byte(&mut self, offset: usize, value: u8) {
        assert!(offset < personal_os_rust_server::qcom_pas_metadata::MAPPING_SIZE);
        unsafe { self.base.add(offset).write_volatile(value) };
    }
    fn sync(&mut self) {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags))
        };
        #[cfg(not(target_arch = "aarch64"))]
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
}
