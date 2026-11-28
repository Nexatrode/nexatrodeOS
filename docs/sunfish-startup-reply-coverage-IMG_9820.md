# IMG_9820: all four ruleset replies retained; the first dispatch and retry waits remain late

Settled **EARLY REPLY COVERAGE**, associated with the preceding manual boot of source `342bc9791c9f14705814be9188fe5d1c5e19d9be`, image SHA256 `7af5ec0a9b13d7620d96de3c0fff9a9594d9096cda1499e7405537ff557c755f`. Association uses the delivered instruction/title, not an independently measured handset hash. The original HEIC, readable crops, full numeric rows and executable decoder are in the profile's separate `device-result/`. All142 delivered artifact hashes were reverified unchanged; no device command ran during analysis.

## The omitted-exchange question is resolved

Ruleset total4/omitted0. Two requests are peer21, rsize4/options17/85bytes, reservations13/1A, host port5003; the other two are peer48, rsize absent/options0F/85bytes, reservations17/1B, port5004. Each offered timeout1000ms and produced ERROR1, flagsBF, with actual publication and exact GLINK completion. No client-ending bit40 is present. Matching numeric projections and peer do not prove complete request-byte equality.

| Exchange | Accepted → dispatch | Dispatch → queue | Queue → publication | Accepted → publication | Completion relative to observed close |
| --- | ---: | ---: | ---: | ---: | ---: |
| Ruleset1 | **1,565.416ms** | 18.341ms | 313.192ms | **1,896.949ms** | 2,634.725ms after |
| Ruleset2 | 1,430.900ms | **7,077.393ms** | 259.256ms | 8,767.549ms | 3,917.838ms after |
| Ruleset3 | 67.213ms | 18.322ms | 209.021ms | 294.556ms | 1,844.312ms after |
| Ruleset4 | 47.250ms | **2,438.419ms** | 352.289ms | 2,837.958ms | 2,879.520ms after |
| EFS1 | 279.548ms | 18.320ms | 390.949ms | 688.817ms | **659.989ms before** |

Ruleset2 admission has12 attempts,10 FlowBusy and1 NoCapacity; ruleset4 has7 attempts,6 FlowBusy and0 NoCapacity. Their earlier same-peer replies still own real flow tokens, so they cannot be retired based on close or a FIFO cursor. Both later replies publish after their peer closes. The first ruleset of each pair publishes before host-observed close (5,030.849ms and625.005ms respectively), yet completion follows close. This panel has no modem-consumption or exact receipt-visibility watch for these replies; it cannot identify whether those gaps are remote receipt generation or host receive processing. In particular ruleset3's294.556ms acceptance-to-publication does not establish the application's deadline or prove a missing-file response was accepted.

EFS total1/omitted0; there is no second EFS request in this boot's capture. Its first request is peer22, reservation12, port5002, ERROR1/74-byte RRQ, timeout1000ms, no admission wait, matching completion659.989ms before close. All-ones EFS2 rows are absent evidence, not an incomplete second exchange. This is a different request pattern from the earlier two-EFS boots.

## Other startup state

Original peer1F sent DATA and its final ACK completed88.679ms before close and10,204.096ms before ELF publication. TX3 calls5/mask007EE3AC includes all three bounded credit/own-return/reply stages. Queue→publication383.480ms; publication→Core5,197.718ms. Do not copy other boots' peer-tail/visibility bounds into this interval.

ELF receive→queue3,571.799ms, queue→publication135.774ms, publication→Owner661.768ms; total4,369.341ms. Body match1, completion1 and host credit7. Modem tail crossing and the matching receipt are first observed together at017555BB,6.160ms after publication. No preceding absent sample exists;40 bytes precede the matching receipt. Core processes it637.291ms after first visibility; Owner follows18.317ms later. This is transport consumption/return, not application acceptance. Most of the current ELF completion interval is after visibility, but no exclusive CPU attribution follows.

Runtime error/detail0, cleanup stage6,13 close observations/commits. Live IPA flags1F, registration1/driver1/publications4/accepted response1, no pending message. RAM MCFG1write/1byte/0 completed reads. First other path8 is digest. Historical IPA1, PD7FFFFFFF, **WLAN RRQ0 / WLFW0**. No new WLAN milestone appeared.

## Next offline target

The new capture resolves the previously omitted ruleset lifecycles. It does not support another MCFG-priority probe or a speculative same-peer release. Existing EFS code has an asymmetric bounded handoff: after the first ruleset queues, it may service the first oldest EFS request. EFS arrived first in this boot, so that condition cannot give the subsequently accepted first ruleset an equivalent extra service opportunity. Source establishes this asymmetry, not that it exclusively accounts for the1,565.416ms dispatch delay.

Replay the reverse arrival order and consider a separately enabled one-shot ordinary service step for the exact first ruleset, only after the original EFS ERROR really queued, with no retained reply/close ahead and only if the same handle is still the oldest pending TFTP request. Preserve all normal admission/validation/grant/credit checks and real ownership. A bounded ordinary writer group at that exact normal queue admission can be tested separately with fresh RX veto, blocked-write fallback and uncertain-fault containment. Do not broaden EFS duplicate admission to ruleset merely from the numeric projection. A hardware run would measure whether the opportunity actually runs and reduces the original delay; it cannot promise WLAN startup.
