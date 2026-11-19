//! One-shot executor for source-derived Sunfish IPA AP GSI v2.2 contexts.
//!
//! The plan is produced by `sunfish_ipa_gsi_profile` after checking live GSI
//! capacities, confirmed AP channel/event allocations, dynamic event IDs, and
//! ring geometry. This executor issues one-shot AP channel/event allocations,
//! writes the planned context words, and can start the three AP system pipes
//! after their matching control IRQ/state is confirmed. It does not configure
//! IRQ delivery, map DMA, or establish IPA/WLAN readiness. A platform
//! implementation must provide exclusive access to the exact uncached GSI
//! device aperture.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_gsi_profile::{
    AP_GSI_CHANNEL_ALLOCATE_ORDER, AP_GSI_CONTEXT_WRITE_COUNT, AP_SYSTEM_PIPE_COUNT,
    ApEventRingAllocator, ApGsiChannelAllocationError, ApGsiChannelAllocator, ApGsiChannelCapacity,
    ApGsiChannelIds, ApGsiChannelSnapshot, ApGsiChannelStart, ApGsiContextPlan,
    ApGsiEventRingPrime, EventRingAllocationError, GsiContextPlanError, PendingApEventRingIds,
    PendingApGsiChannelIds, SUNFISH_GSI_MAX_EVENT_RINGS, SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET,
};

/// Sunfish DTS GSI register aperture size (`reg` length 0x2c000).
pub const SUNFISH_GSI_APERTURE_BYTES: u32 = 0x2c000;
pub const SUNFISH_GSI_CONTROL_TYPE_IRQ_OFFSET: u32 = 0x0001_2080;
pub const SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET: u32 = SUNFISH_GSI_CONTROL_TYPE_IRQ_OFFSET;
pub const SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT: u32 = 1 << 1;
pub const SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET: u32 = 0x0001_2094;
pub const SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET: u32 = 0x0001_20a4;
pub const SUNFISH_GSI_EVENT_CONTEXT0_OFFSET: u32 = 0x0001_0000;
pub const SUNFISH_GSI_CONTEXT_STRIDE: u32 = 0x80;
pub const SUNFISH_GSI_EVENT_DOORBELL0_OFFSET: u32 = 0x0001_1100;
pub const SUNFISH_GSI_EVENT_DOORBELL1_OFFSET: u32 = 0x0001_1104;
pub const SUNFISH_GSI_EVENT_DOORBELL_STRIDE: u32 = 8;
pub const SUNFISH_GSI_CHANNEL_COMMAND_OFFSET: u32 = 0x0001_2008;
/// EE0 GSI manager status. The source driver requires bit 0 before it marks
/// the registered GSI device enabled or permits channel/event operations.
pub const SUNFISH_GSI_AP_STATUS_OFFSET: u32 = 0x0001_2000;
pub const SUNFISH_GSI_AP_STATUS_ENABLED_BIT: u32 = 1;
pub const SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT: u32 = 1;
pub const SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET: u32 = 0x0001_2090;
pub const SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET: u32 = 0x0001_20a0;
pub const SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET: u32 = 0x0000_f000;
pub const SUNFISH_GSI_CHANNEL_DOORBELL1_OFFSET: u32 = 0x0001_1004;
pub const SUNFISH_GSI_CHANNEL_DOORBELL_STRIDE: u32 = 8;
const GSI_CHANNEL_START_OPCODE: u32 = 1;
const GSI_EVENT_CONTEXT_STATE_MASK: u32 = 0x00f0_0000;
const GSI_EVENT_CONTEXT_STATE_SHIFT: u32 = 20;
const GSI_CHANNEL_CONTEXT_STATE_MASK: u32 = 0x00f0_0000;
const GSI_CHANNEL_CONTEXT_STATE_SHIFT: u32 = 20;
const GSI_CHANNEL_STATE_STARTED: u8 = 2;
const GSI_CHANNEL_STATE_FLOW_CONTROL: u8 = 5;

/// Narrow register interface for the GSI device aperture.
pub trait RegisterIo {
    type Error;

    /// Perform one ordered 32-bit Device-memory write. An error may be
    /// reported after the write reached hardware; callers must quarantine the
    /// GSI/IPA generation and must not replay the context plan.
    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error>;
}

/// Register access required to consume the GSI event-control interrupt that
/// completes one event-ring allocation. The production adapter must be bound
/// to the exclusive Sunfish GSI aperture and the corresponding GSI IRQ owner.
pub trait EventControlIo {
    type Error;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error>;
    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error>;
}

/// Register access used to confirm AP GSI channel-start completion. The
/// production adapter must be the exclusive owner of the EE0 channel-control
/// IRQ and GSI aperture while this state machine is active.
pub trait ChannelControlIo {
    type Error;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error>;
    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum ExecuteError<E> {
    InvalidRegister {
        index: usize,
        offset: u32,
    },
    Write {
        index: usize,
        offset: u32,
        confirmed_writes: u16,
        source: E,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub struct ExecuteReceipt {
    pub confirmed_writes: u16,
    pending_event_primes: PendingApEventRingPrimes,
    pending_channels: PendingApChannelStarts,
    quarantined: bool,
}

impl ExecuteReceipt {
    pub const fn next_channel_id(&self) -> Option<u8> {
        if self.quarantined || self.pending_event_primes.next_event_id().is_some() {
            None
        } else {
            self.pending_channels.next_channel_id()
        }
    }

    pub const fn all_channels_started(&self) -> bool {
        !self.quarantined
            && self.pending_event_primes.all_primed()
            && self.pending_channels.all_channels_started()
    }

    pub const fn next_event_ring_to_prime(&self) -> Option<u8> {
        if self.quarantined {
            None
        } else {
            self.pending_event_primes.next_event_id()
        }
    }

    fn quarantine(&mut self) {
        self.quarantined = true;
        self.pending_channels.quarantine();
    }
}

#[derive(Debug, Eq, PartialEq)]
struct PendingApEventRingPrimes {
    events: [ApGsiEventRingPrime; AP_SYSTEM_PIPE_COUNT],
    next_index: usize,
}

impl PendingApEventRingPrimes {
    const fn new(events: [ApGsiEventRingPrime; AP_SYSTEM_PIPE_COUNT]) -> Self {
        Self {
            events,
            next_index: 0,
        }
    }

    const fn next_event_id(&self) -> Option<u8> {
        if self.next_index >= AP_SYSTEM_PIPE_COUNT {
            None
        } else {
            Some(self.events[self.next_index].event_id())
        }
    }

    const fn all_primed(&self) -> bool {
        self.next_index >= AP_SYSTEM_PIPE_COUNT
    }
}

#[derive(Debug, Eq, PartialEq)]
struct PendingApChannelStarts {
    channels: [ApGsiChannelStart; AP_SYSTEM_PIPE_COUNT],
    next_index: usize,
    command_in_flight: bool,
    quarantined: bool,
}

impl PendingApChannelStarts {
    const fn new(channels: [ApGsiChannelStart; AP_SYSTEM_PIPE_COUNT]) -> Self {
        Self {
            channels,
            next_index: 0,
            command_in_flight: false,
            quarantined: false,
        }
    }

    const fn next_channel_id(&self) -> Option<u8> {
        if self.quarantined || self.next_index >= AP_SYSTEM_PIPE_COUNT {
            None
        } else {
            Some(self.channels[self.next_index].channel_id())
        }
    }

    const fn all_channels_started(&self) -> bool {
        !self.quarantined && self.next_index >= AP_SYSTEM_PIPE_COUNT
    }

    fn quarantine(&mut self) {
        self.quarantined = true;
        self.command_in_flight = false;
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum EventRingCommandError<E> {
    Allocation(EventRingAllocationError),
    Write {
        event_id: u8,
        source: E,
        quarantine_error: Option<EventRingAllocationError>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRingCommandReceipt {
    /// The command-register write returned success. Hardware allocation is
    /// not complete until the matching IRQ and context state are confirmed.
    pub event_id: u8,
    pub command_word: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum EventControlError<E> {
    Allocation(EventRingAllocationError),
    Read {
        offset: u32,
        source: E,
    },
    UnexpectedType {
        status: u32,
        quarantine_error: Option<EventRingAllocationError>,
    },
    UnexpectedBitmap {
        expected: u32,
        observed: u32,
        quarantine_error: Option<EventRingAllocationError>,
    },
    Acknowledge {
        event_id: u8,
        source: E,
        quarantine_error: Option<EventRingAllocationError>,
    },
    ContextRead {
        event_id: u8,
        source: E,
        quarantine_error: Option<EventRingAllocationError>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub enum EventRingPrimeError<E> {
    Quarantined,
    NoPendingEventRing,
    ChannelStartAlreadyBegan,
    InvalidRegister {
        offset: u32,
    },
    Write {
        event_id: u8,
        offset: u32,
        write_index: u8,
        source: E,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelStartCommandReceipt {
    pub channel_id: u8,
    pub command_word: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelStartCompletion {
    pub channel_id: u8,
    /// Hardware channel state: STARTED (2) or FLOW_CONTROL (5).
    pub state: u8,
    /// The source driver's initial write-pointer MSB written after start.
    pub doorbell_msb: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChannelStartCommandError<E> {
    Quarantined,
    EventRingsNotPrimed,
    CommandAlreadyIssued,
    NoPendingChannel,
    InvalidRegister { offset: u32 },
    Write { channel_id: u8, source: E },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChannelControlError<E> {
    Quarantined,
    CommandNotIssued,
    Read { offset: u32, source: E },
    UnexpectedType { status: u32 },
    UnexpectedBitmap { expected: u32, observed: u32 },
    Acknowledge { channel_id: u8, source: E },
    ContextRead { channel_id: u8, source: E },
    UnexpectedState { channel_id: u8, state: u8 },
    DoorbellWrite { channel_id: u8, source: E },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelAllocationCommandReceipt {
    pub channel_id: u8,
    pub command_word: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChannelAllocationCommandError<E> {
    Allocation(ApGsiChannelAllocationError),
    InvalidRegister {
        offset: u32,
        quarantine_error: Option<ApGsiChannelAllocationError>,
    },
    Write {
        channel_id: u8,
        source: E,
        quarantine_error: Option<ApGsiChannelAllocationError>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ApGsiChannelSnapshotError<E> {
    Read { offset: u32, source: E },
    ManagerNotEnabled { observed: u32 },
    InvalidCapacity(GsiContextPlanError),
    ChannelOutsideCapacity { channel_id: u8, reported_count: u8 },
    Snapshot(ApGsiChannelAllocationError),
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChannelAllocationControlError<E> {
    Allocation(ApGsiChannelAllocationError),
    Read {
        offset: u32,
        source: E,
    },
    UnexpectedType {
        status: u32,
        quarantine_error: Option<ApGsiChannelAllocationError>,
    },
    UnexpectedBitmap {
        expected: u32,
        observed: u32,
        quarantine_error: Option<ApGsiChannelAllocationError>,
    },
    Acknowledge {
        channel_id: u8,
        source: E,
        quarantine_error: Option<ApGsiChannelAllocationError>,
    },
    ContextRead {
        channel_id: u8,
        source: E,
        quarantine_error: Option<ApGsiChannelAllocationError>,
    },
}

/// Read the live EE0 channel count and every AP channel CHSTATE before any
/// allocation command is permitted. The caller must own the GSI aperture and
/// exclude all other GSI users for the duration of this snapshot and the
/// subsequent allocation sequence. The source driver's EE0 GSI status bit
/// must also show that manager initialization has completed.
pub fn read_ap_gsi_channel_snapshot<I: ChannelControlIo>(
    io: &mut I,
) -> Result<ApGsiChannelSnapshot, ApGsiChannelSnapshotError<I::Error>> {
    let hw_param_2 = io
        .read32(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET)
        .map_err(|source| ApGsiChannelSnapshotError::Read {
            offset: SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET,
            source,
        })?;
    let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(hw_param_2)
        .map_err(ApGsiChannelSnapshotError::InvalidCapacity)?;
    if let Some(channel_id) = AP_GSI_CHANNEL_ALLOCATE_ORDER
        .into_iter()
        .find(|channel_id| *channel_id >= capacity.count())
    {
        return Err(ApGsiChannelSnapshotError::ChannelOutsideCapacity {
            channel_id,
            reported_count: capacity.count(),
        });
    }
    let status = io.read32(SUNFISH_GSI_AP_STATUS_OFFSET).map_err(|source| {
        ApGsiChannelSnapshotError::Read {
            offset: SUNFISH_GSI_AP_STATUS_OFFSET,
            source,
        }
    })?;
    if status & SUNFISH_GSI_AP_STATUS_ENABLED_BIT == 0 {
        return Err(ApGsiChannelSnapshotError::ManagerNotEnabled { observed: status });
    }
    let mut states = [0u8; 10];
    for (index, channel_id) in AP_GSI_CHANNEL_ALLOCATE_ORDER.into_iter().enumerate() {
        let offset = SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET
            + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(channel_id);
        let context0 = io
            .read32(offset)
            .map_err(|source| ApGsiChannelSnapshotError::Read { offset, source })?;
        states[index] =
            ((context0 & GSI_CHANNEL_CONTEXT_STATE_MASK) >> GSI_CHANNEL_CONTEXT_STATE_SHIFT) as u8;
    }
    ApGsiChannelSnapshot::from_readback(capacity, states)
        .map_err(ApGsiChannelSnapshotError::Snapshot)
}

/// Issue the next source-ordered EE0 GSI channel allocation. The reservation
/// is marked before MMIO, and an uncertain command write permanently
/// quarantines all ten AP channel IDs in that allocator generation.
pub fn issue_next_ap_channel_allocation<I: RegisterIo>(
    io: &mut I,
    allocator: &mut ApGsiChannelAllocator,
    reservation: PendingApGsiChannelIds,
) -> Result<ChannelAllocationCommandReceipt, ChannelAllocationCommandError<I::Error>> {
    let channel_id = allocator
        .next_channel_id(reservation)
        .map_err(ChannelAllocationCommandError::Allocation)?
        .ok_or(ChannelAllocationCommandError::Allocation(
            ApGsiChannelAllocationError::NoChannelToAllocate,
        ))?;
    let command = allocator
        .mark_command_issued(reservation, channel_id)
        .map_err(ChannelAllocationCommandError::Allocation)?;
    let offset = command.offset;
    if offset & 3 != 0
        || offset
            .checked_add(4)
            .map_or(true, |end| end > SUNFISH_GSI_APERTURE_BYTES)
    {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(ChannelAllocationCommandError::InvalidRegister {
            offset,
            quarantine_error,
        });
    }
    if let Err(source) = io.write32(offset, command.value) {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(ChannelAllocationCommandError::Write {
            channel_id,
            source,
            quarantine_error,
        });
    }
    Ok(ChannelAllocationCommandReceipt {
        channel_id,
        command_word: command.value,
    })
}

/// Confirm the next AP channel allocation through the GSI channel-control IRQ
/// and CHSTATE readback. `Ok(None)` means the command has not completed yet;
/// a successful return value is minted only after all ten AP channels have
/// individually reported the source-defined ALLOCATED state.
pub fn complete_next_ap_channel_allocation<I: ChannelControlIo>(
    io: &mut I,
    allocator: &mut ApGsiChannelAllocator,
    reservation: PendingApGsiChannelIds,
) -> Result<Option<ApGsiChannelIds>, ChannelAllocationControlError<I::Error>> {
    if !allocator
        .command_in_flight(reservation)
        .map_err(ChannelAllocationControlError::Allocation)?
    {
        return Err(ChannelAllocationControlError::Allocation(
            ApGsiChannelAllocationError::CompletionNotIssued,
        ));
    }
    let channel_id = allocator
        .next_channel_id(reservation)
        .map_err(ChannelAllocationControlError::Allocation)?
        .ok_or(ChannelAllocationControlError::Allocation(
            ApGsiChannelAllocationError::NoChannelToAllocate,
        ))?;
    let type_status = io
        .read32(SUNFISH_GSI_CONTROL_TYPE_IRQ_OFFSET)
        .map_err(|source| ChannelAllocationControlError::Read {
            offset: SUNFISH_GSI_CONTROL_TYPE_IRQ_OFFSET,
            source,
        })?;
    if type_status == 0 {
        return Ok(None);
    }
    if type_status != SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(ChannelAllocationControlError::UnexpectedType {
            status: type_status,
            quarantine_error,
        });
    }
    let observed = io
        .read32(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET)
        .map_err(|source| ChannelAllocationControlError::Read {
            offset: SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET,
            source,
        })?;
    let expected = 1u32 << channel_id;
    if observed != expected {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(ChannelAllocationControlError::UnexpectedBitmap {
            expected,
            observed,
            quarantine_error,
        });
    }
    if let Err(source) = io.write32(SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET, observed) {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(ChannelAllocationControlError::Acknowledge {
            channel_id,
            source,
            quarantine_error,
        });
    }
    let context_offset =
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(channel_id);
    let context0 = match io.read32(context_offset) {
        Ok(value) => value,
        Err(source) => {
            let quarantine_error = allocator.quarantine_pending(reservation).err();
            return Err(ChannelAllocationControlError::ContextRead {
                channel_id,
                source,
                quarantine_error,
            });
        }
    };
    let state =
        ((context0 & GSI_CHANNEL_CONTEXT_STATE_MASK) >> GSI_CHANNEL_CONTEXT_STATE_SHIFT) as u8;
    allocator
        .confirm_channel_allocated(reservation, observed, state)
        .map_err(ChannelAllocationControlError::Allocation)
}

/// Drain and confirm the next AP event-ring allocation completion from the
/// source-defined event-control registers. `Ok(None)` means the command is
/// still pending and may be polled again. Any ambiguous completion or
/// acknowledged-but-unread context quarantines the entire reservation.
/// Unexpected IRQ types/IDs are left unacknowledged and also quarantine the
/// reservation so the caller must stop and diagnose the GSI owner state.
pub fn complete_next_ap_event_ring_allocation<I: EventControlIo>(
    io: &mut I,
    allocator: &mut ApEventRingAllocator,
    reservation: PendingApEventRingIds,
) -> Result<Option<crate::sunfish_ipa_gsi_profile::ApEventRingIds>, EventControlError<I::Error>> {
    if !allocator
        .command_in_flight(reservation)
        .map_err(EventControlError::Allocation)?
    {
        return Err(EventControlError::Allocation(
            EventRingAllocationError::CompletionNotIssued,
        ));
    }
    let event_id = allocator
        .next_event_id(reservation)
        .map_err(EventControlError::Allocation)?
        .ok_or(EventControlError::Allocation(
            EventRingAllocationError::NoEventToAllocate,
        ))?;
    let type_status = io
        .read32(SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET)
        .map_err(|source| EventControlError::Read {
            offset: SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
            source,
        })?;
    if type_status == 0 {
        return Ok(None);
    }
    if type_status != SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(EventControlError::UnexpectedType {
            status: type_status,
            quarantine_error,
        });
    }

    let event_irq_bitmap = io
        .read32(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET)
        .map_err(|source| EventControlError::Read {
            offset: SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET,
            source,
        })?;
    let expected_bitmap = 1u32 << event_id;
    if event_irq_bitmap != expected_bitmap || event_id >= SUNFISH_GSI_MAX_EVENT_RINGS {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(EventControlError::UnexpectedBitmap {
            expected: expected_bitmap,
            observed: event_irq_bitmap,
            quarantine_error,
        });
    }

    if let Err(source) = io.write32(SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, event_irq_bitmap) {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(EventControlError::Acknowledge {
            event_id,
            source,
            quarantine_error,
        });
    }

    let context_offset =
        SUNFISH_GSI_EVENT_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(event_id);
    let context0 = match io.read32(context_offset) {
        Ok(value) => value,
        Err(source) => {
            let quarantine_error = allocator.quarantine_pending(reservation).err();
            return Err(EventControlError::ContextRead {
                event_id,
                source,
                quarantine_error,
            });
        }
    };
    let context_state =
        ((context0 & GSI_EVENT_CONTEXT_STATE_MASK) >> GSI_EVENT_CONTEXT_STATE_SHIFT) as u8;
    allocator
        .confirm_event_allocated(reservation, event_irq_bitmap, context_state)
        .map_err(EventControlError::Allocation)
}

/// Issue exactly one source-defined GSI event-ring ALLOCATE command for the
/// next reserved AP ring. The allocator is marked before MMIO; if the write
/// returns an error, all three reserved IDs are quarantined because the
/// command may have reached hardware. The caller must wait for the event
/// control IRQ, acknowledge it, read the event context state, and then call
/// `confirm_event_allocated` on the allocator before issuing the next ring.
/// The completed AP-channel proof enforces Linux's source order: GSI v2.2
/// allocates every EE channel at boot before `ipa3_setup_apps_pipes()`.
pub fn issue_next_ap_event_ring_allocation<I: RegisterIo>(
    io: &mut I,
    allocator: &mut ApEventRingAllocator,
    reservation: PendingApEventRingIds,
    _allocated_channels: ApGsiChannelIds,
) -> Result<EventRingCommandReceipt, EventRingCommandError<I::Error>> {
    let event_id = allocator
        .next_event_id(reservation)
        .map_err(EventRingCommandError::Allocation)?
        .ok_or(EventRingCommandError::Allocation(
            EventRingAllocationError::NoEventToAllocate,
        ))?;
    let command = allocator
        .mark_command_issued(reservation, event_id)
        .map_err(EventRingCommandError::Allocation)?;
    if let Err(source) = io.write32(command.offset, command.value) {
        let quarantine_error = allocator.quarantine_pending(reservation).err();
        return Err(EventRingCommandError::Write {
            event_id,
            source,
            quarantine_error,
        });
    }
    Ok(EventRingCommandReceipt {
        event_id,
        command_word: command.value,
    })
}

/// Write one validated AP context plan in its source-derived order.
///
/// All offsets are validated before the first hardware access. On an I/O
/// failure, the executor stops immediately and reports the prefix whose
/// writes returned success. It never retries because the failed write may
/// have taken effect.
///
/// The caller must already have successfully allocated the referenced GSI
/// event rings and channels and installed the matching DMA/SMMU state. This
/// function does not verify those preconditions or issue a start command.
pub fn execute_ap_context<I: RegisterIo>(
    io: &mut I,
    plan: ApGsiContextPlan,
) -> Result<ExecuteReceipt, ExecuteError<I::Error>> {
    let channel_starts = *plan.channel_starts();
    let writes = plan.writes();
    debug_assert_eq!(writes.len(), AP_GSI_CONTEXT_WRITE_COUNT);
    for (index, write) in writes.iter().enumerate() {
        if write.offset & 3 != 0
            || write
                .offset
                .checked_add(4)
                .map_or(true, |end| end > SUNFISH_GSI_APERTURE_BYTES)
        {
            return Err(ExecuteError::InvalidRegister {
                index,
                offset: write.offset,
            });
        }
    }

    let mut confirmed_writes = 0u16;
    for (index, write) in writes.iter().enumerate() {
        if let Err(source) = io.write32(write.offset, write.value) {
            return Err(ExecuteError::Write {
                index,
                offset: write.offset,
                confirmed_writes,
                source,
            });
        }
        confirmed_writes += 1;
    }
    Ok(ExecuteReceipt {
        confirmed_writes,
        pending_event_primes: PendingApEventRingPrimes::new(*plan.event_ring_primes()),
        pending_channels: PendingApChannelStarts::new(channel_starts),
        quarantined: false,
    })
}

/// Prime the next configured GPI event ring as the pinned Linux driver does:
/// write the initial pointer MSB followed by its LSB. The ring memory must
/// already be backed by the planned IOVA and zeroed/flushed by its owner.
/// Any uncertain doorbell write quarantines the context generation, and
/// channel starts remain unavailable until all three event rings are primed.
pub fn prime_next_ap_event_ring<I: RegisterIo>(
    io: &mut I,
    receipt: &mut ExecuteReceipt,
) -> Result<u8, EventRingPrimeError<I::Error>> {
    if receipt.quarantined {
        return Err(EventRingPrimeError::Quarantined);
    }
    if receipt.pending_channels.next_index != 0 || receipt.pending_channels.command_in_flight {
        return Err(EventRingPrimeError::ChannelStartAlreadyBegan);
    }
    let event = *receipt
        .pending_event_primes
        .events
        .get(receipt.pending_event_primes.next_index)
        .ok_or(EventRingPrimeError::NoPendingEventRing)?;
    let event_id = event.event_id();
    let msb_offset = SUNFISH_GSI_EVENT_DOORBELL1_OFFSET
        + SUNFISH_GSI_EVENT_DOORBELL_STRIDE * u32::from(event_id);
    let lsb_offset = SUNFISH_GSI_EVENT_DOORBELL0_OFFSET
        + SUNFISH_GSI_EVENT_DOORBELL_STRIDE * u32::from(event_id);
    for offset in [msb_offset, lsb_offset] {
        if offset & 3 != 0
            || offset
                .checked_add(4)
                .map_or(true, |end| end > SUNFISH_GSI_APERTURE_BYTES)
        {
            receipt.quarantine();
            return Err(EventRingPrimeError::InvalidRegister { offset });
        }
    }
    let pointer = event.initial_write_pointer();
    for (write_index, (offset, value)) in [
        (msb_offset, (pointer >> 32) as u32),
        (lsb_offset, pointer as u32),
    ]
    .into_iter()
    .enumerate()
    {
        if let Err(source) = io.write32(offset, value) {
            receipt.quarantine();
            return Err(EventRingPrimeError::Write {
                event_id,
                offset,
                write_index: write_index as u8,
                source,
            });
        }
    }
    receipt.pending_event_primes.next_index += 1;
    Ok(event_id)
}

/// Issue one source-defined EE0 `GSI_CH_START` command for the next Apps
/// system channel whose complete context plan was written successfully.
/// Mark the command in flight before touching MMIO; an uncertain write
/// quarantines this context generation and must never be replayed.
pub fn issue_next_ap_channel_start<I: RegisterIo>(
    io: &mut I,
    receipt: &mut ExecuteReceipt,
) -> Result<ChannelStartCommandReceipt, ChannelStartCommandError<I::Error>> {
    if receipt.quarantined {
        return Err(ChannelStartCommandError::Quarantined);
    }
    if !receipt.pending_event_primes.all_primed() {
        return Err(ChannelStartCommandError::EventRingsNotPrimed);
    }
    let pending = &mut receipt.pending_channels;
    if pending.quarantined {
        return Err(ChannelStartCommandError::Quarantined);
    }
    if pending.command_in_flight {
        return Err(ChannelStartCommandError::CommandAlreadyIssued);
    }
    let channel = *pending
        .channels
        .get(pending.next_index)
        .ok_or(ChannelStartCommandError::NoPendingChannel)?;
    let channel_id = channel.channel_id();
    let offset = SUNFISH_GSI_CHANNEL_COMMAND_OFFSET;
    if channel_id >= 32 || offset & 3 != 0 || offset + 4 > SUNFISH_GSI_APERTURE_BYTES {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelStartCommandError::InvalidRegister { offset });
    }
    let command_word = (GSI_CHANNEL_START_OPCODE << 24) | u32::from(channel_id);
    pending.command_in_flight = true;
    if let Err(source) = io.write32(offset, command_word) {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelStartCommandError::Write { channel_id, source });
    }
    Ok(ChannelStartCommandReceipt {
        channel_id,
        command_word,
    })
}

/// Confirm the next channel start through the source driver's channel-control
/// IRQ path: require the exact control type and channel bit, acknowledge that
/// bit, read CHSTATE, and write the initial transfer-ring pointer MSB. `None`
/// means the start command remains pending and may only be polled, not resent.
/// Any unexpected completion, ambiguous acknowledgement, or failure after
/// acknowledgement quarantines the whole context generation.
pub fn complete_next_ap_channel_start<I: ChannelControlIo>(
    io: &mut I,
    receipt: &mut ExecuteReceipt,
) -> Result<Option<ChannelStartCompletion>, ChannelControlError<I::Error>> {
    if receipt.quarantined {
        return Err(ChannelControlError::Quarantined);
    }
    let pending = &mut receipt.pending_channels;
    if pending.quarantined {
        return Err(ChannelControlError::Quarantined);
    }
    if !pending.command_in_flight {
        return Err(ChannelControlError::CommandNotIssued);
    }
    let channel = *pending
        .channels
        .get(pending.next_index)
        .ok_or(ChannelControlError::CommandNotIssued)?;
    let channel_id = channel.channel_id();

    let type_status = io
        .read32(SUNFISH_GSI_CONTROL_TYPE_IRQ_OFFSET)
        .map_err(|source| ChannelControlError::Read {
            offset: SUNFISH_GSI_CONTROL_TYPE_IRQ_OFFSET,
            source,
        })?;
    if type_status == 0 {
        return Ok(None);
    }
    if type_status != SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelControlError::UnexpectedType {
            status: type_status,
        });
    }

    let channel_irq_bitmap =
        io.read32(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET)
            .map_err(|source| ChannelControlError::Read {
                offset: SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET,
                source,
            })?;
    let expected_bitmap = 1u32 << channel_id;
    if channel_irq_bitmap != expected_bitmap {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelControlError::UnexpectedBitmap {
            expected: expected_bitmap,
            observed: channel_irq_bitmap,
        });
    }

    if let Err(source) = io.write32(SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET, channel_irq_bitmap) {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelControlError::Acknowledge { channel_id, source });
    }

    let context_offset =
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(channel_id);
    let context0 = match io.read32(context_offset) {
        Ok(value) => value,
        Err(source) => {
            pending.quarantine();
            receipt.quarantined = true;
            return Err(ChannelControlError::ContextRead { channel_id, source });
        }
    };
    let state =
        ((context0 & GSI_CHANNEL_CONTEXT_STATE_MASK) >> GSI_CHANNEL_CONTEXT_STATE_SHIFT) as u8;
    if state != GSI_CHANNEL_STATE_STARTED && state != GSI_CHANNEL_STATE_FLOW_CONTROL {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelControlError::UnexpectedState { channel_id, state });
    }

    let doorbell_offset = SUNFISH_GSI_CHANNEL_DOORBELL1_OFFSET
        + SUNFISH_GSI_CHANNEL_DOORBELL_STRIDE * u32::from(channel_id);
    let doorbell_msb = (channel.initial_write_pointer() >> 32) as u32;
    if let Err(source) = io.write32(doorbell_offset, doorbell_msb) {
        pending.quarantine();
        receipt.quarantined = true;
        return Err(ChannelControlError::DoorbellWrite { channel_id, source });
    }

    pending.command_in_flight = false;
    pending.next_index += 1;
    Ok(Some(ChannelStartCompletion {
        channel_id,
        state,
        doorbell_msb,
    }))
}
