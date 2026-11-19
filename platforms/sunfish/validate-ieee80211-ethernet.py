#!/usr/bin/env python3
"""Audit the station-mode Ethernet-to-802.11 codec against pinned Linux."""

from hashlib import sha256
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[2]
KERNEL = ROOT / ".cache/sunfish-lineage-kernel"
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
SOURCE_HASHES = {
    "net/wireless/util.c": "8aced8cd2f328c11b5a047ab6343a5da59ca358d52d6c21e1ef7e8c3e8c52945",
    "net/mac80211/tx.c": "1f42af53a6b61c093741b1d38819dd8b3b0cf8a08ce8c2a2dbcbe8e303ec8708",
}
CODEC = ROOT / "rust/src/ieee80211_ethernet.rs"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def main() -> None:
    commit = subprocess.check_output(
        ["git", "-C", str(KERNEL), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == PINNED_COMMIT, f"unexpected Lineage kernel source commit {commit}")

    texts = {}
    for relative, expected in SOURCE_HASHES.items():
        contents = (KERNEL / relative).read_bytes()
        require(sha256(contents).hexdigest() == expected, f"pinned source changed: {relative}")
        texts[relative] = contents.decode()

    util = texts["net/wireless/util.c"]
    converter_start = util.index("int ieee80211_data_from_8023(")
    converter_end = util.index("EXPORT_SYMBOL(ieee80211_data_from_8023);", converter_start)
    converter = util[converter_start:converter_end]
    require("case NL80211_IFTYPE_STATION:" in converter, "station conversion case missing")
    require("fc |= cpu_to_le16(IEEE80211_FCTL_TODS);" in converter, "To-DS flag missing")
    require(
        "memcpy(hdr.addr1, bssid, ETH_ALEN);" in converter
        and "memcpy(hdr.addr2, skb->data + ETH_ALEN, ETH_ALEN);" in converter
        and "memcpy(hdr.addr3, skb->data, ETH_ALEN);" in converter,
        "station BSSID/source/destination address order changed",
    )
    require("{ 0xaa, 0xaa, 0x03, 0x00, 0x00, 0x00 }" in util, "RFC1042 SNAP prefix changed")
    require("{ 0xaa, 0xaa, 0x03, 0x00, 0x00, 0xf8 }" in util, "bridge-tunnel SNAP prefix changed")
    require(
        "if (ethertype == ETH_P_AARP || ethertype == ETH_P_IPX)" in converter
        and "encaps_data = bridge_tunnel_header;" in converter
        and "encaps_data = rfc1042_header;" in converter,
        "EtherType-to-SNAP selection changed",
    )

    codec = CODEC.read_text()
    for fragment in (
        "const FC_DATA_TO_DS: u16 = 0x0108;",
        "if ethernet[6..12] != station",
        "output[4..10].copy_from_slice(&bssid);",
        "output[10..16].copy_from_slice(&station);",
        "output[16..22].copy_from_slice(&ethernet[..6]);",
        "ETHERTYPE_AARP | ETHERTYPE_IPX => [0x00, 0x00, 0xf8]",
        "_ => [0x00, 0x00, 0x00]",
    ):
        require(fragment in codec, f"Rust codec no longer matches pinned station conversion: {fragment}")

    print(f"PASS: station-mode Ethernet conversion matches pinned Lineage source {commit}")
    print("PASS: To-DS addresses and RFC1042/bridge-tunnel LLC/SNAP selection match")
    print("LIMIT: source audit does not provide QoS, CCMP, CE publication, DMA, or radio access")


if __name__ == "__main__":
    main()
