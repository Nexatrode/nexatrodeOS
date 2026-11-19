# Sunfish Post-Auth GLINK Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Continue the retained Sunfish seL4 startup from verified MPSS PAS4/Q6-ready receipts through GLINK/QRTR service discovery, without publishing WLAN TFTP or claiming Wi-Fi operation.

**Architecture:** Keep the existing startup PD and its already-live owners. Consume the actual auth/ready receipts once, move one checked `RefCell` gate containing the single SMMU/native adapter into a non-self-referential runtime, and attach the existing GLINK/QRTR service owners. Each native operation uses a short `try_borrow_mut` closure; no adapter clone or stored borrow is allowed. Add the GLINK IRQ to the same CapDL owner and quarantine retained resources on any uncertain operation or borrow conflict.

**Tech Stack:** Rust 2024, seL4 Microkit, CapDL/system XML, AArch64 target, pinned Sunfish Linux 4.14.357 protocol behavior, Python source and capability audits.

**Spec:** `docs/superpowers/specs/2026-10-27-sunfish-post-auth-glink-runtime-design.md`

## Global Constraints

- PAS4 authentication and the delivered Q6-ready notification precede GLINK startup.
- Consume the actual generation-1 auth and ready receipts once; display values and counters cannot create authority.
- Retain the existing IPA/SMMU, SMEM, RMTFS, power, and modem owners through the whole session; use one serialized native mapping owner.
- Keep the native adapter behind one runtime-owned checked `RefCell` gate. Use `try_borrow_mut`, release the guard before callbacks, and treat conflicts as terminal; this remains a single-threaded runtime, not synchronized shared access.
- Add only the source-verified GLINK IRQ 481 / GIC SPI 449 on notification channel 4; preserve existing channels 0–3, SMP2P channel 5, and watchdog channel 6.
- Stop at bounded QRTR service discovery. Do not issue WLFW operations, publish TFTP, answer RRQ, expose WLAN firmware bytes, start WLAN, or claim association/network traffic.
- Construct a runtime-specific GLINK `Session`; do not reuse the auth-only profile that premarks services ready and prepares TFTP.
- Do not flash, retry PAS, reset MPSS, reclaim ownership, or release retained resources after an uncertain write, IRQ ACK, or transport operation.
- The predecessor MPSS-auth run has recorded PAS4 return and delivered Q6-ready evidence; do not boot a new image until the offline build/audits pass and the frozen artifact is reviewed.
- Tie the PAS/Q6-ready-before-GLINK ordering to pinned Linux commit `2267e248204e7e051e0f7355298949a6440de579`: `qcom_pas_start` waits for ready, remoteproc starts subdevices after its `start` callback, and the GLINK subdevice registers SMEM transport.

## Review Focus

- Missing, stale, or replayed PAS/Q6-ready receipts must prevent GLINK startup and leave owners retained.
- GLINK VERSION and IPCRTR OPEN may arrive in either source-supported order; use the peer-selected channel ID.
- Co-delivered IPA, GSI, SMMU, GLINK, SMP2P, and watchdog notifications must be preserved and routed to their actual owners before a modem failure is made terminal.
- Transport backpressure or a silent peer must preserve queued work and use the physical-counter deadline without repeating an operation with uncertain effects.
- QRTR may discover TFTP, but must never publish a TFTP service, answer RRQ, or emit WLAN firmware DATA in this stage.

---

### Task 1: One-use auth handoff and shared native owner

**Files:**
- Modify: `rust/src/pixel_mpss_auth.rs`
- Modify: `rust/src/pixel_smmu_install.rs`
- Modify: `rust/src/pixel_ipa_receive.rs`
- Modify: `rust/src/sunfish_mpss_linux_transport_native.rs`
- Create: `rust/src/sunfish_mpss_live_domain.rs`
- Test: `tests/pixel_mpss_auth_root_test.rs`
- Test: `tests/pixel_mpss_auth_lifecycle_cases.rs`
- Test: `tests/sunfish_mpss_linux_transport_test.rs`

**Interfaces:**
- Produce `pixel_mpss_auth::take_runtime(&mut Startup) -> Result<RuntimeParts, Fault>`; `RuntimeParts` carries the actual retained boot, PAS-returned receipt, delivered-ready receipt, sole moved native gate, runtime-profile `Session`, extracted receive runtime, physical-counter frequency, and pending/deferred notification badges. It does not carry a TFTP server or image provider.
- Produce a non-self-referential `RuntimeDomain` that takes the sole native gate by value from `pixel_smmu_install::Probe`. The gate owns `RefCell<Native>` and offers only scoped `try_borrow_mut` closures. `RuntimeDomain` owns the transport phase (`BootTransport` then existing `qcom_modem_owner::Owner`) and receive runtime; neither phase stores a borrow back into `RuntimeDomain` or `Startup`.
- Keep the gate in one phase owner only. Expose receive access through that owner's scoped gate method; no second gate/adapter copy, `Rc`, allocator, persistent `Ref`/`RefMut`, or sibling-field reference. A borrow conflict permanently quarantines the shared owner.
- Build the `Session` under the new combined-runtime feature: register RemoteStorage at `0x4000` but leave it unavailable; do not pre-register SERVREG or IPA-server; retain only the prepared Linux IPA client endpoint; and do not prepare TFTP or set a TFTP port. Leave the existing auth-only profile unchanged.
- Make `sunfish_mpss_linux_transport_native::Retained` carry the sole gate by value and expose a controlled scoped-borrow path to the runtime's receive checks. No long-lived reference into another `Startup` field is stored; failures or borrow conflicts quarantine the gate permanently.
- Produce `transport::start_live(gate, auth, session, hz) -> BootTransport<LiveAccess>` using that gate and the actual PAS-returned receipt. Keep and validate the delivered-ready receipt separately before starting this transport; retain the extracted receive runtime inside `RuntimeDomain` and dispatch it through the same gate.

- [x] **Step 1: Add failing lifecycle tests** for handoff before `Phase::Ready`, absent auth/ready receipt, wrong generation, second handoff, and valid handoff. Assert each invalid case refuses GLINK and retains/quarantines owners.
- [x] **Step 2: Run `python3 platforms/sunfish/test-native-mpss-auth-root.py`** and confirm the new lifecycle cases fail for the missing handoff.
- [x] **Step 3: Implement `RuntimeParts` and `take_runtime`** in `pixel_mpss_auth.rs`. Set `Phase::Delegated` before moving any owner; move receipts and retained resources with one-use `Option`/`ManuallyDrop` transitions.
- [x] **Step 4: Add failing checked-gate tests** for receive/GLINK sequential interleaving, nested borrow rejection, mapping identity change, transport failure, and attempted second extraction. Assert one gate identity, no persistent borrow, and permanent quarantine after conflict/failure.
- [x] **Step 5: Implement `RuntimeDomain` and `start_live`** in the live-domain and transport adapters. Move the gate out of the probe once, retain it through the `BootTransport` then `Owner` phase, and expose only closure-scoped `try_borrow_mut` access to receive/GLINK operations; store no reference from one `Startup` field into another.
- [x] **Step 6: Run `python3 platforms/sunfish/test-native-mpss-auth-root.py` and `python3 platforms/sunfish/test-native-mpss-linux-transport.py`**; expect one-use receipt and single-owner cases to pass and unchanged pinned dependencies.

### Task 2: Reuse the existing bounded QRTR service owners

**Files:**
- Modify: `rust/src/pixel_linux_rmtfs.rs`
- Modify: `platforms/sunfish/audit-native-mpss-runtime-source.py`
- Modify: `platforms/sunfish/validate-native-mpss-runtime.py`
- Create: `tests/sunfish_mpss_runtime_session_cases.rs`
- Test: `tests/sunfish_mpss_linux_glink_cases.rs`
- Test: `tests/sunfish_glink_qmi_test.rs`
- Test: `tests/sunfish_tftp_test.rs`
- Create: `platforms/sunfish/test-native-mpss-runtime-services.py`

**Interfaces:**
- Reuse `qcom_modem_owner::Owner::attach_linux_ipa(adapter, generation, shared, counter_hz)` and `start_control()` for QRTR control; do not add a parallel service dispatcher.
- Before `start_control()`, call `prepare_service_registry()` exactly once. It registers and stages SERVREG for QRTR to publish after HELLO. Do not call `enable_service_registry()` afterward: that method registers the endpoint again and would hit the router's duplicate-registration error.
- Keep RemoteStorage registered but unavailable in the runtime session. Bind the actual retained RAM-backed RMTFS server first with `activate_linux_rmtfs()`, which marks it available only after binding succeeds.
- Drive bounded service work through the existing `dispatch_service_registry_step()`, `dispatch_linux_rmtfs_step()`, and `service_linux_ipa_step(now, interrupts)` methods. Preserve each existing owner’s queued reply and terminal-failure behavior.
- The runtime session must have no IPA-server endpoint, TFTP port, or prepared TFTP mode. Do not call either `prepare_tftp_observer()` or `prepare_tftp_server()`; attach no TFTP server/image provider to this runtime. The milestone is QRTR discovery with no TFTP service published or firmware response emitted.

- [x] **Step 1: Add failing session-profile tests** proving the combined-runtime `Session` has no TFTP port (using a read-only/test accessor if needed), no prepared TFTP server/observer, no IPA-server endpoint, SERVREG initially absent, and RemoteStorage registered but unavailable in the QRTR readiness report. Prove the auth-only profile remains unchanged.
- [x] **Step 2: Add failing integrated tests** for auth-to-Owner attachment, QRTR HELLO/server discovery, one-time SERVREG preparation and post-HELLO publication (without duplicate registration), RMTFS unavailable before actual server binding and available only after successful binding, IPA-client service, response backpressure, and physical-counter deadline handling.
- [x] **Step 3: Add negative assertions** that a TFTP RRQ cannot reach a server, no firmware DATA response can be emitted, and no WLFW operation starts.
- [x] **Step 4: Run `python3 platforms/sunfish/test-native-mpss-runtime-services.py`** and confirm the new session-profile and integration cases fail before implementation.
- [x] **Step 5: Implement the feature-gated runtime session profile in `pixel_linux_rmtfs.rs`** and integrate the existing `Owner` APIs with the retained transport adapter. Do not duplicate the Linux QMI/SERVREG/RMTFS state machines.
- [x] **Step 6: Update the source and CapDL audit result** so `wlan_firmware_tftp_serving_enabled` is `false`, `tftp_service_published` is `false`, no IPA-server endpoint is registered, and RemoteStorage is unavailable until its real server is bound.
- [x] **Step 7: Run `python3 platforms/sunfish/test-native-mpss-runtime-services.py`**; expect bounded discovery to pass and mutations that re-enable TFTP, publish RMTFS early, register IPA-server, or substitute a fake deadline to be rejected.

### Task 3: Compose runtime loop, IRQ dispatch, and exact CapDL authority

**Files:**
- Create: `rust/src/pixel_mpss_runtime.rs`
- Modify: `rust/src/pixel_mpss_runtime_irq.rs`
- Modify: `rust/src/sunfish_native_startup_preflight.rs`
- Modify: `rust/Cargo.toml`
- Modify: `platforms/sunfish/build-native-startup-preflight.sh`
- Create: `platforms/sunfish/native-startup-mpss-runtime.system`
- Modify: `platforms/sunfish/validate-native-mpss-runtime.py`
- Test: `tests/pixel_mpss_runtime_irq_test.rs`
- Test: `platforms/sunfish/test-native-mpss-runtime-irq.py`

**Interfaces:**
- Add exactly `sunfish-native-startup-mpss-runtime = ["sunfish-native-startup-mpss-auth", "sunfish-native-startup-mpss-glink"]` without changing existing feature dependencies.
- `pixel_mpss_runtime::start(&Display, &mut Startup)` consumes the auth handoff after `pixel_mpss_auth::start` reaches ready and returns; `pixel_mpss_runtime::notified(&Display, &mut Startup, ChannelSet)` routes each later notification through the combined runtime and existing service owners.
- The runtime object owns the in-progress `BootTransport` or the attached `qcom_modem_owner::Owner`, plus the extracted receive runtime. It processes bounded receive and GLINK work in sequence through one mutable domain owner, with no `Rc`, allocator, self-reference, or cloned native adapter.
- `pixel_mpss_runtime_irq::Router` owns logical badge bits 4/5 for GLINK/SMP2P; it forwards bits 0–3 unchanged to the retained IPA/GSI/SMMU owners and treats watchdog/unknown/duplicate notifications as terminal without ACK.

- [x] **Step 1: Extend failing IRQ-router tests** for co-delivery, bad/duplicate badges, wrong ACKs, pending-event preservation, and uncertain ACK no-retry.
- [x] **Step 2: Run `python3 platforms/sunfish/test-native-mpss-runtime-irq.py` and confirm the new co-delivery/integration cases fail.**
- [x] **Step 3: Implement `pixel_mpss_runtime::start` and `notified`**. Start the GLINK negotiation immediately after the auth function has returned with the real delivered-ready receipt; later `ChannelSet` notifications enter the combined router rather than the auth-only handler. Process bounded physical-counter deadlines without adding a new timer/IRQ capability. Display PAS/Q6-ready, GLINK VERSION/OPEN, QRTR HELLO/discovery, and first-error milestones, with an explicit no-WLAN claim.
- [x] **Step 4: Add IRQ 481 as channel 4** to `native-startup-mpss-runtime.system`; preserve IRQ 483/channel 5 and IRQ 298/channel 6. Do not add extra caps or ACK aliases.
- [x] **Step 5: Extend `build-native-startup-preflight.sh` with the `mpss-runtime` profile** and the exact Cargo feature extension; validate GLINK trigger/IRQ against the pinned Sunfish device tree.
- [x] **Step 6: Extend `platforms/sunfish/test-native-mpss-runtime-irq.py`** to reject changes to the pinned sequence: Sunfish uses `qcom,sm7150-mpss-pas`, `qcom_pas_start` waits for ready, `rproc_start` starts subdevices after the start callback, and `glink_subdev_start` registers GLINK SMEM.
- [x] **Step 7: Run `python3 platforms/sunfish/test-native-mpss-runtime-irq.py` and `python3 platforms/sunfish/validate-native-mpss-runtime.py` on the generated model**; expect only the exact IRQ 481 capability to be new and all previous resource owners unchanged.

### Task 4: Freeze and audit the post-auth image for review

**Files:**
- Create: `platforms/sunfish/build-native-mpss-runtime-integration.py`
- Create: `platforms/sunfish/freeze-native-mpss-runtime.py`
- Modify: `platforms/sunfish/audit-native-mpss-runtime-source.py`
- Modify: `platforms/sunfish/audit-native-mpss-runtime-layout.py`
- Modify: `platforms/sunfish/README.md`

**Interfaces:**
- The integration builder writes a private proof directory containing the source snapshot, host-test result, actual AArch64 ELF, CapDL model/report, and their SHA-256 inventory.
- The freezer emits a separate manifest and temporary boot image only after source, test, CapDL, input, and loader-layout audits pass. It performs no device action.
- The predecessor PAS4/Q6-ready photo is recorded in the prior run artifact. Keep the device gate closed until the offline gates pass; a later one-shot boot requires review of the frozen image and cannot flash or retry.

- [x] **Step 1: Add failing audit checks** that reject stale source hashes, extra capabilities, changed dependencies, firmware/TFTP service publication, an unbounded frame range, or packaging when the predecessor auth result is unresolved.
- [x] **Step 2: Implement the isolated integration builder and freezer** using the existing `audit-native-mpss-runtime-source.py`, `audit-native-mpss-runtime-layout.py`, and `validate-native-mpss-runtime.py` as gates.
- [x] **Step 3: Record output scope as `native_wifi_proven=false` and `production_ready=false`.** The artifact must say it stops at QRTR discovery and does not publish TFTP or start WLFW/WLAN.
- [x] **Step 4: Run `python3 platforms/sunfish/build-native-mpss-runtime-integration.py` and `python3 platforms/sunfish/freeze-native-mpss-runtime.py`**; inspect the frozen manifest, hashes, CapDL authority, and loader layout before the one-shot device check.

## Acceptance

- Offline tests prove one-use real auth handoff; checked-gate single ownership, sequential access, and borrow-conflict quarantine; both GLINK VERSION/OPEN orders; peer-selected channel ID; QRTR HELLO and bounded discovery; co-delivered IRQ forwarding; service backpressure/deadlines; terminal quarantine; and zero TFTP publication/RRQ/WLAN firmware data.
- The runtime session tests prove no inherited TFTP endpoint/preparation, no IPA-server publication, RemoteStorage unavailable until the actual RMTFS server is bound, and no change to the auth-only session profile.
- AArch64 and generated CapDL audits prove the runtime extension adds only IRQ 481/channel 4 and preserves the prior owner’s authority and mappings.
- The frozen image and inputs are hash-bound, temporary-only, and explicitly do not prove WLAN operation.
- Device validation is a later step, after the offline gates pass and the frozen runtime artifact is reviewed. It must show PAS4 return, delivered Q6-ready, GLINK open, QRTR HELLO, and service discovery while TFTP remains unpublished.
