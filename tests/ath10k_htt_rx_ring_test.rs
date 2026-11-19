#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_htt_rx_ring.rs"]
mod ath10k_htt_rx_ring;

#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_firmware.rs"]
mod ath10k_firmware;

#[path = "../rust/src/ath10k_wmi.rs"]
mod ath10k_wmi;

#[path = "../rust/src/ath10k_htt_rx_owner.rs"]
mod ath10k_htt_rx_owner;

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LpaePageMapping {
        pub iova_page: u64,
        pub physical_page: u64,
    }
}
