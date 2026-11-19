//! Narrow volatile, uncached pool/SCM-argument adapter. No live instance is
//! installed by the modem owner. Smc is a serialized private capability.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::qcom_modem_handlers::{Fault, IO_BYTES, Pool};
use crate::qcom_modem_pools::{ARG_BYTES, Call, Io, MAPPINGS, Mapping, Observation};

pub trait Gate {
    fn observe(&mut self) -> Result<Observation, Fault>;
}
pub trait Smc {
    /// Exactly one capability invocation. No retry/fallback/resume loops;
    /// even a kernel invocation error may have changed secure-world state.
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault>;
}
pub struct Native<G: Gate, S: Smc> {
    gate: G,
    smc: S,
    mappings: [Mapping; 3],
}
impl<G: Gate, S: Smc> Native<G, S> {
    /// # Safety
    /// Own all three exact uncached RW/NX mappings and their physical frames,
    /// guard holes and SMC/scratch capability exclusively for this lifetime.
    /// No cached aliases, external users, interrupt or teardown may race this
    /// object. Gate must check actual lifecycle/mapping identity and a reviewed
    /// fresh-boot HLOS/SCM64 contract. Retain the object after faults; never
    /// reset/unmap/reuse it on Drop. Pass it directly to Pools::attach.
    /// Invalid geometry returns the gate/capability to retain both resources.
    pub unsafe fn new(gate: G, smc: S, mappings: [Mapping; 3]) -> Result<Self, (G, S, Fault)> {
        if mappings != MAPPINGS {
            return Err((gate, smc, Fault::Failed));
        }
        Ok(Self {
            gate,
            smc,
            mappings,
        })
    }
    fn span(&self, p: Pool, offset: u32, bytes: usize) -> Result<*mut u8, Fault> {
        let i = if p == Pool::Storage { 0 } else { 1 };
        let m = self.mappings[i];
        if bytes == 0
            || bytes > IO_BYTES
            || (offset as usize)
                .checked_add(bytes)
                .is_none_or(|e| e > m.region.bytes as usize)
        {
            return Err(Fault::Failed);
        }
        Ok((m.virtual_address + u64::from(offset)) as *mut u8)
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
    fn stage(&mut self, call: &Call) -> Result<(), Fault> {
        let ptr = self.mappings[2].virtual_address as *mut u8;
        for i in 0..ARG_BYTES {
            unsafe {
                core::ptr::write_volatile(ptr.add(i), call.bytes()[i]);
            }
        }
        sync();
        for i in 0..ARG_BYTES {
            if unsafe { core::ptr::read_volatile(ptr.add(i)) } != call.bytes()[i] {
                return Err(Fault::Failed);
            }
        }
        sync();
        Ok(())
    }
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault> {
        sync();
        self.smc.invoke(call)
    }
    fn complete(&mut self) -> Result<(), Fault> {
        sync();
        Ok(())
    }
    fn zero(&mut self, p: Pool, offset: u32, bytes: usize) -> Result<(), Fault> {
        let ptr = self.span(p, offset, bytes)?;
        for i in 0..bytes {
            unsafe {
                core::ptr::write_volatile(ptr.add(i), 0);
            }
        }
        sync();
        for i in 0..bytes {
            if unsafe { core::ptr::read_volatile(ptr.add(i)) } != 0 {
                return Err(Fault::Uncertain);
            }
        }
        sync();
        Ok(())
    }
    fn read(&mut self, p: Pool, offset: u32, out: &mut [u8]) -> Result<(), Fault> {
        let ptr = self.span(p, offset, out.len())?;
        sync();
        for (i, b) in out.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        }
        sync();
        Ok(())
    }
    fn write(&mut self, p: Pool, offset: u32, bytes: &[u8]) -> Result<(), Fault> {
        let ptr = self.span(p, offset, bytes.len())?;
        for (i, b) in bytes.iter().enumerate() {
            unsafe {
                core::ptr::write_volatile(ptr.add(i), *b);
            }
        }
        sync();
        Ok(())
    }
}
