#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_QRTR_CREDIT_PROBE=1
export SUNFISH_MODEM_QRTR_DISCOVERY_OUT_DIR="${SUNFISH_MODEM_QRTR_CREDIT_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-qrtr-credit-probe}"
exec "$root_dir/platforms/sunfish/build-modem-qrtr-discovery-probe.sh"
