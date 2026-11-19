#!/bin/sh
# Isolated eight-app Slint iPhone image with on-screen render timings.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1-explicit-nc-timing/release/microkit-sdk-2.3.1-iphone7plus-16k-explicit-nc-timing"
out_dir="$root_dir/build/iphone7plus/nexatrode-slint-full-suite"
target_elf="$root_dir/experiments/slint-notes/target/aarch64-sel4-microkit-iphone16k/release/slint_notes_iphone.elf"
board_dir="$sdk_dir/board/iphone7plus/release"
objcopy=$(command -v llvm-objcopy || true)
if [ -z "$objcopy" ]; then objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi

if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -x "$objcopy" ]; then
    echo 'The explicit Normal-NC timing SDK and llvm-objcopy are required.' >&2
    exit 1
fi

export PATH="$root_dir/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
cd "$root_dir"
mkdir -p "$out_dir"
SEL4_INCLUDE_DIRS="$sdk_dir/board/iphone7plus/release/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --features full-suite-profile --bin slint_notes_iphone \
    --target rust/targets/aarch64-sel4-microkit-iphone16k.json \
    -Z build-std=core,alloc -Z json-target-spec

cp "$target_elf" "$out_dir/slint_notes_iphone.elf"
sh "$root_dir/platforms/iphone7plus/build-touch-service.sh" "$board_dir" "$out_dir"
python3 "$root_dir/platforms/iphone7plus/add-slint-frame-wake.py" \
    "$root_dir/platforms/iphone7plus/nexatrode-slint-notes-explicit-nc.system" "$out_dir/system.xml"
"$sdk_dir/bin/microkit" \
    "$out_dir/system.xml" \
    --search-path "$out_dir" --board iphone7plus --config release --image-type elf \
    -o "$out_dir/loader.elf" -r "$out_dir/report.txt"
"$objcopy" -O binary "$out_dir/loader.elf" "$out_dir/loader.bin"
shasum -a 256 "$out_dir/loader.bin"
