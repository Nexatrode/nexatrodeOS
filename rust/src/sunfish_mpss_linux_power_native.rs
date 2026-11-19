//! Retained Sunfish QMP endpoint for the selected Linux 64-byte modem ON.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::{
    qcom_qmp::Descriptor,
    qcom_qmp_link::{LinkHandshake, Step},
    sunfish_mpss_linux_power::{Control, Error, OnRequest},
};
use core::{marker::PhantomData, mem::ManuallyDrop};
pub trait Gate {
    /// Actual serialized root lifecycle/mapping/domain/controller ownership.
    fn check(&mut self, generation: u64, pre_auth: bool) -> Result<(), Error>;
    /// Bounded scheduling delay; no reset, remap, mailbox clearing or retry.
    fn wait(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Link,
    Payload,
    Publish,
    Consume,
    Held,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub failed: bool,
    pub stores: u32,
    pub notifications: u32,
    pub descriptor: Option<Descriptor>,
    pub receive_length: u32,
    pub transmit_length: u32,
}
pub struct Native<'a, G: Gate> {
    ram: *mut u32,
    apcs: *mut u32,
    gate: ManuallyDrop<G>,
    generation: Option<u64>,
    used: bool,
    failed: bool,
    completed: bool,
    link: Option<LinkHandshake>,
    polls: u32,
    report: Report,
    _retained: PhantomData<&'a mut [u32]>,
}
fn barrier() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}
impl<'a, G: Gate> Native<'a, G> {
    /// # Safety
    /// Retain uncached RW/NX Device pages for physical 0c300000 MSGRAM and
    /// 17c00000 APCS. These are this serialized root's sole AP endpoint views;
    /// no other AP mailbox writer, reset or remap may race them. Any later
    /// delegated SMP2P bit14 access must remain under that same root owner,
    /// serialized after ON completion. No physical identity follows from
    /// numerical arguments; the real capability/mapping audit must prove it.
    /// Keep views/providers on all outcomes. This adapter never closes the
    /// negotiated channel, publishes OFF, clears TX on timeout or retries.
    pub unsafe fn new(
        ram: *mut u32,
        ram_pa: u64,
        apcs: *mut u32,
        apcs_pa: u64,
        polls: u32,
        gate: G,
    ) -> Result<Self, (G, Error)> {
        let r = ram as usize;
        let a = apcs as usize;
        if ram.is_null()
            || apcs.is_null()
            || r % 4096 != 0
            || a % 4096 != 0
            || ram_pa != 0x0c300000
            || apcs_pa != 0x17c00000
            || polls == 0
            || polls > 65536
            || r.checked_add(4096).is_none()
            || a.checked_add(4096).is_none()
            || (r < a + 4096 && a < r + 4096)
        {
            return Err((gate, Error::Address));
        }
        Ok(Self {
            ram,
            apcs,
            gate: ManuallyDrop::new(gate),
            generation: None,
            used: false,
            failed: false,
            completed: false,
            link: None,
            polls,
            report: Report {
                phase: Phase::New,
                failed: false,
                stores: 0,
                notifications: 0,
                descriptor: None,
                receive_length: u32::MAX,
                transmit_length: u32::MAX,
            },
            _retained: PhantomData,
        })
    }
    fn read(&self, at: usize) -> u32 {
        u32::from_le(unsafe { self.ram.add(at / 4).read_volatile() })
    }
    pub fn report(&self) -> Report {
        let mut r = self.report;
        r.failed = self.failed;
        r
    }
    fn snapshot(&mut self) -> Result<(Descriptor, u32, u32), Error> {
        for _ in 0..8 {
            let a = core::array::from_fn::<u32, 16, _>(|i| self.read(i * 4));
            let d = Descriptor::from_words(&a);
            self.report.descriptor = Some(d);
            let l = d.validate(4096).map_err(|_| Error::Aop)?;
            if l.receive.offset() != 0xc0
                || l.receive.size() != 0x68
                || l.transmit.offset() != 0x128
                || l.transmit.size() != 0x2d8
            {
                return Err(Error::Aop);
            }
            let rx = self.read(0xc0);
            let tx = self.read(0x128);
            self.report.receive_length = rx;
            self.report.transmit_length = tx;
            barrier();
            if core::array::from_fn::<u32, 16, _>(|i| self.read(i * 4)) == a
                && (rx, tx) == (self.read(0xc0), self.read(0x128))
            {
                return Ok((d, rx, tx));
            }
        }
        Err(Error::Aop)
    }
    fn guard(&mut self, pre_auth: bool) -> Result<(), Error> {
        self.gate
            .check(self.generation.ok_or(Error::Guard)?, pre_auth)
    }
    fn ring(&mut self) -> Result<(), Error> {
        self.guard(true)?;
        barrier();
        self.report.notifications += 1;
        unsafe { self.apcs.add(3).write_volatile(1u32.to_le()) };
        barrier();
        Ok(())
    }
    fn run(&mut self, request: &OnRequest) -> Result<(), Error> {
        self.guard(true)?;
        self.report.phase = Phase::Link;
        let (d, rx, tx) = self.snapshot()?;
        let h = LinkHandshake::open(d, 4096, self.polls, rx, tx).map_err(|_| Error::Aop)?;
        self.link = Some(h);
        loop {
            self.guard(true)?;
            let (d, rx, tx) = self.snapshot()?;
            match self
                .link
                .as_mut()
                .unwrap()
                .advance(d, rx, tx)
                .map_err(|_| Error::Aop)?
            {
                Step::Notify(writes) => {
                    for w in writes.into_iter().flatten() {
                        self.guard(true)?;
                        self.report.stores += 1;
                        unsafe {
                            self.ram
                                .add(w.word_index())
                                .write_volatile(w.value().to_le())
                        };
                    }
                    self.ring()?;
                }
                Step::Wait => self.gate.wait()?,
                Step::Connected => break,
                Step::Complete => return Err(Error::Aop),
            }
        }
        self.guard(true)?;
        let (d, rx, tx) = self.snapshot()?;
        if !d.fully_connected() || rx != 0 || tx != 0 {
            return Err(Error::Aop);
        }
        let packet = request.words();
        self.report.phase = Phase::Payload;
        for (n, v) in packet.iter().enumerate().skip(1) {
            self.guard(true)?;
            self.report.stores += 1;
            unsafe { self.ram.add(0x128 / 4 + n).write_volatile(v.to_le()) };
        }
        self.guard(true)?;
        let (d, rx, tx) = self.snapshot()?;
        if !d.fully_connected() || rx != 0 || tx != 0 {
            return Err(Error::Aop);
        }
        barrier();
        self.report.phase = Phase::Publish;
        self.report.stores += 1;
        unsafe { self.ram.add(0x128 / 4).write_volatile(64u32.to_le()) };
        barrier();
        if self.read(0x128) != 64 {
            return Err(Error::Readback);
        }
        self.ring()?;
        self.report.phase = Phase::Consume;
        for _ in 0..self.polls {
            self.guard(true)?;
            let (d, rx, tx) = self.snapshot()?;
            if !d.fully_connected() || rx != 0 || (tx != 0 && tx != 64) {
                return Err(Error::Aop);
            }
            if tx == 0 {
                return Ok(());
            }
            self.gate.wait()?;
        }
        Err(Error::Aop)
    }
}
impl<G: Gate> Control for Native<'_, G> {
    fn check(&mut self, generation: u64, pre_auth: bool) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Consumed);
        }
        self.failed = true;
        if generation == 0 || self.generation.is_some_and(|g| g != generation) {
            return Err(Error::Generation);
        }
        self.generation = Some(generation);
        self.guard(pre_auth)?;
        if self.completed {
            let (d, rx, tx) = self.snapshot()?;
            if !d.fully_connected() || rx != 0 || tx != 0 {
                return Err(Error::Aop);
            }
        }
        self.failed = false;
        Ok(())
    }
    fn load_state_on(&mut self, request: &OnRequest) -> Result<(), Error> {
        if self.failed || self.used {
            return Err(Error::Consumed);
        }
        self.used = true;
        self.failed = true;
        self.run(request)?;
        self.completed = true;
        self.report.phase = Phase::Held;
        self.failed = false;
        Ok(())
    }
}
