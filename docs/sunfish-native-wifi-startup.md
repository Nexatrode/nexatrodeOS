# Native Sunfish Wi-Fi startup from the working Linux reference

The destination is a native seL4 host. Linux is the reference implementation
and comparison environment; it is not a base layer in the final system.

## Proven reference and current native work

The [powered-off Linux run](sunfish-mobian-comparison.md) on 2026-10-25 reached
modem running, WLFW discovery, ath10k, association, DHCP, and sustained traffic.
It used kernel `miromraz/linux` commit
`2267e248204e7e051e0f7355298949a6440de579` and Mobian integration
`789679f25f859bd741d690fe221d1cc68a23444e`. The four EFS partitions remained
unchanged; modem storage writes went to private RAM shadows.

The first native implementation changes reproduce that storage mode and add
a bounded SMMU adapter. They do **not** yet establish native modem/WLAN startup
or a working data path.

| Dependency | Implementation and evidence | Remaining device evidence |
| --- | --- | --- |
| EFS backing | `qcom_modem_ram_storage.rs` verifies all four complete source hashes before copying any shadow. The private provider and its cached RW/NX shadows are constructed and retained by the cold native pre-DMA image. The selected Linux RMTFS resource owner couples this real provider to its single reference shared buffer; the actual QRTR/FIFO owner now dispatches storage requests against it offline, with bounded copies, RAM-only sync and retained replies under backpressure. | Construct audited physical mappings/capabilities, retain the storage server across MPSS startup, and verify device requests/replies. |
| SMMU layout | Exact-kernel read-only module captured all 79 route slots and 61 context banks while Linux remained associated. A stable cold native census identified bank 4 while preserving the UFS/display banks. | Revalidate the full preimage in each later installation. |
| SMMU installation | Cold native tests passed the WLAN alias route and the two exact IPA routes `520/0`, `522/0` in bank 4. IMG_9138 passed real IMEM/SMEM installation with the live allocation owner and 32 private pages; IMG_9178 adds a real private-pool command/event DMA pass without defined fault flags. | Functional use of the IMEM/SMEM windows, production lifecycle/ownership integration and full IPA/WLAN resources. Experimental installation receipts grant no production readiness. |
| IPA startup | Cold native tests passed all eleven real Apps DRV2 power floors, nine primitive IPA configuration writes, the two-entry SMMU installation, SRAM canaries, 11 owned buffers, eight endpoint checks, seven resource-limit writes and UC IRQ arming/binding/readback. Later tests add PAS15, GSI allocation, AP endpoints, command START, memory DMA and six-command table initialization. | Exception RX/default route, IPA QMI and later packet DMA. UC initialization follows modem startup. No production IPA-ready token is minted. |
| IPA shared memory | IMG_9138 confirms the combined native pass: live Rust allocation/get, all four metadata stores completed, lock release sent, owner retained, real IMEM identity span `146a8000/2000` and SMEM item `8611d030/2000` mapped across three pages from `8611d000`. | Functional translation and the remaining pre-firmware/runtime dependencies. No production DMA/firmware-ready capability follows from installation/readback alone. |
| IPA UC interrupt dispatch | Opt-in native dispatcher transfers the armed IRQ, clears before byte reads, drains pending sources and ACKs delivery; behavioral tests, AArch64 build and actual CapDL audit pass offline. Native firmware integration and strict post-auth guards are present. | Physical interrupt delivery/init response remains unproven. Pinned source places UC firmware loading at the first modem boot; zero UC init before MPSS is expected and does not block GSI setup. |
| IPA firmware loader | IMG_9143 records a cold native PAS15 success: masks `3f/3f`, 7,020 metadata bytes, 16,832 staged/verified bytes, auth monitor/service zero, GSI status `1`, no defined SMMU faults. The exact run and cleanup to LineageOS are saved. | No further PAS15-only repeat is needed. Endpoint transfers, modem startup and WLAN remain required. |
| GSI allocation/context | IMG_9156 passes the cold native V2 test: four AP event/channel pairs allocated and programmed/read back, four fresh modem-EE allocations, 12 physical completion notices and ACKs, all 170 expected writes, no defined SMMU fault flags. IMG_9178 adds first command/event DMA. | Packet transfers and the remaining IPA/MPSS/WLAN path. Allocation and readback do not grant production DMA or WLAN readiness. |
| IPA AP endpoint configuration | IMG_9167 passes all 36 endpoint writes and defined-bit readbacks, with all four endpoints and two final samples complete. Seven behavioral groups, four rejected mutations and compiled-C comparisons pass. Exact reference-kernel headers/config independently confirm SKB/QMAP layout and the 6-KiB aggregation threshold. | Functional transfers; endpoint register readback alone grants no DMA readiness. |
| IPA command-channel START | IMG_9167 passes the empty channel-1 START: state `2`, nine writes, one physical GSI notice/ACK and no defined SMMU fault flags. Eight behavioral groups, nine compiled-C comparisons, three rejected encoder mutations and 96 capability mutations pass. | The later memory/table tests prove fixed command DMA; receive and packet DMA remain. The empty START test itself publishes no TRE or transfer doorbell. |
| IPA memory immediate commands | IMG_9178 passes the first real transaction: four commands, 48 payload/TRE stores checked, all 1,619 target words changed from nonzero to zero, the expected channel-1 completion event and physical notice/ACK, all 25 canaries intact and no defined SMMU faults. | Exception RX/default route and IPA QMI. IMG_9183 subsequently passes fixed table DMA. These prove command/event transactions, not packet traffic, readiness or Wi-Fi frames. |
| IPA route/filter table commands | IMG_9183 passes six commands after memory DMA: all 120 destination words, 50 canary checks, expected event and physical notice/ACK, no defined SMMU faults. | Exception RX/default route, IPA QMI and packet transfers. Native modem/WLAN startup remains unproven. |
| IPA exception receive | IMG_9184 passes the cold native diagnostic: RX phase 3, 251 buffers posted through 16 doorbells, default route set, one physical START notice/ACK, two final samples, both error fields zero and no defined SMMU faults. | Receive DMA, IPA status parsing and packet delivery remain unproven; zero aggregates arrived. Native modem/WLAN startup remains required. |
| IPA QMI | The Linux v4.2 codec, cold handshake and dual-completion transport now run inside the existing serialized FIFO/IRQ owner. Sixteen owner groups, 161 older FIFO/owner groups in both feature modes, seven rejected faults, concrete AArch64 volatile-backend instantiation and the full seL4 library pass offline, in addition to the original Linux byte/framing comparisons. | Hook the native startup-resource factory and deadline wake, retaining actual IPA/domain/firmware/pool and RAM-backed host-service owners across MPSS startup. Host-model protocol success is not native QMI, modem or Wi-Fi proof. |
| GLINK/QRTR | Existing native FIFO/channel, credit, routing, discovery and QMI/SERVREG adapters. Earlier device probes exchanged HELLOs, discovered services and completed SERVREG requests. | Retain and dispatch these components after the new cold native MPSS startup, with the required host providers; earlier peer/service access does not prove this composed boot. |
| Selected MPSS loader | `sunfish_mpss_linux_loader.rs` implements the pinned monolithic PAS4 load order. The complete private modem image matches the original Linux C loader across all 152 MiB of target RAM, including BSS and unchanged gaps; compact metadata and all three PAS descriptors match. Native scalar memory, fifteen host groups, seven rejected faults, concrete AArch64 and the full seL4 library pass offline. | Compose actual modem power, the new SMEM allocation layout, SMP2P ready/IPA signaling and native FIFO service dispatch. No new physical MPSS start has been attempted. |
| MPSS shared-memory preparation | `sunfish_mpss_linux_smem.rs` retains the actual successful IPA allocation owner and its hardware lock view, allocates fresh 428/478/479, and zeros/verifies 16,756 new payload bytes in bounded chunks. The volatile adapter and retained shared/SMMU-owner API pass fifteen new groups and predecessor regressions. The concrete `mpss-smem` boot entry now builds; eleven actual-entry host scenarios, ten source mutations and 174 capability/layout mutations pass, alongside the original Linux C comparisons and AArch64 builds. | Activate two-entry SMP2P and GLINK, compose native modem power/PAS4 and retained host-service dispatch, then test the composed cold startup. The new image is unbooted; no physical extension or new modem start has been attempted. |
| Modem host services | Existing downstream RMTFS/heap/SERVREG/TFTP dispatch and prepared-service composition. The successful selected Linux service inventories contain RMTFS, TFTP, IPA and later SERVREG, with no heap-memory service; its separate RMTFS buffer is at `fde01000/200000`. The private provider verifies the exact reference WLAN image and can supply the real TFTP image capability; a complete 3,751,004-byte host transfer passed. | Use the selected reference resources and startup ordering rather than imposing the old downstream heap/pool profile. Observe actual requests and successful replies in the composed native boot. |
| WLAN startup | Existing bounded protocol, CE, WMI and station components. | Match the selected ath10k reference profile, then WLFW/MSA, firmware-ready, CE/HTC/WMI, scan, association and sustained traffic. Keep downstream QCACLD assumptions separate. |

The immediate sequence after the table-DMA pass is exception RX backing,
channel/completion handling and the default route, then IPA QMI and native
MPSS startup with its host services already available. Existing GLINK/QRTR
transport and discovery are reused and revalidated in that composed cold
boot; they are not new work from zero. Their earlier SERVREG results do not
prove MPSS can be booted by this new startup path.

## Selected MPSS loader and remaining native composition

`sunfish_mpss_linux_loader.rs` follows the pinned kernel's
`qcom_mdt_pas_load()` and the non-TZMem branch of
`qcom_scm_pas_prepare_and_auth_reset()`. Both the built DTB and the saved live
reference DTB lack an `iommus` property on the MPSS node. The original driver
sets its PAS context's `use_tzmem` from that property, so this selected path
uses direct PAS4 authentication rather than creating a host SHM bridge.
Resident privileged firmware remains involved, as in the working reference.

The loader verifies the complete exact private image before exposing its
31 program headers and 29 loadable segments. It compacts the ELF/header and
hash segment into retained contiguous uncached metadata, verifies that copy,
submits INIT_IMAGE, submits MEM_SETUP for `8b000000/09800000`, stages only
segment file bytes and zero tails, verifies them, then submits AUTH_AND_RESET.
Three read-only call-availability checks precede INIT. Each mutating call
acquires and releases the actual SCM resource provider's lease. Both monitor
and secure-service status are checked and recorded. The native platform
provider for that lease is still to be composed with the new modem owner.

No metadata read follows INIT submission, and no target access follows AUTH
submission, including uncertain/error returns. This deliberately avoids the
older downstream probe's post-INIT erased-metadata fingerprint. All memory
owners are retained, each chunk is at most 4 KiB, and any failure, lifecycle
loss or unwinding action consumes the attempt. The authentication receipt is
one-use generation evidence only; it does not assert SMP2P ready, GLINK
ownership, modem running or WLAN readiness. Linux's automatic shutdown and
cleanup paths are not copied into this one-shot experiment.

`sunfish_mpss_linux_memory.rs` checks the complete scalar range and word
alignment before volatile accesses and orders them with hardware barriers.
Its unsafe constructor requires actual exclusive mappings and boot-chain
staging authority; validating numbers does not supply either. Host canaries
and rejected scalar/geometry cases exercise the real adapter. The temporary
native system has not yet gained these mappings or a PAS4 call path.

Run `python3 platforms/sunfish/test-native-mpss-linux-loader.py` for the private
CPU comparison. It compiles original pinned Linux MDT and SCM function bodies
against recording stubs, compares every target byte and PAS descriptor with
the Rust implementation, tests both status layers and resource balancing,
rejects seven behavioral mutations, builds the concrete AArch64 scalar
loader and the full seL4 library. Private evidence is retained in
`.cache/sunfish-native-wifi/private/mpss-linux-loader/`. There is no device
action in that runner.

Before the next native modem boot, the older fixed SMEM addresses must be
replaced. The current IPA owner already allocates item 497 at
`8611d030/2000`, advancing the uncached cursor from `20` to `2030`. A new
composition must allocate SMP2P item 428 and GLINK items 478/479 after that
existing item, under the real shared lock, and retain the live resulting
identities. Copying the old probe's first-page allocation would overlap IPA.
Writable pages and byte-level store limits must match the new layout while
preserving all of item 497. The native transport must use the current SMEM
virtual mapping and separate logical IRQ channels from the existing SMMU,
IPA and GSI handlers.

The selected Linux SMP2P node also registers **two** outbound entries,
`master-kernel` and `ipa`, and two inbound entries, `slave-kernel` and `ipa`.
The older master-only handshake is insufficient for the composed profile.
The IPA clock-query response must reflect the actual retained power owner:
Linux updates the enabled bit first and the valid bit second, once per
lifecycle. Modem ready, fatal and handover remain distinct from those IPA
bits. Retaining power floors does not authorize writing the fatal/stop lines
or claiming that a ready transition occurred. The next integration must also
serve RAM-backed RMTFS/TFTP and IPA QMI as soon as the authenticated transport
becomes available, without requiring an unrelated old readiness token.

`python3 platforms/sunfish/audit-native-mpss-smem-layout.py` compiles the
original Linux private allocator and IPA power notification function. All six
allocation orders preserve the existing IPA payload. For the selected native
order (497, then 428, 478 and 479), the new payloads are `8611f040/154`,
`8611f1a8/20` and `8611f1d8/4000` (hex), with the free cursor at `61d8`.
The new payload mapping spans five pages from `8611f000`, in addition to the
existing partition-header page used to publish the cursor. Item 497's tail
shares that first new page, so page permissions alone cannot protect it from
an incorrect CPU store; the native adapter needs exact byte/word limits.
The original IPA notifier confirms the on-power response values `2` then `3`
and off-power values `0` then `1`, with repeat queries suppressed within the
lifecycle. These are source-derived values, not permission to publish them
without the real power/transport owners. The private plan and source evidence
are in `.cache/sunfish-native-wifi/private/mpss-smem-layout/`.

### Retained native SMEM allocation extension

`sunfish_mpss_linux_smem::Extension` owns the actual completed
`sunfish_ipa_smem::Owner` and the native payload writer. It cannot accept a
copied IPA address in place of that predecessor. The predecessor lends its
same metadata and hardware-lock view only after its successful publication,
lookup and release. Each extension step acquires lock 3, checks the table and
all five relevant item lookups twice, performs one allocation or at most
4 KiB of payload work, checks ownership again, and releases only an AP-owned
lock. Item 497's payload is neither read nor written. Existing modem items,
unexpected free/cached cursors, changed headers, foreign partition overlap,
uncertain writes, lost locks or lifecycle failures stop the attempt.

The selected order adds items 428, 478 and 479 after item 497. It preserves
unused header words and the four bytes of SMP2P alignment padding, orders
each completed header before its free-cursor publication, verifies lookup,
and then zeros/verifies the three new payloads. Clearing the fresh full
descriptor and TX FIFO is an intentional pre-auth experiment difference:
Linux's transport registration resets the two AP-owned descriptor cursors.
Once a peer is active, clearing its cursors or an existing allocation would
violate ownership; this extension rejects that starting state and has no
runtime clearing/retry API.

`sunfish_mpss_linux_smem_native.rs` admits only the exact twelve metadata
stores and ordered zero stores inside those three new payloads. It cannot
write IPA bytes, item headers' reserved fields, padding or the peer RX FIFO.
The five writable pages must be delegated through the real native SMEM
Window constructor. The existing predecessor-only Window cannot attach the
extension because it lacks that delegated view. `SharedMemory` transfers its
actual allocation owner into the extension before hardware effects; mapping
validation continues to resolve item 497 through that same retained owner.
The SMMU adapter forwards these operations only after installation and keeps
the entire backing retained on failure.

`python3 platforms/sunfish/test-native-mpss-linux-smem.py` runs eleven
allocation/native groups and four composition groups. Checks include every
header/cursor failure and selected payload boundaries before/after effects,
readback corruption, changed tables, busy/lost locks, unwinding stores,
one-use preparation, and byte-for-byte preservation outside all planned
stores with the actual volatile adapter. It also runs the twelve predecessor
groups, shared mapping regressions and the existing source audit (now ten
ownership mutations, retaining its original checks). Seven additional
behavioral mutations are rejected. Concrete AArch64 functions cover the
actual parent, native writer and shared-owner composition; the complete
seL4 library builds with the selected RMTFS feature. Snapshotted source and
evidence are in `.cache/sunfish-native-wifi/private/mpss-linux-smem/`.

The original `rmtfs` image keeps the predecessor-only read-only layout.
The new `mpss-smem` image uses the extended Window constructor and executes
the extension through the actual retained SMMU adapter after RMTFS provider
preparation. The preparation receipt asserts only bounded memory preparation;
it does not initialize the SMP2P header/version or entries, notify a peer,
start GLINK, authenticate MPSS or assert modem/WLAN readiness.

### Concrete pre-auth memory boot entry

Build the opt-in composition with:

```sh
SUNFISH_PRIVATE_INPUT_DIR=/path/to/prepared-private-inputs \
  sh platforms/sunfish/build-native-startup-preflight.sh mpss-smem
```

`pixel_mpss_smem.rs` runs after the retained RMTFS factory. It preserves that
provider's existing error screen during init if preparation did not finish.
For a new extension attempt it checks the actual receive runtime, retained
domain and completed provider factory, then delegates to the domain's real
allocation owner. It polls pending IPA/GSI notifications and repeats health
checks between every bounded step and before taking the preparation receipt.
Each native store also checks the pre-auth generation/factory contract and
the real SMMU fault registers. The read-only fault delegation validates the
existing injected virtual/physical bindings and uses the same GR0/CB4 FSR
offsets and defined-bit mask as the retained adapter. It neither clears nor
writes SMMU status. Faults latch the attempt and retain all owners; no retry,
unvote or cleanup is introduced.

`native-startup-mpss-smem.system` splits the prior SMEM tail into one read-only
page, five writable pages at `8611f000`, and a read-only remainder from
`86124000`. The complete SMEM CPU view stays contiguous and uncached. The
actual CapDL audit verifies all five new frame mappings, the remaining
read-only pages, injection symbols, aliases, permissions, cache attributes,
guard pages and inherited IRQ/SMC capabilities. It adds no IRQ or SMC
authority. The closed native store whitelist protects the IPA tail sharing
the first writable page; page permissions alone cannot do that.

`test-native-mpss-smem-entry.py` compiles the actual entry module against real
retained native CPU-memory owners with host-only outer display, lifecycle
and pending-IRQ providers. Each of eleven scenarios runs in a fresh process.
They cover success, missing prerequisites or writable views, loss of the
cold contract, faults during stores, IRQ failure before the first and after
the final step, duplicate invocation, and retention of earlier provider
diagnostics. No physical IRQ, secure call or hardware ownership follows
from those host results. The source audit binds this result and the earlier
core proof to the concrete entry, extended Window and live guard, rejecting
ten source mutations.

The frozen **unbooted** image is 27,942,912 bytes, SHA-256
`cc7deffb65f614486b06f92998356311e9c96c81aa2a8c2ec71c332e8a954000`.
Its evidence directory is
`.cache/sunfish-native-wifi/private/native-startup-mpss-smem/`.
The build contains 6,216 root objects and 3,299 selected frames. The audit
rejects 174 capability/layout mutations and verifies one writable metadata
page, five writable modem pages and 506 read-only SMEM pages. The manifest
binds 251 compiler inputs, 282 source files, the private inputs, runtime/DTB
identities, proof manifests and exact image. The attempt is unused and
`ready_for_cold_device_check` remains false. Keep the phone in LineageOS
while runtime signaling, modem power/authentication and host-service
dispatch are composed for the next necessary startup experiment.

### Selected SMP2P runtime: offline implementation

`sunfish_mpss_linux_smp2p.rs` now owns the actual completed SMEM extension
and its one-use preparation receipt. It publishes the Linux two-entry
`master-kernel`/`ipa` layout in AP item 428, advertises version 2, supports
downgrade to version 1, and intersects the negotiated feature bits. The
host `master-kernel` value remains zero. The incoming item 435 is looked
up through the real locked directory after authentication is submitted;
its address is not assumed to follow the host allocation at a fixed offset.
Two matching payload samples are required before using its state. An
unstable sample produces no protocol writes and can be observed again.

The IPA clock reply publishes enabled before valid (`2` then `3` for held
power, `0` then `1` when inactive), with the same changed-value notification
order as Linux. The initial negotiated restart acknowledgement follows the
incoming restart-done flag. Fatal/stop assertions, loss of an observed ready
state, a subsequent restart flag change after ready, changed target names,
replaced items, regressing lifecycle/cursors, lost power ownership or foreign
AP writes stop this one-shot owner. This implements no recovery, stop or
fatal output request. An observed ready value is not proof of a physical
interrupt, successful authentication, WLFW availability or usable Wi-Fi.

The allocation's existing volatile writer admits a separate closed SMP2P
sequence after its completed metadata/zero stage. Forged requests for the
host control value, adjacent items, arbitrary fields or reversed IPA state
ordering are rejected by that boundary. The new native notification adapter
can write only bit 14 to APCS `17c0000c`; its constructor requires retained
Device mapping authority and its callback must bind actual lifecycle,
power references and mapping health. The selected DT's `sdm845-apss-shared`
fallback and the pinned mailbox driver's matching data establish offset 12.
A completed CPU notification store is not a peer acknowledgement.

The opt-in `sunfish-native-startup-mpss-smp2p` library feature moves the
actual prepared extension into this controller inside `SharedMemory` and
the retained SMMU adapter. It stores that owner before initialization effects,
including unwinding failures. Item 497 and the established mapping identity
continue to resolve through the same owner. No new system image, APCS/IRQ
capability binding or physical attempt has been prepared for this feature;
the earlier frozen image remains an unused pre-auth memory-only artifact.

`python3 platforms/sunfish/test-native-mpss-linux-smp2p.py` passes fourteen
protocol/native groups, two actual shared-owner activation groups and forty
inherited groups. It compiles the original pinned Linux C initialization,
outbound-entry, negotiation, state-update, IPA-notify and restart-ack functions
to compare complete 340-byte snapshots and every doorbell snapshot for both
versions and both power states. Eight behavioral mutations are rejected.
The concrete volatile/shared-owner composition compiles for AArch64 and the
full seL4 library builds with this feature. Private source snapshots and
results are in `.cache/sunfish-native-wifi/private/mpss-linux-smp2p/`; the
original-C fixtures are in `private/mpss-smp2p-linux/` under the same cache.
The previous allocation and eleven entry scenarios also pass after this
change. These are CPU/build results only; no device action was taken.

The remaining integration must bind a real runtime lifecycle gate, modem
power leases and PAS4 owner, the SMP2P physical IRQ and native GLINK dispatch.
RAM-backed RMTFS/TFTP and IPA QMI must remain serviceable during authenticated
boot rather than waiting for the global ready indication. Only then can a
new combined image be frozen and audited for a necessary cold device test.

### Selected modem power and AOP endpoint: offline implementation

`sunfish_mpss_linux_power.rs` plans the modem rails from the parsed live
command database. It resolves the fixed `mx.lvl`, `cx.lvl`, `mss.lvl` and
`xo.lvl` resources, validates their ARC address/type and derives the maximum
raw corner index from the u16 auxiliary map, with the same first-padding-zero
and INT_MAX-clamp rules as pinned `rpmhpd.c`. This is the power-domain format;
the regulator consumer's added-one voltage convention does not apply. The
selected native profile also requires a zero/off index and a usable nonzero
corner. XO's source-defined on value is 3. No prior hardcoded CX6/MSS7 tuple
or downstream pre-IPA bus baseline is reused as the startup requirement.

The selected Linux MPSS driver has proxy domains CX and MSS, while the
Sunfish RPMh description makes MX the parent of CX. The native cold experiment
explicitly holds the maximum active MX corner before CX, then MSS and XO.
That parent floor is an intentional experiment difference from Linux genpd
aggregation, not a claim to have reproduced all other consumers. It programs
only active commands and retains them until reboot; it neither edits resident
sleep/wake banks nor implements Linux handover unvotes. The selected MPSS DT
has XO and QMP, with no optional MPSS interconnect path, aggre2 clock, CX/PX
regulator supplies or MPSS IOMMU property. SCM's separate crypto-to-EBI path
is still required and retained around its secure calls.

The new `sunfish_mpss_linux_power_native.rs` owns the actual QMP/doorbell
views under a trusted lifecycle callback. It opens the existing bounded
handshake, sends the original Linux `qcom_aoss.c` 64-byte zero-padded ON
buffer, observes transport consumption and keeps the negotiated channel.
The previous downstream probe's 96-byte format remains a distinct profile.
Only AP descriptor words 4/6/9/11, the 17-word packet at MSGRAM offset 128
(hex), and bit 0 at APCS offset c can be written. The adapter never sends OFF,
clears a timed-out TX length, closes the channel or retries. Its cached
diagnostic report records link/payload/publication/consumption checkpoints,
descriptor/length snapshots, attempted stores and notifications without
performing additional I/O. These counters do not establish remote acceptance.

The power coordinator sends ON before its rail transactions, checks the real
RSC bank snapshot around every request and repeatedly reads all four rails.
The same actual RSC endpoint remains exclusively borrowed from its retained
root owner; no new endpoint or copied resource address substitutes for it.
Partial, uncertain or unwound work consumes the power attempt. A composed
secure-call provider checks both these live rails and the existing retained
SCM bus floors on acquire/release. Lease identity and outstanding status stay
in the original SCM owner, including when a caller drops a live lease. The
original SCM provider delegates to those same lease operations and retains
its prior behavior. No call lease releases a hardware floor.

`python3 platforms/sunfish/test-native-mpss-linux-power.py` passes ten new
groups plus 96 inherited SCM/QMP/dependency groups. The C oracle compiles the
original pinned rpmhpd level-map/performance/power-on functions for 93 maps,
and the original q6v5 load-state and QMP send functions for the exact packet.
Tests exercise the real transaction engine and volatile QMP adapter in host
RAM, all 44 request-timeout positions, lost floors/banks, uncertain AOP work,
owner retention, outstanding leases, packet footprint and diagnostic phases.
Eight behavioral mutations are rejected. Concrete AArch64 functions use the
actual native QMP and RSC adapters and composed call provider; the full seL4
library builds. Evidence is in `private/mpss-linux-power/` and original-C
fixtures in `private/mpss-power-linux/`, under `.cache/sunfish-native-wifi/`.
The SMP2P suite was refreshed successfully against the same source tree.

The physical Pixel was verified in rooted LineageOS on the unchanged boot
`193ee37d-2450-419c-8e0f-11ed9edf253b`. Restarting the authorized ADB daemon
as root worked. The running kernel has CONFIG_DEVMEM and CONFIG_DEBUG_FS
disabled, so it does not export a raw command-database dump through those
interfaces. No phone reboot, radio change, partition write or native power
test was performed. The observation is recorded separately in
`private/lineage-root-power-inventory/`; CPU tests do not claim it as rail proof.

The root entry still needs to bind the actual lifecycle callbacks, retained
AOP/RSC capability views and live database snapshot, extend RMTFS's pre-auth
lifecycle honestly before PAS4 submission, and dispatch GLINK services while
waiting for modem readiness. The new power components are not yet a frozen
bootable test image or a modem/WLAN startup result. Keep the Pixel in LineageOS.

### Combined pre-auth boot entry

The opt-in `sunfish-native-startup-mpss-preauth` entry now composes the real
retained IPA receive, prepared RMTFS factory and fresh SMEM extension with
the selected modem power and SMP2P components. It captures the actual injected
command database twice, derives the rail plan from those bytes and rechecks
the complete snapshot during power preparation. The entry retains its native
QMP/power owner before effects, uses the existing RSC endpoint, and verifies
the composed modem/SCM holds before and after transferring the actual SMEM
preparation receipt into the SMMU adapter's SMP2P controller. The final pass
state is published only after the last hold verification and pending IPA/GSI
health check. No MPSS authentication/reset release or GLINK data publication
occurs in this profile.

The QMP endpoint and later SMP2P signal share the root's APCS page through
serialized, fixed bit-0 and bit-14 adapters. Live callbacks check the retained
cold RMTFS generation, real SMMU fault status, injected physical identities
and original endpoint pointers. The actual CapDL audit pins virtual addresses;
the runtime retains the injected pointers rather than constructing MMIO
pointers from integer constants. The parent entry still checks the fixed
command-database virtual bindings. No constant callback claims a modem-ready
transition. This pre-auth profile never polls a peer or sends an IPA power
reply; it only initializes the two AP entries and sends their two notifications.

Earlier factory/SMEM errors retain their original screen and first diagnostic.
The new entry cannot replace them if it never started. Once started, its
own error latches, owners and any partial state remain until reboot. The
settled screen shows the power phase/error, cached AOP checkpoint and lengths,
rail attempts/acknowledgements and final values, SMP2P store/notification counts
and IPA receive health.

`test-native-mpss-preauth-entry.py` executes the exact entry module in sixteen
fresh host processes with native QMP/shared-memory adapters and explicit
outer factory, health, IRQ, RSC and remote-peer models. Cases include the
complete handoff, missing prerequisites, original-error preservation, invalid
physical/database bindings, AOP/rail timeouts, guard loss, late faults and IRQ
loss at each final boundary. Host views are relocated allocations; these
results prove neither physical mappings nor the simulated outer providers.
The separate concrete AArch64 entry build and capability audit cover the
native binding. `audit-native-mpss-preauth-source.py` preserves the inherited
audits and rejects eight additional binding/order mutations. The inherited
IPA loader audit now checks the shared SCM lease helpers and both consumers,
including seven rejection cases for lost lease identity or quarantine.

Build the combined temporary profile with:

```sh
SUNFISH_PRIVATE_INPUT_DIR=/path/to/prepared-private-inputs \
  sh platforms/sunfish/build-native-startup-preflight.sh mpss-preauth
```

`native-startup-mpss-preauth.system` adds only the QMP MSGRAM page at
`0c300000` and APCS page at `17c00000`, with unmapped CPU guard pages and
physical-address injections. It adds no IRQ or SMC capability. The validator
checks these frame identities, mappings, aliases, rights/cache/execute bits
and injections in addition to all inherited IPA/RMTFS/SMEM checks. Subpage
register/bit limits are enforced by the closed software adapters, since page
capabilities alone cannot enforce them. The precompile identity record,
proof/source snapshots and final image are frozen before a physical attempt.

This combined check is necessary to establish the pre-auth providers on real
hardware before adding PAS4 and concurrent host services. The RMTFS factory
still intentionally represents only a stopped pre-auth modem in this image;
its runtime lifecycle must be extended honestly when PAS4 is integrated.

The combined image is **frozen and its one attempt is consumed**, 27,979,776 bytes, SHA-256
`003e016f7d57df4e08597b73b2d2df036d6d905e256a5d8c6d92577c4d6703e7`.
It contains 6,231 root objects and 3,301 selected frames. All 192 capability
mutations are rejected; sixteen actual-entry host scenarios and eight new
source-binding mutations pass. Its private evidence directory is
`.cache/sunfish-native-wifi/private/native-startup-mpss-preauth/`, with 256
compiler-input snapshots, 301 source bindings, seven private input identities
and pinned runtime/DTB/boot identities. Historical proof-bound documentation
is preserved separately from the notes captured at freeze time.

The user confirmed cold Fastboot after the requested ten-second power-off.
On 2026-10-27 at 06:31 UTC, the Mac verified physical serial
`02281FQC200058`, product `sunfish`, unlocked state, slot `a`, and a
`10000000`-byte hexadecimal download limit. All 57 frozen artifacts, 301
source bindings, 256 compiler inputs and ten environment identities matched
before publication. The attempt was consumed before issuing the single
`fastboot boot`; both sending and booting returned `OKAY`. The transcript,
verification, device identity and attempt record are saved in
`.cache/sunfish-native-wifi/private/device-runs/mpss-preauth-20261027T062657Z/`.
No partition was flashed. `IMG_9185.heic` shows `MODEM HOST SMEM` / `HOST
SMEM STOP`, native error `3`, IPA RX healthy `1`, and no prepared receipt.
The extension report is absent: attachment returned before retaining the
extension. The three printed physical addresses are intended layout labels,
not evidence that those items were allocated. The frozen entry reaches this
screen only after the real RMTFS factory finished preparation/sharing; that
prerequisite completed, although its SCM return registers are not shown in
the photo. Modem SMEM preparation, power and SMP2P publication remain unproved.
The attempt is consumed and must not be retried. Cleanup was verified over
physical ADB: LineageOS boot completed, `system_server` was running, and the
new boot ID was `549b7a97-f88d-4674-9c59-dd5f3fa02d75`.

### Corrected post-install SMEM handoff

The failure was reproduced offline through the real
`Native::install_experiment` -> receive-memory handoff -> SMEM attachment.
The installer deliberately sets its `failed` latch after *every* result to
consume its MMIO write path. The new SMEM/SMP2P delegation incorrectly called
that installation-only `check()`, so it rejected even a successful, quiet,
retained domain. Earlier entry tests substituted the outer `Probe` and called
the shared owner directly, missing this layer. The captured baseline regression
fails at attachment with `State`, matching the stopped screen.

The six post-install modem operations now use `with_retained_modem`: it
revalidates the actual installed routes, context and all eight table images,
retains the original shared owner, performs the bounded child operation, then
revalidates again. A separate quarantine latch is raised before checks/effects
and cleared only on full success; errors and unwind stay terminal. The
installation latch is never cleared, so this change cannot replay installation
or authorize additional SMMU writes.

`test-native-mpss-postinstall.py` runs the real native adapter against CPU RAM.
Four new groups cover the full consumed-install/receive/SMEM/SMP2P path;
changed route, table, context, global/context fault, IRQ fault and incomplete
installation; failed/unwound children; and a domain change during the child
operation. All 113 native/dependency groups pass. Five behavioral mutations
are rejected: the old installation check, missing before/after domain checks,
ignored quarantine and reopening installation writes. These are host results,
not a physical pass.

The corrected V2 image was rebuilt and all inherited behavioral, original-C,
AArch64/seL4 and source checks passed, including the sixteen actual pre-auth
entry scenarios. The final capability audit rejected all 192 mutations and
retained the same 6,231 root objects and 3,301 selected frames. The image is
27,979,776 bytes, SHA-256
`485aa6ca1e270cedd865f3c630c55287570df60edee678ceaf14a5b280eab2b7`.
Its distinct experiment directory is
`.cache/sunfish-native-wifi/private/native-startup-mpss-preauth-v2/`.
The freeze procedure binds the post-install regression evidence as well as
the original providers, actual compiler inputs, private data and environment.
On 2026-10-27 at 07:04 UTC, the user confirmed cold Fastboot, and the Mac
verified the same unlocked physical Sunfish, slot `a`, and download limit.
All 60 artifacts, 304 source bindings, 256 compiler inputs, ten environment
inputs and the post-install regression proof matched the frozen manifest.
The V2 attempt was consumed before its single `fastboot boot`; sending and
booting both returned `OKAY`. Its transcript and identity records are in
`.cache/sunfish-native-wifi/private/device-runs/mpss-preauth-v2-20261027T070327Z/`.
`IMG_9186.heic` records `MODEM PREAUTH PROVIDERS` / `PREAUTH PROVIDERS PASS`.
Native/power errors are zero, power/AOP phase is `5`, AOP stores are `15`
(21 decimal), and both TX/RX message lengths are zero. Rail attempt/ACK masks
are both `f`; the source-selected final CX/MSS/MX/XO selectors are `6/7/4/3`.
These are the actual device selectors, not the host fixture's synthetic levels.
SMP2P phase is `1`, stores sent/done are both `10` (16 decimal), and IPA RX
remains healthy. Notification counts are not visible and are not transcribed
as observed values. The final pass requires successful retained RMTFS/SMEM
preparation, QMP completion, both live power checks, host SMP2P initialization
and final IRQ/health checks in this exact boot. This establishes those fixed
pre-auth prerequisites on hardware. It does not establish PAS4 authentication,
MPSS readiness, runtime GLINK/QRTR services, WLFW startup or network frames.

Cleanup was confirmed over physical ADB: LineageOS boot completed,
`system_server` was running and the new boot ID was
`a7594f5d-cfe0-4475-9611-891a38dc85e8`. Both images are consumed. The frozen V1
source/image/photo remain the original failure evidence, and V2 remains the
successful corrected pre-auth experiment. The next integration must keep the
actual storage/SMEM/power owners and extend the RMTFS reset lifecycle before
the sealed PAS4 authentication request can release MPSS reset.

### Retained PAS4/RMTFS authentication boundary (offline)

The opt-in `sunfish-linux-mpss-auth` library feature adds
`sunfish_mpss_linux_boot::Boot`. It retains the actual PAS4 loader and original
RMTFS server, including its native pool and verified RAM shadows, rather than
accepting a copied status/address as a replacement. The platform still retains
and lends the actual modem/SCM power backend for each bounded loader step.

Before metadata or target access, and throughout loading, the original pool
must still be shared in the same stopped generation. The loader's pending
`auth_submitted` Guard is only intent. On the sealed AUTH request, the original
pool consumes its one lifecycle transition and calls its native `AuthGate`
**before** delegating the secure operation. The native adapter independently
requires its exact RMTFS assignment to have succeeded and completed, validates
the closed PAS4 envelope, and consumes the transition before calling the gate.
The gate must permanently represent reset as possibly released, including a
failed, uncertain or unwound secure operation. A provider that continues to
report stopped state or changes identity/generation is rejected.

Successful AUTH, resource release and final guards are required before moving
the actual sealed `AuthReturned` receipt. The same server then remains available
while the future native event loop waits for MPSS readiness; there is no global
ready-bit prerequisite for storage dispatch. The transport must independently
establish its actual FIFO/IRQ/generation authority. Any error or unwind keeps
the loader, metadata/target mappings, native I/O and RAM shadows retained and
terminal. There is no reset retry, re-zero, ownership reclaim or Drop cleanup.

`test-native-mpss-linux-boot.py` passes nine handoff groups and all 231 host
groups, with six behaviorally rejected mutations. The tests use the real
loader, volatile reserved-pool adapter, RAM-shadow server and QRTR owner in
CPU RAM. They cover ordering, continuous stopped provenance, lost generation,
stale stopped state after submission, both secure status registers, transport
and release failures, interrupted transitions, actual native assignment
completion, retained owners, and storage reads/writes through QRTR after AUTH.
The complete private modem image was compared again against the pinned original
Linux MDT/PAS C functions, and the original RMTFS wire/legacy regressions passed.
A concrete no-std `Boot<Window, Native<Gate, Monitor>>` wrapper compiled for the
actual AArch64 seL4 target with unchanged pinned dependencies. Its callbacks
are compilation fixtures, not native root bindings or hardware authority.
Private results and exact source snapshots are in
`.cache/sunfish-native-wifi/private/mpss-linux-boot/`.

The native root's concrete gate is still pre-auth-only. The next integration
must bind this boundary to that root's actual reset lifecycle, retained power
and PAS4 monitor; reserve and validate the real modem/metadata mappings and
private firmware input; and compose the native GLINK/QRTR/IRQ loop needed
during the ready wait. No MPSS authentication image has been built or booted.
The phone remains in LineageOS. Native MPSS/WLFW startup, association and
network traffic remain unproved.

### Retained GLINK memory delegation (offline)

The opt-in `sunfish-linux-mpss-glink` feature adds a memory boundary inside
the original `sunfish_mpss_linux_smp2p::Controller`. The controller retains
its original extension, IPA item-497 allocation, native metadata/lock views
and payload writer. Claiming GLINK consumes the real sealed `AuthReturned`
receipt and requires the same generation, an active controller, active power
and a live lifecycle at least `AuthReturned`. A global MPSS ready bit is not
required: the later runtime must serve boot-time requests before that bit.
This does not yet connect the boundary to the native root's pre-auth-only
gate or release MPSS reset.

The selected Linux GLINK SMEM driver assigns descriptor words 0/3 to the
peer and words 1/2 to the AP. Its RX item 480 appears lazily and supplies its
own length. The new boundary discovers that item through the retained SMEM
directory under the original TCSR lock, pins its exact extent, and checks
the directory and AP-owned state before and after each operation. The native
writer admits only item-479 TX bytes and the two AP-owned cursors. It cannot
reset cursors, write RX data, modify remote cursors, publish allocation
metadata or write SMP2P through a GLINK request. The existing SMP2P path
continues to sample fatal/restart state and answer its selected power query.
Errors and unwound operations retain the owners and block further use;
no recovery, cleanup or replay path is introduced.

`platforms/sunfish/audit-native-mpss-glink-memory.py` compiles the original
FIFO functions from pinned Linux commit
`2267e248204e7e051e0f7355298949a6440de579`. The RAM comparison matches all
descriptor, TX, RX and readback bytes across FIFO wrap, with RX sizes of
4, 16 and 32 KiB. It also checks the selected DT's GLINK SPI 449 (GIC IRQ
481) and APCS mailbox bit 12; this is source evidence, not physical IRQ
delivery. The test fixture retains the actual volatile adapters and emulates
only the hardware lock's claim semantics and explicit injected failures.

`platforms/sunfish/test-native-mpss-linux-glink.py` passes 11 new GLINK memory
groups and 266 total host groups. The one private-image comparison remains
separately exercised by the prerequisite loader audit. Tests cover delayed
RX allocation, variable RX length, ownership/cursor drift, bounds, foreign
locks, lifecycle/generation/power loss, lost writes, errors after writes,
unwind, and the native whitelist challenged with test-only forged tokens.
All eight compiled behavioral mutants are rejected. The selected SMP2P and
AUTH/RMTFS regressions were refreshed with the new feature disabled. A
concrete no-std controller using the actual native window, payload and signal
types compiled for the seL4 AArch64 target with unchanged pinned dependencies.
The exact 71 source bindings, prerequisite proof hashes, test logs and build
result are in `.cache/sunfish-native-wifi/private/mpss-linux-glink/`.
The runner's `--reuse-prerequisites` option validates the recorded source
closures, referenced proof bytes and consumed original-C fixture hashes
before reusing an unchanged prerequisite result.

This boundary performs no FIFO space accounting, VERSION/OPEN/intent
negotiation, GLINK doorbell or IRQ dispatch itself. The transport integration
below now adds the preceding channel opening, variable RX length and forwarding
through the enclosing installed domain. Actual root reset/PAS4 bindings,
reserved modem/metadata capabilities and interrupt scheduling remain required
before building an authentication image. No device action occurred in this
memory-boundary validation; the phone stays in LineageOS.

### Selected Linux GLINK opening and retained runtime (offline)

`sunfish_mpss_linux_transport.rs`, enabled by `sunfish-linux-mpss-glink`,
implements the selected VERSION-1 exchange and IPCRTR opening ahead of the
existing QRTR session. It intersects the supported intent-reuse feature with
the peer's offer, handles a counter-offer, supports local and remote opening
orders, and requires both directions of OPEN/ACK before handing over. The
sole configured local channel is ID 1; the remote ID is learned from the
actual IPCRTR OPEN, not assumed to remain 2. Other bounded, named OPENs are
remembered without ACK or buffer grants, as for a Linux channel with no
interested rpmsg consumer. They can also be consumed after IPCRTR opens.

VERSION can be published before the peer allocates RX item 480. The same
FIFO code subsequently uses that item's pinned length for bounds, peer
progress and wrap arithmetic; the default established legacy profile remains
16 KiB. TX reserves 16 bytes, including the single reserved READ_NOTIF slot
when blocked. A waiting control reply survives backpressure without replaying
its received request. The monotonic counter deadline is five seconds from
the first negotiation step; a root caller must supply the actual counter
frequency and schedule the deadline. No guessed timer interrupt is used.

The peer can advertise intents before its final OPEN acknowledgement. Up to
eight bounded early INTENT/INTENT_REQ records are copied privately and
consumed once while negotiation reaches that acknowledgement. They move
with the rings into the runtime and are decoded by the original session
after its control plane starts. This keeps that prepared session pristine
for attachment, preserves its actual service registrations/providers and
binds only its learned channel IDs. Handoff resets no cursor and reallocates
no ring. Local receive buffers remain one-shot; no reusable buffer is granted
merely because intent reuse was negotiated. A native logical reset is rejected.

The `sunfish-native-startup-mpss-glink` feature provides
`sunfish_mpss_linux_transport_native::start`. It borrows the actual enclosing
`sunfish_smmu_native::Native`, consumes the successful loader receipt, and
retains that borrow through the negotiated adapter. Every native I/O operation
passes through the existing retained-domain checks before and after its child
operation. It never extracts a raw SMEM pointer or creates a second writer.
GLINK bit 12 uses the original retained APCS signal object, alongside its
existing SMP2P bit 14. The native bit-12 method independently rejects a
pre-auth lifecycle or inactive power. Protocol failure also quarantines the
enclosing modem delegation without reset, cleanup or reopening install writes.

`audit-native-mpss-glink-transport.py` compiles the pinned original Linux
VERSION, feature/counter-offer and OPEN wire functions. The native tests match
those bytes and both Linux opening orders. `test-native-mpss-linux-transport.py`
passes 12 new transport/domain groups and all 367 host groups, plus 116 FIFO
groups with the new feature disabled. The complete private-image comparison
is separately exercised by the refreshed loader prerequisite. Eleven compiled
behavioral mutants are rejected. Tests include early-intent ordering, delayed
RX allocation, 4/16/32 KiB RX sizes, actual wrapped QRTR HELLO ingestion,
backpressure, malformed/torn input, timeout/clock regression, write uncertainty,
unwind, wrong channel/doorbell, protocol quarantine, and changed SMMU
routes/tables/fault state. The native cases run the real install path and
retained SMEM/SMP2P/volatile adapters in CPU RAM; their lifecycle/power and
counter inputs are simulations, not hardware authority.

A concrete no-std seL4 AArch64 wrapper instantiates the actual enclosing-domain
borrow, negotiation, handoff, receive, publication and control methods with
unchanged pinned dependencies. Exact results, 101 source bindings, prerequisite
hashes and logs are under
`.cache/sunfish-native-wifi/private/mpss-linux-transport/`. This proves the
offline composition, not physical GLINK IRQ delivery or MPSS/WLAN startup.

The root sequence has one source-backed refinement. In the pinned kernel,
`remoteproc_core.c::rproc_start` invokes the processor's start operation before
starting its subdevices. `qcom_pas_start` authenticates/releases reset and
waits up to 5000 ms for the Q6 ready completion before returning; the GLINK
subdevice's start callback then registers SMEM/VERSION. The root comparison
should follow that ordering by default, while continuing SMP2P and IPA clock
service during the ready wait. The codec's ability to operate after AUTH
without a global-ready prerequisite does not establish that early GLINK
service is required by this Linux path. The checked source hashes and ordering
decision are recorded separately in `root-startup-order.json` in the transport
result directory. This is source evidence, not a physical MPSS start result.

The first concrete root authentication entry is described below. GLINK
negotiation and boot-service publication still need to be connected after its
ready milestone. Native MPSS/WLFW startup, association and network traffic
remain unproved on the device.

### Concrete root MPSS authentication and ready check

`sunfish-native-startup-mpss-auth` selects `pixel_mpss_auth.rs` after the
physically proven combined pre-auth sequence. The entry retains the actual
IPA receive/domain owners, modem/SCM power holds, original assigned RMTFS
server, pristine prepared QRTR session and TFTP provider. It stages the exact
private modem image through the tested PAS4 loader and the actual uncached
window; its private monitor uses the existing sole seL4 SMC slot 7.

The root lifecycle consumes the cold/stopped state before entering AUTH,
including failed or uncertain calls. Only the actual loader receipt advances
it to AUTH-returned. The shared SMP2P interrupt is SPI 451 (GIC IRQ 483,
channel 5), and watchdog is SPI 266 (IRQ 298, channel 6), from the pinned
Sunfish DTS. The existing IPA/GSI/SMMU notification bits remain with their
owners. Pending modem bits survive the earlier IPA polling helpers and SMCCC
continuations; watchdog, unknown badges and ownership faults stop the retained
boot without reset, reclaim, unvote or replay.

Immediately before AUTH, a locked cold controller poll rejects any existing
peer item. After authentication, the root services the actual SMP2P controller
and IPA clock query while waiting up to five seconds using the physical
counter. A real delivered SMP2P notification and a stable current ready bit
are required alongside the actual AUTH receipt. The IRQ stays pending across
version downgrade and unstable samples until the controller has processed a
stable negotiated sample. The two opaque receipts are retained separately;
neither a report number nor a previously observed ready bit creates a receipt.

The monitor reuses the tested SCM64 interrupted/busy mechanics: original Linux
arguments are restored for busy retries, with 30 ms waits and 21 retries;
interrupted resumes preserve the returned context. The native work bounds
also cap interrupted and total calls. Unsupported WAITQ statuses stop and
retain the owners instead of guessing a resume operation. Metadata is never
read after INIT submission, and target RAM is never read or written after
AUTH submission, even on error.

The system grants three private uncached RW/NX metadata pages and exactly the
live/pinned Linux MPSS reservation `0x8b000000 + 0x09800000`, using 76 large
frames. The large embedded firmware exposed the old loader address overlap
and allocator gaps that the smaller images did not reach. An isolated SDK
places only its loader at `0xc0000000`; kernel, monitor and initialiser remain
identical to the base SDK. A runtime entry check requires the previously
observed ABL source `0xb0081000` before relocation, bounds the complete payload,
and stops visibly for another placement. Offline checks cover the full
permitted source/destination envelopes and every actual copy destination
against all three cached DT reservation sets.

The remaining display reservation and the Linux-only XBL reservation are
allocated as deliberately unmapped frame objects, with no PD frame cap. This
keeps the allocator out without adding an application mapping. The derived
CapDL audit checks every frame object, including ELF data, for reservation
overlap, plus actual frame granules, physical/virtual aliases, guards, rights,
IRQ bindings and the sole SMC cap. The base SDK is unchanged.

`test-native-mpss-auth-root.py` exercises the actual root Gate and IRQ policy
with actual sealed loader/SMEM receipts from CPU fixtures. Injected symbols
are numeric models: the host tests do not dereference the fixed root VAs or
execute SMC/IRQ operations. The existing loader handoff suite adds an external
quarantine regression before and after AUTH. The concrete complete entry is
compiled for seL4/AArch64; the build/source/CapDL/layout audits and compiler
input snapshot must all pass before `freeze-native-mpss-auth.py` creates an
eligible one-shot device image.

This first image stops before GLINK/WLAN startup. `READY RECEIPT` is historical
evidence of the milestone, not sustained modem health: a later watchdog or
peer stop is displayed separately while retaining that receipt. Firmware
services, native WLFW discovery and the radio/data path remain subsequent
steps. No physical MPSS authentication or ready result is claimed here. The
phone stays in LineageOS until a frozen image is ready for a fresh cold test.

WLFW service control over QMI/QRTR is separate from the ath10k CE/HTC/WMI/HTT
path. Command/event IPA DMA does not establish either bidirectional packet
traffic or Wi-Fi association. The later network bridge must validate and
decode receive data before delivering frames to the network stack.

## Selected Linux IPA QMI profile

`rust/src/sunfish_ipa_linux_qmi.rs` follows the same pinned kernel's
[`ipa_qmi.c`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/net/ipa/ipa_qmi.c),
[`ipa_qmi_msg.c`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/net/ipa/ipa_qmi_msg.c)
and v4.2 resource table. It is separate from the earlier downstream
`qcom_ipa`/`qcom_ipa_service` policy; no downstream post-init permit is reused.

The native request is 108 bytes including the seven-byte QMI header. It
derives SRAM bounds from the existing measured `0x415` memory geometry and
region table. The modem routing-table end is index 7, rather than a byte
address or count of 8. Control traffic is directed to AP endpoint 9. The
request always includes tag `0x18` (`skip_uc_load`), false on a cold boot;
it omits absent hash, compression and drop-stat fields instead of encoding
zero-valued optional TLVs. The original factory and QMI encoder produce
identical complete bytes for both false and true encoding inputs.

The host service is `0x31`/version 1/instance 1; discovery targets modem
instance 2. INIT_DRIVER uses a client handle, while registration responses,
DRIVER_INIT_COMPLETE responses and the final INIT_COMPLETE indication use
the host server handle. A modem request's source port need not equal its
discovered service port. Both are checked against their respective roles.

`ColdSession` changes publication-dependent state only after an explicit
confirmation of the complete record. Opaque records bind owner identity,
trusted generation and sequence; wrong peers, stale transactions and
cross-owner commits cannot advance the handshake. Uncertain publication,
timeouts and peer loss terminate this cold-only session without automatic
retry. The native binding must provide the real transport confirmations
and keep its actual startup/domain/firmware/pool owners alive. Register the
host services and modem lookup before MPSS reset release; the handshake
itself occurs after MPSS runs and advertises its service. It cannot be a
prerequisite for starting the remote processor that must answer it.

The original `qmi_send_indication()` allocates a server-side transaction;
its ID may be zero or nonzero. `prepare_indication()` takes the trusted
allocator's ID instead of forcing zero. The existing downstream GLINK IPA
indication helper requires zero and must not be silently reused for this
selected Linux path. The comparison includes the original transaction-init
and send helpers with deterministic host ID allocation and socket stand-ins.

Three protocol conditions are distinct: a successful INIT_DRIVER response,
a responded-to DRIVER_INIT_COMPLETE request, and a requested and published
INIT_COMPLETE indication. The QMI `uc_ready` flag is not the hardware
UC_INIT_COMPLETED interrupt or the hardware `uc_loaded` state. All six
orderings match the original Linux callbacks. Unlike Linux's client callback,
which ignores the decoded standard response, the native tracker deliberately
requires result and error both zero. Other response metadata is diagnostic
data and cannot create hardware authority.

`python3 platforms/sunfish/test-native-ipa-linux-qmi.py` verifies the pinned
archive, compiles its original resource table, request factory, message
descriptors, full QMI encoder and handshake callbacks, then compares the Rust
implementation. It covers malformed/truncated/duplicate TLVs, all raw status
byte values, publication failures, stale records and terminal loss. It also
rejects deliberate faults and builds the core for AArch64 without std.
Private source snapshots, the original-source oracle and results are in
`.cache/sunfish-native-wifi/private/ipa-linux-qmi/`. No device action or native
QMI/MPSS/WLAN success is claimed by these offline results.

### QMI transport composition

The `sunfish-linux-ipa-qmi` Cargo feature selects an explicit profile on the
existing `qcom_glink_qmi::Session`. Its child module
`sunfish_ipa_linux_qmi_link.rs` carries opaque `Prepared` records instead of
converting them into downstream IPA client/server types or readiness permits.
The two IPA client profiles cannot coexist. Local ports remain disjoint from
RMTFS, heap, SERVREG, WLFW and TFTP. The host IPA service must be published and
the modem service uniquely discovered before a Linux IPA record can be queued.

The Linux profile also selects its own bounded request validator in the real
router. This preserves Linux's treatment of raw registration/status bytes
while keeping the downstream boolean checks unchanged. Server replies match
the held request's original peer, message and transaction, including a peer
client port distinct from the modem's advertised service port. INIT_DRIVER
uses the local client endpoint; replies and indications use the host server.

Every Linux IPA record requests QRTR credit and keeps its GLINK buffer until
both completions match the published route and intent. `LinuxIpaReceipt` can
only be produced by that retirement. It is consumed by the same transport
session when committing the matching protocol record. Owner, generation and
monotonic record identity prevent stale, cross-session and replayed commits;
an unconsumed receipt cannot be overwritten by another record. Incoming modem
responses may race those completions, so they stay in bounded held receive
slots until the protocol owner accepts them. Returning transport credit alone
does not accept the QMI response or advance startup.

An uncertain FIFO publication quarantines the transport. A matching native
owner must propagate that failure to its retained `ColdSession`; it must also
retain firmware, DMA mappings and buffers until hardware quiescence or reboot.
Logical transport reset is not evidence of hardware quiescence. No automatic
SSR restart, production readiness token or modem-start authority is added.

`python3 platforms/sunfish/test-native-ipa-linux-qmi-link.py` exercises this
actual composition, with host record publication standing in for the native
FIFO. Thirteen groups cover all six handshake orders, both completion orders,
early responses, distinct peer ports, service gates, backpressure, uncertain
writes, stale/foreign records, receipt retention and endpoint loss. It runs
all 133 older transport tests both with and without Linux profile support,
rejects seven deliberate faults, and cross-builds the composed core plus the
full seL4 library. A complete emitted frame also matches the pinned kernel's
original GLINK/QRTR header definitions and assignment code, using the original
QMI encoder's payload. This verifies that bounded wire subset, not Linux's
entire flow-control or channel-negotiation implementation. Private evidence is
in `.cache/sunfish-native-wifi/private/ipa-linux-qmi-link/`.

### Serialized Linux QMI owner

`Owner::attach_linux_ipa()` now consumes the real `qcom_glink_fifo::Adapter`
with its prepared Linux-profile session. The feature-gated
`sunfish_ipa_linux_qmi_fifo.rs` adapter gates every queue, receipt, commit and
held-response access on the live generation, ring identity, cursor and peer
observations. `sunfish_ipa_linux_qmi_owner.rs` retains the actual cold protocol
state, prepared outbound record and held early response inside the existing
serialized owner. It does not substitute a downstream IPA readiness permit.

`service_linux_ipa_step()` performs one bounded action. A prepared reply
survives transmit-slot or destination-flow backpressure; it is not regenerated
or marked queued early. Owner request handles leave the dispatcher queue only
after successful enqueue, while the transport retains their router tickets
until both completions. Complete incoming responses remain held until the
original request's publication commits. Heap/RMTFS handler dispatch and the
manifest-backed SERVREG dispatcher keep their existing ownership and reply
behavior alongside this IPA work. The coexistence test uses the native port
assignments: IPA server `0x4002`, SERVREG `0x4003`, IPA client `0x4004`.

The owner converts the source's 60-second INIT_DRIVER timeout to the supplied
counter frequency and exposes its next deadline. The native entry must supply
one consistent physical counter and arrange a timer wake; this code does not
invent a timer capability or spin while awaiting the peer. Counter wrap is
handled, regression is terminal, and completing the outbound request does not
cancel the still-pending response deadline. New protocol work schedules a
bounded continuation; failure to schedule also stops the owner.

FIFO, peer, protocol, deadline and dispatcher faults stop the owner and fail
the protocol while retaining pending state. An unwinding head/doorbell path
leaves the owner stopped, and its public report cannot indicate protocol
completion even if the handshake had previously completed. Further calls
return before IO; no FIFO head, firmware/reset operation or IRQ ACK is retried.
These are containment properties, not hardware quiescence or permission to
release the retained native resources.

`python3 platforms/sunfish/test-native-ipa-linux-qmi-owner.py` runs 16 owner
groups through the real FIFO adapter and serialized IRQ/service code over
guarded host rings. It covers all handshake and completion orders, early and
duplicate responses, deadlines, counter wrap/regression, host-service
coexistence, destination-flow backpressure, scheduling failure, uncertain
head-store effects and unwinding doorbells before and after protocol completion.
All 161 earlier FIFO/owner groups pass with feature support on and off. Seven
deliberate faults are rejected. The test also builds concrete AArch64
`Owner<NativeIo<...>>` attach/notification/service functions and the full seL4
library. The native-code wrapper uses callback-supplied live observations;
it is compiled only and does not run on the phone. Private evidence is in
`.cache/sunfish-native-wifi/private/ipa-linux-qmi-owner/`.

The remaining native hookup must carry the actual startup resources, real
RAM-backed service pools/providers and prepared session across the MPSS
authenticated-start sequence, then attach this owner after the real GLINK
handoff and wire its deadline wake. The receive-start hardware pass remains
the latest native device result. It contains no modem boot or received packet.

## Selected Linux modem-storage resources

The pinned board
[`sm7150-google-sunfish.dts`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts)
reserves `0xfde01000` for `0x200000` bytes, client ID 1, with VMID
`MSS_MSA` (15). It does not request `qcom,use-guard-pages`, so subtracting
two pages from that physical reservation would change the reference. Native
virtual guard holes remain a separate mapping requirement. The pinned
[`rmtfs_mem.c`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/soc/qcom/rmtfs_mem.c)
shares the whole region from HLOS (3) to HLOS and MSS_MSA with RW permissions
(6) during probe. Userspace `rmtfs` opens `/dev/qcom_rmtfs_mem1`; its allocation
helper repeatedly returns that fixed address and does not perform ownership
transitions per QMI allocation request.

The captured successful cold Linux inventories independently show host RMTFS
(`0x0e`), TFTP (`0x1000`) and IPA (`0x31`, instance 1) before MPSS startup,
and SERVREG (`0x40`) later. Neither captured inventory contains heap-memory
service `0x34`. This establishes the providers in this working reference;
it is not a universal claim about every firmware build. The older native
`qcom_modem_pools` reservation at `0xa2001000` plus a 5-MiB heap remains a
separate downstream profile, not a prerequisite inferred for this experiment.

`sunfish_linux_rmtfs.rs` owns a single pool and the actual verified
`Storage<RamShadow>` provider; its type cannot accept a generic persistent-disk
backend that merely reports RAM-shadow semantics. It prepares at most one 4-KiB page per step,
then performs one pre-MPSS sharing transition. Zeroing and verification before
sharing are native confidentiality hardening rather than a Linux requirement.
Both monitor transport and command status must be zero before the ordered
completion is accepted. The owner consumes its attempt before invoking the
monitor; failures, identity loss and unwinding calls retain the resources and
stop further use. There is no reclaim, retry, PAS, firmware-start or persistent
storage path. Repeated allocation requests report the shared reservation
without zeroing its contents again. The native request schema deliberately
rejects zero-byte allocations even though the Linux allocation helper accepts
them.

Transfers validate physical-buffer and disk-sector bounds, then copy at most
4 KiB through private memory to/from the real EFS shadows. Sync acknowledges
RAM only; all original snapshot slices remain immutable. The eventual QRTR
server must additionally validate the authenticated route, handles and whole
iovec before dispatching these bounded chunks. No packet address is converted
to a CPU pointer. `sunfish_linux_rmtfs_native.rs` contains the concrete volatile
adapter with barriers, scratch readback and a single-use monitor invocation;
its unsafe constructor requires actual mapping ownership, attributes and cold
HLOS provenance from the platform factory. The code does not manufacture that
provenance from readable memory or a successful source comparison.

`python3 platforms/sunfish/test-native-linux-rmtfs.py` compiles the original
pinned SCM assignment functions and the RMTFS permission construction, then
compares the complete 320-byte native scratch representation. It also compiles
the original userspace allocation helper and archives the non-secret service
inventories. Behavioral checks exercise both transfer directions for all four
verified EFS shadows, boundary rejection, one-shot sharing, failure/unwind
containment, guarded host pages through the real volatile adapter, and SCM
status errors. The runner rejects seven behavioral mutations, compiles concrete
AArch64 volatile/RAM-shadow resource functions, and builds the full seL4 library.
Private evidence is in `.cache/sunfish-native-wifi/private/linux-rmtfs/`.

This is offline resource implementation and source/behavior comparison. The
native CapDL reservation, live lifecycle gate and concrete SMC capability still
are now bound in an opt-in concrete preboot image with the storage dispatcher
and retained IPA resources. MPSS startup and the native transport handoff still
need to be added before a composed modem test is selected. No phone operation
follows from this offline audit. The latest native physical evidence remains
the receive-start pass, with no receive aggregates or network traffic.

## Selected RMTFS dispatch through the serialized owner

`sunfish-linux-rmtfs` explicitly enables the selected storage server and its
owner integration, together with `sunfish-linux-ipa-qmi`. It is not enabled by
the receive-probe feature chain. `sunfish_linux_rmtfs_server.rs` retains the
actual `Resources<Native<...>>` and verified EFS RAM shadows. The server can
attach only after the complete buffer has been shared; the serialized owner's
`activate_linux_rmtfs` validates its generation, local endpoint, direct peer
node and channel lifecycle before advertising RMTFS. No heap endpoint is
required. The server binds once to one owner, and the old downstream
storage/heap handler mode is mutually exclusive with this mode.

Only the owner can begin or commit a transport-bound request. It selects RMTFS
from the shared four-slot request queue while leaving SERVREG and IPA requests
available to their own dispatchers. Each service call executes at most one
4-KiB transfer chunk or one metadata action. The complete vector list is
validated before the first chunk, including disk and physical pool bounds,
sector alignment and nonzero spans. File IDs are checked against the live
handle table. As in the Linux provider, handles belong to the authenticated
peer node and can be used from a different QRTR source port on that same node.
Repeated opens of a live partition return the same handle. Closing it never
reclaims the pre-shared buffer. The native implementation additionally avoids
reusing a closed handle ID and retains changed RAM-shadow contents through
close/reopen for this boot; these are explicit differences from Linux's file
and shadow allocation lifetime.

The dispatcher caches the completed response before queueing it. `FlowBusy`
and `NoCapacity` retain that response and its original request without
repeating allocation, copies or sync. After enqueue, the real session retains
the receive slot/router ticket until both GLINK RX_DONE and matching QRTR
RESUME_TX have arrived, in either order. The server's queued-reply count is
local publication bookkeeping, not a claim that the peer received it. Invalid
handles/ranges produce an RMTFS internal-error reply; uncertain effects,
identity loss, binding mismatch, scheduling errors and unwinding actions
quarantine the retained state. A server report alone cannot establish the
health of its outer FIFO owner or authorize resource release.

`python3 platforms/sunfish/test-native-linux-rmtfs-server.py` runs actual owner,
FIFO, QRTR, storage resource and RAM-shadow code over guarded host memory. Its
peer replenishes GLINK receive intents so long storage sequences exercise the
real cyclic intent lifecycle. The source comparison compiles the original
RMTFS 1.1 response descriptors with libqrtr 1.1's complete encoder, comparing
OPEN, ALLOC, CLOSE, device-status and error replies byte for byte. Original
encoded allocation/read requests also traverse the real native dispatcher.
The one-byte device-status field is preserved. The native status handler
reports the checked file's latched status rather than reproducing the apparent
inverted null check in the reference daemon's device-error handler.

The checks include all four partitions in both directions, close/reopen RAM
retention with unchanged originals, whole-vector rejection before effects,
backpressure with both ACK orders, forged-context rejection, one-use binding,
old/new handler exclusion, uncertain copies and unwinding actions. They also
complete the selected Linux IPA handshake while a storage reply is blocked.
The runner revalidates the earlier resource proofs, rejects seven deliberate
faults, builds concrete AArch64 functions for the volatile FIFO plus volatile
RMTFS resource/server, and builds the full seL4 library with the explicit new
feature. Evidence is in
`.cache/sunfish-native-wifi/private/linux-rmtfs-server/`.

This storage/transport test does not release MPSS reset or prove device storage
requests. The concrete preboot binding below adds actual mappings and the SMC
adapter; its modem/transport handoff remains to be implemented and tested.

## Concrete native RMTFS preboot factory

The explicit `sunfish-native-startup-linux-rmtfs` feature extends the proven IPA
receive-stage entry with `pixel_linux_rmtfs.rs` and
`native-startup-linux-rmtfs.system`. The new factory owns the existing verified
RAM-shadow storage and authorized TFTP image source. It creates the real
volatile pool adapter from Microkit-injected virtual/physical addresses and
the sole SMC capability at slot 7. One page is zeroed and verified per step;
one logical SCM assignment follows. The tested SCM helper bounds its busy and
interrupted-call handling and revalidates the same mapping/lifecycle before
every low-level monitor invocation. Both transport and command return status
must succeed before the storage server is constructed.

The factory retains that server, TFTP source and a pristine QRTR/QMI session
with RMTFS, SERVREG, selected IPA and TFTP endpoints prepared locally. It does
not publish HELLO, serve requests or start MPSS. The receive-stage domain,
firmware, rings, power holds and interrupt owners remain in the surrounding
`Startup`. A late IPA/receive fault also closes the RMTFS lifecycle. No old
downstream IPA-ready permit or heap pool is used.

The cold source convention is an explicit unsafe boot precondition. The exact
bound image must boot once on the expected Sunfish after at least ten seconds
fully powered off, with no earlier host MPSS start. The common ABL/secure
firmware handoff is compared with the working Linux reference; the reference
also passes through U-Boot while this native image boots directly. Local
state flags, readable mappings, an absent QRTR service and SMP2P bits do not
prove these external conditions or initial HLOS ownership. The one-use factory
records this reviewed experimental contract and cannot recover or reinitialize
after a fault. This version has only a pre-MPSS lifecycle; the next composition
must extend it using the actual authenticated-start/ready handoff before it can
represent an executing modem.

`build-native-startup-preflight.sh rmtfs` now generates a complete temporary
image. The actual CapDL/page-table audit passes for 3,299 selected frames and
36 unmapped guard pages, including all 512 fixed RMTFS pages and the separate
private SCM argument page. The fixed reservation also matches the hashed DTB
from the selected Linux build. All earlier IPA frame, IRQ and SMC checks remain
active, and 140 capability/layout mutations are rejected. Six additional source
mutations reject a changed SMC slot, relaxed cold/mapping gate, safe rather than
unsafe cold entry, relaxed sealed-call comparison, wrong TFTP transfer-port
binding and bypassed IPA prerequisite. The resource/server behavioral tests and
full AArch64 binary build pass. The generated image has not been booted, so its
RMTFS assignment and device storage requests remain unproven.

Private output is in
`.cache/sunfish-native-wifi/private/native-startup-linux-rmtfs/`. It is an
offline integration baseline, not the next selected device test. The next work
is to connect authenticated MPSS image loading/startup, native GLINK handoff,
IRQ/deadline handling and the retained host services, including any service
work required while modem readiness is pending. Keep the Pixel in LineageOS
while that composed startup image is prepared.

## Linux SMMU measurement

The helper in `platforms/sunfish/mobian-comparison/smmu-readback/` was built
against the exact running kernel configuration and loaded once. It reads
registers only while the SMMU device is already active; it neither resumes the
device nor writes registers. The module was unloaded after capture. Its final
route/global comparison reported no change. Private raw evidence is in
`.cache/sunfish-native-wifi/smmu-readback/`.

* ID0 `4c017e4f`, ID1 `6000003d`: 79 route groups, 61 context banks,
  4-KiB register pages, context-bank base offset `80000`.
* SCR0 `00281c06`; global FSR zero.
* WLAN stream IDs `0240/0241`, mask `0001`, slot 29, translated through bank 9.
* IPA `0520` and `0522` share bank 5 in this Linux configuration; `0521` has
  no route. This differs from earlier downstream planning assumptions.
* Active reference banks use SCTLR readback `67`, ACTLR `1`, CBAR `1f300`,
  CBA2R `1`. Context FSR `400` is the address-format field, not a fault.
  The executor preserves raw preimage checks while excluding those format
  bits from the fault predicate; an actual translation fault still stops it.
* ID0.ATOSNS is set. The native experiment must not issue nonsecure ATS
  translation commands.

The source bases for these interpretations are the pinned kernel's
[`arm-smmu.h`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/iommu/arm/arm-smmu/arm-smmu.h),
[`arm-smmu.c`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/iommu/arm/arm-smmu/arm-smmu.c),
and [`sm7150.dtsi`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/arch/arm64/boot/dts/qcom/sm7150.dtsi).
Linux assigns ASID equal to the allocated bank index and invalidates stage-1
translations with `S1_TLBIASID` at offset `610`, followed by bounded TLBSYNC.
The native Linux-reference profile follows that sequence and keeps the
previous Lineage register policy distinct.

## First native hardware experiment

Build with:

```sh
sh platforms/sunfish/build-smmu-install-probe.sh
```

`SUNFISH_REFERENCE_BOOT` may select the local Android v2 wrapping template.
The build emits an opt-in Microkit image under
`.cache/sunfish-native-wifi/smmu-install-probe/`. Its actual CapDL allocation,
page tables, frame rights, guard holes, IRQ destinations and reserved-memory
exclusions are audited. Mutation checks reject writable foreign contexts,
cached page tables, executable data, extra frame aliases, SMC authority and
incorrect IRQ bindings.

The image has one PD, no SMC capability, no firmware/EFS payload and no modem,
WLAN, IPA or UFS register mappings. It may write one previously invalid WLAN
route and measured candidate context bank 4 only. Other banks, including the retained
UFS bank 2, are mapped read-only to reject a
live ASID collision. It preserves the context's observed ACTLR except for the pinned Linux
MMU-500 CPRE workaround described below, and refuses
any existing route reference to the candidate bank, including an invalid SMR retaining a
translation reference. The new page descriptors are non-global and refer
only to the retained private data page. It does not modify global SCR0,
reset the SMMU, clear faults, invoke ATS, or initiate DMA.

The experiment requires a full power-off before temporary `fastboot boot`,
with the product verified as `sunfish`. `INSTALL READBACK PASS` would establish
only that this narrow install sequence and readback completed. A fault,
readback mismatch, stale preimage or timeout retains the mappings and stops
further writes. No automatic rollback or register retry is performed. Restart
after recording the result; an install receipt is not a production WLAN
readiness token.

## Validation

`python3 platforms/sunfish/test-modem-qmi.py` passed after these changes,
including the new RAM mode and native adapter suites, existing owner/pool
composition and AArch64 no_std checks. The actual experiment also cross-built
for the Sunfish Microkit target and passed the CapDL audit plus seven mutation
checks. The first cold temporary boot on 2026-10-25 displayed ID0 `4c017e4f`, ID1
`6000003d`, and slot count `4f`. The status row overlapped because the probe
used glyph-only painting for successive labels. No selected-route or write
checkpoint was visible, so installation is not established. The revised image
clears each replaced row and reports the exact route-planner error, free-slot
count, overlapping WLAN match and retained bank-2 references. The revised
image rebuilt and passed the same capability audit and seven mutation checks;
the corrected screen confirmed a pre-write guard stop:
`NoVerifiedFreeContextBank` (`0c`). It found 73 free slots, no WLAN match, and
one reference to bank 2: slot 4, SMR `800f0300`, S2CR `00000002`. This is the
retained ABL UFS match (`300/00f`); it must not be replaced.

The subsequent profile considered bank 3, never assumed it was free,
and maps the UFS bank read-only. The adapter checks every route reference,
SCTLR.M, faults, table identity and active ASIDs before an install. It checks
ID2's 4-KiB support and address-width limits before touching table frames.
The new native suite passed 87 tests, including preservation of that exact
UFS route, its CBAR and every bank-2 register byte in the RAM-backed MMIO
simulation. The bank-3 image cross-built and passed the capability audit and
seven mutation checks. The cold device run also stopped before installation:
bank 3 is referenced by slot 5, SMR `84400800`, S2CR `00000003`. The pinned
DTS identifies that `800/440` match as MDSS/display. ID2 read `00005555`.
Both the UFS and display routes remain intact.

The next check is a read-only bank census, built with:

```sh
sh platforms/sunfish/build-smmu-bank-census.sh
```

It reuses the existing read-only C probe with `SUNFISH_BANK_CENSUS` and a
large-text report. It reads all implemented SMR/S2CR pairs, including invalid
SMRs, and all 61 banks. Reference, active, fault and ASID-conflict bitmaps
exclude occupied candidates. Bank 0 is excluded from the native nonzero-ASID
policy. A second read of the route and context fields detects changes during
capture; instability, global faults or CLIENTPD produces no eligible bank.
`FIRST BANK` is only a planning observation, never ownership or DMA evidence;
a later install must repeat all preconditions in its own boot.

The generated census CapDL model is audited for the exact 256 read-only,
uncached, non-executable register pages, with no frame aliases, SMC, IRQ or
application IPC authority. Four mutation checks rejected writable register
access, an extra alias, SMC and IRQ authority. The AArch64 census build and
legacy read-mode compile passed. The hardware census result is recorded below.

Private EFS snapshots and their device-bound manifest are retained under
`.cache/sunfish-native-wifi/private/` with restrictive permissions. They are
not repository assets. The originals are immutable inputs; native modem writes
will use separate RAM frames, matching the successful Linux experiment.

## Stable cold handoff census and bank-4 experiment

The read-only census on 2026-10-25 was stable across its two reads. SCR0 was
`00280406`, global FSR zero, reference mask `000000000000000f`, active, fault
and ASID-conflict masks all zero. Candidate mask was `1ffffffffffffff0`;
bank 4 was the first candidate. Its raw state was SCTLR `2a944312` (M clear),
ACTLR `3`, FSR zero, CBAR zero, TTBR0 high `5b919038`, TTBR1 high `e58ea767`.
These stale TTBR values are not mappings to reuse. The installer replaces them
with its own retained table frames after checking the fresh preimage.

The pinned kernel enables `CONFIG_ARM_SMMU_MMU_500_CPRE_ERRATA=y`. Its
[`arm_mmu500_reset`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/iommu/arm/arm-smmu/arm-smmu-impl.c)
clears ACTLR.CPRE (bit 1) for MMU-500 prefetch errata. The Linux-reference plan
now applies that operation only to the selected unused bank, preserving other
ACTLR bits; the measured value `3` becomes `1`. It does not perform Linux's
global reset or modify global ACR/cache-lock state. A blocked ACTLR write
stops on readback before translation is enabled. ID7 and ACR are displayed
for diagnosis if that happens.

The current image grants write access to bank 4 only; banks 0–3 and all other
banks remain read-only. Its context fault IRQ is GIC 132 and global fault IRQ
97. The native suite passed 88 tests, including replacement of the measured
stale state, CPRE handling, and preservation of the retained UFS and display
routes, CBARs and banks. All 99 register-plan tests and the rebuilt image's
capability audit plus seven mutation checks passed. The bank-4 hardware
install reached slot 6/bank 4. Three writes were confirmed before TCR2
readback stopped it at step 3: expected `00038012`, actual `00038072`.
Translation was still disabled and the WLAN route unpublished. ID7 was `24`,
ACR `1c`; ACR.CACHE_LOCK was clear. The raw bank state changed across cold
boots, so the installer correctly used a new preimage, not the census bytes.

The `60` difference is TBI0/TBI1. ARM IHI0062D.c §16.5.39
([page 338](https://documentation-service.arm.com/static/5f900d34f86e16515cdc08fb#page=338))
limits those fields to address-translation operations. The adapter has no ATS
path and now explicitly requires ID0.ATOSNS. Readback ignores only those two
bits, preserving strict address-size, ASID and other control checks. The actual
hardware pair and negative control cases pass in the 89-test native suite.
The rebuilt image passed the capability audit and seven mutation checks.

The next cold temporary boot, image SHA-256
`4e17ea35bf87c164790b52c7200db4c77a4588204043e39d066eade7a3360e20`,
displayed **`INSTALL READBACK PASS`**. The final readback was slot 6,
SMR `80010240`, S2CR `00000004`, SCTLR `00000067`, ACTLR `00000001`,
global FSR zero and context FSR `00000400` (format only). The bounded TLB
wait completed after one read. The final write checkpoint was step `11`
(hexadecimal), register `818`, the selected SMR. The receipt's write-count
label exceeded the display helper's length bound and was not rendered; its
count is not a photographed result. No fault-stop banner appeared.

The photo and exact tested source/artifacts are retained in
`.cache/sunfish-native-wifi/device-runs/smmu-install-bank4-tbi-20261025T224808Z/`.
This proves completion of this specific native installation/readback sequence,
not successful device translation, DMA, modem startup or Wi-Fi traffic.
The experiment retains the installed private domain until restart. The user
returned to LineageOS; ADB confirmed the same Sunfish serial and Android
`system_server` under a new boot ID before further work.

## Private boot-input provider

The `sunfish-native-wifi-private-inputs` feature provides
`sunfish_private_boot_inputs::Inputs::prepare`. It checks a separately selected
device binding, the complete exact-reference WLAN image, and all four EFS
sources before changing any shadow. It returns real `Storage<RamShadow>` and
`AuthorizedWlanImage` providers. The firmware remains immutably borrowed for
TFTP; EFS writes and sync affect separate, exclusively borrowed RAM only.
This is the owner's authorized private device experiment. No firmware or EFS
bytes are repository/distribution assets.

`platforms/sunfish/prepare-native-boot-inputs.py` requires the independently
checked serial and device-binding digest. It validates all source hashes and
exact partition geometry, then creates a new mode-0700 directory containing
mode-0600 payloads, a manifest and a generated Rust include module. That
module takes the native generation explicitly; snapshot Linux boot ID is
provenance, not a future native generation. These are frozen comparison
snapshots, not proof of the phone's latest EFS contents. Temporary boot must
still verify the physical phone's serial/product; neither the generator nor
the Rust provider detects hardware identity.

`platforms/sunfish/verify-native-boot-inputs.py --inputs <private-directory>`
compiled that generated module with the real Rust provider and the real
private payloads. All four EFS copies passed read/write/sync checks. The
routed TFTP RRQ/DATA/ACK transfer completed all 3,751,004 WLAN bytes in 7,327
DATA packets, comparing every returned byte. Disk source hashes remained
unchanged. Build artifacts containing private bytes stay inside that private
directory. This host check performs no USB, network, SCM or MMIO operation.

Four Rust tests cover provider composition, wrong-device/firmware rejection,
and rejecting corruption of the last EFS source before changing the first
shadow. Six generator tests cover hashes, geometry/order, identity, volatile
policy, private output and preservation of an existing destination. The
feature also passed the AArch64 Sunfish Microkit no_std library check.
The library then cross-built successfully for that target. The full
`test-modem-qmi.py` regression passed, including its safe-core and native
AArch64 compilation checks. Its 91 Rust summary records contain 3,413 passing
test invocations, including shared modules repeated across harnesses. The
offline storage crate explicitly recognizes the shared protocol's excluded
private-input feature so strict configuration linting remains enabled.

The default native modem entry still does not instantiate these providers.
It needs the real SCM pool lifecycle and IPA hardware backend first. In the
pinned reference, `ipa_firmware_load` loads the DT-selected IPA image through
`qcom_mdt_load` and calls PAS authentication/reset for peripheral 15 before
`ipa_setup`; setup initializes GSI, enables the command endpoint, initializes
memory/tables, enables the exception endpoint and publishes IPA QMI. The
existing `sunfish_ipa_post_init` is explicitly a downstream Lineage profile,
so its readiness cannot substitute for executing that Linux-reference path.
The successful one-page WLAN SMMU installation does not establish IPA DMA or
either subsystem's firmware readiness.

## Integrated native prestart entry

The opt-in `rust_sunfish_native_startup_preflight` entry instantiates the
verified EFS RAM-shadow storage and retained WLAN TFTP server on device. The
four shadow runs are cached CPU-owned RW/NX RAM with unmapped guard pages;
they are not SCM buffers or DMA mappings. The entry then resolves all eleven
selected BCM records from a twice-checked live CMD_DB and invokes
`sunfish_ipa_linux_power::PowerOnce` through the concrete native Apps DRV2
controller. Neither RMTFS nor TFTP is advertised to a modem yet.

The source audit independently resolves the three ICC routes from the exact
working kernel graph, including node widths/channels and BCM membership.
Its IPA v4.2 profile uses memory average/peak 80/465 MB/s, imem peak
68.570 MB/s, config peak 30 MB/s, and a 100 MHz core clock. In particular,
the IP0 clock unit is CMD_DB kHz multiplied by 1000 before rate division,
and the mainline ACV enable mask is bit 3. These differ from treating IP0 as
a fourth downstream interconnect path or inheriting the legacy ACV mask.

This is deliberately a one-shot active-floor experiment. Two equal resource
read passes are required before planning any resource write. It retains each
observed numeric component, ORs ACV mask bits, applies each interconnect floor
as a committed single-resource TCS transaction, and enables IP0 last. Each
transaction requires command/IRQ completion, verified cleanup and unchanged
full DRV2 programming. A subsequent resource read must meet the floor.
Any failure ends the attempt; the entry retains partial floors until reboot.
It never retries a vote or attempts automatic unvote/rollback.

The baseline reads are aggregate hardware observations, not the previous AP
vote or a complete client ledger. Consequently this floor coordinator is
not a runtime-PM implementation or the ordinary per-client Linux bandwidth
provider. It cannot mint `sunfish_ipa_post_init::Ready`, authorize modem
startup, or establish DMA correctness.

After all eleven floors complete, the entry reads two samples of IPA
FLAVOR_0/SHARED_MEM_SIZE and GSI STATUS/HW_PARAM_2. The corrected physical
apertures are IPA `01e40000` and **GSI `01e04000`**, matching the working
kernel's SM7150 DT. IPA v4.2 selects the `gsi_reg-v4.0.c` table, with EE0
STATUS at offset `1f000` and HW_PARAM_2 at `1f040`. The older downstream
probe's `01e44000` GSI aperture must not be reused for this reference.
Both apertures are mapped read-only in this image.

Build with an already verified private input directory:

```sh
SUNFISH_PRIVATE_INPUT_DIR=/absolute/private/prepared-inputs \
  sh platforms/sunfish/build-native-startup-preflight.sh
```

The resulting image and all compilation outputs containing private payloads
remain under `.cache/sunfish-native-wifi/private/`. The build re-verifies the
private providers, source-derived profile and controller tests. The actual
CapDL audit verifies 1,636 selected frames, 6,422,528 shadow bytes, eight
unmapped shadow guards, exact cache attributes/rights, no frame aliases,
reserved-memory exclusions and no SMC, IRQ or IPC authority. Six mutations
of XML/actual CapDL rights, executable and cache attributes were rejected.
The image therefore cannot call PAS, access modem/WLAN/SMMU registers, start
GSI DMA, or flash a partition.

The first image is 27,639,808 bytes, SHA-256
`854706bf0bdfda84d1ac0fc8580ba85f6909087e6e4b69926cbd1c9c24a13dc2`.
All 59 tests in the new controller/profile harness and 25 legacy BCM adapter
tests passed; the full modem regression and AArch64 image build passed.
The user completed the cold Fastboot procedure. Fastboot accepted and started
this image (3.328 seconds total). IMG_9075 shows **PRESTART SNAPSHOT PASS**:
all eleven requests submitted and completed (`0x7ff`), two equal IPA/GSI
samples, IPA FLAVOR `08090811`, SHARED_MEM_SIZE `00000415`, GSI STATUS
`00000000` and HW_PARAM_2 `e4046b59`. IP0 readback was `200009c4`; ACV
retained bit 3 (`20000008`). GSI is still disabled, as expected before
firmware startup. These observations establish power/register access only.
The user then returned to LineageOS; physical ADB verified a fresh Android
boot ID `e2e01398-f6c0-4ea7-a29d-8d2f54b37554` and a running system_server.
The temporary floor experiment ended with that reboot. The private run
archive contains both original photo and transcription with hashes.
The next required implementation stages remain IPA configuration/PAS/GSI,
native SCM pool lifecycle and modem host services before MPSS start.


## Linux primitive IPA configuration entry

The opt-in `sunfish-native-startup-ipa-config` feature extends the same real
private-provider/power entry with the nine `ipa_hardware_config()` writes
from the working Linux commit. It does not use the downstream hardware-init
profile. The source audit independently derives offsets and field encodings
from `ipa_reg-v4.2.c`, values from `ipa_data-v4.2.c`, and the function order
and version branches from `ipa_main.c`. Four source mutations are rejected.

The writes, in source order, are:

| Register | IPA offset | Operation |
| --- | --- | --- |
| BCR | `1d0` | Write zero |
| TX_CFG | `1fc` | Clear PA_MASK_EN, bit 12; retain other bits |
| CLKON_CFG | `044` | Write `30000000` |
| COMP_CFG | `03c` | OR `8180`; retain other bits |
| QSB_MAX_WRITES | `074` | Write 8 |
| QSB_MAX_READS | `078` | Write 12; read-beat limit zero |
| COUNTER_CFG | `1f0` | Write `f0`: 500 us at 32 kHz gives 15 in bits 8:4 |
| FILT_ROUT_HASH_EN | `148` | Write zero; v4.2 has no hashed tables |
| IDLE_INDICATION_CFG | `240` | Write `100`; debounce 256, constant-non-idle disabled |

`ConfigureOnce` requires a completed power report in the entry and two equal
snapshots of all nine registers plus FLAVOR, SHARED_MEM_SIZE, aggregation
activity, IPA IRQ enable and GSI status/parameters. This experiment accepts
only the exact measured Sunfish flavor/shared-memory/GSI-parameter values,
with no aggregation, enabled IPA interrupt or enabled GSI. Before every
write the complete snapshot must match the expected state; after each write
and at the end, full-word readback and all guards must match. An error ends
the attempt permanently, retains partial configuration and requires cold
reboot before another attempt. No register is retried or automatically undone.
The screen checkpoints the initial register reads and each write.

The `hardware-config` build selector keeps the earlier read-only profile
and its output separate. Its actual CapDL grants IPA page `01e40000` RW,
with the remaining six IPA pages and all GSI pages RO. This is page-level
authority: the concrete adapter's closed register enum limits the writes to
the nine audited offsets. IRQ/PAS/SMC/DMA/endpoint-start code is absent from
this stage. The actual-capability audit rejects eight XML/CapDL mutations;
the earlier fully read-only profile also passes re-audit with six rejections.

Eight controller tests cover exact source order and values, RMW preservation,
incomplete power, changed/active hardware, every read failure, uncertain or
non-sticking writes, and concurrent changes between writes. The full modem
regression and the actual AArch64 Microkit image build passed. Build with:

```sh
SUNFISH_PRIVATE_INPUT_DIR=/absolute/private/prepared-inputs \
  sh platforms/sunfish/build-native-startup-preflight.sh hardware-config
```

The prepared image SHA-256 is
`606d549b05dd0b10623d99c0f6b4f69993ddbe4df4a9e522e06ac2faed900465`
(27,639,808 bytes). After the user confirmed cold Fastboot, the physical
Sunfish identity, unlocked state, slot and all recorded source/artifact
hashes were checked. Fastboot accepted and started the archived image
(3.200 seconds total). IMG_9079 shows **IPA HW CONFIG PASS**, with
submitted/completed masks `1ff`. Readback values were BCR `0`, TX `400`,
CLKON `30000000`, COMP `f81f8`, QSB writes/reads `8`/`c`, counter `f0`,
hash enable `0` and idle config `100`. Aggregation, IPA IRQ enable and GSI
status all remained zero; flavor/shared-memory/GSI parameters remained
unchanged. The original photo and transcription are hashed in the private
archive. A return to LineageOS has been requested to clear the temporary
configuration. Physical ADB then confirmed LineageOS with a fresh boot ID
`b5d973d2-7c3d-4d62-bfa6-df36fe6197e4` and system_server running; the
temporary experiment has ended. The private run is
`ipa-hw-config-20261026T000744Z` (host UTC). This executes only the first
part of `ipa_config`, and cannot establish IPA readiness.
The remaining stages include the SMMU/DMA-backed allocations and memory
configuration, real interrupts/uC, endpoints/tables/resource limits, PAS
firmware authentication/reset and GSI setup; modem startup still follows
those dependencies and the concrete native SCM/host-service lifecycle.


A separate offline check of the exact Linux monolithic firmware inputs
confirmed the next loader dependency: IPA has three loadable segments
(12,488 file bytes, aligned extent `5000` inside its `10000` reservation),
and its compact PAS metadata is 7,020 bytes. Modem has 29 loadable segments
(68,393,962 file bytes, extent `9800000`) and compact metadata of 9,100 bytes.
The SM7150 compatible selects `mpss_resource_init`, PAS peripheral 4;
IPA uses peripheral 15. One modem BSS-only header has a file offset beyond
EOF. The pinned loader correctly ignores the offset when file size is zero
and zeroes the memory size. A native packed-image adapter must preserve that
behavior rather than treating this valid BSS as a truncated segment. These
are offline range/metadata checks, not signature authentication or a native
loader. No PAS operation was added to the primitive-config image.


## Packed Linux firmware source adapters

`qcom_mdt_packed` now borrows the exact monolithic MBN layout used by the
working Linux boot. It validates every nonempty PHDR file range before
exposing a segment, tolerates BSS-only offsets beyond EOF, checks ELF/section
bounds and rejects ambiguous hash headers or overlapping target ranges.
Metadata copying produces the header immediately followed by hash bytes,
without file padding, and leaves extra output bytes untouched. The compact
PAS payload is not reparsed as a monolithic file: its PHDR offsets still
refer to the original file. A header-plus-hash-only input retains Linux's
compact-hash shortcut. Split files remain on the separate existing path.

`sunfish_linux_firmware::Ipa` connects this view to the existing real IPA
placement and staging code. `modem_plan` constructs the existing exact
29-segment modem plan using borrowed file slices and temporary compact
metadata, retaining its MEM_SETUP-gated target-staging lifecycle. Neither
adapter establishes firmware identity, signatures, target ownership, PAS
success or readiness. The caller must bind the firmware identity separately.

The private host verifier pins both reference firmware hashes and the exact
Linux source archive. It ran the real Rust adapters, compared every metadata
byte against an independent Python extraction, and compared all 65,536 bytes
of IPA staging in an ordinary CPU Vec: payloads, zero tails and untouched
`a5` gaps. IPA metadata is 7,020 bytes, three segments total 12,488 file bytes
and 4,344 BSS bytes. Modem metadata is 9,100 bytes; its existing plan accepts
all 29 segments, including the last BSS-only header with an offset beyond
EOF. Original source files remained unchanged. The harness has 41 passing
tests including shared parser/legacy lifecycle tests, and the final library
cross-build passed for the native AArch64 target. No secure call was made or
hardware lifecycle token fabricated for this host check.

## Initial IPA DMA-domain attempt (masked pair; stopped)

The first opt-in image used `sunfish-smmu-ipa-install-probe`. The source audit
confirms that the working SM7150 IPA node binds exactly SIDs `520` and `522`
(hex), each with DT mask zero, to one Apps SMMU domain. The native route
combines precisely these two IDs using SMR ID `520`, mask `2` (expected SMR
`80020520`). It excludes `521` and `523`; the exhaustive 15-bit SID test
checks every identity. This is separate from the legacy single-SID IPA
planner and the already tested WLAN alias pair.

The complete route capture rejects overlap with either IPA SID and excludes
any candidate context referenced by an S2CR, including an invalid SMR's
retained translation reference. The same concrete bank-4 adapter and
Linux-reference context plan are used. Its fresh bank/ASID/fault checks,
exact ordered writes, readback, real context/global fault IRQ bindings and
terminal-error behavior remain in force. Retained UFS/display routes and
banks are not repurposed. There are no IPA/GSI/modem/WLAN register mappings
or SMC capability in this image.

The owned pool contains 32 physically contiguous Microkit allocator frames
(128 KiB). Each CPU word is zeroed and read back using aligned volatile
operations before any SMMU write. Four separately owned table frames map
these pages RW/NX at IOVA `80000000` through `8001ffff`, with all other leaf
entries invalid. This establishes only a bounded mapping experiment: it has
no GSI descriptors, actual DMA/ATS transaction or production DMA/Ready token.
It does not yet supply the separate IMEM/SMEM mappings, full RX buffer pool
or later GSI runtime required by Linux.

The actual CapDL audit checks the 32-frame physical contiguity, exact CPU
mappings, unmapped guards, no aliases or reserved-memory overlap, no other
writable context banks and exactly fault IRQs 97/132. Nine mutations are
rejected, including a physically discontiguous pool. The original one-page
system still passes its audit. The RAM MMIO simulator exercises the actual
adapter, table staging and register executor with the two-SID/32-page plan,
checks every leaf including invalid neighbors, and preserves the observed
ABL UFS/display routes. All 92 tests in that harness pass. The full modem
regression passes with 94 Rust summary records / 3,535 passing invocations
(including shared modules repeated by different harnesses), plus safe/native
AArch64 compilation checks.

Build this separate image with:

```sh
sh platforms/sunfish/build-smmu-install-probe.sh ipa
```

The prepared image is 18,690,048 bytes, SHA-256
`48965cb3112650910f2c69b9d409dc5b733d463feb65cf3e9083946d1929e81f`.
The user confirmed cold Fastboot, and physical identity/unlocked/slot plus
all recorded source/artifact hashes were checked. Fastboot accepted and
started the archived image (2.655 seconds total). IMG_9083 shows
**INSTALL STOPPED / READBACK ERROR** at step `11` (hex), SMR slot 6
offset `818`: expected `80020520`, actual `0`. The candidate bank was 4;
its preimage was SCTLR `2bbc4312` (M clear), ACTLR `3`, FSR `0`. The
context/route sequence is a partial attempt, not an installed DMA domain.
No automatic retry follows. Returning to LineageOS was requested to clear
the experiment before any subsequent cold test. Physical ADB then verified
a fresh LineageOS boot (`baea813d-8743-4bcb-851a-1ff78ebdbdac`) with
system_server running. The original photo and
transcription are hashed in the private run `ipa-domain-20261026T005032Z`
(host UTC).
A successful context/route readback will still require subsequent real DMA
and complete pre-PAS configuration before firmware or modem readiness can
be established.


## Revised IPA domain: two exact Linux DT matches

The failed merged `520/2` route was a native optimization, not Linux's actual
DT representation. Although the 15-bit matching model is equivalent, its
hardware acceptance was not established. The revision removes that
optimization: `plan_ipa_linux_exact_routes_from_snapshot` selects two distinct
inactive slots for exact SID `520/0` and `522/0`, with one shared bank 4.
It rejects overlap with either SID, too few free slots, retained S2CR
references and mismatched context/preimage bindings. The masked planner is
retained only for models/failure analysis and is no longer used by the native
IPA entry. The rejected write alone does not identify whether the cause was
the mask, per-stream firmware handling or another initialization dependency.

The pinned Linux driver copies each fwspec SID/mask into its own allocated
SME; `arm_smmu_write_sme` writes S2CR before SMR, and device attachment applies
one context to the device's entries. The audit now checks those source paths
and the native two-exact-entry selection. Linux's overall probe also has an
earlier initial-domain stage; this bounded experiment does not reproduce all
global/bypass initialization and cannot establish that those steps are
unnecessary. It retains the guarded translation-target-before-valid ordering
and touches only the selected context and two inactive slots.

`SmmuInstallPlan` keeps the legacy 18-action sequence and adds two actions for
the exact peer, with bounded capacity 20. Both raw route preimages and the
context preimage are checked before the first register write; global fault
status is checked last. The bank, four table frames, TLB synchronization and
fault binding are configured once. The expected result is 17 confirmed
register writes with SMRs `80000520` and `80000522`, both S2CRs `4`.

The concrete native adapter restricts writes to those two selected slots and
bank 4. Every SMR bit must read back exactly; zero is still a terminal error.
An uncertain peer write quarantines the complete attempt, retaining any
already installed primary entry until reboot. The display includes peer
slot/SMR/S2CR on success and current global/context fault status on failure.
No new secure-call, bypass, global reset, clock, DMA or radio authority was
added. The same 32-page private pool and read-only retained banks remain.

The native simulator harness passes 95 tests; the register/executor harness
passes 104. New checks cover distinct exact entries with one context,
requiring two free slots, rejecting a stale peer before writes, refusing a
masked primary/different bank/duplicate peer, and quarantining either
uncertain peer write with no replay. The full modem regression and AArch64
image build passed; all nine actual-capability audit mutations are rejected.

`build-smmu-install-probe.sh ipa` now produces a separate private
`smmu-ipa-exact-install-probe` artifact, retaining the failed image and run.
The revised image is 18,694,144 bytes, SHA-256
`a3a8ff16f0edb5f4bc13fb4795e27425630cdfae2ccdb5467ae2c09a0c998401`.
After the user confirmed cold Fastboot, the physical device was verified as
`sunfish`, unlocked, slot `a`, serial `02281FQC200058`. The image, selected
sources, manifest and audit artifacts were frozen in private run
`ipa-domain-exact-20261026T010947Z`. One temporary `fastboot boot` was accepted
(sending 0.566 s, booting 2.235 s, total 2.804 s); no partition was flashed.
IMG_9088 confirms **IPA DOMAIN INSTALL PASS**: slots 6 and 7 read back
SMRs `80000520` and `80000522`, both S2CRs `4`; bank 4 SCTLR is `67`,
ACTLR `1`, and TLB synchronization completed after one status read. The
32-page private pool is 128 KiB at PA `802dd000`. Global FSR is `0`;
context FSR `400` contains only FORMAT bits [10:9] (format 2), not a
defined fault flag, consistent with the pinned Linux fault mask. The final
action index is `13` (hex), peer SMR register `81c`. Original and converted
photos plus the transcription are hashed in the private run. The exact-route
revision succeeds where the prior masked attempt stopped; this does not
isolate every possible cause of the earlier failure. Returning to LineageOS
was requested to clear the experiment before the next stage. Physical ADB
then verified a fresh LineageOS boot (`7976e4a0-ce56-4a83-a266-a002654a36d1`) with
system_server running. Actual DMA translation remains unproven; no DMA/ATS, PAS/SMC, modem/WLAN or flash was
performed.


## Integrated IPA pre-DMA stage

The standalone two-route installation is now combined with the prior proven
power-floor and primitive IPA configuration stages in one opt-in image.
Private, verified EFS RAM shadows and the WLAN TFTP source remain retained
and inactive. The SMMU installer is shared as `pixel_smmu_install`; the
standalone entry delegates to the same implementation. This extraction does
not change the planned SMMU registers, preimage checks, 32-page pool, fault
handling or strict readbacks.

The combined entry calls the installer only after the real power report
completes mask `7ff` and the actual nine-write configuration report completes
mask `1ff` without error. After installation, a read-only adapter captures
all nine IPA configuration words and six guards twice, comparing both full
samples with the successful configuration report. A changed register, active
GSI/aggregation/IRQ state or I/O error stops without replaying writes.
The final headline `INTEGRATED PRE DMA PASS` requires both route readbacks
and that retained-configuration check. It is an experimental milestone;
no production IPA/DMA Ready capability is constructed.

The image directly audits the union of capabilities against actual CapDL:
1,735 selected frames, 18 unmapped guards, cached RW/NX EFS shadows,
uncached RW/NX SMMU tables/private pages, other context banks read-only,
only IPA page 0 writable, all GSI read-only, and exactly global/context
fault IRQs 97/132. There is no SMC or IPC capability. Eighteen mutations are
rejected, including incorrect physical-symbol injection, noncontiguous
private pages and widened retained-bank/GSI rights. Legacy preflight and
configuration audits still reject their six/eight mutations respectively.
The power harness passes 59 tests, configuration/retained-check harness 10,
and SMMU/native shared harness 95; integrated, standalone SMMU, preflight
and primitive-configuration AArch64 builds all pass.

Build with `SUNFISH_PRIVATE_INPUT_DIR` selecting the existing prepared private
inputs and `sh platforms/sunfish/build-native-startup-preflight.sh dma-domain`.
The private image is 27,684,864 bytes, SHA-256
`a63f6a96a3ffd9c3a7e3180cfd3ced256d0911996e73fc81dc7a50ba50765454`.
Its source/artifact hashes and validation results are in the private manifest.
After the standalone cleanup was verified in LineageOS, the user confirmed
a new full power-off and cold Fastboot. Physical Sunfish identity, unlocked
state and slot `a`, plus every recorded source/artifact/private-input hash,
were checked. The inputs and image were frozen in private run
`ipa-integrated-domain-20261026T012605Z` (host UTC). One temporary
`fastboot boot` was accepted: sending 0.764 s, booting 2.421 s, total
3.190 s. IMG_9092 confirms **INTEGRATED PRE DMA PASS**: power completion
mask `7ff`, primitive configuration mask `1ff`, primary/peer SMRs `80000520`
and `80000522` in slots 6/7, both S2CRs `4`, bank-4 SCTLR `67` and ACTLR `1`.
The two complete post-install IPA/GSI snapshots matched the successful
configuration report. Global FSR is `0`; context FSR `400` is format-only.
The 32-page (128 KiB) private pool starts at PA `80d37000`. The original
photo, converted photo and transcription are hashed in the private run.
Returning to LineageOS was requested to clear the experiment. Physical ADB
verified fresh boot `5c251045-73dd-4ec0-b761-e1b252f3f391` with system_server
running; cleanup is complete. No DMA/ATS, remaining IPA
memory/interrupt/endpoint setup, PAS/SMC or modem/WLAN engine startup
is performed by this stage; actual DMA translation remains unproven.


## IPA local-memory canary stage

`sunfish_ipa_linux_memory` implements the bounded canary portion of the
working kernel's `ipa_mem_config()`. The pinned source's 18 memory entries
(including the zero-sized end marker) define 25 canary words. The planner
uses Linux's region order and pre-decrement order within each pair, placing
`deadbeef` only at those 25 aligned words. The last is offset `1ffc`.
No region payload is cleared. An independent source audit derives every
entry, canary location/value, SRAM aperture, SHARED_MEM_SIZE field and
8-byte unit from the exact working source archive; four source mutations
are rejected.

The measured SHARED_MEM_SIZE is `415`: offset zero, reported size `20a8`,
larger than the DT's actual mapped `2000`. The experiment binds to that
exact measured profile and caps all accesses to `2000`, matching Linux's
retained mapped limit. Different geometry fails before I/O. The new opt-in
image adds exactly two uncached RW/NX SRAM frames at PA `01e47000` / VA
`44200000`. Its actual CapDL audit validates 1,737 frames, all 18 prior
private guards and existing rights/IRQ restrictions; 22 mutations fail.

The real integrated pre-DMA sequence must succeed before the new coordinator
runs. Before every canary write, it checks the retained domain's current
fault status and two complete IPA/GSI configuration snapshots. Each write
is recorded as attempted before MMIO and must read back exactly. Two full
canary samples plus retained domain/configuration checks follow the final
write. A changed guard, fault, uncertain write or failed read consumes the
attempt; no repair, replay or rollback is attempted. Mappings, partial
writes and floors remain retained until reboot. The display clears prior
stage fields and identifies the canary index/offset and error on failure.
The success headline is `IPA CANARY SETUP PASS`, with completion mask
`01ffffff`, 25 verified words, last value `deadbeef`, and the SRAM geometry.

Ten new host tests cover independent exact offsets, preservation of every
non-canary word, geometry bounds, every uncertain write/non-sticking write,
all 75 canary-read failures, all 810 guard-read failures, each domain/guard
boundary, changed earlier canaries in either final sample, and no I/O on
retry. Power/configuration/SMMU harnesses pass 59/10/95 tests. The full modem
regression passes 95 Rust summary records and 3,553 passing invocations
(including repeated shared modules), with AArch64 core/native compilation.
The actual local-memory image and previous pre-DMA feature both cross-build.
Legacy capability profiles retain their 6/8/18 mutation rejections.

Build with the existing private inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh local-memory`.
The private image is 27,688,960 bytes, SHA-256
`f9b690897ef07c4def8ef093f6bd103a9215fe62a14a0ebd364588cff53f5bed`.
After the verified LineageOS cleanup following IMG_9092, the user confirmed
a new full power-off and cold Fastboot. Physical Sunfish identity, unlocked
state and slot `a`, plus every selected source/artifact/private-input hash,
were checked. The image and inputs were frozen in private run
`ipa-local-memory-20261026T014913Z` (host UTC). One temporary `fastboot boot`
was accepted: sending 0.764 s, booting 2.412 s, total 3.182 s. The settled
screen IMG_9099 confirms **IPA CANARY SETUP PASS**: attempted/completed
masks `01ffffff`, 25 verified words (`19` hex), final index `18`, offset
`1ffc`, and last value `deadbeef`. Configuration remained unchanged; global
FSR is `0` and context FSR `400` is format-only. Geometry is exactly the
expected reported `20a8`, offset `0`, and usable `2000` bytes. The original
photo, conversion and transcription are hashed in the private run. Returning
to LineageOS was requested to clear the experiment. Physical ADB verified
fresh boot `1afefd93-98c8-4cd0-92cb-07ce879a925c` with system_server running;
cleanup is complete. This remains a partial `ipa_mem_config()` step:
there is no DMA zero-buffer allocator, IMEM/SMEM mapping, `ipa_mem_setup()`
command, PAS/SMC, modem/WLAN startup or production Ready capability. Actual
DMA translation remains unproven.


## Retained Linux-profile IPA buffers (CPU preparation passed)

`sunfish_ipa_linux_buffers` now prepares real backing memory through an owned
backend, rather than reusing the legacy downstream ring model. The four AP
channels match the working mainline data exactly: command TX channel 1 / EP6
(256 TREs, 256 events, TLV20), LAN RX channel 2 / EP8 (256/256/TLV6), modem TX
channel 0 / EP1 (512/512/TLV8), modem RX channel 3 / EP9 (256/256/TLV6).
Each ring element is 16 bytes and each ring has size-aligned IOVA. No hardware
event IDs are assigned at this stage.

The command payload union is independently derived as 24 bytes from its
fixed-width C declarations, rounded to a 32-byte pool stride. Linux's command
TRE limit is 256-(20-1)=237; `(237+20-1)*32` gives an 8-KiB pool. The DMA-zero
source is 8 KiB, bound to the comparison kernel's verified 4-KiB page setting.
The empty table has a zero rule, filter bitmap `24` (EP1/EP4 shifted left for
IPA v4.2), and 15 pointers to that same zero rule. Its payload is 136 bytes,
reserved in one 4-KiB page at IOVA `80002000` with rule alignment satisfied.
No immediate commands or DMA descriptors are submitted.

The eight ring buffers, command pool, zero buffer and empty table are 11
disjoint buffers totaling 60 KiB. Alignment consumes a 64-KiB span, leaving
64 KiB of the already installed 128-KiB private window unused. The pure
planner uses IOVA alignment; physical pages need only the established 4-KiB
frame alignment. RX packet backing, CPU transaction/completion bookkeeping,
command-payload reuse and runtime GSI ownership are separate future work.

After the entire prior sequence and 25-canary verification succeed, the
retained SMMU Probe allows one CPU-writer claim of its exact private mapping.
`sunfish_ipa_linux_buffers_native::NativeRam` validates the 32 contiguous
physical frames and bounds every aligned scalar volatile 32-bit access.
The actual CapDL audit establishes uncached RW/NX authority, retention and
lack of aliases; neither a caller boolean nor the allocator establishes
those rights. There are no additional mappings or IRQs: the new image retains
the same 1,737 selected frames, 18 private guards and 22 mutation rejections.

The pool initializes only its reserved regions, then verifies all 60 KiB in
two complete passes, including every table pointer and zero-filled ring and
payload byte. Before each initialization/verification region and at the end,
the real domain/fault state and two IPA/GSI snapshots must remain unchanged.
Uncertain writes, readback failures or guard changes quarantine the pool,
withhold prepared-buffer metadata, consume the attempt and retain all RAM
until reboot. A later fault IRQ also quarantines the retained owner. No
production IPA/DMA Ready capability or hardware-start authority is created.

Nine host tests cover independent exact layout/content fixtures, preservation
of padding/tail, both readback passes, buffer-edge write/read failures,
non-sticking writes, corruption between passes, all 34 domain/configuration
guard boundaries, no retry I/O, rejected frame/bounds geometry and the actual
volatile backend on host-owned test RAM. The source audit derives channel
counts, sizes, payload layout, zero source and table content from the pinned
archive and rejects five mutations. Existing harnesses pass 59/10/95/10 tests.
The complete regression passes 96 Rust summary records / 3,562 invocations
(including repeated shared modules), plus AArch64 core/native compilation.
The model core retains `forbid(unsafe_code)`; the native RAM adapter is compiled
in the separate native harness with concrete read/write wrappers. Both the
actual buffer image and prior memory feature cross-build; the prior memory
capability audit still rejects 22 mutations.

Build with the existing private inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh buffers`.
The private image is 27,697,152 bytes, SHA-256
`964a637b3801fa288cb7d708a5eec24b7dda5a7f6b1b0fd14fd600c93879c456`.
After the prior experiment's verified LineageOS cleanup, the user confirmed
a new full power-off of at least ten seconds and cold Fastboot. Physical
Sunfish identity, unlocked state and slot `a`, plus all 50 selected source,
17 artifact and two private-input hashes, were verified and frozen in private
run `ipa-buffers-20261026T023133Z` (host UTC). One temporary `fastboot boot`
was accepted: sending 0.764 s, booting 2.356 s, total 3.126 s. The settled
screen IMG_9103 confirms **IPA BUFFER PREP PASS**: 11 buffers,
initialized/verified masks `7ff` and checked bytes `f000` (60 KiB), with
configuration retained. Pool IOVA is `80000000`, physical base low word
`80d3c000`, used span `10000`, reserved bytes `f000` and remaining bytes
`10000`. Zero source and command pool are each `2000` bytes; table payload
is `88` bytes at IOVA `80002000`, with filter bitmap `24`. Global FSR is
zero; context FSR `400` is format-only, with no defined fault flags.
The original photo, PNG conversion and transcription are archived with
hashes in the private run. Physical ADB then verified fresh LineageOS boot
`3c9e4131-2276-4b53-a9c5-5191367c4fd6` with system_server running; cleanup
of the temporary IPA/SMMU setup is complete. This proves CPU preparation only; actual DMA
translation, IMEM/SMEM mapping,
remaining pre-PAS configuration, firmware/PAS and modem/WLAN startup remain
unproven.

## Linux-profile endpoint census and resource limits (passed)

`sunfish_ipa_linux_resources` implements the read-only endpoint-availability
checks and seven resource-limit writes from the same pinned Linux v4.2
profile. It validates all eight defined AP/modem endpoint directions against
the measured FLAVOR producer/consumer fields: eight TX endpoints, nine RX
endpoints starting at eight, available mask `1ffff`, defined mask `f72`.
This is availability validation, not endpoint programming or engine startup.

The source profile has exactly one source and one destination resource group.
The five source limits (min/max) are packet contexts 3/63, descriptor lists
3/3, descriptor buffers 10/10, HPS DMARs 1/1 and acknowledgements 5/5.
Destination data sectors are 3/3 and DPS DMARs 1/63. The resulting full-word
writes, in Linux source order, are `400=3f03`, `420=303`, `440=a0a`, `460=101`,
`480=505`, `500=303` and `520=3f01`; Y-group and reserved bits are zero.
No group-23/45/67 register is touched. `ipa_table_config()` performs no writes
on v4.2: both filter and route configuration return when hashing is unsupported.
The DMA table-initialization commands in `ipa_table_setup()` remain separate.

The new feature repeats the proven power, primitive configuration, exact
SMMU routes, canaries and private-buffer preparation before this subset.
It deliberately leaves IPA interrupts disabled; `ipa_interrupt_config()` and
`ipa_uc_config()` are not claimed complete. There are no new mappings or IRQs:
the seven words lie within the already writable IPA first page, and all GSI
and remaining IPA register pages stay read-only. The actual image's CapDL
audit retains 1,737 selected frames, 18 private guards and 22 mutation rejections.

The one-shot coordinator requires two equal full resource baselines. Each
write is recorded before submission and followed by strict readback of all
seven words, with two final complete samples. Each of 18 sample boundaries
checks the real retained SMMU/fault state, prepared pool and two unchanged
primitive IPA/GSI snapshots. An uncertain write or any guard/readback failure
stops without retry or rollback and quarantines the retained private pool.
A successful report grants no production Ready capability.

Seven host tests cover an independent Linux fixture, endpoint topology bounds
and directions, all 126 resource-read failures, every uncertain/non-sticking
write, all 54 domain/configuration/buffer guard failures, and changes to every
resource word at every sample after the first. The source audit independently
derives limits, register geometry/fields and endpoint directions from the
pinned archive and rejects five resource-source mutations. The actual
`sunfish-native-startup-ipa-resources` image builds and passes its capability
audit; the previous buffer feature also cross-builds. The full regression
passes 97 Rust summary records / 3,569 passing invocations (including
repeated shared modules), with AArch64 core/native compilation. Build with the existing private inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh resources`.

The private image is 27,701,248 bytes, SHA-256
`470e0e72ee51be77f3359410a6a5caf50057d3c75ed639ef4e06280ba1c782e0`.
After the verified LineageOS cleanup following IMG_9103, the user confirmed
a new full power-off for at least ten seconds and cold Fastboot. Physical
Sunfish identity, unlocked state and slot `a`, all 53 selected source hashes,
17 artifact hashes and both private-input hashes were verified and frozen in
private run `ipa-resources-20261026T024822Z` (host UTC). One temporary
`fastboot boot` was accepted: sending 0.792 s, booting 2.430 s, total 3.230 s.
The settled screen IMG_9107 confirms **IPA RESOURCE LIMITS PASS** with
submitted/completed masks `7f` and all seven expected values. Endpoint census
is exactly TX count `8`, RX count `9`, RX base `8`, available mask `1ffff`
and defined mask `f72`. Configuration and buffers are retained; global FSR
is zero and context FSR `400` is format-only, with no defined fault flags.
The original photo, PNG conversion and transcription are hashed in the private
run. Physical ADB verified fresh LineageOS boot
`f370a858-4628-4229-82a3-b500aaf948b3` with system_server running; cleanup
of the temporary setup is complete.
IMEM/SMEM mappings, interrupt/UC configuration,
PAS, GSI setup, actual DMA and modem/WLAN startup remain unproven.

## IPA microcontroller interrupt arming (stopped on initial status)

The next pre-PAS subset binds the real IPA interrupt and reproduces the
`IPA_IRQ_EN` writes from `ipa_interrupt_config()` and `ipa_uc_config()`.
The pinned DTS specifies GIC SPI 311, edge rising: physical IRQ 343,
Microkit channel 2. IPA v4.2 AP enable/status offsets are `300c`/`3008`;
UC event and response indications are bits 2 and 3. Enable values are exactly
`0`, `4`, `c` in that order. This does not ring `IPA_IRQ_UC`, start firmware,
program GSI or enable endpoint/suspend interrupts.

After every earlier proven stage succeeds, the coordinator requires two
quiet IRQ baselines and checks real retained SMMU/fault state, prepared
buffers, two primitive IPA/GSI snapshots and two full resource-limit snapshots
at each sample boundary. The primitive snapshot receipt must still describe
disabled interrupts; its read-only comparison may substitute only `0`, `4`
or `c` in the IRQ-enable guard. Every other field remains exact. Actual
`Channel::irq_ack()` succeeds before the first nonzero enable, followed by
another quiet sample. Each write receives strict readback and two complete
final samples are required. Failed arming masks the source with `IRQ_EN=0`
as containment, quarantines the private pool and never retries the arm.

The native event loop deliberately omits Microkit's default startup ACK of
all IRQs. Only the concrete setup steps arm each actual capability. Any IPA
notification before PAS is terminal: mask the source first, retain status,
read only the defined one-byte UC event/response fields when their status
bits are present, and leave the delivered kernel IRQ unacknowledged. There
is no interrupt-clear write, pending-bit drain loop or re-enable. A co-delivered
SMMU fault is handled by the existing fault latch and receives the final
screen. The event and response offsets, 20 and 12, are derived from the C
layout; accesses never widen across the neighboring reserved UC bytes.

The actual profile adds precisely one writable IPA IRQ register page and
one edge-triggered IRQ capability; it splits the old read-only tail around
that page. All GSI and other IPA tail pages stay read-only. The actual CapDL
audit checks the additional handler's destination, badge and trigger as well
as its register geometry and rights. Firmware authentication, IMEM/SMEM
allocation/mapping and GSI execution remain separate work.

Eight new host tests cover exact Linux enable order and real-binding API
ordering, nonzero initial masks/status, all 22 arm-read failures, all 44
guard failures, failed binding, uncertain/non-sticking writes, newly pending
conditions, and terminal notification containment. Notification tests check
mask-before-read ordering, no IRQ ACK/clear, byte-field selection, failure
retention and no repeated I/O. An additional configuration test exercises all
three permitted masks, every comparison-read failure, forbidden masks and a
forged enabled baseline. The source audit independently derives IRQ identity,
trigger, masks, register offsets and UC field layout and rejects five mutations.
The full regression passes 98 Rust summary records / 3,578 passing invocations
(including repeated shared modules), with AArch64 core/native compilation.
The actual IRQ image and previous resource-only feature both cross-build.
The IRQ image retains 1,737 selected frames and 18 private guards; its capability
audit rejects 31 mutations. The previous resource-only image still rejects
22 mutations under the extended validator.

Build with the existing private inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh irq`.
The private image is 27,701,248 bytes, SHA-256
`d667d59350b810b7545fce40fa7f1770a4985d66ce84f6496676d29504cf940c`.
After verified LineageOS cleanup following IMG_9107, the user confirmed a new
full power-off for at least ten seconds and cold Fastboot. Physical Sunfish
identity, unlocked state and slot `a`, all 56 selected source hashes,
23 artifact hashes and both private-input hashes were verified and frozen in
private run `ipa-irq-20261026T034317Z` (host UTC). One temporary `fastboot boot`
was accepted: sending 0.785 s, booting 2.424 s, total 3.224 s. IMG_9112 confirms
**IPA UC IRQ ARM STOP** on the first guarded sample: enable `0`, raw status
`10080`, arm submitted/completed `0`, bound `0`, local error `9` (`Pending`).
No arm write or IRQ ACK occurred. The separate failure-containment path wrote
enable `0`, read back `0` and preserved status `10080`. The original screen's
`IRQ NOTICES=1` counts that containment report, not a delivered IRQ; the revised
display separates those counts. Global FSR is zero and context FSR `400` is
format-only, with no defined SMMU fault flags. Photo, conversion and transcription
are hashed in the private run. Physical ADB then verified fresh LineageOS
boot `e77856f8-cf38-4cee-a166-616ed8c5a2f5` with system_server running;
cleanup is complete. Physical
interrupt arming/delivery and UC initialization remain unproven.

## Disabled IPA status correction (arming/readback passed)

The original guard incorrectly required the entire raw status word to be zero.
The pinned Linux ISR explicitly handles `pending & enabled`: raw status also
contains disabled indications. In this measured `10080` value, bit 7 is
`IPA_IRQ_UC_RX_CMD_Q_NOT_FULL` and bit 16 is `IPA_IRQ_BAM_GSI_IDLE`. Both are
unused by the pinned driver and remain disabled in this experiment. They are
distinct from the UC event/response indications at bits 2/3; their presence
alone is not proof of a firmware crash.

The corrected guard tolerates only the two observed disabled bits, mask
`10080`, and preserves their raw values. It still requires exact enable
readback, stable initial samples, all retained domain/config/resource/buffer
checks, and no pending UC indication or any other status bit. No interrupt
clear or broader enable is introduced. Every unexpected delivered interrupt
still masks and stops; this remains an arming/containment test, not a production
UC ISR or firmware-ready capability.

The screen's `IRQ NOTICES` now counts only actual channel-2 notifications.
Failure-path masking is separately shown as `IRQ CONTAINED`, so a failed
precondition cannot be mistaken for interrupt-delivery evidence. `ALLOWED STATUS`
shows the exact tolerated mask. A new host fixture covers all four subsets of
the two tolerated bits, unchanged raw status and rejection of every one of
the other 30 pending bits before arm or binding. The source audit independently
derives these bit positions and adds a sixth IRQ-source mutation rejection.
The new image uses the same XML/authority profile in a separate private output
directory, preserving the original stopped image and frozen device run.

The actual revised image cross-builds and passes the unchanged authority
audit: 1,737 selected frames, 18 private guards and 31 rejected mutations.
The full regression passes 98 Rust summary records / 3,579 passing invocations
(including repeated shared modules), with AArch64 core/native compilation.
The image is 27,701,248 bytes, SHA-256
`1777bf0a87646052a0485f28e67559973b2b0cb48809a5bf12ac717ca851b851`.
Build it separately from the stopped image with the existing private inputs:

```sh
SUNFISH_NATIVE_OUTPUT_DIR="$PWD/.cache/sunfish-native-wifi/private/native-startup-ipa-irq-status-v2" \
  sh platforms/sunfish/build-native-startup-preflight.sh irq
```

After verified LineageOS cleanup following IMG_9112, the user confirmed a
new full power-off for at least ten seconds and cold Fastboot. Physical
Sunfish identity, unlocked state and slot `a`, all 56 selected source hashes,
17 artifact hashes and both private-input hashes were verified and frozen in
private run `ipa-irq-status-v2-20261026T035840Z` (host UTC). One temporary
`fastboot boot` was accepted: sending 0.765 s, booting 2.433 s, total 3.213 s.
The settled screen IMG_9117 confirms **IPA UC IRQ ARM PASS**. Submitted and
completed masks are `7`, actual binding is `1`, physical IRQ is `157` (343),
channel `2` and enable `c`. Raw status `10080` is preserved, equal to the
allowed disabled mask; actual IRQ notices are zero. Configuration, resource
limits and private buffers are retained. Global FSR is zero and context FSR
`400` is format-only, with no defined fault flags. The original photo,
conversion and transcription are hashed in the private run. Physical ADB
verified fresh LineageOS boot `4bad7a53-f7f4-48e9-a0c0-6ad91a3de8fe` with
system_server running; cleanup of the temporary setup and interrupt enables
is complete.
This establishes arming/readback only; physical interrupt delivery, UC
initialization and firmware/DMA readiness remain unproven.

## Shared-memory prerequisites and item-497 metadata census (matched; item absent)

The pinned `ipa_mem_init()` runs before `ipa_config()` and IPA firmware loading.
Its IMEM mapping is physical/identity IOVA `146a8000`, size `2000`. This Sunfish
DTS has no `sram` override, so Linux uses that v4.2 data profile. The separate
SMEM requirement is host 1, item 497, exactly `2000` bytes. Its address must
come from a real allocation/get operation; `86000000` is the beginning of the
whole 2-MiB SMEM reservation, not the item's address. Linux rounds the returned
item's physical span to pages and identity-maps it RW, which can require three
pages for an unaligned 8-KiB item.

The source audit now derives the concrete SMEM v12 metadata layouts, mutex
geometry and allocation-publication order. Linux serializes allocation with
TCSR hardware lock 3 at `01f43000`, APPS owner 1; the private allocator writes
the header, issues a write barrier and only then advances the uncached free
offset. Lookup prefers the 0:1 private partition and otherwise uses the global
partition/heap path. A native implementation must resolve that actual route,
validate existing size or reserve a new item, and retain ownership for the
boot. These are source requirements, not evidence that native allocation works.
Six source mutations reject changed IMEM/item/size/lock geometry, missing
publication and a changed metadata ABI.

The mapping gap is also concrete: the private IOVA `80000000` and SMEM range
use L1 root index 2, while IMEM uses root index 0. The installed four-frame
adapter and existing multi-L3 planner cover one 1-GiB root region; simply
adding another leaf to that plan cannot cover IMEM. The next mapping work
needs owned L2 tables for both roots and separate leaves, without changing
the retained UFS/display contexts. No PAS or DMA-ready capability follows
from the current arming pass.

The prepared standalone census queries only item 497's metadata twice while
holding the shared lock. It reports the selected private/global route, free
offsets, item presence, size, cached-list flag and physical-address candidate.
An absent item is a valid census result and performs no allocation. A size
mismatch, changed second sample or lost lock owner stops the probe. Release
is sent only while the lock still reports APPS ownership; it is not forced
after a failed claim or ownership loss. The probe does not inspect item
contents, write SMEM, allocate, apply IPA power/configuration, alter the Apps
SMMU, invoke PAS/SMC, start DMA/modem/WLAN or flash.

The C variant reuses the bounded directory parser with a one-item selector.
Thirteen actual-C fixture groups cover uncached and cached metadata, global
fallback, missing item, corrupt headers/padding, duplicate partitions and
range/count/cacheline failures. Valid and absent-item walks run with item
payload pages protected by the host VM, so payload reads would fault. The
legacy five-item AArch64 profile also compiles. The actual census uses one
PD, no reset helper, no IRQ/IPC/SMC authority, 2 MiB of uncached RO SMEM, one
RW uncached TCSR lock page and the splash framebuffer. Its CapDL audit checks
524 selected frames and rejects 11 mutations. The prepared private firmware
and EFS snapshots are not included. Build with
`sh platforms/sunfish/build-ipa-smem-census.sh`. The image is 18,649,088 bytes,
SHA-256 `123103eec52e857b5486df9a17c5c884b1dda305cba76c059ef7138773b719a3`.

After verified LineageOS cleanup following IMG_9117, the user confirmed a
new full power-off for at least ten seconds and cold Fastboot. Physical Sunfish
identity, unlocked state and slot `a`, all 67 selected source hashes, 14 artifact
hashes and the reference boot template were verified. Sources and artifacts
were frozen in private run `ipa-smem-census-20261026T043517Z` (host UTC).
One temporary `fastboot boot` was accepted: sending 0.563 s, booting 2.232 s,
total 2.802 s. IMG_9126 confirms **SMEM METADATA MATCH**: two completed scans,
one lock attempt, release sent `1` and local error zero. The selected private
partition is index `7`, SMEM-relative offset `11d000`, size `7c000`, cacheline
`20`; its uncached free offset is `20` and cached free offset is `7c000`.
Item 497 is absent (`ITEM PRESENT=0`), so its zero address/size fields do not
describe a usable mapping. The original photo, conversion and transcription
are hashed in the private run. Physical ADB verified fresh LineageOS boot
`bd34d93c-29d9-458a-b263-1c021ebe553e` with system_server running; cleanup is complete. This establishes stable metadata observation and
the native shared-lock path, never allocation, mapping or firmware readiness.

## One-shot item-497 allocation/get experiment (guard stop; zero writes)

The captured cold census establishes an empty host-0:1 private partition at
SMEM-relative `11d000`, size `7c000`, table index `7`, cacheline `20`, uncached
free offset `20` and cached free offset `7c000`. The next standalone C test
accepts only that exact layout and an absent item 497 after two matching
locked metadata scans. A different route, existing/cached item, overlap with
another partition, invalid item limit or changed snapshot stops before any
SMEM store. The optional `SIII` item limit follows the pinned source's layout
and fallback to 512; fixed IDs below eight are not allocated.

The test implements the pinned `qcom_smem_alloc_private()` publication order:
write the canary/item pair, aligned size and zero padding as three 32-bit
header words; leave the reserved word untouched; issue a full barrier; then
publish the new uncached free offset in a fourth word. Each store consumes
its operation before execution and checks lock ownership and readback. Failed
or uncertain stores stop without retry or rollback. The shared lock is released
only if it still reports APPS ownership. Two post-publication metadata gets
must return the new uncached 8-KiB item and match before the test passes.
No item payload is initialized, read or written.

The new Microkit profile splits the SMEM reservation into contiguous mappings:
one RW uncached page at `8611d000`, with the other 511 pages RO uncached. This
is page-granular authority: the writable page also contains the beginning of
the payload, but the actual C transaction stores only the four planned metadata
words. The framebuffer and TCSR lock page are the only other writable device
regions. One PD has no IRQ, IPC, reset helper or SMC capability; no IPA power,
configuration, Apps SMMU, DMA, PAS, firmware/modem/WLAN start or flash occurs.
An allocated item persists until AP reboot, including after a later failure;
reboot is required before another experiment.

Eighteen actual-C fixture groups pass, including each of four uncertain writes,
each of eleven post-claim ownership-loss boundaries, lock timeout, changed pre
and post snapshots, malformed/present/cached items, wrong virtual injection,
foreign partition overlaps, item limits and forbidden write operations. They
check that every unplanned SMEM byte remains unchanged and reject scalar
payload reads, including bytes that share a VM page with metadata. The same
fixtures pass AddressSanitizer and UndefinedBehaviorSanitizer. The 13 guarded
census fixture groups also pass. Allocator, census and legacy directory profiles
cross-compile for AArch64 with warnings as errors. The source audit binds the
new constants to the hashed cold census and pinned Linux, rejects eight new
mutations and retains six shared-memory source mutation checks. The actual
CapDL audit checks 524 selected frames and rejects 14 authority mutations.

Build with `sh platforms/sunfish/build-ipa-smem-allocate.sh`. The prepared
private image is 18,653,184 bytes, SHA-256
`8ccedf99734f964a5e3aeb902dc5b4c88c147ceae4f2bbf015f64c8f1eba5180`.
It includes no prepared private firmware or EFS payloads. All 74 selected
source files and 16 artifacts are hashed in its manifest. After verified
LineageOS cleanup following IMG_9126, the user confirmed a new full power-off
for at least ten seconds and cold Fastboot. Physical Sunfish identity,
unlocked state and slot `a`, all selected sources/artifacts, the reference
boot template and archived census evidence were verified and frozen in
private run `ipa-smem-allocate-20261026T045126Z` (host UTC). One temporary
`fastboot boot` was accepted: sending 0.562 s, booting 2.364 s, total 2.933 s.
IMG_9129 confirms **SMEM ALLOC GET STOP** with local error `2b` (43), two
pre-scans and zero post-scans. Lock acquisition took one attempt and release
was sent. Submitted/completed write masks and verified publication are all
zero; the uncached free offset remains `20` and item 497 is absent. The
source-derived item limit is `26d` (621), so the item-limit check passed.
Error 43 groups the foreign table's empty-slot, range and overlap checks;
the current screen does not identify the rejected row or condition. This
is a precondition stop, not a hardware write failure or proven physical
overlap. No allocation was attempted. Photo, conversion and transcription
are hashed in the private run. Physical ADB verified fresh LineageOS boot
`a8fbc3d9-d891-43d6-b3ab-2876a17f6439` with system_server running; cleanup is complete.

If the cold preconditions still hold, the expected headline is
`SMEM ALLOC GET PASS`, pre/post scan counts `2`/`2`, total scans `4`, submitted
and completed write masks `f`, verified publication `1`, release sent `1`,
item present `1`, item physical candidate `8611d030`, size `2000`, new free
offset `2030` and local error zero. The future page-aligned identity-map span
would start at `8611d000` and cover three pages. Those are conditional expected
values, not a measured allocation or installed SMMU mapping. A successful
allocation/get experiment still grants no production DMA or firmware readiness.

## Read-only explanation of the allocation guard (cause identified)

IMG_9129's local `2b` (43) was raised before any SMEM allocation write.
The stopped guard uses one code for a foreign row with exactly one zero
field, a nonzero out-of-range span or a nonzero overlap with the selected
partition. The pinned Linux global/private partition enumeration skips a row
when either offset or size is zero. The native guard differs there, but the
photo does not show which row failed; that difference is a hypothesis, not
a confirmed root cause. The frozen allocation guard remains unchanged.

The next standalone observer captures each table row's offset, size, host
pair and cacheline, plus the selected item/partition metadata and optional
item limit, twice under shared lock 3. It compares all captured fields and
releases only while APPS still owns the lock. Classification occurs from the
captured private RAM after release. `GUARD MASK` and `BAD CAUSE` use `1` for
exactly one zero field, `2` for an invalid nonzero range and `4` for an in-range
nonzero overlap; `8` denotes a selected partition overlapping the fixed global
header. The screen reports the first rejected row, its offset/size/hosts,
and counts of all partial-empty, range and overlap cases. An entirely empty
row and the selected partition's own row do not count as foreign failures.
A `SMEM TABLE MATCH` headline means two geometry snapshots matched, even if
the foreign guard mask is nonzero; it is not allocation approval.

All 2 MiB of SMEM are uncached read-only in the actual CapDL. Only the shared
lock page and existing framebuffer have device write authority. There is one
PD and no IRQ, IPC, SMC or reset helper. The ELF contains no allocation routine
or metadata-write virtual symbol. The observer never reads item payloads,
writes/allocates SMEM, configures IPA/SMMU, starts DMA/firmware/modem/WLAN,
flashes or automatically resets the device.

Eighteen actual-C layout fixture groups cover distinct partial-empty/range/
overlap cases, combined causes, adjacent spans, table-field changes, optional
item-limit metadata, malformed metadata, lock timeout and lost ownership.
Capture also passes with the entire synthetic SMEM image protected read-only
by the host VM and with scalar payload reads rejected. The 13 census and 18
stopped-allocator fixture groups remain passing. All four C profiles compile
for AArch64 with warnings as errors. The source audit checks the exact frozen
allocation guard, the pinned Linux zero-field policy and rejects five new
layout-source mutations. The actual CapDL audit checks 524 selected frames
and rejects 11 mutations, including writable SMEM, cache/execute changes and
extra SMC/IRQ authority.

Build with `sh platforms/sunfish/build-ipa-smem-layout.sh`. The prepared image
is 18,649,088 bytes, SHA-256
`14f08b16ddb766b234e6bc37abd62cb2ba152eb309c7823eb48bec9ee572c146`.
Its manifest hashes 81 selected source files, 16 artifacts and the stopped
photo/transcription and frozen guard inputs. It contains no prepared private
firmware or EFS payload. After verified LineageOS cleanup following IMG_9129,
the user confirmed a new full power-off for at least ten seconds and cold
Fastboot. Physical Sunfish identity, unlocked state and slot `a`, all selected
sources/artifacts, the reference boot template, stopped evidence and unchanged
guard were verified and frozen in private run
`ipa-smem-layout-20261026T050609Z` (host UTC). One temporary `fastboot boot`
was accepted: sending 0.560 s, booting 2.233 s, total 2.809 s. IMG_9132 confirms
**SMEM TABLE MATCH**, two scans, release sent `1` and local error zero. There
are `b` (11) rows and an item limit of `26d` (621). The first rejected row is
index `0`, cause `2`, offset `1ff000`, size `1000`, hosts `ffff:ffff`: exactly
the final SMEM page holding the TOC. The old guard incorrectly required that
metadata descriptor to lie before the TOC. The combined guard mask is `3`:
one range rejection, one partial-empty row and zero overlaps. Linux's source
also skips a row when either offset or size is zero. The target partition
remains `7` / `11d000` / `7c000`, uncached free `20`, item 497 absent. Original
photo, conversion and transcription are hashed in the run. Physical ADB verified
fresh LineageOS boot `4a89c537-1ada-4022-a88e-e7ca5ca7170c` with system_server
running; cleanup is complete. This identifies the local guard problem; allocation,
mapping and firmware readiness remain unproven.

## Corrected TOC descriptor / empty-slot allocation guard (allocation/get passed)

The corrected profile changes the foreign-table validation only. It requires
exactly the measured 11 table rows and row `0` to describe the final 4-KiB
TOC page: offset `1ff000`, size `1000`, hosts `ffff:ffff`. A changed count,
offset, size or either host fails with local error 47 before any SMEM store.
Only that verified metadata row is excluded from the data-partition checks.
Every other nonempty span must still fit strictly before the TOC and not
overlap the selected target partition; another TOC-shaped row remains an error.
Rows with either zero offset or zero size are skipped, matching the pinned
Linux partition enumeration. The source separately maps the TOC at
`region base + size - SZ_4K`; hosts `ffff:ffff` are neither APPS host zero nor
the global-partition host `fffe`. This correction does not widen general
range checks or disable overlap detection.

The original default guard remains selectable and reproduces its TOC-span
rejection; its frozen failed image/run are preserved. A separate
`SMEM_IPA_ALLOC_V2` variant selects the new guard through the same allocation
transaction, with screen header `IPA SMEM ALLOC V2`. All target preconditions,
metadata-only access, two pre-scans, four publication stores/barriers,
owner/readback checks, two post-gets, release rules and consumed-attempt
behavior are unchanged. Uncertain writes still stop without retry or rollback.
The single RW uncached SMEM page and the other 511 RO pages are unchanged;
no IPA/SMMU/IRQ, DMA, PAS/SMC, firmware/modem/WLAN startup or flash is added.

Twenty-seven actual-C corrected-allocator fixture groups pass, including all
four uncertain-write and eleven post-claim ownership-loss boundaries. New
cases cover the observed TOC descriptor with either half-empty slot form,
changed descriptor fields/hosts/count, another TOC-shaped row, true overlaps,
invalid ranges and a valid unrelated data span. Every unplanned SMEM byte
stays unchanged and scalar payload reads are rejected. The same 27 groups
pass AddressSanitizer and UndefinedBehaviorSanitizer. The 19 default-allocator,
18 read-only-layout and 13 guarded-census groups also pass. All five C profiles
compile for AArch64 with warnings as errors. The source audit binds the change
to the hashed IMG_9132 evidence and Linux host/TOC/empty-slot logic, rejecting
nine new correction mutations while retaining eight publication/lifecycle
and six shared-memory source mutations. The actual capability audit still
checks 524 selected frames and rejects 14 authority mutations.

Build with `sh platforms/sunfish/build-ipa-smem-allocate-v2.sh`. The new private
image is 18,653,184 bytes, SHA-256
`fc5f67224bd16c96b239cddaed7534a86f511925983f62fc4db57f1bdb01e5a1`.
Its manifest hashes 87 selected source files, 18 artifacts, the census and
layout photos/transcriptions, and the reference boot template. It embeds no
prepared private firmware or EFS payload. Fresh LineageOS cleanup after
IMG_9132 is verified. The user then confirmed a new full power-off for at least
ten seconds and cold Fastboot. Physical Sunfish identity, unlocked state and
slot `a`, all 87 selected sources, 18 artifacts, the reference boot template
and both photo evidence sets were verified and frozen in private run
`ipa-smem-allocate-v2-20261026T052736Z` (host UTC). One temporary
`fastboot boot` was accepted: sending 0.574 s, booting 2.236 s, total 2.816 s.
The attached screen confirms **SMEM ALLOC GET PASS** under revision header
`IPA SMEM ALLOC V2`. Submitted/completed write masks are `f`, verified
publication and release are `1`, pre/post scans are `2`/`2`, total scans `4`
and local error zero. Item 497 is present at `8611d030`, size `2000`; the
uncached free offset advanced from `20` to `2030`. The future page-aligned
identity-map candidate is `8611d000` for three pages. The original PNG and
transcription are hashed in the private run. Return to LineageOS is requested
to clear the temporary allocation. Physical ADB verified fresh LineageOS boot
`e2d48130-4f11-48ae-8563-5114ecf22cca` with system_server running; cleanup is complete.

The measured values match the planned allocation/get result: pre/post scans
`2`/`2`, total scans `4`, write masks `f`, publication/release `1`, item physical
address `8611d030`, size `2000`, free offset `2030` and error zero. A later
identity-map span would use `8611d000` for three pages. The standalone C
experiment proves allocation/get only; it does not grant a production DMA
lease, retain the item across reboot, install a mapping or start firmware.

## Seven-frame, two-root private-RAM mapping experiment (device pass)

The measured shared item requires three pages from IOVA `8611d000`. Together
with the source-derived IMEM IOVA `146a8000` and existing private pool IOVA
`80000000`, the planned address geometry occupies root entries `0` and `2`
and three distinct leaf regions. The default four-frame adapter covers only
one root/leaf region, so adding shared mappings to its old image would be
incorrect. The new bounded pure planner accepts exactly two roots and three
leaf regions, up to 128 sorted distinct pages, through seven caller-owned
frames: one TTBR0 root, two L2 tables, three L3 tables and an empty TTBR1 root.
Existing one-root planner APIs keep their original behavior and limits.

The adapter has a separately selected seven-frame staging path. It validates
each extra physical/virtual identity, distinctness from all retained tables,
register spans and private data, hardware address widths and data-page
ownership before touching table frames. It consumes and retains the extra
frames before the first scalar volatile table write, reads back all seven
images, and accepts a register plan only when its publication step names the
exact seven staged identities. The executor has an explicit seven-frame
publication operation. Existing captured route/context preimages, active-bank
and ASID checks, ordered register writes/readbacks, bound fault IRQs and bounded
TLBSYNC remain in force. A failure retains tables/routes without retry,
rollback or release; no production DMA/firmware-ready token is created.

The first device experiment intentionally maps **only allocator-owned private
RAM** at the future IOVA geometry. Two pages use `146a8000`, 32 use `80000000`
and three use `8611d000`; all 37 physical outputs are distinct private frames.
It does not map actual IMEM or SMEM and does not reuse the expired C allocation
as a live ownership claim. All private RAM is zeroed/read back before installing
the two exact IPA SID entries `520`/`522` in eligible bank 4. Every other route
and bank, including retained UFS bank 2 and display bank 3, remains protected.
The sole writable context window is bank 4; other context banks are RO.
No IPA power/configuration, real shared-memory allocation/payload access,
DMA/ATS, PAS/SMC, firmware/modem/WLAN startup, flash or automatic reset is added.

Nine new pure tests independently walk generated physical table links for
all mappings and invalid holes, distinguish identical L2 indexes in separate
roots, reject every pairwise table alias and table/data alias, check later
IOVA/PA bounds and ordering, and preserve buffers on identity mismatch. Three
new RAM-backed native tests execute the complete two-route install/publication,
compare every foreign context span, reject unowned/aliased extra frames before
writes and reject substituted publication identities before MMIO. Focused
suites pass 33 root-table tests, 98 native-adapter tests and 104 register/executor
tests (including their shared-module fixtures). The first model assertion
incorrectly expected hardware's `67` SCTLR readback from ordinary RAM; the
fixture correctly retains the programmed ASIDPNE bit and expects `1067`.
The register policy is unchanged and the initial failed assertion is retained.
The full regression passes 99 Rust summary records / 3,615 invocations, plus
AArch64 no_std core/native compilation. Both the new seven-frame and existing
four-frame features cross-build; the legacy actual CapDL audit and its nine
mutations still pass.

The new actual CapDL audit proves seven uncached RW/NX table frames, 37
contiguous private RAM frames (148 KiB), ten unmapped guard pages, exact physical
symbol injection, context/global fault IRQs 132/97 and no SMC/IPC/reset helper.
It rejects 15 mutations, including cache/execute changes for all three added
tables. The pinned-source audit binds the IOVA geometry to the successful
allocation photo and Linux IMEM/pool requirements, rejects four mutations
and confirms that physical outputs always come from private allocator frames.

Build with `sh platforms/sunfish/build-smmu-install-probe.sh ipa-roots`.
The prepared image is 18,698,240 bytes, SHA-256
`034b693c895ae7131df2f9aad890fc7e9a98e060304c76b553de1b6136a588cf`.
Its manifest hashes 91 selected sources and 27 artifacts, including nested
legacy-profile validation files, the allocation evidence and reference boot
template. It embeds no prepared private firmware or EFS payload. LineageOS
cleanup after the allocation experiment is verified. The first readiness
report still exposed a fresh Android session over physical ADB, so no image
was sent then. After the user confirmed a new cold Fastboot, physical
Fastboot verified Sunfish, unlocked state and slot `a`. All 91 selected
sources, 27 artifacts, allocation evidence and the reference boot template
were verified and frozen in private run
`ipa-root-mapping-20261026T060750Z` (host UTC). One temporary `fastboot boot`
was accepted: sending 0.559 s, booting 2.235 s, total 2.805 s. Accepted boot
alone was recorded as pending, without claiming installation or translation.

IMG_9137 confirms **ROOT MAP INSTALL PASS** under `IPA ROOT MAP TEST`, root
mask `5`, table frames `7`, leaf tables `3`, private pages `25` and bytes
`25000` (all hex). Primary slot `6` reads SMR `80000520`, peer slot `7` reads
SMR `80000522`, and both S2CR values are `4`. SCTLR is `67`, ACTLR is `1`,
TLBSYNC takes one wait read, global FSR is zero and context FSR `400` contains
only the permitted format field, with no defined fault flags. The private
pool starts at physical `802e0000`. Retained `WRITE STEP 13` and `REGISTER
81c` are successful checkpoints, not failure fields. The original HEIC,
converted PNG and literal screen transcription are hashed in the private run.

This proves experimental private table/register installation readback across
the required two-root geometry. Every output PA remains private allocator RAM;
the screen explicitly states `IMEM SMEM NOT MAPPED`. Actual device DMA
translation and real retained IMEM/SMEM ownership/mapping remain separate
dependencies, and no production readiness is granted. After the user reported
restarting, physical ADB verified fresh LineageOS boot
`2be02bfa-2dfb-4463-ae24-9a5e84066762` with system_server running; cleanup is
complete. No further device operation is part of the Rust-owner work below.

## Retained Rust SMEM owner (host validation)

`sunfish_ipa_smem.rs` ports the successful bounded C allocation/get path to
an owner that consumes and retains its metadata/lock view. It uses the existing
Rust v12 directory parser, accepts only the measured empty host-1 partition,
and preserves the exact TOC descriptor, empty-slot and foreign-span guards.
It serializes through real TCSR lock 3, compares two pre-scans, submits the
three fixed header words and then the free-offset publication word, reads
back each store and compares two post-gets. The attempt and each possibly
uncertain store are consumed before hardware access. On failure it retains
partial state without retry or rollback, and never releases a foreign lock.
There is no reset/free/extraction path or Drop-time hardware operation.

The separate `sunfish_ipa_smem_native.rs` window checks physical and virtual
SMEM/metadata/lock identities and alias bounds before access. It admits only
the four exact ordered word/value pairs in the one metadata page, rechecks
APPS lock ownership before a store and uses full system barriers with scalar
volatile reads/writes. Construction still requires audited real uncached
mappings and exclusion of other AP clients; numerical addresses alone grant
no authority. The initial library-only stage selected no new device manifest
or boot image; the combined profile below now selects this owner.
Returned item/page-span values are diagnostics, not exclusive page leases:
page rounding includes partition/item metadata. A later SMMU integration must
retain the owner and establish the real shared-memory mapping authority.

Twelve new host test groups cover the measured allocation/get values, both
host orientations, Linux partial-empty slots, the item-limit fallback, changed
TOC/target/foreign spans, already-present items, unstable snapshots, all eight
uncertain outcomes at four stores, readback errors, lock loss at every owned
boundary, bounded lock contention, both post-get failures and native RAM-backed
window identity/order/whitelist checks. Payload reads fail the model; every
unplanned byte, including the reserved header word, stays unchanged. The
pinned Linux/C proof audit rejects nine additional Rust-source mutations.

The initial instability fixture accidentally described an invalid cached
allocation, so the parser correctly returned an allocation error before the
expected instability check. The fixture now changes a valid cacheline field
between snapshots; no runtime guard was weakened. That initial failure log is
retained. The first broader cross-check also put the unsafe IO contract/window
inside the forbid-unsafe core harness. They now stay in the separate native
harness, with a concrete allocation wrapper to force AArch64 code generation;
the safe directory parser remains in the forbid-unsafe core. The corrected
full regression passes 100 Rust summary records / 3,627 invocations, plus
AArch64 no_std core/native compilation and concrete allocation code generation.
The source snapshots, audit, host executable and final/initial logs are hashed
in the private `ipa-smem-rust-owner/manifest.json`. This is library validation
only: no new image had booted. The subsequent integration below supplies the
combined image and mapping path; its device pass is recorded below. The
Pixel stays in LineageOS during subsequent offline preparation.

## Real IMEM/SMEM mappings in combined native startup (device pass)

The opt-in `sunfish-native-startup-ipa-shared` feature now selects the live
Rust allocation owner and seven-table adapter in the existing native startup
binary. The combined sequence retains the private EFS shadows/TFTP provider,
applies the previously measured power/configuration stages, captures eligible
routes and context-bank preimages, allocates item 497, installs its real shared
mappings, and then repeats SRAM canaries, private buffer preparation, resource
limits and UC IRQ arming. It stops before PAS, firmware start and DMA.

`sunfish_ipa_shared_memory.rs` retains both the actual reserved IMEM frame view
and `Owner<Window>` for the live SMEM allocation. It is consumed into the native
adapter **before** allocation writes. Failed allocation quarantines that
adapter while retaining the report and memory views. The shared staging path
requires this retained object and compares every IOVA/PA pair against its
canonical plan; a copied item address cannot authorize a mapping. IMEM and
the entire SMEM reservation are excluded from private/table frame identities,
and reserved CPU views cannot alias any register or table mapping. The
existing private-only staging API keeps its ownership checks and validation
order. Both paths still check hardware widths for every retained private
page, including pages absent from a particular plan, plus all output/table PAs.

The plan contains two real identity-mapped IMEM pages at `146a8000`, the same
32 owned private pool pages at IOVA `80000000`, and the three-page identity
span derived from the **live** allocated item, expected from `8611d000` for
item `8611d030/2000`. Seven retained uncached tables cover root indices `0`
and `2` and three leaf regions. Exact SID entries `520` and `522` share the
eligible bank 4 under the existing preimage/ASID/fault/readback/TLBSYNC guards.
Other routes/banks, including UFS/display, remain protected. Descriptor memory
attributes match the pinned Linux READ|WRITE mapping without CACHE/MMIO;
native descriptors additionally retain XN. No CPU IMEM/SMEM payload is read,
zeroed or written, and no device transaction is issued to test translation.
Rounded SMEM pages include shared metadata; they are not exclusive RAM leases.

The new actual CapDL audit covers 2,255 selected frames, seven table frames,
the 128-KiB private pool and all reserved views. IMEM's two CPU pages and 511
SMEM pages are RO/NX uncached; only the single SMEM metadata page is RW/NX.
Real lock 3 is the sole metadata serializer. Fault IRQs 97/132 and IPA IRQ 343
retain their existing channels/triggers. Eighteen distinct guard pages are
unmapped (24 boundary checks), with no SMC/IPC capability. The audit rejects
47 mutations, including new reserved-view rights/cache/execute changes,
added-table cache/execute changes and incorrect fixed physical bindings. The source audit
binds live ownership/geometry and Linux memory attributes, rejecting eight
new mutations in addition to the retained owner/C audits.

Four new shared-backing test groups (28 tests with existing planner fixtures)
check the exact real/private map, every substituted IOVA/PA, missing/failed
allocation, private/reserved aliases, IMEM identity/size/CPU aliases and unchanged
payload bytes. Four new native integration groups execute the full two-route
installation in a RAM-backed hardware model and check missing/substituted
ownership, failure retention, table aliases and unchanged foreign banks.
A further width test preserves rejection of an unmapped retained page outside
hardware physical bounds. The native suite passes 103 tests; the full
regression passes 101 Rust summary records / 3,660 invocations, plus AArch64
no_std core/native compilation and concrete shared attach/staging code
generation. The combined image cross-build passes, and the existing four-table
IRQ image separately cross-builds and passes its actual CapDL audit with 31
mutations rejected. The initial binary build required the Microkit symbol
macro's `*mut` pointer form followed by a const cast; actual CPU permissions
remain RO as audited. Initial build logs are retained.

Build with the prepared private-input directory selected by
`SUNFISH_PRIVATE_INPUT_DIR` and
`sh platforms/sunfish/build-native-startup-preflight.sh shared`.
The prepared image is 27,738,112 bytes, SHA-256
`7190c3f814ddb275286984817646daa79a8ee160d852d2df8ab4a4e9de6fcda9`.
Its private manifest hashes 102 selected source snapshots and 46 artifacts,
including nested legacy validation files. It separately binds the five
prepared private payloads, their manifest/Rust binding, both allocation/root-test
photo sets and the verified reference boot template. Physical ADB verified
LineageOS boot `2be02bfa-2dfb-4463-ae24-9a5e84066762` before the requested
cold transition. The user then reported ready after the full power-off/cold
Fastboot instructions. Physical Fastboot verified Sunfish, unlocked state
and slot `a`. All 102 selected sources, 46 artifacts, five private payloads,
both evidence sets and the boot template were verified and frozen in private
run `ipa-real-shared-20261026T151556Z` (host UTC). One temporary `fastboot boot`
was accepted: sending 0.774 s, booting 2.440 s, total 3.220 s. Accepted boot
was recorded as pending without claiming allocation, mapping or readiness.

IMG_9138 confirms **IPA SHARED SETUP PASS** under `NATIVE IPA SHARED MAPS`.
Root mask is `5`, table frames `7`, leaf tables `3`, IMEM `146a8000/2000`,
item `8611d030/2000`, rounded SMEM span `8611d000` for `3` pages, metadata
write masks `f/f`, release `1` and private pages `20` (all numbers hex).
Both exact IPA SIDs use bank `4`. Global FSR is zero and context FSR `400`
contains only the permitted format field, with no defined fault flags.
The screen retains the live owner and real mappings, with no shared payload
access, DMA/ATS commands, SMC/flash or modem/WLAN start. Its power/configuration/
canary label is clipped after `PA` at the right edge; the gated overall pass
headline, buffer/limit/IRQ pass label and all numeric values are readable.
The original HEIC, converted PNG and literal transcription are hashed in
the private run and bound to the exact one-shot image/source archive.

The combined native live allocation/get and real reserved-memory installation/
readback path has now passed on the device. The adapter keeps its owner and
every mapping through success/failure without retry, rollback or firmware
start. Functional DMA, production UC handling, PAS/GSI, modem/WLAN startup and
traffic remain separate dependencies. After the user reported restarting,
physical ADB verified fresh LineageOS boot
`ba24c93c-bec6-4df3-af0a-0aaa8e87a8b0` with system_server running; cleanup is
complete. The second IMG_9138 attachment is byte-identical to the archived
original and is indexed as a duplicate, without claiming another device run.

## Native UC interrupt dispatcher (offline integration)

The opt-in `sunfish-native-startup-ipa-uc` feature extends the successful shared
mapping entry with `sunfish_ipa_linux_uc::Runtime`. A successful `ArmOnce`
transfers its state exactly once; copied diagnostic reports cannot construct
the handler, and the earlier terminal-notification path is retired after the
transfer. The native entry retains the same power floors, domain, live shared
owner and private buffers. Its actual CapDL profile is unchanged.

The dispatcher follows the pinned Linux
[interrupt handler](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/net/ipa/ipa_interrupt.c)
and [UC byte protocol](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/net/ipa/ipa_uc.c):
clear each UC bit at IPA offset `3010` before reading its defined event byte
at `20` or response byte at `12` (offsets decimal for the two bytes). It handles
event before response, rereads pending status, and clears the captured disabled
mask after draining enabled sources. It then acknowledges the actual IPA IRQ
capability. Scalar byte access avoids every reserved UC byte. Closed write
requests select only pending-status W1C or containment `IRQ_EN=0`; there is no
UC command/shared-memory writer, firmware call or DMA command.

Enabled-source draining is bounded to 64 rounds. Any guard, MMIO, byte-read or
physical ACK failure consumes the uncertain operation, masks once, records
both the original and containment errors, and quarantines the handler without
replay or resource release. Co-delivered SMMU/foreign faults take priority:
they mask IPA without clearing pending UC evidence, reading shared bytes or
acknowledging either fault. Unsupported UC codes are counted as Linux warns
about them; LOG_INFO is accepted. Unlike Linux's log-only ERROR handling,
the initial native startup quarantines on a UC ERROR event.

`INIT_COMPLETED` increments an observation counter; duplicate responses stay
visible. It cannot mint an IPA/GSI/firmware-ready token. The current test keeps
all power floors held rather than implementing Linux's runtime-PM reference
drop and retention transition. Its native guard still requires GSI disabled,
so the later PAS/GSI stage must supply the appropriate post-auth guard before
firmware execution. This is an integrated pre-PAS dispatcher, not proof of
physical delivery or a complete production power lifecycle.

Twelve new behavioral test groups check clear/read/ACK order, simultaneous
sources, new indications during byte reads, repeated/spurious/disabled-only
notifications, unsupported codes, UC ERROR, bounded storms, uncertain outcomes
at every I/O boundary, changed enables, containment failures and fault priority.
An additional arming test verifies the one-time transfer and retired containment
path. All 12 groups pass; the full regression passes 102 Rust summary records /
3,673 invocations plus AArch64 no_std core and concrete native compilation.
The actual native ELF contains the specialized runtime dispatcher and physical
IRQ ACK implementation. The capability audit again covers 2,255 selected frames
and rejects all 47 mutations, with no added SMC/IPC/IRQ/memory authority.

Build using the existing prepared private-input directory and
`sh platforms/sunfish/build-native-startup-preflight.sh uc`. The private output
is `native-startup-ipa-uc`, separate from the archived shared-map pass. The
final image is 27,742,208 bytes, SHA-256
`6382d2942cee7fa91e87cf3ad9b1e6768ef0cedebe0c28f7379b8b8fee9f8aba`.
The source/reference audit, regression/build logs and source/artifact hashes
are retained there. No device boot is needed solely for this offline handler
integration; the Pixel remains in LineageOS. Physical IRQ delivery, functional
translation, firmware/PAS, GSI/AP endpoints and modem/WLAN traffic remain unproven.

The next firmware work must follow the working kernel's SCM implementation,
not reuse the older downstream PAS envelope unchanged. In pinned
[`qcom_scm.c`](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/firmware/qcom/qcom_scm.c),
INIT_IMAGE, MEM_SETUP and AUTH_AND_RESET all use the clock/bandwidth envelope.
Actual optional resources must be derived from the complete board DT and SCM
driver selection; holding IPA's floors alone does not establish that envelope.
The private `next-pas-source-dependency.json` preserves this source comparison
and the remaining metadata-pool/firmware/post-auth-guard dependencies. No secure
operation was added or performed in this stage.

## Retained Linux IPA firmware loader and SCM hold (offline)

`sunfish_ipa_linux_loader` now composes the checked packed-image adapter and
existing sealed PAS codec into one retained attempt. Its public image
constructor accepts only the exact private working reference: 29,120 bytes,
SHA-256 `7d61b64288248719d14a4b316e9cb83917c11a2c9753dff5475a7a2db95394dd`.
Source identity does not replace PAS signature authentication. The loader
owns its memory backend through `ManuallyDrop`, with no extraction, retry,
rollback or Drop-time free, including after a possibly uncertain secure call.

Two contiguous private pages hold compact header/hash metadata. Both pages
must initially be zero; copy/readback and a final full payload/padding check
precede INIT. The loader never accesses that allocation again after submission,
because secure firmware may modify or protect it. After successful INIT and
relocatable MEM_SETUP, it stages only planned segments and BSS into the
`98f80000/10000` reservation, verifies every written byte, then attempts AUTH
once. No target access follows AUTH submission. The infallible legacy staging
interface is wrapped by a first-error latch: a native write/sync failure
suppresses further accesses and prevents authentication despite an otherwise
successful staging report. Every source gap stays untouched.

`sunfish_ipa_linux_loader_native::Window` supplies scalar volatile byte access
and system barriers, checking the two metadata frame identities, physical
contiguity, exact target region, alignment, bounds and view separation. Actual
uncached RW/NX mapping, allocator ownership, all reserved/allocated exclusions
and absence of a preexisting SHM bridge remain explicit native construction
requirements. Geometry alone grants no authority. This initial library stage
selected no device mappings; the opt-in image below now selects the loader.

`sunfish_ipa_linux_pas::LinuxTransport` is required by the loader's start API,
so the older downstream envelope cannot accidentally omit SCM resources around
MEM_SETUP. It checks the sealed request kind and brackets all three mutating
calls, retains both secure and release errors, and becomes terminal on failure.
The pinned SCM driver and both built/live Linux reference DTs establish that
Sunfish's optional core/iface/bus clocks are absent, while its real ICC path is
aggre2 `MASTER_CRYPTO_CORE_0` to mc-virt `SLAVE_EBI_CH0`, tag `7`. The source
request is average zero, peak `UINT_MAX` kB/s. The graph selects CE0, SN11, SN0,
SH0, MC0 and ACV; numerical peak divisors are 8, 16, 16, 16 and 4 respectively,
while ACV uses bit 3. The audit also binds the PAS IDs, metadata-pointer ABI,
reserved target and stage-before-auth order to the exact kernel archive.

`sunfish_scm_linux_power::HoldOnce` implements the corresponding bounded cold
hold through the real RSC transaction interface. Two stable baseline passes,
CMD_DB identity checks and full handoff preservation precede its six writes.
It retains observed X and raises Y without decreasing another observed
component, saturating numerical votes to 14 bits and preserving ACV masks.
Each committed write is consumed before submission and checked by readback.
Call leases verify all six BCMs and the same handoff before and after PAS.
Lease sequence, outstanding state and terminal verification failure live in
the retained owner, so dropping/recreating a proxy cannot reset them. These
are experimental active floors retained until reboot, not a full shared-client
ledger or Linux's suspend/wake voting and unvote lifecycle. No production
power or firmware-ready permit is minted.

Fourteen new behavioral groups cover identity, full sequencing, protected-view
rules, metadata changes and uncertain stores, target writes/readback, every
secure-call failure, resource/guard failures, native bounds, exact SCM arithmetic,
handoff/floor loss and proxy reconstruction. The loader suite passes 40 tests
with existing fixtures (plus one separately run private-image test); the SCM
suite passes 82 tests with existing fixtures. The exact private reference
passes the scalar native backend in ordinary host RAM: 7,020 metadata bytes
and 16,832 staged/verified firmware+BSS bytes. Its secure replies and MMIO are
models, not device evidence. The full regression passes 104 Rust summary
records / 3,795 invocations, plus AArch64 no_std core and concrete native
loader/metadata/SCM-hold code generation. Legacy private entrypoints unused
by the partial core harness receive narrow dead-code allowances; strict
warnings remain enabled and their dedicated suites still run.

Initial logs are retained: one SCM fixture changed an unobserved word instead
of the TCS enable field, and the expanded no_std harness initially omitted
legacy modules/entrypoints. The source audit also corrected binding spellings
from the actual pinned header and skips unused providers without a phandle;
both selected providers must still resolve exactly. No runtime guard was
relaxed to resolve those failures. Private `ipa-linux-loader/manifest.json`
binds source snapshots, reference audit, host/private-image tests and final logs.

That library stage selected no image/SMC capability and performed no device
action. The integration below completes its native-entry, mapping and guard
dependencies. PAS results, physical UC delivery, GSI/AP endpoints, functional
DMA and native modem/WLAN association/traffic remain unproven.

## Native IPA firmware entry (device checked)

The opt-in `sunfish-native-startup-ipa-firmware` image extends the proven
shared-memory setup and offline UC runtime. `pixel_ipa_firmware` retains the
actual RSC controller, SCM hold and native loader in `Startup` before its
metadata writes or PAS calls. It resolves the six real SCM CMD_DB entries,
applies/verifies their bounded cold holds, checks the immutable private IPA
input, prepares the owned metadata, and runs the sealed PAS15 sequence. Every
pre-auth phase requires the retained domain/buffer/resource/configuration
checks and no unexpected pending UC interrupt. Any failure masks once without
clearing evidence or acknowledging delivery, retains every partial owner and
never retries or rolls back. It does not start modem or WLAN.

The new actual CapDL profile selects 2,273 frames: the inherited 2,255 plus two
contiguous allocator metadata pages at VA `75601000` and sixteen reserved
firmware pages at `98f80000`, VA `75800000`. Both new CPU views are uncached
RW/NX, without frame/CPU aliases or overlap with other allocations. Metadata
is excluded from all reserved DT spans. Four additional guard pages are
unmapped. There is exactly one unbadged SMC capability, in the sole native
owner's slot 7, and no client IPC. Its adapter receives only sealed requests;
the native loader requires the PAS15 Linux transport. GSI stays entirely RO,
the IRQ set remains `97/132/343`, and no host GSI/DMA command is supplied.
Real IMEM/SMEM CPU payload remains inaccessible to this loader.

The working Linux `gsi_setup()` requires GSI_STATUS bit 0 after authentication.
The new read-only post-auth comparison therefore permits exactly GSI status
`0 -> 1`, with UC enables `c`, while comparing every other captured word twice.
The UC handler selects that comparison only through its retained loader with
an actual successful AuthReturned session; copied diagnostic reports do not
select it. It continues to check domain faults, private buffers and all seven
resource words. No production-ready token is created. The combined result
screen preserves SCM/PAS masks, metadata/staging counts, auth replies, GSI and
SMMU status alongside UC response, init observation and IRQ ACK counts. It
does not overwrite the PAS result when an interrupt arrives.

The strict post-auth guard's new test covers each changed captured word,
every read failure, reserved/disabled GSI values and forged cold input. The
full host regression passes 104 Rust summary records / 3,796 invocations plus
AArch64 no_std core/native compilation. The real native entry cross-builds
with the actual Microkit SMC adapter, source-bound to the pinned Linux path;
seven new source mutations are rejected. The capability audit rejects 60
mutations, including new mapping rights/cache/execute changes, SMC removal/
duplication/misplacement and wrong physical/symbol bindings. The older shared
profile separately still rejects its 47 mutations with SMC disabled. The
initial mutation callback routing failure is retained in the build logs; the
firmware cases now all reach the correct complete audit, including its
contiguity-specific failure assertion.

Build with the existing prepared private inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh firmware`.
`SUNFISH_IPA_FIRMWARE` may select the private source path; the build pins its
exact identity before embedding it. The final image is 27,774,976 bytes,
SHA-256 `e49f0aad489ed11e484db833425f6c81cef4231c306ff749d672886115082ba4`.
Sources, artifacts, private input identities, audits and final logs are bound
in private `native-startup-ipa-firmware/manifest.json`. No image has been sent
to the phone in this preparation stage.

One cold temporary boot was accepted on 2026-10-26 after verifying serial
`02281FQC200058`, product Sunfish and unlocked state. The immutable run is
`device-runs/ipa-firmware-20261026T171919Z`, including the exact source/artifact
copies, pre-boot manifest, identity reads and one Fastboot result. `IMG_9143`
shows `IPA AUTH RETURNED`: SCM and PAS masks `3f/3f`, metadata `1b6c`, target
writes/verification `41c0`, auth monitor/service zero, GSI status `1`, global
FSR zero and context FSR `400`. These are actual native PAS15 success and
enabled GSI evidence. No host GSI or DMA command, modem/WLAN start or partition
flash was issued. The run is consumed and must not be retried.

The screen also shows UC init count zero. The earlier expectation of a UC
initialization response immediately after PAS15 was incorrect. Pinned
[`ipa_uc.h`](https://raw.githubusercontent.com/miromraz/linux/2267e248204e7e051e0f7355298949a6440de579/drivers/net/ipa/ipa_uc.h)
states that the modem loads and starts the IPA-resident microcontroller on
its first boot. `ipa_modem_notify()` takes its proxy power reference at
`QCOM_SSR_BEFORE_POWERUP`. This test deliberately never boots the modem.
`ipa_setup()` proceeds from PAS15 to `gsi_setup()` without waiting for UC init;
the working cold Linux trace likewise completes IPA setup at 70.88 seconds
before starting the modem at 176.34 seconds. UC init/physical IPA IRQ delivery
remain unproven later milestones, not a reason to repeat this PAS15 test.

Next is the pinned mainline GSI allocation/context path: four AP channel/event
pairs and the IPA v4.2 modem-channel allocation workaround, then command and
exception endpoint setup and functional DMA. Native MPSS startup, UC init,
WLAN association and traffic remain required. Physical ADB confirmed a fresh
LineageOS boot after this result, clearing the experiment's retained state.

## Mainline GSI allocation and context core

`sunfish_ipa_linux_gsi.rs` keeps this path distinct from the older downstream
ten-channel profile. It derives ring addresses from the actual prepared
128-KiB pool. Event IDs are assigned in endpoint-name order, producing channel
to event pairs `0 -> 2`, `1 -> 0`, `2 -> 1`, `3 -> 3`; setup then runs in
ascending channel order. Each event allocation must complete through its
physical IRQ and reach ALLOCATED state before its eleven context/doorbell
writes. Each channel allocation similarly precedes its nine context/scratch
writes. Contexts are checked twice without issuing a channel START or a TRE
transfer doorbell. The event doorbell only publishes the last consumed empty
slot. Ring buffers/domain and authenticated firmware remain retained by the
trusted backend throughout; plans and reports are diagnostic copies.

The source's actual scratch-3 expression preserves the low sixteen bits,
despite a nearby comment saying upper. The implementation follows the
expression. GSI's IRQ is SPI 432 plus 32, **464, level high**, from the pinned
Sunfish DT, distinct from IPA's edge IRQ 343. Nine source-ordered mask writes
precede binding/arming; the error log and four error-enable writes precede
the first allocation. Raw sources are checked after a completion clear and
before masking, so a failed clear cannot be hidden by the aggregate status.

IPA v4.2 then allocates modem-EE channels 0 through 3 with generic commands;
this does not boot MPSS. The source accepts result 1 (success) and 2 (existing
state), which the report records separately. Every other result, including
RETRY, stops this one-shot attempt. Unexpected or co-delivered errors, bad
states, read/guard/write/ACK failures and expiration mask once, preserve the
first and containment errors, retain all partial allocations and never clear,
ACK, free or replay after failure. The source's 50-ms deadline is sampled
immediately before each actual command store, after preparation/guards, with
wrapping counter arithmetic and no zero/ambiguous timeout.

`python3 platforms/sunfish/test-native-ipa-linux-gsi.py` verifies its thirteen
reference source files against the pinned kernel archive, compiles the actual
register definitions/encoders, context functions, full `gsi_setup()` and its
IRQ/ring/channel setup and allocation functions with host MMIO/completion
fixtures, and compares 740 writes. The C setup starts with nonzero error logs,
including the raw value measured in IMG_9155. Twenty-three Rust groups cover whole-setup sequencing, every successful
path's I/O and guard failure point, uncertain writes, readback corruption,
wrong sources/states, both generic success forms, errors, wrap/deadlines,
one-shot late-interrupt containment and no retry. Four deliberately incorrect encodings fail the independent C
comparison. The core also compiles for AArch64 without std. This script now
runs from `test-modem-qmi.py`; its private manifest binds source snapshots,
generated C, logs and outputs under `ipa-linux-gsi/`. The full regression also
passes its existing 104 Rust summary records / 3,796 invocations and AArch64
core/native code generation before the IMG_9155 correction. The corrected
GSI suite also covers initialization of unqualified raw logs and new pending
sources arriving immediately before or after log initialization.

IMG_9156 subsequently proves physical GSI IRQ delivery, allocation and context
readback. Channel start and functional DMA remain unproven.

## First native GSI entry (stopped before allocation)

`sunfish-native-startup-ipa-gsi` now selects `pixel_ipa_gsi` after the retained
native PAS15 session has actually returned successfully. It retains `SetupOnce`
in `Startup` before the first GSI write. The backend borrows the live prepared
pool, installed domain and authenticated loader, using the strict post-auth
UC/configuration/resource guard on every normal access. A copied plan only
supplies geometry. The command whitelist accepts only source-defined setup
words, those ring IOVAs and ALLOCATE opcodes; the compiled native whitelist
accepts the independent C vectors and rejects unrelated pages and other
channel/generic command encodings.

The profile splits the original 44-page GSI aperture into eight nonoverlapping
views with identical CPU/physical geometry. Only pages `c000`, `1c000`,
`1d000`, `1e000` and `1f000` become uncached RW/NX; the other 39 pages stay RO.
It adds level-high IRQ 464 on Microkit channel 3 while preserving the original
SMMU fault IRQs and edge-triggered IPA IRQ. There are still 2,273 selected
frames, seven owned SMMU tables, 32 private pool pages, two PAS metadata pages,
the exact sixteen firmware target pages, one SMC capability in slot 7 and no
client IPC. Both the XML and actual CapDL binding/rights/cache/alias checks
pass, rejecting 96 mutations. The legacy shared and firmware models retain
their 47/60 mutation checks with their old IRQ/SMC policies. A deliberately
missing new IRQ slot initially raised a KeyError; the validator now rejects
it through its explicit binding check, without changing valid authority.

The runner uses the same input/reply cap slots as pinned Microkit, with
nonblocking receive and the physical counter. This lets every missing
completion expire after 50 ms without a separate timer IRQ. Only a delivered
GSI badge advances a command. Co-delivered foreign/fault notifications first
contain GSI and UC, quarantine buffers and latch the domain stop, without
clearing or acknowledging the delivered evidence. A late GSI interrupt after
the allocation-only setup similarly masks once and retains every resource.
No framework ACK is added. The result screen preserves allocation/program
masks, fresh versus existing modem results, actual notifications/ACKs, last
access/value, PAS completion and SMMU status. Display rendering is outside
the active command waits.

Build with the private prepared inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh gsi`.
The first tested image is 27,791,360 bytes, SHA-256
`a149b36296de25ebce72e423587f5f3e068523ec79fef857f7ad0d9bf851699e`.
The actual native entry, counter, IRQ ACK and nonblocking receive cross-build;
the seven existing atomic deprecation warnings remain unrelated. The native
source audit rejects seven new mutations in addition to its inherited PAS/UC
audits. Full host regression and AArch64 core/native checks pass, including
the twenty-one GSI groups and the independent native whitelist test.

After a confirmed cold power-off, one temporary boot was accepted in private
run `ipa-gsi-20261026T181905Z`. IMG_9155 reports `GSI SETUP STOP`, phase `6`,
index zero, all allocation/program masks zero, no completion notices/ACKs,
one initial IRQ arm and `a` writes (nine setup writes plus containment).
The last read is ERROR_LOG offset `1f200`, value `e71132f0`; local error `8`
is `Pending`. PAS returned `3f`; global FSR is zero and context FSR `400`
has no defined fault flags. No allocation command or transfer was attempted.
Physical ADB subsequently verified a fresh LineageOS boot
`87c561ca-0e0a-4b24-beac-3af9f8070e11` with system_server running.

The stop came from a local guard requiring ERROR_LOG to be zero before its
initialization. The pinned `gsi_setup()` instead writes zero unconditionally
after IRQ/ring setup. The log value alone is not pending-interrupt evidence:
`gsi_isr_glob_ee()` consults the global interrupt status ERROR_INT before
decoding the log. The photo does not establish a GSI firmware crash or a
meaningful error code in those pre-initialization bits.

The corrected core saves the raw log before initialization, rechecks all
five actual raw/aggregate pending-status sources, writes the source-defined
zero once, saves the post-write log and rechecks the pending sources before
enabling interrupts. A pending source still stops without clearing it or
acknowledging it; any failure consumes the attempt. The new screen headline
is `NATIVE IPA GSI V2`, with `ERROR LOG PRE` and `ERROR LOG POST` values.
Both raw values are diagnostic, not readiness predicates. The existing
opcode, MMIO, retained-owner and timeout restrictions are unchanged.

The next device check uses the corrected frozen image after another full
power-off. Expected hex evidence is GSI phase `5`, index `4`, four
AP allocation/program masks `f`, modem accepted `f`, fresh/existing masks
whose union is `f`, `c` physical notices, `c` completion ACKs, one initial arm,
`aa` GSI writes, PAS returned `3f` and no defined SMMU faults. Any other state
is retained evidence, not a reason to retry. No channel START, TRE transfer,
MPSS/WLAN start or flash is part of this image. Native GSI allocation and
physical IRQ delivery remain pending; the full Wi-Fi goal is not complete.

The corrected V2 image is 27,791,360 bytes, SHA-256
`6f5aefe73babace81c1a8104e1328ac24ddff286641a2d416625bb2f2e465942`.
It was built with `SUNFISH_NATIVE_OUTPUT_DIR` selecting the separate private
`native-startup-ipa-gsi-v2` directory; the first image, source snapshots and
stopped run remain preserved. The actual AArch64 native entry, counter and
IRQ paths compile. The source audit rejects eight mutations and the unchanged
CapDL authority passes all 96 rejection cases. The 23 GSI groups, 740 compiled
C write comparisons, four rejected encoder mutations and compiled native
whitelist pass. Its cold device result is recorded below.
The full regression also passes all 104 existing Rust summary records / 3,796
passing invocations and AArch64 core/native checks, with the corrected 23-group
GSI suite run by the same harness.

## Native GSI V2 cold allocation and interrupt pass

After another confirmed full power-off, the exact corrected image was
temporarily booted once in private run `ipa-gsi-v2-20261026T183324Z`.
IMG_9156 reports **GSI ALLOCATION PASS**, phase `5`, index `4`, all four AP
allocation/program masks `f`, modem accepted/fresh `f` and existing `0`.
It received `c` (12) physical completion notifications on badge `8` and sent
`c` completion ACKs, in addition to the one initial IRQ arm. The total is
`aa` (170) GSI writes. The last write/read are `1f108`/`1f080`, with final
value zero. ERROR_LOG changed from raw `e79832f0` to zero through the source's
initialization step. PAS returned `3f`; global FSR is zero and context FSR
`400` has no defined fault flags. No GSI or containment error is displayed.
The original HEIC, PNG conversion, transcription and exact frozen inputs
are saved in the private run.

This proves the allocation/context sequence and actual completion interrupt
delivery. The modem-EE allocations are GSI resources, not an MPSS boot.
No channel START, TRE publication or transfer doorbell was issued, so real
DMA translation, the command/exception endpoint path, modem/WLAN startup
and Wi-Fi frames remain unproven. The requested next cleanup is a return to
LineageOS; physical ADB verified fresh boot
`f1873eed-9071-4207-8fd1-2dac39d892c2` with system_server running. The next
implementation work is AP endpoint configuration and channel startup before
the first actual command transfer.

## Mainline AP endpoint configuration core

`sunfish_ipa_linux_endpoint` follows the same pinned kernel's
`ipa_endpoint_setup()` and `ipa_endpoint_program()`. It programs only AP
endpoints 1 (modem TX), 6 (command TX), 8 (LAN/exception RX) and 9 (modem RX),
in ascending endpoint-ID order. The modem-owned endpoints 4/5/10/11 are
included in availability validation but are not programmed by this step.
There are ten writes per TX endpoint and eight per RX endpoint, 36 in all,
within the already used first IPA register page. The closed write requests
carry no arbitrary register offset, GSI command or transfer doorbell.

The command endpoint's DMA-mode value `83` selects the LAN RX endpoint 8.
That register setting alone does not start its GSI channel or transfer data.
The source-derived TX sequencing/status values, QMAP header lengths and
offsets, RX metadata byte order, aggregation timer/size, disabled HOL-block
words and resource groups are preserved. All defined readback fields must
match; reserved readback bits are ignored. There is no requirement for the
pre-initialization endpoint registers to be zero.

The receive aggregation limit depends on Linux's SKB layout, so it was not
guessed from buffer size alone. An offline C probe was compiled using the
actual reference kernel's saved compiler command, headers and configuration,
with the reference build mounted read-only without journal replay. Its
constant section was read as data; no probe code ran on a device. The exact
configuration has 4-KiB pages, 64-byte cache alignment and NET_SKB_PAD,
320-byte aligned skb_shared_info, 384-byte IPA RX overhead and MTU 1500.
Thus `(8192 - 64 - 1500 - 384) / 1024` is six KiB. The four-byte QMAP header
has mux/length offsets 1/2, and its UL checksum extension is four bytes.
An independent host fixture compiling the actual pinned structure definitions
matches all thirteen exact-kernel constants.

The coordinator requires the retained successful GSI setup object before any
I/O, then checks the trusted retained-owner guard, GSI pending sources/masks
and all eight allocated channel/event states around each endpoint write.
Each write is submitted once and read back, followed by two complete final
comparisons. Any guard, read, uncertain write, pending source or readback
failure records the partial masks and contains interrupts once through the
trusted backend. No rollback, retry, clear, IRQ ACK, free or Ready token is
provided. The core still needs a native backend; these fixtures do not grant
hardware authority.

`python3 platforms/sunfish/test-native-ipa-linux-endpoint.py` verifies fifteen
reference files against the pinned archive, compiles the actual endpoint
initializer, register descriptors, setup iteration and programming functions,
and compares all 36 write values/addresses plus their defined-field masks.
Seven behavioral groups exercise the retained-GSI prerequisite, every read,
guard and uncertain-write failure boundary, every non-sticking write,
defined-field corruption, reserved bits, pending status/state changes,
containment failure and consumed attempts. Four mutated destination/size/
endianness/readback encodings fail the independent C comparison. The core
also compiles for AArch64 without std and is included in the shared regression.
Private source snapshots, C fixtures, exact-kernel ABI evidence and logs are
under `ipa-linux-endpoint/`.
The full shared regression passes all 104 existing Rust summary records /
3,796 passing invocations, the 23 GSI groups and these seven endpoint groups,
including AArch64 core/native compilation. No additional phone test was used
for this offline implementation.

The next startup order is also checked against the pinned `ipa_setup()`:
GSI allocation, AP endpoint configuration, command-channel enable, immediate
commands for IPA memory and table initialization, then exception RX enable,
default route and IPA QMI setup. Enabling exception RX before those memory/
table commands would differ from the working reference. Channel START,
actual command DMA, modem/WLAN startup and Wi-Fi traffic remain unproven.

## Native endpoint and empty command-channel start (passed)

`sunfish-native-startup-ipa-command` extends the proven GSI allocation path
with the four AP endpoint configurations and the first channel enable from
`ipa_setup()`: AP command TX, endpoint 6/channel 1. It does not enable the
exception RX channel prematurely. The pinned full `gsi_channel_start()`
enables NAPI in Linux and event-ring-0 IEOB completion interrupts, then
performs the channel-control START. The native host supplies its own bounded
IRQ dispatcher instead of NAPI. A fresh allocated state `1` must become
started state `2` through a delivered channel-control interrupt on the
existing IRQ 464/badge `8`. The source comparison includes all nine writes,
not only the START opcode.

The source sequence is IEOB mask `1f0b8=1`, type mask `1f088=4c`, channel
clear `1f0a0=ffffffff`, channel mask `1f098=2`, type mask `1f088=4d`, START
`1f008=01000001`, captured channel-source clear `1f0a0=2`, type mask
`1f088=4c`, channel mask `1f098=0`. IEOB stays enabled for future command
completion handling, but this image publishes no TRE or transfer doorbell.
It accepts only the fresh allocated state, never replays a START, and
retains every partial resource and mapping until reboot.

Both actual typed GSI and endpoint owners must have completed before the
command core can perform I/O. Every normal native access checks the retained
domain, private pool, authenticated firmware and primitive/resource guards;
during command startup it also rereads the configured endpoint fields.
Before issuing START, the native runner rejects any already queued input
notification. During the pending command it uses the physical counter and
nonblocking receive, accepts only a GSI badge, and requires the exact channel
source and expected channel/event states. It clears and verifies the captured
source before the sole completion ACK. Timeouts, foreign/co-delivered
sources, uncertain stores, bad state/readback or ACK errors consume the
attempt, contain the sources and retain the owners. No cleanup/deallocation
or production DMA-ready token follows from a successful report.

`test-native-ipa-linux-command.py` compiles the actual pinned full start,
its control/IEOB helpers and channel ISR into the independent Linux C oracle.
Eight behavioral groups cover the exact nine writes, owner prerequisites,
every read/guard/uncertain-store boundary, slow preparation, counter wrap,
missing completions, pending or foreign sources, wrong states, non-sticking
masks/clears, ACK/containment failure and terminal attempts. Three wrong
opcode/IEOB mutations fail the C comparison. The native source audit rejects
seven additional mutations and compiles the actual endpoint/command
whitelists against both C vectors, including exhaustive opcode/channel
rejection. The full regression passes 104 existing Rust summary records /
3,796 invocations plus the 23 GSI, seven endpoint and eight command groups,
with AArch64 core/native checks. Actual Microkit code generation contains
the native entry, counter instructions, volatile stores and IRQ path.

Build with the prepared private inputs and
`sh platforms/sunfish/build-native-startup-preflight.sh command`.
The separate command profile has identical mapping/IRQ authority to the GSI
profile: five GSI write pages, the IPA first/IRQ pages, 2,273 selected frames,
seven SMMU tables, 32 pool pages, two metadata pages, sixteen firmware pages,
one SMC slot and IRQs 97/132/343/464. Its validator explicitly records that
this software issues a channel START, rejects all 96 inherited authority
mutations and claims no physical start completion or functional DMA proof.
The source build runs only the most-derived source audit, which includes the
GSI/PAS/UC checks, rather than repeating that same chain at every level.

The tested image is 27,803,648 bytes, SHA-256
`c9dbc76eaa6b5c82bdc6732fe3d0bf57bf2ed7337e0af5f80dc666567fff8584`.
One cold temporary boot was accepted on 2026-10-26. IMG_9167 shows
`COMMAND START PASS`: endpoint phase `2`, endpoints `f`, all three endpoint
low/high masks `ffffffff/f`, final samples `2`, command phase/state `2/2`,
command writes `9`, notices/ACKs `1/1`, badge `8`, GSI phase `5`, PAS returned
`3f`, global FSR zero and context FSR `400` (format bits only).
Last write/read are `1f098/1f0b0`, last value zero. The image, exact sources,
boot transcript and original photo are retained under
`.cache/sunfish-native-wifi/private/device-runs/ipa-command-20261026T194703Z/`.
The return to LineageOS was verified through physical ADB: boot completed,
`system_server` was running and the boot ID changed from
`f1873eed-9071-4207-8fd1-2dac39d892c2` to
`7810dfa7-fa00-438e-90c5-5e5de8ddcb79`. This check starts the empty command
channel only; the next transfer work is IPA memory/table immediate commands,
then exception RX/default route, IPA QMI and modem startup. Wi-Fi frames
remain unproven.

## First IPA memory transaction (offline encoding only)

`sunfish_ipa_linux_memsetup` now describes the next transaction in the pinned
`ipa_setup()` order. It neither publishes descriptors nor exposes a hardware
write path. `ipa_mem_setup()` emits four immediate commands: local-header init
for `04a8/0140`, then zero-fill DMA commands for processing contexts
`05f0/0200`, `07f0/0200` and modem memory `0bf0/140c`. No optional AP header
region exists in the v4.2 data. All four commands read the retained private
8-KiB zero source; they preserve UC memory, filter/route tables and canaries.

The fixed private layout places 32-byte payload slots at `80006000` through
`80006060`, and four TREs at `80004000` through `80004030`. The first three
TREs carry immediate-command type plus CHAIN; the last carries IEOT and
Linux's toward-IPA BEI setting. The channel doorbell would publish the first
unfilled TRE `80004040`, not the last TRE. After the sole expected event,
the event doorbell would report the last consumed slot `80003000`. The direct
processing-context register write `001e8=000005f0` belongs after transaction
completion, as in the source.

`test-native-ipa-linux-memsetup.py` compiles the original memory-setup function,
command encoders, TRE encoders, v4.2 register descriptors and ring-doorbell
functions against host allocator/commit/MMIO stand-ins. All four payloads
(including zero allocation padding), all four TREs and three register-write
values match. Five test groups cover alternate geometry, destination/canary
preservation, foreign/high transfer pointers, wrong channels and extra events.
The parser preserves TX event length/code/type as raw diagnostics because
the pinned Linux TX path does not use them as completion predicates.
Four deliberate opcode/direction/BEI/doorbell mutations are rejected, and
the core cross-compiles for AArch64 without std. The original C event
structure independently binds the parser's offsets and field widths. The full
`test-modem-qmi.py` regression completed successfully after adding the suite:
104 existing Rust summaries / 3,796 passing invocations, the 23 GSI, seven
endpoint, eight command-start and five memory-transaction groups, and the
existing AArch64 checks.

This is preparatory software evidence only. A real native transfer still
requires retained startup owners, publication ordering, physical interrupt
delivery, stable event consumption, destination/canary readback and fault
checks, with resources retained on every uncertain outcome. No new image or
device command is issued by this suite; memory/table setup and functional
DMA remain unproven.

## First retained memory DMA controller

`sunfish_ipa_linux_transfer` now connects that fixed encoder to a one-shot
controller. It requires the actual completed GSI, endpoint and command-start
owners and claims the original prepared pool once. Crate-private pool access
restricts CPU writes to the first four payload slots and four command TREs;
zero/event/table/other-channel memory has no CPU write path in this stage.
Completion retains the pool and records a stage status, never production
DMA or IPA readiness. A second coordinator cannot reclaim the pool.

Before publishing, the controller checks the exact shared-memory geometry,
all owned GSI states, masks and raw pending sources, the empty event producer,
all canaries, the private zero source and both staged/read-back descriptor
sets. It records local-memory preimage nonzero counts without modifying those
destinations from the CPU. The publication counter is sampled immediately
before the sole channel doorbell, after preparation, with a bounded one-second
native timeout. This diagnostic bound is native policy; Linux's memory
transaction waits without that deadline.

Only a real delivered GSI notification may enter completion handling. The
expected source is IEOB/event 0, with every foreign raw source quiet. The
disable order follows `gsi_isr_ieob`: type mask `44`, IEOB mask zero, clear
captured source `1`. The controller samples the producer and event twice,
requires the last TRE `80004030` and channel 1, then reports the consumed
event slot. It verifies all 1,619 local-memory words are zero and all 25
canaries are intact before re-enabling IEOB, writing the processing-context
register and checking final state twice. The native bounded event work runs
before the completion ACK. No ring space is reused or freed.

Ten host test groups cover this full path, every MMIO read/guard/uncertain
register store and every CPU read/uncertain descriptor store, non-sticking
descriptors, exclusive ownership, foreign or torn events, bad target/canary
data, timeouts and counter wrap, late IRQ invalidation, ACK/containment errors,
and the entire pool write boundary. The expected eight register writes combine
the original compiled Linux IEOB ISR/re-enable helpers with the three compiled
memory/doorbell writes; their assembly into a bounded native dispatcher is
explicit host policy. The core builds for AArch64 without std.

The opt-in `sunfish-native-startup-ipa-transfer` backend retains the original
pool/domain/firmware and startup owners, orders uncached CPU/DMA observations,
rejects queued notifications before publication and accepts badge `8` only.
Its actual compiled whitelist admits only the fixed two doorbells, masks,
captured IEOB clear and local-context value. Seven new source mutations are
rejected, together with the inherited command/GSI/PAS/UC audits. It adds no
memory mapping or IRQ authority to the command-start profile. Build with
`sh platforms/sunfish/build-native-startup-preflight.sh transfer` and the
existing prepared private inputs. Any failure consumes the attempt, masks
the source, quarantines the original pool and retains mappings/power until
reboot. The image starts no MPSS/WLAN and flashes nothing.

The first physical memory-command transaction subsequently passed, as
recorded below. Table initialization must still precede exception
RX/default-route setup and IPA QMI.

The generated image is 27,815,936 bytes, SHA-256
`dfc9d8770f0b035557720871c5e4bb4b88b073bafb24d7f2088b2c79cbdbd525`.
Actual AArch64 code generation contains the native transfer entry,
specialized preparation/publication/completion methods and physical-counter
instructions. The parsed system profile has identical maps, setvars and IRQs
to the command-start profile; the actual CapDL audit selects 2,273 frames and
rejects all 96 inherited mutations. Its scope explicitly permits one host
transfer and grants no production readiness. The 223 compiler input files
are captured for freezing with the source, proof and private-input snapshots.

Expected settled success is `MEMORY DMA PASS`: phase `3`, CPU
sent/done/check `30/30/30`, published `1`, DMA writes `8`, zero words `800`,
target words `653`, canary checks `32`, final samples `2`, event producer
`80003010`, event TRE `80004030`, channel `1`, one notice/ACK, badge `8`, last
write `1e8`, PAS returned `3f` and no defined SMMU faults. All display values
are hexadecimal. `BEFORE NONZERO` and the raw event code/type are observations,
not fixed success constants.

### Physical first-memory DMA pass

One cold temporary boot on 2026-10-26 was accepted. IMG_9178 shows
`MEMORY DMA PASS`, phase `3`, CPU sent/done/check `30/30/30`, published `1`,
eight register writes, zero-source words `800`, preimage nonzero words `653`
and verified target words `653`. Thus all 1,619 target words were nonzero
before the transaction and zero afterward. The CPU staging API cannot write
these IPA-local targets; it stages only private payloads/TREs. Canary checks
`32` mean all 25 canaries passed before and after; final samples are `2`.
The event producer/TRE/channel are `80003010/80004030/1`, one physical notice
and ACK, badge `8`, last write `1e8`, PAS returned `3f`, global FSR zero and
context FSR `400` (format bits only).

Raw event code/type `223` decodes to code `02`, type `23`. These are recorded
diagnostics, not an error inference or a newly invented completion predicate;
the pinned TX path identifies its transaction through channel/TRE. The exact
run, frozen image/source/inputs, boot transcript and original photo are saved
in `.cache/sunfish-native-wifi/private/device-runs/ipa-first-dma-20261026T203309Z/`.
This establishes the fixed private-pool command fetch, local-memory writes and
event-ring return through the installed domain. It does not establish use of
every installed stream/window, table setup, packet DMA, modem/WLAN startup or
production readiness.

Cleanup was confirmed in normal LineageOS through physical ADB. Android boot
completed, `system_server` was running and the fresh boot ID was
`d8330d26-0d40-4749-a2b9-d2d407e6979c`, replacing
`7810dfa7-fa00-438e-90c5-5e5de8ddcb79`.

## IPA v4.2 tables (offline encoding)

`sunfish_ipa_linux_tables` describes the next transaction from the same pinned
kernel. `ipa_table_setup()` reserves eight TREs but emits six for v4.2:
IPv4 route init (`7`), IPv6 route init (`8`), IPv4 filter init (`3`), unused
IPv4 filter space zero (`13`), IPv6 filter init (`4`) and unused IPv6 filter
space zero (`13`). Values here are hexadecimal. V4.2 has no hashed tables;
hashed addresses, sizes and payload bits stay zero.

Each route covers `78` bytes, at `3a0` and `428`. Each filter initializes
`18` bytes (bitmap `24` plus two rule addresses), at `290` and `318`, then
zeroes the unused `60` bytes. The 136-byte private source table contains the
zero rule, filter bitmap and fifteen references to the zero rule. Source
addresses skip the zero rule for filters (`80002008`) and also skip the
bitmap for routes (`80002010`). No canary, UC region or header is a target.

The plan uses the next six payloads at `80006080` through `80006120`, TREs
`80004040` through `80004090`, channel doorbell `800040a0` and event slot
`80003010`. The expected next producer is `80003020`. Those are planning
values, never transfer authority or evidence that the earlier transaction
has completed in a new boot.

`test-native-ipa-linux-tables.py` compiles the original endpoint/memory data,
table allocation/validation/setup functions, command/TRE encoders and ring
doorbells. All 136 source bytes, six payloads/TREs and both doorbell values
match. Four test groups cover exact nonhashed destination spans, preservation
of other memory/canaries, fresh ring slots, event validation and geometry;
four opcode/source/rule-address/cursor mutations fail the C comparison. The
core builds for AArch64 without std and is included in the regular regression
runner. The runtime and native integration below provide the next experiment;
there is no table-ready capability.

## Retained table transaction after memory DMA

`sunfish_ipa_linux_table_transfer::TableOnce` wraps the retained transfer
engine with the fixed six-command table plan. It requires the actual completed
memory-transfer owner, GSI/endpoint/command owners and the original pool with
memory setup complete. A public report or plan cannot substitute for these
owners. The pool admits one table claim and restricts CPU stores to payload
slots 4–9 and TRE slots 4–9. It cannot overwrite the prior transaction,
zero/table sources, event ring or other channels.

The second transaction requires the hardware producer to be `80003010` and
event slot 1 to be blank. It verifies all 2,048 zero-source words, all 34
table-source words, the existing processing-context value, the 25 canaries
and retained GSI states before publication. It stages 72 payload/TRE words
and checks them immediately and again before the channel doorbell. Completion
requires this boot's exclusive GSI notification, producer `80003020`, the
last TRE `80004090` on channel 1 and stable repeated event reads. The
controller verifies all 120 destination words against the table plan and
all canaries again, restores the IEOB enables, takes two final samples and
ACKs the physical IRQ before marking the pool's table stage complete. It
does not repeat the processing-context write.

The completion policy retains raw code/type fields and follows the same
pinned Linux channel/TRE checks as the proven first transaction. Any
uncertain operation, foreign/stale/extra event, timeout or later GSI delivery
consumes the attempt, masks GSI and quarantines the retained pool. No resource
is freed, reused, rolled back or retried.

Seven additional behavioral groups run the actual memory transaction first
and then test table success, prerequisites/exclusivity, every read/guard/store
failure boundary, corrupt sources, stale/torn/foreign completions, incorrect
table destinations, timeout/ACK/containment failures, late IRQs and the full
pool store boundary. The original ten memory-DMA groups remain in the same
suite. The seven expected table MMIO writes combine the compiled original
table doorbells with the original GSI IEOB ISR and re-enable helper.

`pixel_ipa_tables.rs` retains `TableOnce` before any I/O and repeats the
actual domain/pool/PAS/config/endpoint and completed memory-owner guards.
Its closed native write whitelist rejects all IPA context writes and the
prior transaction's doorbell values. It uses physical-counter timing, one
second after publication, the existing IRQ 464/channel 3 capability and no
initial rearm. Nine source mutations and an exhaustive compiled whitelist
check cover the new bindings, in addition to inherited audits.

Build the temporary experiment with:

```sh
SUNFISH_PRIVATE_INPUT_DIR=/path/to/prepared-private-inputs \
  sh platforms/sunfish/build-native-startup-preflight.sh tables
```

It repeats the proven native startup and memory DMA before the table
transaction. The parsed system mappings and IRQ authority match the first
DMA profile. The physical table result below establishes this fixed transaction;
exception RX/default route, IPA QMI, modem and WLAN startup remain.


## Table initialization: cold device result

The single cold temporary boot on 2026-10-26 passed on Sunfish. The image was
27,832,320 bytes, SHA-256
`e9a30163cd227eda580f714af21150b3de932805906d9de85ed506ae0714dab4`.
`IMG_9183.heic` shows `NATIVE IPA TABLE DMA` / `TABLE DMA PASS`. The first
memory stage passed again, followed by all six fixed route/filter commands.
All 72 staged words were checked, all 120 destination words matched and the
25 SRAM canaries passed before and after (50 checks). The producer was
`80003020`, last TRE `80004090`, channel 1. One physical GSI completion was
received and ACKed, badge 8. Global FSR was zero; context FSR `400` contains
no defined fault flags. Raw event code/type `223` remains diagnostic.

The frozen source, private inputs, exact image, identity and boot transcript,
original photo, PNG and transcription are preserved in
`.cache/sunfish-native-wifi/private/device-runs/ipa-tables-20261026T214826Z/`.
The attempt is consumed and must not be retried. Cleanup was confirmed via
physical ADB: LineageOS boot completed, system_server was running and the
fresh boot ID was `5d90d1ca-57ac-4fef-88a6-b0f7cc1a5e94`. This result proves fixed IPA table initialization, not
exception receive, IPA QMI, modem/WLAN startup, network frames or readiness.
