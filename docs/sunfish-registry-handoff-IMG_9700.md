# IMG_9700: no handoff attempt; predecessor publication dominates

The settled REGISTRY HANDOFF panel matches the delivered `6753bb61` profile, image SHA256 `b37b1f8961519668ed508b7d664cd78b1b484d104481f2823b65362653353d34`. The original photo, detail crops/preview, complete27-field transcription and arithmetic are retained privately in `native-registry-handoff-v1/device-result`. The verified delivery manifest and source snapshots are preserved.

Runtime error is0. HANDOFFS and HANDOFF MATCH are both0; all bounded-writer and retry fields areFFFFFFFF. The scheduling helper did not consume a candidate on this boot. Its absence is not a stalled retry or a failed publication. The earlier two-reply window still admits two replies and PUB BEFORE DONE remains1.

| Interval | IMG_9699 | IMG_9700 |
|---|---:|---:|
| Transaction4 receipt to queue | 8165.430ms | 7578.083ms |
| Transaction4 queue to publication | 498.374ms | 500.529ms |
| Transaction4 publication to completion | 1265.724ms | 635.138ms |
| ELF receipt to queue | 9158.170ms | 8090.642ms |
| ELF queue to publication | 204.008ms | 430.081ms |
| ELF publication to completion | 3800.053ms | 660.301ms |
| ELF receipt to completion | 13162.231ms | 9181.024ms |

Transaction4 queued587.347ms sooner relative to receipt. ELF completed3981.207ms sooner relative to receipt. Its queue-to-publication phase was226.073ms longer; its publication-to-completion phase was3139.752ms shorter. These are separate boots, not controlled causal benchmarks. In particular, none of the improvement can be credited to a handoff that did not run.

| Observation | Hex microseconds |
|---|---|
| Transaction3 queue | 012A6268 |
| Transaction3 full publication | 01C34F3B |
| Transaction3 matching completion | 01CEBB18 |
| Transaction4 receive | 01508515 |
| Transaction4 queue | 01C426F8 |
| Transaction4 publication | 01CBCA29 |
| Transaction4 completion | 01D57B2B |
| ELF receive | 0158E2DD |
| ELF queue | 01D456EF |
| ELF publication | 01DAE6F0 |
| ELF completion | 01E4FA3D |

Transaction3 spent10022.099ms queued before its full publication. It queued2499.245ms before transaction4 was received and3047.541ms before ELF was received. Of transaction4's7578.083ms prequeue interval,7522.854ms preceded transaction3 publication; transaction4 queued only55.229ms afterward. Publication of4 preceded matching completion of3 by192.751ms, so transaction3 RX_DONE was still not required before admitting/publishing4.

This arrival order explains a concrete trigger gap in the implementation: `dispatch_service_registry_bounded` only arms a candidate after a successful first queue, with the exact ELF request already pending, followed immediately by a waiting second dispatch. Transaction3 queued well before the later4/ELF requests existed in the owner's accepted queue. When a later call encounters4 waiting behind already-queued3, its first result is WaitingReply and it returns without arming. The offline fixtures that queued all three requests before dispatch did not reproduce this staggered physical arrival order. The trigger was too narrow for this boot.

The source condition and observed order establish that missing opportunity; the photograph does not identify which controls, grants, credits, FIFO conditions or normal service work occupied transaction3's long publication interval. It would be incorrect to attribute all10.022seconds to any one of them or to bypass their ordering/validation rules.

ACK PHASE4, IPA COMPLETE1 and WRITES COMPLETE1 preserve the prior original DATA/final-ACK and IPA milestones. WLAN RRQ RX and WLFW SERVERS remain0: no WLAN firmware request or WLFW startup was observed. ELF completion is still a locator response, not a firmware transfer.

Next offline replay should receive/queue3 first, then deliver4/ELF on later passes while3 remains unpublished. Check whether a bounded candidate can recognize that exact live older/waiting pair without requiring same-pass queueing, while consuming the opportunity only once and retaining all grant, credit, FIFO, close/fault and ordinary selector checks. Do not re-arm an attempted pair on every waiting service call. Capture the predecessor's actual publication pressure before deciding what it can safely advance. No further Pixel command is needed to interpret this result; no device command was executed while recording it.
