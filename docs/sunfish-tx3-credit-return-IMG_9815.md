# IMG_9815 — credit committed, early converted reply, later receipt pressure

Settled **TX3 CREDIT RETURN**, associated with the preceding manual boot instruction for source `9e0de51b20df3724fb9d864bb785235dad98c439`, image SHA256 `59912253e23e84ab8b2259e488c06994a20745c23823808eb0e27bf4b6af5a63`. Panel/instruction association does not independently read the handset image hash. Original HEIC, readable JPEG, all 27 physical data rows, executable decoder and comparisons are retained separately in `native-tx3-credit-return-v1/device-result/`. All 127 delivered artifacts were reverified unchanged. No device command or new build was performed for this analysis.

## Credit first ran; the complete three-stage loan did not

Frozen TX3 snapshot2 has 5 publication observations, mask **00063FA4**. Bits17/18 establish the checked credit callback attempt and success. The old converted-credit selector is restricted to transaction4, and the ELF paired-credit target is restricted to the first ELF response. Consequently the TX3 credit bits identify the new scoped TX3 credit path. Bits11/12 also show the existing converted-grant reply attempt and success. Bits21/22 (new own-return callback) and19/20 (ready reply callback) are absent. This boot used **credit then an existing converted-grant reply opportunity**, not the complete borrowed credit→own RX_DONE→ready reply sequence.

Own-return first-failure counters0..7 are `0,1,1,1,0,0,0,0`. The three recorded checks hit protected work/priority DATA/cooldown/conversion, no positive pending ACK, and no unadvertised backing respectively. In particular the credit itself is selected priority DATA and excludes a simultaneous own-return choice. These unordered counters do not identify which subsequent grant changed, the original return's ordinary publication time, or every one of the five publication observations. No candidate7 or failed/blocked borrowed own-return callback is recorded. A separate successful credit does not guarantee that the next ordinary call retains all fresh-backing and conversion prerequisites.

TX3 queue→publication **262.627ms**, versus **12,666.803ms** in IMG_9813: 12,404.176ms shorter across these boots. TX4 queue→publication is262.744ms. The new credit actually ran, but fast converted publication around262.6ms was already observed in IMG_9802. This is a real current result, not proof of a repeatable 12-second improvement solely caused by this change. Current TX3 follows the known fast-publication/late-receipt branch; it does not inherit IMG_9813's prompt receipt.

## Exact FIFO observation separates modem return from host backlog

| Interval | TX3 | ELF |
| --- | ---: | ---: |
| Publication → observed modem tail crossing | 6.210ms | 6.125ms |
| Publication → last matching receipt absent sample | 3,705.933ms | 241.739ms |
| Publication → first matching receipt visible sample | 3,724.885ms | 260.751ms |
| Visibility bracket | 18.952ms | 19.012ms |
| First visibility → Core matching completion | 698.208ms | 563.061ms |
| Publication → Core matching completion | 4,423.093ms | 823.812ms |

Both replies were consumed promptly by the observed peer tail. The much later matching RX_DONE visibility is separate from the additional host receive/dispatch backlog after visibility. There were **104 bytes ahead of TX3's completion and160 ahead of ELF's**, not a zero-backlog return. TX3 history evictions1 limits reconstruction of the intervening sequence, although the matching record's retained bounds are present; ELF evictions0. These are sample bounds, not exact modem event instants. This panel does not identify those preceding bytes' headers or classify every millisecond as firmware wait versus host checks.

ELF armed with48 unread bytes, remote intent0. Owner handling follows its Core completion by18.428ms. The actual reply completion follows normal identity/ownership; neither early peer consumption nor FIFO visibility itself frees a transmit owner.

## ELF admission pressure changed to the global transmit pool

ELF receive→queue **2,295.105ms**, queue→publication111.993ms, publication→Owner842.240ms, total **3,249.338ms**, versus10,513.281ms in IMG_9813. One ELF writer call, maskA0, no paired credit/DATA callback. Its completed writer span36.904ms agrees with first/last clocks to1us rounding.

ELF wait reasons are `0,0,4,0,0,0`: four transmit-capacity rejections, no recorded peer/flow/credit/capability rejection. Latest failure sequence4/reason2 retains two fully published registry owners, txn3/4 flags3B (`00033B04/00043B04`), gates1. That snapshot names their ownership but does not identify all eight occupied transmit slots or show those two caused the global pool to remain full. The first actual free-slot observation is107.825ms before queueing. ELF queues **441.686ms before TX3's matching Core receipt**, so retiring TX3 was not an admission prerequisite. Existing third-owner admission and separate pool capacity must not be conflated. The next offline review should use the retained all-slot/first-release and receipt/header captures; speculative published-buffer retirement is unsupported.

## Startup and next work

Runtime error/detail0, cleanup stage6, live IPA1F, no pending IPA record, exact ELF credit mask7/body match1, original final ACK00DA0887. Locator queued/completed masks75. Historical IPA complete1; **WLAN firmware RRQ0 / WLFW0**. RAM MCFG remains one byte and complete firmware reads0. Startup is earlier but there is no new WLAN milestone.

Both retained digest records now have flagsBF and real publication/completion before their own closes, unlike IMG_9813's unpublished second record. They are distinct peer4E/4F, transfer ports5009/500A, reservations22/24; each reply is19 bytes. These completed short replies are not WLAN firmware transfers. Total9/omitted7 prevents generalizing to all digest requests.

No additional Pixel run is needed to interpret this photograph. Next work is offline comparison with the already documented fast-publication/late-receipt boots, the retained full-pool/first-release ownership, and the104/160-byte receive backlog. This avoids repeating the earlier no-writer-turn probe and preserves FIFO validation, grant advertisement-before-ACK, QRTR credits and real matching completion lifetimes.
