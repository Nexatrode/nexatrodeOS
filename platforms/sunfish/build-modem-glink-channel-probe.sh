#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_GLINK_CHANNEL_PROBE=1
export SUNFISH_MODEM_GLINK_OPEN_OUT_DIR="${SUNFISH_MODEM_GLINK_CHANNEL_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-channel-probe}"
exec "$root_dir/platforms/sunfish/build-modem-glink-open-probe.sh"
