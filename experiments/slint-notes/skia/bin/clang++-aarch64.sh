#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec /opt/homebrew/opt/llvm/bin/clang++ \
  --target=aarch64-unknown-none-elf \
  -ffreestanding \
  -fno-stack-protector \
  -mstrict-align \
  -stdlib=libc++ \
  -I "$root/sysroot/cxx" \
  -idirafter "$root/sysroot/include" \
  -include "$root/sysroot/include/guest_prelude.h" \
  -include "$root/sysroot/include/guest_locale_extras.h" \
  -D_LIBCPP_DISABLE_AVAILABILITY \
  -DSK_BUILD_FOR_UNIX \
  -D__linux__ \
  -D_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE \
  -D__unix__ \
  "$@" \
  -fvisibility=default \
  -w \
  -Wno-error
