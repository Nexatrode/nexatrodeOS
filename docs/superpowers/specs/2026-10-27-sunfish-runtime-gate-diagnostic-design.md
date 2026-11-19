# Sunfish MPSS Runtime Gate Diagnostic

## Intent

Identify why the post-auth runtime image remained on the pre-auth screen. The
`PREAUTH PROVIDERS PASS` footer is fixed display text, so it is not evidence
that the auth handoff or reset path ran.

## Goals

- Show each auth-entry health condition independently: IPA receive health,
  retained domain quiet state, preauth verification, and retained RMTFS
  generation.
- Set the auth-attempt marker before reading any health condition, and flush a
  named checkpoint before each health read so a trap or false value remains
  diagnosable.
- Show the runtime-profile requirements before consuming the RMTFS/session
  handoff: factory phase, server state, session presence, remote-storage
  registration/readiness, SERVREG and IPA-server registration, Linux IPA
  client state, TFTP port/prepared state/provider presence, and preboot
  generation.
- Flush `PROFILE READ START` before the profile read, then flush a named
  checkpoint before each profile field is read and its value immediately
  afterward, followed by the final gate result.
- Keep the gate report visible if an IRQ arrives after auth entry; preauth,
  IPA receive, GSI, command, transfer, table, UC, containment, and
  SMMU/domain-fault painters may not replace it. The ordinary runtime's
  existing display behavior remains unchanged.
- Use a diagnostic-only build mode that stops before `take_runtime_boot_parts`
  and PAS4, even if all reported gates pass.

## Non-goals and constraints

- Do not change the normal runtime image's gate decisions or handoff behavior.
- Do not embed or copy MPSS firmware bytes in this diagnostic image; the report
  stops before the auth loader needs them.
- The diagnostic auth-entry path must not consume or quarantine owners, submit
  PAS calls, release reset, start GLINK/QRTR, start WLAN, or flash/retry
  anything. Existing preauth provider and notification handling remains in
  place.
- The existing preauth setup still runs as the established prerequisite; this
  offline design does not authorize another device boot.
- Do not infer gate results from the preauth screen's fixed footer.

## Approaches considered

1. **Report both gates before proceeding (selected).** Add a read-only gate
   snapshot and a diagnostic-only stop. It localizes either early health
   failures or runtime-profile mismatches while preserving the normal runtime
   path. This is the selected approach because the current screen cannot
   distinguish an early return from an IRQ repaint.
2. **Painter priority only.** Preserve a generic auth-entry failure screen
   after startup is attempted. This is smaller but does not identify which
   health or profile field failed.
3. **Detailed handoff errors only.** Expand errors from
   `take_runtime_boot_parts`. This covers the later profile check but does not
   expose the earlier silent health return and may require entering the
   mutating handoff path.

## Proposed behavior

At auth entry, mark diagnostics as the display owner before the first health
read. Flush a named checkpoint before each of the four health predicates and
their result. If any predicate is false, retain the named stop result. If all
pass, flush `PROFILE READ START` before calling the read-only runtime-profile
report, then display and flush its requirements individually. A trap in that
call therefore leaves the profile-start checkpoint visible. After the
preboot-generation check, show whether handoff would pass, then stop before
`take_runtime_boot_parts` and PAS4 even when every gate passes. IRQ handling
may continue servicing IPA state, but must leave the current diagnostic screen
untouched. If a report field is unavailable, display an explicit unavailable
value rather than treating it as a pass.

The on-device result should make clear whether the health gate passed, whether
each runtime-profile predicate passed, and that PAS4 was not attempted by this
diagnostic image. A later PAS4/runtime attempt requires a separate image and
separate device-run authorization.

## Builder decisions

Use the existing display and `RuntimeProfileReport`, adding a callback form
that exposes each field read as it happens. Add only the smallest read-only
factory snapshot needed to expose checks currently hidden inside
`take_runtime_boot_parts`. Keep the diagnostic mode behind a dedicated Cargo
feature so normal runtime behavior remains unchanged. The implementation plan
will define exact screen rows and source-level checks.
