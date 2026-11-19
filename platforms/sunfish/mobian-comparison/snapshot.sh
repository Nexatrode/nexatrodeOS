#!/bin/sh
set -eu
out=/run/sunfish-comparison
mkdir -p "$out"
cat /proc/uptime > "$out/uptime-before.txt"
uname -a > "$out/uname.txt"
cat /proc/cmdline > "$out/cmdline.txt"
cat /proc/interrupts > "$out/interrupts-before.txt"
cat /proc/modules > "$out/modules-before.txt"
dmesg > "$out/kernel-before.log"
{
    for d in /sys/class/remoteproc/remoteproc*; do
        [ -d "$d" ] || continue
        printf '%s name=%s state=%s firmware=%s\n' "$d" \
            "$(cat "$d/name")" "$(cat "$d/state")" "$(cat "$d/firmware")"
    done
} > "$out/remoteproc-before.txt"
if [ -r /sys/firmware/fdt ]; then
    sha256sum /sys/firmware/fdt > "$out/live-fdt.sha256"
fi
sha256sum /etc/sunfish-comparison/reference.json > "$out/reference-manifest.sha256"
for unit in rmtfs.service tqftpserv.service pd-mapper.service protection-domain-mapper.service; do
    state=$(systemctl is-enabled "$unit" 2>/dev/null || true)
    test "$state" = masked || { echo "STAGE0 ERROR: $unit is not masked"; exit 1; }
done
test "$(cat /etc/sunfish-comparison/storage-policy)" = unselected
echo 'STAGE0 CHECK: reference radio services masked; EFS policy unselected'
echo 'SUNFISH COMPARISON STAGE0 READY: startup snapshot saved in RAM'
echo 'REFERENCE LAUNCH: pending firmware inputs, storage policy and boot-handoff review'
