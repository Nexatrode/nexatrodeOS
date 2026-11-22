#!/bin/sh
# Rebuild the experimental 16 KiB SDK from the prepared bring-up sources.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
src_dir="$root_dir/.cache/microkit-src-2.3.1"
sel4_dir="$root_dir/.cache/sel4-src-microkit231"
rust_dir="$root_dir/.cache/rust-sel4-iphone7plus16k"
python_bin="$root_dir/.cache/sel4-venv/bin/python"
if [ ! -f "$sel4_dir/src/plat/iphone7plus/config.cmake" ] || [ ! -f "$src_dir/build_sdk.py" ]; then
    echo 'Prepare the existing iPhone bring-up sources first; this is an incremental port.' >&2
    exit 1
fi
if [ ! -d "$rust_dir" ]; then
    git clone https://github.com/seL4/rust-sel4 "$rust_dir"
    git -C "$rust_dir" checkout --detach dbe6445d56059ed9a757e53c7137892aece1d179
fi
python3 "$root_dir/platforms/iphone7plus/apply-16k-patches.py"
export PATH="$root_dir/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
cd "$src_dir"
cargo test -p microkit-tool --locked
cargo build -p microkit-tool --release --locked
"$python_bin" build_sdk.py --sel4 "$sel4_dir" --llvm --boards iphone7plus \
    --configs release --version 2.3.1-iphone7plus-16k --build-dir build-16k \
    --skip-tool --skip-docs --skip-tar
mkdir -p release/microkit-sdk-2.3.1-iphone7plus-16k/bin
cp target/release/microkit release/microkit-sdk-2.3.1-iphone7plus-16k/bin/microkit
"$root_dir/platforms/iphone7plus/build-microkit-smoke-16k.sh"
board_dir="$src_dir/release/microkit-sdk-2.3.1-iphone7plus-16k/board/iphone7plus/release"
python3 "$root_dir/platforms/iphone7plus/check-16k-boot.py" \
    "$root_dir/build/iphone7plus/smoke-16k/loader.bin" "$board_dir/elf/sel4.elf" \
    --initialiser "$board_dir/elf/initialiser.elf" \
    --user-elf "$root_dir/build/iphone7plus/smoke-16k/idle.elf" \
    --report "$root_dir/build/iphone7plus/smoke-16k/emulation.json"
