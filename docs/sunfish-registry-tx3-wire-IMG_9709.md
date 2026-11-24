# Transaction 3 wire log — IMG_9709

The settled REGISTRY TX3 WIRE LOG panel reports runtime error zero, WLAN RRQ zero and WLFW zero. The immutable image is commit `32bbc2a3`, profile `native-registry-tx3-wire-v1`. The original photo, preview, tail crop and selected verified scalar/header transcription are saved in its separate private `device-result` directory. The published build manifest is unchanged.

| Event | Hex microseconds | Relative to publication |
| --- | --- | --- |
| Transaction 3 full publication | 012D8CD6 | 0 |
| Peer tail crosses reply | 012DA4F1 | 6.171 ms |
| Matching RX_DONE first visible | 01919523 | 6,555.725 ms |
| Matching RX_DONE decoded | 019E3082 | 7,381.932 ms |

The matching header is RX command 14, channel 2, remote intent 4, recorded at 019E3085. The opening TX header is command 9, channel 1, intent 4, 104 wire bytes: QRTR DATA from port 0x4003 to port 3, QMI response transaction 3 / message 0x21. This is the service-registry GET_DOMAIN_LIST reply, distinct from an IPA registration response. Peer-tail observation establishes transport consumption, not application acceptance.

Transport consumption is again prompt. The subsequent visibility gap is 6,549.554 ms; decoding after visibility takes 826.207 ms. Compared with IMG_9708, visibility is 859.803 ms earlier relative to publication and total processing is 677.371 ms earlier, but these separate instrumented boots are not a controlled performance benchmark.

## Intervening traffic

The observer retained 79 ordinary records and the exact completion, omitted 56 middle records, and reported zero unavailable projections. The opening segment repeatedly shows inbound QRTR NEW_SERVER records (type 4), receive-intent requests for 0x34 = 52 bytes, host non-reusable RX_DONE (command 6), INTENT advertisements (command 5), and positive grant ACKs (command 8, parameter 1). For one visible cycle, request 013025DB precedes advertisement 0131ED97 by 116.668 ms; ACK 013349C7 follows the advertisement by 89.136 ms. The compact advertisement header reports a count, not its backing ID or size.

The tail retains incoming DATA from port 0x49 to 0x5000 at 0186415A and 018C3974, followed by an exact QRTR DEL_CLIENT naming node 0 / port 0x49 at 0191E65E. A host DATA packet from transfer port 0x5005 to port 0x49 is published at 01997775, 495.895 ms after the close was decoded. Its ports/length identify the transfer path; this compact capture does not include the TFTP opcode and cannot alone prove it was an OACK or correlate both preceding packets to identical WRQ bytes.

The accepted RX sequence after that close and before transaction 3's receipt contains four reusable completions for remote intent IDs 0x40, 1, 2 and 3, followed by the exact ID 4. Interspersed host writes include grant ACKs, an IPA QMI response (transaction 0x20 / message 0x35) and the transfer reply. These are real earlier FIFO records that must preserve order. The log records accepted/decoded times, not when each record first appeared; it does not prove all those records were present at the earlier matching-receipt visibility sample. In particular, do not carry IMG_9708's 96-byte-ahead count into this different boot.

## Source cross-check and next offline work

The pinned Mobian `qcom_glink_handle_intent_req` allocates a fresh non-reusable intent, advertises it and then ACKs. Android first searches for a fitting reusable local intent and otherwise takes the same fresh-advertisement-before-ACK path. Thus native's non-reusable returns and grant cycles are not by themselves a protocol defect, and the photo does not justify removing their ordering or validation. Linux's saved first-WRQ window demonstrates prompt reusable returns for its available buffers, but is not an exact transaction-3 timing reference and does not establish the size/availability of native's omitted slots.

This capture narrows the next offline review to the receive-buffer lifecycle during the service-announcement burst and the work draining the completion prefix. Check existing buffer-state notes and replay completed control packets separately from retained registry requests before considering reusable returns or bounded servicing. Preserve opaque transaction identity, error/reset behavior, FIFO checks, advertisement-before-ACK and QRTR credits. The 56 omitted records prevent attributing the entire gap to the visible grant cycles. No new Pixel run is needed merely to reread this evidence; no new image or device action was performed while recording it.
