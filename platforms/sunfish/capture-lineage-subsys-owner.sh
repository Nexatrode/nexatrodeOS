#!/bin/sh
set -eu

command -v adb >/dev/null 2>&1 || {
    printf '%s\n' 'adb is required.' >&2
    exit 2
}

adb_serial=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --serial)
            shift
            if [ "$#" -eq 0 ] || [ -z "$1" ]; then
                printf '%s\n' '--serial requires an ADB serial.' >&2
                exit 2
            fi
            adb_serial=$1
            ;;
        -h|--help)
            printf '%s\n' 'Usage: capture-lineage-subsys-owner.sh [--serial ADB_SERIAL]'
            exit 0
            ;;
        *)
            printf 'Unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

adb_cmd() {
    if [ -n "$adb_serial" ]; then
        adb -s "$adb_serial" "$@"
    else
        adb "$@"
    fi
}

if [ -n "$adb_serial" ]; then
    device_count=$(adb devices | awk -v serial="$adb_serial" 'NR > 1 && $1 == serial && $2 == "device" { count++ } END { print count + 0 }')
    if [ "$device_count" -ne 1 ]; then
        printf 'Expected authorized ADB device %s; found %s matching devices.\n' "$adb_serial" "$device_count" >&2
        exit 2
    fi
else
    device_count=$(adb devices | awk 'NR > 1 && $2 == "device" { count++ } END { print count + 0 }')
    if [ "$device_count" -ne 1 ]; then
        printf 'Expected exactly one authorized ADB device; found %s. Use --serial to select one.\n' "$device_count" >&2
        exit 2
    fi
fi

product=$(adb_cmd shell getprop ro.product.device | tr -d '\r')
lineage_version=$(adb_cmd shell getprop ro.lineage.version | tr -d '\r')
uid=$(adb_cmd shell id -u | tr -d '\r')
if [ "$product" != sunfish ] || [ -z "$lineage_version" ]; then
    printf '%s\n' 'Refusing capture: the connected device is not identified as Sunfish running LineageOS.' >&2
    exit 2
fi
if [ "$uid" != 0 ]; then
    printf '%s\n' 'Refusing capture: ADB shell is not root. Enable Rooted debugging and reconnect; this script will not restart adbd.' >&2
    exit 2
fi

subsys_node=absent
if adb_cmd shell test -c /dev/subsys_modem; then
    subsys_node=character_device
elif adb_cmd shell test -e /dev/subsys_modem; then
    subsys_node=present_not_character_device
fi

printf 'device=%s\nlineage=%s\nadb_uid=%s\n' "$product" "$lineage_version" "$uid"
printf 'subsys_modem_node=%s\n' "$subsys_node"
adb_cmd shell sh -s <<'REMOTE'
found=0
matches=$(toybox find /proc/[0-9]*/fd -maxdepth 1 -type l \
    \( -lname /dev/subsys_modem -o -lname '*/firmware_name' \) -print 2>/dev/null || true)
for fd in $matches; do
    target=$(readlink "$fd" 2>/dev/null) || continue
    pid=${fd#/proc/}
    pid=${pid%%/*}
    fdnum=${fd##*/}
    comm=$(cat "/proc/$pid/comm" 2>/dev/null || printf '%s' unavailable)
    cmdline=$(tr '\000' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true)
    cmdline=$(printf '%s' "$cmdline" | cut -c1-240)
    printf 'fd_owner pid=%s fd=%s comm=%s target=%s cmdline=%s\n' \
        "$pid" "$fdnum" "$comm" "$target" "$cmdline"
    found=1
done
if [ "$found" -eq 0 ]; then
    printf '%s\n' 'fd_owner=none_observed'
fi
REMOTE
printf '%s\n' 'scope=persistent descriptors only; a transient firmware_name sysfs write may not remain open long enough to observe'
