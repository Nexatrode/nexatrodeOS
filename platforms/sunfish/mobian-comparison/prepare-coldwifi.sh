#!/bin/sh
# Run only on the temporary Linux image, after uploading the verified RAM bundle.
set -eu
umask 077
work=/run/sunfish-coldwifi
test "$(id -u)" = 0
test "$(uname -r)" = 7.1.0-sm7150+
tr '\000' '\n' </proc/device-tree/compatible | grep -qx google,sunfish
test -d "$work/firmware"
test -f "$work/firmware.sha256"
test ! -e "$work/prepared"
test -c /dev/qcom_rmtfs_mem1
if lsmod | grep -Eq '^(ipa|qcom_q6v5_pas|ath10k_snoc) '; then
    echo 'COLD WIFI STOP: a held driver is already loaded; use the cold-hold image' >&2
    exit 1
fi
for unit in rmtfs.service tqftpserv.service pd-mapper.service protection-domain-mapper.service; do
    test "$(systemctl is-enabled "$unit" 2>/dev/null || true)" = masked
done
(cd "$work/firmware" && sha256sum -c ../firmware.sha256)
for program in rmtfs tqftpserv qrtr-lookup modprobe iw wpa_supplicant stdbuf; do
    sha256sum "$(command -v "$program")"
done > "$work/program-inputs.sha256"
dpkg-query -W rmtfs tqftpserv protection-domain-mapper qrtr-tools iw wpasupplicant > "$work/package-versions.tsv"
cat /proc/cmdline > "$work/kernel-cmdline.txt"
mkdir -p /usr/lib/firmware
mount --bind "$work/firmware" /usr/lib/firmware
mount -o remount,bind,ro /usr/lib/firmware
mkdir -p "$work/efs" /tmp/tqftpserv
for pair in modemst1:modem_fs1 modemst2:modem_fs2 fsg:modem_fsg fsc:modem_fsc; do
    part=${pair%:*}; file=${pair#*:}
    test -b "/dev/disk/by-partlabel/$part"
    blockdev --setro "/dev/disk/by-partlabel/$part"
    test "$(blockdev --getro "/dev/disk/by-partlabel/$part")" = 1
    sha256sum "/dev/disk/by-partlabel/$part" >> "$work/efs-before.sha256"
    dd if="/dev/disk/by-partlabel/$part" of="$work/efs/$file" bs=1048576 status=none
done
cat > /run/systemd/system/sunfish-wifi-rmtfs.service <<'UNIT'
[Unit]
Description=Sunfish comparison RMTFS using private RAM EFS copies
[Service]
Type=simple
ExecStart=/usr/bin/stdbuf -oL -eL /usr/bin/rmtfs -r -v -o /run/sunfish-coldwifi/efs
Restart=no
ProtectSystem=strict
ProtectHome=yes
DevicePolicy=closed
DeviceAllow=/dev/qcom_rmtfs_mem1 rw
ReadWritePaths=/run/sunfish-coldwifi
StandardOutput=journal
StandardError=journal
UNIT
cat > /run/systemd/system/sunfish-wifi-tftp.service <<'UNIT'
[Unit]
Description=Sunfish comparison TQFTP with RAM firmware and temporary files
[Service]
Type=simple
ExecStart=/usr/bin/stdbuf -oL -eL /usr/bin/tqftpserv
Restart=no
ProtectSystem=strict
ProtectHome=yes
DevicePolicy=closed
ReadWritePaths=/tmp/tqftpserv
StandardOutput=journal
StandardError=journal
UNIT
systemd-analyze verify /run/systemd/system/sunfish-wifi-rmtfs.service /run/systemd/system/sunfish-wifi-tftp.service
systemctl daemon-reload
printf 'RAM copies + rmtfs -r; raw EFS block devices read-only; no persistent EFS writes\n' > "$work/storage-policy"
findmnt -rn -o SOURCE,TARGET,FSTYPE,OPTIONS > "$work/mounts-before.txt"
cat /proc/interrupts > "$work/interrupts-before.txt"
dmesg > "$work/dmesg-before.txt"
touch "$work/prepared"
echo 'COLD WIFI PREPARED: firmware verified; EFS copied into RAM; modem not started'
