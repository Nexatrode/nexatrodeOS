# IMG_9701: repeated intent-control traffic held registry publication

The settled REGISTRY TX3 PRESSURE panel matches source `6a7eb5bf` and image SHA256 `6e5743d5477b30e39da086bea37ee9c7ea2e8d4aa56df922bbb697fdd7e60f15`. Original photo, preview/detail, all27fields, unpacked counts and arithmetic are saved privately in `native-registry-predecessor-pressure-v1/device-result`. The original verified delivery manifest and source snapshots remain unchanged.

Runtime error is0. OLD SNAP STATE2 confirms the exact transaction3/peer0:9 publication counters froze after full publication and survived matching RX_DONE. OLD BYTES0x58 is88bytes. This is that reply's complete unpublished-window history, not a final pressure sample of a later reply.

## Writer evidence

| First applicable registry-priority gate | Calls |
|---|---:|
| Cooldown | 0 |
| TFTP priority | 1 |
| Pending intent acknowledgement | 78 |
| Own receive return | 0 |
| No registry candidate | 0 |
| Another registry candidate | 0 |
| Ready QRTR credit | 1 |
| Selected registry priority | 1 |
| Total | 81 |

OLD CALLS0x51 is81 and the gate counts sum exactly to81. The ACK-window event counters record26successfully committed intent acknowledgements,26host receive-buffer returns (RX_DONE) and26receive advertisements, with zero selected intent-ACK FIFO WouldBlock outcomes. Those78actual control commits account for the78ACK-gated calls. They do not identify one-to-one triples, request sizes, intent IDs or whether every acknowledgement granted rather than denied a request.

The longest completed intent-ACK age is0x1BD=445ms. This identifies repeated intent-control traffic rather than a single acknowledgement stuck for the full10seconds. These are GLINK intent acknowledgements, distinct from the final TFTP ACK and the modem's matching completion of transaction3.

OLD MASK0xAC sets control-selected, other-DATA-selected, reply-written and registry-priority-committed bits. No-suitable-intent, outstanding outbound-intent-request, selected-reply FIFO-blocked and outbound-intent-request-selected bits are clear. The writer therefore observed a suitable remote intent on every captured call. There is no evidence here for the no-fit replay's missing-buffer mechanism or a selected transaction3/intent-ACK write blocked on FIFO space. These observations do not exclude gaps before publisher entry or conditions between calls.

The first-gate ordering matters: pending ACK is checked before own RX_DONE, candidate and credit checks. Their lower gate counts cannot prove they were simultaneously clear during all78ACK-gated calls. Counts/event classes are not an elapsed-time partition, and the10.053seconds can include receive processing, full validation and service gaps.

## Timeline

| Observation | Hex microseconds |
|---|---|
| Transaction3 queue | 012B13EE |
| Transaction3 full publication | 01C47A95 |
| Transaction3 matching completion | 01CFEB93 |
| Transaction4 receive | 01514617 |
| Transaction4 queue | 01C552C6 |
| Transaction4 publication | 01CCF944 |
| ELF receive | 015D2FBF |
| ELF queue | 01D43488 |
| ELF publication | 01DC1CC4 |
| ELF matching completion | 01DD89C7 |

Transaction3 queue-to-publication is10053.287ms and publication-to-completion749.822ms. It queued2503.209ms before transaction4 receipt. Transaction4 receive-to-queue is7605.423ms, with only55.345ms from transaction3 publication to transaction4 queue. Transaction4 publication precedes transaction3's matching RX_DONE by193.103ms, preserving the two-outstanding-reply result.

ELF receipt-to-queue is7800.009ms, queue-to-publication518.204ms and publication-to-completion93.443ms; total8411.656ms. Relative to IMG_9700, ELF total is769.368ms shorter while transaction4 prequeue is27.340ms longer. Separate-boot comparisons are not causal speedup proofs; this image changed observation only.

ACK PHASE4 and IPA COMPLETE1 retain the prior accepted original final-DATA/ACK and IPA milestones. WLAN RRQ RX and WLFW SERVERS remain0. ELF completion remains a locator response, not a WLAN firmware transfer.

## Next offline target

The scheduler unconditionally excludes registry priority whenever an intent acknowledgement is pending. A just-completed ACK sets the existing intent-ACK yield flag, but a subsequent incoming grant request can again exclude registry priority; ordinary control work can consume that yield. The frozen record now identifies that repeated competition on the actual Pixel, rather than leaving no-fit and control pressure indistinguishable.

Replay a single bounded registry reply opportunity between completed intent-ACK cycles. Keep its own receive-return prerequisite, actual fitting intent, FIFO/route and outstanding-reply order, protected TFTP/WRQ turns and ready QRTR credits. For a fresh positive grant, any borrowed turn must precede its backing advertisement; once advertised, its positive ACK retains the existing order. Consume the special opportunity before a blocked write, using live per-transmit state rather than diagnostic masks, clock values or counters. Test normal fallback, denial/positive cases, same-peer receipts and uncertain faults before changing the native image. Do not merely add an unrestricted service-loop writer drain or skip grant/credit work.

The current panel is sufficient to select this next offline replay; no new Pixel command is needed now. It does not prove the next scheduling change will launch WLAN. No device command was executed while recording this result.
