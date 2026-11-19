//! Uncached first-page Apps RSC adapter. The engine selects only active TCS0.

use personal_os_rust_server::qcom_rpmh_read::{Register, RegisterIo};

pub struct Controller {
    registers: *mut u32,
}

impl Controller {
    /// # Safety
    /// `registers` must be an aligned, uncached writable 4 KiB mapping of
    /// physical 0x18220000 for this boot. The caller must own the standalone
    /// AP DRV2 endpoint exclusively (no running Linux or competing AP client).
    /// This is not a grant to modify any other DRV or sleep/wake TCS.
    pub unsafe fn new(registers: *mut u32) -> Self {
        Self { registers }
    }
}

impl RegisterIo for Controller {
    fn read(&mut self, register: Register) -> u32 {
        barrier();
        let value = unsafe { self.registers.add(register.offset() / 4).read_volatile() };
        barrier();
        u32::from_le(value)
    }

    fn write(&mut self, register: Register, value: u32) {
        barrier();
        unsafe {
            self.registers
                .add(register.offset() / 4)
                .write_volatile(value.to_le())
        };
        barrier();
    }
}

fn barrier() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags))
    };
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}
