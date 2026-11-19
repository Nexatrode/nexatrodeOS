#!/usr/bin/env python3
"""Verify IPA/GSI probe offsets and firmware-start order against pinned source."""

from __future__ import annotations

import json
import importlib.util
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
SOURCE_FILES = (
    "drivers/platform/msm/ipa/ipa_v3/dump/sm8150/ipa_hwio.h",
    "drivers/platform/msm/gsi/gsi_reg_v2.h",
    "drivers/platform/msm/ipa/ipa_v3/ipa.c",
    "drivers/platform/msm/ipa/ipa_v3/ipa_utils.c",
    "drivers/platform/msm/ipa/ipa_v3/ipahal/ipahal_reg.c",
    "include/uapi/linux/msm_ipa.h",
    "drivers/soc/qcom/subsys-pil-tz.c",
    "drivers/soc/qcom/subsystem_restart.c",
    "drivers/soc/qcom/peripheral-loader.c",
    "arch/arm64/boot/dts/qcom/atoll.dtsi",
    "drivers/platform/msm/ipa/ipa_v3/ipa_i.h",
    "drivers/platform/msm/ipa/ipa_v3/ipahal/ipahal_reg_i.h",
    "drivers/soc/qcom/mdt_loader.c",
    "include/dt-bindings/msm/msm-bus-ids.h",
    "arch/arm64/boot/dts/qcom/sm6150-bus.dtsi",
    "drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c",
    "drivers/platform/msm/ipa/ipa_v3/rmnet_ipa.c",
    "include/uapi/linux/ipa_qmi_service_v01.h",
    "drivers/platform/msm/ipa/ipa_v3/ipa_uc.c",
)
BUS_SOURCE_FILES = (
    "drivers/soc/qcom/msm_bus/msm_bus_of.c",
    "drivers/soc/qcom/msm_bus/msm_bus_arb_rpmh.c",
    "drivers/soc/qcom/msm_bus/msm_bus_fabric_rpmh.c",
    "drivers/soc/qcom/msm_bus/msm_bus_core.h",
)
IPA_VERSION_CHAIN = ("v3_0", "v3_1", "v3_5", "v3_5_1", "v4_0", "v4_1", "v4_2")
IPA_OFFSETS = {
    "VERSION": 0x034,
    "COMP_CFG": 0x03C,
    "CLKON_CFG": 0x044,
    "SHARED_MEM_SIZE": 0x054,
    "QSB_MAX_WRITES": 0x074,
    "QSB_MAX_READS": 0x078,
    "FILT_ROUT_HASH_EN": 0x148,
    "BCR": 0x1D0,
    "COUNTER_CFG": 0x1F0,
    "TX_CFG": 0x1FC,
}


def fail(message: str) -> None:
    raise SystemExit(f"IPA source validation failed: {message}")


def main() -> None:
    source_root = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / ".cache/sunfish-lineage-kernel"
    revision = subprocess.check_output(
        ["git", "-C", str(source_root), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision != PINNED_COMMIT:
        fail(f"kernel revision is {revision}, expected {PINNED_COMMIT}")

    diff = subprocess.run(
        ["git", "-C", str(source_root), "diff", "--quiet", PINNED_COMMIT, "--",
         *SOURCE_FILES, *BUS_SOURCE_FILES],
        check=False,
    )
    if diff.returncode != 0:
        fail("one or more register, bus, or source-map files differ from the pinned commit")

    ipa_header = (source_root / SOURCE_FILES[0]).read_text()
    for register, expected in IPA_OFFSETS.items():
        pattern = rf"^#define HWIO_IPA_{register}_ADDR \(IPA_CFG_REG_BASE \+ (0x[0-9a-fA-F]+)\)$"
        match = re.search(pattern, ipa_header, re.MULTILINE)
        if match is None or int(match.group(1), 16) != expected:
            fail(f"IPA {register} address does not match 0x{expected:03x}")

    ipa_fields = (source_root / SOURCE_FILES[11]).read_text()
    shared_mem_fields = (
        ("SHARED_MEM_BADDR_BMSK", "0xffff0000"),
        ("SHARED_MEM_BADDR_SHFT", "0x10"),
        ("SHARED_MEM_SIZE_BMSK", "0xffff"),
        ("SHARED_MEM_SIZE_SHFT", "0x0"),
    )
    for field, expected in shared_mem_fields:
        pattern = rf"^#define IPA_SHARED_MEM_SIZE_{field}\s+{expected}$"
        if re.search(pattern, ipa_fields, re.MULTILINE) is None:
            fail(f"IPA shared-memory field {field} does not match {expected}")

    snapshot_decoder = (ROOT / "rust/src/sunfish_ipa_snapshot.rs").read_text()
    decoder_constants = {
        "IPA_SHARED_MEM_SIZE_OFFSET": 0x054,
        "IPA_SHARED_MEM_BADDR_MASK": 0xFFFF0000,
        "IPA_SHARED_MEM_BADDR_SHIFT": 0x10,
        "IPA_SHARED_MEM_SIZE_MASK": 0xFFFF,
        "IPA_SHARED_MEM_SIZE_SHIFT": 0,
    }
    for constant, expected in decoder_constants.items():
        match = re.search(
            rf"^pub const {constant}: u32 = (0x[0-9a-fA-F_]+|\d+);$",
            snapshot_decoder,
            re.MULTILINE,
        )
        if match is None or int(match.group(1).replace("_", ""), 0) != expected:
            fail(f"IPA snapshot decoder {constant} does not match the pinned register definition")
    if (
        "decode_shared_mem_size_register" not in snapshot_decoder
        or "register_value & IPA_SHARED_MEM_BADDR_MASK" not in snapshot_decoder
        or "register_value & IPA_SHARED_MEM_SIZE_MASK" not in snapshot_decoder
    ):
        fail("IPA snapshot decoder no longer extracts the pinned shared-memory fields")

    gsi_header = (source_root / SOURCE_FILES[1]).read_text()
    flattened = re.sub(r"\\\s*\n", " ", gsi_header)
    address_pattern = (
        r"#define GSI_V2_2_EE_n_GSI_HW_PARAM_2_OFFS\(n\)\s+"
        r"\(GSI_GSI_REG_BASE_OFFS \+ 0x0001f040 \+ 0x4000 \* \(n\)\)"
    )
    if re.search(address_pattern, flattened) is None:
        fail("GSI v2.2 HW_PARAM_2 address does not match 0x1f040 + 0x4000*n")
    status_pattern = (
        r"#define GSI_EE_n_GSI_STATUS_OFFS\(n\)\s+"
        r"\(GSI_GSI_REG_BASE_OFFS \+ 0x00012000 \+ 0x4000 \* \(n\)\)"
    )
    if re.search(status_pattern, flattened) is None:
        fail("GSI EE status address does not match 0x12000 + 0x4000*n")
    if re.search(r"^#define GSI_EE_n_GSI_STATUS_ENABLED_BMSK 0x1$", gsi_header, re.MULTILINE) is None:
        fail("GSI EE enabled status mask changed")
    for field, expected in (
        ("GSI_NUM_EV_PER_EE_BMSK", "0x1f00"),
        ("GSI_NUM_EV_PER_EE_SHFT", "0x8"),
    ):
        pattern = rf"^#define GSI_V2_2_EE_n_GSI_HW_PARAM_2_{field} {expected}$"
        if re.search(pattern, gsi_header, re.MULTILINE) is None:
            fail(f"GSI v2.2 event-ring field {field} changed")

    ipa_source = re.sub(r"\s+", " ", (source_root / SOURCE_FILES[2]).read_text())
    if "case IPA_HW_v4_2: gsi_ver = GSI_VER_2_2; break;" not in ipa_source:
        fail("the kernel no longer maps IPA v4.2 to GSI v2.2")

    loader_start = ipa_source.rindex("static void ipa3_load_ipa_fw")
    loader_end = ipa_source.index("static ssize_t ipa3_write", loader_start)
    loader = ipa_source[loader_start:loader_end]
    attach = loader.find("ipa3_attach_to_smmu()")
    pil_start = loader.find("ipa3_pil_load_ipa_fws(IPA_SUBSYSTEM_NAME)")
    post_init = loader.find("ipa3_post_init(&ipa3_res, ipa3_ctx->cdev.dev)")
    if min(attach, pil_start, post_init) < 0 or not attach < pil_start < post_init:
        fail("IPA startup no longer attaches SMMU, PIL-starts ipa_fws, then post-initializes")
    if "ipa3_ctx->ipa_hw_type >= IPA_HW_v3_5" not in loader:
        fail("the IPA v3.5+ selection for the PIL ipa_fws path changed")
    pil_loader_start = ipa_source.index("static int ipa3_pil_load_ipa_fws")
    pil_loader_end = ipa_source.index("static void ipa3_load_ipa_fw", pil_loader_start)
    if "subsystem_get(sub_sys)" not in ipa_source[pil_loader_start:pil_loader_end]:
        fail("ipa_fws no longer starts through the subsystem restart framework")

    ipa_header = (source_root / SOURCE_FILES[10]).read_text()
    if re.search(r"\bIPA_PLAT_TYPE_MDM\s*=\s*0", ipa_header) is None:
        fail("IPA_PLAT_TYPE_MDM no longer has value 0")
    atoll = (source_root / SOURCE_FILES[9]).read_text()
    ipa_node_start = atoll.index("ipa_hw: qcom,ipa@1e00000 {")
    ipa_node_end = atoll.index("\n\t};", ipa_node_start)
    ipa_node = atoll[ipa_node_start:ipa_node_end]
    if "qcom,ipa-hw-ver = <16>" not in ipa_node:
        fail("Atoll IPA node no longer selects hardware type 16")
    if "qcom,platform-type" in ipa_node:
        fail("Atoll IPA node now overrides the driver's default platform type")
    platform_default = re.sub(r"\s+", " ", ipa_source)
    if (
        'of_property_read_u32(pdev->dev.of_node, "qcom,platform-type", '
        '&ipa_drv_res->platform_type);' not in platform_default
        or "using default (IPA_PLAT_TYPE_MDM) for platform-type" not in platform_default
    ):
        fail("the IPA platform-type default changed")

    fws_start = atoll.index("qcom,ipa_fws {")
    fws_end = atoll.index("\n\t};", fws_start)
    fws_node = atoll[fws_start:fws_end]
    if not all(
        item in fws_node
        for item in (
            'compatible = "qcom,pil-tz-generic"',
            "qcom,pas-id = <0xf>",
            'qcom,firmware-name = "ipa_fws"',
            "qcom,pil-force-shutdown",
            "memory-region = <&pil_ipa_fw_mem>",
        )
    ):
        fail("Atoll ipa_fws PAS node contract changed")
    if "qcom,mas-crypto" in fws_node:
        fail("Atoll ipa_fws now overrides the shared PAS crypto-master fallback")

    pil_memory = re.sub(r"\s+", " ", atoll)
    if re.search(
        r"pil_ipa_fw_mem:\s+ipa_fw_region@93b00000\s*\{[^}]*?"
        r"reg\s*=\s*<0\s+0x93b00000\s+0\s+0x10000>",
        pil_memory,
    ) is None:
        fail("Atoll IPA PIL firmware reservation changed")

    active_profile = json.loads((ROOT / "platforms/sunfish/wlan-resources.json").read_text())
    firmware = active_profile["ipa"]["firmware"]
    if (
        firmware.get("pas_id") != "0xf"
        or firmware.get("firmware_name") != "ipa_fws"
        or firmware.get("ipa_region", {}).get("reg_cells")
        != ["0x0", "0x98f80000", "0x0", "0x10000"]
    ):
        fail("active Sunfish IPA-FW profile changed")
    planner = (ROOT / "rust/src/sunfish_ipa_fws.rs").read_text()
    if not all(
        re.search(pattern, planner) is not None
        for pattern in (
            r"pub const PAS_ID: u32 = 0x0f;",
            r"pub const REGION_BASE: u64 = 0x98f8_0000;",
            r"pub const REGION_BYTES: u64 = 0x1_0000;",
        )
    ):
        fail("IPA-FW planner constants no longer match the active Sunfish profile")

    mdt_source = re.sub(r"\s+", " ", (source_root / SOURCE_FILES[12]).read_text())
    mdt_start = mdt_source.index("int qcom_mdt_load(")
    mdt_end = mdt_source.index("EXPORT_SYMBOL_GPL(qcom_mdt_load)", mdt_start)
    mdt_loader = mdt_source[mdt_start:mdt_end]
    required_mdt_flow = (
        "qcom_scm_pas_init_image(pas_id, fw->data, fw->size)",
        "if (relocate)",
        "qcom_scm_pas_mem_setup(pas_id, mem_phys, max_addr - min_addr)",
        "offset + phdr->p_memsz > mem_size",
        'sprintf(fw_name + fw_name_len - 3, "b%02d", i)',
        "seg_fw->size != phdr->p_filesz",
        "memset(ptr + phdr->p_filesz, 0, phdr->p_memsz - phdr->p_filesz)",
    )
    positions = [mdt_loader.find(item) for item in required_mdt_flow]
    if min(positions) < 0 or positions != sorted(positions):
        fail("Qualcomm MDT metadata/setup/segment-load ordering or bounds changed")

    pil_source = (source_root / SOURCE_FILES[6]).read_text()
    for call in ("PAS_INIT_IMAGE_CMD", "PAS_MEM_SETUP_CMD", "PAS_AUTH_AND_RESET_CMD"):
        if call not in pil_source:
            fail(f"generic PIL-TZ secure loader no longer implements {call}")

    def pil_function(name: str, next_name: str) -> str:
        start = pil_source.index(f"static int {name}(")
        end = pil_source.index(f"static int {next_name}(", start + 1)
        return re.sub(r"\s+", " ", pil_source[start:end])

    init_image = pil_function("pil_init_image_trusted", "pil_mem_setup_trusted")
    mem_setup = pil_function("pil_mem_setup_trusted", "pil_auth_and_reset")
    auth_reset = pil_function("pil_auth_and_reset", "pil_shutdown_trusted")
    init_order = [
        init_image.find("scm_pas_enable_bw()"),
        init_image.find("dma_alloc_attrs("),
        init_image.find("PAS_INIT_IMAGE_CMD"),
    ]
    if init_order[-1] >= 0:
        init_order.append(init_image.find("scm_pas_disable_bw()", init_order[-1]))
    if min(init_order) < 0 or init_order != sorted(init_order):
        fail("PAS_INIT_IMAGE metadata allocation and bandwidth-vote order changed")
    if any(
        token in mem_setup
        for token in (
            "scm_pas_enable_bw()",
            "scm_pas_disable_bw()",
            "enable_regulators(",
            "prepare_enable_clocks(",
        )
    ):
        fail("PAS_MEM_SETUP unexpectedly acquired a PIL bandwidth or power envelope")
    auth_order = [
        auth_reset.find("scm_pas_enable_bw()"),
        auth_reset.find("enable_regulators("),
        auth_reset.find("prepare_enable_clocks("),
        auth_reset.find("PAS_AUTH_AND_RESET_CMD"),
        auth_reset.find("scm_pas_disable_bw()"),
    ]
    if min(auth_order) < 0 or auth_order != sorted(auth_order):
        fail("PAS_AUTH_AND_RESET bandwidth/power wrapper order changed")

    # PAS uses the legacy msm_bus client API. Pin its exact default/active
    # request and the dynamic master selection rather than inferring a BCM
    # vote from a generic source/destination pair.
    pil_flat = re.sub(r"\s+", " ", pil_source)
    pas_bw = re.search(
        r"static struct msm_bus_paths scm_pas_bw_tbl\[\] = \{(.*?)\}; "
        r"static struct msm_bus_scale_pdata scm_pas_bus_pdata",
        pil_flat,
    )
    if pas_bw is None:
        fail("shared SCM PAS bandwidth table is missing")
    bw_table = pas_bw.group(1)
    if (
        bw_table.count(".vectors = (struct msm_bus_vectors[])") != 2
        or bw_table.count(".src = MSM_BUS_MASTER_SPS") != 2
        or bw_table.count(".dst = MSM_BUS_SLAVE_EBI_CH0") != 2
        or bw_table.count(".num_paths = 1") != 2
        or ".ib = 492 * 8 * 1000000UL" not in bw_table
        or ".ab = 492 * 8 * 100000UL" not in bw_table
    ):
        fail("SCM PAS default/active bandwidth request changed")
    bus_ids = (source_root / SOURCE_FILES[13]).read_text()
    for name, expected in (
        ("MSM_BUS_MASTER_CRYPTO_CORE_0", 125),
        ("MSM_BUS_SLAVE_EBI_CH0", 512),
    ):
        if re.search(rf"^#define\s+{name}\s+{expected}$", bus_ids, re.MULTILINE) is None:
            fail(f"SCM PAS bus endpoint {name} changed")
    pas_init_start = pil_source.index("static void scm_pas_init(int id)")
    pas_init_end = pil_source.index("static int of_read_clocks", pas_init_start)
    pas_init = re.sub(r"\s+", " ", pil_source[pas_init_start:pas_init_end])
    if not all(
        token in pas_init
        for token in (
            "scm_pas_bw_tbl[0].vectors[0].src = id;",
            "scm_pas_bw_tbl[1].vectors[0].src = id;",
            "if (is_inited) return;",
        )
    ):
        fail("SCM PAS bandwidth master initialization changed")
    pil_probe = re.sub(r"\s+", " ", pil_source)
    if not all(
        re.search(pattern, pil_probe) is not None
        for pattern in (
            r"crypto_id = MSM_BUS_MASTER_CRYPTO_CORE_0;",
            r'of_parse_phandle\(pdev->dev.of_node, "qcom,mas-crypto", 0\)',
            r'of_property_read_u32\(crypto_node, "cell-id", &crypto_id\)',
            r"scm_pas_init\(\(int\)crypto_id\)",
        )
    ):
        fail("SCM PAS dynamic crypto-master selection changed")
    bus_dts = (source_root / SOURCE_FILES[14]).read_text()
    crypto_node = re.search(r"mas_qxm_crypto:\s+mas-qxm-crypto\s*\{([^}]*)\}", bus_dts, re.DOTALL)
    if crypto_node is None or not all(
        token in crypto_node.group(1)
        for token in (
            "cell-id = <MSM_BUS_MASTER_CRYPTO_CORE_0>",
            "qcom,buswidth = <8>",
            "qcom,connections = <&slv_qns_a1noc_snoc>",
            "qcom,bcms = <&bcm_ce0>",
        )
    ):
        fail("SM6150 crypto master bus-node metadata changed")

    atoll = (source_root / SOURCE_FILES[9]).read_text()
    ipa_fws_match = re.search(r"qcom,ipa_fws\s*\{([^}]*)\}", atoll, re.DOTALL)
    if ipa_fws_match is None:
        fail("Sunfish IPA-FW PIL descriptor is missing")
    if re.search(r"\bclocks\s*=|\b\w+-supply\s*=|qcom,proxy-(?:clock|reg)-names", ipa_fws_match.group(1)):
        fail("Sunfish IPA-FW descriptor unexpectedly declares regulator or clock resources")

    ipa_pas = (ROOT / "rust/src/sunfish_ipa_pas.rs").read_text()
    for token in (
        "CallKind::Availability",
        "CallKind::InitImage",
        "CallKind::MemSetup",
        "CallKind::AuthAndReset",
        "TransportError::RequestKindMismatch",
        "words[0] == 0x4200_0601",
        "words[2] == u64::from(PAS_ID)",
        "CallKind::InitImage | CallKind::AuthAndReset",
        "acquire_shared_pas_vote()",
        "release_shared_pas_vote(lease)",
        "SecureAndBandwidthRelease",
    ):
        if token not in ipa_pas:
            fail(f"IPA PAS resource-envelope adapter is missing {token!r}")
    adapter_start = ipa_pas.index("impl<S, B> IpaPasTransport for ResourceEnvelopeTransport")
    adapter = ipa_pas[adapter_start:ipa_pas.index("pub trait IpaPasTransport", adapter_start)]
    vote_acquire = adapter.find("acquire_shared_pas_vote()")
    secure_call = adapter.find("let secure_result = self.secure.invoke(request)")
    vote_release = adapter.find("release_shared_pas_vote(lease)")
    if min(vote_acquire, secure_call, vote_release) < 0 or not vote_acquire < secure_call < vote_release:
        fail("IPA PAS init/auth vote lease no longer brackets the SMC call")

    subsystem_source = re.sub(r"\s+", " ", (source_root / SOURCE_FILES[7]).read_text())
    if "ret = subsys_start(subsys);" not in subsystem_source:
        fail("subsystem_get no longer starts a PIL-backed subsystem on first reference")

    ipa_utils = (source_root / SOURCE_FILES[3]).read_text()
    if re.search(r"ipa_version\s*==\s*0\s*\)\s*return\s+-EFAULT\s*;", ipa_utils) is None:
        fail("the kernel's nonzero IPA_VERSION access check changed")

    # Sunfish IPA v4.2 selects a logical SVS2 rate which maps to bus usecase
    # 1. The DT flag makes ipa3_get_clks() intentionally omit the GCC handle;
    # uC clock notification is also a no-op past IPA v4.0. Pin this actual
    # first-access path and refuse to describe a bare RO image as safe.
    ipa_node_start = atoll.index("ipa_hw: qcom,ipa@1e00000")
    ipa_node_end = atoll.index("\n\tipa_smmu_ap:", ipa_node_start)
    ipa_node = atoll[ipa_node_start:ipa_node_end]
    if "qcom,bandwidth-vote-for-ipa;" not in ipa_node:
        fail("Sunfish no longer gates IPA clocks through the bus-vote path")
    if "qcom,msm-bus,num-cases = <5>;" not in ipa_node or "qcom,msm-bus,num-paths = <4>;" not in ipa_node:
        fail("Sunfish IPA bus-vote table geometry changed")
    if "qcom,msm-bus,active-only;" in ipa_node:
        fail("Sunfish IPA client context changed from the audited dual-context default")

    ipa_lifecycle_source = re.sub(
        r"\s+", " ", (source_root / SOURCE_FILES[2]).read_text()
    )
    get_clks_start = ipa_lifecycle_source.index("static int ipa3_get_clks(struct device *dev)")
    get_clks_end = ipa_lifecycle_source.index("void _ipa_enable_clks_v3_0(void)", get_clks_start)
    get_clks = ipa_lifecycle_source[get_clks_start:get_clks_end]
    if (
        "if (ipa3_res.use_bw_vote)" not in get_clks
        or "ipa3_clk = NULL; return 0;" not in get_clks
        or "ipa3_clk = clk_get(dev, \"core_clk\");" not in get_clks
    ):
        fail("bandwidth-vote IPA path no longer omits the GCC core-clock handle")

    vote_start = ipa_lifecycle_source.index("void ipa3_enable_clks(void)")
    disable_start = ipa_lifecycle_source.index("void ipa3_disable_clks(void)", vote_start)
    enable_clks = ipa_lifecycle_source[vote_start:disable_start]
    bus_vote_call = enable_clks.find("msm_bus_scale_client_update_request(")
    clock_enable_call = enable_clks.find("ipa3_ctx->ctrl->ipa3_enable_clks();")
    if min(bus_vote_call, clock_enable_call) < 0 or bus_vote_call >= clock_enable_call:
        fail("IPA startup no longer submits the bus vote before the clock callback")

    clock_body_start = ipa_lifecycle_source.index("void _ipa_enable_clks_v3_0(void)")
    clock_body_end = ipa_lifecycle_source.index(
        "static unsigned int ipa3_get_bus_vote(", clock_body_start
    )
    core_clock_body = ipa_lifecycle_source[clock_body_start:clock_body_end]
    if (
        "if (ipa3_clk) {" not in core_clock_body
        or "clk_prepare(ipa3_clk)" not in core_clock_body
        or "clk_enable(ipa3_clk)" not in core_clock_body
        or "clk_set_rate(ipa3_clk, ipa3_ctx->curr_ipa_clk_rate)" not in core_clock_body
        or core_clock_body.index("ipa3_uc_notify_clk_state(true);")
        < core_clock_body.index("clk_set_rate(ipa3_clk, ipa3_ctx->curr_ipa_clk_rate)")
    ):
        fail("optional GCC clock callback or uC notification position changed")

    ipa_uc_source = re.sub(r"\s+", " ", (source_root / SOURCE_FILES[-1]).read_text())
    uc_notify_start = ipa_uc_source.index("int ipa3_uc_notify_clk_state(bool enabled)")
    uc_notify_end = ipa_uc_source.index("int ipa3_uc_update_hw_flags(", uc_notify_start)
    uc_notify = ipa_uc_source[uc_notify_start:uc_notify_end]
    unsupported_v42 = uc_notify.find("if (ipa3_ctx->ipa_hw_type > IPA_HW_v4_0)")
    uc_send = uc_notify.find("ipa3_uc_send_cmd(0, opcode, 0, true, 0)")
    if unsupported_v42 < 0 or uc_send < 0 or unsupported_v42 >= uc_send:
        fail("IPA v4.2 uC clock notification no longer returns before sending a command")

    ipa_probe_start = ipa_lifecycle_source.index(
        "ipa3_ctx->curr_ipa_clk_rate = ipa3_ctx->ctrl->ipa_clk_rate_svs2;"
    )
    ipa_probe_end = ipa_lifecycle_source.index("ipa3_ctx->mmio = ioremap(", ipa_probe_start)
    ipa_probe_startup = ipa_lifecycle_source[ipa_probe_start:ipa_probe_end]
    rate_assignment = ipa_probe_startup.find(
        "ipa3_ctx->curr_ipa_clk_rate = ipa3_ctx->ctrl->ipa_clk_rate_svs2;"
    )
    initial_enable = ipa_probe_startup.find("ipa3_enable_clks();")
    if rate_assignment < 0 or initial_enable < 0 or rate_assignment >= initial_enable:
        fail("IPA probe no longer selects the initial SVS2 rate before the first enable")
    if ipa_probe_startup.count("ipa3_enable_clks();") != 1:
        fail("IPA probe startup sequence contains an unexpected initial clock enable")

    post_init = (ROOT / "rust/src/sunfish_ipa_post_init.rs").read_text()
    if (
        "Stage::InitialBusVote" not in post_init
        or "Stage::RegisterAccess" not in post_init
        or post_init.index("apply_initial_bus_vote(self.generation, initial_use_case)")
        >= post_init.index("stage!(establish_register_access, RegisterAccess)")
        or any(
            token in post_init
            for token in (
                "Stage::ClockPrepare",
                "Stage::ClockEnable",
                "Stage::ClockSetRate",
                "Stage::ClockRateReadback",
                "Stage::UcClockNotify",
                "ClockRateMismatch",
            )
        )
    ):
        fail("native IPA post-init does not match the bus-vote-before-MMIO v4.2 path")

    bus_of = re.sub(r"\s+", " ", (source_root / BUS_SOURCE_FILES[0]).read_text())
    if (
        'if (of_property_read_bool(of_node, "qcom,msm-bus,active-only"))' not in bus_of
        or "pdata->active_only = 1;" not in bus_of
        or "active_only flag absent." not in bus_of
        or "Using dual context by default\\n" not in bus_of
    ):
        fail("the msm_bus dual-context default changed")
    if (
        "usecase[i].vectors[j].ab = (uint64_t) KBTOB(be32_to_cpu(vec_arr[index + 2]));"
        not in bus_of
        or "usecase[i].vectors[j].ib = (uint64_t) KBTOB(be32_to_cpu(vec_arr[index + 3]));"
        not in bus_of
    ):
        fail("msm_bus no longer converts IPA DTS bandwidth vectors from KB/s to B/s")
    bus_core = (source_root / BUS_SOURCE_FILES[3]).read_text()
    if re.search(r"^#define\s+KBTOB\(a\)\s+\(a\s*\*\s*1000ULL\)$", bus_core, re.MULTILINE) is None:
        fail("the msm_bus KB/s to B/s conversion changed")

    bus_arb = re.sub(r"\s+", " ", (source_root / BUS_SOURCE_FILES[1]).read_text())
    update_start = bus_arb.index("static int update_client_paths(")
    update_end = bus_arb.index("static int update_client_alc(", update_start)
    update_client = bus_arb[update_start:update_end]
    dual_split = (
        "if (pdata->active_only) { slp_clk = 0; slp_bw = 0; } else { "
        "slp_clk = req_clk; slp_bw = req_bw; req_clk = 0; req_bw = 0; }"
    )
    if dual_split not in update_client:
        fail("msm_bus client requests no longer split into active and dual contexts as expected")
    if "aggregate_bus_req(dev_info, i); bcm_update_bus_req(next_dev, i);" not in bus_arb:
        fail("route updates no longer aggregate bus nodes before BCM votes")
    aggregate_start = bus_arb.index("static void aggregate_bus_req(")
    aggregate_end = bus_arb.index("static void aggregate_bus_query_req(", aggregate_start)
    aggregate_body = bus_arb[aggregate_start:aggregate_end]
    if (
        "sum_ab += bus_dev->lnode_list[i].lnode_ab[ctx];" not in aggregate_body
        or "max_ib = max(max_ib, bus_dev->lnode_list[i].lnode_ib[ctx]);" not in aggregate_body
    ):
        fail("msm_bus node aggregation no longer sums average and maximizes instantaneous bandwidth")
    bus_model = (ROOT / "rust/src/qcom_msm_bus_aggregate.rs").read_text()
    bus_model_tokens = (
        "pub struct Snapshot<'a>",
        "pub struct ClientReport<'a>",
        "client.identity != snapshot.identity",
        "client.generation != snapshot.generation",
        "checked_add(contribution.average)",
        "target.peak = target.peak.max(contribution.peak)",
        "add(&mut contexts.active_only, vote.contexts.active_only)?",
        "add(&mut contexts.dual, vote.contexts.dual)?",
        "Error::MissingRouteNode",
        "pub struct BcmBinding<'a>",
        "pub fn aggregate_bcm_links<",
        "Error::MissingNodeAggregate",
        "legacy_update_ordered(ordered).map_err(Error::Bcm)",
    )
    for token in bus_model_tokens:
        if token not in bus_model:
            fail(f"closed-set MSM bus aggregation model is missing {token!r}")
    bcm_update_start = bus_arb.index("static void bcm_update_bus_req(")
    bcm_update_end = bus_arb.index("static void bcm_query_bus_req(", bcm_update_start)
    bcm_update = bus_arb[bcm_update_start:bcm_update_end]
    bcm_scale_tokens = (
        "cur_dev->node_bw[ctx].max_ib * (uint64_t)bcm_dev->bcmdev->width",
        "cur_dev->node_info->agg_params.buswidth",
        "cur_dev->node_bw[ctx].sum_ab * (uint64_t)bcm_dev->bcmdev->width",
        "cur_dev->node_info->agg_params.buswidth * cur_dev->node_info->agg_params.num_aggports",
    )
    if any(token not in bcm_update for token in bcm_scale_tokens):
        fail("pinned bcm_update_bus_req scaling inputs changed")
    for token in ("active_only: scale_node(", "dual: scale_node("):
        if token not in bus_model:
            fail(f"complete-node BCM scaling adapter is missing {token!r}")

    bus_fabric = re.sub(r"\s+", " ", (source_root / BUS_SOURCE_FILES[2]).read_text())
    commit_start = bus_fabric.index("int msm_bus_commit_data(")
    commit_end = bus_fabric.index("int msm_bus_query_gen(", commit_start)
    commit_body = bus_fabric[commit_start:commit_end]
    active_submit = commit_body.find("rpmh_write_batch(cur_mbox, cur_rsc->rscdev->req_state, cmdlist_active, n_active)")
    wake_submit = commit_body.find("rpmh_write_batch(cur_mbox, RPMH_WAKE_ONLY_STATE, cmdlist_wake, n_wake)")
    sleep_submit = commit_body.find("rpmh_write_batch(cur_mbox, RPMH_SLEEP_STATE, cmdlist_sleep, n_sleep)")
    if min(active_submit, wake_submit, sleep_submit) < 0 or not active_submit < wake_submit < sleep_submit:
        fail("msm_bus no longer submits active, wake-only, then sleep BCM batches")

    vector_body_match = re.search(
        r"qcom,msm-bus,vectors-KBps\s*=\s*(.*?)\s*;\s*qcom,bus-vector-names",
        ipa_node,
        re.DOTALL,
    )
    if vector_body_match is None:
        fail("Sunfish IPA bus-vote vectors are missing")
    vector_body = re.sub(r"/\*.*?\*/", "", vector_body_match.group(1), flags=re.DOTALL)
    vectors = [
        (master, slave, int(average), int(instantaneous))
        for master, slave, average, instantaneous in re.findall(
            r"<\s*([A-Z0-9_]+)\s+([A-Z0-9_]+)\s+(\d+)\s+(\d+)\s*>",
            vector_body,
        )
    ]
    expected_vectors = [
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_EBI_CH0", 0, 0),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_OCIMEM", 0, 0),
        ("MSM_BUS_MASTER_AMPSS_M0", "MSM_BUS_SLAVE_IPA_CFG", 0, 0),
        ("MSM_BUS_MASTER_IPA_CORE", "MSM_BUS_SLAVE_IPA_CORE", 0, 0),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_EBI_CH0", 80_000, 465_000),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_OCIMEM", 80_000, 68_570),
        ("MSM_BUS_MASTER_AMPSS_M0", "MSM_BUS_SLAVE_IPA_CFG", 80_000, 30),
        ("MSM_BUS_MASTER_IPA_CORE", "MSM_BUS_SLAVE_IPA_CORE", 0, 30),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_EBI_CH0", 80_000, 2_000_000),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_OCIMEM", 80_000, 267_461),
        ("MSM_BUS_MASTER_AMPSS_M0", "MSM_BUS_SLAVE_IPA_CFG", 80_000, 109_890),
        ("MSM_BUS_MASTER_IPA_CORE", "MSM_BUS_SLAVE_IPA_CORE", 0, 109),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_EBI_CH0", 206_000, 4_000_000),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_OCIMEM", 206_000, 712_961),
        ("MSM_BUS_MASTER_AMPSS_M0", "MSM_BUS_SLAVE_IPA_CFG", 206_000, 491_520),
        ("MSM_BUS_MASTER_IPA_CORE", "MSM_BUS_SLAVE_IPA_CORE", 0, 491),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_EBI_CH0", 206_000, 5_598_900),
        ("MSM_BUS_MASTER_IPA", "MSM_BUS_SLAVE_OCIMEM", 206_000, 1_436_481),
        ("MSM_BUS_MASTER_AMPSS_M0", "MSM_BUS_SLAVE_IPA_CFG", 206_000, 491_520),
        ("MSM_BUS_MASTER_IPA_CORE", "MSM_BUS_SLAVE_IPA_CORE", 0, 491),
    ]
    if vectors != expected_vectors:
        fail("Sunfish IPA msm_bus vectors changed")

    bus_ids = (source_root / SOURCE_FILES[13]).read_text()
    expected_bus_ids = {
        "MSM_BUS_MASTER_AMPSS_M0": 1,
        "MSM_BUS_MASTER_IPA": 90,
        "MSM_BUS_MASTER_IPA_CORE": 143,
        "MSM_BUS_SLAVE_EBI_CH0": 512,
        "MSM_BUS_SLAVE_OCIMEM": 585,
        "MSM_BUS_SLAVE_IPA_CFG": 676,
        "MSM_BUS_SLAVE_IPA_CORE": 777,
    }
    for name, value in expected_bus_ids.items():
        if re.search(rf"^#define\s+{name}\s+{value}$", bus_ids, re.MULTILINE) is None:
            fail(f"Sunfish IPA bus endpoint {name} changed")

    ipa_rates = {
        "IPA_V4_2_CLK_RATE_SVS2": 50_000_000,
        "IPA_V4_2_CLK_RATE_SVS": 100_000_000,
        "IPA_V4_2_CLK_RATE_NOMINAL": 201_000_000,
        "IPA_V4_2_CLK_RATE_TURBO": 240_000_000,
    }
    for name, value in ipa_rates.items():
        if re.search(rf"#define\s+{name}\s+\({value // 1_000_000}\s*\*\s*1000\s*\*\s*1000UL\)", ipa_utils) is None:
            fail(f"Sunfish IPA clock rate {name} changed")
    ipa_driver = re.sub(r"\s+", " ", (source_root / SOURCE_FILES[2]).read_text())
    rate_to_index = (
        ("ipa_clk_rate_svs2", 1),
        ("ipa_clk_rate_svs", 2),
        ("ipa_clk_rate_nominal", 3),
    )
    for rate_name, index in rate_to_index:
        if re.search(rf"curr_ipa_clk_rate == ipa3_ctx->ctrl->{rate_name}\) \{{ idx = {index}; \}}", ipa_driver) is None:
            fail(f"Sunfish IPA clock rate {rate_name} no longer selects bus usecase {index}")
    if re.search(r"ipa3_ctx->curr_ipa_clk_rate = ipa3_ctx->ctrl->ipa_clk_rate_svs2; .*?ipa3_enable_clks\(\);", ipa_driver) is None:
        fail("Linux no longer applies the initial SVS2 vote before IPA register setup")
    enable_start = ipa_driver.index("void ipa3_enable_clks(void)")
    enable_end = ipa_driver.index("void _ipa_disable_clks_v3_0(void)", enable_start)
    enable_body = ipa_driver[enable_start:enable_end]
    if not enable_body.index("msm_bus_scale_client_update_request") < enable_body.index("ipa3_enable_clks();"):
        fail("Linux IPA clock enable no longer submits the bus vote first")

    ipa_bus_profile = (ROOT / "rust/src/sunfish_ipa_bus_vote.rs").read_text()
    route_validator_path = ROOT / "platforms/sunfish/validate-bcm-routes.py"
    route_spec = importlib.util.spec_from_file_location(
        "sunfish_bcm_routes", route_validator_path
    )
    if route_spec is None or route_spec.loader is None:
        fail("could not load the pinned Sunfish BCM route validator")
    route_validator = importlib.util.module_from_spec(route_spec)
    route_spec.loader.exec_module(route_validator)
    route_constants = {
        "SUNFISH_IPA_EBI_ROUTE_NODES": "ipa_ebi",
        "SUNFISH_IPA_OCIMEM_ROUTE_NODES": "ipa_ocimem",
        "SUNFISH_IPA_CFG_ROUTE_NODES": "ipa_cfg",
        "SUNFISH_IPA_CORE_ROUTE_NODES": "ipa_core",
    }
    for rust_name, route_name in route_constants.items():
        match = re.search(
            rf"pub const {rust_name}:\s*\[u16;\s*\d+\]\s*=\s*\[([^]]*)\]",
            ipa_bus_profile,
        )
        if match is None:
            fail(f"missing Rust Sunfish IPA route {rust_name}")
        observed_nodes = [
            int(token.strip().replace("_", ""), 0)
            for token in match.group(1).split(",")
            if token.strip()
        ]
        expected_nodes = [
            route_validator.NODES[name][0]
            for name in route_validator.ROUTES[route_name]
        ]
        if observed_nodes != expected_nodes:
            fail(f"Rust {rust_name} differs from the pinned DT route {route_name}")
    print("PASS: Rust IPA route-node sequences match the pinned Sunfish route table")

    rust_to_dts_bus_names = {
        "MASTER_AMPSS_M0": "MSM_BUS_MASTER_AMPSS_M0",
        "MASTER_IPA": "MSM_BUS_MASTER_IPA",
        "MASTER_IPA_CORE": "MSM_BUS_MASTER_IPA_CORE",
        "SLAVE_EBI_CH0": "MSM_BUS_SLAVE_EBI_CH0",
        "SLAVE_OCIMEM": "MSM_BUS_SLAVE_OCIMEM",
        "SLAVE_IPA_CFG": "MSM_BUS_SLAVE_IPA_CFG",
        "SLAVE_IPA_CORE": "MSM_BUS_SLAVE_IPA_CORE",
    }
    rust_vectors = [
        (
            rust_to_dts_bus_names[master],
            rust_to_dts_bus_names[slave],
            int(average.replace("_", "")),
            int(instantaneous.replace("_", "")),
        )
        for master, slave, average, instantaneous in re.findall(
            r"path\((MASTER_[A-Z0-9_]+),\s*(SLAVE_[A-Z0-9_]+),\s*([\d_]+),\s*([\d_]+)\)",
            ipa_bus_profile,
        )
    ]
    if rust_vectors != vectors:
        fail("Rust Sunfish IPA bus-vote profile differs from the complete DTS usecase table")
    profile_tokens = (
        "DUAL_CONTEXT: bool = true",
        "RATE_SVS2_HZ: u32 = 50_000_000",
        "RATE_SVS_HZ: u32 = 100_000_000",
        "RATE_NOMINAL_HZ: u32 = 201_000_000",
        "RATE_TURBO_HZ: u32 = 240_000_000",
        "RATE_SVS2_HZ => 1",
        "RATE_SVS_HZ => 2",
        "RATE_NOMINAL_HZ => 3",
        "RATE_TURBO_HZ => USE_CASE_COUNT - 1",
    )
    for token in profile_tokens:
        if token not in ipa_bus_profile:
            fail(f"Rust Sunfish IPA bus-vote profile is missing {token!r}")

    # Pin the fixed IPA v4.2 fields used to build Sunfish's
    # INIT_MODEM_DRIVER QMI request. Runtime base/size, endpoint allocation,
    # and uC-loaded state remain caller observations in Rust.
    profile = (ROOT / "rust/src/sunfish_ipa_profile.rs").read_text()
    table_start = ipa_utils.index("static struct ipa3_mem_partition ipa_4_2_mem_part = {")
    table_end = ipa_utils.index("\n};", table_start)
    table = ipa_utils[table_start:table_end]
    partition_fields = {
        "end_ofst": 0x2000,
        "v4_rt_nhash_ofst": 0x3A0,
        "v4_modem_rt_index_hi": 0x7,
        "v6_rt_nhash_ofst": 0x428,
        "v6_modem_rt_index_hi": 0x7,
        "v4_flt_nhash_ofst": 0x290,
        "v6_flt_nhash_ofst": 0x318,
        "modem_hdr_ofst": 0x4A8,
        "modem_hdr_size": 0x140,
        "modem_hdr_proc_ctx_ofst": 0x5F0,
        "modem_hdr_proc_ctx_size": 0x200,
        "modem_ofst": 0xBF0,
        "modem_size": 0x140C,
        "stats_quota_q6_ofst": 0xA50,
        "stats_quota_q6_size": 0x60,
        "stats_drop_ofst": 0xBF0,
        "stats_drop_size": 0,
        "modem_comp_decomp_size": 0,
        "v4_rt_hash_size": 0,
        "v6_rt_hash_size": 0,
        "v4_flt_hash_size": 0,
        "v6_flt_hash_size": 0,
    }
    for field, expected in partition_fields.items():
        source_match = re.search(
            rf"\.{re.escape(field)}\s*=\s*(0x[0-9a-fA-F]+|\d+)", table
        )
        if source_match is None or int(source_match.group(1), 0) != expected:
            fail(f"IPA v4.2 partition field {field} no longer matches {expected:#x}")

    profile_constants = {
        "PARTITION_END_BYTES": 0x2000,
        "V4_ROUTE_OFFSET": 0x3A0,
        "V6_ROUTE_OFFSET": 0x428,
        "V4_FILTER_OFFSET": 0x290,
        "V6_FILTER_OFFSET": 0x318,
        "HEADER_OFFSET": 0x4A8,
        "HEADER_SIZE": 0x140,
        "HEADER_PROCESSING_OFFSET": 0x5F0,
        "HEADER_PROCESSING_SIZE": 0x200,
        "MODEM_MEMORY_OFFSET": 0xBF0,
        "MODEM_MEMORY_SIZE": 0x140C,
        "STATS_QUOTA_OFFSET": 0xA50,
        "STATS_QUOTA_SIZE": 0x60,
        "STATS_DROP_OFFSET": 0xBF0,
        "STATS_DROP_SIZE": 0,
        "APPS_WAN_CONS_ENDPOINT": 9,
    }
    for name, expected in profile_constants.items():
        match = re.search(
            rf"(?:pub )?const {name}: u32 = (0x[0-9a-fA-F_]+|\d+);", profile
        )
        if match is None or int(match.group(1).replace("_", ""), 0) != expected:
            fail(f"Rust IPA profile constant {name} no longer matches {expected:#x}")

    endpoint_start = ipa_utils.find("[IPA_4_2][IPA_CLIENT_APPS_WAN_CONS]")
    endpoint_end = ipa_utils.find("\n\t[IPA_4_2][", endpoint_start + 1)
    endpoint_table = ipa_utils[endpoint_start:endpoint_end] if endpoint_start >= 0 and endpoint_end >= 0 else ""
    if re.search(
        r"\{\s*9\s*,\s*3\s*,\s*6\s*,\s*6\s*,\s*IPA_EE_AP\s*,\s*GSI_ESCAPE_BUF_ONLY\s*\}",
        endpoint_table,
    ) is None:
        fail("IPA v4.2 APPS_WAN_CONS endpoint assignment changed")

    atoll_ipa = re.search(r"ipa_hw:\s+qcom,ipa@1e00000\s*\{([^}]*)\}", atoll, re.DOTALL)
    if atoll_ipa is None or "qcom,ipa-fltrt-not-hashable;" not in atoll_ipa.group(1):
        fail("Sunfish IPA node no longer disables unsupported hash tables")
    if re.search(r"hash_tables:\s*None,", profile) is None:
        fail("Rust IPA profile no longer omits the unsupported hash-table TLVs")

    qmi_source = (source_root / SOURCE_FILES[15]).read_text()
    qmi_flat = re.sub(r"\s+", " ", qmi_source)
    request_field_sources = (
        "req.platform_type = ipa_wan_platform;",
        "req.hdr_tbl_info.modem_offset_start = IPA_MEM_PART(modem_hdr_ofst) + smem_restr_bytes;",
        "req.hdr_tbl_info.modem_offset_end = IPA_MEM_PART(modem_hdr_ofst) + smem_restr_bytes + IPA_MEM_PART(modem_hdr_size) - 1;",
        "req.v4_route_tbl_info.route_tbl_start_addr = IPA_MEM_PART(v4_rt_nhash_ofst) + smem_restr_bytes;",
        "req.v4_route_tbl_info.num_indices = IPA_MEM_PART(v4_modem_rt_index_hi);",
        "req.v6_route_tbl_info.route_tbl_start_addr = IPA_MEM_PART(v6_rt_nhash_ofst) + smem_restr_bytes;",
        "req.v6_route_tbl_info.num_indices = IPA_MEM_PART(v6_modem_rt_index_hi);",
        "req.v4_filter_tbl_start_addr = IPA_MEM_PART(v4_flt_nhash_ofst) + smem_restr_bytes;",
        "req.v6_filter_tbl_start_addr = IPA_MEM_PART(v6_flt_nhash_ofst) + smem_restr_bytes;",
        "req.modem_mem_info.block_start_addr = IPA_MEM_PART(modem_ofst) + smem_restr_bytes;",
        "req.modem_mem_info.size = IPA_MEM_PART(modem_size);",
        "req.ctrl_comm_dest_end_pt = ipa3_get_ep_mapping(IPA_CLIENT_APPS_WAN_CONS);",
        "req.hw_stats_quota_base_addr = IPA_MEM_PART(stats_quota_q6_ofst) + smem_restr_bytes;",
        "req.hw_stats_quota_size = IPA_MEM_PART(stats_quota_q6_size);",
        "req.hw_drop_stats_base_addr = IPA_MEM_PART(stats_drop_ofst) + smem_restr_bytes;",
        "req.hw_drop_stats_table_size = IPA_MEM_PART(stats_drop_size);",
    )
    if any(re.sub(r"\s+", " ", item) not in qmi_flat for item in request_field_sources):
        fail("IPA INIT_MODEM_DRIVER request field formulas changed")
    rmnet_source = re.sub(
        r"\s+", " ", (source_root / SOURCE_FILES[16]).read_text()
    )
    if "ipa3_qmi_service_init(QMI_IPA_PLATFORM_TYPE_MSM_ANDROID_V01);" not in rmnet_source:
        fail("Sunfish WAN init no longer selects the MSM Android QMI platform type")
    qmi_schema = (source_root / SOURCE_FILES[17]).read_text()
    if re.search(
        r"QMI_IPA_PLATFORM_TYPE_MSM_ANDROID_V01\s*=\s*3\s*,", qmi_schema
    ) is None:
        fail("MSM Android IPA QMI platform value changed")

    probe = (ROOT / "rust/src/sunfish_ipa_read_probe.rs").read_text()
    if (
        'display.hex(3, "SHMEM BASE U8", shared_mem >> 16);' not in probe
        or 'display.hex(4, "SHMEM SIZE U8", shared_mem & 0xffff);' not in probe
        or "IPA_SHARED_MEM_SIZE" not in probe
        or "write32(" in probe
        or "write_volatile(" in probe
    ):
        fail("read-only IPA probe output no longer matches the pinned shared-memory fields")

    ipa_reg_source = (source_root / SOURCE_FILES[4]).read_text()
    ipa_enum = (source_root / SOURCE_FILES[5]).read_text()
    for index, version in enumerate(IPA_VERSION_CHAIN, start=10):
        pattern = rf"\bIPA_HW_{version}\s*=\s*{index}\s*,"
        if re.search(pattern, ipa_enum) is None:
            fail(f"IPA hardware enum {version} no longer has expected value {index}")
    init_source = re.sub(r"\s+", " ", ipa_reg_source)
    if "if (!memcmp(&ipahal_reg_objs[i+1][j], &zero_obj, sizeof(struct ipahal_reg_obj)))" not in init_source:
        fail("IPA register inheritance algorithm changed")

    for register, expected in IPA_OFFSETS.items():
        entries: dict[str, int] = {}
        entry_pattern = (
            rf"\[IPA_HW_(v[0-9_]+)\]\[IPA_{register}\]\s*=\s*\{{([^}}]+)\}}"
        )
        for match in re.finditer(entry_pattern, ipa_reg_source, re.DOTALL):
            fields = [field.strip() for field in match.group(2).split(",")]
            if len(fields) < 3:
                fail(f"IPA {register} table entry has no address field")
            entries[match.group(1)] = int(fields[2], 0)

        effective_offset = None
        for version in IPA_VERSION_CHAIN:
            if version in entries:
                value = entries[version]
                effective_offset = None if value == -1 else value
        if effective_offset != expected:
            fail(
                f"IPA v4.2 {register} resolves to {effective_offset!r}, expected 0x{expected:03x}"
            )

    print("IPA/GSI register source validation passed for pinned Sunfish kernel")
    print("PASS: Sunfish IPA v4.2 attaches SMMU, starts PAS-0xf ipa_fws, then runs post-init")
    print("PASS: PAS init/auth bandwidth and auth resource wrappers match PIL-TZ source")
    print("PASS: PAS bandwidth table and dynamic crypto-master endpoint match pinned source")
    print("PASS: ipa_fws declares no per-device clock or regulator resources")
    print("PASS: IPA PAS adapter matches request classes and brackets only init/auth with a shared vote lease")
    print("PASS: Sunfish IPA v4.2 QMI profile constants, endpoint, and request formulas match source")
    print("PASS: Sunfish IPA clock rates, msm_bus endpoints, usecases, and initial SVS2 vote match source")
    print("PASS: IPA votes default to dual contexts and Linux submits active, wake, and sleep BCM sets")
    print("PASS: closed-set MSM bus aggregation model follows pinned per-node sum/max semantics")
    print("PASS: complete bus-node contexts scale and aggregate in pinned BCM binding order")
    print("LIMIT: this is source evidence only; no secure firmware path is wired in seL4")
    print("LIMIT: the standalone IPA register probe has no IPA bandwidth-vote provider and must not be booted")


if __name__ == "__main__":
    main()
