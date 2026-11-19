#!/bin/sh
# Build the exact community reference kernel/config, without a firmware payload.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cache_dir="$root_dir/.cache/sunfish-mobian-comparison"
mkdir -p "$cache_dir/artifacts"
python3 "$root_dir/platforms/sunfish/audit-mobian-comparison.py" --fetch --output "$cache_dir/audit.json"

# A private ext4 loop image preserves Linux symlinks, case distinctions and
# device nodes across a macOS/virtiofs share. It contains only this build.
python3 - "$cache_dir/build.ext4" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
if not path.exists():
    with path.open('xb') as stream:
        stream.truncate(40 * 1024**3)
PY

cat > "$cache_dir/build-in-container.sh" <<'BUILD'
#!/bin/sh
set -eu
export DEBIAN_FRONTEND=noninteractive
if ! blkid -p /inputs/build.ext4 >/dev/null 2>&1; then
    python3 - <<'PY'
with open('/inputs/build.ext4', 'rb') as stream:
    if any(stream.read(1024 * 1024)):
        raise SystemExit('Refusing to format a non-empty, unrecognized build image')
PY
    mkfs.ext4 -q -L SunfishBuild /inputs/build.ext4
fi
test "$(blkid -p -s LABEL -o value /inputs/build.ext4)" = SunfishBuild
for index in 0 1 2 3 4 5 6 7; do
    test -b "/dev/loop$index" || mknod "/dev/loop$index" b 7 "$index"
done
mkdir -p /build
mount -o loop /inputs/build.ext4 /build
trap 'cd /; umount /build' EXIT
dpkg-query -W > /build/toolchain-packages.tsv
cd /build
if [ ! -f source/.reference-commit ]; then
    test ! -e source || { echo 'Refusing an unmarked source directory' >&2; exit 2; }
    mkdir source
    tar -xzf /inputs/downloads/linux-2267e248.tar.gz -C source --strip-components=1
    printf '%s\n' 2267e248204e7e051e0f7355298949a6440de579 > source/.reference-commit
fi
test "$(cat source/.reference-commit)" = 2267e248204e7e051e0f7355298949a6440de579
mkdir -p linux-out artifacts
cp /workspace/.cache/mobian-pixel4a-port-789679/kernel-config-7.1.0-rc3-sm7150.txt reference.config
test -f linux-out/.config || cp reference.config linux-out/.config
export KBUILD_BUILD_USER=sunfish-reference KBUILD_BUILD_HOST=linux-build
export KBUILD_BUILD_TIMESTAMP='2026-10-25 00:00:00 +0000'
export KBUILD_BUILD_VERSION=1
make -C source O=/build/linux-out ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ olddefconfig
python3 /workspace/platforms/sunfish/audit-mobian-comparison.py \
    --config /build/linux-out/.config --config-only
python3 source/scripts/diffconfig reference.config linux-out/.config > config.delta
aarch64-linux-gnu-gcc --version > compiler.txt
make -C source O=/build/linux-out ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ \
    -j"${BUILD_JOBS:-4}" Image vmlinuz.efi qcom/sm7150-google-sunfish.dtb modules
make -C source O=/build/linux-out ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ \
    INSTALL_MOD_PATH=/build/modules-stage INSTALL_MOD_STRIP=1 modules_install
cp linux-out/arch/arm64/boot/Image artifacts/
cp linux-out/arch/arm64/boot/vmlinuz.efi artifacts/
cp linux-out/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dtb artifacts/
cp linux-out/.config artifacts/kernel.config
cp config.delta compiler.txt toolchain-packages.tsv artifacts/
make -s -C source O=/build/linux-out ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ kernelrelease > artifacts/kernelrelease.txt
tar --exclude=build --exclude=source -C modules-stage -czf artifacts/modules.tar.gz lib/modules
cd artifacts
sha256sum Image vmlinuz.efi sm7150-google-sunfish.dtb kernel.config modules.tar.gz > SHA256SUMS
cp -a . /export/
printf '%s\n' 'PASS: reference kernel, EFI image, Sunfish DTB and matching modules built.'
BUILD

podman build -t localhost/sunfish-mobian-toolchain:20261025 \
    -f "$root_dir/platforms/sunfish/Containerfile.mobian-reference" "$root_dir/platforms/sunfish"
if [ "$(uname -s)" = Darwin ]; then
    podman machine ssh sudo modprobe loop ext4
fi
podman image inspect localhost/sunfish-mobian-toolchain:20261025 \
    --format '{{.Id}}' > "$cache_dir/artifacts/build-container-id.txt"
podman run --rm --privileged --name sunfish-mobian-reference-build \
    -e BUILD_JOBS="${BUILD_JOBS:-4}" \
    -v "$root_dir:/workspace:ro" \
    -v "$cache_dir:/inputs" \
    -v "$cache_dir/artifacts:/export" \
    localhost/sunfish-mobian-toolchain:20261025 \
    sh /inputs/build-in-container.sh
