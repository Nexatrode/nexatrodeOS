# Current WRQ stage-cost diagnostic

Goal: Attribute the current-profile visibility-to-WRQ-return delay on one handset boot. Reuse existing receive-record and successful cursor-sample observers, extending their lifetime through the original exact port-0x1f WRQ return. Preserve every protocol decision, eight-slot allocation, FIFO validation, fault boundary, advertisement/ACK and QRTR credit ordering.

User authorization: Continue offline through a useful built and verified image; no more approval questions or automatic device actions. Existing dirty Wi-Fi worktree is the authorized isolated workspace.

Evidence: IMG_9679/9681 give visibility→return 887.865 ms but omit current acceptance/queue times. IMG_9661 has 599.867 ms ingress with 391.749 ms in nested checks; older same-boot accepted→queue/queue→return stages and replay counts cannot be pasted onto the current boot. Earlier queue/return reorderings and empty duplicate-lookup reductions have already been tested and are not untried fixes.

Design: Add one opt-in observer leaf based on the shipped spare-buffer image. Freeze record/check histories at original WRQ return. Export three clipped stage totals, successful cursor-check time/count (nested), RX/TX record-span time, history loss, and bounded QRTR type/source/destination metadata for the records. Export acceptance, OACK queue and return on the same clock as WRQ visibility, OACK head/read and exact close bounds. Keep no arbitrary payload. A diagnostic cost sum must not add nested checks to record time. Loss and missing endpoints remain explicit. Preserve matching original DATA and WLAN/WLFW outcomes. Packet metadata for incomplete QRTR headers remains unknown.

Task 1: Test lifecycle extension/freeze, clipping, loss and header attribution first; implement observer and public accessors. Observe RED then GREEN.
Task 2: Add current stage panel with labels and metadata rows; render/inspect both framebuffers. Compare fresh Linux/Android source with current native gates and saved same-boot evidence; document the limits.
Task 3: Full owner/link/source/source-guard/stack tests, observer-on/off replay parity across both prefix shapes/credit/fault controls, AArch64 build, packaged ELF/kernel/boot verification and exact final 280-mutation audit. Hash evidence and report exact manual cold fastboot command. No commit/merge/push or phone action.

Review focus: Histories must freeze at exact original return, preserve the return write's overlapping span, retain prefix/header identity across ring wrap, report lost evidence, and leave on/off FIFO I/O and peer events identical. No new production scheduling policy.
