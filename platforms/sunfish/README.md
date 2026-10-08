# Pixel 4a (`sunfish`) seL4/Microkit bring-up

The UART handoff probe has booted on the Pixel. A separate seL4 16.0 /
Microkit 2.3.1 debug SDK and UART hello image now build from the exact live
Lineage device tree. The Microkit image is experimental: use **temporary
`fastboot boot` only**. Nothing from this directory should be flashed yet.

The old [`overlay.dts`](overlay.dts) is a mainline-based desk sketch. The
build uses [`sel4-overlay.dts`](sel4-overlay.dts) on the extracted live
`boot_a` DTB instead. See the measured addresses and backup notes in
[`docs/pixel-p0-boot-feasibility.md`](../../docs/pixel-p0-boot-feasibility.md).

## Contents

| File | Role |
| --- | --- |
| [`sel4-overlay.dts`](sel4-overlay.dts) | Overlay for the live Lineage DT, including observed memory, timer, GICv3, and UART |
| [`sel4-config.cmake`](sel4-config.cmake), [`sel4-geni-uart.c`](sel4-geni-uart.c), [`sel4-constants.h`](sel4-constants.h) | seL4 board, GENI console, and CPU constants |
| [`prepare-microkit-sources.py`](prepare-microkit-sources.py) | Idempotently installs board glue in disposable upstream worktrees under `.cache/` |
| [`build-microkit-sdk.sh`](build-microkit-sdk.sh) | Builds the debug board SDK with LLVM/LLD |
| [`uart-smoke.S`](uart-smoke.S) | Linux Image-shaped AArch64 UART/EL probe with PSCI reset |
| [`repack-boot-v2.py`](repack-boot-v2.py) | Temporary boot-v2 packer using the exact live Lineage ramdisk and DTB |
| [`build-uart-smoke.sh`](build-uart-smoke.sh) | Build the probe and boot-v2 image in `.cache/pixel-sunfish/` |
| [`hello.c`](hello.c), [`hello.system`](hello.system) | First Microkit UART marker and PSCI reset test |
| [`wrap-microkit-image.py`](wrap-microkit-image.py), [`build-microkit-hello.sh`](build-microkit-hello.sh) | Page-aligned loader wrapper and temporary boot image |
| [`capture-uart.py`](capture-uart.py) | 115200-baud capture holding one open serial descriptor |
| [`splash-probe.c`](splash-probe.c), [`splash-probe.system`](splash-probe.system), [`build-splash-probe.sh`](build-splash-probe.sh) | Read-only probe of the reserved continuous-splash memory |
| [`splash-pattern.c`](splash-pattern.c), [`splash-pattern.system`](splash-pattern.system), [`build-splash-pattern.sh`](build-splash-pattern.sh) | Temporary two-buffer black/white scanout test |
| [`slint-display.system`](slint-display.system), [`build-slint-display.sh`](build-slint-display.sh) | Eight-screen Slint shell, touch service, and isolated volatile Notes service/vault |
| [`touch-mmio-probe.c`](touch-mmio-probe.c), [`build-touch-mmio-probe.sh`](build-touch-mmio-probe.sh) | Read-only GENI SE7 touch-bus register probe |
| [`touch-i2c-ping.c`](touch-i2c-ping.c), [`build-touch-i2c-ping.sh`](build-touch-i2c-ping.sh) | Bounded address-only touch I²C probe; temporary boot only |
| [`foundation-probe.system`](foundation-probe.system), [`build-foundation-probe.sh`](build-foundation-probe.sh) | Bounded scheduler-budget and unmapped-memory isolation probe; temporary boot image |
| [`ipc-probe.system`](ipc-probe.system), [`build-ipc-probe.sh`](build-ipc-probe.sh) | Protected-call and missing-capability probe; temporary boot image |
| [`policy-probe.system`](policy-probe.system), [`build-policy-probe.sh`](build-policy-probe.sh) | Per-channel read/write authorization probe; temporary boot image |

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
5. The seL4 overlay uses ABL's contiguous DRAM entries and retains all
   reserved-memory nodes from the current Lineage DT. Recheck these against
   the live slot after an update.

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
