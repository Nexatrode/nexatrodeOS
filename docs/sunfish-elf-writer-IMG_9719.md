# IMG_9719: exact ELF publishes in its first writer pass

Settled **ELF WRITER PUBLICATION**, source merge`b3fbfad0` (WiFi change`f42cc9f6`), image SHA-256 `2918e000de75c0d9f2051f8cdb18ba0b6ed02329c8e5e33f670c4bd247066870`. Raw photo and decoded rows are retained in the private delivery profile's separate `device-result/`. The verification manifest is unchanged. Runtime error0.

## The four-second queued stall did not recur

ELF receive/queue/full publication/Owner completion are `0149AD91 / 017AE836 / 017C5572 / 0185DBA1`.

| Interval | Current boot |
| --- | ---: |
| Receive → queue | **3,226.277ms** |
| Queue → publication | **93.500ms** |
| Publication → matching completion | 624.175ms |
| Receive → matching completion | 3,943.952ms |

IMG9718's queued interval was4,139.576ms and total6,845.265ms. Current observation-only capture cannot establish why that earlier stall happened or claim a controlled performance fix. The current result establishes that the writer can publish promptly under this boot's conditions. The delay before queueing remains larger than the queued/publication interval.

Queued-turn entry/end `017AE838 / 017EA6C9`, flags**1F**. Exact ELF publication is inside that ordinary dispatch. It continued after publication before returning; its245.393ms full span must not be assigned entirely to ELF's write.

There is exactly **one writer pass**, one timed interval, no omitted records and no unmatched active entry. Entry/end `017BC540 / 017C5564` span**36.900ms** (`9024`). The direct queue→first-entry field is56.585ms (`DD09`); subtraction of displayed ages gives56.586ms. The14us end→publication interval completes the93.500ms displayed-age total. The1us conversion difference is expected. MAX GAP=FFFFFFFF is unavailable because there is only one pass, not evidence of a gap or stall.

Record`09430005 / 00000003` decodes to GLINK command9, registry-reply kind4, flags3 (fitting intent and exact target selected), QMI transaction5, remote intent3. Neither callback-blocked nor uncertain flag is set. Existing publication calls1/maskA0/transaction5/bytes58 establish a successful DATA callback and existing registry-priority publication. Turn gate7(selected)=1; every other priority gate is0. Grant gate0(no pending ACK)=1, all other grant gates0. No ACK sends, blocks, receive returns ahead or advertisements ahead were recorded while this exact reply was unpublished. This does not imply that no earlier grant existed.

## The first-slot ordering remains correct

SLOT TURN1, admitted sequenceA/count1/last receipt3. Exact Core receipt3`01794FBD`, admission`017AE82F`, receipt4`017B6537`. ELF admission is104.562ms after receipt3 and32.008ms before receipt4, retaining one original owner. The writer starts24.585ms after receipt4 is processed. Admission before the second receipt and ordinary FIFO processing of that second receipt before publication are distinct facts; this does not recreate an unconditional two-completion admission requirement.

## MCFG and WLAN outcome

Current first postmatching-ELF MCFG target is peer4A, stagesBF, native server result0, reply opcode**5 (TFTP ERROR)**. BF establishes received/dispatched/nonempty reply/queued/published/completed/closed; the client ACK/ERROR bit is absent. Server result0 means the handler returned a prepared reply normally, **not that the read succeeded**. The actual error code, request options, and file state at that dispatch are retained by the underlying capture but not displayed on this writer-focused panel. They must not be inferred from opcode5 or from final file state.

IPA COMPLETE1; PD state7FFFFFFF; **WLAN RRQ0, WLFW0**. MCFG remains one completed write, one committed byte and zero completed full reads. The saved Linux comparison already shows a missing MCFG read after WLAN firmware requests began, and stat-like reads need not increment the full-read counter. This ERROR alone is not a demonstrated WLAN root cause or reason to invent/prepopulate a file.

No rerun is needed to establish the clean current writer path. It does not explain IMG9718's unreproduced queued stall. The next offline review should distinguish the remaining prequeue wait and the actual post-ELF error/startup sequence using existing notes and retained producer fields, rather than treating this boot as another blocked writer. No device command or new build was performed while decoding this result.
