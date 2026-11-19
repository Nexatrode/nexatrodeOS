# Sunfish RMTFS Pre-SCM Checkpoints Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Build a temporary diagnostic image that reports the last RMTFS share-preflight stage reached and always stops before RMTFS ownership-assignment SCM.

**Architecture:** Add a read-only/checkpointed preflight path through the existing storage-owner and pool-owner validation and scratch staging. A dedicated Cargo feature paints each boundary and exits before the existing ownership assignment call; the standard runtime path remains unchanged.

**Tech Stack:** Rust 2024, seL4 Microkit, existing Sunfish RMTFS provider, Cargo feature-gated image build.

**Spec:** `docs/superpowers/specs/2026-10-28-sunfish-rmtfs-pre-scm-checkpoints-design.md`

## Global Constraints

- The diagnostic stops before invoking the RMTFS ownership-assignment SCM call, changing RMTFS ownership, submitting PAS4, releasing reset, starting GLINK/QRTR/WLAN, or transferring MPSS/WLAN firmware. Earlier IPA firmware startup and secure-loader calls remain in scope.
- No retry of v6 or any consumed image.
- Keep the change behind a dedicated diagnostic feature so the normal runtime path remains unchanged.
- Freeze and review the new artifact hash, manifest, and stop boundary before requesting one boot.
- No test suites are added or invoked; verification consists of static source/layout audits and a fresh offline image build.

## Review Focus

- A storage-owner validation stall leaves its own flushed checkpoint visible.
- A pool-owner check or call-construction stall cannot be mistaken for scratch staging or RMTFS SCM.
- A scratch-buffer write/read stall leaves the pre-stage checkpoint visible; successful staging paints a distinct completion line.
- The diagnostic's successful path always stops before the RMTFS `Native::invoke`/SCM assignment call, including if all checkpoints pass.
- IPA IRQ handling cannot repaint over the RMTFS diagnostic's last checkpoint.
- The standard runtime share path and its one-shot ownership semantics remain unchanged when the diagnostic feature is absent.

---

### Task 1: Add a checkpointed RMTFS share preflight

**Files:**
- Modify: `rust/src/sunfish_linux_rmtfs.rs`
- Modify: `rust/src/pixel_linux_rmtfs.rs`
- Modify: `rust/src/sunfish_native_startup_preflight.rs`
- Modify: `rust/Cargo.toml`

**Interfaces:**
- Add a `ShareCheckpoint` enum for storage validation, pool validation, call construction, scratch staging, post-stage validation, and ready-before-SCM.
- Add a separate `Factory::preflight_share_with` path that emits those checkpoints and returns without calling `Pool::share`'s RMTFS assignment SCM invoke path.
- Preserve `Factory::step` and ordinary `Resources::share` / `Pool::share` behavior outside the diagnostic feature.
- The renderer must flush before each potentially blocking operation and stop after the ready-before-SCM checkpoint.

- [x] Add Cargo feature `sunfish-native-startup-mpss-runtime-prescm-diagnostic` with dependencies limited to the already validated RMTFS/IPA foundations.
- [x] Implement a diagnostic-only preflight operation through the storage wrapper and pool that validates lifecycle, constructs and stages the exact SCM call bytes in scratch, runs the final owner check, and stops before the invoke call.
- [x] Render each checkpoint and current zeroed-page/phase/attempt/monitor values on the Pixel display; report a local error distinctly and never proceed to PAS4.
- [x] Verify the ordinary `share()` path remains unchanged when the diagnostic feature is absent.

### Task 2: Audit, build, and freeze the one-shot diagnostic image

**Files:**
- Create: `platforms/sunfish/audit-native-mpss-runtime-prescm-diagnostic.py`
- Create: `platforms/sunfish/build-native-mpss-runtime-prescm-diagnostic.py`
- Modify: `platforms/sunfish/build-native-startup-preflight.sh`
- Reuse: the existing Sunfish startup system profile and private image-freezer pattern.

**Interfaces:**
- The source audit requires all named checkpoints, a diagnostic stop before RMTFS assignment SCM invoke, and unchanged normal runtime selection.
- The builder selects only the diagnostic feature, emits a fresh private image and manifest, and never boots the device. It can skip the existing shell builder's broad regression suite for this plan while retaining the target build, loader packaging, and CapDL validation.
- The manifest states that RMTFS assignment SCM/PAS4/reset-release/GLINK/QRTR/WLAN/MPSS/WLAN firmware transfer/flash/retry were not invoked by the diagnostic; it accounts for earlier IPA firmware startup and secure-loader calls.

- [x] Add the static source/layout audit and fail closed if the diagnostic path can reach `Native::invoke` or an auth/reset/transport/WLAN start.
- [x] Add a dedicated builder that validates input identities, builds the feature-gated image, packages it under a new private directory, and records exact commands and inputs.
- [x] Review the generated manifest, image hash, and bounds; confirm the new image does not overwrite or reuse v6.
- [x] Do not boot the Pixel as part of this plan. Present the frozen image hash and scope for a separate one-shot approval.

## Completion Criteria

- Every pre-SCM validation/staging boundary has a distinct flushed display checkpoint.
- All successful diagnostic paths stop before the RMTFS ownership-assignment SCM invocation.
- The ordinary runtime ownership path is unchanged when the diagnostic feature is absent.
- Static audits and the offline image build pass, and the new image is hash-bound and explicitly unbooted.
