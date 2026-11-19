//! Volatile adapter for the selected reserved RMTFS buffer and private scratch.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::qcom_modem_handlers::Fault;
use crate::qcom_modem_storage::RAM_IO_BYTES as IO_BYTES;
use crate::sunfish_linux_rmtfs::{ARG_BYTES, Call, Io, Mappings, Observation, PAGE};

pub trait Gate {
    fn observe(&mut self) -> Result<Observation, Fault>;
}
#[cfg(feature = "sunfish-linux-mpss-auth")]
pub trait AuthGate: Gate {
    /// Before the actual sealed PAS4 AUTH call, permanently change the real
    /// retained boot lifecycle from stopped to possibly executing. Validate
    /// generation, mappings and original owners. Never undo this on an error.
    /// A report or pending loader Guard is not evidence of this transition.
    fn submit_mpss_auth(
        &mut self,
        generation: u64,
        request: &crate::sunfish_mpss_linux_loader::Request,
    ) -> Result<(), Fault>;
}
pub trait Smc {
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault>;
}
pub struct Native<G: Gate, S: Smc> {
    gate: G,
    smc: S,
    mappings: Mappings,
    staged: bool,
    attempted: bool,
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    auth_attempted: bool,
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    assignment_succeeded: bool,
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    assignment_completed: bool,
}
impl<G: Gate, S: Smc> Native<G, S> {
    /// # Safety
    /// Retain both exact RW/NX uncached mappings, physical reservations,
    /// unmapped virtual guards and SMC authority for this lifetime, including
    /// faults. No cached aliases, other host users, races or teardown. Gate
    /// must check actual identity/reset lifecycle and reviewed cold HLOS
    /// provenance. Mapping checks alone do not establish secure ownership.
    /// Shared-buffer operations follow the modem's request handoff; a read is
    /// one private copy, not a claim that hostile concurrent DMA is atomic.
    pub unsafe fn new(gate: G, smc: S, mappings: Mappings) -> Result<Self, (G, S, Fault)> {
        if mappings.validate().is_err() {
            return Err((gate, smc, Fault::Failed));
        }
        Ok(Self {
            gate,
            smc,
            mappings,
            staged: false,
            attempted: false,
            #[cfg(feature = "sunfish-linux-mpss-auth")]
            auth_attempted: false,
            #[cfg(feature = "sunfish-linux-mpss-auth")]
            assignment_succeeded: false,
            #[cfg(feature = "sunfish-linux-mpss-auth")]
            assignment_completed: false,
        })
    }
    fn span(&self, offset: u32, bytes: usize) -> Result<*mut u8, Fault> {
        if bytes == 0
            || bytes > IO_BYTES
            || (offset as usize)
                .checked_add(bytes)
                .is_none_or(|e| e > self.mappings.buffer.bytes as usize)
        {
            return Err(Fault::Failed);
        }
        Ok((self.mappings.buffer.virtual_address + u64::from(offset)) as *mut u8)
    }
}
#[cfg(feature = "sunfish-linux-mpss-auth")]
impl<G: AuthGate, S: Smc> crate::sunfish_mpss_linux_boot::AuthIo for Native<G, S> {
    fn submit_mpss_auth(
        &mut self,
        generation: u64,
        request: &crate::sunfish_mpss_linux_loader::Request,
    ) -> Result<(), Fault> {
        if self.auth_attempted || !self.assignment_completed {
            return Err(Fault::Uncertain);
        }
        self.auth_attempted = true;
        if request.kind() != crate::sunfish_mpss_linux_loader::Kind::AuthAndReset
            || request.words() != &[0x42000205, 1, 4, 0, 0, 0, 0, 0]
        {
            return Err(Fault::Failed);
        }
        self.gate.submit_mpss_auth(generation, request)
    }
}
fn sync() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}
impl<G: Gate, S: Smc> Io for Native<G, S> {
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.gate.observe()
    }
    fn zero(&mut self, offset: u32, bytes: usize) -> Result<(), Fault> {
        if self.staged || self.attempted || offset % PAGE != 0 || bytes != PAGE as usize {
            return Err(Fault::Failed);
        }
        let p = self.span(offset, bytes)?;
        for i in 0..bytes {
            unsafe {
                core::ptr::write_volatile(p.add(i), 0);
            }
        }
        sync();
        for i in 0..bytes {
            if unsafe { core::ptr::read_volatile(p.add(i)) } != 0 {
                return Err(Fault::Uncertain);
            }
        }
        sync();
        Ok(())
    }
    fn stage(&mut self, call: &Call) -> Result<(), Fault> {
        let expected = Call::new(self.mappings)?;
        if self.staged
            || self.attempted
            || call.words() != expected.words()
            || call.bytes() != expected.bytes()
        {
            return Err(Fault::Failed);
        }
        let p = self.mappings.scratch.virtual_address as *mut u8;
        for i in 0..ARG_BYTES {
            unsafe {
                core::ptr::write_volatile(p.add(i), call.bytes()[i]);
            }
        }
        sync();
        for i in 0..ARG_BYTES {
            if unsafe { core::ptr::read_volatile(p.add(i)) } != call.bytes()[i] {
                return Err(Fault::Uncertain);
            }
        }
        sync();
        self.staged = true;
        Ok(())
    }
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault> {
        let expected = Call::new(self.mappings)?;
        if !self.staged
            || self.attempted
            || call.words() != expected.words()
            || call.bytes() != expected.bytes()
        {
            return Err(Fault::Failed);
        }
        self.attempted = true;
        sync();
        let reply = self.smc.invoke(call)?;
        #[cfg(feature = "sunfish-linux-mpss-auth")]
        {
            self.assignment_succeeded = reply[0] == 0 && reply[1] == 0;
        }
        Ok(reply)
    }
    fn complete(&mut self) -> Result<(), Fault> {
        sync();
        #[cfg(feature = "sunfish-linux-mpss-auth")]
        {
            if !self.assignment_succeeded || self.assignment_completed {
                return Err(Fault::Uncertain);
            }
            self.assignment_completed = true;
        }
        Ok(())
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Fault> {
        let p = self.span(offset, out.len())?;
        sync();
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = unsafe { core::ptr::read_volatile(p.add(i)) };
        }
        sync();
        Ok(())
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Fault> {
        let p = self.span(offset, bytes.len())?;
        sync();
        for (i, byte) in bytes.iter().enumerate() {
            unsafe {
                core::ptr::write_volatile(p.add(i), *byte);
            }
        }
        sync();
        Ok(())
    }
}
