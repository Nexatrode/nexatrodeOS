# One bounded QRTR credit turn at the converted transaction 4 opportunity

The settled `bdc954d9` result identified the first missed opportunity: the registry request/token, fitting buffer and normal candidate were valid, but a ready QRTR credit excluded registry priority (gate 10). The writer instead committed the grant advertisement, ending that opportunity, then ACKed the grant. Transaction 4 published 6,199.020 ms after queueing; IPA completed, WLAN RRQ and WLFW remained zero. The trace did not identify when that credit subsequently published or establish that the same credit blocked all ten arms. The full result and the zero length-field limitation are recorded separately in `sunfish-registry-tx4-life-bdc954d9.md`.

## Change and boundaries

`sunfish-registry-converted-credit-turn` lends one ordinary writer attempt to an already-ready QRTR Resume at that exact converted registry opportunity. It honors the credit rather than skipping it to force a registry response.

The actual converted token must be valid for the current owner/generation/request. The normal registry candidate must match it, with its own RX_DONE complete, normal fit/send ordering and no registry cooldown or protected TFTP/WRQ work. The route must be ServiceRegistry transaction 4 to peer 0:9. The normal class-1 selector must find an unsent 52-byte Resume with a fitting remote intent. No captured diagnostic state or clock decides eligibility.

A separate per-registry-transmit credit latch is spent immediately before the actual checked callback, including WouldBlock, uncertainty or unwind. The same conversion token is consumed, so it cannot lend both a credit and registry write. Its fresh grant advertisement and positive ACK retain their normal order afterward. The original registry borrowed-attempt latch remains independent: a later legitimate conversion can still lend the registry reply a turn after the credit has progressed. Reset/retirement use the existing empty-transmit reset; identities cannot transfer to another request or generation.

The credit goes through the existing encoder, remote-intent ownership, FIFO validation, readback, TX-head publication and GLINK receipt rules. No extra device reads, early retirement, reference-flow credit creation, expanded reply window or bypass of a queued packet is introduced. Selection only changes at this bounded opportunity. If another ready credit remains later, it retains normal priority; this change does not drain every flow or guarantee a hardware deadline.

The saved Linux/Android handlers retain advertisement before positive ACK. The new intervening packet is an unrelated, already-ready QRTR credit using a valid modem intent, not the positive grant ACK itself. Offline tests preserve the matching grant's advertisement-before-ACK order and keep credit and registry GLINK receipts distinct.

## Trace and the size anomaly

The panel retains the first token's entry, selection, callback outcome, token end and matching grant timing, and counts later arms without overwriting it. New evidence:

* `ENTRY FLAGS` bit 19 marks the bounded credit candidate; bit 20 marks agreement between the live nonzero transmit length and the queue's recorded publication length. Earlier bits/gate definitions remain unchanged.
* `TX4 MASK` bits 17/18 mark the borrowed credit callback attempt/commit. A normal fallback commit does not retrospectively set borrowed-commit bit 18.
* `EXIT CAUSE 5` means the bounded credit callback started. It is not proof of commit; inspect WRITE RESULT and bit 18.
* `CREDIT NODES` and `CREDIT PORTS` pack source high 16 bits and destination low 16 bits from the selected Resume. Out-of-range identifiers remain unavailable rather than being truncated. They identify an attempted credit; WRITE RESULT establishes its outcome.
* `ARM LEN SENT` is now captured as one packed u32, with high length from the already-recorded queue publication size and low bytes sent. The independent live-length comparison is reported by bit 20. The preceding zero field remains unexplained; this instrumentation change is not a claim that the phone sent a zero-byte response or that its cause is proven fixed. Neither value controls policy.

The two live identity fields replaced by CREDIT NODES/PORTS remain validated by entry bits 3/4 and ARM BACKING/REQUEST. CHOICE TXN CMD still packs QMI transaction high 16 bits (FFFF when absent), GLINK command low 16 bits; a Resume should show `FFFF0009`. Its GLINK remote intent is CHOICE PARAM. WRITE RESULT remains 0 veto/no callback, 1 committed, 2 WouldBlock, 3 uncertain, 4 callback entered without returned result. Grant advertisement and ACK have separate retained times.

The first selected packet may be a vetoed bounded follow-up while a later ordinary entry borrows the credit. In that case inspect the attempt/commit mask and EXIT as well as the frozen first CHOICE. No timestamps or diagnostic counters control scheduling.

## Offline evidence and delivery

Six new dedicated tests cover successful credit-before-grant/reply sequencing with distinct original receipts; blocked callback and byte-identical normal fallback; uncertain/unwound quarantine and stale generation rejection; a bounded follow-up that cannot spend the ordinary opportunity; missing credit/registry fit; and two ready credits proving the loan is once per original registry reply. The inherited seven lifetime tests remain enabled with feature-specific expected ordering. A capacity fixture was corrected to ensure transaction 3 consumed its large intent before installing the intended small-intent controls; the original fixture had left a large intent available and did not represent a missing-fit case.

Current/prior Core/link suites: **257/251 passed**. Current/prior Owner suites: **577/577 passed**, one existing ignored test each. Thirteen lifetime tests include the six new tests. Twenty-four paired Owner control schedules preserve full device I/O and peer-event parity; those controls did not exercise an order-changing credit opportunity, so they are not evidence that the new policy is behaviorally inert. The dedicated tests exercise that change. Existing FIFO, uncertain write, close, advertisement, cursor, exhaustive table/domain fault and actual Linux credit-oracle checks remain passing. The real 27-field renderer and both framebuffers are verified.

Private profile: `.cache/sunfish-native-wifi/private/native-registry-converted-credit-v1`.

* Boot image: **68,923,392 bytes**.
* SHA256: **b42b902f36139f97f160beac588e4fb525875ee6a1011393141d6f98f1076326**.
* Native selected stack: **4,321,024 bytes**, leaving **921,856 bytes** in the 5 MiB stack. This excludes TLS, indirect calls and other descendants; it is not whole-program worst-case stack analysis.
* CapDL: **24,542 objects**. All **3,637 named protocol-frame physical bindings** match the preceding image, including all eight Normal-NC table frames, MMIO and data pools.
* The actual ELF/loader/compressed boot payload, capability audit input hashes and **304 rejected mutations** are verified. Whole loader/CapDL/report reproduction and the packed initializer archive bind the reused pinned SDK/tool components, actual object count and eight Normal-NC maps. SDK components are not claimed freshly recompiled. Exact source and artifact hashes are preserved privately.

Hardware outcome is untested until the next cold temporary boot. The goal is to observe the credit commit followed by the grant pair and earlier registry publication; this does not predict WLAN/WLFW will start. No device command is executed during offline preparation.

Fully power off, enter bootloader with Volume Down + Power, then run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-converted-credit-v1/fastboot.img
```

Wait for **REGISTRY CREDIT TURN** and **RESTART WHEN DONE**, then capture the entire panel.
