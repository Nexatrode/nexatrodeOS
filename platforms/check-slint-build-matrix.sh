#!/bin/sh
# Build-check the shared Slint UI in QEMU and both native adapters.
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root_dir"

python3 tools/check-slint-platform-matrix.py

mkdir -p build/slint-platform-matrix
cargo +nightly-2026-09-25 run --quiet --locked \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --features full-suite-ui --bin personal-os-slint-notes-spike -- \
    1080 2340 build/slint-platform-matrix/utm-preview.ppm 2.75 \
    --page=home --exercise-suite --compare-line

cargo +nightly-2026-09-25 run --quiet --locked \
    --manifest-path experiments/slint-notes/Cargo.toml \
    --features full-suite-ui --bin personal-os-slint-notes-spike -- \
    1080 2340 build/slint-platform-matrix/wallet-preview.ppm 2.75 \
    --page=wallet --wallet-tab=1 --exercise-wallet

./dev test-slint-utm --timeout 30

sh platforms/iphone7plus/build-slint-full-suite.sh
SLINT_RESET_OUTER=0 sh platforms/sunfish/build-slint-display.sh

echo "Slint platform matrix build checks passed."
