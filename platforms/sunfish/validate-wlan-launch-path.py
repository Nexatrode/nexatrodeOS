#!/usr/bin/env python3
"""Audit Sunfish WLAN image launch and WLFW provider evidence in pinned Linux."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-linux-mainline-ref"
LINEAGE_KERNEL_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_LINEAGE_KERNEL_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"
LINEAGE_ICNSS_PATHS = (
    "drivers/soc/qcom/icnss.c",
    "drivers/soc/qcom/icnss_qmi.c",
)
LINEAGE_MODEM_STARTUP_PATHS = (
    "drivers/soc/qcom/subsystem_restart.c",
    "drivers/soc/qcom/subsys-pil-tz.c",
    "drivers/soc/qcom/peripheral-loader.c",
    "arch/arm64/boot/dts/qcom/atoll.dtsi",
)
PD_MAPPER_COMMIT = "5ecd2fe926aca7abfe40724177f63b942cff3947"
DEFAULT_PD_MAPPER_SOURCE = ROOT / ".cache/sunfish-pd-mapper-ref"
LINEAGE_DEVICE_COMMIT = "52074bf3c0b66efcba89f557e605535feb29aaab"
DEFAULT_LINEAGE_DEVICE_SOURCE = ROOT / ".cache/lineage-device-sunfish-23.2"
LINEAGE_DEVICE_PATHS = (
    "init.hardware.rc",
    "init.mdm.sh",
    "init.insmod.sunfish.cfg",
    "init.qcom.modem_links.sh",
    "BoardConfig-common.mk",
    "device.mk",
    "device-common.mk",
    "device-sunfish.mk",
    "aosp_sunfish.mk",
    "sunfish/init.sunfish.rc",
    "sunfish/proprietary-files-vendor.txt",
)
AUDITED_PATHS = (
    "drivers/soc/qcom/qcom_pd_mapper.c",
    "drivers/net/wireless/ath/ath10k/qmi.c",
    "drivers/net/wireless/ath/ath10k/qmi_wlfw_v01.h",
    "drivers/net/wireless/ath/ath10k/qmi_wlfw_v01.c",
    "drivers/net/wireless/ath/ath10k/wmi.c",
    "drivers/net/wireless/ath/ath10k/wmi.h",
    "arch/arm64/boot/dts/qcom/sm7150.dtsi",
    "arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts",
    "drivers/net/wireless/ath/ath10k/hw.h",
    "drivers/net/wireless/ath/ath10k/core.c",
    "drivers/net/wireless/ath/ath10k/core.h",
    "drivers/net/wireless/ath/ath10k/mac.c",
    "drivers/net/wireless/ath/ath10k/htc.h",
    "drivers/net/wireless/ath/ath10k/htc.c",
    "drivers/net/wireless/ath/ath10k/ce.h",
    "drivers/net/wireless/ath/ath10k/ce.c",
    "drivers/remoteproc/qcom_q6v5_pas.c",
    "drivers/firmware/qcom/qcom_scm.c",
    "drivers/soc/qcom/apr.c",
    "Documentation/devicetree/bindings/soc/qcom/qcom,apr-services.yaml",
    "drivers/soc/qcom/pdr_interface.c",
)


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(source), *args], text=True
    )


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def section(text: str, start: str, end: str) -> str:
    begin = text.index(start)
    finish = text.index(end, begin)
    return text[begin:finish]


def init_service(text: str, name: str) -> str:
    """Return one Android init service block without adjacent triggers."""
    blocks = re.split(r"(?m)(?=^(?:service|on|import)\s)", text)
    prefix = f"service {name} "
    for block in blocks:
        if block.startswith(prefix):
            return block
    raise SystemExit(f"FAIL: Android init service not found: {name}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lineage-kernel-source", type=Path, default=DEFAULT_LINEAGE_KERNEL_SOURCE)
    parser.add_argument("--pd-mapper-source", type=Path, default=DEFAULT_PD_MAPPER_SOURCE)
    parser.add_argument("--lineage-device-source", type=Path, default=DEFAULT_LINEAGE_DEVICE_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    lineage_kernel_source = args.lineage_kernel_source.resolve()
    pd_mapper_source = args.pd_mapper_source.resolve()
    lineage_device_source = args.lineage_device_source.resolve()
    if not source.is_dir():
        raise SystemExit(f"FAIL: source tree not found: {source}")
    if not lineage_kernel_source.is_dir():
        raise SystemExit(f"FAIL: LineageOS kernel source tree not found: {lineage_kernel_source}")
    if not pd_mapper_source.is_dir():
        raise SystemExit(f"FAIL: pd-mapper source tree not found: {pd_mapper_source}")
    if not lineage_device_source.is_dir():
        raise SystemExit(f"FAIL: LineageOS Sunfish device source not found: {lineage_device_source}")

    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")
    lineage_kernel_commit = git(lineage_kernel_source, "rev-parse", "HEAD").strip()
    require(lineage_kernel_commit == LINEAGE_KERNEL_COMMIT,
            f"expected LineageOS kernel {LINEAGE_KERNEL_COMMIT}, got {lineage_kernel_commit}")
    pd_mapper_commit = git(pd_mapper_source, "rev-parse", "HEAD").strip()
    require(pd_mapper_commit == PD_MAPPER_COMMIT,
            f"expected pd-mapper {PD_MAPPER_COMMIT}, got {pd_mapper_commit}")
    lineage_device_commit = git(lineage_device_source, "rev-parse", "HEAD").strip()
    require(lineage_device_commit == LINEAGE_DEVICE_COMMIT,
            f"expected LineageOS device tree {LINEAGE_DEVICE_COMMIT}, got {lineage_device_commit}")
    result = subprocess.run(
        ["git", "-C", str(pd_mapper_source), "diff", "--quiet", "HEAD", "--", "pd-mapper.c"],
        check=False,
    )
    require(result.returncode == 0, "audited userspace pd-mapper source has local edits")

    for path in LINEAGE_DEVICE_PATHS:
        result = subprocess.run(
            ["git", "-C", str(lineage_device_source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(result.returncode == 0, f"audited LineageOS device source has local edits: {path}")

    for path in AUDITED_PATHS:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(result.returncode == 0, f"audited source file has local edits: {path}")

    for path in LINEAGE_ICNSS_PATHS:
        result = subprocess.run(
            ["git", "-C", str(lineage_kernel_source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(result.returncode == 0, f"audited LineageOS ICNSS source has local edits: {path}")

    for path in LINEAGE_MODEM_STARTUP_PATHS:
        result = subprocess.run(
            ["git", "-C", str(lineage_kernel_source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(result.returncode == 0, f"audited LineageOS modem startup source has local edits: {path}")

    mapper = git(source, "show", f"HEAD:{AUDITED_PATHS[0]}")
    pas_source = git(source, "show", f"HEAD:{AUDITED_PATHS[16]}")
    scm_source = git(source, "show", f"HEAD:{AUDITED_PATHS[17]}")
    pd_mapper = git(pd_mapper_source, "show", "HEAD:pd-mapper.c")
    lineage_init = git(lineage_device_source, "show", "HEAD:init.hardware.rc")
    lineage_mdm_sh = git(lineage_device_source, "show", "HEAD:init.mdm.sh")
    lineage_insmod = git(lineage_device_source, "show", "HEAD:init.insmod.sunfish.cfg")
    lineage_links = git(lineage_device_source, "show", "HEAD:init.qcom.modem_links.sh")
    lineage_board = git(lineage_device_source, "show", "HEAD:BoardConfig-common.mk")
    lineage_device_mk = git(lineage_device_source, "show", "HEAD:device.mk")
    lineage_device_common_mk = git(lineage_device_source, "show", "HEAD:device-common.mk")
    lineage_device_sunfish_mk = git(lineage_device_source, "show", "HEAD:device-sunfish.mk")
    lineage_aosp_sunfish_mk = git(lineage_device_source, "show", "HEAD:aosp_sunfish.mk")
    lineage_vendor_blobs = git(
        lineage_device_source,
        "show",
        "HEAD:sunfish/proprietary-files-vendor.txt",
    )
    lineage_sunfish_init = git(lineage_device_source, "show", "HEAD:sunfish/init.sunfish.rc")
    qmi = git(source, "show", f"HEAD:{AUDITED_PATHS[1]}")
    wlfw_header = git(source, "show", f"HEAD:{AUDITED_PATHS[2]}")
    wlfw_codec = git(source, "show", f"HEAD:{AUDITED_PATHS[3]}")
    wmi = git(source, "show", f"HEAD:{AUDITED_PATHS[4]}")
    wmi_header = git(source, "show", f"HEAD:{AUDITED_PATHS[5]}")
    sm7150_dtsi = git(source, "show", f"HEAD:{AUDITED_PATHS[6]}")
    sunfish_dts = git(source, "show", f"HEAD:{AUDITED_PATHS[7]}")
    ath10k_hw = git(source, "show", f"HEAD:{AUDITED_PATHS[8]}")
    ath10k_core = git(source, "show", f"HEAD:{AUDITED_PATHS[9]}")
    ath10k_core_h = git(source, "show", f"HEAD:{AUDITED_PATHS[10]}")
    ath10k_mac = git(source, "show", f"HEAD:{AUDITED_PATHS[11]}")
    htc_header = git(source, "show", f"HEAD:{AUDITED_PATHS[12]}")
    htc = git(source, "show", f"HEAD:{AUDITED_PATHS[13]}")
    ce_header = git(source, "show", f"HEAD:{AUDITED_PATHS[14]}")
    ce = git(source, "show", f"HEAD:{AUDITED_PATHS[15]}")
    apr = git(source, "show", f"HEAD:{AUDITED_PATHS[18]}")
    apr_binding = git(source, "show", f"HEAD:{AUDITED_PATHS[19]}")
    pdr_client = git(source, "show", f"HEAD:{AUDITED_PATHS[20]}")
    icnss = git(lineage_kernel_source, "show", f"HEAD:{LINEAGE_ICNSS_PATHS[0]}")
    icnss_qmi = git(lineage_kernel_source, "show", f"HEAD:{LINEAGE_ICNSS_PATHS[1]}")
    lineage_restart = git(
        lineage_kernel_source,
        "show",
        f"HEAD:{LINEAGE_MODEM_STARTUP_PATHS[0]}",
    )
    lineage_pil_tz = git(
        lineage_kernel_source,
        "show",
        f"HEAD:{LINEAGE_MODEM_STARTUP_PATHS[1]}",
    )
    lineage_pil_loader = git(
        lineage_kernel_source,
        "show",
        f"HEAD:{LINEAGE_MODEM_STARTUP_PATHS[2]}",
    )
    lineage_atoll = git(
        lineage_kernel_source,
        "show",
        f"HEAD:{LINEAGE_MODEM_STARTUP_PATHS[3]}",
    )

    # Pin the public root-MPSS startup mechanism separately from the private
    # process that invokes it. The /dev/subsys_modem character-device open is
    # one concrete path into SSR/PIL; source order alone cannot attribute a
    # captured runtime open to modem_svc, pm-proxy, or another caller.
    lineage_char_add = section(
        lineage_restart,
        "static int subsys_char_device_add(",
        "static void subsys_char_device_remove(",
    )
    lineage_subsys_open = section(
        lineage_restart,
        "static int subsys_device_open(",
        "static int subsys_device_close(",
    )
    lineage_subsystem_get = section(
        lineage_restart,
        "void *__subsystem_get(",
        "/**\n * subsytem_get()",
    )
    lineage_subsystem_get_fwname = section(
        lineage_restart,
        "void *subsystem_get_with_fwname(",
        "EXPORT_SYMBOL(subsystem_get_with_fwname);",
    )
    lineage_firmware_name_store = section(
        lineage_restart,
        "static ssize_t firmware_name_store(",
        "static DEVICE_ATTR_RW(firmware_name);",
    )
    lineage_subsys_start = section(
        lineage_restart,
        "static int subsys_start(",
        "static void subsys_stop(",
    )
    lineage_pil_powerup = section(
        lineage_pil_tz,
        "static int subsys_powerup(",
        "static int subsys_ramdump(",
    )
    lineage_pil_probe = section(
        lineage_pil_tz,
        "static int pil_tz_driver_probe(",
        "static int pil_tz_driver_exit(",
    )
    lineage_pil_boot = section(
        lineage_pil_loader,
        "int pil_boot(struct pil_desc *desc)",
        "EXPORT_SYMBOL(pil_boot);",
    )
    lineage_modem_node = section(
        lineage_atoll,
        "pil_modem: qcom,mss@4080000 {",
        "\n\t};",
    )
    open_to_start = (
        "subsystem_get_with_fwname(subsys_dev->desc->name" in lineage_subsys_open
        and "return __subsystem_get(name, fw_name);" in lineage_subsystem_get_fwname
        and "ret = subsys_start(subsys);" in lineage_subsystem_get
        and "subsys->desc->powerup(subsys->desc)" in lineage_subsys_start
        and "d->desc.fw_name = subsys->fw_name;" in lineage_pil_powerup
        and "pil_boot(&d->desc)" in lineage_pil_powerup
    )
    pil_boot_steps = (
        'request_firmware(&fw, fw_name, desc->dev)',
        "pil_proxy_vote(desc)",
        "desc->ops->init_image(desc, fw->data, fw->size)",
        "desc->ops->mem_setup(desc, priv->region_start",
        "pil_load_segs(desc)",
        "desc->ops->auth_and_reset(desc)",
    )
    pil_boot_positions = [lineage_pil_boot.find(step) for step in pil_boot_steps]
    require(
        '"subsys_%s"' in lineage_char_add
        and "subsys_dev->desc->name" in lineage_char_add
        and open_to_start
        and '.name = d->desc.name;' in lineage_pil_probe
        and 'of_property_read_string(pdev->dev.of_node, "qcom,firmware-name"' in lineage_pil_probe
        and 'qcom,firmware-name = "modem";' in lineage_modem_node
        and 'pr_info("Changing subsys fw_name to %s\\n", fw_name);' in lineage_subsystem_get
        and 'pr_info("Changing subsys fw_name to %s\\n", buf);' in lineage_firmware_name_store,
        "public Lineage /dev/subsys_modem open-to-PIL startup path changed",
    )
    require(
        all(position >= 0 for position in pil_boot_positions)
        and pil_boot_positions == sorted(pil_boot_positions)
        and 'snprintf(fw_name, sizeof(fw_name), "%s.mdt", desc->fw_name);' in lineage_pil_boot,
        "public Lineage PIL boot no longer requests, loads, authenticates, and starts the configured MDT in order",
    )

    # Upstream qcom_pd_mapper is the public service-location map. It builds a
    # static service/domain table and answers locator queries; it is not the
    # private Android PD loader and must not be treated as a launch API.
    mapper_lookup = section(
        mapper,
        "static void qcom_pdm_get_domain_list(",
        "static void qcom_pdm_pfr(",
    )
    mapper_pfr = section(
        mapper,
        "static void qcom_pdm_pfr(",
        "static const struct qmi_msg_handler qcom_pdm_msg_handlers[]",
    )
    mapper_start = section(
        mapper,
        "static struct qcom_pdm_data *qcom_pdm_start(",
        "static int qcom_pdm_probe(",
    )
    require("qcom_pdm_find(data, req->service_name)" in mapper_lookup
            and "qmi_send_response(qmi, sq, txn, SERVREG_GET_DOMAIN_LIST_REQ" in mapper_lookup,
            "public qcom_pd_mapper no longer serves the static service-location map")
    require("qmi_send_response(qmi, sq, txn, SERVREG_LOC_PFR_REQ" in mapper_pfr
            and "qmi_send_request" not in mapper_pfr,
            "public qcom_pd_mapper PFR handler no longer only acknowledges the report")
    require("of_machine_get_match(qcom_pdm_domains)" in mapper_start
            and "qcom_pdm_add_domain(data, domains[i])" in mapper_start
            and "qmi_add_server(&data->handle, QMI_SERVICE_ID_SERVREG_LOC" in mapper_start,
            "public qcom_pd_mapper no longer initializes a static map and locator server")
    require("remoteproc" not in mapper and "qcom_scm_pas_auth_and_reset" not in mapper,
            "public qcom_pd_mapper acquired an apparent PD launch/auth path; review it")

    # Public Sunfish Linux starts the root MPSS modem through q6v5 PAS. This
    # is a known root-subsystem boot path, not a wlan_pd child-domain launcher.
    mpss_data = section(
        pas_source,
        "static const struct qcom_pas_data mpss_resource_init =",
        "static const struct qcom_pas_data sc8180x_mpss_resource =",
    )
    pas_match = section(
        pas_source,
        "static const struct of_device_id qcom_pas_of_match[] =",
        "MODULE_DEVICE_TABLE(of, qcom_pas_of_match);",
    )
    pas_start = section(
        pas_source,
        "static int qcom_pas_start(struct rproc *rproc)",
        "static void qcom_pas_handover(",
    )
    scm_pas_start = section(
        scm_source,
        "int qcom_scm_pas_prepare_and_auth_reset(",
        "EXPORT_SYMBOL_GPL(qcom_scm_pas_prepare_and_auth_reset);",
    )
    mpss_dts = section(sunfish_dts, "&remoteproc_mpss {", "};")
    pas_steps = (
        "qcom_q6v5_prepare(&pas->q6v5)",
        "qcom_mdt_pas_load(pas->pas_ctx",
        "qcom_pas_map_carveout(rproc, pas->mem_phys",
        "qcom_scm_pas_prepare_and_auth_reset(pas->pas_ctx)",
        "qcom_q6v5_wait_for_start(&pas->q6v5",
    )
    pas_positions = [pas_start.find(step) for step in pas_steps]
    require('.firmware_name = "modem.mdt"' in mpss_data
            and ".pas_id = 4" in mpss_data
            and ".auto_boot = false" in mpss_data
            and '.compatible = "qcom,sm7150-mpss-pas", .data = &mpss_resource_init' in pas_match
            and 'firmware-name = "qcom/sm7150/google/sunfish/modem.mbn";' in mpss_dts,
            "public Sunfish MPSS root-domain PAS identity or firmware mapping changed")
    require(all(position >= 0 for position in pas_positions)
            and pas_positions == sorted(pas_positions),
            "public MPSS PAS load/auth/start ordering changed")
    require("When Linux running @ EL1" in scm_source
            and "Gunyah hypervisor running @ EL2 traps the" in scm_source
            and "auth_and_reset call and create an shmbridge" in scm_source
            and "When Linux runs @ EL2 Linux must create the shmbridge itself" in scm_source
            and "if (!ctx->use_tzmem)" in scm_pas_start
            and "return qcom_scm_pas_auth_and_reset(ctx->pas_id);" in scm_pas_start
            and "qcom_tzmem_shm_bridge_create(ctx->mem_phys, ctx->mem_size, &handle)" in scm_pas_start
            and "qcom_tzmem_shm_bridge_delete(handle);" in scm_pas_start,
            "public PAS secure-memory bridge behavior or EL1/EL2 split changed")

    # The separate public userspace pd-mapper reads remoteproc firmware metadata
    # to locate JSON service maps, then publishes SERVREG. That lookup role is
    # distinct from launching/authenticating a protection-domain image.
    pd_mapper_enum = section(
        pd_mapper,
        "static int pd_enumerate_jsons(",
        "static int pd_load_maps(",
    )
    pd_mapper_load = section(
        pd_mapper,
        "static int pd_load_map(",
        "static int concat_path(",
    )
    pd_mapper_lookup = section(
        pd_mapper,
        "static void handle_get_domain_list(",
        "static int pd_load_map(",
    )
    pd_mapper_main = pd_mapper[pd_mapper.index("int main("):]
    pfr_dispatch = pd_mapper_main.split("case SERVREG_LOC_PFR:", 1)[1].split("break;", 1)[0]
    require('open("/sys/class/remoteproc", O_RDONLY | O_DIRECTORY)' in pd_mapper_enum
            and 'openat(class_fd, firmware_attr, O_RDONLY)' in pd_mapper_enum
            and 'read(firmware_fd, firmware_value' in pd_mapper_enum
            and '".jsn.xz"' in pd_mapper
            and '".jsn"' in pd_mapper,
            "public userspace pd-mapper no longer discovers JSON maps from remoteproc firmware metadata")
    require('json_get_child(root, "sr_domain")' in pd_mapper_load
            and 'json_get_child(root, "sr_service")' in pd_mapper_load
            and 'sprintf((char *)map->service, "%s/%s", provider, service)' in pd_mapper_load
            and 'sprintf((char *)map->domain, "%s/%s/%s", soc, domain, subdomain)' in pd_mapper_load,
            "public userspace pd-mapper map construction changed")
    require("!strcmp(pd_map->service, req.name)" in pd_mapper_lookup
            and "qmi_encode_message(&resp_buf" in pd_mapper_lookup
            and "qrtr_sendto(sock, pkt->node, pkt->port" in pd_mapper_lookup
            and "qrtr_publish(fd, SERVREG_QMI_SERVICE" in pd_mapper_main
            and "handle_get_domain_list(fd, &pkt)" in pd_mapper_main,
            "public userspace pd-mapper no longer publishes SERVREG and answers static domain lookups")
    require('printf("[PD-MAPPER] pfr\\n")' in pfr_dispatch
            and "qmi_send_request" not in pd_mapper
            and "qcom_scm_pas_auth_and_reset" not in pd_mapper
            and "rproc_boot(" not in pd_mapper
            and "remoteproc_set" not in pd_mapper,
            "public userspace pd-mapper PFR or launch behavior changed; review it")

    # The public LineageOS Sunfish init tree starts the generic modem/WLAN
    # support services and inserts wlan.ko, but does not disclose whether
    # opaque vendor daemons or the modem firmware launch wlan_pd. Keep that
    # distinction explicit: module insertion is not firmware startup.
    tftp_service = init_service(lineage_init, "tftp_server")
    modem_service = init_service(lineage_init, "modem_svc")
    mapper_service = init_service(lineage_init, "pd_mapper")
    cnss_service = init_service(lineage_init, "cnss-daemon")
    per_mgr_service = init_service(lineage_init, "vendor.per_mgr")
    per_proxy_service = init_service(lineage_init, "per_proxy")
    mdm_helper_service = init_service(lineage_sunfish_init, "vendor.mdm_helper")
    require(tftp_service.startswith("service tftp_server /vendor/bin/tftp_server")
            and "class core" in tftp_service,
            "LineageOS Sunfish TFTP service definition changed")
    require(modem_service.startswith("service modem_svc /vendor/bin/modem_svc -q")
            and "class core" in modem_service,
            "LineageOS Sunfish modem service definition changed")
    require(mapper_service.startswith("service pd_mapper /vendor/bin/pd-mapper")
            and "class core" in mapper_service,
            "LineageOS Sunfish PD-mapper service definition changed")
    require(cnss_service.startswith("service cnss-daemon /vendor/bin/cnss-daemon -n -l")
            and "class late_start" in cnss_service,
            "LineageOS Sunfish CNSS service definition changed")
    require(per_mgr_service.startswith("service vendor.per_mgr /vendor/bin/pm-service")
            and "class core" in per_mgr_service
            and "user system" in per_mgr_service,
            "LineageOS Sunfish Peripheral Manager service definition changed")
    require(per_proxy_service.startswith("service per_proxy /vendor/bin/pm-proxy")
            and "disabled" in per_proxy_service
            and "on property:init.svc.vendor.per_mgr=running\n    start per_proxy" in lineage_init,
            "LineageOS Sunfish Peripheral Manager proxy startup relationship changed")
    require('if [ "$baseband" = "mdm" ] || [ "$baseband" = "mdm2" ]; then' in lineage_mdm_sh
            and "start vendor.mdm_helper" in lineage_mdm_sh
            and mdm_helper_service.startswith("service vendor.mdm_helper /vendor/bin/mdm_helper")
            and "class core" in mdm_helper_service
            and "user root" in mdm_helper_service,
            "LineageOS Sunfish conditional modem-helper service declaration changed")
    public_product_mks = (
        lineage_device_mk + lineage_device_common_mk + lineage_device_sunfish_mk + lineage_aosp_sunfish_mk
    )
    require("init.mdm.sh" not in public_product_mks
            and "vendor/google_devices/sunfish/proprietary/device-vendor.mk" in lineage_aosp_sunfish_mk
            and "inherit-product-if-exists" in lineage_aosp_sunfish_mk,
            "public Sunfish makefiles changed how the MDM init script/vendor product is wired")
    insmod_lines = re.findall(r"(?m)^modprobe\|([^\n]+)$", lineage_insmod)
    require(any("wlan.ko" in line.split() for line in insmod_lines)
            and "BOARD_WLAN_DEVICE := qcwcn" in lineage_board
            and "WIFI_DRIVER_DEFAULT := qca_cld3" in lineage_board,
            "LineageOS Sunfish no longer configures and inserts the QCACLD WLAN module")
    require("android.hardware.wifi-service" in lineage_device_mk
            and "wpa_supplicant_overlay.conf" in lineage_device_mk,
            "LineageOS Sunfish Wi-Fi service/configuration packaging changed")
    public_launch_config = (
        lineage_init + lineage_mdm_sh + lineage_insmod + lineage_links + lineage_sunfish_init
    )
    require("wlan_pd" not in public_launch_config
            and "wlanmdsp.mbn" not in public_launch_config,
            "a direct wlan_pd or WLAN image trigger appeared in the audited public init files")
    require("# All blobs below are extracted from the release mentioned in proprietary-files.txt"
            in lineage_vendor_blobs
            and "vendor/bin/cnss-daemon" in lineage_vendor_blobs
            and "vendor/bin/modem_svc" in lineage_vendor_blobs
            and "vendor/bin/pm-service" in lineage_vendor_blobs
            and "vendor/bin/pm-proxy" in lineage_vendor_blobs
            and "vendor/firmware/wlanmdsp.mbn" in lineage_vendor_blobs,
            "LineageOS Sunfish no longer identifies CNSS/modem/image binaries as vendor blobs")

    # WMI service-ready memory requests must stay bounded to the fixed array
    # size and explicit overflow guard used by this pinned ath10k source.
    require("#define WMI_MAX_MEM_REQS 16" in wmi_header,
            "the pinned ath10k maximum host-memory request count changed")
    require("if (num_mem_reqs > WMI_MAX_MEM_REQS)" in wmi,
            "the pinned ath10k service-ready count guard changed")
    allocation = section(
        wmi,
        "static int ath10k_wmi_alloc_chunk",
        "static bool\nath10k_wmi_is_host_mem_allocated",
    )
    require("dma_alloc_coherent(ar->dev, pool_size" in allocation
            and "num_units * round_up(unit_len, 4)" in allocation,
            "ath10k host-memory allocation sizing or coherent allocator changed")
    chunk_schema = section(
        wmi_header,
        "struct host_memory_chunk {",
        "\n} __packed;",
    )
    schema_fields = (
        "__le32 req_id;",
        "__le32 ptr;",
        "__le32 size;",
    )
    field_positions = [chunk_schema.find(field) for field in schema_fields]
    require(all(position >= 0 for position in field_positions)
            and field_positions == sorted(field_positions),
            "the pinned WMI INIT chunk record layout changed")
    put_chunks = section(
        wmi,
        "void ath10k_wmi_put_host_mem_chunks",
        "\nstatic struct sk_buff *ath10k_wmi_op_gen_init",
    )
    require("chunks->count = __cpu_to_le32(ar->wmi.num_mem_chunks)" in put_chunks
            and "chunk->req_id = __cpu_to_le32(ar->wmi.mem_chunks[i].req_id)" in put_chunks
            and "chunk->ptr = __cpu_to_le32(ar->wmi.mem_chunks[i].paddr)" in put_chunks
            and "chunk->size = __cpu_to_le32(ar->wmi.mem_chunks[i].len)" in put_chunks,
            "the pinned WMI INIT host-memory response encoding changed")

    resource_config = section(
        wmi_header,
        "struct wmi_resource_config_10_4 {",
        "\n} __packed;",
    )
    resource_fields = re.findall(r"__le32\s+\w+\s*(?:\[(\d+)\])?\s*;", resource_config)
    resource_words = sum(int(count or "1") for count in resource_fields)
    require(resource_words == 42,
            f"WMI 10.4 resource config has {resource_words} words, expected 42")
    mem_chunks = section(
        wmi_header,
        "struct wmi_host_mem_chunks {",
        "\n} __packed;",
    )
    require("struct host_memory_chunk item;" in mem_chunks
            and "DECLARE_FLEX_ARRAY(struct host_memory_chunk, items);" in mem_chunks,
            "the one-item-minimum host-memory chunk ABI changed")
    init_10_4_struct = section(
        wmi_header,
        "struct wmi_init_cmd_10_4 {",
        "\n} __packed;",
    )
    require(
        init_10_4_struct.index("struct wmi_resource_config_10_4 resource_config;")
        < init_10_4_struct.index("struct wmi_host_mem_chunks mem_chunks;"),
        "the WMI 10.4 INIT body field order changed",
    )
    init_10_4 = section(
        wmi,
        "static struct sk_buff *ath10k_wmi_10_4_op_gen_init",
        "\nint ath10k_wmi_start_scan_verify",
    )
    require("struct_size(cmd, mem_chunks.items," in init_10_4
            and "ar->wmi.num_mem_chunks" in init_10_4
            and "memcpy(&cmd->resource_config, &config, sizeof(config));" in init_10_4
            and "ath10k_wmi_put_host_mem_chunks(ar, &cmd->mem_chunks);" in init_10_4,
            "the WMI 10.4 INIT body construction changed")
    skb_alloc = section(
        wmi,
        "struct sk_buff *ath10k_wmi_alloc_skb",
        "\nstatic void ath10k_wmi_htc_tx_complete",
    )
    require("memset(skb->data, 0, round_len);" in skb_alloc,
            "ath10k WMI command tail is no longer zero-initialized")

    # Check the generated WCN3990 WMI 10.4 resource words against the exact
    # upstream ABI order and the runtime/target choices made by core.c.
    resource_field_names = []
    for field, count in re.findall(r"__le32\s+(\w+)\s*(?:\[(\d+)\])?\s*;", resource_config):
        if count:
            resource_field_names.extend(f"{field}[{index}]" for index in range(int(count)))
        else:
            resource_field_names.append(field)
    rust_wmi = (ROOT / "rust/src/ath10k_wmi.rs").read_text()
    rust_resource_words = section(
        rust_wmi,
        "let words: [u32; WMI_10_4_RESOURCE_CONFIG_WORDS] = [",
        "\n        ];",
    )
    rust_resource_field_names = re.findall(
        r"//\s*([a-z][a-z0-9_]*(?:\[\d+\])?)", rust_resource_words
    )
    require(rust_resource_field_names == resource_field_names,
            "Rust WCN3990 resource-word order differs from the pinned 10.4 struct")
    require(
        "host_memory_units_for_hint(" in rust_wmi
        and "plan_host_memory_for_resource_config(" in rust_wmi
        and re.search(r"configured_count\s*\.checked_add\(1\)", rust_wmi)
        and "if active_peers == 0" in rust_wmi,
        "Rust host-memory planner no longer binds dynamic counts to WCN3990 INIT resources",
    )
    source_assignment_fields = []
    for field, index in re.findall(r"config\.(\w+)(?:\[(\d+)\])?\s*=", init_10_4):
        source_assignment_fields.append(f"{field}[{index}]" if index else field)
    require(len(source_assignment_fields) == len(resource_field_names)
            and set(source_assignment_fields) == set(resource_field_names),
            "pinned WMI 10.4 generator no longer assigns every resource word exactly once")
    required_target_definitions = (
        "#define TARGET_HL_TLV_NUM_PEERS\t\t\t33",
        "#define TARGET_10_4_NUM_VDEVS\t\t\t16",
        "#define TARGET_10_4_NUM_STATIONS\t\t32",
        "#define TARGET_10_4_ACTIVE_PEERS\t\t0",
        "#define TARGET_10_4_NUM_QCACHE_PEERS_MAX\t512",
        "#define TARGET_10_4_QCACHE_ACTIVE_PEERS\t\t50",
        "#define TARGET_10_4_QCACHE_ACTIVE_PEERS_PFC\t35",
        "#define TARGET_10_4_NUM_OFFLOAD_PEERS\t\t0",
        "#define TARGET_10_4_NUM_OFFLOAD_REORDER_BUFFS\t0",
        "#define TARGET_10_4_NUM_PEER_KEYS\t\t2",
        "#define TARGET_10_4_TGT_NUM_TIDS\t\t((TARGET_10_4_NUM_PEERS) * 2)",
        "#define TARGET_10_4_AST_SKID_LIMIT\t\t32",
        "#define TARGET_10_4_RX_TIMEOUT_LO_PRI\t\t100",
        "#define TARGET_10_4_RX_TIMEOUT_HI_PRI\t\t40",
        "#define TARGET_10_4_SCAN_MAX_REQS\t\t4",
        "#define TARGET_10_4_BMISS_OFFLOAD_MAX_VDEV\t3",
        "#define TARGET_10_4_ROAM_OFFLOAD_MAX_VDEV\t3",
        "#define TARGET_10_4_ROAM_OFFLOAD_MAX_PROFILES   8",
        "#define TARGET_10_4_TX_DBG_LOG_SIZE\t\t1024",
        "#define TARGET_10_4_NUM_WDS_ENTRIES\t\t32",
        "#define TARGET_10_4_DMA_BURST_SIZE\t\t1",
        "#define TARGET_10_4_RX_SKIP_DEFRAG_TIMEOUT_DUP_DETECTION_CHECK 1",
        "#define TARGET_10_4_GTK_OFFLOAD_MAX_VDEV\t3",
        "#define TARGET_10_4_NUM_MSDU_DESC\t\t(1024 + 400)",
        "#define TARGET_10_4_NUM_MSDU_DESC_PFC\t\t2500",
        "#define TARGET_10_4_11AC_TX_MAX_FRAGS\t\t2",
        "#define TARGET_10_4_MAX_PEER_EXT_STATS\t\t16",
        "#define TARGET_10_4_NUM_MCAST_GROUPS\t\t0",
        "#define TARGET_10_4_NUM_MCAST_TABLE_ELEMS\t0",
        "#define TARGET_10_4_MCAST2UCAST_MODE\t\t0",
        "#define TARGET_10_4_MAC_AGGR_DELIM\t\t0",
        "#define TARGET_10_4_VOW_CONFIG\t\t\t0",
        "#define TARGET_10_4_SMART_ANT_CAP\t\t0",
        "#define TARGET_10_4_BK_MIN_FREE\t\t\t0",
        "#define TARGET_10_4_BE_MIN_FREE\t\t\t0",
        "#define TARGET_10_4_VI_MIN_FREE\t\t\t0",
        "#define TARGET_10_4_VO_MIN_FREE\t\t\t0",
        "#define TARGET_10_4_RX_BATCH_MODE\t\t1",
        "#define TARGET_10_4_THERMAL_THROTTLING_CONFIG\t0",
        "#define TARGET_10_4_ATF_CONFIG\t\t\t0",
        "#define TARGET_10_4_IPHDR_PAD_CONFIG\t\t1",
        "#define TARGET_10_4_QWRAP_CONFIG\t\t0",
    )
    for definition in required_target_definitions:
        require(definition in ath10k_hw, f"WCN3990 resource-profile source changed: {definition}")
    wcn3990_hw = section(
        ath10k_core,
        ".id = WCN3990_HW_1_0_DEV_VERSION,",
        ".target_64bit = true,",
    )
    require(".tx_chain_mask = 0x7" in wcn3990_hw
            and ".rx_chain_mask = 0x7" in wcn3990_hw
            and ".num_peers = TARGET_HL_TLV_NUM_PEERS" in wcn3990_hw,
            "WCN3990 hardware peer/chain overrides changed")
    wmi_10_4_core = section(
        ath10k_core,
        "case ATH10K_FW_WMI_OP_VERSION_10_4:",
        "case ATH10K_FW_WMI_OP_VERSION_UNSET:",
    )
    for marker in (
        "ar->num_active_peers = TARGET_10_4_ACTIVE_PEERS;",
        "ar->max_num_vdevs = TARGET_10_4_NUM_VDEVS;",
        "ar->num_tids = TARGET_10_4_TGT_NUM_TIDS;",
        "ar->htt.max_num_pending_tx = TARGET_10_4_NUM_MSDU_DESC_PFC;",
        "ar->htt.max_num_pending_tx = TARGET_10_4_NUM_MSDU_DESC;",
    ):
        require(marker in wmi_10_4_core,
                f"WMI 10.4 runtime resource selection changed: {marker}")
    require("if (ar->hw_params.num_peers)" in ath10k_core
            and "ar->max_num_peers = ar->hw_params.num_peers;" in ath10k_core,
            "WCN3990 hardware peer override no longer takes precedence")
    require("ar->wmi.rx_decap_mode = ATH10K_HW_TXRX_NATIVE_WIFI;" in ath10k_core
            and "ATH10K_HW_TXRX_NATIVE_WIFI = 1" in ath10k_hw,
            "default native-Wi-Fi decapsulation selection changed")

    service_enum = section(wmi_header, "enum wmi_service {", "\n};")
    service_enum = re.sub(r"/\*.*?\*/|//[^\n]*", "", service_enum, flags=re.S)
    service_names = [
        entry.strip().split("=")[0].strip()
        for entry in service_enum.removeprefix("enum wmi_service {").split(",")
        if entry.strip()
    ]
    require(
        "WMI_SERVICE_PEER_CACHING" in service_names
        and service_names.index("WMI_SERVICE_PEER_CACHING") == 78,
        "Linux normalized WMI_SERVICE_PEER_CACHING index changed",
    )
    service_10_4_enum = section(wmi_header, "enum wmi_10_4_service {", "\n};")
    service_10_4_enum = re.sub(r"/\*.*?\*/|//[^\n]*", "", service_10_4_enum, flags=re.S)
    service_10_4_names = [
        entry.strip().split("=")[0].strip()
        for entry in service_10_4_enum.removeprefix("enum wmi_10_4_service {").split(",")
        if entry.strip()
    ]
    require(
        "WMI_10_4_SERVICE_PEER_CACHING" in service_10_4_names
        and service_10_4_names.index("WMI_10_4_SERVICE_PEER_CACHING") == 37,
        "raw WMI 10.4 peer-caching service bit changed",
    )
    service_map = section(wmi_header, "static inline void wmi_10_4_svc_map", "\n}\n")
    require(
        re.search(
            r"SVCMAP\(WMI_10_4_SERVICE_PEER_CACHING,\s*WMI_SERVICE_PEER_CACHING,\s*len\);",
            service_map,
        ) is not None,
        "raw WMI 10.4 peer-caching bit no longer maps to Linux service bit 78",
    )

    # WMI operation version 10.4 uses the fixed non-TLV event decoders. Keep
    # this separate from the WMI-TLV parser and validate its wire framing.
    wmi_10_4_ops = section(wmi, "static const struct wmi_ops wmi_10_4_ops = {", "\n};")
    require(
        ".pull_svc_rdy = ath10k_wmi_main_op_pull_svc_rdy_ev" in wmi_10_4_ops
        and ".pull_rdy = ath10k_wmi_op_pull_rdy_ev" in wmi_10_4_ops,
        "WMI 10.4 SERVICE_READY/READY decoder selection changed",
    )
    service_ready_struct = section(
        wmi_header, "struct wmi_service_ready_event {", "\n} __packed;"
    )
    required_service_ready_fields = (
        "__le32 wmi_service_bitmap[16];",
        "__le32 max_bcn_ie_size;",
        "__le32 num_mem_reqs;",
        "struct wlan_host_mem_req mem_reqs[];",
    )
    field_positions = [service_ready_struct.find(field) for field in required_service_ready_fields]
    direct_service_words = sum(
        int(count or "1")
        for count in re.findall(r"__le32\s+\w+\s*(?:\[(\d+)\])?\s*;", service_ready_struct)
    )
    hal_reg_struct = section(wmi_header, "struct hal_reg_capabilities {", "\n} __packed;")
    hal_reg_words = len(re.findall(r"__le32\s+\w+\s*;", hal_reg_struct))
    require(
        all(position >= 0 for position in field_positions)
        and field_positions == sorted(field_positions)
        and direct_service_words + hal_reg_words == 40,
        "fixed WMI SERVICE_READY bitmap/request field layout changed",
    )
    pull_svc_rdy = section(
        wmi,
        "ath10k_wmi_main_op_pull_svc_rdy_ev(struct ath10k *ar",
        "static int\nath10k_wmi_10x_op_pull_svc_rdy_ev",
    )
    require(
        "skb->len < sizeof(*ev)" in pull_svc_rdy
        and "arg->service_map = ev->wmi_service_bitmap;" in pull_svc_rdy
        and "arg->num_mem_reqs = ev->num_mem_reqs;" in pull_svc_rdy
        and "sizeof(arg->mem_reqs[0])" in pull_svc_rdy,
        "fixed WMI SERVICE_READY bounds/layout decoder changed",
    )
    ready_struct = section(wmi_header, "struct wmi_ready_event {", "\n} __packed;")
    mac_addr_struct = section(wmi_header, "struct wmi_mac_addr {", "\n} __packed;")
    require(
        ready_struct.index("__le32 sw_version;")
        < ready_struct.index("__le32 abi_version;")
        < ready_struct.index("struct wmi_mac_addr mac_addr;")
        < ready_struct.index("__le32 status;")
        and "u8 addr[6];" in mac_addr_struct
        and "u32 word1;" in mac_addr_struct,
        "fixed WMI READY event field order changed",
    )
    cmd_enum = section(wmi_header, "enum wmi_10_4_cmd_id {", "\n};")
    require(
        "WMI_10_4_START_CMDID = 0x9000," in cmd_enum
        and "WMI_10_4_END_CMDID = 0x9FFF,\n\tWMI_10_4_INIT_CMDID," in cmd_enum,
        "WMI 10.4 INIT command ID sequence changed",
    )
    cmd_header_struct = section(wmi_header, "struct wmi_cmd_hdr {", "\n};")
    require(
        "__le32 cmd_id;" in cmd_header_struct,
        "WMI command header width changed",
    )
    cmd_send = section(
        wmi,
        "int ath10k_wmi_cmd_send_nowait(struct ath10k *ar",
        "static void ath10k_wmi_tx_beacon_nowait",
    )
    require(
        "skb_push(skb, sizeof(struct wmi_cmd_hdr))" in cmd_send
        and "cmd_hdr->cmd_id = __cpu_to_le32(cmd);" in cmd_send,
        "WMI command header prefix or endian encoding changed",
    )
    require(
        "WMI_10_4_SERVICE_READY_FIXED_SIZE: usize = 160" in rust_wmi
        and "WMI_10_4_SERVICE_BITMAP_OFFSET: usize = 20" in rust_wmi
        and "WMI_10_4_SERVICE_READY_NUM_MEM_REQS_OFFSET: usize = 156" in rust_wmi
        and "WMI_10_4_READY_EVENT_FIXED_SIZE: usize = 20" in rust_wmi
        and "WMI_10_4_CMD_HEADER_SIZE: usize = 4" in rust_wmi
        and "WMI_10_4_INIT_CMD_ID: u32 = 0xa000" in rust_wmi
        and "WMI_10_4_SERVICE_PEER_CACHING_INDEX: usize = 37" in rust_wmi,
        "Rust WMI 10.4 event or command constants differ from pinned ABI",
    )
    service_ready_qcache = section(
        wmi,
        "if (test_bit(WMI_SERVICE_PEER_CACHING, ar->wmi.svc_map)) {",
        "/* TODO: Adjust max peer count",
    )
    for marker in (
        "TARGET_10_4_QCACHE_ACTIVE_PEERS_PFC +",
        "TARGET_10_4_QCACHE_ACTIVE_PEERS +",
        "TARGET_10_4_NUM_QCACHE_PEERS_MAX +",
        "ar->num_tids = ar->num_active_peers * 2;",
    ):
        require(marker in service_ready_qcache,
                f"SERVICE_READY peer-cache resource adjustment changed: {marker}")
    wmi_10_4_init = section(
        wmi,
        "static struct sk_buff *ath10k_wmi_10_4_op_gen_init(",
        "int ath10k_wmi_start_scan_verify(",
    )
    for marker in (
        "config.num_vdevs = __cpu_to_le32(ar->max_num_vdevs);",
        "config.num_peers = __cpu_to_le32(ar->max_num_peers);",
        "config.num_active_peers = __cpu_to_le32(ar->num_active_peers);",
        "config.num_tids = __cpu_to_le32(ar->num_tids);",
    ):
        require(marker in wmi_10_4_init,
                f"WMI 10.4 INIT no longer uses SERVICE_READY resource values: {marker}")
    host_memory_counts = section(
        wmi,
        "ath10k_wmi_free_host_mem(ar);",
        "ret = ath10k_wmi_alloc_host_mem(ar, req_id, num_units,",
    )
    for marker in (
        "num_units = ar->num_active_peers + 1;",
        "num_units = ar->max_num_peers + 1;",
        "num_units = ar->max_num_vdevs + 1;",
    ):
        require(marker in host_memory_counts,
                f"WMI host-memory dynamic-count rule changed: {marker}")

    # The active-scan codec is intentionally the WMI 10.4 command body, not
    # the separate WMI-TLV start-scan ABI. Pin the common words, command ID,
    # list order, list limits, and zero padding to ath10k's source.
    scan_common = section(
        wmi_header, "struct wmi_start_scan_common {", "\n} __packed;"
    )
    scan_common_fields = re.findall(r"__le32\s+(\w+)\s*;", scan_common)
    expected_scan_common_fields = [
        "scan_id", "scan_req_id", "vdev_id", "scan_priority",
        "notify_scan_events", "dwell_time_active", "dwell_time_passive",
        "min_rest_time", "max_rest_time", "repeat_probe_time",
        "probe_spacing_time", "idle_time", "max_scan_time", "probe_delay",
        "scan_ctrl_flags",
    ]
    require(scan_common_fields == expected_scan_common_fields,
            "WMI 10.4 common scan-command field order changed")
    require(
        "WMI_10_4_START_CMDID = 0x9000," in cmd_enum
        and "WMI_10_4_START_SCAN_CMDID = WMI_10_4_START_CMDID," in cmd_enum
        and ".start_scan_cmdid = WMI_10_4_START_SCAN_CMDID," in wmi,
        "WMI 10.4 start-scan command ID mapping changed",
    )
    wmi_10_4_ops = section(wmi, "static const struct wmi_ops wmi_10_4_ops = {", "\n};")
    require(
        ".gen_start_scan = ath10k_wmi_op_gen_start_scan," in wmi_10_4_ops,
        "WMI 10.4 start-scan generator selection changed",
    )
    scan_verify = section(
        wmi,
        "int ath10k_wmi_start_scan_verify(",
        "\nstatic size_t\nath10k_wmi_start_scan_tlvs_len",
    )
    for marker in (
        "arg->ie_len > WLAN_SCAN_PARAMS_MAX_IE_LEN",
        "arg->n_channels > ARRAY_SIZE(arg->channels)",
        "arg->n_ssids > WLAN_SCAN_PARAMS_MAX_SSID",
        "arg->n_bssids > WLAN_SCAN_PARAMS_MAX_BSSID",
    ):
        require(marker in scan_verify, f"ath10k scan request bound changed: {marker}")
    require(
        "#define WLAN_SCAN_PARAMS_MAX_SSID    16" in wmi_header
        and "#define WLAN_SCAN_PARAMS_MAX_BSSID   4" in wmi_header
        and "#define WLAN_SCAN_PARAMS_MAX_IE_LEN  256" in wmi_header
        and "u16 channels[64];" in wmi_header,
        "ath10k WMI 10.4 scan-list capacities changed",
    )
    require(
        "#define WMI_HOST_SCAN_REQ_ID_PREFIX 0xA000" in wmi_header
        and "#define WMI_HOST_SCAN_REQUESTOR_ID_PREFIX 0xA000" in wmi_header
        and "scan_id |= arg->scan_id;" in wmi
        and "scan_req_id |= arg->scan_req_id;" in wmi,
        "ath10k host scan ID/requestor prefix behavior changed",
    )
    scan_channel_entry = section(
        wmi_header, "struct wmi_chan_list_entry {", "\n} __packed;"
    )
    scan_ssid_entry = section(wmi_header, "struct wmi_ssid {", "\n} __packed;")
    scan_mac_address = section(wmi_header, "struct wmi_mac_addr {", "\n} __packed;")
    scan_ie_data = section(wmi_header, "struct wmi_ie_data {", "\n} __packed;")
    require(
        scan_channel_entry.index("__le16 freq;")
        < scan_channel_entry.index("u8 phy_mode;")
        < scan_channel_entry.index("u8 reserved;")
        and "__le32 ssid_len;" in scan_ssid_entry
        and "u8 ssid[32];" in scan_ssid_entry
        and "u8 addr[6];" in scan_mac_address
        and "u32 word1;" in scan_mac_address
        and "__le32 tag;" in scan_ie_data
        and "__le32 ie_len;" in scan_ie_data
        and "WMI_CHAN_LIST_TAG\t0x1" in wmi_header
        and "WMI_SSID_LIST_TAG\t0x2" in wmi_header
        and "WMI_BSSID_LIST_TAG\t0x3" in wmi_header
        and "WMI_IE_TAG\t\t0x4" in wmi_header,
        "WMI scan list entry layout or TLV tags changed",
    )
    scan_generator = section(
        wmi,
        "ath10k_wmi_op_gen_start_scan(struct ath10k *ar,",
        "\nstatic struct sk_buff *\nath10k_wmi_10x_op_gen_start_scan",
    )
    scan_tlvs = section(
        wmi,
        "static void\nath10k_wmi_put_start_scan_tlvs(",
        "\nstatic struct sk_buff *\nath10k_wmi_op_gen_start_scan",
    )
    scan_tlv_order = [
        scan_tlvs.index("if (arg->n_channels)"),
        scan_tlvs.index("if (arg->n_ssids)"),
        scan_tlvs.index("if (arg->n_bssids)"),
        scan_tlvs.index("if (arg->ie_len)"),
    ]
    require(
        scan_tlv_order == sorted(scan_tlv_order)
        and "WMI_CHAN_LIST_TAG" in scan_tlvs
        and "WMI_SSID_LIST_TAG" in scan_tlvs
        and "WMI_BSSID_LIST_TAG" in scan_tlvs
        and "WMI_IE_TAG" in scan_tlvs
        and "channels->channel_list[i].freq =" in scan_tlvs
        and "ssids->ssids[i].ssid_len =" in scan_tlvs
        and "ether_addr_copy(bssids->bssid_list[i].addr" in scan_tlvs
        and "memcpy(ie->ie_data, arg->ie, arg->ie_len);" in scan_tlvs,
        "WMI scan list serialization or source ordering changed",
    )
    require(
        "len = sizeof(*cmd) + ath10k_wmi_start_scan_tlvs_len(arg);" in scan_generator
        and "ath10k_wmi_put_start_scan_common(&cmd->common, arg);" in scan_generator
        and "ath10k_wmi_put_start_scan_tlvs(cmd->tlvs, arg);" in scan_generator
        and "cmd->burst_duration_ms = __cpu_to_le32(0);" in scan_generator
        and "struct wmi_start_scan_cmd {" in wmi_header
        and "__le32 burst_duration_ms;" in wmi_header,
        "WMI 10.4 scan body framing or burst-duration behavior changed",
    )
    rust_scan = (ROOT / "rust/src/ath10k_scan.rs").read_text()
    fixed_scan_words = section(rust_scan, "for value in [", "] {")
    expected_rust_scan_values = (
        "HOST_SCAN_ID_PREFIX | u32::from(request.scan_id)",
        "HOST_SCAN_ID_PREFIX | u32::from(request.requestor_id)",
        "request.vdev_id", "request.priority", "request.notify_scan_events",
        "request.active_dwell_ms", "request.passive_dwell_ms",
        "request.min_rest_ms", "request.max_rest_ms", "request.repeat_probe_ms",
        "request.probe_spacing_ms", "request.idle_ms", "request.max_scan_ms",
        "request.probe_delay_ms", "request.scan_control_flags", "0,",
    )
    rust_scan_positions = [fixed_scan_words.find(value) for value in expected_rust_scan_values]
    require(
        all(position >= 0 for position in rust_scan_positions)
        and rust_scan_positions == sorted(rust_scan_positions)
        and "pub const WMI_10_4_START_SCAN_COMMAND_ID: u32 = 0x9000;" in rust_scan
        and "pub const MAX_SCAN_CHANNELS: usize = 64;" in rust_scan
        and "pub const MAX_SCAN_SSIDS: usize = 16;" in rust_scan
        and "pub const MAX_SCAN_BSSIDS: usize = 4;" in rust_scan
        and "pub const MAX_SCAN_IE_BYTES: usize = 256;" in rust_scan
        and "pub const MAX_SSID_BYTES: usize = 32;" in rust_scan
        and "command.fill(0);" in rust_scan
        and "offset += MAC_ADDRESS_BYTES; // Two word-padding bytes remain zero." in rust_scan,
        "Rust bounded scan encoder no longer matches the pinned WMI 10.4 wire format",
    )
    print("PASS: bounded Rust scan-command body matches pinned ath10k WMI 10.4 ABI")

    scan_defaults = section(
        wmi,
        "void ath10k_wmi_start_scan_init(struct ath10k *ar,",
        "\nstatic struct sk_buff *\nath10k_wmi_op_gen_stop_scan",
    )
    linux_scan_path = section(
        ath10k_mac,
        "static int ath10k_hw_scan(struct ieee80211_hw *hw,",
        "\nstatic void ath10k_cancel_hw_scan",
    )
    require(
        "#define ATH10K_SCAN_ID 0" in ath10k_core_h
        and "arg->scan_req_id = 1;" in scan_defaults
        and "arg->scan_priority = WMI_SCAN_PRIORITY_LOW;" in scan_defaults
        and "WMI_SCAN_PRIORITY_VERY_LOW = 0,\n\tWMI_SCAN_PRIORITY_LOW," in wmi_header
        and "arg->dwell_time_active = 50;" in scan_defaults
        and "arg->dwell_time_passive = 150;" in scan_defaults
        and "arg->min_rest_time = 50;" in scan_defaults
        and "arg->max_rest_time = 500;" in scan_defaults
        and "arg->repeat_probe_time = 0;" in scan_defaults
        and "arg->probe_spacing_time = 0;" in scan_defaults
        and "arg->idle_time = 0;" in scan_defaults
        and "arg->max_scan_time = 20000;" in scan_defaults
        and "arg->probe_delay = 5;" in scan_defaults
        and "WMI_SCAN_EVENT_FOREIGN_CHANNEL_EXIT" in scan_defaults
        and "WMI_SCAN_EVENT_DEQUEUED" in scan_defaults
        and "arg->scan_ctrl_flags |= WMI_SCAN_CHAN_STAT_EVENT;" in scan_defaults
        and "arg->n_bssids = 1;" in scan_defaults
        and '\\xFF\\xFF\\xFF\\xFF\\xFF\\xFF' in scan_defaults
        and "arg->scan_id = ATH10K_SCAN_ID;" in linux_scan_path
        and "arg->scan_ctrl_flags |= WMI_SCAN_FLAG_PASSIVE;" in linux_scan_path,
        "Linux ath10k scan defaults or empty-SSID passive policy changed",
    )
    require(
        "pub const WMI_SCAN_FLAG_PASSIVE: u32 = 0x01;" in rust_scan
        and "pub const WMI_SCAN_CHAN_STAT_EVENT: u32 = 0x10;" in rust_scan
        and "pub const LINUX_SCAN_REQUESTOR_ID: u16 = 1;" in rust_scan
        and "pub const LINUX_SCAN_ID: u16 = 0;" in rust_scan
        and "pub const LINUX_SCAN_PRIORITY_LOW: u32 = 1;" in rust_scan
        and "pub const fn linux_reference_defaults(" in rust_scan
        and "if ssids.is_empty()" in rust_scan
        and "bssids: &BROADCAST_BSSID_LIST" in rust_scan,
        "Rust Linux-reference scan profile no longer matches the pinned ath10k defaults",
    )
    print("PASS: scan defaults and passive-empty-SSID policy match pinned ath10k")

    scan_started = section(
        wmi,
        "static void ath10k_wmi_event_scan_started(struct ath10k *ar)",
        "\nstatic void ath10k_wmi_event_scan_start_failed",
    )
    scan_start_failed = section(
        wmi,
        "static void ath10k_wmi_event_scan_start_failed(struct ath10k *ar)",
        "\nstatic void ath10k_wmi_event_scan_completed",
    )
    scan_completed = section(
        wmi,
        "static void ath10k_wmi_event_scan_completed(struct ath10k *ar)",
        "\nstatic void ath10k_wmi_event_scan_bss_chan",
    )
    scan_finish = section(
        ath10k_mac,
        "void __ath10k_scan_finish(struct ath10k *ar)",
        "\nvoid ath10k_scan_finish(struct ath10k *ar)",
    )
    require(
        "case ATH10K_SCAN_STARTING:" in scan_started
        and "ar->scan.state = ATH10K_SCAN_RUNNING;" in scan_started
        and "case ATH10K_SCAN_STARTING:" in scan_start_failed
        and "__ath10k_scan_finish(ar);" in scan_start_failed
        and "case ATH10K_SCAN_RUNNING:" in scan_completed
        and "case ATH10K_SCAN_ABORTING:" in scan_completed
        and "case ATH10K_SCAN_STARTING:" in scan_completed
        and "ar->scan.state = ATH10K_SCAN_IDLE;" in scan_finish,
        "pinned ath10k scan lifecycle transitions changed",
    )
    submit_return = linux_scan_path.index("ret = ath10k_start_scan(ar, arg);")
    submit_error = linux_scan_path.index("if (ret) {", submit_return)
    state_reset = linux_scan_path.index("ar->scan.state = ATH10K_SCAN_IDLE;", submit_error)
    require(submit_return < submit_error < state_reset,
            "pinned ath10k no-submit-error path no longer returns the scan to idle")
    rust_scan_session = section(
        rust_scan,
        "pub struct ScanSession {",
        "impl Default for ScanSession",
    )
    require(
        "pub fn reserve_start(" in rust_scan_session
        and "pub fn submission_failed(" in rust_scan_session
        and "self.phase != ScanPhase::Starting" in rust_scan_session
        and "self.finish();" in rust_scan_session
        and "ScanPhase::Starting" in rust_scan_session
        and "ScanPhase::Running" in rust_scan_session
        and "ScanPhase::Aborting" in rust_scan_session
        and "event.requestor_id != self.requestor_id" in rust_scan_session
        and "event.scan_id != self.scan_id" in rust_scan_session
        and "event.vdev_id != self.vdev_id" in rust_scan_session
        and "WMI_SCAN_EVENT_START_FAILED if self.phase == ScanPhase::Starting" in rust_scan_session
        and "aborted = self.phase == ScanPhase::Aborting" in rust_scan_session,
        "Rust scan-session tracking lost its bounded lifecycle or event identity checks",
    )
    print("PASS: bounded scan-session transitions match ath10k and filter stale event identities")

    scan_event_struct = section(
        wmi_header, "struct wmi_scan_event {", "\n} __packed;"
    )
    require(
        re.findall(r"__le32\s+(\w+)\s*;", scan_event_struct)
        == ["event_type", "reason", "channel_freq", "scan_req_id", "scan_id", "vdev_id"],
        "ath10k WMI scan-event field order changed",
    )
    scan_pull = section(
        wmi,
        "static int ath10k_wmi_op_pull_scan_ev(",
        "int ath10k_wmi_event_scan(",
    )
    for marker in (
        "skb->len < sizeof(*ev)", "skb_pull(skb, sizeof(*ev));",
        "arg->event_type = ev->event_type;", "arg->reason = ev->reason;",
        "arg->channel_freq = ev->channel_freq;", "arg->scan_req_id = ev->scan_req_id;",
        "arg->scan_id = ev->scan_id;", "arg->vdev_id = ev->vdev_id;",
    ):
        require(marker in scan_pull, f"ath10k scan-event decode changed: {marker}")
    require(
        ".pull_scan = ath10k_wmi_op_pull_scan_ev," in wmi_10_4_ops
        and "WMI_10_4_SCAN_EVENTID = WMI_10_4_START_EVENTID," in wmi_header,
        "WMI 10.4 scan-event selection or ID changed",
    )
    rust_scan_event_markers = (
        "pub const WMI_SCAN_EVENT_BYTES: usize = 24;",
        "pub const WMI_SCAN_EVENT_STARTED: u32 = 1 << 0;",
        "pub const WMI_SCAN_EVENT_FOREIGN_CHANNEL_EXIT: u32 = 1 << 8;",
        "pub fn parse_scan_event(input: &[u8]) -> Result<ScanEvent, Error>",
        "pub event_type: u32,",
        "pub const fn kind(self) -> Option<ScanEventKind>",
    )
    require(
        all(marker in rust_scan for marker in rust_scan_event_markers)
        and all(
            marker in rust_scan
            for marker in (
                "event_type: read_u32(input, 0)",
                "reason: read_u32(input, 4)",
                "channel_frequency_mhz: read_u32(input, 8)",
                "requestor_id: read_u32(input, 12)",
                "scan_id: read_u32(input, 16)",
                "vdev_id: read_u32(input, 20)",
            )
        )
        and "pub const WMI_10_4_MGMT_RX_HEADER_BYTES: usize = 28;"
        in (ROOT / "rust/src/ath10k_mgmt_rx.rs").read_text(),
        "Rust scan-event or management-RX parser ABI markers changed",
    )

    mgmt_rx_header = section(
        wmi_header, "struct wmi_10_4_mgmt_rx_hdr {", "\n} __packed;"
    )
    mgmt_rx_fields = re.findall(
        r"(?:__le32|u8)\s+(\w+)(?:\[(\d+)\])?\s*;", mgmt_rx_header
    )
    require(
        mgmt_rx_fields == [
            ("channel", ""), ("snr", ""), ("rssi_ctl", "4"),
            ("rate", ""), ("phy_mode", ""), ("buf_len", ""), ("status", ""),
        ],
        "WMI 10.4 management-RX header layout changed",
    )
    mgmt_rx_pull = section(
        wmi,
        "static int ath10k_wmi_10_4_op_pull_mgmt_rx_ev(",
        "static bool ath10k_wmi_rx_is_decrypted(",
    )
    for marker in (
        "pull_len = sizeof(*ev);", "skb->len < pull_len", "skb_pull(skb, pull_len);",
        "arg->channel = ev_hdr->channel;", "arg->buf_len = ev_hdr->buf_len;",
        "arg->status = ev_hdr->status;", "arg->snr = ev_hdr->snr;",
        "arg->phy_mode = ev_hdr->phy_mode;", "arg->rate = ev_hdr->rate;",
        "skb->len < msdu_len", "WMI_RX_STATUS_EXT_INFO", "ALIGN(le32_to_cpu(arg->buf_len), 4)",
        "sizeof(struct wmi_mgmt_rx_ext_info)", "skb_trim(skb, msdu_len);",
    ):
        require(marker in mgmt_rx_pull, f"ath10k 10.4 management-RX decode changed: {marker}")
    mgmt_rx_rust = (ROOT / "rust/src/ath10k_mgmt_rx.rs").read_text()
    require(
        ".pull_mgmt_rx = ath10k_wmi_10_4_op_pull_mgmt_rx_ev," in wmi_10_4_ops
        and "case WMI_10_4_MGMT_RX_EVENTID:" in section(
            wmi,
            "static void ath10k_wmi_10_4_op_rx(",
            "static const struct wmi_ops wmi_10_4_ops = {",
        )
        and "pub const WMI_RX_STATUS_EXT_INFO: u32 = 0x40;" in mgmt_rx_rust
        and "let extension_start = frame_start" in mgmt_rx_rust
        and "let aligned_length = round_up_four(frame_length)?;" in mgmt_rx_rust
        and "pub rssi_control: [u8; 4]," in mgmt_rx_rust
        and "frame: &input[frame_start..frame_end]," in mgmt_rx_rust,
        "Rust management-RX decoder no longer follows the pinned WMI 10.4 envelope",
    )
    for marker in (
        "channel_number: read_u32(input, 0)", "snr: read_u32(input, 4)",
        "[input[8], input[9], input[10], input[11]]", "rate: read_u32(input, 12)",
        "phy_mode: read_u32(input, 16)", "read_u32(input, 20)", "let status = read_u32(input, 24);",
    ):
        require(marker in mgmt_rx_rust, f"Rust management-RX field offset changed: {marker}")

    mgmt_rx_handler = section(
        wmi,
        "int ath10k_wmi_event_mgmt_rx(struct ath10k *ar, struct sk_buff *skb)",
        "static int freq_to_idx(",
    )
    for marker in (
        "test_bit(ATH10K_CAC_RUNNING, &ar->dev_flags)",
        "WMI_RX_STATUS_ERR_DECRYPT", "WMI_RX_STATUS_ERR_KEY_CACHE_MISS",
        "WMI_RX_STATUS_ERR_CRC", "status->signal = snr + ATH10K_DEFAULT_NOISE_FLOOR;",
        "channel >= 1 && channel <= 14",
        "channel >= 36 && channel <= ATH10K_MAX_5G_CHAN",
    ):
        require(marker in mgmt_rx_handler, f"pinned ath10k management-RX policy changed: {marker}")
    require("#define ATH10K_DEFAULT_NOISE_FLOOR -95" in ath10k_core_h,
            "pinned ath10k default noise floor changed")
    max_5g_match = re.search(r"#define ATH10K_MAX_5G_CHAN (\d+)", ath10k_core_h)
    require(max_5g_match is not None, "pinned ath10k maximum 5 GHz channel is missing")
    scan_results_rust = (ROOT / "rust/src/ieee80211_scan_results.rs").read_text()
    for marker in (
        "const ATH10K_DEFAULT_NOISE_FLOOR_DBM: i32 = -95;",
        f"const ATH10K_MAX_5G_CHANNEL: u8 = {max_5g_match.group(1)};",
        "if cac_running", "if event.status & RX_ERROR_MASK != 0",
        "snr.checked_add(ATH10K_DEFAULT_NOISE_FLOOR_DBM)",
        "(1..=14).contains(&channel) || (36..=ATH10K_MAX_5G_CHANNEL).contains(&channel)",
        "self.storage[..event.frame.len()].copy_from_slice(event.frame);",
    ):
        require(marker in scan_results_rust, f"Rust scan-result policy changed: {marker}")
    print("PASS: bounded WMI scan-event and management-RX decoders match ath10k 10.4 source")

    # Keep the ath10k WMI control command on its upstream HTC control/service
    # path, including the target-assigned endpoint and WCN3990 CE descriptor.
    htc_header_layout = section(htc_header, "struct ath10k_htc_hdr {", "\n} __packed __aligned(4);")
    require(
        htc_header_layout.index("u8 eid;")
        < htc_header_layout.index("u8 flags;")
        < htc_header_layout.index("__le16 len;")
        < htc_header_layout.index("u8 seq_no;")
        and "ATH10K_HTC_MSG_READY_ID                = 1" in htc_header
        and "ATH10K_HTC_MSG_CONNECT_SERVICE_ID      = 2" in htc_header
        and "ATH10K_HTC_MSG_CONNECT_SERVICE_RESP_ID = 3" in htc_header
        and "ATH10K_HTC_SVC_ID_WMI_CONTROL\t= SVC(ATH10K_HTC_SVC_GRP_WMI, 0)" in htc_header,
        "pinned HTC header, control-message IDs, or WMI service ID changed",
    )
    htc_prepare = section(
        htc,
        "static void ath10k_htc_prepare_tx_skb",
        "static int ath10k_htc_consume_credit",
    )
    htc_credit = section(
        htc,
        "static int ath10k_htc_consume_credit",
        "static void ath10k_htc_release_credit",
    )
    require(
        "hdr->eid = ep->eid;" in htc_prepare
        and "hdr->len = __cpu_to_le16(skb->len - sizeof(*hdr));" in htc_prepare
        and "hdr->flags |= ATH10K_HTC_FLAG_NEED_CREDIT_UPDATE;" in htc_prepare
        and "hdr->seq_no = ep->seq_no++;" in htc_prepare
        and "credits = DIV_ROUND_UP(len, ep->tx_credit_size);" in htc_credit,
        "pinned HTC frame or per-endpoint credit behavior changed",
    )
    htc_endpoint_enum = section(htc_header, "enum ath10k_htc_ep_id {", "\n};")
    require(
        "ATH10K_HTC_EP_0 = 0," in htc_endpoint_enum
        and "ATH10K_HTC_EP_8," in htc_endpoint_enum
        and "ATH10K_HTC_EP_COUNT," in htc_endpoint_enum
        and "ATH10K_HTC_FLAG_NEED_CREDIT_UPDATE = 0x01" in htc_header,
        "ath10k HTC endpoint range or credit-update flag changed",
    )
    htc_header_positions = [
        htc_header_layout.index(marker)
        for marker in (
            "u8 eid;", "u8 flags;", "__le16 len;", "u8 trailer_len;",
            "u8 seq_no;", "__le16 pad_len;",
        )
    ]
    require(
        htc_header_positions == sorted(htc_header_positions),
        "ath10k HTC transmit header byte order changed",
    )
    htc_send = section(
        htc,
        "int ath10k_htc_send(struct ath10k_htc *htc,",
        "\nvoid ath10k_htc_tx_completion_handler",
    )
    htc_send_positions = [
        htc_send.index(marker)
        for marker in (
            "skb_push(skb, sizeof(struct ath10k_htc_hdr));",
            "skb_len = skb->len;",
            "ath10k_htc_consume_credit(ep, skb_len, true);",
            "ath10k_htc_prepare_tx_skb(ep, skb);",
        )
    ]
    require(
        htc_send_positions == sorted(htc_send_positions)
        and "if (ep->tx_credit_flow_enabled && !ep->bundle_tx)" in htc_prepare,
        "ath10k HTC credit and sequence preparation order changed",
    )
    wmi_tx_rust = (ROOT / "rust/src/ath10k_wmi_tx.rs").read_text()
    require(
        "pub const HTC_HEADER_BYTES: usize = 8;" in wmi_tx_rust
        and "pub const WMI_COMMAND_HEADER_BYTES: usize = 4;" in wmi_tx_rust
        and "pub const HTC_ENDPOINT_COUNT: u8 = 9;" in wmi_tx_rust
        and "pub const HTC_FLAG_NEED_CREDIT_UPDATE: u8 = 0x01;" in wmi_tx_rust
        and "let credits = div_ceil(frame_bytes, usize::from(credit_size));" in wmi_tx_rust
        and "let credit_update_requested = credit_flow_enabled && !bundle_tx_enabled;" in wmi_tx_rust
        and "output[5] = plan.sequence_number;" in wmi_tx_rust
        and "copy_from_slice(&(wmi_payload_bytes as u16).to_le_bytes());" in wmi_tx_rust
        and "copy_from_slice(&command_id.to_le_bytes());" in wmi_tx_rust,
        "Rust upstream ath10k WMI/HTC frame layout differs from pinned source",
    )
    print("PASS: single WMI command frame matches upstream ath10k HTC header, sequence and credits")
    ce_desc = section(ce_header, "struct ce_desc_64 {", "\n};")
    ce_send = section(ce, "static int _ath10k_ce_send_nolock_64(", "\nint ath10k_ce_send_nolock(")
    require(
        ce_desc.index("__le64 addr;")
        < ce_desc.index("__le16 nbytes;")
        < ce_desc.index("__le16 flags;")
        < ce_desc.index("__le32 toeplitz_hash_result;")
        and "addr[0] = __cpu_to_le32(buffer);" in ce_send
        and "sdesc.nbytes = __cpu_to_le16(nbytes);" in ce_send
        and "sdesc.flags  = __cpu_to_le16(desc_flags);" in ce_send,
        "pinned WCN3990 CE descriptor layout or serializer changed",
    )
    rust_ce = (ROOT / "rust/src/ath10k_ce.rs").read_text()
    rust_probe = (ROOT / "rust/src/wlan_protocol_probe.rs").read_text()
    rust_ce_descriptor = section(
        rust_ce,
        "pub struct Wcn3990Descriptor([u8; 16]);",
        "/// Parsed receive-side HTC header.",
    )
    rust_wmi_transfer = section(
        wmi_tx_rust,
        "pub fn encode_wmi_command_transfer(",
        "\nfn div_ceil",
    )
    transfer_positions = [
        rust_wmi_transfer.index(marker)
        for marker in (
            "let tx = preflight_wmi_command_frame(",
            "Wcn3990Descriptor::encode(",
            "write_wmi_command_frame(",
            "Ok(TransferPlan { tx, descriptor })",
        )
    ]
    wmi_tx_imports = re.search(
        r"use crate::ath10k_ce::\{(.*?)\};", wmi_tx_rust, re.S
    )
    require(
        transfer_positions == sorted(transfer_positions)
        and wmi_tx_imports is not None
        and all(
            name in wmi_tx_imports.group(1)
            for name in ("CeError", "CeTxRing", "HtcCreditLedger", "Wcn3990Descriptor")
        )
        and "pub descriptor: Wcn3990Descriptor" in wmi_tx_rust
        and "map_err(Error::CeDescriptor)?" in rust_wmi_transfer
        and "const WCN3990_METADATA_SHIFT: u32 = 4;" in rust_ce
        and "WCN3990_GATHER_ADDRESS_FLAG: u32 = 1 << 31;" in rust_ce
        and "bytes[0..4].copy_from_slice(&(iova as u32).to_le_bytes());" in rust_ce_descriptor
        and "bytes[4..8].copy_from_slice(&address_high.to_le_bytes());" in rust_ce_descriptor
        and "bytes[8..10].copy_from_slice(&length.to_le_bytes());" in rust_ce_descriptor
        and "bytes[10..12].copy_from_slice(&flags.to_le_bytes());" in rust_ce_descriptor,
        "WMI-to-HTC-to-WCN3990-CE serializer composition changed",
    )
    print("PASS: one WMI command composes into an HTC frame and WCN3990 CE descriptor")
    rust_wmi_queue = section(
        wmi_tx_rust,
        "pub fn queue_transfer(",
        "\n    pub fn apply_credit_report",
    )
    queue_positions = [
        rust_wmi_queue.index(marker)
        for marker in (
            "preflight_wmi_command_frame(",
            ".consume(self.endpoint_id, tx.frame_bytes)",
            "self.next_sequence_number = self.next_sequence_number.wrapping_add(1);",
            "Wcn3990Descriptor::encode(",
            "write_wmi_command_frame(",
            "next_ring.enqueue(context)",
            "self.credits = next_credits;",
        )
    ]
    require(
        queue_positions == sorted(queue_positions)
        and "map_err(Error::TxRing)?" in rust_wmi_queue
        and "pub fn reclaim_to(" in wmi_tx_rust
        and "pub fn apply_credit_report(" in wmi_tx_rust,
        "stateful HTC-credit/sequence/CE-ring submission model changed",
    )
    print("PASS: WMI queue model follows source credit, sequence, CE-slot and completion ownership")
    for marker in (
        "HtcControlMessage::Ready(ready)",
        "HtcControlMessage::ConnectServiceResponse(response)",
        "session.connect(WMI_CONTROL_SERVICE_ID, response)",
        "HtcTxHeader::write_frame(",
        "credits.consume(endpoint.endpoint_id, frame_len)",
        "Wcn3990Descriptor::encode(0x8000_0000, frame_len, 1, false, false)",
        "fn scan_transfer_codec_ok() -> bool",
        "StartScanRequest::linux_reference_defaults(1, &channels, &ssids, &ies)",
        "WmiCommandTxQueue::<4>::new_wmi_control(",
        "let Ok(start) = queue_start_scan(",
        "let Ok(stop) = queue_stop_scan(",
        "queue.reclaim_to(2, &mut completed)",
    ):
        require(marker in rust_probe,
                f"no-radio WMI/HTC/CE composition check changed: {marker}")
    for marker in (
        "HTC_READY_MESSAGE_ID: u16 = 1",
        "HTC_CONNECT_REQUEST_MESSAGE_ID: u16 = 2",
        "HTC_CONNECT_RESPONSE_MESSAGE_ID: u16 = 3",
        "HTC_WMI_CONTROL_SERVICE_ID: u16 = 0x0100",
        "pub struct HtcTxHeader",
        "pub struct HtcSession",
    ):
        require(marker in rust_ce,
                f"Rust HTC control or WMI service constant changed: {marker}")

    firmware_parser = section(
        ath10k_core,
        "int ath10k_core_fetch_firmware_api_n(",
        "\nstatic void ath10k_core_get_fw_name",
    )
    for marker in (
        "ATH10K_FIRMWARE_MAGIC",
        "strlen(ATH10K_FIRMWARE_MAGIC) + 1",
        "magic_len = ALIGN(magic_len, 4);",
        "while (len > sizeof(struct ath10k_fw_ie))",
        "ie_id = le32_to_cpu(hdr->id);",
        "ie_len = le32_to_cpu(hdr->len);",
        "if (len < ie_len)",
        "case ATH10K_FW_IE_FEATURES:",
        "for (i = 0; i < ATH10K_FW_FEATURE_COUNT; i++)",
        "if (data[index] & (1 << bit))",
        "case ATH10K_FW_IE_WMI_OP_VERSION:",
        "case ATH10K_FW_IE_HTT_OP_VERSION:",
        "if (ie_len != sizeof(u32))",
        "ie_len = ALIGN(ie_len, 4);",
    ):
        require(marker in firmware_parser,
                f"pinned ath10k firmware metadata parser changed: {marker}")
    for marker in (
        "ATH10K_FW_IE_FEATURES = 2",
        "ATH10K_FW_IE_FW_IMAGE = 3",
        "ATH10K_FW_IE_WMI_OP_VERSION = 5",
        "ATH10K_FW_IE_HTT_OP_VERSION = 6",
        "ATH10K_FW_WMI_OP_VERSION_10_4 = 6",
    ):
        require(marker in ath10k_hw,
                f"pinned ath10k firmware metadata identifier changed: {marker}")
    for marker in (
        "ATH10K_FW_FEATURE_PEER_FLOW_CONTROL = 13",
        "ATH10K_FW_FEATURE_NON_BMI = 19",
        "ATH10K_FW_FEATURE_COUNT,",
    ):
        require(marker in ath10k_core_h,
                f"pinned ath10k firmware feature index changed: {marker}")
    require("ATH10K_FW_FEATURE_NON_BMI" in ath10k_core
            and "ATH10K_FW_IE_FW_IMAGE" in firmware_parser,
            "pinned API-5 external-image gate changed")
    firmware_rs = (ROOT / "rust/src/ath10k_firmware.rs").read_text()
    for marker in (
        "pub fn parse(bytes: &[u8]) -> Result<Self, Ath10kFirmwareError>",
        "ATH10K_FW_FEATURE_PEER_FLOW_CONTROL: usize = 13",
        "ATH10K_FW_FEATURE_NON_BMI: usize = 19",
        "pub const fn has_peer_flow_control(&self) -> bool",
        "pub const fn requires_external_firmware(&self) -> bool",
        "IePaddingTruncated",
    ):
        require(marker in firmware_rs,
                f"bounded Rust API-5 metadata parser changed: {marker}")

    # Keep protocol guidance tied to this exact ath10k client implementation.
    # In this source, 0x003c is M3_INFO_REQ (not M3_INIT_REQ); it is declared
    # in the schema but is not sent by qmi.c's startup path. The C structure is
    # input to the QMI TLV codec, not a raw serialized request buffer.
    require("#define QMI_WLFW_M3_INFO_REQ_V01 0x003C" in wlfw_header,
            "the pinned WLFW message ID/name for 0x003c changed")
    require("QMI_WLFW_M3_INIT_REQ" not in wlfw_header + wlfw_codec + qmi,
            "an M3_INIT request appeared; audit its schema and startup use")
    m3_info = section(
        wlfw_header,
        "struct wlfw_m3_info_req_msg_v01 {",
        "\n};",
    )
    require("u64 addr;" in m3_info and "u32 size;" in m3_info,
            "M3_INFO request fields changed")
    require("WLFW_M3_INFO_REQ_MSG_V01_MAX_MSG_LEN 18" in wlfw_header,
            "M3_INFO v01 payload size changed")
    m3_info_codec = section(
        wlfw_codec,
        "const struct qmi_elem_info wlfw_m3_info_req_msg_v01_ei[] = {",
        "\n};",
    )
    require(m3_info_codec.count(".tlv_type") == 2
            and ".tlv_type       = 0x01" in m3_info_codec
            and ".tlv_type       = 0x02" in m3_info_codec,
            "M3_INFO fields are no longer encoded as the two pinned QMI TLVs")
    require("QMI_WLFW_M3_INFO_REQ_V01" not in qmi,
            "M3_INFO was added to the ath10k QMI path; audit its sequencing")

    startup = section(
        qmi,
        "static void ath10k_qmi_event_server_arrive",
        "\nstatic int ath10k_qmi_fetch_board_file",
    )
    startup_calls = (
        "ath10k_qmi_ind_register_send_sync_msg(qmi)",
        "ath10k_qmi_host_cap_send_sync(qmi)",
        "ath10k_qmi_msa_mem_info_send_sync_msg(qmi)",
        "ath10k_qmi_setup_msa_permissions(qmi)",
        "ath10k_qmi_msa_ready_send_sync_msg(qmi)",
        "ath10k_qmi_cap_send_sync_msg(qmi)",
    )
    positions = [startup.find(call) for call in startup_calls]
    require(all(position >= 0 for position in positions)
            and positions == sorted(positions),
            "ath10k WLFW startup call order changed")

    fw_log = section(
        qmi,
        "int ath10k_qmi_set_fw_log_mode",
        "\nstatic void ath10k_qmi_event_server_arrive",
    )
    require("QMI_WLFW_INI_REQ_V01" in fw_log
            and "enablefwlog_valid" in fw_log
            and "enablefwlog = fw_log_mode" in fw_log
            and "QMI_WLFW_INI_REQ_V01" not in startup,
            "WLFW INI request is no longer limited to firmware log-mode control")

    msa_ready = section(
        qmi,
        "static void ath10k_qmi_event_msa_ready",
        "\nstatic int ath10k_qmi_event_fw_ready_ind",
    )
    msa_ready_calls = (
        "ath10k_qmi_fetch_board_file(qmi)",
        "ath10k_qmi_bdf_dnld_send_sync(qmi)",
        "ath10k_qmi_send_cal_report_req(qmi)",
    )
    msa_positions = [msa_ready.find(call) for call in msa_ready_calls]
    require(all(position >= 0 for position in msa_positions)
            and msa_positions == sorted(msa_positions),
            "ath10k MSA_READY board/calibration sequence changed")

    wlan_pd = section(
        mapper,
        "static const struct qcom_pdm_domain_data mpss_wlan_pd = {",
        "\n};",
    )
    require('.domain = "msm/modem/wlan_pd"' in wlan_pd,
            "WLAN protection-domain service mapping changed")
    require(".instance_id = 180" in wlan_pd,
            "WLAN protection-domain instance changed")
    require('"kernel/elf_loader"' in wlan_pd and '"wlan/fw"' in wlan_pd,
            "WLAN protection-domain service names changed")

    # Public APR source consumes kernel/elf_loader as a service dependency:
    # it registers a PDR lookup and creates the APR client device only after
    # that service's protection domain reports UP. This is not a child-PD
    # image loader or launch API.
    apr_lookup = section(
        apr,
        "static int of_apr_add_pd_lookups(",
        "static void of_register_apr_devices(",
    )
    apr_register = section(
        apr,
        "static void of_register_apr_devices(",
        "static int apr_remove_device(",
    )
    apr_status = section(
        apr,
        "static void apr_pd_status(",
        "static int apr_probe(",
    )
    apr_probe = section(
        apr,
        "static int apr_probe(",
        "static void apr_remove(",
    )
    require("service name and path for APR service" in apr_binding
            and '"kernel/elf_loader", "msm/modem/wlan_pd"' in apr_binding,
            "APR binding no longer identifies kernel/elf_loader as an APR service dependency")
    require("pdr_add_lookup(apr->pdr, service_name, service_path)" in apr_lookup,
            "APR client no longer resolves protection-domain services through PDR")
    require("if (svc_path)" in apr_register
            and "if (strcmp(service_path, svc_path))" in apr_register
            and "device_register(&adev->dev)" in apr,
            "APR client device is no longer registered against the matching available domain")
    require("case SERVREG_SERVICE_STATE_UP:" in apr_status
            and "of_register_apr_devices(apr->dev, svc_path)" in apr_status
            and "case SERVREG_SERVICE_STATE_DOWN:" in apr_status,
            "APR service device lifecycle no longer follows PDR UP/DOWN notifications")
    apr_probe_calls = (
        "apr->pdr = pdr_handle_alloc(apr_pd_status, apr)",
        "of_apr_add_pd_lookups(dev)",
        "of_register_apr_devices(dev, NULL)",
    )
    apr_positions = [apr_probe.find(call) for call in apr_probe_calls]
    require(all(position >= 0 for position in apr_positions)
            and apr_positions == sorted(apr_positions)
            and "qcom_scm_pas_auth_and_reset" not in apr
            and "rproc_boot(" not in apr,
            "APR client registration order changed or acquired a launch operation")

    # pdr_restart_pd() is an RPC client: it uses the already-resolved service
    # address and waits for the remote SERVREG_NOTIF response. It is not the
    # local image loader/authenticator, and its source does not define what the
    # remote modem-side PDR endpoint does for an UNINIT WLAN domain.
    pdr_restart = section(
        pdr_client,
        "int pdr_restart_pd(struct pdr_handle *pdr, struct pdr_service *pds)",
        "EXPORT_SYMBOL_GPL(pdr_restart_pd);",
    )
    require("if (!pds->service_connected)" in pdr_restart
            and "addr = pds->addr;" in pdr_restart
            and "qmi_send_request(&pdr->notifier_hdl, &addr" in pdr_restart
            and "SERVREG_RESTART_PD_REQ" in pdr_restart
            and "qmi_txn_wait(&txn" in pdr_restart,
            "PDR restart no longer sends a response-correlated request to the resolved service")
    require("request for PD restart failed" in pdr_restart
            and "qcom_scm_pas" not in pdr_restart
            and "rproc_boot(" not in pdr_restart
            and "request_firmware" not in pdr_restart,
            "PDR client no longer distinguishes remote restart RPC from local image launch")

    # ICNSS registers for domain-state notifications during driver setup, but
    # its RESTART_PD helper is entered from recovery hooks after that setup.
    # Do not mistake this public recovery operation for a cold-start trigger.
    pdr_setup = section(
        icnss,
        "static int icnss_pd_restart_enable(",
        "int __icnss_register_driver(",
    )
    pdr_location = section(
        icnss,
        "static int icnss_get_service_location_notify(",
        "static int icnss_pd_restart_enable(",
    )
    pdr_recovery = section(
        icnss,
        "int icnss_trigger_recovery(",
        "EXPORT_SYMBOL(icnss_trigger_recovery);",
    )
    esoc_power_off = section(
        icnss,
        "static void icnss_esoc_ops_power_off(",
        "static int icnss_register_esoc_client(",
    )
    require("get_service_location(" in pdr_setup
            and "service_notif_pd_restart" not in pdr_setup
            and "icnss_trigger_recovery" not in pdr_setup,
            "ICNSS PDR registration now triggers restart during initial setup")
    require("service_notif_register_notifier(" in pdr_location
            and "ICNSS_PDR_REGISTERED" in pdr_location
            and "service_notif_pd_restart" not in pdr_location,
            "ICNSS locator callback no longer only registers domain-state notifiers")
    require("if (!test_bit(ICNSS_PDR_REGISTERED, &priv->state))" in pdr_recovery
            and "if (!priv->service_notifier || !priv->service_notifier[0].handle)" in pdr_recovery
            and "service_notif_pd_restart(priv->service_notifier[0].name" in pdr_recovery,
            "ICNSS PDR recovery no longer requires registered service state and handle")
    require("ESOC_HOOK_MDM_DOWN" in esoc_power_off
            and "icnss_trigger_recovery(&priv->pdev->dev)" in esoc_power_off,
            "ICNSS external-modem recovery hook changed; review its startup implications")
    require(icnss.count("icnss_trigger_recovery(&priv->pdev->dev)") == 2
            and "case 3:\n\t\t\tret = icnss_trigger_recovery(&priv->pdev->dev);" in icnss,
            "ICNSS gained an unexpected host-triggered PDR call site")

    # ICNSS is a WLFW QMI client. Its new-server callback only posts an event;
    # it neither publishes the service nor launches the firmware image.
    wlfw_new_server = section(
        icnss_qmi,
        "static int wlfw_new_server(",
        "static void wlfw_del_server(",
    )
    wlfw_client_registration = section(
        icnss_qmi,
        "int icnss_register_fw_service(",
        "void icnss_unregister_fw_service(",
    )
    require("icnss_driver_event_post(ICNSS_DRIVER_EVENT_SERVER_ARRIVE" in wlfw_new_server
            and "qmi_add_server" not in wlfw_new_server
            and "request_firmware" not in wlfw_new_server,
            "ICNSS WLFW server-arrival callback gained a local service/image launch")
    require("qmi_handle_init(" in wlfw_client_registration
            and "qmi_add_lookup(&priv->qmi, WLFW_SERVICE_ID_V01" in wlfw_client_registration
            and "qmi_add_server" not in wlfw_client_registration,
            "ICNSS WLFW registration no longer behaves as a client-side lookup")

    sm7150_domains = section(
        mapper,
        "static const struct qcom_pdm_domain_data *sm7150_domains[] = {",
        "\n};",
    )
    require("&mpss_wlan_pd" in sm7150_domains,
            "SM7150 no longer advertises the WLAN protection domain")
    require("&mpss_root_pd_gps" in sm7150_domains
            and "&mpss_root_pd_gps_pdr" not in sm7150_domains,
            "SM7150 root-domain aliases changed; review PDR feature discovery")
    require('.compatible = "qcom,sm7150", .data = sm7150_domains' in mapper,
            "SM7150 domain map is no longer selected by the qcom,sm7150 compatible")

    root_pd_gps = section(
        mapper,
        "static const struct qcom_pdm_domain_data mpss_root_pd_gps = {",
        "\n};",
    )
    require('"gps/gps_service"' in root_pd_gps
            and '"tms/pdr_enabled"' not in root_pd_gps,
            "SM7150 root-domain service aliases changed")
    require('"tms/pdr_enabled"' in mapper,
            "source lost the platform-specific tms/pdr_enabled comparison")

    require("qmi_add_lookup" in qmi and "QMI_SERVICE_ID_WLFW" in qmi,
            "ath10k no longer performs WLFW service discovery")
    require("qmi_add_server" not in qmi,
            "ath10k QMI path now contains server registration; review provider role")

    mpss = section(sunfish_dts, "&remoteproc_mpss {", "\n};")
    require('firmware-name = "qcom/sm7150/google/sunfish/modem.mbn"' in mpss,
            "Sunfish MPSS firmware declaration changed")
    require("remoteproc_wlan" not in sm7150_dtsi + sunfish_dts,
            "an explicit WLAN remoteproc node appeared; review launch path")

    references = subprocess.run(
        [
            "git", "-C", str(source), "grep", "-n", "-i", "-E",
            "wlanmdsp[.]mbn|tqftpserv", "HEAD", "--",
        ],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    require(references.returncode == 1,
            "an explicit WLAN image or TFTP reference appeared anywhere in pinned Linux; review it")

    boot_references = subprocess.run(
        [
            "git", "-C", str(lineage_device_source), "grep", "-n", "-i", "-E",
            "wlan_pd|wlanmdsp[.]mbn|tqftpserv", "HEAD", "--",
            "init*", "*.rc", "*.sh",
        ],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    require(boot_references.returncode == 1,
            "a Sunfish init/rc/shell file directly references WLAN PD/image/TFTP; review it")

    # The native owner must preserve the observed Linux ordering: ICNSS power
    # votes complete before the first WLFW indication-registration request.
    # Keep the public launch-path audit tied to the actual runtime composition,
    # not only the standalone state-machine fixture.
    glink_runtime = (ROOT / "rust/src/qcom_glink_qmi.rs").read_text()
    modem_owner = (ROOT / "rust/src/qcom_modem_owner.rs").read_text()
    pixel_runtime = (ROOT / "rust/src/pixel_modem_owner.rs").read_text()
    wlan_power = (ROOT / "rust/src/qcom_wlan_power.rs").read_text()
    gated_session = section(
        glink_runtime,
        "pub fn enable_wlfw_client_power_gated(",
        "/// Reserve one local QRTR port for the exact IPA",
    )
    registration_stage = section(
        glink_runtime,
        "fn stage_wlfw_registration(",
        "fn stage_wlfw_dynamic_feature_mask(",
    )
    runtime_start = section(
        pixel_runtime,
        "pub fn start(&mut self) -> Result<(), Error>",
        "pub fn notified(&mut self, channels: ChannelSet)",
    )
    runtime_notified = section(
        pixel_runtime,
        "pub fn notified(&mut self, channels: ChannelSet)",
        "#[cfg(all(test",
    )
    power_services = section(
        pixel_runtime,
        "pub struct IcnssPowerServices<",
        "const TFTP_SERVER_PORT:",
    )
    power_backend = section(
        pixel_runtime,
        "pub struct BrokeredIcnssPowerBackend<",
        "/// Runtime composition that keeps WLFW indication registration",
    )
    power_step = power_services
    require(
        "wlfw_power_gate: bool" in glink_runtime
        and "wlfw_power_ready: bool" in glink_runtime
        and "if self.wlfw_power_gate && !self.wlfw_power_ready" in registration_stage
        and "pub fn authorize_wlfw_power" in gated_session
        and "permit.generation() != generation" in gated_session,
        "the bounded GLINK session no longer blocks WLFW registration on a matching power permit",
    )
    require(
        "owner.enable_wlfw_client_power_gated(WLFW_CLIENT_PORT)?;" in pixel_runtime
        and "wlan_startup_step(&mut self.owner" in runtime_start
        and "wlan_startup_step(&mut self.owner" in runtime_notified
        and "self.backend.ensure_powered(generation)" in power_step
        and "owner.authorize_wlfw_power(permit, interrupts)" in power_step
        and "endpoint.is_none()" in power_step
        and "self.stop_power_if_active(owner)" in power_step,
        "the production runtime no longer orders ICNSS power before WLFW or restores it on endpoint loss",
    )
    require(
        "pub struct PowerReadyPermit" in wlan_power
        and "pub fn power_ready_permit(&self, generation: u64)" in wlan_power
        and "generation: active" in wlan_power
        and "if active == generation" in wlan_power,
        "the ICNSS power authorization no longer comes from an active sequencer state",
    )
    require(
        "qcom_wlan_rpmh_io::BrokeredWlanIo<'a, G, T>" in power_backend
        and "BrokeredWlanIo::new(" in power_backend
        and ".start(" in power_backend
        and "&mut self.io, generation, self.plan)" in power_backend
        and ".power_ready_permit(generation)" in power_backend
        and ".stop_with_permit(" in power_backend
        and "&mut self.io, generation)" in power_backend,
        "the runtime power backend no longer composes the sequencer with the shared RPMh broker adapter",
    )
    require(
        "pub unsafe fn attach_with_icnss_power" in pixel_runtime
        and pixel_runtime.count("attach_with_icnss_power") == 1
        and "impl IcnssPowerBackend for" not in pixel_runtime,
        "the current production source unexpectedly wires an ICNSS backend or attachment call site",
    )

    print(f"PASS: pinned Linux source {commit} and clean audited files")
    print(f"PASS: pinned LineageOS Sunfish ICNSS source {lineage_kernel_commit} and clean audited files")
    print(f"PASS: pinned LineageOS Sunfish device tree {lineage_device_commit} and clean audited init files")
    print("PASS: SM7150 service map advertises msm/modem/wlan_pd instance 180")
    print("PASS: SM7150 uses the root-domain map without tms/pdr_enabled; empty lookup is expected")
    print("PASS: public qcom_pd_mapper builds a static service map and answers locator queries; it is not a PD launcher")
    print("PASS: public userspace pd-mapper reads remoteproc firmware metadata and serves static SERVREG lookups; no PD launch path found")
    print("PASS: public APR code resolves kernel/elf_loader via PDR and registers its client after PD-UP; this is not the PD launcher")
    print("PASS: public PDR helper sends RESTART_PD to an already resolved SERVREG_NOTIF endpoint; remote load behavior remains outside this client")
    print("PASS: public ICNSS registers PDR notifiers at setup; its RESTART_PD request is gated recovery, not initial WLAN start")
    print("PASS: public ICNSS discovers WLFW as a QMI client; server arrival does not launch the server or image")
    print("PASS: LineageOS starts TFTP, modem_svc, pd-mapper, CNSS, and per_mgr/pm-proxy services")
    print("PASS: public Sunfish init contains a ro.baseband-gated mdm_helper start and root service; public product makefiles omit init.mdm.sh and conditionally inherit private vendor makefiles, so static source alone does not establish runtime installation")
    print("PASS: per_mgr and pm-proxy are extracted vendor binaries; their private launch behavior is not inferred")
    print("PASS: LineageOS inserts wlan.ko for qca_cld3; no pinned Sunfish init/rc/shell file directly references wlan_pd, wlanmdsp.mbn, or tqftpserv")
    print("PASS: LineageOS vendor manifest classifies modem_svc, cnss-daemon, pm-service, pm-proxy, and wlanmdsp.mbn as extracted vendor blobs")
    print("PASS: pinned Sunfish Linux starts root MPSS via PAS 4/authentication and readiness wait; this does not establish a wlan_pd child launcher")
    print("PASS: pinned SCM source documents the Gunyah EL1 trap/shared-memory bridge path; it grants no seL4 caller authority")
    print("PASS: pinned Lineage /dev/subsys_modem open can enter SSR/PIL and load modem.mdt; the identical fw_name log can also come from sysfs, so the runtime caller remains unknown")
    print("PASS: ath10k looks up WLFW as a client; this file does not register a server")
    print("PASS: pinned WLFW startup order is registration, optional host-cap, MSA, permission, ready, capability")
    print("PASS: 0x003c is declared M3_INFO with addr/size TLVs; ath10k startup does not send it")
    print("PASS: WLFW INI_REQ is firmware-log-mode control, separate from WLAN startup configuration")
    print("PASS: Sunfish DTS names MPSS modem.mbn and has no WLAN remoteproc node")
    print("PASS: ath10k service-ready host-memory requests are capped at 16")
    print("PASS: ath10k host-memory chunks use one coherent rounded-size allocation and req_id/ptr/size response fields")
    print("PASS: ath10k WMI 10.4 INIT uses 42 resource words, count plus req_id/ptr/size chunks, and a zeroed minimum slot")
    print("PASS: WMI 10.4 selects fixed SERVICE_READY/READY layouts, raw peer-cache bit 37 maps to Linux bit 78, and INIT uses command ID 0xa000")
    print("PASS: WCN3990 resource-word order, 33-peer hardware override, 0x7 chain masks, and PFC descriptor branch match pinned source")
    print("PASS: raw WMI 10.4 peer-caching bit 37 drives the WCN3990 peer/active-peer/TID fields before INIT")
    print("PASS: host-memory vdev/peer/active-peer counts use pinned +1 rules and active-peer fallback")
    print("PASS: HTC WMI_CONTROL framing, dynamic endpoint/credits, and WCN3990 CE descriptor match pinned source")
    print("PASS: bounded API-5 metadata parser matches ath10k magic, IE alignment, feature bits, and WMI/HTT version fields")
    print("PASS: pinned Linux tree has no explicit wlanmdsp.mbn or tqftpserv reference")
    print("PASS: production WLFW registration is gated on sequenced ICNSS power and runtime lifecycle work")
    print("PASS: generic ICNSS backend composes the verified sequencer with the shared RPMh broker adapter")
    print("LIMIT: attach_with_icnss_power has no production caller or concrete backend")
    print("LIMIT: no source-backed WLAN image launcher or WLFW provider is established")


if __name__ == "__main__":
    main()
