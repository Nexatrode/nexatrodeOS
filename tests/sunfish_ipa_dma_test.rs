#![allow(dead_code)]
#![forbid(unsafe_code)]

mod qcom_ufs_platform {
    pub const SMMU_STREAM_ID: u16 = 0x300;
}

#[path = "../rust/src/sunfish_ipa_dma.rs"]
mod sunfish_ipa_dma;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
