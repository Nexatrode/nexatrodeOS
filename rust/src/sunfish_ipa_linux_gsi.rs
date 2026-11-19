//! Mainline Sunfish IPA v4.2 GSI context plan and AP allocation protocol.
//!
//! Pin: Linux 2267e248204e7e051e0f7355298949a6440de579. This is separate from
//! the downstream ten-channel profile. Plans and reports are diagnostics, not
//! DMA ownership or startup authority. A native backend must retain the real
//! prepared pool, installed domain, powered firmware and physical IRQ binding.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_buffers::{self as buffers, Kind, Memory, Pool};
pub const AP_COUNT: usize = 4;
pub const IRQ: u32 = 464; // Pinned sm7150.dtsi: SPI 432, level high, plus 32.
pub const CHANNEL_COMMAND: usize = 0x1f008;
pub const EVENT_COMMAND: usize = 0x1f010;
pub const TYPE_STATUS: usize = 0x1f080;
pub const TYPE_MASK: usize = 0x1f088;
pub const CHANNEL_STATUS: usize = 0x1f090;
pub const EVENT_STATUS: usize = 0x1f094;
pub const CHANNEL_MASK: usize = 0x1f098;
pub const EVENT_MASK: usize = 0x1f09c;
pub const CHANNEL_CLEAR: usize = 0x1f0a0;
pub const EVENT_CLEAR: usize = 0x1f0a4;
pub const BASE_IRQ_TYPES: u32 = (1 << 2) | (1 << 6);
/// Linux's 50-ms command timeout, rounded up to the platform counter tick.
pub fn command_timeout_ticks(counter_hz: u64) -> Result<u64, Error> {
    if counter_hz == 0 {
        return Err(Error::Layout);
    }
    Ok(counter_hz / 20 + u64::from(counter_hz % 20 != 0))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotPrepared,
    HardwareLimits,
    Layout,
    Consumed,
    Guard,
    Io,
    State,
    Pending,
    UnexpectedIrq,
    Timeout,
    Readback,
    Acknowledge,
    Binding,
    CommandResult(u8),
}

pub const GSI_STATUS: usize = 0x1f000;
pub const HW_PARAM_2: usize = 0x1f040;
pub const GENERIC_COMMAND: usize = 0x1f018;
pub const GLOBAL_STATUS: usize = 0x1f100;
pub const GLOBAL_ENABLE: usize = 0x1f108;
pub const GLOBAL_CLEAR: usize = 0x1f110;
pub const GENERAL_STATUS: usize = 0x1f118;
pub const ERROR_LOG: usize = 0x1f200;
pub const GENERIC_SCRATCH: usize = 0x1f400;
const GENERIC_RESULT_MASK: u32 = 7 << 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    channels: u8,
    events: u8,
}
impl Limits {
    pub fn decode(hw_param_2: u32) -> Result<Self, Error> {
        let v = Self {
            channels: ((hw_param_2 >> 3) & 31) as u8,
            events: ((hw_param_2 >> 8) & 31) as u8,
        };
        if v.channels < 4 || v.events < 4 {
            return Err(Error::HardwareLimits);
        }
        Ok(v)
    }
    pub const fn channels(self) -> u8 {
        self.channels
    }
    pub const fn events(self) -> u8 {
        self.events
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel {
    pub id: u8,
    pub event: u8,
    pub endpoint: u8,
    pub toward_ipa: bool,
    pub command: bool,
    pub tlvs: u16,
    pub tre_iova: u64,
    pub tre_bytes: u32,
    pub event_iova: u64,
    pub event_bytes: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Word {
    pub offset: usize,
    pub value: u32,
    pub readback_mask: u32,
}
impl Word {
    const fn new(offset: usize, value: u32) -> Self {
        Self {
            offset,
            value,
            readback_mask: u32::MAX,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    channels: [Channel; AP_COUNT],
    limits: Limits,
}
impl Plan {
    pub fn from_prepared<M: Memory>(pool: &Pool<M>, hw_param_2: u32) -> Result<Self, Error> {
        if pool.buffer(Kind::Zero).is_none() {
            return Err(Error::NotPrepared);
        }
        let limits = Limits::decode(hw_param_2)?;
        let mut result = [None; AP_COUNT];
        // Linux initializes in endpoint-name order, assigning the lowest free
        // event ID. Reserved MHI event IDs 10..16 do not affect these four.
        for (event, source) in buffers::CHANNELS.iter().enumerate() {
            let event_buffer = pool
                .buffer(Kind::Event(source.channel_id))
                .ok_or(Error::NotPrepared)?;
            let tre = pool
                .buffer(Kind::Tre(source.channel_id))
                .ok_or(Error::NotPrepared)?;
            if source.channel_id >= 4
                || result[source.channel_id as usize].is_some()
                || source.event_count != source.tre_count
            {
                return Err(Error::Layout);
            }
            for buffer in [event_buffer, tre] {
                let bytes = buffer.requested();
                if !bytes.is_power_of_two()
                    || bytes < 4096
                    || bytes > 0xffff
                    || buffer.iova() % (bytes as u64) != 0
                    || buffer.iova() > u32::MAX as u64
                    || buffer.iova() + bytes as u64
                        > buffers::POOL_IOVA + buffers::POOL_BYTES as u64
                {
                    return Err(Error::Layout);
                }
            }
            result[source.channel_id as usize] = Some(Channel {
                id: source.channel_id,
                event: event as u8,
                endpoint: source.endpoint_id,
                toward_ipa: matches!(source.channel_id, 0 | 1),
                command: source.channel_id == 1,
                tlvs: source.tlv_count as u16,
                tre_iova: tre.iova(),
                tre_bytes: tre.requested() as u32,
                event_iova: event_buffer.iova(),
                event_bytes: event_buffer.requested() as u32,
            });
        }
        if result.iter().any(Option::is_none) {
            return Err(Error::Layout);
        }
        Ok(Self {
            channels: result.map(Option::unwrap),
            limits,
        })
    }
    /// Ascending hardware channel order, deliberately different from the
    /// endpoint/event allocation order. Retain the actual pool separately.
    pub const fn channels(&self) -> &[Channel; 4] {
        &self.channels
    }
    pub const fn limits(&self) -> Limits {
        self.limits
    }
    pub fn event_words(&self, index: usize) -> Result<[Word; 11], Error> {
        let ch = self.channels.get(index).ok_or(Error::Layout)?;
        let base = 0x1d000 + usize::from(ch.event) * 0x80;
        let mut words = [Word::new(0, 0); 11];
        words[0] = Word {
            offset: base,
            value: 0x10010002,
            readback_mask: 0xff01000f,
        };
        words[1] = Word {
            offset: base + 4,
            value: ch.event_bytes,
            readback_mask: 0xffff,
        };
        words[2] = Word::new(base + 8, ch.event_iova as u32);
        words[3] = Word::new(base + 12, (ch.event_iova >> 32) as u32);
        words[4] = Word {
            offset: base + 0x20,
            value: 0x10020,
            readback_mask: 0x00ffffff,
        };
        for (slot, offset) in (0x24..=0x34).step_by(4).enumerate() {
            words[slot + 5] = Word::new(base + offset, 0);
        }
        // index=0 => last processed event is (count-1)*16, not base-16.
        words[10] = Word::new(
            0x1e100 + usize::from(ch.event) * 8,
            (ch.event_iova + u64::from(ch.event_bytes) - 16) as u32,
        );
        Ok(words)
    }
    pub fn channel_words(&self, index: usize, scratch3: u32) -> Result<[Word; 9], Error> {
        let ch = self.channels.get(index).ok_or(Error::Layout)?;
        let base = 0x1c000 + usize::from(ch.id) * 0x80;
        Ok([
            Word {
                offset: base,
                value: 0x10000002 | ((ch.toward_ipa as u32) << 3) | (u32::from(ch.event) << 14),
                readback_mask: 0xff07c00f,
            },
            Word {
                offset: base + 4,
                value: ch.tre_bytes,
                readback_mask: 0xffff,
            },
            Word::new(base + 8, ch.tre_iova as u32),
            Word::new(base + 12, (ch.tre_iova >> 32) as u32),
            Word::new(base + 0x5c, if ch.command { 15 } else { 1 << 10 }),
            Word::new(base + 0x60, 0),
            Word::new(base + 0x64, 0),
            Word::new(base + 0x68, u32::from(ch.tlvs) * 16 << 16),
            // Follow the *actual expression*: it preserves low 16 bits. The
            // nearby C comment incorrectly says upper; do not reverse it.
            Word::new(base + 0x6c, (32 << 16) | (scratch3 & 0xffff)),
        ])
    }
}

/// Source order from gsi_irq_setup(); bind the actual IRQ only after these
/// writes and before IRQ_ENABLE. Does not itself bind any capability.
pub const IRQ_CONFIG: [Word; 9] = [
    Word::new(0x1f180, 1),
    Word::new(TYPE_MASK, 0),
    Word::new(CHANNEL_MASK, 0),
    Word::new(EVENT_MASK, 0),
    Word::new(0x1f108, 0),
    Word::new(0x1f0b8, 0),
    Word::new(0xc020, 0),
    Word::new(0xc024, 0),
    Word::new(0x1f120, 0),
];
pub const IRQ_ENABLE: [Word; 4] = [
    Word::new(0x1f108, 1),
    Word::new(TYPE_MASK, 4),
    Word::new(0x1f120, 14),
    Word::new(TYPE_MASK, BASE_IRQ_TYPES),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Allocation {
    Event(u8),
    Channel(u8),
}
impl Allocation {
    fn id(self) -> u8 {
        match self {
            Self::Event(id) | Self::Channel(id) => id,
        }
    }
    fn registers(self) -> (usize, usize, usize, usize, u32) {
        match self {
            Self::Event(id) => (
                EVENT_COMMAND,
                EVENT_MASK,
                EVENT_CLEAR,
                0x1d000 + usize::from(id) * 0x80,
                2,
            ),
            Self::Channel(id) => (
                CHANNEL_COMMAND,
                CHANNEL_MASK,
                CHANNEL_CLEAR,
                0x1c000 + usize::from(id) * 0x80,
                1,
            ),
        }
    }
    fn source(self) -> usize {
        match self {
            Self::Event(_) => EVENT_STATUS,
            Self::Channel(_) => CHANNEL_STATUS,
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
/// Native caller must retain actual prepared rings/domain, successful PAS and
/// powered GSI; only call notified() for a delivered bound GSI IRQ. The backend
/// guards every access and never treats a copied Plan/Report as that authority.
pub trait Io {
    /// Same monotonic tick domain used by notified()/timeout(). The command
    /// deadline begins immediately before its MMIO store, after guards and
    /// context preparation, so slow preparation cannot consume the timeout.
    fn ticks(&mut self) -> u64;
    fn guard(&mut self) -> Result<(), Error>;
    fn read(&mut self, offset: usize) -> Result<u32, Error>;
    fn write(&mut self, request: &Request) -> Result<(), Error>;
    fn acknowledge(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Pending,
    Completed,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub operation: Allocation,
    pub writes_sent: u8,
    pub writes_done: u8,
    pub irq_type: Option<u32>,
    pub irq_sources: Option<u32>,
    pub state: Option<u8>,
    pub ack_sent: bool,
    pub ack_done: bool,
    pub mask_sent: bool,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}
pub struct AllocateOnce {
    report: Report,
    started_at: u64,
    timeout_ticks: u64,
}
impl AllocateOnce {
    pub fn new(operation: Allocation, limits: Limits, timeout_ticks: u64) -> Result<Self, Error> {
        // Only the four pinned AP IDs are in scope. This also bounds every
        // bitmap shift even if the hardware supports additional resources.
        if timeout_ticks == 0
            || timeout_ticks >= 1 << 63
            || operation.id() >= AP_COUNT as u8
            || operation.id()
                >= match operation {
                    Allocation::Event(_) => limits.events,
                    Allocation::Channel(_) => limits.channels,
                }
        {
            return Err(Error::Layout);
        }
        Ok(Self {
            report: Report {
                phase: Phase::New,
                operation,
                writes_sent: 0,
                writes_done: 0,
                irq_type: None,
                irq_sources: None,
                state: None,
                ack_sent: false,
                ack_done: false,
                mask_sent: false,
                error: None,
                containment_error: None,
            },
            started_at: 0,
            timeout_ticks,
        })
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.report.phase == Phase::Pending)
            .then(|| self.started_at.wrapping_add(self.timeout_ticks))
    }
    fn read(&mut self, io: &mut impl Io, offset: usize) -> Result<u32, Error> {
        io.guard()?;
        io.read(offset)
    }
    fn write(&mut self, io: &mut impl Io, offset: usize, value: u32) -> Result<(), Error> {
        io.guard()?;
        if matches!(offset, CHANNEL_COMMAND | EVENT_COMMAND) {
            self.started_at = io.ticks();
        }
        self.report.writes_sent += 1;
        io.write(&Request {
            word: Word::new(offset, value),
        })?;
        self.report.writes_done += 1;
        Ok(())
    }
    fn fail(&mut self, io: &mut impl Io, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error = Some(error);
        self.report.mask_sent = true;
        // Containment is not replay: mask once without clearing/ACKing pending
        // evidence, even if the normal power/domain guard has failed.
        self.report.containment_error = io
            .write(&Request {
                word: Word::new(TYPE_MASK, 0),
            })
            .err();
        error
    }
    pub fn begin(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Pending;
        let result = (|| {
            let (command, mask, clear, context, kind) = self.report.operation.registers();
            if self.read(io, TYPE_MASK)? != BASE_IRQ_TYPES {
                return Err(Error::Readback);
            }
            let pending = self.read(io, TYPE_STATUS)?;
            self.report.irq_type = Some(pending);
            if pending != 0 {
                return Err(Error::Pending);
            }
            if self.read(io, CHANNEL_MASK)? != 0 || self.read(io, EVENT_MASK)? != 0 {
                return Err(Error::Pending);
            }
            let state = ((self.read(io, context)? >> 20) & 15) as u8;
            self.report.state = Some(state);
            if state != 0 {
                return Err(Error::State);
            }
            if self.read(io, self.report.operation.source())? != 0 {
                return Err(Error::Pending);
            }
            self.write(io, clear, u32::MAX)?;
            self.write(io, mask, 1 << self.report.operation.id())?;
            self.write(io, TYPE_MASK, BASE_IRQ_TYPES | kind)?;
            if self.read(io, mask)? != (1 << self.report.operation.id())
                || self.read(io, TYPE_MASK)? != (BASE_IRQ_TYPES | kind)
            {
                return Err(Error::Readback);
            }
            self.write(io, command, u32::from(self.report.operation.id()))?;
            Ok(())
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
            let (_, mask, clear, context, kind) = self.report.operation.registers();
            let pending = self.read(io, TYPE_STATUS)?;
            self.report.irq_type = Some(pending);
            if pending != kind {
                return Err(Error::UnexpectedIrq);
            }
            let sources = self.read(io, self.report.operation.source())?;
            self.report.irq_sources = Some(sources);
            if sources != (1 << self.report.operation.id()) {
                return Err(Error::UnexpectedIrq);
            }
            self.write(io, clear, sources)?;
            if self.read(io, self.report.operation.source())? != 0 {
                return Err(Error::Pending);
            }
            let state = ((self.read(io, context)? >> 20) & 15) as u8;
            self.report.state = Some(state);
            if state != 1 {
                return Err(Error::State);
            }
            self.write(io, TYPE_MASK, BASE_IRQ_TYPES)?;
            self.write(io, mask, 0)?;
            if self.read(io, TYPE_MASK)? != BASE_IRQ_TYPES || self.read(io, mask)? != 0 {
                return Err(Error::Readback);
            }
            if self.read(io, TYPE_STATUS)? != 0 {
                return Err(Error::Pending);
            }
            io.guard()?;
            self.report.ack_sent = true;
            io.acknowledge()?;
            self.report.ack_done = true;
            self.report.phase = Phase::Completed;
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
    /// A co-delivered SMMU/IPA fault takes priority. Keep every hardware
    /// allocation and pending source retained, with no clear or IRQ ACK.
    pub fn external_fault(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if matches!(self.report.phase, Phase::Completed | Phase::Failed) {
            return Err(Error::Consumed);
        }
        Err(self.fail(io, Error::Guard))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModemReport {
    pub phase: Phase,
    pub channel: u8,
    pub writes_sent: u8,
    pub writes_done: u8,
    pub irq_type: Option<u32>,
    pub global_status: Option<u32>,
    pub result: Option<u8>,
    pub ack_sent: bool,
    pub ack_done: bool,
    pub mask_sent: bool,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}
/// IPA v4.2 workaround: allocate a modem-EE GSI resource, without booting
/// MPSS. No AP context-bank/register view can query that modem channel state.
pub struct ModemAllocateOnce {
    report: ModemReport,
    started_at: u64,
    timeout_ticks: u64,
}
impl ModemAllocateOnce {
    pub fn new(channel: u8, limits: Limits, timeout_ticks: u64) -> Result<Self, Error> {
        if channel >= 4
            || channel >= limits.channels
            || timeout_ticks == 0
            || timeout_ticks >= 1 << 63
        {
            return Err(Error::Layout);
        }
        Ok(Self {
            report: ModemReport {
                phase: Phase::New,
                channel,
                writes_sent: 0,
                writes_done: 0,
                irq_type: None,
                global_status: None,
                result: None,
                ack_sent: false,
                ack_done: false,
                mask_sent: false,
                error: None,
                containment_error: None,
            },
            started_at: 0,
            timeout_ticks,
        })
    }
    pub const fn report(&self) -> ModemReport {
        self.report
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.report.phase == Phase::Pending)
            .then(|| self.started_at.wrapping_add(self.timeout_ticks))
    }
    fn read(&mut self, io: &mut impl Io, offset: usize) -> Result<u32, Error> {
        io.guard()?;
        io.read(offset)
    }
    fn write(&mut self, io: &mut impl Io, offset: usize, value: u32) -> Result<(), Error> {
        io.guard()?;
        if offset == GENERIC_COMMAND {
            self.started_at = io.ticks();
        }
        self.report.writes_sent += 1;
        io.write(&Request {
            word: Word::new(offset, value),
        })?;
        self.report.writes_done += 1;
        Ok(())
    }
    fn fail(&mut self, io: &mut impl Io, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error = Some(error);
        self.report.mask_sent = true;
        self.report.containment_error = io
            .write(&Request {
                word: Word::new(TYPE_MASK, 0),
            })
            .err();
        error
    }
    pub fn begin(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Pending;
        let result = (|| {
            if self.read(io, TYPE_MASK)? != BASE_IRQ_TYPES || self.read(io, GLOBAL_ENABLE)? != 1 {
                return Err(Error::Readback);
            }
            if self.read(io, TYPE_STATUS)? != 0
                || self.read(io, GLOBAL_STATUS)? != 0
                || self.read(io, GENERAL_STATUS)? != 0
                || self.read(io, CHANNEL_MASK)? != 0
                || self.read(io, EVENT_MASK)? != 0
            {
                return Err(Error::Pending);
            }
            self.write(io, GLOBAL_ENABLE, 3)?;
            let scratch = self.read(io, GENERIC_SCRATCH)? & !GENERIC_RESULT_MASK;
            self.write(io, GENERIC_SCRATCH, scratch)?;
            if self.read(io, GLOBAL_ENABLE)? != 3 || self.read(io, GENERIC_SCRATCH)? != scratch {
                return Err(Error::Readback);
            }
            self.write(
                io,
                GENERIC_COMMAND,
                2 | (u32::from(self.report.channel) << 5) | (1 << 10),
            )?;
            Ok(())
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
            let pending = self.read(io, TYPE_STATUS)?;
            self.report.irq_type = Some(pending);
            if pending != 4 {
                return Err(Error::UnexpectedIrq);
            }
            let global = self.read(io, GLOBAL_STATUS)?;
            self.report.global_status = Some(global);
            if global != 2 {
                return Err(Error::UnexpectedIrq);
            }
            // Source ISR clears captured GP_INT1 before reading its result.
            self.write(io, GLOBAL_CLEAR, global)?;
            if self.read(io, GLOBAL_STATUS)? != 0 {
                return Err(Error::Pending);
            }
            let result = ((self.read(io, GENERIC_SCRATCH)? & GENERIC_RESULT_MASK) >> 5) as u8;
            self.report.result = Some(result);
            // Match Linux's generic handler, which accepts both values.
            // Result 2 is retained as compatible existing state, never as
            // proof of a fresh allocation. Result 6 is not retried.
            if !matches!(result, 1 | 2) {
                return Err(Error::CommandResult(result));
            }
            self.write(io, GLOBAL_ENABLE, 1)?;
            if self.read(io, GLOBAL_ENABLE)? != 1 {
                return Err(Error::Readback);
            }
            if self.read(io, TYPE_STATUS)? != 0 {
                return Err(Error::Pending);
            }
            io.guard()?;
            self.report.ack_sent = true;
            io.acknowledge()?;
            self.report.ack_done = true;
            self.report.phase = Phase::Completed;
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
        if matches!(self.report.phase, Phase::Completed | Phase::Failed) {
            return Err(Error::Consumed);
        }
        Err(self.fail(io, Error::Guard))
    }
}

/// Bind and arm the actual level-high IRQ 464 while all GSI masks are zero.
/// A successful model binding is not evidence of physical IRQ delivery.
pub trait SetupIo: Io {
    fn bind(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupPhase {
    New,
    Configuring,
    EventPending,
    ChannelPending,
    ModemPending,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupReport {
    pub phase: SetupPhase,
    pub index: u8,
    pub irq_bind_sent: bool,
    pub irq_bound: bool,
    /// Raw diagnostic samples, not interrupt status or a readiness permit.
    pub error_log_before: Option<u32>,
    pub error_log_after: Option<u32>,
    pub events_allocated: u8,
    pub events_programmed: u8,
    pub channels_allocated: u8,
    pub channels_programmed: u8,
    pub modem_accepted: u8,
    pub modem_fresh: u8,
    pub modem_existing: u8,
    pub writes_sent: u16,
    pub writes_done: u16,
    pub last_word: Option<Word>,
    pub allocation: Option<Report>,
    pub modem: Option<ModemReport>,
    pub mask_sent: bool,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}
/// Retained one-shot setup through allocation and context programming only.
/// Completion is not a production DMA permit; no channel START, TRE publish,
/// host transfer doorbell, modem boot or teardown command exists here.
pub struct SetupOnce {
    plan: Plan,
    report: SetupReport,
    allocation: Option<AllocateOnce>,
    modem: Option<ModemAllocateOnce>,
    timeout_ticks: u64,
}
impl SetupOnce {
    pub fn new(plan: Plan, timeout_ticks: u64) -> Result<Self, Error> {
        if timeout_ticks == 0 || timeout_ticks >= 1 << 63 {
            return Err(Error::Layout);
        }
        Ok(Self {
            plan,
            timeout_ticks,
            allocation: None,
            modem: None,
            report: SetupReport {
                phase: SetupPhase::New,
                index: 0,
                irq_bind_sent: false,
                irq_bound: false,
                error_log_before: None,
                error_log_after: None,
                events_allocated: 0,
                events_programmed: 0,
                channels_allocated: 0,
                channels_programmed: 0,
                modem_accepted: 0,
                modem_fresh: 0,
                modem_existing: 0,
                writes_sent: 0,
                writes_done: 0,
                last_word: None,
                allocation: None,
                modem: None,
                mask_sent: false,
                error: None,
                containment_error: None,
            },
        })
    }
    pub const fn report(&self) -> SetupReport {
        self.report
    }
    /// Diagnostic geometry; the native backend must still retain and guard
    /// the actual pool/domain/firmware owners on every hardware access.
    pub const fn plan(&self) -> Plan {
        self.plan
    }
    pub fn deadline(&self) -> Option<u64> {
        match self.report.phase {
            SetupPhase::EventPending | SetupPhase::ChannelPending => {
                self.allocation.as_ref()?.deadline()
            }
            SetupPhase::ModemPending => self.modem.as_ref()?.deadline(),
            _ => None,
        }
    }
    fn read(&mut self, io: &mut impl Io, offset: usize) -> Result<u32, Error> {
        io.guard()?;
        io.read(offset)
    }
    fn write(&mut self, io: &mut impl Io, word: Word) -> Result<(), Error> {
        io.guard()?;
        self.report.last_word = Some(word);
        self.report.writes_sent += 1;
        io.write(&Request { word })?;
        self.report.writes_done += 1;
        Ok(())
    }
    fn fail(&mut self, io: &mut impl Io, error: Error) -> Error {
        self.report.phase = SetupPhase::Failed;
        self.report.error = Some(error);
        self.report.mask_sent = true;
        self.report.containment_error = io
            .write(&Request {
                word: Word::new(TYPE_MASK, 0),
            })
            .err();
        error
    }
    fn adopt_allocation(&mut self, error: Error) -> Error {
        let r = self.allocation.as_ref().unwrap().report();
        self.report.allocation = Some(r);
        self.report.phase = SetupPhase::Failed;
        self.report.error = Some(error);
        self.report.mask_sent = r.mask_sent;
        self.report.containment_error = r.containment_error;
        error
    }
    fn adopt_modem(&mut self, error: Error) -> Error {
        let r = self.modem.as_ref().unwrap().report();
        self.report.modem = Some(r);
        self.report.phase = SetupPhase::Failed;
        self.report.error = Some(error);
        self.report.mask_sent = r.mask_sent;
        self.report.containment_error = r.containment_error;
        error
    }
    fn begin_ap(&mut self, io: &mut impl Io, event: bool) -> Result<(), Error> {
        let ch = self.plan.channels[usize::from(self.report.index)];
        let op = if event {
            Allocation::Event(ch.event)
        } else {
            Allocation::Channel(ch.id)
        };
        self.allocation = Some(AllocateOnce::new(op, self.plan.limits, self.timeout_ticks)?);
        self.report.phase = if event {
            SetupPhase::EventPending
        } else {
            SetupPhase::ChannelPending
        };
        let result = self.allocation.as_mut().unwrap().begin(io);
        self.report.allocation = Some(self.allocation.as_ref().unwrap().report());
        result.map_err(|e| self.adopt_allocation(e))
    }
    fn begin_modem(&mut self, io: &mut impl Io) -> Result<(), Error> {
        self.modem = Some(ModemAllocateOnce::new(
            self.report.index,
            self.plan.limits,
            self.timeout_ticks,
        )?);
        self.report.phase = SetupPhase::ModemPending;
        let result = self.modem.as_mut().unwrap().begin(io);
        self.report.modem = Some(self.modem.as_ref().unwrap().report());
        result.map_err(|e| self.adopt_modem(e))
    }
    fn require_quiet(&mut self, io: &mut impl Io) -> Result<(), Error> {
        // These are the actual pending status sources, including raw sources
        // while their interrupt masks are disabled. ERROR_LOG alone is not
        // an interrupt indication and Linux initializes it during gsi_setup().
        for offset in [
            TYPE_STATUS,
            GLOBAL_STATUS,
            GENERAL_STATUS,
            CHANNEL_STATUS,
            EVENT_STATUS,
        ] {
            if self.read(io, offset)? != 0 {
                return Err(Error::Pending);
            }
        }
        Ok(())
    }
    pub fn start(&mut self, io: &mut impl SetupIo) -> Result<(), Error> {
        if self.report.phase != SetupPhase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = SetupPhase::Configuring;
        let result = (|| {
            if self.read(io, GSI_STATUS)? != 1 {
                return Err(Error::State);
            }
            self.require_quiet(io)?;
            for word in IRQ_CONFIG {
                self.write(io, word)?;
                if self.read(io, word.offset)? != word.value {
                    return Err(Error::Readback);
                }
            }
            io.guard()?;
            self.report.irq_bind_sent = true;
            io.bind()?;
            self.report.irq_bound = true;
            if Limits::decode(self.read(io, HW_PARAM_2)?)? != self.plan.limits {
                return Err(Error::HardwareLimits);
            }
            // Match pinned gsi_setup(): initialize the log, without assuming
            // its pre-initialization contents must be zero. Retain both raw
            // samples and refuse to clear a log with an actual pending source.
            self.report.error_log_before = Some(self.read(io, ERROR_LOG)?);
            self.require_quiet(io)?;
            self.write(io, Word::new(ERROR_LOG, 0))?;
            self.report.error_log_after = Some(self.read(io, ERROR_LOG)?);
            self.require_quiet(io)?;
            for word in IRQ_ENABLE {
                self.write(io, word)?;
                if self.read(io, word.offset)? != word.value {
                    return Err(Error::Readback);
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            return Err(self.fail(io, e));
        }
        self.begin_ap(io, true)
    }
    fn program(&mut self, io: &mut impl Io, words: &[Word], context: usize) -> Result<(), Error> {
        if ((self.read(io, context)? >> 20) & 15) != 1 {
            return Err(Error::State);
        }
        for &word in words {
            self.write(io, word)?;
        }
        for _ in 0..2 {
            for word in words {
                if word.readback_mask != 0
                    && self.read(io, word.offset)? & word.readback_mask
                        != word.value & word.readback_mask
                {
                    return Err(Error::Readback);
                }
            }
        }
        if ((self.read(io, context)? >> 20) & 15) != 1 {
            return Err(Error::State);
        }
        Ok(())
    }
    pub fn notified(&mut self, io: &mut impl Io, now: u64) -> Result<(), Error> {
        match self.report.phase {
            SetupPhase::EventPending | SetupPhase::ChannelPending => {
                let event = self.report.phase == SetupPhase::EventPending;
                let result = self.allocation.as_mut().unwrap().notified(io, now);
                self.report.allocation = Some(self.allocation.as_ref().unwrap().report());
                if let Err(e) = result {
                    return Err(self.adopt_allocation(e));
                }
                let index = usize::from(self.report.index);
                let ch = self.plan.channels[index];
                if event {
                    self.report.events_allocated |= 1 << ch.event;
                    let words = self.plan.event_words(index)?;
                    if let Err(e) =
                        self.program(io, &words[..10], 0x1d000 + usize::from(ch.event) * 0x80)
                    {
                        return Err(self.fail(io, e));
                    }
                    // Event doorbell announces the last consumed empty slot,
                    // not a TRE transfer. It has no readback guarantee.
                    if let Err(e) = self.write(io, words[10]) {
                        return Err(self.fail(io, e));
                    }
                    self.report.events_programmed |= 1 << ch.event;
                    self.begin_ap(io, false)
                } else {
                    self.report.channels_allocated |= 1 << ch.id;
                    let base = 0x1c000 + usize::from(ch.id) * 0x80;
                    let scratch = match self.read(io, base + 0x6c) {
                        Ok(v) => v,
                        Err(e) => return Err(self.fail(io, e)),
                    };
                    let words = self.plan.channel_words(index, scratch)?;
                    if let Err(e) = self.program(io, &words, base) {
                        return Err(self.fail(io, e));
                    }
                    self.report.channels_programmed |= 1 << ch.id;
                    self.report.index += 1;
                    if self.report.index < 4 {
                        self.begin_ap(io, true)
                    } else {
                        self.report.index = 0;
                        self.begin_modem(io)
                    }
                }
            }
            SetupPhase::ModemPending => {
                let result = self.modem.as_mut().unwrap().notified(io, now);
                let r = self.modem.as_ref().unwrap().report();
                self.report.modem = Some(r);
                if let Err(e) = result {
                    return Err(self.adopt_modem(e));
                }
                let bit = 1 << self.report.index;
                self.report.modem_accepted |= bit;
                if r.result == Some(1) {
                    self.report.modem_fresh |= bit;
                } else {
                    self.report.modem_existing |= bit;
                }
                self.report.index += 1;
                if self.report.index < 4 {
                    self.begin_modem(io)
                } else {
                    self.report.phase = SetupPhase::Complete;
                    Ok(())
                }
            }
            _ => Err(Error::Consumed),
        }
    }
    pub fn timeout(&mut self, io: &mut impl Io, now: u64) -> Result<(), Error> {
        match self.report.phase {
            SetupPhase::EventPending | SetupPhase::ChannelPending => {
                let result = self.allocation.as_mut().unwrap().timeout(io, now);
                self.report.allocation = Some(self.allocation.as_ref().unwrap().report());
                result.map_err(|e| self.adopt_allocation(e))
            }
            SetupPhase::ModemPending => {
                let result = self.modem.as_mut().unwrap().timeout(io, now);
                self.report.modem = Some(self.modem.as_ref().unwrap().report());
                result.map_err(|e| self.adopt_modem(e))
            }
            _ => Err(Error::Consumed),
        }
    }
    pub fn external_fault(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if matches!(self.report.phase, SetupPhase::Complete | SetupPhase::Failed) {
            return Err(Error::Consumed);
        }
        Err(self.fail(io, Error::Guard))
    }
    /// No further GSI notification is expected once this allocation-only
    /// setup completes. Contain a late delivered IRQ once, preserving it and
    /// all resources without clearing/ACKing it or issuing another command.
    pub fn late_notification(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if self.report.phase != SetupPhase::Complete {
            return Err(Error::Consumed);
        }
        Err(self.fail(io, Error::UnexpectedIrq))
    }
}
