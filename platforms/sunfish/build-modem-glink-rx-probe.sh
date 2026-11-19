#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_GLINK_RX_PACKET_PROBE=1
export SUNFISH_MODEM_GLINK_INTENTS_V2_OUT_DIR="${SUNFISH_MODEM_GLINK_RX_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-rx-probe}"
exec "$root_dir/platforms/sunfish/build-modem-glink-intents-v2-probe.sh"
