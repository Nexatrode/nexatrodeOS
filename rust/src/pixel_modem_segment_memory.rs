//! Byte-only adapter for the fixed segment14 window with read-only guard pages.
use personal_os_rust_server::qcom_modem_segment::{DATA_END, DATA_START, Memory, WINDOW_BYTES};
pub struct SegmentMemory {
    base: *mut u8,
}
impl SegmentMemory {
    /// # Safety
    /// `base` must exclusively map a contiguous16K window at0x8b77f000, NX, with only the
    /// middle8K RW and the outer4K pages read-only,
    /// uncached Device-nGnRnE for the lifetime of this adapter. Access may only
    /// occur after successful PAS4 memory setup and metadata integrity gates.
    pub unsafe fn new(base: *mut u8) -> Self {
        Self { base }
    }
}
impl Memory for SegmentMemory {
    fn read_byte(&mut self, offset: usize) -> u8 {
        assert!(offset < WINDOW_BYTES);
        unsafe { self.base.add(offset).read_volatile() }
    }
    fn write_byte(&mut self, offset: usize, value: u8) {
        assert!((DATA_START..DATA_END).contains(&offset));
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
