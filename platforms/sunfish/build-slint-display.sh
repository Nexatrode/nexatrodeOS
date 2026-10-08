#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/slint-display"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
pixel_features="pixel-pd,full-suite-ui${SLINT_PIXEL_EXTRA_FEATURES:+,$SLINT_PIXEL_EXTRA_FEATURES}"
reset_outer=${SLINT_RESET_OUTER:-80}
case "$reset_outer" in
    ''|*[!0-9]*) echo 'SLINT_RESET_OUTER must be an integer from 0 to 5000 (0 disables automatic reset).' >&2; exit 2 ;;
esac
if [ "$reset_outer" -lt 0 ] || [ "$reset_outer" -gt 5000 ]; then
    echo 'SLINT_RESET_OUTER must be an integer from 0 to 5000 (0 disables automatic reset).' >&2
    exit 2
fi

test -f "$reference"
test -x "$sdk/bin/microkit"
mkdir -p "$out_dir"
cd "$root_dir"
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --features "$pixel_features" --bin slint_notes_pixel \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp experiments/slint-notes/target/aarch64-sel4-microkit-pixel4k/release/slint_notes_pixel.elf \
    "$out_dir/slint_notes_pixel.elf"
clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
    -Werror -I"$sdk/board/sunfish/debug/include" \
    -c "$port_dir/touch-service.c" -o "$out_dir/touch-service.o"
ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/touch-service.o" \
    -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
    -o "$out_dir/touch-service.elf"
for pd in notes-service notes-volatile-vault; do
    clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
        -Werror -I"$sdk/board/sunfish/debug/include" -I"$port_dir" \
        -c "$port_dir/$pd.c" -o "$out_dir/$pd.o"
    ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/$pd.o" \
        -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
        -o "$out_dir/$pd.elf"
done
clang --target=aarch64-none-elf -nostdlib -ffreestanding -O2 -Wall -Wextra \
    -Werror -DRESET_OUTER="$reset_outer" -I"$sdk/board/sunfish/debug/include" \
    -c "$port_dir/slint-reset.c" -o "$out_dir/slint-reset.o"
ld.lld -L"$sdk/board/sunfish/debug/lib" "$out_dir/slint-reset.o" \
    -lmicrokit -T"$root_dir/.cache/microkit-src-2.3.1-sunfish/libmicrokit/microkit.ld" \
    -o "$out_dir/slint-reset.elf"
"$sdk/bin/microkit" "$port_dir/slint-display.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/slint-display.Image"
lz4 -z -f -q "$out_dir/slint-display.Image" "$out_dir/slint-display.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/slint-display.Image.lz4" "$out_dir/fastboot.img"
