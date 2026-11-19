//! Capability-gated volatile UFSHCI and DMA adapters.
//!
//! This module does not acquire Microkit capabilities, initialize clocks/rails,
//! configure SMMU, or discover a DMA aperture. A platform authority must supply
//! an unsafe `Gate` implementation that pins the exact mappings and exclusive
//! fresh-boot lease for each access. No production gate is wired yet.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::qcom_ufs_host::{self as host, DmaIo, Error, RegisterIo};
pub use crate::qcom_ufs_platform::MAX_DMA_APERTURE_BYTES;
use crate::qcom_ufs_platform::{HOST_BASE, HOST_BYTES, SMMU_STREAM_ID};
pub const HCI_VIRTUAL_BASE: usize = 0x6400_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub fresh_boot: bool,
    pub exclusive: bool,
    /// The source-derived HBA/PHY power stage required for CAP/VER/HCS reads.
    pub controller_power_ready: bool,
    /// Full SMMU/DMA/IRQ and platform readiness for HCE and command traffic.
    pub platform_ready: bool,
    pub host_device_uncached: bool,
    pub host_virtual: u64,
    pub host_physical: u64,
    pub host_bytes: u64,
    pub dma_virtual: u64,
    pub dma_iova: u64,
    pub dma_bytes: u64,
    pub dma_stream_id: u16,
    pub smmu_route_slot: Option<u8>,
    pub smmu_route_id: u16,
    pub smmu_route_mask: u16,
    pub smmu_route_translated: bool,
    pub smmu_context_bank: Option<u8>,
    pub smmu_context_enabled: bool,
    pub dma_uncached: bool,
    pub dma_exclusive: bool,
    pub dma_translated: bool,
}

pub trait Guard {
    fn observation(&self) -> Observation;
}

/// Native authority boundary for mapped UFS resources.
///
/// # Safety
/// `lock` must pin the exact Microkit mappings and exclusive resource lease
/// reported by its `Guard` until that guard is dropped. It must serialize with
/// reset, teardown, remapping, and competing owners. The observation must come
/// from live authority state, never a manifest constant or peer-provided data.
/// SMMU evidence must describe the exact, unmasked UFS SID route, a translated
/// enabled context bank, and the live DMA page-table mapping for this aperture;
/// a valid context-bank index or static route plan alone is insufficient.
pub unsafe trait Gate {
    type Guard<'a>: Guard
    where
        Self: 'a;

    fn lock(&mut self) -> Result<Self::Guard<'_>, Error>;
}

pub trait Delay {
    fn delay_us(&mut self, microseconds: u32);
}

#[derive(Clone, Copy)]
pub struct HciMapping {
    pub base: *mut u8,
    pub virtual_base: u64,
    pub bytes: usize,
}

#[derive(Clone, Copy)]
pub struct DmaMapping {
    pub cpu_base: *mut u8,
    pub virtual_base: u64,
    pub iova_base: u64,
    pub bytes: u64,
}

fn smmu_authorized(observation: Observation) -> bool {
    observation.dma_translated
        && observation.dma_stream_id == SMMU_STREAM_ID
        && observation
            .smmu_route_slot
            .is_some_and(|slot| usize::from(slot) < crate::sunfish_smmu::SUNFISH_MAX_SMR_SLOTS)
        && observation.smmu_route_id == SMMU_STREAM_ID
        && observation.smmu_route_mask == 0
        && observation.smmu_route_translated
        && observation
            .smmu_context_bank
            .is_some_and(|bank| usize::from(bank) < crate::sunfish_smmu::SUNFISH_MAX_CONTEXT_BANKS)
        && observation.smmu_context_enabled
}

fn hci_power_authorized(
    observation: Observation,
    mapping: HciMapping,
    generation: u64,
) -> Result<(), Error> {
    if generation == 0
        || observation.generation != generation
        || !observation.fresh_boot
        || !observation.exclusive
        || !observation.controller_power_ready
        || !observation.host_device_uncached
        || observation.host_virtual != mapping.virtual_base
        || observation.host_virtual != HCI_VIRTUAL_BASE as u64
        || observation.host_physical != HOST_BASE
        || observation.host_bytes != HOST_BYTES
        || mapping.bytes as u64 != HOST_BYTES
    {
        return Err(Error::PlatformNotReady);
    }
    Ok(())
}

fn hci_authorized(
    observation: Observation,
    mapping: HciMapping,
    generation: u64,
) -> Result<(), Error> {
    hci_power_authorized(observation, mapping, generation)?;
    if !observation.platform_ready
        || !smmu_authorized(observation)
        || observation.dma_virtual == 0
        || observation.dma_iova == 0
        || observation.dma_bytes == 0
        || observation.dma_bytes > MAX_DMA_APERTURE_BYTES
        || !observation.dma_uncached
        || !observation.dma_exclusive
        || !observation.dma_iova.is_multiple_of(4096)
        || !observation.dma_bytes.is_multiple_of(4096)
        || observation
            .dma_iova
            .checked_add(observation.dma_bytes)
            .is_none()
        || !observation.dma_virtual.is_multiple_of(4096)
        || observation
            .dma_virtual
            .checked_add(observation.dma_bytes)
            .is_none()
        || ranges_overlap(
            observation.host_virtual,
            HOST_BYTES,
            observation.dma_virtual,
            observation.dma_bytes,
        )
    {
        return Err(Error::PlatformNotReady);
    }
    Ok(())
}

fn dma_authorized(
    observation: Observation,
    mapping: DmaMapping,
    generation: u64,
) -> Result<(), Error> {
    if generation == 0
        || observation.generation != generation
        || !observation.fresh_boot
        || !observation.exclusive
        || !observation.controller_power_ready
        || !observation.platform_ready
        || !observation.host_device_uncached
        || !smmu_authorized(observation)
        || !observation.dma_uncached
        || !observation.dma_exclusive
        || observation.dma_virtual != mapping.virtual_base
        || observation.dma_iova != mapping.iova_base
        || observation.dma_bytes != mapping.bytes
        || observation.host_virtual != HCI_VIRTUAL_BASE as u64
        || observation.host_physical != HOST_BASE
        || observation.host_bytes != HOST_BYTES
        || ranges_overlap(
            observation.host_virtual,
            HOST_BYTES,
            observation.dma_virtual,
            observation.dma_bytes,
        )
        || mapping.iova_base == 0
        || mapping.bytes == 0
        || mapping.bytes > MAX_DMA_APERTURE_BYTES
        || !mapping.iova_base.is_multiple_of(4096)
        || !mapping.bytes.is_multiple_of(4096)
        || mapping.iova_base.checked_add(mapping.bytes).is_none()
        || mapping.cpu_base.is_null()
        || mapping.cpu_base as usize % 4096 != 0
        || !mapping.virtual_base.is_multiple_of(4096)
    {
        return Err(Error::PlatformNotReady);
    }
    Ok(())
}

fn hci_readable(offset: u32) -> bool {
    matches!(
        offset,
        host::REG_CAPABILITIES
            | host::REG_VERSION
            | host::REG_INTERRUPT_STATUS
            | host::REG_CONTROLLER_STATUS
            | host::REG_CONTROLLER_ENABLE
            | host::REG_UTRL_DOORBELL
            | host::REG_UIC_ARGUMENT_2
    )
}

fn hci_snapshot_readable(offset: u32) -> bool {
    matches!(
        offset,
        host::REG_CAPABILITIES | host::REG_VERSION | host::REG_CONTROLLER_STATUS
    )
}

fn hci_writable(offset: u32) -> bool {
    matches!(
        offset,
        host::REG_INTERRUPT_STATUS
            | host::REG_INTERRUPT_ENABLE
            | host::REG_CONTROLLER_ENABLE
            | host::REG_UTRL_BASE_LO
            | host::REG_UTRL_BASE_HI
            | host::REG_UTRL_DOORBELL
            | host::REG_UTRL_RUN_STOP
            | host::REG_UTMRL_BASE_LO
            | host::REG_UTMRL_BASE_HI
            | host::REG_UTMRL_RUN_STOP
            | host::REG_UIC_COMMAND
            | host::REG_UIC_ARGUMENT_1
            | host::REG_UIC_ARGUMENT_2
            | host::REG_UIC_ARGUMENT_3
    )
}

fn check_register(offset: u32, allowed: bool) -> Result<(), Error> {
    if offset % 4 != 0
        || offset
            .checked_add(4)
            .is_none_or(|end| end > HOST_BYTES as u32)
        || !allowed
    {
        return Err(Error::Io);
    }
    Ok(())
}

fn barrier() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: DSB SY orders device and DMA accesses without touching memory.
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
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

pub struct NativeRegisterIo<G: Gate, T: Delay> {
    mapping: HciMapping,
    generation: u64,
    gate: G,
    delay: T,
}

impl<G: Gate, T: Delay> NativeRegisterIo<G, T> {
    /// # Safety
    /// `mapping` must be the exact, aligned `ufs_hci_mmio` device mapping from
    /// the UFS Microkit profile and remain valid for this object's lifetime.
    /// The gate must pin it while each method accesses it. The mapping must not
    /// have a writable alias in another protection domain. It may authorize
    /// CAP/VER/HCS reads once `controller_power_ready` is live, but must require
    /// full platform readiness for all other HCI accesses.
    pub unsafe fn new(mapping: HciMapping, mut gate: G, delay: T) -> Result<Self, Error> {
        if mapping.base.is_null()
            || mapping.base as usize % 4 != 0
            || (mapping.base as usize).checked_add(mapping.bytes).is_none()
            || mapping.virtual_base != HCI_VIRTUAL_BASE as u64
            || !mapping.virtual_base.is_multiple_of(4096)
            || ((!cfg!(test) || cfg!(target_os = "none"))
                && mapping.base as usize as u64 != mapping.virtual_base)
            || mapping.bytes as u64 != HOST_BYTES
        {
            return Err(Error::PlatformNotReady);
        }
        let guard = gate.lock()?;
        let observation = guard.observation();
        hci_power_authorized(observation, mapping, observation.generation)?;
        drop(guard);
        Ok(Self {
            mapping,
            generation: observation.generation,
            gate,
            delay,
        })
    }
}

impl<G: Gate, T: Delay> RegisterIo for NativeRegisterIo<G, T> {
    fn controller_power_ready(&mut self) -> bool {
        let mapping = self.mapping;
        let generation = self.generation;
        self.gate.lock().is_ok_and(|guard| {
            hci_power_authorized(guard.observation(), mapping, generation).is_ok()
        })
    }

    fn platform_ready(&mut self) -> bool {
        let mapping = self.mapping;
        let generation = self.generation;
        self.gate
            .lock()
            .is_ok_and(|guard| hci_authorized(guard.observation(), mapping, generation).is_ok())
    }

    fn read32(&mut self, offset: u32) -> Result<u32, Error> {
        check_register(offset, hci_readable(offset))?;
        let mapping = self.mapping;
        let _guard = self.gate.lock()?;
        let observation = _guard.observation();
        if hci_snapshot_readable(offset) {
            hci_power_authorized(observation, mapping, self.generation)?;
        } else {
            hci_authorized(observation, mapping, self.generation)?;
        }
        // SAFETY: the gate pins an aligned, authorized device mapping and the
        // register offset is in-range and on the read allowlist.
        let value = unsafe {
            mapping
                .base
                .add(offset as usize)
                .cast::<u32>()
                .read_volatile()
        };
        Ok(u32::from_le(value))
    }

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Error> {
        check_register(offset, hci_writable(offset))?;
        let mapping = self.mapping;
        let _guard = self.gate.lock()?;
        hci_authorized(_guard.observation(), mapping, self.generation)?;
        // SAFETY: the gate pins an authorized device mapping and the offset is
        // in-range and on the narrow host-driver write allowlist.
        unsafe {
            mapping
                .base
                .add(offset as usize)
                .cast::<u32>()
                .write_volatile(value.to_le())
        };
        Ok(())
    }

    fn dma_write_barrier(&mut self) -> Result<(), Error> {
        let mapping = self.mapping;
        let _guard = self.gate.lock()?;
        hci_authorized(_guard.observation(), mapping, self.generation)?;
        barrier();
        Ok(())
    }

    fn dma_read_barrier(&mut self) -> Result<(), Error> {
        let mapping = self.mapping;
        let _guard = self.gate.lock()?;
        hci_authorized(_guard.observation(), mapping, self.generation)?;
        barrier();
        Ok(())
    }

    fn delay_us(&mut self, microseconds: u32) {
        if microseconds == 0 || microseconds > 1_000_000 {
            return;
        }
        let mapping = self.mapping;
        let generation = self.generation;
        if let Ok(_guard) = self.gate.lock() {
            if hci_authorized(_guard.observation(), mapping, generation).is_ok() {
                self.delay.delay_us(microseconds);
            }
        }
    }
}

pub struct NativeDmaIo<G: Gate> {
    mapping: DmaMapping,
    generation: u64,
    gate: G,
}

impl<G: Gate> NativeDmaIo<G> {
    /// # Safety
    /// `mapping` must name one exact, page-aligned CPU mapping of a device-
    /// translated UFS IOVA aperture. The region must be exclusively reserved,
    /// stable, writable, and free of cacheable or writable aliases. The gate
    /// must pin both the mapping and SMMU lease across each operation. This
    /// constructor never invents or selects a DMA address.
    pub unsafe fn new(mapping: DmaMapping, mut gate: G) -> Result<Self, Error> {
        if mapping.cpu_base.is_null()
            || mapping.cpu_base as usize % 4096 != 0
            || (mapping.cpu_base as usize)
                .checked_add(usize::try_from(mapping.bytes).unwrap_or(usize::MAX))
                .is_none()
            || mapping.iova_base == 0
            || !mapping.iova_base.is_multiple_of(4096)
            || mapping.bytes == 0
            || !mapping.bytes.is_multiple_of(4096)
            || mapping.bytes > MAX_DMA_APERTURE_BYTES
            || mapping.iova_base.checked_add(mapping.bytes).is_none()
            || mapping.virtual_base.checked_add(mapping.bytes).is_none()
            || ((!cfg!(test) || cfg!(target_os = "none"))
                && mapping.cpu_base as usize as u64 != mapping.virtual_base)
        {
            return Err(Error::InvalidDma);
        }
        let guard = gate.lock()?;
        let observation = guard.observation();
        dma_authorized(observation, mapping, observation.generation)?;
        drop(guard);
        Ok(Self {
            mapping,
            generation: observation.generation,
            gate,
        })
    }

    fn range(&self, address: u64, bytes: usize) -> Result<usize, Error> {
        if bytes == 0 {
            return Err(Error::InvalidDma);
        }
        let bytes = u64::try_from(bytes).map_err(|_| Error::InvalidDma)?;
        let end = address.checked_add(bytes).ok_or(Error::InvalidDma)?;
        let aperture_end = self
            .mapping
            .iova_base
            .checked_add(self.mapping.bytes)
            .ok_or(Error::InvalidDma)?;
        if address < self.mapping.iova_base || end > aperture_end {
            return Err(Error::InvalidDma);
        }
        usize::try_from(address - self.mapping.iova_base).map_err(|_| Error::InvalidDma)
    }
}

impl<G: Gate> DmaIo for NativeDmaIo<G> {
    fn ready(&mut self) -> bool {
        let mapping = self.mapping;
        let generation = self.generation;
        self.gate
            .lock()
            .is_ok_and(|guard| dma_authorized(guard.observation(), mapping, generation).is_ok())
    }

    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Error> {
        let offset = self.range(address, bytes.len())?;
        let mapping = self.mapping;
        let _guard = self.gate.lock()?;
        dma_authorized(_guard.observation(), mapping, self.generation)?;
        for (index, byte) in bytes.iter().enumerate() {
            // SAFETY: the gate pins this exact DMA mapping and range() checked
            // the full copy against its translated aperture.
            unsafe { mapping.cpu_base.add(offset + index).write_volatile(*byte) };
        }
        barrier();
        Ok(())
    }

    fn read(&mut self, address: u64, out: &mut [u8]) -> Result<(), Error> {
        let offset = self.range(address, out.len())?;
        let mapping = self.mapping;
        let _guard = self.gate.lock()?;
        dma_authorized(_guard.observation(), mapping, self.generation)?;
        barrier();
        for (index, byte) in out.iter_mut().enumerate() {
            // SAFETY: the gate pins this exact DMA mapping and range() checked
            // the full copy against its translated aperture.
            *byte = unsafe { mapping.cpu_base.add(offset + index).read_volatile() };
        }
        Ok(())
    }
}
