# IMG_9802: early transaction3 publication, then ELF transmit-capacity pressure

Settled **TX3 AND ELF RETRY**, associated with the preceding manual boot of `native-tx3-elf-retry-v1`, integrated source revision `6923ea276f316eb9acbc8dcfa069ed8bd3937448`, image SHA256 `eb6670d9a20d5bfdd296defe35a29520993b1d634b490e4653fc483468fe9d98`. Association uses the panel and preceding instruction, not an independently read handset image hash. Raw HEIC, readable crops and all decoded hexadecimal rows are retained separately in the profile's `device-result/`. All256 delivered artifact hashes and sizes were rechecked; the frozen delivery manifest and artifacts were unchanged. No device command or new build was performed while recording this result.

## Transaction3 used the existing converted-grant reply turn

| Boundary | Hexadecimal clocks | Interval |
| --- | --- | ---: |
| Transaction3 queue → full publication | `012F9298 → 0133948B` | **262.643 ms** |
| Converted backing reserved → publication | `0132BC18 → 0133948B` | 55.411 ms |
| Publication → matching grant advertisement | `0133948B → 01346CC6` | 55.355 ms |
| Matching advertisement → grant ACK | `01346CC6 → 0135C918` | 89.170 ms |
| Publication → exact Core receipt | `0133948B → 017265C5` | 4,116.794 ms |

Frozen snapshot2 identifies transaction3,88 bytes,3 publication calls, mask`3FA4`. The existing bounded conversion/borrowed reply path actually committed: bits2,5,7,8,9,10,11,12,13 are set. No no-fit, outstanding-intent-request, selected-write-blocked or other-DATA bit is set. This is the already implemented path demonstrated in [IMG_9704](sunfish-registry-grant-retry-IMG_9704.md), not a new scheduling change in this diagnostic.

All3 first-failing ordinary priority observations name the pending intent ACK (gate2). Converted-grant gates report denial1, no unadvertised backing1, eligible/selected1; all others0. The ordinary priority selected counter being0 is compatible with a successful borrowed path and its publication mask. The unpublished window records1 ACK commit,1 receive return ahead,0 advertisement commits ahead,0 selected ACK WouldBlock and maximum completed ACK age267.384 ms. These are observations, not an elapsed-time partition. That earlier ACK is distinct from the exact converted grant's later ACK. The exact converted backing is`3B`; its advertisement still precedes its ACK.

Compared with [IMG_9801](sunfish-elf-prequeue-owners-IMG_9801.md), transaction3's queued interval is12,374.137 ms shorter (262.643 versus12,636.780 ms). This diagnostic changes observation overhead and the boots have different arrivals/pressure. It demonstrates that the long publication stall did not recur; it does not prove a newly fixed or repeatable performance improvement. Conversely, the current matching receipt arrives4.117 seconds after publication, so this boot cannot inherit IMG_9801's prompt25.838 ms return explanation. The current panel does not show when the modem read transaction3 or when its completion first became visible.

## ELF waited for the ordinary eight-slot pool, not an unpublished predecessor

| Transaction | Accepted | Queued | Published | Matching completion |
| --- | --- | --- | --- | --- |
| 3 | Not displayed | `012F9298` | `0133948B` | Core `017265C5` |
| 4 | `01442AB5` | `01454AFC` | `014A2598` | Core `0173BB30`, Owner `0174033D` |
| ELF5 | `014BCA0A` | `016B490E` | `016D4623` | Core `017C2418`, Owner `017C6C19` |

Transaction4 queues73.799 ms after acceptance, publishes318.108 ms later and completes2,743.717 ms after publication at the Owner boundary. Its independent frozen snapshot is2,4 calls, mask`3FA4`, transaction4/224 bytes. ELF arrives107.634 ms after transaction4 publication. Preselection calls/runs are0: the earlier unpublished-reply dispatch chain from IMG_9801 is absent here.

ELF waits by reason are`0 / 0 / 3 / 0 / 0 / 0`: **three transmit-capacity rejections**, no recorded same-peer, flow or credit rejection. There are4 admission attempts including success. The latest failure at`0168BBDC`, sequence3/reason2, sees2 older owners/gates1. Entries`00033B04 / 00000004` and`00043B04 / 00000010` identify transactions3/4, registry kind4, flags3B and remote intents4/10. Both are fully published with local QRTR retirement recorded and their remote slots/reference tokens retained; neither has its matching GLINK receipt yet. The existing bounded third-reply rule can admit alongside those2 eligible owners. The rejection here is the separate full transmit pool, before reserving a new reply credit.

Admission at`016B4908`, sequence4, still records2 owners/gates1, no prior registry receipt. It precedes transaction3's Core receipt by466.108 ms. The first registry receipt is sequence5 (3), then6 (4), then7 (target ELF5);3 receipts,0 omissions. The predecessor timestamp and journal sample differ by1 us at their separate Core observation sites. Transaction4 and ELF Owner timestamps follow their corresponding Core samples by18.445 and18.433 ms. These are separate boundaries, not contradictory identities.

Capacity-retry flags`F`, result1 show the existing bounded capacity retry armed, entered, returned and queued. The lifetime completion-slot Boolean is0, consistent with admission before the first registry receipt. The queued handoff spans93.479 ms with flags1F. These are different bounded opportunities and must not be conflated.

| ELF interval | Current boot |
| --- | ---: |
| Accepted → queued | **2,064.132 ms** |
| Latest capacity failure → successful Core admission | 167.212 ms |
| Queued → publication | 130.325 ms |
| Publication → matching Owner completion | 992.758 ms |
| Accepted → matching Owner completion | **3,187.215 ms** |

The total is7,535.061 ms shorter than IMG_9801's10,722.276 ms across separate boots. The last-failure interval is not the entire2.064-second prequeue span, and the three failure counters do not partition its time.

## Empty retry fields are expected for this boot

New retry flags0, missing blocker/slot/boundary times and zero copied progress counts mean the exact unpublished-peer-blocker receipt path was not exercised. Its observer intentionally binds that failure identity; an ELF capacity failure is not such a blocker. Therefore this boot neither reproduces nor explains IMG_9801's753.629 ms Core-receipt-to-admission gap. It would be incorrect to call these empty fields an instrumentation failure or claim the previous gap was eliminated by a fix.

## Startup result and next offline target

Runtime error/detail0, live IPA flags1F, no pending IPA response or MCFG close, cleanup stage6. Original final ACK completed at`00DA7BFB`. Read count16, kind mask2E; locator queued/completed masks75. Pair`0001017F` records one exact second-EFS overlap offer/admission with flags7F. This panel does not display the EFS completion/close clocks, so it does not establish both EFS deadlines were met.

**IPA completed; WLAN firmware RRQ0 and WLFW servers0 remain unchanged.** The earlier publication chain is substantially shorter in this run, but no WLAN startup milestone has appeared.

The next offline target is the current full transmit pool and its first legitimate release. `owner.elf_tx_slots()` already retains first/latest full-pool snapshots and first-release/admission metadata, although this panel does not display them. Reuse that bounded capture and review the normal retire/admission paths before adding a hook or changing scheduling. IMG_9725/9726's eight slot identities cannot be substituted for this boot. Transactions3/4 are still retained when ELF queues, so their later completions were not prerequisites for this admission. Published buffers must retain exact completion ownership; early speculative retirement is not justified by this result.

No additional Pixel command or rerun is needed to interpret IMG_9802. Another cold boot would be for a newly prepared question, not a repeat of the absent unpublished-peer retry episode.
