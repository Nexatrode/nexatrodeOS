#!/bin/sh
# Build only the active-only IPA snapshot helper against pinned Linux outputs.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cache_dir="$root_dir/.cache/sunfish-mobian-comparison"
out_dir="$root_dir/.cache/sunfish-native-wifi/ipa-config-readback"
mkdir -p "$out_dir"
podman machine ssh sudo modprobe loop ext4
podman run --rm --privileged --name sunfish-linux-ipa-config-readback-build \
    -v "$root_dir:/workspace:ro" -v "$cache_dir:/inputs" -v "$out_dir:/export" \
    localhost/sunfish-mobian-toolchain:20261025 sh -eu -c '
test "$(blkid -p -s LABEL -o value /inputs/build.ext4)" = SunfishBuild
for index in 0 1 2 3 4 5 6 7; do
    test -b "/dev/loop$index" || mknod "/dev/loop$index" b 7 "$index"
done
mkdir -p /build
mount -o loop /inputs/build.ext4 /build
trap "cd /; umount /build" EXIT
test "$(cat /build/source/.reference-commit)" = 2267e248204e7e051e0f7355298949a6440de579
cmp /build/linux-out/.config /inputs/artifacts/kernel.config
test "$(cat /build/linux-out/include/config/kernel.release)" = 7.1.0-sm7150+
mkdir -p /build/reference-ipa-config-readback
cp /workspace/platforms/sunfish/mobian-comparison/ipa-config-readback/Makefile /build/reference-ipa-config-readback/
cp /workspace/platforms/sunfish/mobian-comparison/ipa-config-readback/sunfish_ipa_config_readback.c /build/reference-ipa-config-readback/
make -C /build/source O=/build/linux-out M=/build/reference-ipa-config-readback \
    ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ modules
cp /build/reference-ipa-config-readback/sunfish_ipa_config_readback.ko /export/
sha256sum /build/reference-ipa-config-readback/sunfish_ipa_config_readback.c > /export/source.sha256
aarch64-linux-gnu-readelf -p .modinfo /export/sunfish_ipa_config_readback.ko > /export/modinfo.txt
sha256sum /export/sunfish_ipa_config_readback.ko > /export/module.sha256
'
