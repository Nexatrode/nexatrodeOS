#![forbid(unsafe_code)]
// These source modules are included as a closed test harness; some public
// helpers are exercised only by other subsystem suites.
#![allow(dead_code)]

#[path = "../rust/src/qcom_bcm.rs"]
pub mod qcom_bcm;
#[path = "../rust/src/qcom_bcm_vote.rs"]
pub mod qcom_bcm_vote;
#[path = "../rust/src/qcom_cmd_db.rs"]
pub mod qcom_cmd_db;
#[path = "../rust/src/qcom_msm_bus_aggregate.rs"]
pub mod qcom_msm_bus_aggregate;
#[path = "../rust/src/qcom_rpmh_read.rs"]
pub mod qcom_rpmh_read;
