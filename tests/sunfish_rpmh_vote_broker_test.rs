//! Host-only failure-path checks for the per-resource RPMh vote broker model.

#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
mod qcom_rpmh_vote_aggregate;
#[path = "../rust/src/qcom_rpmh_vote_broker.rs"]
mod qcom_rpmh_vote_broker;
#[path = "../rust/src/qcom_rpmh_vrm_command.rs"]
mod qcom_rpmh_vrm_command;

use qcom_rpmh_vote_aggregate::{
    ENABLE_REGISTER, HEADROOM_REGISTER, MODE_REGISTER, VOLTAGE_REGISTER,
};

#[test]
fn register_slots_match_the_vrm_aggregate_contract() {
    assert_eq!(
        [
            VOLTAGE_REGISTER,
            ENABLE_REGISTER,
            MODE_REGISTER,
            HEADROOM_REGISTER,
        ],
        [0, 1, 2, 3]
    );
}
