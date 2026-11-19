//! Capability-gated Adrastea CE register I/O for the Sunfish ICNSS window.
//!
//! This adapter supports bounded CE ring setup/snapshots, gated HTC-control
//! CE0 and WMI CE3 transmit, HTT CE4 transmit, CE1/CE2 receive posting, and
//! bounded completion copying.
//! It does not power WLAN, configure SMMU, allocate DMA, or bind interrupts.
//! No production Gate is wired.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::qcom_ce_v3::{self, MAX_CE_IOVA, RingCursor};
use crate::qcom_ce_v3_init::{EngineSnapshot, SetupWriter, SetupWriterError};
use crate::qcom_ce_v3_profile::{Profile, RingDirection, RingPlan};
use crate::qcom_ce_v3_regs::{self as regs, Register, RegisterIo};
use crate::qcom_htc::{HtcCreditLedger, HtcError};
use crate::qcom_wmi_ce::{
    HTT_DATA_DESTINATION_CE, HTT_DATA_SERVICE_ID, HTT_DATA_SOURCE_CE, RxBufferLedger,
    SensitiveTxBufferLedger, ServiceCeRxPostPlan, WmiCeRxPostPlan, WmiCeTxPlan,
};
use crate::sunfish_htt_rx_ring::{
    SunfishHttRxBufferPool, SunfishHttRxRefillError, SunfishHttRxRingIo, SunfishHttRxRingPlan,
    SunfishHttRxRingRefillState,
};
use crate::sunfish_qcacld_ce_tx::{CE4_DATA_DESCRIPTOR_COUNT, CeTxPlan, HTC_HTT_PREFIX_BYTES};
use crate::sunfish_qcacld_htc::wmi_control::{
    SunfishWmiControlSession, WmiControlEndpoint, WmiControlPhase, WmiControlSessionError,
};
use crate::sunfish_qcacld_htc::{self, HttStartupError, HttStartupSession};
use crate::sunfish_qcacld_mgmt_tx::{
    EncodedWmiMgmtTx, Error as MgmtTxError, MgmtTxLease, MgmtTxOutstanding,
};
use crate::sunfish_qcacld_rx_session::SunfishQcacldRxSession;
use crate::sunfish_qcacld_tx::{
    HELIUMPLUS_FRAGMENT_DESC_BYTES, HTT_DATA_HOST_TO_TARGET_CE, HTT_DATA_TARGET_TO_HOST_CE,
};
use crate::sunfish_smmu::{SUNFISH_MAX_CONTEXT_BANKS, SUNFISH_MAX_SMR_SLOTS};

/// Physical start of the Adrastea CE register block within ICNSS.
pub const CE_MMIO_PHYSICAL_BASE: u64 = 0x1880_0000 + regs::CE0_OFFSET as u64;
/// Twelve CE register pages; no other ICNSS registers are mapped by this adapter.
pub const CE_MMIO_BYTES: usize = regs::CE_COUNT * regs::CE_STRIDE;
pub const WLAN_SMMU_ROUTE_ID: u16 = 0x0240;
pub const WLAN_SMMU_ROUTE_MASK: u16 = 0x0001;
pub const WLAN_SIDS: [u16; 2] = [0x0240, 0x0241];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub fresh_boot: bool,
    pub exclusive: bool,
    pub platform_ready: bool,
    pub ce_owner_exclusive: bool,
    pub ce_powered: bool,
    pub ce_reset_released: bool,
    pub ce_quiescent: bool,
    /// Runtime binding for the exact GIC interrupt that delivers the WMI
    /// control receive-completion interrupt on CE2.
    pub ce_irq_intid: Option<u16>,
    pub ce_irq_bound: bool,
    pub ce_irq_exclusive: bool,
    /// Runtime binding for the HTT data-service CE1 receive interrupt.
    pub htt_ce_irq_intid: Option<u16>,
    pub htt_ce_irq_bound: bool,
    pub htt_ce_irq_exclusive: bool,
    /// Target-selected WMI_CONTROL_SVC routes tied to this fresh-boot
    /// generation; populated only after the owner validates the selected map.
    pub wmi_map_generation: u64,
    pub wmi_host_to_target_engine: Option<u8>,
    pub wmi_target_to_host_engine: Option<u8>,
    /// Target-selected HTC control-service routes tied to this boot.
    pub htc_control_map_generation: u64,
    pub htc_control_host_to_target_engine: Option<u8>,
    pub htc_control_target_to_host_engine: Option<u8>,
    /// Target-selected HTT_DATA_MSG_SVC routes tied to this fresh-boot
    /// generation; CE4 carries TX data and CE1 carries RX data on Sunfish.
    pub htt_map_generation: u64,
    pub htt_host_to_target_engine: Option<u8>,
    pub htt_target_to_host_engine: Option<u8>,
    /// Dynamic HTC endpoint bound to a validated HTT_DATA_MSG_SVC response.
    pub htt_endpoint_generation: u64,
    pub htt_endpoint_id: Option<u8>,
    pub htt_endpoint_max_message_size: u16,
    pub mmio_device_uncached: bool,
    /// Exact uncached virtual mapping of the CE register aperture only.
    pub ce_mmio_virtual: u64,
    pub ce_mmio_physical: u64,
    pub ce_mmio_bytes: u64,
    pub dma_virtual: u64,
    pub dma_iova: u64,
    pub dma_bytes: u64,
    pub dma_uncached: bool,
    pub dma_exclusive: bool,
    pub dma_translated: bool,
    pub smmu_route_slot: Option<u8>,
    pub smmu_route_id: u16,
    pub smmu_route_mask: u16,
    pub smmu_route_sids: [u16; 2],
    pub smmu_route_translated: bool,
    pub smmu_context_bank: Option<u8>,
    /// True only when the live context uses the planned S1-translated,
    /// S2-bypass format and SCTLR.M is set. A TRANS S2CR route by itself does
    /// not prove that the context performs translation.
    pub smmu_context_s1_translation_enabled: bool,
    pub smmu_context_enabled: bool,
}

pub trait Guard {
    fn observation(&self) -> Observation;
}

/// Native authority boundary for the CE-register mapping and DMA aperture.
///
/// # Safety
/// `lock` must pin the exact CE-register and DMA mappings reported by its guard and
/// keep exclusive, fresh-boot CE ownership stable until the guard is dropped.
/// It must serialize against reset, teardown, remapping, competing owners,
/// and any changes to the WLAN SMMU route/context. Its observation must come
/// from live authority state, not a manifest or cached probe value.
pub unsafe trait Gate {
    type Guard<'a>: Guard
    where
        Self: 'a;

    fn lock(&mut self) -> Result<Self::Guard<'_>, regs::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxPublishError {
    LeaseLost,
    InvalidPlan,
    ManagementTx(MgmtTxError),
    SensitiveTx(crate::qcom_wmi_ce::Error),
    HtcCredit(HtcError),
    Cursor(qcom_ce_v3::Error),
    Register(regs::Error),
    /// Descriptor(s) and producer index were written, but software bookkeeping
    /// could not be advanced. Callers must quarantine the queue.
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SensitiveWmiTxReclaimReport {
    pub descriptors_reclaimed: usize,
    pub sensitive_buffers_scrubbed: usize,
    pub sensitive_bytes_scrubbed: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RxPostError {
    LeaseLost,
    InvalidPlan,
    Cursor(qcom_ce_v3::Error),
    Ledger(crate::qcom_wmi_ce::Error),
    Register(regs::Error),
    /// The destination producer index was written but software bookkeeping
    /// could not be committed. Keep the buffer reserved and quarantine CE2.
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RxConsumeError {
    LeaseLost,
    InvalidPlan,
    NotReady,
    OutputTooSmall,
    Cursor(qcom_ce_v3::Error),
    Ledger(crate::qcom_wmi_ce::Error),
    Descriptor(qcom_ce_v3::Error),
    Register(regs::Error),
    /// Payload bytes were copied to the caller but software retirement failed.
    /// The receive queue must be quarantined and the caller must discard them.
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttConnectRxError {
    Consume(RxConsumeError),
    Startup(HttStartupError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiControlConnectRxError {
    Consume(RxConsumeError),
    Session(WmiControlSessionError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RxIrqError {
    LeaseLost,
    InvalidPlan,
    NotReady,
    Register(regs::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttRxRefillError {
    LeaseLost,
    InvalidPlan,
    Refill(SunfishHttRxRefillError<regs::Error>),
}

#[derive(Clone, Copy)]
pub struct CeMmioMapping {
    pub base: *mut u8,
    pub virtual_base: u64,
    pub physical_base: u64,
    pub bytes: usize,
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

fn authorized(observation: Observation, mapping: CeMmioMapping, generation: u64) -> bool {
    authorized_with_quiescence(observation, mapping, generation, true)
}

/// Validate stable owner, power, mapping, and SMMU authority for operations
/// that may run while CE is processing an already-published queue.
fn authorized_runtime(observation: Observation, mapping: CeMmioMapping, generation: u64) -> bool {
    authorized_with_quiescence(observation, mapping, generation, false)
}

fn authorized_with_quiescence(
    observation: Observation,
    mapping: CeMmioMapping,
    generation: u64,
    require_quiescent: bool,
) -> bool {
    let Some(dma_end) = observation.dma_iova.checked_add(observation.dma_bytes) else {
        return false;
    };
    generation != 0
        && observation.generation == generation
        && observation.fresh_boot
        && observation.exclusive
        && observation.platform_ready
        && observation.ce_owner_exclusive
        && observation.ce_powered
        && observation.ce_reset_released
        && (!require_quiescent || observation.ce_quiescent)
        && observation.mmio_device_uncached
        && observation.ce_mmio_virtual == mapping.virtual_base
        && observation.ce_mmio_physical == CE_MMIO_PHYSICAL_BASE
        && observation.ce_mmio_bytes == CE_MMIO_BYTES as u64
        && mapping.physical_base == CE_MMIO_PHYSICAL_BASE
        && mapping.bytes == CE_MMIO_BYTES
        && !mapping.base.is_null()
        && mapping.base as usize as u64 == mapping.virtual_base
        && mapping.virtual_base.is_multiple_of(4096)
        && (mapping.base as usize).is_multiple_of(4096)
        && observation.dma_virtual != 0
        && observation.dma_iova != 0
        && observation.dma_bytes != 0
        && observation.dma_uncached
        && observation.dma_exclusive
        && observation.dma_translated
        && observation.dma_iova.is_multiple_of(4096)
        && observation.dma_bytes.is_multiple_of(4096)
        && dma_end <= MAX_CE_IOVA
        && observation.dma_virtual.is_multiple_of(4096)
        && observation
            .dma_virtual
            .checked_add(observation.dma_bytes)
            .is_some()
        && !ranges_overlap(
            mapping.virtual_base,
            CE_MMIO_BYTES as u64,
            observation.dma_virtual,
            observation.dma_bytes,
        )
        && observation
            .smmu_route_slot
            .is_some_and(|slot| usize::from(slot) < SUNFISH_MAX_SMR_SLOTS)
        && observation.smmu_route_id == WLAN_SMMU_ROUTE_ID
        && observation.smmu_route_mask == WLAN_SMMU_ROUTE_MASK
        && observation.smmu_route_sids == WLAN_SIDS
        && observation.smmu_route_translated
        && observation
            .smmu_context_bank
            .is_some_and(|bank| usize::from(bank) < SUNFISH_MAX_CONTEXT_BANKS)
        && observation.smmu_context_s1_translation_enabled
        && observation.smmu_context_enabled
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccessKind {
    Read,
    InternalRead,
    Write,
    Publish,
    IrqControl,
    IrqAcknowledge,
}

#[derive(Clone, Copy)]
enum Ce2RxRoute {
    WmiControl,
    HtcControl,
}

fn register_access(offset: usize, kind: AccessKind) -> Result<(), regs::Error> {
    let relative = offset
        .checked_sub(regs::CE0_OFFSET)
        .ok_or(regs::Error::OutsideWindow)?;
    let engine = relative / regs::CE_STRIDE;
    let register = relative % regs::CE_STRIDE;
    if engine >= regs::CE_COUNT
        || register
            .checked_add(core::mem::size_of::<u32>())
            .is_none_or(|end| end > regs::CE_STRIDE)
        || relative
            .checked_add(core::mem::size_of::<u32>())
            .is_none_or(|end| end > CE_MMIO_BYTES)
    {
        return Err(regs::Error::OutsideWindow);
    }
    let allowed = match kind {
        AccessKind::Read => matches!(
            register,
            0x04 | 0x08 | 0x10 | 0x14 | 0x18 | 0x3c | 0x40 | 0x44 | 0x48 | 0x4c | 0x50
        ),
        AccessKind::InternalRead => matches!(
            register,
            0x00 | 0x04
                | 0x08
                | 0x0c
                | 0x10
                | 0x14
                | 0x18
                | 0x2c
                | 0x30
                | 0x3c
                | 0x40
                | 0x44
                | 0x48
                | 0x4c
                | 0x50
        ),
        AccessKind::Write => matches!(
            register,
            0x00 | 0x04 | 0x08 | 0x0c | 0x10 | 0x14 | 0x18 | 0x4c | 0x50
        ),
        AccessKind::Publish => {
            register == Register::SourceWriteIndex.relative_offset()
                || register == Register::DestinationWriteIndex.relative_offset()
        }
        AccessKind::IrqControl => register == Register::HostInterruptEnable.relative_offset(),
        AccessKind::IrqAcknowledge => register == Register::HostInterruptStatus.relative_offset(),
    };
    if !allowed {
        return Err(regs::Error::ReadOnly);
    }
    Ok(())
}

fn io_barrier() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: orders device-register accesses before/after volatile MMIO.
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

fn read32_mapping(mapping: CeMmioMapping, offset: usize) -> Result<u32, regs::Error> {
    register_access(offset, AccessKind::InternalRead)?;
    io_barrier();
    // SAFETY: the caller holds the live gate for this mapping and the
    // allowlist/bounds check proves this is a CE register word.
    let aperture_offset = offset - regs::CE0_OFFSET;
    let value =
        unsafe { core::ptr::read_volatile(mapping.base.add(aperture_offset).cast::<u32>()) };
    io_barrier();
    Ok(value)
}

fn write32_mapping(mapping: CeMmioMapping, offset: usize, value: u32) -> Result<(), regs::Error> {
    register_access(offset, AccessKind::Write)?;
    io_barrier();
    // SAFETY: the caller holds the live gate for this mapping and the
    // allowlist/bounds check proves this is a CE register word.
    unsafe {
        let aperture_offset = offset - regs::CE0_OFFSET;
        core::ptr::write_volatile(mapping.base.add(aperture_offset).cast::<u32>(), value);
    }
    io_barrier();
    Ok(())
}

fn publish_source_index_mapping(
    mapping: CeMmioMapping,
    offset: usize,
    value: u32,
) -> Result<(), regs::Error> {
    register_access(offset, AccessKind::Publish)?;
    io_barrier();
    // SAFETY: the caller holds the live gate, and the typed allowlist permits
    // only a CE source producer-index word for this publication operation.
    unsafe {
        core::ptr::write_volatile(
            mapping.base.add(offset - regs::CE0_OFFSET).cast::<u32>(),
            value,
        );
    }
    io_barrier();
    Ok(())
}

fn publish_destination_index_mapping(
    mapping: CeMmioMapping,
    offset: usize,
    value: u32,
) -> Result<(), regs::Error> {
    register_access(offset, AccessKind::Publish)?;
    io_barrier();
    // SAFETY: the caller holds the live gate, and the typed allowlist permits
    // only a CE destination producer-index word for this publication.
    unsafe {
        core::ptr::write_volatile(
            mapping.base.add(offset - regs::CE0_OFFSET).cast::<u32>(),
            value,
        );
    }
    io_barrier();
    Ok(())
}

fn set_ce_interrupt_enable_mapping(
    mapping: CeMmioMapping,
    engine: usize,
    enabled: bool,
) -> Result<(), regs::Error> {
    let offset = regs::register_spec(engine, Register::HostInterruptEnable)?.offset;
    register_access(offset, AccessKind::IrqControl)?;
    io_barrier();
    // The matched Sunfish SNOC implementation writes exactly 1 to this
    // individual per-engine register to enable, and 0 to disable.
    unsafe {
        core::ptr::write_volatile(
            mapping.base.add(offset - regs::CE0_OFFSET).cast::<u32>(),
            u32::from(enabled),
        );
    }
    io_barrier();
    Ok(())
}

fn acknowledge_host_status_mapping(
    mapping: CeMmioMapping,
    engine: usize,
    bits: u32,
) -> Result<(), regs::Error> {
    if bits == 0 || bits & !regs::HOST_INTERRUPT_STATUS_MASK != 0 {
        return Err(regs::Error::InvalidStatusMask);
    }
    let spec = regs::register_spec(engine, Register::HostInterruptStatus)?;
    let pending = read32_mapping(mapping, spec.offset)?;
    if bits & !pending != 0 {
        return Err(regs::Error::StatusNotPending);
    }
    register_access(spec.offset, AccessKind::IrqAcknowledge)?;
    io_barrier();
    // SAFETY: the caller holds the live CE owner gate, and the typed access
    // check restricts this write to the per-engine host-status W1C register.
    unsafe {
        core::ptr::write_volatile(
            mapping
                .base
                .add(spec.offset - regs::CE0_OFFSET)
                .cast::<u32>(),
            bits,
        );
    }
    io_barrier();
    Ok(())
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

fn dma_iova_contains(observation: Observation, address: u64, bytes: u64) -> bool {
    range_contains(observation.dma_iova, observation.dma_bytes, address, bytes)
}

fn dma_pointer(observation: Observation, address: u64, bytes: u64) -> Option<*mut u8> {
    if !dma_iova_contains(observation, address, bytes) {
        return None;
    }
    let offset = address.checked_sub(observation.dma_iova)?;
    let virtual_address = observation.dma_virtual.checked_add(offset)?;
    let virtual_end = virtual_address.checked_add(bytes)?;
    let dma_virtual_end = observation.dma_virtual.checked_add(observation.dma_bytes)?;
    if virtual_end > dma_virtual_end {
        return None;
    }
    Some(usize::try_from(virtual_address).ok()? as *mut u8)
}

fn ring_plan_valid_in_dma(observation: Observation, profile: Profile, rings: &RingPlan) -> bool {
    if rings.allocations().is_empty() {
        return false;
    }
    let mut expected_count = 0usize;
    for engine in 0..regs::CE_COUNT {
        let Ok(config) = profile.config(engine) else {
            return false;
        };
        expected_count += usize::from(config.source_entries != 0);
        expected_count += usize::from(config.destination_entries != 0);
    }
    if rings.allocations().len() != expected_count {
        return false;
    }
    for (index, allocation) in rings.allocations().iter().enumerate() {
        let Ok(config) = profile.config(usize::from(allocation.engine)) else {
            return false;
        };
        let expected_entries = match allocation.direction {
            RingDirection::Source => config.source_entries,
            RingDirection::Destination => config.destination_entries,
        };
        if expected_entries == 0
            || allocation.entries != expected_entries
            || allocation.descriptor_bytes
                != u32::from(expected_entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || allocation.allocation_bytes
                != allocation.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
            || !allocation
                .descriptor_iova
                .is_multiple_of(crate::qcom_ce_v3_profile::DMA_ALIGNMENT)
            || !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            )
        {
            return false;
        }
        for other in &rings.allocations()[index + 1..] {
            if ranges_overlap(
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
                other.descriptor_iova,
                u64::from(other.allocation_bytes),
            ) {
                return false;
            }
        }
    }
    true
}

fn source_ring(
    observation: Observation,
    profile: Profile,
    rings: &RingPlan,
    engine: usize,
    cursor_entries: usize,
) -> Option<crate::qcom_ce_v3_profile::RingAllocation> {
    if !ring_plan_valid_in_dma(observation, profile, rings) {
        return None;
    }
    let config = profile.config(engine).ok()?;
    let mut selected = None;
    for allocation in rings.allocations().iter().filter(|allocation| {
        usize::from(allocation.engine) == engine && allocation.direction == RingDirection::Source
    }) {
        if selected.replace(*allocation).is_some() {
            return None;
        }
    }
    let selected = selected?;
    if selected.entries != config.source_entries || usize::from(selected.entries) != cursor_entries
    {
        return None;
    }
    Some(selected)
}

fn is_staged_htt_connect_frame(frame: &[u8]) -> bool {
    let mut request = [0u8; 8];
    let Ok(request_len) = sunfish_qcacld_htc::encode_connect_request(&mut request) else {
        return false;
    };
    let mut expected = [0u8; 16];
    let Ok(frame_len) =
        crate::qcom_htc::encode_control_frame(&request[..request_len], &mut expected)
    else {
        return false;
    };
    frame == &expected[..frame_len]
}

fn is_staged_wmi_control_connect_frame(frame: &[u8]) -> bool {
    if frame.len() != 16
        || frame[..8] != [0, 0, 8, 0, 0, 0, 0, 0]
        || frame[8..12] != [2, 0, 0, 1]
        || frame[14..16] != [0, 0]
    {
        return false;
    }
    let connection_flags = u16::from_le_bytes([frame[12], frame[13]]);
    // Sunfish's pinned HTC setup has one WMI endpoint. It assigns all READY
    // credits to that service in bits 15:8 and enables no optional flags.
    connection_flags != 0 && connection_flags & 0x00ff == 0
}

fn is_staged_setup_complete_frame(frame: &[u8]) -> bool {
    frame
        == [
            0, 0, 12, 0, 0, 0, 0, 0, // endpoint 0, 12-byte payload
            5, 0, 0, 0, // HTC_SETUP_COMPLETE_EX
            0, 0, 0, 0, // zero setup flags and reserved word
            0, 0, 0, 0, // zero reserved tail
        ]
}

fn is_staged_htc_control_frame(frame: &[u8]) -> bool {
    is_staged_htt_connect_frame(frame)
        || is_staged_wmi_control_connect_frame(frame)
        || is_staged_setup_complete_frame(frame)
}

fn ring_slot_iova(ring: crate::qcom_ce_v3_profile::RingAllocation, index: usize) -> Option<u64> {
    if index >= usize::from(ring.entries) {
        return None;
    }
    let offset = u64::try_from(index)
        .ok()?
        .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)?;
    let address = ring.descriptor_iova.checked_add(offset)?;
    range_contains(
        ring.descriptor_iova,
        u64::from(ring.descriptor_bytes),
        address,
        qcom_ce_v3::DESCRIPTOR_BYTES as u64,
    )
    .then_some(address)
}

fn copy_to_dma(destination: *mut u8, bytes: &[u8]) {
    for (index, byte) in bytes.iter().copied().enumerate() {
        // SAFETY: caller resolved this pointer through the live, bounded DMA
        // aperture and holds its exclusive uncached mapping lease.
        unsafe { destination.add(index).write_volatile(byte) };
    }
}

fn zero_dma(destination: *mut u8, bytes: usize) {
    for index in 0..bytes {
        // SAFETY: caller resolved this pointer through the live, bounded DMA
        // aperture and holds its exclusive uncached mapping lease.
        unsafe { destination.add(index).write_volatile(0) };
    }
}

fn copy_from_dma(source: *const u8, bytes: &mut [u8]) {
    for (index, byte) in bytes.iter_mut().enumerate() {
        // SAFETY: caller resolved this pointer through the live, bounded DMA
        // aperture and holds its exclusive uncached mapping lease.
        *byte = unsafe { source.add(index).read_volatile() };
    }
}

fn snapshot_mapping(
    mapping: CeMmioMapping,
    engine: usize,
    generation: u64,
) -> Result<EngineSnapshot, regs::Error> {
    let read = |register| {
        let spec = regs::register_spec(engine, register)?;
        read32_mapping(mapping, spec.offset)
    };
    Ok(EngineSnapshot {
        generation,
        source_size: read(Register::SourceSize)?,
        destination_size: read(Register::DestinationSize)?,
        source_read_index: read(Register::CurrentSourceReadIndex)?,
        source_write_index: read(Register::SourceWriteIndex)?,
        destination_read_index: read(Register::CurrentDestinationReadIndex)?,
        destination_write_index: read(Register::DestinationWriteIndex)?,
        source_base_high: read(Register::SourceBaseHigh)?,
        destination_base_high: read(Register::DestinationBaseHigh)?,
        control1: read(Register::Control1)?,
        source_watermark: read(Register::SourceWatermark)?,
        destination_watermark: read(Register::DestinationWatermark)?,
    })
}

pub struct NativeIo<G: Gate> {
    mapping: CeMmioMapping,
    generation: u64,
    gate: G,
}

struct NativeHttRxRingIo<'a, T: Guard> {
    plan: SunfishHttRxRingPlan,
    observation: Observation,
    _authority: &'a T,
}

impl<T: Guard> NativeHttRxRingIo<'_, T> {
    fn buffer_iova_is_planned(&self, address: u64, bytes: usize) -> bool {
        if bytes != self.plan.buffer_bytes() as usize {
            return false;
        }
        let Some(count) = self
            .plan
            .initial_fill_level()
            .checked_add(self.plan.fallback_buffer_count())
        else {
            return false;
        };
        (0..count).any(|buffer_id| self.plan.buffer_iova(buffer_id) == Ok(address))
    }

    fn ring_entry_is_planned(&self, address: u64) -> bool {
        let ring = self.plan.memory().ring_iova;
        let Some(bytes) = u64::from(self.plan.ring_slots()).checked_mul(8) else {
            return false;
        };
        address
            .checked_sub(ring)
            .is_some_and(|offset| offset < bytes && offset.is_multiple_of(8))
    }
}

impl<T: Guard> SunfishHttRxRingIo for NativeHttRxRingIo<'_, T> {
    type Error = regs::Error;

    fn read_alloc_index(&mut self, address: u64) -> Result<u32, Self::Error> {
        if address != self.plan.memory().alloc_index_iova {
            return Err(regs::Error::OutsideWindow);
        }
        let pointer =
            dma_pointer(self.observation, address, 4).ok_or(regs::Error::OutsideWindow)?;
        let mut bytes = [0u8; 4];
        io_barrier();
        copy_from_dma(pointer, &mut bytes);
        io_barrier();
        Ok(u32::from_le_bytes(bytes))
    }

    fn sync_buffer_for_device(&mut self, address: u64, bytes: usize) -> Result<(), Self::Error> {
        if !self.buffer_iova_is_planned(address, bytes)
            || dma_pointer(self.observation, address, bytes as u64).is_none()
        {
            return Err(regs::Error::OutsideWindow);
        }
        // The live gate only admits an uncached DMA aperture, so cache clean
        // is unnecessary; the single DSB in dma_write_barrier orders all
        // volatile buffer and ring stores before allocation-index publish.
        Ok(())
    }

    fn write_ring_entry(&mut self, address: u64, buffer_iova: u64) -> Result<(), Self::Error> {
        if !self.ring_entry_is_planned(address)
            || !self.buffer_iova_is_planned(buffer_iova, self.plan.buffer_bytes() as usize)
        {
            return Err(regs::Error::OutsideWindow);
        }
        let pointer =
            dma_pointer(self.observation, address, 8).ok_or(regs::Error::OutsideWindow)?;
        copy_to_dma(pointer, &buffer_iova.to_le_bytes());
        Ok(())
    }

    fn dma_write_barrier(&mut self) {
        io_barrier();
    }

    fn write_alloc_index(&mut self, address: u64, next: u32) -> Result<(), Self::Error> {
        if address != self.plan.memory().alloc_index_iova || next >= self.plan.ring_slots() {
            return Err(regs::Error::OutsideWindow);
        }
        let pointer =
            dma_pointer(self.observation, address, 4).ok_or(regs::Error::OutsideWindow)?;
        copy_to_dma(pointer, &next.to_le_bytes());
        io_barrier();
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum ReceiveInterruptService {
    WmiControl,
    HttData,
}

impl ReceiveInterruptService {
    const fn engine(self) -> usize {
        match self {
            Self::WmiControl => 2,
            Self::HttData => HTT_DATA_DESTINATION_CE as usize,
        }
    }

    fn route_ready(self, observation: Observation, generation: u64) -> bool {
        match self {
            Self::WmiControl => {
                observation.wmi_map_generation == generation
                    && observation.wmi_host_to_target_engine == Some(3)
                    && observation.wmi_target_to_host_engine == Some(2)
            }
            Self::HttData => {
                observation.htt_map_generation == generation
                    && observation.htt_host_to_target_engine == Some(HTT_DATA_SOURCE_CE)
                    && observation.htt_target_to_host_engine == Some(HTT_DATA_DESTINATION_CE)
                    && observation.htt_endpoint_generation == generation
                    && observation.htt_endpoint_id.is_some()
                    && observation.htt_endpoint_max_message_size != 0
            }
        }
    }

    fn irq_binding(self, observation: Observation) -> (Option<u16>, bool, bool) {
        match self {
            Self::WmiControl => (
                observation.ce_irq_intid,
                observation.ce_irq_bound,
                observation.ce_irq_exclusive,
            ),
            Self::HttData => (
                observation.htt_ce_irq_intid,
                observation.htt_ce_irq_bound,
                observation.htt_ce_irq_exclusive,
            ),
        }
    }
}

impl<G: Gate> NativeIo<G> {
    /// # Safety
    /// `mapping.base` must be the exact uncached 48 KiB CE-register mapping at
    /// physical base `0x18a40000`, remain valid for this object's lifetime, and be
    /// pinned by `gate` for every register access.
    pub unsafe fn new(
        mapping: CeMmioMapping,
        generation: u64,
        mut gate: G,
    ) -> Result<Self, regs::Error> {
        if mapping.base.is_null()
            || mapping.base as usize as u64 != mapping.virtual_base
            || mapping.physical_base != CE_MMIO_PHYSICAL_BASE
            || mapping.bytes != CE_MMIO_BYTES
            || !mapping.virtual_base.is_multiple_of(4096)
            || !(mapping.base as usize).is_multiple_of(4096)
        {
            return Err(regs::Error::Io);
        }
        let guard = gate.lock()?;
        if !authorized(guard.observation(), mapping, generation) {
            return Err(regs::Error::Io);
        }
        drop(guard);
        Ok(Self {
            mapping,
            generation,
            gate,
        })
    }

    /// Arm or disarm the source-backed per-engine CE2 receive interrupt.
    /// Enabling requires a live, exclusive binding for Sunfish CE2's decoded
    /// GIC INTID and at least one posted receive descriptor. This only writes
    /// the individual CE HostInterruptEnable register (1/0); it does not
    /// acknowledge GIC state or claim an IRQ capability.
    pub fn set_wmi_rx_irq(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &RingCursor,
        enabled: bool,
    ) -> Result<(), RxIrqError> {
        self.set_receive_irq(
            generation,
            profile,
            rings,
            cursor,
            ReceiveInterruptService::WmiControl,
            enabled,
        )
    }

    /// Arm or disarm the HTT data-service CE1 receive interrupt. Enabling
    /// requires the live `0x0300` route, its negotiated HTC endpoint, an
    /// exclusive binding for Sunfish CE1's GIC INTID, and a posted receive
    /// descriptor. This writes only CE1's copy-complete enable bit; it does
    /// not acknowledge the GIC interrupt or claim an IRQ capability.
    pub fn set_htt_rx_ce1_irq(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &RingCursor,
        enabled: bool,
    ) -> Result<(), RxIrqError> {
        self.set_receive_irq(
            generation,
            profile,
            rings,
            cursor,
            ReceiveInterruptService::HttData,
            enabled,
        )
    }

    /// Clear CE2's WMI-control copy-complete status after the caller has
    /// drained the corresponding receive completions. This acknowledges only
    /// the fresh, observed W1C bit under the same-generation WMI route and
    /// exclusive CE2 IRQ binding; it does not acknowledge the seL4 GIC IRQ.
    pub fn acknowledge_wmi_rx_ce2_copy_complete(
        &mut self,
        generation: u64,
    ) -> Result<(), RxIrqError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxIrqError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
        {
            return Err(RxIrqError::LeaseLost);
        }
        let service = ReceiveInterruptService::WmiControl;
        let expected_irq = regs::gic_intid(service.engine()).map_err(RxIrqError::Register)?;
        let (irq_intid, irq_bound, irq_exclusive) = service.irq_binding(observation);
        if !service.route_ready(observation, generation)
            || irq_intid != Some(expected_irq)
            || !irq_bound
            || !irq_exclusive
        {
            return Err(RxIrqError::LeaseLost);
        }
        acknowledge_host_status_mapping(
            mapping,
            service.engine(),
            regs::HOST_INTERRUPT_COPY_COMPLETE_MASK,
        )
        .map_err(RxIrqError::Register)?;
        drop(guard);
        Ok(())
    }

    /// Clear CE1's copy-complete status after the caller has serviced the
    /// corresponding receive completions. This acknowledges only the fresh,
    /// observed `HOST_IS_COPY_COMPLETE_MASK` bit; it does not acknowledge the
    /// seL4 GIC interrupt or dispatch receive work.
    pub fn acknowledge_htt_rx_ce1_copy_complete(
        &mut self,
        generation: u64,
    ) -> Result<(), RxIrqError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxIrqError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
        {
            return Err(RxIrqError::LeaseLost);
        }
        let service = ReceiveInterruptService::HttData;
        let expected_irq = regs::gic_intid(service.engine()).map_err(RxIrqError::Register)?;
        let (irq_intid, irq_bound, irq_exclusive) = service.irq_binding(observation);
        if !service.route_ready(observation, generation)
            || irq_intid != Some(expected_irq)
            || !irq_bound
            || !irq_exclusive
        {
            return Err(RxIrqError::LeaseLost);
        }
        acknowledge_host_status_mapping(
            mapping,
            service.engine(),
            regs::HOST_INTERRUPT_COPY_COMPLETE_MASK,
        )
        .map_err(RxIrqError::Register)?;
        drop(guard);
        Ok(())
    }

    fn set_receive_irq(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &RingCursor,
        service: ReceiveInterruptService,
        enabled: bool,
    ) -> Result<(), RxIrqError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxIrqError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
        {
            return Err(RxIrqError::LeaseLost);
        }
        let engine = service.engine();
        let expected_irq = regs::gic_intid(engine).map_err(RxIrqError::Register)?;
        let (irq_intid, irq_bound, irq_exclusive) = service.irq_binding(observation);
        if !service.route_ready(observation, generation)
            || irq_intid != Some(expected_irq)
            || !irq_bound
            || !irq_exclusive
        {
            return Err(RxIrqError::LeaseLost);
        }

        let config = profile
            .config(engine)
            .map_err(|_| RxIrqError::InvalidPlan)?;
        let mut selected_ring = None;
        for allocation in rings.allocations() {
            if !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxIrqError::InvalidPlan);
            }
            if usize::from(allocation.engine) == engine
                && allocation.direction == RingDirection::Destination
            {
                if selected_ring.replace(*allocation).is_some() {
                    return Err(RxIrqError::InvalidPlan);
                }
            }
        }
        let ring = selected_ring.ok_or(RxIrqError::InvalidPlan)?;
        if ring.entries != config.destination_entries
            || usize::from(ring.entries) != cursor.entries()
            || ring.descriptor_bytes
                != u32::from(ring.entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || ring.allocation_bytes
                != ring.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
        {
            return Err(RxIrqError::InvalidPlan);
        }
        for (index, allocation) in rings.allocations().iter().enumerate() {
            for other in &rings.allocations()[index + 1..] {
                if ranges_overlap(
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                    other.descriptor_iova,
                    u64::from(other.allocation_bytes),
                ) {
                    return Err(RxIrqError::InvalidPlan);
                }
            }
        }
        if enabled && cursor.pending() == 0 {
            return Err(RxIrqError::NotReady);
        }

        let read_index = regs::register_spec(engine, Register::CurrentDestinationReadIndex)
            .map_err(RxIrqError::Register)?;
        let producer_index = regs::register_spec(engine, Register::DestinationWriteIndex)
            .map_err(RxIrqError::Register)?;
        let destination_size =
            regs::register_spec(engine, Register::DestinationSize).map_err(RxIrqError::Register)?;
        let destination_base_low = regs::register_spec(engine, Register::DestinationBaseLow)
            .map_err(RxIrqError::Register)?;
        let destination_base_high = regs::register_spec(engine, Register::DestinationBaseHigh)
            .map_err(RxIrqError::Register)?;
        let host_interrupt_enable = regs::register_spec(engine, Register::HostInterruptEnable)
            .map_err(RxIrqError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(RxIrqError::Register)? as usize;
        let live_producer =
            read32_mapping(mapping, producer_index.offset).map_err(RxIrqError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, destination_size.offset).map_err(RxIrqError::Register)?;
        let live_base_low =
            read32_mapping(mapping, destination_base_low.offset).map_err(RxIrqError::Register)?;
        let live_base_high =
            read32_mapping(mapping, destination_base_high.offset).map_err(RxIrqError::Register)?;
        if live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
            || cursor.rx_completions_to(live_read).is_err()
        {
            return Err(RxIrqError::LeaseLost);
        }
        let live_enable =
            read32_mapping(mapping, host_interrupt_enable.offset).map_err(RxIrqError::Register)?;
        if live_enable > 1 {
            return Err(RxIrqError::InvalidPlan);
        }
        if live_enable != u32::from(enabled) {
            set_ce_interrupt_enable_mapping(mapping, engine, enabled)
                .map_err(RxIrqError::Register)?;
        }
        drop(guard);
        Ok(())
    }

    /// Publish one already-framed WMI packet through the CE3 source ring.
    /// This is separate from generic register writes: it checks live ring
    /// geometry and cursors under one gate, copies packet and descriptor bytes
    /// into the translated uncached DMA mapping, orders those stores, then
    /// publishes only CE3's source producer index. It does not grant WLAN
    /// power, configure DMA, or bind an interrupt.
    pub fn publish_wmi_tx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: WmiCeTxPlan<'_>,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        self.publish_wmi_tx_inner(generation, profile, rings, plan, None, cursor)
    }

    /// Erase a sensitive WMI buffer that has a pre-publication ledger entry
    /// but whose CE3 ticket is still the next unpublished slot. This is for a
    /// definite failure before producer-index publication; an uncertain
    /// publication must stay quarantined and use the read-index path instead.
    pub fn scrub_unpublished_sensitive_wmi_tx<const N: usize>(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &RingCursor,
        sensitive: &mut SensitiveTxBufferLedger<N>,
        lease: crate::qcom_wmi_ce::SensitiveTxBufferLease,
    ) -> Result<(), TxPublishError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| TxPublishError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || observation.wmi_map_generation != generation
            || observation.wmi_host_to_target_engine != Some(3)
            || observation.wmi_target_to_host_engine != Some(2)
        {
            return Err(TxPublishError::LeaseLost);
        }
        if lease.generation() != generation
            || !sensitive.active().any(|active| active == lease)
            || sensitive
                .active()
                .any(|active| active.generation() != generation)
        {
            return Err(TxPublishError::SensitiveTx(
                crate::qcom_wmi_ce::Error::SensitiveTxBufferNotReserved,
            ));
        }
        if cursor.prepare_next().map_err(TxPublishError::Cursor)? != lease.ticket() {
            return Err(TxPublishError::SensitiveTx(
                crate::qcom_wmi_ce::Error::Cursor(qcom_ce_v3::Error::StaleTicket),
            ));
        }
        let ring = source_ring(observation, profile, rings, 3, cursor.entries())
            .ok_or(TxPublishError::InvalidPlan)?;
        let read_index = regs::register_spec(3, Register::CurrentSourceReadIndex)
            .map_err(TxPublishError::Register)?;
        let producer_index =
            regs::register_spec(3, Register::SourceWriteIndex).map_err(TxPublishError::Register)?;
        let source_size =
            regs::register_spec(3, Register::SourceSize).map_err(TxPublishError::Register)?;
        let source_base_low =
            regs::register_spec(3, Register::SourceBaseLow).map_err(TxPublishError::Register)?;
        let source_base_high =
            regs::register_spec(3, Register::SourceBaseHigh).map_err(TxPublishError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(TxPublishError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(TxPublishError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, source_size.offset).map_err(TxPublishError::Register)?;
        let live_base_low =
            read32_mapping(mapping, source_base_low.offset).map_err(TxPublishError::Register)?;
        let live_base_high =
            read32_mapping(mapping, source_base_high.offset).map_err(TxPublishError::Register)?;
        cursor
            .preview_reclaim_tx_to(live_read)
            .map_err(TxPublishError::Cursor)?;
        io_barrier();
        if live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(TxPublishError::LeaseLost);
        }
        let buffer = lease.buffer();
        if !dma_iova_contains(observation, buffer.iova, u64::from(buffer.capacity))
            || rings.allocations().iter().any(|allocation| {
                ranges_overlap(
                    buffer.iova,
                    u64::from(buffer.capacity),
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                )
            })
        {
            return Err(TxPublishError::InvalidPlan);
        }
        let pointer = dma_pointer(observation, buffer.iova, u64::from(buffer.capacity))
            .ok_or(TxPublishError::InvalidPlan)?;
        // SAFETY: the live gate pins an exclusive uncached DMA mapping, and
        // the producer-index check proves this buffer is not device-owned.
        unsafe {
            core::ptr::write_bytes(pointer, 0, buffer.capacity as usize);
        }
        io_barrier();
        let removed = sensitive
            .cancel_unpublished(generation, lease.ticket())
            .map_err(TxPublishError::SensitiveTx)?;
        if removed != lease {
            return Err(TxPublishError::Uncertain);
        }
        drop(guard);
        Ok(())
    }

    /// Publish only the staged HTT_DATA_MSG_SVC connection request on the
    /// pinned HTC control endpoint over CE0. The control route and all DMA/MMIO
    /// authority must be live for this generation. No other control payload
    /// can pass the canonical-frame check.
    pub fn publish_htt_connect_request(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        session: &mut HttStartupSession,
        frame_iova: u64,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        let frame =
            session
                .staged_connect_request_frame(generation)
                .map_err(|error| match error {
                    HttStartupError::WrongGeneration { .. } => TxPublishError::LeaseLost,
                    _ => TxPublishError::InvalidPlan,
                })?;
        match self
            .publish_htc_control_tx_inner(generation, profile, rings, frame_iova, frame, cursor)
        {
            Ok(()) => session
                .request_tx_submitted(generation)
                .map_err(|_| TxPublishError::Uncertain),
            Err(TxPublishError::Uncertain) => {
                session
                    .request_tx_uncertain(generation)
                    .map_err(|_| TxPublishError::Uncertain)?;
                Err(TxPublishError::Uncertain)
            }
            Err(error) => {
                session
                    .request_tx_failed(generation)
                    .map_err(|_| TxPublishError::Uncertain)?;
                Err(error)
            }
        }
    }

    /// Publish only the staged WMI_CONTROL_SVC connection request on the
    /// pinned HTC control endpoint over CE0. A definite pre-publication error
    /// restores the session to Ready; uncertain producer publication
    /// quarantines it.
    pub fn publish_wmi_control_connect_request(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        session: &mut SunfishWmiControlSession,
        frame_iova: u64,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        self.publish_wmi_control_frame(
            generation,
            profile,
            rings,
            session,
            WmiControlPhase::ConnectStaged,
            frame_iova,
            cursor,
        )
    }

    /// Publish the staged HTC_SETUP_COMPLETE_EX only after WMI_CONTROL_SVC
    /// has been assigned a valid endpoint. The WMI INIT data command remains
    /// a later CE3 operation.
    pub fn publish_wmi_setup_complete(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        session: &mut SunfishWmiControlSession,
        frame_iova: u64,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        self.publish_wmi_control_frame(
            generation,
            profile,
            rings,
            session,
            WmiControlPhase::SetupStaged,
            frame_iova,
            cursor,
        )
    }

    fn publish_wmi_control_frame(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        session: &mut SunfishWmiControlSession,
        expected_phase: WmiControlPhase,
        frame_iova: u64,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        if session.phase() != expected_phase {
            return Err(TxPublishError::InvalidPlan);
        }
        let staged = session
            .staged_control_frame(generation)
            .map_err(|error| match error {
                WmiControlSessionError::WrongGeneration { .. } => TxPublishError::LeaseLost,
                _ => TxPublishError::InvalidPlan,
            })?;
        let mut frame = [0u8; 20];
        let frame_len = staged.len();
        if frame_len > frame.len() {
            return Err(TxPublishError::InvalidPlan);
        }
        frame[..frame_len].copy_from_slice(staged);
        match self.publish_htc_control_tx_inner(
            generation,
            profile,
            rings,
            frame_iova,
            &frame[..frame_len],
            cursor,
        ) {
            Ok(()) => session
                .control_tx_submitted(generation)
                .map(|_| ())
                .map_err(|_| TxPublishError::Uncertain),
            Err(TxPublishError::Uncertain) => {
                session
                    .control_tx_uncertain(generation)
                    .map_err(|_| TxPublishError::Uncertain)?;
                Err(TxPublishError::Uncertain)
            }
            Err(error) => {
                session
                    .control_tx_failed(generation)
                    .map_err(|_| TxPublishError::Uncertain)?;
                Err(error)
            }
        }
    }

    fn publish_htc_control_tx_inner(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        frame_iova: u64,
        frame: &[u8],
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| TxPublishError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || observation.htc_control_map_generation != generation
            || observation.htc_control_host_to_target_engine != Some(0)
            || observation.htc_control_target_to_host_engine != Some(2)
        {
            return Err(TxPublishError::LeaseLost);
        }
        if !is_staged_htc_control_frame(frame) || !frame_iova.is_multiple_of(8) {
            return Err(TxPublishError::InvalidPlan);
        }

        let config = profile.config(0).map_err(|_| TxPublishError::InvalidPlan)?;
        let ring = source_ring(observation, profile, rings, 0, cursor.entries())
            .ok_or(TxPublishError::InvalidPlan)?;
        if ring.entries != config.source_entries
            || ring.descriptor_bytes
                != u32::from(ring.entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || ring.allocation_bytes
                != ring.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
            || frame.len() > usize::from(config.source_max_bytes)
            || !matches!(frame.len(), 16 | 20)
        {
            return Err(TxPublishError::InvalidPlan);
        }
        for allocation in rings.allocations() {
            if ranges_overlap(
                frame_iova,
                frame.len() as u64,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(TxPublishError::InvalidPlan);
            }
        }

        let ticket = cursor.prepare_next().map_err(TxPublishError::Cursor)?;
        let next_producer = cursor
            .preview_publish(ticket)
            .map_err(TxPublishError::Cursor)?;
        let slot_iova = ring_slot_iova(ring, ticket.index()).ok_or(TxPublishError::InvalidPlan)?;
        let descriptor = qcom_ce_v3::TxDescriptor::encode(
            frame_iova,
            u32::try_from(frame.len()).map_err(|_| TxPublishError::InvalidPlan)?,
            0,
            false,
            false,
        )
        .map_err(|_| TxPublishError::InvalidPlan)?;
        let frame_pointer = dma_pointer(observation, frame_iova, frame.len() as u64)
            .ok_or(TxPublishError::InvalidPlan)?;
        let descriptor_pointer =
            dma_pointer(observation, slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(TxPublishError::InvalidPlan)?;

        let read_index = regs::register_spec(0, Register::CurrentSourceReadIndex)
            .map_err(TxPublishError::Register)?;
        let producer_index =
            regs::register_spec(0, Register::SourceWriteIndex).map_err(TxPublishError::Register)?;
        let source_size =
            regs::register_spec(0, Register::SourceSize).map_err(TxPublishError::Register)?;
        let source_base_low =
            regs::register_spec(0, Register::SourceBaseLow).map_err(TxPublishError::Register)?;
        let source_base_high =
            regs::register_spec(0, Register::SourceBaseHigh).map_err(TxPublishError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(TxPublishError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(TxPublishError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, source_size.offset).map_err(TxPublishError::Register)?;
        let live_base_low =
            read32_mapping(mapping, source_base_low.offset).map_err(TxPublishError::Register)?;
        let live_base_high =
            read32_mapping(mapping, source_base_high.offset).map_err(TxPublishError::Register)?;
        if live_read != cursor.consumer_index()
            || live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(TxPublishError::LeaseLost);
        }

        copy_to_dma(frame_pointer, frame);
        copy_to_dma(descriptor_pointer, descriptor.as_bytes());
        io_barrier();
        publish_source_index_mapping(mapping, producer_index.offset, next_producer as u32)
            .map_err(TxPublishError::Register)?;
        if cursor.commit_publish(ticket) != Ok(next_producer) {
            return Err(TxPublishError::Uncertain);
        }
        drop(guard);
        Ok(())
    }

    /// Stage a by-reference QCACLD management frame and its WMI command under
    /// one native CE/DMA lease, then publish CE3. The frame lease is reserved
    /// before the producer write so an immediate firmware completion cannot
    /// race ahead of software ownership tracking. Definite pre-publication
    /// errors cancel that reservation; an uncertain publication retains it.
    pub fn publish_qcacld_mgmt_tx<const N: usize>(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        encoded: &EncodedWmiMgmtTx,
        plan: WmiCeTxPlan<'_>,
        frame_iova: u64,
        frame: &[u8],
        outstanding: &mut MgmtTxOutstanding<N>,
        credits: &mut HtcCreditLedger,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        let command = encoded.command();
        if generation != command.generation()
            || plan.frame() != encoded.frame()
            || !encoded.references_frame(frame_iova, frame)
        {
            return Err(TxPublishError::InvalidPlan);
        }
        let lease = MgmtTxLease {
            generation,
            descriptor_id: u32::from(command.descriptor_id()),
            frame_iova,
            frame_bytes: command.frame_bytes(),
        };
        let credit_plan = encoded.wmi_plan();
        credits
            .reserve(generation, credit_plan)
            .map_err(TxPublishError::HtcCredit)?;
        if let Err(error) = outstanding.reserve(lease) {
            if credits
                .cancel_unpublished(generation, credit_plan.credits_required)
                .is_err()
            {
                return Err(TxPublishError::Uncertain);
            }
            return Err(TxPublishError::ManagementTx(error));
        }

        match self.publish_wmi_tx_inner(
            generation,
            profile,
            rings,
            plan,
            Some((frame_iova, frame)),
            cursor,
        ) {
            Ok(()) => Ok(()),
            Err(TxPublishError::Uncertain) => Err(TxPublishError::Uncertain),
            Err(error) => {
                let lease_cancelled = outstanding
                    .cancel_unpublished(generation, lease.descriptor_id)
                    .is_ok_and(|cancelled| cancelled == lease);
                let credits_cancelled = credits
                    .cancel_unpublished(generation, credit_plan.credits_required)
                    .is_ok();
                if lease_cancelled && credits_cancelled {
                    Err(error)
                } else {
                    Err(TxPublishError::Uncertain)
                }
            }
        }
    }

    fn publish_wmi_tx_inner(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: WmiCeTxPlan<'_>,
        additional_payload: Option<(u64, &[u8])>,
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| TxPublishError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
        {
            return Err(TxPublishError::LeaseLost);
        }
        if observation.wmi_map_generation != generation
            || observation.wmi_host_to_target_engine != Some(3)
            || observation.wmi_target_to_host_engine != Some(2)
        {
            return Err(TxPublishError::LeaseLost);
        }
        let route = plan.route();
        let frame = plan.frame();
        let descriptor = plan.descriptor();
        if route.host_to_target_engine != 3 || route.target_to_host_engine != 2 {
            return Err(TxPublishError::InvalidPlan);
        }
        let config = profile.config(3).map_err(|_| TxPublishError::InvalidPlan)?;
        let mut selected_ring = None;
        for allocation in rings.allocations() {
            if !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(TxPublishError::InvalidPlan);
            }
            if allocation.engine == 3 && allocation.direction == RingDirection::Source {
                if selected_ring.replace(*allocation).is_some() {
                    return Err(TxPublishError::InvalidPlan);
                }
            }
        }
        let ring = selected_ring.ok_or(TxPublishError::InvalidPlan)?;
        if ring.entries != config.source_entries
            || usize::from(ring.entries) != cursor.entries()
            || ring.descriptor_bytes
                != u32::from(ring.entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || ring.allocation_bytes
                != ring.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
            || frame.len() != usize::from(plan.frame_bytes())
            || frame.len() != usize::from(descriptor.nbytes())
            || frame.len() > usize::from(config.source_max_bytes)
            || descriptor.address() & 7 != 0
        {
            return Err(TxPublishError::InvalidPlan);
        }
        for (index, allocation) in rings.allocations().iter().enumerate() {
            for other in &rings.allocations()[index + 1..] {
                if ranges_overlap(
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                    other.descriptor_iova,
                    u64::from(other.allocation_bytes),
                ) {
                    return Err(TxPublishError::InvalidPlan);
                }
            }
            if ranges_overlap(
                descriptor.address(),
                frame.len() as u64,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(TxPublishError::InvalidPlan);
            }
        }

        let ticket = cursor.prepare_next().map_err(TxPublishError::Cursor)?;
        if ticket != plan.ticket() {
            return Err(TxPublishError::InvalidPlan);
        }
        let next_producer = cursor
            .preview_publish(ticket)
            .map_err(TxPublishError::Cursor)?;
        if next_producer != plan.next_producer_index() {
            return Err(TxPublishError::InvalidPlan);
        }
        let slot_offset = u64::try_from(ticket.index())
            .map_err(|_| TxPublishError::InvalidPlan)?
            .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
            .ok_or(TxPublishError::InvalidPlan)?;
        let slot_iova = ring
            .descriptor_iova
            .checked_add(slot_offset)
            .ok_or(TxPublishError::InvalidPlan)?;
        if slot_iova != plan.descriptor_slot_iova()
            || !range_contains(
                ring.descriptor_iova,
                u64::from(ring.descriptor_bytes),
                slot_iova,
                qcom_ce_v3::DESCRIPTOR_BYTES as u64,
            )
            || ranges_overlap(
                descriptor.address(),
                frame.len() as u64,
                slot_iova,
                qcom_ce_v3::DESCRIPTOR_BYTES as u64,
            )
        {
            return Err(TxPublishError::InvalidPlan);
        }
        let packet_pointer = dma_pointer(observation, descriptor.address(), frame.len() as u64)
            .ok_or(TxPublishError::InvalidPlan)?;
        let descriptor_pointer =
            dma_pointer(observation, slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(TxPublishError::InvalidPlan)?;
        let additional_payload = if let Some((address, bytes)) = additional_payload {
            let payload_bytes =
                u64::try_from(bytes.len()).map_err(|_| TxPublishError::InvalidPlan)?;
            if bytes.is_empty()
                || !address.is_multiple_of(8)
                || ranges_overlap(
                    address,
                    payload_bytes,
                    descriptor.address(),
                    frame.len() as u64,
                )
                || ranges_overlap(
                    address,
                    payload_bytes,
                    slot_iova,
                    qcom_ce_v3::DESCRIPTOR_BYTES as u64,
                )
                || rings.allocations().iter().any(|allocation| {
                    ranges_overlap(
                        address,
                        payload_bytes,
                        allocation.descriptor_iova,
                        u64::from(allocation.allocation_bytes),
                    )
                })
            {
                return Err(TxPublishError::InvalidPlan);
            }
            let pointer = dma_pointer(observation, address, payload_bytes)
                .ok_or(TxPublishError::InvalidPlan)?;
            Some((pointer, bytes))
        } else {
            None
        };

        let read_index = regs::register_spec(3, Register::CurrentSourceReadIndex)
            .map_err(TxPublishError::Register)?;
        let producer_index =
            regs::register_spec(3, Register::SourceWriteIndex).map_err(TxPublishError::Register)?;
        let source_size =
            regs::register_spec(3, Register::SourceSize).map_err(TxPublishError::Register)?;
        let source_base_low =
            regs::register_spec(3, Register::SourceBaseLow).map_err(TxPublishError::Register)?;
        let source_base_high =
            regs::register_spec(3, Register::SourceBaseHigh).map_err(TxPublishError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(TxPublishError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(TxPublishError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, source_size.offset).map_err(TxPublishError::Register)?;
        let live_base_low =
            read32_mapping(mapping, source_base_low.offset).map_err(TxPublishError::Register)?;
        let live_base_high =
            read32_mapping(mapping, source_base_high.offset).map_err(TxPublishError::Register)?;
        if live_read != cursor.consumer_index()
            || live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(TxPublishError::LeaseLost);
        }
        register_access(producer_index.offset, AccessKind::Publish)
            .map_err(TxPublishError::Register)?;

        if let Some((pointer, bytes)) = additional_payload {
            copy_to_dma(pointer, bytes);
        }
        copy_to_dma(packet_pointer, frame);
        copy_to_dma(descriptor_pointer, descriptor.as_bytes());
        io_barrier();
        publish_source_index_mapping(mapping, producer_index.offset, next_producer as u32)
            .map_err(TxPublishError::Register)?;
        if cursor.commit_publish(ticket) != Ok(next_producer) {
            return Err(TxPublishError::Uncertain);
        }
        drop(guard);
        Ok(())
    }

    /// Reclaim completed descriptors from the native HTT CE4 source ring.
    ///
    /// The owner must poll this before planning more packets. The live
    /// source-read index is checked against the current producer window, and
    /// only then advances the software consumer cursor. CE4 has interrupts
    /// disabled in the pinned Adrastea profile, so this path is poll-driven.
    pub fn reclaim_qcacld_data_tx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
    ) -> Result<usize, TxPublishError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| TxPublishError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || observation.htt_map_generation != generation
            || observation.htt_host_to_target_engine != Some(HTT_DATA_HOST_TO_TARGET_CE)
            || observation.htt_target_to_host_engine != Some(HTT_DATA_TARGET_TO_HOST_CE)
            || observation.htt_endpoint_generation != generation
            || observation.htt_endpoint_id.is_none()
            || observation.htt_endpoint_max_message_size == 0
        {
            return Err(TxPublishError::LeaseLost);
        }
        let ring = source_ring(
            observation,
            profile,
            rings,
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            cursor.entries(),
        )
        .ok_or(TxPublishError::InvalidPlan)?;
        let read_index = regs::register_spec(
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            Register::CurrentSourceReadIndex,
        )
        .map_err(TxPublishError::Register)?;
        let producer_index = regs::register_spec(
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            Register::SourceWriteIndex,
        )
        .map_err(TxPublishError::Register)?;
        let source_size = regs::register_spec(
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            Register::SourceSize,
        )
        .map_err(TxPublishError::Register)?;
        let source_base_low = regs::register_spec(
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            Register::SourceBaseLow,
        )
        .map_err(TxPublishError::Register)?;
        let source_base_high = regs::register_spec(
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            Register::SourceBaseHigh,
        )
        .map_err(TxPublishError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(TxPublishError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(TxPublishError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, source_size.offset).map_err(TxPublishError::Register)?;
        let live_base_low =
            read32_mapping(mapping, source_base_low.offset).map_err(TxPublishError::Register)?;
        let live_base_high =
            read32_mapping(mapping, source_base_high.offset).map_err(TxPublishError::Register)?;
        if live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(TxPublishError::LeaseLost);
        }
        let reclaimed = cursor
            .reclaim_tx_to(live_read)
            .map_err(TxPublishError::Cursor)?;
        drop(guard);
        Ok(reclaimed)
    }

    /// Reclaim CE3 source descriptors and erase key-bearing WMI buffers only
    /// after the live source-read index proves their tickets completed. The
    /// ledger must have been reserved before publication; uncertain tickets
    /// remain retained and fail closed until a trusted lifecycle recovery.
    pub fn reclaim_sensitive_wmi_tx<const N: usize>(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
        sensitive: &mut SensitiveTxBufferLedger<N>,
    ) -> Result<SensitiveWmiTxReclaimReport, TxPublishError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| TxPublishError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || observation.wmi_map_generation != generation
            || observation.wmi_host_to_target_engine != Some(3)
            || observation.wmi_target_to_host_engine != Some(2)
        {
            return Err(TxPublishError::LeaseLost);
        }
        let ring = source_ring(observation, profile, rings, 3, cursor.entries())
            .ok_or(TxPublishError::InvalidPlan)?;
        let read_index = regs::register_spec(3, Register::CurrentSourceReadIndex)
            .map_err(TxPublishError::Register)?;
        let producer_index =
            regs::register_spec(3, Register::SourceWriteIndex).map_err(TxPublishError::Register)?;
        let source_size =
            regs::register_spec(3, Register::SourceSize).map_err(TxPublishError::Register)?;
        let source_base_low =
            regs::register_spec(3, Register::SourceBaseLow).map_err(TxPublishError::Register)?;
        let source_base_high =
            regs::register_spec(3, Register::SourceBaseHigh).map_err(TxPublishError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(TxPublishError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(TxPublishError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, source_size.offset).map_err(TxPublishError::Register)?;
        let live_base_low =
            read32_mapping(mapping, source_base_low.offset).map_err(TxPublishError::Register)?;
        let live_base_high =
            read32_mapping(mapping, source_base_high.offset).map_err(TxPublishError::Register)?;
        io_barrier();
        if live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(TxPublishError::LeaseLost);
        }
        let completed = cursor
            .preview_reclaim_tx_to(live_read)
            .map_err(TxPublishError::Cursor)?;
        let consumer_generation = cursor
            .consumer_generation()
            .checked_add(completed as u64)
            .ok_or(TxPublishError::Cursor(
                qcom_ce_v3::Error::GenerationExhausted,
            ))?;

        let mut sensitive_buffers_scrubbed = 0usize;
        let mut sensitive_bytes_scrubbed = 0u64;
        for lease in sensitive.active() {
            if lease.generation() != generation
                || lease.ticket().generation() < cursor.consumer_generation()
                || lease.ticket().generation() >= cursor.producer_generation()
            {
                return Err(TxPublishError::SensitiveTx(
                    crate::qcom_wmi_ce::Error::SensitiveTxBufferNotReserved,
                ));
            }
            if cursor
                .tx_ticket_completed_to(lease.ticket(), live_read)
                .map_err(TxPublishError::Cursor)?
            {
                let buffer = lease.buffer();
                if !dma_iova_contains(observation, buffer.iova, u64::from(buffer.capacity))
                    || rings.allocations().iter().any(|allocation| {
                        ranges_overlap(
                            buffer.iova,
                            u64::from(buffer.capacity),
                            allocation.descriptor_iova,
                            u64::from(allocation.allocation_bytes),
                        )
                    })
                    || dma_pointer(observation, buffer.iova, u64::from(buffer.capacity)).is_none()
                {
                    return Err(TxPublishError::InvalidPlan);
                }
                sensitive_buffers_scrubbed = sensitive_buffers_scrubbed
                    .checked_add(1)
                    .ok_or(TxPublishError::InvalidPlan)?;
                sensitive_bytes_scrubbed = sensitive_bytes_scrubbed
                    .checked_add(u64::from(buffer.capacity))
                    .ok_or(TxPublishError::InvalidPlan)?;
            }
        }

        for lease in sensitive.active() {
            if cursor
                .tx_ticket_completed_to(lease.ticket(), live_read)
                .map_err(TxPublishError::Cursor)?
            {
                let buffer = lease.buffer();
                let pointer = dma_pointer(observation, buffer.iova, u64::from(buffer.capacity))
                    .ok_or(TxPublishError::InvalidPlan)?;
                // SAFETY: the gate pins an uncached exclusive DMA mapping, and
                // the live CE3 read index has passed this exact source ticket.
                unsafe {
                    core::ptr::write_bytes(pointer, 0, buffer.capacity as usize);
                }
            }
        }
        io_barrier();
        let reclaimed = cursor
            .reclaim_tx_to(live_read)
            .map_err(TxPublishError::Cursor)?;
        if reclaimed != completed || cursor.consumer_generation() != consumer_generation {
            return Err(TxPublishError::Uncertain);
        }
        let retired = sensitive
            .retire_scrubbed(generation, consumer_generation)
            .map_err(TxPublishError::SensitiveTx)?;
        if retired != sensitive_buffers_scrubbed {
            return Err(TxPublishError::Uncertain);
        }
        drop(guard);
        Ok(SensitiveWmiTxReclaimReport {
            descriptors_reclaimed: reclaimed,
            sensitive_buffers_scrubbed,
            sensitive_bytes_scrubbed,
        })
    }

    /// Publish one source-matched native-Wi-Fi data packet through the
    /// dynamically negotiated HTC endpoint and CE4's two-entry gather queue.
    ///
    /// The live gate must report the exact current HTT service map and
    /// connection response. All payload, fragment-table, and descriptor bytes
    /// are staged in the pinned uncached DMA window before a single CE4 source
    /// producer-index write. This does not create the firmware/session owner,
    /// bind CE1 interrupts, or provide a production Gate.
    pub fn publish_qcacld_data_tx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: CeTxPlan,
        frame: &[u8],
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| TxPublishError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || observation.htt_map_generation != generation
            || observation.htt_host_to_target_engine != Some(HTT_DATA_HOST_TO_TARGET_CE)
            || observation.htt_target_to_host_engine != Some(HTT_DATA_TARGET_TO_HOST_CE)
            || observation.htt_endpoint_generation != generation
            || observation.htt_endpoint_id != Some(plan.endpoint_id())
        {
            return Err(TxPublishError::LeaseLost);
        }
        let route = plan.route();
        if route.host_to_target_engine != HTT_DATA_HOST_TO_TARGET_CE
            || route.target_to_host_engine != HTT_DATA_TARGET_TO_HOST_CE
            || frame.is_empty()
            || frame.len() != usize::from(plan.frame_bytes())
            || plan.first_descriptor().address() == 0
            || plan.frame_iova() == 0
        {
            return Err(TxPublishError::InvalidPlan);
        }
        let ring = source_ring(
            observation,
            profile,
            rings,
            usize::from(HTT_DATA_HOST_TO_TARGET_CE),
            cursor.entries(),
        )
        .ok_or(TxPublishError::InvalidPlan)?;
        let config = profile
            .config(usize::from(HTT_DATA_HOST_TO_TARGET_CE))
            .map_err(|_| TxPublishError::InvalidPlan)?;

        let first = plan.first_descriptor();
        let second = plan.second_descriptor();
        let fragment_iova = plan.fragment_table_iova();
        let frame_iova = plan.frame_iova();
        let frame_download_bytes = usize::from(plan.frame_download_bytes());
        let total_download_bytes = usize::from(plan.total_download_bytes());
        let expected_total = HTC_HTT_PREFIX_BYTES
            .checked_add(frame_download_bytes)
            .ok_or(TxPublishError::InvalidPlan)?;
        let htc_payload_bytes = total_download_bytes
            .checked_sub(crate::qcom_htc::HTC_FRAME_HEADER_SIZE)
            .ok_or(TxPublishError::InvalidPlan)?;
        if plan.endpoint_id() != observation.htt_endpoint_id.unwrap_or(0)
            || observation.htt_endpoint_max_message_size == 0
            || htc_payload_bytes > usize::from(observation.htt_endpoint_max_message_size)
            || total_download_bytes != expected_total
            || frame_download_bytes == 0
            || frame_download_bytes > frame.len()
            || frame_download_bytes > usize::from(config.source_max_bytes)
            || first.nbytes() as usize != HTC_HTT_PREFIX_BYTES
            || first.transfer_id() != u16::from(plan.endpoint_id())
            || second.address() != frame_iova
            || second.nbytes() as usize != frame_download_bytes
            || second.transfer_id() != u16::from(plan.endpoint_id())
        {
            return Err(TxPublishError::InvalidPlan);
        }

        let prefix = plan.htc_htt_prefix();
        let htc_length =
            u16::try_from(htc_payload_bytes).map_err(|_| TxPublishError::InvalidPlan)?;
        if prefix[0] != plan.endpoint_id()
            || prefix[1] != 0
            || u16::from_le_bytes([prefix[2], prefix[3]]) != htc_length
            || prefix[4] != 0
            || prefix[5] != plan.sequence()
            || prefix[6] != 0
            || prefix[7] != 0
            || u16::from_le_bytes([prefix[12], prefix[13]]) != plan.frame_bytes()
            || u32::from_le_bytes([prefix[16], prefix[17], prefix[18], prefix[19]])
                != fragment_iova as u32
            || prefix[20..24] != [0, 0, 0, 0]
        {
            return Err(TxPublishError::InvalidPlan);
        }

        let mut expected_fragment = [0u8; HELIUMPLUS_FRAGMENT_DESC_BYTES];
        expected_fragment[24..28].copy_from_slice(&(frame_iova as u32).to_le_bytes());
        let fragment_hi_and_len =
            ((u32::from(plan.frame_bytes())) << 16) | ((frame_iova >> 32) as u32 & 0xffff);
        expected_fragment[28..32].copy_from_slice(&fragment_hi_and_len.to_le_bytes());
        if plan.fragment_descriptor() != expected_fragment {
            return Err(TxPublishError::InvalidPlan);
        }

        let first_control = u32::from_le_bytes([
            first.as_bytes()[4],
            first.as_bytes()[5],
            first.as_bytes()[6],
            first.as_bytes()[7],
        ]);
        let second_control = u32::from_le_bytes([
            second.as_bytes()[4],
            second.as_bytes()[5],
            second.as_bytes()[6],
            second.as_bytes()[7],
        ]);
        let expected_first_control = ((first.address() >> 32) as u32 & 0x1f) | (1 << 31);
        let expected_second_control = (second.address() >> 32) as u32 & 0x1f;
        if first_control != expected_first_control
            || second_control != expected_second_control
            || first.as_bytes()[12..] != [0, 0, 0, 0]
            || second.as_bytes()[12..] != [0, 0, 0, 0]
            || first.address() == frame_iova
            || ranges_overlap(
                first.address(),
                HTC_HTT_PREFIX_BYTES as u64,
                frame_iova,
                frame.len() as u64,
            )
            || ranges_overlap(
                first.address(),
                HTC_HTT_PREFIX_BYTES as u64,
                fragment_iova,
                HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
            )
            || ranges_overlap(
                frame_iova,
                frame.len() as u64,
                fragment_iova,
                HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
            )
        {
            return Err(TxPublishError::InvalidPlan);
        }

        for allocation in rings.allocations() {
            if ranges_overlap(
                first.address(),
                HTC_HTT_PREFIX_BYTES as u64,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) || ranges_overlap(
                frame_iova,
                frame.len() as u64,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) || ranges_overlap(
                fragment_iova,
                HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(TxPublishError::InvalidPlan);
            }
        }

        let ticket = cursor.prepare_next().map_err(TxPublishError::Cursor)?;
        if ticket != plan.first_ticket() {
            return Err(TxPublishError::InvalidPlan);
        }
        let next_producer = cursor
            .preview_publish_batch(ticket, CE4_DATA_DESCRIPTOR_COUNT)
            .map_err(TxPublishError::Cursor)?;
        if next_producer != plan.next_producer_index() {
            return Err(TxPublishError::InvalidPlan);
        }
        let second_index = (ticket.index() + 1) & (cursor.entries() - 1);
        let first_slot = ring_slot_iova(ring, ticket.index()).ok_or(TxPublishError::InvalidPlan)?;
        let second_slot = ring_slot_iova(ring, second_index).ok_or(TxPublishError::InvalidPlan)?;
        if first_slot != plan.first_slot_iova() || second_slot != plan.second_slot_iova() {
            return Err(TxPublishError::InvalidPlan);
        }

        let first_slot_pointer =
            dma_pointer(observation, first_slot, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(TxPublishError::InvalidPlan)?;
        let second_slot_pointer = dma_pointer(
            observation,
            second_slot,
            qcom_ce_v3::DESCRIPTOR_BYTES as u64,
        )
        .ok_or(TxPublishError::InvalidPlan)?;
        let prefix_pointer = dma_pointer(observation, first.address(), HTC_HTT_PREFIX_BYTES as u64)
            .ok_or(TxPublishError::InvalidPlan)?;
        let frame_pointer = dma_pointer(observation, frame_iova, frame.len() as u64)
            .ok_or(TxPublishError::InvalidPlan)?;
        let fragment_pointer = dma_pointer(
            observation,
            fragment_iova,
            HELIUMPLUS_FRAGMENT_DESC_BYTES as u64,
        )
        .ok_or(TxPublishError::InvalidPlan)?;

        let engine = usize::from(HTT_DATA_HOST_TO_TARGET_CE);
        let read_index = regs::register_spec(engine, Register::CurrentSourceReadIndex)
            .map_err(TxPublishError::Register)?;
        let producer_index = regs::register_spec(engine, Register::SourceWriteIndex)
            .map_err(TxPublishError::Register)?;
        let source_size =
            regs::register_spec(engine, Register::SourceSize).map_err(TxPublishError::Register)?;
        let source_base_low = regs::register_spec(engine, Register::SourceBaseLow)
            .map_err(TxPublishError::Register)?;
        let source_base_high = regs::register_spec(engine, Register::SourceBaseHigh)
            .map_err(TxPublishError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(TxPublishError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(TxPublishError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, source_size.offset).map_err(TxPublishError::Register)?;
        let live_base_low =
            read32_mapping(mapping, source_base_low.offset).map_err(TxPublishError::Register)?;
        let live_base_high =
            read32_mapping(mapping, source_base_high.offset).map_err(TxPublishError::Register)?;
        if live_read != cursor.consumer_index()
            || live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(TxPublishError::LeaseLost);
        }
        register_access(producer_index.offset, AccessKind::Publish)
            .map_err(TxPublishError::Register)?;

        copy_to_dma(frame_pointer, frame);
        copy_to_dma(fragment_pointer, &plan.fragment_descriptor());
        copy_to_dma(prefix_pointer, &prefix);
        copy_to_dma(first_slot_pointer, first.as_bytes());
        copy_to_dma(second_slot_pointer, second.as_bytes());
        io_barrier();
        publish_source_index_mapping(mapping, producer_index.offset, next_producer as u32)
            .map_err(TxPublishError::Register)?;
        if cursor.commit_publish_batch(ticket, CE4_DATA_DESCRIPTOR_COUNT) != Ok(next_producer) {
            return Err(TxPublishError::Uncertain);
        }
        drop(guard);
        Ok(())
    }

    /// Post one zeroed WMI receive buffer on CE2. The descriptor remains
    /// owned by the target until a separately validated completion consumes
    /// it and releases the matching buffer lease from `RxBufferLedger`.
    pub fn post_wmi_rx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: WmiCeRxPostPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
    ) -> Result<(), RxPostError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxPostError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
        {
            return Err(RxPostError::LeaseLost);
        }
        if observation.wmi_map_generation != generation
            || observation.wmi_host_to_target_engine != Some(3)
            || observation.wmi_target_to_host_engine != Some(2)
            || plan.route().host_to_target_engine != 3
            || plan.route().target_to_host_engine != 2
        {
            return Err(RxPostError::LeaseLost);
        }

        let config = profile.config(2).map_err(|_| RxPostError::InvalidPlan)?;
        let mut selected_ring = None;
        for allocation in rings.allocations() {
            if !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxPostError::InvalidPlan);
            }
            if allocation.engine == 2 && allocation.direction == RingDirection::Destination {
                if selected_ring.replace(*allocation).is_some() {
                    return Err(RxPostError::InvalidPlan);
                }
            }
        }
        let ring = selected_ring.ok_or(RxPostError::InvalidPlan)?;
        let buffer = plan.buffer();
        let descriptor = plan.descriptor();
        if ring.entries != config.destination_entries
            || usize::from(ring.entries) != cursor.entries()
            || ring.descriptor_bytes
                != u32::from(ring.entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || ring.allocation_bytes
                != ring.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
            || buffer.capacity < 256
            || buffer.capacity > u32::from(config.source_max_bytes)
            || buffer.iova & 7 != 0
            || descriptor.address() != buffer.iova
            || descriptor.as_bytes()[8..10] != [0, 0]
        {
            return Err(RxPostError::InvalidPlan);
        }
        for (index, allocation) in rings.allocations().iter().enumerate() {
            for other in &rings.allocations()[index + 1..] {
                if ranges_overlap(
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                    other.descriptor_iova,
                    u64::from(other.allocation_bytes),
                ) {
                    return Err(RxPostError::InvalidPlan);
                }
            }
            if ranges_overlap(
                buffer.iova,
                u64::from(buffer.capacity),
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxPostError::InvalidPlan);
            }
        }
        ledger
            .preview_reserve(buffer)
            .map_err(RxPostError::Ledger)?;

        let ticket = cursor.prepare_next().map_err(RxPostError::Cursor)?;
        if ticket != plan.ticket() {
            return Err(RxPostError::InvalidPlan);
        }
        let next_producer = cursor
            .preview_publish(ticket)
            .map_err(RxPostError::Cursor)?;
        if next_producer != plan.next_producer_index() {
            return Err(RxPostError::InvalidPlan);
        }
        let slot_offset = u64::try_from(ticket.index())
            .map_err(|_| RxPostError::InvalidPlan)?
            .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
            .ok_or(RxPostError::InvalidPlan)?;
        let slot_iova = ring
            .descriptor_iova
            .checked_add(slot_offset)
            .ok_or(RxPostError::InvalidPlan)?;
        if slot_iova != plan.descriptor_slot_iova()
            || !range_contains(
                ring.descriptor_iova,
                u64::from(ring.descriptor_bytes),
                slot_iova,
                qcom_ce_v3::DESCRIPTOR_BYTES as u64,
            )
        {
            return Err(RxPostError::InvalidPlan);
        }
        let buffer_pointer = dma_pointer(observation, buffer.iova, u64::from(buffer.capacity))
            .ok_or(RxPostError::InvalidPlan)?;
        let descriptor_pointer =
            dma_pointer(observation, slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(RxPostError::InvalidPlan)?;

        let read_index = regs::register_spec(2, Register::CurrentDestinationReadIndex)
            .map_err(RxPostError::Register)?;
        let producer_index = regs::register_spec(2, Register::DestinationWriteIndex)
            .map_err(RxPostError::Register)?;
        let destination_size =
            regs::register_spec(2, Register::DestinationSize).map_err(RxPostError::Register)?;
        let destination_base_low =
            regs::register_spec(2, Register::DestinationBaseLow).map_err(RxPostError::Register)?;
        let destination_base_high =
            regs::register_spec(2, Register::DestinationBaseHigh).map_err(RxPostError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(RxPostError::Register)? as usize;
        let live_producer =
            read32_mapping(mapping, producer_index.offset).map_err(RxPostError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, destination_size.offset).map_err(RxPostError::Register)?;
        let live_base_low =
            read32_mapping(mapping, destination_base_low.offset).map_err(RxPostError::Register)?;
        let live_base_high =
            read32_mapping(mapping, destination_base_high.offset).map_err(RxPostError::Register)?;
        if cursor.rx_completions_to(live_read).is_err()
            || live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(RxPostError::LeaseLost);
        }
        register_access(producer_index.offset, AccessKind::Publish)
            .map_err(RxPostError::Register)?;

        zero_dma(buffer_pointer, buffer.capacity as usize);
        copy_to_dma(descriptor_pointer, descriptor.as_bytes());
        io_barrier();
        ledger
            .reserve(buffer, ticket)
            .map_err(RxPostError::Ledger)?;
        publish_destination_index_mapping(mapping, producer_index.offset, next_producer as u32)
            .map_err(RxPostError::Register)?;
        if cursor.commit_publish(ticket) != Ok(next_producer) {
            return Err(RxPostError::Uncertain);
        }
        drop(guard);
        Ok(())
    }

    /// Post one HTT-data receive buffer on CE1 using the runtime-selected
    /// `HTT_DATA_MSG_SVC` route. CE1 transports HTT/HTC indications; the large
    /// MSDU payload ring is configured separately by `sunfish_htt_rx_ring`.
    pub fn post_htt_rx_ce1(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: ServiceCeRxPostPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
    ) -> Result<(), RxPostError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxPostError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || plan.service_id() != HTT_DATA_SERVICE_ID
            || observation.htt_map_generation != generation
            || observation.htt_host_to_target_engine != Some(HTT_DATA_SOURCE_CE)
            || observation.htt_target_to_host_engine != Some(HTT_DATA_DESTINATION_CE)
            || plan.route().host_to_target_engine != HTT_DATA_SOURCE_CE
            || plan.route().target_to_host_engine != HTT_DATA_DESTINATION_CE
        {
            return Err(RxPostError::LeaseLost);
        }

        let engine = usize::from(HTT_DATA_DESTINATION_CE);
        let config = profile
            .config(engine)
            .map_err(|_| RxPostError::InvalidPlan)?;
        let mut selected_ring = None;
        for allocation in rings.allocations() {
            if !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxPostError::InvalidPlan);
            }
            if usize::from(allocation.engine) == engine
                && allocation.direction == RingDirection::Destination
            {
                if selected_ring.replace(*allocation).is_some() {
                    return Err(RxPostError::InvalidPlan);
                }
            }
        }
        let ring = selected_ring.ok_or(RxPostError::InvalidPlan)?;
        let buffer = plan.buffer();
        let descriptor = plan.descriptor();
        if ring.entries != config.destination_entries
            || usize::from(ring.entries) != cursor.entries()
            || buffer.capacity < u32::from(config.source_max_bytes)
            || buffer.capacity > u16::MAX as u32
            || buffer.iova & 7 != 0
            || descriptor.address() != buffer.iova
            || descriptor.as_bytes()[8..10] != [0, 0]
        {
            return Err(RxPostError::InvalidPlan);
        }
        for (index, allocation) in rings.allocations().iter().enumerate() {
            for other in &rings.allocations()[index + 1..] {
                if ranges_overlap(
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                    other.descriptor_iova,
                    u64::from(other.allocation_bytes),
                ) {
                    return Err(RxPostError::InvalidPlan);
                }
            }
            if ranges_overlap(
                buffer.iova,
                u64::from(buffer.capacity),
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxPostError::InvalidPlan);
            }
        }
        ledger
            .preview_reserve(buffer)
            .map_err(RxPostError::Ledger)?;

        let ticket = cursor.prepare_next().map_err(RxPostError::Cursor)?;
        if ticket != plan.ticket() {
            return Err(RxPostError::InvalidPlan);
        }
        let next_producer = cursor
            .preview_publish(ticket)
            .map_err(RxPostError::Cursor)?;
        if next_producer != plan.next_producer_index() {
            return Err(RxPostError::InvalidPlan);
        }
        let slot_offset = u64::try_from(ticket.index())
            .map_err(|_| RxPostError::InvalidPlan)?
            .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
            .ok_or(RxPostError::InvalidPlan)?;
        let slot_iova = ring
            .descriptor_iova
            .checked_add(slot_offset)
            .ok_or(RxPostError::InvalidPlan)?;
        if slot_iova != plan.descriptor_slot_iova()
            || !range_contains(
                ring.descriptor_iova,
                u64::from(ring.descriptor_bytes),
                slot_iova,
                qcom_ce_v3::DESCRIPTOR_BYTES as u64,
            )
        {
            return Err(RxPostError::InvalidPlan);
        }
        let buffer_pointer = dma_pointer(observation, buffer.iova, u64::from(buffer.capacity))
            .ok_or(RxPostError::InvalidPlan)?;
        let descriptor_pointer =
            dma_pointer(observation, slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(RxPostError::InvalidPlan)?;

        let read_index = regs::register_spec(engine, Register::CurrentDestinationReadIndex)
            .map_err(RxPostError::Register)?;
        let producer_index = regs::register_spec(engine, Register::DestinationWriteIndex)
            .map_err(RxPostError::Register)?;
        let destination_size = regs::register_spec(engine, Register::DestinationSize)
            .map_err(RxPostError::Register)?;
        let destination_base_low = regs::register_spec(engine, Register::DestinationBaseLow)
            .map_err(RxPostError::Register)?;
        let destination_base_high = regs::register_spec(engine, Register::DestinationBaseHigh)
            .map_err(RxPostError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(RxPostError::Register)? as usize;
        let live_producer =
            read32_mapping(mapping, producer_index.offset).map_err(RxPostError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, destination_size.offset).map_err(RxPostError::Register)?;
        let live_base_low =
            read32_mapping(mapping, destination_base_low.offset).map_err(RxPostError::Register)?;
        let live_base_high =
            read32_mapping(mapping, destination_base_high.offset).map_err(RxPostError::Register)?;
        if cursor.rx_completions_to(live_read).is_err()
            || live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(RxPostError::LeaseLost);
        }
        register_access(producer_index.offset, AccessKind::Publish)
            .map_err(RxPostError::Register)?;

        zero_dma(buffer_pointer, buffer.capacity as usize);
        copy_to_dma(descriptor_pointer, descriptor.as_bytes());
        io_barrier();
        ledger
            .reserve(buffer, ticket)
            .map_err(RxPostError::Ledger)?;
        publish_destination_index_mapping(mapping, producer_index.offset, next_producer as u32)
            .map_err(RxPostError::Register)?;
        if cursor.commit_publish(ticket) != Ok(next_producer) {
            return Err(RxPostError::Uncertain);
        }
        drop(guard);
        Ok(())
    }

    /// Consume one completed WMI receive descriptor from CE2. The hardware
    /// destination read index is checked against the still-posted window; the
    /// descriptor must retain the exact ticket's buffer address and report a
    /// bounded nonzero length. Only then are bytes copied and software
    /// ownership retired. This does not acknowledge CE interrupts or decode
    /// HTC/WMI payloads.
    pub fn consume_wmi_rx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
        output: &mut [u8],
    ) -> Result<qcom_ce_v3::RxCompletion, RxConsumeError> {
        self.consume_ce2_rx_inner(
            generation,
            profile,
            rings,
            cursor,
            ledger,
            Ce2RxRoute::WmiControl,
            output,
        )
    }

    /// Consume and validate the one outstanding HTT_DATA_MSG_SVC control
    /// response from CE2. The CE descriptor and buffer lease are retired under
    /// the native gate before the copied frame is parsed by the startup session.
    pub fn receive_htt_connect_response_ce2(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
        session: &mut HttStartupSession,
        output: &mut [u8],
    ) -> Result<
        (
            qcom_ce_v3::RxCompletion,
            crate::sunfish_qcacld_htc::HttEndpoint,
        ),
        HttConnectRxError,
    > {
        if session.generation() != generation {
            return Err(HttConnectRxError::Startup(
                HttStartupError::WrongGeneration {
                    expected: session.generation(),
                    observed: generation,
                },
            ));
        }
        if session.phase() != crate::sunfish_qcacld_htc::HttStartupPhase::AwaitResponse {
            return Err(HttConnectRxError::Startup(HttStartupError::InvalidState {
                expected: crate::sunfish_qcacld_htc::HttStartupPhase::AwaitResponse,
                observed: session.phase(),
            }));
        }
        let completion = self
            .consume_ce2_rx_inner(
                generation,
                profile,
                rings,
                cursor,
                ledger,
                Ce2RxRoute::HtcControl,
                output,
            )
            .map_err(HttConnectRxError::Consume)?;
        let endpoint = session
            .accept_connect_response_frame(generation, &output[..usize::from(completion.nbytes)])
            .map_err(HttConnectRxError::Startup)?;
        Ok((completion, endpoint))
    }

    /// Consume the outstanding WMI_CONTROL_SVC connect response from CE2.
    /// The dynamic WMI endpoint is bound only after the copied HTC control
    /// frame matches the submitted service request and the live CE2 route.
    pub fn receive_wmi_control_connect_response_ce2(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
        session: &mut SunfishWmiControlSession,
        output: &mut [u8],
    ) -> Result<(qcom_ce_v3::RxCompletion, WmiControlEndpoint), WmiControlConnectRxError> {
        if session.generation() != generation {
            return Err(WmiControlConnectRxError::Session(
                WmiControlSessionError::WrongGeneration {
                    expected: session.generation(),
                    observed: generation,
                },
            ));
        }
        if session.phase() != WmiControlPhase::AwaitConnectResponse {
            return Err(WmiControlConnectRxError::Session(
                WmiControlSessionError::InvalidState {
                    expected: WmiControlPhase::AwaitConnectResponse,
                    observed: session.phase(),
                },
            ));
        }
        let completion = self
            .consume_ce2_rx_inner(
                generation,
                profile,
                rings,
                cursor,
                ledger,
                Ce2RxRoute::HtcControl,
                output,
            )
            .map_err(WmiControlConnectRxError::Consume)?;
        let endpoint = session
            .accept_connect_response_frame(generation, &output[..usize::from(completion.nbytes)])
            .map_err(WmiControlConnectRxError::Session)?;
        Ok((completion, endpoint))
    }

    fn consume_ce2_rx_inner(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
        route: Ce2RxRoute,
        output: &mut [u8],
    ) -> Result<qcom_ce_v3::RxCompletion, RxConsumeError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxConsumeError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
        {
            return Err(RxConsumeError::LeaseLost);
        }
        let route_valid = match route {
            Ce2RxRoute::WmiControl => {
                observation.wmi_map_generation == generation
                    && observation.wmi_host_to_target_engine == Some(3)
                    && observation.wmi_target_to_host_engine == Some(2)
            }
            Ce2RxRoute::HtcControl => {
                observation.htc_control_map_generation == generation
                    && observation.htc_control_host_to_target_engine == Some(0)
                    && observation.htc_control_target_to_host_engine == Some(2)
            }
        };
        if !route_valid {
            return Err(RxConsumeError::LeaseLost);
        }

        let config = profile.config(2).map_err(|_| RxConsumeError::InvalidPlan)?;
        let mut selected_ring = None;
        for allocation in rings.allocations() {
            if !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxConsumeError::InvalidPlan);
            }
            if allocation.engine == 2 && allocation.direction == RingDirection::Destination {
                if selected_ring.replace(*allocation).is_some() {
                    return Err(RxConsumeError::InvalidPlan);
                }
            }
        }
        let ring = selected_ring.ok_or(RxConsumeError::InvalidPlan)?;
        if ring.entries != config.destination_entries
            || usize::from(ring.entries) != cursor.entries()
            || ring.descriptor_bytes
                != u32::from(ring.entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || ring.allocation_bytes
                != ring.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
        {
            return Err(RxConsumeError::InvalidPlan);
        }
        for (index, allocation) in rings.allocations().iter().enumerate() {
            for other in &rings.allocations()[index + 1..] {
                if ranges_overlap(
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                    other.descriptor_iova,
                    u64::from(other.allocation_bytes),
                ) {
                    return Err(RxConsumeError::InvalidPlan);
                }
            }
        }

        let ticket = match cursor.next_rx_completion_ticket() {
            Ok(ticket) => ticket,
            Err(qcom_ce_v3::Error::InvalidSlot) => return Err(RxConsumeError::NotReady),
            Err(error) => return Err(RxConsumeError::Cursor(error)),
        };
        let buffer = ledger.window_for(ticket).map_err(RxConsumeError::Ledger)?;
        if buffer.capacity < 256
            || buffer.capacity > u32::from(config.source_max_bytes)
            || buffer.iova & 7 != 0
            || !dma_iova_contains(observation, buffer.iova, u64::from(buffer.capacity))
        {
            return Err(RxConsumeError::InvalidPlan);
        }
        for allocation in rings.allocations() {
            if ranges_overlap(
                buffer.iova,
                u64::from(buffer.capacity),
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxConsumeError::InvalidPlan);
            }
        }

        let slot_offset = u64::try_from(ticket.index())
            .map_err(|_| RxConsumeError::InvalidPlan)?
            .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
            .ok_or(RxConsumeError::InvalidPlan)?;
        let slot_iova = ring
            .descriptor_iova
            .checked_add(slot_offset)
            .ok_or(RxConsumeError::InvalidPlan)?;
        if !range_contains(
            ring.descriptor_iova,
            u64::from(ring.descriptor_bytes),
            slot_iova,
            qcom_ce_v3::DESCRIPTOR_BYTES as u64,
        ) {
            return Err(RxConsumeError::InvalidPlan);
        }
        let descriptor_pointer =
            dma_pointer(observation, slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(RxConsumeError::InvalidPlan)?;

        let read_index = regs::register_spec(2, Register::CurrentDestinationReadIndex)
            .map_err(RxConsumeError::Register)?;
        let producer_index = regs::register_spec(2, Register::DestinationWriteIndex)
            .map_err(RxConsumeError::Register)?;
        let destination_size =
            regs::register_spec(2, Register::DestinationSize).map_err(RxConsumeError::Register)?;
        let destination_base_low = regs::register_spec(2, Register::DestinationBaseLow)
            .map_err(RxConsumeError::Register)?;
        let destination_base_high = regs::register_spec(2, Register::DestinationBaseHigh)
            .map_err(RxConsumeError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(RxConsumeError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(RxConsumeError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, destination_size.offset).map_err(RxConsumeError::Register)?;
        let live_base_low = read32_mapping(mapping, destination_base_low.offset)
            .map_err(RxConsumeError::Register)?;
        let live_base_high = read32_mapping(mapping, destination_base_high.offset)
            .map_err(RxConsumeError::Register)?;
        if live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(RxConsumeError::LeaseLost);
        }
        let completed = cursor
            .rx_completions_to(live_read)
            .map_err(|_| RxConsumeError::LeaseLost)?;
        if completed == 0 {
            return Err(RxConsumeError::NotReady);
        }

        let mut descriptor_bytes = [0u8; qcom_ce_v3::DESCRIPTOR_BYTES];
        io_barrier();
        copy_from_dma(descriptor_pointer, &mut descriptor_bytes);
        io_barrier();
        let descriptor = qcom_ce_v3::RxDescriptor::from_bytes(descriptor_bytes);
        let completion = descriptor
            .completion(buffer.iova, buffer.capacity)
            .map_err(RxConsumeError::Descriptor)?;
        let byte_count = usize::from(completion.nbytes);
        if output.len() < byte_count {
            return Err(RxConsumeError::OutputTooSmall);
        }
        cursor
            .preview_consume_rx(ticket, &descriptor, buffer.iova, buffer.capacity)
            .map_err(RxConsumeError::Cursor)?;
        ledger
            .preview_release(ticket, buffer)
            .map_err(RxConsumeError::Ledger)?;
        let payload_pointer = dma_pointer(observation, buffer.iova, byte_count as u64)
            .ok_or(RxConsumeError::InvalidPlan)?;

        io_barrier();
        copy_from_dma(payload_pointer, &mut output[..byte_count]);
        if cursor.consume_rx(ticket, &descriptor, buffer.iova, buffer.capacity) != Ok(completion)
            || ledger.release(ticket, buffer).is_err()
        {
            return Err(RxConsumeError::Uncertain);
        }
        drop(guard);
        Ok(completion)
    }

    /// Consume a completed HTT indication buffer from CE1. This is the
    /// transport-facing receive path; payloads from the separate low-latency
    /// HTT MSDU address ring are handled by `SunfishQcacldRxSession`.
    pub fn consume_htt_rx_ce1(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        cursor: &mut RingCursor,
        ledger: &mut RxBufferLedger,
        output: &mut [u8],
    ) -> Result<qcom_ce_v3::RxCompletion, RxConsumeError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| RxConsumeError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || observation.htt_map_generation != generation
            || observation.htt_host_to_target_engine != Some(HTT_DATA_SOURCE_CE)
            || observation.htt_target_to_host_engine != Some(HTT_DATA_DESTINATION_CE)
        {
            return Err(RxConsumeError::LeaseLost);
        }

        let engine = usize::from(HTT_DATA_DESTINATION_CE);
        let config = profile
            .config(engine)
            .map_err(|_| RxConsumeError::InvalidPlan)?;
        let mut selected_ring = None;
        for allocation in rings.allocations() {
            if !dma_iova_contains(
                observation,
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxConsumeError::InvalidPlan);
            }
            if usize::from(allocation.engine) == engine
                && allocation.direction == RingDirection::Destination
            {
                if selected_ring.replace(*allocation).is_some() {
                    return Err(RxConsumeError::InvalidPlan);
                }
            }
        }
        let ring = selected_ring.ok_or(RxConsumeError::InvalidPlan)?;
        if ring.entries != config.destination_entries
            || usize::from(ring.entries) != cursor.entries()
            || ring.descriptor_bytes
                != u32::from(ring.entries) * qcom_ce_v3::DESCRIPTOR_BYTES as u32
            || ring.allocation_bytes
                != ring.descriptor_bytes
                    + crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES as u32
        {
            return Err(RxConsumeError::InvalidPlan);
        }
        for (index, allocation) in rings.allocations().iter().enumerate() {
            for other in &rings.allocations()[index + 1..] {
                if ranges_overlap(
                    allocation.descriptor_iova,
                    u64::from(allocation.allocation_bytes),
                    other.descriptor_iova,
                    u64::from(other.allocation_bytes),
                ) {
                    return Err(RxConsumeError::InvalidPlan);
                }
            }
        }

        let ticket = match cursor.next_rx_completion_ticket() {
            Ok(ticket) => ticket,
            Err(qcom_ce_v3::Error::InvalidSlot) => return Err(RxConsumeError::NotReady),
            Err(error) => return Err(RxConsumeError::Cursor(error)),
        };
        let buffer = ledger.window_for(ticket).map_err(RxConsumeError::Ledger)?;
        if buffer.capacity < u32::from(config.source_max_bytes)
            || buffer.capacity > u16::MAX as u32
            || buffer.iova & 7 != 0
            || !dma_iova_contains(observation, buffer.iova, u64::from(buffer.capacity))
        {
            return Err(RxConsumeError::InvalidPlan);
        }
        for allocation in rings.allocations() {
            if ranges_overlap(
                buffer.iova,
                u64::from(buffer.capacity),
                allocation.descriptor_iova,
                u64::from(allocation.allocation_bytes),
            ) {
                return Err(RxConsumeError::InvalidPlan);
            }
        }

        let slot_offset = u64::try_from(ticket.index())
            .map_err(|_| RxConsumeError::InvalidPlan)?
            .checked_mul(qcom_ce_v3::DESCRIPTOR_BYTES as u64)
            .ok_or(RxConsumeError::InvalidPlan)?;
        let slot_iova = ring
            .descriptor_iova
            .checked_add(slot_offset)
            .ok_or(RxConsumeError::InvalidPlan)?;
        if !range_contains(
            ring.descriptor_iova,
            u64::from(ring.descriptor_bytes),
            slot_iova,
            qcom_ce_v3::DESCRIPTOR_BYTES as u64,
        ) {
            return Err(RxConsumeError::InvalidPlan);
        }
        let descriptor_pointer =
            dma_pointer(observation, slot_iova, qcom_ce_v3::DESCRIPTOR_BYTES as u64)
                .ok_or(RxConsumeError::InvalidPlan)?;

        let read_index = regs::register_spec(engine, Register::CurrentDestinationReadIndex)
            .map_err(RxConsumeError::Register)?;
        let producer_index = regs::register_spec(engine, Register::DestinationWriteIndex)
            .map_err(RxConsumeError::Register)?;
        let destination_size = regs::register_spec(engine, Register::DestinationSize)
            .map_err(RxConsumeError::Register)?;
        let destination_base_low = regs::register_spec(engine, Register::DestinationBaseLow)
            .map_err(RxConsumeError::Register)?;
        let destination_base_high = regs::register_spec(engine, Register::DestinationBaseHigh)
            .map_err(RxConsumeError::Register)?;
        let live_read =
            read32_mapping(mapping, read_index.offset).map_err(RxConsumeError::Register)? as usize;
        let live_producer = read32_mapping(mapping, producer_index.offset)
            .map_err(RxConsumeError::Register)? as usize;
        let live_size =
            read32_mapping(mapping, destination_size.offset).map_err(RxConsumeError::Register)?;
        let live_base_low = read32_mapping(mapping, destination_base_low.offset)
            .map_err(RxConsumeError::Register)?;
        let live_base_high = read32_mapping(mapping, destination_base_high.offset)
            .map_err(RxConsumeError::Register)?;
        if live_producer != cursor.producer_index()
            || live_size != u32::from(ring.entries)
            || live_base_low != ring.descriptor_iova as u32
            || live_base_high & 0x1f != ((ring.descriptor_iova >> 32) as u32 & 0x1f)
        {
            return Err(RxConsumeError::LeaseLost);
        }
        let completed = cursor
            .rx_completions_to(live_read)
            .map_err(|_| RxConsumeError::LeaseLost)?;
        if completed == 0 {
            return Err(RxConsumeError::NotReady);
        }

        let mut descriptor_bytes = [0u8; qcom_ce_v3::DESCRIPTOR_BYTES];
        io_barrier();
        copy_from_dma(descriptor_pointer, &mut descriptor_bytes);
        io_barrier();
        let descriptor = qcom_ce_v3::RxDescriptor::from_bytes(descriptor_bytes);
        let completion = descriptor
            .completion(buffer.iova, buffer.capacity)
            .map_err(RxConsumeError::Descriptor)?;
        let byte_count = usize::from(completion.nbytes);
        if output.len() < byte_count {
            return Err(RxConsumeError::OutputTooSmall);
        }
        cursor
            .preview_consume_rx(ticket, &descriptor, buffer.iova, buffer.capacity)
            .map_err(RxConsumeError::Cursor)?;
        ledger
            .preview_release(ticket, buffer)
            .map_err(RxConsumeError::Ledger)?;
        let payload_pointer = dma_pointer(observation, buffer.iova, byte_count as u64)
            .ok_or(RxConsumeError::InvalidPlan)?;

        io_barrier();
        copy_from_dma(payload_pointer, &mut output[..byte_count]);
        if cursor.consume_rx(ticket, &descriptor, buffer.iova, buffer.capacity) != Ok(completion)
            || ledger.release(ticket, buffer).is_err()
        {
            return Err(RxConsumeError::Uncertain);
        }
        drop(guard);
        Ok(completion)
    }

    /// Refill the low-latency HTT RX buffer ring while holding one live CE
    /// ownership lease across every index read, buffer validation, ring-entry
    /// store, barrier, and allocation-index publication. The supplied pool
    /// must be the exact uncached virtual backing corresponding to this
    /// generation's translated DMA aperture.
    pub fn refill_htt_rx_batch<const N: usize>(
        &mut self,
        generation: u64,
        plan: SunfishHttRxRingPlan,
        refill_state: &mut SunfishHttRxRingRefillState,
        session: &mut SunfishQcacldRxSession<N>,
        buffer_ids: &[u32],
        buffers: &mut SunfishHttRxBufferPool<'_>,
    ) -> Result<u32, HttRxRefillError> {
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| HttRxRefillError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized_runtime(observation, mapping, generation)
            || generation != expected_generation
            || !ReceiveInterruptService::HttData.route_ready(observation, generation)
        {
            return Err(HttRxRefillError::LeaseLost);
        }
        if buffers.plan() != plan {
            return Err(HttRxRefillError::InvalidPlan);
        }

        let ring_bytes = u64::from(plan.ring_slots())
            .checked_mul(crate::sunfish_htt_rx_ring::SUNFISH_HTT_RX_RING_ENTRY_BYTES)
            .ok_or(HttRxRefillError::InvalidPlan)?;
        let initial_bytes = plan
            .initial_buffer_bytes()
            .ok_or(HttRxRefillError::InvalidPlan)?;
        let fallback_bytes = plan.fallback_buffer_bytes();
        let memory = plan.memory();
        let ring_pointer = dma_pointer(observation, memory.ring_iova, ring_bytes)
            .ok_or(HttRxRefillError::InvalidPlan)?;
        let index_pointer = dma_pointer(
            observation,
            memory.alloc_index_iova,
            crate::sunfish_htt_rx_ring::SUNFISH_HTT_RX_RING_INDEX_BYTES,
        )
        .ok_or(HttRxRefillError::InvalidPlan)?;
        let initial_pointer = dma_pointer(observation, memory.initial_buffers_iova, initial_bytes)
            .ok_or(HttRxRefillError::InvalidPlan)?;
        let fallback_pointer =
            dma_pointer(observation, memory.fallback_buffers_iova, fallback_bytes)
                .ok_or(HttRxRefillError::InvalidPlan)?;
        if !(ring_pointer as usize).is_multiple_of(8)
            || !(index_pointer as usize).is_multiple_of(4)
            || !(initial_pointer as usize).is_multiple_of(8)
            || !(fallback_pointer as usize).is_multiple_of(8)
            || !buffers
                .backing_matches_virtual_bases(initial_pointer as usize, fallback_pointer as usize)
        {
            return Err(HttRxRefillError::InvalidPlan);
        }
        let buffer_count = plan
            .initial_fill_level()
            .checked_add(plan.fallback_buffer_count())
            .ok_or(HttRxRefillError::InvalidPlan)?;
        for buffer_id in 0..buffer_count {
            let iova = plan
                .buffer_iova(buffer_id)
                .map_err(|_| HttRxRefillError::InvalidPlan)?;
            if dma_pointer(observation, iova, u64::from(plan.buffer_bytes())).is_none() {
                return Err(HttRxRefillError::InvalidPlan);
            }
        }

        let mut io = NativeHttRxRingIo {
            plan,
            observation,
            _authority: &guard,
        };
        let result = refill_state
            .refill_batch(plan, session, buffer_ids, buffers, &mut io)
            .map_err(HttRxRefillError::Refill);
        drop(io);
        drop(guard);
        result
    }
}

impl<G: Gate> RegisterIo for NativeIo<G> {
    fn read32(&mut self, offset: usize) -> Result<u32, regs::Error> {
        register_access(offset, AccessKind::Read)?;
        let mapping = self.mapping;
        let generation = self.generation;
        let guard = self.gate.lock()?;
        if !authorized(guard.observation(), mapping, generation) {
            return Err(regs::Error::Io);
        }
        let value = read32_mapping(mapping, offset)?;
        drop(guard);
        Ok(value)
    }

    fn write32(&mut self, offset: usize, value: u32) -> Result<(), regs::Error> {
        register_access(offset, AccessKind::Write)?;
        let mapping = self.mapping;
        let generation = self.generation;
        let guard = self.gate.lock()?;
        if !authorized(guard.observation(), mapping, generation) {
            return Err(regs::Error::Io);
        }
        write32_mapping(mapping, offset, value)?;
        drop(guard);
        Ok(())
    }
}

impl<G: Gate> SetupWriter for NativeIo<G> {
    fn write(
        &mut self,
        generation: u64,
        iova_base: u64,
        iova_bytes: u64,
        engine: u8,
        expected_snapshot: Option<EngineSnapshot>,
        register: Register,
        value: u32,
    ) -> Result<(), SetupWriterError> {
        let engine_index = usize::from(engine);
        let spec = regs::register_spec(engine_index, register).map_err(|_| SetupWriterError::Io)?;
        register_access(spec.offset, AccessKind::Write).map_err(|_| SetupWriterError::Io)?;
        let mapping = self.mapping;
        let expected_generation = self.generation;
        let guard = self.gate.lock().map_err(|_| SetupWriterError::LeaseLost)?;
        let observation = guard.observation();
        if !authorized(observation, mapping, generation)
            || generation != expected_generation
            || iova_base != observation.dma_iova
            || iova_bytes != observation.dma_bytes
        {
            return Err(SetupWriterError::LeaseLost);
        }
        if let Some(expected) = expected_snapshot {
            if expected.generation != generation
                || snapshot_mapping(mapping, engine_index, generation)
                    .map_err(|_| SetupWriterError::Io)?
                    != expected
            {
                return Err(SetupWriterError::LeaseLost);
            }
        }
        write32_mapping(mapping, spec.offset, value).map_err(|_| SetupWriterError::Io)?;
        drop(guard);
        Ok(())
    }
}
