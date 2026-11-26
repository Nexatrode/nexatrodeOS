# IMG_9773: early modem_efs request is dispatched after client close

The settled **EARLY STARTUP REPLIES** panel is associated with source `d7dcf6ddf6682212dde9a5ca41e552f95b5b0a88`, image `bd432c73a3ed3445d099c30b1415367c0b94eb5bf7914bbe8e9347e81d9ff6b6`. Raw photo, views/crops and decoded rows are saved separately under that profile's `device-result`. Association is not an independently measured handset image hash. The delivered manifest and all207 artifacts remain unchanged.

## Original write and ELF still complete

Original server-check peer0x1f received WRQ `00C11A94`, published OACK `00CC1045`, accepted DATA `00CDD1DB`, and published/completed final ACK `00DA147A / 00DA7979`. ACK phase4 identifies the matching real GLINK receipt. That completion is **7.285309s before ELF request acceptance**, not an ACK delayed behind ELF. The original close photo is retained, but no derived original-close interval relies on its small transcription.

ELF received/queued/published/completed at `0149A3B6 / 01717FF9 / 01736552 / 01806E8E`; exact body match1, host-credit mask7, locator queued/completed name masks both0x75. ELF receive to publication is2.736540s, receive to completion3.590872s. These are host observations, not modem worker execution clocks.

## Early negative replies differ from the successful Linux sequence

All four retained entries have flags **0xbf**: received, dispatched, response produced, queued, published, matching GLINK completion and closed; no client ACK/ERROR is captured. All replies are **TFTP ERROR1, 19 bytes**, with server result0 and option-parse flags0. Thus none calls for an OACK or client DATA. Flags alone do not establish that completion preceded close; the clocks must be compared.

| Retained request | Peer / reservation | Dispatch after receive | Publication relative to close | Completion relative to close |
| --- | --- | ---: | ---: | ---: |
| First ruleset |0x22 /0x12|55.368ms|1,102.756ms before|358.027ms before|
| Second ruleset |0x22 /0x14|55.300ms|149.891ms after|7,961.997ms after|
| First modem_efs |0x21 /0x1a|3,911.462ms|7,487.201ms after|8,432.564ms after|
| Second modem_efs |0x21 /0x13|55.319ms|1,078.227ms before|6,855.058ms after|

The first modem_efs request (`00DD3245`) is accepted **before** its second (`00EE0B6F`), yet its server dispatch (`0118E16B`) is later than the second dispatch (`00EEE386`) and **1,151.809ms after the client's close**. Its successful queue `01759283` comes another **6,074.648ms after dispatch**, after ten admission attempts: eight FlowBusy, one NoCapacity, then success. The later-accepted request queues at `00EF2B6E` on its first attempt. This is a concrete service/admission-order divergence, not merely a slow matching receipt.

The second modem_efs ERROR publishes at `00F6DA58`, before close `01074E2B`, but matching completion is `016FE7BD`,7.933285s after publication. The first request's ERROR publishes at `01798D0B`, completes `0187F9DE`, and its close observation is `01074E2A`. Those close clocks are projections of one client-close event, sampled one microsecond apart for separate entries. The first request's queued reply follows the earlier-queued reply in reservation identity. The panel does not capture TX FIFO peer-tail or matching RX_DONE first visibility for either early exchange, so it cannot divide that7.933s into remote read versus delayed receipt generation versus host processing. Do not treat late GLINK RX_DONE as proof that the client had not read the error before closing.

Ruleset RRQs are85 bytes, explicit rsize4, option presence0x17. Modem_efs RRQs are74 bytes, rsize absent, option presence0x0f. Only this numeric projection is retained; the same-path requests are not proven byte-identical retries. The first two of four ruleset observations are retained (two omitted); both modem_efs observations are retained.

## What follows

Runtime error/detail0, no pending close, cleanup stage6, IPA complete1, PD UNINIT, **WLAN RRQ0 / WLFW0**. Global read count18, kinds0x2e, without WLAN kind. No working-WLAN milestone moved.

The Linux journal answers the analogous early missing-file reads before its first WLAN firmware request; its later MCFG/digest exchange remains unsuitable as the presumed initial trigger. Native now has a named earlier mismatch: a first modem_efs request remains held while a later same-peer request is dispatched first, then waits behind a published response. Owner currently selects held requests by array position and flushes retained replies before selecting new work; reused lower indices can precede older requests. That source structure is a possible explanation, not a reconstructed handset queue state. An offline replay should test the two arrivals, slot reuse, client close and exact published predecessor before proposing scheduling or cleanup changes. It should also distinguish the earlier-published response's delayed receipt from its application acceptance. The present panel does not prove that correcting this exchange alone will start WLAN. No new Pixel command is needed to record or replay this result.
