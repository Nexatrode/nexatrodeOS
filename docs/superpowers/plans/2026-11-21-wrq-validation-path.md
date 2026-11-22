# WRQ validation and post-return path

Authorized scope: offline work through a verified image, then manual cold temporary boot. No phone actions, commits or integration changes.

Evidence: IMG_9682 measured 602.720 ms of successful FIFO validation inside 888.418 ms before WRQ return, then 665.148 ms to OACK publication. Existing code does six retained-domain scans per successful sample. Prior status/cursor sharing and removed-check trials changed fault boundaries; earlier priority-only trials did not establish the required end-to-end saving.

Design: observer-only leaf inheriting WRQ STAGE COSTS and the existing spare-buffer schedule. Preserve every validation, access, selector, fault boundary and ordering. Measure status/cursor/acquire-status call spans and existing native pre/controller/post boundaries for each successful sample. Retain bounded timestamped histories through original OACK publication, independently of the old frozen-at-return history. Export before-return and after-return clipped costs, registration markers, bounded post-return record identities, missing-domain evidence and history loss. No arbitrary payload. Domain time nests inside operation/check time; RX/TX spans can contain checks. Do not add nested totals or treat elapsed intervals as exclusive CPU time.

1. Add failing clipping, partial-domain, overflow, lifetime and unchanged-I/O tests. Implement bounded observer and native timestamp hooks without replacing checks.
2. Add a single legible WRQ VALIDATION PATH panel; retain exact close visibility, OACK peer read, DATA and WLAN/WLFW outcomes. Render and inspect.
3. Review source and compare complete observer on/off replay I/O/events, existing fault controls; run owner/link/source/stack tests. Build target, package, audit final capability mutations, verify exact payload and hashes. Document evidence/limits. Deliver one manual fastboot command.
