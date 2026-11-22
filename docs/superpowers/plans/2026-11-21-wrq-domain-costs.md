# WRQ domain costs

Continue the authorized offline investigation through the next useful verified image, with no automatic device action. Existing dirty Wi-Fi worktree remains isolated; no commits or merge.

IMG_9684 attributes485.468ms before WRQ return and372.814ms afterward to complete native pre/post domain validation. Existing notes already cover retained canonical expectations, grouped table checks, rejected status/cursor pairing and small final-write CPU scans. No evidence yet assigns the remaining domain time to uncached table reads instead of live fault/ASID/route/register reads. Host cached-memory timings cannot settle this.

Design: observer-only leaf sunfish-wrq-domain-costs inherits the physically tested validation-path schedule. Reuse the existing retained_receive_domain_with progress callbacks to time six categories: entry/fault/control, ASID banks, route slots, final registers, table setup/sync, table comparison/trailing sync. Timestamp category transitions only, not every word/bank/slot. Retain exact bounded timestamps for each pre/post validation of three operations per successful Ring sample, so two phase windows clip at WRQ return precisely. Keep current domain and operation timing, missing/evicted evidence and matching OACK/read/close outcomes. No skipped reads, cached authority, widened accesses, moved validation, altered grants/credits or scheduling.

1. Add failing pure stage-transition/clipping/missing tests and native stage/fault/order tests; implement optional callbacks, scoped transport capture and bounded phase cost accounting. Domain stage callbacks do not authorize anything.
2. Add WRQ DOMAIN COSTS panel with before/after phase totals, six category costs, domain/body cross-checks and current outcomes; render and inspect. Every partial failed sample stays unavailable; terminal errors remain authoritative.
3. Review diff against shipped observer, compare exact full FIFO I/O/peer-event parity and native ordinary/observed stage sequences; run full owner/link/source/stack tests and fault controls. Build, package, final280 capability audit, exact payload/hash verification. Record ready command; no phone action.

All costs are elapsed wall time including timestamp overhead. Stage categories nest in domain spans, which nest in checks and may nest in RX/TX. Small timestamp gaps outside stage Entry/Complete remain explicit residuals. This diagnostic does not claim a speedup or working Wi-Fi.
