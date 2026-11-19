//! Host tests for source-derived UFS client requests on RPMh VRM rails.

#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
#[allow(dead_code)]
mod qcom_rpmh_vote_aggregate;
#[path = "../rust/src/qcom_ufs_rpmh_profile.rs"]
mod qcom_ufs_rpmh_profile;

use qcom_rpmh_vote_aggregate::ClientVote;
use qcom_ufs_rpmh_profile::{
    Error, HPM_MIN_LOAD_UA, PMIC5_LDO_HPM_MODE, SupplyClient, UFS_SUPPLY_SPECS,
    enabled_client_vote, requested_millivolts,
};

#[test]
fn ufs_supply_specs_preserve_the_source_derived_rail_alias_and_loads() {
    assert_eq!(UFS_SUPPLY_SPECS.len(), 5);
    assert_eq!(
        UFS_SUPPLY_SPECS.map(|spec| spec.resource_key),
        ["ldoa19", "ldoa12", "ldoa4", "ldoc3", "ldoc3"]
    );
    assert_eq!(
        UFS_SUPPLY_SPECS.map(|spec| spec.max_load_microamps),
        [600_000, 600_000, 62_900, 18_300, 100]
    );
    assert!(UFS_SUPPLY_SPECS
        .iter()
        .all(|spec| spec.max_load_microamps >= HPM_MIN_LOAD_UA));
}

#[test]
fn enabled_votes_match_rounding_mode_and_both_source_sets() {
    let expected_mv = [2950, 1800, 800, 1200, 1200];
    for (index, client) in [
        SupplyClient::Vcc,
        SupplyClient::Vccq2,
        SupplyClient::VddaPhy,
        SupplyClient::VddaPll,
        SupplyClient::VddpRefClk,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            enabled_client_vote(client),
            Ok(ClientVote {
                set_active: true,
                set_sleep: true,
                valid_mask: 0b0111,
                registers: [expected_mv[index], 1, PMIC5_LDO_HPM_MODE, 0],
            })
        );
    }
}

#[test]
fn voltage_rounding_respects_the_pinned_maximum_and_overflow_bounds() {
    assert_eq!(requested_millivolts(2_950_000, 2_960_000), Ok(2950));
    assert_eq!(requested_millivolts(1_800_001, 1_802_000), Ok(1801));
    assert_eq!(
        requested_millivolts(1_800_001, 1_800_999),
        Err(Error::InvalidVoltageRange)
    );
    assert_eq!(
        requested_millivolts(u32::MAX, u32::MAX),
        Err(Error::InvalidVoltageRange)
    );
}
