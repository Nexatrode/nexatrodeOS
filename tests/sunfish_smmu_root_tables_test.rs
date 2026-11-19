#![allow(dead_code)]
#![forbid(unsafe_code)]
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
use sunfish_smmu_lpae::*;

fn pages() -> LpaeRootTablePages {
    LpaeRootTablePages {
        ttbr0_root: 0x90000000,
        level2: [0x90001000, 0x90002000],
        level3: [0x90003000, 0x90004000, 0x90005000],
        ttbr1_root: 0x90006000,
    }
}
fn config() -> LpaeS1Config {
    LpaeS1Config {
        input_address_bits: 39,
        output_address_bits: 40,
        asid: 4,
        table_walk_policy: TableWalkPolicy::NonCoherent,
        ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
        arm_ns_quirk: false,
    }
}
fn mappings() -> [LpaePageMapping; 37] {
    core::array::from_fn(|i| LpaePageMapping {
        iova_page: if i < 2 {
            0x146a8000 + i as u64 * 4096
        } else if i < 34 {
            0x80000000 + (i - 2) as u64 * 4096
        } else {
            0x8611d000 + (i - 34) as u64 * 4096
        },
        physical_page: 0x88000000 + i as u64 * 4096,
    })
}

#[test]
fn per_page_root_encoder_matches_full_seven_table_image() {
    let maps = mappings();
    let plan = plan_lpae_s1_roots(&maps, pages(), config()).unwrap();
    let mut full = [[0u64; 512]; 7];
    plan.materialize_table_image(pages(), &mut full).unwrap();
    for (index, expected) in full.iter().enumerate() {
        let mut actual = [0xa5u64; 512];
        plan.materialize_table_page(pages(), index, &mut actual).unwrap();
        assert_eq!(&actual, expected, "table {index}");
    }
}
// Independent architectural walk: follow physical descriptor links rather
// than using the planner's root/leaf grouping or storage indexes.
fn walk(image: &[[u64; 512]; 7], pages: LpaeRootTablePages, iova: u64) -> Option<u64> {
    let identities = pages.pages();
    let mut table = 0;
    for shift in [30, 21] {
        let descriptor = image[table][((iova >> shift) & 511) as usize];
        if descriptor & 3 != 3 {
            return None;
        }
        table = identities
            .iter()
            .position(|page| *page == descriptor & SMMU_LPAE_ADDRESS_MASK)?;
    }
    let descriptor = image[table][((iova >> 12) & 511) as usize];
    (descriptor & 3 == 3).then_some(descriptor)
}
#[test]
fn seven_table_image_walks_every_private_page_and_leaves_all_holes_invalid() {
    let maps = mappings();
    let plan = plan_lpae_s1_roots(&maps, pages(), config()).unwrap();
    assert_eq!(plan.root_indices(), [0, 2]);
    assert_eq!(plan.descriptor_count(), 42);
    let mut image = [[0xa5; 512]; 7];
    let receipt = plan.materialize_table_image(pages(), &mut image).unwrap();
    assert_eq!(receipt.written_descriptors, 42);
    for mapping in &maps {
        let value = walk(&image, pages(), mapping.iova_page).unwrap();
        assert_eq!(value & SMMU_LPAE_ADDRESS_MASK, mapping.physical_page);
        assert_eq!(value & ((3 << 53) | 0xfff), (3 << 53) | 0xe43);
    }
    for hole in [
        0, 0x146a7000, 0x146aa000, 0x40000000, 0x80020000, 0x8611c000, 0x86120000,
    ] {
        assert_eq!(walk(&image, pages(), hole), None);
    }
    assert!(image[6].iter().all(|value| *value == 0));
    assert_eq!(
        image.iter().flatten().filter(|value| **value != 0).count(),
        42
    );
}
#[test]
fn equal_level2_indexes_in_different_roots_never_alias_leaf_tables() {
    let maps = [
        LpaePageMapping {
            iova_page: 0x00200000,
            physical_page: 0x88000000,
        },
        LpaePageMapping {
            iova_page: 0x80200000,
            physical_page: 0x88001000,
        },
        LpaePageMapping {
            iova_page: 0x80400000,
            physical_page: 0x88002000,
        },
    ];
    let plan = plan_lpae_s1_roots(&maps, pages(), config()).unwrap();
    let mut image = [[0; 512]; 7];
    plan.materialize_table_image(pages(), &mut image).unwrap();
    for m in maps {
        assert_eq!(
            walk(&image, pages(), m.iova_page).unwrap() & SMMU_LPAE_ADDRESS_MASK,
            m.physical_page
        );
    }
    assert_ne!(image[1][1], image[2][1]);
}
#[test]
fn root_materialization_rejects_wrong_backing_without_mutating_buffers() {
    let maps = mappings();
    let plan = plan_lpae_s1_roots(&maps, pages(), config()).unwrap();
    let mut wrong = pages();
    wrong.level3.swap(1, 2);
    let mut image = [[0xa5; 512]; 7];
    assert_eq!(
        plan.materialize_table_image(wrong, &mut image),
        Err(LpaePlanError::TableBackingIdentityMismatch)
    );
    assert!(image.iter().flatten().all(|v| *v == 0xa5));
}
#[test]
fn all_seven_table_identities_are_distinct_and_separate_from_every_data_page() {
    let maps = mappings();
    for i in 0..7 {
        for j in 0..i {
            let mut values = pages().pages();
            values[i] = values[j];
            let p = LpaeRootTablePages {
                ttbr0_root: values[0],
                level2: [values[1], values[2]],
                level3: [values[3], values[4], values[5]],
                ttbr1_root: values[6],
            };
            assert_eq!(
                plan_lpae_s1_roots(&maps, p, config()),
                Err(LpaePlanError::DuplicateTablePage)
            );
        }
    }
    for i in 0..7 {
        let mut maps = mappings();
        maps[36].physical_page = pages().pages()[i];
        assert_eq!(
            plan_lpae_s1_roots(&maps, pages(), config()),
            Err(LpaePlanError::TablePageAliasesDmaPage)
        );
    }
}
#[test]
fn extra_frames_reject_zero_misalignment_and_output_overflow() {
    let maps = mappings();
    for (value, expected) in [
        (0, LpaePlanError::TablePageZero),
        (0x90002001, LpaePlanError::TablePageMisaligned),
        (1 << 40, LpaePlanError::TablePageOutOfRange),
    ] {
        let mut p = pages();
        p.level2[1] = value;
        assert_eq!(plan_lpae_s1_roots(&maps, p, config()), Err(expected));
    }
}
#[test]
fn root_planner_rejects_iova_order_duplicates_and_physical_aliases() {
    let mut maps = mappings();
    maps.swap(2, 3);
    assert_eq!(
        plan_lpae_s1_roots(&maps, pages(), config()),
        Err(LpaePlanError::IovaPagesNotSorted)
    );
    let mut maps = mappings();
    maps[3].iova_page = maps[2].iova_page;
    assert_eq!(
        plan_lpae_s1_roots(&maps, pages(), config()),
        Err(LpaePlanError::DuplicateIovaPage)
    );
    let mut maps = mappings();
    maps[36].physical_page = maps[0].physical_page;
    assert_eq!(
        plan_lpae_s1_roots(&maps, pages(), config()),
        Err(LpaePlanError::DuplicateDmaPage)
    );
}
#[test]
fn root_planner_bounds_later_iova_and_physical_pages_not_just_first_mapping() {
    for (iova, physical, expected) in [
        (1 << 39, 0x88024000, LpaePlanError::IovaOutOfRange),
        (0x8611f001, 0x88024000, LpaePlanError::IovaOutOfRange),
        (0x8611f000, 1 << 40, LpaePlanError::DmaPageOutOfRange),
        (0x8611f000, 0x88024001, LpaePlanError::DmaPageOutOfRange),
    ] {
        let mut maps = mappings();
        maps[36] = LpaePageMapping {
            iova_page: iova,
            physical_page: physical,
        };
        assert_eq!(plan_lpae_s1_roots(&maps, pages(), config()), Err(expected));
    }
}
#[test]
fn root_and_leaf_counts_are_exact_and_bounded() {
    let maps = mappings();
    assert_eq!(
        plan_lpae_s1_roots(&maps[..2], pages(), config()),
        Err(LpaePlanError::RootTableCountMismatch)
    );
    assert_eq!(
        plan_lpae_s1_roots(&maps[..34], pages(), config()),
        Err(LpaePlanError::Level3TableCountMismatch)
    );
    let three_roots = [
        LpaePageMapping {
            iova_page: 0x1000,
            physical_page: 0x88000000,
        },
        LpaePageMapping {
            iova_page: 0x40001000,
            physical_page: 0x88001000,
        },
        LpaePageMapping {
            iova_page: 0x80001000,
            physical_page: 0x88002000,
        },
    ];
    assert_eq!(
        plan_lpae_s1_roots(&three_roots, pages(), config()),
        Err(LpaePlanError::RootTableCountMismatch)
    );
    let four_leaves = core::array::from_fn::<_, 4, _>(|i| LpaePageMapping {
        iova_page: [0x1000, 0x201000, 0x80001000, 0x80201000][i],
        physical_page: 0x88000000 + i as u64 * 4096,
    });
    assert_eq!(
        plan_lpae_s1_roots(&four_leaves, pages(), config()),
        Err(LpaePlanError::Level3TableCountMismatch)
    );
    let too_many = [maps[0]; 129];
    assert_eq!(
        plan_lpae_s1_roots(&too_many, pages(), config()),
        Err(LpaePlanError::TooManyDmaPages)
    );
}
#[test]
fn context_and_ns_cache_attributes_remain_the_existing_validated_profile() {
    let maps = mappings();
    let mut c = config();
    c.arm_ns_quirk = true;
    c.ring_memory_attribute = RingMemoryAttribute::NormalWriteBack;
    c.table_walk_policy = TableWalkPolicy::Coherent;
    let p = plan_lpae_s1_roots(&maps, pages(), c).unwrap();
    let mut image = [[0; 512]; 7];
    p.materialize_table_image(pages(), &mut image).unwrap();
    let legacy = plan_lpae_s1(
        &maps[..1],
        LpaeTablePages {
            ttbr0_root: pages().ttbr0_root,
            level2: pages().level2[0],
            level3: pages().level3[0],
            ttbr1_root: pages().ttbr1_root,
        },
        c,
    )
    .unwrap();
    assert_eq!(p.context(), legacy.context());
    assert_ne!(image[0][0] & (1 << 63), 0);
    assert_eq!(
        walk(&image, pages(), maps[0].iova_page).unwrap() & 0x24,
        0x24
    );
}
