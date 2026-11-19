//! RAM-backed MMIO simulation for the native adapter; never accesses a device.
#![allow(dead_code)]

#[path = "../rust/src/qcom_modem_status.rs"]
mod qcom_modem_status;
#[path = "../rust/src/sunfish_ipa_shared_memory.rs"]
mod sunfish_ipa_shared_memory;
#[path = "../rust/src/sunfish_ipa_smem.rs"]
mod sunfish_ipa_smem;
#[path = "../rust/src/sunfish_ipa_smem_native.rs"]
mod sunfish_ipa_smem_native;

#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/qcom_ufs_dma.rs"]
mod qcom_ufs_dma;
#[path = "../rust/src/qcom_ufs_platform.rs"]
mod qcom_ufs_platform;
#[path = "../rust/src/qcom_wmi_ce.rs"]
mod qcom_wmi_ce;
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_ipa_dma.rs"]
mod sunfish_ipa_dma;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_exec.rs"]
mod sunfish_smmu_exec;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
#[path = "../rust/src/sunfish_smmu_native.rs"]
mod sunfish_smmu_native;
#[path = "../rust/src/sunfish_smmu_program.rs"]
mod sunfish_smmu_program;
#[path = "../rust/src/sunfish_smmu_snapshot.rs"]
mod sunfish_smmu_snapshot;
#[path = "../rust/src/sunfish_wlan_dma.rs"]
mod sunfish_wlan_dma;

#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
mod sunfish_ipa_linux_gsi;
#[path = "../rust/src/sunfish_ipa_linux_receive.rs"]
mod sunfish_ipa_linux_receive;
#[path = "../rust/src/sunfish_ipa_linux_receive_domain.rs"]
mod sunfish_ipa_linux_receive_domain;
#[path = "../rust/src/sunfish_ipa_linux_receive_native.rs"]
mod sunfish_ipa_linux_receive_native;

#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
#[path = "../rust/src/sunfish_mpss_linux_smem.rs"]
mod sunfish_mpss_linux_smem;
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
#[path = "../rust/src/sunfish_mpss_linux_smem_native.rs"]
mod sunfish_mpss_linux_smem_native;
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
#[path = "../rust/src/sunfish_mpss_linux_smp2p.rs"]
mod sunfish_mpss_linux_smp2p;
#[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
#[path = "../rust/src/sunfish_mpss_linux_smp2p_native.rs"]
mod sunfish_mpss_linux_smp2p_native;
