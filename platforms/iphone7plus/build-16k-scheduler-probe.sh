#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1/release/microkit-sdk-2.3.1-iphone7plus-16k"
board_dir="$sdk_dir/board/iphone7plus/release"
if [ "${IPHONE_SCHEDULER_CONTROL:-0}" = 1 ]; then
    output_dir="$root_dir/build/iphone7plus/scheduler-control-16k"
    control_define=-DIPHONE_TIMER_CONTROL
elif [ "${IPHONE_SCHEDULER_SYSCALL:-0}" = 1 ]; then
    output_dir="$root_dir/build/iphone7plus/scheduler-syscall-16k"
    control_define=-DIPHONE_TIMER_SYSCALL
else
    output_dir="$root_dir/build/iphone7plus/scheduler-16k"
    control_define=
fi
if [ "${IPHONE_OBSERVER_LEGACY:-0}" = 1 ]; then
    legacy_define=-DIPHONE_OBSERVER_LEGACY
else
    legacy_define=
fi
target_cc=${IPHONE7PLUS_CLANG:-clang}
target_ld=${IPHONE7PLUS_LLD:-ld.lld}
target_objcopy=${IPHONE7PLUS_OBJCOPY:-$(command -v llvm-objcopy || true)}
if [ -z "$target_objcopy" ]; then target_objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi
if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -x "$target_objcopy" ]; then
    echo 'Build the 16 KiB SDK first; llvm-objcopy is also required.' >&2
    exit 1
fi
mkdir -p "$output_dir"
for role in spinner observer; do
    case "$role" in
        spinner) role_define=IPHONE_TIMER_SPINNER ;;
        observer) role_define=IPHONE_TIMER_OBSERVER ;;
    esac
    "$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
        -ffreestanding -Wall -Wno-unused-function -Werror -mstrict-align \
        -mcpu=generic -D"$role_define" ${control_define:+"$control_define"} \
        ${legacy_define:+"$legacy_define"} -I"$board_dir/include" \
        "$root_dir/platforms/iphone7plus/smoke/idle-16k.c" \
        -o "$output_dir/$role.o"
    "$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
        -L"$board_dir/lib" "$output_dir/$role.o" -lmicrokit \
        -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
        -o "$output_dir/$role.elf"
done
"$sdk_dir/bin/microkit" \
    "$root_dir/platforms/iphone7plus/smoke/scheduler-16k.system" \
    --search-path "$output_dir" --board iphone7plus --config release \
    --image-type elf -o "$output_dir/loader.elf" -r "$output_dir/report.txt"
"$target_objcopy" -O binary "$output_dir/loader.elf" "$output_dir/loader.bin"
if [ "${IPHONE_SKIP_EMULATION:-0}" = 1 ]; then
    echo 'Skipping QEMU: its GIC timer route does not emulate Apple FIQ delivery.'
else
    python3 "$root_dir/platforms/iphone7plus/check-16k-boot.py" \
        "$output_dir/loader.bin" "$board_dir/elf/sel4.elf" \
        --initialiser "$board_dir/elf/initialiser.elf" \
        --user-elf "$output_dir/spinner.elf" --report "$output_dir/emulation.json"
fi
if [ "${IPHONE_SKIP_EMULATION:-0}" != 1 ] && \
   { [ "${IPHONE_SCHEDULER_CONTROL:-0}" = 1 ] || [ "${IPHONE_SCHEDULER_SYSCALL:-0}" = 1 ]; }; then
    python3 "$root_dir/platforms/iphone7plus/check-16k-boot.py" \
        "$output_dir/loader.bin" "$board_dir/elf/sel4.elf" \
        --initialiser "$board_dir/elf/initialiser.elf" \
        --user-elf "$output_dir/spinner.elf" \
        --observer-elf "$output_dir/observer.elf" \
        --report "$output_dir/observer-emulation.json"
fi
echo "Built 16 KiB scheduler probe: $output_dir/loader.bin"
