#![allow(dead_code)]
#![forbid(unsafe_code)]

mod qcom_ufs_platform {
    pub const SMMU_STREAM_ID: u16 = 0x300;
}

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
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
#[path = "../rust/src/sunfish_wlan_dma.rs"]
mod sunfish_wlan_dma;

use qcom_ce_v3::RingCursor;
use qcom_ce_v3_profile::{plan_rings, Profile, RingPlan, PAGE_BYTES};
use qcom_htc::encode_wmi_command_frame;
use qcom_wmi_ce::{
    plan_wmi_rx_post, plan_wmi_tx, Direction, DmaPool, RxBufferLedger, ServiceCeMapping,
    WMI_CONTROL_SERVICE_ID,
};
use sunfish_smmu::{plan_ufs_route, plan_wlan_alias_route, RoutePlan};
use sunfish_smmu_lpae::{
    plan_lpae_s1, LpaePageMapping, LpaeS1Config, LpaeTableBuffers, LpaeTablePages,
    RingMemoryAttribute, TableWalkPolicy, LPAE_ENTRIES_PER_TABLE,
};
use sunfish_wlan_dma::{
    plan_wlan_ce_dma_translation, WlanDmaPlanError, LPAE_LEVEL2_BYTES, MAX_WMI_TX_BUFFERS,
    WLAN_CONTROL_DMA_POOL_BYTES, WMI_CONTROL_BUFFER_BYTES,
};

const RING_IOVA_BASE: u64 = 0x8000_0000;
const TABLE_PAGES: LpaeTablePages = LpaeTablePages {
    ttbr0_root: 0x9000_0000,
    level2: 0x9000_1000,
    level3: 0x9000_2000,
    ttbr1_root: 0x9000_3000,
};
const CONFIG: LpaeS1Config = LpaeS1Config {
    input_address_bits: 39,
    output_address_bits: 40,
    asid: 2,
    table_walk_policy: TableWalkPolicy::NonCoherent,
    ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
    arm_ns_quirk: false,
};

fn route() -> RoutePlan {
    plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap()
}

fn rings(base_iova: u64) -> RingPlan {
    plan_rings(Profile::Mission, base_iova, 0x2_0000).unwrap()
}

fn wmi_service_map() -> [ServiceCeMapping; 2] {
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

fn physical_pages() -> Vec<u64> {
    let count = (WLAN_CONTROL_DMA_POOL_BYTES / PAGE_BYTES) as usize;
    (0..count)
        .map(|index| 0xa000_0000 + index as u64 * PAGE_BYTES)
        .collect()
}

fn walk(tables: &[[u64; LPAE_ENTRIES_PER_TABLE]; 4], iova: u64) -> Option<u64> {
    let l1 = tables[0][((iova >> 30) & 0x1ff) as usize];
    if l1 & 0x3 != 0x3 {
        return None;
    }
    let l2 = tables[1][((iova >> 21) & 0x1ff) as usize];
    if l2 & 0x3 != 0x3 {
        return None;
    }
    let l3 = tables[2][((iova >> 12) & 0x1ff) as usize];
    if l3 & 0x3 != 0x3 {
        return None;
    }
    Some((l3 & 0x0000_ffff_ffff_f000) | (iova & (PAGE_BYTES - 1)))
}

#[test]
fn mission_ce_rings_compose_with_the_exact_wlan_alias_route_and_lpae_map() {
    let ring_plan = rings(RING_IOVA_BASE);
    let physical_pages = physical_pages();
    let plan = plan_wlan_ce_dma_translation(
        route(),
        ring_plan,
        WLAN_CONTROL_DMA_POOL_BYTES,
        &physical_pages,
        TABLE_PAGES,
        CONFIG,
    )
    .unwrap();

    assert_eq!(plan.route().matched_sids(), [0x240, 0x241]);
    assert_eq!((plan.stream_match_slot(), plan.context_bank()), (6, 4));
    assert_eq!(plan.rings().total_bytes(), 101_192);
    assert_eq!(plan.mappings().len(), 104);
    assert_eq!(plan.pool_bytes(), WLAN_CONTROL_DMA_POOL_BYTES);
    assert_eq!(plan.rx_buffers().len(), 127);
    assert_eq!(plan.tx_buffers().len(), MAX_WMI_TX_BUFFERS);
    assert_eq!(plan.lpae().descriptor_writes().len(), 106);
    assert_eq!(
        plan.context_registers().ttbr0,
        TABLE_PAGES.ttbr0_root | (2u64 << 48)
    );

    let covered_end = RING_IOVA_BASE + plan.mappings().len() as u64 * PAGE_BYTES;
    for allocation in plan.rings().allocations() {
        let allocation_end = allocation.descriptor_iova + u64::from(allocation.allocation_bytes);
        assert!(allocation.descriptor_iova >= RING_IOVA_BASE);
        assert!(allocation_end <= covered_end);
    }
    for (index, mapping) in plan.mappings().iter().enumerate() {
        assert_eq!(
            mapping.iova_page,
            RING_IOVA_BASE + index as u64 * PAGE_BYTES
        );
        assert_eq!(mapping.physical_page, physical_pages[index]);
    }
    for (index, buffer) in plan.rx_buffers().iter().enumerate() {
        assert_eq!(buffer.capacity, WMI_CONTROL_BUFFER_BYTES);
        assert!(buffer.iova >= covered_end - plan.pool_bytes() + plan.rings().total_bytes());
        assert!(buffer.iova + u64::from(buffer.capacity) <= covered_end);
        assert_eq!(buffer.iova & 7, 0);
        if index > 0 {
            assert_eq!(
                buffer.iova,
                plan.rx_buffers()[index - 1].iova + u64::from(WMI_CONTROL_BUFFER_BYTES)
            );
        }
    }
    for (index, buffer) in plan.tx_buffers().iter().enumerate() {
        assert_eq!(buffer.capacity, WMI_CONTROL_BUFFER_BYTES);
        assert!(buffer.iova >= plan.rx_buffers().last().unwrap().iova + 2048);
        assert!(buffer.iova + u64::from(buffer.capacity) <= covered_end);
        assert_eq!(buffer.iova & 7, 0);
        if index > 0 {
            assert_eq!(
                buffer.iova,
                plan.tx_buffers()[index - 1].iova + u64::from(WMI_CONTROL_BUFFER_BYTES)
            );
        }
    }

    let mut root = [0u64; LPAE_ENTRIES_PER_TABLE];
    let mut level2 = [0u64; LPAE_ENTRIES_PER_TABLE];
    let mut level3 = [0u64; LPAE_ENTRIES_PER_TABLE];
    let mut ttbr1 = [0u64; LPAE_ENTRIES_PER_TABLE];
    let mut buffers = LpaeTableBuffers {
        physical_pages: TABLE_PAGES,
        ttbr0_root: &mut root,
        level2: &mut level2,
        level3: &mut level3,
        ttbr1_root: &mut ttbr1,
    };
    plan.lpae().materialize_table_image(&mut buffers).unwrap();
    let tables = [root, level2, level3, ttbr1];
    for (index, mapping) in plan.mappings().iter().enumerate() {
        assert_eq!(
            walk(&tables, mapping.iova_page),
            Some(physical_pages[index])
        );
    }
    assert_eq!(walk(&tables, covered_end), None);

    let service_map = wmi_service_map();
    let dma_pool = DmaPool::new(RING_IOVA_BASE, plan.pool_bytes()).unwrap();
    let mut rx_ledger = RxBufferLedger::new();
    let mut rx_cursor = RingCursor::new(128).unwrap();
    for buffer in plan.rx_buffers().iter().copied() {
        let post = plan_wmi_rx_post(
            &service_map,
            Profile::Mission,
            dma_pool,
            buffer,
            &rx_ledger,
            plan.rings(),
            rx_cursor,
        )
        .unwrap();
        assert_eq!(post.descriptor().address(), buffer.iova);
        rx_cursor.commit_publish(post.ticket()).unwrap();
        rx_ledger.reserve(buffer, post.ticket()).unwrap();
    }
    assert_eq!(rx_ledger.len(), 127);
    assert_eq!(rx_cursor.pending(), 127);

    let mut frame_bytes = [0xa5; 64];
    let wmi = encode_wmi_command_frame(1, 256, 64, 4, 0x1234, &[0x5a], &mut frame_bytes).unwrap();
    let tx = plan_wmi_tx(
        &service_map,
        &frame_bytes[..wmi.frame_len],
        wmi,
        1,
        7,
        dma_pool,
        plan.tx_buffers()[0],
        Profile::Mission,
        plan.rings(),
        RingCursor::new(32).unwrap(),
    )
    .unwrap();
    assert_eq!(tx.descriptor().address(), plan.tx_buffers()[0].iova);
}

#[test]
fn mission_no_pktlog_profile_also_fits_the_bounded_wlan_translation_plan() {
    let ring_plan = plan_rings(Profile::MissionNoPktlog, RING_IOVA_BASE, 0x2_0000).unwrap();
    let physical_pages = physical_pages();
    let plan = plan_wlan_ce_dma_translation(
        route(),
        ring_plan,
        WLAN_CONTROL_DMA_POOL_BYTES,
        &physical_pages,
        TABLE_PAGES,
        CONFIG,
    )
    .unwrap();
    assert_eq!(plan.mappings().len(), 104);
    assert_eq!(plan.rx_buffers().len(), 63);
    assert_eq!(plan.tx_buffers().len(), 31);
    assert_eq!(
        plan.lpae().descriptor_writes().len(),
        plan.mappings().len() + 2
    );
    for (index, mapping) in plan.mappings().iter().enumerate() {
        assert_eq!(mapping.physical_page, physical_pages[index]);
        assert_eq!(
            mapping.iova_page,
            RING_IOVA_BASE + index as u64 * PAGE_BYTES
        );
    }
    let service_map = wmi_service_map();
    let dma_pool = DmaPool::new(RING_IOVA_BASE, plan.pool_bytes()).unwrap();
    let mut ledger = RxBufferLedger::new();
    let mut cursor = RingCursor::new(64).unwrap();
    for buffer in plan.rx_buffers().iter().copied() {
        let post = plan_wmi_rx_post(
            &service_map,
            Profile::MissionNoPktlog,
            dma_pool,
            buffer,
            &ledger,
            plan.rings(),
            cursor,
        )
        .unwrap();
        cursor.commit_publish(post.ticket()).unwrap();
        ledger.reserve(buffer, post.ticket()).unwrap();
    }
    assert_eq!(ledger.len(), 63);
    assert_eq!(cursor.pending(), 63);
}

#[test]
fn rejects_wrong_route_iova_alignment_and_physical_page_count() {
    let ring_plan = rings(RING_IOVA_BASE);
    let physical_pages = physical_pages();
    let ufs_route = plan_ufs_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    assert_eq!(
        plan_wlan_ce_dma_translation(
            ufs_route,
            ring_plan,
            WLAN_CONTROL_DMA_POOL_BYTES,
            &physical_pages,
            TABLE_PAGES,
            CONFIG,
        ),
        Err(WlanDmaPlanError::RouteNotWlanAlias)
    );
    assert_eq!(
        plan_wlan_ce_dma_translation(
            route(),
            rings(RING_IOVA_BASE + PAGE_BYTES),
            WLAN_CONTROL_DMA_POOL_BYTES,
            &physical_pages,
            TABLE_PAGES,
            CONFIG,
        ),
        Err(WlanDmaPlanError::RingPoolNotLevel2Aligned)
    );
    assert_eq!(
        plan_wlan_ce_dma_translation(
            route(),
            ring_plan,
            0x2_0000,
            &physical_pages[..32],
            TABLE_PAGES,
            CONFIG,
        ),
        Err(WlanDmaPlanError::ControlPoolTooSmall)
    );
    assert_eq!(
        plan_wlan_ce_dma_translation(
            route(),
            ring_plan,
            WLAN_CONTROL_DMA_POOL_BYTES,
            &physical_pages[..physical_pages.len() - 1],
            TABLE_PAGES,
            CONFIG,
        ),
        Err(WlanDmaPlanError::PhysicalPageCountMismatch)
    );
    assert!(RING_IOVA_BASE.is_multiple_of(LPAE_LEVEL2_BYTES));
}

#[test]
fn rejects_duplicate_physical_frames_before_materializing_any_table() {
    let ring_plan = rings(RING_IOVA_BASE);
    let mut physical_pages = physical_pages();
    physical_pages[1] = physical_pages[0];
    assert!(matches!(
        plan_wlan_ce_dma_translation(
            route(),
            ring_plan,
            WLAN_CONTROL_DMA_POOL_BYTES,
            &physical_pages,
            TABLE_PAGES,
            CONFIG,
        ),
        Err(WlanDmaPlanError::Lpae(
            sunfish_smmu_lpae::LpaePlanError::DuplicateDmaPage
        ))
    ));
}

#[test]
fn generic_lpae_mapping_limit_is_explicit_and_fails_closed() {
    let mappings: [LpaePageMapping; 129] = core::array::from_fn(|index| LpaePageMapping {
        iova_page: RING_IOVA_BASE + index as u64 * PAGE_BYTES,
        physical_page: 0xa000_0000 + index as u64 * PAGE_BYTES,
    });
    assert_eq!(
        plan_lpae_s1(&mappings, TABLE_PAGES, CONFIG),
        Err(sunfish_smmu_lpae::LpaePlanError::TooManyDmaPages)
    );
}
