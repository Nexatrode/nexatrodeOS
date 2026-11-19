#!/bin/sh
# A single attempt with the reference startup script. No automatic restart.
set -eu
work=/run/sunfish-coldwifi
test -f "$work/prepared"
test ! -e "$work/attempted"
touch "$work/attempted"
trace=/sys/kernel/tracing
mkdir -p "$work/logs"
# Return probes add observability when this kernel exposes the named symbols.
# Unsupported probes are recorded; their absence is not treated as a result.
if [ -w "$trace/kprobe_events" ]; then
    for function in qcom_scm_pas_init_image qcom_scm_pas_mem_setup qcom_scm_pas_auth_and_reset qcom_scm_assign_mem; do
        if grep -q " $function$" /proc/kallsyms; then
            if printf 'r:sunfish_ref/%s %s ret=$retval\n' "$function" "$function" >> "$trace/kprobe_events" 2>> "$work/logs/probe-setup.txt"; then
                printf '1\n' > "$trace/events/sunfish_ref/$function/enable"
            fi
        fi
    done
else
    printf 'Kprobe return events unavailable in the unchanged reference kernel; use PAS/error logs and remoteproc state.\n' > "$work/logs/probe-setup.txt"
fi
if [ -w /sys/kernel/debug/dynamic_debug/control ]; then
    for source in drivers/remoteproc/qcom_q6v5_pas.c drivers/soc/qcom/mdt_loader.c drivers/net/wireless/ath/ath10k/qmi.c drivers/net/ipa/ipa_qmi.c; do
        printf 'file %s +p\n' "$source" > /sys/kernel/debug/dynamic_debug/control 2>> "$work/logs/probe-setup.txt" || true
    done
fi
systemctl start sunfish-wifi-rmtfs.service sunfish-wifi-tftp.service
systemctl is-active sunfish-wifi-rmtfs.service sunfish-wifi-tftp.service
timeout 3 stdbuf -oL qrtr-lookup > "$work/logs/providers-before-pas.txt" 2>&1 || test "$?" = 124
awk '$1 == 14 {rmtfs=1} $1 == 4096 {tftp=1} END {exit !(rmtfs && tftp)}' "$work/logs/providers-before-pas.txt"
# The pinned integration's softdep loads IPA and the kernel PD mapper first.
modprobe qcom_q6v5_pas
test -L /sys/bus/platform/devices/1e40000.ipa/driver
rp=
for directory in /sys/class/remoteproc/remoteproc*; do
    test -d "$directory" || continue
    if [ "$(cat "$directory/name")" = modem ]; then rp=$directory; fi
done
test -n "$rp"
test "$(cat "$rp/state")" = offline
printf 'disabled\n' > "$rp/recovery"
test "$(cat "$rp/recovery")" = disabled
printf '%s\n' "$rp" > "$work/modem-remoteproc"
timeout 3 stdbuf -oL qrtr-lookup > "$work/logs/providers-before-modem.txt" 2>&1 || test "$?" = 124
# qcom_common creates the PD mapper auxiliary device in the remoteproc prepare
# callback. Its locator endpoint cannot be required before the first start.
# Keep the already-running userspace storage/firmware providers mandatory.
awk '$1 == 14 {rmtfs=1} $1 == 4096 {tftp=1} END {exit !(rmtfs && tftp)}' "$work/logs/providers-before-modem.txt"
printf 'MPSS STARTING: %s; one reference-script attempt\n' "$rp"
set +e
sh /usr/share/sunfish-reference/wcn/sbin/wcn-wifi-bringup.sh > "$work/logs/bringup.txt" 2>&1
result=$?
set -e
printf '%s\n' "$result" > "$work/bringup-result"
timeout 3 stdbuf -oL qrtr-lookup > "$work/logs/providers-after-start.txt" 2>&1 || test "$?" = 124
cat "$rp/state" > "$work/modem-state-after.txt"
cat /proc/interrupts > "$work/interrupts-after-start.txt"
dmesg > "$work/logs/dmesg-after.txt"
journalctl -b --no-pager -o short-monotonic > "$work/logs/journal-after.txt"
if [ -r "$trace/trace" ]; then cat "$trace/trace" > "$work/logs/secure-call-returns.txt"; fi
sha256sum -c "$work/efs-before.sha256" > "$work/efs-check-after-start.txt"
iw dev > "$work/logs/iw-dev.txt"
printf 'COLD WIFI REFERENCE RESULT: %s\n' "$result"
cat "$work/logs/bringup.txt" "$work/logs/iw-dev.txt" "$work/efs-check-after-start.txt"
exit "$result"
