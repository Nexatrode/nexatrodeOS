//! Byte-only adapter for the one fixed reserved modem data page.
use personal_os_rust_server::qcom_modem_data::Page;
pub struct DataPage {
    base: *mut u8,
}
impl DataPage {
    /// # Safety
    /// `base` must exclusively map the fixed 0x93200000 4K modem page RW, NX,
    /// uncached Device-nGnRnE for the lifetime of this adapter. Access may only
    /// occur after successful PAS4 memory setup and metadata integrity gates.
    pub unsafe fn new(base: *mut u8) -> Self {
        Self { base }
    }
}
impl Page for DataPage {
    fn read_byte(&mut self, offset: usize) -> u8 {
        assert!(offset < personal_os_rust_server::qcom_modem_data::PAGE_BYTES);
        unsafe { self.base.add(offset).read_volatile() }
    }
    fn write_byte(&mut self, offset: usize, value: u8) {
        assert!(offset < personal_os_rust_server::qcom_modem_data::PAGE_BYTES);
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
