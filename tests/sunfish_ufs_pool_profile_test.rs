#[path = "../rust/src/qcom_ufs_platform.rs"]
#[allow(dead_code)]
mod qcom_ufs_platform;

use qcom_ufs_platform::{
    DmaBackingPool, Error, UfsSmmuTablePool, DMA_PAGE_BYTES, UFS_DMA_POOL_BYTES,
    UFS_DMA_POOL_VIRTUAL_BASE, UFS_SMMU_TABLE_PAGE_COUNT, UFS_SMMU_TABLE_POOL_BYTES,
    UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
};

#[test]
fn accepts_the_candidate_pool_geometry_without_assigning_an_iova() {
    let pool =
        DmaBackingPool::from_profile(UFS_DMA_POOL_VIRTUAL_BASE, 0x88f0_0000, UFS_DMA_POOL_BYTES)
            .unwrap();

    assert_eq!(pool.virtual_base(), UFS_DMA_POOL_VIRTUAL_BASE);
    assert_eq!(pool.physical_base(), 0x88f0_0000);
    assert_eq!(pool.bytes(), 0x0100_0000);
    assert_eq!(pool.page_count(), 4096);
    assert_eq!(pool.page_physical(0).unwrap(), 0x88f0_0000);
    assert_eq!(
        pool.page_physical(pool.page_count() - 1).unwrap(),
        0x89ef_f000
    );
    assert_eq!(
        pool.page_physical(pool.page_count()),
        Err(Error::InvalidDmaBacking)
    );
}

#[test]
fn rejects_invalid_or_unbounded_profile_values() {
    let invalid = [
        (
            UFS_DMA_POOL_VIRTUAL_BASE + DMA_PAGE_BYTES,
            0x88f0_0000,
            UFS_DMA_POOL_BYTES,
        ),
        (UFS_DMA_POOL_VIRTUAL_BASE, 0, UFS_DMA_POOL_BYTES),
        (UFS_DMA_POOL_VIRTUAL_BASE, 0x88f0_0001, UFS_DMA_POOL_BYTES),
        (
            UFS_DMA_POOL_VIRTUAL_BASE,
            0x88f0_0000,
            UFS_DMA_POOL_BYTES - DMA_PAGE_BYTES,
        ),
        (
            UFS_DMA_POOL_VIRTUAL_BASE,
            (u64::MAX & !(DMA_PAGE_BYTES - 1)) - 0x1000,
            UFS_DMA_POOL_BYTES,
        ),
        (UFS_DMA_POOL_VIRTUAL_BASE, 1u64 << 40, UFS_DMA_POOL_BYTES),
    ];

    for (virtual_base, physical_base, bytes) in invalid {
        assert_eq!(
            DmaBackingPool::from_profile(virtual_base, physical_base, bytes),
            Err(Error::InvalidDmaBacking)
        );
    }
}

#[test]
fn accepts_exact_separate_lpae_table_storage_and_numbers_all_pages() {
    let tables = UfsSmmuTablePool::from_profile(
        UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
        0x8a00_0000,
        UFS_SMMU_TABLE_POOL_BYTES,
    )
    .unwrap();

    assert_eq!(tables.virtual_base(), UFS_SMMU_TABLE_POOL_VIRTUAL_BASE);
    assert_eq!(tables.physical_base(), 0x8a00_0000);
    assert_eq!(tables.bytes(), 0xb000);
    assert_eq!(tables.page_physical(0).unwrap(), 0x8a00_0000);
    assert_eq!(tables.page_physical(1).unwrap(), 0x8a00_1000);
    assert_eq!(tables.level3_pages()[0], 0x8a00_2000);
    assert_eq!(tables.level3_pages()[7], 0x8a00_9000);
    assert_eq!(tables.ttbr1_root(), 0x8a00_a000);
    assert_eq!(
        tables.page_physical(UFS_SMMU_TABLE_PAGE_COUNT),
        Err(Error::InvalidSmmuTables)
    );
}

#[test]
fn rejects_invalid_or_unbounded_smmu_table_profile_values() {
    let invalid = [
        (
            UFS_SMMU_TABLE_POOL_VIRTUAL_BASE + DMA_PAGE_BYTES,
            0x8a00_0000,
            UFS_SMMU_TABLE_POOL_BYTES,
        ),
        (
            UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
            0,
            UFS_SMMU_TABLE_POOL_BYTES,
        ),
        (
            UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
            0x8a00_0001,
            UFS_SMMU_TABLE_POOL_BYTES,
        ),
        (
            UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
            0x8a00_0000,
            UFS_SMMU_TABLE_POOL_BYTES - DMA_PAGE_BYTES,
        ),
        (
            UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
            (u64::MAX & !(DMA_PAGE_BYTES - 1)) - 0x1000,
            UFS_SMMU_TABLE_POOL_BYTES,
        ),
        (
            UFS_SMMU_TABLE_POOL_VIRTUAL_BASE,
            1u64 << 40,
            UFS_SMMU_TABLE_POOL_BYTES,
        ),
    ];

    for (virtual_base, physical_base, bytes) in invalid {
        assert_eq!(
            UfsSmmuTablePool::from_profile(virtual_base, physical_base, bytes),
            Err(Error::InvalidSmmuTables)
        );
    }
}
