# IMG_9725: five published owners, three unsent packets, first freed slot refilled

Settled **ELF TX SLOT OWNERS**, image `e7a5fd655601bdc0e6ef4803f160aa83a18022a12ac6ad25da01bf6602a6dae0`, source `63d88d214e840748fc9db3c993caf8f894dbf0fe`. Raw HEIC and decoded records are retained separately in the private profile's `device-result/`. Delivered verification manifest is unchanged. Runtime error zero.

## Latest blocked pool

All routes below go from host node1 to modem node0; ports/IDs are hexadecimal. The rows are the **latest failed admission snapshot**, not the final settled states of those packets.

| Slot | Packet | Source → destination | Length / sent | Remote intent | State at snapshot |
| --- | --- | --- | --- | --- | --- |
| 0 | QRTR RESUME_TX | 5000 → 22 | 52 / 52 | 2 | Fully published, waiting GLINK completion |
| 1 | TFTP ERROR, opcode5 | 5005 → 49 | 52 / 52 | 0 | Fully published, waiting GLINK completion |
| 2 | TFTP ERROR, opcode5 | 5003 → 22 | 52 / 0 | unavailable | Queued, not started |
| 3 | TFTP ERROR, opcode5 | 5004 → 23 | 52 / 52 | 3 | Fully published, waiting GLINK completion |
| 4 | IPA QMI response, transaction2/message0035 DRIVER_INIT_COMPLETE | 4002 → 20 | 48 / 0 | unavailable | Queued, not started |
| 5 | Registry GET_DOMAIN_LIST response, transaction3/message0021 | 4003 → 9 | 88 / 88 | 4 | Fully published, waiting GLINK completion |
| 6 | Registry GET_DOMAIN_LIST response, transaction4/message0021 | 4003 → 9 | 224 / 224 | 10 | Fully published, waiting GLINK completion |
| 7 | QRTR RESUME_TX | 4003 → 9 | 52 / 0 | unavailable | Queued, not started |

Metadata are `01370007 / 06770005 / 06510005 / 06770005 / 07510002 / 04770002 / 04770002 / 01110007`. Every slot has local QRTR condition bit10 set and GLINK-done bit08 clear. Five have fully-published bit04; three have sent0 and no reserved remote intent. There is no already-GLINK-complete/QRTR-only owner in this snapshot to retire by simply observing an outstanding QRTR credit. Published packets still require their exact real completion; unsent packets must publish or follow an existing validated cancellation path.

The TFTP ERROR bodies and filenames are not captured. Opcode5 and 52 aligned wire bytes do not alone establish a path or raw error code. IPA complete1 likewise does not mean every outbound IPA response has completed: the exact 0035 response is visibly unsent here. Native `ColdSession::protocol_complete` checks its existing handshake flags, not retirement of all outstanding response records.

## Which slot actually freed first

Opening full-pool time `015656F7`; latest full-pool time `016A80EF`; five reason2 captures. First clear `01663374`, **slot1**, cause**1** (exact GLINK RX_DONE), empty mask**02**. Its original opening identity is `06770005 / 5003 / 22 / 0`, length/sent `00340034`: the 52-byte TFTP ERROR to peer0x22, fully published using remote intent0.

This happens **1,039.485 ms after the opening full snapshot**. Slot1 then holds the different ERROR reply to peer0x49 by the latest full snapshot, **281.979 ms later**. Remote intent0 was reused after the earlier legitimate completion; it is not evidence of simultaneous ownership. Changed mask06 means slot1 and slot2 metadata differ between opening/latest; the panel does not show the opening slot2 fields, so its change cannot be assigned to a new packet rather than state changes.

ELF eventually queues in **slot0**, not the first-free slot1. The admission empty mask is zero after staging it; both older same-peer owners remain. Queueing is **484.645 ms after first free** and **202.666 ms after latest full**. The later slot0 release timestamp is not displayed. Thus the first available normal slot was filled by another packet before ELF's successful attempt. Do not equate first free with the slot ultimately used by ELF, or infer the full intermediate scheduling sequence.

## ELF and startup result

Receive/queue/publication/Owner completion are `014A09D9 / 016D9899 / 0179EA28 / 01802A79`.

| Interval | Current boot |
| --- | ---: |
| Receive → queue | 2,330.304 ms |
| Queue → publication | 807.311 ms |
| Publication → Owner completion | 409.681 ms |
| Receive → Owner completion | 3,547.296 ms |

Compared with IMG_9724, prequeue is 540.520 ms shorter but queue-to-publication is 714.414 ms longer; total is 171.860 ms shorter. These are different boots with additional observation work, not a controlled overall speedup. Current phase ages partition acceptance→queue into 726.848 / 48.819 / 12.213 / 1,542.424 ms; direct fields differ by at most1us through conversion.

IPA complete1, PD7FFFFFFF, **WLAN RRQ0 / WLFW0**. Original final ACK `00D8FB81`; one MCFG write/one byte/zero full reads. No new WLAN firmware or WLFW milestone is observed.

Next offline target is the competition after the first release, including the unsent IPA0035 response and queued QRTR credits: can existing bounded dispatch give required startup replies a prompt normal publication/admission opportunity without fabricating completions, stealing credits or bypassing FIFO/grant order? Compare with source and saved Linux ordering before changing policy. This result does not authorize early retirement of any of the five in-flight buffers or prove WLAN depends only on ELF latency. No rerun of this same image is needed to decode these records.
