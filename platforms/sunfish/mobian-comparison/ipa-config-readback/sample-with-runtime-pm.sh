#!/bin/sh
# Reference Linux only. Temporarily resume IPA, sample, and restore autosuspend.
set -eu
work=/run/sunfish-coldwifi
power=/sys/bus/platform/devices/1e40000.ipa/power
test "$(id -u)" = 0
test "$(uname -r)" = 7.1.0-sm7150+
tr '\000' '\n' </proc/device-tree/compatible | grep -qx google,sunfish
test "$(cat "$power/control")" = auto
test "$(cat "$power/runtime_status")" = suspended
printf '%s  %s\n' 8978901e9718fa64be64ccd915efd332b6db9689869822cdacc3087d0be61972 "$work/sunfish_ipa_config_readback.ko" | sha256sum -c -
changed=0
cleanup() {
    if [ "$changed" = 1 ]; then
        if ! printf 'auto\n' > "$power/control"; then
            echo 'ERROR: failed to restore IPA runtime power control' >&2
        fi
    fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
changed=1
printf 'on\n' > "$power/control"
n=0
while [ "$(cat "$power/runtime_status")" != active ]; do
    n=$((n+1))
    if [ "$n" -ge 20 ]; then echo 'IPA did not become active' >&2; exit 1; fi
    sleep 1
done
insmod "$work/sunfish_ipa_config_readback.ko"
rmmod sunfish_ipa_config_readback
dmesg | grep 'sunfish-ipa-config:' | tail -n 1
printf 'auto\n' > "$power/control"
changed=0
test "$(cat "$power/control")" = auto
printf 'IPA runtime power control restored to auto\n'
