#!/bin/sh
# Compile the owner PD and Microkit capability layout. Never boot or flash.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
out="$root_dir/.cache/pixel-sunfish/modem-owner-offline-build"
python3 "$root_dir/platforms/sunfish/validate-modem-owner.py"
mkdir -p "$out"
cd "$root_dir"
export SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include"
export SUNFISH_MODEM_MDT_PATH="$root_dir/.cache/pixel-sunfish/pas-metadata-prepare/modem.mdt"
export SUNFISH_MODEM_SOURCES_RS_PATH="$root_dir/.cache/pixel-sunfish/modem-glink-channel-probe/firmware-sources.rs"
export CARGO_TARGET_DIR="$out/target"
cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --features sunfish-modem-owner \
    --bin rust_sunfish_modem_smp2p_probe --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp "$out/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_modem_smp2p_probe.elf" "$out/modem_image_probe.elf"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
python3 "$root_dir/platforms/sunfish/prepare-image-loader-sdk.py" --out-dir "$out/placement"
MICROKIT_SDK="$out/placement/sdk" "$sdk/bin/microkit" \
    "$root_dir/platforms/sunfish/modem-owner.system" --board sunfish --config debug \
    --search-path "$out" -o "$out/owner-system.elf" -r "$out/microkit-report.txt" \
    --capdl-json "$out/capdl.json"
python3 "$root_dir/platforms/sunfish/validate-modem-owner.py" \
    "$root_dir/platforms/sunfish/modem-owner.system" --capdl "$out/capdl.json"
python3 "$root_dir/platforms/sunfish/validate-modem-pools.py" \
    --capdl "$out/capdl.json" --report "$out/microkit-report.txt" --image "$out/owner-system.elf" \
    --loader-elf "$out/placement/loader-relocated-build/loader.elf" \
    --profile "$out/placement/loader-sdk-profile.json" \
    --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
    --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
printf '%s\n' "Owner PD ELF compiled offline: $out/modem_image_probe.elf"
