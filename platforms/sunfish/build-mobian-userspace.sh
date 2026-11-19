#!/bin/sh
# Build a desktop-free RAM userspace with the reference services installed but held.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cache_dir="$root_dir/.cache/sunfish-mobian-comparison"
stage=all
if [ "${1:-}" = --prepare ]; then stage=prepare; fi
if [ "$stage" = all ] && [ ! -s "$cache_dir/artifacts/SHA256SUMS" ]; then
    echo 'Complete build-mobian-reference.sh first, or use --prepare for userspace only.' >&2
    exit 2
fi
python3 - "$cache_dir/userspace.ext4" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
if not p.exists():
    with p.open('xb') as stream:
        stream.truncate(8 * 1024**3)
PY
if [ ! -f "$cache_dir/comparison-key" ]; then
    ssh-keygen -q -t ed25519 -N '' -C sunfish-comparison \
        -f "$cache_dir/comparison-key"
fi
if [ "$(uname -s)" = Darwin ]; then
    podman machine ssh sudo modprobe loop ext4
fi
podman run --rm --privileged --name sunfish-mobian-userspace-build \
    -e BUILD_STAGE="$stage" \
    -v "$root_dir:/workspace:ro" -v "$cache_dir:/inputs" \
    localhost/sunfish-mobian-toolchain:20261025 \
    sh /workspace/platforms/sunfish/mobian-comparison/build-userspace.sh
