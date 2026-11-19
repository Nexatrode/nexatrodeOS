#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

#[path = "../rust/src/ath10k_htt.rs"]
mod ath10k_htt;
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
#[path = "../rust/src/sunfish_qcacld_htt_event.rs"]
mod sunfish_qcacld_htt_event;
#[path = "../rust/src/sunfish_qcacld_htt_event_native.rs"]
mod sunfish_qcacld_htt_event_native;
#[path = "../rust/src/sunfish_qcacld_key.rs"]
mod sunfish_qcacld_key;
#[path = "../rust/src/sunfish_qcacld_key_native.rs"]
mod sunfish_qcacld_key_native;
#[path = "../rust/src/sunfish_qcacld_mgmt_tx.rs"]
mod sunfish_qcacld_mgmt_tx;
#[path = "../rust/src/sunfish_qcacld_resource.rs"]
mod sunfish_qcacld_resource;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_ind.rs"]
mod sunfish_qcacld_rx_ind;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_qcacld_tx.rs"]
mod sunfish_qcacld_tx;
#[path = "../rust/src/sunfish_qcacld_wmi_rx.rs"]
mod sunfish_qcacld_wmi_rx;
#[path = "../rust/src/sunfish_qcacld_wmi_rx_native.rs"]
mod sunfish_qcacld_wmi_rx_native;
#[path = "../rust/src/sunfish_qcacld_wmi_startup.rs"]
mod sunfish_qcacld_wmi_startup;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
#[path = "../rust/src/wlan_key_confirmation.rs"]
mod wlan_key_confirmation;

use core::cell::Cell;
use core::ptr::NonNull;
use qcom_ce_v3::RingCursor;
use qcom_ce_v3_native_io::{
    CE_MMIO_BYTES, CE_MMIO_PHYSICAL_BASE, CeMmioMapping, Gate, Guard, HttConnectRxError, NativeIo,
    Observation, RxConsumeError, RxIrqError, RxPostError, TxPublishError, WLAN_SIDS,
};
use qcom_ce_v3_profile::{Profile, RingPlan, plan_rings};
use qcom_ce_v3_regs::{self as regs, Register};
use qcom_htc::{HtcCreditLedger, HtcReady, WmiTxPlan, encode_wmi_command_frame};
use qcom_wmi_ce::{
    Direction, DmaPool, HTT_DATA_SERVICE_ID, RxBufferLedger, RxBufferWindow,
    SensitiveTxBufferLease, SensitiveTxBufferLedger, ServiceCeMapping, TxBufferWindow,
    WMI_CONTROL_SERVICE_ID, WmiCeRxPostPlan, WmiCeTxPlan, WmiRoute, plan_service_rx_post,
    plan_wmi_rx_post, plan_wmi_tx,
};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::rc::Rc;
use sunfish_qcacld_htc::{HttStartupPhase, HttStartupSession};
use sunfish_qcacld_htt_event::{CompletedEvent, Event};
use sunfish_qcacld_htt_event_native::service_native_htt_ce1_irq;
use sunfish_qcacld_key::{
    CcmpInstallKey, KeyKind, QcacldCcmpKeyInstallTracker, plan_qcacld_key_install_ce_tx,
};
use sunfish_qcacld_key_native::publish_qcacld_key_install;
use sunfish_qcacld_mgmt_tx::{
    MgmtTxLease, MgmtTxOutstanding, MgmtTxRequest, WmiMgmtTxCapability, encode_wmi_mgmt_tx_frame,
    parse_wmi_mgmt_tx_completion, plan_wmi_mgmt_tx_ce,
};
use sunfish_qcacld_resource::SunfishQcacldResourceInputs;
use sunfish_qcacld_wmi_rx::{
    SunfishWmiStartupSession, WMI_READY_EVENT_ID, WMI_SERVICE_AVAILABLE_EVENT_ID,
    WMI_SERVICE_READY_EVENT_ID, receive_wmi_event,
};
use sunfish_qcacld_wmi_rx_native::{
    service_native_wmi_ce2_irq, service_native_wmi_ce2_irq_with_action,
};
use sunfish_qcacld_wmi_startup::{
    SunfishQcacldWmiInitRuntime, SunfishQcacldWmiInitRuntimeOutcome, SunfishWmiControlSession,
    WmiControlPhase, WmiControlSessionError, publish_sunfish_qcacld_wmi_init_after_observe,
};
use wlan_key_confirmation::KeyInstallConfirmation;

const GENERATION: u64 = 9;
const DMA_IOVA: u64 = 0x8000_0000;
const DMA_BYTES: usize = 0x0004_0000;
const FRAME_BYTES: [u8; 13] = [1, 1, 5, 0, 0, 0, 0, 0, 0x34, 0x12, 0, 0, 0x5a];
const WMI_CONTROL_READY: [u8; 12] = [1, 0, 8, 0, 0, 1, 9, 0, 1, 1, 0, 0];

struct TestMemory {
    base: NonNull<u8>,
    layout: Layout,
}

impl TestMemory {
    fn new(bytes: usize) -> Self {
        let layout = Layout::from_size_align(bytes, 4096).unwrap();
        // SAFETY: the matching layout is retained and used by Drop.
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
        // SAFETY: offsets are register-aligned and inside this allocation.
        unsafe { self.base.as_ptr().add(offset).cast::<u32>().read_volatile() }
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        // SAFETY: offsets are register-aligned and inside this allocation.
        unsafe {
            self.base
                .as_ptr()
                .add(offset)
                .cast::<u32>()
                .write_volatile(value)
        };
    }
}

impl Drop for TestMemory {
    fn drop(&mut self) {
        // SAFETY: this pointer was allocated with this exact layout.
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
        ce_irq_intid: Some(0x01c0),
        ce_irq_bound: true,
        ce_irq_exclusive: true,
        htt_ce_irq_intid: Some(0x01bf),
        htt_ce_irq_bound: true,
        htt_ce_irq_exclusive: true,
        wmi_map_generation: GENERATION,
        wmi_host_to_target_engine: Some(3),
        wmi_target_to_host_engine: Some(2),
        htc_control_map_generation: GENERATION,
        htc_control_host_to_target_engine: Some(0),
        htc_control_target_to_host_engine: Some(2),
        htt_map_generation: 0,
        htt_host_to_target_engine: None,
        htt_target_to_host_engine: None,
        htt_endpoint_generation: 0,
        htt_endpoint_id: None,
        htt_endpoint_max_message_size: 0,
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
            service_id: WMI_CONTROL_SERVICE_ID,
            direction: Direction::HostToTarget,
            engine: 3,
        },
        ServiceCeMapping {
            service_id: WMI_CONTROL_SERVICE_ID,
            direction: Direction::TargetToHost,
            engine: 2,
        },
    ]
}

fn management_tx_capability() -> WmiMgmtTxCapability {
    fn append_tlv(event: &mut Vec<u8>, tag: u16, value: &[u8]) {
        event.extend_from_slice(&(value.len() as u16).to_le_bytes());
        event.extend_from_slice(&tag.to_le_bytes());
        event.extend_from_slice(value);
    }

    let fixed = [0u8; 128];
    let mut bitmap = [0u8; 128];
    bitmap[19 * 4..20 * 4].copy_from_slice(&1u32.to_le_bytes());
    let mut event = Vec::new();
    append_tlv(&mut event, 32, &fixed);
    append_tlv(&mut event, 33, &[0; 36]);
    append_tlv(&mut event, 16, &bitmap);
    let service_ready = qcacld_wmi::ServiceReadyView::parse(&event).unwrap();
    WmiMgmtTxCapability::from_service_ready(GENERATION, &service_ready).unwrap()
}

fn htt_route() -> [ServiceCeMapping; 2] {
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

fn make_frame() -> (&'static [u8], WmiTxPlan) {
    let mut frame = vec![0u8; 64];
    let plan = encode_wmi_command_frame(1, 256, 64, 4, 0x1234, &[0x5a], &mut frame).unwrap();
    assert_eq!(&frame[..plan.frame_len], FRAME_BYTES);
    (&FRAME_BYTES, plan)
}

fn setup() -> (
    TestMemory,
    TestMemory,
    Rc<Cell<Observation>>,
    CeMmioMapping,
    RingPlan,
    TxBufferWindow,
    &'static [u8],
    WmiTxPlan,
    WmiCeTxPlan<'static>,
) {
    let mut mmio = TestMemory::new(0x0080_0000);
    let dma = TestMemory::new(DMA_BYTES);
    let mapping = mmio.mapping();
    let current = Rc::new(Cell::new(observation(
        mapping.virtual_base,
        dma.base.as_ptr() as usize as u64,
    )));
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let rings = plan_rings(Profile::Mission, pool.base(), pool.bytes()).unwrap();
    let control_ring = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 0 && ring.direction == qcom_ce_v3_profile::RingDirection::Source
        })
        .unwrap();
    let control_base_low = regs::register_spec(0, Register::SourceBaseLow).unwrap();
    let control_base_high = regs::register_spec(0, Register::SourceBaseHigh).unwrap();
    let control_size = regs::register_spec(0, Register::SourceSize).unwrap();
    mmio.write_u32(control_base_low.offset, control_ring.descriptor_iova as u32);
    mmio.write_u32(
        control_base_high.offset,
        (control_ring.descriptor_iova >> 32) as u32,
    );
    mmio.write_u32(control_size.offset, u32::from(control_ring.entries));
    let tx_ring = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 3 && ring.direction == qcom_ce_v3_profile::RingDirection::Source
        })
        .unwrap();
    let base_low = regs::register_spec(3, Register::SourceBaseLow).unwrap();
    let base_high = regs::register_spec(3, Register::SourceBaseHigh).unwrap();
    let size = regs::register_spec(3, Register::SourceSize).unwrap();
    mmio.write_u32(base_low.offset, tx_ring.descriptor_iova as u32);
    mmio.write_u32(base_high.offset, (tx_ring.descriptor_iova >> 32) as u32);
    mmio.write_u32(size.offset, u32::from(tx_ring.entries));
    let rx_ring = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 2 && ring.direction == qcom_ce_v3_profile::RingDirection::Destination
        })
        .unwrap();
    let rx_base_low = regs::register_spec(2, Register::DestinationBaseLow).unwrap();
    let rx_base_high = regs::register_spec(2, Register::DestinationBaseHigh).unwrap();
    let rx_size = regs::register_spec(2, Register::DestinationSize).unwrap();
    mmio.write_u32(rx_base_low.offset, rx_ring.descriptor_iova as u32);
    mmio.write_u32(rx_base_high.offset, (rx_ring.descriptor_iova >> 32) as u32);
    mmio.write_u32(rx_size.offset, u32::from(rx_ring.entries));
    let next_page = (DMA_IOVA + rings.total_bytes() + 4095) & !4095;
    let buffer = TxBufferWindow {
        iova: next_page,
        capacity: 512,
    };
    let (frame, wmi) = make_frame();
    let tx = plan_wmi_tx(
        &route(),
        &frame,
        wmi,
        1,
        7,
        pool,
        buffer,
        Profile::Mission,
        &rings,
        RingCursor::new(usize::from(tx_ring.entries)).unwrap(),
    )
    .unwrap();
    (mmio, dma, current, mapping, rings, buffer, frame, wmi, tx)
}

fn make_io(mapping: CeMmioMapping, current: Rc<Cell<Observation>>) -> NativeIo<MockGate> {
    // SAFETY: test allocations stay alive for NativeIo's lifetime, and the
    // mock gate reports those exact, exclusive test mappings.
    unsafe { NativeIo::new(mapping, GENERATION, MockGate(current)) }.unwrap()
}

fn make_rx_plan(rings: &RingPlan, buffer: TxBufferWindow) -> WmiCeRxPostPlan {
    plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap(),
        RxBufferWindow {
            iova: buffer.iova,
            capacity: 2048,
        },
        &RxBufferLedger::new(),
        rings,
        RingCursor::new(128).unwrap(),
    )
    .unwrap()
}

fn simulate_rx_completion(
    dma: &TestMemory,
    mmio: &mut TestMemory,
    plan: WmiCeRxPostPlan,
    payload: &[u8],
) {
    let descriptor_offset = (plan.descriptor_slot_iova() - DMA_IOVA) as usize;
    let payload_offset = (plan.buffer().iova - DMA_IOVA) as usize;
    // SAFETY: both ranges are part of the live test DMA allocation and the
    // fake device is the only writer used by this test.
    unsafe {
        for (index, byte) in payload.iter().copied().enumerate() {
            dma.base
                .as_ptr()
                .add(payload_offset + index)
                .write_volatile(byte);
        }
        dma.base
            .as_ptr()
            .add(descriptor_offset + 8)
            .cast::<u32>()
            .write_volatile(payload.len() as u32 | (7 << 16));
    }
    let read = regs::register_spec(2, Register::CurrentDestinationReadIndex).unwrap();
    mmio.write_u32(read.offset, plan.next_producer_index() as u32);
}

fn make_wmi_rx_event(endpoint_id: u8, event_id: u32, body: &[u8]) -> Vec<u8> {
    let declared = 4 + body.len();
    let mut bytes = vec![0u8; 8 + declared];
    let header = u32::from(endpoint_id) | ((declared as u32) << 16);
    bytes[..4].copy_from_slice(&header.to_le_bytes());
    bytes[8..12].copy_from_slice(&event_id.to_le_bytes());
    bytes[12..].copy_from_slice(body);
    bytes
}

fn make_htt_connect_response(endpoint_id: u8, max_message_size: u16) -> [u8; 20] {
    let mut frame = [0u8; 20];
    frame[..4].copy_from_slice(&(12u32 << 16).to_le_bytes());
    let response_header = 3u32 | (u32::from(HTT_DATA_SERVICE_ID) << 16);
    frame[8..12].copy_from_slice(&response_header.to_le_bytes());
    let endpoint_and_size = (u32::from(max_message_size) << 16) | (u32::from(endpoint_id) << 8);
    frame[12..16].copy_from_slice(&endpoint_and_size.to_le_bytes());
    frame
}

fn put_test_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn append_test_tlv(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(value);
}

fn qcacld_service_ready_body() -> Vec<u8> {
    let mut fixed = [0u8; 128];
    for (index, word) in [
        qcacld_wmi::SUNFISH_WCN3998_FIRMWARE_BUILD,
        0x0100_0000,
        762,
        0x5f41_4351,
        0x0000_4c4d,
        0,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        put_test_word(&mut fixed, index * 4, word);
    }
    let mut body = Vec::new();
    append_test_tlv(&mut body, 32, &fixed);
    append_test_tlv(&mut body, 33, &[0; 36]);
    append_test_tlv(&mut body, 16, &[0; 128]);
    append_test_tlv(&mut body, 18, &[]);
    body
}

fn qcacld_service_available_body() -> Vec<u8> {
    let mut fixed = [0u8; 20];
    put_test_word(&mut fixed, 0, 128);
    let mut body = Vec::new();
    append_test_tlv(&mut body, 559, &fixed);
    body
}

fn qcacld_ready_body() -> Vec<u8> {
    let mut fixed = [0u8; 60];
    for (index, word) in [0x0100_0000, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0]
        .into_iter()
        .enumerate()
    {
        put_test_word(&mut fixed, index * 4, word);
    }
    put_test_word(&mut fixed, 24, 0x3322_1102);
    put_test_word(&mut fixed, 28, 0x0000_5544);
    let mut body = Vec::new();
    append_test_tlv(&mut body, 35, &fixed);
    body
}

#[test]
fn native_wmi_ce2_irq_drains_dispatches_acknowledges_and_rearms_posted_buffers() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let first_buffer = RxBufferWindow {
        iova: buffer.iova,
        capacity: 2048,
    };
    let second_buffer = RxBufferWindow {
        iova: buffer.iova + 0x1000,
        capacity: 2048,
    };
    let first_plan = plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        pool,
        first_buffer,
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        first_plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    let second_plan = plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        pool,
        second_buffer,
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        second_plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let frame = make_wmi_rx_event(7, 0x1234, &[0xaa, 0xbb]);
    simulate_rx_completion(&dma, &mut mmio, first_plan, &frame);
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, true)
        .unwrap();
    let host_ie = regs::register_spec(2, Register::HostInterruptEnable).unwrap();
    let host_is = regs::register_spec(2, Register::HostInterruptStatus).unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 1);
    mmio.write_u32(host_is.offset, regs::HOST_INTERRUPT_COPY_COMPLETE_MASK);

    let mut credits = HtcCreditLedger::new(GENERATION, 7, 8, 64, 512).unwrap();
    let mut outstanding = MgmtTxOutstanding::<4>::new(GENERATION).unwrap();
    let mut startup = SunfishWmiStartupSession::new(GENERATION);
    let mut scratch = [0u8; 512];
    let mut dispatched = Vec::new();
    let report = service_native_wmi_ce2_irq(
        &mut io,
        GENERATION,
        Profile::Mission,
        &rings,
        &mut cursor,
        &mut ledger,
        7,
        512,
        &mut credits,
        &mut outstanding,
        &mut startup,
        &mut scratch,
        4,
        |event| {
            dispatched.push(event.retired.received.event_id);
            Ok::<_, ()>(())
        },
    )
    .unwrap();

    assert_eq!(dispatched, [0x1234]);
    assert_eq!(report.completions, 1);
    assert!(report.irq_acknowledged);
    assert!(!report.needs_deferred_drain);
    assert_eq!(cursor.pending(), 1);
    assert_eq!(ledger.len(), 1);
    assert_eq!(mmio.read_u32(host_ie.offset), 1);
    assert_eq!(
        startup.phase(),
        sunfish_qcacld_wmi_rx::WmiStartupPhase::AwaitServiceReady
    );
}

#[test]
fn service_available_event_plans_and_publishes_wmi_init_before_ce2_ack() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let mut rx_cursor = RingCursor::new(128).unwrap();
    let mut rx_ledger = RxBufferLedger::new();
    let rx_buffer = RxBufferWindow {
        iova: buffer.iova,
        capacity: 2048,
    };
    let rx_plan = plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        pool,
        rx_buffer,
        &rx_ledger,
        &rings,
        rx_cursor,
    )
    .unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        rx_plan,
        &mut rx_cursor,
        &mut rx_ledger,
    )
    .unwrap();

    let control = started_wmi_control();
    let mut credits = control.wmi_credit_ledger(GENERATION).unwrap();
    let mut startup = SunfishWmiStartupSession::after_wmi_control_started(&control).unwrap();
    let service_ready = qcacld_service_ready_body();
    let service_ready_frame = make_wmi_rx_event(7, WMI_SERVICE_READY_EVENT_ID, &service_ready);
    let received =
        receive_wmi_event(GENERATION, 7, 512, &mut credits, &service_ready_frame).unwrap();
    startup.observe(&received).unwrap();
    assert_eq!(
        startup.phase(),
        sunfish_qcacld_wmi_rx::WmiStartupPhase::AwaitServiceAvailable
    );

    let service_available = qcacld_service_available_body();
    let service_available_frame =
        make_wmi_rx_event(7, WMI_SERVICE_AVAILABLE_EVENT_ID, &service_available);
    simulate_rx_completion(&dma, &mut mmio, rx_plan, &service_available_frame);
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &rx_cursor, true)
        .unwrap();
    let host_is = regs::register_spec(2, Register::HostInterruptStatus).unwrap();
    mmio.write_u32(host_is.offset, regs::HOST_INTERRUPT_COPY_COMPLETE_MASK);

    let ce3_entries = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 3 && ring.direction == qcom_ce_v3_profile::RingDirection::Source
        })
        .unwrap()
        .entries as usize;
    let mut tx_cursor = RingCursor::new(ce3_entries).unwrap();
    let tx_buffer = TxBufferWindow {
        iova: buffer.iova + 0x1000,
        capacity: 512,
    };
    let mappings = route();
    let mut body_output = [0u8; 512];
    let mut frame_output = [0u8; 512];
    let mut runtime = SunfishQcacldWmiInitRuntime {
        inputs: SunfishQcacldResourceInputs::observed_lineage_wmi_init_2026_10_24(
            qcacld_wmi::QcacldPeerLimit::OtherTarget32,
        ),
        hardware_mode_index: None,
        transfer_id: 7,
        mappings: &mappings,
        dma_pool: pool,
        buffer: tx_buffer,
        tx_cursor: &mut tx_cursor,
        wmi_body_output: &mut body_output,
        htc_frame_output: &mut frame_output,
    };
    let mut outstanding = MgmtTxOutstanding::<4>::new(GENERATION).unwrap();
    let mut scratch = [0u8; 512];
    let report = service_native_wmi_ce2_irq_with_action(
        &mut io,
        GENERATION,
        Profile::Mission,
        &rings,
        &mut rx_cursor,
        &mut rx_ledger,
        7,
        512,
        &mut credits,
        &mut outstanding,
        &mut startup,
        &mut scratch,
        4,
        |event, context| {
            assert_eq!(
                event.startup_transition,
                Some(sunfish_qcacld_wmi_rx::WmiStartupTransition::InitReady)
            );
            assert_eq!(
                context.startup.phase(),
                sunfish_qcacld_wmi_rx::WmiStartupPhase::InitReady
            );
            assert_eq!(context.credits.available(), 8);
            assert_eq!(
                publish_sunfish_qcacld_wmi_init_after_observe(event, context, &mut runtime,)
                    .unwrap(),
                SunfishQcacldWmiInitRuntimeOutcome::Published
            );
            Ok::<_, ()>(())
        },
    )
    .unwrap();

    assert_eq!(report.completions, 1);
    assert!(report.irq_acknowledged);
    assert!(!report.needs_deferred_drain);
    assert_eq!(
        startup.phase(),
        sunfish_qcacld_wmi_rx::WmiStartupPhase::AwaitReady
    );
    assert!(credits.in_flight() > 0);
    let htc_header = u32::from_le_bytes(runtime.htc_frame_output[..4].try_into().unwrap());
    assert_eq!(htc_header as u8, 7);
    assert_eq!((htc_header >> 16) as usize, 316);
    let wmi_command = u32::from_le_bytes(runtime.htc_frame_output[8..12].try_into().unwrap());
    assert_eq!(
        wmi_command & 0x00ff_ffff,
        qcacld_wmi::SUNFISH_WMI_INIT_COMMAND_ID
    );
    let published_offset = (tx_buffer.iova - DMA_IOVA) as usize;
    // SAFETY: the command was published into this test's live DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(published_offset), 324),
            &runtime.htc_frame_output[..324]
        );
    }
    drop(runtime);
    assert_eq!(tx_cursor.pending(), 1);
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 1);
}

#[test]
fn native_wmi_publish_copies_packet_and_descriptor_then_advances_one_index() {
    let (mmio, dma, current, mapping, rings, buffer, frame, _wmi, tx) = setup();
    let mut cursor = RingCursor::new(32).unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });

    io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor)
        .unwrap();

    let packet_offset = (buffer.iova - DMA_IOVA) as usize;
    let descriptor_offset = (tx.descriptor_slot_iova() - DMA_IOVA) as usize;
    // SAFETY: both slices are within the live test DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(packet_offset), frame.len()),
            frame
        );
        assert_eq!(
            core::slice::from_raw_parts(
                dma.base.as_ptr().add(descriptor_offset),
                qcom_ce_v3::DESCRIPTOR_BYTES
            ),
            tx.descriptor().as_bytes()
        );
    }
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 1);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(cursor.pending(), 1);
}

#[test]
fn native_reclaim_scrubs_sensitive_wmi_dma_only_after_matching_ce3_read_index() {
    let (mut mmio, dma, current, mapping, rings, buffer, frame, _wmi, tx) = setup();
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let mut cursor = RingCursor::new(32).unwrap();
    let lease = SensitiveTxBufferLease::new(
        GENERATION,
        tx.ticket(),
        TxBufferWindow {
            iova: buffer.iova,
            capacity: frame.len() as u32,
        },
    )
    .unwrap();
    let mut sensitive = SensitiveTxBufferLedger::<2>::new();
    sensitive.reserve(lease, cursor, pool).unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor)
        .unwrap();

    let buffer_offset = (buffer.iova - DMA_IOVA) as usize;
    // SAFETY: these bytes belong to the test's live, exclusive DMA pool.
    unsafe {
        dma.base
            .as_ptr()
            .add(buffer_offset + frame.len())
            .write_volatile(0xa5);
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(buffer_offset), frame.len()),
            frame
        );
    }
    assert_eq!(
        io.reclaim_sensitive_wmi_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut sensitive,
        )
        .unwrap(),
        qcom_ce_v3_native_io::SensitiveWmiTxReclaimReport {
            descriptors_reclaimed: 0,
            sensitive_buffers_scrubbed: 0,
            sensitive_bytes_scrubbed: 0,
        }
    );
    assert_eq!(sensitive.len(), 1);

    let read_index = regs::register_spec(3, Register::CurrentSourceReadIndex).unwrap();
    mmio.write_u32(read_index.offset, 2);
    assert_eq!(
        io.reclaim_sensitive_wmi_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut sensitive,
        ),
        Err(TxPublishError::Cursor(
            qcom_ce_v3::Error::InvalidHardwareIndex
        ))
    );
    // The invalid index was rejected before touching key-bearing memory.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(buffer_offset), frame.len()),
            frame
        );
    }

    mmio.write_u32(read_index.offset, 1);
    assert_eq!(
        io.reclaim_sensitive_wmi_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut sensitive,
        )
        .unwrap(),
        qcom_ce_v3_native_io::SensitiveWmiTxReclaimReport {
            descriptors_reclaimed: 1,
            sensitive_buffers_scrubbed: 1,
            sensitive_bytes_scrubbed: frame.len() as u64,
        }
    );
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(buffer_offset), frame.len()),
            vec![0; frame.len()]
        );
        assert_eq!(
            dma.base
                .as_ptr()
                .add(buffer_offset + frame.len())
                .read_volatile(),
            0xa5
        );
    }
    assert!(sensitive.is_empty());
    assert_eq!(cursor.pending(), 0);
}

#[test]
fn native_key_install_requires_wmi_ready_then_tracks_and_scrubs_consumed_frame() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let control = started_wmi_control();
    let mut credits = control.wmi_credit_ledger(GENERATION).unwrap();
    let mut startup = SunfishWmiStartupSession::after_wmi_control_started(&control).unwrap();

    let service_ready = qcacld_service_ready_body();
    let event = make_wmi_rx_event(7, WMI_SERVICE_READY_EVENT_ID, &service_ready);
    let received = receive_wmi_event(GENERATION, 7, 512, &mut credits, &event).unwrap();
    startup.observe(&received).unwrap();
    let service_available = qcacld_service_available_body();
    let event = make_wmi_rx_event(7, WMI_SERVICE_AVAILABLE_EVENT_ID, &service_available);
    let received = receive_wmi_event(GENERATION, 7, 512, &mut credits, &event).unwrap();
    startup.observe(&received).unwrap();
    startup.mark_init_submitted(GENERATION).unwrap();
    let ready = qcacld_ready_body();
    let event = make_wmi_rx_event(7, WMI_READY_EVENT_ID, &ready);
    let received = receive_wmi_event(GENERATION, 7, 512, &mut credits, &event).unwrap();
    startup.observe(&received).unwrap();
    assert_eq!(
        startup.phase(),
        sunfish_qcacld_wmi_rx::WmiStartupPhase::Ready
    );

    let peer = [0x02, 0x12, 0x34, 0x56, 0x78, 0x9a];
    let key = [0x5a; 16];
    let request = CcmpInstallKey {
        vdev_id: 1,
        peer_address: peer,
        key_index: 0,
        kind: KeyKind::Pairwise,
        temporal_key: &key,
        group_receive_sequence_counter: &[0; 8],
    };
    let mut tracker = QcacldCcmpKeyInstallTracker::new(22, 1, 0x42, peer, 0, None).unwrap();
    let mut cursor = RingCursor::new(32).unwrap();
    let mappings = route();
    let mut body_output = [0xa5; sunfish_qcacld_key::WMI_VDEV_INSTALL_KEY_BODY_BYTES];
    let mut frame_output = [0xa5; 512];
    let plan = plan_qcacld_key_install_ce_tx(
        GENERATION,
        request,
        7,
        512,
        credits.credit_size(),
        credits.available(),
        &mappings,
        3,
        pool,
        buffer,
        Profile::Mission,
        &rings,
        cursor,
        &mut body_output,
        &mut frame_output,
    )
    .unwrap();
    let frame_bytes = plan.frame_bytes();
    let mut sensitive = SensitiveTxBufferLedger::<2>::new();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    publish_qcacld_key_install(
        &mut io,
        plan,
        GENERATION,
        22,
        &startup,
        Profile::Mission,
        &rings,
        &mut cursor,
        &mut credits,
        &mut sensitive,
        &mut tracker,
    )
    .unwrap();

    assert_eq!(
        body_output,
        [0; sunfish_qcacld_key::WMI_VDEV_INSTALL_KEY_BODY_BYTES]
    );
    assert_eq!(&frame_output[..frame_bytes], &vec![0; frame_bytes]);
    assert!(frame_output[frame_bytes..].iter().all(|byte| *byte == 0xa5));
    assert_eq!(credits.available(), 7);
    assert_eq!(credits.in_flight(), 1);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(sensitive.len(), 1);
    let dma_offset = (buffer.iova - DMA_IOVA) as usize;
    // SAFETY: the key frame is in this test's live, exclusively owned DMA pool.
    let sent =
        unsafe { core::slice::from_raw_parts(dma.base.as_ptr().add(dma_offset), frame_bytes) };
    let decoded = qcom_htc::parse_wmi_tx_frame(sent, 7, 512).unwrap();
    assert_eq!(decoded.command_id, 0x5009);
    assert_eq!(&decoded.tlv_body[108..124], &key);
    assert_eq!(
        tracker.observe_security_indication(
            22,
            ath10k_htt::HttSecurityIndication {
                peer_id: 0x42,
                security_type: ath10k_htt::HTT_SECURITY_TYPE_AES_CCMP,
                is_unicast: true,
            },
        ),
        sunfish_qcacld_key::KeyInstallAction::Installed {
            kind: KeyKind::Pairwise,
            all_required_installed: true,
        }
    );
    assert!(tracker.confirms_keys_for_association(22, peer, 0, None));

    let read_index = regs::register_spec(3, Register::CurrentSourceReadIndex).unwrap();
    mmio.write_u32(read_index.offset, 1);
    assert_eq!(
        io.reclaim_sensitive_wmi_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut sensitive,
        )
        .unwrap(),
        qcom_ce_v3_native_io::SensitiveWmiTxReclaimReport {
            descriptors_reclaimed: 1,
            sensitive_buffers_scrubbed: 1,
            sensitive_bytes_scrubbed: frame_bytes as u64,
        }
    );
    // SAFETY: the lease's exact key-bearing DMA range was scrubbed above.
    assert!(unsafe {
        core::slice::from_raw_parts(dma.base.as_ptr().add(dma_offset), frame_bytes)
            .iter()
            .all(|byte| *byte == 0)
    });
    assert!(sensitive.is_empty());
    assert_eq!(cursor.pending(), 0);
}

#[test]
fn native_key_install_rejects_pre_ready_session_without_spending_credits_or_ce_slots() {
    let (_mmio, _dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let control = started_wmi_control();
    let mut credits = control.wmi_credit_ledger(GENERATION).unwrap();
    let startup = SunfishWmiStartupSession::after_wmi_control_started(&control).unwrap();
    let peer = [0x02, 0x12, 0x34, 0x56, 0x78, 0x9a];
    let key = [0x5a; 16];
    let request = CcmpInstallKey {
        vdev_id: 1,
        peer_address: peer,
        key_index: 0,
        kind: KeyKind::Pairwise,
        temporal_key: &key,
        group_receive_sequence_counter: &[0; 8],
    };
    let mut tracker = QcacldCcmpKeyInstallTracker::new(22, 1, 0x42, peer, 0, None).unwrap();
    let cursor = RingCursor::new(32).unwrap();
    let mappings = route();
    let mut body_output = [0xa5; sunfish_qcacld_key::WMI_VDEV_INSTALL_KEY_BODY_BYTES];
    let mut frame_output = [0xa5; 512];
    let plan = plan_qcacld_key_install_ce_tx(
        GENERATION,
        request,
        7,
        512,
        credits.credit_size(),
        credits.available(),
        &mappings,
        3,
        pool,
        buffer,
        Profile::Mission,
        &rings,
        cursor,
        &mut body_output,
        &mut frame_output,
    )
    .unwrap();
    let frame_bytes = plan.frame_bytes();
    let mut cursor = cursor;
    let mut sensitive = SensitiveTxBufferLedger::<2>::new();
    let mut io = make_io(mapping, current);

    assert!(matches!(
        publish_qcacld_key_install(
            &mut io,
            plan,
            GENERATION,
            22,
            &startup,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut credits,
            &mut sensitive,
            &mut tracker,
        ),
        Err(
            sunfish_qcacld_key_native::KeyInstallPublishError::StartupNotReady {
                phase: sunfish_qcacld_wmi_rx::WmiStartupPhase::AwaitServiceReady,
            }
        )
    ));
    assert_eq!(
        body_output,
        [0; sunfish_qcacld_key::WMI_VDEV_INSTALL_KEY_BODY_BYTES]
    );
    assert_eq!(&frame_output[..frame_bytes], &vec![0; frame_bytes]);
    assert_eq!(credits.available(), 8);
    assert_eq!(credits.in_flight(), 0);
    assert_eq!(cursor.producer_index(), 0);
    assert!(sensitive.is_empty());
    assert!(!tracker.publication_uncertain());
}

#[test]
fn native_unpublished_key_buffer_scrub_requires_the_live_ce3_producer_to_match() {
    let (mut mmio, dma, current, mapping, rings, buffer, frame, _wmi, tx) = setup();
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let cursor = RingCursor::new(32).unwrap();
    let lease = SensitiveTxBufferLease::new(
        GENERATION,
        tx.ticket(),
        TxBufferWindow {
            iova: buffer.iova,
            capacity: frame.len() as u32,
        },
    )
    .unwrap();
    let mut sensitive = SensitiveTxBufferLedger::<2>::new();
    sensitive.reserve(lease, cursor, pool).unwrap();
    let buffer_offset = (buffer.iova - DMA_IOVA) as usize;
    // SAFETY: the range belongs to this test's exclusive DMA mapping.
    unsafe {
        core::ptr::write_bytes(dma.base.as_ptr().add(buffer_offset), 0x5a, frame.len());
    }
    let mut io = make_io(mapping, current);

    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    mmio.write_u32(producer.offset, 1);
    assert_eq!(
        io.scrub_unpublished_sensitive_wmi_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            &cursor,
            &mut sensitive,
            lease,
        ),
        Err(TxPublishError::LeaseLost)
    );
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(buffer_offset), frame.len()),
            vec![0x5a; frame.len()]
        );
    }
    assert_eq!(sensitive.len(), 1);

    mmio.write_u32(producer.offset, 0);
    io.scrub_unpublished_sensitive_wmi_tx(
        GENERATION,
        Profile::Mission,
        &rings,
        &cursor,
        &mut sensitive,
        lease,
    )
    .unwrap();
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(buffer_offset), frame.len()),
            vec![0; frame.len()]
        );
    }
    assert!(sensitive.is_empty());
}

fn staged_htt_connect() -> HttStartupSession {
    let mut session = HttStartupSession::new(
        GENERATION,
        HtcReady {
            credit_count: 8,
            credit_size: 256,
            max_endpoints: 9,
            htc_version: 1,
            max_messages_per_bundle: 1,
            alternate_data_credit_size: 0,
        },
    )
    .unwrap();
    let mut output = [0u8; 16];
    assert_eq!(
        session.stage_connect_request(GENERATION, &mut output),
        Ok(16)
    );
    assert_eq!(session.phase(), HttStartupPhase::RequestStaged);
    session
}

fn connected_htt() -> HttStartupSession {
    let mut session = staged_htt_connect();
    session.request_tx_submitted(GENERATION).unwrap();
    let response = make_htt_connect_response(5, 512);
    session
        .accept_connect_response_frame(GENERATION, &response)
        .unwrap();
    session
}

fn staged_wmi_control_connect() -> SunfishWmiControlSession {
    let htt = connected_htt();
    let mut session =
        SunfishWmiControlSession::after_htt_connected(GENERATION, &WMI_CONTROL_READY, &htt)
            .unwrap();
    let mut output = [0u8; 20];
    assert_eq!(
        session.stage_connect_request(GENERATION, &mut output),
        Ok(16)
    );
    assert_eq!(session.phase(), WmiControlPhase::ConnectStaged);
    session
}

fn started_wmi_control() -> SunfishWmiControlSession {
    let htt = connected_htt();
    let mut session =
        SunfishWmiControlSession::after_htt_connected(GENERATION, &WMI_CONTROL_READY, &htt)
            .unwrap();
    let mut output = [0u8; 20];
    session
        .stage_connect_request(GENERATION, &mut output)
        .unwrap();
    session.control_tx_submitted(GENERATION).unwrap();
    let response = wmi_control_connect_response(7, 512);
    session
        .accept_connect_response_frame(GENERATION, &response)
        .unwrap();
    session
        .stage_setup_complete(GENERATION, &mut output)
        .unwrap();
    session.control_tx_submitted(GENERATION).unwrap();
    session
}

fn wmi_control_connect_response(endpoint: u8, max_message_size: u16) -> [u8; 20] {
    let max = max_message_size.to_le_bytes();
    [
        0, 0, 12, 0, 0, 0, 0, 0, // endpoint zero, 12-byte control payload
        3, 0, 0, 1, // CONNECT_SERVICE_RESPONSE, WMI_CONTROL_SVC
        0, endpoint, max[0], max[1], // success, assigned endpoint and size
        0, 0, 0, 0, // empty metadata and reserved bits
    ]
}

#[test]
fn native_htt_connect_request_publishes_only_on_gated_ce0_control_route() {
    let (mmio, dma, current, mapping, rings, ..) = setup();
    let frame_iova = DMA_IOVA + 0x3_0000;
    let mut session = staged_htt_connect();
    let frame: [u8; 16] = session
        .staged_connect_request_frame(GENERATION)
        .unwrap()
        .try_into()
        .unwrap();
    let mut cursor = RingCursor::new(16).unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));

    io.publish_htt_connect_request(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut session,
        frame_iova,
        &mut cursor,
    )
    .unwrap();

    assert_eq!(session.phase(), HttStartupPhase::AwaitResponse);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(cursor.pending(), 1);
    let packet_offset = (frame_iova - DMA_IOVA) as usize;
    // CE0 source ring is the first allocation in the profile.
    let descriptor_iova = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 0 && ring.direction == qcom_ce_v3_profile::RingDirection::Source
        })
        .unwrap()
        .descriptor_iova;
    let descriptor_offset = (descriptor_iova - DMA_IOVA) as usize;
    // SAFETY: both slices lie inside the mapped test DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(packet_offset), frame.len()),
            frame
        );
        let descriptor = core::slice::from_raw_parts(
            dma.base.as_ptr().add(descriptor_offset),
            qcom_ce_v3::DESCRIPTOR_BYTES,
        );
        assert_eq!(
            u32::from_le_bytes(descriptor[..4].try_into().unwrap()),
            frame_iova as u32
        );
        assert_eq!(
            u32::from_le_bytes(descriptor[8..12].try_into().unwrap()) as u16,
            16
        );
    }
    let producer = regs::register_spec(0, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 1);
}

#[test]
fn native_htt_connect_request_rejects_unvalidated_control_route_before_publication() {
    let (mmio, _dma, current, mapping, rings, ..) = setup();
    current.set(Observation {
        htc_control_host_to_target_engine: Some(3),
        ..current.get()
    });
    let mut session = staged_htt_connect();
    let mut cursor = RingCursor::new(16).unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));

    assert_eq!(
        io.publish_htt_connect_request(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut session,
            DMA_IOVA + 0x3_0000,
            &mut cursor,
        ),
        Err(TxPublishError::LeaseLost)
    );

    assert_eq!(session.phase(), HttStartupPhase::AwaitRequest);
    assert_eq!(cursor.producer_index(), 0);
    let producer = regs::register_spec(0, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
}

#[test]
fn native_ce2_control_response_binds_htt_endpoint_only_after_route_and_frame_validation() {
    let (mut mmio, dma, current, mapping, rings, buffer, ..) = setup();
    let mut session = staged_htt_connect();
    let mut tx_cursor = RingCursor::new(16).unwrap();
    let mut rx_cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let rx_plan = make_rx_plan(&rings, buffer);
    let mut io = make_io(mapping, Rc::clone(&current));

    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        rx_plan,
        &mut rx_cursor,
        &mut ledger,
    )
    .unwrap();
    io.publish_htt_connect_request(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut session,
        DMA_IOVA + 0x3_0000,
        &mut tx_cursor,
    )
    .unwrap();

    let response = make_htt_connect_response(5, 512);
    simulate_rx_completion(&dma, &mut mmio, rx_plan, &response);
    current.set(Observation {
        ce_quiescent: false,
        htc_control_target_to_host_engine: Some(1),
        ..current.get()
    });
    let mut output = [0xa5u8; 2048];
    assert_eq!(
        io.receive_htt_connect_response_ce2(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut rx_cursor,
            &mut ledger,
            &mut session,
            &mut output,
        ),
        Err(HttConnectRxError::Consume(RxConsumeError::LeaseLost))
    );
    assert_eq!(session.phase(), HttStartupPhase::AwaitResponse);
    assert_eq!(rx_cursor.pending(), 1);
    assert_eq!(ledger.len(), 1);

    current.set(Observation {
        htc_control_target_to_host_engine: Some(2),
        ..current.get()
    });
    let (completion, endpoint) = io
        .receive_htt_connect_response_ce2(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut rx_cursor,
            &mut ledger,
            &mut session,
            &mut output,
        )
        .unwrap();
    assert_eq!(completion.nbytes, response.len() as u16);
    assert_eq!(&output[..response.len()], &response);
    assert_eq!(endpoint.endpoint_id(), 5);
    assert_eq!(endpoint.max_message_size(), 512);
    assert_eq!(session.endpoint(), Some(endpoint));
    assert_eq!(session.phase(), HttStartupPhase::Connected);
    assert_eq!(rx_cursor.pending(), 0);
    assert!(ledger.is_empty());
}

#[test]
fn wmi_control_session_requires_prior_same_generation_htt_connection() {
    let not_connected = staged_htt_connect();
    assert_eq!(
        SunfishWmiControlSession::after_htt_connected(
            GENERATION,
            &WMI_CONTROL_READY,
            &not_connected,
        ),
        Err(WmiControlSessionError::HttNotConnected {
            observed: HttStartupPhase::RequestStaged,
        })
    );

    let connected = connected_htt();
    assert_eq!(
        SunfishWmiControlSession::after_htt_connected(
            GENERATION + 1,
            &WMI_CONTROL_READY,
            &connected,
        ),
        Err(WmiControlSessionError::WrongHttGeneration {
            expected: GENERATION,
            observed: GENERATION + 1,
        })
    );
    let mut different_ready = WMI_CONTROL_READY;
    different_ready[4] ^= 1;
    assert_eq!(
        SunfishWmiControlSession::after_htt_connected(GENERATION, &different_ready, &connected,),
        Err(WmiControlSessionError::ReadyPayloadMismatch)
    );

    let control =
        SunfishWmiControlSession::after_htt_connected(GENERATION, &WMI_CONTROL_READY, &connected)
            .unwrap();
    assert_eq!(control.htt_endpoint().endpoint_id(), 5);
}

#[test]
fn wmi_control_startup_uses_ready_credit_allocation_and_enforces_setup_order() {
    let htt = connected_htt();
    let mut session =
        SunfishWmiControlSession::after_htt_connected(GENERATION, &WMI_CONTROL_READY, &htt)
            .unwrap();
    let mut output = [0xa5u8; 20];
    assert_eq!(
        session.stage_connect_request(GENERATION + 1, &mut output),
        Err(WmiControlSessionError::WrongGeneration {
            expected: GENERATION,
            observed: GENERATION + 1,
        })
    );
    assert_eq!(output, [0xa5; 20]);

    assert_eq!(
        session.stage_connect_request(GENERATION, &mut output),
        Ok(16)
    );
    assert_eq!(
        &output[..16],
        &[0, 0, 8, 0, 0, 0, 0, 0, 2, 0, 0, 1, 0, 8, 0, 0]
    );
    assert_eq!(
        session.control_tx_failed(GENERATION),
        Ok(WmiControlPhase::Ready)
    );

    session
        .stage_connect_request(GENERATION, &mut output)
        .unwrap();
    assert_eq!(
        session.control_tx_submitted(GENERATION),
        Ok(WmiControlPhase::AwaitConnectResponse)
    );
    let response = wmi_control_connect_response(3, 512);
    let endpoint = session
        .accept_connect_response_frame(GENERATION, &response)
        .unwrap();
    assert_eq!(endpoint.endpoint_id(), 3);
    assert_eq!(endpoint.max_message_size(), 512);
    assert_eq!(session.phase(), WmiControlPhase::ServiceConnected);

    assert_eq!(
        session.stage_setup_complete(GENERATION, &mut output),
        Ok(20)
    );
    assert_eq!(
        &output,
        &[0, 0, 12, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        session.control_tx_submitted(GENERATION),
        Ok(WmiControlPhase::Started)
    );
    let credits = session.wmi_credit_ledger(GENERATION).unwrap();
    assert_eq!(credits.endpoint_id(), 3);
    assert_eq!(credits.available(), 8);
    assert_eq!(credits.credit_size(), 256);
}

#[test]
fn uncertain_wmi_control_publication_cannot_be_retried_in_same_generation() {
    let mut session = staged_wmi_control_connect();
    assert_eq!(session.control_tx_uncertain(GENERATION), Ok(()));
    assert_eq!(session.phase(), WmiControlPhase::SubmissionUncertain);
    assert!(matches!(
        session.staged_control_frame(GENERATION),
        Err(WmiControlSessionError::InvalidState { .. })
    ));
    assert!(matches!(
        session.stage_connect_request(GENERATION, &mut [0u8; 20]),
        Err(WmiControlSessionError::InvalidState { .. })
    ));
}

#[test]
fn native_wmi_control_connect_and_setup_complete_use_gated_ce0_and_ce2() {
    let (mut mmio, dma, current, mapping, rings, buffer, ..) = setup();
    let rx_plan = make_rx_plan(&rings, buffer);
    let mut rx_cursor = RingCursor::new(128).unwrap();
    let mut rx_ledger = RxBufferLedger::new();
    let mut control_cursor = RingCursor::new(16).unwrap();
    let mut session = staged_wmi_control_connect();
    let request: [u8; 16] = session
        .staged_control_frame(GENERATION)
        .unwrap()
        .try_into()
        .unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));

    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        rx_plan,
        &mut rx_cursor,
        &mut rx_ledger,
    )
    .unwrap();
    io.publish_wmi_control_connect_request(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut session,
        DMA_IOVA + 0x3_0000,
        &mut control_cursor,
    )
    .unwrap();
    assert_eq!(session.phase(), WmiControlPhase::AwaitConnectResponse);
    assert_eq!(control_cursor.producer_index(), 1);
    let request_offset = 0x3_0000;
    // SAFETY: staged request data is within the test DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(request_offset), request.len()),
            request
        );
    }

    let response = wmi_control_connect_response(4, 512);
    simulate_rx_completion(&dma, &mut mmio, rx_plan, &response);
    let mut receive = [0xa5u8; 2048];
    let (completion, endpoint) = io
        .receive_wmi_control_connect_response_ce2(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut rx_cursor,
            &mut rx_ledger,
            &mut session,
            &mut receive,
        )
        .unwrap();
    assert_eq!(completion.nbytes, response.len() as u16);
    assert_eq!(&receive[..response.len()], &response);
    assert_eq!(endpoint.endpoint_id(), 4);
    assert_eq!(session.phase(), WmiControlPhase::ServiceConnected);

    let mut setup_output = [0u8; 20];
    session
        .stage_setup_complete(GENERATION, &mut setup_output)
        .unwrap();
    let setup_frame = setup_output;
    io.publish_wmi_setup_complete(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut session,
        DMA_IOVA + 0x3_1000,
        &mut control_cursor,
    )
    .unwrap();
    assert_eq!(session.phase(), WmiControlPhase::Started);
    assert_eq!(control_cursor.producer_index(), 2);
    // SAFETY: setup-complete frame is in the same bounded test DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(0x3_1000), setup_frame.len()),
            setup_frame
        );
    }
    let producer = regs::register_spec(0, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 2);
    let credits = session.wmi_credit_ledger(GENERATION).unwrap();
    assert_eq!(credits.endpoint_id(), 4);
    assert_eq!(credits.available(), 8);
}

#[test]
fn native_startup_connects_htt_before_wmi_control_on_one_ce0_ce2_generation() {
    let (mut mmio, dma, current, mapping, rings, buffer, ..) = setup();
    let mut io = make_io(mapping, Rc::clone(&current));
    let mut rx_cursor = RingCursor::new(128).unwrap();
    let mut rx_ledger = RxBufferLedger::new();
    let mut control_cursor = RingCursor::new(16).unwrap();
    let mut htt = staged_htt_connect();

    let mut rx_plan = make_rx_plan(&rings, buffer);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        rx_plan,
        &mut rx_cursor,
        &mut rx_ledger,
    )
    .unwrap();
    io.publish_htt_connect_request(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut htt,
        DMA_IOVA + 0x3_0000,
        &mut control_cursor,
    )
    .unwrap();
    assert_eq!(htt.phase(), HttStartupPhase::AwaitResponse);
    assert_eq!(control_cursor.producer_index(), 1);

    let htt_response = make_htt_connect_response(5, 512);
    simulate_rx_completion(&dma, &mut mmio, rx_plan, &htt_response);
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    let mut receive = [0xa5u8; 2048];
    let (_, htt_endpoint) = io
        .receive_htt_connect_response_ce2(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut rx_cursor,
            &mut rx_ledger,
            &mut htt,
            &mut receive,
        )
        .unwrap();
    assert_eq!(htt_endpoint.endpoint_id(), 5);
    assert_eq!(htt.phase(), HttStartupPhase::Connected);

    let mut wmi =
        SunfishWmiControlSession::after_htt_connected(GENERATION, &WMI_CONTROL_READY, &htt)
            .unwrap();
    let mut staged = [0u8; 20];
    wmi.stage_connect_request(GENERATION, &mut staged).unwrap();
    let wmi_request: [u8; 16] = staged[..16].try_into().unwrap();

    rx_plan = plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap(),
        RxBufferWindow {
            iova: buffer.iova,
            capacity: 2048,
        },
        &rx_ledger,
        &rings,
        rx_cursor,
    )
    .unwrap();
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        rx_plan,
        &mut rx_cursor,
        &mut rx_ledger,
    )
    .unwrap();
    io.publish_wmi_control_connect_request(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut wmi,
        DMA_IOVA + 0x3_0100,
        &mut control_cursor,
    )
    .unwrap();
    assert_eq!(wmi.phase(), WmiControlPhase::AwaitConnectResponse);
    assert_eq!(control_cursor.producer_index(), 2);
    // SAFETY: this exact request range is inside the test's DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(0x3_0100), wmi_request.len()),
            wmi_request
        );
    }

    let wmi_response = wmi_control_connect_response(4, 512);
    simulate_rx_completion(&dma, &mut mmio, rx_plan, &wmi_response);
    let (_, wmi_endpoint) = io
        .receive_wmi_control_connect_response_ce2(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut rx_cursor,
            &mut rx_ledger,
            &mut wmi,
            &mut receive,
        )
        .unwrap();
    assert_eq!(wmi_endpoint.endpoint_id(), 4);
    assert_eq!(wmi.phase(), WmiControlPhase::ServiceConnected);

    wmi.stage_setup_complete(GENERATION, &mut staged).unwrap();
    io.publish_wmi_setup_complete(
        GENERATION,
        Profile::Mission,
        &rings,
        &mut wmi,
        DMA_IOVA + 0x3_0200,
        &mut control_cursor,
    )
    .unwrap();
    assert_eq!(wmi.phase(), WmiControlPhase::Started);
    assert_eq!(control_cursor.producer_index(), 3);
    assert_eq!(rx_cursor.consumer_index(), 2);
    assert!(rx_ledger.is_empty());
    assert_eq!(wmi.wmi_credit_ledger(GENERATION).unwrap().endpoint_id(), 4);
}

#[test]
fn qcacld_management_tx_flows_through_gated_ce3_publication_and_completion_lease() {
    let (mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let mut management_frame = [0u8; 30];
    management_frame[0] = 0xb0; // open-system authentication request
    management_frame[4..10].copy_from_slice(&[0x10, 0x21, 0x32, 0x43, 0x54, 0x65]);
    management_frame[10..16].copy_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    management_frame[16..22].copy_from_slice(&[0x10, 0x21, 0x32, 0x43, 0x54, 0x65]);
    management_frame[26..28].copy_from_slice(&1u16.to_le_bytes());
    let frame_iova = DMA_IOVA + 0x3_0000;
    let frame_offset = (frame_iova - DMA_IOVA) as usize;

    let request = MgmtTxRequest {
        vdev_id: 1,
        descriptor_id: 0x1234,
        channel_frequency_mhz: 2412,
        station: [0x02, 0x11, 0x22, 0x33, 0x44, 0x55],
        bssid: [0x10, 0x21, 0x32, 0x43, 0x54, 0x65],
        frame_iova,
        frame: &management_frame,
    };
    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let encoded = encode_wmi_mgmt_tx_frame(
        management_tx_capability(),
        GENERATION,
        pool,
        request,
        5,
        256,
        64,
        8,
    )
    .unwrap();
    let cursor = RingCursor::new(32).unwrap();
    let plan = plan_wmi_mgmt_tx_ce(
        &encoded,
        &route(),
        5,
        7,
        pool,
        buffer,
        Profile::Mission,
        &rings,
        cursor,
    )
    .unwrap();
    let command = encoded.frame().to_vec();
    let mut cursor = RingCursor::new(32).unwrap();
    let lease = MgmtTxLease {
        generation: GENERATION,
        descriptor_id: u32::from(request.descriptor_id),
        frame_iova,
        frame_bytes: management_frame.len() as u16,
    };
    let mut outstanding = MgmtTxOutstanding::<1>::new(GENERATION).unwrap();
    let mut htc_credits = HtcCreditLedger::new(GENERATION, 5, 8, 64, 256).unwrap();
    let mut io = make_io(mapping, current.clone());
    let authorized_observation = current.get();
    let mut revoked = authorized_observation;
    revoked.exclusive = false;
    current.set(revoked);
    assert_eq!(
        io.publish_qcacld_mgmt_tx(
            GENERATION,
            Profile::Mission,
            &rings,
            &encoded,
            plan,
            frame_iova,
            &management_frame,
            &mut outstanding,
            &mut htc_credits,
            &mut cursor,
        ),
        Err(TxPublishError::LeaseLost)
    );
    assert!(outstanding.is_empty());
    assert_eq!(htc_credits.available(), 8);
    assert_eq!(htc_credits.in_flight(), 0);
    assert_eq!(cursor.pending(), 0);
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    current.set(authorized_observation);

    io.publish_qcacld_mgmt_tx(
        GENERATION,
        Profile::Mission,
        &rings,
        &encoded,
        plan,
        frame_iova,
        &management_frame,
        &mut outstanding,
        &mut htc_credits,
        &mut cursor,
    )
    .unwrap();
    assert_eq!(outstanding.len(), 1);
    assert_eq!(htc_credits.available(), 6);
    assert_eq!(htc_credits.in_flight(), 2);

    let command_offset = (buffer.iova - DMA_IOVA) as usize;
    let descriptor_offset = (plan.descriptor_slot_iova() - DMA_IOVA) as usize;
    // SAFETY: these reads remain inside the backing mock DMA allocation.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(command_offset), command.len()),
            command
        );
        assert_eq!(
            core::slice::from_raw_parts(
                dma.base.as_ptr().add(frame_offset),
                management_frame.len()
            ),
            management_frame
        );
        assert_eq!(
            core::slice::from_raw_parts(
                dma.base.as_ptr().add(descriptor_offset),
                qcom_ce_v3::DESCRIPTOR_BYTES
            ),
            plan.descriptor().as_bytes()
        );
    }
    assert_eq!(mmio.read_u32(producer.offset), 1);
    assert_eq!(cursor.pending(), 1);

    let mut completion_event = [0u8; 44];
    completion_event[..4].copy_from_slice(&0x01a7_0028u32.to_le_bytes());
    completion_event[4..8].copy_from_slice(&u32::from(request.descriptor_id).to_le_bytes());
    let completion = parse_wmi_mgmt_tx_completion(&completion_event).unwrap();
    let retired = outstanding.complete(GENERATION, completion).unwrap();
    assert_eq!(retired.lease, lease);
    assert!(outstanding.is_empty());

    // QCACLD returns transmit credits in an HTC receive trailer, independent
    // of the WMI management completion that retires the frame DMA lease.
    let mut rx_frame = [0u8; 64];
    rx_frame[0] = 5;
    rx_frame[1] = qcom_htc::HTC_FLAG_RECV_TRAILER;
    rx_frame[2..4].copy_from_slice(&56u16.to_le_bytes());
    rx_frame[4] = 8;
    rx_frame[8..12]
        .copy_from_slice(&sunfish_qcacld_mgmt_tx::WMI_MGMT_TX_COMPLETION_EVENT_ID.to_le_bytes());
    rx_frame[12..56].copy_from_slice(&completion_event);
    rx_frame[56..64].copy_from_slice(&[qcom_htc::HTC_RECORD_CREDITS, 4, 0, 0, 5, 2, 0, 0]);
    let received = qcom_htc::parse_wmi_frame(&rx_frame, 5, 256).unwrap();
    assert_eq!(
        received.command_id,
        sunfish_qcacld_mgmt_tx::WMI_MGMT_TX_COMPLETION_EVENT_ID
    );
    assert_eq!(
        htc_credits
            .apply_reports(GENERATION, received.credit_reports())
            .unwrap(),
        2
    );
    assert_eq!(htc_credits.available(), 8);
    assert_eq!(htc_credits.in_flight(), 0);
}

#[test]
fn native_wmi_rx_post_zeros_buffer_copies_descriptor_and_publishes_ce2_index() {
    let (mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });

    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let buffer_offset = (buffer.iova - DMA_IOVA) as usize;
    let descriptor_offset = (plan.descriptor_slot_iova() - DMA_IOVA) as usize;
    // SAFETY: both slices remain within the live test DMA allocation.
    unsafe {
        assert!(
            core::slice::from_raw_parts(dma.base.as_ptr().add(buffer_offset), 2048)
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(
            core::slice::from_raw_parts(
                dma.base.as_ptr().add(descriptor_offset),
                qcom_ce_v3::DESCRIPTOR_BYTES
            ),
            plan.descriptor().as_bytes()
        );
    }
    let producer = regs::register_spec(2, Register::DestinationWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 1);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(cursor.pending(), 1);
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.active(), &[plan.buffer()]);
}

#[test]
fn native_wmi_rx_completion_copies_only_valid_bytes_and_retires_its_exact_slot() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let mut payload = FRAME_BYTES;
    payload[1] = 0; // Receive-direction bit 0 is not a credit-update request.
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, current);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    simulate_rx_completion(&dma, &mut mmio, plan, &payload);

    let mut output = [0xa5; 16];
    let completion = io
        .consume_wmi_rx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut ledger,
            &mut output,
        )
        .unwrap();

    assert_eq!(completion.address, plan.buffer().iova);
    assert_eq!(completion.nbytes as usize, payload.len());
    assert_eq!(completion.transfer_id, 7);
    assert_eq!(&output[..payload.len()], &payload);
    assert!(output[payload.len()..].iter().all(|byte| *byte == 0xa5));
    let parsed = qcom_htc::parse_wmi_frame(&output[..payload.len()], 1, 64).unwrap();
    assert_eq!(parsed.endpoint_id, 1);
    assert_eq!(parsed.command_id, 0x1234);
    assert_eq!(parsed.tlv_body, &[0x5a]);
    assert_eq!(cursor.consumer_index(), 1);
    assert_eq!(cursor.pending(), 0);
    assert!(ledger.is_empty());
}

#[test]
fn native_wmi_rx_completion_requires_hardware_progress_and_a_ready_descriptor() {
    let (mut mmio, _dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, current);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let mut output = [0xa5; 16];
    assert_eq!(
        io.consume_wmi_rx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut ledger,
            &mut output,
        ),
        Err(RxConsumeError::NotReady)
    );
    assert_eq!(cursor.consumer_index(), 0);
    assert_eq!(ledger.active(), &[plan.buffer()]);
    assert!(output.iter().all(|byte| *byte == 0xa5));

    let read = regs::register_spec(2, Register::CurrentDestinationReadIndex).unwrap();
    mmio.write_u32(read.offset, 1);
    assert_eq!(
        io.consume_wmi_rx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut ledger,
            &mut output,
        ),
        Err(RxConsumeError::Descriptor(qcom_ce_v3::Error::NotReady))
    );
    assert_eq!(cursor.consumer_index(), 0);
    assert_eq!(ledger.active(), &[plan.buffer()]);
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn native_wmi_rx_completion_rejects_short_output_without_retiring_ownership() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let payload = [1, 2, 3, 4, 5, 6, 7, 8];
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, current);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    simulate_rx_completion(&dma, &mut mmio, plan, &payload);

    let mut output = [0xa5; 4];
    assert_eq!(
        io.consume_wmi_rx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut ledger,
            &mut output,
        ),
        Err(RxConsumeError::OutputTooSmall)
    );
    assert_eq!(cursor.consumer_index(), 0);
    assert_eq!(cursor.pending(), 1);
    assert_eq!(ledger.active(), &[plan.buffer()]);
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn native_wmi_rx_post_allows_hardware_progress_within_unretired_window() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let first = make_rx_plan(&rings, buffer);
    let second_buffer = RxBufferWindow {
        iova: first.buffer().iova + 0x1000,
        capacity: 2048,
    };
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, current);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        first,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    simulate_rx_completion(&dma, &mut mmio, first, &[0x42]);
    let second = plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap(),
        second_buffer,
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();

    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        second,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    assert_eq!(cursor.consumer_index(), 0);
    assert_eq!(cursor.producer_index(), 2);
    assert_eq!(cursor.pending(), 2);
    assert_eq!(ledger.len(), 2);
    let producer = regs::register_spec(2, Register::DestinationWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 2);
}

#[test]
fn native_wmi_rx_completion_cannot_release_a_different_buffers_slot() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, current);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    simulate_rx_completion(&dma, &mut mmio, plan, &[1, 2, 3]);
    let descriptor_offset = (plan.descriptor_slot_iova() - DMA_IOVA) as usize;
    // SAFETY: the descriptor word lies inside the allocated fake DMA window.
    unsafe {
        dma.base
            .as_ptr()
            .add(descriptor_offset)
            .cast::<u32>()
            .write_volatile((plan.buffer().iova + 8) as u32);
    }

    let mut output = [0xa5; 16];
    assert_eq!(
        io.consume_wmi_rx(
            GENERATION,
            Profile::Mission,
            &rings,
            &mut cursor,
            &mut ledger,
            &mut output,
        ),
        Err(RxConsumeError::Descriptor(
            qcom_ce_v3::Error::AddressChanged
        ))
    );
    assert_eq!(cursor.consumer_index(), 0);
    assert_eq!(cursor.pending(), 1);
    assert_eq!(ledger.active(), &[plan.buffer()]);
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn native_wmi_rx_post_rejects_duplicate_live_buffer_before_any_new_write() {
    let (mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let first = make_rx_plan(&rings, buffer);
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, current);
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        first,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let duplicate = plan_wmi_rx_post(
        &route(),
        Profile::Mission,
        DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap(),
        first.buffer(),
        &ledger,
        &rings,
        cursor,
    );
    assert_eq!(duplicate, Err(qcom_wmi_ce::Error::ReceiveBufferOverlap));
    let producer = regs::register_spec(2, Register::DestinationWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 1);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(ledger.len(), 1);
    // SAFETY: all other bytes, including slot one and the original buffer,
    // remain within the live test DMA allocation.
    unsafe {
        let bytes = core::slice::from_raw_parts(dma.base.as_ptr(), DMA_BYTES);
        let next_descriptor = (rings.allocations()[2].descriptor_iova - DMA_IOVA) as usize + 16;
        assert!(
            bytes[next_descriptor..next_descriptor + 16]
                .iter()
                .all(|byte| *byte == 0)
        );
    }
}

#[test]
fn native_wmi_rx_irq_uses_only_the_live_ce2_binding_and_individual_enable_register() {
    let (mmio, _dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, Rc::clone(&current));
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let host_ie = regs::register_spec(2, Register::HostInterruptEnable).unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 0);
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, true)
        .unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 1);
    io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, false)
        .unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 0);
    assert_eq!(
        qcom_ce_v3_regs::RegisterIo::write32(&mut io, host_ie.offset, 1),
        Err(qcom_ce_v3_regs::Error::ReadOnly)
    );
    assert_eq!(mmio.read_u32(host_ie.offset), 0);
}

#[test]
fn native_wmi_rx_irq_requires_posted_buffers_and_an_exclusive_exact_binding() {
    let (mmio, _dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let mut cursor = RingCursor::new(128).unwrap();
    let mut io = make_io(mapping, Rc::clone(&current));
    assert_eq!(
        io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::NotReady)
    );
    let host_ie = regs::register_spec(2, Register::HostInterruptEnable).unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 0);

    let plan = make_rx_plan(&rings, buffer);
    let mut ledger = RxBufferLedger::new();
    io.post_wmi_rx(
        GENERATION,
        Profile::Mission,
        &rings,
        plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    current.set(Observation {
        ce_irq_intid: Some(0x01c1),
        ..current.get()
    });
    assert_eq!(
        io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::LeaseLost)
    );
    current.set(Observation {
        ce_irq_intid: Some(0x01c0),
        ce_irq_exclusive: false,
        ..current.get()
    });
    assert_eq!(
        io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::LeaseLost)
    );
    current.set(Observation {
        ce_irq_exclusive: true,
        ce_irq_bound: false,
        ..current.get()
    });
    assert_eq!(
        io.set_wmi_rx_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(mmio.read_u32(host_ie.offset), 0);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn native_htt_ce1_rx_irq_requires_live_route_endpoint_and_ce1_binding() {
    let (mut mmio, _dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    current.set(Observation {
        htt_map_generation: GENERATION,
        htt_host_to_target_engine: Some(4),
        htt_target_to_host_engine: Some(1),
        htt_endpoint_generation: GENERATION,
        htt_endpoint_id: Some(7),
        htt_endpoint_max_message_size: 2048,
        ..current.get()
    });

    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let rx_ring = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 1 && ring.direction == qcom_ce_v3_profile::RingDirection::Destination
        })
        .unwrap();
    let mut cursor = RingCursor::new(usize::from(rx_ring.entries)).unwrap();
    let mut ledger = RxBufferLedger::new();
    let rx_plan = plan_service_rx_post(
        &htt_route(),
        HTT_DATA_SERVICE_ID,
        WmiRoute {
            host_to_target_engine: 4,
            target_to_host_engine: 1,
        },
        Profile::Mission,
        pool,
        RxBufferWindow {
            iova: buffer.iova,
            capacity: 2048,
        },
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();

    let read_index = regs::register_spec(1, Register::CurrentDestinationReadIndex).unwrap();
    let producer_index = regs::register_spec(1, Register::DestinationWriteIndex).unwrap();
    let size = regs::register_spec(1, Register::DestinationSize).unwrap();
    let base_low = regs::register_spec(1, Register::DestinationBaseLow).unwrap();
    let base_high = regs::register_spec(1, Register::DestinationBaseHigh).unwrap();
    mmio.write_u32(read_index.offset, 0);
    mmio.write_u32(producer_index.offset, 0);
    mmio.write_u32(size.offset, u32::from(rx_ring.entries));
    mmio.write_u32(base_low.offset, rx_ring.descriptor_iova as u32);
    mmio.write_u32(base_high.offset, (rx_ring.descriptor_iova >> 32) as u32);

    let mut io = make_io(mapping, Rc::clone(&current));
    io.post_htt_rx_ce1(
        GENERATION,
        Profile::Mission,
        &rings,
        rx_plan,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let host_ie = regs::register_spec(1, Register::HostInterruptEnable).unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 0);
    io.set_htt_rx_ce1_irq(GENERATION, Profile::Mission, &rings, &cursor, true)
        .unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 1);
    io.set_htt_rx_ce1_irq(GENERATION, Profile::Mission, &rings, &cursor, false)
        .unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 0);

    current.set(Observation {
        htt_ce_irq_intid: Some(0x01c0),
        ..current.get()
    });
    assert_eq!(
        io.set_htt_rx_ce1_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(mmio.read_u32(host_ie.offset), 0);

    current.set(Observation {
        htt_ce_irq_intid: Some(0x01bf),
        htt_ce_irq_bound: false,
        ..current.get()
    });
    assert_eq!(
        io.set_htt_rx_ce1_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(mmio.read_u32(host_ie.offset), 0);

    current.set(Observation {
        htt_ce_irq_bound: true,
        htt_map_generation: GENERATION + 1,
        ..current.get()
    });
    assert_eq!(
        io.set_htt_rx_ce1_irq(GENERATION, Profile::Mission, &rings, &cursor, true),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(mmio.read_u32(host_ie.offset), 0);
}

#[test]
fn native_htt_ce1_irq_dispatches_sec_ind_acknowledges_and_rearms_posted_buffers() {
    let (mut mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    current.set(Observation {
        htt_map_generation: GENERATION,
        htt_host_to_target_engine: Some(4),
        htt_target_to_host_engine: Some(1),
        htt_endpoint_generation: GENERATION,
        htt_endpoint_id: Some(5),
        htt_endpoint_max_message_size: 512,
        ..current.get()
    });

    let pool = DmaPool::new(DMA_IOVA, DMA_BYTES as u64).unwrap();
    let rx_ring = rings
        .allocations()
        .iter()
        .find(|ring| {
            ring.engine == 1 && ring.direction == qcom_ce_v3_profile::RingDirection::Destination
        })
        .unwrap();
    let mut cursor = RingCursor::new(usize::from(rx_ring.entries)).unwrap();
    let mut ledger = RxBufferLedger::new();
    let read_index = regs::register_spec(1, Register::CurrentDestinationReadIndex).unwrap();
    let producer_index = regs::register_spec(1, Register::DestinationWriteIndex).unwrap();
    let size = regs::register_spec(1, Register::DestinationSize).unwrap();
    let base_low = regs::register_spec(1, Register::DestinationBaseLow).unwrap();
    let base_high = regs::register_spec(1, Register::DestinationBaseHigh).unwrap();
    mmio.write_u32(read_index.offset, 0);
    mmio.write_u32(producer_index.offset, 0);
    mmio.write_u32(size.offset, u32::from(rx_ring.entries));
    mmio.write_u32(base_low.offset, rx_ring.descriptor_iova as u32);
    mmio.write_u32(base_high.offset, (rx_ring.descriptor_iova >> 32) as u32);

    let mut io = make_io(mapping, Rc::clone(&current));
    let route = WmiRoute {
        host_to_target_engine: 4,
        target_to_host_engine: 1,
    };
    let first = plan_service_rx_post(
        &htt_route(),
        HTT_DATA_SERVICE_ID,
        route,
        Profile::Mission,
        pool,
        RxBufferWindow {
            iova: buffer.iova,
            capacity: 2048,
        },
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();
    io.post_htt_rx_ce1(
        GENERATION,
        Profile::Mission,
        &rings,
        first,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();
    let second = plan_service_rx_post(
        &htt_route(),
        HTT_DATA_SERVICE_ID,
        route,
        Profile::Mission,
        pool,
        RxBufferWindow {
            iova: buffer.iova + 0x1000,
            capacity: 2048,
        },
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();
    io.post_htt_rx_ce1(
        GENERATION,
        Profile::Mission,
        &rings,
        second,
        &mut cursor,
        &mut ledger,
    )
    .unwrap();

    let mut payload = [0u8; 28];
    payload[0] = 0x0b;
    payload[1] = 0x86;
    payload[2..4].copy_from_slice(&0x1234u16.to_le_bytes());
    let endpoint = connected_htt().endpoint().unwrap();
    let mut frame = [0u8; 64];
    let frame_len = sunfish_qcacld_htc::encode_data_frame(endpoint, 0x71, &payload, &mut frame)
        .unwrap()
        .frame_len;
    let descriptor_offset = (first.descriptor_slot_iova() - DMA_IOVA) as usize;
    let payload_offset = (first.buffer().iova - DMA_IOVA) as usize;
    // SAFETY: these ranges are in the test DMA allocation; this simulates one
    // completed device write and preserves the posted descriptor address.
    unsafe {
        for (index, byte) in frame[..frame_len].iter().copied().enumerate() {
            dma.base
                .as_ptr()
                .add(payload_offset + index)
                .write_volatile(byte);
        }
        dma.base
            .as_ptr()
            .add(descriptor_offset + 8)
            .cast::<u32>()
            .write_volatile(frame_len as u32 | (5 << 16));
    }
    mmio.write_u32(read_index.offset, first.next_producer_index() as u32);
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });
    io.set_htt_rx_ce1_irq(GENERATION, Profile::Mission, &rings, &cursor, true)
        .unwrap();
    let host_ie = regs::register_spec(1, Register::HostInterruptEnable).unwrap();
    let host_is = regs::register_spec(1, Register::HostInterruptStatus).unwrap();
    assert_eq!(mmio.read_u32(host_ie.offset), 1);
    mmio.write_u32(host_is.offset, regs::HOST_INTERRUPT_COPY_COMPLETE_MASK);

    let security = Cell::new(None);
    let sequence = Cell::new(None);
    let mut scratch = [0u8; 512];
    let report = service_native_htt_ce1_irq(
        &mut io,
        GENERATION,
        Profile::Mission,
        &rings,
        &mut cursor,
        &mut ledger,
        endpoint,
        &mut scratch,
        4,
        |completed: &CompletedEvent<'_>| {
            sequence.set(Some(completed.decoded.sequence));
            if let Event::SecurityIndication(indication) = completed.decoded.event {
                security.set(Some(indication));
            }
            Ok::<_, ()>(())
        },
    )
    .unwrap();

    assert_eq!(sequence.get(), Some(0x71));
    assert_eq!(
        security.get(),
        Some(ath10k_htt::HttSecurityIndication {
            peer_id: 0x1234,
            security_type: 6,
            is_unicast: true,
        })
    );
    assert_eq!(report.completions, 1);
    assert!(report.irq_acknowledged);
    assert!(!report.needs_deferred_drain);
    assert_eq!(cursor.pending(), 1);
    assert_eq!(ledger.len(), 1);
    assert_eq!(mmio.read_u32(host_ie.offset), 1);
}

#[test]
fn native_wmi_ce2_irq_ack_clears_only_observed_copy_complete_status() {
    let (mut mmio, _dma, current, mapping, _rings, _buffer, _frame, _wmi, _tx) = setup();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        ce_quiescent: false,
        ..current.get()
    });

    let host_is = regs::register_spec(2, Register::HostInterruptStatus).unwrap();
    mmio.write_u32(host_is.offset, 0x11);
    io.acknowledge_wmi_rx_ce2_copy_complete(GENERATION).unwrap();
    assert_eq!(
        mmio.read_u32(host_is.offset),
        regs::HOST_INTERRUPT_COPY_COMPLETE_MASK
    );

    mmio.write_u32(host_is.offset, 0);
    assert_eq!(
        io.acknowledge_wmi_rx_ce2_copy_complete(GENERATION),
        Err(RxIrqError::Register(regs::Error::StatusNotPending))
    );
    assert_eq!(mmio.read_u32(host_is.offset), 0);

    mmio.write_u32(host_is.offset, regs::HOST_INTERRUPT_COPY_COMPLETE_MASK);
    current.set(Observation {
        ce_irq_intid: Some(0x01c1),
        ..current.get()
    });
    assert_eq!(
        io.acknowledge_wmi_rx_ce2_copy_complete(GENERATION),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(
        mmio.read_u32(host_is.offset),
        regs::HOST_INTERRUPT_COPY_COMPLETE_MASK
    );

    current.set(Observation {
        ce_irq_intid: Some(0x01c0),
        ce_irq_exclusive: false,
        ..current.get()
    });
    assert_eq!(
        io.acknowledge_wmi_rx_ce2_copy_complete(GENERATION),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(
        mmio.read_u32(host_is.offset),
        regs::HOST_INTERRUPT_COPY_COMPLETE_MASK
    );

    current.set(Observation {
        ce_irq_exclusive: true,
        wmi_map_generation: GENERATION + 1,
        ..current.get()
    });
    assert_eq!(
        io.acknowledge_wmi_rx_ce2_copy_complete(GENERATION),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(
        mmio.read_u32(host_is.offset),
        regs::HOST_INTERRUPT_COPY_COMPLETE_MASK
    );
}

#[test]
fn native_htt_ce1_irq_ack_clears_only_observed_copy_complete_status() {
    let (mut mmio, _dma, current, mapping, _rings, _buffer, _frame, _wmi, _tx) = setup();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        htt_map_generation: GENERATION,
        htt_host_to_target_engine: Some(4),
        htt_target_to_host_engine: Some(1),
        htt_endpoint_generation: GENERATION,
        htt_endpoint_id: Some(7),
        htt_endpoint_max_message_size: 2048,
        ce_quiescent: false,
        ..current.get()
    });

    let host_is = regs::register_spec(1, Register::HostInterruptStatus).unwrap();
    // Include an unrelated watermark bit: the adapter must issue only the
    // W1C mask for CE1 copy-complete after observing it pending.
    mmio.write_u32(host_is.offset, 0x11);
    io.acknowledge_htt_rx_ce1_copy_complete(GENERATION).unwrap();
    assert_eq!(
        mmio.read_u32(host_is.offset),
        regs::HOST_INTERRUPT_COPY_COMPLETE_MASK
    );

    mmio.write_u32(host_is.offset, 0);
    assert_eq!(
        io.acknowledge_htt_rx_ce1_copy_complete(GENERATION),
        Err(RxIrqError::Register(regs::Error::StatusNotPending))
    );
    assert_eq!(mmio.read_u32(host_is.offset), 0);

    mmio.write_u32(host_is.offset, regs::HOST_INTERRUPT_COPY_COMPLETE_MASK);
    current.set(Observation {
        htt_ce_irq_intid: Some(0x01c0),
        ..current.get()
    });
    assert_eq!(
        io.acknowledge_htt_rx_ce1_copy_complete(GENERATION),
        Err(RxIrqError::LeaseLost)
    );
    assert_eq!(
        mmio.read_u32(host_is.offset),
        regs::HOST_INTERRUPT_COPY_COMPLETE_MASK
    );
}

#[test]
fn native_wmi_rx_post_requires_live_lifecycle_and_route_evidence() {
    let (mmio, dma, current, mapping, rings, buffer, _frame, _wmi, _tx) = setup();
    let plan = make_rx_plan(&rings, buffer);
    let mut cursor = RingCursor::new(128).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        wmi_target_to_host_engine: Some(3),
        ..current.get()
    });
    assert_eq!(
        io.post_wmi_rx(
            GENERATION,
            Profile::Mission,
            &rings,
            plan,
            &mut cursor,
            &mut ledger
        ),
        Err(RxPostError::LeaseLost)
    );
    let producer = regs::register_spec(2, Register::DestinationWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert_eq!(cursor.producer_index(), 0);
    assert!(ledger.is_empty());
    // SAFETY: test allocation is initialized to zero and remains live.
    assert!(unsafe {
        core::slice::from_raw_parts(dma.base.as_ptr(), DMA_BYTES)
            .iter()
            .all(|byte| *byte == 0)
    });
}

#[test]
fn lifecycle_loss_prevents_all_dma_and_register_writes() {
    let (mmio, dma, current, mapping, rings, _buffer, _frame, _wmi, tx) = setup();
    let mut io = make_io(mapping, Rc::clone(&current));
    current.set(Observation {
        ce_powered: false,
        ..current.get()
    });
    let mut cursor = RingCursor::new(32).unwrap();
    assert_eq!(
        io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor),
        Err(TxPublishError::LeaseLost)
    );
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert_eq!(cursor.producer_index(), 0);
    // SAFETY: test allocation is initialized to zero and remains live.
    assert!(unsafe {
        core::slice::from_raw_parts(dma.base.as_ptr(), DMA_BYTES)
            .iter()
            .all(|byte| *byte == 0)
    });
}

#[test]
fn native_publication_requires_wmi_routes_attested_for_the_same_generation() {
    let (mmio, dma, current, mapping, rings, _buffer, _frame, _wmi, tx) = setup();
    let mut io = make_io(mapping, Rc::clone(&current));
    let mut cursor = RingCursor::new(32).unwrap();
    current.set(Observation {
        wmi_map_generation: GENERATION + 1,
        ..current.get()
    });
    assert_eq!(
        io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor),
        Err(TxPublishError::LeaseLost)
    );
    current.set(Observation {
        wmi_map_generation: GENERATION,
        wmi_host_to_target_engine: Some(2),
        ..current.get()
    });
    assert_eq!(
        io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor),
        Err(TxPublishError::LeaseLost)
    );
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert_eq!(cursor.producer_index(), 0);
    // SAFETY: test allocation is initialized to zero and remains live.
    assert!(unsafe {
        core::slice::from_raw_parts(dma.base.as_ptr(), DMA_BYTES)
            .iter()
            .all(|byte| *byte == 0)
    });
}

#[test]
fn stale_ring_indices_fail_before_dma_copy_or_publication() {
    let (mut mmio, dma, current, mapping, rings, _buffer, _frame, _wmi, tx) = setup();
    let read = regs::register_spec(3, Register::CurrentSourceReadIndex).unwrap();
    mmio.write_u32(read.offset, 1);
    let mut io = make_io(mapping, current);
    let mut cursor = RingCursor::new(32).unwrap();
    assert_eq!(
        io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor),
        Err(TxPublishError::LeaseLost)
    );
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert_eq!(cursor.producer_index(), 0);
    // SAFETY: test allocation is initialized to zero and remains live.
    assert!(unsafe {
        core::slice::from_raw_parts(dma.base.as_ptr(), DMA_BYTES)
            .iter()
            .all(|byte| *byte == 0)
    });
}

#[test]
fn stale_plan_is_rejected_before_dma_copy_or_publication() {
    let (mmio, dma, current, mapping, rings, _buffer, _frame, _wmi, tx) = setup();
    let mut io = make_io(mapping, current);
    let mut cursor = RingCursor::new(32).unwrap();
    let stale = cursor.prepare_next().unwrap();
    cursor.commit_publish(stale).unwrap();
    assert_eq!(
        io.publish_wmi_tx(GENERATION, Profile::Mission, &rings, tx, &mut cursor),
        Err(TxPublishError::InvalidPlan)
    );
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(mmio.read_u32(producer.offset), 0);
    assert_eq!(cursor.producer_index(), 1);
    assert_eq!(cursor.pending(), 1);
    // SAFETY: test allocation is initialized to zero and remains live.
    assert!(unsafe {
        core::slice::from_raw_parts(dma.base.as_ptr(), DMA_BYTES)
            .iter()
            .all(|byte| *byte == 0)
    });
}

#[test]
fn producer_index_remains_blocked_from_generic_register_api() {
    let (mmio, _dma, current, mapping, _rings, _buffer, _frame, _wmi, _tx) = setup();
    let mut io = make_io(mapping, current);
    let producer = regs::register_spec(3, Register::SourceWriteIndex).unwrap();
    assert_eq!(
        regs::RegisterIo::write32(&mut io, producer.offset, 1),
        Err(regs::Error::ReadOnly)
    );
    assert_eq!(mmio.read_u32(producer.offset), 0);
    let destination = regs::register_spec(2, Register::DestinationWriteIndex).unwrap();
    assert_eq!(
        regs::RegisterIo::write32(&mut io, destination.offset, 1),
        Err(regs::Error::ReadOnly)
    );
    assert_eq!(mmio.read_u32(destination.offset), 0);
}
