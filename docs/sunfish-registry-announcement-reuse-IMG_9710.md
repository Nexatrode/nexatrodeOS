# Announcement reuse result — IMG_9710

The settled REGISTRY ANNOUNCE REUSE panel is from image commit `f286a506`, profile `native-registry-announcement-reuse-v1`. Runtime error is zero; WLAN RRQ and WLFW are both zero. The original photo, preview and selected verified header/scalar transcription are saved in its separate private `device-result` directory. The published build manifest is unchanged.

| Interval | IMG_9709 | IMG_9710 |
| --- | --- | --- |
| Reply publication to peer-tail crossing | 6.171 ms | 6.120 ms |
| Publication to exact RX_DONE first visible | 6,555.725 ms | 3,781.296 ms |
| Visibility to matching receipt processing | 826.207 ms | 822.355 ms |
| Publication to matching receipt processing | 7,381.932 ms | 4,603.651 ms |
| Omitted middle / unavailable projections | 56 / 0 | 1 / 0 |
| Ordinary accepted/committed headers in window | 135 | 80 |

Metadata timestamps are publication `012C69B9`, tail `012C81A1`, visibility `01661C69`, matching decoded completion `0172A8BC`. The matching command-14 header for remote intent 4 is recorded at `0172A8BF`. The opening response is QMI transaction 3 / message 0x21, QRTR source port 0x4003 to peer port 9. IMG_9709 used peer port 3; the photos do not provide byte-identical request bodies or a controlled repeated latency benchmark.

## What changed on hardware

Intent 0x3B is visibly reused for multiple incoming NEW_SERVER messages, with host command 14 returns rather than non-reusable returns and a fresh advertisement for every packet. It later receives a registry request for transaction 4 (`013B2D53`), is returned non-reusable with command 6 (`0140D6C7`), and a correctly identified transaction-4 response is published (`0141AE2A`) while transaction 3 is still awaiting its own completion. Intent 0x45 is also reused across multiple announcements. These records demonstrate the intended wire behavior and subsequent request identity in this boot.

The window contains 55 fewer ordinary records. Exact receipt visibility is 2,774.429 ms earlier relative to publication; total matching-receipt processing is 2,778.281 ms earlier. The post-visibility processing interval remains about 822 ms, so the observed gain primarily precedes receipt visibility. The transport-consumption interval stays about 6 ms. Buffer reuse worked and the boot improved; this single pair of boots does not assign every millisecond of the difference to the change or establish a guaranteed deadline.

## What remains

Port 0x49 sends DATA-shaped QRTR packets to service port 0x5000 at `015AD5FC` and `0160C742`. Its exact DEL_CLIENT is decoded at `01667061`; the host transfer packet from 0x5005 to 0x49 is published at `016DF50F`, 492.718 ms later. This closely matches the previous boot's 495.895 ms close-to-transfer-reply interval. Ports and lengths do not alone establish the TFTP opcode, duplicate identity, filename or whether a DATA block was sent. Do not label these packets OACK/WRQ solely from this compact capture.

A host QRTR RESUME_TX from 0x4003 to port 9 is recorded at `016B8268`. The exact transaction-3 completion was already visible 353.791 ms earlier, before the subsequent accepted close/completion records and this credit publication. Thus this later credit write cannot be required for the already-visible transaction-3 receipt. It may still matter for later request servicing. Incoming completions for remote intents 0x40, 1, 2 and 3 are processed before the exact intent-4 completion; the transcript records accepted/committed observations, not each packet's physical arrival time.

WLAN and WLFW have not advanced. This panel does not include ELF final timings or an IPA-complete counter, so neither outcome can be inferred from it. The remaining offline targets are the nearly complete post-publication sequence (only one middle record omitted), receive/dispatch cost after visibility, and the transfer reply that still follows port 0x49's close. No additional Pixel run or new image was prepared merely to record this evidence.
