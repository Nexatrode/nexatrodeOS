# iPhone 7 Plus PongoOS probe

This is an experimental bring-up log. Later entries document subsequent
results and supersede earlier statements that a particular test is pending.
The broader findings and limitations are in
[`docs/iphone7plus-sel4-bringup.md`](../../docs/iphone7plus-sel4-bringup.md).

**2026-10-05 finding:** the phone reports `ID_AA64MMFR0_EL1=0xF0100001`:
4 KiB translation granules unsupported, 16 KiB supported. The earlier loader
and seL4 build selected 4 KiB. Earlier standalone MMU successes were false
positives: their SVC return incorrectly skipped the following instruction,
including the entire enable call in the call-frame control. Those three source
handlers are corrected; old standalone binaries are not evidence of success.
The successful terminal hardware test is
`build/iphone7plus/diagnostics/mmu-16k-transition/loader.bin`, SHA-256
`0bb5e2da00e82a4907f4f4814bed2b60de9fe1c8ab73139c19556f1de5bdf49b`.
Expected row 0: `0031364B50415353` (`16KPASS`), row 2: `0000000030D00801`.
It passed generic QEMU emulation and the phone displayed `16KPASS` with
SCTLR.M=1, the expected 16 KiB TCR and successful code/display PAR results.
This confirms the terminal loader transition on hardware.

**Experimental full 16 KiB port:** the kernel, Microkit loader/tool/monitor,
and Rust initialiser have now been updated. The smoke task draws `16K BOOT`
and `USER OK` in black and white. Generic QEMU reaches that task and verifies
its framebuffer writes. The phone photo on 2026-10-05 also shows both lines.
This confirms the first seL4/Microkit user task started on hardware.
The upload artifact is `build/iphone7plus/diagnostics/16k-boot-feb3da968639/loader.bin`
(848244 bytes, SHA-256 `feb3da9686393cb9799ac3fe8a73007e1597eb6506c2983d27d8c1e7d75cebb7`).

Rebuild the separate SDK and smoke image with `platforms/iphone7plus/build-16k-sdk.sh`.
This requires the existing prepared iPhone bring-up sources, LLVM, Rust,
QEMU, and `.cache/sel4-venv`. `apply-16k-patches.py` checks recorded before/after
source hashes; it refuses to overwrite unrecognized local edits. The three
repository-owned `*-iphone7plus-16k.patch` files are incremental over the
recorded pre-port sources. The Rust dependency is isolated under
`.cache/rust-sel4-iphone7plus16k` at base commit
`dbe6445d56059ed9a757e53c7137892aece1d179` plus its patch.

Validation: 152 Microkit tests pass; a separate default 4 KiB QEMU kernel
compiles and links; all three patches reconstruct the modified sources.
The emulator uses a 24 MHz counter and instruction-counted time. It verifies
actual smoke-task instructions and physical display pixels, avoiding false
positives from the monitor's overlapping virtual addresses. The phone result validates the startup path and display writes. It does not
validate Apple timer interrupt delivery, sustained scheduling, or broader
memory reservations.

**Timer/FIQ hardware probe (run 2026-10-05):**
`build/iphone7plus/diagnostics/16k-scheduler-ba37c2b5dbc3/loader.bin`
(SHA-256 `ba37c2b5dbc3addb4a39aae638e5df07ba3a8dc765b97de13bc98d1264d5647d`, 885740 bytes). It starts a
priority-100 spinner that never yields and a priority-99 observer. Both map
the same screen. The spinner writes `16K BOOT` / `USER OK`. Only the observer
writes `TIMER OK` on a third line, after the timer has preempted the spinner
and exhausted its 20 ms scheduling budget. Build with
`platforms/iphone7plus/build-16k-scheduler-probe.sh`. QEMU reaches the
spinner but does not run the observer with the generic virtual interrupt
controller. On the phone, Steve saw both boot lines but no `TIMER OK`, so
asynchronous timer-driven preemption has not been demonstrated. The original
proven full-boot image remains pinned separately.

**Syscall control image (run 2026-10-05):**
`build/iphone7plus/diagnostics/16k-scheduler-syscall-1abee24c4de8/loader.bin`
(SHA-256 `1abee24c4de87ea44897dd2a7db2ce8c8cf677ff3a459081dada258441d6ca0d`,
885740 bytes). This uses the same two PDs, but the spinner enters the kernel
periodically via `seL4_Yield`. The observer writes `TEST OK` if budget
accounting can schedule it without a hardware timer FIQ. QEMU verified all
three lines, and Steve saw `TEST OK` on the phone, with display bleed. This
isolates the remaining problem to asynchronous timer arming, FIQ delivery, or
FIQ entry handling. Set `IPHONE_SCHEDULER_SYSCALL=1` when running the build script.

**Timer-enable fix observed on hardware (2026-10-05):**
The Apple IRQ-controller initialization ran after `initTimer()` and wrote
`CNTV_CTL_EL0 = IMASK`, unintentionally clearing `ENABLE`. IRQ setup later
cleared `IMASK`, leaving the timer disabled. The corrected code preserves
`ENABLE` while masking. The new pinned image is
`build/iphone7plus/diagnostics/16k-timer-enable-2e6ad7b59d17/loader.bin`
(SHA-256 `2e6ad7b59d17d20aff38b2ffd7edd379e80583f06dfd57248b3384ce77f99314`,
885740 bytes). It uses the original non-yielding spinner and should show
`TIMER OK` only if the timer can preempt it. The kernel compiled and its
disassembly confirms the read/modify/write. QEMU's GIC timer wiring differs
from Apple FIQ routing and its full boot check did not pass with the timer
enabled. Steve reported the third line present on the phone; the photo shows
only its top pixels clearly. The text straddled the dark header and the white
background, explaining why it appears to bleed out. The observer source now
clears a dark rectangle under all 56 glyph rows for future builds; the pinned
image above remains unchanged. One timer-driven observer run is observed;
sustained periodic timer operation is still untested.

The original raw-entry probe checks one early boundary: whether PongoOS
`bootr` enters a position-independent AArch64 payload with the expected Apple
`boot_args` pointer in `x0`, and whether that payload can write the Pongo-provided
framebuffer after handoff.

The probe validates basic geometry and attempts to draw a 128 x 48 pixel
green/amber marker at the top-left of the screen, then waits. In the first
physical run on iPhone 7 Plus (iPhone9,2), PongoOS 2.6.3 accepted the payload
and the user observed a short bar at the top-left. Viewed straight on, it looks
pinkish above and light yellow below; at an angle, it can look like a single
red bar. This confirms raw entry and visible framebuffer writes, but the
angle-dependent appearance makes color-channel interpretation inconclusive.
The apparent dimensions also need characterization. It has no storage,
radio, network, partition, or persistent-state code. It does not prove seL4's
loader contract, interrupt support, or that a Microkit image can boot here.

Build on the development Mac with:

```sh
./platforms/iphone7plus/build-pongo-raw-probe.sh
```

Output is written under the ignored `build/iphone7plus/` directory. When
libusb-1.0 development files are available, the same command also builds a
small uploader. The first payload is the framebuffer marker. A second,
stackless state probe clears only a 128 x 96 pixel patch and renders:

- one white track for the current exception level (EL0 at top through EL3 at
  bottom);
- an MMU marker at the left if `SCTLR_EL1.M` is set, at the right if clear;
- four white squares for `DAIF.D`, `.A`, `.I`, `.F` mask bits, left to right.

A third probe sets a temporary stack at `payload_base + 0xF0000`, inside
PongoOS's guaranteed 1 MiB minimum upload buffer, exercises it with nested C
calls and a stack canary, then displays raw `boot_args` values in indexed
hexadecimal rows. The row IDs are: `0` stack test marker (`STACKOK` in ASCII
hex); `1` `virtBase`; `2` `physBase`; `3` `memSize`; `4` `topOfKernelData`;
`5` device-tree pointer; `6` device-tree length; `7` framebuffer base; `8` row
stride; `9` width; `A` height; `B` payload address; `C` temporary stack top.
The latest build adds `D` = `CNTFRQ_EL0` (timer frequency in Hz) and `E` =
`MPIDR_EL1` (CPU affinity ID). The physical run reported `D=0x016E3600`
(24,000,000 Hz) and `E=0x80000000` (Aff3:Aff0 all zero, primary affinity 0).
These are read-only system-register reads.
This stack placement is only for this small probe, not a final kernel stack.
The original on-device run displayed `STACKOK`, confirming that nested
calls and the stack canary succeeded. A live `bootargs` read through Pongo's
console on the first measured boot reported `physBase=0x8000EC000`,
`memSize=0xBB720000`, `topOfKernelData=0x8074E0000`, and
`deviceTreeLength=0x2A804`; a later boot moved `topOfKernelData` to
`0x807154000`. These values
give a candidate physical RAM interval `[0x8000EC000, 0x8BB80C000)`, before
subtracting active reservations. The prior screenshot transcription was from
an earlier run and is superseded by this console read. The framebuffer stride
is 4352 bytes at 1080 x 1920 pixels.

The first custom recursive tree-walker did not finish on hardware. The better
diagnostic is Pongo's own USB shell: build/run `pongoterm`, then issue
`dt /chosen/memory-map -8`. This exercises Pongo's validated parser directly
and prints each 16-byte range as two 64-bit values. In the 2026-10-04
iPhone9,2 session, Pongo reported all 16 `MemoryMapReserved-*` slots as
`(0, 0)`: they are unused placeholders, not active reservations. The named
entries such as `DeviceTree`, `BootArgs`, and the kernel segments contain the
actual ranges. The live output and current values are recorded in
[`docs/iphone7plus-sel4-bringup.md`](../../docs/iphone7plus-sel4-bringup.md).
Use the native command instead of treating the incomplete custom walker as
evidence.

A temporary read-only Pongo module queried every page through Pongo's own
allocator. The first boot reported `[0x8074E0000, 0x8BAD80000)`; a later boot
reported `[0x807154000, 0x8BAD80000)`. This confirms the start can move between
boots, so query the range again after uploading the actual image; Pongo can
move or resize its upload buffer.

The `pongo_freeram_probe.bin` smoke test used `0x8BAD80000` as its stack top,
checked nested stack traffic, and wrote and restored two canary words at the
first free address (`0x8074E0000`), then displayed the tested address and pass bits.
It touches only Pongo-reported free RAM and the framebuffer; it does not write
storage. A refreshed allocator query is required after any upload that changes
Pongo's allocation state. The first physical run showed RAM and stack pass
bits set, confirming access at the range's base and a working stack near its
top; it did not test every page or the seL4 loader contract.

The follow-up `pongo_ramsweep_probe.bin` checks both the first and last 64-bit
word in every 16 KiB page of the allocator-reported interval, restoring each
word after verification. It uses a temporary stack in the Pongo upload buffer
so the full candidate interval remains available to scan. Its report shows
the number of pages checked, interval bounds, any failing page, stack status,
and the temporary stack address. This is a page-access sweep, not a full
memory-pattern or seL4 loader test. On the later boot it reported `SWEEPOK`,
checked all `0x2CF0B` (184,075) pages in `[0x807154000, 0x8BAD80000)`, showed
no failing address, and passed its upload-buffer stack check. The report gave
payload base `0x8BADC0000` and stack top `0x8BAEB0000`. For each page it
tested and restored the first and last 64-bit words. This establishes sparse
read/write/restore access across every candidate 16 KiB page, not full-page
coverage or final suitability as seL4 RAM.

With the phone already at the PongoOS shell, run either payload with the same
uploader. The state probe's first physical run showed the second current-EL
track (EL1), the MMU marker on the right (MMU off), and four lit DAIF squares
(D/A/I/F masked).

For example:

```sh
./build/iphone7plus/load-pongo-raw-probe \
  ./build/iphone7plus/pongo_bootargs_probe.bin
```

This requests only PongoOS's raw `bootr` handoff; it does not use `bootl` or
write persistent storage. Do not send the existing QEMU `loader.elf` to
`bootr`: these probes are position-independent payloads and do not contain the
QEMU image. All probes wait indefinitely after drawing, so force-restart the
phone to return to iOS or PongoOS. Both the original 2,116-byte and larger-
glyph 1,756-byte boot_args/stack probes built without unresolved symbols or
relocations and ran on the phone.

## Microkit SDK and smoke image

Microkit 2.3.1 now has an `iphone7plus` board record and loader-side single-core,
no-PSCI, no-UART settings. The source changes are preserved in
[`patches/microkit-2.3.1-iphone7plus-loader.patch`](../../patches/microkit-2.3.1-iphone7plus-loader.patch).
The complete target SDK builds locally. Build the one-component no-I/O system
and inspect its report with:

```sh
./platforms/iphone7plus/build-microkit-smoke.sh
```

The ELF and report go under ignored `build/iphone7plus/smoke/`. The latest
image has one load segment at `0x810000000` and is about 2.5 MiB. A live Pongo
allocator query reported free RAM beginning at `0x8072D8000`, following earlier
observations at `0x8074E0000` and `0x807154000`. The compile fixture now starts
at `0x808000000`; its generated RAM interval is `[0x808000000, 0x8BAD80000)`,
and the smoke report places system objects between `0x808000000` and
`0x8082A9A80`. This leaves measured headroom above the changing firmware
boundary, but remains a static map that must be checked against the actual
boot's reservations. PongoOS can resize its 1 MiB default receive allocation
up to 128 MiB using a 4-byte size request; the host uploader sends that request
before bulk transfer. `bootr` executes the first byte of the received buffer
and does not parse ELF. The Microkit AArch64 loader has a position-independent
startup routine that relocates its flat binary to `0x810000000`, so the build
emits `loader.bin` from `loader.elf` for `bootr`. Its iPhone-specific startup
preserves Pongo's `boot_args` pointer across relocation and draws a green/amber
block at the framebuffer from the loader's C entry. The phone displayed those
two bands, confirming the loader reached C entry with usable display metadata.
A second, distinct block now draws after `copy_data()` to mark that the kernel
and system regions were copied. The second run displayed four bands, reported
as pink/yellow/red/yellow, confirming both loader milestones; neither marker
proves the seL4 kernel or smoke component started. The 2,611,108-byte image was
uploaded and `bootr` was issued, but its USB control request timed out during
handoff. A later PongoTerm connection also timed out.

The current image adds a temporary TTBR0 identity mapping for the Pongo
framebuffer window as Device-nGnRnE. The mapping is installed only if its L1
slot is empty and the complete display buffer fits in the 1 GiB block. On the
third run the user saw a fifth bar at the expected pre-MMU marker position;
its white half blends into the screen, which is evidence that the mapping was
installed. On the fourth run the user saw seven bars ending in pink. That pink
is the checkpoint immediately before `arch_mmu_enable()`; no return marker was
reported. The next image adds nine checkpoints from helper entry through EL
setup, cache/MMU disable, page-table/TLB setup, SCTLR.M, and both return
boundaries. They are 56 rows apart starting at row offset 224. This is a
loader diagnostic, not the final seL4 memory map. The latest build also maps
the loader's executable identity window as Normal non-cacheable instead of
Device memory. Arm allows some implementations to fault on instruction fetch
from a Device mapping, making this a targeted MMU-transition hypothesis test.
That run reproduced the same checkpoint sequence. The newest build records
SCTLR.WXN in the pre-MMU marker and clears WXN for the temporary loader mapping
before enabling translation; this tests a second permission-related cause.
The user reported no change. The current image adds a visible exception-handler
marker at row offset 784, with different colors for instruction abort, data
abort, and other synchronous exceptions. Build it with
`./platforms/iphone7plus/build-microkit-smoke.sh`; it is ready at
`build/iphone7plus/smoke/loader.bin`.
The user reported the same 19 visible stripes ending in red, without the new
exception marker. The latest image preserves that diagnostic and enables only
SCTLR.M, leaving both caches disabled; it tests whether translation succeeds
independently of cache activation. That run also stopped at the same point. The
newest image changes the loader identity mapping from a 2 MiB L2 block to one
1 GiB L1 block, retaining the MMU-only enable and exception marker. It is ready
at `build/iphone7plus/smoke/loader.bin`. That run also showed no new stripes.
The current image changes the loader mapping to cacheable Normal memory while
leaving SCTLR.C and SCTLR.I disabled; it isolates the memory attribute from
cache activation. The user reported no visible change. The newest image writes
`SCTLR_EL1 = 0x30d00801` (seL4's RES1 baseline plus M) rather than preserving
Pongo's other SCTLR controls. It is ready at
`build/iphone7plus/smoke/loader.bin`. The user still saw the same colors,
ending in red. The current image sets both TCR_EL1 table-walk shareability
fields to inner-shareable, alongside the clean SCTLR baseline and 1 GiB loader
mapping.
The user reported the same colors, ending in red, so the exception-vector
marker did not appear. The next image uses a 39-bit low virtual-address range
with TTBR0 rooted directly at L1. It removes the L0 walk while covering the
phone's RAM and framebuffer. The user again reported no change. A standalone
Pongo MMU probe is now available as `build/iphone7plus/pongo_mmu_probe.bin`.
It avoids Microkit, the C runtime, and the stack, and builds one L1 identity
table at runtime. Its bands indicate payload entry, page-table setup, MMU-on
execution, or entry into its own exception vector. Upload it with:

```sh
./build/iphone7plus/load-pongo-raw-probe ./build/iphone7plus/pongo_mmu_probe.bin
```

The first run showed the entry and table-ready markers, with no MMU-on marker.
A second run added an intentional SVC before enabling translation. The user
saw the entry, table-ready, MMU-on, and red SVC markers in order. This confirms
that this probe can complete an EL1 MMU transition. A closer address comparison
found a mismatch: Pongo's receive buffer (`0x8BADC0000`) and framebuffer
(`0x8BB80C000`) share L1 index 34, so the probe's framebuffer Device mapping
overwrites its Normal code mapping. Microkit relocates to `0x810000000` (index
32), maps that block as Normal/AttrIdx4, and maps the framebuffer separately as
Device at index 34. The first Microkit test with TTBR1 walks disabled still
ended in red, so EPD1 alone did not visibly change the failure. A relocated
standalone control now matches Microkit's loader address, TCR/MAIR fields, and
split code/framebuffer blocks. It builds as
`build/iphone7plus/pongo_mmu_relocated_probe.bin`; violet marks successful
relocation, followed by table-ready, MMU-on, and SVC-return markers. The user
reported pink, yellow, light yellow, pink, red in that order (the relocation
marker rendered yellow), confirming the entire relocated MMU test. This rules
out the broad TCR/MAIR/split-block configuration as the cause. The next
Microkit image rebuilds its lower L1 table at runtime after `copy_data()` and
adds just the loader/kernel Normal block and display Device block. A violet
row-168 checkpoint identifies that image immediately before MMU setup. The
updated `build/iphone7plus/smoke/loader.bin` builds and passes `git diff
--check`. The user ran it and reported the fourth band (row 168) as yellow,
followed by the familiar stripes ending in red. That fourth band confirms the
runtime table rebuild reached the pre-MMU handoff; the run still showed no
later MMU-on success marker. Rebuilding the table at runtime did not resolve
the failure. Next isolate Microkit's stack frames, cache-maintenance calls, and
MMU enable/return sequence inside the known-working relocated raw probe. That
control is available as `build/iphone7plus/pongo_mmu_callframe_probe.bin`. It
relocates to the Microkit address, uses a stack in the Normal block, performs
Microkit-style set/way D-cache clean and I-cache invalidation, sets matching
MAIR/TCR/TTBR values, enables M, then tests stack restore and return. The
additional green marker at row 280 means the saved frame and return succeeded.
The user ran it and reported the sequence through red at row 224 and pink at
row 280. The row-280 marker after the red SVC control confirms the EL1 routine
restored its saved registers and returned to the caller. The immediate post-MMU
marker at row 168 was described as two dark/black rows, so its color is
ambiguous; the later return marker nevertheless proves execution passed that
point. The standalone probe therefore validates the relocated mapping,
Microkit-style cache operations, SVC handling, MMU-on code path, and stack
return. The next Microkit build gives the actual loader's post-SCTLR.M,
assembly-return, and C-wrapper-return checkpoints distinguishable colors.

For that loader diagnostic, the nine 48-row bands begin at framebuffer row
224 and are spaced 56 rows apart. From top to bottom they mark entry to
`arch_mmu_enable()`, completion of EL setup, selection of the EL routine, EL1
assembly entry, completion of cache/MMU-off setup, completion of table/TLB
setup, execution after setting SCTLR.M, return from the EL1 assembly routine,
and return from the C architecture wrapper. The revised final three
checkpoints use one, two, and three yellow vertical bars on dark backgrounds.
The assembly writes the one-bar marker directly after SCTLR.M at the first
checkpoint row, before the C marker redraws it. The user's later report showed
no distinguishable one/two/three-bar pattern and no bands below the familiar
red stripes. The three red-looking boxes were prepainted placeholders, so they
do not prove that SCTLR.M was enabled or that the loader returned. In the
latest run, the user saw four red-looking boxes. That count is ambiguous because
the new image prepainted a fourth placeholder. The user confirmed the raw transfer completed, then `bootr` timed out as the
USB device disconnected. On the next screen they reported five bands after the
familiar red: white, pink, yellow, white, pink. Their spacing aligns with rows
560/616/672/728/784. The `cfa5…` run was not a distinct visual diagnostic: it cleared rows
560/616/672/728, then repainted them as pre-MMU placeholders and reused them
for the post-MMU marker. That repaint is removed. The subsequent `44f281…`
payload moved a cyan marker to row 560 directly after SCTLR.M, but the user saw
no band there. An earlier standalone control also failed to show its immediate
post-MMU write despite showing a later marker after the MMU routine returned,
so absence at that first store is inconclusive. The current payload instead
writes a solid magenta band at row 560 only after `el1_mmu_enable()` returns.
The user ran that C-based marker and reported no change. The current diagnostic
uses a register-only assembly writer at that same post-return point, matching
the successful standalone control. Payload SHA-256:
`6ad81c6f89ccb14042226ce834868c0ebd6ff4b864f3a9b3deca9525eac02270`. The
user verified this hash and again saw no change. Therefore the production
loader's post-return marker is still not visible; given the standalone control's
known invisible immediate store, treat the framebuffer trace as inconclusive
rather than repeating color or stripe uploads. The remaining comparison is the
production loader's C/boot state and call context versus the standalone control.

To compare the loader's nested MMU-disable call and page-table root addresses,
upload `./build/iphone7plus/pongo_mmu_nested_probe.bin` instead. This variant
follows the real `el1_mmu_enable() -> el1_mmu_disable()` stack/call/return
sequence, sets TTBR0 to the loader's `boot_lvl1_lower` address (`0x810009000`)
and TTBR1 to `boot_lvl0_upper` (`0x810005000`), then uses the same two known-
working identity mappings and visible markers. The user has run the nested-call
version with both the table inside the probe and the loader's exact root
addresses; both showed the later return marker. The nested probe's exact
post-MMU marker and a standalone solid-red framebuffer-store control produced
no visible change, so standalone variants were retired. The latest payload places one solid magenta band at row 560 after
`el1_mmu_enable()` returns. This follows the row-504 pre-MMU checkpoint; later
loader milestones remain at rows 784, 840, 896, 952, and 1008.

## seL4 source milestone

A timer-only FIQ kernel prototype against the pinned seL4 16.0.0 sources now
compiles and links to an AArch64 ELF. The patch is at
[`patches/sel4-16.0-iphone7plus-timer-fiq.patch`](../../patches/sel4-16.0-iphone7plus-timer-fiq.patch),
and build notes are in
[`docs/iphone7plus-sel4-bringup.md`](../../docs/iphone7plus-sel4-bringup.md).
It has not been loaded on the phone. The compile fixture now links at
`0x808000000`, above all allocator starts recorded across measured boots. Its
static free-memory table still needs comparison with live firmware and Pongo
reservations before boot. Runtime reservation discovery and A10
translation/cache validation remain open.

The diagnostic build at `build/iphone7plus/smoke/loader.bin` omits only the
checkpoint-drawing calls at MMU stages 4, 5, and 6. It leaves MMU setup and
return unchanged, so this only shortens the screen trace; it does not isolate
runtime behavior. Build it with
`./platforms/iphone7plus/build-mmu-stage-checkpoint-omission.sh`. The user ran the payload
twice with SHA-256
`42ec1cd6f1c3a9ed4b48fb7a531a62182249ba0212fb42983113c7a2e44a7f92` and saw the
shorter trace, but no later progress marker. Do not repeat this payload. The
compiled C wrapper was also checked: it calls `ensure_correct_el()`, reads
`CurrentEL`, then branches directly to `el1_mmu_enable()` for EL1. The prior
state probe showed the phone starts at EL1, so there is no EL transition on this
path. No source-level error has been established.

The next candidate overwrites the already-visible stage-2 band at row 280 with
solid black only after the real EL1 MMU routine returns. This tests for a
positive return signal on a known-visible band, instead of relying on a new
stripe at row 560. Build it with
`./platforms/iphone7plus/build-mmu-return-overlay.sh`. Current payload SHA-256:
`017ad12d5515e34577ac8a32944c86cd0f8f19e31418cc2610b2bf18895f7994`. The build
passed locally. The user later confirmed this hash and sent a photo with the
same trace; row 280 did not appear black. That is evidence that stage 8 was not
reached, though the display path still prevents treating it as conclusive. Do
not repeat this payload.

The follow-up result payload keeps the black return overlay at row 280 and adds
a solid-red overlay there if the EL1 exception handler runs. Black means the
EL1 MMU routine returned; red means an exception reached the loader's vector;
the original row means neither visible marker appeared. It built locally with
SHA-256 `52276f77e57356a01604bdaf40ba7c07c10bc48c09b3074061f7cc790542c84c`.
The user verified and ran it but reported the original row, with neither red
nor black visible.

The current follow-up candidate removes the vector's framebuffer write before
register/stack handling. That early write could recursively fault if the
original transition fault involved framebuffer translation. The C exception
handler now disables EL1 translation before drawing its red row-280 signal;
the successful `arch_mmu_enable()` return path likewise disables translation
before drawing green and halting. Black remains the immediate post-`M`
checkpoint. The candidate is repeat-build stable at SHA-256
`b57f7b641a58e91cbfc6afbe68902cbf6b0f12a2209be545da91a864da40d0e5` and has
not yet been tested on hardware. Red means the exception handler reached its
safe identity-access path; green means the MMU-enable routine returned. An
unchanged trace remains inconclusive.

The user verified that the `b57f7b...` payload was uploaded and reported the
same trace. The next payload removes C and stack dependence from both outcomes:
green is written by direct assembly immediately after setting `SCTLR_EL1.M`,
and the diagnostic exception vector branches to stackless code that clears
translation/cache-enable bits, writes red, and halts. It built twice with
identical SHA-256
`89ff2aebf3bdcaea71038233f9d145f19daead6b29085052c8cba4801d0fac42`; it has
not yet been run on hardware. Green indicates the first post-MMU framebuffer
stores completed; red indicates an exception reached the direct vector. An
unchanged trace remains inconclusive.

The user verified and ran the magenta row-280 candidate with SHA-256
`e9f685e50c68b74bf5314df7ec0e3e0de934e2815353537e72382e9f52a980ff`, then
reported the same trace. Because magenta blends with an existing pink band,
reported the same trace. A later black/green/red diagnostic with SHA-256
`ba9d527a62d1595acfbd38441a74ce9470336621dc5a256a4e0667ab9bc5e83f` also
showed the same trace. The framebuffer range `0x8BB80C000`–`0x8BC004000` is
within TTBR0 L1 index 34, so the single-block mapping is valid; that audit did
not explain the missing overlay. The result remains inconclusive. Do not repeat
the same marker payload; compare the compiled loader's framebuffer pointer,
mapping descriptor, and exception/write path with the standalone probe.

The subsequent binary audit invalidated the claimed stackless-vector test in
`89ff2aeb...`: its assembly checked `BOARD_iphone7plus`, which the assembly build
does not define. The actual vector also omitted the low 12 address bits of its
register-save buffer and overwrote the cached framebuffer pointer/stride. The
repaired diagnostic uses `CONFIG_PLAT_IPHONE7PLUS`, fixes the save address, and
uses a 128x128 black square at the top for exceptions and a black band at
row 280 for the MMU checkpoints, removing dependence on uncalibrated colors.
Its SHA-256 is
`8ecb679527ae6d86ecb7d8d663faf16d65eb506108408d0975ff29af6d5f5d02`.
The build now runs `verify-mmu-diagnostic.py` against the ELF and packaged raw
payload; it verifies all 16 vector branches and the stackless handler's output
path and rejects the prior uploaded image. The repaired image has not yet been
tested on hardware. Previous unchanged traces do not establish MMU success or
prove that the exception vector was never entered.

The user subsequently uploaded the pinned `8ecb6795` image and reported no
visible change. `check-loader-emulation.py` passes both its unmodified MMU
round trip and an injected exception in generic ARM emulation. This does not
model the A10-specific hardware. The next image is an explicit pre-MMU positive
control: `build-entry-vector-control.py` changes three instructions in a copy
to draw black at entry and trigger SVC immediately after vector installation.
It is saved as `build/iphone7plus/diagnostics/entry-vector-control/loader.bin`,
SHA-256 `59c47fcee73f5ff208e7b4cfc777fc9a9de8bcb78daef34d23519f0ebb7582a0`.
Those exact bytes pass emulation and should replace the first few colored
bands with a 128x128 black square before halting. Hardware validation is pending.

The user subsequently observed the expected black square without colored
bands for the entry control, validating that early exception/display path on
hardware. The next checkpoint is `diagnostics/before-mmu-control/loader.bin`,
which deliberately triggers the same handler in place of the SCTLR enable
write, after the preceding setup. A black square again is the expected positive
result; colored bands may remain below it. A second `after-mmu-control` image
places the deliberate exception immediately after the enable write and ISB.
Both new control images pass emulation; hardware results are pending.

The before-M control subsequently produced the expected black region with a
thin yellow remainder and later colored bands. This matches the black square
covering rows 0–127 and confirms the before-enable checkpoint on hardware.
The prepared `diagnostics/after-mmu-control/loader.bin` is next; it triggers
SVC immediately after the SCTLR enable write and its ISB. Hardware validation
of that checkpoint remains pending.

The after-M control subsequently showed only the short black entry band and
the earlier colored trace, without the larger exception square. We have a
positive checkpoint before enable and no observed handler after it; the exact
failure within enable/ISB/translated exception entry is still unknown.

The next diagnostic reads the real pre-enable registers and page-table bytes
as sixteen hexadecimal rows, preserving the original loader/table addresses:

```sh
./build/iphone7plus/load-pongo-raw-probe ./build/iphone7plus/diagnostics/mmu-register-report/loader.bin
```

SHA-256: `7d63d9f50092ccd4a70df34ac3939a94682af26b01fc1d62e32704f9b617a9ad`.
This deliberately halts before enabling translation. Photograph rows 0–F;
their meanings are documented in `docs/iphone7plus-sel4-bringup.md`. The exact
image passed emulation, including decoding every drawn glyph and checking
register/table values. Hardware validation remains pending.

Rebuild with `build-mmu-register-report.py`, passing the archived `8ecb6795`
ELF, its raw payload and the output directory. The generator rejects other
source hashes and requires all-NOP padding for the report. It changes one
instruction plus that padding, preserving the baseline source artifact.
