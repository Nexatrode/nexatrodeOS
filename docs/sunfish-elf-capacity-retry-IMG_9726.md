# IMG_9726: first-free ELF retry succeeds; WLAN remains unchanged

Settled **ELF CAPACITY RETRY**, source `66adb113d8b0d7797021bf2c04dbf551d72c467e`, image SHA256 `98d9c20175fd2e9b04ddf15c0c62f2c95a55d31c84f1e655e9c54858b94958e4`. Raw HEIC, manual transcription, OCR and decoding script are retained in the private profile's `device-result/`, outside the unchanged delivered manifest. Full-resolution crops resolve early-entry time as `016760D8` and first full time as `0156720B`. Runtime error zero.

## The one-shot attempt ran and queued successfully

`EARLY ARM ENTER END FLAGS = 01567213 / 016760D8 / 0167E513 / 0000000F`. `EARLY SLOT RESULT ARMS FULL = 1 / 1 / 1 / 2`. Thus one opportunity armed, entered, returned and queued through normal admission. Two eligible Owner calls deferred while the ordinary pool was full; that count is not every Runtime pass.

First full `0156720B`, latest full `01655C64`, three reason2 captures, finished1. First exact release `01664FE8`, slot1, cause1 GLINK RX_DONE, empty mask02. The opening owner was the fully published 52-byte TFTP ERROR from host5003 to modem22 using intent0, metadata06770005/length00340034. ELF subsequently admitted in **the same slot1**, empty mask0, with both older registry owners retained. The new attempt entered69.872ms after release and queued33.851ms later: total **103.723ms from first free to queue**, versus484.645ms in IMG_9725. The targeted admission competition was avoided in this boot.

Changed mask0 compares opening/latest **full** snapshots only. Those snapshots precede the first release; it does not mean that no subsequent packet state changed. It also does not reconstruct every intermediate scheduling event.

## ELF timing

Receive/queue/publication/Owner completion: `014A2425 / 0167E513 / 016BD872 / 0177E2FE`.

| Interval | IMG_9726 | IMG_9725 | Difference |
| --- | ---: | ---: | ---: |
| Receive to queue | 1,949.934ms | 2,330.304ms | -380.370ms |
| Queue to publication | 258.911ms | 807.311ms | -548.400ms |
| Publication to Owner completion | 789.132ms | 409.681ms | +379.451ms |
| Receive to Owner completion | 2,997.977ms | 3,547.296ms | -549.319ms |

The time before first full is806.374ms; first full to actual first release1,039.837ms; release to queue103.723ms. The remaining prequeue time therefore includes a real full-pool wait, not only admission competition. These are separate physical boots with variable arrivals and observation overhead. The figures are observations, not a controlled speedup or deadline proof.

## Latest full-pool ownership

All routes are host node1 to modem node0, ports/intent IDs hexadecimal. This is the last blocked snapshot, **not** the settled state of all packets.

| Slot | Packet | Source to destination | Length/sent | Remote intent |
| --- | --- | --- | --- | --- |
| 0 | QRTR RESUME_TX | 5000 to22 | 52/52 | 2 |
| 1 | TFTP ERROR opcode5 | 5003 to22 | 52/52 | 0 |
| 2 | TFTP ERROR opcode5 | 5002 to21 | 52/52 | 1 |
| 3 | TFTP ERROR opcode5 | 5004 to23 | 52/52 | 3 |
| 4 | IPA response txn2/message0035 DRIVER_INIT_COMPLETE | 4002 to20 | 48/0 | unavailable |
| 5 | Registry response txn3/message0021 | 4003 to9 | 88/88 | 4 |
| 6 | Registry response txn4/message0021 | 4003 to9 | 224/224 | 10 |
| 7 | QRTR RESUME_TX | 4003 to9 | 52/0 | unavailable |

Six published packets await real GLINK completion; two are unstarted. All local QRTR retirement conditions are set. No packet is already GLINK-complete and merely waiting for the local QRTR condition. Published owners cannot be speculatively retired. Opcode5 alone does not establish filenames or error codes. The IPA0035 response is unstarted at this snapshot; the panel does **not** establish whether it is still undelivered after ELF completes. IPA complete1 is the existing handshake-state flag, not proof that every response retired.

## Outcome and next offline question

IPA1, PD7FFFFFFF, **WLAN RRQ0 / WLFW0**. Original final ACK00D91104; MCFG one write/one byte/zero full reads. No WLAN milestone changed. This boot confirms the bounded admission change, so repeating its same first-free probe is unnecessary.

Keep the successful admission boundary. Reconcile the post-ELF sequence with saved Linux traffic and identify the actual publication/completion of the remaining IPA0035 response and registry credit before assigning the WLAN blockage to either. The current snapshot alone cannot answer that or establish a proprietary modem dependency. No new Pixel command is required to decode this result.
