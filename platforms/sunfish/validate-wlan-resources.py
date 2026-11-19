#!/usr/bin/env python3
"""Fail if Sunfish WLAN resources or dynamic reserved-memory policy drift."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import subprocess
import xml.etree.ElementTree as ET
from pathlib import Path

SUNFISH_DIR = str(Path(__file__).resolve().parent)
if SUNFISH_DIR not in sys.path:
    sys.path.insert(0, SUNFISH_DIR)

from smmu_runtime_route_readback import (
    RouteReadbackError,
    parse_route_readback,
    require_route,
    require_usb_context_reassignment,
)


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name("wlan-resources.json")
DEFAULT_DTBS = [
    ROOT / ".cache/pixel-sunfish/live-runtime.dtb",
    ROOT / ".cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb",
]
ICNSS_POWER_SEQUENCE = [
    ("vdd-cx-mx", 640_000, 640_000),
    ("vdd-1.8-xo", 1_800_000, 1_800_000),
    ("vdd-1.3-rfa", 1_304_000, 1_304_000),
    ("vdd-3.3-ch1", 3_312_000, 3_312_000),
    ("vdd-3.3-ch0", 3_312_000, 3_312_000),
]
ICNSS_SOURCE_DTS_3V3_CONFIG = ["0x2dc6c0", "0x327b40"]
ICNSS_LINEAGE_SOURCE_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
SEL4_SMMU_SOURCE_COMMIT = "6e7c3b733d296cfd88d5fbf635c96e447a882374"
MICROKIT_SMMU_SOURCE_COMMIT = "ec86afdcd662b5976d11d4994acf1b11a2979882"
ICNSS_DRIVER_SOURCE_PATH = "drivers/soc/qcom/icnss.c"
ICNSS_DRIVER_SOURCE_SHA256 = (
    "ac6847c9aa6efa1495a03a879b1594169c6d616d9f38fa3921515943e2f9ab60"
)
WLAN_SMMU_ACTLR_DTS_SOURCE = (
    "arch/arm64/boot/dts/qcom/msm-arm-smmu-sdmmagpie.dtsi"
)
WLAN_SMMU_ACTLR_DTS_SHA256 = (
    "53902d14b374b1e2aed3bbc5e19df97be8a6bec8d9bae502c20a1dcf9705696d"
)
WLAN_SMMU_ACTLR_DRIVER_SOURCE = "drivers/iommu/arm-smmu.c"
WLAN_SMMU_ACTLR_DRIVER_SHA256 = (
    "54c3ce1434c1e9e048f222ba80cd3b81a726f76a29c13769ef48cee27c7d06bc"
)
WLAN_MAINLINE_SOURCE_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
WLAN_MAINLINE_WIFI_DTS_PATH = "arch/arm64/boot/dts/qcom/sm7150.dtsi"
WLAN_MAINLINE_WIFI_DTS_SHA256 = (
    "59495b0881906bb6a4708dc4b7006f8addb6714cacfe61e602e1fb18d61fab58"
)
WLAN_MAINLINE_SUNFISH_DTS_PATH = (
    "arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts"
)
WLAN_MAINLINE_SUNFISH_DTS_SHA256 = (
    "bb3757e66d2c0567cdafa81067f34e7c04d59d8da6ff9e5a463e9a86ca52cb77"
)
WLAN_LINEAGE_GENERIC_SM6150_DTS_PATH = "arch/arm64/boot/dts/qcom/sm6150.dtsi"
WLAN_LINEAGE_GENERIC_SM6150_DTS_SHA256 = (
    "6700ec8456ba8e51848210018dccf46fdb7aa9d0605179d608911d7bf2bbf9bd"
)
WLAN_LINEAGE_SUNFISH_BASE_DTS_PATH = "arch/arm64/boot/dts/qcom/sdmmagpie.dts"
WLAN_LINEAGE_SUNFISH_BASE_DTS_SHA256 = (
    "6ab514c91f776198fe7de3a2203496685becc3490280d05aa102afec496d0839"
)
WLAN_LINEAGE_SUNFISH_BASE_DTSI_PATH = "arch/arm64/boot/dts/qcom/sdmmagpie.dtsi"
WLAN_LINEAGE_SUNFISH_BASE_DTSI_SHA256 = (
    "e0b37d7cdc137503fef1f36dd6119039175c5c780bca9aaef267bb00f55fcbbb"
)
WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_PATH = "arch/arm64/boot/dts/google/Makefile"
WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_SHA256 = (
    "144ba7ea7917ed4c0b480a75999389f47a61d631d8428dda621e6094cb18a2cc"
)
WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_PATH = (
    "arch/arm64/boot/dts/google/sm7150-sunfish-dvt1.0-overlay.dts"
)
WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_SHA256 = (
    "d404388141f38728b279bf23d0c1ea280285df7323278e15f8ceeda112ca8c02"
)
ICNSS_RPMH_REGULATOR_DTS_SOURCE_PATH = (
    "arch/arm64/boot/dts/qcom/sdmmagpie-regulator.dtsi"
)
ICNSS_RPMH_REGULATOR_DTS_SOURCE_SHA256 = (
    "a041e089a2cd28f7b58a8b0be98e1889ecf4a54b8c16d404b9569be16f110958"
)
ICNSS_OF_REGULATOR_SOURCE_PATH = "drivers/regulator/of_regulator.c"
ICNSS_OF_REGULATOR_SOURCE_SHA256 = (
    "04c6cb4347fc32d9bea6f38b53f972b545ecf45bfd09b6ca57850586afd32ed0"
)
ICNSS_REGULATOR_CORE_SOURCE_PATH = "drivers/regulator/core.c"
ICNSS_REGULATOR_CORE_SOURCE_SHA256 = (
    "50e39e9fb99802b127ba529e712c2d413d3637158c914825b564067734a52e54"
)
ICNSS_RPMH_REGULATOR_SOURCE_PATH = "drivers/regulator/rpmh-regulator.c"
ICNSS_RPMH_REGULATOR_SOURCE_SHA256 = (
    "d8045626d0b0fd9ff52c1c7a66774353394eb97007d7756f167dacf394601eb4"
)
ICNSS_RPMH_MODE_THRESHOLDS = {
    "source_commit": ICNSS_LINEAGE_SOURCE_COMMIT,
    "source_path": ICNSS_RPMH_REGULATOR_DTS_SOURCE_PATH,
    "source_sha256": ICNSS_RPMH_REGULATOR_DTS_SOURCE_SHA256,
    "resource_thresholds_microamps": {
        "ldoa9": [0, 1],
        "ldoc1": [0, 10_000],
        "ldoc2": [0, 10_000],
        "ldoc10": [0, 10_000],
        "ldoc11": [0, 1],
    },
    "scope": "Sunfish SDM Magpie board regulator definition; generic Atoll regulator defaults are not the active board thresholds.",
}
ICNSS_RPMH_LOAD_RECALCULATION_SOURCE = {
    "source_path": ICNSS_REGULATOR_CORE_SOURCE_PATH,
    "source_sha256": ICNSS_REGULATOR_CORE_SOURCE_SHA256,
    "drms_load_sum": "regulator core sums every consumer uA_load plus regulator system_load",
    "enable_behavior": "_regulator_enable calls drms_uA_update before enabling, even if the consumer never called regulator_set_load",
}
ICNSS_RPMH_RESOURCES = {
    "vdd-cx-mx": "ldoa9",
    "vdd-1.8-xo": "ldoc1",
    "vdd-1.3-rfa": "ldoc2",
    "vdd-3.3-ch0": "ldoc10",
    "vdd-3.3-ch1": "ldoc11",
}
ICNSS_RPMH_SUPPLY_CONSUMERS = (
    ("ldoa9", "/soc/qcom,icnss@18800000", "vdd-cx-mx-supply"),
    ("ldoc1", "/soc/bt_wcn3990", "qca,bt-vdd-xtal-supply"),
    ("ldoc1", "/soc/qcom,icnss@18800000", "vdd-1.8-xo-supply"),
    ("ldoc2", "/soc/bt_wcn3990", "qca,bt-vdd-core-supply"),
    ("ldoc2", "/soc/qcom,icnss@18800000", "vdd-1.3-rfa-supply"),
    ("ldoc10", "/soc/bt_wcn3990", "qca,bt-vdd-pa-supply"),
    ("ldoc10", "/soc/qcom,icnss@18800000", "vdd-3.3-ch0-supply"),
    ("ldoc11", "/soc/qcom,icnss@18800000", "vdd-3.3-ch1-supply"),
)
ICNSS_BT_SHARED_RAIL_REQUESTS = (
    (
        "ldoc1",
        "qca,bt-vdd-xtal-supply",
        "qca,bt-vdd-xtal-voltage-level",
        "qca,bt-vdd-xtal-current-level",
        1_700_000,
        1_900_000,
        1,
    ),
    (
        "ldoc2",
        "qca,bt-vdd-core-supply",
        "qca,bt-vdd-core-voltage-level",
        "qca,bt-vdd-core-current-level",
        1_245_000,
        1_350_000,
        1,
    ),
    (
        "ldoc10",
        "qca,bt-vdd-pa-supply",
        "qca,bt-vdd-pa-voltage-level",
        "qca,bt-vdd-pa-current-level",
        3_200_000,
        3_400_000,
        1,
    ),
)
ICNSS_BT_SHARED_RAIL_SOURCE = {
    "source_commit": ICNSS_LINEAGE_SOURCE_COMMIT,
    "source_path": "drivers/bluetooth/bluetooth-power.c",
    "source_sha256": "eee8e57070761eb2888bf746f844504ffdc8e98bd4c6149c16b32f32a3ffbeca",
    "enable_order": "set voltage range, set load, then enable each configured regulator",
    "disable_order": "disable each regulator, then request minimum voltage 0 and load 0",
    "mode_selection": "the regulator core sums all consumer loads before selecting the highest supported mode whose threshold is at most the total load; the BT 0/1 uA request alone does not determine the final mode",
    "scope": "Configured Bluetooth requests only; DT does not establish runtime enable state, and the running driver identity is unverified.",
}
ICNSS_BT_SHARED_RAIL_VRM_NODES = {
    "ldoc1": ("/soc/rpmh-regulator-ldoc1", "/soc/rpmh-regulator-ldoc1/regulator-pm6150l-l1"),
    "ldoc2": ("/soc/rpmh-regulator-ldoc2", "/soc/rpmh-regulator-ldoc2/regulator-pm6150l-l2"),
    "ldoc10": ("/soc/rpmh-regulator-ldoc10", "/soc/rpmh-regulator-ldoc10/regulator-pm6150l-l10"),
}
ICNSS_RPMH_VRM_PROFILES = {
    "ldoa9": (
        "/soc/rpmh-regulator-ldoa9",
        "/soc/rpmh-regulator-ldoa9/regulator-pm6150-l9",
        [0, 1],
    ),
    "ldoc1": (
        "/soc/rpmh-regulator-ldoc1",
        "/soc/rpmh-regulator-ldoc1/regulator-pm6150l-l1",
        [0, 10_000],
    ),
    "ldoc2": (
        "/soc/rpmh-regulator-ldoc2",
        "/soc/rpmh-regulator-ldoc2/regulator-pm6150l-l2",
        [0, 10_000],
    ),
    "ldoc10": (
        "/soc/rpmh-regulator-ldoc10",
        "/soc/rpmh-regulator-ldoc10/regulator-pm6150l-l10",
        [0, 10_000],
    ),
    "ldoc11": (
        "/soc/rpmh-regulator-ldoc11",
        "/soc/rpmh-regulator-ldoc11/regulator-pm6150l-l11",
        [0, 1],
    ),
}
ICNSS_BT_SHARED_RAIL_ICNSS_SUPPLIES = {
    "ldoc1": "vdd-1.8-xo-supply",
    "ldoc2": "vdd-1.3-rfa-supply",
    "ldoc10": "vdd-3.3-ch0-supply",
}
ICNSS_RPMH_LOAD_MODE_ASSUMPTIONS = {
    "icnss_load_source_path": ICNSS_DRIVER_SOURCE_PATH,
    "icnss_load_source_sha256": ICNSS_DRIVER_SOURCE_SHA256,
    "icnss_load_microamps_by_resource": {
        "ldoa9": 0,
        "ldoc1": 0,
        "ldoc2": 0,
        "ldoc10": 0,
        "ldoc11": 0,
    },
    "bluetooth_load_microamps_by_resource": {
        "ldoc1": 1,
        "ldoc2": 1,
        "ldoc10": 1,
    },
    "system_load_property": "regulator-system-load",
    "system_load_source_path": ICNSS_OF_REGULATOR_SOURCE_PATH,
    "system_load_source_sha256": ICNSS_OF_REGULATOR_SOURCE_SHA256,
    "system_load_property_present_on_icnss_regulators": False,
    "expected_mode_code": 4,
    "expected_mode_name": "RPMH_REGULATOR_MODE_LPM",
    "scope": "Source-derived request reduction for the five ICNSS regulator devices; this is not a live Apps RSC vote/cache readback or authority to publish votes.",
}
ICNSS_RPMH_VOTE_SEMANTICS = {
    "source_commit": ICNSS_LINEAGE_SOURCE_COMMIT,
    "source_path": "drivers/regulator/rpmh-regulator.c",
    "source_sha256": ICNSS_RPMH_REGULATOR_SOURCE_SHA256,
    "mode_threshold_source": ICNSS_RPMH_MODE_THRESHOLDS,
    "load_recalculation_source": ICNSS_RPMH_LOAD_RECALCULATION_SOURCE,
    "load_mode_assumptions": ICNSS_RPMH_LOAD_MODE_ASSUMPTIONS,
    "aggregation": "The regulator core first combines each regulator device's consumer requests and sums loads for mode selection; for each shared RPMh resource, the provider then max-aggregates valid registers across regulator devices associated with that exact resource, separately for active and sleep sets.",
    "active_ack": "Upward active requests wait for ACK; a voltage request may also wait when its new maximum is below the previously requested voltage.",
    "native_write_gate": "A trusted service must arbitrate all requesters and preserve their contributions; MMIO readback is not a per-client vote ledger.",
}


class Dtb:
    def __init__(self, path: Path):
        self.path = path

    def get(self, *args: str) -> str:
        return subprocess.check_output(
            ["fdtget", str(self.path), *args], text=True, stderr=subprocess.PIPE
        ).strip()

    def has(self, node: str, prop: str) -> bool:
        try:
            return prop in self.get("-p", node).splitlines()
        except subprocess.CalledProcessError:
            return False

    def strings(self, node: str, prop: str) -> list[str]:
        return self.get("-t", "s", node, prop).split()

    def cells(self, node: str, prop: str) -> list[int]:
        return [int(value, 16) for value in self.get("-t", "x", node, prop).split()]


def dts_nodes_from_text(dts_text: str) -> dict[str, dict[str, str | list[int]]]:
    """Parse the simple property forms needed from dtc's decompiled output."""
    nodes: dict[str, dict[str, str | list[int]]] = {}
    stack: list[str] = []
    node_header = re.compile(r"^(.+?)\s*\{\s*$")
    cell_property = re.compile(r"^([\w,.-]+)\s*=\s*<([^>]*)>;\s*$")
    string_property = re.compile(r'^([\w,.-]+)\s*=\s*"([^"\n]+)";\s*$')

    for line in dts_text.splitlines():
        statement = line.strip()
        if statement == "/ {":
            stack.clear()
            continue
        if statement == "};":
            if stack:
                stack.pop()
            continue

        header = node_header.match(statement)
        if header is not None:
            declaration = header.group(1).strip()
            node_name = declaration.rsplit(":", 1)[-1].strip()
            if "=" not in node_name:
                stack.append(node_name)
            continue

        path = "/" + "/".join(stack)
        cells = cell_property.match(statement)
        if cells is not None:
            try:
                value: str | list[int] = [
                    int(cell, 0) for cell in cells.group(2).split()
                ]
            except ValueError:
                continue
            nodes.setdefault(path, {})[cells.group(1)] = value
            continue
        string = string_property.match(statement)
        if string is not None:
            nodes.setdefault(path, {})[string.group(1)] = string.group(2)

    return nodes


def rpmh_resource_regulators_from_dts(
    dts_text: str, resource_names: set[str]
) -> tuple[tuple[str, str], ...]:
    """List regulator devices grouped into each selected shared RPMh resource."""
    nodes = dts_nodes_from_text(dts_text)
    regulators = []
    for node_path, properties in nodes.items():
        resource = properties.get("qcom,resource-name")
        compatible = properties.get("compatible")
        if (
            isinstance(resource, str)
            and resource in resource_names
            and compatible == "qcom,rpmh-vrm-regulator"
        ):
            regulators.append((resource, node_path))
    return tuple(sorted(regulators))


def rpmh_supply_consumers_from_dts(
    dts_text: str, resource_names: set[str]
) -> tuple[tuple[str, str, str], ...]:
    """Resolve every direct regulator supply reference for selected RPMh rails."""
    nodes = dts_nodes_from_text(dts_text)

    phandle_resources: dict[int, str] = {}
    for provider_path, properties in nodes.items():
        if not provider_path.rsplit("/", 1)[-1].startswith("rpmh-regulator-"):
            continue
        resource = properties.get("qcom,resource-name")
        if not isinstance(resource, str):
            continue
        prefix = provider_path.rstrip("/") + "/"
        for child_path, child_properties in nodes.items():
            if child_path != provider_path and not child_path.startswith(prefix):
                continue
            phandles = child_properties.get("phandle")
            if isinstance(phandles, list) and len(phandles) == 1:
                phandle_resources[phandles[0]] = resource

    consumers: set[tuple[str, str, str]] = set()
    for node_path, properties in nodes.items():
        for property_name, value in properties.items():
            if not property_name.endswith("-supply") or not isinstance(value, list) or not value:
                continue
            resource = phandle_resources.get(value[0])
            if resource in resource_names:
                consumers.add((resource, node_path, property_name))
    return tuple(sorted(consumers))


def check_icnss_rpmh_supply_consumers(
    dtb: Dtb, power: dict, errors: list[str]
) -> None:
    """Ensure each DTB retains the complete direct client list for ICNSS rails."""
    try:
        dts_text = subprocess.check_output(
            ["dtc", "-I", "dtb", "-O", "dts", str(dtb.path)],
            text=True,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        fail(errors, f"cannot enumerate RPMh supply consumers in {dtb.path}: {exc}")
        return

    resources = set(ICNSS_RPMH_RESOURCES.values())
    actual_regulators = rpmh_resource_regulators_from_dts(dts_text, resources)
    manifest_regulators = power.get("rpmh_resource_regulator_devices")
    if not isinstance(manifest_regulators, list) or any(
        not isinstance(entry, dict)
        or not isinstance(entry.get("resource"), str)
        or not isinstance(entry.get("node"), str)
        for entry in manifest_regulators
    ):
        fail(errors, "RPMh shared-resource regulator-node manifest is malformed")
        expected_regulators = ()
    else:
        expected_regulators = tuple(
            sorted((entry["resource"], entry["node"]) for entry in manifest_regulators)
        )
    source_regulators = tuple(
        sorted(
            (resource, profile[0])
            for resource, profile in ICNSS_RPMH_VRM_PROFILES.items()
        )
    )
    if expected_regulators != source_regulators:
        fail(errors, "ICNSS RPMh regulator-device nodes differ from the five-rail source profile")
    if actual_regulators != expected_regulators:
        fail(
            errors,
            f"{dtb.path}: ICNSS RPMh regulator-device grouping changed; "
            f"expected={expected_regulators!r}, actual={actual_regulators!r}",
        )

    shared_resources = set(ICNSS_BT_SHARED_RAIL_VRM_NODES)
    shared_manifest = power.get("rpmh_shared_resource_regulator_devices")
    if not isinstance(shared_manifest, list) or any(
        not isinstance(entry, dict)
        or not isinstance(entry.get("resource"), str)
        or not isinstance(entry.get("node"), str)
        for entry in shared_manifest
    ):
        fail(errors, "shared Bluetooth/ICNSS regulator-node manifest is malformed")
        expected_shared_regulators = ()
    else:
        expected_shared_regulators = tuple(
            sorted((entry["resource"], entry["node"]) for entry in shared_manifest)
        )
    source_shared_regulators = tuple(
        sorted(
            (resource, provider_path)
            for resource, (provider_path, _regulator_path)
            in ICNSS_BT_SHARED_RAIL_VRM_NODES.items()
        )
    )
    actual_shared_regulators = rpmh_resource_regulators_from_dts(
        dts_text, shared_resources
    )
    if expected_shared_regulators != source_shared_regulators:
        fail(errors, "shared Bluetooth/ICNSS regulator nodes differ from the source profile")
    if actual_shared_regulators != expected_shared_regulators:
        fail(errors, f"{dtb.path}: shared RPMh regulator grouping changed")

    actual = rpmh_supply_consumers_from_dts(dts_text, resources)
    expected = tuple(
        sorted(
            (
                entry["resource"],
                entry["node"],
                entry["property"],
            )
            for entry in power.get("rpmh_supply_consumers", [])
            if isinstance(entry, dict)
            and isinstance(entry.get("resource"), str)
            and isinstance(entry.get("node"), str)
            and isinstance(entry.get("property"), str)
        )
    )
    if actual != expected:
        missing = sorted(set(expected) - set(actual))
        unexpected = sorted(set(actual) - set(expected))
        fail(
            errors,
            f"{dtb.path}: RPMh direct supply-client inventory changed; "
            f"missing={missing!r}, unexpected={unexpected!r}",
        )


def check_icnss_bt_shared_rail_requests(
    dtb: Dtb, power: dict, errors: list[str]
) -> None:
    """Check Bluetooth's configured contributions to rails shared with ICNSS."""
    bt_node = "/soc/bt_wcn3990"
    manifest_entries = power.get("rpmh_dynamic_client_profiles")
    expected_entries = [
        {
            "resource": resource,
            "client": "WCN3990 Bluetooth",
            "node": bt_node,
            "supply_property": supply_property,
            "voltage_property": voltage_property,
            "current_property": current_property,
            "min_microvolts": min_uv,
            "max_microvolts": max_uv,
            "load_microamps": load_ua,
        }
        for (
            resource,
            supply_property,
            voltage_property,
            current_property,
            min_uv,
            max_uv,
            load_ua,
        ) in ICNSS_BT_SHARED_RAIL_REQUESTS
    ]
    if manifest_entries != expected_entries:
        fail(errors, "Bluetooth shared-rail request profile differs from the captured DT values")
        return

    for (
        resource,
        supply_property,
        voltage_property,
        current_property,
        min_uv,
        max_uv,
        load_ua,
    ) in ICNSS_BT_SHARED_RAIL_REQUESTS:
        try:
            voltage_cells = dtb.cells(bt_node, voltage_property)
            current_cells = dtb.cells(bt_node, current_property)
            supply_cells = dtb.cells(bt_node, supply_property)
            icnss_supply_cells = dtb.cells(
                "/soc/qcom,icnss@18800000",
                ICNSS_BT_SHARED_RAIL_ICNSS_SUPPLIES[resource],
            )
        except subprocess.CalledProcessError as exc:
            fail(errors, f"{bt_node}: missing Bluetooth request property for {resource}: {exc}")
            continue
        if voltage_cells != [min_uv, max_uv]:
            fail(errors, f"{bt_node}: {voltage_property} differs from the source profile")
        if current_cells != [load_ua]:
            fail(errors, f"{bt_node}: {current_property} differs from the source profile")
        if not supply_cells:
            fail(errors, f"{bt_node}: {supply_property} has no regulator phandle")
        if supply_cells != icnss_supply_cells:
            fail(errors, f"{resource}: Bluetooth and ICNSS no longer share one regulator phandle")
        provider_path, regulator_path = ICNSS_BT_SHARED_RAIL_VRM_NODES[resource]
        try:
            regulator_type = dtb.strings(provider_path, "qcom,regulator-type")
            supported_modes = dtb.cells(provider_path, "qcom,supported-modes")
            mode_thresholds = dtb.cells(provider_path, "qcom,mode-threshold-currents")
            active_sleep_set = dtb.cells(regulator_path, "qcom,set")
        except subprocess.CalledProcessError as exc:
            fail(errors, f"{resource}: missing VRM mode or set profile in {dtb.path}: {exc}")
            continue
        if regulator_type != ["pmic5-ldo"]:
            fail(errors, f"{resource}: expected a PMIC5 LDO regulator")
        if supported_modes != [2, 4]:
            fail(errors, f"{resource}: supported RPMh modes differ from the source profile")
        if mode_thresholds != [0, 10_000]:
            fail(errors, f"{resource}: mode-load thresholds differ from the source profile")
        if active_sleep_set != [3]:
            fail(errors, f"{resource}: Bluetooth regulator must contribute to active and sleep sets")


def check_icnss_rpmh_regulator_profiles(dtb: Dtb, errors: list[str]) -> None:
    """Verify all five ICNSS regulator devices include both RPMh sets."""
    for resource, (provider_path, regulator_path, thresholds) in (
        ICNSS_RPMH_VRM_PROFILES.items()
    ):
        try:
            resource_name = dtb.strings(provider_path, "qcom,resource-name")
            regulator_type = dtb.strings(provider_path, "qcom,regulator-type")
            supported_modes = dtb.cells(provider_path, "qcom,supported-modes")
            mode_thresholds = dtb.cells(
                provider_path, "qcom,mode-threshold-currents"
            )
            active_sleep_set = dtb.cells(regulator_path, "qcom,set")
        except subprocess.CalledProcessError as exc:
            fail(
                errors,
                f"{resource}: missing RPMh regulator profile in {dtb.path}: {exc}",
            )
            continue
        if resource_name != [resource]:
            fail(errors, f"{resource}: RPMh resource-name differs from its rail")
        if regulator_type != ["pmic5-ldo"]:
            fail(errors, f"{resource}: expected a PMIC5 LDO regulator")
        if supported_modes != [2, 4]:
            fail(errors, f"{resource}: supported RPMh modes differ from the source profile")
        if mode_thresholds != thresholds:
            fail(errors, f"{resource}: mode-load thresholds differ from the source profile")
        if active_sleep_set != [3]:
            fail(errors, f"{resource}: regulator must contribute to active and sleep sets")
        if dtb.has(regulator_path, "regulator-system-load"):
            fail(errors, f"{resource}: unexpected regulator-system-load property")


def check_icnss_bt_shared_rail_source(power: dict) -> list[str]:
    expected_entries = [
        {
            "resource": resource,
            "client": "WCN3990 Bluetooth",
            "node": "/soc/bt_wcn3990",
            "supply_property": supply_property,
            "voltage_property": voltage_property,
            "current_property": current_property,
            "min_microvolts": min_uv,
            "max_microvolts": max_uv,
            "load_microamps": load_ua,
        }
        for (
            resource,
            supply_property,
            voltage_property,
            current_property,
            min_uv,
            max_uv,
            load_ua,
        ) in ICNSS_BT_SHARED_RAIL_REQUESTS
    ]
    errors: list[str] = []
    if power.get("rpmh_dynamic_client_profiles") != expected_entries:
        fail(errors, "Bluetooth shared-rail request profile differs from the captured DT values")
    if power.get("rpmh_dynamic_client_source") != ICNSS_BT_SHARED_RAIL_SOURCE:
        fail(errors, "Bluetooth dynamic-client source, sequencing, or runtime-evidence scope changed")
    errors.extend(check_icnss_rpmh_mode_threshold_source())
    return errors


def check_icnss_rpmh_mode_threshold_source() -> list[str]:
    """Pin board thresholds and the regulator-core load-recalculation behavior."""
    errors: list[str] = []
    kernel_root = ROOT / ".cache/sunfish-lineage-kernel"
    source_path = kernel_root / ICNSS_RPMH_REGULATOR_DTS_SOURCE_PATH
    try:
        source = source_path.read_bytes()
        source_text = source.decode()
    except (OSError, UnicodeDecodeError) as exc:
        return [f"cannot read Sunfish RPMh regulator DTS source: {exc}"]

    source_digest = hashlib.sha256(source).hexdigest()
    if source_digest != ICNSS_RPMH_REGULATOR_DTS_SOURCE_SHA256:
        fail(errors, "Sunfish RPMh regulator DTS source hash differs from the pinned board source")

    for resource, thresholds in ICNSS_RPMH_MODE_THRESHOLDS[
        "resource_thresholds_microamps"
    ].items():
        block = _dts_block(source_text, f"rpmh-regulator-{resource}")
        expected_cells = " ".join(str(value) for value in thresholds)
        if block is None or not re.search(
            rf"qcom,mode-threshold-currents\s*=\s*<{re.escape(expected_cells)}>\s*;",
            block,
        ):
            fail(
                errors,
                f"Sunfish board DTS mode thresholds differ for RPMh resource {resource}",
            )

    icnss_path = kernel_root / ICNSS_DRIVER_SOURCE_PATH
    try:
        icnss_contents = icnss_path.read_bytes()
        icnss_text = icnss_contents.decode()
    except (OSError, UnicodeDecodeError) as exc:
        fail(errors, f"cannot read pinned ICNSS regulator source: {exc}")
        icnss_text = ""
    if icnss_text:
        if hashlib.sha256(icnss_contents).hexdigest() != ICNSS_DRIVER_SOURCE_SHA256:
            fail(errors, "pinned ICNSS regulator source hash changed")
        vreg_info = _braced_block(
            icnss_text, "static struct icnss_vreg_info icnss_vreg_info[]"
        )
        observed_loads = (
            re.findall(
                r'\{\s*NULL,\s*"([^"]+)"\s*,\s*\d+\s*,\s*\d+\s*,\s*(\d+)\s*,\s*\d+\s*,\s*(?:true|false)\s*\}',
                vreg_info,
            )
            if vreg_info is not None
            else []
        )
        expected_loads = [
            ("vdd-cx-mx", "0"),
            ("vdd-1.8-xo", "0"),
            ("vdd-1.3-rfa", "0"),
            ("vdd-3.3-ch1", "0"),
            ("vdd-3.3-ch0", "0"),
        ]
        if observed_loads != expected_loads:
            fail(errors, "ICNSS regulator load requests differ from the five zero-load source entries")
        vreg_on = _braced_block(icnss_text, "static int icnss_vreg_on(")
        if (
            vreg_on is None
            or "if (vreg_info->load_ua)" not in vreg_on
            or "regulator_set_load" not in vreg_on
        ):
            fail(errors, "ICNSS enable path no longer applies only nonzero load requests")

    of_regulator_path = kernel_root / ICNSS_OF_REGULATOR_SOURCE_PATH
    try:
        of_regulator_contents = of_regulator_path.read_bytes()
        of_regulator_text = of_regulator_contents.decode()
    except (OSError, UnicodeDecodeError) as exc:
        fail(errors, f"cannot read pinned regulator DT parser source: {exc}")
        of_regulator_text = ""
    if of_regulator_text:
        if hashlib.sha256(of_regulator_contents).hexdigest() != ICNSS_OF_REGULATOR_SOURCE_SHA256:
            fail(errors, "pinned regulator DT parser source hash changed")
        constraints = _braced_block(
            of_regulator_text, "static void of_get_regulation_constraints("
        )
        init_data = _braced_block(
            of_regulator_text,
            "struct regulator_init_data *of_get_regulator_init_data(",
        )
        if (
            constraints is None
            or not re.search(
                r'if\s*\(!of_property_read_u32\(np,\s*"regulator-system-load",\s*&pval\)\)\s*constraints->system_load\s*=\s*pval\s*;',
                constraints,
            )
            or init_data is None
            or "devm_kzalloc(dev, sizeof(*init_data), GFP_KERNEL)" not in init_data
        ):
            fail(errors, "regulator-system-load parsing/default behavior differs from the pinned source")

    for relative_path, expected_digest, label in (
        (
            ICNSS_REGULATOR_CORE_SOURCE_PATH,
            ICNSS_REGULATOR_CORE_SOURCE_SHA256,
            "regulator core",
        ),
        (
            ICNSS_RPMH_REGULATOR_SOURCE_PATH,
            ICNSS_RPMH_REGULATOR_SOURCE_SHA256,
            "RPMh regulator provider",
        ),
    ):
        path = kernel_root / relative_path
        try:
            contents = path.read_bytes()
            text = contents.decode()
        except (OSError, UnicodeDecodeError) as exc:
            fail(errors, f"cannot read pinned {label} source: {exc}")
            continue
        if hashlib.sha256(contents).hexdigest() != expected_digest:
            fail(errors, f"pinned {label} source hash changed")
        if label == "regulator core":
            drms = _dts_block(text, "static int drms_uA_update(")
            enable = _dts_block(text, "static int _regulator_enable(")
            if (
                drms is None
                or not re.search(
                    r"list_for_each_entry\(sibling,\s*&rdev->consumer_list,\s*list\)\s*current_uA\s*\+=\s*sibling->uA_load\s*;",
                    drms,
                )
                or "current_uA += rdev->constraints->system_load;" not in drms
            ):
                fail(errors, "regulator-core DRMS consumer/system load sum changed")
            if (
                enable is None
                or not re.search(
                    r"if\s*\(regulator_ops_is_valid\(rdev,\s*REGULATOR_CHANGE_DRMS\)\)\s*drms_uA_update\(rdev\);",
                    enable,
                )
            ):
                fail(errors, "regulator-core enable path no longer recalculates DRMS")
        else:
            set_load = _dts_block(text, "static int rpmh_regulator_vrm_set_load(")
            if (
                set_load is None
                or "min_load_ua <= load_ua" not in set_load
                or "rpmh_regulator_vrm_set_mode_index(vreg, i)" not in set_load
                or "aggr_vreg->mode[i].min_load_ua = buf[i];" not in text
            ):
                fail(errors, "RPMh VRM mode threshold selection behavior changed")
    return errors


def fail(errors: list[str], message: str) -> None:
    errors.append(message)


def check_icnss_runtime_regulator_delta(power: dict) -> list[str]:
    """Validate the captured Wi-Fi on/off regulator-framework observation."""
    errors: list[str] = []
    capture = power.get("lineage_runtime_regulator_delta_capture")
    if not isinstance(capture, dict):
        return ["ICNSS runtime regulator delta capture is missing"]
    if (
        capture.get("device") != "sunfish"
        or capture.get("kernel_release") != "4.14.357-openela-g28f9290ae067"
        or capture.get("adb_shell_uid") != 0
    ):
        fail(errors, "ICNSS runtime regulator capture identity differs from rooted Sunfish")
    transition = capture.get("wifi_transition")
    if (
        not isinstance(transition, dict)
        or transition.get("off_sample_delay_seconds") != 5
        or transition.get("restored_connected") is not True
        or transition.get("restored_wlan0_carrier") != 1
        or transition.get("restored_wlan0_operstate") != "up"
    ):
        fail(errors, "ICNSS runtime regulator capture does not verify Wi-Fi restoration")
    if capture.get("debugfs_regulator_summary_available") is not False:
        fail(errors, "ICNSS capture must not claim regulator_summary/debugfs access")

    expected = {
        "ldoa9": ("vdd-cx-mx", "pm6150_l9", ("enabled", 640_000, 1), ("disabled", 624_000, 0)),
        "ldoc1": ("vdd-1.8-xo", "pm6150l_l1", ("enabled", 1_800_000, 2), ("enabled", 1_700_000, 1)),
        "ldoc2": ("vdd-1.3-rfa", "pm6150l_l2", ("enabled", 1_304_000, 2), ("enabled", 1_245_000, 1)),
        "ldoc10": ("vdd-3.3-ch0", "pm6150l_l10", ("enabled", 3_312_000, 2), ("enabled", 3_200_000, 1)),
        "ldoc11": ("vdd-3.3-ch1", "pm6150l_l11", ("enabled", 3_312_000, 1), ("disabled", 2_950_000, 0)),
    }
    readings = capture.get("readings")
    if not isinstance(readings, list) or len(readings) != len(expected):
        fail(errors, "ICNSS runtime regulator capture must include all five rails")
        readings = []
    seen: set[str] = set()
    captured_by_resource: dict[str, dict] = {}
    for entry in readings:
        if not isinstance(entry, dict):
            fail(errors, "ICNSS runtime regulator reading is malformed")
            continue
        resource = entry.get("resource")
        if resource not in expected or resource in seen:
            fail(errors, "ICNSS runtime regulator capture has an unknown or duplicate resource")
            continue
        seen.add(resource)
        captured_by_resource[resource] = entry
        supply, regulator_name, on_expected, off_expected = expected[resource]
        if entry.get("supply") != supply or entry.get("regulator_name") != regulator_name:
            fail(errors, f"ICNSS runtime regulator identity differs for {resource}")
        for label, fields_expected in (("wifi_on", on_expected), ("wifi_off", off_expected)):
            sample = entry.get(label)
            if not isinstance(sample, dict) or (
                sample.get("state"), sample.get("microvolts"), sample.get("num_users")
            ) != fields_expected:
                fail(errors, f"ICNSS runtime regulator {label} values differ for {resource}")
        if entry.get("restored") != entry.get("wifi_on"):
            fail(errors, f"ICNSS runtime regulator restore did not return to baseline for {resource}")
    if seen != set(expected):
        fail(errors, "ICNSS runtime regulator capture is missing one or more rails")
    direct_consumer_counts: dict[str, int] = {}
    for resource, _node, _property in ICNSS_RPMH_SUPPLY_CONSUMERS:
        direct_consumer_counts[resource] = direct_consumer_counts.get(resource, 0) + 1
    for resource, entry in captured_by_resource.items():
        wifi_on = entry.get("wifi_on")
        wifi_off = entry.get("wifi_off")
        if not isinstance(wifi_on, dict) or not isinstance(wifi_off, dict):
            continue
        expected_on_users = direct_consumer_counts.get(resource)
        if wifi_on.get("num_users") != expected_on_users:
            fail(errors, f"ICNSS runtime regulator user count does not match direct DT clients for {resource}")
        if wifi_off.get("num_users") != wifi_on.get("num_users", 0) - 1:
            fail(errors, f"ICNSS runtime regulator did not lose one WLAN-correlated user for {resource}")

    limits = capture.get("semantics_and_limits")
    if (
        not isinstance(limits, str)
        or "not a direct electrical measurement" not in limits
        or "does not identify consumers" not in limits
        or "complete per-client vote ledger" not in limits
    ):
        fail(errors, "ICNSS regulator capture must preserve its measurement and ledger limits")
    return errors


def check_icnss_runtime_bluetooth_status(power: dict) -> list[str]:
    """Keep the framework/rfkill disagreement separate from the rail sample."""
    errors: list[str] = []
    capture = power.get("lineage_runtime_bluetooth_status_capture")
    expected = {
        "sampled_at_local": "2026-10-25 01:00:00 -0700",
        "device": "sunfish",
        "kernel_release": "4.14.357-openela-g28f9290ae067",
        "adb_shell_uid": 0,
        "wifi_setting": 1,
        "wlan0_up": True,
        "bluetooth_setting": 1,
        "framework_enabled": True,
        "framework_state": "ON",
        "rfkill_name": "bt_power",
        "rfkill_type": "bluetooth",
        "rfkill_state": 0,
        "rfkill_soft_blocked": True,
        "rfkill_hard_blocked": False,
        "hci0_present": False,
    }
    if not isinstance(capture, dict):
        return ["rooted LineageOS Bluetooth status capture is missing"]
    for field, value in expected.items():
        if capture.get(field) != value:
            fail(errors, f"rooted Bluetooth status capture differs for {field}")
    scope = capture.get("semantics_and_limits")
    if (
        not isinstance(scope, str)
        or "framework reports ON while bt_power rfkill is soft-blocked" not in scope
        or "not simultaneous with the regulator delta" not in scope
        or "cannot attribute retained regulator users to Bluetooth" not in scope
    ):
        fail(errors, "Bluetooth status capture must preserve the framework/rfkill disagreement and timing limit")
    return errors


def check_icnss_power_profile(power: dict) -> list[str]:
    """Keep the modeled sequence aligned with the pinned ICNSS/DT profile."""
    errors: list[str] = []
    expected_supplies = {f"{rail}-supply" for rail, _, _ in ICNSS_POWER_SEQUENCE}
    supplies = power.get("optional_supplies")
    if (
        not isinstance(supplies, list)
        or len(supplies) != len(expected_supplies)
        or any(not isinstance(supply, str) for supply in supplies)
        or set(supplies) != expected_supplies
    ):
        fail(errors, "ICNSS optional supplies differ from the audited five-rail sequence")

    sequence = power.get("lineage_power_on_sequence")
    actual_sequence = []
    if isinstance(sequence, list):
        for entry in sequence:
            if not isinstance(entry, dict):
                actual_sequence.append(None)
                continue
            actual_sequence.append(
                (
                    entry.get("rail"),
                    entry.get("min_microvolts"),
                    entry.get("max_microvolts"),
                )
            )
    expected_sequence = list(ICNSS_POWER_SEQUENCE)
    if actual_sequence != expected_sequence:
        fail(errors, "ICNSS rail order or requested voltage ranges differ from the pinned driver/DT")

    if power.get("vdd_cx_mx_config_cells") != ["0x9c400", "0x9c400"]:
        fail(errors, "ICNSS CX/MX voltage override must remain exactly 640000 uV")
    if power.get("lineage_source_commit") != ICNSS_LINEAGE_SOURCE_COMMIT:
        fail(errors, "ICNSS source reference must remain pinned to the audited Lineage commit")
    if (
        power.get("lineage_driver_source") != "drivers/soc/qcom/icnss.c"
        or power.get("lineage_device_tree_source") != "arch/arm64/boot/dts/qcom/atoll.dtsi"
    ):
        fail(errors, "ICNSS driver and DTS source paths differ from the audited Lineage sources")
    if power.get("running_driver_source_match") != "unverified":
        fail(errors, "running ICNSS driver source identity must remain explicitly unverified")
    for rail, key in (
        ("vdd-3.3-ch0", "source_dts_vdd_3_3_ch0_config_cells"),
        ("vdd-3.3-ch1", "source_dts_vdd_3_3_ch1_config_cells"),
    ):
        if power.get(key) != ICNSS_SOURCE_DTS_3V3_CONFIG:
            fail(errors, f"{rail} pinned source DTS range must remain 3000000-3312000 uV")
        present_key = key.replace("source_dts_", "captured_dtb_").replace("_config_cells", "_config_present")
        if power.get(present_key) is not False:
            fail(errors, f"{rail} override presence must reflect the captured DTB")

    if power.get("rpmh_resources") != ICNSS_RPMH_RESOURCES:
        fail(errors, "ICNSS rail-to-RPMh resource mapping differs from the audited DT")
    consumers = power.get("rpmh_supply_consumers")
    consumer_entries: list[tuple[str, str, str]] = []
    if isinstance(consumers, list):
        for entry in consumers:
            if not isinstance(entry, dict) or any(
                not isinstance(entry.get(field), str)
                for field in ("resource", "node", "property")
            ):
                fail(errors, "ICNSS RPMh supply-client entry is malformed")
                continue
            consumer_entries.append(
                (entry["resource"], entry["node"], entry["property"])
            )
    actual_consumers = tuple(sorted(consumer_entries))
    if actual_consumers != tuple(sorted(ICNSS_RPMH_SUPPLY_CONSUMERS)):
        fail(errors, "ICNSS RPMh direct supply-client inventory differs from the captured DTBs")
    inventory_scope = power.get("rpmh_supply_inventory_scope")
    if (
        not isinstance(inventory_scope, str)
        or "direct -supply references" not in inventory_scope
        or "live regulator-class capture below" not in inventory_scope
        or "complete Apps RSC cache" not in inventory_scope
    ):
        fail(errors, "ICNSS RPMh supply inventory scope overstates runtime or RSC evidence")
    errors.extend(check_icnss_runtime_regulator_delta(power))
    errors.extend(check_icnss_runtime_bluetooth_status(power))
    if power.get("rpmh_vote_semantics") != ICNSS_RPMH_VOTE_SEMANTICS:
        fail(errors, "RPMh shared-vote behavior or native ownership gate differs from the audited policy")
    errors.extend(check_icnss_bt_shared_rail_source(power))
    if power.get("clock_names_present") is not False:
        fail(errors, "ICNSS must not claim a reference-clock name absent from the captured DT")
    semantics = power.get("voltage_request_semantics")
    if (
        not isinstance(semantics, str)
        or "captured DTBs omit" not in semantics
        or "not measured output voltages" not in semantics
        or "running driver identity is unverified" not in semantics
    ):
        fail(errors, "ICNSS defaults and absent DT overrides must not be represented as measured output voltages")
    return errors


def check_modem_firmware_inventory(manifest: dict) -> list[str]:
    """Check inventory integrity and keep release provenance explicit."""
    errors: list[str] = []
    try:
        observation = manifest["modem_firmware_observation"]
        segments = observation["segment_hash_inventory"]
        expected_digest = observation["segment_inventory_sha256"]
        vendor_fingerprint = observation["vendor_build_fingerprint"]
    except (KeyError, TypeError):
        return ["modem firmware hash inventory is incomplete"]

    if not isinstance(segments, list):
        return ["modem segment hash inventory is not a list"]
    expected_names = [f"modem.b{index:02d}" for index in range(31)]
    actual_names = [item.get("name") for item in segments if isinstance(item, dict)]
    if actual_names != expected_names or len(segments) != len(expected_names):
        fail(errors, "modem segment inventory must contain modem.b00 through modem.b30 in order")

    digest_lines: list[str] = []
    empty_sha256 = hashlib.sha256(b"").hexdigest()
    for item in segments:
        if not isinstance(item, dict):
            fail(errors, "modem segment inventory entry is not an object")
            continue
        name = item.get("name")
        size = item.get("size_bytes")
        digest = item.get("sha256")
        if not isinstance(size, int) or size < 0:
            fail(errors, f"{name}: invalid modem segment size")
            continue
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail(errors, f"{name}: invalid SHA-256")
            continue
        if size == 0 and digest != empty_sha256:
            fail(errors, f"{name}: zero-length file must use the SHA-256 of empty content")
        digest_lines.append(f"{name} {size} {digest}\n")

    calculated = hashlib.sha256("".join(digest_lines).encode("utf-8")).hexdigest()
    if calculated != expected_digest:
        fail(errors, "modem segment inventory digest does not match its entries")
    if not isinstance(vendor_fingerprint, str) or not vendor_fingerprint:
        fail(errors, "modem vendor build fingerprint is missing")
    caveat = observation.get("provenance_caveat")
    if not isinstance(caveat, str) or not caveat.strip():
        fail(errors, "modem firmware provenance caveat is missing")

    version_info = observation.get("modem_version_info")
    if not isinstance(version_info, dict):
        fail(errors, "observed modem version metadata is missing")
    else:
        for field in ("path", "modem_image_build_id", "metabuild_id", "timestamp", "provenance_caveat"):
            value = version_info.get(field)
            if not isinstance(value, str) or not value.strip():
                fail(errors, f"observed modem version metadata is missing {field}")

    release = observation.get("firmware_release_match")
    if not isinstance(release, dict):
        fail(errors, "modem firmware release-match status is missing")
    else:
        status = release.get("status")
        if not isinstance(status, str) or status not in {"unverified", "matched", "mismatch"}:
            fail(errors, "modem firmware release-match status must be unverified, matched, or mismatch")
        reason = release.get("reason")
        if not isinstance(reason, str) or not reason.strip():
            fail(errors, "modem firmware release-match reason is missing")
        if isinstance(status, str) and status in {"matched", "mismatch"}:
            evidence = release.get("evidence")
            if not isinstance(evidence, dict):
                fail(errors, "a compared modem release requires immutable-source, artifact-digest, and comparison-basis evidence")
            else:
                for field in ("immutable_source", "comparison_basis"):
                    value = evidence.get(field)
                    if not isinstance(value, str) or not value.strip():
                        fail(errors, f"compared modem release evidence is missing {field}")
                digest = evidence.get("artifact_sha256")
                if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
                    fail(errors, "compared modem release evidence requires a valid artifact SHA-256")
    return errors


def check_wlan_firmware_inventory(manifest: dict) -> list[str]:
    """Check the observed WLAN file identities, not release authenticity."""
    try:
        observation = manifest["wlan_firmware_observation"]
        files = observation["files"]
        caveat = observation["provenance_caveat"]
        runtime = observation["runtime_reported"]
    except (KeyError, TypeError):
        return ["WLAN firmware observation is incomplete"]
    if not isinstance(caveat, str) or not caveat.strip():
        return ["WLAN firmware provenance caveat is missing"]
    if not isinstance(runtime, dict) or any(
        not isinstance(runtime.get(field), str) or not runtime[field].strip()
        for field in (
            "source", "firmware_version", "hardware_version", "driver_version",
            "matched_source_version_constant", "matched_source_path", "matched_source_commit", "caveat",
        )
    ):
        return ["WLAN runtime firmware/driver version report is incomplete"]
    if runtime["driver_version"] != runtime["matched_source_version_constant"]:
        return ["reported WLAN driver version does not match its recorded source version constant"]
    lineage_identity = observation.get("lineage_runtime_identity")
    expected_lineage_identity = {
        "kernel": "4.14.357-openela-g28f9290ae067",
        "firmware_build": "3226822e",
        "host_software_version": "5.2.03.9H",
        "firmware_version": "3.2.2.9.558.0",
        "hardware_version": "0x40090000",
        "qdf_board_id": 0,
        "qdf_board_version": [18, 0, 0, 0, 0],
        "qcacld_board_version": 18,
        "reference_design_id": 0,
        "customer_id": 0,
        "project_id": 0,
        "board_data_revision": 0,
    }
    if not isinstance(lineage_identity, dict):
        return ["current LineageOS WLAN runtime identity is missing"]
    if not isinstance(lineage_identity.get("source"), str) or not lineage_identity["source"].strip():
        return ["current LineageOS WLAN identity source is missing"]
    for field, expected in expected_lineage_identity.items():
        if lineage_identity.get(field) != expected:
            return [f"current LineageOS WLAN identity {field} differs from the captured QDF/QCACLD values"]
    if lineage_identity.get("evidence_path") != ".cache/pixel-sunfish/lineage-wlan-runtime-identity-20261023.txt":
        return ["current LineageOS WLAN identity evidence path changed"]
    if not re.fullmatch(r"[0-9a-f]{64}", str(lineage_identity.get("evidence_sha256", ""))):
        return ["current LineageOS WLAN identity evidence SHA-256 is invalid"]
    scope = lineage_identity.get("scope")
    if not isinstance(scope, str) or not all(
        phrase in scope for phrase in ("not ath10k SNOC QMI", "cannot select", "No firmware payload")
    ):
        return ["current LineageOS QDF identity must remain distinct from ath10k selector data"]
    if not isinstance(files, list):
        return ["WLAN firmware file inventory is not a list"]

    package = observation.get("official_package_reference")
    if not isinstance(package, dict):
        return ["official Pixel firmware package reference is missing"]
    package_sha = package.get("published_archive_sha256")
    observed_sha = package.get("observed_archive_sha256")
    if not isinstance(package_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", package_sha):
        return ["official package reference has an invalid published SHA-256"]
    if observed_sha != package_sha:
        return ["streamed official package SHA-256 does not match Google's published checksum"]
    if package.get("use_permission") != "unverified":
        return ["proprietary WLAN firmware use permission must remain unverified"]
    license_review = package.get("license_review")
    if not isinstance(license_review, dict):
        return ["official WLAN package license review evidence is missing"]
    if license_review.get("archive_sha256") != package_sha:
        return ["official WLAN package license review is not bound to the verified archive"]
    if license_review.get("reviewed_member") != "extract-google_devices-sunfish.sh":
        return ["official WLAN package license review does not identify its reviewed member"]
    if license_review.get("sections_reviewed") != ["1(c)", "2(a)", "3(b)"]:
        return ["official WLAN package license review is missing the scope and restriction clauses"]
    if license_review.get("handset_wlan_payloads_compared") is not False:
        return ["official WLAN package review must not claim a handset-payload comparison"]
    if license_review.get("nexatrodeos_permission") != "not_established":
        return ["official WLAN package must not be recorded as authorized for NexatrodeOS"]
    if not isinstance(license_review.get("summary"), str) or not all(
        phrase in license_review["summary"]
        for phrase in ("Section 2(a)", "Section 3(b)", "prohibits incorporating", "No conclusion")
    ):
        return ["official WLAN package license summary omits material scope limitations"]
    if not all(
        isinstance(package.get(field), str) and package[field].strip()
        for field in ("download_url", "source_page", "license_observation", "policy", "comparison")
    ):
        return ["official package reference is missing source, license, or comparison details"]

    candidate = observation.get("upstream_ath10k_candidate")
    if not isinstance(candidate, dict) or candidate.get("status") != "reference_candidate_not_device_verified":
        return ["upstream ath10k alternative must remain an unverified reference candidate"]
    linux_fw = candidate.get("linux_firmware_candidate")
    if not isinstance(linux_fw, dict):
        return ["upstream ath10k firmware candidate metadata is missing"]
    if not re.fullmatch(r"[0-9a-f]{40}", str(linux_fw.get("commit", ""))):
        return ["upstream ath10k firmware candidate must be pinned to an immutable commit"]
    candidate_hash = linux_fw.get("wlanmdsp_sha256")
    device_hash = linux_fw.get("observed_wlanmdsp_sha256")
    if not all(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) for value in (candidate_hash, device_hash)):
        return ["upstream ath10k WLAN modem firmware hashes are invalid"]
    if candidate_hash == device_hash or linux_fw.get("observed_wlanmdsp_match") is not False:
        return ["upstream ath10k firmware mismatch must not be recorded as a device match"]
    if not all(
        isinstance(linux_fw.get(field), str) and linux_fw[field].strip()
        for field in (
            "license_file", "license_summary", "license_review", "binary_review",
            "test_readiness_note",
        )
    ):
        return ["upstream ath10k firmware license and review caveats are incomplete"]
    if linux_fw.get("sunfish_reference_test_decision") != (
        "no_go_generic_bundle_platform_scope_hash_and_loader_do_not_match"
    ):
        return ["generic WCN3990 bundle must remain excluded from Sunfish hardware testing"]
    if linux_fw.get("wlanmdsp_platform_scope_source") != (
        "https://kernel.googlesource.com/pub/scm/linux/kernel/git/srini/linux-firmware/+/"
        "a0142c57045701b7557c3060af5c4246c420e4d8"
    ):
        return ["generic WCN3990 platform-scope source commit is missing or changed"]
    if not all(
        phrase in linux_fw["test_readiness_note"]
        for phrase in ("NO-GO", "hash differs", "SDM845-specific", "out-of-band")
    ):
        return ["generic WCN3990 Sunfish no-go rationale is incomplete"]
    candidate_files = linux_fw.get("candidate_file_integrity")
    if not isinstance(candidate_files, list):
        return ["upstream ath10k candidate file integrity inventory is missing"]
    expected_candidate_files = {
        "ath10k/WCN3990/hw1.0/board-2.bin": (
            893528,
            "c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d",
        ),
        "ath10k/WCN3990/hw1.0/firmware-5.bin": (
            60,
            "fef6539e0127579536bc977be57a90d018b83f2931fedc3a8870fbe38d6c4127",
        ),
        "ath10k/WCN3990/hw1.0/wlanmdsp.mbn": (
            3725044,
            "92e1501254e6de78c0f2e2cf091507d488b608d07e53acd14813a82744823ec2",
        ),
        "ath10k/WCN3990/hw1.0/notice.txt_wlanmdsp": (
            27005,
            "d7a42cb726d992c427fc130b60b076c5536297aafceecdb053b1b5de17f7d5fa",
        ),
    }
    candidate_entries = {
        item.get("path"): item for item in candidate_files if isinstance(item, dict)
    }
    if set(candidate_entries) != set(expected_candidate_files) or len(candidate_files) != len(expected_candidate_files):
        return ["upstream ath10k candidate file inventory is incomplete or duplicated"]
    for path, (expected_size, expected_digest) in expected_candidate_files.items():
        item = candidate_entries[path]
        if item.get("size_bytes") != expected_size or item.get("sha256") != expected_digest:
            return [f"upstream ath10k candidate integrity changed for {path}"]
    firmware5 = candidate_entries["ath10k/WCN3990/hw1.0/firmware-5.bin"]
    if firmware5.get("git_mode") != "100644" or firmware5.get("git_blob_id") != "69b4cae2d5e9fc340fbc43f3f120dd0698d907af":
        return ["upstream ath10k firmware-5.bin source object metadata changed"]
    if linux_fw.get("firmware5_artifact_kind") != "ath10k_fw_feature_flags" or linux_fw.get("firmware5_is_firmware_payload") is not False:
        return ["upstream ath10k firmware-5.bin must be recorded as feature metadata, not a firmware payload"]
    if set(linux_fw.get("firmware5_features", [])) != {"wowlan", "mgmt-tx-by-ref", "non-bmi"}:
        return ["upstream ath10k firmware-5.bin feature metadata changed"]
    for field in ("firmware5_non_bmi_effect", "wlanmdsp_platform_scope"):
        if not isinstance(linux_fw.get(field), str) or not linux_fw[field].strip():
            return [f"upstream ath10k candidate is missing {field}"]

    current = linux_fw.get("current_linux_firmware_snapshot")
    if not isinstance(current, dict) or current.get("commit") != "95c414d44c8e60880be787822d5020a010044a49":
        return ["current Linux-firmware review must be pinned to its audited immutable snapshot"]
    if current.get("sunfish_compatibility") != "unverified":
        return ["current upstream firmware must remain unverified for Sunfish"]
    expected_current = {
        "generic_wcn3990": {
            "version": "WLAN.HL.2.0-01387-QCAHLSWMTPLZ-1",
            "license_file": "LICENSES/LICENSE.QualcommAtheros_ath10k",
            "license_sha256": "337a55102138d7baa143ee4a4c6c91693e0113fece35d380b2a12109e8c23b3f",
            "files": {
                "ath10k/WCN3990/hw1.0/board-2.bin": (670116, "c03d801cba1233914d777644e368ea942f36064e805ba6102514dedb47e53c76"),
                "ath10k/WCN3990/hw1.0/firmware-5.bin": (60, "fef6539e0127579536bc977be57a90d018b83f2931fedc3a8870fbe38d6c4127"),
                "ath10k/WCN3990/hw1.0/wlanmdsp.mbn": (3725044, "92e1501254e6de78c0f2e2cf091507d488b608d07e53acd14813a82744823ec2"),
                "ath10k/WCN3990/hw1.0/notice.txt_wlanmdsp": (27005, "d7a42cb726d992c427fc130b60b076c5536297aafceecdb053b1b5de17f7d5fa"),
            },
        },
        "qcm2290": {
            "version": "WLAN.HL.3.3.7.c2-00931-QCAHLSWMTPLZ-1",
            "license_file": "LICENSE.qcom",
            "license_sha256": "be904cd28cb292b80cdb6cf412ab0d9159d431671e987ad433c1f62e0988a9bc",
            "notice_file": "qcom/NOTICE.txt",
            "notice_sha256": "fa43e1b9a13b341a07adca9dbe73d0f9072d7966fdfe811c01f0dd2872d7309a",
            "files": {
                "ath10k/WCN3990/hw1.0/qcm2290/firmware-5.bin": (60, "80daf4f3a89e6ea8222e45387bb362c854fcd30c02e9da59ca723b39b269cdd2"),
                "ath10k/WCN3990/hw1.0/qcm2290/wlanmdsp.mbn": (3828228, "4a0f4d8a5d7cdc094a5b0b907f4b9906657b76070993fed4d8cc4ccf5776b515"),
            },
        },
    }
    for variant_name, expected in expected_current.items():
        variant = current.get(variant_name)
        if not isinstance(variant, dict):
            return [f"current Linux-firmware {variant_name} candidate metadata is missing"]
        for field, value in expected.items():
            if field == "files":
                continue
            if variant.get(field) != value:
                return [f"current Linux-firmware {variant_name} {field} changed"]
        listed = variant.get("files")
        if not isinstance(listed, list):
            return [f"current Linux-firmware {variant_name} integrity inventory is missing"]
        entries = {item.get("path"): item for item in listed if isinstance(item, dict)}
        if len(entries) != len(listed) or set(entries) != set(expected["files"]):
            return [f"current Linux-firmware {variant_name} integrity inventory is incomplete or duplicated"]
        for path, (size, digest) in expected["files"].items():
            if entries[path].get("size_bytes") != size or entries[path].get("sha256") != digest:
                return [f"current Linux-firmware integrity changed for {path}"]
    if current.get("qrb4210_symlinks") != {
        "firmware-5.bin": "ath10k/WCN3990/hw1.0/qcm2290/firmware-5.bin",
        "wlanmdsp.mbn": "ath10k/WCN3990/hw1.0/qcm2290/wlanmdsp.mbn",
    }:
        return ["QRB4210 Linux-firmware paths must remain identified as QCM2290 symlinks"]
    if current.get("pinned_driver_path_selection") != {
        "source_commit": "fd78d17954a86ea6ceb0c535e618ea129e09f8dd",
        "snoc_source": "drivers/net/wireless/ath/ath10k/snoc.c",
        "core_source": "drivers/net/wireless/ath/ath10k/core.c",
        "base_directory": "ath10k/WCN3990/hw1.0",
        "selector_property": "firmware-name",
        "current_sunfish_property_present": False,
        "lookup_order": ["<base>/<firmware-name>/<file>", "<base>/<file>"],
        "qcm2290_test_overlay_value": "qcm2290",
        "selector_backport_required": False,
        "parent_board_file_required": "ath10k/WCN3990/hw1.0/board-2.bin",
        "out_of_band_wlanmdsp_launch_provided": False,
    }:
        return ["pinned ath10k firmware-name lookup and QCM2290 test-path facts changed"]
    gate = current.get("reference_test_gate")
    if not isinstance(gate, str) or not all(
        phrase in gate for phrase in ("Do not stage", "board data", "WLFW", "SMMU", "license")
    ):
        return ["current Linux-firmware reference-test safety gates are incomplete"]

    latest = linux_fw.get("latest_upstream_snapshot")
    if not isinstance(latest, dict) or latest.get("commit") != "a6bd225f33b3d6bdf31a9b654a3ce51ea04d69e2":
        return ["latest Linux-firmware follow-up must be pinned to the audited main snapshot"]
    if latest.get("audit_date") != "2026-10-27" or "git ls-remote" not in latest.get("head_verification", ""):
        return ["latest Linux-firmware follow-up must record current-head verification"]
    if latest.get("sunfish_compatibility") != "unverified":
        return ["latest upstream firmware candidates must remain unverified for Sunfish"]
    if candidate.get("current_main_board_selector_audit") != {
        "repository": "https://gitlab.com/kernel-firmware/linux-firmware",
        "source_commit": "a6bd225f33b3d6bdf31a9b654a3ce51ea04d69e2",
        "audit_date": "2026-10-27",
        "path": "ath10k/WCN3990/hw1.0/board-2.bin",
        "size_bytes": 893528,
        "sha256": "c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d",
        "selector_scan": "Raw board-2.bin byte scan for the exact ASCII strings google_sunfish and sunfish; neither string is present. The printable NUL-delimited bus=snoc selector records contain other Google variants but no Sunfish selector.",
        "google_sunfish_selector_present": False,
        "sunfish_selector_present": False,
        "sunfish_hardware_test_decision": "no_go_no_sunfish_board_selector",
    }:
        return ["current WCN3990 base board-data selector audit changed"]
    if latest.get("relevant_candidate_commit") != "f64d7db7d2646e1c7744963d540524f523ae0443":
        return ["latest Shikra firmware addition commit changed"]
    if latest.get("latest_shikra_update_commit") != "72f3b81c89fbc52677b9dbbfa92cde0ae00a36e2":
        return ["latest Shikra firmware stability-update commit changed"]
    latest_qcm = latest.get("qcm2290")
    expected_latest_qcm = {
        "directory": "ath10k/WCN3990/hw1.0/qcm2290",
        "version": "WLAN.HL.3.3.7.c2-00931-QCAHLSWMTPLZ-1",
        "whence_license_reference": "LICENSE.qcom and NOTICE.qcom",
        "license_file": "LICENSES/LICENSE.qcom",
        "license_sha256": "be904cd28cb292b80cdb6cf412ab0d9159d431671e987ad433c1f62e0988a9bc",
        "notice_file": "LICENSES/NOTICE.qcom",
        "notice_sha256": "fa43e1b9a13b341a07adca9dbe73d0f9072d7966fdfe811c01f0dd2872d7309a",
        "license_scope_summary": "The agreement permits install/use and binary redistribution subject to its terms: binaries only, operation only with platforms incorporating Qualcomm Technologies chipsets, include the agreement text with redistribution, preserve notices, and no reverse engineering/disassembly/decompilation. It grants no patent rights and bars applying third-party license terms to the materials.",
        "license_review_status": "source_audited; agreement acceptance, patent implications, and compatibility with NexatrodeOS redistribution require separate review",
        "board_data": "Shares the parent WCN3990 board-2.bin; WHENCE lists no QCM2290-specific board file.",
        "status": "Separate QCM2290-specific candidate; not a Sunfish match.",
    }
    if not isinstance(latest_qcm, dict):
        return ["latest QCM2290 WCN3990 candidate metadata is missing"]
    for field, expected in expected_latest_qcm.items():
        if latest_qcm.get(field) != expected:
            return [f"latest QCM2290 candidate {field} differs from the audited source facts"]
    latest_qcm_files = latest_qcm.get("files")
    expected_latest_qcm_files = {
        "ath10k/WCN3990/hw1.0/qcm2290/firmware-5.bin": (60, "80daf4f3a89e6ea8222e45387bb362c854fcd30c02e9da59ca723b39b269cdd2"),
        "ath10k/WCN3990/hw1.0/qcm2290/wlanmdsp.mbn": (3828228, "4a0f4d8a5d7cdc094a5b0b907f4b9906657b76070993fed4d8cc4ccf5776b515"),
    }
    if not isinstance(latest_qcm_files, list):
        return ["latest QCM2290 candidate file integrity inventory is missing"]
    latest_qcm_entries = {item.get("path"): item for item in latest_qcm_files if isinstance(item, dict)}
    if len(latest_qcm_entries) != len(latest_qcm_files) or set(latest_qcm_entries) != set(expected_latest_qcm_files):
        return ["latest QCM2290 candidate file inventory is incomplete or duplicated"]
    for path, (size, digest) in expected_latest_qcm_files.items():
        if latest_qcm_entries[path].get("size_bytes") != size or latest_qcm_entries[path].get("sha256") != digest:
            return [f"latest QCM2290 candidate integrity changed for {path}"]
    if latest.get("license_path_note") != (
        "At this main snapshot WHENCE refers to LICENSE.qcom and NOTICE.qcom by name; the files are stored at "
        "LICENSES/LICENSE.qcom and LICENSES/NOTICE.qcom. The earlier 95c414d snapshot used LICENSE.qcom and qcom/NOTICE.txt."
    ):
        return ["latest Linux-firmware license paths differ from the audited main snapshot"]
    shikra = latest.get("shikra")
    if not isinstance(shikra, dict):
        return ["latest Shikra WCN3990 candidate metadata is missing"]
    expected_shikra = {
        "directory": "ath10k/WCN3990/hw1.0/shikra",
        "version": "WLAN.HL.3.3.7.c5-00226-QCAHLSWMTPL-1",
        "update_note": "Upstream says this firmware update addresses stability issues; it remains a Shikra-specific candidate.",
        "whence_license": "LICENSES/LICENSE.QualcommAtheros_ath10k",
        "license_sha256": "337a55102138d7baa143ee4a4c6c91693e0113fece35d380b2a12109e8c23b3f",
        "license_scope_summary": "Binary use and redistribution without modification for use solely with a Qualcomm Atheros chipset; retain notice; no reverse engineering.",
        "board_data": "WHENCE lists no shikra-specific board-2.bin; ath10k would use the parent WCN3990 board file, whose Sunfish match is unverified.",
        "out_of_band_loader_source": "Commit 837ae0e5beccc2f1e974afba56c22c3cf981fc4e states Shikra loads this WLAN image through tqftpserv from the DSP firmware directory; no equivalent Sunfish loader path is verified.",
        "status": "Separate platform-specific candidate; not a Sunfish match.",
    }
    for field, expected in expected_shikra.items():
        if shikra.get(field) != expected:
            return [f"latest Shikra candidate {field} differs from the audited source facts"]
    board = shikra.get("parent_board_file")
    if board != {
        "path": "ath10k/WCN3990/hw1.0/board-2.bin",
        "size_bytes": 893528,
        "sha256": "c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d",
    }:
        return ["latest Shikra parent board-data identity changed"]
    shikra_files = shikra.get("files")
    expected_shikra_files = {
        "ath10k/WCN3990/hw1.0/shikra/firmware-5.bin": (60, "7f35f16380124ceff31a4c3064fb6f66b366ccd76b517b271a53e4c9029fe41b"),
        "ath10k/WCN3990/hw1.0/shikra/wlanmdsp.mbn": (3808072, "ea1787533f969c084c24292defdcc88d75d71955bea7e12ffee2b5fb16248350"),
        "ath10k/WCN3990/hw1.0/shikra/notice.txt_wlanmdsp": (27574, "e9a920d4b55f62ac8a66fe7693aa74f461a75f330847f9b51f37a2565adb85d3"),
    }
    if not isinstance(shikra_files, list):
        return ["latest Shikra candidate file inventory is missing"]
    shikra_entries = {item.get("path"): item for item in shikra_files if isinstance(item, dict)}
    if len(shikra_entries) != len(shikra_files) or set(shikra_entries) != set(expected_shikra_files):
        return ["latest Shikra candidate file inventory is incomplete or duplicated"]
    for path, (size, digest) in expected_shikra_files.items():
        if shikra_entries[path].get("size_bytes") != size or shikra_entries[path].get("sha256") != digest:
            return [f"latest Shikra candidate integrity changed for {path}"]
    latest_gate = latest.get("reference_test_gate")
    if not isinstance(latest_gate, str) or not all(
        phrase in latest_gate for phrase in ("Do not stage", "Shikra", "WLFW", "SMMU", "license")
    ):
        return ["latest Linux-firmware reference-test gates are incomplete"]

    expected_paths = {
        "/vendor/firmware/wlanmdsp.mbn",
        "/vendor/firmware/bdwlan-sunfish.bin",
        "/vendor/firmware/bdwlan-sunfish-EVT1.0.bin",
        "/vendor/firmware/wlan/qca_cld/WCNSS_qcom_cfg.ini",
    }
    entries = {item.get("path"): item for item in files if isinstance(item, dict)}
    errors: list[str] = []
    if set(entries) != expected_paths or len(files) != len(expected_paths):
        fail(errors, "WLAN firmware inventory must contain the four observed Sunfish files exactly once")
    for path in sorted(expected_paths):
        item = entries.get(path)
        if item is None:
            continue
        size = item.get("size_bytes")
        digest = item.get("sha256")
        if not isinstance(size, int) or size < 0:
            fail(errors, f"{path}: invalid WLAN file size")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail(errors, f"{path}: invalid WLAN file SHA-256")
        if not isinstance(item.get("role"), str) or not item["role"].strip():
            fail(errors, f"{path}: WLAN file role is missing")
        source_match = item.get("source_match")
        if source_match is not None:
            if not isinstance(source_match, dict):
                fail(errors, f"{path}: WLAN source match is not an object")
                continue
            if source_match.get("status") != "exact_sha256_match":
                fail(errors, f"{path}: unsupported WLAN source-match status")
            commit = source_match.get("commit")
            if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
                fail(errors, f"{path}: source match must use an immutable Git commit")
            for field in (
                "repository", "path", "method", "source_class",
                "identity_scope", "license_status",
            ):
                value = source_match.get(field)
                if not isinstance(value, str) or not value.strip():
                    fail(errors, f"{path}: source match is missing {field}")
            if source_match.get("identity_scope") != "byte_identity_only":
                fail(errors, f"{path}: source match must be scoped to byte identity only")
            if source_match.get("source_class") == "community_firmware_rehost":
                if source_match.get("license_status") != "not_declared_in_repository":
                    fail(errors, f"{path}: firmware source licensing must remain explicitly unverified")
            elif source_match.get("source_class") == "public_device_tree_source":
                if source_match.get("license_status") != "not_applicable_text_configuration":
                    fail(errors, f"{path}: text configuration match has an invalid license classification")
            else:
                fail(errors, f"{path}: unsupported WLAN source classification")
            source_digest = source_match.get("source_sha256")
            if not isinstance(source_digest, str) or source_digest != digest:
                fail(errors, f"{path}: source SHA-256 does not match the observed WLAN file")

    observed_mbn = entries.get("/vendor/firmware/wlanmdsp.mbn")
    observed_board_data = entries.get("/vendor/firmware/bdwlan-sunfish.bin")
    source_path = ROOT / "rust/src/sunfish_vendor_wlan_source.rs"
    try:
        source_text = source_path.read_text()
    except OSError as exc:
        fail(errors, f"cannot read Rust WLAN source identity: {exc}")
    else:
        length_match = re.search(
            r"pub const PHONE_OBSERVED_WLAN_MDSP_LEN: u64 = ([\d_]+);", source_text
        )
        digest_match = re.search(
            r"pub const PHONE_OBSERVED_WLAN_MDSP_SHA256: \[u8; 32\] = \[(.*?)\];",
            source_text,
            re.DOTALL,
        )
        if observed_mbn is None:
            fail(errors, "observed WLAN MBN is missing; cannot validate the Rust identity")
        elif length_match is None or digest_match is None:
            fail(errors, "Rust WLAN source does not define the phone-observed image identity")
        else:
            try:
                code_length = int(length_match.group(1).replace("_", ""))
                code_digest = bytes(
                    int(value, 16)
                    for value in re.findall(r"0x([0-9a-fA-F]{1,2})", digest_match.group(1))
                )
            except ValueError:
                fail(errors, "Rust WLAN source identity contains an invalid numeric value")
            else:
                if code_length != observed_mbn.get("size_bytes"):
                    fail(errors, "Rust WLAN image length differs from the observed manifest")
                try:
                    manifest_digest = bytes.fromhex(observed_mbn.get("sha256", ""))
                except ValueError:
                    manifest_digest = b""
                if len(code_digest) != 32 or code_digest != manifest_digest:
                    fail(errors, "Rust WLAN image SHA-256 differs from the observed manifest")

        board_length_match = re.search(
            r"pub const PHONE_OBSERVED_BOARD_DATA_LEN: u64 = ([\d_]+);", source_text
        )
        board_digest_match = re.search(
            r"pub const PHONE_OBSERVED_BOARD_DATA_SHA256: \[u8; 32\] = \[(.*?)\];",
            source_text,
            re.DOTALL,
        )
        if observed_board_data is None:
            fail(errors, "observed Sunfish WLAN board data is missing; cannot validate the Rust identity")
        elif board_length_match is None or board_digest_match is None:
            fail(errors, "Rust WLAN source does not define the phone-observed board-data identity")
        else:
            try:
                code_length = int(board_length_match.group(1).replace("_", ""))
                code_digest = bytes(
                    int(value, 16)
                    for value in re.findall(r"0x([0-9a-fA-F]{1,2})", board_digest_match.group(1))
                )
            except ValueError:
                fail(errors, "Rust WLAN board-data identity contains an invalid numeric value")
            else:
                if code_length != observed_board_data.get("size_bytes"):
                    fail(errors, "Rust WLAN board-data length differs from the observed manifest")
                try:
                    manifest_digest = bytes.fromhex(observed_board_data.get("sha256", ""))
                except ValueError:
                    manifest_digest = b""
                if len(code_digest) != 32 or code_digest != manifest_digest:
                    fail(errors, "Rust WLAN board-data SHA-256 differs from the observed manifest")
    return errors


def compare_cells(dtb: Dtb, node: str, prop: str, expected: list[str], errors: list[str]) -> None:
    try:
        actual = dtb.cells(node, prop)
    except subprocess.CalledProcessError:
        fail(errors, f"{node}: missing/unreadable {prop}")
        return
    wanted = [int(value, 0) for value in expected]
    if actual != wanted:
        fail(errors, f"{node}:{prop}: got {[hex(x) for x in actual]}, expected {[hex(x) for x in wanted]}")


def compare_strings(dtb: Dtb, node: str, prop: str, expected: list[str], errors: list[str]) -> None:
    try:
        actual = dtb.strings(node, prop)
    except subprocess.CalledProcessError:
        fail(errors, f"{node}: missing/unreadable {prop}")
        return
    if actual != expected:
        fail(errors, f"{node}:{prop}: got {actual!r}, expected {expected!r}")


def path_children(dtb: Dtb, node: str) -> list[str]:
    return dtb.get("-l", node).splitlines()


def check_msa(dtb: Dtb, icnss: dict, errors: list[str]) -> None:
    node = icnss["path"]
    msa = icnss["msa"]
    compare_cells(dtb, node, "qcom,wlan-msa-memory", msa["referenced_size_cells"], errors)
    try:
        phandle = dtb.cells(node, "qcom,wlan-msa-fixed-region")
    except subprocess.CalledProcessError:
        fail(errors, f"{node}: missing qcom,wlan-msa-fixed-region")
        return
    if len(phandle) != 1:
        fail(errors, f"{node}: expected a single MSA region phandle")
        return

    reserved = "/reserved-memory"
    found = None
    for child in path_children(dtb, reserved):
        path = f"{reserved}/{child}"
        try:
            candidate = dtb.cells(path, "phandle")
        except subprocess.CalledProcessError:
            continue
        if candidate == phandle:
            found = path
            break
    if found is None:
        fail(errors, "qcom,wlan-msa-fixed-region does not resolve inside /reserved-memory")
        return
    compare_cells(dtb, found, "reg", msa["fixed_region_reg_cells"], errors)
    if dtb.has(found, "no-map") != msa["fixed_region_no_map"]:
        fail(errors, f"{found}: no-map does not match the WLAN MSA policy")


def check_smem(dtb: Dtb, icnss: dict, errors: list[str]) -> None:
    spec = icnss["wlfw_control_transport"]["shared_smem"]
    path = spec["device_tree_path"]
    base = int(spec["base"], 0)
    size = int(spec["size"], 0)
    compare_cells(dtb, path, "reg", ["0x0", hex(base), "0x0", hex(size)], errors)
    if dtb.has(path, "no-map") != spec["no_map"]:
        fail(errors, f"{path}: no-map does not match the SMEM ownership policy")

    # Confirm the SMEM controller's memory-region phandle resolves to this
    # exact node, rather than trusting a label or the upstream source default.
    controller = "/soc/qcom,smem"
    try:
        region_phandle = dtb.cells(path, "phandle")
        controller_region = dtb.cells(controller, "memory-region")
        if len(region_phandle) != 1 or controller_region != region_phandle:
            fail(errors, f"{controller}: memory-region does not resolve to {path}")
    except subprocess.CalledProcessError:
        fail(errors, f"{controller}: missing SMEM region reference or {path} phandle")


def check_runtime_pil_regions(dtb: Dtb, manifest: dict, errors: list[str]) -> None:
    """Keep the post-DTBO PIL/WLAN carveouts aligned and non-overlapping."""
    regions = manifest["modem_firmware_observation"]["mdt"]["runtime_pil_reservations"]
    intervals: list[tuple[int, int, str]] = []
    for name, spec in regions.items():
        path = spec["device_tree_path"]
        compare_cells(dtb, path, "reg", spec["reg_cells"], errors)
        if dtb.has(path, "no-map") != spec["no_map"]:
            fail(errors, f"{path}: no-map does not match the active reserved-memory policy")
        cells = [int(value, 0) for value in spec["reg_cells"]]
        base = (cells[0] << 32) | cells[1]
        size = (cells[2] << 32) | cells[3]
        intervals.append((base, base + size, name))

    intervals.sort()
    for previous, current in zip(intervals, intervals[1:]):
        if current[0] < previous[1]:
            fail(errors, f"reserved PIL regions overlap: {previous[2]} and {current[2]}")


def check_dynamic(dtb: Dtb, manifest: dict, errors: list[str]) -> int:
    root = "/reserved-memory"
    expected = manifest["dynamic_reserved_memory"]["nodes"]
    names = path_children(dtb, root)
    actual_dynamic: dict[str, str] = {}
    try:
        size_cells = int(dtb.get("-t", "x", root, "#size-cells"), 16)
    except subprocess.CalledProcessError:
        fail(errors, f"{dtb.path}: missing /reserved-memory/#size-cells")
        return 0

    for name in names:
        path = f"{root}/{name}"
        if dtb.has(path, "reg") or not dtb.has(path, "size"):
            continue
        actual_dynamic[name] = path

    if set(actual_dynamic) != set(expected):
        fail(
            errors,
            "dynamic reserved-memory inventory changed: "
            f"missing={sorted(set(expected) - set(actual_dynamic))}, "
            f"unexpected={sorted(set(actual_dynamic) - set(expected))}",
        )

    for name, spec in expected.items():
        path = actual_dynamic.get(name)
        if path is None:
            continue
        if spec.get("native_consumer") != "inactive":
            fail(errors, f"manifest must classify {name} as inactive until it has a physical owner")
        compare_strings(dtb, path, "compatible", [spec["compatible"]], errors)
        if dtb.has(path, "reusable") != spec["reusable"]:
            fail(errors, f"{path}: reusable marker differs from the manifest")
        for prop in ("size", "alignment", "alloc-ranges"):
            expected_value = spec.get({"alloc-ranges": "alloc_ranges"}.get(prop, prop))
            if expected_value is None:
                if dtb.has(path, prop):
                    fail(errors, f"{path}: unexpected {prop}; manifest says absent")
                continue
            try:
                actual = dtb.cells(path, prop)
            except subprocess.CalledProcessError:
                fail(errors, f"{path}: missing/unreadable {prop}")
                continue
            if prop in ("size", "alignment"):
                # These properties are scalar quantities encoded as #size-cells.
                actual_value = sum(value << (32 * (size_cells - index - 1)) for index, value in enumerate(actual))
                wanted_value = int(expected_value, 0)
                if actual_value != wanted_value:
                    fail(errors, f"{path}:{prop}: got {actual_value:#x}, expected {wanted_value:#x}")
            elif actual != [int(value, 0) for value in expected_value]:
                wanted = [int(value, 0) for value in expected_value]
                fail(errors, f"{path}:{prop}: got {[hex(x) for x in actual]}, expected {[hex(x) for x in wanted]}")
    return len(actual_dynamic)


def resolve_reserved_phandle(dtb: Dtb, phandle: int) -> str | None:
    root = "/reserved-memory"
    for child in path_children(dtb, root):
        node = f"{root}/{child}"
        try:
            if dtb.cells(node, "phandle") == [phandle]:
                return node
        except subprocess.CalledProcessError:
            continue
    return None


def _interval(base: int, size: int) -> tuple[int, int]:
    return base, base + size


def _overlap(a: tuple[int, int], b: tuple[int, int]) -> bool:
    return a[0] < b[1] and b[0] < a[1]


def check_native_ipa_authority(dtb: Dtb, ipa: dict, errors: list[str]) -> tuple[int, int, int, int]:
    """Ensure today's owner has not been given unreviewed IPA capabilities."""
    system_path = ROOT / ipa["native_owner"]["system"]
    try:
        system = ET.parse(system_path).getroot()
    except (OSError, ET.ParseError) as exc:
        fail(errors, f"cannot inspect native owner system {system_path}: {exc}")
        return 0, 0, 0, 0

    device_ranges: list[tuple[int, int, str]] = []
    for item in system.findall("memory_region"):
        physical = item.get("phys_addr")
        size = item.get("size")
        if physical is None or size is None:
            continue
        try:
            base, length = int(physical, 0), int(size, 0)
        except ValueError:
            fail(errors, f"native memory region {item.get('name')} has malformed physical bounds")
            continue
        if length > 0:
            device_ranges.append((base, base + length, item.get("name", "unnamed")))

    maps = {item.get("mr") for item in system.iter("map") if item.get("mr")}
    ipa_hw = ipa["hardware"]
    register_cells = [int(value, 0) for value in ipa_hw["reg_cells"]]
    register_ranges = [
        _interval(register_cells[i], register_cells[i + 1])
        for i in range(0, len(register_cells), 2)
    ]
    fw_ranges = []
    for region in (ipa["firmware"]["ipa_region"], ipa["firmware"]["gsi_region"]):
        cells = [int(value, 0) for value in region["reg_cells"]]
        base = (cells[0] << 32) | cells[1]
        length = (cells[2] << 32) | cells[3]
        fw_ranges.append(_interval(base, length))

    ipa_mappings = 0
    firmware_mappings = 0
    for start, end, name in device_ranges:
        if any(_overlap((start, end), wanted) for wanted in register_ranges):
            if name in maps:
                ipa_mappings += 1
        if any(_overlap((start, end), wanted) for wanted in fw_ranges) and name in maps:
            firmware_mappings += 1
            fail(errors, f"native owner maps IPA controller firmware memory through {name}")
    if ipa_mappings:
        fail(errors, "native owner already maps IPA/GSI registers; review this authority change explicitly")

    expected_irqs = {int(value, 0) for value in ipa_hw["sel4_gic_irq_intids"]}
    bound_irqs = {
        int(item.get("irq"), 0)
        for item in system.iter("irq")
        if item.get("irq") is not None and re.fullmatch(r"0[xX][0-9a-fA-F]+|[0-9]+", item.get("irq", ""))
    }
    irq_bindings = len(expected_irqs & bound_irqs)
    if irq_bindings:
        fail(errors, "native owner binds an IPA/GSI interrupt before its runtime is reviewed")

    smmu = dtb.cells("/soc/apps-smmu@0x15000000", "reg")
    smmu_base, smmu_size = smmu[0], smmu[1]
    smmu_interval = _interval(smmu_base, smmu_size)
    smmu_mappings = sum(1 for start, end, name in device_ranges if _overlap((start, end), smmu_interval) and name in maps)
    if smmu_mappings:
        fail(errors, "native owner maps Apps SMMU registers before route/context ownership is reviewed")
    observed = (bool(ipa_mappings), bool(irq_bindings), bool(firmware_mappings), bool(smmu_mappings))
    expected = tuple(
        ipa["native_owner"][field]
        for field in (
            "ipa_registers_mapped",
            "ipa_interrupts_bound",
            "ipa_pas_memory_mapped",
            "ipa_smmu_contexts_owned",
        )
    )
    if observed != expected:
        fail(errors, f"native owner IPA authority snapshot is {observed}, inventory says {expected}")
    return ipa_mappings, irq_bindings, firmware_mappings, smmu_mappings


def check_ipa_resources(dtb: Dtb, manifest: dict, errors: list[str]) -> tuple[int, int, int]:
    """Cross-check the IPA v4.2 platform inventory without granting access."""
    ipa = manifest["ipa"]
    hardware = ipa["hardware"]
    node = hardware["path"]
    compare_strings(dtb, node, "compatible", [hardware["compatible"]], errors)
    compare_strings(dtb, node, "reg-names", hardware["reg_names"], errors)
    compare_cells(dtb, node, "reg", hardware["reg_cells"], errors)
    compare_strings(dtb, node, "interrupt-names", hardware["interrupt_names"], errors)
    compare_cells(dtb, node, "interrupts", hardware["interrupt_cells"], errors)
    compare_cells(dtb, node, "qcom,ipa-hw-ver", [hardware["hardware_version"]], errors)
    compare_cells(dtb, node, "qcom,ipa-hw-mode", [hardware["hardware_mode"]], errors)
    compare_cells(dtb, node, "qcom,ee", [hardware["execution_environment"]], errors)
    if dtb.has(node, "qcom,arm-smmu") != hardware["arm_smmu_enabled"]:
        fail(errors, f"{node}: ARM SMMU attachment differs from the inventory")

    controller = "/soc/apps-smmu@0x15000000"
    try:
        controller_phandle = dtb.cells(controller, "phandle")
        if len(controller_phandle) != 1:
            fail(errors, f"{controller}: expected one phandle")
            controller_phandle = []
    except subprocess.CalledProcessError:
        fail(errors, f"{controller}: missing/unreadable phandle")
        controller_phandle = []

    readback = manifest["smmu_handoff_snapshot"]["latest_targeted_readback_2026_10_21"]["target_sids"]
    saved = ipa["saved_smmu_readback"]
    if set(saved["target_stream_ids"]) != {context["stream_id"] for context in ipa["smmu_contexts"]}:
        fail(errors, "IPA SMMU stream inventory differs from its targeted ABL readback scope")
    for context in ipa["smmu_contexts"]:
        path = context["path"]
        compare_strings(dtb, path, "compatible", [context["compatible"]], errors)
        try:
            iommus = dtb.cells(path, "iommus")
            expected_sid = int(context["stream_id"], 0)
            expected_mask = int(context["stream_mask"], 0)
            if len(iommus) != 3 or iommus[1:] != [expected_sid, expected_mask]:
                fail(errors, f"{path}: iommus SID/mask differs from {context['stream_id']}/{context['stream_mask']}")
            elif controller_phandle and iommus[0] != controller_phandle[0]:
                fail(errors, f"{path}: IOMMU provider is not {controller}")
        except subprocess.CalledProcessError:
            fail(errors, f"{path}: missing/unreadable iommus")
        if dtb.has(path, "qcom,smmu-s1-bypass") != context["stage1_bypass"]:
            fail(errors, f"{path}: stage-1 bypass property differs from the inventory")
        if readback.get(context["stream_id"]) != "no matching route displayed":
            fail(errors, f"saved targeted ABL readback does not show the recorded no-route result for {context['stream_id']}")
        for property_name, key in (
            ("qcom,iova-mapping", "iova_mapping_cells"),
            ("qcom,additional-mapping", "additional_mapping_cells"),
        ):
            if key not in context:
                if dtb.has(path, property_name):
                    fail(errors, f"{path}: unexpected {property_name}")
            else:
                compare_cells(dtb, path, property_name, context[key], errors)

    firmware = ipa["firmware"]
    pas = firmware["pas_node"]
    compare_strings(dtb, pas, "compatible", [firmware["compatible"]], errors)
    compare_cells(dtb, pas, "qcom,pas-id", [firmware["pas_id"]], errors)
    compare_strings(dtb, pas, "qcom,firmware-name", [firmware["firmware_name"]], errors)
    if dtb.has(pas, "qcom,pil-force-shutdown") != firmware["pil_force_shutdown"]:
        fail(errors, f"{pas}: force-shutdown policy differs from the inventory")
    try:
        phandle = dtb.cells(pas, "memory-region")
        if len(phandle) != 1:
            fail(errors, f"{pas}: expected one IPA firmware memory-region phandle")
        else:
            resolved = resolve_reserved_phandle(dtb, phandle[0])
            if resolved != firmware["ipa_region"]["device_tree_path"]:
                fail(errors, f"{pas}: IPA firmware phandle resolves to {resolved!r}")
    except subprocess.CalledProcessError:
        fail(errors, f"{pas}: missing/unreadable memory-region")

    for region in (firmware["ipa_region"], firmware["gsi_region"]):
        compare_cells(dtb, region["device_tree_path"], "reg", region["reg_cells"], errors)
        if dtb.has(region["device_tree_path"], "no-map") != region["no_map"]:
            fail(errors, f"{region['device_tree_path']}: no-map differs from the inventory")

    grants = check_native_ipa_authority(dtb, ipa, errors)
    return grants


def _dts_block(text: str, signature: str) -> str | None:
    start = text.find(signature)
    if start < 0:
        return None
    opening = text.find("{", start + len(signature))
    if opening < 0:
        return None
    depth = 0
    for index in range(opening, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[start:index + 1]
    return None


def _braced_block(text: str, signature: str) -> str | None:
    """Return the C brace block beginning at a unique source signature."""
    start = text.find(signature)
    if start < 0:
        return None
    opening = text.find("{", start + len(signature))
    if opening < 0:
        return None
    depth = 0
    for index in range(opening, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[start:index + 1]
    return None


def parse_qcom_actlr_entries(smmu_block: str) -> list[tuple[int, int, int]]:
    """Parse the three-cell `<SID, mask, ACTLR>` tuples in qcom,actlr."""
    property_match = re.search(r"qcom,actlr\s*=\s*(.*?);", smmu_block, re.DOTALL)
    if property_match is None:
        return []
    value = re.sub(r"/\*.*?\*/|//[^\n]*", " ", property_match.group(1), flags=re.DOTALL)
    entries = []
    for cells in re.findall(r"<([^<>]+)>", value):
        values = cells.split()
        if len(values) != 3:
            continue
        try:
            entries.append(tuple(int(item, 0) for item in values))
        except ValueError:
            continue
    return entries


def actlr_entry_matches_route(
    entry: tuple[int, int, int], route_sid: int, route_mask: int
) -> bool:
    """Use arm_smmu_fwspec_match_smr's masked-ID matching rule."""
    entry_sid, entry_mask, _actlr = entry
    return ((entry_sid ^ route_sid) & ~(entry_mask | route_mask)) == 0


def check_wlan_smmu_actlr_policy(manifest: dict) -> list[str]:
    """Pin source facts while keeping runtime ACTLR state explicitly unknown."""
    errors: list[str] = []
    policy = manifest["wlan_smmu_actlr_policy"]
    kernel_root = ROOT / ".cache/sunfish-lineage-kernel"
    try:
        source_commit = subprocess.check_output(
            ["git", "-C", str(kernel_root), "rev-parse", "HEAD"], text=True
        ).strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        return [f"cannot identify pinned WLAN SMMU source: {exc}"]
    if source_commit != policy["source_commit"] or source_commit != ICNSS_LINEAGE_SOURCE_COMMIT:
        return [f"WLAN SMMU source checkout is {source_commit}, expected {policy['source_commit']}"]

    expected_sources = {
        "icnss_driver": (ICNSS_DRIVER_SOURCE_PATH, ICNSS_DRIVER_SOURCE_SHA256),
        "sunfish_smmu_dts": (WLAN_SMMU_ACTLR_DTS_SOURCE, WLAN_SMMU_ACTLR_DTS_SHA256),
        "arm_smmu_driver": (WLAN_SMMU_ACTLR_DRIVER_SOURCE, WLAN_SMMU_ACTLR_DRIVER_SHA256),
    }
    source_text: dict[str, str] = {}
    for key, (relative_path, expected_sha256) in expected_sources.items():
        record = policy["sources"][key]
        if record["path"] != relative_path or record["sha256"] != expected_sha256:
            errors.append(f"WLAN SMMU policy has an unexpected pinned source for {key}")
            continue
        path = kernel_root / relative_path
        try:
            data = path.read_bytes()
        except OSError as exc:
            errors.append(f"cannot read pinned WLAN SMMU source {relative_path}: {exc}")
            continue
        digest = hashlib.sha256(data).hexdigest()
        if digest != expected_sha256:
            errors.append(f"pinned WLAN SMMU source hash changed: {relative_path}")
            continue
        source_text[key] = data.decode()

    route_sid = int(policy["route"]["stream_id"], 0)
    route_mask = int(policy["route"]["stream_mask"], 0)
    icnss_cells = [int(value, 0) for value in manifest["icnss"]["iommus_cells"]]
    if icnss_cells[1:] != [route_sid, route_mask]:
        errors.append("WLAN ACTLR policy route differs from the ICNSS IOMMU SID and mask")
    aliases = [int(value, 0) for value in policy["route"]["matched_stream_ids"]]
    if aliases != [route_sid, route_sid | route_mask]:
        errors.append("WLAN ACTLR policy aliases do not match the SID mask")
    expected_route_programming = {
        "context_before_route": True,
        "icnss_s1_bypass_s2cr_type": "S2CR_TYPE_TRANS",
        "route_register_write_order": ["S2CR", "SMR"],
        "route_slot_and_cb_source": (
            "Linux allocates a software stream-match slot and context bank, then writes that CB index into S2CR before making SMR valid"
        ),
        "bootloader_route_import": {
            "init_reads": ["SMR", "S2CR"],
            "imports_only": "valid SMR entries whose S2CR type is TRANS",
            "reserves_existing_context_bank": True,
            "device_tree_overlap_requirement": "Exactly one device-tree device must overlap stream IDs already used by the bootloader",
            "scope_caveat": "Pinned Linux HLOS handoff implementation only. It is not a stable API or an seL4 delegation contract. The separately captured post-attach runtime routes were dynamically programmed by Linux and are not the ABL handoff state.",
        },
        "scope_caveat": (
            "Pinned Sunfish Linux HLOS behavior only. It does not prove that a Fastboot-booted seL4 image receives the same Apps-SMMU MMIO mapping, EL access, clocks/power, secure permissions, or exclusive route/context-bank ownership."
        ),
    }
    if policy.get("linux_route_programming") != expected_route_programming:
        errors.append("WLAN Linux route-programming evidence or its seL4 scope caveat changed")

    dts = source_text.get("sunfish_smmu_dts")
    if dts is not None:
        apps_smmu = _dts_block(dts, "apps_smmu: apps-smmu@0x15000000")
        if apps_smmu is None or 'compatible = "qcom,qsmmu-v500";' not in apps_smmu:
            errors.append("Sunfish apps_smmu is not the pinned QSMMUv500 node")
        else:
            if "qcom,skip-init;" not in apps_smmu:
                errors.append("Sunfish apps_smmu no longer declares qcom,skip-init")
            entries = parse_qcom_actlr_entries(dts[dts.find("&apps_smmu") :])
            expected_entries = [
                tuple(int(value, 0) for value in row)
                for row in policy["apps_smmu_actlr_entries"]
            ]
            if entries != expected_entries:
                errors.append(f"Sunfish apps_smmu ACTLR entries changed: {entries}")
            matches = [
                entry
                for entry in entries
                if any(actlr_entry_matches_route(entry, sid, route_mask) for sid in aliases)
            ]
            expected_matches = policy["matching_actlr_entries"]
            parsed_expected_matches = [
                tuple(int(value, 0) for value in row) for row in expected_matches
            ]
            if matches != parsed_expected_matches:
                errors.append(f"WLAN route ACTLR matches changed: {matches}")

    icnss_driver = source_text.get("icnss_driver")
    if icnss_driver is not None:
        icnss_compact = re.sub(r"\s+", " ", icnss_driver)
        if not re.search(
            r'of_property_read_bool\(pdev->dev\.of_node, "qcom,smmu-s1-bypass"\)\) '
            r"priv->bypass_s1_smmu = true;",
            icnss_compact,
        ):
            errors.append("pinned ICNSS driver no longer derives S1-bypass policy from the DT property")
        init = _braced_block(icnss_driver, "static int icnss_smmu_init(")
        if (
            init is None
            or not re.search(
                r"if\s*\(priv->bypass_s1_smmu\)\s*\{.*?"
                r"iommu_domain_set_attr\(mapping->domain,\s*"
                r"DOMAIN_ATTR_S1_BYPASS,\s*&s1_bypass\);",
                init,
                re.DOTALL,
            )
            or "arm_iommu_attach_device(&priv->pdev->dev, mapping)" not in re.sub(r"\s+", " ", init)
        ):
            errors.append("pinned ICNSS SMMU setup no longer applies the bypass attribute before attach")

    driver = source_text.get("arm_smmu_driver")
    if driver is not None:
        compact = re.sub(r"\s+", " ", driver)
        required_patterns = (
            (r"smmu->cbs = devm_kcalloc\(smmu->dev, smmu->num_context_banks, sizeof\(\*smmu->cbs\), GFP_KERNEL\)", "zeroed context-bank allocation"),
            (r"ARM_SMMU_MATCH_DATA\(qcom_smmuv500, ARM_SMMU_V2, QCOM_SMMUV500, &qsmmuv500_arch_ops\)", "QSMMUv500 architecture-op binding"),
            (r"\{ \.compatible = \"qcom,qsmmu-v500\", \.data = &qcom_smmuv500 \}", "QSMMUv500 compatible binding"),
            (r"\(\(smr->id \^ smr2->id\) & ~\(smr->mask \| smr2->mask\)\)", "masked stream-ID matching formula"),
            (r"writel_relaxed\(cb->actlr, cb_base \+ ARM_SMMU_CB_ACTLR\)", "context-bank ACTLR write"),
        )
        for pattern, label in required_patterns:
            if not re.search(pattern, compact):
                errors.append(f"pinned arm-smmu.c no longer confirms {label}")

        context = _braced_block(driver, "static int arm_smmu_init_domain_context(")
        write_context = _braced_block(driver, "static void arm_smmu_write_context_bank(")
        context_compact = re.sub(r"\s+", " ", context or "")
        write_compact = re.sub(r"\s+", " ", write_context or "")
        if "case ARM_SMMU_DOMAIN_S1: cfg->cbar = CBAR_TYPE_S1_TRANS_S2_BYPASS;" not in context_compact:
            errors.append("pinned arm-smmu.c no longer configures S1 domains with S2 bypass")
        if (
            "if ((!(attributes & (1 << DOMAIN_ATTR_S1_BYPASS)) &&" not in write_compact
            or "|| !stage1) reg |= SCTLR_M;" not in write_compact
        ):
            errors.append("pinned arm-smmu.c SCTLR.M gate for stage-1 bypass changed")

        write_smr = _braced_block(driver, "static void arm_smmu_write_smr(")
        write_s2cr = _braced_block(driver, "static void arm_smmu_write_s2cr(")
        write_sme = _braced_block(driver, "static void arm_smmu_write_sme(")
        route_attach = _braced_block(driver, "static int arm_smmu_domain_add_master(")
        attach_dev = _braced_block(driver, "static int arm_smmu_attach_dev(")
        write_smr_compact = re.sub(r"\s+", " ", write_smr or "")
        write_s2cr_compact = re.sub(r"\s+", " ", write_s2cr or "")
        write_sme_compact = re.sub(r"\s+", " ", write_sme or "")
        route_attach_compact = re.sub(r"\s+", " ", route_attach or "")
        attach_dev_compact = re.sub(r"\s+", " ", attach_dev or "")
        if (
            "writel_relaxed(reg, ARM_SMMU_GR0(smmu) + ARM_SMMU_GR0_SMR(idx));"
            not in write_smr_compact
        ):
            errors.append("pinned arm-smmu.c no longer writes the stream-match register directly")
        if (
            "writel_relaxed(reg, ARM_SMMU_GR0(smmu) + ARM_SMMU_GR0_S2CR(idx));"
            not in write_s2cr_compact
        ):
            errors.append("pinned arm-smmu.c no longer writes the stream-to-context route directly")
        if not re.search(
            r"arm_smmu_write_s2cr\(smmu, idx\); if \(smmu->smrs\) arm_smmu_write_smr\(smmu, idx\);",
            write_sme_compact,
        ):
            errors.append("pinned arm-smmu.c no longer writes S2CR before SMR")
        if not re.search(
            r"if \(smmu_domain->stage == ARM_SMMU_DOMAIN_BYPASS\) type = S2CR_TYPE_BYPASS; else type = S2CR_TYPE_TRANS;",
            route_attach_compact,
        ) or not re.search(
            r"s2cr\[idx\]\.type = type; .*?s2cr\[idx\]\.cbndx = cbndx; arm_smmu_write_sme\(smmu, idx\);",
            route_attach_compact,
        ):
            errors.append("pinned arm-smmu.c route attach no longer binds S2CR to the allocated context bank")
        context_init_pos = attach_dev_compact.find(
            "arm_smmu_init_domain_context(domain, smmu, dev)"
        )
        route_write_pos = attach_dev_compact.find(
            "arm_smmu_domain_add_master(smmu_domain, fwspec)"
        )
        if context_init_pos < 0 or route_write_pos < 0 or context_init_pos >= route_write_pos:
            errors.append("pinned arm-smmu.c no longer initializes the context before adding the route")

        handoff = _braced_block(driver, "static int arm_smmu_handoff_cbs(")
        handoff_compact = re.sub(r"\s+", " ", handoff or "")
        handoff_requirements = (
            "readl_relaxed(ARM_SMMU_GR0(smmu) + ARM_SMMU_GR0_SMR(i))",
            "if (!(raw_smr & SMR_VALID)) continue;",
            "raw_s2cr = readl_relaxed(ARM_SMMU_GR0(smmu) + ARM_SMMU_GR0_S2CR(i));",
            "if (s2cr.type != S2CR_TYPE_TRANS) continue;",
            "smmu->smrs[i] = smr; smmu->s2crs[i] = s2cr;",
            "bitmap_set(smmu->context_map, s2cr.cbndx, 1);",
        )
        if handoff is None or any(item not in handoff_compact for item in handoff_requirements):
            errors.append("pinned arm-smmu.c bootloader route handoff behavior changed")
        if not re.search(
            r"there must be exactly one device in devicetree with stream-ids \* overlapping those used by the bootloader\.",
            compact,
        ):
            errors.append("pinned arm-smmu.c bootloader route handoff overlap requirement changed")
        init = _braced_block(driver, "static int arm_smmu_device_dt_probe(")
        if init is None or "arm_smmu_handoff_cbs(smmu)" not in re.sub(r"\s+", " ", init):
            errors.append("pinned arm-smmu.c no longer imports bootloader routes during SMMU probe")

        ops = _braced_block(driver, "struct arm_smmu_arch_ops qsmmuv500_arch_ops =")
        if ops is None or ".init_context_bank = qsmmuv500_init_cb" not in ops or ".device_reset" in ops:
            errors.append("QSMMUv500 ops no longer show the audited init hook/no-reset-hook policy")
        group = _braced_block(driver, "static int qsmmuv500_device_group(")
        init_cb = _braced_block(driver, "static void qsmmuv500_init_cb(")
        init_context = _braced_block(driver, "static void arm_smmu_init_context_bank(")
        destroy_context = _braced_block(driver, "static void arm_smmu_destroy_domain_context(")
        if group is None or "arm_smmu_fwspec_match_smr(fwspec, smr)" not in group or "iommudata->has_actlr = true" not in group:
            errors.append("QSMMUv500 ACTLR table-match behavior changed")
        if init_cb is None or "if (!iommudata->has_actlr) return;" not in re.sub(r"\s+", " ", init_cb):
            errors.append("QSMMUv500 no-match ACTLR initialization behavior changed")
        if init_context is None or re.search(r"cb->actlr\s*=", init_context):
            errors.append("generic context-bank initialization now assigns ACTLR; re-audit the policy")
        if destroy_context is None or re.search(r"(?:cbs\[[^]]+\]|cb)->actlr\s*=", destroy_context):
            errors.append("context destruction now clears or changes ACTLR; re-audit the policy")

    derived = policy["source_derived_behavior"]
    if derived["fresh_context_actlr"] != "0x00000000":
        errors.append("fresh zero-allocated unmatched Linux context-bank ACTLR must remain 0")
    expected_fresh_scope = (
        "Only a newly zero-allocated software context-bank record with no matching "
        "qcom,actlr entry; Linux writes that stored zero during non-dynamic context setup"
    )
    if derived["fresh_context_actlr_scope"] != expected_fresh_scope:
        errors.append("fresh-context ACTLR scope must remain limited to the audited Linux path")
    if not derived["apps_smmu_skip_init"]:
        errors.append("Sunfish apps_smmu skip-init caveat must remain explicit")
    if derived["live_context_actlr"] != "unknown" or derived["zero_is_safe_runtime_default"]:
        errors.append("WLAN ACTLR policy must not claim zero as the live or safe runtime value")
    if derived["qsmmuv500_has_device_reset_hook"] or derived["context_destroy_clears_actlr"]:
        errors.append("WLAN ACTLR lifecycle caveat disagrees with the pinned QSMMUv500 source")
    return errors


def check_wlan_route_source_comparison(manifest: dict) -> list[str]:
    """Keep the captured route distinct from conflicting public DTS records."""
    errors: list[str] = []
    policy = manifest["wlan_smmu_actlr_policy"]
    comparison = policy.get("route_source_comparison", {})
    runtime = comparison.get("runtime_capture", {})
    mainline = comparison.get("upstream_sunfish_dts", {})
    lineage_generic = comparison.get("lineage_generic_sm6150_dts", {})
    lineage_base = comparison.get("lineage_sunfish_base_dts", {})
    lineage_overlay = comparison.get("lineage_sunfish_overlay", {})
    lineage_runtime = comparison.get("lineage_runtime_observation", {})
    runtime_routes = comparison.get("lineage_runtime_route_capture", {})

    expected_runtime_cells = ["0x240", "0x1"]
    if (
        runtime.get("source")
        != ".cache/pixel-sunfish/runtime-dt-20261024.dtb, captured from /sys/firmware/fdt"
        or runtime.get("node") != manifest["icnss"]["path"]
        or runtime.get("iommus_cells_without_phandle") != expected_runtime_cells
        or manifest["icnss"]["iommus_cells"][1:] != expected_runtime_cells
    ):
        errors.append("captured ICNSS route evidence must remain recorded as runtime 0x240/0x1")

    expected_mainline = {
        "repository": "https://github.com/sm7150-mainline/linux.git",
        "commit": WLAN_MAINLINE_SOURCE_COMMIT,
        "path": WLAN_MAINLINE_WIFI_DTS_PATH,
        "sha256": WLAN_MAINLINE_WIFI_DTS_SHA256,
        "node": "wifi: wifi@18800000",
        "compatible": "qcom,wcn3990-wifi",
        "iommus_cells_without_phandle": ["0x240", "0x1"],
        "target_dts_enables_node": True,
    }
    expected_lineage_generic = {
        "repository": "https://github.com/LineageOS/android_kernel_google_msm-4.14.git",
        "commit": ICNSS_LINEAGE_SOURCE_COMMIT,
        "path": WLAN_LINEAGE_GENERIC_SM6150_DTS_PATH,
        "sha256": WLAN_LINEAGE_GENERIC_SM6150_DTS_SHA256,
        "node": "icnss: qcom,icnss@18800000",
        "compatible": "qcom,icnss",
        "iommus_cells_without_phandle": ["0x1640", "0x1"],
        "s1_bypass_property": True,
        "scope_note": "Generic SM6150 DTS; not the Sunfish DVT overlay base",
    }
    expected_lineage_base = {
        "repository": "https://github.com/LineageOS/android_kernel_google_msm-4.14.git",
        "commit": ICNSS_LINEAGE_SOURCE_COMMIT,
        "base_dtb": "qcom-base/sdmmagpie.dtb",
        "dts_path": WLAN_LINEAGE_SUNFISH_BASE_DTS_PATH,
        "dts_sha256": WLAN_LINEAGE_SUNFISH_BASE_DTS_SHA256,
        "dtsi_path": WLAN_LINEAGE_SUNFISH_BASE_DTSI_PATH,
        "dtsi_sha256": WLAN_LINEAGE_SUNFISH_BASE_DTSI_SHA256,
        "node": "icnss: qcom,icnss@18800000",
        "compatible": "qcom,icnss",
        "iommus_cells_without_phandle": ["0x240", "0x1"],
        "s1_bypass_property": True,
    }
    expected_lineage_overlay = {
        "google_makefile_path": WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_PATH,
        "google_makefile_sha256": WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_SHA256,
        "overlay_target": "sm7150-sunfish-dvt1.0-overlay.dtbo",
        "base_dtb": "qcom-base/sdmmagpie.dtb",
        "overlay_path": WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_PATH,
        "overlay_sha256": WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_SHA256,
        "overlay_direct_icnss_or_iommus_override": False,
    }
    expected_lineage_runtime = {
        "capture_date": "2026-10-24",
        "device_tree_model": "Google Inc. MSM sm7150 Sunfish DVT1.0",
        "build_fingerprint": "google/sunfish/sunfish:13/TQ3A.230805.001.S1/10786265:user/release-keys",
        "kernel_release": "4.14.357-openela-g28f9290ae067",
        "live_fdt_path": ".cache/pixel-sunfish/live-lineage-fdt-20261024.dtb",
        "live_fdt_sha256": "d5e8c8e44e46cb77ebcbf99da47cbcfcc92f26831444f474bc8e887952282f1d",
        "icnss_iommus_cells_be": ["0x30", "0x240", "0x1"],
        "apps_smmu_of_node": "/sys/firmware/devicetree/base/soc/apps-smmu@0x15000000",
        "apps_smmu_fdt_node": "/soc/apps-smmu@0x15000000",
        "icnss_driver": "icnss",
        "icnss_iommu_group": "7",
        "iommu_group_devices": ["18800000.qcom,icnss"],
        "wifi_enabled_and_connected": True,
        "scope": "Confirms the live Linux DT binding and device-group attachment only. The separately recorded diagnostic route capture provides post-attach S2CR/CB state; neither record delegates SMMU control to seL4.",
    }
    expected_runtime_capture = {
        "capture_date": "2026-10-09",
        "device_state": "Temporary fastboot boot of a diagnostic LineageOS kernel; no partition was flashed.",
        "kernel_release": "4.14.357-openela-g28f9290ae067",
        "kernel_source_commit": ICNSS_LINEAGE_SOURCE_COMMIT,
        "diagnostic_patch_path": "patches/lineage-sunfish-smmu-route-audit.patch",
        "diagnostic_patch_sha256": "7efdc523e9aeda205de864b7c9696ee8d3a0843f73a554905b1c808b213b26ed",
        "readback_path": "platforms/sunfish/fixtures/lineage-smmu-runtime-route-readback.log",
        "readback_sha256": "8ac23109a69251005219912cd927911e33a1e740f1e35e64f1a0e262bf2a3626",
        "capture_method": "The diagnostic patch reads SMR, S2CR, CBAR, and SCTLR after the matched Linux attach path programs its normal route. It adds no SMMU writes; ordinary Linux route setup remains active.",
        "usb_context_history": [1, 23],
        "scope": "Post-attach LineageOS Linux route state only. It is distinct from the earlier ABL snapshot, where these candidate routes were absent. Linux dynamically programmed these routes; this capture does not establish state inherited by a Fastboot-booted seL4 image, authority to reproduce them, or ownership of a translated DMA context.",
    }
    expected_runtime_route_rows = [
        {"name": "icnss_wlan", "sid": "0x240", "mask": "0x1", "device": "18800000.qcom,icnss", "slot": 13, "stage": 0, "s2cr_type": 0, "context_bank": 6, "smr": "0x80010240", "s2cr": "0x00000306", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
        {"name": "usb_initial", "sid": "0x540", "mask": "0x0", "device": "a600000.ssusb", "slot": 3, "stage": 0, "s2cr_type": 0, "context_bank": 1, "smr": "0x80000540", "s2cr": "0x00000301", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
        {"name": "display", "sid": "0x800", "mask": "0x440", "device": "ae00000.qcom,mdss_mdp", "slot": 5, "stage": 0, "s2cr_type": 0, "context_bank": 3, "smr": "0x84400800", "s2cr": "0x00000303", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
        {"name": "usb_final", "sid": "0x540", "mask": "0x0", "device": "a600000.ssusb", "slot": 3, "stage": 0, "s2cr_type": 0, "context_bank": 23, "smr": "0x80000540", "s2cr": "0x00000317", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
        {"name": "ipa_ap", "sid": "0x520", "mask": "0x0", "device": "soc:ipa_smmu_ap", "slot": 8, "stage": 0, "s2cr_type": 0, "context_bank": 37, "smr": "0x80000520", "s2cr": "0x00000325", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
        {"name": "ipa_wlan", "sid": "0x521", "mask": "0x0", "device": "soc:ipa_smmu_wlan", "slot": 9, "stage": 0, "s2cr_type": 0, "context_bank": 38, "smr": "0x80000521", "s2cr": "0x00000326", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
        {"name": "ipa_uc", "sid": "0x522", "mask": "0x0", "device": "soc:ipa_smmu_uc", "slot": 10, "stage": 0, "s2cr_type": 0, "context_bank": 39, "smr": "0x80000522", "s2cr": "0x00000327", "cbar": "0x0001f300", "sctlr": "0x00c000e6"},
    ]
    if mainline != expected_mainline:
        errors.append("upstream Sunfish WLAN DTS source comparison changed")
    if lineage_generic != expected_lineage_generic:
        errors.append("generic Lineage SM6150 DTS comparison changed")
    if lineage_base != expected_lineage_base:
        errors.append("Lineage Sunfish sdmmagpie base-DTS comparison changed")
    if lineage_overlay != expected_lineage_overlay:
        errors.append("Lineage Sunfish DVT overlay selection changed")
    if lineage_runtime != expected_lineage_runtime:
        errors.append("read-only Lineage runtime ICNSS/IOMMU observation changed")
    if not isinstance(runtime_routes, dict):
        errors.append("Lineage runtime SMMU route capture is missing")
        runtime_routes = {}
    runtime_metadata = {key: value for key, value in runtime_routes.items() if key != "routes"}
    if runtime_metadata != expected_runtime_capture or runtime_routes.get("routes") != expected_runtime_route_rows:
        errors.append("Lineage runtime SMMU route capture metadata or rows changed")
    try:
        fixture_path = ROOT / expected_runtime_capture["readback_path"]
        patch_path = ROOT / expected_runtime_capture["diagnostic_patch_path"]
        fixture_bytes = fixture_path.read_bytes()
        patch_bytes = patch_path.read_bytes()
        if hashlib.sha256(fixture_bytes).hexdigest() != expected_runtime_capture["readback_sha256"]:
            errors.append("Lineage runtime SMMU route readback fixture hash changed")
        if hashlib.sha256(patch_bytes).hexdigest() != expected_runtime_capture["diagnostic_patch_sha256"]:
            errors.append("Lineage runtime SMMU route diagnostic patch hash changed")
        parsed_routes = parse_route_readback(fixture_bytes.decode())
        for row in expected_runtime_route_rows:
            require_route(
                parsed_routes,
                sid=int(row["sid"], 0),
                mask=int(row["mask"], 0),
                device=row["device"],
                slot=row["slot"],
                context_bank=row["context_bank"],
                cbar=int(row["cbar"], 0),
                sctlr=int(row["sctlr"], 0),
            )
            selected = next(
                item for item in parsed_routes
                if item.sid == int(row["sid"], 0)
                and item.mask == int(row["mask"], 0)
                and item.device == row["device"]
                and item.slot == row["slot"]
                and item.context_bank == row["context_bank"]
            )
            for field in ("stage", "s2cr_type"):
                if getattr(selected, field) != row[field]:
                    errors.append(f"runtime route {row['name']} {field} changed")
            for field in ("smr", "s2cr", "cbar", "sctlr"):
                if getattr(selected, field) != int(row[field], 0):
                    errors.append(f"runtime route {row['name']} {field} changed")
        require_usb_context_reassignment(parsed_routes)
    except (OSError, UnicodeDecodeError, RouteReadbackError, StopIteration) as exc:
        errors.append(f"cannot validate Lineage runtime SMMU route readback: {exc}")
    if not comparison.get("resolution", "").startswith(
        "The apparent SID mismatch came from comparing the generic sm6150.dtsi with Sunfish."
    ):
        errors.append("Lineage Sunfish route resolution must identify the actual sdmmagpie build base")

    mainline_root = ROOT / ".cache/sunfish-linux-mainline-ref"
    lineage_root = ROOT / ".cache/sunfish-lineage-kernel"
    try:
        mainline_commit = subprocess.check_output(
            ["git", "-C", str(mainline_root), "rev-parse", "HEAD"], text=True
        ).strip()
        lineage_commit = subprocess.check_output(
            ["git", "-C", str(lineage_root), "rev-parse", "HEAD"], text=True
        ).strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        return errors + [f"cannot identify WLAN route DTS source checkouts: {exc}"]
    if mainline_commit != WLAN_MAINLINE_SOURCE_COMMIT:
        errors.append(f"upstream Sunfish DTS checkout is {mainline_commit}, expected {WLAN_MAINLINE_SOURCE_COMMIT}")
    if lineage_commit != ICNSS_LINEAGE_SOURCE_COMMIT:
        errors.append(f"Lineage ICNSS DTS checkout is {lineage_commit}, expected {ICNSS_LINEAGE_SOURCE_COMMIT}")

    pinned_files = (
        (mainline_root, WLAN_MAINLINE_WIFI_DTS_PATH, WLAN_MAINLINE_WIFI_DTS_SHA256),
        (mainline_root, WLAN_MAINLINE_SUNFISH_DTS_PATH, WLAN_MAINLINE_SUNFISH_DTS_SHA256),
        (lineage_root, WLAN_LINEAGE_GENERIC_SM6150_DTS_PATH, WLAN_LINEAGE_GENERIC_SM6150_DTS_SHA256),
        (lineage_root, WLAN_LINEAGE_SUNFISH_BASE_DTS_PATH, WLAN_LINEAGE_SUNFISH_BASE_DTS_SHA256),
        (lineage_root, WLAN_LINEAGE_SUNFISH_BASE_DTSI_PATH, WLAN_LINEAGE_SUNFISH_BASE_DTSI_SHA256),
        (lineage_root, WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_PATH, WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_SHA256),
        (lineage_root, WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_PATH, WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_SHA256),
    )
    source_text: dict[str, str] = {}
    for root, relative_path, expected_sha256 in pinned_files:
        try:
            data = (root / relative_path).read_bytes()
        except OSError as exc:
            errors.append(f"cannot read pinned WLAN route DTS {relative_path}: {exc}")
            continue
        if hashlib.sha256(data).hexdigest() != expected_sha256:
            errors.append(f"pinned WLAN route DTS hash changed: {relative_path}")
            continue
        source_text[relative_path] = data.decode()

    mainline_dts = source_text.get(WLAN_MAINLINE_WIFI_DTS_PATH)
    if mainline_dts is not None:
        block = _dts_block(mainline_dts, "wifi: wifi@18800000")
        if (
            block is None
            or 'compatible = "qcom,wcn3990-wifi";' not in block
            or not re.search(r"iommus\s*=\s*<&apps_smmu\s+0x240\s+0x1>", block)
        ):
            errors.append("upstream Sunfish WCN3990 node no longer declares Apps-SMMU SID/mask 0x240/0x1")
    target_dts = source_text.get(WLAN_MAINLINE_SUNFISH_DTS_PATH)
    if target_dts is not None:
        wifi_start = target_dts.find("&wifi {")
        block = (
            _dts_block(target_dts[wifi_start:], "&wifi")
            if wifi_start >= 0
            else None
        )
        if block is None or not re.search(r'status\s*=\s*"okay"\s*;', block):
            errors.append("upstream Sunfish target DTS no longer enables the WCN3990 node")

    lineage_generic_dts = source_text.get(WLAN_LINEAGE_GENERIC_SM6150_DTS_PATH)
    if lineage_generic_dts is not None:
        block = _dts_block(lineage_generic_dts, "icnss: qcom,icnss@18800000")
        if (
            block is None
            or 'compatible = "qcom,icnss";' not in block
            or not re.search(r"iommus\s*=\s*<&apps_smmu\s+0x1640\s+0x1>", block)
            or "qcom,smmu-s1-bypass;" not in block
        ):
            errors.append("generic Lineage SM6150 ICNSS node no longer declares SID/mask 0x1640/0x1")

    lineage_base_dts = source_text.get(WLAN_LINEAGE_SUNFISH_BASE_DTS_PATH)
    lineage_base_dtsi = source_text.get(WLAN_LINEAGE_SUNFISH_BASE_DTSI_PATH)
    if lineage_base_dts is not None and '#include "sdmmagpie.dtsi"' not in lineage_base_dts:
        errors.append("Lineage sdmmagpie.dts no longer includes the Sunfish base DTSI")
    if lineage_base_dtsi is not None:
        block = _dts_block(lineage_base_dtsi, "icnss: qcom,icnss@18800000")
        if (
            block is None
            or 'compatible = "qcom,icnss";' not in block
            or not re.search(r"iommus\s*=\s*<&apps_smmu\s+0x0*240\s+0x1>", block)
            or "qcom,smmu-s1-bypass;" not in block
        ):
            errors.append("Lineage Sunfish sdmmagpie ICNSS node no longer declares SID/mask 0x240/0x1")

    google_makefile = source_text.get(WLAN_LINEAGE_GOOGLE_DTS_MAKEFILE_PATH, "")
    if "sm7150-sunfish-dvt1.0-overlay.dtbo-base := qcom-base/sdmmagpie.dtb" not in google_makefile:
        errors.append("Lineage Sunfish DVT1.0 overlay no longer uses the sdmmagpie base DTB")
    dvt_overlay = source_text.get(WLAN_LINEAGE_SUNFISH_DVT_OVERLAY_PATH, "")
    if re.search(r"\b(?:icnss|iommus|apps_smmu)\b", dvt_overlay, re.IGNORECASE):
        errors.append("Lineage Sunfish DVT1.0 overlay now directly overrides ICNSS/IOMMU properties")
    runtime_cells = lineage_runtime.get("icnss_iommus_cells_be", [])
    try:
        runtime_cells_u32 = [int(cell, 0) for cell in runtime_cells]
    except (TypeError, ValueError):
        runtime_cells_u32 = []
    if runtime_cells_u32 != [0x30, 0x240, 1]:
        errors.append("live Lineage ICNSS iommus property no longer decodes to phandle 0x30, SID 0x240, mask 1")
    live_fdt_path = ROOT / lineage_runtime.get("live_fdt_path", "")
    try:
        live_fdt_bytes = live_fdt_path.read_bytes()
        if hashlib.sha256(live_fdt_bytes).hexdigest() != lineage_runtime.get("live_fdt_sha256"):
            errors.append("saved live Lineage FDT hash does not match the runtime observation")
        live_dtb = Dtb(live_fdt_path)
        live_icnss_cells = live_dtb.cells("/soc/qcom,icnss@18800000", "iommus")
        apps_smmu_node = lineage_runtime.get("apps_smmu_fdt_node", "")
        apps_smmu_phandle = live_dtb.cells(apps_smmu_node, "phandle")
        if live_icnss_cells != [0x30, 0x240, 1] or apps_smmu_phandle != [0x30]:
            errors.append("saved live Lineage FDT does not match the recorded ICNSS SID and Apps-SMMU phandle")
    except (OSError, subprocess.CalledProcessError, ValueError) as exc:
        errors.append(f"cannot verify saved live Lineage FDT route: {exc}")

    return errors


def check_sel4_smmu_support(manifest: dict) -> list[str]:
    """Pin the distinction between generic seL4 SMMU APIs and Sunfish support."""
    errors: list[str] = []
    record = manifest.get("sel4_smmu_support", {})
    sel4 = record.get("sel4", {})
    microkit = record.get("microkit", {})
    sunfish = record.get("sunfish_port", {})
    sel4_root = ROOT / ".cache/sel4-src-16.0-sunfish"
    microkit_root = ROOT / ".cache/microkit-src-2.3.1-sunfish"

    for root, commit, label in (
        (sel4_root, SEL4_SMMU_SOURCE_COMMIT, "seL4 SMMU"),
        (microkit_root, MICROKIT_SMMU_SOURCE_COMMIT, "Microkit SMMU"),
    ):
        try:
            actual = subprocess.check_output(
                ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
            ).strip()
        except (OSError, subprocess.CalledProcessError) as exc:
            errors.append(f"cannot identify {label} source checkout: {exc}")
            continue
        expected = sel4.get("commit") if label == "seL4 SMMU" else microkit.get("commit")
        if actual != commit or actual != expected:
            errors.append(f"{label} source checkout is {actual}, expected {commit}")

    files = (
        (sel4_root, sel4.get("arm_smmu_config", {}), "seL4 ARM SMMU config"),
        (sel4_root, sel4.get("smmu_v2_driver", {}), "seL4 SMMUv2 driver"),
        (sel4_root, sel4.get("hardware_header", {}), "seL4 SMMUv2 hardware header"),
        (sel4_root, sel4.get("api_manual", {}), "seL4 SMMU manual"),
        (sel4_root, sel4.get("caveats", {}), "seL4 SMMU caveats"),
        (microkit_root, microkit.get("iommu_parser", {}), "Microkit IOMMU parser"),
        (microkit_root, microkit.get("kernel_config_detection", {}), "Microkit kernel config parser"),
        (microkit_root, microkit.get("aarch64_rejection_test", {}), "Microkit AArch64 IOMMU test"),
        (microkit_root, microkit.get("cspace_cap_mapping", {}), "Microkit CSpace cap mapping"),
        (microkit_root, microkit.get("capdl_smmu_support", {}), "Microkit CapDL SMMU support"),
    )
    source_text: dict[str, str] = {}
    for root, item, label in files:
        relative = item.get("path")
        expected_hash = item.get("sha256")
        if not relative or not expected_hash:
            errors.append(f"{label} source inventory is incomplete")
            continue
        path = root / relative
        try:
            data = path.read_bytes()
        except OSError as exc:
            errors.append(f"cannot read {label} source {relative}: {exc}")
            continue
        if hashlib.sha256(data).hexdigest() != expected_hash:
            errors.append(f"{label} source hash changed: {relative}")
            continue
        source_text[label] = data.decode()

    config_text = source_text.get("seL4 ARM SMMU config", "")
    if 'DEPENDS "KernelPlatformTx2"' not in config_text or 'config_option(KernelArmSMMU ARM_SMMU "Enable SystemMMU"' not in config_text:
        errors.append("generic seL4 ARM_SMMU config is no longer restricted to TX2")
    manual = source_text.get("seL4 SMMU manual", "")
    if "Arm SMMU version 2.0" not in manual or r"seL4\_ARM\_SID\_BindCB" not in manual:
        errors.append("pinned seL4 manual no longer documents the Arm SMMUv2 capability API")
    caveats = source_text.get("seL4 SMMU caveats", "")
    if "AArch64: Armv8-a" not in caveats or "no SMMU" not in caveats:
        errors.append("pinned seL4 caveats no longer exclude SMMU from verified AArch64 configurations")
    if sel4.get("caveats", {}).get("verified_aarch64_configurations_enable_smmu") is not False:
        errors.append("seL4 caveat record must state that verified AArch64 configs exclude SMMU")
    driver = source_text.get("seL4 SMMUv2 driver", "")
    if "been only tested on the TX2 platform" not in driver or "device space to be configured statically" not in driver:
        errors.append("generic seL4 SMMUv2 static-map/TX2 test limitations changed")
    for required in (
        "smmu_mapping_init();\n    smmu_dev_reset();",
        "smmu_write_reg32(SMMU_GR0_PPTR, SMMU_S2CRn(i), reg);",
        "smmu_write_reg32(SMMU_GR0_PPTR, SMMU_SMRn(i), reg);",
        "smmu_write_reg32(cb_bank_ptr, SMMU_CBn_SCTLR, 0);",
        "reg &= ~CBn_ACTLR_CPRE;",
        "smmu_write_reg32(SMMU_GR0_PPTR, SMMU_TLBIALLNSNH, SMMU_TLB_INVALL_MASK);",
    ):
        if required not in driver:
            errors.append("generic seL4 SMMUv2 reset/route behavior changed")
            break
    expected_reset_effect = (
        "Sets every stream route to context-bank type with default CB index 0, invalidates every stream-match entry, "
        "disables all context banks, clears their fault state, clears ACTLR.CPRE on each bank, invalidates SMMU TLBs, "
        "then enables the SMMU"
    )
    if sel4.get("smmu_v2_driver", {}).get("reset_effect") != expected_reset_effect:
        errors.append("SMMUv2 reset summary must preserve all route, CB, ACTLR, and TLB side effects")
    expected_mapping_assumptions = (
        "The mapping code says it was tested only on TX2, assumes a static kernel virtual mapping for SMMU register windows, "
        "and maps each context bank into that fixed window."
    )
    if sel4.get("smmu_v2_driver", {}).get("mapping_assumptions") != expected_mapping_assumptions:
        errors.append("SMMUv2 mapping summary must preserve its static-window/TX2-only assumptions")
    header = source_text.get("seL4 SMMUv2 hardware header", "")
    if "#define SMMU_TX2_PADDR                         0x12000000" not in header:
        errors.append("generic seL4 SMMUv2 header no longer hardcodes the audited TX2 base")
    if sel4.get("hardware_header", {}).get("fixed_controller_base") != "0x12000000":
        errors.append("seL4 SMMU hardware-header base record changed")

    platform_path = ROOT / sunfish.get("config_path", "")
    try:
        platform_data = platform_path.read_bytes()
        platform_text = platform_data.decode()
        if hashlib.sha256(platform_data).hexdigest() != sunfish.get("config_sha256"):
            errors.append("Sunfish seL4 platform config hash changed")
        if "set(KernelArmSMMU OFF)" not in platform_text:
            errors.append("Sunfish platform config no longer disables generic ARM_SMMU")
    except OSError as exc:
        errors.append(f"cannot read Sunfish seL4 platform config: {exc}")
    if sunfish.get("apps_smmu_runtime_base") != "0x15000000":
        errors.append("Sunfish Apps SMMU runtime base must remain distinct from the TX2 driver base")

    generated_path = ROOT / sunfish.get("generated_config_path", "")
    try:
        generated = json.loads(generated_path.read_text())
        if generated.get("ARM_SMMU") is not False or sunfish.get("generated_arm_smmu") is not False:
            errors.append("generated Sunfish kernel config unexpectedly enables ARM_SMMU")
    except (OSError, json.JSONDecodeError) as exc:
        errors.append(f"cannot read generated Sunfish kernel config: {exc}")

    parser_source = source_text.get("Microkit IOMMU parser", "")
    if "Arch::Aarch64 | Arch::Riscv64" not in parser_source or "UnsupportedArch" not in parser_source:
        errors.append("Microkit IOMMU parser no longer rejects AArch64 identifiers")
    sel4_config_source = source_text.get("Microkit kernel config parser", "")
    if "Arch::X86_64 => json_str_as_bool(&kernel_config_json, \"IOMMU\")?" not in sel4_config_source:
        errors.append("Microkit kernel config parser's architecture-specific IOMMU rule changed")
    test_source = source_text.get("Microkit AArch64 IOMMU test", "")
    if "fn test_iommu_unsupported_on_aarch64()" not in test_source:
        errors.append("Microkit AArch64 IOMMU rejection test disappeared")
    cspace_source = source_text.get("Microkit CSpace cap mapping", "")
    required_cap_maps = ('"cap_tcb"', '"cap_sc"', '"cap_vspace"')
    if not all(tag in cspace_source for tag in required_cap_maps) or "cap_smmu" in cspace_source:
        errors.append("Microkit CSpace mappings no longer reflect the audited TCB/SC/VSpace-only cap set")
    cspace_record = microkit.get("cspace_cap_mapping", {})
    if cspace_record.get("supported_mapping_elements") != ["cap_tcb", "cap_sc", "cap_vspace"] or cspace_record.get("smmu_control_cap_mapping") is not False:
        errors.append("Microkit SMMU control-cap delegation record changed")
    capdl_source = source_text.get("Microkit CapDL SMMU support", "")
    if "In future supporting SMMU can be done by matching on the IommuDeviceIdentifier" not in capdl_source:
        errors.append("Microkit CapDL source no longer identifies SMMU support as future work")

    expected_resolution = (
        "A generic seL4 Arm SMMUv2 capability API exists, but it is not enabled or platform-supported by this Sunfish build."
    )
    if not record.get("resolution", "").startswith(expected_resolution):
        errors.append("seL4 SMMU support resolution must preserve the API-versus-platform distinction")
    return errors


def check_ipa_source_files(ipa: dict) -> list[str]:
    """Verify the generic, Sunfish-board, and memory-overlay facts agree."""
    errors: list[str] = []
    kernel_root = ROOT / ".cache/sunfish-lineage-kernel"
    try:
        source_commit = subprocess.check_output(
            ["git", "-C", str(kernel_root), "rev-parse", "HEAD"], text=True
        ).strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        return [f"cannot identify pinned Lineage kernel source: {exc}"]
    expected_commit = ipa["lineage_source"]["commit"]
    if source_commit != expected_commit:
        errors.append(f"Lineage source checkout is {source_commit}, expected {expected_commit}")
        return errors

    source_paths = {
        "hardware": kernel_root / ipa["lineage_source"]["hardware_dts"],
        "sunfish_smmu": kernel_root / ipa["lineage_source"]["sunfish_smmu_dts"],
        "sunfish_memory": kernel_root / ipa["lineage_source"]["sunfish_memory_dts"],
    }
    try:
        source_text = {name: path.read_text() for name, path in source_paths.items()}
    except OSError as exc:
        return [f"cannot read pinned IPA DTS source: {exc}"]

    hardware = _dts_block(source_text["hardware"], "ipa_hw: qcom,ipa@1e00000")
    if hardware is None:
        errors.append("atoll.dtsi has no Sunfish IPA hardware node")
    else:
        for pattern, label in (
            (r"reg\s*=\s*<0x1e00000\s+0x34000>\s*,\s*<0x1e04000\s+0x2c000>", "IPA/GSI register windows"),
            (r"interrupts\s*=\s*<0\s+311\s+0>\s*,\s*<0\s+432\s+0>", "IPA/GSI interrupts"),
            (r"qcom,ipa-hw-ver\s*=\s*<16>", "IPA v4.2 hardware version"),
        ):
            if not re.search(pattern, hardware):
                errors.append(f"atoll.dtsi {label} differ from the IPA inventory")

    for context in ipa["smmu_contexts"]:
        block = _dts_block(source_text["sunfish_smmu"], f"ipa_smmu_{context['name']}: ipa_smmu_{context['name']}")
        if block is None:
            errors.append(f"sdmmagpie.dtsi has no IPA {context['name']} SMMU context")
            continue
        sid = int(context["stream_id"], 0)
        pattern = rf"iommus\s*=\s*<&apps_smmu\s+0x{sid:x}\s+0x0>"
        if not re.search(pattern, block, re.IGNORECASE):
            errors.append(f"sdmmagpie.dtsi IPA {context['name']} SID differs from {context['stream_id']}")
        if context["stage1_bypass"] and not re.search(r"qcom,smmu-s1-bypass\s*;", block):
            errors.append(f"sdmmagpie.dtsi IPA {context['name']} bypass state differs from the inventory")

    for region_key, node, base, size in (
        ("ipa_region", "pil_ipa_fw_mem", "0x98f80000", "0x10000"),
        ("gsi_region", "pil_ipa_gsi_mem", "0x98f90000", "0x5000"),
    ):
        block = _dts_block(source_text["sunfish_memory"], f"&{node}")
        if block is None or not re.search(
            rf"reg\s*=\s*<0\s+{base}\s+0\s+{size}>", block, re.IGNORECASE
        ):
            errors.append(f"Sunfish memory DTS IPA region {node} differs from the inventory")
        spec = ipa["firmware"][region_key]
        cells = [int(value, 0) for value in spec["reg_cells"]]
        if cells != [0, int(base, 0), 0, int(size, 0)]:
            errors.append(f"IPA inventory region {region_key} differs from its Sunfish memory DTS")
    return errors


def check_one(path: Path, manifest: dict) -> list[str]:
    errors: list[str] = []
    dtb = Dtb(path)
    icnss = manifest["icnss"]
    node = icnss["path"]
    try:
        compare_strings(dtb, node, "compatible", [icnss["compatible"]], errors)
        compare_strings(dtb, node, "reg-names", icnss["reg_names"], errors)
        compare_cells(dtb, node, "reg", icnss["reg_cells"], errors)
        compare_cells(dtb, node, "iommus", icnss["iommus_cells"], errors)
        iommu = [int(value, 0) for value in icnss["iommus_cells"]]
        sid, sid_mask = iommu[1], iommu[2]
        matched_sids = [
            candidate for candidate in range(0x8000)
            if ((candidate ^ sid) & (~sid_mask & 0x7fff)) == 0
        ]
        expected_matched = [int(value, 0) for value in icnss["sid_mask_semantics"]["matched_stream_ids"]]
        if matched_sids != expected_matched:
            fail(errors, f"{node}: IOMMU stream mask selects {matched_sids}, expected {expected_matched}")
        compare_cells(dtb, node, "interrupts", icnss["interrupts_cells"], errors)
        raw_irq = [int(value, 0) for value in icnss["interrupts_cells"]]
        decode = icnss["interrupt_irq_decode"]
        if len(raw_irq) % 3 or any(raw_irq[index] != 0 for index in range(0, len(raw_irq), 3)):
            fail(errors, f"{node}: expected only GIC SPI interrupt specifiers under PDC delegation")
        else:
            spi_numbers = raw_irq[1::3]
            expected_spis = [int(value, 0) for value in decode["gic_spi_numbers"]]
            intids = [spi + 32 for spi in spi_numbers]
            expected_intids = [int(value, 0) for value in decode["sel4_gic_irq_intids"]]
            if spi_numbers != expected_spis:
                fail(errors, f"{node}: GIC SPI numbers do not match the resource manifest")
            if intids != expected_intids:
                fail(errors, f"{node}: derived GIC INTIDs do not match the manifest")
            if any(intid > decode["sel4_max_irq"] for intid in intids):
                fail(errors, f"{node}: a decoded GIC INTID exceeds the configured seL4 MAX_IRQ")
            config = (ROOT / "platforms/sunfish/sel4-config.cmake").read_text()
            max_irq_match = re.search(r"\bMAX_IRQ\s+(\d+)", config)
            if not max_irq_match or int(max_irq_match.group(1)) != decode["sel4_max_irq"]:
                fail(errors, "manifest seL4 MAX_IRQ disagrees with platforms/sunfish/sel4-config.cmake")
        if dtb.has(node, "qcom,smmu-s1-bypass") != icnss["s1_bypass_property"]:
            fail(errors, f"{node}: qcom,smmu-s1-bypass property changed")

        power = icnss["power"]
        errors.extend(check_icnss_power_profile(power))
        check_icnss_rpmh_supply_consumers(dtb, power, errors)
        check_icnss_bt_shared_rail_requests(dtb, power, errors)
        check_icnss_rpmh_regulator_profiles(dtb, errors)

        smp2p = icnss["smp2p"]
        compare_strings(dtb, smp2p["path"], "interrupt-names", smp2p["interrupt_names"], errors)
        # These are child interrupt-controller specifiers, not hardware IRQs.
        # Validate the real modem-SMP2P parent doorbell and shared entry so the
        # manifest cannot silently drift into treating child bits as GIC IDs.
        parent = smp2p["parent_controller"]
        compare_cells(dtb, parent, "qcom,smem", [str(value) for value in smp2p["parent_smem_ids"]], errors)
        compare_cells(dtb, parent, "interrupts", smp2p["parent_interrupt_cells"], errors)
        compare_cells(dtb, parent, "qcom,local-pid", [str(smp2p["local_pid"])], errors)
        compare_cells(dtb, parent, "qcom,remote-pid", [str(smp2p["remote_pid"])], errors)
        try:
            parent_irq = dtb.cells(parent, "interrupts")
            if len(parent_irq) == 3 and parent_irq[0] == 0:
                derived_intid = parent_irq[1] + 32
                if derived_intid != int(smp2p["parent_sel4_gic_irq_intid"], 0):
                    fail(errors, f"{parent}: derived GIC INTID does not match the SMP2P manifest")
            else:
                fail(errors, f"{parent}: expected one GIC SPI parent interrupt")
        except subprocess.CalledProcessError:
            fail(errors, f"{parent}: missing/unreadable SMP2P parent interrupt")
        compare_strings(dtb, smp2p["entry_path"], "qcom,entry-name", [smp2p["entry_name"]], errors)
        try:
            extended = dtb.cells(smp2p["path"], "interrupts-extended")
            if len(extended) != 6:
                fail(errors, f"{smp2p['path']}: expected two three-cell interrupts-extended entries")
            else:
                if extended[0] != extended[3]:
                    fail(errors, f"{smp2p['path']}: SMP2P interrupt controller phandles differ")
                tails = [extended[1:3], extended[4:6]]
                expected_tails = [[int(v, 0) for v in pair] for pair in smp2p["interrupt_specifier_tails"]]
                if tails != expected_tails:
                    fail(errors, f"{smp2p['path']}: SMP2P interrupt specifiers changed: {tails}")
        except subprocess.CalledProcessError:
            fail(errors, f"{smp2p['path']}: missing/unreadable interrupts-extended")

        for rail in power["optional_supplies"]:
            if not dtb.has(node, rail):
                fail(errors, f"{node}: expected supply reference {rail} is absent")
        compare_cells(dtb, node, "qcom,vdd-cx-mx-config", power["vdd_cx_mx_config_cells"], errors)
        for prop, present_key in (
            ("qcom,vdd-3.3-ch0-config", "captured_dtb_vdd_3_3_ch0_config_present"),
            ("qcom,vdd-3.3-ch1-config", "captured_dtb_vdd_3_3_ch1_config_present"),
        ):
            if dtb.has(node, prop) != power[present_key]:
                fail(errors, f"{node}: {prop} presence differs from the captured DTB profile")
        rpmh = power["rpmh_controller"]
        compare_strings(dtb, rpmh["path"], "compatible", [rpmh["compatible"]], errors)
        compare_cells(dtb, rpmh["path"], "reg", rpmh["reg_cells"], errors)
        compare_cells(dtb, rpmh["path"], "interrupts", rpmh["interrupt_cells"], errors)
        compare_cells(dtb, rpmh["path"], "qcom,drv-id", [rpmh["driver_id"]], errors)
        compare_cells(dtb, rpmh["path"], "qcom,tcs-config", rpmh["tcs_config_cells"], errors)
        # The optional reference clock is not present in the captured DT. Keep
        # its presence state explicit so a future change is reviewed, not guessed.
        clock_names_present = dtb.has(node, "clock-names")
        if clock_names_present != power["clock_names_present"]:
            fail(errors, f"{node}: clock-names presence changed")
        elif clock_names_present:
            compare_strings(dtb, node, "clock-names", [power["optional_clock"]], errors)

        check_msa(dtb, icnss, errors)
        check_smem(dtb, icnss, errors)
        check_runtime_pil_regions(dtb, manifest, errors)
        dynamic_count = check_dynamic(dtb, manifest, errors)
        ipa_grants = check_ipa_resources(dtb, manifest, errors)
        if errors:
            print(f"FAIL {path}")
            for error in errors:
                print(f"  - {error}")
        else:
            print(
                f"OK {path}: ICNSS resources and live regulator user deltas match; "
                "PIL/WLAN pools are fixed/no-map and non-overlapping; "
                f"{dynamic_count} dynamic Linux reservations have explicit inactive policies; "
                f"IPA v4.2 resource inventory matches with native register/IRQ/PAS/SMMU grants {ipa_grants}"
            )
    except subprocess.CalledProcessError as exc:
        print(f"FAIL {path}: fdtget failed: {exc}")
        errors.append(str(exc))
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--dtb", type=Path, action="append", dest="dtbs")
    args = parser.parse_args()
    try:
        manifest = json.loads(args.manifest.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        parser.error(f"cannot read manifest {args.manifest}: {exc}")
    inventory_errors = check_modem_firmware_inventory(manifest)
    inventory_errors.extend(check_wlan_firmware_inventory(manifest))
    inventory_errors.extend(check_wlan_smmu_actlr_policy(manifest))
    inventory_errors.extend(check_wlan_route_source_comparison(manifest))
    inventory_errors.extend(check_sel4_smmu_support(manifest))
    inventory_errors.extend(check_ipa_source_files(manifest["ipa"]))
    if inventory_errors:
        print("FAIL modem firmware hash inventory")
        for error in inventory_errors:
            print(f"  - {error}")
        return 1
    print("OK modem and WLAN firmware identity inventories are structurally valid (hashes are not authentication)")
    print("OK pinned ICNSS route path, resolved Lineage Sunfish/runtime SID 0x240 binding, generic seL4 SMMU API versus disabled Sunfish support, Microkit AArch64 IOMMU limitation, IPA, ACTLR policy, and firmware inventories match their source records")
    dtbs = args.dtbs or DEFAULT_DTBS
    missing = [str(path) for path in dtbs if not path.is_file()]
    if missing:
        parser.error("missing DTB(s): " + ", ".join(missing))
    errors = []
    for path in dtbs:
        errors.extend(check_one(path, manifest))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
