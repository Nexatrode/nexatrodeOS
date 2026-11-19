#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/scm-availability-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

test -x "$sdk/bin/microkit"
test -f "$reference"
mkdir -p "$out_dir"

clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 \
    -Wall -Wextra -Werror -I"$sdk/board/sunfish/debug/include" \
    -c "$port_dir/scm-availability-probe.c" -o "$out_dir/scm-availability-probe.o"
ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/scm-availability-probe.o" \
    -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
    -o "$out_dir/scm-availability-probe.elf"

"$sdk/bin/microkit" "$port_dir/scm-availability-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/scm-availability-probe.Image"
lz4 -z -f -q "$out_dir/scm-availability-probe.Image" \
    "$out_dir/scm-availability-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/scm-availability-probe.Image.lz4" \
    "$out_dir/fastboot.img"

printf 'Built read-only SCM availability probe: %s\n' "$out_dir/fastboot.img"
printf 'No image was installed or booted.\n'
