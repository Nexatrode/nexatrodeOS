#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1/release/microkit-sdk-2.3.1-iphone7plus-16k"
board_dir="$sdk_dir/board/iphone7plus/release"
output_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes"
objcopy=$(command -v llvm-objcopy || true)
if [ -z "$objcopy" ]; then objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi
if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -x "$objcopy" ]; then
    echo 'The 16 KiB iPhone Microkit SDK and llvm-objcopy are required.' >&2
    exit 1
fi

sh "$root_dir/platforms/iphone7plus/build-nexatrode-shell.sh" touch
SEL4_INCLUDE_DIRS="$board_dir/include" cargo +nightly-2026-09-25 build \
    --locked --offline --release \
    --manifest-path "$root_dir/experiments/slint-notes/Cargo.toml" \
    --features iphone-pd --bin slint_notes_iphone \
    --target "$root_dir/rust/targets/aarch64-sel4-microkit-iphone16k.json" \
    -Z build-std=core,alloc -Z json-target-spec

mkdir -p "$output_dir"
cp "$root_dir/experiments/slint-notes/target/aarch64-sel4-microkit-iphone16k/release/slint_notes_iphone.elf" \
    "$output_dir/slint_notes_iphone.elf"
cp "$root_dir/build/iphone7plus/nexatrode-touch-shell/touch-service.elf" \
    "$output_dir/touch-service.elf"
"$sdk_dir/bin/microkit" \
    "$root_dir/platforms/iphone7plus/nexatrode-slint-notes.system" \
    --search-path "$output_dir" --board iphone7plus --config release \
    --image-type elf -o "$output_dir/loader.elf" -r "$output_dir/report.txt"
"$objcopy" -O binary "$output_dir/loader.elf" "$output_dir/loader.bin"
echo "Built native Slint Notes image: $output_dir/loader.bin"
wc -c < "$output_dir/loader.bin"
