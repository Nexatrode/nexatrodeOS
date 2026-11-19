#![allow(dead_code)]
#![forbid(unsafe_code)]
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
mod sunfish_ipa_linux_gsi;
#[path = "../rust/src/sunfish_ipa_linux_receive.rs"]
mod sunfish_ipa_linux_receive;
#[path = "../rust/src/sunfish_ipa_linux_receive_domain.rs"]
mod sunfish_ipa_linux_receive_domain;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;

use sunfish_ipa_linux_receive as rx;
use sunfish_ipa_linux_receive_domain::*;
use sunfish_smmu_lpae::*;

fn shared() -> [LpaePageMapping; 37] {
    core::array::from_fn(|n| {
        let iova = if n < 2 {
            0x146a8000 + n as u64 * 4096
        } else if n < 34 {
            0x80000000 + (n - 2) as u64 * 4096
        } else {
            0x8611d000 + (n - 34) as u64 * 4096
        };
        LpaePageMapping {
            iova_page: iova,
            physical_page: if (2..34).contains(&n) {
                0x88000000 + (n - 2) as u64 * 4096
            } else {
                iova
            },
        }
    })
}
fn physical() -> [u64; 512] {
    core::array::from_fn(|n| 0x98000000 + n as u64 * 4096)
}
fn old_tables() -> LpaeRootTablePages {
    LpaeRootTablePages {
        ttbr0_root: 0x90000000,
        level2: [0x90001000, 0x90002000],
        level3: [0x90003000, 0x90004000, 0x90005000],
        ttbr1_root: 0x90006000,
    }
}
fn tables() -> LpaeRootTablePages<4> {
    let old = old_tables();
    LpaeRootTablePages {
        ttbr0_root: old.ttbr0_root,
        level2: old.level2,
        level3: [old.level3[0], old.level3[1], 0x90007000, old.level3[2]],
        ttbr1_root: old.ttbr1_root,
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

#[test]
fn page_by_page_materialization_matches_full_image_and_rejects_invalid_inputs() {
    let shared = shared();
    let physical = physical();
    let mapping = Mapping::new(&shared, &physical).unwrap();
    for ns in [false, true] {
        for attr in [RingMemoryAttribute::NormalNonCacheable, RingMemoryAttribute::NormalWriteBack] {
            let mut cfg = config();
            cfg.arm_ns_quirk = ns;
            cfg.ring_memory_attribute = attr;
            let plan = plan_lpae_s1_receive_roots(mapping.mappings(), tables(), cfg).unwrap();
            let mut full = [[0u64; 512]; 8];
            plan.materialize_table_image(tables(), &mut full).unwrap();
            for index in 0..8 {
                let mut page = [0xa5u64; 512];
                plan.materialize_table_page(tables(), index, &mut page).unwrap();
                assert_eq!(page, full[index], "table {index}, ns={ns}, attr={attr:?}");
            }
        }
    }
    let plan = mapping.plan(tables()).unwrap();
    let mut page = [0xa5u64; 512];
    let mut wrong = tables();
    wrong.level3.swap(0, 1);
    assert_eq!(plan.materialize_table_page(wrong, 0, &mut page), Err(LpaePlanError::TableBackingIdentityMismatch));
    assert!(page.iter().all(|&v| v == 0xa5));
    assert_eq!(plan.materialize_table_page(tables(), 8, &mut page), Err(LpaePlanError::DescriptorCountInvariant));
    assert!(page.iter().all(|&v| v == 0xa5));
}
// Walk physical descriptor links without using planner table/leaf indexes.
fn walk<const N: usize>(image: &[[u64; 512]; N], identities: [u64; N], iova: u64) -> Option<u64> {
    let mut table = 0;
    for shift in [30, 21] {
        let d = image[table][((iova >> shift) & 511) as usize];
        if d & 3 != 3 {
            return None;
        }
        table = identities
            .iter()
            .position(|p| *p == d & SMMU_LPAE_ADDRESS_MASK)?;
    }
    let d = image[table][((iova >> 12) & 511) as usize];
    (d & 3 == 3).then_some(d)
}

#[test]
fn rxdomain_full_539_page_plan_walks_data_and_excludes_all_ten_tail_pages() {
    let mapping = Mapping::new(&shared(), &physical()).unwrap();
    assert_eq!(mapping.mappings().len(), 539);
    assert_eq!(mapping.receive_allocation(), &physical());
    let p = mapping.plan(tables()).unwrap();
    assert_eq!(p.root_indices(), [0, 2]);
    assert_eq!(p.descriptor_count(), 545);
    let mut image = [[0xa5; 512]; 8];
    let receipt = p.materialize_table_image(tables(), &mut image).unwrap();
    assert_eq!(receipt.written_descriptors, 545);
    assert_eq!(image.iter().flatten().filter(|p| **p != 0).count(), 545);
    assert!(image[7].iter().all(|v| *v == 0));
    for m in mapping.mappings() {
        let d = walk(&image, tables().pages(), m.iova_page).unwrap();
        assert_eq!(d & SMMU_LPAE_ADDRESS_MASK, m.physical_page);
        assert_eq!(d & ((3 << 53) | 0xfff), (3 << 53) | 0xe43);
    }
    for n in 0..512 {
        let value = walk(&image, tables().pages(), rx::DATA_IOVA + n as u64 * 4096);
        if n < 502 {
            assert_eq!(value.unwrap() & SMMU_LPAE_ADDRESS_MASK, physical()[n]);
        } else {
            assert_eq!(value, None, "CPU-only tail {n}");
        }
    }
    for hole in [
        0, 0x146a7000, 0x146aa000, 0x40000000, 0x80020000, 0x801ff000, 0x80400000, 0x8611c000,
        0x86120000,
    ] {
        assert_eq!(walk(&image, tables().pages(), hole), None, "{hole:x}");
    }
}

#[test]
fn rxdomain_extension_preserves_all_old_entries_context_and_profile_bounds() {
    let old_maps = shared();
    let old = plan_lpae_s1_roots(&old_maps, old_tables(), config()).unwrap();
    let mapping = Mapping::new(&old_maps, &physical()).unwrap();
    let new = mapping.plan(tables()).unwrap();
    assert_eq!(old.context(), new.context());
    let mut a = [[0; 512]; 7];
    let mut b = [[0; 512]; 8];
    old.materialize_table_image(old_tables(), &mut a).unwrap();
    new.materialize_table_image(tables(), &mut b).unwrap();
    for (old_page, new_page) in [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 6), (6, 7)] {
        for entry in 0..512 {
            if old_page == 2 && entry == 1 {
                assert_eq!(a[old_page][entry], 0);
                assert_eq!(b[new_page][entry], 0x90007003);
            } else {
                assert_eq!(a[old_page][entry], b[new_page][entry], "{old_page}/{entry}");
            }
        }
    }
    assert_eq!(
        plan_lpae_s1_roots(mapping.mappings(), old_tables(), config()).err(),
        Some(LpaePlanError::TooManyDmaPages)
    );
    let mut excessive = mapping.mappings().to_vec();
    excessive.push(LpaePageMapping {
        iova_page: 0x86120000,
        physical_page: 0x99000000,
    });
    assert_eq!(
        plan_lpae_s1_receive_roots(&excessive, tables(), config()).err(),
        Some(LpaePlanError::TooManyDmaPages)
    );
    assert_eq!(
        plan_lpae_s1_receive_roots(&old_maps, tables(), config()).err(),
        Some(LpaePlanError::Level3TableCountMismatch)
    );
}

#[test]
fn rxdomain_complete_shared_and_receive_geometry_and_alias_checks() {
    for n in 0..37 {
        let mut s = shared();
        s[n].iova_page ^= 4096;
        assert!(matches!(Mapping::new(&s, &physical()), Err(Error::Shared)));
        let mut s = shared();
        s[n].physical_page ^= 4096;
        assert!(Mapping::new(&s, &physical()).is_err());
    }
    for len in [0, 1, 32, 501, 502, 511, 513] {
        let mut p = physical().to_vec();
        p.resize(len, 0);
        assert!(matches!(Mapping::new(&shared(), &p), Err(Error::Geometry)));
    }
    for n in [0, 1, 31, 250, 501, 502, 511] {
        for bad in [0, 0x7ffff000, 0x98000001, 0x98000000, 1u64 << 40, u64::MAX] {
            let mut p = physical();
            if p[n] == bad {
                continue;
            }
            p[n] = bad;
            assert!(
                matches!(Mapping::new(&shared(), &p), Err(Error::Physical)),
                "{n}/{bad:x}"
            );
        }
    }
    for base in [0x88000000, 0x87f00000, 0x86100000] {
        let p: [u64; 512] = core::array::from_fn(|n| base + n as u64 * 4096);
        assert!(
            matches!(Mapping::new(&shared(), &p), Err(Error::Alias)),
            "{base:x}"
        );
    }
}

#[test]
fn rxdomain_all_eight_table_frames_exclude_dma_and_cpu_tail_aliases() {
    let mapping = Mapping::new(&shared(), &physical()).unwrap();
    for index in 0..8 {
        for bad in [
            0,
            1,
            0x90000000,
            1u64 << 40,
            u64::MAX,
            physical()[0],
            physical()[501],
            physical()[502],
            physical()[511],
            0x88000000,
            0x8611d000,
        ] {
            let mut t = tables();
            let original = t.pages();
            if original[index] == bad {
                continue;
            }
            match index {
                0 => t.ttbr0_root = bad,
                1 => t.level2[0] = bad,
                2 => t.level2[1] = bad,
                3..=6 => t.level3[index - 3] = bad,
                _ => t.ttbr1_root = bad,
            }
            assert!(mapping.plan(t).is_err(), "{index}/{bad:x}");
        }
    }
    let plan = mapping.plan(tables()).unwrap();
    let mut wrong_size = [[0xa5; 512]; 7];
    assert_eq!(
        plan.materialize_table_image(tables(), &mut wrong_size),
        Err(LpaePlanError::DescriptorCountInvariant)
    );
    assert!(wrong_size.iter().flatten().all(|v| *v == 0xa5));
    let mut excessive_size = [[0xa5; 512]; 9];
    assert_eq!(
        plan.materialize_table_image(tables(), &mut excessive_size),
        Err(LpaePlanError::DescriptorCountInvariant)
    );
    assert!(excessive_size.iter().flatten().all(|v| *v == 0xa5));
    let mut wrong_identity = tables();
    wrong_identity.level3.swap(1, 2);
    let mut image = [[0xa5; 512]; 8];
    assert_eq!(
        plan.materialize_table_image(wrong_identity, &mut image),
        Err(LpaePlanError::TableBackingIdentityMismatch)
    );
    assert!(image.iter().flatten().all(|v| *v == 0xa5));
}

#[test]
fn rxdomain_four_leaf_generic_encoder_validates_late_entries_and_full_geometry() {
    let mapping = Mapping::new(&shared(), &physical()).unwrap();
    for n in [0, 1, 33, 34, 128, 256, 535, 536, 538] {
        for kind in 0..5 {
            let mut pages = *mapping.mappings();
            match kind {
                0 => pages[n].iova_page |= 1,
                1 => pages[n].physical_page |= 1,
                2 => pages[n].physical_page = 0x90007000,
                3 => pages[n].physical_page = 1u64 << 40,
                _ => {
                    pages[n].physical_page = if n == 0 {
                        pages[1].physical_page
                    } else {
                        pages[0].physical_page
                    }
                }
            }
            assert!(
                plan_lpae_s1_receive_roots(&pages, tables(), config()).is_err(),
                "{n}/{kind}"
            );
        }
    }
    let mut pages = *mapping.mappings();
    pages.swap(400, 401);
    assert_eq!(
        plan_lpae_s1_receive_roots(&pages, tables(), config()).err(),
        Some(LpaePlanError::IovaPagesNotSorted)
    );
    let mut pages = *mapping.mappings();
    pages[538].iova_page = 0x100000000;
    assert_eq!(
        plan_lpae_s1_receive_roots(&pages, tables(), config()).err(),
        Some(LpaePlanError::RootTableCountMismatch)
    );
    let mut pages = *mapping.mappings();
    pages[538].iova_page = 0x86400000;
    assert_eq!(
        plan_lpae_s1_receive_roots(&pages, tables(), config()).err(),
        Some(LpaePlanError::Level3TableCountMismatch)
    );
}
