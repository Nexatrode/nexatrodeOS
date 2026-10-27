# Sunfish MPSS source-address correction

## Intent

The v6 source-guard diagnostic ran on the Pixel and reported `_start` at
`0xB1281000`. The current MPSS-auth loader accepts `0xC0000000` or the stale
expected runtime address `0xB0081000`, so it stops before relocation. Update the
native MPSS-auth path to recognize the measured Fastboot placement and continue
only after its existing prefix, overflow, and full-source-size checks pass.

## Goals

- Accept exactly the linked address `0xC0000000` and measured runtime address
  `0xB1281000`.
- Preserve the `0x20000` loader-prefix and `0x6000000` total-source limits.
- Keep every source check before relocation, PAS calls, or any modem-side
  activity; any unexpected address or invalid bound must stop the image.
- Keep the standalone MPSS runtime-layout audit consistent with the measured
  auth-loader source base.
- Build a new, separately frozen MPSS-auth image and one-shot run record. Never
  modify or retry the consumed auth image or v6 diagnostic.
- Verify the actual Sunfish product and bind a fresh cold-Fastboot confirmation
  to the new image before a temporary `fastboot boot`.

## Non-goals and anti-goals

- Do not start WLAN, GLINK, or Wi-Fi traffic in this image.
- Do not flash partitions, broaden the source guard to arbitrary addresses,
  weaken any size check, or add retry behavior.
- Do not treat a successful Fastboot command or Q6-ready result as proof of
  WLAN readiness or network frames.

## Approaches

1. Keep the current guard and build another bounds-only diagnostic. This avoids
   modem startup, but the matching frozen auth image already contains the same
   loader prefix and data extent, and its runtime guard checks those bounds
   before relocation.
2. Add the exact measured `0xB1281000` address to the MPSS-auth guard and run a
   new authenticated-start image. **Chosen.** The runtime still fails closed
   before relocation if ABL chooses another address or if any size bound fails;
   when all checks pass it continues through the already-audited PAS4/Q6-ready
   path. This avoids an extra diagnostic boot while exercising the next real
   startup milestone.
3. Accept any source address or alter boot-image address fields. Rejected
   because that would remove the source-placement guarantee without evidence
   that ABL honors those fields in this Fastboot path.

## Evidence and constraints

The v6 screen reported `SOURCE ADDRESS 00000000B1281000`. Offline inspection of
both v6 and the frozen MPSS-auth v1 loader found `_start=0xC0000000`,
`_bss_end=0xC000E010`, prefix `0xE010`, loader-data size `0x51849BC`, and full
source extent `0x51929CC`. These are within the current limits. The v6 device
path stopped at the address comparison, so its runtime size checks were not
executed; the new auth image must retain and audit those checks before
relocation.

The new image goes in a fresh private output directory, separate from the
consumed `native-startup-mpss-auth-v1` and
`native-startup-mpss-auth-source-diagnostic-v6` bundles. The Pixel should be
returned to LineageOS while the image is built. A new cold-Fastboot
confirmation is required for the new image hash. The only device command is a
single temporary `fastboot boot`; no automatic retry or partition flash.

The older `loader-copy-*` experiments and their B008 address fixtures describe
separate image profiles and remain unchanged; do not replace every textual
occurrence of the old address in historical evidence.

## Validation

Test the measured address as accepted and an unmeasured address as rejected;
retain regressions for prefix overflow and total-source overflow. Validate the
generated AArch64 guard and prove the bound branches precede `relocate`; test
that the standalone runtime-layout audit derives its source base from the same
measured constant. Re-run
the boot-layout, capability, source-input, and one-shot runner audits; freeze
the new image and verify it offline before requesting the fresh cold-Fastboot
state. Record the device result without claiming modem health beyond the
measured PAS/Q6-ready fields.
