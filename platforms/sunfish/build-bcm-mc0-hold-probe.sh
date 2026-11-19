#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/bcm-mc0-hold-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test "$(fdtget -t s "$dtb" /soc/mailbox@18220000 label)" = "apps_rsc"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 reg)" = "18220000 100 18220d00 3000"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,drv-id)" = "2"
test "$(fdtget -t x "$dtb" /soc/mailbox@18220000 qcom,tcs-config)" = "2 2 0 3 1 3 3 1"
mkdir -p "$out_dir"
cd "$root_dir"
python3 - <<'PY'
import json
from pathlib import Path
d = json.loads(Path('platforms/sunfish/rpmh-handoff.json').read_text())
metadata = d['bcm_read_observations'][-1]
assert metadata['result'] == 'METADATA PASS'
expected_addresses = {'MC0': 0x50000, 'ACV': 0x5000c, 'CE0': 0x500a0,
                      'SN11': 0x5006c, 'SN0': 0x50078, 'SH0': 0x5003c}
for name, address in expected_addresses.items():
    assert int(metadata['resources'][name]['address'], 0) == address
mc0 = metadata['resources']['MC0']
assert (mc0['unit_size'], mc0['width'], mc0['clock_domain']) == (1770000, 16, 0)
observed = d['bcm_reads_observations'][-1]
assert observed['result'] == 'BCM READ PASS'
assert (observed['reads_completed'], observed['handoff_checks']) == (12, 24)
expected = {'MC0': 0x29917f75, 'ACV': 0x20000008, 'CE0': 0,
            'SN11': 0x213c44f1, 'SN0': 0x213c44f1, 'SH0': 0x24005adf}
for pass_name in ['responses_first', 'responses_second']:
    assert {name: int(raw, 0) for name, raw in observed[pass_name].items()} == expected
assert observed['bank_programming_preserved']
assert (expected['MC0'] | (1 << 30)) == 0x69917f75
assert (expected['MC0'] & 0x3fff) >= 15819
PY
python3 "$port_dir/validate-bcm-routes.py" "$dtb" > "$out_dir/bus-reference.json"
python3 -m unittest tests.sunfish_bcm_reads_probe_test
rustc +stable --edition 2024 --test tests/sunfish_bcm_reads_mmio_test.rs \
    -o "$out_dir/bcm-mmio-test"
"$out_dir/bcm-mmio-test" --quiet
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_bcm_mc0_hold_probe \
    --bin rust_sunfish_bcm_reads_probe --bin rust_sunfish_rpmh_cx_read_probe \
    --bin rust_sunfish_rpmh_resource_reads_probe --bin rust_sunfish_rpmh_cx_turbo_hold_probe \
    --bin rust_sunfish_rpmh_resources_hold_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_bcm_mc0_hold_probe.elf \
    "$out_dir/bcm_mc0_hold_probe.elf"
"$sdk/bin/microkit" "$port_dir/bcm-mc0-hold-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/bcm-mc0-hold-probe.Image"
lz4 -z -f -q "$out_dir/bcm-mc0-hold-probe.Image" "$out_dir/bcm-mc0-hold-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/bcm-mc0-hold-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built fixed MC0 hold probe (no modem start): %s\n' "$out_dir/fastboot.img"
