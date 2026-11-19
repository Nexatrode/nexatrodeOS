//! Three-page Apps RSC adapter exposing only read access for handoff inspection.

use personal_os_rust_server::qcom_rpmh_handoff::{ReadRegisters, Register};

pub struct Controller {
    registers: *const u32,
}

impl Controller {
    /// # Safety
    /// `registers` must be aligned and map physical 0x18220000..0x18223000
    /// uncached and readable for this object's lifetime. The dedicated handoff
    /// probe maps all three pages read-only. A bus transaction probe may map
    /// the first page writable through a separate adapter, with this interface
    /// still exposing no register write path.
    pub unsafe fn new(registers: *const u32) -> Self {
        Self { registers }
    }
}

impl ReadRegisters for Controller {
    fn read(&mut self, register: Register) -> u32 {
        barrier();
        let value = unsafe { self.registers.add(register.offset() / 4).read_volatile() };
        barrier();
        u32::from_le(value)
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
