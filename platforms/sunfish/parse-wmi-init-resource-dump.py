#!/usr/bin/env python3
"""Decode the bounded WMI INIT resource TLV logged by the Sunfish probe patch."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import TextIO


PREFIX = "SUNFISH_WMI_INIT_RSRC"
RESOURCE_TAG = 75
RESOURCE_VALUE_BYTES = 272
RESOURCE_TLV_BYTES = 276
RESOURCE_WORDS = 68
LINE_RE = re.compile(
    rf".*?{re.escape(PREFIX)}\s+([0-9a-fA-F]{{8}}):\s*(.*?)\s*$"
)
RESOURCE_FIELD_NAMES = (
    "num_vdevs",
    "num_peers",
    "num_offload_peers",
    "num_offload_reorder_buffs",
    "num_peer_keys",
    "num_tids",
    "ast_skid_limit",
    "tx_chain_mask",
    "rx_chain_mask",
    "rx_timeout_pri[0]",
    "rx_timeout_pri[1]",
    "rx_timeout_pri[2]",
    "rx_timeout_pri[3]",
    "rx_decap_mode",
    "scan_max_pending_req",
    "bmiss_offload_max_vdev",
    "roam_offload_max_vdev",
    "roam_offload_max_ap_profiles",
    "num_mcast_groups",
    "num_mcast_table_elems",
    "mcast2ucast_mode",
    "tx_dbg_log_size",
    "num_wds_entries",
    "dma_burst_size",
    "mac_aggr_delim",
    "rx_skip_defrag_timeout_dup_detection_check",
    "vow_config",
    "gtk_offload_max_vdev",
    "num_msdu_desc",
    "max_frag_entries",
    "num_tdls_vdevs",
    "num_tdls_conn_table_entries",
    "beacon_tx_offload_max_vdev",
    "num_multicast_filter_entries",
    "num_wow_filters",
    "num_keep_alive_pattern",
    "keep_alive_pattern_size",
    "max_tdls_concurrent_sleep_sta",
    "max_tdls_concurrent_buffer_sta",
    "wmi_send_separate",
    "num_ocb_vdevs",
    "num_ocb_channels",
    "num_ocb_schedules",
    "flag1",
    "smart_ant_cap",
    "BK_Minfree",
    "BE_Minfree",
    "VI_Minfree",
    "VO_Minfree",
    "alloc_frag_desc_for_data_pkt",
    "num_ns_ext_tuples_cfg",
    "bpf_instruction_size",
    "max_bssid_rx_filters",
    "use_pdev_id",
    "max_num_dbs_scan_duty_cycle",
    "max_num_group_keys",
    "peer_map_unmap_v2_support",
    "sched_params",
    "twt_ap_pdev_count",
    "twt_ap_sta_count",
    "max_nlo_ssids",
    "num_packet_filters",
    "num_max_sta_vdevs",
    "max_bssid_indicator",
    "ul_resp_config",
    "msdu_flow_override_config0",
    "msdu_flow_override_config1",
    "flags2",
)
assert len(RESOURCE_FIELD_NAMES) == RESOURCE_WORDS

SELECTED_FIELD_NAMES = (
    "num_vdevs",
    "num_peers",
    "num_offload_peers",
    "num_offload_reorder_buffs",
    "num_peer_keys",
    "num_tids",
    "tx_chain_mask",
    "rx_chain_mask",
    "num_msdu_desc",
    "max_frag_entries",
    "flag1",
    "num_packet_filters",
    "num_max_sta_vdevs",
)


class DumpError(ValueError):
    """The kernel log does not contain one complete, valid resource TLV."""


def parse_dump(log_text: str) -> dict[str, object]:
    captures: list[bytearray] = []
    active: bytearray | None = None

    for line_number, line in enumerate(log_text.splitlines(), start=1):
        match = LINE_RE.match(line)
        if match is None:
            continue

        offset = int(match.group(1), 16)
        byte_tokens = match.group(2).split()
        if not byte_tokens or any(
            len(token) != 2 or not re.fullmatch(r"[0-9a-fA-F]{2}", token)
            for token in byte_tokens
        ):
            raise DumpError(f"line {line_number}: malformed hex-byte list")
        chunk = bytes(int(token, 16) for token in byte_tokens)
        if len(chunk) > 16:
            raise DumpError(f"line {line_number}: dump row exceeds 16 bytes")

        if offset == 0:
            if active is not None:
                captures.append(active)
            active = bytearray()
        elif active is None:
            raise DumpError(f"line {line_number}: dump starts at nonzero offset")

        assert active is not None
        if offset != len(active):
            raise DumpError(
                f"line {line_number}: expected offset {len(active):08x}, "
                f"observed {offset:08x}"
            )
        active.extend(chunk)

    if active is not None:
        captures.append(active)
    if len(captures) != 1:
        raise DumpError(f"expected one WMI INIT dump, found {len(captures)}")

    tlv = bytes(captures[0])
    if len(tlv) != RESOURCE_TLV_BYTES:
        raise DumpError(
            f"expected {RESOURCE_TLV_BYTES} bytes, observed {len(tlv)}"
        )
    # WMITLV_SET_HDR serializes (tag << 16) | value_len on little-endian
    # Sunfish, so the length occupies the first two bytes.
    value_bytes = int.from_bytes(tlv[0:2], "little")
    tag = int.from_bytes(tlv[2:4], "little")
    if tag != RESOURCE_TAG:
        raise DumpError(f"expected resource-config tag {RESOURCE_TAG}, got {tag}")
    if value_bytes != RESOURCE_VALUE_BYTES:
        raise DumpError(
            f"expected TLV value length {RESOURCE_VALUE_BYTES}, got {value_bytes}"
        )

    words = [
        int.from_bytes(tlv[4 + index * 4 : 8 + index * 4], "little")
        for index in range(RESOURCE_WORDS)
    ]
    resource_fields = {
        name: f"0x{word:08x}"
        for name, word in zip(RESOURCE_FIELD_NAMES, words, strict=True)
    }
    return {
        "tlv_tag": tag,
        "tlv_value_bytes": value_bytes,
        "tlv_bytes_hex": tlv.hex(),
        "words_le": [f"0x{word:08x}" for word in words],
        "resource_fields": resource_fields,
        "selected_fields": {
            name: resource_fields[name] for name in SELECTED_FIELD_NAMES
        },
    }


def read_input(path: str) -> str:
    if path == "-":
        return sys.stdin.read()
    return Path(path).read_text(encoding="utf-8")


def main(argv: list[str] | None = None, *, stdin: TextIO | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", help="kernel log file, or - to read stdin")
    args = parser.parse_args(argv)
    try:
        if args.log == "-" and stdin is not None:
            log_text = stdin.read()
        else:
            log_text = read_input(args.log)
        result = parse_dump(log_text)
    except (OSError, UnicodeError, DumpError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
