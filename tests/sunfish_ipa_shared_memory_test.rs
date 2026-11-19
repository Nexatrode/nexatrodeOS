#![allow(dead_code)]
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/qcom_modem_status.rs"]
mod qcom_modem_status;
mod sunfish_ipa_shared_fixture;
#[path = "../rust/src/sunfish_ipa_shared_memory.rs"]
mod sunfish_ipa_shared_memory;
#[path = "../rust/src/sunfish_ipa_smem.rs"]
mod sunfish_ipa_smem;
#[path = "../rust/src/sunfish_ipa_smem_native.rs"]
mod sunfish_ipa_smem_native;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
use sunfish_ipa_shared_fixture::Fixture;
use sunfish_ipa_shared_memory::{Error, SharedMemory};

#[test]
fn live_owner_admits_only_exact_identity_shared_pages_and_private_pool() {
    let mut f = Fixture::new();
    let imem = f.imem.iter().flat_map(|p| p.0).collect::<Vec<_>>();
    let before = f.smem.iter().flat_map(|p| p.0).collect::<Vec<_>>();
    let mut owner = f.shared();
    let pool: [u64; 32] = core::array::from_fn(|i| 0xa0000000 + i as u64 * 4096);
    assert_eq!(owner.mappings(&pool), Err(Error::NotAllocated));
    owner.prepare().unwrap();
    assert_eq!(owner.item_physical(), Some(0x8611d030));
    let maps = owner.mappings(&pool).unwrap();
    for i in 0..37 {
        let (iova, pa) = if i < 2 {
            (0x146a8000 + i as u64 * 4096, 0x146a8000 + i as u64 * 4096)
        } else if i < 34 {
            (0x80000000 + (i - 2) as u64 * 4096, pool[i - 2])
        } else {
            (
                0x8611d000 + (i - 34) as u64 * 4096,
                0x8611d000 + (i - 34) as u64 * 4096,
            )
        };
        assert_eq!((maps[i].iova_page, maps[i].physical_page), (iova, pa));
    }
    owner.validate_mappings(&maps, &pool).unwrap();
    let r = owner.report();
    assert_eq!((r.submitted, r.completed), (15, 15));
    assert!(r.release_sent);
    for i in 0..37 {
        let mut changed = maps;
        changed[i].physical_page += 4096;
        assert_eq!(
            owner.validate_mappings(&changed, &pool),
            Err(Error::Geometry)
        );
        changed = maps;
        changed[i].iova_page += 4096;
        assert_eq!(
            owner.validate_mappings(&changed, &pool),
            Err(Error::Geometry)
        );
    }
    assert_eq!(
        owner.validate_mappings(&maps[..36], &pool),
        Err(Error::Geometry)
    );
    drop(owner);
    assert_eq!(f.imem.iter().flat_map(|p| p.0).collect::<Vec<_>>(), imem);
    let after = f.smem.iter().flat_map(|p| p.0).collect::<Vec<_>>();
    for i in 0..after.len() {
        if !(0x11d020..0x11d02c).contains(&i) && !(0x11d00c..0x11d010).contains(&i) {
            assert_eq!(before[i], after[i], "unplanned shared byte {i:x}");
        }
    }
}

#[test]
fn private_page_aliases_and_wrong_pool_geometry_are_rejected() {
    let mut f = Fixture::new();
    let mut owner = f.shared();
    owner.prepare().unwrap();
    let pool: [u64; 32] = core::array::from_fn(|i| 0xa0000000 + i as u64 * 4096);
    for page in [
        0x146a8000,
        0x86000000,
        0x8611d000,
        0x861ff000,
        0xa0000001,
        1u64 << 40,
        pool[1],
    ] {
        let mut changed = pool;
        changed[0] = page;
        assert_eq!(owner.mappings(&changed), Err(Error::Geometry));
    }
    assert_eq!(owner.mappings(&pool[..31]), Err(Error::Geometry));
    assert_eq!(
        owner.prepare(),
        Err(Error::Allocation(sunfish_ipa_smem::Error::Consumed))
    );
}

#[test]
fn allocation_failure_retains_report_and_never_authorizes_shared_maps() {
    let mut f = Fixture::new();
    f.word(0x11d010, 0x7b000);
    let mut owner = f.shared();
    assert!(owner.prepare().is_err());
    assert!(owner.item_physical().is_none());
    assert_eq!((owner.report().submitted, owner.report().completed), (0, 0));
    assert!(owner.report().consumed && owner.report().release_sent);
    assert_eq!(owner.shared_pages(), Err(Error::NotAllocated));
}

#[test]
fn imem_reservation_identity_size_and_cpu_alias_are_required_before_access() {
    use sunfish_ipa_smem_native::Window;
    for (wrong_physical, bytes, alias) in [
        (0x146a9000, 8192, false),
        (0x146a8000, 4096, false),
        (0x146a8000, 8192, true),
    ] {
        let mut f = Fixture::new();
        let base = f.smem.as_mut_ptr().cast::<u8>();
        let imem = if alias {
            base.cast_const()
        } else {
            f.imem.as_ptr().cast()
        };
        let result = unsafe {
            let window = Window::new(
                base,
                0x86000000,
                base.add(0x11d000).cast(),
                0x8611d000,
                f.lock.0.as_mut_ptr().cast(),
                0x01f43000,
            )
            .unwrap();
            SharedMemory::new(window, imem, wrong_physical, bytes)
        };
        assert!(result.is_err());
        assert_eq!(f.lock.0, [0; 4096]);
    }
}
