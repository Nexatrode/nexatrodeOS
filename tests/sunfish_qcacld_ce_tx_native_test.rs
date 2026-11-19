#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/qcacld_wmi.rs"]
mod qcacld_wmi;
#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_init.rs"]
mod qcom_ce_v3_init;
#[path = "../rust/src/qcom_ce_v3_native_io.rs"]
mod qcom_ce_v3_native_io;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/qcom_ufs_platform.rs"]
mod qcom_ufs_platform;
#[path = "../rust/src/qcom_wmi_ce.rs"]
mod qcom_wmi_ce;
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_qcacld_ce_tx.rs"]
mod sunfish_qcacld_ce_tx;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_mgmt_tx.rs"]
mod sunfish_qcacld_mgmt_tx;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_qcacld_tx.rs"]
mod sunfish_qcacld_tx;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;

use core::cell::Cell;
use core::ptr::NonNull;
use qcom_ce_v3::RingCursor;
use qcom_ce_v3_native_io::{
    CE_MMIO_BYTES, CE_MMIO_PHYSICAL_BASE, CeMmioMapping, Gate, Guard, HttRxRefillError, NativeIo,
    Observation, RxPostError, TxPublishError, WLAN_SIDS,
};
use qcom_ce_v3_profile::{Profile, RingPlan, plan_rings};
use qcom_ce_v3_regs::{self as regs, Register};
use qcom_wmi_ce::{
    Direction, DmaPool, HTT_DATA_DESTINATION_CE, HTT_DATA_SOURCE_CE, RxBufferLedger,
    RxBufferWindow, ServiceCeMapping, TxBufferWindow, WmiRoute, plan_service_rx_post,
};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::rc::Rc;
use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_htt_rx_ring::{
    SunfishHttRxBufferPool, SunfishHttRxRingMemory, SunfishHttRxRingOptions,
    SunfishHttRxRingRefillState, plan_sunfish_htt_rx_ring,
};
use sunfish_qcacld_ce_tx::{CeTxBuffers, CeTxPlan, plan_native_wifi_tx};
use sunfish_qcacld_htc::{HTT_DATA_SERVICE_ID, parse_connect_response};
use sunfish_qcacld_rx_session::SunfishQcacldRxSession;
use sunfish_qcacld_tx::{HELIUMPLUS_FRAGMENT_DESC_BYTES, TxMetadata};

const GENERATION: u64 = 17;
const DMA_IOVA: u64 = 0x8000_0000;
const DMA_BYTES: usize = 0x0004_0000;
const RX_DMA_BYTES: usize = 0x0050_0000;

struct TestMemory {
    base: NonNull<u8>,
    layout: Layout,
}

impl TestMemory {
    fn new(bytes: usize) -> Self {
        let layout = Layout::from_size_align(bytes, 4096).unwrap();
        // SAFETY: the same layout is retained and used by Drop.
        let base = unsafe { alloc_zeroed(layout) };
        Self {
            base: NonNull::new(base).expect("test allocation failed"),
            layout,
        }
    }

    fn mapping(&mut self) -> CeMmioMapping {
        // SAFETY: the allocation is large enough for the CE register offset
        // used by this test's backing and mapped window.
        let base = unsafe { self.base.as_ptr().add(regs::CE0_OFFSET) };
        CeMmioMapping {
            base,
            virtual_base: base as usize as u64,
            physical_base: CE_MMIO_PHYSICAL_BASE,
            bytes: CE_MMIO_BYTES,
        }
    }

    fn read_u32(&self, offset: usize) -> u32 {
        // SAFETY: offsets are aligned register words inside this allocation.
        unsafe { self.base.as_ptr().add(offset).cast::<u32>().read_volatile() }
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        // SAFETY: offsets are aligned register words inside this allocation.
        unsafe {
            self.base
                .as_ptr()
                .add(offset)
                .cast::<u32>()
                .write_volatile(value)
        };
    }

    fn write_bytes(&mut self, offset: usize, bytes: &[u8]) {
        // SAFETY: callers pass a range inside this retained test allocation.
        unsafe {
            core::slice::from_raw_parts_mut(self.base.as_ptr().add(offset), bytes.len())
                .copy_from_slice(bytes)
        };
    }

    fn read_bytes(&self, offset: usize, length: usize) -> &[u8] {
        // SAFETY: callers pass ranges inside this allocation.
        unsafe { core::slice::from_raw_parts(self.base.as_ptr().add(offset), length) }
    }
}

impl Drop for TestMemory {
    fn drop(&mut self) {
        // SAFETY: this pointer was allocated using this exact layout.
        unsafe { dealloc(self.base.as_ptr(), self.layout) };
    }
}

struct MockGuard(Observation);

impl Guard for MockGuard {
    fn observation(&self) -> Observation {
        self.0
    }
}

struct MockGate(Rc<Cell<Observation>>);

unsafe impl Gate for MockGate {
    type Guard<'a> = MockGuard;

    fn lock(&mut self) -> Result<Self::Guard<'_>, regs::Error> {
        Ok(MockGuard(self.0.get()))
    }
}

fn observation(mmio_virtual: u64, dma_virtual: u64) -> Observation {
    Observation {
        generation: GENERATION,
        fresh_boot: true,
        exclusive: true,
        platform_ready: true,
        ce_owner_exclusive: true,
        ce_powered: true,
        ce_reset_released: true,
        ce_quiescent: true,
        ce_irq_intid: None,
        ce_irq_bound: false,
        ce_irq_exclusive: false,
        htt_ce_irq_intid: None,
        htt_ce_irq_bound: false,
        htt_ce_irq_exclusive: false,
        wmi_map_generation: 0,
        wmi_host_to_target_engine: None,
        wmi_target_to_host_engine: None,
        htc_control_map_generation: 0,
        htc_control_host_to_target_engine: None,
        htc_control_target_to_host_engine: None,
        htt_map_generation: GENERATION,
        htt_host_to_target_engine: Some(4),
        htt_target_to_host_engine: Some(1),
        htt_endpoint_generation: GENERATION,
        htt_endpoint_id: Some(5),
        htt_endpoint_max_message_size: 512,
        mmio_device_uncached: true,
        ce_mmio_virtual: mmio_virtual,
        ce_mmio_physical: CE_MMIO_PHYSICAL_BASE,
        ce_mmio_bytes: CE_MMIO_BYTES as u64,
        dma_virtual,
        dma_iova: DMA_IOVA,
        dma_bytes: DMA_BYTES as u64,
        dma_uncached: true,
        dma_exclusive: true,
        dma_translated: true,
        smmu_route_slot: Some(7),
        smmu_route_id: 0x0240,
        smmu_route_mask: 1,
        smmu_route_sids: WLAN_SIDS,
        smmu_route_translated: true,
        smmu_context_bank: Some(4),
        smmu_context_s1_translation_enabled: true,
        smmu_context_enabled: true,
    }
}

fn route() -> [ServiceCeMapping; 2] {
    [
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::HostToTarget,
            engine: 4,
        },
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::TargetToHost,
            engine: 1,
        },
    ]
}

fn endpoint() -> sunfish_qcacld_htc::HttEndpoint {
    let encoded = (5u32 << 8) | (512u32 << 16);
    let response = [
        3,
        0,
        0,
        3,
        encoded as u8,
        (encoded >> 8) as u8,
        (encoded >> 16) as u8,
        (encoded >> 24) as u8,
        0,
        0,
        0,
        0,
    ];
    parse_connect_response(
        &response,
        qcom_htc::HtcReady {
            credit_count: 4,
            credit_size: 64,
            max_endpoints: 9,
            htc_version: 1,
            max_messages_per_bundle: 1,
            alternate_data_credit_size: 0,
        },
    )
    .unwrap()
}

fn frame() -> Vec<u8> {
    let mut frame = vec![0u8; 400];
    frame[..2].copy_from_slice(&0x0008u16.to_le_bytes());
    frame
}

fn setup() -> (
    TestMemory,
    TestMemory,
    Rc<Cell<Observation>>,
    CeMmioMapping,
    RingPlan,
    Vec<u8>,
    CeTxPlan,
    u64,
    u64,
) {
    let mut mmio = TestMemory::new(0x0080_0000);
    let dma = TestMemory::new(DMA_BYTES);
    let mapping = mmio.mapping();
    let current = Rc::new(Cell::new(observation(
        mapping.virtual_base,
        dma.base.as_ptr() as usize as u64,
    )));
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let rings = plan_rings(Profile::Mission, DMA_IOVA, DMA_BYTES as u64).unwrap();
    let ring = rings
        .allocations()
        .iter()
        .find(|allocation| allocation.engine == 4)
        .unwrap();
    for register in [
        Register::CurrentSourceReadIndex,
        Register::SourceWriteIndex,
        Register::SourceSize,
        Register::SourceBaseLow,
        Register::SourceBaseHigh,
    ] {
        let spec = regs::register_spec(4, register).unwrap();
        let value = match register {
            Register::SourceSize => u32::from(ring.entries),
            Register::SourceBaseLow => ring.descriptor_iova as u32,
            Register::SourceBaseHigh => (ring.descriptor_iova >> 32) as u32,
            _ => 0,
        };
        mmio.write_u32(spec.offset, value);
    }
    let data_base = DMA_IOVA + rings.total_bytes().div_ceil(4096) * 4096 + 4096;
    let prefix_iova = data_base;
    let fragment_iova = data_base + 0x1000;
    let frame_iova = data_base + 0x2000;
    let tx_frame = frame();
    let metadata = TxMetadata {
        fragment_table_iova: fragment_iova,
        frame_iova,
        msdu_id: 3,
        vdev_id: 1,
        ext_tid: 0,
        hardware_encrypt: false,
        request_completion: false,
    };
    let plan = plan_native_wifi_tx(
        &route(),
        endpoint(),
        0x39,
        &tx_frame,
        metadata,
        CeTxBuffers {
            htc_htt_prefix: TxBufferWindow {
                iova: prefix_iova,
                capacity: 64,
            },
        },
        pool,
        Profile::Mission,
        &rings,
        RingCursor::new(usize::from(ring.entries)).unwrap(),
    )
    .unwrap();
    (
        mmio,
        dma,
        current,
        mapping,
        rings,
        tx_frame,
        plan,
        prefix_iova,
        fragment_iova,
    )
}

fn make_io(mapping: CeMmioMapping, current: Rc<Cell<Observation>>) -> NativeIo<MockGate> {
    // SAFETY: test allocations stay alive and the mock gate reports their
    // exact mappings for every native access.
    unsafe { NativeIo::new(mapping, GENERATION, MockGate(current)) }.unwrap()
}

fn htt_rx_plan() -> sunfish_htt_rx_ring::SunfishHttRxRingPlan {
    plan_sunfish_htt_rx_ring(
        plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap(),
        SunfishHttRxRingMemory {
            ring_iova: DMA_IOVA,
            alloc_index_iova: DMA_IOVA + 0x4000,
            initial_buffers_iova: DMA_IOVA + 0x4008,
            fallback_buffers_iova: DMA_IOVA + 0x0044_3788,
        },
        SunfishHttRxRingOptions {
            packet_log_enabled: false,
            global_monitor_mode: false,
        },
    )
    .unwrap()
}

#[test]
fn native_htt_refill_holds_authority_and_publishes_into_exact_dma_backing() {
    let plan = htt_rx_plan();
    let mut mmio = TestMemory::new(0x0080_0000);
    let mut dma = TestMemory::new(RX_DMA_BYTES);
    let mapping = mmio.mapping();
    let mut live = observation(mapping.virtual_base, dma.base.as_ptr() as usize as u64);
    live.dma_bytes = RX_DMA_BYTES as u64;
    let current = Rc::new(Cell::new(live));
    let mut io = make_io(mapping, current);

    let memory = plan.memory();
    let index_offset = usize::try_from(memory.alloc_index_iova - DMA_IOVA).unwrap();
    dma.write_bytes(index_offset, &plan.initial_alloc_index().to_le_bytes());
    let initial_offset = usize::try_from(memory.initial_buffers_iova - DMA_IOVA).unwrap();
    let fallback_offset = usize::try_from(memory.fallback_buffers_iova - DMA_IOVA).unwrap();
    let initial_bytes = plan.initial_buffer_bytes().unwrap() as usize;
    let fallback_bytes = plan.fallback_buffer_bytes() as usize;
    // SAFETY: the planned ranges lie within the retained 5 MiB DMA test
    // allocation, are disjoint, and remain alive through the refill call.
    let (initial, fallback) = unsafe {
        (
            core::slice::from_raw_parts_mut(dma.base.as_ptr().add(initial_offset), initial_bytes),
            core::slice::from_raw_parts_mut(dma.base.as_ptr().add(fallback_offset), fallback_bytes),
        )
    };
    let mut pool = SunfishHttRxBufferPool::new(plan, initial, fallback).unwrap();
    let mut session = SunfishQcacldRxSession::<2048>::new().unwrap();
    for buffer_id in 0..plan.initial_fill_level() {
        session.post(buffer_id).unwrap();
    }
    let completed = session.take_next().unwrap();
    assert_eq!(completed.buffer_id(), 0);
    session.release(completed).unwrap();
    let mut refill = SunfishHttRxRingRefillState::new(plan);

    assert_eq!(
        io.refill_htt_rx_batch(GENERATION, plan, &mut refill, &mut session, &[0], &mut pool,),
        Ok(2047)
    );
    assert_eq!(refill.next_alloc_index(), 0);
    assert_eq!(session.posted_count(), 2047);
    assert_eq!(session.occupied_count(), 2047);
    drop(pool);
    assert_eq!(
        dma.read_bytes(0x3ff8, 8),
        &plan.initial_buffer_iova(0).unwrap().to_le_bytes()
    );
    assert_eq!(dma.read_bytes(index_offset, 4), &0u32.to_le_bytes());
}

#[test]
fn native_htt_refill_rejects_unmapped_pool_without_touching_ring_or_session() {
    let plan = htt_rx_plan();
    let mut mmio = TestMemory::new(0x0080_0000);
    let mut dma = TestMemory::new(RX_DMA_BYTES);
    let mapping = mmio.mapping();
    let mut live = observation(mapping.virtual_base, dma.base.as_ptr() as usize as u64);
    live.dma_bytes = RX_DMA_BYTES as u64;
    let current = Rc::new(Cell::new(live));
    let mut io = make_io(mapping, current);
    let index_offset = usize::try_from(plan.memory().alloc_index_iova - DMA_IOVA).unwrap();
    dma.write_bytes(index_offset, &plan.initial_alloc_index().to_le_bytes());

    let mut wrong_initial = vec![0u8; plan.initial_buffer_bytes().unwrap() as usize];
    let mut wrong_fallback = vec![0u8; plan.fallback_buffer_bytes() as usize];
    let mut pool =
        SunfishHttRxBufferPool::new(plan, &mut wrong_initial, &mut wrong_fallback).unwrap();
    let mut session = SunfishQcacldRxSession::<2048>::new().unwrap();
    for buffer_id in 0..plan.initial_fill_level() {
        session.post(buffer_id).unwrap();
    }
    let completed = session.take_next().unwrap();
    session.release(completed).unwrap();
    let before_session = session;
    let mut refill = SunfishHttRxRingRefillState::new(plan);

    assert_eq!(
        io.refill_htt_rx_batch(GENERATION, plan, &mut refill, &mut session, &[0], &mut pool,),
        Err(HttRxRefillError::InvalidPlan)
    );
    assert_eq!(session, before_session);
    assert_eq!(refill.next_alloc_index(), plan.initial_alloc_index());
    assert_eq!(dma.read_bytes(index_offset, 4), &2047u32.to_le_bytes());
    assert_eq!(dma.read_bytes(0x3ff8, 8), &[0; 8]);
}

#[test]
fn native_qcacld_publish_stages_gather_buffers_and_both_descriptors_before_ce4_index() {
    let (mmio, dma, current, mapping, rings, frame, plan, prefix_iova, fragment_iova) = setup();
    let mut io = make_io(mapping, current.clone());
    let mut live = current.get();
    live.ce_quiescent = false;
    current.set(live);
    let mut cursor = RingCursor::new(2048).unwrap();

    io.publish_qcacld_data_tx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &frame,
        &mut cursor,
    )
    .unwrap();

    let prefix_offset = (prefix_iova - DMA_IOVA) as usize;
    let frame_offset = (plan.frame_iova() - DMA_IOVA) as usize;
    let fragment_offset = (fragment_iova - DMA_IOVA) as usize;
    let first_slot_offset = (plan.first_slot_iova() - DMA_IOVA) as usize;
    let second_slot_offset = (plan.second_slot_iova() - DMA_IOVA) as usize;
    assert_eq!(
        &dma.read_bytes(prefix_offset, plan.htc_htt_prefix().len()),
        &plan.htc_htt_prefix()
    );
    assert_eq!(dma.read_bytes(frame_offset, frame.len()), frame.as_slice());
    assert_eq!(
        dma.read_bytes(fragment_offset, HELIUMPLUS_FRAGMENT_DESC_BYTES),
        &plan.fragment_descriptor()
    );
    assert_eq!(
        dma.read_bytes(first_slot_offset, qcom_ce_v3::DESCRIPTOR_BYTES),
        plan.first_descriptor().as_bytes()
    );
    assert_eq!(
        dma.read_bytes(second_slot_offset, qcom_ce_v3::DESCRIPTOR_BYTES),
        plan.second_descriptor().as_bytes()
    );
    let producer = regs::register_spec(4, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 2);
    assert_eq!(cursor.producer_index(), 2);
    assert_eq!(cursor.pending(), 2);
}

#[test]
fn native_qcacld_reclaim_tracks_partial_and_complete_gather_progress() {
    let (mut mmio, _dma, current, mapping, rings, _frame, _plan, _prefix, _fragment) = setup();
    let mut io = make_io(mapping, current.clone());
    let mut live = current.get();
    live.ce_quiescent = false;
    current.set(live);
    let mut cursor = RingCursor::new(2048).unwrap();
    let read = regs::register_spec(4, Register::CurrentSourceReadIndex).unwrap();
    let producer = regs::register_spec(4, Register::SourceWriteIndex).unwrap();
    mmio.write_u32(producer.offset, 2);
    cursor
        .commit_publish_batch(cursor.prepare_next().unwrap(), 2)
        .unwrap();

    mmio.write_u32(read.offset, 1);
    assert_eq!(
        io.reclaim_qcacld_data_tx(GENERATION, Profile::Mission, &rings, &mut cursor),
        Ok(1)
    );
    assert_eq!(cursor.consumer_index(), 1);
    assert_eq!(cursor.pending(), 1);
    mmio.write_u32(read.offset, 2);
    assert_eq!(
        io.reclaim_qcacld_data_tx(GENERATION, Profile::Mission, &rings, &mut cursor),
        Ok(1)
    );
    assert_eq!(cursor.consumer_index(), 2);
    assert_eq!(cursor.pending(), 0);
}

#[test]
fn native_qcacld_publish_rejects_stale_endpoint_and_wrong_live_ring_without_writes() {
    let (mut mmio, dma, current, mapping, rings, frame, plan, _prefix, _fragment) = setup();
    let mut io = make_io(mapping, current.clone());
    let mut cursor = RingCursor::new(2048).unwrap();
    let producer = regs::register_spec(4, Register::SourceWriteIndex).unwrap();

    let mut stale = current.get();
    stale.htt_endpoint_generation = GENERATION + 1;
    current.set(stale);
    assert_eq!(
        io.publish_qcacld_data_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            plan,
            &frame,
            &mut cursor
        ),
        Err(TxPublishError::LeaseLost)
    );
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert!(dma.read_bytes(0, DMA_BYTES).iter().all(|byte| *byte == 0));

    let mut valid = current.get();
    valid.htt_endpoint_generation = GENERATION;
    valid.htt_endpoint_id = Some(5);
    current.set(valid);
    let wrong_base = regs::register_spec(4, Register::SourceBaseLow).unwrap();
    mmio.write_u32(wrong_base.offset, (DMA_IOVA + 0x1000) as u32);
    assert_eq!(
        io.publish_qcacld_data_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            plan,
            &frame,
            &mut cursor
        ),
        Err(TxPublishError::LeaseLost)
    );
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert!(dma.read_bytes(0, DMA_BYTES).iter().all(|byte| *byte == 0));
}

#[test]
fn native_htt_ce1_rx_posts_then_consumes_a_runtime_indication() {
    let (mut mmio, mut dma, current, mapping, rings, _frame, _tx_plan, _prefix, _fragment) =
        setup();
    let ce1_ring = rings
        .allocations()
        .iter()
        .find(|allocation| allocation.engine == HTT_DATA_DESTINATION_CE)
        .unwrap();
    for register in [
        Register::CurrentDestinationReadIndex,
        Register::DestinationWriteIndex,
        Register::DestinationSize,
        Register::DestinationBaseLow,
        Register::DestinationBaseHigh,
    ] {
        let spec = regs::register_spec(usize::from(HTT_DATA_DESTINATION_CE), register).unwrap();
        let value = match register {
            Register::DestinationSize => u32::from(ce1_ring.entries),
            Register::DestinationBaseLow => ce1_ring.descriptor_iova as u32,
            Register::DestinationBaseHigh => (ce1_ring.descriptor_iova >> 32) as u32,
            _ => 0,
        };
        mmio.write_u32(spec.offset, value);
    }

    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let data_base = DMA_IOVA + rings.total_bytes().div_ceil(4096) * 4096 + 4096;
    let buffer = RxBufferWindow {
        iova: data_base + 0x3000,
        capacity: 2048,
    };
    let service_map = route();
    let expected_route = WmiRoute {
        host_to_target_engine: HTT_DATA_SOURCE_CE,
        target_to_host_engine: HTT_DATA_DESTINATION_CE,
    };
    let mut ledger = RxBufferLedger::new();
    let mut cursor = RingCursor::new(usize::from(ce1_ring.entries)).unwrap();
    let plan = plan_service_rx_post(
        &service_map,
        HTT_DATA_SERVICE_ID,
        expected_route,
        Profile::Mission,
        pool,
        buffer,
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();
    let mut io = make_io(mapping, current.clone());
    let mut live = current.get();
    live.ce_quiescent = false;
    current.set(live);

    let mut stale = current.get();
    stale.htt_map_generation = GENERATION + 1;
    current.set(stale);
    assert_eq!(
        io.post_htt_rx_ce1(
            GENERATION,
            Profile::Mission,
            &rings,
            plan,
            &mut cursor,
            &mut ledger,
        ),
        Err(RxPostError::LeaseLost)
    );
    let producer = regs::register_spec(
        usize::from(HTT_DATA_DESTINATION_CE),
        Register::DestinationWriteIndex,
    )
    .unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert!(dma.read_bytes(0, DMA_BYTES).iter().all(|byte| *byte == 0));

    let mut live = current.get();
    live.htt_map_generation = GENERATION;
    current.set(live);
    io.post_htt_rx_ce1(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 1);
    assert_eq!(cursor.pending(), 1);
    assert_eq!(ledger.len(), 1);
    let buffer_offset = (buffer.iova - DMA_IOVA) as usize;
    assert!(
        dma.read_bytes(buffer_offset, buffer.capacity as usize)
            .iter()
            .all(|byte| *byte == 0)
    );

    let mut completed_descriptor = *plan.descriptor().as_bytes();
    completed_descriptor[8..10].copy_from_slice(&6u16.to_le_bytes());
    completed_descriptor[10..12].copy_from_slice(&4u16.to_le_bytes());
    let descriptor_offset = (plan.descriptor_slot_iova() - DMA_IOVA) as usize;
    dma.write_bytes(descriptor_offset, &completed_descriptor);
    let packet = [0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6];
    dma.write_bytes(buffer_offset, &packet);
    let read = regs::register_spec(
        usize::from(HTT_DATA_DESTINATION_CE),
        Register::CurrentDestinationReadIndex,
    )
    .unwrap();
    mmio.write_u32(read.offset, 1);

    let mut output = [0u8; 8];
    let completion = io
        .consume_htt_rx_ce1(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut ledger,
            &mut output,
        )
        .unwrap();
    assert_eq!(completion.nbytes, packet.len() as u16);
    assert_eq!(&output[..packet.len()], &packet);
    assert_eq!(cursor.pending(), 0);
    assert!(ledger.is_empty());
}
