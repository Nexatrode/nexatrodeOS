#!/bin/sh
set -eu
umask 077
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
case "${1:-wlan}" in
    wlan) feature="sunfish-smmu-install-probe"; profile="smmu-install-probe"; data_pages=1; out_dir="$root_dir/.cache/sunfish-native-wifi/smmu-install-probe" ;;
    ipa) feature="sunfish-smmu-ipa-install-probe"; profile="smmu-ipa-exact-install-probe"; data_pages=32; out_dir="$root_dir/.cache/sunfish-native-wifi/private/$profile" ;;
    ipa-roots) feature="sunfish-smmu-ipa-root-install-probe"; profile="smmu-ipa-root-install-probe"; data_pages=37; out_dir="$root_dir/.cache/sunfish-native-wifi/private/$profile" ;;
    *) printf 'Expected wlan, ipa or ipa-roots\n' >&2; exit 2 ;;
esac
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="${SUNFISH_REFERENCE_BOOT:-$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img}"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
mkdir -p "$out_dir"
cd "$root_dir"
if [ "$data_pages" = 37 ]; then
    python3 "$port_dir/audit-ipa-root-source.py" --output "$out_dir/linux-source-audit.json"
    validator="validate-smmu-root-install-probe.py"
    rustc --edition 2024 -D warnings --test tests/sunfish_smmu_root_tables_test.rs -o "$out_dir/root-test"
    "$out_dir/root-test" --quiet > "$out_dir/root-tests.log"
else
    python3 "$port_dir/audit-native-ipa-linux-source.py" --output "$out_dir/linux-source-audit.json"
    validator="validate-smmu-install-probe.py"
fi
rustc --edition 2024 -D warnings --test tests/sunfish_smmu_native_test.rs -o "$out_dir/native-test"
"$out_dir/native-test" --quiet > "$out_dir/native-tests.log"
export SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include"
export CARGO_TARGET_DIR="$root_dir/.cache/sunfish-native-wifi/target"
if ! cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --features "$feature" \
    --bin rust_sunfish_smmu_install_probe --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec > "$out_dir/cargo-build.log" 2>&1; then
    tail -n 35 "$out_dir/cargo-build.log"
    exit 1
fi
cp "$CARGO_TARGET_DIR/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_smmu_install_probe.elf" "$out_dir/smmu_install_probe.elf"
"$sdk/bin/microkit" "$port_dir/$profile.system" --search-path "$out_dir" \
    --board sunfish --config debug -o "$out_dir/loader.img" -r "$out_dir/report.txt" \
    --capdl-json "$out_dir/capdl.json"
python3 "$port_dir/$validator" --system "$port_dir/$profile.system" --data-pages "$data_pages" --capdl "$out_dir/capdl.json" \
    --report "$out_dir/report.txt" --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
    --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" \
    --output "$out_dir/capability-audit.json"
python3 "$port_dir/test-smmu-install-probe-audit.py" --system "$port_dir/$profile.system" --data-pages "$data_pages" --capdl "$out_dir/capdl.json" --report "$out_dir/report.txt" > "$out_dir/capability-mutations.log"
python3 "$port_dir/wrap-microkit-image.py" "$out_dir/loader.img" "$out_dir/smmu-install-probe.Image"
lz4 -z -f -q "$out_dir/smmu-install-probe.Image" "$out_dir/smmu-install-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" "$reference" "$out_dir/smmu-install-probe.Image.lz4" "$out_dir/fastboot.img"
shasum -a 256 "$out_dir/fastboot.img" > "$out_dir/fastboot.img.sha256"
printf 'Built bounded SMMU context/route experiment: %s\n' "$out_dir/fastboot.img"
