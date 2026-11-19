//! Aligned word/byte access to the fixed RW/NX modem pool and separate owned RAM.
use personal_os_rust_server::qcom_modem_image::{Memory, POOL_BYTES, SNAPSHOT_BYTES};
pub struct ImageMemory {
    pool: *mut u8,
    snapshot: *mut u8,
}
impl ImageMemory {
    /// # Safety
    /// `pool` exclusively maps all152MiB at physical0x8b000000 RW/NX,
    /// uncached Device-nGnRnE, with four-byte alignment. `snapshot` maps a
    /// separate owned2MiB RW/NX RAM region. Neither may alias metadata/ELF/other
    /// memory. Pool access must follow successful PAS4 MEM_SETUP and its gates.
    pub unsafe fn new(pool: *mut u8, snapshot: *mut u8) -> Self {
        Self { pool, snapshot }
    }
}
impl Memory for ImageMemory {
    fn read_word(&mut self, offset: usize) -> u32 {
        assert!(offset % 4 == 0 && offset <= POOL_BYTES - 4);
        unsafe { u32::from_le(self.pool.add(offset).cast::<u32>().read_volatile()) }
    }
    fn write_word(&mut self, offset: usize, value: u32) {
        assert!(offset % 4 == 0 && offset <= POOL_BYTES - 4);
        unsafe {
            self.pool
                .add(offset)
                .cast::<u32>()
                .write_volatile(value.to_le())
        };
    }
    fn read_byte(&mut self, offset: usize) -> u8 {
        assert!(offset < POOL_BYTES);
        unsafe { self.pool.add(offset).read_volatile() }
    }
    fn write_byte(&mut self, offset: usize, value: u8) {
        assert!(offset < POOL_BYTES);
        unsafe { self.pool.add(offset).write_volatile(value) };
    }
    fn snapshot_read(&mut self, offset: usize) -> u8 {
        assert!(offset < SNAPSHOT_BYTES);
        unsafe { self.snapshot.add(offset).read_volatile() }
    }
    fn snapshot_write(&mut self, offset: usize, value: u8) {
        assert!(offset < SNAPSHOT_BYTES);
        unsafe { self.snapshot.add(offset).write_volatile(value) };
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
