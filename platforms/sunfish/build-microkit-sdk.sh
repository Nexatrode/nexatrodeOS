#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
microkit_src="$root_dir/.cache/microkit-src-2.3.1-sunfish"
sel4_src="$root_dir/.cache/sel4-src-16.0-sunfish"
sdk="$microkit_src/release/microkit-sdk-2.3.1-sunfish"
tool="$root_dir/.cache/microkit-sdk-2.3.1/bin/microkit"
python="$root_dir/.cache/sel4-venv/bin/python"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
export LDFLAGS="-fuse-ld=lld"

test -d "$microkit_src"
test -d "$sel4_src"
test -x "$python"
test -x "$tool"
"$python" "$root_dir/platforms/sunfish/prepare-microkit-sources.py"
cd "$microkit_src"
"$python" build_sdk.py --sel4 "$sel4_src" --llvm --boards sunfish \
    --configs debug --version 2.3.1-sunfish --skip-tool --skip-docs --skip-tar
cp "$tool" "$sdk/bin/microkit"
echo "Sunfish debug SDK: $sdk"
