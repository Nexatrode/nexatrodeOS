#!/bin/sh
# Build a temporary read-only GCC snapshot; never start UFS or write a register.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/ufs-gcc-snapshot-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
dtb="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test -f "$dtb"
reg=$(fdtget -t x "$dtb" /soc/qcom,gcc@100000 reg)
test "$reg" = "100000 1f0000"

mkdir -p "$out_dir"
cd "$root_dir"
python3 "$port_dir/validate-ufs-gcc-snapshot-probe.py" \
    --system "$port_dir/ufs-gcc-snapshot-probe.system" \
    --source rust/src/sunfish_ufs_gcc_snapshot_probe.rs \
    --decoder rust/src/qcom_ufs_gcc.rs
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    CARGO_TARGET_DIR="$out_dir/target" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --bin rust_sunfish_ufs_gcc_snapshot_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp "$out_dir/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_ufs_gcc_snapshot_probe.elf" \
    "$out_dir/ufs_gcc_snapshot_probe.elf"

"$sdk/bin/microkit" "$port_dir/ufs-gcc-snapshot-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt" \
    --capdl-json "$out_dir/capdl.json"
python3 "$port_dir/validate-ufs-gcc-snapshot-probe.py" \
    --system "$port_dir/ufs-gcc-snapshot-probe.system" \
    --source "$root_dir/rust/src/sunfish_ufs_gcc_snapshot_probe.rs" \
    --decoder "$root_dir/rust/src/qcom_ufs_gcc.rs" \
    --capdl-json "$out_dir/capdl.json"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/ufs_gcc_snapshot_probe.Image"
lz4 -z -f -q "$out_dir/ufs_gcc_snapshot_probe.Image" \
    "$out_dir/ufs_gcc_snapshot_probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/ufs_gcc_snapshot_probe.Image.lz4" \
    "$out_dir/fastboot.img"

printf 'Built temporary read-only UFS GCC snapshot (not booted): %s\n' "$out_dir/fastboot.img"
