//! Selected MPSS cold power hold, composed with the retained SCM bus hold.
//! No PAS/reset, metadata access, sleep/wake programming, rollback or unvote.
#![forbid(unsafe_code)]
use crate::{
    qcom_cmd_db::Database,
    qcom_rpmh_handoff::{self as handoff, ReadRegisters},
    qcom_rpmh_read::{ActiveTransaction, Command, ReadResource, RegisterIo},
    sunfish_ipa_linux_pas::Resources as CallResources,
    sunfish_scm_linux_power as scm,
};
use core::mem::ManuallyDrop;
pub const COUNT: usize = 4;
pub const LOAD_STATE: &str = "{class: image, res: load_state, name: modem, val: on}";
const ORDER: [ReadResource; COUNT] = [
    ReadResource::Mx,
    ReadResource::Cx,
    ReadResource::Mss,
    ReadResource::Xo,
];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    Generation,
    Database,
    Address,
    Levels,
    Guard,
    Aop,
    Handoff,
    Baseline,
    Controller,
    Readback,
    Lease,
}
impl Error {
    pub const fn code(self) -> u64 {
        self as u64 + 1
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rail {
    resource: ReadResource,
    selector: u32,
    raw_level: u16,
    enable_corner: u8,
}
impl Rail {
    pub fn resource(self) -> ReadResource {
        self.resource
    }
    pub fn selector(self) -> u32 {
        self.selector
    }
    pub fn raw_level(self) -> u16 {
        self.raw_level
    }
    pub fn enable_corner(self) -> u8 {
        self.enable_corner
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    rails: [Rail; COUNT],
}
impl Plan {
    pub fn from_database(db: &Database<'_>) -> Result<Self, Error> {
        let mut rails = [None; COUNT];
        for (i, (resource, name)) in ORDER
            .into_iter()
            .zip(["mx.lvl", "cx.lvl", "mss.lvl", "xo.lvl"])
            .enumerate()
        {
            let r = db.lookup(name).map_err(|_| Error::Database)?;
            if r.command_address(3).map_err(|_| Error::Address)? != resource.address() {
                return Err(Error::Address);
            }
            let (selector, raw_level, enable_corner) = if resource == ReadResource::Xo {
                (3, 0, 0)
            } else {
                let aux = r.auxiliary;
                if aux.is_empty() || aux.len() % 2 != 0 || aux.len() > 64 {
                    return Err(Error::Levels);
                }
                let mut count = 0usize;
                let mut last = 0;
                let mut enable = 0;
                // rpmhpd uses u16 raw levels (no regulator +1 offset), retains
                // their original indices, and stops at the first padded zero.
                for (n, b) in aux.chunks_exact(2).enumerate() {
                    let level = u16::from_le_bytes([b[0], b[1]]);
                    if n == 0 && level != 0 {
                        return Err(Error::Levels);
                    }
                    if n > 0 && level == 0 {
                        break;
                    }
                    if enable == 0 && level != 0 {
                        enable = n as u8;
                    }
                    count = n + 1;
                    last = level;
                }
                if count < 2 || last == 0 || enable == 0 {
                    return Err(Error::Levels);
                }
                ((count - 1) as u32, last, enable)
            };
            rails[i] = Some(Rail {
                resource,
                selector,
                raw_level,
                enable_corner,
            });
        }
        Ok(Self {
            rails: rails.map(Option::unwrap),
        })
    }
    pub fn rails(self) -> [Rail; COUNT] {
        self.rails
    }
}
pub struct OnRequest(());
impl OnRequest {
    pub fn message(&self) -> &'static str {
        LOAD_STATE
    }
    /// Pinned qcom_aoss::qmp_send uses a zero-padded 64-byte buffer. The older
    /// downstream probe's 96-byte packet is a separate profile.
    pub fn words(&self) -> [u32; 17] {
        let mut bytes = [0u8; 64];
        bytes[..LOAD_STATE.len()].copy_from_slice(LOAD_STATE.as_bytes());
        let mut out = [0; 17];
        out[0] = 64;
        for (word, chunk) in out[1..].iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes(chunk.try_into().unwrap());
        }
        out
    }
}
/// Trusted retained native providers. check(true) establishes the actual cold
/// pre-auth lifecycle; check(false) permits its authorized running continuation.
/// Both revalidate generation, mappings, domain/IPA health and exclusive AP
/// controller/mailbox ownership. Constant providers are host models only.
pub trait Control {
    fn check(&mut self, generation: u64, pre_auth: bool) -> Result<(), Error>;
    /// Publish the fixed load-state ON once and observe transport consumption.
    /// Consumption does not establish semantic acceptance, power or readiness.
    fn load_state_on(&mut self, request: &OnRequest) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Aop,
    Baseline,
    Rails,
    Verify,
    Held,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub error: Option<Error>,
    pub aop_submitted: bool,
    pub aop_consumed: bool,
    pub attempted: u8,
    pub acknowledged: u8,
    pub last_rail: Option<ReadResource>,
    pub initial: [u32; COUNT],
    pub final_values: [u32; COUNT],
    pub reads: u32,
}
/// The caller retains the actual sole RSC endpoint alongside this owner; each
/// method exclusively borrows that same endpoint. Control and all partial
/// state remain retained on error/unwind. No address-only power permit exists.
pub struct HoldOnce<C: Control> {
    control: ManuallyDrop<C>,
    generation: u64,
    plan: Plan,
    report: Report,
    busy: bool,
    used: bool,
    handoff: Option<handoff::Snapshot>,
}
impl<C: Control> HoldOnce<C> {
    pub fn new(control: C, generation: u64, plan: Plan) -> Self {
        let error = (generation == 0).then_some(Error::Generation);
        Self {
            control: ManuallyDrop::new(control),
            generation,
            plan,
            report: Report {
                phase: if error.is_some() {
                    Phase::Failed
                } else {
                    Phase::New
                },
                error,
                aop_submitted: false,
                aop_consumed: false,
                attempted: 0,
                acknowledged: 0,
                last_rail: None,
                initial: [0; COUNT],
                final_values: [0; COUNT],
                reads: 0,
            },
            busy: false,
            used: false,
            handoff: None,
        }
    }
    pub fn report(&self) -> Report {
        let mut r = self.report;
        if self.busy && r.error.is_none() {
            r.phase = Phase::Failed;
            r.error = Some(Error::Consumed)
        }
        r
    }
    /// Cached diagnostics only; the provider remains owned and immutable.
    pub fn control(&self) -> &C {
        &self.control
    }
    fn fail(&mut self, e: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error.get_or_insert(e);
        e
    }
    fn guard(&mut self, initial: bool) -> Result<(), Error> {
        self.control
            .check(self.generation, initial)
            .map_err(|_| Error::Guard)
    }
    fn preserve(
        &mut self,
        io: &mut (impl RegisterIo + ReadRegisters),
        initial: bool,
    ) -> Result<(), Error> {
        self.guard(initial)?;
        if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)?
            != self.handoff.ok_or(Error::Handoff)?
        {
            return Err(Error::Handoff);
        }
        Ok(())
    }
    fn request(
        &mut self,
        io: &mut (impl RegisterIo + ReadRegisters),
        command: Command,
        polls: u32,
        wait: &mut impl FnMut(),
        initial: bool,
    ) -> Result<Option<u32>, Error> {
        self.preserve(io, initial)?;
        let mut t = ActiveTransaction::new(command, polls);
        t.start(io).map_err(|_| Error::Controller)?;
        while !t.poll(io).map_err(|_| Error::Controller)? {
            self.guard(initial)?;
            wait();
        }
        self.preserve(io, initial)?;
        Ok(t.response())
    }
    fn sample(
        &mut self,
        io: &mut (impl RegisterIo + ReadRegisters),
        polls: u32,
        wait: &mut impl FnMut(),
        initial: bool,
    ) -> Result<[u32; COUNT], Error> {
        let mut samples = [[0; COUNT]; 2];
        for pass in &mut samples {
            for r in ReadResource::ALL {
                pass[r.index()] = self
                    .request(io, Command::Read(r), polls, wait, initial)?
                    .ok_or(Error::Readback)?;
                self.report.reads += 1;
            }
        }
        if samples[0] != samples[1] {
            return Err(Error::Readback);
        }
        for rail in self.plan.rails {
            let value = samples[0][rail.resource.index()];
            if if rail.resource == ReadResource::Xo {
                value != 0 && value != 3
            } else {
                value > rail.selector
            } {
                return Err(Error::Baseline);
            }
        }
        Ok(samples[0])
    }
    fn held_values(&self, actual: [u32; COUNT]) -> Result<(), Error> {
        for rail in self.plan.rails {
            if actual[rail.resource.index()] < rail.selector {
                return Err(Error::Readback);
            }
        }
        Ok(())
    }
    pub fn enable(
        &mut self,
        io: &mut (impl RegisterIo + ReadRegisters),
        polls: u32,
        mut wait: impl FnMut(),
        mut progress: impl FnMut(Report),
    ) -> Result<(), Error> {
        if self.busy || self.used || self.report.error.is_some() {
            return Err(self.fail(Error::Consumed));
        }
        self.used = true;
        self.busy = true;
        let result = (|| {
            if polls == 0 {
                return Err(Error::Controller);
            }
            self.guard(true)?;
            self.report.phase = Phase::Aop;
            progress(self.report);
            self.report.aop_submitted = true;
            self.control
                .load_state_on(&OnRequest(()))
                .map_err(|_| Error::Aop)?;
            self.report.aop_consumed = true;
            self.guard(true)?;
            self.handoff = Some(handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)?);
            self.report.phase = Phase::Baseline;
            self.report.initial = self.sample(io, polls, &mut wait, true)?;
            self.report.phase = Phase::Rails;
            // CX's selected genpd has MX as parent. The native experiment
            // retains maximum active-only MX explicitly before CX, instead of
            // guessing other genpd consumers' contributions or lowering them.
            for (n, rail) in self.plan.rails.into_iter().enumerate() {
                self.report.last_rail = Some(rail.resource);
                progress(self.report);
                self.report.attempted |= 1 << n;
                self.request(
                    io,
                    Command::MpssLinuxFloor {
                        resource: rail.resource,
                        data: rail.selector,
                    },
                    polls,
                    &mut wait,
                    true,
                )?;
                self.report.acknowledged |= 1 << n;
                let actual = self.sample(io, polls, &mut wait, true)?;
                for prior in &self.plan.rails[..=n] {
                    if actual[prior.resource.index()] < prior.selector {
                        return Err(Error::Readback);
                    }
                }
                for (value, before) in actual.iter().zip(self.report.initial) {
                    if *value < before {
                        return Err(Error::Readback);
                    }
                }
                self.report.final_values = actual;
            }
            self.report.phase = Phase::Verify;
            self.guard(true)?;
            self.preserve(io, true)?;
            self.held_values(self.report.final_values)?;
            self.report.last_rail = None;
            self.report.phase = Phase::Held;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.busy = false;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    pub fn verify(
        &mut self,
        io: &mut (impl RegisterIo + ReadRegisters),
        polls: u32,
        mut wait: impl FnMut(),
    ) -> Result<(), Error> {
        if self.busy || self.report.error.is_some() || self.report.phase != Phase::Held {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = true;
        let result = self.sample(io, polls, &mut wait, false).and_then(|values| {
            self.held_values(values)?;
            self.report.final_values = values;
            Ok(())
        });
        match result {
            Ok(()) => {
                self.busy = false;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }
}
/// One composed secure-call resource provider. Every acquire/release verifies
/// the live modem rails as well as the retained SCM crypto-to-EBI floors.
/// A dropped outstanding lease stays outstanding in the original SCM owner.
pub struct Resources<'a, C: Control, I: RegisterIo + ReadRegisters> {
    power: &'a mut HoldOnce<C>,
    scm: &'a scm::HoldOnce,
    io: &'a mut I,
    polls: u32,
    failed: bool,
}
impl<'a, C: Control, I: RegisterIo + ReadRegisters> Resources<'a, C, I> {
    pub fn new(
        power: &'a mut HoldOnce<C>,
        scm: &'a scm::HoldOnce,
        io: &'a mut I,
        polls: u32,
    ) -> Result<Self, Error> {
        if power.report().phase != Phase::Held
            || power.report().error.is_some()
            || scm.report().error.is_some()
            || scm.report().completed != 0x3f
            || polls == 0
        {
            return Err(Error::Lease);
        }
        Ok(Self {
            power,
            scm,
            io,
            polls,
            failed: false,
        })
    }
    fn fail(&mut self, e: Error) -> u64 {
        self.failed = true;
        self.power.fail(e);
        e.code()
    }
}
impl<C: Control, I: RegisterIo + ReadRegisters> CallResources for Resources<'_, C, I> {
    type Lease = scm::Lease;
    fn acquire(&mut self) -> Result<Self::Lease, u64> {
        if self.failed {
            return Err(Error::Lease.code());
        }
        if let Err(e) = self
            .power
            .verify(self.io, self.polls, core::hint::spin_loop)
        {
            return Err(self.fail(e));
        }
        let lease = self
            .scm
            .acquire_lease(self.io, self.polls)
            .map_err(|_| self.fail(Error::Lease))?;
        if let Err(e) = self
            .power
            .verify(self.io, self.polls, core::hint::spin_loop)
        {
            return Err(self.fail(e));
        }
        Ok(lease)
    }
    fn release(&mut self, lease: Self::Lease) -> Result<(), u64> {
        if self.failed {
            return Err(Error::Lease.code());
        }
        self.scm
            .release_lease(self.io, self.polls, lease)
            .map_err(|_| self.fail(Error::Lease))?;
        self.power
            .verify(self.io, self.polls, core::hint::spin_loop)
            .map_err(|e| self.fail(e))
    }
}
