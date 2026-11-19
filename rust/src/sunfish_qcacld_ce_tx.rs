//! Offline two-entry CE4 gather planning for Sunfish QCACLD native-Wi-Fi TX.
//!
//! QCACLD sends the HTC/HTT prefix as one source descriptor and the frame
//! prefix as the terminating descriptor. This module validates and plans that
//! packet against a selected service map, the Adrastea CE profile, and a
//! bounded DMA pool. It does not copy bytes, synchronize caches, publish the
//! producer index, or authorize WLAN access.
#![forbid(unsafe_code)]

use crate::qcom_ce_v3::{self, RingCursor, SlotTicket, TxDescriptor};
use crate::qcom_ce_v3_profile::{self, Profile, RingAllocation, RingDirection, RingPlan};
use crate::qcom_htc::HtcError;
use crate::qcom_wmi_ce::{Direction, DmaPool, ServiceCeMapping, TxBufferWindow};
use crate::sunfish_qcacld_htc::{self, HttEndpoint};
use crate::sunfish_qcacld_tx::{
    self, HELIUMPLUS_FRAGMENT_DESC_BYTES, HTT_DATA_HOST_TO_TARGET_CE, HTT_DATA_SERVICE_ID,
    HTT_DATA_TARGET_TO_HOST_CE, TxMetadata,
};

pub const HTC_HTT_PREFIX_BYTES: usize = 8 + sunfish_qcacld_tx::HTT_TX_DESC_BYTES;
pub const CE4_DATA_DESCRIPTOR_COUNT: usize = 2;
/// Source-configured compact-SNOC download: HTC+HTT header (28), worst-case
/// outer header (36), VLAN (4), LLC/SNAP (8), and low-latency IP prefix (16).
pub const QCACLD_NATIVE_WIFI_DOWNLOAD_BYTES: usize = HTC_HTT_PREFIX_BYTES + 36 + 4 + 8 + 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttRoute {
    pub host_to_target_engine: u8,
    pub target_to_host_engine: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    MissingRoute,
    DuplicateRoute,
    WrongRoute,
    InvalidDmaPool,
    InvalidPrefixBuffer,
    InvalidFrameBuffer,
    InvalidFragmentTable,
    BufferOverlap,
    MissingCe4SourceRing,
    DuplicateCe4SourceRing,
    InvalidCe4SourceRing,
    DownloadTooLarge,
    InvalidSlotAddress,
    Htt(sunfish_qcacld_tx::Error),
    Htc(HtcError),
    Descriptor(qcom_ce_v3::Error),
    Cursor(qcom_ce_v3::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeTxBuffers {
    /// DMA region containing exactly the HTC header followed by the HTT TX
    /// descriptor. The full advertised capacity must be disjoint from data,
    /// table, and CE ring regions.
    pub htc_htt_prefix: TxBufferWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeTxPlan {
    route: HttRoute,
    endpoint_id: u8,
    sequence: u8,
    frame_bytes: u16,
    fragment_table_iova: u64,
    total_download_bytes: u16,
    frame_download_bytes: u16,
    htt_prefix: [u8; HTC_HTT_PREFIX_BYTES],
    fragment_descriptor: [u8; HELIUMPLUS_FRAGMENT_DESC_BYTES],
    first_descriptor: TxDescriptor,
    second_descriptor: TxDescriptor,
    first_ticket: SlotTicket,
    first_slot_iova: u64,
    second_slot_iova: u64,
    next_producer_index: usize,
}

impl CeTxPlan {
    pub const fn route(self) -> HttRoute {
        self.route
    }

    pub const fn endpoint_id(self) -> u8 {
        self.endpoint_id
    }

    pub const fn sequence(self) -> u8 {
        self.sequence
    }

    pub const fn frame_bytes(self) -> u16 {
        self.frame_bytes
    }

    pub const fn frame_iova(self) -> u64 {
        self.second_descriptor.address()
    }

    pub const fn fragment_table_iova(self) -> u64 {
        self.fragment_table_iova
    }

    pub const fn total_download_bytes(self) -> u16 {
        self.total_download_bytes
    }

    pub const fn frame_download_bytes(self) -> u16 {
        self.frame_download_bytes
    }

    pub const fn htc_htt_prefix(self) -> [u8; HTC_HTT_PREFIX_BYTES] {
        self.htt_prefix
    }

    pub const fn fragment_descriptor(self) -> [u8; HELIUMPLUS_FRAGMENT_DESC_BYTES] {
        self.fragment_descriptor
    }

    pub const fn first_descriptor(self) -> TxDescriptor {
        self.first_descriptor
    }

    pub const fn second_descriptor(self) -> TxDescriptor {
        self.second_descriptor
    }

    pub const fn first_ticket(self) -> SlotTicket {
        self.first_ticket
    }

    pub const fn first_slot_iova(self) -> u64 {
        self.first_slot_iova
    }

    pub const fn second_slot_iova(self) -> u64 {
        self.second_slot_iova
    }

    pub const fn next_producer_index(self) -> usize {
        self.next_producer_index
    }
}

/// Verify the target-selected HTT data service route in both directions.
pub fn validate_htt_route(mappings: &[ServiceCeMapping]) -> Result<HttRoute, Error> {
    let mut host_to_target = None;
    let mut target_to_host = None;
    for mapping in mappings
        .iter()
        .filter(|mapping| mapping.service_id == HTT_DATA_SERVICE_ID)
    {
        let selected = match mapping.direction {
            Direction::HostToTarget => &mut host_to_target,
            Direction::TargetToHost => &mut target_to_host,
        };
        if selected.replace(mapping.engine).is_some() {
            return Err(Error::DuplicateRoute);
        }
    }
    let host_to_target = host_to_target.ok_or(Error::MissingRoute)?;
    let target_to_host = target_to_host.ok_or(Error::MissingRoute)?;
    if host_to_target != HTT_DATA_HOST_TO_TARGET_CE || target_to_host != HTT_DATA_TARGET_TO_HOST_CE
    {
        return Err(Error::WrongRoute);
    }
    Ok(HttRoute {
        host_to_target_engine: host_to_target,
        target_to_host_engine: target_to_host,
    })
}

/// Build a bounded HTT packet and two-descriptor CE4 gather plan.
///
/// The default Sunfish SNOC build uses the source-defined compact download
/// length of 92 bytes. The driver clips this to the actual HTC/HTT+frame packet
/// length for short frames. The frame IOVA and fragment table still describe
/// the complete frame when CE4 downloads only its classification prefix.
pub fn plan_native_wifi_tx(
    mappings: &[ServiceCeMapping],
    endpoint: HttEndpoint,
    sequence: u8,
    frame: &[u8],
    metadata: TxMetadata,
    buffers: CeTxBuffers,
    dma_pool: DmaPool,
    profile: Profile,
    rings: &RingPlan,
    cursor: RingCursor,
) -> Result<CeTxPlan, Error> {
    let route = validate_htt_route(mappings)?;
    let pool_base = dma_pool.base();
    let pool_end = pool_base
        .checked_add(dma_pool.bytes())
        .ok_or(Error::InvalidDmaPool)?;
    if pool_base == 0 || pool_end > qcom_ce_v3::MAX_CE_IOVA {
        return Err(Error::InvalidDmaPool);
    }

    let prefix_window = buffers.htc_htt_prefix;
    if prefix_window.capacity < HTC_HTT_PREFIX_BYTES as u32
        || !prefix_window
            .iova
            .is_multiple_of(qcom_ce_v3_profile::DMA_ALIGNMENT)
        || !pool_contains(
            pool_base,
            pool_end,
            prefix_window.iova,
            u64::from(prefix_window.capacity),
        )
    {
        return Err(Error::InvalidPrefixBuffer);
    }
    if frame.is_empty()
        || !pool_contains(pool_base, pool_end, metadata.frame_iova, frame.len() as u64)
    {
        return Err(Error::InvalidFrameBuffer);
    }
    if !pool_contains(
        pool_base,
        pool_end,
        metadata.fragment_table_iova,
        HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
    ) {
        return Err(Error::InvalidFragmentTable);
    }
    if ranges_overlap(
        prefix_window.iova,
        u64::from(prefix_window.capacity),
        metadata.frame_iova,
        frame.len() as u64,
    ) || ranges_overlap(
        prefix_window.iova,
        u64::from(prefix_window.capacity),
        metadata.fragment_table_iova,
        HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
    ) || ranges_overlap(
        metadata.frame_iova,
        frame.len() as u64,
        metadata.fragment_table_iova,
        HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
    ) {
        return Err(Error::BufferOverlap);
    }

    let (source_ring, source_config) = find_ce4_source_ring(profile, rings)?;
    if usize::from(source_ring.entries) != cursor.entries()
        || source_ring.entries != source_config.source_entries
    {
        return Err(Error::InvalidCe4SourceRing);
    }

    let tx = sunfish_qcacld_tx::encode_native_wifi_data(frame, metadata).map_err(Error::Htt)?;
    let total_packet_bytes = HTC_HTT_PREFIX_BYTES
        .checked_add(frame.len())
        .ok_or(Error::DownloadTooLarge)?;
    let total_download_bytes = QCACLD_NATIVE_WIFI_DOWNLOAD_BYTES.min(total_packet_bytes);
    let frame_download_bytes = total_download_bytes - HTC_HTT_PREFIX_BYTES;
    if frame_download_bytes > usize::from(source_config.source_max_bytes) {
        return Err(Error::DownloadTooLarge);
    }
    let htc_payload_bytes = sunfish_qcacld_tx::HTT_TX_DESC_BYTES
        .checked_add(frame_download_bytes)
        .ok_or(Error::DownloadTooLarge)?;
    if htc_payload_bytes > endpoint.max_message_size() {
        return Err(Error::Htc(HtcError::MessageTooLarge {
            limit: endpoint.max_message_size(),
            observed: htc_payload_bytes,
        }));
    }

    validate_ring_allocations(profile, pool_base, pool_end, rings)?;
    for allocation in rings.allocations() {
        if ranges_overlap(
            prefix_window.iova,
            u64::from(prefix_window.capacity),
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        ) || ranges_overlap(
            metadata.frame_iova,
            frame.len() as u64,
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        ) || ranges_overlap(
            metadata.fragment_table_iova,
            HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        ) {
            return Err(Error::BufferOverlap);
        }
    }

    let mut data_payload = [0u8; sunfish_qcacld_tx::HTT_TX_DESC_BYTES + 256];
    data_payload[..sunfish_qcacld_tx::HTT_TX_DESC_BYTES].copy_from_slice(&tx.htt_descriptor);
    data_payload[sunfish_qcacld_tx::HTT_TX_DESC_BYTES..htc_payload_bytes]
        .copy_from_slice(&frame[..frame_download_bytes]);
    let mut encoded_frame = [0u8; HTC_HTT_PREFIX_BYTES + 256];
    let frame_plan = sunfish_qcacld_htc::encode_data_frame(
        endpoint,
        sequence,
        &data_payload[..htc_payload_bytes],
        &mut encoded_frame,
    )
    .map_err(Error::Htc)?;
    if frame_plan.frame_len != total_download_bytes
        || encoded_frame[HTC_HTT_PREFIX_BYTES..total_download_bytes]
            != frame[..frame_download_bytes]
    {
        return Err(Error::Htc(HtcError::InvalidFramePayloadLength {
            declared: frame_plan.payload_len,
            observed: htc_payload_bytes,
        }));
    }

    let first_ticket = cursor.prepare_next().map_err(Error::Cursor)?;
    let next_producer_index = cursor
        .preview_publish_batch(first_ticket, CE4_DATA_DESCRIPTOR_COUNT)
        .map_err(Error::Cursor)?;
    let mask = cursor.entries() - 1;
    let first_slot_iova = ring_slot_iova(source_ring, first_ticket.index())?;
    let second_index = (first_ticket.index() + 1) & mask;
    let second_slot_iova = ring_slot_iova(source_ring, second_index)?;
    let transfer_id = u32::from(endpoint.endpoint_id());
    let first_descriptor = TxDescriptor::encode(
        prefix_window.iova,
        HTC_HTT_PREFIX_BYTES as u32,
        transfer_id,
        false,
        true,
    )
    .map_err(Error::Descriptor)?;
    let second_descriptor = TxDescriptor::encode(
        metadata.frame_iova,
        u32::try_from(frame_download_bytes).map_err(|_| Error::DownloadTooLarge)?,
        transfer_id,
        false,
        false,
    )
    .map_err(Error::Descriptor)?;

    let mut htt_prefix = [0u8; HTC_HTT_PREFIX_BYTES];
    htt_prefix.copy_from_slice(&encoded_frame[..HTC_HTT_PREFIX_BYTES]);
    Ok(CeTxPlan {
        route,
        endpoint_id: endpoint.endpoint_id(),
        sequence,
        frame_bytes: u16::try_from(frame.len()).map_err(|_| Error::DownloadTooLarge)?,
        fragment_table_iova: metadata.fragment_table_iova,
        total_download_bytes: u16::try_from(total_download_bytes)
            .map_err(|_| Error::DownloadTooLarge)?,
        frame_download_bytes: u16::try_from(frame_download_bytes)
            .map_err(|_| Error::DownloadTooLarge)?,
        htt_prefix,
        fragment_descriptor: tx.fragment_descriptor,
        first_descriptor,
        second_descriptor,
        first_ticket,
        first_slot_iova,
        second_slot_iova,
        next_producer_index,
    })
}

fn find_ce4_source_ring(
    profile: Profile,
    rings: &RingPlan,
) -> Result<(RingAllocation, qcom_ce_v3_profile::EngineConfig), Error> {
    let config = profile
        .config(usize::from(HTT_DATA_HOST_TO_TARGET_CE))
        .map_err(|_| Error::InvalidCe4SourceRing)?;
    let mut selected = None;
    for allocation in rings.allocations().iter().filter(|allocation| {
        allocation.engine == HTT_DATA_HOST_TO_TARGET_CE
            && allocation.direction == RingDirection::Source
    }) {
        if selected.replace(*allocation).is_some() {
            return Err(Error::DuplicateCe4SourceRing);
        }
    }
    Ok((selected.ok_or(Error::MissingCe4SourceRing)?, config))
}

fn validate_ring_allocations(
    profile: Profile,
    pool_base: u64,
    pool_end: u64,
    rings: &RingPlan,
) -> Result<(), Error> {
    for (index, allocation) in rings.allocations().iter().enumerate() {
        let config = profile
            .config(usize::from(allocation.engine))
            .map_err(|_| Error::InvalidCe4SourceRing)?;
        let expected_entries = match allocation.direction {
            RingDirection::Source => config.source_entries,
            RingDirection::Destination => config.destination_entries,
        };
        if allocation.entries != expected_entries
            || allocation.descriptor_bytes
                != u32::from(expected_entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || allocation.allocation_bytes
                != allocation.descriptor_bytes
                    + qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
            || !pool_contains(
                pool_base,
                pool_end,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            )
        {
            return Err(Error::InvalidCe4SourceRing);
        }
        for other in &rings.allocations()[index + 1..] {
            if ranges_overlap(
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
                other.descriptor_iova,
                u64::from(other.allocation_bytes),
            ) {
                return Err(Error::InvalidCe4SourceRing);
            }
        }
    }
    Ok(())
}

fn ring_slot_iova(ring: RingAllocation, index: usize) -> Result<u64, Error> {
    if index >= usize::from(ring.entries) {
        return Err(Error::InvalidSlotAddress);
    }
    let offset = u64::try_from(index)
        .map_err(|_| Error::InvalidSlotAddress)?
        .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
        .ok_or(Error::InvalidSlotAddress)?;
    let address = ring
        .descriptor_iova
        .checked_add(offset)
        .ok_or(Error::InvalidSlotAddress)?;
    if !range_contains(
        ring.descriptor_iova,
        u64::from(ring.descriptor_bytes),
        address,
        qcom_ce_v3::DESCRIPTOR_BYTES as u64,
    ) {
        return Err(Error::InvalidSlotAddress);
    }
    Ok(address)
}

fn pool_contains(pool_base: u64, pool_end: u64, address: u64, bytes: u64) -> bool {
    let Some(end) = address.checked_add(bytes) else {
        return false;
    };
    address >= pool_base && end <= pool_end
}

fn ranges_overlap(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    let Some(a_end) = a.checked_add(a_bytes) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_bytes) else {
        return true;
    };
    a < b_end && b < a_end
}

fn range_contains(base: u64, bytes: u64, address: u64, required: u64) -> bool {
    let Some(end) = base.checked_add(bytes) else {
        return false;
    };
    let Some(required_end) = address.checked_add(required) else {
        return false;
    };
    address >= base && required_end <= end
}
