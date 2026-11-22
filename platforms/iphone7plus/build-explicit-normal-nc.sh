#!/bin/sh
# Build a separate SDK and Notes image with an explicit Normal-NC screen map.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
base_sel4="$root_dir/.cache/sel4-src-microkit231"
base_rust="$root_dir/.cache/rust-sel4-iphone7plus16k"
base_microkit="$root_dir/.cache/microkit-src-2.3.1"
test_sel4="$root_dir/.cache/sel4-src-microkit231-explicit-nc"
test_rust="$root_dir/.cache/rust-sel4-iphone7plus16k-explicit-nc"
test_microkit="$root_dir/.cache/microkit-src-2.3.1-explicit-nc"
sdk_dir="$test_microkit/release/microkit-sdk-2.3.1-iphone7plus-16k-explicit-nc"
notes_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes"
out_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes-explicit-nc"
objcopy=$(command -v llvm-objcopy || true)
if [ -z "$objcopy" ]; then objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi

if [ ! -d "$base_sel4" ] || [ ! -d "$base_rust" ] ||
   [ ! -f "$base_microkit/build_sdk.py" ] ||
   [ ! -f "$notes_dir/slint_notes_iphone.elf" ] ||
   [ ! -f "$notes_dir/touch-service.elf" ] ||
   [ ! -x "$objcopy" ]; then
    echo 'Build the base 16 KiB SDK and native Slint Notes image first.' >&2
    exit 1
fi

if [ ! -d "$test_sel4" ]; then
    cp -R "$base_sel4" "$test_sel4"
    patch -d "$test_sel4" -p1 < "$root_dir/patches/sel4-iphone7plus-explicit-normal-nc.patch"
fi
if [ ! -d "$test_rust" ]; then
    cp -R "$base_rust" "$test_rust"
    patch -d "$test_rust" -p1 < "$root_dir/patches/rust-sel4-iphone7plus-explicit-normal-nc.patch"
fi
if [ ! -d "$test_microkit" ]; then
    mkdir -p "$test_microkit"
    rsync -a --exclude '/build/' --exclude '/build-*/' --exclude '/target/' \
        --exclude '/release/' --exclude '/__pycache__/' \
        "$base_microkit/" "$test_microkit/"
    patch -d "$test_microkit" -p1 < "$root_dir/patches/microkit-iphone7plus-explicit-normal-nc.patch"
fi

# Fail early if a partial checkout or stale patch has been supplied.
grep -q 'armNormalNonCacheable' "$test_sel4/src/arch/arm/64/kernel/vspace.c"
grep -q 'normal_nc: bool' "$test_rust/crates/sel4-capdl-initializer/types/src/spec.rs"
grep -q 'normal_nc: bool' "$test_microkit/tool/microkit/src/sdf/memory_region.rs"

export PATH="$root_dir/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
cd "$test_microkit"
cargo test -p microkit-tool --locked --offline
cargo build -p microkit-tool --release --locked --offline
"$root_dir/.cache/sel4-venv/bin/python" build_sdk.py \
    --sel4 "$test_sel4" --llvm --boards iphone7plus --configs release \
    --version 2.3.1-iphone7plus-16k-explicit-nc \
    --build-dir build-16k-explicit-nc --skip-tool --skip-docs --skip-tar
mkdir -p "$sdk_dir/bin" "$out_dir"
cp target/release/microkit "$sdk_dir/bin/microkit"
cp "$notes_dir/slint_notes_iphone.elf" "$out_dir/"
cp "$notes_dir/touch-service.elf" "$out_dir/"
"$sdk_dir/bin/microkit" "$root_dir/platforms/iphone7plus/nexatrode-slint-notes-explicit-nc.system" \
    --search-path "$out_dir" --board iphone7plus --config release \
    --image-type elf -o "$out_dir/loader.elf" -r "$out_dir/report.txt"
"$objcopy" -O binary "$out_dir/loader.elf" "$out_dir/loader.bin"
shasum -a 256 "$out_dir/loader.bin"
