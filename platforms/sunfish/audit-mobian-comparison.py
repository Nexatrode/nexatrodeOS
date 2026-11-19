#!/usr/bin/env python3
"""Verify the pinned comparison inputs and audit host-side startup dependencies."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
LOCK = Path(__file__).with_name("mobian-comparison.json")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_config(path: Path) -> dict[str, str]:
    result = {}
    for line in path.read_text().splitlines():
        if line.startswith("CONFIG_") and "=" in line:
            key, value = line.split("=", 1)
            result[key] = value
        elif match := re.fullmatch(r"# (CONFIG_\w+) is not set", line):
            result[match[1]] = "n"
    return result


def check_config(path: Path, manifest: dict) -> None:
    actual = read_config(path)
    for key, value in manifest["kernel"]["critical_config"].items():
        require(actual.get(key) == value,
                f"{key}: expected {value}, got {actual.get(key)} in {path}")


def body(text: str, marker: str) -> str:
    """Return a balanced C/DTS block following an exact source marker."""
    require(marker in text, f"missing source marker: {marker}")
    start = text.index("{", text.index(marker))
    depth = 0
    for end in range(start, len(text)):
        depth += (text[end] == "{") - (text[end] == "}")
        if depth == 0:
            return text[start:end + 1]
    raise SystemExit(f"FAIL: unterminated source block: {marker}")


def ordered(text: str, markers: list[str], description: str) -> None:
    positions = [text.find(marker) for marker in markers]
    require(min(positions) >= 0 and positions == sorted(positions), description)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--config-only", action="store_true")
    parser.add_argument("--fetch", action="store_true", help="fetch missing pinned public source inputs")
    args = parser.parse_args()
    manifest = json.loads(LOCK.read_text())
    if args.config_only:
        require(args.config is not None, "--config-only requires --config")
        check_config(args.config, manifest)
        print("PASS: resolved comparison config retains the critical reference options")
        return

    integration = manifest["integration"]
    source = ROOT / integration["cache"]
    kernel = manifest["kernel"]
    archive = ROOT / ".cache/sunfish-mobian-comparison/downloads" / kernel["archive_name"]
    if args.fetch:
        if not source.exists():
            subprocess.run(["git", "clone", "--filter=blob:none", "--no-checkout",
                            integration["repository"], str(source)], check=True)
            subprocess.run(["git", "-C", str(source), "fetch", "--depth=1", "origin",
                            integration["commit"]], check=True)
            subprocess.run(["git", "-C", str(source), "checkout", "--detach",
                            integration["commit"]], check=True)
        if not archive.exists():
            archive.parent.mkdir(parents=True, exist_ok=True)
            print(f"Fetching pinned public kernel source: {kernel['commit']}", flush=True)
            temporary = None
            try:
                with tempfile.NamedTemporaryFile(dir=archive.parent, delete=False) as stream:
                    temporary = Path(stream.name)
                    with urllib.request.urlopen(kernel["archive_url"], timeout=90) as response:
                        while block := response.read(1024 * 1024):
                            stream.write(block)
                require(sha256(temporary) == kernel["archive_sha256"], "downloaded archive hash differs")
                temporary.rename(archive)
            finally:
                if temporary and temporary.exists():
                    temporary.unlink()
    commit = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == integration["commit"], "Mobian integration commit differs")
    for relative, expected in integration["files"].items():
        require(sha256(source / relative) == expected, f"integration hash: {relative}")
    check_config(args.config or source / integration["config"], manifest)

    require(archive.is_file(), f"download the pinned kernel archive first: {archive}")
    require(archive.stat().st_size == kernel["archive_size"], "kernel archive size differs")
    require(sha256(archive) == kernel["archive_sha256"], "kernel archive hash differs")
    files = {}
    with tarfile.open(archive) as stream:
        prefix = stream.getnames()[0].split("/")[0]
        for relative, expected in kernel["files"].items():
            data = stream.extractfile(prefix + "/" + relative).read()
            require(hashlib.sha256(data).hexdigest() == expected, f"kernel hash: {relative}")
            files[relative] = data.decode()

    makefile = files["Makefile"]
    require(re.search(r"^VERSION = 7$", makefile, re.M) is not None
            and re.search(r"^PATCHLEVEL = 1$", makefile, re.M) is not None
            and re.search(r"^SUBLEVEL = 0$", makefile, re.M) is not None,
            "kernel release is not 7.1.0")
    pas = files["drivers/remoteproc/qcom_q6v5_pas.c"]
    require(re.search(r'"qcom,sm7150-mpss-pas",\s*\.data = &mpss_resource_init', pas)
            is not None, "Sunfish MPSS descriptor differs")
    descriptor = body(pas, "struct qcom_pas_data mpss_resource_init =")
    require(".pas_id = 4" in descriptor and ".auto_boot = false" in descriptor,
            "MPSS PAS ID or auto-boot policy differs")
    start = body(pas, "static int qcom_pas_start(")
    ordered(start, ["qcom_q6v5_prepare(", "qcom_mdt_pas_load(",
                    "qcom_scm_pas_prepare_and_auth_reset(pas->pas_ctx)",
                    "qcom_q6v5_wait_for_start("], "PAS host startup order differs")

    wifi = body(files["arch/arm64/boot/dts/qcom/sm7150.dtsi"], "wifi: wifi@18800000")
    require('compatible = "qcom,wcn3990-wifi"' in wifi, "WLAN compatible differs")
    require("iommus = <&apps_smmu 0x240 0x1>" in wifi, "WLAN SMMU specifier differs")
    require("smmu-s1-bypass" not in wifi, "Unexpected stock S1-bypass property")
    for irq in range(414, 426):
        require(f"GIC_SPI {irq} IRQ_TYPE_LEVEL_HIGH" in wifi, f"CE SPI {irq} differs")
    board = files["arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts"]
    require('qcom,ath10k-calibration-variant = "google_sunfish"' in body(board, "&wifi {"),
            "Sunfish calibration selector differs")
    snoc = files["drivers/net/wireless/ath/ath10k/snoc.c"]
    irq_request = body(snoc, "static int ath10k_snoc_request_irq(")
    require("IRQF_NO_AUTOEN" in irq_request and "IRQF_TRIGGER_" not in irq_request,
            "SNOC CE request now overrides the DT interrupt trigger")
    qmi = files["drivers/net/wireless/ath/ath10k/qmi.c"]
    mapping = body(qmi, "static int ath10k_qmi_map_msa_permission(")
    unmapping = body(qmi, "static int ath10k_qmi_unmap_msa_permission(")
    for principal in ["HLOS", "MSS_MSA", "WLAN", "WLAN_CE"]:
        require(f"QCOM_SCM_VMID_{principal}" in mapping, f"MSA mapping lacks {principal}")
    require("dst_perms.perm = QCOM_SCM_PERM_RW;" in unmapping,
            "MSA teardown permission changed; do not borrow downstream RWX")

    patch = (source / "work/patch.sh").read_text()
    require("softdep qcom_q6v5_pas pre: ipa qcom_pd_mapper" in patch,
            "reference no longer requests IPA and PD mapper ahead of MPSS")
    require("ExecStart=/usr/bin/rmtfs -P -s" in patch,
            "reference RMTFS argument change must be re-audited")
    bringup = (source / "work/wcn/sbin/wcn-wifi-bringup.sh").read_text()
    ordered(bringup, ['echo start > "$RP/state"', 'grep -qi "ATH10k WLAN firmware"',
                      "modprobe ath10k_snoc", "/sys/class/net/wl*"],
            "MPSS, WLFW, ath10k and netdev order differs")

    report = {
        "schema_version": 1,
        "integration_commit": commit,
        "kernel_commit": kernel["commit"],
        "manifest_sha256": sha256(LOCK),
        "reference_config_sha256": sha256(source / integration["config"]),
        "verified": [
            "Pinned integration/configuration and kernel archive/file hashes",
            "Linux version 7.1.0 despite the v7.2 branch label",
            "SM7150 MPSS uses PAS ID 4 and auto_boot=false",
            "PAS host prepare/load/authenticate/start-indication sequence",
            "IPA and in-kernel PD mapper soft-dependencies ahead of PAS driver",
            "WLFW discovery precedes ath10k_snoc probe",
            "Sunfish calibration selector and SID/mask 0x240/1",
            "CE SPIs 414..425 use DT level-high without a driver trigger override",
            "Mainline MSA teardown returns HLOS RW, distinct from downstream RWX",
            "Reference recipe removes -r and permits persistent modem EFS writes"
        ],
        "not_established": [
            "A power-off-to-network reproduction on this Pixel",
            "Full service readiness merely from systemd After/Wants or modprobe softdep",
            "Firmware provenance/terms for the selected local payload",
            "A choice to permit persistent EFS writes in the comparison",
            "Temporary boot packaging and boot-environment equivalence",
            "Native seL4 secure-call, SMMU, DMA or IRQ operation"
        ]
    }
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS: {len(report['verified'])} comparison-input and startup checks")
    print("INPUTS: Mobian 789679f / kernel 2267e24 / original kernel configuration")
    print("DECISION NEEDED BEFORE DEVICE START: RMTFS persistent vs RAM-shadow storage")
    print("LIMIT: source audit; no firmware payload or hardware operation")


if __name__ == "__main__":
    main()
