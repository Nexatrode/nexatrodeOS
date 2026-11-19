#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/aop-read-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test -f "$dtb"
# Refuse a stale hard-coded map if the SDK's captured platform DT differs.
aop_node=/soc/qcom,qmp-aop@c300000
test "$(fdtget -t s "$dtb" "$aop_node" compatible)" = "qcom,qmp-mbox"
test "$(fdtget -t x "$dtb" "$aop_node" reg)" = "c300000 1000 17c0000c 4"
test "$(fdtget -t x "$dtb" "$aop_node" mbox-desc-offset)" = "0"
mkdir -p "$out_dir"

cd "$root_dir"
python3 -m unittest tests.sunfish_aop_probe_isolation_test
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_aop_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_aop_probe.elf \
    "$out_dir/aop_read_probe.elf"

"$sdk/bin/microkit" "$port_dir/aop-read-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/aop-read-probe.Image"
lz4 -z -f -q "$out_dir/aop-read-probe.Image" "$out_dir/aop-read-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/aop-read-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built read-only AOP mailbox probe: %s\n' "$out_dir/fastboot.img"
