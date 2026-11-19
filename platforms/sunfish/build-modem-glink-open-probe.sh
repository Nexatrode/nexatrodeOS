#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_GLINK_OPEN_PROBE=1
export SUNFISH_MODEM_GLINK_PENDING_OUT_DIR="${SUNFISH_MODEM_GLINK_OPEN_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-open-probe}"
exec "$root_dir/platforms/sunfish/build-modem-glink-pending-probe.sh"
