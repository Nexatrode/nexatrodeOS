#!/usr/bin/env python3
"""Host-only checks for modem inventory provenance claims."""

from __future__ import annotations

import copy
import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VALIDATOR_PATH = ROOT / "platforms" / "sunfish" / "validate-wlan-resources.py"
MANIFEST_PATH = ROOT / "platforms" / "sunfish" / "wlan-resources.json"
SPEC = importlib.util.spec_from_file_location("validate_wlan_resources", VALIDATOR_PATH)
assert SPEC and SPEC.loader
VALIDATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATOR)


class ModemFirmwareProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest = json.loads(MANIFEST_PATH.read_text())

    def test_observed_inventory_is_explicitly_unverified(self) -> None:
        self.assertEqual(VALIDATOR.check_modem_firmware_inventory(self.manifest), [])
        release = self.manifest["modem_firmware_observation"]["firmware_release_match"]
        self.assertEqual(release["status"], "unverified")

    def test_matched_release_requires_immutable_evidence(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        release = manifest["modem_firmware_observation"]["firmware_release_match"]
        release["status"] = "matched"
        errors = VALIDATOR.check_modem_firmware_inventory(manifest)
        self.assertTrue(any("requires immutable-source" in error for error in errors))

    def test_runtime_build_label_cannot_be_omitted(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        del manifest["modem_firmware_observation"]["modem_version_info"]["modem_image_build_id"]
        errors = VALIDATOR.check_modem_firmware_inventory(manifest)
        self.assertTrue(any("missing modem_image_build_id" in error for error in errors))

    def test_wlan_file_inventory_has_four_device_local_identities(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        files = self.manifest["wlan_firmware_observation"]["files"]
        self.assertEqual(len(files), 4)

    def test_wlan_inventory_rejects_malformed_hash(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["wlan_firmware_observation"]["files"][0]["sha256"] = "not-a-hash"
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("invalid WLAN file SHA-256" in error for error in errors))

    def test_wlan_runtime_version_report_is_present(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        runtime = self.manifest["wlan_firmware_observation"]["runtime_reported"]
        self.assertEqual(runtime["firmware_version"], "3.2.2.9.558.0")
        self.assertEqual(runtime["driver_version"], "5.2.03.9H")
        self.assertEqual(runtime["driver_version"], runtime["matched_source_version_constant"])

    def test_current_lineage_qdf_identity_is_not_ath10k_selector_data(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        identity = self.manifest["wlan_firmware_observation"]["lineage_runtime_identity"]
        self.assertEqual(identity["qdf_board_id"], 0)
        self.assertEqual(identity["qdf_board_version"], [18, 0, 0, 0, 0])
        self.assertIn("not ath10k SNOC QMI", identity["scope"])
        self.assertIn("cannot select", identity["scope"])

    def test_current_lineage_qdf_identity_mutation_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["wlan_firmware_observation"]["lineage_runtime_identity"]["qdf_board_id"] = 2
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("captured QDF/QCACLD values" in error for error in errors))

    def test_official_package_checksum_and_proprietary_use_gate(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        package = self.manifest["wlan_firmware_observation"]["official_package_reference"]
        self.assertEqual(package["observed_archive_sha256"], package["published_archive_sha256"])
        self.assertEqual(package["use_permission"], "unverified")
        review = package["license_review"]
        self.assertEqual(review["archive_sha256"], package["published_archive_sha256"])
        self.assertEqual(review["sections_reviewed"], ["1(c)", "2(a)", "3(b)"])
        self.assertFalse(review["handset_wlan_payloads_compared"])
        self.assertEqual(review["nexatrodeos_permission"], "not_established")

    def test_official_package_license_review_rejects_implied_payload_match(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["wlan_firmware_observation"]["official_package_reference"][
            "license_review"
        ]["handset_wlan_payloads_compared"] = True
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("must not claim a handset-payload comparison" in error for error in errors))

    def test_official_package_cannot_be_marked_permission_verified_by_hash(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["wlan_firmware_observation"]["official_package_reference"]["use_permission"] = "verified"
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("must remain unverified" in error for error in errors))

    def test_generic_wcn3990_candidate_is_no_go_for_sunfish_hardware_test(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        candidate = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]
        firmware = candidate["linux_firmware_candidate"]
        self.assertEqual(candidate["status"], "reference_candidate_not_device_verified")
        self.assertFalse(firmware["observed_wlanmdsp_match"])
        self.assertNotEqual(firmware["wlanmdsp_sha256"], firmware["observed_wlanmdsp_sha256"])
        self.assertEqual(
            firmware["sunfish_reference_test_decision"],
            "no_go_generic_bundle_platform_scope_hash_and_loader_do_not_match",
        )
        self.assertIn("a0142c57045701b7557c3060af5c4246c420e4d8", firmware["wlanmdsp_platform_scope"])
        self.assertIn("NO-GO", firmware["test_readiness_note"])

    def test_linux_firmware_candidate_integrity_is_complete_and_pinned(self) -> None:
        candidate = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]
        files = {item["path"]: item for item in candidate["candidate_file_integrity"]}
        self.assertEqual(
            set(files),
            {
                "ath10k/WCN3990/hw1.0/board-2.bin",
                "ath10k/WCN3990/hw1.0/firmware-5.bin",
                "ath10k/WCN3990/hw1.0/wlanmdsp.mbn",
                "ath10k/WCN3990/hw1.0/notice.txt_wlanmdsp",
            },
        )
        self.assertEqual(files["ath10k/WCN3990/hw1.0/firmware-5.bin"]["size_bytes"], 60)
        self.assertEqual(files["ath10k/WCN3990/hw1.0/firmware-5.bin"]["git_mode"], "100644")

    def test_latest_linux_firmware_qcm_license_paths_match_main_snapshot(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        latest = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]["latest_upstream_snapshot"]
        qcm2290 = latest["qcm2290"]
        self.assertEqual(qcm2290["license_file"], "LICENSES/LICENSE.qcom")
        self.assertEqual(qcm2290["notice_file"], "LICENSES/NOTICE.qcom")
        self.assertIn("only with platforms incorporating Qualcomm Technologies chipsets", qcm2290["license_scope_summary"])
        self.assertIn("grants no patent rights", qcm2290["license_scope_summary"])
        self.assertIn("require separate review", qcm2290["license_review_status"])
        self.assertEqual(len(qcm2290["files"]), 2)

    def test_latest_linux_firmware_shikra_update_is_pinned_and_not_mislabeled_sunfish(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        latest = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]["latest_upstream_snapshot"]
        self.assertEqual(latest["commit"], "412eb7861d40b9dc3dfa01d4059d4b5c414b0307")
        self.assertEqual(latest["latest_shikra_update_commit"], "72f3b81c89fbc52677b9dbbfa92cde0ae00a36e2")
        shikra = latest["shikra"]
        self.assertEqual(shikra["version"], "WLAN.HL.3.3.7.c5-00226-QCAHLSWMTPL-1")
        self.assertIn("stability issues", shikra["update_note"])
        wlanmdsp = next(item for item in shikra["files"] if item["path"].endswith("/wlanmdsp.mbn"))
        self.assertEqual(wlanmdsp["size_bytes"], 3808072)
        self.assertEqual(
            wlanmdsp["sha256"],
            "ea1787533f969c084c24292defdcc88d75d71955bea7e12ffee2b5fb16248350",
        )
        self.assertEqual(shikra["status"], "Separate platform-specific candidate; not a Sunfish match.")

    def test_current_wcn3990_board_file_has_no_sunfish_selector(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        candidate = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]
        audit = candidate["current_main_board_selector_audit"]
        self.assertEqual(audit["source_commit"], "bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0")
        self.assertEqual(audit["path"], "ath10k/WCN3990/hw1.0/board-2.bin")
        self.assertEqual(audit["size_bytes"], 893528)
        self.assertEqual(
            audit["sha256"],
            "c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d",
        )
        self.assertFalse(audit["google_sunfish_selector_present"])
        self.assertFalse(audit["sunfish_selector_present"])
        self.assertEqual(audit["sunfish_hardware_test_decision"], "no_go_no_sunfish_board_selector")

    def test_linux_firmware_api5_file_is_flags_metadata_not_image_payload(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        candidate = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]
        self.assertEqual(candidate["firmware5_artifact_kind"], "ath10k_fw_feature_flags")
        self.assertFalse(candidate["firmware5_is_firmware_payload"])
        self.assertEqual(set(candidate["firmware5_features"]), {"wowlan", "mgmt-tx-by-ref", "non-bmi"})
        self.assertIn("out of band", candidate["firmware5_non_bmi_effect"])
        self.assertIn("does not establish compatibility", candidate["wlanmdsp_platform_scope"])

    def test_linux_firmware_api5_metadata_cannot_be_reclassified_as_firmware_image(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        candidate = manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]
        candidate["firmware5_is_firmware_payload"] = True
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("feature metadata" in error for error in errors))

    def test_linux_firmware_candidate_integrity_mutations_are_rejected(self) -> None:
        for mutate in (
            lambda files: files.pop(),
            lambda files: files[0].update(sha256="0" * 64),
        ):
            manifest = copy.deepcopy(self.manifest)
            files = manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]["candidate_file_integrity"]
            mutate(files)
            errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
            self.assertTrue(any("candidate" in error for error in errors))

    def test_sm7150_mainline_sunfish_binding_is_not_runtime_verified(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        candidate = self.manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]
        device_tree = candidate["sunfish_device_tree_evidence"]
        mainline = device_tree["pinned_sm7150_mainline"]
        self.assertEqual(device_tree["reserved_msa_base"], "0x98d00000")
        self.assertEqual(device_tree["reserved_msa_size"], "0x200000")
        self.assertEqual(device_tree["driver_msa_allocation_size"], "0x100000")
        self.assertFalse(mainline["device_runtime_verified"])

    def test_upstream_ath10k_mismatch_cannot_be_claimed_as_a_match(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        firmware = manifest["wlan_firmware_observation"]["upstream_ath10k_candidate"]["linux_firmware_candidate"]
        firmware["observed_wlanmdsp_sha256"] = firmware["wlanmdsp_sha256"]
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("mismatch must not be recorded" in error for error in errors))

    def test_wlan_source_version_mismatch_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["wlan_firmware_observation"]["runtime_reported"]["matched_source_version_constant"] = "different"
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("does not match its recorded source" in error for error in errors))

    def test_wlan_config_match_is_pinned_and_hash_identical(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        files = self.manifest["wlan_firmware_observation"]["files"]
        config = next(item for item in files if item["path"].endswith("WCNSS_qcom_cfg.ini"))
        match = config["source_match"]
        self.assertEqual(match["status"], "exact_sha256_match")
        self.assertEqual(config["sha256"], match["source_sha256"])

    def test_firmware_rehost_matches_are_scoped_and_license_is_unverified(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_firmware_inventory(self.manifest), [])
        files = self.manifest["wlan_firmware_observation"]["files"]
        by_path = {item["path"]: item for item in files}
        for path in ("/vendor/firmware/wlanmdsp.mbn", "/vendor/firmware/bdwlan-sunfish.bin"):
            match = by_path[path]["source_match"]
            self.assertEqual(match["source_class"], "community_firmware_rehost")
            self.assertEqual(match["identity_scope"], "byte_identity_only")
            self.assertEqual(match["license_status"], "not_declared_in_repository")
            self.assertEqual(match["source_sha256"], by_path[path]["sha256"])
        evt = by_path["/vendor/firmware/bdwlan-sunfish-EVT1.0.bin"]
        self.assertNotIn("source_match", evt)

    def test_wlan_match_cannot_claim_license_or_release_authentication(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        wlanmdsp = manifest["wlan_firmware_observation"]["files"][0]
        wlanmdsp["source_match"]["license_status"] = "approved"
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("licensing must remain explicitly unverified" in error for error in errors))

    def test_wlan_source_match_rejects_mutated_digest(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        config = manifest["wlan_firmware_observation"]["files"][3]
        config["source_match"]["source_sha256"] = "0" * 64
        errors = VALIDATOR.check_wlan_firmware_inventory(manifest)
        self.assertTrue(any("source SHA-256 does not match" in error for error in errors))


class IcnssPowerProfileTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest = json.loads(MANIFEST_PATH.read_text())
        self.power = self.manifest["icnss"]["power"]

    def test_power_profile_matches_lineage_source(self) -> None:
        self.assertEqual(VALIDATOR.check_icnss_power_profile(self.power), [])

    def test_supply_inventory_order_is_independent_of_enable_order(self) -> None:
        power = copy.deepcopy(self.power)
        power["optional_supplies"].reverse()
        self.assertEqual(VALIDATOR.check_icnss_power_profile(power), [])

    def test_power_sequence_and_rail_range_drift_are_rejected(self) -> None:
        power = copy.deepcopy(self.power)
        power["lineage_power_on_sequence"][0], power["lineage_power_on_sequence"][1] = (
            power["lineage_power_on_sequence"][1],
            power["lineage_power_on_sequence"][0],
        )
        power["lineage_power_on_sequence"][3]["max_microvolts"] = 3_300_000
        errors = VALIDATOR.check_icnss_power_profile(power)
        self.assertTrue(any("rail order or requested voltage ranges" in error for error in errors))

    def test_dt_cells_mapping_and_measurement_claims_are_checked(self) -> None:
        power = copy.deepcopy(self.power)
        power["source_dts_vdd_3_3_ch0_config_cells"][1] = "0x327358"
        power["rpmh_resources"]["vdd-3.3-ch1"] = "ldoc10"
        power["voltage_request_semantics"] = "the output is measured at 3.312 V"
        power["running_driver_source_match"] = "verified"
        errors = VALIDATOR.check_icnss_power_profile(power)
        self.assertTrue(any("vdd-3.3-ch0 pinned source DTS range" in error for error in errors))
        self.assertTrue(any("rail-to-RPMh resource mapping" in error for error in errors))
        self.assertTrue(any("not be represented as measured" in error for error in errors))
        self.assertTrue(any("identity must remain explicitly unverified" in error for error in errors))

    def test_shared_rpmh_vote_policy_cannot_be_simplified_to_single_client(self) -> None:
        power = copy.deepcopy(self.power)
        power["rpmh_vote_semantics"]["aggregation"] = "single WLAN vote wins"
        errors = VALIDATOR.check_icnss_power_profile(power)
        self.assertTrue(any("shared-vote behavior" in error for error in errors))

    def test_shared_rpmh_consumer_inventory_matches_both_captured_dtb_profiles(self) -> None:
        self.assertEqual(VALIDATOR.check_icnss_power_profile(self.power), [])
        consumers = self.power["rpmh_supply_consumers"]
        self.assertEqual(len(consumers), 8)
        self.assertIn(
            {
                "resource": "ldoc1",
                "node": "/soc/bt_wcn3990",
                "property": "qca,bt-vdd-xtal-supply",
            },
            consumers,
        )
        self.assertIn(
            {
                "resource": "ldoc10",
                "node": "/soc/bt_wcn3990",
                "property": "qca,bt-vdd-pa-supply",
            },
            consumers,
        )

    def test_shared_rpmh_consumer_inventory_drift_is_rejected(self) -> None:
        power = copy.deepcopy(self.power)
        power["rpmh_supply_consumers"].pop()
        errors = VALIDATOR.check_icnss_power_profile(power)
        self.assertTrue(any("direct supply-client inventory" in error for error in errors))

    def test_bluetooth_shared_rail_requests_match_both_captured_dtbs(self) -> None:
        self.assertEqual(VALIDATOR.check_icnss_bt_shared_rail_source(self.power), [])
        for path in VALIDATOR.DEFAULT_DTBS:
            with self.subTest(dtb=path):
                errors: list[str] = []
                VALIDATOR.check_icnss_bt_shared_rail_requests(
                    VALIDATOR.Dtb(path), self.power, errors
                )
                self.assertEqual(errors, [])

    def test_bluetooth_shared_rail_request_drift_is_rejected(self) -> None:
        power = copy.deepcopy(self.power)
        power["rpmh_dynamic_client_profiles"][1]["load_microamps"] = 10000
        errors = VALIDATOR.check_icnss_bt_shared_rail_source(power)
        self.assertTrue(any("Bluetooth shared-rail request profile" in error for error in errors))

    def test_all_five_rpmh_regulators_match_both_captured_dtb_profiles(self) -> None:
        self.assertEqual(VALIDATOR.check_icnss_rpmh_mode_threshold_source(), [])
        for path in VALIDATOR.DEFAULT_DTBS:
            with self.subTest(dtb=path):
                errors: list[str] = []
                VALIDATOR.check_icnss_rpmh_supply_consumers(
                    VALIDATOR.Dtb(path), self.power, errors
                )
                VALIDATOR.check_icnss_rpmh_regulator_profiles(
                    VALIDATOR.Dtb(path), errors
                )
                self.assertEqual(errors, [])

    def test_missing_icnss_regulator_device_is_rejected(self) -> None:
        power = copy.deepcopy(self.power)
        power["rpmh_resource_regulator_devices"].pop()
        errors: list[str] = []
        VALIDATOR.check_icnss_rpmh_supply_consumers(
            VALIDATOR.Dtb(VALIDATOR.DEFAULT_DTBS[0]), power, errors
        )
        self.assertTrue(any("regulator-device nodes differ" in error for error in errors))

    def test_load_mode_assumptions_preserve_zero_icnss_load_and_lpm_result(self) -> None:
        self.assertEqual(VALIDATOR.check_icnss_power_profile(self.power), [])
        assumptions = self.power["rpmh_vote_semantics"]["load_mode_assumptions"]
        self.assertEqual(set(assumptions["icnss_load_microamps_by_resource"].values()), {0})
        self.assertEqual(assumptions["expected_mode_code"], 4)
        power = copy.deepcopy(self.power)
        power["rpmh_vote_semantics"]["load_mode_assumptions"]["expected_mode_code"] = 7
        errors = VALIDATOR.check_icnss_power_profile(power)
        self.assertTrue(any("shared-vote behavior" in error for error in errors))

    def test_bluetooth_runtime_status_does_not_claim_shared_rail_ownership(self) -> None:
        self.assertEqual(VALIDATOR.check_icnss_runtime_bluetooth_status(self.power), [])
        power = copy.deepcopy(self.power)
        capture = power["lineage_runtime_bluetooth_status_capture"]
        capture["rfkill_soft_blocked"] = False
        errors = VALIDATOR.check_icnss_runtime_bluetooth_status(power)
        self.assertTrue(any("rfkill_soft_blocked" in error for error in errors))

    def test_bluetooth_source_scope_does_not_claim_runtime_enable_state(self) -> None:
        power = copy.deepcopy(self.power)
        power["rpmh_dynamic_client_source"]["scope"] = "Bluetooth is enabled in the live system"
        errors = VALIDATOR.check_icnss_bt_shared_rail_source(power)
        self.assertTrue(any("runtime-evidence scope" in error for error in errors))

    def test_dts_parser_resolves_nested_regulator_phandle_to_all_clients(self) -> None:
        dts = '''
/dts-v1/;
/ {
    soc {
        rpmh-regulator-ldoa9 {
            qcom,resource-name = "ldoa9";
            pm6150_l9: regulator-pm6150-l9 {
                phandle = <0x20>;
            };
        };
        icnss {
            vdd-cx-mx-supply = <0x20>;
        };
        bt {
            qca,bt-vdd-core-supply = <0x20>;
        };
        unrelated {
            vdd-supply = <0x21>;
        };
    };
};
'''
        consumers = VALIDATOR.rpmh_supply_consumers_from_dts(
            dts, {"ldoa9"}
        )
        self.assertEqual(
            consumers,
            (
                ("ldoa9", "/soc/bt", "qca,bt-vdd-core-supply"),
                ("ldoa9", "/soc/icnss", "vdd-cx-mx-supply"),
            ),
        )


if __name__ == "__main__":
    unittest.main()
