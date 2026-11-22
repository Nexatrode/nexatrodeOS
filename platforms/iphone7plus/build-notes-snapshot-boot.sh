#!/bin/sh
# Pre-render the exact Notes default frame, then build its isolated fast-splash image.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1-explicit-nc-timing/release/microkit-sdk-2.3.1-iphone7plus-16k-explicit-nc-timing"
out_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes-snapshot-boot"
target_elf="$root_dir/experiments/slint-notes/target/aarch64-sel4-microkit-iphone16k/release/slint_notes_iphone.elf"
touch_elf="$root_dir/build/iphone7plus/nexatrode-slint-notes/touch-service.elf"
asset="$root_dir/experiments/slint-notes/assets/notes-1080x1920.rle"
objcopy=$(command -v llvm-objcopy || true)
if [ -z "$objcopy" ]; then objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi

if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -f "$touch_elf" ] || [ ! -x "$objcopy" ]; then
    echo 'Build the frame-timing SDK and touch-service image first.' >&2
    exit 1
fi

export PATH="$root_dir/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
cd "$root_dir"
mkdir -p "$out_dir" "$(dirname "$asset")"
cargo run --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --bin personal-os-slint-notes-spike -- \
    1080 1920 "$out_dir/snapshot.ppm" 2.6086957 --compare-line
cargo run --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --bin personal-os-slint-notes-spike -- \
    1080 1920 "$out_dir/reused.ppm" 2.6086957 --reused-buffer
cmp "$out_dir/snapshot.ppm" "$out_dir/reused.ppm"
cargo run --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --bin encode_snapshot -- "$out_dir/snapshot.ppm" "$asset"

SEL4_INCLUDE_DIRS="$sdk_dir/board/iphone7plus/release/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --features iphone-pd,snapshot-boot --bin slint_notes_iphone \
    --target rust/targets/aarch64-sel4-microkit-iphone16k.json \
    -Z build-std=core,alloc -Z json-target-spec

cp "$target_elf" "$out_dir/slint_notes_iphone.elf"
cp "$touch_elf" "$out_dir/touch-service.elf"
"$sdk_dir/bin/microkit" \
    "$root_dir/platforms/iphone7plus/nexatrode-slint-notes-explicit-nc.system" \
    --search-path "$out_dir" --board iphone7plus --config release --image-type elf \
    -o "$out_dir/loader.elf" -r "$out_dir/report.txt"
"$objcopy" -O binary "$out_dir/loader.elf" "$out_dir/loader.bin"
shasum -a 256 "$asset" "$out_dir/loader.bin"
