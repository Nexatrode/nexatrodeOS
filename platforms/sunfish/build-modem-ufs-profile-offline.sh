#!/bin/sh
# Compile and validate the opt-in UFS authority profile offline. Never boot or flash.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
out="$root_dir/.cache/pixel-sunfish/modem-owner-offline-build"

sh "$root_dir/platforms/sunfish/build-modem-owner-offline.sh"
MICROKIT_SDK="$out/placement/sdk" "$sdk/bin/microkit" \
    "$root_dir/platforms/sunfish/modem-owner-ufs-profile.system" \
    --board sunfish --config debug --search-path "$out" \
    -o "$out/owner-ufs-profile-system.elf" \
    -r "$out/ufs-profile-report.txt" \
    --capdl-json "$out/ufs-profile-capdl.json"
python3 "$root_dir/platforms/sunfish/validate-modem-owner.py" \
    --ufs-profile "$root_dir/platforms/sunfish/modem-owner-ufs-profile.system" \
    --capdl "$out/ufs-profile-capdl.json"
python3 "$root_dir/platforms/sunfish/validate-ufs-dma-pool.py" \
    --capdl "$out/ufs-profile-capdl.json" \
    --smmu-tables \
    --dtb "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" \
    --dtb "$root_dir/.cache/pixel-sunfish/live-runtime.dtb"
printf '%s\n' "Offline candidate UFS authority profile validated: $out/owner-ufs-profile-system.elf"
