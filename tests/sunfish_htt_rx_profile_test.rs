#![forbid(unsafe_code)]

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;
}

#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;

use sunfish_htt_rx_profile::{HttRxProfileError, SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};

#[test]
fn pinned_sunfish_qcacld_profile_matches_the_source_capacity_formula() {
    let plan = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();

    assert_eq!(plan.ring_slots(), 2048);
    assert_eq!(plan.initial_fill_level(), 2047);
    assert_eq!(plan.rx_buffer_bytes(), 2176);
    assert_eq!(plan.address_ring_bytes(), 16_384);
    assert_eq!(plan.index_bytes(), 4);
    assert_eq!(plan.fallback_buffer_capacity_bytes(), 139_264);
    assert_eq!(plan.initial_dma_bytes(), Some(4_470_660));
    assert_eq!(plan.minimum_packed_page_count(), Some(1092));
    assert_eq!(plan.reserved_pool_bytes(), Some(4_609_924));
    assert_eq!(plan.minimum_reserved_page_count(), Some(1126));
}

#[test]
fn full_reorder_mode_accounts_for_its_extra_target_index() {
    let config = sunfish_htt_rx_profile::HttRxConfig {
        has_target_index: true,
        ..SUNFISH_QCACLD_CONFIG
    };
    let plan = plan_htt_rx_profile(config).unwrap();

    assert_eq!(plan.index_bytes(), 8);
    assert_eq!(plan.initial_dma_bytes(), Some(4_470_664));
}

#[test]
fn rejects_invalid_divisors_and_ring_bounds() {
    assert_eq!(
        plan_htt_rx_profile(sunfish_htt_rx_profile::HttRxConfig {
            average_frame_bytes: 0,
            ..SUNFISH_QCACLD_CONFIG
        }),
        Err(HttRxProfileError::ZeroFrameSize)
    );
    assert_eq!(
        plan_htt_rx_profile(sunfish_htt_rx_profile::HttRxConfig {
            ring_max: 127,
            ..SUNFISH_QCACLD_CONFIG
        }),
        Err(HttRxProfileError::InvalidRingBounds)
    );
}
