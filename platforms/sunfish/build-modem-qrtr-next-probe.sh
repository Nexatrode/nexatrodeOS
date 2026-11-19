#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_QRTR_NEXT_PROBE=1
export SUNFISH_MODEM_QRTR_HELLO_OUT_DIR="${SUNFISH_MODEM_QRTR_NEXT_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-qrtr-next-probe}"
exec "$root_dir/platforms/sunfish/build-modem-qrtr-hello-probe.sh"
