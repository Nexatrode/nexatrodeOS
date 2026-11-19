//! Single endpoint combining first-page transactions and full read snapshots.
#[path = "pixel_rpmh_controller.rs"]
mod first_page;
#[path = "pixel_rpmh_handoff.rs"]
mod read_snapshot;

use personal_os_rust_server::{qcom_rpmh_handoff as guard, qcom_rpmh_read as transaction};

pub struct Controller {
    transaction: first_page::Controller,
    guard: read_snapshot::Controller,
}
impl Controller {
    /// # Safety
    /// Map physical 0x18220000..0x18223000 contiguously and uncached; only its
    /// first page is writable, the other two pages must be read-only. Caller
    /// owns standalone AP DRV2 with no Linux/other AP controller client.
    pub unsafe fn new(base: *mut u32) -> Self {
        Self {
            transaction: unsafe { first_page::Controller::new(base) },
            guard: unsafe { read_snapshot::Controller::new(base.cast_const()) },
        }
    }
}
impl transaction::RegisterIo for Controller {
    fn read(&mut self, r: transaction::Register) -> u32 {
        transaction::RegisterIo::read(&mut self.transaction, r)
    }
    fn write(&mut self, r: transaction::Register, v: u32) {
        transaction::RegisterIo::write(&mut self.transaction, r, v)
    }
}
impl guard::ReadRegisters for Controller {
    fn read(&mut self, r: guard::Register) -> u32 {
        guard::ReadRegisters::read(&mut self.guard, r)
    }
}
