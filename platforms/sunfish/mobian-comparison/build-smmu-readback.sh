#!/bin/sh
# Build only the read-only helper against the existing pinned Linux outputs.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cache_dir="$root_dir/.cache/sunfish-mobian-comparison"
out_dir="$root_dir/.cache/sunfish-native-wifi/smmu-readback"
mkdir -p "$out_dir"
podman machine ssh sudo modprobe loop ext4
podman run --rm --privileged --name sunfish-linux-smmu-readback-build \
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
mkdir -p /build/reference-smmu-readback
cp /workspace/platforms/sunfish/mobian-comparison/smmu-readback/Makefile /build/reference-smmu-readback/
cp /workspace/platforms/sunfish/mobian-comparison/smmu-readback/sunfish_smmu_readback.c /build/reference-smmu-readback/
make -C /build/source O=/build/linux-out M=/build/reference-smmu-readback \
    ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ modules
cp /build/reference-smmu-readback/sunfish_smmu_readback.ko /export/
sha256sum /build/reference-smmu-readback/sunfish_smmu_readback.c > /export/source.sha256
aarch64-linux-gnu-readelf -p .modinfo /export/sunfish_smmu_readback.ko > /export/modinfo.txt
sha256sum /export/sunfish_smmu_readback.ko > /export/module.sha256
'
