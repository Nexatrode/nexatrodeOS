#!/usr/bin/env bash
# Build both wallet scan PDs against the same Microkit kernel configuration.
set -euo pipefail
if [[ $# -lt 2 ]]; then
    echo "usage: $0 SEL4_INSTALL OUTPUT_DIR [--handoff-prime-m1-100 | --generic | experimental flags: --utm-74 --benchmark --healthy-ms=50|100|250 --profile --utm-m1-tune] [--event-pump]" >&2
    exit 2
fi
export SEL4_PREFIX
SEL4_PREFIX=$(cd "$1" && pwd)
mkdir -p "$2"
output=$(cd "$2" && pwd)
shift 2
features=wallet-scan-workers
healthy_ms=
tune_m1=false
if [[ $# -eq 0 || $1 == --handoff-prime-m1-100 ]]; then
    features+=,wallet-handoff-prime
    if [[ $# -gt 0 ]]; then shift; fi
    set -- --utm-74 --benchmark --healthy-ms=100 --utm-m1-tune "$@"
elif [[ $# -eq 1 && $1 == --generic ]]; then
    shift
fi
for option in "$@"; do
    case "$option" in
        --utm-74) features+=,walletcore-core/utm-range-74,wallet-wide-rx ;;
        --benchmark) features+=,wallet-scan-benchmark ;;
        --healthy-ms=50|--healthy-ms=100|--healthy-ms=250) healthy_ms=${option#*=} ;;
        --profile) features+=,wallet-range-profile ;;
        --event-pump) features+=,wallet-event-pump ;;
        --utm-m1-tune) tune_m1=true ;;
        *) echo "unknown option: $option" >&2; exit 2 ;;
    esac
done
if [[ -n "$healthy_ms" && ",$features," != *,wallet-scan-benchmark,* ]]; then
    echo "--healthy-ms requires --benchmark" >&2
    exit 2
fi
cargo_options=(--locked --release -Zjson-target-spec)
if "$tune_m1"; then
    if [[ ",$features," != *,walletcore-core/utm-range-74,* || ",$features," != *,wallet-scan-benchmark,* ]]; then
        echo "--utm-m1-tune requires --utm-74 and --benchmark" >&2
        exit 2
    fi
    # Tune instruction scheduling for this specific host and package. The
    # target's AArch64 ISA and features continue to come from its JSON file.
    cargo_options+=(-Z profile-rustflags --config 'profile.release.package.curve25519-dalek.rustflags=["-Ztune-cpu=apple-m1"]' -vv)
fi
export NEXAWAL_BENCH_HEALTHY_MS=${healthy_ms:-250}
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
if ! command -v llvm-ar >/dev/null && command -v brew >/dev/null; then
    PATH="$(brew --prefix llvm)/bin:$PATH"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$output/cargo}"
export CC_aarch64_sel4_microkit="${CC_aarch64_sel4_microkit:-clang}"
export AR_aarch64_sel4_microkit="${AR_aarch64_sel4_microkit:-llvm-ar}"
export ARFLAGS_aarch64_sel4_microkit="${ARFLAGS_aarch64_sel4_microkit:---format=gnu}"
export CFLAGS_aarch64_sel4_microkit="--target=aarch64-none-elf -ffreestanding -I$root/experiments/ghc-microkit/freestanding-headers ${CFLAGS_aarch64_sel4_microkit:-}"
cargo +nightly-2026-09-25 build "${cargo_options[@]}" \
    --config rust/.cargo/config.toml --manifest-path rust/Cargo.toml \
    --target rust/targets/aarch64-sel4-microkit.json --features "$features" \
    --bin rust_wallet_app --bin rust_wallet_range_worker
cp "$CARGO_TARGET_DIR/aarch64-sel4-microkit/release/"{rust_wallet_app,rust_wallet_range_worker}.elf "$output/"
