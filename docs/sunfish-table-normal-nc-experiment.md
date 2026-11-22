# Sunfish table Normal-NC experiment

The next cold boot tests a measured startup bottleneck, rather than adding another scheduling probe. IMG_9687 spent 662.252 ms comparing eight table pages; the original OACK was read 81.147 ms after the exact client close first became visible. The approved experiment maps only those table pages as Normal non-cacheable RAM. It retains all 4096 scalar volatile loads/comparisons, outer barriers, validation boundaries, grant ordering and fault quarantine. The screen identifies it as **WRQ TABLE NORMAL NC**.

Normal memory permits different hardware speculation/reordering/merging from Device-nGnRnE. This is an explicitly approved memory-model experiment, not proof of equivalent transient observations. No handset speedup, successful first transfer, WLAN firmware request or working Wi-Fi has yet been demonstrated with this image.

The kernel, initializer and Microkit sources are isolated under `.cache/sunfish-native-wifi/private/native-wrq-table-normal-nc-v1`. The Device baseline image/SDK remains intact. Source work is directly on `main`.

The explicit VM attribute is bit `0x08`; only ordinary RAM frames on AArch64 EL1 accept it. Cached+NC, Device-cap+NC and hypervisor NC are rejected before mapping mutation. It selects existing MAIR index 3, with Outer Shareable (`SH=2`) matching the separate noncoherent SMMU walk policy. Default uncached MMIO remains Device; the SMMU TCR is unchanged.

The eight 4 KiB table frames retain their physical addresses: `0x80003000`, `0x80004000`, `0x80005000`, `0x80006000`, `0x80008000`, `0x80009000`, `0x8000a000`, `0x8000d000`. Their initializer fill lists are empty, so they bypass temporary cached initializer mappings. Kernel object creation/clearing and cache cleaning precede runtime ownership. The kernel's cached direct-map alias still exists; the checked map path does not access table contents through it after initialization. This is not a claim that every conceivable cached alias is absent.

Rebuilt boot components shift two dynamically allocated, empty-fill protocol RAM pools by 8 KiB:

| Pool | Device baseline | Candidate | Frames |
| --- | --- | --- | --- |
| `smmu_data` | `[0x8528c000,0x852ac000)` | `[0x8528e000,0x852ae000)` | 32 |
| `ipa_receive_data` | `[0x852ac000,0x854ac000)` | `[0x852ae000,0x854ae000)` | 512 |

Their VAs, permissions, sizes and Device memory policy stay unchanged. Existing generated physical-address cells supply the new PAs to the runtime and SMMU descriptors. No other CapDL physical address changes. Both models have the same 24,533 ordered objects. Other expected differences are the NC fields, rebuilt native-PD ELF fill data and its entry address. The complete authority audit validates the actual candidate allocation against RAM/reservations and required ownership.

Verification includes 554 transport tests, 552 tests for the previous shipped observer profile, 212 link tests, 31 runtime source checks, 19 loader source-guard checks, 8 stack checks, 21 new mapping/kernel/gate tests, 4 complete-source pin tests, 154 Microkit tests, 64 Frame archive cases, 24 paired schedules/48 executions/96 modes with exact protocol I/O parity, ten fault controls and all 304 capability mutation controls. Host replay does not simulate Normal-memory reordering or phone elapsed time.

The raw XML/CapDL memory-type allowlist runs first. Only then does a canonical copy omit the already validated new field for the existing authority checker and its 280 controls; 24 additional controls target the raw experimental mapping policy. The packed initializer archive is inspected independently for all eight intended NC Frame mappings. Complete loader bytes, CapDL JSON and report must also reproduce byte-for-byte from the pinned tool/SDK/input profile. This deterministic reproduction binds the whole serialized authority; the small archive inspector alone is not an inverse audit of every capability.

The SDK builder pins complete source trees, patches, local Sunfish board modifications and external DTS/overlay inputs. The installed kernel/initializer/monitor equal the reproduced SDK; the tool equals its rebuilt binary; the relocated loader retains its separately verified source guard. The final boot envelope, compression and preserved Android boot components are audited. Private manifests bind source and artifact hashes.

An accidentally broad Python test selection also ran unrelated firmware-provenance tests: two assertions expecting older cached upstream firmware commit IDs failed. Those test files and cached provenance were not changed by this experiment. The focused NC, runtime, source-guard, stack, transport and image checks pass; this is not a claim that every repository-wide test passes.

Reproduce the SDK with `python3 platforms/sunfish/build-table-normal-nc-sdk.py`; `--prepare-only` checks source closure without rebuilding. The three `patches/*-sunfish-table-normal-nc.patch` files preserve the Sunfish-specific adaptations. The final private profile contains `CAPTURE.txt`, build scripts, replay fixture, audit logs and the image manifest. No proprietary firmware is committed.

The cold boot should answer whether table validation becomes faster, whether the original OACK is read before the close, whether DATA follows and whether WLAN/WLFW startup moves. If table time falls but those outcomes do not change, use that result to locate the remaining path; do not repeat the same memory-type experiment as another scheduling probe.

Verified image: **68,898,816 bytes**, SHA-256 `6a518644ad3261d092fcc998f86e13e0515a86f388c8d06ea7ce3f7163eabbaf`. Private profile `manifest.json` records `physical_boot_tested=false`. No phone commands have been run by the agent.

After fully powering off and entering bootloader fastboot with Volume Down + Power, the manual temporary boot command is:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-wrq-table-normal-nc-v1/fastboot.img
```

Wait for **WRQ TABLE NORMAL NC** and **RESTART WHEN DONE**, then photograph the whole settled panel. This boots RAM temporarily and does not flash the device.
