# Sunfish RMTFS Pre-SCM Checkpoints

## Intent

Diagnose the Pixel 4a startup stall after RMTFS buffer preparation and before
the first SCM ownership-assignment call. The last device screen showed all
512 pages zeroed, pool phase `Prepared`, zero assignment attempts, and zero
monitor calls. The next temporary image should identify which pre-call stage
returns or stalls.

## Goals

- Paint and flush named checkpoints for storage-owner validation, pool-owner
  validation, call construction, scratch-buffer staging, post-stage validation,
  and the final state immediately before SCM entry.
- Preserve the observed values and current prepared pool state in the display.
- Stop before invoking the RMTFS ownership-assignment SCM call, changing
  RMTFS ownership, submitting PAS4, releasing
  reset, starting GLINK/QRTR/WLAN, or transferring MPSS/WLAN firmware. Earlier
  IPA firmware startup and its secure-loader calls remain in scope.
- Keep the change behind a dedicated diagnostic feature so the normal runtime
  path remains unchanged.
- Keep the checkpoint display owned by this diagnostic while IPA IRQ handling
  continues, so an IRQ cannot replace the stalled-stage label.
- Build a fresh hash-bound temporary image and manifest; never reuse v6.

## Non-goals

- No retry of v6 or any consumed image.
- No RMTFS ownership-assignment SCM call, RMTFS ownership transfer, memory
  reclamation, flash, or reset retry in this diagnostic. The existing IPA
  startup stages remain part of the image and may make their own secure-loader
  calls before the RMTFS checkpoint.
- No claim that RMTFS sharing, PAS4, modem readiness, GLINK/QRTR, WLAN, or
  network frames work.

## Approaches

1. **Checkpoint-only pre-SCM diagnostic (selected).** Add flushed progress
   reporting across the pre-call validation and staging boundaries, then stop
   before the RMTFS assignment SCM call. This distinguishes a local check or
   staging stall without another ownership transition. It cannot prove that
   SCM would return successfully.
2. **Checkpoint and retry SCM.** Paint the same checkpoints, then call SCM if
   staging succeeds. This could advance startup, but risks repeating an
   ambiguous ownership operation and still obscures the current pre-call
   boundary if the device stalls again.

The checkpoint-only path is selected because the evidence shows no SCM call
was reached, so another ownership attempt is not needed to localize the
current failure.

## Constraints and implementation choices

- Use the existing one-shot cold Fastboot workflow and isolated worktree.
- Make the smallest diagnostic-only change around the existing RMTFS share
  path; preserve the normal runtime implementation and all source-proven
  limits.
- Paint before and after each RMTFS pre-assignment operation that might stall.
  A checkpoint must be flushed before entering the operation, so a stuck
  operation leaves an unambiguous last line on screen.
- Freeze and review the new artifact hash, manifest, and stop boundary before
  requesting one boot. The user must explicitly approve that concrete boot.

## Acceptance

- Offline source/build checks confirm that every RMTFS pre-SCM boundary has a
  distinct flushed checkpoint and every successful path stops before the
  RMTFS assignment SCM call.
- The manifest states the RMTFS assignment SCM/PAS4/reset-release/GLINK/QRTR/
  WLAN/flash/retry calls were not invoked by the diagnostic, while accounting
  for the earlier IPA secure-loader startup calls.
- A device run, if approved, records the last checkpoint reached. No claim of
  network readiness follows from this diagnostic alone.
- IRQ handling may continue, but the diagnostic's last checkpoint remains
  visible after notifications.
