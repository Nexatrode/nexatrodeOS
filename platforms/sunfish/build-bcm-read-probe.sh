#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/bcm-read-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test "$(fdtget -t s "$dtb" /soc/qcom,cmd-db@c3f000c compatible)" = "qcom,cmd-db"
test "$(fdtget -t x "$dtb" /soc/qcom,cmd-db@c3f000c reg)" = "c3f000c 8"
test "$(fdtget -t x "$dtb" /reserved-memory/xbl_aop_mem@85e00000 reg)" = "0 85e00000 0 1ff000"
mkdir -p "$out_dir"
cd "$root_dir"
python3 "$port_dir/validate-bcm-routes.py" "$dtb" > "$out_dir/bus-reference.json"
python3 -m unittest tests.sunfish_bcm_probe_test
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_bcm_read_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_bcm_read_probe.elf \
    "$out_dir/bcm_read_probe.elf"
"$sdk/bin/microkit" "$port_dir/bcm-read-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt" \
    --read-only-cmd-db-system "$port_dir/bcm-read-probe.system"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/bcm-read-probe.Image"
lz4 -z -f -q "$out_dir/bcm-read-probe.Image" "$out_dir/bcm-read-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/bcm-read-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built read-only bus metadata probe: %s\n' "$out_dir/fastboot.img"
