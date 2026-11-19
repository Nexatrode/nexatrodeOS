#!/bin/sh
# Runs only in the local build container. No handset is accessed here.
set -eu
if ! blkid -p /inputs/userspace.ext4 >/dev/null 2>&1; then
    python3 - <<'PY'
with open('/inputs/userspace.ext4', 'rb') as stream:
    if any(stream.read(1024 * 1024)):
        raise SystemExit('Refusing to format a non-empty, unrecognized userspace image')
PY
    mkfs.ext4 -q -L SunfishUsers /inputs/userspace.ext4
fi
test "$(blkid -p -s LABEL -o value /inputs/userspace.ext4)" = SunfishUsers
test -z "$(losetup -j /inputs/userspace.ext4)" || {
    echo 'Private build image is already mounted; refusing concurrent mounts.' >&2; exit 2;
}
for index in 0 1 2 3 4 5 6 7; do
    test -b "/dev/loop$index" || mknod "/dev/loop$index" b 7 "$index"
done
mkdir -p /build
mount -o loop /inputs/userspace.ext4 /build
trap 'cd /; umount /build' EXIT
root=/build/comparison-rootfs
if [ ! -f /build/rootfs-base.ready ]; then
test ! -e "$root" || {
    echo 'Refusing to overwrite an existing userspace build.' >&2; exit 2;
}
mirror=https://snapshot.debian.org/archive/debian/20261025T000000Z
python3 - "$mirror/dists/trixie/Release" <<'PY'
import hashlib, sys, urllib.request
with urllib.request.urlopen(sys.argv[1], timeout=60) as stream:
    data = stream.read()
expected = 'ed56aac47e7911e65ee63aae8d67e29f20e023f840e49f8ed037043a33de138a'
if hashlib.sha256(data).hexdigest() != expected:
    raise SystemExit('Debian snapshot Release hash differs')
PY
mkdir -p "$root/usr/sbin"
printf '#!/bin/sh\nexit 101\n' > "$root/usr/sbin/policy-rc.d"
chmod 755 "$root/usr/sbin/policy-rc.d"
debootstrap --arch=arm64 --variant=minbase \
    --include=systemd-sysv,udev,openssh-server,qrtr-tools,rmtfs,tqftpserv,protection-domain-mapper,iw,wpasupplicant,iproute2,iputils-ping,iperf3,kmod,busybox,ca-certificates,procps \
    trixie "$root" "$mirror"
touch /build/rootfs-base.ready
fi
if [ "${BUILD_STAGE:-all}" = prepare ]; then
    echo 'PASS: minimal Debian userspace prepared; matching modules will be added after the kernel build'
    exit 0
fi
(cd /inputs/artifacts && sha256sum -c SHA256SUMS)
mkdir -p "$root/etc/sunfish-comparison" "$root/root/.ssh" \
    "$root/usr/local/sbin" "$root/usr/share/sunfish-reference" \
    "$root/etc/systemd/system/ssh.service.d"
cp /workspace/platforms/sunfish/mobian-comparison.json "$root/etc/sunfish-comparison/reference.json"
cp /inputs/audit.json "$root/etc/sunfish-comparison/source-audit.json"
cp /inputs/artifacts/SHA256SUMS "$root/etc/sunfish-comparison/kernel-artifacts.sha256"
cp /inputs/comparison-key.pub "$root/root/.ssh/authorized_keys"
chmod 700 "$root/root/.ssh"
chmod 600 "$root/root/.ssh/authorized_keys"
mkdir -p "$root/usr/lib"
# Preserve Debian's /lib -> usr/lib link; the module archive starts at lib/.
tar -xzf /inputs/artifacts/modules.tar.gz --strip-components=1 -C "$root/usr/lib"
find "$root/lib/modules" -maxdepth 2 -type l \( -name build -o -name source \) -delete
cp /workspace/platforms/sunfish/mobian-comparison/init "$root/init"
cp /workspace/platforms/sunfish/mobian-comparison/usb-gadget.sh "$root/usr/local/sbin/sunfish-reference-usb"
cp /workspace/platforms/sunfish/mobian-comparison/snapshot.sh "$root/usr/local/sbin/sunfish-reference-snapshot"
chmod 755 "$root/init" "$root/usr/local/sbin/sunfish-reference-usb" "$root/usr/local/sbin/sunfish-reference-snapshot"
cp -a /workspace/.cache/mobian-pixel4a-port-789679/work/wcn "$root/usr/share/sunfish-reference/"
printf 'softdep qcom_q6v5_pas pre: ipa qcom_pd_mapper\n' > "$root/etc/modprobe.d/ipa-first.conf"
printf 'blacklist ath10k_snoc\n' > "$root/etc/modprobe.d/ath10k-snoc-blacklist.conf"
printf 'blacklist ipa\nblacklist qcom_q6v5_pas\n' > "$root/etc/modprobe.d/sunfish-reference-cold-hold.conf"
printf 'unselected\n' > "$root/etc/sunfish-comparison/storage-policy"
cat > "$root/etc/systemd/system/sunfish-comparison.target" <<'UNIT'
[Unit]
Description=Sunfish comparison staging environment
Requires=basic.target
After=basic.target
Wants=sunfish-reference-usb.service sunfish-reference-snapshot.service ssh.service
AllowIsolate=yes
UNIT
cat > "$root/etc/systemd/system/sunfish-reference-usb.service" <<'UNIT'
[Unit]
Description=USB-only comparison transport
After=systemd-udev-trigger.service
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/sbin/sunfish-reference-usb
UNIT
cat > "$root/etc/systemd/system/sunfish-reference-snapshot.service" <<'UNIT'
[Unit]
Description=Record state before reference radio services
After=basic.target
[Service]
Type=oneshot
ExecStart=/usr/local/sbin/sunfish-reference-snapshot
StandardOutput=journal+console
UNIT
cat > "$root/etc/systemd/system/ssh.service.d/comparison.conf" <<'UNIT'
[Unit]
After=sunfish-reference-usb.service
ConditionPathExists=/sys/class/net/usb0
[Service]
ExecStartPre=
ExecStartPre=/usr/bin/ssh-keygen -A
ExecStartPre=/usr/sbin/sshd -t
UNIT
cat > "$root/etc/ssh/sshd_config.d/comparison.conf" <<'SSH'
PermitRootLogin prohibit-password
PasswordAuthentication no
KbdInteractiveAuthentication no
ListenAddress 172.16.42.1
ListenAddress ::
SSH
# Hold every auto-start path from the installed packages. Preserve the original
# community units under /usr/share for later comparison after inputs are selected.
for unit in rmtfs.service tqftpserv.service pd-mapper.service protection-domain-mapper.service; do
    ln -sf /dev/null "$root/etc/systemd/system/$unit"
done
ln -sf sunfish-comparison.target "$root/etc/systemd/system/default.target"
printf '[Journal]\nStorage=volatile\n' > "$root/etc/systemd/journald.conf"
printf 'sunfish-reference\n' > "$root/etc/hostname"
: > "$root/etc/fstab"
: > "$root/etc/machine-id"
chroot "$root" dpkg-query -W > /inputs/artifacts/userspace-packages.tsv
cp /inputs/artifacts/userspace-packages.tsv "$root/etc/sunfish-comparison/"
mkdir -p "$root/run/sshd"
chroot "$root" ssh-keygen -A
chroot "$root" /usr/sbin/sshd -t
rm -f "$root"/etc/ssh/ssh_host_*_key "$root"/etc/ssh/ssh_host_*_key.pub
test -c "$root/dev/console" || mknod "$root/dev/console" c 5 1
test -c "$root/dev/null" || mknod "$root/dev/null" c 1 3
find "$root/lib/firmware" -type f 2>/dev/null > /build/rootfs-firmware-files.txt || true
test ! -s /build/rootfs-firmware-files.txt || {
    echo 'Unexpected firmware payload in staging userspace; inspect before packaging.' >&2; exit 2;
}
chroot "$root" /usr/bin/systemd-analyze verify \
    sunfish-comparison.target sunfish-reference-usb.service sunfish-reference-snapshot.service
# Retrieve only the EFI executable package; no bootctl install or disk writes.
# Keep its provenance before dropping package indexes from the RAM image.
if [ ! -s /inputs/artifacts/BOOTAA64.EFI ]; then
    chroot "$root" /bin/sh -c 'cd /tmp; apt-get download systemd-boot-efi'
    package=$(find "$root/tmp" -maxdepth 1 -name 'systemd-boot-efi_*_arm64.deb' -print)
    test -n "$package"
    sha256sum "$package" > /inputs/artifacts/efi-package.sha256
    dpkg-deb -f "$package" Package Version Architecture > /inputs/artifacts/efi-package.txt
    mkdir -p /build/efi-package
    dpkg-deb -x "$package" /build/efi-package
    cp /build/efi-package/usr/lib/systemd/boot/efi/systemd-bootaa64.efi /inputs/artifacts/BOOTAA64.EFI
    rm -f "$package"
fi
find "$root/var/cache/apt" -type f -delete
for directory in usr/share/doc usr/share/man usr/share/locale var/lib/apt/lists; do
    if [ -d "$root/$directory" ]; then find "$root/$directory" -mindepth 1 -delete; fi
done
python3 - <<'PY'
from pathlib import Path
import hashlib, json
root = Path('/workspace')
paths = [
    'platforms/sunfish/mobian-comparison.json',
    'platforms/sunfish/audit-mobian-comparison.py',
    'platforms/sunfish/build-mobian-reference.sh',
    'platforms/sunfish/build-mobian-userspace.sh',
    'platforms/sunfish/Containerfile.mobian-reference',
    'platforms/sunfish/mobian-comparison/init',
    'platforms/sunfish/mobian-comparison/usb-gadget.sh',
    'platforms/sunfish/mobian-comparison/snapshot.sh',
    'platforms/sunfish/mobian-comparison/build-userspace.sh',
]
data = {p: hashlib.sha256((root/p).read_bytes()).hexdigest() for p in paths}
output = Path('/inputs/artifacts/comparison-source-inputs.json')
output.write_text(json.dumps(data, indent=2) + '\n')
Path('/build/comparison-rootfs/etc/sunfish-comparison/build-inputs.json').write_bytes(output.read_bytes())
PY
cd "$root"
for directory in proc sys dev run; do
    if mountpoint -q "$root/$directory"; then
        echo "Unexpected live mount in userspace archive: $directory" >&2
        exit 2
    fi
done
find . -print0 > /build/rootfs-files.unsorted
LC_ALL=C sort -z /build/rootfs-files.unsorted > /build/rootfs-files.sorted
cpio --null -o --format=newc < /build/rootfs-files.sorted \
    > /build/comparison-stage0.cpio 2>/build/cpio.log
gzip -1 -c /build/comparison-stage0.cpio > /inputs/artifacts/comparison-stage0.cpio.gz
# The kernel's initramfs XZ decoder supports CRC32, not xz's default CRC64.
xz --check=crc32 -T2 -6 -c /build/comparison-stage0.cpio > /inputs/artifacts/comparison-stage0.cpio.xz
cd /inputs/artifacts
sha256sum comparison-stage0.cpio.gz comparison-stage0.cpio.xz userspace-packages.tsv BOOTAA64.EFI > USERSPACE-SHA256SUMS
echo 'PASS: desktop-free staging userspace; radio services held; no firmware payload'
