// Host-only test harness for the pure SMMUv2 snapshot decoder and route model.
// It has no register access and does not link into the device image.
mod qcom_ufs_platform {
    pub const SMMU_STREAM_ID: u16 = 0x300;
}

#[path = "../rust/src/sunfish_smmu.rs"]
pub mod sunfish_smmu;
