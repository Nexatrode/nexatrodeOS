#!/usr/bin/env python3
"""Check Sunfish HTC/WMI/HTT startup and TX codecs against pinned source."""

from hashlib import sha256
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[2]
KERNEL = ROOT / ".cache/sunfish-lineage-kernel"
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
SOURCE_HASHES = {
    "drivers/staging/fw-api/fw/htt.h":
        "87fd87b718cb242b176c231ddf9ec65219665eac4dcc042badfec637b2a7106f",
    "drivers/staging/fw-api/fw/htt_common.h":
        "47d869a6343bd1ed379a3bc8de0185dbf542c591f3f10a1249eb3b029a647229",
    "drivers/staging/qcacld-3.0/configs/default_defconfig":
        "0fbd8dab50221d97800a77c79c6ad84dc7d81ca2368bc5289f2297c7d4f559ef",
    "drivers/staging/qcacld-3.0/Kbuild":
        "eb861eaca894ffdda7acc0730c09c2b1f9b543e1b1c23119ca6ba3abc32705b7",
    "drivers/staging/qcacld-3.0/core/dp/htt/htt_types.h":
        "8589d850c39e792fb30e3e1805f82c97f4620c56a3be71b257aac48cd4df0d4b",
    "drivers/staging/qcacld-3.0/core/dp/ol/inc/ol_htt_tx_api.h":
        "22dd91f1695dd8d670345d1e610133d066ad8d115cd8bbd5f427b09f0a8566e9",
    "drivers/staging/qcacld-3.0/core/dp/htt/htt_tx.c":
        "ad01fa854a2ccdb9faa8dbc65bc9e69cd3879ad820d5a1fa74da57e50fd0fb93",
    "drivers/staging/qcacld-3.0/core/dp/txrx/ol_tx_desc.c":
        "53164a1601672ea44e391fabcf41c4694e558854d8b4897f7d7c6c916ee69fd1",
    "drivers/staging/qcacld-3.0/core/dp/htt/htt.c":
        "7b7d7cae2255060ac7fda0fa81ebcdc1702f11d4c2ca82f828de63af651dfb46",
    "drivers/staging/qcacld-3.0/core/dp/txrx/ol_cfg.c":
        "d39851c7adeb8a170722d945c53c8dfaf56cd5918b2e984fae2f0b5465d95aef",
    "drivers/staging/qcacld-3.0/core/dp/txrx/ol_tx_ll_fastpath.c":
        "73b849e4cb92849db07fc37addb84a3753fb9a3c5c3cb4ffb21be71a3ff1fc00",
    "drivers/staging/fw-api/fw/htc_services.h":
        "96adfbf9139d894c60baa947835464a35da2e9b621d600c44b32026a68a3ddac",
    "drivers/staging/fw-api/fw/htc.h":
        "b3face5874b7bfbe61637caa76d5381a32e3043654e2c476b5d0c10873b77800",
    "drivers/staging/qca-wifi-host-cmn/htc/htc_services.c":
        "f99ad98e7acaccc6b52a76b430921f24776fe326cec1033cb2eea36ac2c80dd7",
    "drivers/staging/qca-wifi-host-cmn/htc/htc.c":
        "34643623d23451e6cf6f4e0f8133487c4e2486a08a2b23fc0accfc375b5802d0",
    "drivers/staging/qca-wifi-host-cmn/htc/htc_send.c":
        "b50e671becfd9922c939294aa686e91d6ee1d16e73e0b7fc49e87880ef2fb5e4",
    "drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_main.c":
        "d8d37bd9ff677844b243275ad17a9632ef87fb5baec0825d6dcde56d5943ff42",
    "drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_service.c":
        "7683aa3a4a65bd72666716ed9656fd75f440bdf43218f10c12cccf5868250757",
    "drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_assignment.h":
        "40ec0ea566cb3f0b2518407fa3096586d1a4ed6a980e016d087381216efca3e7",
    "drivers/staging/qcacld-3.0/core/cds/src/cds_api.c":
        "d35149ac11a1897458e9a12b821661eab206c3e66625f5bb85e7341402fc72ba",
    "drivers/staging/qcacld-3.0/core/wma/src/wma_main.c":
        "855246868007c34b78031d80db6ae0e98441b0144f7bf0a81ea31a1ffc0b4343",
    "drivers/staging/qcacld-3.0/core/hdd/src/wlan_hdd_main.c":
        "0aa714f6566af7cbd28ca5c6c1242a4f744c5325204cc82aecc4260eddac7bc3",
    "drivers/staging/qcacld-3.0/core/dp/txrx/ol_txrx.c":
        "1faa77c50d73e3213bd515379e2fb99bf158f59384e83711e8967538f536c775",
}


def require(text: str, fragment: str, source: str) -> None:
    if fragment not in text:
        raise SystemExit(f"{source}: missing pinned ABI evidence: {fragment!r}")


def main() -> None:
    commit = subprocess.check_output(
        ["git", "-C", str(KERNEL), "rev-parse", "HEAD"], text=True
    ).strip()
    if commit != PINNED_COMMIT:
        raise SystemExit(f"unexpected QCACLD source commit: {commit}")

    texts = {}
    for relative, expected in SOURCE_HASHES.items():
        path = KERNEL / relative
        actual = sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            raise SystemExit(f"pinned QCACLD source hash mismatch: {relative}")
        texts[relative] = path.read_text(errors="replace")

    config = texts["drivers/staging/qcacld-3.0/configs/default_defconfig"]
    for line in (
        "CONFIG_HELIUMPLUS := y",
        "CONFIG_64BIT_PADDR := y",
        "CONFIG_HTT_PADDR64 := y",
        "CONFIG_WIFI_3_0_ADRASTEA := y",
    ):
        require(config, line, "default_defconfig")
    for fragment in (
        "ifdef CONFIG_ICNSS\n\tCONFIG_ROME_IF = snoc",
        "ifeq ($(CONFIG_ICNSS), y)\nCONFIG_WIFI_3_0_ADRASTEA := y",
        "CONFIG_ATH_11AC_TXCOMPACT := y",
        "ifeq ($(CONFIG_ROME_IF),snoc)\n\tCONFIG_HIF_SNOC:= y",
    ):
        require(config, fragment, "default_defconfig")

    kbuild = texts["drivers/staging/qcacld-3.0/Kbuild"]
    for fragment in (
        "cppflags-$(CONFIG_ATH_11AC_TXCOMPACT) += -DATH_11AC_TXCOMPACT",
        "cppflags-$(CONFIG_HIF_SNOC) += -DHIF_SNOC",
        "ifeq ($(CONFIG_QCA_WIFI_SDIO), y)\ncppflags-y += -DCONFIG_HL_SUPPORT",
        "ifeq ($(CONFIG_HIF_USB), y)\ncppflags-y += -DHIF_USB",
    ):
        require(kbuild, fragment, "Kbuild")
    if kbuild.count("-DCONFIG_HL_SUPPORT") != 2:
        raise SystemExit("Kbuild: expected CONFIG_HL_SUPPORT only in SDIO and USB branches")

    ol_cfg = texts["drivers/staging/qcacld-3.0/core/dp/txrx/ol_cfg.c"]
    require(ol_cfg, "cfg_ctx->tx_download_size = 16;", "ol_cfg.c")

    common = texts["drivers/staging/fw-api/fw/htt_common.h"]
    for fragment in (
        "HTT_NON_QOS_TID     16",
        "HTT_INVALID_TID     31",
        "htt_pkt_type_native_wifi = 1",
    ):
        require(common, fragment, "htt_common.h")

    htt = texts["drivers/staging/fw-api/fw/htt.h"]
    for fragment in (
        "HTT_H2T_MSG_TYPE_TX_FRM                = 0x1",
        "HTT_TX_DESC_PKT_TYPE_M         0x0000e000",
        "HTT_TX_DESC_VDEV_ID_M          0x003f0000",
        "HTT_TX_DESC_EXT_TID_M          0x07c00000",
        "HTT_TX_DESC_NO_ENCRYPT_M      0x00000400",
        "HTT_TX_DESC_TX_COMP_M 0x80000000",
        "HTT_TX_DESC_FRM_LEN_M          0x0000ffff",
        "HTT_TX_DESC_FRM_ID_M           0xffff0000",
        "HTT_TX_DESC_PEER_ID_OFFSET_BYTES_64 16",
        "HTT_TX_DESC_CHAN_FREQ_M 0xffff0000",
    ):
        require(htt, fragment, "fw/htt.h")
    for fragment in (
        "#define HTT_TX_HDR_SIZE_802_11_RAW 36",
        "#define HTT_LL_TX_HDR_SIZE_IP 16",
        "#define HTT_TX_HDR_SIZE_802_1Q 4",
        "#define HTT_TX_HDR_SIZE_LLC_SNAP 8",
    ):
        require(htt, fragment, "fw/htt.h")

    types = texts["drivers/staging/qcacld-3.0/core/dp/htt/htt_types.h"]
    for fragment in (
        "struct msdu_ext_frag_desc",
        "struct msdu_ext_desc_t",
        "struct msdu_ext_frag_desc frags[6];",
        "semantically, this is an array of 6 of 2-tuples",
        "a 48-bit physical address and a 16 bit len field",
        "uint8_t htc_header[HTC_HEADER_LEN];",
        "struct htt_tx_msdu_desc_t tx_desc;",
    ):
        require(types, fragment, "htt_types.h")

    api = texts["drivers/staging/qcacld-3.0/core/dp/ol/inc/ol_htt_tx_api.h"]
    for fragment in (
        "#define HTT_TX_DESC_VADDR_OFFSET 8",
        "word32 == (uint32_t *)",
        "&(((struct msdu_ext_desc_t *)desc)->frags[0])",
        "frag_phys_addr",
        "The frag_phys address is 37 bits.",
        "fdesc[num_frags].u.desc64 = 0",
    ):
        require(api, fragment, "ol_htt_tx_api.h")

    tx = texts["drivers/staging/qcacld-3.0/core/dp/htt/htt_tx.c"]
    for fragment in (
        "pdev->frag_descs.size = sizeof(struct msdu_ext_desc_t);",
        "*frag_filed_ptr = 0;",
        "pkt_type = msdu_info->info.l2_hdr_type;",
        "HTT_H2T_MSG_TYPE_SET(local_word0, HTT_H2T_MSG_TYPE_TX_FRM);",
        "qdf_nbuf_len(msdu)",
        "msdu_info->action.do_encrypt ?",
        "htc_send_data_pkt(pdev->htc_pdev, msdu,",
        "pdev->htc_tx_endpoint",
    ):
        require(tx, fragment, "htt_tx.c")

    htt_attach = texts["drivers/staging/qcacld-3.0/core/dp/htt/htt.c"]
    require(htt_attach, "HTT_SET_WIFI_IP(pdev, 2, 0);", "htt.c")
    require(htt_attach, "htt_htc_attach(pdev, HTT_DATA_MSG_SVC)", "htt.c")
    require(htt_attach, "htt_update_endpoint(pdev, service_id, response.Endpoint);",
            "htt.c")
    htt_pdev_alloc = htt_attach[htt_attach.index("htt_pdev_alloc("):]
    htt_pdev_alloc = htt_pdev_alloc[:htt_pdev_alloc.index("\n}")]
    require(htt_pdev_alloc, "htt_htc_attach_all(pdev)", "htt.c")
    require(htt_pdev_alloc, "before calling htt_rx_attach", "htt.c")
    htt_connect = htt_attach[htt_attach.index("int htt_htc_attach("):]
    htt_connect = htt_connect[:htt_connect.index("\n}")]
    require(htt_connect, "connect.service_id = service_id;", "htt.c")
    require(htt_connect, "status = htc_connect_service(pdev->htc_pdev, &connect, &response);",
            "htt.c")
    for fragment in (
        "pdev->download_len = sizeof(struct htt_host_tx_desc_t)",
        "+ HTT_TX_HDR_SIZE_OUTER_HDR_MAX /* worst case */",
        "+ HTT_TX_HDR_SIZE_802_1Q",
        "+ HTT_TX_HDR_SIZE_LLC_SNAP",
        "+ ol_cfg_tx_download_size(pdev->ctrl_pdev);",
    ):
        require(htt_attach, fragment, "htt.c")
    tx_desc = texts["drivers/staging/qcacld-3.0/core/dp/txrx/ol_tx_desc.c"]
    require(tx_desc, "htt_tx_desc_num_frags(pdev->htt_pdev, tx_desc->htt_frag_desc,",
            "ol_tx_desc.c")
    fastpath = texts[
        "drivers/staging/qcacld-3.0/core/dp/txrx/ol_tx_ll_fastpath.c"
    ]
    require(fastpath,
            "qdf_mem_zero(tx_desc->htt_frag_desc, sizeof(struct msdu_ext_desc_t));",
            "ol_tx_ll_fastpath.c")

    services = texts["drivers/staging/fw-api/fw/htc_services.h"]
    require(services, "HTT_SERVICE_GROUP        = 3", "htc_services.h")
    require(services, "HTT_DATA_MSG_SVC  MAKE_SERVICE_ID(HTT_SERVICE_GROUP,0)",
            "htc_services.h")
    htc_wire = texts["drivers/staging/fw-api/fw/htc.h"]
    require(htc_wire, "#define HTC_CONNECT_FLAGS_DISABLE_CREDIT_FLOW_CTRL          (1 << 3)",
            "htc.h")
    for fragment in (
        "ControlBytes1 : 8",
        "reserved : 16",
    ):
        require(htc_wire, fragment, "htc.h")
    control_sequence_line = next(
        (line for line in htc_wire.splitlines()
         if "#define HTC_FRAME_HDR_CONTROLBYTES1_LSB" in line),
        "",
    )
    if not control_sequence_line or control_sequence_line.split()[-1] != "8":
        raise SystemExit("htc.h: HTC data-frame sequence byte offset changed")
    htt_data_rx = (ROOT / "rust/src/sunfish_qcacld_htc.rs").read_text()
    for fragment in (
        "parse_rx_data_frame(",
        "sequence: frame.control_bytes1",
        "credit_returns: frame.credit_returns()",
    ):
        require(htt_data_rx, fragment, "sunfish_qcacld_htc.rs")
    htc_codec = (ROOT / "rust/src/qcom_htc.rs").read_text()
    for fragment in (
        "HTC_FLAG_RECV_1MORE_BLOCK: u8 = 1 << 0",
        "HTC_FLAG_RECV_TRAILER: u8 = 1 << 1",
        "HTC_FLAG_NEED_CREDIT_UPDATE: u8 = 1",
        "if flags & !HTC_FLAG_RECV_TRAILER != 0",
        "flags & !HTC_FLAG_NEED_CREDIT_UPDATE",
    ):
        require(htc_codec, fragment, "qcom_htc.rs")
    htc_services = texts["drivers/staging/qca-wifi-host-cmn/htc/htc_services.c"]
    for fragment in (
        "unsigned int htc_credit_flow = 1;",
        "htc_get_credit_allocation(target,",
        "ConnectionFlags & ~HTC_SET_RECV_ALLOC_MASK",
        "HTC_CONNECT_FLAGS_SET_RECV_ALLOCATION(txAlloc)",
        "pEndpoint->service_id = pConnectReq->service_id;",
        "pEndpoint->TxCreditFlowEnabled = false;",
    ):
        require(htc_services, fragment, "htc_services.c")
    htc_core = texts["drivers/staging/qca-wifi-host-cmn/htc/htc.c"]
    for fragment in (
        "target->wmi_ep_count = 1;",
        "target->TotalTransmitCredits =",
        "target->wmi_ep_count > max_wmi_svc",
        "credits = credits / target->wmi_ep_count;",
        "pEntry->service_id = svc_id[i];",
        "pEntry->CreditAllocation = credits;",
    ):
        require(htc_core, fragment, "htc.c")
    if "WMI_CONTROL_SVC, WMI_CONTROL_SVC_WMAC1" not in htc_core:
        raise SystemExit("htc.c: WMI_CONTROL_SVC must remain the first allocated service")
    staging = KERNEL / "drivers/staging"
    endpoint_count_callers = []
    for source_path in staging.rglob("*"):
        if not source_path.is_file() or source_path.suffix not in (".c", ".h"):
            continue
        if source_path.name in ("htc.c", "htc_api.h", "htc_services.c"):
            continue
        source_text = source_path.read_text(errors="replace")
        if "htc_set_wmi_endpoint_count(" in source_text:
            endpoint_count_callers.append(str(source_path.relative_to(KERNEL)))
        if "htc_global_credit_flow_disable(" in source_text:
            raise SystemExit(
                "unexpected HTC credit-flow disable call: "
                + str(source_path.relative_to(KERNEL))
            )
    if endpoint_count_callers:
        raise SystemExit(
            "unexpected WMI endpoint-count override: " + ", ".join(endpoint_count_callers)
        )
    require(htc_codec, "HTC_CONNECT_RECV_ALLOC_SHIFT: u16 = 8", "qcom_htc.rs")
    require(htc_codec, "initial_wmi_credit_allocation()?", "qcom_htc.rs")
    require(htc_codec, "u16::from(receive_allocation) << HTC_CONNECT_RECV_ALLOC_SHIFT",
            "qcom_htc.rs")
    wmi_control = (ROOT / "rust/src/sunfish_qcacld_wmi_control.rs").read_text()
    for fragment in (
        "pub struct SunfishWmiControlSession",
        "WmiControlPhase::AwaitConnectResponse",
        "WmiControlPhase::Started",
        "pub fn stage_setup_complete(",
    ):
        require(wmi_control, fragment, "sunfish_qcacld_wmi_control.rs")
    htc_service_ids = texts["drivers/staging/fw-api/fw/htc_services.h"]
    for fragment in (
        "RSVD_SERVICE_GROUP       = 0",
        "#define HTC_CTRL_RSVD_SVC MAKE_SERVICE_ID(RSVD_SERVICE_GROUP,1)",
        "HTT_SERVICE_GROUP        = 3",
        "#define HTT_DATA_MSG_SVC  MAKE_SERVICE_ID(HTT_SERVICE_GROUP,0)",
    ):
        require(htc_service_ids, fragment, "htc_services.h")
    htc_send = texts["drivers/staging/qca-wifi-host-cmn/htc/htc_send.c"]
    for fragment in (
        "SM(ep_id, HTC_FRAME_HDR_ENDPOINTID)",
        "SM(pEndpoint->SeqNo, HTC_FRAME_HDR_CONTROLBYTES1)",
        "hif_send_head(target->hif_dev,",
    ):
        require(htc_send, fragment, "htc_send.c")
    pipes = texts[
        "drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_main.c"
    ]
    require(pipes, "case TARGET_TYPE_ADRASTEA:", "ce_main.c")
    require(pipes, "target_service_to_ce_map_wlan;", "ce_main.c")
    require(pipes, "HTT_DATA_MSG_SVC,\n\t\tPIPEDIR_OUT,    /* out = UL = host -> target */\n\t\t4,",
            "ce_main.c")
    require(pipes, "HTT_DATA_MSG_SVC,\n\t\tPIPEDIR_IN,     /* in = DL = target -> host */\n\t\t1,",
            "ce_main.c")
    require(pipes,
            "HTC_CTRL_RSVD_SVC,\n\t\tPIPEDIR_OUT,    /* out = UL = host -> target */\n\t\t0,",
            "ce_main.c")
    require(pipes,
            "HTC_CTRL_RSVD_SVC,\n\t\tPIPEDIR_IN,     /* in = DL = target -> host */\n\t\t2,",
            "ce_main.c")

    wma = texts["drivers/staging/qcacld-3.0/core/wma/src/wma_main.c"]
    wma_pre_start = wma[wma.index("QDF_STATUS wma_pre_start(void)"):]
    wma_pre_start = wma_pre_start[:wma_pre_start.index("\n}")]
    wmi_connect_at = wma_pre_start.index("wmi_unified_connect_htc_service(")
    cfg_download_at = wma_pre_start.index("WNI_CFG_DNLD_REQ", wmi_connect_at)
    if wmi_connect_at >= cfg_download_at:
        raise SystemExit("wma_pre_start: WMI control service must connect before CFG download")

    cds = texts["drivers/staging/qcacld-3.0/core/cds/src/cds_api.c"]
    dp_open = cds[cds.index("QDF_STATUS cds_dp_open("):]
    dp_open = dp_open[:dp_open.index("\n}")]
    require(dp_open, "cdp_pdev_attach(", "cds_api.c")
    pre_enable = cds[cds.index("QDF_STATUS cds_pre_enable("):]
    pre_enable = pre_enable[:pre_enable.index("\n}")]
    order = [pre_enable.index(token) for token in (
        "wma_pre_start()",
        "htc_start(gp_cds_context->htc_ctx)",
        "wma_wait_for_ready_event(",
        "cdp_pdev_post_attach(",
    )]
    if order != sorted(order):
        raise SystemExit("cds_pre_enable: WMI connect, HTC start, READY wait, and post-attach order changed")

    hdd_main = texts["drivers/staging/qcacld-3.0/core/hdd/src/wlan_hdd_main.c"]
    open_at = hdd_main.index("status = cds_open(hdd_ctx->psoc);")
    dp_at = hdd_main.index("status = cds_dp_open(hdd_ctx->psoc);", open_at)
    pre_enable_at = hdd_main.index("status = cds_pre_enable();", dp_at)
    if not open_at < dp_at < pre_enable_at:
        raise SystemExit("wlan_hdd_main.c: datapath attach no longer precedes WMI pre-enable")

    txrx = texts["drivers/staging/qcacld-3.0/core/dp/txrx/ol_txrx.c"]
    pdev_attach = txrx[txrx.index("ol_txrx_pdev_attach("):]
    pdev_attach = pdev_attach[:pdev_attach.index("\n}")]
    require(pdev_attach, "htt_pdev_alloc(", "ol_txrx.c")

    ce_service = texts[
        "drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_service.c"
    ]
    for fragment in (
        "dma_addr = qdf_nbuf_get_frag_paddr(msdu, 0);",
        "shadow_src_desc->nbytes = qdf_nbuf_get_frag_len(msdu, 0);",
        "shadow_src_desc->gather = 1;",
        "dma_addr = qdf_nbuf_get_frag_paddr(msdu, 1);",
        "shadow_src_desc->nbytes =  download_len;",
        "if (shadow_src_desc->nbytes > frag_len)",
        "shadow_src_desc->gather    = 0;",
    ):
        require(ce_service, fragment, "ce_service.c")

    ce_assignment = texts[
        "drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_assignment.h"
    ]
    require(ce_assignment,
            "static struct CE_attr host_ce_config_wlan_adrastea[]",
            "ce_assignment.h")
    require(ce_assignment,
            "{ /* CE1 */ CE_ATTR_FLAGS, 0, 0,  2048, 512, NULL,},",
            "ce_assignment.h")
    require(ce_assignment,
            "static struct CE_pipe_config target_ce_config_wlan_adrastea[]",
            "ce_assignment.h")
    require(ce_assignment,
            "{ /* CE1 */ 1, PIPEDIR_IN,  32, 2048, CE_ATTR_FLAGS, 0,},",
            "ce_assignment.h")
    require(ce_assignment, "#define CE_HTT_H2T_MSG_SRC_NENTRIES 2048",
            "ce_assignment.h")
    require(ce_assignment,
            "{ /* CE4 */ 4, PIPEDIR_OUT, 256, 256,\n\t\t(CE_ATTR_FLAGS | CE_ATTR_DISABLE_INTR), 0,},",
            "ce_assignment.h")

    print("Sunfish QCACLD WMI control connect advertises the READY credit allocation, uses the HTC control CE0/CE2 route, and precedes CFG download; HTT remains on CE4/CE1 with the pinned descriptor and compact-SNOC TX layout")


if __name__ == "__main__":
    main()
