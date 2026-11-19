//! AP endpoint programming for Linux IPA v4.2, kernel 2267e248204e7e051e0f7355298949a6440de579.
//!
//! This follows ipa_endpoint_setup()/ipa_endpoint_program() after the retained
//! GSI allocation owner completes. It programs only AP endpoints 1/6/8/9, in
//! endpoint-ID order. No channel START, TRE publication, DMA, modem boot or
//! production Ready capability is exposed. Failures consume the attempt and
//! retain partial configuration; the trusted backend contains interrupts.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_gsi::{self as gsi, SetupOnce, SetupPhase};

pub const COUNT: usize = 36;
pub const COMPLETE: u64 = (1 << COUNT) - 1;
/// Linux's 8-KiB receive buffers use a soft six-KiB aggregation threshold.
/// Exact reference ABI: NET_SKB_PAD=64, aligned skb_shared_info=320,
/// IPA_RX_BUFFER_OVERHEAD=384, MTU=1500; (8192-64-1500-384)/1024 = 6.
/// These constants configure the endpoint; they do not allocate RX buffers.
pub const AGGREGATION_KB: u32 = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Endpoint {
    ModemTx,
    CommandTx,
    LanRx,
    ModemRx,
}
pub const ENDPOINTS: [Endpoint; 4] = [
    Endpoint::ModemTx,
    Endpoint::CommandTx,
    Endpoint::LanRx,
    Endpoint::ModemRx,
];
impl Endpoint {
    pub const fn id(self) -> u8 {
        [1, 6, 8, 9][self as usize]
    }
    pub const fn channel(self) -> u8 {
        self as u8
    }
    pub const fn event(self) -> u8 {
        [2, 0, 1, 3][self as usize]
    }
    pub const fn toward_ipa(self) -> bool {
        matches!(self, Self::ModemTx | Self::CommandTx)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Config,
    Nat,
    Header,
    HeaderExt,
    MetadataMask,
    Mode,
    Aggregation,
    HolBlock,
    Deaggregation,
    Resource,
    Sequence,
    Status,
}
impl Register {
    pub const fn base(self) -> usize {
        match self {
            Self::Config => 0x808,
            Self::Nat => 0x80c,
            Self::Header => 0x810,
            Self::HeaderExt => 0x814,
            Self::MetadataMask => 0x818,
            Self::Mode => 0x820,
            Self::Aggregation => 0x824,
            Self::HolBlock => 0x82c,
            Self::Deaggregation => 0x834,
            Self::Resource => 0x838,
            Self::Sequence => 0x83c,
            Self::Status => 0x840,
        }
    }
    pub const fn mask(self) -> u32 {
        match self {
            Self::Config => 0x17f,
            Self::Nat => 3,
            Self::Header => 0x1fffffff,
            Self::HeaderExt => 0x3fff,
            Self::MetadataMask => u32::MAX,
            Self::Mode => 0x7ffff1f7,
            Self::Aggregation => 0x017fffff,
            Self::HolBlock => 1,
            Self::Deaggregation => 0xffff7fff,
            Self::Resource => 1,
            Self::Sequence => 0xffff,
            Self::Status => 0x33f,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Word {
    pub endpoint: Endpoint,
    pub register: Register,
    pub value: u32,
}
impl Word {
    pub const fn offset(self) -> usize {
        self.register.base() + self.endpoint.id() as usize * 0x70
    }
}
const fn w(endpoint: Endpoint, register: Register, value: u32) -> Word {
    Word {
        endpoint,
        register,
        value,
    }
}
use Endpoint::{CommandTx, LanRx, ModemRx, ModemTx};
use Register::{
    Aggregation, Config, Deaggregation, Header, HeaderExt, HolBlock, MetadataMask, Mode, Nat,
    Resource, Sequence, Status,
};
/// Diagnostic profile, not a write capability. Only Request can cross Io::write.
pub const PROFILE: [Word; COUNT] = [
    w(ModemTx, Config, 0x0a),
    w(ModemTx, Nat, 0),
    w(ModemTx, Header, 0x48),
    w(ModemTx, HeaderExt, 1),
    w(ModemTx, Mode, 0),
    w(ModemTx, Aggregation, 0),
    w(ModemTx, Deaggregation, 0),
    w(ModemTx, Resource, 0),
    w(ModemTx, Sequence, 0x806),
    w(ModemTx, Status, 0x15),
    w(CommandTx, Config, 0),
    w(CommandTx, Nat, 0),
    w(CommandTx, Header, 0),
    w(CommandTx, HeaderExt, 0),
    w(CommandTx, Mode, 0x83),
    w(CommandTx, Aggregation, 0),
    w(CommandTx, Deaggregation, 0),
    w(CommandTx, Resource, 0),
    w(CommandTx, Sequence, 0),
    w(CommandTx, Status, 0),
    w(LanRx, Config, 0),
    w(LanRx, Header, 0),
    w(LanRx, HeaderExt, 0x800),
    w(LanRx, MetadataMask, 0),
    w(LanRx, Aggregation, 0x411 | (AGGREGATION_KB << 5)),
    w(LanRx, HolBlock, 0),
    w(LanRx, Resource, 0),
    w(LanRx, Status, 1),
    w(ModemRx, Config, 4),
    w(ModemRx, Header, 0x2800c4),
    w(ModemRx, HeaderExt, 0x0b),
    w(ModemRx, MetadataMask, 0xff000000),
    w(ModemRx, Aggregation, 0x200411 | (AGGREGATION_KB << 5)),
    w(ModemRx, HolBlock, 0),
    w(ModemRx, Resource, 0),
    w(ModemRx, Status, 0),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Endpoint(Word),
    GsiStatus,
    TypeStatus,
    GlobalStatus,
    GeneralStatus,
    ChannelStatus,
    EventStatus,
    TypeMask,
    ChannelMask,
    EventMask,
    GlobalEnable,
    GeneralEnable,
    Channel(Endpoint),
    Event(Endpoint),
}
impl Read {
    pub const fn is_gsi(self) -> bool {
        !matches!(self, Self::Endpoint(_))
    }
    pub const fn offset(self) -> usize {
        match self {
            Self::Endpoint(w) => w.offset(),
            Self::GsiStatus => gsi::GSI_STATUS,
            Self::TypeStatus => gsi::TYPE_STATUS,
            Self::GlobalStatus => gsi::GLOBAL_STATUS,
            Self::GeneralStatus => gsi::GENERAL_STATUS,
            Self::ChannelStatus => gsi::CHANNEL_STATUS,
            Self::EventStatus => gsi::EVENT_STATUS,
            Self::TypeMask => gsi::TYPE_MASK,
            Self::ChannelMask => gsi::CHANNEL_MASK,
            Self::EventMask => gsi::EVENT_MASK,
            Self::GlobalEnable => gsi::GLOBAL_ENABLE,
            Self::GeneralEnable => 0x1f120,
            Self::Channel(e) => 0x1c000 + e.channel() as usize * 0x80,
            Self::Event(e) => 0x1d000 + e.event() as usize * 0x80,
        }
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    GsiNotComplete,
    Layout,
    Guard,
    Io,
    State,
    Pending,
    Readback,
}
pub trait Io {
    /// Retained power/domain/pool/PAS owners and their actual fault/config state.
    fn guard(&mut self) -> Result<(), Error>;
    fn read(&mut self, register: Read) -> Result<u32, Error>;
    fn write(&mut self, request: &Request) -> Result<(), Error>;
    /// Disable the owned GSI/IPA interrupt sources once. No clear, ACK, rollback,
    /// reset or free; the trusted backend retains all partial state until reboot.
    fn contain(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Programming,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub writes_sent: u64,
    pub writes_done: u64,
    pub readback: u64,
    pub endpoints_done: u8,
    pub final_samples: u8,
    pub last_word: Option<Word>,
    pub last_value: Option<u32>,
    pub error: Option<Error>,
    pub containment_sent: bool,
    pub containment_error: Option<Error>,
}
pub struct ConfigureOnce {
    report: Report,
}
impl Default for ConfigureOnce {
    fn default() -> Self {
        Self::new()
    }
}
impl ConfigureOnce {
    pub const fn new() -> Self {
        Self {
            report: Report {
                phase: Phase::New,
                writes_sent: 0,
                writes_done: 0,
                readback: 0,
                endpoints_done: 0,
                final_samples: 0,
                last_word: None,
                last_value: None,
                error: None,
                containment_sent: false,
                containment_error: None,
            },
        }
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    fn require_setup(setup: &SetupOnce) -> Result<(), Error> {
        let r = setup.report();
        if r.phase != SetupPhase::Complete
            || r.index != 4
            || !r.irq_bound
            || r.error.is_some()
            || r.mask_sent
            || r.containment_error.is_some()
            || [
                r.events_allocated,
                r.events_programmed,
                r.channels_allocated,
                r.channels_programmed,
                r.modem_accepted,
            ]
            .iter()
            .any(|&v| v != 15)
        {
            return Err(Error::GsiNotComplete);
        }
        for (ch, e) in setup.plan().channels().iter().zip(ENDPOINTS) {
            if ch.id != e.channel()
                || ch.endpoint != e.id()
                || ch.event != e.event()
                || ch.toward_ipa != e.toward_ipa()
            {
                return Err(Error::Layout);
            }
        }
        Ok(())
    }
    fn idle(io: &mut impl Io) -> Result<(), Error> {
        io.guard()?;
        if io.read(Read::GsiStatus)? != 1 {
            return Err(Error::State);
        }
        for r in [
            Read::TypeStatus,
            Read::GlobalStatus,
            Read::GeneralStatus,
            Read::ChannelStatus,
            Read::EventStatus,
        ] {
            if io.read(r)? != 0 {
                return Err(Error::Pending);
            }
        }
        for (r, expected) in [
            (Read::TypeMask, gsi::BASE_IRQ_TYPES),
            (Read::ChannelMask, 0),
            (Read::EventMask, 0),
            (Read::GlobalEnable, 1),
            (Read::GeneralEnable, 14),
        ] {
            if io.read(r)? != expected {
                return Err(Error::State);
            }
        }
        for e in ENDPOINTS {
            for r in [Read::Channel(e), Read::Event(e)] {
                if (io.read(r)? >> 20) & 15 != 1 {
                    return Err(Error::State);
                }
            }
        }
        io.guard()
    }
    fn fail(&mut self, io: &mut impl Io, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error = Some(error);
        self.report.containment_sent = true;
        self.report.containment_error = io.contain().err();
        error
    }
    fn verify(&mut self, io: &mut impl Io, word: Word) -> Result<(), Error> {
        let value = io.read(Read::Endpoint(word))?;
        self.report.last_word = Some(word);
        self.report.last_value = Some(value);
        if value & word.register.mask() != word.value & word.register.mask() {
            return Err(Error::Readback);
        }
        Ok(())
    }
    pub fn execute(&mut self, setup: &SetupOnce, io: &mut impl Io) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Programming;
        if let Err(e) = Self::require_setup(setup) {
            // An incomplete or foreign owner authorizes no I/O, even containment.
            self.report.phase = Phase::Failed;
            self.report.error = Some(e);
            return Err(e);
        }
        let result = (|| {
            for (i, word) in PROFILE.into_iter().enumerate() {
                Self::idle(io)?;
                self.report.last_word = Some(word);
                self.report.last_value = None;
                self.report.writes_sent |= 1 << i;
                io.write(&Request { word })?;
                self.report.writes_done |= 1 << i;
                Self::idle(io)?;
                self.verify(io, word)?;
                self.report.readback |= 1 << i;
                if i + 1 == COUNT || PROFILE[i + 1].endpoint != word.endpoint {
                    self.report.endpoints_done |= 1 << word.endpoint as usize;
                }
            }
            for _ in 0..2 {
                Self::idle(io)?;
                for word in PROFILE {
                    self.verify(io, word)?;
                }
                Self::idle(io)?;
                self.report.final_samples += 1;
            }
            self.report.phase = Phase::Complete;
            Ok(())
        })();
        result.map_err(|e| self.fail(io, e))
    }
}
