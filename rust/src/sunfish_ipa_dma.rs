//! Source-bounded composition of the IPA AP route, ring pages, and SMMUv2
//! translation table plan.
//!
//! This module prevents independently valid plans from being accidentally
//! combined for the wrong stream ID or context bank. It still performs no
//! allocation, MMIO, cache maintenance, SMMU programming, or ownership check of
//! its own. In particular, the booleans used to obtain a [`RoutePlan`] are
//! caller assertions, not proof that QHEE or the boot chain delegated control.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_gsi_profile::DmaPageMapPlan;
use crate::sunfish_smmu::{
    RoutePlan, SUNFISH_IPA_AP_DMA_SID, SUNFISH_MAX_CONTEXT_BANKS, SUNFISH_MAX_SMR_SLOTS,
};
use crate::sunfish_smmu_lpae::{
    LpaeContextRegisters, LpaePlanError, LpaeS1Config, LpaeS1Plan, LpaeTablePages,
    plan_ap_ring_lpae_s1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpaApDmaPlanError {
    RouteNotIpaAp,
    RouteResourceOutOfRange,
    Lpae(LpaePlanError),
}

impl From<LpaePlanError> for IpaApDmaPlanError {
    fn from(value: LpaePlanError) -> Self {
        Self::Lpae(value)
    }
}

/// One consistent IPA AP translated-DMA plan.
///
/// Construction accepts only the exact IPA AP route plan (`SID 0x520`, no
/// mask, one matched SID) and binds its selected context bank and slot to the
/// page-map and LPAE plans. This is a planning value, not an execution permit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpaApDmaPlan {
    route: RoutePlan,
    mappings: DmaPageMapPlan,
    lpae: LpaeS1Plan,
}

impl IpaApDmaPlan {
    pub const fn route(&self) -> RoutePlan {
        self.route
    }

    pub const fn mappings(&self) -> &DmaPageMapPlan {
        &self.mappings
    }

    pub const fn lpae(&self) -> LpaeS1Plan {
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

/// Combine validated route and page-map plans into one IPA AP configuration.
///
/// The `RoutePlan` can only be created by the route planner, which checks that
/// the caller asserted a fresh complete snapshot, delegated control, and
/// ownership transfer. This composition does not independently authenticate
/// any of those assertions and must not be used as authorization to touch
/// hardware.
pub fn plan_ipa_ap_dma_translation(
    route: RoutePlan,
    mappings: DmaPageMapPlan,
    table_pages: LpaeTablePages,
    config: LpaeS1Config,
) -> Result<IpaApDmaPlan, IpaApDmaPlanError> {
    if route.id() != SUNFISH_IPA_AP_DMA_SID
        || route.mask() != 0
        || route.matched_sids() != [SUNFISH_IPA_AP_DMA_SID, 0]
        || route.matched_sid_count() != 1
    {
        return Err(IpaApDmaPlanError::RouteNotIpaAp);
    }
    if route.slot() >= SUNFISH_MAX_SMR_SLOTS || route.context_bank() >= SUNFISH_MAX_CONTEXT_BANKS {
        return Err(IpaApDmaPlanError::RouteResourceOutOfRange);
    }

    let lpae = plan_ap_ring_lpae_s1(&mappings, table_pages, config)?;
    Ok(IpaApDmaPlan {
        route,
        mappings,
        lpae,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sunfish_ipa_gsi_profile::{
        AP_DMA_PAGE_MAPPING_COUNT, DmaIovaWindow, DmaPool, LINUX_IPA_AP_IOVA_BYTES,
        LINUX_IPA_AP_IOVA_START, plan_ap_dma_page_mappings, plan_ap_dma_rings,
    };
    use crate::sunfish_smmu::{plan_ipa_ap_dma_route, plan_ufs_route, plan_wlan_alias_route};
    use crate::sunfish_smmu_lpae::{RingMemoryAttribute, TableWalkPolicy};

    fn route() -> RoutePlan {
        plan_ipa_ap_dma_route(true, true, true, 15, 78, 61, &[], &[7], &[9]).unwrap()
    }

    fn mappings() -> DmaPageMapPlan {
        let rings = plan_ap_dma_rings(DmaPool {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: 0xe000,
        })
        .unwrap();
        let physical_pages: [u64; AP_DMA_PAGE_MAPPING_COUNT] =
            core::array::from_fn(|index| 0x8800_0000 + index as u64 * 0x1000);
        plan_ap_dma_page_mappings(
            &rings,
            DmaIovaWindow {
                base_iova: LINUX_IPA_AP_IOVA_START,
                bytes: LINUX_IPA_AP_IOVA_BYTES,
            },
            &physical_pages,
            40,
        )
        .unwrap()
    }

    fn table_pages() -> LpaeTablePages {
        LpaeTablePages {
            ttbr0_root: 0x9000_0000,
            level2: 0x9000_1000,
            level3: 0x9000_2000,
            ttbr1_root: 0x9000_3000,
        }
    }

    fn config() -> LpaeS1Config {
        LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 2,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        }
    }

    #[test]
    fn binds_the_ipa_ap_route_to_its_exact_ring_map_and_lpae_context() {
        let plan =
            plan_ipa_ap_dma_translation(route(), mappings(), table_pages(), config()).unwrap();
        assert_eq!(plan.stream_match_slot(), 7);
        assert_eq!(plan.context_bank(), 9);
        assert_eq!(plan.route().id(), SUNFISH_IPA_AP_DMA_SID);
        assert_eq!(plan.route().mask(), 0);
        assert_eq!(plan.mappings().pages().len(), 14);
        assert_eq!(plan.lpae().table_pages(), table_pages());
        assert_eq!(plan.context_registers().ttbr0, 0x9000_0000 | (2u64 << 48));
    }

    #[test]
    fn rejects_other_stream_plans_before_building_a_translation_context() {
        let wlan = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[7], &[9]).unwrap();
        let ufs = plan_ufs_route(true, true, true, 15, 78, 61, &[], &[7], &[9]).unwrap();
        for wrong_route in [wlan, ufs] {
            assert_eq!(
                plan_ipa_ap_dma_translation(wrong_route, mappings(), table_pages(), config()),
                Err(IpaApDmaPlanError::RouteNotIpaAp)
            );
        }
    }

    #[test]
    fn propagates_lpae_geometry_errors_without_losing_route_identity() {
        let mut bad_config = config();
        bad_config.output_address_bits = 35;
        assert_eq!(
            plan_ipa_ap_dma_translation(route(), mappings(), table_pages(), bad_config),
            Err(IpaApDmaPlanError::Lpae(
                LpaePlanError::UnsupportedOutputAddressWidth
            ))
        );
    }
}
