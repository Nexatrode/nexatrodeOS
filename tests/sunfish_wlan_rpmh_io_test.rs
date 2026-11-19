//! Host tests for composing the source-derived ICNSS plan with shared RPMh votes.

#[path = "../rust/src/qcom_bcm.rs"]
#[allow(dead_code)]
mod qcom_bcm;
#[path = "../rust/src/qcom_bcm_vote.rs"]
#[allow(dead_code)]
mod qcom_bcm_vote;
#[path = "../rust/src/qcom_cmd_db.rs"]
#[allow(dead_code)]
mod qcom_cmd_db;
#[path = "../rust/src/qcom_rpmh_handoff.rs"]
#[allow(dead_code)]
mod qcom_rpmh_handoff;
#[path = "../rust/src/qcom_rpmh_read.rs"]
#[allow(dead_code)]
mod qcom_rpmh_read;
#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
mod qcom_rpmh_vote_aggregate;
#[path = "../rust/src/qcom_rpmh_vote_broker.rs"]
mod qcom_rpmh_vote_broker;
#[path = "../rust/src/qcom_rpmh_vrm.rs"]
mod qcom_rpmh_vrm;
#[path = "../rust/src/qcom_rpmh_vrm_command.rs"]
mod qcom_rpmh_vrm_command;
#[path = "../rust/src/qcom_rpmh_vrm_tcs.rs"]
mod qcom_rpmh_vrm_tcs;
#[path = "../rust/src/qcom_rpmh_vrm_tcs_io.rs"]
mod qcom_rpmh_vrm_tcs_io;
#[path = "../rust/src/qcom_rpmh_vrm_tcs_transport.rs"]
mod qcom_rpmh_vrm_tcs_transport;
#[path = "../rust/src/qcom_wlan_power.rs"]
mod qcom_wlan_power;
#[path = "../rust/src/qcom_wlan_rpmh_io.rs"]
mod qcom_wlan_rpmh_io;
#[path = "../rust/src/qcom_wlan_rpmh_map.rs"]
mod qcom_wlan_rpmh_map;
#[path = "../rust/src/qcom_wlan_rsc_native_io.rs"]
mod qcom_wlan_rsc_native_io;

#[test]
fn icnss_adapter_requires_all_five_source_mapped_vote_resources() {
    assert_eq!(qcom_wlan_power::RAIL_COUNT, 5);
    assert_eq!(qcom_rpmh_vote_aggregate::MAX_CLIENTS, 16);
    assert_eq!(qcom_wlan_rpmh_map::VRM_SLAVE_ID, 3);
    assert_eq!(
        qcom_wlan_rpmh_map::SUNFISH_RAIL_RESOURCE_KEYS,
        ["ldoa9", "ldoc1", "ldoc2", "ldoc11", "ldoc10"]
    );
}

#[test]
fn verified_icnss_power_plan_keeps_its_boot_generation() {
    let profile = qcom_wlan_power::VerifiedIcNssProfile::test_fixture(
        7,
        qcom_wlan_power::Config::new(qcom_wlan_power::ALL_RAILS_MASK, false),
    );
    let plan = qcom_wlan_power::verified_sunfish_plan(&profile).unwrap();

    assert_eq!(plan.generation(), 7);
}
