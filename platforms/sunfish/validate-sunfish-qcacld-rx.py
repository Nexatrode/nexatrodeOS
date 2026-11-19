#!/usr/bin/env python3
"""Check the bounded Sunfish QCACLD RX decoder against pinned Lineage source."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"
PATHS = {
    "htt_internal": "drivers/staging/qcacld-3.0/core/dp/htt/htt_internal.h",
    "rx_desc": "drivers/staging/qcacld-3.0/core/dp/htt/rx_desc.h",
    "rx_handler": "drivers/staging/qcacld-3.0/core/dp/htt/htt_rx.c",
    "rx_t2h": "drivers/staging/qcacld-3.0/core/dp/htt/htt_t2h.c",
    "rx_monitor": "drivers/staging/qcacld-3.0/core/dp/htt/htt_monitor_rx.c",
    "rx_tid": "drivers/staging/qcacld-3.0/core/dp/txrx/ol_txrx_types.h",
    "fw_desc": "drivers/staging/fw-api/fw/wal_rx_desc.h",
    "fw_htt": "drivers/staging/fw-api/fw/htt.h",
    "fw_htt_common": "drivers/staging/fw-api/fw/htt_common.h",
    "adrastea": "drivers/staging/qca-wifi-host-cmn/hif/src/adrastea_reg_def.h",
}


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *args], text=True)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def section(text: str, start: str, end: str) -> str:
    begin = text.index(start)
    finish = text.index(end, begin)
    return text[begin:finish]


def compile_layout(source: Path) -> None:
    compiler = shutil.which("cc")
    require(compiler is not None, "a C compiler is required for descriptor-layout assertions")
    source_text = r'''#include <stddef.h>
#include <stdint.h>
#define ATH_PERF_PWR_OFFLOAD 1
#define HELIUMPLUS 1
#define QCA_WIFI_3_0_ADRASTEA 1
#include "fw-api/fw/wal_rx_desc.h"
#include "qcacld-3.0/core/dp/htt/rx_desc.h"
#define RX_HTT_HDR_STATUS_LEN 256
struct htt_host_fw_desc_base {
    union { struct fw_rx_desc_base val; uint32_t dummy_pad; } u;
};
struct htt_host_rx_desc_base {
    struct htt_host_fw_desc_base fw_desc;
    struct rx_attention attention;
    struct rx_frag_info frag_info;
    struct rx_mpdu_start mpdu_start;
    struct rx_msdu_start msdu_start;
    struct rx_msdu_end msdu_end;
    struct rx_mpdu_end mpdu_end;
    struct rx_ppdu_start ppdu_start;
    struct rx_ppdu_end ppdu_end;
    char rx_hdr_status[RX_HTT_HDR_STATUS_LEN];
};
_Static_assert(offsetof(struct htt_host_rx_desc_base, attention) == 4, "attention");
_Static_assert(offsetof(struct htt_host_rx_desc_base, frag_info) == 8, "frag_info");
_Static_assert(offsetof(struct htt_host_rx_desc_base, msdu_start) == 28, "msdu_start");
_Static_assert(offsetof(struct htt_host_rx_desc_base, msdu_end) == 48, "msdu_end");
_Static_assert(offsetof(struct htt_host_rx_desc_base, msdu_end) + 8 * 4 == 80,
               "l3 header padding word");
_Static_assert(sizeof(struct htt_host_rx_desc_base) == 552, "descriptor size");
'''
    with tempfile.TemporaryDirectory(prefix="sunfish-qcacld-rx-layout-") as directory:
        temp = Path(directory)
        input_path = temp / "rx_layout.c"
        input_path.write_text(source_text)
        command = [
            compiler,
            "-std=gnu11",
            "-Werror",
            "-c",
            str(input_path),
            "-o",
            str(temp / "rx_layout.o"),
            f"-I{source / 'drivers/staging'}",
            f"-I{source / 'drivers/staging/qcacld-3.0/uapi/linux'}",
            f"-I{source / 'drivers/staging/fw-api/fw'}",
        ]
        subprocess.run(command, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    require(source.is_dir(), f"source tree not found: {source}")
    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")
    for path in PATHS.values():
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(result.returncode == 0, f"audited source file has local edits: {path}")
    source_text = {
        key: git(source, "show", f"HEAD:{path}") for key, path in PATHS.items()
    }

    desc = section(
        source_text["htt_internal"],
        "struct htt_host_rx_desc_base {",
        "\n};",
    )
    ordered_fields = (
        "struct htt_host_fw_desc_base fw_desc;",
        "struct rx_attention attention;",
        "struct rx_frag_info frag_info;",
        "struct rx_mpdu_start mpdu_start;",
        "struct rx_msdu_start msdu_start;",
        "struct rx_msdu_end msdu_end;",
        "struct rx_mpdu_end mpdu_end;",
        "struct rx_ppdu_start ppdu_start;",
        "struct rx_ppdu_end ppdu_end;",
        "char rx_hdr_status[RX_HTT_HDR_STATUS_LEN];",
    )
    positions = [desc.find(field) for field in ordered_fields]
    require(all(position >= 0 for position in positions) and positions == sorted(positions),
            "HTT host RX descriptor field order changed")
    require("#define RX_STD_DESC_SIZE (sizeof(struct htt_host_rx_desc_base))"
            in source_text["htt_internal"], "standard RX descriptor size definition changed")
    require("#define MSDU_SIZE 1560" in source_text["htt_internal"]
            and "MAX_RX_PAYLOAD_SZ (HTT_RX_BUF_SIZE - RX_STD_DESC_SIZE)"
            in source_text["htt_internal"], "RX buffer/payload sizing contract changed")

    attention = section(source_text["rx_desc"], "struct rx_attention {", "\n};")
    frag = section(source_text["rx_desc"], "struct rx_frag_info {", "\n};")
    msdu_start = section(source_text["rx_desc"], "struct rx_msdu_start {", "\n};")
    msdu_end = section(source_text["rx_desc"], "struct rx_msdu_end {", "\n};")
    require("mpdu_length_err:1" in attention and "msdu_done:1" in attention,
            "attention error/completion bitfields changed")
    require("msdu_length_err:1" in attention, "MSDU length error bitfield changed")
    require("ring2_more_count:8" in frag and "[23:16]" in frag,
            "ring-2 chained-buffer count layout changed")
    require("msdu_length:14" in msdu_start and "[13:0]" in msdu_start,
            "MSDU length field layout changed")
    require("decap_format:2" in msdu_start and "[9:8]" in msdu_start,
            "MSDU decapsulation format field layout changed")
    require("l3_header_padding:3" in msdu_end and "[12:10]" in msdu_end,
            "HELIUMPLUS L3-header padding layout changed")

    fw_desc = source_text["fw_desc"]
    for macro, value in (
        ("FW_RX_DESC_DISCARD_M", "0x1"),
        ("FW_RX_DESC_FORWARD_M", "0x2"),
        ("FW_RX_DESC_ANY_ERR_M", "0x4"),
        ("FW_RX_DESC_DUP_ERR_M", "0x8"),
        ("FW_RX_DESC_EXT_M", "0xc0"),
    ):
        require(f"#define {macro}" in fw_desc and value in fw_desc.split(f"#define {macro}", 1)[1][:120],
                f"firmware RX descriptor flag {macro} changed")

    adrastea = source_text["adrastea"]
    for macro, value in (
        ("ADRASTEA_RX_ATTENTION_0_MPDU_LENGTH_ERR_MASK", "0x08000000"),
        ("ADRASTEA_RX_FRAG_INFO_0_RING2_MORE_COUNT_LSB", "16"),
        ("ADRASTEA_RX_FRAG_INFO_0_RING2_MORE_COUNT_MASK", "0x00ff0000"),
        ("ADRASTEA_RX_MSDU_START_0_MSDU_LENGTH_LSB", "0"),
        ("ADRASTEA_RX_MSDU_START_0_MSDU_LENGTH_MASK", "0x00003fff"),
        ("ADRASTEA_RX_MSDU_START_2_DECAP_FORMAT_LSB", "8"),
        ("ADRASTEA_RX_MSDU_START_2_DECAP_FORMAT_MASK", "0x00000300"),
        ("ADRASTEA_RX_MSDU_END_4_FIRST_MSDU_LSB", "14"),
        ("ADRASTEA_RX_MSDU_END_4_LAST_MSDU_LSB", "15"),
    ):
        require(f"#define {macro}" in adrastea
                and value in adrastea.split(f"#define {macro}", 1)[1][:120],
                f"Adrastea RX field {macro} changed")

    handler = source_text["rx_handler"]
    require("HTT_WIFI_IP(pdev, 2, 0)" in handler
            and "rx_desc->msdu_end.l3_header_padding" in handler,
            "runtime L3 header padding predicate changed")
    require("RX_ATTENTION_0_MSDU_DONE_MASK" in handler
            and "RX_FRAG_INFO_0_RING2_MORE_COUNT_MASK" in handler
            and "RX_MSDU_START_0_MSDU_LENGTH_MASK" in handler,
            "RX ring completion/length/chaining handling changed")

    ring_pop = section(
        handler,
        "static inline qdf_nbuf_t htt_rx_netbuf_pop(",
        "/*\n * FIX ME: this function applies only to LL rx descs.",
    )
    require("sw_rd_idx.msdu_payld" in ring_pop
            and "pdev->rx_ring.fill_cnt--;" in ring_pop
            and "idx &= pdev->rx_ring.size_mask;" in ring_pop,
            "low-latency RX pop cursor/count semantics changed")
    amsdu_pop = section(
        handler,
        "htt_rx_amsdu_pop_ll(htt_pdev_handle pdev,",
        "\n#endif\n\n#if defined(CONFIG_HL_SUPPORT)",
    )
    for fragment in (
        "htt_rx_netbuf_pop(pdev)",
        "RX_FRAG_INFO_0_RING2_MORE_COUNT_MASK",
        "qdf_nbuf_pull_head(msdu, HTT_RX_STD_DESC_RESERVATION +",
        "while (msdu_chained--)",
        "msdu_len -= HTT_RX_BUF_SIZE",
        "RX_STD_DESC_SIZE +",
        "qdf_nbuf_trim_tail(next",
        "RX_MSDU_END_4_LAST_MSDU_MASK",
        "if (last_msdu)",
    ):
        require(fragment in amsdu_pop,
                f"QCACLD MSDU-group FIFO semantics changed: {fragment}")
    require("rx_ind_msdu_byte_idx < num_msdu_bytes" in amsdu_pop
            and "rx_desc->fw_desc.u.val" in amsdu_pop
            and "rx_ind_msdu_byte_idx += 1" in amsdu_pop,
            "QCACLD per-MSDU firmware descriptor override from RX_IND changed")
    ring_refill = section(
        handler,
        "static void htt_rx_fill_ring_count(",
        "#define RX_RING_REFILL_DEBT_MAX",
    )
    require("fill_level - pdev->rx_ring.fill_cnt" in ring_refill
            and "htt_rx_ring_fill_n(pdev, num_to_fill" in ring_refill,
            "low-latency receive refill count changed")
    replenish = section(
        handler,
        "void htt_rx_msdu_buff_replenish(",
        "#define RX_RING_REFILL_DEBT_MAX",
    )
    require("htt_rx_fill_ring_count(pdev);" in replenish,
            "upper-layer packet release no longer triggers ring refill")

    compile_layout(source)

    rust = (ROOT / "rust/src/sunfish_qcacld_rx.rs").read_text()
    require("more_buffers != 0" in rust and "Error::ChainedMsdu" in rust,
            "native parser must keep chained payloads out of the single-buffer view")
    require("l3_header_padding_active: bool" in rust,
            "target-dependent L3 padding must remain explicit caller input")
    require("SUNFISH_RX_DECAP_FORMAT_8023: u8 = 2" in rust
            and "MSDU_START_DECAP_FORMAT_MASK: u32 = 0x0000_0300" in rust
            and "pub decap_format: u8" in rust,
            "native RX descriptor must retain the captured 802.3 decap format")
    require("pub fn decode_with_firmware_descriptor" in rust
            and "pub fn decode_single_buffer_with_firmware_descriptor" in rust,
            "native RX parser must accept the firmware descriptor copied from RX_IND")
    session = (ROOT / "rust/src/sunfish_qcacld_rx_session.rs").read_text()
    require("buffer_ids.len() > self.available_slots()" in session
            and "self.consumer_index = next_index::<N>(index);" in session,
            "native RX session must preserve bounded one-empty-slot FIFO ownership")
    require("self.validate_lease(lease)?;" in session
            and "slot.generation != lease.generation" in session
            and "self.occupied -= 1;" in session,
            "native RX session must validate generations before releasing DMA buffers")
    require("copy_from_slice(payload)" in session
            and "self.release" not in session,
            "native RX copy must leave explicit release to the caller")
    group_start = session.index("pub fn take_next_mpdu_group(")
    group_end = session.index("pub fn copy_single_buffer(", group_start)
    group = session[group_start:group_end]
    require("1usize + usize::from(descriptor.more_buffers)" in group
            and "if descriptor.last_msdu" in group
            and "pub struct RxMsduChain" in session
            and "first_buffer_index" in group
            and "buffer_count: buffers_for_msdu" in group
            and "if msdus_output.len() < msdu_count" in group
            and "if leases_output.len() < buffer_count" in group
            and group.index("if leases_output.len() < buffer_count")
                < group.index("if msdus_output.len() < msdu_count")
            and group.index("if msdus_output.len() < msdu_count")
                < group.index("slot.state = SlotState::Leased"),
            "native MPDU drain must expose per-MSDU chain spans and preflight capacities")
    require("copy_ethernet_single_buffer" in session
            and "WrongDecapFormat" in session
            and "descriptor.firmware.discard" in session,
            "sDDF Ethernet handoff must reject unexpected decap and firmware-discarded MSDUs")
    require("copy_ethernet_msdu_chain" in session
            and "final_continuation_bytes" in session
            and "checked_add(final_remainder)" in session
            and "let descriptor_bytes" in session
            and "planned_output_bytes != payload_bytes" in session,
            "sDDF Ethernet handoff must stitch bounded source-layout ring-2 MSDU chains")
    require("take_next_mpdu_group_with_firmware_descriptors" in session
            and "copy_ethernet_msdu_chain_with_firmware_descriptor" in session,
            "native RX ring grouping/copy must apply firmware status bytes from RX_IND")
    delivery = (ROOT / "rust/src/sunfish_qcacld_rx_delivery.rs").read_text()
    sync_start = session.find("pub fn sync_next_mpdu_group_for_cpu<")
    sync_end = session.find("fn take_next_mpdu_group_inner(", sync_start)
    require(sync_start >= 0 and sync_end > sync_start,
            "native RX session CPU-sync preflight is missing")
    sync_group = session[sync_start:sync_end]
    require("sync_for_cpu(head_slot.buffer_id)" in sync_group
            and "sync_for_cpu(slot.buffer_id)" in sync_group
            and sync_group.index("let buffer =")
                < sync_group.index("sync_for_cpu(head_slot.buffer_id)")
            and sync_group.index("sync_for_cpu(head_slot.buffer_id)")
                < sync_group.index("decode_group_descriptor(")
            and sync_group.index("sync_for_cpu(slot.buffer_id)")
                < sync_group.index("return Ok(buffer_count)"),
            "native RX CPU sync must precede descriptor reads for heads and continuations")
    recycle_start = session.find("pub fn copy_next_posted_buffer_ids(")
    recycle_end = session.find("fn take_next_mpdu_group_inner(", recycle_start)
    require(recycle_start >= 0 and recycle_end > recycle_start,
            "native RX released-buffer ID export is missing")
    recycle = session[recycle_start:recycle_end]
    require(recycle.index("if count > self.posted")
            < recycle.index("for item in output.iter_mut().take(count)"),
            "native RX recycle IDs must be capacity/state checked before output mutation")
    delivery_sync_start = delivery.find("pub fn deliver_indication_with_cpu_sync<")
    delivery_sync_end = delivery.find("pub fn deliver_mpdu<", delivery_sync_start)
    require(delivery_sync_start >= 0 and delivery_sync_end > delivery_sync_start,
            "native RX delivery CPU-sync adapter is missing")
    delivery_sync = delivery[delivery_sync_start:delivery_sync_end]
    require(delivery_sync.index("sync_next_mpdu_group_for_cpu(")
                < delivery_sync.index("let group_report = deliver_mpdu("),
            "native RX delivery must synchronize a full group before claiming/delivering it")
    collect_start = delivery.find("pub fn deliver_indication_with_cpu_sync_and_collect<")
    collect_end = delivery.find("fn deliver_indication_with_cpu_sync_inner<", collect_start)
    require(collect_start >= 0 and collect_end > collect_start,
            "native RX delivery-to-refill ID collection path is missing")
    collect = delivery[collect_start:collect_end]
    require("recycled_buffer_count" in collect
            and "Some(recycled_buffer_ids)" in collect,
            "native RX delivery must return bounded IDs for batch ring refill")
    pipeline = (ROOT / "rust/src/sunfish_qcacld_rx_pipeline.rs").read_text()
    collect_pipeline_start = pipeline.find(
        "pub fn consume_and_deliver_with_cpu_sync_and_collect<"
    )
    collect_pipeline = pipeline[collect_pipeline_start:]
    require(collect_pipeline_start >= 0
            and collect_pipeline.index("if available < required_slots")
                < collect_pipeline.index("consume_and_decode(endpoint, transport_scratch, consume)")
            and "deliver_indication_with_cpu_sync_and_collect(" in collect_pipeline,
            "native CE1 pipeline must preflight recycle scratch before consuming and deliver released IDs")
    rx_pop_start = source_text["rx_handler"].find("htt_rx_amsdu_pop_ll(")
    rx_pop_start = source_text["rx_handler"].rfind("static int", 0, rx_pop_start)
    unmap = source_text["rx_handler"].find("qdf_nbuf_unmap(pdev->osdev, msdu,", rx_pop_start)
    descriptor_read = source_text["rx_handler"].find("rx_desc = htt_rx_desc(msdu);", unmap)
    require(rx_pop_start >= 0 and unmap > rx_pop_start and descriptor_read > unmap,
            "pinned QCACLD must unmap/synchronize RX buffers before reading descriptors")
    chain_copy_start = session.index("pub fn copy_ethernet_msdu_chain(")
    chain_copy_end = session.index("fn decode_leased_single_buffer", chain_copy_start)
    chain_copy = session[chain_copy_start:chain_copy_end]
    require(chain_copy.index("if output.len() < payload_bytes")
            < chain_copy.index("output[..first_payload_bytes]")
            and chain_copy.index("for index in 0..continuation_count")
            < chain_copy.index("output[..first_payload_bytes]"),
            "chained MSDU copy must preflight capacity, leases, and spans before mutation")

    fw_htt = source_text["fw_htt"]
    rx_type_enum = section(
        fw_htt,
        "enum htt_t2h_msg_type {",
        "};",
    )
    require("HTT_T2H_MSG_TYPE_RX_IND" in rx_type_enum
            and "= 0x1" in rx_type_enum.split("HTT_T2H_MSG_TYPE_RX_IND", 1)[1][:80],
            "low-latency RX indication message ID changed")
    rx_prefix = section(
        fw_htt,
        "struct htt_rx_ind_hdr_prefix_t",
        "#define HTT_RX_IND_HDR_PREFIX_BYTES",
    )
    for field in ("msg_type:      8", "ext_tid:       5", "release_valid: 1",
                  "flush_valid:   1", "reserved0:     1", "num_mpdu_ranges:       8"):
        require(field in rx_prefix, f"RX indication prefix field changed: {field}")
    require("peer_id:" in rx_prefix and "16;" in rx_prefix.split("peer_id:", 1)[1][:24],
            "RX indication peer-ID width changed")
    rx_suffix = section(
        fw_htt,
        "struct htt_rx_ind_hdr_suffix_t",
        "#define HTT_RX_IND_HDR_SUFFIX_BYTES",
    )
    require("fw_rx_desc_bytes: 16" in rx_suffix and "reserved0: 16" in rx_suffix,
            "RX indication firmware descriptor span field changed")
    for macro, value in (
        ("HTT_RX_IND_FLUSH_SEQ_NUM_START_M", "0x3f"),
        ("HTT_RX_IND_FLUSH_SEQ_NUM_END_M", "0xfc0"),
        ("HTT_RX_IND_REL_SEQ_NUM_START_M", "0x3f000"),
        ("HTT_RX_IND_REL_SEQ_NUM_END_M", "0xfc0000"),
        ("HTT_RX_IND_NUM_MPDU_RANGES_M", "0xff000000"),
    ):
        define = f"#define {macro}"
        require(define in fw_htt and value in fw_htt.split(define, 1)[1][:100],
                f"RX indication range field changed: {macro}")
    require(source_text["rx_tid"].count("#define OL_TXRX_NUM_EXT_TIDS 19") == 1,
            "valid RX indication extended-TID count changed")
    require("HW_RX_DECAP_FORMAT_RAW = 0,\n\tHW_RX_DECAP_FORMAT_NWIFI,\n"
            "\tHW_RX_DECAP_FORMAT_8023,\n\tHW_RX_DECAP_FORMAT_ETH2," in source_text["rx_monitor"],
            "QCACLD RX decapsulation values changed")

    mpdu_statuses = section(
        source_text["fw_htt_common"],
        "enum htt_rx_ind_mpdu_status {",
        "};",
    )
    require("HTT_RX_IND_MPDU_STATUS_UNKNOWN = 0x0,\n"
            "    HTT_RX_IND_MPDU_STATUS_OK," in mpdu_statuses,
            "RX indication MPDU success status is no longer wire value 1")
    require("HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER, /* only accept EAPOL frames */"
            in mpdu_statuses,
            "QCACLD unauthenticated-peer EAPOL-only status changed")
    rx_ind = (ROOT / "rust/src/sunfish_qcacld_rx_ind.rs").read_text()
    require("HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER: u8 = 6" in rx_ind
            and "EAPOL_ETHERTYPE: u16 = 0x888e" in rx_ind
            and "pub const fn permits_ethernet_frame" in rx_ind
            and "self.0 == HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER" in rx_ind,
            "native RX indication EAPOL-only status policy changed")
    require("status.permits_ethernet_frame(frame)" in delivery
            and "for lease in leases_output.iter_mut().take(group.buffer_count)" in delivery
            and "session.release(lease)" in delivery,
            "RX delivery must enforce MPDU status and release the whole group")
    require("pub fn deliver_indication" in delivery
            and "for mpdu in indication.mpdus()" in delivery
            and "checked_add(group_report.group.msdu_count)" in delivery,
            "RX indication delivery must advance the firmware byte cursor by drained MSDUs")
    pipeline = (ROOT / "rust/src/sunfish_qcacld_rx_pipeline.rs").read_text()
    require("consume_and_decode(endpoint, transport_scratch, consume)" in pipeline
            and "deliver_indication(" in pipeline
            and "pub mod sunfish_qcacld_rx_pipeline;"
            in (ROOT / "rust/src/lib.rs").read_text(),
            "native RX pipeline must compose CE1 decode with indication delivery")

    t2h = source_text["rx_t2h"]
    rx_ind_handler = section(
        t2h,
        "case HTT_T2H_MSG_TYPE_RX_IND:",
        "case HTT_T2H_MSG_TYPE_TX_COMPL_IND:",
    )
    for fragment in (
        "HTT_RX_IND_HDR_BYTES + num_msdu_bytes + 3",
        "HTT_RX_IND_NUM_MPDU_RANGES_GET",
        "calculated_msg_len > msg_len",
        "tid >= OL_TXRX_NUM_EXT_TIDS",
    ):
        require(fragment in rx_ind_handler,
                f"RX indication parser length/TID contract changed: {fragment}")
    rx_ind = (ROOT / "rust/src/sunfish_qcacld_rx_ind.rs").read_text()
    require("SUNFISH_HTT_RX_IND_HEADER_BYTES: usize = 48" in rx_ind
            and ".checked_add(3)" in rx_ind
            and "checked_mul(RX_IND_RANGE_BYTES)" in rx_ind,
            "native RX indication parser must preserve pinned header/alignment/range bounds")
    require("RX_IND_FLUSH_END_SHIFT: u32 = 6" in rx_ind
            and "RX_IND_RELEASE_START_SHIFT: u32 = 12" in rx_ind
            and "RX_IND_RELEASE_END_SHIFT: u32 = 18" in rx_ind,
            "native RX indication parser sequence ranges no longer match the wire layout")
    require("HTT_RX_IND_MPDU_STATUS_OK: u8 = 1" in rx_ind
            and "pub fn mpdus(self) -> MpduIter" in rx_ind
            and "self.0 == HTT_RX_IND_MPDU_STATUS_OK" in rx_ind,
            "native per-MPDU expansion must preserve the pinned success status")
    ring_fill = section(
        source_text["rx_handler"],
        "static int htt_rx_ring_fill_n(",
        "#ifndef CONFIG_HL_SUPPORT",
    )
    attention_clear = ring_fill.find("*(uint32_t *) &rx_desc->attention = 0;")
    dma_map = ring_fill.find("qdf_nbuf_map(pdev->osdev", attention_clear)
    address_write = ring_fill.find("pdev->rx_ring.buf.paddrs_ring[idx] = paddr_marked;", dma_map)
    barrier = ring_fill.find("qdf_mb();", address_write)
    alloc_index_write = ring_fill.find("*(pdev->rx_ring.alloc_idx.vaddr) = idx;", barrier)
    require(
        all(index >= 0 for index in (
            attention_clear, dma_map, address_write, barrier, alloc_index_write
        )) and attention_clear < dma_map < address_write < barrier < alloc_index_write,
        "QCACLD RX refill must clear attention, map, post the address, barrier, then publish alloc index",
    )
    ring_adapter = (ROOT / "rust/src/sunfish_htt_rx_ring.rs").read_text()
    require(
        "prepare_buffer_for_device" in ring_adapter
        and "SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words" in ring_adapter
        and "buffer[attention_start..attention_end].fill(0);" in ring_adapter
        and "pub fn prepare_initial_buffers" in ring_adapter
        and "buffer.len() < self.buffer_bytes() as usize" in ring_adapter,
        "native HTT RX initialization must clear only the bounded attention word",
    )
    refill_start = ring_adapter.find("pub fn refill_batch<")
    refill_end = ring_adapter.find("\nfn buffer_iova(", refill_start)
    require(refill_start >= 0 and refill_end > refill_start,
            "native HTT RX refill transaction is missing")
    refill = ring_adapter[refill_start:refill_end]
    refill_clear = refill.find("plan.prepare_buffer_for_device(")
    refill_sync = refill.find("io.sync_buffer_for_device(buffer_iova")
    refill_entry = refill.find("io.write_ring_entry(ring_entry_iova, buffer_iova)")
    refill_barrier = refill.find("io.dma_write_barrier();")
    refill_publish = refill.find("io.write_alloc_index(memory.alloc_index_iova, next_index)")
    require(
        all(position >= 0 for position in (
            refill_clear, refill_sync, refill_entry, refill_barrier, refill_publish
        )) and refill_clear < refill_sync < refill_entry < refill_barrier < refill_publish,
        "native HTT RX refill must clear, sync, write entry, barrier, then publish index",
    )
    require(
        "self.poisoned = true;" in refill
        and ".preview_post_batch(buffer_ids)" in refill
        and "session.post_batch(buffer_ids)" in refill
        and refill.find("session.post_batch(buffer_ids)") > refill.find("if let Err(error) = io.write_alloc_index"),
        "native HTT RX refill must preflight/commit ownership and poison on ambiguous index publication",
    )
    print(f"PASS: pinned Lineage source {commit} and clean audited files")
    print("PASS: RX descriptor fields, decap mode, Adrastea masks, firmware flags, and target pad gate match source")
    print("PASS: compiled QCACLD C layout matches native descriptor/padding offsets")
    print("PASS: native parser rejects incomplete/oversized spans and never flattens chains")
    print("PASS: QCACLD RX pop/replenish and chained MPDU drain match bounded native leases")
    print("PASS: HTC-framed low-latency RX indication header, TID bounds, range spans, and MPDU status match source")
    print("PASS: RX completion buffers synchronize for CPU before descriptor parsing and packet delivery")
    print("PASS: QCACLD clears RX attention before map/post/barrier/index publish; native buffer prep matches")


if __name__ == "__main__":
    main()
