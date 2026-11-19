# MPSS runtime stop after authentication: IMG_9222

The 2 MiB stack image reached PAS4 and reported `AUTH RECEIPT 1`,
`READY RECEIPT 0`, `SMP NOTICES 1`, `SMP ACKS 0`, and local error `0x30`.
The screen does not distinguish a rejected health term or the notification
bits that were delivered. It does not show a successful Q6-ready receipt.
The frozen manifest matches the current `pixel_mpss_auth.rs` SHA-256.

In that source, `service` calls `pixel_ipa_receive::poll_pending` and then
returns `0x30` if `healthy_with_progress` fails. The latter requires healthy
IPA receive, a quiet retained SMMU domain, verified pre-auth state and the
retained RMTFS generation. The final painter does not retain which predicate
failed. None of these is established as the observed cause by the photo.

A separate source-level routing inconsistency is a candidate, not an observed
device cause. `pixel_mpss_auth::capture` retains SMP/watchdog bits and returns
all other bits. In the runtime build, `notified` masks GLINK bit 4 from that
return before dispatching to IPA; it does not retain that bit for the later
runtime handoff. In contrast, `pixel_ipa_receive::poll_pending` obtains the
same return via `receive_badge` and sends any nonzero result to IPA's badge
classifier, where a badge containing GLINK bit 4 classifies as `Fault`.
The runtime handoff consumes `PENDING | DEFERRED`, so a robust correction must
account for both paths and preserve the GLINK notification until its owner can
service and rearm it. `capture_during_call` also checks the remaining mask
under a separate monitor policy; its behavior needs to be considered without
weakening watchdog or other fault handling.

Host reproduction: `nativepolicy_auth_split_forwards_glink_to_ipa_as_a_fault`
composes the actual auth split and IPA badge policies for a simultaneous SMP
and GLINK notification, with and without IPA/GSI bits. Every case forwards
GLINK to the IPA policy and is classified as `Fault`. The native policy audit
passed five policy groups, 28 retained-core regression groups and rejected its
four existing mutations. This test models the polled routing composition; it
does not prove that GLINK was delivered on the Pixel or exercise hardware IRQs.

Before attributing the hardware stop to GLINK, a diagnostic needs to retain
the failed health term and the observed notification masks, including any IPA
dispatcher error and SMMU fault status, without clearing or acknowledging
unowned interrupts. It must preserve the actual authentication and Q6-ready
guards. The Pixel has been returned to LineageOS; no new device run is implied
by this investigation.

## Follow-up device evidence and offline fix

`IMG_9223.heic` reports `HEALTH FAIL 1` (IPA receive), `RX ERROR 8`,
`RX BADGE LOW 0x10`, `IRQ SEEN LOW 0x38`, `AUTH RECEIPT 1` and
`READY RECEIPT 0`. The last badge is exactly the GLINK bit passed to IPA,
matching the polled routing reproduction. The cumulative IRQ mask alone does
not specify ordering; the displayed last IPA badge provides the direct link.

The runtime auth split now retains bit 4 with SMP/watchdog, instead of passing
it to IPA or masking and discarding it in the callback. SMP acknowledgement
clears only the SMP bit; the existing handoff passes the still-pending GLINK
bit into the runtime router, which rearms it after a successful boot-transport
step. Non-runtime builds retain the previous split. Host tests exercise the
split, coexistence with IPA/GSI, preservation across SMP acknowledgement, and
watchdog/SMMU/unknown-fault rejection. The existing runtime router tests cover
the one-shot GLINK rearm and uncertain-ack quarantine. The corrected image has
passed its offline build and CapDL/layout audits, but requires device testing.

An additional temporary host harness composed the runtime-configured auth
split with the actual runtime IRQ router: for badge `0x38`, GSI remains with
IPA, GLINK survives SMP-bit removal, the runtime router receives it, and its
modeled kernel ACK occurs once. To compile the existing standalone router
harness without enabling unrelated feature-dependent modules, the harness used
a temporary copy of the auth policy with its runtime `cfg` branch selected.
This is a CPU model of the handoff, not an execution of the root notification
entry or a physical IRQ test; no frozen image bytes were changed.
The same check is now reproducible with
`python3 platforms/sunfish/test-auth-glink-handoff.py`; it also confirms that
the old split fails the combined test. It selects the runtime policy branch
in a temporary copy for the standalone harness and exercises the real router.

## Subsequent GLINK negotiation

`IMG_9240.heic` shows `GLINK NEGOTIATING`, one GLINK ACK, runtime error zero
and remote channel zero. Entry into this runtime requires the prior Q6-ready
receipt. The user reported that the display remained unchanged. It does not
show QRTR discovery or prove the peer failed. The TX/RX record labels exceed
the display's 15-character hex-label limit and therefore were not rendered.

The root calls the opening transport once at handoff and once per later
notification. Each `BootTransport::step` performs at most one FIFO operation.
The host regression `linux_transport_peer_version_requires_a_later_step_to_send_reply`
demonstrates that receiving a peer VERSION consumes the record and queues its
ACK, but does not send that ACK until another local step. No new peer event is
needed for that next step. Thus an interrupt-only caller can leave a reply
pending if no further notification arrives. This is a demonstrated software
liveness gap, not proof of the exact peer record present on this device run.
The current deadline is checked only inside `step`; without another event it
cannot independently update the stalled screen.

## QRTR runtime result

`IMG_9241.heic` shows Q6-ready, QRTR service runtime, RMTFS bound and
`QRTR ANNOUNCED 0x05`. In `qcom_qrtr_control::status`, that field is a
bitmask, not a count: bits 0 and 2 correspond to local RemoteStorage and
ServiceRegistry announcements. The control plane sets an announced bit only
after the corresponding complete GLINK datagram is published; this does not
prove the peer consumed it. The screen then shows runtime error `0x21`. Since
RMTFS was already bound, the `promote` path had reached the first service-owner
pass; the image collapsed that pass's error to `0x21` and rendered only whether
the owner had an error. A subsequent `owner.fault()` also overwrote the first
owner error with `Stopped`. The next diagnostic preserves that first error and
prints the underlying service-stage code and owner/router error text, without
altering the fail-closed hardware policy.

## Client withdrawal evidence

`IMG_9245.heic` identifies DEL_CLIENT (command 6), peer port 10, one
RemoteStorage (`0x0e`) OPEN request (message 1, transaction 1) in ReplyQueued
state, with its inbound GLINK RX_DONE no longer pending. This does not show
whether the reply was fully published or whether its own GLINK and QRTR
completion signals had arrived.

The saved working Linux cold-boot journal at
`.cache/sunfish-mobian-comparison/device-runs/cold-wifi-20261025T200752Z/journal-complete.txt`
shows a modem request from node 0 port 10 opening `/boot/modem_fs1`, followed
by `del_client 0:10`; Linux then opens the other modem storage files and
continues. The captured reference rmtfs implementation's `rmtfs_del_client`
only logs this event and returns. Thus this particular client closure is a
normal pattern in the reference, not in itself a reason to stop the modem.

The current native session retains each reply's request ticket until both
GLINK RX_DONE and QRTR RESUME_TX, and quarantines a matching DEL_CLIENT before
either distinction is visible. The selected pinned Linux `net/qrtr/af_qrtr.c`
uses RESUME_TX to reset a per-destination flow-control counter, not as a
one-to-one QMI reply acknowledgement. A host test demonstrates that even after
a reply is fully published and GLINK RX_DONE arrives, a DEL_CLIENT before
QRTR RESUME_TX still triggers ClientGone. The same screen values could also
arise with a not-yet-published reply, so that test does not establish which
ordering occurred on the Pixel.

Any correction must account for unpublished, partially published and fully
published replies separately; retain hardware-owned GLINK buffers until the
real completion; avoid replaying storage effects; keep RMTFS file handles
bound to the peer node as in the reference; and avoid treating a stale QRTR
flow credit as completion of a new transaction when a port is reused. IPA
service receipts and uncertain publication must remain protected. No guard is
removed merely because a client closes.

## RMTFS progress after the client-close fix

`IMG_9253` and `IMG_9254` show seven fenced RMTFS peer ports, eight
requests, seven replies queued, four handles and an active request, with
no owner/runtime error. The second photo has the same displayed values.
The framebuffer is repainted only on runtime entry/notifications, so this
is not sufficient to prove which request is active or that its live state
is frozen.

The selected cold Linux journal shows four open requests, allocation, two
one-sector reads and then a 4094-sector read (port 18). If the device request
order is the same, that would be request eight, but the device screen does
not record request message ID, vectors or port, so this is a hypothesis.
A 4094-sector read needs 512 dispatches at the server's 4096-byte chunk size,
plus a final response step.

There is a source-backed liveness gap independent of that hypothesis:
`RuntimeDomain::service_owner` runs at most `WORK_BUDGET` (16) service
passes and then returns even if `Router::continuation_pending` remains true.
The router's `schedule` only sets that in-memory flag; the current system
has no timer or software continuation channel to invoke `notified` again.
Consequently a chunked RMTFS operation can remain pending indefinitely in
the absence of another hardware notification. Existing host RMTFS rig tests
call `drive()` in a loop and therefore do not exercise this native boundary.
A safe correction must deliver a real continuation or otherwise complete
bounded work with an explicit, fail-closed budget, preserving real IRQ ACK
ownership, backpressure, and fairness. A larger unbounded busy loop or a
fabricated hardware interrupt is not established as safe.

### Owner-dispatch timing follow-up (2026-10-30)

The photo IMG_9276 shows elapsed 0xA105 ms (41221), owner dispatch 0x7BEC ms (31724), RMTFS 0x613 ms (1555), IPA 0x57E ms (1406), registry 0x85B ms (2139), and measured painting 0x35F ms (863). The owner timer wraps `Router::dispatch`, which includes `Owner::notified`, revalidation, RX polling, TX publication and interrupt handling. Those values are wall-clock call durations, not CPU attribution, and final error-path capture can be included. Runtime elapsed starts at `RuntimeDomain` construction after authentication; final error rendering is not fully included in the paint total. The previous IMG_9275 showed exactly the same elapsed, RMTFS maximum and paint totals; the reason for that coincidence or reuse is not established. The new footer is visible on IMG_9276. Do not assume the timings prove causation.

The follow-up instrumentation counts only native GLINK IO calls made while `Router::dispatch` runs. It separates observe/peer poll, descriptor reads, FIFO reads, and remaining native IO, including any error handling inside each call. It neither removes safety revalidation nor changes the established modem/firmware sequence. It adds counter reads and can perturb wall-clock timings, so these totals are diagnostic rather than a baseline of an uninstrumented system. No hardware result is implied by a passing build.

### GLINK native-I/O breakdown (IMG_9277)

The owner wall time was 0x7B43 ms (31555), split into peer `observe`/poll 0x2B84 (11140), descriptor accesses 0x1204 (4612), FIFO reads 0x835 (2101), and other native I/O 0x3584 (13700). Elapsed was 0xA10B (41227); instrumented painting 0x35B (859). Those categories are mutually exclusive timed native calls inside router dispatch, and may include failure processing; they do not identify the cause or represent uninstrumented timing.

Tracing shows each `glink_sample_peer` calls the SMP2P poll. Under shared lock, its directory check used five separate allocation-list walks (IPA item 497, AP item 428, descriptor 478, TX FIFO 479 and peer item 435); GLINK I/O also has further directory checks. Linux's pinned `qcom_glink_smem.c` caches its RX FIFO pointer after first lookup, but that does not by itself establish equivalent safety rules for this host. A prototype resolves the five fixed items using two complete, matching bounded snapshots under the existing lock. A host fixture records 72 reads per batched snapshot vs 360 across five individual walks, so two snapshots require 144 reads on that fixture. This is not a Pixel timing result. Mutation and equivalence tests compare the new parser to the original; SMP2P checks pass. Cross-snapshot equality retains a stability check but changes precisely when the memory is sampled, so behavior under all concurrent peer mutations is not claimed to be identical. No hardware test yet.

### IMG_9278 and follow-up

The batched-five-item boot reached Q6 ready and QRTR service runtime, then reported the same item-421 watchdog reason, runtime error 0x73, service-pass detail 0x52, and transport detail 0xF03. Detail 0x52 locates the failing service-registry dispatch call; `pending_request_for_service` revalidates the transport even with no request, so it does not establish that a SERVREG message caused the watchdog. The owner total was 0x7544 ms (30020), elapsed 0xA10A ms (41226); GLINK acknowledgments were 3 versus 1 in IMG_9277. The runs did not expose equal work counts, so these totals are not a normalized performance comparison. The final error rendering hid the RMTFS counters; storage progress in this run is unknown.

A follow-up includes the RX FIFO item 480 in the same two locked batch scans used by GLINK's directory check, instead of doing a further independent lookup. It still compares complete checked snapshots and validates the RX bounds, overlap, identity and cursor, but it changes sampling points and thus is not claimed observationally equivalent for all concurrent peer mutations. The error screen retains the most recent server report: handle, request, reply, chunk, and active-message fields, with FFFFFFFF for unavailable report/message. These are sampled after service calls and may lag a terminal fault; queued replies are not peer receipts. Host SMP2P and GLINK tests, duplicate-RX rejection, parser mutation comparisons and target check passed. The follow-up is not tested on hardware.

### IMG_9279 and bounded RMTFS burst

The follow-up phone screen reported 8 RMTFS requests, 7 replies queued, 4 handles, 0x11 (17) chunks and active message 3 (read/write), alongside runtime error 0x73/detail 0x52 and the same item-421 watchdog reason. The saved report is the last completed host service report, not proof that queued replies reached the modem; the requested iovec direction and size are not shown. Linux's reference large read is a useful hypothesis, not an established match. Comparing a previous seven-chunk screen is not a normalized measurement.

The runtime invoked the full QRTR owner, SERVREG and IPA paths between each bounded 4 KiB RMTFS step. A proposed wrapper runs at most eight existing `dispatch_linux_rmtfs_step` calls per storage turn, stopping immediately on idle, queued reply, backpressure or error. Each step retains the existing transport and resource validation and shared-memory ownership, and the outer bounded loop still services the other handlers between bursts. The scheduling callback remains idempotent and the physical IRQ acknowledgement path is unchanged. This changes interleaving and can affect service latency; it does not prove the watchdog is caused by slow storage. Host tests exercise the eight-step limit, complete a multi-chunk request, and verify an uncertain write stops without retry. Device result is pending.

### IMG_9280 and 32-step burst candidate

The eight-step phone boot again stopped at watchdog with 8 requests, 7 replies queued and 4 handles; active message 3 remained. Its retained chunk count was 0x52 (82), versus 0x11 (17) in IMG_9279. This is suggestive of more storage progress in a comparable-looking sequence, but the screens do not establish identical iovec sizes, directions, or peer consumption of queued replies.

A candidate extends the bounded burst to 32 existing 4 KiB steps per service turn, without dropping each step's transport/resource checks. It adds read/write direction and total/remaining sectors to the error display, calculated from the validated active iovecs. As before, this changes scheduling fairness; other services can wait for 32 checked storage steps per turn, and the outer native owner has its own bounded loop. Host tests verify the 32-step bound, progress and sector counts, and fail-closed handling of uncertain copies and ownership loss. Full storage suite and AArch64 check passed; hardware behavior and timing remain unproven.

### IMG_9281 and larger RAM-shadow transfer candidate

The 32-step run again stopped at the modem watchdog with 8 requests, 7 replies queued and message 3 active. The report identifies the active transfer as a read (`WRITE=0`), 0xFFE (4094) sectors total and 0xAFE (2814) remaining: 1280 sectors, or 655360 bytes, completed in that active request. This matches the Linux reference large read in direction and size; the screen does not establish the same file, offset, or transport completion. Total host chunks were 0xA2 (162), including earlier requests.

A candidate raises the per-copy limit from 4 KiB to 32 KiB for the selected volatile RAM-shadow RMTFS path and reduces the burst from 32 to 4 steps: both bounds allow at most 128 KiB before returning to other services. Each larger step still checks the transport and retained resources, validates the full requested iovec before effects, and fails closed on uncertain I/O. The interval between lifecycle/ownership checks within one copy increases, and a failing step can affect more bytes; scheduling latency and hardware throughput are not yet measured. Durable backing retains the original 4 KiB storage cap, and legacy modem handler IO_BYTES remains 4 KiB. Host tests cover 32 KiB across page boundaries, out-of-range rejection, source preservation, both copy directions, no retry on failure or lifecycle loss, the unchanged durable cap, original userspace wire compatibility, and the target build. A complete device image is being built; no device result is implied.

### IMG_9282 and 64 KiB RAM-shadow transfer candidate

The 32 KiB-per-copy phone run stopped with the same item-421 modem watchdog, runtime error 0x73 and service detail 0x54, with 8 requests and 7 replies queued. The active request was a read of 0xFFE (4094) sectors, with 0x2FE (766) sectors remaining: 3328 sectors had completed, around 81.3%. Its size and direction match the Linux reference read, but file and offset have not been captured. The preceding 4 KiB-copy screen showed 1280 sectors completed on a request of the same reported size and direction. The new run is promising but does not by itself prove identical requests or peer receipt.

A candidate permits at most 64 KiB in one checked RAM-shadow transfer, with two steps per burst (up to 128 KiB per service turn, as before). Durable storage stays capped at 4 KiB, as does the generic modem handler. Larger individual copies mean lifecycle/ownership checks are less frequent by bytes and an uncertain effect can span up to 64 KiB; existing fail-closed handling remains and host tests exercise uncertain transfer and ownership loss. Tests include native 64 KiB page crossing, source/bounds guards, both RAM-shadow transfer directions, 64 KiB QMI server request, durable cap, wire oracle, 199 storage regressions and AArch64 build. This is not a hardware result.

### IMG_9284 and 128 KiB RAM-shadow transfer candidate

The 64 KiB-per-copy run stopped with the same watchdog and an active 4094-sector read; 0xFE (254) sectors remained, so 3840 sectors (about 93.8%) completed. The screen reports 8 requests and 7 queued replies, not peer receipt, and does not identify the file or offset. The preceding 32 KiB run showed 766 sectors remaining on a request with the same size and direction; these observations are encouraging but do not establish identical request identity.

A candidate permits one checked 128 KiB RAM-shadow operation per service turn. The maximum bytes per turn remains 128 KiB, but the single-copy failure boundary and interval between per-step lifecycle checks increase from 64 KiB to 128 KiB. Durable storage and generic modem handlers retain 4 KiB caps. Host tests cover 128 KiB native page crossings, bounds, unchanged source, both RAM-shadow directions, QMI server behavior, fail-closed uncertain copy and ownership loss, durable cap and 132 durable/pool regressions; the storage suite includes 199 regressions and original userspace wire oracle, with AArch64 target checks. A complete device image is being built; no hardware result is implied.

### IMG_9285 and two-copy candidate

The 128 KiB, one-step run again reported 8 RMTFS requests, 7 replies queued and an active 4094-sector read with 254 sectors left, the same remaining count as IMG_9284. Its chunk count (17) cannot be compared directly to the prior 64 KiB chunk count. The same per-turn byte limit is a possible explanation for the plateau, not proof of causation; the watchdog and transport fatal evidence remain.

A candidate runs up to two separately validated 128 KiB RAM-shadow steps per storage turn, at most 256 KiB before other service dispatch; the single-copy uncertainty boundary remains 128 KiB. This changes service scheduling latency and no absolute latency bound has been measured on the phone. A host integration test replays a Linux-sized 4094-sector read (sector 2, shared address +1024), verifies all bytes and a success response, and asserts at most two chunks per burst; companion tests check the bound, retained requests and fail-closed behavior on uncertain I/O and ownership loss. Host tests cannot establish the phone's request identity, modem watchdog timing, or peer receipt. Target and complete image validation remain in progress.

### IMG_9286 and large-read reply trace

The two-copy boot again reached Q6-ready and QRTR service runtime, then reported runtime 0x73/detail 0x61 and the item-421 watchdog. The screen shows 8 RMTFS requests, 8 replies queued, 0x12 (18) chunks and no active message. The eighth request therefore left the storage server's active state after a reply was queued; the screen did not preserve that response's status, whether it was published, or either peer completion. It does not establish that the modem consumed the read or that the following Linux request was seen. Linux next logged a 128-sector read, then TFTP requests, but the screen alone does not establish that this native run reached them.

A diagnostic records the first 4094-sector read reply's exact request handle: bit 0 queued, bit 1 fully published to GLINK, bit 2 matching GLINK RX_DONE, bit 3 matching QRTR RESUME_TX, bit 4 successful QMI result. Bits are latched from local transport state and events; even both transport completions do not prove the modem interpreted the contents. The trace is observational and does not add retry, change firmware startup, or enable WLAN. Host integration tests cover queued-before-publication, held acknowledgments, both completion orders, and a successful 4094-sector transfer. A phone image is under validation.

### IMG_9287 and reply timing follow-up

The subsequent device screen showed flags 0x17: the selected response was queued with QMI success, fully published, and received a matching GLINK RX_DONE. A matching QRTR RESUME_TX was not recorded before the owner stopped on the same fatal watchdog evidence. This does not prove that the peer processed the reply or that missing credit caused the fatal state. Our reply explicitly requests confirmation, but Linux's QRTR credit mechanism is flow-based rather than a per-message acknowledgment; a missing observed completion can also result from the peer terminating or the host stopping its receive loop.

The next diagnostic samples those four latched reply milestones at dispatch boundaries, measured from runtime construction after authentication, and records when a failing dispatch returns. Times are coarse first observations; multiple changes within one dispatch receive the same sample and a failing dispatch may spend time on error handling before returning. They cannot establish the modem's internal watchdog time or precise causal ordering inside that call. The image makes no change to transport flow control or the storage operations. The host unit test covers first observation, missing values and time conversion; the AArch64 target check passes. A full image build is required before any device test.

### IMG_9288 and intra-dispatch follow-up

Flags remain 0x17 and the watchdog text is unchanged. The queue was observed at 0x90FB ms (37115) and publication, GLINK RX_DONE and the failing dispatch's return all at 0x98A3 ms (39075), a 1960 ms difference from queue to that dispatch boundary. RESUME_TX was not observed. Because the three observations were recorded only after the same dispatch returned, they do not reveal their ordering within that dispatch, and the error-return time is not the modem's watchdog timestamp.

The follow-up adds an optional clock at the owner and latches times where the existing code records queue, publication, RX_DONE, RESUME_TX and the return of an owner work error. It does not alter flow-control, retries, modem boot or storage; each timestamp is a host observation, and the work-error timestamp can still include time spent below the owner before the error returns. An instrumented test checks both acknowledgement orders and first-event preservation. Hardware execution is not yet established.

### IMG_9290 and metadata-read optimization candidate

The subsequent screen showed queue 0x8F91 ms (36753), publication 0x90C4 (37060), RX_DONE 0x915C (37212), and owner work error 0x98B3 (39091), measured from post-auth runtime construction. Thus queue-to-publication was 307 ms, publication-to-RX_DONE was 152 ms, and RX_DONE preceded the work-error return by 1879 ms. The error path takes a crash snapshot before returning, so those timestamps do not establish the actual peer fatal assertion time. RESUME_TX remained unobserved, and the cause of the modem watchdog is still unproved.

Each transport operation performs SMP2P and GLINK guards, including repeated two-pass SMEM directory scans. The existing reader performs four byte-wide volatile reads for each metadata u32. A candidate allows the retained, page-aligned uncached SMEM window to use a single aligned volatile u32 read for batched-directory metadata; unaligned offsets retain the byte fallback. Bounds, lock ownership, both complete directory scans, duplicate detection, cursor checks and lifecycle validation remain in place. This changes the width and timing of individual observations under concurrent peer mutation and is not claimed equivalent for all races. SMEM is shared memory, and the pinned Linux GLINK implementation also reads descriptor words as little-endian u32; that observation does not by itself prove device timing or safety for every metadata transition.

On a static host fixture one batched scan performs 30 read operations instead of 72 byte reads; this is not a Pixel timing measurement. The optimized and byte implementations agree on valid fixtures and cached/uncached metadata mutation cases; native aligned, unaligned and end-bound reads, single-versus-batched lookup, 13 IPA SMEM tests, 39 status tests, SMP2P/native groups and AArch64 target checks pass. Full image and capability/layout checks remain required; this candidate has not been tested on hardware and could fail or expose a different concurrent observation.

### IMG_9291 and full SMP2P read candidate

The word-metadata build reached the same watchdog state and flags 0x17, with 8 requests and 8 replies queued. Queue, publication and RX_DONE observations were 0x8E30 (36400), 0x8F60 (36704), and 0x8FF6 (36854) ms from runtime construction, respectively, 353–358 ms earlier than the preceding run. A single run does not establish causation or a general speedup. Error detail changed from 0x61 to 0x53: the error was returned by the RMTFS service burst, which also calls the transport revalidation, rather than by the main router dispatch. The owner-work-only error timestamp remains unavailable on this path, so no comparison with its preceding error-return time is possible. The unchanged IO diagnostic 0xF03 and crash reason indicate the peer-fatal observation, not why the modem stopped.

Further source review found that each peer poll reads the entire 340-byte peer item twice, and the AP-owned 340-byte item is also repeatedly verified byte by byte. A new offline candidate uses the same bounded little-endian word reader for the complete SMP2P items, retaining every byte, both peer snapshots, existing barriers, lock, lifecycle and ownership checks; unaligned native reads fall back to bytes. A 340-byte aligned item becomes 85 word reads per snapshot instead of 340 byte reads. This changes read atomicity and timing and is not guaranteed equivalent under every concurrent mutation. It is a host operation count, not proof of device performance or watchdog resolution. Hardware validation has not yet occurred.

### IMG_9293 and GLINK reuse completion

The snapshot-optimized boot reached 9 RMTFS requests and 9 queued replies instead of 8. The 4094-sector response was queued at 0x842A ms (33834), published at 0x8544 (34116) and received a matching GLINK RX_DONE at 0x85D0 (34256), relative to runtime construction. The run stopped with runtime 0x73/detail 0x61 and `TRANSPORT NEGOTIATION`, IO diagnostic 0, with no watchdog text displayed. The screen does not prove which ninth request was served or that the peer interpreted its reply.

An earlier inference that the feature-bit guard was the sole possible source of live FIFO `Negotiation` was wrong: late channel OPEN parsing in `open_length` and `Channels::open` also returns that error. The selected Linux `qcom_glink_native.c` handles command 14 (`RX_DONE_W_REUSE`) without a feature-bit condition, and our session validates the actual busy remote intent and complete outstanding transmission. A targeted host regression reproduced the rejected valid completion with feature 0, then passed after removing only the FIFO feature guard; it also exercises feature 1 and rejects duplicate completion. That correction was independently source-backed, but the screenshot did not establish that command 14 caused this device's stop.

### IMG_9294 and late OPEN diagnosis

The image without the feature-bit guard again reached 9 RMTFS requests, 9 replies queued, and `TRANSPORT NEGOTIATION` with IO diagnostic zero. Thus the previous stop was not eliminated and the actual received record remains unknown. Live late-OPEN handling, including the length check in `Ring::peek` and channel validation/capacity check in `Channels::open`, can also return this error. The next diagnostic retains the existing rejection behavior and records the header, channel, advertised length, existing other-channel count and up to 24 name bytes only when one of those late OPEN checks fails. Stage 1 means the single initial length header failed, which is not a double-validated record; stage 2 means the complete record was already copied and checked twice before channel validation failed. No name or header from an unrelated error is inferred. Hardware result pending.

For the diagnostic, `OPEN REASON` is a bitmask: 0x001 invalid length, 0x002 zero channel, 0x004 missing NUL, 0x008 non-printable name, 0x010 channel duplicates IPCRTR, 0x020 channel duplicates another channel, 0x040 name duplicates another channel, 0x080 late IPCRTR, and 0x100 no free slot for another channel. `OPEN OTHERS` is the number of tracked non-IPCRTR opens. Stage 1 data is based on one unvalidated header and cannot prove a persistent peer record. Valid frame completion, FIFO tail and error handling are unchanged.

### IMG_9295 and bounded channel capacity

The diagnostic reported stage 2 (full double-checked OPEN), channel 0xA, length 6, name `DATA3`, reason 0x100 and 8 tracked other channels. The stop is therefore the fixed eight-entry non-IPCRTR channel table filling, rather than the reusable-completion guard hypothesized earlier. The reference Linux GLINK driver stores remote channel IDs in a dynamically allocated IDR; it does not impose this eight-entry table. This observation does not establish that `DATA3` is necessary for Wi-Fi or that WLAN has started.

The candidate expands the statically bounded table from 8 to 64 slots. This is a finite engineering bound, not a measured maximum; an eventual 65th unconfigured open still fails closed. Duplicate ID/name checks, no ACK or buffer grant for unconfigured channels, and rejection without FIFO consumption when full remain in place. Tests fill 64 slots, verify operation past the former limit and reject a 65th. Hardware result remains pending.

### IMG_9296 and AP IPA service dependency

The 64-slot build did not report the previous channel-capacity stop; it instead reported runtime error 0x73, detail 0x54 (`service_linux_ipa_step`), with `Transport(Session(Control(Unavailable)))`, IO diagnostic 0, nine RMTFS requests and nine queued replies. QRTR ready and announced masks were both 0x05: RMTFS and service registry are present, while the AP IPA service bit 0x10 is absent. The screen does not show the number of channels tracked or prove the ninth request's identity or that the modem consumed its reply.

The actual `Session::native_runtime_profile` deliberately registers only RMTFS and enables the Linux IPA client. The post-auth `activate_services` stages service registry and activates RMTFS, but does not call `prepare_ipa_server_endpoint` or `enable_ipa_server`. Once the remote IPA service is found, `LinuxRuntime::step` prepares INIT_DRIVER; `queue_linux_ipa` requires `control.accepts(Service::IpaServer)` and otherwise returns `Unavailable`. The selected Linux reference `drivers/net/ipa/ipa_qmi.c` initializes and registers its AP-side server before initializing the modem client and adding its lookup. Together these observations strongly identify the missing AP service setup as the cause of the observed local error; no extra phone run is needed to reproduce the composition gap.

This is not permission to assert IPA readiness. `Owner::enable_ipa_server` requires a generation-matched `ApReadyPermit`; `sunfish_ipa_post_init::Runner` defines a proof based on ordered, trusted post-init operations and a validated runtime snapshot. Its concrete production backend is not wired into the current native startup. The retained `pixel_ipa_receive::Runtime` proves the narrower tested receive/UC handoff and is explicitly not a production-ready permit. Further work must map each actual retained, same-generation IPA owner and its hardware evidence to the matched reference's server-readiness requirements, implement any missing stages, then stage and announce the endpoint only with genuine readiness. The Lineage-derived `sunfish_ipa_post_init` contract and the Mobian/Linux QMI profile have different source lineages and must not be treated as automatically interchangeable.

Two host-only composition tests now reproduce both sides: the actual runtime session without a registered host IPA service rejects the INIT request as unavailable, while a test-only synthetic readiness permit plus explicit endpoint preparation and announcement lets the same runtime profile queue INIT_DRIVER. The synthetic permit is confined to test builds and is not evidence of device readiness. The corresponding GLINK/QMI suite passed 151 tests and the owner/RMTFS suite passed 220 tests. No new device boot or production readiness assertion was made.

### Retained hardware recheck for AP IPA readiness

The selected Mobian kernel's `ipa_setup()` orders GSI setup, endpoint setup, command endpoint enable, memory setup, table setup, receive endpoint enable and default route before `ipa_qmi_setup()` registers the AP service. The native receive owner's existing platform guard checks retained power/SCM, authenticated IPA firmware, completed GSI/endpoint/command/memory/table owners and their acknowledgements, prepared buffers, the live configuration and route, GSI masks, channel and event context words, and fault status. The receive startup state also requires the RX channel running, route set, buffers published and final snapshots complete. These are related source-backed facts; they are not by themselves a proof that every Linux setup obligation or lifetime has been covered.

`sunfish_ipa_linux_receive_dispatch::Runtime::validate_retained_setup` now repeats the existing live platform guard on the retained owner and requires the RX and route state. `pixel_ipa_receive::validate_runtime_setup` constructs that guard using the actual retained startup owners and a borrowed runtime SMMU adapter; a failed or missing guard input marks the receive state terminal. The runtime takes the actual Q6-ready receipt's generation, rechecks that retained setup through the live gate, stages the AP IPA endpoint before control starts, and announces it only using the resulting permit. The hardware owners remain held by the same runtime. Host tests verify the guard's success and failure, zero/stale-generation rejection and no writes by the guard; the dispatch regression/mutation harness and AArch64 no-std check pass. The concrete startup binary also passes an AArch64 cargo check.

The Linux-profile permit is separate from the Lineage `ipa3_post_init` proof and is obtained through the live guard, rather than a cached `Armed` flag. The source audit checks guard, endpoint preparation, control start and announcement order; the updated capability and freeze manifests describe the endpoint as potentially announced, not as a proven device result. The full image build reached the modem-auth regression but failed when the filesystem ran out of space. No complete new image or device result exists. Re-run the full build and audits after freeing disk space; the physical readiness, service announcement and modem behavior remain untested.

### IMG_9297: AP IPA announced; peer watchdog persists

After space was freed, the full build passed, including 151 GLINK/QMI and 220 owner/RMTFS cases, 280 rejected capability mutations, and the image-layout audit. Frozen temporary image SHA-256 `340ce4f01014030907b704316f76201fc66d8aab0fb9f59d7e0713e482426316` was booted once on verified Sunfish. User photo `IMG_9297.heic` SHA-256 `cda18b5875617a05a3e9e6f92428408ef48e4e9884ceb51d7b263721f4f6a950` shows QRTR ready and announced masks both `0x15`, including the AP IPA bit `0x10`; this contrasts with `0x05` in IMG_9296. Q6 ready is 1 and RMTFS reports nine requests and nine replies queued. `IPA CLIENT 1` only means the Linux IPA client object exists; it is not a completed QMI handshake.

The runtime subsequently stopped with `0x73/0x61` during router dispatch, IO diagnostic `0xF03`, reason flags `0xF` and the peer watchdog text `SLAVE2 DOG DETECTS STALLED INITIALIZATION`. The monitored large-read response flags remain `0x17`: queued successful, published, and matching GLINK RX_DONE recorded, with no matching QRTR resume observed before the stop. Host event observations are queue 34106 ms, publish 34388 ms, RX_DONE 34528 ms and error return 39118 ms from runtime construction; these do not establish the peer watchdog assertion time or causation. The screen does not show IPA protocol milestones, the peer's interpretation of the reply, or Wi-Fi startup. The new announcement was reached; whether the IPA exchange completed and what caused the watchdog remain open.

### IMG_9298: IPA client had not established a peer session

The follow-up image SHA-256 `0af34d1aa17ac3abf351757445ec4f73cda08d17a31b454f254692d8566931bf` passed the full build and was booted once. User photo `IMG_9298.heic` SHA-256 `928d51f51c238b4b054883c6b41a859689596ee678ddabc7f3d94fcb6e651468` shows `IPA CORE 0`, `IPA FLAGS 0`, `IPA COMMITS 0`, and `IPA RESPONSES 0`, alongside ready/announced masks `0x15`, nine RMTFS requests and replies, and the same `0x73/0x61`, `0xF03` peer watchdog stop. The last recorded IPA report has no `ColdSession`; in this implementation the peer must be discovered and the session created before `INIT_DRIVER` can be prepared, so the recorded client did not get that far. This observation does not prove the remote service never appeared during the failing dispatch, or why it did not appear in an earlier completed service check.

The large-read trace again shows successful queued response, publication and GLINK RX_DONE (`0x17`), with no observed QRTR resume before the failure. The working Linux journal records its 4094-sector read at approximately 176.806–176.811 s, a subsequent 128-sector request at 176.956–176.957 s, then TFTP requests. These observations motivate comparing host scheduling, shared-memory and storage/QRTR servicing against that reference; they do not prove slow host processing caused the watchdog, that the peer consumed the native response, or that an IPA service was expected to appear at the same point.

### Comparison after IMG_9298

The pinned Mobian Linux `net/qrtr/af_qrtr.c` uses flow control keyed by destination node and port. For a newly allocated data flow, `qrtr_tx_wait` increments a pending count, requests `confirm_rx` at its low watermark of five, and blocks further sends at the high watermark of ten until the flow resumes. The native `Session::queue_reply` currently sets `confirm_rx` on every reply and keeps its request ticket until both GLINK RX_DONE and a matching QRTR RESUME_TX or the separately handled client-close path. This is a concrete host behavior difference, not evidence that the modem rejects the packet or that the missing resume caused the watchdog. A reference-matched change would need bounded per-destination counters, backpressure and port-reuse handling, correct retirement after actual GLINK completion when confirmation was not requested, and failure tests for partially published packets; simply discarding the QRTR wait or treating RX_DONE as application receipt would be unsafe and unsupported.

The Linux comparison journal reports its TFTP service started before the modem was booted, followed after the large read by a 128-sector storage request and then a TFTP write request for `server_check.txt`. The current native runtime explicitly neither stages nor advertises TFTP, and IMG_9298 shows only the number of queued RMTFS replies, not the identity or completion of its ninth request. Thus missing TFTP availability is another concrete difference, but neither the image nor this log proves the watchdog was caused by it or that the native peer reached a TFTP request. A TFTP observer advertises a service and can alter peer behavior even if it never sends file bytes; it must be described as such, not as a passive read-only observation.

### Experimental reference-based RMTFS flow accounting

An opt-in `sunfish-qrtr-rmtfs-flow-experiment` host-tested path now counts QRTR DATA sends per destination, requests confirmation at the fifth pending packet, and blocks at ten until a validated flow resume. Storage replies retain their GLINK buffer and router ticket until actual GLINK RX_DONE, but no longer require a QRTR resume as a per-reply acknowledgment. A close is still fenced for the rest of the boot and an uncertain write still quarantines the session. When several transmits share a destination, staged sequence numbers preserve publication order without waiting for a completed packet's GLINK acknowledgment. The table is bounded to 64 destinations and fails closed on exhaustion, which is stricter than Linux's dynamically allocated table.

The counter is shared with other service sends to the same destination. Other existing service paths retain their previous forced-confirmation and completion rules where applicable; therefore this is a targeted RMTFS experiment, not a claim of complete Linux QRTR equivalence. `platforms/sunfish/test-native-qrtr-flow.py` compiles the exact pinned Linux `qrtr_tx_wait` function into a host oracle and compares its thresholds to the accounting tests, then runs the actual GLINK/QMI and owner/RMTFS suites with the experiment enabled. Tests exercise the fifth/eleventh sends, real GLINK buffer lifetime, failed or uncertain publication, closed-port fencing, mixed services and storage backpressure without duplicate I/O. The experiment remains opt-in. Neither the old missing resume nor a timing difference establishes why the modem watchdog fired.

### Experimental flow result: IPA peer discovered, then capacity stop

The frozen experiment image `ee053a1de8d361920854810dfde193ed920f6cf79b7ec4a5f8aa4fec8c0bb2fc` passed the full build, 280 rejected capability mutations and image-layout audit, and was booted once. User screenshot SHA-256 `15f161ae33e5b4aae1bf569911e5a2e1e40e25f087544052102be40f1f18afc7` shows `IPA CORE 1`, flags `0x703`, commits 0 and responses 0. The recorded core indicates the client discovered a modem IPA endpoint and created its protocol session; flags record pending and queued work, initial-boot state and subsequent failure. No committed IPA publication or accepted response was reported. The preceding image showed no core. The display then reports runtime `0x73/0x61` and `Transport(Session(NoCapacity))`, IO diagnostic 0, nine RMTFS requests and nine queued replies. It does not show the earlier watchdog text, and this screen alone neither proves the peer remained healthy nor identifies which capacity was exhausted.

`NoCapacity` can originate from several distinct resources, including the peer's GLINK intent inventory, local receive/transmit slots, in-flight requests or experimental flow tables. Increasing any one of them from this screen alone would be speculative. A follow-up capacity snapshot records the receive/publish phase, inbound command and parameter when applicable, and bounded resource counts after the failed operation; the FIFO test confirms an overfull peer-intent batch leaves RX tail unchanged while preserving that diagnostic. This is an offline implementation and has not yet been run on the Pixel.

### Capacity snapshot: four admitted QMI requests

The capacity diagnostic image `60b341dc72b033339f390bffa617cac0295c3d3008951baf9f3e5538246057b8` passed the full build and was booted once. The user screenshot SHA-256 `4a47fa17efc9e0ec733cac1bc38067aa9da8111db9bddba9de2127d7f7d555c0` shows receive phase 1, GLINK command 9 (DATA), intent `0x35`, zero free RX slots, six free TX slots, two free remote-intent entries, four in-flight QMI requests, 11 flow entries, one active flow and nine retired peers. `IPA CORE 1`, flags `0x703`, no IPA commits or responses, and `0x73/0x61 NoCapacity` remain. The image reports counts after the failing receive; it does not expose the rejected QMI message's identity or exact branch.

The ordinary routed-request path explicitly rejects when `router.in_flight() >= MAX_REQUESTS`, with `MAX_REQUESTS = 4`. The measured four in-flight requests hit that exact bound, while six free TX slots make simple TX-slot exhaustion inconsistent with this snapshot. Special receive paths must still be checked before calling this definitively the failed branch. A bounded deferred-receive design is being evaluated: retain a complete inbound request in its existing RX intent without acknowledging it, consume the already-copied FIFO record, continue processing control/completion records, and route the held request only when capacity becomes available. This requires preserving arrival order, client-close and reset handling, and no replay on partial or uncertain publication. No new hardware behavior is authorized merely by this diagnostic and no new image has been tested.


### Bounded deferral experiment: runtime stopped on receive notification

The frozen experimental image `95e460d5d6b47229d881a6d8e2f3e88fc23bdb87fa10d6ac658ed6fcf06da20b` was temporarily booted once. The first live screen showed Q6 ready, no runtime error, eight storage requests and seven replies with one active; the later photo (SHA-256 `0ae6ac31add3a172e8d3ffc70182bd8a6929e9a5a00d4f3b89557203998c97c3`) shows nine requests and nine queued replies, Q6 ready, IPA core present, flags `0x503`, no IPA commits or responses, and `RUNTIME ERROR 0x70`, detail zero. The previous `0x73/0x61` NoCapacity screen was not reproduced in this observation.

In `pixel_mpss_runtime.rs`, `0x70` records a failure dispatching a non-router notification into the IPA receive path. The call site discards `dispatch_receive`'s returned detail, so the zero displayed is not proof of the underlying error; the observed screen also does not prove that the new deferred path was exercised. Preserve these distinctions when investigating the original notification and receive error offline. The screen displays `STOPPED`; no Wi-Fi or network frames are established.


### IPA receive-notification failure captured

The one-shot diagnostic `73d240bc94c721ae826f3e54bea099a1191390983f8ef4f4f6ecee1102e0e1ea` produced IMG_9300 (SHA-256 `7d1590a511c068c1139cda6a465889b2abf68cfcc65761622d3eab7c678672ce`). It reports runtime `0x70`, dispatch scope `0x11`, IPA notification badge `0x4`, native receive wrapper `0x4a`, and nested `Receive(Guard)`. Q6 was ready, IPA core was present with flags `0x503` and no commits/responses, and the last report recorded nine RMTFS requests and nine queued replies. The code maps badge 4 to an IPA interrupt. The guard covers retained prerequisites, power/security state, IPA configuration, SMMU, GSI/endpoints and fault state; this screen does not identify which guard predicate failed or demonstrate that a hardware fault occurred. The next useful offline work is preserve the first failing guard stage and observed/expected metadata without introducing extra register reads or loosening validation.


### First failed IPA receive guard comparison

The temporary `13defcc58e12404744ce23c5ca936cf27e3291686f6fecdef9127c18d2e613fa` image stopped with runtime `0x70`, detail `0x11`, guard stage 3, IPA space 0, offset `0x44`, actual `0x00000000`, expected `0x30000000`, mask `0xffffffff` (screenshot SHA-256 `1516a128da6cdbcac85a8153dc4db3735f2eda3886f80a29fd1d924699d83854`). This reports the first mismatch in the already-read IPA configuration snapshot, not necessarily the only mismatch, and it does not prove when or why the register changed. The same screen reports Q6 ready, 9 RMTFS requests and 9 queued replies, IPA core present and zero IPA commits/responses.

The pinned Mobian kernel reference `drivers/net/ipa/ipa_main.c`, `ipa_hardware_config_clkon`, writes IPA v4.2 `CLKON_GLOBAL | GLOBAL_2X_CLK`, whose fields are bits 28 and 29 in `reg/ipa_reg-v4.2.c`. Thus the expected value is a source-backed initialization workaround. The source and this screenshot do not establish that the subsequent zero is normal, nor who changed it. Do not simply relax the guard or rewrite the register before establishing surrounding retained state and lifecycle.


### IPA configuration snapshot: two differences

The one-shot `f8e380057282bfa71ba7b917d51042b0b954a3249a1d81e8c7d803bc9991c928` image stopped with guard stage 3 and config mismatch mask `0x14` (IMG_9302 SHA-256 `d96c5d879006d42fcf033603ade6ca67fb09b8e5d05e0a7fe033c074ad9fb654`). Of the nine compared configuration registers, bit 2 is CLKON (actual `0`, baseline `0x30000000`) and bit 4 is QSB_MAX_WRITES (actual `0x78`, configured baseline `0x08`). The seven other compared config registers matched. CLKON's last successful snapshot was `0x30000000`; there were `0xd72` (3442) successful config snapshots before failure. This does not report a timestamp, identify the writer or establish whether differences appeared simultaneously.

The pinned v4.2 register definition splits QSB_MAX_WRITES into bits 3:0 (QMB0) and 7:4 (QMB1); `0x08` to `0x78` preserves QMB0 and changes QMB1 from 0 to 7. The pinned kernel initializes QSB_MAX_WRITES in `ipa_hardware_config_qsb`. A peer or shared ownership is a hypothesis, not established by this data. Do not overwrite or loosen either comparison without identifying the relevant lifecycle, reference behavior and side effects.


### Active-only Linux reference comparison prepared

The pinned reference's `ipa_hardware_config_clkon` sets bits 28/29 for v4.2, and `ipa_hardware_config_qsb` writes the QMB limits during host configuration. Its teardown source does not show a host write that deliberately clears CLKON; neither source proves whether modem/firmware changes these registers. The successful reference log records IPA setup before the MPSS powerup. The existing log does not include these register values after the modem starts.

Built a separate reference-kernel module `sunfish_ipa_config_readback.ko` (SHA-256 `8978901e9718fa64be64ccd915efd332b6db9689869822cdacc3087d0be61972`, vermagic `7.1.0-sm7150+`) from `platforms/sunfish/mobian-comparison/ipa-config-readback/`. It verifies the Sunfish board, exact IPA resource and bound driver, takes an active-only runtime PM reference, and reads offsets 0x44 and 0x74 twice without register writes or waking a suspended device. The source-scope tests pass. A RAM-only copy of the prior one-shot cold-reference startup script inserts samples before MPSS start and after WLAN bringup; it keeps EFS shadowed in RAM, does not write EFS or flash, and does not require network association. The prepared script SHA-256 is `cf46e74eb252b032cb5953beb46c4a92aac3a988d74d66ae4d7e43a74f59907b`. If IPA is runtime suspended, the helper reports unavailable rather than waking it or inventing a value. No phone action or reference sample has yet occurred in this comparison.


### Temporary Linux reference comparison, November 1

One temporary reference boot was performed after verifying `sunfish` and image SHA-256 `6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d`. The first startup script stopped before loading the modem because TFTP service 4096 had not yet appeared; after it appeared, a guarded continuation was run once, without resetting the attempt marker. The before-modem active-only module sample reported stable `CLKON=30000000/30000000`, `QSB_WRITES=00000008/00000008`. The continuation started the modem; the reference reported bringup status 0, modem running and wlan0 present. The post-bringup active-only snapshot reported unavailable because IPA was runtime-suspended; a subsequent 45-second passive wait observed no active sample. Neither outcome establishes post-start register values.

The continuation script then exited 2 due to an unset `trace` variable in the continuation, after the reference bringup and after the unavailable sample. The original startup script defined that variable; the generated continuation omitted its definition. No rerun was made. Independent read-only checks subsequently verified all four original modem-storage hashes unchanged and block devices remained read-only. This is a capture-script error, not evidence of a Linux or modem bringup failure.

A separate candidate script for a single Linux runtime-PM wake, active-only sample and restoration to `auto` has been prepared and staged in tmpfs with SHA-256 `bdc4a77b09610c1f1ee5e433b328f662f789f0bc740f4e6000ddf1b62f0c4338`. Because the approved comparison explicitly promised no wake, it has not been run pending user authorization.


### Linux reference after-modem IPA values

With explicit authorization to temporarily resume IPA via Linux runtime PM, the active-only module returned stable `CLKON=00000000/00000000` and `QSB_WRITES=00000078/00000078` after reference modem startup and ath10k firmware bringup; both exactly match the native guard's failing snapshot. Before MPSS start the same Linux instance reported stable `30000000` and `00000008`. Linux then reported power control `auto`, runtime status `suspended`, modem `running`, wlan0 present, and all four modem storage hashes unchanged with the devices read-only. The kernel journal shows IPA setup complete, modem running and ath10k WCN3990 firmware/HTT initialization. This run did not associate to an AP or exchange network traffic. The previously observed continuation exit 2 was caused by its unset `trace` shell variable after bringup, not by the successful sample; preserve that limitation.

This reference strongly contradicts treating the startup values as immutable throughout this later Linux lifecycle. It does not identify a writer, establish the exact transition timing, show both fields are safe in every intermediate state, or independently prove native hardware readiness. Notably the reference after-sample used an IPA runtime resume following suspension; the native failing state was observed in its retained-power startup path. Any guard change needs a bounded, source- and lifecycle-specific predicate plus retained power/domain/fault checks and fault-injection tests, rather than an unconditional comparison bypass or register rewrite.


### Narrow post-modem IPA reference experiment prepared

An explicit, non-default `sunfish-ipa-postmodem-reference-experiment` recognizes only the exact Linux-observed pair (`CLKON=0`, `QSB_MAX_WRITES=0x78`) against startup baseline (`0x30000000`, `0x08`), and only with the retained same-generation Q6-ready receipt, running receive channel and default route. The actual modem owner is revalidated before the allowance is used, and a simultaneously captured router failure disables it. The pair is normalized only for the existing comparison; no register write is added. All other config, power, SCM, firmware, SMMU, endpoint, GSI and fault guards remain unchanged. A `PAIR ACCEPTED` counter appears after the allowance is used.

The reference sample was taken after Linux runtime suspend/resume and WLAN firmware startup; it does not prove the native state has identical timing or physical readiness. This opt-in is an experiment, not a general claim that these values are safe in any other state. The focused policy tests reject any other changed bit, forged baseline, missing readiness and early RX/route states; default and opt-in AArch64 checks, storage/FIFO suites and the full build, runtime source, CapDL mutation and image layout audits passed. Frozen one-shot image SHA-256 `bc17b429b9017d895bdab393be29a189993b614a71e643ec3f27a0a2eedc39b9`; it has not yet been booted.


### Post-modem reference-pair experiment on Pixel

The one-shot image `bc17b429b9017d895bdab393be29a189993b614a71e643ec3f27a0a2eedc39b9` produced IMG_9303 (SHA-256 `a7bc9f60f661077826217db968d9c99da6baae0c9143ef9e5509482aaba53169`). The screen reports `PAIR ACCEPTED 0x1c` (28 guard evaluations), so the exact reference pair was accepted in the experiment; it subsequently stopped with runtime `0x73`, detail `0x54`, `LinuxIpa(Timeout)`. Q6 ready is 1; IPA core is present, flags `0x707`, commits 0 and accepted responses 0. RMTFS reports 9 requests and 9 queued replies. Flags decode as pending, queued and held IPA response, initial boot, failed and core present. The transport only holds a packet after checking QRTR source/destination and decoding QMI kind RESPONSE and message ID INIT_DRIVER; the held packet has not yet been committed/accepted by the protocol state machine and its transaction and result are not established by this screen. Zero committed publications is a software receipt counter, not proof that no packet was transmitted.

`LinuxRuntime::step` checks a 60-second deadline on pending or init and, while a publication is queued, waits for the adapter's Linux IPA publication receipt before committing and processing the held response. Investigate which completion/credit the pending transmission is waiting on, how the Linux QRTR flow differs and whether early validated responses may safely advance without conflating GLINK buffer ownership or replaying side effects. Do not just extend the timeout, release buffers, or infer successful QMI from the held response.

### Live IPA transport snapshot, 2026-11-01

A single cold temporary boot of `b6e8552ed39e5184c4697147148eeda8650e79f4a4f5295f8b16b3e685c82c34` reached Q6 ready and QRTR service runtime. IMG_9304 showed eight RMTFS requests and seven queued replies; IMG_9305 and IMG_9306 showed nine requests and nine queued replies, IPA flags `0x507`, zero commits and accepted responses, with runtime error zero. The last two images showed no observable change. These are display snapshots, not proof of an independently running or stalled processor. No WLAN startup or network traffic was attempted.

Source inspection found `LinuxIpaReport::next_deadline` and an explicit requirement for the caller to arrange a timer wake, but no use of that deadline or timer wake in `pixel_mpss_runtime.rs`; the service is revisited on notifications. Thus a silent peer can leave the last display visible past the deadline, although this alone does not prove why this particular display did not change. The timeout-only transport snapshot was not displayed, and the run does not identify which completion is missing. The next offline build also displays the existing private-state snapshot while the response is held and its request is pending, on an otherwise normal repaint. It does not accept the response, retire the request, change the deadline or introduce polling. A real deadline wake remains separate follow-up work.

### IPA transport completion identified, 2026-11-01

Cold temporary boot `b62fd47f0d3e91d2c0ae3d6098b6b64bad9451bfd1f661a3fa995bc6f26af5d7` produced IMG_9307. The 0x8c-byte IPA request was fully published; diagnostic flags `0x15` report real GLINK RX_DONE, no matching QRTR RESUME_TX recorded, requested confirmation, and a held response. TX and raw response transactions were both zero; raw QMI result/error were zero, but the protocol had not accepted or validated that response. No WLAN or network frames were started.

The Linux reference treats RESUME_TX as resetting a destination flow counter, not a per-packet acknowledgement. The previous IPA link forced confirm on every packet and withheld its software publication receipt until a QRTR resume, unlike the already opt-in Linux flow model used for RMTFS. A new independently opt-in IPA flow experiment shares that bounded flow model and still requires actual GLINK RX_DONE before retiring the send. Host tests exercise held responses, response validation and all six handshake orders. This is a hypothesis-driven transport change, not proof that the modem will send or accept subsequent messages. The missing autonomous IPA deadline wake remains separately unresolved.

### IPA credit experiment advances handshake, 2026-11-01

One cold temporary boot of `cbd40842a65444f07a48eda0d2ca952637ce2c1452f36820285cb1cbba11f669` produced IMG_9308. It showed Q6 ready, `IPA FLAGS 0x550`, two committed publications, one accepted response, no runtime error, and nine RMTFS requests with nine replies queued. The flag bits indicate IPA protocol core present, initial boot, modem-ready from the validated INIT_DRIVER response, and indication registration accepted. They do not indicate QMI UC-ready, final indication sent, or protocol completion. The user subsequently reported no visible change; an unchanged screen does not by itself prove a live process or establish whether a later request was sent or processed. No TFTP, WLFW, WLAN or network frame was started.

The pinned `drivers/net/ipa/ipa_qmi.c` describes the outstanding initial-boot milestone as a modem `DRIVER_INIT_COMPLETE` request, followed by the AP's response and final `INIT_COMPLETE` indication once registration has also completed. Native code sets its diagnostic `qmi_uc_ready` only after the matching response publication is committed. This is a QMI protocol marker, not direct proof of an IPA microcontroller hardware state. More evidence is needed to distinguish a modem that has not sent that request from a request that is delayed, misrouted or not processed. The independently identified missing autonomous IPA deadline wake still limits conclusions from a quiet screen.

### IPA microcontroller observation, 2026-11-01

A cold temporary boot of `f932a081e0652e3208022a887fa818a65d827a5e51ecd335dd298b4037e76de2` produced IMG_9309: Q6 ready; IPA flags `0x550`, two committed publications, one accepted response, no runtime error; one retained IPA UC notification, two status reads, one init response observed, and last response value `1`. Last captured UC status was `0x01810080`. These are software report values captured by the existing dispatcher; the last status is its last drain read, not necessarily its first interrupt status. They show a UC INIT_COMPLETED response was observed, independently of the still-missing QMI DRIVER_INIT_COMPLETE processing; they do not prove the entire IPA hardware is operational. Nine RMTFS replies were queued, not necessarily persistently written or independently confirmed here. WLAN and network frames were not started.

The current IPA report does not distinguish a valid `DRIVER_INIT_COMPLETE` request queued for the owner from one not observed at that point. A candidate adds per-message counters only after a validated IPA request enters the existing owner queue. On a normal display refresh during the QMI wait it shows these counts with the existing UC report; it adds no hardware read, packet acceptance, extra polling, or readiness inference. Zero can mean the message was not received or validated/queued, not that the modem definitely never sent it. A quiet display also remains subject to the absent autonomous timer wake.

### QMI request queue snapshot, 2026-11-01

A cold temporary boot of `f00366d8e7fcaf875a626c0b92b1066844416df511866843950abd2bf00b338f` produced IMG_9310 (SHA-256 `793bb99a55701532794ef824db54b4d61abf762bb5fb03669eae885df1a5c94e`). At the displayed snapshot, IPA flags were `0x550`, two publications were committed and one response accepted, UC initialization was observed once, one validated indication-registration request had entered the owner queue, and zero validated DRIVER_INIT_COMPLETE requests had entered it. No runtime error was displayed. This does not prove that the modem never sent that request or that the display continued refreshing afterward.

Source review found the runtime intentionally suppresses ordinary framebuffer redraw on pure software-continuation notifications. Since the owner can process requests during those continuations, a snapshot can remain stale after IPA counters or protocol state advance. A candidate now repaints if the IPA report changes during a continuation while retaining the no-redraw path for an unchanged continuation. This does not arrange the missing autonomous timer, force a continuation, or establish that any later message arrived. Offline tests cover changed and unchanged continuation, error and hardware-notification cases.

### Refreshed report and Linux function-trace preparation, 2026-11-01

A cold temporary boot of `fe6fb98405b54cbd8050c94b5d52985c1f14f4154ce90413fbe8ea4309a17cec` produced IMG_9311 (`faf4d1de2961b33bb299e8cb37695651ac5eb51b771408b97e4f34644e1bb9ff`) and, after a wait, IMG_9313 (`1086232450f15d5b584ff7751d01e020291ab69beb681c739a8740db116d5735`). Both show IPA `0x550`, two committed publications, one accepted response, one UC INIT observation, one registration request queued, zero driver-init-complete requests queued and no displayed runtime error. The redraw-on-change candidate was included, but the identical images do not establish that every event was delivered or that an unchanged system was actively running. No WLAN startup or network frames were attempted.

The saved working Linux log reports modem and WLAN firmware startup but does not record the individual IPA QMI callbacks. The exact reference kernel has function tracing and dynamic ftrace enabled but not kprobes. Its pinned `ipa.ko` contains the seven selected client, server, readiness and UC entry points with patchable function entries; whether each appears in the running kernel's tracefs list remains a runtime check. A separate cold-reference startup script is prepared at `ipa-qmi-ftrace-prepared-v2`. It preserves the previously used reference startup and RAM-shadow EFS setup, checks every selected entry is uniquely provided by `ipa`, then creates an isolated tracefs instance before modem start, uses a bounded function filter and captures trace/clock/per-CPU loss statistics on exit. The existing reference script starts WLAN firmware but does not perform an association or network traffic test. Tracing changes timing and function entries do not reveal QMI payloads, callback outcomes, modem-internal activity or proof of a missing transmission. A missing symbol stops the script before modem startup. The prepared script passed baseline-preservation and shell syntax checks; no Linux boot or trace has yet been performed for this comparison.

### Working Linux IPA callback comparison, 2026-11-01

One temporary reference boot of `6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d` was performed for the prepared filtered trace. The first startup script stopped before loading or starting the modem because the first provider lookup saw TFTP but not RMTFS. Both providers subsequently appeared; all four EFS devices were read-only and unchanged, and no modem-start file or driver was present. A hash-checked continuation resumed from the unchanged script tail and performed the first modem-start attempt. It used RAM-backed firmware and EFS copies.

`device-runs/ipa-qmi-ftrace-20261102T043459Z/ipa-function-trace.txt` (SHA-256 `a24ac550b8f62ece2136ee0642baf2eb5c9e1d04134c639a45acf25647da19e9`) records, using a global trace clock, the Linux client discovery/work, registration callback, two IPA UC interrupt-handler entries, INIT response callback and DRIVER_INIT_COMPLETE server callback. In this particular trace, the first UC handler entry preceded the driver callback by approximately 0.938 ms; the INIT response callback preceded the driver callback by about 0.021 ms. These are host function-entry timestamps, not modem send timestamps. There were no recorded per-CPU trace buffer drops/overruns; tracing can still perturb timing. The UC handler's function entry does not expose which interrupt or byte it processed, and callback entry does not establish its return value or prove every possible packet was captured. Linux's reference startup exited successfully, `wlan0` existed but was not connected, the isolated trace instance was removed, and the four underlying EFS devices remained read-only with unchanged hashes.

A new host-only native owner regression replays the observed registration / INIT response / driver-request ordering with the INIT GLINK completion, then supplies the third request after a receive intent becomes available. It completes the synthetic handshake only after real simulated GLINK receipts. The IPA flow suites pass 151 link, 183 owner and 226 combined runtime tests. This narrows the question but does not reproduce the phone's absence of a queued driver request, establish exact modem timing, or prove the native physical IRQ/FIFO path cannot lose or delay a notification. The two-initial-intent limit is a real property of the native profile, not evidence the modem ignored its flow control.

### Receive-intent comparison and host stress test, 2026-11-01

The pinned Linux `qcom_glink_announce_create` supplies five 1 KiB reusable receive intents by default when a channel has no `qcom,intents` property. The saved live reference DT shows the modem GLINK edge has no child override, and Linux's channel matching falls back to those defaults when no child matches. Native currently targets two available 4 KiB non-reusable intents and can offer additional bounded slots on request. This source difference may affect timing or burst behavior; neither the trace nor the comparison establishes that it caused the missing QMI request.

A host-only test holds the registration request and INIT response in both initially offered slots, delivers a peer RX_INTENT_REQ, verifies the native session advertises another slot and grants it, then queues DRIVER_INIT_COMPLETE and completes the synthetic IPA handshake only after real simulated GLINK completions. The test also sends an oversized intent request and confirms it is denied and counted. It passes along with the experimental IPA QRTR flow suite. This does not establish what the physical peer requested or observed, nor reproduce the device's silence.

A candidate extends the existing on-screen IPA-wait report with the number of peer receive-intent requests, denials and currently locally available advertised slots. These are counters/state of the existing session, without extra hardware reads or polling. They cover the IPCRTR owner period only; an available slot does not prove the peer saw it, and a zero request count does not prove the peer sent no traffic. A normal notification or IPA report change is still needed for a fresh screen; the autonomous deadline timer remains unimplemented.

### IPCRTR receive-intent observation, 2026-11-01

A cold temporary boot of `cefd8960fee0b109d7d51b0ec1fc2aee504efe692668fec6a1a7549681ff5591` produced the screen photo SHA-256 `4941521064fb7b96baee419746b3511c28209ed8e28abba0589ed42318c30edb`. It shows Q6 ready, IPA flags `0x550`, two commits and one accepted response; one UC initialization observation, one validated IPA registration request and no validated DRIVER_INIT_COMPLETE request queued at that displayed snapshot. It shows IPCRTR intent requests `0x25` (37), denied requests `0x19` (25), and zero locally available advertised receive slots. No runtime error is displayed. The counters cover events observed by the runtime, not all wire traffic; the screen may become stale without a further notification. Neither zero available nor a denial identifies the individual slot states or whether the peer saw each advertisement.

The native session accepts a request only when its requested length is nonzero, no larger than the supported datagram, and a receive slot is free. Thus the observed denials could be due to size or occupied slots (or both at different times); this screen does not distinguish them. A cause-specific retained diagnostic and host exhaustion/retirement tests are needed before changing the slot count or treating the missing IPA request as explained. No WLAN or network frames were started.

A follow-up candidate retains the first denied request's requested byte count and the state of all eight local receive slots at that instant. The state word has one four-bit code per slot, low slot first: free 0, advertising 1, available 2, assembling 3, request 4, reply queued 5, control 6, TFTP 7, held 8, Linux IPA 9, deferred 10. This is local metadata, not proof of what the peer saw; it cannot by itself identify later denial causes. Host tests distinguish an oversized request against eight free slots from a valid-sized request against eight occupied slots; both preserve the existing rejection behavior. The opt-in IPA QRTR flow suite passed 152 link, 184 owner and 227 runtime tests, and the retained-screen source audit passed nine tests. Full build and device result remain separate.

### First denied receive intent and completion priority, 2026-11-01

The cold temporary diagnostic image `45cf5db4f284f7fedc65056e64f2054f7844338dcbc520fae294e5443032e013` produced photo SHA-256 `dcd113215eaebeccddbc6a5ebb2c88d40025e80b46507525160b8bcba5d902c7`. The first denied peer intent request was `0x34` bytes, within the 4096-byte supported limit. Its eight receive-state nibbles were `0x66664465`: five control slots, two request slots, one reply-queued slot, and no free slot at that instant. The display later reports 37 intent requests and 25 denials; it cannot identify all subsequent denial causes or establish peer receipt. The screenshot again shows one IPA registration request and zero validated driver-init-complete requests at display time; Q6 and UC init were observed, with no runtime error displayed.

Source review shows receive completion (GLINK RX_DONE) was scheduled behind both advertisements and peer intent acknowledgments. A host regression first failed when it required a pending control completion before a newly requested advertisement; the actual first publication was the advertisement. A candidate preserves advertise-before-granted-ACK but gives a pending receive completion priority while an intent ACK is outstanding. A blocked writer still leaves the completion pending, tested by retry. This is a plausible starvation mechanism and a change in transport scheduling, not proof of why the physical run filled the slots or that the candidate fixes the handshake. Opt-in IPA QRTR link/owner/runtime tests passed 153/184/227, and the legacy owner suite passed including both feature modes and seven rejected mutations. Full build and device validation are separate.

### Receive-completion priority candidate, 2026-11-01

The cold temporary image `0d0b4399da9f33353044a82a6ce704e3c6098306deb2cca308ebd720bd2faea0` produced photo SHA-256 `8186bfb4205481f54ecf95afdd6ea2f46a6df6af1a37195cd458342488490fa6`. At the displayed snapshot the runtime reported Q6 ready, one UC init observed, IPA flags `0x550`, two committed publications, one accepted response, one queued registration request and zero queued driver-init-complete requests. Intent counts were eight requests and three denials, compared with 37 and 25 in the preceding run. These are snapshots from separate boots, so they do not establish the total or causal improvement. The first denial was a 96-byte request, with state word `0xaa942445`: one reply queued, three requests, one available advertisement, one held Linux IPA response and two deferred requests, with zero free slots. The available state is local only and does not prove the peer observed or could use that advertisement. No runtime error or WLAN startup is shown. A follow-up screen was requested to distinguish an evolving run from a stale snapshot; a missing timer wake can still leave a quiet display stale.

A second photo after a wait (SHA-256 `e04f77b33f4674859eae0aa3710c31b16a28fb9c287e42d83401243b36da22de`) showed unchanged fields. This does not establish a dead modem or actively refreshed live state: the runtime has no autonomous IPA timer wake and the display can remain stale without a notification. In particular `DENY STATES` records the first denial, not necessarily the present slot ownership.

A follow-up candidate keeps the existing transport ordering and adds a pure software snapshot at screen-paint time of the eight local slot states, service owners and reply progress flags. Each word is packed four bits per slot, least significant slot first. Service nibbles are 0 unknown/none, 1 remote storage, 2 heap, 3 service registry, 4 IPA server; deferred-service classification uses the destination port and is not a payload validity judgment. Reply nibbles are bit 0 record exists, bit 1 fully published, bit 2 GLINK completion, bit 3 QRTR completion. There is no new hardware access or autonomous timer and the screen can still be stale. Host tests cover local request/reply snapshots, the deferred destination classification and the prior completion-priority behavior. The IPA QRTR suites passed 154 link, 184 owner and 227 runtime cases; deferred suites passed 118, 141 and 225; runtime-source audit passed nine. Full build and on-device outcome are separate.

### Service-registry reply credit, 2026-11-02

The temporary image `dba0b28dbc78070d55a2ce14bfe5a36052498d68f068c9af3c470d7ec0f750e9` produced `IMG_9314.heic` (SHA-256 `f6323c1c960a098d3f77cec358488d659c4efc7ca4fb63ffd3057a41a524e64a`). On its displayed update, `SLOT STATES AA44A4A5`, `SLOT SERVICE 44334333` and `SLOT REPLIES 00000007` show one reply queued for service registry, three registry requests, one registry deferred request and three IPA deferred requests. The registry reply's local flags indicate a record exists, is fully published and received GLINK completion, while QRTR completion has not been observed. This is a display-time software snapshot and can be stale; deferred destination classification is not validation. Q6 was ready, no runtime error was shown and the IPA DRIVER_INIT_COMPLETE request had not entered the owner queue. WLAN was not started.

The prior experimental path applies Linux-style destination QRTR credit to RMTFS and IPA but forces confirmation and waits on individual QRTR RESUME for service-registry replies. In the pinned Linux `net/qrtr/af_qrtr.c`, normal DATA requests confirmation at pending count five, blocks at ten, and RESUME resets the destination count; it is not a per-reply acknowledgment. That mismatch is a concrete candidate explanation for the observed retained registry reply and capacity pressure, but it does not prove why the peer sent no resume or that a correction will advance on hardware.

The separately opt-in `sunfish-qrtr-servreg-flow-experiment` applies the existing bounded destination-credit model to service-registry replies; it continues to require actual GLINK completion and leaves non-experimental behavior unchanged. A targeted host test failed on the old forced first confirmation and now passes, including same-destination backpressure before GLINK completion; another verifies the fifth-message flag, tenth-message limit and destination resume. Full opt-in host owner/runtime suites passed 185/228, and the existing IPA flow baseline and nine source-audit tests passed. A full image build and device outcome are separate.

### Routing size failure after SERVREG flow experiment, 2026-11-02

The temporary image `c2ed99b04270db860a9404fa06f1a714ebeeb74e192014f2b502029ba5172f7f` produced `IMG_9315.heic` (SHA-256 `38f7f689c22c101b192d116e9d46e356d6c21a7219f8fe244103fba31ef9e0e6`). It showed Q6 ready, nine RMTFS requests and nine queued replies, and then runtime error `0x73`, detail `0x61`, `TRANSPORT SESSION ROUTING WIRE TOO LARGE`. IPA showed one committed publication and zero accepted responses in the failure snapshot; the image did not report an IPA driver-complete request. No Wi-Fi startup was attempted. This is a different stop, but does not prove that the prior service-registry bottleneck is fully resolved.

The routing label can come from a message-specific IPA validator: a complete GLINK receive is bounded by 4096 bytes, and the Linux-profile `validate_server_request` maps its stricter per-message TooLarge error into the same routing error. The screenshot does not identify the endpoint, message or field lengths, so those must not be inferred from the label alone and limits have not been loosened.

A candidate preserves the index of a failed routing attempt and exposes only bounded QMI header metadata: endpoint service ID, body length, message ID, transaction and first two TLV type/length pairs. It does not print TLV payloads or modify validation, and reads only retained software data even after owner quarantine. A host owner test sends an oversized driver-complete request, verifies the error and retained fields, and the opt-in IPA QRTR (154/185/228), service-registry flow (186/229) and 10 source-screen tests passed. These are synthetic, not the rejected device message; full build and a new device observation remain separate.

### IPA CONFIG request and pinned Linux behavior, 2026-11-02

The temporary routing-header diagnostic `a109cf529bc7bc833dbc22cc0b856def3a98b38ba40154f9ed802be31fafc597` produced `IMG_9316.heic` (SHA-256 `53275ee58e4da955e7a73d94fc2997633bff55b243f1eab1d1500a447823c128`). Its stopped screen identifies service `0x31`, QMI message `0x27`, body length `0x3f` (63), transaction `3`, and first TLV headers `0x10`/length 4 and `0x15`/length 4. It does not expose the remaining TLVs or payload values. The Lineage IPA vendor header calls `0x27` QMI_IPA_CONFIG_REQ and gives a 102-byte maximum payload; its vendor GSI MHI handler returns success without applying the non-GSI settings. This does not establish the values of the observed request or authorize claiming vendor configuration was applied.

The pinned working Linux reference `drivers/net/ipa/ipa_qmi.c` registers only IPA indication-registration `0x20` and driver-init-complete `0x35` server handlers. Its `drivers/soc/qcom/qmi_interface.c` returns without invoking a handler or sending a QMI response for an unmatched message ID. The separately opt-in `sunfish-ipa-ignore-config-reference` candidate models that narrow behavior for the IPA server's QMI request `0x27`: it checks the QRTR envelope and complete QMI framing, discards no other ID, stages any required QRTR resume, emits the ordinary GLINK receive completion and records a count. It does not send a CONFIG response, claim configuration succeeded, start WLAN or relax existing validators. Malformed headers and duplicate TLVs remain stopped. The device's complete TLVs are not known, so compatibility remains to be measured.

A source-hash-checked reference test verifies the exact handler table and unmatched-handler return in the pinned Linux archive. Host suites with the opt-in passed 188 owner and 231 runtime tests, including a synthetic 63-byte message matching the observed first two TLV headers, no fabricated QMI response, actual simulated RX_DONE and malformed rejection. Baseline IPA QRTR flow tests and ten runtime source checks also passed. Full image build and on-device outcome remain separate.

### Unhandled IPA filter request, 2026-11-02

The temporary image `c45d7173f843c31e4b9c8749d26f4be7b1699ce9eb72ff92999c967efcfa9f7b` produced `IMG_9317.heic` (SHA-256 `cd02151ee142293e31ae39fa9b281f261aeaa7602af412839690374ea25b033a`). The screen shows `CONFIG IGNORED 1`, then a routing size stop for service `0x31`, message `0x23`, transaction 4, QMI body size `0x8d4` (2260), and first TLV `0x15` with length `0x8ca` (2250). This is consistent with the prior `0x27` stop being passed in this run, but the IPA handshake was not complete. Lineage's vendor header calls `0x23` IPA INSTALL FILTER RULE; the pinned Linux reference registers neither that ID nor `0x27` as IPA server handlers and drops unmatched QMI messages. No filter payload or installed rule was observed.

A separately opted-in `sunfish-ipa-ignore-unhandled-reference` experiment extends the previous config-only branch to other IPA server request IDs not handled by the pinned reference (`0x20` and `0x35` remain on the existing validated path). It validates the complete bounded QRTR/QMI framing, retains any required QRTR resume and actual GLINK receive completion, counts discarded message IDs, and sends no QMI response or fabricated success. It does not install filter rules, grant hardware readiness, or apply WLAN configuration. Malformed headers, duplicate TLVs and packets for other endpoints still fail normally. This deliberately models the pinned working Linux reference's unmatched-handler behavior; it does not establish equivalence to the vendor IPA service or that ignored requests are harmless in every environment.

A source-hash-checked host test verifies the pinned Linux handler list and unmatched-handler return. A synthetic `0x23` request with a 2260-byte QMI body and one 2250-byte `0x15` TLV is consumed without a QMI success response and requires a real simulated GLINK completion. Additional tests reject malformed and wrong-endpoint packets, while preserving the handled-request validation. Full opt-in owner/runtime suites passed 190/233, the prior config-only suites passed and 11 runtime-source checks passed. Full image build and an on-device result remain separate.

### IPA driver-ready milestone and invalid-reply stop, 2026-11-02

The temporary image `ceb3f475b2cb2bf4c532029fe001b93137842da4a762e1be0c193a4dc300ab99` produced `IMG_9323.heic` (SHA-256 `2bb1f5f4fa19050a8bd0e0c9a8404d55b5b0e3ceaf5737c4db64b30df7d45f69`). Its display shows one ignored unmatched IPA request with last ID `0x23`, Q6 ready, three IPA commits and one accepted IPA response. `IPA FLAGS 0x773` encodes a present protocol core, modem-ready, QMI driver/uC-ready and indication-requested, with an IPA publication pending and queued; the final init indication was not recorded as sent, and the complete bit is clear. The failure flag is set after quarantine. This proves the driver-ready protocol milestone was reached in this run, not that the full IPA handshake or WLAN succeeded. Runtime then stopped with `0x73`, detail `0x61`, `TRANSPORT SESSION INVALID REPLY`; this label can arise at more than one transport/session branch, so the failing message is unknown.

A follow-up candidate retains the slot of any failed completed receive, not only failed routing, so the existing bounded metadata display can show the rejected QMI destination, kind, ID, length and first two TLV headers when available. It also records whether a session error occurred while retrying a deferred packet (stage 1), ingesting an early buffered record (2), ingesting a current ring record (3), or publishing a session record (4), with record command and parameter where available. This is software bookkeeping only; no payload bytes, extra hardware reads, acceptance changes or retry are introduced. Host tests reproduce a rejected IPA-client reply, verify metadata remains after owner stop and identify the ring-ingest stage. The full opt-in owner/runtime suites passed 191/234 and 12 source-screen tests passed; a full build and device test remain separate.

### IPA INIT_COMPLETE response, 2026-11-02

The temporary image `8ba54b2518ec9c93846ccbe9c809f1efb429d5d0e6efca9c40018741866f5ba4` produced `IMG_9325.heic` (SHA-256 `ac40e1aa2b18c598341cc8b4aca09032f0c8ed3b9c1b26343bd097d9d085792f`). Its failure header shows a QMI response (kind 2), service `0x31`, message `0x22`, transaction 0, body length 14, a single TLV `0x02` of length 4, and destination host IPA server port `0x4002`. The failure occurred during current-ring GLINK DATA ingestion (stage 3, command 9), with runtime error `0x73` and detail `0x61`. Its sender port and TLV values were not displayed. As in the prior run, IPA flags `0x773` show the modem-ready, driver-ready and registration milestones but not final indication completion. No WLAN was started.

In the pinned Linux reference `ipa_server_init_complete` sends IPA `0x22` with `qmi_send_indication`; `qmi_send_indication` cancels its temporary transaction after sending, and `qmi_handle_message` ignores responses lacking a matching transaction. The native unhandled-request branch rejected the observed kind 2 before it could be ignored. A separately opt-in candidate tracks only the peer and transaction of a fully published host `0x22` indication and ignores a matching response from that peer on the host IPA server endpoint. It validates the QRTR/QMI envelope, does not use response contents to grant success, and retains the requirement for the real GLINK completion to commit the indication. Responses before publication, from another peer or transaction, for another message, and malformed responses remain rejected. Header-only failure diagnostics now also show source port if the new guard does not match. A Linux-source-pinned host script checks the reference send/cancel and unexpected-response paths; opt-in owner/runtime suites passed 193/236, including early and late matching responses and the negative cases; the baseline and 12 runtime screen tests passed. Full build and a new device result remain separate.

### Native IPA handshake complete, 2026-11-02

The one-shot image `0fb6229d552102279ad3ccfdd4f6ebbc9172ca879a4271535dc44f12a61873f7` produced `IMG_9327.heic` (SHA-256 `76bb6cb330a7925c2a76f8e7e4ed5e81a43f2e0fe60733cad87796cf0d68066c`). The screen shows IPA flags `0x4f8`, four commits, one accepted response, one ignored INIT response, Q6 ready and no runtime error at the displayed snapshot. The flags include the protocol-complete milestone, after actual transport completion; this does not demonstrate WLAN operation or traffic. Nine RMTFS requests and nine replies were recorded. One unmatched request was ignored, with last ID `0x23`: no successful reply was fabricated and no associated filter installation is established.

The working Linux comparison starts both RMTFS and the QRTR TFTP provider before starting MPSS (`platforms/sunfish/mobian-comparison/start-coldwifi.sh`). The current native runtime explicitly excludes the TFTP port from its pre-auth profile and its screen reports no TFTP or WLFW start. A native TFTP observer already exists in the transport, but preparing it changes the advertised service profile and must be integrated deliberately with the runtime ownership checks. The next source audit must distinguish service advertisement, request metadata observation and actual firmware serving; the IPA result alone does not prove a WLFW server is present.

### TFTP observer, 2026-11-02

One cold temporary boot of image `a2f2bd9c33f347d44f76cf8a07934df59e921418153fdb6e3e066e3d7b19a22b` produced `IMG_9329.heic` (SHA-256 `d1fd9ec9501e7a3cd5efed86ad46716fa03b93994bdb4de92334c7d5831108a0`). The displayed snapshot shows Q6 ready, IPA flags `0x4f8`, four commits, one response, and no runtime error. The observer reports its QRTR service announced, 0x12 (18) packets, zero parsed read requests, no current parse error and a last parsed opcode of 2 (WRQ), from peer port `0x56`. The screen shows no WLFW service. The last opcode and last parse status do not establish that all 18 packets were identical; the diagnostic did not display a path, and cannot identify the requested file or prove how the peer reacted. No TFTP response or file bytes were sent.

The saved working Linux journal at `device-runs/cold-wifi-20261025T200752Z/journal-complete.txt` records a WRQ for `/readwrite/server_check.txt`, followed by RRQs for read/write resources. That sequence suggests a possible next dependency, but the native WRQ path was not displayed and is not established to match. Capture its path and options before choosing a response; the metadata-only observer cannot satisfy write requests. The observer is staged after Q6 readiness but before the native QRTR service control starts. This is not proof that timing matches Linux's local provider startup before PAS.

### TFTP write-request path, 2026-11-02

The temporary path diagnostic `faca640b0695db89af015d31200491c0ae41fe2b979bc1254f4208e9400dc56b` produced `IMG_9331.heic` (SHA-256 `6cea807fb30512b4178f64467edb8a706fa60d4d8423a8ae8f8f5c6948de3903`). It shows Q6 ready, IPA flags `0x4f8`, four IPA commits, one response and zero runtime error. TFTP was announced and recorded 0x12 packets, zero parsed RRQs and a last opcode of 2 (WRQ). `SERVER CHECK 1` and the displayed 27 bytes independently identify the last parsed path as `/readwrite/server_check.txt`. `OPTION BYTES 0x25` records 37 bytes after the mode terminator, not their names, values or validation status. No TFTP response or file bytes were sent; no WLFW service was displayed. The earlier `IMG_9330.heic` (SHA-256 `735a3696da235b152b37763f7fb8f68bd8c75d1ebd343397cf5a5bfb251ff076`) shows a prior in-progress snapshot and no TFTP request yet.

The pinned comparison's `tqftpserv.c::handle_wrq` opens a translated path for writing and uses a fresh QRTR socket; if option bytes are present it parses options and sends an option acknowledgement. The Linux log shows later read requests including the WLAN image, but it does not establish that acknowledging this specific WRQ alone causes that progress. A native implementation must validate the actual option names and values, address routing, transfer state and bounded RAM-only write handling before claiming compatibility. The existing observer has not captured those option contents or authorized any response.

### TFTP write options, 2026-11-02

Temporary image `3fa0b9ddde169eac7c5f197798e76c3538349eb4c1dce3141e7f3695446e5383` produced `IMG_9332.heic` (SHA-256 `602fec30da191fd2d90457db597cf3f732bbdad2e332119ba63678c62621e4fd`). The 37 captured option bytes decode to NUL-terminated pairs `blksize=7680`, `timeoutms=1000`, `wsize=10`. The screenshot again shows the `server_check.txt` path match, last opcode WRQ, 18 packets, zero RRQs, no runtime error, Q6 ready and IPA flags `0x4f8`. This is observed request metadata, not an acknowledgement or transfer result.

The pinned `tqftpserv` creates/opens the translated file and sends an OACK from a separate connected QRTR socket containing the selected block size and timeout, and the nonzero window size; then its writer accepts DATA, writes it and acknowledges its block. It uses a 516-byte receive buffer and a 512-byte completion threshold even though this request advertises a 7680-byte block, a reference limitation that must not be silently equated with a general 7680-byte writer. The native existing read-only TFTP path denies WRQ. A restricted bounded RAM-only writer requires independently tested option validation, separate transfer-port routing, DATA/ACK lifecycle, retransmission and size limits; this observation does not prove any particular write payload or that answering this request alone brings up WLAN.

### Bounded server-check writer, offline, 2026-11-02

The opt-in `sunfish-native-tftp-server-check` profile stages a single-purpose server at `0x5000`, accepts only the observed server-check path and exact option tuple (with a case-insensitive octet mode), and sends the reference OACK from transfer port `0x5001`. It binds subsequent DATA to the same peer and holds up to 8192 bytes in private RAM; it never writes a persistent file or serves firmware. Like the reference it considers a DATA payload shorter than 512 bytes complete, but it rejects a payload above 512 bytes rather than silently truncating it. Thus the 7680-byte negotiated block remains a known interoperability limitation, not evidence of general TFTP support. Requests for other paths and invalid or out-of-order blocks are rejected without granting success. Repeated identical last DATA is acknowledged without a second write. Pending output is retained before the input slot is released when the transport is backpressured.

Four host model tests cover the observed OACK, transfer port and peer binding, duplicate and changed DATA, buffer limits, completion and oversized rejection. The owner integration test verifies actual queued QRTR bytes and delayed-credit retention; all 165 owner tests pass with QRTR flow and this feature. The full v1 image build passed; v2 adds the first rejection and packet-length screen fields and repeats the affected tests, AArch64 compilation, source audit, 280 capability mutations and boot-layout audit. Frozen temporary image SHA-256 `06b9aa57a15cc6ad00fe3b63bf92b3da828607c0c2927a5e6374b9fb28fd6e0a`. No device result is claimed.

### Server-check queue stop and bounded-yield candidate, 2026-11-02

One cold temporary boot of `06b9aa57a15cc6ad00fe3b63bf92b3da828607c0c2927a5e6374b9fb28fd6e0a` produced `IMG_9336.heic` (SHA-256 `3843bb1c59129d094529509c1ce01c8b239b8b957267d0371773d29a9ca70de8`). It reached Q6 ready with QRTR services announced, then stopped at runtime `0x73`, detail `0x61`, `QUEUEFULL`, with IPA flags `0x703`, zero IPA commits and responses, and RMTFS requests/replies 9/9. The error screen did not preserve the TFTP counters, so it does not establish whether a TFTP OACK was queued or any DATA was received. Two distinct owner queues can return this same error; the screenshot cannot identify which queue filled.

A host regression reproduced `QUEUEFULL` when five TFTP requests arrived in one transport turn: the owner can hold four requests, and the TFTP service runs only after that turn returns. An opt-in candidate yields after holding a TFTP request, retains IRQ ownership and schedules the existing continuation, allowing the dispatcher a turn before collecting more; the same regression now passes and all 166 owner tests pass under the server-check and QRTR flow features. This is a demonstrated host scheduling issue and a plausible match, not a proven diagnosis of the physical run. The candidate also records whether a subsequent owner `QUEUEFULL` came from the routed-QMI queue (1) or the held-TFTP queue (2), and retains TFTP counts on the error screen. No new device result is claimed.

### Held TFTP queue confirmed, 2026-11-02

One cold temporary boot of `c33fd9abd5bf88bdab2393edaccedc2df63948be589f36cc70b002fd13c54769` produced `IMG_9338.heic` (SHA-256 `a7cbe4d7dc56fa74a460574918de29b6d440fab91aac089cc65ab605cce61cf3`). It stopped at runtime `0x73`, detail `0x61`, `QUEUEFULL`, with queue kind 2 (held TFTP), one TFTP response queued and zero ignored by the writer. The queued counter establishes local queueing, not publication, peer receipt or write completion. The earlier `IMG_9337.heic` (SHA-256 `db3dece9da590abc99f27821ab6d9034f0e7b289dec8cdb282c5ecd94f467558`) was an in-progress storage snapshot. The later failure does not reveal the TFTP packet contents or prove why the queue filled.

A further host regression reproduces queue exhaustion with repeated identical server-check requests while a matching OACK is retained for QRTR credit; the earlier yield alone does not prevent this. A candidate coalesces only an exact byte-for-byte observed WRQ addressed to the service port from the same peer while its matching OACK is retained locally for retry. It still releases the received GLINK slot normally; altered messages and other peers are not coalesced. The regression passes, including negative peer/content cases, as do 167 owner and 239 complete-feature runtime tests. This reproduction is a plausible explanation but the physical packet stream was not captured. No new device result is claimed.

### Duplicate-coalescing device result, 2026-11-02

The cold temporary image `850b5d3d52447c220b1e67d24d1365d1c5077719547732748666181772eb19a7` produced a screen capture SHA-256 `c91ecb5beac0f5159f1dfd27d2221a28c56e8ed8823ac920e5756b02b91c38b0`. It again stopped at runtime `0x73`, detail `0x61`, `QUEUEFULL`, kind 2 (held TFTP), with one TFTP response queued and zero coalesced. The queueing counter does not establish publication or peer receipt. Zero coalesced means no received packet satisfied all of the exact-byte, peer, port, and locally retained OACK conditions; it does not by itself identify which condition failed. The display did not retain the individual held packet routes/opcodes or pending-reply state. The previous host reproduction therefore did not explain this particular device run. Capture those metadata and the response transport lifecycle before changing request policy again.

### Server-check held-packet snapshot

The queue-snapshot boot stopped with runtime error 0x73, queue kind 2. Five held records each had metadata 0x00490502: 73-byte exact captured WRQs addressed to the service port. Their peer ports were 0x48, 0x21, 0x47, 0x21 and 0x47. One reply was retained locally. One TFTP transmit had flags 0x0f (present, started, fully published, GLINK receive-done; no QRTR completion observed at the snapshot). One response had been queued, and zero requests coalesced. This does not establish that the published transmit was the retained reply or that the TFTP peer processed it. Different peer ports explain why the existing same-peer coalescer cannot combine all requests; the retained reply's destination and the transport credit lifecycle remain to be checked.

### Server-check QRTR flow candidate

The held-packet snapshot showed a single TFTP transmit with GLINK done but no QRTR completion, while another reply was retained. The server-check endpoint had used the legacy always-confirm path. The pinned Linux QRTR flow instead counts by destination, requests confirmation at the low watermark, and retires a packet after GLINK completion without requiring a per-packet resume. The experimental server-check transfer port now uses the existing bounded reference flow; other TFTP ports retain their legacy policy. A host regression verifies that the first OACK has confirm_rx=0, remains allocated until GLINK RX_DONE, and allows the subsequent DATA ACK to queue without a fabricated QRTR resume. The 168-test owner suite passed both with and without the reference flow; the source-pinned flow oracle passed. This is a candidate, not proof that the peer received OACK or that the queue-full problem is solved on device.

### Server-check QRTR reference-flow device result

The temporary image SHA-256 68aebc744d6a2377e457f844e155536dcfe0f89e3b2b243f308f2f9c4722d740 stopped with runtime error 0x73 and TFTP queue kind 2. IMG_9339.heic SHA-256 8b6836e28eebfc38eed51146b8131f9153bb10575c9128665f904ef70cac3146 shows one retained reply, zero active TFTP transmits and five held exact 73-byte WRQs (peer ports 0x21, 0x48, 0x21, 0x47, 0x47). One reply was locally queued; the current snapshot cannot prove delivery or explain why the retained reply last encountered backpressure. Compared with the prior image, no TFTP transmit was outstanding at the stop; queue-full remains. A subsequent software-only diagnostic records the latest waiting error (FlowBusy or NoCapacity), total transport TX and flow occupancy at the attempt and at stop, and pending destination. This diagnostic does not itself resolve the backpressure or prove peer receipt.

### Server-check retained-reply wait diagnosis

The wait-detail image SHA-256 61d549005dcd7b9f417465620a5f3c12f2e53b73fbf4188ddd837495484e4ca8 stopped at held TFTP queue kind 2. IMG_9340.heic SHA-256 53d49ab6a0ef94deb2b8de15b1ed6f416f5e0f9e7f0d6c85c290ac543cc3654f shows the last retained reply attempt returned FlowBusy, with five total TX records and two flow records at that attempt, destined for peer port 0x1f; by the stop both snapshot counts were zero. The screen does not identify which busy check fired or prove that the retained reply would have succeeded if retried. A candidate now yields to the service dispatcher when GLINK reports a transmit completion while a reply is pending. It also coalesces only an exact observed WRQ when an identical request from the same route is already held; separate peers and different requests stay distinct, and GLINK RX_DONE is still sent. A host model tests a busy retained reply with queued requests and completions, and checks exact same-route deduplication and negative cases. The device outcome remains unproven.

### Completion-yield and held-request coalescing device result

The one-shot image SHA-256 4c8113093aa931e42efbc6d9378393aa7d121e3ce276cbaa12dcf46c0ce6ff31 produced IMG_9341.heic SHA-256 3c616359272a09e77dbdee1db281e09d7a6502ef91b8929cd9cacd8147cc962c and IMG_9342.heic SHA-256 6ee820eeb73a3c99840d2703df3628fa817039ca64f46c79938abaed06db54fb. Both showed runtime error zero, IPA flags 0x4f8, four commits and one response, two locally queued TFTP replies, 0x12 ignored requests, four coalesced requests, zero RAM bytes and no write completion. Reject code 2 is UnexpectedRequest with size 0x49; the recorded first rejection does not contain the peer or byte identity. The output shows progress beyond the former immediate queue-full failure, but does not establish receipt of the OACK or completion of the write. The current ServerCheck permits only one peer; pinned Linux tqftpserv handle_wrq allocates an independent QRTR socket and writer per request. Earlier queue snapshots showed exact requests from multiple peer ports, so the single-peer restriction is a source-backed candidate to investigate, not a proven explanation of this particular rejection or of the absent DATA.

### Bounded independent server-check writers

The next experimental handler supports up to four simultaneous independent peer addresses, with distinct QRTR transfer ports 0x5001 through 0x5004 and separate 8 KiB RAM buffers. Slots are never reused during the boot; a fifth new peer is rejected rather than reassigning a port and risking a late packet crossing sessions. It still accepts only the observed server-check WRQ, rejects DATA over 512 bytes, and does not persist data or serve firmware. Repeated initial WRQs from the same peer use that peer's existing transfer port until data starts. Packet route validation checks peer and destination port before each DATA write. The reference-flow accounting and pending-OACK duplicate logic now cover all four transfer ports. The runtime reports aggregate RAM bytes, active sessions and completed writes. Host tests verify per-peer port routing, isolation, the fixed bound and the unchanged GLINK completion requirements. Linux's dynamically allocated per-request writers and the hardware behavior have not been reproduced in full; in particular no timeout or slot recycling is implemented.

### Four-peer server-check device result

One temporary image SHA-256 a8fcc06356634e257799e56d5fd44a9252647b1b5ca76fd93b2507b940f944e6 produced IMG_9343.heic SHA-256 80bc62af466633ce529a6c383b2fc838cbb61591f9dcbd0373b4403ad4a34048 and IMG_9344.heic SHA-256 5d6734927741175dfb82682493a2c25de2f38504000dc9d406c0cc822407be00. The first showed one active session, zero RAM bytes and no runtime error. The second showed runtime error 0x73, held TFTP queue kind 2, four replies locally queued, five retained requests, five coalesced, one retained reply whose last attempt returned FlowBusy, and two TFTP TX records. TFTP TX flags 0x11 are an OR across those records: present and QRTR bookkeeping done, with none marked started, fully published or GLINK done. This establishes neither peer receipt nor a completed write. At the last failed attempt three total TX and three flow records existed; the stop snapshot showed four and three respectively.

The pinned Linux 2267e248 qcom_glink_native.c (extracted SHA-256 14a1b55234934beba5d9166c6113fd7064df45d14c91ac084c06cc4ca0ffc34e) requests a suitably sized remote intent with GLINK command 7 when a blocking sender has none, and handles command 8 acknowledgement. The current native session instead reports NoRemoteIntent and neither sends command 7 nor handles inbound command 8. This is a real source-level behavior gap, not proof that the two unsent records were blocked by remote intent availability: FIFO backpressure, pending receive/control work, destination ordering and snapshot timing also need investigation. Any implementation must bound requests, handle grant/deny and intent advertisement ordering, preserve publication and TX lifetime checks, and avoid uncontrolled retries.

### Opt-in outbound GLINK intent-request model

The host experiment `sunfish-glink-intent-request-experiment` adds a bounded outbound command 7 with the needed datagram size when a queued transmission lacks a suitable remote intent, and accepts a matching channel's command 8 acknowledgement. It allows one published request per boot generation; a denial latches without retries, and a grant is not considered sufficient until a suitable remote intent exists. An uncertain publication quarantines the session; a proven WouldBlock leaves it retryable. Control RX_DONE, intent advertisements and credit packets remain separate from payload success, and a published or granted request is not evidence of peer receipt. This initial experiment does not implement Linux's ten-second timer, a timeout signal, or further requests in the generation, and waits without a timer if no acknowledgement arrives. It is disabled by default, has not been included in a phone image, and has not been proven to address IMG_9344's unsent packets. Source-oracle validation checks the pinned Linux command values and grant/intent wait contract. Host tests cover both ordering cases, denial, malformed or duplicate ack, would-block/uncertain publication, and command 8 through the FIFO.

The opt-in experiment was also cross-compiled with the complete current Sunfish runtime feature set, but no Microkit image was built or booted. The QMI tests passed with the experiment enabled (140) and baseline disabled (138); the FIFO model passed with the experiment (118), including command 8 traversing the ring; the server-check/owner suite passed with QRTR reference flow and the experiment (171), and the pinned Linux source oracle and runtime source checks passed. This remains a candidate; an observable deadline and wake mechanism are still needed before relying on it as a device-stage timeout.

### Experimental outbound intent deadline

The opt-in experiment now has a ten-second deadline measured from the runtime's first observation of a waiting request, not from the wire-publication instant. Since this image has no userspace timer service, it reuses the audited self-notification to poll the counter until the deadline; this can consume CPU and alter timing. A quiet poll skips transport service and framebuffer redraw only when no physical badge, pending modem IRQ or software continuation needs service. On expiration the runtime stops and quarantines instead of retrying, and the display reports intent state, request size and timeout. State 0 means no request, 1 waiting for ACK, 2 grant seen but suitable intent not yet available, 3 denied, and 4 grant plus suitable intent observed. State 4 does not prove payload publication or receipt. The handler still sends at most one request per generation. Tests cover both ACK/advertisement orders, denied and unsolicited ACK, publication uncertainty, wraparound and preservation of pending IRQ/continuation work. The live hardware outcome is unproven.

### First outbound-intent diagnostic on device

The temporary image SHA-256 09293ef2f39e5212afd67c994208934886b2587eaf8daefb0499b591b28430c9 produced IMG_9345.heic SHA-256 5f2286948e4b7f5ec083fe130fe78d0eaabe265a42d5ece6d85877bab4908158. It stopped with runtime error 0x73, held TFTP queue kind 2, retained reply, last wait FlowBusy and two TFTP transmit records with aggregate flags 0x11 (present, reference QRTR bookkeeping done, none shown started). The screen displayed intent state and size zero, but the runtime updated its cached intent state only after a successful service pass; this stop occurred inside a failed pass, so zero cannot establish whether command 7 was published. Owner quarantine leaves the session's software status intact. The corrected diagnostic reads that retained local state on the error screen and refreshes it even after a failed dispatch. A FIFO test confirms the status remains readable after suspend; no additional hardware access is required by the getter. This is a display/reporting fix; the queue-full behavior remains unresolved.

### Retained outbound-intent state and bounded priority candidate

One temporary image SHA-256 d83c973029007bf5b7f48efd3b78a6ae6ee08d439b2ffc47fa8fa35b97bee50d produced IMG_9346.heic SHA-256 87b9d982c9e018d466806009d8214dee5f4363f11624e9f9effdd400ed909a99. It stopped at 0x73, TFTP queue kind 2, with retained reply and two unstarted TFTP TX records. The corrected display read retained intent state 0 and size 0: no outbound intent request was successfully published by the stop. State 0 does not show whether a publication attempt was blocked. The deadline starts only after a published request is observed and did not fire here.

The original `publish_next` evaluated an outbound request only after pending receive-control publications. A host regression reproduced a queued reply with no remote intent in which a receive completion was emitted before the request. The new opt-in candidate allows its one request ahead of an ordinary receive completion if there is no publishable transmission, no pending intent acknowledgement, and no receive advertisement awaiting publication. It preserves the existing acknowledgement/advertisement order and publishes the deferred completion next; existing suitable remote intents retain normal priority. It also records request attempts and definite WouldBlock results, and displays them after an error. This is an offline-tested scheduling candidate; the device photo does not prove control-record priority was the cause of the stop.
# Intent-request priority experiment: IMG_9347 and IMG_9348

Request-type image `827f6cf40677c5796af33fcb62fa55249bc1fc43c234362150e7f0fe295afd86` produced IMG_9360 (`a0de53eafe14080d1d7465aa23df3728b56cf888044dbe65cfc4ddadf6d9e379`). The user reports unchanged counters afterward. It shows WRQ RX `0x18` (24), RRQ RX 0, WLAN RRQ RX 0, DATA RX 0, queued replies 8, TFTP TX started/sent/done 8/8/8, four active sessions, ignored `0x0f`, bytes/writes 0 and runtime error 0. The counters inspect packets reaching the native TFTP request handler; they do not establish absence before routing, a live display after all work ceased, peer interpretation of the responses, or exact peer/port for each packet. No WLAN image was requested by that handler in the displayed snapshot.

Offline follow-up: `test-tftp-reference-oack.py` compiles the saved tqftpserv C option parser and OACK sender from source SHA-256 `e8dc6a2ced3315f2c2e8b544f710b69fb6e8d464a92ce88ed800cf891edf22f1`, runs the observed options through it, and compares the output to the native Rust constant. The 39-byte OACKs match. This tests generated model bytes, not the actual delivered device packet or peer processing. The working Linux journal shows a server-check WRQ followed by several read requests and later WLAN-image RRQs; it does not prove the WRQ completed. The native model retains only its first rejection: the new host test demonstrates that a capacity rejection can conceal a later WLAN RRQ in that display. Incoming WRQ/RRQ/DATA and WLAN-image RRQ counters were added without serving new resources or storing DATA; their host tests and source checks pass. A device result is required before asserting whether any such request arrived.

TFTP TX-progress image `73f609a812cf03a5bf0d4db84ba9e4a802483c9a9d59763539f66117f8cb2a8a`: IMG_9356 captured an earlier frame with no active TFTP sessions. IMG_9358 shows four sessions and TX started/sent/done 3/3/3; IMG_9359 shows 8/8/8, four active sessions, RAM bytes 0, completed writes 0, runtime error 0, first reject Capacity with length 0x49. The user reports the screen unchanged after another minute. Thus eight local TFTP TX records were fully published and matched GLINK completions; that is not TFTP-level acceptance or proof the intended client processed the OACK. Counts do not identify individual peers/opcodes. Four fixed lifetime transfer slots, ignored requests, and absent DATA remain distinct leads. Photo SHA-256: IMG_9356 `c8181543958ec66f4ef5aaee31c7328c6936df9c317d3dfb56021fdb627e35ae`; IMG_9358 `b7013fef166f60e7ef5479f14748c0104ebb30a6586e1ecf27813aee0f790a12`; IMG_9359 `a3336573ddbc8b55e6cb0cc8d542462cfcf612f5687b4f8ad0e3793e188e4862`.

Per-peer reply image `455d910f7bd668344a926c1613315d0ec829666032ed6accbd999b30867720b4` produced IMG_9354 and IMG_9355. Both show runtime error 0, TFTP queued 8, ignored `0x11` (17), active sessions 4, RAM bytes 0, writes complete 0, coalesced 1, and first reject code 5 with `0x49` bytes. The unchanged display does not establish that the runtime is alive or hung, and the queued count proves neither GLINK delivery nor a completed TFTP transfer. Code 5 is the model's Capacity error. The narrow server has only four lifetime transfer slots; the code has two capacity branches (no free slot for an accepted WRQ, and RAM-capacity overflow). With zero total RAM bytes in these frames, RAM overflow is not supported by the displayed state, making the fifth-distinct-peer branch the source-consistent interpretation; packet contents and peer identities for this rejection are not shown. The HEIC originals were no longer present for hashing when recording; converted PNG copies in `/tmp` have SHA-256 `580bc2b3dfd90d29236ca910d2549d9dc36bfd722ba5f372d387f1c8e09aaab7` and `f9784434e28264ae136ad3453602a780dad857bdda10475d66e229033442ec09`, respectively. Compare reply publication and peer completion before changing the fixed capacity or claiming a fix.

Owner-loop follow-up: the RAM-only server has four independent transfer sessions but the owner has one `pending_tftp_reply` and one output buffer. `service_tftp_server_check_step` retries that retained reply before inspecting any held request. An extended owner regression confirms that when the retained reply gets FlowBusy, a request from a different peer remains held behind it. All 184 owner/IRQ tests pass with the device experiment flags. This is a real software head-of-line constraint consistent with the observed retained reply and queue filling, not proof that it was the only cause of IMG_9353: the earlier TX still needs a fitting remote intent and a publication/completion. A candidate needs bounded independent pending replies, immutable per-reply bytes, per-peer ordering and round-robin progress while preserving exact retransmission matching and GLINK completion ownership; no new device image has been built.

Offline follow-up to IMG_9353: `pending_peer_intent_control_precedes_tftp_fairness_selection` constructs gate combination 7 while a TFTP reply is queued. The session publishes the peer's backing receive-intent advertisement before reaching data selection, so the candidate credit/TFTP selection rule cannot affect that particular turn. The same host fixture then supplies no further incoming work; the required ACK and TFTP transmit both progress within a bounded number of turns. Thus the snapshot is consistent with control priority delaying data, but it does not establish permanent starvation or explain the whole phone run. All 114 control tests pass under the combined experiments. The recorded Linux IPA trace contains no usable IPCRTR remote-intent sizes, so the 30 free intents in IMG_9353 cannot be assumed to fit the TFTP replies.

IMG_9353 tested the credit-fairness image `59c45e4955d50bf58bdef4fd5eb38b6eb86a3fa5bd185f66478342b03042edd7`. It reproduced the queue-full stop `0x73`, kind 2, with TFTP TX flags `0x11`, two TFTP replies unsent, ready TX kind 2 (Resume), 30 remote intents free, no outbound intent attempt, gate combinations `0xaa`, and last combination 7. The candidate did not clear the blockage. The screen does not show whether the candidate's fairness branch ran, whether a free intent fit the TFTP data, or the complete publication chronology. Image SHA-256 `5882f9000d2d47566df47a46e83468edb155231e0ff2130b4c22845955b38adf`.

Offline response to IMG_9352: the experimental credit-fairness feature gives a sendable TFTP reply one data turn after a Resume, while preserving the original credit priority for other traffic and all normal receive/control ordering and flow checks. If no TFTP reply has a fitting remote intent, credits remain eligible. The feature is off by default. A host QRTR fixture checks credit → TFTP → credit; control test fixture additionally models an intent-request ACK when that existing experiment is enabled. Host suites pass (113 QRTR control, 143 combined GLINK session, 138 default session, 118 FIFO, 184 owner/IRQ, and 12 source tests). This is an unproven device candidate; IMG_9352 did not report buffer sizes and does not prove TX priority was the sole cause.

IMG_9352 gate-combination image `b781d81e539f163e69b48c41a631cc6cdd193170fb2d378acad63bd40a98c692` stopped at runtime `0x73`, queue kind `2`. `GATE COMBOS 0xaa` records combinations 1, 3, 5, 7; every observed turn with unsent data had some sendable TX. `GATE LAST 7` means the last instrumented turn also had pending intent ACK and advertising receive buffers. At the stop the snapshot showed `TX TFTP UNSENT 2`, `TX READY KIND 2` (QRTR Resume), and `REMOTE FREE 0x1e` (30 free slots, without size information). Intent attempts remained zero. This supports investigating strict credit priority and the owner dispatch schedule; it does not prove that the TFTP replies had a fitting buffer, nor that any Resume or TFTP reply reached the peer. Photo SHA-256 `e3e7b39e5e31c38425ebd5f12ac933bd52f9fce0c5189c9803de41ac39423920`.

IMG_9351 gate-trace image `0c001a2ff2d8941d8fb95b7071725960480628ab45d89ba19e380c40226d1d24` stopped on runtime error `0x73`, queue kind `2`, retained reply `1`, wait reason `1`. It reported gate calls `0x10f` (271), calls with unsent TX `0x9d` (157), intent attempts `0`, blocked `0`, state `0`, and accumulated gate mask `0x7`. During turns with unsent TX it observed a sendable TX, a pending intent ACK, and advertising local receive buffers; the OR mask does not establish that these coincided or which held at the end. Zero attempts means no turn satisfied all request-eligibility conditions while there was unsent TX, as instrumented; it does not prove the remote has no intent. Photo SHA-256 `cf00004168d421477d8f15408483f6a80b0184bc3170d60665ed7c10fcd1bd57`.

IMG_9349 tested the opt-in bounded TX fairness image `eab55d0fd466a5ce67601561f732a554c89071a8e2ef6e4262c31330e9b3bd9c`. It stopped with runtime error `0x73`, queue kind `2`, retained reply `1`, wait reason `1`, attempts `0`, blocked `0`, and intent state/size `0`. IPA commits was `1`, responses `0`; this does not prove TFTP delivery. The scheduling candidate did not resolve this stop in this boot, and the eligibility predicate responsible remains unknown. Photo SHA-256 `34a382b67a9898d64fce1050331246afbde6d2996de2a6bd6729df7548c32fa9`.

Offline follow-up: feature `sunfish-glink-tx-fairness-experiment`, disabled by default, gives an already-sendable data record one turn after a successfully published ordinary receive completion. Pending peer ACK and local intent advertisements keep their prior priority; publication failure does not advance the fairness state. A host regression reproduces and tests this bounded ordering, including a would-block writer and subsequent receive completion. Feature-enabled session tests (143), FIFO tests (118), owner/IRQ tests (184), default session tests (138), and 12 existing source-audit tests pass. This is a scheduling candidate; the device's last screen did not distinguish the conditions blocking its outbound intent request and does not prove starvation. It does not establish peer receipt, TFTP completion, or WLAN startup.

The one-shot image `39bdcce6e23fa02043fde8f5b244da70bc213eb15d74377dff62bdd1b7de5d1a` again stopped with runtime error `0x73`, queue kind `2`, and a retained TFTP reply. IMG_9348 reports intent attempts `0`, blocked `0`, state `0`, and size `0`: the publication callback was not invoked. This does not show that a FIFO write was blocked or that the peer lacked a suitable intent. IMG_9347 was an earlier in-progress frame.

The current eligibility condition also requires that no queued transmit be immediately sendable, that no peer intent ACK be pending, and that no local receive intent be advertising. The device result does not distinguish these conditions. An existing host scenario, `waiting_large_reply_does_not_block_small_qrtr_credit`, now explicitly verifies that a sendable small QRTR credit can keep the request ineligible while receive completions are published; all 142 feature-enabled GLINK session tests pass. This establishes a possible scheduling explanation, not the device cause. Do not treat it as proof of starvation or change ownership/flow guards on that basis.

Photo SHA-256: IMG_9347 `0e524f639dcd24cd545b63695b7bc5335c1b43824e95ebbd05237ec1ff34581b`; IMG_9348 `ea03b632903b17f979af72c67e9c79aae07a7f9159af37537a21d883011706dd`.


### Extended server-check session candidate

The request-type run in IMG_9360 reported 24 WRQs, zero RRQs, zero WLAN RRQs, zero DATA, and 8/8/8 TFTP transmit start/publish/completion counts. The user confirmed those counters remained unchanged. The WRQ count does not establish how many distinct peers were involved, and GLINK completion does not establish TFTP acceptance. The native server's first rejection was previously capacity with four lifetime sessions; Linux tqftpserv instead creates a fresh socket and writer for each WRQ until system resources are exhausted.

An opt-in host-tested candidate increases the bounded lifetime session limit from four to 32 without recycling slots, raising the fixed RAM data buffers from 32 KiB to 256 KiB. It retains peer/port isolation and the existing RAM-only exact-path restrictions. It is a capacity test, not a proven fix for reply delivery. It displays first rejection alongside WRQ/DATA and transmit counts; the first rejection cannot describe all later rejected packets. Further distinct requests may still exceed the new bound, and DATA and completion remain unproven.

The first 32-session candidate (SHA-256 de6a2c00f370316235a14b23ee33d583dc4224960d947a3ec7a65b1c45d8a729) reached `MPSS AUTH READY PASS` with Q6 ready, but remained on that screen across subsequent photos. The TFTP runtime screen was not observed, so the capacity experiment produced no TFTP result. On inspection, the release AArch64 binary reserves about 0xa6000 bytes in `pixel_mpss_runtime::start` and 0xa4000 bytes in the persistent Microkit entry frame, against a 0x200000-byte PD stack; nested frames consume additional space. Stack pressure is a plausible regression from enlarging inline session buffers, not a confirmed cause of the stall. Investigate the call chain and storage layout before another boot; do not repeat this image.

Offline comparison of AArch64 prologues found that entry and runtime-start frames grew from approximately 0x6c000 and 0x6d000 in the four-session image to 0xa4000 and 0xa6000 in the 32-session image; `step_opening` contributes approximately 0x6a000 in both. These are selected frame allocations, not a complete stack trace or proof of overflow. An isolated 4 MiB-stack system profile is being built to test the hypothesis while retaining the same 32-session behavior.

The isolated 4 MiB-stack 32-session image (SHA-256 0747b16bfacc7c9503afcfed44ab23268f9ec2257a95580bfd259c8995a4c5f5) reached the native GLINK/QRTR service runtime, unlike the preceding 2 MiB-stack candidate. IMG_9361.heic (SHA-256 e79db025d3a30608baa6520cd6628c2e32c316cebf6b924f0bb24a1ee2764376) shows Q6 ready, IPA flags 0x6f8, runtime error 0x73 and detail 0x61, CAP PHASE 1, QUEUE KIND 0, TFTP QUEUED 0xf and TX TFTP UNSENT 6. The error points to router dispatch failure, but this photo does not expose its precise cause. The success-only session, rejection and DATA counters are not visible on this error screen, so it does not establish successful TFTP write completion or whether 32 sessions suffice. The larger stack correlates with getting past the prior stalled screen; it does not prove the prior stall's root cause.

The capacity-detail boot (SHA-256 3facddf875f61e99d04cb878a3ae7ed3064328f8d87c3f521ab46041b3bf6df5) reached a live service screen in IMG_9362 (SHA-256 bd35f9fd4b44f0404e29a5e4462092237703e378c3e3a73a673c1521e7a0dae4). Its later IMG_9364 (SHA-256 892c748523a2c5538bb6a170392a318a29b2d64ffb24a30dfaf88bf8bd86e0dd) shows the same values: runtime error 0, WRQs 0xc, RRQs 0x14, WLAN RRQs 0, DATA RX 2, locally queued and GLINK started/sent/done 0xd each, RAM bytes 5, active sessions 6, completed writes 1, ignored 0x14, first rejection code 2 and length 0x49. The restricted model regards the short DATA block as a completed local write. These counters do not separately prove receipt of that write's final ACK. The first rejection is sticky and cannot identify every ignored request. The runtime screen remained unchanged for the user's comparison; this alone does not prove a crash. No firmware was served or WLAN started. The earlier capacity condition did not recur in this boot, so its command and resource counts remain unknown.

Offline Linux comparison after IMG_9364: the saved working Linux journal records RRQs for `/readwrite/ota_firewall/ruleset` and `/readwrite/modem_efs` after the server-check WRQ; both opens failed and the pinned tqftpserv `handle_rrq` sends TFTP ERROR code 1, `file not found`, from a newly connected QRTR socket on a failed open. The native restricted server instead ignores requests outside its exact server-check write profile. The native screenshots count RRQs but do not capture their paths, so they do not establish that these specific Linux requests occurred natively or that sending errors will unblock WLAN. The new host-only `platforms/sunfish/test-tftp-reference-missing.py` compiles the exact pinned C `tftp_send_error` and confirms that the existing Rust encoder produces identical 19-byte code-1 missing-file packets. No RRQ response was added to the runtime or sent to the device.

An opt-in first-RRQ observer has been added to the restricted server-check profile. It retains only the first successfully parsed RRQ's bounded, sanitized request metadata, leaving the existing response policy unchanged. The success screen displays its path length and the first 60 path bytes as hex across six rows because the probe font cannot represent slash, underscore or lowercase. Paths longer than 60 bytes are visibly truncated; malformed requests can precede the first parseable one. A host owner test verifies that a later RRQ cannot replace the first. The owner suite (173), screen diagnostic suite (10), source checks and reference missing-file oracle passed. Temporary candidate SHA-256 `3f898e374f66261b60bd4453b039bf243d09a0d970a65e58d22a452473f8482e` passed its 4 MiB-stack capability audit (280 rejected mutations) and boot-layout audit; no device boot or read-request response has occurred with this image.

First-RRQ observer boot `3f898e374f66261b60bd4453b039bf243d09a0d970a65e58d22a452473f8482e`: IMG_9365 and IMG_9366 show interim runtime activity without error. IMG_9367 shows runtime error 0x73 detail 0x61 and a captured capacity failure at receive phase 1, GLINK command 9, parameter 0x59. RX FREE=0, TX FREE=0, REMOTE FREE=2, REMOTE BUSY=0, QMI REQUESTS=4, FLOWS USED=0x13, FLOWS ACTIVE=8, TFTP QUEUED=9, ACTIVE SESSIONS=7, RAM BYTES=0, WRITES COMPLETE=0. The first RRQ path did not display. The snapshot proves both local RX and TX slot pools were full when the incoming record was rejected, but does not by itself identify which individual allocation returned NoCapacity or whether later progress would have occurred. The runtime is stopped; avoid retrying this image and investigate bounded backpressure and slot lifecycle offline.

A host regression reproduced one concrete path compatible with IMG_9367: when the eight transmit slots are occupied, a valid TFTP QRTR packet with confirm_rx set previously caused `NoCapacity` during GLINK receive. Under the existing opt-in deferred-request feature, the candidate instead retains that complete receive intent without RX_DONE and retries it after capacity returns. The regression fills the TX pool, confirms the packet is deferred, releases one TX via actual GLINK completion, and verifies exactly one TFTP request with intact bytes. It failed before the change and passed afterward; 142 GLINK feature tests, 138 baseline GLINK tests, 174 owner tests, source checks, 280 capability mutations and boot-layout audit passed. This confirms the software path is fixed in the host model, but IMG_9367 does not prove that specific path caused the device error. The first-RRQ capture remains enabled. Frozen temporary image SHA-256 `72af991e5ac8b15da7ad06b46a418d3e9055f9f869f0979d8330bf44a3b41f2d` has not yet been booted.

The temporary TFTP-defer image `72af991e5ac8b15da7ad06b46a418d3e9055f9f869f0979d8330bf44a3b41f2d` reached service runtime then stopped at runtime error 0x73 / detail 0x61. Photo SHA-256 `b47d9623e73faf6da35c8b9406d91e95d97036829dc94a0cefdd6ff2b2d36d71` displays a truncated `TRANSPORT SESSION CONTRO...` error, queue kind 0, TFTP queued 0xa, TX TFTP UNSENT 7, remote free 0x1e and wire source 0xfffffffe. The first RRQ path did not display. No capacity snapshot is visible; the photo does not identify the full nested control error or prove whether the TX-defer path was reached. Overlapping screen fields hide the complete error and failure command. Do not repeat the image; investigate retained software diagnostics offline.

The prior screen's nested `Transport(Session(Control(...)))` error was painted across three rows, but later TFTP/intent painters overwrote rows 15 and 16. A candidate now repaints the full error after those generic fields when there is no capacity snapshot or captured crash reason; it also preserves the GLINK failure stage, command, parameter, TFTP counts and session count. This is a display-only diagnostic change, not a claim that the underlying control error is fixed. A formatter test covers a representative nested control variant, source checks pass, and the temporary image `0fc5d0d5b2940cdfa244a837a72faa695ea2afee8b8a30587901e31603d5cd54` passed target build, 280 capability mutations and boot-layout audit. The prior TFTP-defer fix and first-RRQ capture remain included. No device result yet.

Control-error display image `0fc5d0d5b2940cdfa244a837a72faa695ea2afee8b8a30587901e31603d5cd54` stopped with `Transport(Session(Control(ClientGone)))`, runtime 0x73/detail 0x61, receive stage 3, GLINK command 9, parameter 0x70. It reports TFTP queued 0x11, ignored 0, WRQ RX 0x1d, RRQ RX 0, DATA RX 0, and writes complete 0; the active sessions value is clipped at the right edge. The source can produce ClientGone either for a closing peer with a deferred receive, or for a closing peer with an in-flight routed request; this photo does not distinguish the two, and the final error repaint can hide the older client-gone detail. The screen confirms neither a captured RRQ path nor completion of a server-check write in this boot. Investigate the two branches offline before modifying client-close handling.

Offline client-close analysis identified two distinct existing ClientGone guards: a peer withdrawn with a deferred receive, and a peer withdrawn with a routed receive/reply still in flight. The candidate retains header-only metadata for the deferred case (close command, peer, destination port, first two body bytes as opcode, and matching count), and reports either that case or the existing routed-request diagnostic in the final error screen. It does not change the stop/quarantine behavior, discard work, or establish that IMG_9367 exercised either branch. A new host test fills TX, defers a TFTP WRQ, then closes that peer and checks the exact branch and header metadata; the existing routed-close test checks it is not mislabeled deferred. All 143 feature GLINK tests, 138 baseline tests, 174 owner tests, source audits, 280 capability mutations, and boot-layout audit passed. Frozen image `c6850b4fdf0e0644121ff8b2798be69064c3a338888a4a95a77d61fd6e8ff347` has not yet been booted.

Client-close detail image `c6850b4fdf0e0644121ff8b2798be69064c3a338888a4a95a77d61fd6e8ff347` produced IMG_9368: `ClientGone`, runtime 0x73/detail 0x61, `CLOSE CASE 1`, `CLOSE COMMAND 6`, `CLOSE PEER 0x57`, `DEFERRED DEST 0x5000`, `DEFERRED OPCODE 2`, `DEFERRED COUNT 1`. This is a QRTR DEL_CLIENT while exactly one matching deferred TFTP write-request packet for the server endpoint is retained. It does not show whether other replies or peer state for the same port are still in flight; a safe handling change must consider those separately, retain correct GLINK RX_DONE ordering, and guard against stale traffic or peer-port reuse. No TFTP RRQ/DATA was shown in this snapshot. The deferred-close guard intentionally stopped; no read path was captured.

The experimental deferred-close candidate handles only a validated QRTR DEL_CLIENT with exactly one deferred byte-exact server-check WRQ to port 0x5000. Immediately before each receive poll, the serialized owner supplies a fail-closed snapshot of whether any TFTP handle or retained reply is pending. Cleanup proceeds only if this snapshot is clear, no other same-peer receive or transmit remains, the peer has no RMTFS flow history, and a closed-peer fence is available. Any existing reference flow for the peer is closed only if no active reservation prevents it; otherwise the runtime retains the original ClientGone stop. The discarded deferred packet is never delivered to the server; its GLINK intent is returned via ordinary RX_DONE, and the peer is fenced for the generation. Tests verify no processing of the abandoned request, both receive completions, late peer traffic fenced, and fail-closed behavior for owner work, same-peer TX, nonmatching requests, and additional held/deferred requests. 146 experimental GLINK tests, 138 baseline tests, 174 owner tests, source audits, 280 capability mutations and boot-layout audit passed. The capability audit was restarted after adding the RMTFS-history guard; only the second build is frozen. Image `2f87b1a4e1687a0399e1fc9dacbe89f34d04deda83f032e015cb4d1c73c435bf` has not yet been booted. This is conservative: unrelated owner-held TFTP work also blocks cleanup, and the 64-entry peer fence can fill; neither the previous photo nor the host tests establish that the Pixel will pass this guard.

The deferred-close experimental image `2f87b1a4e1687a0399e1fc9dacbe89f34d04deda83f032e015cb4d1c73c435bf` produced IMG_9369 with runtime error 0x73/detail 0x61, `Control(ClientGone)`, close case 1, command 6, peer 0x5f, one deferred opcode-2 packet to 0x5000, and RRQ/DATA counters 0. The conservative cleanup was not applied in this boot. The display did not capture which eligibility condition was false (owner TFTP busy, packet exactness, other same-peer RX/TX, RMTFS history, reference-flow closure or fence capacity), so the photo cannot establish which condition blocked cleanup or whether this is the same peer/sequence as earlier boots. No WLAN or RRQ success is shown. Investigate the fail-closed conditions offline; do not repeat the image.

The IMG_9369 screen did not include a close eligibility breakdown; therefore its blocking condition is unknown. The next opt-in candidate replaces only the prior all-peers owner-work guard with a bounded 9-entry snapshot covering all four owner-held receive handles, the legacy retained reply, and four per-peer retained replies. The owner refreshes it before every receive poll; an absent snapshot fails closed, and work for the closing peer continues to block cleanup. Other receive, TX, exact-WRQ, modem-storage history, existing-flow closure, and closed-peer fence checks remain. On a stopped deferred close, `CLOSE BLOCKS` reports bits 0 invalid request, 1 same-peer owner work or absent snapshot, 2 other same-peer RX, 3 same-peer TX, 4 modem-storage history, 5 already fenced, 6 no fence slot, 7 flow-close failure. A host test confirms unrelated-owner peer work does not block the narrow cleanup and same-peer owner work does; the snapshot does not establish why IMG_9369 stopped. The 147 peer-specific GLINK tests, 146 global-guard tests, 138 baseline GLINK tests, 174 owner tests, source checks, 280 capability mutations and boot-layout audit passed. Frozen image `5b6ca9c605f77e5ba9e73377ef4a615c7b88c134630b722a9cbfbf25299da7e1` has not yet been booted.

Peer-specific deferred-close image `5b6ca9c605f77e5ba9e73377ef4a615c7b88c134630b722a9cbfbf25299da7e1` produced IMG_9371: runtime 0x73/detail 0x54, `LinuxIpa(Timeout)`, IPA flags 0x707, one IPA commit, zero IPA responses, 0xc WRQ, 0xa RRQ, two DATA, one locally complete write, and six active sessions. No CLOSE CASE or CLOSE BLOCKS appears because this stop is an IPA timeout, not a recorded deferred-close stop; the photo cannot prove whether a client-close cleanup was exercised. The error screen replaced the first-RRQ path even though RRQ count is nonzero. No WLAN startup or served RRQ is established. Compare IPA TX/reply state and preserve the captured first-RRQ metadata at failure before changing the protocol.

### IPA timeout and first RRQ error display

IMG_9371 showed runtime error `0x73`, detail `0x54` (Linux IPA timeout), with one IPA commit and no IPA response. The same screen recorded 12 write requests, 10 read requests and two DATA packets; it does not establish that the peer-close guard was exercised. The IPA timeout can arise from a pending record deadline or the modem-ready deadline, so this result does not distinguish the cause. The previous error screen did not preserve the captured first read path.

The display-only timeout diagnostic preserves IPA transmit length, sent count, flags and remote intent alongside the first parsed read-request path length and up to 60 path bytes in hexadecimal. This does not answer reads or change the restricted RAM-only server-check behavior. Runtime source tests (12), source audit, diagnostic display tests (11), target build, 280-mutation capability audit and boot-layout audit passed. Frozen temporary image SHA-256: `0c2f45c5a7433f551f519927d1253d048115f153512ee5c21ab94469bb1076e9`. Device result pending.

### Peer-close guard on IMG_9372

Temporary image `0c2f45c5a7433f551f519927d1253d048115f153512ee5c21ab94469bb1076e9` stopped at runtime error `0x73`, detail `0x61`, transport `ClientGone`. `CLOSE CASE 1`, close command `6`, peer `0x57`, deferred destination `0x5000`, opcode `2`, count `1`, and `CLOSE BLOCKS 2` identify a deferred write request blocked by the peer-specific owner-work check. Bit 1 is set when the owner snapshot is invalid or contains the closing peer; the snapshot display alone does not distinguish these alternatives or identify whether the owner held a request or reply. No other guard bit was reported. This run did not reach the IPA-timeout display, so it did not reveal the first RRQ path or IPA transmit details. `IPA COMMITS 1` and `IPA RESPONSES 0` do not establish completion of the IPA handshake. The displayed `TFTP DATA RX 0` does not prove peer behavior beyond this observation. Photo SHA-256 `a809bddf6bc5101945506af32b3f5e7abe8343cae407f5d13fbca2fdf7c30d03`.

Next investigate owner snapshot validity and the same-peer owner-held request/reply lifecycle before changing cleanup: removing the guard could discard outstanding work. An offline regression reproducing the close while an owner holds same-peer work would help determine a safe transition.

Offline follow-up: `ModemOwner::work` rebuilds and installs the peer snapshot immediately before each `poll_receive`; that adapter handles the inbound close within the poll. In this owner path, a successful snapshot call thus makes a matching peer the likely explanation for block bit 1, though the photo cannot independently prove snapshot contents. The snapshot includes held TFTP request handles, the legacy retained reply, and per-peer retained replies; it does not distinguish them. The existing feature-enabled GLINK regression explicitly exercises owner work for the closing peer and expects `ClientGone`, and separately checks that unrelated owner work permits a close. Running the three focused deferred-close tests passed. These tests model the snapshot; they do not reproduce the full real owner lifecycle or establish whether the held work is safe to cancel. No cleanup guard was relaxed and no new device image was booted.

Owner-level follow-up: added two host regressions that construct a real owner and adapter, saturate the transmit queue, defer an exact same-peer WRQ, and deliver DEL_CLIENT. One case holds an earlier request; the other holds a retained reply. Both reach `ClientGone` with owner-work bit 1 set and preserve the respective held work. Other guard bits may also be set, so these cases do not establish which state produced IMG_9372's exact mask. The feature-enabled owner suite passed 175 tests. They demonstrate that a broad cleanup could discard live owner state; they do not yet establish a safe cancellation protocol or prove which case occurred on the phone. No runtime behavior or device image was changed.

The subsequent owner-kind diagnostic records the same pre-poll peer snapshot used by the existing guard, with kind bits: `1` held request, `2` legacy retained reply, `4` per-peer retained reply. `OWNER KINDS` is the bitwise union for the exact node and port named by the close; it is shown on the deferred-close stop screen. Cleanup policy is unchanged. Both owner-level reproductions assert the corresponding bit, all 175 feature-enabled owner tests and 147 GLINK tests passed, as did 12 runtime source tests, 11 display tests, the target build, capability audit (280 mutations) and boot-layout audit. Frozen temporary image SHA-256 `006499f57976e1232d5adb74018127d4f64f4837314bee02ff5a23a16560bb06`; no device result yet.

IMG_9373, temporary image `006499f57976e1232d5adb74018127d4f64f4837314bee02ff5a23a16560bb06`, stopped at `ClientGone` (`0x73/0x61`), close case 1, command 6, peer `0x57`, one deferred WRQ to `0x5000`, and `CLOSE BLOCKS 2`. `OWNER KINDS 4` identifies a per-peer retained reply in the pre-poll owner snapshot for the closing node/port. It does not identify the reply payload or establish whether it has been delivered; nor does it establish that cancellation is safe. `IPA COMMITS 1`, `IPA RESPONSES 0` are not evidence of completed IPA initialization. Photo SHA-256 `08939a388596ac65054d0acd0e73996d684f1cd9c02c33d2a7cbb56f6bfdaee6`. Investigate the retained-reply lifecycle and peer close handling before changing behavior.

### Retained reply lifecycle and candidate close handoff

The `OWNER KINDS 4` slot is an owner-held `PeerTftpReply`, populated after `ServerCheck::receive` and cleared only when `queue_tftp_response` succeeds. The recorded reply itself therefore had not yet been queued to GLINK when the pre-poll snapshot was taken; this does not say whether some earlier reply to that peer was delivered. The same guard snapshot reported no same-peer queued transport transmit (`CLOSE BLOCKS` was exactly 2), but that is separate from end-to-end application receipt.

`ServerCheck` also maintains a per-peer RAM transfer and reserves its transfer port for the boot, including after completion. Removing only the retained reply would leave that state behind. The current transport checks the owner snapshot while processing a close and fails closed before returning a control event to the owner, so it cannot safely mutate owner state itself. A candidate solution requires a bounded, explicit owner/transport close handoff: recognize only a validated DEL_CLIENT with the exact deferred server-check WRQ and one matching unpublished owner-held reply; preserve and validate full peer identity and generation; fence the peer; cancel only that peer's matching retained reply and mark the server-check transfer abandoned without reusing its port; return the consumed receive intent through the normal completion path; and leave unrelated peers and all storage paths untouched. If any required validation, capacity, ownership, or lifecycle step fails, stop without partial cleanup. The ordering and rollback guarantees, including control event/intent completion and any in-flight transport state, still need an implementation and owner-level regression tests. The photo alone does not prove the retained reply is an OACK or that canceling it resolves WLAN startup. No behavior change has been made.

Offline handoff implementation started: under the existing peer-close experiment feature, `ServerCheck::abandon_empty_peer` can mark an exact existing, empty, incomplete transfer abandoned while retaining its peer and port reservation. It rejects missing peers, repeated abandonment and transfers with data or completion; subsequent requests and data for that transfer are rejected. Host tests cover peer isolation, late packets, non-reuse of the transfer port, and preservation of an in-progress transfer; 8 tests passed with 4 and 32 slots, and all 6 baseline tests passed without the feature. The operation is not connected to client-close handling, and the transport/owner handoff, ordering, fail-closed rollback, and end-to-end tests are still outstanding. No new image was built or booted.

Further offline handoff work: `ServerCheck::can_abandon_empty_peer` and `can_abandon_empty_peer_at` preflight the exact peer/transfer port, while `abandon_empty_peer` revalidates at commit. A regression verifies that data arriving after preflight prevents abandonment. The owner now has an experimental `can_cancel_server_check_reply` preflight that accepts only a live owner, one exact peer's retained OACK, a matching reserved transfer port and an empty server session; it rejects same-peer held requests and legacy replies. This does not validate transport state, cancel a reply, or process a client close; it requires exclusive access and a separate transport check. The 32-slot server tests (9), baseline server tests (6), and feature-enabled owner tests (175) passed. No device image was built; transport/owner commit and fail-closed recovery remain unimplemented.

Transport handoff prototype: new host-only feature `sunfish-tftp-close-handoff-prototype` reports `TftpClosePending` when the exact deferred-close case has only the owner-work blocker. It holds both receive slots and prevents normal session operations and publication until a decision. Rejection requires the same generation and peer and quarantines without returning RX_DONE. Host tests confirm stale/wrong-peer rejection and no writer callback while pending; all 147 feature-enabled GLINK tests passed. Native runtime has an explicit compile-time rejection for this prototype because it has no owner decision/commit integration yet. This is not a usable phone fix and must not be booted; commit ordering, owner/server coordination and full adapter/runtime regressions remain outstanding.

Host-only close commit prototype: a held candidate can now be committed after an external owner/server preflight. The transport revalidates the generation, peer, two held records and exact WRQ; it fences the peer, processes DEL_CLIENT, and returns both receives through normal RX_DONE. Failed commits quarantine. The owner prototype accepts only the exact retained OACK and empty server transfer, invokes the checked adapter commit, marks the transfer abandoned with its port reserved, and then drops the owner-held reply. A failure stops the owner; no retry is attempted. Owner/adapter tests cover successful coordination and rejection when data arrives between hold and decision, retaining both data and reply. All 176 prototype owner tests, 148 prototype GLINK tests, and 175 pre-existing peer-close owner tests passed. This remains host-only: the native runtime explicitly rejects the prototype feature, and its scheduling integration, full hardware-gate failure/partial-commit audit, and target validation are not complete. No new image was built or booted.

Scheduling and fault follow-up: the feature-gated runtime source invokes the pending-close owner decision after router dispatch and before its normal service pass. An actual owner/IRQ-router host test checks successful continuation and that rejection leaves the modem IRQ masked. Injected observation, descriptor, barrier and not-ready failures stop the owner, retain the reply and server transfer, and publish no writes. The owner/IRQ suite passed 192 cases; the Linux IPA/RMTFS owner harness with the selected prototype flags passed 239, and the runtime source suite passed 14. A full AArch64 target binary compiled from an isolated source copy with the explicit host-only compile block removed only in that copy; this was a build check, not a bootable-image audit. The workspace retains the native compile block. The full partial-commit failure audit, new capability/boot-image validation and device verification remain outstanding. No image was built or booted.

Partial-commit audit follow-up: host-only fault injection now forces failure immediately after the QRTR flow close, after client-control processing, after the adapter commit, and after RAM transfer abandonment but before owner reply retirement. In all four cases the owner stops, retains the unpublished reply, publishes no new writes and rejects retry. The first three retain an unabandoned server session; the final injection leaves it marked abandoned and its port reserved. Hardware observation/descriptor/barrier/not-ready fault tests also remain in the suite. A feature-gated counter increments only after all handoff state changes complete; the live painter displays `CLOSE COMMITS` and repaints on a change. The IRQ/router suite passed 194 tests, the Linux IPA/RMTFS harness 241, and runtime source checks 15 plus the source audit. The complete AArch64 binary compiled from an isolated copy with the native prototype guard removed only in the copy; this is not an image or hardware test. The workspace native-boot guard remains, and a separate image capability/layout audit is still required.

The one-shot close-handoff image was built from an isolated copy: the workspace's native compile guard stayed intact, and the isolated source differs only by removal of that guard. All 363 captured Rust build-source hashes were rechecked against workspace and isolated copy, and the frozen manifest records 380 source hashes and four input hashes. Selected handoff source invariants (10), runtime source checks (15), prior 194 IRQ/owner and 241 Linux IPA/RMTFS tests, the 280-mutation capability audit, and complete boot-layout audit passed. Frozen image SHA-256 `c2ead51a086eb0b2529f10baffffb09c103c4f5c5670ac6d0b75439a8d961e82`. The temporary test repeats the earlier IPA, modem, RMTFS and RAM-only server-check startup and may cancel only the exact unpublished OACK for a validated closing peer with an empty matching transfer. It serves no read requests or firmware, does not start WLAN or flash, and has no device result yet. `CLOSE COMMITS` counts local completed cleanup, not peer receipt or Wi-Fi success.

IMG_9374 from one-shot image `c2ead51a086eb0b2529f10baffffb09c103c4f5c5670ac6d0b75439a8d961e82` shows `CLOSE COMMITS 1`, proving one local guarded close handoff completed, and `RUNTIME ERROR 0` at the captured moment. Screen values are hexadecimal: `IPA FLAGS 0x4f8`, `IPA COMMITS 4`, `IPA RESPONSES 1`, `TFTP WRQ RX 0x13`, `TFTP RRQ RX 0`, `WLAN RRQ RX 0`, `TFTP QUEUED 0xf`, `TFTP TX START/SENT/DONE 0xf` each, `TFTP DATA RX 0`, `RAM BYTES 0`, `WRITES COMPLETE 0`, `ACTIVE SESSIONS 0xa` (includes reserved abandoned slots), `CLOSED PORTS 0xa`, and `TFTP COALESCED 3`. The user reports counters unchanged after about a minute. TX DONE is a transport completion and does not establish application acceptance. No Wi-Fi firmware was served or WLAN started. Do not infer the runtime is definitively dead, that the modem accepted an OACK, or why data has not arrived from a static screen. Photo SHA-256 `949dd4b771779e2e7a484362a331bea015f8f332c6bc863815ded9a2581884be`.

Decoding `IPA FLAGS 0x4f8` against `pixel_mpss_runtime_diagnostic::ipa_flags` and the painter's argument order: bit 3 is the sticky `ipa_complete_seen`, bits 4–7 report modem-ready, microcontroller-ready, indication-requested and indication-sent, and bit 10 reports the protocol state is present. Therefore this screen records that the native IPA protocol-complete milestone was observed during this run; it does not prove WLAN firmware startup or network traffic. The bounded TFTP OACK model had previously matched the saved Linux `tqftpserv` parser's generated bytes in an offline test, but neither that test nor the TX completion counters prove the destination application accepted any OACK.

Offline endpoint comparison after IMG_9374: the saved pinned `tqftpserv` WRQ code calls `qrtr_open(0)`, connects that new QRTR socket to the request source, and sends the generated OACK; its subsequent writer path rejects packets from other peers. In the matching Linux tree at commit `2267e248204e7e051e0f7355298949a6440de579`, `net/qrtr/af_qrtr.c` (saved SHA-256 `49276b19fca3ac6356ecd7e77fa34273761a0ad8d8914381b8c0545c2fba496e`) autobinds the connected socket to an ephemeral local port in `0x4000..0x7fff`. Native server-check transfer ports `0x5001..0x5020` are in that range and its OACK route addresses the incoming peer. That source comparison does not prove the native on-wire route or modem acceptance match Linux. Rerunning the saved reference oracles passed: 39-byte OACK and 19-byte missing-file error match the extracted Linux C functions. On this particular run, the RRQ counter was zero, so responding to a missing file is not yet the evidenced next action. Native `handle_writer`'s 512-byte data limit also matches the saved Linux writer buffer and short-block completion rule; the nominal 7680-byte option alone is not evidence of a mismatch.

Further reference audit: the pinned `af_qrtr.c` `qrtr_connect` autobinds a new socket; `qrtr_node_enqueue` encodes a little-endian QRTR v1 DATA header containing the source/destination addresses, the destination-flow confirmation flag and the unpadded payload length, then pads to four bytes. The host owner regression now checks all eight header words, zero padding and the exact OACK payload on a representative native outgoing packet, in addition to verifying GLINK completion and a simulated inbound DATA packet. The Linux IPA/RMTFS host harness with the 32-session and close-handoff features passed all 241 tests. This checks model serialization against source; it does not observe the native or Linux packet on the device, the actual live peer address, or modem application acceptance. The pinned reference implementation was fetched from https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/net/qrtr/af_qrtr.c and stored locally under the comparison source-review tree (hash documented above).

Linux endpoint-comparison preparation: metadata-only ARM64 `LD_PRELOAD` tracer `adbb2583f3d3e8f026cf91b9fb5add9a945e29862259d666a75fd8b24c516e39` observes `connect`, `send`, and `recvfrom` for QRTR sockets, logging only local/peer node and port, two-byte opcode, return count/status and monotonic time. It uses safe self-copy for caller-supplied pointers, preserves errno, and does not deliberately log paths or packet bodies. Synthetic ARM64 Linux loopback, invalid pointer, non-QRTR exclusion, privacy and `stdbuf` launch tests passed; these do not test QRTR or the tracer on the Pixel. The downloaded Debian 1.1-4 ARM64 `tqftpserv` imports all three symbols and has executable SHA-256 `01e063925e3e86b0c33af5f87227633cf0e9305cdd35d1b1552262d489bddf56`; the prepared script requires the reference device executable to match before starting the providers. Prepared script SHA-256 `961295a89726f3566e035bff04b264edb67a34d1a3644d4c393db0917e360fce`, pinned reference image and RAM-shadow inputs recorded in `.cache/sunfish-native-wifi/private/tftp-reference-metadata-capture-v1/manifest.json`. Logging can perturb timing or be denied; missing events do not prove absence of traffic, and GLINK/Linux send results do not establish modem application receipt. The existing reference bringup/dmesg logs can contain firmware paths. No device boot or Linux capture performed.

Endpoint comparison is prepared but has not been run on the Pixel. The Linux tracer's ARM64 synthetic tests were extended to valid-descriptor invalid-pointer failures and the `stdbuf` launch; the binary hash remains `adbb2583f3d3e8f026cf91b9fb5add9a945e29862259d666a75fd8b24c516e39`. The prepared Linux capture is now `.cache/sunfish-native-wifi/private/tftp-reference-metadata-capture-v2/` (startup script hash unchanged `961295a89726f3566e035bff04b264edb67a34d1a3644d4c393db0917e360fce`). Logs can perturb timing, safe self-copy can be denied and leave fields unavailable, and no QRTR/device test has yet verified interposition.

For the native comparison, an opt-in metadata observer latches the first queued OACK's source/destination node and port, payload length, QRTR confirmation flag and matching GLINK intent. Flags encode queued=1, publication-started=2, publication-completed=4, matching GLINK RX_DONE=8, QRTR confirm requested=16. The observer stores no packet content and paints only on the non-error runtime screen; a different error may prevent the endpoint snapshot from being visible. Snapshot changes trigger display refresh without an added timer or liveness guarantee. A first-OACK test covers both confirmation flag settings, exact intent and GLINK completion; 149 GLINK tests, 194 IRQ/owner tests, 241 Linux IPA/RMTFS tests, 16 source tests, 13 targeted source invariants, 280 capability mutations and the full boot-layout audit passed. Frozen native image SHA-256 `bfeb9ca6b9e1a3481dabb4df3e30edb5248115840966225444cfd84029af1fd5`; its manifest hashes 380 sources and four build inputs. The workspace native guard remains; only its isolated build copy omits it. Neither image has been sent or booted, and payload delivery or Wi-Fi readiness remains unproven.


Linux reference endpoint capture after USB reconnection (2026-11-06): the Pixel accepted one cold temporary Linux boot of the pinned image. The initial capture script stopped before loading modem drivers because its three-second provider lookup returned no services; a later read-only lookup found both services active. A separately recorded one-shot continuation rechecked those providers and the existing attempt markers, then continued from the original script immediately before modprobe. The modem reached running, the reference bringup returned zero, WLAN interface was reported, and all four read-only physical EFS hash checks passed. Captured files and SHA-256 values are under `.cache/sunfish-native-wifi/private/tftp-reference-metadata-capture-v2/run-reconnected/results.json`.

The metadata-only trace recorded a 73-byte WRQ from QRTR node 0 port 30 to node 1 port 16384 (0x4000), a 39-byte OACK from node 1 port 16391 (0x4007) to that peer, a 9-byte DATA packet to that transfer port, and a 4-byte ACK to the peer. All four were observed within approximately 0.98 ms of the initial receive. The WRQ length matches the previously observed native server-check request, but the metadata observer intentionally records no path or body, so this trace alone does not prove its pathname or exact option bytes. Later RRQ/DATA exchanges occurred. This establishes that the Linux peer sent TFTP DATA after an OACK in this run; Linux socket send completion and the Linux result do not prove the native OACK reached or was accepted by the modem. Compare the frozen native endpoint observer on its own later cold boot before changing routing based solely on these ports. Tracing perturbs timing and its QRTR coverage was first exercised here. The host's initial provider-lookup failure was a startup timing observation, not a modem or WLAN failure.


Native first-OACK comparison, image `bfeb9ca6b9e1a3481dabb4df3e30edb5248115840966225444cfd84029af1fd5`: the first photographed runtime screen showed Q6 ready, runtime error 0, close commits 1, WRQ count 0x13, RRQ count 0, TFTP DATA count 0, and TX sent/done 0xf. First OACK was node 1 port 0x5001 to node 0 port 0x1f, payload length 0x27 (39) and flags 0xf: queued, started, fully published, GLINK RX_DONE. The QRTR confirm-requested bit 0x10 was clear. The Linux metadata-only comparison saw a 39-byte OACK from node 1 port 0x4007 to node 0 port 0x1e, followed by a 9-byte DATA packet. Both source ports fall within Linux's 0x4000..0x7fff dynamic range; the remote endpoint values differ between boots, so numeric inequality alone is not evidence of bad routing. The observer latches the outgoing route; without the incoming source in the same trace it does not independently prove that the destination matches that request. RX_DONE is GLINK buffer completion, not modem TFTP acceptance. No DATA was displayed at the time of the photograph; screen changes after this snapshot must be checked separately. Photo SHA-256 `ea518bde3d5d9d22cd1fb23ba935ee9cd5dcd2b80e406ddd57fbf9436c2e0396`. The user reported unchanged counters after about one minute; the unchanged display is not proof that the runtime was executing or that no packet arrived outside the displayed accounting.


Native first-WRQ timing follow-up: the user reported the endpoint-comparison screen unchanged after a minute. The pinned Linux QRTR DATA header and native encoder agree in field order, widths, size and padding; the Linux and native source ports are both in the Linux dynamic range, which does not establish modem acceptance or timing. To distinguish a slow host response from a fast but unaccepted reply, an opt-in timing observer records the first exact server-check WRQ peer and the first OACK to that peer. It samples host-owner receipt, software queue, completed GLINK publication and matching RX_DONE using the existing architectural counter. A missing first exact request is shown as unavailable, a different peer is explicitly marked, and no mismatched timings are paired. Values are elapsed whole milliseconds from host receipt and exclude modem-to-host travel; they are observations taken after each event, add some overhead, require normal runtime progress, and an error display can hide them. This does not establish packet acceptance or a WLAN connection.

The timing feature passed 254 full-feature host tests, 164 baseline tests, 17 runtime-source checks, a full AArch64 build, 280 capability mutation checks and the boot-layout audit. Native compile guard removal was restricted to the isolated build copy, as for the earlier prototype; workspace guard remains. Private source inputs were relocated by prefix in the isolated copy and hashed along with their referenced files. Frozen temporary image SHA-256 `ad755f526a33bd66c2c9db2897b2d4d6e82fb2cc8b03ff225e5a423df1d5eb51` is recorded under `.cache/sunfish-native-wifi/private/native-tftp-timing-frozen-v1/manifest.json`. It repeats the previously tested IPA/modem/RAM-only server-check startup, does not answer RRQs, serve firmware, start WLAN or flash. No device action has been made with this image.


Native timing image `ad755f526a33bd66c2c9db2897b2d4d6e82fb2cc8b03ff225e5a423df1d5eb51`: photo SHA-256 `636737b797a205a2986ead8846e74880ad7d04e83b66c32842c391b7a5218de3` shows the first exact WRQ peer node 0 port 0x1f and `PEER MATCH 1`; queue age 0x1df = 479 ms, publication-observed age 0x7943 = 31,043 ms, GLINK RX_DONE-observed age 0x7c05 = 31,749 ms, and OACK flags 0xf. Runtime error is zero, WRQ count 0x13 and DATA count zero. The elapsed time between the queue and publication observations is 30,564 ms; the Linux observed WRQ-to-OACK-send interval was approximately 0.475 ms. These native ages begin only when the owner accepts the request, and the observer reads milestones after the relevant operations return; they are not a wire timestamp or direct evidence of modem receipt. In this source the owner has one adapter publish_next call, and the OACK publication flag is set on successful ring-writer completion before that call returns. The result supports investigating native publication scheduling/transport work and timing instrumentation overhead; it does not by itself identify which operation consumed the interval or prove why the modem did not send DATA. No WLAN was started.


Native TFTP fairness candidate after the 31-second publication observation: `next_transmit` previously allowed a ready fragmented non-TFTP transfer in an earlier slot to win repeatedly over a ready OACK unless a QRTR credit happened to trigger the existing TFTP turn. A host regression using real session queues reproduced a later OACK losing its next turn to an unrelated fragmented reply. An opt-in scheduling change offers TFTP a turn after other data as well as after credit, and separates the ordinary-data selection class from TFTP so repeated low-slot TFTP traffic does not starve other ready data. It retains per-destination sequence checks, intent-size checks, receive-completion handling, ring-space checks and uncertainty quarantine. A second real-session regression first exposed a reverse-starvation flaw in the initial candidate; the final version fixes both cases. Tests also cover insufficient intent, credit progress and a blocked writer. Two existing close-handling tests were adjusted to permit bounded data/completion interleaving, while still requiring both receive intents to be returned within eight publication opportunities; their former exact-two-record assumption also failed with the pre-existing fairness features. Baseline GLINK suite passed 154, candidate full-feature GLINK suite 160, full owner suite 254; both targeted starvation mutations were rejected. These tests demonstrate bounded scheduling in the exercised host fixtures, not an absolute device-time bound or proof that this scenario caused the measured 31 seconds.

The pinned Linux reference `drivers/rpmsg/qcom_glink_native.c` at `2267e248204e7e051e0f7355298949a6440de579`, SHA-256 `14a1b55234934beba5d9166c6113fd7064df45d14c91ac084c06cc4ca0ffc34e`, tracks incoming fragmented data by intent in its non-intentless path and serializes each FIFO record. This supports the shape of the experiment but is not evidence that proprietary modem firmware implements every detail identically.

The candidate retains first-WRQ-to-OACK timing and adds a non-redraw-triggering snapshot of calls to the GLINK publisher while the first OACK remains unpublished. `WAIT MASK` is an OR across calls, not one simultaneous snapshot: bit 0 suitable intent; bit 1 first OACK selected; bit 2 another transmit selected; bit 3 outbound intent request waiting; bit 4 receive-intent advertisement pending; bit 5 intent ACK pending; bit 6 receive completion pending; bit 7 any ring writer blocked while the first OACK was pending. It is displayed only after the first OACK has GLINK RX_DONE, and may be hidden by error screens. Counters and timing add overhead and cannot prove modem application acceptance. Frozen image SHA-256 `b08422827e46ebce6a9010aafce38a34121332519129d0ad489799ae005649a7`, under `.cache/sunfish-native-wifi/private/native-tftp-fairness-frozen-v1/`, passed AArch64 target build, 17 source checks, 280 capability mutations and boot-layout audit. It repeats the existing IPA/modem/RAM-only test and does not answer RRQs, serve WLAN firmware, start WLAN or flash. No device action has yet been made with this image.


Native fairness image `b08422827e46ebce6a9010aafce38a34121332519129d0ad489799ae005649a7`: IMG_9375 (SHA-256 `2018124a3689d50278e3eda28564843b4bf4044228b88e4f392b372ccd4cb012`) shows Q6 ready and runtime error zero. The first exact WRQ peer is node 0 port 0x1f, and the first OACK is matched to it. Host-observed queue, publication and GLINK RX_DONE ages are 0x1df = 479 ms, 0x7800 = 30,720 ms and 0x78a4 = 30,884 ms. First OACK flags 0xf include publication and GLINK completion. The first OACK was pending across 0x5e = 94 calls to the session publisher. Accumulated wait mask 0x73 indicates that at some calls a suitable intent existed (bit 0), the OACK was selected by next_transmit (bit 1), receive-intent advertisements were pending (bit 4), an intent ACK was pending (bit 5), and receive completion was pending (bit 6). The OR mask cannot say whether those conditions coincided, which one was selected, how long each lasted, or whether uninstrumented work dominated. No other-TX-selected (bit 2), outbound-intent-waiting (bit 3), or writer-WouldBlock (bit 7) was recorded while this OACK was pending; this does not rule out blocked work elsewhere. The first publication age remains similar to the previous 31,043 ms observation, so this one run does not demonstrate an improvement for that first OACK.

The screen also shows 0x20 = 32 WRQs, 4 RRQs and 2 DATA packets received, with WLAN RRQs zero. This differs from the preceding run's 19 WRQs and no displayed RRQs/DATA, but runs are not controlled-identical and counters do not establish which peer sent DATA, whether the server accepted it, or whether a write completed. Rejection code 2 is the server-check model's UnexpectedRequest, with a 73-byte rejected packet; the screen does not identify the request's path or opcode. The user reported the counters unchanged after about a minute; an unchanged display is not proof of execution or wire inactivity. The image does not answer RRQs, serve WLAN firmware, start WLAN, or flash.

### TFTP receive-control fairness candidate

The fairness device run in IMG_9375 recorded a matched OACK queue observation at 479 ms, publication observation at 30,720 ms, and RX_DONE observation at 30,884 ms. Its wait mask accumulates observations across calls, so it does not identify a simultaneous blocking condition or prove why publication was delayed. Two DATA packets and four read requests appeared in this run; their peer and acceptance are not established by those counters.

The next opt-in candidate gives a ready TFTP transmission a bounded turn after receive-control publication, while retaining ordering between advertisements, acknowledgements and completions. A host test reproduces starvation without the new selection; additional tests cover a granted intent acknowledgement and an uncertain publication. This is a candidate explanation, not a proven device fix. The same image classifies the first captured read request: 0 unavailable, 1 exact firewall ruleset, 2 exact modem EFS, 3 WLAN firmware filename suffix, 4 another parsed path. An unavailable result does not prove there was no read request; a filename match does not authorize a firmware transfer.

The target build, source audits, boot-layout audit and capability audit passed, including 280 rejected mutations. Host tests passed: 163 GLINK with the selected features, 160 baseline GLINK, 254 owner, 17 source and 12 diagnostic tests. The control-starvation mutation was rejected. The native compile guard was removed only from the isolated build copy. The frozen temporary image is `native-tftp-control-fairness-frozen-v1`, SHA-256 `b26384649909c05ac7ecbc8d7f4f2e9e60f11d2cbb10209ba451a4390f1224dc`. Its scope remains RAM-only server-check testing: no read responses, WLAN firmware serving, WLAN start, flash or persistent modem writes.

The one-shot control-fairness candidate was booted on sunfish. IMG_9376 shows `RUNTIME ERROR 0x73`, `ERROR DETAIL 0x54`, `LINUXIPA TIMEOUT`, IPA commits 1, responses 0, and TFTP RRQ RX 2. IPA TX fields are `FFFFFFFF` (unavailable). The captured RRQ path, decoded from the 19 displayed hex bytes, is `/readwrite/mcfg.tmp`. This error screen does not show the OACK scheduling timing or wait fields, so it cannot establish whether the candidate improved the previous publication delay or whether this timeout was caused by the scheduling change. This run did not test WLAN startup or reply to the read request.

The IPA owner uses the same 60-second timeout error for two conditions: an expired pending publication and an expired initialization deadline before modem-ready. These can both hold. The IMG_9376 `FFFFFFFF` transport fields mean the diagnostic found no retained Linux IPA transmit record at snapshot time; that alone does not prove which condition expired. Offline instrumentation now retains a two-bit cause (bit 0 pending publication, bit 1 initialization deadline) and the pending/queued/held-response state. When no IPA transmit snapshot exists, the timeout screen instead preserves first OACK queue/publication/RX_DONE ages and peer-match status, alongside the existing RRQ path. These observations are not proof of modem receipt. The modified IPA owner harness passed 186 tests and runtime source checks passed 17; the target build passed. This is diagnostic instrumentation, not a fix for the timeout or the TFTP delay.

The timeout-cause image passed the target build, boot-layout audit, source audit, 186 IPA-owner-harness tests, 17 runtime-source tests, and the capability audit with 280 rejected mutations. It is frozen as `native-ipa-timeout-cause-frozen-v1`, SHA-256 `0be7bc09d210f1cb9762c91316731fbb19000b258e8bb0fe3d487b287712ae25`. It repeats the existing temporary IPA/modem and RAM-only server-check test; it does not answer read requests, serve WLAN firmware, start WLAN, flash, or persist modem writes. No device attempt has yet occurred.

The timeout-cause image on sunfish produced IMG_9378 and, after about a minute, unchanged IMG_9379. Runtime error was 0; IPA commits 4 and responses 1. For the matched first OACK, queue age was 0x1da (474 ms), publication age 0x35e (862 ms), and GLINK RX_DONE age 0x7709 (30,473 ms); publication calls 2, accumulated wait mask 0x73. Times are host-observed and this single run cannot establish causality or modem application receipt. Compared with the previous observed 30,720 ms publication, the first publication observation was markedly earlier; the roughly 29.6-second observed publication-to-completion interval remains unexplained. Screen counters show 18 WRQ, 6 RRQ, 2 DATA, 0 WLAN RRQ; first RRQ kind 4 is another parsed path, not an identification of the path. The server-check rejection code 2 and 73 bytes do not identify the rejected packet. No timeout cause was displayed, since the IPA timeout did not recur. An unchanged display does not establish that the runtime is no longer executing.

The owner observes OACK publication after `publish_next` and a matching GLINK RX_DONE after `poll_receive` processes a completion for the busy remote intent and fully published transmit. Thus both timestamps are host-side observations; neither identifies when the modem put the completion into the ring. The pinned Linux GLINK handler likewise associates RX_DONE with a remote intent, but the saved Linux TFTP metadata capture does not include GLINK RX_DONE events, so it cannot determine this interval. The prior native DATA count is aggregate across peers and cannot establish whether the first OACK's peer acted on it.

An opt-in observation now counts received DATA packets only after the first OACK is queued and only when both the full peer and the local transfer port match its endpoint; it records the first matching host observation time without storing packet data. It does not establish that the server accepted a write, and zero matches cannot rule out earlier or unobserved data. A unit test and a real owner-path test cover other peers, wrong ports, exact matches and first timestamp retention. The RMTFS/owner harness passed 237 tests with the saved reference fixtures. This records data only; TFTP behavior and Wi-Fi scope are unchanged.

The DATA-peer observation image passed its target build, source audits, 237 host tests with reference fixtures, boot-layout audit and capability audit (280 rejected mutations). It is frozen as `native-tftp-data-peer-frozen-v1`, SHA-256 `9ef00d43376e5c811014f0a5c6e9a0d5982546a8d907527070117c52d4572846`. It repeats the previous temporary IPA/modem and RAM-only test, adds header-only DATA matching and timing, and does not answer reads, serve WLAN firmware, start WLAN, flash or persist modem writes.

DATA-peer candidate image `9ef00d43376e5c811014f0a5c6e9a0d5982546a8d907527070117c52d4572846` on sunfish produced IMG_9380: runtime error `0x73/0x54` Linux IPA timeout; timeout cause `2` reports initialization deadline without modem-ready, not expiration of the pending-publication deadline. `IPA PENDING 3` means a record remained pending and queued; `IPA HELD RSP 1` means a response was held. These fields do not identify the pending record or prove why it is still pending. The first matched OACK queue, publication and RX_DONE host observations were 478 ms, 870 ms and 30,727 ms respectively. The RRQ path displayed is `/readwrite/mcfg.tmp`, and four RRQs were counted. The error painter hid MATCH DATA fields, so this image does not establish a matched DATA result. No read response, WLAN firmware or WLAN startup was attempted.

IMG_9380's cause bit 2 identifies the initialization deadline; its pending/queued bits and held-response bit do not identify the pending message or establish whether its transport receipt has already arrived. `service_linux_ipa_step` checks the deadline before taking a pending receipt or accepting a held response. The transport can retire a completed IPA transmit into a separate receipt slot, so an absent retained transmit record is compatible with a receipt awaiting consumption, but does not prove it. A new read-only receipt-presence diagnostic and pending QMI message ID distinguish these cases without consuming the receipt or changing the deadline. The timeout screen also preserves matching-DATA count and observed age after the first 20 RRQ path bytes; the displayed full length still indicates when that prefix is truncated. Tests cover that receipt observation does not consume it; the IPA/GLINK link harness passed 154 and RMTFS/owner harness passed 237 tests. This remains diagnostic, not an initialization fix.

The IPA receipt-detail image passed its target build, source audits, 237 RMTFS/owner tests, 154 IPA/GLINK link tests, boot-layout audit and capability audit (280 rejected mutations). It is frozen as `native-ipa-receipt-detail-frozen-v1`, SHA-256 `20f89e2d90a52976a2950e189d719a851d87d641d6165c2a626771c2a1c9d827`. It repeats the temporary IPA/modem RAM-only test; no read replies, WLAN firmware serving, WLAN start, flash or persistent modem writes. No device attempt has yet occurred.

The receipt-detail image `20f89e2d90a52976a2950e189d719a851d87d641d6165c2a626771c2a1c9d827` produced IMG_9382: runtime error `0x73/0x54`, init deadline cause 2, no pending owner publication, held response 1, no waiting receipt, pending ID unavailable. The owner checks this deadline before accepting the held response; the photo does not show when that response arrived, whether it is valid, or whether the deadline policy itself caused the underlying delay. First matched OACK queue/publication/RX_DONE ages were 475/864/30522 ms as observed by the host; MATCH DATA 0, age unavailable. No matching DATA was observed after OACK queueing; this is not proof no packet arrived earlier or was missed. RRQ path is `/readwrite/mcfg.tmp`, count 2. This image does not answer reads, serve WLAN firmware, start WLAN, or flash.

Pinned Linux `drivers/net/ipa/ipa_qmi.c` issues INIT_DRIVER and waits for its QMI transaction from a workqueue; a separate receive path may complete it. In the native owner, the response is held while a prepared publication remains pending and the initialization deadline is checked before response acceptance. IMG_9382 cannot establish whether the response was observed before the deadline or is valid. An offline diagnostic now records the host counter when `LinuxIpaResponse` is observed, reports the elapsed ticks from native init preparation, and classifies strictly before versus at/after the same 60-second deadline. Missing or implausibly reversed counter values remain unavailable; this does not change timeout behavior or validate a response. The owner harness passed 239 tests, including pre-deadline and exact-deadline observations. The Linux function-entry trace cited above is not a modem transmit timestamp and does not establish behavior for this native run.

The response-age diagnostic image passed its target build, source audits, 239 RMTFS/owner tests, 154 IPA/GLINK link tests, boot-layout audit and capability audit (280 rejected mutations). It is frozen as `native-ipa-response-age-frozen-v1`, SHA-256 `be26285f5c2a41a46178575ce16193fbc2c09ea86d9a4bd40dc29e02eae2ffb9`. It repeats the temporary IPA/modem RAM-only test and does not change the deadline, answer reads, serve WLAN firmware, start WLAN, flash or persist modem writes. No device attempt has yet occurred.

The response-age image `be26285f5c2a41a46178575ce16193fbc2c09ea86d9a4bd40dc29e02eae2ffb9` on sunfish produced IMG_9383. It stopped with `0x73/0x54`, init deadline cause 2, pending/queued bits 3, held response 1, pending message ID 0x20 and receipt waiting 1. The host observed the held IPA response at age 0x8bbf = 35,775 ms, before the 0xea60 = 60,000 ms initialization deadline (classification 1). This is host observation of an opaque response, not validation or peer transmit time. Deadline enforcement occurred before the owner consumed the waiting publication receipt and held response. It does not establish when the receipt became available or whether the response is valid. Matching DATA count 0 after the first OACK was queued. Matched OACK queue/publication/RX_DONE ages were 475/864/30540 ms as observed by the host. No reads were answered and WLAN was not started. Inspect ownership and tests before changing deadline behavior.

The Linux IPA client receives INIT_DRIVER independently from the server's indication-registration response. `ColdSession::receive_init_response` validates the source, QMI kind, ID, transaction, result and the already-committed initial request, and can do so while a later server publication remains pending. An opt-in native candidate allows this validated response to be accepted independently without consuming the registration receipt or committing registration early. At or after the initialization deadline it is eligible only when the same host counter observed it strictly before the deadline; absent, late, or inconsistent timestamps remain ineligible. A pending-publication timeout still stops first. An uncommitted INIT_DRIVER cannot be bypassed. The experiment can process a timely response after the deadline when owner servicing was delayed; it does not extend eligibility for responses first observed at or after the deadline.

The candidate passed 245 RMTFS/owner host tests, including early independent processing with the registration still pending, late owner service of a pre-deadline response, exact-deadline and missing-clock rejection, expired-registration timeout, and invalid transaction/remote failure. With the experiment disabled, 239 tests passed. This is a candidate for the observed race, not proof that the device response is valid or that WLAN will start.

The independent IPA response candidate passed the target build, source audits, 245 experiment-enabled owner tests, 239 experiment-disabled baseline tests, 154 IPA/GLINK link tests, boot-layout audit and capability audit (280 rejected mutations). It is frozen as `native-ipa-independent-response-frozen-v1`, SHA-256 `25a20057395752a94acf2e85c7ff95698819e8aa8ca51d0d15f5246167242541`. This one-shot temporary image changes the IPA response-processing order as described above and repeats the existing RAM-only IPA/modem test. It does not answer TFTP reads, serve WLAN firmware, start WLAN, flash or persist modem writes. No device attempt has yet occurred.

The independent-response candidate `25a20057395752a94acf2e85c7ff95698819e8aa8ca51d0d15f5246167242541` on sunfish produced the first runtime photo and IMG_9384 after the wait. Both show IPA flags `0x4f8`: the runtime latched protocol-complete and its state bits report modem-ready, QMI UC-ready, indication requested and indication sent; commits 4, responses accepted 1, runtime error 0. This is one successful protocol observation, not proof that the new independent branch executed or that Wi-Fi or the IPA data path works. TFTP counts stayed at WRQ 0x16 (22), RRQ 4, DATA 2, WLAN RRQ 0; no DATA matched the first OACK peer and transfer port after queueing. First matched OACK queue/publication/RX_DONE ages are 475/864/30544 ms as observed by host; accumulated wait mask 0x73, publisher calls 2. A 73-byte request was rejected with code 2, without identifying its opcode or path. No WLAN, RRQ response, firmware serving, flash or persistent writes were attempted. An unchanged screen does not prove the runtime has stopped.

Offline comparison: the saved Linux `tqftpserv` WRQ/OACK construction and prior source oracle agree with the modeled 39-byte OACK for the captured options; Linux's metadata-only trace saw DATA from the same connected peer shortly after its OACK, but did not capture GLINK completion. Native transfer ports are accepted by the current QRTR receive router. Those facts do not explain the native run's missing same-peer DATA or prove modem application receipt.

An opt-in cursor observer tracks the first fully published OACK's FIFO boundary using existing validated descriptor samples. It records the first host observation that the peer tail passed all bytes up to that boundary, without adding descriptor/MMIO reads; partial consumption is not enough. The owner test covers partial advancement, the complete boundary, and subsequent distinct RX_DONE. Cursor passage proves FIFO-byte consumption only, not application acceptance; missing timing does not prove non-consumption because observations require later host activity. The target build, source audits, 245 observer-enabled and 245 disabled host tests, boot-layout audit, and capability audit (280 rejected mutations) passed. The one-shot image is frozen as `native-tftp-tail-observer-frozen-v1`, SHA-256 `de8da0ad5bfa5d1d88dc8c426e30d61ff0833ed10a1b22a33e7bb6364509a72d`; it keeps the existing IPA ordering experiment and RAM-only server-check scope, with no RRQ replies, WLAN firmware serving, WLAN startup, flash or persistent modem writes. No device attempt has yet occurred.

The FIFO-tail observer image `de8da0ad5bfa5d1d88dc8c426e30d61ff0833ed10a1b22a33e7bb6364509a72d` on sunfish produced two unchanged runtime photos after a wait. IPA flags `0x4f8` report the protocol-complete latch with 4 committed publications, one accepted response and no runtime error. The first OACK's host-observed queue, full publication, peer TX-tail crossing and RX_DONE ages were 479, 871, 896 and 30,780 ms respectively; the peer's cursor passed the record in the first sampled interval, but that does not prove modem application receipt or establish when RX_DONE entered the host ring. Match DATA remained zero, while aggregate DATA was 2, WRQ 18 and RRQ 6 (WLAN RRQ zero). Reject code 2 and byte count 73 do not identify the rejected packet. No RRQ response or WLAN start was attempted. An unchanged display does not prove the runtime stopped.

Follow-up offline investigation: the prior IMG_9277 measured substantial aggregate owner-dispatch time, including native GLINK observe, descriptor and FIFO operations, but covered a different run and not the exact OACK-to-RX_DONE interval. To distinguish a long interval between native owner observation points from frequent owner observations while the peer completion remains absent, an opt-in diagnostic tracks the maximum counter gap and count between existing `observe_tftp_timing` calls from the first OACK's fully published state through its GLINK completion observation. It uses the existing counter read and adds no register or shared-memory reads. The maximum gap is elapsed host time between software observation points, not a direct measurement of a specific operation, the time the completion entered the ring, or modem send time; a missing or small gap cannot prove the peer was slow. Error paths can hide or truncate observations. The 246-test host owner suite passed, including an explicit synthetic long gap and stopping the measurement after completion; 245 tests passed with this observer disabled. The target build passed. This diagnostic does not change transfer handling.

The host-observation-gap image passed the target build, source audits, 246 observer-enabled and 245 disabled host tests, boot-layout audit and capability audit (280 rejected mutations). It is frozen as `native-tftp-host-gap-frozen-v1`, SHA-256 `ddb09daf00f02e65537bd98e4d6a77c4b60d60fd1023002fd4427352b5bf535d`. It repeats the existing temporary IPA/modem RAM-only test; it does not answer reads, serve WLAN firmware, start WLAN, flash or persist modem writes. No device attempt has yet occurred.

The host-gap image `ddb09daf00f02e65537bd98e4d6a77c4b60d60fd1023002fd4427352b5bf535d` ran on sunfish. Two photos after a wait show unchanged runtime state: IPA flags 0x4f8, commits 4, accepted responses 1, runtime error 0; TFTP WRQ 21, RRQ 4, aggregate DATA 2, matching first-OACK peer/port DATA 0, WLAN RRQ 0. First OACK queue/publish/peer-tail/RX_DONE host observations were 478/869/894/30722 ms. During the published-to-RX_DONE interval the owner recorded 186 observation points and maximum host-observation gap 0x5cc = 1484 ms. Therefore this run does not show one continuous 30-second gap without an owner observation. The points are after software operations, not timestamps of the modem's send or proof that RX_DONE was present in the ring; queueing behind other receive records or a peer delay remain possible. The unchanged screen does not establish execution has stopped. No WLAN, TFTP read responses, firmware serving, flash or persistent writes were attempted.

The receive-backlog image `18b3ef80ec169bf5d77c2e60447c9bbe0b583bff2d47fea2614dfb64d4f326cc` adds an opt-in observation of the last sampled empty receive FIFO and maximum sampled FIFO occupancy while the first OACK awaits completion, using already validated cached samples and including the startup queue in the empty check. The observation timestamp is taken after the owner operation, not at the exact descriptor read; an empty sample says nothing about other software queues or when RX_DONE was sent or entered the ring. The target build, source and boot-layout checks, 248 observer-enabled and 246 disabled host tests, and capability audit (280 rejected mutations) passed. A single temporary boot was accepted on sunfish; device results are pending. The test does not answer reads, serve WLAN firmware, start WLAN, flash, or persist modem-storage writes.

The receive-backlog image `18b3ef80ec169bf5d77c2e60447c9bbe0b583bff2d47fea2614dfb64d4f326cc` produced the 2026-11-07 22:33 photo. The runtime reported IPA flags 0x4f8, four commits, one response, Q6 ready, and runtime error zero. First OACK queue/publish/peer-tail/RXDONE ages were 475/864/888/30534 ms. RX EMPTY MS 0x755f = 30047 ms is the end-of-operation timestamp of the last cached empty FIFO and empty startup queue sample during the interval; sampled maximum RX FIFO occupancy was 0x20 = 32 bytes. Maximum interval between owner observations was 0x5c7 = 1479 ms. The empty sample was observed approximately 487 ms before host processing of RXDONE, but the descriptor read precedes the timestamp and other software queues are not covered; this does not establish when RXDONE entered the FIFO or when the peer sent it. TFTP WRQ/RRQ/DATA counts were 0x16/5/2, WLAN RRQ zero, reject code 2; the photo alone does not identify whether the two DATA packets matched the first OACK. No device result for WLAN startup or firmware transfer is claimed.

Offline completion comparison: the pinned Linux reference GLINK source at commit `2267e248204e7e051e0f7355298949a6440de579` emits `qcom_glink_cmd_tx_data` just before invoking its TX function and `qcom_glink_cmd_rx_done` on receipt of a completion. The corresponding pinned trace definitions expose remote/channel, channel IDs, intent ID, size/continuation, direction and reuse, rather than payload bytes. Matching by channel/intent can measure Linux-observed intervals, subject to intent reuse, concurrency, trace loss and the fact the TX event precedes actual publication. The saved Linux TFTP metadata independently shows a server-check OACK followed by same-peer DATA about 0.4 ms later, but has no GLINK events and cannot establish the OACK's RX_DONE interval. Native FIFO empty samples constrain continuous queued backlog only; they do not prove when a particular completion arrived or why the peer did not send same-peer DATA.

`platforms/sunfish/mobian-comparison/prepare-glink-tftp-reference.py` prepares two guarded paths for a possible fresh temporary Linux reference boot: the normal startup and its provider-ready continuation. They retain the earlier RAM-backed EFS and TFTP metadata observer, and add a private tracefs instance with only GLINK data and completion event types enabled. The preparer checks pinned reference image, input bundle, tracer, source-script and kernel-config hashes, verifies the exact trace definitions in the pinned source, and shell-checks both scripts. A separate host check verified their hashes, trace setup after module load and before modem start, and cleanup registration. The kernel config enables trace events, but actual event availability and formats remain runtime checks. The trace includes all GLINK channels' metadata, not payload; its clock, per-CPU loss stats and event formats are saved, and tracing can perturb timing. Correlating a Linux TFTP packet to a GLINK record by timestamp and size is not unique proof under concurrent traffic. No Linux boot or capture has yet occurred using these scripts.

A single temporary Linux reference boot of `6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d` ran the GLINK/TFTP comparison. The initial provider lookup was empty, so startup stopped before modem start; a guarded continuation verified both providers and made the first modem-start attempt. It reported success, modem running, wlan0 present and all four raw EFS hashes unchanged. The mono-clock private trace recorded 1,634 events with zero reported per-CPU overruns, commit overruns or drops. At 212.502335 s, the tqftpserv task emitted a 72-byte outbound IPCRTR TX trace on intent 0; at 212.502415 s a matching RX_DONE for intent 0 appeared, about 80 microseconds later. The same task's metadata recorded a 39-byte OACK send returning at 212.502376897 s and same-peer DATA at 212.502836168 s. Task, size and timing strongly support associating that trace record with the OACK; no payload was captured, so the association is not cryptographic proof. TX trace precedes publication, and Linux reference timing does not establish when or why the native peer sent its RX_DONE. This confirms the approximately 30-second native host-observed interval is not inherent to the same reference exchange. Raw artifacts and hashes are in `.cache/sunfish-native-wifi/private/glink-tftp-reference-prepared-v1/run/summary.json`. This Linux test did not associate to a network or flash; it does not establish native WLAN operation.

Follow-up on first-OACK attribution: the diagnostic latches the transmit slot for the first queued OACK and sets its completion flag only after `receive_done` finds the busy peer intent associated with a fully published transmit in that slot. An added host regression queues OACKs for two peers, completes the second peer first, and verifies the first OACK remains incomplete until its own intent completes. The standalone GLINK/QMI suite passed all 142 tests with the endpoint and reference-flow features enabled. This rules out that simple cross-peer misattribution in the exercised model, not device-side observation mistakes or timing effects. The native transport performs repeated lifecycle, peer, directory, descriptor and lock checks around individual IO and barriers; earlier IMG_9277 measured substantial aggregate time in these categories, but that was a different run and does not establish which operation caused the current first-OACK delay. In the new Linux capture, host-observed server-check WRQ-to-OACK-send-return was about 0.476 ms; native first-WRQ-observation-to-OACK-publication was about 864 ms in the latest run. These measurements have different observation points and instrumentation. The approximately one-second TFTP timeout makes native latency relevant to examine, but neither this comparison nor FIFO-tail movement proves the first modem client timed out or that OACK delivery failed.

Offline candidate: feature `sunfish-glink-aligned-fifo-reads` makes `Controller::glink_read` use bounded aligned 32-bit `Read::read_le32` operations for complete words, with byte reads for unaligned leading/trailing portions. It is disabled by default. The requested span and pre/post lock, lifecycle, directory, descriptor and barrier checks are unchanged; GLINK record-level independent readback/compare remains in place. The native retained SMEM `Window` implements aligned `read_le32` as one volatile word read, while the trait default remains byte-based. The change alters the timing and granularity of samples of peer-owned memory; matching output for stable data and retained double-read checks do not prove equivalence under every concurrent mutation. Host span tests checked offsets and lengths for byte equality and exact in-range accesses, including unaligned edges; a retained native-memory integration test checked RX/TX offsets and FIFO boundaries. The enabled suite passed 286 tests (one ignored), disabled baseline 284 (one ignored), and FIFO contract suite 117; isolated AArch64 release build and boot layout passed. This is a potential CPU IO reduction, not evidence of a real-device speedup or a fix for the observed GLINK completion delay. Device image capability audit is still in progress.

The aligned-FIFO candidate is frozen as `native-glink-aligned-fifo-frozen-v1`, image SHA-256 `d31da27577aaec58b270d0998627c76b39a5cb9849e3429dd936b35bba241c00`. Capability audit passed with 280 rejected mutations, and the frozen source, fixture, build-input, image and feature hashes were verified. It retains the prior experimental IPA ordering and RAM-only server-check test. Aligned FIFO reads change peer-memory sampling granularity and timing; they do not remove the range, lock, lifecycle or double-record checks. No RRQ responses, WLAN firmware serving, WLAN startup, flash or persistent modem writes are included. No device attempt has yet occurred.

The aligned-FIFO image `d31da27577aaec58b270d0998627c76b39a5cb9849e3429dd936b35bba241c00` was booted once on sunfish. IMG_9385 (SHA-256 `89ce5bd9ed6de94fb52b313d38201b97675104228613fcd685db9b2409e8314c`) shows IPA flags 0x4f8, commits 4, responses 1, Q6 ready and runtime error 0. First matched OACK queue/publication/peer-tail/RXDONE host-observed ages were 479/871/895/30769 ms; RX last empty observation 30278 ms, maximum sampled RX backlog 32 bytes, max host observation gap 1485 ms. Aggregate TFTP WRQ/RRQ/DATA counts 22/5/2 and WLAN RRQ 0. Compared with the immediately preceding byte-read run's 475/864/888/30534 ms ages, this single run does not demonstrate an improvement in the measured OACK or completion timing. The snapshots and timestamps are host observations, not modem-send times; the empty observation only covers the sampled FIFO and startup queue. This image does not show whether the two DATA packets matched the first OACK and did not answer reads, serve WLAN firmware, start WLAN, flash, or persist modem-storage writes.

Offline follow-up to IMG_9385: changing byte reads to aligned word reads did not materially change that run's observed OACK timing. The native runtime's `paint_progress` replaces four full 1080-by-72 rows in two framebuffer buffers whenever an owner pass begins after seven RMTFS requests; each call therefore clears 622,080 pixels before drawing glyphs. `show` additionally clears both 1080-by-2340 buffers and redraws rows on many notifications or changed reports. The earlier IMG_9277 paint measurement covered a different workload and does not prove painting caused this OACK delay. The display-only opt-in `sunfish-tftp-quiet-oack-experiment` keeps progress checkpoint state but suppresses routine progress-row paints while the first observed server-check OACK is outstanding. It gates normal full repaints during that interval, permits another on a qualifying event after five seconds, and retains immediate error handling and the completion-driven refresh. It does not change IRQ handling, transport scheduling, messages or service calls. No timer is introduced, so in the absence of a later event the screen can remain stale; timing changes can affect the peer. The display-policy host tests pass 13 cases including invalid/backwards/wrapped clocks, errors and completion; runtime source tests pass 18 and both feature-enabled and feature-disabled isolated AArch64 release builds pass. The candidate boot layout passes; capability audit is in progress. The candidate retains the previously tested aligned-FIFO-read feature to isolate display policy against IMG_9385. No new device run has occurred.

The quiet-OACK candidate was frozen as `native-tftp-quiet-oack-frozen-v1`, SHA-256 `918c9e7844b85c91787eb59ba00c96a50054020ef073a027ba9df994ce3f092e`. Its boot layout, source audits, enabled/disabled AArch64 builds, 13 diagnostic tests, 18 runtime source tests and capability audit (280 rejected mutations) passed; source, artifact and input hashes were verified. A single temporary boot was accepted on sunfish. This image retains aligned FIFO reads and the existing IPA/TFTP experiments so the display change can be compared with IMG_9385. It does not answer RRQs, serve WLAN firmware, start WLAN, flash, or persist modem-storage writes. During the first pending OACK, normal display updates may be held until a later qualifying event after five seconds; error screens are immediate. Device result pending.

Quiet-OACK display image `918c9e7844b85c91787eb59ba00c96a50054020ef073a027ba9df994ce3f092e` produced IMG_9386 (SHA-256 `25a678d97492a65f6de29674de03677b7cb40449a4d2e0edc17b019618a6876d`). The display reports IPA flags 0x4f8, four commits, one response, Q6 ready and runtime error zero. First matched OACK queue/publication/peer-tail/RXDONE host observations were 475/863/888/29744 ms; last empty RX snapshot 29257 ms, max sampled FIFO occupancy 32 bytes, and max host observation gap 777 ms. TFTP WRQ/RRQ/DATA counts are 22/4/2, WLAN RRQ zero. Compared with IMG_9385's 479/871/895/30769 ms and max host gap 1485 ms, the observed largest host gap is smaller in this run, but publication and completion still take roughly the same time. Different runs and instrumentation do not establish the display policy caused the gap change or prove when the peer sent RX_DONE. The screen shows aggregate DATA only; it does not identify matching peers. No RRQ response, WLAN firmware serving, native WLAN startup, flashing, or persistent EFS write was attempted.

After IMG_9386, source inspection found the native owner observes the first exact WRQ inside its bounded transport turn, then performs an outbound publication step before returning. The runtime then calls registry, RMTFS and IPA service steps before the TFTP server-check dispatcher. The earlier 475 ms request-to-queue age includes those operations and intervening work; it does not identify which call dominates. Linux's metadata shows much shorter WRQ-to-OACK time in a different, traced run, not a direct per-call comparison.

The opt-in `sunfish-tftp-oack-phase-observer` records the first WRQ's host timestamp and the existing end-of-call counter values for the first owner dispatch, registry, storage and IPA steps. The normal screen displays those cumulative ages alongside queue, publication and RXDONE ages; `FFFFFFFF` means unavailable or reversed. It adds no counter reads or hardware access, and changes no service order or packet handling. These are cumulative host observations after calls, not exact peer times or exclusive CPU attribution; sampling and rendering can perturb timing. It retains the quiet-OACK display policy and aligned FIFO reads from IMG_9386. The diagnostic suite passed 14 tests, runtime source suite 19, source audits passed, enabled and disabled AArch64 release builds passed, and boot layout passed. Capability audit is in progress; no device result for this observer exists.

The first-OACK phase observer is frozen as `native-tftp-oack-phases-frozen-v1`, SHA-256 `c68a0394bd0c6849dc70af687faa67356dd764793d65cb7a98ee4234313e4450`. Its 14 diagnostic tests, 19 runtime source tests, source audits, enabled/disabled AArch64 builds, boot-layout audit and capability audit (280 rejected mutations) passed. Frozen source, build inputs, test fixtures and artifact hashes were verified. It preserves the quiet display and aligned FIFO-read experiments used in IMG_9386, does not answer reads or start WLAN, and performs no flash or persistent modem-storage writes. No device attempt has yet occurred.

The phase-observer image `c68a0394bd0c6849dc70af687faa67356dd764793d65cb7a98ee4234313e4450` ran once on sunfish. IMG_9388 (SHA-256 `32ab846890353a28736a41f7de8a6660e0242ab83e542b66935f0f4e46b7d731`) reports Q6 ready, IPA flags 0x4f8 with four commits/one response, and runtime error zero. The first WRQ host-observation-to-boundary ages are dispatch 206 ms, registry 230 ms, storage 255 ms, IPA 328 ms, first matched OACK queued 475 ms, published 863 ms and RXDONE observed 29748 ms. The differences between sampled boundaries are approximately 24, 25, 73, 147 and 388 ms, respectively, but include intervening host work and do not establish exclusive call CPU time or modem send/receive timestamps. Dispatch after WRQ observation and the queued-to-publication interval together are substantial; no individual service accounts for the full delay. Counts show 18 WRQ, 6 RRQ, 2 aggregate DATA and zero WLAN RRQ. No RRQ reply, WLAN firmware serving, WLAN start, flashing or persistent modem-storage write was attempted.

Following IMG_9388, the opt-in `sunfish-tftp-rx-yield-experiment` gives a newly held TFTP request a service turn before one otherwise immediate `publish_next` call. A debt flag requires a publication attempt before another such early yield, including under a stream of distinct requests; coalesced requests do not trigger it. Existing lifecycle and publication checks remain in place. This can delay outgoing control or data by one service turn and can change fragment interleaving; it is a timing hypothesis, not proof that the original long RXDONE delay is caused there.

The first feature-enabled host run exposed a test helper that assumed fragments of different GLINK intent IDs cannot interleave. The candidate caused a data fragment from intent 2, a complete control datagram on intent 3, and then the remainder of intent 2. The updated test helper tracks fragments separately by intent and rejects a duplicate start or missing start. The Linux reference GLINK receiver's intent-based path also associates incoming fragments with an intent ID; this does not prove the modem's behavior for the candidate. With that correction, the selected owner suite passed 180 tests with the experiment and 179 without it; the integrated IPA/RMTFS suite passed 245 with a 32 MiB host test-thread stack. Both AArch64 feature-enabled and disabled release builds, 19 runtime source tests, 14 display diagnostics, source audits and the boot layout passed. The isolated build retains the earlier explicitly recorded close-handoff prototype treatment: the native compile guard remains in the workspace and is removed only from the isolated build copy. The capability audit is still pending; no device attempt of this candidate has occurred.

The bounded receive-yield image is frozen as `native-tftp-rx-yield-frozen-v1`, SHA-256 `e88f38738aa2a86dabfdb2577f884d364f76bad483da5e93c3933c3d9279e2d5`. Capability audit passed with 280 rejected mutations. Inputs, source closure and frozen artifacts are recorded in its manifest. Its timing and modem compatibility remain unproven; it does not answer reads, serve WLAN firmware, flash or start Wi-Fi.

The first receive-yield package `e88f38738aa2a86dabfdb2577f884d364f76bad483da5e93c3933c3d9279e2d5` was sent once and rejected by Fastboot at boot image verification with `Unsupported`; no runtime result was obtained and this attempt is consumed. The package mistakenly used the legacy LZ4 frame (`02214c18`), while the earlier accepted package uses the standard frame (`04224d18`). A new package uses the same standard frame header as the earlier image; decompression matches the validated loader byte-for-byte, and the complete boot-layout check passes. The new frozen image is `eec62cbbf4ee39a2060ef030da8c6d773fd628b13dd1f29829fc601957144c93`; it has the same compiled payload and previously passed capability audit as the rejected package, and its changed packing has no device result yet.

The corrected standard-frame receive-yield image `eec62cbbf4ee39a2060ef030da8c6d773fd628b13dd1f29829fc601957144c93` was accepted for one temporary boot. IMG_9389 (SHA-256 `623c33d112e10708343fa5e03f93361450c12e814469f979663166e4bcc131b6`) shows Q6 ready, IPA flags 0x4f8, four commits, one response, and zero reported runtime error. First-WRQ host-observed cumulative ages are dispatch 0x30 (48 ms), registry 0x49 (73 ms), storage 0x61 (97 ms), IPA 0xab (171 ms), queue 0x13d (317 ms), publication 0x2c2 (706 ms), and RXDONE 0x7537 (30007 ms). Compared with the one prior phase-observer run (206, 230, 255, 328, 475, 863, 29748 ms respectively), dispatch and downstream ages are roughly 157–158 ms earlier, while RXDONE remains about 30 seconds. This single-run comparison is consistent with the early-yield hypothesis but does not establish repeatability or exclusive timing attribution. TFTP counts are 0x14 WRQ, 4 RRQ, 0 WLAN RRQ and 2 aggregate DATA; reject code 2 remains shown. Neither these counters nor transport completion prove matching peer DATA, successful server-check completion, or Wi-Fi startup. Quiet-display policy can leave the frame stale until another event; a follow-up observation is pending.

Follow-up IMG_9390 (SHA-256 `741545a59270192315960f5af0c403db6ae6c9d9ce723fff4919cc9c9bbb0050`) shows the same visible timings, TFTP counts and zero runtime error as IMG_9389 after the wait. The quiet display can remain stale until a subsequent event, so unchanged pixels do not prove the runtime is halted. No further progress to a matching TFTP transfer or native WLAN is demonstrated.

Offline follow-up: the remaining first-OACK queue-to-publication observation is 389 ms in IMG_9389, compared with 388 ms in the prior run, and the approximately 30-second RXDONE observation remains. Host `Ring::write` samples lifecycle and descriptors before writes, verifies payload readback, publishes the head, then samples and notifies the peer; the displayed publication timestamp is recorded after this operation and is not the exact head or doorbell time. No evidence yet identifies whether the long completion interval occurs in the peer, in delivery or observation, or in client lifetime.

The opt-in `sunfish-tftp-peer-close-observer` records the first host-observed close for the port of the first observed WRQ, including the queued/published/completed flags at that observation. It watches normal validated `ClientRemoved` events and the guarded TFTP-close-pending handoff event; it adds a counter read only on those events and changes neither packet handling nor transport validation. A close event that fails before either event is returned may not be recorded; no displayed close does not establish that the peer remained open, and matching a port cannot rule out reuse or delayed events. Times describe host event observation, not peer send time. Host tests check unrelated ports, phase latching, the normal control path, and the guarded close-handoff path; the 246-test integrated suite and the existing 19 runtime source tests passed. This is an observation, not a fix or evidence of Wi-Fi readiness. Target build and boot-layout check passed; the capability audit and disabled-feature target build are in progress. No device attempt has occurred for this observer.

The peer-close observer image is frozen as `native-tftp-peer-close-observer-frozen-v1`, SHA-256 `6da06debe5a894eab496b58d80d13fea792d46ea6d8208a661496c7b737b25f1`. The capability audit rejected all 280 mutations, the enabled and disabled target builds passed, the standard LZ4 frame decompresses exactly to the validated loader, and source closure, boot layout and inputs were checked. It retains the prior receive-yield and RAM-only test. It does not answer reads, serve WLAN firmware, start WLAN, flash or persist modem writes. No device attempt has yet occurred.

The peer-close observer image `6da06debe5a894eab496b58d80d13fea792d46ea6d8208a661496c7b737b25f1` ran once on sunfish. The user explicitly excluded IMG_9391; IMG_9392 (SHA-256 `a5cd0173e8f96f5fd9d9ef3c54909a0829dd4d91dcf085a73b43c9009804d927`) is the relevant photo. It shows Q6 ready, IPA flags 0x4f8 with 4 commits and 1 response, and runtime error 0. The first WRQ host-observed OACK queue and publication ages are 0x13f = 319 ms and 0x2c7 = 711 ms; first matching client-port close is observed at 0xa65 = 2661 ms, phase 3 (queued and published, completion not yet observed); GLINK RXDONE is observed at 0x760a = 30218 ms. The host therefore processed a matching-port close between its publication and completion observations. This is not the modem's exact event time and cannot eliminate delayed close records or port reuse; it does not prove the OACK caused the close. WRQ 14, RRQ 10, WLAN RRQ 0, DATA 2 are aggregate counts; reject code 2 and 73 rejected bytes do not identify the rejected request. No read response, WLAN firmware, WLAN start, flash or persistent modem write was attempted. The display may remain stale without another event.

The captured first server-check WRQ requests `timeoutms=1000`. IMG_9392 reports first OACK publication observed 711 ms after the host observed that WRQ and matching-port close observed at 2661 ms. Because the modem's send and timeout clocks are not observed, and the publication timestamp follows the ring writer's checks, these facts do not establish that the first OACK was late, that a timeout caused the close, or that the 30-second completion was caused by TFTP. The Linux reference saw its server-check OACK in approximately 0.5 ms and matching DATA shortly afterward, a different instrumented run.

The opt-in `sunfish-tftp-early-service-experiment` calls the same bounded RAM-only server-check handler once at the start of the service pass instead of after registry, RMTFS and IPA. Registry, RMTFS and IPA still run on a successful pass; a TFTP error now stops the pass before them. All existing TFTP bounds, GLINK checks and other service logic remain. It retains the receive-yield and matching-port close observer. Source regression checks assert the feature is explicit and the two mutually exclusive placements select one handler call; these are structural checks, not a runtime scheduling proof. Enabled and disabled AArch64 builds, source audits, 20 source tests and boot layout passed. The prior 246-test integrated owner suite remains relevant to the unchanged owner path, but does not exercise this runtime ordering. Standard-frame LZ4 decompression matches the loader. Capability audit is in progress; no device attempt has occurred.

The earlier-service candidate is frozen as `native-tftp-early-service-frozen-v1`, SHA-256 `d90d6bf83ef11368d2d88ce75b54b5f019a06d67cd20f804082c5782e72cdd7d`. Capability audit passed with 280 rejected mutations; enabled/disabled AArch64 builds, 20 source tests, source audits, boot layout, standard LZ4 round trip and frozen input/source-closure checks passed. The parent 246-test owner suite covers the unchanged owner path, but does not exercise the runtime service ordering itself. The test remains RAM-only, does not answer reads or serve WLAN firmware, and does not start WLAN, flash or persist modem storage. An earlier TFTP error can stop a pass before registry, storage and IPA; successful passes still run all of them. No device attempt has yet occurred.

The earlier-service image `d90d6bf83ef11368d2d88ce75b54b5f019a06d67cd20f804082c5782e72cdd7d` was accepted for a single temporary sunfish boot. IMG_9396 (SHA-256 `c14840b2a6367998a69d986efae2cc810e324cf65d21b78557b5e7509c0404b3`) shows Q6 ready, IPA flags 0x4f8, 4 commits, one response and runtime error 0. The first matching WRQ host-observed queue age is 0xc5 = 197 ms, publication 0x2c8 = 712 ms, matching-port close 0xa68 = 2664 ms, and RXDONE 0x7625 = 30245 ms; close phase 3 says queued and published, but not completion, at the host close observation. The previous image reported 319/711/2661/30218 ms. These two individual runs are consistent with a ~122 ms earlier queue observation but do not show a meaningful publication or completion improvement. They do not establish peer packet or close send times, or rule out event delay or port reuse. Counts are WRQ 14, RRQ 10, WLAN RRQ 0, aggregate DATA 2, rejection code 2/73 bytes; the display does not show a matched DATA result. No read replies, WLAN firmware or startup, flash or persistent modem-storage writes were attempted.

After IMG_9396, source review found that `ResponseQueued` schedules a router continuation, but the early-service profile still runs registry, storage and IPA before its next normal `Router::dispatch`. An opt-in `sunfish-tftp-early-dispatch-experiment` invokes one extra bounded actual router dispatch immediately after that handler returns `ResponseQueued`, before the other service steps. It uses the existing continuation, IRQ translation, lifecycle and FIFO gates, does not loop on backpressure, and resolves a guarded TFTP close if one arises in that dispatch. On successful passes registry, storage and IPA still run, but the extra bounded dispatch can delay them; a dispatch or close error stops the pass before them. A host integration test exercises an actual owner, fake FIFO and IRQ router: the queued OACK publishes in the next dispatch and the original physical IRQ is rearmed exactly once. The feature-enabled IRQ/owner suite passed 197 tests; 21 runtime source tests and source audits passed. These fixtures do not prove the extra turn will select or publish the first OACK under device traffic or improve timing. The enabled target build, boot layout and standard LZ4 round trip passed; disabled build and capability audit remain in progress. No device attempt has occurred.

The extra-dispatch candidate is frozen as `native-tftp-early-dispatch-frozen-v1`, SHA-256 `edd948ae967d29554e2c791b57831d88d2dd40e4c91552e5f0cc504df8e5bd14`. The capability audit rejected 280 mutations; 197 enabled IRQ/owner host tests, 21 source tests, both target builds, source audits, boot layout, standard LZ4 round trip and frozen input/source-closure checks passed. The added dispatch is bounded but may delay other services or terminate the pass on an error, and host fixtures do not establish actual device timing or that the first OACK is selected on that turn. It retains the prior RAM-only scope and does not answer reads, serve WLAN firmware, start WLAN, flash or persist modem storage. No device attempt has yet occurred.

The extra-dispatch image `edd948ae967d29554e2c791b57831d88d2dd40e4c91552e5f0cc504df8e5bd14` booted once on sunfish. IMG_9401 showed MPSS auth/ready pass and IMG_9402 an earlier in-progress runtime frame. IMG_9403 (SHA-256 `f061078fb74d0b6713769f3ad88b643f2474ea8633155a939a0cb2c18a311aa5`) shows Q6 ready, IPA flags 0x4f8, four commits, one response, and zero runtime error. First-WRQ host-observed queue/publication/matching-port close/RXDONE ages were 0xc3/0x247/0x8be/0x7457 = 195/583/2238/29783 ms; close phase 3 indicates queued and published but not completed at the host close observation. Compared with prior IMG_9396's 197/712/2664/30245 ms, publication was observed 129 ms sooner in this single run, while completion was still ~30 s and the matching-port close remained earlier. The host-observed storage and IPA phase boundaries were 0x6e7/0x730 = 1767/1840 ms, versus 246/320 ms in IMG_9396. These cumulative timestamps include the inserted router dispatch and intervening work, are from separate runs, and do not measure exclusive storage/IPA time; the added dispatch is a plausible contributor but not proven causal. WRQ 16, RRQ 7, DATA 2 and WLAN RRQ 0 are aggregate counts. Close timing is host-observed and may be affected by delayed records or port reuse. No RRQ response, WLAN firmware, WLAN start, flash or persistent modem storage write was attempted.

Offline follow-up: the extra-dispatch run moved the publication observation earlier in one comparison but did not improve the ~30-second completion and had substantially later cumulative storage/IPA checkpoints. The extra-dispatch feature is therefore disabled in the next diagnostic; the earlier bounded service and receive-yield remain enabled. The opt-in `sunfish-tftp-publish-io-observer` snapshots the existing per-category native-IO profiler totals before and after the normal owner/router dispatch in which the first queued OACK changes to published. It adds no new hardware reads or counter calls: that dispatch was already profiled. The reported deltas cover the entire owner turn, including any other work, not only the OACK write; `IO OTHER` combines writes, barriers, cursor stores, notifications and RX-length checks. Missing/reversed/saturated observations remain unavailable and the first observed transition is latched. A compile-time guard excludes the extra-dispatch feature because it would publish outside this profiling window. Fifteen diagnostic tests and 22 source tests pass, including absent flags, invalid and saturated counters, first-transition latching, and feature exclusion checks; enabled target build and source audits passed. Boot-layout check, disabled target build, and capability audit are in progress. This is a timing observation, not proof of client receipt or a Wi-Fi startup test.

The publication-turn I/O observer is frozen as `native-tftp-publish-io-frozen-v1`, SHA-256 `b9daef75c39ba47f740eb3fafb7bacebe93e04a92a63d3d00ea6e6ecb84f708b`. The capability audit rejected 280 mutations; 15 diagnostic tests, 22 source tests, enabled/disabled AArch64 builds, source audits, boot layout, standard-frame LZ4 round trip and frozen source/input checks passed. It preserves early service and receive-yield but excludes extra dispatch via a compile-time guard. Four displayed I/O categories cover the whole router dispatch in which the first queued OACK becomes published, not only OACK operations; they use existing profiler counts and add no hardware reads or clock calls. No device attempt has yet occurred.

The publication-turn I/O observer image `b9daef75c39ba47f740eb3fafb7bacebe93e04a92a63d3d00ea6e6ecb84f708b` booted once on sunfish. IMG_9411 (SHA-256 `80f6af560e921bf9e24e3d625e8fcd0031b0fee3485f699df813e6ea950fe684`) shows Q6 ready, IPA flags 0x4f8 with four commits and one response, runtime error 0, matched first peer, first OACK queue 0xc3 = 195 ms, publication observation 0x2c2 = 706 ms, and RXDONE observation 0x7537 = 30007 ms. The dispatch in which the first OACK was observed published accumulated existing native-IO profiler category deltas: observe 0x207 = 519 ms, descriptors 0xd2 = 210 ms, read 0x46 = 70 ms, and other 0x2a4 = 676 ms (sum 1475 ms, subject to each conversion's rounding). These deltas span the entire router/owner dispatch, including possible work after publication and other records, and are elapsed host durations, not exclusive OACK time or peer timing. In particular, their sum can legitimately exceed the ~511 ms between the queue and publication observations. OTHER combines RX-length queries, writes, head/tail stores, barriers and notify calls. WRQ 20, RRQ 8, WLAN RRQ 0 and DATA 2 are aggregate counts; reject code 2/73 bytes does not identify the packet. No RRQ response, WLAN firmware/start, flash or persistent modem write was attempted.

Offline follow-up to IMG_9411: the 676 ms `IO OTHER` profiler total covers an entire router turn and combines RX-length checks, TX FIFO writes, TX-head stores, RX-tail stores, barriers and notifications. Each operation is already wrapped by the same native `run_profiled` clock pair; the opt-in `sunfish-tftp-publish-op-observer` classifies those existing elapsed samples into six additional cumulative counters and snapshots their deltas for the first owner turn in which the queued OACK becomes published. It makes no new clock calls or hardware accesses and does not change access validation, but recording counters adds software work. Like the preceding profiler, this is the entire dispatch (potentially including other records and work after publication), not per-OACK cost, exact head/doorbell time or peer latency. The extra dispatch experiment remains disabled. Host tests check category mapping, profiler enable/saturation and four- and six-category snapshot handling, and source checks verify existing clock calls are reused. The enabled target build, source checks, 23 source tests, 16 diagnostic tests and two detail-counter tests passed; boot-layout, disabled target build and capability checks are in progress. No device attempt has occurred.

The publication-operation observer is frozen as `native-tftp-publish-op-frozen-v1`, SHA-256 `6260c9f25f1c1f1db157ca0495d84e54a6e894de90ff86c49ddfcc7db6d6490e`. Both target builds, 16 diagnostic tests, 23 source tests, enabled/disabled detail-counter tests, source audits, boot-layout and standard-LZ4 checks passed; capability audit rejected all 280 mutations and the source/input closure was verified. It reuses existing per-operation clock samples, adds bounded counter bookkeeping, and reports six operation categories over the entire router turn in which the first OACK is observed published. The extra dispatch remains disabled. It has no device result yet and does not answer reads, serve WLAN firmware, start WLAN, flash or persist modem storage.

The publication-operation observer image `6260c9f25f1c1f1db157ca0495d84e54a6e894de90ff86c49ddfcc7db6d6490e` ran once on sunfish. IMG_9415 (SHA-256 `9fec9a5fda636b3bcf39b65ca6b77ad070709f3021e91d19843f7b3277e70e09`) reports Q6 ready, IPA flags 0x4f8, 4 commits, one response and runtime error 0. First-OACK queue/publication/RXDONE host observations are 0xc3/0x2c1/0x7502 = 195/705/29954 ms. For the whole router dispatch in which the OACK was observed published, existing native profiler samples classified OTHER into RX-length 0xba = 186 ms, FIFO write 0x1e = 30 ms, TX-head store 0x1d = 29 ms, RX-tail store 0x0a = 10 ms, barrier 0x185 = 389 ms, and notify 0x1d = 29 ms (individual rounded values sum to 673 ms). The categories include the entire checked wrapper for each operation, including gate/lock/lifecycle/directory work as applicable; in particular `IO BARR MS` is not the hardware memory barrier's isolated cost. They span the full dispatch, potentially other records and work after OACK publication, not just the first reply. Different runs cannot prove the entire 676 ms OTHER from IMG_9411 is identical work, although the figures are close. WRQ count 20 and aggregate DATA count 2 do not show a matching DATA transfer; no WLAN firmware/start, RRQ response, flash or persistent modem write was attempted.

Offline wrapper inspection: each successful native GLINK operation runs `retained_receive_domain()` before and after the operation. Each successful domain check validates the live faults, ASID, routes and final programmed registers, then compares 512 volatile words in each of eight translation-table pages with the canonical image. Thus one successful wrapped operation includes 8,192 table-word reads, plus the other checks. A barrier also enters the peer, lock, directory and cursor validation path. This count follows the source; it does not isolate their elapsed time on the Pixel or show why the completion is observed approximately 30 seconds later. The existing host fault-injection test was expanded to corrupt each of the eight table pages individually, in addition to its route, context and fault checks; all 109 tests in the native SMMU host harness pass. These checks serve an observable fail-closed purpose, and moving or eliminating them would change the interval in which corruption or hardware faults can be detected. No such optimization or new device boot is claimed. The next useful measurement must distinguish domain checks from peer/lock/directory work while preserving the original validation order and acknowledging instrumentation overhead.

The barrier-domain observer image `4bddb7cc2cf22fae890c174fdea45ecd1b55c1f449b7f2b280252f7fdbc615b1` ran once. IMG_9416 (SHA-256 `20ca7c4a9383f0c5a8a85b01f0e6fd371672600aeeadb150f78034d1ad6995db`) reports 0xad/0xad = 173/173 ms for the domain checks before/after the profiled barrier operations and 0x185 = 389 ms for the complete barrier wrappers. These totals cover all profiled barriers in the router turn where the first OACK was observed published, including work unrelated to the OACK, and have separately rounded millisecond values; the extra clock reads and bookkeeping may alter timing. Queue, publication and RXDONE observations were 0xc3/0x2c3/0x754c = 195/707/30028 ms. Q6 ready and runtime error zero were shown, along with aggregate TFTP DATA 2 and reject code 2. The measurement establishes that the two domain-check intervals account for much of the measured wrapper time in this run; it does not establish the cause of the approximately 30-second completion observation or native Wi-Fi operation. Offline source review found that rebuilding the canonical plan performs a nested duplicate-physical-page scan over up to 539 mappings on every domain check. A bounded incremental lookup may retain the checks and error order, but neither its performance nor equivalence has been proven on device.

The opt-in `sunfish-smmu-root-unique-fast-check` candidate replaces only the pure prefix search for duplicate physical pages in the root-table planner with a 1,024-slot incremental lookup; each page is still range-checked and checked for duplicates in the original order, and a full table falls back to the original prefix scan. The slot sentinel cannot be a valid input at that point because physical-page end overflow/range validation precedes insertion. All SMMU fault, route, register and volatile table-entry observations, lock boundaries and barriers remain in place. The lookup introduces an 8 KiB local array and changes CPU timing, and therefore still requires stack, build and device validation. The feature-enabled host tests pass for forced collisions, saturation, the retained-domain fault checks and receive-plan geometry; a separate comparison of 3,234 invalid mapping cases plus all eight valid table images produced the same result digest for enabled and disabled builds. On this host, 1,000 complete 539-page plan validations took about 3.9 ms enabled versus 37.3 ms disabled in one microbenchmark; this measures only planner CPU work, not Pixel hardware, the whole domain check or the completion delay. The candidate temporary image has been built, and the boot-layout and capability checks are in progress. No device attempt or WLAN operation is claimed.

The optimized-planner image is frozen as `native-tftp-root-unique-frozen-v1`, SHA-256 `afd8039f4f287f7e934889c3522c4eb6a33e5b412b62c948c5783ed317d9ef9d`. Both target builds, boot layout, standard LZ4 round trip, source/input closure, root and shared source audits, and the capability audit with 280 rejected mutations passed. AArch64 disassembly shows the planner's standalone stack frame as approximately 8.5 KiB versus 2.4 KiB in the previous image; this is not a complete call-chain stack bound. The existing 4 MiB runtime stack remains mapped. The image retains the earlier barrier-domain observer and repeats the prior RAM-only test; it does not answer read requests, serve WLAN firmware, start WLAN, flash or persist modem-storage writes. The existing experimental close-handoff compile guard was removed only in the isolated build copy, as in the earlier authorized tests; it remains in the workspace. No device attempt has been made.

The optimized-planner image was booted once. IMG_9420 (SHA-256 `575642908c85bf6e6ec5a9da2f09e2ac15b08884817f0bd16349a12cdaf3c393`) reports Q6 ready, IPA flags 0x4f8, four commits, one response, runtime error zero, and reject code 2. First matched OACK queue/publication/RXDONE observations were 0xab/0x26b/0x66cd = 171/619/26317 ms. In the whole router dispatch where publication was observed, barrier-domain pre/post totals were 0x95/0x95 = 149/149 ms and complete barrier-wrapper time was 0x156 = 342 ms. The preceding IMG_9416 run reported 195/707/30028 ms, 173/173 ms, and 389 ms respectively. This single comparison is consistent with a reduction in measured work, but differences in traffic, timing and instrumentation prevent a repeatability or causality claim; these are host observation timestamps, not the time the peer generated its response. Two aggregate TFTP DATA packets do not establish that the first transfer completed. No read reply, WLAN firmware/start, flash, or persistent modem storage write was attempted.

Loader/runtime cost review following IMG_9420: the pinned Linux `drivers/soc/qcom/mdt_loader.c` uses `memcpy` for file-backed segment bytes and `memset` for BSS; `drivers/remoteproc/qcom_q6v5_pas.c` maps the reserved region through `devm_ioremap_resource_wc`. The native `sunfish_mpss_linux_memory::Window` instead retains uncached scalar views and executes `dsb sy` before and after each byte/word access, with a separate complete target readback. This difference is a plausible copy cost, not a measured attribution; Linux's mapping/copy policy cannot simply be assumed valid for the current Microkit mapping. The pinned Linux GLINK SMEM FIFO functions also directly read/copy FIFO data and update cursors, unlike native per-operation retained-domain validation. The saved Linux reference associates its first OACK with GLINK TX/RXDONE about 80 microseconds apart and subsequent same-peer DATA, subject to the task/size/time correlation limitations recorded in that reference. This does not identify why native completion is observed tens of seconds later.

There is a second concrete cost in the pre-auth loader: `pixel_mpss_auth::service` calls the retained-domain check, and the loader's backend calls it before and after every successful chunk. Each successful check compares 4,096 volatile table words in addition to fault/route/register checks and CPU plan reconstruction. For the exact 29-segment private image, target staging plus full readback require 77,030 4-KiB steps, accounting for at least 946,544,640 table-word reads from those three checks alone. This is a source-derived operation count on a successful path, not a device duration measurement; it excludes metadata, secure-call checks and other work. Modem reset is not released until staging and verification finish, so this loading delay must not be equated with the later modem initialization watchdog or TFTP deadline.

The explicit opt-in `sunfish-mpss-target-chunk-64k` groups only target staging and verification into up to 65,536 bytes per step; metadata remains 4 KiB. This reduces target steps to 4,844 and the same three-check table-read count to 59,523,072. Scalar memory access sizes, their existing barriers, segment bounds, zero-fill, complete readback, PAS ordering, pre-AUTH cold-peer check, terminal failures and ownership retention are unchanged. This deliberately changes fault-detection and service spacing: up to 64 KiB can be accessed before the next surrounding check, and other startup servicing can wait for that chunk. It is not equivalent fault latency, a guaranteed speedup, or a runtime/TFTP fix. The display update interval scales with target budget to keep approximately the previous target-byte cadence without adding a chunk buffer or changing the loop's existing upper bound. The default 4-KiB mode remains available.

`platforms/sunfish/test-native-mpss-target-budget.py` verifies the pinned archive/firmware identities, compiles original Linux MDT/PAS C functions with recording stubs, and compares every target byte (including untouched gaps), compact metadata and secure-call trace for both native budgets in CPU RAM. Both modes passed 18 ordinary host tests and the full private-image comparison; both concrete loader/memory paths compiled as AArch64 no_std libraries with the installed toolchain's core built from source. Added tests cover a multi-chunk unaligned segment with a file/BSS boundary inside a word, untouched gaps, exact per-step budgets, guard loss before/after a chunk in both phases, and corruption at chunk/file/BSS edges that must prevent authentication and replay. Source hashes and results are retained under `.cache/sunfish-native-wifi/private/mpss-target-budget-check-v1/result.json`. The full candidate runtime image and its audits are still being prepared; no device attempt or WLAN operation has occurred.

The 64-KiB target-budget candidate is frozen as `native-mpss-target-64k-frozen-v1`, SHA-256 `355eee85d07cc6191e98cbd187feafa4d04ee893a071143071241148fec234a0`. The complete enabled and disabled seL4/AArch64 binaries built; 251 integrated handoff tests passed (the private-image test in that harness is ignored and was run separately in both loader modes), 24 runtime source tests and the runtime source audit passed, boot layout and standard-frame LZ4 round trip passed, and the capability audit rejected 280 mutations. The handoff harness allowed existing deprecated/dead-code warnings; no product code was changed to silence them. The source/input closure was verified against the prior frozen runtime and the recorded loader comparison. The prior isolated runtime's prototype-guard exception remains explicitly recorded, with the workspace compile guard intact; this build does not establish that prototype's production readiness. No runtime source changed beyond the pre-auth loader budget and progress display cadence.

After the user reported cold Fastboot ready, the host verified serial `02281FQC200058`, product `sunfish`, the frozen image and source/input/artifact hashes. The one-shot attempt was marked consumed before transmission. `fastboot boot` returned success (1.701 seconds sending and 4.757 seconds boot command, 6.488 seconds total); that is host transport acceptance, not MPSS readiness or speed evidence. The expected successful full loader count is 4,856 (`0x12f8`), comprising 4,844 target steps and 12 other steps, rather than 77,042 (`0x12cf2`). Actual Pixel loading time and the runtime outcome await a screen result. The phone was not flashed, and no retry was made.

IMG_9424 (SHA-256 `f1644a6fa400e548d50058d840a0da55a2080f0ece16439a67ca386e122c52e9`) shows that the 64 KiB candidate reached QRTR service runtime with Q6 ready, IPA flags 0x4f8, four commits, one response, and runtime error zero. Reject code 2 remains visible with 73 rejected bytes. First matched OACK queue/publication/RXDONE observations were 171/619/26299 ms, compared with 171/619/26317 ms in IMG_9420. The photographed runtime screen leaves pre-auth elapsed time and the final loader counter unmeasured. The two aggregate DATA packets leave first-transfer completion unverified.

Offline review after IMG_9424: no repeat boot is needed merely to estimate loader elapsed time. The successful runtime screen leaves that elapsed time unmeasured, while the runtime OACK completion observation remains approximately 26 seconds. These are distinct stages. The pinned Linux GLINK source (`2267e248204e7e051e0f7355298949a6440de579`, `drivers/rpmsg/qcom_glink_native.c`) and the saved `glink-tftp-reference-prepared-v1/run` trace were compared with the native request/reply path. The native RAM-only server calls `finish_tftp_request` after retaining the reply and before flushing it; that call makes the incoming request eligible for RX_DONE rather than waiting for the outgoing reply's completion. The Linux trace associates first-OACK TX and modem RX_DONE about 80 microseconds apart and shows the host's RX_DONE for the incoming WRQ afterward. Thus these observations do not support the suspected cyclic dependency between the two completions. Native one-shot receive-intent identifiers also participate in stale-handle protection; adopting Linux reusable intents requires reviewing that identity model, rather than simply changing a command number.

The next candidate, explicit opt-in `sunfish-glink-combined-cursor-sample`, targets a source-proven repeated cost. `Ring::sample` previously called native `rx_length` (a local controller-report lookup) and `descriptors` as separate operations. Both calls went through `Retained::run_profiled` and the checked native gate, each with independent full retained-domain validation before and after. The candidate combines the local lookup and the existing live descriptor read within one `run_profiled` operation, preserving their order. It removes two full retained-domain scans per `Ring::sample` (8,192 volatile table-word reads on the successful retained eight-page path), not the domain checks around the actual descriptor access. The initial attach path, separate lifecycle observation, peer snapshots, SMEM lock/directory checks, cursor/ownership/bounds validation, memory barriers, data readback, publication and quarantine remain. This is not an atomic peer snapshot or a cached authorization result. If the sampled FIFO length has changed, a read-only descriptor observation can now occur before `Ring` rejects that length; no payload or cursor write is authorized by that rejected sample. The profiler attributes the combined operation to descriptor time, so its length and descriptor categories are not individually comparable with the prior run.

Host verification for this candidate: the feature-off/on FIFO suites passed 117/118 tests and the real native transport/controller suites passed 397/400 tests, with the separate private-firmware loader test ignored in each transport run. New cases cover a failed combined read, late RX discovery with the original length-before-descriptor ordering, changed peer lifecycle/generation/power, fatal state, allocation/cursor/reserved-word corruption, and independent corruption of each of eight retained table pages. The page tests require terminal quarantine even after restoring the altered word and require subsequent writes to remain blocked. The existing post-install suite passed 113 tests and rejected all five mutations that remove required domain/quarantine behavior. Existing runtime source checks passed 24 tests and the source audit passed. These are CPU/RAM results, not a measured Pixel improvement. The complete feature-on and feature-off target binaries built successfully; image audits and the full experimental runtime test configuration are being finalized. No device action has occurred for this candidate.

The combined-cursor candidate is frozen as `native-glink-cursor-sample-frozen-v1`, SHA-256 `3804864052b23afa78d2ebb752107d18dac4617a71a4e031117ae40b897d2452`. The complete experimental runtime host configuration passed 448 tests (one separate private-firmware loader test ignored). Its expanded test harness removes only duplicate module declarations from the included server harness; original modules and test bodies are retained, and the exact generated harness and compiler command are saved. Both full target builds, boot layout, standard LZ4 round trip, source/input/artifact closure, runtime source checks and the capability audit with 280 rejected mutations passed. The prior isolated-build close-handoff prototype exception remains explicit, with the workspace compile guard intact. No device timing improvement is claimed by these checks.

After the user reported cold Fastboot ready, serial `02281FQC200058` and product `sunfish` were verified, as were all frozen source/input/artifact hashes and the unused attempt. The one-shot was marked consumed before transmission. One `fastboot boot` returned success: sending 1.716 seconds, boot command 4.443 seconds, host total 6.174 seconds. These measure host acceptance only. No flash or retry occurred. The Pixel screen result is pending; compare queue/publication/completion observations and runtime status against IMG_9424 rather than inferring WLAN progress from host boot acceptance.

IMG_9431 (SHA-256 `f1a8f875f07fbdbafd709948d0c66458fa72f28286da6e77d09e9452b768cc45`) shows the combined-cursor image reaching QRTR service runtime: Q6 ready 1, IPA flags 0x4f8, four commits, one response, runtime error zero, GLINK ACKs 8, reject code 2 with 73 rejected bytes, WRQ 14 and aggregate DATA 2. First matched OACK queue/publication/RXDONE ages are 0x8d/0x212/0x59be = 141/530/22974 ms, versus 171/619/26299 ms in IMG_9424. The queue-to-publication interval is 389 ms versus 448 ms. In the publication turn, domain pre/post totals are 150/150 ms, whole barrier-wrapper 344 ms, head/tail/notify 26/8/26 ms. The barrier checks themselves remain similar to the prior 149/149/342 ms. This single comparison is consistent with lower surrounding runtime overhead; traffic and timing variation prevent attributing the entire difference to the change. The long completion observation remains, aggregate DATA does not prove first-transfer completion, and no WLAN startup occurred. Awaiting a settled-screen follow-up; no second attempt was made.

The user reports IMG_9431 counters unchanged after waiting. A fresh execution of `test-tftp-reference-oack.py` confirms the current native 39-byte OACK exactly matches the pinned tqftpserv C output for the captured options; this checks encoding, not on-device delivery or client acceptance. Review confirms the measured barrier domain totals remain approximately 300 ms combined in the publication turn. `Ring::write` still brackets its individual samples, payload copy/readback, barriers, head publication and notification as separate checked backend calls. Grouping a bounded operation is a possible next overhead reduction, but its fault/ownership boundaries must be reviewed before implementation. Separately, the current timing display hides the existing `completed_count` and total-RAM-byte fields. Aggregate DATA=2 and a delayed first-OACK completion cannot decide whether a later peer completed a server-check write. The next review must retain that distinction rather than treating the first transfer's timing as proof of the remaining protocol blocker. No retry or new device action occurred during this review.

Follow-up to the unchanged IMG_9431: the next explicit opt-in, `sunfish-glink-barrier-observe-pair`, groups only an existing barrier followed immediately by an existing status observation. `Ring::sample` uses this for its final acquire barrier/status pair, and `Ring::write` uses it for the release barrier/status pair immediately before publishing the TX head. `Io` defaults to the same separate calls in the same order. The retained native override calls the unchanged controller barrier and poll methods within one full retained-domain pre/post pair. It removes two domain validations (8,192 volatile table-word reads on the successful retained eight-page path) at each paired call, but does not remove the barrier, lifecycle checks, peer snapshots, SMEM lock/directory checks, cursor checks, data readback or failure quarantine. No FIFO payload or cursor publication occurs inside this group. Controller polling can still negotiate/acknowledge SMP state as before; it is not described as an entirely side-effect-free read. A persistent domain change during the group is detected at its end rather than at the former intermediate boundary, and an existing SMP acknowledgement can precede that detection. No checked authorization is cached across calls. The whole group is attributed to barrier time by the profiler, so barrier/observation categories are not directly comparable individually.

`sunfish-tftp-outcome-summary` is a separate explicit display option. The last success overlay now shows WRITES COMPLETE, RAM BYTES, FIRST COMPLETE and ACTIVE SESSIONS while preserving first-OACK queue/publication/completion ages, aggregate DATA, IPA status and errors. These are the existing RAM-only server counters, with no additional packet accesses or replies. WRITES COMPLETE means the restricted local server accepted the final data block into RAM, not that its acknowledgement was delivered to the peer. FIRST COMPLETE identifies the first allocated transfer, not every later client. Error displays are not overwritten by this overlay. The test still does not answer reads or serve WLAN firmware.

The paired-call host tests cover the original barrier-before-observation order, either-half failures, post-barrier generation/identity/readiness/fatal changes, retained page corruption, peer/lifecycle rejection, and corruption injected from inside the native operation. They require terminal quarantine after restoring the bad inputs and no subsequent FIFO publication. The full runtime configuration also exercises the instrumented pre/post wrapper. Host feature-off/on FIFO groups passed 118/120 and native transport/controller groups passed 400/403, with one separately tested private-image loader test ignored in each transport suite. The full experimental runtime configuration passed 451 tests, one ignored. The existing post-install and runtime-source audits are being refreshed against the final source; no device action has occurred for this candidate.


### 2026-11-08 — barrier/status pair candidate frozen and sent once

Frozen artifact: `.cache/sunfish-native-wifi/private/native-glink-barrier-pair-frozen-v1/manifest.json`; image SHA-256 `8272a62b90709bd51970b6fc4012000312aa128013385ba5d9de7367167e7f84`. Both target configurations built. FIFO off/on: 118/120 passed; transport off/on: 400/403 passed (one private-firmware case ignored each); full runtime configuration: 451 passed, one ignored. Source tests: 25 passed. Postinstall: 113 passed and five removed-check mutations rejected. Capability audit: 280 mutations rejected. Boot-layout and standard-LZ4 round trip passed. Final-source ELF matched the packed/audited ELF exactly. Frozen artifacts and workspace source hashes were reverified immediately before boot.

User confirmed cold Fastboot. Serial `02281FQC200058`, product `sunfish` verified. Marked the attempt consumed before invoking exactly one `fastboot boot`; host reported sending and booting OKAY, total 6.351 s. No flash or retry. Host acceptance is not an on-device result. Await settled photo: WRITES COMPLETE, RAM BYTES, FIRST COMPLETE, ACTIVE SESSIONS, QUEUE/PUBLISH/RXDONE AGE MS, TFTP DATA RX, and errors. RAM completion does not itself prove final ACK delivery.

IMG_9435 (SHA-256 `135d8d341dc8fc3fb2448663c0c1402c74403e5d2e2384267cdb205f96d756a0`) shows Q6 ready, IPA flags 0x4f8, commits 4, response 1 and runtime error zero. The newly exposed RAM counters report one locally completed write, five bytes retained, first slot incomplete, seven allocated sessions and two aggregate DATA packets. This establishes local acceptance of a final server-check data block for a later slot, not its final ACK delivery; prior images did not expose these counters, so it cannot establish that this candidate newly enabled completion. Reject code 2 with 73 bytes remains. First-OACK queue/publication/RXDONE ages are 96/520/18784 ms versus 141/530/22974 ms in IMG_9431; queue-to-publication is 424 ms versus 389 ms. The completion observation is earlier in this one run, but the queue-to-publication interval is not reduced. Whole paired barrier category 320 ms is not directly comparable with old separate barrier/observation categories; notify is 21 ms. No runtime stop is reported, and no WLAN started. Await settled follow-up before restarting; do not infer liveness or first-peer acceptance solely from runtime error zero.

User confirms IMG_9435 counters unchanged after waiting. No runtime error was displayed; no further screen-visible progress established. Source review: completed_count counts Transfer.complete locally; active_count counts every peer-bearing slot, including completed/abandoned reserved slots, not necessarily seven in-flight writes. Per-peer owner receives DATA, retains reply bytes, finishes the receive and queues the reply. GLINK TFTP completion counters are aggregate, and the existing endpoint timing trace follows the first OACK. Neither associates the final ACK with the completed five-byte transfer. Next offline review must correlate that transfer and final ACK before assuming server-check failure or treating the old first-OACK completion as the blocker. No new device attempt.


### 2026-11-08 — completed-write final-ACK observation

User authorized the next offline step after the unchanged IMG_9435. The explicit `sunfish-tftp-final-ack-observer` option retains metadata only for the first successfully accepted final server-check DATA response, as indicated by the existing restricted RAM server's Reply.completed. It captures both QRTR endpoints and the ACK's block number before reply queueing. The session records queue acceptance only for that exact route, opcode and block, tags that particular transmit object, and records publication and the validated matching GLINK RX_DONE. The tag is cleared at completion and by Transmit::empty, so a reused transmit slot cannot advance the old observation. Logical transport reset clears the trace. The original first-OACK observer remains unchanged.

Flags: 0=no completed-write ACK captured; 0x1=accepted final DATA/ACK retained; 0x3=ACK admitted to the transmit queue; 0x7=started; 0xf=fully published; 0x1f=matching GLINK completion. These are cumulative. GLINK completion is transport evidence, not a new acknowledgement from the remote TFTP application. The four-byte final ACK is expected to publish in one fragment, so 0x7 may not be visible. The screen refresh compares this trace independently, and its final success overlay shows ACK flags, peer/local node and port, block and remote intent alongside completed writes, retained RAM bytes, allocated session count and aggregate DATA. The final overlay replaces first-OACK timing rows to avoid mixing different transfers; error screens retain priority. No extra clock, MMIO, packet read or hardware operation is added by the observer. Metadata comparisons and potentially added redraws can affect timing.

Packet content, validation, queue scheduling, timeouts and cleanup behavior are unchanged. Earlier experimental transport and RAM-only profile remain; no read replies, firmware service, Wi-Fi or flash are enabled. Tests cover a real owner WRQ/OACK/DATA/ACK exchange, distinguish local completion from queue/publication/GLINK completion, ignore unrelated endpoints/blocks/opcodes and earlier/later peer completions, preserve the trace after slot reuse, and clear it at logical generation reset. A would-block publication cannot advance its published flag. The integrated actual-feature host configuration passed 451 tests, one private-image case ignored; session feature-off/on groups passed 142/143; 26 runtime source checks passed. Full enabled target build and source audit passed; disabled target build, postinstall and final capability/layout audits are being completed. No new device action has occurred.

Final-ACK observer image frozen as `native-tftp-final-ack-frozen-v1`, SHA-256 `cdb1cb713c03865c423ae8896e0f7718d7fec263b97b2db88376b6862bfac512`. Enabled and disabled full target builds passed; source audit and 26 source tests passed; session feature-off/on 142/143 passed; actual runtime features 451 passed, one private-image test ignored; postinstall 113 passed and five check-removal mutations rejected. Capability audit rejected 280 mutations; boot layout and standard-LZ4 exact round trip passed. Isolated source matched workspace exactly except the previously documented prototype compile-guard removal in the build copy; the workspace guard remains.

User confirmed cold Fastboot while the final audit ran. After it passed, serial `02281FQC200058` and product `sunfish` were verified, followed by frozen artifact/source/build-input/test-input hash verification. Attempt marked consumed before exactly one temporary `fastboot boot`; send and boot commands returned OKAY, total host time 6.494 seconds. No flash or retry. Host acceptance is not an on-device result. Await settled screen with WRITES COMPLETE, RAM BYTES, FINAL ACK FLAGS, ACK endpoint/block/intent fields and any runtime error.

IMG_9438 (SHA-256 `e4eb10514b4090b3a0b38e5fee3d7a9e69ef1f5fba46cf5d67335c028d291802`) reaches the final-ACK target: WRITES COMPLETE 1, RAM BYTES 5, FINAL ACK FLAGS 0x1f. The tracked final ACK is block 1, local QRTR 1:0x5007 to peer 0:0x51, remote intent 0 (a valid identifier, not an unavailable marker). This trace records acceptance of final DATA, queueing of the matching ACK, successful full publication and the validated matching GLINK RX_DONE. Q6 ready 1, IPA flags 0x4f8, commits 4, responses 1, runtime error 0. WRQ 14, aggregate DATA 2, seven allocated/reserved session slots, GLINK ACKs 7. First rejection remains UnexpectedRequest (2), 73 bytes. This screen does not identify that rejected packet.

The final acknowledgement for this completed five-byte server-check write is no longer an unproven transport stage. This is not proof that all clients completed or that WLAN started, and it does not separately prove the remote TFTP application accepted the ACK. The prior first-OACK delay describes a different earlier transfer and must not be treated as evidence this final ACK is stuck. Next offline step: follow the already captured read requests (earlier evidence includes `/readwrite/mcfg.tmp`) against the Linux reference and the native restricted profile, which deliberately does not answer reads. Do not infer that the current 73-byte rejection is that exact RRQ without correlation. No further boot or scope expansion authorized by this photo alone.


### 2026-11-08 — narrow missing-temporary-file reply candidate

After IMG_9438 established final-ACK GLINK completion for the completed five-byte write, the user authorized offline continuation. Reviewed the pinned tqftpserv handle_rrq/tftp_send_error and translate_open source plus saved reference captures. Linux uses a new connected QRTR transfer endpoint and sends the 19-byte ERROR 1 `file not found` packet when a read open fails, before any OACK. Earlier native evidence identifies `/readwrite/mcfg.tmp`; earlier Linux journal evidence identifies failed opens for `/readwrite/ota_firewall/ruleset` and `/readwrite/modem_efs`. The later endpoint metadata trace contains opcodes, sizes and routes, not filenames, so it does not establish an mcfg path. The latest 73-byte rejection is still not identified solely by its size.

The optional `sunfish-tftp-missing-temp-replies` candidate answers only those three exact names, which are absent in the restricted RAM namespace. It validates node/port, opcode, bounded NUL-terminated octet mode and printable nonempty option pairs, then returns ERROR 1 without any file contents. It does not enable general filesystem access, firmware service or WLAN. Missing reads share the existing 32 non-reused transfer slots; each peer retains a distinct local transfer port. Same-peer/same-name retries use the reserved port; mixed read/write reuse, other names, malformed reads and DATA to a read slot are rejected. The existing server-check WRQ/DATA behavior remains. Missing-read slots cannot enter the empty-write abandonment path. This deliberately preserves close guards; pending read-reply close behavior remains a possible next stop.

The first missing reply is traced independently of the final write ACK: MISSING KIND 1=mcfg.tmp, 2=ota_firewall/ruleset, 3=modem_efs; MISS RSP FLAGS 1=accepted read/retained reply, 2=queued, 4=started, 8=fully published, 16=matching GLINK RX_DONE (cumulative target 0x1f). Trace follows the exact reply bytes, route and transmit lifetime, survives unrelated completions and slot reuse, and resets with transport generation. GLINK completion is not remote application acceptance. The final-ACK and RAM outcome rows remain visible. Existing errors retain display priority. No new hardware reads or timing calls; redraws can affect timing.

Offline results: pinned Linux C error encoder and native restricted reply match byte for byte. Session feature-off/on 143/144 tests passed; restricted-server feature-off/on 9/11 passed, including malformed packets, output/capacity bounds, peer isolation and unchanged write behavior. Actual runtime configuration passed 451 tests, one private-image case ignored, including write completion followed by a missing-read ERROR through validated GLINK completion. Runtime source tests 27 passed; source audit passed. Postinstall 113 tests passed and five removed-check mutations rejected. Both target builds passed; final capability and layout validation is in progress. No new phone action has occurred.

Candidate frozen as `native-tftp-missing-temp-frozen-v1`, SHA-256 `c63fac2811f8b1faedb4fbe94dd651250207179138e38979b217dc7c9c8da4f8`. Enabled and disabled builds, source audit, 27 source tests, Linux byte oracle, session/model suites and actual runtime tests passed. Capability audit rejected all 280 mutations; boot layout and standard LZ4 round trip passed. The isolated 360 Rust source files match the workspace except the previously documented build-only close-handoff compile-guard removal. Frozen artifacts rehashed successfully. No device boot attempted; phone should remain in LineageOS until a fresh cold-Fastboot readiness confirmation for this scope.

User confirmed fresh cold Fastboot for the missing-file reply scope. Serial 02281FQC200058 and product sunfish verified; all frozen artifact, workspace source and input hashes matched. Attempt marked consumed before exactly one temporary fastboot boot. Host exit code 0. No flash or retry. Await device evidence for MISSING KIND, MISS RSP FLAGS, RRQ/WLAN counters and any error; host acceptance does not establish device success.

IMG_9440 (SHA-256 384e91dada2a9c52e1bc811fd5e1f4b578b6458ad4116e7fadf212b8166c0414) shows MISSING KIND 1 (exact /readwrite/mcfg.tmp), MISSING RRQ 3, MISS RSP FLAGS 0x1f, peer 0:0x53, local 1:0x5008 and remote intent 0. The first missing-file ERROR was accepted, queued, fully published and completed by validated matching GLINK RX_DONE. Three aggregate RRQ events were accepted by the narrow handler; only first reply completion is traced, so this is not evidence for all three reply completions or three unique paths. WRITES COMPLETE 1, RAM BYTES 5 and FINAL ACK FLAGS 0x1f persist. Q6 ready 1, IPA flags 0x4f8, commits 4, response 1, runtime error 0. WRQ 0x14, RRQ 3, DATA 2, WLAN RRQ 0. First rejected packet is now 0x41 bytes with code 2; its identity is not established by length. No WLAN startup. Await a settled comparison before deciding next step.

User confirms IMG_9440 counters unchanged after a minute. No further progress visible. Three incoming RRQ events and three accepted missing-read events provide no count-level evidence of an unanswered read in this run; first reply alone is completion-traced. The 65-byte first rejection is not established as the blocker. Offline source review confirms the runtime does not call the existing SERVREG_NOTIF WLAN state query or restart APIs. Existing code supports a bounded exact-instance 180/version 1 WLAN-PD state query and separately gated listener/restart. Current screen also hides the already available WLFW server inventory count. Next recommended boundary: display exact SERVREG_NOTIF/WLFW discovery and issue one read-only WLAN-PD state query through the existing owner, without restart or firmware transfer, to distinguish an unstarted domain from missing discovery/transport. Do not infer that Linux restart sequence or this read query alone will enable Wi-Fi. User advised return to LineageOS; no new image built or device action.

### 2026-11-08: one-shot WLAN-PD state query candidate

User approved the read-only query boundary after the settled IMG_9440 result. The optional `sunfish-wlan-state-query` feature reserves local QRTR client port 0x4005 before control starts, reuses the existing validated SERVREG_NOTIF client, and attempts QUERY_STATE (0x21) for exactly `msm/modem/wlan_pd` once after current IPA protocol completion and exact instance 180/version 1 discovery. The attempt latch is set before queueing; a queue or scheduling failure cannot cause an automatic second request. The candidate does not call listener registration, PD restart or WLFW client startup. Existing RAM-only server-check writes and three exact missing-file ERROR replies remain enabled. No read contents, firmware, flash or persistent storage writes are added.

The screen exposes software-owned NOTIF (0x42/0xb401) and WLFW (0x45/1) inventory for peer node 0, plus query phase and raw response. QUERY PHASE 1 waits for discovery; 2 waits for IPA; 3 means attempted/pending (not proof of publication); 4 means a validated successful response with a state; 5 means QMI failure; 6 means successful response without state. QUERY RESULT/ERROR remain FFFFFFFF until a response. Successful known raw states are DOWN 0x0fffffff, UP 0x1fffffff, EARLY_DOWN 0x2fffffff and UNINIT 0x7fffffff. Unknown values remain raw. An absent response is never interpreted as DOWN. This candidate adds no timer or retry, so pending can remain pending. It cannot establish that restarting WLAN is necessary or that a state query itself starts the radio.

New owner-level tests run the actual QRTR/GLINK implementation: discovery and IPA gating; exact query payload and route; one-shot behavior; successful, missing-state and QMI-failure responses; unknown-state preservation; and schedule failure consuming the attempt. Existing source checks enforce activation ordering, separate port, display refresh and absence of startup calls. Feature-on host runtime passed 454 tests (one private-input case ignored); disabled comparison and final audits are being collected. Phone remains outside this candidate; no device attempt yet.

Feature-disabled runtime comparison passed 451 tests, one ignored; enabled passed 454, one ignored. Reusable runner records feature closure and source hashes for both. Runtime source tests 28 passed and source audit passed; postinstall 113 tests passed with all five guard-removal mutations rejected. Both enabled/disabled target builds and boot layout passed. Parent source changes are limited to Cargo feature declaration, owner helper, runtime wiring/display, owner tests and source regression tests; parent build/test inputs are unchanged. No device action.

Candidate frozen as `native-wlan-state-query-frozen-v1`, SHA-256 `6c288576283ebfbe75a4eaa7b5727ae5a2979be0a6df45649492a36def993e16`. Capability audit rejected all 280 mutations; standard LZ4 round trip and layout passed. All frozen artifacts and recorded sources rehashed successfully. Attempt remains unconsumed. Await fresh cold-Fastboot readiness for this scope; no phone operation has been performed.

User confirmed fresh cold Fastboot for the WLAN-state query. Verified serial/product sunfish and frozen artifact/source/input hashes. Marked attempt consumed before exactly one temporary fastboot boot; host exit code 0. No flash or retry. Await device query state and inventory evidence; host acceptance does not establish query success.

IMG_9441 (SHA-256 661ac8025ed371088af4839a1b36414cc89eda618f86cce40cb3295742114bbb) confirms QUERY PHASE 4, QUERY RESULT/ERROR 0/0 and PD STATE RAW 0x7fffffff = UNINIT. Exact NOTIF inventory is one server at node 0 port 3; matching WLFW inventory is empty (node/port unavailable). Q6 ready 1, IPA flags 0x4f8, commits 4, response 1, runtime error 0; final write ACK and first missing-file reply flags both 0x1f; one completed RAM write. This proves the read-only query returned a validated state, not a timeout. No listener registration, PD restart or WLAN startup ran. Next offline boundary is review of the existing listener/guarded restart path against the pinned Linux startup sequence; this result alone does not prove restart will produce WLAN firmware requests. Ask return to LineageOS; no retry of the consumed image.

### 2026-11-08: listener-only follow-up after confirmed UNINIT

The user requested continuation. Pinned Linux source review changes the earlier suggested restart direction: mainline `pdr_notifier_work` registers a listener and reports the current state, while Lineage ICNSS setup obtains service location and registers notifications. The ICNSS `service_notif_pd_restart` call belongs to recovery, not its initial setup. No public-source evidence here establishes RESTART_PD as the cold-start trigger for this UNINIT domain. Do not call it simply because the state query returned UNINIT. The prepared candidate therefore adds listener registration only; a restart is explicitly excluded.

References were read directly from mainline commit fd78d17954a86ea6ceb0c535e618ea129e09f8dd (`drivers/soc/qcom/pdr_interface.c`) and Lineage commit 28f9290ae067d3e7857126eb6312720dbe9c7c80 (`drivers/soc/qcom/icnss.c`, `service-notifier.c`). The targeted reference audit records the blob hashes and checks setup/recovery separation. Mainline sends SET_ACK with the indication transaction then cancels its local transaction without waiting; Lineage waits for a matching response and checks its result. Existing native behavior retains the latter response check and serializes indications. It can remain pending or stop on a second update while an ACK is unresolved; this candidate does not relax that behavior or add a timer/retry.

Opt-in `sunfish-wlan-state-listener` depends on the query feature. It attempts one listener registration on the same exact domain and port only after a current-boot successful UNINIT query and current IPA completion. The attempt is consumed before queueing/scheduling. Runtime uses the existing owner, response validator and matching state-indication ACK path, with no raw transport or new register access. The query continues to run once. No restart, WLFW client, read contents, WLAN firmware, association, flash or persistent write is added. Existing RAM write and exact missing-file reply scope is inherited.

The screen shows QUERY PHASE/PD QUERY STATE, LISTEN PHASE/STATE/RESULT/ERROR, UPDATE TXN/STATE, ACK RESULT/ERROR and current WLFW inventory. Listener phase 1 waits for the prerequisite (including IPA), 2 means the query did not return successful UNINIT, 3 means registration attempted/pending, 4 is a successful response with state, 5 is a QMI failure, 6 is success without state. FFFFFFFF means unavailable; no-response is never interpreted as DOWN. Listener state and later update state are separate so an old registration snapshot is not presented as a live current value. No notification is fabricated when there has been none. Errors retain priority over the overlay.

Actual runtime tests pass with feature enabled (457, one ignored) and disabled (454, one ignored). Tests cover exact listener request bytes and route, one registration, matching update and SET_ACK response without restart, failed/missing/non-UNINIT query gating, listener failure/missing state, and non-retry after scheduling failure. Source tests 29 and runtime source audit pass. Both target builds and boot-layout check passed; final capability audit is running. No new phone action; leave the device in LineageOS until a frozen candidate and fresh readiness confirmation.

Listener-only candidate frozen as `native-wlan-state-listener-frozen-v1`, SHA-256 `6ca352ef9457a417460333670a894e6a19b2687b0773f1a0dca4cc7f424bb5ae`. All 280 capability mutations rejected, boot-layout/LZ4 round trip passed, isolated Rust matches workspace except the documented compile-guard removal, and artifacts/sources/inputs rehashed successfully. Prior postinstall proof reused only after its exact input hashes were verified unchanged (113 tests, five rejected mutations). Reference blobs rechecked against pinned git objects. Attempt unconsumed; no new device action. Await fresh cold Fastboot for listener-only scope.

2026-11-09: User confirmed fresh cold Fastboot for the listener-only candidate, then reconnected USB after initial non-enumeration. Verified serial 02281FQC200058/product sunfish and all frozen artifact/source/input hashes. Attempt consumed before exactly one temporary fastboot boot; host exit code 0. No PD restart, flash or retry. Await listener response/state-update evidence; host acceptance is not device success.

IMG_9442 (SHA-256 523a618b63d3d04f87d2030284579dbd3082602cbc4d002ec6d83b6be963e654) confirms QUERY PHASE 4 and LISTEN PHASE 4, LISTEN RESULT/ERROR 0/0, LISTEN STATE and PD QUERY STATE both 0x7fffffff = UNINIT. Listener registration succeeded, independently confirming the query state at registration. UPDATE TXN/STATE and ACK RESULT/ERROR are unavailable (FFFFFFFF): no recorded state notification/ACK, not a failed ACK. WLFW SERVERS 0; Q6 ready 1, IPA 0x4f8/4 commits/1 response, runtime error 0. No PD restart or WLAN startup. Await one settled comparison before concluding no visible transition.

User confirms IMG_9442 fields unchanged after about one minute. Successful query and listener registration did not yield a visible state transition or WLFW advertisement. This does not identify the cold-start trigger, prove a missing restart requirement, or establish runtime liveness. The consumed image should not be retried. Next offline investigation: trace the actual WLAN-domain launch prerequisites in the working Linux boot and compare against native host-service behavior; do not substitute the recovery restart RPC without evidence. Requested return to LineageOS to clear temporary state.

### 2026-11-09: offline cold-start review after settled UNINIT listener

User is returning the phone to LineageOS. No new image was booted or authorized by that status reply. Reviewed the actual pinned Mobian integration script (commit 789679f25f859bd741d690fe221d1cc68a23444e, script SHA-256 9dd5fc8ae9302c3275b23ccef23112ee2627239b133802a3583c467aeeccc8b2), the reference start wrapper, saved cold-Linux journal, native service map and restricted TFTP handler. Machine-readable input hashes and selected journal lines are in `.cache/sunfish-native-wifi/private/wlan-cold-start-review-20261109.json`.

The integration script starts MPSS if needed, waits for WLFW discovery, and only then loads ath10k_snoc. It has no WLAN-PD restart step. The local wrapper starts RAM-backed RMTFS/TFTP and prepares IPA/kernel PD mapper before that script. In the saved successful run, the first WLAN-image read is at 178.730030 seconds (vendor fallback), with canonical `/readonly/firmware/image/wlanmdsp.mbn` at 178.734089; ath10k firmware setup follows at 184.770318. The mcfg.tmp failed read is later, at 187.027163. These observations do not identify the modem's internal trigger, but rule out treating ath10k loading or the later mcfg request as the demonstrated initial WLAN-image trigger in this run.

SM7150's reviewed mainline map uses mpss_root_pd_gps, not the PDR-enabled alternative, and includes mpss_wlan_pd with kernel/elf_loader and wlan/fw. The native modem mappings contain these services too; do not invent a missing tms/pdr_enabled requirement. This static comparison is not proof that every native registry response was consumed in time.

The unresolved 65-byte native TFTP rejection remains unidentified. `ServerCheck::receive` preserves only `(Error, packet.len())` for the first rejection. UnexpectedRequest can mean a nonmatching request or a peer/session-state conflict; length alone cannot distinguish them. The next useful observational change is a bounded first-rejection metadata snapshot (route, opcode, path/mode/options boundaries and truncation/parse status), without copying DATA payloads, expanding file handling or sending new replies. This can distinguish an unsupported request from a supported request rejected due to state. It does not assume that the rejection causes UNINIT. No new runtime change, build, PD restart, read service, firmware transfer or phone operation was performed in this offline review.

### 2026-11-09: first rejected TFTP request capture

The optional `sunfish-tftp-rejection-capture` records the first rejection's source and destination node/port, opcode, full packet length, and up to 60 bytes for RRQ/WRQ only. Other opcodes retain no packet bytes. The first capture cannot be overwritten; existing error handling, responses and accepted request scope are unchanged. On-screen captured length must be compared to full length: request bytes can be truncated, and existing runtime errors retain display priority. This diagnostic does not establish that the rejected request causes WLAN UNINIT.

Focused tests passed with the feature enabled (7) and disabled (6), including first-capture retention, no DATA capture and truncation. Integrated final-input runtime suite passed 457 with one ignored; runtime source checks 29, source audit, enabled and disabled target builds, boot layout, standard LZ4 round trip and all 280 capability mutations passed. Isolated build source matches workspace except the documented build-only compile-guard removal. Frozen image `native-tftp-rejection-capture-frozen-v1`, SHA-256 `688b108aea714174c8de959105216be1d0c7aafda0150db110307f4585b24bcb`, has not been booted and its attempt is unconsumed. No new device action, firmware service, PD restart or Wi-Fi start.

IMG_9443 (SHA-256 `9a3847c7833cbd636c611a3021a153b8d7254013cf36758cc66bc30cc504a9fc`) identifies the first rejection as a TFTP WRQ (opcode 2), length 0x41 = 65 bytes, from node 0 port 0x54 to node 1 port 0x5000. Captured 0x3c = 60 bytes decode as `/readwrite/mcfg.tmp`, mode `octet`, `blksize` 7680, `timeoutms` 1000, followed by `wsiz` and five uncaptured bytes. The full name and first two options are visible; do not infer the trailing five bytes. The restricted native write handler accepts only server_check.txt, so this path is outside its write scope. The Linux reference did perform mcfg.tmp writes, but only after the WLAN-image requests in that recorded boot; this does not establish that servicing this write will start WLAN. The image captured one rejection, not all requests or their causal ordering. No firmware was served, no WLAN startup attempted. Boot attempt consumed; request return to LineageOS.

### 2026-11-09: mcfg write follow-up reference review

Pinned tqftpserv `handle_wrq` calls `translate_open(filename, O_WRONLY | O_CREAT)`, then negotiates options and creates a per-peer QRTR transfer; `translate_readwrite` maps the name under its temporary directory. A file may thus be read later in the same boot. The current native model instead always returns file-not-found for mcfg.tmp and accepts no mcfg write. Simply allowing its WRQ while leaving the missing-read response unconditional would falsely report a written file absent. Also IMG_9443 captured 60 of 65 bytes: the visible trailing `wsiz` does not prove the final option value. The exact candidate bytes ending `wsize\0 10\0` would have length 65 and match the captured prefix, but that suffix remains a hypothesis, not device evidence.

A safe next implementation needs a bounded RAM file lifecycle shared across transfers; distinct mcfg and server-check names and peers; exact validated option parsing rather than guessing the five unseen bytes; no file-not-found response once mcfg exists; bounded DATA and transfer sizes, ordering and retransmission checks; and explicit behavior for reads after creation. Host tests must cover read-before-create, successful write and subsequent read, interleaving peers, retransmissions, malformed options and capacity. A mere OACK-only change would not establish working file semantics. Determine the existing transport block-size limit before agreeing to any negotiated size. No new image has been built or device action attempted.

Offline candidate core `rust/src/qcom_mcfg_ram.rs` provides a bounded 8192-byte RAM store with one peer-owned staged writer, ordered blocks, exact retry validation, atomic completed-file publication, abort and offset reads. Three standalone host tests cover no-file, creation/read, peer isolation, duplicate/changed retry, ordering, capacity, abort, and replacement visibility. This is an isolated storage primitive only: it is not linked to the runtime, does not implement option negotiation, transfer ACK/OACK, reader sessions or close/retry handling, and has not been tested on device. Its atomic visibility intentionally differs from tqftpserv's immediate O_CREAT write behavior and needs protocol-level assessment before integration. Existing native QRTR datagram max is 4096 including the header; the 7680 requested block cannot simply be echoed as accepted for this implementation. Reference tqftpserv echoes its parsed block size and reads writes in a 516-byte buffer, so a smaller negotiated size is a behavioral change requiring validation. No new image is ready or booted.

The offline `qcom_mcfg_ram.rs` write-transfer candidate now validates the complete request and offers block size 512/window 1 rather than echoing an unsupported 7680-byte block. Six standalone tests pass, including OACK, DATA, final ACK, duplicate final DATA, wrong-peer and too-small-output handling. This is still isolated: `ServerCheck` and `qcom_modem_owner` do not call it, and the read-session, pending-reply/close and transport behavior are not yet integrated. The negotiated change and the atomic staging semantics are not established as accepted by the modem; no image has been built.

The RAM-backed mcfg candidate is now connected to the native TFTP owner. It validates the complete WRQ, negotiates 512-byte blocks with one outstanding block, commits a completed file up to 8192 bytes, and serves subsequent reads from a per-peer snapshot. The final read ACK releases the receive record without producing a TFTP reply. Existing transfer ports remain reserved for their peers. Before the first write commits, the existing missing-file response applies. The diagnostic displays MCFG WRITES, MCFG FILE BYTES and MCFG READ DONE. These are local counters, and modem acceptance of the negotiated sizes remains to be tested.

The GLINK owner fixture passed the complete WRQ/OACK/DATA/ACK/RRQ/OACK/DATA/ACK exchange. Enabled host tests passed 458, disabled passed 457, and each run ignored one private-input case. Six standalone mcfg tests, 29 source checks, the source audit, both target builds, the boot layout and LZ4 checks, and all 280 capability mutations passed. Frozen temporary image native-mcfg-ram-frozen-v1 has SHA-256 338c4168f3a3016f1cde367242c5016e27171e375f1e13231efa5c44fe74fcc0. The attempt is unconsumed and awaits cold Fastboot.

IMG_9444 (SHA-256 `8cb014d8bf38087b4a8dd43837c4cca839b5eb08b774084ea36efdccb82c673e`) shows MCFG WRITES 1, MCFG FILE BYTES FFFFFFFF (no committed file), and MCFG READ DONE 0. The first rejection is a 65-byte mcfg.tmp WRQ from 0:0x56 to 1:0x5000, with the same captured 60-byte prefix as IMG_9443. The write count measures local admission, not OACK publication or acceptance. The existing display cannot distinguish competing-writer rejection, other request rejection, or why the admitted write failed to commit. Next targeted instrumentation should correlate admitted WRQ peer, matching OACK publication/completion, first DATA opcode/block/size without contents, and exact rejection reason. Do not relax the writer guard or change negotiation solely on this photo.

The MCFG transfer trace retains the prior 512-byte, window-one negotiation and bounded RAM file behavior. It records the first admitted WRQ's QRTR peer and transfer port, and tracks that OACK in its assigned transmit slot. OACK FLAGS uses 1 for local admission, 2 for queueing, 4 for GLINK start, 8 for full publication and 16 for matching GLINK completion. The first DATA or ERROR from the same peer records opcode, block or error number, packet length and destination port. No DATA bytes appear in the trace. The first rejected TFTP packet's error code and route are displayed. Code 8 identifies the existing active-writer guard and code 9 identifies invalid mcfg packet syntax or options. The trace does not widen accepted requests or change scheduling.

Frozen trace image native-mcfg-trace-frozen-v1 has SHA-256 ac886106c920c4dc9539013c78b3f35502d6b6ff877156b3f5e4503c20daaa22. The enabled host suite passed 459 tests and the disabled suite passed 458, with one private-input case ignored per run. The GLINK fixture verified queue, publication and completion flags and the correlated first DATA header. Both target builds, 29 runtime source checks, the source audit, boot layout, LZ4 round trip and all 280 capability mutations passed. The attempt is unconsumed and awaits cold Fastboot.

IMG_9445 (SHA-256 `472e0c4702acf4dcc0b82b6bcebfaf7bd5033faf5412300d363cf73fc819d509`) shows OACK flags 0x1f, including publication and matching GLINK completion, for the admitted mcfg WRQ from port 0x54 with transfer port 0x5009 and remote intent 0. At capture, the trace had seen no DATA or ERROR from that peer, and no file had committed. The 65-byte WRQ from 0x56 to 0x5000 hit the active-writer guard (code 8). The later request could be a retry on a new port or another writer. GLINK completion establishes transport receipt only. The photo cannot distinguish an option negotiation mismatch from another cause of peer silence. PD state remains 0x7fffffff with zero WLFW servers. The trace image attempt is consumed.

The saved Linux run requested the first WLAN firmware path at 178.730030 seconds and first looked up mcfg.tmp at 187.027163 seconds. This gives no basis to treat the stalled mcfg write as a prerequisite for WLAN discovery. The next temporary image puts the earlier startup evidence on one screen. LOC QUEUED MASK and LOC DONE MASK follow the first reply for each name bucket. Bit order follows the displayed LOC counters from TMS SERVREG at bit 0 through OTHER at bit 6. DONE means matching GLINK completion. The view also reports IPA completion, inbound TFTP request counts, first final write ACK and missing-file reply flags, PD query and listener state, and WLFW inventory.

Frozen image native-wlan-startup-trace-frozen-v1 has SHA-256 f933be1a24468e8e4c1963046316410b85b64daba08272b7a4eccfcf675e60e3. The host test verified first reply tracking across tms/servreg and kernel/elf_loader. Both host configurations passed 459 tests with one private-input case ignored each. Both target builds, 29 runtime source checks, the source audit, boot layout, LZ4 round trip and all 280 capability mutations passed. The attempt is unconsumed and awaits cold Fastboot.

IMG_9446 (SHA-256 `a0fdfded3c3409964acabb35ff6461f039f8a899d84ce9f35f756d69f8a8f310`) shows five LOC requests and five queued replies. Queued and GLINK done masks are both 0x75. With one request in each of the five set buckets, every observed LOC reply completed at GLINK, including kernel/elf_loader. The last accepted LOC name displayed is KERNEL ELF LOADER. LOC TX WAITS is 0x34 (52 enqueue backpressure observations, not a duration). There is no wlan/fw LOC query, WLAN image RRQ, WLFW server or state update. IPA completed. The first final write ACK and first missing-file reply both have 0x1f flags. Query and listener reached phase 4 with PD query state 0x7fffffff (UNINIT). This narrows the observed gap to the interval around or after ELF-loader lookup and before a WLAN image request. Transport receipt does not establish the modem accepted the mapping or responded within an internal deadline. Compare the actual LOC response and timing against a successful Linux run before changing the map or forcing a restart.

The saved successful Linux kernel source at 2267e248204e7e051e0f7355298949a6440de579 defines the ELF-loader row as msm/modem/wlan_pd, instance 180, with database revision 1. Its nested QMI string encoder prefixes a one-byte length because the LOC name descriptor has a maximum of 65 bytes. The previous native encoder used two bytes per nested name. The candidate opts into the Linux width for LOC replies and retains the existing service map. The reference source hashes are included in the frozen artifact set. The saved Linux journal does not contain comparable per-reply timings.

The ELF-loader view correlates its first request handle with the reply. It shows requested offset, total domains, returned rows, receive-to-queue time, queue-to-publication time, receive-to-GLINK-completion time, and queue waits. The host fixture verified the exact reply bytes and all four timestamps through the owner. Both host runs passed 459 tests, with one ignored per run. Both target builds, the source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. Frozen image native-wlan-elf-timing-frozen-v1 has SHA-256 17233f478755a791a12cba233dc089554bd9d1c8bcfa4b09d82c7380bd535c44. The temporary boot attempt is unconsumed.

IMG_9447 (SHA-256 `47c8fac9367bd3b9bad9ef34f064e843d6e5bcc878d47d11b821bc6be78c4ca8`) confirms offset 0, one ELF-loader domain and one returned row with the Linux-width encoder. Peer port is 9. ELF request age is 33,853 ms from native runtime start. Receive to queue is 46,317 ms (0xb4ed) with 39 observed enqueue waits. Queue to publication is 3,558 ms (0xde6). Receive to GLINK completion is 49,965 ms (0xc32d), giving about 90 ms from publication to observed completion by subtraction of rounded values. All five LOC replies are still GLINK-completed (mask 0x75). IPA complete, WLAN RRQ 0, WLFW servers 0, and WLAN PD UNINIT. The encoding fix alone did not yield a WLAN image request in this boot. The dominant observed delay precedes queueing. Next separate per-request FlowBusy from NoCapacity and identify what holds the required credit or transmit slot before changing scheduling. The trace supplies no internal modem deadline.

The ELF reply queue diagnostic records waits for the first tracked ELF request at the branch that returned the error. WAIT SAME PEER counts a queued TX for the same destination, WAIT TX FULL counts all eight slots in use, WAIT LEGACY MAP counts legacy flow-map capacity, WAIT REF CREDIT counts blocked or closed reference flows, and WAIT REF CAP counts reference table capacity. LAST WAIT is the last observed reason in that order (1–5). Occupancy fields are the snapshot at that attempt. TX KIND MASK sets bit 1 for QRTR resume, 2 for control, 3 for RMTFS, 4 for registry, 5 for other QMI replies, 6 for TFTP, 7 for IPA, and 8 for other traffic. PEER TX KIND uses those same codes. PEER TX FLAGS has bits 0–3 for publication started, fully published, GLINK complete, and QRTR satisfied. The latter bit can already be set for a reference-flow packet before publication.

Frozen ELF reply queue trace image native-wlan-elf-queue-trace-frozen-v1 has SHA-256 d5756e224e136184d37b76ab07493f82fef96e724b4fe577c327b37cadbd8da3. Both target builds, the boot layout and LZ4 checks, source audit, 29 source checks and all 280 capability mutations passed. The enabled host suite passed 460 and disabled passed 459 with one ignored each. The targeted owner test separates a same-destination TFTP transmit from a full queue of unrelated destinations. The attempt is unconsumed and awaits cold Fastboot.

IMG_9448 (SHA-256 `a16a9b84901725f8227f168fae71a877053fc1b867c9323689b5ed4940ac8ef2`) shows all 42 ELF enqueue waits were same-destination TX conflicts. Full-slot, flow table, reference credit and unknown counters are zero. At the final blocked attempt only one TX slot was occupied, by an unpublished registry reply to the same destination (kind 4, mask 0x10, flags 0x8). The QRTR flag alone is not an acknowledgement. This snapshot does not identify the earlier reply's transaction, prove it was the same blocker across all waits, or show why it had not published. ELF request age was 34,243 ms, receive-to-queue 50,968 ms, queue-to-publication 5 ms and receive-to-GLINK-completion 51,272 ms. WLAN remains UNINIT with zero WLAN image reads and WLFW servers. IPA complete. The attempt is consumed. Next offline focus: distinguish publication scheduling, remote intent availability and wakeup for the earlier registry reply while preserving same-peer ordering and completion checks.

The ELF blocker publication trace records the earlier registry reply's transaction, padded datagram size, and actual publisher calls while that slot has unsent bytes. BLOCKER MASK bits 0–6 record no suitable remote intent, an outstanding outbound intent request, selection of receive control, selection of another data slot, selection of this slot followed by FIFO WouldBlock, successful publication of this slot, and selection of an outbound intent request. These are observations over that slot's lifetime as of the last blocked ELF enqueue. They are neither durations nor proof of why time passed between publisher calls. A zero call count means the blocker had not reached the publisher before that snapshot. The trace adds no I/O reads and preserves ordering, admission, intent, and completion rules.

Frozen image native-wlan-elf-publish-trace-frozen-v1 has SHA-256 8aaff59806fa2b3e47da4b0b5e5aa3439dfeb4455cbe7284b0119354d4980433. Both integrated host runs passed 460 tests, with one private-input case ignored per run. All 167 standalone GLINK tests passed, including the new fixture covering absent intent, pending grant, FIFO backpressure, and completion releasing the subsequent same-peer reply. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. The image attempt is unconsumed and awaits fresh cold Fastboot.

IMG_9449 (SHA-256 `800bd8acdd9261763c239422b0786195f7fbf7fcf651a7c6122659f1ba1da563`) captures 39 same-destination waits and zero full-slot waits. Last blocker is registry transaction 4, 116 padded bytes, with 107 publisher calls and mask 0x2c: receive control selected, other data selected, and the blocker successfully written. No absent suitable intent, pending or selected outbound intent request, or selected-but-FIFO-blocked event was recorded for that slot at the snapshot. At the last ELF enqueue failure the blocking reply was fully published and awaiting matching GLINK completion (peer flags 0xb); QRTR was already satisfied under the reference-flow model. The prior IMG_9448 final blocker was still unpublished, so these runs show distinct snapshots. ELF request age was 34,282 ms; receive-to-queue 46,243 ms, queue-to-publish 3,691 ms, receive-to-GLINK-done 50,024 ms. The trace records selections, not elapsed intervals for the blocker and not identity across all retries. Next measure the blocker's queue-to-publication and publication-to-completion intervals separately before choosing a scheduling change. IPA complete, WLAN PD UNINIT, zero WLAN RRQ and WLFW servers. Attempt consumed.

The registry transaction 4 timing image tracks the first LOC transaction 4 from QRTR node 0 port 9. TX4 NAME CODE identifies the lookup, TX4 RX AGE MS runs from native runtime start to request receipt, TX4 QUEUE MS runs from receipt to enqueue, TX4 PUB MS runs from enqueue to full GLINK publication, and TX4 RXDONE MS runs from publication to the observed matching GLINK completion. Missing timing endpoints display FFFFFFFF. TX4 ELF WAITS counts blocked ELF enqueue attempts attributed to that exact request handle. This is a retry count, not elapsed time. Earlier ELF timings and blocker metadata remain visible. The new observations add no I/O and preserve the prior RAM-only behavior and transport rules.

Frozen image native-wlan-registry-txn4-timing-frozen-v1 has SHA-256 9a94031272c6052413434f39c6e62485836dd316fe88e486e7bc8374d7c34f83. Both target builds, 461 enabled and 460 disabled host tests (one ignored per run), 29 source checks, the source audit, boot layout, LZ4 round trip and all 280 capability mutations passed. The fixture rejects another port's transaction 4 and distinguishes queue, publication and exact GLINK completion while correlating both ELF waits. The attempt is unconsumed and awaits fresh cold Fastboot.

IMG_9450 (SHA-256 `720fd01af70771e2f4a179022da93227b2c74a5ad5258dc5f50b7d32f2d47087`) identifies registry transaction 4 as tms/servreg (name code 1) and ties all 39 blocked ELF enqueue attempts to its exact handle. TX4 was received at runtime age 33,372 ms. It took 14,424 ms from receive to enqueue, 31,747 ms from enqueue to full publication, and 459 ms from publication to observed matching GLINK completion. The last blocked snapshot showed TX4 published and awaiting GLINK completion (flags 0xb). Blocker mask 0x2c recorded receive-control and other-data selections plus successful TX4 publication across 107 publisher calls, with no unsuitable-intent or selected FIFO-block event recorded. The mask does not assign the elapsed time to individual causes. ELF took 46,659 ms to enqueue and 50,489 ms to reach matching GLINK completion from receipt. IPA complete, WLAN PD UNINIT, no WLAN-image reads or WLFW servers. Attempt consumed. Next candidate: test a bounded send turn for ready registry replies while preserving same-peer order, receive-control dependencies, credit and completion validation, then compare TX4 and ELF times.

The registry transmit-turn comparison adds a single priority publication for an eligible registry reply before returning to the ordinary scheduler for at least one successful record. A reply is eligible only through the existing transmit and remote-intent checks and after its own receive completion. The added turn yields to ready QRTR credits, existing TFTP priority and pending intent ACKs. It preserves same-peer serialization and validated completion. BLOCKER MASK bit 7 records a successful experimental publication of that exact blocker. REGISTRY TX TURN retains the TX4 and ELF timing fields.

Frozen image native-wlan-registry-tx-turn-frozen-v1 has SHA-256 45ecdefa0a1a49ee70e1384c7b1ae234b93d543fc7264b854393a07510231e95. Both integrated suites passed 461 tests with one ignored per run. Standalone GLINK passed 169 enabled and 167 disabled, including bounded priority, identical retry bytes after WouldBlock, exact same-peer completion, and mandatory receive control. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed.

IMG_9451 (SHA-256 `d7d71461399d211a17b73e60cb2b0db9423cb0672320878988127e6ea993b712`) reports TX4 tms/servreg queue/publication/RX_DONE intervals 13,405/305/104 ms versus baseline 14,424/31,747/459 ms. ELF queue/publication/completion intervals are 13,718/292/16,548 ms (completion measured from receipt) versus baseline 46,659/3,740/50,489 ms. ELF retries, same-peer waits, full-slot waits, and TX4-attributed waits are zero. With no blocked enqueue snapshot, BLOCKER fields are FFFFFFFF and this screen does not show whether TX4 itself used the added priority branch. The comparison nevertheless shows a large improvement in publication and total ELF completion in this boot. The dominant residual interval is request receipt to first successful enqueue. Zero observed retries does not distinguish a wait before first registry dispatch from earlier service work or delayed host reentry. IPA complete; WLAN PD UNINIT, WLAN-image reads and WLFW servers zero. Attempt consumed. Next isolate request-to-first-dispatch timing and the runtime work before that call.

The ELF dispatch timing image retains the registry priority turn and adds three first observations for the existing ELF lookup handle. RX PASS MS is receipt to successful return of the owner transport work (including any receive, publication, and IRQ work left in that pass). PASS TO REG MS is then the interval to the first registry handler entry. REG TO ELF MS runs until the registry handler first selects the exact ELF request, potentially across multiple service turns. ELF ENQUEUE MS runs from first selection to successful queueing, including retries if any. FFFFFFFF means an endpoint is absent. ELF and TX4 timing fields remain available. The image adds observations only.

Frozen image native-wlan-elf-dispatch-timing-frozen-v1 has SHA-256 fca4ae2a26c3b9f243b4f06b3942c51a259a98e3daeffc078ab97afd7fd6e51a. Both integrated suites passed 461 tests with one ignored per run. The host fixtures verify the ordered checkpoints on a normal reply and distinguish first registry entry from later selection with an older request ahead. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. The attempt is unconsumed and awaits fresh cold Fastboot.

IMG_9452 (SHA-256 `65fa1ec10b96be8ce06fc9f420f92c4cdac589b0a97933551dff9300a8e9f333`) reports 734 ms receipt to owner transport return, 27 ms return to first registry entry, 12,793 ms registry entry to first selection of the exact ELF request, and 41 ms selection to successful enqueue. Total queue interval was 13,596 ms (component values are individually rounded). No ELF retries, same-peer or full-slot failures were recorded. Adding the rounded host ages places TX4 matching GLINK completion near 46.80 s and ELF selection near 47.10 s after runtime start. This is correlation only. The selection interval can span multiple registry calls and time in other runtime work. Next trace the number, selected LOC identity and outcome of intervening registry turns and the longest gap between them, including the existing phase timings across the gap. IPA complete; WLAN PD UNINIT, WLAN-image reads and WLFW servers zero. Attempt consumed.

The ELF registry-turn trace keeps the four dispatch intervals and records non-ELF registry replies selected after the ELF request was received and before it was selected. REG CALLS counts recorded preselection calls. REG MAX GAP MS is the largest registry entry-to-entry interval through the call selecting ELF; REG MAX CALL MS is the largest preselection entry-to-queue-outcome interval, potentially from a different call. REG RUN 0–3 show consecutive runs as TTTTNOCC in hex: transaction, name code (0 unknown), outcome (1 queued, 2 flow busy, 3 no capacity, 4 other error), and count. The transaction number is peer-scoped. REG OMITTED counts calls beyond the four bounded rows. The instrumentation retains prior ordering and adds no device I/O.

Frozen image native-wlan-elf-registry-turns-frozen-v1 has SHA-256 5fdf219428b2dabf370a5c4cdf4f5f15d91c177d2bfd0aaa057d1762daa6e67a. Integrated host suites passed 462 enabled and 461 disabled with one ignored per run. Tests cover the earlier TX4 selection ahead of ELF, outcome encoding, repeated-call compression, overflow, and separate maximum gap and handler duration. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. The attempt is unconsumed and awaits fresh cold Fastboot.

The settled REGISTRY TURNS photo (SHA-256 `5066c7bfa660ff2e6c3f4b2653cff92ca6cec6e5df46b47ad0b3ddae2d462d4f`) shows 13 recorded preselection calls and zero omitted. Run 0 is transaction 4, name code 1 (tms/servreg), outcome 2 (FlowBusy), count 12. Run 1 is the same transaction and name with outcome 1 (queued), count 1. Transaction IDs are peer-scoped and the compressed rows do not display the peer. Maximum registry entry gap was 2,783 ms through the selecting call; maximum observed preselection call was 82 ms. Registry entry to ELF selection was 13,216 ms. ELF queued without any retries once selected. TX4 then took 305 ms from queue to publication and 104 ms to observed matching GLINK completion. The trace establishes repeated TX4 enqueue backpressure ahead of ELF, but does not attribute the 12 FlowBusy outcomes to an earlier same-peer transmit or to the reference flow's blocked/closed path. Next use the existing queue-pressure snapshot on the exact TX4 request and capture reason counts and blocking reply identity/state. IPA complete, WLAN PD UNINIT, zero WLAN-image reads and WLFW servers. Attempt consumed.

The offline second-reply candidate allows one additional reference-flow registry reply after the one older same-peer registry reply is fully published and still holds its GLINK intent. The candidate leaves other services serialized, uses the ordinary credit reservation and distinct busy intent rules, and retains exact matching completion. The targeted fixture verified send order, two separate intents, a blocked third reply, out-of-order completions matched by intent, and the QRTR watermark still blocking a new reply. The pinned Linux QRTR oracle also passed (using the same deprecated/dead-code lint allowances as the integrated host runner). The overlap candidate is not enabled in the device image.

TX4 FLOWBUSY TRACE counts the exact transaction 4 request's same-peer and reference-flow queue rejections separately. TX4 OTHER WAIT combines the remaining reasons and unclassified rejections. TX4 LAST REASON and the blocker transaction, kind, flags and publication mask describe the last rejected attempt. The reference-flow reason includes both credit blocked and closed. FFFFFFFF indicates unavailable blocker information. The prior registry-turn trace and faster publisher stay enabled, with some display rows replaced.

Frozen image native-wlan-tx4-pressure-frozen-v1 has SHA-256 ccdb100d20a3f686761117e07f11af24556b54bc6bd4148a541e9806eee9fc8e. The integrated host runs passed 463 enabled and 462 disabled tests with one ignored per run, including retention of the exact preceding blocker after TX4 succeeds. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. The attempt is unconsumed and awaits fresh cold Fastboot.

Settled TX4 FLOWBUSY TRACE photo (SHA-256 `a298c84487e63110816c223d730b95156a7a4e90c08d195789d190074bf208af`) records all 12 TX4 rejections at the native same-peer transmit guard (reason 1), with zero reference-flow or other waits. The last rejected attempt identifies registry transaction 3 as the predecessor (kind 4). Flags 0x8 mean no bytes sent, no GLINK completion, and QRTR satisfied under the reference-flow bookkeeping. The 0xc mask records receive control and other data selected during that predecessor's queued lifetime through the sample, with no recorded missing suitable intent or selected FIFO block. This is only the last pressure sample. The offline second-reply experiment requires the predecessor to be fully published, so its condition was false at that sample. Next isolate why TX3 did not get an eligible registry transmit turn, including its own RX_DONE dependency, pending intent ACK, TFTP priority, ready QRTR credits, and turn budget. ELF received-to-enqueue 13,306 ms and received-to-GLINK-completion 13,686 ms, with zero ELF enqueue retries. WLAN PD UNINIT, WLAN RRQ and WLFW servers zero. Attempt consumed.

TX4 BLOCKER TX GATES counts one first applicable priority gate per publisher call while each queued registry reply is unpublished. The screen retains the last TX4 rejection's blocker and distinguishes a normal-turn cooldown, existing TFTP priority, pending intent ACK, the reply's own pending RX_DONE, no eligible registry candidate, a different candidate, ready QRTR credit, and priority selection. GATE SELECTED does not imply a successful write; use TX4 BLOCK MASK and FLAGS for publication evidence. The existing scheduler and same-peer admission rules remain in effect, with the offline second-reply feature disabled.

Frozen image native-wlan-registry-turn-gates-frozen-v1 has SHA-256 e9ba59879ec61cf7c1fffa7b3086b12b49c1fe36e22a227d59270cdb5c7aa794. Both integrated runs passed 463 tests with one ignored each; 169 standalone GLINK tests passed. The tests cover exact blocker retention, the counter sum and selected writes that can still return WouldBlock. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. Attempt unconsumed, awaiting cold Fastboot.

Intermediate TX4 BLOCKER TX GATES photo (SHA-256 `7c2fd00143016de449fd8fcf185dfa12079fd411140306f2e5a49829cb5d506e`): TX4 12 same-peer waits, no reference-flow waits. Last registry blocker was unpublished (flags 8), with mask 0xc. Gate counts 70 pending host intent ACK, 3 TFTP, all other first-gate counts zero. Progress paint overwrote BLOCKER CALLS and TX4 BLOCK TXN; IPA completion and state query should be read from a settled full refresh. Gate counts are attempts, and do not identify whether one ACK remained pending or consecutive ACKs kept winning. Attempt consumed.

Settled TX4 BLOCKER TX GATES photo (SHA-256 `aef65f704701fddd435f2082a7b5396e59cd4c6a0a8400fe4a3f07c1724f6137`) confirms txn3 as the last TX4 blocker. 73 publisher calls in its snapshot: 70 attributed to the pending host intent-ACK gate, 3 to TFTP priority, no selections via registry priority. Flags 8 show no publication yet at the sample. Counters count first applicable gates and do not establish lower-gate eligibility, elapsed time, or the number of distinct ACKs. TX4 had 12 same-peer waits and zero reference-flow waits. ELF reached GLINK completion in 13,691 ms from receipt. IPA complete, query phase 4 with WLAN PD UNINIT, no WLAN image reads or WLFW servers. Next distinguish repeated successful intent ACKs from one pending ACK held behind receive completion, advertisement, or FIFO backpressure. Attempt consumed.

TX4 BLOCKER ACK TRACE tracks successfully published host intent ACKs, ACK FIFO WouldBlock outcomes, and receive completions and advertisements published while an ACK is pending. The counters cover the last TX4 rejection's exact registry blocker while it was unpublished. ACK MAX AGE MS is the longest completed ACK observed for that blocker, measured from accepting the intent request to successful publication. ACK AT WAIT MS is the age of the pending ACK at the last TX4 rejection if its blocker is unpublished. FFFFFFFF means missing. Both use the existing host counter. The image preserves the active scheduler and the existing backing-advertisement and completion order.

Frozen image native-wlan-registry-ack-trace-frozen-v1 has SHA-256 7a60861d8e9aa0ad24579257db55ecab829ecb870ae3384595d39ce6f89c9824. Both integrated suites passed 463 tests with one ignored per run. All 170 standalone GLINK tests passed, including the new fixture for a held ACK, a FIFO-blocked attempt and two successful ACKs with independent ages. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. Attempt unconsumed, awaiting cold Fastboot.

Settled TX4 BLOCKER ACK TRACE (SHA-256 `647bd434ec2c48f3c08d55e387b5f13b4e671964e9923b0e6891d8d26e8f1cec`): transaction 3 was the unpublished same-peer blocker at TX4's last rejection, with 12 same-peer waits and zero reference-flow waits. While that blocker was unpublished, one host intent ACK was successfully published, aged 20,609 ms from acceptance. The publisher sent 33 RX_DONE records and 36 receive advertisements while an ACK was pending. No selected ACK write hit FIFO WouldBlock and no ACK was pending at the final snapshot (FFFFFFFF). The 69 receive-control records plus one ACK account for the 70 intent-ACK-gated calls in the snapshot. The trace does not distinguish grant from denial or identify the backing advertisement. Next candidate is a bounded scheduler turn for the pending ACK and, for a grant, the exact backing advertisement first. Preserve validated writes, normal receive progress, and same-peer/credit rules. Compare ACK age, TX4 retries, ELF completion and WLAN RRQ/WLFW. IPA complete, WLAN PD UNINIT, WLAN RRQ and WLFW servers zero. Attempt consumed.

The GLINK INTENT ACK TURN image gives each pending intent acknowledgement a bounded priority turn. A grant first publishes the advertisement for the buffer allocated to that request. A denial uses only the ACK. The next successful normal publication reopens the priority turn. If no other record is ready, an ACK can still progress through the normal path. The existing writer handles retry and uncertain publication. ACK TOTAL and ACK BOOT MAX MS summarize successful acknowledgements across the boot. The blocker-specific rows are still available when TX4 had a queue rejection.

Frozen image native-wlan-intent-ack-turn-frozen-v1 has SHA-256 7450082da6cb504883919c32910c9fa5ef6e7fd13e3f5c94adf4ae03424cdba7. Both integrated suites passed 463 tests with one ignored per run. All 170 standalone GLINK tests passed, including exact backing selection, identical bytes after WouldBlock, and ordinary control and data progress under successive ACKs. Both target builds, source audit, 29 source checks, boot layout, LZ4 round trip and all 280 capability mutations passed. Attempt unconsumed, awaiting cold Fastboot.

GLINK INTENT ACK TURN photo (SHA-256 `48282f6df4a76a42817dd3e7d9f4f259f13e2c3e5f15dea4a119fe18409167ac`) shows 150 successful host intent ACKs, with a boot-wide maximum completed ACK age of 2,633 ms versus the previously observed blocker-specific 20,609 ms. TX4 same-peer waits dropped from 12 to zero and no TX4 blocker sample was present (FFFFFFFF). ELF request-to-matching-GLINK-completion dropped from 16,220 to 3,977 ms. ELF request age moved from 34,840 to 42,036 ms, so runtime-start-to-completion improved from about 51.06 s to 46.01 s. IPA complete; WLAN query reached phase 4 and reported UNINIT. WLAN RRQ and WLFW servers remained zero. Carry the ACK scheduling change into the next comparison and inspect the exchanges around and after the ELF reply against the working Linux sequence. The captured improvement did not establish the modem's WLAN launch trigger. Attempt consumed.

### SM7150 locator comparison after the ACK scheduling fix

The pinned Linux reference (`2267e248204e7e051e0f7355298949a6440de579`, `drivers/soc/qcom/qcom_pd_mapper.c`) builds `tms/servreg` from all six entries in `sm7150_domains[]`. In order they are `msm/adsp/audio_pd` (74), `msm/adsp/root_pd` (74), `msm/adsp/sensor_pd` (74), `msm/cdsp/root_pd` (76), `msm/modem/root_pd` (180), and `msm/modem/wlan_pd` (180). Native currently returns the last two. Both map `kernel/elf_loader` and `wlan/fw` to `msm/modem/wlan_pd` at instance 180. The four additional rows are static locator metadata. The mapper adds them when building its table.

With Linux's one-byte nested string length and QRTR v1 padding, the two-row `tms/servreg` response is 116 bytes and the six-row response is 224 bytes. The one-row ELF-loader response is 88 bytes. The saved Linux GLINK trace records a 224-byte outbound IPCRTR record at 214.119486 s, its matching RX_DONE 192 microseconds later, and an 88-byte outbound record at 214.123309 s. The TFTP trace records a remote read request at 214.125477 s. This metadata is consistent with the calculated sizes and order. The trace did not retain the QMI payloads or that TFTP path, so it cannot identify those records by size alone. The separate working cold Linux journal records a request for `/readonly/firmware/image/wlanmdsp.mbn`.

Next controlled comparison is the six-row `tms/servreg` reply with the ACK scheduling fix retained. Check the row order, instance IDs, encoded lengths and existing reply bounds offline. On device, compare matching GLINK completion, WLAN-image read requests, the WLAN domain state and WLFW service discovery. The comparison tests whether the map difference matters to WLAN startup.

The six-row SM7150 locator image is frozen as `native-wlan-sm7150-locator-frozen-v1` with SHA-256 `ad8f7a9048ccd6048b4e635ddbf94f52da3f1e7a6696ce77ed2abf97dc5273f9`. The ACK scheduling fix remains enabled. Both 463-test host runs, all 170 standalone GLINK tests, 29 source tests, both target builds, boot layout, LZ4 round trip and 280 capability mutation checks passed. The integrated owner test verifies a 224-byte registry reply through GLINK. Device test pending.

IMG_9453 (SHA-256 `057956b8e926e2cd8dfa5f60f03199911be9c0850bcf27f5906db7a245c52880`) records the single six-row image boot. IPA completed and TX4 same-peer waits remained zero. TX4 publication took 489 ms, with matching GLINK RX_DONE 90 ms later, versus 492 and 90 ms on the two-row ACK-turn baseline. The ELF reply reached matching GLINK completion in 3,947 ms versus 3,977 ms. The run recorded 160 successful intent ACKs and a maximum completed ACK age of 740 ms versus 2,633 ms on the baseline. WLAN query phase was 4 and returned UNINIT, with zero WLAN-image reads and zero WLFW servers. The six-row image reproduced the same WLAN checkpoint. The display does not verify the response payload or firmware handling of it. Next compare the QMI exchanges around and after the ELF lookup, using message identity and endpoints where available, to find the first confirmed divergence from working Linux. Attempt consumed.

### 2026-11-10: locate the ELF exchange in the Linux reference

The six-row native build and the successful cold Linux run have matching MPSS firmware hashes in their manifests. The saved Linux GLINK capture shows a 60-byte modem IPCRTR receive at 214.123045 s, an 88-byte send at 214.123309 s, and its matching RX_DONE at 214.123477 s. Those sizes fit the offset-free `kernel/elf_loader` LOC lookup and its one-row response. If that identification holds, the exchange took 432 µs through matching GLINK completion, versus 3,947 ms for the identified native exchange in IMG_9453. The Linux trace does not contain QMI transaction IDs, service names, or QRTR data endpoints. Nearby 52-byte traffic is unidentified.

In that same trace boot, the first three subsequent remote TFTP read requests carried 102, 92, and 91 bytes. The first arrived at 214.125477 s, 2.000 ms after the 88-byte reply's completion. The sizes fit the three WLAN firmware paths recorded in the separate path-bearing cold Linux journal in the same order, with exactly 54 non-path bytes in each case. The first two Linux transfers receive TFTP ERROR and the third receives OACK, also consistent with the separate journal. This strengthens the WLAN-read identification without making it definitive. Native's WLAN RRQ counter recognizes all three path suffixes on successfully observed requests, including both vendor aliases. Zero is evidence about the owner-observed traffic, with rejected or unrouted traffic outside that counter.

The pinned Linux locator has a separate PFR crash-report handler and the native locator accepts only GET_DOMAIN_LIST. There is no decoded PFR request in the saved captures. The pinned Linux kernel configuration enables dynamic debug and builds the PD mapper as a module. For the next reference capture, enable only the existing `qcom_pdm_get_domain_list` debug sites for the startup window, align those service names and row counts with the existing GLINK timestamps, and capture the existing TFTP path log in the same boot. QRTR name-service tracepoints can identify announcements and their endpoints. Those tracepoints do not decode QMI data messages. This comparison would test the candidate ELF timing and following WLAN read without logging bulk packet contents. No native behavior or device state changed during this review.

The targeted Linux reference capture is prepared as `elf-locator-reference-prepared-v1`. Both the standard startup and the guarded provider-ready continuation retain the verified stage0-v2 image, RAM-backed EFS policy, and TFTP metadata observer. The wrappers require the two pinned locator debug sites and the mono-clock GLINK and QRTR server-add tracepoints before the single modem start. Cleanup restores the locator sites' print flags and saves locator events, trace clocks and loss stats, TFTP socket metadata, and timestamped TFTP path logs. The capture does not add a native image or change the Linux startup inputs. Script syntax, activation order, cleanup registration, hashes and path extraction from the saved Linux journal passed offline. Runtime observer availability is pending the temporary boot.

The targeted Linux capture completed on one temporary boot. The first wrapper stopped at provider lookup before modem start. The guarded provider-ready continuation ran the first modem start and finished successfully. Linux reported modem running and wlan0 present. All four raw EFS hashes matched, both locator print flags were restored, and the eight CPU trace stats reported zero overruns or dropped events. Logs and artifact hashes are in `.cache/sunfish-native-wifi/private/elf-locator-reference-prepared-v1/run/summary.json`.

The mapper logged both the six-row `tms/servreg` query and the one-row, offset-free `kernel/elf_loader` query. The likely matching GLINK sequence took 374 µs from the 60-byte request to RX_DONE for the 88-byte reply. The first named WLAN firmware request reached the TFTP socket 692 µs after that RX_DONE, and WLFW service 69/version 1 was later announced at node 0 port 81. Locator printk times are about 6.5 ms ahead of the associated GLINK send records in this capture. The intervals are measured using GLINK and TFTP socket monotonic timestamps. Length, order, and service logs support the association, with QMI transaction identity still unavailable.

The capture also proves Linux answered an earlier `avs/audio` lookup with `msm/adsp/audio_pd`, instance 74. Native's six-row build includes that domain under `tms/servreg` but still returns zero rows for the separate `avs/audio` key. Earlier native startup captured one lookup under OTHER, which is insufficient to identify it as `avs/audio`. This is a concrete lookup difference to check against the native request names and an offline wire fixture before changing behavior. If tested on device, isolate the exact alias from any change to ELF scheduling or domain power state.

The earlier native photo records one OTHER lookup and retains only the final overall name, KERNEL ELF LOADER. The optional native locator diagnostic stores the first accepted OTHER name for display and counts exact lowercase `avs/audio` requests. The observer fixture verifies first-name retention, exact byte matching and the existing zero-row AVS result. Both 463-test host runs, both target builds, 29 source tests, source audit, boot layout, LZ4 round trip and 280 capability mutation checks passed. The frozen image is `native-wlan-locator-other-frozen-v1`, SHA-256 `726a1fc3a6e4b52f57f89023f7e59cc2fc1b1b91970dbfde90aae41994135fa0`. Fastboot accepted one temporary boot after cold readiness was confirmed. Device display pending.

IMG_9454 (SHA-256 `15a036591093f40c9b3f803c036e6138252ab46558059f86d9b6d03d1dfc3d4c`) identifies the single OTHER lookup as the exact lowercase `avs/audio` request. The native image still returned the empty map. IPA completed and the ELF reply reached GLINK completion in 3,978 ms. WLAN remained UNINIT with zero firmware read requests and zero WLFW servers. The next comparison adds only the reference alias for this exact lookup and keeps existing scheduling. The user returned the Pixel to LineageOS and requested preparation only.

The AVS alias comparison image is frozen as `native-wlan-avs-audio-frozen-v1`, SHA-256 `7529b800b232ccd914abeab396eb93c4c27e9b8a86624cbbd4c7702dd9439702`. Its sole runtime behavior change is the exact `avs/audio` reply with one `msm/adsp/audio_pd` row at instance 74. A byte-exact reply fixture passes with the alias enabled and disabled, including the previous empty response when disabled. Both 464-test host runs, both target builds, 29 source tests, source audit, boot layout, LZ4 round trip, and 280 capability mutation checks passed. The phone was left in LineageOS and the attempt is unused.

IMG_9456 through IMG_9459 show the AVS alias boot progressing from zero to one exact AVS request, through IPA completion, and to query phase 4 with a validated UNINIT response. No WLAN firmware read request or WLFW server appears in these snapshots. The AVS receive counter measures the query, and the displayed TX4 and checkpoint fields do not report completion of the AVS reply or of the ELF reply. The result is specific to the captured interval.

IMG_9460 (SHA-256 `7fa8696016de8bb08f3b76f27cf58827bf7d8273af410612b9cca5e9854b9792`) now shows matching GLINK completion of the ELF reply at 3,948 ms from request receipt, versus 3,978 ms before the AVS alias. IPA is complete and query phase 4 still reports UNINIT with zero WLAN read requests and zero WLFW servers. The AVS alias alone did not move the observed WLAN checkpoint in this boot. AVS AUDIO RX records one accepted request and does not provide an AVS reply-specific completion marker. The next timing comparison should identify which stage accounts for the remaining native ELF response delay.

The ELF REPLY PHASES image is frozen as `native-wlan-elf-phase-display-frozen-v1`, SHA-256 `ae41ee1a69473c387ff2038bb0f84c4cc238bad6f31236c22b6c29314e59d633`. It displays six intervals using timestamps already recorded by the modem owner, plus the registry call count and longest observed gap. The image changes the display. Both 464-test host runs, both target builds, 29 source tests, boot layout, LZ4 round trip and 280 capability mutation checks passed. The boot attempt is unused.

IMG_9461 through IMG_9463 show a 4,456 ms ELF request-to-matching-GLINK-completion interval. The owner spent 926 ms finishing its receive pass after accepting the request, under 1 ms to enter registry dispatch, 2,937 ms from first registry entry to ELF selection, 41 ms enqueueing, 461 ms publishing, and 90 ms from publication to observed RX_DONE. Displayed intervals sum to 4,455 ms due to integer rounding. The longest gap between registry entries was 2,910 ms. One other registry request was processed before ELF selection and ELF had zero enqueue waits. The gap points to time between dispatch turns. These measurements do not identify the work or wakeup responsible for that gap. IPA completed, WLAN remained UNINIT, and both WLAN firmware reads and WLFW server counts stayed at zero.

The ELF REGISTRY GAP image is frozen as `native-wlan-elf-registry-gap-frozen-v1`, SHA-256 `8580e30671ce07655ea84daac9db4e0926d1b4507e301a90135b7ffbed3c5f0e`. It records the longest interval between adjacent registry entries before first ELF selection. It attributes existing router, RMTFS, IPA, registry, and display timing deltas, and measures time between runtime notification callbacks. Unattributed time includes unmeasured paths and timing overhead. Notification count and the physical badge for the ending pass distinguish a later callback from another pass within the same callback. The feature observes existing scheduling. The timing fixture, host suites, both builds, boot layout, LZ4 round trip, and capability audit passed. Attempt unused.

IMG_9464 (SHA-256 `41301e4c2ae21dd2edb01bb40d920d0864a8f48709b8c28e0357fc0e0bfb069d`) attributes 2,750 ms of the 2,900 ms longest registry-entry gap to router dispatch, about 95 percent. RMTFS accounted for 27 ms, IPA for 54 ms, and registry work for 68 ms. Display, time outside the callback and unattributed time each rounded to zero. The component sum is 2,899 ms due to rounding. No new runtime notification occurred within the interval, and the ending pass retained badge 0x130. The gap was within one callback while servicing the owner. ELF request-to-matching-GLINK-completion was 3,933 ms, with zero enqueue waits. IPA complete, WLAN UNINIT, no WLAN read requests or WLFW servers. Next inspect the existing checked-I/O profiler for the router pass across this exact registry gap, including status/domain checks, descriptor/cursor reads, FIFO reads/writes, barriers and notification.

The ELF GAP IO TIMING image is frozen as `native-wlan-elf-gap-io-frozen-v1`, SHA-256 `1c04352fc45d3fd62be8ce54ad86b9914a55b11a55c8da92e29dc302090044a6`. Its snapshot uses the exact longest registry interval selected by the prior diagnostic, with cumulative deltas from existing owner I/O counters. The rows show observation, descriptor/cursor reads, FIFO reads, other I/O, individual write/barrier operations, and the two barrier-domain checks. Detail and domain rows are nested measurements. A fixture verifies interval identity, retention and invalid counter handling. Both 464-test host runs, 19 diagnostic tests, both builds, 29 source tests, boot layout, LZ4 round trip and 280 capability mutation checks passed. Attempt unused.

IMG_9465 (SHA-256 `0f7fcffafb617da616d4259ab4c8f37f4af2b68556f3732ff3f0befe40f48301`) shows a 3,006 ms registry gap and 2,854 ms in router dispatch. The top-level I/O profiler accounts for 2,851 ms, with 712 ms observation, 517 ms descriptor/cursor sampling, 252 ms FIFO reads and 1,370 ms other I/O. Within other I/O, barrier work took 1,104 ms. Its outer retained-domain checks took 449 ms before and 450 ms after, 899 ms combined. These domain rows overlap barrier time. The adapter uses the same outer domain revalidation around the other profiled operations, but their domain times are not separately measured here. Writes/head/tail/notification took 71/70/53/70 ms. The code rechecks live faults, ASID banks, route slots, installed registers and the table image in retained_receive_domain. Next audit this repeated path for a smaller equivalent verification cost, using the current guard behavior as the regression boundary. IPA complete, WLAN UNINIT, no WLAN image reads or WLFW servers.


The retained-table-expectation candidate adds opt-in `sunfish-smmu-retained-table-expectation`. Receive staging already reconstructs an independent canonical plan, compares all supplied descriptors with it, publishes the exact eight tables, and reads the publication back. After these checks succeed, the candidate retains that validated CPU-owned image. Runtime revalidation uses the retained expectation instead of reconstructing the mapping, alias/geometry checks, root plan and descriptors around every small GLINK operation. The address/geometry inputs are private adapter fields or lifetime-long immutable borrows, and the selected installation cannot be staged again. The retained expectation is never sourced from live page-table memory. It adds 32 KiB to each native adapter value.

The candidate preserves both outer revalidation boundaries and their quarantine behavior, the fault flag and live global/context fault reads, ASID-bank scan, route-slot scan, installed-register comparisons, both synchronization barriers, and volatile comparisons of all 4,096 live table words including empty entries. A missing expectation fails closed. Hardware, seL4 mappings and timing behavior remain unproven until a fresh device test. Avoiding CPU reconstruction may leave uncached MMIO/table reads as the dominant cost.

Host validation passed with 466 tests enabled and 465 disabled, each with one prior ignored test. Both runs flip every word in each of the eight live tables and reject all 4,096 corruptions, then accept the restored tables. Existing route/context/fault and uncertain-publication tests pass. Additional mutants that remove the live-table comparison, canonical-image comparison or handoff revalidation are each rejected. Both AArch64 builds, the 29 runtime source tests, 19 timing-fixture tests, source audit and boot-layout check pass. A static inspection of direct-call prologues found a largest explored path of 3,444,928 bytes within the existing 4 MiB stack. This excludes unresolved indirect calls and is not a device high-water measurement. The capability audit rejected all 280 mutations. The candidate is frozen as `native-smmu-retained-expectation-frozen-v1`, SHA-256 `07c348acc50f002a9b186c946934aab1b5069d6cdcbed0c4550b4d309d5a96ad`. The image, source and test inputs are pinned in its manifest. Its one-shot boot attempt is unused. The device comparison retains ELF GAP IO TIMING so DOMAIN PRE/POST, barrier/router totals and ELF completion can be compared with IMG_9465. No phone action was taken during this preparation.


Clipboard photo 58d4929d (SHA-256 `9cd839ab5db250919c0b93a53e10a954e3f96182e75f16156ba3fa79b83cb7d8`) records the consumed retained-expectation image. ELF request-to-matching-GLINK-completion is 3,250 ms versus 3,972 ms in IMG_9465, 722 ms or about 18.2 percent shorter in this single comparison. The current phases are 2,027 ms finishing the receive pass, 0 ms to registry entry, 23 ms registry entry to ELF selection, 34 ms enqueue, 1,089 ms to publication, and 75 ms to matching RXDONE. Their rounded sum is 3,248 ms. ELF was selected on the first registry turn, with zero intervening calls, zero preselection gap and zero TX waits. Consequently RegistryGap.longest is absent and the conditional ELF GAP IO TIMING overlay does not appear. The screenshot cannot quantify the individual domain-check savings. The remaining measured time is concentrated in receive-pass completion and enqueue-to-publication. Any follow-up profiler should cover those request-scoped intervals even without an intervening registry turn. IPA complete, WLAN PD UNINIT, zero WLAN RRQ and zero WLFW servers. This painter requires runtime error zero. One snapshot does not establish continued activity or a WLAN startup advance.

After about one additional minute, the user reports the retained-expectation run fields unchanged. The timing improvement did not produce a WLAN startup advance in the observed interval. The attempt remains consumed. Next comparison needs request-scoped profiling of receive-pass completion and enqueue-to-publication rather than requiring a second preselection registry entry. Return to LineageOS before further temporary device tests.


The ELF PHASE IO diagnostic adds opt-in `sunfish-wlan-elf-phase-io`. It copies existing cumulative owner-scoped I/O counters at the first exact ELF request's receive timestamp boundary, first receive-pass return, successful reply queue and first matching publication. The receive counter copy follows query decoding without an intervening I/O operation. These snapshots identify the first request even when the first registry turn selects it, and preserve the first phase boundaries through subsequent work. No additional MMIO, FIFO read, barrier, peer notification or scheduling turn is introduced. The native hook copies CPU state through an immutable adapter reference.

The screen shows R-prefixed receive-pass and P-prefixed queue-to-publication deltas for observation, descriptor/cursor sampling, FIFO reads, other I/O, barrier time and its two retained-domain checks. Missing, saturated or reversed samples remain unavailable. The broader phase elapsed times and ELF total are retained, along with WLAN query state and WLAN/WLFW discovery fields. Counter timing is scoped to already-enabled owner router dispatch and excludes operations outside that profiling scope. Barrier and domain rows are nested and cannot be added independently to the four broad categories. Counter copies and larger trace state can affect CPU timing.

The actual transport/owner host suite passed 467 tests enabled and 466 disabled, each with one prior ignored test. A regression fixture covers first-turn selection, exact counter boundaries, immutable snapshots through completion and no added I/O for the counter read. Twenty diagnostic fixture tests include missing, saturated, reversed and zero deltas. Both AArch64 target builds, source audit, 29 source regression tests and boot layout pass. The capability audit rejected all 280 mutations. The image is frozen as `native-wlan-elf-phase-io-frozen-v1`, SHA-256 `44f1a40666389b7bb04545c2f847178729053e5cf33f09413ff43b5e01173ee5`. After the user's readiness reply, all frozen hashes and the sunfish identity were verified. Exactly one temporary fastboot boot was accepted. The attempt is consumed, and no on-device runtime result has been observed yet.


ELF PHASE IO intermediate clipboard d0f2824e (SHA-256 `69d3c25382a82d9b482244c348b7100f2c2de518d32e73e3b9719b5ba34e8a5c`) shows IPA complete and ELF matching completion at 2,813 ms. The first receive phase is 2,017 ms, with 501 ms observation, 362 ms descriptors, 172 ms FIFO reads and 981 ms other I/O. The four rounded broad categories sum to 2,016 ms. Within other I/O, barriers take 790 ms, including retained-domain checks of 309 ms before and 309 ms after. Visible publication subtotals are 289 ms other I/O, 234 ms barriers, and 91 ms per domain check. Progress text covers publication elapsed and its first three broad categories. Query phase 3 is pending, PD state FFFFFFFF is unavailable, and WLAN reads/WLFW remain zero in this frame. The user reports a later screen, not yet inspected. Do not treat this as the settled result or add nested category times as independent work.


The later complete ELF PHASE IO photo 138e849d (SHA-256 `a070b47fa91afd0580bc5a23597152bf4222de3e1866a3d00ebca479b142195e`) preserves the receive figures from the intermediate frame and now shows queue-to-publication of 662 ms. Publication I/O consists of 150 ms observation, 109 ms descriptors, 54 ms FIFO reads and 289 ms other work, summing to 602 ms; about 60 ms of the phase is outside those four rounded counters. Barriers are a nested 234 ms of other I/O, with 91 ms in each retained-domain check. Across the two phases, the barrier-specific domain measurements total 800 ms, within 1,024 ms total barrier work. Domain time around other I/O operations is included in the broad categories but not separately measured here. ELF total remains 2,813 ms. IPA complete, query phase 4 returns UNINIT, no WLAN reads or WLFW servers, and the successful painter requires runtime error zero. The concrete remaining cost is checked transport I/O across both phases. Next audit equivalent reductions in validation work or checked-operation count; these measurements alone do not justify removing live route, context, table or fault validation.


The adjacent-pairs audit found three remaining literal barrier-then-observation sequences in Ring: the release check immediately before the receive-tail store, and the complete checks immediately before and after peer notification. They can use the same existing native barrier_observe primitive already used by sample/write. The opt-in `sunfish-glink-adjacent-barrier-pairs` routes these sites through that primitive. The controller still executes glink_barrier followed by glink_poll, each with its existing peer/lock/directory/cursor checks; one full retained-domain pre/post pair brackets them. Ring validates the returned generation, identity, readiness and fatal status at the original boundary. No FIFO read/write, cursor store, barrier, notification or scheduling turn is reordered or removed. Errors and uncertain effects retain quarantine. Each combined pair avoids one outer domain validation pair.

Enabled and disabled host suites each pass 469 tests with one prior ignored test. New tests cover the exact barrier/status/cursor/doorbell order, then inject generation, identity, readiness and fatal changes at all three late boundaries. They verify whether the RX tail and doorbell had already committed, forbid any later I/O and reject retry. Existing native tests still check every retained table page, live fault/peer state and an in-pair table change. The 20 timing fixture tests and 29 runtime source tests pass. Both target builds and boot layout passed; the capability audit rejected all 280 mutations. Three additional mutants omitting each cursor/doorbell check were rejected. The image is frozen as `native-glink-adjacent-pairs-frozen-v1`, SHA-256 `2acc5d5fc9e05e4faaa8f6d914b40dd6e9b7197043763b8d31c32d35fae6422f`. All frozen artifacts and source/test inputs are pinned in the manifest. The one-shot boot attempt is unused, and the phone has not been booted with this image. Pairing classifies the status-check cost under barrier/other instead of observation; the phase elapsed times and broad-category sums are the meaningful comparison. The existing ELF PHASE IO screen is retained.


After the user confirmed cold Fastboot, all adjacent-pairs frozen artifact/source/build/test hashes and product sunfish were verified. Exactly one temporary boot of `2acc5d5fc9e05e4faaa8f6d914b40dd6e9b7197043763b8d31c32d35fae6422f` was accepted. The attempt is consumed. No runtime result has been observed yet.


Adjacent-pairs intermediate photo 18f2ebb6 (SHA-256 `f4d3e8b26ac1d41b51ed3a2bd624d8dd61574e6cb43020bab30c1a336244540e`) shows a 326 ms receive phase, with 62 ms observation, 62 ms descriptors, 21 ms FIFO reads and 179 ms other I/O, summing to 324 ms after rounding. Barrier time is a nested 142 ms, with 54 ms per retained-domain check. Visible publication subtotals are 84 ms other I/O, 69 ms barriers and 26 ms per domain check. Progress text covers publication elapsed and its first three broad categories. ELF completion is 2,970 ms versus 2,813 ms in the preceding run, so the shorter receive interval is not an end-to-end improvement. Changed work mix and scheduling can affect these request-scoped intervals. IPA is not complete; query phase 2 is waiting for IPA, with unavailable PD state FFFFFFFF, no WLAN-image reads and no WLFW servers. Do not interpret this frame as the settled query result.


The later complete adjacent-pairs photo fa8da092 (SHA-256 `bfed2aac825e787d3210e893fa7543e71feec5cb420cc899a0035b0dd5062f68`) shows IPA completion and a valid UNINIT query response, with no WLAN-image reads or WLFW server. Receive remains 326 ms, versus 2,017 ms in the preceding boot. Queue-to-publication is 214 ms, versus 662 ms. Its broad I/O categories are 32 ms observation, 32 ms descriptors, 18 ms FIFO reads and 84 ms other, summing to 166 ms, with roughly 48 ms outside those rounded counters. Nested barriers take 69 ms, with 26 ms per retained-domain check. ELF total remains 2,970 ms, 157 ms longer than the prior 2,813 ms. Receive plus publication is 540 ms, leaving approximately 2,430 ms across the other existing ELF phases. The full six-phase display already measures pass-to-registry, registry-to-selection, enqueue and publication-to-matching-RXDONE, but the I/O overlay hides them. A follow-up should reveal those existing intervals with the adjacent-pairs candidate retained, rather than assume the remainder is in the domain guard or alter another timing policy. These single-boot comparisons also include work-mix and scheduling variation. No runtime error is shown by this success-only painter.


The six-phase follow-up adds `sunfish-wlan-elf-six-phase-display`, which depends on the current phase-I/O feature and leaves all its capture hooks active. Three display conditions suppress the later registry-gap, gap-I/O and phase-I/O overlays so the existing ELF REPLY PHASES painter remains visible. Transport code, service order, firmware inputs, adjacent-pairs optimization and counter captures are unchanged from the preceding image. The six existing intervals cover request-to-receive-pass-end, pass-end-to-registry-entry, registry-entry-to-selection, selection-to-enqueue, enqueue-to-publication and publication-to-matching-GLINK-RXDONE. Whole-boot work mix and display work can still affect the comparison.

Both 469-test host configurations, the 20 timing fixtures, source audit, 29 source regression tests and boot layout pass. The inherited adjacent-boundary mutation report remains source-identical for the unchanged FIFO implementation. Both target builds passed, and the capability audit rejected all 280 mutations. The image is frozen as `native-wlan-elf-six-phase-frozen-v1`, SHA-256 `29632e04e6ba39e7c373dce727b2f9cca9393022ea004ab3834ed5d40d5dea3e`. Its manifest pins the source, tests, build inputs and artifacts, and records the source-identical inherited boundary-mutation report. The one-shot attempt is unused. No device action has been taken during this preparation.


After the user confirmed cold Fastboot, all six-phase frozen artifact/source/build/test hashes and product sunfish were verified. Exactly one temporary boot of `29632e04e6ba39e7c373dce727b2f9cca9393022ea004ab3834ed5d40d5dea3e` was accepted. The attempt is consumed. The runtime result is not yet observed.


Six-phase photo c7fd3b16 (SHA-256 `a15cce0e031a7d1995983ed81adefe23c5d5dc3b83736b10791749e9e5a4bb3d`) reports receive 233 ms, pass-to-registry 0 ms, registry-to-ELF-selection 2,387 ms, enqueue 34 ms, publication 460 ms and observed matching RXDONE 72 ms. Rounded phases sum to 3,186 ms versus total 3,188 ms. The largest gap between registry entries is 2,364 ms, with one recorded non-ELF preselection dispatch attempt. Its transaction, name and outcome are not displayed in this frame. There are zero ELF TX waits and zero recorded TX4 same-peer waits. About 74.9 percent of total latency occurs before selecting ELF after registry entry. The source processes one LOC request per service_pass, then storage, IPA and WLAN-query/listener work, and another bounded router dispatch before the following registry pass; this frame does not attribute the intervening time separately. Next test a bounded additional registry dispatch against same-peer flow/backpressure rules offline before proposing a scheduling candidate. Do not assume merely moving the selection timestamp earlier will shorten actual queue/publication/completion. IPA completed, WLAN UNINIT, zero WLAN-image reads or WLFW servers.


The immediate-second-turn host fixture uses the actual owner and FIFO implementation with two received LOC requests. A different-peer ELF reply queues immediately after the first registry reply. A same-peer ELF reply returns WaitingReply both before and after first publication, then queues only after that reply's matching completion. The selected timestamp can move earlier without queueing; the test explicitly distinguishes those outcomes. This fixture passed as part of a 470-test suite.

The candidate `sunfish-servreg-bounded-second-turn` adds one ordinary registry dispatch and at most one additional dispatch only after ReplyQueued while the exact retained ELF handle is still in the pending-request queue. It uses actual handle/queue state rather than diagnostic clocks or timestamps. It preserves the existing request selector, same-peer FlowBusy rule, transmit-slot limits, reference credit, completion receipts and fatal-error behavior. The older `sunfish-wlan-registry-second-reply` credit/window experiment remains disabled. Storage, IPA and WLAN-query/listener service calls remain after the bounded registry step in the same service_pass. One extra dispatch can delay them; the test is not an unbounded drain or a retry loop.

The six-phase display is retained. REG EXTRA TRY counts actual extra calls; REG EXTRA OUT latches their last result: 0 none, 1 idle, 2 progress, 3 a registry reply queued, 4 waiting/backpressure. A queued extra reply need not be ELF if another registry request precedes it. ELF PEER PORT identifies the first ELF request; LAST WAIT WHY and LAST PEER FLAGS retain the ELF queue-pressure snapshot, not necessarily a current blocker. A smaller registry-selection interval is insufficient; actual enqueue, publication and matching completion must be compared.

Enabled host tests passed 471 cases and the disabled run passed 470, each with one prior ignored test. Tests cover two-call limits with a third request still pending, same-peer waiting without repeated scheduling, stopping the extra-turn scope when ELF is queued, independence from an absent diagnostic clock, and terminal faults allowing no additional access. The 20 timing fixtures, 29 source regression tests, source audit and both AArch64 builds passed. The three scheduling mutations were rejected, and the capability audit rejected all 280 mutations. Boot layout and the LZ4 round trip passed. The image was frozen as `native-servreg-second-turn-frozen-v1` with SHA-256 `1a5623848daf18d22d00f2eb259257abae424964e0395d461350c40fc3609cf6`.

After cold Fastboot was confirmed, the frozen hashes and Pixel product `sunfish` were verified. Fastboot accepted the single temporary boot. The attempt is consumed. The runtime result is pending.


Bounded second-turn photo 6067e3a7 (SHA-256 `3a5c55d73935e1b6ddcc1495be10f62d5fb2ffcccf017a69130f7211e1105b35`). The one extra registry dispatch selected the ELF request and returned WaitingReply. The ELF queue-pressure reason was the same-destination transmit guard. The matching slot snapshot has qrtr_done set, with started, fully published and glink_done clear. TX4 SAME PEER is a separate counter. Selection moved from 2387 to 80 ms after registry entry while enqueue wait grew from 34 to 2398 ms. The reply took 3092 ms end to end, 96 ms less than the previous single boot. Publication and matching RXDONE were unchanged at 460 and 72 ms. IPA completed, the WLAN query returned UNINIT and no WLAN-image request or WLFW service was observed. This points to measuring or shortening the time to publish and complete the preceding same-peer reply under the existing guard.


The existing registry priority only runs inside a router dispatch. It can yield to TFTP, intent ACKs, QRTR credits and the registry reply's own receive completion. The extra registry dispatch itself only stages a reply or observes backpressure. It does not publish to GLINK. A single offline comparison used the actual IRQ Router, Owner and FIFO with a modeled peer and clock. The same scheduled router turn published the exact registry blocker at either the earlier or later modeled time. A valid RXDONE for a different peer with the same QMI transaction left the blocker pending. The matching completion admitted ELF and each modeled physical IRQ was acknowledged once. Moving the modeled turn moved admission by the same synthetic interval. This establishes a feasible ordering under available transport resources, without predicting a phone speedup.

The optional `sunfish-wlan-elf-blocker-timing` image adds observations to the bounded second-turn profile. It captures the first unpublished registry reply that triggers ELF's same-destination guard. The opaque request handle binds later full-publication and GLINK completion observations to the exact reply, including when another peer uses the same QMI transaction. BLOCK TXN and BLOCK FLAGS describe the initial snapshot. BLOCK TURN MS spans wait detection to the next owner work entry. BLOCK PUB MS spans wait detection to full publication and includes BLOCK TURN MS. BLOCK DONE MS spans publication to observed matching GLINK RXDONE. DONE TO ELF MS spans that completion to ELF admission. FFFFFFFF indicates an unavailable endpoint. These are host observations and the extra clock reads can affect them. The image tests the existing scheduling while the earlier ordering is exercised offline.

The host suites passed 472 and 471 tests with the timing feature enabled and disabled, with one prior ignored test per suite. Runtime source audit, all 29 runtime source regressions, both AArch64 builds and boot layout passed. Four owner mutations were rejected, including accepting an unrelated completion as the exact blocker's completion. The inherited three adjacent-boundary mutation results match their unchanged FIFO source. The capability audit rejected all 280 mutations, and the LZ4 round trip passed. The image is frozen as `native-elf-blocker-timing-frozen-v1` with SHA-256 `7e37dc5910ac7cd2a3a0f4d4c328399e91902690495723f65c315ca0439399c4`. The one-shot attempt is available. Device timing is pending.


The user confirmed cold Fastboot. The frozen image and inputs were verified, the Pixel identified as sunfish, and Fastboot accepted the one-shot ELF blocker timing boot. The attempt is consumed. The settled device timing is pending.


ELF blocker timing photo c538d172 (SHA-256 `c7ca6b3c84c2d12598158a3a6cf806f1001a4a3ba46e44cf115eb6b3528b6b90`). Registry transaction 4 was the unpublished same-peer blocker. The first router turn began 69 ms after the wait was recorded, full publication was observed at 173 ms from that same origin, and matching GLINK RXDONE followed 72 ms later. The next 2,065 ms elapsed after the host observed completion and before ELF queued, about 88% of the 2,346 ms selection-to-enqueue interval. The displayed intervals leave approximately 36 ms between ELF selection and recording the first wait, including rounding. ELF completed in 3,443 ms. The screen does not attribute the post-completion delay among remaining router work, TFTP close or early service, and registry work. IPA completed, the WLAN state was UNINIT, and both WLAN firmware-read and WLFW counts were zero.

Offline, test a single bounded yield at the exact completion when ELF still waits on that reply. Finish the current checked transport iteration and preserve the existing continuation, IRQ ownership and service order. Compare the remaining owner pass and the time until ELF queue admission before choosing a device candidate.


The opt-in `sunfish-wlan-elf-completion-turn` candidate yields once when the validated RXDONE belongs to the exact previously observed registry blocker, the QRTR side was already satisfied, and the exact ELF request remains queued for service. It uses request and completion identity without a timing threshold. It finishes the current checked transport iteration, then leaves IRQ ownership and continuation scheduling on the normal budget-expiry path. The ordinary service pass still runs its existing TFTP, registry, RMTFS and IPA order. The earlier handoff can delay later FIFO records until that continuation. ELF goes through the normal queue and flow checks and its reply is published by a subsequent router turn.

The enabled and disabled host configurations each passed 472 tests with one prior ignored test. The actual Router/Owner/FIFO fixture used an unrelated completion with the same QMI transaction and a trailing advertisement. Both variants drained the unrelated case. On the matching completion the enabled variant retained the following advertisement, held the physical IRQ, ran the ordinary registry dispatch, then consumed the advertisement and rearmed the IRQ exactly once. The disabled variant drained the following advertisement before registry service. This proves ordering on a modeled peer. The phone will determine whether the post-completion interval drops from the preceding 2,065 ms observation. The screen reports ELF YIELDED, DONE YIELD MS, YIELD ELF MS and DONE TO ELF MS alongside the existing blocker and WLAN fields.

Both AArch64 builds, 29 runtime source regressions and the runtime source audit passed. The four owner mutations were rejected, including yielding for an unrelated completion and rearming before the continuation. The boot-layout audit and LZ4 round trip passed, and the capability audit rejected 280 mutations. The image is frozen as `native-elf-completion-turn-frozen-v1` with SHA-256 `2c4ae28adb4926d03c07e49aea746257f05306ff331dab355f95efc2458b45e8`. The one-shot attempt is available. Device timing is pending.


After cold Fastboot was confirmed, the frozen inputs and image were verified and the Pixel identified as sunfish. Fastboot accepted the one-shot ELF completion turn boot. The attempt is consumed. Device timing is pending.


ELF completion turn photo fc92d077 (SHA-256 `0b1843eb7ebbb7d7b126cd07f941cf88f0f3460fb54905ca4b11cfd13862ea93`). The exact completion handoff ran once. The blocking registry transaction was 4. Completion to end-of-iteration yield was 81 ms, followed by 57 ms to ELF admission, summing to the reported 138 ms DONE TO ELF interval. This interval fell from the preceding 2065 ms observation by 1927 ms, approximately 93.3 percent. ELF end-to-end completion fell from 3443 to 1515 ms, approximately 56.0 percent in this single-boot comparison. Selection-to-enqueue fell from 2346 to 573 ms; ELF publication was effectively unchanged at 368 versus 369 ms, and matching RXDONE remained 72 ms. The blocker itself published 327 ms after initial wait versus 173 ms before, so work mix also varied. IPA completed, WLAN state remained UNINIT, and zero WLAN firmware-read requests or WLFW servers were observed. This confirms the intended ordering ran and coincides with removal of most of the measured post-completion scheduling gap. It does not establish acceptance of the locator reply by WLAN firmware or WLAN startup.


The user confirmed the WLAN fields stayed unchanged and is returning the Pixel to LineageOS. Verified the saved working Linux capture against its artifact hashes. Linux logged an offset-free kernel/elf_loader query and returned msm/modem/wlan_pd at instance 180. The pinned mapper and QMI descriptors define success, total count one, revision one, and one 17-byte domain name with a one-byte name length, u32 instance, u8 service-data flag and u32 zero service data. The native owner/FIFO fixture emits this 55-byte QMI response in an 88-byte padded QRTR packet.

The likely Linux request/reply sequence is 60-byte receive at 138.272783 s, 88-byte transmit at 138.273077 s, and matching GLINK RXDONE at 138.273157 s. The next inbound GLINK data records were 52 and 136 bytes. The TFTP socket received the first named wlanmdsp.mbn RRQ 691.852 microseconds after the candidate completion, from node 0 port 59 to node 1 port 16384. Its 102-byte TFTP message fits the 136-byte padded QRTR record. WLFW service 69 version 1 instance 0 appeared later at node 0 port 81. The Linux GLINK trace records lengths, intent IDs and ordering. The service identity and TFTP association are inferred from those fields and the independent named events. The Linux capture used locator port 16390 and TFTP port 16384. The native locator and TFTP ports are 16387 and 20480 respectively. Prior native server-check exchanges show the modem can discover the native TFTP port. The comparison leaves the WLAN startup trigger unresolved.

The exchange trace image keeps the measured completion handoff and the ordinary modem services. The owner supplies the exact first ELF request handle. The diagnostic reads the serialized reply from its existing transmit buffer and compares its QMI bytes with the source-derived Linux reply. It latches the matching GLINK completion and retains headers of the first four complete subsequent QRTR datagrams before routing. RX META encodes version in bits 31–24, packet type in bits 23–16 and actual padded length in bits 15–0. RX SRC PORT and RX DST PORT show the corresponding header ports. FFFFFFFF marks an unavailable packet slot. The capture retains no packet bodies. WLAN RRQ and WLFW counters continue across all events.

The enabled and disabled host suites each passed 472 tests with one prior ignored. The owner/FIFO fixture verifies the exact 88-byte serialized reply, exact matching completion, capture start and order, and the four-header limit. Both AArch64 builds and 29 source regressions passed. Boot layout and the LZ4 round trip passed. The capability audit rejected all 280 mutations. The image is frozen as `native-elf-exchange-trace-frozen-v1` with SHA-256 `c2b434e2397992ff298170adbc0c44778f2296b421f6b72e14ccc3158a43a740`. The one-shot attempt is available.


The user confirmed cold Fastboot. The frozen inputs and image were verified and the Pixel identified as sunfish. Fastboot accepted the one-shot ELF QRTR exchange boot. The attempt is consumed and the device result is pending.


IMG_9466 (SHA-256 `16815cf009b58ef8cebd3b163683941b8f752bc4aa38f4a50779fad29b779822`). The native transaction 5 reply is 88 padded bytes, with QMI bytes matching the source-derived Linux schema. It requests QRTR confirmation and received the matching GLINK RXDONE. Total ELF completion is 1516 ms and the post-blocker queue interval remains 138 ms. The first four complete QRTR packets after RXDONE are all 52-byte v1 NEW_SERVER announcements between control ports. The snapshot retains header fields only, so their advertised services and any later QRTR credit are unavailable. The working Linux trace had a 52-byte record of unknown type followed by a 136-byte record consistent with the named WLAN image request. Native IPA completed, WLAN was UNINIT, and both the WLAN RRQ and WLFW counters were zero.

A focused follow-up can retain validated RESUME_TX metadata for the ELF reply's destination across the whole publication and completion window, along with the first non-announcement QRTR packet and announced service IDs. A flow credit is per destination and cannot by itself prove that the modem accepted this QMI transaction.


The ELF QRTR CREDIT image observes validated RESUME_TX records for the destination named in the exact ELF reply. The pinned Linux code looks up that destination from the resume body. The native reset additionally requires the header source to equal that address and to be on the peer node. CREDIT PRE PUB, CREDIT PRE DONE and CREDIT AFT DONE count body matches from local reply admission onward, split at full publication and matching GLINK RXDONE. CREDIT APPLIED counts successful native flow resets. The first matching record supplies the source and target ports and its age from reply admission. These observations measure destination flow credit. The 52-byte packet in the working Linux trace has unknown type, and a native flow credit carries no QMI transaction identifier.

The image also shows the first complete DATA header and the first four validated NEW_SERVER service IDs and ports after the matching GLINK RXDONE. The first DATA header identifies a QRTR packet for follow-up without retaining its body. The diagnostic keeps the existing transport policy, completion handoff and RAM-only service profile.

The enabled and disabled host suites passed 473 and 472 tests, with one prior ignored per suite. The new flow fixture checks all three observation windows, an unrelated destination, a matching resume with a different header source, and the applied reset count. The actual owner/FIFO fixture checks that DATA and announcements remain observable beyond the original four-header window. The runtime source audit and 29 source regressions passed, as did 20 diagnostic fixture tests and both AArch64 builds.

The boot-layout audit and LZ4 round trip passed. The capability audit rejected all 280 mutations. The diagnostic is frozen as `native-elf-credit-trace-frozen-v1` with SHA-256 `8ac71ba925a61243bd3f561e0a1b1844794ce92408704e22822eede2775bff5e`.

The user confirmed cold Fastboot. The frozen image and pinned inputs were verified, the Pixel identified as sunfish, and Fastboot accepted the one-shot ELF QRTR credit boot. The attempt is consumed and the screen result is pending.


ELF QRTR CREDIT photo 6b0183b4 (SHA-256 `e214c7fa2b2a45d49b64f3621ebbb2bc0c099475d941c452e5ae4a8cff52e462`). The ELF reply was published, matched the reference QMI bytes, requested confirmation, and reached GLINK RXDONE. The trace saw one validated RESUME_TX whose body names the exact ELF destination, modem port 9. It arrived after RXDONE at 16,089 ms from local reply admission. Both header ports are the QRTR control port `0xfffffffe`. The native reference-flow reset requires the source address to equal the body key and therefore skipped this credit. The pinned Linux `qrtr_tx_resume` looks up the body key on the receiving endpoint. The first DATA header is QRTR v1, 108 bytes padded, from port `0x21` to the native TFTP port `0x5000`. Its payload was outside the trace. IPA completed, WLAN stayed UNINIT, and WLAN RRQ and WLFW counts were zero.

The next candidate should replay this control-port header and peer-port-9 body in the host fixture, then test a body-keyed reset for an existing live flow on the validated peer. Verify that credit restores the intended destination's send capacity when exhausted, preserves unrelated and closed flows, and stays independent of GLINK and per-reply completion. A later device comparison can check CREDIT APPLIED, then WLAN RRQ and WLFW. The credit and observed mismatch alone do not establish why WLAN stays UNINIT.


The candidate enables `sunfish-qrtr-control-port-resume` in the ELF QRTR CREDIT profile. After ordinary wire and node validation, it accepts the observed form with both header ports set to `0xfffffffe` and the body's node equal to the validated peer. It passes the body address to the bounded flow table, which resets only an existing live entry. The original exact-header form still works. The per-reply completion check still requires its original exact route, and GLINK buffer retirement still uses the matching RXDONE.

The ELF trace fixture replays the observed control-port form after the exact ELF completion. Only the enabled profile counts its reset. A separate session fixture sends real registry replies until ports 9 and 10 reach the transmit high watermark, then injects the observed packet. Port 9 can queue and complete another reply with the fix. Port 10 stays blocked, and the earlier outstanding reply still needs its own GLINK RXDONE. A mismatched header port and a body naming another node grant no credit to port 9. The ten flow-table tests also passed, including the closed-destination and outstanding-reservation checks.

Both runtime host configurations passed 474 tests with one prior ignored per configuration. The runtime source audit and 29 source regressions, 20 timing fixture tests, both AArch64 builds, and the boot-layout audit passed. The comparison screen retains CREDIT APPLIED, the credit stage counts, WLAN RRQ RX and WLFW SERVERS.

The capability audit rejected all 280 mutations and the LZ4 round trip passed. The image is frozen as `native-elf-credit-fix-frozen-v1` with SHA-256 `78dbcc926e3739db848797e6a00591144ffd090dc5ee21ad57f866a8e9b52f6d`. The one-shot attempt is available. Device verification is pending.

After the user confirmed cold Fastboot, the frozen inputs and image were verified and the Pixel identified as sunfish. Fastboot accepted the one-shot QRTR credit fix boot. The attempt is consumed and the screen result is pending.


ELF QRTR CREDIT fix photo 48594128 (SHA-256 `6eb1c1af828bccca455ffe29536a4326f202aff2afe0b219d6987b59764a19b0`). The fix applied one validated control-port RESUME_TX for the ELF destination at port 9. The credit arrived after the matching GLINK RXDONE, 16,427 ms from reply admission. Both header ports were `0xfffffffe` as before. The first subsequent DATA header again shows a 108-byte padded packet from port `0x21` to TFTP port `0x5000`. The query still reports WLAN UNINIT, with zero WLAN firmware read requests and zero WLFW servers. This run verifies the destination flow reset on the Pixel. WLAN startup did not advance in the observed run.


Follow-up comparison verified all 44 saved Linux capture artifacts. Between the candidate 60-byte ELF request and 88-byte reply, Linux sent another 52-byte packet at 138.272980 s, 97 microseconds before the reply. Its GLINK completion preceded the candidate ELF reply's completion by 5 microseconds. Pinned Linux `qrtr_recvmsg` sends RESUME_TX before returning a received datagram that requests confirmation. That fits the 52-byte packet's size and order, while the trace lacks QRTR type and route to identify it. Native `stage_resume_tx` encodes the corresponding form for a confirmed request. Its publication and completion relative to this particular ELF reply are outside the current trace.

For the next comparison, bind any outbound RESUME_TX to the exact incoming ELF request, then record the request's confirmation flag, the resume's publication and matching RXDONE, and their order relative to the ELF reply. Also classify the first post-ELF TFTP request by opcode and allowlisted path kind. The native 108-byte DATA packet has a body of 73–76 bytes after accounting for QRTR padding. Both the reference's 73-byte server-check WRQ and 74-byte modem_efs RRQ fit that size. The named first Linux WLAN image request has a 102-byte body and arrived 691.852 microseconds after the candidate ELF reply's GLINK completion. Length alone does not identify the native request or establish a WLAN startup dependency.


The opt-in ELF HOST CREDIT diagnostic associates the first accepted ELF request handle with the transmit slot staged by its existing confirmation path. It records the actual remote intent at full publication, the matching GLINK completion, and both flags as of full publication of the ELF reply. REQ CONFIRM reports the incoming request bit. The existing ELF CONFIRM row continues to report the reply bit. FFFFFFFF indicates an unavailable observation. There is no new credit, scheduling change, or request response. The previously verified inbound credit fix remains enabled.

After exact ELF reply completion the diagnostic latches the first RRQ or WRQ addressed to the advertised TFTP port. It stores opcode, body length, ports, and a fixed label for the known path or decoding result. It neither retains arbitrary path bytes nor uses the classification to authorize an operation. FIRST DATA remains the separate first post-completion QRTR DATA header and can refer to a different datagram.

The host fixture checks the serialized 52-byte resume, its distinct remote intent, and completion arriving after publication of the ELF reply. It verifies that pre-reply TFTP traffic and DATA do not latch the first request and that a later WRQ does not replace it. The existing owner fixture verifies the unconfirmed ELF case. The enabled and disabled host suites passed 475 and 474 tests respectively, with one prior ignored case each. Ten flow-table tests, 20 diagnostic fixture tests, the runtime source audit, and 29 source regressions passed.

Both AArch64 builds, boot layout, and the LZ4 round trip passed. The capability audit rejected all 280 mutations. The one-shot image is frozen as `native-elf-host-credit-frozen-v1` with SHA-256 `143e2c45cdc0a54182f30b034a4a44a9a2e4722a9453b47a3839ede86c85f426`. Device verification is pending.

After the user confirmed cold Fastboot, the frozen inputs and image were verified and the Pixel identified as sunfish. Fastboot accepted the one-shot ELF HOST CREDIT boot. The attempt is consumed and the screen result is pending.


ELF HOST CREDIT photo 33eb6908 (SHA-256 `df80ba27dcf8c28edb8e8ab9a28253ab90936d1ba2b31013874fece49cdf38fb`). ELF transaction 5 requested QRTR confirmation. Its host RESUME_TX was published using remote intent 0 and reached matching GLINK RXDONE before full publication of the ELF reply. The ELF reply also completed and the later inbound credit was applied. The first post-reply TFTP request was a WRQ for `/readwrite/server_check.txt`, opcode 2, with a 73-byte body from peer port `0x21` to local TFTP port `0x5000`. Its route and padded length match the first post-reply DATA header. WLAN query state was UNINIT with zero WLAN RRQs and WLFW servers. The screen captures request identity and ordering, with no observation of this WRQ's transfer outcome.


The working Linux socket capture identifies one server-check WRQ of 73 bytes from modem port 30 to service port 16384. Relative to WRQ receipt, the socket connected to the peer from transfer port 16391 at 383.645 microseconds, sent the 39-byte OACK at 467.864 microseconds, received 9 bytes of DATA at 844.583 microseconds, and sent the 4-byte ACK at 943.958 microseconds. This transaction predates the inferred Linux ELF reply RXDONE by about 1.58 seconds. The native observed server-check WRQ followed its ELF reply. Linux socket timestamps and native GLINK observations have distinct boundaries.

The opt-in ELF SERVER CHECK trace follows the first server-check WRQ after exact ELF reply completion. It records whether that request was dispatched, rejected, or coalesced. OACK and final ACK phases are 0 absent, 1 prepared, 2 queued, 3 published and 4 matching GLINK RXDONE. A coalesced WRQ can share a same-peer OACK. DATA is correlated by peer and allocated transfer port, and WRITE COMPLETE requires the existing RAM server to accept a final block and prepare its matching ACK. The native age rows use microseconds from the validated WRQ observation. FFFFFFFF denotes a missing observation. The trace changes no transfer behavior and retains no DATA content. First TFTP request classification and the WLAN request and WLFW counters remain on the screen.

The offline fixture runs the observed packet sizes through the actual RAM server and session, checks the response phases and timestamp endpoints, and verifies that DATA from another peer cannot advance the observed write. Both runtime host configurations passed 475 tests with one prior ignored each. The flow table and timing fixture suites passed 10 and 20 tests respectively. The runtime source audit and all 29 source regressions passed.

Both AArch64 builds, boot layout, and the LZ4 round trip passed. The capability audit rejected all 280 mutations. The one-shot image is frozen as `native-elf-server-check-frozen-v1` with SHA-256 `f8cd2f26f2bfae7508f015fcbe65f43d0f8a43db9bfde9c4a25e5edf79de4947`. Device verification is pending.

After the user confirmed cold Fastboot, the frozen inputs and image were verified and the Pixel identified as sunfish. Fastboot accepted the one-shot ELF SERVER CHECK boot. The attempt is consumed and the screen result is pending.


ELF SERVER CHECK photo 5c7d3ceb (SHA-256 `0bc46f884ca2ad3e8acf7081e795d9813fc96c481c51b34032fc45daa4e15903`). The 73-byte post-ELF server-check WRQ from modem port 0x21 was accepted without coalescing and assigned host transfer port 0x5002. The OACK published at 3,332,866 microseconds and reached matching GLINK RXDONE at 3,462,953 microseconds from WRQ observation, a 130,087-microsecond interval after publication. No matching DATA was observed on that route. WRITE COMPLETE and FINAL ACK PHASE stayed zero. IPA completed and inbound ELF credit was applied. WLAN query state remained UNINIT with zero WLAN RRQs and WLFW servers.

Linux recorded OACK socket send at 467.864 microseconds and final ACK socket send at 943.958 microseconds from its WRQ. Native publication and Linux socket send are different observation boundaries. The native WRQ passed the exact server options check, including timeoutms 1000. The OACK publication age exceeds that requested interval, but the trace does not establish when the peer starts the timer or whether it abandoned the request. The focused follow-up is to split WRQ-to-dispatch, dispatch-to-queue and queue-to-publication for this transfer, and correlate same-peer retries, close or DATA on a different destination port. This isolates the delayed stage before choosing a scheduling change.


The ELF SERVER TIMING diagnostic records DISPATCH US after owner selection and adapter revalidation, OACK READY US when the server's OACK has been retained, and OACK QUEUE US at successful admission to the GLINK transmit queue. The ages share the existing WRQ origin and OACK PUB US endpoint. A coalesced request can share a response and leave its own dispatch or preparation fields unavailable. WRQ REPEATS counts additional received datagrams from the same modem port to the advertised service for the server-check path. Internal retries of a deferred datagram do not increment this count. PEER CLOSES counts validated QRTR DEL_CLIENT controls naming that port. The first age and OACK phase are recorded before applying the close, including when its existing handling stops the runtime. The screen keeps the error code and detail in that case. Transfer handling is unchanged.

The offline fixture verifies separate dispatch, preparation and queue checkpoints, filters unrelated peer traffic, and checks first-repeat and first-close ages without losing the prior write and final ACK observations. Both host configurations passed 475 tests with one prior ignored each. Ten flow-table tests, 20 diagnostic fixture tests, the runtime source audit and all 29 source regressions passed.

Both AArch64 builds, boot layout, and the LZ4 round trip passed. The capability audit rejected all 280 mutations. The one-shot image is frozen as `native-elf-server-timing-frozen-v1` with SHA-256 `5fe88fe7e1f90f33ae1820560b657501c2b5913d6545d81f172c6a6eb236f3dc`.

The user confirmed cold Fastboot. The frozen inputs and image were verified and the Pixel identified as sunfish. Fastboot accepted the one-shot ELF SERVER TIMING boot. The attempt is consumed and the screen result is pending.


ELF SERVER TIMING photo 716734a4 (SHA-256 `b5a3c66df8e6554c9dc3d8f8e102ed040ff34df422626243ba009230ee2d2df5`). From the accepted WRQ, dispatch began at 92,572 microseconds, OACK preparation completed at 104,167 microseconds, and queue admission occurred at 138,876 microseconds. Publication occurred at 3,400,425 microseconds, so the queued interval was 3,261,549 microseconds, 95.9 percent of the measured time to publication. GLINK RXDONE followed at 3,530,858 microseconds. Zero same-port WRQ repeats were observed. The one validated QRTR DEL_CLIENT named modem port 0x21 and was observed at 3,934,244 microseconds, 403,386 microseconds after OACK RXDONE. CLOSE OACK PH was 4. No matching DATA arrived at transfer port 0x5002 in the observed window, and the write and final ACK remained incomplete. This isolates the elapsed interval after queue admission without identifying which publication condition held it.


The ELF OACK TX WAIT image samples the exact queued OACK's slot at the existing session publication calls. FIRST TX US is the interval from admission to the first session call. MAX TX GAP US measures the largest gap from the end observation of one call to entry of the next. These intervals include work outside the session such as the adapter's checked ring sample. TX ACTIVE US totals the measured time in session calls, TX WRITER US totals time in their writer callbacks, and OACK WRITE US is the writer subset for the selected OACK. The OACK age rows still use the WRQ origin.

The eligibility counts record whether each call had an available remote intent of the required size, a pending outbound intent request, or an unsent flow predecessor. Selection counts record the successful competing receive advertisements, intent ACKs, RXDONEs, TFTP and other data records, and outbound intent requests. OACK BLOCKED and OTHER BLOCKED count FIFO writer WouldBlock for the target and other selections. Idle counts reflect calls that selected no record. A condition counted at a call is a snapshot, so the screen does not assign unsampled gap time to that condition. The observer uses the existing remote-intent and flow-order predicates and makes no scheduling or wire changes.

The real session fixture consumes all suitable peer intents with other outstanding replies. It verifies the intent request and no-fit idle turn, a WouldBlock on the exact selected OACK, and later full publication and GLINK completion. A synthetic clock checks first-call, writer, and gap boundaries. Both host configurations passed 475 tests with one prior ignored each. Ten flow-table tests, 20 diagnostic fixture tests, the runtime source audit, and all 29 source regressions passed.

Both AArch64 builds, boot layout, and the LZ4 round trip passed. The capability audit rejected all 280 mutations. The one-shot image is frozen as `native-elf-oack-queue-frozen-v1` with SHA-256 `8f703695dc317d495b1246221f7481a34b8ecc1814ca739cbda97096deb1991c`. Device verification is pending.

Fastboot accepted that one-shot image on sunfish. The ELF OACK TX WAIT photo a3e06be7 records 3,192,131 microseconds from queue admission to the first session transmit call, 93.5 percent of the 3,414,488 microseconds to publication. The first call sent an intent ACK and the second sent the OACK. Neither call recorded missing intent capacity, an older flow predecessor, or a FIFO writer block. The gap between calls was 83,858 microseconds and the OACK writer took 69,242 microseconds. Matching GLINK RXDONE followed publication by 129,993 microseconds. The report recorded one peer close and zero completed writes. The transmit samples leave the work preceding the first session call unmeasured.

The opt-in ELF OACK EARLY TURN path gives the accepted, non-coalesced post-ELF OACK one existing router continuation immediately after queue admission, before the other service steps. The native router still performs the existing bounded owner pass with its receive, transmit and IRQ rules. An earlier turn can defer registry, RMTFS and IPA work. The display reports queue-to-router, router-to-first-TX and total early-turn time, whether first TX occurred in that turn, and the matching DATA and final ACK state. The modeled router and FIFO fixture exercised the early handoff, confirmed that an unrelated WRQ did not use it, and verified the OACK's destination, payload and the original IRQ rearm counts. The fixture exercises ordering with a synthetic clock.

The image is frozen as `native-elf-oack-router-turn-frozen-v1` with SHA-256 `636bbe8cbc056a5620aeb8cffd75d6c0ede64292680e7650d5cbe783b84127e9`. Host tests passed with the feature enabled and disabled (476 and 475, respectively, with one previously ignored test per run). Both AArch64 builds, source checks, boot layout, the LZ4 round trip and the capability audit with 280 rejected mutations passed. This image reuses the isolated native profile's build-copy exception for the close-handoff compile guard. The one temporary boot is unconsumed.

Fastboot accepted the early-turn image. Photo 865142c0 shows router entry 8 microseconds after queue admission and first transmit selection 95,640 microseconds after admission, down from 3,192,131. TX IN TURN was 1. The OACK published 396,506 microseconds after the WRQ and reached GLINK RXDONE at 434,344 microseconds. The peer closed once. Matching DATA and final ACK were absent, with zero WLAN RRQs and WLFW servers.

The ELF OACK PEER RX trace retains the early turn and samples reassembled QRTR datagrams from the tracked WRQ peer after OACK publication and before the first observed close. It counts DATA, ERROR and repeat server-check WRQs and latches each class's first destination and timestamp. DATA LEN BLOCK and ERROR LEN CODE pack the body length in the upper half and the block or error code in the lower half. The screen shows WRQ-relative ages, the allocated transfer port, and close age and OACK phase. This capture occurs before routing, so a packet on another destination port still appears in the report. The receive fixture checks wrong-port DATA, ERROR, other-peer exclusion, a repeated WRQ and the publication and close boundaries.

The image is frozen as `native-elf-oack-peer-trace-frozen-v1` with SHA-256 `08fd4baf3507902d1794a50e2a7125e1e77e3b21f54cb1c8bfe59d4e5aa419a9`. Both host configurations passed 476 tests with one prior ignored test each. Both target builds, the source and layout checks, the LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated native profile retains the documented build-copy exception for the close-handoff compile guard. The temporary boot is unconsumed.

Fastboot accepted the peer-trace image. Photo f625e04d shows the post-ELF WRQ from port 0x21 with transfer port 0x5002. The host observed OACK publication at 395,913 microseconds from the request, matching GLINK RXDONE at 526,168, and DEL_CLIENT at 775,624 with OACK phase 4. DATA, ERROR, and repeat counts were zero between publication and close. The pinned tqftpserv oracle passed a fresh byte comparison of the 39-byte OACK. The working Linux server-check sequence preceded the inferred ELF completion, so the post-ELF trace leaves the corresponding earlier window unobserved on native.

The TFTP FIRST WRQ candidate tracks the first server-check write as soon as the TFTP service receives it, including before ELF completion. It retains the existing post-ELF trace and keeps the early router turn on that post-ELF path. The first-write screen reports the WRQ result, PRE ELF, the selected transfer port, OACK and final ACK phases, DATA and ERROR metadata, close timing, and the separately observed post-ELF peer. A deferred re-entry retains its original WRQ observation. The integrated host fixture completes one pre-ELF write through final ACK RXDONE, then checks that a different peer's post-ELF OACK takes the existing early router turn and leaves the earlier result intact.

The one-shot image is frozen as `native-tftp-first-server-check-frozen-v1` with SHA-256 `187b069eeacb3346f5fbb098e873c73f10fc03610520db34e5d8114369114824`. Both host configurations passed 476 tests with one prior ignored test each. Both AArch64 builds, source checks, boot layout, the LZ4 round trip and the capability audit with 280 rejected mutations passed. The build uses the prior isolated runtime profile and its documented build-copy exception for the close-handoff compile guard. The temporary boot remains unconsumed.

Fastboot accepted the first-WRQ image. IMG_9467 shows the pre-ELF WRQ from 0x1f on transfer port 0x5001. The host observed OACK queue admission at 138,934 microseconds from the request, publication at 361,995, DEL_CLIENT at 1,766,804 with OACK phase 3, and GLINK RXDONE at 2,708,892. One repeat WRQ was recorded, with zero DATA and ERROR counts and no final ACK. ELF completed at 15,436,213 microseconds. The post-ELF trace records a different peer, 0x21, with OACK phase 4.

The TFTP FIRST TAIL candidate arms on successful publication of the first server-check OACK's last GLINK record and saves the distance to that FIFO head. Existing validated cursor samples account for bytes consumed through the boundary. The first crossing latches PEER TAIL US relative to the WRQ, and TAIL BYTES LEFT includes any older FIFO records still ahead of that boundary. REPEAT AGE US and CLOSE AGE US use the same origin. The peer tail measures observed transport progress. The post-ELF early turn stays enabled.

The one-shot image is frozen as `native-tftp-first-tail-frozen-v1` with SHA-256 `a3813ac5d16b221a9d37f88599eca0fbb549d4828cfa93f7abcb32b37765024c`. The offline fixture verified partial cursor advancement, exact crossing with subsequent traffic still in the FIFO, and a distinct later GLINK RXDONE. Both host profiles passed 476 tests with one prior ignored test each. Both target builds, source checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. This image uses the prior isolated runtime profile and its documented build-copy exception for the close-handoff compile guard. The temporary boot is unconsumed.

Fastboot accepted the first-tail image. IMG_9468 shows a pre-ELF server-check WRQ from peer port 0x1f on transfer port 0x5001. With ages measured from the original WRQ, the OACK was queued at 139,043 microseconds and published at 362,276. The peer FIFO tail crossed the end of the OACK at 373,866. The same peer repeated the WRQ at 1,494,900 and closed at 1,768,085 with the original OACK still at phase 3. Its matching GLINK RXDONE was observed at 2,710,820. The trace recorded zero DATA and ERROR packets from that peer before close, and no completed write.

The TFTP WRQ REPEAT diagnostic follows the first same-peer repeat by its receive handle, from coalescing or server dispatch through the queue outcome, publication and matching GLINK completion. It also records the repeat OACK phase at peer close. An offline owner fixture reproduced the ordering of the original FIFO-tail crossing, repeat WRQ and late RXDONE. The repeat reached dispatch, prepared an OACK and encountered the existing same-peer FlowBusy check. The repeat was then admitted, published and completed after the original RXDONE. A separate fixture exercised exact coalescing while the original response was still retained by the owner.

The one-shot image is frozen as `native-tftp-wrq-repeat-frozen-v1` with SHA-256 `ff8e382709ee5bf96db43af7b9670ed09eca3b160db21dd9483e81439d496a00`. Both host profiles passed 476 tests with one prior ignored test each. Both target builds, source checks, boot layout, the LZ4 round trip and the capability audit with 280 rejected mutations passed. The build reuses the isolated profile's close-handoff compile-guard exception. All 24 frozen artifact hashes were verified. The temporary boot is unconsumed.

After the user confirmed cold Fastboot, the frozen image and input hashes were verified and the same Pixel enumerated as sunfish. Fastboot accepted the one temporary TFTP WRQ REPEAT boot. The attempt is consumed and the diagnostic screen result is pending.

TFTP WRQ REPEAT photo b80ad127 (SHA-256 `9859df993d60bc9a429df4701580c8a5243c894d7b8175a59532ecf40af44172`). The first pre-ELF WRQ was from port 0x1f and used transfer port 0x5001. Ages in microseconds from that WRQ were 361,147 for original OACK publication, 372,700 for the peer tail crossing, 1,490,343 for the repeat WRQ, 1,582,815 for repeat dispatch, 1,762,842 for the observed peer close, and 2,702,727 for the original GLINK RXDONE. The repeat dispatched successfully without coalescing and hit FlowBusy twice, with zero NoCapacity returns. At close the original OACK was at phase 3 and the repeat was at phase 1. The repeat reached queue admission at 2,841,305, publication at 3,397,057 and RXDONE at 4,147,357. The final phase 4 therefore represents transport completion after the peer close. DATA COUNT, WRITE COMPLETE, FINAL ACK PHASE, WLAN RRQ RX and WLFW SERVERS stayed at zero.

The repeat reached dispatch with about 180 ms left before the observed close. Test offline whether one exact server-check OACK retransmission can get an immediate bounded transmit turn while the original still owns its GLINK intent. Preserve the transfer port, independent GLINK completion for each publication, and normal QRTR credit and FIFO checks. Exercise cancellation of any unpublished retry when the peer closes. The original FIFO tail crossed before the repeat, so the trace establishes the local retry delay while the modem's decision to repeat the original request remains unresolved.

The TFTP OACK RETRY candidate admits one byte-identical repeat of the first pre-ELF server-check WRQ alongside its original published OACK after the peer FIFO tail passes the original. Both datagrams use the existing transfer port, independent QRTR reservations and independent GLINK completions. One ordinary bounded router turn follows admission. A matching close cancels the tracked unpublished retry in the owner or the GLINK queue. RETRY FLAGS bits 0–3 report overlapping admission, early router dispatch, cancellation and a pre-ELF original, respectively.

The offline fixture published the repeat before the original RXDONE, then completed the repeat first while the original still owned its intent. The close fixture covered an unpublished retry retained by the owner and one admitted to the GLINK queue. Both host suites passed (477 with the retry enabled and 476 without it, with one prior ignored test each). Both target builds, source checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The frozen image uses the isolated build profile's close-handoff compile-guard exception and has SHA-256 `ad0f406a5894d51eff70feb30345f52753f31e8fa712849bfac032bfe95ec88a`.

After cold Fastboot confirmation, the artifact, source and input hashes were reverified and the same Pixel identified as sunfish. Fastboot accepted the one temporary TFTP OACK RETRY boot. The attempt is consumed and the diagnostic screen result is pending.

TFTP OACK RETRY photo 7f877446 (SHA-256 `e2bb3baa7978e795cdedbc90594473d664101297e4d5eff727368e9739913518`). RETRY FLAGS 0x0f records overlapping admission, early router dispatch, cancellation and a pre-ELF original. Relative to the original WRQ, its OACK published at 361,071 microseconds and the peer tail crossed at 372,610. The repeat arrived at 1,490,039, dispatched at 1,582,390 and queued at 1,628,569 without FlowBusy or NoCapacity. The peer close was observed at 1,681,547, 52,978 microseconds after admission, with the original OACK at phase 3 and the repeat at phase 2. Repeat publication and RXDONE were absent. The original reached RXDONE at 2,586,550. DATA, write completion, final ACK, WLAN RRQ and WLFW remained zero.

The next offline check should replay the early turn with the close already at the head of receive, a competing control record, and no free fitting peer intent. The current screen shows that the router was entered and that it canceled the queued reply, while the owner polls receive before the transmit selection. It gives no per-turn publish attempt or selection record for the retry. Reuse the OACK publication diagnostics for the tracked retry to distinguish close-before-first-selection, competing traffic and actual FIFO or intent backpressure. The original FIFO-tail observation still leaves the modem's reason for repeating the original request unresolved.

The TFTP RETRY TX diagnostic records the first owner turn, the end of its first checked receive poll and the first GLINK publish call that produced a selection outcome while the tracked OACK was queued. RPT TX CALLS counts observed selection outcomes. RPT TX FIRST and LAST encode the selection in the low byte: 1 idle, 2 outbound intent request, 3 receive advertisement, 4 intent acknowledgement, 5 receive done, 6 tracked retry, 7 other TFTP, and 8 other data. Bits 8 through 12 respectively indicate no fitting remote intent for the retry, a pending outbound intent request, an older unsent flow entry, writer WouldBlock, and uncertain publication. RPT TX SEEN uses bits 0 through 7 for the corresponding selection classes and accumulates the same condition bits. FFFFFFFF denotes an unavailable timestamp or selection. The instrumentation adds clock reads and in-memory observations to the checked owner and writer paths.

The synthetic owner and router replay modeled the 52,978 microseconds between retry admission and observed close. A close at the head of receive canceled with zero retry selection outcomes. A GLINK intent denial ACK ahead of close produced selection 4, then cancellation. The no-fit run held the original on the only sufficiently large remote intent, observed the no-fit bit without a retry selection, and canceled at close. The previously tested independent completions and owner-side and queue-side cancellation checks also passed.

The trace-enabled and baseline host suites passed 478 and 477 tests respectively, with one prior ignored test in each. Both target builds, source checks, boot layout, the LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the close-handoff compile-guard exception. The frozen image SHA-256 is `3306b392bb70f65d02f032727f012a3e1a13903c9d9574bbd013275814fb91ae`.

After cold Fastboot confirmation, the artifact, source and input hashes were reverified and the same Pixel identified as sunfish. Fastboot accepted the one temporary TFTP RETRY TX boot. The attempt is consumed and the diagnostic screen result is pending.

TFTP RETRY TX photo 08c9133f (SHA-256 `e65b69affe7e584bc803a9fae8fcb46709bdc8fbd716f7c57a6ad715c8993b20`). Times are relative to the first WRQ. The original OACK published at 361,973 microseconds and the observed peer tail crossed its boundary at 373,552. The same peer retried at 1,493,669. The repeat queued at 1,632,561 and the owner entered the early turn 10 microseconds later. The close was observed at 1,685,639 during the first receive poll, which ended at 1,720,370. The repeat was at phase 2 at close and RETRY FLAGS 0x0f records cancellation. RPT TX CALLS and RPT TX SEEN are zero, and all retry selection and publication timestamps are absent. The original matching RXDONE was observed later at 2,592,911. DATA, write completion, final ACK, WLAN RRQ and WLFW stayed at zero.

This matches the close-first replay. The timestamps record when the host observed the close and do not date its arrival in the FIFO. For the next offline comparison, give the original pre-ELF OACK the same bounded immediate router turn already used for post-ELF OACKs. Preserve receive-before-transmit and close handling. Compare first OACK publication and the peer's first DATA against the previous hardware run. Earlier service targets the exchange while the peer is active and leaves the cause of its original retry to be determined by that comparison.

The TFTP FIRST OACK candidate uses the existing server-check router dispatch when the first pre-ELF WRQ succeeded and its original OACK is queued. It gives that exchange one earlier turn while a continuation is pending and the peer has no observed close. The normal owner still receives before selecting transmit work and applies its usual validation, IRQ and close handling. The post-ELF OACK and the retry retain their own turn conditions. The screen records original queue, turn, publication, matched DATA and final ACK timing or state.

The host integration fixture published the original OACK inside the immediate turn, confirmed the same transfer port and payload, and completed the simulated DATA and final ACK exchange. The post-ELF OACK received its own turn, with the original IRQ acknowledgement counts preserved. Both feature profiles passed 478 tests with one prior ignored test each. Synthetic timing establishes ordering. The Pixel comparison will measure the change from the prior 361,973-microsecond OACK publication and whether the peer returns DATA.

Both target builds, source checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the close-handoff compile-guard exception. The frozen image SHA-256 is `2717875fb6c06645109de3e333018fc82ff98e1ec1a2a16f38712d78ff31e1b0`. All 24 frozen artifact hashes were verified. The temporary boot remains unconsumed.

After readiness confirmation, the frozen artifact, source and input hashes were reverified and the same Pixel identified as sunfish. Fastboot accepted the one temporary TFTP FIRST OACK boot. The attempt is consumed and the diagnostic screen result is pending.

TFTP FIRST OACK photo 9d6f6992 (SHA-256 `c8b1a6fb61ed8d71d4aae82feae04a60442a2419d307c6411a9150a71c36b29a`). Times are relative to the first WRQ from peer port 0x1f, transfer port 0x5001. The original OACK queued at 138,893 microseconds, entered the immediate turn 20 microseconds later, and published at 304,070, 57,903 microseconds earlier than the prior run. The peer FIFO tail passed the published record at 315,644 and the original GLINK RXDONE arrived at 2,627,311. The repeat arrived at 1,435,580, 69,457 microseconds before the original router turn returned. The peer close was observed at 1,650,679 with the repeat at phase 0, before OACK preparation. RETRY FLAGS 0x0c records a pre-ELF original and cancellation. Repeat admission, publication and completion stayed unavailable. The first peer produced zero DATA and ERROR observations, with no completed write or final ACK. WLAN RRQ and WLFW remained zero.

This comparison demonstrates earlier original OACK publication. It still leaves the first peer's TFTP acceptance unresolved. The first-peer overlay hides the aggregate server-check result. IMG_9438 previously showed a completed write and matching final ACK GLINK completion on a later peer. Before spending another boot on first-peer timing alone, use the existing aggregate write count, final ACK trace and WLAN state alongside the first-peer measurements to see how far the current profile gets on later transfers.

The TFTP WLAN PROGRESS screen shows original OACK timing, FIRST DATA RX, FIRST WR DONE and FIRST ACK PHASE alongside WRITES COMPLETE for all server-check transfers. FINAL ACK FLAGS and the two ACK port fields follow the first completed write. Flags 0x1f include the matching GLINK completion. PD QUERY PHASE and PD QUERY STATE report the WLAN domain query outcome beside the WLAN RRQ and WLFW counts. The overlay reads the existing software state.

Both host profiles passed 478 tests each with one prior ignored test. Both target builds, source checks, display label checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the close-handoff compile-guard exception. The frozen image SHA-256 is `93a40262e412859b7aa55294cf229e1e351a173d3a55d135e40cd1d68c974de6`. All 24 artifact hashes were verified and the temporary boot is available.

After readiness confirmation, the artifact, source and input hashes were reverified and the same Pixel identified as sunfish. Fastboot accepted the one temporary TFTP WLAN PROGRESS boot. The attempt is consumed and the diagnostic screen result is pending.

TFTP WLAN PROGRESS photo 96742717 (SHA-256 `46de944a25b830ab594b06004455b6e398394f0fb437cf3358b588419305b2b2`). The first transfer from modem port 0x1f to host transfer port 0x5001 reported zero DATA, write completion and ACK phase. Relative to the original WRQ, its OACK queued at 138,889 microseconds and published at 304,105, with the peer tail crossing at 315,686 and matching GLINK completion at 2,627,720. A repeat was observed at 1,435,917, followed by close at 1,651,019. RETRY FLAGS was 0x0c.

WRITES COMPLETE was 1. The final ACK was for a different transfer, modem peer port 0x59 and host transfer port 0x5010. Flags 0x1f include publication and matching GLINK RX_DONE. The WLAN domain query returned a valid UNINIT state (phase 4, state 0x7fffffff). The WLAN firmware read and WLFW service counts were both zero. This boot confirms a later server-check transfer can complete while WLAN discovery remains at UNINIT. The next comparison can correlate the completed write with the existing exact kernel/elf_loader reply timing and the first observable divergence before the firmware request in the saved working Linux capture.

The saved working Linux trace shows the first server-check final ACK sent at the TFTP socket 943.958 microseconds after the WRQ and 1.582305 seconds before the candidate ELF reply's GLINK RX_DONE. The first named wlanmdsp.mbn RRQ appeared at the TFTP socket 691.852 microseconds after that candidate completion. The ELF association follows the captured packet lengths, order and mapper log. Linux TFTP socket timestamps and native GLINK observations mark different boundaries.

The ELF WLAN TIMELINE image records the first completed write's final DATA acceptance, ACK publication and matching GLINK completion alongside the ELF request, reply publication and completion. It also shows the first classified TFTP request after the ELF exchange, the first WLAN firmware RRQ and exact WLFW advertisement, and the WLAN domain query state. The event timestamps share a runtime origin. ELF ROUND US measures request to GLINK completion and FFFFFFFF indicates an unavailable value. The fixture verified ACK completion before the synthetic ELF request and the subsequent WLAN timestamps, including their stability on repeated packets.

The image is frozen as `native-elf-wlan-timeline-frozen-v1` with SHA-256 `9abe6323058ce3c4dc1a6e15af8ae86db6b66db3a651821839de20e4079a2b36`. Both host profiles passed 478 tests, with one prior ignored test each. Both target builds, source checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the documented close-handoff compile-guard exception. All 24 frozen artifact hashes were verified and the temporary boot is unconsumed.

After readiness confirmation, the frozen artifact, source and input hashes were reverified and the same Pixel identified as sunfish in bootloader Fastboot. Fastboot accepted the one temporary ELF WLAN TIMELINE boot. The attempt is consumed and the diagnostic screen result is pending.

ELF WLAN TIMELINE photo 3b368543 (SHA-256 `0d078670161bbed18ab914dba318166e088705de80d9e8479428254dd06deed4`) is the interim frame. It has zero completed writes and query phase 2, which waits for IPA. The storage progress paint covers the ACK endpoint and post-ELF opcode and peer rows. Photo 725a5abf (SHA-256 `997e2262015488d61b2c6b5b78918d23ad0ce9c707702e2541a366113a181a1f`) shows the same ELF and first WRQ/OACK timestamps with later state.

In the later frame, the first WRQ arrived at 19,728,800 microseconds from runtime start. Its OACK published 304,108 microseconds later, and the first peer yielded zero DATA. The ELF reply for transaction 5 matched the reference QMI encoding. Request, publication and GLINK RX_DONE were observed at 33,940,714, 36,151,140 and 36,223,763 microseconds. The first classified TFTP request after the ELF reply was a server-check WRQ from port 0x21. The one completed write was from another peer, port 0x57, to transfer port 0x500d. Final DATA acceptance, ACK publication and matching GLINK RX_DONE were observed at 74,598,969, 74,949,450 and 75,079,973 microseconds. Final ACK flags were 0x1f. ACK publication followed ELF completion by 38.726 seconds and ACK GLINK completion followed it by 38.856 seconds.

The later frame reports runtime error zero, a successful WLAN query (phase 4) returning UNINIT, and zero WLAN RRQs and WLFW servers. In the working Linux capture, the first server-check write completed before the candidate ELF GLINK completion. The native run's first completed write came well after it. This establishes a difference in the host-observed sequence, with the Linux ACK recorded at the TFTP socket and native ACK publication recorded at GLINK. The next comparison should focus on why the first native server-check yields no DATA, using the existing exact OACK payload comparison, the roughly 304-millisecond native publication interval, and the later successful native transfer as controls. The ordering alone leaves the modem's WLAN startup dependency unresolved.

Offline comparison of the first server-check exchange. Both saved Linux captures show the same peer send a 73-byte WRQ and a nine-byte DATA packet, with the 39-byte OACK sent from a separate transfer socket. OACK socket send return followed WRQ socket receive by 467.864 microseconds in the ELF locator reference and 476.042 microseconds in the earlier GLINK/TFTP reference. Same-peer DATA socket receive followed OACK send return by 376.719 and 459.271 microseconds, respectively. Correlating the GLINK records by size, direction, time and intent ID puts the host's WRQ RX_DONE with reuse before OACK in the ELF locator capture and after OACK in the earlier capture. Both exchanges completed.

The pinned tqftpserv oracle again matched all 39 native OACK payload bytes. Two temporary host-only replays using the full timeline feature set emitted a 72-byte padded QRTR OACK, with outgoing confirmation clear. In the first and post-ELF cases with incoming confirmation requested, the model emitted the WRQ's one-shot GLINK RX_DONE, OACK, then the matching QRTR RESUME_TX. With incoming confirmation clear it emitted RX_DONE and OACK, with no RESUME_TX for that WRQ. Injecting matching DATA before the peer's OACK RX_DONE still completed the RAM write locally. Its final ACK was retained while the prior transmit was outstanding and queued after matching OACK RX_DONE. This exercises transport and host state only. The successful native hardware transfer from port 0x57 has a final ACK trace, while its original WRQ and OACK control ordering were outside that capture.

The Linux traces allow either WRQ RX_DONE/OACK order. Both returned the WRQ intent with reuse. Native retires that intent and advertises new ones, and the offline DATA replay explicitly supplied an advertised buffer. The next device comparison should correlate the original failed transfer and the first completed transfer with the incoming WRQ confirmation flag, exact host RX_DONE and RESUME publication, and the number of already-advertised receive intents when each OACK publishes. Capture matching DATA, a retry or close on the same peer. This can distinguish lack of a buffer for DATA or delayed credit from a peer that had transport resources and still abandoned the exchange. The comparison leaves receive intent reuse, scheduling, packet formats and device state untouched.

The TFTP OACK COMPARE candidate follows the original accepted WRQ on each allocated server-check transfer port. Exact receive intent identity binds the WRQ's outgoing GLINK RX_DONE and any requested QRTR RESUME_TX. The original OACK transmit is tagged independently of retries. After successful publication, RX READY samples the count of locally advertised receive intents still available in host state. The modem may already have used an intent whose incoming FIFO record the host has yet to read. Ages use each transfer's own WRQ origin, and FFFFFFFF marks unavailable values. FIRST follows the earliest admitted WRQ by observed receive time. DONE follows the first server-check write accepted through its final DATA on the matching peer and local port. FINAL ACK FLAGS is displayed only when the inherited first final-ACK trace belongs to DONE. WLAN domain state and discovery counts remain visible.

The owner fixture held the first transfer incomplete, rejected wrong-port DATA from the second peer, then completed the second transfer on its own port and verified its distinct requested QRTR credit and OACK timestamps. The retry fixture verified separate repeat and close times while the original OACK completion was still outstanding. The instrumentation reads only existing local protocol state and the existing clock. The runtime can repaint when either displayed trace advances, which can affect timing.

TFTP OACK COMPARE is frozen as `native-tftp-oack-compare-frozen-v1` with SHA-256 `0256dbef636d7be2b64ad1eb823480c36d82e2dc0444219ca1a204136dea85e5`. Both host profiles passed 478 tests with one prior ignored test each. Both target builds, source checks, display label checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the close-handoff compile-guard exception. After cold Fastboot confirmation, all 24 artifacts, source and input hashes were verified and the same Pixel identified as sunfish in bootloader Fastboot. Fastboot accepted the one temporary boot. The attempt is consumed and the screen result is pending.

TFTP OACK COMPARE photo 360b549a (SHA-256 `1df2784501d84ac37cd6ee22aa844e4e76d161fa266dfee6cead4a882906602d`). Both original WRQs have CONFIRM=0 and no recorded QRTR RESUME. The first peer is 0x1f, transfer port 0x5001. Relative to its WRQ, the host published the OACK at 304,325 microseconds with one available receive intent in host state. The peer retried at 1,436,761, closed at 1,652,008, and the matching peer OACK RX_DONE was observed at 2,629,408. Matching DATA is absent. The host's separate RX_DONE for the incoming WRQ published at 81,778,245 microseconds, 80,126,237 microseconds after close.

The first completed peer is 0x58, transfer port 0x500e. Relative to that WRQ, OACK publication was at 455,115 microseconds with zero available receive intents in host state, peer OACK RX_DONE at 493,019, host WRQ RX_DONE publication at 608,862 and first matching DATA at 1,700,953. FINAL ACK FLAGS=0x1f and WRITES COMPLETE=1. Runtime error and detail are zero. WLAN query state is UNINIT and WLAN RRQ and WLFW counts are zero.

The host WRQ RX_DONE trace binds to the original receive intent, whose ID is allocated monotonically within the generation, and stamps after successful publication. It is distinct from the matching peer completion of the outgoing OACK. The first observed completion delay is a concrete candidate. The available-count sample describes a single host observation, and the recorded close leaves the modem's exact reason unresolved. At dispatch the host marks the original receive intent done-pending; the publication code can select advertisements, intent ACKs and prioritized data ahead of ordinary RX_DONE, and the ordinary scan favors the lowest receive slot. The next offline test should reproduce deferral of the exact first WRQ under competing traffic and test a bounded turn for its existing RX_DONE. Preserve genuine publication, FIFO backpressure and uncertainty handling. The success marker on a future boot is first-peer DATA and a final ACK before ELF completes, alongside WLAN RRQ and WLFW counts.

The baseline replay reproduced deferral using the production session. With the first WRQ released and its OACK queued, the writer published a fresh receive advertisement, the OACK, and an unrelated lower slot's RX_DONE before the first WRQ's RX_DONE. The replay exercises scheduling order. The 81.778-second delay remains a hardware observation.

TFTP WRQ RXDONE adds one opt-in selection for the pending RX_DONE of the first exact accepted pre-ELF server-check request. The record becomes eligible after dispatch has accepted the request and released its original receive intent. A preferred peer intent ACK and its backing advertisement retain precedence. The candidate goes through the existing checked writer and normal completion commit. WouldBlock retains the exact pending intent. Successful publication consumes it once. The existing uncertainty path quarantines the session. In the candidate replay, an intent denial ACK still went first, the first WRQ's exact RX_DONE was blocked and then published, and the competing advertisement, OACK and lower-slot completion also progressed.

The screen retains the two transfer timing groups and WLAN discovery fields. RXDONE PRIORITY records a successful priority publication for FIRST. ACK BEFORE ELF is 1 when the first transfer's final ACK reached matching GLINK completion before the ELF reply reached its completion, 0 when the opposite ordering is observed, and FFFFFFFF when the pair is unavailable. This image tests the timing hypothesis on the Pixel and does not establish modem application acceptance of either reply.

TFTP WRQ RXDONE is frozen as `native-tftp-wrq-rxdone-frozen-v1` with SHA-256 `da133ea0e288f40fa1607c66bcb8f213a43ea37de31a7d54474d971c6efaa3b4`. Both host profiles passed 479 tests with one prior ignored test each. Both target builds, source checks, label checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the close-handoff compile-guard exception. All 24 frozen artifacts were verified and the temporary boot is unconsumed.

After cold Fastboot confirmation, all 24 frozen artifacts, source and input hashes were reverified and the same Pixel identified as sunfish in bootloader Fastboot. Fastboot accepted the single temporary TFTP WRQ RXDONE boot. The attempt is consumed and the diagnostic result is pending.

TFTP WRQ RXDONE photo 5fa5dcc6 (SHA-256 `c0b44f473aceb50b445d6fd6d79edbde8267ec7a90059af7cd2239c836cc32c7`). RXDONE PRIORITY=1. For the first peer 0x1f on transfer port 0x5001, relative to the WRQ, host WRQ RX_DONE published at 304,117 microseconds and OACK at 457,727. The WRQ requested no QRTR confirmation and RX READY was 1 at OACK publication. The peer retried at 1,435,867 and closed at 1,650,969. The host observed the matching peer OACK RX_DONE at 2,627,674, 976,705 microseconds after observing close. The first peer produced no matching DATA and ACK BEFORE ELF is unavailable. The selected host receive completion moved from 81.778 seconds to 0.304 seconds. First-peer retry, close and OACK completion ages stayed within 2 milliseconds of the prior comparison despite the host completion and OACK publication changes.

The completed transfer was peer 0x57 on port 0x500d. Its OACK published at 393,739 microseconds, the peer OACK RX_DONE was observed at 431,609, host WRQ RX_DONE published at 547,367, and matching DATA arrived at 742,575. The requested QRTR RESUME published at 5,835,203, after DATA. The final ACK reached flags 0x1f. WLAN state is UNINIT with zero WLAN RRQs and WLFW servers. Runtime error is zero.

Earlier host RX_DONE alone did not complete the first write in this run. The remaining concrete divergence is the interval from OACK publication to matching peer RX_DONE observation, 2,169,947 microseconds for the first peer and 37,870 for the completed peer. Older pre-ELF captures showed prompt peer TX FIFO consumption of the original OACK even when completion was observed later. Those cursor observations measure byte consumption. Review the checked inbound FIFO observations for the exact first completion versus the peer close, to test whether matching completion was delayed before it entered the FIFO or while the host serviced it. Keep the recorded Linux receive-intent reuse difference in scope for the subsequent offline comparison.

TFTP OACK RX FIFO starts a bounded history after the checked writer publishes the original OACK. Existing validated cursor observations track RX producer advances as monotonic byte offsets across ring wrap. When the existing session validates the exact peer RX_DONE for that OACK, the adapter correlates the consumed record with the first retained cursor sample that included its end. RX ABSENT US is the start of the last validated sample that excluded that end. RX VISIBLE US is the end of the first sample that included it. These bracket host visibility. They do not report the modem's exact send time. RX BYTES AHEAD gives the bytes in front of the matched record at the first covering sample. RX AT CLOSE reports whether the validated producer cursor used while processing the first peer close covered the completion. FFFFFFFF marks unavailable values, including an evicted cursor observation after 32 producer advances. PEER TAIL US is the existing host observation of the modem reading past the original OACK on the opposite ring. FIRST DONE US remains the session's matching RX_DONE processing time. Ages use the first WRQ origin.

The fixture replayed a real close followed by matching completion through the owner and adapter, including unrelated completion traffic. It verified the older head sample excluded the matching record and the newer one included it. A second fixture checked the case where one sample covers both the close and completion and retains the original sample time as the host advances the tail. The diagnostic uses the existing checked polling path and can affect timing through counter reads and display work.

TFTP OACK RX FIFO is frozen as `native-tftp-oack-rx-fifo-frozen-v1` with SHA-256 `5515898b4033cdf35e094040b54e308ebd2fdb64d1d644fe10f0a52c87e978ba`. The enabled host suite passed 480 tests and the baseline passed 479, with one prior ignored test each. Both target builds, source checks, label checks, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed. The isolated build reused the close-handoff compile-guard exception. All 24 frozen artifacts were verified and the temporary boot is unconsumed.

After cold Fastboot confirmation, all 24 frozen artifacts, source and input hashes were reverified and the same Pixel identified as sunfish in bootloader Fastboot. Fastboot accepted the single temporary TFTP OACK RX FIFO boot. The attempt is consumed and the diagnostic result is pending.

TFTP OACK RX FIFO photo c3a7417a (SHA-256 `e8ff5219757afdf73bb738cf96b0ab09e7c59aa228d06cc8727b8a4c640ca825`). For first peer 0x1f on transfer port 0x5001, ages from its WRQ were 303,834 microseconds for host WRQ RX_DONE publication, 457,302 for original OACK publication, and 468,875 for observation of the peer TX tail passing the OACK. The host observed the retry at 1,434,732 and close at 1,649,642, with no matching DATA. The validated RX head at close excluded the matching peer OACK RX_DONE (RX AT CLOSE=0). The final excluding and first including observations bound host visibility to 2,552,522–2,587,755 microseconds. RX BYTES AHEAD=0 at the first covering sample. The session processed the completion at 2,625,561, 37,806 microseconds after the first covering sample. The delay was predominantly before the completion was visible in the inbound ring. The samples leave the modem's exact publication time and reason for closing unresolved.

A later transfer from peer 0x58 showed 37,836 microseconds from OACK publication to matching peer GLINK completion and received DATA. Its final ACK reached flags 0x1f. Runtime error is zero. WLAN remained UNINIT with zero WLAN RRQs and zero WLFW servers.

The saved Linux captures both return the WRQ's exact receive intent with reuse. The native path retires it and advertises fresh intents. Linux GLINK emits RX_DONE_W_REUSE and retains the receive intent when it is marked reusable. The next offline comparison should isolate that lifetime difference in the first server-check exchange and replay same-intent DATA, publication backpressure and uncertain publication using the native checked writer. The FIFO trace alone leaves the effect of reuse on modem behavior open.

TFTP WRQ REUSE changes the checked completion of the first exact accepted pre-ELF server-check write to GLINK command 14 (RX_DONE_W_REUSE). It uses the existing priority selection. Successful publication returns that slot to Available under the same wire intent with cleared receive state and a fresh private TFTP datagram identity. The next completion retires the slot normally. WouldBlock leaves the original intent pending. Uncertain publication keeps the session quarantined. The display reports FIRST WRQ REUSE only after successful publication and keeps the original peer, first DATA, final ACK, ACK-before-ELF and WLAN fields. A later transfer can still populate the completed-peer and final-ACK rows.

The offline replay exercised the real session with competing publications and the RAM server. It checked the exact command 14 before and after WouldBlock, rejected same-intent DATA before publication and an old request handle after publication, accepted new DATA under the wire intent, and generated the final block ACK. The uncertainty case rejected both receive and publication after quarantine. Integrated host suites passed 481 enabled and 480 baseline tests with one prior ignored test each. Both AArch64 builds, the 29 runtime source tests, 10 flow tests, 20 diagnostic fixtures, boot layout, the LZ4 round trip, and the capability audit with 280 rejected mutations passed.

TFTP WRQ REUSE is frozen as `native-tftp-wrq-reuse-frozen-v1` with SHA-256 `6363aa66f719c3b7d7da91c33538c5155cc27d355bf53ca172a74fa4549a053f`. All 24 frozen artifacts were reverified. The isolated build reused the prior close-handoff compile-guard exception. The one-shot Fastboot attempt is unused, and the effect on the modem remains to be measured.

After cold Fastboot confirmation, all 24 frozen artifacts and the recorded source, build and test inputs were reverified. Fastboot identified the same Pixel as sunfish in bootloader mode and accepted the single temporary TFTP WRQ REUSE boot. The attempt is consumed and the device result is pending.

TFTP WRQ REUSE photo 29f0176c (SHA-256 `a8b3087460731c94fe6706eb48f82618dd27b706cfb56aec1771bd2fc7b16d69`). FIRST WRQ REUSE and RXDONE PRIORITY are both 1. For peer 0x1f on transfer port 0x5001, the host published the reuse completion at 304,957 microseconds and OACK at 458,950 from the WRQ. FIRST RX READY increased from one to two. The modem's TX-tail observation was at 470,570, 11,620 microseconds after OACK publication. The peer retried at 1,439,854 and closed at 1,655,580. No matching first DATA arrived. RX AT CLOSE and RX BYTES AHEAD were zero. The matching OACK completion became visible between 5,302,566 and 5,337,947 and was processed at 5,364,257. The observed visibility interval moved later than in the preceding boot. This comparison cannot attribute the difference to reuse.

Later peer 0x5a received its OACK completion 37,972 microseconds after publication and supplied DATA at 497,635 microseconds from its WRQ. FINAL ACK FLAGS 0x1f belongs to that completed transfer. ACK BEFORE ELF is unavailable for the first transfer. Runtime error is zero, WLAN is UNINIT, and both WLAN RRQ and WLFW counts are zero.

Both successful Linux GLINK captures show a 108-byte incoming packet matching the first WRQ, a nearby 44-byte incoming packet, and a 48-byte outbound packet with matching RX_DONE before the 72-byte OACK. The 48-byte transmit and OACK both used remote intent 0 in both captures. The capture contains lengths and intent metadata, so the surrounding packets' QRTR endpoints and QMI identity remain open. The native and Linux transmit selectors both choose a smallest fitting free remote intent. A useful next comparison is the bounded packet sequence around the first WRQ, capturing QRTR endpoints and QMI message ID where applicable and correlating adjacent replies with GLINK publication and completion. That can identify the surrounding exchange or a concrete ordering difference before changing TFTP again. The data so far do not establish that completing the first WRQ is itself required for WLAN startup.

The two saved Linux IPCRTR traces were reverified against their capture hashes. Relative to the 108-byte candidate WRQ receive, the ELF-locator capture shows the 44-byte receive at 61 microseconds, the 48-byte transmit at 277, its matching GLINK RX_DONE at 329, and the 72-byte candidate OACK transmit at 759. The earlier capture shows 98, 353, 439 and 696 microseconds respectively. The 48-byte completion preceded the candidate OACK transmit trace by 430 and 257 microseconds. The 482- and 343-microsecond intervals begin at the earlier transmit trace. IPA indication registration (0x20) and driver init complete (0x35) can both fit the observed sizes. The saved traces expose lengths and GLINK intent IDs, so the Linux packet identity remains unresolved.

FIRST WRQ WINDOW retains the last 44-byte QRTR DATA receive and last 48-byte transmit before the first exact pre-ELF server-check WRQ. It also retains the first of each after the WRQ through the matching client close. The screen displays QRTR ports, decodable QMI kind and message ID, and times from the native runtime origin. The RX44 TX48 CNT upper and lower halves count the post-WRQ candidates. POST48 INTENT identifies the selected transmit buffer. Matching GLINK RX_DONE can populate the transmit row after close. The trace holds header metadata only and can perturb timing through clock reads and display work.

The new offline fixture checked pre-WRQ replacement, post-WRQ retention, header decoding, backpressure without a false publication, exact completion with a reused peer intent, and clearing on reset. The integrated host suites passed 482 enabled and 481 baseline tests with one prior ignored test each. Both AArch64 builds, 29 source regressions, 10 flow tests, 20 diagnostic fixtures, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed.

FIRST WRQ WINDOW is frozen as `native-tftp-first-wrq-window-frozen-v1`, SHA-256 `847f3cfc86414034e89208ce1b28fdd391be683e13f7ba6adfcfa3600b1a294b`. All 24 frozen artifacts were reverified. The isolated build used the prior close-handoff compile-guard exception. The one-shot Fastboot attempt is unused.

After cold Fastboot confirmation, all 24 frozen artifacts and the recorded source, build and test inputs were reverified. Fastboot identified the same Pixel as sunfish in bootloader mode and accepted the single temporary FIRST WRQ WINDOW boot. The attempt is consumed and the device result is pending.

FIRST WRQ WINDOW photo 0b7cfa6a (SHA-256 `51a3bb770cfb0f72ef78520733d41e67f5db55e737b1d3f73e056389784dc72e`). Runtime error is zero. The first WRQ was observed at 19,677,355 microseconds from the runtime origin, OACK published at 20,133,696, and the peer close was observed at 21,323,547. The only matching 44-byte post-WRQ receive was QMI request 0x20 (IPA indication registration) from modem port 0x20 to the host IPA server at 0x4002. The host observed it at 20,787,602, or 1,110,247 microseconds after the WRQ, 653,906 after OACK publication and 535,945 before the close. The trace records host processing times. No 48-byte QRTR DATA transmit published in the interval from WRQ through the close. The earlier 48-byte packet was a QMI response with ID 0x0003, from host port 0x4000 to modem port 0x14, and completed 2,490,633 microseconds before the WRQ. It is distinct from the expected registration response. No first-transfer DATA, WLAN RRQ or WLFW service appeared.

The native IPA response encoder produces a 14-byte QMI reply with the incoming transaction and message ID. Its QRTR wrapper pads the result to 48 bytes. The native IPA owner serializes prepared sends and waits for each pending transport receipt. Queue backpressure can yield WaitingQueue, and a queued send can yield WaitingCompletion. The window ends at the TFTP peer close, so this photo leaves open whether the IPA reply was prepared, queued, waiting on a previous send, or sent after the window. The saved Linux size-only GLINK captures still leave their 48-byte packet's QMI identity open. The next focused check can correlate the exact native 0x20 request and matching response across IPA preparation, queueing, GLINK publication and completion, keeping the timestamps after the TFTP close. The comparison should establish the blocked stage before any scheduling change.

IPA REG REPLY follows the first valid modem IPA indication registration request (QMI 0x20), starting at host receive before the router capacity check. It retains the request identity and timestamps for routing, preparation, transmit queue admission, full publication and matching transport completion. Request stages match the same receive handle. Transmit stages match the publication ID bound to that handle. The trace records a prior IPA message ID if another IPA packet occupied a transmit slot at receive, plus the exact reply's FlowBusy and NoCapacity queue returns. It keeps tracking after the TFTP peer closes and displays the registration and TFTP timestamps against the same runtime origin.

The offline fixture used the real owner and FIFO. The registration stayed unprepared while the previous INIT_DRIVER send was outstanding. It then published a 48-byte registration response with the exact transaction and captured the response's matching GLINK completion. A subsequent registration exchange did not change the first record. Host suites passed 483 enabled and 482 baseline tests with one prior ignored test each. Both AArch64 builds, the boot layout and the capability audit with 280 rejected mutations passed, along with the runtime source and diagnostic checks.

IPA REG REPLY is frozen as `native-ipa-registration-trace-frozen-v1` with SHA-256 `1a3417bf6efba7f12d9ca0cf086ef0a0551ad889340d92db1d9c5dfb1847d1c3`. All 24 frozen artifacts were reverified. The isolated build uses the previously recorded close-handoff compile-guard exception. The one-shot boot attempt is unused.

After cold Fastboot confirmation, all 24 frozen artifacts and the recorded source, build and test inputs were reverified. Fastboot identified the same Pixel as sunfish in bootloader mode and accepted the single temporary IPA REG REPLY boot. The attempt is consumed and the device result is pending.

IPA REG REPLY photo b0abb37f (SHA-256 `d9f535e993ba90d43fc5f8fa6d483f4c7f072ee2427cb7eb93ccaa88077117b7`). The first QMI 0x20 transaction was 1, from modem port 0x20 to IPA server port 0x4002. Times in microseconds from runtime start were 20,808,113 at receive, 20,808,117 at route, 31,342,499 at preparation, 33,689,563 at queue admission, 36,510,765 at publication, and 36,548,586 at matching GLINK completion on intent 0. The reply took 15.740 seconds from receive to completion. The stage intervals were 4 microseconds to route, 10.534 seconds to prepare, 2.347 seconds to queue, 2.821 seconds to publish, and 37.821 milliseconds to complete. Flow and capacity wait counters were zero. No earlier IPA transmit slot was recorded at receive, and the timeout cause was zero. The trace leaves the specific cause of the pre-preparation delay open.

The first TFTP OACK published at 20,152,897 microseconds. The host observed registration 655,216 microseconds later and the TFTP close at 21,345,156. The close preceded reply preparation by 9.997 seconds and completion by 15.203 seconds. No first-transfer DATA arrived. Runtime error and detail were zero. The WLAN domain query remained UNINIT with zero WLAN RRQs and zero WLFW servers. The trace localizes the largest delay to host service before preparation. The existing runtime dispatches the router, registry and RMTFS before taking one IPA step. An offline scheduling comparison can test a bounded earlier IPA turn for an already-routed registration and distinguish pending IPA state from time spent in other work before a device experiment.

IPA REG EARLY TURN gives the first routed IPA indication registration one normal service step before TFTP, registry and RMTFS when it is the next eligible IPA request or its prepared reply is ready for queue admission. The normal late IPA step is skipped in those passes. The router can yield after the checked iteration that routes the exact request, with the IRQ still pending and the existing software continuation scheduled. The trace keeps the prior registration and TFTP milestones and adds REG YIELD US and REG EARLY US.

The offline fixture checked that a competing receive waits until the router resumes, an outstanding INIT send retains priority, preparation and queue admission take separate turns, and the exact GLINK completion still retires the reply. The host suites passed 484 candidate and 483 baseline tests with one prior ignored test each. Both AArch64 builds, 29 source tests, 10 flow tests, 20 diagnostic fixtures, the boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed.

IPA REG EARLY TURN is frozen as `native-ipa-registration-early-turn-frozen-v1`, SHA-256 `3c3873da4c478b37b3ab12571a0495d23773c50e0bfb35795ec64992d6848208`. All 24 artifacts and the recorded source, build and test inputs were verified. The isolated build uses the prior close-handoff compile-guard exception. The one-shot boot attempt is unused. The timing and the first TFTP peer's response still need a device run.

After cold Fastboot confirmation, all 24 frozen artifacts and their source, build and test inputs were reverified. Fastboot confirmed the same Pixel in sunfish bootloader mode and accepted the single temporary IPA REG EARLY TURN boot. The attempt is consumed and the device result is pending.

IPA REG EARLY TURN photo 5a444a1c (SHA-256 `83e2edc881d7cd1b70e56a9bbde565104a586732fcdf11a55185ba74333582ea`). Registration transaction 1 from 0x20 to 0x4002 was observed at 20,770,321 microseconds from runtime start and routed at 20,770,326. No router yield was recorded. The first early service step began at 31,188,839, 10,418,513 microseconds after routing. Preparation followed 46,161 microseconds later. Queue admission was at 33,449,812, publication at 36,000,916 and matching GLINK completion at 36,038,637. The route-to-prepare interval was 10.465 seconds versus 10.534 in IPA REG REPLY. The peer closed at 21,306,106, 535,785 microseconds after registration arrived and 9.883 seconds before the early step. No first DATA, WLAN RRQ or WLFW server appeared. WLAN queried as UNINIT. Runtime error, IPA timeout cause, registration flow waits and registration capacity waits were all zero.

The early turn only moves an eligible registration. REG PRIOR QMI samples occupied transport transmit slots at receive. Two isolated host replays reproduced the combination of no prior transmit slot, no yield, and a later early step. In one, the INIT_DRIVER transmit had retired in GLINK but its receipt was still waiting for the IPA owner to commit it. In the other, the owner already held an INIT_DRIVER response and had yet to accept it. These are possible device explanations. The display cannot distinguish them or the other eligibility checks. Capture the gate at routing, including the pending IPA message and its queued and receipt bits, the held INIT response and the first post-route IPA step's time and outcome. Preserve the current ordering for that comparison so the sample identifies the blocker before changing it.

IPA REG OWNER TRACE records the first exact registration after owner queue admission and before transport publication. ROUTE IPA FLAGS bits 0–8 identify the core, init start, pending record, queue admission, available transport receipt, held init response, first IPA request, early-turn eligibility and modem readiness. ROUTE IPA QMI identifies the pending record. IPA STEP US and IPA STEP END US bracket the first protocol step after routing. IPA STEP RESULT 7 indicates a commit, 8 indicates response acceptance and 3 indicates reply preparation. The snapshot uses in-memory state and the existing counter. It retains the prior service ordering and can affect measured timings.

The two cases passed through the real owner and FIFO offline. An uncommitted INIT_DRIVER receipt produced flags 0x5f, pending QMI 0x21 and first step Committed. A held INIT_DRIVER response produced flags 0x63, no pending QMI and first step ResponseAccepted. Subsequent early preparation left both snapshots intact. The host suites passed 485 candidate and 484 baseline tests with one prior ignored test each. Both AArch64 builds, 29 source tests, 10 flow tests, 20 diagnostic fixtures, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed.

IPA REG OWNER TRACE is frozen as `native-ipa-registration-owner-trace-frozen-v1` with SHA-256 `2ebdf8588f525ae0517dc8c7a2b308b47efadb22552bb3d588eec7b5b6ef4e0c`. All 24 artifacts and the recorded inputs were verified. The isolated build uses the prior close-handoff compile-guard exception. The one-shot boot attempt is unused.

After cold Fastboot confirmation, the 24 artifacts and recorded inputs were reverified. Fastboot confirmed the same sunfish in bootloader mode and accepted the single temporary IPA REG OWNER TRACE boot. The attempt is consumed and the device result is pending.

IPA REG OWNER TRACE photo 9217d293 (SHA-256 `4b7128542274a1c288fbd0a5e29744f6fe892cef275f29d699f6582da4fb4283`). ROUTE IPA FLAGS was 0x40. The tracked registration was next in the IPA queue, with the core absent, init unstarted, no pending record or receipt, and no held init response. Transaction 1 from modem port 0x20 was routed at 20,801,590 microseconds from runtime start. The first IPA step started 437,075 microseconds later and took 34,634 microseconds. Result 2 was Discovered, which creates the core from the modem endpoint already available at that step.

The first peer closed 64,494 microseconds after discovery ended, with no first DATA. The first early registration step started 9,952,949 microseconds after discovery ended. Registration preparation finished 46,179 microseconds after that step started. Queue admission, publication and matching completion followed at 33,488,651, 35,888,036 and 36,078,980 microseconds. The route-to-prepare delay was 10.471 seconds. There were zero registration flow or capacity waits, zero runtime error, and no IPA timeout. WLAN remained UNINIT with zero WLAN RRQs and zero WLFW services. This trace locates the largest gap after discovery, but does not timestamp INIT_DRIVER preparation, admission, publication, or receipt within the gap.

An isolated replay of the owner and FIFO held back the modem service announcement and delivered it with registration in a single transport pass. It reproduced route flags 0x40 and a first step of Discovered. Two subsequent checked protocol steps prepared and queued INIT_DRIVER. Registration preparation still waited for the real matching GLINK receipt to be committed. A bounded continuation can take those two local init steps when discovery succeeds with the exact registration already waiting, yield on queue backpressure or peer completion, and measure INIT_DRIVER publication and commit alongside the first TFTP close. The replay establishes the local transition order. The device trace does not establish how much of the delay comes from those local turns versus the intervening transport work, or that changing the order will complete the first TFTP transfer.

IPA INIT CONTINUATION uses up to two ordinary local IPA steps after discovery, when the exact registration is already next in the IPA queue. Preparation and queue admission run in that service pass. The existing GLINK owner publishes and retires the record. The IPA state machine still requires its matching receipt before commit or registration reply preparation. The display times INIT_DRIVER from discovery through commit and retains the registration, first TFTP close, and WLAN milestones.

The integrated fixture checked both discovery cases, including that discovery without a waiting registration takes no extra steps. It verified queue admission before publication, waiting for the real GLINK completion, exact retirement before owner commit, and registration preparation after commit. The host suites passed 486 candidate and 485 baseline tests with one prior ignored test each. Both AArch64 builds, 29 source tests, 10 flow tests, 20 diagnostic fixtures, boot layout, LZ4 round trip, and the capability audit with 280 rejected mutations passed.

IPA INIT CONTINUATION was frozen as `native-ipa-init-continuation-frozen-v1` with SHA-256 `e374d0e618f97f9754b3dd940f4a7893c0191627afa33f51dc93e88b0b3cee19`. The isolated build uses the prior close-handoff compile-guard exception. After cold Fastboot confirmation, the 24 artifacts and recorded inputs were reverified. Fastboot confirmed the same sunfish in bootloader mode and accepted the single temporary boot. The attempt is consumed and the device result is pending.

IPA INIT CONTINUATION photo 1d860e81 (SHA-256 `8486d1a89b57cf54c3d4142dc8f2cb6bc898a7c52c4fbab10609fde518879fe0`). ROUTE IPA FLAGS remained 0x40. INIT EXTRA was 2 and INIT QUEUE WAIT was 0. Times in microseconds from runtime start were 20,838,385 at registration route, 21,311,126 at discovery, 21,345,846 at init preparation, 21,380,564 at queue admission, 25,880,178 at full publication, 26,770,921 at matching transport retirement, and 29,049,768 at owner commit. The two extra steps queued INIT_DRIVER 69,438 microseconds after discovery, 64,647 before the first TFTP peer closed. Publication followed queueing by 4.500 seconds and came 4.435 seconds after the close. Retirement followed publication by 891 milliseconds and owner commit followed retirement by 2.279 seconds.

Registration preparation finished at 31,403,649 microseconds, 2.354 seconds after init commit. The route-to-prepare interval was 10.565 seconds compared with 10.471 in IPA REG OWNER TRACE. The first TFTP exchange recorded no DATA. The RAM server reported one completed write in its transfer table, which does not establish completion of that first exchange. Runtime error and IPA timeout cause were zero. WLAN remained UNINIT with zero WLAN RRQs and WLFW services.

Queue admission and publication are separate. The zero queue-wait count only covers admission backpressure. The owner schedules the existing continuation when INIT_DRIVER is queued, and the native loop normally takes another router turn while that continuation is pending. A router turn can process a pending close before reaching its writer. The checked writer can choose control or other data, wait for a fitting peer intent, or encounter FIFO backpressure. This run timestamps the end of publication and leaves those cases unresolved. Replay the existing selector with INIT_DRIVER and competing records. Compare the first router entry, first writer call, exact target selection and actual publication, keeping the existing intent and FIFO checks. The same timestamps can distinguish retirement from a later IPA service turn that commits the receipt.

IPA INIT TX TRACE binds to the exact INIT_DRIVER publication ID. It records the first transport owner work entry, first session publication pass, first checked writer call for any record, first writer call for INIT_DRIVER, full publication, transport retirement and owner commit. Between queueing and full publication it counts competing control and DATA publications, samples peer intent fit, and distinguishes FIFO backpressure on the exact INIT_DRIVER record. TX FLAGS bits 0–3 record an outstanding intent request, an older same-flow packet, a competing record blocked by FIFO space, and an uncertain write. The trace leaves scheduling and checked writes in place. The additional measurements and display updates can affect timing.

The owner replay used small peer buffers that could carry competing control and QRTR credit records while INIT_DRIVER was waiting. It observed no target selection until a fitting intent became available, then verified exact publication and matching retirement. The link replay made the checked writer return WouldBlock for INIT_DRIVER and verified the subsequent successful write used the same bytes. The host suites passed 487 candidate and 486 baseline tests with one prior ignored test each. The targeted link test, both AArch64 builds, 29 source tests, 10 flow tests, 20 diagnostic fixtures, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed.

IPA INIT TX TRACE is frozen as `native-ipa-init-tx-trace-frozen-v1` with SHA-256 `4e926597438cdd8648d3da6a396deda58859f46a83a0cbc9f6609a2603915c5d`. All 24 artifacts and recorded inputs were verified. The isolated build uses the prior close-handoff compile-guard exception. The single temporary boot attempt is unused.

After cold Fastboot confirmation, the 24 artifacts and recorded inputs were reverified. Fastboot confirmed the same sunfish in bootloader mode and accepted the single temporary IPA INIT TX TRACE boot. The attempt is consumed and the device result is pending.

IPA INIT TX TRACE photo 460931ca (SHA-256 `6c1ac33c2c27d56b87c25f1ad5b7214fc38538eb267bbe39fc8ea766cf5bbbeb`). The transport owner entered 2 microseconds after INIT_DRIVER queue admission. Its first publication pass and writer call followed queue admission by 110,630 and 110,631 microseconds respectively. The writer selected INIT_DRIVER 4.306 seconds after its first call and took another 69.363 milliseconds to publish it. The 28 passes account for 26 successful control records, one other DATA record and the target record. No pass recorded a missing fitting intent, a FIFO block on INIT_DRIVER, an outstanding intent request, an older same-flow packet, a blocked competing record, or an uncertain writer outcome. The 4.486-second queue-to-publication interval was dominated by the time before target selection. The trace groups control records and leaves the individual control types and the cost between passes unresolved.

The first TFTP peer close was observed 64.481 milliseconds after INIT_DRIVER queue admission and 46.150 milliseconds before the first writer call. No first DATA appeared. The RAM server reported one completed write. WLAN remained UNINIT with zero WLAN RRQs and zero WLFW announcements. The later delays were 888.024 milliseconds from init publication to retirement, 2.271 seconds from retirement to commit, and 2.286 seconds from commit to registration reply preparation. Runtime error and IPA timeout cause were zero.

Test a one-time INIT_DRIVER transmit turn in the existing checked selector. Preserve the intent grant advertisement and ACK, first server-check RX_DONE, existing TFTP and registry priority turns, ready QRTR credits, and the target's intent and destination-flow checks. Compare competing control counts, actual publication, and later service milestones. The prior close was observed before the first writer call, so a transmit-selection change alone leaves the first peer's timing unresolved.

IPA INIT TX TURN gives the exact queued INIT_DRIVER one priority writer attempt. It keeps pending intent acknowledgements, first WRQ RX_DONE, existing TFTP and registry turns, and ready QRTR credits ahead of that attempt. It uses the same fitting-intent, outstanding-request, and destination-flow checks as ordinary transmit selection. The priority applies to a complete unsent datagram that fits one GLINK record. The ordinary checked writer publishes it and the matching completion still creates its receipt. WouldBlock consumes the priority attempt, leaves the bytes queued, and returns later passes to ordinary selection. The screen retains the transmit timing and reports INIT TX TURN as 0 for unused, 1 for attempted without a successful write, or 3 for a successful priority write.

The targeted replays verified that the intent grant advertisement and ACK and a ready QRTR credit go first, the priority attempt can precede an ordinary receive advertisement, and a blocked attempt preserves the target bytes and lets the advertisement go on the next pass. The receipt still requires matching GLINK completion. The candidate and baseline host suites each passed 487 tests with one prior ignored test. Both AArch64 builds, two targeted link tests, 29 source tests, 10 flow tests, 20 diagnostic fixtures, boot layout, LZ4 round trip and the capability audit with 280 rejected mutations passed.

IPA INIT TX TURN is frozen as `native-ipa-init-tx-turn-frozen-v1` with SHA-256 `c50c4783615d07e6330af508d614665d82dee8eba0375384841612517ab6bfd1`. All 24 artifacts and recorded inputs were verified. The isolated build uses the prior close-handoff compile-guard exception. The single temporary boot attempt is unused.

## IPA scheduling trace and Linux/Android review — 2026-11-12

The user-supplied `codex-clipboard-a017862a-1b6e-49df-b4c6-93c272b158c7.png`
shows IPA INIT TX TURN=3, zero runtime error/detail, 24 publication passes,
22 competing control publications and one competing DATA publication. No
missing-fit, target FIFO-block or TX flag was recorded. Hexadecimal timestamps
decode to 21,329,618 us at queue admission, 21,440,267 at first writer entry,
25,128,089 at target selection, 25,197,391 at publication, 26,858,845 at
retirement, 29,042,948 at owner commit and 31,326,027 at registration preparation.
The first close was observed at 21,983,918 us. Thus queue-to-publication was
3.867773 s and the close preceded publication by 3.213473 s. The screen gives
no first DATA timestamp, one completed write elsewhere, WLAN UNINIT and zero
WLAN RRQ/WLFW counts. This is evidence of host delay and absent WLAN discovery,
not proof that the delay caused the peer close or the missing WLAN startup.

`sunfish-ipa-schedule-trace` adds observation to the same INIT priority-turn
profile. It does not change selector precedence, wire packets, FIFO admission,
receipt ownership, service order, interrupt authority or notification policy.
The earlier TX timestamp fields remain retained in the transport/owner trace;
the screen is replaced by `IPA SCHEDULE TRACE`.

The `G` rows contain two saturating 16-bit pass counts, first label in the
high half and second in the low half. ACK, first-WRQ completion, TFTP priority,
registry priority and ready QRTR credit are the existing higher-priority
conditions. UNREADY means the exact target is fragmented, too large for its
one-shot priority turn, or lacks a ready intent/flow. USED means the one
priority attempt has already happened; ELIGIBLE means that pass selected the
priority candidate. These are overlapping conditions, not mutually exclusive
winning categories. Pair these counts with the wire-command counts. Pending
ACK can gate INIT even on a pass that publishes an advertisement or RX_DONE.

`CMD 5 6`, `CMD 7 8` and `CMD 14 OTHER` count actual successful competing
GLINK wire commands: intent advertisement, RX_DONE, intent request, intent
request acknowledgement, RX_DONE_WITH_REUSE, and other non-DATA commands.
WouldBlock and uncertain writes do not count as successful commands. The
counts stop when the exact INIT datagram is fully sent, even without a clock.
They should sum to CONTROL AHEAD in the retained trace. They count records,
not bytes, firmware acceptance or time.

Each `TP ... MS` row packs two millisecond totals: high 16 bits cover INIT queue
through publication; low 16 bits cover publication through first registration
reply preparation. The categories are router dispatch, storage, IPA, painting,
service registry, TFTP, outside callbacks and other work. TFTP includes its
existing nested early router dispatch; it is not double-counted as router.
Outside means time from the end of a runtime callback to the next entry, not
proof of seL4 scheduler starvation. Other includes unprofiled callback work,
receive IRQ dispatch, close resolution and observer overhead. Paint includes
existing progress updates and full diagnostic repaint. The current repaint's
cost becomes visible on a subsequent repaint.

Physical-counter intervals are split at the actual retained queue/publication/
retirement/commit/preparation timestamps. Four standalone millisecond rows
preserve those interval durations. Completed windows stop accumulating. A
missing endpoint displays FFFFFFFF for a standalone duration. Packed halves
use FFFF for unavailable and FFFE for saturation; count halves saturate at
FFFF. All screen values remain hexadecimal. TRACE INVALID=1 indicates a
reversed, missing-prefix, changed or late-discovered boundary; timing values
then display unavailable instead of silently assigning misleading durations.
Extra counter reads and changed screen contents can perturb measured timing.
The feature does not itself cause extra diagnostic repaints.

### What the open source establishes

The selected Linux source is the extracted working-reference kernel
`2267e248204e7e051e0f7355298949a6440de579`, in
`.cache/sunfish-mobian-comparison/source-review/kernel/`. Android source is
`.cache/sunfish-lineage-kernel/` at
`28f9290ae067d3e7857126eb6312720dbe9c7c80`. Exact reviewed source hashes and the
original-C oracle output are retained in the new diagnostic build directory.

* Linux `drivers/net/ipa/ipa_qmi.c:ipa_server_indication_register()` sends the
  registration response immediately in its server callback. Only after a
  successful send does it mark registration and call the readiness helper.
  `ipa_client_new_server()` schedules INIT_DRIVER on separate work;
  `ipa_client_init_driver_work()` waits for its QMI response there. The existing
  original-C oracle's RIU and RUI orders both mark registration while
  `modem_ready` is still false. Its transport is stubbed; this demonstrates
  callback behavior, not a full kernel concurrency or device timing result.
* Android `drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c:
  ipa3_handle_indication_req()` also sends the registration response before
  testing `ipa3_qmi_modem_init_fin`; that later test gates the completion
  indication, not the registration reply. Client initialization runs on its
  request workqueue.
* Linux `drivers/rpmsg/qcom_glink_native.c:__qcom_glink_send()` selects a fitting
  free intent and writes through `qcom_glink_tx()`. The latter writes/kicks
  when space is available, waiting without the TX lock if necessary. Successful
  return from this send path does not wait for peer RX_DONE. Receive completion
  work has its own worker. These functions do not contain our global bounded
  service loop or its control-before-INIT arbitration.
* Native `sunfish_ipa_linux_qmi_owner::Runtime::step()` resolves a pending
  publication before preparing an IPA server reply. The core's
  `ColdSession::prepare()` independently rejects a second publication while
  one is pending. The transport also carries the exact publication and receipt.
  Therefore moving a call earlier cannot by itself reproduce Linux's independent
  client/server progress. Changing this requires distinct bounded publication
  identities and receipts, with tests for out-of-order completion, backpressure,
  stale/wrong receipts, failure and lifecycle loss. Receipt checks must remain.

This is a concrete architectural difference and a strong candidate for the
next offline comparison. It does not establish that the modem requires the
registration reply before INIT or that correcting it alone starts WLAN.
Use the new timings to locate expensive work as well; the earlier ~69-ms
checked packet write and multi-second service gaps warrant measurement.

### Remaining distance to WiFi

The saved working Linux integration starts MPSS and waits for WLFW before
loading `ath10k_snoc`. Its journal shows WLAN image requests before ath10k
initialization. The native path has substantial IPA/SMMU/MPSS/GLINK/QRTR and
host-service evidence, but this run still stops before that WLFW milestone.
Linux `ath10k/qmi.c` and Android `icnss.c` then perform registration, MSA and
capability exchanges and process firmware-ready events before later driver
startup. Their platform/firmware quirks must remain tied to the selected
reference; they are not interchangeable recipes.

After discovery, the composed native path still needs firmware-ready, CE/HTC/
WMI initialization, scan, association/key installation, DHCP and sustained
bidirectional traffic on the device. Existing codecs and host tests reduce
implementation work but do not prove that integration. Neither a percentage
complete nor a fixed number of additional screens is supported by the evidence.
The host source is available; the firmware state machine is binary, and Linux's
scheduler, IRQ/DMA/cache and service semantics are not automatically supplied
by a native seL4 port. The productive next step is source-led offline comparison
plus a discriminating device measurement, not a succession of untested timing
changes.

## IPA schedule device result and independent-server offline comparison — 2026-11-12

The user authorized one temporary boot from cold bootloader fastboot. The frozen
`native-ipa-schedule-trace-frozen-v1/fastboot.img` and its recorded inputs were
verified before boot. SHA-256 is
`7ff9601d4ce085a4d951dfe9f3c7952466f3471c0d1c410527f857e610d92b56`.
Fastboot reported successful sending and booting. The attempt record and
transcript are beside that frozen image; no partition was flashed. The user
then supplied `Screenshot 2026-11-12 at 10.55.45 PM.png` of the settled screen.
The original image, transcription and interpretation are retained under
`.cache/sunfish-native-wifi/private/ipa-independent-server-offline-v1/device-result/`.

### What this boot establishes

Runtime error/detail, TRACE INVALID and TX FLAGS are zero. INIT TX TURN=3
records the successful priority attempt. The readable gate counts show 23
passes with ACK pending, one with TFTP priority, no first-WRQ or registry
priority, and one eligible INIT priority selection. Gate counts overlap.
The successful competing commands are six intent advertisements, seven
RX_DONE records and nine intent-request acknowledgements: 22 control records.
No command 7, command 14 or other control command was counted.

The photo reads QUEUE PUB MS=`00000F1D`, or 3,869 ms. Its category totals are
consistent after integer truncation. Timing attribution is:

| Work category | INIT queued to published | Published to registration reply prepared |
| --- | ---: | ---: |
| Router dispatch | 3,592 ms | 5,865 ms |
| Storage | 46 ms | 46 ms |
| IPA | 69 ms | 126 ms |
| Service registry | 46 ms | 92 ms |
| TFTP | 115 ms | 0 ms |
| Paint, outside callbacks, other | 0 ms displayed | 0 ms displayed |

Publication to retirement is 1,662 ms (`0000067E`); commit to registration
preparation is 2,284 ms (`000008EC`). The post-publication bucket sum is 6,129
ms, placing the intervening retirement-to-commit time at approximately 2.18 s
by subtraction, subject to millisecond truncation. It is not a directly
visible measurement on this photo. Most measured elapsed time lies inside
router dispatch. Zero outside-callback milliseconds does not prove the absence
of sub-millisecond scheduling delays, and router time includes its nested
transport/validation work. This screen does not identify which operation
inside that category is expensive or why firmware has not started WLAN.

Two intended rows did not render. `Display::hex()` allows a 15-character label
in its 24-column buffer. `G CREDIT UNREADY` and `RETIRE COMMIT MS` are each 16
characters, so the function returned without replacing those rows. The visible
`FIRST TX US` and `REPEAT DST PORT` belong to earlier OACK/TFTP panels. Neither
is evidence for an IPA credit count or retirement-to-commit measurement.
The earlier panel bounds test incorrectly allowed 17 characters. The workspace
now uses `G CREDIT UNRDY` and `RET COMMIT MS`, and its bounds check derives the
limit from the actual renderer. This correction has passed offline checks;
the already-booted frozen image is unchanged.

PD QUERY STATE remains `7FFFFFFF` (UNINIT); WLAN RRQ and WLFW service counts
remain zero. INIT scheduling progress and the absence of a runtime error do
not establish WLAN readiness.

### Source-led comparison and experimental implementation

`platforms/sunfish/compare-ipa-registration-reference.py` extracts and compiles
the original `ipa_server_indication_register()` callback from the pinned Linux
source and `ipa3_handle_indication_req()` from the pinned Android source
identified above. Eight cases cover modem-ready false/true and response-send
success/failure for each source. Both attempt the registration response while
modem readiness is false. Successful sends mark registration; failed sends do
not. Android's later init-complete indication remains gated by modem readiness.
Transport and Linux's readiness helper are recording stubs. This establishes
callback ordering, not kernel scheduling, real transport acceptance or a
proprietary firmware requirement. Linux's independent client work is at
`ipa_qmi.c:401,448`; Android's client request workqueue and delayed work are
in `ipa_qmi_service.c` (including lines 1692 and 1868).

The feature `sunfish-ipa-independent-server-experiment` implements a bounded
offline comparison in the actual Rust core, GLINK session, checked FIFO and
owner. The core retains one client publication and one server publication.
The link retains matching per-socket transport receipts, a common owner and
generation, and monotonic sequence checks per socket. The runtime alternates
progress between the sockets and can service one while the other waits.
It retains both deadlines and all peer/generation, transaction, capacity,
uncertainty and receipt checks. Same-destination flow blocking remains.
No readiness flag is granted merely because the other socket completed.

The paired scenario publishes INIT, withholds its GLINK completion, supplies
an early INIT response, and delivers an IPA registration request. The unchanged
serialized baseline cannot publish the registration reply at that point.
The candidate can, and its server receipt can commit while INIT and its early
response remain held. Both completion orders finish the modeled handshake.
The candidate still requires exact GLINK retirement before protocol commit;
Linux's send-return semantics are less restrictive. This experiment isolates
client/server independence rather than replacing the entire Linux transport.

Validation retained in
`.cache/sunfish-native-wifi/private/ipa-independent-server-offline-v1/result.json`:

* Baseline: 13 core, 235 owner and 155 link tests passed.
* Candidate: 13 core, 238 owner and 158 link tests passed.
* Both variants cross-compile as `no_std` AArch64 libraries with the concrete
  volatile NativeIo owner instantiated. This is not the complete Pixel binary.
* Eight source-extracted Linux/Android callback cases passed.
* Candidate cases cover both queue/retirement orders, coexisting receipts,
  wrong-owner/stale/crossed receipt rejection, bounded server records, timeout
  of either socket despite progress on the other, generation loss, uncertain
  publication and no I/O after terminal failure. Existing owner tests also
  exercise backpressure and failure paths.
* The schedule diagnostic's unchanged-behavior profiles each passed 487 host
  tests (one preexisting ignored test), plus two link arbitration, 22 timing
  and 29 runtime-source checks. Its corrected panel bounds check passed.

The old owner assertion that a retained INIT must hold registration is now
baseline-only, replaced for the experiment by the paired scenario above.
The link's existing unconsumed-receipt test checks both policies and still
rejects receipt substitution; the core milestone test verifies that a server
commit cannot publish INIT. These are intentional expectation changes, not
ignored failures.

The experiment is disabled by default and the Pixel runtime has an explicit
compile guard against composing it into a boot image. The full runtime's
existing early-service shortcuts and timing boundaries have not yet been
adapted to overlapping IPA publications. No candidate was booted. Before a
hardware trial, integrate that composition and its event tracing, then use the
same exact-record tests against the full runtime feature profile. The separate
performance question is now concentrated in router dispatch and its nested
transport/guard operations; another broad timing screen is not needed to
establish that concentration. Neither the callback comparison nor this photo
proves that independent IPA replies alone will make WLFW appear.

## Composed independent IPA runtime — 2026-11-12

Following the user's approval to integrate the offline comparison, the opt-in
`sunfish-ipa-independent-server-runtime` profile composes the independent
client/server core with the full native service loop. It depends on the
existing INIT TX TURN instrumentation and independent-response/QRTR flow
profiles. The standalone independent-server experiment remains blocked in the
Pixel binary unless this composition is explicitly selected.

The registration early-service predicate now considers the server's pending
record independently. An INIT response held behind INIT's own transport
receipt no longer prevents registration service. The early turn respects the
alternating socket scheduler: if the client may make progress and owns the
next turn, it gets the ordinary service opportunity. The existing discovery
continuation remains bounded to two local attempts to admit INIT; it does not
wait for INIT completion before permitting a server reply. Registry, storage,
TFTP and WLAN query/listener service order and their existing authority checks
remain in place.

Both sockets still share GLINK intents, FIFO space and transport arbitration.
A full-profile small-buffer test demonstrates this limit: the server reply
can be admitted while INIT is pending, but a shared intent-request gate can
hold both publications. Clearing that gate permits both records, and the
server can then complete without committing INIT. Independent client/server
bookkeeping does not bypass transport backpressure or guarantee low latency.

The new `IPA INDEPENDENT` panel shows 27 rows of retained observations. All
`... US` values are hexadecimal microseconds since this runtime started;
`FFFFFFFF` means unavailable. INIT discovery, preparation, queue, publication,
retirement, commit and response arrival have separate timestamps. Registration
receive, route, preparation, queue, publication and completion also have
separate timestamps. They do not assume that INIT completes first. INIT RSP US
is arrival metadata, while RSP ACCEPTED separately counts accepted responses.
The existing single-window schedule observer is rejected when combined with
this profile because it assumes the older event ordering.

`IPA FLAGS` bits 0–5 mean modem ready, QMI UC ready, registration committed,
completion indication committed, handshake complete, and owner failed.
`PENDING FLAGS` bits 0–4 mean client record retained, client queued, server
record retained, server queued, and INIT response held. `REG PRIOR MSG` is the
message retained in transport when registration arrived. INIT TX TURN and
CONTROL/DATA AHEAD retain their previous meanings. FIRST CLOSE US, PD QUERY
STATE, WLAN RRQ RX and WLFW SERVERS preserve the external startup milestones.
No displayed protocol flag is a substitute for a hardware-ready capability.

The full candidate feature profile passed 493 host tests; the serialized
baseline passed 487. Each has the same one preexisting ignored test. Candidate
coverage includes overlapping discovery and early turns, registration before
INIT retirement, both completion orders, correct timestamp ownership, early
response retention, bounded admission fairness, both socket deadlines,
uncertain writes, generation loss and no I/O after failure. The previous
strictly serialized discovery expectation remains baseline-only; the composed
cases replace it. The original registration trace and small-buffer tests now
check the appropriate behavior for both configurations.

The paired core/owner/link suites and source-extracted Linux/Android callbacks
also pass. Both minimal variants compile for AArch64, and both full Pixel
profiles build for the actual Microkit target. The source checks pass, all 27
labels and final hex digits render in both buffers using the actual display
implementation, and compiler tests verify the permitted and rejected native
feature combinations. `platforms/sunfish/test-native-ipa-independent-runtime.py`
is the reproducible host/profile/render/guard runner; logs and source hashes
are under `native-ipa-independent-server-build-v1` in the private cache.

The candidate uses the same isolated build-copy exception for the existing
close-handoff guard as its accepted predecessor; that guard is retained in the
workspace. The independence guard is not removed: the composed feature is now
its explicit supported path. Neither timeout values nor firmware, MMIO/IRQ
capabilities, receipt requirements or packet encodings were relaxed.

The temporary-boot image is 68,796,416 bytes, with SHA-256
`a8e572662981139172fc6a4852132118c4e791eec328aa6b80f23be70c1566cf`.
The complete source/relocation/initial-task/copy-target boot-layout check passed,
and LZ4 round-trip verification matches the wrapped loader exactly. This entry
records preparation, not a device result. No candidate boot or flash was
performed during integration. The first hardware question is whether the
registration publication/completion now precedes INIT retirement/commit and
whether WLAN image requests or WLFW discovery subsequently appear; either
outcome must be read from a fresh cold boot.

The final capability audit passed with all 280 mutations rejected. The verified
candidate and its source/proof identities are frozen as
`native-ipa-independent-server-frozen-v1`. Its temporary-boot attempt is unused.

## Independent IPA device result — 2026-11-12

The user subsequently supplied cold bootloader fastboot. The frozen candidate,
71 artifacts and 558 source snapshots were verified; exactly one sunfish in
bootloader fastboot was selected. One temporary boot succeeded (6.337 seconds
reported by fastboot), with no flash. The attempt is now consumed. The two
user-supplied 11:53 PM photos and their transcription are saved under
`native-ipa-independent-server-frozen-v1/device-result/`; the frozen image and
manifest remain unchanged.

The first photo is intermediate: IPA FLAGS=7, PENDING FLAGS=C, three committed
publications and one accepted INIT response. The server still has a queued
record; the final completion indication has not committed. Its query state
FFFFFFFF is unavailable, not UNINIT. The live progress painter overwrote rows
15–18 (registration preparation, queue, publication and completion) with
CHECKPOINT/WAKE/RMTFS markers. This is a separate display-composition problem
from the previously fixed label-width issue; the row-render tests do not model
subsequent progress repainting.

The later photo restores all event rows and shows IPA FLAGS=1F, PENDING FLAGS=0,
four commits and one accepted response. Runtime error/detail remain zero.
Thus the host's complete cold IPA QMI handshake is observed. WLAN RRQ and WLFW
counts remain zero and the completed PD query reports UNINIT (7FFFFFFF).
Handshake completion alone has not triggered observable WLAN startup.

Selected times in seconds since runtime start:

| Event | Time |
| --- | ---: |
| Registration received | 20.659478 |
| INIT queued | 21.180054 |
| First TFTP close | 21.244889 |
| Registration prepared | 21.530258 |
| Registration queued | 24.094765 |
| INIT published | 25.081112 |
| INIT retired | 26.740211 |
| INIT committed | 28.913457 |
| Registration published | 29.082747 |
| Registration transport completed | 29.521521 |
| INIT response observed | 44.840664 |

The independent protocol path works at preparation and admission: registration
was prepared and queued before INIT was published or retired. However, actual
registration publication still followed INIT commit by 169.290 ms. It waited
4.987982 seconds after queue admission, in addition to 2.564507 seconds between
preparation and admission. INIT queue-to-publication remained 3.901058 seconds,
with 22 competing control records and one DATA record. The run therefore does
not demonstrate registration publication/completion before INIT retirement,
and cannot confirm or refute a firmware requirement for that earlier reply.

The next focused question is what prevents the exact already-queued
registration reply from being selected/published during those 4.99 seconds:
shared intent availability/request state, competing control/credit work,
flow eligibility, FIFO backpressure, or expensive dispatch work. The source
still contains these transport gates; the screen does not identify a winner.
The first TFTP close also preceded registration preparation, so attributing
that close or absent WLAN solely to the remaining IPA delay would be premature.
No new firmware image or scheduler change follows from this observation alone.

## Registration TX wait device capture — 2026-11-13

The temporary image with SHA-256
`382ae030888c04e86f8f64867ff5395e4efd4f26ddc1fb3a1875d5fc3ccff563`
passed the target build, full boot-layout and LZ4 round-trip checks, and the
280-mutation capability audit. The full host trace profile passed 493 tests
with one ignored. The target build exposed the missing registration-trace
feature gate on the counter formatter, which was corrected. The build used
the same isolated close-handoff guard exception as the independent IPA image.

The user's first photo shows the expected IPA REG TX WAIT panel and is saved
in the private registration TX build directory under device-result.
Registration publication followed queueing by 4,958 ms, and transport
completion followed publication by about 436 ms. The first observed publish
pass began after 99 ms. A fitting ready remote intent existed for the exact
reply on all 33 observed passes. The trace recorded zero request-wait,
no-fit, flow-predecessor, idle, blocked-write and uncertain-write counts.
Before selecting the reply, 29 GLINK control records (10 intent
advertisements, 6 RX_DONE, 13 intent-request ACKs) and 3 other DATA records
were successfully published. Both measured active-pass and writer time
rounded to 2,284 ms. The remaining queue time is outside those measured
active intervals. Gate-presence counts overlap and do not identify which
record won selection on each pass.

This is an intermediate frame. Runtime error and detail are zero, IPA flags
are 0x04 (registration committed), and query state FFFFFFFF is unavailable.
WLAN RRQ and WLFW server counts are zero at capture. A settled capture is
needed to determine whether IPA completed and the WLAN PD query ran.

The settled photo (device-result/complete.png) shows IPA flags 0x1F, zero
runtime error/detail, completed PD query UNINIT (0x7FFFFFFF), zero WLAN RRQs,
and zero WLFW servers. Registration timings and all TX wait counters match
the intermediate frame. The 4,958 ms wait also matches the preceding
independent-IPA boot's 4,988 ms within 30 ms. This run identifies competing
successful GLINK publications while the reply had a ready intent. It does
not establish whether earlier reply publication would change WLAN startup.
The next offline question is where the scheduler can give the ready reply
an earlier turn while preserving required GLINK control ordering and flow
accounting. A controlled timing comparison would test the effect on WLAN.

## Bounded registration reply TX turn — 2026-11-13

The opt-in `sunfish-ipa-registration-tx-turn` profile gives the exact first
registration reply one early writer attempt as a whole datagram. It uses
normal intent and same-flow eligibility, waits for the request's own GLINK
RX_DONE, and yields to a pending peer grant/ACK, protected TFTP and registry
turns, and ready QRTR credit. It can precede INIT's priority turn and ordinary
advertisements and receive completions. WouldBlock consumes the priority
attempt and leaves the reply queued for ordinary arbitration. The same write,
quarantine, and retirement paths handle publication and completion.

The regression fixture verifies the grant advertisement before ACK, QRTR
credit, request RX_DONE, registration ahead of queued INIT and ordinary
advertisement, and fallback after WouldBlock. The link suites passed 189
candidate and 188 baseline tests. Both runtime profiles passed 493 tests with
one ignored each. The panel render, source checks, Pixel build, boot-layout
audit, LZ4 round trip, and 280-mutation capability audit passed. The isolated
build copy used the predecessor's close-handoff guard exception.

The 68,800,512-byte temporary image is under
`native-ipa-registration-tx-turn-build-v1/fastboot.img` in the private cache.
SHA-256 is `df509747f6dec59b50c05dba79e366be24c6996e30dfdc6766c204d916cbc9b5`.
On a cold boot capture the settled IPA REG TX EARLY panel to compare queue
wait, competing records, IPA flags and the WLAN query/inventory against
the 4,958 ms IPA REG TX WAIT baseline. Device result pending.

The settled device capture shows a 4,986 ms queue wait (+28 ms versus the
4,958 ms baseline), 33 transmit passes, the same 29 competing control
records and three other DATA records, and zero blocked or uncertain writes.
Writer time was 2,297 ms (+13 ms). The IPA handshake completed (0x1F)
with zero runtime error/detail. The WLAN PD returned UNINIT, with zero
WLAN firmware requests and zero WLFW servers. The bounded priority turn
produced no observable timing or WLAN improvement in this boot.

The screen does not expose whether priority selected the final write or
which guard deferred priority on each earlier pass. The aggregate RX_DONE
counter covers any receive slot, and ACK presence can overlap RX_DONE.
A follow-up trace should record the request's exact RX_DONE time and the
priority decision/guard on each pass before changing control precedence.

## Registration priority guard trace — 2026-11-13

The IPA REG TX GUARDS panel timestamps publication of the exact registration
request's RX_DONE and reports whether the early attempt reached the writer.
Each queued pass counts its first deferral only: existing priority data,
protected WRQ completion, pending intent ACK, ready QRTR credit, the exact
request's pending RX_DONE, or remaining eligibility checks. The experiment
keeps the earlier-turn ordering. The host fixture verifies ACK, credit,
and exact RX_DONE deferral counts and the selection marker.

The pinned Linux and LineageOS Android GLINK sources match the recorded
reference hashes. Both advertise a newly allocated intent before ACKing
the grant and both data-send paths directly use a fitting free peer intent.
Both invoke the receive callback before their GLINK RX_DONE call. These
source paths establish local order within those operations; they do not
establish a global arbitration rule across concurrent sends. The existing
12 extracted send cases per kernel reproduced their recorded results.

The final image passed 493 runtime tests (one ignored), 189 candidate and
188 baseline link tests, the display and source checks, the Pixel build,
boot-layout and LZ4 checks, and the 280-mutation capability audit. It reuses
the isolated close-handoff build exception. The temporary image is
`native-ipa-registration-tx-guards-final/fastboot.img` in the private cache,
SHA-256 `56a67ead776e0ce7c363869231188e9f606775d9dd5992a0fd7db9980fd2cef3`.
Device result pending.

The settled device capture shows 33 queued transmit passes. The first guard
was a pending intent ACK on 21, the exact request's pending RX_DONE on 11,
and existing priority data on one. No early writer attempt was selected.
The exact RX_DONE timestamp was unavailable at capture. The reply went out
through normal arbitration after 4,961 ms (+3 ms versus baseline), with
29 control records and three other DATA records ahead. IPA completed
(0x1F) with zero runtime error/detail. WLAN PD remained UNINIT, with zero
WLAN RRQs and WLFW servers. These counts rank the first guard per pass,
not its exclusive contribution to elapsed time.

A focused next comparison is a bounded turn for the exact request's pending
RX_DONE, preserving grant advertisement/ACK ordering and existing protected
work. This directly tests whether clearing the prerequisite sooner creates
a usable early reply turn.

## Bounded registration request RX_DONE turn — 2026-11-13

The opt-in profile gives the exact queued registration request's RX_DONE
one early writer attempt when the whole reply has a usable peer intent.
Existing protected WRQ, TFTP and registry turns, a pending intent grant/ACK,
and ready QRTR credit retain precedence. WouldBlock consumes the borrowed
turn and leaves the receive completion pending for ordinary arbitration.
The existing successful GLINK commit path records its publication timestamp.
The panel adds RXDONE TURN to distinguish an attempted early completion
from an RX_DONE published through normal scheduling.

The Pixel build, layout and compression checks, panel and source checks,
493 runtime tests (one ignored), 189 link tests per profile, and the
280-mutation capability audit passed. The build uses the same isolated
close-handoff guard exception. The temporary image is
`native-ipa-registration-rxdone-turn-build-v1/fastboot.img` with SHA-256
`7d94c86214c4a2785c8b0a4470a2ebfd7a7f6bb441583b313fc2e31187dfb247`.
Device result pending.

The settled RX_DONE early-turn capture shows RXDONE TURN=1, a finite exact
request RX_DONE timestamp, PRIO SELECTED=1, and zero blocked or uncertain
writes. Queue wait fell from 4,961 ms to 1,137 ms (77% lower), with eight
publish passes versus 33. The exact RX_DONE was sent about 984 ms after
queue admission, and the reply followed about 154 ms later. Six control
records (one advertisement, two RX_DONE, three intent ACKs) and one other
DATA record went ahead. IPA completed (0x1F), runtime error/detail were zero,
and the WLAN PD still reported UNINIT with zero WLAN RRQs and WLFW servers.
This verifies a registration scheduling improvement in this boot. The panel
does not measure its timing relative to INIT retirement or the first TFTP
close, so the run does not resolve those ordering hypotheses.

## IPA startup event order trace — 2026-11-13

The IPA STARTUP ORDER panel combines registration publication, INIT
transport retirement and the first close tracked for the first TFTP
server-check transfer. It uses the existing timestamps on the same
runtime-relative microsecond clock. Values are hexadecimal and FFFFFFFF
means unavailable. The image uses the previous early RX_DONE scheduling
profile, with a display change only and the same isolated close-handoff
guard exception.

The Pixel build, boot-layout and compression checks, display and source
checks, 493 runtime tests (one ignored) and the 280-mutation capability
audit passed. The temporary image is
`native-ipa-startup-order-build-v1/fastboot.img` with SHA-256
`740bab3d05121a86931f2b4a35448f97f12a0aaa55f5d89c4a16ed2af6b80f7f`.
Device result pending.

The settled photo shows first tracked TFTP close at 21.114430 s, registration
publication at 25.083924 s, registration transport completion at 25.677293 s,
and INIT transport retirement at 25.830548 s (runtime-relative). The reply
published 746.624 ms and completed 153.255 ms before INIT retired. The first
tracked close preceded reply publication by 3.969494 s. Queue wait displayed
1,134 ms with eight passes and both early-turn markers set. IPA completed
(0x1F) with zero runtime error/detail, but WLAN PD remained UNINIT with zero
WLAN RRQs and WLFW servers. Earlier registration than INIT retirement was
achieved and was insufficient for WLAN startup in this run. The first TFTP
close remains an earlier milestone for comparison with the reference boot.

## First server-check close compared with working references

Both saved Linux capture summaries and their artifact hashes were verified.
From the first 73-byte write request to the same peer's 39-byte OACK, nine-byte
DATA and final ACK, the socket timestamps were 468/845/944 us in the locator
capture and 476/935/1,036 us in the earlier GLINK capture. The locator boot
then returned file-not-found to modem_efs and two ota_firewall/ruleset reads,
and the first named WLAN RRQ reached the socket about 692 us after the likely
ELF locator reply's matching GLINK completion. Native already has exact
missing-read responses for those paths. The Linux metadata does not provide
a matching remote DEL_CLIENT timestamp or establish the QMI identity of the
adjacent 48-byte outbound packet.

FIRST CLOSE US on native is receipt of QRTR DEL_CLIENT for the tracked modem
peer. The latest boot puts it 3.969 s before IPA registration publication.
Earlier detailed native captures show OACK publication after roughly 304–459
ms, the peer FIFO tail advancing over the OACK, a retry and DEL_CLIENT with
no matching DATA. The exact OACK RX_DONE was still outside the inbound ring
at close in the cursor-trace boot. Prior native experiments advanced the
WRQ's host RX_DONE and separately reused its intent, yet the first peer still
closed without DATA. Some later native peers completed the write. Those
details come from earlier boots and are not measurements of the latest boot.

The saved Lineage kernel log shows vendor TFTP startup at 31.91–32.02 s and
the WLAN firmware-ready event at 46.11 s. Its proprietary vendor TFTP
implementation and the available boot logs provide no comparable first
WRQ/OACK/DATA/DEL_CLIENT timeline. The source manifest confirms the vendor
binary and init service. Android therefore corroborates successful later
bring-up but cannot attribute the first transfer behavior.

The first native server-check failure is a reproducible divergence from
working Linux, but there is no proof the modem gates WLAN on this transfer.
Before another device run, compare the full reference exchange offline
against the native transport, including the QRTR source and destination
ports, adjacent control traffic, and the unidentified Linux 48-byte packet.
Linux uses service 0x4000 and transfer 0x4007 in the locator capture; native
uses service 0x5000 and first transfer 0x5001. Later native transfers work
with the native port scheme, so the port difference alone does not explain
the first failure. No phone action was needed for this comparison.

### QRTR endpoints and adjacent packets

Both Linux socket captures show the same route: WRQ 0:0x1e to 1:0x4000,
OACK 1:0x4007 to 0:0x1e, DATA back to 1:0x4007, and final ACK from that
same transfer port. The first detailed native route is WRQ 0:0x1f to
1:0x5000 and OACK 1:0x5001 to 0:0x1f. The service/transfer split and
reply direction agree. Linux allocates ephemeral ports starting at 0x4000;
the observed port numbers are not constants in the QRTR protocol. Later
native DATA reached native transfer ports, so this does not show a bad
port range. It leaves first-client-specific behavior untested.

Both Linux GLINK windows show 108-byte RX, 44-byte RX, 48-byte TX and its
matching RX_DONE, then 72-byte OACK TX. The 48-byte packet completed
257–430 us before OACK TX. In contrast, the native header window identified
a 44-byte IPA registration request 654 ms after OACK and no 48-byte reply
between WRQ and client close. The latest native boot confirms registration
publication after close. The Linux 48-byte packet is consistent with an
IPA reply but remains unidentified. A standard QRTR v1 RESUME_TX or
DEL_CLIENT with its full 20-byte control body would be 52 bytes, not 48.
The 52-byte records after Linux DATA/ACK have no decoded control opcodes
in this capture and cannot be assigned to either client. The native
first-close marker is a decoded peer DEL_CLIENT; it carries a node and
port, not a TFTP status or reason.

Linux completed the write with WRQ RX_DONE on either side of OACK. In both
captures the modem's matching OACK RX_DONE was visible before the next
44-byte receive associated with DATA. On the earlier native cursor trace,
that exact completion was still absent at peer close even though the
modem's read tail had passed the OACK. This points to a difference after
the modem reads the record, without identifying whether its TFTP client
accepted it or why it closed. Earlier native WRQ RX_DONE and intent reuse
did not restore first-client DATA. The first native WRQ in the detailed
compare did not request QRTR confirmation. Linux's saved GLINK trace has
no QRTR confirm bits, so it cannot establish a Linux credit difference.

The next discriminating evidence would be a header-only reference
capture that identifies the 44/48-byte request/reply pair, the confirm
bits and the nearby 52-byte control opcodes. The two saved captures
cannot resolve these identities by size. No device action in this review.

### Header-only Linux reference capture preparation

The existing stage0-v2 image and RAM-backed reference inputs were reused.
Its kernel has no kprobe or fprobe support. An instrumented replacement
qrtr-smd module was compiled against the pinned Sunfish kernel build and
checked for matching AArch64 vermagic. It adds a scalar tracepoint at
QRTR transport receive/send entry, recording direction, version, endpoints,
confirm bit, record size, control opcode and DEL_CLIENT/RESUME_TX keys.
For well-formed candidate 44/48-byte DATA it records QMI kind, transaction
and message ID. It emits no packet payload. The existing private instance
correlates these events with GLINK intent/RX_DONE and socket metadata.
Tracing can perturb timing. The module and runner are in
`qrtr-header-reference-prepared-v1` under the private native Wi-Fi cache.

Bounds and header decoding passed ASan/UBSan fixtures. The module built
against the exact configuration, generated shell scripts passed syntax
checks, and the runner's loss checks and first-WRQ report were exercised
offline. The temporary boot and USB capture remain to be run on the Pixel.

The USB reconnect exposed the temporary Sunfish Linux kernel. The prior
runner had timed out during USB discovery. The fresh RAM boot generated a
new SSH host key, so the capture-local pin was refreshed after identifying
the directly connected USB gadget and verifying kernel and board identity.
The runner was resumed on the same boot with the observed en15 interface
because ifconfig reported inconsistent MAC values to the child process.
The provider-ready continuation made the first modem start. Capture exit
was zero and all 24 loss counters were zero.

In run/first-wrq-window.txt, the 48-byte TX at 248.450248 is an IPA
indication registration response (QMI kind 2, transaction 1, message 0x20)
from 1:0x4001 to 0:0x1f. Its 44-byte request arrived 240 us earlier. Matching
GLINK RX_DONE arrived at 248.450333, 326 us before the OACK header event.
The first transfer is 0:0x1e to service 1:0x4000, then transfer 1:0x4007.
DATA followed the OACK and the final ACK followed DATA. The adjacent
52-byte QRTR control records are DEL_CLIENT for host transfer 1:0x4007
and modem client 0:0x1e, in that order, after the final ACK. The nearby
52-byte type-1 records are DATA-class traffic on other transfer routes.
The first WRQ, OACK, DATA and ACK all had confirm=0 and no RESUME_TX
appeared in the window. This proves the ordering in this Linux boot
and identifies a controlled native experiment, not a dependency inside
the modem. No further reference boot is needed for these packet identities.

### Native IPA completion before first OACK comparison

The opt-in native diagnostic holds the original first server-check OACK in
the transmit queue until the matching registration reply retires. It
excludes that OACK from intent request selection while held. A matching
client close cancels an unsent OACK and returns its flow reservation.
Receive and unrelated transmit work retain their normal scheduling.
The panel compares registration completion, first WRQ/OACK/OACK completion,
matching first DATA and close on one clock, with WLAN state and counters.
A close before the gate opens would leave the Linux ordering untested.

The full runtime suite passed 493 tests with one ignored. Link tests passed
190 candidate and 189 baseline, including a session replay of completion
release and peer-close cancellation. The Pixel build, layout, LZ4, display,
source and 280-mutation capability checks passed. The isolated build used
the existing close-handoff guard exception. The verified temporary image
is `native-ipa-before-first-oack-final/fastboot.img` in the private cache,
SHA-256 `0172af907c0e958c5aa026be4c5b7b68660b7aae4b91212c882450489c855fbb`.
Device result pending.

The settled native capture shows the first client closed about 1.70 s
after the first WRQ and about 12.65 s before the matching registration
reply completed. First OACK, OACK completion, and matching DATA timestamps
are all unavailable. The reply queue wait was 9,710 ms across 68 passes,
versus 1,134 ms across eight passes in the prior startup-order boot.
Both early-turn markers are set. IPA completed with zero runtime errors;
WLAN PD was UNINIT with zero firmware requests and zero WLFW servers.
The client closed while the OACK was withheld, so this boot did not
exercise the working Linux sequence of registration completion followed
by OACK to the still-live first client.

### Offline comparison of registration scheduling

Queue admission was at 23.950 s in the startup-order boot and 24.021 s
in the held-OACK boot, about 71 ms apart. Queue-to-request-RX_DONE grew
from 981 to 7,322 ms (+6,340 ms); request-RX_DONE-to-reply-publication
grew from 153 to 2,378 ms (+2,225 ms). These account for the full
8,565 ms timestamp-derived increase. The cumulative writer figure across
all observed passes grew from 554 to 4,717 ms, and the remaining queue
interval from roughly 580 to 4,993 ms. Both recorded zero blocked or
uncertain writes. Reply publication to matching transport completion
actually fell from 593 to 48 ms.

Pending intent ACK was the dominant first guard in the slower capture.
The scheduler gives a pending intent grant/ACK precedence over both
registration priority attempts. Gate counts are observations, not
distinct ACKs or an elapsed-time attribution. The held-OACK change
excludes that OACK from normal and intent-request selection and cancels
it on close; it does not change the priority predicates for the two
registration records. The aggregate traces cannot identify which wire
records or receive/dispatch work filled the extra passes, or establish
that withholding the OACK itself caused the extra control traffic.
In both boots the first peer closed before the registration reply was
even queued. Optimizing the post-queue arbitration alone cannot reproduce
the reference sequence within the observed first-client window. The
existing snapshots leave the exact registration receive-to-queue
breakdown unavailable in these two boots. No device action in this review.

### Offline registration receive-to-queue review

The closest saved hardware trace with all pre-queue timestamps is the
independent-IPA boot. It routed the request 6 us after receive, and the
first TFTP close followed receive by 585 ms. Preparation completed 285 ms
after close; admission took another 2,565 ms, placing it 2,850 ms after
close. Both occurred before INIT transport retirement. The two latest
screens lack receive and preparation timestamps; this older measurement
cannot attribute the exact pre-queue intervals in either newer boot.

In the composed runtime the router's early handoff is evaluated after
the iteration's checked transmit pass. It requires an existing IPA core,
the exact next server request (or its prepared pending reply), and the
server's turn when the client can progress. An acceptable held INIT
response can also take the shared prelude. If the core is discovered with
the registration already next, two bounded continuation steps try to
prepare and admit INIT before returning to the normal service loop.
The independent scheduler advances at most one socket action per step:
registration preparation and queue admission are separate steps, with
transport and other service work able to run between them. Admission
uses the checked FIFO gate and records actual FlowBusy/NoCapacity
failures separately. A prepared server reply can progress while INIT
awaits retirement; the serialized pre-independence dependency is not the
current prerequisite. The existing independent scheduling and early-turn
host fixtures passed again.

A next comparison can reuse the existing route/first-step snapshot,
discovery and early-turn timestamps, and add the first admission-attempt
boundary alongside existing preparation, successful queue and close
timestamps. This would distinguish waiting for a service turn from
attempted admission or gate work. No device action was needed for this
review.

### Registration receive-to-queue device trace

IPA REG RX TO QUEUE records receive, route, preparation, the exact reply's
first queue attempt before FIFO gate validation, successful admission and
first TFTP close on the same runtime-relative microsecond clock. The first
attempt is latched across retries. FlowBusy and NoCapacity are separate
counts of admission rejections. The existing held-first-OACK experiment
remains active for comparison. The build uses the previous isolated
close-handoff guard exception.

The runtime suite passed 493 tests with one ignored, including the updated
owner fixture verifying preparation, attempt and admission order. The
Pixel build, display, source, boot layout, LZ4 and 280-mutation capability
checks passed. Image: native-ipa-registration-admission-build-v1/fastboot.img
in the private cache, SHA-256
894861620875474dc768c8e29ed1d84d9ebaf09b14ddee3098f84d6a9d73e33c.
Device result pending.

The settled capture shows routing 6 us after receive, preparation about
0.865 s after routing and about 0.284 s after the first TFTP close.
The first queue attempt followed preparation by about 2.60 s. It succeeded
in about 11.5 ms with zero FlowBusy and NoCapacity rejections. The later
queue-to-publication wait was 9,686 ms. No first OACK or DATA was observed.
IPA completed with zero runtime errors; WLAN PD was UNINIT, with zero WLAN
RRQs and WLFW servers. The largest pre-admission gap was before the first
attempt, and eliminating that gap alone would still miss this first close.

### Registration routing-to-preparation work trace

IPA REG PREP WORK records the route-time IPA state, early yield, first
owner step and result, INIT discovery and queueing, early registration
step, and preparation on the same runtime clock as first TFTP close.
It reports inclusive wall-clock durations for router, RMTFS, IPA,
display, locator and TFTP work clipped to the route-to-preparation
window. TFTP includes router work invoked inside its own handler;
the router row is the main dispatch interval. Time outside the timed
sections is left unattributed. IPA STEP RESULT 1 is waiting for peer,
2 discovery, 3 prepared, 4 queued, 5 waiting queue, 6 waiting completion,
7 committed, 8 response accepted, 9 waiting handshake, A complete.
The held-first-OACK experiment and isolated close-handoff guard exception
are inherited.

493 host tests passed with one ignored. Source, clipped timing boundary,
display, boot layout, LZ4 and 280-mutation capability checks passed.
The temporary image is native-ipa-registration-prep-work-build-v1/fastboot.img
with SHA-256 9cb14c500632a22fbf9427f6a86293c10b2cb9e0526ac18c8c6a9e563696b0f2.
Device result pending.

The settled preparation-work capture shows 868,488 us from route to
preparation. TFTP including nested router dispatch accounts for 391,903 us,
main router dispatch 314,539 us, IPA 115,737 us, RMTFS 23,152 us, locator
23,141 us and display zero. The first IPA step began 438,199 us after route
and discovered the peer/core in 23,137 us. Route flags 0x40 show registration
next with no core; no early receive yield was recorded. INIT queued 64,673 us
before first close. Registration's early step began 311,226 us after INIT
queued and prepared the reply 38,057 us later, 284,610 us after close.
The first queue attempt came 2,610,763 us after preparation and admission
followed 11,587 us later. IPA finished with zero runtime error/detail and
WLAN remained UNINIT with zero WLAN RRQ and WLFW. The TFTP bucket is
inclusive and does not separately attribute nested transport work.

### Offline review of an earlier cold registration turn

At route time flags 0x40 mean the request was next but the IPA core did
not exist. The current early-turn predicate therefore returned false.
Even when eligible, the existing handoff is after the checked writer
attempt in that receive iteration. The runtime then uses its ordinary
service pass for discovery and, with the exact registration waiting,
takes two local steps to prepare and attempt admission of INIT_DRIVER
before returning to the router. Registration preparation and admission
are separate later service actions. INIT retirement is not required by
the independent server path.

An isolated experiment can give the exact routed registration a one-shot
handoff before the next writer attempt when the normal service-discovery
state already has the modem IPA endpoint. The ensuing bounded service
turn can use the checked owner step for discovery and give the server
one preparation and one admission attempt before returning to normal
client/server arbitration. If endpoint discovery is still pending or
admission is blocked, yield to the ordinary loop. Keep INIT's existing
validation, deadlines, and independent receipt, and preserve intent
grant ordering and the registration reply's own RX_DONE prerequisite.
Exercise endpoint arrival in either order and WouldBlock/queue rejection
in the replay before using a new image. Do not derive the INIT peer
endpoint from the incoming registration's source port.

The measured routing-to-close budget was 583,878 us. In the previous
unheld-OACK run even the improved queue-to-publication interval was
about 1.13 s. An earlier admission is therefore a measurable milestone,
while reproducing the Linux exchange still depends on downstream
publication and receipt timing. No code or device action in this review.

### One-shot cold IPA registration handoff

When the first routed registration is next, the core is absent and the
normal modem IPA endpoint is known, the router yields before publication
after a receive. The one-shot service uses ordinary checked IPA steps for
discovery, server preparation and one admission attempt. Either arrival
order is supported; until both inputs exist normal progress continues.
The next ordinary turn can prepare INIT. Existing deadlines, checked FIFO
admission, transport arbitration and receipt processing remain in use.
The held-first-OACK experiment and isolated close-handoff build exception
are inherited.

The real owner/FIFO replay tested both arrival orders, an absent endpoint,
one-shot consumption, admission before INIT, and normal INIT service
afterward. 494 host tests passed with one ignored. Source, panel,
boot-layout, LZ4 and 280-mutation capability checks passed. Temporary image:
native-ipa-registration-cold-handoff-build-v1/fastboot.img, SHA-256
441f55349f1f9cf385e96786b7564827de56fdaf511727a3b64005c1d94af88e.
Device result pending.

The settled cold-handoff capture shows a recorded yield, then discovery,
then INIT queue admission before registration preparation. The observed
order differs from the intended one-shot replay. The panel does not log
handoff consumption or its individual step outcomes. Registration was
prepared 168,343 us before the first TFTP close, admitted 633,118 us
after it, and completed 11,118,713 us after it. Preparation-to-attempt
was 789,908 us. IPA finished with zero runtime error/detail, and WLAN
remained UNINIT with zero firmware requests and WLFW servers. The next
offline check is to reproduce the observed INIT-first path through the
full runtime dispatch and service sequence before another boot.

### Offline replay of the INIT-first handoff

The main runtime checks the early IPA handoff before TFTP service. The TFTP
handler can then dispatch the router again on first OACK admission, including
with the OACK held at transmit selection. If this nested dispatch receives
the registration and discovered endpoint, it arms the cold handoff after
the early check. The late branch still sees early_ipa=false and performs
ordinary IPA discovery plus the two INIT preparation/admission steps.
At the next early check the cold handoff is consumed and returns false
because the core now exists. Ordinary early registration can then prepare
the reply, with queue admission on a separate turn.

The host replay uses the real Router, Owner, FIFO and TFTP service and
mirrors the runtime service order. It reproduces a recorded yield, INIT
admission before registration preparation, and no registration queue
attempt in that preparation step. The full host profile passed 495 tests
with one ignored. The device panel does not identify the nested dispatch
directly; this is a reproduced path consistent with its event order.
A follow-up should service a pending cold handoff after nested dispatch
and before the late IPA discovery branch, preserving one-shot behavior.
No runtime code or device state changed in this review.

### Cold registration handoff after TFTP nested dispatch

The runtime rechecks the one-shot cold IPA handoff immediately after the
early TFTP service can dispatch the router. If handled, the pass skips
ordinary IPA discovery and its INIT continuation. Existing validation,
bounded admission, and transmit scheduling still apply. The updated real
Router/Owner/FIFO/TFTP replay verifies registration admission before INIT
and normal INIT preparation and admission afterward. 495 host tests
passed with one ignored, including the nested-dispatch replay. Source,
display, boot layout, compression and 280-mutation capability checks passed.
The held-first-OACK profile and isolated close-handoff build exception are
inherited. Temporary image native-ipa-registration-handoff-after-tftp-v1/
fastboot.img has SHA-256
700a9eed1d4a637dac54295b6b9a93f6f7e0616bb0388f83c4265d9ac929b88f.
Device result pending; panel title remains IPA REG COLD HANDOFF.

The settled post-TFTP handoff capture confirms registration admission
138,580 us after routing, 1,077,431 us before INIT admission and 467,056 us
before first TFTP close. Matching registration completion came 13,792,256 us
after admission and 13,325,200 us after close. The panel lacks a registration
publication timestamp, so it cannot split the interval into publication
delay and time waiting for the matching completion. Runtime error/detail
are zero. WLAN PD remains UNINIT, with zero WLAN RRQs and WLFW servers.
The next comparison should put registration publication, matching
completion, and first close together with transmit guard counts.

### Post-TFTP handoff transmit trace

IPA HANDOFF TX TRACE reuses the cold-handoff scheduling profile and
displays registration queue, publication, matching GLINK completion,
INIT admission and first TFTP close on one clock. The DEFER rows pack
first-deferral counts in label order, high 16 bits then low 16 bits.
The G rows show overlapping eligibility observations and BLOCK REG OTHER
separates blocked writes for registration and other records. The
held-first-OACK profile and isolated close-handoff build exception remain.

The Pixel build, 495 host tests (one ignored), 29 source checks, panel
render, boot-layout and LZ4 checks and 280-mutation capability audit
passed. Temporary image native-ipa-registration-handoff-tx-trace-v1/
fastboot.img has SHA-256
2e08e4aa59290693fe63d7b2583d30175593316fce7a5c8f561d2fa630b81e4e.
Device result pending.

The settled handoff TX capture shows registration queued at 20.655605 s,
466.875 ms before first close at 21.122480 s. Request RX_DONE published
14.039503 s after admission, and the reply followed 92.446 ms later at
34.787554 s, 13.665074 s after close. Matching completion followed
37.792 ms later at 34.825346 s. Both priority markers are set.
First deferrals were 88 pending intent ACK, four ready QRTR credit, two
existing priority data and one exact request RX_DONE. The ready count
was 96; intent-request wait, no-fit, flow predecessor and writer-block
counts were zero. The 88 is a pass count, not a count of distinct ACKs.
First OACK and DATA were unavailable. IPA completed (0x1F) with zero
runtime error/detail. WLAN remained UNINIT with zero RRQs and WLFW servers.

### One-shot registration RX_DONE between intent grants

IPA RXDONE GRANT TURN retains the cold handoff and held-first-OACK
profile. After a successful intent ACK, the exact queued registration
request can take one RX_DONE attempt before the next grant advertisement.
The existing eligibility, WRQ, priority data and QRTR credit checks remain.
A blocked write consumes that borrowed turn and normal arbitration resumes.
The following grant still requires its exact backing advertisement before
its ACK. The replay verified both successful and blocked RX_DONE cases.

191 candidate and 190 baseline link tests passed, and 495 host runtime
tests passed with one ignored. Source, display, boot-layout, compression
and 280-mutation capability checks passed. The isolated close-handoff
build exception is inherited. Temporary image
native-ipa-registration-between-grants-v1/fastboot.img has SHA-256
36bed9107198224f3cdfa6ab558ddadd08b6f9e484703b47ddb63867853a7c45.
Device result pending.

The settled grant-turn capture shows request RX_DONE 738,142 us after
registration admission, 13,301,361 us sooner than the previous boot
measured from admission. RX_DONE still followed first close by 269,565 us.
Reply publication followed RX_DONE by 4,013,005 us, placing publication
4,282,570 us after close and completion 4,877,916 us after close.
Both priority markers were set. A pending intent ACK was the first
reply deferral on 29 passes, and existing priority data on one. The
reply was ready on 31 observed passes and no blocked writes were recorded.
The trace aggregates these deferrals across the full queued interval.
First OACK and DATA were unavailable. IPA completed with zero runtime
error/detail; WLAN remained UNINIT with zero RRQs and WLFW servers.

### Registration reply turn between grants

IPA REPLY GRANT TURN lets the exact registration reply borrow one writer
attempt after a successful intent ACK while preserving its own request
RX_DONE prerequisite, fitting intent, flow order, and protected turns.
The next grant still requires its exact backing advertisement before ACK.
The existing one-shot and WouldBlock fallback apply. The profile retains
the RX_DONE grant turn and held-first-OACK comparison.

192 candidate link tests and 495 host runtime tests passed (one runtime
test ignored). Source, display, boot-layout, compression and the
280-mutation capability audit passed. The isolated close-handoff build
exception is inherited. Temporary image
native-ipa-registration-reply-between-grants-v1/fastboot.img has SHA-256
065380c5538562635108b2e005b34a41dc3e06c1c0e7b55780e0b8304d1470ed.

The settled capture shows reply publication 1,001,028 us after request
RX_DONE, down from 4,013,005 us. First TFTP close preceded request RX_DONE
by 268,592 us, reply publication by 1,269,620 us, and reply completion by
1,798,542 us. ACK-first deferrals fell from 29 to 6, with 9 flow-ready
observations and zero request no-fit or blocked writes. First OACK and
DATA remained unavailable. IPA completed with zero runtime error/detail.
WLAN remained UNINIT with zero WLAN RRQs and WLFW servers.

### Registration request RX_DONE writer trace

IPA RXDONE TURN TRACE records publisher decisions from receipt of the
first exact IPA registration request through its successful RX_DONE.
The first eligible timestamp is the first pass that selected the one-shot
priority RX_DONE, even if its writer blocked. It shares the runtime
counter with the first TFTP close and registration transmit milestones.
PASSES AT CLOSE is the number of completed trace passes when the
publisher first observes the first client's close, while tracing is
active. FFFFFFFF means unavailable. The gate pairs count overlapping
observations across the interval, high half first.

Each TURNS word encodes ten decisions with the earliest in bits 2:0.
Codes are 0 idle, 1 advertisement, 2 intent ACK, 3 other RX_DONE,
4 QRTR credit, 5 other DATA, 6 intent request, and 7 target RX_DONE.
The failure bitmap follows the same pass indices. The panel retains
the first 40 decisions and reports the full pass and failure counts.

The local mainline reference sends RX_DONE from a worker. The local
LineageOS sunfish kernel first tries a nonblocking RX_DONE when its
completion queue is empty and falls back to a worker. Both advertise
newly allocated receive intents before granting them. The LineageOS
handler can also grant an intent request using an existing reusable
intent. These are implementation references for the native trace.

The diagnostic image passed 192 GLINK tests in each feature configuration,
495 runtime tests with one ignored, 29 source tests, the panel render,
LZ4 round trip, boot-layout checks and 280 capability mutation checks.
The isolated build retains the previous close-handoff exception.
Temporary image native-ipa-registration-rxdone-trace-v1/fastboot.img has
SHA-256 899c254c6a08ff8912230adcd13881e5a7e5544afc0a66dc5fcef99d7d01fc04.

The settled device trace records three passes: intent advertisement,
intent ACK, registration request RX_DONE. Only the advertisement pass
preceded the first TFTP close. The pending-grant gate was set on the first
two passes, with no other recorded gate or writer failure. Priority
RX_DONE first became eligible at 21,320,690 us, 199,290 us after close
and 666,091 us after queueing. It published 69,237 us later, 268,527 us
after close. Registration reply completion followed close by 1,798,242
us. First OACK was unavailable. WLAN remained UNINIT with zero firmware
RRQs and WLFW servers.

### Registration RX_DONE before the pending grant

IPA RXDONE BEFORE GRANT lets the exact registration request borrow
one writer attempt ahead of the pending grant. Its existing eligibility
checks still apply. The grant ACK still waits for its backing
advertisement. WouldBlock consumes the priority attempt and normal
arbitration resumes. The host replay reproduces the previous three
turns and verifies the new success and blocked-write sequences.

192 candidate and 193 baseline GLINK tests passed. The runtime suite
passed 495 with one ignored. Source, panel, compression, boot layout,
and 280 capability mutation checks passed. The isolated close-handoff
build exception was reused. The temporary image is
native-ipa-registration-rxdone-before-grant-v1/fastboot.img, SHA-256
40661c005ce31aa74b0398c6ad8b72affdb026d03440aada144b52c67f41b7f6.

The settled device trace records target RX_DONE on the first eligible
pass, with zero writer failures. It published 371,624 us before the first
TFTP close, a 640,151 us improvement relative to close over the prior
boot. The registration reply published 640,385 us after RX_DONE and
268,761 us after close. Matching completion followed close by 4,154,011
us. No first OACK published. The pass count at close is unavailable
because the RX_DONE trace had already stopped. WLAN remained UNINIT
with zero firmware RRQs and WLFW servers, and runtime error/detail
were zero.

### IPA registration reply FIFO trace

IPA REPLY FIFO TRACE reports reply writer passes and competing records
after the exact request RX_DONE. It follows the exact reply's final TX
boundary using existing validated FIFO cursor samples and records the
first observed peer-tail crossing, last observed empty RX FIFO, maximum
observed unread RX bytes, and host receive-poll count and maximum gap.
These are host observations. The crossing is transport consumption, not
QMI acceptance, and the samples cannot directly date the modem's send.
Observation ends at matching registration transport retirement.

The temporary image passed 192 GLINK tests in each profile, 495 runtime
tests with one ignored, 29 source tests, panel render, LZ4 round trip, boot
layout and 280 capability mutations. The isolated close-handoff exception
was reused. Image native-ipa-registration-reply-path-v1/fastboot.img has
SHA-256 546e788362b6dae2176453873c8d3c9c0fa8912b3bae65018dbcb7a034f7f99c.

The settled panel records one intent advertisement, one intent ACK,
and registration reply publication across three writer passes after
request RX_DONE. No other commands, DATA, idle passes or blocked writes
were recorded. The first pass started 260,839 us after request RX_DONE.
Publication followed request RX_DONE by 640,267 us and the first TFTP
close by 268,708 us. The peer TX read cursor passed the reply boundary
11,559 us after publication. The matching completion was processed
3,884,361 us after publication. The RX FIFO was sampled empty 49,846 us
before completion. There were 29 receive polls, a 260,820 us maximum
poll gap, and a peak of 128 unread bytes. The last empty sample rules out
the matching record sitting in the sampled FIFO throughout the wait.
First OACK was unavailable. WLAN remained UNINIT with no WLAN RRQs or
WLFW servers.

### Native IPA reply header window

IPA REPLY HEADERS displays the exact opening registration reply, the
first 18 intervening records, and the matching completion on the last
record row. DROPPED counts intervening records beyond display capacity.
The first line of each entry is elapsed microseconds, TX flag (1 TX,
0 RX), GLINK command, channel, parameter, and padded GLINK record size.
The second line is QRTR v1 type, source port, destination port, QMI
kind, transaction, and message ID. All values are hexadecimal. All-F
fields are unavailable. QRTR is decoded on first fragments and QMI
only on complete decodable single-fragment DATA. Payloads are not
retained. Observations use host publication and receive processing
timestamps and retain the previous peer-tail and first-close markers.

192 link tests passed in each profile and 496 runtime tests passed
with one ignored. Source, display, LZ4, boot layout and 280 capability
mutation checks passed. The isolated close-handoff exception was reused.
Temporary image native-ipa-registration-reply-headers-v1/fastboot.img
has SHA-256 f0131b0589e25ae8d668ace176f857ee8b22497efb5c49b82a4a1a74ca17b8ab.
Device result pending.

The first header capture (IMG_9501) retained GLINK intent traffic,
a second TFTP route and two QRTR NEW_SERVER announcements before
matching RX_DONE_W_REUSE for intent 0. The peer tail crossed 11,527 us
after the opening reply record, and retirement followed 3,882,122 us
after that record. The first client had already closed. The panel
omitted 26 records before the reserved completion row.

IPA REPLY HEAD TAIL retains the opening reply and nine subsequent
records, plus the last nine records before the reserved completion.
MIDDLE OMITTED counts intervening records displaced when the window
fills. The 496 runtime tests (one ignored), 29 source tests, display
render, LZ4, boot layout and 280 capability mutation checks passed.
The isolated close-handoff build exception was reused. Temporary image
native-ipa-registration-reply-head-tail-v1/fastboot.img has SHA-256
4a7c7c6076502c05716bf35d25f2dc1aa4cf5f28ef21d3d4f57898bd39f6f126.
Device result pending.

The settled IPA REPLY HEAD TAIL capture ended with a 52-byte peer
intent request, host RX_DONE for intent 0x2d, a negative intent ACK,
and matching RX_DONE_W_REUSE for the registration reply. The matching
completion followed the negative ACK by 37,855 us. Native admission
decides on receipt, so the valid request was denied while all eight
slots were occupied, before the host RX_DONE freed one.

IPA INTENT SLOT RETRY lets a pending full-pool denial use a slot freed
by a committed RX_DONE. It advertises a fresh backing intent before
the positive ACK. The host replay covers the exact 52-byte request,
a blocked RX_DONE and a blocked advertisement. Both link profiles
passed 193 tests. The runtime passed 496 with one ignored, source
passed 29, and display, LZ4, boot layout and 280 capability mutations
passed. The isolated close-handoff build exception was reused.
Temporary image native-ipa-intent-slot-retry-v1/fastboot.img has SHA-256
fa1238b09cdc517e21ce4518920460bbe5e86313e93ed2ed090cd4728f0dee3f.
Device result pending.

### Early registry receive-slot release

IPA REG SLOT RELEASE adds one host-only entry for a registry request
whose RX_DONE has committed. The original handle and router ticket
remain valid, and an already-queued reply follows the moved entry.
If that entry is occupied, other requests use the existing lifetime.
Only the eight wire slots can be advertised. The retry may reserve
the freed wire slot for a pending request and still advertises it
before a positive ACK.

Offline replays cover moving a registry request before and after reply
admission, blocked RX_DONE, reusing the wire slot for another request,
and completing the original transaction. Both link profiles passed
195 tests. The runtime passed 496 with one ignored, and source, display,
LZ4, boot layout and 280 capability mutations passed. The isolated
close-handoff build exception was reused. Temporary image
native-ipa-registry-slot-release-v1/fastboot.img has SHA-256
bbd4ee9fd899c9a3534fc31532c0145b74e6dfc2006445d7f54d6acb2991ace9.
Device result pending.

IPA REG TWO ENTRIES adds a second host-only registry entry to the
early-release profile. Only the eight wire slots are advertised. A
full host-only pool falls back to the existing request lifetime.
The full-pool replay verifies ordered replies for transactions 3
and 4, reuse for transaction 5, and advertisement before positive
ACK including a blocked advertisement. Link tests passed 196 in
both profiles, runtime passed 496 with one ignored, and 29 source
tests, display, LZ4, boot layout and 280 capability mutations passed.
The isolated close-handoff build exception was reused. Temporary
image native-ipa-registry-two-entries-v1/fastboot.img has SHA-256
04a94de26c869b3380a17ea9970fa202b2bbbffe03e46691cc0c3a14c5b0e331.
Device result pending.

IPA INTENT DENIAL snapshots all eight GLINK receive slots and both
host-only registry entries at request ingress and successful negative
ACK publication. It retains the last such pair within the IPA reply
publication-to-completion window. Each compact row shows slot index,
request intent ID and four flag nibbles, then ACK intent ID and flags.
The nibbles are slot state, service index plus one, reply status
(queued, published, GLINK done, QRTR done in bits 0 through 3),
and pending RX_DONE. The first-denial diagnostic remains independent.
The image enables observation of the existing two-entry configuration.

197 candidate and 196 baseline link tests passed. The runtime suite
passed 496 with one ignored. Source, display, compression, boot layout
and 280 capability mutations passed. The isolated close-handoff build
exception was reused. Temporary image native-ipa-intent-denial-v1/fastboot.img
has SHA-256 bfe6bb6e39d7a63cbd4f4ece672e5cfabb77bdc554230f6cfe092332067030fb.
Device result pending.

IMG_9522 shows a 108-byte denial with identical ingress and ACK
snapshots 207,792 us apart. Four wire slots had control RX_DONE
pending, two were advertising, and the remaining two held registry
and IPA transactions. Both host-only registry entries were occupied.
The default preferred ACK bypassed ordinary RX_DONE selection.

IPA DENIAL RXDONE TURN gives a pending full-pool denial one attempt
at a Control RX_DONE that can release a wire slot. The existing
retry reserves the freed slot and enforces advertisement before
positive ACK. WouldBlock on the borrowed RX_DONE falls back to the
negative ACK. The replay verifies the baseline denial, successful
conversion, blocked advertisement and blocked RX_DONE. Both link
profiles passed 198 tests, the runtime passed 496 with one ignored,
and 29 source tests, display, compression, boot layout and all 280
capability mutations passed. The isolated close-handoff build
exception was reused. Temporary image native-ipa-denial-rxdone-turn-v1/fastboot.img
has SHA-256 aae83abbd73abff1edb48df6b892f477c8c0c152f6f965d0f4429eb4df777ee9.
Device result pending.

FIRST TFTP NO HOLD disables the IPA completion hold on the original
server-check OACK. It reports first-peer DATA, the same transfer's
final ACK publication and matching GLINK completion, and ELF reply
completion. The final ACK observer is scoped to the original WRQ
route so a later write cannot claim its trace. The close cancellation
and other transport policies still apply. The host replay verifies OACK
publication while IPA completion is pending with the hold disabled.
Both link profiles passed (199 candidate, 198 baseline), as did 496
runtime tests with one ignored, 29 source tests, the display check,
compression, boot layout and 280 capability mutation checks. The
isolated build reused the close-handoff compile-guard exception.
Temporary image native-first-tftp-no-hold-v1/fastboot.img has SHA-256
252b6c79a9488421110b9ee85abb6c1f4a98099331059becf6f916f24d27afa9.
Device result pending.

FIRST TFTP FIFO displays the existing first OACK FIFO visibility
bracket, cursor crossing, presence at close and bytes ahead in the
no-hold timeline. DONE PEER, DONE OACK US and DONE GLINK US refer to
the first completed write. Timestamps use the runtime origin. The
panel retains the original first client's final ACK and ELF ordering.
199 candidate and 198 baseline link tests, 496 runtime tests with one
ignored, 29 source checks, display, compression, boot layout and 280
capability mutations passed. The isolated close-handoff compile-guard
exception was reused. Temporary image native-first-tftp-fifo-v1/fastboot.img
has SHA-256 7b6018501ab65535c34f9d9bd065ea7cb41031f6d365b07cc0fc8bb3947e98f9.
Device result pending.

FIRST OACK HEADERS retains the last twelve recorded IPCRTR records
after the original OACK publication and before its matching GLINK
RX_DONE, plus that exact completion on a separate row. It reuses
the FIFO visibility bracket and header-only decoder. The omission
count covers earlier records displaced from the twelve-record window.
Rows use the same hex header format as IPA REPLY HEAD TAIL. Timing and
metadata do not identify the modem's internal completion condition.
The 199 link tests per profile, 497 runtime tests with one ignored,
29 source tests, display, compression, boot layout and 280 capability
mutations passed. The isolated close-handoff exception was reused.
Temporary image native-first-oack-headers-v1/fastboot.img has SHA-256
7078671e20e56df9f8282ba5bbb951cabccbfc745a55b44c3b41191d9c5a63f8.
Device result pending.

FIRST OACK CLIENT adds the body node and port to complete QRTR
DEL_CLIENT entries. The second line of those entries shows type,
header source port, header destination port, client node and client
port. The other entries retain the previous header layout. The
decoder test distinguishes body identity from control ports and
checks truncated fragments and mismatched control types.
199 link tests in each profile, 498 runtime tests with one ignored,
29 source checks, display, compression, boot layout and 280 capability
mutations passed. The isolated close-handoff exception was reused.
Temporary image native-first-oack-client-v1/fastboot.img has SHA-256
59c5c64d265dd2753590014ff53e38262d8194bbdf4cf0254d1c629c02207498.
Device result pending.

IPA REG RX VISIBILITY reuses the checked FIFO cursor history to bracket
visibility of the record completing the first validated IPA registration
request. The window starts at the first TFTP request and includes records
already queued behind it. If registration is processed first, a sample at
its processing point gives an upper bound. The OACK stays unheld.
The display compares the last absent and first visible samples with
registration receipt, first WRQ and OACK. These are host observations.
199 link tests per profile, 497 runtime tests with one ignored, source,
display, compression, boot layout and 280 capability mutations passed.
The isolated close-handoff compile-guard exception was reused.
Image native-ipa-reg-rx-visibility-v1/fastboot.img has SHA-256
0a087af5020b00e7f5ce118df5a5cffbe39fde5f77287d0d33c69af65623b1bc.
Device result pending.

IPA REG RX FIRST gives the first queued pre-ELF server-check OACK a
bounded receive-first window. The owner can skip at most eight normal
writer turns after receive progress while looking for the already-queued
registration. Idle or partial reads fall through, and intent requests,
TFTP requests and transmit completions keep their normal path. The OACK
does not wait for IPA completion. The actual owner/router replay showed
registration processed and handed off before OACK publication. 199 link
tests per profile and 497 runtime tests (one ignored) passed, as did
source, display, boot-layout, compression and 280 capability checks.
The isolated close-handoff compile-guard exception was reused.
Image native-ipa-reg-rx-first-v1/fastboot.img has SHA-256
811a3ea5a7ecf6f22a943050049f9b075eb85b42e18e544643385b6d79cc6105.
Device result pending.

## OACK identity and reference review — 2026-11-17

Settled IMG_9585 shows peer port 0x49 and reserved OACK peer port 0x49,
both using transfer port 0x5005. Peer-trace publication 0x02740786 and
reserved publication 0x02740784 us differ by 2 us. The completion observer
armed at 0x0274078f us with 0x88 unread RX bytes; its visible-at-arm state
must not be called an exact packet-arrival timestamp. The TX peer tail
crossed at 0x027434bf us, 11.579 ms after reserved publication. The client
close and matching completion became visible in the same cursor sample at
0x027e1205 us, 658.049 ms after publication. Close was processed at
0x027e8704 us and the reserved slot's intent-5 completion was processed at
0x029b9b94 us. This establishes host-side FIFO visibility/processing, not
TFTP acceptance. No matching DATA, WLAN RRQ or WLFW server was reported.
The earlier claim that close/completion visibility preceded publication was
based on an OCR reading of IMG_9583 and is not supported by IMG_9585.

Offline comparison: the saved Linux trace reports server-check WRQ to OACK
send 476 microseconds and same-peer DATA at 935 microseconds, measured at
Linux socket hooks. A previous native IMG_9581 inventory reports 0x02659e79
to 0x02742964 us, about 953.067 ms from peer-0x49 WRQ processing to OACK
publication; those are different boot runs and observation points. The
pinned-source OACK oracle was rerun and passes for the generated 39-byte
model, but it does not capture native hardware's actual OACK payload. The
known native server limitation (negotiated 7680-byte block yet 512-byte
DATA limit) remains material for later transfers, but cannot explain why no
DATA from this peer reached its handler. An earlier client in a separate
native run did complete a small write. Focus further work on the initial
request-to-OACK latency and actual outbound envelope plus peer traffic,
without assuming the TFTP close proves the cause of absent WLAN startup.

### Combined delay and outgoing envelope diagnostic

The OACK queue path requires a free TX slot and an available per-peer QRTR
flow reservation; publication additionally observes GLINK writer ordering,
remote intent and flow eligibility. The earlier 953 ms photograph does not
attribute that whole interval to one of these checks. The new temporary
TFTP OACK DELAY WIRE panel reports the matching request receipt, request
RX_DONE, reserved reply queue, publication, peer-tail, close and completion
on one clock. It also records the first 72 bytes of the actual QRTR datagram
from the reserved TX record after successful publication, its total aligned
length and peer intent; the FIFO writer checks readback. The datagram bytes
are displayed as little-endian words and include only QRTR header and the
fixed server-check OACK. This does not establish modem application acceptance.
The packet-snapshot regression compares the recorded bytes with the selected
GLINK frame and the remote intent, in addition to the pinned Linux OACK model
comparison. Both link profiles passed 201 tests; the transport suite passed
504 tests with one ignored. Target build, boot layout, checksum and capability
audit with 280 rejected mutations passed. The image retains the documented
isolated close-handoff prototype guard exception. Device result is pending.

### IMG_9586 queued-OACK offline follow-up

In IMG_9586, WRQ receipt 0x0265ef11 us, reserved queue 0x02691002 us,
WRQ RX_DONE 0x0270c400 us, and publication 0x02747abe us yield 205.041 ms
receipt-to-queue, 504.830 ms queue-to-WRQ-RX_DONE, and 243.390 ms from
WRQ-RX_DONE to publication. The same-run measurement does not establish
that the first two events causally blocked publication. The user confirmed
the photographed word was 736d7475; with that reading the displayed QRTR
header and 39-byte OACK agree with the fixed native/pinned-reference model.
The completion ID matched the captured remote intent (5). This is a local
publication and completion check, not evidence of TFTP-level acceptance.

Offline inspection: ordinary GLINK selection includes pending receive-done,
intent advertisement/ACK, prioritized work, and normal transmit selection;
ready_remote_for_transmit also checks the experiment's waiting outbound
intent and older same-flow transmit. The bounded port-0x49 retry paths preserve
grant advertisement-before-ACK and QRTR credit ordering. The existing
linuxlink_oack_prime_handles_pending_grant_and_full_receive_pool replay passes,
including blocked writer fallback; that proves its modeled cases, not which
condition held the physical 0x49 reply in IMG_9586. The general OACK queue
counter observes first/ELF traces, not this reserved 0x49 slot. No per-turn
reasons, no-fit observations or gaps for the 748 ms window were displayed,
so its exact attribution cannot be recovered from this photograph. If a
further device observation is authorized, target counters on this exact
reserved TX slot from queue through publication, including competing writer
choices, no-fit/flow, blocked writes, and maximum inter-pass gap; avoid
changing writer ordering in the diagnostic.

### Reserved OACK writer-choice diagnostic

The opt-in TFTP RESERVED WRITER image observes the exact reserved port-0x49
TX slot from queueing until successful publication or cancellation. It records
writer-pass count, first pass, maximum gap (including queue-to-first-pass),
active/writer time, remote-intent fit, ready status, outbound intent wait and
older-flow predicates. It counts successful advertisements, intent ACKs,
receive completions, other TFTP/data and OACK selection, blocked writes,
idle/request choices and bounded-followup yields. IDLE REQUEST packs idle in
the upper 16 bits and intent requests in the lower 16 bits. A yield means a
bounded followup declined the selected record; it is not a committed write.
Counts are per observed pass and eligibility categories can overlap. The
instrumentation takes extra clock reads and does not change writer selection.
The timing fields are runtime-relative, with FFFFFFFF meaning unavailable.

The link candidate and baseline each passed 201 tests, with a replay checking
successful selected OACK and a blocked attempt. The runtime suite passed 504
tests (one ignored). The target compiled, the panel was verified in the ELF,
and boot-layout, checksum and capability audit (280 rejected mutations)
passed. The temporary diagnostic retains the previously documented isolated
close-handoff prototype compile-guard exception. The Pixel result is pending.

Settled IMG_9587: WRQ 0x0263a393 us, reserved queue 0x0266cf27,
first pass 0x0266fc42, original WRQ RX_DONE 0x026e6e0b, OACK publication
0x0272212a. Queue-to-publication was 741.891 ms; queue-to-first-pass
11.547 ms. Eight observed passes total 415.631 ms active, including
415.509 ms attributed to writer callbacks; the largest inter-pass gap was
173.055 ms. Counts: one advertisement, two intent ACKs, one RX_DONE,
one other DATA and one OACK selected, plus two bounded-followup yields;
no idle/request, blocked writes, no-fit, outbound-intent wait, or older-flow
observations. The exact reply was eligible in all eight observed samples.
These are sampled pass states and aggregate callback time, not continuous
eligibility or attribution to a specific I/O operation. No WLAN RRQ or WLFW
server was observed. Instrumentation may perturb timing. Device result does
not prove the modem accepted the OACK or explain its later close.

### Reserved writer callback I/O diagnostic

Offline review of Ring::write shows checked cursor samples, FIFO writes and
readback, barriers, head publication and notification. Earlier ELF phase-I/O
measurements implicated checked transport and domain operations in a different
exchange, so they cannot attribute the 415.509 ms callback total in IMG_9587.
The TFTP WRITER IO image brackets only writer callbacks while the exact reserved
port-0x49 OACK is pending, sampling existing CPU-side I/O counters when their
native profiler is enabled. It reports profiled, unavailable and invalid
snapshots; four disjoint broad categories (observation, descriptor, FIFO read,
other) and their sum; and nested write, cursor, barrier, notification and
barrier-domain details. Nested values must not be added to the broad sum. This
captures Ring::write I/O, not all time between writer-entry and return, and
extra instrumentation may alter timing. No hardware operation or scheduling
order was removed or reordered.

Accumulator host tests verify enabled deltas, unavailable counters and reversed
samples. The runtime suite passed 505 tests (one ignored); both link profiles
passed 201. Target build, panel and stack checks, boot layout, checksum and
capability audit (280 rejected mutations) passed. The temporary image retains
the documented isolated close-handoff prototype guard exception. Pixel result
is pending.

### Reserved writer cost and checked-operation review

IMG_9588 measured 415.894 ms of callback-scoped I/O in six profiled
callbacks, including 262.450 ms other I/O, 66.069 ms observation, 66.107 ms
descriptor/cursor work and 22.035 ms FIFO reads. Barrier time was 196.211 ms
nested inside other I/O; nested barrier-domain pre and post checks were
74.630 and 74.622 ms. No profiled callback was unavailable or invalid.
The maximum inter-pass gap was 173.660 ms and peer DATA, WLAN RRQ and WLFW
remained absent. These measurements establish a substantial host-side cost,
not that slow host processing alone caused the close or absent WLAN.

The current image already enables retained-table expectation, combined cursor
sampling, paired barrier/observe and adjacent barrier pairs. In the normal
non-wrapping successful Ring::write path, static inspection yields three
samples (each observe, cursor and acquire-barrier/observe), FIFO write,
release barrier and readback, a release barrier/observe, TX-head store,
completion barrier, and notification bracketed by two barrier/observe pairs:
18 checked backend operations. Each operation is wrapped in retained-domain
pre/post revalidation. FIFO wrap adds operations. The count is source-derived,
not a device trace. Linux's saved qcom_glink_tx uses locked space check,
write and kick; Linux's trust and ownership context differs, so its shorter
path is not proof that native checks can be removed safely.

One source-level candidate for further offline analysis is pairing the initial
status observation with cursor sampling under one domain wrapper, preserving
controller and protocol checks. This would remove an intermediate domain
validation boundary and thus is not established equivalent: faults or identity
changes between the two operations, partial progress, and quarantine behavior
must be explicitly modeled. No such change is implemented or booted. The
existing barrier/late-status host tests were rerun: five passed. Previously a
faster validation experiment did not, by itself, produce WLAN startup.

### Status/cursor pairing fault-order analysis

The current Ring::sample first validates an Io::observe result (generation,
identity, ready, fatal), then performs cursor_snapshot and validates its
cursor and descriptor values. Each native operation is independently wrapped
by retained-domain checks. The proposed pairing would move the second domain
pre-check after the cursor read while retaining one pre-check before the pair
and one post-check after it. Its implementation would also have to validate
status before attempting the cursor to preserve stale/unstable/identity failure
ordering. The cursor operation itself checks live peer state, lock, directory
and cursor values, but does not replace the skipped retained-domain boundary.

The new host-only status_cursor_pair_can_read_after_mid_pair_domain_corruption
fixture uses the actual native gate, controller and loader receipt. It injects
a retained table change during the status poll. The existing separate path
fails its status operation's post-domain check and does not attempt the cursor.
A hypothetical action containing glink_poll followed by glink_cursor_snapshot
attempts the cursor after the table changed and only then fails the shared
post-domain check. In both modeled cases restoring the table did not revive
the failed gate, subsequent TX writes failed, shared FIFO bytes were unchanged,
and the lock was released. Controller lock operations themselves still occur.
The fixture shows a changed read-before-detection boundary; it does not prove
that a real device incurs damage, nor prove the extra read safe under all live
fault, table and lifecycle transitions. The pre-existing barrier/observe pairing
also uses shared outer checks but does not establish this distinct pairing's
safety contract. Without an explicit argument that cursor reads during a
compromised retained domain are safe, the proposed optimization is not ready
for a device image. The targeted replay and full runtime suite pass (506 tests,
one ignored); no production behavior or device image changed in this analysis.

### Full retained-validation CPU-work review

The active retained-domain routine already uses the prevalidated private table
expectation; every boundary still reads global/context fault status, scans ASID
banks and route slots, checks final planned registers, and compares every word
of eight live 512-entry tables with that expectation. The plan contains at
most 20 steps. Its last-write filtering repeats a bounded suffix search; this
is CPU-only work, and a retained final-write bit mask might remove it without
reducing live MMIO/table reads, but would introduce another retained state and
need lifecycle and exact ordered-register equivalence tests before use.

A host-only synthetic probe compared the suffix and reverse-mask algorithms
across 100,000 generated 20-step plans with identical results. On this Mac,
one run measured approximately 132 ns for the existing synthetic plan scan
and 1,351 ns for 4,096 cached volatile reads. Those are neither Pixel device
timings nor a measurement of its uncached table accesses, and do not establish
a device speedup or prove a production implementation safe. They do show no
reason to assume that the small CPU scan dominates the 74.6 ms domain subtotal
measured across the barrier callbacks. Source is retained under
retained-validation-offline-v1/plan-scan-probe.rs.

The exhaustive native fixture still rejects corruption at every table word,
and the domain-loss fixture still stops later FIFO writes. Both targeted tests
passed. Replacing volatile live checks with cached values, skipping table
entries, widening accesses, or dropping intermediate validation boundaries
would change the current protection contract. No material, proven-safe
boundary-preserving speedup was found in this review, so no new image or
Pixel test was prepared. Next work should focus on a whole-protocol or runtime
architecture that preserves required checks and uses the current observations;
a 20-step bookkeeping change alone is not an evidence-backed device fix.

### Broader runtime and protocol review after IMG_9588

The saved Linux locator reference's five relevant artifact hashes were
rechecked. The run reports modem running and wlan0 present; its ELF/QMI
association is inferred from lengths, event ordering and timestamps, not
captured QMI identities or payloads. Its server-check exchange completed at
the socket, and a later named wlanmdsp.mbn request preceded WLFW discovery.
In native IMG_9446, a five-byte server-check write's final ACK and a
missing-file reply had matching GLINK completions, the observed locator reply
mask completed including ELF, IPA completed, and WLAN still queried UNINIT
with no observed wlanmdsp RRQ or WLFW server. Those host-side completions do
not prove modem application acceptance, and neither result establishes that
one particular port-0x49 failure causes the WLAN stall. In another working
Linux boot, mcfg.tmp access followed the first WLAN firmware request; it is
not established as a prerequisite there.

A separate real integration gap remains after a WLAN RRQ: current native
pixel_mpss_runtime calls only service_tftp_server_check_step and explicitly
does not serve WLAN firmware. This is independent of whether the peer
currently reaches that RRQ. The private wlanmdsp.mbn input exists and its
3,751,004-byte SHA-256 matches the previously observed Sunfish image. The
preflight constructs an authorized qcom_qrtr_tftp::Server using service port
0x5000 and initial firmware-transfer port 0x5100; its generic bounded server
and owner handler have synthetic host coverage. But the runtime-specific
Factory::take_runtime_boot_parts deliberately leaves that provider in the
factory, and RuntimeDomain carries only ServerCheck. A safe integration must
move its unique capability at the existing handoff, route a single advertised
service by request and exact peer/transfer identity, keep the firmware transfer
ports disjoint from ServerCheck's 0x5001..0x5020 range and all other local
endpoints, preserve per-peer retained reply/backpressure, close and generation
semantics, and keep firmware source validation and the existing checked I/O
and ownership boundaries. The generic Server uses a monotonic allocator with
only eight explicit reserved ports, so the chosen lifetime-long port policy
needs proof rather than assuming initial disjointness is enough. No such
composed runtime has been demonstrated on-device. Adding it does not explain
why WLAN RRQs are currently absent or guarantee the modem will start WLAN.

Offline tests: synthetic TFTP server/read-session suite passed 29 tests;
source/partition suite passed 11 with one live-input test ignored; the real
owner's held-RRQ and backpressure fixtures each passed. A pre-existing source
regression test was stale after the cold-handoff macro gained mutually
exclusive feature branches: it counted source text as if only one branch
existed and expected the former direct call. Its assertions were updated to
check both gated fallback paths and the macro invocation; all 29 runtime source
tests now pass. This test-only repair changes no Pixel behavior. No new safe,
fully integrated runtime change or device image is claimed in this review.
