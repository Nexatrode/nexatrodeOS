#!/usr/bin/env python3
"""Audit the WMI 10.4 CCMP key encoder against pinned Sunfish ath10k source."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-linux-mainline-ref"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def section(source: str, start: str, end: str) -> str:
    begin = source.index(start)
    finish = source.index(end, begin)
    return source[begin:finish]


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
    ath = source / "drivers/net/wireless/ath/ath10k"
    relative_paths = (
        "drivers/net/wireless/ath/ath10k/wmi.c",
        "drivers/net/wireless/ath/ath10k/wmi.h",
        "drivers/net/wireless/ath/ath10k/mac.c",
    )
    for relative in relative_paths:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"pinned source has local edits: {relative}")

    wmi_c = (ath / "wmi.c").read_text()
    wmi_h = (ath / "wmi.h").read_text()
    mac_c = (ath / "mac.c").read_text()
    rust = (ROOT / "rust/src/ath10k_key.rs").read_text()

    command_ids = section(wmi_h, "enum wmi_10_4_cmd_id {", "};")
    install_id = command_ids.index("WMI_10_4_VDEV_INSTALL_KEY_CMDID,")
    set_param_id = command_ids.index("WMI_10_4_VDEV_SET_PARAM_CMDID,")
    require(set_param_id < install_id,
            "10.4 install-key command is no longer after vdev set-param")
    command_map = section(wmi_c, "static struct wmi_cmd_map wmi_10_4_cmd_map = {", "};")
    require(".vdev_install_key_cmdid = WMI_10_4_VDEV_INSTALL_KEY_CMDID" in command_map,
            "10.4 command map no longer selects the install-key command")

    key_struct = section(wmi_h, "struct wmi_vdev_install_key_cmd {", "};")
    fields = (
        ("__le32 vdev_id;", 4),
        ("struct wmi_mac_addr peer_macaddr;", 8),
        ("__le32 key_idx;", 4),
        ("__le32 key_flags;", 4),
        ("__le32 key_cipher;", 4),
        ("struct wmi_key_seq_counter key_rsc_counter;", 8),
        ("struct wmi_key_seq_counter key_global_rsc_counter;", 8),
        ("struct wmi_key_seq_counter key_tsc_counter;", 8),
        ("u8 wpi_key_rsc_counter[16];", 16),
        ("u8 wpi_key_tsc_counter[16];", 16),
        ("__le32 key_len;", 4),
        ("__le32 key_txmic_len;", 4),
        ("__le32 key_rxmic_len;", 4),
    )
    cursor = 0
    fixed_bytes = 0
    for marker, size in fields:
        index = key_struct.find(marker, cursor)
        require(index >= 0, f"install-key C fields changed order or are missing: {marker}")
        cursor = index + len(marker)
        fixed_bytes += size
    require(fixed_bytes == 92, f"expected 92-byte fixed command, found {fixed_bytes}")

    generator = section(wmi_c, "ath10k_wmi_op_gen_vdev_install_key(",
                        "ath10k_wmi_op_gen_vdev_spectral_conf(")
    for marker in (
        "ath10k_wmi_alloc_skb(ar, sizeof(*cmd) + arg->key_len)",
        "cmd->key_idx       = __cpu_to_le32(arg->key_idx);",
        "cmd->key_flags     = __cpu_to_le32(arg->key_flags);",
        "cmd->key_cipher    = __cpu_to_le32(arg->key_cipher);",
        "cmd->key_len       = __cpu_to_le32(arg->key_len);",
        "ether_addr_copy(cmd->peer_macaddr.addr, arg->macaddr);",
        "memcpy(cmd->key_data, arg->key_data, arg->key_len);",
    ):
        require(marker in generator, f"pinned install-key generator changed: {marker}")
    allocator = section(wmi_c, "struct sk_buff *ath10k_wmi_alloc_skb(",
                        "static void ath10k_wmi_htc_tx_complete(")
    require("memset(skb->data, 0, round_len);" in allocator,
            "WMI skb no longer zeroes sequence counters and alignment fields")
    key_arg = section(wmi_h, "struct wmi_vdev_install_key_arg {", "};")
    require("key_rsc_counter" not in key_arg and "key_tsc_counter" not in key_arg,
            "pinned host argument now carries sequence counters; revisit RSC policy")

    mac_key = section(mac_c, "static int ath10k_send_key(", "static int ath10k_install_key(")
    require("WLAN_CIPHER_SUITE_CCMP" in mac_key and
            "WMI_CIPHER_AES_CCM" in mac_key,
            "mac80211 CCMP mapping to WMI AES-CCM changed")
    key_state = mac_c[mac_c.index("static int ath10k_set_key("):]
    require("flags |= WMI_KEY_PAIRWISE;" in key_state and
            "flags |= WMI_KEY_GROUP;" in key_state,
            "pairwise/group key flags changed")

    require("WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID: u32 = 0x901d" in rust,
            "Rust WMI 10.4 install-key ID differs from the pinned enum")
    require("VDEV_INSTALL_KEY_FIXED_BYTES: usize = 92" in rust,
            "Rust fixed command size differs from the pinned packed C struct")
    for marker in (
        "body.fill(0);",
        "KEY_FLAGS_PAIRWISE: u32 = 0",
        "KEY_FLAGS_GROUP: u32 = 1",
        "CIPHER_AES_CCM: u32 = 4",
        "body[80..84]",
        "body[VDEV_INSTALL_KEY_FIXED_BYTES..body_bytes]",
    ):
        require(marker in rust, f"Rust serializer no longer matches source field policy: {marker}")

    print(f"PASS: pinned Sunfish ath10k source {commit}")
    print("PASS: WMI 10.4 install-key command map, 92-byte ABI, CCMP cipher and key flags")
    print("PASS: counters and padding stay zero as in Linux; the WMI argument omits RSC")
    print("LIMIT: key encoder and queue are host-only; no firmware key install is confirmed")


if __name__ == "__main__":
    main()
