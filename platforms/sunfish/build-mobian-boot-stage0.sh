#!/bin/sh
# Build a temporary ABL -> U-Boot -> RAM ESP -> EFI/Linux transport candidate.
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cache_dir="$root_dir/.cache/sunfish-mobian-comparison"
test -s "$cache_dir/artifacts/comparison-stage0.cpio.gz"
python3 - "$cache_dir" <<'PY'
from pathlib import Path
import hashlib, sys, urllib.request
p = Path(sys.argv[1])/'downloads/u-boot-f8c04469.tar.gz'
if not p.exists():
    url = 'https://codeload.github.com/sm7150-mainline/u-boot/tar.gz/f8c04469e8284aa2f1691a30aa3b7dc59fa0b8cd'
    temporary = p.with_suffix('.part')
    with urllib.request.urlopen(url, timeout=90) as src, temporary.open('wb') as dst:
        while block := src.read(1024*1024):
            dst.write(block)
    temporary.rename(p)
with p.open('rb') as stream:
    if hashlib.file_digest(stream, 'sha256').hexdigest() != '8ae281f417f9caaaff8d10987241012dcde3e3189e3514408a6be2538e8a1a15':
        raise SystemExit('Pinned U-Boot source archive hash differs')
PY
if [ "$(uname -s)" = Darwin ]; then podman machine ssh sudo modprobe loop ext4; fi
podman run --rm --privileged --name sunfish-mobian-boot-build \
    -v "$root_dir:/workspace:ro" -v "$cache_dir:/inputs" \
    localhost/sunfish-mobian-toolchain:20261025 \
    sh /workspace/platforms/sunfish/mobian-comparison/build-boot.sh
python3 "$root_dir/platforms/sunfish/pack-mobian-stage0.py"
