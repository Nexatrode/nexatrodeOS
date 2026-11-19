//! Host-only ICNSS regulator-request to RPMh VRM encoding checks.

#[allow(dead_code)]
#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
mod qcom_rpmh_vote_aggregate;
#[path = "../rust/src/qcom_rpmh_vrm.rs"]
mod qcom_rpmh_vrm;

use qcom_rpmh_vote_aggregate::{ClientVote, HEADROOM_REGISTER, MODE_REGISTER};
use qcom_rpmh_vrm::apply_icnss_action;
use qcom_wlan_power::{
    ALL_RAILS_MASK, Action, Config, Rail, VerifiedIcNssProfile, verified_sunfish_plan,
};

#[test]
fn full_icnss_plan_encodes_each_rail_through_its_own_vote_record() {
    let profile = VerifiedIcNssProfile::test_fixture(7, Config::new(ALL_RAILS_MASK, false));
    let plan = verified_sunfish_plan(&profile).unwrap();
    let initial = ClientVote {
        set_active: true,
        set_sleep: true,
        valid_mask: (1 << MODE_REGISTER) | (1 << HEADROOM_REGISTER),
        registers: [0, 0, 7, 55],
    };
    let mut votes = [initial; 5];
    for index in 0..plan.action_count() {
        let action = plan.action(index).unwrap();
        let rail = match action {
            Action::SetVoltage { rail, .. } | Action::EnableRail(rail) => rail,
            other => panic!("unexpected source-profile action: {other:?}"),
        };
        let slot = match rail {
            Rail::CxMx => 0,
            Rail::Xo18 => 1,
            Rail::Rfa13 => 2,
            Rail::Ch1_33 => 3,
            Rail::Ch0_33 => 4,
        };
        votes[slot] = apply_icnss_action(votes[slot], action).unwrap();
    }
    assert_eq!(
        votes.map(|vote| vote.registers[0]),
        [640, 1800, 1304, 3312, 3312]
    );
    assert!(votes.iter().all(|vote| vote.registers[1] == 1));
    assert!(votes.iter().all(|vote| vote.registers[2..] == [7, 55]));
    assert!(votes.iter().all(|vote| vote.valid_mask == 0b1111));
}
#[allow(dead_code)]
#[path = "../rust/src/qcom_wlan_power.rs"]
mod qcom_wlan_power;
