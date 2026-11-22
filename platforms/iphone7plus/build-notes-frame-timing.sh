#!/bin/sh
# Build matched Device and Normal-NC Notes images with an EL0 frame timer.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
base_sel4="$root_dir/.cache/sel4-src-microkit231-explicit-nc"
base_microkit="$root_dir/.cache/microkit-src-2.3.1-explicit-nc"
test_sel4="$root_dir/.cache/sel4-src-microkit231-explicit-nc-timing"
test_microkit="$root_dir/.cache/microkit-src-2.3.1-explicit-nc-timing"
sdk_dir="$test_microkit/release/microkit-sdk-2.3.1-iphone7plus-16k-explicit-nc-timing"
target_elf="$root_dir/experiments/slint-notes/target/aarch64-sel4-microkit-iphone16k/release/slint_notes_iphone.elf"
touch_elf="$root_dir/build/iphone7plus/nexatrode-slint-notes/touch-service.elf"
objcopy=$(command -v llvm-objcopy || true)
if [ -z "$objcopy" ]; then objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi

if [ ! -d "$base_sel4" ] || [ ! -f "$base_microkit/build_sdk.py" ] ||
   [ ! -x "$base_microkit/release/microkit-sdk-2.3.1-iphone7plus-16k-explicit-nc/bin/microkit" ] ||
   [ ! -f "$touch_elf" ] || [ ! -x "$objcopy" ]; then
    echo 'Build the explicit Normal-NC SDK and Notes image first.' >&2
    exit 1
fi

if [ ! -d "$test_sel4" ]; then
    cp -R "$base_sel4" "$test_sel4"
    patch -d "$test_sel4" -p1 < "$root_dir/patches/sel4-iphone7plus-vcnt-timing.patch"
fi
if [ ! -d "$test_microkit" ]; then
    mkdir -p "$test_microkit"
    rsync -a --exclude '/build/' --exclude '/build-*/' --exclude '/target/' \
        --exclude '/release/' --exclude '/__pycache__/' \
        "$base_microkit/" "$test_microkit/"
    patch -d "$test_microkit" -p1 < "$root_dir/patches/microkit-iphone7plus-vcnt-timing.patch"
fi
grep -q 'set(KernelArmHaveGenericTimer ON)' "$test_sel4/src/drivers/timer/config.cmake"
grep -q '"KernelArmExportVCNTUser": True' "$test_microkit/build_sdk.py"

export PATH="$root_dir/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
cd "$test_microkit"
"$root_dir/.cache/sel4-venv/bin/python" build_sdk.py \
    --sel4 "$test_sel4" --llvm --boards iphone7plus --configs release \
    --version 2.3.1-iphone7plus-16k-explicit-nc-timing \
    --build-dir build-16k-explicit-nc-timing --skip-tool --skip-docs --skip-tar
mkdir -p "$sdk_dir/bin"
cp "$base_microkit/release/microkit-sdk-2.3.1-iphone7plus-16k-explicit-nc/bin/microkit" \
   "$sdk_dir/bin/microkit"
grep -q '#define CONFIG_EXPORT_VCNT_USER  1' \
    "$sdk_dir/board/iphone7plus/release/include/kernel/gen_config.h"

cd "$root_dir"
SEL4_INCLUDE_DIRS="$sdk_dir/board/iphone7plus/release/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --features iphone-pd,frame-timing --bin slint_notes_iphone \
    --target rust/targets/aarch64-sel4-microkit-iphone16k.json \
    -Z build-std=core,alloc -Z json-target-spec

for variant in normal-nc device; do
    out_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes-timing-$variant"
    if [ "$variant" = normal-nc ]; then
        system="$root_dir/platforms/iphone7plus/nexatrode-slint-notes-explicit-nc.system"
    else
        system="$root_dir/platforms/iphone7plus/nexatrode-slint-notes.system"
    fi
    mkdir -p "$out_dir"
    cp "$target_elf" "$out_dir/slint_notes_iphone.elf"
    cp "$touch_elf" "$out_dir/touch-service.elf"
    "$sdk_dir/bin/microkit" "$system" --search-path "$out_dir" \
        --board iphone7plus --config release --image-type elf \
        -o "$out_dir/loader.elf" -r "$out_dir/report.txt"
    "$objcopy" -O binary "$out_dir/loader.elf" "$out_dir/loader.bin"
    shasum -a 256 "$out_dir/loader.bin"
done
