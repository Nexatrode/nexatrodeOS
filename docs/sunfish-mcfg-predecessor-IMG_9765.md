# IMG_9765: completed predecessor and a 52-byte full-pool return

Settled **MCFG PREDECESSOR TURN**, prescribed revision `fd5fd78e2081a9e1d92a6f1e8c482b542c3ece39`, image `be84059e2872231ae00bcb7eccb362ab55cb0eaa3cc071280dec3b65a3d53cbe`. Raw photo, preview and manually decoded rows are saved in the profile's separate `device-result`. This is an association with the prescribed command/title, not an independently measured handset image hash. The delivered manifest is unchanged.

The target ERROR 1 to peer `0x4a` is reservation `0x1d`, 19 bytes on port `0x5006`, flags `0x18f`: queued, closed, canceled, never published or completed. Four queue attempts include three FlowBusy results and no NoCapacity. The RAM file was absent at dispatch; server result 0 formed the valid negative response.

The last queue blocker is reservation `0x1c`, TFTP-reply kind 6, opcode 5, 52-byte aligned datagram with **all 52 bytes sent**. Flags `0x0b` mean sent, fully sent and the initialized reference-TFTP QRTR-done marker, without GLINK-done. Its exact error code/body/source port are not shown. Unlike IMG_9764's last unsent snapshot, this snapshot is a published response waiting for its real GLINK completion. No predecessor publication was observed after capture, so PUB correctly stays `FFFFFFFF`; matching completion is `0189B646`.

| Host observation | Runtime-relative microseconds, hex |
| --- | --- |
| Target receive / dispatch | `01837898 / 018450B1` |
| First queue attempt / last failed attempt | `0184985F / 0188C1A7` |
| Predecessor matching RX_DONE | `0189B646` |
| Target queued | `018AD64F` |
| Target's recorded Core writer entry / end | `018C2B0A / 018CBADA` |
| Exact close observed / target canceled | `018D2742 / 018D8710` |

The predecessor completed **62.623 ms after the last failed attempt**. The target queued **73.737 ms after that completion**, **482.743 ms after host acceptance**. Its first recorded Core writer pass began **87.227 ms after queueing** and lasted **36.816 ms**. It committed **RX_DONE `0x58`**, not the target ERROR. Gates `04a304a3` identify fitting and arbiter-ready remote capacity, a pending negative grant ACK, a pending receive return and retry-full-intent; intent-ACK-yield is false. Last requested grant size is **52 bytes**. The row watches target `0x1d`, not the receive-return packet; predecessor phase and exact-target-selected bits are clear. One pass is retained and none omitted.

This identifies a retryable full local receive pool despite available remote transmit capacity. The normal full-pool path uses a committed Control RX_DONE to free/reserve a slot, then advertises that backing before its positive ACK. The photo records no subsequent advertisement, ACK or target publication while the target remained active. It does not independently expose the receive entry's pre-return state or ordinary/follow-up path, so it is not a complete recovered call stack. The exact close is observed **27.752 ms after the recorded pass ends**, **151.795 ms after queueing**; cancellation follows **24.526 ms** later. These host intervals are not a proven FIFO-visibility deadline or a guarantee that extra writes would fit.

Runtime error/detail 0/0, no pending close, stage 6; all four observed close cleanups committed. Unsent cancellations 2 and cumulative kept 1 describe all cleanups, not disposal of the completed predecessor. IPA registration/driver/commits `1/1/4`, complete 1, no pending message/failure/timeout. ELF RX/queue/publication/completion `01488A29/016F3126/017115EA/017E036E`: **2.533117 seconds to queue**, **3.504453 seconds to completion**. Independent host-credit mask 7 means requested/published/completed. Original final ACK `00D95829` completed **10.791749 seconds before ELF completion**. Body-match is not displayed by this panel.

One MCFG write committed one byte; no MCFG read completed. Read inventory 16, kind mask `0x2e`. First other request is exact MBN digest path, peer `0x4e`, opcode 1, 93 TFTP-body bytes. WLAN requests **0**, WLFW services **0**, valid PD state `0x7fffffff` (UNINIT). No new WLAN startup milestone moved.

The next offline experiment should try an earlier bounded turn for the ordinary writer after an authoritative MCFG error queues, preserving the fresh FIFO veto, normal receive-return/grant/credit priorities, advertisement-before-ACK order, blocked-write fallback and terminal faults. The observed 73.737 ms completion-to-queue gap is a separate service-admission interval; do not claim a publication handoff alone removes it. No need to repeat this unchanged image. Saved Linux requests WLAN before MCFG/digest, so this MCFG exchange is still not established as the initial WLAN trigger. No device commands ran while recording this result.
