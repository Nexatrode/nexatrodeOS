# Transaction3 publication pressure and the blocked-ELF retry boundary

[IMG_9801](sunfish-elf-prequeue-owners-IMG_9801.md) identifies an earlier registry chain, not the global transmit pool: transaction3 waited **12,636.780 ms queued before publication**, transaction4 had20 FlowBusy outcomes before queueing, and ELF waited8,869.219 ms before selection. ELF's own one failed admission named unpublished transaction4. Its subsequent admission occurred753.629 ms after transaction4's Core receipt. ELF then published/completed promptly (61.130/44.119 ms). WLAN RRQ and WLFW remained zero.

## Offline findings and remaining uncertainty

The existing predecessor trace already freezes transaction3's per-transmit publication counters, first-failing priority/grant gates, ACK/return/advertisement observations and converted-grant times after full publication. They survive its real matching receipt and slot reuse. [IMG_9701](sunfish-registry-pressure-IMG_9701.md) found repeated intent-control competition in an older boot; [IMG_9704](sunfish-registry-grant-retry-IMG_9704.md) demonstrated a bounded conversion-bound publication. Those historical counters cannot be substituted for IMG_9801's12.637-second interval. The current picture does not display its frozen counters, so it does not yet identify which writer condition caused that delay.

The smaller retry gap has a concrete source mechanism worth distinguishing. Owner's first eligible registry receipt ends its checked transport iteration once, via `elf_slot_turn_taken`. The opportunity is lifetime-bounded for the first ELF request. A transaction3 receipt can qualify while ELF is pending but transaction4 remains the service queue's head. Normal bounded registry service may then queue4 and try ELF, which correctly rejects4 while unpublished. The later transaction4 receipt does not grant another lifetime exception; normal Owner work can continue through the rest of its16-iteration budget, then Runtime runs close/TFTP/IPA work before registry service. An actual Owner/FIFO replay exercises exactly that ordering, preserving all normal queue checks and real receipts.

This proves the path is possible, not that all753.629 ms in IMG_9801 followed it. Earlier per-iteration breaks, pending cleanup and different schedules can change which receipt consumes the opportunity. The old photo has no consumed-turn timestamp or Owner-return boundary. The next capture observes those facts rather than adding another scheduling exception. A same-peer gate being structurally eligible at an earlier untried instant does not prove fresh credits or every other guard would have passed.

## Incremental observer

`sunfish-tx3-elf-retry` inherits `sunfish-elf-prequeue-owners`. The new **TX3 AND ELF RETRY** panel reuses the existing transaction3 counters and adds one bounded Owner-only retry observer (size capped at256 bytes). It does not change Core, Adapter, FIFO, queue admission, writer choice, receive retirement, grant advertisement-before-ACK, QRTR credits, close cleanup or the service/transport budget. Diagnostic data never authorizes I/O.

The observer binds the first pending ELF's actual opaque handle, which includes owner/generation identity. It records:

1. The **actual successful one-shot branch**, after its existing live Core witness check and after the existing Boolean is set. It retains the matching Core receipt transaction/sequence and a new optional Owner clock. It does not consume the witness again or decide when to break.
2. The first **exact unpublished blocker receipt**, comparing the real completion handle with the existing ELF blocker's opaque handle while ELF remains pending. It copies the already-recorded Owner completion clock and current progress counters. It does not add a second receipt clock or infer identity from transaction number alone.
3. The successful **Owner work return** for that receipt, after existing rearm/schedule/error handling, immediately before clearing `stopped`. If its optional clock unwinds, the Owner remains stopped and no successful return is latched.
4. The next **registry dispatch entry**, after its existing live check, and the selection following the normal pending-request/validation checks. The selected request is compared with the original ELF handle; a different selection is explicitly flagged.

Each new clock callback runs with the Owner stopped guard set and restores its prior value only on a normal return; unwinding leaves the Owner stopped. There are at most four additional optional clock samples: consumed-slot branch, successful work return, first registry entry and its selection. Latches work without clocks. No extra FIFO/MMIO reads or validation calls are introduced. Apart from guarding the new clock callbacks, hooks only read existing software state and write their private metadata; four timing samples and a larger Owner can perturb wall-clock costs. Both normal and Linux-IPA Owner constructors initialize an empty observer. Invalid completion cannot latch a receipt; a fault after a valid receipt cannot fabricate return/entry. The bound handle cannot be silently rebound to a later request. Fresh Owner construction resets it.

Removing the gated blocks (eight in Owner, one in its Linux-IPA constructor, two in the prior renderer) recovers the source at `0e7fca41` exactly. Older source-normalization tests now strip only this child observer before applying their existing proofs. Existing producer bodies remain unchanged; the new observer adds metadata at the stated boundaries. It is not a pure renderer-only change.

## Panel decoding

All values are hexadecimal; times and spans are converted microseconds. FFFFFFFF means missing/unavailable/regressed conversion as appropriate, not zero. Counters are not an exclusive elapsed-time partition. All numeric rows fit six eight-digit values on both framebuffers.

| Row / label | Values and scope |
| --- | --- |
| 2 ERROR DETAIL IPA PENDING CLOSE STAGE | Error, detail, existing live IPA flags, pending IPA message ID, pending MCFG-close port, cleanup stage. |
| 3 TX3 QUEUE PUB CORE DONE SNAP TXN BYTES | Existing predecessor queue/publication/Core receipt; snapshot state0 absent/1 live/2 frozen; retained transaction/bytes. |
| 4 TX3 CALLS MASK GRANT ID RESERVED AD ACK | Frozen publication calls/mask; exact converted grant's backing ID, conversion, advertisement, ACK times. Missing conversion does not mean no grant traffic occurred. |
| 5..6 TX3 PRIORITY GATES | Full counters0..7: cooldown, protected TFTP, pending intent ACK, own receive return, no candidate, other registry candidate, priority/credit unavailable, selected. Only the first failing condition is counted per observation. |
| 7..9 TX3 GRANT GATES | Counters0..B: no ACK, denial, no unadvertised backing, no yield/conversion eligibility, cooldown, TFTP, WRQ return, own return, opportunity used, candidate mismatch, priority mismatch, selected. |
| 10 TX3 ACK SENT BLOCKED DONE ADS AGE US | ACK commits, selected ACK WouldBlock, receive returns ahead, advertisements ahead, maximum completed pending-ACK age. These are GLINK ACKs, not a TFTP final ACK. |
| 11 TX4 RX QUEUE PUB OWNER DONE | Existing exact transaction4 timeline, with the later Owner completion boundary. |
| 12 ELF RX QUEUE PUB OWNER DONE | Existing exact first ELF timeline. |
| 13 RETRY FLAGS BLOCKER SLOT US TXN SEQ | New observer flags, exact blocker's transaction, actual consumed-slot-branch time, its Core receipt transaction and observation sequence. |
| 14 RECEIPT OWNER RETURN REG ENTRY SELECT | Existing matching blocker Owner receipt clock, successful containing work return, next registry dispatch entry, selection after normal pending-request checks. |
| 15 COUNTS RECEIPT RX TX RETURN RX TX | Existing Owner progress counts copied at the receipt hook and work return. RX at receipt includes that matching receive; later TX count includes any publication work remaining in that iteration. Their difference describes recorded progress in the same work call, not all hardware records or exclusive costs. |
| 16 RETRY US WORK SERVICE CHECK TO QUEUE | Receipt→work return; return→registry entry; entry→selection; selection→ELF queue. With valid ordered boundaries and exact-selection flag these partition Owner-receipt→queue. They are not exclusive CPU time. A different selected request or entry-before-return flag prevents interpreting the last span as ELF's own queue-check cost. |
| 17 ELF WAITS NONE PEER TXCAP FLOW CREDIT CAP | Existing failure buckets0..5, as on the preceding panel. |
| 18 PRESELECT CALLS OMIT RUN0..3 | Existing earlier-dispatch compressed runs before a recorded first ELF selection; transaction/name/outcome/count encoding is unchanged. Clockless producer scope retains its prior limitation. |
| 19 LAST FAIL / 20 OWNERS A B | Existing latest exact ELF rejection and up to two same-peer owner identities. Reason and packed entry flags retain the preceding panel's meanings. |
| 21..22 RX0..3 / 23 RECEIPTS OMIT SEQ0..3 | Existing exact Core receipt journal and sequence/omission counts. Each receipt triple is time, `transaction<<16 | remaining<<8 | target*2 | matched_last_failure`, full intent ID. Compare its sequence with the consumed-slot sequence. |
| 24 ADMIT US SEQ N GATES LASTRX TRIES | Existing successful Core admission, sequence, earlier-owner count, derived gates, last receipt transaction, attempt count. Owner queue time is later. |
| 25 SLOT USED CAP FLAGS RESULT TURN IN OUT FLAGS | Existing lifetime first-slot Boolean, capacity-retry flags/result, queued-ELF handoff entry/end/flags. Different exception types remain separate. |
| 26 TX4 SNAP CALLS MASK TXN BYTES | Existing paired transaction4 publication snapshot0 absent/1 live/2 frozen, calls, mask, transaction, bytes. It is not substituted for transaction3's counters. |
| 27 ORIG ACK READ MASK LOC Q DONE | Original final ACK completion, read count/kind mask, locator queued/completed name masks. |
| 28 IPA PD WLAN WLFW PAIR | Historical IPA-complete latch, host PD state, firmware RRQ count, WLFW servers, retained EFS overlap flags/admissions/offers. |

Retry flags:1 original pending ELF bound;2 exact blocker receipt observed;4 successful containing work returned;8 next registry entry;10 selection observed;20 actual one-shot consumed;40 one-shot was already spent at the receipt hook;80 selected request is the original ELF;100 work returned deferred;200 registry entry appeared before a successful recorded work return. Flag values in this paragraph are hexadecimal. Missing timestamps with present flags can mean no clock, and do not erase identity/event evidence.

Publication mask bits0..7 retain no-fit/outstanding-intent/control/other-DATA/selected-blocked/DATA-committed/outbound-request/priority-committed meanings. Higher bounded-experiment bits retain their documented conversion/borrowed-turn semantics; see [the original pressure note](sunfish-registry-pressure-IMG_9701.md) and [writer capture](sunfish-elf-writer-publication.md). A gate count or selected header alone is not proof of a successful write. GLINK buffer return and sampled shared-FIFO consumption are not remote application acceptance.

## Verification and next cold boot

Actual Owner cases cover the transaction3-consumed turn followed by transaction4 blocking ELF, both clocked and clockless latches, real receipt/completion identity, unrelated traffic, invalid completion, terminal failure after a matching receipt, injected clock unwinds at receipt/return/registry-entry/selection, getter no-I/O, frozen later traffic and fresh-Owner reset. The clockless observer-on/off schedule compares the complete recorded Fake I/O sequence/arguments and actual ELF QRTR bytes. Removing either receipt or consumed-slot hook makes the actual Owner case fail as intended. The actual renderer tests independent frozen snapshots, every gate, exact retry spans, partial/missing/clockless/regressed data and both framebuffers.

Full Owner/Core/IRQ suites and inherited grant/credit/retry/backpressure/close/fault tests remain required. The final native selected-stack audit,304 capability mutations, actual Normal-NC packed archive mappings and whole loader/report/CapDL reproduction bind the private manifest. The pinned SDK is reused, not recompiled. These are finite offline schedules, not measured Pixel timing or a prediction of WLAN startup.

Profile `native-tx3-elf-retry-v1`. Fully power off, enter bootloader with Volume Down + Power, then manually run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-tx3-elf-retry-v1/fastboot.img
```

Wait at least a minute and photograph the full settled **TX3 AND ELF RETRY** panel. It distinguishes current transaction3 writer pressure and the retry boundary, without another scheduling change. No automatic device action is performed.
