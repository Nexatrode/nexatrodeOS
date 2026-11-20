#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1/release/microkit-sdk-2.3.1-iphone7plus"
board_dir="$sdk_dir/board/iphone7plus/release"
output_dir="$root_dir/build/iphone7plus/smoke"
target_cc=${IPHONE7PLUS_CLANG:-clang}
target_ld=${IPHONE7PLUS_LLD:-ld.lld}
target_objcopy=${IPHONE7PLUS_OBJCOPY:-$(command -v llvm-objcopy || true)}

if [ -z "$target_objcopy" ]; then
    target_objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy
fi

if [ ! -x "$sdk_dir/bin/microkit" ]; then
    echo "Missing iPhone Microkit SDK: $sdk_dir" >&2
    exit 1
fi
if [ ! -x "$target_objcopy" ]; then
    echo "llvm-objcopy is required to produce the flat Pongo bootr payload" >&2
    exit 1
fi

mkdir -p "$output_dir"
"$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
    -ffreestanding -Wall -Wno-unused-function -Werror -mstrict-align \
    -mcpu=generic -I"$board_dir/include" \
    "$root_dir/platforms/iphone7plus/smoke/idle.c" \
    -o "$output_dir/idle.o"
"$target_ld" -L"$board_dir/lib" "$output_dir/idle.o" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/idle.elf"

"$sdk_dir/bin/microkit" \
    "$root_dir/platforms/iphone7plus/smoke/idle.system" \
    --search-path "$output_dir" --board iphone7plus --config release \
    --image-type elf -o "$output_dir/loader.elf" \
    -r "$output_dir/report.txt"

# Pongo bootr enters the first byte of its raw receive buffer. The Microkit
# loader's AArch64 crt0 is position-independent and relocates its flat image to
# LINK_ADDRESS before continuing, so strip the ELF container before upload.
"$target_objcopy" -O binary "$output_dir/loader.elf" \
    "$output_dir/loader.bin"
echo "Built flat Pongo payload: $output_dir/loader.bin"
wc -c < "$output_dir/loader.bin"
