# Sunfish MPSS source-address correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update the native MPSS-auth image to accept the measured Fastboot loader entry at `0xB1281000`, while preserving fail-closed source-size checks before relocation.

**Architecture:** Change the single measured runtime-source constant and update its generated-image audits. Build and freeze a distinct MPSS-auth v2 bundle; its existing runtime checks validate the prefix, addition overflow, and total extent before relocation, then the existing PAS4/Q6-ready sequence runs only if those checks pass.

**Tech Stack:** Python 3 image build/audit scripts, AArch64 loader assembly, Microkit SDK, Rust/seL4 image, Fastboot one-shot runner.

**Spec:** `docs/superpowers/specs/2026-10-27-sunfish-mpss-source-address-design.md`

## Global Constraints

- Accept only link address `0xC0000000` and measured runtime `_start` `0xB1281000`.
- Keep prefix bound `0x20000` and complete source limit `0x6000000` unchanged.
- Check runtime source and bounds before relocation, PAS, or modem-side activity.
- Preserve frozen auth v1 and consumed v6 diagnostic; create a fresh v2 output and one-shot record.
- Use temporary `fastboot boot` only; require fresh cold-Fastboot confirmation bound to the new hash; never flash or retry.
- This image starts MPSS only; it does not start GLINK/WLAN or prove network traffic.

## Review Focus

- Stale runtime address `0xB0081000` must still stop before relocation; test its rejection in Task 1.
- A source-size sum carry must stop before relocation; preserve the overflow branch and its test in Task 1.
- Prefix greater than `0x20000` must stop; preserve the prefix branch and its test in Task 1.
- Complete source extent greater than `0x6000000` must stop; preserve the extent branch and its test in Task 1.
- Another ABL placement must not be accepted; test a neighboring unexpected address in Task 1 and retain the runtime exact-match guard.
- The device image must be the newly frozen v2 hash; Task 2 offline runner verification and Task 3 hash-bound confirmation cover this.

---

### Task 1: Bind and audit the measured runtime source

**Files:**
- Modify: `platforms/sunfish/prepare-image-loader-sdk.py`
- Modify: `platforms/sunfish/validate-native-mpss-auth-boot.py`
- Modify: `platforms/sunfish/audit-native-mpss-runtime-layout.py`
- Test: `platforms/sunfish/test-native-source-guard-diagnostic.py`

**Interfaces:**
- Keep `native_startup_entry(text, diagnostic_only=False)` unchanged.
- Set `RUNTIME_SOURCE` to `0xB1281000`; generate both normal-auth and diagnostic guards from it; report `source_image_base` as `0xB1280000`.

- [x] **Step 1: Write the failing source-contract tests.** Require the generated normal and diagnostic guards to compare against `0xb1281000`, reject the old `0xb0081000` and a neighboring address, retain the prefix/overflow/extent limits before relocation, and require `audit-native-mpss-runtime-layout.py` to derive its source base from `prepare.RUNTIME_SOURCE`.
- [x] **Step 2: Run `python3 platforms/sunfish/test-native-source-guard-diagnostic.py -v` and confirm the new address assertions fail against the old constant.**
- [x] **Step 3: Change `RUNTIME_SOURCE` to `0xb1281000` and make both branches of `native_startup_entry()` emit that same exact value; update loader-profile and boot-layout validation to require runtime source `0xb1281000` and source base `0xb1280000`. Do not change link address or bounds.**
- [x] **Step 4: Update `audit-native-mpss-runtime-layout.py` to derive its source base and runtime guard value from the same `prepare.RUNTIME_SOURCE` constant.** Preserve its source-envelope, relocation-disjointness, and three-DTB reservation checks; update the rejected-copy mutation to target the measured source.
- [x] **Step 5: Run `python3 platforms/sunfish/test-native-source-guard-diagnostic.py -v`.** Require all guard-value, runtime-layout source-base, and unchanged-bound cases to pass; Task 2's full image audit validates the generated normal-auth AArch64 branch.
- [x] **Step 6: Commit only the four changed source/test files.**

### Task 2: Build and freeze a new MPSS-auth image

**Files:**
- Modify only if required: `platforms/sunfish/build-native-startup-preflight.sh`, `platforms/sunfish/freeze-native-mpss-auth.py`, `platforms/sunfish/run-native-mpss-auth.py`
- Create output only: `.cache/sunfish-native-wifi/private/native-startup-mpss-auth-v2/`

**Interfaces:**
- Use existing `mpss-auth` build and freeze interfaces with a fresh `SUNFISH_NATIVE_OUTPUT_DIR` and freeze `--output` path.
- Keep runner feature `sunfish-native-startup-mpss-auth`; do not treat the source-diagnostic feature as auth.

- [x] **Step 1: Build `mpss-auth` into the fresh v2 directory using the pinned private inputs.** Use absolute paths for these two environment variables because the build exports them into Cargo build-script paths. From the repository root, run:

```sh
repo_root=$(pwd)
SUNFISH_PRIVATE_INPUT_DIR="$repo_root/.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025" \
SUNFISH_NATIVE_OUTPUT_DIR="$repo_root/.cache/sunfish-native-wifi/private/native-startup-mpss-auth-v2" \
  sh platforms/sunfish/build-native-startup-preflight.sh mpss-auth
```

- [x] **Step 2: Run the existing capability, source, boot-layout, CapDL, and compiler-input audits.** Confirm `0xB1281000` is the only runtime candidate, source and destination envelopes avoid reserved ranges, and the AArch64 check branches precede relocation.
- [x] **Step 3: Freeze v2.** Run:

```sh
python3 platforms/sunfish/freeze-native-mpss-auth.py \
  --output .cache/sunfish-native-wifi/private/native-startup-mpss-auth-v2
```

Confirm a new manifest binds the exact image, source and proof inputs; do not overwrite v1 or v6.
- [x] **Step 4: Run the offline gate.** Run:

```sh
python3 platforms/sunfish/run-native-mpss-auth.py \
  --image-dir .cache/sunfish-native-wifi/private/native-startup-mpss-auth-v2
```

Require the exact v2 hash to verify without a device action or attempt record.
- [x] **Step 5: Commit only the changed source, tests, design, and plan files; keep `.cache` artifacts private and untracked.**

### Task 3: One-shot cold-device MPSS-auth check

**Files:**
- Create: `<v2-dir>/cold-confirmation.json` from the user's fresh confirmation.
- Create: `<v2-dir>/attempt.json` and run artifacts through the existing runner.

- [ ] **Step 1: Ask for a fresh confirmation that the Pixel is fully powered off for at least 10 seconds, then in Fastboot with USB connected.** Do not reuse the v6 confirmation.
- [ ] **Step 2: Run `run-native-mpss-auth.py --image-dir <v2-dir> --execute --cold-confirmation <v2-dir>/cold-confirmation.json` exactly once.** The runner must verify product `sunfish`, device serial, unlocked state, and image hash before `fastboot boot`; never use `flash` or retry.
- [ ] **Step 3: Ask for the settled screen's PAS, source-guard, SMP2P/watchdog, and Q6-ready fields.** Record only the observed milestone; return the phone to LineageOS after capturing it.
- [ ] **Step 4: Retain the exact attempt transcript and user-reported screen result in the private device-run directory.** Do not put private input bytes or an unredacted photo in the repository.

## Completion Evidence

Task 1 is complete when the exact address and unchanged bounds pass source tests and compiled-loader audits. Task 2 is complete when a fresh v2 image freezes and verifies offline. Task 3 establishes only the observed MPSS/PAS/Q6 result; it does not complete native Wi-Fi. GLINK/QMI, WLAN firmware, CE/HTC/WMI, packet TX/RX, scanning, association, and sustained traffic remain later requirements.
