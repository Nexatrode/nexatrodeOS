//! Offline CE v3 ring-programming plan with fail-closed readiness checks.
//!
//! The sequence and read/modify/write fields mirror QCACLD's Adrastea legacy
//! `ce_legacy_src_ring_setup` and `ce_legacy_dest_ring_setup`. This module
//! produces a plan only. It does not establish that evidence is live, map
//! memory, touch MMIO, start an engine, or allocate DMA.

#![forbid(unsafe_code)]

use crate::qcom_ce_v3::{DESCRIPTOR_BYTES, MAX_CE_IOVA};
use crate::qcom_ce_v3_profile::{
    EngineConfig, MAX_RING_ALLOCATIONS, Profile, RingAllocation, RingDirection, RingPlan,
};
use crate::qcom_ce_v3_regs::{CE_COUNT, Register};

const SOURCE_ADDRESS_MASK: u32 = 0x1f;
const DESTINATION_ADDRESS_MASK: u32 = 0x1f;
const CONTROL1_MAX_LENGTH_MASK: u32 = 0x0000_ffff;
const WATERMARK_LOW_MASK: u32 = 0xffff_0000;
const WATERMARK_HIGH_MASK: u32 = 0x0000_ffff;
pub const MAX_SETUP_WRITES: usize = 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidEngine,
    LifecycleNotReady,
    InvalidDmaWindow,
    DmaNotReady,
    InvalidRingAllocation,
    MissingRing,
    UnexpectedRing,
    DuplicateRing,
    RingOverlap,
    EngineNotEmpty,
    StaleSnapshot,
    AddressOverflow,
    TooManyWrites,
}

/// Assertions the future native owner must derive from a live exclusive lease.
/// Unit tests can model them, but only an authority implementation can make
/// them meaningful on the phone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupEvidence {
    pub generation: u64,
    pub fresh_boot: bool,
    pub exclusive_ce_owner: bool,
    pub platform_ready: bool,
    pub ce_powered: bool,
    pub ce_reset_released: bool,
    pub engine_quiescent: bool,
    pub translated_dma_ready: bool,
    pub dma_exclusive: bool,
    pub dma_uncached: bool,
    pub iova_base: u64,
    pub iova_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineSnapshot {
    pub generation: u64,
    pub source_size: u32,
    pub destination_size: u32,
    pub source_read_index: u32,
    pub source_write_index: u32,
    pub destination_read_index: u32,
    pub destination_write_index: u32,
    pub source_base_high: u32,
    pub destination_base_high: u32,
    pub control1: u32,
    pub source_watermark: u32,
    pub destination_watermark: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterWrite {
    pub register: Register,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineSetupPlan {
    pub engine: u8,
    snapshot: EngineSnapshot,
    writes: [RegisterWrite; MAX_SETUP_WRITES],
    write_count: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ProfileSetupPlan {
    profile: Profile,
    generation: u64,
    iova_base: u64,
    iova_bytes: u64,
    engines: [EngineSetupPlan; CE_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyProgress {
    Wrote { engine: u8, register: Register },
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyError {
    LeaseLost,
    WriteFailed,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupWriterError {
    LeaseLost,
    Io,
}

/// A narrow writer for ordinary CE register writes.
pub trait SetupWriter {
    fn write(
        &mut self,
        generation: u64,
        iova_base: u64,
        iova_bytes: u64,
        engine: u8,
        expected_snapshot: Option<EngineSnapshot>,
        register: Register,
        value: u32,
    ) -> Result<(), SetupWriterError>;
}

/// One-shot, fail-closed application state for a complete CE setup plan.
///
/// The platform owner must keep this value as the durable attempt record. It
/// is intentionally not `Clone`; after an uncertain effect, the owner must
/// quarantine the CE resources and may not create a replacement attempt for
/// the same generation.
pub struct SetupApply {
    plan: ProfileSetupPlan,
    engine_index: usize,
    write_index: usize,
    failed: bool,
    complete: bool,
    in_flight: bool,
}

impl SetupApply {
    pub const fn new(plan: ProfileSetupPlan) -> Self {
        Self {
            plan,
            engine_index: 0,
            write_index: 0,
            failed: false,
            complete: false,
            in_flight: false,
        }
    }

    pub fn step<W: SetupWriter>(&mut self, writer: &mut W) -> Result<ApplyProgress, ApplyError> {
        if self.failed || self.in_flight {
            self.failed = true;
            return Err(ApplyError::Terminal);
        }
        if self.complete {
            return Ok(ApplyProgress::Complete);
        }

        while self.engine_index < CE_COUNT {
            let engine = self.plan.engines[self.engine_index];
            let writes = engine.writes();
            if self.write_index >= writes.len() {
                self.engine_index += 1;
                self.write_index = 0;
                continue;
            }

            let write = writes[self.write_index];
            self.in_flight = true;
            let expected_snapshot = (self.write_index == 0).then_some(engine.snapshot);
            let result = writer.write(
                self.plan.generation,
                self.plan.iova_base,
                self.plan.iova_bytes,
                engine.engine,
                expected_snapshot,
                write.register,
                write.value,
            );
            self.in_flight = false;
            match result {
                Err(SetupWriterError::LeaseLost) => {
                    self.failed = true;
                    return Err(ApplyError::LeaseLost);
                }
                Err(SetupWriterError::Io) => {
                    self.failed = true;
                    return Err(ApplyError::WriteFailed);
                }
                Ok(()) => {
                    self.write_index += 1;
                    return Ok(ApplyProgress::Wrote {
                        engine: engine.engine,
                        register: write.register,
                    });
                }
            }
        }

        self.complete = true;
        Ok(ApplyProgress::Complete)
    }
}

impl EngineSetupPlan {
    pub fn writes(&self) -> &[RegisterWrite] {
        &self.writes[..self.write_count]
    }
}

impl ProfileSetupPlan {
    pub const fn profile(&self) -> Profile {
        self.profile
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn iova_base(&self) -> u64 {
        self.iova_base
    }

    pub const fn iova_bytes(&self) -> u64 {
        self.iova_bytes
    }

    pub fn engine(&self, engine: usize) -> Option<&EngineSetupPlan> {
        self.engines.get(engine)
    }
}

fn empty_engine_plan(engine: usize, snapshot: EngineSnapshot) -> EngineSetupPlan {
    EngineSetupPlan {
        engine: engine as u8,
        snapshot,
        writes: [RegisterWrite {
            register: Register::SourceBaseLow,
            value: 0,
        }; MAX_SETUP_WRITES],
        write_count: 0,
    }
}

fn validate_evidence(evidence: SetupEvidence) -> Result<(u64, u64), Error> {
    if evidence.generation == 0
        || !evidence.fresh_boot
        || !evidence.exclusive_ce_owner
        || !evidence.platform_ready
        || !evidence.ce_powered
        || !evidence.ce_reset_released
        || !evidence.engine_quiescent
    {
        return Err(Error::LifecycleNotReady);
    }
    if !evidence.translated_dma_ready || !evidence.dma_exclusive || !evidence.dma_uncached {
        return Err(Error::DmaNotReady);
    }
    if evidence.iova_base == 0
        || !evidence
            .iova_base
            .is_multiple_of(crate::qcom_ce_v3_profile::PAGE_BYTES)
        || evidence.iova_bytes == 0
        || !evidence
            .iova_bytes
            .is_multiple_of(crate::qcom_ce_v3_profile::PAGE_BYTES)
    {
        return Err(Error::InvalidDmaWindow);
    }
    let end = evidence
        .iova_base
        .checked_add(evidence.iova_bytes)
        .ok_or(Error::AddressOverflow)?;
    if end > MAX_CE_IOVA {
        return Err(Error::InvalidDmaWindow);
    }
    Ok((evidence.iova_base, end))
}

fn validate_ring(
    engine: usize,
    direction: RingDirection,
    expected_entries: u16,
    ring: Option<RingAllocation>,
    pool_start: u64,
    pool_end: u64,
) -> Result<Option<RingAllocation>, Error> {
    if expected_entries == 0 {
        return if ring.is_none() {
            Ok(None)
        } else {
            Err(Error::UnexpectedRing)
        };
    }
    let allocation = ring.ok_or(Error::MissingRing)?;
    let descriptor_bytes = u64::from(expected_entries)
        .checked_mul(DESCRIPTOR_BYTES as u64)
        .ok_or(Error::AddressOverflow)?;
    let allocation_bytes = descriptor_bytes
        .checked_add(crate::qcom_ce_v3_profile::RING_ALIGNMENT_SLACK_BYTES)
        .ok_or(Error::AddressOverflow)?;
    let allocation_end = allocation
        .descriptor_iova
        .checked_add(allocation_bytes)
        .ok_or(Error::AddressOverflow)?;
    if usize::from(allocation.engine) != engine
        || allocation.direction != direction
        || allocation.entries != expected_entries
        || u64::from(allocation.descriptor_bytes) != descriptor_bytes
        || u64::from(allocation.allocation_bytes) != allocation_bytes
        || !allocation
            .descriptor_iova
            .is_multiple_of(crate::qcom_ce_v3_profile::DMA_ALIGNMENT)
        || allocation.descriptor_iova < pool_start
        || allocation_end > pool_end
        || allocation_end > MAX_CE_IOVA
    {
        return Err(Error::InvalidRingAllocation);
    }
    Ok(Some(allocation))
}

fn push_write(
    writes: &mut [RegisterWrite; MAX_SETUP_WRITES],
    write_count: &mut usize,
    register: Register,
    value: u32,
) -> Result<(), Error> {
    let write = writes.get_mut(*write_count).ok_or(Error::TooManyWrites)?;
    *write = RegisterWrite { register, value };
    *write_count += 1;
    Ok(())
}

fn append_source_writes(
    writes: &mut [RegisterWrite; MAX_SETUP_WRITES],
    write_count: &mut usize,
    config: EngineConfig,
    snapshot: EngineSnapshot,
    allocation: RingAllocation,
) -> Result<(), Error> {
    let address = allocation.descriptor_iova;
    let high = ((address >> 32) as u32 & SOURCE_ADDRESS_MASK)
        | (snapshot.source_base_high & !SOURCE_ADDRESS_MASK);
    push_write(writes, write_count, Register::SourceBaseLow, address as u32)?;
    push_write(writes, write_count, Register::SourceBaseHigh, high)?;
    push_write(
        writes,
        write_count,
        Register::SourceSize,
        u32::from(config.source_entries),
    )?;
    let control1 = (snapshot.control1 & !CONTROL1_MAX_LENGTH_MASK)
        | (u32::from(config.source_max_bytes) & CONTROL1_MAX_LENGTH_MASK);
    push_write(writes, write_count, Register::Control1, control1)?;
    let source_lowmark = snapshot.source_watermark & !WATERMARK_LOW_MASK;
    push_write(
        writes,
        write_count,
        Register::SourceWatermark,
        source_lowmark,
    )?;
    let source_highmark = (source_lowmark & !WATERMARK_HIGH_MASK)
        | (u32::from(config.source_entries) & WATERMARK_HIGH_MASK);
    push_write(
        writes,
        write_count,
        Register::SourceWatermark,
        source_highmark,
    )?;
    Ok(())
}

fn append_destination_writes(
    writes: &mut [RegisterWrite; MAX_SETUP_WRITES],
    write_count: &mut usize,
    config: EngineConfig,
    snapshot: EngineSnapshot,
    allocation: RingAllocation,
) -> Result<(), Error> {
    let address = allocation.descriptor_iova;
    let high = ((address >> 32) as u32 & DESTINATION_ADDRESS_MASK)
        | (snapshot.destination_base_high & !DESTINATION_ADDRESS_MASK);
    push_write(
        writes,
        write_count,
        Register::DestinationBaseLow,
        address as u32,
    )?;
    push_write(writes, write_count, Register::DestinationBaseHigh, high)?;
    push_write(
        writes,
        write_count,
        Register::DestinationSize,
        u32::from(config.destination_entries),
    )?;
    let destination_lowmark = snapshot.destination_watermark & !WATERMARK_LOW_MASK;
    push_write(
        writes,
        write_count,
        Register::DestinationWatermark,
        destination_lowmark,
    )?;
    let destination_highmark = (destination_lowmark & !WATERMARK_HIGH_MASK)
        | (u32::from(config.destination_entries) & WATERMARK_HIGH_MASK);
    push_write(
        writes,
        write_count,
        Register::DestinationWatermark,
        destination_highmark,
    )?;
    Ok(())
}

/// Build one engine's initial register writes from an empty reset snapshot and
/// source-matched descriptor allocations. `evidence` must be re-derived under
/// a live platform lease by any eventual native caller.
fn plan_engine_setup(
    profile: Profile,
    engine: usize,
    source_ring: Option<RingAllocation>,
    destination_ring: Option<RingAllocation>,
    snapshot: EngineSnapshot,
    evidence: SetupEvidence,
) -> Result<EngineSetupPlan, Error> {
    if engine >= CE_COUNT {
        return Err(Error::InvalidEngine);
    }
    let (pool_start, pool_end) = validate_evidence(evidence)?;
    if snapshot.generation != evidence.generation {
        return Err(Error::StaleSnapshot);
    }
    if snapshot.source_size != 0
        || snapshot.destination_size != 0
        || snapshot.source_read_index != 0
        || snapshot.source_write_index != 0
        || snapshot.destination_read_index != 0
        || snapshot.destination_write_index != 0
    {
        return Err(Error::EngineNotEmpty);
    }
    let config = profile.config(engine).map_err(|_| Error::InvalidEngine)?;
    let source_ring = validate_ring(
        engine,
        RingDirection::Source,
        config.source_entries,
        source_ring,
        pool_start,
        pool_end,
    )?;
    let destination_ring = validate_ring(
        engine,
        RingDirection::Destination,
        config.destination_entries,
        destination_ring,
        pool_start,
        pool_end,
    )?;
    if let (Some(source), Some(destination)) = (source_ring, destination_ring) {
        let source_end = source
            .descriptor_iova
            .checked_add(u64::from(source.allocation_bytes))
            .ok_or(Error::AddressOverflow)?;
        let destination_end = destination
            .descriptor_iova
            .checked_add(u64::from(destination.allocation_bytes))
            .ok_or(Error::AddressOverflow)?;
        if source.descriptor_iova < destination_end && destination.descriptor_iova < source_end {
            return Err(Error::InvalidRingAllocation);
        }
    }

    let mut plan = empty_engine_plan(engine, snapshot);
    if let Some(source) = source_ring {
        append_source_writes(
            &mut plan.writes,
            &mut plan.write_count,
            config,
            snapshot,
            source,
        )?;
    }
    if let Some(destination) = destination_ring {
        append_destination_writes(
            &mut plan.writes,
            &mut plan.write_count,
            config,
            snapshot,
            destination,
        )?;
    }
    Ok(plan)
}

/// Build all twelve engine plans from a planner-produced opaque ring plan.
/// The complete descriptor set is checked for profile mismatches, duplicates,
/// out-of-pool addresses and overlap before any writes are returned.
///
/// This remains a plan only. A native owner must revalidate `evidence` under
/// its live lease immediately before any MMIO and must apply the writes only
/// while the corresponding CE engines are held quiescent.
pub fn plan_profile_setup(
    profile: Profile,
    rings: &RingPlan,
    snapshots: &[EngineSnapshot; CE_COUNT],
    evidence: SetupEvidence,
) -> Result<ProfileSetupPlan, Error> {
    let (pool_start, pool_end) = validate_evidence(evidence)?;
    let allocations = rings.allocations();
    if allocations.len() > MAX_RING_ALLOCATIONS {
        return Err(Error::UnexpectedRing);
    }

    for allocation in allocations {
        let engine = usize::from(allocation.engine);
        let config = profile.config(engine).map_err(|_| Error::UnexpectedRing)?;
        let expected_entries = match allocation.direction {
            RingDirection::Source => config.source_entries,
            RingDirection::Destination => config.destination_entries,
        };
        validate_ring(
            engine,
            allocation.direction,
            expected_entries,
            Some(*allocation),
            pool_start,
            pool_end,
        )?;
    }

    for first in 0..allocations.len() {
        let first_allocation = allocations[first];
        let first_end = first_allocation
            .descriptor_iova
            .checked_add(u64::from(first_allocation.allocation_bytes))
            .ok_or(Error::AddressOverflow)?;
        for second_allocation in allocations.iter().skip(first + 1) {
            let second_end = second_allocation
                .descriptor_iova
                .checked_add(u64::from(second_allocation.allocation_bytes))
                .ok_or(Error::AddressOverflow)?;
            if first_allocation.descriptor_iova < second_end
                && second_allocation.descriptor_iova < first_end
            {
                return Err(Error::RingOverlap);
            }
        }
    }

    let mut plans = core::array::from_fn(|engine| empty_engine_plan(engine, snapshots[engine]));
    for engine in 0..CE_COUNT {
        let mut source_ring = None;
        let mut destination_ring = None;
        for allocation in allocations
            .iter()
            .filter(|allocation| usize::from(allocation.engine) == engine)
        {
            let slot = match allocation.direction {
                RingDirection::Source => &mut source_ring,
                RingDirection::Destination => &mut destination_ring,
            };
            if slot.replace(*allocation).is_some() {
                return Err(Error::DuplicateRing);
            }
        }
        plans[engine] = plan_engine_setup(
            profile,
            engine,
            source_ring,
            destination_ring,
            snapshots[engine],
            evidence,
        )?;
    }

    Ok(ProfileSetupPlan {
        profile,
        generation: evidence.generation,
        iova_base: evidence.iova_base,
        iova_bytes: evidence.iova_bytes,
        engines: plans,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_ce_v3_profile::plan_rings;

    const READY: SetupEvidence = SetupEvidence {
        generation: 1,
        fresh_boot: true,
        exclusive_ce_owner: true,
        platform_ready: true,
        ce_powered: true,
        ce_reset_released: true,
        engine_quiescent: true,
        translated_dma_ready: true,
        dma_exclusive: true,
        dma_uncached: true,
        iova_base: 0x1_8000_0000,
        iova_bytes: 0x20000,
    };

    const EMPTY: EngineSnapshot = EngineSnapshot {
        generation: 1,
        source_size: 0,
        destination_size: 0,
        source_read_index: 0,
        source_write_index: 0,
        destination_read_index: 0,
        destination_write_index: 0,
        source_base_high: 0xa0,
        destination_base_high: 0x60,
        control1: 0xa5a5_ffff,
        source_watermark: 0x1234_5678,
        destination_watermark: 0x9abc_def0,
    };

    fn allocations(engine: usize) -> (Option<RingAllocation>, Option<RingAllocation>) {
        let ring_plan = plan_rings(Profile::Mission, READY.iova_base, READY.iova_bytes).unwrap();
        let source = ring_plan.allocations().iter().copied().find(|ring| {
            usize::from(ring.engine) == engine && ring.direction == RingDirection::Source
        });
        let destination = ring_plan.allocations().iter().copied().find(|ring| {
            usize::from(ring.engine) == engine && ring.direction == RingDirection::Destination
        });
        (source, destination)
    }

    #[test]
    fn ce0_source_setup_matches_qcacld_order_and_read_modify_write_values() {
        let (source, destination) = allocations(0);
        let plan =
            plan_engine_setup(Profile::Mission, 0, source, destination, EMPTY, READY).unwrap();
        assert_eq!(plan.engine, 0);
        assert_eq!(plan.writes().len(), 6);
        assert_eq!(plan.writes()[0].register, Register::SourceBaseLow);
        assert_eq!(plan.writes()[0].value, 0x8000_0000);
        assert_eq!(plan.writes()[1].register, Register::SourceBaseHigh);
        assert_eq!(plan.writes()[1].value, 0xa1);
        assert_eq!(plan.writes()[2].register, Register::SourceSize);
        assert_eq!(plan.writes()[2].value, 16);
        assert_eq!(plan.writes()[3].register, Register::Control1);
        assert_eq!(plan.writes()[3].value, 0xa5a5_0800);
        assert_eq!(plan.writes()[4].register, Register::SourceWatermark);
        assert_eq!(plan.writes()[4].value, 0x0000_5678);
        assert_eq!(plan.writes()[5].register, Register::SourceWatermark);
        assert_eq!(plan.writes()[5].value, 16);
    }

    #[test]
    fn ce1_destination_setup_preserves_non_address_high_bits_and_sets_watermarks() {
        let (source, destination) = allocations(1);
        let mut snapshot = EMPTY;
        snapshot.destination_base_high = 0x63;
        let plan =
            plan_engine_setup(Profile::Mission, 1, source, destination, snapshot, READY).unwrap();
        assert_eq!(plan.writes().len(), 5);
        assert_eq!(plan.writes()[0].register, Register::DestinationBaseLow);
        assert_eq!(plan.writes()[1].register, Register::DestinationBaseHigh);
        assert_eq!(plan.writes()[1].value, 0x61);
        assert_eq!(plan.writes()[2].register, Register::DestinationSize);
        assert_eq!(plan.writes()[2].value, 512);
        assert_eq!(plan.writes()[3].register, Register::DestinationWatermark);
        assert_eq!(plan.writes()[3].value, 0x0000_def0);
        assert_eq!(plan.writes()[4].register, Register::DestinationWatermark);
        assert_eq!(plan.writes()[4].value, 512);
    }

    #[test]
    fn setup_refuses_missing_rings_stale_indices_and_unready_evidence() {
        let (source, destination) = allocations(0);
        assert_eq!(
            plan_engine_setup(Profile::Mission, 0, None, destination, EMPTY, READY),
            Err(Error::MissingRing)
        );
        let mut stale = EMPTY;
        stale.source_write_index = 1;
        assert_eq!(
            plan_engine_setup(Profile::Mission, 0, source, destination, stale, READY),
            Err(Error::EngineNotEmpty)
        );
        let mut stale_snapshot = EMPTY;
        stale_snapshot.generation += 1;
        assert_eq!(
            plan_engine_setup(
                Profile::Mission,
                0,
                source,
                destination,
                stale_snapshot,
                READY
            ),
            Err(Error::StaleSnapshot)
        );
        let mut unready = READY;
        unready.translated_dma_ready = false;
        assert_eq!(
            plan_engine_setup(Profile::Mission, 0, source, destination, EMPTY, unready),
            Err(Error::DmaNotReady)
        );
        let mut stale_lease = READY;
        stale_lease.generation = 0;
        assert_eq!(
            plan_engine_setup(Profile::Mission, 0, source, destination, EMPTY, stale_lease),
            Err(Error::LifecycleNotReady)
        );
    }

    #[test]
    fn setup_rejects_forged_or_out_of_pool_ring_allocations() {
        let (source, destination) = allocations(0);
        let mut forged = source.unwrap();
        forged.engine = 1;
        assert_eq!(
            plan_engine_setup(Profile::Mission, 0, Some(forged), destination, EMPTY, READY),
            Err(Error::InvalidRingAllocation)
        );
        let mut outside = source.unwrap();
        outside.descriptor_iova = READY.iova_base + READY.iova_bytes;
        assert_eq!(
            plan_engine_setup(
                Profile::Mission,
                0,
                Some(outside),
                destination,
                EMPTY,
                READY
            ),
            Err(Error::InvalidRingAllocation)
        );
    }

    #[derive(Default)]
    struct MockWriter {
        ready: bool,
        attempts: usize,
        fail_on_attempt: Option<usize>,
        initial_snapshots: usize,
        continuation_writes: usize,
        writes: std::vec::Vec<(u8, Register, u32)>,
    }

    impl SetupWriter for MockWriter {
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
            let attempt = self.attempts;
            self.attempts += 1;
            if !self.ready
                || generation != READY.generation
                || iova_base != READY.iova_base
                || iova_bytes != READY.iova_bytes
                || usize::from(engine) >= CE_COUNT
            {
                return Err(SetupWriterError::LeaseLost);
            }
            match expected_snapshot {
                Some(snapshot) if snapshot.generation == generation => self.initial_snapshots += 1,
                Some(_) => return Err(SetupWriterError::LeaseLost),
                None => self.continuation_writes += 1,
            }
            if self.fail_on_attempt == Some(attempt) {
                return Err(SetupWriterError::Io);
            }
            self.writes.push((engine, register, value));
            Ok(())
        }
    }

    struct PanicWriter {
        attempts: usize,
    }

    impl SetupWriter for PanicWriter {
        fn write(
            &mut self,
            _generation: u64,
            _iova_base: u64,
            _iova_bytes: u64,
            _engine: u8,
            _expected_snapshot: Option<EngineSnapshot>,
            _register: Register,
            _value: u32,
        ) -> Result<(), SetupWriterError> {
            self.attempts += 1;
            panic!("simulated uncertain register write")
        }
    }

    fn complete_profile_plan() -> ProfileSetupPlan {
        let rings = plan_rings(Profile::Mission, READY.iova_base, READY.iova_bytes).unwrap();
        plan_profile_setup(Profile::Mission, &rings, &[EMPTY; CE_COUNT], READY).unwrap()
    }

    #[test]
    fn setup_apply_serializes_each_write_under_the_live_generation() {
        let mut apply = SetupApply::new(complete_profile_plan());
        let mut writer = MockWriter {
            ready: true,
            ..MockWriter::default()
        };
        loop {
            if apply.step(&mut writer).unwrap() == ApplyProgress::Complete {
                break;
            }
        }
        assert_eq!(writer.writes.len(), 49);
        assert_eq!(writer.attempts, writer.writes.len());
        assert_eq!(writer.initial_snapshots, 9);
        assert_eq!(writer.continuation_writes, 40);
        assert_eq!(apply.step(&mut writer), Ok(ApplyProgress::Complete));
        assert_eq!(writer.attempts, 49);
    }

    #[test]
    fn setup_apply_stops_permanently_after_live_lease_loss() {
        let mut apply = SetupApply::new(complete_profile_plan());
        let mut writer = MockWriter {
            ready: true,
            ..MockWriter::default()
        };
        assert!(matches!(
            apply.step(&mut writer),
            Ok(ApplyProgress::Wrote { .. })
        ));
        writer.ready = false;
        assert_eq!(apply.step(&mut writer), Err(ApplyError::LeaseLost));
        assert_eq!(apply.step(&mut writer), Err(ApplyError::Terminal));
        assert_eq!(writer.attempts, 2);
        assert_eq!(writer.writes.len(), 1);
    }

    #[test]
    fn setup_apply_does_not_retry_a_failed_or_uncertain_register_write() {
        let mut apply = SetupApply::new(complete_profile_plan());
        let mut writer = MockWriter {
            ready: true,
            ..MockWriter::default()
        };
        writer.fail_on_attempt = Some(1);
        assert!(matches!(
            apply.step(&mut writer),
            Ok(ApplyProgress::Wrote { .. })
        ));
        assert_eq!(apply.step(&mut writer), Err(ApplyError::WriteFailed));
        assert_eq!(apply.step(&mut writer), Err(ApplyError::Terminal));
        assert_eq!(writer.attempts, 2);
        assert_eq!(writer.writes.len(), 1);
    }

    #[test]
    fn setup_apply_panics_are_quarantined_before_any_retry() {
        let mut apply = SetupApply::new(complete_profile_plan());
        let mut writer = PanicWriter { attempts: 0 };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = apply.step(&mut writer);
        }));
        assert!(result.is_err());
        assert_eq!(apply.step(&mut writer), Err(ApplyError::Terminal));
        assert_eq!(writer.attempts, 1);
    }
}
