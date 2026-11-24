# One ordinary router turn for the original final ACK

IMG_9696 narrowed the original nine-byte DATA packet's path to603.140ms before
dispatch,6.354ms through ACK preparation,18.281ms through admission, and
471.093ms from admission to publication. ACK completion followed first close
visibility by19.617ms. ELF selection waited14.746seconds while transaction4
returned FlowBusy18times. These are separate observations; the photo does not
show which packet or credit state held transaction4.

The current runtime's early TFTP dispatch helpers match newly queued OACKs and
its retained OACK retry. They do not match the final ACK. The generic early
TFTP-dispatch experiment is not enabled in this native profile and conflicts
with its publication-I/O observer. Consequently the queued final ACK can retain
a runnable router continuation across the remaining registry/storage/IPA/WLAN
service work before the outer loop dispatches again. This is a specific source
opportunity, not a measured partition of every operation in the471ms.

I found the saved transaction3/4 and MUX service-boundary replays in the paired
OACK/INIT worktree and rebuilt them against current Rust sources and features.
The earlier regular router turn still publishes transaction3 before storage
service, with idle or active storage. Transaction4 still requires the matching
completion; an unrelated completion does not admit it. The actual grant/credit
writer replay retains advertisement, positive grant ACK, request RX_DONE, QRTR
RESUME and transaction3 reply ordering. These are rechecks of existing work,
not newly discovered fixes or proofs that transaction3 blocked IMG_9696.

A separate coherent owner/router replay shows both OACK-only helpers returning
false for an accepted, queued original final ACK. A regular checked router
turn then publishes the exact ACK, with and without a pending52-byte grant.
An older fixture that mixed its simple IRQ recorder with a separate runtime
router produced UnexpectedAck; it was replaced with consistent router ownership.
An impossible synthetic tail jump was rejected as PeerOverrun rather than used
to manufacture FIFO pressure. The final backpressure test uses valid RX backlog
and normal fallback, without changing cursor validation.

The opt-in `sunfish-original-final-ack-turn` change lends one regular router
dispatch to the original **single-DATA** server-check ACK after admission. It
requires a retained continuation and no other-owner notification. The one-shot
is consumed before the call, including a deferred or faulted attempt. No loop
forces a particular packet, and ordinary continuation owns subsequent retries.
The new callback uses the same checked receive processing, grant arbitration,
QRTR credits, readback/barriers, generation, IRQ rearm and quarantine paths.
The runtime resolves a firmware-close handoff after this dispatch as it does
for other early router turns. Registry scheduling itself is unchanged.

Review caught an indirect timing dependency in the initial guard: the display's
historical original-view getter sorts inventory by `wrq_at`. Scheduling now uses
a separate clock-free predicate that indexes the captured transfer port and
checks original owner/generation/intent, full peer, transfer, accepted final
DATA, request-aware queue proof, final block, active TX slot/type/routes and
exact four-byte ACK. The old display selection remains unchanged. Regression
cases use an unstamped or late-stamped original row beside an earlier second
row, and clocks that panic if read. The same ACK stays eligible independently
of those times. Seventeen identity/packet/state mutations reject readiness.

A multi-block transfer whose first DATA is not final keeps normal scheduling;
this optimization covers the short original transfer physically observed here.
Missing inventory/proof also keeps normal scheduling. It does not use a guessed
client deadline or infer remote TFTP application acceptance from GLINK RX_DONE.

The new **ACK TURN ELF BLOCKER** panel keeps DATA/ACK and ELF phase timestamps,
replacing DATA RETURN with ACK TURN (attempted0/1, not success). It replaces the
initial registry run display with transaction4's existing last-pressure record:

| Field | Meaning |
|---|---|
| TX4 LAST WHY | Last queue rejection:1same-peer TX,2TX-slot capacity,3legacy QRTR-flow rejection,4reference-flow busy,5reference-flow capacity;0other/unclassified |
| TX4 PEER TXN | Transaction of the peer registry reply in that snapshot, orFFFFFFFF |
| TX4 KIND FLAGS | `(peer_kind << 8) | flags`; kind4is a reply; flags bit0started,bit1fully published,bit2GLINK done,bit3QRTR done |

This is the last failed check, not the entire18-retry history. In particular,
FlowBusy can come from a same-peer record or the separate reference-credit
admission path. The photo's outcome count alone cannot distinguish them.

The lifecycle test covers four combinations of pending grant and valid RX
backpressure, plus OACK exclusion, other-owner deferral, one-shot behavior and
ordinary fallback. Wire checks require a positive grant after its sufficiently
large advertisement and the exact final ACK's source node/transfer port,
destination node/port, length and block. Two actual-source renderer fixtures
exercise feature-on/off, missing state, pressure, clocks and dual framebuffers.
The24paired owner-only replay schedules protect unchanged underlying transport
paths; they do not execute the new runtime ACK helper. Its boundary schedules
are tested separately, and no synthetic tick count predicts handset time.

No Pixel command is run by this workflow. The native image remains a manual
cold temporary boot. Working WLAN/WLFW/Wi-Fi is not claimed before its result.

Final delivery verification:573owner tests pass with one prior ignored,
baseline570pass with one ignored, link220pass. The four ACK lifecycle schedules,
two clock-independent readiness tests/17negative mutations, two actual renderer
fixtures,24paired owner-only replay schedules,10fault controls and native
table/domain corruption gates pass. The selected compiled stack peak remains
4,292,464bytes in5MiB, leaving950,416bytes; this is not whole-program WCS.

The exact-image capability audit rejects304mutations. The unchanged pinned SDK
reproduces the entire loader/CapDL/report byte-for-byte. Its actual initializer
archive contains24,536objects and the eight expected Normal-NC RW/NX/uncached
mapping caps; wrong/missing archive-count controls reject. Object count and all
named region-frame physical bindings match the preceding image. Source,
reference, private-input, compiled-helper, wrapper, LZ4 and boot-layout bindings
pass. The earlier unrelated78-vs128SMR model expectation remains documented in
the inherited unrelated-model baseline; this change does not claim it passes.

Profile `native-original-final-ack-turn-v1/fastboot.img`:68,902,912bytes,
SHA-256 `2fe071b2282c6c9ef3f04fd095c6a9be6ae32e4236605d3d074d613c4714bba6`.
The private manifest, frozen source locations, actual disassembly, stack/audit
reports and manual capture instructions are retained beside the image. The
phone outcome remains untested at delivery. No device command was executed.
