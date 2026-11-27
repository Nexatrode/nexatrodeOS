# IMG_9806: settled prerequisites complete; ELF waited after queueing

Settled **READY STARTUP OUTCOME**, delivered source `bea6daab0717060146353fee46683362f33ce55b`, image SHA256 `2f9d01e0911e7a9d0e88a825bb6d5f3730ce68f5a6a34137b56a684a62ec4dd5`. All75 delivered artifact hashes reverified unchanged. Raw HEIC, full/readable PNGs, manual rows, executable decoder and result hashes are separately retained in the profile's `device-result/`. No device command ran during analysis. Error/detail0; close cleanup stage6, no pending MCFG close.

## Completed exchanges and limits

Live IPA flags1F, registration1/driver1/publications4/accepted response1, pendingFFFFFFFF, failed0, timeout0. This is current protocol-complete with no pending/queued/held response, separately from historical IPA1. There is one ignored unhandled request, ID23. The saved Nov2 investigation already identifies that request and the pinned Linux unmatched-handler behavior; this is not a newly discovered unanswered prerequisite. No filter installation or harmlessness in every environment is inferred.

ELF body match1/completed1 and exact host credit requested/published/completed1/1/1. The credit's publication indicator uses assigned intent, and its completion flag has no displayed timestamp. It is not a permanently retained credit at settlement.

Original peer1F WRQ/OACK/DATA `00BF688A / 00CA4825 / 00CCC615`; OACK publication followed acceptance by712.603ms. Final ACK phase4/block1, publication/completion/close `00D82FBA / 00D89438 / 00D9EDC4`. The ACK completed88.460ms before original close and13,282.282ms before ELF publication. DATA and final ACK are established; their arrival is not a new breakthrough relative to the already successful original transfer.

The first two retained ruleset records and both retained EFS records each show flagsBF, TFTP ERROR opcode5/code1, successful local server dispatch result0, publication and matching GLINK completion before observed close, no client ACK/ERROR. ERROR replies do not expect normal DATA continuation. Completion-to-close leads: ruleset1 1,024.091ms; ruleset2 3.714ms; EFS1 781.709ms; EFS2 695.075ms. Ruleset total4/omitted2 prevents generalizing to the two undisplayed records. EFS total2/omitted0. These are retained request-specific observations, not joins by endpoint alone.

## Where the delay moved

TX3 queue/publication/Core completion `012F7988 / 01337316 / 016BEB08`, queue to publication260.494ms. TX4 receive/queue/publication/Owner completion `014133E9 / 014481E1 / 0147225D / 016D8500`. Both frozen masks3FA4, TX3 calls3 and TX4 calls2. The existing converted-grant path supplied publication; neither new handoff trace nor new mask bits19/20 ran.

ELF receive/queue/publication/Owner completion `0148C377 / 015860CF / 01A34022 / 01A7FDCD`:

| Interval | IMG_9806 | IMG_9805 |
| --- | ---: | ---: |
| Receive→queue | 1,023.320ms | 2,062.219ms |
| Queue→publication | **4,906.835ms** | 130.251ms |
| Publication→Owner completion | 310.699ms | 991.661ms |
| Receive→Owner completion | 6,240.854ms | 3,184.131ms |

Every displayed ELF admission-wait counter is0. The large current delay is **after successful admission**, so the prior full-pool release fix does not explain it. These are different physical boots; they do not isolate renderer effects or establish a controlled scheduling regression. This panel does not show ELF writer selections, fitting-intent availability or time spent in callbacks, and cannot assign the4.907seconds to a particular control packet. Earlier retained ELF writer/queued-turn captures and source should be examined before adding another observer or modifying order.

## Post-ELF request and WLAN

First TFTP request accepted after exact ELF completion: OTHER PATH category6, RRQ1, peer4E,93bytes. The separately retained first-other category8/peer4E maps in `qcom_post_elf_mcfg.rs::category` to the exact `/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig` pathname. This identifies the path by its exact classifier plus matching observed peer; it does not give its server reply, arrival clock, retry or completion. The prior IMG_9728 boot also reached this path but first accepted post-ELF MCFG; do not assume identical order across boots.

No bound post-ELF MCFG record exists here; its fields are unavailable. This does not prove that no MCFG read occurred earlier or outside the capture. Read total14/mask2E includes ruleset/EFS/MCFG/other and no WLAN bit. RAM MCFG writes1/file1byte/complete reads0. Close observed/committed4/4.

PD7FFFFFFF, WLAN RRQ0, WLFW0. The useful result is narrowing the current settled blockers: the original ACK, live IPA, captured EFS replies and ELF credit all complete. It is not evidence that Wi-Fi is nearly operational. Next offline work is the4.907second post-admission ELF delay and the actual mbn_hw.dig server/client ending against saved native/Linux traces. Do not repeat the original eight-slot inventory, treat ignored IPA23 as new, or declare MCFG's unavailable row a new transport failure. No repeated cold boot is needed merely to decode this result.
