#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/ipa-path-bcm-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test "$(fdtget -t s "$dtb" /soc/qcom,cmd-db@c3f000c compatible)" = "qcom,cmd-db"
test "$(fdtget -t x "$dtb" /soc/qcom,cmd-db@c3f000c reg)" = "c3f000c 8"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 reg)" = "18220000 100 18220d00 3000"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,drv-id)" = "2"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,tcs-config)" = "2 2 0 3 1 3 3 1"
mkdir -p "$out_dir"
cd "$root_dir"
python3 "$port_dir/validate-bcm-routes.py" "$dtb" > "$out_dir/bus-reference.json"
python3 - <<'PY'
import json
from pathlib import Path

data = json.loads(Path('.cache/pixel-sunfish/ipa-path-bcm-probe/bus-reference.json').read_text())
routes = data['routes']
ipa_keys = {'ipa_ebi', 'ipa_ocimem', 'ipa_cfg', 'ipa_core'}
assert ipa_keys <= set(routes)
names = {bcm for key in ipa_keys for node in routes[key] for bcm in node['bcms']}
expected = {'SN11', 'SN0', 'SH0', 'MC0', 'ACV', 'SN1', 'SH5', 'SH2', 'SN15', 'CN0', 'IP0'}
assert names == expected, (names, expected)
PY
python3 -m unittest tests.sunfish_ipa_path_bcm_probe_test
rustc +stable --edition 2024 --test tests/sunfish_bcm_reads_mmio_test.rs \
    -o "$out_dir/ipa-path-bcm-mmio-test"
"$out_dir/ipa-path-bcm-mmio-test" --quiet
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_ipa_path_bcm_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_ipa_path_bcm_probe.elf \
    "$out_dir/ipa_path_bcm_probe.elf"
"$sdk/bin/microkit" "$port_dir/ipa-path-bcm-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt" \
    --read-only-cmd-db-system "$port_dir/ipa-path-bcm-probe.system"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/ipa-path-bcm-probe.Image"
lz4 -z -f -q "$out_dir/ipa-path-bcm-probe.Image" "$out_dir/ipa-path-bcm-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/ipa-path-bcm-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built read-only IPA-route BCM probe: %s\n' "$out_dir/fastboot.img"
