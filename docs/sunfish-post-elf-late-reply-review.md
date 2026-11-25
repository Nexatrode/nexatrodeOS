# Offline reconciliation of IMG_9728's two MCFG OACKs

The saved photo and [decoded result](sunfish-post-elf-outcome-IMG_9728.md) show two45-byte OACKs on5009 to4A, one before the captured request's scalar queue clock. The current post-ELF observer binds the first MCFG RRQ **accepted after ELF completes**, not necessarily that peer's first RRQ. This distinction is enough to explain the apparent inconsistency without a regressed clock or corrupted packet identity.

## Reproduced through the actual Owner

A fresh private fixture uses source identical to the delivered POST ELF OUTCOME implementation, its complete feature closure, actual Session/Adapter/Owner/FIFO code and actual RAM MCFG server. Three additional characterization cases run beside four existing cases: **seven passed**. These are synthetic schedules, not reconstructed phone packets or timing predictions. Sources, cases, logs, input hashes and reproduction helper are retained in private `post-elf-late-reply-review-v1/`.

The actual Owner case performs:

1. Accept ELF but leave its reply incomplete.
2. Accept an original MCFG RRQ from4A. It cannot become the observer's post-ELF target.
3. Complete ELF, then accept another identical RRQ from4A. This distinct request becomes the target.
4. Process the valid4A DEL_CLIENT before either MCFG request is dispatched. Owner remains live and both requests remain held.
5. Serve the older request. Its OACK appears in the journal, while the captured retry's server-result and queue clocks remain absent.
6. Complete that publication, then serve the captured retry. The RAM server returns the same initial OACK on the same transfer port, since its read has not advanced.

The final target is **BF**, with zero queue waits, no client ACK/ERROR and target repeat count0. The original OACK's journal time precedes the retry's scalar queue time. The two QRTR reply datagrams are identical, but they belong to distinct retained request handles and transport publications. The captured target's server dispatch/queue/publication/completion remain consistently ordered. A separate Core test exercises the same boundary with explicit request identities and clocks.

This reproduces the observed pattern, including the zero repeat count: the older request is before ELF completion and therefore before target selection. It is an evidence-backed possible explanation, not proof that these exact two phone requests had that relationship. IMG_9728 omits25 middle records and displays no OACK request identifiers. We cannot recover that missing identity from its port and payload length alone.

The selected reusable-receive path allocates a fresh private datagram identifier when a wire intent is returned for reuse; `tftp_index` checks the current datagram identity and state. A reused GLINK intent number alone is not evidence that the opaque TFTP handles aliased. This review found no demonstrated clock or handle-alias defect requiring a timing-trace fix.

## A real close-handling gap

The replay also establishes that generic MCFG request/response state can survive a validated client close and be served afterward. Current cancellation paths protect the exact first server-check retry and the WLAN firmware server. They do not provide generic RAM MCFG close cleanup. MCFG reads preserve their per-peer initial state and transfer port, so the later retained RRQ can still obtain another OACK after close.

This is a native cleanup gap, not evidence that a published GLINK buffer can be discarded. A future cleanup must distinguish held datagrams, unpublished retained replies, published transport owners and the exclusive RAM file/transfer owner. Held requests still require normal RX_DONE publication; published replies retain their actual intent and credit accounting until real completion. Canceling a staged MCFG writer must preserve the previously committed file. Generic deletion by peer port or fabricated receipts would be unsafe.

These constraints need a bounded offline implementation and fault/backpressure tests before another image. There is no need to repeat the current photo to establish the gap.

## mbn_hw.dig differs from Linux, but is not an established WLAN prerequisite

An actual Owner test feeds a93-byte RRQ for `/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig`. The WLAN decoder rejects that name because it is outside its three firmware-path aliases; the combined service therefore reaches the builtin server-check/MCFG server. That name is also absent from its restricted missing-path list. The server returns `UnexpectedRequest`, Owner consumes/releases the request and reports Ignored, with **no TFTP reply and no transfer allocated**. That error is a host dispatch outcome, not an ERROR packet sent to the client.

Pinned `tqftpserv.c::handle_rrq` instead calls `tftp_send_error(sock, 1, "file not found")` after a failed translated open. The saved successful Linux cold journal explicitly rejects this exact path at187.041263 and shows subsequent retries. Linux's first WLAN firmware request was178.730030, **8.311233 seconds earlier**. The earlier notes already documented the Linux missing-file/retry sequence; this review adds the actual native no-reply demonstration.

Thus an exact, bounded negative reply would close a real behavior gap without fabricating file contents, but it is not established as the missing trigger for initial WLAN startup. Broadening the WLAN provider's whitelist or synthesizing an MBN/MCFG file is not justified by these observations.

## Outcome of this review

The apparent OACK timestamp contradiction has a reproduced request-identity explanation. There are two concrete follow-up correctness gaps: generic MCFG close cleanup and a source-compatible negative response for the exact MBN digest path. The current evidence does not assign either as the proprietary WLAN worker's root cause. IPA and ELF credit completion remain established by IMG_9728; no new WLAN/WLFW milestone is claimed. Production code and boot images are unchanged; no Pixel commands were run.
