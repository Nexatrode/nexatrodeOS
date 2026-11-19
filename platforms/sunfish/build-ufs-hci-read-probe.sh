#!/bin/sh
# Build a temporary UFSHCI register snapshot. Never flash or issue UFS commands.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/ufs-hci-read-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test -f "$dtb"

reg=$(fdtget -t x "$dtb" /soc/ufshc@1d84000 reg)
test "$reg" = "1d84000 3000"

mkdir -p "$out_dir"
cd "$root_dir"
python3 "$port_dir/validate-ufs-hci-read-probe.py" \
    --system "$port_dir/ufs-hci-read-probe.system"
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    CARGO_TARGET_DIR="$out_dir/target" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_ufs_hci_read_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp "$out_dir/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_ufs_hci_read_probe.elf" \
    "$out_dir/ufs_hci_read_probe.elf"

"$sdk/bin/microkit" "$port_dir/ufs-hci-read-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt" \
    --capdl-json "$out_dir/capdl.json"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/ufs-hci-read-probe.Image"
lz4 -z -f -q "$out_dir/ufs-hci-read-probe.Image" \
    "$out_dir/ufs-hci-read-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/ufs-hci-read-probe.Image.lz4" \
    "$out_dir/fastboot.img"

printf 'Built temporary UFSHCI read-only probe (not booted): %s\n' "$out_dir/fastboot.img"
