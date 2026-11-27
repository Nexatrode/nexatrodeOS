# Registry and ELF pressure with queue-order-independent TX3 binding

Feature `sunfish-registry-elf-pressure` inherits `sunfish-elf-credit-pair-turn`. It includes a narrow Core ordering fix and a combined display. No new timer, I/O, authority state, receive buffer, queue slot or capability is introduced.

## Existing TX3 loan after early receive return

The original TX3 ready marker was set during its own RX_DONE commit only when the response already existed. Host-only registry storage permits RX_DONE before admission. A real Core replay reproduces that order with a validated successful request and original transaction3 response; the original code selected the fresh grant advertisement instead of its existing one-shot DATA opportunity.

After successful normal reply admission, this feature marks the actual transmit when its route matches the original scope: ServiceRegistry transaction3, local registry source port, destination node0/port9. The existing marker is set whether own return came earlier or will come later. This changes no reply bytes and allocates no new state. The normal class3 selector still requires ReplyQueued, actual own-return completion, a fitting peer intent and normal flow/route checks. Existing ready-turn admission protects credits, positive/unadvertised grant backing, protected work and ordinary-call boundaries. The per-transmit spent latch is consumed before callback, so a blocked callback or fresh grant cannot rearm the loan. Advertisement remains before its ACK; normal fallback remains byte-identical. Uncertainty/unwind quarantines; reset creates a fresh Session.

This preserves the original TX3 scope, including failure replies. It does not extend the paired ELF credit loan to TX3: ready QRTR credits retain priority. It does not establish that IMG_9810 missed this marker; the old panel did not show that condition.

Core tests cover early-return success with/without clocks and matching completion, blocked one-shot followed by another fresh grant and identical normal retry, pending own return, ready credit, missing fit, unrelated transaction4, and uncertain/unwound writes plus reset. Parent cases retain return-after-queue and bounded-call exclusions. Removing only the new queue-time assignment makes the exact early-return case fail. Prior paired-credit eligibility/commit removal controls are rerun against this source. Removing marked child blocks and the Cargo feature recovers every parent production source file exactly.

## Combined panel

Title: **REGISTRY ELF PRESSURE**. Times use the existing runtime clock. Read Q/PUB/CORE for TX3 separately from Q/PUB/OWNER for TX4 and ELF. Historical completion does not substitute for live IPA pending state or WLAN/WLFW counts.

Rows3–5 retain both reply timelines and frozen calls/masks. Rows6–9 show all8 TX3 priority counters, all12 grant admission counters and ACK progress; rows10–13 show the same ELF counters. Counter definitions are unchanged. Attempt/success bits19/20 show the ready DATA loan; ELF credit-stage bits17/18 remain independent.

Rows14–18 retain ELF admission wait reasons, last failure and bounded owners, dispatch boundaries, writer totals/omissions and first physical slot release. Rows19–22 show both globally retained digest requests with reservation identity, server result, publication/completion/close and packed port/length. Rows23–28 retain exact-blocker handoff, retry boundaries, actual first-free/admission, live IPA state/credit/body match, original final ACK, digest omission/close counts and independent WLAN/WLFW outcomes. No producer sampling is added. Missing timestamps remainFFFFFFFF, counters remain hexadecimal, packed fields and omissions are explicit. A successful early loan does not guarantee overall startup or WLAN.

Verification uses recorded source-boundary, physical panel, Core/Owner/IRQ, stack,304 capability-mutation, packed-payload and pinned-SDK reproduction checks. The SDK is reused; native runtime is rebuilt. Finite replays and selected stack analysis do not prove every schedule or worst-case stack use. Physical outcome remains to be tested.
