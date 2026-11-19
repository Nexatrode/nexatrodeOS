#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk_dir="$root_dir/.cache/microkit-src-2.3.1/release/microkit-sdk-2.3.1-iphone7plus-16k"
board_dir="$sdk_dir/board/iphone7plus/release"
mode=${1:-static}
case "$mode" in
    static) output_dir="$root_dir/build/iphone7plus/nexatrode-shell"; system=nexatrode-shell.system; shell_source=nexatrode-shell.c ;;
    touch) output_dir="$root_dir/build/iphone7plus/nexatrode-touch-shell"; system=nexatrode-touch-shell.system; shell_source=nexatrode-touch-shell.c ;;
    *) echo 'Usage: build-nexatrode-shell.sh [static|touch]' >&2; exit 2 ;;
esac
touch_dir="$root_dir/platforms/iphone7plus/touch"
target_cc=${IPHONE7PLUS_CLANG:-clang}
target_ld=${IPHONE7PLUS_LLD:-ld.lld}
target_objcopy=${IPHONE7PLUS_OBJCOPY:-$(command -v llvm-objcopy || true)}
if [ -z "$target_objcopy" ]; then target_objcopy=/opt/homebrew/opt/llvm/bin/llvm-objcopy; fi
if [ ! -x "$sdk_dir/bin/microkit" ] || [ ! -x "$target_objcopy" ]; then
    echo 'The 16 KiB iPhone Microkit SDK and llvm-objcopy are required.' >&2
    exit 1
fi

mkdir -p "$output_dir"
(
    cd "$root_dir/rust"
    set -- --features iphone-shell --bin rust_settings_app --bin rust_phone_app
    if [ "$mode" = touch ]; then set -- "$@" --bin rust_fake_modem; fi
    SEL4_INCLUDE_DIRS="$board_dir/include" cargo build --locked --release \
        --target aarch64-sel4-microkit-iphone16k \
        --target-dir "$root_dir/build/iphone7plus/rust-target" \
        "$@"
)
for app in settings phone; do
    cp "$root_dir/build/iphone7plus/rust-target/aarch64-sel4-microkit-iphone16k/release/rust_${app}_app.elf" \
        "$output_dir/rust_${app}_app.elf"
done
if [ "$mode" = touch ]; then
    cp "$root_dir/build/iphone7plus/rust-target/aarch64-sel4-microkit-iphone16k/release/rust_fake_modem.elf" \
        "$output_dir/rust_fake_modem.elf"
fi
"$target_cc" -target aarch64-none-elf -c -std=gnu11 -g -O3 -nostdlib \
    -ffreestanding -Wall -Wextra -Werror -mstrict-align -mcpu=generic \
    -I"$board_dir/include" -I"$touch_dir" \
    "$root_dir/platforms/iphone7plus/$shell_source" \
    -o "$output_dir/shell.o"
if [ "$mode" = touch ]; then
    sh "$root_dir/platforms/iphone7plus/build-touch-service.sh" "$board_dir" "$output_dir"
fi
"$target_ld" --defsym=MICROKIT_PAGE_SIZE=0x4000 -z max-page-size=0x4000 \
    -L"$board_dir/lib" "$output_dir/shell.o" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1/libmicrokit/microkit.ld" \
    -o "$output_dir/shell.elf"
"$sdk_dir/bin/microkit" "$root_dir/platforms/iphone7plus/$system" \
    --search-path "$output_dir" --board iphone7plus --config release \
    --image-type elf -o "$output_dir/loader.elf" -r "$output_dir/report.txt"
"$target_objcopy" -O binary "$output_dir/loader.elf" "$output_dir/loader.bin"
echo "Built NexatrodeOS app shell: $output_dir/loader.bin"
wc -c < "$output_dir/loader.bin"
