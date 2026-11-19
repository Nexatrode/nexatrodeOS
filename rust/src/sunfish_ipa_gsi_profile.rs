//! Source-derived IPA v4.2/GSI v2.2 AP-pipe and context profile for Sunfish.
//!
//! Endpoint assignments come from LineageOS kernel commit
//! `28f9290ae067d3e7857126eb6312720dbe9c7c80`,
//! `drivers/platform/msm/ipa/ipa_v3/ipa_utils.c`. The three AP system-pipe
//! requests follow `ipa3_setup_apps_pipes()` in `ipa.c`; GSI context fields
//! follow `drivers/platform/msm/gsi/gsi.c` and `gsi_reg_v2.h`. This module
//! creates bounded IOVA and register-write plans only. It does not allocate or
//! map rings, access MMIO, enable IRQs, issue GSI commands, or start IPA/WLAN.
#![forbid(unsafe_code)]

pub const SUNFISH_IPA_HW_V4_2: u32 = 16;
/// `ipa3_get_gsi_ver(IPA_HW_v4_2)` returns `GSI_VER_2_2` (enum value 5).
pub const SUNFISH_GSI_VERSION_2_2: u32 = 5;
/// The pinned Qualcomm GSI driver declares `GSI_EVT_RING_MAX` as 24, so
/// allocatable event-ring IDs are `0..24` even though the ERINDEX field is
/// five bits wide.
pub const SUNFISH_GSI_MAX_EVENT_RINGS: u8 = 24;
/// `GSI_CHAN_MAX` in the pinned GSI driver; channel indices are `0..31`.
pub const SUNFISH_GSI_MAX_CHANNELS: u8 = 31;
/// `GSI_V2_2_EE_n_GSI_HW_PARAM_2_OFFS(0)` from the pinned register header.
pub const SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET: u32 = 0x0001_f040;
/// AP execution-environment event-ring command register from the pinned
/// `GSI_EE_n_EV_CH_CMD_OFFS(0)` definition.
pub const SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET: u32 = 0x0001_2010;
/// The source driver's `GSI_EVT_ALLOCATE` opcode is zero; the event ID is in
/// bits 7:0 of the command word.
pub const SUNFISH_GSI_EVENT_ALLOCATE_OPCODE: u32 = 0;
/// AP channels are allocated in ascending IPA endpoint order by
/// `ipa3_alloc_gsi_channel()`, after filtering the IPA v4.2 map to EE_AP.
/// The IDs are USB producer, Apps WAN producer, Apps LAN producer, WLAN
/// producer, Apps command producer, Apps LAN consumer, Apps WAN consumer,
/// USB DPL consumer, WLAN consumer, and USB consumer.
pub const AP_GSI_CHANNEL_ALLOCATE_ORDER: [u8; 10] = [5, 0, 6, 7, 1, 2, 3, 4, 8, 9];
pub const SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET: u32 = 0x0001_2008;
/// `GSI_CH_ALLOCATE` is opcode zero; the AP channel ID occupies bits 7:0.
pub const SUNFISH_GSI_CHANNEL_ALLOCATE_OPCODE: u32 = 0;
const GSI_V2_2_NUM_EVENT_RINGS_MASK: u32 = 0x0000_1f00;
const GSI_V2_2_NUM_EVENT_RINGS_SHIFT: u32 = 8;
const GSI_V2_2_NUM_CHANNELS_MASK: u32 = 0x0000_00f8;
const GSI_V2_2_NUM_CHANNELS_SHIFT: u32 = 3;
pub const AP_EXECUTION_ENVIRONMENT: u8 = 0;

pub const COMMON_EVENT_RING_BYTES: u32 = 0x7c00;
pub const AP_CMD_DESC_FIFO_BYTES: u32 = 0x800;
pub const AP_LAN_RX_DESC_FIFO_BYTES: u32 = 0x800;
pub const AP_LAN_TX_DESC_FIFO_BYTES: u32 = 0x1000;
pub const PAGE_BYTES: u32 = 0x1000;
/// Linux's `ipa_smmu_ap` qcom,iova-mapping start and size from Sunfish DTS.
/// A seL4 mapping may use another window only when its translated context and
/// DMA aperture are explicitly configured to cover that window.
pub const LINUX_IPA_AP_IOVA_START: u64 = 0x2000_0000;
pub const LINUX_IPA_AP_IOVA_BYTES: u64 = 0x4000_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Client {
    AppsCommandProducer,
    AppsLanConsumer,
    AppsLanProducer,
    AppsWanConsumer,
    WlanProducer,
    WlanConsumer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefetchMode {
    UsePrefetchBuffers,
    EscapeBufferOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub ipa_endpoint: u8,
    pub gsi_channel: u8,
    pub ipa_if_tlv: u8,
    pub ipa_if_aos: u8,
    pub execution_environment: u8,
    pub prefetch_mode: PrefetchMode,
    pub prefetch_threshold: u8,
}

pub const fn endpoint(client: Client) -> Endpoint {
    match client {
        // { IPA ep, GSI channel, IPA_IF TLV, IPA_IF AOS, EE, prefetch }
        Client::AppsCommandProducer => Endpoint {
            ipa_endpoint: 6,
            gsi_channel: 1,
            ipa_if_tlv: 20,
            ipa_if_aos: 20,
            execution_environment: AP_EXECUTION_ENVIRONMENT,
            prefetch_mode: PrefetchMode::UsePrefetchBuffers,
            prefetch_threshold: 0,
        },
        Client::AppsLanConsumer => Endpoint {
            ipa_endpoint: 8,
            gsi_channel: 2,
            ipa_if_tlv: 6,
            ipa_if_aos: 6,
            execution_environment: AP_EXECUTION_ENVIRONMENT,
            prefetch_mode: PrefetchMode::EscapeBufferOnly,
            prefetch_threshold: 0,
        },
        Client::AppsLanProducer => Endpoint {
            ipa_endpoint: 2,
            gsi_channel: 6,
            ipa_if_tlv: 8,
            ipa_if_aos: 9,
            execution_environment: AP_EXECUTION_ENVIRONMENT,
            prefetch_mode: PrefetchMode::EscapeBufferOnly,
            prefetch_threshold: 0,
        },
        Client::AppsWanConsumer => Endpoint {
            ipa_endpoint: 9,
            gsi_channel: 3,
            ipa_if_tlv: 6,
            ipa_if_aos: 6,
            execution_environment: AP_EXECUTION_ENVIRONMENT,
            prefetch_mode: PrefetchMode::EscapeBufferOnly,
            prefetch_threshold: 0,
        },
        Client::WlanProducer => Endpoint {
            ipa_endpoint: 3,
            gsi_channel: 7,
            ipa_if_tlv: 6,
            ipa_if_aos: 7,
            execution_environment: AP_EXECUTION_ENVIRONMENT,
            prefetch_mode: PrefetchMode::UsePrefetchBuffers,
            prefetch_threshold: 0,
        },
        Client::WlanConsumer => Endpoint {
            ipa_endpoint: 14,
            gsi_channel: 8,
            ipa_if_tlv: 6,
            ipa_if_aos: 9,
            execution_environment: AP_EXECUTION_ENVIRONMENT,
            prefetch_mode: PrefetchMode::UsePrefetchBuffers,
            prefetch_threshold: 0,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipeMode {
    Basic,
    Dma,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventRingPolicy {
    Private,
    SharedCommon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppsSystemPipe {
    pub client: Client,
    pub descriptor_fifo_bytes: u32,
    pub mode: PipeMode,
    /// The CMD producer sends through IPA DMA to the LAN consumer endpoint.
    pub destination_endpoint: Option<u8>,
    pub event_ring_policy: EventRingPolicy,
}

/// AP pipe creation order from `ipa3_setup_apps_pipes()` on non-MHI Sunfish.
pub const AP_SYSTEM_PIPES: [AppsSystemPipe; 3] = [
    AppsSystemPipe {
        client: Client::AppsCommandProducer,
        descriptor_fifo_bytes: AP_CMD_DESC_FIFO_BYTES,
        mode: PipeMode::Dma,
        destination_endpoint: Some(8),
        event_ring_policy: EventRingPolicy::Private,
    },
    AppsSystemPipe {
        client: Client::AppsLanConsumer,
        descriptor_fifo_bytes: AP_LAN_RX_DESC_FIFO_BYTES,
        mode: PipeMode::Basic,
        destination_endpoint: None,
        event_ring_policy: EventRingPolicy::Private,
    },
    AppsSystemPipe {
        client: Client::AppsLanProducer,
        descriptor_fifo_bytes: AP_LAN_TX_DESC_FIFO_BYTES,
        mode: PipeMode::Basic,
        destination_endpoint: None,
        event_ring_policy: EventRingPolicy::SharedCommon,
    },
];
pub const AP_SYSTEM_PIPE_COUNT: usize = AP_SYSTEM_PIPES.len();

/// GSI transfer-ring bytes used by the matched driver for a system pipe.
/// GSI transfer elements are 16 bytes; this preserves the driver's explicit
/// `ring_size = 2 * desc_fifo_sz` formula and does not allocate that memory.
pub const fn transfer_ring_bytes(descriptor_fifo_bytes: u32) -> Option<u32> {
    descriptor_fifo_bytes.checked_mul(2)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingKind {
    CommonEvent,
    AppsCommandEvent,
    AppsCommandTransfer,
    AppsLanReceiveEvent,
    AppsLanReceiveTransfer,
    AppsLanTransmitTransfer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaPool {
    /// IOVA base selected by the owner and covered by its SMMU context.
    pub base_iova: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingRegion {
    pub kind: RingKind,
    pub base_iova: u64,
    /// The hardware-visible ring length, which may be smaller than the map.
    pub ring_bytes: u32,
    /// Page-rounded extent reserved for the fixed IOVA pool.
    pub mapped_bytes: u32,
    /// GSI requires each ring base to align to `next_power_of_two(ring_bytes)`.
    pub base_alignment: u32,
}

pub const RING_REGION_COUNT: usize = 6;

/// Number of GSI context registers programmed for the three AP system pipes
/// and their common/private GPI event rings by the pinned Linux driver.
pub const AP_GSI_CONTEXT_WRITE_COUNT: usize = 45;
/// Page mappings required when all six planned ring regions use 4 KiB pages.
pub const AP_DMA_PAGE_MAPPING_COUNT: usize = 14;

/// The following offsets are relative to the GSI register base. They match
/// `drivers/platform/msm/gsi/gsi_reg_v2.h` at the pinned LineageOS revision.
const GSI_CHANNEL_CONTEXT_BASE: u32 = 0x0000_f000;
const GSI_EVENT_CONTEXT_BASE: u32 = 0x0001_0000;
const GSI_CHANNEL_QOS_BASE: u32 = 0x0001_c05c;
const GSI_EE_STRIDE: u32 = 0x4000;
const GSI_CONTEXT_STRIDE: u32 = 0x80;
const GSI_CHANNEL_ERINDEX_SHIFT: u32 = 14;
const GSI_CHANNEL_DIR_SHIFT: u32 = 3;
const GSI_ELEMENT_SIZE_SHIFT: u32 = 24;
const GSI_EVENT_INTERRUPT_TYPE_SHIFT: u32 = 16;
const GSI_EVENT_MODERATION_COUNT_SHIFT: u32 = 16;
const GSI_CHANNEL_QOS_PREFETCH_SHIFT: u32 = 10;

/// Dynamic event-ring IDs returned by the source driver's allocation calls.
/// The common ring is shared by Apps LAN producer and any private ring IDs
/// must be distinct from it and from one another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApEventRingIds {
    common: u8,
    apps_command: u8,
    apps_lan_receive: u8,
}

impl ApEventRingIds {
    pub const fn common(self) -> u8 {
        self.common
    }

    pub const fn apps_command(self) -> u8 {
        self.apps_command
    }

    pub const fn apps_lan_receive(self) -> u8 {
        self.apps_lan_receive
    }

    const fn allocation_mask(self) -> u32 {
        (1 << self.common) | (1 << self.apps_command) | (1 << self.apps_lan_receive)
    }
}

/// An in-flight reservation for the three event rings used by the AP system
/// pipes. IDs are selected with the same lowest-free-bit policy as the
/// reference GSI driver, but they are not usable in a context plan until each
/// allocation command has an IRQ and an `ALLOCATED` context-state readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingApEventRingIds {
    ids: ApEventRingIds,
    generation: u32,
}

impl PendingApEventRingIds {
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventRingAllocationError {
    InvalidOccupiedMask,
    TooFewFreeEventRings,
    GenerationExhausted,
    ReservationAlreadyPending,
    StaleReservation,
    CommandAlreadyIssued,
    WrongEventOrder,
    CompletionNotIssued,
    CompletionNotReady,
    NoEventToAllocate,
    UnexpectedEventState { event_id: u8, state: u8 },
    CannotCancelAfterHardwareAccess,
}

/// Tracks allocated, pending, and quarantined AP event-ring IDs. A command
/// whose write or completion is uncertain permanently consumes its reserved
/// IDs for this allocator generation; callers must not retry those IDs.
#[derive(Debug, Eq, PartialEq)]
pub struct ApEventRingAllocator {
    capacity: ApEventRingCapacity,
    occupied_mask: u32,
    quarantined_mask: u32,
    pending: Option<PendingApEventRingIds>,
    next_generation: u32,
    next_event_index: u8,
    command_issued: bool,
}

impl ApEventRingAllocator {
    /// `occupied_mask` must reflect event IDs already allocated in this GSI
    /// execution environment. It is retained across successful reservations.
    pub fn new(
        capacity: ApEventRingCapacity,
        occupied_mask: u32,
    ) -> Result<Self, EventRingAllocationError> {
        if occupied_mask & !event_capacity_mask(capacity) != 0 {
            return Err(EventRingAllocationError::InvalidOccupiedMask);
        }
        Ok(Self {
            capacity,
            occupied_mask,
            quarantined_mask: 0,
            pending: None,
            next_generation: 1,
            next_event_index: 0,
            command_issued: false,
        })
    }

    /// Reserve the first three free IDs, matching the common, Apps command,
    /// and Apps LAN receive allocation order in `ipa3_setup_apps_pipes()`.
    /// No allocator state changes if fewer than three IDs are free.
    pub fn reserve_ap_system_rings(
        &mut self,
    ) -> Result<PendingApEventRingIds, EventRingAllocationError> {
        if self.pending.is_some() {
            return Err(EventRingAllocationError::ReservationAlreadyPending);
        }
        let generation = self.next_generation;
        let next_generation = generation
            .checked_add(1)
            .ok_or(EventRingAllocationError::GenerationExhausted)?;
        let mut selected = [0u8; 3];
        let mut found = 0usize;
        for id in 0..self.capacity.count() {
            if self.occupied_mask & (1u32 << id) == 0 {
                selected[found] = id;
                found += 1;
                if found == selected.len() {
                    break;
                }
            }
        }
        if found != selected.len() {
            return Err(EventRingAllocationError::TooFewFreeEventRings);
        }
        let ids = ApEventRingIds {
            common: selected[0],
            apps_command: selected[1],
            apps_lan_receive: selected[2],
        };
        self.occupied_mask |= ids.allocation_mask();
        let reservation = PendingApEventRingIds { ids, generation };
        self.pending = Some(reservation);
        self.next_generation = next_generation;
        self.next_event_index = 0;
        self.command_issued = false;
        Ok(reservation)
    }

    /// Return the only event ID whose allocation command may be issued next.
    pub fn next_event_id(
        &self,
        reservation: PendingApEventRingIds,
    ) -> Result<Option<u8>, EventRingAllocationError> {
        self.check_reservation(reservation)?;
        if self.next_event_index >= 3 {
            return Ok(None);
        }
        Ok(Some(event_id_at(reservation.ids, self.next_event_index)))
    }

    pub fn command_in_flight(
        &self,
        reservation: PendingApEventRingIds,
    ) -> Result<bool, EventRingAllocationError> {
        self.check_reservation(reservation)?;
        Ok(self.command_issued)
    }

    /// Mark the next allocation command as issued before attempting its MMIO
    /// write. If that write reports an error, the caller must quarantine the
    /// reservation because the command may have reached hardware.
    pub fn mark_command_issued(
        &mut self,
        reservation: PendingApEventRingIds,
        event_id: u8,
    ) -> Result<GsiRegisterWrite, EventRingAllocationError> {
        self.check_reservation(reservation)?;
        if self.command_issued {
            return Err(EventRingAllocationError::CommandAlreadyIssued);
        }
        if self.next_event_index >= 3
            || event_id != event_id_at(reservation.ids, self.next_event_index)
        {
            return Err(EventRingAllocationError::WrongEventOrder);
        }
        self.command_issued = true;
        Ok(GsiRegisterWrite {
            offset: SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET,
            value: (SUNFISH_GSI_EVENT_ALLOCATE_OPCODE << 24) | u32::from(event_id),
        })
    }

    /// Accept one command completion only when its event IRQ bit is present
    /// and the event context reports the source-defined `ALLOCATED` state
    /// (`1`). A missing IRQ is retryable observation; an IRQ with any other
    /// state quarantines all three IDs and cannot be retried.
    pub fn confirm_event_allocated(
        &mut self,
        reservation: PendingApEventRingIds,
        event_irq_bitmap: u32,
        context_state: u8,
    ) -> Result<Option<ApEventRingIds>, EventRingAllocationError> {
        self.check_reservation(reservation)?;
        if !self.command_issued || self.next_event_index >= 3 {
            return Err(EventRingAllocationError::CompletionNotIssued);
        }
        let event_id = event_id_at(reservation.ids, self.next_event_index);
        if event_irq_bitmap & (1u32 << event_id) == 0 {
            return Err(EventRingAllocationError::CompletionNotReady);
        }
        if context_state != 1 {
            self.quarantine(reservation);
            return Err(EventRingAllocationError::UnexpectedEventState {
                event_id,
                state: context_state,
            });
        }

        self.command_issued = false;
        self.next_event_index += 1;
        if self.next_event_index == 3 {
            self.pending = None;
            return Ok(Some(reservation.ids));
        }
        Ok(None)
    }

    /// Permanently consume every reserved ID after an uncertain write, timeout,
    /// or abandoned transaction following hardware access.
    pub fn quarantine_pending(
        &mut self,
        reservation: PendingApEventRingIds,
    ) -> Result<(), EventRingAllocationError> {
        self.check_reservation(reservation)?;
        self.quarantine(reservation);
        Ok(())
    }

    /// Release the reservation only when no hardware command has been issued.
    pub fn cancel_before_io(
        &mut self,
        reservation: PendingApEventRingIds,
    ) -> Result<(), EventRingAllocationError> {
        self.check_reservation(reservation)?;
        if self.command_issued || self.next_event_index != 0 {
            return Err(EventRingAllocationError::CannotCancelAfterHardwareAccess);
        }
        self.occupied_mask &= !reservation.ids.allocation_mask();
        self.pending = None;
        Ok(())
    }

    pub const fn occupied_mask(&self) -> u32 {
        self.occupied_mask
    }

    pub const fn quarantined_mask(&self) -> u32 {
        self.quarantined_mask
    }

    fn check_reservation(
        &self,
        reservation: PendingApEventRingIds,
    ) -> Result<(), EventRingAllocationError> {
        if self.pending == Some(reservation) {
            Ok(())
        } else {
            Err(EventRingAllocationError::StaleReservation)
        }
    }

    fn quarantine(&mut self, reservation: PendingApEventRingIds) {
        self.quarantined_mask |= reservation.ids.allocation_mask();
        self.pending = None;
        self.command_issued = false;
    }
}

const fn event_capacity_mask(capacity: ApEventRingCapacity) -> u32 {
    (1u32 << capacity.count()) - 1
}

const fn event_id_at(ids: ApEventRingIds, index: u8) -> u8 {
    match index {
        0 => ids.common,
        1 => ids.apps_command,
        _ => ids.apps_lan_receive,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GsiRegisterWrite {
    /// Offset relative to the GSI register aperture, not an MMIO address.
    pub offset: u32,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GsiContextPlanError {
    UnsupportedVersion,
    InvalidEventRingCount,
    InvalidChannelCount,
    EventIdOutOfRange,
    DuplicateEventId,
    ChannelNotAllocated { channel_id: u8 },
    MissingRing,
    InvalidRingGeometry,
    WriteCountInvariant,
}

/// Live AP-execution-environment event-ring capacity decoded from GSI v2.2
/// `HW_PARAM_2`. The field stays private so callers cannot provide an
/// unchecked count to the AP context planner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApEventRingCapacity(u8);

impl ApEventRingCapacity {
    /// Decode `GSI_NUM_EV_PER_EE` from the v2.2 AP `HW_PARAM_2` readback.
    /// The pinned driver rejects zero and values above its 24-entry context
    /// array before it permits event-ring allocation.
    pub fn from_v2_2_hw_param_2(register_value: u32) -> Result<Self, GsiContextPlanError> {
        let count = ((register_value & GSI_V2_2_NUM_EVENT_RINGS_MASK)
            >> GSI_V2_2_NUM_EVENT_RINGS_SHIFT) as u8;
        if count == 0 || count > SUNFISH_GSI_MAX_EVENT_RINGS {
            return Err(GsiContextPlanError::InvalidEventRingCount);
        }
        Ok(Self(count))
    }

    pub const fn count(self) -> u8 {
        self.0
    }
}

/// Live AP-execution-environment channel capacity decoded from GSI v2.2
/// `HW_PARAM_2`. The type is private-fielded so allocation and context
/// planning cannot accept a caller-invented channel count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApGsiChannelCapacity(u8);

impl ApGsiChannelCapacity {
    pub fn from_v2_2_hw_param_2(register_value: u32) -> Result<Self, GsiContextPlanError> {
        let count =
            ((register_value & GSI_V2_2_NUM_CHANNELS_MASK) >> GSI_V2_2_NUM_CHANNELS_SHIFT) as u8;
        if count == 0 || count > SUNFISH_GSI_MAX_CHANNELS {
            return Err(GsiContextPlanError::InvalidChannelCount);
        }
        Ok(Self(count))
    }

    pub const fn count(self) -> u8 {
        self.0
    }
}

/// Read-only AP channel-state snapshot used as the allocator's occupied-mask
/// source. A channel is considered occupied whenever its CHSTATE is nonzero;
/// only the source-defined NOT_ALLOCATED state is eligible for a new command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApGsiChannelSnapshot {
    capacity: ApGsiChannelCapacity,
    states: [u8; 10],
}

impl ApGsiChannelSnapshot {
    pub const fn capacity(self) -> ApGsiChannelCapacity {
        self.capacity
    }

    pub const fn state(self, channel_id: u8) -> Option<u8> {
        let mut index = 0;
        while index < AP_GSI_CHANNEL_ALLOCATE_ORDER.len() {
            if AP_GSI_CHANNEL_ALLOCATE_ORDER[index] == channel_id {
                return Some(self.states[index]);
            }
            index += 1;
        }
        None
    }

    pub const fn occupied_mask(self) -> u32 {
        let mut mask = 0;
        let mut index = 0;
        while index < AP_GSI_CHANNEL_ALLOCATE_ORDER.len() {
            if self.states[index] != 0 {
                mask |= 1u32 << AP_GSI_CHANNEL_ALLOCATE_ORDER[index];
            }
            index += 1;
        }
        mask
    }

    pub(crate) fn from_readback(
        capacity: ApGsiChannelCapacity,
        states: [u8; 10],
    ) -> Result<Self, ApGsiChannelAllocationError> {
        if let Some(channel_id) = AP_GSI_CHANNEL_ALLOCATE_ORDER
            .into_iter()
            .find(|channel_id| *channel_id >= capacity.count())
        {
            return Err(ApGsiChannelAllocationError::ChannelOutsideCapacity { channel_id });
        }
        Ok(Self { capacity, states })
    }
}

/// Unforgeable proof that every Sunfish AP GSI channel needed by the pinned
/// Linux IPA endpoint map has reached ALLOCATED state in the current GSI
/// generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApGsiChannelIds {
    allocated_mask: u32,
    generation: u32,
}

impl ApGsiChannelIds {
    pub const fn contains(self, channel_id: u8) -> bool {
        channel_id < 32 && self.allocated_mask & (1u32 << channel_id) != 0
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }

    pub const fn allocated_mask(self) -> u32 {
        self.allocated_mask
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingApGsiChannelIds {
    generation: u32,
}

impl PendingApGsiChannelIds {
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApGsiChannelAllocationError {
    InvalidOccupiedMask,
    ChannelOutsideCapacity { channel_id: u8 },
    ChannelAlreadyOccupied { channel_id: u8 },
    GenerationExhausted,
    ReservationAlreadyPending,
    StaleReservation,
    CommandAlreadyIssued,
    WrongChannelOrder,
    CompletionNotIssued,
    CompletionNotReady,
    NoChannelToAllocate,
    UnexpectedChannelBitmap { expected: u32, observed: u32 },
    UnexpectedChannelState { channel_id: u8, state: u8 },
    CannotCancelAfterHardwareAccess,
}

/// Tracks the source-ordered AP channel allocation sequence. The initial
/// occupied mask must come from a complete GSI channel-state snapshot taken
/// by the exclusive owner immediately before this allocator is created.
#[derive(Debug, Eq, PartialEq)]
pub struct ApGsiChannelAllocator {
    capacity: ApGsiChannelCapacity,
    occupied_mask: u32,
    quarantined_mask: u32,
    pending: Option<PendingApGsiChannelIds>,
    next_generation: u32,
    next_channel_index: u8,
    command_issued: bool,
}

impl ApGsiChannelAllocator {
    pub(crate) fn new(
        capacity: ApGsiChannelCapacity,
        occupied_mask: u32,
    ) -> Result<Self, ApGsiChannelAllocationError> {
        if occupied_mask & !channel_capacity_mask(capacity) != 0 {
            return Err(ApGsiChannelAllocationError::InvalidOccupiedMask);
        }
        Ok(Self {
            capacity,
            occupied_mask,
            quarantined_mask: 0,
            pending: None,
            next_generation: 1,
            next_channel_index: 0,
            command_issued: false,
        })
    }

    pub fn from_snapshot(
        snapshot: ApGsiChannelSnapshot,
    ) -> Result<Self, ApGsiChannelAllocationError> {
        Self::new(snapshot.capacity(), snapshot.occupied_mask())
    }

    /// Reserve every AP channel in the same order that `ipa3_alloc_gsi_channel`
    /// reaches them after filtering the IPA endpoint table to EE_AP. No state
    /// changes if a channel is out of range or already occupied.
    pub fn reserve_ap_channels(
        &mut self,
    ) -> Result<PendingApGsiChannelIds, ApGsiChannelAllocationError> {
        if self.pending.is_some() {
            return Err(ApGsiChannelAllocationError::ReservationAlreadyPending);
        }
        let generation = self.next_generation;
        let next_generation = generation
            .checked_add(1)
            .ok_or(ApGsiChannelAllocationError::GenerationExhausted)?;
        let mut requested_mask = 0u32;
        for channel_id in AP_GSI_CHANNEL_ALLOCATE_ORDER {
            if channel_id >= self.capacity.count() {
                return Err(ApGsiChannelAllocationError::ChannelOutsideCapacity { channel_id });
            }
            requested_mask |= 1u32 << channel_id;
        }
        let occupied = self.occupied_mask & requested_mask;
        if occupied != 0 {
            let channel_id = AP_GSI_CHANNEL_ALLOCATE_ORDER
                .into_iter()
                .find(|channel_id| occupied & (1u32 << channel_id) != 0)
                .unwrap_or(0);
            return Err(ApGsiChannelAllocationError::ChannelAlreadyOccupied { channel_id });
        }
        self.occupied_mask |= requested_mask;
        let reservation = PendingApGsiChannelIds { generation };
        self.pending = Some(reservation);
        self.next_generation = next_generation;
        self.next_channel_index = 0;
        self.command_issued = false;
        Ok(reservation)
    }

    pub fn next_channel_id(
        &self,
        reservation: PendingApGsiChannelIds,
    ) -> Result<Option<u8>, ApGsiChannelAllocationError> {
        self.check_reservation(reservation)?;
        if self.next_channel_index as usize >= AP_GSI_CHANNEL_ALLOCATE_ORDER.len() {
            return Ok(None);
        }
        Ok(Some(
            AP_GSI_CHANNEL_ALLOCATE_ORDER[self.next_channel_index as usize],
        ))
    }

    pub fn command_in_flight(
        &self,
        reservation: PendingApGsiChannelIds,
    ) -> Result<bool, ApGsiChannelAllocationError> {
        self.check_reservation(reservation)?;
        Ok(self.command_issued)
    }

    pub fn mark_command_issued(
        &mut self,
        reservation: PendingApGsiChannelIds,
        channel_id: u8,
    ) -> Result<GsiRegisterWrite, ApGsiChannelAllocationError> {
        self.check_reservation(reservation)?;
        if self.command_issued {
            return Err(ApGsiChannelAllocationError::CommandAlreadyIssued);
        }
        if self.next_channel_index as usize >= AP_GSI_CHANNEL_ALLOCATE_ORDER.len()
            || channel_id != AP_GSI_CHANNEL_ALLOCATE_ORDER[self.next_channel_index as usize]
        {
            return Err(ApGsiChannelAllocationError::WrongChannelOrder);
        }
        self.command_issued = true;
        Ok(GsiRegisterWrite {
            offset: SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET,
            value: (SUNFISH_GSI_CHANNEL_ALLOCATE_OPCODE << 24) | u32::from(channel_id),
        })
    }

    pub fn confirm_channel_allocated(
        &mut self,
        reservation: PendingApGsiChannelIds,
        channel_irq_bitmap: u32,
        context_state: u8,
    ) -> Result<Option<ApGsiChannelIds>, ApGsiChannelAllocationError> {
        self.check_reservation(reservation)?;
        if !self.command_issued
            || self.next_channel_index as usize >= AP_GSI_CHANNEL_ALLOCATE_ORDER.len()
        {
            return Err(ApGsiChannelAllocationError::CompletionNotIssued);
        }
        let channel_id = AP_GSI_CHANNEL_ALLOCATE_ORDER[self.next_channel_index as usize];
        let expected_bitmap = 1u32 << channel_id;
        if channel_irq_bitmap == 0 {
            return Err(ApGsiChannelAllocationError::CompletionNotReady);
        }
        if channel_irq_bitmap != expected_bitmap {
            self.quarantine(reservation);
            return Err(ApGsiChannelAllocationError::UnexpectedChannelBitmap {
                expected: expected_bitmap,
                observed: channel_irq_bitmap,
            });
        }
        if context_state != 1 {
            self.quarantine(reservation);
            return Err(ApGsiChannelAllocationError::UnexpectedChannelState {
                channel_id,
                state: context_state,
            });
        }
        self.command_issued = false;
        self.next_channel_index += 1;
        if self.next_channel_index as usize == AP_GSI_CHANNEL_ALLOCATE_ORDER.len() {
            self.pending = None;
            return Ok(Some(ApGsiChannelIds {
                allocated_mask: channel_mask(AP_GSI_CHANNEL_ALLOCATE_ORDER),
                generation: reservation.generation,
            }));
        }
        Ok(None)
    }

    pub fn quarantine_pending(
        &mut self,
        reservation: PendingApGsiChannelIds,
    ) -> Result<(), ApGsiChannelAllocationError> {
        self.check_reservation(reservation)?;
        self.quarantine(reservation);
        Ok(())
    }

    pub fn cancel_before_io(
        &mut self,
        reservation: PendingApGsiChannelIds,
    ) -> Result<(), ApGsiChannelAllocationError> {
        self.check_reservation(reservation)?;
        if self.command_issued || self.next_channel_index != 0 {
            return Err(ApGsiChannelAllocationError::CannotCancelAfterHardwareAccess);
        }
        self.occupied_mask &= !channel_mask(AP_GSI_CHANNEL_ALLOCATE_ORDER);
        self.pending = None;
        Ok(())
    }

    pub const fn occupied_mask(&self) -> u32 {
        self.occupied_mask
    }

    pub const fn quarantined_mask(&self) -> u32 {
        self.quarantined_mask
    }

    fn check_reservation(
        &self,
        reservation: PendingApGsiChannelIds,
    ) -> Result<(), ApGsiChannelAllocationError> {
        if self.pending == Some(reservation) {
            Ok(())
        } else {
            Err(ApGsiChannelAllocationError::StaleReservation)
        }
    }

    fn quarantine(&mut self, reservation: PendingApGsiChannelIds) {
        self.quarantined_mask |= channel_mask(AP_GSI_CHANNEL_ALLOCATE_ORDER);
        self.pending = None;
        self.command_issued = false;
        let _ = reservation;
    }
}

const fn channel_capacity_mask(capacity: ApGsiChannelCapacity) -> u32 {
    (1u32 << capacity.count()) - 1
}

const fn channel_mask(channels: [u8; 10]) -> u32 {
    let mut mask = 0;
    let mut index = 0;
    while index < channels.len() {
        mask |= 1u32 << channels[index];
        index += 1;
    }
    mask
}

#[derive(Debug, Eq, PartialEq)]
pub struct ApGsiContextPlan {
    writes: [GsiRegisterWrite; AP_GSI_CONTEXT_WRITE_COUNT],
    event_ring_primes: [ApGsiEventRingPrime; AP_SYSTEM_PIPE_COUNT],
    channel_starts: [ApGsiChannelStart; AP_SYSTEM_PIPE_COUNT],
}

impl ApGsiContextPlan {
    pub const fn writes(&self) -> &[GsiRegisterWrite; AP_GSI_CONTEXT_WRITE_COUNT] {
        &self.writes
    }

    pub const fn channel_starts(&self) -> &[ApGsiChannelStart; AP_SYSTEM_PIPE_COUNT] {
        &self.channel_starts
    }

    pub const fn event_ring_primes(&self) -> &[ApGsiEventRingPrime; AP_SYSTEM_PIPE_COUNT] {
        &self.event_ring_primes
    }
}

/// The pinned driver primes a GPI event ring at its final element, writing
/// the 64-bit doorbell pointer as MSB then LSB after installing its context.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApGsiEventRingPrime {
    event_id: u8,
    initial_write_pointer: u64,
}

impl ApGsiEventRingPrime {
    pub const fn event_id(self) -> u8 {
        self.event_id
    }

    pub const fn initial_write_pointer(self) -> u64 {
        self.initial_write_pointer
    }
}

/// The source driver's initial channel write pointer is the transfer-ring
/// base. This plan is minted alongside the context writes and only becomes
/// executable after every context write has returned success.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApGsiChannelStart {
    channel_id: u8,
    initial_write_pointer: u64,
}

impl ApGsiChannelStart {
    pub const fn channel_id(self) -> u8 {
        self.channel_id
    }

    pub const fn initial_write_pointer(self) -> u64 {
        self.initial_write_pointer
    }
}

/// Encode the source-matched GSI v2.2 GPI event/channel context writes for
/// IPA's three Apps system pipes. Event IDs are supplied by the caller because
/// Linux allocates them dynamically. `event_capacity` must come from the AP
/// `GSI_V2_2_EE_n_GSI_HW_PARAM_2` readback; IDs must be below its live count.
/// The capacity type enforces the driver's static 24-ring limit. This is a
/// pure plan: it neither accesses MMIO nor allocates memory, issues GSI
/// commands, enables interrupts, or grants DMA authority.
///
/// Source: LineageOS kernel commit
/// `28f9290ae067d3e7857126eb6312720dbe9c7c80`,
/// `drivers/platform/msm/gsi/gsi.c` (`gsi_program_evt_ring_ctx`,
/// `gsi_program_chan_ctx`, `gsi_program_chan_ctx_qos`) and
/// `drivers/platform/msm/ipa/ipa_v3/ipa_dp.c` (Apps system-ring properties).
pub fn plan_ap_gsi_context_writes(
    ring_plan: &DmaRingPlan,
    event_ids: ApEventRingIds,
    channel_ids: ApGsiChannelIds,
    gsi_version: u32,
    event_capacity: ApEventRingCapacity,
) -> Result<ApGsiContextPlan, GsiContextPlanError> {
    if gsi_version != SUNFISH_GSI_VERSION_2_2 {
        return Err(GsiContextPlanError::UnsupportedVersion);
    }
    // The event-index field is five bits wide, but IDs are further limited by
    // both the driver's fixed array size and this device's reported capacity.
    if [
        event_ids.common,
        event_ids.apps_command,
        event_ids.apps_lan_receive,
    ]
    .iter()
    .any(|id| *id >= event_capacity.count())
    {
        return Err(GsiContextPlanError::EventIdOutOfRange);
    }
    if event_ids.common == event_ids.apps_command
        || event_ids.common == event_ids.apps_lan_receive
        || event_ids.apps_command == event_ids.apps_lan_receive
    {
        return Err(GsiContextPlanError::DuplicateEventId);
    }

    let mut writes = [GsiRegisterWrite::default(); AP_GSI_CONTEXT_WRITE_COUNT];
    let mut channel_starts = [ApGsiChannelStart::default(); AP_SYSTEM_PIPE_COUNT];
    let mut count = 0;
    let common = ring_region(ring_plan, RingKind::CommonEvent)?;
    let command_event = ring_region(ring_plan, RingKind::AppsCommandEvent)?;
    let lan_receive_event = ring_region(ring_plan, RingKind::AppsLanReceiveEvent)?;
    append_event_context(&mut writes, &mut count, event_ids.common, common, 0, 1)?;
    // `ipa3_setup_apps_pipes` zeroes each `sys_in`, so these are the non-NAPI
    // moderation settings selected by `ipa_gsi_setup_event_ring`.
    append_event_context(
        &mut writes,
        &mut count,
        event_ids.apps_command,
        command_event,
        16,
        1,
    )?;
    append_event_context(
        &mut writes,
        &mut count,
        event_ids.apps_lan_receive,
        lan_receive_event,
        16,
        1,
    )?;
    let event_ring_primes = [
        event_ring_prime(event_ids.common, common)?,
        event_ring_prime(event_ids.apps_command, command_event)?,
        event_ring_prime(event_ids.apps_lan_receive, lan_receive_event)?,
    ];

    for (index, pipe) in AP_SYSTEM_PIPES.into_iter().enumerate() {
        let (channel_ring, event_id, direction) = match pipe.client {
            Client::AppsCommandProducer => (
                ring_region(ring_plan, RingKind::AppsCommandTransfer)?,
                event_ids.apps_command,
                1,
            ),
            Client::AppsLanConsumer => (
                ring_region(ring_plan, RingKind::AppsLanReceiveTransfer)?,
                event_ids.apps_lan_receive,
                0,
            ),
            Client::AppsLanProducer => (
                ring_region(ring_plan, RingKind::AppsLanTransmitTransfer)?,
                event_ids.common,
                1,
            ),
            _ => return Err(GsiContextPlanError::MissingRing),
        };
        if channel_ring.ring_bytes % 16 != 0
            || channel_ring.base_iova % u64::from(channel_ring.base_alignment) != 0
        {
            return Err(GsiContextPlanError::InvalidRingGeometry);
        }
        let channel_id = endpoint(pipe.client).gsi_channel;
        if !channel_ids.contains(channel_id) {
            return Err(GsiContextPlanError::ChannelNotAllocated { channel_id });
        }
        append_channel_context(
            &mut writes,
            &mut count,
            channel_id,
            event_id,
            direction,
            channel_ring,
            pipe.client,
        )?;
        channel_starts[index] = ApGsiChannelStart {
            channel_id,
            initial_write_pointer: channel_ring.base_iova,
        };
    }

    if count != AP_GSI_CONTEXT_WRITE_COUNT {
        return Err(GsiContextPlanError::WriteCountInvariant);
    }
    Ok(ApGsiContextPlan {
        writes,
        event_ring_primes,
        channel_starts,
    })
}

fn event_ring_prime(
    event_id: u8,
    ring: RingRegion,
) -> Result<ApGsiEventRingPrime, GsiContextPlanError> {
    let last_element_offset = ring
        .ring_bytes
        .checked_sub(16)
        .ok_or(GsiContextPlanError::InvalidRingGeometry)?;
    let initial_write_pointer = ring
        .base_iova
        .checked_add(u64::from(last_element_offset))
        .ok_or(GsiContextPlanError::InvalidRingGeometry)?;
    Ok(ApGsiEventRingPrime {
        event_id,
        initial_write_pointer,
    })
}

fn ring_region(plan: &DmaRingPlan, kind: RingKind) -> Result<RingRegion, GsiContextPlanError> {
    plan.regions()
        .iter()
        .find(|region| region.kind == kind)
        .copied()
        .ok_or(GsiContextPlanError::MissingRing)
}

fn append_write(
    writes: &mut [GsiRegisterWrite; AP_GSI_CONTEXT_WRITE_COUNT],
    count: &mut usize,
    offset: u32,
    value: u32,
) -> Result<(), GsiContextPlanError> {
    let slot = writes
        .get_mut(*count)
        .ok_or(GsiContextPlanError::WriteCountInvariant)?;
    *slot = GsiRegisterWrite { offset, value };
    *count += 1;
    Ok(())
}

fn event_context_offset(event_id: u8, word: u32) -> u32 {
    GSI_EVENT_CONTEXT_BASE
        + GSI_EE_STRIDE * u32::from(AP_EXECUTION_ENVIRONMENT)
        + GSI_CONTEXT_STRIDE * u32::from(event_id)
        + 4 * word
}

fn channel_context_offset(channel_id: u8, word: u32) -> u32 {
    GSI_CHANNEL_CONTEXT_BASE
        + GSI_EE_STRIDE * u32::from(AP_EXECUTION_ENVIRONMENT)
        + GSI_CONTEXT_STRIDE * u32::from(channel_id)
        + 4 * word
}

fn append_event_context(
    writes: &mut [GsiRegisterWrite; AP_GSI_CONTEXT_WRITE_COUNT],
    count: &mut usize,
    event_id: u8,
    ring: RingRegion,
    moderation_ticks: u16,
    moderation_count: u8,
) -> Result<(), GsiContextPlanError> {
    if ring.ring_bytes > u32::from(u16::MAX)
        || ring.ring_bytes < 16
        || ring.ring_bytes % 16 != 0
        || ring.base_iova % u64::from(ring.base_alignment) != 0
    {
        return Err(GsiContextPlanError::InvalidRingGeometry);
    }
    let context0 = (2u32) // GSI_EVT_CHTYPE_GPI_EV
        | (1u32 << GSI_EVENT_INTERRUPT_TYPE_SHIFT) // GSI_INTR_IRQ
        | (16u32 << GSI_ELEMENT_SIZE_SHIFT); // GSI_EVT_RING_RE_SIZE_16B
    let context8 = u32::from(moderation_ticks)
        | (u32::from(moderation_count) << GSI_EVENT_MODERATION_COUNT_SHIFT);
    for (word, value) in [
        (0, context0),
        (1, ring.ring_bytes),
        (2, ring.base_iova as u32),
        (3, (ring.base_iova >> 32) as u32),
        (8, context8),
        (9, 0), // IRQ mode: intvec/MSI fields are zero-initialized by Linux.
        (10, 0),
        (11, 0),
        (12, 0), // no event read-pointer update address
        (13, 0),
    ] {
        append_write(writes, count, event_context_offset(event_id, word), value)?;
    }
    Ok(())
}

fn append_channel_context(
    writes: &mut [GsiRegisterWrite; AP_GSI_CONTEXT_WRITE_COUNT],
    count: &mut usize,
    channel_id: u8,
    event_id: u8,
    direction: u32,
    ring: RingRegion,
    client: Client,
) -> Result<(), GsiContextPlanError> {
    if channel_id >= 32
        || ring.ring_bytes > u32::from(u16::MAX)
        || ring.ring_bytes < 16
        || ring.ring_bytes % 16 != 0
        || ring.base_iova % u64::from(ring.base_alignment) != 0
    {
        return Err(GsiContextPlanError::InvalidRingGeometry);
    }
    let context0 = (2u32) // GSI_CHAN_PROT_GPI
        | (direction << GSI_CHANNEL_DIR_SHIFT)
        | (u32::from(event_id) << GSI_CHANNEL_ERINDEX_SHIFT)
        | (16u32 << GSI_ELEMENT_SIZE_SHIFT); // GSI_CHAN_RE_SIZE_16B
    let low_weight = if client == Client::AppsCommandProducer {
        15u32
    } else {
        1u32
    };
    let use_escape_buffer_only =
        u32::from(endpoint(client).prefetch_mode == PrefetchMode::EscapeBufferOnly);
    // GSI 2.2 uses the v2.0 QoS layout; v2.5-only fields are absent.
    let qos = low_weight | (use_escape_buffer_only << GSI_CHANNEL_QOS_PREFETCH_SHIFT);
    for (word, value) in [
        (0, context0),
        (1, ring.ring_bytes),
        (2, ring.base_iova as u32),
        (3, (ring.base_iova >> 32) as u32),
    ] {
        append_write(
            writes,
            count,
            channel_context_offset(channel_id, word),
            value,
        )?;
    }
    append_write(
        writes,
        count,
        GSI_CHANNEL_QOS_BASE
            + GSI_EE_STRIDE * u32::from(AP_EXECUTION_ENVIRONMENT)
            + GSI_CONTEXT_STRIDE * u32::from(channel_id),
        qos,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaPlanError {
    PoolBaseNotPageAligned,
    EmptyPool,
    AddressOverflow,
    RingAlignmentOverflow,
    RingTooSmallOrMisaligned,
    PoolTooSmall,
    ChannelRingCrosses4GiB,
    RingPlanInvariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaRingPlan {
    regions: [RingRegion; RING_REGION_COUNT],
    pub common_event_ring_consumed_bytes: u32,
    pub common_event_ring_remaining_bytes: u32,
    pub pool_bytes_reserved: u64,
}

impl DmaRingPlan {
    pub const fn regions(&self) -> &[RingRegion; RING_REGION_COUNT] {
        &self.regions
    }
}

/// Plan a fixed IOVA layout for the common GSI event ring and the three AP
/// system pipes. The matched Linux driver allocates the common event ring
/// first, then a private event ring and transfer ring for CMD and LAN RX; LAN
/// TX shares the common event ring. The plan uses static ring lengths and
/// GSI's power-of-two base alignment, rounds mappings to pages, checks the
/// GSI channel 4-GiB boundary rule, and rejects any pool that cannot contain
/// the whole layout. It grants no mapping and touches no device state.
pub fn plan_ap_dma_rings(pool: DmaPool) -> Result<DmaRingPlan, DmaPlanError> {
    if pool.bytes == 0 {
        return Err(DmaPlanError::EmptyPool);
    }
    if pool.base_iova & u64::from(PAGE_BYTES - 1) != 0 {
        return Err(DmaPlanError::PoolBaseNotPageAligned);
    }
    let pool_end = pool
        .base_iova
        .checked_add(pool.bytes)
        .ok_or(DmaPlanError::AddressOverflow)?;

    let mut regions = [RingRegion {
        kind: RingKind::CommonEvent,
        base_iova: 0,
        ring_bytes: 0,
        mapped_bytes: 0,
        base_alignment: 0,
    }; RING_REGION_COUNT];
    let mut cursor = pool.base_iova;
    let mut index = 0;

    let mut append = |kind: RingKind, ring_bytes: u32, channel_ring: bool| {
        if index >= RING_REGION_COUNT {
            return Err(DmaPlanError::RingPlanInvariant);
        }
        if ring_bytes < 16 || ring_bytes % 16 != 0 {
            return Err(DmaPlanError::RingTooSmallOrMisaligned);
        }
        let base_alignment = ring_bytes
            .checked_next_power_of_two()
            .ok_or(DmaPlanError::RingAlignmentOverflow)?;
        let base =
            align_up(cursor, u64::from(base_alignment)).ok_or(DmaPlanError::AddressOverflow)?;
        let mapped_bytes = align_up(u64::from(ring_bytes), u64::from(PAGE_BYTES))
            .ok_or(DmaPlanError::AddressOverflow)?;
        let end = base
            .checked_add(mapped_bytes)
            .ok_or(DmaPlanError::AddressOverflow)?;
        if base < pool.base_iova || end > pool_end {
            return Err(DmaPlanError::PoolTooSmall);
        }
        let ring_end = base
            .checked_add(u64::from(ring_bytes) - 1)
            .ok_or(DmaPlanError::AddressOverflow)?;
        if channel_ring && base >> 32 != ring_end >> 32 {
            return Err(DmaPlanError::ChannelRingCrosses4GiB);
        }
        regions[index] = RingRegion {
            kind,
            base_iova: base,
            ring_bytes,
            mapped_bytes: u32::try_from(mapped_bytes).map_err(|_| DmaPlanError::AddressOverflow)?,
            base_alignment,
        };
        index += 1;
        cursor = end;
        Ok(())
    };

    append(RingKind::CommonEvent, COMMON_EVENT_RING_BYTES, false)?;

    let mut common_event_ring_consumed_bytes = 0u32;
    for pipe in AP_SYSTEM_PIPES {
        let ring_bytes =
            transfer_ring_bytes(pipe.descriptor_fifo_bytes).ok_or(DmaPlanError::AddressOverflow)?;
        if pipe.event_ring_policy == EventRingPolicy::Private {
            let event_kind = match pipe.client {
                Client::AppsCommandProducer => RingKind::AppsCommandEvent,
                Client::AppsLanConsumer => RingKind::AppsLanReceiveEvent,
                _ => return Err(DmaPlanError::RingTooSmallOrMisaligned),
            };
            append(event_kind, ring_bytes, false)?;
        } else {
            common_event_ring_consumed_bytes = common_event_ring_consumed_bytes
                .checked_add(ring_bytes)
                .ok_or(DmaPlanError::AddressOverflow)?;
        }

        let transfer_kind = match pipe.client {
            Client::AppsCommandProducer => RingKind::AppsCommandTransfer,
            Client::AppsLanConsumer => RingKind::AppsLanReceiveTransfer,
            Client::AppsLanProducer => RingKind::AppsLanTransmitTransfer,
            _ => return Err(DmaPlanError::RingTooSmallOrMisaligned),
        };
        append(transfer_kind, ring_bytes, true)?;
    }
    if index != RING_REGION_COUNT || common_event_ring_consumed_bytes > COMMON_EVENT_RING_BYTES {
        return Err(DmaPlanError::RingPlanInvariant);
    }

    Ok(DmaRingPlan {
        regions,
        common_event_ring_consumed_bytes,
        common_event_ring_remaining_bytes: COMMON_EVENT_RING_BYTES
            - common_event_ring_consumed_bytes,
        pool_bytes_reserved: cursor - pool.base_iova,
    })
}

/// IOVA interval that the selected translated SMMU context will cover.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaIovaWindow {
    pub base_iova: u64,
    pub bytes: u64,
}

/// One 4 KiB translation needed by an IPA/GSI ring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaPageMapping {
    pub ring: RingKind,
    pub iova_page: u64,
    pub physical_page: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaPageMapError {
    InvalidIovaWindow,
    IovaOutsideWindow,
    InvalidPhysicalAddressWidth,
    WrongPhysicalPageCount,
    PhysicalPageZero,
    PhysicalPageNotAligned,
    PhysicalPageOutOfRange,
    DuplicateIovaPage,
    DuplicatePhysicalPage,
    RingRegionInvalid,
    PageMapInvariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaPageMapPlan {
    pages: [DmaPageMapping; AP_DMA_PAGE_MAPPING_COUNT],
}

impl DmaPageMapPlan {
    pub const fn pages(&self) -> &[DmaPageMapping; AP_DMA_PAGE_MAPPING_COUNT] {
        &self.pages
    }
}

/// Plan page-granular translations for the six source-matched GSI ring
/// regions. The owner supplies distinct physical pages and the IOVA interval
/// configured in the translated IPA AP context. The planner verifies complete
/// window coverage, alignment, physical-address width, and aliases; it does
/// not allocate frames, create page tables, install mappings, or assert page
/// ownership.
pub fn plan_ap_dma_page_mappings(
    ring_plan: &DmaRingPlan,
    iova_window: DmaIovaWindow,
    physical_pages: &[u64],
    physical_address_bits: u8,
) -> Result<DmaPageMapPlan, DmaPageMapError> {
    let page_bytes = u64::from(PAGE_BYTES);
    if iova_window.bytes == 0
        || iova_window.base_iova % page_bytes != 0
        || iova_window.bytes % page_bytes != 0
    {
        return Err(DmaPageMapError::InvalidIovaWindow);
    }
    let window_end = iova_window
        .base_iova
        .checked_add(iova_window.bytes)
        .ok_or(DmaPageMapError::InvalidIovaWindow)?;
    if !(32..=64).contains(&physical_address_bits) {
        return Err(DmaPageMapError::InvalidPhysicalAddressWidth);
    }
    let max_physical_address = if physical_address_bits == 64 {
        u64::MAX
    } else {
        (1u64 << physical_address_bits) - 1
    };

    if physical_pages.len() != AP_DMA_PAGE_MAPPING_COUNT {
        return Err(DmaPageMapError::WrongPhysicalPageCount);
    }
    let mut pages = [DmaPageMapping {
        ring: RingKind::CommonEvent,
        iova_page: 0,
        physical_page: 0,
    }; AP_DMA_PAGE_MAPPING_COUNT];
    let mut count = 0usize;
    for region in ring_plan.regions() {
        if region.base_iova % page_bytes != 0
            || region.mapped_bytes == 0
            || u64::from(region.mapped_bytes) % page_bytes != 0
            || region.ring_bytes == 0
            || region.ring_bytes > region.mapped_bytes
        {
            return Err(DmaPageMapError::RingRegionInvalid);
        }
        let region_end = region
            .base_iova
            .checked_add(u64::from(region.mapped_bytes))
            .ok_or(DmaPageMapError::RingRegionInvalid)?;
        if region.base_iova < iova_window.base_iova || region_end > window_end {
            return Err(DmaPageMapError::IovaOutsideWindow);
        }
        let region_pages = u64::from(region.mapped_bytes) / page_bytes;
        for page_index in 0..region_pages {
            let iova_page = region.base_iova + page_index * page_bytes;
            let physical_page = *physical_pages
                .get(count)
                .ok_or(DmaPageMapError::PageMapInvariant)?;
            if physical_page == 0 {
                return Err(DmaPageMapError::PhysicalPageZero);
            }
            if physical_page % page_bytes != 0 {
                return Err(DmaPageMapError::PhysicalPageNotAligned);
            }
            let physical_end = physical_page
                .checked_add(page_bytes - 1)
                .ok_or(DmaPageMapError::PhysicalPageOutOfRange)?;
            if physical_end > max_physical_address {
                return Err(DmaPageMapError::PhysicalPageOutOfRange);
            }
            if pages[..count]
                .iter()
                .any(|previous| previous.iova_page == iova_page)
            {
                return Err(DmaPageMapError::DuplicateIovaPage);
            }
            if pages[..count]
                .iter()
                .any(|previous| previous.physical_page == physical_page)
            {
                return Err(DmaPageMapError::DuplicatePhysicalPage);
            }
            let slot = pages
                .get_mut(count)
                .ok_or(DmaPageMapError::PageMapInvariant)?;
            *slot = DmaPageMapping {
                ring: region.kind,
                iova_page,
                physical_page,
            };
            count += 1;
        }
    }
    if count != AP_DMA_PAGE_MAPPING_COUNT {
        return Err(DmaPageMapError::PageMapInvariant);
    }
    Ok(DmaPageMapPlan { pages })
}

fn align_up(value: u64, alignment: u64) -> Option<u64> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    value
        .checked_add(alignment - 1)
        .map(|rounded| rounded & !(alignment - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sunfish_ipa_type_selects_gsi_22_and_ap_execution_environment() {
        assert_eq!(SUNFISH_IPA_HW_V4_2, 16);
        assert_eq!(SUNFISH_GSI_VERSION_2_2, 5);
        assert_eq!(SUNFISH_GSI_MAX_EVENT_RINGS, 24);
        for client in [
            Client::AppsCommandProducer,
            Client::AppsLanConsumer,
            Client::AppsLanProducer,
            Client::AppsWanConsumer,
            Client::WlanProducer,
            Client::WlanConsumer,
        ] {
            assert_eq!(
                endpoint(client).execution_environment,
                AP_EXECUTION_ENVIRONMENT
            );
        }
    }

    #[test]
    fn ap_system_pipe_order_and_static_endpoint_assignments_match_lineage() {
        assert_eq!(
            AP_SYSTEM_PIPES.map(|pipe| pipe.client),
            [
                Client::AppsCommandProducer,
                Client::AppsLanConsumer,
                Client::AppsLanProducer,
            ]
        );
        assert_eq!(
            endpoint(Client::AppsCommandProducer),
            Endpoint {
                ipa_endpoint: 6,
                gsi_channel: 1,
                ipa_if_tlv: 20,
                ipa_if_aos: 20,
                execution_environment: 0,
                prefetch_mode: PrefetchMode::UsePrefetchBuffers,
                prefetch_threshold: 0,
            }
        );
        assert_eq!(
            endpoint(Client::AppsLanConsumer),
            Endpoint {
                ipa_endpoint: 8,
                gsi_channel: 2,
                ipa_if_tlv: 6,
                ipa_if_aos: 6,
                execution_environment: 0,
                prefetch_mode: PrefetchMode::EscapeBufferOnly,
                prefetch_threshold: 0,
            }
        );
        assert_eq!(
            endpoint(Client::AppsLanProducer),
            Endpoint {
                ipa_endpoint: 2,
                gsi_channel: 6,
                ipa_if_tlv: 8,
                ipa_if_aos: 9,
                execution_environment: 0,
                prefetch_mode: PrefetchMode::EscapeBufferOnly,
                prefetch_threshold: 0,
            }
        );
        assert_eq!(
            AP_SYSTEM_PIPES.map(|pipe| pipe.descriptor_fifo_bytes),
            [0x800, 0x800, 0x1000]
        );
        assert_eq!(AP_SYSTEM_PIPES[0].mode, PipeMode::Dma);
        assert_eq!(AP_SYSTEM_PIPES[0].destination_endpoint, Some(8));
        assert_eq!(AP_SYSTEM_PIPES[1].mode, PipeMode::Basic);
        assert_eq!(AP_SYSTEM_PIPES[2].mode, PipeMode::Basic);
        assert_eq!(
            AP_SYSTEM_PIPES.map(|pipe| pipe.event_ring_policy),
            [
                EventRingPolicy::Private,
                EventRingPolicy::Private,
                EventRingPolicy::SharedCommon,
            ]
        );
    }

    #[test]
    fn control_and_wlan_endpoint_ids_are_distinct_from_dynamic_event_rings() {
        assert_eq!(
            endpoint(Client::AppsWanConsumer),
            Endpoint {
                ipa_endpoint: 9,
                gsi_channel: 3,
                ipa_if_tlv: 6,
                ipa_if_aos: 6,
                execution_environment: 0,
                prefetch_mode: PrefetchMode::EscapeBufferOnly,
                prefetch_threshold: 0,
            }
        );
        assert_eq!(endpoint(Client::WlanProducer).gsi_channel, 7);
        assert_eq!(endpoint(Client::WlanConsumer).gsi_channel, 8);
        assert_eq!(transfer_ring_bytes(0x800), Some(0x1000));
        assert_eq!(transfer_ring_bytes(0x1000), Some(0x2000));
        assert_eq!(transfer_ring_bytes(u32::MAX), None);
        assert_eq!(COMMON_EVENT_RING_BYTES, 0x7c00);
    }

    #[test]
    fn fixed_iova_plan_matches_linux_event_and_transfer_ring_allocation_order() {
        let plan = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: 0xe000,
        })
        .unwrap();
        assert_eq!(
            plan.regions().map(|region| region.kind),
            [
                RingKind::CommonEvent,
                RingKind::AppsCommandEvent,
                RingKind::AppsCommandTransfer,
                RingKind::AppsLanReceiveEvent,
                RingKind::AppsLanReceiveTransfer,
                RingKind::AppsLanTransmitTransfer,
            ]
        );
        assert_eq!(
            plan.regions().map(|region| region.base_iova),
            [
                0x2000_0000,
                0x2000_8000,
                0x2000_9000,
                0x2000_a000,
                0x2000_b000,
                0x2000_c000,
            ]
        );
        assert_eq!(
            plan.regions().map(|region| region.ring_bytes),
            [0x7c00, 0x1000, 0x1000, 0x1000, 0x1000, 0x2000]
        );
        assert_eq!(
            plan.regions().map(|region| region.mapped_bytes),
            [0x8000, 0x1000, 0x1000, 0x1000, 0x1000, 0x2000]
        );
        assert_eq!(
            plan.regions().map(|region| region.base_alignment),
            [0x8000, 0x1000, 0x1000, 0x1000, 0x1000, 0x2000]
        );
        assert_eq!(plan.common_event_ring_consumed_bytes, 0x2000);
        assert_eq!(plan.common_event_ring_remaining_bytes, 0x5c00);
        assert_eq!(plan.pool_bytes_reserved, 0xe000);
        assert_eq!(LINUX_IPA_AP_IOVA_START, 0x2000_0000);
        assert_eq!(LINUX_IPA_AP_IOVA_BYTES, 0x4000_0000);
        let final_region = plan.regions().last().unwrap();
        assert!(
            final_region.base_iova + u64::from(final_region.mapped_bytes)
                <= LINUX_IPA_AP_IOVA_START + LINUX_IPA_AP_IOVA_BYTES
        );
    }

    #[test]
    fn fixed_iova_plan_rounds_mapping_and_rejects_bad_or_short_pools() {
        let shifted = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START + 0x4000,
            bytes: 0x12000,
        })
        .unwrap();
        assert_eq!(shifted.regions()[0].base_iova, 0x2000_8000);
        assert_eq!(shifted.pool_bytes_reserved, 0x12000);

        assert_eq!(
            plan_ap_dma_rings(DmaPool {
                base_iova: 0x8000_0001,
                bytes: 0x20000,
            }),
            Err(DmaPlanError::PoolBaseNotPageAligned)
        );
        assert_eq!(
            plan_ap_dma_rings(DmaPool {
                base_iova: LINUX_IPA_AP_IOVA_START,
                bytes: 0xdfff,
            }),
            Err(DmaPlanError::PoolTooSmall)
        );
        assert_eq!(
            plan_ap_dma_rings(DmaPool {
                base_iova: u64::MAX & !u64::from(PAGE_BYTES - 1),
                bytes: u64::from(PAGE_BYTES),
            }),
            Err(DmaPlanError::AddressOverflow)
        );
    }

    fn value_at(plan: &ApGsiContextPlan, offset: u32) -> u32 {
        plan.writes()
            .iter()
            .find(|write| write.offset == offset)
            .unwrap_or_else(|| panic!("missing GSI context offset {offset:#x}"))
            .value
    }

    fn event_capacity(count: u8) -> ApEventRingCapacity {
        ApEventRingCapacity::from_v2_2_hw_param_2(
            u32::from(count) << GSI_V2_2_NUM_EVENT_RINGS_SHIFT,
        )
        .unwrap()
    }

    fn allocated_ap_channels() -> ApGsiChannelIds {
        let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
        let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
        let reservation = allocator.reserve_ap_channels().unwrap();
        let mut completed = None;
        for channel_id in AP_GSI_CHANNEL_ALLOCATE_ORDER {
            allocator
                .mark_command_issued(reservation, channel_id)
                .unwrap();
            completed = allocator
                .confirm_channel_allocated(reservation, 1u32 << channel_id, 1)
                .unwrap();
        }
        completed.expect("every AP channel must be confirmed")
    }

    fn complete_ap_event_rings(
        allocator: &mut ApEventRingAllocator,
        reservation: PendingApEventRingIds,
    ) -> ApEventRingIds {
        loop {
            let event_id = allocator
                .next_event_id(reservation)
                .unwrap()
                .expect("reservation must have an event awaiting completion");
            allocator
                .mark_command_issued(reservation, event_id)
                .unwrap();
            if let Some(ids) = allocator
                .confirm_event_allocated(reservation, 1u32 << event_id, 1)
                .unwrap()
            {
                return ids;
            }
        }
    }

    #[test]
    fn event_allocator_reserves_lowest_free_ids_and_requires_ordered_confirmations() {
        let capacity = event_capacity(8);
        let mut allocator = ApEventRingAllocator::new(capacity, 0b0000_0101).unwrap();
        let reservation = allocator.reserve_ap_system_rings().unwrap();
        assert_eq!(allocator.next_event_id(reservation), Ok(Some(1)));
        assert_eq!(
            allocator.reserve_ap_system_rings(),
            Err(EventRingAllocationError::ReservationAlreadyPending)
        );
        assert_eq!(
            allocator.confirm_event_allocated(reservation, 1 << 1, 1),
            Err(EventRingAllocationError::CompletionNotIssued)
        );
        assert_eq!(
            allocator.mark_command_issued(reservation, 3),
            Err(EventRingAllocationError::WrongEventOrder)
        );
        allocator.mark_command_issued(reservation, 1).unwrap();
        assert_eq!(
            allocator.confirm_event_allocated(reservation, 0, 1),
            Err(EventRingAllocationError::CompletionNotReady)
        );
        assert_eq!(
            allocator.confirm_event_allocated(reservation, 1 << 1, 1),
            Ok(None)
        );

        let ids = complete_ap_event_rings(&mut allocator, reservation);
        assert_eq!(ids.common(), 1);
        assert_eq!(ids.apps_command(), 3);
        assert_eq!(ids.apps_lan_receive(), 4);
        assert_eq!(allocator.occupied_mask(), 0b0001_1111);
        assert_eq!(allocator.quarantined_mask(), 0);
        assert_eq!(
            allocator.next_event_id(reservation),
            Err(EventRingAllocationError::StaleReservation)
        );
    }

    #[test]
    fn event_allocator_requires_capacity_and_rejects_out_of_range_occupied_bits() {
        let capacity = event_capacity(3);
        assert_eq!(
            ApEventRingAllocator::new(capacity, 0b1000),
            Err(EventRingAllocationError::InvalidOccupiedMask)
        );
        let mut allocator = ApEventRingAllocator::new(capacity, 0b001).unwrap();
        assert_eq!(
            allocator.reserve_ap_system_rings(),
            Err(EventRingAllocationError::TooFewFreeEventRings)
        );
        assert_eq!(allocator.occupied_mask(), 0b001);
        assert_eq!(allocator.quarantined_mask(), 0);
    }

    #[test]
    fn event_allocator_quarantines_failed_allocations_and_never_reuses_ids() {
        let mut allocator = ApEventRingAllocator::new(event_capacity(8), 0).unwrap();
        let reservation = allocator.reserve_ap_system_rings().unwrap();
        let event_id = allocator.next_event_id(reservation).unwrap().unwrap();
        allocator
            .mark_command_issued(reservation, event_id)
            .unwrap();
        assert_eq!(
            allocator.confirm_event_allocated(reservation, 1 << event_id, 0),
            Err(EventRingAllocationError::UnexpectedEventState { event_id, state: 0 })
        );
        assert_eq!(allocator.occupied_mask(), 0b0000_0111);
        assert_eq!(allocator.quarantined_mask(), 0b0000_0111);
        let replacement = allocator.reserve_ap_system_rings().unwrap();
        assert_eq!(allocator.next_event_id(replacement), Ok(Some(3)));
        allocator.quarantine_pending(replacement).unwrap();
        assert_eq!(allocator.quarantined_mask(), 0b0011_1111);
        assert_eq!(allocator.occupied_mask(), 0b0011_1111);
    }

    #[test]
    fn event_allocator_allows_cancel_only_before_any_command_is_issued() {
        let mut allocator = ApEventRingAllocator::new(event_capacity(8), 0).unwrap();
        let first = allocator.reserve_ap_system_rings().unwrap();
        allocator.cancel_before_io(first).unwrap();
        assert_eq!(allocator.occupied_mask(), 0);
        assert_eq!(
            allocator.next_event_id(first),
            Err(EventRingAllocationError::StaleReservation)
        );

        let second = allocator.reserve_ap_system_rings().unwrap();
        let event_id = allocator.next_event_id(second).unwrap().unwrap();
        allocator.mark_command_issued(second, event_id).unwrap();
        assert_eq!(
            allocator.cancel_before_io(second),
            Err(EventRingAllocationError::CannotCancelAfterHardwareAccess)
        );
        allocator.quarantine_pending(second).unwrap();
        assert_eq!(allocator.occupied_mask(), 0b111);
    }

    #[test]
    fn ap_event_capacity_decodes_v22_hw_param_field_and_driver_limits() {
        assert_eq!(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET, 0x1f040);
        assert_eq!(
            ApEventRingCapacity::from_v2_2_hw_param_2(0xa5a5_1234)
                .unwrap()
                .count(),
            18
        );
        assert_eq!(
            ApEventRingCapacity::from_v2_2_hw_param_2(24 << GSI_V2_2_NUM_EVENT_RINGS_SHIFT)
                .unwrap()
                .count(),
            24
        );
        for count in [0, 25, 31] {
            assert_eq!(
                ApEventRingCapacity::from_v2_2_hw_param_2(count << GSI_V2_2_NUM_EVENT_RINGS_SHIFT),
                Err(GsiContextPlanError::InvalidEventRingCount)
            );
        }
    }

    #[test]
    fn ap_channel_capacity_and_allocator_match_the_v22_source_order() {
        assert_eq!(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET, 0x1f040);
        assert_eq!(
            ApGsiChannelCapacity::from_v2_2_hw_param_2(0xa5a5_1234)
                .unwrap()
                .count(),
            6
        );
        assert_eq!(
            ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << GSI_V2_2_NUM_CHANNELS_SHIFT)
                .unwrap()
                .count(),
            31
        );
        assert_eq!(
            ApGsiChannelCapacity::from_v2_2_hw_param_2(0),
            Err(GsiContextPlanError::InvalidChannelCount)
        );

        let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
        let occupied = 1 << 8;
        let mut busy_allocator = ApGsiChannelAllocator::new(capacity, occupied).unwrap();
        assert_eq!(
            busy_allocator.reserve_ap_channels(),
            Err(ApGsiChannelAllocationError::ChannelAlreadyOccupied { channel_id: 8 })
        );

        let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
        let reservation = allocator.reserve_ap_channels().unwrap();
        let mut proof = None;
        for expected in AP_GSI_CHANNEL_ALLOCATE_ORDER {
            assert_eq!(allocator.next_channel_id(reservation), Ok(Some(expected)));
            allocator
                .mark_command_issued(reservation, expected)
                .unwrap();
            proof = allocator
                .confirm_channel_allocated(reservation, 1u32 << expected, 1)
                .unwrap();
        }
        let proof = proof.unwrap();
        assert_eq!(
            proof.allocated_mask(),
            channel_mask(AP_GSI_CHANNEL_ALLOCATE_ORDER)
        );
        assert!(proof.contains(1));
        assert!(proof.contains(2));
        assert!(proof.contains(6));
        assert_eq!(proof.generation(), reservation.generation());
        assert_eq!(
            allocator.next_channel_id(reservation),
            Err(ApGsiChannelAllocationError::StaleReservation)
        );
    }

    #[test]
    fn ap_channel_allocator_quarantines_bad_completion_and_only_cancels_before_io() {
        let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
        let all_channels = channel_mask(AP_GSI_CHANNEL_ALLOCATE_ORDER);
        let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
        let reservation = allocator.reserve_ap_channels().unwrap();
        assert_eq!(
            allocator.confirm_channel_allocated(reservation, 1 << 5, 1),
            Err(ApGsiChannelAllocationError::CompletionNotIssued)
        );
        allocator.cancel_before_io(reservation).unwrap();
        assert_eq!(allocator.occupied_mask(), 0);

        let reservation = allocator.reserve_ap_channels().unwrap();
        allocator.mark_command_issued(reservation, 5).unwrap();
        assert_eq!(
            allocator.confirm_channel_allocated(reservation, 1 << 0, 1),
            Err(ApGsiChannelAllocationError::UnexpectedChannelBitmap {
                expected: 1 << 5,
                observed: 1 << 0,
            })
        );
        assert_eq!(allocator.quarantined_mask(), all_channels);
        assert_eq!(allocator.occupied_mask(), all_channels);
        assert_eq!(
            allocator.next_channel_id(reservation),
            Err(ApGsiChannelAllocationError::StaleReservation)
        );
    }

    #[test]
    fn gsi_v22_context_words_match_linux_ap_event_and_channel_properties() {
        let rings = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: 0xe000,
        })
        .unwrap();
        let plan = plan_ap_gsi_context_writes(
            &rings,
            ApEventRingIds {
                common: 0,
                apps_command: 4,
                apps_lan_receive: 5,
            },
            allocated_ap_channels(),
            SUNFISH_GSI_VERSION_2_2,
            event_capacity(24),
        )
        .unwrap();
        assert_eq!(plan.writes().len(), AP_GSI_CONTEXT_WRITE_COUNT);
        assert_eq!(
            plan.event_ring_primes()
                .iter()
                .map(|ring| (ring.event_id(), ring.initial_write_pointer()))
                .collect::<std::vec::Vec<_>>(),
            [(0, 0x2000_7bf0), (4, 0x2000_8ff0), (5, 0x2000_aff0),]
        );
        assert_eq!(
            plan.channel_starts()
                .iter()
                .map(|channel| (channel.channel_id(), channel.initial_write_pointer()))
                .collect::<std::vec::Vec<_>>(),
            [(1, 0x2000_9000), (2, 0x2000_b000), (6, 0x2000_c000),]
        );

        // Event context: GPI event, IRQ, 16-byte elements, and source-matched
        // ring length/address/moderation values.
        assert_eq!(value_at(&plan, 0x1_0000), 0x1001_0002);
        assert_eq!(value_at(&plan, 0x1_0004), 0x7c00);
        assert_eq!(value_at(&plan, 0x1_0008), 0x2000_0000);
        assert_eq!(value_at(&plan, 0x1_0020), 0x0001_0000);
        assert_eq!(value_at(&plan, 0x1_0200), 0x1001_0002);
        assert_eq!(value_at(&plan, 0x1_0204), 0x1000);
        assert_eq!(value_at(&plan, 0x1_0208), 0x2000_8000);
        assert_eq!(value_at(&plan, 0x1_0220), 0x0001_0010);
        assert_eq!(value_at(&plan, 0x1_0288), 0x2000_a000);

        // Channel context: protocol GPI, direction, event index, ring IOVA,
        // direct doorbell mode and source-matched channel weights/prefetch.
        assert_eq!(value_at(&plan, 0xf080), 0x1001_000a); // CMD, channel 1, event 4
        assert_eq!(value_at(&plan, 0xf084), 0x1000);
        assert_eq!(value_at(&plan, 0xf088), 0x2000_9000);
        assert_eq!(value_at(&plan, 0x1_c0dc), 15);
        assert_eq!(value_at(&plan, 0xf100), 0x1001_4002); // LAN RX, channel 2, event 5
        assert_eq!(value_at(&plan, 0x1_c15c), 0x401);
        assert_eq!(value_at(&plan, 0xf300), 0x1000_000a); // LAN TX shares event 0
        assert_eq!(value_at(&plan, 0xf308), 0x2000_c000);
        assert_eq!(value_at(&plan, 0x1_c35c), 0x401);
    }

    #[test]
    fn gsi_context_plan_rejects_wrong_version_and_invalid_dynamic_event_ids() {
        let rings = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: 0xe000,
        })
        .unwrap();
        let ids = ApEventRingIds {
            common: 0,
            apps_command: 4,
            apps_lan_receive: 5,
        };
        assert_eq!(
            plan_ap_gsi_context_writes(&rings, ids, allocated_ap_channels(), 4, event_capacity(24)),
            Err(GsiContextPlanError::UnsupportedVersion)
        );
        assert_eq!(
            plan_ap_gsi_context_writes(
                &rings,
                ApEventRingIds {
                    common: 0,
                    apps_command: 32,
                    apps_lan_receive: 5,
                },
                allocated_ap_channels(),
                SUNFISH_GSI_VERSION_2_2,
                event_capacity(24),
            ),
            Err(GsiContextPlanError::EventIdOutOfRange)
        );
        for event_id in [24, 31] {
            assert_eq!(
                plan_ap_gsi_context_writes(
                    &rings,
                    ApEventRingIds {
                        common: 0,
                        apps_command: event_id,
                        apps_lan_receive: 5,
                    },
                    allocated_ap_channels(),
                    SUNFISH_GSI_VERSION_2_2,
                    event_capacity(24),
                ),
                Err(GsiContextPlanError::EventIdOutOfRange)
            );
        }
        assert_eq!(
            plan_ap_gsi_context_writes(
                &rings,
                ApEventRingIds {
                    common: 0,
                    apps_command: 1,
                    apps_lan_receive: 2,
                },
                allocated_ap_channels(),
                SUNFISH_GSI_VERSION_2_2,
                event_capacity(3),
            )
            .unwrap()
            .writes()
            .len(),
            AP_GSI_CONTEXT_WRITE_COUNT
        );
        assert_eq!(
            plan_ap_gsi_context_writes(
                &rings,
                ApEventRingIds {
                    common: 0,
                    apps_command: 1,
                    apps_lan_receive: 3,
                },
                allocated_ap_channels(),
                SUNFISH_GSI_VERSION_2_2,
                event_capacity(3),
            ),
            Err(GsiContextPlanError::EventIdOutOfRange)
        );
        assert_eq!(
            plan_ap_gsi_context_writes(
                &rings,
                ApEventRingIds {
                    common: 0,
                    apps_command: 4,
                    apps_lan_receive: 4,
                },
                allocated_ap_channels(),
                SUNFISH_GSI_VERSION_2_2,
                event_capacity(24),
            ),
            Err(GsiContextPlanError::DuplicateEventId)
        );
    }

    #[test]
    fn ipa_ring_page_map_covers_exact_iovas_with_distinct_physical_pages() {
        let rings = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: 0xe000,
        })
        .unwrap();
        let physical_pages: [u64; AP_DMA_PAGE_MAPPING_COUNT] =
            core::array::from_fn(|index| 0x9000_0000 + u64::try_from(index).unwrap() * 0x2000);
        let plan = plan_ap_dma_page_mappings(
            &rings,
            DmaIovaWindow {
                base_iova: LINUX_IPA_AP_IOVA_START,
                bytes: LINUX_IPA_AP_IOVA_BYTES,
            },
            &physical_pages,
            40,
        )
        .unwrap();
        assert_eq!(plan.pages().len(), AP_DMA_PAGE_MAPPING_COUNT);
        assert_eq!(
            core::array::from_fn(|index| plan.pages()[index].iova_page),
            [
                0x2000_0000,
                0x2000_1000,
                0x2000_2000,
                0x2000_3000,
                0x2000_4000,
                0x2000_5000,
                0x2000_6000,
                0x2000_7000,
                0x2000_8000,
                0x2000_9000,
                0x2000_a000,
                0x2000_b000,
                0x2000_c000,
                0x2000_d000,
            ]
        );
        assert_eq!(
            core::array::from_fn(|index| plan.pages()[index].physical_page),
            physical_pages
        );
        assert_eq!(
            core::array::from_fn(|index| plan.pages()[index].ring),
            [
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::CommonEvent,
                RingKind::AppsCommandEvent,
                RingKind::AppsCommandTransfer,
                RingKind::AppsLanReceiveEvent,
                RingKind::AppsLanReceiveTransfer,
                RingKind::AppsLanTransmitTransfer,
                RingKind::AppsLanTransmitTransfer,
            ]
        );
    }

    #[test]
    fn ipa_ring_page_map_rejects_bad_windows_pages_and_address_widths() {
        let rings = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: 0xe000,
        })
        .unwrap();
        let physical_pages: [u64; AP_DMA_PAGE_MAPPING_COUNT] =
            core::array::from_fn(|index| 0x9000_0000 + u64::try_from(index).unwrap() * 0x2000);
        let full_window = DmaIovaWindow {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: LINUX_IPA_AP_IOVA_BYTES,
        };
        assert_eq!(
            plan_ap_dma_page_mappings(
                &rings,
                DmaIovaWindow {
                    base_iova: LINUX_IPA_AP_IOVA_START,
                    bytes: 0xd000,
                },
                &physical_pages,
                40,
            ),
            Err(DmaPageMapError::IovaOutsideWindow)
        );
        assert_eq!(
            plan_ap_dma_page_mappings(&rings, full_window, &physical_pages[..13], 40,),
            Err(DmaPageMapError::WrongPhysicalPageCount)
        );
        let mut zero_page = physical_pages;
        zero_page[0] = 0;
        assert_eq!(
            plan_ap_dma_page_mappings(&rings, full_window, &zero_page, 40),
            Err(DmaPageMapError::PhysicalPageZero)
        );
        let mut misaligned = physical_pages;
        misaligned[0] += 1;
        assert_eq!(
            plan_ap_dma_page_mappings(&rings, full_window, &misaligned, 40),
            Err(DmaPageMapError::PhysicalPageNotAligned)
        );
        let mut aliases = physical_pages;
        aliases[13] = aliases[0];
        assert_eq!(
            plan_ap_dma_page_mappings(&rings, full_window, &aliases, 40),
            Err(DmaPageMapError::DuplicatePhysicalPage)
        );
        assert_eq!(
            plan_ap_dma_page_mappings(&rings, full_window, &physical_pages, 31),
            Err(DmaPageMapError::InvalidPhysicalAddressWidth)
        );
        let mut over_32_bit_limit = physical_pages;
        over_32_bit_limit[0] = 0x1_0000_0000;
        assert_eq!(
            plan_ap_dma_page_mappings(&rings, full_window, &over_32_bit_limit, 32),
            Err(DmaPageMapError::PhysicalPageOutOfRange)
        );
        assert_eq!(
            plan_ap_dma_page_mappings(
                &rings,
                DmaIovaWindow {
                    base_iova: u64::MAX & !u64::from(PAGE_BYTES - 1),
                    bytes: u64::from(PAGE_BYTES),
                },
                &physical_pages,
                40,
            ),
            Err(DmaPageMapError::InvalidIovaWindow)
        );
    }
}
