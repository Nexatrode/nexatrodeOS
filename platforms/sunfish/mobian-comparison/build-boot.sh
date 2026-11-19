#!/bin/sh
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends dosfstools gdisk mtools swig python3-dev xxd libgnutls28-dev
for index in 0 1 2 3 4 5 6 7; do
    test -b "/dev/loop$index" || mknod "/dev/loop$index" b 7 "$index"
done
for image in build; do
    test -z "$(losetup -j "/inputs/$image.ext4")" || {
        echo 'Wait for the other build container to release its filesystem.' >&2; exit 2;
    }
done
mkdir -p /build
mount -o loop /inputs/build.ext4 /build
trap 'cd /; umount /build' EXIT
(cd /inputs/artifacts && sha256sum -c SHA256SUMS && sha256sum -c USERSPACE-SHA256SUMS)
cd /build
if [ ! -e u-boot-source ]; then
    mkdir u-boot-source
    tar -xzf /inputs/downloads/u-boot-f8c04469.tar.gz -C u-boot-source --strip-components=1
fi
python3 - <<'PY'
from pathlib import Path
import tarfile
relative = 'arch/arm/mach-snapdragon/board.c'
with tarfile.open('/inputs/downloads/u-boot-f8c04469.tar.gz') as archive:
    prefix = archive.getnames()[0].split('/')[0]
    source = archive.extractfile(prefix + '/' + relative).read().decode()
anchor = '/* Stolen from arch/arm/mach-apple/board.c */'
assert source.count(anchor) == 1
helper = Path('/workspace/platforms/sunfish/mobian-comparison/ram-esp-reserve.c').read_text()
source = source.replace(anchor, helper + '\n' + anchor)
anchor = '\t/* We need to be fairly conservative here as we support boards with just 1G of TOTAL RAM */'
assert source.count(anchor) == 1
source = source.replace(anchor, '\tcomparison_reserve_ram_esp();\n\n' + anchor)
Path('/build/u-boot-source', relative).write_text(source)
PY
cp /workspace/platforms/sunfish/mobian-comparison/ram-esp.env u-boot-source/board/qualcomm/sunfish-comparison.env
make -C u-boot-source O=/build/u-boot-out CROSS_COMPILE=aarch64-linux-gnu- \
    qcom_defconfig qcom-phone.config tauchgang.config
u-boot-source/scripts/config --file u-boot-out/.config \
    --set-str DEFAULT_DEVICE_TREE qcom/sm7150-google-sunfish \
    --set-str ENV_DEFAULT_ENV_TEXT_FILE board/qualcomm/sunfish-comparison.env \
    --disable USE_PREBOOT --disable FASTBOOT_FLASH --disable FASTBOOT_OEM_RUN \
    --disable EFI_RUNTIME_UPDATE_CAPSULE --disable EFI_CAPSULE_ON_DISK \
    --disable EFI_CAPSULE_ON_DISK_EARLY --disable EFI_VARIABLE_FILE_STORE \
    --enable EFI_VARIABLE_NO_STORE
make -C u-boot-source O=/build/u-boot-out CROSS_COMPILE=aarch64-linux-gnu- olddefconfig
python3 - <<'PY'
from pathlib import Path
text = Path('/build/u-boot-out/.config').read_text()
for setting in ['CONFIG_BLKMAP=y', 'CONFIG_CMD_BLKMAP=y',
                'CONFIG_SAVE_PREV_BL_INITRAMFS_START_ADDR=y',
                'CONFIG_EFI_VARIABLE_NO_STORE=y']:
    if setting not in text:
        raise SystemExit('Missing required temporary-boot setting: ' + setting)
for setting in ['CONFIG_FASTBOOT_FLASH=y', 'CONFIG_FASTBOOT_OEM_RUN=y',
                'CONFIG_EFI_RUNTIME_UPDATE_CAPSULE=y', 'CONFIG_EFI_VARIABLE_FILE_STORE=y']:
    if setting in text:
        raise SystemExit('Unexpected persistent-write feature: ' + setting)
PY
make -j4 -C u-boot-source O=/build/u-boot-out CROSS_COMPILE=aarch64-linux-gnu-
cp u-boot-out/u-boot-nodtb.bin /inputs/artifacts/
cp u-boot-out/u-boot.dtb /inputs/artifacts/uboot-sunfish.dtb
cp u-boot-out/.config /inputs/artifacts/uboot.config
dpkg-query -W > /inputs/artifacts/uboot-toolchain-packages.tsv
aarch64-linux-gnu-gcc --version > /inputs/artifacts/uboot-compiler.txt
disk=/build/comparison-esp.img
truncate -s 64M "$disk"
sgdisk --clear --new=1:2048:0 --typecode=1:ef00 --change-name=1:SunfishCompare "$disk"
partition_sectors=$(sgdisk -i 1 "$disk" | awk '/Partition size:/ {print $3}')
truncate -s "$((partition_sectors * 512))" /build/comparison-esp.fat
mkfs.vfat -F 32 -n SUNFISHCMP /build/comparison-esp.fat
fat=/build/comparison-esp.fat
mmd -i "$fat" ::/EFI ::/EFI/BOOT ::/loader ::/loader/entries
mcopy -i "$fat" /inputs/artifacts/BOOTAA64.EFI ::/EFI/BOOT/BOOTAA64.EFI
mcopy -i "$fat" /inputs/artifacts/vmlinuz.efi ::/vmlinuz.efi
mcopy -i "$fat" /inputs/artifacts/comparison-stage0.cpio.xz ::/stage0.cpio.xz
mcopy -i "$fat" /inputs/artifacts/sm7150-google-sunfish.dtb ::/sunfish.dtb
printf 'default sunfish.conf\ntimeout 0\neditor no\n' > /build/loader.conf
cat > /build/sunfish.conf <<'ENTRY'
title Sunfish Linux comparison staging
linux /vmlinuz.efi
initrd /stage0.cpio.xz
devicetree /sunfish.dtb
options rdinit=/init console=tty0 console=ttyMSM0,115200n8 console=ttyGS0,115200n8 loglevel=4 panic=0
ENTRY
mcopy -i "$fat" /build/loader.conf ::/loader/loader.conf
mcopy -i "$fat" /build/sunfish.conf ::/loader/entries/sunfish.conf
fsck.vfat -n "$fat"
dd if="$fat" of="$disk" bs=512 seek=2048 conv=notrunc status=none
sgdisk --verify "$disk"
cp "$disk" /inputs/artifacts/comparison-esp.img
echo 'PASS: U-Boot and RAM ESP transport candidate built; not yet booted on Sunfish'
