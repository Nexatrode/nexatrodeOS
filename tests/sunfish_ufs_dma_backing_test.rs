#[path = "../rust/src/qcom_ufs_dma.rs"]
#[allow(dead_code)]
mod qcom_ufs_dma;
#[path = "../rust/src/qcom_ufs_platform.rs"]
#[allow(dead_code)]
mod qcom_ufs_platform;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
#[allow(dead_code)]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_smmu.rs"]
#[allow(dead_code)]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
#[allow(dead_code)]
mod sunfish_smmu_lpae;

use qcom_ufs_dma::{
    fill_ufs_dma_backing_mappings, plan_ufs_dma_from_backing_pool, plan_ufs_dma_from_profile_pools,
    Error,
};
use qcom_ufs_platform::{
    DmaAperture, DmaBackingPool, UfsSmmuTablePool, DMA_PAGE_BYTES, UFS_DMA_POOL_BYTES,
    UFS_DMA_POOL_VIRTUAL_BASE, UFS_SMMU_TABLE_POOL_BYTES, UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
};
use sunfish_smmu::{plan_ufs_route, plan_wlan_alias_route};
use sunfish_smmu_lpae::{
    LpaeMultiTablePages, LpaePageMapping, LpaeS1Config, RingMemoryAttribute, TableWalkPolicy,
};

const POOL_PHYSICAL_BASE: u64 = 0x8035_d000;
const IOVA_BASE: u64 = 0x9000_0000;

fn pool() -> DmaBackingPool {
    DmaBackingPool::from_profile(
        UFS_DMA_POOL_VIRTUAL_BASE,
        POOL_PHYSICAL_BASE,
        UFS_DMA_POOL_BYTES,
    )
    .unwrap()
}

fn aperture(base: u64, bytes: u64, exclusive: bool) -> DmaAperture {
    DmaAperture {
        base,
        bytes,
        exclusive,
    }
}

#[test]
fn maps_every_caller_iova_to_the_matching_pool_frame() {
    let pool = pool();
    let aperture = aperture(IOVA_BASE, UFS_DMA_POOL_BYTES, true);
    let mut mappings = vec![
        LpaePageMapping {
            iova_page: 0,
            physical_page: 0,
        };
        pool.page_count()
    ];

    fill_ufs_dma_backing_mappings(pool, aperture, &mut mappings).unwrap();

    assert_eq!(mappings.len(), 4096);
    assert_eq!(
        mappings[0],
        LpaePageMapping {
            iova_page: IOVA_BASE,
            physical_page: POOL_PHYSICAL_BASE,
        }
    );
    assert_eq!(
        mappings[mappings.len() - 1],
        LpaePageMapping {
            iova_page: IOVA_BASE + UFS_DMA_POOL_BYTES - DMA_PAGE_BYTES,
            physical_page: POOL_PHYSICAL_BASE + UFS_DMA_POOL_BYTES - DMA_PAGE_BYTES,
        }
    );
    for pair in mappings.windows(2) {
        assert_eq!(pair[1].iova_page - pair[0].iova_page, DMA_PAGE_BYTES);
        assert_eq!(
            pair[1].physical_page - pair[0].physical_page,
            DMA_PAGE_BYTES
        );
    }
}

#[test]
fn refuses_unproven_or_mismatched_iova_windows() {
    let pool = pool();
    let mut mappings = vec![
        LpaePageMapping {
            iova_page: 0,
            physical_page: 0,
        };
        pool.page_count()
    ];

    assert_eq!(
        fill_ufs_dma_backing_mappings(
            pool,
            aperture(IOVA_BASE, UFS_DMA_POOL_BYTES, false),
            &mut mappings,
        ),
        Err(Error::InvalidAperture)
    );
    assert_eq!(
        fill_ufs_dma_backing_mappings(
            pool,
            aperture(IOVA_BASE + 1, UFS_DMA_POOL_BYTES, true),
            &mut mappings,
        ),
        Err(Error::InvalidAperture)
    );
    assert_eq!(
        fill_ufs_dma_backing_mappings(
            pool,
            aperture(IOVA_BASE, UFS_DMA_POOL_BYTES - DMA_PAGE_BYTES, true),
            &mut mappings,
        ),
        Err(Error::MappingCountMismatch)
    );
    let short_len = mappings.len() - 1;
    assert_eq!(
        fill_ufs_dma_backing_mappings(
            pool,
            aperture(IOVA_BASE, UFS_DMA_POOL_BYTES, true),
            &mut mappings[..short_len],
        ),
        Err(Error::MappingCountMismatch)
    );
}

#[test]
fn rejects_iova_wraparound_before_writing_any_mapping() {
    let pool = pool();
    let mut mappings = vec![
        LpaePageMapping {
            iova_page: 0x55,
            physical_page: 0xaa,
        };
        pool.page_count()
    ];

    assert_eq!(
        fill_ufs_dma_backing_mappings(
            pool,
            aperture(u64::MAX & !(DMA_PAGE_BYTES - 1), UFS_DMA_POOL_BYTES, true),
            &mut mappings,
        ),
        Err(Error::InvalidAperture)
    );
    assert_eq!(mappings[0].iova_page, 0x55);
    assert_eq!(mappings[0].physical_page, 0xaa);
}

#[test]
fn composes_the_pool_map_with_an_exact_ufs_route_and_full_lpae_plan() {
    let pool = pool();
    let aperture = aperture(0x9000_0000, UFS_DMA_POOL_BYTES, true);
    let route = plan_ufs_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    let table_pool = UfsSmmuTablePool::from_profile(
        UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
        0x8200_0000,
        UFS_SMMU_TABLE_POOL_BYTES,
    )
    .unwrap();
    let config = LpaeS1Config {
        input_address_bits: 39,
        output_address_bits: 40,
        asid: 5,
        table_walk_policy: TableWalkPolicy::NonCoherent,
        ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
        arm_ns_quirk: false,
    };
    let mut mappings = vec![
        LpaePageMapping {
            iova_page: 0,
            physical_page: 0,
        };
        pool.page_count()
    ];

    let plan =
        plan_ufs_dma_from_profile_pools(route, pool, aperture, &mut mappings, &table_pool, config)
            .unwrap();

    assert_eq!(plan.route().id(), 0x300);
    assert_eq!(plan.stream_match_slot(), 6);
    assert_eq!(plan.context_bank(), 4);
    assert_eq!(plan.mappings().len(), 4096);
    assert_eq!(plan.lpae().mapping_count(), 4096);
    assert_eq!(plan.mappings()[0].iova_page, aperture.base);
    assert_eq!(plan.mappings()[0].physical_page, pool.physical_base());
    assert_eq!(
        plan.mappings()[4095].iova_page,
        aperture.base + UFS_DMA_POOL_BYTES - DMA_PAGE_BYTES
    );
    assert_eq!(
        plan.mappings()[4095].physical_page,
        pool.physical_base() + UFS_DMA_POOL_BYTES - DMA_PAGE_BYTES
    );
}

#[test]
fn invalid_route_does_not_fill_the_pool_mapping_buffer() {
    let pool = pool();
    let aperture = aperture(IOVA_BASE, UFS_DMA_POOL_BYTES, true);
    let route = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    let level3 = [0x8200_2000];
    let table_pages = LpaeMultiTablePages {
        ttbr0_root: 0x8200_0000,
        level2: 0x8200_1000,
        level3: &level3,
        ttbr1_root: 0x8200_3000,
    };
    let config = LpaeS1Config {
        input_address_bits: 39,
        output_address_bits: 40,
        asid: 5,
        table_walk_policy: TableWalkPolicy::NonCoherent,
        ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
        arm_ns_quirk: false,
    };
    let mut mappings = vec![
        LpaePageMapping {
            iova_page: 0x55,
            physical_page: 0xaa,
        };
        pool.page_count()
    ];

    assert_eq!(
        plan_ufs_dma_from_backing_pool(route, pool, aperture, &mut mappings, table_pages, config,),
        Err(Error::RouteNotUfs)
    );
    assert_eq!(mappings[0].iova_page, 0x55);
    assert_eq!(mappings[0].physical_page, 0xaa);
}

#[test]
fn table_frames_cannot_alias_dma_backing_pages() {
    let pool = pool();
    let aperture = aperture(IOVA_BASE, UFS_DMA_POOL_BYTES, true);
    let route = plan_ufs_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    let table_pool = UfsSmmuTablePool::from_profile(
        UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
        POOL_PHYSICAL_BASE + DMA_PAGE_BYTES,
        UFS_SMMU_TABLE_POOL_BYTES,
    )
    .unwrap();
    let config = LpaeS1Config {
        input_address_bits: 39,
        output_address_bits: 40,
        asid: 5,
        table_walk_policy: TableWalkPolicy::NonCoherent,
        ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
        arm_ns_quirk: false,
    };
    let mut mappings = vec![
        LpaePageMapping {
            iova_page: 0x55,
            physical_page: 0xaa,
        };
        pool.page_count()
    ];

    assert_eq!(
        plan_ufs_dma_from_profile_pools(route, pool, aperture, &mut mappings, &table_pool, config,),
        Err(Error::TablePoolOverlapsDmaPool)
    );
    assert_eq!(mappings[0].iova_page, 0x55);
    assert_eq!(mappings[0].physical_page, 0xaa);
}
