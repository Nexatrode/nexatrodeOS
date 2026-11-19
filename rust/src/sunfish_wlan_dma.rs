//! Bounded composition of the WCN3998 route, QCACLD CE v3 rings, WMI control
//! buffers, and an LPAE page-table plan.
//!
//! This is an offline plan. It allocates no physical memory, maps no IOVA,
//! writes no SMMU register, and does not provide HTT/network packet buffers or
//! establish authority to operate the WLAN device.
#![forbid(unsafe_code)]

use crate::qcom_ce_v3_profile::{PAGE_BYTES, RingDirection, RingPlan};
use crate::qcom_wmi_ce::{DmaPool, MAX_POSTED_WMI_RX_BUFFERS, RxBufferWindow, TxBufferWindow};
use crate::sunfish_htt_rx_profile::{
    HttRxProfileError, SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile,
};
use crate::sunfish_htt_rx_ring::{SunfishHttRxRingError, SunfishHttRxRingPlan};
use crate::sunfish_qcacld_rx_session::{
    Error as QcacldRxSessionError, RxBufferLease, SunfishQcacldRxSession,
};
use crate::sunfish_smmu::{RoutePlan, SUNFISH_MAX_CONTEXT_BANKS, SUNFISH_MAX_SMR_SLOTS};
use crate::sunfish_smmu_lpae::{
    LpaeContextRegisters, LpaeMultiS1Plan, LpaeMultiTablePages, LpaePageMapping, LpaePlanError,
    LpaeS1Config, LpaeS1Plan, LpaeTablePages, SMMU_LPAE_PAGE_BYTES, plan_lpae_s1,
    plan_lpae_s1_multi,
};

/// A 416-KiB WMI-control arena: up to one page of ring padding, as many as 127
/// CE2 RX buffers, and 31 CE3 TX buffers. The remaining bytes cover the
/// Mission CE descriptor rings within one 2-MiB LPAE level-2 entry.
pub const WLAN_CONTROL_DMA_POOL_BYTES: u64 = 0x6_8000;
pub const WMI_CONTROL_BUFFER_BYTES: u32 = 2048;
pub const MAX_WLAN_CE_DMA_PAGES: usize = 128;
pub const MAX_WMI_TX_BUFFERS: usize = 31;
/// The selected ring IOVA must fit in one ARM LPAE level-2 entry.
pub const LPAE_LEVEL2_BYTES: u64 = 1 << 21;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WlanDmaPlanError {
    RouteNotWlanAlias,
    RouteResourceOutOfRange,
    EmptyRingPlan,
    InvalidRingLayout,
    RingPoolNotLevel2Aligned,
    RingPoolSpansLevel2Entries,
    TooManyPoolPages,
    PhysicalPageCountMismatch,
    ControlPoolTooSmall,
    InvalidControlPool,
    HttPoolTooSmall,
    HttProfile(HttRxProfileError),
    HttRxRing(SunfishHttRxRingError),
    Lpae(LpaePlanError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttRxBufferClass {
    Initial,
    Fallback,
}

/// Fixed IOVA/capacity assigned by the HTT RX plan to a QCACLD buffer ID.
/// This identifies a planned window only; it grants no DMA authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttRxDmaBufferWindow {
    pub buffer_id: u32,
    pub iova: u64,
    pub capacity_bytes: u32,
    pub class: HttRxBufferClass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttRxBufferResolveError {
    Session(QcacldRxSessionError),
    BufferIdOutOfRange(u32),
}

impl From<LpaePlanError> for WlanDmaPlanError {
    fn from(value: LpaePlanError) -> Self {
        Self::Lpae(value)
    }
}

impl From<SunfishHttRxRingError> for WlanDmaPlanError {
    fn from(value: SunfishHttRxRingError) -> Self {
        Self::HttRxRing(value)
    }
}

/// One consistent WCN3998 CE-ring and WMI-control-buffer DMA plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlanCeDmaPlan {
    route: RoutePlan,
    rings: RingPlan,
    pool_bytes: u64,
    mappings: [LpaePageMapping; MAX_WLAN_CE_DMA_PAGES],
    mapping_count: usize,
    rx_buffers: [RxBufferWindow; MAX_POSTED_WMI_RX_BUFFERS],
    rx_buffer_count: usize,
    tx_buffers: [TxBufferWindow; MAX_WMI_TX_BUFFERS],
    tx_buffer_count: usize,
    lpae: LpaeS1Plan,
}

/// A translated WCN alias route bound to a caller-provided multi-L3 DMA map.
/// The map must meet the source-profile capacity lower bound and cover the
/// exact ring, index, startup-buffer, and fallback-buffer IOVAs. It does not
/// allocate buffers, prove page ownership, or authorize device execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlanHttDmaPlan<'a> {
    route: RoutePlan,
    rx_ring: SunfishHttRxRingPlan,
    mappings: &'a [LpaePageMapping],
    /// Source-derived minimum page capacity for the HTT startup and fallback
    /// pools, rounded as though packed into 4-KiB pages.
    minimum_reserved_pages: usize,
    lpae: LpaeMultiS1Plan<'a>,
}

impl WlanHttDmaPlan<'_> {
    pub const fn route(&self) -> RoutePlan {
        self.route
    }

    pub const fn mappings(&self) -> &[LpaePageMapping] {
        self.mappings
    }

    pub const fn rx_ring(&self) -> SunfishHttRxRingPlan {
        self.rx_ring
    }

    pub const fn minimum_reserved_pages(&self) -> usize {
        self.minimum_reserved_pages
    }

    pub const fn lpae(&self) -> LpaeMultiS1Plan<'_> {
        self.lpae
    }

    pub const fn stream_match_slot(&self) -> usize {
        self.route.slot()
    }

    pub const fn context_bank(&self) -> usize {
        self.route.context_bank()
    }

    pub const fn context_registers(&self) -> LpaeContextRegisters {
        self.lpae.context()
    }

    /// Resolve an HTT buffer ID to the IOVA window already validated by this
    /// plan. IDs below the initial fill level name the posted ring buffers;
    /// later IDs address the separate fallback reserve.
    pub fn rx_buffer_window(&self, buffer_id: u32) -> Option<HttRxDmaBufferWindow> {
        let ring = self.rx_ring;
        let (class, index) = if buffer_id < ring.initial_fill_level() {
            (HttRxBufferClass::Initial, buffer_id)
        } else {
            (
                HttRxBufferClass::Fallback,
                buffer_id - ring.initial_fill_level(),
            )
        };
        let iova = match class {
            HttRxBufferClass::Initial => ring.initial_buffer_iova(index),
            HttRxBufferClass::Fallback => ring.fallback_buffer_iova(index),
        }
        .ok()?;
        Some(HttRxDmaBufferWindow {
            buffer_id,
            iova,
            capacity_bytes: ring.buffer_bytes(),
            class,
        })
    }

    /// Resolve only a currently active QCACLD lease. The session validates its
    /// generation and ownership state before the buffer ID is mapped to an
    /// IOVA window.
    pub fn rx_buffer_window_for_lease<const N: usize>(
        &self,
        session: &SunfishQcacldRxSession<N>,
        lease: RxBufferLease,
    ) -> Result<HttRxDmaBufferWindow, HttRxBufferResolveError> {
        let buffer_id = session
            .buffer_id_for_lease(lease)
            .map_err(HttRxBufferResolveError::Session)?;
        self.rx_buffer_window(buffer_id)
            .ok_or(HttRxBufferResolveError::BufferIdOutOfRange(buffer_id))
    }
}

impl WlanCeDmaPlan {
    pub const fn route(&self) -> RoutePlan {
        self.route
    }

    pub const fn rings(&self) -> &RingPlan {
        &self.rings
    }

    pub const fn pool_bytes(&self) -> u64 {
        self.pool_bytes
    }

    pub fn mappings(&self) -> &[LpaePageMapping] {
        &self.mappings[..self.mapping_count]
    }

    pub fn rx_buffers(&self) -> &[RxBufferWindow] {
        &self.rx_buffers[..self.rx_buffer_count]
    }

    pub fn tx_buffers(&self) -> &[TxBufferWindow] {
        &self.tx_buffers[..self.tx_buffer_count]
    }

    pub const fn lpae(&self) -> LpaeS1Plan {
        self.lpae
    }

    pub const fn stream_match_slot(&self) -> usize {
        self.route.slot()
    }

    pub const fn context_bank(&self) -> usize {
        self.route.context_bank()
    }

    pub const fn context_registers(&self) -> LpaeContextRegisters {
        self.lpae.context()
    }
}

/// Combine the exact WCN alias route with a source-derived CE ring plan and a
/// mapped arena for WMI_CONTROL transmit and receive buffers.
///
/// The ring plan must begin at a 2-MiB IOVA boundary. `pool_bytes` must be a
/// page-aligned arena large enough for its rings, 127 fixed 2-KiB CE2 receive
/// buffers, and 31 fixed 2-KiB CE3 transmit buffers. `physical_pages` must
/// contain one distinct, page-aligned frame per arena page. The LPAE planner
/// checks physical-address width and table-page aliases; this function does
/// not prove that the supplied frames are allocated, owned, or safe for DMA.
pub fn plan_wlan_ce_dma_translation(
    route: RoutePlan,
    rings: RingPlan,
    pool_bytes: u64,
    physical_pages: &[u64],
    table_pages: LpaeTablePages,
    config: LpaeS1Config,
) -> Result<WlanCeDmaPlan, WlanDmaPlanError> {
    if route.id() != 0x240
        || route.mask() != 1
        || route.matched_sids() != [0x240, 0x241]
        || route.matched_sid_count() != 2
    {
        return Err(WlanDmaPlanError::RouteNotWlanAlias);
    }
    if route.slot() >= SUNFISH_MAX_SMR_SLOTS || route.context_bank() >= SUNFISH_MAX_CONTEXT_BANKS {
        return Err(WlanDmaPlanError::RouteResourceOutOfRange);
    }

    let allocations = rings.allocations();
    if allocations.is_empty() || rings.total_bytes() == 0 {
        return Err(WlanDmaPlanError::EmptyRingPlan);
    }
    let rx_entries = allocations
        .iter()
        .find(|allocation| {
            allocation.engine == 2 && allocation.direction == RingDirection::Destination
        })
        .map(|allocation| usize::from(allocation.entries))
        .ok_or(WlanDmaPlanError::InvalidRingLayout)?;
    let tx_entries = allocations
        .iter()
        .find(|allocation| allocation.engine == 3 && allocation.direction == RingDirection::Source)
        .map(|allocation| usize::from(allocation.entries))
        .ok_or(WlanDmaPlanError::InvalidRingLayout)?;
    if !(2..=MAX_POSTED_WMI_RX_BUFFERS + 1).contains(&rx_entries)
        || !(2..=MAX_WMI_TX_BUFFERS + 1).contains(&tx_entries)
    {
        return Err(WlanDmaPlanError::InvalidRingLayout);
    }
    let rx_buffer_count = rx_entries - 1;
    let tx_buffer_count = tx_entries - 1;
    let iova_base = allocations[0].descriptor_iova;
    if !iova_base.is_multiple_of(LPAE_LEVEL2_BYTES) {
        return Err(WlanDmaPlanError::RingPoolNotLevel2Aligned);
    }
    let ring_end = iova_base
        .checked_add(rings.total_bytes())
        .ok_or(WlanDmaPlanError::InvalidRingLayout)?;
    if ring_end <= iova_base || pool_bytes == 0 || !pool_bytes.is_multiple_of(SMMU_LPAE_PAGE_BYTES)
    {
        return Err(WlanDmaPlanError::InvalidControlPool);
    }
    let pool =
        DmaPool::new(iova_base, pool_bytes).map_err(|_| WlanDmaPlanError::InvalidControlPool)?;
    let pool_end = iova_base
        .checked_add(pool_bytes)
        .ok_or(WlanDmaPlanError::InvalidControlPool)?;
    if ring_end > pool_end {
        return Err(WlanDmaPlanError::ControlPoolTooSmall);
    }
    if (iova_base / LPAE_LEVEL2_BYTES) != ((pool_end - 1) / LPAE_LEVEL2_BYTES) {
        return Err(WlanDmaPlanError::RingPoolSpansLevel2Entries);
    }

    let mut cursor = iova_base;
    for allocation in allocations {
        if allocation.descriptor_iova != cursor
            || allocation.descriptor_bytes == 0
            || u64::from(allocation.allocation_bytes) < u64::from(allocation.descriptor_bytes)
        {
            return Err(WlanDmaPlanError::InvalidRingLayout);
        }
        cursor = cursor
            .checked_add(u64::from(allocation.allocation_bytes))
            .ok_or(WlanDmaPlanError::InvalidRingLayout)?;
    }
    if cursor != ring_end {
        return Err(WlanDmaPlanError::InvalidRingLayout);
    }

    let buffer_base = ring_end
        .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
        .ok_or(WlanDmaPlanError::InvalidRingLayout)?
        & !(SMMU_LPAE_PAGE_BYTES - 1);
    let rx_bytes = u64::try_from(rx_buffer_count)
        .map_err(|_| WlanDmaPlanError::ControlPoolTooSmall)?
        .checked_mul(u64::from(WMI_CONTROL_BUFFER_BYTES))
        .ok_or(WlanDmaPlanError::ControlPoolTooSmall)?;
    let tx_base = buffer_base
        .checked_add(rx_bytes)
        .ok_or(WlanDmaPlanError::ControlPoolTooSmall)?;
    let tx_bytes = u64::try_from(tx_buffer_count)
        .map_err(|_| WlanDmaPlanError::ControlPoolTooSmall)?
        .checked_mul(u64::from(WMI_CONTROL_BUFFER_BYTES))
        .ok_or(WlanDmaPlanError::ControlPoolTooSmall)?;
    let buffers_end = tx_base
        .checked_add(tx_bytes)
        .ok_or(WlanDmaPlanError::ControlPoolTooSmall)?;
    if buffers_end > pool_end {
        return Err(WlanDmaPlanError::ControlPoolTooSmall);
    }

    let page_count_u64 = pool_bytes / SMMU_LPAE_PAGE_BYTES;
    let page_count =
        usize::try_from(page_count_u64).map_err(|_| WlanDmaPlanError::TooManyPoolPages)?;
    if page_count > MAX_WLAN_CE_DMA_PAGES {
        return Err(WlanDmaPlanError::TooManyPoolPages);
    }
    if physical_pages.len() != page_count {
        return Err(WlanDmaPlanError::PhysicalPageCountMismatch);
    }

    let mut mappings = [LpaePageMapping {
        iova_page: 0,
        physical_page: 0,
    }; MAX_WLAN_CE_DMA_PAGES];
    for (index, physical_page) in physical_pages.iter().copied().enumerate() {
        mappings[index] = LpaePageMapping {
            iova_page: iova_base + index as u64 * PAGE_BYTES,
            physical_page,
        };
    }
    let lpae = plan_lpae_s1(&mappings[..page_count], table_pages, config)?;
    let mut rx_buffers = [RxBufferWindow {
        iova: 0,
        capacity: 0,
    }; MAX_POSTED_WMI_RX_BUFFERS];
    for (index, buffer) in rx_buffers[..rx_buffer_count].iter_mut().enumerate() {
        *buffer = RxBufferWindow {
            iova: buffer_base + index as u64 * u64::from(WMI_CONTROL_BUFFER_BYTES),
            capacity: WMI_CONTROL_BUFFER_BYTES,
        };
    }
    let mut tx_buffers = [TxBufferWindow {
        iova: 0,
        capacity: 0,
    }; MAX_WMI_TX_BUFFERS];
    for (index, buffer) in tx_buffers[..tx_buffer_count].iter_mut().enumerate() {
        *buffer = TxBufferWindow {
            iova: tx_base + index as u64 * u64::from(WMI_CONTROL_BUFFER_BYTES),
            capacity: WMI_CONTROL_BUFFER_BYTES,
        };
    }
    Ok(WlanCeDmaPlan {
        route,
        rings,
        pool_bytes: pool.bytes(),
        mappings,
        mapping_count: page_count,
        rx_buffers,
        rx_buffer_count,
        tx_buffers,
        tx_buffer_count,
        lpae,
    })
}

/// Bind a caller's WLAN alias route and HTT-capable DMA map to the multi-L3
/// planner. The map must be sorted by IOVA and may include discontiguous IOVA
/// extents within one 1-GiB root region. It must contain enough pages for the
/// source-derived HTT startup and fallback pool and cover the exact IOVAs
/// selected by `rx_ring`. This does not prove ownership. All mapping and table
/// frames remain caller-provided.
pub fn plan_wlan_htt_dma_translation<'a>(
    route: RoutePlan,
    rx_ring: SunfishHttRxRingPlan,
    mappings: &'a [LpaePageMapping],
    table_pages: LpaeMultiTablePages<'a>,
    config: LpaeS1Config,
) -> Result<WlanHttDmaPlan<'a>, WlanDmaPlanError> {
    if route.id() != 0x240
        || route.mask() != 1
        || route.matched_sids() != [0x240, 0x241]
        || route.matched_sid_count() != 2
    {
        return Err(WlanDmaPlanError::RouteNotWlanAlias);
    }
    if route.slot() >= SUNFISH_MAX_SMR_SLOTS || route.context_bank() >= SUNFISH_MAX_CONTEXT_BANKS {
        return Err(WlanDmaPlanError::RouteResourceOutOfRange);
    }

    let minimum_reserved_pages = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG)
        .map_err(WlanDmaPlanError::HttProfile)?
        .minimum_reserved_page_count()
        .and_then(|pages| usize::try_from(pages).ok())
        .ok_or(WlanDmaPlanError::HttProfile(
            HttRxProfileError::ArithmeticOverflow,
        ))?;
    if mappings.len() < minimum_reserved_pages {
        return Err(WlanDmaPlanError::HttPoolTooSmall);
    }
    rx_ring.validate_mappings(mappings)?;

    let lpae = plan_lpae_s1_multi(mappings, table_pages, config)?;
    Ok(WlanHttDmaPlan {
        route,
        rx_ring,
        mappings,
        minimum_reserved_pages,
        lpae,
    })
}
