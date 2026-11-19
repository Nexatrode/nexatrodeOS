#!/usr/bin/env python3
"""Audit the pinned Mobian Sunfish Wi-Fi launch path without loading firmware."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "789679f25f859bd741d690fe221d1cc68a23444e"
DEFAULT_SOURCE = ROOT / ".cache/mobian-pixel4a-port-789679"

AUDITED_PATHS = (
    "README.md",
    "kernel-config-7.1.0-rc3-sm7150.txt",
    "work/patch.sh",
    "work/wcn/etc/modprobe.d/ath10k-snoc-blacklist.conf",
    "work/wcn/etc/tmpfiles.d/qcom-fw-path.conf",
    "work/wcn/sbin/wcn-wifi-bringup.sh",
    "work/wcn/units/pd-mapper.service",
    "work/wcn/units/tqftpserv.service",
    "work/wcn/units/wcn-wifi.service",
    "work/ipa/sbin/ipa-late-load.sh",
    "work/ipa/units/ipa-late-load.service",
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    if not source.is_dir():
        raise SystemExit(f"FAIL: source tree not found: {source}")

    commit = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")

    for relative in AUDITED_PATHS:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"audited source file has local edits: {relative}")

    def read(relative: str) -> str:
        return (source / relative).read_text()

    README = read("README.md")
    config = read("kernel-config-7.1.0-rc3-sm7150.txt")
    patch = read("work/patch.sh")
    blacklist = read("work/wcn/etc/modprobe.d/ath10k-snoc-blacklist.conf")
    firmware_path = read("work/wcn/etc/tmpfiles.d/qcom-fw-path.conf")
    bringup = read("work/wcn/sbin/wcn-wifi-bringup.sh")
    pd_mapper = read("work/wcn/units/pd-mapper.service")
    tftp = read("work/wcn/units/tqftpserv.service")
    wifi = read("work/wcn/units/wcn-wifi.service")
    ipa_late_load = read("work/ipa/sbin/ipa-late-load.sh")
    ipa_late_unit = read("work/ipa/units/ipa-late-load.service")

    require(
        "CONFIG_ATH10K_SNOC=m" in config,
        "pinned kernel config does not build ath10k_snoc as a module",
    )
    require(
        "softdep qcom_q6v5_pas pre: ipa qcom_pd_mapper" in patch,
        "IPA and the kernel PD mapper are not ordered before MPSS remoteproc",
    )
    require(
        blacklist.strip() == "blacklist ath10k_snoc",
        "ath10k_snoc is no longer held back from autoload",
    )
    require(
        "After=rmtfs.service pd-mapper.service tqftpserv.service qrtr-ns.service" in wifi,
        "Wi-Fi unit no longer orders the launch prerequisites",
    )
    require(
        "Wants=rmtfs.service pd-mapper.service tqftpserv.service" in wifi,
        "Wi-Fi unit no longer requests its launch prerequisites",
    )
    require(
        "ExecStart=/usr/local/bin/pd-mapper" in pd_mapper,
        "PD mapper unit does not start the expected service",
    )
    sunfish_fw_path = "/lib/firmware/qcom/sm7150/google/sunfish"
    require(
        sunfish_fw_path in pd_mapper and sunfish_fw_path in firmware_path,
        "firmware-class path is not set to the Sunfish directory",
    )
    require(
        "ExecStart=/usr/local/bin/tqftpserv" in tftp
        and "After=qrtr-ns.service" in tftp,
        "TFTP service is not ordered after QRTR namespace service",
    )
    require(
        "Before=rmtfs.service" in pd_mapper and "Before=rmtfs.service" in tftp,
        "PD mapper and TFTP server are no longer ordered before remote storage",
    )
    require(
        'grep -q "Wireless Data Service"' in ipa_late_load
        and "sleep 8" in ipa_late_load
        and "modprobe -i ipa" in ipa_late_load,
        "IPA late-load path no longer waits for WDS before loading IPA",
    )
    require(
        "After=multi-user.target qrtr-ns.service rmtfs.service ModemManager.service"
        in ipa_late_unit,
        "IPA late-load unit ordering changed",
    )

    modem_start = bringup.find('echo start > "$RP/state"')
    wlfw_lookup = bringup.find('grep -qi "ATH10k WLAN firmware"')
    driver_load = bringup.find("modprobe ath10k_snoc")
    netdev_check = bringup.find("/sys/class/net/wl*")
    phy_check = bringup.find("/sys/class/ieee80211")
    require(
        min(modem_start, wlfw_lookup, driver_load, netdev_check, phy_check) >= 0,
        "bring-up script is missing a required observable step",
    )
    require(
        modem_start < wlfw_lookup < driver_load < netdev_check
        and driver_load < phy_check,
        "driver load no longer follows modem start and WLFW discovery, or lacks readiness checks",
    )
    require("reported working WiFi" in README or "WiFi (2.4/5 GHz)" in README,
            "README no longer records the port's reported Wi-Fi result")
    require("variant=google_sunfish" in README,
            "README no longer identifies the required Sunfish board-data variant")

    print(f"PASS: pinned Mobian source {commit}")
    print("PASS: pinned recipe requests IPA and kernel PD mapper before MPSS remoteproc")
    print("PASS: PD mapper, TFTP and remote-storage services precede the Wi-Fi unit")
    print("PASS: modem start and WLFW discovery precede the single ath10k_snoc load")
    print("PASS: reported readiness requires both an IEEE 802.11 PHY and a wl* netdev")
    print("LIMIT: the same pinned recipe retains an IPA late-load unit that waits for modem WDS")
    print("LIMIT: patch comments treat late loading as a no-op after the soft-dependency; this audit cannot verify runtime ordering")
    print("LIMIT: this audits source ordering only; it does not reproduce the reported Linux boot")
    print("LIMIT: no firmware bytes were read, staged, served, decoded, or loaded")
    print("LIMIT: the cited firmware rehosts have no declared license and are not authorized by this check")


if __name__ == "__main__":
    main()
