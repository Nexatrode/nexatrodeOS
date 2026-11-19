#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_MODEM_IMAGE_OUT_DIR="${SUNFISH_LOADER_COPY_OUT_DIR:-$root_dir/.cache/pixel-sunfish/loader-copy-hold}"
export SUNFISH_MODEM_IMAGE_BIN=rust_sunfish_modem_image_boot_check
export SUNFISH_IMAGE_LOADER_CHECKPOINTS=1
export SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION=0
export SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION=0
export SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY=1
export SUNFISH_IMAGE_LOADER_COPY_PROGRESS=0
export SUNFISH_IMAGE_LOADER_COPY_INPUTS=0
exec "$root_dir/platforms/sunfish/build-modem-image-probe.sh"
