#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_GLINK_PENDING_PROBE=1
export SUNFISH_MODEM_GLINK_VERSION_OUT_DIR="${SUNFISH_MODEM_GLINK_PENDING_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-pending-probe}"
exec "$root_dir/platforms/sunfish/build-modem-glink-version-probe.sh"
