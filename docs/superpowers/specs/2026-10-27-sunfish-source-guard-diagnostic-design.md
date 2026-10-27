# Sunfish loader source-guard diagnostic

## Intent

The frozen MPSS-auth image stopped at `IMAGE SOURCE STOP`. Its source guard
runs before relocation or payload copying, but the current screen does not
show which check failed. The v2 boot image still carries `kernel_addr =
0x8000`; AOSP defines that field as a physical load address, while the actual
entry address used by Sunfish ABL for this unlocked `fastboot boot` path has
not been measured.

## Goals and constraints

- Show the failed guard branch and the relevant 64-bit value on the existing
  diagnostic display.
- Preserve the current strict acceptance rules for source address, prefix
  size, arithmetic overflow, and total image envelope.
- Make the diagnostic image stop before relocation even if the source checks
  pass; the normal authentication image keeps its existing continue behavior.
- Stop before relocation, copy, PAS, SMEM, IPA, SMMU, reset, GLINK, or WLAN
  activity on any failed guard.
- Build a new, separately frozen diagnostic image. The consumed MPSS-auth
  image is never retried.
- Run the image only after a fresh cold-Fastboot confirmation, using temporary
  `fastboot boot`; do not flash or automatically retry.

## Approaches

1. Change the boot-image address fields to match the guard. This is small, but
   assumes Sunfish ABL honors those fields in the unlocked temporary-boot path.
2. Add a diagnostic-only screen that identifies the failing guard and prints
   its actual address or size. This requires one additional temporary boot but
   answers the uncertainty without loosening the guard. **Chosen.**
3. Reverse-engineer the local ABL binary. No ABL image is present in the local
   backup, and this may not reveal the exact runtime placement as directly.

## Design

The loader will use distinct stop labels for unexpected entry address, prefix
bound, overflow, and total source extent. It will display the offending value
in hexadecimal. A source-diagnostic build will also display the entry and
extent when checks pass, then stop unconditionally before `relocate`. The
accepted source addresses and bounds will not change, and the normal auth
build will retain its existing continue behavior. Only framebuffer
diagnostics are added before the diagnostic stop loop.

## Validation and limits

Host checks will cover every guard failure label, hexadecimal display, and
the unconditional diagnostic stop; then the AArch64 loader will be rebuilt
and checked in a new private frozen manifest. The check proves only what the
image contains. A successful Fastboot command does not prove that the loader
advanced, and this diagnostic does not prove MPSS, GLINK, WLAN, or network
traffic. The device result will be the screen photo after the one-shot boot.

## Calls left to the implementer

Use the existing framebuffer and loader build path, keep the new image
separate from consumed runs, and leave all unrelated dirty files untouched.
