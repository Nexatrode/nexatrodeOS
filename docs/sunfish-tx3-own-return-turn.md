# Bounded original TX3 receive return before a fresh grant advertisement

IMG_9812 reproduced the already documented IMG_9804 publication-pressure pattern: TX3 waited12,614.991ms over128 writer observations, including105 pending-control exclusions and13 own-return exclusions. Its actual reply was consumed/returned promptly after publication. The successful converted-grant path in IMG_9811 and the ordinary ready-reply path remain distinct. The current photo does not reveal all simultaneous prerequisites during each earlier positive-grant window.

## Offline characterization

The existing ready DATA loan requires the queued reply's own real RX_DONE first. A normal positive fresh grant can take its advertisement/ACK while that return is still pending. In a real Core replay with a fitting modem intent and the original TX3 reply queued, the proposed first RX_DONE expectation fails on the parent: the writer selects ReceiveIntent instead. The red log is retained privately. A full-slot replay also shows a normal Control return converting a denial into a fresh grant while TX3's own return remains pending and no converted-registry candidate exists.

Feature `sunfish-tx3-own-return-turn` supplies one original request-return attempt in that narrow window. Successful replay order becomes **own RX_DONE → existing ready reply → fresh advertisement → positive ACK**, with matching original reply completion. This is a finite safe opportunity, not proof that all these prerequisites existed in IMG_9812 or that this removes all12.615 seconds on hardware.

## Authority and ordering

A private Boolean on each transmit starts false. The candidate is an unsent live ServiceRegistry transaction3 reply from the local registry port to node0/port9, with the existing queue/return-ready marker, its original physical wire receive slot still ReplyQueued and done-pending, and no used own-return, grant-DATA or converted-credit loan. The ordinary Session generation/owner and routed queue validation remain authoritative.

Only an ordinary publisher call may choose it. Protected TFTP/WRQ work, other already selected priority DATA, registry cooldown and any active converted-grant token veto it. The peer grant must be positive and have its exact backing still Advertising. A reused or already advertised backing cannot lend this turn. The original reply must be the normal unprotected class0 DATA candidate, retaining normal order, fitting modem intent, outbound-intent-wait and same-flow predecessor checks. A ready class1 QRTR credit vetoes the new return. Existing protected WRQ and IPA registration returns remain ahead of this candidate.

The new choice enters the normal Commit::Done path; no special serializer, hardware writer, receive loop or direct slot release is added. Its private attempt latch is consumed before the checked callback. A blocked callback cannot reborrow on the same or a later grant; normal advertisement/ACK and ordinary return/reply fallback remain responsible for progress. Uncertainty or unwind quarantines. A successful return uses the existing host-entry migration and sealed request identity. The next ordinary call may use the existing checked ready-reply DATA opportunity only if all its live guards still hold. No modem intent is reserved speculatively, and intervening protected work can still defer that reply.

Advertisement always precedes the matching positive ACK. No FIFO/native source validation, reply bytes, credits, published-buffer lifetime, queue capacity, firmware provider or close behavior is relaxed. Normal matching GLINK receipts still retire the original transactions. The private latch clears with normal transmit construction/reset and survives blocked writes and fresh grants. No new optional clock call or transport I/O is introduced; only existing checked publications are rescheduled.

## Evidence and panel

Core schedules cover clocked/clockless success, full-slot denial conversion, blocked return fallback, blocked DATA after successful return across another fresh grant, negative ACK, ready credit, missing fit, transaction4 exclusion, bounded-followup exclusion, uncertain/unwound callbacks and reset. A real manifest-backed88-byte `avs/audio` response retains `msm/adsp/audio_pd` and original transaction3. Real Adapter/FIFO schedules verify exact RX_DONE/DATA/advertisement/ACK bytes and original completion, plus uncertain notification quarantine without false reply publication. Parent fixtures that require advertisement retry first explicitly spend the new return attempt with a blocked writer; they retain their original assertions. Two functional removal controls independently disable the actual choice and the spent latch. Inherited receipt and earlier policy controls are rerun.

The title is **TX3 OWN RETURN TURN**. TX3 MASK bit21 (`00200000`) is the new original-return callback attempt; bit22 (`00400000`) is its successful write. Existing DATA attempt/commit bits19/20 remain independent. All four contribute `00780000`; other mask bits may also be present. Actual callback masks are distinct from candidate counters.

Rows9 and25 replace the displayed first converted-grant snapshot and ready-handoff summary with all eight new first-failing counters. Those original producer observations remain retained. Counters are recorded only when the ordinary choice point is reached, not on earlier publisher exits:

| Gate | First failure or candidate |
| --- | --- |
| 0 | Bounded/nonordinary call |
| 1 | Protected work, priority DATA, registry cooldown or active conversion token |
| 2 | No positive pending grant ACK |
| 3 | No exact unadvertised fresh backing |
| 4 | No eligible original TX3 pending return, or its opportunity already spent |
| 5 | Normal DATA order, fit, outbound wait or flow-predecessor selector differs |
| 6 | Ready QRTR credit |
| 7 | Candidate available; actual callback still shown separately by mask |

Both reply timelines, all existing priority/grant gates, exact FIFO tail/visibility/Core/Owner boundaries, bounded history loss, digest identities/outcomes, original final ACK and live IPA/WLAN/WLFW state remain visible. Counters and masks never authorize publication. The new helper reads only private live state and normal selectors; its observation update is separate.

Removing marked child blocks and its Cargo feature recovers parent production source `7dee737a` exactly, with one explicit lexical rejoin of the unchanged priority-return chain. Full current and parent Core, selected Owner/IRQ, actual physical panel and older source checks are required. Native stack,304 capability mutations, exact committed rebuild, complete boot payload and pinned-SDK/Normal-NC reproduction bind delivery. The SDK is reused. Selected stack bounds and finite schedules do not establish every interleaving, handset deadline or a working WiFi milestone.
