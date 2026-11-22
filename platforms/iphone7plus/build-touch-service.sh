#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
board_dir=$1
output_dir=$2
touch_dir="$root_dir/platforms/iphone7plus/touch"
target_cc=${IPHONE7PLUS_CLANG:-clang}
target_ld=${IPHONE7PLUS_LLD:-ld.lld}
private_stream="$root_dir/.cache/iphone7plus-touch-fw/boot-stream.bin"
expected_stream_sha=c4157ef64c0901f7e8aa14768e8173ab76ac406fc54e668f1f64b2eb3d74a0b3
if [ ! -f "$private_stream" ] ||
   [ "$(shasum -a 256 "$private_stream" | cut -d ' ' -f 1)" != "$expected_stream_sha" ]; then
    echo 'The device-specific D11 boot stream is missing or has changed.' >&2
    exit 1
fi
for source in touch-service hw_control i2c_pmu spi2_transport \
              boot_stream boot_execute z2_report packet report freestanding; do
    "$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
        -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
        -I"$board_dir/include" -I"$touch_dir" "$touch_dir/$source.c" \
        -o "$output_dir/$source.o"
done
printf '.section .rodata\n.balign 16\n.global touch_firmware_start\ntouch_firmware_start:\n.incbin "%s"\n.global touch_firmware_end\ntouch_firmware_end:\n' "$private_stream" > "$output_dir/firmware-data.S"
"$target_cc" -target aarch64-none-elf -c "$output_dir/firmware-data.S" \
    -o "$output_dir/firmware-data.o"
"$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
    -L"$board_dir/lib" "$output_dir/touch-service.o" \
    "$output_dir/hw_control.o" "$output_dir/i2c_pmu.o" \
    "$output_dir/spi2_transport.o" "$output_dir/boot_stream.o" \
    "$output_dir/boot_execute.o" "$output_dir/z2_report.o" \
    "$output_dir/packet.o" "$output_dir/report.o" \
    "$output_dir/freestanding.o" "$output_dir/firmware-data.o" \
    -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/touch-service.elf"
