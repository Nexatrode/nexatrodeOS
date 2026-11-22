#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/touch-i2c-ping"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
mkdir -p "$out_dir"
for name in touch-i2c-ping slint-reset; do
    clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
        -Werror -I"$sdk/board/sunfish/debug/include" \
        -c "$port_dir/$name.c" -o "$out_dir/$name.o"
    ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/$name.o" \
        -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
        -o "$out_dir/$name.elf"
done
"$sdk/bin/microkit" "$port_dir/touch-i2c-ping.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/touch-i2c-ping.Image"
lz4 -z -f -q "$out_dir/touch-i2c-ping.Image" "$out_dir/touch-i2c-ping.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/touch-i2c-ping.Image.lz4" "$out_dir/fastboot.img"
