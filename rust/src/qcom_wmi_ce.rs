//! Offline composition of a framed HTC/WMI command with a validated CE route
//! and source-ring slot. This module plans bytes only: it has no DMA, MMIO,
//! cache, lifecycle, interrupt, or hardware publication authority.

#![forbid(unsafe_code)]

use crate::qcom_ce_v3::{self, RingCursor, RxDescriptor, SlotTicket, TxDescriptor};
use crate::qcom_ce_v3_profile::{self, Profile, RingAllocation, RingDirection, RingPlan};
use crate::qcom_htc::{
    self, HTC_FRAME_HEADER_SIZE, HTC_LOCAL_ENDPOINT_LIMIT, HTC_WMI_COMMAND_HEADER_SIZE, WmiTxPlan,
};

pub const WMI_CONTROL_SERVICE_ID: u16 = qcom_htc::HTC_WMI_CONTROL_SERVICE_ID;
pub const WMI_CONTROL_SOURCE_CE: u8 = 3;
pub const WMI_CONTROL_DESTINATION_CE: u8 = 2;
pub const HTT_DATA_SERVICE_ID: u16 = 0x0300;
pub const HTT_DATA_SOURCE_CE: u8 = 4;
pub const HTT_DATA_DESTINATION_CE: u8 = 1;
pub const HTC_NEED_CREDIT_UPDATE: u8 = 1;
pub const MAX_POSTED_WMI_RX_BUFFERS: usize = 127;
const WMI_COMMAND_ID_MASK: u32 = 0x00ff_ffff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    HostToTarget,
    TargetToHost,
}

/// One entry from the target-selected service-to-CE table. The runtime owner
/// must supply the selected table; a compile-time default is not attestation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCeMapping {
    pub service_id: u16,
    pub direction: Direction,
    pub engine: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiRoute {
    pub host_to_target_engine: u8,
    pub target_to_host_engine: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaPool {
    base: u64,
    bytes: u64,
}

impl DmaPool {
    pub fn new(base: u64, bytes: u64) -> Result<Self, Error> {
        if base == 0
            || !base.is_multiple_of(qcom_ce_v3_profile::PAGE_BYTES)
            || bytes == 0
            || !bytes.is_multiple_of(qcom_ce_v3_profile::PAGE_BYTES)
        {
            return Err(Error::InvalidDmaPool);
        }
        let end = base.checked_add(bytes).ok_or(Error::AddressOverflow)?;
        if end > qcom_ce_v3::MAX_CE_IOVA {
            return Err(Error::DmaPoolOutsideCeWindow);
        }
        Ok(Self { base, bytes })
    }

    pub const fn base(self) -> u64 {
        self.base
    }

    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    fn contains(self, address: u64, bytes: u64) -> bool {
        let Some(end) = address.checked_add(bytes) else {
            return false;
        };
        let Some(pool_end) = self.base.checked_add(self.bytes) else {
            return false;
        };
        address >= self.base && end <= pool_end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxBufferWindow {
    pub iova: u64,
    pub capacity: u32,
}

/// A key-bearing CE3 source buffer whose bytes must be erased only after the
/// matching descriptor ticket is reported consumed by the hardware read
/// index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SensitiveTxBufferLease {
    generation: u64,
    ticket: SlotTicket,
    buffer: TxBufferWindow,
}

impl SensitiveTxBufferLease {
    pub fn new(generation: u64, ticket: SlotTicket, buffer: TxBufferWindow) -> Result<Self, Error> {
        if generation == 0
            || buffer.iova == 0
            || !buffer
                .iova
                .is_multiple_of(qcom_ce_v3_profile::DMA_ALIGNMENT)
            || buffer.capacity == 0
            || buffer
                .iova
                .checked_add(u64::from(buffer.capacity))
                .is_none()
        {
            return Err(Error::InvalidSensitiveTxBuffer);
        }
        Ok(Self {
            generation,
            ticket,
            buffer,
        })
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn ticket(self) -> SlotTicket {
        self.ticket
    }

    pub const fn buffer(self) -> TxBufferWindow {
        self.buffer
    }
}

/// Fixed-capacity owner for sensitive WMI frames staged in CE3's source DMA
/// pool. Reserve a lease before publishing its descriptor; cancel it only on a
/// definite pre-publication failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SensitiveTxBufferLedger<const N: usize> {
    leases: [Option<SensitiveTxBufferLease>; N],
    count: usize,
}

impl<const N: usize> SensitiveTxBufferLedger<N> {
    pub const fn new() -> Self {
        Self {
            leases: [None; N],
            count: 0,
        }
    }

    pub const fn len(self) -> usize {
        self.count
    }

    pub const fn is_empty(self) -> bool {
        self.count == 0
    }

    pub fn active(&self) -> impl Iterator<Item = SensitiveTxBufferLease> + '_ {
        self.leases[..self.count].iter().flatten().copied()
    }

    pub fn reserve(
        &mut self,
        lease: SensitiveTxBufferLease,
        cursor: RingCursor,
        pool: DmaPool,
    ) -> Result<(), Error> {
        if self.count == N {
            return Err(Error::SensitiveTxLedgerFull);
        }
        let expected_ticket = cursor.prepare_next().map_err(Error::Cursor)?;
        if expected_ticket != lease.ticket {
            return Err(Error::Cursor(qcom_ce_v3::Error::StaleTicket));
        }
        if !pool.contains(lease.buffer.iova, u64::from(lease.buffer.capacity)) {
            return Err(Error::InvalidSensitiveTxBuffer);
        }
        if self.active().any(|active| {
            active.generation == lease.generation && active.ticket == lease.ticket
                || overlaps(
                    active.buffer.iova,
                    u64::from(active.buffer.capacity),
                    lease.buffer.iova,
                    u64::from(lease.buffer.capacity),
                )
        }) {
            return Err(Error::SensitiveTxBufferOverlap);
        }
        self.leases[self.count] = Some(lease);
        self.count += 1;
        Ok(())
    }

    pub fn cancel_unpublished(
        &mut self,
        generation: u64,
        ticket: SlotTicket,
    ) -> Result<SensitiveTxBufferLease, Error> {
        let index = self.leases[..self.count]
            .iter()
            .position(|entry| {
                entry.is_some_and(|lease| lease.generation == generation && lease.ticket == ticket)
            })
            .ok_or(Error::SensitiveTxBufferNotReserved)?;
        let lease = self.leases[index].ok_or(Error::SensitiveTxBufferNotReserved)?;
        self.count -= 1;
        self.leases[index] = self.leases[self.count];
        self.leases[self.count] = None;
        Ok(lease)
    }

    /// Remove leases whose ticket generation is older than the cursor's new
    /// consumer generation after the native owner has scrubbed their buffers.
    #[allow(dead_code)] // Used by the native CE reclaim path, omitted from no-unsafe model builds.
    pub(crate) fn retire_scrubbed(
        &mut self,
        generation: u64,
        consumer_generation: u64,
    ) -> Result<usize, Error> {
        if self.active().any(|lease| lease.generation != generation) {
            return Err(Error::SensitiveTxBufferNotReserved);
        }
        let mut retired = 0;
        let mut index = 0;
        while index < self.count {
            let lease = self.leases[index].ok_or(Error::SensitiveTxBufferNotReserved)?;
            if lease.ticket.generation() < consumer_generation {
                self.count -= 1;
                self.leases[index] = self.leases[self.count];
                self.leases[self.count] = None;
                retired += 1;
            } else {
                index += 1;
            }
        }
        Ok(retired)
    }
}

impl<const N: usize> Default for SensitiveTxBufferLedger<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxBufferWindow {
    pub iova: u64,
    pub capacity: u32,
}

impl RxBufferWindow {
    const EMPTY: Self = Self {
        iova: 0,
        capacity: 0,
    };
}

/// Tracks receive buffers handed to CE2 until a completion is validated and
/// consumed, preventing overlapping leases from entering the receive ring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxBufferLedger {
    buffers: [RxBufferWindow; MAX_POSTED_WMI_RX_BUFFERS],
    tickets: [Option<SlotTicket>; MAX_POSTED_WMI_RX_BUFFERS],
    count: usize,
}

impl RxBufferLedger {
    pub const fn new() -> Self {
        Self {
            buffers: [RxBufferWindow::EMPTY; MAX_POSTED_WMI_RX_BUFFERS],
            tickets: [None; MAX_POSTED_WMI_RX_BUFFERS],
            count: 0,
        }
    }

    pub const fn len(self) -> usize {
        self.count
    }

    pub const fn is_empty(self) -> bool {
        self.count == 0
    }

    pub fn active(&self) -> &[RxBufferWindow] {
        &self.buffers[..self.count]
    }

    #[allow(dead_code)]
    pub(crate) fn window_for(&self, ticket: SlotTicket) -> Result<RxBufferWindow, Error> {
        self.tickets[..self.count]
            .iter()
            .position(|active| *active == Some(ticket))
            .map(|index| self.buffers[index])
            .ok_or(Error::ReceiveBufferNotReserved)
    }

    pub(crate) fn preview_reserve(&self, window: RxBufferWindow) -> Result<(), Error> {
        if self.count == self.buffers.len() {
            return Err(Error::ReceiveLedgerFull);
        }
        if self.active().iter().any(|active| {
            overlaps(
                window.iova,
                u64::from(window.capacity),
                active.iova,
                u64::from(active.capacity),
            )
        }) {
            return Err(Error::ReceiveBufferOverlap);
        }
        Ok(())
    }

    // Used by the separately compiled native CE adapter after descriptor
    // staging; the pure core harness does not include that unsafe boundary.
    #[allow(dead_code)]
    pub(crate) fn reserve(
        &mut self,
        window: RxBufferWindow,
        ticket: SlotTicket,
    ) -> Result<(), Error> {
        self.preview_reserve(window)?;
        if self.tickets[..self.count].contains(&Some(ticket)) {
            return Err(Error::ReceiveBufferNotReserved);
        }
        self.buffers[self.count] = window;
        self.tickets[self.count] = Some(ticket);
        self.count += 1;
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn preview_release(
        &self,
        ticket: SlotTicket,
        window: RxBufferWindow,
    ) -> Result<(), Error> {
        if self.tickets[..self.count].iter().zip(self.active()).any(
            |(active_ticket, active_window)| {
                *active_ticket == Some(ticket) && *active_window == window
            },
        ) {
            Ok(())
        } else {
            Err(Error::ReceiveBufferNotReserved)
        }
    }

    // Called only after native completion validation has retired the slot.
    #[allow(dead_code)]
    pub(crate) fn release(
        &mut self,
        ticket: SlotTicket,
        window: RxBufferWindow,
    ) -> Result<(), Error> {
        self.preview_release(ticket, window)?;
        let index = self.tickets[..self.count]
            .iter()
            .position(|active| *active == Some(ticket))
            .ok_or(Error::ReceiveBufferNotReserved)?;
        self.count -= 1;
        self.buffers[index] = self.buffers[self.count];
        self.buffers[self.count] = RxBufferWindow::EMPTY;
        self.tickets[index] = self.tickets[self.count];
        self.tickets[self.count] = None;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidDmaPool,
    DmaPoolOutsideCeWindow,
    AddressOverflow,
    MissingWmiRoute,
    DuplicateWmiRoute,
    WrongWmiRoute,
    MissingServiceRoute,
    DuplicateServiceRoute,
    WrongServiceRoute,
    InvalidFrame,
    InvalidPlan,
    FrameTooLarge,
    BufferTooSmall,
    BufferOutsideDmaPool,
    BufferOverlapsRing,
    ReceiveBufferOverlap,
    ReceiveLedgerFull,
    ReceiveBufferNotReserved,
    InvalidSensitiveTxBuffer,
    SensitiveTxBufferOverlap,
    SensitiveTxLedgerFull,
    SensitiveTxBufferNotReserved,
    MissingDestinationRing,
    DuplicateDestinationRing,
    InvalidDestinationRing,
    MissingSourceRing,
    DuplicateSourceRing,
    InvalidSourceRing,
    InvalidSlotAddress,
    Descriptor(qcom_ce_v3::Error),
    Cursor(qcom_ce_v3::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiCeTxPlan<'a> {
    frame: &'a [u8],
    route: WmiRoute,
    ticket: SlotTicket,
    descriptor: TxDescriptor,
    descriptor_slot_iova: u64,
    next_producer_index: usize,
    frame_bytes: u16,
}

impl<'a> WmiCeTxPlan<'a> {
    /// Endpoint byte from the validated staged HTC frame. The frame is
    /// non-empty for every plan accepted by `plan_wmi_tx`.
    pub const fn endpoint_id(self) -> u8 {
        self.frame[0]
    }

    pub const fn route(self) -> WmiRoute {
        self.route
    }

    pub const fn ticket(self) -> SlotTicket {
        self.ticket
    }

    pub const fn descriptor(self) -> TxDescriptor {
        self.descriptor
    }

    pub const fn descriptor_slot_iova(self) -> u64 {
        self.descriptor_slot_iova
    }

    pub const fn next_producer_index(self) -> usize {
        self.next_producer_index
    }

    pub const fn frame_bytes(self) -> u16 {
        self.frame_bytes
    }

    pub const fn frame(self) -> &'a [u8] {
        self.frame
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiCeRxPostPlan {
    route: WmiRoute,
    buffer: RxBufferWindow,
    ticket: SlotTicket,
    descriptor: RxDescriptor,
    descriptor_slot_iova: u64,
    next_producer_index: usize,
}

/// Generic target-to-host CE post for an explicitly selected service route.
/// Unlike the WMI alias, this plan is also used for HTT's CE1 receive path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCeRxPostPlan {
    service_id: u16,
    route: WmiRoute,
    buffer: RxBufferWindow,
    ticket: SlotTicket,
    descriptor: RxDescriptor,
    descriptor_slot_iova: u64,
    next_producer_index: usize,
}

impl ServiceCeRxPostPlan {
    pub const fn service_id(self) -> u16 {
        self.service_id
    }

    pub const fn route(self) -> WmiRoute {
        self.route
    }

    pub const fn buffer(self) -> RxBufferWindow {
        self.buffer
    }

    pub const fn ticket(self) -> SlotTicket {
        self.ticket
    }

    pub const fn descriptor(self) -> RxDescriptor {
        self.descriptor
    }

    pub const fn descriptor_slot_iova(self) -> u64 {
        self.descriptor_slot_iova
    }

    pub const fn next_producer_index(self) -> usize {
        self.next_producer_index
    }
}

impl WmiCeRxPostPlan {
    pub const fn route(self) -> WmiRoute {
        self.route
    }

    pub const fn buffer(self) -> RxBufferWindow {
        self.buffer
    }

    pub const fn ticket(self) -> SlotTicket {
        self.ticket
    }

    pub const fn descriptor(self) -> RxDescriptor {
        self.descriptor
    }

    pub const fn descriptor_slot_iova(self) -> u64 {
        self.descriptor_slot_iova
    }

    pub const fn next_producer_index(self) -> usize {
        self.next_producer_index
    }
}

/// Validate the WMI_CONTROL_SVC row in the target-selected map in both
/// directions. Unrelated service rows are ignored; duplicate WMI rows fail.
pub fn validate_wmi_route(mappings: &[ServiceCeMapping]) -> Result<WmiRoute, Error> {
    let route =
        validate_service_route(mappings, WMI_CONTROL_SERVICE_ID).map_err(|error| match error {
            Error::MissingServiceRoute => Error::MissingWmiRoute,
            Error::DuplicateServiceRoute => Error::DuplicateWmiRoute,
            other => other,
        })?;
    if route.host_to_target_engine != WMI_CONTROL_SOURCE_CE
        || route.target_to_host_engine != WMI_CONTROL_DESTINATION_CE
    {
        return Err(Error::WrongWmiRoute);
    }
    Ok(route)
}

/// Resolve exactly one mapping in each direction for a selected service.
/// Unrelated services are ignored, but duplicates or a missing direction fail.
pub fn validate_service_route(
    mappings: &[ServiceCeMapping],
    service_id: u16,
) -> Result<WmiRoute, Error> {
    let mut host_to_target = None;
    let mut target_to_host = None;
    for mapping in mappings
        .iter()
        .filter(|mapping| mapping.service_id == service_id)
    {
        let selected = match mapping.direction {
            Direction::HostToTarget => &mut host_to_target,
            Direction::TargetToHost => &mut target_to_host,
        };
        if selected.replace(mapping.engine).is_some() {
            return Err(Error::DuplicateServiceRoute);
        }
    }
    let host_to_target = host_to_target.ok_or(Error::MissingServiceRoute)?;
    let target_to_host = target_to_host.ok_or(Error::MissingServiceRoute)?;
    Ok(WmiRoute {
        host_to_target_engine: host_to_target,
        target_to_host_engine: target_to_host,
    })
}

/// Plan a bounded CE receive-buffer post for any service with a validated
/// target-selected route. The destination buffer must meet the engine's
/// configured minimum receive capacity (`src_sz_max`).
pub fn plan_service_rx_post(
    mappings: &[ServiceCeMapping],
    service_id: u16,
    expected_route: WmiRoute,
    profile: Profile,
    dma_pool: DmaPool,
    buffer: RxBufferWindow,
    ledger: &RxBufferLedger,
    rings: &RingPlan,
    cursor: RingCursor,
) -> Result<ServiceCeRxPostPlan, Error> {
    let route = validate_service_route(mappings, service_id)?;
    if route != expected_route {
        return Err(Error::WrongServiceRoute);
    }
    validate_rings(profile, dma_pool, rings)?;
    let ring = find_destination_ring(rings, route.target_to_host_engine)?;
    let config = profile
        .config(usize::from(route.target_to_host_engine))
        .map_err(|_| Error::InvalidDestinationRing)?;
    if ring.entries != config.destination_entries
        || usize::from(ring.entries) != cursor.entries()
        || buffer.capacity < u32::from(config.source_max_bytes)
        || buffer.capacity > u16::MAX as u32
        || buffer.iova & (qcom_ce_v3_profile::DMA_ALIGNMENT - 1) != 0
        || !dma_pool.contains(buffer.iova, u64::from(buffer.capacity))
    {
        return Err(Error::InvalidDestinationRing);
    }
    for allocation in rings.allocations() {
        if overlaps(
            buffer.iova,
            u64::from(buffer.capacity),
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        ) {
            return Err(Error::BufferOverlapsRing);
        }
    }
    ledger.preview_reserve(buffer)?;

    let ticket = cursor.prepare_next().map_err(Error::Cursor)?;
    let next_producer_index = cursor.preview_publish(ticket).map_err(Error::Cursor)?;
    let slot_offset = u64::try_from(ticket.index())
        .map_err(|_| Error::InvalidSlotAddress)?
        .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        .ok_or(Error::InvalidSlotAddress)?;
    let descriptor_slot_iova = ring
        .descriptor_iova
        .checked_add(slot_offset)
        .ok_or(Error::InvalidSlotAddress)?;
    if !dma_pool.contains(descriptor_slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        || !range_contains(
            ring.descriptor_iova,
            u64::from(ring.descriptor_bytes),
            descriptor_slot_iova,
            qcom_ce_v3::DESCRIPTOR_BYTES as u64,
        )
    {
        return Err(Error::InvalidSlotAddress);
    }
    Ok(ServiceCeRxPostPlan {
        service_id,
        route,
        buffer,
        ticket,
        descriptor: RxDescriptor::post(buffer.iova).map_err(Error::Descriptor)?,
        descriptor_slot_iova,
        next_producer_index,
    })
}

/// Check a caller-owned HTC/WMI frame and produce the CE3 source descriptor
/// plan. The caller must still hold an exclusive, live DMA/CE lease. Before
/// `RingCursor::commit_publish`, native code must copy descriptor bytes,
/// perform the required ordering operation, and successfully write CE3's
/// producer index. This function does none of those operations.
pub fn plan_wmi_tx<'a>(
    mappings: &[ServiceCeMapping],
    frame: &'a [u8],
    wmi: WmiTxPlan,
    endpoint_id: u8,
    transfer_id: u16,
    dma_pool: DmaPool,
    buffer: TxBufferWindow,
    profile: Profile,
    rings: &RingPlan,
    cursor: RingCursor,
) -> Result<WmiCeTxPlan<'a>, Error> {
    let route = validate_wmi_route(mappings)?;
    let frame_len = validate_wmi_frame(frame, wmi, endpoint_id)?;
    if frame_len > qcom_ce_v3::MAX_DESCRIPTOR_BYTES as usize {
        return Err(Error::FrameTooLarge);
    }

    validate_rings(profile, dma_pool, rings)?;
    let source_ring = find_source_ring(rings, route.host_to_target_engine)?;
    let config = profile
        .config(usize::from(route.host_to_target_engine))
        .map_err(|_| Error::InvalidSourceRing)?;
    if usize::from(source_ring.entries) != cursor.entries()
        || source_ring.entries != config.source_entries
        || frame_len > usize::from(config.source_max_bytes)
    {
        return Err(Error::InvalidSourceRing);
    }

    if buffer.capacity < frame_len as u32 {
        return Err(Error::BufferTooSmall);
    }
    if buffer.iova & (qcom_ce_v3_profile::DMA_ALIGNMENT - 1) != 0 {
        return Err(Error::BufferOutsideDmaPool);
    }
    if !dma_pool.contains(buffer.iova, u64::from(buffer.capacity)) {
        return Err(Error::BufferOutsideDmaPool);
    }
    for allocation in rings.allocations() {
        if overlaps(
            buffer.iova,
            u64::from(buffer.capacity),
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        ) {
            return Err(Error::BufferOverlapsRing);
        }
    }

    let ticket = cursor.prepare_next().map_err(Error::Cursor)?;
    let slot_offset = u64::try_from(ticket.index())
        .map_err(|_| Error::InvalidSlotAddress)?
        .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        .ok_or(Error::InvalidSlotAddress)?;
    let descriptor_slot_iova = source_ring
        .descriptor_iova
        .checked_add(slot_offset)
        .ok_or(Error::InvalidSlotAddress)?;
    if !dma_pool.contains(descriptor_slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        || !range_contains(
            source_ring.descriptor_iova,
            u64::from(source_ring.descriptor_bytes),
            descriptor_slot_iova,
            qcom_ce_v3::DESCRIPTOR_BYTES as u64,
        )
    {
        return Err(Error::InvalidSlotAddress);
    }

    let descriptor = TxDescriptor::encode(
        buffer.iova,
        u32::try_from(frame_len).map_err(|_| Error::FrameTooLarge)?,
        u32::from(transfer_id),
        false,
        false,
    )
    .map_err(Error::Descriptor)?;
    let next_producer_index = (ticket.index() + 1) & (cursor.entries() - 1);
    Ok(WmiCeTxPlan {
        frame,
        route,
        ticket,
        descriptor,
        descriptor_slot_iova,
        next_producer_index,
        frame_bytes: u16::try_from(frame_len).map_err(|_| Error::FrameTooLarge)?,
    })
}

/// Plan one bounded receive-buffer post for the selected WMI CONTROL service.
/// The buffer remains reserved in `RxBufferLedger` until a later validated CE2
/// completion is consumed and the owner releases it.
pub fn plan_wmi_rx_post(
    mappings: &[ServiceCeMapping],
    profile: Profile,
    dma_pool: DmaPool,
    buffer: RxBufferWindow,
    ledger: &RxBufferLedger,
    rings: &RingPlan,
    cursor: RingCursor,
) -> Result<WmiCeRxPostPlan, Error> {
    let route = validate_wmi_route(mappings)?;
    validate_rings(profile, dma_pool, rings)?;
    let ring = find_destination_ring(rings, route.target_to_host_engine)?;
    let config = profile
        .config(usize::from(route.target_to_host_engine))
        .map_err(|_| Error::InvalidDestinationRing)?;
    if ring.entries != config.destination_entries
        || usize::from(ring.entries) != cursor.entries()
        || buffer.capacity < 256
        || buffer.capacity > u32::from(config.source_max_bytes)
        || buffer.iova & (qcom_ce_v3_profile::DMA_ALIGNMENT - 1) != 0
        || !dma_pool.contains(buffer.iova, u64::from(buffer.capacity))
    {
        return Err(Error::InvalidDestinationRing);
    }
    for allocation in rings.allocations() {
        if overlaps(
            buffer.iova,
            u64::from(buffer.capacity),
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        ) {
            return Err(Error::BufferOverlapsRing);
        }
    }
    ledger.preview_reserve(buffer)?;

    let ticket = cursor.prepare_next().map_err(Error::Cursor)?;
    let next_producer_index = cursor.preview_publish(ticket).map_err(Error::Cursor)?;
    let slot_offset = u64::try_from(ticket.index())
        .map_err(|_| Error::InvalidSlotAddress)?
        .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        .ok_or(Error::InvalidSlotAddress)?;
    let descriptor_slot_iova = ring
        .descriptor_iova
        .checked_add(slot_offset)
        .ok_or(Error::InvalidSlotAddress)?;
    if !dma_pool.contains(descriptor_slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        || !range_contains(
            ring.descriptor_iova,
            u64::from(ring.descriptor_bytes),
            descriptor_slot_iova,
            qcom_ce_v3::DESCRIPTOR_BYTES as u64,
        )
    {
        return Err(Error::InvalidSlotAddress);
    }
    Ok(WmiCeRxPostPlan {
        route,
        buffer,
        ticket,
        descriptor: RxDescriptor::post(buffer.iova).map_err(Error::Descriptor)?,
        descriptor_slot_iova,
        next_producer_index,
    })
}

fn validate_wmi_frame(frame: &[u8], plan: WmiTxPlan, endpoint_id: u8) -> Result<usize, Error> {
    if frame.len() < HTC_FRAME_HEADER_SIZE + HTC_WMI_COMMAND_HEADER_SIZE + 1
        || frame.len() != plan.frame_len
        || plan.wmi_payload_len < HTC_WMI_COMMAND_HEADER_SIZE + 1
        || plan.frame_len != HTC_FRAME_HEADER_SIZE + plan.wmi_payload_len
        || plan.credits_required == 0
    {
        return Err(Error::InvalidPlan);
    }
    let first = u32::from_le_bytes(frame[0..4].try_into().map_err(|_| Error::InvalidFrame)?);
    let observed_endpoint = first as u8;
    let flags = (first >> 8) as u8;
    let declared_payload = (first >> 16) as usize;
    if endpoint_id == qcom_htc::HTC_CONTROL_ENDPOINT
        || endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT
        || observed_endpoint != endpoint_id
        || flags & !HTC_NEED_CREDIT_UPDATE != 0
        || (flags & HTC_NEED_CREDIT_UPDATE != 0) != plan.requested_credit_update
        || declared_payload != plan.wmi_payload_len
        || frame[4..8] != [0, 0, 0, 0]
    {
        return Err(Error::InvalidFrame);
    }
    let command = u32::from_le_bytes(
        frame[HTC_FRAME_HEADER_SIZE..HTC_FRAME_HEADER_SIZE + HTC_WMI_COMMAND_HEADER_SIZE]
            .try_into()
            .map_err(|_| Error::InvalidFrame)?,
    );
    if command & !WMI_COMMAND_ID_MASK != 0 {
        return Err(Error::InvalidFrame);
    }
    Ok(frame.len())
}

fn find_source_ring(rings: &RingPlan, engine: u8) -> Result<RingAllocation, Error> {
    let mut found = None;
    for allocation in rings.allocations().iter().filter(|allocation| {
        allocation.engine == engine && allocation.direction == RingDirection::Source
    }) {
        if found.replace(*allocation).is_some() {
            return Err(Error::DuplicateSourceRing);
        }
    }
    found.ok_or(Error::MissingSourceRing)
}

fn find_destination_ring(rings: &RingPlan, engine: u8) -> Result<RingAllocation, Error> {
    let mut found = None;
    for allocation in rings.allocations().iter().filter(|allocation| {
        allocation.engine == engine && allocation.direction == RingDirection::Destination
    }) {
        if found.replace(*allocation).is_some() {
            return Err(Error::DuplicateDestinationRing);
        }
    }
    found.ok_or(Error::MissingDestinationRing)
}

fn validate_rings(profile: Profile, pool: DmaPool, rings: &RingPlan) -> Result<(), Error> {
    for allocation in rings.allocations() {
        if usize::from(allocation.engine) >= crate::qcom_ce_v3_regs::CE_COUNT {
            return Err(Error::InvalidSourceRing);
        }
    }
    for engine in 0..crate::qcom_ce_v3_regs::CE_COUNT {
        let config = profile
            .config(engine)
            .map_err(|_| Error::InvalidSourceRing)?;
        for (direction, expected_entries) in [
            (RingDirection::Source, config.source_entries),
            (RingDirection::Destination, config.destination_entries),
        ] {
            let mut matches = rings.allocations().iter().filter(|allocation| {
                usize::from(allocation.engine) == engine && allocation.direction == direction
            });
            match (expected_entries, matches.next()) {
                (0, None) => {}
                (0, Some(_)) | (_, None) => return Err(Error::InvalidSourceRing),
                (expected, Some(allocation)) => {
                    if allocation.entries != expected
                        || allocation.descriptor_bytes
                            != u32::from(expected) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
                        || allocation.allocation_bytes
                            != allocation.descriptor_bytes
                                + qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
                        || !allocation
                            .descriptor_iova
                            .is_multiple_of(qcom_ce_v3_profile::DMA_ALIGNMENT)
                        || !pool.contains(
                            allocation.descriptor_iova,
                            u64::from(allocation.allocation_bytes),
                        )
                        || matches.next().is_some()
                    {
                        return Err(Error::InvalidSourceRing);
                    }
                }
            }
        }
    }
    for (index, allocation) in rings.allocations().iter().enumerate() {
        for other in &rings.allocations()[index + 1..] {
            if overlaps(
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
                other.descriptor_iova,
                u64::from(other.allocation_bytes),
            ) {
                return Err(Error::InvalidSourceRing);
            }
        }
    }
    Ok(())
}

fn overlaps(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    let Some(a_end) = a.checked_add(a_bytes) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_bytes) else {
        return true;
    };
    a < b_end && b < a_end
}

fn range_contains(outer: u64, outer_bytes: u64, inner: u64, inner_bytes: u64) -> bool {
    let Some(outer_end) = outer.checked_add(outer_bytes) else {
        return false;
    };
    let Some(inner_end) = inner.checked_add(inner_bytes) else {
        return false;
    };
    inner >= outer && inner_end <= outer_end
}
