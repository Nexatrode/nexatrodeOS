# Post-ELF MCFG probe comparison

Offline follow-up to [the settled POST ELF STARTUP result](sunfish-post-elf-startup-20261123.md). No production behavior, accepted request policy, firmware file, boot image or device state changed. Seven new native fixtures and one pinned-C reference oracle passed. Inputs, hashes and logs are retained under private `mcfg-post-elf-sequence-review-v1/`.

## What the successful Linux boot actually does

The original `cold-wifi-20261025T200752Z/journal-final.txt` records WLAN-image requests starting at **178.730030 s**. The first MCFG read is at **187.027163 s**, explicitly fails because the file is absent, and is followed by an MCFG write at **187.028831 s**. A subsequent MCFG read at **187.032197 s** ends with client **TFTP ERROR 9, End of Transfer**, at **187.032619 s**. Another write at **187.040181 s** offers `unlink=0`, which the daemon reports as an ignored unknown option. It next attempts `/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig`, gets file-not-found, and retries that lookup.

Thus MCFG activity occurs **8.297133 s after the first WLAN-image request** in that successful reference. The Linux reference does not establish an initial WLAN dependency on MCFG being present or on a successful full MCFG read. The later MCFG work is consistent with modem configuration activity; it is not a unique marker of the WLAN worker's progress.

The separate Linux locator reference still provides the likely ELF-response-to-WLAN correlation, based on names, sizes, order and intent IDs rather than captured QMI payloads. Do not splice timestamps from these two boots or mix journal/printk time with GLINK/socket monotonic observations.

## Zero read completions can be normal

The actual Linux `handle_reader` explicitly treats ERROR 9 as the ending used by stat-like calls, then returns to the daemon's reader cleanup. It sends no DATA in response to that ending. Native `ServerCheck` also terminates a valid exact-peer ERROR packet without sending another response. That path does not increment `mcfg_reads_completed`; the counter increments after ACK of a final DATA block.

The new native fixture completes a synthetic one-byte write, obtains a valid size OACK reporting size one, and ends it with the correct peer's ERROR 9. Its settled counters are exactly **1 / 1 / 0**, with no rejection. A separate full DATA/ACK read reaches **1 / 1 / 1**. The new peer-isolation case verifies that a different peer cannot end the transfer.

This explains why the current zero read-completion counter alone is insufficient to diagnose a stall. It does not prove ERROR 9 occurred on the Pixel: neither the first post-ELF response nor its peer's ending is on that panel.

## One concrete compatibility difference, with no handset identification yet

Two different, well-formed synthetic MCFG RRQs both have the observed **73-byte** body length:

| Options after MCFG name and octet mode | Linux reference | Native with an existing file |
| --- | --- | --- |
| blksize=7680, timeoutms=1000, wsize=10, **tsize=0** | OACK reports actual size | OACK reports actual size, negotiates block 512/window 1 |
| Same first three options, **rsize=0** | Accepted; zero rsize omitted from OACK | Rejected as UnexpectedRequest by the existing positive-rsize validator |

The pinned Linux parser and OACK encoder were compiled with in-memory I/O stubs. For a modeled one-byte file, the size-probe OACK is 47 bytes and the explicit-zero-rsize OACK is 39 bytes. The actual Linux reader function also accepted ERROR 9 as stat termination and completed a synthetic one-byte DATA/ACK exchange. No actual QRTR socket, modem memory, calibration file or full Linux daemon is used by the oracle. The source hash is pinned to `e8dc6a2ced3315f2c2e8b544f710b69fb6e8d464a92ce88ed800cf891edf22f1`.

The native tests demonstrate three different histories ending at the same **1 / 1 / 0** snapshot:

1. First read gets ERROR 1 while the file is absent; a later write commits one byte.
2. A committed file supplies a valid size OACK; the client ends the probe with ERROR 9.
3. A committed file receives an explicit-zero-rsize request and the existing parser rejects it.

These option tuples are **synthetic alternatives, not reconstructed phone bytes**. The photo's name/opcode/length cannot distinguish them. Current settled file presence also cannot establish presence when the first post-ELF RRQ was dispatched. Linux's unknown-option tolerance (`unlink=0`) versus native's strict rejection is another verified difference, but the latest phone request does not identify that option either.

Do not merely broaden the numeric range to accept zero: native `ReadPlan` with an explicit zero limit would select an empty transfer, whereas the tested Linux one-byte read sends its actual byte. Any compatibility change needs explicit semantics and a request observed on the handset. No such change is made here.

## What this does and does not explain about WLAN

Current native completes the original port-0x1F write before ELF, returns the expected ELF body, completes that reply, and later has a committed one-byte MCFG file. Those older missing milestones cannot be carried forward as current failures. The saved early ELF-host-credit boot also demonstrated a confirmation RESUME before the reply; its timing is not assumed to be this boot's timing.

The observed MCFG sequence is compatible with ordinary probing. The rsize compatibility difference could affect MCFG servicing if that option was sent, but **it does not by itself explain the missing initial WLAN request**, which precedes MCFG in the successful Linux cold boot. A native fixture also checks that a recognizable WLAN RRQ with invalid zero-rsize options is still visible to the existing path observer. Parser rejection after dispatch would not by itself hide it from the WLAN RRQ counter. A zero counter remains an observation at host dispatch, not proof the modem never generated or attempted a request.

The first-post-ELF latch records **host acceptance order after matching GLINK completion**, not modem generation order or a demonstrated choice of MCFG instead of WLAN. A source-correct locator reply and its GLINK receipt also do not establish that the remote application accepted it before its own deadline. The exact cause of UNINIT has not been established by these fixtures or the current panel.

## Next hardware discriminator

Another photo or repeat boot of the current image cannot supply the missing option/transaction outcome. The useful next diagnostic is one bounded journal spanning ELF request, reply and its exact credit, through the first post-ELF TFTP exchanges. It should retain QRTR/QMI identities and named DEL_CLIENT node/port, plus the first MCFG RRQ's offered option values, file presence at dispatch, actual server result, response opcode/error/options, queue/publication/read/completion, and exact peer ACK/ERROR code or close. Include the first other-path request so a normal MCFG probe does not consume the entire observation. This can distinguish an ordinary stat ending, a real admission mismatch, a missing reply, and an expired/closed startup exchange without another writer-priority experiment.

No new image was built during this comparison. A new cold boot is needed only after that capture is prepared and verified. No additional Pixel command is required now.
