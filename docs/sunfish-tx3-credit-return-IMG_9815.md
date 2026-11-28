# IMG_9815 / IMG_9817 — early converted reply, later receipt pressure

Settled **TX3 CREDIT RETURN**, associated with the preceding manual boot instruction for source `9e0de51b20df3724fb9d864bb785235dad98c439`, image SHA256 `59912253e23e84ab8b2259e488c06994a20745c23823808eb0e27bf4b6af5a63`. Panel/instruction association does not independently read the handset image hash. Original HEIC, readable JPEG, all 27 physical data rows, executable decoder and comparisons are retained separately in `native-tx3-credit-return-v1/device-result/`. All 127 delivered artifacts were reverified unchanged. No device command or new build was performed for this analysis.

## Clearer IMG_9817 corrects the calls/mask reading

IMG_9817 has the same frozen timestamps, FIFO bounds, eight own-return counters and digest identities as IMG_9815. It is a clearer view of the same captured run, not an independent repeat. The earlier transcription of calls5/mask00063FA4 was wrong. The correct values are **3 calls / mask00003FA4**. The earlier claim that the new credit callback ran is withdrawn. Original images and a correction note remain archived; no delivered build artifact is altered.

Bits11/12 show the existing converted-grant reply attempt/commit. Credit bits17/18, own-return bits21/22 and ready-reply bits19/20 are all absent. Thus none of the three new handoff stages was observed. Own-return gates0..7 remain `0,1,1,1,0,0,0,0`: three first failures (protected/priority/cooldown/conversion; no positive ACK; no fresh backing), no candidate. Counters are unordered and do not isolate the specific protected condition. There is no evidence of a failed borrowed callback.

TX3 queue→publication **262.627ms**, versus **12,666.803ms** in IMG_9813: 12,404.176ms shorter across these boots. TX4 queue→publication is262.744ms. Fast converted publication around262.6ms was already observed in IMG_9802; this is the known fast-publication/late-receipt branch, not evidence that the new credit path fixed the slow branch. IMG_9813's prompt-receipt explanation cannot be carried into this run.

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
