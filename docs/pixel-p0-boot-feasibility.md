# Pixel 4a P0: boot and debug feasibility checklist

Planning baseline: 28 September 2026 (refreshed **29 September 2026**). This is
the bounded **P0** study from the [roadmap](roadmap.md). It produces a written
go/no-go for a later **P1** minimal seL4/Microkit boot on the physical Pixel 4a
(`sunfish`). It does **not** flash OS partitions and does not replace the UTM
development loop.

Related notes: [Pixel key / StrongBox probe](pixel-key-feasibility.md).  
Local artifacts: [pixel-p0-assets/](pixel-p0-assets/README.md).

## Scope

| In scope for P0 | Out of scope for P0 |
| --- | --- |
| Document boot-image anatomy and AVB constraints | Packaging or flashing a seL4 image |
| Inventory recovery / rollback assets | Display, touch, UFS, modem, camera drivers |
| Confirm UART enable and serial capture path | Direct Titan M / StrongBox from seL4 |
| Map Linux DT sources to seL4 port needs | Claiming EL2 or a Linux driver-VM path works |
| Write go/no-go criteria for P1 | Daily-driver or cellular milestones |

**Hard rule until P0 is signed off:** do not run `fastboot flash`, `fastboot erase`,
`fastboot flashing unlock`, `fastboot flashing lock`, `fastboot oem disable-verification`,
or any command that writes `boot`, `vbmeta`, `dtbo`, `system`, or `userdata`. Read-only
`getvar` / `getprop` queries and a temporary Android Keystore probe are fine (already
done). Enabling UART with `fastboot oem uart enable` is a bootloader setting, not a
partition write; treat it as optional and reversible, and only after noting the
current UART state.

## Already established

| Item | 2026-09-27 probe | 2026-09-28 refresh | 2026-09-29 ADB |
| --- | --- | --- | --- |
| Product | `sunfish` | `sunfish` | `sunfish` |
| OS | Android 16 | LineageOS `23.2-20260827-NIGHTLY-sunfish` (`BP4A.251205.006`) | unchanged (`BP4A.251205.006`) |
| Fingerprint | — | `google/sunfish/sunfish:13/TQ3A.230805.001.S1/10786265:user/release-keys` | unchanged |
| Verified-boot | `orange` | `orange` | `orange` |
| Slot | `b` | `b` (`androidboot.slot_successful=yes`) | `_b` / successful |
| Bootloader | unlocked | unlocked; version `s5-0.5-10252351`; `secure: yes` | (not re-entered) |
| SoC / RAM / UFS | — | SM7150; 6 GB LPDDR4X; 128 GB UFS | — |
| StrongBox | feature true; probe removed | unchanged | — |
| Live cmdline console | — | `console=null` (UART not enabled for this boot) | `console=null`; `/dev/ttyMSM0` present |

Do not re-run unlock/lock. Do not include the device serial in project logs.

## Deliverable status

| Question | Status |
| --- | --- |
| Boot handoff / image shape | **Answered for planning:** Android boot v2 `boot.img` with LZ4 kernel + separate DTB + AVB footer |
| Early UART without flashing | **Partial:** bootloader reports UART `disabled`; enable syntax known; **no SBU cable capture yet** |
| DT → seL4 map | **Credible candidates documented** (GIC/timer/UART/memory); GENI UART not in seL4 `hardware.yml`; desk scaffold in [`platforms/sunfish/`](../platforms/sunfish/README.md) |
| Lineage return path | **Kit on disk** under `docs/pixel-p0-assets/lineage-ref/20260924/` (SHA256 verified listing); restore runbook drafted; **userdata backup still unconfirmed on host** |
| P1 go/no-go | **Conditional go** after UART cable proof + userdata backup + live slot dumps; **not ready to flash** |

---

## 1. Boot-image anatomy (`sunfish`) — results

Pixel 4a is pre-GKI. Lineage publishes standalone `boot.img`, `dtbo.img`, and
`vbmeta.img` beside the signed ZIP (the ZIP itself is an OTA `payload.bin`, not a
raw partition dump).

### Reference image studied

Host copies under `docs/pixel-p0-assets/lineage-ref/20260924/` (gitignored binaries):

| File | SHA-256 | Size |
| --- | --- | --- |
| `boot.img` | `08ac8c5a9ceb94ee37afecadee5ae06a681ea6993f17c0faa7fab8a463ee3079` | 64 MiB |
| `dtbo.img` | `73a5061d9a1b9ffd1d1543ffe96873a18192cb0739fa4c3f756607ca12313aa0` | 8 MiB |
| `vbmeta.img` | `e83aff239d7e448c38b3a27efcb9f523cc455988254cdb535c457b0d3cde6598` | 4 KiB |
| `lineage-23.2-20260924-nightly-sunfish-signed.zip` | `f459c5b600c2be693c5a053c7f1e4d402fac7e95ebe05b60c6064eca0c4ba0c7` | ~1.12 GiB |

**Note:** the handset is still on **20260827**, which has already aged off
mirrorbits. The 20260924 kit is a valid reinstall path but will **update** Lineage,
not bit-identical restore. Before any flash, also dump the live `boot_b` /
`vbmeta_b` / `dtbo_b` images to the host.

### Unpacked `boot.img` (header version 2)

| Field | Value |
| --- | --- |
| Page size | 4096 |
| Header size | 1660 |
| Kernel | 20 536 278 bytes @ load hint `0x8000`; LZ4 frame (`04224d18…`) |
| Ramdisk | 17 976 012 bytes @ `0x1000000` |
| DTB | 405 604 bytes @ `0x1f00000`; FDT magic `d00dfeed` |
| Recovery DTBO in boot | none (`recovery_dtbo_size=0`) |
| Cmdline (in image) | includes `console=ttyMSM0,115200n8 androidboot.console=ttyMSM0 …` |
| AVB | footer magic `AVBf` near end of 64 MiB sparse-sized image |
| `vbmeta.img` | `AVB0` header |
| `dtbo.img` | DTBO table, 8 entries |

Live Android cmdline currently forces `console=null` while bootloader UART is
disabled; the image still carries the MSM console string for when UART is enabled.

**P1 packaging implication:** replace kernel (and likely DTB) inside a v2 boot
image, keep page alignment, and plan for AVB/`vbmeta`. Do **not** expect
`fastboot boot Image.lz4` to work. Whether `fastboot boot <modified-boot.img>`
works without flashing remains **unproven** — assume a flash to one slot may be
required, with a pre-dump of the current slot.

Checklist:

- [x] Capture on-phone Lineage / fingerprint / bootloader IDs
- [x] Download Lineage boot/dtbo/vbmeta + signed ZIP with checksums (20260924)
- [ ] Google factory image optional second path (deferred; Lineage kit first)
- [x] Unpack reference `boot.img`; record kernel/DTB/AVB
- [x] Treat `fastboot boot` of custom images as unproven → plan for flash + backup
- [x] Do not rely on recovery-as-loader without evidence (Lineage ZIP is payload-based)

---

## 2. UART / early debug path — results

| Topic | Finding |
| --- | --- |
| Physical | USB-C SBU UART expected (~1.8 V, 115200); **adapter not verified in this session** |
| Bootloader | `fastboot oem uart` → `(bootloader) disabled` |
| Enable syntax | `oem uart [enable [loglevel]|full|disable|citadel]` (from bootloader error text) |
| Android node | `/dev/ttyMSM0` exists; mainline DT aliases `serial0 = &uart8` |
| UART hardware | `uart8: serial@a88000`, compatible `qcom,geni-debug-uart`, IRQ SPI 355 |
| seL4 drivers | `hardware.yml` has `qcom,msm-uartdm` and GICv3 / arch timer — **not** GENI debug UART |

Read-only bootloader session (2026-09-28): product `sunfish`, unlocked, slot `b`,
then `fastboot reboot` back to Android successfully. UART was **not** enabled
(no serial cable attached yet).

Checklist:

- [ ] Obtain 1.8 V USB-C SBU UART adapter; label orientation
- [x] List UART-related OEM commands / state (`disabled`; enable/full/disable/citadel)
- [ ] Optional: `oem uart enable`, capture bootloader/kernel text, then disable
- [x] Record live cmdline (`console=null` today)
- [ ] Decision after first successful serial capture

---

## 3. Recovery and rollback — results

### Assets on disk (host)

- Lineage 23.2 **20260924** ZIP + `boot.img` + `dtbo.img` + `vbmeta.img` + SHA256
- DT sources: `sm7150-google-sunfish.dts`, `sm7150.dtsi`

### On-phone vs kit mismatch

| | Version |
| --- | --- |
| Phone | `23.2-20260827-NIGHTLY-sunfish` (gone from mirrorbits) |
| Rollback kit | `23.2-20260924-NIGHTLY-sunfish` |

Reinstall will move the phone forward to 20260924 (or whatever build you sideload).

### Restore runbook (for **after** a failed P1 experiment — do not run now)

1. Back up personal data off-device before any write (photos, 2FA, exports).
2. Before first flash: dump live partitions from the active slot, e.g. via rooted
   recovery or documented `dd`/`fastboot fetch` if available for this bootloader —
   at minimum keep host copies of known-good `boot` / `vbmeta` / `dtbo`.
3. Enter recovery: power off → Volume Down + Power → Recovery.
4. Sideload the signed ZIP (`adb sideload lineage-23.2-20260924-nightly-sunfish-signed.zip`)
   per [Lineage sunfish install](https://wiki.lineageos.org/devices/sunfish/install).
5. If the boot slot is broken and recovery is unreachable, use fastboot to flash
   the matching `boot.img` / `vbmeta.img` / `dtbo.img` to the **inactive or known**
   slot only after comparing checksums; prefer recovery sideload when possible.
6. Factory image path: optional Google `sunfish` factory package as last resort
   (will replace Lineage entirely).

Checklist:

- [ ] User data backed up off-device
- [x] Lineage ZIP + boot/dtbo/vbmeta + checksums retained
- [x] Restore sequence documented
- [x] A/B awareness noted (current slot `b`)
- [x] No recovery flash / relock during P0

---

## 4. Hardware map and seL4 port breakdown — results

Sources: mainline-style DT in `pixel-p0-assets/` and DTB extracted from Lineage
`boot.img` (GIC / PSCI / arch timer present).

### Candidates for a minimal port

| Need | Candidate | Notes |
| --- | --- | --- |
| DRAM | Base `0x80000000`; size filled by bootloader; phone reports 6 GB | seL4 needs explicit size in overlay; use ~6 GiB minus reserved |
| Reserved / no-map | modem `0x8b000000`, venus/cdsp/adsp, wlan, NPU, IPA, splash `0x9c000000`, ramoops, rmtfs `0xfde01000` | Must not hand these to the kernel as free RAM |
| IRQ | GICv3 `0x17a00000` / redistributor `0x17a60000` | Matches seL4 `arm,gic-v3` |
| Timer | `arm,armv8-timer` PPIs; also mem timer `@17c20000` | Arch timer preferred for first port |
| UART | GENI debug UART `@0x00a88000` | Needs new elfloader/kernel putchar |
| CPU bring-up | `arm,psci-1.0`; CPUs `enable-method = "psci"` | Single-core first |
| Early FB (later) | simple-framebuffer `@0x9c000000` 1080×2340 | P2 territory; useful only after UART hello |

### Privilege

Still **unknown** whether ABL enters the payload at EL2 or EL1. Plan the first
port as **EL1-capable**; treat EL2 / driver-VM as a separate experiment.

### P1 engineering order (desk scaffold started; not on device)

1. Platform skeleton + overlay DTS — desk sketch in
   [`platforms/sunfish/overlay.dts`](../platforms/sunfish/overlay.dts)
2. GENI UART putchar + GICv3 + arch timer — putchar stub in
   [`platforms/sunfish/geni_uart_putchar.c`](../platforms/sunfish/geni_uart_putchar.c);
   GIC/timer still need seL4 board integration
3. Elfloader handoff under ABL  
4. Microkit UART hello (not the UTM image)  
5. Pack as boot v2 + AVB strategy  
6. On-device loop with serial  
7. Stop line if firmware blocks timer/IRQ/EL access  

Checklist:

- [x] Obtain sunfish / SM7150 DT sources  
- [x] Record DRAM, GIC, timer, UART, PSCI  
- [x] Diff against seL4 `hardware.yml` (GENI UART gap)  
- [x] Mark EL2 unknown; assume EL1 for first attempt  
- [x] Desk overlay DTS + GENI putchar stubs (not linked / not flashed)  
- [ ] Wire scaffold into an seL4/Microkit board tree (P1)  

---

## 5. StrongBox / Titan M

Unchanged: Android StrongBox works; no seL4 path demonstrated.
See [pixel-key-feasibility.md](pixel-key-feasibility.md).

---

## 6. Go / no-go for P1

| Criterion | Pass condition | Result |
| --- | --- | --- |
| Device identity | `sunfish`, unlocked, Lineage recoverable | **Pass** |
| Boot container understood | boot v2 + DTB + AVB notes; flash vs `fastboot boot` decided | **Pass** (assume flash may be required) |
| Debug channel | UART enabled; serial capture of bootloader or kernel | **Fail / pending** — need SBU adapter + enable test |
| Rollback kit | ZIP + images + checksums; data backed up | **Partial** — kit yes; **userdata backup not confirmed** |
| Hardware map | DRAM/GIC/timer/UART from DT | **Pass** |
| Privilege story | Explicit EL1 vs EL2 assumption | **Pass** (EL1-first) |
| Scope discipline | First goal UART hello | **Pass** |

**Verdict: conditional go.** Desk feasibility is good enough to *design* a sunfish
platform port. Do **not** flash a custom image until:

1. UART cable proves bootloader/kernel text after `oem uart enable`, and  
2. Personal data is backed up, and  
3. Live `boot`/`vbmeta`/`dtbo` for slot `b` are dumped to the host.

**Not a go for “run the UTM Personal OS image on the Pixel”** — that remains a
much later milestone after P1 hello.

---

## 7. Remaining P0 sessions

Ordered blockers (do these before any flash):

1. **Userdata backup (host)** — photos, 2FA, app exports off-device. No confirmed
   Pixel/`sunfish` backup tree was found on this Mac in a shallow search
   (2026-09-29). Record the path here once it exists.
2. **USB-C SBU UART adapter** — ~1.8 V, 115200; label plug orientation. Do **not**
   run `fastboot oem uart enable` until the adapter is attached and a serial
   client is listening.
3. **UART proof** — bootloader session: note current `oem uart` state → enable →
   capture bootloader and/or one Android boot log → optionally `oem uart disable`
   → `fastboot reboot`. Decision: keep enabled for P1 work or leave disabled.
4. **Live slot dumps** — before first custom flash, copy active-slot `boot` /
   `vbmeta` / `dtbo` to the host (recovery/`dd`/`fastboot fetch` as available).

Optional (not blocking P0 sign-off):

5. Google factory image as secondary restore.  
6. Continue desk-only seL4 overlay / GENI putchar under [`platforms/sunfish/`](../platforms/sunfish/README.md)
   (still no flash).

Mark P0 complete in the roadmap when (1)–(3) pass and (4) is done or explicitly
scheduled as the first step of P1.

Keep UTM / `./dev test-net` as the daily software loop. Dual-UTM Shared Network
manual `SEAL OK` remains optional and independent of Pixel.
