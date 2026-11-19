#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
source_dir="$root_dir/.cache/sunfish-linux-mainline-ref"
output_dir="$root_dir/.cache/sunfish-linux-build"
expected_commit=fd78d17954a86ea6ceb0c535e618ea129e09f8dd

command -v podman >/dev/null 2>&1 || {
    printf '%s\n' 'podman is required; start its machine before running this build.' >&2
    exit 2
}
git -C "$source_dir" rev-parse --git-dir >/dev/null 2>&1 || {
    printf 'Pinned Linux source tree is missing: %s\n' "$source_dir" >&2
    exit 2
}

actual_commit=$(git -C "$source_dir" rev-parse HEAD)
test "$actual_commit" = "$expected_commit" || {
    printf 'Expected Linux source %s, found %s.\n' "$expected_commit" "$actual_commit" >&2
    exit 2
}

for relative in \
    arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts \
    drivers/net/wireless/ath/ath10k/snoc.c \
    drivers/net/wireless/ath/ath10k/core.c
do
    git -C "$source_dir" diff --quiet HEAD -- "$relative" || {
        printf 'Refusing to build locally modified audited source: %s\n' "$relative" >&2
        exit 2
    }
done

mkdir -p "$output_dir"
cat > "$output_dir/build-ath10k-reference-objects.sh" <<'IN_CONTAINER'
#!/bin/sh
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    bc bison build-essential dwarves flex gcc-aarch64-linux-gnu \
    binutils-aarch64-linux-gnu libelf-dev libssl-dev lz4 make perl python3
cd /src
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- O=/build/linux-out olddefconfig
make -j4 ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- \
    O=/build/linux-out qcom/sm7150-google-sunfish.dtb
make -j4 ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- \
    O=/build/linux-out drivers/net/wireless/ath/ath10k/ath10k_snoc.o
IN_CONTAINER

podman run --rm --name sunfish-ath10k-reference-objects \
    -v "$source_dir:/src:ro" \
    -v "$output_dir:/build" \
    debian:bookworm sh /build/build-ath10k-reference-objects.sh

dtb="$output_dir/linux-out/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dtb"
snoc_object="$output_dir/linux-out/drivers/net/wireless/ath/ath10k/ath10k_snoc.o"
test -s "$dtb"
test -s "$snoc_object"
file "$dtb" "$snoc_object"
shasum -a 256 "$dtb" "$snoc_object"
printf '%s\n' 'PASS: pinned Sunfish DTB and ath10k SNOC relocatable object built.'
printf '%s\n' 'LIMIT: this does not link a loadable .ko, build a kernel Image, inspect firmware bytes, or test radio compatibility.'
