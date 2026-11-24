# Registry TFTP identity result — IMG_9711

The settled REGISTRY TFTP IDENTITY panel is from profile `native-registry-tftp-identity-v1`, delivered at main commit `48c33e34` (diagnostic implementation `e5eb62d4`). Runtime error is zero. WLAN firmware RRQ and WLFW counters remain zero. The original photo, preview and selected verified transcription are saved separately under the profile's private `device-result` directory. The delivered build manifest and image are unchanged.

## Port identities resolved

Port **0x49 requests a read of `/readwrite/mcfg.tmp`**, not a server-check write. The incoming records at `015A7443` and `0160BC1F` both carry `A 0001FFFF 02000049`: RRQ, MCFG path category, 73-byte TFTP body. They have the same projected metadata; exact option bytes and byte-identical retry identity are not recorded. At `01660CC3`, host transfer port 0x5005 sends `A 00050001 00000013` to 0x49: TFTP ERROR 1, file not found, with a 19-byte body / 72-byte GLINK record. The first accepted request header precedes this error publication by 759.936 ms. This interval excludes any earlier FIFO residence and does not establish a one-second client deadline.

Port **0x48 requests `/readwrite/ota_firewall/ruleset`**, with 85-byte RRQs at `01495B7E` and `0150D39F`. Host port 0x5004 publishes ERROR 1 at `014DF7D1`. The DEL_CLIENT at `0156EBB4` names node 0 / port **0x48**, not 0x49. Another earlier close names port 0x22. No port-0x49 close is shown in the retained tail. Do not carry IMG_9710's port-0x49 close timestamp or historical server-check role into this boot.

These are read/error exchanges. Waiting for an OACK followed by client DATA is not the expected continuation of an ERROR reply to an RRQ. Publication of the error also does not prove the modem consumed it: the intent-0 RX_DONE at `01631087` precedes the error's publication and belongs to an earlier use. No matching error completion is identified within this window.

The current native implementation supports RAM-backed MCFG reads when a file exists. In `qcom_tftp_server_check.rs`, `mcfg_request` returns no handled reply for an MCFG read when its RAM file length is absent, and `receive_inner` then permits the supported missing-path ERROR response. This is a possible explanation of the observed reply, not a measurement of file state or proof that MCFG absence prevents WLAN startup.

## Registry and ELF timing

| Interval | Measured time |
| --- | ---: |
| Transaction 3 publication to peer-tail crossing | 6.184 ms |
| Publication to exact RX_DONE visibility | 4,243.183 ms |
| Visibility to matching receipt processing | 732.802 ms |
| Publication to matching receipt processing | 4,975.985 ms |
| ELF request receive to reply queue | 3,662.788 ms |
| ELF queue to publication | 117.969 ms |
| ELF publication to completion | 731.178 ms |
| ELF request receive to completion | 4,511.935 ms |
| Transaction 3 completion to ELF queue | 203.846 ms |

Metadata clocks are transaction-3 publication `011F5901`, tail `011F7129`, first receipt visibility `016017F0`, and processing `016B4672`. The exact intent-3 command-14 header is captured three microseconds later at `016B4675`. ELF receive/queue/publication/completion are respectively `01367EF4`, `016E62B8`, `01702F89`, `017B57B3`. Owner receive observations and later accepted-header observations are different instrumentation boundaries.

Transaction-3 total completion is 372.334 ms longer than IMG_9710's 4,603.651 ms. These are separate instrumented boots, not a controlled latency regression test. The capture retains 86 ordinary records (79 displayed plus seven omitted middle records), with the separate exact completion making 87 total; unavailable projections are zero. A host QRTR RESUME_TX from 0x4003 to port 9 appears at `01481746`, well before receipt visibility in this boot. IMG_9710's later-credit ordering cannot be reused as this boot's event order.

## What this establishes and the next offline comparison

The ELF locator reply completed its existing retirement conditions. That is not WLAN firmware loading. Both MCFG RRQs and the error publication precede ELF publication and completion. The header window freezes on transaction-3 completion, before ELF even queues; its ELF summary clocks continue updating afterward. Consequently this panel does not show the true post-ELF traffic sequence.

The saved successful Linux `qrtr-header-reference-prepared-v1/run/logs/tftp-paths.txt` includes server-check WRQ, missing-file reads of ruleset and modem_efs, then WLAN firmware RRQs around 1.7 seconds later. It contains no MCFG request. Linux's progress despite other unavailable files does not establish whether this native MCFG read/error is harmless or a startup dependency. Port numbers alone cannot align the boots.

The next offline target is MCFG availability and the locator/startup sequence across saved native and Linux evidence, followed by identifying what remains unobserved after ELF completion. This result resolves packet identity and rules out treating the current port-0x49 exchange as an unsent server-check OACK. It does not yet identify the reason WLAN remains uninitialized. No further handset command or new image was needed to record these findings.
