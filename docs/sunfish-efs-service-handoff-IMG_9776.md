# IMG_9776: first EFS reply completes before client close

The settled **EFS SERVICE HANDOFF** photo corresponds to the delivered `native-efs-service-handoff-v1` profile, revision `bf0d4db67b462c1efe9944ae2c59ae673a861815`, image SHA256 `ec29089cf112136d49fc2914295a51cd979376f03167f0b9f6f724c139bf88d3`. This is a panel/manual-command association, not an independently measured handset image hash. Raw HEIC, readable views and decoded fields/arithmetic are saved in its separate `device-result`. Delivered artifacts and manifest remain unchanged.

## The intended service handoff ran

`EARLY CALL WRITE RX BLOCK SVC STATE = 1 2 0 0 1 2`: the extra ordinary service step ran once and returned ResponseQueued. The inherited early writer committed two Session writes with no RX veto or TX backpressure. STOP1 and CMDS`00000908` identify grant ACK8 then DATA9, least-significant byte first. The word identifies operations, not a separate reservation/route projection for that DATA; actual EFS publication and matching completion remain independently tracked by the original transaction.

The first EFS is peer0x21, reservation0x13, remote intent1, ERROR1/19 bytes, local port0x5003, flags0xbf. Its queue admission succeeded on the first attempt with no FlowBusy or NoCapacity. The ruleset peer is0x22. These two roles had opposite port assignments in the preceding boot; compare exact paths and within-boot handles rather than assuming a stable peer port across boots. The runtime scope uses actual handles and worked with this assignment too.

| First EFS event | Same-clock hex microseconds |
| --- | --- |
| Accepted | `00DFB216` |
| Dispatch completed | `00E1188C` |
| Queued | `00E1601A` |
| Published | `00EB7A3F` |
| FIFO watch armed | `00EB7A49` |
| Peer tail crossed reply | `00EB9221` |
| Last sample excluding matching receipt | `00F2BEC9` |
| First sample containing matching receipt | `00F30901` |
| Matching completion processed | `00F846FA` |
| Client close processed | `0103DA44` |

Acceptance-to-dispatch is **91.766ms**, compared with 932.895 ms previously: 841.129 ms shorter. Dispatch-to-queue 18.318 ms and queue-to-publication 662.053 ms give **772.137ms acceptance-to-publication**, compared with 1,336.418 ms previously. Queue-to-publication itself increased from 385.088 ms; the total improvement comes from earlier dispatch. One boot each does not establish a stable deadline or isolate every interleaved cost, but the intended extra service step demonstrably ran and the expected dispatch interval shortened substantially.

The modem's tail crossing was observed **6.114ms after publication**. This is shared-FIFO consumption, not proof of application acceptance. Matching RX_DONE first becomes visible 489.184 ms after that crossing, within a 19.000 ms absent/visible sampling bracket. The host processes it 343.545 ms after first visibility. Publication-to-processed-completion is **838.843ms**, compared with 7,964.614 ms previously. The first EFS completion now precedes its host-observed client close by **758.602ms**.

The watch finished with no history evictions. RX unread at arm 8; 144 bytes precede the matching receipt at first visibility and are not identified by this panel. RX-at-close and visible-at-close are absent because the watch had already finished; those all-ones fields do not negate the actual completion-before-close timestamps.

## A second same-peer reply still completes late

The second EFS RRQ is the same peer and numeric option projection but a different opaque request and reservation0x15. This panel cannot prove byte-identical retransmission. It is accepted 941.800 ms after the first acceptance (169.663ms after first publication). Its clocks are:

- RX`00EE10FE`, dispatch`00EEE817`, queue`00F965CB`.
- Publication`00FE1FE6`, matching completion`01703D9F`, client close`0103DA44`.

It dispatches in 55.065 ms, then takes 687.540 ms to queue and 309.787 ms to publish. Admission takes 3 attempts with 2 FlowBusy and no NoCapacity. Total acceptance-to-publication is 1,052.392 ms. It publishes 375.390 ms before the observed close, but completes 7,478.713 ms after publication and **7,103.323ms after close**. Earlier dispatch for the first request has not established prompt completion of every same-peer request.

## Other startup state and next offline question

Original peer0x1f sent DATA and its final ACK completed (phase4) before close. ELF body match1, host-credit mask7 and completion are present. Runtime error/detail0, no pending close, cleanup stage6, IPA complete1. PD remains UNINIT and **WLAN RRQ0 / WLFW0**; no WLAN milestone appeared. The image establishes an earlier successful EFS exchange, not working Wi-Fi.

The next offline comparison is the second same-peer EFS request, its retained-reply/flow wait and completion against the successful Linux exchange. Do not generalize this run's first receipt bounds to its second reply, synthesize a completion, relax QRTR accounting or retire published buffers on close. Reuse existing source/trace evidence to determine what the current same-peer admission rule guarantees and whether that behavior differs from Linux before requesting another device probe. No additional Pixel command is needed to interpret this photo.
