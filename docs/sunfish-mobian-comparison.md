# Sunfish Linux comparison for native seL4 WLAN

Native implementation progress: [Sunfish native Wi-Fi startup](sunfish-native-wifi-startup.md).

The destination is a native seL4 host. The Linux comparison is a temporary
measurement environment for reproducing the community port's host-side modem
and WLAN startup. It is not a LineageOS base, a Linux driver domain in the
final design, or an attempt to replace Qualcomm's resident secure firmware.

The intended final stack is seL4, native host services/drivers and the existing
Qualcomm firmware. The temporary comparison stack is Linux, a small Debian
userspace and that firmware. A successful Linux comparison does not establish
that the equivalent seL4 path already works.

## Pinned inputs

The machine-readable input is
[`mobian-comparison.json`](../platforms/sunfish/mobian-comparison.json).

| Input | Revision / identity |
| --- | --- |
| Mobian integration | `miromraz/mobian-pixel4a-port` at `789679f25f859bd741d690fe221d1cc68a23444e` |
| Kernel | `miromraz/linux` at `2267e248204e7e051e0f7355298949a6440de579` |
| Kernel version | `Makefile` says **7.1.0**; `sunfish-venus-v7.2` is a branch label |
| Original configuration | `kernel-config-7.1.0-rc3-sm7150.txt`, SHA-256 `f368f5c1c94ad8fdea43131a1f3475af8a268d3b0211ff51828c73ff24b45fdb` |
| Kernel source archive | SHA-256 `d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2` |
| Sunfish DT | The board DTS and SM7150 DTSI at the same kernel commit |
| Boot environment reported by the port | Unlocked vendor ABL → U-Boot → systemd-boot → EFI/Linux → initramfs/Mobian |
| Peripheral firmware metadata | Sunfish firmware repository `fe63598…`, board-data repository `792c716…`; exact file hashes in the manifest |

The kernel configuration stays intact as the input to `olddefconfig`. Build
source-revision and toolchain feature resolution is recorded in `config.delta` and the final
configuration is hashed. A changed critical WLAN, remoteproc, SMMU, QRTR or
USB option stops the build. This is not a claim of a bit-identical rebuild of
the author's binary. The supplied configuration records GCC 16.1.0, whereas
the initial container toolchain is GCC 12.2; any resolved differences must be
reviewed before choosing a device comparison.

The old `.cache/sunfish-linux-mainline-ref` / `fd78d179…` object build is a
separate reference. Its artifacts must not be presented as this exact Mobian
kernel or mixed with modules from this build.

## Startup dependency map

These are host-side implementation facts. Secure-world authentication, reset
and access decisions remain resident-firmware operations.

| Stage | Pinned reference and observable checkpoint | Native seL4 counterpart / remaining integration |
| --- | --- | --- |
| Boot handoff | Preserve the vendor secure environment; record EL, hypervisor observations, SMEM, reset procedure and MPSS state before a daemon can start it | seL4 boot composition and actual secure-call access must be measured under its own handoff |
| AP prerequisites | `work/patch.sh` adds `softdep qcom_q6v5_pas pre: ipa qcom_pd_mapper`; service units order QRTR, PD mapper and TQFTP ahead of RMTFS/WLAN | Existing IPA, SERVREG and QRTR models need real providers and production composition; source order is not proof of provider readiness |
| Remote storage | Reference overrides Debian RMTFS with `rmtfs -P -s`; `-s` can start MPSS before the Wi-Fi script | [`qcom_modem_services.rs`](../rust/src/qcom_modem_services.rs), [`qcom_modem_storage.rs`](../rust/src/qcom_modem_storage.rs), pools and storage backend need a selected live provider |
| MPSS startup | `qcom,sm7150-mpss-pas` selects `mpss_resource_init`, PAS ID 4, `auto_boot=false`; `qcom_pas_start()` prepares power/clock state, loads MDT, invokes PAS authentication/reset and waits for start indication | Existing MDT/PAS/lifecycle models are references; they need the trusted power/SMC/memory backend under seL4 |
| Remote firmware requests | Modem-side loading requests `wlanmdsp.mbn` through TQFTP; record actual request names, results and byte completion | [`qcom_tftp.rs`](../rust/src/qcom_tftp.rs) and [`qcom_qrtr_tftp.rs`](../rust/src/qcom_qrtr_tftp.rs) provide bounded protocol models; a selected firmware source and providers for any observed storage/heap requests are still needed |
| WLFW discovery | `wcn-wifi-bringup.sh` waits for MPSS `running` and the WLFW QRTR endpoint before probing ath10k once | Discovery is a checkpoint, not a launch command; static SERVREG data alone does not satisfy it |
| WLAN control | `ath10k/qmi.c` registers indications/capabilities, negotiates MSA, assigns firmware-returned regions through SCM, downloads board data, reports calibration and handles FW_READY | Existing [`qcom_wlfw_client.rs`](../rust/src/qcom_wlfw_client.rs) includes distinct profiles; the downstream ICNSS/QCACLD profile must not be silently treated as mainline ath10k |
| CE/SMMU/IRQ | Mainline DT has SID/mask `0x240/1` and CE SPIs 414–425 level-high; ath10k requests IRQs with `IRQF_NO_AUTOEN`, preserving the DT trigger | Dedicated native DMA backing, mappings, ownership and IRQ capabilities still need production integration; a software-selected context bank is not runtime evidence |
| Network link | PHY/netdev is followed by scan, association and sustained bidirectional traffic, with fault/interrupt observations | Existing CE/HTC/HTT/802.11 codecs and sDDF endpoints need their production event and link-control path |

The kernel's in-kernel PD mapper and the separately launched userspace mapper
must be identified at runtime. The recipe also retains an IPA late-load unit;
the new soft dependency may make it a no-op. Neither service names nor module
load success prove that their protocol endpoints were ready before MPSS.

## Differences that must stay explicit

* **Persistent modem storage:** `work/patch.sh` removes `-r`. The reference can
  persist modem EFS changes. The comparison's storage mode is **not selected**
  in the manifest. Keeping `-r` would use the RMTFS RAM shadow and be a declared
  deviation; its result could not establish persistence across boots.
* **IRQ triggering:** the selected mainline path uses DT level-high CE IRQs.
  The earlier downstream QCACLD caller explicitly requested rising edge.
* **SMMU policy:** the supplied mainline configuration selects strict DMA
  translation, and its Wi-Fi DT node has no downstream S1-bypass property.
  Record the actual runtime domain and mappings; configuration alone is not
  a readback of hardware state.
* **MSA teardown:** this mainline QMI implementation returns regions to HLOS
  with **RW** permissions. The inspected downstream ICNSS teardown requests
  **RWX**. These are different implementations.
* **Firmware:** the 60-byte `firmware-5.bin` is API metadata. The substantive
  WLAN executable is `wlanmdsp.mbn`. The generic upstream WCN3990 bundle is
  not an established substitute for the Sunfish set. Track the particular
  files' provenance and applicable terms before loading them.

## Cold-start comparison record

Logging must begin before RMTFS, the PAS module or the Wi-Fi script can start
MPSS. A script that accepts an already-running modem cannot alone prove a
cold start.

1. Record the exact power-off/reset procedure, boot-firmware identifiers,
   starting MPSS state, kernel command line, kernel/config/DTB/module hashes,
   and every service binary hash and argument list.
2. Record the PAS result and start indication, modem crash/fatal state, and
   first GLINK/QRTR observations. Retain failures and timestamps.
3. Record firmware-file requests, selected paths/hashes, completion or failure,
   WLFW discovery, MSA/SCM results and WLAN firmware readiness. Existing log
   visibility must be audited before claiming a missing message.
4. Require a PHY/netdev, scan, association and sustained transmit/receive
   traffic. Record interface/CE interrupt deltas and SMMU faults. A PHY or
   interface by itself is not a data-path pass.
5. Compare the first divergent checkpoint with the seL4 path. Preserve known
   differences in EL, secure-call routing, resident hypervisor, SMEM and
   bootloader-established mappings.

This first reference run should reduce userspace, not remove driver options
or host services before establishing a successful baseline. A userspace
dependency can be removed later as a separate measured experiment.

## Offline build

Run:

```sh
python3 platforms/sunfish/audit-mobian-comparison.py --fetch \
  --output .cache/sunfish-mobian-comparison/audit.json
sh platforms/sunfish/build-mobian-reference.sh
```

The builder runs in Podman, with a private sparse ext4 build image so Linux
case distinctions, links and device nodes survive the macOS filesystem share.
It builds the original configuration's `Image`, `vmlinuz.efi`, Sunfish DTB and
module set; source, configuration and toolchain records accompany the output.
The ext4 image and all artifacts stay under
`.cache/sunfish-mobian-comparison/` and are excluded from the repository.

This build contains no peripheral firmware. A minimal userspace, selected
firmware inputs and audited temporary boot packaging are separate artifacts.
The published full Mobian install modifies boot and userdata; the reference
source audit does not require that install. Do not assume ABL can directly
boot the generated mainline `Image`, or that a temporary U-Boot start proves
the same firmware handoff as the author's installed chain.

The userspace builder is:

```sh
sh platforms/sunfish/build-mobian-userspace.sh --prepare
# After the kernel artifacts are complete:
sh platforms/sunfish/build-mobian-userspace.sh
```

It pins Debian 13's package snapshot to `20261025T000000Z` and verifies that
snapshot's Release hash. The staging initramfs contains the matching modules,
QRTR/RMTFS/TQFTP/PD-mapper tools, wireless diagnostics and USB networking/SSH,
with no desktop or peripheral firmware. It boots into a dedicated staging
target, records starting state in RAM and holds the radio-service units.
This verifies the boot and capture environment before a selected cold-start
experiment. Those deliberately held services mean a staging boot is not a
reproduction of WLAN startup. The community units and script are preserved
under `/usr/share/sunfish-reference` for the later configured launch.

SSH uses a task-specific key under the ignored cache directory; only its
public key is included in the image. USB networking uses `172.16.42.1/24` and the link-local IPv6 address
`fe80::42:1` (with the Mac USB interface scope). IPv6 SSH avoids needing a
privileged host-side IPv4 configuration. USB ACM also carries the kernel console.

The actual kernel and initramfs can be smoke-tested independently of the
phone boot chain:

```sh
python3 tests/sunfish_mobian_stage0_qemu_smoke.py
```

This checks that systemd reaches the staging target and captures its initial
state, with the reference radio-service units masked and EFS policy unselected.
QEMU's `virt` machine has none of Sunfish's modem/WLAN hardware or resident
secure firmware; this test cannot establish radio startup or native seL4 access.

The separate temporary transport candidate is built with:

```sh
sh platforms/sunfish/build-mobian-boot-stage0.sh
```

It uses the pinned Sunfish U-Boot revision, an Android v0 boot wrapper and a
64 MiB GPT/FAT RAM disk. U-Boot obtains the RAM-disk address from ABL's FDT,
validates its start/end and 64 MiB extent, reserves it through LMB before
board payload allocations, maps it as a block device and invokes the EFI boot
manager. The task-local `ram-esp-reserve.c` hook is an explicit change to the
pinned U-Boot; the source archive itself stays unchanged. LMB checks the RAM
range against existing allocations and DT reservations and notifies EFI.
The blkmap `preserve` flag alone does not reserve LMB memory. The RAM disk
contains systemd-boot, the kernel EFI executable, matching DTB and XZ initramfs.
XZ uses CRC32 to match the kernel's decoder. Fastboot flashing/OEM command
execution, EFI capsule updates and EFI file-variable storage are disabled in
the comparison U-Boot configuration.

This RAM-disk transport is an adaptation of the installed reference chain.
Its v0 wrapper was rejected by this ABL's temporary-boot path. The separate v2
handoff experiment below is the transport that subsequently booted on the phone.
The port's full installation is not required.

```sh
python3 platforms/sunfish/pack-mobian-stage0-v2.py /path/to/known-working-sunfish-boot-v2.img
```

This uses the existing temporary-v2 packer, preserves the vendor DTB and original
header fields, and compresses `u-boot-nodtb.bin + U-Boot control DTB` together.
That concatenation follows the pinned Makefile's `OF_SEPARATE` `u-boot.bin`
layout. ABL receives its vendor DTB while U-Boot selects its internal mainline
control DTB and reads ABL's memory/initrd handoff. This is an explicit difference
from the community's installed v0 chain. Both payloads were independently
unpacked with AOSP's parser before testing.

## Current result

The powered-off Linux comparison passed on 2026-10-25: modem startup, WLFW,
scanning, WPA2 association, DHCP and sustained traffic in both directions.
The run below records the evidence and the limits of this result. Native seL4
WLAN remains the implementation target and has not passed this test.

The source audit verifies all pinned hashes and ten startup/configuration
checks. The matching kernel, DTB and full module set have built successfully.
The actual kernel and staging userspace also reached their checks under QEMU
with HVF; the log is `.cache/sunfish-mobian-comparison/qemu-stage0-hvf.log`.
Artifact hashes, resolved configuration differences and package/toolchain
inventories are under `.cache/sunfish-mobian-comparison/artifacts/`.

The RAM disk passed `sgdisk --verify` and `fsck.vfat -n`. AOSP's
`android-13.0.0_r75/unpack_bootimg.py` independently extracted the wrapper
payloads and each matched its built input. The v2 packer's existing five tests
also passed.

The Pixel then booted the v2 image temporarily. The first run reached the
staging target and exposed USB ECM/ACM, but SSH failed because Debian's inherited
configuration check preceded host-key generation. Resetting and ordering the
pre-start commands fixed that startup bug. A second temporary run verified:

* `7.1.0-sm7150+` on Google Pixel 4a, with root SSH over USB IPv6;
* the active staging target and no failed systemd units;
* all CPUs starting at **EL1**, as reported by the kernel;
* ADSP, CDSP and modem host states **offline**, with reference radio units masked;
* UFS partitions visible, with the modem firmware VFAT readable through a
  read-only mount, subsequently unmounted;
* the RAM ESP retained at **0xb0000000–0xb4000000**, recorded as a volatile
  `pmem-region` in the captured Linux device tree.

The initial successful staging image was `artifacts/sunfish-linux-comparison-stage0-v2.img`
(68,055,040 bytes), SHA-256
`85acb80d3a991d329165376ac526034f8d7525bfc1a1b896fce4e10d35eb5c0a`.
That artifact path now contains the cold-hold revision described below.
Validation and limits
are in `artifacts/stage0-validation.json`. Full startup snapshots, journal,
dmesg, FDT, memory map and block-device inventory are under
`.cache/sunfish-mobian-comparison/device-runs/stage0-v2-ssh/`.

The source DT maps the observed early fault SIDs `0x800` and `0xc40` to MDSS's
`0x800/0x440` SID/mask. Display PLL errors and missing optional firmware were
also logged. These are not evidence of WLAN failure: WLAN firmware was not
staged or started. A passive TQFTP process unintentionally launched by a help
query was terminated, and the modem remained offline; that procedure detail is
recorded with the capture.

This initial run was a warm restart test and proved the temporary Linux
staging/USB/UFS path. Firmware selection, modem/WLFW startup and traffic were
still untested at that milestone; the powered-off run below completes that
Linux comparison.

## Cold-start firmware and storage preparation (2026-10-25)

The next test's private inputs are prepared under
`.cache/sunfish-mobian-comparison/wifi-preparation/`. The owner phone supplied the
modem MDT/segments from read-only `modem_a`, and the IPA/WLAN/board files from a
read-only RAM copy of active `vendor_a`. LP geometry, metadata-header and table
SHA-256 checks passed before assembling its three extents. The live source
partitions were not mounted writable. Source mounts and the RAM vendor loop
were detached after copying, and all four EFS source hashes remained unchanged.

| Runtime input | Bytes | SHA-256 |
| --- | ---: | --- |
| `modem.mbn` | 68,460,672 | `8489e6828dc12ed98e26dec15adb7102a31aafaf7314e249854c737ddba4cdc0` |
| `ipa_fws.mbn` | 29,120 | `7d61b64288248719d14a4b316e9cb83917c11a2c9753dff5475a7a2db95394dd` |
| `wlanmdsp.mbn` | 3,751,004 | `a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e` |
| Full reference `board-2.bin` | 317,016 | `051607c79e6f26c68076bcdada3a77b762aadcc368ce89eee38c2531893bbc63` |
| API-5 metadata | 60 | `fef6539e0127579536bc977be57a90d018b83f2931fedc3a8870fbe38d6c4127` |

Pinned `linux-msm/pil-squasher` commit
`3c9f8b8756ba6e4dbf9958570fd4c9aea7a70cf4` assembled the split modem/IPA files.
Independent checks verified every nonempty segment against its source and
matched the complete files' Git blob identities to the pinned Mobian firmware
tree. The selected Sunfish entry in the unmodified reference board container
was independently compared with the phone's `bdwlan-sunfish.bin`. Regulatory
database/signature inputs use the upstream-signed variant from the same verified
Debian snapshot. `wifi-input-manifest.json` records all nine runtime files,
source and tool pins, script hashes and deliberate deviations. Firmware remains
in the private ignored cache and is transferred into phone RAM after boot.

The cold-test image has SHA-256
`6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d`.
It keeps the full kernel configuration but holds automatic IPA/PAS module
loading until firmware arrives. Its kernel/initramfs passed QEMU staging again,
the Android v2 payloads were independently unpacked, and the hold configuration
was checked inside the actual compressed archive.

[`prepare-coldwifi.sh`](../platforms/sunfish/mobian-comparison/prepare-coldwifi.sh)
verifies the RAM firmware tree and binds it read-only, snapshots fresh EFS into
RAM files, sets the original EFS block devices read-only, and creates temporary
RMTFS/TFTP units. RMTFS uses `-r -v -o /run/sunfish-coldwifi/efs`, without raw
partition mode or automatic remoteproc start. Its device policy permits only
the RMTFS shared-memory character device, plus systemd's standard devices.
An isolated test of Debian 1.1's actual `storage.c` verified changed data reads
back from RAM while backing bytes remain unchanged even after sync.

[`start-coldwifi.sh`](../platforms/sunfish/mobian-comparison/start-coldwifi.sh)
checks published RMTFS/TFTP endpoints, loads PAS through the pinned IPA/PD-mapper
soft dependency, checks the modem's offline state, then
invokes the original Wi-Fi script once. Modem automatic recovery is disabled
to retain the first failure. Services do not restart automatically. The unchanged
kernel has dynamic debug but no kprobes; return-probe absence is explicitly
recorded rather than reported as a successful secure call.

This RAM-shadow storage mode is a declared difference from the community's
persistent RMTFS setup and cannot establish persistence across boots. EFS
hashes are checked again after the attempt. The user selected their existing
Wi-Fi network for the association/traffic phase.

## Powered-off Linux WLAN run (2026-10-25)

The user powered the Pixel off for at least ten seconds, returned directly to
Fastboot, and confirmed readiness. Fastboot identified serial `02281FQC200058`,
product `sunfish`, unlocked, slot `a`. The verified cold-hold image above booted
with `fastboot boot`; no partition was flashed. Linux reported all CPUs at EL1.
IPA, PAS and ath10k were initially absent. All nine firmware hashes passed after
transfer into RAM. The four raw EFS block devices were set read-only before
copying, and the firmware bind mount was read-only.

The first harness invocation stopped **before modem startup**: its guard
incorrectly required service 64 (SERVREG locator) before calling remoteproc
start. The pinned kernel's `drivers/remoteproc/qcom_common.c` creates the
`pd-mapper` auxiliary device in `pdm_notify_prepare()`, registered as the
remoteproc subdevice's `prepare` callback. That triggers
`qcom_pd_mapper.c:qcom_pdm_probe()` and publishes the locator. Requiring the
endpoint before this callback was a circular prerequisite, not a device failure.

The source script now requires the already-running RMTFS/TFTP providers before
start and records locator discovery afterward. On this boot, a separately
hashed continuation checked that the modem was still offline, recovery remained
disabled, EFS hashes matched, no bringup log/result existed, and the provider
services were active. It did not reload drivers or reset the attempt marker.
It invoked the original pinned `wcn-wifi-bringup.sh` exactly once. The kernel
log contains one modem power-up and one successful modem-up event.

| Milestone | Captured result |
| --- | --- |
| IPA prerequisite | Driver initialized and setup completed |
| MPSS | `modem.mbn` 68,460,672 bytes; modem up at about 176.8 seconds since boot |
| PD mapper | Kernel locator service 64, version 1, instance 1, host node 1 |
| WLAN firmware request | TQFTP RRQ for `/readonly/firmware/image/wlanmdsp.mbn`, requested size 3,751,004 bytes |
| WLFW | Service 69, version 1, instance 0, remote node 0, port 81 |
| ath10k | Chip `0x30224`, board `0xff`, API 5; HTT 3.96 / WMI operation version 4 |
| Interface | `phy0` / `wlan0`; original startup script returned 0 |
| Network | WPA2-PSK / CCMP on 5200 MHz; DHCP lease acquired |
| Gateway | Five replies from five pings, no packet loss |
| Pixel → Mac | 207,618,048 bytes received over 20.02 seconds: **82.95 Mbit/s** |
| Mac → Pixel | 305,528,832 bytes received over 20.00 seconds: **122.21 Mbit/s** |

Each iperf3 direction omitted a two-second warmup. The Pixel client bound its
Wi-Fi IPv4 address; its route selected `wlan0`. The Mac server bound its Wi-Fi
address, and its route selected `en0`. USB IPv6 carried SSH diagnostics. WLAN
CE1 and CE2 interrupt counts increased during traffic. No SMMU fault, remoteproc
fatal or watchdog interrupt count increased during that interval. The earlier
display context-fault count stayed at 7,213. No new SMMU fault or modem crash
message appeared after modem startup. The modem remained running with the same
Linux boot ID, and ADSP/CDSP remained offline.

The source EFS hashes matched after startup, traffic and final capture. Network
credentials were supplied through SSH standard input, held only in a private
RAM configuration, excluded from evidence archives, and removed after testing.
The Pixel was left in Linux, still associated; no Fastboot transition is needed
to inspect this working state.

Evidence is in
`.cache/sunfish-mobian-comparison/device-runs/cold-wifi-20261025T200752Z/`:
`result.json`, `evidence-sha256.json`, both iperf client/server JSON pairs,
host/live kernel journals, initial/final interrupt tables, routes, firmware and
script manifests, storage hash checks, and the guarded-continuation review.
Device RTC time is incorrect (1973); use kernel monotonic timestamps and the
recorded host UTC time when correlating events.

This is one successful cold run, with RAM-backed modem storage and a temporary
U-Boot/EFI boot chain. It does not establish persistence, repeated cold-boot
reliability, or native seL4 WLAN. The unchanged kernel lacks kprobes, so the
capture establishes successful remoteproc startup rather than individual SCM
return values. TQFTP logged rejected vendor-path probes before the canonical
firmware request; its default logging is not a byte-by-byte transfer trace.
ath10k also logged nonfatal invalid-frequency scan messages and selected a
random MAC address. These did not prevent association or sustained traffic.

The next seL4 work can target the observed order: RMTFS/TFTP readiness → IPA
prerequisites → authenticated MPSS startup with locator preparation → WLFW
arrival → one ath10k/QMI initialization → CE/WMI/HTT data path. A successful Linux
run at EL1 provides a concrete host implementation to compare against; it does
not substitute for implementing and testing seL4's power, secure-call, memory,
SMMU and interrupt backends.
