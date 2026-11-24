# MCFG availability and post-ELF startup review

Offline comparison after [IMG_9711](sunfish-registry-tftp-identity-IMG_9711.md), 2026-11-23. No production behavior, file contents, transport policy or boot image changed. No device commands ran. Inputs, selected original journal lines, hashes, feature checks and focused test results are retained in private `registry-mcfg-post-elf-review-v1/review.json`; the corresponding `review.py` reproduces the checks from the repository root.

## Missing MCFG is not an established WLAN prerequisite

The original successful Linux cold boot, in `cold-wifi-20261025T200752Z/journal-final.txt`, shows:

| Linux journal observation | Time, seconds |
| --- | ---: |
| Initial server-check WRQ | 177.023110 |
| First WLAN-image request, vendor fallback | 178.730030 |
| Canonical WLAN-image request | 178.734089 |
| Full 3,751,004-byte WLAN-image read request | 178.751234 |
| ath10k firmware information | 184.770318 |
| First MCFG RRQ; file explicitly absent | 187.027163 |
| First MCFG WRQ | 187.028831 |
| Subsequent MCFG RRQ | 187.032197 |

The first MCFG read is **8.297133 seconds after the first WLAN-image request**. Its log explicitly says `failed to open mcfg.tmp: No such file or directory`. WLAN firmware had already been requested and ath10k had reported firmware information while that file remained absent. This is stronger evidence than merely finding no MCFG entry in the shorter QRTR-header capture. It rules out treating an already-created MCFG file as a demonstrated requirement for initial WLAN startup in this reference boot. It does not establish every modem configuration's dependencies.

The saved notes already recorded this ordering on November 9. This review checked the underlying journal again instead of promoting the latest missing-file response into a new root cause. The later MCFG read/write lifecycle can still matter to modem configuration independently of initial WLAN startup.

## Linux and native file semantics

Pinned `tqftpserv.c::handle_rrq` opens the translated path read-only and sends TFTP ERROR 1, `file not found`, if the open fails. `handle_wrq` uses `O_WRONLY | O_CREAT`; `translate.c::translate_readwrite` maps names into a temporary directory. Consequently an admitted write creates a directory entry before the transfer completes. The later Linux read need not wait for an atomic completed-file publication. The journal alone does not establish which bytes were visible to that reader.

Native has a real bounded RAM MCFG implementation, rather than an unconditional missing-file policy. It stages one writer, publishes the file after final DATA, preserves the previous committed version during a replacement, and serves reads from a per-peer snapshot. Before any commit, the supported MCFG RRQ falls through to the same 19-byte ERROR 1 body. Thus IMG_9711's MCFG error is consistent with no committed RAM file when the reply was prepared. A retained reply can be published later; its publication timestamp does not itself measure current file state.

The atomic-publication difference from Linux is real. There is no current evidence that it blocks the initial WLAN path, and the successful Linux ordering does not support pre-populating a guessed MCFG file or changing native publication semantics as a WLAN fix. Historical IMG_9444/9445 admitted an MCFG write without committing a file; IMG_9445 observed its OACK completion but no same-peer DATA. Those are different boots, not the current file state.

## Startup sequence and what changed in the evidence

The successful Linux locator reference has a complete first server-check exchange, then negative ruleset/modem_efs reads, then WLAN firmware reads and WLFW discovery. The likely ELF exchange is an incoming 60-byte QRTR packet at GLINK time 138.272783, an outgoing 88-byte reply at 138.273077, and matching GLINK completion at 138.273157. The first WLAN RRQ socket receive follows the latter by 691.852 microseconds. This ELF association uses service-name logs, sizes, order and intent identity; it is not a captured QMI-payload match. Locator printk/journal timestamps are not interchangeable with the GLINK/socket monotonic timestamps, and no mixed-clock causal deadline is derived here. Five original reference artifact hashes were rechecked against the saved summary.

Native IMG_9593/9595/9596 and IMG_9604 previously reached ELF and/or completed a later server-check write, but then observed MCFG reads with no WLAN request. That was already a known divergence; another generic MCFG-only probe would repeat it. Older native summaries stating that neither early ruleset nor modem_efs reached dispatch apply to their particular boots.

IMG_9711 now identifies a ruleset RRQ and ERROR response before ELF completion. This is progress in the observed startup path and makes the older MCFG-only description unsuitable as a universal account of current behavior. It still shows no WLAN RRQ/WLFW. Its two MCFG RRQs and ERROR response also occur before ELF publication. They cannot be labeled the first post-ELF requests.

The latest header window freezes at transaction-3 completion, before ELF queues. Live ELF scalar clocks subsequently show the locator reply completing, but do not extend the header window. Older boots cannot supply the missing current post-ELF records. This is the specific remaining observational gap.

## Current host pieces and offline verification

The delivered feature closure includes Linux-compatible nested string lengths, the six-domain SM7150 topology, avs/audio mapping, RAM MCFG handling, the shared WLAN firmware dispatcher and firmware close handling. The native `kernel/elf_loader` and `wlan/fw` mappings both name `msm/modem/wlan_pd`, instance 180, matching the pinned Linux `mpss_wlan_pd` map. This is a locator answer, not a host command that starts the domain. The reference bring-up script waits for WLFW before loading ath10k; it supplies no missing WLAN-PD cold-start restart operation.

A freshly compiled owner harness using the delivered feature list passed two MCFG tests, seven service-map tests and three combined firmware-dispatch tests. Six fresh standalone MCFG storage/transfer tests passed: **18 focused tests total**. The owner test completes WRQ/OACK/DATA/final ACK and a subsequent read through actual GLINK/owner routing; combined tests cover a multiblock firmware read, shared-port routing and a dispatch fault. Relevant eight native source/test files match the delivered image source pins. These synthetic peers do not model the modem's private startup decision.

The combined fixture initially overflowed the default host test-thread stack when invoked directly. Restoring the existing harness setting `RUST_MIN_STACK=16777216` made all three tests pass. This is a host invocation issue, not a new Pixel stack fault or a replacement for the delivered target stack audit. The initial failure log is retained alongside the configured run.

## Next hardware discriminator

There is no evidence-backed MCFG or writer-priority fix from this comparison. The next useful current-boot observation is a whole-startup outcome panel using existing retained data: original server-check peer/DATA/final ACK versus the first completed peer; exact ELF body-match/publication/completion; the first service request accepted after ELF completion with opcode/path/peer; MCFG writes/present length/read completions; and WLAN/WLFW state. First-post-ELF observation means host acceptance after completion, not proof that the modem generated the request afterward.

Existing `elf_exchange` / `first_tftp_request`, original server-check traces, final-ACK trace, read inventory and `mcfg_progress` already retain these fields. Reusing them can answer whether the current faster boot still follows the older MCFG-only branch, or whether a different request/transfer fails after ELF. It should preserve the same grant order, FIFO checks, credit accounting and firmware source. A longer packet capture should be added only if that outcome leaves a named exchange unresolved.

This offline review is complete. No new image has been built, and no further handset command is required for the comparison itself. A new current-boot post-ELF observation will require a separately prepared and verified display/image; the delivered transaction-3 header image cannot answer it by rerunning the same capture.
