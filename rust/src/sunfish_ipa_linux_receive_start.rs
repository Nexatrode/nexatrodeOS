//! Retained exception-RX startup core; no native backend or Ready token.
//!
//! Owns the original rings/command pool and separate receive data pool.
//! Requires actual GSI, endpoint, command, memory and table owners. Native
//! integration must bind every guard/store to those retained physical
//! resources; this core alone supplies no SMMU or physical DMA authority.
#![forbid(unsafe_code)]

use crate::{
    sunfish_ipa_linux_buffers::{Memory, Pool},
    sunfish_ipa_linux_command as command,
    sunfish_ipa_linux_endpoint::{self as endpoint, ENDPOINTS},
    sunfish_ipa_linux_gsi as gsi,
    sunfish_ipa_linux_receive::{self as rx, Plan, Space, Word},
    sunfish_ipa_linux_receive_pool::{self as data, DataPool},
    sunfish_ipa_linux_table_transfer::TableOnce,
    sunfish_ipa_linux_transfer::{self as transfer, TransferOnce},
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
    Timeout,
    Acknowledge,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Preparing,
    Pending,
    Armed,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Gsi(command::Read),
    RxProducer,
    IpaEnable,
    SuspendEnable,
    DefaultRoute,
}
impl Read {
    pub const fn space(self) -> Space {
        match self {
            Self::Gsi(_) | Self::RxProducer => Space::Gsi,
            _ => Space::Ipa,
        }
    }
    pub const fn offset(self) -> usize {
        match self {
            Self::Gsi(r) => r.offset(),
            Self::RxProducer => 0x1d090,
            Self::IpaEnable => 0x300c,
            Self::SuspendEnable => 0x3034,
            Self::DefaultRoute => 0x48,
        }
    }
}
/// Read-only transition expectation; only this core can construct one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuardState {
    ipa_mask: u32,
    suspend: u32,
    types: u32,
    ieob: u32,
    channel_mask: u32,
    running: bool,
    route: bool,
}
impl GuardState {
    #[allow(dead_code)] // Earlier standalone startup suites omit the runtime.
    pub(crate) fn set_runtime_ieob(&mut self, mask: u32) {
        self.ieob = mask;
    }
    pub const fn ipa_mask(self) -> u32 {
        self.ipa_mask
    }
    pub const fn suspend_mask(self) -> u32 {
        self.suspend
    }
    pub const fn gsi_types(self) -> u32 {
        self.types
    }
    pub const fn gsi_ieob(self) -> u32 {
        self.ieob
    }
    pub const fn gsi_channel_mask(self) -> u32 {
        self.channel_mask
    }
    pub const fn rx_running(self) -> bool {
        self.running
    }
    pub const fn route_set(self) -> bool {
        self.route
    }
}
pub struct Request {
    word: Word,
}
impl Request {
    pub const fn word(&self) -> Word {
        self.word
    }
}
pub trait Io<R: Memory, D: Memory> {
    fn ticks(&mut self) -> u64;
    /// Actual domain, both physical pools, firmware/power/config and prior
    /// owners must still be valid. GuardState permits only tracked IRQ changes.
    fn guard(
        &mut self,
        rings: &Pool<R>,
        data: &DataPool<D>,
        state: GuardState,
    ) -> Result<(), Error>;
    fn read(&mut self, r: Read) -> Result<u32, Error>;
    fn write(&mut self, r: &Request) -> Result<(), Error>;
    fn acknowledge_start(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub cpu_sent: usize,
    pub cpu_done: usize,
    pub cpu_checked: usize,
    pub blank_words: usize,
    pub guard_words: usize,
    pub writes_sent: u8,
    pub writes_done: u8,
    pub start_sent: bool,
    pub ack_sent: bool,
    pub ack_done: bool,
    pub published_buffers: usize,
    pub doorbells: u8,
    pub route_done: bool,
    pub final_samples: u8,
    pub rx_pending: bool,
    pub rx_producer: Option<u32>,
    pub containment_sent: u8,
    pub containment_done: u8,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}
pub struct ReceiveOnce<R: Memory, D: Memory> {
    rings: Pool<R>,
    data: DataPool<D>,
    plan: Plan,
    report: Report,
    expected: GuardState,
    timeout_ticks: u64,
    started_at: u64,
}
impl<R: Memory, D: Memory> ReceiveOnce<R, D> {
    /// Always retains both owners, even if begin rejects its preconditions.
    /// No extraction/free/retry API is provided.
    pub fn new(rings: Pool<R>, data: DataPool<D>, timeout_ticks: u64) -> Self {
        let plan = data.plan();
        Self {
            rings,
            data,
            plan,
            timeout_ticks,
            started_at: 0,
            expected: GuardState {
                ipa_mask: 0xc,
                suspend: 0,
                types: 0x4c,
                ieob: 1,
                channel_mask: 0,
                running: false,
                route: false,
            },
            report: Report {
                phase: Phase::New,
                cpu_sent: 0,
                cpu_done: 0,
                cpu_checked: 0,
                blank_words: 0,
                guard_words: 0,
                writes_sent: 0,
                writes_done: 0,
                start_sent: false,
                ack_sent: false,
                ack_done: false,
                published_buffers: 0,
                doorbells: 0,
                route_done: false,
                final_samples: 0,
                rx_pending: false,
                rx_producer: None,
                containment_sent: 0,
                containment_done: 0,
                error: None,
                containment_error: None,
            },
        }
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub const fn plan(&self) -> Plan {
        self.plan
    }
    pub fn pools(&self) -> (&Pool<R>, &DataPool<D>) {
        (&self.rings, &self.data)
    }
    /// Borrowed only by the retained runtime, never extracted or duplicated.
    #[allow(dead_code)]
    pub(crate) fn runtime_parts(
        &mut self,
    ) -> Result<(&mut Pool<R>, &mut DataPool<D>, GuardState), Error> {
        let expected = self.runtime_state()?;
        Ok((&mut self.rings, &mut self.data, expected))
    }
    #[allow(dead_code)]
    pub(crate) fn runtime_state(&self) -> Result<GuardState, Error> {
        if self.report.phase != Phase::Armed
            || self.report.error.is_some()
            || !self.report.ack_done
            || !self.report.route_done
            || self.report.published_buffers != rx::BUFFER_COUNT
            || self.report.doorbells as usize != rx::DOORBELL_COUNT
            || self.report.final_samples != 2
            || !self.expected.running
            || !self.expected.route
            || !self.rings.table_setup_complete()
            || !self.data.prepared()
        {
            return Err(Error::Prerequisite);
        }
        Ok(self.expected)
    }
    #[allow(dead_code)]
    pub(crate) fn quarantine_pools(&mut self) {
        self.rings.quarantine();
        self.data.quarantine();
    }
    /// The shared IPA dispatcher already attempted its sole IPA mask. Do
    /// not repeat that uncertain store; contain GSI and retain both pools.
    pub fn ipa_dispatch_failed(&mut self, io: &mut impl Io<R, D>) {
        if self.report.phase == Phase::Failed {
            return;
        }
        self.report.phase = Phase::Failed;
        self.report.error = Some(Error::Guard);
        self.report.containment_sent += 1;
        match io.write(&Request {
            word: Word {
                space: Space::Gsi,
                offset: 0x1f088,
                value: 0,
            },
        }) {
            Ok(()) => self.report.containment_done += 1,
            Err(e) => {
                self.report.containment_error.get_or_insert(e);
            }
        }
        self.quarantine_pools();
    }
    fn guard(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        io.guard(&self.rings, &self.data, self.expected)
    }
    fn read(&mut self, io: &mut impl Io<R, D>, r: Read) -> Result<u32, Error> {
        self.guard(io)?;
        io.read(r)
    }
    fn write(&mut self, io: &mut impl Io<R, D>, word: Word) -> Result<(), Error> {
        self.guard(io)?;
        if word.space == Space::Gsi && word.offset == 0x1f008 {
            self.started_at = io.ticks();
            self.report.start_sent = true;
        }
        self.report.writes_sent += 1;
        io.write(&Request { word })?;
        self.report.writes_done += 1;
        match (word.space, word.offset) {
            (Space::Ipa, 0x300c) => self.expected.ipa_mask = word.value,
            (Space::Ipa, 0x3034) => self.expected.suspend = word.value,
            (Space::Ipa, 0x48) => self.expected.route = true,
            (Space::Gsi, 0x1f088) => self.expected.types = word.value,
            (Space::Gsi, 0x1f0b8) => self.expected.ieob = word.value,
            (Space::Gsi, 0x1f098) => self.expected.channel_mask = word.value,
            _ => {}
        }
        Ok(())
    }
    fn fail(&mut self, io: &mut impl Io<R, D>, e: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error = Some(e);
        // No rollback/reset/free. Published receive buffers remain mapped
        // and retained even if their hardware engine can still write them.
        for word in [
            Word {
                space: Space::Gsi,
                offset: 0x1f088,
                value: 0,
            },
            Word {
                space: Space::Ipa,
                offset: 0x300c,
                value: 0,
            },
        ] {
            self.report.containment_sent += 1;
            match io.write(&Request { word }) {
                Ok(()) => self.report.containment_done += 1,
                Err(e) => {
                    self.report.containment_error.get_or_insert(e);
                }
            }
        }
        self.rings.quarantine();
        self.data.quarantine();
        e
    }
    fn owners(
        &self,
        g: &gsi::SetupOnce,
        e: &endpoint::ConfigureOnce,
        c: &command::StartOnce,
        m: &TransferOnce,
        t: &TableOnce,
    ) -> Result<(), Error> {
        let gr = g.report();
        let er = e.report();
        let cr = c.report();
        let mr = m.report();
        let tr = t.report();
        if gr.phase != gsi::SetupPhase::Complete
            || gr.error.is_some()
            || gr.mask_sent
            || er.phase != endpoint::Phase::Complete
            || er.error.is_some()
            || er.containment_sent
            || er.readback != endpoint::COMPLETE
            || er.writes_done != endpoint::COMPLETE
            || er.endpoints_done != 15
            || er.final_samples != 2
            || cr.phase != command::Phase::Complete
            || cr.error.is_some()
            || cr.mask_sent
            || !cr.ack_done
            || cr.state != Some(2)
            || mr.phase != transfer::Phase::Complete
            || mr.error.is_some()
            || mr.containment_sent
            || !mr.ack_done
            || mr.target_words != 1619
            || tr.phase != transfer::Phase::Complete
            || tr.error.is_some()
            || tr.containment_sent
            || !tr.ack_done
            || tr.cpu_verified != 72
            || tr.writes_done != 7
            || tr.target_words != 120
            || tr.canary_checks != 50
            || tr.final_samples != 2
            || !self.rings.table_setup_complete()
            || !self.data.prepared()
        {
            return Err(Error::Prerequisite);
        }
        let ch = g.plan().channels()[rx::CHANNEL as usize];
        if ch.id != 2 || ch.endpoint != 8 || ch.event != 1 || ch.command || ch.toward_ipa {
            return Err(Error::Prerequisite);
        }
        Ok(())
    }
    fn guards(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        let expected = self.expected;
        let rings = &self.rings;
        let r = self.data.verify_guards_with(|data| {
            io.guard(rings, data, expected)
                .map_err(|_| data::Error::Guard)
        });
        self.report.guard_words += r.checked_words;
        if let Some(e) = r.error {
            return Err(if e == data::Error::Guard {
                Error::Guard
            } else {
                Error::Pool
            });
        }
        Ok(())
    }
    fn quiet(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        use command::Read as Q;
        for r in [
            Q::TypeStatus,
            Q::ChannelStatus,
            Q::EventStatus,
            Q::GlobalStatus,
            Q::GeneralStatus,
            Q::IeobStatus,
        ] {
            if self.read(io, Read::Gsi(r))? != 0 {
                return Err(Error::Pending);
            }
        }
        Ok(())
    }
    fn state(&mut self, io: &mut impl Io<R, D>, rx_state: u32) -> Result<(), Error> {
        use command::Read as Q;
        for (r, want) in [
            (Q::Status, 1),
            (Q::TypeMask, self.expected.types),
            (Q::ChannelMask, self.expected.channel_mask),
            (Q::EventMask, 0),
            (Q::GlobalEnable, 1),
            (Q::GeneralEnable, 14),
            (Q::IeobMask, self.expected.ieob),
        ] {
            if self.read(io, Read::Gsi(r))? != want {
                return Err(Error::State);
            }
        }
        for e in ENDPOINTS {
            let want = match e.channel() {
                1 => 2,
                2 => rx_state,
                _ => 1,
            };
            if (self.read(io, Read::Gsi(Q::Channel(e)))? >> 20) & 15 != want
                || (self.read(io, Read::Gsi(Q::Event(e)))? >> 20) & 15 != 1
            {
                return Err(Error::State);
            }
        }
        if self.read(io, Read::IpaEnable)? != self.expected.ipa_mask
            || self.read(io, Read::SuspendEnable)? != self.expected.suspend
        {
            return Err(Error::State);
        }
        Ok(())
    }
    fn blank(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        for base in [
            self.plan.buffer(0).map_err(|_| Error::Layout)?.tre_offset(),
            self.plan.event_offset(),
        ] {
            self.guard(io)?;
            for offset in (base..base + rx::RING_COUNT * 16).step_by(4) {
                if self
                    .rings
                    .memory_setup_read32(offset)
                    .map_err(|_| Error::Pool)?
                    != 0
                {
                    return Err(Error::Readback);
                }
                self.report.blank_words += 1;
            }
        }
        Ok(())
    }
    fn descriptors(&mut self, io: &mut impl Io<R, D>, stage: bool) -> Result<(), Error> {
        for index in 0..rx::BUFFER_COUNT {
            self.guard(io)?;
            let b = self.plan.buffer(index).map_err(|_| Error::Layout)?;
            for (n, bytes) in b.tre().chunks_exact(4).enumerate() {
                let offset = b.tre_offset() + n * 4;
                let value = u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Layout)?);
                if stage {
                    self.report.cpu_sent += 1;
                    self.rings
                        .receive_write32(offset, value)
                        .map_err(|_| Error::Pool)?;
                    self.report.cpu_done += 1;
                }
                if self
                    .rings
                    .memory_setup_read32(offset)
                    .map_err(|_| Error::Pool)?
                    != value
                {
                    return Err(Error::Readback);
                }
                if !stage {
                    self.report.cpu_checked += 1;
                }
            }
            self.guard(io)?;
        }
        Ok(())
    }
    pub fn begin(
        &mut self,
        g: &gsi::SetupOnce,
        e: &endpoint::ConfigureOnce,
        c: &command::StartOnce,
        m: &TransferOnce,
        t: &TableOnce,
        io: &mut impl Io<R, D>,
    ) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Preparing;
        // Rejection still retains both pools and consumes this coordinator.
        if self.timeout_ticks == 0 || self.timeout_ticks >= 1 << 63 {
            self.report.phase = Phase::Failed;
            self.report.error = Some(Error::Layout);
            return Err(Error::Layout);
        }
        if let Err(e) = self.owners(g, e, c, m, t) {
            self.report.phase = Phase::Failed;
            self.report.error = Some(e);
            return Err(e);
        }
        let result = (|| {
            self.rings.claim_receive().map_err(|_| Error::Pool)?;
            self.quiet(io)?;
            self.state(io, 1)?;
            self.guards(io)?;
            self.blank(io)?;
            self.descriptors(io, true)?;
            self.descriptors(io, false)?;
            self.quiet(io)?;
            self.state(io, 1)?;
            for word in &rx::START_WORDS[..4] {
                self.write(io, *word)?;
            }
            self.quiet(io)?;
            self.state(io, 1)?;
            self.report.phase = Phase::Pending;
            self.write(io, rx::START_WORDS[4])
        })();
        result.map_err(|e| self.fail(io, e))
    }
    /// Only after this boot's exclusive physical GSI notification is delivered.
    pub fn notified(&mut self, io: &mut impl Io<R, D>, now: u64) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        if now.wrapping_sub(self.started_at) >= self.timeout_ticks {
            return Err(self.fail(io, Error::Timeout));
        }
        let result = (|| {
            use command::Read as Q;
            if self.read(io, Read::Gsi(Q::TypeStatus))? != 1
                || self.read(io, Read::Gsi(Q::ChannelStatus))? != 4
            {
                return Err(Error::UnexpectedIrq);
            }
            for r in [
                Q::EventStatus,
                Q::GlobalStatus,
                Q::GeneralStatus,
                Q::IeobStatus,
            ] {
                if self.read(io, Read::Gsi(r))? != 0 {
                    return Err(Error::UnexpectedIrq);
                }
            }
            self.state(io, 2)?;
            self.expected.running = true;
            for word in &rx::START_WORDS[5..] {
                self.write(io, *word)?;
            }
            self.quiet(io)?;
            self.state(io, 2)?;
            self.guard(io)?;
            self.report.ack_sent = true;
            io.acknowledge_start()?;
            self.report.ack_done = true;
            // ACK the cleared START source before any RX publication. A later
            // RX delivery belongs to its own event-drain path, not this ACK.
            for word in rx::SUSPEND_WORDS {
                self.write(io, word)?;
            }
            self.state(io, 2)?;
            for batch in 0..rx::DOORBELL_COUNT {
                let db = self.plan.doorbell(batch).map_err(|_| Error::Layout)?;
                self.write(io, db.word)?;
                self.report.doorbells += 1;
                self.report.published_buffers = db.published_buffers;
            }
            self.write(io, rx::DEFAULT_ROUTE)?;
            self.report.route_done = true;
            self.guards(io)?;
            for _ in 0..2 {
                self.state(io, 2)?;
                if self.read(io, Read::DefaultRoute)? & 0x013fffff != rx::DEFAULT_ROUTE.value {
                    return Err(Error::Readback);
                }
                // RX can arrive while the initial buffers are being posted.
                // Record it without clearing/ACKing any receive indication.
                let types = self.read(io, Read::Gsi(Q::TypeStatus))?;
                let ieob = self.read(io, Read::Gsi(Q::IeobStatus))?;
                if types & !8 != 0 || ieob & !2 != 0 {
                    return Err(Error::UnexpectedIrq);
                }
                for r in [
                    Q::ChannelStatus,
                    Q::EventStatus,
                    Q::GlobalStatus,
                    Q::GeneralStatus,
                ] {
                    if self.read(io, Read::Gsi(r))? != 0 {
                        return Err(Error::UnexpectedIrq);
                    }
                }
                if ieob != 0 {
                    // Linux reads this producer while servicing completions,
                    // not as an unstarted/empty-ring invariant. The event
                    // owner must order and bounds-check it before DMA reads.
                    self.report.rx_producer = Some(self.read(io, Read::RxProducer)?);
                }
                self.report.rx_pending |= types != 0 || ieob != 0;
                self.report.final_samples += 1;
            }
            self.report.phase = Phase::Armed;
            Ok(())
        })();
        result.map_err(|e| self.fail(io, e))
    }
    pub fn timeout(&mut self, io: &mut impl Io<R, D>, now: u64) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        if now.wrapping_sub(self.started_at) >= self.timeout_ticks {
            return Err(self.fail(io, Error::Timeout));
        }
        Ok(())
    }
    /// Only a foreign/fault indication. Legitimate RX is left to the receive
    /// event parser/dispatcher; Armed does not imply its data path is proved.
    pub fn external_fault(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        if !matches!(self.report.phase, Phase::Pending | Phase::Armed) {
            return Err(Error::Consumed);
        }
        Err(self.fail(io, Error::Guard))
    }
}
