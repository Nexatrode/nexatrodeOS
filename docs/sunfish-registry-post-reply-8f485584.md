# REGISTRY POST REPLY: matching completion arrived late, transaction 4 missed the borrowed turn

The settled photo `codex-clipboard-8f485584-3e5b-464f-9815-fe1d54c539ab.png` shows the panel from source `1d7e8df1`, image SHA256 `9c84d94acef84f72c4bfeb467966d66fa4a1b95fa883b33250a4544a26f72293`. Original photo, all 27 fields and arithmetic are saved privately in `native-registry-post-reply-v1/device-result`. The original build manifest and source snapshots remain unchanged. Runtime error is zero.

## Transaction 3 was read promptly; its receipt was not present for most of the delay

| Same-boot interval | Time |
|---|---:|
| Full reply publication to sampled modem tail crossing | **6.138 ms** |
| Reply publication to matching completion last-absent sample start | 10,252.828 ms |
| Reply publication to matching completion first-visible sample end | **10,271.948 ms** |
| Receipt arrival bracket | 19.120 ms |
| First visible completion to host processing | **647.595 ms** |
| Full reply publication to processed completion | 10,919.543 ms |

The modem consumed the reply's FIFO bytes promptly. That does not establish application acceptance. The exact completion did not become fully visible in the sampled RX FIFO until roughly 10.25–10.27 seconds after publication. Consequently, host parsing delay alone cannot explain the roughly eleven-second publication-to-processed-receipt interval. There is also a separate measurable 648 ms after first visibility.

OLD RX AHEAD is `0x60`: 96 bytes preceded this receipt when its arrival span was observed. Their headers are not recorded by this panel, so their identities cannot be reconstructed from size alone. RX END EVICT `00010018` means the exact receipt matched/froze and 24 older head-history spans were overwritten. The matching receipt's actual bounds remain present; the eviction count does not invalidate those populated bounds or supply the omitted intervening sequence.

The tracked converted grant used backing intent `0x38`. Its advertisement committed 55.289 ms after reply publication and its positive ACK 144.476 ms after publication, 89.187 ms after the advertisement. Thus the tracked advertisement preceded its ACK and neither remained pending for the subsequent ten-second receipt delay. This does not rule out later unrelated receive-intent pressure or another modem prerequisite.

## Transaction 4 still waits behind later arbitration

Transaction 4 queue-to-publication is **7,100.521 ms**, compared with 7,084.542 ms in IMG_9704. Its snapshot state is 2, full publication frozen, with 43 calls. The packed first-condition counts decode to:

* No pending ACK: 2.
* Pending negative ACK: 14.
* Positive ACK without unadvertised backing: 14.
* Fresh backing without an ordinary yield or exact conversion opportunity: 13.
* Remaining eight conditions: zero.

These counts sum to 43. They count entry observations, not durations, and earlier conditions mask later ones. They do not assign seven seconds to individual operations.

TX4 MASK `0x24AC` has bits 2, 3, 5, 7, 10 and 13 set: control and other DATA were selected while it waited; a registry-priority DATA fragment ultimately committed; a denial-to-grant conversion was seen and its exact opportunity armed. The conversion-bound attempted/committed bits 11 and 12 and generic borrowed callback bits 8 and 9 are absent. **An opportunity armed for transaction 4 but the borrowed callback did not execute.** The panel does not identify what expired or invalidated that opportunity. No fitting-intent absence, outbound-wait/request-selected or selected-reply FIFO-block bit appears in this captured window; this applies to sampled calls, not every instant.

The transaction 3 completion was already visible **337.751 ms before transaction 4 publication**, but was processed **309.844 ms after it**. This independently preserves the two-outstanding-reply behavior: transaction 4 publication did not require transaction 3's processed receipt. It also prevents misattributing the entire transaction 4 transmit wait to receipt processing.

## Startup result and next offline target

IPA COMPLETE remains 1; WLAN RRQ RX and WLFW SERVERS remain zero. ELF DONE is present, but this panel omits ELF receive/queue/publication, so its absolute timestamp cannot establish an improved end-to-end ELF duration across boots.

This observation separates prompt modem FIFO consumption from late matching receipt visibility and exposes an armed-but-unused transaction 4 turn. Next review the lifetime of that exact opportunity: conversion, intervening receive/dispatch work, advertisement/ACK expiration and normal writer selection. Use saved native traffic and the existing staggered-arrival replay before changing scheduling. The reviewed Linux/Android allocate-or-reuse/advertise-before-ACK contract remains relevant, but does not supply this native runtime's missing event sequence. Preserve grant ordering, the two-reply bound, normal FIFO validation and credit checks.

No new Pixel command is needed to interpret this result or begin that offline review. No device command was executed while saving it.
