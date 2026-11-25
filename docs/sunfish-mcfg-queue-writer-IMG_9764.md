# IMG_9764: an unsent predecessor, then a receive-return writer turn

Settled **MCFG QUEUE WRITER**, prescribed revision `02c4de4e26adbdf09cd85b0d83a9626931c0d1f8`, image `0a541f4585cb96ab6ea920897fc92e0964ece49cf04670cd4d20500ea281466b`. The title matches the prescribed command; the photo does not independently measure the image hash. Raw photo, preview and decoded scalar rows are saved under the profile's separate `device-result`; its delivered manifest is unchanged. Enlarged rows were read manually. Native OCR confirmed the close's final D but misread several other hex glyphs, so its other output is not authoritative.

The first post-ELF MCFG read from `0x4a` again formed ERROR 1, 19 bytes, transfer port `0x5006`. Original reservation `0x1e` has flags `0x18f`: queued, closed, canceled, never published or completed. Three queue attempts include two FlowBusy and zero NoCapacity results.

The last failed admission selected **check 1**, same-destination exclusion. Its blocker was TFTP-reply kind 6, opcode 5 (ERROR), aligned datagram length `0x34` (52), **sent 0**, reservation `0x1d`. Flags `0x08` indicate only QRTR-done; the reference TFTP path initializes that bit without requiring confirmation. It is not a GLINK receipt. This snapshot therefore identifies a wholly unsent earlier error packet. It contradicts applying the earlier synthetic fully-sent/waiting-for-completion explanation to this particular last failure. Its exact error code, body and source port are not captured. The first failed attempt and how the blocker subsequently retired are not visible.

| Host observation | Runtime-relative microseconds, hex |
| --- | --- |
| Target request accepted | `0180E318` |
| Dispatch | `0181BB30` |
| First queue attempt | `01820315` |
| Last failed queue attempt | `018417B9` |
| Queued | `01870487` |
| Core writer entry/end | `01885980 / 0188E952` |
| Exact close observed | `0189558D` |
| Canceled | `0189B597` |

Acceptance to dispatch: **55.320 ms**; dispatch to first queue attempt: **18.405 ms**; acceptance to successful queue: **401.775 ms**. Queue to first recorded Core writer entry: **87.289 ms**. That pass took **36.818 ms** and committed command `06`, parameter `0x56`: RX_DONE for local intent `0x56`. It did not attempt the target ERROR. Gates `00a300a3` show a fitting and arbiter-ready remote buffer, a pending negative grant ACK, and a pending receive return at entry and callback. No outbound-intent wait or unsent-flow-predecessor gate blocked the now-queued target; the exact-selection bit is clear. One pass was recorded, none omitted. Earlier Adapter validation or receive-priority vetoes before Core entry are outside this count.

The close was observed **27.707 ms** after that pass ended, **151.814 ms** after queueing. Cancellation followed by **24.586 ms**. These are host observations; the panel has no first FIFO close-visibility sample, pending-denial size, retry-full flag or intent-ACK-yield flag. It cannot uniquely identify which receive-return arbitration branch ran or prove a safe extra transmit window.

Runtime error/detail 0/0, no pending close, stage 6; all four observed closes committed. Unsent cancellations 1; cumulative kept replies/credits 1. One MCFG write committed one byte; no MCFG read completed. IPA registration 1, driver 1, commits 4, complete 1, no pending message/failures/timeouts. ELF RX/queue/publication/completion `013D15E6/0168938E/016AC090/017A73B1`: **2.850216 seconds** to queue and **4.021707 seconds** to completion. Its independent host credit requested/published/completed. Original final ACK `00D9577F` precedes ELF completion by **10.558514 seconds**. ELF body-match is not displayed by this panel and is not claimed from it.

Read inventory 16, mask `0x2e`; journal total 44, omitted 28. First other request is exact MBN digest path, peer `0x4e`, opcode 1, **93 TFTP-body bytes**; IMG_9763 had a different 80-byte request. WLAN firmware requests 0, WLFW services 0, PD state `0x7fffffff` (UNINIT). No WLAN startup milestone moved.

Next offline work should distinguish the older unsent error's writer delay from the target's later receive-return/grant sequence. The last-block snapshot alone cannot tell what held reservation `0x1d` before its target queued. A full-pool-denial replay can check normal RX_DONE, advertisement and ACK ordering; a timing/priority change should not be justified by the short interval to host-observed close. Saved Linux requests WLAN before MCFG/digest, so this exchange is still not established as the initial WLAN prerequisite. No repeat of this unchanged image is needed. No device commands ran while recording the result.
