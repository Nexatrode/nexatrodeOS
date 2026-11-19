#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_QRTR_HELLO_PROBE=1
export SUNFISH_MODEM_GLINK_DONE_OUT_DIR="${SUNFISH_MODEM_QRTR_HELLO_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-qrtr-hello-probe}"
exec "$root_dir/platforms/sunfish/build-modem-glink-done-probe.sh"
