//! Adapter from one verified Sunfish UFS LUN to the modem remote-storage API.
//!
//! This module does not initialize UFS, discover a LUN, read a GPT, or acquire
//! DMA/SMMU authority. Construction requires the result of paired GPT
//! validation and a trusted expected disk GUID. The supplied `BlockIo` must
//! issue exact bounded UFS operations through a live exclusive owner; it must
//! report the actual fresh-boot lease and exact translated SID 0x300 route on
//! every observation. No production `BlockIo` is wired yet.
#![forbid(unsafe_code)]

use crate::qcom_modem_handlers::{Fault, IO_BYTES};
use crate::qcom_modem_services::Partition;
use crate::qcom_modem_storage::{
    self as modem_storage, Configuration, Digest, Io, Observation, Profile, SECTOR_BYTES,
};
use crate::qcom_tftp::ReadSourceIo;
use crate::qcom_ufs_gpt::{PartitionExtent, VerifiedModemPartitions};
use crate::qcom_ufs_platform::SMMU_STREAM_ID;
use crate::sunfish_smmu::{SUNFISH_MAX_CONTEXT_BANKS, SUNFISH_MAX_SMR_SLOTS};

const READ10_MAX_LBA: u64 = u32::MAX as u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Extent {
    pub lun: u8,
    pub first_lba: u64,
    pub sectors: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidConfiguration,
    DiskGuidMismatch,
    InvalidExtent,
    ExtentOverlap,
}

/// Live authority and media facts. Implementations must derive these values
/// from the retained UFS lease, UFS capacity/discovery result, and live SMMU
/// state; constants from a manifest or the caller are not evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaObservation {
    pub generation: u64,
    pub identity: u64,
    pub device: Digest,
    pub gpt_disk_guid: [u8; 16],
    /// Immutable checkpoint identities verified while this UFS lease was
    /// acquired; these are not hashes of mutable post-write media contents.
    pub source_digests: [Digest; 4],
    pub fresh_boot: bool,
    pub exclusive: bool,
    pub platform_ready: bool,
    pub dma_uncached: bool,
    pub dma_exclusive: bool,
    pub dma_translated: bool,
    pub dma_stream_id: u16,
    pub smmu_route_slot: Option<u8>,
    pub smmu_route_id: u16,
    pub smmu_route_mask: u16,
    pub smmu_route_translated: bool,
    pub smmu_route_context_bank: Option<u8>,
    pub smmu_context_bank: Option<u8>,
    pub smmu_context_enabled: bool,
    pub lun: u8,
    /// Capacity in the modem storage API's 512-byte sector units.
    pub lun_sectors: u64,
    /// Native SCSI logical block size reported by READ CAPACITY.
    pub logical_block_bytes: u32,
    pub write_supported: bool,
    pub sync_cache_supported: bool,
}

/// Exact, bounded block operations for the single LUN containing Sunfish's
/// modemst1/modemst2/fsg/fsc GPT entries. `Failed` from a write must guarantee
/// that no bytes were submitted; any ambiguous completion must be `Uncertain`
/// so the modem storage owner quarantines the provider instead of retrying.
pub trait BlockIo {
    fn observe(&mut self) -> Result<MediaObservation, Fault>;
    fn read_blocks(&mut self, first_lba: u64, out: &mut [u8]) -> Result<(), Fault>;
    fn write_blocks(&mut self, first_lba: u64, bytes: &[u8]) -> Result<(), Fault>;
    fn synchronize_cache(&mut self) -> Result<(), Fault>;
}

/// Maps the four remote-storage service names to GPT extents in their fixed
/// protocol order: modem_fs1→modemst1, modem_fs2→modemst2, modem_fsg→fsg,
/// modem_fsc→fsc. The disk GUID is checked against trusted setup evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartitionMap {
    configuration: Configuration,
    disk_guid: [u8; 16],
    extents: [Extent; 4],
    super_extent: Extent,
    logical_block_bytes: u32,
}

impl PartitionMap {
    pub fn from_verified_gpt(
        configuration: Configuration,
        expected_disk_guid: [u8; 16],
        gpt: VerifiedModemPartitions,
        lun: u8,
    ) -> Result<Self, Error> {
        Profile::validate(configuration).map_err(|_| Error::InvalidConfiguration)?;
        let gpt = gpt.partitions();
        if expected_disk_guid == [0; 16] || gpt.disk_guid != expected_disk_guid {
            return Err(Error::DiskGuidMismatch);
        }
        if !crate::qcom_ufs_protocol::supported_logical_block_bytes(gpt.logical_block_bytes) {
            return Err(Error::InvalidExtent);
        }
        let sectors_per_block = u64::from(gpt.logical_block_bytes) / SECTOR_BYTES;
        let gpt_extents = [gpt.modemst1, gpt.modemst2, gpt.fsg, gpt.fsc];
        if gpt_extents.iter().any(|value| {
            value.sectors == 0
                || value
                    .first_lba
                    .checked_add(value.sectors - 1)
                    .is_none_or(|last| last != value.last_lba)
        }) {
            return Err(Error::InvalidExtent);
        }
        let extents = [
            extent(lun, gpt_extents[0], sectors_per_block).ok_or(Error::InvalidExtent)?,
            extent(lun, gpt_extents[1], sectors_per_block).ok_or(Error::InvalidExtent)?,
            extent(lun, gpt_extents[2], sectors_per_block).ok_or(Error::InvalidExtent)?,
            extent(lun, gpt_extents[3], sectors_per_block).ok_or(Error::InvalidExtent)?,
        ];
        let super_extent =
            extent(lun, gpt.super_partition, sectors_per_block).ok_or(Error::InvalidExtent)?;
        if super_extent.sectors == 0
            || super_extent.first_lba > READ10_MAX_LBA
            || super_extent
                .first_lba
                .checked_add(super_extent.sectors - 1)
                .is_none_or(|last| last > READ10_MAX_LBA)
            || extents.iter().any(|current| {
                current.lun == super_extent.lun
                    && ranges_overlap(
                        current.first_lba,
                        current.sectors,
                        super_extent.first_lba,
                        super_extent.sectors,
                    )
            })
        {
            return Err(Error::InvalidExtent);
        }
        for (index, current) in extents.iter().enumerate() {
            let binding = configuration.bindings[index];
            let sectors = binding.disk.bytes / SECTOR_BYTES;
            if current.first_lba > READ10_MAX_LBA
                || current
                    .first_lba
                    .checked_add(current.sectors - 1)
                    .is_none_or(|last| last > READ10_MAX_LBA)
                || current.sectors.checked_mul(SECTOR_BYTES) != Some(binding.disk.bytes)
            {
                return Err(Error::InvalidExtent);
            }
            if index > 0
                && extents[..index].iter().any(|previous| {
                    previous.lun == current.lun
                        && ranges_overlap(
                            previous.first_lba,
                            previous.sectors,
                            current.first_lba,
                            current.sectors,
                        )
                })
            {
                return Err(Error::ExtentOverlap);
            }
            if sectors != current.sectors {
                return Err(Error::InvalidExtent);
            }
        }
        Ok(Self {
            configuration,
            disk_guid: expected_disk_guid,
            extents,
            super_extent,
            logical_block_bytes: gpt.logical_block_bytes,
        })
    }

    pub const fn configuration(self) -> Configuration {
        self.configuration
    }

    pub const fn disk_guid(self) -> [u8; 16] {
        self.disk_guid
    }

    pub fn extent(self, partition: Partition) -> Extent {
        self.extents[modem_storage::index(partition)]
    }

    pub const fn super_extent(self) -> Extent {
        self.super_extent
    }
}

fn extent(lun: u8, value: PartitionExtent, sectors_per_block: u64) -> Option<Extent> {
    let first_lba = value.first_lba.checked_mul(sectors_per_block)?;
    let sectors = value.sectors.checked_mul(sectors_per_block)?;
    let last_lba = first_lba.checked_add(sectors.checked_sub(1)?)?;
    let expected_last = value
        .last_lba
        .checked_add(1)?
        .checked_mul(sectors_per_block)?
        .checked_sub(1)?;
    if last_lba != expected_last {
        return None;
    }
    Some(Extent {
        lun,
        first_lba,
        sectors,
    })
}

fn ranges_overlap(a: u64, a_sectors: u64, b: u64, b_sectors: u64) -> bool {
    let Some(a_end) = a.checked_add(a_sectors) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_sectors) else {
        return true;
    };
    a < b_end && b < a_end
}

pub struct UfsModemStorage<B: BlockIo> {
    map: PartitionMap,
    block: B,
    super_faulted: bool,
}

impl<B: BlockIo> UfsModemStorage<B> {
    /// Own the block provider without performing media IO. Readiness is
    /// reported only after the first live observation passes every gate.
    pub const fn new(map: PartitionMap, block: B) -> Self {
        Self {
            map,
            block,
            super_faulted: false,
        }
    }

    pub const fn partition_map(&self) -> PartitionMap {
        self.map
    }

    /// Borrow a read-only byte source over the verified GPT `super` partition.
    /// The view is bound to this provider's live device, boot generation, LUN,
    /// disk GUID, and translated SMMU lease. It exposes no write operation.
    pub fn super_source(&mut self) -> Result<UfsSuperReadSource<'_, B>, Fault> {
        if self.super_faulted {
            return Err(Fault::Failed);
        }
        let observed = self.live_observation()?;
        let extent = self.map.super_extent;
        let partition_bytes = extent
            .sectors
            .checked_mul(SECTOR_BYTES)
            .filter(|bytes| *bytes != 0)
            .ok_or(Fault::Failed)?;
        if extent.lun != observed.lun
            || extent
                .first_lba
                .checked_add(extent.sectors)
                .is_none_or(|end| end > observed.lun_sectors)
        {
            return Err(Fault::Failed);
        }
        Ok(UfsSuperReadSource {
            storage: self,
            partition_bytes,
            identity: observed.identity,
            generation: observed.generation,
            lun: observed.lun,
            lun_sectors: observed.lun_sectors,
        })
    }

    fn live_observation(&mut self) -> Result<MediaObservation, Fault> {
        let observed = self.block.observe()?;
        let required_writes = self
            .map
            .configuration
            .bindings
            .iter()
            .any(|binding| binding.disk.writable);
        let required_sync = self
            .map
            .configuration
            .bindings
            .iter()
            .any(|binding| binding.disk.durable);
        let route_slot_valid = observed
            .smmu_route_slot
            .is_some_and(|slot| usize::from(slot) < SUNFISH_MAX_SMR_SLOTS);
        let route_context_bank_valid = observed
            .smmu_route_context_bank
            .is_some_and(|bank| usize::from(bank) < SUNFISH_MAX_CONTEXT_BANKS);
        let context_bank_valid = observed
            .smmu_context_bank
            .is_some_and(|bank| usize::from(bank) < SUNFISH_MAX_CONTEXT_BANKS);
        let mut extent_capacity_ok = true;
        for extent in self
            .map
            .extents
            .iter()
            .copied()
            .chain(core::iter::once(self.map.super_extent))
        {
            if extent
                .first_lba
                .checked_add(extent.sectors)
                .is_none_or(|end| end > observed.lun_sectors)
            {
                extent_capacity_ok = false;
                break;
            }
        }
        if observed.generation != self.map.configuration.generation
            || observed.identity == 0
            || observed.device != self.map.configuration.device
            || observed.gpt_disk_guid != self.map.disk_guid
            || observed.source_digests
                != self
                    .map
                    .configuration
                    .bindings
                    .map(|binding| binding.source)
            || !observed.fresh_boot
            || !observed.exclusive
            || !observed.platform_ready
            || !observed.dma_uncached
            || !observed.dma_exclusive
            || !observed.dma_translated
            || observed.dma_stream_id != SMMU_STREAM_ID
            || !route_slot_valid
            || observed.smmu_route_id != SMMU_STREAM_ID
            || observed.smmu_route_mask != 0
            || !observed.smmu_route_translated
            || !route_context_bank_valid
            || !context_bank_valid
            || observed.smmu_route_context_bank != observed.smmu_context_bank
            || !observed.smmu_context_enabled
            || observed.lun != self.map.extents[0].lun
            || observed.lun_sectors == 0
            || observed.logical_block_bytes != self.map.logical_block_bytes
            || !crate::qcom_ufs_protocol::supported_logical_block_bytes(
                observed.logical_block_bytes,
            )
            || observed.lun_sectors % (u64::from(observed.logical_block_bytes) / SECTOR_BYTES) != 0
            || !extent_capacity_ok
            || (required_writes && !observed.write_supported)
            || (required_sync && !observed.sync_cache_supported)
        {
            return Err(Fault::Failed);
        }
        Ok(observed)
    }

    fn resolve(&self, partition: Partition, offset: u64, bytes: usize) -> Result<u64, Fault> {
        let binding = self.map.configuration.bindings[modem_storage::index(partition)];
        let extent = self.map.extent(partition);
        if bytes == 0
            || bytes > IO_BYTES
            || bytes as u64 % SECTOR_BYTES != 0
            || offset % SECTOR_BYTES != 0
            || offset
                .checked_add(bytes as u64)
                .is_none_or(|end| end > binding.disk.bytes)
        {
            return Err(Fault::Failed);
        }
        let relative_lba = offset / SECTOR_BYTES;
        let lba = extent
            .first_lba
            .checked_add(relative_lba)
            .ok_or(Fault::Failed)?;
        let sectors = bytes as u64 / SECTOR_BYTES;
        if lba
            .checked_add(sectors - 1)
            .is_none_or(|last| last > READ10_MAX_LBA)
            || relative_lba
                .checked_add(sectors)
                .is_none_or(|end| end > extent.sectors)
        {
            return Err(Fault::Failed);
        }
        Ok(lba)
    }
}

/// Read-only byte access to a verified `super` GPT extent. Each transfer is
/// sector aligned, capped at the UFS READ(10) data limit, and bracketed by
/// fresh lease observations. Any uncertain read or lease change permanently
/// latches this source off.
pub struct UfsSuperReadSource<'a, B: BlockIo> {
    storage: &'a mut UfsModemStorage<B>,
    partition_bytes: u64,
    identity: u64,
    generation: u64,
    lun: u8,
    lun_sectors: u64,
}

impl<B: BlockIo> UfsSuperReadSource<'_, B> {
    fn observe_live(&mut self) -> Result<(), Fault> {
        if self.storage.super_faulted {
            return Err(Fault::Failed);
        }
        let observation = self.storage.live_observation()?;
        if observation.identity != self.identity
            || observation.generation != self.generation
            || observation.lun != self.lun
            || observation.lun_sectors != self.lun_sectors
        {
            return Err(Fault::Failed);
        }
        Ok(())
    }

    fn latch_fault(&mut self) {
        self.storage.super_faulted = true;
    }
}

impl<B: BlockIo> ReadSourceIo for UfsSuperReadSource<'_, B> {
    fn len(&mut self) -> Result<u64, ()> {
        if self.observe_live().is_err() {
            self.latch_fault();
            return Err(());
        }
        Ok(self.partition_bytes)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        if self.observe_live().is_err() {
            self.latch_fault();
            return Err(());
        }
        let end = offset.checked_add(out.len() as u64).ok_or(())?;
        if end > self.partition_bytes {
            return Err(());
        }

        let extent = self.storage.map.super_extent;
        let mut completed = 0usize;
        let mut scratch = [0u8; IO_BYTES];
        while completed < out.len() {
            if self.observe_live().is_err() {
                self.latch_fault();
                return Err(());
            }
            let byte_at = offset.checked_add(completed as u64).ok_or(())?;
            let relative_lba = byte_at / SECTOR_BYTES;
            let within_sector = (byte_at % SECTOR_BYTES) as usize;
            let lba = extent.first_lba.checked_add(relative_lba).ok_or(())?;
            let take = (out.len() - completed).min(IO_BYTES - within_sector);
            let transfer_bytes = (within_sector + take)
                .checked_add(SECTOR_BYTES as usize - 1)
                .ok_or(())?
                / SECTOR_BYTES as usize
                * SECTOR_BYTES as usize;
            if transfer_bytes == 0 || transfer_bytes > IO_BYTES {
                return Err(());
            }
            if self
                .storage
                .block
                .read_blocks(lba, &mut scratch[..transfer_bytes])
                .is_err()
            {
                self.latch_fault();
                return Err(());
            }
            if self.observe_live().is_err() {
                self.latch_fault();
                return Err(());
            }
            out[completed..completed + take]
                .copy_from_slice(&scratch[within_sector..within_sector + take]);
            completed += take;
        }
        Ok(())
    }
}

impl<B: BlockIo> Io for UfsModemStorage<B> {
    fn observe(&mut self) -> Result<Observation, Fault> {
        let observed = self.live_observation()?;
        Ok(Observation {
            configuration: self.map.configuration,
            identity: observed.identity,
            ready: true,
        })
    }

    fn read(&mut self, partition: Partition, offset: u64, out: &mut [u8]) -> Result<usize, Fault> {
        self.live_observation()?;
        let lba = self.resolve(partition, offset, out.len())?;
        match self.block.read_blocks(lba, out) {
            Ok(()) => {
                self.live_observation()?;
                Ok(out.len())
            }
            Err(Fault::Failed) => {
                self.live_observation()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(Fault::Uncertain),
        }
    }

    fn write(&mut self, partition: Partition, offset: u64, bytes: &[u8]) -> Result<usize, Fault> {
        let observed = self.live_observation()?;
        let binding = self.map.configuration.bindings[modem_storage::index(partition)];
        if !binding.disk.writable || !observed.write_supported {
            return Err(Fault::Failed);
        }
        let lba = self.resolve(partition, offset, bytes.len())?;
        match self.block.write_blocks(lba, bytes) {
            Ok(()) => {
                self.live_observation()?;
                Ok(bytes.len())
            }
            Err(Fault::Failed) => {
                self.live_observation()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(Fault::Uncertain),
        }
    }

    fn flush(&mut self, partition: Partition) -> Result<(), Fault> {
        let observed = self.live_observation()?;
        let binding = self.map.configuration.bindings[modem_storage::index(partition)];
        if !binding.disk.durable || !observed.sync_cache_supported {
            return Err(Fault::Failed);
        }
        match self.block.synchronize_cache() {
            Ok(()) => {
                self.live_observation()?;
                Ok(())
            }
            Err(Fault::Failed) => {
                self.live_observation()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(Fault::Uncertain),
        }
    }
}
