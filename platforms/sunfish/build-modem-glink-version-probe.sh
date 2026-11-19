#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_GLINK_VERSION_PROBE=1
export SUNFISH_MODEM_SMP2P_PREAUTH_OUT_DIR="${SUNFISH_MODEM_GLINK_VERSION_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-glink-version-probe}"
exec "$root_dir/platforms/sunfish/build-modem-smp2p-preauth-probe.sh"
