#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_MODEM_IMAGE_OUT_DIR="${SUNFISH_MODEM_IMAGE_BOOT_CHECK_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-image-boot-check}"
export SUNFISH_MODEM_IMAGE_BIN=rust_sunfish_modem_image_boot_check
export SUNFISH_IMAGE_LOADER_CHECKPOINTS=1
exec "$root_dir/platforms/sunfish/build-modem-image-probe.sh"
