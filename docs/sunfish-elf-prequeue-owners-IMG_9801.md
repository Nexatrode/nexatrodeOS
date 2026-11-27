# IMG_9801: ELF waits behind transaction4 and transaction3's late publication

Settled **ELF PREQUEUE OWNERS**, associated with the preceding manual boot of profile `native-elf-prequeue-owners-v1`, revision `ebaf9d4d9c26559c4987db84e602785c9175723b`, image SHA256 `342e143a346c78f682aeeb1c52befbd974673b97bb58628489b54d22833dc9a2`. Association uses the panel/manual instruction, not an independently measured handset image hash. Raw HEIC, readable crops and decoded rows are retained separately in `device-result/`; delivered artifacts and manifest remain unchanged.

## The dominant interval is before exact ELF selection

ELF acceptance/queue/publication/Owner completion: `014ECCE4 / 01F0CDA7 / 01F1BC71 / 01F268C8`.

| Boundary | Current interval |
| --- | ---: |
| Accepted → receive-pass end (`015C2CDB`) | 876.535 ms |
| Receive end → first registry entry (`015C5CB8`) | 12.253 ms |
| Registry entry → first exact ELF selection (`01E3B21B`) | **8,869.219 ms** |
| Selection → normal queue success | 859.020 ms |
| Accepted → queued | **10,617.027 ms** |
| Queued → full publication | 61.130 ms |
| Full publication → matching Owner completion | 44.119 ms |
| Accepted → matching Owner completion | 10,722.276 ms |

This closely repeats IMG_9798's 10,652.966 ms prequeue and clean61.315/44.193 ms publication/completion intervals. Overall completion is only36.198 ms shorter across separate physical boots. It is not a meaningful speedup claim.

PRESELECT CALLS15 is21, omitted0. Run `00041214` decodes as transaction4, name code1 (TMS service-registry lookup), outcome2 FlowBusy, count20. Run `00041101` is the same transaction/name, outcome1 queued, count1. All21 recorded preselection dispatches therefore worked on transaction4, before exact ELF selection. These are20 FlowBusy outcomes, not20 displayed individual pressure reasons or an exclusive CPU-time partition.

## Transaction3 published late; its return was prompt afterward

| Transaction | Accepted | Queued | Published | Matching completion |
| --- | --- | --- | --- | --- |
| Existing predecessor3 | Not displayed | `0120E482` | `01E1B6EE` | Core `01E21BDC` |
| Transaction4 | `0142F048` | `01E3828F` | `01E4E8E1` | Core `01E54DC5`, Owner `01E59558` |

Transaction3 spends **12,636.780 ms queued before publication**. ELF has already been accepted for9,628.170 ms when that publication occurs. Transaction4 queues117.665 ms after it, then publishes91.730 ms after its own queue success. Transaction4's receive-to-queue span is10,523.207 ms. The normal same-peer rule forbids admitting another ordinary registry reply alongside an unpublished predecessor. This source rule and the chronology support the earlier unpublished predecessor holding up the transaction4/ELF chain. The current panel does not expose transaction4's individual pressure reasons, nor transaction3's frozen writer gates, so it cannot attribute every preselection FlowBusy outcome or identify what held transaction3 itself.

Transaction3's sampled TX consumption and first matching RX_DONE visibility both occur at `01E1CF32`, **6.212 ms after publication**, with zero bytes ahead. Its exact Core receipt follows19.626 ms after visibility,25.838 ms after publication. Visibility is a sampling bound, not exact remote emission or application acceptance. These fields do not support a long remote-return delay after transaction3 publication in this boot.

The row5 predecessor DONE clock is recorded in Core after exact completion matching. It is not the later Owner boundary used by row3 ELF and row6 transaction4; the panel documentation has been clarified. The receipt journal agrees with the predecessor clock here. Transaction4's Owner timestamp is18.323 ms after its Core journal receipt, and ELF's is18.324 ms later. These are different observation sites, not mismatched completions.

## Exact ELF has one actual peer rejection, not a full-pool rejection

First/latest failed admission are the **same** check: `01E3F9BD`, sequence2/reason1, count1/gates18. The single retained owner is `00042804`: transaction4, entry flags28, registry kind4, remote intent absent. Flags28 mean local QRTR retirement and reference token retained, with no started/full publication or remote slot. Gates18 mean unpublished bytes8 and no remote slot10. This directly identifies why that ordinary ELF attempt failed.

Receipt journal order is:

| Sequence | Event | Clock | Intent / earlier same-peer owners remaining |
| --- | --- | --- | --- |
| 1 | Transaction3 exact receipt | `01E21BDC` | Intent0 / remaining0 |
| 2 | ELF rejected behind unpublished4 | `01E3F9BD` | No assigned remote intent for4 |
| 3 | Transaction4 exact receipt; matches last failed owner | `01E54DC5` | Intent10 / remaining0 |
| 4 | Exact ELF admitted | `01F0CDA2` | Earlier owners0 / gates0 |
| 5 | Exact ELF transaction5 receipt | `01F22134` | Intent0 / remaining0, target flag |

Three receipts, zero omissions. There are two exact ELF admission attempts: the one rejection and success. No attempt is recorded between transaction4 publication and its receipt, or during the following **753.629 ms** from that Core receipt to admission. Queue time is5 us after the Core admission snapshot. It would be incorrect to label that entire later gap a continuing peer rejection, or infer that credits and all other guards would have passed at an untried earlier instant.

The normal-pool observer is absent, failure count0, all slot/free fields unknown. This means **no reason2 full-pool failure was captured for exact ELF**, not that all slots were empty or that capacity pressure never occurred elsewhere. The previous eight-slot owners from IMG_9725/9726 do not apply to this boot. One queued-window writer pass is retained; queue-to-publication remains fast.

## Startup outcome and next offline question

Runtime error/detail0, no pending MCFG close, cleanup stage6. Live IPA flags1F and absent pending message indicate a completed handshake with no pending/queued/held response at the settled sample. Historical IPA complete1, PD7FFFFFFF, **WLAN firmware RRQ0 / WLFW servers0**. Original final ACK completed at`00D97CB6`. Pair33 means one EFS request captured/queued original/closed, with no second request or overlap offer/admission; it is not a regression of IMG_9797's successful two-request result.

The next dominant target is **transaction3's12.637-second queued publication**, with the smaller post-transaction4 retry gap kept separate. The existing `RegistryPredecessorTrace` still retains its exact frozen publication/gate counters after completion. Earlier [IMG_9701](sunfish-registry-pressure-IMG_9701.md) found repeated advertisement/ACK/RX_DONE control work, but those counts cannot be imported as this boot's explanation. Review the retained current source/guard paths and earlier pressure/prearm notes before choosing a change or another display. No new probe hook is necessary merely to expose the existing counters.

This result identifies the current wait chain, but does not establish the private WLAN launch condition or working WiFi. No rerun or additional Pixel command is needed to decode it. No new image or device action was performed while recording these findings.
