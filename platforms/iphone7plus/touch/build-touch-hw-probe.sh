#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1/release/microkit-sdk-2.3.1-iphone7plus-16k"
board_dir="$sdk_dir/board/iphone7plus/release"
output_dir="$root_dir/build/iphone7plus/touch-hw-probe"
target_cc=${IPHONE7PLUS_CLANG:-clang}
target_ld=${IPHONE7PLUS_LLD:-ld.lld}
target_objcopy=${IPHONE7PLUS_OBJCOPY:-$(command -v llvm-objcopy || true)}
if [ -z "$target_objcopy" ]; then target_objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi
if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -x "$target_objcopy" ]; then
    echo 'The 16 KiB iPhone Microkit SDK and llvm-objcopy are required.' >&2
    exit 1
fi

mkdir -p "$output_dir"
"$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
    -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
    -DTOUCH_HW_PROBE -I"$board_dir/include" \
    "$root_dir/platforms/iphone7plus/touch/spi2-read.c" \
    -o "$output_dir/touch-hw-probe.o"
"$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
    -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
    "$root_dir/platforms/iphone7plus/touch/hw_control.c" \
    -o "$output_dir/hw_control.o"
"$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
    -L"$board_dir/lib" "$output_dir/touch-hw-probe.o" \
    "$output_dir/hw_control.o" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/touch-hw-probe.elf"
"$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
    -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
    -I"$board_dir/include" "$root_dir/platforms/iphone7plus/touch/spi2-fault.c" \
    -o "$output_dir/spi2-fault.o"
"$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
    -L"$board_dir/lib" "$output_dir/spi2-fault.o" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/spi2-fault.elf"
"$sdk_dir/bin/microkit" "$root_dir/platforms/iphone7plus/touch/touch-hw-probe.system" \
    --search-path "$output_dir" --board iphone7plus --config release \
    --image-type elf -o "$output_dir/loader.elf" -r "$output_dir/report.txt"
"$target_objcopy" -O binary "$output_dir/loader.elf" "$output_dir/loader.bin"
echo "Built touch hardware probe: $output_dir/loader.bin"
wc -c < "$output_dir/loader.bin"
