# Registry continuation: defer routine framebuffer redraws

This follows the settled **REGISTRY PREARM IO** result `712d88d4`, documented in `sunfish-registry-prearm-712d88d4.md`. Of 3,029.378 ms from transaction 4 queue observation to first Adapter writer entry, only 119.735 ms was inside measured native transport I/O. The roughly 2,909.643 ms outside those calls was not assigned to a particular runtime operation. The request's local RX_DONE was already complete and a fitting remote intent existed. IPA completed, but WLAN RRQ/WLFW counters remained zero.

## Source review and change

A successful Owner reply queue schedules a continuation rather than synchronously calling the writer. The runtime can finish the current service pass and bounded slice, update progress, and fully redraw the debug display before the next dispatch. A full redraw clears both 1080×2340 framebuffers using volatile stores, then clears/repaints individual rows. These framebuffer stores are outside native transport I/O counters. The existing first-OACK quiet gate throttles selected repaints for five seconds while that OACK remains pending; it does not defer all routine painting whenever the live service continuation is pending. This is a concrete additional source of elapsed work, not proof that rendering accounted for all 2.910 seconds in the preceding boot.

The opt-in `sunfish-registry-defer-paint` feature changes presentation admission only:

- During the Services stage, defer ordinary progress and full-panel rendering while the live router's `continuation_pending` flag is set. No diagnostic clock, registry trace flag, or historical counter determines this gate.
- Retain a dirty bit so the first quiescent opportunity performs a full settled redraw even if no newly reported state changed. An idle footer/progress update cannot consume that debt. Multiple busy requests coalesce into one full repaint.
- Error display stays immediate, including existing direct error-display branches. Startup/opening display remains outside the busy service predicate. Progress checkpoints, schedule observations and the prior first-OACK quiet check remain before the new progress-rendering guard.
- All driver methods, normal writer choices, native checks, Router dispatch, receive processing, grants, advertisement-before-ACK ordering and QRTR credits are unchanged. The presentation change deliberately changes wall timing and may change handset interleavings.

CPU-side render-span accounting clips the already timed progress/full-show spans to the exact Adapter queue → first Adapter writer window. Render totals are diagnostics only. Missing first-entry clocks, reversed endpoints or saturated totals stay unknown. These are enclosing wall times, including preemption, not exclusive CPU time or hardware-operation counts. They must not be added to overlapping outer runtime totals.

## Phone panel and interpretation

The title is **REGISTRY DEFER PAINT**. It retains the previous queue/Adapter/Core/arming/first-packet observations and I/O categories, plus transaction 4 publication, ELF queue/completion, IPA completion and WLAN/WLFW counts.

**PAINT DEFERRED** replaces ELF RX. It counts deferred ordinary rendering requests across the runtime, saturating at `FFFFFFFF`; it is not specifically the number of full redraws in the registry window. **PAINT GAP US** replaces ELF publication. It shows the measured rendered-span total in the transaction 4 queue-to-first-writer window, only after a valid first-entry observation exists. With no rendered span in that completed window, zero is meaningful. It cannot retrospectively measure the preceding boot's render time.

A much shorter queue-to-first-writer interval, with rendering requests deferred, would support repaint contention as a contributor. If the approximately three-second wait persists, that points to other service/handoff work and we should use the retained queue/entry markers to follow it. There is no promise that this resolves the separate ELF prequeue delay or completes Wi-Fi. ELF completion remains a service-registry locator milestone, not firmware loading.

## Offline verification

The actual presentation helper and runtime busy predicate are tested across coalesced busy requests, debt retention across unrequested work and idle progress updates, one settled repaint, immediate errors, service/opening stages, clipped prequeue/post-writer spans, reversed clocks and saturation. Source integration assertions bind the gate to both rendering call sites and preserve progress checkpoint/old quiet-gate order. The actual 27-field renderer is run with both buffers, absent evidence/clocks, packed values and the physical 15-character numeric-label limit; the previous renderer remains tested.

Current and prior feature profiles each pass 580 Owner tests (one existing ignored) and 260 Core tests. The three new presentation/runtime cases are separate from Owner-only controls. Twenty-four paired Owner schedules preserve full protocol and I/O digests and peer-event order; they cannot prove Runtime presentation behavior or all hardware interleavings. Retained credit/grant/backpressure/uncertain-publication/fault tests also pass.

The six relevant protocol/validation source files are byte-identical to `fe3ba2b3`, including Core, FIFO, Owner, Router, native transport and SMMU code. Their hashes are retained in the private policy-boundary proof. This is self-review, without an independent agent.

The native image, selected-stack budget, actual CapDL authority and 304 mutation rejections, Normal-NC packed initializer maps, full loader/report/CapDL reproduction and pinned SDK reuse are verified before delivery. Pinned SDK/tool equality is not a claim of new SDK compilation. All 3,637 named protocol frame addresses remain unchanged from the preceding image. The selected stack scope has 917,120 bytes remaining in 5 MiB, excluding TLS, indirect calls and other descendants; it is not a whole-program worst-case proof.

Private profile: `.cache/sunfish-native-wifi/private/native-registry-defer-paint-v1`. Delivery metadata binds the source, native ELF, packed image, evidence and preserved prior images. The image SHA256 is `2db3517ae7e3e233d62fdbe41393a76bc960c1046fa04660599109dea40185b7`. Handset results will be saved separately without rewriting the delivery manifest. No device command was run while preparing it.

## Manual cold temporary boot

Power off completely, then enter bootloader with Volume Down + Power. Run on the Mac:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-defer-paint-v1/fastboot.img
```

Routine display updates may pause while the router remains busy; fatal errors remain visible and a full settled redraw is retained. Wait for **REGISTRY DEFER PAINT / RESTART WHEN DONE**, then photograph the whole panel.
