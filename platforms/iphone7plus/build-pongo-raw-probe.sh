#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUT="$ROOT/build/iphone7plus"
SRC="$ROOT/platforms/iphone7plus/pongo_raw_probe.S"
STATE_SRC="$ROOT/platforms/iphone7plus/pongo_state_probe.S"
BOOTARGS_ENTRY="$ROOT/platforms/iphone7plus/pongo_bootargs_entry.S"
BOOTARGS_REPORT="$ROOT/platforms/iphone7plus/pongo_bootargs_report.c"
FREERAM_ENTRY="$ROOT/platforms/iphone7plus/pongo_freeram_entry.S"
FREERAM_REPORT="$ROOT/platforms/iphone7plus/pongo_freeram_report.c"
RAMSWEEP_ENTRY="$ROOT/platforms/iphone7plus/pongo_ramsweep_entry.S"
RAMSWEEP_REPORT="$ROOT/platforms/iphone7plus/pongo_ramsweep_report.c"
DTMAP_ENTRY="$ROOT/platforms/iphone7plus/pongo_dtmap_entry.S"
DTMAP_REPORT="$ROOT/platforms/iphone7plus/pongo_dtmap_report.c"
DTREAD_ENTRY="$ROOT/platforms/iphone7plus/pongo_dtread_entry.S"
DTREAD_REPORT="$ROOT/platforms/iphone7plus/pongo_dtread_report.c"
DTROOT_ENTRY="$ROOT/platforms/iphone7plus/pongo_dtroot_entry.S"
DTROOT_REPORT="$ROOT/platforms/iphone7plus/pongo_dtroot_report.c"
DTCHECK_ENTRY="$ROOT/platforms/iphone7plus/pongo_dtcheck_entry.S"
DTCHECK_REPORT="$ROOT/platforms/iphone7plus/pongo_dtcheck_report.c"
MMU_SRC="$ROOT/platforms/iphone7plus/pongo_mmu_probe.S"
MMU_RELOC_SRC="$ROOT/platforms/iphone7plus/pongo_mmu_relocated_probe.S"
MMU_CALLFRAME_SRC="$ROOT/platforms/iphone7plus/pongo_mmu_callframe_probe.S"
FLAT_LINKER="$ROOT/platforms/iphone7plus/pongo_flat_probe.ld"
UPLOADER="$ROOT/platforms/iphone7plus/load-pongo-raw-probe.c"
LLVM_BIN=${LLVM_BIN:-/opt/homebrew/opt/llvm/bin}
OBJCOPY=$(command -v llvm-objcopy || true)
READELF=$(command -v llvm-readelf || true)

if [ -z "$OBJCOPY" ]; then
    OBJCOPY="$LLVM_BIN/llvm-objcopy"
fi
if [ -z "$READELF" ]; then
    READELF="$LLVM_BIN/llvm-readelf"
fi
if [ ! -x "$OBJCOPY" ] || [ ! -x "$READELF" ]; then
    echo "llvm-objcopy and llvm-readelf are required" >&2
    exit 1
fi

mkdir -p "$OUT"
clang --target=aarch64-none-elf -c "$SRC" -o "$OUT/pongo_raw_probe.o"
ld.lld --image-base=0 -Ttext=0 -e _start -o "$OUT/pongo_raw_probe.elf" \
    "$OUT/pongo_raw_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_raw_probe.elf" "$OUT/pongo_raw_probe.bin"

if "$READELF" -r "$OUT/pongo_raw_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in raw probe" >&2
    exit 1
fi

echo "Built $OUT/pongo_raw_probe.bin"
shasum -a 256 "$OUT/pongo_raw_probe.bin"
wc -c < "$OUT/pongo_raw_probe.bin"

clang --target=aarch64-none-elf -c "$STATE_SRC" -o "$OUT/pongo_state_probe.o"
ld.lld --image-base=0 -Ttext=0 -e _start -o "$OUT/pongo_state_probe.elf" \
    "$OUT/pongo_state_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_state_probe.elf" "$OUT/pongo_state_probe.bin"
if "$READELF" -r "$OUT/pongo_state_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in state probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_state_probe.bin"
shasum -a 256 "$OUT/pongo_state_probe.bin"
wc -c < "$OUT/pongo_state_probe.bin"

clang --target=aarch64-none-elf -c "$BOOTARGS_ENTRY" \
    -o "$OUT/pongo_bootargs_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$BOOTARGS_REPORT" -o "$OUT/pongo_bootargs_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" -o "$OUT/pongo_bootargs_probe.elf" \
    "$OUT/pongo_bootargs_entry.o" "$OUT/pongo_bootargs_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_bootargs_probe.elf" "$OUT/pongo_bootargs_probe.bin"
if "$READELF" -r "$OUT/pongo_bootargs_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in boot_args probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_bootargs_probe.bin"
shasum -a 256 "$OUT/pongo_bootargs_probe.bin"
wc -c < "$OUT/pongo_bootargs_probe.bin"

clang --target=aarch64-none-elf -c "$FREERAM_ENTRY" \
    -o "$OUT/pongo_freeram_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$FREERAM_REPORT" -o "$OUT/pongo_freeram_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" \
    -o "$OUT/pongo_freeram_probe.elf" \
    "$OUT/pongo_freeram_entry.o" "$OUT/pongo_freeram_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_freeram_probe.elf" "$OUT/pongo_freeram_probe.bin"
if "$READELF" -r "$OUT/pongo_freeram_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in free-RAM smoke probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_freeram_probe.bin"
shasum -a 256 "$OUT/pongo_freeram_probe.bin"
wc -c < "$OUT/pongo_freeram_probe.bin"

clang --target=aarch64-none-elf -c "$RAMSWEEP_ENTRY" \
    -o "$OUT/pongo_ramsweep_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$RAMSWEEP_REPORT" -o "$OUT/pongo_ramsweep_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" \
    -o "$OUT/pongo_ramsweep_probe.elf" \
    "$OUT/pongo_ramsweep_entry.o" "$OUT/pongo_ramsweep_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_ramsweep_probe.elf" "$OUT/pongo_ramsweep_probe.bin"
if "$READELF" -r "$OUT/pongo_ramsweep_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in RAM page-sweep probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_ramsweep_probe.bin"
shasum -a 256 "$OUT/pongo_ramsweep_probe.bin"
wc -c < "$OUT/pongo_ramsweep_probe.bin"

clang --target=aarch64-none-elf -c "$DTMAP_ENTRY" \
    -o "$OUT/pongo_dtmap_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$DTMAP_REPORT" -o "$OUT/pongo_dtmap_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" -o "$OUT/pongo_dtmap_probe.elf" \
    "$OUT/pongo_dtmap_entry.o" "$OUT/pongo_dtmap_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_dtmap_probe.elf" "$OUT/pongo_dtmap_probe.bin"
if "$READELF" -r "$OUT/pongo_dtmap_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in Apple device-tree probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_dtmap_probe.bin"
shasum -a 256 "$OUT/pongo_dtmap_probe.bin"
wc -c < "$OUT/pongo_dtmap_probe.bin"

clang --target=aarch64-none-elf -c "$DTREAD_ENTRY" \
    -o "$OUT/pongo_dtread_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$DTREAD_REPORT" -o "$OUT/pongo_dtread_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" -o "$OUT/pongo_dtread_probe.elf" \
    "$OUT/pongo_dtread_entry.o" "$OUT/pongo_dtread_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_dtread_probe.elf" "$OUT/pongo_dtread_probe.bin"
if "$READELF" -r "$OUT/pongo_dtread_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in bounded device-tree read probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_dtread_probe.bin"
shasum -a 256 "$OUT/pongo_dtread_probe.bin"
wc -c < "$OUT/pongo_dtread_probe.bin"

clang --target=aarch64-none-elf -c "$DTROOT_ENTRY" \
    -o "$OUT/pongo_dtroot_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$DTROOT_REPORT" -o "$OUT/pongo_dtroot_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" -o "$OUT/pongo_dtroot_probe.elf" \
    "$OUT/pongo_dtroot_entry.o" "$OUT/pongo_dtroot_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_dtroot_probe.elf" "$OUT/pongo_dtroot_probe.bin"
if "$READELF" -r "$OUT/pongo_dtroot_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in root-property probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_dtroot_probe.bin"
shasum -a 256 "$OUT/pongo_dtroot_probe.bin"
wc -c < "$OUT/pongo_dtroot_probe.bin"

clang --target=aarch64-none-elf -c "$DTCHECK_ENTRY" \
    -o "$OUT/pongo_dtcheck_entry.o"
clang --target=aarch64-none-elf -std=c11 -O2 -ffreestanding -fno-builtin \
    -fno-stack-protector -fno-pic -fno-pie -fno-jump-tables \
    -fno-unwind-tables -fno-asynchronous-unwind-tables \
    -c "$DTCHECK_REPORT" -o "$OUT/pongo_dtcheck_report.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" -o "$OUT/pongo_dtcheck_probe.elf" \
    "$OUT/pongo_dtcheck_entry.o" "$OUT/pongo_dtcheck_report.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_dtcheck_probe.elf" "$OUT/pongo_dtcheck_probe.bin"
if "$READELF" -r "$OUT/pongo_dtcheck_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in Pongo-style tree validator" >&2
    exit 1
fi
echo "Built $OUT/pongo_dtcheck_probe.bin"
shasum -a 256 "$OUT/pongo_dtcheck_probe.bin"
wc -c < "$OUT/pongo_dtcheck_probe.bin"

clang --target=aarch64-none-elf -c "$MMU_SRC" -o "$OUT/pongo_mmu_probe.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" -o "$OUT/pongo_mmu_probe.elf" \
    "$OUT/pongo_mmu_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_mmu_probe.elf" "$OUT/pongo_mmu_probe.bin"
if "$READELF" -r "$OUT/pongo_mmu_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in standalone MMU probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_mmu_probe.bin"
shasum -a 256 "$OUT/pongo_mmu_probe.bin"
wc -c < "$OUT/pongo_mmu_probe.bin"

clang --target=aarch64-none-elf -c "$MMU_RELOC_SRC" \
    -o "$OUT/pongo_mmu_relocated_probe.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" \
    -o "$OUT/pongo_mmu_relocated_probe.elf" \
    "$OUT/pongo_mmu_relocated_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_mmu_relocated_probe.elf" \
    "$OUT/pongo_mmu_relocated_probe.bin"
if "$READELF" -r "$OUT/pongo_mmu_relocated_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in relocated standalone MMU probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_mmu_relocated_probe.bin"
shasum -a 256 "$OUT/pongo_mmu_relocated_probe.bin"
wc -c < "$OUT/pongo_mmu_relocated_probe.bin"

clang --target=aarch64-none-elf -c "$MMU_CALLFRAME_SRC" \
    -o "$OUT/pongo_mmu_callframe_probe.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" \
    -o "$OUT/pongo_mmu_callframe_probe.elf" \
    "$OUT/pongo_mmu_callframe_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_mmu_callframe_probe.elf" \
    "$OUT/pongo_mmu_callframe_probe.bin"
if "$READELF" -r "$OUT/pongo_mmu_callframe_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in stack/call-frame MMU probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_mmu_callframe_probe.bin"
shasum -a 256 "$OUT/pongo_mmu_callframe_probe.bin"
wc -c < "$OUT/pongo_mmu_callframe_probe.bin"

clang --target=aarch64-none-elf -DEXACT_NESTED_DISABLE -DMATCH_LOADER_TABLE_LAYOUT \
    -DMATCH_LOADER_POST_ENABLE_MARKER \
    -c "$MMU_CALLFRAME_SRC" \
    -o "$OUT/pongo_mmu_nested_probe.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" \
    -o "$OUT/pongo_mmu_nested_probe.elf" \
    "$OUT/pongo_mmu_nested_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_mmu_nested_probe.elf" \
    "$OUT/pongo_mmu_nested_probe.bin"
if "$READELF" -r "$OUT/pongo_mmu_nested_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in nested-call MMU probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_mmu_nested_probe.bin"
shasum -a 256 "$OUT/pongo_mmu_nested_probe.bin"
wc -c < "$OUT/pongo_mmu_nested_probe.bin"

clang --target=aarch64-none-elf -DEXACT_NESTED_DISABLE -DMATCH_LOADER_TABLE_LAYOUT \
    -DFORCE_VISIBLE_POSTSTORE -c "$MMU_CALLFRAME_SRC" \
    -o "$OUT/pongo_mmu_poststore_probe.o"
ld.lld --image-base=0 -T "$FLAT_LINKER" \
    -o "$OUT/pongo_mmu_poststore_probe.elf" \
    "$OUT/pongo_mmu_poststore_probe.o"
"$OBJCOPY" --only-section=.text -O binary \
    "$OUT/pongo_mmu_poststore_probe.elf" \
    "$OUT/pongo_mmu_poststore_probe.bin"
if "$READELF" -r "$OUT/pongo_mmu_poststore_probe.elf" | grep -q 'R_AARCH64'; then
    echo "unexpected relocations in post-MMU store probe" >&2
    exit 1
fi
echo "Built $OUT/pongo_mmu_poststore_probe.bin"
shasum -a 256 "$OUT/pongo_mmu_poststore_probe.bin"
wc -c < "$OUT/pongo_mmu_poststore_probe.bin"

if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libusb-1.0; then
    # pkg-config emits compiler/linker flags; intentional word splitting.
    # shellcheck disable=SC2046
    cc -Wall -Wextra -Werror $(pkg-config --cflags libusb-1.0) \
        "$UPLOADER" -o "$OUT/load-pongo-raw-probe" \
        $(pkg-config --libs libusb-1.0)
    echo "Built $OUT/load-pongo-raw-probe"
else
    echo "libusb-1.0 development files not found; uploader not built" >&2
fi
