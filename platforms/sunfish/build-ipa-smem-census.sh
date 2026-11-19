#!/bin/sh
set -eu
umask 077
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out="$root_dir/.cache/sunfish-native-wifi/private/ipa-smem-census"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="${SUNFISH_REFERENCE_BOOT:-$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img}"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
mkdir -p "$out"
python3 "$port_dir/audit-native-ipa-linux-source.py" --output "$out/linux-source-audit.json"
python3 "$port_dir/test-ipa-smem-census.py" > "$out/host-tests.log" 2>&1
clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra -Werror \
    -I"$sdk/board/sunfish/debug/include" -c "$port_dir/ipa-smem-census.c" -o "$out/ipa-smem-census.o"
ld.lld -L"$sdk/board/sunfish/debug/lib" "$out/ipa-smem-census.o" -lmicrokit \
    -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" -o "$out/ipa-smem-census.elf"
# The legacy directory profile must still compile independently.
clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra -Werror \
    -I"$sdk/board/sunfish/debug/include" -c "$port_dir/smem-directory-probe.c" -o "$out/legacy-directory.o"
"$sdk/bin/microkit" "$port_dir/ipa-smem-census.system" --search-path "$out" --board sunfish --config debug \
    -o "$out/loader.img" -r "$out/report.txt" --capdl-json "$out/capdl.json"
python3 "$port_dir/validate-ipa-smem-census.py" --capdl "$out/capdl.json" --report "$out/report.txt" \
    --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
    --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
python3 "$port_dir/wrap-microkit-image.py" "$out/loader.img" "$out/ipa-smem-census.Image"
lz4 -z -f -q "$out/ipa-smem-census.Image" "$out/ipa-smem-census.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" "$reference" "$out/ipa-smem-census.Image.lz4" "$out/fastboot.img"
shasum -a 256 "$out/fastboot.img" > "$out/fastboot.img.sha256"
printf 'Built temporary item-497 metadata census: %s\n' "$out/fastboot.img"
