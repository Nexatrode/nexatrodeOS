#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
src_dir="$root_dir/.cache/microkit-src-2.3.1"
board_sdk="$src_dir/release/microkit-sdk-2.3.1-iphone7plus/board/iphone7plus/release/elf"

apply_once() {
    patch_file=$1
    if git -C "$src_dir" apply --check --reverse "$patch_file" >/dev/null 2>&1; then
        return
    fi
    # The exception-overlay patch changes both exceptions.c and util64.S.
    # Later patches replace the exception marker sequence and remove the
    # unsafe pre-stack vector marker, so the original two-file diff no longer
    # reverse-applies even though its intended state is present.
    if [ "$(basename "$patch_file")" = "microkit-2.3.1-iphone7plus-mmu-exception-overlay.patch" ] &&
       grep -q 'el1_mmu_disable();' "$src_dir/loader/src/aarch64/exceptions.c" &&
       ! grep -q 'ldr[[:space:]]*x15, =0x8BB80C000' "$src_dir/loader/src/aarch64/util64.S"; then
        return
    fi
    if [ "$(basename "$patch_file")" = "microkit-2.3.1-iphone7plus-mmu-safe-vector.patch" ] &&
       grep -q 'pongo_mmu_fault_vector' "$src_dir/loader/src/aarch64/util64.S"; then
        return
    fi
    if [ "$(basename "$patch_file")" = "microkit-2.3.1-iphone7plus-mmu-post-enable-marker.patch" ] &&
       grep -q 'Direct black row 280 after SCTLR.M' "$src_dir/loader/src/aarch64/util64.S"; then
        return
    fi
    if ! git -C "$src_dir" apply --check "$patch_file"; then
        echo "Cannot apply $patch_file to the cached Microkit source" >&2
        exit 1
    fi
    git -C "$src_dir" apply "$patch_file"
}

apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-stage-checkpoint-omission.patch"
apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-return-overlay.patch"
apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-exception-overlay.patch"
apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-post-enable-marker.patch"
apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-safe-vector.patch"
apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-off-result.patch"
apply_once "$root_dir/patches/microkit-2.3.1-exception-register-address.patch"
apply_once "$root_dir/patches/microkit-2.3.1-iphone7plus-mmu-direct-vector.patch"

# Row 280 turns black after SCTLR_EL1.M and again after the MMU-off return.
# A stackless exception vector paints a 128x128 black square at the top. Stages 4-6 are hidden to keep
# the trace compact; their calls only draw bars.
make -C "$src_dir/loader" \
    BUILD_DIR=../build/iphone7plus/release/loader \
    ARCH=aarch64 BOARD=iphone7plus LINK_ADDRESS=0x810000000 \
    TARGET_TRIPLE=aarch64-none-elf LLVM=True GCC_CPU=generic \
    SEL4_SDK=../release/microkit-sdk-2.3.1/board/iphone7plus/release \
    ASM_FLAGS="-mcpu=generic -g -MP -MD -I../release/microkit-sdk-2.3.1/board/iphone7plus/release/include -DCONFIG_PLAT_IPHONE7PLUS_SKIP_MMU_STAGE_HOOKS" \
    all

mkdir -p "$board_sdk"
cp "$src_dir/build/iphone7plus/release/loader/loader.elf" \
   "$board_sdk/loader.elf"
"$root_dir/platforms/iphone7plus/build-microkit-smoke.sh"
python3 "$root_dir/platforms/iphone7plus/verify-mmu-diagnostic.py" \
    "$src_dir/build/iphone7plus/release/loader/loader.elf" \
    "$root_dir/build/iphone7plus/smoke/loader.bin"
