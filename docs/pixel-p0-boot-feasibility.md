# Pixel 4a P0: boot and debug feasibility checklist

Planning baseline: 28 September 2026 (refreshed **6 October 2026**). This is
the bounded **P0** study from the [roadmap](roadmap.md). It produces a written
go/no-go for a later **P1** minimal seL4/Microkit boot on the physical Pixel 4a
(`sunfish`). No OS partition has been flashed. Temporary `fastboot boot` is
proven and is the preferred P1 test path.

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

**P0 rule:** do not run `fastboot flash`, `fastboot erase`,
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
| Early UART without flashing | **Pass:** USB-Cereal at 1.8 V captured readable bootloader and Android kernel logs at 115200 after `fastboot oem uart enable` |
| DT → seL4 map | **Credible candidates documented** (GIC/timer/UART/memory); GENI UART not in seL4 `hardware.yml`; desk scaffold in [`platforms/sunfish/`](../platforms/sunfish/README.md) |
| Lineage return path | **Exact 20261001 boot/DTBO/vbmeta images for both slots** dumped in rooted recovery to `~/Backups/pixel-sunfish-20261006/live-slots/`, with host/device SHA-256 match; older 20260924 reinstall kit and 20260929 `/sdcard` backup also retained |
| P1 go/no-go | **Go for Pixel seL4 board port using temporary `fastboot boot`.** A boot-v2 image built from the live slot successfully booted Lineage; an AArch64 UART probe printed from EL1 and reset back to Lineage. No partition flash needed for this stage. |

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

**Current state (2026-10-06):** the handset has updated to
`23.2-20261001-NIGHTLY-sunfish` on slot `a`; slot `b` is marked unbootable by
the bootloader. The 20260924 kit is now a **downgrade**, not an exact restore.
Both slots' live `boot`, `dtbo`, and `vbmeta` partitions were dumped and
SHA-256 verified in rooted recovery. See the recovery section below.

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

Before the UART test, live Android used `console=null`. With bootloader UART
enabled, the live cmdline changed to `console=ttyMSM0,115200n8` and
`androidboot.console=ttyMSM0`; a normal Android boot produced readable kernel
and init logs on USB-Cereal.

**P1 packaging implication:** replace the kernel inside a v2 boot image, keep
page alignment, and retain a valid DTB. `fastboot boot Image.lz4` is not the
right container. `fastboot boot <modified-boot.img>` **works on this unlocked
phone**: first with the unchanged live kernel and then with the UART smoke
payload. The smoke image had no AVB footer and did not change partitions.

Checklist:

- [x] Capture on-phone Lineage / fingerprint / bootloader IDs
- [x] Download Lineage boot/dtbo/vbmeta + signed ZIP with checksums (20260924)
- [ ] Google factory image optional second path (deferred; Lineage kit first)
- [x] Unpack reference `boot.img`; record kernel/DTB/AVB
- [x] Prove temporary `fastboot boot` with live Lineage kernel, then a tiny
  UART probe; no flash needed
- [x] Do not rely on recovery-as-loader without evidence (Lineage ZIP is payload-based)

---

## 2. UART / early debug path — results

| Topic | Finding |
| --- | --- |
| Physical | USB-Cereal set to 1.8 V, directly connected to Pixel 4a; `/dev/cu.usbserial-0001`, 115200 baud, correct orientation proven by readable capture |
| Bootloader | Initially `disabled`; `fastboot oem uart enable` succeeded and a subsequent query reported `enabled` |
| Enable syntax | `oem uart [enable [loglevel]|full|disable|citadel]` (from bootloader error text) |
| Android node | `/dev/ttyMSM0` exists; mainline DT aliases `serial0 = &uart8` |
| UART hardware | `uart8: serial@a88000`, compatible `qcom,geni-debug-uart`, IRQ SPI 355 |
| seL4 drivers | `hardware.yml` has `qcom,msm-uartdm` and GICv3 / arch timer — **not** GENI debug UART |

Read-only bootloader session (2026-09-28): product `sunfish`, unlocked, slot `b`,
then `fastboot reboot` back to Android successfully. UART was **not** enabled
(no serial cable attached yet).

UART capture session (2026-10-06): with USB-Cereal at 1.8 V and a serial client
already listening, `adb reboot bootloader` reached fastboot. The bootloader
reported UART `disabled`; `fastboot oem uart enable` changed it to `enabled`.
Rebooting the bootloader produced readable early firmware/ABL text at 115200.
A normal `fastboot reboot` then produced a readable Linux 4.14 kernel and init
log, and ADB returned. The serial capture is a temporary local file outside
the repo; device identifiers and the raw log are not committed. UART remains
enabled for the next bring-up session. Opening the Mac serial device reset its
speed to 9600, so set 115200 **after** opening the capture client.

Temporary boot session (2026-10-06): a packed copy of the current slot's
kernel, ramdisk and DTB booted Lineage fully through `fastboot boot`. A second
boot-v2 image containing a 224-byte AArch64 UART probe printed a marker;
the refined probe printed `SUNFISH UART SMOKE OK`, `EL1 MMU OFF`,
`PC 0x80080000`, and `DTB 0x84500000`. It then called PSCI `SYSTEM_RESET`,
and Lineage on slot `a` booted again. This proves the bootloader's unlocked
temporary handoff, the GENI TX command sequence, the EL1/MMU-off entry, and
the loader address to use for the first seL4 image. No partition was flashed.

Checklist:

- [x] Obtain 1.8 V USB-C SBU UART adapter; orientation proven by readable output
- [x] List UART-related OEM commands / state (`disabled`; enable/full/disable/citadel)
- [x] `oem uart enable`, capture bootloader and kernel text; kept enabled for P1
- [x] Record live cmdline (`console=ttyMSM0,115200n8` after enabling UART)
- [x] Decision after first successful serial capture: proceed with P1 desk port

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

That table is the 2026-09-29 snapshot. The current phone is on
`23.2-20261001-NIGHTLY-sunfish` in slot `a`, so the 20260924 kit would now
be a downgrade.

### Exact current boot-partition backup (2026-10-06)

Rooted Lineage recovery exposed the six relevant block devices. Host copies
are at `~/Backups/pixel-sunfish-20261006/live-slots/` (`boot_a.img`,
`boot_b.img`, `dtbo_a.img`, `dtbo_b.img`, `vbmeta_a.img`, `vbmeta_b.img`, and
`SHA256SUMS`). All sizes exactly match the partitions (64 MiB / 8 MiB / 64 KiB)
and each host SHA-256 matches `sha256sum` of the corresponding phone block
device. The current slot is `a`; the bootloader reports slot `a` successful
and slot `b` unbootable. Do not assume switching to `b` alone restores
Lineage. These images provide an exact boot-partition rollback if a future
experiment flashes one, but they are not a backup of app-private `/data` or
the dynamic `super` partition.

### Restore runbook (for **after** a failed P1 experiment — do not run now)

1. Back up personal data off-device before any write (photos, 2FA, exports).
2. Verify the exact `20261006/live-slots/` backup hashes and active slot.
   Prefer temporary `fastboot boot` experiments, which do not write a slot.
3. Enter recovery: power off → Volume Down + Power → Recovery.
4. Sideload the signed ZIP (`adb sideload lineage-23.2-20260924-nightly-sunfish-signed.zip`)
   per [Lineage sunfish install](https://wiki.lineageos.org/devices/sunfish/install).
5. If a **future flashed** boot slot is broken and recovery is unreachable,
   restore that slot's exact saved `boot`, `vbmeta`, and `dtbo` images with
   fastboot after comparing hashes and slot identity. Prefer recovery sideload
   when possible. The current temporary-boot test does not require this.
6. Factory image path: optional Google `sunfish` factory package as last resort
   (will replace Lineage entirely).

Checklist:

- [x] Shared storage (`/sdcard`) backed up off-device — host path
  `~/Backups/pixel-sunfish-20260929/` (~172 MiB, 111 files + SHA-256;
  2026-09-29). Does **not** include app-private `/data` (no root).
- [x] App-private / 2FA risk accepted (2026-09-29): no authenticator apps;
  SimpleX Chat is a recent test install — passphrase/keystore not exported;
  user OK to recreate profile after wipe
- [x] Lineage ZIP + boot/dtbo/vbmeta + checksums retained
- [x] Exact current boot/DTBO/vbmeta images for both slots saved and SHA-256
  checked against recovery block devices
- [x] Restore sequence documented
- [x] A/B awareness noted (current slot `a`, slot `b` marked unbootable)
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
| UART | GENI debug UART `@0x00a88000` | Polling TX command proven in temporary boot; needs elfloader/kernel integration |
| CPU bring-up | `arm,psci-1.0`; CPUs `enable-method = "psci"` | Single-core first |
| Early FB (later) | simple-framebuffer `@0x9c000000` 1080×2340 | P2 territory; useful only after UART hello |

### Privilege

The temporary AArch64 probe printed `EL1 MMU OFF`: ABL entered the payload at
EL1 with the MMU disabled. Treat EL2 / driver-VM as a separate experiment.

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
- [x] Confirm EL1, MMU off, through temporary UART payload
- [x] Prove AArch64 boot-v2 handoff and UART output without flashing
- [x] Build an seL4/Microkit debug board from the live Lineage DT
- [x] Confirm Microkit UART hello from a temporary boot
- [x] Confirm controlled return to Lineage after Microkit boots
- [ ] Bring up Pixel display and touch for Slint UI

---

## 5. StrongBox / Titan M

Unchanged: Android StrongBox works; no seL4 path demonstrated.
See [pixel-key-feasibility.md](pixel-key-feasibility.md).

---

## 6. Go / no-go for P1

| Criterion | Pass condition | Result |
| --- | --- | --- |
| Device identity | `sunfish`, unlocked, Lineage recoverable | **Pass** |
| Boot container understood | boot v2 + DTB + AVB notes; flash vs `fastboot boot` decided | **Pass** — temporary boot works |
| Debug channel | UART enabled; serial capture of bootloader or kernel | **Pass** — USB-Cereal captured both |
| Rollback kit | ZIP + images + checksums; data backed up | **Pass for temporary boot** — exact boot images, older reinstall kit and `/sdcard` backup; app-private data not imaged |
| Hardware map | DRAM/GIC/timer/UART from DT | **Pass** |
| Privilege story | Explicit EL1 vs EL2 assumption | **Pass** — observed EL1, MMU off |
| Scope discipline | First goal UART hello | **Pass** |

**Verdict: go for the P1 seL4/Microkit board port and temporary UART-hello
image.** Boot handoff, debug UART, entry level, and exact boot partition
backups are proven. There is no reason to flash a partition for the first
seL4 iterations. Keep the current `a` slot untouched while bringing up the
kernel and Microkit console via `fastboot boot`.

**Not a go for “run the UTM Personal OS image on the Pixel”** — that remains a
much later milestone after P1 hello.

---

## 7. P0 completion and P1 handoff

Completed prerequisites:

1. **Userdata backup (host)** — **done for this device:** shared storage at
   `~/Backups/pixel-sunfish-20260929/`. App-private `/data` skipped by choice
   (no authenticator apps; SimpleX test profile accepted as disposable).
2. **USB-C SBU UART adapter** — **done 2026-10-06:** USB-Cereal at 1.8 V,
   orientation and 115200 baud verified with readable output.
3. **UART proof** — **done 2026-10-06:** bootloader and Android logs captured;
   UART left enabled for P1 work; Android rebooted normally.
4. **Live slot dumps** — **done 2026-10-06:** rooted recovery provided both
   slots' boot/DTBO/vbmeta images; host and device SHA-256 digests match.
5. **Temporary boot** — **done 2026-10-06:** original Lineage and custom UART
   probe booted with `fastboot boot`; the probe reset back to Lineage.

Optional (not blocking P0 sign-off):

6. Google factory image as secondary restore.
7. Continue seL4 overlay / GENI putchar under [`platforms/sunfish/`](../platforms/sunfish/README.md)
   using temporary boots.

P0 is complete. P1 can proceed without a partition flash.

On 2026-10-06, a clean seL4 16.0 / Microkit 2.3.1 `sunfish` debug SDK
compiled against the exact current Lineage `boot_a` device tree. A Microkit
UART hello image was packaged for `fastboot boot`. The first attempt did not
return to Lineage; its loader was offset 64 bytes from its required 4 KiB
alignment, and the serial capture reopened at 9600 rather than 115200 baud.
With both defects corrected, serial showed the loader copying its regions,
enabling the MMU, entering seL4, and printing `SUNFISH MICROKIT HELLO` from
the Microkit protection domain. The subsequent user-space counter read stopped
before the reset marker. With that read removed, a second temporary boot
printed `SUNFISH MICROKIT RESET REQUEST`, invoked PSCI, and LineageOS
23.2-20261001 returned fully on slot `a`. No partition was flashed.

The live DTBO includes an ST `fts` touch controller on I²C bus
`i2c@0xa84000`, address `0x49`; Android reports 1080×2340 touch coordinates.
The display uses Qualcomm MDSS/SDE with DSI rather than a simple `fb0` node.
A `cont_splash_region@9c000000` reservation exists. A read-only Microkit probe
found zero at all 4096 page starts in its first 16 MiB. A subsequent temporary
boot wrote black/white stripes to both candidate 1080×2340×4 frames there;
the user saw them on the Pixel. This proves that the bootloader kept scanning
the reserved memory during Microkit, without a new DSI initialization. A
display-only build of the existing eight-screen Slint shell then drew Home on
the Pixel. The user confirmed its colors and text looked good. A 45 logical
pixel top inset cleared the punch-hole camera, confirmed in a second boot.
UART marker receipt times in a repeat measured about 191 ms from the Slint
service starting to its first frame completing, excluding physical scanout.
Both tests reset to LineageOS on slot `a`; no partition was flashed.

Pixel touch is still open. The live DTBO enables ST `fts` at I²C address
`0x49` on Qualcomm GENI SE7. A read-only SE7 probe under Microkit returned
firmware revision `0x303` (I²C protocol 3) and master clock `0x11`, with idle
status/IRQ/FIFO zero. A follow-up read of the candidate TLMM GPIO6 mapping
stopped output at the first register and the phone restarted without the
probe's reset marker. This is not evidence of the GPIO value; verify the
mapping and access path before any further TLMM access. The Slint Home test
does not yet accept touch, and bootloader-owned display scanout is not a
permanent display-driver replacement.

Keep UTM / `./dev test-net` as the daily software loop. Dual-UTM Shared Network
manual `SEAL OK` remains optional and independent of Pixel.
