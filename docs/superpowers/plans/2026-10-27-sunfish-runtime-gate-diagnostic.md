# Sunfish MPSS Runtime Gate Diagnostic Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a temporary diagnostic image that identifies the exact MPSS auth-entry or runtime-profile gate that blocked the prior runtime image, then stops before handoff and PAS4.

**Architecture:** Add an immutable factory snapshot for checks currently hidden inside `take_runtime_boot_parts`, then render the auth health predicates and runtime-profile requirements on the existing display. A dedicated Cargo feature makes this diagnostic stop before handoff; an auth-attempt marker gives the diagnostic screen display priority over the earlier preauth painter.

**Tech Stack:** Rust 2024, seL4 Microkit, existing Sunfish MPSS/RMTFS providers, Cargo feature-gated image build.

**Spec:** `docs/superpowers/specs/2026-10-27-sunfish-runtime-gate-diagnostic-design.md`

## Global Constraints

- The diagnostic image stops before `take_runtime_boot_parts` and PAS4, even when every reported gate passes.
- The normal `sunfish-native-startup-mpss-runtime` behavior remains unchanged.
- The diagnostic auth-entry path performs no owner consumption/quarantine, reset release, GLINK/QRTR start, WLAN start, flash, or retry; existing preauth provider and notification handling remains in place.
- IRQ handling may continue servicing IPA state, but cannot repaint the preauth screen after auth diagnostics claim the display.
- No test suites are added or invoked; the deliverable includes a static source audit and an offline image build.
- A device boot is a separate action and requires review of the frozen artifact and fresh authorization.

## Review Focus

- A failed IPA/domain/preauth/RMTFS predicate must be shown separately, not collapsed into a generic health failure.
- Missing Factory/session data must render as unavailable and fail closed.
- Each runtime-profile mismatch must remain distinguishable, including TFTP port/provider/prepared state.
- An IRQ after auth entry must not replace the gate screen with the static preauth screen.
- A fully passing gate report must still stop before the runtime handoff and PAS4.

---

### Task 1: Expose the runtime handoff preconditions without consuming owners

**Files:**
- Modify: `rust/src/pixel_linux_rmtfs.rs`
- Modify: `rust/src/qcom_glink_qmi.rs`

**Interfaces:**
- Add `Factory::runtime_boot_report(&self) -> RuntimeBootReport`.
- Add a `Copy` factory report with `phase`, `server_present`, `server_failed`, `session_present`, and `tftp_provider_present`.
- Add a separate runtime-profile read path that flushes `PROFILE READ START`, then reports each field immediately before and after its getter runs.
- The method reads state only; it must not call `fault`, move an `Option`, or change `Factory::phase`.

- [x] Add the report type beside `Factory` and implement the snapshot by reusing the current checks in `take_runtime_boot_parts`.
- [x] Keep `take_runtime_boot_parts` as the normal runtime handoff path; do not change its acceptance predicates or failure behavior.
- [x] Review the report's unavailable representation so absent session/profile data cannot be mistaken for a passing profile.

### Task 2: Paint both gate reports and preserve display ownership

**Files:**
- Modify: `rust/src/pixel_mpss_auth.rs`
- Modify: `rust/src/sunfish_native_startup_preflight.rs`
- Modify: `rust/src/qcom_glink_qmi.rs`
- Modify: `rust/src/pixel_ipa_receive.rs`
- Modify: `rust/src/pixel_ipa_gsi.rs`
- Modify: `rust/src/pixel_ipa_command.rs`
- Modify: `rust/src/pixel_ipa_transfer.rs`
- Modify: `rust/src/pixel_ipa_tables.rs`
- Modify: `rust/src/pixel_smmu_install.rs`
- Modify: `rust/Cargo.toml`

**Interfaces:**
- Add feature `sunfish-native-startup-mpss-runtime-gate-diagnostic`, depending on `sunfish-native-startup-mpss-runtime`.
- Add `pixel_mpss_auth::diagnostics_owned() -> bool`.
- The diagnostic screen reports the four auth health predicates and every field in `RuntimeBootReport` / `RuntimeProfileReport` using explicit pass/fail/unavailable values, with a flush before each health read, before the profile read, and after each returned profile requirement.

- [x] Record an auth-start-attempt marker before the first health read; flush a named checkpoint before each of preauth verification, IPA receive health, retained domain quiet state, and retained RMTFS generation, then show every result and stop if any fails.
- [x] Flush `PROFILE READ START`, then flush a named checkpoint before each profile field read and its value immediately afterward; if all gates pass, show that handoff would pass and stop before `take_runtime_boot_parts` or PAS4.
- [x] In the diagnostic feature, return before the existing owner handoff or PAS4 path regardless of whether the report passes.
- [x] In the normal runtime feature, leave the existing gate and handoff behavior unchanged.
- [x] Update IRQ display routing so preauth, IPA, and SMMU/domain fault painters are skipped after the diagnostic attempt marker is set and the current checkpoint remains visible; continue routing notification work to its current owners and preserve ordinary runtime display behavior.

### Task 3: Add a private build mode for the diagnostic artifact

**Files:**
- Create: `platforms/sunfish/audit-native-mpss-runtime-gate-diagnostic.py`
- Create: `platforms/sunfish/build-native-mpss-runtime-gate-diagnostic.py`
- Reuse: `platforms/sunfish/native-startup-mpss-auth.system`

**Interfaces:**
- The builder selects the diagnostic Cargo feature and auth system profile, which has the capabilities needed for preauth providers without the later GLINK IRQ capability.
- Emit the image and manifest under a new private output directory; the manifest must state `PAS4 attempted: false`, `GLINK started: false`, `WLAN started: false`, and `flash/retry: false`.
- The diagnostic feature excludes MPSS image bytes, so the builder packages no `modem.mbn` input.
- Keep the output separate from the consumed `native-mpss-runtime-review` artifact.
- The builder may compile and package the image and run the static source/layout audit, but must not invoke test suites.

- [x] Add a static audit that requires the diagnostic feature to stop before `take_runtime_boot_parts`, PAS submission, reset release, GLINK/QRTR, and WLAN start; also check the runtime feature's ordinary path remains selected when the diagnostic feature is absent.
- [x] Add a standalone builder that validates input identities, prepares the loader SDK, builds the diagnostic feature, packages it with the auth system profile, and records the exact commands and inputs without invoking tests.
- [x] Emit a fresh hash-bound temporary image and manifest without reusing or overwriting the prior runtime artifact.
- [x] Review the generated manifest and image metadata; do not boot the Pixel as part of this plan.

## Completion Criteria

- The diagnostic screen identifies each failed auth and runtime-profile predicate, including unavailable data.
- The diagnostic feature stops before consuming owners or entering PAS4, even when all gates pass.
- An IRQ after auth entry cannot replace the diagnostic with the preauth painter.
- The normal runtime feature path is unchanged, and the separate image is hash-bound and clearly labeled as not booted.
