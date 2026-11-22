# Pixel 4a (`sunfish`) seL4/Microkit bring-up

The UART handoff probe has booted on the Pixel. A separate seL4 16.0 /
Microkit 2.3.1 debug SDK and UART hello image now build from the exact live
Lineage device tree. The Microkit image is experimental: use **temporary
`fastboot boot` only**. Nothing from this directory should be flashed yet.

The old [`overlay.dts`](overlay.dts) is a mainline-based desk sketch. The
build uses [`sel4-overlay.dts`](sel4-overlay.dts) on the final merged Lineage
runtime FDT instead of the base DTB embedded in `boot_a`. See the measured addresses and backup notes in
[`docs/pixel-p0-boot-feasibility.md`](../../docs/pixel-p0-boot-feasibility.md).

For the WMI queue, CE ring and descriptor compatibility constraints, see the
[WMI and Copy Engine integration notes](../../docs/sunfish-wmi-ce-integration.md).

## Contents

| File | Role |
| --- | --- |
| [`sel4-overlay.dts`](sel4-overlay.dts) | Overlay for the live Lineage DT, including observed memory, timer, GICv3, and UART |
| [`sel4-config.cmake`](sel4-config.cmake), [`sel4-geni-uart.c`](sel4-geni-uart.c), [`sel4-constants.h`](sel4-constants.h) | seL4 board, GENI console, and CPU constants |
| [`prepare-microkit-sources.py`](prepare-microkit-sources.py) | Idempotently installs board glue in disposable upstream worktrees under `.cache/`; requires the final runtime FDT captured from `/sys/firmware/fdt`, including the active DTBO |
| [`build-microkit-sdk.sh`](build-microkit-sdk.sh) | Builds the debug board SDK with LLVM/LLD |
| [`uart-smoke.S`](uart-smoke.S) | Linux Image-shaped AArch64 UART/EL probe with PSCI reset |
| [`repack-boot-v2.py`](repack-boot-v2.py) | Temporary boot-v2 packer using the exact live Lineage ramdisk and DTB |
| [`build-uart-smoke.sh`](build-uart-smoke.sh) | Build the probe and boot-v2 image in `.cache/pixel-sunfish/` |
| [`hello.c`](hello.c), [`hello.system`](hello.system) | First Microkit UART marker and PSCI reset test |
| [`wrap-microkit-image.py`](wrap-microkit-image.py), [`build-microkit-hello.sh`](build-microkit-hello.sh) | Page-aligned loader wrapper and temporary boot image |
| [`capture-uart.py`](capture-uart.py) | 115200-baud capture holding one open serial descriptor |
| [`splash-probe.c`](splash-probe.c), [`splash-probe.system`](splash-probe.system), [`build-splash-probe.sh`](build-splash-probe.sh) | Read-only probe of the reserved continuous-splash memory |
| [`splash-pattern.c`](splash-pattern.c), [`splash-pattern.system`](splash-pattern.system), [`build-splash-pattern.sh`](build-splash-pattern.sh) | Temporary two-buffer black/white scanout test |
| [`slint-display.system`](slint-display.system), [`build-slint-display.sh`](build-slint-display.sh), [`validate-slint-smc-policy.py`](validate-slint-smc-policy.py) | Eight-screen Slint shell, touch service without generic SMC authority, isolated volatile Notes service/vault, and a build-time check that only the fixed-purpose reset watchdog receives SMC |
| [`touch-mmio-probe.c`](touch-mmio-probe.c), [`build-touch-mmio-probe.sh`](build-touch-mmio-probe.sh) | Read-only GENI SE7 touch-bus register probe |
| [`touch-i2c-ping.c`](touch-i2c-ping.c), [`build-touch-i2c-ping.sh`](build-touch-i2c-ping.sh) | Bounded address-only touch I²C probe; temporary boot only |
| [`foundation-probe.system`](foundation-probe.system), [`build-foundation-probe.sh`](build-foundation-probe.sh) | Bounded scheduler-budget and unmapped-memory isolation probe; temporary boot image |
| [`ipc-probe.system`](ipc-probe.system), [`build-ipc-probe.sh`](build-ipc-probe.sh) | Protected-call and missing-capability probe; temporary boot image |
| [`policy-probe.system`](policy-probe.system), [`build-policy-probe.sh`](build-policy-probe.sh) | Per-channel read/write authorization probe; temporary boot image |
| [`audit-capdl-memory.py`](audit-capdl-memory.py) | Check generated CapDL pages against fixed reserved-memory intervals in the merged Sunfish DT |
| [`wlan-resources.json`](wlan-resources.json), [`validate-wlan-resources.py`](validate-wlan-resources.py), [`smmu_runtime_route_readback.py`](smmu_runtime_route_readback.py), and [`fixtures/lineage-smmu-runtime-route-readback.log`](fixtures/lineage-smmu-runtime-route-readback.log) | Compare ICNSS and IPA assumptions against the DTBs; pin the Sunfish DVT overlay/base-DTS path; validate a read-only post-attach Lineage SMMU capture (ICNSS SID `0x240` at SME13/CB6, IPA-WLAN SID `0x521` at SME9/CB38, and USB CB reassignment). The Linux runtime capture is distinct from the ABL handoff and grants no SMMU authority to seL4; live ACTLR and reused-context policy remain unresolved |
| [`capture-lineage-ipa-state.sh`](capture-lineage-ipa-state.sh) | Read-only IPA geometry plus filtered modem, MDM-helper, Peripheral Manager server/client/proxy, CNSS, TFTP-server and WLFW boot-order capture; reports `ro.baseband`, `vendor.mdm_helper` service state/PID, runtime presence of `/vendor/bin/init.mdm.sh` and `/vendor/bin/mdm_helper`, `per_mgr`/`per_proxy` init timestamps, proxy modem-vote lines, and TFTP request logs; supports `--serial` when an emulator is attached; current Lineage kernel does not register debugfs, so IPA geometry files are absent even to root; boot log still confirms WMA firmware-ready |
| [`capture-lineage-subsys-owner.sh`](capture-lineage-subsys-owner.sh) | Read-only rooted snapshot of persistent process descriptors referencing `/dev/subsys_modem` or a firmware_name sysfs node; verifies one Sunfish Lineage device and existing root ADB, never restarts adbd, and warns that transient sysfs writes can be missed |
| [`wlan-protocol-probe.system`](wlan-protocol-probe.system), [`build-wlan-protocol-probe.sh`](build-wlan-protocol-probe.sh), [`wlan_protocol_probe.rs`](../../rust/src/wlan_protocol_probe.rs) | No-radio Microkit target self-test for WCN3990 CE, WLFW QRTR, IPA QMI, WMI 10.4 INIT, synthetic scan start/stop, and CCMP pairwise/group commands correlated with synthetic SEC_IND events through the generation-bound tracker; synthetic key bytes only, splash framebuffer only, no physical modem transport, firmware key install, SMC, IRQ, MMIO, DMA publication, or radio start |
| [`validate-ath10k-stop-scan.py`](validate-ath10k-stop-scan.py), [`ath10k_scan.rs`](../../rust/src/ath10k_scan.rs), [`ieee80211_scan_results.rs`](../../rust/src/ieee80211_scan_results.rs) | Source-audited WMI 10.4 `STOP_SCAN` encoder and WMI_CONTROL queue transition tied to active scan IDs; software enqueue rollback and aborted completion are covered by host tests |
| [`validate-ath10k-firmware-path.py`](validate-ath10k-firmware-path.py) | Read-only source audit for the pinned `firmware-name` subdirectory lookup, generic fallback order, `board-2.bin`/API-5 use, and the current Sunfish DTS selector state |
| [`validate-ath10k-board-data.py`](validate-ath10k-board-data.py), [`ath10k_board.rs`](../../rust/src/ath10k_board.rs) | Source-audited, bounded API-2 `board-2.bin` selector; constructs ordered Sunfish SNOC QMI board-name candidates from runtime IDs and the `google_sunfish` variant, returns borrowed board data, and keeps extended-board lookup on its separate BMI identity path; 12 parser tests plus a composed synthetic selected-payload-to-WLFW-BDF framing test |
| [`validate-wlan-launch-path.py`](validate-wlan-launch-path.py) | Read-only source audit for SM7150 `wlan_pd`/`tms/pdr_enabled` map semantics, the public Linux `qcom_pd_mapper` and userspace `pd-mapper` locator roles, APR's PDR-gated `kernel/elf_loader` client-device registration, LineageOS ICNSS PDR recovery versus initial startup and WLFW client discovery, pinned Sunfish init/module declarations and proprietary-blob manifest, ath10k's WLFW startup order, M3_INFO versus firmware-log INI semantics, and the current absence of a source-backed Sunfish WLAN image launcher/provider. It includes the public `ro.baseband`-gated `mdm_helper` declaration and verifies that public product makefiles omit `init.mdm.sh` while conditionally inheriting private vendor makefiles; the separate saved rooted boot capture shows the binary missing in one boot, and private vendor launch behavior remains unproven; it also checks the pinned Lineage /dev/subsys_modem open-to-SSR/PIL modem.mdt path, while recording that the same firmware-name log marker has two source call sites and the runtime caller remains unknown; blob manifests are not treated as license terms |
| [`validate-icnss-startup.py`](validate-icnss-startup.py) | Pinned Lineage source audit separating ICNSS WLFW server-arrival setup from guarded PDR recovery; explicitly records that the private initial `wlan_pd` launcher is still unidentified |
| [`validate-lineage-modem-startup-order.py`](validate-lineage-modem-startup-order.py), [`fixtures/lineage-modem-startup-order-20261025.log`](fixtures/lineage-modem-startup-order-20261025.log), [`fixtures/lineage-modem-startup-order-20261025-probe.log`](fixtures/lineage-modem-startup-order-20261025-probe.log) | Preserve and validate two filtered rooted Lineage boot timelines: `/vendor/bin/mdm_helper` was missing in both, while `modem_svc`, TFTP, `pd_mapper`, and `per_proxy` preceded MPSS reset release, `wlan_pd UP`, and WLAN FW READY; ordering does not establish which service launched either domain |
| [`parse-wmi-init-resource-dump.py`](parse-wmi-init-resource-dump.py), [`sunfish-wmi-init-resource-dump.patch`](../../patches/sunfish-wmi-init-resource-dump.patch), [`validate-qcacld-wmi-resource-policy.py`](validate-qcacld-wmi-resource-policy.py) | Source-matched diagnostic patch and offline decoder for the exact 276-byte QCACLD WMI INIT resource TLV, naming all 68 words and validating field order/runtime inputs against pinned source; exact TLV and active service bitmaps captured from LineageOS with matching module ABI. A read-only LineageOS 23.2 log capture on kernel `4.14.357-openela-g28f9290ae067` corroborates the WMI build/ABI, board and hardware versions, config path, and READY status 0; this still does not identify the private initial `wlan_pd` image launcher |
| [`build-modem-tftp-observer-probe.sh`](build-modem-tftp-observer-probe.sh), [`loader-task-regions-modem-tftp-observer.json`](loader-task-regions-modem-tftp-observer.json) | Temporary read-only SERVREG and passive QRTR observer for IPA/WLFW announcements and TFTP request metadata; no QMI client request, TFTP reply, firmware source, or partition flashing |
| [`qcom_tftp.rs`](../../rust/src/qcom_tftp.rs), [`qcom_qrtr_tftp.rs`](../../rust/src/qcom_qrtr_tftp.rs) | Bounded read-only TFTP and QRTR transfer state over fixed-extent sources; `ReadSourceCell` adapts mutable storage I/O with live extent checks, reads only negotiated blocks, quarantines source faults without retry, and keeps authorized-image constructors test-only until a licensed trusted provider exists |
| [`modem-servreg-state-probe.system`](modem-servreg-state-probe.system), [`build-modem-servreg-state-probe.sh`](build-modem-servreg-state-probe.sh), [`loader-task-regions-modem-servreg-state.json`](loader-task-regions-modem-servreg-state.json) | Temporary one-query SERVREG trace attributes reply-enqueue waits by LOC name and reports aggregate `FlowBusy` / `NoCapacity` counts plus first-reply QRTR `RESUME_TX`; 2026-10-23 photo shows prerequisite stopped in RSC/MC0 with `StartingValues` mismatch before secure auth, so query was not reached; no listener/PDR, WLAN DMA, or WLAN firmware transfer |
| [`ipa-read-probe.system`](ipa-read-probe.system), [`build-ipa-read-probe.sh`](build-ipa-read-probe.sh), [`validate-ipa-read-probe-source.py`](validate-ipa-read-probe-source.py), [`sunfish_ipa_read_probe.rs`](../../rust/src/sunfish_ipa_read_probe.rs) | IPA v4.2/GSI v2.2 register-snapshot candidate; read-only mapping is not sufficient because Linux applies the IPA SVS2 bandwidth vote before MMIO; the build script refuses a bootable image until seL4 has a source-matched IPA bandwidth-vote provider; the prior cached image is quarantined |
| [`sunfish_ipa_post_init.rs`](../../rust/src/sunfish_ipa_post_init.rs), [`sunfish_ipa_bus_vote.rs`](../../rust/src/sunfish_ipa_bus_vote.rs), [`sunfish_ipa_snapshot.rs`](../../rust/src/sunfish_ipa_snapshot.rs) | One-shot IPA readiness coordinator applies the source-ordered SVS2 bandwidth usecase before register mapping; Sunfish's bandwidth-vote path has no GCC core-clock handle and IPA v4.2 uC clock notification is a no-op; pinned-source audit and host tests pass, but no production bus/MMIO provider exists, so IPA and WLAN remain disabled |
| [`sunfish_ipa_fws.rs`](../../rust/src/sunfish_ipa_fws.rs), [`sunfish_ipa_pas.rs`](../../rust/src/sunfish_ipa_pas.rs) | Source-matched PAS-0xf MDT placement, staging, and fail-stop PIL sequence model for active IPA-FW region `0x98f80000..0x98f90000`; generic adapter validates sealed request classes and brackets only INIT/AUTH with a shared-vote lease, covered by host tests; no production vote/SMC provider, firmware payload, physical memory access, or subsystem start |
| [`sunfish_qcacld_rx.rs`](../../rust/src/sunfish_qcacld_rx.rs), [`validate-sunfish-qcacld-rx.py`](validate-sunfish-qcacld-rx.py) | Bounded parser for the pinned Sunfish QCACLD HTT RX descriptor; yields only completed single-buffer MSDUs, keeps target L3 padding explicit, and refuses chains/extensions instead of flattening them; six host tests, no DMA or radio access |
| [`sunfish_htt_rx_ring.rs`](../../rust/src/sunfish_htt_rx_ring.rs) | Source-matched HTT ring setup and serialized one/batch refill transaction; validates all buffers, writes entries, applies one barrier, and publishes one final allocation index before committing session ownership; halts on ambiguous publication; host-tested, no production cache/DMA or hardware provider |
| [`sunfish_qcacld_rx_session.rs`](../../rust/src/sunfish_qcacld_rx_session.rs), [`sunfish_wlan_dma.rs`](../../rust/src/sunfish_wlan_dma.rs) | Fixed-capacity QCACLD RX ownership with FIFO leases and source-layout 802.3 MSDU copies; consumes per-MSDU firmware descriptor bytes from RX_IND and resolves validated active leases to initial/fallback IOVA windows; host-tested only, no DMA synchronization or device-ring authority |
| [`sunfish_qcacld_rx_delivery.rs`](../../rust/src/sunfish_qcacld_rx_delivery.rs), [`sunfish_qcacld_rx_pipeline.rs`](../../rust/src/sunfish_qcacld_rx_pipeline.rs), [`sunfish_wlan_net_sink.rs`](../../rust/src/sunfish_wlan_net_sink.rs) | Composes completed CE1 HTC/HTT indications through FIFO MSDU groups to a synchronous Ethernet sink, with per-buffer CPU sync, batched refill IDs, EAPOL-only unauthenticated-peer policy, and lease cleanup; `test-wlan-net-bridge-ffi.py` runs the synthetic RX pipeline through the Rust sink and actual C bridge into pinned sDDF queues; no production DMA/cache provider or Gate |
| [`qcom_ce_v3_native_io.rs`](../../rust/src/qcom_ce_v3_native_io.rs), [`qcom_ce_v3_regs.rs`](../../rust/src/qcom_ce_v3_regs.rs) | Guarded WMI CE2 and HTT-data CE1 register paths require only the exact 48 KiB CE aperture at `0x18a40000`; CE1 and CE2 have same-generation, route- and exclusive-binding-gated copy-complete W1C acknowledgement. Runtime CE publish/post/consume and IRQ-mask operations validate current ownership, power/reset, mappings and translated routes while CE is active; host-tested, no seL4 IRQ capability/dispatcher, production Gate, or device run |
| [`qcom_htc.rs`](../../rust/src/qcom_htc.rs), [`qcacld_wmi.rs`](../../rust/src/qcacld_wmi.rs), [`sunfish_qcacld_wmi_irq_microkit.rs`](../../rust/src/sunfish_qcacld_wmi_irq_microkit.rs), [`sunfish_qcacld_wmi_rx.rs`](../../rust/src/sunfish_qcacld_wmi_rx.rs), [`sunfish_qcacld_wmi_rx_native.rs`](../../rust/src/sunfish_qcacld_wmi_rx_native.rs), [`sunfish_qcacld_wmi_startup.rs`](../../rust/src/sunfish_qcacld_wmi_startup.rs), [`validate-sunfish-htc-credit.py`](validate-sunfish-htc-credit.py), [`validate-sunfish-qcacld-wmi-rx.py`](validate-sunfish-qcacld-wmi-rx.py) | Separates HTC send/receive flags, parses source-matched credit trailers, and dispatches strict QCACLD READY/SERVICE_READY/SERVICE_AVAILABLE, scan, and management-TX events. The native CE2 service masks, drains/dispatches within a fixed budget, acknowledges only after observing an empty ring, and re-arms only with posted buffers remaining; the target-only Microkit adapter binds that service to its bounded IRQ/continuation loop. An accepted SERVICE_AVAILABLE can now trigger WMI_INIT planning and CE3 publication inside the post-observation/pre-CE2-ACK hook, using owned startup snapshots and live HTC credits; the 160-test native CE suite covers the exact DMA publication and state transition. Host regressions and an AArch64 seL4 Microkit target check pass. No production Gate/system composition, IRQ capability binding, or radio path is wired |
| [`sunfish_qcacld_tx.rs`](../../rust/src/sunfish_qcacld_tx.rs), [`sunfish_qcacld_htc.rs`](../../rust/src/sunfish_qcacld_htc.rs), [`sunfish_qcacld_wmi_control.rs`](../../rust/src/sunfish_qcacld_wmi_control.rs), [`sunfish_qcacld_wmi_rx.rs`](../../rust/src/sunfish_qcacld_wmi_rx.rs), [`qcom_ce_v3_native_io.rs`](../../rust/src/qcom_ce_v3_native_io.rs), [`validate-sunfish-qcacld-tx.py`](validate-sunfish-qcacld-tx.py) | Source-audited startup transaction connects `HTT_DATA_MSG_SVC` (`0x0300`) through HTC control CE0 before WMI pre-enable; WMI control construction requires the same-generation HTT connection and matching HTC_READY payload. Dynamic endpoints bind only after validated CE2 responses. The WMI startup session retains both negotiated endpoints for its generation and clears them on generation advance. Gated CE0 publication accepts only canonical staged frames, and the combined native test runs HTT connect, WMI connect, and setup-complete in source order over one generation. No production CE/SMMU Gate, CE2 IRQ/system dispatcher, or radio access |
| [`sunfish_qcacld_resource.rs`](../../rust/src/sunfish_qcacld_resource.rs), [`validate-qcacld-wmi-resource-policy.py`](validate-qcacld-wmi-resource-policy.py) | Source-derived 68-word QCACLD WMI INIT resource profile with explicit effective runtime overrides; tests feed captured Lineage base and extended service bitmap words and reproduce all 68 live resource words and the Linux TLV header. Generation-bound snapshots compose into a 312-byte WMI body and 324-byte HTC frame after receive-buffer reuse. Mock CE3 publication is tested; no production Gate or device submission has been exercised. The stock driver reports WMI READY status 0 |
| [`sunfish_qcacld_mgmt_tx.rs`](../../rust/src/sunfish_qcacld_mgmt_tx.rs), [`validate-sunfish-qcacld-mgmt-tx.py`](validate-sunfish-qcacld-mgmt-tx.py) | Service-76-gated QCACLD `0x7008` management-TX command and `0x7006` completion decoder; mock native publisher stages the exact by-reference frame, command, and CE descriptor under one gate and retains the frame lease until firmware completion; 11 module tests plus CE3 publication/completion integration pass; no production CE3 Gate, CE2 event-service integration, or radio transmission exists |
| [`sunfish_wlan_net_bridge.c`](sunfish_wlan_net_bridge.c), [`sunfish_wlan_net_source.rs`](../../rust/src/sunfish_wlan_net_source.rs), [`sunfish_wlan_tx_pipeline.rs`](../../rust/src/sunfish_wlan_tx_pipeline.rs), [`qcom_ce_v3_native_io.rs`](../../rust/src/qcom_ce_v3_native_io.rs), [`test-wlan-net-bridge.py`](test-wlan-net-bridge.py), [`test-wlan-net-bridge-ffi.py`](test-wlan-net-bridge-ffi.py) | Bounded sDDF Ethernet RX/TX copies with Rust RX-sink and TX-source adapters; the composed WPA2/CCMP path hands plans to the Gate-checked native CE4 publisher. Tests verify unauthorized/invalid-route preflight does not dequeue, and native publisher tests write only into mock MMIO/DMA. Requires confirmed RX decap mode 2, one mapped TX source region (`oid` 0), and a live production Gate; no Sunfish composition or radio test is wired |
| [`ieee80211_station.rs`](../../rust/src/ieee80211_station.rs) | Bounded open-system authentication and association request/response codecs for the Native-WiFi link-control path; carries caller-selected RSN/additional IEs but does not derive/install keys, send frames, or mark protected traffic authorized |
| [`ieee80211_ethernet.rs`](../../rust/src/ieee80211_ethernet.rs), [`validate-ieee80211-ethernet.py`](validate-ieee80211-ethernet.py) | Bounded Ethernet-II to unprotected To-DS Native-WiFi LLC/SNAP encapsulation, source-audited against pinned Lineage mac80211; six host tests, no QoS selection, CCMP protection, or radio access |
| [`ieee80211_join.rs`](../../rust/src/ieee80211_join.rs) | Bounded selected-BSS open-authentication and association flow joined to the WPA2 supplicant; its auth/association requests are now host-tested through the synthetic WMI_CONTROL/HTC/CE software queue, with synthetic AP responses accepted by the join state machine; this does not submit radio traffic, and scan/channel selection and live transport remain external |
| [`ath10k_scan.rs`](../../rust/src/ath10k_scan.rs) | Bounded WMI 10.4 start-scan body and one-request session, now composed into a WMI_CONTROL-bound software HTC/CE queue with scan-state rollback on credit or descriptor failure; 55 isolated host tests pass; no hardware descriptor publication or radio access |
| [`ath10k_key.rs`](../../rust/src/ath10k_key.rs), [`validate-ath10k-key.py`](validate-ath10k-key.py) | Source-audited WMI 10.4 CCMP-128 pairwise/group key body, WMI_CONTROL software-queue adapter, and generation-bound SEC_IND correlation tracker; Wpa2Join accepts only matching completed-key evidence; zeroed RSC/TSC and padding match pinned Linux; host-tested, no live firmware event owner, DMA publication, or radio access |
| [`ath10k_scan.rs`](../../rust/src/ath10k_scan.rs), [`ieee80211_scan_results.rs`](../../rust/src/ieee80211_scan_results.rs), [`ieee80211_station.rs`](../../rust/src/ieee80211_station.rs), [`ieee80211_join.rs`](../../rust/src/ieee80211_join.rs), [`ieee80211_wpa2.rs`](../../rust/src/ieee80211_wpa2.rs), [`ieee80211_eapol.rs`](../../rust/src/ieee80211_eapol.rs), [`ieee80211_ccmp.rs`](../../rust/src/ieee80211_ccmp.rs), [`ieee80211_ethernet.rs`](../../rust/src/ieee80211_ethernet.rs), [`ieee80211_supplicant.rs`](../../rust/src/ieee80211_supplicant.rs), [`sunfish_wlan_tx_pipeline.rs`](../../rust/src/sunfish_wlan_tx_pipeline.rs), [`qcom_ce_v3_native_io.rs`](../../rust/src/qcom_ce_v3_native_io.rs) | Selected-BSS WPA2/CCMP data can pass through the guarded native CE4 publisher. ScanWorkflow exposes caller-budgeted start-response and maximum-duration deadlines; the owner arms the start deadline only after native WMI publication and polls before event dispatch. A timed-out start quarantines its scan identifiers until a fresh WMI generation; automatic retry remains the caller's decision. Wpa2Join accepts caller-budgeted deadlines for auth, association, M1, and M3, with the owner polling before response dispatch. Host tests cover these deadlines alongside scan completion, authorization, pre-dequeue route checks, packet-number freshness, scratch clearing, firmware no-encrypt policy, and mock-memory descriptor publication. No production Gate/system composition or live radio transport exists yet; scan scheduling, fresh-generation recovery, WPA2 retransmission timers, hardware key installation, rekey, fragmentation/reassembly, and management-frame protection remain incomplete |
| [`ath10k_firmware.rs`](../../rust/src/ath10k_firmware.rs), [`ath10k_ce.rs`](../../rust/src/ath10k_ce.rs), [`ath10k_htt_rx.rs`](../../rust/src/ath10k_htt_rx.rs), [`ath10k_htt_rx_ring.rs`](../../rust/src/ath10k_htt_rx_ring.rs), [`ath10k_wmi.rs`](../../rust/src/ath10k_wmi.rs), [`validate-ath10k-htt-rx.py`](validate-ath10k-htt-rx.py), [`validate-wlan-launch-path.py`](validate-wlan-launch-path.py) | Bounded API-5 metadata parsing, WCN3990 HTT RX/ring preflight, and source-gated WMI 10.4 INIT through synthetic HTC WMI_CONTROL endpoint/credit negotiation and CE descriptor serialization; 13 RX tests and 69 combined CE/HTC/firmware/WMI host tests, pinned ABI/policy checks; no DMA or radio access |
| [`ath10k_mgmt_tx.rs`](../../rust/src/ath10k_mgmt_tx.rs) | Bounded unprotected WMI 10.4 management-frame encoder for probe/auth/association traffic; source-audited command ABI and capability gate; requires a negotiated WMI_CONTROL binding and composes into the host-only HTC credit/sequence and CE descriptor queue, including a selected-BSS auth/association integration test; no DMA publication, doorbell, or radio access |
| [`smmu-read-probe.c`](smmu-read-probe.c), [`build-smmu-read-probe.sh`](build-smmu-read-probe.sh) | Read-only SMMU probe; current candidate renders up to 80 valid route rows with per-route CBAR/SCTLR/ACTLR plus a context ACTLR nonzero bitmap/count on-screen, while UART retains the full per-bank register dump; the SMMU map is read-only and the image has no reset-PD or SMC capability |
| [`validate-smmu-handoff.py`](validate-smmu-handoff.py), [`smmu_handoff_model.py`](smmu_handoff_model.py), [`sunfish_smmu_exec.rs`](../../rust/src/sunfish_smmu_exec.rs) | Host-only SID collision/route planning and ordered installer; executor now requires an opaque authority token whose constructor is absent from production builds. Only mock tests can mint it until a Sunfish platform gate proves EL1 control, exclusive ownership, fresh routes, and fault containment; no hardware adapter or SMMU writes are present |
| [`smem-directory-probe.system`](smem-directory-probe.system), [`smem-directory-probe.c`](smem-directory-probe.c), [`build-smem-directory-probe.sh`](build-smem-directory-probe.sh) | Temporary read-only SMEM v12 lookup for GLINK items 478–480 and SMP2P items 435/428; maps SMEM read-only, acquires/releases TCSR lock 3, and displays only directory metadata without inspecting item payloads or starting WLAN |
| [`aop-read-probe.system`](aop-read-probe.system), [`build-aop-read-probe.sh`](build-aop-read-probe.sh) | Native Rust QMP descriptor observation: read-only AOP MSGRAM, no mailbox writes, doorbell, IRQ, or SMC; device photo confirms stable dormant AP endpoint and empty mailboxes, recorded in [`aop-handoff.json`](aop-handoff.json) |
| [`aop-handshake-probe.system`](aop-handshake-probe.system), [`build-aop-handshake-probe.sh`](build-aop-handshake-probe.sh) | Standalone native QMP link/open/close, restricted to AP-owned descriptor fields and DT-checked doorbell; device photo confirms HANDSHAKE PASS, three notifications, empty mailboxes; no payload or SMC, result held until restart |
| [`aop-packet-probe.system`](aop-packet-probe.system), [`build-aop-packet-probe.sh`](build-aop-packet-probe.sh) | Native fixed modem-OFF notification delivery: device photo confirms PACKET PASS, publication/readback, consumption, four notifications, empty mailboxes, and AP close; no ON, PAS, firmware load, or DMA |
| [`rpmh-read-probe.system`](rpmh-read-probe.system), [`build-rpmh-read-probe.sh`](build-rpmh-read-probe.sh) | Photo confirms READ ONLY DONE: stable 134-entry command DB, CX/MSS/MX TURBO selectors 6/7/4, XO address, eight DRV2 TCSs with 16 commands each, both active banks idle; no votes, IRQ-clear, doorbell, SMC, or DMA |
| [`rpmh-cx-read-probe.system`](rpmh-cx-read-probe.system), [`build-rpmh-cx-read-probe.sh`](build-rpmh-cx-read-probe.sh) | Photo confirms CX READ PASS, response 5, checked completion/cleanup and restored IRQ enable; active TCS0 only, no resource WRITE, voltage vote, modem start, or DMA |
| [`rpmh-resource-reads-probe.system`](rpmh-resource-reads-probe.system), [`build-rpmh-resource-reads-probe.sh`](build-rpmh-resource-reads-probe.sh) | Photo confirms eight READs pass, both rounds CX=5/MSS=0/MX=3/XO=3, checked cleanup and bank reuse; same RSC/splash mappings, no power votes or modem start |
| [`rpmh-cx-turbo-hold-probe.system`](rpmh-cx-turbo-hold-probe.system), [`build-rpmh-cx-turbo-hold-probe.sh`](build-rpmh-cx-turbo-hold-probe.sh) | Photo confirms CX TURBO HOLD PASS, CX5->6 in two readbacks, MSS/MX/XO unchanged at 0/3/3, checked acknowledgement/cleanup; request held until restart, no guessed restore or modem start |
| [`rpmh-resources-hold-probe.system`](rpmh-resources-hold-probe.system), [`build-rpmh-resources-hold-probe.sh`](build-rpmh-resources-hold-probe.sh) | Observed RESOURCE HOLD PASS: fixed CX6/MSS7/XO3 requests acknowledged with clean controller completion and repeated readbacks; MX stayed 3 without a WRITE; holds until restart, no bus/PAS/reset/DMA |
| [`bcm-read-probe.system`](bcm-read-probe.system), [`build-bcm-read-probe.sh`](build-bcm-read-probe.sh) | Observed METADATA PASS for MC0/ACV/CE0/SN11/SN0/SH0, including ACV width0/unit1/VCD11 after correcting our generic zero-width rejection; final DT routes checked; no RSC mapping, SMC, bus vote, or modem start |
| [`rpmh-handoff-probe.system`](rpmh-handoff-probe.system), [`build-rpmh-handoff-probe.sh`](build-rpmh-handoff-probe.sh) | Observed READ ONLY STABLE: all eight active/sleep/wake banks empty/idle, config0x80008104, IRQ status0; three controller pages read-only plus splash; no resource command, register write, SMC, or modem start |
| [`bcm-reads-probe.system`](bcm-reads-probe.system), [`build-bcm-reads-probe.sh`](build-bcm-reads-probe.sh) | Observed BCM READ PASS: 12 raw replies, matching passes, 24 bank-preservation checks; first controller page RW, two tail pages read-only; no resource WRITE, SMC, bus vote, or modem start |
| [`bcm-mc0-hold-probe.system`](bcm-mc0-hold-probe.system), [`build-bcm-mc0-hold-probe.sh`](build-bcm-mc0-hold-probe.sh) | Observed MC0 HOLD PASS: fixed word0x69917f75 acknowledged, numeric request unchanged in two readbacks, other five unchanged, 50 bank checks and checked cleanup; no other resource WRITE, auto unvote, PAS/reset, or modem start |
| [`rpmh-proxy-hold-probe.system`](rpmh-proxy-hold-probe.system), [`build-rpmh-proxy-hold-probe.sh`](build-rpmh-proxy-hold-probe.sh) | Observed COMBINED HOLD PASS: MC0 then CX6/MSS7/XO3, ACK mask0xf, 72 requests, 144 bank checks and unchanged final bus replies; no AOP-on, PAS/reset, firmware, DMA or automatic unvote |
| [`rpmh-pas-bus-probe.system`](rpmh-pas-bus-probe.system), [`build-rpmh-pas-bus-probe.sh`](build-rpmh-pas-bus-probe.sh) | Observed BUS FLOOR HOLD PASS: ACK0x7f, 119 requests, 238 bank checks; SN11/SN0 0x213c5d02 and saturated CE0 0x2fffffff; other bus/rail replies preserved; no sufficient-bandwidth/PAS-readiness claim |
| [`scm-pas-query-probe.system`](scm-pas-query-probe.system), [`build-scm-pas-query-probe.sh`](build-scm-pas-query-probe.sh) | Observed QUERY DONE: SMC64 trial and all four init/memory/auth-reset/shutdown availability queries return status0/value1; no queried PAS operation, pointer, PAS ID, resource vote or firmware |
| [`pas-metadata-probe.system`](pas-metadata-probe.system), [`build-pas-metadata-probe.sh`](build-pas-metadata-probe.sh) | Pins installed MDT header, validates/copies/reparses it in five dedicated contiguous pages with byte-volatile Device/NX access, two guards and zero padding; no SMC capability, segment loading, modem-pool mapping or reset; METADATA STAGE PASS observed, LineageOS return confirmed |
| [`modem-prepare-probe.system`](modem-prepare-probe.system), [`build-modem-prepare-probe.sh`](build-modem-prepare-probe.sh) | Observed PREPARE PASS on Sunfish: AOP mask `0x1f`, four notifications, RSC phase `0x0b`, ACK `0x7f`, 119 requests and 238 preservation checks; MDT verified, no controller/guard error; fixed holds remain until restart; no secure call, segment files, modem reset release or WLAN start |
| [`modem-bcm-after-aop-probe.system`](modem-bcm-after-aop-probe.system), [`build-modem-bcm-after-aop-probe.sh`](build-modem-bcm-after-aop-probe.sh) | After `StartingValues` failed in modem preparation, displays two six-resource BCM READ passes after the fixed AOP ON notification; photo showed both passes stable, with SH0 `0x240d5adf` differing from the recent reference `0x24005adf`; no BCM resource WRITE, secure call, modem reset release, WLAN start, or DMA |
| [`modem-bcm-aop-delta-probe.system`](modem-bcm-aop-delta-probe.system), [`build-modem-bcm-aop-delta-probe.sh`](build-modem-bcm-aop-delta-probe.sh) | Same-boot before/after AOP comparison passed: all six BCM values and pair samples unchanged, transition mask 0; image SHA-256 `591de328…`; no BCM resource WRITE, secure call, modem start, or WLAN DMA |
| [`pas-init-probe.system`](pas-init-probe.system), [`build-pas-init-probe.sh`](build-pas-init-probe.sh) | Repeats PREPARE, checks SCM64/INIT availability, then hands the owned metadata to PAS4 once; raw x0-x7 retained, no retry/resume, MEM_SETUP, executable segments, AUTH_AND_RESET, SHUTDOWN or DMA; INIT returned monitor0/service0 but metadata mismatched; diagnostic confirmed all payload bytes zero, both guards/padding intact and repeated reads identical |
| [`pas-mem-setup-probe.system`](pas-mem-setup-probe.system), [`build-pas-mem-setup-probe.sh`](build-pas-mem-setup-probe.sh) | Successful INIT plus exact observed consumed-buffer pattern and fresh query gate one fixed PAS4 MEM_SETUP for 0x8b000000..0x94800000; full range excluded from AP frames, no segments/auth/reset/DMA; MEM SETUP PASS observed, return to LineageOS confirmed |
| [`modem-data-probe.system`](modem-data-probe.system), [`build-modem-data-probe.sh`](build-modem-data-probe.sh) | Repeat successful INIT/MEM_SETUP gates, then transfer only installed non-executable segment23 (45 bytes) to one fixed RW/NX page at0x93200000; check payload, unchanged page gap and stable readback; no auth/reset/DMA; DATA COPY PASS observed, return to LineageOS confirmed |
| [`modem-segment-probe.system`](modem-segment-probe.system), [`build-modem-segment-probe.sh`](build-modem-segment-probe.sh) | Repeat INIT/MEM_SETUP gates, then load non-executable segment14:4160 file bytes and4032 BSS zero bytes across two RW/NX pages, with two adjacent read-only guards; exact full readback and stability checks; no auth/reset/DMA; SEGMENT LOAD PASS observed, return to LineageOS confirmed |
| [`modem-image-probe.system`](modem-image-probe.system), [`build-modem-image-probe.sh`](build-modem-image-probe.sh) | Repeat INIT/MEM_SETUP gates,then stage all29 pinned loadable segments and exact BSS in the fixed RW/NX pool; two complete verification passes and exact gap snapshots; private relocated loader and minimal temporary ramdisk; no auth/reset/firmware execution/DMA; returned to LineageOS without result evidence, full-image result unverified |
| [`build-modem-image-boot-check.sh`](build-modem-image-boot-check.sh) | Complete payload boot/read diagnostic with early loader screen markers; no PAS, votes or modem-memory writes; returned to LineageOS without a checkpoint report, result unverified |
| [`build-entry-hold-check.py`](build-entry-hold-check.py) | Compact and large entry holds confirmed on Pixel; identical entry code draws IMAGE ENTRY HOLD and loops before relocation or seL4; no PAS, modem access or resource votes |
| [`build-image-relocation-hold.sh`](build-image-relocation-hold.sh) | Full payload in the actual private loader; stops at IMAGE RELOCATED before loader main or seL4; complete copy verified in QEMU; native Google/Lineage return without marker, result unverified |
| [`build-loader-entry-hold.sh`](build-loader-entry-hold.sh) | Actual loader IMAGE ENTRY hold confirmed on Pixel with explicit interrupt mask; stops before relocation |
| [`build-loader-copy-hold.sh`](build-loader-copy-hold.sh) | Byte copy/readback hold: native IMAGE COPY START observed, followed by user-confirmed automatic return to LineageOS; copy completion unverified |
| [`build-loader-copy-progress.sh`](build-loader-copy-progress.sh) | Native IMAGE COPY FAIL FFFFFFFE observed: expected-address/bounds guard rejected input; copy completion unverified |
| [`build-loader-copy-inputs.sh`](build-loader-copy-inputs.sh) | Native photo reports source b0081000, destination b0000000 and ERR201: full image overlap; reproduced with frozen artifact in QEMU |
| [`build-loader-copy-native.sh`](build-loader-copy-native.sh) | Native IMAGE COPY PASS confirmed: complete copy from b0081000 to c0000000 and both immutable readback passes; holds before caches or destination execution |
| [`build-loader-relocation-native.sh`](build-loader-relocation-native.sh) | Native IMAGE RELOCATED confirmed: copied loader executed at c0000000 and stopped before loader main or seL4 |
| [`build-loader-task-images.sh`](build-loader-task-images.sh) | Relocates, reaches loader main, validates the pinned 11-region plan, copies and verifies every AP image twice, then holds at IMAGE LOAD PASS before kernel/CPU start; host and QEMU checks pass, native IMAGE LOAD PASS confirmed |
| [`build-loader-kernel-boot.sh`](build-loader-kernel-boot.sh) | After verified relocation and all eleven task readbacks, enable the real MMU and start seL4 into the unchanged read-only payload diagnostic; QEMU reaches the real kernel entry with exact arguments, native BOOT CHECK PASS confirmed (68,393,962 bytes, sum 0x2766d207); no modem start |
| [`build-modem-image-stage-native.sh`](build-modem-image-stage-native.sh) | Unchanged 29-segment staging program on the confirmed guarded loader path, with its own pinned AP plan; repeats INIT/MEM_SETUP gates, stages data/BSS RW/NX, verifies twice and preserves gaps; no auth/reset/DMA, native IMAGE STAGE PASS confirmed with zero mismatches and intact gap/metadata checks |
| [`build-modem-auth-probe.sh`](build-modem-auth-probe.sh) | Repeats complete staging, then fresh INFO/metadata gates and one fixed PAS4 authentication/reset call; can start the modem, reports secure values only; no READY claim, shared-ring writes, WLAN QMI, host DMA or automatic cleanup; native AUTH RETURN PASS confirmed, readiness unobserved |
| [`build-modem-ready-probe.sh`](build-modem-ready-probe.sh) | Repeats one-shot staging/authentication then passively checks a newly published modem SMP2P status item; read-only SMEM and lock3 only, ten-second observation, no AP transport/IRQ/WLAN/host DMA setup; native status header published with zero entries, ready observation timed out |
| [`build-modem-smp2p-probe.sh`](build-modem-smp2p-probe.sh) | After full staging/auth return and a new modem status header, publish one fresh AP master-kernel status item in one RW SMEM page and notify once; SSR features disabled, peer negotiation required; no GLINK/WLAN/IRQ/host DMA setup; native allocation guard Bounds11, AP publication0 and peer negotiation0 |
| [`build-modem-smp2p-preauth-probe.sh`](build-modem-smp2p-preauth-probe.sh) | After staging/INFO/fresh-metadata gates, publish AP428 in the verified empty partition before AUTH, with a one-shot permit and the same one RW page; recheck metadata, authenticate, then observe and notify once; no post-start AP allocation; native READY BIT SEEN confirmed |
| [`build-modem-glink-directory-probe.sh`](build-modem-glink-directory-probe.sh) | Native DIRECTORY SNAPSHOT PASS: mask5, descriptor32B and AP RX16KiB present, AP TX absent; allocation metadata only, no GLINK payload read/write, allocation or IRQ |
| [`build-modem-glink-receive-probe.sh`](build-modem-glink-receive-probe.sh) | Native RX EMPTY SNAPSHOT PASS, ERROR0/RX AVAILABLE0; descriptor/header read-only check after ready; no GLINK writes, FIFO advance, allocation, ACK or IRQ |
| [`build-modem-glink-prepare-probe.sh`](build-modem-glink-prepare-probe.sh) | Native GLINK MEMORY PREPARE PASS, FREE AFTER41C8/GUARD BYTESFBC; fresh descriptor478/TX479 verification, exact five-page writer, no authentication/start or GLINK packet |
| [`build-modem-glink-boot-probe.sh`](build-modem-glink-boot-probe.sh) | Native BOOT MEMORY PASS, ERROR0/MASK7/RX AVAILABLE8; replay header COMMAND0/PARAM ONE1/PARAM TWO7 confirms a VERSION v1/features7 offer; same five-page mapping, no post-AUTH GLINK write or packet |
| [`build-modem-glink-version-probe.sh`](build-modem-glink-version-probe.sh) | Native TX CONSUMED1, ACK SEEN0, STOPPED Payload8; peer OPEN2/channel1/name length0xa at unread RX front, available0x50 bytes; no channel accepted; later ACK unknown |
| [`build-modem-glink-pending-probe.sh`](build-modem-glink-pending-probe.sh) | Native RX SNAPSHOT PASS: complete80-byte queue, three OPENs including IPCRTR remote ID2, exact VERSION_ACK1/features0 behind them; parser error0, no RX consumption or channel acceptance |
| [`build-modem-glink-open-probe.sh`](build-modem-glink-open-probe.sh) | Original native request STOPPED Payload8, STAGED0/HEAD STORE0/NOTIFICATIONS0; no request published, exact validation site unknown; original frozen image retained |
| [`build-modem-glink-open-v2-probe.sh`](build-modem-glink-open-v2-probe.sh) | Native IPCRTR REQUEST PASS, ERROR0; fixed request consumed (TX tail/head32), OPEN_ACK4/local1/0 captured; RX tail8 unchanged, no peer OPEN_ACK sent, no intents or QRTR data; no partition flashed |
| [`build-modem-glink-channel-probe.sh`](build-modem-glink-channel-probe.sh) | Native IPCRTR ACK PASS/ERROR0, TX tail/head40; reciprocal ACK consumed;128 new bytes contain8 one-entry INTENT records for remote2,360 newer bytes unclassified; RX tail8 unchanged, no intent use or QRTR/WLAN/DMA |
| [`build-modem-glink-intents-probe.sh`](build-modem-glink-intents-probe.sh) | Original native expanded capture STOPPED Payload8 after all488 new bytes were copied; ACK consumed, rejected record unknown; first header5/remote2/count1 is not the failure location; original frozen image/evidence retained |
| [`build-modem-glink-intents-v2-probe.sh`](build-modem-glink-intents-v2-probe.sh) | Native INTENT INVENTORY PASS/ERROR0: all488 new bytes parsed,30 unique IDs0..29, groups128x16/512x8/1152x5/8320x1 and one52-byte receive request; no local intent, response or QRTR packet |
| [`build-modem-glink-rx-probe.sh`](build-modem-glink-rx-probe.sh) | Native RECEIVE COPY PASS/ERROR0:52 bytes copied with guards intact; QRTR v1/type2/body command2 observed; RX tail8 retained, no RX_DONE or outgoing QRTR |
| [`build-modem-glink-done-probe.sh`](build-modem-glink-done-probe.sh) | Native RXDONE CONSUMED PASS/ERROR0: TX indices72, RX indices656 and empty; private buffer/guards retained, AP intent retired; no new intent or QRTR TX |
| [`build-modem-qrtr-hello-probe.sh`](build-modem-qrtr-hello-probe.sh) | Native HELLO TRANSPORT PASS/ERROR0: fixed HELLO consumed and completion14 seen for peer intent0/capacity128; one52-byte receive request pending, no reuse or RX advance |
| [`build-modem-qrtr-next-probe.sh`](build-modem-qrtr-next-probe.sh) | Native NEXT PACKET COPY PASS; both buffers intact, NEW_SERVER service0x42/opaque instance word0xb401/node0/port3; later8 bytes unclassified, no service QMI or DMA |
| [`build-modem-qrtr-discovery-probe.sh`](build-modem-qrtr-discovery-probe.sh) | Native STOPPED, timeout0x10/stage7 after2 announcements; latest service0x2b/instance0x1202/node0/port1; RX empty at0x338, both guards pass. Exact kernel source identifies0x42 as SERVREG_NOTIF and0x2b as SSCTL; WLFW0x45 was not seen in this bounded sample |
| [`build-modem-qrtr-credit-probe.sh`](build-modem-qrtr-credit-probe.sh) | Native fresh intent4 offer consumed, no unsolicited ACK/tail store; then Timeout0x10/stage8 waiting for DATA;2 records, empty RX, both guards pass; exact elapsed time not shown |
| [`build-modem-qrtr-wait-probe.sh`](build-modem-qrtr-wait-probe.sh) | Native Fatal0x0e/stage8 after38.663seconds at19.2MHz;2 records, WLFW0, TX/RX empty and both buffer guards pass; crash cause not captured |
| [`build-modem-crash-probe.sh`](build-modem-crash-probe.sh) | First temporary read-only reason capture stopped with `Error::Partition`0x4 after fatal status7; both guards passed, but no reason bytes were captured. The initial header-host diagnosis was not the blocker. |
| [`build-modem-fatal-trace-probe.sh`](build-modem-fatal-trace-probe.sh), [`modem-fatal-trace-probe.system`](modem-fatal-trace-probe.system), [`validate-modem-fatal-trace-inputs.py`](validate-modem-fatal-trace-inputs.py) | Settled screen: `0x6` to `0x7` at `0x97f6` ms; 1,295 status samples, one change, 1,440 DATA polls. Item 421 reason read passed with both guards; no GIC access, WLFW discovery, or partition flash |
| [`modem-crash-probe-v2`](../../.cache/pixel-sunfish/modem-crash-probe-v2/prepared-modem-crash-probe-v2/fastboot.img) | Native reason capture stopped at check stages2/2 after38.908s; fatal status7, both guards pass, WLFW not seen. Stage2 was the extra scan of unrelated host-pair ranges; the specific range/subcheck is unknown. No reason bytes captured. |
| [`modem-crash-probe-v3`](../../.cache/pixel-sunfish/modem-crash-probe-v3/prepared-modem-crash-probe-v3/fastboot.img) | Temporary boot stopped at the prerequisite IPCRTR OPEN shape gate (error8, plan detail4, guard stage1); zero request writes/head stores/notifications, so the reason capture was not reached |
| [`modem-crash-probe-v4`](../../.cache/pixel-sunfish/modem-crash-probe-v4/prepared-modem-crash-probe-v4/fastboot.img) | Native `REASON SNAPSHOT PASS`: discovery ended `Fatal`0x0e after `0x97bf` ms; fatal SMP2P status7; baseline/reason reads pass and the bounded 256-byte item421 prefix changed. Item offset0x12510/size0x100, NUL-terminated text0x5c bytes; guards pass, WLFW not seen in this bounded sample. No partition flashed. |
| [`modem-fatal-trace-probe`](../../.cache/pixel-sunfish/modem-fatal-trace-probe/fastboot.img) | Temporary `fastboot boot` captured fatal at `0x97f6` ms after ready/handover, with one status transition (`0x6` to `0x7`), 1,295 status samples, and 1,440 DATA polls. Item 421 reason: `DOG HAL COMMON C 173 TMR SLAVE2 DOG DETECTS STALLED INITIALIZATION TRIAGE WITH IMAGE OWNER`. The GIC distributor is kernel-owned; no watchdog pending read or partition flash. |
| [`modem-host-services.json`](modem-host-services.json), [working Lineage comparison](../../docs/pixel-modem-lifecycle-design.md#working-lineage-host-service-comparison-2026-10-20) | Live namespace confirms remote storage `0x0e` and heap-memory service `0x34`; source trace identifies missing handlers, variable-length transport, separate 2 MiB / 5 MiB pools, VMID assignments, and storage backend. The modem remained ONLINE with crash count zero; no custom boot or service-specific request was run. |
| [`test-modem-qmi.py`](test-modem-qmi.py), [`qcom_modem_owner.rs`](../../rust/src/qcom_modem_owner.rs), [`pixel_modem_owner.rs`](../../rust/src/pixel_modem_owner.rs), [`modem-owner.system`](modem-owner.system) | Bounded GLINK/QMI owner wired to native mappings, a live SMEM lifecycle gate, GLINK/SMP2P IRQs and deferred work. The offline regression suites pass, the owner PD links, and Microkit/CapDL mapping and notification routes validate. Direct-peer QRTR control and readiness-gated announcements are implemented; bounded storage/memory actions pass offline; guarded pool reservations and the SCM adapter pass offline; the composed backing-store/pool backend and host file provider pass offline; two tests exercise service activation and OPEN replies through the FIFO owner with stand-ins. The default native runtime has no host-service handlers; an opt-in retained-handler hook exists, but no production backend is attached. UFS coverage includes an exact-SID SMMU route planner that refuses the inherited broad route; native UFS platform ownership, DMA/SMMU mapping, IRQ-to-host wiring, backing-data identity, and physical media access remain. No device access. |
| [`qcom_qrtr_control.rs`](../../rust/src/qcom_qrtr_control.rs), [`sunfish_qrtr_control_test.rs`](../../tests/sunfish_qrtr_control_test.rs) | Bounded HELLO, peer registry and removals; passive WLFW endpoint discovery requires one exact service/version/instance match and never forwards local NEW_LOOKUP. Explicit backend readiness gates local NEW_SERVER/DEL_SERVER. One-shot RX intents replenish within eight buffers. Native owner enables control but leaves unfinished storage/memory services unannounced. Offline only. |
| [`qcom_modem_handlers.rs`](../../rust/src/qcom_modem_handlers.rs), [`sunfish_modem_handlers_test.rs`](../../tests/sunfish_modem_handlers_test.rs) | Bounded storage OPEN/CLOSE/ALLOC/RW/device-error and heap QUERY/ALLOC/FREE, eight handles, confirmed leases and 4096-byte action steps. Cached responses prevent effect replay under backpressure. Tests use private backing images and simulated ownership/durability; native services stay unannounced pending real providers. |
| [`qcom_modem_pools.rs`](../../rust/src/qcom_modem_pools.rs), [`qcom_modem_pool_native.rs`](../../rust/src/qcom_modem_pool_native.rs), [`validate-modem-pools.py`](validate-modem-pools.py) | Dedicated 2 MiB storage / 5 MiB heap plus private SCM scratch, six unmapped physical/virtual guards, actual CapDL/allocation/DT/loader checks. One sealed SCM64 call per grant/reclaim; ambiguous completion quarantines without retry or rollback. 38 new pool/handler tests plus 14 reservation tests pass; live ownership contract and backing store are still required. Pixel untouched. |
| [`qcom_modem_storage.rs`](../../rust/src/qcom_modem_storage.rs), [`qcom_modem_backend.rs`](../../rust/src/qcom_modem_backend.rs), [`qcom_ufs_protocol.rs`](../../rust/src/qcom_ufs_protocol.rs), [`qcom_ufs_gpt.rs`](../../rust/src/qcom_ufs_gpt.rs), [`qcom_ufs_scsi_block_io.rs`](../../rust/src/qcom_ufs_scsi_block_io.rs), [`qcom_ufs_host.rs`](../../rust/src/qcom_ufs_host.rs), [`qcom_ufs_irq.rs`](../../rust/src/qcom_ufs_irq.rs), [`qcom_ufs_platform.rs`](../../rust/src/qcom_ufs_platform.rs), [`qcom_ufs_native_io.rs`](../../rust/src/qcom_ufs_native_io.rs), [`qcom_ufs_dma.rs`](../../rust/src/qcom_ufs_dma.rs), [`sunfish_smmu.rs`](../../rust/src/sunfish_smmu.rs), [`sunfish_smmu_program.rs`](../../rust/src/sunfish_smmu_program.rs), [`modem-owner-ufs-profile.system`](modem-owner-ufs-profile.system), [`modem-storage-host`](../../experiments/modem-storage-host) | The offline storage stack supports native 512- and 4096-byte UFS blocks while keeping modem-service addressing in 512-byte sectors; tests cover native 4 KiB GPT parsing, extent normalization, boundary reads and safe sub-block writes. The actual primary GPT has 15 entries and a zero disk GUID. The offline UFS profile now records GCC reset provider plus GCC_UFS_PHY_BCR line ID 3, correcting its former placeholder ID 1. The read-only backup header has a valid CRC and reciprocal header LBAs but points its entry array back to primary LBA 2; none of the four eligible tail blocks matches that array. Paired verification correctly rejects the live layout as `BackupGeometry`, so the parsed `super` extent is not a trusted paired-table proof. The full storage-host crate test run passes 203 tests (2 ignored), including a CAP decoder check against observed value 0x1587031F; the main Rust library’s 590 tests pass. Read-only LineageOS evidence identifies the phone's Micron UFS 2.1 device and 4 KiB LUN geometry; it does not verify the custom-OS provider. The active owner map is unchanged. Production lease/provider, UFS DMA/SMMU setup and IRQ wiring remain absent; no custom-OS SCSI read, firmware transfer or WLAN start has occurred. |
| [`build-modem-owner-offline.sh`](build-modem-owner-offline.sh), [`build-modem-ufs-profile-offline.sh`](build-modem-ufs-profile-offline.sh), [`validate-modem-owner.py`](validate-modem-owner.py), [`validate-ufs-dma-pool.py`](validate-ufs-dma-pool.py) | Explicit owner-feature ELF and offline capability-layout builds, with a private relocated SDK loader. The candidate-only UFS DMA pool is allocated as a contiguous 16 MiB run; a separate 11-page uncached region holds the LPAE roots and eight leaf tables. CapDL audits prove both runs are contiguous, mapped once in the owner, non-aliasing, within RAM and outside fixed reservations in the kernel and live DTBs. Seven dynamic reservations have no fixed addresses to compare. No SMMU route, device DMA, boot, flash, or Android image packaging. |
| [`build-ufs-pool-probe.sh`](build-ufs-pool-probe.sh), [`ufs-pool-probe.system`](ufs-pool-probe.system), [`sunfish_ufs_pool_probe.rs`](../../rust/src/sunfish_ufs_pool_probe.rs), [`validate-ufs-pool-probe.py`](validate-ufs-pool-probe.py) | Temporary CPU-only runtime check of the build-selected 16 MiB pool: a temporary Fastboot boot on `sunfish` slot `a` reported `PROFILE VALID`, `PAGE RW PASS`, and all 4,096 pages checked at `0x8035d000`. It writes, reads, and restores one word per page. The profile grants only the pool and splash framebuffer, with no device MMIO, IRQ, SMMU, SMC, modem or WLAN capability. CapDL and DTB pool audits pass; seven dynamic reservations have no fixed address to compare. A follow-up source fix fences restores on the early-failure path; the rebuilt image passes offline audits but has not been booted. `fill_ufs_dma_backing_mappings` plus `plan_ufs_dma_from_backing_pool` derive and validate all 4,096 pool-frame mappings through the exact UFS SID and LPAE plan for a caller-provided exclusive IOVA window. Tests do not reserve IOVA, install an SMMU route, establish live ownership, or start DMA. |
| [`build-ufs-hci-read-probe.sh`](build-ufs-hci-read-probe.sh), [`ufs-hci-read-probe.system`](ufs-hci-read-probe.system), [`sunfish_ufs_hci_read_probe.rs`](../../rust/src/sunfish_ufs_hci_read_probe.rs), [`validate-ufs-hci-read-probe.py`](validate-ufs-hci-read-probe.py), [`validate-ufs-platform-source.py`](validate-ufs-platform-source.py), [`qcom_ufs_platform.rs`](../../rust/src/qcom_ufs_platform.rs) | Temporary bare read-only UFSHCI CAP/VER/HCS probe stopped at `SNAPSHOT START`; LineageOS sysfs separately reports cached CAP `0x1587031F` and HCI version `0x210`, while raw HCS remains unavailable. The pinned-source audit distinguishes pre-CAP HBA/rail/PHY/bus setup from post-CAP reset/calibration/SerDes/PCS/UniPro and HCE setup. Before CAP/VER, Linux sets the DT maximum rates (200 MHz core, 150 MHz UniPro, 300 MHz ICE), then enables all eight controller clocks, including TX/RX lane-sync clocks; Qualcomm invokes its lane-clock helper again later during HCE. GCC's three rate-capped sources require the shared `vdd_cx` supply at the nominal corner, mapped by the runtime DT to RPMh ARC resource `cx.lvl`; this is source-derived and not a measured voltage vote. The offline lifecycle model now permits the pre-CAP read-only stage without IRQ binding or DMA/SMMU readiness, and defers those to post-CAP preparation; its 20 platform tests pass. The model is not a native resource provider. The exact clock IDs, four rails, HBA GDSC, interconnect paths, and GCC UFS-PHY reset line 3 are recorded; the former reset placeholder 1 was wrong. The stopped bare probe does not show controller failure. Native resource lease/provider and custom-OS SCSI I/O remain absent; no firmware transfer or WLAN start occurred. |
| [`build-ufs-gcc-snapshot-probe.sh`](build-ufs-gcc-snapshot-probe.sh), [`ufs-gcc-snapshot-probe.system`](ufs-gcc-snapshot-probe.system), [`sunfish_ufs_gcc_snapshot_probe.rs`](../../rust/src/sunfish_ufs_gcc_snapshot_probe.rs), [`qcom_ufs_gcc.rs`](../../rust/src/qcom_ufs_gcc.rs), [`validate-ufs-gcc-snapshot-probe.py`](validate-ufs-gcc-snapshot-probe.py) | Temporary GCC-only snapshot reads the UFS domain, PHY reset, four source roots, and nine clock gates. The screen checkpoints region mappings and each read, then classifies GCC-visible pre-CAP state against pinned Linux source. Mappings are read-only and uncached; no UFSHCI, IRQ, DMA, SMMU, SMC, reset or power write is available. Ten decoder tests require settled updates, Linux root-off status, source/divider values, and domain/reset/gate state; they do not treat the CMD_ROOT_EN request bit as the active-state indicator. One test uses the photographed values as a regression fixture. The source validator pins those bits to `clk-rcg2.c`; CapDL authority audit, Rust library suite, AArch64 `no_std` native codegen, and image build pass. The first instrumented boot stopped at `MAP LOCAL` before any GCC read. ELF review showed `memory_region_symbol!` panics on a zero address symbol; the three GCC mappings lacked `setvar_vaddr`. They now bind those symbols, and the validator requires each binding. The corrected image (SHA-256 `9eae5a867433cb7629d85ac0d513d21ad3a97d841f983e6f093f26c9b49a0eed`) booted temporarily and read all mappings/registers. Observed: GDSCR `0xF8222000`, PHY BCR `0`, all four RCG command registers `0`, configs AXI/UniPro/ICE/AUX `0x00100105`/`0x00100107`/`0x00000103`/`0`, and all nine branch gates have enable bit 0 set. The screen's `AXI ROOT DISABLED` label was an incorrect decoder result: pinned Linux `clk_rcg2_is_enabled()` checks `CMD_ROOT_OFF`, which is clear in the captured `CMD=0` value. The observed PHY auxiliary HID 0 is also equivalent to HID 1 for divide-by-one per `clk_rcg2_calc_rate()`. The corrected GCC-only decoder accepts this photo as GCC-visible pre-CAP ready; RPMh vote provenance, PMIC rails, HBA regulator ownership, PHY power, interconnect state, and exclusive ownership remain unproven. The earlier HCI `SNAPSHOT START` stop matches a missing `setvar_vaddr` for the read-only HCI mapping while the probe used `memory_region_symbol!`, which can panic on a null address before CAP/VER. The HCI probe now validates that injected address before reading; its temporary image passed the read-only CapDL audit and built, but has not been booted again. A passing GCC subset still does not prove RPMh votes, PMIC rails, HBA regulator, PHY power, interconnect state, or ownership. No partition was flashed and no UFS command or WLAN start occurred. |

## Assumptions (explicit)

1. ABL handed the temporary payload to **EL1 with MMU off** on 2026-10-06;
   EL2 / driver-VM is out of scope here.
2. Single CPU core; PSCI multi-core later.
3. Debug UART is `uart8` @ `0x00a88000` (live DT compatible
   `qcom,msm-geni-console`, SPI 355).
4. Early putchar **does not** program GCC clocks, interconnect, or TLMM. It
   sends a one-byte GENI TX command and polls for completion. The temporary
   boot probe verified this works after `fastboot oem uart enable`; the first
   character at the bootloader handoff may be lost.
5. The seL4 overlay uses ABL's contiguous DRAM map and retains all
   reserved-memory nodes from the final merged Lineage runtime FDT. With
   Lineage's rooted-debugging option enabled, run `adb root`, then capture
   `/sys/firmware/fdt` with:

   ```sh
   adb exec-out cat /sys/firmware/fdt > .cache/pixel-sunfish/live-runtime.dtb
   ```

   Repeat after each Lineage/device-tree update; the base DTB embedded in
   `boot_a` omits device-specific reservations added by DTBO. Return ADB to
   the normal shell user with `adb unroot` after capture.

## Next real steps (still no flash)

1. SBU UART capture of bootloader and Android kernel text: **proven 2026-10-06**
   with USB-Cereal at 1.8 V and 115200 baud. The bootloader UART setting is
   currently enabled.
2. Exact live `boot`, `vbmeta`, and `dtbo` images for both slots: **backed up
   and hash-verified 2026-10-06** in rooted recovery. Current Lineage
   20261001 is on slot `a`; `b` is marked unbootable.
3. Temporary `fastboot boot` of the live Lineage image and the UART probe:
   **proven 2026-10-06**. The probe printed `SUNFISH UART SMOKE OK`,
   `EL1 MMU OFF`, `PC 0x80080000`, and `DTB 0x84500000`, then reset to Lineage.
4. seL4 kernel, Microkit SDK, and UART hello: **compiled 2026-10-06** in
   separate `.cache/*-sunfish` worktrees. This is not yet proof of an on-device
   seL4 boot.
5. First Microkit `fastboot boot` did not return to Lineage or yield readable
   UART output. Its wrapper put the loader at +64 bytes, violating the loader's
   page-relative relocation assumptions. The corrected +4096-byte wrapper
   booted into seL4 and printed `SUNFISH MICROKIT HELLO` on 2026-10-06.
   A user-space counter read after that marker stopped the service before its
   reset request. With the read removed, the service printed its reset marker,
   PSCI reset the phone, and LineageOS 23.2-20261001 booted again on slot `a`.
6. A read-only Microkit probe sampled the first 16 MiB of the current DT's
   `cont_splash_region@9c000000` on 2026-10-06. All 4096 page-start samples
   were zero. A follow-up wrote black/white stripes to both 1080×2340×4
   candidate frames in that reserved region. The stripes were visible on the
   Pixel before PSCI reset it to Lineage. The bootloader's existing scanout is
   therefore usable for an early UI, although independent panel bring-up is
   still open.
7. A Pixel 4 KiB Rust target and display-only build of the existing eight-screen
   Slint shell booted in Microkit. The user saw the Home screen with readable
   colors and text. A 45 logical-pixel top inset moved its title below the
   punch-hole camera; this was verified on the Pixel. The test watchdog reset
   the phone, and Lineage returned on slot `a`. A timed repeat measured about
   191 ms from the UART `SLINT START` marker to `FRAME READY`; this excludes
   panel scanout. That image proved first-frame display; later experiments
   established a real Microkit touch packet (item 14).
8. A read-only probe of Qualcomm GENI SE7 at `0xa84000` returned firmware
   revision `0x00000303` (protocol 3, I²C), master clock configuration
   `0x00000011`, and zero idle status/IRQ/FIFO values. A subsequent experiment
   reading `0x03406000` as an apparent GPIO6 TLMM address stopped at that read
   and the phone restarted without the probe's reset marker. That address
   omitted the NORTH bank's `0x00500000` offset; see item 12 for a successful
   read at the corrected address. Do not reuse `0x03406000` for GPIO6.
   The [sunfish FTS driver](https://android.googlesource.com/kernel/msm-modules/fts_touch/+/refs/heads/android-msm-sunfish-4.14-s-beta-4/fts.c)
   also routes the I²C switch (GPIO72) to the application processor and enables
   touch power before normal event reads. Read Android's live pin/clock/regulator
   state before attempting an SE7 transaction from Microkit.
9. With LineageOS rooted debugging enabled, Android reports GPIO8 and GPIO72
   as claimed outputs and GPIO9 as a claimed input; GPIO6/7 are unused.
   The current DTBO assigns GPIO8 to touch reset, GPIO9 to the interrupt,
   GPIO72 to the AP/SLPI I²C switch, and PM6150 GPIO4 to touch power.
   The touch I²C controller is runtime-suspended when idle. A filtered,
   temporary I²C trace of the driver's read-only `status` query captured a
   successful two-message transfer to address `0x49`: write
   `FA 20 00 00 2D`, then read one byte (`C0`). The trace was disabled
   afterward. This confirms the live bus/device path under Android, but does
   **not** establish its power, pin, or clock state after a Microkit handoff.
   A second temporary trace showed the `gcc_qupv3_wrap1_s1_clk` and source
   clock being prepared/enabled for that transfer. The [sunfish GENI I²C
   driver](https://android.googlesource.com/kernel/msm/+/refs/heads/android-msm-sunfish-4.14-s-beta-4/drivers/i2c/busses/i2c-qcom-geni.c)
   selects active I²C pins and enables SE resources on runtime resume, then
   selects sleep pins and disables resources when idle. Its 1 MHz timing entry
   uses divider 1, high 1, low 5, cycle 13; the Microkit read-only probe saw
   `SE_I2C_SCL_COUNTERS=0`, so the temporary boot has not configured those
   timings. Both tracing groups and global tracing were restored to disabled.
10. A confirmed physical tap was captured under LineageOS. Linux `fts` input
    reported one contact at `(479, 1424)` and its release. The same tap's
    I²C FIFO reads used a one-byte write `86` followed by an eight-byte read
    from address `0x49`. The first packet was `13 01 DF 01 59 0A 21 00`,
    motion packets began with `23`, and the release was
    `33 01 DF 01 59 10 20 00`. Applying the [sunfish driver's coordinate
    packing](https://android.googlesource.com/kernel/msm-modules/fts_touch/+/refs/heads/android-msm-sunfish-4.14-s-beta-4/fts.c#2849)
    to bytes 2-4 gives `(479, 1424)`, matching Linux input. The parser in
    [`experiments/slint-notes/src/pixel_touch.rs`](../../experiments/slint-notes/src/pixel_touch.rs)
    checks this captured down/up pair. The capture was stopped and tracing
    disabled. A later Microkit probe established bus initialization (item 13).
11. A separate temporary Microkit boot set the GENI SE7 1 MHz I²C timing
    register, then issued only an address-only command to `0x49`. The command
    did not complete within two million status polls: IRQ status stayed zero,
    `GENI_STATUS` became `0x41` (master command active), and `I2C_IOS` stayed
    zero. The probe requested PSCI reset and LineageOS booted on slot `a`.
    GCC readback showed the SE7 branch halted and its clock-vote bit 23 clear.
    In the next temporary boot, setting that bit according to the [sunfish GCC
    driver](https://android.googlesource.com/kernel/msm/+/refs/heads/android-msm-sunfish-4.14-s-beta-4/drivers/clk/qcom/gcc-sdmmagpie.c)
    cleared both the branch halt and source-root-off flags, but `I2C_IOS`
    remained zero and the address-only command still did not finish. Thus the
    missing GCC vote was real but was not the whole bus initialization. The
    later PMIC-power and pin-routing probe completed the transaction (item 13).
12. The [sdmmagpie pinctrl driver](https://android.googlesource.com/kernel/msm/+/refs/heads/android-msm-sunfish-4.14-s-beta-4/drivers/pinctrl/qcom/pinctrl-sdmmagpie.c)
    places GPIO6/7 in the NORTH bank at `0x03906000` and `0x03907000`.
    A read-only, explicitly mapped temporary boot successfully read their
    control registers as `0x1` each (GPIO mode) and their IO values as zero.
    GPIO8 (touch reset) and GPIO72 (AP/SLPI switch) also started at CFG
    `0x1`, IO zero. A bounded probe voted on SE7's GCC clock and set GPIO6/7
    to `qup11` (CFG `0x4`); `I2C_IOS` stayed zero, so it sent no I2C command.
    A further bounded probe drove reset high (GPIO8 CFG `0x200`, IO `0x3`)
    and the AP switch low (GPIO72 CFG `0x200`, IO `0`); `I2C_IOS` still
    stayed zero and it again sent no I2C command. Each image reset through
    PSCI and LineageOS returned on slot `a`. The live touch node's first
    `pinctrl-0` phandle points to PM6150 GPIO4's `touch_power_default`,
    configured `output-high`. The first SPMI captures ended before the
    Microkit handoff; their missing marker was a capture-window error, not
    evidence of a failed mapping. See the successful readback in item 13.
13. The SPMI arbiter reported version `0x50000000`; PM6150 GPIO4's PPID
    `0x0C3` mapped to APID `0xE4`, owner EE0. Extended-long SPMI reads
    identified a GPIO MV peripheral (type `0x10`, subtype `0x11`). A temporary
    boot configured its VIN, pull, output, source, and mode as the live
    `touch_power_default` pinctrl requires, then read each value back. With
    GPIO6/7 muxed to `qup11`, reset GPIO8 high, and AP switch GPIO72 low,
    `I2C_IOS` became `0x3`. An address-only command to touch address `0x49`
    completed with IRQ `0x1`; an `0x86` FIFO read returned eight bytes.
14. The [sunfish FTS driver](https://android.googlesource.com/kernel/msm-modules/fts_touch/+/refs/heads/android-msm-sunfish-4.14-s-beta-4/fts.c)
    enables active scanning after startup with command `A0 00 FF`. A bounded
    Microkit probe sent that command, then received a real finger-down FIFO
    packet `13 01 9A F2 50 10 22 00`, decoded as `(666, 1295)`. It painted
    the temporary display green and requested reset. An earlier version of
    the probe faulted on an unavailable EL0 counter register; the successful
    run used a bounded software poll loop.
15. The experimental Slint boot image now includes a separate touch service
    and a noncached single-producer event queue. Its first boot gave the touch
    service priority 100, which kept Slint from starting until the touch poll
    timed out; the user then saw WORKSPACES but taps could not work. The image
    was rebuilt with Slint priority 99 and touch priority 90. In the next
    temporary boot, UART showed `SLINT FRAME READY`, `TOUCH READY`, raw
    finger-down packets, and matching `SLINT TOUCH` coordinates. The user
    opened Notes, typed letters, entered `1 2 3` in Calculator, and opened
    Contacts; all registered, and switching and typing felt fast. Clock,
    Calendar, Messages, and Settings also opened and their visible controls
    responded in the same boot. These are the interactive in-memory UI
    prototypes; persistent storage and device services remain separate work.
    The touch service's bounded test then requested PSCI reset; ADB confirmed
    LineageOS 23.2 returned with boot complete on slot `a`. No partition has
    been flashed; all Microkit images used temporary `fastboot boot`.

## Foundation probe (Pixel 4a, 2026-10-07)

Run `sh platforms/sunfish/build-foundation-probe.sh` to produce
`.cache/pixel-sunfish/foundation-probe/fastboot.img`. It contains three
separate protection domains: a priority-100 bounded CPU hog with a 2 ms / 10 ms
budget, a priority-95 intruder that intentionally reads an unmapped address,
and a priority-90 witness. The latter two have 200 ms budgets so UART output
and the reset request can finish even if timer delivery is broken. The hog and
witness share a single uncached page;
the intruder has no mapping for it. The witness requests PSCI reset when the
hog finishes or its bounded wait ends. This probe does not include the UI.

The first temporary boot exposed a board configuration error. The live DT
routes the timer node through the Qualcomm PDC by inheritance; seL4's hardware
generator produced `KERNEL_TIMER_IRQ=-1`. The hog ran for roughly 1.9 seconds
without yielding its 2 ms budget. The witness first ran with `DONE=1`, and its
reset request did not complete. A physical restart returned LineageOS.

[`sel4-overlay.dts`](sel4-overlay.dts) now assigns the architected timer's
interrupt parent directly to the GIC. The rebuilt SDK generates
`KERNEL_TIMER_IRQ=19` (virtual timer PPI) for this EL1 build. In the corrected
temporary boot, the monitor reported `foundation_intruder` taking a level-1
translation fault at unmapped virtual address `0x40000000`. The witness then
reported `STARTED=1 DONE=0`, demonstrating that it got CPU while the
higher-priority hog was still active. It later reported `DONE=1` and requested
PSCI reset. UART showed the following Lineage boot, and ADB confirmed
LineageOS 23.2 boot complete on slot `a`. Concurrent UART writes interleaved
some characters in the hog and reset marker lines; the witness state and
monitor fault lines were readable. This verifies this bounded scheduling and
memory-isolation case, not yet general process isolation or system-call policy.
All runs used `fastboot boot`; no partition was flashed.

## Capability-controlled IPC probe (Pixel 4a, 2026-10-07)

`sh platforms/sunfish/build-ipc-probe.sh` builds a separate temporary image.
The passive service accepts a protected call on channel 0 with label 42 and
message word 41, then replies with label 43 and word 42. The client alone has
the service endpoint capability in slot 74. An intruder makes a raw
`seL4_Call` using that same slot number in its own CSpace, where the slot is
empty. A witness reads the client result and service invocation count from a
page mapped only into the service, client, and witness, then requests reset.

The final temporary Pixel boot reported `CLIENT PASS`, intruder reply label
`2` (`seL4_InvalidCapability`), `DENIED INVALID CAP`, and witness result
`CLIENT RESULT=1 SERVICE CALLS=1`. The first version of the probe labeled
any return from the intruder call a failure; source inspection showed that
an invocation of a null capability returns a syscall error to the caller.
The corrected probe checked that error and counted service invocations.
ADB then confirmed LineageOS 23.2 boot complete on slot `a`. No partition was
flashed. This demonstrates one authorized call and one denied call; it does
not yet define application-level privileges, a general syscall API, or
persistent storage.

## Per-channel service policy probe (Pixel 4a, 2026-10-07)

The [service-policy note](../../docs/pixel-service-policy.md) sets out the
current protection-domain model and target app boundaries. The separate
[`policy-probe.system`](policy-probe.system) image gives a writer and reader
different channels into one passive service. The service accepts writes only
on the writer's channel; a third domain has no endpoint capability. On a
temporary Pixel boot, UART showed `WRITER PASS`, `READER PASS`, and `INTRUDER
DENIED NO CAP`. Its witness reported `VALUE=1313821765 WRITES=1 READS=3
DENIED=1`, proving the reader's attempted write left the `NOTE` test word
unchanged. The witness requested reset, and ADB confirmed LineageOS 23.2 boot
complete on slot `a`. This is volatile test state, not Pixel vault storage.

## Isolated Notes prototype (Pixel 4a, 2026-10-07)

The Slint image now has a `notes_service` protection domain with private
editable text and a bounded view mapped read-only by Slint. Only Notes has a
protected-call endpoint to `notes_volatile_vault`; Slint cannot call the vault
directly. `sh platforms/sunfish/build-slint-display.sh` builds the normal
temporary boot image. Setting `SLINT_PIXEL_EXTRA_FEATURES=pixel-notes-smoke`
adds a one-shot boot-time key/save/delete check; that version printed
`SUNFISH NOTES IPC SMOKE PASS` and `SUNFISH VOLATILE VAULT WRITE` on the
Pixel. The normal image was rebuilt afterward, and LineageOS returned with
slot `a` boot complete. The vault keeps data in RAM only; it provides no
encryption or persistence yet.

| build-modem-crash-probe.sh first temporary run | Native reason snapshot stopped with Error::Partition0x4; fatal status7; baseline and reason errors4; both guard checks passed; no reason text was captured. The photo does not identify which partition check failed. |
| modem-crash-probe-v2 temporary boot | Screen showed CHECK STAGES00020002: both reads stopped in the non-global route scan. The exact entry/subcheck is unknown; the global-header diagnosis was incorrect. |
| modem-crash-probe-v3 temporary boot | Stopped before request staging at the IPCRTR OPEN shape preflight: error8, guard stage1, plan detail4; zero TX bytes/head stores/notifications. The global reason capture did not run. |
| modem-crash-probe-v4 temporary boot | The OPEN preflight passed and the read-only item421 reason snapshot completed: `DISC ERROR`0x0e (`Fatal`), `SLAVE STATUS`7, status/base/reason errors0, `PREFIX CHANGED`1, `CHECK STAGES`0. The selected item is at0x12510, size0x100; its sanitized NUL-terminated text is0x5c bytes and begins `DOG HAL COMMON C 173 TMR SLAVE2 DOG DETECTS STALLED INITIALIZATION TRIAGE WITH IMAGE OWNER` as displayed. Both guards pass; WLFW was not seen during the bounded discovery sample. This records a changed reason prefix alongside the fatal result but does not establish causation or global WLFW absence. No partition flashed. |
