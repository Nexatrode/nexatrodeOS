#!/usr/bin/env python3
"""Run host wire tests and compile the same core as an AArch64 no_std library.

Requires rustc, cargo, cached host SHA-256 dependencies and the aarch64-unknown-none core target. Uses only a
temporary host directory and a private pinned-C oracle cache; never invokes
ADB, Fastboot, firmware or a device.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import re
import os
import sys


ROOT = Path(__file__).resolve().parents[2]
MODULES = ("ath10k_ce", "ath10k_board", "ath10k_firmware", "ath10k_htt_rx", "ath10k_htt_rx_ring", "ath10k_htt_rx_owner", "ath10k_htt_rx_session", "ath10k_nwifi_rx", "ieee80211_ethernet", "ath10k_scan", "ath10k_key", "ath10k_mgmt_rx", "ath10k_mgmt_tx", "ath10k_wmi_tx", "ieee80211_probe", "ieee80211_station", "ath10k_wmi", "qcom_qmi", "qcom_ipa", "qcom_ipa_client", "qcom_ipa_service", "sunfish_ipa_post_init", "sunfish_ipa_profile", "sunfish_ipa_bus_vote", "sunfish_ipa_hw_init", "sunfish_ipa_gsi_profile", "sunfish_ipa_gsi_exec", "sunfish_ipa_snapshot", "qcom_servreg", "qcom_servreg_notif", "qcom_servreg_notif_client", "qcom_modem_services", "qcom_qmi_router", "qcom_qrtr_control", "qcom_qrtr_tftp", "wlfw_protocol", "qcom_msa", "qcom_msa_handoff", "qcom_msa_native", "qcom_scm_assign", "qcom_wlfw_bdf", "qcom_wlfw_client", "qcacld_wmi", "sunfish_qcacld_resource", "sunfish_qcacld_tx", "qcom_htc", "sunfish_qcacld_htc", "sunfish_qcacld_ce_tx", "qcom_wlan_power", "sunfish_icnss_startup", "qcom_cmd_db", "qcom_bcm", "qcom_bcm_vote", "qcom_msm_bus_aggregate", "qcom_rpmh_vote_aggregate", "qcom_rpmh_vote_broker", "qcom_rpmh_handoff", "qcom_rpmh_vrm", "qcom_rpmh_vrm_command", "qcom_rpmh_vrm_tcs", "qcom_rpmh_vrm_tcs_io", "qcom_rpmh_vrm_tcs_transport", "qcom_wlan_rpmh_map", "qcom_wlan_rpmh_io", "qcom_glink_qmi", "qcom_glink_fifo", "qcom_modem_owner", "qcom_modem_handlers", "qcom_modem_pools", "qcom_modem_storage", "qcom_modem_backend", "qcom_ufs_protocol", "qcom_ufs_host", "qcom_ufs_gpt", "qcom_ufs_irq", "qcom_ufs_modem_storage", "qcom_ufs_dma", "qcom_ufs_gcc", "qcom_ufs_scsi_block_io", "qcom_ufs_platform", "qcom_ufs_rpmh_map", "qcom_ufs_rpmh_profile", "qcom_ce_v3", "qcom_ce_v3_regs", "qcom_ce_v3_profile", "qcom_ce_v3_init", "qcom_wmi_ce", "sunfish_qcacld_mgmt_tx", "sunfish_qcacld_wmi_rx", "qcom_tftp", "sunfish_lpd", "sunfish_vendor_ext4", "sunfish_vendor_wlan_source", "sunfish_smmu", "sunfish_smmu_lpae", "sunfish_ipa_dma", "sunfish_smmu_snapshot", "sunfish_smmu_program", "sunfish_smmu_exec", "sunfish_wlan_dma", "sunfish_htt_rx_profile", "sunfish_htt_rx_ring", "sunfish_qcacld_rx", "sunfish_qcacld_rx_ind", "sunfish_qcacld_rx_transport", "sunfish_qcacld_rx_session", "sunfish_qcacld_rx_delivery", "sunfish_qcacld_rx_pipeline")

MODULES = ("ath10k_htt",) + MODULES
MODULES += ("sunfish_qcacld_htt_event",)
MODULES += ("sunfish_qcacld_key",)
MODULES += ("wlan_key_confirmation",)
MODULES += ("qcom_modem_ram_storage",)
MODULES += ("sunfish_ipa_linux_config", "sunfish_ipa_linux_memory", "sunfish_ipa_linux_buffers", "sunfish_ipa_linux_resources", "sunfish_ipa_linux_irq")
MODULES += ("qcom_modem_status", "sunfish_ipa_linux_uc")
MODULES += ("sunfish_ipa_linux_gsi",)
MODULES += ("sunfish_ipa_linux_endpoint",)
MODULES += ("sunfish_ipa_linux_command",)
MODULES += ("sunfish_ipa_linux_memsetup",)
MODULES += ("sunfish_ipa_linux_tables",)
MODULES += ("sunfish_ipa_linux_transfer",)
MODULES += ("sunfish_ipa_linux_table_transfer",)
MODULES += ("qcom_rpmh_read", "qcom_rpmh_bus")
MODULES += ("qcom_mdt", "qcom_mdt_packed", "qcom_pas_metadata", "qcom_pas_init", "qcom_pas_mem_setup", "qcom_modem_image", "sunfish_ipa_fws", "sunfish_ipa_pas", "sunfish_linux_firmware", "sunfish_ipa_linux_pas", "sunfish_ipa_linux_loader", "sunfish_scm_linux_power")


def main():
    rustc = shutil.which("rustc")
    if rustc is None:
        raise SystemExit("rustc is required")
    subprocess.run([sys.executable, str(ROOT / "platforms/sunfish/test-native-ipa-linux-gsi.py")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, str(ROOT / "platforms/sunfish/test-native-ipa-linux-endpoint.py")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, str(ROOT / "platforms/sunfish/test-native-ipa-linux-command.py")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, str(ROOT / "platforms/sunfish/test-native-ipa-linux-memsetup.py")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, str(ROOT / "platforms/sunfish/test-native-ipa-linux-tables.py")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, str(ROOT / "platforms/sunfish/test-native-ipa-linux-transfer.py")], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="sunfish-qmi-") as temporary:
        out = Path(temporary)
        for suite in ("sunfish_ipa_linux_loader_test", "sunfish_scm_linux_power_test", "sunfish_ipa_linux_uc_test", "sunfish_ipa_shared_memory_test", "sunfish_ipa_smem_test", "sunfish_smmu_root_tables_test", "sunfish_ipa_linux_irq_test", "sunfish_ipa_linux_resources_test", "sunfish_ipa_linux_buffers_test", "sunfish_ipa_linux_memory_test", "sunfish_linux_firmware_test", "sunfish_qmi_routing_test", "sunfish_tftp_test", "sunfish_lpd_test", "sunfish_vendor_ext4_test", "sunfish_servreg_notif_test", "sunfish_servreg_notif_client_test", "sunfish_glink_qmi_test", "sunfish_qrtr_control_test", "sunfish_wlfw_client_test", "sunfish_qcacld_wmi_test", "sunfish_qcacld_resource_test", "sunfish_qcom_htc_test", "sunfish_qcacld_htc_test", "sunfish_qcacld_rx_ind_test", "sunfish_qcacld_rx_transport_test", "sunfish_qcacld_ce_tx_test", "sunfish_qcacld_ce_tx_native_test", "sunfish_qcacld_wmi_irq_runtime_test", "sunfish_modem_handlers_test", "sunfish_modem_pools_test", "sunfish_modem_owner_test", "sunfish_modem_owner_gate_test", "sunfish_modem_owner_native_test", "sunfish_ufs_native_test", "sunfish_ufs_pool_profile_test", "sunfish_ufs_dma_backing_test", "sunfish_smmu_fault_test", "sunfish_smmu_lpae_test", "sunfish_htt_lpae_test", "sunfish_ipa_dma_test", "sunfish_ipa_pas_test", "sunfish_smmu_program_test", "sunfish_smmu_native_test", "sunfish_wlan_dma_test", "sunfish_htt_rx_profile_test", "sunfish_htt_rx_ring_test", "ath10k_board_test", "ath10k_htt_rx_test", "ath10k_htt_rx_ring_test", "ath10k_htt_rx_session_test", "ath10k_nwifi_rx_test", "ath10k_scan_test", "ath10k_key_test", "ath10k_mgmt_rx_test", "ath10k_wmi_tx_test", "ieee80211_probe_test", "ieee80211_ethernet_test", "ieee80211_station_test", "ath10k_wmi_test", "sunfish_qcacld_rx_test", "sunfish_qcacld_rx_session_test", "sunfish_qcacld_rx_delivery_test", "sunfish_qcacld_tx_test", "sunfish_ipa_bus_vote_test", "sunfish_ipa_hw_init_test", "sunfish_ipa_gsi_profile_test", "sunfish_ipa_gsi_exec_test", "sunfish_ce_v3_test", "sunfish_ce_v3_regs_test", "sunfish_ce_v3_profile_test", "sunfish_ce_v3_init_test", "sunfish_ce_v3_native_test", "sunfish_ce_v3_tx_native_test", "sunfish_wmi_ce_tx_test", "sunfish_msm_bus_aggregate_test", "sunfish_rpmh_vote_aggregate_test", "sunfish_rpmh_vote_broker_test", "sunfish_rpmh_vrm_test", "sunfish_wlan_rpmh_io_test", "sunfish_ufs_rpmh_map_test", "sunfish_ufs_rpmh_profile_test"):
            tests = out / suite
            subprocess.run(
                [rustc, "--edition=2024", "-D", "warnings", "--test",
                 str(ROOT / "tests" / f"{suite}.rs"), "-o", str(tests)],
                cwd=ROOT, check=True,
            )
            arguments = [str(tests)]
            if suite == "sunfish_modem_owner_gate_test":
                arguments.append("owner_gate_")
            elif suite == "sunfish_modem_owner_native_test":
                arguments.append("owner_native_")
            subprocess.run(arguments, cwd=ROOT, check=True)
        config_test = out / "sunfish_ipa_linux_config_test"
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--test",
             str(ROOT / "tests/sunfish_ipa_linux_config_test.rs"), "-o", str(config_test)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(config_test)], cwd=ROOT, check=True)
        htt_event_test = out / "sunfish_qcacld_htt_event_test"
        ipa_linux_test = out / "sunfish_ipa_linux_power_test"
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--test",
             str(ROOT / "tests/sunfish_ipa_linux_power_test.rs"), "-o", str(ipa_linux_test)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(ipa_linux_test)], cwd=ROOT, check=True)
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--test",
             str(ROOT / "tests/sunfish_qcacld_htt_event_test.rs"), "-o", str(htt_event_test)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(htt_event_test)], cwd=ROOT, check=True)
        qcacld_key_test = out / "sunfish_qcacld_key_test"
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--test",
             str(ROOT / "tests/sunfish_qcacld_key_test.rs"), "-o", str(qcacld_key_test)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(qcacld_key_test)], cwd=ROOT, check=True)
        sink_test = out / "sunfish_wlan_net_sink_test"
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--test",
             str(ROOT / "tests/sunfish_wlan_net_sink_test.rs"), "-o", str(sink_test)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(sink_test)], cwd=ROOT, check=True)
        subprocess.run(
            ["python3", "-m", "unittest", "tests.sunfish_smmu_actlr_policy_test", "-v"],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", "-m", "unittest", "tests.sunfish_native_boot_inputs_test", "-v"],
            cwd=ROOT, check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--manifest-path",
             str(ROOT / "rust/Cargo.toml"), "--lib", "--features",
             "sunfish-native-wifi-private-inputs", "sunfish_private_boot_inputs"],
            cwd=ROOT, check=True,
        )
        subprocess.run(
            ["python3", "-m", "unittest", "tests.sunfish_smmu_runtime_route_test", "-v"],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", "-m", "unittest", "tests.sunfish_wlan_protocol_probe_isolation_test", "-v"],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-ath10k-board-data.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-ath10k-stop-scan.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-ath10k-key.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-ath10k-htt-security.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-lineage-modem-startup-order.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-sunfish-qcacld-key.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-sunfish-qcacld-tx.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-sunfish-qcacld-mgmt-tx.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-sunfish-qcacld-wmi-rx.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-ieee80211-ethernet.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-sunfish-qcacld-rx.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/validate-sunfish-htc-credit.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/test-wlan-net-bridge.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["python3", str(ROOT / "platforms/sunfish/test-wlan-net-bridge-ffi.py")],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--manifest-path",
             str(ROOT / "rust/Cargo.toml"), "--lib", "sunfish_qcacld_mgmt_tx"],
            cwd=ROOT, check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--manifest-path",
             str(ROOT / "rust/Cargo.toml"), "--lib", "sunfish_qcacld_wmi_rx"],
            cwd=ROOT, check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--manifest-path",
             str(ROOT / "rust/Cargo.toml"), "--lib", "ieee80211_"],
            cwd=ROOT, check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--manifest-path",
             str(ROOT / "rust/Cargo.toml"), "--lib", "sunfish_qcacld_key_join"],
            cwd=ROOT, check=True,
        )
        wifi_security_nostd = out / "wifi-security-nostd"
        (wifi_security_nostd / "src").mkdir(parents=True)
        (wifi_security_nostd / "Cargo.toml").write_text(
            '[package]\nname = "sunfish-wifi-security-nostd-check"\n'
            'version = "0.1.0"\nedition = "2024"\n'
            '[dependencies]\n'
            'aes = { version = "=0.9.3", default-features = false, features = ["zeroize"] }\n'
            'hmac = { version = "=0.12.1", default-features = false }\n'
            'pbkdf2 = { version = "=0.12.2", default-features = false, features = ["hmac"] }\n'
            'sha1 = { version = "=0.10.7", default-features = false }\n'
            'subtle = { version = "=2.6.1", default-features = false }\n'
            'zeroize = { version = "=1.9.0", default-features = false }\n'
        )
        wpa2_module = (
            str(ROOT / "rust/src/ieee80211_wpa2.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        eapol_module = (
            str(ROOT / "rust/src/ieee80211_eapol.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ccmp_module = (
            str(ROOT / "rust/src/ieee80211_ccmp.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ethernet_module = (
            str(ROOT / "rust/src/ieee80211_ethernet.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        supplicant_module = (
            str(ROOT / "rust/src/ieee80211_supplicant.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        station_module = (
            str(ROOT / "rust/src/ieee80211_station.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        join_module = (
            str(ROOT / "rust/src/ieee80211_join.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        nwifi_rx_module = (
            str(ROOT / "rust/src/ath10k_nwifi_rx.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ath10k_htt_module = (
            str(ROOT / "rust/src/ath10k_htt.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ath10k_key_module = (
            str(ROOT / "rust/src/ath10k_key.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        wlan_key_confirmation_module = (
            str(ROOT / "rust/src/wlan_key_confirmation.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ath10k_ce_module = (
            str(ROOT / "rust/src/ath10k_ce.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ath10k_scan_module = (
            str(ROOT / "rust/src/ath10k_scan.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        ath10k_wmi_tx_module = (
            str(ROOT / "rust/src/ath10k_wmi_tx.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        bss_module = (
            str(ROOT / "rust/src/ieee80211_bss.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        mgmt_rx_module = (
            str(ROOT / "rust/src/ath10k_mgmt_rx.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        scan_results_module = (
            str(ROOT / "rust/src/ieee80211_scan_results.rs")
            .replace("\\", "\\\\")
            .replace('"', '\\"')
        )
        sunfish_tx_modules = [
            (name, str(ROOT / f"rust/src/{name}.rs")
             .replace("\\", "\\\\").replace('"', '\\"'))
            for name in (
                "qcom_ce_v3_regs",
                "qcom_ce_v3",
                "qcom_ce_v3_profile",
                "qcom_ce_v3_init",
                "qcom_htc",
                "ath10k_firmware",
                "ath10k_wmi",
                "qcacld_wmi",
                "qcom_wmi_ce",
                "sunfish_qcacld_mgmt_tx",
                "sunfish_qcacld_htt_event",
                "sunfish_qcacld_htt_event_native",
                "sunfish_qcacld_key",
                "sunfish_qcacld_key_join",
                "sunfish_qcacld_key_native",
                "sunfish_qcacld_key_join_native",
                "sunfish_qcacld_wmi_rx",
                "qcom_ufs_platform",
                "sunfish_ipa_gsi_profile",
                "sunfish_smmu_lpae",
                "sunfish_smmu",
                "sunfish_htt_rx_profile",
                "sunfish_qcacld_rx",
                "sunfish_qcacld_rx_ind",
                "sunfish_qcacld_rx_session",
                "sunfish_htt_rx_ring",
                "sunfish_qcacld_tx",
                "sunfish_qcacld_htc",
                "sunfish_qcacld_ce_tx",
                "qcom_ce_v3_native_io",
                "sunfish_wlan_net_source",
                "sunfish_wlan_tx_pipeline",
            )
        ]
        (wifi_security_nostd / "src/lib.rs").write_text(
            '#![no_std]\n'
            f'#[path = "{nwifi_rx_module}"]\n'
            'pub mod ath10k_nwifi_rx;\n'
            f'#[path = "{ath10k_htt_module}"]\n'
            'pub mod ath10k_htt;\n'
            f'#[path = "{wlan_key_confirmation_module}"]\n'
            'pub mod wlan_key_confirmation;\n'
            f'#[path = "{ath10k_key_module}"]\n'
            'pub mod ath10k_key;\n'
            f'#[path = "{mgmt_rx_module}"]\n'
            'pub mod ath10k_mgmt_rx;\n'
            f'#[path = "{ath10k_ce_module}"]\n'
            'pub mod ath10k_ce;\n'
            f'#[path = "{ath10k_wmi_tx_module}"]\n'
            'pub mod ath10k_wmi_tx;\n'
            f'#[path = "{ath10k_scan_module}"]\n'
            'pub mod ath10k_scan;\n'
            f'#[path = "{bss_module}"]\n'
            'pub mod ieee80211_bss;\n'
            f'#[path = "{scan_results_module}"]\n'
            'pub mod ieee80211_scan_results;\n'
            f'#[path = "{wpa2_module}"]\n'
            'pub mod ieee80211_wpa2;\n'
            f'#[path = "{eapol_module}"]\n'
            'pub mod ieee80211_eapol;\n'
            f'#[path = "{ccmp_module}"]\n'
            'pub mod ieee80211_ccmp;\n'
            f'#[path = "{ethernet_module}"]\n'
            'pub mod ieee80211_ethernet;\n'
            f'#[path = "{supplicant_module}"]\n'
            'pub mod ieee80211_supplicant;\n'
            f'#[path = "{station_module}"]\n'
            'pub mod ieee80211_station;\n'
            f'#[path = "{join_module}"]\n'
            'pub mod ieee80211_join;\n'
            + "".join(
                f'#[path = "{path}"]\npub mod {name};\n'
                for name, path in sunfish_tx_modules
            )
        )
        subprocess.run(
            ["cargo", "+nightly-2026-09-25", "-Z", "build-std=core", "check",
             "--offline", "--target", "aarch64-unknown-none"],
            cwd=wifi_security_nostd, check=True,
        )
        environment = os.environ.copy()
        environment["RUSTFLAGS"] = environment.get("RUSTFLAGS", "") + " -D warnings"
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--manifest-path",
             str(ROOT / "experiments/modem-storage-host/Cargo.toml"),
             "--target-dir", str(out / "storage-host-target")],
            cwd=ROOT, env=environment, check=True,
        )
        lines = ["#![no_std]", "#![forbid(unsafe_code)]"]
        for module in MODULES:
            path = str(ROOT / "rust/src" / f"{module}.rs")
            escaped = path.replace("\\", "\\\\").replace('"', '\\"')
            # Partial core harness: these legacy private entrypoints are
            # exercised by their dedicated suites, not this loader fragment.
            if module in ("qcom_rpmh_read", "qcom_pas_init", "qcom_modem_image"):
                lines.append("#[allow(dead_code)]")
            lines.extend([f'#[path = "{escaped}"]', f"pub mod {module};"])
        harness = out / "core.rs"
        harness.write_text("\n".join(lines) + "\n")
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--crate-type=lib",
             "--crate-name=sunfish_qmi_core", "--target=aarch64-unknown-none",
             str(harness), "-o", str(out / "libsunfish_qmi_core.rlib")],
            cwd=ROOT, check=True,
        )
        sink_module = str(ROOT / "rust/src/sunfish_wlan_net_sink.rs")
        sink_escaped = sink_module.replace("\\", "\\\\").replace('"', '\\"')
        source_module = str(ROOT / "rust/src/sunfish_wlan_net_source.rs")
        source_escaped = source_module.replace("\\", "\\\\").replace('"', '\\"')
        sink_nostd = out / "wlan_net_sink_nostd.rs"
        sink_nostd.write_text("\n".join([
            "#![no_std]",
            "pub mod sunfish_qcacld_rx_delivery {",
            " pub trait EthernetRxSink { type Error; fn receive(&mut self, frame: &[u8]) -> Result<(), Self::Error>; }",
            "}",
            f'#[path = "{sink_escaped}"]',
            "pub mod sunfish_wlan_net_sink;",
            f'#[path = "{source_escaped}"]',
            "pub mod sunfish_wlan_net_source;",
            "",
        ]))
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--crate-type=lib",
             "--target=aarch64-unknown-none", str(sink_nostd),
             "-o", str(out / "libsunfish_wlan_net_sink.rlib")],
            cwd=ROOT, check=True,
        )
        # Keep unsafe volatile access isolated from the safe, forbid-unsafe core.
        # Export concrete wrappers to force AArch64 code generation of the
        # backend, rather than only type-checking unused generic methods.
        native_path = str(ROOT / "rust/src/qcom_glink_native_io.rs")
        escaped = native_path.replace("\\", "\\\\").replace('"', '\\"')
        ufs_native_path = str(ROOT / "rust/src/qcom_ufs_native_io.rs")
        ufs_escaped = ufs_native_path.replace("\\", "\\\\").replace('"', '\\"')
        ce_native_path = str(ROOT / "rust/src/qcom_ce_v3_native_io.rs")
        ce_escaped = ce_native_path.replace("\\", "\\\\").replace('"', '\\"')
        key_native_path = str(ROOT / "rust/src/sunfish_qcacld_key_native.rs")
        key_native_escaped = key_native_path.replace("\\", "\\\\").replace('"', '\\"')
        htt_event_native_path = str(ROOT / "rust/src/sunfish_qcacld_htt_event_native.rs")
        htt_event_native_escaped = htt_event_native_path.replace("\\", "\\\\").replace('"', '\\"')
        wmi_rx_native_path = str(ROOT / "rust/src/sunfish_qcacld_wmi_rx_native.rs")
        wmi_rx_native_escaped = wmi_rx_native_path.replace("\\", "\\\\").replace('"', '\\"')
        rsc_native_path = str(ROOT / "rust/src/qcom_wlan_rsc_native_io.rs")
        rsc_escaped = rsc_native_path.replace("\\", "\\\\").replace('"', '\\"')
        native = out / "native.rs"
        native.write_text("\n".join([
            "#![no_std]", "#![deny(unsafe_op_in_unsafe_fn)]", *lines[2:],
            f'#[path = "{escaped}"]', "pub mod qcom_glink_native_io;",
            f'#[path = "{ROOT / "rust/src/qcom_modem_pool_native.rs"}"]', "pub mod qcom_modem_pool_native;",
            f'#[path = "{ufs_escaped}"]', "pub mod qcom_ufs_native_io;",
            f'#[path = "{ce_escaped}"]', "pub mod qcom_ce_v3_native_io;",
            f'#[path = "{key_native_escaped}"]', "pub mod sunfish_qcacld_key_native;",
            f'#[path = "{htt_event_native_escaped}"]', "pub mod sunfish_qcacld_htt_event_native;",
            f'#[path = "{wmi_rx_native_escaped}"]', "pub mod sunfish_qcacld_wmi_rx_native;",
            f'#[path = "{rsc_escaped}"]', "pub mod qcom_wlan_rsc_native_io;",
            f'#[path = "{ROOT / "rust/src/sunfish_ipa_linux_buffers_native.rs"}"]', "pub mod sunfish_ipa_linux_buffers_native;",
            f'#[path = "{ROOT / "rust/src/sunfish_ipa_smem.rs"}"]', "pub mod sunfish_ipa_smem;",
            f'#[path = "{ROOT / "rust/src/sunfish_ipa_smem_native.rs"}"]', "pub mod sunfish_ipa_smem_native;",
            f'#[path = "{ROOT / "rust/src/sunfish_ipa_shared_memory.rs"}"]', "pub mod sunfish_ipa_shared_memory;",
            f'#[path = "{ROOT / "rust/src/sunfish_smmu_native.rs"}"]', "pub mod sunfish_smmu_native;",
            f'#[path = "{ROOT / "rust/src/sunfish_ipa_linux_loader_native.rs"}"]', "pub mod sunfish_ipa_linux_loader_native;",
            "pub struct IpaScmCodegen(pub fn(&qcom_pas_init::Request)->Result<[u64;8],u64>);",
            "impl qcom_pas_init::Transport for IpaScmCodegen { fn invoke(&mut self,r:&qcom_pas_init::Request)->Result<[u64;8],u64>{(self.0)(r)} }",
            "pub struct IpaRscCodegen {pub read:fn(qcom_rpmh_read::Register)->u32,pub write:fn(qcom_rpmh_read::Register,u32),pub snapshot:fn(qcom_rpmh_handoff::Register)->u32}",
            "impl qcom_rpmh_read::RegisterIo for IpaRscCodegen {fn read(&mut self,r:qcom_rpmh_read::Register)->u32{(self.read)(r)} fn write(&mut self,r:qcom_rpmh_read::Register,v:u32){(self.write)(r,v)}}",
            "impl qcom_rpmh_handoff::ReadRegisters for IpaRscCodegen {fn read(&mut self,r:qcom_rpmh_handoff::Register)->u32{(self.snapshot)(r)}}",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_linux_scm_hold_probe(hold:&mut sunfish_scm_linux_power::HoldOnce,io:&mut IpaRscCodegen,metadata:[qcom_bcm::Metadata;6])->Result<(),sunfish_scm_linux_power::Error>{hold.enable(io,metadata,65536,core::hint::spin_loop,|_|{})}",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_linux_metadata_prepare_probe(loader:&mut sunfish_ipa_linux_loader::Loader<'_,sunfish_ipa_linux_loader_native::Window<'_>>)->Result<(),sunfish_ipa_linux_loader::Error>{loader.prepare()}",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_linux_loader_start_probe(loader:&mut sunfish_ipa_linux_loader::Loader<'_,sunfish_ipa_linux_loader_native::Window<'_>>,secure:&mut IpaScmCodegen,hold:&sunfish_scm_linux_power::HoldOnce,io:&mut IpaRscCodegen)->Result<(),sunfish_ipa_linux_loader::Error>{let mut resources=sunfish_scm_linux_power::HeldResources::new(hold,io,65536).map_err(|_|sunfish_ipa_linux_loader::Error::Guard)?;loader.start(&mut sunfish_ipa_linux_pas::LinuxTransport::new(secure,&mut resources),|_|Ok(()),|_|{})}",
            "pub type SmmuCodegen<'a> = sunfish_smmu_native::Native<'a,fn(usize)->Result<(),sunfish_smmu_native::Error>>;",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_shared_attach_probe<'a>(native: &mut SmmuCodegen<'a>, memory: sunfish_ipa_shared_memory::SharedMemory<'a>) -> Result<(),sunfish_smmu_native::Error> { native.attach_shared_memory(memory) }",
            "#[unsafe(no_mangle)]",
            "pub unsafe fn ipa_shared_stage_probe(native: &mut SmmuCodegen<'_>, plan: &sunfish_smmu_lpae::LpaeRootS1Plan<'_>, extra: [*mut u64;3], physical:[u64;3]) -> Result<(),sunfish_smmu_native::Error> { unsafe { native.stage_shared_root_tables(plan,extra,physical) } }",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_smem_allocate_probe(owner: &mut sunfish_ipa_smem::Owner<sunfish_ipa_smem_native::Window<'_>>) -> Result<u64,sunfish_ipa_smem::Error> { owner.allocate_once().map(|item|item.physical()) }",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_buffer_native_read_probe(io: &mut sunfish_ipa_linux_buffers_native::NativeRam<'_>, offset: usize) -> Result<u32,sunfish_ipa_linux_buffers::MemoryError> { sunfish_ipa_linux_buffers::Memory::read32(io,offset) }",
            "#[unsafe(no_mangle)]",
            "pub fn ipa_buffer_native_write_probe(io: &mut sunfish_ipa_linux_buffers_native::NativeRam<'_>, offset: usize, value: u32) -> Result<(),sunfish_ipa_linux_buffers::MemoryError> { sunfish_ipa_linux_buffers::Memory::write32(io,offset,value) }",
            "use qcom_glink_fifo::{Barrier, Fault, Io, Observation};",
            "use qcom_glink_native_io::{Gate, NativeIo};",
            "pub struct CodegenGate(pub Observation);",
            "impl Gate for CodegenGate {",
            " fn observe(&mut self) -> Result<Observation, Fault> { Ok(self.0) }",
            "}",
            # This static stand-in exists only in the temporary codegen crate;
            # it is never linked into the owner or used to authorize hardware.
            "pub struct UfsCodegenGuard;",
            "impl qcom_ufs_native_io::Guard for UfsCodegenGuard {",
            " fn observation(&self) -> qcom_ufs_native_io::Observation { qcom_ufs_native_io::Observation { generation: 1, fresh_boot: true, exclusive: true, controller_power_ready: true, platform_ready: true, host_device_uncached: true, host_virtual: 0x64000000, host_physical: 0x01d84000, host_bytes: 0x3000, dma_virtual: 0x65000000, dma_iova: 0x80000000, dma_bytes: 0x100000, dma_stream_id: 0x300, smmu_route_slot: Some(7), smmu_route_id: 0x300, smmu_route_mask: 0, smmu_route_translated: true, smmu_context_bank: Some(3), smmu_context_enabled: true, dma_uncached: true, dma_exclusive: true, dma_translated: true } }",
            "}",
            "pub struct UfsCodegenGate;",
            "unsafe impl qcom_ufs_native_io::Gate for UfsCodegenGate {",
            " type Guard<'a> = UfsCodegenGuard;",
            " fn lock(&mut self) -> Result<Self::Guard<'_>, qcom_ufs_host::Error> { Ok(UfsCodegenGuard) }",
            "}",
            "pub struct UfsCodegenDelay;",
            "impl qcom_ufs_native_io::Delay for UfsCodegenDelay { fn delay_us(&mut self, _: u32) {} }",
            "pub type UfsRegisterIo = qcom_ufs_native_io::NativeRegisterIo<UfsCodegenGate,UfsCodegenDelay>;",
            "pub type UfsDmaIo = qcom_ufs_native_io::NativeDmaIo<UfsCodegenGate>;",
            "#[unsafe(no_mangle)]",
            "pub fn ufs_native_read_probe(io: &mut UfsRegisterIo, offset: u32) -> Result<u32,qcom_ufs_host::Error> { qcom_ufs_host::RegisterIo::read32(io,offset) }",
            "#[unsafe(no_mangle)]",
            "pub fn ufs_native_write_probe(io: &mut UfsRegisterIo, offset: u32, value: u32) -> Result<(),qcom_ufs_host::Error> { qcom_ufs_host::RegisterIo::write32(io,offset,value) }",
            "#[unsafe(no_mangle)]",
            "pub fn ufs_native_dma_read_probe(io: &mut UfsDmaIo, addr: u64, out: &mut [u8]) -> Result<(),qcom_ufs_host::Error> { qcom_ufs_host::DmaIo::read(io,addr,out) }",
            "#[unsafe(no_mangle)]",
            "pub fn ufs_native_dma_write_probe(io: &mut UfsDmaIo, addr: u64, bytes: &[u8]) -> Result<(),qcom_ufs_host::Error> { qcom_ufs_host::DmaIo::write(io,addr,bytes) }",
            "pub struct CeCodegenGuard;",
            "impl qcom_ce_v3_native_io::Guard for CeCodegenGuard {",
            " fn observation(&self) -> qcom_ce_v3_native_io::Observation { qcom_ce_v3_native_io::Observation { generation: 1, fresh_boot: true, exclusive: true, platform_ready: true, ce_owner_exclusive: true, ce_powered: true, ce_reset_released: true, ce_quiescent: true, ce_irq_intid: Some(0x1c0), ce_irq_bound: true, ce_irq_exclusive: true, htt_ce_irq_intid: Some(0x1bf), htt_ce_irq_bound: true, htt_ce_irq_exclusive: true, wmi_map_generation: 1, wmi_host_to_target_engine: Some(3), wmi_target_to_host_engine: Some(2), htc_control_map_generation: 1, htc_control_host_to_target_engine: Some(0), htc_control_target_to_host_engine: Some(2), htt_map_generation: 1, htt_host_to_target_engine: Some(4), htt_target_to_host_engine: Some(1), htt_endpoint_generation: 1, htt_endpoint_id: Some(5), htt_endpoint_max_message_size: 512, mmio_device_uncached: true, ce_mmio_virtual: 0x62000000, ce_mmio_physical: 0x18a40000, ce_mmio_bytes: 0xc000, dma_virtual: 0x63000000, dma_iova: 0x80000000, dma_bytes: 0x20000, dma_uncached: true, dma_exclusive: true, dma_translated: true, smmu_route_slot: Some(7), smmu_route_id: 0x240, smmu_route_mask: 1, smmu_route_sids: [0x240,0x241], smmu_route_translated: true, smmu_context_bank: Some(4), smmu_context_s1_translation_enabled: true, smmu_context_enabled: true } }",
            "}",
            "pub struct CeCodegenGate;",
            "unsafe impl qcom_ce_v3_native_io::Gate for CeCodegenGate {",
            " type Guard<'a> = CeCodegenGuard;",
            " fn lock(&mut self) -> Result<Self::Guard<'_>,qcom_ce_v3_regs::Error> { Ok(CeCodegenGuard) }",
            "}",
            "pub type CeNativeIo = qcom_ce_v3_native_io::NativeIo<CeCodegenGate>;",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_read_probe(io: &mut CeNativeIo, offset: usize) -> Result<u32,qcom_ce_v3_regs::Error> { qcom_ce_v3_regs::RegisterIo::read32(io,offset) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_write_probe(io: &mut CeNativeIo, offset: usize, value: u32) -> Result<(),qcom_ce_v3_regs::Error> { qcom_ce_v3_regs::RegisterIo::write32(io,offset,value) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_publish_wmi_probe(io: &mut CeNativeIo, generation: u64, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, plan: qcom_wmi_ce::WmiCeTxPlan<'_>, cursor: &mut qcom_ce_v3::RingCursor) -> Result<(),qcom_ce_v3_native_io::TxPublishError> { io.publish_wmi_tx(generation,profile,rings,plan,cursor) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_publish_key_probe(io: &mut CeNativeIo, plan: sunfish_qcacld_key::QcacldKeyInstallCePlan<'_>, active_generation: u64, association_generation: u32, startup: &sunfish_qcacld_wmi_rx::SunfishWmiStartupSession, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, cursor: &mut qcom_ce_v3::RingCursor, credits: &mut qcom_htc::HtcCreditLedger, sensitive: &mut qcom_wmi_ce::SensitiveTxBufferLedger<2>, tracker: &mut sunfish_qcacld_key::QcacldCcmpKeyInstallTracker) -> Result<(),sunfish_qcacld_key_native::KeyInstallPublishError> { sunfish_qcacld_key_native::publish_qcacld_key_install(io,plan,active_generation,association_generation,startup,profile,rings,cursor,credits,sensitive,tracker) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_post_wmi_rx_probe(io: &mut CeNativeIo, generation: u64, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, plan: qcom_wmi_ce::WmiCeRxPostPlan, cursor: &mut qcom_ce_v3::RingCursor, ledger: &mut qcom_wmi_ce::RxBufferLedger) -> Result<(),qcom_ce_v3_native_io::RxPostError> { io.post_wmi_rx(generation,profile,rings,plan,cursor,ledger) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_consume_wmi_rx_probe(io: &mut CeNativeIo, generation: u64, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, cursor: &mut qcom_ce_v3::RingCursor, ledger: &mut qcom_wmi_ce::RxBufferLedger, output: &mut [u8]) -> Result<qcom_ce_v3::RxCompletion,qcom_ce_v3_native_io::RxConsumeError> { io.consume_wmi_rx(generation,profile,rings,cursor,ledger,output) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_wmi_rx_irq_probe(io: &mut CeNativeIo, generation: u64, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, cursor: &qcom_ce_v3::RingCursor, enabled: bool) -> Result<(),qcom_ce_v3_native_io::RxIrqError> { io.set_wmi_rx_irq(generation,profile,rings,cursor,enabled) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_wmi_ce2_irq_probe(io: &mut CeNativeIo, generation: u64, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, cursor: &mut qcom_ce_v3::RingCursor, rx_ledger: &mut qcom_wmi_ce::RxBufferLedger, endpoint_id: u8, max_message_size: usize, credits: &mut qcom_htc::HtcCreditLedger, outstanding: &mut sunfish_qcacld_mgmt_tx::MgmtTxOutstanding<4>, startup: &mut sunfish_qcacld_wmi_rx::SunfishWmiStartupSession, scratch: &mut [u8], max_completions: usize) -> Result<sunfish_qcacld_wmi_rx::WmiCe2DrainReport,sunfish_qcacld_wmi_rx_native::NativeWmiCe2ServiceError<()>> { sunfish_qcacld_wmi_rx_native::service_native_wmi_ce2_irq(io,generation,profile,rings,cursor,rx_ledger,endpoint_id,max_message_size,credits,outstanding,startup,scratch,max_completions,|_| Ok::<(),()>(())) }",
            "#[unsafe(no_mangle)]",
            "pub fn ce_native_htt_ce1_irq_probe(io: &mut CeNativeIo, generation: u64, profile: qcom_ce_v3_profile::Profile, rings: &qcom_ce_v3_profile::RingPlan, cursor: &mut qcom_ce_v3::RingCursor, rx_ledger: &mut qcom_wmi_ce::RxBufferLedger, endpoint: sunfish_qcacld_htc::HttEndpoint, scratch: &mut [u8], max_completions: usize) -> Result<sunfish_qcacld_htt_event::HttCe1DrainReport,sunfish_qcacld_htt_event_native::NativeHttCe1ServiceError<()>> { sunfish_qcacld_htt_event_native::service_native_htt_ce1_irq(io,generation,profile,rings,cursor,rx_ledger,endpoint,scratch,max_completions,|_| Ok::<(),()>(())) }",
            # This stand-in only forces compilation of the RSC volatile backend;
            # it never runs or supplies real hardware authority.
            "#[derive(Clone,Copy)] pub struct RscCodegenLease;",
            "pub struct RscCodegenGuard;",
            "impl qcom_wlan_rsc_native_io::Guard<RscCodegenLease> for RscCodegenGuard {",
            " fn observation(&self) -> qcom_wlan_rsc_native_io::Observation { qcom_wlan_rsc_native_io::Observation { identity: 1, generation: 1, fresh_boot: true, exclusive: true, first_virtual: 0x40000000, first_physical: 0x18220000, first_bytes: 0x1000, first_device_uncached: true, first_writable: true, tail_virtual: 0x40001000, tail_physical: 0x18221000, tail_bytes: 0x2000, tail_device_uncached: true, tail_read_only: true } }",
            " fn owns_lease(&self, _: &RscCodegenLease) -> bool { true }",
            "}",
            "pub struct RscCodegenGate;",
            "// SAFETY: compile-only stand-in. This type is not instantiated by any system profile and cannot authorize hardware.",
            "unsafe impl qcom_wlan_rsc_native_io::Gate<RscCodegenLease> for RscCodegenGate {",
            " type Guard<'a> = RscCodegenGuard; type Error = ();",
            " fn lock(&mut self) -> Result<Self::Guard<'_>,Self::Error> { Ok(RscCodegenGuard) }",
            "}",
            "pub type RscNativeIo = qcom_wlan_rsc_native_io::NativeRscIo<RscCodegenLease,RscCodegenGate>;",
            "#[unsafe(no_mangle)]",
            "pub fn rsc_native_read_probe(io: &mut RscNativeIo, lease: &RscCodegenLease, offset: usize) -> Result<u32,qcom_rpmh_vrm_tcs_io::AccessError<qcom_wlan_rsc_native_io::Error<()>>> { qcom_rpmh_vrm_tcs_io::GuardedRegisters::read32(io,lease,1,1,offset) }",
            "#[unsafe(no_mangle)]",
            "pub fn rsc_native_write_probe(io: &mut RscNativeIo, lease: &RscCodegenLease, offset: usize, value: u32) -> Result<(),qcom_rpmh_vrm_tcs_io::AccessError<qcom_wlan_rsc_native_io::Error<()>>> { qcom_rpmh_vrm_tcs_io::GuardedRegisters::write32(io,lease,1,1,offset,value) }",
            "#[unsafe(no_mangle)]",
            "pub fn rsc_native_snapshot_probe(io: &mut RscNativeIo, lease: &RscCodegenLease) -> Result<qcom_rpmh_handoff::Snapshot,qcom_wlan_rsc_native_io::Error<()>> { qcom_rpmh_vrm_tcs_transport::Backend::capture_stable(io,lease,1,1) }",
            "#[unsafe(no_mangle)]",
            "pub fn native_read_probe(io: &mut NativeIo<CodegenGate>, at: usize, out: &mut [u8]) -> Result<(), Fault> { io.read_rx(at, out) }",
            "#[unsafe(no_mangle)]",
            "pub fn native_publish_probe(io: &mut NativeIo<CodegenGate>, at: usize, bytes: &[u8], head: u32) -> Result<(), Fault> {",
            " io.write_tx(at, bytes)?; io.barrier(Barrier::Release)?; io.store_tx_head(head)?; io.barrier(Barrier::Complete)?; io.notify_peer()",
            "}",
            "#[unsafe(no_mangle)]",
            "pub fn native_receive_probe(adapter: &mut qcom_glink_fifo::Adapter<NativeIo<CodegenGate>>) -> Result<qcom_glink_fifo::Receive, qcom_glink_fifo::Error> { adapter.poll_receive(1) }",
            "#[unsafe(no_mangle)]",
            "pub fn native_transmit_probe(adapter: &mut qcom_glink_fifo::Adapter<NativeIo<CodegenGate>>) -> Result<qcom_glink_fifo::Transmit, qcom_glink_fifo::Error> { adapter.publish_next(1) }",
            # Force concrete handler/paired-owner code generation too. The
            # stand-in cannot perform storage or ownership actions; it is a
            # compilation harness, never a native backend or readiness proof.
            "use qcom_modem_handlers as h;",
            "pub struct CodegenActions(pub h::Observation);",
            "impl h::Backend for CodegenActions {",
            " type Receipt = h::Grant;",
            " fn observe(&mut self) -> Result<h::Observation,h::Fault> { Ok(self.0) }",
            " fn disk(&mut self, _: qcom_modem_services::Partition) -> Result<h::Disk,h::Fault> { Err(h::Fault::Failed) }",
            " fn zero(&mut self, _: h::Pool, _: u32, _: usize) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            " fn grant(&mut self, _: h::Grant) -> Result<h::Grant,h::Fault> { Err(h::Fault::Failed) }",
            " fn info(&mut self, receipt: &h::Grant) -> Result<h::Grant,h::Fault> { Ok(*receipt) }",
            " fn reclaim(&mut self, _: &h::Grant) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            " fn disk_read(&mut self, _: qcom_modem_services::Partition, _: u64, _: &mut [u8]) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            " fn disk_write(&mut self, _: qcom_modem_services::Partition, _: u64, _: &[u8]) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            " fn sync(&mut self, _: qcom_modem_services::Partition) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            " fn pool_read(&mut self, _: &h::Grant, _: u32, _: &mut [u8]) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            " fn pool_write(&mut self, _: &h::Grant, _: u32, _: &[u8]) -> Result<(),h::Fault> { Err(h::Fault::Failed) }",
            "}",
            "pub struct CodegenPoolGate(pub qcom_modem_pools::Observation);",
            "impl qcom_modem_pool_native::Gate for CodegenPoolGate { fn observe(&mut self) -> Result<qcom_modem_pools::Observation,h::Fault> { Ok(self.0) } }",
            "pub struct CodegenSmc;",
            "impl qcom_modem_pool_native::Smc for CodegenSmc { fn invoke(&mut self, _: &qcom_modem_pools::Call) -> Result<[u64;8],h::Fault> { Err(h::Fault::Uncertain) } }",
            "pub type CodegenPoolNative = qcom_modem_pool_native::Native<CodegenPoolGate,CodegenSmc>;",
            "#[unsafe(no_mangle)]",
            "pub fn pool_native_stage_probe(io: &mut CodegenPoolNative, call: &qcom_modem_pools::Call) -> Result<(),h::Fault> { qcom_modem_pools::Io::stage(io,call) }",
            "#[unsafe(no_mangle)]",
            "pub fn pool_grant_probe(p: &mut qcom_modem_pools::Pools<CodegenPoolNative>, g: h::Grant) -> Result<qcom_modem_pools::Receipt,h::Fault> { p.grant(g) }",
            "#[unsafe(no_mangle)]",
            "pub fn pool_reclaim_probe(p: &mut qcom_modem_pools::Pools<CodegenPoolNative>, r: &qcom_modem_pools::Receipt) -> Result<(),h::Fault> { p.reclaim(r) }",
            "#[unsafe(no_mangle)]",
            "pub fn pool_zero_probe(p: &mut qcom_modem_pools::Pools<CodegenPoolNative>, at: u32, n: usize) -> Result<(),h::Fault> { p.zero(h::Pool::Heap,at,n) }",
            "pub struct CodegenDisk(pub qcom_modem_storage::Observation);",
            "impl qcom_modem_storage::Io for CodegenDisk {",
            " fn observe(&mut self) -> Result<qcom_modem_storage::Observation,h::Fault> { Ok(self.0) }",
            " fn read(&mut self, _: qcom_modem_services::Partition, _: u64, _: &mut [u8]) -> Result<usize,h::Fault> { Err(h::Fault::Failed) }",
            " fn write(&mut self, _: qcom_modem_services::Partition, _: u64, _: &[u8]) -> Result<usize,h::Fault> { Err(h::Fault::Uncertain) }",
            " fn flush(&mut self, _: qcom_modem_services::Partition) -> Result<(),h::Fault> { Err(h::Fault::Uncertain) }",
            "}",
            "pub type ComposedBackend = qcom_modem_backend::Backend<CodegenPoolNative,CodegenDisk>;",
            "#[unsafe(no_mangle)]",
            "pub fn composed_handlers_probe(handlers: &mut h::Handlers<ComposedBackend>) -> Result<h::Progress,h::Error> { handlers.step() }",
            "#[unsafe(no_mangle)]",
            "pub fn composed_owner_handlers_probe(owner: &mut qcom_modem_owner::Owner<NativeIo<CodegenGate>>, handlers: &mut h::Handlers<ComposedBackend>) -> Result<qcom_modem_owner::CombinedProgress,qcom_modem_owner::Error> { owner.notified_with_handlers(8,handlers,&mut CodegenInterrupts) }",
            "pub struct CodegenInterrupts;",
            "impl qcom_modem_owner::Interrupts for CodegenInterrupts {",
            " fn acknowledge(&mut self, _: u8) -> Result<(),qcom_modem_owner::Error> { Err(qcom_modem_owner::Error::Ack) }",
            " fn schedule(&mut self) -> Result<(),qcom_modem_owner::Error> { Err(qcom_modem_owner::Error::Schedule) }",
            "}",
            "#[unsafe(no_mangle)]",
            "pub fn handlers_step_probe(handlers: &mut h::Handlers<CodegenActions>) -> Result<h::Progress,h::Error> { handlers.step() }",
            "#[unsafe(no_mangle)]",
            "pub fn owner_handlers_probe(owner: &mut qcom_modem_owner::Owner<NativeIo<CodegenGate>>, handlers: &mut h::Handlers<CodegenActions>) -> Result<qcom_modem_owner::CombinedProgress,qcom_modem_owner::Error> { owner.notified_with_handlers(8, handlers, &mut CodegenInterrupts) }",
            "",
        ]))
        subprocess.run(
            [rustc, "--edition=2024", "-D", "warnings", "--crate-type=lib",
             "--crate-name=sunfish_glink_native", "--target=aarch64-unknown-none",
             "--emit=link,asm", "--out-dir", str(out), str(native)],
            cwd=ROOT, check=True,
        )
        assembly = (out / "sunfish_glink_native.s").read_text()
        for instruction in (r"\bdsb\s+sy\b", r"\bldrb\b", r"\bstrb\b", r"\bstr\b"):
            if re.search(instruction, assembly) is None:
                raise SystemExit(f"native AArch64 codegen missing: {instruction}")
        for entry in ("ipa_linux_scm_hold_probe", "ipa_linux_metadata_prepare_probe", "ipa_linux_loader_start_probe", "ipa_shared_attach_probe", "ipa_shared_stage_probe", "ipa_smem_allocate_probe", "handlers_step_probe", "owner_handlers_probe", "pool_native_stage_probe", "pool_grant_probe", "pool_reclaim_probe", "pool_zero_probe", "composed_handlers_probe", "composed_owner_handlers_probe", "rsc_native_read_probe", "rsc_native_write_probe", "rsc_native_snapshot_probe"):
            if entry not in assembly:
                raise SystemExit(f"AArch64 handler codegen missing: {entry}")
    print("Host routing/GLINK/FIFO tests and AArch64 no_std core/native compilation passed.")
    print("Native AArch64 codegen contains system barriers and volatile byte/cursor stores.")
    print("Native IPA metadata/staging and SCM hold paths generated with compile-only SCM/MMIO stand-ins; no secure-call authority or hardware startup is claimed.")
    print("RSC volatile read/write and stable-snapshot paths generated for AArch64; the stand-in gate is compile-only and provides no hardware authority.")
    print("Concrete composed storage/pool backend, handlers and owner code generated for AArch64; native storage and ownership authority remain unverified.")


if __name__ == "__main__":
    main()
