//! Retained CPU preparation of coherent-memory inputs for Linux IPA v4.2.
//!
//! This owns its memory backend for the boot, with no free/reuse path. It
//! prepares the four AP TRE/event ring pairs, command payload pool, DMA-zero
//! source and empty rule table inside the installed private 128-KiB window.
//! It does not allocate GSI hardware IDs, publish descriptors, start DMA,
//! perform IMEM/SMEM mappings or construct production IPA/DMA readiness.
#![forbid(unsafe_code)]

pub const POOL_IOVA: u64 = 0x80000000;
pub const POOL_BYTES: usize = 0x20000;
pub const PAGE_BYTES: usize = 4096;
pub const RING_ELEMENT_BYTES: usize = 16;
pub const ZERO_BYTES: usize = 0x2000;
pub const COMMAND_ELEMENT_BYTES: usize = 32;
pub const COMMAND_POOL_BYTES: usize = 0x2000;
pub const TABLE_ENTRIES: usize = 15;
pub const FILTER_BITMAP: u64 = 0x24;
pub const TABLE_BYTES: usize = 8 + (1 + TABLE_ENTRIES) * 8;
pub const BUFFER_COUNT: usize = 11;
pub const COMPLETE: u16 = (1 << BUFFER_COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel {
    pub name: &'static str,
    pub channel_id: u8,
    pub endpoint_id: u8,
    pub tre_count: usize,
    pub event_count: usize,
    pub tlv_count: usize,
}
pub const CHANNELS: [Channel; 4] = [
    Channel {
        name: "AP_COMMAND_TX",
        channel_id: 1,
        endpoint_id: 6,
        tre_count: 256,
        event_count: 256,
        tlv_count: 20,
    },
    Channel {
        name: "AP_LAN_RX",
        channel_id: 2,
        endpoint_id: 8,
        tre_count: 256,
        event_count: 256,
        tlv_count: 6,
    },
    Channel {
        name: "AP_MODEM_TX",
        channel_id: 0,
        endpoint_id: 1,
        tre_count: 512,
        event_count: 512,
        tlv_count: 8,
    },
    Channel {
        name: "AP_MODEM_RX",
        channel_id: 3,
        endpoint_id: 9,
        tre_count: 256,
        event_count: 256,
        tlv_count: 6,
    },
];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Zero,
    Table,
    Event(u8),
    Tre(u8),
    CommandPayload,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Buffer {
    kind: Kind,
    offset: usize,
    requested: usize,
    reserved: usize,
    iova: u64,
}
impl Buffer {
    pub const fn kind(self) -> Kind {
        self.kind
    }
    pub const fn offset(self) -> usize {
        self.offset
    }
    pub const fn requested(self) -> usize {
        self.requested
    }
    pub const fn reserved(self) -> usize {
        self.reserved
    }
    pub const fn iova(self) -> u64 {
        self.iova
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    Layout,
    Bounds,
    Io,
}
pub trait Memory {
    fn bytes(&self) -> usize;
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError>;
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyStarted,
    Quarantined,
    Layout,
    Domain,
    Config,
    Memory(MemoryError),
    Readback,
}
impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::AlreadyStarted => 1,
            Self::Quarantined => 2,
            Self::Layout => 3,
            Self::Domain => 4,
            Self::Config => 5,
            Self::Memory(_) => 6,
            Self::Readback => 7,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    buffers: [Buffer; BUFFER_COUNT],
    used: usize,
    reserved: usize,
}
impl Plan {
    pub fn new() -> Result<Self, Error> {
        let mut plan = Self {
            buffers: [Buffer {
                kind: Kind::Zero,
                offset: 0,
                requested: 0,
                reserved: 0,
                iova: 0,
            }; BUFFER_COUNT],
            used: 0,
            reserved: 0,
        };
        let mut count = 0;
        let mut push = |kind, requested: usize, reserved: usize| -> Result<(), Error> {
            if reserved < PAGE_BYTES
                || !reserved.is_power_of_two()
                || requested > reserved
                || count >= BUFFER_COUNT
            {
                return Err(Error::Layout);
            }
            let offset =
                plan.used.checked_add(reserved - 1).ok_or(Error::Layout)? & !(reserved - 1);
            let end = offset.checked_add(reserved).ok_or(Error::Layout)?;
            if end > POOL_BYTES {
                return Err(Error::Layout);
            }
            let iova = POOL_IOVA.checked_add(offset as u64).ok_or(Error::Layout)?;
            if iova & (reserved as u64 - 1) != 0 {
                return Err(Error::Layout);
            }
            plan.buffers[count] = Buffer {
                kind,
                offset,
                requested,
                reserved,
                iova,
            };
            plan.used = end;
            plan.reserved += reserved;
            count += 1;
            Ok(())
        };
        push(Kind::Zero, ZERO_BYTES, ZERO_BYTES)?;
        push(Kind::Table, TABLE_BYTES, PAGE_BYTES)?;
        for channel in CHANNELS {
            let events = channel.event_count * RING_ELEMENT_BYTES;
            let tres = channel.tre_count * RING_ELEMENT_BYTES;
            push(Kind::Event(channel.channel_id), events, events)?;
            push(Kind::Tre(channel.channel_id), tres, tres)?;
            if channel.channel_id == 1 {
                let tre_max = channel.tre_count - (channel.tlv_count - 1);
                let payload_bytes = (tre_max + channel.tlv_count - 1) * COMMAND_ELEMENT_BYTES;
                if payload_bytes != COMMAND_POOL_BYTES {
                    return Err(Error::Layout);
                }
                push(Kind::CommandPayload, payload_bytes, payload_bytes)?;
            }
        }
        if count != BUFFER_COUNT {
            return Err(Error::Layout);
        }
        Ok(plan)
    }
    pub const fn buffers(&self) -> &[Buffer; BUFFER_COUNT] {
        &self.buffers
    }
    pub const fn used_bytes(&self) -> usize {
        self.used
    }
    pub const fn reserved_bytes(&self) -> usize {
        self.reserved
    }
    pub const fn remaining_bytes(&self) -> usize {
        POOL_BYTES - self.used
    }
    pub fn find(&self, kind: Kind) -> Option<Buffer> {
        self.buffers.iter().find(|b| b.kind == kind).copied()
    }
    fn expected_word(&self, buffer: Buffer, local_offset: usize) -> u32 {
        if buffer.kind != Kind::Table || local_offset >= TABLE_BYTES {
            return 0;
        }
        let slot = local_offset / 8;
        let value = match slot {
            0 => 0,
            1 => FILTER_BITMAP,
            _ => buffer.iova,
        };
        (value >> ((local_offset % 8) * 8)) as u32
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Initialize,
    VerifyFirst,
    VerifySecond,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub submitted: u16,
    pub initialized: u16,
    pub verified: u16,
    pub failed_buffer: Option<usize>,
    pub failed_offset: Option<usize>,
    pub expected_word: u32,
    pub actual_word: u32,
    pub checked_bytes: usize,
    pub error: Option<Error>,
}
/// The concrete native instance retains its sole RAM backend. Public buffer
/// metadata is diagnostic information, not an authorization to start hardware.
pub struct Pool<M: Memory> {
    memory: M,
    plan: Plan,
    started: bool,
    prepared: bool,
    quarantined: bool,
    memory_setup_claimed: bool,
    memory_setup_complete: bool,
    table_setup_claimed: bool,
    table_setup_complete: bool,
    receive_claimed: bool,
}
impl<M: Memory> Pool<M> {
    pub fn new(memory: M) -> Result<Self, Error> {
        if memory.bytes() != POOL_BYTES {
            return Err(Error::Layout);
        }
        Ok(Self {
            memory,
            plan: Plan::new()?,
            started: false,
            prepared: false,
            quarantined: false,
            memory_setup_claimed: false,
            memory_setup_complete: false,
            table_setup_claimed: false,
            table_setup_complete: false,
            receive_claimed: false,
        })
    }
    pub const fn plan(&self) -> &Plan {
        &self.plan
    }
    pub fn buffer(&self, kind: Kind) -> Option<Buffer> {
        if self.prepared && !self.quarantined {
            self.plan.find(kind)
        } else {
            None
        }
    }
    pub fn quarantine(&mut self) {
        self.quarantined = true;
        self.prepared = false;
    }
    // Only the typed memory-setup coordinator can use these crate-private
    // operations. It retains the original pool, prevents a second claim,
    // and never exposes the raw backend or a hardware-ready token.
    #[allow(dead_code)] // Standalone buffer/GSI suites omit the later coordinator.
    pub(crate) fn claim_memory_setup(&mut self) -> Result<(), MemoryError> {
        if !self.prepared || self.quarantined || self.memory_setup_claimed {
            return Err(MemoryError::Layout);
        }
        self.memory_setup_claimed = true;
        Ok(())
    }
    #[allow(dead_code)]
    pub(crate) fn memory_setup_read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        if !self.memory_setup_claimed || self.quarantined || offset % 4 != 0 || offset >= POOL_BYTES
        {
            return Err(MemoryError::Bounds);
        }
        self.memory.read32(offset)
    }
    #[allow(dead_code)]
    pub(crate) fn memory_setup_write32(
        &mut self,
        offset: usize,
        value: u32,
    ) -> Result<(), MemoryError> {
        // The first transaction can stage only its four 32-byte payloads
        // and four 16-byte TREs. Event/zero/table/other-channel buffers have
        // no CPU write path here, even if a coordinator supplies a bad offset.
        let payload = self
            .plan
            .find(Kind::CommandPayload)
            .ok_or(MemoryError::Layout)?;
        let tre = self.plan.find(Kind::Tre(1)).ok_or(MemoryError::Layout)?;
        let permitted = (payload.offset..payload.offset + 4 * COMMAND_ELEMENT_BYTES)
            .contains(&offset)
            || (tre.offset..tre.offset + 4 * RING_ELEMENT_BYTES).contains(&offset);
        if !self.memory_setup_claimed
            || self.memory_setup_complete
            || self.quarantined
            || offset % 4 != 0
            || !permitted
        {
            return Err(MemoryError::Bounds);
        }
        self.memory.write32(offset, value)
    }
    #[allow(dead_code)]
    pub(crate) fn finish_memory_setup(&mut self) -> Result<(), MemoryError> {
        if !self.memory_setup_claimed
            || self.memory_setup_complete
            || self.quarantined
            || !self.prepared
        {
            return Err(MemoryError::Layout);
        }
        self.memory_setup_complete = true;
        Ok(())
    }
    #[allow(dead_code)] // Earlier standalone stages intentionally omit DMA.
    pub const fn memory_setup_complete(&self) -> bool {
        self.memory_setup_complete && !self.quarantined
    }
    #[allow(dead_code)]
    pub(crate) fn claim_table_setup(&mut self) -> Result<(), MemoryError> {
        if !self.memory_setup_complete() || self.table_setup_claimed || !self.prepared {
            return Err(MemoryError::Layout);
        }
        self.table_setup_claimed = true;
        Ok(())
    }
    #[allow(dead_code)]
    pub(crate) fn table_setup_write32(
        &mut self,
        offset: usize,
        value: u32,
    ) -> Result<(), MemoryError> {
        let payload = self
            .plan
            .find(Kind::CommandPayload)
            .ok_or(MemoryError::Layout)?;
        let tre = self.plan.find(Kind::Tre(1)).ok_or(MemoryError::Layout)?;
        // Fresh slots 4..10. The prior memory transaction, zero source,
        // table source, event ring and other channels cannot be overwritten.
        let permitted = (payload.offset + 4 * COMMAND_ELEMENT_BYTES
            ..payload.offset + 10 * COMMAND_ELEMENT_BYTES)
            .contains(&offset)
            || (tre.offset + 4 * RING_ELEMENT_BYTES..tre.offset + 10 * RING_ELEMENT_BYTES)
                .contains(&offset);
        if !self.memory_setup_complete()
            || !self.table_setup_claimed
            || self.table_setup_complete
            || offset % 4 != 0
            || !permitted
        {
            return Err(MemoryError::Bounds);
        }
        self.memory.write32(offset, value)
    }
    #[allow(dead_code)]
    pub(crate) fn finish_table_setup(&mut self) -> Result<(), MemoryError> {
        if !self.memory_setup_complete()
            || !self.table_setup_claimed
            || self.table_setup_complete
            || !self.prepared
        {
            return Err(MemoryError::Layout);
        }
        self.table_setup_complete = true;
        Ok(())
    }
    #[allow(dead_code)]
    pub const fn table_setup_complete(&self) -> bool {
        self.table_setup_complete && self.memory_setup_complete()
    }
    #[allow(dead_code)] // Earlier standalone stages omit the receive owner.
    pub(crate) fn claim_receive(&mut self) -> Result<(), MemoryError> {
        if !self.table_setup_complete() || !self.prepared || self.receive_claimed {
            return Err(MemoryError::Layout);
        }
        self.receive_claimed = true;
        Ok(())
    }
    /// Sole retained backend, never a second CPU alias. The receive
    /// coordinator owns which FREE slot it can stage before publication.
    /// Even a bad internal offset cannot reach an event, command or source.
    #[allow(dead_code)]
    pub(crate) fn receive_write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        let tre = self.plan.find(Kind::Tre(2)).ok_or(MemoryError::Layout)?;
        if !self.table_setup_complete()
            || !self.receive_claimed
            || !self.prepared
            || offset % 4 != 0
            || !(tre.offset..tre.offset + tre.requested).contains(&offset)
        {
            return Err(MemoryError::Bounds);
        }
        let result = self.memory.write32(offset, value);
        if result.is_err() {
            self.quarantine();
        }
        result
    }
    /// Runtime reads are confined to channel 2's own event/TRE rings.
    #[allow(dead_code)]
    pub(crate) fn receive_read32(
        &mut self,
        offset: usize,
        event: bool,
    ) -> Result<u32, MemoryError> {
        let base = if event { 0x8000 } else { 0x9000 };
        if !self.table_setup_complete()
            || !self.receive_claimed
            || offset % 4 != 0
            || !(base..base + 0x1000).contains(&offset)
        {
            return Err(MemoryError::Bounds);
        }
        let result = self.memory.read32(offset);
        if result.is_err() {
            self.quarantine();
        }
        result
    }
    pub fn prepare(
        &mut self,
        mut guard: impl FnMut() -> Result<(), Error>,
        mut progress: impl FnMut(Phase, usize, Buffer),
    ) -> Report {
        let mut report = Report {
            phase: Phase::Initialize,
            submitted: 0,
            initialized: 0,
            verified: 0,
            failed_buffer: None,
            failed_offset: None,
            expected_word: 0,
            actual_word: 0,
            checked_bytes: 0,
            error: None,
        };
        if self.quarantined {
            report.error = Some(Error::Quarantined);
            return report;
        }
        if self.started {
            report.error = Some(Error::AlreadyStarted);
            return report;
        }
        self.started = true;
        let result = (|| {
            for (index, buffer) in self.plan.buffers.into_iter().enumerate() {
                report.failed_buffer = Some(index);
                report.failed_offset = None;
                guard()?;
                progress(Phase::Initialize, index, buffer);
                report.submitted |= 1 << index;
                for local in (0..buffer.reserved).step_by(4) {
                    report.failed_offset = Some(buffer.offset + local);
                    let value = self.plan.expected_word(buffer, local);
                    report.expected_word = value;
                    self.memory
                        .write32(buffer.offset + local, value)
                        .map_err(Error::Memory)?;
                }
                report.initialized |= 1 << index;
            }
            for phase in [Phase::VerifyFirst, Phase::VerifySecond] {
                report.phase = phase;
                for (index, buffer) in self.plan.buffers.into_iter().enumerate() {
                    report.failed_buffer = Some(index);
                    report.failed_offset = None;
                    guard()?;
                    progress(phase, index, buffer);
                    for local in (0..buffer.reserved).step_by(4) {
                        report.failed_offset = Some(buffer.offset + local);
                        report.expected_word = self.plan.expected_word(buffer, local);
                        report.actual_word = self
                            .memory
                            .read32(buffer.offset + local)
                            .map_err(Error::Memory)?;
                        if report.actual_word != report.expected_word {
                            return Err(Error::Readback);
                        }
                    }
                    if phase == Phase::VerifySecond {
                        report.verified |= 1 << index;
                        report.checked_bytes += buffer.reserved;
                    }
                }
            }
            report.failed_buffer = None;
            report.failed_offset = None;
            guard()?;
            self.prepared = true;
            Ok(())
        })();
        report.error = result.err();
        if report.error.is_some() {
            self.quarantine();
        }
        report
    }
}
