# IMG_9798: ELF publishes on its first pass; delay is before queueing

Settled **POST EFS ELF WRITER**, associated with the preceding manual boot of `native-post-efs-elf-writer-v1`, revision `775428a28d816ce4722e4d143e67c7ff7888ed9a`, image SHA256 `6dfd30b5d7be11fa4656bab14a70de0f390dde803f7b3b02ed673c2459868aed`. Association uses the command/panel identity, not an independently measured handset image hash. Raw HEIC, readable crops and decoded rows are saved separately in the profile's `device-result/`; delivered artifacts and verification manifest remain unchanged.

## The queued writer stall did not recur

ELF accepted/queued/published/completed clocks are `014F4691 / 01F1D3B7 / 01F2C33A / 01F36FDB`.

| Interval | IMG_9797 | IMG_9798 |
| --- | ---: | ---: |
| Accepted → queued | 790.295 ms | **10,652.966 ms** |
| Queued → published | 4,998.402 ms | **61.315 ms** |
| Published → matching Owner completion | 309.557 ms | **44.193 ms** |
| Accepted → completion | 6,098.254 ms | **10,758.474 ms** |

The current queued interval is 4,937.087 ms shorter. Total accepted-to-completion is **4,660.220 ms longer**, because prequeue increased 9,862.671 ms. This renderer-only boot does not establish a controlled speedup or explain the previous queued stall. The useful current target is the wait before normal queue admission, not another attempt to accelerate this clean writer pass.

There is one writer entry, one timed completed record, zero omissions, finished1/active0/regressed0. Queue-to-first-entry is **24.502 ms**. Entry/end `01F2336D / 01F2C32B` span 36.798 ms by age subtraction; direct span `00008FBD` is **36.797 ms**. The 1 us difference is timestamp conversion rounding. Last observed end-to-publication is 15 us. MAX GAP is unavailable because there is no second pass, not evidence of a stall.

Record `09430005 / 00000000 / 00000001` means GLINK DATA9, registry-reply kind4, fitting-buffer and exact-target-selected flags3, transaction5, remote intent0, actual writer callback committed1. Publication calls1/maskA0/transaction5/bytes58 establish the successful exact 88-byte registry reply and existing priority path. Priority gate7(selected)=1, grant gate0(no pending ACK)=1. All other recorded priority/grant gates, ACK sends/blocks, receive returns ahead, and advertisements ahead are zero. These observations cover the queued ELF window, not earlier startup or the 10.653-second prequeue interval.

The ordinary queued handoff entered `01F1D3B9`, ended `01F69304`, flags1F, publication-progress count4. It saw exact ELF publication before returning. Its full **311.115 ms** span overlaps ELF publication/completion and other normal work; it is not 311 ms of exclusive ELF-writing cost, nor four ELF replies. No callback-backpressure, uncertain write, missing fitting buffer, active unmatched entry, omitted history, or pending grant ACK is demonstrated in the one captured pass.

## EFS, post-ELF requests, and live IPA

Only the first EFS entry is captured in this boot. It published `00E6D8CC`, completed `00EB66C8`, and closed `00F5810D`, flagsBF. Completion follows publication by **298.492 ms** and precedes close by **662.085 ms**. EFS2 clocks are absent. PAIR33 means enabled, first captured, original transmit bound, peer closed; second/exact-match/second-queued bits are absent and offer/admission counts are zero. Thus this boot did not exercise a second same-peer overlap; those absent fields do not demonstrate a regression of IMG_9797's successful pair.

ELF reference-body match1 and host-credit mask7 remain present. At the settled sample live IPA flags1F mean modem ready, uC ready, indication requested/sent, and protocol complete; failed/pending-record/record-queued/held-response/initial-boot flags are clear. Pending message ID is absent. This rules out a still-visible held IPA response at the settled sample; it does not reconstruct whether IPA work contributed earlier.

The first captured TFTP request after matching ELF is **OTHER PATH**, RRQ opcode1, modem port **0x50**, **93 bytes**. This panel does not display its filename, so matching it to an earlier digest request from length alone would be an inference. No postmatching-ELF MCFG target is captured. That does not rule out earlier MCFG requests; the global read counter is15/kind mask2e, and locator queued/completed-name masks are75.

Original peer0x1f's final ACK completed at `00D99729`, well before matching ELF completion. Runtime error/detail0, no pending MCFG close, cleanup stage6. Historical IPA complete1, host PD state7FFFFFFF, **WLAN firmware RRQ0 / WLFW servers0**. No new WLAN milestone is observed.

## Next offline target

Retain the working EFS guard and normal ELF writer path. Reconcile the **10.653 seconds before ELF queueing** with the saved prequeue phase/predecessor/full-slot notes and the existing retained admission/receipt captures before choosing another change. This screen cannot identify which specific older reply, capacity/credit gate, completion, or service gap held admission. In particular, the slot owners from IMG_9725/9726 are historical evidence, not the current boot's slot identities.

The successful Linux journal still places WLAN firmware before the later MCFG read; faster ELF publication alone has not produced WLAN here. This result supports narrowing the current investigation to prequeue startup/admission while keeping the missing firmware-private launch condition explicit. No additional Pixel command or rerun is needed to decode this result. No new diagnostic image or device action was performed while recording it.
