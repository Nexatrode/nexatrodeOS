# IMG_9771: watched MCFG reply completes before close without an idle loan

The settled **MCFG IDLE TURN DETAIL** panel is associated with source `b837274116e48c2f7ff2440f14e1452036c71787`, image `4a0831986bfd98d81acb69b2c530b9c7fc675a72bd0a338a513519302e44eaca`. Raw photo, full readable view, crop and decoded rows are saved separately in that profile's `device-result`. The association is not an independently measured handset hash; the delivered manifest and its 202 artifacts remain unchanged.

The watched post-ELF request is peer **0x4b**, reservation **0x1e**, flags **0xff**. It was received, dispatched, produced a response, queued, published, completed, saw a client ACK-or-ERROR, and closed. It was **not canceled**. The panel does not expose the reply or client packet opcode, so it cannot distinguish OACK/DATA/ERROR or client ACK versus ERROR. Unlike IMG_9770's peer0x4a reservation0x1f, this reply was admitted on its first attempt with **zero FlowBusy and NoCapacity**. Two watched Core writer passes are recorded and no blocking packet snapshot exists. Different ports across boots do not establish the same logical request identity.

| Event | Same-clock microseconds, hexadecimal |
| --- | --- |
| Receive | `0177EDA5` |
| Server dispatch | `0178C3F7` |
| Queue / first and only attempt | `01790B5D` |
| Publication | `017B4687` |
| Matching GLINK completion | `017BAAE1` |
| Client close | `01833CFD` |

Receive to dispatch is54.866ms; dispatch to queue18.278ms; queue to publication146.218ms; publication to completion25.690ms. The reply went out219.362ms after receive and completed **496.156ms before close**. These are host observation clocks; the panel contains no modem FIFO-tail clock for this reply.

The actual first-idle-loan capture is **unarmed**: all four metadata fields unknown, call/write/RX-veto/block0, start/end unknown, no writer or following-RX records. Source arming happens exactly when a retained missing-file response lends the idle opportunity. Thus the empty detail area means **no such loan ran in this boot**, not that one stalled before its first captured writer. The existing normal/queue-success path was sufficient for the watched reply; this boot cannot identify IMG_9770's one committed idle control or its stopping RX event. Rerunning solely to fill this conditional capture is not currently justified.

ELF receive/queue/publication/completion were `013A2ADB / 01483CD4 / 015987FA / 015F841D`; receive to publication2.055455s and publication to completion392.227ms. Runtime error/detail0, no pending close, cleanup stage6, global close observations/commits3/3. IPA registration/driver/commits1/1/4, complete1, accepted response1, no pending message/failure/timeout. **WLAN RRQ0, WLFW0, PD UNINIT**. Original final ACK and file progress are not displayed and cannot be imported from another boot.

This resolves the watched queue-blocking outcome positively for this run without a new scheduling fix, while showing that successful completion of this one MCFG reply is insufficient to establish WLAN startup. The saved Linux reference already requests WLAN firmware before its later MCFG/digest sequence. The next offline review should therefore reconcile the earlier native startup with Linux and existing whole-startup notes, rather than repeatedly optimize this conditional MCFG idle loan. Keep the full-image correctness and grant/credit/FIFO checks intact. No new image or device command was run for this result analysis.
