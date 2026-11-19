#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export SUNFISH_LOADER_COPY_PROGRESS_OUT_DIR="${SUNFISH_LOADER_COPY_NATIVE_OUT_DIR:-$root_dir/.cache/pixel-sunfish/loader-copy-native}"
exec "$root_dir/platforms/sunfish/build-loader-copy-progress.sh"
