//! One retained APCS write-only SMP2P doorbell, with a live lifecycle gate.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::sunfish_mpss_linux_smp2p::{DOORBELL, Error, Observation, Signals};
use core::{marker::PhantomData, mem::ManuallyDrop};
pub const APCS_REGISTER: u64 = 0x17c0000c;
pub trait Gate {
    /// Revalidate actual retained boot generation, PAS lifecycle, mappings,
    /// power references and domain health. No concurrent reset/remap/unvote.
    fn observe(&mut self, generation: u64) -> Result<Observation, Error>;
}
impl Gate for fn(u64) -> Result<Observation, Error> {
    fn observe(&mut self, g: u64) -> Result<Observation, Error> {
        self(g)
    }
}
pub struct Native<'a, G: Gate> {
    register: *mut u32,
    gate: ManuallyDrop<G>,
    last: Option<Observation>,
    failed: bool,
    _retained: PhantomData<&'a mut u32>,
}
pub type Platform<'a> = Native<'a, fn(u64) -> Result<Observation, Error>>;
impl<'a, G: Gate> Native<'a, G> {
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub(crate) fn span(&self) -> (usize, usize) {
        (self.register as usize, 4)
    }
    /// # Safety
    /// `register` is the retained aligned RW/NX Device mapping of physical
    /// 17c0000c. Only this serialized root owner writes this register; keep
    /// its capability, mapping and gate providers live even after failure.
    /// The gate binds actual retained owners, never remote-supplied state.
    /// This view excludes the APCS power/reset registers. All writes are the
    /// fixed SMP2P bit14, plus fixed GLINK bit12 only through the opt-in
    /// GlinkSignals delegation. There is no arbitrary register/bit API.
    pub unsafe fn new(register: *mut u32, physical: u64, gate: G) -> Result<Self, (G, Error)> {
        if register.is_null() || register as usize % 4 != 0 || physical != APCS_REGISTER {
            return Err((gate, Error::Guard));
        }
        Ok(Self {
            register,
            gate: ManuallyDrop::new(gate),
            last: None,
            failed: false,
            _retained: PhantomData,
        })
    }
}
fn barrier() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}
impl<G: Gate> Signals for Native<'_, G> {
    fn observe(&mut self, generation: u64) -> Result<Observation, Error> {
        if self.failed {
            return Err(Error::Consumed);
        }
        self.failed = true;
        let o = self.gate.observe(generation)?;
        if generation == 0
            || o.generation != generation
            || self.last.is_some_and(|p| o.lifecycle < p.lifecycle)
        {
            return Err(Error::Guard);
        }
        self.last = Some(o);
        self.failed = false;
        Ok(o)
    }
    fn notify(&mut self) -> Result<(), Error> {
        self.notify_bit(DOORBELL)
    }
}
impl<G: Gate> Native<'_, G> {
    fn notify_bit(&mut self, bit: u32) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Consumed);
        }
        self.failed = true; // no replay after an uncertain native notification
        let prior = self.last.ok_or(Error::Guard)?;
        if self.gate.observe(prior.generation)? != prior {
            return Err(Error::Guard);
        }
        barrier();
        unsafe { self.register.write_volatile(bit.to_le()) };
        barrier();
        // W1S register: a CPU store is not a remote acknowledgement.
        self.failed = false;
        Ok(())
    }
}
#[cfg(feature = "sunfish-linux-mpss-glink")]
impl<G: Gate> crate::sunfish_mpss_linux_smp2p::GlinkSignals for Native<'_, G> {
    fn notify_glink(&mut self) -> Result<(), Error> {
        use crate::sunfish_mpss_linux_smp2p::{Lifecycle, Power};
        if self
            .last
            .is_none_or(|o| o.lifecycle < Lifecycle::AuthReturned || o.power != Power::Active)
        {
            self.failed = true;
            return Err(Error::Guard);
        }
        self.notify_bit(1 << 12)
    }
}
