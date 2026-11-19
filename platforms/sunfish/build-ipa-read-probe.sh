#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
kernel_source="$root_dir/.cache/sunfish-lineage-kernel"

python3 "$root_dir/platforms/sunfish/validate-ipa-read-probe-source.py" "$kernel_source"
printf '%s\n' \
    'Refusing to build a bootable IPA MMIO probe: Sunfish requires the IPA SVS2 bandwidth vote before register access, and seL4 has no such provider yet.' >&2
exit 3
