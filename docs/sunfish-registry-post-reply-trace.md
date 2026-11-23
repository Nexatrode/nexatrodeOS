# Registry post-reply trace after IMG_9704

IMG_9704 confirmed that the conversion-bound transaction 3 turn actually ran: queue-to-publication fell from 9,997.159 ms to 317.864 ms, with four publication calls instead of 81. This is a local scheduling improvement, not an end-to-end WLAN result. Transaction 4 then waited 7,084.542 ms between queueing and publication. Transaction 3's matching RX_DONE was processed 10,902.755 ms after publication. Transaction 4 published 309.842 ms before that completion, so waiting for transaction 3's receipt is not an admission prerequisite for transaction 4. ELF still took 8,818.316 ms from receive to completion; WLAN RRQ and WLFW counters stayed zero.

The saved panel does not contain transaction 3's peer-tail/arrival observations or transaction 4's publication-pressure snapshot. It cannot establish whether the modem read transaction 3 late, returned its buffer late, or returned it behind a backlog that the host processed late. Repeating transaction 3's already-successful queue-delay capture would not answer that question. No additional scheduler relaxation is justified from those missing observations.

## Offline review and unchanged contracts

The saved Linux GLINK handler allocates a requested receive intent, advertises it, then sends the positive request ACK. Android first looks for an already advertised reusable intent; otherwise it allocates, advertises and ACKs in the same order. Reviewed copies are the pinned `qcom_glink_native.c` files under the private Mobian source-review and Lineage kernel trees; their hashes remain in the delivery's reference-hashes file. The native converted-grant turn retains advertisement before ACK. Its one borrowed reply write, same-peer reply window, own RX_DONE requirement, fitting remote buffer selection, FIFO checks and QRTR credit accounting are unchanged from `cb48831c`.

The current pure replay tests preserve receipt identity, blocked-write fallback and actual credit pressure. They cannot manufacture the missing hardware timestamps. A finite set of Owner schedules also preserves exact full device I/O and peer-event order with this observer on/off; it is not proof of all interleavings or a phone deadline.

## New observation

The feature `sunfish-registry-post-reply-trace` adds three bounded observations:

* Transaction 3's full-publication boundary arms a separate FIFO window. It reuses successful validated cursor samples, tracks peer-tail consumption through the reply, and retains 32 RX-head advance spans until the normal decoder identifies the exact opaque predecessor receipt. It does not scan or bypass intervening records. It arms and freezes correctly without a clock, leaving timestamps unknown.
* The converted backing intent ID and its matching advertisement/positive ACK survive consumption of the scheduling token. Later unrelated grants do not overwrite the capture. Advertisement timing reuses the existing grant timestamp. A matching ACK adds one optional clock observation. If a later request supersedes the tracked backing, this capture does not label that request's ACK as the original grant's ACK.
* Transaction 4's existing per-transmit publication counters remain live while unpublished and freeze after full successful publication, bound to its opaque owner/generation/request identity. Retirement and TX-slot reuse cannot replace the snapshot.

No hardware read, validation gate, writer priority or protocol credit limit is added or removed. Optional clock observations and bounded metadata do add overhead; timing comparisons must account for that. The selected stack audit measures 4,318,912 bytes against a 5,242,880-byte stack, leaving 923,968 bytes. It excludes TLS, indirect calls and other descendants and is not whole-program worst-case stack analysis.

## Panel: REGISTRY POST REPLY

All displayed timestamps are hexadecimal microseconds on the same runtime clock. `FFFFFFFF` means missing/unknown, not an elapsed duration. OLD is the exact transaction 3 predecessor, not IPA registration or transaction 4.

| Fields | Meaning |
|---|---|
| OLD PUB / TAIL / DONE | Full checked publication; sampled modem FIFO consumption through the reply; host processing of matching RX_DONE |
| OLD RX ABSENT / VISIBLE / AHEAD | Last sample start before the complete receipt appeared, first sample end containing it, and bytes queued ahead at that arrival observation |
| RX END EVICT | High 16 bits: exact receipt matched/frozen; low 16 bits: overwritten RX-head history spans, saturating |
| GRANT ID / RESERVED / AD / ACK | Exact converted backing ID, reservation, committed advertisement and matching committed positive ACK |
| TX4 QUEUE / PUB / SNAP STATE | Queue/publication times; state 0 absent, 1 live, 2 full-publication snapshot frozen |
| TX4 CALLS / MASK | Publication-call count and accumulated selection/availability bits during the unpublished interval |
| NOACK DENY through CREDIT BORROW | Twelve first-condition counts packed in order, two saturating 16-bit counts per line |
| ELF DONE / IPA COMPLETE / WLAN RRQ RX / WLFW SERVERS | End-to-end startup outcomes observed in this boot |

Mask bits 0–6 respectively mean no fitting remote intent, outbound request waiting, control selected, other DATA selected, selected reply blocked by FIFO, DATA fragment committed, and outbound intent request selected. Bits 7–12 retain the existing ordinary/borrowed/conversion-bound selection and commit markers. Bits 13–16 retain conversion-seen, own return pending, not the exact normal candidate and already-used-at-conversion observations. The mask is an accumulated set, not a chronological packet trace.

The twelve grant gates are: no pending ACK; pending denial; positive ACK with no unadvertised backing; fresh backing without ordinary yield/exact conversion opportunity; registry cooldown; protected TFTP; protected WRQ; own receive return pending; per-reply borrowed turn already spent; no exact normal candidate; ready QRTR credit; selected borrowed candidate. Only the first applicable condition increments, so a high-priority exclusion can hide lower-priority conditions. A zero credit bucket does not prove credit was available at every instant.

If arrival history was evicted, completion matching can still freeze the window but visibility timestamps may remain unknown. Never substitute DONE for first visibility. TAIL is transport FIFO consumption, not application-level acceptance. A matching GLINK receipt and ELF locator completion do not establish WLAN firmware loading or working Wi-Fi.

## Delivery and verification

Private image: `.cache/sunfish-native-wifi/private/native-registry-post-reply-v1/fastboot.img`.

* Size: **68,919,296 bytes**.
* SHA256: **9c84d94acef84f72c4bfeb467966d66fa4a1b95fa883b33250a4544a26f72293**.
* Current/prior Owner suites: **577/575 passed**, one existing ignored test each. Current/prior Core/link suites: **244/242 passed**.
* Dedicated tests cover exact receipt across cursor wrap, unrelated record ahead, absent clock, frozen snapshots, bounded history loss, blocked advertisement, later unrelated grant, transaction 4 missing-fit and blocked/frozen publication, reverse receipt identity and reset. Actual renderer verifies all 27 fields, missing values, saturated packed counts, title fit and both framebuffers.
* Twenty-four paired Owner control schedules retain exact full I/O and peer-event parity. The pinned Linux C credit oracle and ten Rust flow tests pass. Existing validation corruption, uncertain write, close, cursor and advertisement fault controls remain enabled.
* Current native capability audit rejects **304 mutations**. The actual CapDL has **24,541 objects**. Compared with the prior image, 544 data-pool frames move 4,096 bytes higher; runtime uses generated physical-address symbols. The other 3,093 named protocol bindings, including all eight Normal-NC table frames and MMIO, remain unchanged.
* Packed loader/ELF/Android boot identity, pinned SDK/tool reproduction, actual initializer archive, Normal-NC maps and selected stack are independently checked against this image. SDK components are reused unchanged; they are not claimed to have been freshly recompiled. Private manifests bind exact artifacts and source hashes.

No device operation is performed by these checks. Hardware outcome remains untested until the next cold temporary boot.

After fully powering off, enter bootloader with Volume Down + Power, then run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-post-reply-v1/fastboot.img
```

Wait for **REGISTRY POST REPLY** and **RESTART WHEN DONE**, then capture the whole panel. This tests the downstream gap while retaining the successfully exercised early reply policy. It does not predict WLAN or WLFW will start.
