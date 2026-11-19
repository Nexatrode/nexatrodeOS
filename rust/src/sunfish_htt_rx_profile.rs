//! Source-derived Qualcomm HTT receive-buffer sizing for Sunfish.
//!
//! This is a capacity profile only. It does not allocate/map DMA memory, alter
//! the existing CE/WMI arena, authorize WLAN ownership, or start the device.
#![forbid(unsafe_code)]

use crate::sunfish_smmu_lpae::SMMU_LPAE_PAGE_BYTES;

pub const SUNFISH_MAX_THROUGHPUT_MBPS: u32 = 800;
pub const HTT_RX_RING_SIZE_MIN: u32 = 128;
pub const HTT_RX_RING_SIZE_MAX: u32 = 2048;
pub const HTT_RX_AVG_FRAME_BYTES: u32 = 1000;
pub const HTT_RX_HOST_LATENCY_MAX_MS: u32 = 20;
pub const HTT_RX_HOST_LATENCY_WORST_LIKELY_MS: u32 = 20;
pub const HTT_RX_PRE_ALLOC_POOL_SIZE: u32 = 64;
pub const SUNFISH_MSDU_BYTES: u32 = 1560;
pub const SUNFISH_HOST_RX_DESC_BYTES: u32 = 552;
pub const SUNFISH_CACHE_LINE_BYTES: u32 = 128;
pub const SUNFISH_TARGET_PADDR_BYTES: u32 = 8;
pub const HTT_RX_INDEX_BYTES: u32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttRxConfig {
    pub max_throughput_mbps: u32,
    pub average_frame_bytes: u32,
    pub ring_latency_ms: u32,
    pub fill_latency_ms: u32,
    pub ring_min: u32,
    pub ring_max: u32,
    pub msdu_bytes: u32,
    pub host_rx_desc_bytes: u32,
    pub cache_line_bytes: u32,
    pub target_paddr_bytes: u32,
    pub preallocated_buffers: u32,
    pub has_target_index: bool,
}

/// Values traced from the pinned Sunfish QCACLD build: QCA_WIFI_3_0 selects a
/// 20-ms fill latency, HTT_PADDR64 selects 64-bit addresses, and the build's
/// MAX_THROUGHPUT is 800 Mbps. The 552-byte descriptor is the AArch64 C layout
/// of htt_host_rx_desc_base with HELIUMPLUS and QCA_WIFI_3_0_ADRASTEA (both
/// present in the matched QCACLD build) and the Sunfish descriptor definitions;
/// `htt_internal.h` then rounds 1560 + descriptor bytes to the 128-byte arm64
/// cache line. The runtime reorder setting is not known from this source
/// checkout, so `has_target_index: false` is a baseline; set it for the
/// full-reorder branch, which adds one 32-bit target index.
pub const SUNFISH_QCACLD_CONFIG: HttRxConfig = HttRxConfig {
    max_throughput_mbps: SUNFISH_MAX_THROUGHPUT_MBPS,
    average_frame_bytes: HTT_RX_AVG_FRAME_BYTES,
    ring_latency_ms: HTT_RX_HOST_LATENCY_MAX_MS,
    fill_latency_ms: HTT_RX_HOST_LATENCY_WORST_LIKELY_MS,
    ring_min: HTT_RX_RING_SIZE_MIN,
    ring_max: HTT_RX_RING_SIZE_MAX,
    msdu_bytes: SUNFISH_MSDU_BYTES,
    host_rx_desc_bytes: SUNFISH_HOST_RX_DESC_BYTES,
    cache_line_bytes: SUNFISH_CACHE_LINE_BYTES,
    target_paddr_bytes: SUNFISH_TARGET_PADDR_BYTES,
    preallocated_buffers: HTT_RX_PRE_ALLOC_POOL_SIZE,
    // The runtime config is not captured here. This baseline models the
    // ordinary receive ring; full-reorder mode adds a 32-bit target index.
    has_target_index: false,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttRxProfileError {
    ZeroFrameSize,
    ZeroRingLatency,
    ZeroCacheLine,
    ZeroAddressWidth,
    InvalidRingBounds,
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttRxProfile {
    ring_slots: u32,
    initial_fill_level: u32,
    rx_buffer_bytes: u32,
    address_ring_bytes: u64,
    index_bytes: u64,
    initial_rx_buffer_bytes: u64,
    fallback_buffer_capacity_bytes: u64,
}

impl HttRxProfile {
    pub const fn ring_slots(self) -> u32 {
        self.ring_slots
    }

    pub const fn initial_fill_level(self) -> u32 {
        self.initial_fill_level
    }

    pub const fn rx_buffer_bytes(self) -> u32 {
        self.rx_buffer_bytes
    }

    pub const fn address_ring_bytes(self) -> u64 {
        self.address_ring_bytes
    }

    pub const fn index_bytes(self) -> u64 {
        self.index_bytes
    }

    /// HTT address ring, indices, and the RX buffers posted at startup.
    /// Separately allocated allocator metadata and page rounding are excluded.
    pub const fn initial_dma_bytes(self) -> Option<u64> {
        let ring_and_indices = match self.address_ring_bytes.checked_add(self.index_bytes) {
            Some(bytes) => bytes,
            None => return None,
        };
        ring_and_indices.checked_add(self.initial_rx_buffer_bytes)
    }

    /// Capacity in the low-memory fallback pool; these buffers are not all
    /// necessarily DMA-mapped at once.
    pub const fn fallback_buffer_capacity_bytes(self) -> u64 {
        self.fallback_buffer_capacity_bytes
    }

    /// Startup DMA storage plus capacity for the 64-buffer low-memory reserve.
    /// The reserve may be mapped only when buffers are posted to the target.
    pub const fn reserved_pool_bytes(self) -> Option<u64> {
        match self.initial_dma_bytes() {
            Some(bytes) => bytes.checked_add(self.fallback_buffer_capacity_bytes),
            None => None,
        }
    }

    /// Lower bound for 4-KiB pages if the reported startup DMA bytes can be
    /// packed contiguously. Real mapping may need more pages due to allocation
    /// boundaries and buffer placement.
    pub const fn minimum_packed_page_count(self) -> Option<u64> {
        let bytes = match self.initial_dma_bytes() {
            Some(bytes) => bytes,
            None => return None,
        };
        let page_bytes = SMMU_LPAE_PAGE_BYTES;
        let rounded = match bytes.checked_add(page_bytes - 1) {
            Some(value) => value,
            None => return None,
        };
        Some(rounded / page_bytes)
    }

    /// 4-KiB pages needed for the startup DMA set and fallback buffer reserve
    /// if represented by one packed IOVA arena.
    pub const fn minimum_reserved_page_count(self) -> Option<u64> {
        let bytes = match self.reserved_pool_bytes() {
            Some(bytes) => bytes,
            None => return None,
        };
        let page_bytes = SMMU_LPAE_PAGE_BYTES;
        let rounded = match bytes.checked_add(page_bytes - 1) {
            Some(value) => value,
            None => return None,
        };
        Some(rounded / page_bytes)
    }
}

fn next_power_of_two(value: u32) -> Result<u32, HttRxProfileError> {
    value
        .checked_next_power_of_two()
        .ok_or(HttRxProfileError::ArithmeticOverflow)
}

fn frames_per_latency(config: HttRxConfig, latency_ms: u32) -> Result<u32, HttRxProfileError> {
    let numerator = u64::from(config.max_throughput_mbps)
        .checked_mul(1000)
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    let denominator = 8u64
        .checked_mul(u64::from(config.average_frame_bytes))
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    let frames = (numerator / denominator)
        .checked_mul(u64::from(latency_ms))
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    u32::try_from(frames).map_err(|_| HttRxProfileError::ArithmeticOverflow)
}

fn roundup(value: u32, alignment: u32) -> Result<u32, HttRxProfileError> {
    let rounded = value
        .checked_add(alignment - 1)
        .ok_or(HttRxProfileError::ArithmeticOverflow)?
        / alignment
        * alignment;
    Ok(rounded)
}

/// Mirror QCACLD's `htt_rx_ring_size`, `htt_rx_ring_fill_level`, and
/// `HTT_RX_BUF_SIZE` arithmetic without allocating or mapping any memory.
pub fn plan_htt_rx_profile(config: HttRxConfig) -> Result<HttRxProfile, HttRxProfileError> {
    if config.average_frame_bytes == 0 {
        return Err(HttRxProfileError::ZeroFrameSize);
    }
    if config.ring_latency_ms == 0 {
        return Err(HttRxProfileError::ZeroRingLatency);
    }
    if config.cache_line_bytes == 0 {
        return Err(HttRxProfileError::ZeroCacheLine);
    }
    if config.target_paddr_bytes == 0 {
        return Err(HttRxProfileError::ZeroAddressWidth);
    }
    if config.ring_min == 0 || config.ring_max < config.ring_min {
        return Err(HttRxProfileError::InvalidRingBounds);
    }

    // The C source clamps the raw size before rounding it to a power of two.
    let mut ring_size = frames_per_latency(config, config.ring_latency_ms)?;
    ring_size = ring_size.clamp(config.ring_min, config.ring_max);
    let ring_slots = next_power_of_two(ring_size)?;

    let fill_size = frames_per_latency(config, config.fill_latency_ms)?;
    let mut initial_fill_level = next_power_of_two(fill_size)?;
    if initial_fill_level >= ring_slots {
        initial_fill_level = ring_slots - 1;
    }

    let unrounded_buffer_bytes = config
        .msdu_bytes
        .checked_add(config.host_rx_desc_bytes)
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    let rx_buffer_bytes = roundup(unrounded_buffer_bytes, config.cache_line_bytes)?;

    let address_ring_bytes = u64::from(ring_slots)
        .checked_mul(u64::from(config.target_paddr_bytes))
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    let index_count = 1u64 + u64::from(config.has_target_index);
    let index_bytes = index_count
        .checked_mul(u64::from(HTT_RX_INDEX_BYTES))
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    let initial_rx_buffer_bytes = u64::from(initial_fill_level)
        .checked_mul(u64::from(rx_buffer_bytes))
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;
    let fallback_buffer_capacity_bytes = u64::from(config.preallocated_buffers)
        .checked_mul(u64::from(rx_buffer_bytes))
        .ok_or(HttRxProfileError::ArithmeticOverflow)?;

    Ok(HttRxProfile {
        ring_slots,
        initial_fill_level,
        rx_buffer_bytes,
        address_ring_bytes,
        index_bytes,
        initial_rx_buffer_bytes,
        fallback_buffer_capacity_bytes,
    })
}
