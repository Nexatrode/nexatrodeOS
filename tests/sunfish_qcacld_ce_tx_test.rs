#![allow(dead_code)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/qcom_wmi_ce.rs"]
mod qcom_wmi_ce;
#[path = "../rust/src/sunfish_qcacld_ce_tx.rs"]
mod sunfish_qcacld_ce_tx;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_tx.rs"]
mod sunfish_qcacld_tx;

use qcom_ce_v3::RingCursor;
use qcom_ce_v3_profile::{Profile, plan_rings};
use qcom_wmi_ce::{Direction, DmaPool, ServiceCeMapping, TxBufferWindow};
use sunfish_qcacld_ce_tx::{
    CE4_DATA_DESCRIPTOR_COUNT, CeTxBuffers, Error, HTC_HTT_PREFIX_BYTES,
    QCACLD_NATIVE_WIFI_DOWNLOAD_BYTES, plan_native_wifi_tx,
};
use sunfish_qcacld_htc::{HTT_DATA_SERVICE_ID, parse_connect_response};
use sunfish_qcacld_tx::{HELIUMPLUS_FRAGMENT_DESC_BYTES, TxMetadata};

const POOL_BASE: u64 = 0x8000_0000;
const POOL_BYTES: u64 = 0x0100_0000;

fn data_frame(length: usize) -> Vec<u8> {
    assert!(length >= 24);
    let mut frame = vec![0u8; length];
    frame[0..2].copy_from_slice(&0x0008u16.to_le_bytes());
    frame
}

fn endpoint() -> sunfish_qcacld_htc::HttEndpoint {
    let endpoint_id = 5u8;
    let max_message_size = 512u16;
    let encoded = (u32::from(endpoint_id) << 8) | (u32::from(max_message_size) << 16);
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

fn mappings() -> [ServiceCeMapping; 2] {
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

fn setup() -> (
    DmaPool,
    qcom_ce_v3_profile::RingPlan,
    CeTxBuffers,
    TxMetadata,
) {
    let dma_pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
    let rings = plan_rings(Profile::Mission, POOL_BASE, POOL_BYTES).unwrap();
    let data_base = POOL_BASE + rings.total_bytes().div_ceil(4096) * 4096 + 4096;
    let prefix_iova = data_base;
    let table_iova = data_base + 0x1000;
    let frame_iova = data_base + 0x2000;
    (
        dma_pool,
        rings,
        CeTxBuffers {
            htc_htt_prefix: TxBufferWindow {
                iova: prefix_iova,
                capacity: 64,
            },
        },
        TxMetadata {
            fragment_table_iova: table_iova,
            frame_iova,
            msdu_id: 3,
            vdev_id: 1,
            ext_tid: 0,
            hardware_encrypt: false,
            request_completion: false,
        },
    )
}

#[test]
fn plan_encodes_source_matched_two_entry_ce4_gather_packet() {
    let (dma_pool, rings, buffers, metadata) = setup();
    let cursor = RingCursor::new(2048).unwrap();
    let frame = data_frame(400);
    let plan = plan_native_wifi_tx(
        &mappings(),
        endpoint(),
        0x7a,
        &frame,
        metadata,
        buffers,
        dma_pool,
        Profile::Mission,
        &rings,
        cursor,
    )
    .unwrap();

    assert_eq!(plan.route().host_to_target_engine, 4);
    assert_eq!(plan.route().target_to_host_engine, 1);
    assert_eq!(plan.endpoint_id(), 5);
    assert_eq!(plan.sequence(), 0x7a);
    assert_eq!(
        plan.total_download_bytes(),
        QCACLD_NATIVE_WIFI_DOWNLOAD_BYTES as u16
    );
    assert_eq!(plan.frame_download_bytes(), 64);
    assert_eq!(plan.next_producer_index(), 2);
    let ce4_ring = rings
        .allocations()
        .iter()
        .find(|allocation| {
            allocation.engine == 4
                && allocation.direction == qcom_ce_v3_profile::RingDirection::Source
        })
        .unwrap();
    assert_eq!(plan.first_slot_iova(), ce4_ring.descriptor_iova);
    assert_eq!(plan.second_slot_iova(), plan.first_slot_iova() + 16);
    assert_eq!(CE4_DATA_DESCRIPTOR_COUNT, 2);

    let first = plan.first_descriptor();
    assert_eq!(first.address(), buffers.htc_htt_prefix.iova);
    assert_eq!(first.nbytes(), HTC_HTT_PREFIX_BYTES as u16);
    assert_eq!(first.transfer_id(), 5);
    assert_ne!(
        u32::from_le_bytes(first.as_bytes()[4..8].try_into().unwrap()) & (1 << 31),
        0
    );

    let second = plan.second_descriptor();
    assert_eq!(second.address(), metadata.frame_iova);
    assert_eq!(second.nbytes(), 64);
    assert_eq!(second.transfer_id(), 5);
    assert_eq!(
        u32::from_le_bytes(second.as_bytes()[4..8].try_into().unwrap()) & (1 << 31),
        0
    );

    let prefix = plan.htc_htt_prefix();
    assert_eq!(&prefix[..8], &[5, 0, 84, 0, 0, 0x7a, 0, 0]);
    assert_eq!(
        plan.fragment_descriptor().len(),
        HELIUMPLUS_FRAGMENT_DESC_BYTES
    );
    assert_eq!(
        plan.fragment_descriptor()[24..28],
        (metadata.frame_iova as u32).to_le_bytes()
    );
}

#[test]
fn source_download_length_is_derived_and_clipped_to_short_packets() {
    let (dma_pool, rings, buffers, metadata) = setup();
    let cursor = RingCursor::new(2048).unwrap();
    let frame = data_frame(100);
    let plan = plan_native_wifi_tx(
        &mappings(),
        endpoint(),
        1,
        &frame,
        metadata,
        buffers,
        dma_pool,
        Profile::Mission,
        &rings,
        cursor,
    )
    .unwrap();
    assert_eq!(
        plan.total_download_bytes(),
        QCACLD_NATIVE_WIFI_DOWNLOAD_BYTES as u16
    );
    assert_eq!(plan.frame_download_bytes(), 64);
    assert_eq!(plan.second_descriptor().nbytes(), 64);

    let short_frame = data_frame(60);
    let short = plan_native_wifi_tx(
        &mappings(),
        endpoint(),
        1,
        &short_frame,
        metadata,
        buffers,
        dma_pool,
        Profile::Mission,
        &rings,
        cursor,
    )
    .unwrap();
    assert_eq!(
        short.total_download_bytes(),
        (HTC_HTT_PREFIX_BYTES + short_frame.len()) as u16
    );
    assert_eq!(short.frame_download_bytes(), short_frame.len() as u16);
}

#[test]
fn route_and_batch_capacity_fail_closed_before_a_plan_is_returned() {
    let (dma_pool, rings, buffers, metadata) = setup();
    let frame = data_frame(80);
    let cursor = RingCursor::new(2048).unwrap();
    let wrong = [
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::HostToTarget,
            engine: 3,
        },
        mappings()[1],
    ];
    assert_eq!(
        plan_native_wifi_tx(
            &wrong,
            endpoint(),
            0,
            &frame,
            metadata,
            buffers,
            dma_pool,
            Profile::Mission,
            &rings,
            cursor,
        ),
        Err(Error::WrongRoute)
    );

    let mut full = RingCursor::new(2048).unwrap();
    for _ in 0..2046 {
        let ticket = full.prepare_next().unwrap();
        full.commit_publish(ticket).unwrap();
    }
    assert_eq!(
        plan_native_wifi_tx(
            &mappings(),
            endpoint(),
            0,
            &frame,
            metadata,
            buffers,
            dma_pool,
            Profile::Mission,
            &rings,
            full,
        ),
        Err(Error::Cursor(qcom_ce_v3::Error::RingFull))
    );
}

#[test]
fn plans_reject_overlapping_mapped_regions() {
    let (dma_pool, rings, _buffers, metadata) = setup();
    let frame = data_frame(80);
    let overlapping = CeTxBuffers {
        htc_htt_prefix: TxBufferWindow {
            iova: metadata.frame_iova,
            capacity: 64,
        },
    };
    assert_eq!(
        plan_native_wifi_tx(
            &mappings(),
            endpoint(),
            0,
            &frame,
            metadata,
            overlapping,
            dma_pool,
            Profile::Mission,
            &rings,
            RingCursor::new(2048).unwrap(),
        ),
        Err(Error::BufferOverlap)
    );
}
