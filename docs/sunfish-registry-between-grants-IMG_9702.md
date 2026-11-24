# IMG_9702: bounded transaction3 turn did not reach the writer

The settled **REGISTRY BETWEEN GRANTS** panel corresponds to source `eef7ff6b` and image SHA256 `1cab2be8115886d6be9e406a0f45ecfb006486ed7c8bf01e219ec266507e06fd`. Original photo, preview/detail, all27fields and arithmetic are preserved privately under `native-registry-between-grants-v1/device-result`. The verified delivery manifest and original source snapshots remain unchanged; the physical result is separate.

Runtime error is0 and snapshot state2 confirms the exact transaction3/peer0:9 counters froze at full publication. OLD MASK is0xAC. Both new borrowed-attempt bit8 and successful-borrowed-commit bit9 are clear: the extra opportunity never reached the checked writer for this reply. This is not evidence that a borrowed write was attempted and blocked. It also does not show whether another registry transaction borrowed a turn later.

The88-byte reply recorded91publication calls. First-condition counts are0cooldown,2TFTP,87pendingACK,0own return,0no candidate,0other registry,1ready credit and1selected registry priority; they sum to91. The existing ACK-window counters record29ACK commits,29host receive returns and29advertisements, accounting for87control commits while ACK was pending. None of the selected intent-ACK writes returned WouldBlock. The longest completed ACK age was3207ms.

Mask0xAC retains control-selected, other-DATA-selected, this-reply-written and registry-priority-committed observations. No suitable-intent failure, outbound-intent request waiting/selected or selected-reply FIFO-blocked bit is present. A fitting modem receive intent was observed on every captured publication call, not necessarily every instant between calls.

| Interval | IMG_9702 | IMG_9701 |
|---|---:|---:|
| Transaction3 queue to publication | 10881.687ms | 10053.287ms |
| Transaction3 publication to matching RX_DONE | 1554.575ms | 749.822ms |
| Transaction4 receive to queue | 8168.093ms | 7605.423ms |
| Transaction3 publication to transaction4 queue | 55.036ms | 55.345ms |
| Transaction4 queue to publication | 498.623ms | 501.374ms |
| ELF receive to queue | 9391.275ms | 7800.009ms |
| ELF queue to publication | 204.413ms | 518.204ms |
| ELF publication to matching RX_DONE | 3800.464ms | 93.443ms |
| ELF receive to matching RX_DONE | 13396.152ms | 8411.656ms |

Transaction3 queued2768.630ms before transaction4 arrived. Transaction4 publication preceded transaction3's matching completion by1000.916ms. The earlier two-outstanding-reply result remains intact. These are separate boots, not controlled causal benchmarks: this boot did not improve the overall outcome, while the exception being tested did not run for the captured predecessor.

ACK PHASE4 and IPA COMPLETE1 retain the original final-ACK GLINK completion and IPA milestones. WLAN RRQ RX0 and WLFW SERVERS0 show no WLAN firmware request or WLFW announcement observed by these counters. ELF completion is a locator reply, not firmware loading or working Wi-Fi.

The new exception requires a completed previous intent ACK's yield flag, a pending positive ACK and a still-unadvertised backing intent, plus the existing registry eligibility checks. Every successful non-ACK publication clears that yield flag. The panel does not capture ACK polarity, backing state or that flag. Its ACK-first gate precedence also masks lower simultaneous conditions, including the reply's own receive return and ready credits. Consequently the absence of the new mask bits identifies a missed opportunity, not the predicate that missed it. The29/29/29counts do not identify one-to-one positive-grant cycles.

Next work is an offline examination of the ACK/receive-return/next-grant sequence and the safe opportunity's lifetime. Reuse the existing replay's own-return and already-advertised-grant controls; do not claim they establish the handset's precise failed predicate. No unrestricted drain, skipped FIFO validation, ACK-before-advertisement or ignored credit is justified. No new Pixel command is needed just to interpret this photo. No device command was executed while recording it.
