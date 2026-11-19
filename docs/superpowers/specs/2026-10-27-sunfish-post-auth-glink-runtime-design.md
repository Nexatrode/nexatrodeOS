# Sunfish post-auth GLINK runtime design

## Intent

Continue the native Sunfish boot after the MPSS PAS4 authentication and
delivered Q6-ready milestones. Keep the live startup owners in the same seL4
protection domain, bring up the pinned Linux-compatible GLINK/QRTR transport,
and attach the existing modem service owners so the native boot can progress
toward WLFW and Wi-Fi.

This is one stage in the full native Wi-Fi effort. Success here does not mean
that WLFW initialized, WLAN firmware loaded, the radio associated, or network
frames moved.

## Current state

The current `mpss-auth` entry in `rust/src/pixel_mpss_auth.rs` performs PAS4,
waits for the delivered SMP2P ready notification, records both receipts, and
returns at `Phase::Ready`. Its notification hook currently consumes the shared
notification while the auth stage is started. The auth CapDL system binds
SMP2P and watchdog, but not the GLINK interrupt.

The repository has a native transport adapter in
`rust/src/sunfish_mpss_linux_transport_native.rs`, the bounded GLINK/QRTR
transport cores, and individual RMTFS, TFTP, SERVREG, and IPA QMI owners. The
current tree has no same-boot root runtime that transfers the live auth state
into that transport and services those owners together. `Cargo.toml` has no
combined auth-plus-GLINK runtime feature.

The current `pixel_linux_rmtfs::Factory::step` also creates a `Session` that
does not fit the post-auth runtime unchanged: it registers RemoteStorage,
ServiceRegistry, and IPA-server endpoints and marks all three available before
the live transport owner is attached; it enables the Linux IPA client and
prepares a TFTP server at port `0x5000`. QRTR can publish ready services after
HELLO, and duplicate endpoint registration fails. A runtime that merely avoids
calling `prepare_tftp_*` would therefore still inherit a ready TFTP endpoint,
could announce RMTFS before its RAM-backed server is bound, and could either
publish the IPA-server endpoint or fail when staging SERVREG. The post-auth
profile must construct a different `Session` state while preserving the
existing auth-only profile.

The MPSS-auth v2 `fastboot boot` command completed and the settled screen photo
is now recorded at
`.cache/sunfish-native-wifi/private/device-runs/mpss-auth-20261027T192834885146Z/artifacts/settled-screen-result.json`.
The screen reports `MPSS AUTH READY PASS`, loader phase `AuthReturned`, PAS
attempted/returned masks `0x3f/0x3f`, matching target write/verify counts
`0x0966fbf5`, zero loader/native/SMP/kernel errors, one SMP notification and
ACK, one delivered-ready observation, and present auth/ready receipts. It also
confirms that GLINK and WLAN were not started. This establishes PAS4 return and
delivered Q6 readiness for the next same-cold-boot stage; it does not establish
GLINK, WLAN, association, or network-frame success.

The current firmware audit in `platforms/sunfish/wlan-resources.json` has no
approved TFTP source: the handset image's project-use authorization is
unverified, the community Sunfish rehost does not declare a binary license,
and the upstream WCN3990 candidates are generic, Shikra, and QCM2290 rather
than a verified Sunfish match. The current upstream `WHENCE` still lists
those platform-specific WCN3990 entries; none establishes a Sunfish source.

The startup ordering is source-verified against the pinned Linux commit
`2267e248204e7e051e0f7355298949a6440de579`: `sm7150.dtsi` binds
`remoteproc_mpss` to `qcom,sm7150-mpss-pas` and declares the GLINK edge as
GIC SPI 449; `qcom_q6v5_pas.c:qcom_pas_start` performs PAS auth/reset and waits
for Q6 ready before returning; `remoteproc_core.c:rproc_start` starts
subdevices only after the remoteproc `start` callback returns; and
`qcom_common.c:glink_subdev_start` then registers the SMEM GLINK transport.
This validates the reference ordering for PAS/Q6-ready before GLINK. It is
source evidence, not proof that the native runtime or device integration works.

## Goals

- Preserve the one-use PAS authentication and delivered-ready receipts, and
  retain the existing memory, power, SMMU, SMEM, and service owners for the
  entire modem session.
- Continue in the same cold boot through Linux-compatible GLINK VERSION and
  IPCRTR OPEN negotiation, QRTR HELLO, and bounded service discovery.
- Attach and service the existing RAM-backed RMTFS, SERVREG, and IPA QMI
  owners using their real serialized transport session and deadline source.
- Build a runtime-specific session profile: register RemoteStorage but leave
  it unavailable until the actual RAM-backed RMTFS server is bound; leave
  SERVREG unregistered until the existing owner stages it; do not register an
  IPA-server service endpoint; retain the Linux IPA client setup required by
  the selected reference; and create no TFTP endpoint or prepared TFTP server.
- Keep any immutable TFTP source provider outside the QRTR session and private
  to the retained runtime until a later, separately authorized WLFW step.
- Route co-delivered GLINK, SMP2P, watchdog, IPA, GSI, and SMMU events without
  dropping a pending event or acknowledging an unowned interrupt.
- Make startup milestones and the first failure visible and retain resources
  after uncertain writes or transport failure.
- Keep subsequent WLFW initialization, WLAN firmware loading, association,
  and packet traffic as explicit later milestones in the overall goal.

## Non-goals

- No flash, automatic retry, PDR restart, modem reset retry, or cleanup that
  treats logical failure as hardware quiescence.
- No change to the working LineageOS or Linux comparison boot.
- No WLAN start, CE/HTC/WMI/HTT data path, SSID association, or packet claim in
  this stage.
- No TFTP service publication, RRQ response, or WLAN firmware bytes until a
  Sunfish-compatible source and applicable use authorization are selected.
- The existing auth-only `Session` profile must remain unchanged; runtime
  hardening applies only to the new combined post-auth profile.
- No new authority to call SCM/SMC, change SMMU routes, access firmware payload
  after PAS submission, or write SMP2P fatal/stop state.
- No production-ready token based only on transport or service discovery.

## Approaches

### One retained startup PD — selected implementation call

Continue the existing root `Startup` owner after `Phase::Ready`. Transfer the
sealed auth and ready receipts, runtime-profile GLINK session, and service
providers once into a post-ready runtime. Add the GLINK IRQ to the same CapDL
owner and extend the shared notification dispatcher to route each badge to its
actual owner. Move the sole installed native SMMU/SMEM adapter by value from
the probe into an owned checked gate in `RuntimeDomain`. The gate wraps the
single adapter in `core::cell::RefCell`; every receive or GLINK operation uses
a short `try_borrow_mut` closure and releases it before returning. The
transport/runtime phase may own the gate through `BootTransport` and then the
existing modem `Owner`, but it stores no reference back into `Startup`, no
cloned adapter, and no outstanding `Ref`/`RefMut`. A failed borrow is
terminally quarantined as an ownership/reentrancy fault.

This keeps the already-live hardware and software state under one serialized
owner. It does require careful event routing and a larger root startup loop.
This is the implementation call because the current boot already retains the
auth state, transport inputs, and service resources together.

### Separate transport PD in the same boot

Move GLINK and service handling to another PD and pass one-use receipts,
capabilities, channel state, and service buffers across an explicit IPC
boundary. This gives the transport a smaller address space, but needs a new
handoff protocol and fault coordination for resources whose hardware lifetime
continues across that boundary. There is no existing post-auth IPC contract in
the current tree.

A separate second boot is excluded: it would restart the modem and invalidate
the authentication and ready observations this stage must consume.

## Runtime sequence

1. Require the actual PAS-returned and delivered Q6-ready receipts plus the
   retained owner health checks. Consume each receipt once.
2. Bind GLINK IRQ 481 (GIC SPI 449) to a dedicated notification badge while
   preserving SMP2P IRQ 483 and watchdog IRQ 298 ownership.
3. Move the sole native SMMU/SMEM adapter into the checked `RuntimeDomain`
   gate and start the native GLINK transport with that owner, the
   runtime-profile session, the authentication receipt, and the physical
   counter. Transport and receive paths use scoped checked mutable borrows;
   no reference to a sibling field is stored.
4. Complete VERSION negotiation and IPCRTR OPEN/ACK in either source-supported
   order. Learn the remote channel ID from the peer; do not assume it.
5. Handoff the live FIFO and session without resetting cursors, then process
   QRTR HELLO, server announcements, and bounded control packets.
6. Before QRTR control starts, call the existing owner's
   `prepare_service_registry()` exactly once; it registers and stages SERVREG,
   which QRTR publishes after HELLO. Do not then call
   `enable_service_registry()`, because that API registers the same endpoint
   and would fail on a duplicate. Bind the actual RAM-backed RMTFS server
   before making RemoteStorage available, then run the existing IPA QMI
   client. The session contains no IPA-server or TFTP endpoint. Preserve
   replies under transport backpressure and schedule the selected Linux
   deadlines from the physical counter. Keep any TFTP source provider private
   and unreachable.
7. Stop at the bounded QRTR service-discovery milestone, before WLFW commands
   or any WLAN firmware request/transfer. Display auth/ready, GLINK, QRTR,
   service, and error milestones. Keep retained resources if any operation is
   uncertain.

## Constraints and calls for the builder

- Preserve the pinned Linux boot order: PAS4 authentication and Q6-ready
  precede GLINK startup unless exact pinned source evidence requires another
  ordering.
- Verify IRQ numbers and trigger types against the pinned Sunfish device tree
  and update the CapDL/system audit together.
- Keep an offline source-order check tied to the pinned `qcom_pas_start`,
  `rproc_start`, and `glink_subdev_start` call sequence; reject a changed
  reference sequence instead of assuming the order.
- The only runtime authorization is the actual retained owner and sealed
  receipts; counters, display values, or host-model state cannot mint them.
- A `RefCell` borrow is a local exclusivity check, not synchronization: the
  runtime remains single-threaded. Use `try_borrow_mut`, never panic on a
  conflict, never retain a borrow over a callback, and quarantine the existing
  owner if a borrow fails.
- Any uncertain GLINK publication, service reply, IRQ ACK, or owner borrow
  quarantines the transport and retains the owners until reboot or proven
  hardware quiescence.
- Keep the post-auth image temporary and one-shot. Do not prepare or send it to
  the Pixel until the offline implementation, CapDL audit, AArch64 build,
  frozen input audit, and source review pass.

## Acceptance evidence

Offline evidence must include tests for the real auth-to-runtime handoff,
receipt single-use, checked-gate single ownership and borrow-conflict
quarantine, GLINK IRQ routing and
co-delivery, both VERSION/OPEN orders, QRTR/service-owner attachment,
backpressure, deadlines, and terminal quarantine. The generated CapDL model
must prove the exact new IRQ authority and unchanged prior resource ownership.
Tests must also prove the runtime-specific session has no TFTP endpoint or
TFTP preparation, does not publish an IPA-server endpoint, leaves
RemoteStorage unavailable until the live server is bound, cannot publish a
TFTP service, and cannot emit a firmware DATA response. They must prove the
auth-only session profile is unchanged. The concrete AArch64 entry, pinned
source/input audit, and frozen image audit must pass.

Device evidence, when authorized by the offline gate, must show the same
cold-boot image reaching PAS4 return, delivered Q6-ready, GLINK open, QRTR
HELLO, and actual service discovery, with TFTP still unpublished. A successful
`fastboot boot` transcript is insufficient. Later device checks must select a
source-backed, authorized Sunfish WLAN image before enabling TFTP, then prove
WLFW, WLAN association, and bidirectional network frames to complete the
overall goal.
