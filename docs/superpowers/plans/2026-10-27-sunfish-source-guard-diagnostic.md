# Sunfish source-guard diagnostic Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a separately frozen, one-shot Sunfish boot image that shows the exact loader source-guard failure and value, then always halts before relocation or MPSS startup.

**Architecture:** Extend the existing loader-only framebuffer checkpoint with a hexadecimal value display and add distinct branches for each source-guard failure. A diagnostic build also halts after reporting a passing source check, so it cannot continue into the authenticated runtime. Freeze and consume it from a new private directory using the existing no-flash/no-retry runner.

**Tech Stack:** Python 3 build and audit scripts, C framebuffer helper, AArch64 loader assembly, Android boot image v2 packer, existing Fastboot one-shot runner.

**Spec:** `docs/superpowers/specs/2026-10-27-sunfish-source-guard-diagnostic-design.md`

## Global Constraints

- Preserve the accepted source addresses `0xc0000000` and `0xb0081000`.
- Preserve the `0x20000` prefix bound and `0x6000000` total source envelope.
- Keep the source diagnostic before relocation, loader copying, PAS, SMEM, IPA, SMMU, reset, GLINK, and WLAN activity.
- The diagnostic image always halts after showing the source check, even when the check passes.
- Store the image under a new private versioned directory; never reuse the consumed auth image's attempt record.
- Require a fresh user cold-Fastboot confirmation tied to the new image; use temporary `fastboot boot` only, never flash or retry.

## Review Focus

- An unexpected ABL entry must show the actual `_start` value and stop before relocation; cover each source-address branch in the source-guard test.
- An oversized prefix must identify the failing prefix bound; cover it in the source-guard test.
- Arithmetic overflow must be reported distinctly and stop; cover the carry branch in the source-guard test.
- An oversized complete source extent must identify the failing extent; cover it in the source-guard test.
- A passing source check must still stop in diagnostic mode; cover the unconditional diagnostic halt in the source-guard test and actual AArch64 disassembly audit.

---

### Task 1: Render the source-guard result without changing guard policy

**Files:**
- Modify: `platforms/sunfish/loader-boot-checkpoints.h`
- Modify: `platforms/sunfish/prepare-image-loader-sdk.py`
- Modify: `platforms/sunfish/validate-native-mpss-auth-boot.py`
- Create: `platforms/sunfish/test-native-source-guard-diagnostic.py`

**Interfaces:**
- Add `loader_checkpoint_hex(const char *title, uint64_t value)` to render a 64-bit uppercase hexadecimal value on a second screen line.
- Extend `native_startup_entry(text, diagnostic_only=False)` so each failed check shows its specific title/value; with `diagnostic_only=True`, every path displays the check result and stops before `relocate`.

- [ ] **Step 1: Write failing tests** for hexadecimal display, each failed guard branch, unchanged limits/accepted addresses, and the diagnostic-only halt on both pass and fail paths.
- [ ] **Step 2: Run `python3 platforms/sunfish/test-native-source-guard-diagnostic.py` and confirm the new assertions fail for the missing formatter/branch distinctions.**
- [ ] **Step 3: Implement the display helper and generated assembly branches** without changing source acceptance constants.
- [ ] **Step 4: Run the focused test and validate a real generated AArch64 loader.** The validator must confirm every failure title/value branch and that `relocate` is unreachable in diagnostic-only mode.
- [ ] **Step 5: Run the relevant existing boot-layout/source validation** and record its output in the new private build directory.

### Task 2: Freeze a distinct diagnostic image and one-shot receipt

**Files:**
- Modify: `platforms/sunfish/build-native-startup-preflight.sh`
- Modify: `platforms/sunfish/freeze-native-mpss-auth.py`
- Modify: `platforms/sunfish/run-native-mpss-auth.py`
- Modify: `platforms/sunfish/test-native-mpss-auth-runner.py`
- Create output only: `.cache/sunfish-native-wifi/private/native-startup-mpss-auth-source-diagnostic-v1/`

**Interfaces:**
- Add `--native-source-diagnostic` to `prepare-image-loader-sdk.py`, activate it with `SUNFISH_SOURCE_GUARD_DIAGNOSTIC=1`, and emit manifest feature `sunfish-native-startup-mpss-auth-source-diagnostic` with diagnostic-only scope.
- Let the runner accept the exact diagnostic feature while retaining identity, hash, cold-confirmation, atomic-consumption, and no-retry checks.

- [ ] **Step 1: Add runner tests** named `test_source_diagnostic_feature_is_accepted_for_verified_image`, `test_auth_manifest_is_not_misclassified_as_source_diagnostic`, `test_cold_confirmation_binds_source_diagnostic_image_hash`, and `test_consumed_source_diagnostic_cannot_be_retried`.
- [ ] **Step 2: Run `python3 platforms/sunfish/test-native-mpss-auth-runner.py -v` and confirm those cases fail for the missing diagnostic feature handling.**
- [ ] **Step 3: Add the distinct manifest feature/scope and source-diagnostic build flag**; keep the ordinary auth feature and one-shot behavior unchanged.
- [ ] **Step 4: Run the focused runner and source-guard suites.**
- [ ] **Step 5: Build and validate the diagnostic image** using the pinned Sunfish inputs, into the new private directory; run the source-guard, capability, and boot-layout audits and freeze the exact artifacts.
- [ ] **Step 6: Run the runner in offline verification mode** and confirm it reports the new image hash with no device action. Do not execute the runner until the user separately confirms a fresh cold-Fastboot state for this image.

## Execution boundary

The current auth image attempt is consumed. The diagnostic image requires its own cold-Fastboot confirmation. A successful Fastboot command is not a source-address, MPSS-ready, GLINK, WLAN, or network-traffic result.
