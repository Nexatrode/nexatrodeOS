# Sunfish ath10k port plan

Current Linux comparison work is recorded in
[`sunfish-mobian-comparison.md`](sunfish-mobian-comparison.md), with a pinned
machine-readable manifest and exact-kernel build entry point. This uses the
public Linux host-side startup implementation while retaining vendor secure
firmware. The absence of official seL4 support is not by itself a stop condition
for source study, building a reference, or implementing the native host side.
On 2026-10-25 the powered-off Linux comparison passed modem/WLFW startup,
scanning, WPA2 association, DHCP and twenty-second transfers in each direction
(82.95 Mbit/s Pixel → Mac, 122.21 Mbit/s Mac → Pixel). It used verified private
firmware inputs, RAM-shadow modem storage and a temporary Fastboot boot. All
four original EFS hashes remained unchanged. This supplies a working Linux
reference for native startup; persistent storage and native seL4 WLAN remain
untested. The linked report records the startup order and capture limits.

Native implementation progress and the next hardware check are tracked in
[`sunfish-native-wifi-startup.md`](sunfish-native-wifi-startup.md).
The cold native one-page SMMU installation/readback has since passed, and the
private EFS/TFTP input provider passed host integration with the real reference
files. Native DMA, IPA/modem startup and Wi-Fi traffic remain untested.

## Decision

Use upstream Linux `ath10k` SNOC as the behavioral and protocol reference for
the Pixel 4a WLAN chip. Do not attempt to compile the Linux driver inside a
Microkit protection domain and do not start the radio yet. The upstream driver
targets `qcom,wcn3990-wifi`; Sunfish's mainline device tree uses that compatible
and the `google_sunfish` calibration variant, while the installed phone reports
WCN3998 and runs the downstream ICNSS/QCACLD stack. The current Mobian Sunfish
mainline port reports working Wi-Fi through ath10k on this handset. Its documented
firmware set uses Sunfish-specific board data, the generic 60-byte ath10k API
metadata file, and the same `wlanmdsp.mbn` bytes observed in the phone's vendor
partition. This is useful Linux compatibility evidence; it does not validate
the seL4 loader, service providers, ownership, or DMA path, and it does not
authorize bundling the vendor or community firmware blobs into NexatrodeOS.

The canonical upstream ath10k cross-reference is Linux commit
`d6d3455d8751854e0d1820f14184682e23ac4475`. The clean Sunfish mainline tree
used by the source validators and target build checks is pinned separately to
`sm7150-mainline/linux` commit
`fd78d17954a86ea6ceb0c535e618ea129e09f8dd`. Keep license notices if code is
ever copied; prefer reimplementing the small protocol/state pieces against
Microkit and sDDF interfaces rather than importing Linux compatibility layers.

## Linux reference compatibility and firmware boundary

The cached Mobian reference is pinned at
`miromraz/mobian-pixel4a-port` commit
[`789679f25f859bd741d690fe221d1cc68a23444e`](https://github.com/miromraz/mobian-pixel4a-port/tree/789679f25f859bd741d690fe221d1cc68a23444e);
its README reports working 2.4/5 GHz Wi-Fi on Sunfish with mainline Linux.
The repo documents a WCN3990
`board-2.bin` carrying `variant=google_sunfish` from
[`sm7150-mainline/firmware-qcom-sm7150-ath10k`](https://github.com/sm7150-mainline/firmware-qcom-sm7150-ath10k/tree/792c716c85c06861c4d17bce5ec21fba7f0f7de6),
plus the 60-byte `ath10k/WCN3990/hw1.0/firmware-5.bin` API header from
[`linux-firmware`](https://kernel.googlesource.com/pub/scm/linux/kernel/git/firmware/linux-firmware/+/afabaf773c4c2e2c841429933a6a084a5af4d14d/ath10k/WCN3990/hw1.0/firmware-5.bin).
Its
`firmware-google-sunfish` input supplies `wlanmdsp.mbn`; the recorded SHA-256
matches the phone's `/vendor/firmware/wlanmdsp.mbn` exactly. The generic
upstream WCN3990 `wlanmdsp.mbn` is a different file and is not this working
Sunfish reference set.

The Mobian
[`wcn-wifi-bringup.sh`](https://github.com/miromraz/mobian-pixel4a-port/blob/789679f25f859bd741d690fe221d1cc68a23444e/work/wcn/sbin/wcn-wifi-bringup.sh)
gives a concrete ordering constraint: keep
`ath10k_snoc` from autoloading, wait for the modem remoteproc to be running,
wait for the QRTR WLFW service to appear, then probe `ath10k_snoc` once and
require both an IEEE 802.11 phy and a network interface. The script comments
that probing before WLFW is available can cause host-capability rejection
(`90`). That ordering was subsequently exercised by the powered-off Linux run linked
above; the source script was invoked once after its prerequisites were ready.

The license boundary applies to specific files, not to a technical step that
must be skipped. `linux-firmware` marks the generic WCN3990 bundle
redistributable under its Qualcomm Atheros firmware license; the 60-byte
`firmware-5.bin` is feature metadata, not executable WLAN firmware. The
Sunfish board-data repository and the `firmware-google-sunfish` repository do
not declare licenses for their binary files, and the latter describes itself
as nonfree. The public Google factory package separately displays terms that
restrict use to the authorized Android device and limit incorporation into
other software. Keep those binary files out of distributable NexatrodeOS images without
applicable rights. The authorized private device comparison uses local
verified inputs; this distribution boundary is not a missing startup step. The uncertainty does
not require us to ignore the Linux compatibility evidence or to repeat a
Linux boot merely to rediscover the reported hardware match.

For native bring-up, the next engineering requirement is to reproduce the
proven ordering with real seL4 providers: AP IPA and the SERVREG map,
remote-storage, heap-sharing, and TFTP services available before modem reset;
then modem-ready and QRTR/WLFW discovery; only then may the WLAN client
advance. The existing native path lacks production implementations for those
providers, a validated native translated SMMU context, and a bounded WLAN DMA
pool. The working private firmware set is already available for the device
comparison; the new RAM provider and SMMU experiment are tracked separately. Keep radio start disabled until
those gates are implemented and validated.

Sunfish's installed downstream driver is QCACLD, not upstream ath10k. The
matched Lineage kernel's `sunfish_defconfig` enables `CONFIG_ICNSS`, and its
QCACLD Kbuild selects `QCA_WIFI_3_0` for that configuration. The host-only
[`rust/src/qcom_ce_v3.rs`](../rust/src/qcom_ce_v3.rs) therefore models the
downstream 16-byte CE v3 descriptors, 37-bit address field, transfer metadata,
and one-empty-slot ring cursor. The WCN3990 codec in `ath10k_ce.rs` remains a
separate upstream reference and is not used as the QCACLD descriptor oracle.
The new codec has no MMIO, DMA, IRQ, cache, or WLAN ownership authority.

The native WLFW `WLAN_CFG` request shape is separately captured in
[`rust/src/sunfish_qcacld_profile.rs`](../rust/src/sunfish_qcacld_profile.rs),
from the pinned Lineage ICNSS/QCACLD source. It includes the downstream host
version, Adrastea CE target and service tables, and legacy shadow-register
table; it omits shadow-register-v2 because ICNSS does not set that field. The
native client path also models ICNSS's default rejuvenation subscription and
dynamic feature mask (`0x01`) before WLAN_CFG. It does not use ath10k's
BDF_DOWNLOAD or CAL_REPORT steps. It records the registration FW_READY status
bit without treating it as the FW_READY indication or skipping MSA_INFO; the
WLAN_CFG gate requires the actual FW_READY indication. The separate
ALREADY_REGISTERED status skips a second MSA_INFO transfer and enters the
existing-server branch; after an actual FW_READY indication, that branch may
continue at WLAN_CFG without replaying the new-server MSA/CAP/mask sequence.
This is source fidelity only. It does not establish that the audited
Linux-firmware candidate is compatible with this handset, nor does it
authorize booting or starting WLAN. The ath10k request profile remains a
distinct comparison path.
The downstream CE7 entry uses `PIPEDIR_INOUT_H2H = 4`, while the generated
WLFW direction enum names only values 0 through 3; the host model preserves the
source value, and the later Linux compatibility test must verify its treatment.

## What the driver actually does

The `ath10k_snoc_probe()` order in `drivers/net/wireless/ath/ath10k/snoc.c`
provides the useful top-level map:

1. Select the device-tree match, constrain DMA to 35 bits, and create the
   ath10k core (`ath10k_snoc_probe`).
2. Discover/map resources, configure CE registers, request per-engine
   interrupts, and acquire the WLAN power sequence, regulators, and clocks
   (`ath10k_snoc_resource_init`, `ath10k_snoc_setup_resource`,
   `ath10k_snoc_request_irq`).
3. Allocate the MSA region, initialize firmware lookup, and start the WLFW QMI
   client (`ath10k_setup_msa_resources`, `ath10k_fw_init`,
   `ath10k_qmi_init`).
4. Register for modem lifecycle notifications (`ath10k_modem_init`). Firmware
   readiness and MSA ownership then proceed asynchronously through
   `drivers/net/wireless/ath/ath10k/qmi.c`.
5. Once the target is ready, initialize Copy Engine (CE) pipes and HTC/HTT
   services, post DMA buffers, and register the Linux wireless device. RX/TX
   delivery relies on Linux networking, mac80211, NAPI, `sk_buff`, DMA mapping,
   workqueues, timers, and interrupt management.

The first four steps are still blocked by platform ownership and firmware
authorization. Step five is the actual data path and is where a seL4 port
needs its own packet interface and bounded DMA buffer pool.

### Packet framing contract from the pinned Linux reference

The pinned ath10k core defaults RX decapsulation to
`ATH10K_HW_TXRX_NATIVE_WIFI` and selects RAW only when raw mode is explicitly
enabled (`drivers/net/wireless/ath/ath10k/core.c`). It also sets each vdev's
TX encapsulation parameter to `ATH10K_HW_TXRX_NATIVE_WIFI`
(`mac.c:ath10k_mac_vif_create`). The RX descriptor distinguishes RAW,
Native WiFi, Ethernet-II, and 802.3 SNAP/LLC payloads
(`rx_desc.h:enum rx_msdu_decap_format`). Native-WiFi receive processing
reconstructs the 802.11 header from descriptor status in `htt_rx.c`; TX
processing normally passes a mac80211-produced 802.11 frame to HTT
(`mac.c:ath10k_mac_tx` and `htt_tx.c`).

This is not directly compatible with the current sDDF/lwIP Ethernet queue:
handing a Native-WiFi payload to `ethernet_input()` would misparse its link
header, and accepting Ethernet-II RX alone would not provide the normal
Native-WiFi TX framing or scan/authentication/association policy. The Linux
driver has an Ethernet TX path for a narrow TDLS workaround, which is not
evidence of a complete Ethernet-only station mode for Sunfish. Before
connecting the WLAN PD to sDDF, either implement and validate the required
802.11 MAC/data-link and link-control services, or demonstrate a complete
firmware Ethernet-offload path in both directions for this target. Do not
infer packet-link readiness from CE/HTT RX completion or successful lwIP
tests on QEMU Ethernet.

## Port boundary

| Linux ath10k responsibility | Native Microkit equivalent | Status / rule |
| --- | --- | --- |
| SNOC MMIO and CE register access | WLAN PD with only the audited WLAN MMIO range | Do not grant until SMMU ownership and translated DMA isolation are proven. |
| CE rings and DMA buffers | Fixed, bounded WLAN-owned pool plus sDDF network queues | No Linux `dma_map_*`, `sk_buff`, or implicit physical-address assumptions. |
| Per-engine IRQ/NAPI | Dedicated decoded IRQ capabilities and budgeted event handling | IRQ IDs are inventoried; no current WLAN domain owns them. |
| MSA / firmware image transfer | Lifecycle manager plus bounded firmware loader and narrow WLFW client | SCM/PAS, peer reset/liveness, and firmware rights are unresolved; remain disabled. |
| Regulators, clocks, and power sequencing | Trusted platform power service with named WLAN operations | Never give WLAN raw RPMh/AOP/SCM authority. |
| HTC/HTT and CE service mapping | Protocol implementation in WLAN PD | Candidate for a host-tested, hardware-independent port after interfaces are fixed. |
| Scan, authentication, association | Separate policy/supplicant service | Not part of the low-level driver; requires crypto and network service interfaces. |
| Linux `mac80211`/netdev integration | sDDF packet endpoint and existing NexatrodeOS network services | DHCP/TCP/TLS are not sufficient by themselves; link and Wi-Fi control APIs are also needed. |

## Ordered work

1. **Freeze a host-only interface.** Start with CE descriptors, ring cursors,
   buffer ownership transitions, interrupt-to-event messages, and the sDDF
   packet endpoint. All addresses crossing the boundary are IOVAs or opaque
   buffer IDs, never physical addresses supplied by a client. The first
   host-only WCN3990 descriptor/ring and bounded HTC receive parser is in
   [`rust/src/ath10k_ce.rs`](../rust/src/ath10k_ce.rs); its tests cover the
   16-byte descriptor fields, 35-bit IOVA bound, one-empty-slot rule,
   wraparound, little-endian HTC lengths with endpoint and 4096-byte frame
   bounds, bounded trailer parsing, fixed-width credit-report validation and
   ledger updates, bounded caller-buffer TX frame encoding, unknown record
   preservation, target-ready capability parsing,
   connect response parsing, fixed-capacity endpoint assignment, and malformed
   or conflicting-response rejection. It encodes outbound service-connect
   messages with explicit flow-control flags and zeroed reserved bytes, and
   models the extended setup-complete message with a bounded optional RX-bundle
   limit. A bounded control-message dispatcher parses READY, service-connect
   response, and suspend-complete messages while preserving unknown IDs and
   payloads for caller policy. The model derives WMI-control versus data-service
   credit policy and commits endpoint plus credit state atomically across
   connect/disconnect. It models bounded TX token ownership, TX completion-index
   wraparound, and RX buffer post/complete/reclaim states.
   The RX tests enforce FIFO delivery and reject lengths beyond each buffer's
   declared capacity. A fixed-size software event queue models fair round-robin
   service under a caller-provided work budget and retains pending work across
   passes. A bounded HTC ledger models per-endpoint credit consumption,
   exhaustion, reports, service-specific policy, and flow-control disablement.
   A CE session guard
   rejects completions from older generations and disallows queue reuse until
   an external layer confirms quiescence. The separate
   [`rust/src/ath10k_htt.rs`](../rust/src/ath10k_htt.rs) module models the
   four-byte HTT version request and bounded target version-confirmation parse.
   [`rust/src/ath10k_wmi.rs`](../rust/src/ath10k_wmi.rs) walks WMI-TLV records
   using the upstream length-then-tag layout, with borrowed values and no
   alignment assumptions or memory allocation. This view is for WMI-TLV
   operation versions; the WCN3990 API-5/WMI 10.4 profile has separate fixed,
   non-TLV SERVICE_READY and READY parsers. Its WMI-TLV service-ready view checks
   the fixed event size, first service-bitmap array, nested host-memory-request
   TLV framing, and declared-versus-observed request count. Request payloads
   are decoded only into the four wire-level words (`request_id`, `unit_size`,
   `num_unit_info`, `num_units`); the count hint recognizes only the four
   known single modes, while unknown or combined flags remain rejected. This
   parser cannot allocate DMA memory. A separate checked planner accepts only
   a caller-supplied request rule, substitutes its trusted unit count for
   dynamic firmware counts, checks four-byte rounding/multiplication and a
   per-request budget, and returns a size plan without producing addresses or
   allocating memory. Service-ready parsing now enforces the pinned ath10k
   limit of 16 host-memory requests. A fixed-capacity batch planner matches
   every request to one unique trusted policy, rejects unknown or duplicate
   request IDs and any incomplete match, and enforces one aggregate byte budget
   before it emits any plans. `assign_iova_pool()` places each chunk at a
   separate 4 KiB-aligned IOVA in a bounded 32-bit pool and encodes the pinned
   `{req_id, ptr, size}` WMI INIT records. This still does not allocate physical
   frames, create SMMU mappings, or grant DMA authority. No Sunfish request
   allow-list is defined yet. The request limit, allocation sizing, chunk
   wire schema, and WMI 10.4 INIT layout are checked against the pinned Sunfish
   ath10k source by
   [`validate-wlan-launch-path.py`](../platforms/sunfish/validate-wlan-launch-path.py).
   The checked assignment now composes with the packed WMI 10.4 INIT body:
   42 resource words followed by the count and ordered chunk records. The
   encoder preserves Linux's one-item minimum union storage and zeroed unused
   slot. Its resource block remains caller-supplied and policy-validated; the
   codec does not select Sunfish values or perform transport/DMA operations.
   The WMI-TLV READY-event view decodes its six ABI words, MAC address, and
   status; the WMI 10.4 view decodes its distinct fixed 20-byte event. Both
   leave compatibility and startup decisions to firmware-specific policy.
   As of 2026-10-12, the complete Rust library, including these WLAN protocol
   modules, cross-compiles for the Pixel's `aarch64-sel4-microkit-pixel4k`
   target using the Sunfish debug SDK. A separate no-radio Microkit probe links
   and executes a CE descriptor encoding check and WLFW QRTR lookup encoding
   check, then displays PASS/FAIL on the existing splash buffer. Its system
   grants only that framebuffer; it has no SMC, WLAN MMIO, IRQ, SMMU, GLINK, or
   modem capability. It was temporarily booted on the Pixel on 2026-10-12; the
   screen showed `WLAN PROTOCOL`, `TARGET LINK CHECK`, `CE DESC PASS`, `QRTR
   PASS`, and `NO RADIO ACCESS`. This confirms the target library and both
   protocol checks execute in the Microkit PD; it is not a radio, firmware,
   transport, or DMA-isolation test. The host unit tests remain the broader
   protocol-behavior checks. A host-side system-manifest regression test also
   ensures the probe stays limited to the reserved splash mapping, with no
   SMC, IRQ, or IPC channel grants. The probe now also encodes the complete
   19-TLV IPA `INIT_MODEM_DRIVER` request and decodes matching success and
   failure responses using synthetic, bounded inputs. The updated temporary
   Sunfish boot image was built on 2026-10-22 (18,649,088 bytes; SHA-256
   `7ac82b434b527f2f81c9ce609b62fcca6acc8aceb63ec5f84b44aacb880f52ad`), but
   this added check has not yet run on the Pixel. It does not query IPA
   resources, issue QMI, start the modem, or touch radio hardware.
   [`rust/src/ath10k_htt_rx_session.rs`](../rust/src/ath10k_htt_rx_session.rs)
   now joins the WCN3990 FIFO RX ownership ledger to the descriptor decoder and
   raw-chain assembler. One completed MPDU group is retired into exact
   buffer-ID leases before parsing; malformed or unsupported groups are
   dropped while retaining their leases for recovery. Valid single-buffer
   payloads and supported raw chains are copied into a caller-owned bounded
   output slice. Tests cover both packet shapes, chain-count mismatch,
   unsupported decapsulation, and preserving leases when the output is too
   small. The suite is part of `platforms/sunfish/test-modem-qmi.py`, including
   the AArch64 `no_std` core compile.
   [Native-WiFi RX helper](../rust/src/ath10k_nwifi_rx.rs) implements
   the pinned Linux WCN3990 Native-WiFi RX reconstruction for one bounded data
   MSDU: it restores the HTT header-status 802.11 header, patches DA/SA from
   the decapped header, handles the advertised four-address padding mode, and
   restores the source-sized crypto parameters when the IV was not stripped.
   The FIFO session composes that decoder with lease retirement, reads
   `rx_hdr_status` from the same first buffer, and accepts only a Native-WiFi
   group marked both first and last MSDU; it drops partial A-MSDU groups and
   still returns their leases. Seven codec tests cover three/four-address
   frames, QoS restoration, crypto-prefix sizes, padding policies, unsupported
   shapes, and bounds; four session tests cover reconstruction and drop/lease
   behavior. This still has no native CE completion loop, SMMU-backed buffer
   access, cache maintenance, packet-queue handoff, full 802.11
   MAC/link-control/supplicant policy, or device authority; it is host-tested
   protocol code only.

2. **Extract protocol-only code.** The no-radio harness models deferred CE
   event budgeting, HTC credit accounting, and reset/session generations.
   Differential-test wire layouts and completion handling against the pinned
   Linux source. Keep MMIO, DMA, QMI, firmware, and power behind mock
   interfaces.
3. **Implement platform gates independently.** Continue the modem lifecycle,
   WLFW peer-generation, MSA, SCM, power, and SMMU ownership work. The WLAN
   protocol test harness must not be able to invoke those services.
4. **Prove containment before a hardware bring-up.** Require a dedicated
   translated SMMU context for both stream IDs, a negative DMA-fault test,
   exclusive AP/modem transport ownership, safe failure rollback, and an
   approved firmware source/license. Then—and only then—build a UART-only
   firmware-ready test image.
5. **Add packet service and Wi-Fi policy after the firmware-ready stage.**
   Keep authentication and app-facing network access in separate services.

## WLFW firmware-ready follow-up

The pinned ath10k QMI path sends `WLAN_CFG` (message `0x23`) and then
`WLAN_MODE` (message `0x22`) after firmware readiness; the SNOC path only
continues to CE initialization after those acknowledgements. The host-only
client now encodes the bounded `WLAN_CFG` schema with optional TLVs, sends
mission mode with hardware debug disabled, checks the exact response identity,
and enforces that ordering after an accepted calibration report and FW_READY
indication. The separate source-derived Sunfish QCACLD profile describes the
native downstream CE tables but has only host-side protocol coverage; no Linux
compatibility boot or radio test has validated it against this handset.
`WlanModeAccepted` means only that WLFW accepted the QMI request; it does not
mean CE pipes are initialized, firmware is running, or Wi-Fi is ready. No
MMIO, DMA, IRQ, SCM, SMMU, power, or ownership authority was added.

The native ICNSS path is modeled separately: its default quirk enables the
FW_REJUVENATE indication subscription, then it registers indications, sends
MSA_INFO, performs the platform ownership handoff, sends MSA_READY and CAP,
and requests dynamic feature mask `0x01`. ICNSS waits for FW_READY before the
QCACLD driver calls WLAN_CFG and WLAN_MODE. ICNSS does not send the ath10k
BDF_DOWNLOAD or CAL_REPORT sequence. Its dynamic-feature-mask helper is
best-effort: the pinned caller ignores its return code, so a valid rejected
response or observed timeout completes that step without treating it as
firmware readiness. The new host tests exercise both client paths and their
separate gates; they do not test Linux/firmware compatibility on the handset.

Do not add the proposed direct context-bank FSR poll using the pasted CB7/CB8
addresses. The pinned ARM SMMU driver defines the context-bank FSR at offset
`0x58` (with TF at bit 1 and PF at bit 3), not offset `0x10`. More importantly,
the Sunfish ABL snapshot reports no matching route for WLAN SIDs `0x240`,
`0x241`, `0x520`, `0x521`, or `0x522`; its only observed translated route is
UFS SID `0x300` to CB2, whose translation is disabled. A context-bank index
cannot be inferred from generic `cb7`/`cb8` guesses. Keep SMMU fault parsing
offline and synthetic until a complete live route snapshot identifies the
actual WLAN context and an owner explicitly delegates a safe read-only mapping.
Even then, a clear FSR would not prove isolation or authorize DMA.

The schemas and order are cross-checked against the pinned
[`qmi.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/qmi.c),
[`qmi_wlfw_v01.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/qmi_wlfw_v01.h),
and [`arm-smmu.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/iommu/arm/arm-smmu/arm-smmu.h).
The direct Rust suites `sunfish_wlfw_client_test.rs` and
`sunfish_glink_qmi_test.rs` cover the bounded client flow and transport module.

## IPA control-service checkpoint

The successful Lineage boot trace shows IPA `INIT_MODEM_DRIVER` completes
before WLAN QMI connects. The live Sunfish device tree reports IPA hardware
version `0x10` (v4.2) and marks filter/routing tables non-hashable. In the
matched kernel's v4.2 memory partition, the drop-statistics offset is `0xbf0`
and its size is zero; the IPA QMI request still marks both fields valid. The
host codec therefore represents these fields as bounded spans, allowing a
zero-size span only when its base remains inside IPA shared memory. The new
[`sunfish_ipa_profile.rs`](../rust/src/sunfish_ipa_profile.rs) constructs the
exact v4.2 TLV set and partition offsets while requiring the runtime SRAM
base/size, APPS_WAN_CONS endpoint, and uC-loaded state as explicit inputs. It
interprets the register-reported size as a partition length and validates QMI
offsets against the exclusive end `base + size`. These details come from the
pinned Lineage [Sunfish IPA device tree](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/atoll.dtsi#L2576),
[v4.2 memory partition](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa_utils.c#L3367),
and [IPA QMI request builder](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c#L517).

The next useful device check is to temporarily boot the updated no-radio
protocol probe and confirm its source-profile codec check passes. That only
validates the AArch64/Microkit execution path with synthetic register inputs.
A separate read-only Lineage capture should record the live IPA register
geometry and any actual `INIT_MODEM_DRIVER` response details available from
the existing trace. The bounded client is now composed through the GLINK/QRTR
session, FIFO adapter, and serialized modem owner. A host integration test
checks explicit request submission, response correlation, and QRTR receive
confirmation. The Pixel runtime does not enable this path: the native IPA
resource provider and AP-side IPA service are still absent, and the capture
does not reveal the allocated endpoint or uC-loaded state. Do not send this
QMI request from the custom image until those runtime inputs have trusted
providers.

### Read-only Lineage IPA geometry capture (2026-10-22)

The matched driver exposes `hw_type` and a read-only `gen_reg` file under
`/sys/kernel/debug/ipa/`. `hw_type` is the driver's enum value (16 for IPA
v4.2). `gen_reg` prints `IPA_SHARED_MEM_RESTRICTED` and `IPA_SHARED_MEM_SIZE`
as hexadecimal 8-byte units after decoding the `IPA_SHARED_MEM_SIZE` register;
its reader brackets the register access with the driver's normal active-client
reference. [`capture-lineage-ipa-state.sh`](../platforms/sunfish/capture-lineage-ipa-state.sh)
checks for exactly one authorized ADB device, verifies `sunfish` and a
LineageOS version property, and reads those two debugfs files without root or
device configuration changes. If either file is unavailable to the ordinary
shell, it records that fact and continues with the independent log check.

[`sunfish_ipa_snapshot.rs`](../rust/src/sunfish_ipa_snapshot.rs) parses that
exact format, rejects a non-v4.2 driver, missing or duplicate geometry fields,
malformed values, and geometry too small for the pinned v4.2 partition. The
capture does not provide APPS_WAN_CONS allocation or IPA uC-loaded state; both
remain explicit inputs. The parser and its profile-composition tests run in
the host suite. This is a Lineage-only evidence capture and does not start an
IPA service or send QMI.

The serial-targeted capture was rerun on 2026-10-24 while an Android emulator
was also attached. The running Lineage kernel's `/proc/filesystems` does not
list `debugfs`, and `/sys/kernel/debug` is absent, so the IPA `hw_type` and
`gen_reg` entries are unavailable because this kernel does not support that
filesystem, not because the shell lacks permission. The last 2,500 kernel
records contained no matching IPA-specific entries. The filtered boot log
still shows the IPA modem-init request and response markers, `wlan_pd` service
notification, ICNSS connection, and both WLAN firmware-ready messages, but
not the IPA response values or request fields. The filtered result is saved
at `.cache/pixel-sunfish/lineage-ipa-state-20261024.txt` (SHA-256
`07fcdfda6f8f5a7418554ffe0cd9fd8a1ad2c44e41e0e7832b45e1a696452f68`). This
closes the current Lineage debugfs capture path; a later IPA geometry read
must come from a kernel that exposes the source's read-only debugfs entries or
from a native IPA owner after its source-matched bandwidth-vote prerequisite
is met.

The capture also tries the existing Android kernel log buffer with ordinary
`adb shell logcat -b kernel -d -t 2500`, then prints only IPA init request,
response, failure, and request-field lines. It never enables dynamic debug or
changes logging settings. The matched `IPAWANDBG` request-field lines use
`pr_debug`; the Sunfish kernel config supports dynamic debug, but the capture
does not turn it on. The saved successful Lineage trace currently contains
only the request-sent and response-received markers. Those markers establish
the exchange order, not the response result/error or raw request fields; the
driver does not report every bad QMI result in its ordinary non-SSR path.

The Lineage capture was rerun on the attached Pixel, but this kernel does not
support debugfs, so it cannot provide the IPA v4.2 SRAM geometry. A target-side
synthetic codec check is still a separate optional validation: the no-radio
probe has only the reserved splash mapping and no SMC, IRQ, IPA/WLAN MMIO,
SMMU, GLINK, modem, firmware, or DMA capability. It cannot test WLAN startup
or supply the missing live APPS_WAN_CONS endpoint and IPA uC-loaded state. Do
not infer those fields from static tables or query the writable `ep_reg`
interface; keep them unknown until a safe read-only source exists. The Lineage
capture does not send QMI or start/restart the modem.

## WMI memory-request policy boundary

The host-memory-request TLV is four little-endian 32-bit words. Decoding those
words is host-tested. A 2026-10-12 diagnostic reload of the byte-reproduced
Pixel QCACLD module observed `num_mem_reqs=0` for the current Lineage/WCN3998
firmware startup. That is a decoded count from the live driver, not a raw
service-ready packet capture; it means no dynamic host-memory chunks were
requested in this boot. It does not include the separate WLFW MSA/QMI regions.
For this observed zero-request case, a host-memory allocator is not needed.
Interpreting a future nonzero request set or constructing WMI INIT remains
blocked on a target-specific policy. In the pinned Linux reference,
`num_unit_info` selects whether `num_units` is a literal or a runtime peer/vdev
count: bits 0, 1, and 2 mean vdevs, peers, and active peers, respectively. Linux
derives those counts from runtime resource settings (including a `+1` peer or
vdev slot), rounds unit size up to four bytes, allocates coherent DMA memory,
and returns the resulting DMA addresses in WMI INIT. INIT also includes
resource configuration derived from Linux/mac80211 defaults and advertised
firmware services. Those values are not
safe defaults for this port, and firmware-supplied counts must never determine
unbounded allocation sizes.

Before implementing any nonzero request path or constructing WMI INIT, obtain
a raw service-ready capture from the exact target firmware and define a
firmware-specific allow-list for request IDs and accepted count modes. The
policy must cap request count, unit size, unit count, and total bytes with
checked arithmetic; reject duplicate or unknown requests; derive any dynamic
count only from fixed, trusted driver configuration; and reserve every
request from a fixed WLAN IOVA pool. INIT must contain only IOVAs returned by a
dedicated translated SMMU domain, with address width checked against the
firmware's advertised address-extension service. A policy rejection must stop
startup before allocating or mapping anything. The observed zero-count case
does not authorize fabricated chunks or radio startup: MSA, firmware ownership,
power, transport, and DMA-isolation gates remain independent prerequisites.

Linux compares existing allocations by request ID and computed size and
rebuilds the chunk set when requests change. A Microkit implementation should
instead bind each accepted request to a predeclared pool slice and generation,
then reject a changed request set until the old DMA generation is quiesced.
Equal byte sizes alone do not prove stale firmware DMA has stopped.

The Linux driver VM is not the immediate shortcut: it would reuse ath10k more
directly, but requires a proven EL2/VMM configuration and safe assignment of
the WLAN MMIO, IRQs, firmware/lifecycle interface, and SMMU domain. Current
evidence does not establish those prerequisites.

## Read-only Lineage WMI INIT resource capture (2026-10-24)

A one-time rooted ADB capture on the attached Sunfish used a kernel-matched
QCACLD module that logs the 276-byte resource-config TLV immediately before
WMI INIT submission. The module was loaded in memory only; after capturing the
single record, the phone was rebooted, the staged module file was removed, and
ADB was returned to the shell user. The phone returned to boot-complete
LineageOS with its stock `wlan` module loaded and Wi-Fi enabled. No partition,
firmware image, resource memory, DMA mapping, or WLAN startup policy was
changed by the diagnostic.

The captured block decodes as tag 75 with a 272-byte value. Selected live
values are `num_vdevs=4`, `num_peers=38`, `num_offload_peers=3`,
`num_offload_reorder_buffs=3`, `num_tids=76`, TX/RX chain masks `3/3`,
`num_msdu_desc=1056`, `max_frag_entries=2`, `num_wow_filters=2`,
`flag1=0x00180600`, `use_pdev_id=1`, and `num_packet_filters=0xffffffff`.
The raw record is a regression fixture in
[`tests/sunfish_wmi_init_resource_dump_test.py`](../tests/sunfish_wmi_init_resource_dump_test.py).

The default Rust candidate did not match this live block: chain masks were
`0x7` instead of `0x3`, `max_frag_entries` was 10 instead of 2, WoW filters
were zero instead of 2, and packet filters were zero instead of
`0xffffffff`. Linux caps fragment entries with
`QCA_OL_11AC_TX_MAX_FRAGS=2`; `wma_update_fw_config()` then takes the PMO
filter values before WMI serialization. The Rust profile now accepts these as
explicit effective resource inputs, and a regression test reproduces all 68
captured words and the `10 01 4b 00` TLV header exactly. The capture also
exposed a reversed tag/length interpretation in the Rust QCACLD resource
parser and serializer; both now follow the pinned `WMITLV_SET_HDR` layout.

`flag1` contains the management-bundle, TX partition, new-HTT-format, and
peer-unmap bits. The host's new-HTT and peer-unmap gates map to service IDs
176 and 182 in the separate `SERVICE_AVAILABLE` extension segment at offset
128, plus the peer-unmap host setting; they are not bits in the base
`SERVICE_READY` bitmap. At the time of this resource-config capture, neither
service bitmap had been logged. A separate read-only capture below now records
both live bitmaps. The captured
`num_packet_filters=0xffffffff` is retained as a raw value pending source-level
interpretation.

The diagnostic emits the TLV before submitting WMI INIT. It proves the live
host constructed this block. A separate read-only `dmesg` capture from the
stock LineageOS boot reports firmware build `0x3226822e` at 46.806 seconds,
logs `ready_extract_init_status_tlv:0` at 46.985 seconds, and then logs
`wma_wait_for_ready_event: FW ready event received` at 47.306 seconds. The
pinned extractor returns the READY event's `ev->status`, and the wait helper
logs success only after its completion event, so stock QCACLD received a
successful WMI READY for this firmware build. The same check confirmed
`sys.boot_completed=1`, Wi-Fi enabled, and the stock `wlan` module loaded;
ADB was returned to the shell user afterward. This is not a raw
SERVICE_READY/READY packet capture, and it does not prove that a seL4 transport
can submit the same INIT. The decoder tests include the live resource-config
capture, and the source validator checks header packing, effective WMA/PMO
inputs, and serializer copies against pinned Linux source.

The host regression feeds the captured 68-word resource profile through the
QCACLD INIT-body and HTC-frame encoders, then reparses the frame and checks the
exact resource words and TLV header. At the time of this capture its
SERVICE_READY bitmap and HTC credit parameters were synthetic test inputs; a
later regression now replaces the bitmap inputs with the captured live words.
The HTC credit parameters remain synthetic, and the resulting frame is never
submitted to the Pixel.

### Fresh stock LineageOS WLAN and IPA log snapshot (2026-10-24)

A bounded read-only kernel-log snapshot from a later LineageOS boot confirms
the same stock WLAN readiness path. QCACLD logged `ready_extract_init_status_tlv:0`
and `FW ready event received`; the driver then reported firmware
`3.2.2.9.558.0`, hardware version `0x40090000`, and board revision 18. The log
also records IPA WDI2 producer and consumer ring setup, with event-ring lengths
of 16,384 and 32,768 bytes and element sizes of 8 and 16 bytes, respectively.
The WLAN interface subsequently completed association and DHCP. This is
repeatable stock LineageOS behavior evidence; it does not establish a live
seL4 IPA provider, native firmware startup, or a translated exclusive SMMU
domain.

A shell-level run of [`capture-lineage-ipa-state.sh`](../platforms/sunfish/capture-lineage-ipa-state.sh)
on that boot also records the service sequence: `vendor.ipacm`, QRTR,
`tftp_server`, `pd_mapper`, and remote storage start before `modem.mdt` is
loaded. After modem reset release and QRTR readiness, remote storage serves
the modem filesystem, the `IPA_INIT_MODEM_DRIVER` request receives a response,
and SERVREG reports `wlan_pd` UP before ICNSS connects to WLFW; WLAN-ready and
WMA FW-ready follow. The running kernel has no debugfs support, so this does
not expose IPA registers or the QMI response values. It confirms ordering,
not which provider is sufficient or how to reproduce those operations in
seL4.

The snapshot was taken from `dmesg` after temporarily switching adbd to root
with `adb root`; `adb unroot` restored the shell UID afterward. No device
partition, WLAN module, firmware, radio setting, DMA mapping, or SMMU state was
changed. The log contains nonfatal QCACLD warnings before the successful
ready event, so those warnings should not be treated as a firmware-start
failure in isolation.

## Live QCACLD WMI service bitmap capture (2026-10-24)

A kernel-matched, temporary QCACLD logger captured the active WMA
`SERVICE_READY` bitmap and WMI `SERVICE_AVAILABLE` extension during one
LineageOS Wi-Fi start. The module used the phone's exact 4.14.357 kernel
vermagic and all 395 imported symbol CRCs matched the stock module. It was
loaded only in memory; afterward the temporary file was removed and a reboot
restored the stock WLAN module. LineageOS returned to boot-complete with Wi-Fi
connected. The capture logs capability words only and changes no command,
firmware, DMA, or partition data.

The service-ready event reports firmware build `0x3226822e`, 32 base service
words, and zero additional host-memory requests. The captured base word at
index 24 is `0x00000009`; under the pinned QCACLD `service_id / 4` and
`service_id % 4` indexing rule, this confirms base service 99. The
`SERVICE_AVAILABLE` event reports segment offset 128 and little-endian words
`0x4531e0c8, 0x00512000, 0x00002000, 0x00000000`, which confirm extended
services 176 and 182 using the pinned `(service_id - 128) / 32` word and
`service_id & 31` bit mapping.

`tests/sunfish_qcacld_resource_test.rs` now feeds these captured raw bitmap
words, rather than synthetic enabled-service lists, into the regression that
reproduces all 68 live WMI INIT resource words and the regression that composes
the offline QCACLD INIT/HTC frame. This proves the service-bit parser and
resource policy agree with the live capability capture and WMI INIT capture.
The HTC credits remain synthetic. No WMI command has been submitted by seL4,
and raw SERVICE_READY/READY packet capture, transport delivery, firmware
startup, and radio operation remain unverified in NexatrodeOS.

## Target-side QCACLD INIT composition check (2026-10-24)

The no-radio Microkit protocol probe now reconstructs the captured base and
extended service bitmaps, checks services 99, 176, and 182, builds the
observed 68-word Sunfish resource profile, and composes/reparses the QCACLD
WMI INIT HTC frame. HTC endpoint, MTU, and credit values are explicit
synthetic inputs. The frame remains in bounded ordinary memory and is never
submitted to a CE ring. The system manifest maps only the splash framebuffer;
it has no WLAN MMIO, IRQ, SMMU, DMA, or secure-call capability.

`build-wlan-protocol-probe.sh` builds the AArch64 image successfully (64 root
objects; temporary `fastboot.img` size 18,669,568 bytes, SHA-256
`0fdc4850ba58fcbda3a7a75ec304e4bc4023b8cf4cb9f5282d54fbf4a38e716a`). On
2026-10-24 Fastboot accepted a temporary boot on `sunfish`; the target screen
result is described below. No partition was flashed. The first full host-suite run
exposed an older HTC composition fixture that reversed the WMI TLV tag/length
fields; the fixture now uses the captured wire order. The full
`test-modem-qmi.py` suite, QCACLD source validator, WLAN launch-path validator,
and no-radio isolation tests pass.

A follow-up LineageOS check confirms rooted ADB access is available, Android
has booted, Wi-Fi is enabled, and the stock 9,105,408-byte `wlan` module is
loaded. The previously captured resource and bitmap records already answer
the requested capture, so this check did not reload or replace the module.
ADB was returned to the shell user. Live transport delivery and radio startup
from the no-radio probe remain untested.

The 2026-10-24 target screen photo showed `CE DESC PASS`, `QRTR PASS`,
`WMI 10 4 PASS`, `HTC WMI TX PASS`, `SCAN FRAME PASS`, and `QCACLD INIT PASS`,
but `IPA CODEC FAIL`. Inspection found the probe's synthetic success response
declared a 20-byte QMI payload although its two TLVs occupy 14 bytes, and its
control-endpoint value disagreed with the expected value. The fixture is now
shared between the target probe and a host test. A second target photo still
showed the IPA failure because the result TLV omitted one zero byte. The
shared check now passes on the host, and the rebuilt no-radio image is
18,669,568 bytes (SHA-256
`88f481914b4b94c2716a6df4b96b8691a96874e4366a4ee389e21fec9c56f521`). It needs
one more target boot to confirm the IPA line changes to PASS. The full
`test-modem-qmi.py` validation completed successfully after the fix, including
host protocol tests and AArch64 `no_std` compilation. The image has no radio,
modem, transport, MMIO, SMMU, DMA, or secure-call access.

## Current decision gates

- The phone's installed WLAN firmware hash differs from the pinned public
  WCN3990 Linux-firmware image. Do not substitute it based only on related
  chipset naming.
- The Qualcomm Atheros firmware license for upstream ath10k images is a
  binary redistribution/use license, not source publication. Its stated use
  condition is a Qualcomm Atheros chipset, not Linux; preserve the notice and
  keep the binaries unmodified. Google's separate phone-specific firmware
  terms do not establish NexatrodeOS use rights.
- The phone uses downstream ICNSS/QCACLD and a different MSA layout from the
  mainline ath10k candidate.
- The observed ABL SMMU state is descriptive, not an available translated DMA
  domain. No WLAN DMA or firmware startup is authorized by this plan.

These gates keep work useful and testable without loading or starting WLAN
firmware on the connected Pixel. See [`pixel-wifi-feasibility.md`](pixel-wifi-feasibility.md) and
[`pixel-modem-lifecycle-design.md`](pixel-modem-lifecycle-design.md) for the
source observations and modem ownership state machine.

## Linux-firmware candidate audit and reference test

The pinned `linux-firmware` WCN3990 bundle is a candidate for a Linux reference
test before using the same unmodified bytes in the native port. Its `WHENCE`
entry assigns the Qualcomm Atheros license to the base WCN3990 bundle. The
license's stated use condition is a Qualcomm Atheros chipset, rather than a
Linux operating system; preserve its notice with any redistribution and do not
modify or reverse engineer the binaries. Google's separate phone-specific
firmware terms remain unresolved. The license file and `notice.txt_wlanmdsp`
must travel with any test bundle; none of these files is checked into the
repository.

A fresh text-only audit of upstream `WHENCE` at commit
`412eb7861d40b9dc3dfa01d4059d4b5c414b0307` confirms that the generic WCN3990
license group includes `board-2.bin`, `firmware-5.bin`, `wlanmdsp.mbn`, and
`notice.txt_wlanmdsp`, and refers to
[`LICENSE.QualcommAtheros_ath10k`](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/LICENSES/LICENSE.QualcommAtheros_ath10k).
That license permits binary redistribution without modification solely for
use with a Qualcomm Atheros chipset, requires the copyright notice and
disclaimer to accompany redistribution, and prohibits reverse engineering,
decompilation, or disassembly. This verifies the upstream license assignment
for the four named files; it does not settle how that condition applies to this
WCN3998 product or a NexatrodeOS distribution, and it does not establish
firmware or board-data compatibility. Only the [`WHENCE` manifest](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/WHENCE)
and license text were read in this check; no firmware payload was opened or
decoded.

At the pinned firmware commit, the following complete-set files were streamed
from the raw source into SHA-256 and byte counts. No firmware bytes were
retained or decoded. The `firmware-5.bin` role and flags below come from the
upstream ath10k firmware review and driver behavior, not from reverse
engineering the WLAN image or board-data file:

| File | Size | SHA-256 |
| --- | ---: | --- |
| `board-2.bin` | 893,528 | `c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d` |
| `firmware-5.bin` | 60 | `fef6539e0127579536bc977be57a90d018b83f2931fedc3a8870fbe38d6c4127` |
| `wlanmdsp.mbn` | 3,725,044 | `92e1501254e6de78c0f2e2cf091507d488b608d07e53acd14813a82744823ec2` |
| `notice.txt_wlanmdsp` | 27,005 | `d7a42cb726d992c427fc130b60b076c5536297aafceecdb053b1b5de17f7d5fa` |

The 60-byte `firmware-5.bin` is not a truncated WLAN image. It is ath10k API-5
feature metadata: the pinned file is a regular blob (`100644`), and the
upstream ath10k firmware review describes this WCN3990 file as a flags-only
container. Its `NON_BMI` flag tells ath10k to skip the host BMI image download
and wait for firmware that starts out of band; the other recorded flags are
`wowlan`, `mgmt-tx-by-ref`, and TLV WMI/HTT operation versions. Therefore the
runtime check must distinguish successful metadata parsing from successful
WLAN firmware startup. The separate `wlanmdsp.mbn` is the image candidate, and
its pinned hash differs from the phone-observed image. Linux-firmware history
also describes this base WCN3990 image as platform-specific to SDM845 and close
relatives; that does not prove compatibility with Sunfish's SM7150/WCN3998.
The board-data file must likewise be matched by ath10k using the device's
reported board/chip identifiers and `google_sunfish` variant; do not decode or
modify the binary to make it match.

### Pinned upstream snapshot reviewed 2026-10-22

The integrity table above pins a historical `linux-firmware` candidate; it is
not the only WCN3990 bundle upstream now publishes. The current
`linux-firmware` `WHENCE` snapshot at commit
`95c414d44c8e60880be787822d5020a010044a49` lists the generic WCN3990 image as
`WLAN.HL.2.0-01387-QCAHLSWMTPLZ-1` and a nested `qcm2290/` pair at
`WLAN.HL.3.3.7.c2-00931-QCAHLSWMTPLZ-1`. The `qrb4210/` image paths in that
snapshot are symlinks to the same QCM2290 pair, not a separate firmware
version. This snapshot has no `shikra` entry. The generic image is explicitly
platform-specific to SDM845 and close relatives in its upstream move commit.
The QCM2290 pair uses `LICENSE.qcom` and `qcom/NOTICE.txt`, not the generic
bundle's Qualcomm Atheros license. That separate agreement permits binary use
and redistribution subject to conditions including use only with Qualcomm
Technologies chipsets, inclusion of the terms file, no sublicensing, and no
reverse engineering. Its chipset condition does not establish a Sunfish
compatibility claim or remove the need to review the intended test and product
use. Neither current `WHENCE` nor the recorded firmware history identifies
these variants as Sunfish-compatible. The 2026 `board-2.bin` update documents
SDM845 HDK, SM8150 HDK, ECS QC710, Pixel 3 `blueline`, and Pixel 3 XL
`crosshatch` entries; it does not provide source evidence for the Sunfish
variant.

I streamed the files from that exact snapshot through SHA-256 and recorded
their sizes below; no firmware bytes were retained or decoded. The QCM2290
pair shares the parent directory's generic `board-2.bin` because WHENCE lists
no QCM2290-specific board file. This pins the reviewed candidates but does not
make either board selection valid for Sunfish.

| File at `95c414d44c8e60880be787822d5020a010044a49` | Size | SHA-256 |
| --- | ---: | --- |
| `ath10k/WCN3990/hw1.0/board-2.bin` | 670,116 | `c03d801cba1233914d777644e368ea942f36064e805ba6102514dedb47e53c76` |
| `ath10k/WCN3990/hw1.0/firmware-5.bin` | 60 | `fef6539e0127579536bc977be57a90d018b83f2931fedc3a8870fbe38d6c4127` |
| `ath10k/WCN3990/hw1.0/wlanmdsp.mbn` | 3,725,044 | `92e1501254e6de78c0f2e2cf091507d488b608d07e53acd14813a82744823ec2` |
| `ath10k/WCN3990/hw1.0/notice.txt_wlanmdsp` | 27,005 | `d7a42cb726d992c427fc130b60b076c5536297aafceecdb053b1b5de17f7d5fa` |
| `LICENSE.QualcommAtheros_ath10k` | 2,713 | `337a55102138d7baa143ee4a4c6c91693e0113fece35d380b2a12109e8c23b3f` |
| `ath10k/WCN3990/hw1.0/qcm2290/firmware-5.bin` | 60 | `80daf4f3a89e6ea8222e45387bb362c854fcd30c02e9da59ca723b39b269cdd2` |
| `ath10k/WCN3990/hw1.0/qcm2290/wlanmdsp.mbn` | 3,828,228 | `4a0f4d8a5d7cdc094a5b0b907f4b9906657b76070993fed4d8cc4ccf5776b515` |
| `LICENSE.qcom` | 13,962 | `be904cd28cb292b80cdb6cf412ab0d9159d431671e987ad433c1f62e0988a9bc` |
| `qcom/NOTICE.txt` | 23,966 | `fa43e1b9a13b341a07adca9dbe73d0f9072d7966fdfe811c01f0dd2872d7309a` |

### Earlier upstream follow-up (snapshot `00b1df13`, 2026-10-22)

A live check of upstream `linux-firmware` main resolved to commit
`00b1df13f9d0adebcb90bd110a4a22de5115e077`. It retains the generic and
QCM2290 entries above and adds a separate `shikra/` WCN3990 pair. The Shikra
files were added in commit
`f64d7db7d2646e1c7744963d540524f523ae0443` at version
`WLAN.HL.3.3.7.c5-00093.2-QCAHLSWMTPL-1`. Upstream assigns the Qualcomm Atheros
binary license: binary use/redistribution must be unmodified, for use solely
with a Qualcomm Atheros chipset, with notices retained; reverse engineering is
prohibited. That license condition does not establish a Sunfish firmware or
board-data match.

| File at `00b1df13f9d0adebcb90bd110a4a22de5115e077` | Size | SHA-256 |
| --- | ---: | --- |
| `ath10k/WCN3990/hw1.0/board-2.bin` (shared parent board data) | 893,528 | `c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d` |
| `ath10k/WCN3990/hw1.0/shikra/firmware-5.bin` | 60 | `7f35f16380124ceff31a4c3064fb6f66b366ccd76b517b271a53e4c9029fe41b` |
| `ath10k/WCN3990/hw1.0/shikra/wlanmdsp.mbn` | 3,811,820 | `f3a5efddcbffaf66c22bbcedb9f47a4f0fad60c4df158dc02cfc5e73c5159be9` |
| `ath10k/WCN3990/hw1.0/shikra/notice.txt_wlanmdsp` | 27,574 | `e9a920d4b55f62ac8a66fe7693aa74f461a75f330847f9b51f37a2565adb85d3` |
| `LICENSES/LICENSE.QualcommAtheros_ath10k` | 2,713 | `337a55102138d7baa143ee4a4c6c91693e0113fece35d380b2a12109e8c23b3f` |

WHENCE lists no Shikra-specific `board-2.bin`, so the driver would fall back to
the shared WCN3990 parent board file. Its Sunfish entry has not been verified.
The firmware repository later recorded that Shikra loads `wlanmdsp.mbn` through
`tqftpserv` from the DSP firmware directory
([loader-path source change](https://kernel.googlesource.com/pub/scm/linux/kernel/git/iwlwifi/linux-firmware/%2B/837ae0e5beccc2f1e974afba56c22c3cf981fc4c)).
No matching `tqftpserv` launch path is established in the pinned Sunfish
reference, and the observed Pixel WCN3998 image hash
`a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e` does not
match this candidate. The new pair is therefore a separately identified
platform candidate, not evidence that Shikra firmware is suitable for Sunfish.
The binaries and notice were streamed in memory for size and SHA-256; none was
saved or decoded.

The QCM2290 pair is still present at the current main snapshot with the same
two payload hashes listed for the earlier `95c414d` review. Its license files
have moved since that older snapshot: current WHENCE refers to `LICENSE.qcom`
and `NOTICE.qcom`, whose current paths are `LICENSES/LICENSE.qcom` and
`LICENSES/NOTICE.qcom`. Use the current paths and hashes in
[`wlan-resources.json`](../platforms/sunfish/wlan-resources.json) if an
approved test bundle is ever staged; the old `qcom/NOTICE.txt` path belongs to
the historical snapshot only. The exact current `LICENSE.qcom` text permits
installation/use and binary redistribution subject to its agreement: binary
format only, operation only on platforms incorporating Qualcomm Technologies
chipsets, inclusion of the agreement text with redistribution, and preservation
of notices. It prohibits reverse engineering, disassembly, and decompilation,
grants no patent rights, and bars applying third-party license terms to the
materials. Those clauses need a separate applicability and distribution review;
this source audit does not accept the agreement or establish that this
QCM2290 image is a Sunfish match.

### Current upstream Shikra firmware update (2026-10-22)

I rechecked `linux-firmware` main at
[`412eb7861d40b9dc3dfa01d4059d4b5c414b0307`](https://gitlab.com/kernel-firmware/linux-firmware/-/tree/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/ath10k/WCN3990/hw1.0).
The intervening commit
[`72f3b81c89fbc52677b9dbbfa92cde0ae00a36e2`](https://gitlab.com/kernel-firmware/linux-firmware/-/commit/72f3b81c89fbc52677b9dbbfa92cde0ae00a36e2)
updates only the Shikra `wlanmdsp.mbn` payload and WHENCE version, with the
commit message saying the update addresses stability issues. The new version
is `WLAN.HL.3.3.7.c5-00226-QCAHLSWMTPL-1`; the binary is 3,808,072 bytes with
SHA-256
`ea1787533f969c084c24292defdcc88d75d71955bea7e12ffee2b5fb16248350`.
The generic WCN3990, QCM2290 and board-data candidate hashes are unchanged at
this head, as are the Shikra API-5 metadata, notice and Qualcomm Atheros
license. The new Shikra image still has no matched Sunfish board entry or
out-of-band Sunfish loader, and its hash does not match the phone-observed
WCN3998 image. This freshness update therefore does not open the Linux
compatibility-test gate or change the candidate selection.

The raw candidate and notice files were streamed to SHA-256 without saving or
decoding their contents. The exact current main commit and updated Shikra
integrity values are pinned in
[`wlan-resources.json`](../platforms/sunfish/wlan-resources.json) and checked
by [`validate-wlan-resources.py`](../platforms/sunfish/validate-wlan-resources.py).

There is a loader-path issue to resolve before a runtime test. The Sunfish
`sm7150-mainline` tree at commit
`fd78d17954a86ea6ceb0c535e618ea129e09f8dd` sets ath10k's base firmware
directory to `ath10k/WCN3990/hw1.0`. The pinned `snoc.c` already reads an
optional WLAN `firmware-name` property into `board_name`; the core fetch helper
tries `<firmware-dir>/<board_name>/<file>` and then falls back to
`<firmware-dir>/<file>`. No driver backport is needed for the QCM2290
subdirectory selector. The current Sunfish `&wifi` node does not set
`firmware-name`, so an explicit test-only DTS overlay could set
`firmware-name = "qcm2290"`.

That selector is only a path choice, not a compatibility claim. It affects
files fetched through the ath10k helper, including API metadata and board data;
it does not launch the out-of-band `wlanmdsp.mbn` image or establish that its
version and board data match WCN3998. Because WHENCE supplies no QCM2290-specific
`board-2.bin`, the intended shared parent board file must remain available and
must contain a matching Sunfish entry. A disposable test tree should include
the QCM2290 pair and that explicitly shared board file, but omit the generic
root `firmware-5.bin` and `wlanmdsp.mbn`; that prevents accidental fallback to
the unverified generic payload while allowing the documented board-data
fallback. Log every resolved request path and stop if either firmware request
falls back to a root payload. A separately identified out-of-band loader is
still required before any radio probe.

[`validate-ath10k-firmware-path.py`](../platforms/sunfish/validate-ath10k-firmware-path.py)
now checks this lookup against the exact pinned Linux source and fails if any
of the three audited source files has local edits. It confirms that both the
API-5 image and `board-2.bin` use the subdirectory-first helper, while the
current Sunfish DTS leaves the property unset. The check is source evidence;
it does not test firmware compatibility or run a radio.

### Offline Sunfish ath10k compile checkpoint (2026-10-22)

The offline reference gate now passes its first build step. Using the pinned
`sm7150-mainline` source at `fd78d17954a86ea6ceb0c535e618ea129e09f8dd`,
`build-ath10k-reference-objects.sh` produced
`sm7150-google-sunfish.dtb` (SHA-256
`d6928a8b08634af858a9229f733e610f000facd9569cac0da69106a6134eda4b`) and the
AArch64 relocatable `ath10k_snoc.o` (SHA-256
`c6f82f6e30357fa81ce9f7881a75cbb7d14ff20687d16cabb5cb59bd6d5b3a41`). The
firmware-name source validator and WLAN resource/license inventory validator
also pass. The Linux candidate files remain unstaged; this build does not link
a loadable module or kernel image, resolve real firmware/board files, or
establish Sunfish compatibility. No radio or device test was run.

Audit outcome: the pinned generic `linux-firmware` candidate remains useful
for understanding ath10k's API-5 metadata handling, but it fails the
device-identity gate for a Sunfish compatibility claim. The phone-observed
WCN3998 firmware reports `3.2.2.9.558.0` and has SHA-256
`a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e`, unlike
that generic candidate.

A separate, more relevant Linux reference has now been identified:
[`miromraz/mobian-pixel4a-port` at `789679f`](https://github.com/miromraz/mobian-pixel4a-port/tree/789679f25f859bd741d690fe221d1cc68a23444e)
reports working 2.4/5 GHz Wi-Fi on the Pixel 4a and documents its firmware
inputs. Its matching kernel branch is
[`sunfish-venus-v7.2` at `2267e24`](https://github.com/miromraz/linux/tree/2267e248204e7e051e0f7355298949a6440de579).
The port uses `wlanmdsp.mbn` from
[`firmware-google-sunfish` at `fe63598`](https://github.com/sm7150-mainline/firmware-google-sunfish/tree/fe63598e1bd9884bd68ba89e4670dae4d70b1e3e),
the `google_sunfish` `board-2.bin` entry from
[`firmware-qcom-sm7150-ath10k` at `792c716`](https://github.com/sm7150-mainline/firmware-qcom-sm7150-ath10k/tree/792c716c85c06861c4d17bce5ec21fba7f0f7de6),
and Linux-firmware's 60-byte WCN3990 `firmware-5.bin`. The pinned manifest
records an exact SHA-256 match between the first file and the phone-observed
image, the board archive's size and hash, and an exact Sunfish
`bdwlan-sunfish.bin` match from the same community source as a separate
board-data identity check. The Mobian README is a third-party compatibility
report, not a reproduced result from this project; its working Wi-Fi claim does
not show that these inputs work through NexatrodeOS or the native seL4 path.

This resolves the earlier source-identity uncertainty for a Linux reference
set, but not the use/redistribution review. The two community firmware
repositories do not declare a license grant, and byte identity or availability
in a working port is not permission to bundle or redistribute the files. Keep
the generic upstream candidate and this Sunfish reference set distinct in any
test record. A test that only observes request metadata or service discovery
does not stage or serve either set and remains independent of that review.

The pinned Sunfish kernel source confirms a discovery map for
`msm/modem/wlan_pd` instance 180 with `kernel/elf_loader` and `wlan/fw`, and
the SM7150 domain table includes that entry. `ath10k_qmi_init()` registers a
WLFW QMI lookup as a client. In the reviewed source, the Sunfish WLAN node is
not a remoteproc, the only named MPSS image is `modem.mbn`, and no explicit
`wlanmdsp.mbn` or `tqftpserv` launch reference appears. This does not prove
that no out-of-tree userspace component exists; it means this pinned kernel
tree alone does not establish the image launcher or WLFW provider required
for a conclusive firmware test. The read-only source check is reproducible
with [`validate-wlan-launch-path.py`](../platforms/sunfish/validate-wlan-launch-path.py).
It passed on 2026-10-22 against the clean pinned tree at
`fd78d17954a86ea6ceb0c535e618ea129e09f8dd`, confirming those limits remain
true at the audited source revision.

The newer Shikra pair does not change the Sunfish finding: its source identifies
a Shikra-specific out-of-band `tqftpserv` loading path that has not been
matched to Sunfish, and its shared parent board data has no verified Sunfish
entry.

The compatibility test has an offline gate and a later, separately approved
Linux reference boot. Do not boot the Pixel as part of this audit. The offline
gate is to build the pinned `sm7150-mainline` kernel with its
`qcom,wcn3990-wifi` node and upstream `ath10k_snoc`, then verify the selected
firmware paths and startup dependencies from source. A separately approved
temporary boot with WLAN disabled can validate only the kernel/DT platform
baseline; it is not a firmware compatibility test. The current source review
does not pass the candidate identity or firmware-directory gates, so do not
stage a candidate bundle yet.

The candidate-specific plan is therefore: first finish the current metadata-only
QRTR observer run and use its screen result to record the actual RRQ path,
options, requesting peer, and whether WLFW service `0x45` appeared. Do not
restart while that result is pending. Then use the observed request and the
working Linux port's documented paths to check the native path resolver and
bounded TFTP session logic in host tests; these checks need no firmware payload
or device state change. In parallel, finish static inspection of the Linux
kernel/DT and a host test of the pinned firmware lookup behavior. Before any
device run that loads firmware, verify a source-backed Sunfish image launcher,
WLFW provider, matching board-data entry, MSA ownership, SMMU isolation, and
applicable license scope. A missing WLFW service is an infrastructure stop,
not a firmware result. A `firmware-5.bin` parse by itself remains metadata
evidence, never a compatibility pass.

Before an eventual device boot, confirm from the source/configuration that the
Linux image has a source-backed path to launch the out-of-band WLAN image and make
the WLFW service available. The `NON_BMI` flag means a successful request for
`firmware-5.bin` alone proves only that metadata was parsed; it does not load
`wlanmdsp.mbn`. If the disposable Linux image cannot identify a source-backed
loader for the candidate image or a WLFW service provider, the test is
inconclusive and must stop before radio startup.

Before staging a candidate bundle, require a license review that confirms the
applicable grant covers the WCN3998 target and the intended test; if that scope
is unclear, obtain written permission and stop. When a specific source-backed
candidate is identified and approved, stage only its unchanged files together
with the applicable license and notice, verify their SHA-256 values against
the pinned manifest, and do not decode or modify the board-data or WLAN image.

For any later candidate-specific run, pin one exact firmware snapshot and test
the matching API-5 metadata, out-of-band `wlanmdsp.mbn`, and board-data image as
one set. Record the exact requested paths and hashes before boot. First verify
that the selected source tree can actually resolve those paths for
`google_sunfish`; a generic driver probe or `firmware-5.bin` parse is not a
firmware-compatibility pass. Then separate the observed milestones in the log:
WLFW discovery/registration, MSA setup and readiness, board-data match and
download, out-of-band image start and FW_READY, WLAN_CFG/WLAN_MODE, and CE/HTC
startup. Stop on the first failure. A missing WLFW service is an infrastructure
stop and remains inconclusive; a missing board entry is a board-data mismatch;
an API-5 parse with no out-of-band FW_READY is not evidence that the WLAN image
was compatible or launched.

If those preconditions pass and a separate device check is explicitly
approved, use one-time `fastboot boot` only: do not flash or erase a partition,
change the LineageOS installation, or run this candidate through the seL4
loader. The temporary image must leave the network interface down and perform
no scan, association, or transmission. Capture the kernel log through:

1. Device-tree match and `ath10k_snoc` probe, including the reported chip,
   board, bus, MMIO, IRQ, DMA/IOMMU, and MSA resources.
2. Successful `firmware-5.bin` API-5 metadata parsing, explicitly showing
   `NON_BMI`, WMI TLV, and HTT TLV behavior.
3. WLAN image launch/readiness, WLFW service arrival, MSA ownership setup,
   capabilities, board-data selection and download, calibration report, and
   FW_READY.
4. `WLAN_CFG`, mission mode, CE initialization, HTC/WMI startup, and wireless
   device registration.

Stop at the first firmware-launch, board-data, QMI, CE, watchdog, or SMMU
error; do not add retries, force a different board match, bypass the SMMU, or
issue a manual PD restart. If WLFW is absent, record a service-discovery stop
and classify the run as inconclusive, not a firmware incompatibility. Reboot
to the existing LineageOS image after collecting the bounded log.

Source review adds two preflight conditions before that device run. The pinned
upstream Sunfish mainline DTS binds WLAN stream IDs `0x240` and `0x241` to the
apps SMMU and reserves MSA at `0x98d00000` for `0x200000` bytes; the captured
runtime FDT and Lineage's actual Sunfish `sdmmagpie` base DTS both declare
`0x240/0x1` for ICNSS. The generic `sm6150.dtsi` value `0x1640/0x1` is not the
Sunfish DVT overlay's base. Upstream `ath10k_snoc`
constrains DMA to 35 bits and requests a `0x100000`-byte MSA allocation. Its
QMI path calls `qcom_scm_assign_mem` to transfer MSA permissions among HLOS,
MSS_MSA, WLAN, and WLAN_CE. A successful Linux reference run is intended to
start a hardware subsystem and can change secure memory ownership when it
reaches MSA setup; it is not a read-only probe and does not authorize the same
operations from the seL4 WLAN domain.

The minimal test init must not load the main-modem remoteproc or package
Google's modem image. The same Sunfish DTS names `modem.mbn` for MPSS, and the
SM7150 MPSS driver does not auto-boot it. `ath10k_snoc` can register its WLFW
QMI client without that image, but it cannot reach firmware startup unless a
WLFW service is already available. The upstream candidate contains
`wlanmdsp.mbn`, not the separate `modem.mbn`. If the WLFW service is absent in
the temporary Linux boot, stop at service discovery and treat the test as
inconclusive; do not fetch or extract a Google modem blob to make it proceed.
The WLAN-disabled boot image must also leave the ath10k module unloaded and
must not start ADSP/CDSP remoteproc modules.

A pass means only that this unmodified bundle can start through the Linux
reference driver on this handset without those errors. It does not prove
seL4 DMA isolation, SCM ownership, safe PD restart, or license permission for
NexatrodeOS distribution. A metadata parse failure is evidence about the
ath10k API-5 flags file; an out-of-band launch or FW_READY failure is evidence
about the candidate platform image or its lifecycle path. Neither result
authorizes firmware extraction, patching, reverse engineering, or native seL4
loading.

## Offline Sunfish DTS baseline (2026-10-22)

The pinned `sm7150-google-sunfish.dts` was preprocessed with Apple Clang and
compiled with DTC 1.8.1 into `/tmp/sm7150-google-sunfish.dtb`, then decompiled
for a static property check. The resulting 89,678-byte DTB identifies WLAN as
`qcom,wcn3990-wifi`, maps the ath10k node to IOMMU stream ID `0x240`, points
`memory-region` at the 2 MiB `no-map` MSA reservation beginning at
`0x98d00000`, and retains the `google_sunfish` calibration variant. Its WLAN
node has no `firmware-name` property, so the driver does not select either
candidate subdirectory by default. These are DT source facts only; the build
does not test the WCN3998 handset, firmware, QMI, or SMMU ownership.

The kernel's own Kbuild DTB target also succeeds in the isolated
`/tmp/codex-sunfish-dtb-build` output tree. GNU Make 4.4 and `ld.lld` were
needed because macOS ships GNU Make 3.81 and Apple's linker is not supported
by this kernel's Kconfig checks. The resulting Kbuild DTB is 89,678 bytes
with SHA-256
`d6928a8b08634af858a9229f733e610f000facd9569cac0da69106a6134eda4b`.
After merging the pinned tree's `sm7150.config` fragment into `defconfig`,
Kconfig resolves `CONFIG_ATH10K_SNOC=m`, `CONFIG_QRTR=y`,
`CONFIG_QCOM_QMI_HELPERS=m`, `CONFIG_QCOM_SMEM=y`, and `CONFIG_ARM_SMMU=y`.
The defconfig's `CONFIG_QCOM_SMP2P=y` was dropped because `CONFIG_MAILBOX`
was unset, so I enabled the mailbox dependency in a scratch config and
rebuilt; the resulting kernel config has `CONFIG_MAILBOX=y`,
`CONFIG_QCOM_SMP2P=y`, and `CONFIG_QCOM_SMSM=y`, with their initcalls present
in `vmlinux`. The pinned ath10k SNOC composite object
`ath10k_snoc.o` now builds as an AArch64 ELF64 relocatable object in
`/tmp/codex-sunfish-ath10k-module`; it links the `qmi.c`,
`qmi_wlfw_v01.c`, and `snoc.c` objects. Kbuild needed temporary GNU utility
and host compatibility shims under `/tmp` for macOS system headers and BSD
`find`, `sed`, `readlink`, and `cp`. A separate output also contains the
matching module set: `ath10k_snoc.ko`, `ath10k_core.ko`, `qmi_helpers.ko`,
`mac80211.ko`, and `cfg80211.ko`. Their metadata reports the same
`7.1.0-sm7150+` AArch64 vermagic; dependencies are SNOC → core + QMI helpers,
core → mac80211 + cfg80211, and mac80211 → cfg80211. QRTR is built into the
kernel. The SNOC module SHA-256 is
`73d202866be1dc3f2f360bf531b0abe77ab8162165d7ecbffb6de57e597e997e`.
This verifies driver/module inputs only; the modules have not been packaged
into an Android boot image. The full pinned Kbuild `Image` and
`qcom/sm7150-google-sunfish.dtb` targets now succeed in
`/tmp/codex-sunfish-kernel-clean-build`; their SHA-256 values are
`7b4900860631a4a845ba4b0ad5aafff808ecb0482ed28f2b3bfc7369a8a54a0a` and
`d6928a8b08634af858a9229f733e610f000facd9569cac0da69106a6134eda4b`,
respectively. The `Image` is an AArch64 Linux kernel image, and the DTB is a
Sunfish Device Tree Blob. This build does not provide a source-backed WLAN
image loader or WLFW service, stage a firmware candidate, or test the handset.
The build remains an offline kernel/DT/module validation and does not authorize
a handset test.

The repository's reproducible build entry point,
[`build-ath10k-reference-objects.sh`](../platforms/sunfish/build-ath10k-reference-objects.sh),
was also run against the pinned source on 2026-10-22. It rebuilt the Sunfish
DTB and the `ath10k_snoc.o` Kbuild target in the containerized reference
environment. The DTB is 89,678 bytes with the same SHA-256 above; the
AArch64 ELF64 relocatable SNOC object is SHA-256
`c6f82f6e30357fa81ce9f7881a75cbb7d14ff20687d16cabb5cb59bd6d5b3a41`.
This makes the reference-driver compile check reproducible from the repository
script. It still does not build or boot a Linux image, load firmware, or test
WLAN compatibility on Sunfish.

## Next device validation checkpoints (2026-10-22)

The last read-only ADB check identified the handset as `sunfish` in LineageOS.
Its process list showed `qrtr-ns`, two `tftp_server` instances,
`modem_svc`, and `pd-mapper` running. A filtered scan of the last 5,000
all-buffer logcat records contains no TFTP request entries. This confirms the
services are present, but it does not show whether an RRQ occurred or what path
was requested; the shell remains unprivileged and no logging properties or
services were changed. The host-only `test-modem-qmi.py` suite also passes,
including the bounded TFTP parser tests and AArch64 `no_std` core/native
compilation.

The next useful handset observation is a passive LineageOS baseline of the
WLAN image-request path. The saved baseline proves that QRTR service `0x1000`
(TFTP) is advertised and that the TFTP server holds a wake lock, but contains
no file-level request, and ordinary filtered logcat did not add one. When a
read-only capture mechanism is available, record
only the requesting peer and RRQ path, mode, options, and requested size during
ordinary WLAN startup; do not retain TFTP DATA payloads. The open-source
Qualcomm reference describes this service as TFTP over AF_QIPCRTR, serving
files requested by a remote processor, and notes that the actual path is
client-dependent ([reference README](https://github.com/linux-msm/tqftpserv/blob/master/README.md)).
This baseline can confirm whether a WLAN image request uses an alias admitted
by `qcom_tftp`; it cannot establish that an upstream image is compatible or
that TFTP caused the earlier pre-QRTR failure. If the request cannot be
captured without modifying the running device, leave this checkpoint open
rather than treating server discovery as request evidence.

A later Linux reference boot remains a separate test, gated on finishing the
out-of-tree module/dependency packaging, reviewing and hash-pinning the
candidate bundle, and identifying a source-backed firmware launch path. It
must use `fastboot boot` only, preserve the installed LineageOS image, omit
main-modem remoteproc startup, and stop at the first missing WLFW service,
firmware-path fallback, QMI/MSA, board-data, FW_READY, CE, watchdog, or SMMU
error. A missing WLFW service is inconclusive; it is not a reason to fetch the
Google modem image or force a PDR restart. The current reference kernel and
DTB builds do not satisfy these gates, and this boot needs its own explicit
go-ahead.

The unprivileged LineageOS log check did not provide a path-level TFTP trace.
For the native seL4 path, resume device testing only after an offline change
passes the existing owner/QRTR tests and builds a bounded raw QRTR observer for
service `0x1000`. That observer should record only the requesting peer and the
RRQ path, mode, options, and size; it must not return TFTP DATA or retain
payload bytes. On the one-time temporary boot, also record whether WLFW
service `0x45` is announced. Stop after the first RRQ or a bounded timeout and
return to LineageOS. This separates QRTR/loader discovery from later firmware
compatibility and data-path tests.

A follow-up attempt to run the read-only LineageOS IPA capture on 2026-10-22
found no authorized ADB device; `fastboot devices -l` was also empty, and the
macOS USB tree contained no Pixel. The capture script refused to proceed and
made no device changes. Retry the capture only after the phone enumerates in
ADB; no Fastboot or firmware action is needed for that observation.

The metadata-only observer is now implemented and built as a separate
`sunfish-modem-tftp-observer` profile. It adds a local QRTR TFTP announcement
beside the existing single SERVREG state query and records only bounded,
sanitized request metadata. Its screen reports the `tsize=0` file-size-probe
flag explicitly, alongside the parsed block size, timeout, read size, window
size, and seek fields, so the stat-like first request is visible without
decoding an option mask. Host integration tests verify that an RRQ does not
reserve a QMI transaction or produce a TFTP response; the only resulting
outbound packet is QRTR receive-credit. The profile does not enable the PDR
restart path, WLAN MSA handoff, WLAN DMA, or file serving. Its temporary
Fastboot image passed the boot-layout and reserved-memory checks; an earlier
build was started once with `fastboot boot` (no partition flashing). After the
bounded owner gained its retained TFTP response buffer, the observer profile's
exact ELF hash and task-memory manifest were updated. The current image is
41,299,968 bytes with SHA-256
`180be7f373ad7c03c16ab238c1b656db71ebce0cdf49fde9cac83f96a309eb5d`; it passed
the full layout verifier, firmware-resource validator, and path-selection
validator. Both the initial observer and its error-detail rebuild were
temporarily booted with `fastboot boot`; no partition was flashed. The settled
photos show two QRTR server records and peer HELLO, but no tracked IPA or WLFW
announcement and no TFTP packet or RRQ. The later screen reports QMI
result/error `0/0`, WLAN state `7FFFFFFF` (`UNINIT`), and a modem-facing fatal
stop after the query. Its server instance words are `0000B401` and `00001202`;
the observer's later revision identifies their service IDs, but neither is the
tracked IPA or WLFW service. It sends no TFTP response, serves no firmware
bytes, and attempts no PDR or WLAN DMA. The observation establishes only that
this custom boot's current path stops before the tracked WLFW/TFTP activity; it
does not show whether a successful Linux WLAN startup uses TFTP. This
checkpoint is complete, so another wait or identical observer boot is not
useful. The no-firmware test was independent of the candidate firmware's
compatibility and license gate.

The native TFTP transfer core now preserves the requested `blksize` for
observation but caps the negotiated DATA block at 4,060 bytes: a 4,096-byte
QRTR datagram minus its 32-byte header and 4-byte TFTP DATA header. Regression
tests request the RFC maximum and verify the DATA packet still fits one QRTR
datagram. They also cover Qualcomm's custom ERROR code 9, which ends the
stat-like size probe without classifying it as a transfer failure, plus
malformed and ordinary ERROR packets. The new
[`qcom_qrtr_tftp.rs`](../rust/src/qcom_qrtr_tftp.rs) protocol layer binds
each accepted RRQ to a fixed-capacity session, a monotonically fresh local
transfer port, and the exact QRTR peer node/port. Synthetic tests cover the
`tsize=0`/ERROR-9 probe, final ACK completion, wrong peer endpoints, capacity,
reserved-port skipping, and non-reuse of retired transfer ports. It still does
not implement retransmission timers or select an authorized immutable image.
The native owner now has an explicit server profile: it holds each incoming
TFTP datagram in a private receive slot, dispatches it through the bounded
session table, retains generated output across transient GLINK/QRTR
backpressure, and releases the slot only after that output is queued or
retained. The observer profile remains unchanged and payload-free. Synthetic
owner tests cover the full RRQ-to-OACK-to-QRTR/GLINK path and a two-block read
where the second DATA response waits for the first response's QRTR credit.
They also verify exact transfer ports and completion behavior. The current
Pixel runtime still selects only the metadata observer. A separate
`TftpServerServices` adapter now lets an explicit runtime attach dispatch held
requests through the owner using a caller-provided immutable image; it binds
the Sunfish TFTP service and transfer ports and is covered by native-owner
tests. No firmware image is attached to the Pixel runtime, and no firmware
bytes have been loaded or served.
The upstream `tqftpserv` reference opens a new connected QRTR socket for each
RRQ and sends OACK/DATA on that per-transfer endpoint. The native protocol
layer now models fresh transfer ports and strict peer/TID matching, and the
GLINK transport can send its QRTR-wrapped response. The owner dispatcher routes
packets for both the advertised service port and unregistered transfer ports
into that table; registered QMI endpoints remain on their existing handlers.
The advertised service port alone is not the transfer endpoint. The Pixel
profile still does not provide an authorized image source
([reference implementation](https://github.com/linux-msm/tqftpserv/blob/master/tqftpserv.c#L463-L539)).
The same reference documents a first, stat-like RRQ with `tsize=0`, followed by
a second RRQ for the bytes. Its `rsize=0` log field is the default because no
`rsize` option was sent; it is not a zero-length request option. The first
request receives the file size through OACK, and the client may then send its
custom ERROR code 9 (`End of Transfer`) to close the probe. The metadata-only
observer already preserves the requested options and sends no OACK, ERROR, or
DATA; when its screen result arrives, use the actual opcode/options to tell a
size probe from a data request. The native serving path must account for this
two-request sequence without treating the custom stop code as a transfer
failure ([pinned reference README](https://github.com/linux-msm/tqftpserv/blob/0bf5b98c24229b0042554d99bb1509d813219f36/README.md),
[tsize parsing](https://github.com/linux-msm/tqftpserv/blob/0bf5b98c24229b0042554d99bb1509d813219f36/tqftpserv.c#L2785-L2798),
[OACK selection](https://github.com/linux-msm/tqftpserv/blob/0bf5b98c24229b0042554d99bb1509d813219f36/tqftpserv.c#L3107-L3123),
[ERROR code 9 handling](https://github.com/linux-msm/tqftpserv/blob/0bf5b98c24229b0042554d99bb1509d813219f36/tqftpserv.c#L3467-L3482)).

## Pixel 4a Linux-port sequencing cross-check (2026-10-22)

The pinned [Mobian Pixel 4a port](https://github.com/miromraz/mobian-pixel4a-port/tree/789679f25f859bd741d690fe221d1cc68a23444e)
adds a source-backed lifecycle reference that the kernel-only audit did not
cover. Its image setup declares `ipa` and the kernel `qcom_pd_mapper` as
prerequisites to the MPSS remoteproc. The Wi-Fi unit also orders QRTR namespace,
RMTFS, `tqftpserv`, and userspace `pd-mapper` ahead of its bring-up script. The
script checks or starts the modem remoteproc, waits for the QRTR WLFW service,
and only then loads `ath10k_snoc`; if WLFW does not appear, it fails closed
without loading the driver. This is a Linux reference sequence, not proof that
the same prerequisites or firmware path are implemented in the current seL4
owner. The new [`validate-mobian-wifi-reference.py`](../platforms/sunfish/validate-mobian-wifi-reference.py)
pins this source and checks the service order, Sunfish firmware path,
autoload blacklist, IPA and kernel PD-mapper soft-dependencies before MPSS,
WLFW-before-driver gate, and the final PHY-plus-netdev readiness condition. It
reads no firmware bytes and does not reproduce the reported Linux boot. The
`tqftpserv` executable and its actual runtime RRQs are not part of this source
audit, so the exact WLAN image request remains a separate observation.

The validator also reports a sequencing caveat: the pinned recipe retains an
IPA late-load unit that waits for modem WDS, while comments treat that late
load as a no-op after the soft-dependency. The source audit cannot verify that
runtime ordering, so the Mobian recipe is evidence for a candidate launch
sequence, not a reproduced or fully proven bring-up path. Its firmware
rehosts remain without a declared license grant.

The matched LineageOS boot log also shows IPA `INIT_MODEM_DRIVER` completing
before ICNSS connects to WLFW. The pinned IPA v3 source builds that request from
live memory partitions, endpoint mappings, hash-table support, and uC/SSR state.
The native codec in [`qcom_ipa.rs`](../rust/src/qcom_ipa.rs) encodes and decodes
the exact bounded QMI transaction, and its one-flight client now composes
through the GLINK/QRTR session, FIFO adapter, and serialized modem owner. A
host integration test verifies the explicit request/response path and receive
confirmation using synthetic runtime fields. The Pixel runtime does not enable
it; no trusted IPA runtime snapshot provider or AP-side IPA service exists.
This exchange is an observed ordering prerequisite in the working boot, not a
proven cause of the custom modem stall. Do not replace its runtime inputs with
guessed offsets or a synthetic success response.

The one-time seL4 observer checkpoint is complete: the current owner saw no
WLFW announcement or TFTP RRQ before its modem-facing fatal stop. The next
device check should follow an offline implementation change that gives a
specific hypothesis about reaching the WLFW announcement point. A separate
Linux reference boot could test launch ordering with the
hash-pinned, unmodified candidate: record each prerequisite and stop if the
modem does not reach `running`, WLFW is absent, or the driver reaches a firmware,
QMI/MSA, board-data, FW_READY, CE, watchdog, or SMMU error. That second test
needs its own explicit go-ahead because it starts the MPSS path. Neither test
authorizes a WLAN restart command, firmware extraction from the phone, or
loading firmware into the seL4 TFTP server. The reference script and its
service units are at [`wcn-wifi-bringup.sh`](https://github.com/miromraz/mobian-pixel4a-port/blob/789679f25f859bd741d690fe221d1cc68a23444e/work/wcn/sbin/wcn-wifi-bringup.sh),
[`wcn-wifi.service`](https://github.com/miromraz/mobian-pixel4a-port/blob/789679f25f859bd741d690fe221d1cc68a23444e/work/wcn/units/wcn-wifi.service),
and [`patch.sh`](https://github.com/miromraz/mobian-pixel4a-port/blob/789679f25f859bd741d690fe221d1cc68a23444e/work/patch.sh).

## Source map

- [`ce.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/ce.h): 64-bit descriptor layout, ring cursors, and Copy Engine API.
- [`ce.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/ce.c): WCN3990 64-bit send/receive descriptors and ring operations.
- [`hw.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/hw.c): WCN3990 CE registers and metadata field placement.
- [`htc.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/htc.h) and [`htc.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/htc.c): HTC control-message layouts and handshake, header and trailer-record framing.
- [`htt.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/htt.h): HTT command and response type fields and version negotiation layouts.
- [`wmi-tlv.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/wmi-tlv.h), [`wmi-tlv.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/wmi-tlv.c), and [`wmi.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/wmi.h): WMI-TLV tags, service-ready and host-memory-request layouts, and the reference bounds-checking iterator.
- [`wmi.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/wmi.c): host-memory chunk allocation, runtime count interpretation, and the allocation behavior this port must constrain.
- [`core.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/core.c): API-5 firmware metadata parsing and the `NON_BMI` path that skips host BMI firmware download.
- [`snoc.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/snoc.c): SNOC probe, resource setup, IRQs, power, MSA, and QMI startup.
- [Sunfish ath10k `qmi.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/qmi.c) and [`core.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/core.c): the reviewed firmware-directory selection and API-file fetch path.
- [Sunfish SM7150 DTS](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150.dtsi) and [Sunfish board DTS](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts): WLAN compatible, IOMMU SIDs, MSA reservation, calibration variant, and remoteproc firmware names.
- [Linux-firmware WCN3990 API-5 metadata review](https://groups.google.com/a/chromium.org/g/chromium-os-reviews/c/7YVeGZrVhtY): explains that `firmware-5.bin` contains driver feature flags, including `NON_BMI`, rather than WLAN firmware payload.
- [Linux-firmware WCN3990 platform-specific image move](https://kernel.googlesource.com/pub/scm/linux/kernel/git/srini/linux-firmware/%2B/a0142c57045701b7557c3060af5c4246c420e4d8): records the upstream note that the base `wlanmdsp.mbn` is for SDM845 and close relatives.
- [Pinned Linux-firmware WCN3990 `WHENCE` at the audited main snapshot](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/WHENCE): generic, QCM2290, and updated Shikra entries, QRB4210 symlinks, firmware version grouping, and license assignments.
- [Pinned Qualcomm QCM firmware license](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/00b1df13f9d0adebcb90bd110a4a22de5115e077/LICENSES/LICENSE.qcom) and [notice](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/00b1df13f9d0adebcb90bd110a4a22de5115e077/LICENSES/NOTICE.qcom): agreement conditions and redistribution notice for the QCM2290 pair.
- [Linux v6.10 ath10k firmware-directory selector commit](https://github.com/torvalds/linux/commit/5abf259772df8329dc95f1f9699617ae59eb0b95) and [core firmware lookup](https://github.com/torvalds/linux/blob/v6.10/drivers/net/wireless/ath/ath10k/core.c): `firmware-name` subdirectory lookup and generic fallback.
- [Sunfish-pinned ath10k SNOC source](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/snoc.c) and [Sunfish DTS](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts): the pinned driver supports the `firmware-name` subdirectory lookup, while its current Sunfish node leaves that property unset and selects `google_sunfish` only for board-data matching.
- [2026 WCN3990 board-data update](https://lists.infradead.org/pipermail/ath12k/2026-February/008568.html): enumerates the board variants added to the public `board-2.bin` refresh.
- [WCN3990 API-5 / image version discussion](https://lists.infradead.org/pipermail/ath10k/2023-December/015042.html): documents that the generic API-5 file and newer QCM2290/QRB4210 image are not version-matched, and that the actual `wlanmdsp.mbn` launch is out of band.
- [Pinned WCN3990 WHENCE entry](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/afabaf773c4c2e2c841429933a6a084a5af4d14d/WHENCE) and [Qualcomm Atheros firmware license](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/afabaf773c4c2e2c841429933a6a084a5af4d14d/LICENSES/LICENSE.QualcommAtheros_ath10k): candidate inventory and license terms.
- [`qmi.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/qmi.c): WLFW QMI startup and SCM MSA permission transitions.
- [`qmi_wlfw_v01.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/qmi_wlfw_v01.h): generated WLFW message IDs and exact QMI v01 array schemas.
- [Matched LineageOS IPA v3 `ipa_qmi_service.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c) and [`ipa_qmi_service_v01.h`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/include/uapi/linux/ipa_qmi_service_v01.h): service instance, `INIT_MODEM_DRIVER` message `0x0021`, 162-byte TLV maximum, and the runtime-derived platform/partition/endpoint fields used by the reference client; codec is offline-only in [`qcom_ipa.rs`](../rust/src/qcom_ipa.rs).
- [`arm-smmu.h`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/iommu/arm/arm-smmu/arm-smmu.h) and [`arm-smmu.c`](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/iommu/arm/arm-smmu/arm-smmu.c): common context-bank register offsets, fault bits, and kernel-selected context-bank allocation.
- [`qcom_q6v5_pas.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/remoteproc/qcom_q6v5_pas.c): SM7150 MPSS auto-boot policy.

## Offline Sunfish ath10k build checkpoint (2026-10-22)

[`build-ath10k-reference-objects.sh`](../platforms/sunfish/build-ath10k-reference-objects.sh)
successfully built the pinned Sunfish Device Tree Blob and the AArch64
`ath10k_snoc.o` from Linux commit `fd78d17954a86ea6ceb0c535e618ea129e09f8dd`.
The outputs were 89,678-byte DTB SHA-256
`d6928a8b08634af858a9229f733e610f000facd9569cac0da69106a6134eda4b` and
relocatable object SHA-256
`c6f82f6e30357fa81ce9f7881a75cbb7d14ff20687d16cabb5cb59bd6d5b3a41`.
This confirms the audited target DT and driver object compile together. It did
not link a module or kernel, read or stage firmware, or test WLAN hardware or
firmware compatibility; the Pixel remains on its existing LineageOS path.

## Offline HTT receive-buffer sizing checkpoint (2026-10-22)

The matched QCACLD build flags include `HELIUMPLUS`, `QCA_WIFI_3_0_ADRASTEA`,
`HTT_PADDR64`, and `ATH_PERF_PWR_OFFLOAD`. Its `ol_cfg.h` selects
`MAX_THROUGHPUT=800`; `htt_rx.c` sets both the maximum ring latency and, under
`QCA_WIFI_3_0`, the worst-likely fill latency to 20 ms. Applying the source's
integer formula gives 2,000 frames before power-of-two rounding: 2,048 HTT
address slots and an initial fill of 2,047 (the ring leaves one entry empty).

The HTT receive descriptor is 552 bytes with the matched `HELIUMPLUS` and
`QCA_WIFI_3_0_ADRASTEA` layouts. A host C layout check and an AArch64 compile
with a static size assertion both confirm that size. Adding the 1,560-byte
MSDU capacity and rounding to the arm64 128-byte cache line gives a 2,176-byte
mapped buffer. The initial receive set therefore needs 4,454,272 bytes for
buffers, plus a 16,384-byte 64-bit address ring and a 4-byte allocation index:
4,470,660 bytes before page-boundary and allocator overhead. The source also
creates a 64-buffer low-memory reserve with 139,264 bytes of additional buffer
capacity. Full-reorder mode adds a separate 4-byte target index; its runtime
configuration is not established by the pinned source checkout.

[`sunfish_htt_rx_profile.rs`](../rust/src/sunfish_htt_rx_profile.rs) mirrors
these source formulas as an offline arithmetic profile. It does not extend or
reuse the existing 416-KiB CE/WMI pool. The startup set needs at least 1,092
packed 4-KiB pages; including the 64-buffer fallback reserve raises the packed
capacity lower bound to 1,126 pages. Actual scattered receive buffers may need
more. The legacy LPAE planner remains capped at 128 pages, so a bounded
multi-table planner was added for maps up to 16 MiB within one 1-GiB TTBR0
region. Its test materializes all 1,092 startup pages across three L3 tables,
including the two 2-MiB boundaries, and verifies that the next IOVA remains
unmapped.

[`sunfish_htt_rx_ring.rs`](../rust/src/sunfish_htt_rx_ring.rs) now encodes the
48-byte one-ring HTT configuration and initializes the 2,048-entry 64-bit
address table with 2,047 startup buffer IOVAs plus the source-matched allocation
index. Descriptor offsets were taken from the compiled AArch64 QCACLD C layout.
The composed `WlanHttDmaPlan` carries that explicit ring plan and rejects a
sorted LPAE map unless every 4-KiB page covering the ring, index, startup
buffers, and 64-buffer fallback range is present. The integration fixture packs
those ranges into the 1,126-page HTT window beginning at IOVA `0x80200000` and
combines it with the separate 104-page CE/WMI arena; the resulting map needs
four L3 tables. The integration test materializes that table image, walks all
1,230 mappings to their supplied synthetic physical frames, and verifies the
pages immediately after the CE arena and at the end of the HTT window are
unmapped. The host tests validate payload encoding, bounded ring-image writes,
complete planned page coverage, and table-image translation only. The physical
frames are synthetic: these tests do not allocate the buffers, establish
CPU/device physical identity or ownership, perform cache maintenance, or verify
a live route. A native RX allocator and execution owner still need those facts
plus a verified installed route/context before any device use.

The index value also matches the pinned QCACLD initialization path: RX setup
zeros `alloc_idx`, fills the ring before sending the HTT configuration, and
the fill routine writes its post-fill index back; the config encoder reads that
live index. With all 2,047 requested buffers posted, the resulting index is
2,047 as modeled here. If allocation or DMA mapping stops the fill early, the
runtime must publish the actual post-fill index and corresponding buffer
count; this offline full-fill plan does not model partial initialization.

The saved SMMU handoff screen labels SIDs `0x240` and `0x241` as `NO ROUTE`,
and `0x0300` as translated to context bank 2 with that bank `OFF`. Those are
probe-reported snapshot fields, not a root-cause diagnosis. They also mean the
offline `0x240/0x241` install plan does not describe a verified live route
state. The user subsequently reported returning the Pixel to LineageOS; that
does not refresh the route snapshot, and no new SMMU read was performed. No
device settings or SMMU state were changed.

## Sunfish QCACLD HTT receive descriptor decode

[`rust/src/sunfish_qcacld_rx.rs`](../rust/src/sunfish_qcacld_rx.rs) now parses
the completed low-latency RX descriptor fields needed to expose one ordinary
MSDU: firmware flags, completion/error bits, ring-2 chain count, payload
length, first/last-MSDU markers, and the target-gated L3 padding. The
single-buffer API returns a borrowed payload only when the descriptor and
payload are in the same full-size ring buffer. It refuses chained MSDUs and
firmware extensions; it does not reclaim the descriptor, perform DMA/cache
operations, parse 802.11 headers, or claim an RX ring owner.

`validate-sunfish-qcacld-rx.py` checks these fields against the clean
LineageOS `28f9290ae067d3e7857126eb6312720dbe9c7c80` sources, including the
Adrastea masks and QCACLD's `HTT_WIFI_IP(pdev, 2, 0)` padding condition. Six
host tests cover the source fields, padding offsets, completion gating,
single-buffer bounds, and explicit chain/extension rejection. The module is in
the AArch64 `no_std` compile harness. This supplies one future RX datapath
primitive; it does not advance firmware start, SMMU ownership, or radio
readiness.

This behavior is specific to the installed LineageOS QCACLD comparison path:
`htt_rx_amsdu_pop_ll()` marks an MSDU chained when the ring-2 continuation
count is nonzero, and `ol_rx.c` routes that MPDU to `chain_msdus()` for buffer
release instead of passing it to reorder/delivery. Its comment leaves
stitching as future work and discards the chain. The planned native driver
reference is upstream ath10k, so this QCACLD behavior does not establish how
the native path should handle chains. Check ath10k's own RX indication and
buffer-chain rules before choosing a native disposition. This QCACLD parser
remains a Lineage comparison primitive; it does not claim to decode ath10k
descriptors or provide the native ring owner/reclaim path.

## Bounded WCN3990 ath10k HTT receive decode and ring preflight

[`rust/src/ath10k_htt_rx.rs`](../rust/src/ath10k_htt_rx.rs) decodes the pinned
WCN3990 `htt_rx_desc_v2` completion, length-error, ring-2 continuation, MSDU
length, decapsulation, and last-MSDU fields. Its bounded unchainer follows
ath10k's `htt_rx_amsdu_pop()` and `ath10k_htt_rx_h_unchain()` policy: chained
buffers have payload only, the first buffer contributes 1688 bytes from its
2048-byte ring slot, continuations contribute up to 2048 bytes, and only a
single RAW logical MSDU with the exact expected queue count is assembled.
Unsupported decapsulation and aggregate queue shapes are dropped explicitly.

The same module parses the pinned TLV `RX_IN_ORD_PADDR_IND` message when
firmware advertises `WMI_SERVICE_RX_FULL_REORDER`. It decodes the seven-byte
indication header and bounded 64-bit `{buffer_iova, msdu_length, fw_desc,
reserved}` entries, capping the list at the WCN3990 ring's 2047 posted buffers.
Reserved fields and any tail bytes remain visible to the caller; the parser
does not resolve or dereference IOVAs. The source validator checks the TLV
message mapping, service-bit selection, 64-bit descriptor layout, and
target-64 dispatch. Packet decoding remains separate from the software
ownership ledger and future native DMA reclaim path.

`validate-ath10k-htt-rx.py` checks those rules and fields against the clean
pinned Linux source at `fd78d17954a86ea6ceb0c535e618ea129e09f8dd`. Five host
tests cover descriptor decoding, single-buffer slicing, chain assembly,
upstream drop conditions, and fail-closed bounds; three more exercise the
in-order packet header, 64-bit entries, and truncation/count bounds. The RX
decoder and ring planners together have thirteen focused tests and are included
in the AArch64 `no_std` harness. They process caller-owned snapshots and
output buffers; they do not access the radio or establish firmware
compatibility.

The separate `rust/src/ath10k_htt_rx_ring.rs` planner follows the pinned
WCN3990 64-bit ath10k path: a 2048-slot address ring with 2047 initially posted
2048-byte buffers, a four-byte allocation index, a 360-byte first-buffer
descriptor, and a 48-byte `RX_RING_CFG` payload (without the outer HTC header).
Its descriptor word offsets were checked by compiling the pinned Linux C
structures on the host. The planner encodes the initial ring, allocation
index, and config into caller-owned slices, then validates sorted SMMU page
coverage and rejects physical page aliases among those DMA regions. Five more
host tests cover those bounds and encodings. The source audit also checks the
pinned ring setup and layout.
This remains preflight only: it does not allocate, map, authorize, or touch DMA
memory, submit HTC/HTT traffic, reclaim buffers, or start the radio.

[`rust/src/ath10k_htt_rx_owner.rs`](../rust/src/ath10k_htt_rx_owner.rs) adds a
host-testable ownership ledger around that ring plan. It models the two
mutually exclusive modes selected by ath10k's WMI service map. FIFO mode
reclaims the software read slot and address entry before advancing its read
index. Full-reorder mode instead matches each returned IOVA to a live buffer,
removes that buffer from device ownership, and deliberately reports no ring
slot: the pinned Linux path removes it from the address hash and decrements
`fill_cnt` without clearing a FIFO slot. Refill uses a prepare/publish/commit
contract so the caller must write the address, issue the required barrier,
publish the allocation index, and only then commit software ownership. If
publication may have reached the device but its result is uncertain, the
ledger quarantines the ring. Mode selection reads the pinned TLV full-reorder
service bit and fails closed if that bitmap word is absent. The source
validator checks the bit index and separate reclaim paths against pinned
`wmi-tlv.h`, `core.c`, and `htt_rx.c`; eight ownership tests cover both modes,
service-bit selection, out-of-order IOVA completion, wraparound, atomic
rejection, stale leases, and quarantine. The combined decoder, ring, and owner
tests total twenty-one, with a separate WMI test for the capability bit. This
remains a software ownership model: it does not allocate or map DMA memory,
touch shared ring bytes, perform cache maintenance, or start WLAN.

## WCN3990 ath10k WMI 10.4 INIT resource profile

[`rust/src/ath10k_wmi.rs`](../rust/src/ath10k_wmi.rs) can now construct the
complete 42-word WMI 10.4 resource block from the pinned ath10k WCN3990 policy.
The base profile uses the WCN3990 hardware's 33-peer override, 16 vdevs, and
96 TIDs. The parsed service-ready bitmap can then enable peer caching, which
overrides the peer count to 528 and sets active peers/TIDs to 66/132, or
51/102 when API-5 metadata also advertises peer flow control. For API-5
metadata selecting WMI operation version 10.4, ath10k uses the non-TLV fixed
`wmi_service_ready_event` (160-byte fixed prefix, 16 service words at offset
20, request count at offset 156, then inline 16-byte memory requests). The raw
WMI 10.4 peer-caching bit is 37; `wmi_10_4_svc_map()` maps that to Linux's
normalized internal `WMI_SERVICE_PEER_CACHING` bit 78. The target profile reads
raw bit 37, not normalized bit 78. Chain masks remain `0x7/0x7`, with
native-Wi-Fi decapsulation and the target's other 10.4 defaults. The fixed
non-TLV READY event is decoded separately from WMI-TLV.
Its host-memory count helper mirrors ath10k's dynamic request rules: peer and
vdev counts add one target slot, and active-peer counts fall back to the peer
count when zero. Fixed-count requests remain policy-gated firmware values.
The bounded [`ath10k_firmware.rs`](../rust/src/ath10k_firmware.rs) parser reads
the pinned API-5 metadata format and exposes WMI operation version and known
feature bits without exposing any embedded image bytes. The resource profile
requires parsed WMI version 10.4. It uses 1,424 descriptors unless the parsed
metadata advertises `peer-flow-ctrl`, in which case it uses 2,500. This does not
establish that any candidate image is compatible.

`validate-wlan-launch-path.py` checks the resource field order, target
constants, WCN3990 hardware overrides, WMI-version setup, and the
peer-flow-control, peer-caching, and host-memory count branches against the
clean pinned source at
`fd78d17954a86ea6ceb0c535e618ea129e09f8dd`. The combined CE/HTC/API-5/WMI
test binary covers the fixed WMI 10.4 SERVICE_READY and READY layouts, raw
service-bit mapping, and INIT command ID prefix alongside the generic WMI-TLV
decoder. It takes the generated INIT command through a synthetic HTC READY
and WMI_CONTROL connect response, uses the returned endpoint and credit policy
to serialize the HTC frame, consumes the rounded frame credit, and serializes
the WCN3990 CE descriptor. The pinned-source audit checks HTC IDs, header
fields, credit behavior, and CE64 descriptor order. This is host-side byte
composition only: it does not allocate host memory, create SMMU mappings, grant
DMA authority, authorize firmware, or submit the descriptor. The suite is
included in `test-modem-qmi.py`; the core also compiles for AArch64 `no_std`.

## Upstream WCN3990 board-file freshness recheck (2026-10-23)

A fresh GitLab API check resolved `linux-firmware` `main` to
`412eb7861d40b9dc3dfa01d4059d4b5c414b0307`, the same commit already pinned
above. Streaming only `ath10k/WCN3990/hw1.0/board-2.bin` through a byte counter
and SHA-256 yielded 893,528 bytes and
`c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d`, matching
the existing manifest; the file was not saved or decoded. The
[2026-08-09 upstream update](https://gitlab.com/kernel-firmware/linux-firmware/-/merge_requests/1171)
changes that binary file and provides no text listing its embedded board
variants. The current [WHENCE entry](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/WHENCE)
still documents the generic WCN3990 parent file plus separate Shikra and
QCM2290 firmware subdirectories, but no Sunfish-specific board-file path.
WHENCE does not enumerate the opaque board entries, so this confirms source
freshness and the documented license provenance, not a Sunfish board match.
No firmware was staged, decoded, or tested on the device.

## Bounded Sunfish SNOC API-2 board-data selector (2026-10-23)

[`rust/src/ath10k_board.rs`](../rust/src/ath10k_board.rs) now parses the
`board-2.bin` API-2 container without allocation and returns a borrowed view of
one exact board-data payload. It checks the NUL-terminated magic and its
four-byte header padding, bounds each little-endian IE and aligned payload,
requires a matching board-name IE before accepting the following board-data
IE, and distinguishes main board from extended board sections. The SNOC QMI
helper builds the same three candidate names as the pinned Linux path—from
runtime board/chip IDs, adding `variant=google_sunfish` to the primary name—and
searches them in source order. The helper is restricted to main board data;
extended board data continues to require the separate BMI eboard identity.

Twelve host tests cover exact and ordered fallback matches, the Sunfish variant
name, board/extended-board separation, source-style short-tail handling,
padding, malformed/truncated lengths, and the absence of an implicit generic
fallback. A composed host test passes a synthetic container through this
selector and the WLFW client's BDF segment encoder, then checks that only the
selected payload bytes appear in the serialized QMI request. It uses the
client's test-only BDF fixture; it does not create a production verification
token or serve an asset. `validate-ath10k-board-data.py` checks the behavior
against the clean pinned Sunfish ath10k commit
`fd78d17954a86ea6ceb0c535e618ea129e09f8dd`. The complete
`test-modem-qmi.py` suite, including the AArch64 `no_std` core compilation,
passes with the parser integrated. All test data is synthetic: this does not
establish that any licensed board file contains a matching Sunfish entry, and
it does not authorize or perform board-data transfer, firmware loading, DMA,
or WLAN startup.

## Fresh LineageOS WLAN readiness baseline (2026-10-24)

The read-only capture at
[`lineage-ipa-state-20261024.txt`](../.cache/pixel-sunfish/lineage-ipa-state-20261024.txt)
identifies the connected device as Sunfish running LineageOS
`23.2-20261001-NIGHTLY-sunfish`. In the observed boot-log order, Lineage starts
IPA, QRTR, TFTP, PD mapper, service-locator, and remote-storage providers
before modem startup. Modem QMI readiness is followed by a successful IPA
`INIT_MODEM_DRIVER` response. The service notifier then reports
`msm/modem/wlan_pd`, ICNSS connects and ACKs the current state, and the kernel
records both `WLAN FW is ready` and the WMA FW-ready event. Remote storage also
serves modem FS reads during boot.

This confirms the full reference firmware-ready milestone. A later read-only
`cmd wifi status` and `dumpsys wifi` check confirms Wi-Fi enabled, connected,
and supplicant `COMPLETED`; shell access still denies IPA debugfs and
`wlan0` interface-state reads. The private initial `wlan_pd` launch trigger and
raw IPA QMI response values remain unknown. No TFTP RRQ appeared in the
filtered logs. The capture made no device changes. The matching pinned source
audits still pass: `validate-icnss-startup.py` confirms the initial WLFW
arrival path does not invoke PDR but cannot identify the launch trigger;
`validate-mobian-wifi-reference.py` confirms the candidate provider order and
WLFW-before-driver gate, with its previously documented late-IPA ordering
caveat. A current-boot MSM-bus trace now maps 49 handles and 57 paths, but does
not expose active usecases or vote values. The next native implementation
checkpoint remains trusted IPA runtime state and the modem-side service
providers required before WLFW discovery.

A read-only inspection of the current LineageOS vendor init configuration
narrowed the missing initial-launch path. `/vendor/etc/init/hw/init.sm7150.rc`
starts `tftp_server` and `pd_mapper` in the core class and also starts
`modem_svc -q`; all three services report running. A targeted search of the
vendor/odm init and pd-mapper configuration directories found no explicit
`wlan_pd` or `wlanmdsp.mbn` launch declaration. This rules out a direct init
service declaration in the searched configuration, but it does not reveal
what `modem_svc`, the modem firmware, or the PDR implementation does internally.
No vendor executable was inspected or modified. ADB was returned to the shell
user; LineageOS remains boot-complete with Wi-Fi connected.

The public LineageOS Sunfish device tree independently shows the declared
startup boundary at pinned commit
[`52074bf3`](https://github.com/LineageOS/android_device_google_sunfish/tree/52074bf3c0b66efcba89f557e605535feb29aaab).
[`init.hardware.rc`](https://github.com/LineageOS/android_device_google_sunfish/blob/52074bf3c0b66efcba89f557e605535feb29aaab/init.hardware.rc)
places `tftp_server`, `modem_svc -q`, and `pd_mapper` in the core class, while
`cnss-daemon -n -l` is late-start. The separate
[`init.insmod.sunfish.cfg`](https://github.com/LineageOS/android_device_google_sunfish/blob/52074bf3c0b66efcba89f557e605535feb29aaab/init.insmod.sunfish.cfg)
loads `wlan.ko`, and `BoardConfig-common.mk` selects `qca_cld3`. The audited
public init and modem-link files contain no direct `wlan_pd` or
`wlanmdsp.mbn` launch line. This records module insertion and the visible
service declarations; it does not establish what the proprietary `modem_svc`,
CNSS daemon, or modem firmware does after startup. The source audit is now
automated in [`validate-wlan-launch-path.py`](../platforms/sunfish/validate-wlan-launch-path.py)
and pins the device tree alongside Linux and pd-mapper. The unresolved path is
narrowed to those opaque vendor/modem components and their runtime interaction
with the already-started TFTP and locator services.

The same pinned tree's
[`sunfish/proprietary-files-vendor.txt`](https://github.com/LineageOS/android_device_google_sunfish/blob/52074bf3c0b66efcba89f557e605535feb29aaab/sunfish/proprietary-files-vendor.txt)
labels its entries as blobs extracted from a release and lists `cnss-daemon`,
`modem_svc`, `pd-mapper`, `tftp_server`, and `wlanmdsp.mbn`. This manifest
documents packaging provenance; it contains no implementation source and is
not a license grant or a finding that a test is prohibited. The public-source
limit is that the Sunfish launch behavior remains inside vendor binaries or
modem firmware. The separate upstream ath10k binary license and the distinct
Google phone-blob terms are audited below; their compatibility and distribution
scope must be considered independently from this missing launcher source.

The pinned Linux APR binding clarifies the `kernel/elf_loader` label: it is a
service/path pair for an APR service, and
[`apr.c`](https://github.com/torvalds/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/soc/qcom/apr.c)
registers a PDR lookup for that pair. It registers the matching APR client
device only after the protection-domain state is UP and removes it on DOWN.
That is AP-side discovery and client-device lifetime after a domain exists;
the audited path contains no operation that loads or starts `wlan_pd`. The
validator now pins this distinction against `apr.c` and the
[`APR service binding`](https://github.com/torvalds/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/Documentation/devicetree/bindings/soc/qcom/qcom,apr-services.yaml).

## Native IPA modem-init client lifecycle (2026-10-23)

The native owner now has an opt-in `IpaModemInitServices` adapter for the
source-observed AP-to-modem QMI `INIT_MODEM_DRIVER` request (`0x0021`). It
reserves the IPA client port before QRTR control starts, waits for the exact
modem IPA service/version/instance announcement, sends one request using
generation-bound runtime fields, and faults on a rejected or indeterminate
response. Normal WLAN MSA-info progression now also requires this client
readiness. If the response sets `modem_driver_init_pending`, MSA stays gated
until the AP IPA server commits the separate `INIT_MODEM_DRIVER_CMPLT`
(`0x0035`) response after transport completion.

The request fields remain guarded by an opaque `InitPermit`, which can only be
minted inside the crate after a trusted provider completes IPA post-init and
captures the matching generation's live SRAM, endpoint, and uC state. That
provider still does not exist, so the default phone runtime remains disabled;
the native-owner fixture uses a test-only synthetic permit. Offline tests cover
the permit binding/stale-generation rejection, client endpoint reservation,
the readiness gate for pending modem completion, and the bounded QMI client
exchange. This is a validated lifecycle adapter, not a device-side change or
proof that IPA initialization alone will resolve WLAN startup.

The owner profile also compiles offline through
`platforms/sunfish/build-modem-owner-offline.sh`; its AArch64 owner ELF and
Microkit/CapDL mapping and pool checks pass. This is compile-only validation
of the default `NoServices` runtime, not evidence that the opt-in IPA adapter
has a production provider or has run on the Pixel. The Sunfish QMI test runner
and all 14 focused native-owner lifecycle tests pass after the final API-boundary
fix.

The IPA bus profile now expands each four-path clock usecase into route-node
reports and feeds them into a fixed-capacity, generation-bound MSM-bus ledger.
The ledger refuses totals until every configured client reports and faults on
stale or malformed reports. The 53-test IPA/BCM/aggregation harness and the
full Sunfish QMI runner pass, as do the source validator and offline owner
Microkit/CapDL build. The four IPA routes match the current live-derived DTB
and pinned arbiter; the second client's bandwidth fixture is synthetic. The
ledger still needs an authoritative seL4 client set, a complete route table for
all configured clients, and an RPMh transaction owner before it can provide
the IPA power vote.

## Pinned upstream WCN3990 board-key audit (2026-10-23)

I fetched the pinned upstream
[`board-2.bin`](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/ath10k/WCN3990/hw1.0/board-2.bin)
into memory and inspected only its API-2 IE names. It remains 893,528 bytes
with SHA-256
`c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d`, matching
the pinned manifest. The archive contains 36 main-board names and none contains
`sunfish`; in particular, it has no `variant=google_sunfish` entry. This
candidate therefore does not pass the exact Sunfish board-variant identity
check.

The Linux selector also tries QMI board/chip IDs without a variant, then a
board-only key. Those runtime IDs have not been captured for this ath10k path,
so this scan does not rule out an accidental fallback match. Until the IDs are
known and a fallback is independently source-validated, the upstream archive
cannot be treated as Sunfish board data. The opaque board payloads were not
decoded, saved, modified, staged, or loaded.

At this pinned revision, [`WHENCE`](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/412eb7861d40b9dc3dfa01d4059d4b5c414b0307/WHENCE)
declares the WCN3990 files redistributable under
`LICENSE.QualcommAtheros_ath10k`. That repository declaration does not by
itself settle whether the license scope covers a NexatrodeOS release. This
audit used the file only to read its board-name metadata and did not stage or
distribute it.

## One-shot IPA post-init coordinator (2026-10-23)

[`rust/src/sunfish_ipa_post_init.rs`](../rust/src/sunfish_ipa_post_init.rs)
now coordinates the pinned Lineage IPA `ipa3_post_init()` checkpoints in
source order: establish register access, perform the fixed-register stage,
validate the modem SRAM partition, allocate GSI support buffers, initialize
interrupts, register GSI, connect AP pipes, then capture and validate the
runtime fields used for `INIT_MODEM_DRIVER`. It returns a generation-bound
readiness value only after every backend step succeeds. The production runtime
uses that value to mint both the AP IPA service permit and modem-init client
permit; their constructors now require opaque post-init evidence rather than a
naked generation integer.

The runner passes its generation to every backend stage. Any backend error or
invalid SRAM/endpoint snapshot quarantines that runner; re-running a partial or
completed sequence is rejected. Six focused tests cover source order,
generation mismatch, failure quarantine/no replay, invalid geometry, zero
generation, and failure of the initial bus vote. The source-matched first-
access sequence applies the IPA v4.2 SVS2 bus usecase before register mapping.
On Sunfish, `qcom,bandwidth-vote-for-ipa` makes `ipa3_get_clks()` leave the GCC
core-clock handle null; the 50 MHz SVS2 value is only the logical selector for
bus usecase 1. Linux calls its clock callback, but that callback has no GCC
clock to prepare or set, and the IPA v4.2 uC clock notification returns
without sending a command because notifications are unsupported past v4.0.
The validator checks these target-specific conditions and the bus-vote-before-
MMIO ordering. The coordinator remains an orchestration contract: there is no
trusted production backend for IPA bus voting, MMIO, SRAM, GSI buffers, IRQs,
PAS firmware, or SMMU ownership. The Pixel was not touched, and the existing
`NoServices` runtime remains disabled for WLAN.

## Third LineageOS IPA bus-transition capture (2026-10-23)

The existing 30-second comparison artifacts in
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261024-012626/`
were re-parsed offline with `run-msm-bus-wifi-comparison.py` and reproduced
the saved `msm-bus-update-trace-windows.json` exactly. The connected baseline
had no IPA handle-10 update events. After Wi-Fi-off was confirmed, handle 10
made four brief `0→1→0` cycles (MSM-bus usecases MIN and SVS2), all within
about 1.24 seconds of the transition. During re-enable, four similar cycles
spanned association; the final `1→0` arrived 44 ms after the connected marker.
The post-connect window then had no further IPA updates. Across the trace,
9,392 request updates and 9,126 devfreq samples were parsed with no unknown
handles, bus errors, or failed paths. This is shared IPA-client behavior
correlated with Wi-Fi transitions, not proof that WLAN owns or caused the
votes; no WLAN-specific bus client appeared.

The capture still cannot support a complete aggregate: handle 20 (`grp3d`)
has 18 updates logged with `num_paths=2`, while the paired registration
snapshot and pinned `sdmmagpie-gpu.dtsi` declare one path. The report marks
those events as a snapshot mismatch instead of assigning them a route. The
trace strengthens the expected IPA transition profile but does not close the
global client/route inventory or supply a production RPMh owner. The markers
confirm Wi-Fi was restored to connected, and the before/during/after files
show the temporary dynamic-debug flags returned to their original state. The
report, raw trace, marker, and joined inventory SHA-256 values are
`1be8792b78bf624f3363837a7969a8a98d97a8ee93fed0a09dc5139bd76ffee6`,
`a6ae190072f73ed780ea8e0e8657022e9ecc92a165ea30c745b61d4b1ae9d721`,
`f216234247866a40bce451be63f32337ff64ba7c26e4cc385d576749a174357c`, and
`a60ae7b67631a1f53646d6e8e92c151cb2a41e9e115b6507e128de8e066a09bb`,
respectively. No seL4 device test or bus vote was performed.

## Fourth LineageOS IPA bus-transition capture (2026-10-23)

The comparison was repeated with a same-boot runtime FDT and filtered
`register_client_adhoc` log, targeting the Pixel by ADB serial because an
Android emulator was also attached. Artifacts are in
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261024-040041/`.
LineageOS reports `BP4A.251205.006` on kernel
`4.14.357-openela-g28f9290ae067`. The runtime snapshot contains 49 unique
handles and 57 logged routes; every route resolved. IPA is handle 10 with the
same four endpoints `(90,512)`, `(90,585)`, `(1,676)`, and `(143,777)`.

This boot assigns handle 20 to `mdss_sde` with two routes, and handle 21 to
`grp3d` with one route. The previous artifact joined handle 20 to `grp3d` and
reported an 18-update path-count mismatch. That pairing is therefore suspect;
because the new capture came from a later boot, it cannot establish the exact
handle assignment during the earlier trace. In this paired capture, all
11,007 request updates matched the same-boot handle and path counts. There were
no unknown handles, route-count mismatches, bus error lines, or failed paths;
four unchanged-index notes had no client handle. The runtime client list had
no WLAN- or Wi-Fi-named bus client.

The 30-second connected baseline had no IPA handle-10 updates. The confirmed
Wi-Fi-off window had eight IPA updates: four `0→1` / `1→0` pairs. During
re-enable and association there were five `0→1` updates and four matching
`1→0` updates; the remaining `1→0` occurred just after the connected marker.
The subsequent connected window contained that final down-transition and no
further IPA changes. The machine-readable report labels these as `MIN`↔`SVS2`
and associates `SVS2` with the source-defined logical 50 MHz rate selector;
it does not measure a GCC clock rate. Across the
trace, 10,678 devfreq samples were captured.
This remains correlation with the shared IPA client's activity, not proof of
WLAN ownership or a WLAN-specific vote. Wi-Fi returned to connected, ADB was
restored to shell mode, and dynamic-debug print flags returned to their
original disabled state. No firmware was transferred, no WLAN service was
started, and no seL4 hardware test or bus vote was performed.

The report, raw trace, marker, runtime inventory, and FDT SHA-256 values are
`a2b19d16c30476868c9630e70443493fcf3f12b9ef0778d114640fdd9cd952e0`,
`464b20d88a40eb338b6634420b62ca2ebbf920a7c256f48a15391c47548b9f00`,
`3f71a013f8541140de06c4d50ab76b6863395e5d22eb276d6cda4072a9b8219f`,
`5daab69bbe9b042049d3cf77c7251e057f8e48dd3b420bf2458e956fd9462ec8`, and
`7fd93ac398779b7dee1c909af355207d5b76cba3d04dc5f97cda259581a50369`,
respectively.

## Fifth LineageOS IPA bus-transition capture (2026-10-23)

The Wi-Fi off/on comparison was repeated on the same LineageOS boot, reusing
the earlier runtime registration snapshot only after the current merged-FDT
hash, Lineage build, kernel release, and continuous uptime all matched. The
kernel ring buffer had already aged out the registration lines; the uptime
delta matched the prior marker's wall-clock age within 0.1 seconds. Artifacts
are in
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261024-042743/`.
The paired snapshot still has 49 handles and 57 resolved routes, with IPA at
handle 10 and the same four endpoints. It is explicitly labeled as a same-boot
cache reuse in the marker and report.

The 30-second connected baseline had no IPA handle-10 updates. The confirmed
Wi-Fi-off window had eight updates, four `MIN→SVS2→MIN` pairs, beginning about
one second after disable confirmation. During the 7.4-second reconnect window,
handle 10 made six `MIN→SVS2` and five `SVS2→MIN` updates; one final down-vote
arrived about 0.14 seconds after Wi-Fi reported connected. The remaining
30-second connected window had only that one transition. `SVS2` is the logical
50 MHz selector for bus usecase 1; no physical clock rate was read. The trace
contains 7,159 request updates and 6,899 devfreq samples, with no unknown
handles, path-count mismatches, bus errors, or failed paths. Four unchanged-
index notes lacked a client handle. This remains shared IPA-client activity
correlated with Wi-Fi transitions, not proof that WLAN owns or caused those
votes; the runtime inventory has no Wi-Fi-named bus client.

Wi-Fi returned to connected, ADB returned to the shell user, and dynamic-debug
flags returned to their disabled state. The temporary capture changed no boot
image, partition, firmware, or modem/WLAN state. The report, trace, marker,
reused registration inventory, and fresh runtime-FDT SHA-256 values are
`f7db6e58f09c9cdc9f0a2a11de4270862d6892fa18707b3865db0a09df149c2a`,
`d92202cefa94909f8b00781f4df229cbc24edd15a7b8c1c2d97ed0615098bfba`,
`9a77d61c894ad4d909b80efbcaf372426eba0b9705964f166e9c060c2c8b18db`,
`5daab69bbe9b042049d3cf77c7251e057f8e48dd3b420bf2458e956fd9462ec8`, and
`7fd93ac398779b7dee1c909af355207d5b76cba3d04dc5f97cda259581a50369`,
respectively.

## Current LineageOS WLAN firmware identity check (2026-10-23)

A read-only filter over the current LineageOS kernel log confirms the installed
stack is downstream QDF/QCACLD, not the candidate ath10k SNOC driver. The
firmware reports build `3226822e`; the host identifies as `5.2.03.9H`, firmware
`3.2.2.9.558.0`, hardware `0x40090000`, and board version 18. QDF also logs
board ID 0 with version tuple `18 0 0 0 0`. These are the stock firmware's QDF
identity fields, not the ath10k QMI board/chip pair used by the API-2 selector.
They cannot select or validate an entry in upstream `board-2.bin`.

The filtered evidence is saved at
`.cache/pixel-sunfish/lineage-wlan-runtime-identity-20261023.txt` with
SHA-256 `26774873194e14fa3cf47ce6f5c80b0e5eee8d5d7b2e1e5ca6c71a1272f02203`.
No firmware payload was read, copied, or retained. The candidate compatibility
test remains gated on an ath10k-capable Linux reference boot plus exact
candidate board-data selection; the current QCACLD board fields do not close
that gate. No candidate firmware was staged or loaded.

## Pure IPA-to-BCM vote composition (2026-10-23)

`rust/src/sunfish_ipa_bus_vote.rs` now has a pure end-to-end planner for one
IPA usecase. It requires a complete generation-matched MSM-bus snapshot,
replaces only the IPA client's report with the selected four-path Sunfish IPA
vector, aggregates all clients by route node, applies the ordered BCM link
scaling rules, and produces BCM requests plus an active/wake/sleep TCS plan.
The planner rejects an incomplete client set, missing or duplicate IPA client,
stale generation, and aggregation/scaling/TCS errors. The synthetic second
client in the composition fixture confirms that the IPA SVS2 vote is combined
with other-client bandwidth before BCM conversion.

The dedicated 87-test harness passes. This closes the pure arithmetic and
ordering composition gap; it does not provide an authoritative complete
seL4 client inventory, runtime route snapshot, or RPMh owner/lease, and the
resulting plan is not submitted to hardware. No device-side bus vote or WLAN
start was attempted; the Pixel remains in LineageOS.

The planner now also converts each bounded active-phase batch into the
existing guarded Apps-RSC TCS0 register plan, validating batch count, length,
and full command coverage before calling the source-matched TCS planner. The
end-to-end fixture uses the observed IPA bus handle 10, composes an SVS2 vote
with a second client, then checks the resulting TCS command address and owner
identity/generation/ticket. This still only builds a plan; it does not acquire
the RSC lease or execute any write.

## Lineage MSM-bus live-state access check (2026-10-23)

The pinned kernel contains read-only `dump_clients` and `dump_bcm_clients`
debugfs readers. The former emits current client route/usecase values through
`bus_client_status`; the latter emits per-BCM client aggregates through
`bus_bcm_client_status` and kernel log records. A targeted LineageOS check
found tracefs mounted but those events disabled, no `/sys/kernel/debug`, and
no available `su` in the shell. Temporary ADB-root access succeeded, but
`mount -t debugfs` returned `No such device`, so the kernel does not provide
the debugfs interface needed for this capture. ADB was returned to the shell
user, and no tracing or Wi-Fi setting was changed. The current image therefore
cannot provide the missing full live client report through this path; an
instrumented/root-capable reference kernel or another source of the full
client vector is still needed before an aggregate bus vote can be authorized.

## Temporary debugfs reference kernel prepared (2026-10-23)

The pinned kernel source at `28f9290ae067` builds for the phone's running
release, `4.14.357-openela-g28f9290ae067`, with `CONFIG_DEBUG_FS=y`. The first
probe image was built from `sunfish_defconfig` using Debian Clang 14, with LTO
and CFI disabled. The build ran in a Linux case-sensitive volume
because the kernel tree contains case-distinct netfilter sources that cannot
coexist in the macOS checkout filesystem. A cache-only debugfs compatibility
overlay leaves GPIO structures and operational drivers untouched: it falls
back to the generic GPIO listing and stubs three unavailable statistics/dump
hooks, which only suppress their diagnostic output. The image uses the
existing boot-v2 ramdisk, DTB, and command line; only the LZ4 kernel payload
changes. The repack verifier confirms the ramdisk and DTB are byte-for-byte
unchanged. It was run with `fastboot boot`; no partition was flashed.

The compiled kernel payload SHA-256 is
`7906deb7a5578068faca7e9ba82be2e284f8153df80e74522e4b3bf8b040cbfd` and the
temporary boot image SHA-256 is
`3fbe99af02c22925a1ad4e8648245b1b0130ce5e16d2b51360189a0f4644cc06`. The
artifacts and build-only overlay are under
`.cache/pixel-sunfish/lineage-debugfs-reference/`. This one-off reference boot
mounted debugfs and read the kernel's current client and BCM snapshots; it did
not submit a bus vote, start WLAN, or change modem or firmware state.

The image reached Android userspace but did not finish booting: at snapshot time
`sys.boot_completed` was empty, boot animation was still running, and the Wi-Fi
service was not registered. The debugfs read produced 46 client-route events
across 38 client names and 76 BCM events. All four IPA routes reported AB=0 and
IB=0. These values are an incomplete-boot observation, not a settled Wi-Fi-on
baseline, so they must not be used to infer the final bus-vote inventory. The
trace controls were restored to their prior disabled state (`tracing_on=0`,
`bus_client_status=0`, and `bus_bcm_client_status=0`). After rebooting to the
installed LineageOS, the phone completed boot (`sys.boot_completed=1`), the
boot animation stopped, and the Wi-Fi service was present and enabled.

A read-only `/proc/config.gz` capture from installed LineageOS shows the
reference kernel has `CONFIG_LTO=y`, `CONFIG_LTO_CLANG=y`, `CONFIG_CFI=y`, and
`CONFIG_CFI_CLANG=y`; `/proc/version` reports Clang 21.0.0 based on Android
Clang r563880. The probe config differs in 37 Kconfig values, including those
LTO/CFI settings and several debug-only options. This build mismatch is a
plausible contributor to the incomplete boot, but does not prove its cause.
The capture is therefore not suitable as a settled reference inventory. The
next attempt should start from the captured device config, enable only
`CONFIG_DEBUG_FS`, and use the matching AOSP Clang r563880 toolchain
([official prebuilt](https://android.googlesource.com/platform/prebuilts/clang/host/linux-x86/+/refs/tags/android-16.0.0_r4/clang-r563880/));
then repeat the bounded temporary boot and snapshot before resuming bus-vote
composition work.

## Exact-config debugfs candidate built (2026-10-24)

The next candidate has now been built from the captured installed-kernel
configuration with `CONFIG_DEBUG_FS=y` and the matching AOSP Clang 21.0.0
r563880 toolchain. `CONFIG_LTO`, `CONFIG_LTO_CLANG`, `CONFIG_CFI`, and
`CONFIG_CFI_CLANG` remain enabled. The 28 raw symbol differences from the
captured config are `DEBUG_FS=y`, its `MSM_IDLE_STATS` dependency,
`LOCALVERSION_AUTO=n`, and 25 symbols that the installed export omitted but
`olddefconfig` materialized as unset. The kernel release is
`4.14.357-openela-g28f9290ae067`.

The first exact-config LTO attempt was killed by the Podman VM's global OOM
killer at about 14.9 GiB linker RSS in its 16 GiB memory configuration. The
incremental build then completed after raising the VM to 20 GiB, growing its
XFS data partition to 24 GiB, and enabling 6 GiB of guest swap. The compressed
kernel payload SHA-256 is
`062e7ef029fcadb8217ba50ddd7a68e8c19839defdbb92c4bcf06c94bd160841`.

The temporary boot-v2 image
`.cache/pixel-sunfish/lineage-debugfs-reference/artifacts/fastboot-debugfs-exact.img`
has SHA-256
`1f497cf7bd5e5d6b19a143443cae7dcdfd62c18c0c4fecd8ac6045119a1454a2`.
Repack verification confirms its kernel is byte-for-byte the built payload;
the reference ramdisk, DTB, command line, and remaining boot-header fields are
unchanged. This image is prepared for `fastboot boot` only; no partition was
flashed. This image was subsequently tested on-device; see the following sections.


## Exact-config candidate tried on-device (2026-10-24)

The exact-config image was temporarily booted with `fastboot boot`; no
partition was written. The phone reached Android userspace and remained
accessible over ADB, but after 305 seconds `sys.boot_completed` was still
empty, the boot animation was still running, and Android's Wi-Fi service was
not registered. Wi-Fi was disabled and disconnected. This is not a settled
Wi-Fi-on reference, so the captured client/BCM vector must not be used as the
operational bus-vote target.

The early debugfs snapshot contained 46 `bus_client_status` events for 38
client names and 87 `bus_bcm_client_status` events for 68 BCM/client pairs.
All four IPA routes were zero AB/IB: `(90,512)`, `(90,585)`, `(1,676)`, and
`(143,777)`. No nonzero IPA BCM pair appeared. The capture is marked
`android-boot-incomplete` and `wifi-not-connected`; trace settings were
restored to `tracing_on=0`, with both bus events disabled.

The exact candidate also produced a large FastRPC error burst: 13,037
`bad ioctl` kernel messages, while the captured logcat contains 37
`apps_dev_init failed`, 74 `Transport endpoint is not connected`, and 12
`fastRPC device driver is disabled` messages. The installed LineageOS boot
capture has three `bad ioctl` messages in dmesg and none of those daemon errors
in its captured logcat. In this kernel source, the rejected command is
`0xc00c5211` (`_IOWR('R', 17, 12)`); the FastRPC ioctl header defines commands
through number 16, so this branch returns `-ENOTTY` for command 17. This
correlation is a lead, not an established explanation for the incomplete
Android boot: the logs do not identify which userspace caller issued it, and
the FastRPC transport errors may be a separate effect of the temporary boot.

The phone was then returned to installed LineageOS and verified healthy at
`sys.boot_completed=1`, with the Android Wi-Fi service present and connected
to the configured network. Keep it in that state while the offline comparison
continues. Before another device boot, trace the command-17 caller and compare
the exact image's framework/driver initialization against the installed boot;
do not treat this early snapshot as bus-vote evidence.


## Debugfs idle-stat isolation on-device (2026-10-24)

A second temporary kernel tested whether the idle-stat code selected by the
exact-config debugfs build was responsible for its incomplete Android boot.
The candidate starts from the same captured LineageOS config and Clang 21
r563880 toolchain, keeps LTO/CFI and `CONFIG_DEBUG_FS=y`, and removes the two
`select MSM_IDLE_STATS if DEBUG_FS` clauses from `MSM_PM` and `MSM_PM_LEGACY`
in an isolated copy of the kernel source. Relative to the exact debugfs config,
the only resulting config differences are `MSM_IDLE_STATS` and its three
bucket constants; the kernel release remains
`4.14.357-openela-g28f9290ae067`.

The compressed kernel payload SHA-256 is
`907220f19666894a2b219ffbc186526dc43c9550c6c19688812b0adfd3bb81b8` and the
repacked temporary image SHA-256 is
`ef5b94487f8c980a3c8011df778db4e131af48de165751ed9145249049684944`. The
repacker confirms that the ramdisk, DTB, command line, and remaining boot
header fields match the exact-config reference image. The candidate was
booted with `fastboot boot` after confirming product `sunfish` and unlocked
state; no partition was flashed.

At 308 seconds the boot animation was still running, `sys.boot_completed` was
empty, and the Android Wi-Fi service was absent. Thus removing the idle-stat
selection did not fix the boot stall. Both debugfs candidates produce the
same FastRPC failure pattern: the minimal candidate logged 13,247 rejected
`0xc00c5211` ioctls, 37 `apps_dev_init failed`, 75 transport-endpoint errors,
and 13 `fastRPC device driver is disabled` messages. These counts closely
match the exact-config candidate and do not establish whether FastRPC errors
cause the boot stall. The shared `DEBUG_FS` change remains a possible factor,
but this comparison does not prove it is causal.

The incomplete-boot snapshot contained 46 client-route events and 76 BCM
client events. All four IPA routes again reported AB=0 and IB=0. The trace
controls were restored to `tracing_on=0`, with both bus events disabled. As
with the first capture, these early values are not a settled Wi-Fi-on baseline
and cannot determine the native bus vote.

The phone was rebooted to installed LineageOS. It returned to
`sys.boot_completed=1`, Wi-Fi enabled and connected to `Vanilla`, with ADB back
as `uid=2000(shell)` and debugfs unmounted. Do not repeat a `DEBUG_FS` image
boot with the same design. The following section records the successful
delayed snapshot using a debugfs-disabled kernel. Use its settled bus data to
continue the model; diagnose the FastRPC boot-stall lead separately.


## Settled MSM-bus snapshot without debugfs (2026-10-24)

A one-shot kernel readout now provides the settled reference that the prior
debugfs-enabled boots could not capture. It was built from the installed
LineageOS kernel config with only two config changes: `CONFIG_LOCALVERSION_AUTO`
was disabled to preserve the installed release string, and
`CONFIG_SUNFISH_MSM_BUS_SNAPSHOT=y` enabled the diagnostic. `CONFIG_DEBUG_FS`
and `CONFIG_MSM_IDLE_STATS` remain unset; LTO and CFI remain enabled. The
readout schedules one delayed work item at late init, waits 180 seconds, then
logs the current MSM-bus usecase clients, direct client handles, and nonzero
BCM aggregates. Its collection path only reads the in-memory client/BCM lists;
it does not submit bus requests or start modem/WLAN, transfer firmware, use
DMA, or touch reset/ownership controls.

The build used the same AOSP Clang 21.0.0 r563880 toolchain and kernel release
`4.14.357-openela-g28f9290ae067`. The compressed kernel payload SHA-256 is
`377333b495d2f6c903c82aede7dfbd99dcdcd59eb0cdcf079ce2a8615b5db734`; the
temporary boot-v2 image SHA-256 is
`459fb1c7ce20a8ce4937f9360c2f5aaa864fd84487426d9340209f935954e497`.
Repack verification confirms the LineageOS ramdisk, DTB, and command line were
preserved. The image was run with `fastboot boot` only; no partition was
flashed, and the temporary source overlay was restored after the build.

The phone reached `sys.boot_completed=1` with Wi-Fi connected to “Vanilla.”
The first settled snapshot, at about 191 seconds uptime, recorded 51 client
rows and 53 BCM rows. A second run used the same image with Wi-Fi disabled
from 115 seconds through the approximately 191-second snapshot; it recorded
51 client rows and 60 BCM rows. Wi-Fi was then re-enabled, reconnected to
“Vanilla,” and the phone returned to installed LineageOS.

All four IPA client paths had `ab=0 ib=0` in both captures: `(90,512)`,
`(90,585)`, `(1,676)`, and `(143,777)`. The `apps_smmu` client was unchanged
at `src=161 dst=627 ab=0 ib=1000000`. Neither capture contained a registered
client name containing `wlan` or `wlfw`. The client rows that differed between
the Wi-Fi-on and Wi-Fi-off snapshots were one QUP0 handle and three CPU
bandwidth/latency clients; the IPA values did not change. This indicates that
the MSM-bus view captured here has no observable IPA or explicitly named WLAN
vote change with Wi-Fi toggled. It does not establish that no WLAN-specific
vote exists through another driver or interface, and it does not reveal the
WLFW/QMI or PDR state.

Raw snapshots and runtime records are saved in
`.cache/pixel-sunfish/lineage-debugfs-reference/artifacts/bus-snapshot/` as
`device-snapshot.log`, `device-snapshot-wifi-off.log`,
`device-runtime.txt`, `device-runtime-wifi-off.txt`, and
`device-runtime-installed-final.txt`; build logs are `build-image.log`,
`build-olddefconfig.log`, and `build-prepare.log`. The installed LineageOS
kernel was running again with Wi-Fi connected when this capture finished.

## LineageOS recovery and service-order confirmation (2026-10-24)

After a temporary diagnostic boot returned a black display, the Pixel restarted
normally into the installed LineageOS image. A read-only ADB check confirmed
`sys.boot_completed=1`, product `sunfish`, shell UID 2000, and Wi-Fi connected
to “Vanilla.” The capture did not change logging, mounts, Wi-Fi, boot state, or
firmware.

The new filtered boot log provides another same-device reference for the AP
service order: `vendor.ipacm`, `vendor.qrtr-ns`, `tftp_server`, `pd_mapper`,
`vendor.rmt_storage`, and `vendor.tftp_server` start before the modem is brought
out of reset. The modem then reaches its power/clock-ready interrupt and QRTR
readiness; remote storage opens and reads the modem NV files; the IPA
`INIT_MODEM_DRIVER` request receives a response; SERVREG reports and
acknowledges `msm/modem/wlan_pd`; ICNSS connects to WLFW; and both ICNSS and WMA
report firmware ready. This confirms the installed Linux service sequence on
this boot. It does not prove that the seL4 owner has production equivalents or
that its previous probe's local stop labels describe the peer's physical
state.

The capture's kernel reports that debugfs is unsupported, so IPA `hw_type` and
`gen_reg` remain unavailable through that interface. No Peripheral Manager
lines were present in the retained log buffers. Filtered service output and
runtime status are saved under
`.cache/pixel-sunfish/lineage-state-capture-20261024/`; the filtered log SHA-256
is `151fe2cdf05b1e9ad11e88714851451c20262ec403e0b9940a510df564787bc1`.
This remains LineageOS reference evidence; the native WLAN path is still
disabled pending production AP service providers, an authorized firmware
source, and proven WLAN DMA/SMMU ownership.

## UFS GCC snapshot and HCI probe correction (2026-10-24)

The read-only GCC snapshot was captured in `IMG_8940.heic`. It reports GDSCR
`0xf8222000`, PHY BCR `0`, command/config pairs AXI `0 / 0x00100105`, UniPro
`0 / 0x00100107`, ICE `0 / 0x00000103`, and PHY auxiliary `0 / 0`; all nine
reported branch gates have enable bit 0 set. Those register values are direct
device evidence, but the original screen's `AXI ROOT DISABLED` classification
was a decoder bug. Pinned Linux `clk_rcg2_is_enabled()` tests `CMD_ROOT_OFF`,
not the `CMD_ROOT_EN` request bit, so `CMD=0` with `ROOT_OFF=0` is active by
Linux's own status predicate. The same source's `clk_rcg2_calc_rate()` treats
HID 0 as the parent rate, making the observed auxiliary config 0 equivalent
to the divide-by-one HID 1 encoding. The decoder and source validator now model
those semantics; all 10 GCC decoder tests pass, and the photographed snapshot
passes the GCC-visible pre-CAP subset. This says nothing about RPMh vote
provenance, PMIC rails, HBA regulator ownership, PHY power, interconnect votes,
or exclusive UFS ownership.

The earlier UFSHCI photo stopped at `SNAPSHOT START`. Source review found that
its HCI mapping lacked `setvar_vaddr` while the probe used
`memory_region_symbol!`, which can panic on a null injected address before
the first CAP read. The HCI probe now displays and validates its injected
virtual address (`0x64000000`) before forming a pointer. It also flushes a
checkpoint before each volatile CAP, VER, and HCS read and replaces that line
with the value after each read, so a stopped screen identifies the first
register access that did not return. The memory-region mapping stays read-only
and the image has no IRQ, SMC, DMA, SMMU, power, or reset authority. Its system
validator enforces the symbol binding, check-before-read ordering, and
checkpoint/read/value order. The AArch64 image built with 48 CapDL pages, zero
overlaps, and passed the authority/source checks. Image SHA-256 is
`e2cd6545d0f1627f18edf8d372cfb09331030e584e65952ed2f3dbcf1c10fcea`; it has
not been booted yet. The remaining device check is a temporary `fastboot boot`
of this read-only CAP/VER/HCS probe; no partition flash or UFS command is
involved.

## Source-checked Sunfish UFS GCC window (2026-10-24)

An offline cross-check against the saved Google SM7150 device tree and the
pinned Lineage kernel confirms the address derivation used by the read-only GCC
probe. Sunfish's GCC window starts at `0x00100000`; the UFS HBA is at
`0x01d84000` for `0x3000` bytes, the UFS PHY at `0x01d87000` for `0x1000`,
and the HBA uses interrupt SPI 265 and SMMU SID `0x300`. The GCC source places
the UFS PHY block reset at offset `0x77000` and its GDSCR at `0x77004`, so the
probe's local read-only page at `0x00177000` contains those registers and the
UFS clock roots/gates.

The pinned `gcc-sdmmagpie.c` places the AXI, UniPro, ICE, and PHY auxiliary
clock-root command/config pairs at `0x77020/0x77024`, `0x77060/0x77064`,
`0x77048/0x7704c`, and `0x77098/0x7709c`. Their gate registers are at
`0x77014` (AHB), `0x77018` (TX symbol), `0x7701c` (RX symbol), `0x77038`
(AXI), `0x7708c` (UniPro), `0x77090` (ICE), and `0x77094` (PHY auxiliary).
The memory-reference gate is at `0x8c000`, matching the separate probe page
at `0x0018c000`. These are source-derived addresses, not a report of current
hardware register values; the probe still has to show its mapping checkpoints
and complete the reads before an observed state can be recorded.

The device-tree clock profile requests maxima of 200 MHz AXI, 150 MHz UniPro,
and 300 MHz ICE, with the related GCC roots voting the shared `vdd_cx` rail.
This confirms that the existing offline pre-CAP profile follows the right
source configuration; it does not establish power-domain, RPMh, interconnect,
PHY, UFS DMA, or media readiness. No hardware write or new device test was
performed for this source cross-check.

## UFS pre-CAP reference-clock rail vote (2026-10-24)

The source audit found one missing logical regulator consumer in the offline
UFS resource mask. Sunfish's runtime DT supplies `qcom,vddp-ref-clk` through
the same PMIC rail phandle as `vdda-pll`, while the pinned PHY source obtains
and enables `vddp-ref-clk` as a separate regulator client after enabling the
PHY interface and reference clocks. `qcom_ufs_platform.rs` now requires that
consumer vote independently; the physical rail remains shared. The source
validator pins the shared phandle, current-load and voltage-bound DT
properties, and the PHY power-on order through this vote. Its host-platform
test rejects a resource map that omits the reference-pad vote. This is an
offline source/contract correction only; no regulator was enabled on-device.

The same runtime DT resolves the five UFS regulator clients to RPMh VRM
resource keys `ldoa19` (`vcc`), `ldoa12` (`vccq2`), `ldoa4` (`vdda-phy`), and
`ldoc3` (both `vdda-pll` and `vddp-ref-clk`). The bounded command-database map
allows only that final pair of client aliases; it rejects missing resources,
non-VRM addresses, and unexpected duplicate command addresses. The source
validator now cross-checks each consumer phandle against its parent
`qcom,resource-name`. This identifies command IDs from a copied database only;
it does not publish RPMh votes or establish command-database provenance.

The UFS VRM profile records each driver's pre-CAP request: VCC at 2.95 V /
600 mA, VCCQ2 at 1.8 V / 600 mA, PHY at 0.8 V / 62.9 mA, PLL at 1.2 V /
18.3 mA, and the reference pad at 1.2 V / 100 µA. The pinned VRM driver rounds
the requested minimum voltage up to millivolts and selects the highest
supported mode whose threshold is no greater than the load. For all five UFS
loads, the Sunfish `<0, 1>` thresholds select PMIC5 LDO HPM mode 7. Each
regulator resource uses `qcom,set = 3`, so this consumer contributes the same
fields to active and sleep sets. Tests exercise that mapping, and the source
validator checks the runtime DT values, PHY voltage limits, set/mode
properties, and pinned RPMh mode/rounding logic. These are per-consumer request
contributions only, not a complete rail client ledger or permission to issue
TCS commands.

The pinned runtime DT also shows these physical rails have other consumers,
so a UFS-only profile cannot seed a complete RPMh broker:

| RPMh resource | Other runtime-DT consumers |
| --- | --- |
| `ldoa19` | SDHC `vdd-supply` |
| `ldoa12` | SDHC `vdd-io-supply` |
| `ldoa4` | DSI PHY0/PHY1 `vdda-0p9-supply`, QUSB PHY `vdd-supply`, USB QMP PHY `vdd-supply` |
| `ldoc3` | DSI0/DSI1 `vdda-1p2-supply`, camera CSI PHY0–3 `mipi-csi-vdd-supply`, USB QMP PHY `core-supply` |

DisplayPort also references `ldoa4` and `ldoc3`, but its node is disabled in
this runtime DT. These references establish sharing, not that each consumer
is currently enabled or its vote values. Before constructing a complete rail
ledger, audit each enabled consumer's voltage/load/mode requests, active and
sleep set selection, enable/disable ordering, and cache state in the pinned
drivers. This is an offline source-audit hold point; it does not authorize any
RPMh command or hardware rail change.

### Bluetooth contributions to WLAN-shared rails (offline)

The captured runtime and generated kernel DTBs both resolve the WCN3990
Bluetooth client's crystal, core, and PA supplies to the WLAN-shared RPMh
resources `ldoc1`, `ldoc2`, and `ldoc10`. The pinned
`drivers/bluetooth/bluetooth-power.c` requests 1.7–1.9 V, 1.245–1.35 V, and
3.2–3.4 V respectively, with a 1 µA load request for each. When enabling, the
driver sets each configured voltage range, sets its load, and then enables the
regulator. On disable it disables the regulator and clears that client's
minimum-voltage and load requests. The resource validator now checks these
values against both captured DTBs and pins the source file hash.

`qcom_rpmh_vrm.rs` models the BT side at the regulator-consumer API level,
distinguishing never-started, enabled, and stopped requests. Its pure
`combine_sunfish_shared_regulator_request` reducer combines the known ICNSS
and BT voltage ranges, enable states, and load requests into one regulator
device vote. This matches the Linux ordering: the regulator core intersects
voltage constraints and sums loads within a regulator device before the RPMh
provider selects a mode and max-aggregates fields only among regulator devices
associated with the same RPMh resource. The reducer preserves a preexisting
mode only when neither known consumer changes state or load. It recalculates
DRMS on an enable transition even if the enabling consumer did not request a
load, using the explicit system-load input.

The resource validator also confirms the complete Device Tree grouping for
these three resource names: each captured DTB has exactly one
`qcom,rpmh-vrm-regulator` device for `ldoc1`, `ldoc2`, and `ldoc10`, at the
manifested provider path. This remains static DT evidence; runtime consumer
state, external clients, and the live Apps RSC cache are still unknown.

These are Bluetooth client contributions, not evidence that Bluetooth was
enabled during capture or that those rails carried these final values. The
complete runtime consumer set, system-load value, Apps RSC cache, and any
non-DT clients remain unverified. The reducer is an offline model, not a
complete ledger or an authority source, and does not make the current
WLAN-only RPMh broker safe to construct or write.

The board-specific HPM thresholds for these three resources are pinned to
`arch/arm64/boot/dts/qcom/sdmmagpie-regulator.dtsi` at the audited Lineage
commit. Its 10,000 µA thresholds match both captured DTBs; the generic Atoll
regulator DTS has a different value and must not be used for this Sunfish mode
calculation. The resource validator checks the source hash and each resource's
threshold definition.

## Working Sunfish reference clarifies the modem startup order (2026-10-24)

The pinned Mobian Pixel 4a port at
[`789679f`](https://github.com/miromraz/mobian-pixel4a-port/tree/789679f25f859bd741d690fe221d1cc68a23444e)
adds the modprobe soft dependency
`qcom_q6v5_pas pre: ipa qcom_pd_mapper`. Its patch rationale says the modem
firmware expects the AP IPA QMI service before MPSS boot, and that the modem
registers protection domains with the locator during boot. This is a
working-port report, not a Qualcomm protocol guarantee. The same port's Wi-Fi
unit waits for `rmtfs`, `pd-mapper`, `tqftpserv`, and `qrtr-ns` before running
its WLFW/ath10k bring-up script.

The paired pinned [Sunfish mainline kernel
source](https://github.com/miromraz/linux/blob/2267e248204e7e051e0f7355298949a6440de579/drivers/soc/qcom/qcom_pd_mapper.c)
maps `msm/modem/wlan_pd`, instance 180, to `kernel/elf_loader` and `wlan/fw`,
and includes that mapping in the SM7150 domain table. This narrows the earlier
“initial `wlan_pd` launch trigger” gap. The AP locator provides the domain
mapping, and the reference port reports modem-side registration during MPSS
boot. Together these suggest (but do not prove) that initial registration is
modem-initiated after MPSS starts; no audited source documents a host
`RESTART_PD` command as the initial start mechanism. The exact private
modem-side launch mechanism remains undocumented.

For NexatrodeOS this gives a concrete candidate order to implement and verify:
bring up the production IPA QMI server and locator before MPSS power-on, with
transport and storage services ready for boot-time requests. The current
`NoServices` owner does not provide these prerequisites; the existing
SERVREG/WLFW state probes therefore cannot establish a valid initial startup
path by themselves. This reference does not resolve the separate firmware
authorization, IPA hardware backend, or exclusive WLAN DMA/SMMU ownership
gates.

The original opt-in `IpaServerServices` path was too late for that candidate
order: `attach_with_services()` consumes a completed GLINK channel checkpoint,
and the adapter previously created its IPA endpoint only after attach. The
transport core now has an explicit prepared-session handoff:
`IpaServerServices::prepare_before_mpss()` stages the IPA and SERVREG endpoints
and their local readiness in a pristine bounded `Session`. Its higher-level
`prepare_from_post_init()` entry derives the AP-server and modem-client permits
from the same completed IPA `Ready` value and composes the existing
`INIT_MODEM_DRIVER` client. The resulting
`PreparedIpaServerServices` must be passed to
`attach_with_prepared_services()` after GLINK negotiation. The same session is
then transferred intact to the owner, which avoids re-registering those
endpoints. Native tests verify endpoint identity, readiness, generation
binding, attachment, and that neither endpoint is announced before peer HELLO.
Tests also confirm that the modem-side client is reserved before QRTR control
starts and that permits derived from one post-init result share its generation.

This is still an in-memory sequencing primitive, not a completed startup path.
`PreparedModemBoot` now provides a typed handoff that stages services from a
trusted IPA `Ready` value before MPSS and consumes them only after a matching
SMP2P/GLINK checkpoint. The checkpoint and prepared session carry the same
generation through native attachment, and a mismatch is rejected before the
owner writes shared memory. No platform startup runner invokes this handoff;
production AP IPA and SERVREG providers and a storage/heap backend remain
absent. The coordinator starts no hardware and has not been exercised on the
phone. Firmware authorization, real MPSS startup, and exclusive WLAN DMA/SMMU
ownership remain separate gates.

## Pre-MPSS router reserves modem storage and heap endpoints (2026-10-24)

The prepared QRTR session reserves the remote-storage and heap-memory endpoint
addresses alongside SERVREG and AP IPA before MPSS. The default preparation
path leaves storage and heap unavailable until their retained handlers
activate after GLINK attachment. An opt-in `prepare_before_mpss_with_memory()`
path now requires the backend to validate its live storage/heap resources before
staging those two services as ready, and `attach_with_prepared_services()`
revalidates that backend before consuming the session. This makes the
boot-time-service ordering expressible without letting `NoServices` or a stale
backend advertise memory services. The existing default path is unchanged.
`attach_with_prepared_services()` recognizes the validated pre-registered
endpoints and avoids registering them a second time. Native owner tests check
the default and memory-ready service masks, rejection without a ready backend,
and rejection if readiness is lost before attachment.

The 277-test owner-native suite and `rustfmt --check` pass. The offline
modem-owner image has now been rebuilt after these changes. The final CapDL
audit passes for owner permissions, aliases, IRQ routes, and continuation
capability; the pool audit reports 1,799 reserved pages, six unmapped guard
pages, 20,049 checked allocations, and all 512 WLAN-MSA pages reserved but
unmapped. Loader and copy targets are disjoint. This confirms the compiled
image layout, not a working boot coordinator: production AP IPA/SERVREG
providers and a storage/heap backend are still absent. The Pixel is restarting
after the last temporary boot and ADB/Fastboot currently enumerate no device,
so no new device check was attempted.

## SMMU handoff probe reports read progress (2026-10-24)

The read-only Apps SMMU handoff probe now paints a framebuffer checkpoint
before its first register read, then marks completion of the global-register,
stream-route, and context-bank scans. This makes a black or frozen display
locate whether the temporary task started and which bounded read phase it
reached. The only writable mapping remains the splash framebuffer; the SMMU
mapping is still read-only, and the probe has no SMC/reset-PD capability.

The rebuilt temporary image has SHA-256
`6b7964b9d2a98f6cf2d19989b87d501f220dbe4a9abff38ed80eaa67b029c2b7`.
All 20 SMMU handoff tests pass and the CapDL build reports 313 root objects
with a 2.71 MiB loader. The candidate has not yet been booted; ADB and Fastboot
currently enumerate no device. Even a completed snapshot would describe only
the ABL handoff and cannot establish free context-bank ownership or the safe
runtime ACTLR value needed before any native SMMU programming.

## WCN3990 firmware inventory freshness check (2026-10-24)

The current upstream `linux-firmware` `main` resolves to
[`bf992df7`](https://gitlab.com/kernel-firmware/linux-firmware/-/commit/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0),
committed 2026-10-23. Its [`WHENCE`](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0/WHENCE)
still lists the generic WCN3990 bundle, a Shikra-specific bundle, and the
QCM2290 bundle; it does not identify a Sunfish-specific WCN3990 firmware path.
The current Mobian Sunfish reference still reports that Linux-firmware lacks
the `google_sunfish` board-data variant and uses the separate SM7150 community
board file plus the handset's vendor `wlanmdsp.mbn` instead. I downloaded the
current generic WCN3990 `board-2.bin`, `firmware-5.bin`, `wlanmdsp.mbn`, and
notice into the ignored `.cache/wcn3990-linux-firmware-audit` directory to
check the actual bytes. The 893,528-byte board file has SHA-256
`c49d2f1894de6edc0ab0c8c9a17ca0328703747c49b6e9428f1de501c2c02c1d`; its
printable selector records include other Google variants but no `sunfish` or
`google_sunfish` string. The 60-byte `firmware-5.bin` is feature metadata with
the non-BMI flag, so it does not contain the executable WLAN image and expects
out-of-band startup. The generic `wlanmdsp.mbn` is 3,725,044 bytes with SHA-256
`92e1501254e6de78c0f2e2cf091507d488b608d07e53acd14813a82744823ec2`; the
phone-observed vendor image is 3,751,004 bytes with SHA-256
`a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e`. This
binary inspection confirms a board-data gap and a different DSP image; it is
not a Sunfish runtime test.

`WHENCE` marks the base WCN3990 files redistributable under
[`LICENSE.QualcommAtheros_ath10k`](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0/LICENSES/LICENSE.QualcommAtheros_ath10k);
retain the accompanying notice and keep the binaries unmodified. That license
does not establish a Sunfish board match or a Sunfish out-of-band loader. The
generic set therefore remains a no-go for a Sunfish hardware test. The audit
copies stay in the ignored cache; no firmware was staged into a boot image or
sent to the phone. The separate read-only SMMU handoff snapshot remains the
next useful device check once the Pixel is responsive and Fastboot enumerates.

### Current upstream candidate recheck and Linux test gate (2026-10-25)

`git ls-remote` still resolves `linux-firmware` `main` to
[`bf992df7`](https://gitlab.com/kernel-firmware/linux-firmware/-/commit/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0).
At that exact head, the official [`WHENCE`](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0/WHENCE)
continues to list generic WCN3990, Shikra, and QCM2290 variants, with no
Sunfish-specific firmware path. I streamed the relevant candidate files in
memory and rechecked their size and SHA-256: generic `wlanmdsp.mbn` is
3,725,044 bytes (`92e15012…`), Shikra is 3,808,072 bytes (`ea178753…`),
and QCM2290 is 3,828,228 bytes (`4a0f4d8a…`). The handset-observed Sunfish
image is 3,751,004 bytes (`a2ee6c86…`), so none is an exact match. The current
parent `board-2.bin` is still 893,528 bytes (`c49d2f18…`); a raw selector scan
finds neither `sunfish` nor `google_sunfish`. The candidate hashes and scan are
recorded in [`wlan-resources.json`](../platforms/sunfish/wlan-resources.json).
No payload was saved, decoded, staged, or sent to the phone.

The separate Linux compatibility test is therefore **not ready to run**. Its
entry gate is an explicitly Sunfish/SM7150-matched `wlanmdsp.mbn`, a matching
board-data selector, a source-backed Linux out-of-band loader/WLFW sequence,
and a reviewed license scope for that test. Once those gates pass, first test
with the established Linux `ath10k_snoc` path and capture firmware selection,
WLFW readiness, PHY registration, and a scan result; do not carry a merely
family-compatible blob into the seL4 image. The current generic, Shikra, and
QCM2290 candidates all fail the Sunfish identity gate, so no hardware test is
planned with them.

## Fresh-context WLAN ACTLR policy (2026-10-24)

The pinned QSMMUv500 source has no `qcom,actlr` match for WLAN SIDs `0x240` and
`0x241`. For a newly zero-allocated software context-bank record, the unmatched
`qsmmuv500_init_cb()` path leaves `cb->actlr` at zero and
`arm_smmu_write_context_bank()` writes that value to `CB_ACTLR`. The pure WLAN
CE and HTT SMMU planners now expose explicitly named helpers for that fresh,
unmatched policy, and the host test verifies they plan an ACTLR write of zero.

This does not establish the phone's live ACTLR value or prove an inherited
context safe: Sunfish sets `qcom,skip-init`, context destruction does not clear
the software ACTLR field, and the ICNSS Linux route is S1 bypass. The helpers
only prepare a plan; SMMU power and exclusive ownership, a complete live route
snapshot, an unused context and slot, owned data/table pages, hardware
readback, and a bound fault handler remain required before any installation.

## Owner build and full host/core validation rerun (2026-10-24)

After adding the fresh-unmatched-context ACTLR planner helpers, the offline
modem-owner image rebuilt successfully. The CapDL owner-mapping and reserved-
pool audits passed, reporting 1,799 reserved pages, six unmapped guard pages,
and all 512 WLAN-MSA pages reserved but unmapped. The full
`platforms/sunfish/test-modem-qmi.py` run also passed: host routing/GLINK/FIFO
tests, the AArch64 `no_std` core and native code-generation checks, plus the
host UFS-storage integration tests. These checks validate compiled behavior
and resource layout only; they do not establish runtime IPA/SERVREG providers,
firmware authorization, exclusive WLAN DMA/SMMU ownership, or native Wi-Fi.
During the Pixel's restart, neither ADB nor Fastboot enumerated a device, so
the pending read-only SMMU handoff snapshot remains unrun.

## Pre-MPSS TFTP readiness is now expressible (2026-10-24)

The prepared native QRTR session now has a strict opt-in path that stages the
TFTP server endpoint alongside IPA, SERVREG, storage, and heap before MPSS.
That path requires the retained storage backend to pass its existing live
readiness check and an already-authorized WLAN image source to remain available
with its originally authorized length. The prepared session verifies the exact
TFTP port and revalidates image availability before attach; activation checks
the attached owner's exact endpoint state before deciding whether registration
is needed. Observer-only and existing post-attach profiles keep their prior
behavior. Tests cover source-length drift, both readiness gates, endpoint
identity, and staging through the real `TftpServerServices` adapter.

The offline owner build and `platforms/sunfish/test-modem-qmi.py` pass after
this change, including the targeted strict-path test, TFTP tests, native owner
suite, AArch64 `no_std` build, and host UFS integration. The current
`AuthorizedWlanImage` still has no production constructor, so this is a safe
ordering capability rather than a usable firmware path until a licensed,
verified Sunfish image provider and live storage/heap backend exist. The latest
owner build reports 20,047 checked allocations, 1,799 reserved pages, six
unmapped guard pages, and all 512 WLAN-MSA pages reserved but unmapped. The
Pixel is restarting; ADB and Fastboot currently enumerate no device.

## Vendor WLAN source has bounded identity verification (2026-10-24)

`SunfishVendorWlanSource` can hash the exact `/vendor/firmware/wlanmdsp.mbn`
contents through the existing read-only LP/ext4 source using a fixed 4 KiB
scratch buffer. `SunfishVendorBoardSource` now does the same for the exact
`/vendor/firmware/bdwlan-sunfish.bin` path. Their phone-observed identity
helpers bind to the recorded lengths and SHA-256 values, and the resource
validator keeps both sets of Rust constants synchronized with the device
inventory. The generic verifier also supports other trusted expected
identities. The verified wrappers retain their sources and recheck live length
for later reads. Host tests verify known digests over both composed
`super` → vendor ext4 → file paths, including metadata-only open, and reject
length and digest mismatches. The observed MBN SHA-256 is
`a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e`; the
observed board-data SHA-256 is
`b5aba8c34db97f25a8d8e9f9361b5b5365e74b58e0f4addc7b922bfe7226b188`.

The wrapper establishes byte identity only. It neither authenticates the
partition origin nor creates `AuthorizedWlanImage`; firmware-use authorization,
a production UFS block provider, and trusted startup/provider wiring are still
required before the bytes can be served or WLAN can start. Google says its
driver binaries are for personal devices and points to each package's terms
([Google driver-binaries page](https://developers.google.com/android/drivers/)).
I streamed the checksum-matched Sunfish S1 archive and read its first member,
`extract-google_devices-sunfish.sh`, without extracting any firmware payload
([official archive](https://dl.google.com/dl/android/aosp/google_devices-sunfish-tq3a.230805.001.s1-2a7bf157.tgz)).
Sections 2(a) and 3(b) restrict the grant to non-commercial use on the named
Android-enabled device and bar use outside that purpose, including
incorporation or compilation into software or devices made by the recipient or
its agents. The handset's WLAN payloads were not individually compared with
that archive, so these terms do not establish which license applies to the
observed files or permission to use them in NexatrodeOS. The community rehost
byte matches also carry no declared binary license. No blob is bundled or
staged; firmware remains disabled until the exact binaries and seL4 use are
authorized. The full
`rustc --edition=2024 -D warnings --test tests/sunfish_lpd_test.rs` passes all
11 active tests (one live-partition test remains intentionally ignored), the
WLAN manifest identity check passes for both files, and the full
`platforms/sunfish/test-modem-qmi.py` host/AArch64 suite passes. The
`platforms/sunfish/build-modem-owner-offline.sh` owner image also builds and
passes its reservation/placement audit. These are offline checks only. The
Pixel was restarting during the final USB check; ADB and Fastboot did not
enumerate, so no duplicate comparison was run during this restart. The
documented LineageOS Wi-Fi on/off comparison series above already provides
that reference evidence; the rebuilt read-only SMMU handoff snapshot remains
unrun.

## Sunfish runtime-system integration remains open (2026-10-24)

The repository-root `system.system` is the QEMU `virt` system. The Pixel-specific
`platforms/sunfish/slint-display.system` is a separate display/touch composition
with Notes services; it does not include the modem owner, QRTR/IPA services,
storage provider, WLAN host, or a network service. `platforms/sunfish/modem-owner.system`
is built as a standalone owner probe and is not composed into that display
system. Therefore the passing protocol tests, native owner image, and read-only
hardware probes are not yet a Sunfish product image capable of reaching the
existing network clients. After the device/SMMU and firmware-authorization
gates, system integration must compose the production storage and IPA/SERVREG
providers, pre-MPSS modem owner, WLAN host, and network service in the Sunfish
system and validate that actual Microkit image. The existing `tests/net`
example is configured for `qemu_virt_aarch64` and uses sDDF's virtual Ethernet
driver, so it supplies neither Sunfish board support nor a WLAN packet/control
interface. Sunfish needs a device-specific WLAN backend and Wi-Fi link/control
service before the current lwIP and network clients can carry traffic.

## Bounded 802.11 receive classification (2026-10-24)

The pinned Linux `ath10k` source applies Native-WiFi header reconstruction to
data MSDUs and retains a separate HTT RAW decapsulation handler. The exact
WCN3990 frame-class distribution across those modes is not yet measured.
`rust/src/ath10k_nwifi_rx.rs` now also exposes a borrowed,
bounds-checked frame view that classifies management, control, data, and
extension frames and returns the body after the Linux-compatible header length.
It covers four-address and QoS data headers, optional HT control, 10/16-byte
control headers, and extension headers without allocating or modifying packet
bytes. Beacon and probe-response parsing now returns borrowed timestamp,
beacon interval, capability, and IE fields; the complete ID/length/value stream
is validated before any IE view is returned. The validator checks both the
pinned Linux header-length function and management/IE structures, while host
tests cover every frame class, beacon/probe response fields, malformed IEs,
and truncation. The focused codec and full Sunfish host/AArch64 regressions
pass with these additions.

`rust/src/ieee80211_probe.rs` adds a bounded Probe Request encoder for wildcard
or specific SSIDs, caller-provided rates, a unicast station address, and a
12-bit sequence number. Tests verify the complete management header and IE
bytes, bounds, and unchanged output on failure. The source validator confirms
management frames select ath10k's management TX mode, share its Native-WiFi
header transform and sequence-number assignment, and follow the pinned
conditional transport policy: WMI management TX when supported, otherwise
HTT for target version 3 or newer, and the legacy HTT management path for
older targets. Native-WiFi data frames use HTT. This source-policy check does
not claim the new encoder is wired into mac80211, scan scheduling, or either
transport.

This only makes raw frames safe to classify at a future link-control boundary;
the encoder does not schedule off-channel TX, control the radio, collect scan
results, authenticate, associate, manage keys, implement RX metadata policy,
or deliver frames to a supplicant. `parse_bss_advertisement` now extracts a
borrowed BSSID, SSID (including hidden/absent distinction), supported and
extended rates, DS/HT primary channel, and raw RSN IE from a beacon or probe
response. It rejects invalid BSSIDs, malformed or duplicate recognized IEs,
invalid rates, and conflicting advertised channels while leaving unknown IEs
and security interpretation to higher layers. The validator pins the IE IDs,
Linux SSID bound, and packed HT-operation layout. This is still host-only: no
scan scheduling, channel metadata/RSSI handoff, authentication, association,
key management, or supplicant transport is implemented, and the Pixel is
restarting.

## Bounded WMI 10.4 scan-command body (2026-10-24)

`rust/src/ath10k_scan.rs` now serializes a bounded `WMI_10_4_START_SCAN_CMDID`
body using the pinned Linux ath10k WMI 10.4 ABI. It writes the 15 common scan
words, host ID prefixes, zero burst duration, and optional channel, SSID,
BSSID, and extra-IE lists in upstream order. Capacities match the source:
64 channels, 16 SSIDs of at most 32 bytes, four BSSIDs, and 256 extra-IE
bytes. It rejects malformed IE streams and invalid bounds before modifying the
output. The host tests check the fixed header, every optional list and its
padding, wildcard SSID encoding, and failure atomicity. The Linux-source
validator checks these layouts against pinned ath10k `wmi.c`/`wmi.h`.

This encoder only creates a command body. The reference profile uses the
source-defined scan/requestor IDs, and `ScanSession` reserves one outstanding
request. `queue_start_scan` now requires a WMI_CONTROL-bound software queue,
reserves the scan slot, encodes the body, and composes it through the HTC
credit/sequence and CE-ring model. It releases the slot when transport
preflight or descriptor validation fails. Legal-channel selection, live vdev
state, hardware CE submission, scan-result collection, and actual radio
operation remain unimplemented. No scan was sent to the Pixel.

`StartScanRequest::linux_reference_defaults` now provides a source-pinned
profile for the common ath10k mac80211 scan path: scan ID 0, requestor 1, low
priority, 50/150 ms active/passive dwell, 50/500 ms rest, 20 s maximum scan,
5 ms probe delay, the default notification mask, and one broadcast BSSID.
Empty SSID lists add the Linux passive-scan flag. Callers still supply channel
frequencies filtered by their own regulatory policy; this constructor neither
chooses channels nor initiates scanning. Host tests cover both active and
passive profile selection, and the Linux-source validator pins the defaults
to `ath10k_wmi_start_scan_init` and `ath10k_hw_scan`.

`ScanSession` now tracks one reserved scan through starting, running, aborting,
and idle phases. It applies start, failure, completion, BSS-channel, and
foreign-channel events only when requestor ID, scan ID, and vdev ID all match
the outstanding request; other events leave state unchanged. A failed local
command submission releases the reservation and returns to idle. This identity
check is intentionally stricter than Linux ath10k, which logs the IDs but
drives its single global scan state without matching them. The session tracks
host state only. The queue adapter serializes into software transport state;
the caller still has to publish a hardware descriptor, wait for transport
completion, enforce timeouts, and publish scan results. Tests cover overlapping
reservation rejection, stale-event rejection, WMI_CONTROL binding, insufficient
credits, CE descriptor rejection, start failure, abort completion, and repeat
completion. No scan was sent to the Pixel.

`rust/src/ath10k_wmi_tx.rs` now wraps a WMI command/body in the upstream
ath10k eight-byte HTC header, including the per-endpoint sequence number,
credit reservation, and `NEED_CREDIT_UPDATE` rule. It deliberately remains
separate from `qcom_htc.rs`, whose WMI transmitter models the downstream
QCACLD path. The bounded transfer composer now takes a WMI command through the
ath10k HTC frame and WCN3990 CE source descriptor, with descriptor validation
before frame output changes. The host test checks both serialized outputs,
while the source validator pins the HTC and CE layouts. This still uses a
caller-supplied IOVA and credit/sequence snapshot: it does not reserve a live
hardware endpoint credit or sequence, map or authorize DMA, publish a CE ring
slot, ring hardware, or transmit to the radio. The separate software queue
object can model credit/sequence and ring-slot reservations but grants no
hardware authority.

The same step now decodes the fixed 24-byte WMI scan-event prefix while
preserving unknown event values, and `rust/src/ath10k_mgmt_rx.rs` parses the
28-byte WMI 10.4 management-RX header into borrowed frame bytes, channel/SNR,
raw chain RSSI, rate, status, and the optional aligned MAC timestamp. These
decoders reject truncated headers/frames and malformed optional timestamp
tails; the source validator pins their offsets and event selection. The
`ScanSession` added above correlates decoded events with one reserved request.

[`rust/src/ieee80211_scan_results.rs`](../rust/src/ieee80211_scan_results.rs)
now connects that decoded WMI RX event to the BSS selector. It follows the
pinned ath10k handler by dropping events during CAC or with CRC, decrypt, or
key-cache errors, mapping channel numbers from the advertised 2.4/5 GHz
ranges (up to the pinned channel 173), and computing signal as SNR plus the
source's `-95 dBm` noise floor.
Beacon/probe-response frames must also match the WMI channel, exact SSID,
caller-approved channels, and WPA2-PSK/CCMP profile. A caller-provided scratch
buffer retains only the strongest valid result, so memory use stays bounded
without allocating a scan table. `ScanWorkflow` now binds the queue reservation,
matching start/terminal events, management-RX accumulation, and successful
completion into one state machine. It ignores RX outside the active request and
exposes a selected BSS only after non-aborted completion. Tests cover the
end-to-end software workflow as well as status filtering, signal mapping,
channel consistency, network matching, strongest-result replacement, and
buffer bounds. The selected result still needs publication to the network/link
owner, and the WMI command has not been sent to live hardware.

The no-radio Microkit protocol probe now also constructs a synthetic passive
scan request and composes its WMI 10.4 body into the caller-buffer HTC frame and
WCN3990 CE descriptor. Its target check validates endpoint, credit/sequence
fields, command bytes, IOVA, transfer length, and descriptor metadata. It also
reserves one synthetic HTC credit and software CE-ring context, then verifies
that simulated CE completion reclaims the context without returning an HTC
credit. It does not map DMA, publish a descriptor, or access WLAN hardware.
The probe manifest remains limited to the reserved splash framebuffer. The
temporary `fastboot boot` image rebuilt on 2026-10-24 has 64 root objects
and is 18,669,568 bytes with SHA-256
`66c711c3b6f41cfe1801f120af316668995a981d015c8737ce83d153ada16986`. The
latest Pixel photo still shows the earlier image's IPA codec failure; the
corrected image has not yet been retested. This target execution check remains
outstanding, and even a pass would validate only the software model, not DMA,
firmware startup, or WLAN operation.

`WmiCommandTxQueue` adds the next host-only state transition: it consumes one
endpoint credit, assigns the wrapping HTC sequence number, encodes the CE
descriptor, and reserves a software CE TX slot. Its ordering follows pinned
`ath10k_htc_send()`: failures before credit consumption preserve sequence
state; failures after HTC header preparation keep the advanced sequence while
leaving credits available for retry; CE completion reclaims only the transfer
token, while credits return only from a validated HTC credit report. Host tests
cover successful commit, sequence wrap, insufficient credit, invalid IOVA,
full-ring rollback, and separation of CE completion from credit return. The
no-radio probe executes the successful queue path with synthetic inputs. The
model still does not map DMA or publish a hardware descriptor/doorbell, and it
is not connected to a radio-facing runtime.

## Sunfish QCACLD receive-buffer ownership

The native-target receive path now has a fixed-capacity ownership model in
[`rust/src/sunfish_qcacld_rx_session.rs`](../rust/src/sunfish_qcacld_rx_session.rs).
It limits a power-of-two ring to its one-empty-slot capacity, claims
completed buffers in FIFO order, tags each lease with a generation, and copies
a supported single-buffer MSDU into caller-owned storage before the caller
explicitly releases the lease. A buffer cannot be reposted while a lease is
live, duplicate live IDs are rejected, and stale leases cannot release a slot
after reuse. Capacity failure leaves the output untouched; parser and buffer
identity failures retain ownership so the caller can retry or deliberately
drop the packet.

[`validate-sunfish-qcacld-rx.py`](../platforms/sunfish/validate-sunfish-qcacld-rx.py)
now checks the model against the pinned QCACLD `htt_rx_netbuf_pop()` cursor and
fill-count updates, plus the source ordering that delays ring refill until
upper-layer packet release. This closes the software buffer-lifetime gap
between the source-matched descriptor decoder and a future packet endpoint.
It still does not implement RX indication decoding, A-MSDU/chained-buffer
assembly, DMA/cache synchronization, firmware descriptor policy, packet
filtering, or link control. The separate queue-copy adapter below covers only
the Ethernet buffer handoff; it grants no device authority and does not make
the radio path operational.

## Copy bridge to sDDF queues

[`platforms/sunfish/sunfish_wlan_net_bridge.c`](../platforms/sunfish/sunfish_wlan_net_bridge.c)
now copies a caller-provided Ethernet frame into a bounded sDDF RX free buffer
and publishes it on the active queue. Its TX direction copies one queued sDDF
frame into caller-owned WLAN staging storage before returning the shared buffer
to the network stack. The adapter validates descriptor offsets, buffer counts,
byte bounds, frame sizes, and queue backpressure; it uses the pinned sDDF
notification protocol and has host tests against the pinned headers. The new
[`SddfWlanNetSource`](../rust/src/sunfish_wlan_net_source.rs) exposes that TX
dequeue to Rust through a synchronous callback and rejects invalid frame
lengths. The Rust/C FFI test verifies that insufficient output capacity leaves
the shared descriptor queued, then checks that a successful copy returns its
buffer to sDDF before the caller processes the private Ethernet copy.

Initialization requires the caller to supply the effective QCACLD RX decap
mode and rejects anything except Ethernet mode 2. That mode is the pinned
default, but QCACLD can select Native Wi-Fi under a compile-time alignment
workaround or RAW in monitor mode, so a future runtime must pass the actual
negotiated value rather than rely on the default. This adapter only performs
copies between already-mapped windows. It does not create mappings, synchronize
DMA, implement Wi-Fi link control, or provide a deployed Sunfish network
composition; the radio path remains unconnected. The Rust TX source implements
a bounded frame-source interface used by the supplicant-to-CE4 path below.

The current TX adapter supports a single mapped sDDF source region and rejects
nonzero `oid` values. sDDF's TX virtualizer preserves each client memory region
in that field, so a multi-client composition must either supply and validate a
region table or put a trusted copy/virtualization service between those client
regions and this adapter. The generated network queue alone does not grant the
WLAN PD CPU access to those buffers; the system description must provide that
mapping explicitly.

[`rust/src/ieee80211_ethernet.rs`](../rust/src/ieee80211_ethernet.rs) adds the
next pure TX codec, checked by
[`validate-ieee80211-ethernet.py`](../platforms/sunfish/validate-ieee80211-ethernet.py)
against the pinned Lineage conversion helper: it maps bounded Ethernet-II
input to an unprotected,
non-QoS To-DS data frame with RFC 1042 LLC/SNAP (and bridge-tunnel OUIs for
AARP/IPX). The composed pipeline below applies CCMP to protected data and uses
the caller-selected vdev/TID and sequence values before invoking the QCACLD
HTT encoder. The host suite covers standard IPv4, VLAN, AARP/IPX, bad identity,
length, and capacity cases. The pipeline test exercises one bounded Ethernet
source packet through framing, encryption, and a publisher interface; a mock
publisher verifies its plan and cursor transition. Separate native CE tests
exercise the real gated publisher against mock MMIO and DMA memory. The FFI
suite separately verifies that the real sDDF bridge copies and retires its
shared TX descriptor correctly. No test accesses the radio or phone hardware.

## Bounded station authentication and association codecs

[`rust/src/ieee80211_station.rs`](../rust/src/ieee80211_station.rs) now encodes
open-system authentication and association requests and validates the
corresponding responses from the selected BSSID. Association requests preserve
caller-selected supported rates, RSN contents, and ordered additional
capability IEs while rejecting duplicate core IEs, malformed IE streams,
inconsistent ESS/IBSS/privacy bits, invalid addresses, and short outputs before
changing the caller buffer. Response parsing checks management subtype,
protection state, peer addresses, authentication algorithm/transaction, status,
and successful AID range. The request shape follows the pinned LineageOS
`mac80211/mlme.c` association layout: fixed capability/listen fields followed
by SSID, rates, security, and caller-supplied capabilities.

These files remain frame codecs, not a station controller. The selected-BSS
management sequence is now composed by `ieee80211_join.rs` below, but scan
selection, channel scheduling, retry/timeouts, deauthentication, and connecting
management/EAPOL frames to CE/HTT remain outside the codecs. A successful
association response still does not authorize protected traffic; the WPA2
transaction keeps that behind its explicit key-install and M4-submission gate.
Seven host tests cover the codec encodings, peer/status checks, address and
capability validation, and failure atomicity. No radio operation is performed.

## WPA2-PSK key derivation primitive

[`rust/src/ieee80211_wpa2.rs`](../rust/src/ieee80211_wpa2.rs) now derives a
32-byte WPA2-Personal PMK from a printable passphrase and SSID with
PBKDF2-HMAC-SHA1, then derives the CCMP-128 KCK/KEK/temporal key from the PMK,
peer MAC addresses, and ANonce/SNonce. The address and nonce ordering and
SHA-1 PRF label/counter follow Android's maintained
[`wpa_common.c`](https://android.googlesource.com/platform/external/wpa_supplicant_8/+/refs/heads/main/src/common/wpa_common.c)
and [`sha1-prf.c`](https://android.googlesource.com/platform/external/wpa_supplicant_8/+/refs/heads/main/src/crypto/sha1-prf.c)
reference. PMK and PTK material are held in non-copyable wrappers that zeroize
their owned buffers on drop; raw 64-hex-digit PSKs, other AKMs/ciphers, and
non-ASCII passphrases are outside this interface.

Three unit tests cover the standard `password`/`IEEE` PMK vector, an independent
CCMP-128 PTK vector plus argument-order invariance, and invalid input rejection.
The Sunfish test runner also compiles this module as an AArch64 `no_std` crate
using the pinned nightly and a locally built `core` library.
This section describes only the key-derivation primitive; the station
transaction and data-protection integration are covered below. The primitive
does not install keys or authorize or start the radio.

## Bounded WPA2 EAPOL-Key codecs

[`rust/src/ieee80211_eapol.rs`](../rust/src/ieee80211_eapol.rs) adds a bounded
EAPOL-Key parser, message-1 validation, message-2 encoding with the selected
association RSN IE, constant-time HMAC-SHA1-128 MIC verification, and message-3
checks for the ANonce, increasing replay counter, required key-info bits, and
bounded AES-wrap input shape. Its descriptor/key-info layout follows Android's
[`wpa_common.h`](https://android.googlesource.com/platform/external/wpa_supplicant_8/+/refs/heads/main/src/common/wpa_common.h)
and the supplicant's [message-1/message-2 flow](https://android.googlesource.com/platform/external/wpa_supplicant_8/+/0be32f16ad21da5b5f30ab56b2b49a10260c41fc/src/rsn_supp/wpa.c).
Message-3 key data remains encrypted when returned; callers must AES-unwrap
and validate the selected RSN/GTK KDE before using any key material. The
standalone codecs do not install keys or authorize protected traffic. They have
no retransmission timer or driver installation path and do not make the radio
operational.

## WPA2-PSK station transaction and CCMP data policy

[`rust/src/ieee80211_supplicant.rs`](../rust/src/ieee80211_supplicant.rs)
joins the WPA2-PSK/CCMP primitives into a bounded M1–M4 association
transaction. It checks a strict WPA2-Personal/CCMP RSN profile, binds EAPOL to
the selected station/BSSID addresses, answers M1 with a stable SNonce/PTK,
verifies M3 MIC and replay state, unwraps and validates the exact RSN IE and
optional GTK KDE, and encodes M4 from the authenticated M3 fields. Exact M1
and M3 retransmissions are handled without deriving or installing a second
key set.

The driver remains behind an explicit boundary: `key_install_view()` lends the
pairwise key, optional GTK, and receive sequence counter to the caller; the
caller confirms successful installation and successful M4 submission
separately. Protected data stays unauthorized until both confirmations
succeed and M3 has the Secure bit. The ordering follows Android's maintained
[supplicant M4/key-install flow](https://android.googlesource.com/platform/external/wpa_supplicant_8/+/b6e9aaf735990dc64cdb6efccc03d076768eabf3/src/rsn_supp/wpa.c): M4 may be submitted before the driver confirms key installation. Pairwise and group RX use independent per-TID CCMP replay windows, with the GTK window seeded from M3's RSC; station-originated data uses the PTK.

Host tests exercise a complete synthetic four-way handshake, failure-safe
small unwrap buffers, the install/M4 authorization gate, and authenticated
pairwise and group CCMP traffic. The same authorization gate now composes one
Ethernet-II packet through To-DS encapsulation and pairwise CCMP, clears the
plaintext scratch after both success and failed encryption, and preserves the
transmit packet number when output capacity is short. The bounded TX pipeline
now checks authorization, staging space, software-encryption policy, and a
maximum-size CE4/DMA plan before it dequeues a shared Ethernet packet; then it
encapsulates and encrypts the private copy and passes it to
`NativeIo::publish_qcacld_data_tx`. That publisher holds its Gate while it
revalidates the live generation, HTT endpoint, ring geometry, CE lease, DMA
aperture, and SMMU route, then copies the frame/prefix/descriptors, orders those
writes, and advances CE4's producer index. Tests verify that unauthorized
traffic and invalid CE routes leave the source queue untouched, the CCMP packet
number is still fresh, plaintext scratch is cleared, and the HTT `NO_ENCRYPT`
flag prevents firmware from applying CCMP again. The integrated test uses a
mock publisher; native CE tests exercise the publisher against mock MMIO/DMA.
There is still no production Gate or Sunfish system composition wired to this
path, and no live radio transmission has occurred. This is not yet a complete
Wi-Fi station: scan/BSS selection, channel scheduling, retry/timeouts, rekeying,
deauthentication, PMF, fragmentation, a production key installer, and CE/HTT
radio transport remain outside the WPA2 module. No device has been asked to
start WLAN by this code.


## Selected-BSS authentication and association flow

[`rust/src/ieee80211_join.rs`](../rust/src/ieee80211_join.rs) now connects the
open-system auth/association codecs to the WPA2 supplicant. A caller supplies
one already-selected BSS, a strict RSN IE, rates, passphrase, and fresh SNonce;
the controller validates the complete request before emitting auth, then
sequences the AP's auth response, association request/response, EAPOL M1/M2,
and M3/M4 state. Rejected auth or association is terminal for that join
attempt, malformed or wrong-peer responses leave the current phase unchanged,
and management sequence numbers wrap at 12 bits only after a frame is encoded.
The PMK is derived before the first radio exchange, while SNonce is held in a
zeroizing wrapper until the WPA2 transaction takes ownership.

The controller preserves the explicit key boundary: the caller obtains the
validated PTK/optional GTK view, reports successful installation, and confirms
that M4 was accepted by its transport. Only then can the joined state pass
protected data to CCMP. Host tests cover the auth/association sequence, AID,
M1 transition, terminal AP rejection, wrong-peer rejection, and output-buffer
failure atomicity. The controller still depends on external BSS selection,
regulatory channel policy, scan result/RSSI handling, frame transport, key
installer, and packet network service; it has not been connected to the
live WMI/HTT/CE runtime or exercised on the Pixel. A new host-only composition
test now takes its auth and association requests through the synthetic
WMI_CONTROL/HTC/CE software queue; that validates serialization and queue
state, not radio submission.

[`rust/src/ieee80211_bss.rs`](../rust/src/ieee80211_bss.rs) now supplies the
bounded selection step that used to be external. It considers at most 128
caller-provided scan frames, matches an exact configured SSID, requires
infrastructure/privacy capabilities and supported rates, accepts only the
implemented WPA2-PSK/CCMP RSN profile, and limits the result to the caller's
approved channel list and optional pinned BSSID. The strongest qualifying
RSSI wins, with a stable BSSID/channel tie break. It borrows scan data and
performs no scanning, association, or transmission. In-module tests cover
policy validation, security/profile filtering, pinning, channel filtering,
RSSI selection, and tie ordering. `JoinConfig::from_selected_bss` now binds
the selector's retained BSSID, SSID, rates, and complete RSN element directly
to station-owned capabilities, listen interval, passphrase, and fresh SNonce.
A host test runs a synthetic beacon through channel/RSSI/security selection
and into a ready WPA2 join transaction. These codecs remain disconnected from
live scan completion, regulatory policy, the WMI/HTT/CE transport, and packet
network service.

## WMI management-frame command encoding

[`rust/src/ath10k_mgmt_tx.rs`](../rust/src/ath10k_mgmt_tx.rs) now encodes the
bounded WMI 10.4 management-TX command for unprotected probe, authentication,
and association frames. The pinned ath10k source audit checks command ID
`0x9031`, the 24-byte header field order, peer address selection, frame length,
and four-byte command padding. The encoder fails closed for protected
management frames and must not be selected unless firmware advertises WMI
management TX support. Its queue adapter requires a queue bound from a
negotiated WMI_CONTROL endpoint and matching HTC credit policy, then composes
through the host-only WMI/HTC credit-and-sequence queue and WCN3990 CE
descriptor planner. A combined host test feeds the selected-BSS join's
authentication and association requests through that queue, verifies the
HTC/WMI frame bytes and sequence progression, and accepts synthetic AP
responses through association. Host tests and the AArch64 `no_std` compile
pass; this does not implement the alternate HTT management path, TX completion,
DMA publication, doorbells, scheduling, or radio authority.

## WMI 10.4 scan cancellation

`rust/src/ath10k_scan.rs` now encodes and queues the matching `STOP_SCAN`
command for a running scan. The pinned Sunfish `ath10k` source defines the
10.4 stop command as `0x9001`, `WMI_SCAN_STOP_ONE` as zero, and the four-word
body in requestor-ID, scan-ID, request-type, vdev-ID order. Its generator adds
the host prefixes to the active requestor and scan IDs. The implementation
keeps the scan in `Aborting` after successful software enqueue, restores
`Running` if the software transport rejects the command, and only retires it
after the matching terminal scan event. [`validate-ath10k-stop-scan.py`](../platforms/sunfish/validate-ath10k-stop-scan.py)
checks those details against the pinned [`wmi.h`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/wmi.h)
and [`wmi.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/wmi.c).

`ScanWorkflow::queue_stop` connects that command to the same bounded host
credit/sequence/CE-slot model used for scan start. The no-radio Microkit probe
now composes a synthetic start and matching stop, checks both HTC/WMI frames
and CE descriptors, and reclaims the two software ring entries. The focused
scan test passes 58 cases, the scan-result workflow tests pass 6 cases, and
the probe isolation test plus target image build pass. The rebuilt temporary
image has SHA-256
`b0aec5cda66dbc1e81604763ec66be48dfd348bf5ba2cdbc4dd6610761fa3d5b`.
This remains a software model: the image has not been booted for this revision,
and neither command is published to hardware or sent to WLAN. IRQ completion,
DMA ownership, firmware availability, and production station/network
integration remain open.

## QCACLD WMI management-TX staging

[`rust/src/sunfish_qcacld_mgmt_tx.rs`](../rust/src/sunfish_qcacld_mgmt_tx.rs)
now encodes Sunfish QCACLD's by-reference management-TX command for bounded
open-system authentication and association requests. It requires service 76
(`WMI_SERVICE_MGMT_TX_WMI`) in the same generation's captured service-ready
bitmap, writes the pinned `0x7008` command with its 36-byte fixed structure and
up to 64 inline frame bytes, and wraps it in the dynamically assigned HTC WMI
endpoint using caller-provided message and credit limits. A bounded completion
decoder handles WMI event `0x7006`; its generation-bound outstanding table
keeps each by-reference DMA frame leased until the matching firmware event, not
merely until CE consumes the command. The complete frame must already reside
in a bounded CE IOVA pool; the encoder performs no DMA or cache operation. Its
CE preflight requires the selected WMI_CONTROL CE3/CE2 route and a valid
source-ring slot, but leaves the producer cursor unchanged.
`NativeIo::publish_qcacld_mgmt_tx` rechecks that the complete frame matches
the command's IOVA, length, and inline prefix, rejects overlap with the command
buffer or CE ring memory, reserves its completion lease before entering the
hardware gate, then stages the referenced frame, WMI command, and CE descriptor
before publishing the source producer. A definite pre-publication failure
cancels the lease; uncertain publication keeps it for recovery. This remains
an adapter test against mock MMIO/DMA, not authorization or evidence of a live
radio send.
[`validate-sunfish-qcacld-mgmt-tx.py`](../platforms/sunfish/validate-sunfish-qcacld-mgmt-tx.py)
checks the service index, command/event IDs, TLV tags, field order, and status
values against the pinned Lineage QCACLD source. Eleven module tests cover
service gating, exact TLV/HTC bytes, authentication and association bounds,
DMA ranges, completion parsing, and lease retirement. A 122-test native CE
mock suite now composes an authentication request through the CE3 publisher
and retires its lease only after the matching completion. The full
`platforms/sunfish/test-modem-qmi.py` host and AArch64 `no_std` validation
passes. The mock test is not radio evidence: a production CE3 Gate, live HTC
credit/event owner, DMA frame allocator, and HTT-based management-TX fallback
are still absent.

## WMI 10.4 CCMP key installation

[`rust/src/ath10k_key.rs`](../rust/src/ath10k_key.rs) now serializes the
bounded CCMP-128 pairwise and group-key form of
`WMI_10_4_VDEV_INSTALL_KEY_CMDID` and can place it in the existing WMI_CONTROL
software queue. The pinned Sunfish ath10k source maps CCMP to
`WMI_CIPHER_AES_CCM`, uses key flags 0/1 for pairwise/group, and defines a
92-byte packed fixed prefix followed by the key bytes. Linux zeroes the skb
before filling it; this encoder therefore zeroes the RSC/TSC fields and MAC
alignment bytes. The supplicant's received sequence counter remains usable by
the host replay check, but the pinned ath10k key argument does not carry it to
the WMI command. [`validate-ath10k-key.py`](../platforms/sunfish/validate-ath10k-key.py)
checks these details against the pinned [`wmi.h`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/wmi.h),
[`wmi.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/wmi.c),
and `mac.c`.

Host tests verify the wire offsets, pairwise/group flags, key index, zeroed
counters, output failure atomicity, and WMI_CONTROL queue frame. This is only
serialization and a software CE-slot plan. A generation-bound
`CcmpKeyInstallTracker` now records successfully queued pairwise and optional
group commands, then matches SEC_IND peer, CCMP cipher, and unicast/group type;
stale generations, mismatches, pre-queue indications, and duplicates do not
advance readiness. SEC_IND does not carry the WMI key index, so the tracker
supports only the single configured group-key install represented by this
flow. `Wpa2Join::confirm_keys_installed` now accepts only a completed tracker
whose owner generation, BSSID, pairwise index, and optional GTK index match
the current association. Its protected-data gate remains closed until that
evidence and M4 submission are both confirmed. The native WMI/HTT event owner
still does not feed real device completions into this model. The frame buffer
contains the plaintext key
in both the caller-provided command-body scratch and queued-frame output; their
owner must protect and clear those buffers after the real transport consumes
the frame.

The current no-radio protocol probe also runs synthetic pairwise and group
key frames through the bounded software WMI_CONTROL queue, parses synthetic
HTT SEC_IND messages, and exercises the generation-bound tracker, including
wrong-peer/cipher/cast rejection, stale events, and duplicate suppression,
then clears its scratch buffers. The same probe checks stale-generation and
wrong-peer rejection, duplicate suppression, and the transition to complete
only after pairwise and group indications. It does not publish CE descriptors
or transmit either frame. Source audits, host tests, probe isolation checks,
and the AArch64 image build pass. This still does not confirm a live firmware
key install or resolve the separate modem/WLFW launch and firmware
authorization blockers.

## Read-only SMMU handoff snapshot result (2026-10-24)

A photo from the temporary read-only SMMU handoff probe now shows six valid
SMR entries and six translated context-bank references. Its selected-route
summary reports SID `0x0300` as translated through CB02 with the bank `OFF`;
SIDs `0x0240`, `0x0241`, `0x0520`, `0x0521`, and `0x0522` show `NO ROUTE`.
The same frame reports `SMMU GFSR 00000000`, `CB COUNT 0000003D`, and
`ACTLR NONZERO 0000003D`. This is raw probe-reported handoff state: it shows
the selected route is inactive at the sample, but does not identify why CB02
is off, establish ownership, or prove the later LineageOS runtime map. No SMMU
register was written. Keep native SMMU programming blocked until context-bank
ownership and the expected route are independently verified.

The frame also reports `ID0 4C017E4E`. Its `NUMSMRG` field is `0x4e`;
the pinned Sunfish Linux driver uses that field directly as the number of
stream-match groups, so the hardware exposes 78 slots numbered 0–77. The
manifest's former count of 79 was incorrect; slot 78 is outside the hardware
range. The current photo is `IMG_8967.heic` (SHA-256
`06df2e4816ba0d8bcb8cbbca1ac2a43626a43aa3b50eaf5b76f31c98805b67b2`).

## Live LineageOS ICNSS-to-SMMU binding (2026-10-24)

A bounded read-only rooted-ADB inspection of the running LineageOS system
confirmed the device-tree model `Google Inc. MSM sm7150 Sunfish DVT1.0`, kernel
`4.14.357-openela-g28f9290ae067`, and an active Wi-Fi connection. Its live
`/sys/firmware/devicetree/base/soc/qcom,icnss@18800000/iommus` property contains
big-endian cells `0x30, 0x240, 0x1`; the phandle resolves to the Apps SMMU at
`0x15000000`. The ICNSS platform device is bound to the `icnss` driver and is
the sole device listed in Linux IOMMU group 7. This read did not unload WLAN or
change radio, route, or SMMU state.

This runtime evidence resolves the earlier source comparison: Lineage's
Google DVT1.0 overlay is built on `qcom-base/sdmmagpie.dtb`; the corresponding
`qcom/sdmmagpie.dts` includes `sdmmagpie.dtsi`, whose ICNSS node declares
`0x0240/0x1`. The selected Google overlay contains no direct ICNSS/IOMMU
override, and its final live property matches the base DTS. The generic
`qcom/sm6150.dtsi` also contains an ICNSS node, but its `0x1640/0x1` assignment
does not describe this Sunfish overlay path. The live binding and group
membership establish Linux's device association only; sysfs does not expose
the actual S2CR type, context bank, full SMMU route state, or authority for
seL4 to program it.

### Diagnostic Lineage SMMU route readback (2026-10-09)

A temporary Fastboot boot of a diagnostic LineageOS kernel captured the Apps
SMMU registers after ordinary Linux device attachment. Its read-only patch
reads SMR, S2CR, CBAR, and SCTLR; it adds no SMMU writes and leaves Linux's
route setup intact. The raw capture is retained at
[`lineage-smmu-runtime-route-readback.log`](../platforms/sunfish/fixtures/lineage-smmu-runtime-route-readback.log)
and parsed by
[`smmu_runtime_route_readback.py`](../platforms/sunfish/smmu_runtime_route_readback.py).

The ICNSS route is SID/mask `0x240/0x1`, SME13, CB6 (`SMR=0x80010240`,
`S2CR=0x00000306`, `CBAR=0x0001f300`, `SCTLR=0x00c000e6`). IPA's WLAN path
is a separate exact SID `0x521` at SME9/CB38; IPA AP and UC use `0x520` at
SME8/CB37 and `0x522` at SME10/CB39. The USB SID `0x540` appears first on
CB1 and later on CB23, showing that route observations change as Linux
attaches devices. The ICNSS S2CR selects translation, while its captured
`SCTLR.M` is clear, consistent with the pinned Linux S1-bypass policy.

These are post-attach Lineage routes that Linux created; they are not the
earlier ABL snapshot. The ABL snapshot still shows no WLAN or IPA routes at
Fastboot handoff. Neither capture proves that these Linux context banks or
routes are inherited by seL4, that seL4 has authority to reproduce them, or
that a bypass context is an acceptable isolation model for native DMA. The
native WLAN gate still needs a separately owned translated context, a
non-clobbering SMMU integration, and a valid authority/fault-containment path.

Do not infer actual page-table translation from `S2CR_TYPE_TRANS` alone. In
the pinned Sunfish Linux path, the ICNSS node requests S1 bypass; the captured
context uses the S1 CBAR format but has `SCTLR.M` clear. The native WLAN DMA
plan instead emits an S1-translated context with `SCTLR.M` set. The CE live
gate now records that context-mode check separately from the route's TRANS
bit and rejects an S1-bypass context before any CE access. This is a software
invariant only; there is still no production Gate or SMMU authority provider.

## Temporary CCMP wire self-test image (2026-10-24)

The rebuilt temporary `wlan-protocol-probe` image includes the synthetic CCMP
pairwise/group and SEC_IND self-test above. The target domain has only the
reserved splash framebuffer mapping; its CE addresses are software plans and
no descriptor is published. Fastboot confirmed product `sunfish` and
`fastboot boot` returned success. The first screen showed `KEY FRAME PASS` but
`SCAN FRAME FAIL`: the probe expected STOP_SCAN descriptor metadata `0x0020`
while its transfer ID 3 correctly produced `0x0030`. Host reproduction
confirmed all encoded scan fields, credits, and ring states; only that local
assertion was wrong. The assertion is corrected, and the rebuilt Fastboot
image SHA-256 is
`36510657e9fdeed0b00b4f078e87ab21b3cb74e62ace7e23edad6f403ae096c8`.
The corrected image booted temporarily and the photographed screen reports
`CE DESC PASS`, `QRTR PASS`, `IPA CODEC PASS`, `WMI 10 4 PASS`,
`HTC WMI TX PASS`, `SCAN FRAME PASS`, `QCACLD INIT PASS`, and
`KEY FRAME PASS`, ending with `NO RADIO ACCESS`. This confirms the local
synthetic wire/self-test paths on the target, not a live transport or WLAN
initialization. Neither temporary image changes a partition or starts WLAN.

## Refreshed LineageOS runtime-DT and offline validation (2026-10-24)

While the Pixel remained in LineageOS with Wi-Fi connected, the read-only
`capture-live-fdt.py --root-read-only` path captured the active FDT to
`.cache/pixel-sunfish/runtime-dt-20261024.dtb` (588,337 bytes; SHA-256
`84e7cbe0a858f23d830e0f2abb13067e953021cbc9d6fc62fb1b6d712a71b3dc`). This
matches the same-boot runtime-FDT hash recorded by the Wi-Fi bus comparison.
The capture's `finally` path restored ADB to shell UID 2000; the Pixel remains
boot-complete with Wi-Fi connected.

`validate-wlan-resources.py` passes against both this current FDT and the
generated Microkit `kernel.dtb`. Both preserve the ICNSS resource and
no-map reservation policy, the IPA v4.2 inventory, and zero native IPA
register/IRQ/PAS/SMMU grants. The live ICNSS node maps Apps SMMU SID/mask
`0x240/0x01` and requests stage-1 bypass; the separate IPA WLAN SMMU context
uses SID/mask `0x521/0x00`. Read-only sysfs places ICNSS in IOMMU group 7 and
the IPA WLAN context in group 3. The running kernel does not support debugfs,
and its retained filtered log has no SMMU fault/register diagnostics. These
facts reconfirm Linux's bypass configuration and device grouping; they do not
show a translated native context or transfer ownership of either route.

The full `platforms/sunfish/test-modem-qmi.py` runner also passes on the
current worktree, including the host protocol/ownership suites, AArch64
`no_std` compilation, native code-generation checks, and host UFS integration
tests. This closes no hardware gate: production AP IPA/SERVREG/storage
providers, authorized WLAN firmware, and exclusive translated WLAN DMA/SMMU
ownership remain necessary before a native radio-start attempt.

## Generation-bound pre-MPSS owner handoff (2026-10-24)

`PreparedModemBoot` now consumes the actual `sunfish_ipa_post_init::Ready`
proof, stages the selected IPA/SERVREG and optional memory/TFTP services before
MPSS, and retains that generation. Its SMP2P constructor binds the handshake
and resulting ready observation to the same generation; the GLINK channel
checkpoint carries that evidence through `attach_after_glink()`. Attachment
passes the generation into the QRTR session, FIFO adapter, native owner, MSA
reservation, and live gate. A mismatched checkpoint is rejected before any
shared-memory write. The lower-level prepared attach path enforces the same
check.

The 281-test native owner suite passes, including a generation-7 prepared-to-
attach path and a different-generation rejection that verifies SMEM, lock, and
doorbell remain unchanged. `platforms/sunfish/test-modem-qmi.py` also passes
with `RUST_TEST_THREADS=1`: host protocol/ownership suites, AArch64 `no_std`
compilation, native code-generation checks, and the host storage integration
tests all complete. One default-parallel run hit a transient `try_lock()` error
in the host storage integration test; the standalone serialized storage tests
and the full serialized runner both pass.

This is a typed software handoff, not a platform boot runner. It does not load
or authenticate firmware, release MPSS, create production IPA/SERVREG or
storage/heap providers, or establish WLAN DMA/SMMU ownership. No device state
was changed for this validation; the Pixel remains in LineageOS.

## Repeated LineageOS WLAN readiness capture (2026-10-24)

After the Pixel was reconnected over ADB, the read-only
`capture-lineage-ipa-state.sh` capture again found a successful
`QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` response, SERVREG UP for
`msm/modem/wlan_pd`, ICNSS connected to the WLFW service, and both
`icnss: WLAN FW is ready` and the WMA FW-ready event. The boot log again places
IPA, QRTR, TFTP, PD mapper, and remote storage before modem readiness. This is
repeatable installed-LineageOS reference evidence; it does not prove native
seL4 service providers or radio ownership. Debugfs remains unsupported, so IPA
register snapshots are unavailable, and no Peripheral Manager lifecycle lines
were present in the retained log buffer. The capture only reads properties and
existing logs.

## HTC receive-credit direction and trailer correction (2026-10-24)

The receive-side parser had reused HTC's send-direction `NEED_CREDIT_UPDATE`
bit meaning for HTT data frames. Pinned Sunfish `htc.h` defines receive bit 0
as `RECV_1MORE_BLOCK` (bundle state) and receive bit 1 as `RECV_TRAILER`;
the HTC RX path reads the trailer length from ControlBytes0, strips the trailer
from the application payload, and routes credit records to the credit
accounting path. HTC TX alone uses `NEED_CREDIT_UPDATE` and advances the
endpoint sequence in ControlBytes1.

The Rust parsers now keep send and receive flags separate, reject unsupported
receive bundling, validate trailer record bounds and reserved fields, and
aggregate returned credits by endpoint. The HTT RX decoder and composed RX
pipeline preserve those totals after the CE scratch buffer is reused, and the
generation-bound WMI credit ledger can apply the validated totals
transactionally. `validate-sunfish-htc-credit.py` pins the relevant source
files and checks the receive/send semantics and the single WMI endpoint
allocation assumption.

The focused suites pass: 43 HTC/WMI tests, four HTT endpoint tests, seven HTT
RX-indication tests, fourteen receive-transport tests, and twenty delivery
pipeline tests. The full serialized `test-modem-qmi.py` runner also passes,
including AArch64 `no_std` compilation and native code-generation checks.
This remains offline protocol work: no live HTC receive event or production
credit/event owner is connected, so the HTT RX pipeline carries returned
credits but does not itself drive the device ledger or hardware.

## QCACLD WMI receive-event dispatch (2026-10-24)

[`sunfish_qcacld_wmi_rx.rs`](../rust/src/sunfish_qcacld_wmi_rx.rs) now
provides the bounded software owner for one WMI CE2 completion. Its
`consume_and_dispatch` adapter accepts the generation-checked CE2 copy callback
and bounds parsing to the descriptor's reported length. It validates the
negotiated HTC endpoint and complete WMI
envelope, applies trailer credit totals to the matching generation-bound WMI
credit ledger, and decodes QCACLD `SERVICE_READY` (`0x1`), `READY` (`0x2`),
`SERVICE_AVAILABLE` (`0x3`), scan (`0x3001`), and management-TX completion
(`0x7006`). The READY parser checks the full 60-byte fixed structure, host ABI,
zero status, and bounded extra-MAC list instead of accepting an ath10k-sized
prefix. The scan decoder checks the exact fixed-parameter TLV, including the
seven-word payload and TSF field; management completion reuses the strict
descriptor/status parser. Unrecognized event IDs remain visible as bounded
borrowed bodies.

Credit processing follows HTC framing rather than application decoding. A
fully valid trailer is applied even if a known WMI event body is malformed;
stale generations, wrong endpoint identity, malformed HTC trailers, and
over-returned credits fail without changing the ledger. The new
[`validate-sunfish-qcacld-wmi-rx.py`](../platforms/sunfish/validate-sunfish-qcacld-wmi-rx.py)
checks the IDs, TLV tags, field order, and event table against the pinned
QCACLD headers. Eight focused tests and the complete serialized
`test-modem-qmi.py` host/AArch64 run pass.

This layer is not wired to `NativeIo::consume_wmi_rx`, a production CE2 IRQ
dispatcher, or the lifetime owner for HTC setup and WMI readiness. The bounded
adapter now retires a management-frame lease after its exact completion, but
the production lease/pool owner is still absent. READY's ABI and status are
validated here; service capability policy and startup transitions remain in
the future runtime coordinator. No LineageOS driver was unloaded or replaced,
and no radio path was exercised.

## WMI management-TX completion retires its frame lease (2026-10-24)

`consume_dispatch_and_retire_mgmt_tx()` now composes the CE2 completion copy,
HTC/WMI validation, and the existing generation-bound
`MgmtTxOutstanding` table. A parsed management-TX completion releases only the
lease with the matching descriptor ID in the matching generation. Events of
other types leave the table untouched; malformed events do not reach lease
retirement, and unknown descriptor IDs or stale generations fail without
releasing a frame. A valid HTC credit trailer is still applied according to
transport framing before the event-level retirement decision.

The same RX layer now parses SERVICE_READY, SERVICE_AVAILABLE, and READY with
the pinned QCACLD-specific views. In particular, READY requires the complete
60-byte fixed parameter and matching ABI/status rather than accepting the
shorter ath10k prefix. An explicit regression rejects that prefix, and the
SERVICE_AVAILABLE test verifies its segment offset and extended service bit.
The validator checks event IDs and the QCACLD parser path. Eleven focused
WMI-RX tests, the QCACLD WMI suite, the source validators, and the serialized
Sunfish host/AArch64 runner pass.

This is still a host-side composition: it does not own production CE2 ring
refill, interrupt acknowledgement, management-frame pool reuse, WMI startup
policy, or a radio-facing runtime, and no device state was changed.

## Public SMMU authority sources (2026-10-24)

The public Qualcomm [Snapdragon access-control overview](https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/an-introduction-to-access-control-on-qualcomm-snapdragon-platforms.pdf)
describes a generic Snapdragon authority split: TrustZone controls secure
context-bank mappings; the hypervisor controls non-secure stream-ID-to-context-
bank routing and stage-2 context-bank configuration; and the CPU OS can
configure only stage-1 contexts specifically delegated to it. The document
also says that delegation is selective. This is useful context, but it is not a
Sunfish boot-chain contract and does not prove this device's exact delegation.

The pinned LineageOS vendor source shows ICNSS reads
`qcom,smmu-s1-bypass`, requests `DOMAIN_ATTR_S1_BYPASS`, and attaches a domain.
The ARM SMMU driver configures its context-bank control for S1 with S2 bypass
but leaves `SCTLR.M` clear for S1 bypass; the route still uses `S2CR_TYPE_TRANS`
to select that context bank. Attach initializes the context before programming
the route, then writes the context-bank index into S2CR before making the SMR
valid. ICNSS separately calls `hyp_assign_phys` for WLAN MSA memory permissions;
that operation does not program the Apps-SMMU SID route.

An earlier SID comparison used the generic Lineage `sm6150.dtsi`, which assigns
`0x1640/0x1`, instead of the source selected by Sunfish. Lineage's Google
Makefile maps the active `sm7150-sunfish-dvt1.0-overlay.dtbo` to
`qcom-base/sdmmagpie.dtb`; `qcom/sdmmagpie.dts` includes `sdmmagpie.dtsi`, whose
ICNSS node assigns `0x0240/0x1`. The running DVT1.0 device tree and upstream
Sunfish WLAN DTS both report `0x240/0x1`. The `0x1640` value remains recorded
as a generic-SM6150 comparison, not as Sunfish's active ICNSS SID. The
validator follows this overlay/base-DTS chain and checks the live Lineage
readout values.

The vendor SMMU driver's `qcom,skip-init` path also imports valid translated
SMR/S2CR routes that are already programmed by the bootloader, reserves their
context banks, and requires exactly one device-tree device to overlap those
bootloader stream IDs. This is a Linux handoff implementation, not a documented
interface for a replacement EL1 kernel. It makes the bootloader's existing
route state relevant, but does not identify the owning device or establish
what the seL4 image is allowed to change.

The Linux code demonstrates an HLOS implementation path, not delegated
authority for this seL4 image. It does not prove that a Fastboot-booted seL4
image has the same Apps-SMMU mapping, EL access, power/clock state, secure
permissions, or exclusive route/context-bank ownership. Do not treat the
generic overview as proof that Sunfish Linux cannot program a route, or either
Linux implementation as authorization for seL4 to do so. The pinned source
comparison is recorded in
[`wlan-resources.json`](../platforms/sunfish/wlan-resources.json), with links
to the [LineageOS ICNSS driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c),
[LineageOS Sunfish base DTS](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/sdmmagpie.dtsi),
[LineageOS generic SM6150 DTS](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/sm6150.dtsi),
[LineageOS Sunfish overlay selection](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/google/Makefile),
[upstream SM7150 DTS](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150.dtsi),
[upstream Sunfish target DTS](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts),
and [LineageOS ARM SMMU driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu.c).

The public sources reviewed here do not document a Sunfish-specific boot-chain
grant or supported API giving this seL4 image control of the WLAN SID route or
Apps-SMMU context-bank allocation. Therefore keep native route/context-bank
programming disabled until its own MMIO, power, exclusive-ownership, and fault-
containment gates are met. A read-only ABL snapshot, a Linux DT declaration,
Linux's working S1-bypass request, root access in LineageOS, and
`hyp_assign_phys` use do not independently satisfy those gates.

There is a useful distinction between a generic seL4 API and Sunfish platform
support. The pinned seL4 source documents an Arm SMMUv2 capability API for
creating SID/CB caps, binding a stream to a context bank, assigning a VSpace,
and handling faults. However, the generic `ARM_SMMU` build option depends on
`KernelPlatformTx2`; the Sunfish board glue explicitly sets it off, and the
generated Sunfish kernel config has `ARM_SMMU: false`. This is a platform
support boundary in the current port, not proof that seL4 has no SMMU API.

Enabling that generic option is not a safe shortcut. The generic boot initializer
calls `plat_smmu_init`, whose reset path rewrites every stream-to-context route,
invalidates every stream-match entry, disables every context bank, clears
context-bank fault state, changes the ACTLR prefetch bit, and invalidates SMMU
TLBs before enabling the SMMU. It is therefore incompatible with preserving an
unowned bootloader handoff until a Sunfish-specific initialization/handoff path
replaces that reset behavior and proves the MMIO, clock/power, access-control,
SID, CB, and fault-IRQ details. The pinned seL4 caveats also list its verified
AArch64 configurations as having no SMMU, so enabling the generic driver would
be outside those verified configurations.

There are additional TX2 assumptions to port. The generic SMMUv2 header fixes
the controller base at `0x12000000`, while Sunfish's runtime DT declares the
Apps SMMU at `0x15000000`. The generic mapping code says it was tested only on
TX2, assumes a static kernel virtual mapping for its register windows, and
maps each context bank into that fixed window. The Sunfish controller is
declared as Qualcomm QSMMUv500 with a `0x100000` register range plus a separate
register range; a valid port must derive and map the actual Sunfish layout
rather than replacing the TX2 constant alone.
Separately, Microkit 2.3.1's system-description I/O-address-space path parses
PCI IDs only and rejects AArch64; its x86 IOMMU SDF support does not configure
this ARM SMMU. A future Sunfish implementation would need explicit kernel and
Microkit integration, rather than merely adding an `<io_address_space>` node.
The current Microkit CSpace SDF only transfers TCB, scheduling-context, and
VSpace caps between protection domains, while its CapDL memory code labels
SMMU support as future work. A WLAN SMMU manager therefore also needs an
explicit, narrowly scoped way to receive SIDControl/CBControl authority and
delegate only the selected SID, CB, and fault handling rights.

The immediate offline gate is now clear: before any native SID/CB binding,
implement or identify a non-clobbering Sunfish SMMUv2 handoff and establish how
the bootloader/hypervisor delegates route ownership to the EL1 image. The
post-attach Lineage routes are now measured and source-checked, but the
Fastboot ABL handoff remains the separate state relevant to seL4; no ownership
or control delegation is established for either state. This source audit
changed no kernel configuration or device state. The details are pinned in
`wlan-resources.json` and checked against the current seL4/Microkit source and
generated Sunfish config. Primary references are the pinned seL4 [Arm
SMMUv2 API manual](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/manual/parts/io.tex),
[ARM_SMMU configuration](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/src/arch/arm/config.cmake),
[TX2 SMMUv2 register header](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/include/drivers/smmu/smmuv2.h),
[SMMUv2 reset implementation](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/src/drivers/smmu/smmuv2.c),
[verification caveats](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/CAVEATS.md),
and the pinned Microkit [AArch64 IOMMU parser](https://github.com/seL4/microkit/blob/ec86afdcd662b5976d11d4994acf1b11a2979882/tool/microkit/src/sdf/iommu.rs)
[CSpace mapping parser](https://github.com/seL4/microkit/blob/ec86afdcd662b5976d11d4994acf1b11a2979882/tool/microkit/src/sdf/cspace.rs)
and [CapDL memory builder](https://github.com/seL4/microkit/blob/ec86afdcd662b5976d11d4994acf1b11a2979882/tool/microkit/src/capdl/memory.rs)
and [2.3.1 change notes](https://github.com/seL4/microkit/blob/ec86afdcd662b5976d11d4994acf1b11a2979882/CHANGES.md).

## QCACLD key-command native publication gate (2026-10-24)

The pinned QCACLD CCMP serializer is composed through the HTC WMI encoder and
CE3 TX planner. The native publisher in
[`sunfish_qcacld_key_native.rs`](../rust/src/sunfish_qcacld_key_native.rs)
requires the matching transport generation, accepted WMI `READY`, the bound
control endpoint, a valid association command, and available HTC credits. It
reserves a sensitive DMA lease before CE3 publication, erases the host staging
frame on every return path, and quarantines uncertain publication. The native
CE path now scrubs only the key-bearing DMA range after the matching CE3 source
read index proves consumption. Host integration tests exercise publication,
credit reservation, SEC_IND tracking, and post-consumption scrubbing; the
source audit pins the 124-byte QCACLD command and its three-credit plan.

The association-owned composition now lives in
[`sunfish_qcacld_key_join_native.rs`](../rust/src/sunfish_qcacld_key_join_native.rs):
it routes decoded CE1 `SEC_IND` events to the active WPA2 join, returns ordinary
RX indications to the caller, and plans/publishes the next pairwise or group
key only after WMI `READY` and the bound control endpoint are present. The
shared native publisher still owns credit reservation, CE3 commit, staging
wipe, uncertain-publication quarantine, and read-index-bound DMA scrubbing.
The offline planner test verifies a join outside `KeysPending` fails before
writing key material. Because `SEC_IND` has no association generation field,
the receive owner must drain prior-association CE1 events before dispatching
under a new association. The native CE1 IRQ service masks the interrupt, drains
bounded HTT events through a synchronous callback, acknowledges only after an
empty poll, and rearms only while receive buffers remain posted. Its WPA2
specialization routes each SEC_IND through the active association before the
owner callback, while preserving the completion's credit reports and borrowed
RX indication for synchronous handling. The system owner still needs to
deliver ordinary RX indications and supply platform-authorized CE/SMMU
resources. No key-install command has been sent to the Pixel. The one-time
rooted LineageOS WMI INIT resource capture noted above was already completed;
this offline work does not repeat it or require another device change.

## Route planners now share the measured Sunfish SMR bound (2026-10-24)

The captured Apps-SMMU ID0 value `0x4C017E4E` reports `NUMSMRG=0x4e`, and the
pinned Lineage driver uses that field directly as the mapping-group array
length. The Rust SMMU planner had retained an older 79-slot bound after the
Python handoff model was corrected to 78. A separate UFS media gate also had
its own 79-slot constant. Those stale limits could accept a nonexistent slot
78 or a 79-group hardware report.

`SUNFISH_MAX_SMR_SLOTS` is now 78 in the shared Rust planner, and the UFS media
gate imports that bound instead of duplicating it. Planner tests accept slot 77
and reject slot 78 or a 79-group geometry; the handoff-model test checks that
the Rust bound matches the captured ID0 value. The modem-storage host harness
now includes the shared SMMU module and its unsafe-media regression rejects
slot 78. The focused 80-test SMMU/CE suite, handoff model and validator, the
full `platforms/sunfish/test-modem-qmi.py` host/AArch64 suite, and the focused
UFS media-gate test pass. This corrects hardware geometry in offline planning;
it does not prove that any route or context bank is free or delegated to seL4.

## Caller-budgeted WPA2 join response deadlines (2026-10-24)

`Wpa2Join` now accepts a caller-supplied monotonic millisecond clock and
response budget for its authentication, association, EAPOL M1, and EAPOL M3
wait phases. The protocol state machine does not choose timeout values or
retransmission policy; its owner must arm the deadline after transport accepts
the request and poll before dispatching each incoming response. Expiry is
one-shot, terminal, rejects late frames, and drops the transient PMK, SNonce,
and supplicant state. `KeysPending` key installation remains the responsibility
of its separate transport owner. Host tests cover phase transitions, expiry,
late-frame rejection, zero duration, deadline overflow, and transient-state
cleanup. This is offline state-machine work; it does not exercise the radio or
provide the missing production system/Gate/SMMU authority.

## Caller-budgeted WMI scan-start response deadline (2026-10-24)

`ScanWorkflow` now supports a separate caller-budgeted deadline for the
firmware `SCAN_STARTED` event. The owner arms it only after native WMI
publication accepts the previously queued `START_SCAN`; queuing bytes in the
software command ring is not treated as a firmware-visible submit. Zero
budgets, deadline overflow, duplicate arming, and arming outside `Starting`
are rejected. The owner must poll this deadline before dispatching each scan
event. Expiry is one-shot, leaves the scan IDs in a terminal timed-out phase,
rejects late `SCAN_STARTED` events, and prevents another scan from reusing the
IDs in that WMI generation. The workflow does not retry; recovery requires the
owner to establish a fresh WMI generation before constructing a new scan
workflow. Focused tests cover the boundary, late event, retry rejection, and
deadline validation. The full `platforms/sunfish/test-modem-qmi.py` suite also
passes, including AArch64 `no_std` compilation and native protocol tests. This
remains offline work: no live WMI command has been published and no production
WLAN Gate or system composition exists.

## Rooted LineageOS retry and ICNSS startup coordinator (2026-10-25)

The attached Sunfish was rechecked over rooted ADB without changing its state.
It identifies as `sunfish` on LineageOS `23.2-20261001-NIGHTLY-sunfish`,
with the stock `wlan` module loaded, Wi-Fi enabled, and `wlan0` up. UID 0 was
confirmed. The bounded IPA/startup capture found no matching IPA, SERVREG, or
WLFW initialization records in the available log buffers; this kernel reports
debugfs unsupported. The existing live WMI INIT resource fixture already
answers the authorized module-capture question, so the WLAN module was not
unloaded or replaced. The phone remains in LineageOS with Wi-Fi up.

`sunfish_icnss_startup.rs` now composes the first WLFW server event with the
source-ordered ICNSS power sequence: it binds the exact server, applies the
verified power plan, and then queues indication registration. Duplicate server
events are idempotent. Failed request preparation restores power and clears the
endpoint; uncertain power rollback or server-removal cleanup quarantines state.
Nine unit tests cover this ordering and failure behavior. The module is
included in the Sunfish host/AArch64 harness, and the complete
`platforms/sunfish/test-modem-qmi.py` suite passes, including AArch64 `no_std`
compilation. The profile still cannot be minted by production code, no trusted
Sunfish power backend is wired, and this coordinator does not start WLAN or
grant SMMU/DMA/secure-call authority.

A later rooted-ADB retry confirmed `adb root` succeeds on this attached
`sunfish` (UID 0); `cnss-daemon`, Peripheral Manager, and `pd-mapper` are
running, with the stock WLAN interface still up and connected. A bounded
current-boot log filter returned only three `cnss-daemon` gateway-parameter
messages ending in `Invalid mac address` / `failed to update gateway`; it
contained no `wlfw_start`, WLAN service request, PDR launch, or SMP2P startup
sequence. This does not identify the private vendor launch trigger. The
device was otherwise left untouched, and the known-good WMI INIT resource
capture was not repeated.

Root-only reads also confirmed the live `/vendor/firmware/modemuw.jsn` SHA-256
matches `wlan-resources.json` (`e75d94b6…9c1a3bf`). Its exact entries map
`kernel/elf_loader`, `tms/servreg`, and `wlan/fw` to
`msm/modem/wlan_pd`, instance 180. There is no `tms/pdr_enabled` entry, so an
empty locator result for that name is expected; it is not evidence that the
`wlan_pd` domain mapping is missing. The running vendor `pd-mapper` is PID 1345,
but `/sys/class/remoteproc` is absent, so the public remoteproc-based
pd-mapper implementation cannot be assumed to describe this vendor daemon.
This confirms the installed service map, not the private initial launch
trigger. The pinned launch-path validator and eight SERVREG/WLFW model tests
pass with this mapping.

## WLFW-to-power generation gate (2026-10-25)

Production WLFW client construction now starts behind a closed ICNSS power
gate. A capability/configuration/registration request cannot be queued until
the owner receives a non-cloneable `PowerReadyPermit` from the sequencer for
the same active power generation and the discovered server is in
`ReadyToRegister`. The runtime wrapper `IcnssPowerServices` provides an
explicit place to acquire the source-ordered power votes when that server
appears and restore them when the endpoint disappears. The SERVREG-only
diagnostic configuration retains its previous behavior. Regression tests
verify that the production client stays `ReadyToRegister` and refuses a
request until given a permit from the matching generation.

The production image builds, the pinned launch-path validator passes, and the
complete `platforms/sunfish/test-modem-qmi.py` suite passes, including host
integration and AArch64 no-std compilation. This closes the software ordering
gap in the WLFW client; it does not provide a live power backend. No production
call site instantiates `attach_with_icnss_power`, no trusted RPMh broker/profile
provider can mint the permit, and no WLAN image launcher or WLFW server is
established. The phone was left in LineageOS and no device state was changed.

The runtime seam now has a generic `BrokeredIcnssPowerBackend` that wraps the
existing `BrokeredWlanIo`: it resolves the five source-derived VRM resources,
starts the ICNSS sequencer through the per-resource RPMh brokers, returns a
generation-bound permit only after the sequencer reaches `On`, and restores
the captured votes on stop. This composes the existing protocol pieces without
creating a profile, complete shared-client ledger, RSC lease, TCS authority,
or clock authority. The pinned DTS omits the optional CXO clock name, so the
current five-rail adapter shape matches that source profile; exact active
driver/configuration and live client ledgers remain unverified.

A fresh rooted ADB read confirmed the Pixel is still boot-complete on `sunfish`
with UID 0, Wi-Fi enabled and connected to `Vanilla`, and `wlan0` carrier and
operstate both up. No module reload, Wi-Fi toggle, reboot, or partition access
was performed. This is installed-LineageOS evidence; it does not mint the
seL4-side profile or grant RPMh/SMMU/SCM authority.

## Rooted Wi-Fi regulator on/off delta (2026-10-25)

Using the already-authorized brief Wi-Fi comparison, root ADB sampled the Linux
regulator class with Wi-Fi connected, disabled for five seconds, then re-enabled.
The saved `Vanilla` connection returned; `wlan0` carrier was `1` and operstate
was `up`. The capture issued no direct PMIC or RPMh writes. The `microvolts`
column is the regulator framework value, not a physical voltage measurement;
`num_users` counts consumers but does not identify them.

| RPMh resource | Wi-Fi on (state, µV, users) | Wi-Fi off (state, µV, users) | Restored |
| --- | --- | --- | --- |
| `ldoa9` / CX-MX | enabled, 640000, 1 | disabled, 624000, 0 | Baseline |
| `ldoc1` / 1.8-XO | enabled, 1800000, 2 | enabled, 1700000, 1 | Baseline |
| `ldoc2` / 1.3-RFA | enabled, 1304000, 2 | enabled, 1245000, 1 | Baseline |
| `ldoc10` / 3.3-CH0 | enabled, 3312000, 2 | enabled, 3200000, 1 | Baseline |
| `ldoc11` / 3.3-CH1 | enabled, 3312000, 1 | disabled, 2950000, 0 | Baseline |

Each rail shows a one-user delta correlated with the Wi-Fi transition. The
on-state user counts exactly match the captured direct DT client counts for
these RPMh resources (1/2/2/2/1). The three rails with a separate Bluetooth DT
consumer retain one user while Wi-Fi is off, at framework values matching
their captured Bluetooth request ranges.
This is a useful live check on the source-derived five-rail sequence and
shared-rail behavior. It still does not identify every requester or provide a
complete RPMh client-vote ledger. `/sys/kernel/debug` and its
`regulator_summary` were unavailable, so this capture cannot establish the
Apps RSC cache or authorize seL4 to take control. The machine-readable sample
and its limits are recorded in `platforms/sunfish/wlan-resources.json` and
checked by `validate-wlan-resources.py`.

## RPMh rail load and sleep-set source audit (2026-10-25)

A read-only rooted LineageOS check found that Android's Bluetooth framework
reported the adapter `ON`, while the `bt_power` rfkill node reported
`state=0`, `soft=1`, `hard=0`, and no `/sys/class/bluetooth/hci0` existed.
This status was captured after, rather than during, the Wi-Fi regulator delta.
It therefore does not identify the retained regulator users as Bluetooth
clients; the earlier consumer-count correlation remains a rail-level
observation only. Wi-Fi stayed up and connected during this check.

The pinned Lineage source audit now validates the complete five-rail RPMh
reduction inputs in both the captured Lineage runtime DTB and the generated
seL4 DTB. Every ICNSS `icnss_vreg_info` entry uses `load_ua=0`; the three
Bluetooth-shared regulator requests specify `1 uA`; all five regulator
children select active and sleep sets with `qcom,set=<3>`; and none sets
`regulator-system-load`. The pinned regulator core adds consumer loads and
system load before mode selection, while the DT parser only overwrites its
zero-initialized system-load field when that property exists. With the source
thresholds (`0/1 uA` on `ldoa9` and `ldoc11`, `0/10000 uA` on the shared
rails), these modeled requests select LPM mode code `4` for all five rails.

This closes the source-derived reducer profile, not the live Apps RSC cache or
per-client vote ledger. It does not authorize RPMh publication. The validator
now checks the five regulator provider/child paths, mode thresholds, set mask,
absence of system-load properties, ICNSS zero-load entries, and parser/core
source hashes. Focused power-profile tests and the full WLAN-resource
validator pass for both DTBs.

A repository-wide caller search in the pinned Lineage kernel (`28f9290a`)
found RPMh writes only in the RPMh core, `clk-rpmh.c`,
`msm_bus_fabric_rpmh.c`, and `rpmh-regulator.c`. The clock path sends clock
resource commands, and the bus path sends BCM bandwidth aggregates. The five
ICNSS LDO rails are sent by the regulator provider from each aggregated
regulator device's `addr + register-index * 4`; no separate direct writer for
those five LDO resources appeared in the pinned C/H source. This narrows the
known Linux writer path to the RPMh regulator framework. It does not prove a
complete runtime requester ledger, exclude private firmware writers, or grant
seL4 access to the Apps RSC/TCS.

The live tracefs exposes `rpmh_send_msg` and regulator enable/voltage events.
During the authorized Wi-Fi off/on comparison, `rpmh_send_msg` recorded only
the shared `apps_rsc` client label plus TCS command/header/address/data fields;
it did not identify the regulator resource or requester. The 7 KiB per-CPU
trace ring recorded 4,182 events but retained 1,224, so this sample is not
accepted as a complete rail-address map. Wi-Fi reconnected to `Vanilla` within
4 seconds (`wlan0` carrier 1, operstate up); tracing and all selected event
flags were returned to off. This trace cannot replace a command-database name
lookup or a source-derived complete RPMh client ledger.

Rooted Linux exposes no `/dev/mem` on this build, and debugfs is disabled. The
live `qcom,cmd-db` device-tree node points to an 8-byte dictionary descriptor;
the bound driver follows it to the shared command database at
`0x85f20000..0x85f21078`. There is no userspace resource-name lookup for that
table. If exact address correlation is needed, use the existing bounded,
read-only `qcom_cmd_db` parser against an authorized snapshot rather than
trying generic physical-memory access. This limitation does not affect the
broker's runtime name resolution once its command-database snapshot is
provided.

## Public ICNSS PDR and WLFW startup boundary (2026-10-25)

The pinned LineageOS Sunfish 4.14 ICNSS source now has a separate launch-path
audit. `icnss_pd_restart_enable()` performs service-location lookup; its
callback registers domain-state notifiers and sets `ICNSS_PDR_REGISTERED`, but
does not send `RESTART_PD`. `icnss_trigger_recovery()` refuses the request until
that registration and a notifier handle exist, then sends
`service_notif_pd_restart()` for the first mapped domain. Within ICNSS, the
call sites are the external-modem-down recovery hook and debug `test_mode=3`.
This is a recovery client path, not source evidence for launching an initial
UNINIT `wlan_pd`.

The same pinned kernel's `icnss_register_fw_service()` uses `qmi_add_lookup()`
for WLFW. Its new-server callback posts a server-arrival event; it does not
publish a server or load an image. Together with the existing public `pd-mapper`
and PDR client audit, this narrows the cold-start path further: AP services and
the root MPSS modem must be live before a WLFW provider can be discovered.
LineageOS also starts `vendor.per_mgr` (`pm-service`) and then `per_proxy`;
both are listed in the Sunfish extracted-vendor manifest. They are candidate
launch-path components, but the public init declarations do not show their
internal behavior, so neither can yet be attributed as the operation that
first brings up `wlan_pd`. No restart was sent to the Pixel. The updated
[`validate-wlan-launch-path.py`](../platforms/sunfish/validate-wlan-launch-path.py)
pins the LineageOS ICNSS sources separately from mainline ath10k and checks
these setup, recovery, and lookup boundaries, plus the `per_mgr`/`per_proxy`
service declarations and vendor-manifest entries.
