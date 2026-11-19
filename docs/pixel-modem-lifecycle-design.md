# Pixel modem lifecycle service design

## Scope and current status

This document defines the authority boundary needed before NexatrodeOS can
bring up the Pixel 4a modem for WLAN. It is a design and host-side policy
model, not a working modem driver. No modem PAS operation, full platform
power sequence, firmware load, memory ownership change, or GLINK write is
implemented by the lifecycle service.
LineageOS remains the only environment currently known to perform the complete
modem lifecycle on this device. Native QMP descriptor validation and a fixed,
zero-padded modem load-state packet encoder now exist in `rust/src/qcom_qmp.rs`.
A read-only AOP mailbox probe was observed on Pixel: AOP link UP, AP link and
both channels DOWN, empty RX/TX, geometry `0xc0/0x68` and `0x128/0x2d8`.
`platforms/sunfish/aop-handoff.json` records the photo. A separate native
link/open/close handshake uses that geometry and only AP-owned descriptor
fields plus the doorbell. A device photo confirms all three phases pass with
three notifications and both mailboxes empty; AP close is acknowledged while
the remote channel remains UP. The user confirmed LineageOS returned.
The separate packet probe now implements one fixed `load_state: off`
notification and waits for TX consumption before closing. A device photo
confirms `PACKET PASS`, OFF publication/consumption, four notifications, empty
final mailboxes, and acknowledged AP close. The user confirmed LineageOS
returned. It never requests `on` or performs the platform power sequence.
TX consumption is transport evidence, not a semantic command acknowledgement
or proof of modem state. See the 2026-10-17 observation section in the Wi-Fi
feasibility notes.

The native resource probe reads the command-DB resources and ARC mappings for
CX, MSS, MX, and XO, plus Apps RSC active-bank status. Its hardware mappings
are read-only. The first device photo confirms the command-DB address/size
but reports parser `TableLayout` error 4, before selectors or RSC reads. A
source-aligned revision then showed a stable version-1 database, four
populated tables, and `SlaveId` error 6 at slot 1 entry 0 (address zero).
The next revision follows Linux's per-consumer address/type validation,
preserving structural bounds and refusing zero/wrong-type selected resources;
the following photo confirms `READ ONLY DONE`: CX/MSS/MX addresses
`0x30000/0x30060/0x30010`, TURBO selectors 6/7/4, and XO `0x30090`.
RSC reports eight DRV2 TCSs, 16 commands each, IRQ status zero, both active
banks idle. This proves resource decoding, not controller ownership or votes.
The fixed active-TCS0 CX READ now passes on device: response 5, checked
completion and cleanup, IRQ enable restored to 1, both banks idle with
enable/wait/control zero. It writes controller registers but never requests
a resource WRITE/vote. The repeated-read image now also passes on device:
eight requests, both rounds CX=5/MSS=0/MX=3/XO=3, and checked cleanup/reuse.
The next restricted candidate accepts only that starting tuple and requests
one fixed CX TURBO selector 6, checks its acknowledgement and two readbacks,
then holds the request until physical restart. It does not restore a guessed
prior vote, touch the other resources, or implement the modem power sequence;
the device photo confirms CX TURBO HOLD PASS with response 5->6, two matching
readbacks, other resources unchanged at 0/3/3, and checked cleanup. This is
the first observed power WRITE experiment. The next candidate requests
CX6/MSS7/XO3 in order, checking each acknowledgement/cleanup and intermediate
readbacks. It remains an active-resource experiment without bus/AOP-on,
sleep/wake programming, PAS/reset, firmware, or DMA. The device photo confirms
RESOURCE HOLD PASS: all three acknowledgements, CX6/MSS7/MX3/XO3 on both final
readbacks, and clean controller completion. The user restarted to LineageOS,
confirmed by ADB. This does not satisfy the full platform-power precondition.
The next read-only image inspects MC0/ACV plus CE0/SN11/SN0/SH0 command-DB
metadata after checking the modem and PAS reference routes in the final DT.
The first metadata photo decodes five resources but reports our generic
width-zero rejection for ACV. The revised raw reader now shows METADATA PASS
for all six, including ACV address0x5000c/unit1/width0/VCD11. A separate
host-tested pure planner implements
bus arithmetic and VCD word grouping, including ACV's post-scaling mask
handling. It supplies no MMIO backend, discovered handoff votes, QoS, or
sleep/wake controller policy; the restricted hardware experiment below is
separate from this planner.
A read-only three-page RSC snapshot now shows READ ONLY STABLE: all eight
active/sleep/wake banks are empty and idle, config0x80008104, IRQ status0.
It cannot establish prior per-resource latched votes or complete ownership.
The next candidate performed 12 fixed BCM resource READs with the existing
active0 transaction engine and checks full bank-programming preservation
before/after each request. Only the first controller page is writable;
sleep/wake pages are read-only. The photo confirms BCM READ PASS, both
six-resource passes matching, and 24 preservation checks. MC0 reads
0x29917f75 (X9797/Y16245), already above the isolated proxy peak field15819;
ACV reads0x20000008. The next candidate requires that exact starting tuple
and reasserts only MC0's same numeric fields with commit (0x69917f75), checking
acknowledgement, cleanup, all-bank preservation, and repeated resource reads.
ACV and the other four resources are never written. The photo now confirms
MC0 HOLD PASS: acknowledgement1, two unchanged MC0 readbacks, other five
unchanged, 50 preservation checks, and clean controller completion. ADB confirms
the subsequent return to LineageOS. No automatic unvote, bandwidth increase,
modem start, or complete bandwidth provider is supplied by this experiment.
The combined candidate now sequences MC0 first, then CX6/MSS7/XO3, preserving
all eight banks around every request and checking repeated rail/bus readbacks.
The photo confirms COMBINED HOLD PASS, all four acknowledgements, 72 requests,
144 bank checks, final CX6/MSS7/MX3/XO3 and unchanged bus replies. ADB confirms
LineageOS returned. It supplies no AOP-on, PAS/reset, firmware, or DMA.
The next candidate tests three additional nondecreasing PAS-route bus floors
(SN11/SN0 Y7426 and CE0 saturated fields16383), with repeated intermediate
readbacks and full-bank preservation. This is not complete client aggregation
or sufficient PAS bandwidth: CE0's source-derived fields exceed the encoder's
14-bit range. The photo now confirms BUS FLOOR HOLD PASS, ACK mask0x7f,
119 requests and 238 bank checks, CE0 0x2fffffff, SN11/SN0 0x213c5d02, and
unchanged MC0/ACV/SH0 plus CX6/MSS7/MX3/XO3. ADB confirms Lineage returned.
The candidate invokes no secure operation or modem start. A separate read-only
INFO probe now checks the SMC64 convention and availability of the four PAS
calls; the photo confirms QUERY DONE with all four advertised, status0/value1,
and no PAS operation invoked. ADB confirms LineageOS returned. Target
advertisement cannot establish PAS ID4 support or loader readiness.
The standalone metadata probe stages only the installed9100-byte MDT header
in dedicated contiguous RAM: byte-volatile Device-nGnRnE/NX access matching the
Linux strongly-ordered mapping, exact copy/readback, zero padding, two guard
pages, and reparse of all29 relocatable nonoverlapping segments. It supplies
no SMC capability, firmware segment loader, modem-pool mapping, or reset.
Metadata staging does not establish secure-world ownership/cache behavior.
The pure legacy bus model now includes divided numeric accumulator carry across
EBI's ordered MC0 -> ACV bindings; zero ACV width alone cannot justify clearing
its Apps mask0x8. Existing and new native probes preserve ACV without writing it.
A rooted read-only Linux inventory shows CX normal/AO enabled while both MX
providers have zero users and no CX parent-consumer links; together with the
source/DT naming mismatch this supports no framework parent in that snapshot,
but does not resolve hardware/AOP rail coupling. Do not assume an effective
parent vote. This resource observation does not
implement the proxy power transaction or satisfy its startup precondition.
The user photo now confirms METADATA STAGE PASS, with exact copy/reparse,
both guards intact and zero padding; ADB confirmed LineageOS boot complete.
A separate integrated preflight candidate combines that CPU staging with a
single fixed AOP ON notification, the strict seven-resource hold experiment,
and read-only final buffer verification. It stops at the first fault and has
no SMC capability or executable firmware/reset/DMA path. ON consumption is
transport evidence only. The user photo confirms PREPARE PASS: all five AOP
phase bits, four notifications, seven write acknowledgements,119 requests,
238 bank-program checks and intact metadata/guards after the holds. ADB
confirmed LineageOS returned. This does not prove full client aggregation,
QoS or the secure loader's acceptance.

The design follows the observed Lineage flow: establish power and clocks, load
and authenticate the modem image through the platform secure path, publish
modem power-up, and only then create the AP-side GLINK/SMEM transport. On stop
or failure, the owner must revoke transport and DMA access before making any
shared region available to another owner. The full modem image reservation
itself remains excluded from ordinary allocation for the system lifetime. The
source audit and concrete blockers are in
[`pixel-wifi-feasibility.md`](pixel-wifi-feasibility.md).

The complete-buffer diagnostic now confirms all9100 metadata bytes zero,
8171 differences from the original, intact guard/padding values and two exact
matching allocation reads. This bounds the observation; the writer and secure
implementation's zeroing policy remain unknown. Linux declares SCM_RW and
frees the metadata before MEM_SETUP, so the next separate experiment models
this as consumed input rather than requiring a reusable ELF after INIT.
A successful INIT session must be consumed into a distinct memory-setup
session, gated by the exact observed byte pattern and a fresh INFO check.
It adds only one fixed PAS4 MEM_SETUP for the excluded modem reservation,
not segment loading, authentication/reset, shutdown or DMA. The artifact is
initially built and audited locally only at the user's request. Their later
“Lineage ready” reply resumed the temporary-boot test workflow; the verified
image was accepted by fastboot without flashing. The subsequent photo confirms
MEM SETUP PASS with zero monitor/service/kernel errors and the exact consumed
buffer checks intact; ADB confirms return to LineageOS. This is request acceptance,
not an authenticated or running modem. A separate bounded next experiment maps
only the4K page at0x93200000 and, after repeating those gates, copies installed
segment23 (45 bytes, non-executable). It snapshots the page and checks payload,
unchanged gap and read stability. The full reservation remains unavailable for
ordinary allocation; the isolated mapping is explicitly audited. No executable
segment, auth/reset or DMA follows. The subsequent photo confirms DATA COPY PASS:
45 writes,matching source/readback CRC,zero payload/gap/drift failures; ADB
confirms LineageOS returned. The next isolated experiment advances to segment14,
4160 file bytes crossing two pages and4032 BSS zero bytes. It retains two
read-only guard pages and checks every byte plus a second full read. The exact
manifest and immutable installed inputs are pinned; still no executable
segment,auth/reset or DMA. The photo now confirms SEGMENT LOAD PASS:4160 file
bytes,4032 BSS zeros,zero guard/payload/zero-tail/drift failures,and matching
CRCs; ADB confirms return to LineageOS. The next isolated image stages all29
loadable segments after the same gates,with two exact source/BSS verification
passes and a separate exact snapshot of every gap byte. It includes executable
segment contents as data in RW/NX memory and stops before authentication or
reset release. All sources match the recorded installed inventory. A private
loader placement and minimal temporary ramdisk accommodate the larger payload;
the base SDK and original boot partition are unchanged. Native whole-image
staging remains unverified: the phone returned to LineageOS without native
result evidence, and the user is unsure whether the restart was automatic.
The reported `watchdog` boot reason does not establish the cause or the stage
reached. A separate temporary boot-only diagnostic now carries the full source
payload and visible loader checkpoints while avoiding PAS calls, resource
votes and modem-memory access. It also returned to LineageOS without a native
checkpoint report. A subsequent compact diagnostic stops before relocation
or kernel startup; the user confirmed its `IMAGE ENTRY HOLD` marker on the
Pixel. A comparison with identical entry code and the large payload appended
as inert data also reached `IMAGE ENTRY HOLD` on the Pixel. This isolates
standalone entry from the actual loader's relocation and modem staging. The
next prepared check stops the actual private loader at `IMAGE RELOCATED`,
before loader `main` or seL4; its full copy and stop point pass an emulator
check. Fastboot accepted the temporary image, but the user reported Google
and ADB later confirmed LineageOS without a native marker. Relocation remains
unverified. A new prepared variant masks interrupts and stops at the first
actual-loader marker, before relocation; its stop point and untouched
relocation destination pass the emulator check. The user then confirmed this
variant's `IMAGE ENTRY` marker on the Pixel. A subsequent copy-only candidate
keeps the interrupt mask, verifies the copied immutable prefix and payload
twice, and stops at the source before cache maintenance or destination
execution. It passes host and emulator checks. The user observed
`IMAGE COPY START` and confirmed the phone returned to LineageOS by itself;
the final copy/readback result and reset cause remain unverified. A separate
candidate replaces the byte copy with aligned volatile 64-bit ordinary-RAM
transfers and byte prologue/tails. It marks the first read/write and completed
4 MiB chunks, then both immutable verification passes, before holding at the
source. Host alignment/bounds/fault tests, sanitizers and execution of the full
image under QEMU pass. The user reported `IMAGE COPY FAIL FFFFFFFE`, the
expected-address/bounds rejection code. A subsequent read-only snapshot
displays SRC/DST/PRE/PAY/LEN and individual guard bits, then holds without
copying. It passes host/sanitizer checks and QEMU execution at two distinct
image placements; both screens decode to the actual arguments and the entire
destination remains untouched. The native photo then reports source
`0xb0081000`, destination `0xb0000000`, and ERR `0x201`: the unexpected-source
and full-image-overlap bits. Replaying that frozen diagnostic at the observed
placement reproduces every reported value. The corrected loader links at
`0xc0000000`, pins the copy guard to the observed source, and checks full
source/destination exclusion at build time. Host/sanitizer checks and QEMU's
complete copy/readback at that source pass. The user then reported
`IMAGE COPY PASS`, confirming the native full copy and both immutable readback
passes at the source hold. The next prepared variant returns from the copy
routine only after success, runs the original SDK cache-maintenance routines,
branches to the copied loader and holds at `IMAGE RELOCATED` before loader main
or seL4. QEMU observes both cache entries and destination entry, checks the
relocated stack, bytes, canaries and exact screen. An injected oversized total
stops at failure before copy/caches/destination execution. The user then
reported `IMAGE RELOCATED`, confirming native destination execution.
The next prepared check reaches loader main, performs CPU/exception/MMU-off
preparation, and validates every field of a pinned 11-region AP image plan
before any task-image write. It loads and verifies each region twice, then
holds at `IMAGE LOAD PASS` before kernel or secondary CPU startup. Its private
diagnostic suppresses serial output and the optional PSCI version query; no
secure call or modem operation occurs. Host/sanitizer checks and QEMU compare
all 11 destination images exactly. An invalid last destination is rejected
before any region is written. The user reported `IMAGE LOAD PASS`, confirming
native loading/readback of all eleven AP regions before kernel execution. The exact causes
of earlier resets have not been established.
The next isolated variant continues past successful task readback into the
unchanged seL4 kernel and the same read-only payload diagnostic. Its single-CPU
build is pinned to that diagnostic program. QEMU executes MMU enable and stops
at the real kernel entry with exact arguments; it does not execute the kernel's
first instruction or model Pixel hardware startup. A bad final destination
still stops before any task write. The photo confirms native `BOOT CHECK PASS`,
`READ BYTES 04139BEA` and `PAYLOAD SUM 2766D207`: the kernel and diagnostic
ran and read the complete owned payload. No modem mapping access, PAS call,
resource vote or reset release occurred in this diagnostic.
The whole-image staging program is now rebuilt unchanged on the corrected
loader, with a separate exact AP-region manifest and mutually exclusive build
mode. It repeats the previous PAS4 INIT/MEM_SETUP and resource gates, copies
all 29 files and BSS into RW/NX reserved memory, checks the loaded bytes twice
and every gap against a separate snapshot, then stops before authentication or
reset. Host checks and QEMU kernel handoff pass. The user report and photo confirm
native `IMAGE STAGE PASS`, zero errors, all masks7FFFFFFC, exact file/BSS and
two verification byte counts, preserved gaps and metadata-post check0. The loader's plan contains only AP targets and performs no
modem-pool access itself.
A separate one-shot authentication/reset-return diagnostic now consumes the
actual completed image-staging session, checks fresh metadata, queries AUTH
and SHUTDOWN availability, checks metadata again, and allows only one fixed
PAS4 AUTH_AND_RESET. There is no shutdown executor or retry/resumption path.
On error/ambiguity it retains all allocations and held requests until full
platform restart. This is an explicit standalone empirical experiment, not an
implementation or relaxation of the production lifecycle manager's preflight.
It can release modem reset but reports only secure return values; no ready
capability, shared-ring initialization, WLAN negotiation or host DMA follows.
Build gates pin the matching separate AP plan, single CPU, proxy-only DT
resource lists and the observed modem event routing. Host tests pass, including
full-image staging into the authentication core with synthetic secure replies;
QEMU stops at real kernel entry. The user reported native `AUTH RETURN PASS`,
confirming the fixed secure call returned with accepted monitor/service status.
Individual numeric rows were not supplied. Modem firmware progress, the ready
signal and full lifecycle READY requirements remain unobserved or unsatisfied.

The follow-on isolated status observer consumes the successful auth session
and a non-copyable absent-before-auth SMEM baseline. It requires that baseline
to remain unchanged before staging and authentication. Afterward it permits
only read-only SMEM directory/payload inspection, serialized by TCSR lock3,
with double snapshots of modem-owned SMP2P item435. Ready bit1 must be present
without fatal/stop/shutdown bits. A bounded physical-counter poll records
`READY BIT SEEN` or a failure/timeout; it mints no lifecycle/transport token.
No AP SMP2P/GLINK write, IRQ setup, host DMA or WLAN operation follows. Full
READY/recovery and DMA ownership gates remain unsatisfied. The user reported
`READY NOT SEEN`, followed by ERROR10 (hex timeout), ITEM OFFSET0011D030,
VALID ENTRIES0 and SLAVE KERNELFFFFFFFF. The new valid modem-owned header
establishes shared-status progress, but no ready entry was published.

The typed passive-status rows establish timeout10, newly published inbound
item at0011D030, valid-entries0 and no slave-kernel value. The separate next
experiment may allocate one fresh AP-owned item428 only after accepted AUTH
and a new valid peer header, under lock3, wholly inside the one explicitly RW
partition page. An empty exact pre-auth partition baseline, allocation absence,
full invalid-version readback, cursor commit/readback and final version1
publication are required. It advertises only `master-kernel=0` with optional
SSR features disabled, then sends exactly one status notification. It verifies
AP identity/payload on every poll and requires peer feature negotiation before
recording a ready bit. There is no SSR acknowledgement/recovery, production
READY token, GLINK, WLAN or host DMA transition. Native `HANDSHAKE STOPPED`, ERROR11 (hex Bounds), AP PUBLISHED0 and
PEER NEGOTIATED0 confirm the post-AUTH allocation guard stopped before AP
block writes. The exact cursor/failed bounds component was not captured.
The next isolated revision publishes AP428 in the verified empty partition
before AUTH. It consumes a one-shot opaque permit from complete staging,
successful INFO queries and fresh consumed metadata, revalidates under lock3,
and retains the same one-page write boundary. Metadata is read again before
AUTH. The observer consumes the successful permitted authentication session,
checks the reserved AP identity/payload, waits for a new peer header and
notifies once. No allocation or AP payload write occurs after AUTH; later
modem allocations may grow beyond the first page without changing AP ownership.
The user reported native `READY BIT SEEN`, confirming the experimental
status-handshake path and ready bit. The raw numeric tail1,1,2,6 is retained
without treating its row assignment as independently confirmed. No production
lifecycle/recovery token is minted; GLINK and WLAN remain uninitialized.
The following read-only directory diagnostic consumes an opaque one-use
observation issued only at that successful ready branch. Under lock3 it
revalidates AP/peer identity and ready state around two identical allocation
snapshots for478/479/480. It reads allocation headers only, with no descriptor
index or FIFO-payload access, and keeps the existing mappings unchanged. The
observation permits no GLINK write, allocation, reset, notification or IRQ
setup and is not a production lifecycle token. A missing item is an absent
point-in-time allocation, not evidence that it can never appear. Fastboot
accepted the frozen directory image on2026-10-18 at06:06:14UTC (exit0;
no partition flashed). Native directory status and allocation readout are
pending; command acceptance does not establish the native result.
The subsequent user readout was `DIRECTORY SNAPSHOT PASS` with mask5,
descriptor0x11d330/0x20, absent TX and RX0x11d360/0x4000. ERROR was omitted
and is inferred zero from the marker. This observes allocation metadata only.
The next explicit receive-header feature rechecks this directory/ready/AP
identity before and after two matching descriptor and bounded pending-header
reads. Hardware lock3 serializes allocation metadata, not remote FIFO writes;
matching reads establish a point-in-time observation, never durable ownership
or an epoch capability. No FIFO index/reset, allocation, ACK/notification, IRQ,
WLAN or DMA operation is added. Native receive-header result is pending.
The frozen receive image was accepted by temporary Fastboot boot on2026-10-18
at06:18:15UTC (exit0, no partition flashed) after46 host tests and both
actual-image QEMU loader checks. The native status/header readout is pending.
The user subsequently reported `RX EMPTY SNAPSHOT PASS`, ERROR0, RX AVAILABLE0.
The individual indices were omitted; only their validated equal/in-bounds
relationship follows from the marker. No pending version offer was observed.
The next separate experiment consumes the pre-AUTH staged/INFO/fresh-metadata
session into a fixed fresh-memory permit and stops before authentication.
It requires intact AP-only metadata, initializes only fresh descriptor478/TX479,
verifies before committing the cursor once, then rechecks AP/directory and
untouched bytes. Five explicitly admitted RW SMEM pages replace the earlier
single-page writer; all other507 pages remain RO. It cannot reset existing
indices, authenticate/start firmware, send a GLINK packet/notification or
transition the production lifecycle into PEER_READY. A memory test pass would
establish allocation/readback only; the production ownership and epoch contract
remains required before a running transport can write shared indices.
The frozen standalone memory image was accepted by temporary Fastboot boot
on2026-10-18 at06:35:44UTC (exit0, no partition flashed) after39 preparation
tests,32 authentication tests,22 Python checks and both actual-image QEMU
loader checks. The native memory-preparation result remains pending.
The user then reported `GLINK MEMORY PREPARE PASS`, FREE AFTER41C8 and
GUARD BYTESFBC. Zero error follows from the marker; the ERROR row was omitted.
This confirms native fresh-memory preparation only, without authentication.
The next explicit boot feature keeps AuthOnce private through actual preparation,
then revalidates all prepared bytes/metadata and fresh MDT before its one AUTH
call. Failed preparation/revalidation or ambiguous authentication cannot return
a post-start identity. Successful authentication returns a non-copyable identity
for read-only inspection after separately observed SMP2P ready. Retained
descriptor/TX identities, untouched AP indices, zero TX data and ready state
are checked again; no post-AUTH GLINK write occurs. The five-page manifest is
unchanged. This diagnostic identity does not satisfy or replace the production
PEER_READY ownership/epoch and recovery contract. Native result is pending.
The frozen boot-memory image was accepted by temporary Fastboot boot
on2026-10-18 at06:52:37UTC (exit0, no partition flashed) after46 boot tests,
34 authentication tests,22 Python checks and both actual-image QEMU loader
checks. The native boot-memory readout is pending.
The user subsequently reported native `BOOT MEMORY PASS`, ERROR0, PRESENT
MASK7 and RX AVAILABLE8. Retained fresh-memory and status-ready checks passed,
and a pending eight-byte header is observed. Its command/parameter rows remain
unreported, so this is not yet an observed version offer or negotiation. No
GLINK packet, RX advance or post-AUTH GLINK write occurred. The current screen
already displays the needed header rows; no restart is needed to capture them.
The user subsequently restarted before capturing those rows. LineageOS return
was confirmed by ADB. The original pass/readiness evidence remains valid, but
the header values are unknown; the same frozen image is being replayed for
that missing readout, without changing code or enabling transport writes.

The unchanged replay subsequently produced COMMAND0/PARAM ONE1/PARAM TWO7:
an observed GLINK VERSION v1/features7 offer. Only those three fields were
reported on replay; the original BOOT MEMORY PASS/ERROR0/MASK7/RX8 are prior
evidence. There is still no native version acknowledgement or channel exchange.

At the user's request, the next standalone experiment is prepared locally only.
It uses a distinct non-copyable `FreshBoot` value returned by its own actual
successful AUTH session; the read-only `BootIdentity` has no conversion into it.
A separately observed ready state, the exact offer0/1/7, retained allocations
and zero TX/AP indices gate two fixed VERSION messages with optional features0.
The only post-AUTH SMEM stores are16 fixed TX bytes, AP TX head16 and AP RX tail8;
one bit12 notification follows outside the lock. Up to five seconds/512 read-only
samples must observe full TX consumption and ACK1/1/0 with live status and exact
TX/identity checks. No reply consumption, channel, IRQ, generic send, retry or
cleanup path is provided. The same five-page experiment mapping applies.
This narrowly scoped experiment does not implement production ownership/epoch,
reset recovery or PEER_READY transition authority. It has not been sent to the
Pixel, and preparation does not authorize a reboot or temporary boot.

The user later explicitly authorized running the prepared version image.
After fresh frozen-image/source checks and Lineage/Fastboot identity checks,
temporary boot of the unchanged archive was accepted on2026-10-18 at
09:33:26PDT (exit0, no partition flashed). The native status is pending;
neither command acceptance nor the earlier offer establishes negotiation or
production lifecycle authority.

The native version experiment subsequently stopped with Payload8, TX CONSUMED1
and ACK SEEN0. The reported front header is OPEN2/channel1/name length0xa,
with0x50 receive bytes available. This confirms fixed TX consumption and a
peer channel request, not channel acceptance or a completed production
negotiation. Neither the OPEN name nor subsequent queued messages were read;
ACK absence cannot be inferred from the ACK-only test stopping at OPEN.

The next standalone pending-RX variant keeps the same fixed version submission
and terminal one-shot adapter. It adds only a bounded128-byte read-only prefix
snapshot after validated TX consumption, comparing two byte copies and exact
descriptor/identity/ready observations. A bounded control parser reports OPEN
names and an exact ACK within that prefix without advancing RX, accepting a
channel or sending another response. Snapshot success is independent of the
original ACK-at-front exchange result. Unknown/truncated records and bytes
beyond the captured prefix remain unclassified. This remains experimental
readout, with no production channel ownership, lifecycle transition or recovery
authority. No subsequent native boot is claimed by local preparation.

The user then supplied the requested Lineage readiness. After fresh frozen
fingerprint and device-identity checks, the unchanged pending-RX archive was
accepted for temporary boot on2026-10-18 at09:55:49PDT (exit0, no partition
flashed). The native snapshot and channel-name readout are pending. Command
acceptance does not establish queue contents, acknowledgement or ownership.

The subsequent photograph confirms RX SNAPSHOT PASS with complete80-byte
capture, four parsed messages, no parser/snapshot error and exact expected ACK
behind three OPENs. IPCRTR is offered as remote ID2 with seven name bytes.
The other normalized name displays are GLINK SSR and SSM RTR MODEM APPS.
Wire case/punctuation must be rechecked in live bytes, not inferred from the
uppercase fixed font. No channel or receive record has been accepted.

The fixed local-open experiment retains a distinct non-copyable continuation
inside the actual version execution, rather than upgrading a copied diagnostic
report. Exact live queue/name/ID and ready checks gate one sixteen-byte IPCRTR
OPEN/local1 append, head32 and one extra notification; RX tail8 and the original
queue remain intact. A bounded read-only reply scan checks ACK4/local1/0 and
full TX consumption without responding to the peer OPEN, advertising receive
intents or sending QRTR data. Any ambiguous failure is terminal until restart.
This one-shot request leg does not satisfy production epoch, reset recovery,
completed channel, MSA or SMMU ownership requirements.

After requested Lineage readiness and fresh frozen-file/device checks,
temporary boot of the unchanged fixed IPCRTR request image was accepted on
2026-10-18 at10:30:03PDT (exit0, no partition flashed). Native request/ACK
was subsequently captured as the stopped result below; acceptance of the command
alone was not channel or lifecycle proof.

Native request execution stopped Payload8 with STAGED0, HEAD STORE0 and
NOTIFICATIONS0. No request was published/notified; unpublished stores and the
exact failed guard were not independently identified. Revision2 requires exact
raw IPCRTR/ID2 but only bounded structural checks for unused channel names,
whose case/punctuation were not established by the font. It preserves the
full queued bytes and all existing identity/ready/index/packet bounds, and
adds guard-stage, plan-detail and issued-store diagnostics. The private actual
version continuation and terminal adapter remain required; this change grants
no authority to accept another channel or advertise buffers.

After fresh Lineage and frozen-file checks, temporary boot of revision2 was
accepted on 2026-10-18 at 11:04:37 PDT (exit0; no partition flashed). Native
status was subsequently captured in a photograph: IPCRTR REQUEST PASS,
ERROR0, STAGED1, HEAD STORE1, NOTIFICATIONS1, TX CONSUMED1, OPEN ACK SEEN1.
TX tail/head32, RX tail8/head96; the eight new bytes are OPEN_ACK4/local1/0.
The local request leg is confirmed. The AP has not acknowledged the peer's
IPCRTR OPEN or consumed this queue, so a usable channel is not yet established.

The next fixed reciprocal-ACK experiment consumes an OpenCheckpoint owned by
the actual successful request execution. Reports cannot construct it. It
rechecks the native32/32/8/96 state and all88 unread bytes before appending
OPEN_ACK4/remote2/0 at TX32, publishing head40 and notifying once. Read-only
control snapshots count peer intents without using their metadata; RX tail8
stays unchanged. No receive buffer, intent advertisement, QRTR payload or DMA
authority is created. Errors remain terminal with no retry/cleanup. This is an
experimental handshake step, not a production channel/epoch capability.
The isolated image is built, locally tested and frozen. After fresh Lineage
and frozen-file checks, temporary boot was accepted on 2026-10-18 at
12:14:14 PDT (exit0, no partition flashed). Native ACK/intent readout remains
pending; no channel transition is inferred from command acceptance. Its QEMU
checks stop before the first kernel instruction.

The native photo subsequently confirms IPCRTR ACK PASS/ERROR0 and TX tail/head40;
the reciprocal acknowledgement was consumed. RX tail8/head584, available576,
and the128-byte prefix contains8 one-entry INTENT5 records for remote2. Another
360 newer bytes were not captured and no total intent count is established.
This proves both fixed acknowledgement legs, not a production channel/epoch
capability or readiness to transfer QRTR data. No advertised intent was used.

The next diagnostic repeats the same actual owned continuation chain and
fixed writes, increasing only the final control-copy bound128 to512. Intent
size/ID words are bounded observations for size groups and duplicate/zero
checks; copied reports still grant no send, buffer or epoch authority. No
allocation from remote size, intent selection, RX advance, extra notification
or QRTR payload is enabled. A complete-inventory marker requires all currently
queued new bytes to fit and parse after stable copies and ready/identity checks.
The separate image is built, tested and frozen. Live ADB already showed the
expected boot-complete LineageOS device, so no manual restart was needed. The
standing iteration instruction authorized the next temporary test. Fastboot
accepted the verified image on 2026-10-18 at 12:29:35 PDT (exit0, no partition
flashed). Native expanded-inventory results remain unobserved; command
acceptance does not grant any additional transport authority.

Native expanded capture stopped Payload8 after stable copies of all488 new
bytes (head584, available576). ACK consumption remains confirmed, but the
first5/remote2/count1 header does not identify the rejected record. Complete
inventory remains unproven. No additional transport authority follows from
this captured-but-unclassified queue.

The separate revision2 image keeps the actual owned continuation chain,
native writer,512-byte copy bound and all ready/identity/TX/prefix gates.
One scanner retains only complete-record stats before a stop and reports its
exact relative offset/header/preview. The per-record diagnostic count bound
is63, derived from512 bytes rather than the old128-byte/15-pair limit. It
observes request7 as an eight-byte control record without answering it or
allocating from its size. These changes do not identify the original native
failure cause or grant buffer/send/epoch authority. The tested and frozen
revision2 image was accepted for temporary boot after fresh Lineage/device
and frozen-file checks on 2026-10-18 at 12:48:45 PDT (exit0; no partition
flashed). Native scan/readout remains pending. Command acceptance does not
identify the failed record or establish a complete inventory.

The subsequent native photo confirms complete488-byte parsing:30 unique peer
intent IDs0..29, four size groups128x16/512x8/1152x5/8320x1, and one52-byte
receive-intent request. The31 control records comprise30 intent records and
one request, with no duplicates/zero sizes. No receive intent or response
was sent. A copied inventory remains observation only; a later receive step
must own the actual running exchange and revalidate all live bytes and ready
state before making a new buffer promise.

The prepared first-receive experiment consumes a non-copyable checkpoint
minted by that actual complete exchange. It requires the current unique
30-intent inventory and52-byte request, unchanged ready/identity/TX state
and576 unread bytes. One private64-byte buffer with32 guards is leased through
restart. Its fixed intent1 and positive request ACK append24 control bytes at
TX40 and publish head64 once, followed by one notification outside the lock.
The address never enters the wire packet. RX tail remains8. Only a double
copied DATA9/remote2/local-intent1 frame, chunk52/left0, can enter the buffer;
two volatile readbacks, zero remainder, guards and final live readiness checks
gate success. Later queued bytes remain unclassified and unconsumed. No
RX_DONE, reuse, outgoing QRTR, peer-intent selection or DMA authority follows.
The lease is parked even on ambiguity. Host111 tests and both loader checks
pass; native packet receipt and interpretation remain pending.

The frozen first-receive image was accepted for temporary boot on
2026-10-18 at13:18:55 PDT (exit0, no partition flashed), after live Lineage
and frozen-file checks. The native result remains pending; acceptance does
not establish that the buffer promise was published or any packet copied.

The native readout subsequently confirms the52-byte guarded copy and
consumption of all64 AP TX bytes. QRTR version1/type2/body command2, control
ports, source node0, broadcast destination and20-byte body are visible.
Remaining body words are not displayed. RX tail8 is still retained and no
RX_DONE was sent, so the local intent must remain leased until a verified
completion or restart. This does not establish outgoing QRTR or WLAN.

The prepared completion experiment owns the successful actual receive token
and its private buffer lease. It double-checks648 queued bytes and the52-byte
private copy, stages only fixed non-reusable RX_DONE6/local1/intent1 at TX64,
verifies the full TX ring, then publishes RXtail656 and TXhead72 once with
intermediate live checks. The fifth notification occurs outside the lock.
After cursor release the peer owns the consumed RX bytes, so verification
uses the private payload and never reads that released prefix again. Later
RX bytes remain unread. Earlier non-IPCRTR offers are discarded without
acceptance. All partial outcomes are terminal; the private buffer stays
reserved through restart, with no new/reused intent or outgoing QRTR data.
Host122 tests,20 source/layout/mode tests,18 common gates and both loader
checks pass. The frozen image has not been sent and native completion is
pending. Even observed TX consumption cannot prove the peer's internal
retirement semantics or grant production lifecycle authority.

After fresh live Lineage and frozen-file verification, Fastboot accepted
that temporary completion image on 2026-10-18 at14:50:03 PDT (exit0;
no partition flashed). Native completion and cursor status remain pending.

The native result now confirms `RXDONE CONSUMED PASS`/ERROR0: both TX
indices72, both RX indices656, no unread bytes, guards intact and AP intent
retired. The fixed completion was consumed after exactly eight byte stores,
one store per cursor and one notification. No new receive promise or QRTR
transmission followed; the private buffer remains reserved through restart.

The prepared first-HELLO probe consumes that actual completed continuation.
The sealed current advertisement, not a copied inventory/photo, must prove
peer intent0 uniquely exists and has capacity for52 bytes. Reserve it once
and retain its lease even after a reuse reply or ambiguous outcome. One
constant72-byte GLINK DATA9/local1/intent0 frame carries QRTR v1 HELLO from
explicit local node1 to observed modem0, control ports and zeroed20-byte
control body apart from command2. Publish TXhead144 after full readback and
live checks; the sixth notification is outside the lock. Keep RXtail656 and
the private buffer, never read released RX bytes. A bounded double-copied
control snapshot must contain exact completion6 or14/remote2/intent0 and be
complete, with TXtail144 and final live gates. Observe other control records
without acting on requests or reusing intents. No service request, new receive
promise, WLAN/DMA or production lifecycle authority follows. Host132 tests,
20 source/mode tests,18 common gates and both loader checks pass. The frozen
candidate has not been sent; native outgoing transport evidence is pending.

After live boot-complete Lineage and frozen-file checks, Fastboot accepted
the temporary HELLO image on 2026-10-18 at15:09:39 PDT (exit0; no partition
flashed). Native send/peer-completion readout remains pending.

The native result now confirms the fixed HELLO's transport completion14 for
peer intent0 and a new52-byte receive request. Intent0's current capacity128
is visible, both TX indices144, RXtail656/head672, all16 pending bytes
captured and parsed. No intent reuse or cursor advance followed. The next
receive promise must own this actual exchange and its copied control bytes,
not a copied report or an assumed record order.

The prepared next-receive experiment retains the actual stable control
snapshot and first payload in a non-copyable HELLO continuation. Exactly
completion14/remote2/intent0 and request7/remote2/52, either order, gate a
fresh local intent2 and a separate guarded64-byte buffer. Recheck both
buffer reservations, first payload, complete16-byte pending prefix, full TX
and live state before staging24 fixed advertisement/ACK bytes at TX144,
one head168 store and seventh notification. Keep RXtail656. Only one bounded
DATA9/remote2/intent2,52-byte unfragmented packet can be copied and verified
twice; header/body words remain observations, not service/endpoint authority.
No further RX_DONE, intent reuse, QRTR TX, receive advance or WLAN/DMA action
follows. Both buffers remain reserved even after ambiguity. Host144 tests,
20 source/mode tests,18 common gates and both loader checks pass. The first
parallel handoff emulation timed out in task copy; unchanged serial rerun
passed, with the failed log retained. The frozen image has not been sent;
native next-packet evidence remains pending.

Following live physical Sunfish/Lineage and frozen-file checks, Fastboot
accepted the temporary next-receive image on 2026-10-18 at15:32:46 PDT
(exit0; no partition flashed). Native receipt and packet words remain pending.



## What the matched Lineage source establishes

The cached Lineage kernel checkout is commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`
(dated 2026-09-11); it has local edits, so use this as a source trace, not a
reproducible build or a guarantee that every line matches the running binary.
The Sunfish memory overlay assigns the modem image reservation
`0x8b000000..0x94800000` (size `0x09800000`), which matches the parsed MDT
extent from the earlier host-only audit. The common SM6150 modem node names
firmware `modem`, PAS ID 4, SMEM item 421, enables AOP load-state notification,
and declares `mss-pil` as its mailbox. The captured live merged FDT in
`.cache/pixel-sunfish/live-runtime.dts` confirms PAS ID 4, SMEM item 421,
`qcom,signal-aop`, mailbox `mss-pil`, and the exact modem reservation. It does
not add authority to use those resources from Microkit.

For ARMv8, the Linux SCM wrapper uses PIL service `0x02` with command IDs
`INIT_IMAGE=0x01`, `MEM_SETUP=0x02`, `AUTH_AND_RESET=0x05`, and
`SHUTDOWN=0x06`. The traced arguments are respectively `(pas_id,
metadata_dma_address)`, `(pas_id, base, size)`, `(pas_id)`, and `(pas_id)`.
The init metadata is copied to a coherent DMA allocation before the call.
The PAS init and mem-setup calls return secure-monitor status; auth/reset and
shutdown also return status. These are Linux wrapper-level signatures only:
they do not specify the complete SMC calling convention, secure firmware
version, descriptor ownership rules, or whether this interface can be safely
called from Microkit. The `qcom,mem-protect-id` property is absent from the
matched common modem node, so this source does not show Linux transferring the
entire modem reservation through its optional `hyp_assign_phys` VMID path.
PAS memory setup and the bootloader/secure-firmware ownership contract still
need direct confirmation.

The modem caller includes `soc/qcom/scm.h` and uses `drivers/soc/qcom/scm.c`.
The cached runtime config has CONFIG_QCOM_SCM=y and CONFIG_QCOM_SCM_64=y.
The wrapper branches at runtime on the SCM convention. The ARMv8 path builds
`scm_desc` requests: metadata is a typed read/write DMA argument, while
the PAS ID and memory range are scalar arguments; return status is split
between the transport result and `desc.ret[0]`. The legacy path instead passes
packed 32-bit request structures through `scm_call`. This confirms that
hard-coding just the visible command numbers and x0-x4 values would not
reproduce Linux's complete calling contract. The matched modem DTS also names
proxy resources `vdd_cx`, `vdd_mss`, and `xo`, plus the `mss-pil` AOP mailbox.
Linux enables these through regulator, clock, bus, and SCM bandwidth frameworks;
those framework transactions and rollback behavior are part of the platform
contract, not optional details that a direct mailbox/SMC call replaces.

The lower ARMv8 `scm_call2` wrapper clarifies the Linux-side call envelope for these
four commands. It forms the function number as `(service << 8) | command`
with PIL service `0x02`, adds the SIP-owner field0x02000000, then adds
the runtime-detected SMC32/SMC64 mask. In the matched source this
produces function numbers `0x0201` (init image), `0x0202` (memory setup),
`0x0205` (authenticate and reset), and `0x0206` (shutdown). Linux supplies
argument metadata separately from the arguments: the init call declares two
arguments, a scalar PAS ID followed by a read/write DMA buffer pointer; memory
setup declares three scalar arguments (PAS ID, base, size); the final two
calls each declare one scalar PAS ID. With the matched ARMv8 SCM wrapper, the
descriptor is marshalled as the full SMCCC call value in x0 (including the
standard-call, convention, and SIP-owner fields), argument metadata in x1,
and the first three values in x2-x4; these calls need no external-argument
array. The source's argument-info values are `0x82` for init image (2 args,
second is read/write), `0x03` for memory setup, and `0x01` for each one-argument
operation. On SMC64 the complete x0 values would be0x42000201/0x42000202/
0x42000205/0x42000206. The standalone first-init experiment now implements only0x42000201 after
fresh INFO queries and the observed preparation path. The separate memory-setup experiment adds one MEM_SETUP and now has an observed
MEM SETUP PASS; it still has no AUTH_AND_RESET or SHUTDOWN executor. The first
INIT image's strict early verifier reported payload mismatch6. The subsequent
whole-buffer diagnostic bounded that mutation to a stable all-zero payload with
intact guards and padding. Later probes require that exact consumed pattern;
the secure implementation's reason for zeroing remains undocumented.
The transport result and secure service result are checked separately.
The wrapper serializes calls, loops on interrupted responses, and retries
`BUSY` with a 30 ms delay and a finite retry count (constant67). The interrupted
loop is not itself bounded. The subsystem caller separately owns PAS bandwidth
and the required modem regulator/clock framework requests. The generic
`drivers/firmware/qcom_scm-64.c` clock wrapper is not evidence of extra clocks
in this modem's `scm_call2` path. The native availability-only probe stops on
any transport error and supplies no PAS executor or interrupted-call resumption.

This is a source trace of Linux's caller and transport behavior, not the
secure monitor's complete ABI or a safe recipe for direct invocation. It does
not establish that the running secure firmware accepts these requests from a
Microkit-originated caller, the meaning of the PAS memory setup operation for
this boot chain, the permitted metadata mapping/cache attributes, or whether
the vendor's AOP and modem resource state can be recreated outside Linux.
Those points remain blockers. Native SCM INFO capability queries are a separate
read-only experiment. A separate trusted metadata-init probe is prepared for
one empirical INIT call with owned memory; the first INIT returned monitor0/service0 on device but the strict post-call
payload comparison failed. The next diagnostic scans every guard/padding byte
and repeats exact reads without repairs or later PAS operations. It does not promote the lifecycle service
to READY or relax its full startup requirements.

## Observed Linux power-sequence trace

The pinned Lineage source and Atoll DTS make the Linux reference sequence more
concrete, but they do not provide an independent Microkit power interface:

1. `pil_boot()` first sends the AOP mailbox text notification
   `{class: image, res: load_state, name: modem, val: on}`. The mailbox client
   is configured for blocking transmit with a 1,000 ms transmit timeout and
   does not use a transmit-done acknowledgement. This reports an image
   load-state transition; it is not proof that the modem power rails were
   sequenced or that the modem is ready.
2. After reserving/loading metadata, Linux makes a temporary PIL proxy vote.
   The modem DTS names `vdd_cx`, `vdd_mss`, and `xo`; the proxy regulators are
   requested at the `TURBO` level. The modem bus client moves to case 1, whose
   final captured Sunfish DTS vector is `<129 512 0 7000000>` KBps (the older
   common-DTS peak 1041408 is overridden). The active regulators and clocks are
   enabled around PAS authentication/reset as a separate vote.
3. Linux lets the modem release the temporary proxy vote through the named
   `qcom,proxy-unvote` interrupt, with a 10,000 ms timeout fallback. On an
   error during PIL boot, that unvote becomes immediate. The interrupt is a
   Linux resource-management signal, not evidence by itself that all DMA has
   stopped or that memory ownership has changed.
4. On a forced normal stop, `modem_shutdown()` sets the `qcom,force-stop`
   SMP2P bit and waits up to 1,000 ms for `qcom,stop-ack`. A timeout only
   logs a warning; Linux clears the request bit and proceeds to PAS shutdown.
   The PAS shutdown callback takes its own bus, proxy rail, and clock votes;
   after PAS reports success it drops the active modem clocks and regulators.
   The outer PIL shutdown then releases remaining proxy votes and sends the
   AOP `val: off` notification. A PAS failure is recorded as shutdown failure,
   but the generic path still attempts proxy unvote and the AOP notification.

This trace gives us names, ordering, and timeout values to carry into a
future source review. It is not a verified hardware contract: the AOP
notification has no modem-ready acknowledgement, the stop-ack timeout does
not stop Linux from continuing shutdown, and neither response proves DMA
containment. The NexatrodeOS lifecycle service must keep its stricter
fail-closed behavior; it cannot copy Linux's timeout-and-continue path. An
independent design still needs a trusted owner for AOP/RPMh, modem reset and
stop/crash events, PAS calls, SMMU isolation, and recovery. No such service is
implemented or called.

The PIL loader reads the MDT, then requests each `.bNN` segment and checks that
its byte count matches the MDT before loading. The PAS init call receives the
MDT metadata (which its API documents as possibly containing authentication
data); PAS auth-and-reset is the later secure-world authentication step. The
host-side parser, segment-size comparison, and a local hash pin are useful for
structure, reproducibility, and compatibility checks, but none substitutes
for successful secure-world authentication or proves the secure monitor's
current key policy.

The `qcom,signal-aop` path sends an AOP mailbox text notification
`{class: image, res: load_state, name: modem, val: on|off}`. That is a
load-state notification, not evidence that it performs the full power-on
sequence. Linux separately applies proxy/active regulator and clock votes,
bus-bandwidth votes, and SCM bandwidth votes; the active modem signals include
watchdog, fatal/error, proxy-unvote, error-ready, stop-ack, and shutdown-ack.
Thus the current `aop_sequence_verified` input in the host model represents a
whole named platform-power transaction and its completion, not merely sending
the `on` string.

The selected seL4/Microkit source also makes the SMC capability boundary
explicit. Microkit's `smc="true"` option installs an unbadged SMC capability;
the seL4 invocation path accepts any `x0` function ID when the capability
badge is zero. A nonzero badge can filter the function ID, but it still cannot
validate the remaining SMC registers or the memory descriptors they point to.
Therefore a Microkit SMC-enabled domain is a trusted component with broad
secure-monitor authority, even if its IPC API exposes only “boot modem.” Do not
grant this capability to WLAN or application-facing Slint domains. In the
current Slint system, only the fixed-purpose reset watchdog receives SMC; the
touch service no longer does. Diagnostic probe systems that request SMC remain
separate from the normal UI system and must be reviewed before use. If the
project later adds a secure-platform manager, keep it tiny,
hard-code service/command IDs, construct every descriptor from fixed policy,
validate all addresses and sizes, and add a kernel/Microkit capability change
for function-ID filtering where practical. That badge is defense in depth, not
descriptor validation.
The Microkit manual also notes that the SMC itself is non-preemptible by seL4,
so its latency must be bounded or explicitly treated as outside scheduler WCET
guarantees.

## Protection-domain boundary

The modem lifecycle manager is a small trusted service. It is separate from
`sunfish_wifi`, Slint, and all application domains. It owns the lifecycle state
machine and coordinates these narrow services:

| Service | Authority | Explicitly excluded |
| --- | --- | --- |
| Firmware verifier/reader | Select one pinned, compatible modem image set; check bundle identity/hash and MDT structure/segment sizes; stream only validated segments | Arbitrary filesystem paths, image replacement, invoking SCM |
| Modem memory manager | Exclude the fixed modem reservation from general allocation for the system lifetime; prepare fixed, bounded image/MSA regions; quarantine on ambiguity | General physical-memory allocation, client-selected physical addresses, reclaiming the modem pool |
| Secure platform manager | Execute a source-reviewed, fixed set of modem PAS/SCM operations with validated descriptors | Generic SMC forwarding, caller-supplied function IDs/descriptors/VMIDs, other secure services |
| AOP/platform power manager | Named modem on/off sequence and required votes with bounded timeout and rollback | Arbitrary regulator/clock IDs or raw AOP command interface to clients |
| Modem lifecycle manager | Order the above; process modem-ready, stop, watchdog and crash events; mint/invalidate peer generations | Direct MMIO, DMA, generic SCM, or firmware-byte access |
| GLINK broker | Use its fixed transport resources only after a fresh peer generation; own AP-side ring metadata | Firmware boot, reset, SMMU policy, arbitrary SMEM item access |
| `sunfish_wifi` | Request WLAN start/stop; use validated WLFW/MSA operations and its assigned device resources | SCM, AOP, modem reset, physical addresses, VMID lists, SMMU registers, raw GLINK shared-memory indices |

The services may initially be combined into fewer protection domains only if
the combined domain has exactly the union of the stated authority and no generic
interface leaks through. Do not give WLAN an SMC capability as a shortcut.

## State machine

`platforms/sunfish/modem_lifecycle_model.py` is an offline executable policy
model of this sequence. `firmware_bundle_identified` means the selected bundle
is pinned to a known source/version and its local identity check passed; this
does not replace the modem's secure PAS authentication. The model takes
booleans as synthetic test evidence;
production code must replace them with unforgeable capabilities or authenticated
responses from the named trusted services. A caller-provided boolean is never
proof of hardware state.

The `OFF -> PREPARED` gate also requires the AP-side IPA provider, SERVREG
mapper, remote-storage provider, heap-memory-sharing provider, and TFTP provider
to report ready. The heap-memory provider is a distinct live QMI service from
the 2 MiB remote-storage transfer pool; Lineage advertises it as
`heap_memory_sharing` (service `0x34`, instance `1`), and native lifecycle
readiness must not treat its absence as covered by remote-storage readiness.
This models
the ordering in the pinned Pixel 4a Linux port: IPA and the kernel
`qcom_pd_mapper` precede MPSS, while `rmtfs` and `tqftpserv` are dependencies of
the service that starts MPSS and waits for WLFW. `ipa_host_ready` means the AP
IPA provider itself is initialized; it does not mean the later modem-facing
`INIT_MODEM_DRIVER` QMI exchange succeeded. The QMI exchange needs live IPA
platform resource values and remains a separate unresolved native integration
step. These gates are synthetic in the model: the active seL4 owner does not yet
provide these five startup services or authenticated readiness capabilities.
See the [pinned Linux-port ordering](https://github.com/miromraz/mobian-pixel4a-port/tree/789679f25f859bd741d690fe221d1cc68a23444e).

| State | Meaning | Permitted next transition |
| --- | --- | --- |
| `OFF` | No active peer; no modem/WLAN DMA may be assumed | `PREPARED`, after all preflight checks |
| `PREPARED` | Firmware, fixed memory plan, and fixed secure-operation policy validated | `BOOTING`, after named AOP sequence completes |
| `BOOTING` | Secure platform manager is executing the reviewed PAS boot transaction | `AUTHENTICATED`, only on successful secure authentication |
| `AUTHENTICATED` | Secure boot succeeded; modem has not yet been exposed to transport | `PEER_READY`, after verified AFTER_POWERUP, fresh reset, and exclusive AP transport ownership |
| `PEER_READY` | Manager minted a generation tied to this modem boot; GLINK may initialize its AP-owned ring under that generation | `WLAN_READY`, after MSA ownership and SMMU isolation are verified; or `STOPPING` |
| `WLAN_READY` | WLAN service may begin the bounded WLFW startup sequence | `STOPPING` |
| `STOPPING` | Peer token is already invalidated; transport and WLAN requests are rejected | `OFF`, only after stop acknowledgement, successful PAS shutdown, named power-down, and transport/DMA containment; the modem pool stays reserved |
| `QUARANTINED` | Reset, authentication, shutdown, or DMA containment is ambiguous; shared memory remains unavailable | `OFF`, only after reset, PAS shutdown, named power-down, and transport/DMA containment are independently verified |

Any watchdog, crash, malformed transport state, failed secure operation, or
uncertain shutdown revokes the generation immediately. Generation values are
monotonic for the lifetime of the manager; they are not reused after stop or
fault. Every WLAN/GLINK operation carries the current opaque generation, so a
late callback from an old boot cannot revive a channel. A `VERSION_ACK` is
transport protocol evidence only; it is not a substitute for the trusted
generation or SMMU/MSA ownership checks.

## Startup contract

1. Identify the exact compatible firmware bundle from a pinned source/version
   and verify its recorded local hashes. Validate every MDT segment's address,
   size, alignment, and identity before the secure boot path can consume it.
   No authenticated project manifest currently exists. The existing
   `qcom_mdt.rs` parser validates structure and fit only; neither it nor local
   hashes authenticate firmware or prove compatibility. Secure-world PAS
   authentication remains a separate required result.
2. Reserve the full modem range and all fixed shared regions outside ordinary
   Microkit allocation and every unrelated protection domain. Reject overlap,
   overflow, unexpected segment sizes, or an unknown firmware manifest.
3. Request one named modem power-on sequence from the AOP service. Require
   completion or bounded failure with rollback; do not expose raw regulator,
   clock, or AOP command access.
4. Ask the secure platform manager to perform only the reviewed PAS/SCM boot
   operation. That manager constructs descriptors from fixed policy and
   validates all ranges and VMIDs. It reports success only after secure
   firmware accepts/authenticates the image and releases the modem.
5. Wait for the platform's modem AFTER_POWERUP event. Establish a fresh modem
   epoch and prove exclusive AP ownership of the GLINK/SMEM resources before
   the broker resets AP-owned ring indices. No directory rescan or VERSION
   negotiation may be used to infer ownership.
6. Mint a new peer-generation capability for GLINK. WLAN remains stopped until
   its own MSA ownership transition is confirmed and SMMU routes are
   device-specific, translated, and restricted to WLAN-owned buffers.
7. Run WLFW indication registration, MSA info, verified MSA handoff, MSA ready,
   and capability negotiation in order. Only then may the driver consider
   later network milestones.

## Stop, crash, and recovery contract

On normal stop, the lifecycle manager first invalidates the generation, rejects
new WLAN/GLINK calls, stops queue production, and waits for WLAN and modem stop
acknowledgements. It then requests PAS shutdown and the named platform
power-down sequence, and independently confirms transport/DMA containment
before returning to `OFF`. The full modem reservation stays excluded from all
other domains and allocators for the system lifetime; this design never
returns that range to general memory. Reopening transport requires a new
successful boot generation, not merely a shutdown acknowledgement.

On watchdog, crash, timeout, malformed ring state, or uncertain secure result,
the manager invalidates the generation and quarantines the modem/MSA regions.
It disables the WLAN SMMU route and keeps the fixed pools unavailable. Recovery
requires independently verified reset, successful PAS shutdown, the named
power-down sequence, and confirmed transport/DMA containment; a successful
restart notification, a GLINK handshake, or a UI reset is insufficient. If any
of those signals is unavailable, remain quarantined and require a full platform
reboot under a trusted boot path.

## Preconditions before any hardware operation

The service design is not authorization to start the modem. All of these gates
remain open:

- The installed MDT, all 31 segment paths, and the two service manifests now
  have an in-place hash/size inventory recorded alongside the reported vendor
  fingerprint and runtime modem build label in
  `platforms/sunfish/wlan-resources.json`. The build label comes from
  `/vendor/firmware_mnt/verinfo/ver_info.txt`; neither it nor the fingerprint
  proves which modem partition revision supplied these bytes. Before use,
  still establish the exact compatible source/release, licensing, and
  calibration inputs, then compare this inventory with that immutable release.
  Treat Qualcomm PAS authentication as a separate secure-world check; local
  hashes do not authenticate the firmware. No authenticated project manifest
  exists yet.
- Review the exact Sunfish secure-monitor/PAS ABI and descriptor layout against
  the matching platform source and secure firmware contract. The Linux
  service/command IDs and argument counts above are known, but the complete
  Microkit SCM ABI is not reviewed. The existing seL4 SMC forwarding path is
  not a Qualcomm SCM implementation.
- Prove that the runtime DT's modem range, MDT load span, active MSA pool, and
  all dynamic reservations are excluded from Microkit allocators and domains.
  Keep the modem reservation fixed for the system lifetime because the matched
  Sunfish node has no `qcom,mem-protect-id` and the Linux modem path does not use
  its optional hypervisor VMID-assignment flow for this range.
- Identify the exact AOP requests, dependencies, ordering, timeout, and rollback
  behavior, including RPMh regulator/clock and bus-vote equivalents; prove the
  manager can receive AFTER_POWERUP, AFTER_SHUTDOWN, watchdog, and crash
  outcomes. The traced AOP `load_state` notification alone does not satisfy
  this gate.
- Review PAS memory-setup and shutdown semantics with the secure firmware
  contract. Do not invent or assume a VMID ownership transfer for the modem
  range; the matched Sunfish loader does not configure that path for it.
- Establish a safe SMMU takeover and a translated WLAN-only stream route before
  any WLAN DMA or MSA exposure. Current route inventory is descriptive and
  does not establish native SMMU ownership.
- Define and test reset behavior for partial boot, lost IPC, manager restart,
  and power loss. Persisted generation state must never make a stale peer valid.

Until those items are resolved, the lifecycle service's modem-start path
stops before PAS operations, firmware load, reset release, shared-ring writes,
or WLAN DMA. The isolated active-resource experiments and SCM INFO queries
above are narrower standalone probes, not a full lifecycle transaction.
The standalone AOP control-handshake and fixed OFF packet probes are transport
experiments: exclusive standalone AP endpoint, pinned measured layout,
no regulator/clock vote, no SMC, and no modem/WLAN
memory mapping. They do not satisfy this startup contract. The host model and
MDT parser are useful policy/build checks; neither proves these platform facts.

## Working Lineage host-service comparison (2026-10-20)

The fatal trace establishes a modem-reported initialization stall, but does
not identify the stalled task or establish a defective firmware image. A
working Lineage boot provides additional host-side evidence. The targeted
capture is in
`.cache/pixel-sunfish/lineage-modem-baseline-20261020T193451Z/`; the follow-up
namespace and storage metadata capture is in
`.cache/pixel-sunfish/modem-host-service-trace-20261020/`.
[`modem-host-services.json`](../platforms/sunfish/modem-host-services.json)
records the service tuples, missing implementations, source hashes/line
anchors, evidence limits, and implementation order.

All 32 installed modem MDT/segment files match the earlier inventory. This
compares observed device bytes; it does not authenticate an immutable vendor
release or prove runtime staging. The modem was ONLINE with crash count zero
at Linux uptime 282.65 seconds and remained so at 661.84 seconds. A bounded
local QRTR namespace lookup completed with 123 server records. The helper
published no service, issued no service-specific request, and was removed;
ADB was returned to UID 2000. No reboot or partition operation was performed.

| Host role | Working Lineage registration | Gap in the fatal-trace path |
| --- | --- | --- |
| Remote modem storage | Service `0x0e`, version 1, instance 0, node 1 | No publication, OPEN/ALLOC/IOVEC handlers, storage backend, or modem-visible transfer pool |
| Heap-memory sharing | Service `0x34`, version 1, instance 1, node 1 | No publication, QUERY/ALLOC/FREE handlers, client leases, or assigned heap pool |
| IPA control | Service `0x31`, version 1, instance 1 on node 1; modem instance 2 on node 0 | No equivalent control handshake/resource setup; its necessity for this stall is unproven |
| TFTP file service | Service `0x1000`, versions 1 through 12 on node 1 | No equivalent file server; this capture does not identify requested files |

The source also distinguishes three memory contracts: the remote-storage
transfer pool, the generic MPSS heap-memory service, and WLAN's own MSA. These
must not be represented by one buffer or one ownership rule.

### Remote storage

The working log shows four OPENs for `/boot/modem_fs1`, `/boot/modem_fs2`,
`/boot/modem_fsg`, and `/boot/modem_fsc`, a 2 MiB buffer request, and subsequent
reads. The public
[`rmtfs` protocol](https://github.com/linux-msm/rmtfs/blob/b30a3eb38f9af283f18dbd3c7755653efc52c094/qmi_rmtfs.qmi)
defines OPEN/CLOSE/RW_IOVEC/ALLOC_BUFFER/GET_DEVICE_ERROR as messages 1 through
5. Its
[`storage.c`](https://github.com/linux-msm/rmtfs/blob/b30a3eb38f9af283f18dbd3c7755653efc52c094/storage.c)
maps those four paths to `modemst1`, `modemst2`, `fsg`, and `fsc`. Lineage's
actual daemon has the matching block devices and `/dev/uio0` open. No partition
contents were copied for this investigation. The public daemon is a reference
implementation, not proof of the vendor binary's exact wire encoding.

The matched
[`msm_sharedmem` driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/uio/msm_sharedmem/msm_sharedmem.c)
interprets the DT's zero base as a dynamic allocation. It allocates a 2 MiB
usable region plus 4 KiB guards on both sides and requests read/write access
for both HLOS (VMID 3) and MSS_MSA (VMID 15). The captured UIO base
`0xf4601000` is an observation of this Linux boot, not an address to replay in
a custom boot. The standalone probe system provides no corresponding pool,
memory-assignment operation, or native storage-controller backend.

IOVEC reads move real partition sectors into the shared pool before returning
success; writes move bytes in the opposite direction. The baseline also
contains a later 2 MiB write to modem_fs2, so persistence is not universally
unused. The upstream daemon's read-only mode uses a snapshot plus RAM write
overlay. That is a possible experiment backend when explicitly supplied with
the owned device's data, not a durable-storage implementation. A dummy success
or zero-filled replacement does not establish equivalent startup behavior.

### Heap-memory sharing

The matched
[`msm_memshare` driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/memshare/msm_memshare.c)
registers service `0x34` and handles ALLOC `0x22`, FREE `0x23`, and QUERY_SIZE
`0x24`. The captured DT configures modem client 1 / processor 0 for 5 MiB,
allocated on request. The working log shows a successful size query and then
a 5,242,880-byte allocation response for that client.

This allocation uses `DMA_ATTR_NO_KERNEL_MAPPING` and requests an ownership
change from HLOS to MSS_MSA alone, with read/write permission. HLOS is retained
in the storage-pool destination list but absent from this heap-pool list.
Allocation metadata, sequence IDs, peer generation, and lifetime must therefore
be separate from CPU-visible payload access. The observed client's DT does not
request a guard band. The Linux helper logs assignment errors without returning
them to the allocation handler, which fills its QMI result first. Consequently,
the success log alone does not independently prove the VMID transfer succeeded;
our implementation must gate address exposure on its own confirmed result.

### IPA modem initialization

The working boot completes `QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` and receives its
response before ICNSS connects to the WLFW service. The pinned IPA v3 source
uses QMI service `0x31`, version 1, modem instance 2, then constructs message
`0x0021` from the live IPA platform type, SRAM/table partition offsets and
sizes, SMEM restriction offset, endpoint allocation, hash-table capability,
and uC-loaded/SSR state. These inputs come from the running IPA platform; a
static all-zero request would not match the Linux contract. The baseline did
not capture the raw request fields, and the modem's later fatal trace does not
prove this exchange is required to bring up WLAN. Keep it as an ordered
startup dependency to investigate, not as a demonstrated root cause.

### Required transport and implementation work

The original fatal-trace probe remains a discovery-only path: it accepts fixed
52-byte QRTR `NEW_SERVER` announcements, and it cannot dispatch QMI service
requests. Separate offline modules now implement bounded QRTR routing, QMI
storage/heap request decoding, variable-length GLINK assembly, service handlers,
and a composed fake-provider path. The minimum heap ALLOC datagram is 67 bytes
(32-byte QRTR v1 header, 7-byte QMI header, and four 7-byte mandatory TLVs),
before optional fields; storage IOVEC arrays require larger bounded messages.
The discovery probe's 64-byte scratch buffers remain too small for those
messages.

The host-side schemas, handlers, pools, owner callback path, and host-file
provider pass offline tests. They are not attached to the native modem owner:
verified per-device modem partition data, native UFS bring-up, a fresh-boot
HLOS/SCM ownership contract, real secure-memory assignment, and native service
callbacks remain missing. The host-file sync/reopen test does not prove Pixel
UFS durability. The SMMU planner is likewise offline only; the 2026-10-21
readback reports no displayed route for WLAN SIDs `0x240`, `0x241`, `0x520`,
`0x521`, and `0x522`, while observed UFS CB2 has translation disabled. Global
value `0x002A0406` indicates client processing and fault handling are enabled,
but it does not delegate SMMU control or authorize WLAN DMA.

Before another integrated modem boot, the remaining work is to supply trusted
data and native storage/SCM owners, establish a fresh complete SMMU snapshot
plus explicit control/ownership delegation, and determine whether the
resource-dependent IPA `INIT_MODEM_DRIVER` exchange belongs in the native path.
Only then can the already-tested service path be attached and produce a
meaningful device hypothesis. The prior full offline regression suite passed;
no device action was performed for this audit. The watchdog reason still does
not identify which missing dependency caused the custom modem failure.
Linux uptime and probe elapsed time have different origins and must not be
compared as one clock.

### Offline QMI routing and decoding implementation (2026-10-20)

The first QMI layer is now implemented independently of the native probe:

- [`qcom_qmi.rs`](../rust/src/qcom_qmi.rs) validates the seven-byte service
  header, message kind, exact payload length, and at most 16 unique TLVs. It
  rejects truncated fields and duplicate types. Unknown optional TLVs are
  tolerated within those bounds; unsupported mandatory fields are rejected by
  each request schema.
- [`qcom_modem_services.rs`](../rust/src/qcom_modem_services.rs) decodes the
  storage OPEN/CLOSE/RW_IOVEC/ALLOC/GET_DEVICE_ERROR requests and memory
  QUERY_SIZE/ALLOC/FREE requests. Arrays borrow validated input instead of
  allocating from peer-supplied counts. Initial profile limits are 255 storage
  IOVECs with at most 2 MiB total transfer, 64 heap-free entries with at most
  5 MiB total, and the four captured modem storage paths. Addresses and handles
  remain data; decoding does not validate their ownership or execute an action.
- [`qcom_qmi_router.rs`](../rust/src/qcom_qmi_router.rs) accepts complete,
  unpadded QRTR v1 DATA datagrams of at most 4096 bytes. Two locally configured
  service endpoints route requests by destination port and pinned peer node.
  Eight pending transactions provide bounded backpressure. Invalid requests
  reserve no slot. Duplicate in-flight transaction keys are rejected; a private
  receipt preserves reply identity and is checked against its router instance,
  generation, and slot serial when retired. Transaction IDs may be reused after
  completion; this is correlation, not a persistent replay filter.

All three modules forbid unsafe code and use no heap allocation. The transport
generation is supplied by trusted local state, not parsed from a peer packet.
Starting a strictly newer generation clears endpoint registrations and pending
requests. A future writer must revalidate the generation when publishing a
response, even if it obtained a reply route earlier.

Run the standalone offline checks with:

```sh
python3 platforms/sunfish/test-modem-qmi.py
```

The runner now executes 30 routing/decoding tests, 38 GLINK exchange tests,
20 QRTR control tests, 29 service-handler tests, 38 pool/SCM tests, 60 FIFO/owner tests (33 FIFO
plus 27 owner), six live-gate tests, and five native handoff tests. The host
file/composed-backend crate adds 23 storage/backend, ten file, four complete
QMI request, and two FIFO-owner service integration tests (39), including two fresh subprocesses inside its persistence test. Compiler
warnings are denied; the two reused legacy fixture harnesses and shared host-test
helpers permit unused fixture APIs. It compiles the
eleven safe modules as an `aarch64-unknown-none` `no_std` library, then compiles
the separate volatile backend and concrete native adapter entry points for
that target. All 265 Rust tests and both target builds passed. Twenty-three additional
Python mapping/capability tests pass (288 total). Fixtures are
hand-authored/source-derived, not captured service-specific
phone traffic. Core tests include literal OPEN and 67-byte ALLOC messages,
every truncation of the allocation datagram, deterministic wire mutations,
malformed TLVs/arrays, size ceilings, endpoint isolation, queue exhaustion,
request correlation, and stale generations/receipts.

### Offline GLINK receive/reply connection (2026-10-20)

[`qcom_glink_qmi.rs`](../rust/src/qcom_glink_qmi.rs) now connects the QMI router
to a bounded transport state machine. It assumes GLINK version/features and an
IPCRTR channel have already been negotiated. The FIFO adapter described below
passes one complete aligned record at a time with a trusted generation. FIFO
wrap/partial records, memory barriers, cursors, and notifications are kept
outside this session core.

The implemented path is:

1. Queue and publish an INTENT backed by a private 4096-byte receive slot.
   Incoming intent requests receive a bounded grant/deny acknowledgement;
   granted backing is advertised before ACK. An unpublished intent cannot be
   used, and local intent IDs never repeat in the session.
2. Copy TX_DATA/TX_DATA_CONT fragments into that slot. The declared remaining
   length must decrease exactly, with at most 64 nonempty fragments and 4096
   total bytes. Interleaved intents have separate buffers. QRTR's four-byte
   body padding is removed separately from GLINK's eight-byte record padding.
3. Route and decode a complete request before queueing its one-shot RX_DONE.
   A private request handle exposes borrowed decoded fields. A malformed
   complete message is held without RX_DONE or an automatic response until a
   trusted reset. No peer address is dereferenced.
4. Serialize an explicit backend result with the sealed source/destination,
   transaction, and message ID. The codec adds the standard result TLV and
   validates optional output TLVs structurally. The backend remains responsible
   for response-specific fields and for actually performing an authorized
   action. A successful mock QUERY_SIZE fixture is not an allocation.
5. Select a peer receive intent large enough for the whole padded datagram,
   fragment the reply into at most 256-byte chunks, and publish one record per
   call. QRTR credit packets have priority and can bypass a reply waiting for a
   larger intent. Missing capacity applies backpressure.
6. Retain the QMI transaction until both GLINK RX_DONE and QRTR RESUME_TX have
   arrived. Every reply requests QRTR confirmation, with one outstanding reply
   per peer destination flow. An inbound request with confirm_rx set gets a
   separate QRTR RESUME_TX packet; that credit never implies QMI success.

There are eight receive slots, eight transmit slots, 32 active remote
intents, and four pending service requests. The buffers alone use 64 KiB of
private memory, in addition to bookkeeping and stack requirements. Receive
intents are one-shot; callers replenish them as slots become free. Remote
RX_DONE_W_REUSE is accepted only when its feature was negotiated. New remote
intent IDs must exceed the previous high-water mark (order within a batch is
free); cyclic/repeated re-advertisements require a trusted generation reset.
This conservative policy prevents an old completion from finding a newly
advertised same-ID intent, but is narrower than the complete Linux driver.
Negotiated reusable intents still rely on the peer's ordered completion
protocol. QRTR credits contain no transaction token and are flow control,
not authentication or a persistent replay defence.

`publish_next` rechecks the generation while holding exclusive session access
and calls a synchronous writer. A successful callback means the complete
record was published exactly once. WouldBlock means no bytes were published
and permits an identical retry. Any uncertain publication quarantines the
session (as does an unwinding host writer); it cannot publish, ingest, or accept
actions until the native owner has stopped/drained the old transport and
performed a trusted reset. Incrementing
a logical generation does not revoke DMA or establish SCM memory ownership.
An outer lifecycle controller must provide deadlines and peer-loss handling;
the core retains outstanding buffers rather than freeing them on a timeout.

The exchange tests cover fragment interleaving and corruption, every record
truncation, deterministic byte mutations, literal ALLOC padding, both maximum
4096-byte paths, failed/retried/uncertain publication, intent grants/denials,
queue exhaustion, exact reply identity, early/duplicate/wrong completions,
both acknowledgement orders, credit routing, stale handles/generations, and
a mock QUERY_SIZE exchange through request/reply/completion.

The native FIFO adapter and owner wiring are implemented below. Remaining
work includes full control-channel dispatch, HELLO/service announcements and
other QRTR control traffic, outbound GLINK intent requests, storage/memory
action handlers, backing data, and SCM pool ownership remain. The current
fatal-trace image does not use this core. No device command or temporary boot
was performed; the Pixel was left in LineageOS.

### Native SMEM FIFO adapter, tested offline (2026-10-20)

[`qcom_glink_fifo.rs`](../rust/src/qcom_glink_fifo.rs) owns a bounded QMI session
and its FIFO IO together. It follows the pinned Lineage
[SMEM ring layout](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/rpmsg/qcom_glink_smem.c):
two 16 KiB rings and a 32-byte descriptor. Word0 is peer-owned TX tail, word1
AP TX head, word2 AP RX tail, and word3 peer RX head. The four reserved words
are pinned and never written. Attachment does not initialize or clear an
existing ring. The passed session must have no live transport state.

The adapter processes at most one record per receive or transmit call:

- RX determines the bounded length from the GLINK header, supports independent
  header/body/padding arrival, and copies across ring wrap into a private
  4112-byte record buffer. Incomplete input leaves the tail and session alone.
  A second read checks that the unconsumed record did not change; legal peer
  head progress is allowed. Session ingestion completes before a release
  barrier and the AP RX tail store. The adapter never writes the peer RX FIFO.
- TX preserves the driver's 16-byte reserve (eight for full/empty distinction
  and eight for READ_NOTIF). It stages one complete record, checks readback,
  revalidates lifecycle/cursors, orders the bytes before one AP TX head store,
  and completes the cursor store before notifying the peer. It writes only
  the actual free TX span, including wrap. A blocked session record is retained;
  one READ_NOTIF may use the extra reserved eight bytes. The adapter tracks its
  consumption so repeated polls cannot fill the ring with notifications.
- Incoming READ_NOTIF is handled as a transport record: consume its eight
  bytes before notifying. Other supported channel records enter the bounded
  session. VERSION/OPEN/CLOSE, other-channel traffic, SIGNALS, and unimplemented
  commands halt without consumption; an outer GLINK controller must handle
  those before/around this IPCRTR adapter. Unsupported records are never skipped
  by guessing a length.
- Every cursor is checked for range/alignment. AP cursor changes, peer
  consumption beyond published TX, peer RX overwrite, reserved-word changes,
  and ready/fatal/generation/identity failures stop the adapter. IO failures
  and unwinding IO also stop it, including a cursor store that took effect
  before reporting failure. An event is not returned before RX consumption
  succeeds. An uncertain RX result cannot leak queued replies through TX.

[`qcom_glink_native_io.rs`](../rust/src/qcom_glink_native_io.rs) is the narrow
unsafe boundary. It accepts already authorized, aligned, disjoint mappings for
SMEM items478/479/480 and the exact APCS IPC register at base+0xc, writes only
AP-owned descriptor words and TX bytes, and rings fixed bit12. All accesses are
volatile and little-endian. All three ordering points use AArch64 `dsb sy`,
with compiler fences. The non-AArch64 fence supports host tests; it is not a
substitute for the target's device ordering. The FIFO and protocol modules
continue to forbid unsafe code.

Native construction uses the live owner `Gate` described below to revalidate
live ready/fatal state, locally managed generation, and the exact mapped
allocation/route identity. A constant test gate is not a production lifecycle
observer. The owner manifest and runtime symbol/layout checks provide the
fixed mapping profile and serialize AP writers and IRQ callbacks. Raw pointers
or a GLINK packet alone cannot establish those facts. Actual mapping attributes
and IRQ delivery have not been tested on the phone.

Logical reset requires a strictly newer observed generation, the same mapping
identity, and both rings empty. It clears session state and registrations but
does not rewrite FIFO cursors. The caller must already have stopped/drained the
old peer and serialized writers/IRQs; an empty ring or incremented generation
does not itself prove hardware quiescence. A relocated allocation needs new
validated mappings. Deadlines, IRQ registration, allocation discovery, channel
negotiation, and memory ownership remain the outer lifecycle owner's work.

The new 33-test suite includes every one of the 2048 aligned ring starting
positions, wrapped partial and maximum-size records, peer progress during
copy, reserve boundaries and READ_NOTIF, malformed input, all cursor ownership
checks, staged-data corruption, readiness loss after staging, IO failure before
and after cursor effects, unwinding IO, reset rejection, and a mock QUERY_SIZE
exchange through both rings and both completion protocols. The actual volatile
backend is also exercised on aligned, guarded private host allocations.
The target build generates concrete receive/transmit entry points and checks
emitted assembly for system barriers and byte/cursor accesses. This validates
code generation, not real peer ordering, cache attributes, interrupt delivery,
modem initialization, or the watchdog fix. No ADB/Fastboot command was run.

### Modem owner, lifecycle gate and IRQ wiring (2026-10-20)

The explicit `sunfish-modem-owner` feature now connects the native fresh-boot
path in [`sunfish_modem_smp2p_probe.rs`](../rust/src/sunfish_modem_smp2p_probe.rs)
to [`pixel_modem_owner.rs`](../rust/src/pixel_modem_owner.rs). It consumes the
actual authenticated `ChannelCheckpoint`, preventing the old polling writer
from continuing. Owner mode is incompatible with the later packet/crash probe
modes. It is not enabled by any existing diagnostic build script.

The handoff checks the exact established IPCRTR profile and all descriptor
words, copies and validates the 88 negotiation bytes, then advances RX tail
from 8 to 96 once. The two secondary OPEN requests (`glink_ssr` and
`ssm_rtr_modem_apps`) remain privately retained and unacknowledged; they have
not been implemented as accepted channels. Incoming intent records remain in
the FIFO for the bounded adapter. No FIFO indices are zeroed or reset.

[`pixel_modem_owner_gate.rs`](../rust/src/pixel_modem_owner_gate.rs) owns the
consumed ready receipt and the only live SMEM lock observer. Every observation
acquires/releases TCSR lock 3, repeats allocation-directory snapshots, validates
the exact AP status payload and peer item/route identity, checks ready/fatal
and stop/shutdown bits, and compares the transport allocation identities.
Unrelated valid directory growth is allowed. A failed gate stays failed, even
if a later sample becomes ready. The local generation is fixed to this one
consumed boot; there is no owner restart API or generation minting from an IRQ.

[`qcom_modem_owner.rs`](../rust/src/qcom_modem_owner.rs) owns the FIFO adapter,
four pending typed request handles, and IRQ/deferred-work state. It handles
GLINK and SMP2P callbacks on the same Microkit thread. A callback performs at
most 16 receive/transmit rounds. If work remains it retains masked IRQs and
signals a local continuation; it does not recurse or poll without a bound.
Once drained, it revalidates before each IRQ ACK, status first. A post-ACK
continuation closes the empty-check/rearm window without requiring another
peer edge. Owner mode overrides the pinned Rust Microkit event loop because
its default startup path unconditionally ACKs all IRQs after init. The owner
explicitly tracks both initial IRQs as pending: the first unmask follows the
same drain and lifecycle checks, including continuation after budget exhaustion.
Failed init never enters an automatic IRQ-ACK path. This is an active PD with
no deferred kernel actions. Backend replies schedule their own work. Failed or uncertain ACKs,
notifications, IO, and lifecycle checks stop the owner without releasing its
session or mappings. There is no automatic reset/retry or false QMI success.

[`modem-owner.system`](../platforms/sunfish/modem-owner.system) retains the
existing five uncached AP RW pages and RO SMEM remainder, adds edge IRQ 481
(GLINK, channel 0) and IRQ 483 (SMP2P, channel 1), and supplies a local output 2 to
input 3 continuation channel. The IRQ routes match the pinned
[board source](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/atoll.dtsi).
The owner uses kernel IRQ capabilities; no GIC mapping or direct GIC write is
added. Stack allocation is increased to 512 KiB for the existing image staging
path plus the bounded owner/session buffers; native stack high-water remains
unmeasured. Other mappings retain the existing boot/resource/display scope.

An RX allocation may start in the final AP allocation page. Page permissions
therefore cannot make every RX byte hardware RO while still writing the end
of TX. The layout requires disjoint actual descriptor/TX/RX byte ranges and
the volatile backend exposes no RX write; page-sharing does not add another
writable page or a physical alias. Runtime injected symbols must match the
fixed contiguous mapping profile before the first owner-mode SMEM access.

Run the offline checks and compile/capability-layout validation with:

```sh
python3 platforms/sunfish/test-modem-qmi.py
python3 -m unittest tests.sunfish_modem_owner_manifest_test -v
sh platforms/sunfish/build-modem-owner-offline.sh
```

The owner-wiring checkpoint passed 138 tests; the QRTR extension below raises
that to 163. The target owner PD links, the Microkit tool constructs
the system/capability layout, and the generated CapDL is checked for the
exact IRQ handler slots, edge triggers, notification objects, and continuation
badge. The existing loader-relocation helper is used in a private SDK because
the default SDK loader address overlaps the embedded firmware payload. The
base SDK and older diagnostic artifacts are unchanged. Cargo still reports
existing unused-code/import and toolchain-deprecation warnings; this target
build is not a warnings-clean assertion. No Android/Fastboot image is produced
and nothing is booted or flashed.

The native handoff host tests call the actual handoff, SMEM/lock gate,
volatile backend and owner code on retained host buffers. Only the secure/peer
fixtures and Microkit kernel calls are simulated. They verify the one tail
commit, subsequent native FIFO writes, and refusal after malformed negotiation,
invalid cursors, or live ready loss. They also check initial unmask order and
deferral across a bounded drain. Static compilation and CapDL checks do not
prove physical mapping/cache behavior, GIC delivery, seL4 boot success, modem
initialization, or watchdog recovery. The bounded QRTR extension below now
handles direct-peer control and announcements. Secondary-channel and SMP2P
service dispatch, real service backends, and memory ownership remain before
an integrated modem test. The Pixel stayed in LineageOS throughout.

### Bounded QRTR control and service announcements (2026-10-20)

[`qcom_qrtr_control.rs`](../rust/src/qcom_qrtr_control.rs) adds the direct-peer
QRTR-v1 controller. The native runtime enables it during fresh attachment,
before advertising any receive intent. Raw codec-only sessions retain their
explicit simulation mode; the native owner always enables the controller.

The decoder checks the complete 52-byte control datagram, version, payload
length, matching header/body command, source node, control ports, destination
(local or broadcast), and command fields before changing state. It supports
HELLO, NEW_SERVER, DEL_SERVER, DEL_CLIENT, BYE and the matched DEL_PROC format.
PING/EXIT are consumed without an echo. A remote server table has 64 slots;
duplicates coalesce, a port can change its descriptor, and a stale deletion
cannot remove its replacement. Overflow stops without eviction. No remote
announcement registers a local decoder or grants storage/memory authority.

The wire layouts follow the pinned
[Lineage QRTR ABI](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/include/uapi/linux/qrtr.h)
and [transport](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/net/qrtr/qrtr.c).
The service instance packing follows
[libqrtr](https://github.com/linux-msm/qrtr/blob/29e36ae164389580a0f8ea7a7fdb728140ae978d/lib/qrtr.c).
This is a direct-peer profile: forwarded nodes and remote NEW_LOOKUP/DEL_LOOKUP
requests are rejected. Those lookup commands are local name-service APIs in
the [Linux name service](https://github.com/torvalds/linux/blob/v6.12/net/qrtr/ns.c).
Unknown or malformed records remain held by the existing FIFO fault path.

One local HELLO is queued; duplicate peer HELLOs do not generate more HELLOs
or repeated service listings. Local service transitions wait for both the
peer HELLO and publication of the local HELLO. Two readiness/announcement
records coalesce desired changes. Encoded control datagrams use the existing
eight TX slots and remote intent reservations. They request no QRTR credit,
but their buffers remain owned until GLINK RX_DONE. A blocked or uncertain
writer cannot mark an announcement sent. QMI replies still retain their
transaction until both GLINK completion and the matching RESUME_TX.

Decoder registration is separate from backend readiness. The trusted owner
API `set_service_available` requires a registered endpoint and schedules
bounded work through the local continuation channel. It announces service
`0x0e` with packed instance `1` and service `0x34` with packed instance `257`
only when explicitly enabled. Withdrawal emits DEL_SERVER. A backend must
remain alive for queued transitions, outstanding requests and leases; this
API does not revoke memory or complete service actions. New QMI requests are
accepted only after readiness and announcement publication. The native
runtime has no storage/memory action backends, so its readiness flags remain
false and it emits no NEW_SERVER for those services. Offline fixtures enable
test services to verify the wire exchange.

The controller maintains a target of two backed one-shot receive intents
within the existing eight RX slots. Consumed control buffers are retired only
after RX_DONE publication; fresh IDs never reuse a prior ID. Exhausted native
TX space or peer intents use the existing backpressure path. All tables,
fragment bounds and the 16-round owner callback budget remain fixed. BYE or
direct DEL_PROC latches peer loss. A DEL_CLIENT/DEL_SERVER affecting a retained
QMI request stops with the request, native RX tail and buffers held. No control
packet restarts the modem or fabricates a new local generation.

At the QRTR checkpoint, the runner passed 154 Rust tests plus nine
mapping/capability tests (163 total). The 25 new cases cover literal wire layouts, malformed/truncated and
spoofed controls, registry capacity, duplicate suppression, fragmentation,
readiness, withdrawal, backpressure, uncertain publication, receive replenishment,
coexistence with QMI credits, and lifecycle failures. The native-buffer test
uses the real handoff, live SMEM gate, FIFO and IRQ owner to decode peer HELLO
and GLINK completion, while verifying that unready services stay unannounced.
The AArch64 safe core/native codegen and actual Microkit PD/system build pass.
Evidence is recorded under
`.cache/pixel-sunfish/modem-qrtr-control-offline-20261020`.

The following extension supplies bounded storage and heap action handlers.
Native backing data, remote pools and SCM ownership, secondary GLINK/SMP2P
service handling, production shutdown/recovery, deadlines, and hardware/cache/IRQ
proof remain. No device command
was run and the Pixel remained in LineageOS.


### Bounded storage and heap actions (2026-10-20)

[`qcom_modem_handlers.rs`](../rust/src/qcom_modem_handlers.rs) implements the
storage OPEN, CLOSE, ALLOC_BUFFER, RW_IOVEC and GET_DEVICE_ERROR requests and
heap QUERY_SIZE, ALLOC_GENERIC and FREE_GENERIC requests. It is allocation-free,
forbids unsafe code, and owns the backend, private progress state, cached reply,
eight storage handles, and one active lease for each service. Only one request
executes at a time; other requests stay in the owner's bounded queue.

The response fields follow the pinned
[storage schema](https://github.com/linux-msm/rmtfs/blob/b30a3eb38f9af283f18dbd3c7755653efc52c094/qmi_rmtfs.qmi)
and [heap schema](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/memshare/heap_mem_ext_v01.c).
OPEN returns a u32 handle, storage allocation a u64 address, and device-error
queries a u8 status. Heap query returns its configured capacity; allocation
returns the sequence and one address/length pair. Standard result/error fields
are encoded by the existing session and retain the request's transaction and
message identity. No physical address is returned after a failed or uncertain
grant. These wire fixtures are source-derived, not a capture of proprietary
service-specific traffic.

A validated profile supplies fixed partition bindings, capacities, write and
durability policy, generation, node/endpoint identities, and disjoint storage
and heap reservations. Payloads must be page aligned, within the 2 MiB/5 MiB
ceilings, and separated from one another and supplied exclusions by page guards.
Storage addresses must fit its 32-bit IOVEC wire interval. The profile checks
arithmetic and layout; it does not establish real physical ownership. The
backend must verify the actual mappings, backing data, cache attributes and
ownership transitions before it can satisfy the interface.

Storage handles are scoped to the full peer address and never recycled within
this handler generation. The four existing path enums remain the allowlist.
All handles for the same peer may use its confirmed shared transfer lease,
matching the observed single allocation serving several open files. The lease
is reclaimed only when that peer's last handle closes, and only after confirmed
return to host ownership. A different peer cannot use its handles or pool.

Every IOVEC is checked before any transfer: handle and lease identity, total
size, disk capacity, granted buffer interval, zero-length entries, and overlapping
disk or buffer spans. The copied plan contains bounded offsets rather than
pointers derived from the peer. Each step performs at most one action IO with
at most 4096 payload bytes, plus bounded observation/receipt checks. Reads copy
a backing-image chunk into private memory, then publish it to the shared pool
on the next step. Writes first take a stable private pool copy, then write the
backing image. All writes sync before success, even when force_sync is false;
forced reads also require a durability-capable backend. Read-only writes and
unsupported durability requests fail before IO. A known partial failure returns
failure and latches device error; preceding chunks may already have changed.
This is not an atomic multi-IOVEC transaction.

Heap policy permits client1/processor0, one contiguous active allocation, and
alignments through 4096 bytes. QUERY defaults an absent processor to the
configured modem client. FREE can omit client/processor because the exact
active lease supplies the peer binding. The handler scrubs
host-owned pages in bounded steps before requesting a modem-exclusive grant.
Exact retries of the active allocation return the same block without another
grant; conflicting active requests and non-increasing retired sequences fail.
FREE accepts exactly the active block, owner and size; it cannot free arbitrary
addresses, duplicate blocks, subranges or another peer's allocation.

FREE has no sequence field. Freed heap pages therefore remain retired for the
whole generation. Later smaller allocations advance to new pages; a full 5 MiB
allocation consumes the reservation for that generation even after FREE. This
prevents an old FREE from releasing a newer allocation at the same address.
It deliberately does not implement a general reusable heap or scatter allocation.
The active lease holds an opaque backend receipt with a locally issued serial,
expected region, peer, generation and ownership role. Allocation and reclaim
success are checked against current observation and receipt identity. A backend
that reports success without the expected ownership state stops the handler.
Uncertain actions, lifecycle loss, identity changes or unwinding IO retain all
state and permit no retry or automatic release.

The owner binds the exact handler instance to its generation, nodes and both
registered endpoints before enabling readiness. `activate_handlers` validates
the backend and enables announcements. `notified_with_handlers` runs the bounded
transport callback and at most one service action step, scheduling local work
while an action progresses. Ordinary notification/reply methods reject bypass
after activation. Completed outcomes stay cached if the response flow or TX
queue is busy; retrying reply enqueue never repeats OPEN, storage writes, grants
or frees. The existing GLINK completion plus QRTR credit retirement stays intact.
Faults stop both objects, and transport ready loss after a successful grant
retains the lease without publishing the address.

The 29 new handler tests use deterministic private backing images and simulated
durability/ownership. Five added owner tests exercise the actual FIFO adapter,
routing, actions, encoded responses, both completion protocols, blocked reply
caching, handler-instance binding and lifecycle/ownership failures. Maximum
2 MiB transfers and 5 MiB allocation scrub through the same bounded steps.
At this handler checkpoint, all 188 Rust tests and nine mapping/capability tests passed (197 total). The
runner also forces concrete AArch64 code generation for the handler and paired
owner with a stand-in that cannot perform actions. The Microkit PD/system builds;
its native runtime still has no action backend attached. The RAM fixture's
committed image is a durability simulation, not proof of device persistence.
Evidence is under `.cache/pixel-sunfish/modem-service-handlers-offline-20261020`.

The next native step requires authorized per-device backing data or a persistent
block provider, disjoint physical reservations and mappings, and a narrow SCM
provider that returns truthful confirmed grant/reclaim receipts. Until those
exist, the native runtime leaves both services unannounced. Native lifecycle
teardown/recovery, deadlines, secondary channels, storage FORCE_SYNC indications,
and hardware/cache/IRQ verification also remain. No ADB, Fastboot or other device
command was run; the Pixel remained in LineageOS.

### Reserved service pools and bounded SCM ownership (2026-10-20)

[`qcom_modem_pools.rs`](../rust/src/qcom_modem_pools.rs) now owns the fixed
reservation profile and private grant/reclaim receipts. The offline owner
manifest allocates these fresh seL4 frames; these addresses are chosen for this
profile, not replayed from Linux's dynamic allocations:

| Resource | Physical payload | Bytes | Virtual payload |
|---|---|---|---|
| Storage transfer | `0xa2001000` | `0x200000` (2 MiB) | `0x61001000` |
| Modem heap | `0xa2401000` | `0x500000` (5 MiB) | `0x62001000` |
| SCM arguments | `0xa2a01000` | `0x1000` | `0x63001000` |

Each resource has a separately reserved 4096-byte physical page on both sides.
Those six guard frames have no PD mapping or frame capability, and corresponding
virtual guard holes stay unmapped. All payload mappings are uncached, RW/NX and
exclusive to the owner. `validate-modem-pools.py` checks all 1,799 physical pages,
walks actual generated page tables from the owner VSpace root, checks every
reference to reserved frames, rejects aliases/guard mappings and checks actual
allocation-report addresses for dynamic frames too. Both cached kernel/live DT
RAM banks and fixed exclusions pass. The whole loader, expected wrapped source,
initial task and eleven actual copy targets are disjoint from these reservations.
These are offline allocation/mapping proofs, not secure-world ownership proof.

The manager checks injected geometry, generation, identity and lifecycle at
attachment and around every external action. Every granted page needs a completed
whole-page scrub; a partial scrub never certifies the page. Data calls accept at
most 4096 bytes and remain within the active receipt. Heap data APIs deny host
access after assignment. One active lease per pool holds exact region, peer,
generation and monotonic serial; foreign/stale receipts cannot reclaim another
lease. Reclaim clears the lease only after confirmed success and requires a new
scrub before the next grant. The existing handler retains its stricter rule of
never reusing freed heap pages within a generation.

The sealed call follows the pinned downstream
[`secure_buffer.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/secure_buffer.c)
and [`scm.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/scm.c)
path: SCM64 service MP/function `0x16`, `x0=0x42000c16`, argument-info
`0x1117`, one 16-byte range, fixed source VMIDs, and 24-byte destination records
with zero context/padding. Seven arguments use a private physical extra-argument
array through `x5`. Separate scratch cache lines hold the range, source list,
destination list and extra arguments. Storage grants HLOS (3) plus MSS_MSA (15)
RW (6); heap grants MSS_MSA alone RW. Reclaim names exactly those prior owners
and returns the range to HLOS RW. Caller/peer fields cannot select VMIDs,
permissions, SCM operation or arbitrary physical memory.

[`qcom_modem_pool_native.rs`](../rust/src/qcom_modem_pool_native.rs) implements
the bounded volatile data/scratch adapter with system barriers, scratch readback
and zero readback. `pixel_modem_pool_resources.rs` supplies the injected-symbol
checks and one private seL4 SMC-capability invocation. These adapters compile for
AArch64; no native instance is attached or invoked. Calls have a count bound of
one per transition, not a wall-time guarantee on secure world. There are no busy
retries, interrupted-call resumes, SMC32 fallbacks or automatic rollback calls.
Known staging failures before submission preserve the old lease. The manager
marks the target unknown before submission; any kernel/transport error, nonzero
monitor return, failed completion barrier or lost post-call lifecycle withholds
the receipt and stops further work. A failed reclaim retains the old receipt and
reservation. Unwinding external code latches the manager before it can retry.
Lifecycle observation failure invalidates both ownership claims. Nothing performs
SCM operations or releases mappings on Drop.

A trusted native gate must establish fresh-boot HLOS authority, exclusive
reservations/no aliases/no other users, exact SCM64 ABI, and a reviewed contract
that a zero monitor return confirms assignment. Call availability, mapped RAM,
Linux's locally updated owner mask, or a success log cannot establish that
contract. The manager refuses attachment without this explicit evidence. There
is no independent owner-readback API implemented here, and no native gate
asserts this evidence yet. This keeps the source-derived protocol separate from
an unverified claim about QHEE on the phone.

All 38 new pool/SCM tests use private host vectors and a recorded monitor stand-in.
They cover failure/unwind points before and after submission, ambiguous grant and
reclaim, scrub/bounds/identity/receipt failures, and actual service-handler
storage/heap allocation and release. An ambiguous assignment produces no address
response. Fourteen new Python tests check malformed reservations, actual page
table paths, guard capabilities, aliases and missing/stale allocation records.
The full suite passes 226 Rust plus 23 Python tests (249 total). Concrete pool
manager and volatile-adapter AArch64 code generation and the owner PD/Microkit
build pass. Existing target build warnings remain; host tests deny warnings.
Evidence: `.cache/pixel-sunfish/modem-pools-scm-offline-20261020`.

Remaining work is the device-specific backing store with truthful durable IO,
the native boot/SCM ownership gate, and composition with the paired owner before
announcing services. Native quiescence/recovery, secondary channels, deadlines,
stack high-water and hardware/cache/IRQ verification remain. No phone command,
partition access or live SCM transition was performed; the Pixel stayed in
LineageOS.

### Backing storage and composed service backend (2026-10-20)

[`qcom_modem_storage.rs`](../rust/src/qcom_modem_storage.rs) implements the
bounded storage controller. Four bindings pin partition enum, provider object,
trusted source/checkpoint digest, device fingerprint, geometry, write policy
and durability capability. The initial profile permits at most 64 MiB per
partition and requires nonzero 512-byte-aligned geometry. Duplicate provider
objects, missing bindings, invalid order and writable/nondurable combinations
are rejected. Profile parsing does not authenticate the supplied device or data;
a real provider must establish those bindings before reporting ready.

Data actions are sector aligned, checked for overflow and capacity, and limited
to one call of at most 4096 bytes. Each action checks current identity,
generation and all four bindings before and after IO. Short reads fail without
being retried. A short or ambiguous write stops the controller and retains the
dirty state. A known no-effect write failure preserves the preceding dirty
state. Successful writes remain dirty until an explicit provider flush and
post-flush lifecycle check both succeed. Flush failures withhold acknowledgement
and stop further work. An unwinding call cannot be retried. Dropping a storage
object never acknowledges a flush or releases SCM ownership.

[`qcom_modem_backend.rs`](../rust/src/qcom_modem_backend.rs) now implements the
existing handler `Backend` using both the storage controller and `Pools`.
Attachment requires the exact native pool geometry, matching generation and
configured disk metadata, clean storage and host-owned pools. The combined
identity belongs to this exact pair. Both resources are checked around every
data, metadata and ownership action; mutable provider access is not exposed.
A storage failure after a successful grant retains the pool and withholds its
receipt/address. Pool lifecycle loss after a disk write withholds success and
retains the unsynced state. Neither failure starts cleanup or rollback IO.
This generic composition is compiled with the actual paired owner and handler
entry points for AArch64, but no native instance is attached.

[`experiments/modem-storage-host`](../experiments/modem-storage-host) supplies a
concrete Unix regular-file provider for offline work. Trusted local setup opens
four existing files without create/truncate/append, checks exact size and
regular-file/inode/mode/link identity, holds exclusive advisory locks, and hashes
each file against an explicitly supplied current SHA-256 checkpoint. It rejects
symlinks, hard links, changed setup timestamps, lock conflicts, missing or
mismatched data and aliases. The locked file capabilities stay private; peer
paths never reach a filesystem operation. Live metadata checks detect unlinks,
replacement, resize and mode changes. Actual files use one positional read or
write per data action and explicit
[`File::sync_all`](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all)
for persistence. Positional operations can complete short, as described by the
[Rust FileExt contract](https://doc.rust-lang.org/std/os/unix/fs/trait.FileExt.html);
these outcomes follow the failure rules above instead of hidden retry loops.
The host-only crate pins cached RustCrypto `sha2` 0.10.9 in its own lockfile and
adds no dependency to the native Rust/Microkit build.

Locks are advisory and require cooperating exclusive users; they are not a
physical storage ownership proof. The trusted caller supplies the device
fingerprint, origin and current checkpoint. Hash equality verifies bytes against
that expected checkpoint, not its authenticity or rollback resistance. Mutable
files require an explicit updated trusted checkpoint on verified restart;
changes are never silently blessed as a new origin. File descriptors close on
explicit object/process teardown without a custom flush operation. Such closure
is not reported as a successful sync, and faults retain all descriptors in the
owned provider. These are host regular files, not a Pixel UFS driver.

The tests now use actual disposable files as well as injected providers. A
synced write survives file reopening and a fresh process, while other images
remain unchanged. Four tests route authentic sealed GLINK/QMI requests through
real handlers, the composed backend, file IO and the pool manager (SCM is still
a stand-in). They check actual data, bounded steps and encoded response results;
failed sync yields no successful response and retains the lease. The 23 storage
and composition tests exercise short IO, read-only policy, failed flush, changed
identity/geometry/device, uncertain operations and unwinding callbacks. Ten file
tests cover verification, locks and restart, with one ignored child-worker test
explicitly invoked twice by its parent. That helper is not counted as an extra
root test. All 265 Rust and 23 Python tests pass (288 total); AArch64 concrete
composition code generation and the Microkit owner build pass. Evidence lives
under `.cache/pixel-sunfish/modem-storage-backend-offline-20261020`; the owner path
checkpoint is `.cache/pixel-sunfish/modem-owner-services-offline-20261020`.

Two additional host tests execute the existing owner callback path over a
bounded fake GLINK ring. The successful path validates the composed backend,
accepts QRTR HELLO, publishes both service announcements, routes an OPEN request,
and checks the encoded response from the transmitted FIFO bytes. The failure
path removes the explicit pool ownership contract and confirms activation leaves
both services unavailable without invoking SCM. These use a fake pool/disk and
do not establish native storage or monitor authority.

### Native UFS boundary from saved sunfish device trees (2026-10-20)

The saved Lineage runtime tree places UFS at `0x1d84000` for `0x3000` bytes,
with a QMP PHY at `0x1d87000` and an inline-crypto/ICE block at `0x1d90000`.
The platform DTS names a high-level UFS interrupt, controller clocks, core
reset, interconnect votes, a power domain, an RPMh operating point and SMMU
stream ID `0x300`. The board DTS enables the controller, supplies device and
PHY rails, and uses GPIO119 for device reset. The captured Lineage tree shows
the active vendor-compatible nodes and voltage settings; the checked-in SM7150
DTS supplies the clock, interconnect and SMMU bindings.

The owner profile maps none of the UFS host, PHY or ICE ranges and has no UFS
interrupt or DMA/SMMU capability. A native `read_at`/`write_at` function alone
cannot access the phone's modem partitions. The next storage boundary needs
either a separate trusted UFS/block provider that owns controller bring-up,
power, IRQ and DMA, or explicitly provisioned modem images with verified
device/checkpoint identity and durable write support. The host regular-file
adapter provides neither. This audit reads saved device-tree snapshots only;
it did not access the phone or partition contents.

Only stored metadata was found in the checked existing Pixel backup/trace
locations; no modem partition images were opened or collected. All new data
fixtures are generated disposable host files. Verified per-device backing data
and a native storage provider (for example an authorized UFS/block service) are
still needed, along with the native boot/SCM ownership gate, before attachment
and service publication on the phone. A fresh-process file test is not a power
loss, controller-failure or Pixel persistence test. The Pixel stayed in Lineage;
no ADB, Fastboot, partition, native memory or live SCM action occurred.

### Sunfish UFS command encoding slice (2026-10-20)

I pinned the device-matched Google `kernel/msm` branch
`android-msm-sunfish-4.14-android13` at commit
[`6ef255005ceaae118d35fba8585de7ba1d4d5efb`](https://android.googlesource.com/kernel/msm/+/6ef255005ceaae118d35fba8585de7ba1d4d5efb/).
This replaces the newer `kernel/common` PHY page cited during the initial
resource audit. The Sunfish Qualcomm host driver binds the generic UFS host
controller, then sequences ICE discovery, PHY initialization and power, lane
clocks, bus votes, secure-configuration restoration and host clocks ([host driver](https://android.googlesource.com/kernel/msm/+/6ef255005ceaae118d35fba8585de7ba1d4d5efb/drivers/scsi/ufs/ufs-qcom.c)).
The matching QMP v3 PHY source programs calibration tables and waits for
PCS-ready ([PHY driver](https://android.googlesource.com/kernel/msm/+/6ef255005ceaae118d35fba8585de7ba1d4d5efb/drivers/phy/qualcomm/phy-qcom-ufs-qmp-v3.c)).
This confirms that a block provider needs a real platform owner and host
bring-up path in addition to command encoding.

The existing RPMh experiments cannot provide that owner: their resource hold
is limited to modem CX/MSS/XO votes, and the BCM path is read-only. The UFS DTS
instead names GCC/RPMh clocks, an RPMh power-domain operating point, two
interconnect paths, four supplies, and controller/PHY reset sequencing. No
checked-in component has authority to claim and operate that full set, so it
would be unsafe to compose the modem-specific RPMh writes into UFS startup.

[`qcom_ufs_protocol.rs`](../rust/src/qcom_ufs_protocol.rs) encodes bounded
TEST UNIT READY, standard INQUIRY, READ CAPACITY(10), READ(10), WRITE(10), and
SYNCHRONIZE CACHE(10) commands as UPIUs, UTRDs, and single-entry PRDTs. It
limits data transfers to 4096 bytes, validates sector and DMA alignment,
32/64-bit address limits and descriptor/data aliasing, and accepts completion
only for a successful OCS, matching response/tag, good SCSI status and zero
residual. Its bounded parsers accept only a direct-access LUN with a standard
36-byte INQUIRY response and 512-byte blocks; the READ CAPACITY(10) sentinel
that requires READ CAPACITY(16) is rejected. Eight offline protocol tests
cover command encodings, malformed/unsupported responses, completion failures,
and a write/sync/read round trip against a disposable image-backed device
model. No kernel driver code was copied into this module.

This is the command-packet layer only. It does not map MMIO, enable clocks or
regulators, initialize PHY/ICE, manage IRQs, create SMMU/DMA mappings, execute a
LUN discovery sequence, validate the modem partition table, or access block
media. The new parsers only validate synthetic command responses; they do not
identify a physical LUN. The native owner still has no UFS capabilities and
keeps service readiness disabled. The remaining implementation boundary is
controller/link bring-up behind the audited exclusive resource contract;
storage cannot be enabled until that and the SCM ownership gate are both
proven.

### Bounded UFSHCI control and IRQ core (2026-10-20)

[`qcom_ufs_host.rs`](../rust/src/qcom_ufs_host.rs) adds an offline, no-unsafe
host-controller core over a narrow register interface. It checks an explicit
live-platform readiness gate before register access, bounds HCE and UIC link
startup polls, requires the supported HCI 2.0/2.1 versions, validates aligned
and non-overlapping UTRDL/UTMRDL DMA ranges against the controller's 64-bit
address capability, programs queue bases only after link/device detection, and
quarantines the host after timeout, partial register failure, lost ownership,
or fatal IRQ status. The interrupt path reads and acknowledges a single status
snapshot. Its single-slot executor stages the command UPIU, PRDT and UTRD,
rings the UTRL doorbell once, waits for the transfer-complete IRQ, then validates
the returned OCS and response UPIU. It does not stage caller payload bytes.

Twenty offline tests exercise bring-up ordering, bounded timeout and UIC
failures, missing device, DMA width/alignment/overlap, queue readiness, single
command submission, completion validation, IRQ acknowledgement, and fail-closed
behavior. They also drive one read-only TEST UNIT READY / INQUIRY / READ
CAPACITY(10) sequence against a caller-selected LUN using fake register and DMA
banks. The sequence bounds tags and payload reads, stops before later commands
after a malformed response, and never retries. The `RegisterIo` platform
readiness method is only an interface contract; this repository has no native
implementation proving exclusive UFS/PHY/ICE ownership, clock/power/reset
sequencing, DMA/SMMU mappings, or IRQ delivery. No physical LUN discovery,
partition extent validation, or native Qualcomm resource bring-up exists.
Therefore this remains a testable HCI/command core, not a native storage
provider, and native service readiness stays disabled. The source sequence
follows the pinned Sunfish generic host implementation and its UFSHCI register
definitions.

### Bounded GPT partition validation (2026-10-20)

[`qcom_ufs_gpt.rs`](../rust/src/qcom_ufs_gpt.rs) parses caller-provided primary
and backup GPT headers and entry arrays without allocation. The paired parser
checks primary/backup locations, matching disk GUID and usable range, matching
entry geometry and byte-identical arrays, both header CRCs, both array CRCs,
reserved bytes, and a fixed 16 KiB array cap. It rejects invalid or duplicate
partition GUIDs, out-of-range and overlapping extents, and requires exactly one
entry for each of `modemst1`, `modemst2`, `fsg`, and `fsc`. Returned offsets
are inclusive LBAs and are not authenticated against a trusted device identity.
The parser follows the [UEFI GPT disk layout](https://uefi.org/specs/UEFI/2.10/05_GUID_Partition_Table_Format.html),
including its primary header at LBA 1, backup header at the final LBA, and
CRC-protected headers and entry arrays.

Twelve integration tests cover matching copies, malformed backup locations,
CRCs and geometry, mismatched contents, plus primary-table extent, duplicate,
overlap, and capacity failures; a unit test checks the CRC32 standard vector.
The parser does not parse the protective MBR. It consumes synthetic buffers
only: no physical partition table was read, no backing data identity was
established, and no partition was attached to the modem service.

### Sunfish UFS resource and lifecycle contract (2026-10-20)

[`qcom_ufs_platform.rs`](../rust/src/qcom_ufs_platform.rs) now validates the
audited host (`0x1d84000/0x3000`), QMP PHY (`0x1d87000/0x1000`, covering the
saved runtime `0xddc` window), ICE (`0x1d90000/0x8000`), level-high SPI265,
SMMU stream ID `0x300`, `UFS_PHY_GDSC`, `GCC_UFS_PHY_BCR`, GPIO119 active-low,
the four named supplies, eight clocks, and two interconnect paths. It bounds
descriptor DMA ranges to a page-aligned translated aperture. The fixture's DMA
address is synthetic and is not evidence of an actual Sunfish mapping.

The offline lifecycle gate requires a fresh-boot exclusive claim and checks
that lease before and after each step. It orders DMA/IRQ reservation, platform
power/clock/interconnect preparation, PHY initialization and reset/calibration,
the two 1.0–1.1 ms delays, SerDes/PCS-ready, lane clocks, and ICE initialization.
Any failed step or lost lease quarantines the owner and retains the claim; it
does not guess that rollback is safe. The ordered stages follow the pinned
Sunfish Qualcomm host and QMP PHY source. Sixteen host tests cover map
rejections, DMA bounds, ordering, every setup-stage failure, and lease loss.

This contract is still an interface/model. There is no native `PlatformIo`
adapter, no actual fresh-boot ownership proof, no UFS mappings in
[`modem-owner.system`](../platforms/sunfish/modem-owner.system), and no live
power, clock, IRQ, DMA/SMMU, PHY, ICE, SCSI, LUN or partition access. The
existing HCI readiness method is not connected to this model, so service
readiness remains disabled. All checks were offline; the Pixel stayed in
Lineage and was not accessed.

### Capability-gated UFS native adapters and candidate Microkit map (2026-10-20)

[`modem-owner-ufs-profile.system`](../platforms/sunfish/modem-owner-ufs-profile.system)
is a separate candidate system description. It adds the audited HCI
`0x1d84000/0x3000`, PHY `0x1d87000/0x1000`, and ICE `0x1d90000/0x8000`
device windows, plus level-triggered SPI265 as Microkit IRQ297. Its offline
builder produces and validates CapDL. The active
[`modem-owner.system`](../platforms/sunfish/modem-owner.system) remains without
these mappings. The candidate profile deliberately has no DMA aperture or
setvar symbols, so it does not supply an unverified memory address to code.

[`qcom_ufs_native_io.rs`](../rust/src/qcom_ufs_native_io.rs) now provides
volatile HCI-register and bounded DMA-copy adapters for the existing UFS host
core. Register reads and writes use narrow allowlists; each operation holds a
gate guard that pins the exact mapping and exclusive generation. DMA access
requires the gate to attest the same fresh-boot lease, an uncached and exclusive
translated aperture, exact IOVA/virtual bounds, and a maximum 16 MiB region.
The constructors also require an exact unmasked SMMU route for SID `0x300`, a
bounded route slot, translated route type, and enabled context bank. The saved
read-only ABL snapshot instead shows SID `0x300` covered by the broader
`0x300/0xf` route to context bank 2, and reports no translation-enabled context
banks. That snapshot cannot authorize DMA; the candidate adapter requires a
new live isolated translation under an explicit owner. The constructors do not
discover mappings or invent an IOVA. Six offline tests cover readiness
rejection, register allowlists, generation loss, route/SID/context mismatches,
and DMA range/translation failures; the adapters also compile into the AArch64
native codegen harness.

[`sunfish_smmu.rs`](../rust/src/sunfish_smmu.rs) now also has a pure UFS route
planner. It proposes only the singleton `0x300/0` match and requires separate
SMMU-control delegation, ownership transfer, and explicitly verified free
route/context-bank resources. It rejects the saved `0x300/0xf` route as an
overlap; it cannot delete or replace that route. Two offline tests cover the
exact-route plan, ownership gates, and that observed collision. A real route
change still needs a trusted owner to retire the inherited match and prove a
fresh snapshot before using this planner.

[`qcom_ufs_irq.rs`](../rust/src/qcom_ufs_irq.rs) adds a one-snapshot interrupt
dispatcher for Microkit channel 4. It clears the HCI status before acknowledging
the kernel IRQ and leaves the IRQ masked after a failed MMIO snapshot. The
freestanding build includes the exact Microkit `Channel::irq_ack()` adapter.
Four offline tests cover route rejection, controller-before-kernel
acknowledgement, fatal-source masking, and failure handling. The dispatcher is
not connected to an operational UFS host because no production platform gate,
DMA/SMMU mapping, or host bring-up is available.

This is still an unbound adapter layer: there is no production `Gate`, no
`PlatformIo` implementation to acquire power, clock, reset, PHY, ICE, and SMMU
ownership, no actual DMA memory map, and no IRQ-to-operational-host wiring. The
candidate Microkit profile is compiled for review only and is not used by the
owner build.
No UFS transfer, partition access, device operation, or boot was performed.
Native storage readiness remains disabled.

### Targeted SMMU readback and fresh-table planning gate (2026-10-21)

The supplemental read-only ABL probe covers WLAN SIDs `0x240`, `0x241`,
`0x520`, `0x521`, and `0x522`, plus UFS SID `0x300`. The user-provided screen
shows no matching route for the five WLAN SIDs and one translated-type route
for UFS to CB2, whose `SCTLR.M` is off. This targeted view does not enumerate
all SMR entries and cannot establish that any SMR slot or context bank is free.
It remains separate from the complete but older 2026-10-09 table snapshot.

The same screen reports `0x002A0406` at global SMMU register offset `0x000`;
the user separately confirmed that exact value in text.
Its common SMMUv2 fields show client accesses processed, global fault reporting
and interrupts enabled, unidentified-stream faults enabled, and stream-match
conflicts configured to fault. The old screen label `SCR0` did not establish
which security bank was read, so the diagnostic now labels the value `CFG0`.
The exact value and bank caveat are recorded in
[`wlan-resources.json`](../platforms/sunfish/wlan-resources.json).

Both offline route planners now require an explicit caller attestation that a
route snapshot is fresh and complete before considering a candidate. A targeted
SID readback cannot satisfy that gate. The planner cannot verify the attestation
or transfer hardware ownership; a trusted live owner must provide those proofs
before any route change. The SMMU handoff suite now passes 18 Python tests and
12 Rust tests; the manifest validator reports the targeted evidence without
inferring a free resource.

### Offline UFS modem-partition adapter (2026-10-21)

[`qcom_ufs_modem_storage.rs`](../rust/src/qcom_ufs_modem_storage.rs) connects
the verified paired-GPT partition type to the modem remote-storage I/O trait.
It maps `modemst1`, `modemst2`, `fsg`, and `fsc` to the protocol's four fixed
partition names, checks the trusted disk GUID and exact byte extents, bounds
each READ(10)-sized operation, and requires fresh live media observations
before and after I/O. The observation gate requires an exact unmasked
translated SID `0x300` route whose target equals the enabled bounded context
bank, exclusive translated DMA, the expected device and checkpoint identities,
and sufficient LUN capacity.
Ambiguous write or cache-sync completion is returned as uncertain so the
existing owner quarantines the provider rather than retrying.

Seven synthetic integration tests cover name mapping, extent bounds, read-only
and flush policy, identity/capacity mismatches, route authority, and uncertain
write quarantine. The module compiles in the AArch64 `no_std` harness. It is
composition code only: no physical GPT was read, no production UFS
`PlatformIo`/live `BlockIo` wiring exists, and the SID `0x300`
route/context-bank authority is still absent. No storage I/O or device
operation was performed; native storage readiness remains disabled.

[`qcom_ufs_scsi_block_io.rs`](../rust/src/qcom_ufs_scsi_block_io.rs) now
bridges the existing operational UFSHCI `Host` to that block contract using
bounded READ(10), WRITE(10), and SYNCHRONIZE CACHE commands. It stages writes
only after range preflight, maps any submitted-command failure to an uncertain
completion so it cannot be replayed, and requires an `ObservationSource` that
will later be supplied by the live owner. Three host integration tests verify
data movement, command ordering, range rejection, invalid host/DMA layouts,
and no retry after a missing completion IRQ. The bridge uses the synthetic
register/DMA fixture today; it
does not supply production `RegisterIo`, `DmaIo`, or media authority.

## Validation

Run the policy model with:

```sh
python3 -m unittest tests.sunfish_modem_lifecycle_model_test -v
```

The tests cover missing image/memory/secure-operation evidence, each missing
AP-side startup service (IPA host provider, SERVREG mapper, remote storage,
heap-memory sharing, and TFTP provider), illegal ordering, failed secure
authentication, stale peer generations, missing MSA/SMMU proof, orderly stop,
and quarantine/recovery behavior. They are synthetic tests and make no device
calls.

## Source references

- [Matched LineageOS PAS/subsystem loader (`subsys-pil-tz.c`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/subsys-pil-tz.c)
- [Matched LineageOS modem SCM caller (`scm.c`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/scm.c)
- [Matched LineageOS SCM descriptor and argument metadata (`scm.h`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/include/soc/qcom/scm.h)
