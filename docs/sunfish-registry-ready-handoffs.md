# Bounded ready-reply and exact-blocker handoffs

[IMG_9804](sunfish-startup-pressure-IMG_9804.md) accounted for all128 transaction3 writer calls:105 pending-intent-control exclusions,13 own-return exclusions,9 TFTP-priority exclusions and eventual publication. The conversion-bound loan never armed while that reply's own RX_DONE was pending. The same boot proved the lifetime first-slot handoff was spent on transaction3, and split the later exact transaction4 receipt→ELF queue interval into194.690 ms finishing transport work,505.798 ms before registry service, and about30.366 ms selecting/admitting. These are two different delays; their counters are not an exclusive CPU-time partition.

Feature`sunfish-registry-ready-handoffs` inherits`sunfish-startup-pressure` and adds two independently bounded scheduling opportunities. All FIFO/native validation, own-return prerequisites, normal request order, fitting-intent selection, route/reference-credit checks and queue limits remain authoritative. This is a scheduling change, not a renderer-only diagnostic.

## Earlier transaction3 reply

A private bit on the actual transmit record is armed only after its own host RX_DONE writer commits, while that unsent registry response is already queued. It applies to transaction3 from the local service-registry port to modem node0/port9. It is not armed from clocks, pressure snapshots, request-number guesses about another record, an unrelated return, or a blocked/uncertain return. Record construction/reset clears the bit.

On an ordinary publisher call, that reply may borrow one DATA attempt before a positive grant's backing advertisement when the actual backing is still in Advertising state. The opportunity is considered only when the existing yield/conversion mechanism has no loan, the registry cooldown is clear, independent protected TFTP/WRQ work is absent, the own return has completed, the normal ready candidate has a fitting remote intent, and no ready QRTR credit preempts registry priority. A reply that already used a grant or converted-credit loan cannot borrow again. Denials, reusable/already-advertised backing and missing fit retain their normal order.

The existing per-transmit borrowed-turn latch is consumed **before** the checked writer, including WouldBlock or unwind. A blocked attempt therefore falls back to the grant advertisement, its ACK and normal byte-identical reply retry. Uncertainty/unwind quarantines the Session. The next fresh grant cannot resurrect that transmit's loan. No extra publisher loop or unrestricted drain is introduced. The matching positive ACK still follows its committed advertisement; a reply never overtakes that ACK after the backing has already been advertised.

Frozen publication mask bit19 (`00080000`) records this new actual callback attempt, bit20 (`00100000`) its successful write. Existing borrowed attempt/commit and registry-priority bits remain set as appropriate. These masks are observations only. The borrowed gate counter now recognizes the added live ready candidate rather than incorrectly calling its selected attempt a missing-yield rejection. The old converted-grant ID/times can remain unavailable for an ordinary grant loan; that does not mean the new attempt failed.

## Exact ELF blocker receipt

After an actual FlowBusy queue result for the production first-ELF handle, Core can identify a unique live unpublished registry owner at the same source/destination endpoints. It validates the waiting opaque handle and route through the current request/router state. Owner retains a separate private `(waiting, blocker)` dependency. Neither the displayed ELF failure history nor its clock is scheduling authority.

If the original lifetime first-slot turn has already been spent, a later **exact validated completion of that blocker** may supply one additional handoff. Owner consumes the current ingest's existing typed receipt witness and repeats the existing conservative live same-peer eligibility checks. It sets a private taken bit/ready handle before finishing the checked iteration and returning normally. Wrong owner/generation/peer/handle, an unrelated or invalid completion, stale receipt, another unpublished peer owner, absent original request or an already-spent new handoff cannot lend this opportunity. Fresh Owner construction resets it.

At the beginning of the ordinary Runtime service pass, after existing MCFG close cleanup, the guarded IRQ router may consume that ready handle once. It requires a live router, continuation, no pending other-owner notifications and matching Owner identity/generation. Owner defers for pending MCFG/firmware/optional prototype TFTP close work. It then consumes the handle before callbacks and repeats normal pending-request ordering, request validation and `dispatch_service_registry_step`. All ordinary capacity, response, flow and credit checks run again. A Wait, veto, error or unwind cannot reuse this attempt; ordinary continuation/service remains responsible for later progress. If ELF queues, its already-existing one-shot checked queued-reply dispatch is used; other service steps continue normally.

The normal `pending_request_for_service`, queue validation and registry timing callback boundaries are stopped-guarded under this feature. A callback unwind leaves Owner stopped; normal return restores the previous stopped value. This includes Core queue clocks and inherited timing observations, with no added clock sample. The handoff is already consumed before them. These guarded boundaries were required by the new clock-fault sweep; diagnostic observation must not make the new path reusable after unwind.

The new trace is a read-only copy, with fields flags/arms/receipts/services/result. Flags:1 dependency bound,2 exact receipt handed off,4 service attempt consumed,8 service returned,10 reply queued,20 stale/wrong-head veto (hexadecimal). Result1 queued,2 normal wait,3 idle,4 progress,5 error;0 unknown/not returned. A trace field never grants permission. Repeated normal queue failures can increment arms while there is still only one new receipt/service opportunity. The earlier first-slot Boolean and its original receipt transaction/sequence remain separate.

## Offline verification

Core replays cover own-return-before-fresh-grant publication, exact original completion, matching response bytes, advertised ACK order, other transaction exclusion, ready QRTR credit priority, missing fit/outbound-intent progression, ordinary versus bounded-followup selection, blocked retry identity, uncertainty, unwind, reset, and live unpublished-handle lookup. Historical fixtures that specifically require an unpublished predecessor now queue after its return under the child feature; they still test their original host-entry/flow invariant without accidentally exercising the new optimization.

Actual Owner/FIFO/Router schedules consume the first-slot turn on3, reject ELF behind unpublished4, observe4's real receipt, take the new handoff and complete the original ELF5 transaction. Additional cases cover unrelated/invalid receipts, stale ordinary-service wins, full capacity returning WaitingReply with no repeated handoff, live native-validation failure, fresh Owner/getter behavior, other-owner IRQ priority, wrong generation, and invalid RX backlog being parsed before queued ELF publication. Every optional clock in a successful early-service schedule is faulted in turn; all unwind cases retain consumed authority and leave Owner stopped. The old fault-after-receipt fixture also checks the new successful checked return followed by a later invalid record, rather than falsely expecting that later record to be consumed in the earlier iteration.

Three functional removal controls disable the actual ready-reply choice, exact-receipt break or guarded early Router service independently. Each makes its real new functional case fail. A fourth control removes the actual Runtime early-service call and makes the integration-placement source contract fail. Selected full Core/Owner/IRQ suites and default-off parent Core/Owner suites remain required. Removing the marked production blocks and child Cargo line recovers every parent Rust source at`8f10d267` exactly (including one explicit lexical rejoin in the borrowed-gate predicate). Older source-normalization tests strip this child delta before applying their original proofs.

Native verification binds committed-source AArch64 bytes to the temporary boot image, checks selected stack headroom, audits capabilities with304 rejected mutations, validates the complete boot layout, and reproduces loader/report/CapDL plus the actual eight Normal-NC serialized mappings using the pinned SDK. The SDK is reused, not freshly compiled. The private manifest binds source/review inputs, final logs, removal controls and preserved previous delivery artifacts. These are finite schedules and selected stack bounds, not universal interleaving or worst-case execution proofs.

## Next cold boot

The adaptive panel is titled **REGISTRY READY HANDOFF** without an actual full-pool capture, or **READY HANDOFF POOL** when the pool capture exists. The parent row meanings remain unchanged except row26 now displays`HANDOFF FLAGS ARMS RECEIPTS SERVICES RESULT`. In the reply view, look for transaction3 mask bits19/20, its queue→publication time, the exact-blocker flags/result and retry spans. In pool mode, first/latest slot records and first-free/admitted identity remain visible. Missing handoff flags can mean that an earlier successful reply eliminated the blocked-ELF sequence; it is not automatically a failed implementation.

This test measures whether either opportunity occurs in the modem's current startup schedule, whether original replies still complete and whether firmware RRQ/WLFW change. It does not promise that faster registry service will launch WLAN or establish a private firmware dependency.

Fully power off, enter the bootloader with Volume Down + Power, then manually run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-ready-handoffs-v1/fastboot.img
```

Wait at least one minute, then send the full settled **REGISTRY READY HANDOFF** or **READY HANDOFF POOL** panel. No automatic device command or partition flash is performed.
