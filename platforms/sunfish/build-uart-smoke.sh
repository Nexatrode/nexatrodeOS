#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out_dir="$root_dir/.cache/pixel-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
clang_bin=/opt/homebrew/opt/llvm/bin/clang
objcopy_bin=/opt/homebrew/opt/llvm/bin/llvm-objcopy

if [ ! -f "$reference" ]; then
    echo "missing verified current-slot backup: $reference" >&2
    exit 1
fi
mkdir -p "$out_dir"
"$clang_bin" --target=aarch64-none-elf -fuse-ld=/opt/homebrew/bin/ld.lld \
    -nostdlib -Wl,--image-base=0 -Wl,-Ttext=0 -Wl,-e,_start -Wl,--build-id=none \
    "$root_dir/platforms/sunfish/uart-smoke.S" -o "$out_dir/uart-smoke.elf"
"$objcopy_bin" -O binary "$out_dir/uart-smoke.elf" "$out_dir/uart-smoke.Image"
lz4 -z -f -q "$out_dir/uart-smoke.Image" "$out_dir/uart-smoke.Image.lz4"
python3 "$root_dir/platforms/sunfish/repack-boot-v2.py" \
    "$reference" "$out_dir/uart-smoke.Image.lz4" "$out_dir/uart-smoke-fastboot.img"
