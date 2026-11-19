#!/bin/sh
# Isolated framebuffer-attribute experiment; leaves the working SDK untouched.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
microkit_dir="$root_dir/.cache/microkit-src-2.3.1"
base_sel4="$root_dir/.cache/sel4-src-microkit231"
test_sel4="$root_dir/.cache/sel4-src-microkit231-screen-normal-nc"
sdk_dir="$microkit_dir/release/microkit-sdk-2.3.1-iphone7plus-16k-screen-normal-nc"
base_sdk="$microkit_dir/release/microkit-sdk-2.3.1-iphone7plus-16k"
notes_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes"
out_dir="$root_dir/build/iphone7plus/nexatrode-slint-notes-normal-nc"
objcopy=$(command -v llvm-objcopy || true)
if [ -z "$objcopy" ]; then objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi

if [ ! -f "$base_sel4/src/arch/arm/64/kernel/vspace.c" ] ||
   [ ! -x "$base_sdk/bin/microkit" ] ||
   [ ! -f "$notes_dir/slint_notes_iphone.elf" ] ||
   [ ! -f "$notes_dir/touch-service.elf" ] ||
   [ ! -x "$objcopy" ]; then
    echo 'Build the base 16 KiB SDK and native Slint Notes image first.' >&2
    exit 1
fi

if [ ! -d "$test_sel4" ]; then
    cp -R "$base_sel4" "$test_sel4"
    patch -d "$test_sel4" -p1 < "$root_dir/patches/sel4-iphone7plus-screen-normal-nc-experiment.patch"
fi
if ! rg -q 'attridx = NORMAL_NC;' "$test_sel4/src/arch/arm/64/kernel/vspace.c"; then
    echo 'Experimental source copy lacks the Normal-NC patch.' >&2
    exit 1
fi

export PATH="$root_dir/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
cd "$microkit_dir"
"$root_dir/.cache/sel4-venv/bin/python" build_sdk.py \
    --sel4 "$test_sel4" --llvm --boards iphone7plus --configs release \
    --version 2.3.1-iphone7plus-16k-screen-normal-nc \
    --build-dir build-16k-screen-normal-nc --skip-tool --skip-docs --skip-tar
mkdir -p "$sdk_dir/bin" "$out_dir"
cp "$base_sdk/bin/microkit" "$sdk_dir/bin/microkit"
cp "$notes_dir/slint_notes_iphone.elf" "$out_dir/"
cp "$notes_dir/touch-service.elf" "$out_dir/"
"$sdk_dir/bin/microkit" "$root_dir/platforms/iphone7plus/nexatrode-slint-notes.system" \
    --search-path "$out_dir" --board iphone7plus --config release \
    --image-type elf -o "$out_dir/loader.elf" -r "$out_dir/report.txt"
"$objcopy" -O binary "$out_dir/loader.elf" "$out_dir/loader.bin"
shasum -a 256 "$out_dir/loader.bin"
