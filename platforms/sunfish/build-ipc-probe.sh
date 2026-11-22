#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/ipc-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
timer_header="$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/gen_headers/plat/machine/devices_gen.h"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -f "$reference"
test -x "$sdk/bin/microkit"
test -f "$sdk/board/sunfish/debug/elf/sel4.elf"
test -f "$timer_header"
if grep -q '^#define KERNEL_TIMER_IRQ -1$' "$timer_header"; then
    echo 'sunfish kernel timer IRQ is unresolved; rebuild the SDK before packaging' >&2
    exit 1
fi
mkdir -p "$out_dir"

for part in service client intruder witness; do
    clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
        -Werror -I"$sdk/board/sunfish/debug/include" \
        -c "$port_dir/ipc-$part.c" -o "$out_dir/ipc-$part.o"
    ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/ipc-$part.o" \
        -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
        -o "$out_dir/ipc-$part.elf"
done

"$sdk/bin/microkit" "$port_dir/ipc-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/microkit.Image"
lz4 -z -f -q "$out_dir/microkit.Image" "$out_dir/microkit.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/microkit.Image.lz4" "$out_dir/fastboot.img"
