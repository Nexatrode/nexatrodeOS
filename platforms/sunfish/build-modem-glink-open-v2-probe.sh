#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_MODEM_GLINK_OPEN_OUT_DIR="${SUNFISH_MODEM_GLINK_OPEN_V2_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-open-v2-probe}"
"$root_dir/platforms/sunfish/build-modem-glink-open-probe.sh"
python3 "$root_dir/platforms/sunfish/validate-glink-open-v2-inputs.py" > "$SUNFISH_MODEM_GLINK_OPEN_OUT_DIR/glink-open-v2-input-proof.json"
