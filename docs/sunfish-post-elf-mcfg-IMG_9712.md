# Settled POST ELF MCFG TRACE — IMG_9712

Physical result supplied November 24, 2026 for verified `native-post-elf-mcfg-trace-v1`, image SHA-256 `d61535d7003b99bdd3ed50cabb23536b4ae7a579425d4fe9320db1acc62f8953`. Original HEIC and decoded clocks/journal are retained in that private profile's `device-result/`; its delivered verification manifest is unchanged.

## The request was a valid missing-file probe

The first accepted post-ELF MCFG RRQ is **peer 0x4B**, body **0x49 = 73 bytes**. Options mask `0x0F`, parser flags zero and unknown count zero show `blksize=7680`, `timeoutms=1000`, `wsize=10`, `tsize=0`; **rsize, seek and unlink were absent**. This identifies the `tsize` alternative from the previous offline comparison, not the zero-rsize rejection alternative.

At actual server dispatch the file was **absent**. Server result zero means a successfully prepared response, not successful file lookup: opcode 5, error 1, length `0x13=19` matches the current native `file not found` reply. There was no OACK to decode. It used transfer port 0x5007, remote GLINK intent 0. Stage mask `0xBF` records receive, dispatch, reply preparation, queue, publication, exact completion and close, with **no peer TFTP ACK/ERROR**. Queue pressure counters are zero.

| Event | Hex microseconds | Seconds from runtime start |
| --- | --- | --- |
| MCFG accepted | 0194DAFA | 26.532602 |
| Error reply queued | 0195FB19 | 26.606361 |
| Error reply fully published | 019B410F | 26.951951 |
| Exact GLINK RX_DONE | 019BA60C | 26.977804 |
| Peer 0x4B DEL_CLIENT | 019D0278 | 27.067000 |

The reply published **419.349 ms after acceptance**, completed **25.853 ms after publication**, and completed **89.196 ms before close**. This excludes an unsent or transport-incomplete reply for this target. No DATA is expected from a client receiving a missing-file error. It does not establish application acceptance or any other peer's result.

The settled file counters are **one completed write / one committed byte / zero completed reads**. Thus the file was created after the observed absent-file dispatch; settled presence cannot be projected backward. Earlier peer 0x49 MCFG RRQs are visible before ELF completion and must not be conflated with target peer 0x4B. The middle 26 relevant journal records are omitted, so later MCFG options, OACKs and ERROR9 endings cannot be reconstructed from this screen.

## ELF and following requests

ELF body match and completion are one. Host-credit flags `0x0F` show the requested confirmation RESUME was both published and completed before the ELF reply was published. ELF confirmation request is one. The original peer's final ACK completed at `00D9B097` (14.266519 s), **7.296217 s before even the ELF request was accepted**.

ELF receive/queue/publication/completion are `01490570 / 017B2A67 / 0181EE9D / 018A383E` (21.562736 / 24.848999 / 25.292445 / 25.835582 s). Receive-to-publication is **3.729709 s**, including **3.286263 s before queueing**, then 443.446 ms queued and 543.137 ms from publication to matching completion. The journal shows another registry reply (transaction 4/message 0x21) ahead of the target transaction 5 lookup; it does not retain enough middle records to assign the full ELF delay to that predecessor.

The first other post-ELF request is an RRQ for exact `/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig`, peer 0x4F, body `0x5D=93` bytes, accepted at `01D27233` (30.569011 s). The journal shows the same peer retrying at `02767A95`, its DEL_CLIENT at `02D452FE`, and another same-path request from peer 0x51 at `02D5B05E`. No reply/result for this other path is established by this bounded capture.

In the successful Linux cold journal, the first MCFG read also fails because the file is absent, then the file is written, then a size probe ends with ERROR9. Linux also rejects `mbn_hw.dig` and receives retries. Crucially, Linux requested WLAN firmware **8.297133 s before the first MCFG read**. Neither missing MCFG at the first probe nor missing `mbn_hw.dig` is established as a prerequisite blocking initial WLAN startup. See [the source/trace comparison](sunfish-mcfg-probe-comparison.md).

## WLAN outcome and next analysis

Runtime error is zero and IPA complete is one. PD query remains `0x7FFFFFFF`; **WLAN RRQ=0 and WLFW servers=0**. This is useful elimination of a specific MCFG option/admission/missing-reply theory, not a new WLAN milestone.

Do not change rsize parsing or populate MCFG/MBN files based on this result. Next useful offline work is comparing the separate WLAN-startup/locator path and its service identities/timing against the saved successful Linux boot, using existing predecessor and ELF traces. The current journal contains ordinary modem configuration work; that cannot establish that the WLAN worker accepted the locator result or started. No rerun of this same image is needed to establish the observed target's outcome. A further cold boot should follow a concrete remaining discriminator, not repeat this probe.
