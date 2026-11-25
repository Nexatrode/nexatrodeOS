# IMG_9763: canceled MCFG reply correctly remains unpublished

Settled **MCFG RECORD IDENTITY**, prescribed source `58c621b5681621cb3e202c706ec89c7f9239e124`, image `ef781410f569c715e6ea3ed13524c2fc1b64ad8a8f1ff3fe6e2eff02dc1bf9c0`. The title agrees with the prescribed image; the photograph does not independently measure its hash. Raw photo, preview and decoded scalar rows are retained in this profile's separate `device-result` directory. The delivered manifest and its artifacts remain unchanged.

## The identity discriminator worked

The first post-completed-ELF MCFG RRQ is peer `0x4a`, opcode 1, 73 TFTP-body bytes. The RAM file was absent at dispatch (`FILE FFFFFFFF`). Server result 0 formed a 19-byte TFTP ERROR 1 on transfer port `0x5006`. This is a negative reply, not an OACK or file DATA.

Flags `0x18f` mean request seen, dispatched, reply prepared, queued, client closed, and exact unpublished record canceled. Neither publication nor completion is set; both clocks remain `FFFFFFFF`. Original reservation sequence is `0x000000000000001e`. Unlike IMG_9762's unreliable slot-only observer, this result does not attribute a later reused slot's packet to the canceled response. The actual cancellation path requires zero bytes sent and no assigned remote intent before it retires the record.

| Host observation | Runtime-relative microseconds, hex |
| --- | --- |
| MCFG request accepted | `017DA818` |
| Original error reply queued | `0187FBCD` |
| Exact peer close observed | `0188F681` |
| Exact reply canceled | `018955C4` |

Acceptance to queue took **676.789 ms**. The close was observed **64.180 ms** after queueing; cancellation followed **24.387 ms** later. Three FlowBusy queue rejections and zero NoCapacity rejections were counted before admission. These counts do not identify what prevented publication after admission. The panel does not record when that close first became visible in the FIFO, so these host observation clocks must not be used as a proven transmit deadline. Twenty-seven middle journal records are omitted; the precise intervening writer choices cannot be recovered from this photograph.

## Established startup and cleanup outcomes

Runtime error/detail 0/0, no pending MCFG close, stage 6. Five closes observed and five cleanup commits; held 1, retained 0, unsent 1, cumulative kept 2. Last successful peer `0x4d`, kind 1 (RAM read), is only the last classification, not all five. Cumulative kept is not a current pending count.

ELF RX/queue/publication/completion: `01496CA5/0167301E/016B23E9/01789554`, **3.090607 seconds** receive to completion. Exact QMI body matches. Independent host credit requested/published/completed on intent 0; it was published but not yet complete at reply publication. Original final ACK completed at `00D84D05`, **10.504271 seconds before ELF completion**. IPA flags `0x1f`, registration 1, driver 1, commits 4, accepted response 1, no pending message, failures 0 and timeouts 0; IPA complete 1.

One MCFG write ultimately committed a one-byte RAM file; no MCFG read completed. Read inventory is 17, kind mask `0x2e`. First other-path request is exact `/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig`, peer `0x4f`, opcode 1, **80 TFTP-body bytes in this boot**. Earlier 93-byte synthetic requests are separate fixtures and must not be substituted for this phone request's options or length. Journal total 43, omitted 27, malformed 0.

WLAN firmware requests **0**, WLFW services **0**, valid PD state `0x7fffffff` (UNINIT). This boot confirms diagnostic identity and preserves cleanup/IPA/ELF progress; it establishes no new WLAN startup milestone.

## What follows from this result

No repeat of the unchanged image is needed. A later MCFG completion was not evidence that the original error reached the closed client. Offline work can now use the exact canceled identity and distinguish prequeue FlowBusy from postqueue writer admission. The omitted writer choices and first FIFO visibility are not recoverable from this panel alone.

The saved successful Linux boot requested WLAN before its later MCFG and digest probes, as recorded in `sunfish-mcfg-post-elf-review.md` and `sunfish-mcfg-record-identity.md`. Therefore neither this canceled missing-file error nor the known native digest negative-response gap is established as the cause of initial WLAN startup failing. Do not reintroduce guessed file contents, discard published owners, or claim a new timing-priority fix from these scalar clocks. Protocol behavior and images are unchanged while recording this result. No device commands ran.
