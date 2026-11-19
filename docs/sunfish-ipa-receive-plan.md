# Native IPA exception receive after table initialization

This is the next offline implementation after the table-DMA device pass.
The Pixel is back in LineageOS; the fresh physical ADB boot ID is
`5d90d1ca-57ac-4fef-88a6-b0f7cc1a5e94`. The table image's source snapshots
and artifacts are preserved in its consumed device run. Its manifest now
records the hardware result and cleanup, not an unattempted image.

The reference is `miromraz/linux` commit
`2267e248204e7e051e0f7355298949a6440de579`. Its `ipa_setup()` completes memory
and table initialization before enabling AP_LAN_RX and setting the default
route. `ipa_endpoint_enable_one()` starts its GSI channel, enables endpoint
suspend handling, and replenishes buffers in that order.

## Full initial receive allocation

The v4.2 data selects endpoint 8, channel 2, event 1, a 256-entry TRE/event
ring, TLV count 6 and 8-KiB receive buffers. Linux's
`gsi_channel_tre_max()` allows `256 - (6 - 1) = 251` outstanding single-TRE
transactions. Each allocation is two 4-KiB pages. The exact reference ABI
uses `NET_SKB_PAD = 64`; `ipa_endpoint_replenish_one()` maps the remaining
8,128 bytes for device writes. Every RX TRE is an XFER with IEOT and no BEI.

The existing 128-KiB pool contains command inputs and four ring pairs. Its
remaining space cannot hold the full receive allocation. The offline plan
uses a separate, host-chosen candidate reservation at IOVA `80200000`, size
`200000`. All numbers here are hexadecimal unless decimal is explicit. The
251 buffers occupy `1f6000` bytes, or 502 pages; `a000` bytes remain outside
the data spans. This IOVA is a proposed host mapping, not a firmware magic
address, installed mapping, allocation receipt or DMA authority.

The initial replenish emits a doorbell after each batch of 16, then flushes
the final eleven when the TRE resource pool becomes empty. There are sixteen
doorbells; the last value is `80009fb0` at GSI offset `1e010`. The receive TRE
ring is the existing owned ring at `80009000`, event ring at `80008000`.
Five TRE slots remain unused. No initial receive descriptor reuses the
command channel's TRE/payload/event slots.

## Interrupt and default-route transitions

Channel START adds event-ring-1 IEOB to the existing command event-ring-0
enable, so the IEOB bitmap becomes `3`. Eight original Linux writes include
the channel-2 control completion handling. A native coordinator must require
the actual delivered IRQ and state transition; a planned list is insufficient.

For an empty endpoint-suspend enable preimage, endpoint 8 sets IPA offset
`3034` to `100`. Enabling TX_SUSPEND adds bit 14 to the retained UC bits 2/3,
making IPA offset `300c` equal `400c`. The existing strict two-UC-bit guard
must be extended through a real receive owner and dispatcher before this
transition is allowed. A generic relaxation of the IRQ guard is not enough.

The final default-route word is `01100050` at IPA offset `48`: endpoint 8
handles normal and fragmented default traffic, retaining the header and
using the local header table. It follows initial replenish, before IPA QMI.

## Offline comparison and remaining integration

`sunfish_ipa_linux_receive.rs` provides immutable planning only and is tested
as a standalone module. The planner, receive CPU backend and receive-domain
geometry are now library modules. The receive-memory native entry selects
them for mapping and CPU preparation only, as described below. The consumed
table image retains its original frozen sources.

`sunfish_ipa_linux_receive_pool::DataPool` owns the separate CPU memory
backend without exposing it. One preparation attempt fills all 2 MiB and
checks it twice: data spans are zero, every 64-byte headroom has a fixed
canary, and the final ten pages have another canary. This reports CPU
preparation only. A read-only guard check covers all 14,256 headroom/tail
words while excluding device-owned packet bodies. Failed or uncertain
stores, reads, domain guards or canary comparisons consume and quarantine
the object; no prepare retry or raw backend accessor is supplied. Five
behavioral groups cover capacity, complete preparation, phase boundaries,
CPU failures, unchanged-body guard observation and containment. Native
physical backing and SMMU ownership remain to be integrated.

`test-native-ipa-linux-receive.py` verifies the pinned archive and compiles
the original full START/ISR path in one host model and original receive
replenish, page-add, TRE/transaction commit, ring doorbell, suspend-enable
and default-route functions in a second model. Kernel allocation, DMA-map,
IRQ waits, work scheduling and transaction bookkeeping are host stand-ins.
The original endpoint data supplies the ring/TLV/buffer dimensions. Both C
models have no device access. The Rust comparison checks all 251 TREs, all
sixteen doorbells and all eleven control/suspend/route writes. Four mutations
change headroom, batching, RX BEI and START channel; all must fail the C
comparison after compiling successfully. The same core builds for AArch64
without std.

The native receive entry below now composes RX start, retained pool/controller
ownership and IPA/GSI dispatch. Physical START/receive evidence, IPA status
parsing, packet delivery and the later modem/WLAN path remain required.
Neither the planner nor its host comparison provides production readiness.

## Retained startup controller

`sunfish_ipa_linux_receive_start::ReceiveOnce` takes ownership of both the
original ring/command pool and separate receive pool. It never extracts or
reuses either pool. It requires the actual completed GSI, endpoint, command,
memory and table owners, together with the pool's table-completion state and
CPU-prepared receive data. An invalid attempt is consumed while retaining
both pools.

The controller checks the two blank receive rings, stages all 251 TREs and
verifies their 1,004 words immediately and in a second pass. CPU staging
precedes START while the receive channel is allocated and has no published
receive descriptors. The original Linux control-write sequence is preserved.
The physical-counter deadline starts immediately before channel-2 START,
so slow CPU preparation does not spend the command's timeout.

Only a delivered exclusive GSI notification may enter the START completion
method. It requires type 1/source 4, validates the started channel and other
retained states, clears the source, restores masks and ACKs that notification
before any receive doorbell. It then enables suspend handling, posts all
sixteen batches, sets the default route, checks data-pool canaries again and
takes two final register samples.

A receive IEOB can legitimately arrive during publication. Initialization
records it and leaves its source and notification for the receive dispatcher;
it never clears or ACKs receive data as part of the earlier START completion.
The hardware producer is not assumed to have a particular value while the
ring is empty or unstarted. Producer samples with pending RX are diagnostic;
the event-drain owner still must order, bounds-check and validate them before
packet reads. The final `Armed` phase proves no packet delivery or readiness.

Every I/O guard receives both retained pools and a core-constructed IRQ-state
expectation. Only completed writes advance that expectation. An uncertain
operation or fault attempts both GSI and IPA masking, quarantines both pools
and retains published DMA targets without reset/free/rollback/retry.

`test-native-ipa-linux-receive-start.py` composes the earlier actual core
owners through real simulated memory and table completion; it does not forge
their reports. It compares all 27 MMIO writes to the two compiled Linux
models. The host backend observes CPU ring stores through the original pool's
RAM and verifies that other ring/command/table memory is unchanged. Failure
tests cover every core read, guard, uncertain register store and descriptor
store, plus bad initial state, foreign sources, ACK failure, timeout wrap,
slow preparation, legitimate RX arrival and later containment.

This controller remains outside the frozen native image. The original-pool writer now uses the exclusive retained backend described
below. Actual 502-page SMMU backing, native MMIO/IRQ bindings, receive
event/parser/dispatch integration and lifecycle handling remain required
before a receive-enabled hardware check.

## Bounded event queue and buffer recycling

`sunfish_ipa_linux_receive_events::Queue` implements the software event and
buffer lifecycle after the full initial replenish. It records which of the
256 TRE slots owns each of the 251 buffers. A completion must identify the
next owned TRE on channel 2, with the full 64-bit pointer equal to that slot;
misalignment, high pointer bits, foreign channels, skipped/duplicate TREs
and lengths above 8,128 are rejected. Zero length is allowed. Code, type and
reserved bytes are retained exactly, without guessed success/error meanings.
The pinned Linux RX path uses channel, TRE and length, not those raw fields.

The producer must be aligned within the owned event ring. Its modular delta
cannot exceed outstanding buffers or completion-queue capacity. Producer
observation is followed by a DMA acquire operation. Each event is copied
twice, with ordering between copies, before it is accepted. All events in a
snapshot are validated before a full DMA ordering barrier and the event
doorbell. The doorbell returns the last processed slot, including slot 255
when the next producer wraps to zero. Later arrivals belong to the next
snapshot. No device packet body is read by this queue.

Completed buffers remain held until the oldest completion's consumer returns
success. Only then does the queue stage four TRE words at the publication
tail, verify them twice, order prior CPU reads/stores before the device and
publish the new channel tail. The buffer is never reposted into its old TRE
merely because that slot completed. The five-slot initial gap rolls through
the ring. This implementation flushes each recycled buffer immediately;
it uses the original immediate-flush transaction path rather than duplicating
Linux's delayed replenish work and allocation lifetime. Batching is not a
correctness precondition here.

Any invalid event, torn copy, failed consumer, memory/order operation or
uncertain doorbell quarantines the queue. Its prior metadata and buffers
remain held; no retry, rollback or extraction API exists. Native integration
must also mask physical sources and quarantine both actual retained pools.

`test-native-ipa-linux-receive-events.py` verifies the pinned source archive
and compiles the original event layout, transaction lookup, event update,
producer polling, TRE commit/encoding and both ring doorbells. Transaction
maps, completion lists, allocator, DMA memory and MMIO are host stand-ins.
The compiled reference processes 2,336 events through repeated event/TRE
wrap, then reuses the same buffers with immediate-flush commits. Rust matches
every completion length/channel/TRE, event credit, reposted TRE and channel
doorbell. Nine behavioral groups additionally cover a fully held queue,
raw-field preservation, all I/O failure boundaries, uncertain publication,
torn events, arrivals during a producer snapshot, omitted stores, consumer
rejection and over 40,000 recycled
transactions. Six compiled mutations of credit position, length bounds,
pointer truncation, TRE reuse, guessed code meaning and omitted stores fail.
The core builds for AArch64 without std.

This queue deliberately grants no packet-read, IRQ or mapping authority.
It is now a library module, owned by the retained runtime described below;
the native receive feature now selects it through the same retained owner.
Its physical behavior, IPA status parsing and installation remain unproven.
These host results prove neither physical receive nor bidirectional IPA
packet traffic or Wi-Fi.


## Sole ring writer and receive CPU backend

The receive controller no longer asks an I/O adapter to store TREs through
another RAM handle. After its actual GSI/endpoint/command/memory/table
prerequisites pass, it claims receive use on the original pool once. Its
1,004 initial descriptor stores go through that pool's sole retained
`Memory` backend. The pool allows aligned stores only inside channel 2's
4-KiB TRE ring; command payloads, other channels, sources and every event
ring remain outside the write boundary. A second claim fails. Any uncertain
backend store immediately quarantines the pool, then the controller also
attempts source masking and quarantines the data pool.

The receive-start suite now injects descriptor failures and suppressed stores
into the actual retained `Memory` backend, rather than a separate test writer.
Its ninth group checks every aligned word of the original 128-KiB pool:
exactly 1,024 RX TRE words may be written after a claim, no others. It also
checks claim prerequisites, unaligned/overflow offsets and quarantine after
an uncertain store. These are CPU ownership tests, not permission to write a
still-device-owned slot; that decision remains with the receive coordinator.

`sunfish_ipa_linux_receive_native::NativeReceiveRam` supplies the non-cloneable
uncached CPU backend for the separate 2-MiB reservation. It retains and
validates 512 contiguous 4-KiB frame addresses and the exclusive CPU span.
The first 502 pages are candidate DMA targets. Its DMA-page diagnostic does
not expose the ten tail pages, which remain CPU canary storage and must be
absent from the eventual device mapping. Scalar volatile 32-bit loads/stores
use the same full AArch64 barriers as the original native pool backend.
There is no raw pointer accessor or arbitrary bulk copy.

Three host groups check invalid physical/virtual geometry, every scalar word
and all boundary cases in the full 2-MiB CPU span, and real `DataPool`
preparation/readback/canaries through this backend. AArch64 no-std compilation
also passes. Host physical addresses are geometry fixtures only. The unsafe
constructor explicitly requires actual retained CapDL ownership, absence of
aliases and verified domain mappings before any RX publication; construction
and host tests establish none of those rights. The planner, backend and
domain geometry are library modules; the receive-memory feature composes
them with the actual native entry without starting the receive channel.

## Full receive SMMU table geometry

The existing shared/command installation uses two occupied 1-GiB root entries,
three occupied 2-MiB leaf regions and seven table frames. The receive window
at `80200000` occupies a fourth 2-MiB region. It therefore needs an eighth
frame; fitting it into the existing seven-table image would be incorrect.

`sunfish_smmu_lpae` now has a bounded four-leaf root-plan API alongside the
existing three-leaf API. The latter retains its 128-page bound. The new API
admits at most 539 pages, requires exactly two root and four leaf regions,
and validates all eight distinct frame identities. Both use the same checked
context/descriptor encoding and reject the wrong table-image size before
touching caller storage. The disabled TTBR1 root remains empty.

`sunfish_ipa_linux_receive_domain::Mapping` composes the exact 37-page
IMEM/command/SMEM list with the full receive allocation. It validates shared
IOVA geometry, IMEM/SMEM identity mappings, the contiguous 32-page private
command pool, and all 512 contiguous receive frame addresses. The output
inserts only the first 502 receive pages, giving 539 sorted mappings. The
last ten pages remain CPU-only, and all 512 pages are excluded from table
frame storage, including those not present in the device mappings.

The eight-table image contains 545 nonzero descriptors: two root links,
four leaf links and 539 page descriptors. The reference configuration stays
39-bit input, 40-bit output, ASID 4, noncoherent walks, normal noncacheable
page attributes and no ARM NS quirk. A fresh eligible bank and the actual
owned frame bindings still have to be verified at installation time.

Five receive-domain groups independently walk physical descriptor links for
every mapped page, check all ten tail holes and surrounding gaps, and compare
every previous table word and context register against the seven-table plan.
Only the new L2 link and its new leaf image differ. Invalid backing, aliases
(including CPU-only tail/table aliases), late map entries, ordering, range,
table identity and image geometry are rejected. The runner also compiles and
runs the existing root, LPAE and native-adapter suites, then cross-compiles
the new plan for AArch64 without std.

This remains uninstalled on the device. The native adapter has the retained
receive extension described below, selected by the receive-memory native
entry. The receive feature adds RX/IRQ composition. Neither XML nor numerical
addresses may substitute for retained resource owners and a generated
capability audit.

The `native-startup-ipa-receive.system` profile adds exactly two private
resources to the prior table-test profile: one 4-KiB `smmu_l3_receive` frame
at CPU virtual `7100f000`, and a 2-MiB `ipa_receive_data` reservation at
`76001000`. Both are uncached RW/NX, have separate physical bindings and
unmapped adjacent guard pages. The layout check removes only these entries
and requires the remaining region/map/IRQ/SMC shape to equal the table
profile exactly. This source-layout check is separate from the generated
CapDL audit. The build script selects the profile in `receive-memory` mode;
this does not yet select a receive-channel runtime.


## Retained eight-frame native installer

The native SMMU adapter consumes the actual non-cloneable `NativeReceiveRam`
before receive staging. It retains the complete CPU span and all 512 physical
identities alongside the live shared-memory owner and original command-pool
identities. Attach rejects aliases with known tables, private/shared pages,
registers and shared CPU views. Extra table validation also excludes the full
receive CPU span and all 512 frames, including the ten CPU-only tail pages.
The unsafe backend/table constructors still require real CapDL ownership;
these checks cannot establish that ownership from numerical inputs.

The shared root staging function admits only the existing 3-extra/7-table
layout or the new 4-extra/8-table receive layout. The latter requires the
retained receive RAM. It regenerates the canonical 539 mappings from its
actual retained owners, compares the complete table image and context policy,
then retains all eight frame pointers/identities before any volatile table
store. Every staged word is read back. No caller-supplied map, cached/NS
attribute variation, tail mapping or table alias may replace that plan.

`PublishReceiveRootPageTables` is a separate install operation. The executor
validates all eight distinct aligned identities and invokes the native
publication callback at the original point in the Linux register sequence.
The seven-table operation and constructor bounds remain intact. Selection
requires the captured exact 520 and 522 routes, bank 4, the staged context and
the matching eight-frame publication record. The original fault binding,
TLB synchronization, route-preimage and ASID checks remain required.

Only successful completion of the actual install method enables one receive
RAM handoff. Before returning the CPU writer, the adapter checks faults,
ASID conflicts, unexpected routes, final planned register values and every
word of all eight tables against the canonical image. Superseded transient
register values (such as initial SCTLR disable) are not final-state checks.
The ten unmapped leaf entries and disabled TTBR1 root are checked too.
Any attempted handoff is consumed, including a premature or failed one.
The adapter retains the metadata after handoff so later domain observations
can revalidate the exact mappings without another CPU alias or writer.

Six host groups exercise the actual native install method with aligned RAM
standing in for MMIO, shared memory and the receive allocation. They cover
complete install/handoff, preservation of UFS/display bank views, canonical
map/attribute substitution, extra table aliases including unmapped tail
frames, live corruption of tables/routes/context/fault state, publication
failure, missing RAM/peer and premature handoff. These are simulated register
and CPU-memory observations. The receive-memory image adds the actual
Microkit entry and allocation audit. Physical installation remains unproven;
no packet or Wi-Fi readiness follows from a RAM simulation or library build.

## Native receive-memory prerequisite image

`build-native-startup-preflight.sh receive-memory` selects the actual
eight-table profile and `sunfish-native-startup-ipa-receive-memory` feature.
The native entry constructs the sole receive CPU backend from Microkit's
physical/virtual symbol bindings, attaches it to the retained domain owner,
and installs the canonical eight-table map. The existing startup, memory DMA
and table DMA run before the receive RAM handoff and CPU preparation.

`pixel_ipa_receive_memory` retains the transferred pool before its first CPU
store. Its guard checks the actual firmware/domain, completed GSI, endpoints,
command and both DMA owners, the quiet interrupt state and channel contexts.
Preparation initializes all 524,288 words, reads them twice and then verifies
14,256 headroom/tail guard words. Failures quarantine the receive pool along
with the inherited command/domain owners. These counts describe the planned
operation; they are not device results.

The actual AArch64 binary and Microkit image compile. The generated CapDL
audit passes for 2,786 selected frames, 32 unmapped guard pages, eight SMMU
table frames and the complete 512-page uncached RW/NX receive allocation.
It checks physical contiguity, fixed resource bindings, frame aliases,
rights, IRQ destinations and reserved-memory exclusions against both DTBs;
119 invalid configurations are rejected. The source audit binds preparation
to the retained owners and rejects eight receive-specific source mutations.

The private candidate image is 27,865,088 bytes, SHA-256
`8fd8f78605666ea9c2437d13fbe99ab45f5eef536ac08b51757b12a0ba8aeb7a`.
No boot or other device operation was issued for it. It has no RX channel
START, receive TRE publication, receive doorbell or packet access, and starts
no modem/WLAN. The separate receive feature described below adds the startup
and dispatcher; this memory-only candidate retains its original scope and
frozen sources.

## Retained receive runtime and bounded CPU reads

`sunfish_ipa_linux_receive_runtime::Runtime` consumes the actual `ReceiveOnce`
and retains both pools on success and failure. It constructs its event queue
only after the startup coordinator has completed START, source clear and
physical ACK, all 251 buffer publications, default route and final samples.
The native adapter must still establish the real physical bindings; this
composition and its reports are not production readiness capabilities.

Runtime access uses the same sole ring backend as startup. Reads are limited
to channel 2's event/TRE rings and stores to its free TRE slots. The queue
returns validated completion metadata to a scoped `PacketRead` borrow of the
same retained data pool. Callers may copy only bytes below that completion's
actual length. Scalar 32-bit reads can include unused bytes in the final
word, but those bytes are not exposed, and reads cannot enter headroom,
another buffer or the CPU-only tail. Packet access is read-only and cannot
escape its consumer borrow. Even an ignored read error prevents reposting.

For an exclusive GSI notification, the runtime checks all sources, disables
receive event 1 while leaving command event 0 enabled, and clears only the
captured receive source. It performs the acquired, stable event drain,
bounded consumption, ordered TRE repost and data guard verification before
re-enabling the receive event and ACKing the physical interrupt. There is
one bounded producer snapshot per delivery. Arrivals during repost remain
latched for the next delivery, rather than being cleared or lost. Failures
retain and quarantine both pools, mask through the startup owner and prevent
any repeat operation or ACK attempt.

Ten host groups compose the actual earlier GSI/endpoint/command/memory/table
owners and startup coordinator. They exercise exact payload bytes and
unaligned reads, both ring wraps over 1,025 completions, zero-length events,
arrival during repost, source/event corruption, consumer errors including
ignored bounds failures, every modeled guard/MMIO/barrier/ACK failure,
actual ring/data backend failures and all CPU ring read boundaries. The
nine startup groups and nine event groups also pass. Original Linux
`gsi_isr_ieob` and its enable/disable helpers produce the same three mask/
clear writes; MMIO, NAPI scheduling and memory are host stand-ins. Four
runtime mutations and the six existing queue mutations are rejected.
The modules are exported and compile for AArch64 without std and as the
actual seL4 library.

Native receive interrupt wiring remains incomplete. In particular, the
legacy UC dispatcher still expects IPA IRQ enable `c`, whereas receive
startup adds TX_SUSPEND and changes it to `400c`. The shared retained core
below now implements that fixed extension. The concrete native handler
must select it rather than relaxing or reusing the old UC-only guard. IPA
status parsing and delivery to a network consumer remain separate work;
bounded aggregate reads do not establish IP frames.

## Shared IPA/GSI interrupt owner

`sunfish_ipa_linux_receive_dispatch::Runtime` consumes the actual receive
runtime and previously armed UC runtime. It retains both on rejection,
quarantines the receive pools for an invalid handoff and provides no owner
extraction. The legacy public UC handler continues to require exactly `c`.
Only the shared dispatcher selects the crate-private fixed `400c` UC/suspend
extension, after validating the actual receive owner.

The shared IPA guard uses that receive owner's sealed interrupt state and
the same retained pools. A GSI fault/source check precedes IPA dispatch.
UC bits are cleared before reading their defined byte. For TX_SUSPEND,
the pinned v4.2 profile has 17 endpoints and one status unit: the dispatcher
reads `IRQ_SUSPEND_INFO` at `3030`, writes its captured nonzero value to
`IRQ_SUSPEND_CLR` at `3038`, then clears IPA source bit 14 at `3010`. Clearing
the IPA source first would destroy the endpoint information. Zero status
does not produce a suspend-clear write. Enabled sources drain in ascending
order, followed by one captured disabled-status clear and the physical ACK
boundary. The existing 64-round bound and fatal-UC containment remain.

For simultaneous legitimate IPA/GSI delivery, IPA dispatch runs first so a
UC fatal indication prevents packet reads or reposting. A co-delivered
foreign/SMMU fault takes priority over both. On an IPA failure, its handler
attempts IPA masking once and the receive owner then masks only GSI. On a
receive failure, that owner attempts both masks and the UC owner latches the
failure without repeating the IPA store. An uncertain mask or ACK is never
retried. Both pools remain retained; the native boundary must also retain
their mappings, firmware and power resources for the full owner lifetime.

Nine shared-dispatch groups pass with the actual prior owners, together
with ten receive-runtime and twelve legacy UC groups. They cover invalid
handoffs, simultaneous sources, source/byte/clear ordering, fresh indications,
zero suspend information, UC fatal indications, bounded drain, strict masks,
fault priority, every modeled I/O/guard/ACK failure and containment without
duplicate mask stores. Five runtime mutations are rejected. The original
`ipa_isr_thread`, `ipa_interrupt_process` and suspend handler compile against
the pinned register definitions and produce the same effects. MMIO, PM,
shared-byte callbacks and the separate Microkit ACK are host stand-ins.
The composed modules compile for AArch64 without std and the seL4 library.

The native binding below now selects this dispatcher, tracks its receive
interrupt state and uses the pools retained inside the owner after handoff.
The later IMG_9184 device result below establishes physical RX startup and
default-route setup. No IP packet or Wi-Fi readiness follows from these host
observations or that startup-only result.

## Receive-enabled native diagnostic

`build-native-startup-preflight.sh receive` selects
`sunfish-native-startup-ipa-receive`, extending the receive-memory feature
with the same XML frame/IRQ/SMC authority. `pixel_ipa_receive` transfers the
actual ring and receive pools into `ReceiveOnce` before any new store. After
physical START completion and initial publication, it transfers that owner
and the actual UC runtime into the shared dispatcher. A pending UC delivery
uses the retained ring pool before this handoff. The native handler routes
all receive-owned notifications ahead of the legacy handlers and keeps
failed owners terminal, preventing duplicate mask/ACK attempts.

The guard checks the retained eight-table domain, actual PAS15 session,
power/SCM reports, prior GSI/endpoint/command/memory/table owners and both
pools. Two primitive snapshots preserve every configuration word while
tracking the sealed IRQ transition. Only endpoint 8's active aggregation
bit is permitted after its owned RX enable. Static GSI context words and
all channel/event states are checked; hardware scratch and producer fields
are not incorrectly treated as reset invariants. START completion and route
publication are tracked inside the core's private `GuardState` before the
native adapter accepts those transitions.

The compiled native policy accepts the exact 27 original Linux startup
writes plus fixed zero containment masks. Runtime doorbells stay aligned
within the owned event/TRE rings. Foreign badges take fault priority. Four
policy groups, 28 startup/receive/shared-dispatch regression groups and four
policy mutations pass. The source binding checks ordering and additionally
rejects six changes that remove the entry/owner, PAS or MMIO checks, or alter
the GSI ACK channel. The actual native binary and Microkit image compile.

The generated CapDL audit passes for 2,786 selected frames, 32 unmapped guard
pages, eight tables and the complete uncached RW/NX 512-page receive span,
with 502 DMA-mapped pages. Both DTBs are checked for reserved-memory overlap;
119 invalid configurations are rejected. This is allocation/capability
evidence, not evidence that the hardware used the new mappings.

The private candidate is 27,893,760 bytes, SHA-256
`a265eff613e692ddc260d9d0b56dab93e35da5159b345341c5f38f450d57ad09`.
It was temporarily booted exactly once after the user confirmed a fresh
power-off of at least ten seconds. IMG_9184 reports `RX ARMED DIAGNOSTIC`,
phase `3`, `3ec` TRE words, `fb` posted buffers, `10` initial doorbells,
route/START-ACK `1` and two final samples (hexadecimal). There was one physical
GSI notice and ACK on badge 8; both error fields and global FSR are zero.
Context FSR `400` has only the format field, with no defined fault flags.
The actual run preserves the frozen image, 403 source snapshots, policy and
reference proofs, private inputs, Fastboot identity/log and original photo:
`.cache/sunfish-native-wifi/private/device-runs/ipa-receive-20261027T003053Z/`.

The bounded consumer reads, counts and discards diagnostic aggregates; it
does not parse IPA status or deliver network frames. The image starts no
MPSS/WLAN and flashes nothing. Physical RX startup/default route passed, but
receive DMA, IPA QMI, modem startup and Wi-Fi remain unproven. Zero aggregates,
received bytes, UC init and suspend notices were observed; before MPSS these
are not failures and provide no packet evidence. After the manual restart,
physical ADB confirmed a fresh completed LineageOS boot
`193ee37d-2450-419c-8e0f-11ed9edf253b`, distinct from baseline
`5d90d1ca-57ac-4fef-88a6-b0f7cc1a5e94`. Temporary state is cleared and the
one-shot attempt remains consumed.
