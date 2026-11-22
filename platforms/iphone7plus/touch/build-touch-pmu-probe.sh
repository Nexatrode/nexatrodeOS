#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
probe_mode=${1:-read}
sdk_dir="$root_dir/.cache/microkit-src-2.3.1/release/microkit-sdk-2.3.1-iphone7plus-16k"
board_dir="$sdk_dir/board/iphone7plus/release"
case "$probe_mode" in
    read) output_dir="$root_dir/build/iphone7plus/touch-pmu-probe"; probe_flags=-DTOUCH_PMU_PROBE; probe_object=touch-pmu-probe; system=touch-pmu-probe.system; sources='spi2-read hw_control i2c_pmu' ;;
    hv) output_dir="$root_dir/build/iphone7plus/touch-hv-probe"; probe_flags='-DTOUCH_PMU_PROBE -DTOUCH_HV_PROBE'; probe_object=touch-pmu-probe; system=touch-pmu-probe.system; sources='spi2-read hw_control i2c_pmu' ;;
    startup) output_dir="$root_dir/build/iphone7plus/touch-startup-probe"; probe_flags=-DTOUCH_STARTUP_PROBE; probe_object=touch-startup-probe; system=touch-startup-probe.system; sources='spi2-read hw_control i2c_pmu spi2_transport' ;;
    firmware) output_dir="$root_dir/build/iphone7plus/touch-firmware-probe"; probe_flags='-DTOUCH_STARTUP_PROBE -DTOUCH_FIRMWARE_PROBE'; probe_object=touch-startup-probe; system=touch-startup-probe.system; sources='spi2-read hw_control i2c_pmu spi2_transport boot_stream boot_execute' ;;
    report) output_dir="$root_dir/build/iphone7plus/touch-report-probe"; probe_flags='-DTOUCH_STARTUP_PROBE -DTOUCH_FIRMWARE_PROBE -DTOUCH_REPORT_PROBE'; probe_object=touch-startup-probe; system=touch-startup-probe.system; sources='spi2-read hw_control i2c_pmu spi2_transport boot_stream boot_execute z2_report packet' ;;
    event) output_dir="$root_dir/build/iphone7plus/touch-event-probe"; probe_flags='-DTOUCH_STARTUP_PROBE -DTOUCH_FIRMWARE_PROBE -DTOUCH_REPORT_PROBE -DTOUCH_EVENT_PROBE'; probe_object=touch-startup-probe; system=touch-startup-probe.system; sources='spi2-read hw_control i2c_pmu spi2_transport boot_stream boot_execute z2_report packet report freestanding' ;;
    *) echo "Usage: $0 [read|hv|startup|firmware|report|event]" >&2; exit 2 ;;
esac
target_cc=${IPHONE7PLUS_CLANG:-clang}
target_ld=${IPHONE7PLUS_LLD:-ld.lld}
target_objcopy=${IPHONE7PLUS_OBJCOPY:-$(command -v llvm-objcopy || true)}
if [ -z "$target_objcopy" ]; then target_objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi
if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -x "$target_objcopy" ]; then
    echo 'The 16 KiB iPhone Microkit SDK and llvm-objcopy are required.' >&2
    exit 1
fi

mkdir -p "$output_dir"
for source in $sources; do
    case "$source" in
        spi2-read) object=$probe_object; extra=$probe_flags ;;
        *) object=$source; extra= ;;
    esac
    "$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
        -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
        $extra -I"$board_dir/include" \
        "$root_dir/platforms/iphone7plus/touch/$source.c" \
        -o "$output_dir/$object.o"
done
if [ "$probe_mode" = firmware ] || [ "$probe_mode" = report ] || [ "$probe_mode" = event ]; then
    private_stream="$root_dir/.cache/iphone7plus-touch-fw/boot-stream.bin"
    expected_stream_sha=c4157ef64c0901f7e8aa14768e8173ab76ac406fc54e668f1f64b2eb3d74a0b3
    if [ ! -f "$private_stream" ] ||
       [ "$(shasum -a 256 "$private_stream" | cut -d ' ' -f 1)" != "$expected_stream_sha" ]; then
        echo 'The device-specific D11 boot stream is missing or has changed.' >&2
        exit 1
    fi
    printf '.section .rodata\n.balign 16\n.global touch_firmware_start\ntouch_firmware_start:\n.incbin "%s"\n.global touch_firmware_end\ntouch_firmware_end:\n' "$private_stream" > "$output_dir/firmware-data.S"
    "$target_cc" -target aarch64-none-elf -c "$output_dir/firmware-data.S" \
        -o "$output_dir/firmware-data.o"
    set -- "$output_dir/spi2_transport.o" "$output_dir/boot_stream.o" \
        "$output_dir/boot_execute.o" "$output_dir/firmware-data.o"
    if [ "$probe_mode" = report ] || [ "$probe_mode" = event ]; then
        set -- "$@" "$output_dir/z2_report.o" "$output_dir/packet.o"
    fi
    if [ "$probe_mode" = event ]; then
        set -- "$@" "$output_dir/report.o" "$output_dir/freestanding.o"
    fi
elif [ "$probe_mode" = startup ]; then
    set -- "$output_dir/spi2_transport.o"
else
    set --
fi
"$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
    -L"$board_dir/lib" "$output_dir/$probe_object.o" \
    "$output_dir/hw_control.o" "$output_dir/i2c_pmu.o" "$@" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/$probe_object.elf"
"$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
    -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
    -I"$board_dir/include" "$root_dir/platforms/iphone7plus/touch/spi2-fault.c" \
    -o "$output_dir/spi2-fault.o"
"$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
    -L"$board_dir/lib" "$output_dir/spi2-fault.o" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/spi2-fault.elf"
"$sdk_dir/bin/microkit" "$root_dir/platforms/iphone7plus/touch/$system" \
    --search-path "$output_dir" --board iphone7plus --config release \
    --image-type elf -o "$output_dir/loader.elf" -r "$output_dir/report.txt"
"$target_objcopy" -O binary "$output_dir/loader.elf" "$output_dir/loader.bin"
echo "Built touch $probe_mode probe: $output_dir/loader.bin"
wc -c < "$output_dir/loader.bin"
