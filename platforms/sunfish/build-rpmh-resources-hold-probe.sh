#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/rpmh-resources-hold-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test "$(fdtget -t s "$dtb" /soc/mailbox@18220000 label)" = "apps_rsc"
test "$(fdtget -t s "$dtb" /soc/mailbox@18220000 compatible)" = "qcom,tcs-drv"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 reg)" = "18220000 100 18220d00 3000"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,drv-id)" = "2"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,tcs-config)" = "2 2 0 3 1 3 3 1"
test "$(fdtget -t s "$dtb" /soc/qcom,mss@4080000 qcom,proxy-reg-names)" = "vdd_cx vdd_mss"
test "$(fdtget -t s "$dtb" /soc/qcom,mss@4080000 qcom,proxy-clock-names)" = "xo"
test "$(fdtget -t x "$dtb" /soc/qcom,mss@4080000 qcom,vdd_cx-uV-uA)" = "181 186a0"
test "$(fdtget -t x "$dtb" /soc/qcom,mss@4080000 qcom,vdd_mss-uV-uA)" = "181 186a0"
mkdir -p "$out_dir"
cd "$root_dir"
python3 - <<'PY'
import json
from pathlib import Path
observed = json.loads(Path('platforms/sunfish/rpmh-handoff.json').read_text())
resources = observed['native_observations'][-1]['resources']
for name, address, selector in [('cx.lvl', '0x00030000', 6), ('mss.lvl', '0x00030060', 7)]:
    assert resources[name]['address'] == address
    assert resources[name]['turbo_selector'] == selector
    assert resources[name]['consumer_level'] == 385
assert resources['xo.lvl']['address'] == '0x00030090'
assert observed['cx_turbo_hold_observations'][-1]['result'] == 'CX TURBO HOLD PASS'
assert observed['cx_turbo_hold_observations'][-1]['after_second'] == {
    'cx.lvl': 6, 'mss.lvl': 0, 'mx.lvl': 3, 'xo.lvl': 3}
source = Path('.cache/sunfish-lineage-kernel/drivers/clk/qcom/clk-rpmh.c').read_text()
assert '"xo.lvl", 0x3, 0x0,' in source
PY
python3 -m unittest tests.sunfish_rpmh_cx_read_probe_isolation_test
rustc +stable --edition=2024 --test tests/sunfish_rpmh_mmio_test.rs \
    -o "$out_dir/rpmh-mmio-tests"
"$out_dir/rpmh-mmio-tests"
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_rpmh_resources_hold_probe \
    --bin rust_sunfish_rpmh_cx_turbo_hold_probe --bin rust_sunfish_rpmh_resource_reads_probe \
    --bin rust_sunfish_rpmh_cx_read_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_rpmh_resources_hold_probe.elf \
    "$out_dir/rpmh_resources_hold_probe.elf"
"$sdk/bin/microkit" "$port_dir/rpmh-resources-hold-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/rpmh-resources-hold-probe.Image"
lz4 -z -f -q "$out_dir/rpmh-resources-hold-probe.Image" "$out_dir/rpmh-resources-hold-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/rpmh-resources-hold-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built CX/MSS/XO resource hold probe (no PAS/reset): %s\n' "$out_dir/fastboot.img"
