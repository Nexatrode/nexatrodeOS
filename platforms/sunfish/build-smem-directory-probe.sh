#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/smem-directory-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
reset_outer=${SMEM_PROBE_RESET_OUTER:-0}
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"

case "$reset_outer" in
    ''|*[!0-9]*) echo 'SMEM_PROBE_RESET_OUTER must be an integer from 0 to 5000.' >&2; exit 2 ;;
esac
if [ "$reset_outer" -gt 5000 ]; then
    echo 'SMEM_PROBE_RESET_OUTER must be an integer from 0 to 5000.' >&2
    exit 2
fi

test -f "$reference"
test -x "$sdk/bin/microkit"
mkdir -p "$out_dir"

clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
    -Werror -I"$sdk/board/sunfish/debug/include" \
    -c "$port_dir/smem-directory-probe.c" -o "$out_dir/smem-directory-probe.o"
ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/smem-directory-probe.o" \
    -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
    -o "$out_dir/smem-directory-probe.elf"

clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
    -Werror -DRESET_OUTER="${reset_outer}u" -I"$sdk/board/sunfish/debug/include" \
    -c "$port_dir/slint-reset.c" -o "$out_dir/slint-reset.o"
ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/slint-reset.o" \
    -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
    -o "$out_dir/slint-reset.elf"

"$sdk/bin/microkit" "$port_dir/smem-directory-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" \
    --dtb "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" \
    --report "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/smem-directory-probe.Image"
lz4 -z -f -q "$out_dir/smem-directory-probe.Image" \
    "$out_dir/smem-directory-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/smem-directory-probe.Image.lz4" \
    "$out_dir/fastboot.img"

printf 'Built temporary SMEM directory probe (not booted): %s\n' \
    "$out_dir/fastboot.img"
