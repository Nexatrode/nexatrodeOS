#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
INVENTORY=${SUNFISH_MSM_BUS_INVENTORY:-"$ROOT/.cache/pixel-sunfish/msm-bus-client-inventory-current-20261023-fabric.json"}
OUT_DIR=${1:-"$ROOT/.cache/pixel-sunfish/msm-bus-update-trace/capture-$(date +%Y%m%d-%H%M%S)"}

command -v adb >/dev/null 2>&1 || {
    printf '%s\n' 'adb is required.' >&2
    exit 2
}
command -v python3 >/dev/null 2>&1 || {
    printf '%s\n' 'python3 is required.' >&2
    exit 2
}
if [ ! -f "$INVENTORY" ]; then
    printf 'MSM-bus inventory not found: %s\n' "$INVENTORY" >&2
    exit 2
fi

device_count=$(adb devices | awk 'NR > 1 && $2 == "device" { count++ } END { print count + 0 }')
if [ "$device_count" -ne 1 ]; then
    printf 'Expected exactly one authorized ADB device; found %s.\n' "$device_count" >&2
    exit 2
fi

product=$(adb shell getprop ro.product.device | tr -d '\r')
if [ "$product" != sunfish ]; then
    printf 'Refusing capture: expected Sunfish, found %s.\n' "$product" >&2
    exit 2
fi

mkdir -p "$OUT_DIR"
raw=$(mktemp)
rooted=1
cleanup() {
    if [ "$rooted" -eq 1 ]; then
        adb unroot >/dev/null 2>&1 || true
    fi
    rm -f "$raw"
}
trap cleanup EXIT HUP INT TERM
if ! adb root; then
    printf '%s\n' 'Could not temporarily restart adbd as root.' >&2
    exit 2
fi
adb wait-for-device
if ! adb shell id | grep -q 'uid=0(root)'; then
    printf '%s\n' 'Temporary root ADB did not provide root access.' >&2
    exit 2
fi
if ! adb shell dmesg >"$raw"; then
    printf '%s\n' 'Could not read the current kernel ring buffer.' >&2
    exit 2
fi

filtered="$OUT_DIR/msm-bus-update-trace.txt"
if ! grep -E \
    'register_client_adhoc:(find path\.src|Failed to find path\.src|Client handle)|update_request_adhoc:|BW MBps: AB:|Err updating client request|Err updating path|Update path failed|error sending active/awake sets|error sending wake sets|error sending sleep sets|bandwidth request failed' \
    "$raw" >"$filtered"; then
    : >"$filtered"
fi
if [ ! -s "$filtered" ]; then
    printf '%s\n' 'No MSM-bus/devbw trace records found; keep the Pixel in place and verify the temporary boot completed.' >&2
    exit 1
fi

report="$OUT_DIR/msm-bus-update-trace.json"
python3 "$ROOT/platforms/sunfish/audit-msm-bus-request-trace.py" \
    "$filtered" --inventory "$INVENTORY" --output "$report"

adb unroot
adb wait-for-device
if adb shell id | grep -q 'uid=0(root)'; then
    printf '%s\n' 'ADB remained root after adb unroot.' >&2
    exit 2
fi
rooted=0

expected=$(python3 - "$INVENTORY" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as stream:
    inventory = json.load(stream)
print(len(inventory.get("runtime_registration_snapshot", {}).get("registrations", [])))
PY
)
observed=$(python3 - "$report" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as stream:
    report = json.load(stream)
print(report.get("summary", {}).get("registered_handles", 0))
PY
)
if [ "$observed" -lt "$expected" ]; then
    registration_status=partial
else
    registration_status=complete
fi

samples=$(python3 - "$report" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as stream:
    report = json.load(stream)
print(report.get("summary", {}).get("devbw_samples", 0))
PY
)
printf 'product=%s\nfiltered_kernel_lines=%s\nregistered_handles=%s/%s\nregistration_status=%s\ndevbw_samples=%s\ntrace=%s\nreport=%s\n' \
    "$product" "$(wc -l <"$filtered" | tr -d ' ')" "$observed" "$expected" \
    "$registration_status" "$samples" "$filtered" "$report"
