#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_QRTR_WAIT_PROBE=1
export SUNFISH_QRTR_SERVREG_PROBE=1
export SUNFISH_QRTR_SERVREG_PDR_REPLY=1
export SUNFISH_MODEM_QRTR_WAIT_OUT_DIR="${SUNFISH_MODEM_QRTR_PDR_REPLY_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-qrtr-pdr-reply-probe}"
exec "$root_dir/platforms/sunfish/build-modem-qrtr-wait-probe.sh"
