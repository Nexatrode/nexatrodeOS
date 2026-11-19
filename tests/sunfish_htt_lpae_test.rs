#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;

use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_smmu_lpae::{
    LPAE_ENTRIES_PER_TABLE, LpaeMultiTableBuffers, LpaeMultiTablePages, LpaePageMapping,
    LpaePlanError, LpaeS1Config, RingMemoryAttribute, SMMU_LPAE_ADDRESS_MASK, SMMU_LPAE_PAGE_BYTES,
    TableWalkPolicy, plan_lpae_s1_multi,
};

const CONFIG: LpaeS1Config = LpaeS1Config {
    input_address_bits: 39,
    output_address_bits: 40,
    asid: 4,
    table_walk_policy: TableWalkPolicy::NonCoherent,
    ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
    arm_ns_quirk: false,
};

const ROOT: u64 = 0x9000_0000;
const LEVEL2: u64 = 0x9000_1000;
const LEVEL3_START: u64 = 0x9000_2000;
const TTBR1: u64 = 0x9000_a000;
const IOVA_BASE: u64 = 0x8000_0000;

fn table_pages(level3: &[u64]) -> LpaeMultiTablePages<'_> {
    LpaeMultiTablePages {
        ttbr0_root: ROOT,
        level2: LEVEL2,
        level3,
        ttbr1_root: TTBR1,
    }
}

fn sequential_table_pages(count: usize) -> Vec<u64> {
    (0..count)
        .map(|index| LEVEL3_START + index as u64 * SMMU_LPAE_PAGE_BYTES)
        .collect()
}

fn resolve_page(
    root: &[u64],
    level2: &[u64],
    level3: &[u64],
    table_pages: &[u64],
    iova: u64,
) -> Option<u64> {
    let l1_descriptor = *root.get(((iova >> 30) & 0x1ff) as usize)?;
    if l1_descriptor & 0x3 != 0x3 {
        return None;
    }
    let l2_descriptor = *level2.get(((iova >> 21) & 0x1ff) as usize)?;
    if l2_descriptor & 0x3 != 0x3 {
        return None;
    }
    let l3_page = l2_descriptor & SMMU_LPAE_ADDRESS_MASK;
    let l3_table_index = table_pages.iter().position(|page| *page == l3_page)?;
    let l3_index = ((iova >> 12) & 0x1ff) as usize;
    let descriptor = *level3.get(l3_table_index * LPAE_ENTRIES_PER_TABLE + l3_index)?;
    if descriptor & 0x3 != 0x3 {
        return None;
    }
    Some((descriptor & SMMU_LPAE_ADDRESS_MASK) | (iova & (SMMU_LPAE_PAGE_BYTES - 1)))
}

#[test]
fn maps_the_source_sized_htt_startup_pool_across_three_leaf_tables() {
    let htt = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let page_count = htt.minimum_packed_page_count().unwrap() as usize;
    assert_eq!(page_count, 1092);
    let mappings: Vec<_> = (0..page_count)
        .map(|index| LpaePageMapping {
            iova_page: IOVA_BASE + index as u64 * SMMU_LPAE_PAGE_BYTES,
            // Model distinct, scattered frames rather than assuming physical
            // contiguity for the target's receive buffers.
            physical_page: 0xa000_0000 + index as u64 * 2 * SMMU_LPAE_PAGE_BYTES,
        })
        .collect();
    let l3_pages = sequential_table_pages(3);
    let plan = plan_lpae_s1_multi(&mappings, table_pages(&l3_pages), CONFIG).unwrap();

    assert_eq!(plan.mapping_count(), page_count);
    assert_eq!(plan.level3_table_count(), 3);
    assert_eq!(plan.descriptor_count(), page_count + 4);
    assert_eq!(plan.context().ttbr0, ROOT | (4u64 << 48));

    let mut root = [u64::MAX; LPAE_ENTRIES_PER_TABLE];
    let mut level2 = [u64::MAX; LPAE_ENTRIES_PER_TABLE];
    let mut level3 = vec![u64::MAX; 3 * LPAE_ENTRIES_PER_TABLE];
    let mut ttbr1 = [u64::MAX; LPAE_ENTRIES_PER_TABLE];
    let receipt = plan
        .materialize_table_image(&mut LpaeMultiTableBuffers {
            physical_pages: table_pages(&l3_pages),
            ttbr0_root: &mut root,
            level2: &mut level2,
            level3: &mut level3,
            ttbr1_root: &mut ttbr1,
        })
        .unwrap();

    assert_eq!(receipt.written_descriptors, (page_count + 4) as u16);
    assert_eq!(root[2], LEVEL2 | 3);
    assert_eq!(level2[0], l3_pages[0] | 3);
    assert_eq!(level2[1], l3_pages[1] | 3);
    assert_eq!(level2[2], l3_pages[2] | 3);
    assert!(ttbr1.iter().all(|entry| *entry == 0));
    for mapping in mappings.iter().copied() {
        assert_eq!(
            resolve_page(&root, &level2, &level3, &l3_pages, mapping.iova_page),
            Some(mapping.physical_page)
        );
        assert_eq!(
            resolve_page(
                &root,
                &level2,
                &level3,
                &l3_pages,
                mapping.iova_page + SMMU_LPAE_PAGE_BYTES - 1
            ),
            Some(mapping.physical_page + SMMU_LPAE_PAGE_BYTES - 1)
        );
    }
    assert_eq!(
        resolve_page(
            &root,
            &level2,
            &level3,
            &l3_pages,
            IOVA_BASE + page_count as u64 * SMMU_LPAE_PAGE_BYTES
        ),
        None
    );
}

#[test]
fn maps_sorted_sparse_iova_extents_without_authorizing_unmapped_gaps() {
    let mappings = [
        LpaePageMapping {
            iova_page: IOVA_BASE,
            physical_page: 0xa100_0000,
        },
        LpaePageMapping {
            iova_page: IOVA_BASE + 3 * SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xa100_2000,
        },
        LpaePageMapping {
            iova_page: IOVA_BASE + 2 * 1024 * 1024 + SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xa100_4000,
        },
    ];
    let l3_pages = sequential_table_pages(2);
    let plan = plan_lpae_s1_multi(&mappings, table_pages(&l3_pages), CONFIG).unwrap();
    let mut root = [0; LPAE_ENTRIES_PER_TABLE];
    let mut level2 = [0; LPAE_ENTRIES_PER_TABLE];
    let mut level3 = vec![0; 2 * LPAE_ENTRIES_PER_TABLE];
    let mut ttbr1 = [0; LPAE_ENTRIES_PER_TABLE];
    plan.materialize_table_image(&mut LpaeMultiTableBuffers {
        physical_pages: table_pages(&l3_pages),
        ttbr0_root: &mut root,
        level2: &mut level2,
        level3: &mut level3,
        ttbr1_root: &mut ttbr1,
    })
    .unwrap();

    for mapping in mappings {
        assert_eq!(
            resolve_page(&root, &level2, &level3, &l3_pages, mapping.iova_page),
            Some(mapping.physical_page)
        );
    }
    assert_eq!(
        resolve_page(
            &root,
            &level2,
            &level3,
            &l3_pages,
            IOVA_BASE + SMMU_LPAE_PAGE_BYTES
        ),
        None
    );
}

#[test]
fn rejects_unsorted_duplicate_and_cross_root_mappings() {
    let l3_pages = sequential_table_pages(2);
    let unsorted = [
        LpaePageMapping {
            iova_page: IOVA_BASE + SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xa200_0000,
        },
        LpaePageMapping {
            iova_page: IOVA_BASE,
            physical_page: 0xa200_1000,
        },
    ];
    assert_eq!(
        plan_lpae_s1_multi(&unsorted, table_pages(&l3_pages), CONFIG),
        Err(LpaePlanError::IovaPagesNotSorted)
    );

    let duplicate = [unsorted[0], unsorted[0]];
    assert_eq!(
        plan_lpae_s1_multi(&duplicate, table_pages(&l3_pages), CONFIG),
        Err(LpaePlanError::DuplicateIovaPage)
    );

    let cross_root = [
        LpaePageMapping {
            iova_page: 0x3fff_f000,
            physical_page: 0xa200_0000,
        },
        LpaePageMapping {
            iova_page: 0x4000_0000,
            physical_page: 0xa200_1000,
        },
    ];
    assert_eq!(
        plan_lpae_s1_multi(&cross_root, table_pages(&l3_pages), CONFIG),
        Err(LpaePlanError::MappingSpansMultipleL1Entries)
    );
}

#[test]
fn rejects_table_count_aliases_and_bad_buffer_lengths_before_writing() {
    let mappings = [LpaePageMapping {
        iova_page: IOVA_BASE,
        physical_page: 0xa300_0000,
    }];
    let no_l3_pages = [];
    assert_eq!(
        plan_lpae_s1_multi(&mappings, table_pages(&no_l3_pages), CONFIG),
        Err(LpaePlanError::Level3TableCountMismatch)
    );

    let duplicate_l3_pages = [LEVEL3_START, LEVEL3_START];
    let two_l2_mappings = [
        mappings[0],
        LpaePageMapping {
            iova_page: IOVA_BASE + 512 * SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xa300_1000,
        },
    ];
    assert_eq!(
        plan_lpae_s1_multi(&two_l2_mappings, table_pages(&duplicate_l3_pages), CONFIG),
        Err(LpaePlanError::DuplicateTablePage)
    );

    let l3_pages = sequential_table_pages(1);
    let plan = plan_lpae_s1_multi(&mappings, table_pages(&l3_pages), CONFIG).unwrap();
    let mut root = [u64::MAX; LPAE_ENTRIES_PER_TABLE];
    let mut level2 = [u64::MAX; LPAE_ENTRIES_PER_TABLE];
    let mut short_level3 = [u64::MAX; LPAE_ENTRIES_PER_TABLE - 1];
    let mut ttbr1 = [u64::MAX; LPAE_ENTRIES_PER_TABLE];
    assert_eq!(
        plan.materialize_table_image(&mut LpaeMultiTableBuffers {
            physical_pages: table_pages(&l3_pages),
            ttbr0_root: &mut root,
            level2: &mut level2,
            level3: &mut short_level3,
            ttbr1_root: &mut ttbr1,
        }),
        Err(LpaePlanError::InvalidTableBufferLength)
    );
    assert!(root.iter().all(|entry| *entry == u64::MAX));
    assert!(level2.iter().all(|entry| *entry == u64::MAX));
    assert!(short_level3.iter().all(|entry| *entry == u64::MAX));
    assert!(ttbr1.iter().all(|entry| *entry == u64::MAX));
}

#[test]
fn enforces_the_leaf_table_and_mapping_capacity_bounds() {
    let mappings: Vec<_> = (0..9)
        .map(|index| LpaePageMapping {
            iova_page: IOVA_BASE + index as u64 * 2 * 1024 * 1024,
            physical_page: 0xa400_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
        })
        .collect();
    let l3_pages = sequential_table_pages(9);
    assert_eq!(
        plan_lpae_s1_multi(&mappings, table_pages(&l3_pages), CONFIG),
        Err(LpaePlanError::TooManyLevel3Tables)
    );

    let too_many: Vec<_> = (0..=sunfish_smmu_lpae::MAX_MULTI_LPAE_PAGE_MAPPINGS)
        .map(|index| LpaePageMapping {
            iova_page: IOVA_BASE + index as u64 * SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xb000_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
        })
        .collect();
    let pages = sequential_table_pages(9);
    assert_eq!(
        plan_lpae_s1_multi(&too_many, table_pages(&pages), CONFIG),
        Err(LpaePlanError::TooManyDmaPages)
    );
}
