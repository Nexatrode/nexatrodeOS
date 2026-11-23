# IMG_9704: the conversion-bound reply turn ran on the Pixel

The settled **REGISTRY GRANT RETRY** panel corresponds to source `cb48831c` and image SHA256 `02e5a6886e055c95ee4934758398d47c41a45449534a71329d662c23a2a05e4b`. Original photo, preview/detail, all 27 fields, decoded masks/counters and arithmetic are preserved privately in `native-registry-converted-grant-turn-v1/device-result`. The original verified delivery manifest and source snapshots remain unchanged; this hardware result is recorded separately.

Runtime error is zero and OLD SNAP STATE is 2: the exact transaction 3 predecessor counters froze after full publication. OLD BYTES is 88. OLD MASK is **0x3FA4**, with bits 2, 5, 7, 8, 9, 10, 11, 12 and 13 set. The conversion was seen, the exact opportunity armed, its callback attempted and its DATA fragment successfully committed. Generic borrowed-attempt/commit and registry-priority publication bits corroborate it. This reply fits in one 256-byte fragment, and the frozen state confirms full publication. Own-return-pending, no-exact-candidate and already-used-at-conversion bits 14–16 are clear.

The unpublished window contains four publication calls, versus 81 in IMG_9703. The first-condition counts are one pending denial, one positive ACK without unadvertised backing, one fresh backing without an ordinary ACK-yield flag, and one selected borrowed turn. The remaining buckets are zero, and the counts sum to four. ACK SENT is one and the longest completed ACK age in this window is 279 ms. These are observations before transaction 3 publication: they do not imply that later grant traffic disappeared. The current snapshot does not retain the subsequent matching converted-grant advertisement/ACK timestamps.

No selected-reply FIFO-blocked, no-fit, outbound-intent waiting/request-selected or other-DATA-selected bit appears in these four calls. These statements apply to the captured window, not transaction 4 or every instant between publisher entries.

## What improved and what remains delayed

| Interval | IMG_9704 | IMG_9703 |
|---|---:|---:|
| Transaction 3 queue to publication | **317.864 ms** | 9,997.159 ms |
| Transaction 3 publication to processed matching RX_DONE | 10,902.755 ms | 746.063 ms |
| Transaction 4 receive to queue | **985.176 ms** | 7,281.835 ms |
| Transaction 4 queue to publication | 7,084.542 ms | 498.966 ms |
| ELF receive to queue | 7,870.011 ms | 7,785.291 ms |
| ELF queue to publication | 854.807 ms | 515.618 ms |
| ELF publication to processed matching RX_DONE | 93.498 ms | 92.971 ms |
| ELF receive to processed matching RX_DONE | 8,818.316 ms | 8,393.880 ms |

The registry reply's local queue wait is 9,679.295 ms shorter, and the new mask independently proves the tested path ran on hardware. Separate-boot timings are not controlled causal benchmarks, but this is a concrete successful scheduler outcome rather than another unexercised opportunity.

Transaction 3 publication preceded transaction 4 receipt by 2,523.195 ms. Transaction 4 queued 3,508.371 ms after transaction 3 publication and published 309.842 ms before transaction 3's matching completion was processed, retaining the two-outstanding-reply behavior. Transaction 3's completion therefore was not a prerequisite for transaction 4 publication.

ELF queued 191.777 ms after transaction 3's completion. This is consistent with the existing two-reply bound while transactions 3 and 4 were still outstanding, but does not prove every earlier ELF prequeue exclusion. The early transaction 3 publication did not carry through to an earlier end-to-end ELF interval. Do not expand the outstanding window or bypass its credit/receipt rules from this timing alone.

OLD DONE is when the host processed the matching GLINK RX_DONE. This panel has no peer-tail or first-RX-visibility sample for this reply. It cannot distinguish late modem reading, late buffer return, a completion already queued behind other RX records, or delayed host parsing. Nor does it retain transaction 4's publication-pressure counters or the fitting-intent/credit state during its seven-second transmit wait.

ACK PHASE remains 4 and IPA COMPLETE remains 1. WLAN RRQ RX and WLFW SERVERS remain zero: no WLAN firmware request or WLFW announcement was observed by these counters. ELF is a locator reply, not firmware loading or proof of working Wi-Fi.

## Next offline target

Review and replay the path after the successful early transaction 3 write: its matching converted-grant advertisement/ACK, transaction 4's admission and writer arbitration, and transaction 3's matching completion. Use the saved traces and current source to distinguish unadvertised grant work, missing fitting modem intents/credits, pending receive returns and receive-backlog effects before changing policy again. If hardware observation is still needed, reuse the existing per-registry publication counters for transaction 4 and the existing FIFO visibility machinery for the exact transaction 3 receipt, rather than repeating the already-proven transaction 3 queue-delay probe. Preserve the two-reply bound, grant ordering and validated FIFO/credit paths.

No additional Pixel command is needed merely to interpret this result or begin that offline review. No device command was executed while recording it.
