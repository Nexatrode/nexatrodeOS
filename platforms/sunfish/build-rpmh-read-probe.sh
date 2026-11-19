#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/rpmh-read-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test -f "$dtb"
test "$(fdtget -t s "$dtb" /soc/qcom,cmd-db@c3f000c compatible)" = "qcom,cmd-db"
test "$(fdtget -t x "$dtb" /soc/qcom,cmd-db@c3f000c reg)" = "c3f000c 8"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 reg)" = "18220000 100 18220d00 3000"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,drv-id)" = "2"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,tcs-config)" = "2 2 0 3 1 3 3 1"
test "$(fdtget -t x "$dtb" /reserved-memory/xbl_aop_mem@85e00000 reg)" = "0 85e00000 0 1ff000"
mkdir -p "$out_dir"

cd "$root_dir"
python3 - <<'PY'
import json
from pathlib import Path
observed = json.loads(Path('platforms/sunfish/rpmh-handoff.json').read_text())
assert observed['linux_claim_start'] == '0x85f20000'
assert observed['linux_claim_end_inclusive'] == '0x85f21078'
assert observed['snapshot_mapping_base'] == '0x85f20000'
assert observed['snapshot_mapping_bytes'] == '0x2000'
PY
python3 -m unittest tests.sunfish_rpmh_read_probe_isolation_test
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_rpmh_read_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_rpmh_read_probe.elf \
    "$out_dir/rpmh_read_probe.elf"

"$sdk/bin/microkit" "$port_dir/rpmh-read-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt" \
    --read-only-cmd-db-system "$port_dir/rpmh-read-probe.system"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/rpmh-read-probe.Image"
lz4 -z -f -q "$out_dir/rpmh-read-probe.Image" "$out_dir/rpmh-read-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/rpmh-read-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built read-only RPMh resource probe: %s\n' "$out_dir/fastboot.img"
