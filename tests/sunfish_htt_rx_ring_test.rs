#![allow(dead_code)]
#![forbid(unsafe_code)]

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LpaePageMapping {
        pub iova_page: u64,
        pub physical_page: u64,
    }
}

#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;

use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_htt_rx_ring::{
    SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS, SUNFISH_HTT_RX_RING_CFG_BYTES,
    SUNFISH_HTT_RX_RING_ENTRY_BYTES, SunfishHttRxBufferPool, SunfishHttRxRefillError,
    SunfishHttRxRingError, SunfishHttRxRingIo, SunfishHttRxRingMemory, SunfishHttRxRingOptions,
    SunfishHttRxRingRefillState, plan_sunfish_htt_rx_ring,
};
use sunfish_qcacld_rx_session::SunfishQcacldRxSession;
use sunfish_smmu_lpae::{LpaePageMapping, SMMU_LPAE_PAGE_BYTES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IoEvent {
    ReadIndex(u64),
    SyncBuffer(u64, usize),
    WriteRingEntry(u64, u64),
    Barrier,
    WriteIndex(u64, u32),
}

#[derive(Default)]
struct MockRingIo {
    alloc_index: u32,
    events: Vec<IoEvent>,
    fail_sync_buffer: Option<u64>,
    fail_ring_write: bool,
    fail_index_write: bool,
}

impl SunfishHttRxRingIo for MockRingIo {
    type Error = &'static str;

    fn read_alloc_index(&mut self, alloc_index_iova: u64) -> Result<u32, Self::Error> {
        self.events.push(IoEvent::ReadIndex(alloc_index_iova));
        Ok(self.alloc_index)
    }

    fn sync_buffer_for_device(
        &mut self,
        buffer_iova: u64,
        buffer_bytes: usize,
    ) -> Result<(), Self::Error> {
        self.events
            .push(IoEvent::SyncBuffer(buffer_iova, buffer_bytes));
        if self.fail_sync_buffer == Some(buffer_iova) {
            Err("buffer sync")
        } else {
            Ok(())
        }
    }

    fn write_ring_entry(
        &mut self,
        ring_entry_iova: u64,
        buffer_iova: u64,
    ) -> Result<(), Self::Error> {
        self.events
            .push(IoEvent::WriteRingEntry(ring_entry_iova, buffer_iova));
        if self.fail_ring_write {
            Err("ring write")
        } else {
            Ok(())
        }
    }

    fn dma_write_barrier(&mut self) {
        self.events.push(IoEvent::Barrier);
    }

    fn write_alloc_index(
        &mut self,
        alloc_index_iova: u64,
        next_index: u32,
    ) -> Result<(), Self::Error> {
        self.events
            .push(IoEvent::WriteIndex(alloc_index_iova, next_index));
        if self.fail_index_write {
            Err("index write")
        } else {
            self.alloc_index = next_index;
            Ok(())
        }
    }
}

fn full_initial_session() -> SunfishQcacldRxSession<2048> {
    let mut session = SunfishQcacldRxSession::new().unwrap();
    for buffer_id in 0..2047 {
        assert_eq!(session.post(buffer_id).unwrap(), buffer_id as u16);
    }
    session
}

fn buffer_backing(plan: sunfish_htt_rx_ring::SunfishHttRxRingPlan, fill: u8) -> (Vec<u8>, Vec<u8>) {
    (
        vec![fill; plan.initial_buffer_bytes().unwrap() as usize],
        vec![fill; plan.fallback_buffer_bytes() as usize],
    )
}

fn plan() -> sunfish_htt_rx_ring::SunfishHttRxRingPlan {
    plan_sunfish_htt_rx_ring(
        plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap(),
        SunfishHttRxRingMemory {
            ring_iova: 0x8020_0000,
            alloc_index_iova: 0x8020_4000,
            initial_buffers_iova: 0x8020_4008,
            fallback_buffers_iova: 0x8064_3788,
        },
        SunfishHttRxRingOptions {
            packet_log_enabled: false,
            global_monitor_mode: false,
        },
    )
    .unwrap()
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect()
}

#[test]
fn source_matched_ring_config_encodes_one_64_bit_low_latency_ring() {
    let plan = plan();
    assert_eq!(plan.ring_slots(), 2048);
    assert_eq!(plan.initial_fill_level(), 2047);
    assert_eq!(plan.buffer_bytes(), 2176);
    assert_eq!(plan.initial_alloc_index(), 2047);
    assert_eq!(plan.initial_buffer_bytes(), Some(4_454_272));

    let mut encoded = [0xaa; SUNFISH_HTT_RX_RING_CFG_BYTES];
    assert_eq!(plan.encode_ring_config(&mut encoded), Ok(48));
    assert_eq!(
        words(&encoded),
        [
            0x0000_0102,
            0x8020_4000,
            0,
            0x8020_0000,
            0,
            0x0880_0800,
            0x07ff_0ff2,
            0x008a_004a,
            0x0025_001b,
            0x001a_0004,
            0x000c_0007,
            0x0002_0001,
        ]
    );
}

#[test]
fn initializes_2047_buffer_addresses_and_leaves_the_one_empty_slot() {
    let plan = plan();
    let ring_bytes =
        usize::try_from(u64::from(plan.ring_slots()) * SUNFISH_HTT_RX_RING_ENTRY_BYTES).unwrap();
    let mut ring = vec![0xaa; ring_bytes + 8];
    assert_eq!(plan.write_initial_address_ring(&mut ring), Ok(ring_bytes));

    assert_eq!(
        u64::from_le_bytes(ring[0..8].try_into().unwrap()),
        0x8020_4008
    );
    assert_eq!(
        u64::from_le_bytes(ring[8..16].try_into().unwrap()),
        0x8020_4888
    );
    let last_posted =
        usize::try_from(u64::from(plan.initial_fill_level() - 1) * SUNFISH_HTT_RX_RING_ENTRY_BYTES)
            .unwrap();
    let last_address =
        0x8020_4008u64 + u64::from(plan.initial_fill_level() - 1) * u64::from(plan.buffer_bytes());
    assert_eq!(
        u64::from_le_bytes(ring[last_posted..last_posted + 8].try_into().unwrap()),
        last_address
    );
    let empty_slot = ring_bytes - 8;
    assert_eq!(&ring[empty_slot..ring_bytes], &[0; 8]);
    assert_eq!(&ring[ring_bytes..], &[0xaa; 8]);

    let mut index = [0xaa; 4];
    assert_eq!(plan.write_initial_alloc_index(&mut index), Ok(4));
    assert_eq!(u32::from_le_bytes(index), 2047);
}

#[test]
fn initial_and_fallback_buffer_ids_resolve_to_distinct_fixed_iova_windows() {
    let plan = plan();
    let last_initial = plan.initial_fill_level() - 1;
    let last_fallback = plan.fallback_buffer_count() - 1;

    assert_eq!(plan.initial_buffer_iova(0), Ok(0x8020_4008));
    assert_eq!(
        plan.initial_buffer_iova(last_initial),
        Ok(0x8020_4008 + u64::from(last_initial) * u64::from(plan.buffer_bytes()))
    );
    assert_eq!(plan.fallback_buffer_iova(0), Ok(0x8064_3788));
    assert_eq!(
        plan.fallback_buffer_iova(last_fallback),
        Ok(0x8064_3788 + u64::from(last_fallback) * u64::from(plan.buffer_bytes()))
    );
    assert_eq!(
        plan.initial_buffer_iova(plan.initial_fill_level()),
        Err(SunfishHttRxRingError::InvalidBufferIndex)
    );
    assert_eq!(
        plan.fallback_buffer_iova(plan.fallback_buffer_count()),
        Err(SunfishHttRxRingError::InvalidBufferIndex)
    );
}

#[test]
fn capture_modes_enable_all_optional_header_and_ppdu_fields() {
    for options in [
        SunfishHttRxRingOptions {
            packet_log_enabled: true,
            global_monitor_mode: false,
        },
        SunfishHttRxRingOptions {
            packet_log_enabled: false,
            global_monitor_mode: true,
        },
    ] {
        let capture_plan = plan_sunfish_htt_rx_ring(
            plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap(),
            plan().memory(),
            options,
        )
        .unwrap();
        let mut encoded = [0; SUNFISH_HTT_RX_RING_CFG_BYTES];
        capture_plan.encode_ring_config(&mut encoded).unwrap();
        assert_eq!(words(&encoded)[6], 0x07ff_ffff);
    }
}

#[test]
fn descriptor_offsets_match_the_compiled_sunfish_rx_layout() {
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.hdr_status_words, 74);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.ppdu_start_words, 27);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.ppdu_end_words, 37);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.mpdu_start_words, 4);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.mpdu_end_words, 26);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words, 7);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words, 12);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words, 1);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.fragment_info_words, 2);
    assert_eq!(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_payload_words, 138);
}

#[test]
fn rx_buffer_preparation_clears_only_the_source_attention_word() {
    let plan = plan();
    let mut initial = vec![0xa5; plan.buffer_bytes() as usize + 8];
    assert_eq!(plan.prepare_buffer_for_device(0, &mut initial), Ok(()));
    assert_eq!(&initial[..4], &[0xa5; 4]);
    assert_eq!(&initial[4..8], &[0; 4]);
    assert!(initial[8..].iter().all(|byte| *byte == 0xa5));

    let fallback_id = plan.initial_fill_level();
    let mut fallback = vec![0x5a; plan.buffer_bytes() as usize];
    assert_eq!(
        plan.prepare_buffer_for_device(fallback_id, &mut fallback),
        Ok(())
    );
    assert_eq!(&fallback[4..8], &[0; 4]);
    assert!(fallback[..4].iter().all(|byte| *byte == 0x5a));
    assert!(fallback[8..].iter().all(|byte| *byte == 0x5a));

    let mut invalid = vec![0xcc; plan.buffer_bytes() as usize];
    assert_eq!(
        plan.prepare_buffer_for_device(
            plan.initial_fill_level() + plan.fallback_buffer_count(),
            &mut invalid
        ),
        Err(SunfishHttRxRingError::InvalidBufferIndex)
    );
    assert!(invalid.iter().all(|byte| *byte == 0xcc));
}

#[test]
fn initial_rx_buffer_preparation_rejects_a_short_set_before_mutation() {
    let plan = plan();
    let mut first = vec![0xa5; plan.buffer_bytes() as usize];
    let result = plan.prepare_initial_buffers(&mut [first.as_mut_slice()]);
    assert_eq!(result, Err(SunfishHttRxRingError::OutputTooSmall));
    assert!(first.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn fixed_buffer_pool_binds_startup_and_fallback_ids_to_their_planned_strides() {
    let plan = plan();
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();
    let fallback_id = plan.initial_fill_level();

    assert_eq!(buffers.plan(), plan);
    assert_eq!(buffers.buffer(0).unwrap().len(), 2176);
    assert_eq!(buffers.buffer(fallback_id).unwrap().len(), 2176);
    buffers.buffer_mut(0).unwrap()[0] = 0x31;
    buffers.buffer_mut(1).unwrap()[0] = 0x32;
    buffers.buffer_mut(fallback_id).unwrap()[0] = 0x64;
    assert_eq!(buffers.buffer(0).unwrap()[0], 0x31);
    assert_eq!(buffers.buffer(1).unwrap()[0], 0x32);
    assert_eq!(buffers.buffer(fallback_id).unwrap()[0], 0x64);
    assert_eq!(
        buffers.buffer(fallback_id + plan.fallback_buffer_count()),
        Err(SunfishHttRxRingError::InvalidBufferIndex)
    );

    assert_eq!(
        buffers.prepare_initial_buffers(),
        Ok(plan.initial_fill_level() as usize)
    );
    for buffer_id in 0..plan.initial_fill_level() {
        assert_eq!(&buffers.buffer(buffer_id).unwrap()[4..8], &[0; 4]);
    }
    assert_eq!(&buffers.buffer(fallback_id).unwrap()[4..8], &[0xa5; 4]);
}

#[test]
fn fixed_buffer_pool_rejects_short_backing_before_exposing_any_pool() {
    let plan = plan();
    let mut initial = vec![0xa5; plan.initial_buffer_bytes().unwrap() as usize - 1];
    let mut fallback = vec![0x5a; plan.fallback_buffer_bytes() as usize];

    assert!(matches!(
        SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback),
        Err(SunfishHttRxRingError::OutputTooSmall)
    ));
    assert!(initial.iter().all(|byte| *byte == 0xa5));
    assert!(fallback.iter().all(|byte| *byte == 0x5a));
}

#[test]
fn live_refill_clears_syncs_posts_then_publishes_and_wraps_index() {
    let plan = plan();
    let mut session = full_initial_session();
    let completed = session.take_next().unwrap();
    session.release(completed).unwrap();
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 2047,
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();

    assert_eq!(
        state.refill_one(plan, &mut session, 0, &mut buffers, &mut io),
        Ok(2047)
    );

    assert_eq!(state.next_alloc_index(), 0);
    assert!(!state.is_poisoned());
    assert_eq!(session.posted_count(), 2047);
    assert_eq!(session.occupied_count(), 2047);
    assert_eq!(&buffers.buffer(0).unwrap()[4..8], &[0; 4]);
    assert_eq!(
        io.events,
        [
            IoEvent::ReadIndex(0x8020_4000),
            IoEvent::SyncBuffer(0x8020_4008, 2176),
            IoEvent::WriteRingEntry(0x8020_3ff8, 0x8020_4008),
            IoEvent::Barrier,
            IoEvent::WriteIndex(0x8020_4000, 0),
        ]
    );
}

#[test]
fn batched_refill_wraps_ring_and_publishes_one_final_index() {
    let plan = plan();
    let mut session = full_initial_session();
    for _ in 0..2 {
        let completed = session.take_next().unwrap();
        session.release(completed).unwrap();
    }
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 2047,
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();
    buffers.buffer_mut(1).unwrap().fill(0x5a);

    assert_eq!(
        state.refill_batch(plan, &mut session, &[0, 1], &mut buffers, &mut io),
        Ok(2047)
    );

    assert_eq!(state.next_alloc_index(), 1);
    assert_eq!(session.posted_count(), 2047);
    assert_eq!(session.occupied_count(), 2047);
    assert_eq!(&buffers.buffer(0).unwrap()[4..8], &[0; 4]);
    assert_eq!(&buffers.buffer(1).unwrap()[4..8], &[0; 4]);
    assert_eq!(
        io.events,
        [
            IoEvent::ReadIndex(0x8020_4000),
            IoEvent::SyncBuffer(0x8020_4008, 2176),
            IoEvent::WriteRingEntry(0x8020_3ff8, 0x8020_4008),
            IoEvent::SyncBuffer(0x8020_4888, 2176),
            IoEvent::WriteRingEntry(0x8020_0000, 0x8020_4888),
            IoEvent::Barrier,
            IoEvent::WriteIndex(0x8020_4000, 1),
        ]
    );
}

#[test]
fn batch_failure_before_index_publication_keeps_session_and_index_unchanged() {
    let plan = plan();
    let mut session = full_initial_session();
    for _ in 0..2 {
        let completed = session.take_next().unwrap();
        session.release(completed).unwrap();
    }
    let before_session = session;
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 2047,
        fail_sync_buffer: Some(0x8020_4888),
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();
    buffers.buffer_mut(1).unwrap().fill(0x5a);

    assert_eq!(
        state.refill_batch(plan, &mut session, &[0, 1], &mut buffers, &mut io),
        Err(SunfishHttRxRefillError::Io("buffer sync"))
    );
    assert_eq!(session, before_session);
    assert_eq!(state.next_alloc_index(), 2047);
    assert!(!state.is_poisoned());
    assert_eq!(&buffers.buffer(0).unwrap()[4..8], &[0; 4]);
    assert_eq!(&buffers.buffer(1).unwrap()[4..8], &[0; 4]);
    assert_eq!(
        io.events,
        [
            IoEvent::ReadIndex(0x8020_4000),
            IoEvent::SyncBuffer(0x8020_4008, 2176),
            IoEvent::WriteRingEntry(0x8020_3ff8, 0x8020_4008),
            IoEvent::SyncBuffer(0x8020_4888, 2176),
        ]
    );
}

#[test]
fn refill_rejects_unexpected_alloc_index_without_mutating_buffer_or_session() {
    let plan = plan();
    let mut session = full_initial_session();
    let completed = session.take_next().unwrap();
    session.release(completed).unwrap();
    let before_session = session;
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 12,
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();

    assert_eq!(
        state.refill_one(plan, &mut session, 0, &mut buffers, &mut io),
        Err(SunfishHttRxRefillError::Ring(
            SunfishHttRxRingError::UnexpectedAllocIndex {
                expected: 2047,
                observed: 12,
            }
        ))
    );
    assert_eq!(session, before_session);
    assert!(buffers.buffer(0).unwrap().iter().all(|byte| *byte == 0xa5));
    assert_eq!(io.events, [IoEvent::ReadIndex(0x8020_4000)]);
    assert!(!state.is_poisoned());
}

#[test]
fn refill_state_cannot_be_reused_with_a_different_ring_plan() {
    let plan = plan();
    let changed_plan = plan_sunfish_htt_rx_ring(
        plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap(),
        SunfishHttRxRingMemory {
            ring_iova: 0x8120_0000,
            alloc_index_iova: 0x8120_4000,
            initial_buffers_iova: 0x8120_4008,
            fallback_buffers_iova: 0x8164_3788,
        },
        SunfishHttRxRingOptions {
            packet_log_enabled: false,
            global_monitor_mode: false,
        },
    )
    .unwrap();
    let mut session = full_initial_session();
    let completed = session.take_next().unwrap();
    session.release(completed).unwrap();
    let before_session = session;
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 2047,
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();

    assert_eq!(
        state.refill_one(changed_plan, &mut session, 0, &mut buffers, &mut io),
        Err(SunfishHttRxRefillError::Ring(
            SunfishHttRxRingError::RefillPlanChanged
        ))
    );
    assert_eq!(session, before_session);
    assert!(io.events.is_empty());
    assert!(buffers.buffer(0).unwrap().iter().all(|byte| *byte == 0xa5));
}

#[test]
fn failed_ring_entry_write_does_not_publish_or_commit_ownership() {
    let plan = plan();
    let mut session = full_initial_session();
    let completed = session.take_next().unwrap();
    session.release(completed).unwrap();
    let before_session = session;
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 2047,
        fail_ring_write: true,
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();

    assert_eq!(
        state.refill_one(plan, &mut session, 0, &mut buffers, &mut io),
        Err(SunfishHttRxRefillError::Io("ring write"))
    );
    assert_eq!(session, before_session);
    assert_eq!(state.next_alloc_index(), 2047);
    assert!(!state.is_poisoned());
    assert_eq!(
        io.events,
        [
            IoEvent::ReadIndex(0x8020_4000),
            IoEvent::SyncBuffer(0x8020_4008, 2176),
            IoEvent::WriteRingEntry(0x8020_3ff8, 0x8020_4008),
        ]
    );
}

#[test]
fn uncertain_alloc_index_write_poison_refill_state_without_software_commit() {
    let plan = plan();
    let mut session = full_initial_session();
    let completed = session.take_next().unwrap();
    session.release(completed).unwrap();
    let before_session = session;
    let mut state = SunfishHttRxRingRefillState::new(plan);
    let mut io = MockRingIo {
        alloc_index: 2047,
        fail_index_write: true,
        ..MockRingIo::default()
    };
    let (mut initial, mut fallback) = buffer_backing(plan, 0xa5);
    let mut buffers = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();

    assert_eq!(
        state.refill_one(plan, &mut session, 0, &mut buffers, &mut io),
        Err(SunfishHttRxRefillError::AllocIndexWriteUncertain(
            "index write"
        ))
    );
    assert_eq!(session, before_session);
    assert_eq!(state.next_alloc_index(), 2047);
    assert!(state.is_poisoned());
    let event_count = io.events.len();
    assert_eq!(
        state.refill_one(plan, &mut session, 0, &mut buffers, &mut io),
        Err(SunfishHttRxRefillError::Ring(
            SunfishHttRxRingError::RefillPoisoned
        ))
    );
    assert_eq!(io.events.len(), event_count);
}

#[test]
fn dma_mapping_must_cover_ring_index_startup_and_fallback_ranges() {
    let plan = plan();
    let page_count = usize::try_from(
        plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG)
            .unwrap()
            .minimum_reserved_page_count()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(page_count, 1126);
    let mappings: Vec<_> = (0..page_count)
        .map(|index| LpaePageMapping {
            iova_page: 0x8020_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
            physical_page: 0x8400_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
        })
        .collect();
    assert_eq!(plan.validate_mappings(&mappings), Ok(()));

    let mut missing_page = mappings.clone();
    missing_page.remove(900);
    assert_eq!(
        plan.validate_mappings(&missing_page),
        Err(SunfishHttRxRingError::UnmappedDmaPage(
            0x8020_0000 + 900 * SMMU_LPAE_PAGE_BYTES
        ))
    );

    let mut missing_fallback_page = mappings.clone();
    missing_fallback_page.pop();
    assert_eq!(
        plan.validate_mappings(&missing_fallback_page),
        Err(SunfishHttRxRingError::UnmappedDmaPage(
            0x8020_0000 + (page_count as u64 - 1) * SMMU_LPAE_PAGE_BYTES
        ))
    );

    let mut unsorted = mappings;
    unsorted.swap(0, 1);
    assert_eq!(
        plan.validate_mappings(&unsorted),
        Err(SunfishHttRxRingError::MappingsNotSorted)
    );
}

#[test]
fn short_output_buffers_are_rejected_without_partial_writes() {
    let plan = plan();
    let mut encoded = [0xa5; SUNFISH_HTT_RX_RING_CFG_BYTES - 1];
    assert_eq!(
        plan.encode_ring_config(&mut encoded),
        Err(SunfishHttRxRingError::OutputTooSmall)
    );
    assert_eq!(encoded, [0xa5; SUNFISH_HTT_RX_RING_CFG_BYTES - 1]);

    let mut ring = [0xa5; 16_384 - 1];
    assert_eq!(
        plan.write_initial_address_ring(&mut ring),
        Err(SunfishHttRxRingError::OutputTooSmall)
    );
    assert_eq!(ring, [0xa5; 16_384 - 1]);

    let mut index = [0xa5; 3];
    assert_eq!(
        plan.write_initial_alloc_index(&mut index),
        Err(SunfishHttRxRingError::OutputTooSmall)
    );
    assert_eq!(index, [0xa5; 3]);
}

#[test]
fn rejects_invalid_profile_address_width_alignment_overlap_and_root_crossing() {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let options = SunfishHttRxRingOptions {
        packet_log_enabled: false,
        global_monitor_mode: false,
    };
    let memory = plan().memory();

    let full_reorder_profile = plan_htt_rx_profile(sunfish_htt_rx_profile::HttRxConfig {
        has_target_index: true,
        ..SUNFISH_QCACLD_CONFIG
    })
    .unwrap();
    assert_eq!(
        plan_sunfish_htt_rx_ring(full_reorder_profile, memory, options),
        Err(SunfishHttRxRingError::InvalidProfile)
    );

    assert_eq!(
        plan_sunfish_htt_rx_ring(
            profile,
            SunfishHttRxRingMemory {
                ring_iova: memory.ring_iova,
                alloc_index_iova: memory.ring_iova + 8,
                initial_buffers_iova: memory.initial_buffers_iova,
                fallback_buffers_iova: memory.fallback_buffers_iova,
            },
            options,
        ),
        Err(SunfishHttRxRingError::MemoryOverlap)
    );
    assert_eq!(
        plan_sunfish_htt_rx_ring(
            profile,
            SunfishHttRxRingMemory {
                ring_iova: memory.ring_iova + 1,
                ..memory
            },
            options,
        ),
        Err(SunfishHttRxRingError::MisalignedRing)
    );
    assert_eq!(
        plan_sunfish_htt_rx_ring(
            profile,
            SunfishHttRxRingMemory {
                initial_buffers_iova: memory.initial_buffers_iova + 1,
                ..memory
            },
            options,
        ),
        Err(SunfishHttRxRingError::MisalignedBufferPool)
    );
    assert_eq!(
        plan_sunfish_htt_rx_ring(
            profile,
            SunfishHttRxRingMemory {
                ring_iova: 1u64 << 37,
                ..memory
            },
            options,
        ),
        Err(SunfishHttRxRingError::AddressOutOfRange)
    );
    assert_eq!(
        plan_sunfish_htt_rx_ring(
            profile,
            SunfishHttRxRingMemory {
                ring_iova: 0xbfff_8000,
                alloc_index_iova: 0xbfff_c000,
                initial_buffers_iova: 0xc000_0008,
                fallback_buffers_iova: 0xc043_f788,
            },
            options,
        ),
        Err(SunfishHttRxRingError::IovaRootMismatch)
    );
}
