//! Start the already allocated IPA v4.2 AP command channel once.
//!
//! Pinned Linux 2267e248204e7e051e0f7355298949a6440de579 enables event-ring-0
//! IEOB interrupts before issuing START to channel 1. This stops after its
//! delivered channel-control completion: no TRE or transfer doorbell exists.
//! The native backend must retain and guard the actual pool/domain/PAS, GSI
//! and endpoint owners throughout; reports and plans are not DMA authority.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_endpoint::{self as endpoint, ConfigureOnce, ENDPOINTS};
use crate::sunfish_ipa_linux_gsi::{self as gsi, SetupOnce};

pub const CHANNEL: u8 = 1;
pub const EVENT: u8 = 0;
pub const START: u32 = 0x01000001;
pub const IEOB_MASK: usize = 0x1f0b8;
pub const IEOB_STATUS: usize = 0x1f0b0;
pub const ENABLED_TYPES: u32 = gsi::BASE_IRQ_TYPES | 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Status,
    TypeMask,
    TypeStatus,
    ChannelMask,
    EventMask,
    ChannelStatus,
    EventStatus,
    GlobalStatus,
    GeneralStatus,
    GlobalEnable,
    GeneralEnable,
    IeobMask,
    IeobStatus,
    Channel(endpoint::Endpoint),
    Event(endpoint::Endpoint),
}
impl Read {
    pub const fn offset(self) -> usize {
        match self {
            Self::Status => gsi::GSI_STATUS,
            Self::TypeMask => gsi::TYPE_MASK,
            Self::TypeStatus => gsi::TYPE_STATUS,
            Self::ChannelMask => gsi::CHANNEL_MASK,
            Self::EventMask => gsi::EVENT_MASK,
            Self::ChannelStatus => gsi::CHANNEL_STATUS,
            Self::EventStatus => gsi::EVENT_STATUS,
            Self::GlobalStatus => gsi::GLOBAL_STATUS,
            Self::GeneralStatus => gsi::GENERAL_STATUS,
            Self::GlobalEnable => gsi::GLOBAL_ENABLE,
            Self::GeneralEnable => 0x1f120,
            Self::IeobMask => IEOB_MASK,
            Self::IeobStatus => IEOB_STATUS,
            Self::Channel(e) => 0x1c000 + e.channel() as usize * 0x80,
            Self::Event(e) => 0x1d000 + e.event() as usize * 0x80,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    TypeMask,
    ChannelMask,
    ChannelClear,
    Command,
    IeobMask,
}
impl Register {
    pub const fn offset(self) -> usize {
        match self {
            Self::TypeMask => gsi::TYPE_MASK,
            Self::ChannelMask => gsi::CHANNEL_MASK,
            Self::ChannelClear => gsi::CHANNEL_CLEAR,
            Self::Command => gsi::CHANNEL_COMMAND,
            Self::IeobMask => IEOB_MASK,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Layout,
    Consumed,
    Prerequisite,
    Guard,
    Io,
    State,
    Pending,
    Readback,
    UnexpectedIrq,
    Timeout,
    Acknowledge,
}
pub trait Io {
    fn ticks(&mut self) -> u64;
    fn guard(&mut self) -> Result<(), Error>;
    fn read(&mut self, register: Read) -> Result<u32, Error>;
    fn write(&mut self, request: &Request) -> Result<(), Error>;
    /// Only for the delivered GSI notification after source clear/readback.
    fn acknowledge(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Pending,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub writes_sent: u8,
    pub writes_done: u8,
    pub state: Option<u8>,
    pub irq_type: Option<u32>,
    pub irq_source: Option<u32>,
    pub ack_sent: bool,
    pub ack_done: bool,
    pub mask_sent: bool,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}
pub struct StartOnce {
    report: Report,
    started_at: u64,
    timeout_ticks: u64,
}
impl StartOnce {
    pub fn new(timeout_ticks: u64) -> Result<Self, Error> {
        if timeout_ticks == 0 || timeout_ticks >= 1 << 63 {
            return Err(Error::Layout);
        }
        Ok(Self {
            timeout_ticks,
            started_at: 0,
            report: Report {
                phase: Phase::New,
                writes_sent: 0,
                writes_done: 0,
                state: None,
                irq_type: None,
                irq_source: None,
                ack_sent: false,
                ack_done: false,
                mask_sent: false,
                error: None,
                containment_error: None,
            },
        })
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.report.phase == Phase::Pending)
            .then(|| self.started_at.wrapping_add(self.timeout_ticks))
    }
    fn owners(setup: &SetupOnce, endpoints: &ConfigureOnce) -> Result<(), Error> {
        let g = setup.report();
        let e = endpoints.report();
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
        {
            return Err(Error::Prerequisite);
        }
        let channel = setup.plan().channels()[CHANNEL as usize];
        if channel.id != CHANNEL
            || channel.event != EVENT
            || channel.endpoint != 6
            || !channel.command
            || !channel.toward_ipa
        {
            return Err(Error::Prerequisite);
        }
        Ok(())
    }
    fn read(&mut self, io: &mut impl Io, r: Read) -> Result<u32, Error> {
        io.guard()?;
        io.read(r)
    }
    fn write(&mut self, io: &mut impl Io, register: Register, value: u32) -> Result<(), Error> {
        io.guard()?;
        if register == Register::Command {
            self.started_at = io.ticks();
        }
        self.report.writes_sent += 1;
        io.write(&Request { register, value })?;
        self.report.writes_done += 1;
        Ok(())
    }
    fn fail(&mut self, io: &mut impl Io, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error = Some(error);
        self.report.mask_sent = true;
        self.report.containment_error = io
            .write(&Request {
                register: Register::TypeMask,
                value: 0,
            })
            .err();
        error
    }
    fn quiet_sources(&mut self, io: &mut impl Io) -> Result<(), Error> {
        for r in [
            Read::TypeStatus,
            Read::ChannelStatus,
            Read::EventStatus,
            Read::GlobalStatus,
            Read::GeneralStatus,
            Read::IeobStatus,
        ] {
            if self.read(io, r)? != 0 {
                return Err(Error::Pending);
            }
        }
        Ok(())
    }
    fn states(&mut self, io: &mut impl Io, command: u8) -> Result<(), Error> {
        for e in ENDPOINTS {
            let state = ((self.read(io, Read::Channel(e))? >> 20) & 15) as u8;
            if e.channel() == CHANNEL {
                self.report.state = Some(state);
            }
            if state != if e.channel() == CHANNEL { command } else { 1 }
                || (self.read(io, Read::Event(e))? >> 20) & 15 != 1
            {
                return Err(Error::State);
            }
        }
        Ok(())
    }
    pub fn begin(
        &mut self,
        setup: &SetupOnce,
        endpoints: &ConfigureOnce,
        io: &mut impl Io,
    ) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Pending;
        if let Err(e) = Self::owners(setup, endpoints) {
            self.report.phase = Phase::Failed;
            self.report.error = Some(e);
            return Err(e);
        }
        let result = (|| {
            self.quiet_sources(io)?;
            for (r, v) in [
                (Read::Status, 1),
                (Read::TypeMask, gsi::BASE_IRQ_TYPES),
                (Read::ChannelMask, 0),
                (Read::EventMask, 0),
                (Read::IeobMask, 0),
                (Read::GlobalEnable, 1),
                (Read::GeneralEnable, 14),
            ] {
                if self.read(io, r)? != v {
                    return Err(Error::State);
                }
            }
            self.states(io, 1)?;
            // Full gsi_channel_start(), including event-0 IEOB enable before
            // the source's channel-control enable/START sequence.
            self.write(io, Register::IeobMask, 1)?;
            self.write(io, Register::TypeMask, ENABLED_TYPES)?;
            if self.read(io, Read::IeobMask)? != 1
                || self.read(io, Read::TypeMask)? != ENABLED_TYPES
            {
                return Err(Error::Readback);
            }
            self.quiet_sources(io)?;
            self.write(io, Register::ChannelClear, u32::MAX)?;
            self.write(io, Register::ChannelMask, 2)?;
            self.write(io, Register::TypeMask, ENABLED_TYPES | 1)?;
            if self.read(io, Read::ChannelMask)? != 2
                || self.read(io, Read::TypeMask)? != ENABLED_TYPES | 1
            {
                return Err(Error::Readback);
            }
            self.quiet_sources(io)?;
            self.write(io, Register::Command, START)
        })();
        result.map_err(|e| self.fail(io, e))
    }
    pub fn notified(&mut self, io: &mut impl Io, now: u64) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        if now.wrapping_sub(self.started_at) >= self.timeout_ticks {
            return Err(self.fail(io, Error::Timeout));
        }
        let result = (|| {
            let kind = self.read(io, Read::TypeStatus)?;
            self.report.irq_type = Some(kind);
            let source = self.read(io, Read::ChannelStatus)?;
            self.report.irq_source = Some(source);
            if kind != 1 || source != 2 {
                return Err(Error::UnexpectedIrq);
            }
            for r in [
                Read::GlobalStatus,
                Read::GeneralStatus,
                Read::EventStatus,
                Read::IeobStatus,
            ] {
                if self.read(io, r)? != 0 {
                    return Err(Error::UnexpectedIrq);
                }
            }
            self.write(io, Register::ChannelClear, source)?;
            if self.read(io, Read::ChannelStatus)? != 0 {
                return Err(Error::Pending);
            }
            self.states(io, 2)?;
            self.write(io, Register::TypeMask, ENABLED_TYPES)?;
            self.write(io, Register::ChannelMask, 0)?;
            if self.read(io, Read::TypeMask)? != ENABLED_TYPES
                || self.read(io, Read::ChannelMask)? != 0
                || self.read(io, Read::IeobMask)? != 1
            {
                return Err(Error::Readback);
            }
            self.quiet_sources(io)?;
            io.guard()?;
            self.report.ack_sent = true;
            io.acknowledge()?;
            self.report.ack_done = true;
            self.report.phase = Phase::Complete;
            Ok(())
        })();
        result.map_err(|e| self.fail(io, e))
    }
    pub fn timeout(&mut self, io: &mut impl Io, now: u64) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        if now.wrapping_sub(self.started_at) >= self.timeout_ticks {
            return Err(self.fail(io, Error::Timeout));
        }
        Ok(())
    }
    pub fn external_fault(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if self.report.phase != Phase::Pending {
            return Err(Error::Consumed);
        }
        Err(self.fail(io, Error::Guard))
    }
}
