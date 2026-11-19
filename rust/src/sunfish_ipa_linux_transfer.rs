//! Retained IPA memory/table transaction engine, never a production Ready token.
//!
//! Owns the attempt while borrowing the actual pool and startup owners. Only
//! the fixed memory/table encodings and channel-1/event-0 doorbells are available.
//! A failed/uncertain store, timeout or unexpected source consumes the attempt,
//! masks GSI once and quarantines the original pool; no retry/free/rollback.
#![forbid(unsafe_code)]

use crate::{
    sunfish_ipa_linux_buffers::{self as buffers, Memory, Pool},
    sunfish_ipa_linux_command as command,
    sunfish_ipa_linux_endpoint::{self as endpoint, ENDPOINTS},
    sunfish_ipa_linux_gsi as gsi, sunfish_ipa_linux_memory as memory,
    sunfish_ipa_linux_memsetup::{self as memsetup, Plan},
    sunfish_ipa_linux_tables as tables,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Layout,
    Consumed,
    Prerequisite,
    Guard,
    Io,
    Pool,
    State,
    Pending,
    Readback,
    UnexpectedIrq,
    Event,
    Timeout,
    Acknowledge,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Gsi(command::Read),
    SharedMemorySize,
    EventPointer,
    LocalContext,
    Shared(usize),
}
impl Read {
    pub const fn offset(self) -> usize {
        match self {
            Self::Gsi(r) => r.offset(),
            Self::SharedMemorySize => 0x54,
            Self::EventPointer => 0x1d010,
            Self::LocalContext => 0x1e8,
            Self::Shared(o) => o,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    ChannelDoorbell,
    EventDoorbell,
    IeobClear,
    IeobMask,
    TypeMask,
    LocalContext,
}
impl Register {
    pub const fn offset(self) -> usize {
        match self {
            Self::ChannelDoorbell => 0x1e008,
            Self::EventDoorbell => 0x1e100,
            Self::IeobClear => 0x1f0c0,
            Self::IeobMask => command::IEOB_MASK,
            Self::TypeMask => gsi::TYPE_MASK,
            Self::LocalContext => 0x1e8,
        }
    }
}
pub struct Request {
    register: Register,
    value: u32,
}
impl Request {
    pub const fn register(&self) -> Register {
        self.register
    }
    pub const fn value(&self) -> u32 {
        self.value
    }
}
pub trait Io<M: Memory> {
    fn ticks(&mut self) -> u64;
    /// Check actual retained domain, this original pool, firmware and config.
    fn guard(&mut self, pool: &Pool<M>) -> Result<(), Error>;
    fn read(&mut self, register: Read) -> Result<u32, Error>;
    /// Publication must order all preceding CPU stores before the doorbell;
    /// event observations must order DMA writes before CPU event reads.
    fn write(&mut self, request: &Request) -> Result<(), Error>;
    fn acknowledge(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Preparing,
    Pending,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub cpu_sent: u8,
    pub cpu_done: u8,
    pub cpu_verified: u8,
    pub writes_sent: u8,
    pub writes_done: u8,
    pub published: bool,
    pub event: Option<memsetup::Event>,
    pub event_pointer: Option<u32>,
    pub zero_source_words: u32,
    pub table_source_words: u32,
    pub before_nonzero: u32,
    pub target_words: u32,
    pub canary_checks: u32,
    pub final_samples: u8,
    pub ack_sent: bool,
    pub ack_done: bool,
    pub containment_sent: bool,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}
pub struct TransferOnce {
    plan: Plan,
    table_plan: Option<tables::Plan>,
    report: Report,
    started_at: u64,
    timeout_ticks: u64,
}
impl TransferOnce {
    pub fn new(timeout_ticks: u64) -> Result<Self, Error> {
        if timeout_ticks == 0 || timeout_ticks >= 1 << 63 {
            return Err(Error::Layout);
        }
        Ok(Self {
            plan: Plan::new(0x415).map_err(|_| Error::Layout)?,
            table_plan: None,
            timeout_ticks,
            started_at: 0,
            report: Report {
                phase: Phase::New,
                cpu_sent: 0,
                cpu_done: 0,
                cpu_verified: 0,
                writes_sent: 0,
                writes_done: 0,
                published: false,
                event: None,
                event_pointer: None,
                zero_source_words: 0,
                table_source_words: 0,
                before_nonzero: 0,
                target_words: 0,
                canary_checks: 0,
                final_samples: 0,
                ack_sent: false,
                ack_done: false,
                containment_sent: false,
                error: None,
                containment_error: None,
            },
        })
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub const fn plan(&self) -> &Plan {
        &self.plan
    }
    pub(crate) fn new_tables(timeout_ticks: u64) -> Result<Self, Error> {
        let mut result = Self::new(timeout_ticks)?;
        result.table_plan = Some(tables::Plan::new(0x415).map_err(|_| Error::Layout)?);
        Ok(result)
    }
    pub(crate) fn table_plan(&self) -> &tables::Plan {
        self.table_plan.as_ref().expect("table-only coordinator")
    }
    pub(crate) fn memory_completed(&self) -> bool {
        self.table_plan.is_none()
            && self.report.phase == Phase::Complete
            && self.report.error.is_none()
            && self.report.published
            && self.report.ack_done
            && !self.report.containment_sent
            && self.report.cpu_done == 48
            && self.report.cpu_verified == 48
            && self.report.writes_done == 8
            && self.report.target_words == 1619
            && self.report.canary_checks == 50
            && self.report.final_samples == 2
    }
    fn count(&self) -> usize {
        if self.table_plan.is_some() {
            tables::COUNT
        } else {
            memsetup::COUNT
        }
    }
    fn command(&self, index: usize) -> Encoded {
        if let Some(p) = &self.table_plan {
            let c = p.commands()[index];
            Encoded {
                offset: c.local_offset(),
                size: c.size(),
                payload_offset: c.payload_offset(),
                tre_offset: c.tre_offset(),
                payload: *c.payload(),
                tre: *c.tre(),
            }
        } else {
            let c = self.plan.commands()[index];
            Encoded {
                offset: c.local_offset(),
                size: c.size(),
                payload_offset: c.payload_offset(),
                tre_offset: c.tre_offset(),
                payload: *c.payload(),
                tre: *c.tre(),
            }
        }
    }
    fn initial_event(&self) -> u32 {
        self.table_plan
            .as_ref()
            .map_or(self.plan.event_iova() as u32, |p| {
                p.initial_event_producer()
            })
    }
    fn event_offset(&self) -> usize {
        self.table_plan.as_ref().map_or(0, |p| p.event_offset())
    }
    fn channel_doorbell(&self) -> u32 {
        self.table_plan
            .as_ref()
            .map_or(self.plan.channel_doorbell().1, |p| p.channel_doorbell().1)
    }
    fn event_doorbell(&self) -> u32 {
        self.table_plan
            .as_ref()
            .map_or(self.plan.event_doorbell().1, |p| p.event_doorbell().1)
    }
    fn completed_event(&self, pointer: u32, bytes: &[u8; 16]) -> Result<memsetup::Event, Error> {
        if let Some(p) = &self.table_plan {
            p.completion(pointer, bytes).map_err(|_| Error::Event)
        } else {
            self.plan
                .completion(pointer, bytes)
                .map_err(|_| Error::Event)
        }
    }
    fn prerequisites<M: Memory>(
        setup: &gsi::SetupOnce,
        endpoints: &endpoint::ConfigureOnce,
        started: &command::StartOnce,
        pool: &Pool<M>,
        memory: Option<&TransferOnce>,
    ) -> Result<(), Error> {
        let g = setup.report();
        let e = endpoints.report();
        let c = started.report();
        if g.phase != gsi::SetupPhase::Complete
            || g.error.is_some()
            || g.mask_sent
            || e.phase != endpoint::Phase::Complete
            || e.error.is_some()
            || e.containment_sent
            || e.readback != endpoint::COMPLETE
            || e.writes_done != endpoint::COMPLETE
            || e.endpoints_done != 15
            || e.final_samples != 2
            || c.phase != command::Phase::Complete
            || c.error.is_some()
            || c.mask_sent
            || !c.ack_done
            || c.state != Some(2)
            || match memory {
                Some(owner) => {
                    !owner.memory_completed()
                        || !pool.memory_setup_complete()
                        || pool.table_setup_complete()
                }
                None => pool.memory_setup_complete(),
            }
        {
            return Err(Error::Prerequisite);
        }
        for kind in [
            buffers::Kind::Zero,
            buffers::Kind::CommandPayload,
            buffers::Kind::Tre(1),
            buffers::Kind::Event(1),
        ] {
            if pool.buffer(kind).is_none() {
                return Err(Error::Prerequisite);
            }
        }
        let ch = setup.plan().channels()[1];
        if ch.id != 1 || ch.event != 0 || ch.endpoint != 6 || !ch.command || !ch.toward_ipa {
            return Err(Error::Prerequisite);
        }
        Ok(())
    }
    fn read<M: Memory>(
        &mut self,
        pool: &Pool<M>,
        io: &mut impl Io<M>,
        r: Read,
    ) -> Result<u32, Error> {
        io.guard(pool)?;
        io.read(r)
    }
    fn write<M: Memory>(
        &mut self,
        pool: &Pool<M>,
        io: &mut impl Io<M>,
        register: Register,
        value: u32,
    ) -> Result<(), Error> {
        io.guard(pool)?;
        if register == Register::ChannelDoorbell {
            self.started_at = io.ticks();
            self.report.published = true;
        }
        self.report.writes_sent += 1;
        io.write(&Request { register, value })?;
        self.report.writes_done += 1;
        Ok(())
    }
    fn fail<M: Memory>(&mut self, pool: &mut Pool<M>, io: &mut impl Io<M>, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error = Some(error);
        self.report.containment_sent = true;
        self.report.containment_error = io
            .write(&Request {
                register: Register::TypeMask,
                value: 0,
            })
            .err();
        pool.quarantine();
        error
    }
    fn quiet<M: Memory>(&mut self, pool: &Pool<M>, io: &mut impl Io<M>) -> Result<(), Error> {
        use command::Read as R;
        for r in [
            R::TypeStatus,
            R::ChannelStatus,
            R::EventStatus,
            R::GlobalStatus,
            R::GeneralStatus,
            R::IeobStatus,
        ] {
            if self.read(pool, io, Read::Gsi(r))? != 0 {
                return Err(Error::Pending);
            }
        }
        Ok(())
    }
    fn states<M: Memory>(
        &mut self,
        pool: &Pool<M>,
        io: &mut impl Io<M>,
        types: u32,
        ieob: u32,
    ) -> Result<(), Error> {
        use command::Read as R;
        for (r, v) in [
            (R::Status, 1),
            (R::TypeMask, types),
            (R::ChannelMask, 0),
            (R::EventMask, 0),
            (R::GlobalEnable, 1),
            (R::GeneralEnable, 14),
            (R::IeobMask, ieob),
        ] {
            if self.read(pool, io, Read::Gsi(r))? != v {
                return Err(Error::State);
            }
        }
        for e in ENDPOINTS {
            if (self.read(pool, io, Read::Gsi(R::Channel(e)))? >> 20) & 15
                != if e.channel() == 1 { 2 } else { 1 }
                || (self.read(pool, io, Read::Gsi(R::Event(e)))? >> 20) & 15 != 1
            {
                return Err(Error::State);
            }
        }
        Ok(())
    }
    fn canaries<M: Memory>(&mut self, pool: &Pool<M>, io: &mut impl Io<M>) -> Result<(), Error> {
        for w in memory::words() {
            if self.read(pool, io, Read::Shared(w.offset()))? != memory::CANARY_VALUE {
                return Err(Error::Readback);
            }
            self.report.canary_checks += 1;
        }
        Ok(())
    }
    fn targets<M: Memory>(
        &mut self,
        pool: &Pool<M>,
        io: &mut impl Io<M>,
        before: bool,
    ) -> Result<(), Error> {
        for i in 0..self.count() {
            let c = self.command(i);
            for offset in (c.offset..c.offset + c.size).step_by(4) {
                let v = self.read(pool, io, Read::Shared(offset))?;
                if before {
                    self.report.before_nonzero += u32::from(v != 0);
                } else {
                    let expected = if let Some(plan) = &self.table_plan {
                        plan.expected32(offset).ok_or(Error::Layout)?
                    } else {
                        0
                    };
                    if v != expected {
                        return Err(Error::Readback);
                    }
                    self.report.target_words += 1;
                }
            }
        }
        Ok(())
    }
    fn descriptors<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
        stage: bool,
    ) -> Result<(), Error> {
        for i in 0..self.count() {
            let c = self.command(i);
            for (offset, bytes) in [
                (c.payload_offset, c.payload.as_slice()),
                (c.tre_offset, c.tre.as_slice()),
            ] {
                io.guard(pool)?;
                for (index, bytes) in bytes.chunks_exact(4).enumerate() {
                    let v = u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Layout)?);
                    if stage {
                        self.report.cpu_sent += 1;
                        if self.table_plan.is_some() {
                            pool.table_setup_write32(offset + index * 4, v)
                        } else {
                            pool.memory_setup_write32(offset + index * 4, v)
                        }
                        .map_err(|_| Error::Pool)?;
                        self.report.cpu_done += 1;
                    }
                    let actual = pool
                        .memory_setup_read32(offset + index * 4)
                        .map_err(|_| Error::Pool)?;
                    if actual != v {
                        return Err(Error::Readback);
                    }
                    if !stage {
                        self.report.cpu_verified += 1;
                    }
                }
                io.guard(pool)?;
            }
        }
        Ok(())
    }
    /// Actual owners are required; public reports/plans cannot substitute.
    pub fn begin<M: Memory>(
        &mut self,
        setup: &gsi::SetupOnce,
        endpoints: &endpoint::ConfigureOnce,
        started: &command::StartOnce,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        self.begin_inner(setup, endpoints, started, None, pool, io)
    }
    pub(crate) fn begin_tables<M: Memory>(
        &mut self,
        setup: &gsi::SetupOnce,
        endpoints: &endpoint::ConfigureOnce,
        started: &command::StartOnce,
        memory: &TransferOnce,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        self.begin_inner(setup, endpoints, started, Some(memory), pool, io)
    }
    fn begin_inner<M: Memory>(
        &mut self,
        setup: &gsi::SetupOnce,
        endpoints: &endpoint::ConfigureOnce,
        started: &command::StartOnce,
        memory: Option<&TransferOnce>,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Preparing;
        if self.table_plan.is_some() != memory.is_some() {
            self.report.phase = Phase::Failed;
            self.report.error = Some(Error::Prerequisite);
            return Err(Error::Prerequisite);
        }
        if let Err(e) = Self::prerequisites(setup, endpoints, started, pool, memory) {
            self.report.phase = Phase::Failed;
            self.report.error = Some(e);
            return Err(e);
        }
        let claim = if self.table_plan.is_some() {
            pool.claim_table_setup()
        } else {
            pool.claim_memory_setup()
        };
        if claim.is_err() {
            self.report.phase = Phase::Failed;
            self.report.error = Some(Error::Prerequisite);
            return Err(Error::Prerequisite);
        }
        let result = (|| {
            if self.read(pool, io, Read::SharedMemorySize)? != 0x415 {
                return Err(Error::Layout);
            }
            self.quiet(pool, io)?;
            self.states(pool, io, command::ENABLED_TYPES, 1)?;
            if self.read(pool, io, Read::EventPointer)? != self.initial_event() {
                return Err(Error::Event);
            }
            if self.table_plan.is_some()
                && self.read(pool, io, Read::LocalContext)? & self.plan.processing_context().2
                    != self.plan.processing_context().1
            {
                return Err(Error::Readback);
            }
            self.canaries(pool, io)?;
            self.targets(pool, io, true)?;
            let zero = pool
                .buffer(buffers::Kind::Zero)
                .ok_or(Error::Prerequisite)?;
            io.guard(pool)?;
            let event = pool
                .buffer(buffers::Kind::Event(1))
                .ok_or(Error::Prerequisite)?;
            for offset in (event.offset() + self.event_offset()
                ..event.offset() + self.event_offset() + 16)
                .step_by(4)
            {
                if pool.memory_setup_read32(offset).map_err(|_| Error::Pool)? != 0 {
                    return Err(Error::Event);
                }
            }
            for offset in (zero.offset()..zero.offset() + zero.requested()).step_by(4) {
                if pool.memory_setup_read32(offset).map_err(|_| Error::Pool)? != 0 {
                    return Err(Error::Readback);
                }
                self.report.zero_source_words += 1;
            }
            if let Some(plan) = &self.table_plan {
                for (index, bytes) in plan.table_bytes().chunks_exact(4).enumerate() {
                    if pool
                        .memory_setup_read32(plan.table_offset() + index * 4)
                        .map_err(|_| Error::Pool)?
                        != u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Layout)?)
                    {
                        return Err(Error::Readback);
                    }
                    self.report.table_source_words += 1;
                }
            }
            self.descriptors(pool, io, true)?;
            self.descriptors(pool, io, false)?;
            self.quiet(pool, io)?;
            self.states(pool, io, command::ENABLED_TYPES, 1)?;
            if self.read(pool, io, Read::EventPointer)? != self.initial_event() {
                return Err(Error::Event);
            }
            self.report.phase = Phase::Pending;
            self.write(pool, io, Register::ChannelDoorbell, self.channel_doorbell())
        })();
        result.map_err(|e| self.fail(pool, io, e))
    }
    /// Only after delivery of this boot's exclusive GSI IRQ notification.
    pub fn notified<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
        now: u64,
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        if now.wrapping_sub(self.started_at) >= self.timeout_ticks {
            return Err(self.fail(pool, io, Error::Timeout));
        }
        let result = (|| {
            use command::Read as R;
            if self.read(pool, io, Read::Gsi(R::TypeStatus))? != 8
                || self.read(pool, io, Read::Gsi(R::IeobStatus))? != 1
            {
                return Err(Error::UnexpectedIrq);
            }
            for r in [
                R::ChannelStatus,
                R::EventStatus,
                R::GlobalStatus,
                R::GeneralStatus,
            ] {
                if self.read(pool, io, Read::Gsi(r))? != 0 {
                    return Err(Error::UnexpectedIrq);
                }
            }
            self.states(pool, io, command::ENABLED_TYPES, 1)?;
            // Same disable/clear ordering as gsi_isr_ieob(): type first
            // when disabling the last enabled event, then event mask/clear.
            self.write(pool, io, Register::TypeMask, gsi::BASE_IRQ_TYPES)?;
            self.write(pool, io, Register::IeobMask, 0)?;
            self.write(pool, io, Register::IeobClear, 1)?;
            self.quiet(pool, io)?;
            self.states(pool, io, gsi::BASE_IRQ_TYPES, 0)?;
            let pointer = self.read(pool, io, Read::EventPointer)?;
            self.report.event_pointer = Some(pointer);
            let event_buffer = pool
                .buffer(buffers::Kind::Event(1))
                .ok_or(Error::Prerequisite)?;
            let mut bytes = [0u8; 16];
            io.guard(pool)?;
            for i in 0..4 {
                bytes[i * 4..i * 4 + 4].copy_from_slice(
                    &pool
                        .memory_setup_read32(event_buffer.offset() + self.event_offset() + i * 4)
                        .map_err(|_| Error::Pool)?
                        .to_le_bytes(),
                );
            }
            self.report.event = Some(self.completed_event(pointer, &bytes)?);
            // Re-sample all bytes and hardware producer before retiring the
            // event; reject unstable/foreign/extra events without ACK/reuse.
            for i in 0..4 {
                if pool
                    .memory_setup_read32(event_buffer.offset() + self.event_offset() + i * 4)
                    .map_err(|_| Error::Pool)?
                    != u32::from_le_bytes(
                        bytes[i * 4..i * 4 + 4]
                            .try_into()
                            .map_err(|_| Error::Layout)?,
                    )
                {
                    return Err(Error::Event);
                }
            }
            if self.read(pool, io, Read::EventPointer)? != pointer {
                return Err(Error::Event);
            }
            self.write(pool, io, Register::EventDoorbell, self.event_doorbell())?;
            self.canaries(pool, io)?;
            self.targets(pool, io, false)?;
            // NAPI's final re-enable, then ipa_mem_setup's direct context
            // write. Native dispatch does the bounded event work before ACK.
            self.write(pool, io, Register::IeobMask, 1)?;
            self.write(pool, io, Register::TypeMask, command::ENABLED_TYPES)?;
            self.quiet(pool, io)?;
            self.states(pool, io, command::ENABLED_TYPES, 1)?;
            if self.table_plan.is_none() {
                self.write(
                    pool,
                    io,
                    Register::LocalContext,
                    self.plan.processing_context().1,
                )?;
            }
            for _ in 0..2 {
                if self.read(pool, io, Read::LocalContext)? & self.plan.processing_context().2
                    != self.plan.processing_context().1
                    || self.read(pool, io, Read::EventPointer)? != pointer
                {
                    return Err(Error::Readback);
                }
                self.quiet(pool, io)?;
                self.states(pool, io, command::ENABLED_TYPES, 1)?;
                self.report.final_samples += 1;
            }
            io.guard(pool)?;
            self.report.ack_sent = true;
            io.acknowledge()?;
            self.report.ack_done = true;
            if self.table_plan.is_some() {
                pool.finish_table_setup()
            } else {
                pool.finish_memory_setup()
            }
            .map_err(|_| Error::Pool)?;
            self.report.phase = Phase::Complete;
            Ok(())
        })();
        result.map_err(|e| self.fail(pool, io, e))
    }
    pub fn timeout<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
        now: u64,
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        if now.wrapping_sub(self.started_at) >= self.timeout_ticks {
            return Err(self.fail(pool, io, Error::Timeout));
        }
        Ok(())
    }
    pub fn external_fault<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        Err(self.fail(pool, io, Error::Guard))
    }
    /// A late delivered notification invalidates the diagnostic completion;
    /// resources stay retained and no stale success can survive containment.
    pub fn late_fault<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        if !matches!(self.report.phase, Phase::Pending | Phase::Complete) {
            return Err(Error::Consumed);
        }
        Err(self.fail(pool, io, Error::UnexpectedIrq))
    }
}

struct Encoded {
    offset: usize,
    size: usize,
    payload_offset: usize,
    tre_offset: usize,
    payload: [u8; 32],
    tre: [u8; 16],
}
