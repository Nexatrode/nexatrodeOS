//! Exact-route, bounded SMMUv2 translation plan for the Sunfish UFS DMA pool.
//!
//! This composes the verified SID `0x300` route, the full caller-supplied UFS
//! IOVA-to-frame map, and the LPAE context. It allocates no memory, writes no
//! registers, performs no cache maintenance, and proves no SMMU ownership.
#![forbid(unsafe_code)]

use crate::qcom_ufs_platform::{
    DMA_PAGE_BYTES, DmaAperture, DmaBackingPool, MAX_DMA_APERTURE_BYTES, SMMU_STREAM_ID,
    UfsSmmuTablePool,
};
use crate::sunfish_smmu::{RoutePlan, SUNFISH_MAX_CONTEXT_BANKS, SUNFISH_MAX_SMR_SLOTS};
use crate::sunfish_smmu_lpae::{
    LpaeContextRegisters, LpaeMultiS1Plan, LpaeMultiTablePages, LpaePageMapping, LpaePlanError,
    LpaeS1Config, plan_lpae_s1_multi,
};

pub const MAX_UFS_DMA_PAGES: usize = (MAX_DMA_APERTURE_BYTES / DMA_PAGE_BYTES) as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    RouteNotUfs,
    RouteResourceOutOfRange,
    InvalidAperture,
    MappingCountMismatch,
    MappingNotExact,
    TablePoolOverlapsDmaPool,
    Lpae(LpaePlanError),
}

impl From<LpaePlanError> for Error {
    fn from(value: LpaePlanError) -> Self {
        Self::Lpae(value)
    }
}

fn required_pages(aperture: DmaAperture) -> Result<usize, Error> {
    if !aperture.exclusive
        || aperture.base == 0
        || aperture.bytes == 0
        || !aperture.base.is_multiple_of(DMA_PAGE_BYTES)
        || !aperture.bytes.is_multiple_of(DMA_PAGE_BYTES)
        || aperture.bytes > MAX_DMA_APERTURE_BYTES
        || aperture.base.checked_add(aperture.bytes).is_none()
    {
        return Err(Error::InvalidAperture);
    }
    usize::try_from(aperture.bytes / DMA_PAGE_BYTES).map_err(|_| Error::InvalidAperture)
}

/// Fill a caller-owned exact IOVA-to-physical-page map from the validated UFS
/// backing pool. The aperture must be an independently supplied exclusive IOVA
/// range of the same size; this function does not reserve it or program SMMU.
pub fn fill_ufs_dma_backing_mappings(
    pool: DmaBackingPool,
    aperture: DmaAperture,
    mappings: &mut [LpaePageMapping],
) -> Result<(), Error> {
    let pages = required_pages(aperture)?;
    if aperture.bytes != pool.bytes() || mappings.len() != pages || pages > MAX_UFS_DMA_PAGES {
        return Err(Error::MappingCountMismatch);
    }
    for (index, mapping) in mappings.iter_mut().enumerate() {
        let displacement = u64::try_from(index)
            .map_err(|_| Error::InvalidAperture)?
            .checked_mul(DMA_PAGE_BYTES)
            .ok_or(Error::InvalidAperture)?;
        *mapping = LpaePageMapping {
            iova_page: aperture
                .base
                .checked_add(displacement)
                .ok_or(Error::InvalidAperture)?,
            physical_page: pool
                .page_physical(index)
                .map_err(|_| Error::InvalidAperture)?,
        };
    }
    Ok(())
}

/// One consistent UFS route and translated DMA-aperture plan.
///
/// Every page of the adapter's aperture must be mapped exactly once in IOVA
/// order. The selected route must be a singleton, unmasked UFS SID route; a
/// WLAN/IPA route or the saved broad ABL UFS match cannot be substituted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UfsDmaPlan<'a> {
    route: RoutePlan,
    aperture: DmaAperture,
    mappings: &'a [LpaePageMapping],
    lpae: LpaeMultiS1Plan<'a>,
}

impl UfsDmaPlan<'_> {
    pub const fn route(&self) -> RoutePlan {
        self.route
    }

    pub const fn aperture(&self) -> DmaAperture {
        self.aperture
    }

    pub const fn mappings(&self) -> &[LpaePageMapping] {
        self.mappings
    }

    pub const fn lpae(&self) -> LpaeMultiS1Plan<'_> {
        self.lpae
    }

    pub const fn stream_match_slot(&self) -> usize {
        self.route.slot()
    }

    pub const fn context_bank(&self) -> usize {
        self.route.context_bank()
    }

    pub const fn context_registers(&self) -> LpaeContextRegisters {
        self.lpae.context()
    }
}

/// Compose a previously planned exact UFS route with a full aperture mapping.
///
/// The route must have been produced from a fresh complete snapshot and
/// explicitly verified free resources. Those booleans remain caller
/// assertions until a trusted owner supplies actual evidence. This function
/// additionally checks that every byte-addressable DMA page in the aperture
/// is covered by exactly one mapping in the same order used by the adapter.
pub fn plan_ufs_dma_translation<'a>(
    route: RoutePlan,
    aperture: DmaAperture,
    mappings: &'a [LpaePageMapping],
    table_pages: LpaeMultiTablePages<'a>,
    config: LpaeS1Config,
) -> Result<UfsDmaPlan<'a>, Error> {
    validate_ufs_route(route)?;
    let required_pages = required_pages(aperture)?;
    if mappings.len() != required_pages || mappings.len() > MAX_UFS_DMA_PAGES {
        return Err(Error::MappingCountMismatch);
    }
    for (index, mapping) in mappings.iter().enumerate() {
        let expected_iova = aperture
            .base
            .checked_add(index as u64 * DMA_PAGE_BYTES)
            .ok_or(Error::InvalidAperture)?;
        if mapping.iova_page != expected_iova {
            return Err(Error::MappingNotExact);
        }
    }

    let lpae = plan_lpae_s1_multi(mappings, table_pages, config)?;
    Ok(UfsDmaPlan {
        route,
        aperture,
        mappings,
        lpae,
    })
}

/// Compose the candidate UFS backing pool with a caller-owned exclusive IOVA
/// aperture and the exact route/page-table plan. The mapping slice is filled
/// from the pool before LPAE planning; no range is reserved and no hardware is
/// programmed by this function.
pub fn plan_ufs_dma_from_backing_pool<'a>(
    route: RoutePlan,
    pool: DmaBackingPool,
    aperture: DmaAperture,
    mappings: &'a mut [LpaePageMapping],
    table_pages: LpaeMultiTablePages<'a>,
    config: LpaeS1Config,
) -> Result<UfsDmaPlan<'a>, Error> {
    validate_ufs_route(route)?;
    fill_ufs_dma_backing_mappings(pool, aperture, mappings)?;
    plan_ufs_dma_translation(route, aperture, mappings, table_pages, config)
}

/// Build the candidate profile's exact full-pool UFS plan using its separate
/// SMMU-table allocation. The table pages are kept outside the mapped DMA pool
/// and borrowed from `table_pool` for the lifetime of the returned plan.
pub fn plan_ufs_dma_from_profile_pools<'a>(
    route: RoutePlan,
    pool: DmaBackingPool,
    aperture: DmaAperture,
    mappings: &'a mut [LpaePageMapping],
    table_pool: &'a UfsSmmuTablePool,
    config: LpaeS1Config,
) -> Result<UfsDmaPlan<'a>, Error> {
    validate_ufs_route(route)?;
    if ranges_overlap(
        pool.physical_base(),
        pool.bytes(),
        table_pool.physical_base(),
        table_pool.bytes(),
    ) {
        return Err(Error::TablePoolOverlapsDmaPool);
    }
    fill_ufs_dma_backing_mappings(pool, aperture, mappings)?;
    let table_pages = LpaeMultiTablePages {
        ttbr0_root: table_pool.ttbr0_root(),
        level2: table_pool.level2(),
        level3: table_pool.level3_pages(),
        ttbr1_root: table_pool.ttbr1_root(),
    };
    plan_ufs_dma_translation(route, aperture, mappings, table_pages, config)
}

fn validate_ufs_route(route: RoutePlan) -> Result<(), Error> {
    if route.id() != u32::from(SMMU_STREAM_ID)
        || route.mask() != 0
        || route.matched_sids() != [u32::from(SMMU_STREAM_ID), 0]
        || route.matched_sid_count() != 1
    {
        return Err(Error::RouteNotUfs);
    }
    if route.slot() >= SUNFISH_MAX_SMR_SLOTS || route.context_bank() >= SUNFISH_MAX_CONTEXT_BANKS {
        return Err(Error::RouteResourceOutOfRange);
    }
    Ok(())
}

fn ranges_overlap(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    let Some(a_end) = a.checked_add(a_bytes) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_bytes) else {
        return true;
    };
    a < b_end && b < a_end
}
