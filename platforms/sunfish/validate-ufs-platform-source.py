#!/usr/bin/env python3
"""Pin the UFS CAP-read and HCE link-start ordering to Sunfish Lineage source."""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
SOURCE_FILES = (
    "drivers/scsi/ufs/ufshcd.c",
    "drivers/scsi/ufs/ufs-qcom.c",
    "drivers/phy/qualcomm/phy-qcom-ufs.c",
    "drivers/scsi/ufs/ufshcd-pltfrm.c",
    "include/dt-bindings/clock/qcom,gcc-sdmmagpie.h",
    "drivers/clk/qcom/gcc-sdmmagpie.c",
    "drivers/clk/qcom/clk-rcg2.c",
    "drivers/clk/qcom/vdd-level-sdmmagpie.h",
    "include/dt-bindings/regulator/qcom,rpmh-regulator.h",
    "drivers/scsi/ufs/ufs.h",
    "drivers/regulator/rpmh-regulator.c",
)
RUNTIME_DTS = ROOT / ".cache/pixel-sunfish/live-lineage-20261010.dts"
RUNTIME_DTS_SHA256 = "1e20ee44f3e1c6c37e1eb125b146fe2eacd3524aa239eaf06b95040d840c07c0"
HOST_CLOCK_NAMES = (
    "core_clk",
    "bus_aggr_clk",
    "iface_clk",
    "core_clk_unipro",
    "core_clk_ice",
    "ref_clk",
    "tx_lane0_sync_clk",
    "rx_lane0_sync_clk",
)
HOST_CLOCK_PROVIDER_IDS = (0x7E, 0x09, 0x7D, 0x89, 0x81, 0x00, 0x88, 0x87)
HOST_CLOCK_GCC_MACROS = (
    "GCC_UFS_PHY_AXI_CLK",
    "GCC_AGGRE_UFS_PHY_AXI_CLK",
    "GCC_UFS_PHY_AHB_CLK",
    "GCC_UFS_PHY_UNIPRO_CORE_CLK",
    "GCC_UFS_PHY_ICE_CORE_CLK",
    None,
    "GCC_UFS_PHY_TX_SYMBOL_0_CLK",
    "GCC_UFS_PHY_RX_SYMBOL_0_CLK",
)
HOST_CLOCK_FREQ_TABLE_HZ = (
    (50_000_000, 200_000_000),
    (0, 0),
    (0, 0),
    (37_500_000, 150_000_000),
    (75_000_000, 300_000_000),
    (0, 0),
    (0, 0),
    (0, 0),
)


def fail(message: str) -> None:
    raise SystemExit(f"UFS source validation failed: {message}")


def function_body(source: str, name: str) -> str:
    clean = re.sub(r"/\*.*?\*/|//[^\n]*", " ", source, flags=re.DOTALL)
    definitions = re.finditer(
        rf"(?:^|\n)\s*(?:static\s+)?(?:int|void)\s+{re.escape(name)}\s*\(",
        clean,
    )
    for definition in definitions:
        open_paren = clean.find("(", definition.start())
        depth = 0
        close_paren = None
        for index in range(open_paren, len(clean)):
            if clean[index] == "(":
                depth += 1
            elif clean[index] == ")":
                depth -= 1
                if depth == 0:
                    close_paren = index
                    break
        if close_paren is None:
            continue
        open_brace = close_paren + 1
        while open_brace < len(clean) and clean[open_brace].isspace():
            open_brace += 1
        if open_brace >= len(clean) or clean[open_brace] != "{":
            continue
        depth = 0
        for index in range(open_brace, len(clean)):
            if clean[index] == "{":
                depth += 1
            elif clean[index] == "}":
                depth -= 1
                if depth == 0:
                    return clean[open_brace + 1 : index]
        fail(f"unterminated body of {name}")
    else:
        fail(f"could not find definition of {name}")


def named_initializer(source: str, declaration: str, name: str) -> str:
    match = re.search(
        rf"\bstatic\s+{re.escape(declaration)}\s+{re.escape(name)}\s*(?:\[\s*\])?\s*=\s*\{{",
        source,
    )
    if match is None:
        fail(f"could not find initializer for {name}")
    open_brace = source.find("{", match.start())
    depth = 0
    for index in range(open_brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[open_brace + 1 : index]
    fail(f"unterminated initializer for {name}")


def validate_clock_corner(gcc_source: str, name: str, expected: dict[str, int]) -> None:
    body = named_initializer(gcc_source, "struct clk_rcg2", name)
    if ".vdd_class = &vdd_cx" not in body:
        fail(f"{name} no longer votes the shared GCC vdd_cx rail")
    rate_match = re.search(
        r"\.rate_max\s*=\s*\(unsigned long\[VDD_NUM\]\)\s*\{([^}]*)\}",
        body,
        re.DOTALL,
    )
    if rate_match is None:
        fail(f"{name} has no VDD-indexed rate_max table")
    rates = {
        level: int(rate)
        for level, rate in re.findall(
            r"\[(VDD_[A-Z0-9_]+)\]\s*=\s*(\d+)", rate_match.group(1)
        )
    }
    if rates != expected:
        fail(f"{name} voltage/rate table changed: {rates!r}")


def validate_rcg_offset(gcc_source: str, name: str, expected: int) -> None:
    body = named_initializer(gcc_source, "struct clk_rcg2", name)
    match = re.search(r"\.cmd_rcgr\s*=\s*(0x[0-9a-fA-F]+)", body)
    if match is None or int(match.group(1), 16) != expected:
        fail(f"{name} command register offset changed")


def validate_rate_entry(
    gcc_source: str,
    name: str,
    expected_rate: int,
    expected_parent: str,
    expected_divisor: float,
    expected_source_select: int,
) -> None:
    clock_body = named_initializer(gcc_source, "struct clk_rcg2", name)
    table_match = re.search(r"\.freq_tbl\s*=\s*(\w+)", clock_body)
    map_match = re.search(r"\.parent_map\s*=\s*(\w+)", clock_body)
    if table_match is None or map_match is None:
        fail(f"{name} no longer declares its frequency table and parent map")
    table_body = named_initializer(
        gcc_source, "const struct freq_tbl", table_match.group(1)
    )
    entries = re.findall(
        r"F\(\s*(\d+)\s*,\s*(P_[A-Z0-9_]+)\s*,\s*([0-9.]+)\s*,",
        table_body,
    )
    expected_hid = round(2 * expected_divisor - 1)
    if not any(
        int(rate) == expected_rate
        and parent == expected_parent
        and round(2 * float(divisor) - 1) == expected_hid
        for rate, parent, divisor in entries
    ):
        fail(f"{name} lacks the expected {expected_rate} Hz source/divider row")

    parent_map_body = named_initializer(
        gcc_source, "const struct parent_map", map_match.group(1)
    )
    select_match = re.search(
        rf"\{{\s*{re.escape(expected_parent)}\s*,\s*(\d+)\s*\}}",
        parent_map_body,
    )
    if select_match is None or int(select_match.group(1)) != expected_source_select:
        fail(f"{name} source-select encoding changed for {expected_parent}")


def validate_branch_register(gcc_source: str, name: str, expected: int, halt: str) -> None:
    body = named_initializer(gcc_source, "struct clk_branch", name)
    for field in ("halt_reg", "enable_reg"):
        match = re.search(rf"\.{field}\s*=\s*(0x[0-9a-fA-F]+)", body)
        if match is None or int(match.group(1), 16) != expected:
            fail(f"{name} {field} changed")
    if not re.search(r"\.enable_mask\s*=\s*BIT\(0\)", body):
        fail(f"{name} no longer uses enable bit 0")
    if not re.search(rf"\.halt_check\s*=\s*{re.escape(halt)}\s*,", body):
        fail(f"{name} halt policy changed")


def require_order(body: str, name: str, *needles: str) -> None:
    positions = []
    for needle in needles:
        position = body.find(needle)
        if position < 0:
            fail(f"{name} no longer contains {needle}")
        positions.append(position)
    if positions != sorted(positions):
        fail(f"{name} call order changed: {', '.join(needles)}")


def dts_labeled_node(source: str, label: str) -> str:
    start = source.find(label)
    if start < 0:
        fail(f"captured runtime DT lacks {label}")
    open_brace = source.find("{", start)
    if open_brace < 0:
        fail(f"captured runtime DT has no body for {label}")
    depth = 0
    for index in range(open_brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[open_brace + 1 : index]
    fail(f"unterminated runtime DT node {label}")


def dts_u32_property(node: str, name: str) -> int:
    match = re.search(rf"\b{re.escape(name)}\s*=\s*<([^>]+)>;", node)
    if match is None:
        fail(f"runtime DT node lacks {name}")
    values = match.group(1).split()
    if len(values) != 1:
        fail(f"runtime DT property {name} is not one cell")
    try:
        return int(values[0], 0)
    except ValueError:
        fail(f"runtime DT property {name} is not a numeric cell")


def dts_u32_array_property(node: str, name: str) -> tuple[int, ...]:
    match = re.search(rf"\b{re.escape(name)}\s*=\s*<([^>]+)>;", node)
    if match is None:
        fail(f"runtime DT node lacks {name}")
    try:
        return tuple(int(value, 0) for value in match.group(1).split())
    except ValueError:
        fail(f"runtime DT property {name} is not a numeric cell array")


def c_define_int(source: str, name: str) -> int:
    match = re.search(
        rf"^#define\s+{re.escape(name)}\s+(\d+)\b", source, re.MULTILINE
    )
    if match is None:
        fail(f"pinned source lacks numeric definition of {name}")
    return int(match.group(1))


def main() -> None:
    source_root = (
        Path(sys.argv[1])
        if len(sys.argv) > 1
        else ROOT / ".cache/sunfish-lineage-kernel"
    )
    revision = subprocess.check_output(
        ["git", "-C", str(source_root), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision != PINNED_COMMIT:
        fail(f"kernel revision is {revision}, expected {PINNED_COMMIT}")
    diff = subprocess.run(
        ["git", "-C", str(source_root), "diff", "--quiet", PINNED_COMMIT, "--", *SOURCE_FILES],
        check=False,
    )
    if diff.returncode != 0:
        fail("one or more UFS startup source files differ from the pinned commit")
    dts_hash = hashlib.sha256(RUNTIME_DTS.read_bytes()).hexdigest()
    if dts_hash != RUNTIME_DTS_SHA256:
        fail(f"captured runtime DT hash is {dts_hash}, expected {RUNTIME_DTS_SHA256}")

    ufshcd = (source_root / SOURCE_FILES[0]).read_text()
    qcom = (source_root / SOURCE_FILES[1]).read_text()
    phy = (source_root / SOURCE_FILES[2]).read_text()
    pltfrm = (source_root / SOURCE_FILES[3]).read_text()
    gcc_header = (source_root / SOURCE_FILES[4]).read_text()
    gcc_source = (source_root / SOURCE_FILES[5]).read_text()
    rcg2_source = (source_root / SOURCE_FILES[6]).read_text()
    vdd_levels = (source_root / SOURCE_FILES[7]).read_text()
    rpmh_regulator = (source_root / SOURCE_FILES[8]).read_text()
    ufs_common = (source_root / SOURCE_FILES[9]).read_text()
    rpmh_regulator_driver = (source_root / SOURCE_FILES[10]).read_text()

    hba_init = function_body(ufshcd, "ufshcd_hba_init")
    require_order(
        hba_init,
        "ufshcd_hba_init",
        "ufshcd_setup_hba_vreg(hba, true)",
        "ufshcd_init_clocks(hba)",
        "ufshcd_enable_clocks(hba)",
        "ufshcd_setup_vreg(hba, true)",
        "ufshcd_variant_hba_init(hba)",
    )

    enable_clocks = function_body(ufshcd, "ufshcd_enable_clocks")
    if "ufshcd_setup_clocks(hba, true" not in enable_clocks:
        fail("generic host clock enable no longer invokes the common clock-list loop")
    clock_loop = function_body(ufshcd, "ufshcd_setup_clocks")
    if "list_for_each_entry(clki, head, list)" not in clock_loop or "clk_prepare_enable(clki->clk)" not in clock_loop:
        fail("generic clock setup no longer enables every parsed host-clock entry")

    init_clocks = function_body(ufshcd, "ufshcd_init_clocks")
    require_order(
        init_clocks,
        "ufshcd_init_clocks",
        "devm_clk_get(dev, clki->name)",
        "if (clki->max_freq)",
        "clk_set_rate(clki->clk, clki->max_freq)",
        "clki->curr_freq = clki->max_freq",
    )
    parse_clock_info = function_body(pltfrm, "ufshcd_parse_clock_info")
    require_order(
        parse_clock_info,
        "ufshcd_parse_clock_info",
        'of_property_count_strings(np, "clock-names")',
        'of_property_read_string_index(np,',
        "list_add_tail(&clki->list, &hba->clk_list_head)",
    )
    dts = RUNTIME_DTS.read_text()
    node_start = dts.find("ufshc_mem: ufshc@1d84000 {")
    node_end = dts.find("\n\t\t};", node_start)
    if node_start < 0 or node_end < 0:
        fail("captured runtime DT lacks the Sunfish UFS host node")
    ufs_node = dts[node_start:node_end]
    names_property = re.search(r"clock-names\s*=\s*([^;]+);", ufs_node, re.DOTALL)
    if names_property is None:
        fail("captured UFS node lacks clock-names")
    clock_names = tuple(re.findall(r'"([^"]+)"', names_property.group(1)))
    if clock_names != HOST_CLOCK_NAMES:
        fail(f"captured UFS host clocks changed: {clock_names!r}")
    clock_cells_property = re.search(r"\bclocks\s*=\s*<([^>]+)>;", ufs_node, re.DOTALL)
    if clock_cells_property is None:
        fail("captured UFS node lacks clocks provider specifiers")
    clock_cells = tuple(int(value, 0) for value in clock_cells_property.group(1).split())
    if len(clock_cells) != 2 * len(HOST_CLOCK_NAMES):
        fail(f"captured UFS clock cells have unexpected length: {clock_cells!r}")
    gcc_match = re.search(
        r"clock_gcc:\s+qcom,gcc@100000\s*\{(?P<body>.*?)\n\t\t\};",
        dts,
        re.DOTALL,
    )
    rpmh_match = re.search(
        r"clock_rpmh:\s+qcom,rpmh\s*\{(?P<body>.*?)\n\t\t\};",
        dts,
        re.DOTALL,
    )
    if gcc_match is None or 'compatible = "qcom,gcc-sdmmagpie"' not in gcc_match.group("body"):
        fail("captured GCC provider is not the Sunfish SDM Magpie clock controller")
    if rpmh_match is None or 'compatible = "qcom,rpmh-clk-sdmmagpie"' not in rpmh_match.group("body"):
        fail("captured RPMh clock provider is not SDM Magpie")
    gcc_phandle_match = re.search(r"phandle\s*=\s*<([^>]+)>;", gcc_match.group("body"))
    rpmh_phandle_match = re.search(r"phandle\s*=\s*<([^>]+)>;", rpmh_match.group("body"))
    if gcc_phandle_match is None or rpmh_phandle_match is None:
        fail("captured GCC or RPMh clock provider lacks a phandle")
    gcc_phandle = int(gcc_phandle_match.group(1), 0)
    rpmh_phandle = int(rpmh_phandle_match.group(1), 0)
    expected_clock_cells = tuple(
        value
        for provider, clock_id in zip(
            (gcc_phandle, gcc_phandle, gcc_phandle, gcc_phandle, gcc_phandle, rpmh_phandle, gcc_phandle, gcc_phandle),
            HOST_CLOCK_PROVIDER_IDS,
            strict=True,
        )
        for value in (provider, clock_id)
    )
    if clock_cells != expected_clock_cells:
        fail(f"captured UFS clock provider/IDs changed: {clock_cells!r}")
    for macro, expected_id in zip(HOST_CLOCK_GCC_MACROS, HOST_CLOCK_PROVIDER_IDS, strict=True):
        if macro is None:
            if expected_id != 0:
                fail("RPMh ref clock unexpectedly has a GCC clock ID")
            continue
        match = re.search(rf"^#define\s+{re.escape(macro)}\s+(\d+)\s*$", gcc_header, re.MULTILINE)
        if match is None or int(match.group(1)) != expected_id:
            fail(f"{macro} no longer maps to GCC ID {expected_id:#x}")
    frequency_property = re.search(r"freq-table-hz\s*=\s*<([^>]+)>;", ufs_node, re.DOTALL)
    if frequency_property is None:
        fail("captured UFS node lacks freq-table-hz")
    frequency_cells = tuple(int(value, 0) for value in frequency_property.group(1).split())
    expected_frequency_cells = tuple(value for pair in HOST_CLOCK_FREQ_TABLE_HZ for value in pair)
    if frequency_cells != expected_frequency_cells:
        fail(f"captured UFS host clock rates changed: {frequency_cells!r}")

    validate_clock_corner(
        gcc_source,
        "gcc_ufs_phy_axi_clk_src",
        {"VDD_LOWER": 50_000_000, "VDD_LOW": 100_000_000, "VDD_NOMINAL": 200_000_000, "VDD_HIGH": 240_000_000},
    )
    validate_clock_corner(
        gcc_source,
        "gcc_ufs_phy_unipro_core_clk_src",
        {"VDD_LOWER": 37_500_000, "VDD_LOW": 75_000_000, "VDD_NOMINAL": 150_000_000},
    )
    validate_clock_corner(
        gcc_source,
        "gcc_ufs_phy_ice_core_clk_src",
        {"VDD_LOWER": 75_000_000, "VDD_LOW": 150_000_000, "VDD_NOMINAL": 300_000_000},
    )
    validate_clock_corner(
        gcc_source,
        "gcc_ufs_phy_phy_aux_clk_src",
        {"VDD_LOWER": 19_200_000},
    )
    for name, rate, parent, divisor, source_select in (
        ("gcc_ufs_phy_axi_clk_src", 200_000_000, "P_GPLL0_OUT_MAIN", 3, 1),
        ("gcc_ufs_phy_unipro_core_clk_src", 150_000_000, "P_GPLL0_OUT_MAIN", 4, 1),
        ("gcc_ufs_phy_ice_core_clk_src", 300_000_000, "P_GPLL0_OUT_MAIN", 2, 1),
        ("gcc_ufs_phy_phy_aux_clk_src", 19_200_000, "P_BI_TCXO", 1, 0),
    ):
        validate_rate_entry(gcc_source, name, rate, parent, divisor, source_select)
    for name, offset in (
        ("gcc_ufs_phy_axi_clk_src", 0x77020),
        ("gcc_ufs_phy_unipro_core_clk_src", 0x77060),
        ("gcc_ufs_phy_ice_core_clk_src", 0x77048),
        ("gcc_ufs_phy_phy_aux_clk_src", 0x77098),
    ):
        validate_rcg_offset(gcc_source, name, offset)
        body = named_initializer(gcc_source, "struct clk_rcg2", name)
        if not re.search(r"\.flags\s*=\s*FORCE_ENABLE_RCG\b", body):
            fail(f"{name} no longer force-enables its RCG root")
    for macro, bit in (("CMD_UPDATE", 0), ("CMD_ROOT_EN", 1), ("CMD_ROOT_OFF", 31)):
        if not re.search(
            rf"^#define\s+{macro}\s+BIT\({bit}\)\s*$", rcg2_source, re.MULTILINE
        ):
            fail(f"RCG2 {macro} bit changed")
    enabled_body = function_body(rcg2_source, "clk_rcg2_is_enabled")
    if "cmd & CMD_ROOT_OFF" not in enabled_body:
        fail("RCG2 enabled-state check no longer uses the root-off status bit")
    calc_rate_match = re.search(
        r"clk_rcg2_calc_rate\s*\([^)]*\)\s*\{(?P<body>[^}]*)\}",
        rcg2_source,
        re.DOTALL,
    )
    if calc_rate_match is None or not re.search(
        r"if\s*\(hid_div\).*?mult_frac\(rate,\s*2,\s*hid_div\s*\+\s*1\)",
        calc_rate_match.group("body"),
        re.DOTALL,
    ):
        fail("RCG2 HID 0 no longer means parent rate, equivalent to HID 1")
    decoder = (ROOT / "rust/src/qcom_ufs_gcc.rs").read_text()
    if "if command & RCG_ROOT_ENABLE_REQUEST == 0" in decoder or (
        "if command & RCG_ROOT_OFF != 0" not in decoder
    ):
        fail("UFS GCC decoder does not follow Linux's ROOT_OFF active-state test")
    if "self.hid_divider == 1 && observed & RCG_CFG_DIVIDER_MASK == 0" not in decoder:
        fail("UFS GCC decoder does not accept both divide-by-one HID encodings")
    for name, offset, halt in (
        ("gcc_ufs_phy_ahb_clk", 0x77014, "BRANCH_HALT"),
        ("gcc_ufs_phy_axi_clk", 0x77038, "BRANCH_HALT"),
        ("gcc_ufs_phy_unipro_core_clk", 0x7708C, "BRANCH_HALT"),
        ("gcc_ufs_phy_ice_core_clk", 0x77090, "BRANCH_HALT"),
        ("gcc_ufs_phy_phy_aux_clk", 0x77094, "BRANCH_HALT"),
        ("gcc_ufs_phy_tx_symbol_0_clk", 0x77018, "BRANCH_HALT_SKIP"),
        ("gcc_ufs_phy_rx_symbol_0_clk", 0x7701C, "BRANCH_HALT_SKIP"),
        ("gcc_aggre_ufs_phy_axi_clk", 0x82024, "BRANCH_HALT"),
        ("gcc_ufs_mem_clkref_clk", 0x8C000, "BRANCH_HALT"),
    ):
        validate_branch_register(gcc_source, name, offset, halt)
    reset_id_match = re.search(
        r"^#define\s+GCC_UFS_PHY_BCR\s+(\d+)\s*$", gcc_header, re.MULTILINE
    )
    if reset_id_match is None or int(reset_id_match.group(1)) != 3:
        fail("GCC_UFS_PHY_BCR no longer has provider ID 3")
    if not re.search(
        r"\[GCC_UFS_PHY_BCR\]\s*=\s*\{\s*0x77000\s*\}", gcc_source
    ):
        fail("GCC UFS PHY reset line no longer maps to 0x77000")
    gdsc_start = dts.find("qcom,gdsc@177004 {")
    gdsc_end = dts.find("\n\t\t};", gdsc_start)
    if gdsc_start < 0 or gdsc_end < 0:
        fail("captured runtime DT lacks the UFS PHY GDSC node")
    gdsc_node = dts[gdsc_start:gdsc_end]
    if not re.search(r"reg\s*=\s*<0x177004\s+0x04>;", gdsc_node):
        fail("captured UFS PHY GDSC register moved")
    for level, macro in (
        ("VDD_MIN", "RPMH_REGULATOR_LEVEL_MIN_SVS"),
        ("VDD_LOWER", "RPMH_REGULATOR_LEVEL_LOW_SVS"),
        ("VDD_LOW", "RPMH_REGULATOR_LEVEL_SVS"),
        ("VDD_LOW_L1", "RPMH_REGULATOR_LEVEL_SVS_L1"),
        ("VDD_NOMINAL", "RPMH_REGULATOR_LEVEL_NOM"),
        ("VDD_HIGH", "RPMH_REGULATOR_LEVEL_TURBO"),
    ):
        if not re.search(rf"{re.escape(macro)}\s*,\s*/\*\s*{re.escape(level)}\s*\*/", vdd_levels):
            fail(f"{level} no longer maps to {macro} in the SDM Magpie VDD table")
    rpmh_level_values = {
        "RPMH_REGULATOR_LEVEL_OFFSET": 1,
        "RPMH_REGULATOR_LEVEL_MIN_SVS": 49,
        "RPMH_REGULATOR_LEVEL_LOW_SVS": 65,
        "RPMH_REGULATOR_LEVEL_SVS": 129,
        "RPMH_REGULATOR_LEVEL_SVS_L1": 193,
        "RPMH_REGULATOR_LEVEL_NOM": 257,
        "RPMH_REGULATOR_LEVEL_TURBO": 385,
    }
    for macro, value in rpmh_level_values.items():
        match = re.search(rf"^#define\s+{re.escape(macro)}\s+(.+)$", rpmh_regulator, re.MULTILINE)
        if match is None:
            fail(f"RPMh regulator binding lacks {macro}")
        expression = match.group(1).strip()
        if macro == "RPMH_REGULATOR_LEVEL_OFFSET":
            valid = expression == "1"
        else:
            valid = expression == f"({value - 1} + RPMH_REGULATOR_LEVEL_OFFSET)"
        if not valid:
            fail(f"{macro} no longer evaluates to {value}")

    gcc_cx_supply = re.search(r"vdd_cx-supply\s*=\s*<([^>]+)>;", gcc_match.group("body"))
    if gcc_cx_supply is None:
        fail("captured GCC node lacks the shared vdd_cx supply")
    cx_phandle = int(gcc_cx_supply.group(1), 0)
    cx_node_start = dts.find("rpmh-regulator-cxlvl {")
    cx_node_end = dts.find("\n\t\t};", cx_node_start)
    if cx_node_start < 0 or cx_node_end < 0:
        fail("captured runtime DT lacks the CX RPMh regulator node")
    cx_node = dts[cx_node_start:cx_node_end]
    if 'qcom,resource-name = "cx.lvl";' not in cx_node:
        fail("captured CX regulator is not the cx.lvl ARC resource")
    if not re.search(rf"phandle\s*=\s*<{cx_phandle:#x}>;", cx_node):
        fail("GCC vdd_cx supply does not resolve to the captured CX ARC regulator")

    init = function_body(ufshcd, "ufshcd_init")
    require_order(
        init,
        "ufshcd_init",
        "ufshcd_hba_init(hba)",
        "ufshcd_hba_capabilities(hba)",
        "ufshcd_get_ufs_version(hba)",
    )

    qcom_init = function_body(qcom, "ufs_qcom_init")
    require_order(
        qcom_init,
        "ufs_qcom_init",
        "phy_init(host->generic_phy)",
        "phy_power_on(host->generic_phy)",
        "ufs_qcom_init_lane_clks(host)",
        "ufs_qcom_set_bus_vote(hba, true)",
        "ufs_qcom_setup_clocks(hba, true, POST_CHANGE)",
    )

    qcom_clocks = function_body(qcom, "ufs_qcom_setup_clocks")
    require_order(
        qcom_clocks,
        "ufs_qcom_setup_clocks",
        "status == POST_CHANGE",
        "phy_power_on(host->generic_phy)",
        "ufs_qcom_ice_resume(host)",
    )
    if "ufs_qcom_enable_lane_clks" in qcom_clocks:
        fail("Qualcomm-specific lane-clock callback moved into setup_clocks")

    hce_enable = function_body(qcom, "ufs_qcom_hce_enable_notify")
    require_order(
        hce_enable,
        "ufs_qcom_hce_enable_notify",
        "ufs_qcom_power_up_sequence(hba)",
        "ufs_qcom_enable_lane_clks(host)",
        "ufs_qcom_ice_init(host)",
    )

    power_up = function_body(qcom, "ufs_qcom_power_up_sequence")
    require_order(
        power_up,
        "ufs_qcom_power_up_sequence",
        "ufs_qcom_assert_reset(hba)",
        "usleep_range(1000, 1100)",
        "ufs_qcom_phy_calibrate_phy(phy",
        "ufs_qcom_deassert_reset(hba)",
        "ufs_qcom_phy_start_serdes(phy)",
        "ufs_qcom_phy_is_pcs_ready(phy)",
    )

    phy_power_on = function_body(phy, "ufs_qcom_phy_power_on")
    phy_power_on = re.sub(r"\s+", " ", phy_power_on)
    require_order(
        phy_power_on,
        "ufs_qcom_phy_power_on",
        "ufs_qcom_phy_enable_vreg(dev, &phy_common->vdda_phy)",
        "power_control(phy_common, true)",
        "ufs_qcom_phy_enable_vreg(dev, &phy_common->vdda_pll)",
        "ufs_qcom_phy_enable_iface_clk(phy_common)",
        "ufs_qcom_phy_enable_ref_clk(phy_common)",
        "if (phy_common->vddp_ref_clk.reg)",
        "ufs_qcom_phy_enable_vreg(dev, &phy_common->vddp_ref_clk)",
    )

    phy_node = dts_labeled_node(dts, "ufsphy_mem:")
    host_node = dts_labeled_node(dts, "ufshc_mem:")
    pll_supply = dts_u32_property(phy_node, "vdda-pll-supply")
    ref_pad_supply = dts_u32_property(host_node, "qcom,vddp-ref-clk-supply")
    if pll_supply == 0 or ref_pad_supply != pll_supply:
        fail("UFS reference-clock pad vote no longer aliases the vdda-pll PMIC rail")
    ufs_cmd_db_map = (ROOT / "rust/src/qcom_ufs_rpmh_map.rs").read_text()
    if '["ldoa19", "ldoa12", "ldoa4", "ldoc3", "ldoc3"]' not in ufs_cmd_db_map:
        fail("UFS RPMh map no longer follows the source-derived regulator resource keys")
    supply_bindings = (
        ("ldoa19", "pm6150_l19:", host_node, "vcc-supply"),
        ("ldoa12", "pm6150_l12:", host_node, "vccq2-supply"),
        ("ldoa4", "pm6150_l4:", phy_node, "vdda-phy-supply"),
        ("ldoc3", "pm6150l_l3:", phy_node, "vdda-pll-supply"),
        ("ldoc3", "pm6150l_l3:", host_node, "qcom,vddp-ref-clk-supply"),
    )
    for resource_key, regulator_label, consumer_node, supply_property in supply_bindings:
        regulator_node = dts_labeled_node(dts, f"rpmh-regulator-{resource_key} {{")
        expected_name = f'qcom,resource-name = "{resource_key}";'
        if expected_name not in regulator_node:
            fail(f"runtime regulator node no longer maps to RPMh resource {resource_key}")
        supply_node = dts_labeled_node(dts, regulator_label)
        if dts_u32_property(supply_node, "qcom,set") != 3:
            fail(f"UFS client {regulator_label} no longer votes active and sleep sets")
        if dts_u32_property(consumer_node, supply_property) != dts_u32_property(
            supply_node, "phandle"
        ):
            fail(f"UFS {supply_property} no longer resolves to {resource_key}")
        if dts_u32_array_property(regulator_node, "qcom,supported-modes") != (2, 4):
            fail(f"RPMh regulator {resource_key} no longer declares LPM/HPM modes")
        if dts_u32_array_property(
            regulator_node, "qcom,mode-threshold-currents"
        ) != (0, 1):
            fail(f"RPMh regulator {resource_key} load thresholds changed")

    profile = (ROOT / "rust/src/qcom_ufs_rpmh_profile.rs").read_text()
    profile_blocks = re.findall(
        r'SupplySpec\s*\{(?=[^}]*resource_key:\s*")([^}]+)\}', profile
    )
    actual_profile = []
    for block in profile_blocks:
        key = re.search(r'resource_key:\s*"([^"]+)"', block)
        values = [
            re.search(rf"{field}:\s*(0x[0-9a-fA-F]+|[\d_]+)", block)
            for field in (
                "min_microvolts",
                "max_microvolts",
                "max_load_microamps",
            )
        ]
        set_mask = re.search(r"set_mask:\s*ACTIVE_SLEEP_SET", block)
        if key is None or any(value is None for value in values) or set_mask is None:
            fail("UFS RPMh profile contains an incomplete source-policy entry")
        actual_profile.append(
            (key.group(1), *(int(value.group(1), 0) for value in values))
        )
    expected_profile = [
        ("ldoa19", 0x2d0370, 0x2d2a80, 0x927c0),
        ("ldoa12", 0x1b7740, 0x1dc130, 0x927c0),
        ("ldoa4", 800_000, 925_000, 0xf5b4),
        ("ldoc3", 1_200_000, 1_800_000, 0x477c),
        ("ldoc3", 1_200_000, 1_200_000, 0x64),
    ]
    if actual_profile != expected_profile:
        fail(f"UFS RPMh request profile changed: {actual_profile!r}")

    for macro, expected in (
        ("VDDA_PHY_MIN_UV", 800_000),
        ("VDDA_PHY_MAX_UV", 925_000),
        ("VDDA_PLL_MIN_UV", 1_200_000),
        ("VDDA_PLL_MAX_UV", 1_800_000),
        ("VDDP_REF_CLK_MIN_UV", 1_200_000),
        ("VDDP_REF_CLK_MAX_UV", 1_200_000),
    ):
        if c_define_int(phy, macro) != expected:
            fail(f"PHY voltage limit {macro} changed")
    if c_define_int(rpmh_regulator_driver, "RPMH_REGULATOR_MODE_PMIC5_LDO_HPM") != 7:
        fail("PMIC5 LDO HPM command value changed")
    if c_define_int(rpmh_regulator, "RPMH_REGULATOR_MODE_HPM") != 4:
        fail("generic RPMh HPM mode identifier changed")
    if c_define_int(rpmh_regulator, "RPMH_REGULATOR_SET_ALL") != 3:
        fail("RPMh active-and-sleep set mask changed")
    load_body = re.sub(
        r"\s+", " ", function_body(rpmh_regulator_driver, "rpmh_regulator_vrm_set_load")
    )
    if "min_load_ua <= load_ua" not in load_body or "rpmh_regulator_vrm_set_mode_index(vreg, i)" not in load_body:
        fail("RPMh VRM load-to-mode selection changed")
    voltage_body = re.sub(
        r"\s+", " ", function_body(rpmh_regulator_driver, "rpmh_regulator_vrm_set_voltage")
    )
    if "DIV_ROUND_UP(min_uv, 1000)" not in voltage_body or "mv * 1000 > max_uv" not in voltage_body:
        fail("RPMh VRM voltage rounding or upper-bound check changed")
    if "PMIC5_LDO_HPM_MODE: u32 = 7;" not in profile or "HPM_MIN_LOAD_UA: u32 = 1;" not in profile:
        fail("UFS request profile no longer follows the source-selected PMIC mode")
    if c_define_int(ufs_common, "UFS_VREG_LPM_LOAD_UA") != 1000:
        fail("UFS low-power load constant changed")
    expected_properties = {
        (phy_node, "vdda-phy-max-microamp"): (0xF5B4,),
        (phy_node, "vdda-pll-max-microamp"): (0x477C,),
        (host_node, "qcom,vddp-ref-clk-max-microamp"): (0x64,),
        (host_node, "vcc-max-microamp"): (0x927C0,),
        (host_node, "vccq2-max-microamp"): (0x927C0,),
        (host_node, "vcc-voltage-level"): (0x2D0370, 0x2D2A80),
        (host_node, "vccq2-voltage-level"): (0x1B7740, 0x1DC130),
    }
    for (node, property_name), expected in expected_properties.items():
        match = re.search(rf"\b{re.escape(property_name)}\s*=\s*<([^>]+)>;", node)
        if match is None:
            fail(f"runtime UFS DT lacks {property_name}")
        values = tuple(int(value, 0) for value in match.group(1).split())
        if values != expected:
            fail(f"runtime UFS DT {property_name} changed: {values!r}")

    platform = (ROOT / "rust/src/qcom_ufs_platform.rs").read_text()
    if "pub const SUPPLY_VDDP_REF_CLK: u8 = 1 << 4;" not in platform:
        fail("UFS native resource map omits the distinct vddp-ref-clk consumer vote")
    if "| SUPPLY_VDDP_REF_CLK;" not in platform:
        fail("UFS required regulator-client mask omits vddp-ref-clk")

    print(
        "PASS: UFS source sets nonzero DT max clock rates then enables every listed clock before CAP/VER, including lane-sync clocks; "
        "the three rate-capped GCC clocks require shared vdd_cx at the nominal corner via cx.lvl; "
        "GDSC, reset, RCG ROOT_OFF state, HID-zero divide-by-one semantics, and branch offsets match the read-only snapshot; "
        "HBA/rails/PHY/bus setup (including the separate shared-rail vddp-ref-clk vote) precedes CAP/VER, then HCE runs reset/SerDes, "
        "Qualcomm lane-clock enable, and ICE init"
    )


if __name__ == "__main__":
    main()
