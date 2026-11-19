#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_GLINK_DONE_PROBE=1
export SUNFISH_MODEM_GLINK_RX_OUT_DIR="${SUNFISH_MODEM_GLINK_DONE_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-done-probe}"
exec "$root_dir/platforms/sunfish/build-modem-glink-rx-probe.sh"
