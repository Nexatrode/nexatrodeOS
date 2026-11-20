# iPhone 7 Plus seL4 bring-up

This is the hardware-port track for booting NexatrodeOS (seL4 + Microkit)
directly on the iPhone 7 Plus. Linux is a reference for hardware discovery and
boot debugging; it is not a host layer for seL4. Keep this work separate from
the existing QEMU `qemu_virt_aarch64` build and the Pixel 4a `sunfish` scaffold.

This document records experiments in chronological order. The early status
below is historical: later entries report a seL4/Microkit user task running on
the phone and interactive Slint prototypes. Those results do not establish a
complete, production-ready hardware port.

## Current evidence

**Latest correction (2026-10-05):** the numeric hardware report identifies an
unsupported granule configuration: `ID_AA64MMFR0_EL1=0xF0100001` reports no
4 KiB support and does report 16 KiB support. The loader selected 4 KiB. Earlier
standalone transition claims below are invalidated by an SVC-return bug;
the subsequent 16 KiB diagnostic now passes on the phone with SCTLR.M=1 and
successful code/display translations. Details are recorded at the end of this
file. This is a loader/MMU milestone; the seL4 kernel has not booted.

- The QEMU image is built for Microkit 2.3.1's `qemu_virt_aarch64` board: ARM
  `virt`, Cortex-A53, 2 GiB, GICv2, with the system image loaded at
  `0x70000000`. Its ELF wrapper is specific to that QEMU loading convention.
  The current wrapper has one executable ELF segment at `0x70000000`; the
  underlying Microkit system image is a raw binary, not a Linux ARM64 Image.
- The official Microkit platform list has no Apple A10 / iPhone target. A new
  phone target therefore needs platform support in seL4/Microkit, not just a
  different screen resolution or device tree.
- The successful Pongo handoff is EL1 with the MMU disabled and all DAIF
  interrupts masked. The cached Microkit SDK 2.3.1 includes a separate
  `qemu_virt_aarch64_el1` build, which is the closer startup-mode reference;
  the ordinary `qemu_virt_aarch64` build uses a different kernel image.
- The packaged QEMU Microkit images cannot be reused at the iPhone's measured
  addresses. In the cached EL1 SDK, `loader.elf` enters at physical
  `0x70000000` and `sel4.elf` has physical load address `0x40000000`; both
  are below the iPhone's RAM base `0x8000EC000`. The EL2 kernel instead has
  physical load address `0x60000000`, also below RAM. A phone build needs a
  board-specific kernel and loader placement inside a freshly queried usable
  range, plus the D11 timer, interrupt controller, and memory description.
- Inspection of the matching Microkit 2.3.1 source clarifies the raw image
  handoff. Its AArch64 loader sets its own stack at entry, relocates to the
  board's `loader_link_address`, reads the Microkit loader metadata embedded
  after its BSS, copies the configured image regions, then enters seL4. The
  current loader ignores Pongo's incoming `x0`; it passes seL4 an embedded
  parameter block and a zero DTB address/size. Therefore a Pongo-to-Microkit
  trampoline is not needed just to translate Apple's `boot_args` pointer.
  The target still needs a real seL4 platform configuration, and its loader,
  kernel, and system-region addresses must all land in RAM.
- The remaining first-boot blocker is interrupt delivery. The Linux T8010 DTS
  identifies an Apple AIC at physical `0x20E100000` and routes the architectural
  physical timer through an AIC FIQ; it also notes that A10 has no EL2. In the
  matching seL4 16.0.0 source, the AArch64 FIQ vectors for both current EL and
  lower EL enter `invalid_vector_entry`. Mapping the timer as an ordinary GIC
  PPI will not work; the kernel needs an FIQ entry path. A full Apple AIC
  acknowledge/mask backend will be needed for ordinary device IRQs, but the
  first timer test can use the timer's own status register.
- Source tracing narrows the first timer experiment. In seL4 AArch64 EL1 mode
  (without `CONFIG_ARM_HYPERVISOR_SUPPORT`), the generic timer code uses
  `CNTV_TVAL_EL0`, `CNTV_CVAL_EL0`, and `CNTV_CTL_EL0`; the physical probe
  measured `CNTFRQ_EL0=24,000,000`, so the platform's `TIMER_FREQUENCY` is
  known. Linux's Apple AIC driver handles FIQs separately from normal AIC IRQs
  and identifies timer FIQs by testing timer `ENABLE`, `IMASK`, and `ISTATUS`
  bits. For an EL1 kernel, Linux translates the DT's guest-virtual timer FIQ to
  its EL0 virtual timer source. This aligns with seL4's existing CNTV choice.
- Therefore the smallest initial kernel experiment is timer-only: route the
  current-EL and lower-EL FIQ vector slots through seL4's existing interrupt
  entry wrappers; have the single-core interrupt backend report the kernel
  timer IRQ when `CNTV_CTL_EL0` says it is firing; and clear the pending
  condition through the existing timer tick/rearm path. A second necessary
  change is the exception return state: seL4 initializes both `PSTATE_USER` and
  `PSTATE_IDLETHREAD` with `PMODE_FIRQ` set. The Apple target must clear that
  mask in those contexts only after the FIQ path is ready; exception entry then
  masks FIQ while the kernel runs, as required for the existing current-EL
  interrupt wrapper's idle-thread assumption.
  The first implementation touches the AArch64 vector table in
  `src/arch/arm/64/traps.S`, the user/idle PSTATE constants in
  `include/arch/arm/arch/64/mode/machine/registerset.h`, and a new
  `INTERRUPT_CONTROLLER` header selected by the D11 platform config. That
  backend needs to cache one synthetic timer IRQ between `getActiveIRQ()` and
  `ackInterrupt()`, since seL4's interrupt API requires stable active-IRQ
  reporting. Before unmasking FIQ in user/idle return state, boot must also
  leave the other Apple FIQ sources (such as PMU and fast IPI) disabled or
  provide an explicit safe handler for them; a timer-only probe cannot let an
  unhandled FIQ repeat forever.
  This first step can avoid treating the whole Apple AIC as a GIC and avoid
  exposing hardware IRQ capabilities to user components. Full AIC acknowledge,
  mask, and device IRQ support remains a later platform milestone. This is a
  source-based implementation plan, not yet a tested kernel patch.
- In the inspected PongoOS 2.6.3 source, `bootl` is explicitly the Linux path:
  it prepares an ARM64 Linux Image and Linux FDT, applies Sandcastle overlays,
  and uses the Linux entry address `0x800080000`. That success does not show
  that Pongo can boot a Microkit image through `bootl`.
- PongoOS's `bootr` is the raw-image path. The inspected code jumps to the
  uploaded buffer and passes Apple's `boot_args` pointer in `x0`. This is not
  yet proven to satisfy Microkit's loader contract; the QEMU image assumes a
  different load address and platform setup.
- The first raw payload physically ran on iPhone9,2 under PongoOS 2.6.3:
  `bootr` transferred control and the payload wrote visible framebuffer
  bands. This confirms raw entry and one `boot_args`-based framebuffer access,
  not a seL4 boot.
- The stack/boot-arguments probe also physically ran. Its `STACKOK` marker
  confirms the temporary stack worked through nested C calls and a canary
  check. The displayed framebuffer values decode to stride `0x1100`, width
  `0x438` (1080), and height `0x780` (1920). A later read of Pongo's live
  `bootargs` command reported `physBase=0x8000EC000`, `memSize=0xBB720000`,
  `topOfKernelData=0x8074E0000`, and `deviceTreeLength=0x2A804` on the first
  measured boot. A later Pongo boot reported `topOfKernelData=0x807154000`,
  demonstrating that this boundary can move between boots.
- The updated bootargs probe ran on the phone and reported
  `CNTFRQ_EL0=0x016E3600` (24 MHz) and `MPIDR_EL1=0x80000000` (all affinity
  fields zero, primary affinity 0). This supplies the generic timer frequency
  for the board config; it does not resolve delivery through the Apple AIC.
- A temporary `freemap` module, built against the pinned PongoOS source and
  loaded through PongoTerm, queried the allocator directly on a later boot.
  It reported a free range `[0x8072D8000, 0x8BAD80000)` with 183,978 16 KiB
  pages. This adds a third observed starting boundary (`0x8074E0000`,
  `0x807154000`, and `0x8072D8000`), so a kernel placed at any one observed
  boundary is unsafe as a general choice.
- The Microkit 2.3.1 iPhone SDK now builds the seL4 kernel, loader, monitor,
  runtime library, and initializer. A one-component, no-I/O smoke system also
  packages successfully. After the live allocator query, the fixture was
  moved to start at `0x808000000`, above all three measured allocator starts;
  the latest generated memory interval is `[0x808000000, 0x8BAD80000)`. The
  kernel's physical segments begin at `0x808000000` and extend through
  `0x808241000`; the Microkit report places system objects from
  `0x808000000` through `0x8082A9A80`. The loader's segment starts at
  `0x810000000` and is about 2.5 MiB. These are compile-time placements with
  margin above measured boundaries, not a live boot check. The compiled static
  memory description still needs validation against all live Pongo
  reservations. The image exceeds Pongo's default 1 MiB receive buffer, but
  the pinned PongoOS source supports resizing it to an aligned image length up
  to 128 MiB. The host uploader sends that resize request. The build also
  strips the ELF container to a flat raw image for Pongo's `bootr`; Microkit's
  AArch64 startup relocates that flat image to its link address. The
  iPhone-specific loader startup now preserves Pongo's `boot_args` pointer
  across relocation, and its C entry draws a green/amber block using the
  framebuffer fields. The phone displayed the two color bands, confirming the
  loader reached C entry with usable display metadata. A second, distinct
  marker is now drawn after `copy_data()` to indicate that the loader copied
  the kernel and system regions. On the second run the phone showed four
  bands, reported as pink/yellow/red/yellow, confirming both loader milestones.
  Neither marker proves that seL4 or the smoke component started. The updated
  2,611,108-byte image was uploaded and the host issued `bootr`; its USB control
  request timed out during handoff, as did a follow-up PongoTerm connection.
- The third loader smoke image adds a temporary TTBR0 mapping for the Pongo
  framebuffer at `0x8BB80C000`, beyond reported RAM. It maps the containing
  1 GiB L1 block as Device-nGnRnE when that slot is empty. The full framebuffer
  ends at `0x8BC004000`; both endpoints are in L1 index 34, so the original
  one-block range guard accepts it. This display mapping is diagnostic only
  and does not add the buffer to seL4 usable RAM.
- On the third run, the user reported five visible bars: pink, light yellow,
  red, dark yellow, dark yellow. The fifth bar was treated as the pre-MMU
  mapping marker, consistent with the original helper accepting the framebuffer
  range. The photo did not establish whether `arch_mmu_enable()` returned.
- On the fourth run the user reported seven visible bars: pink, light yellow,
  red, yellow, yellow, light off-white, pink. The last pink was interpreted as
  the checkpoint immediately before `arch_mmu_enable()`; no later return marker
  was reported. This still does not localize the failure conclusively.
- The next image adds nine 48-row checkpoints, 56 rows apart starting at row
  offset 224: `arch_mmu_enable()` entry; after EL setup; before the EL-specific
  routine; EL1 assembly entry; after cache/MMU disable; after page-table and
  TLB setup; immediately after setting SCTLR.M; after the EL1 routine returns;
  and after `arch_mmu_enable()` returns. The visible markers should identify
  which substep fails or stalls. The new image builds to
  `build/iphone7plus/smoke/loader.bin`.
- The latest build retains those checkpoints and changes only the iPhone
  loader's identity-map memory type from Device-nGnRnE to Normal non-cacheable.
  Arm's architecture manual allows instruction fetch from a non-XN Device
  mapping either to act as Normal non-cacheable or to raise a permission fault,
  so this tests a plausible A10-specific cause at the MMU switch. The
  framebuffer keeps its Device mapping. This remains a hypothesis, not a
  confirmed fix.
- The user ran that build and reported the same bar sequence, ending at the
  pre-MMU-enable checkpoint. The non-cacheable Normal mapping did not move the
  failure point. The next build reads `SCTLR_EL1.WXN` into the pre-transition
  marker color, then clears WXN for the temporary iPhone loader mapping before
  setting SCTLR.M. The linked assembly was checked to confirm that it clears
  bit 19. This tests whether write-xor-execute on the loader's combined
  code/data block is preventing the first post-MMU instruction fetch. The user
  again reported the same sequence, so clearing WXN did not visibly advance the
  transition. The following image adds an iPhone-only marker in the loader
  exception handler: instruction aborts, data aborts, and other synchronous
  exceptions receive distinct colors at row offset 784. This should tell us
  whether the failed transition reaches the existing exception vector; it does
  not yet identify a fault address. The user reported the same 19 visible
  stripes, ending in red, so that marker did not appear. The next image keeps
  the exception marker and enables SCTLR.M while leaving the instruction and
  data caches disabled. This isolates address translation from enabling both
  caches at the same transition. The user saw no additional stripes. The next
  image simplifies the temporary loader identity map to a single 1 GiB L1
  block, removing the L2 table walk for the first translated instruction
  fetch. It retains the framebuffer mapping, exception marker, and MMU-only
  enable so mapping depth is the test variable. The user reported the same
  visible stripes. The current image changes only the loader block's memory
  attribute to cacheable Normal memory, while keeping SCTLR.C and SCTLR.I
  disabled. This checks whether the non-cacheable mapping itself is incompatible
  with execution on this A10 path. The user again reported the same colors. The
  newest image now writes a clean SCTLR value (`0x30d00801`: seL4's RES1 bits
  plus M) instead of preserving Pongo's other SCTLR controls. This tests
  whether inherited control bits interfere with enabling translation. The user
  reported the same colors, with red still last. The current image adds inner-
  shareable attributes for both TCR_EL1 table walks, matching common arm64
  kernel setup while preserving the clean SCTLR and single-block identity map.
  The user reported the same stripes, ending in red. The next image keeps those
  settings and inner-shareable walks but switches the TCR table-walk cacheability
  fields from write-back to non-cacheable. This checks whether the first walk
  depends on cache maintenance or the shareability/cacheability combination;
  it remains an isolation test, not a fix. The user reported the same stripes,
  ending in red. The next image adds an assembly-only, stackless framebuffer
  marker immediately after setting SCTLR.M and before returning to the C
  marker path. If the new two-color band at row 840 appears, translation and a
  store through the mapped framebuffer worked, and investigation can focus on
  the C return path. The user reported the same colors, ending in red, so the
  stackless marker did not appear. The next image writes a separate marker as
  the very first code in the AArch64 exception vector, before the vector uses
  SP. The user reported the same colors, so that marker did not appear. The
  next image makes TTBR0 use a 39-bit low virtual-address range rooted directly
  at the existing L1 table, removing the L0 table walk while keeping all iPhone
  RAM and framebuffer addresses in range. This isolates the low-range root
  level from the prior 48-bit TTBR0 setup. The user again reported the same
  colors. The next probe is independent of Microkit: raw Pongo entry, no C
  runtime or stack, one runtime-built L1 table, an identity block for the
  payload, and a Device block for the framebuffer. It paints separate bands
  before table setup, after table setup, after enabling the MMU, and from its
  own exception vector. This distinguishes a general EL1 MMU transition issue
  from a problem specific to Microkit's loader or generated page tables. The
  user photo first showed the entry marker (pink) and page-table-ready marker
  (light yellow), with the MMU-on marker still absent. A controlled SVC was then
  added before the transition. The user reported pink, yellow, pink, red with
  the white lower halves blending into the screen, confirming raw entry,
  page-table setup, successful MMU-on execution, and the SVC vector/return for
  that probe's mappings. A closer address comparison found that Pongo's receive
  buffer (`0x8BADC0000`) and framebuffer (`0x8BB80C000`) share L1 index 34. The
  probe first marks its code block Normal, then overwrites that same L1 entry
  with the framebuffer's Device descriptor. Its successful test therefore
  does not prove execution with Microkit's actual layout: Microkit relocates to
  `0x810000000` (L1 index 32), maps that block Normal using AttrIdx4, and maps
  the framebuffer in a separate Device block at index 34. The first Microkit
  test disabling TTBR1 walks still ended in red, so EPD1 alone did not visibly
  change the failure. A new standalone control probe is built at
  `build/iphone7plus/pongo_mmu_relocated_probe.bin`; it relocates to the same
  address and matches Microkit's fixed TCR/MAIR fields and split-block mapping.
  The user reported five bands in sequence (pink, yellow, light yellow, pink,
  red, with black/white separators), confirming relocated entry, table setup,
  MMU-on execution, and SVC return with the split code/display mappings. The
  relocation marker rendered yellow rather than violet on this framebuffer.
  This rules out the broad MMU/TCR/MAIR/split-block setup as the cause. The
  remaining distinguishing factor is now the Microkit loader's pre-generated
  table contents versus runtime table setup (plus its surrounding loader
  control flow). The next Microkit image rebuilds the 512-entry lower L1 table
  at runtime after `copy_data()`, then adds only the loader/kernel Normal block
  and display Device block. A violet checkpoint at row 168 confirms this setup
  reached the pre-MMU handoff. The user reported pink, light yellow, red,
  yellow, followed by the familiar Microkit stripes ending in red. The fourth
  band is the row-168 runtime-table checkpoint (rendered yellow); the image
  still did not show a later MMU-on success marker. Rebuilding the table at
  runtime therefore did not resolve the fault. The next isolation should use
  the successful relocated raw probe to reproduce Microkit's stack frames,
  cache-maintenance calls, and exact MMU enable/return sequence one at a time.
  That control is now built at
  `build/iphone7plus/pongo_mmu_callframe_probe.bin`: it relocates to the
  Microkit address, uses a stack in the Normal block, runs the same set/way
  D-cache flush and I-cache invalidation, loads matching MAIR/TCR/TTBR values,
  enables M, writes a marker, and returns through the saved stack frame. An
  additional green marker at row 280 means the stack restore and call return
  both succeeded. It passed assembly, link, and no-relocation checks. The user
  ran it and reported the bands through red at row 224 and pink at row 280.
  Red is the intentional SVC control; the later pink return marker confirms
  the MMU routine restored its saved registers and returned. The expected
  immediate post-MMU marker at row 168 was described as two dark/black rows,
  making its color ambiguous, but the row-280 marker proves execution passed
  that point. This validates the relocated mapping, Microkit-like cache work,
  SVC handling, MMU-on path, and stack return in the standalone probe. It does
  not yet prove the real Microkit loader follows the same path. The next loader
  image assigns visible colors to the post-SCTLR.M, assembly-return, and
  C-wrapper-return checkpoints.
  The current loader retains pre-MMU progress bands at rows 224/280/336/392/448/504.
  It no longer prepaints the post-MMU rows. One dedicated cyan band at row 560
  is written directly after setting SCTLR.M; seeing it proves execution reached
  the immediate post-MMU marker. Later C checkpoints remain at rows 784/840/896
  and 952/1008; row 1176 marks an exception. Earlier interpretations based on
  prepainted rows were invalid because the source reused those rows for both
  placeholders and post-MMU markers.
  The nested-call variant was run. The user reported a red SVC marker followed
  by the later pink return marker, showing that the loader-shaped nested
  `el1_mmu_enable() -> el1_mmu_disable()` call, helper stack frame, and return
  work with the standalone probe's mappings. This makes the helper call frame
  itself an unlikely cause of the real loader's failure. The next variant
  keeps that nested sequence but relocates its TTBR0 L1 root to the loader's
  actual `boot_lvl1_lower` address (`0x810009000`) and sets TTBR1 to the
  loader's `boot_lvl0_upper` address (`0x810005000`); it also reserves that
  upper-root page and moves the test stack clear of both roots. The user saw
  the same red-then-pink markers, confirming that the real root addresses work
  with the probe's two identity mappings. The rebuilt probe now also calls the
  loader's exact first post-MMU marker assembly using the same framebuffer
  base/stride register contract. The user reported no visible change, despite
  the later return marker still appearing. A second standalone control wrote a
  solid red 128-by-48 band with those same inputs and also produced no visible
  change. Standalone framebuffer-marker variants are no longer useful for
  narrowing the real loader's failure. The next and final hardware check in
  this sequence changes the real loader's post-MMU marker itself to a solid
  red band. The user confirmed the raw transfer completed, then `bootr` timed out as the
  USB device disconnected. On the next screen they reported five bands after
  the familiar red: white, pink, yellow, white, pink. Their spacing aligns
  with rows 560/616/672/728/784. The `cfa5…` run was not a distinct visual diagnostic: it cleared rows
  560/616/672/728, then repainted them as pre-MMU placeholders and reused them
  for the post-MMU marker. That repaint is removed. The subsequent `44f281…`
  payload moved a cyan marker to row 560 directly after SCTLR.M, but the user
  saw no band there. An earlier standalone control also failed to show its
  immediate post-MMU write despite showing a later marker after the MMU routine
  returned, so absence at that first store is inconclusive. The current
  payload instead writes a solid magenta band at row 560 only after
  `el1_mmu_enable()` returns. The user ran `44f281…` and saw the last red
  pre-MMU band without row 560, then ran `3fb9c8…` using the C-based return
  marker and reported no change. The current diagnostic uses a register-only
  assembly writer at that post-return point, matching the successful standalone
  control. Payload SHA-256:
  `6ad81c6f89ccb14042226ce834868c0ebd6ff4b864f3a9b3deca9525eac02270`. The
  user verified this hash and again saw no change. Therefore the production
  loader's post-return marker is not visible; because the standalone control's
  immediate marker was also invisible despite a later successful return, the
  framebuffer trace remains inconclusive. Do not repeat color/stripe uploads.
  The next investigation is the production loader's C/boot state and call
  context versus the standalone control.

- The MCS `KERNEL_WCET` setting in this build is a temporary `10 us` placeholder
  copied from an existing Microkit board configuration. It is not an A10
  measurement and must not be used to set real protection-domain budgets.
- Ferros is a Rust library for writing seL4 userland components. It may help
  with Rust services after the kernel and Microkit system boot, but it does not
  provide the Pongo entry path or replace this loader port.
- The Sandcastle iOS 15 patch inspected earlier targets D10 and a different
  Pongo build. It is not a basis for a D11 (`iPhone9,2`) boot attempt.

## Bring-up sequence

1. **Freeze a reproducible baseline.** Record the phone's exact board ID, Pongo
   build hash, Linux image and DTB hashes, and a known-good Linux boot log. Do
   not change partitions or restore iOS as part of this stage.
2. **Specify the raw handoff.** From the exact Pongo source/build, document the
   entry address, exception level, MMU/cache state, register values, usable RAM
   ranges, reserved regions, and how to obtain the iPhone's live device tree.
   Define a tiny trampoline that can validate those facts before entering seL4.
3. **Establish a debug path.** Identify output available after Pongo hands off
   (serial, USB, or framebuffer) and verify it with a minimal AArch64 payload.
   Without post-handoff output, a silent boot cannot distinguish an entry fault
   from a later seL4 failure.
4. **Complete the memory handoff contract.** Merge the current Pongo allocator
   map with the named bootargs/DT ranges, preserve active Pongo and diagnostic
   buffers, and make the loader's input size fit the actual receive path. The
   current compile fixture is only a layout estimate.
5. **Prove incrementally.** Boot a minimal seL4/Microkit system, then verify
   inter-component isolation, timer interrupts, and repeatable warm/cold boots
   before attempting display, touch, storage, or networking drivers.

## First go/no-go

Do not feed the existing QEMU ELF wrapper to Pongo's Linux loader. Source
review and the non-destructive raw-entry/framebuffer probe are now complete.
The raw-entry, CPU-state, framebuffer, boot-arguments, and sparse RAM-access
checks now pass, and the timer-only kernel plus a Microkit smoke image compile.
The pinned PongoOS source confirms `usbloader.c` starts with a 1 MiB allocation,
and its request-1 control transfer with a 4-byte length resizes and arms the
bulk receive path, with a 128 MiB maximum. The host uploader now sends that
request for the complete image size before transferring chunks. It compiles,
but no large payload has been sent. Pongo's `bootr` jumps straight to the first
byte of the receive buffer and does not parse ELF. The Microkit AArch64 loader's
`crt0.S` is designed to relocate itself from a flat payload to `LINK_ADDRESS`;
the build now emits `loader.bin` from `loader.elf` for that raw entry path. The
phone enumerated as PongoOS during the 2026-10-04 check, but `pongoterm` timed
out or stalled before presenting a prompt, so no fresh allocator command was
run. The next gate is a reliable current-boot memory map; the current image's
kernel and system regions were placed from an older allocator measurement and
are not approved for upload. The first timer-only port does not need normal device IRQ delivery; add
the full AIC event path before exposing hardware IRQs to components. The current
Pongo `boot_args` and framebuffer give us a post-handoff diagnostic channel
while UART support is unavailable. No iOS restore, repartition, or flash is
needed for these build and probe steps.

The first raw-entry probe and a libusb uploader are built under
`platforms/iphone7plus/`. The 156-byte payload has no relocations, checks the
framebuffer fields in `boot_args`, attempts to paint two small colored bands,
and waits. In the first physical run on iPhone9,2 with PongoOS 2.6.3,
`bootr` transferred the payload and the user observed a short bar at the
top-left. Viewed straight on it looks pinkish above and light yellow below;
at an angle it can look like a single red bar. This is evidence that the raw
handoff reached the payload and that it wrote visible pixels, but the
angle-dependent appearance makes color-channel interpretation inconclusive.
The apparent size also needs characterization. This is not a seL4 boot test.

## Raw handoff details from PongoOS 2.6.3 source

The local Pongo source is revision `2.6.3-4-g6632a20`. `pongo_boot_raw()`
sets `BOOT_FLAG_RAW`; the boot path shuts down its timer/USB shell and disables
interrupts before entering `pongo_entry()`. That path cleans up and disables
the MMU, then calls `jump_to_image_extended()` with the raw upload buffer,
`gBootArgs`, no trampoline, and Pongo's previous entry point. The assembly
sets `x0` to the `boot_args` pointer, clears `x1`–`x3`, sets `sp` to zero, and
branches to the uploaded image. The current exception level is not normalized
by this raw branch; the source may issue `smc 0` to reach EL3 on hardware that
implements it. The probe's successful framebuffer write confirms `x0` and the
video fields are usable. The follow-up state probe then measured EL1, MMU off,
and all four DAIF masks set, confirming the corresponding source-level
expectations.

The 416-byte state probe ran on the phone. The user-provided image shows the
second `CurrentEL` track (EL1), the `SCTLR_EL1.M` marker on the right (MMU
disabled), and all four `DAIF` squares lit (D/A/I/F masked). This gives a
concrete raw-entry target: EL1, MMU off, `sp` unset, and all DAIF masks set.

The original 2,116-byte boot-arguments probe ran on-device, and the subsequent
1,756-byte larger-text rebuild also ran successfully. Pongo allocates its
upload buffer as a contiguous 1 MiB minimum; the probe installs a temporary
stack at `payload_base + 0xF0000`, verifies stack traffic with a canary and
nested C calls, then renders hexadecimal values for `virtBase`, `physBase`,
`memSize`, `topOfKernelData`, the device-tree pointer/length, framebuffer
geometry, payload address, and stack address. The earlier photo was decoded
as follows. Those photographed address values were transcribed unreliably and
are superseded by the live console data below. The framebuffer geometry and
probe payload/stack values were independent fields from that earlier run.

Pongo's `deviceTreeP` is Apple's compact recursive device-tree format, not a
standard FDT blob. Pongo's local `dt_node_t`/`dt_prop_t` definitions and parser
show how to walk it, including the `/chosen/memory-map` properties. The local
Sandcastle checkout contains no matching iPhone9,2 tree. The custom raw walker
did not finish; Pongo's native `dt` command is used for the live tree instead.

Pongo's live `bootargs` output in the first measured console session reported:

```text
virtBase         0xfffffff0140ec000
physBase         0x8000ec000
memSize          0xbb720000
topOfKernelData  0x8074e0000
deviceTreeP      0xfffffff01b318000
deviceTreeLength 0x2a804
framebuffer      stride 0x1100, width 0x438, height 0x780
```

Using `physBase` and `memSize` gives a candidate physical RAM interval
`[0x8000EC000, 0x8BB80C000)`. This is a bound, not yet a complete seL4
usable-memory map: Pongo's upload buffer, framebuffer, device registers, and
other active reservations must be accounted for. The earlier payload address
and temporary stack values belonged to a prior run and should not be combined
with this boot. Do not jump the QEMU ELF directly.

## Live memory-map inspection through Pongo's console

The custom raw recursive walker did not complete, so its partial output is not
used as memory-map evidence. Instead, the local Pongo `pongoterm` connected to
the iPhone9,2 running `pongoOS 2.6.3-6632a20b`; Pongo's own command
`dt /chosen/memory-map -8` successfully parsed the live node. The 16
`MemoryMapReserved-*` properties all decoded to `(0, 0)`, meaning these are
currently unused slots. They are not occupied RAM ranges and must not be
subtracted from available memory.

The populated ranges printed by Pongo on that first measured boot were:

| Name | Physical start | Size |
|---|---:|---:|
| TrustCache | `0x8047B8000` | `0x10000` |
| DeviceTree | `0x807318000` | `0x2A804` |
| SEPFW | `0x807344000` | `0x158000` |
| preoslog | `0x80749C000` | `0x40000` |
| BootArgs | `0x8074DC000` | `0x4000` |
| Kernel-__TEXT | `0x8064F0000` | `0xB0000` |
| Kernel-__DATA_CONST | `0x8065A0000` | `0xCC000` |
| Kernel-__TEXT_EXEC | `0x80666C000` | `0x694000` |
| Kernel-__KLD | `0x806D00000` | `0x4000` |
| Kernel-__LASTDATA_CONST | `0x806D04000` | `0x4000` |
| Kernel-__LAST | `0x806D08000` | `0x4000` |
| Kernel-__KLDDATA | `0x806D0C000` | `0x4000` |
| Kernel-__DATA | `0x806D10000` | `0xA8000` |
| Kernel-__BOOTDATA | `0x806DB8000` | `0x44000` |
| Kernel-__PRELINK_TEXT | `0x8047C8000` | `0x66C000` |
| Kernel-__PRELINK_INFO | `0x807024000` | `0x2F4000` |
| Kernel-__PLK_TEXT_EXEC | `0x804E34000` | `0x1390000` |
| Kernel-__PRELINK_DATA | `0x806ED0000` | `0x154000` |
| Kernel-__PLK_DATA_CONST | `0x8061C4000` | `0x32C000` |
| Kernel-__LINKINFO | `0x806DFC000` | `0x44000` |
| Kernel-__LINKEDIT | `0x806E40000` | `0x8E308` |
| Kernel-mach_header | `0x8064F0000` | `0x20` |

### Pongo-owned allocations visible in the first measured session

The device tree lists iBoot/XNU regions but not Pongo's own dynamic
allocations. I read the live Pongo globals with its `md8` command, using
symbols from the matching local `2.6.3-4-g6632a20` Mach-O. Pongo's cacheable
alias translates to physical addresses by adding `0x400000000`.

| Resource | Physical range | Evidence |
|---|---:|---|
| `bootr` upload buffer | `[0x8BADC0000, 0x8BAEC0000)` | `loader_xfer_recv_data` resolves to `0x8BADC0000`; live receive allocation size is `0x100000` |
| Framebuffer shadow copy | `[0x8BAF18000, 0x8BAF98000)` | `gFramebufferCopy` resolves to `0x8BAF18000`; `screen_init()` allocates `stride × height = 0x7F8000` bytes |
| Page metadata and page tables | `[0x8BB730000, 0x8BB80C000)` | `ppage_list` starts at `0x8BB730000`, contains `memSize / 0x4000` 32-bit entries (`0xBB720` bytes), and the page-table allocator occupies the aligned tail through RAM end |
| Display framebuffer | `[0x8BB80C000, 0x8BC004000)` | `boot_args.Video.v_baseAddr`, stride `0x1100`, height `0x780`; `stride × height = 0x7F8000` bytes, starting at the end of reported RAM |

These exact allocations close several gaps in the device-tree view. To check
the remaining RAM, I built a temporary read-only Pongo module that calls the
exported `phys_get_entry()` for every 16 KiB page and prints contiguous pages
whose allocator state is `PAGE_FREE` (zero). Its `freemap` command reported:

```text
FREE 0x8074E0000 0x8BAD80000 pages=183848
physBase=0x8000EC000 memSize=0xBB720000
```

So that Pongo allocator snapshot exposed one free interval,
`[0x8074E0000, 0x8BAD80000)`, with size `0xB38A0000` (2,872.63 MiB). On a
later boot, `freemap` reported `[0x807154000, 0x8BAD80000)` (184,075 16 KiB
pages). It starts at that boot's `topOfKernelData`; the named iBoot/XNU regions
lie below it. The upload buffer, framebuffer shadow, page metadata, and page
tables are marked allocated outside the interval. These are Pongo allocator
snapshots, not a generic firmware memory map. Query again after uploading the
actual seL4/Microkit image, since Pongo may resize or relocate its upload
buffer.
Keep the display framebuffer separately mapped at
`[0x8BB80C000, 0x8BC004000)`; it lies beyond reported RAM. Confirm the
device-tree and hardware carve-outs before treating the candidate as final.

The module lived only under `/tmp` and made no allocator changes. The
read-only query did not boot an image or write persistent storage.

### Raw-entry free-RAM smoke test

The 1,336-byte `pongo_freeram_probe.bin` was then uploaded with `bootr`. The
user-provided iPhone9,2 photo shows `STACKOK`, RAM pass bit `1`, tested address
`0x8074E0000`, and stack pass bit `1`; the bottom row is cropped, but the
payload sets `sp` to `0x8BAD80000`. The probe saved the original two 64-bit
words at the first free address, wrote and verified two patterns, restored the
words, and verified the restoration. This confirms raw-entry stack use near
the top of the allocator-reported interval and read/write access at its base.
It does not validate every page in the interval or prove the Microkit loader
contract. The host's USB command timed out as Pongo handed off, while the
expected report appeared on screen.

### Raw-entry free-RAM page sweep

The follow-up `pongo_ramsweep_probe.bin` ran on the later boot using the freshly
queried interval `[0x807154000, 0x8BAD80000)`. Its on-device report showed
`SWEEPOK`, `0x2CF0B` (184,075) pages checked, failing address zero, and stack
status `1`. Its final rows identified the upload buffer at `0x8BADC0000` and
temporary stack top at `0x8BAEB0000`. The probe saved, tested, restored, and
rechecked the first and last 64-bit words of each 16 KiB page. This is stronger evidence than the
base-address canary: every candidate page accepted accesses at both ends, and
the upload-buffer stack remained intact throughout the sweep. It still does
not test all bytes of each page, detect every aliasing/cache issue, or prove
that seL4/Microkit can load and boot using this range. The USB command timed
out during `bootr` handoff; the report appeared on the phone. No persistent
storage was written.

### seL4 timer/FIQ compile fixture

The current source prototype is saved as
[`patches/sel4-16.0-iphone7plus-timer-fiq.patch`](../patches/sel4-16.0-iphone7plus-timer-fiq.patch)
against the pinned seL4 `16.0.0` checkout used by Microkit 2.3.1. It adds an
iPhone 7 Plus platform config, routes Apple timer FIQ vector entries through
seL4's IRQ entry handlers, exposes CNTV status as a synthetic timer IRQ, and
clears the saved FIQ mask for user/idle return so the timer can arrive. The
prototype masks the physical timer and does not implement the Apple interrupt
controller or any device IRQs. Unknown interrupts stop the kernel.

I compiled and linked the modified kernel on the development Mac with Clang
and `ld.lld`; the result is an AArch64 ELF. An LLD warning exposed an
unaligned `.rodata` section in seL4's ARM linker script. Aligning it to 8 bytes
removed the warning. The ELF entry is now `0xFFFFFF88000EC000`, with physical
load address `0x8000EC000`, and `.rodata` is aligned at `0xFFFFFF88000FCFD0`.
The generated device tree uses the measured outer RAM envelope
`[0x8000EC000, 0x8BB80C000)`. It is still only a build fixture: the generated
`avail_p_regs` treats that entire envelope as available and does not subtract
the live Pongo/iBoot allocations, so it must not be loaded on the phone.

There are two separate boot contracts to bridge before a hardware test. Pongo's
raw `bootr` entry gives the payload an Apple `boot_args` pointer in `x0`, as
confirmed by the probe. seL4's AArch64 `_start` expects `x0`/`x1` user-image
physical bounds, `x2` a physical/virtual offset, `x3` the virtual entry, and
`x4`/`x5` an optional standard FDT address and size. A small loader must parse
the Apple arguments, load and relocate the seL4/Microkit image, then call it
with that ABI; raw-uploading the ELF cannot satisfy it.

Microkit 2.3.1's AArch64 loader already makes the six-argument seL4 call, but
its platform code has no iPhone backend. It performs board-specific UART and
CPU setup, adjusts EL/MMU state, loads the configured system regions, and then
jumps to seL4. Its current built-in GIC setup is limited to selected QEMU and
Zynq boards. Porting this loader to Pongo is therefore a separate task: define
safe logging/no-logging, one-core startup, EL1/MMU transitions, and the raw
Pongo upload/relocation contract while providing actual memory reservations.

seL4 also compiles `avail_p_regs` from the build-time device tree. Pongo's
`topOfKernelData` and free allocator boundary vary across boots, so the
outer-RAM map cannot safely describe runtime free RAM. The loader and
kernel must agree on a runtime reservation list covering firmware/kernel
segments, Pongo's upload and framebuffer buffers, the seL4 image, page tables,
and any DTB. The measured free interval is evidence for candidate RAM, not yet
the final seL4 untyped map. A real build must also establish the A10 physical
address and cache settings.

The build uses the pinned checkout at `.cache/sel4-src-microkit231`, plus a
local ignored Python virtual environment at `.cache/sel4-venv` populated from
that checkout's `tools/python-deps` package:

```sh
cmake -S .cache/sel4-src-microkit231 \
  -B .cache/sel4-build-iphone7plus -G Ninja \
  -DCMAKE_TOOLCHAIN_FILE="$PWD/.cache/sel4-src-microkit231/llvm.cmake" \
  -DTRIPLE=aarch64-none-elf \
  -DKernelPlatform=iphone7plus \
  -DKernelSel4Arch=aarch64 \
  -DKernelDebugBuild=ON \
  -DKernelPrinting=ON \
  -DPYTHON3="$PWD/.cache/sel4-venv/bin/python" \
  -DCMAKE_EXE_LINKER_FLAGS=-fuse-ld=lld
cmake --build .cache/sel4-build-iphone7plus --parallel 6
```

This is a source/build milestone, not a hardware boot. Next, implement the
Pongo-to-seL4 loader ABI bridge and runtime memory reservations, replace the
synthetic fixture and approximate Cortex A53 PA/cache settings, and validate
ELF loading and timer behavior before attempting a phone boot.

For the current MMU boundary investigation, the production and standalone
control sources agree on the register values, identity-map descriptors, cache
maintenance, nested stack frames, and return sequence. One remaining difference
is that the production assembly calls the C checkpoint-drawing hook at stages
4, 5, and 6, while the standalone control does not. The hook only paints the
checkpoint bars; UART initialization and exception-vector setup happen earlier
in `main()`. A checkpoint-omission payload is built by
`platforms/iphone7plus/build-mmu-stage-checkpoint-omission.sh`; it compiles the
real loader assembly with only those three drawing calls omitted. Its flat
payload hash is
`42ec1cd6f1c3a9ed4b48fb7a531a62182249ba0212fb42983113c7a2e44a7f92`.
It built and passed packaging, and the user verified the hash and uploaded it
twice. The shorter trace ending pink matches the omitted checkpoint bars. This
is a visual-only change, not a runtime isolation test: the register setup and
EL1 MMU return code are unchanged. No later return/progress marker was
reported, so the transition remains unconfirmed; the known invisible immediate
framebuffer stores keep absence of that marker inconclusive. The compiled C
wrapper was also checked: it calls `ensure_correct_el`, reads `CurrentEL`, then
branches directly to `el1_mmu_enable` for EL1. The prior state probe showed the
phone starts at EL1, so there is no EL transition in this path. No source-level
error has been established.

The next diagnostic fixes a weakness in that exception overlay: the vector
previously painted to the framebuffer before saving registers or reaching the
C handler. If the transition fault were itself a framebuffer translation
fault, that first paint could fault recursively and leave the original image.
The new vector reaches the handler without that write. The handler records
ESR/FAR/ELR, disables EL1 translation, and only then paints red at row 280. If
`arch_mmu_enable()` returns successfully, the loader also disables translation
before painting green at row 280 and halting. Black immediately after setting
`SCTLR_EL1.M` remains the early checkpoint. This candidate built and passed a
repeat-build check with SHA-256
`b57f7b641a58e91cbfc6afbe68902cbf6b0f12a2209be545da91a864da40d0e5`; it has
not yet been run on hardware. Red now means the C exception handler gained
control and returned to identity access before its marker; green means the
EL-specific MMU routine returned. An unchanged image remains inconclusive: the
vector, stack, exception handler, or display path may still prevent a visible
signal.

The user verified that the `b57f7b...` payload was uploaded and reported the
same trace. The next payload removes C and stack dependence from both outcomes:
green is written by direct assembly immediately after setting `SCTLR_EL1.M`,
and the diagnostic exception vector branches to stackless code that clears
translation/cache-enable bits, writes red, and halts. It built twice with
identical SHA-256
`89ff2aebf3bdcaea71038233f9d145f19daead6b29085052c8cba4801d0fac42`; it has
not yet been run on hardware. Green indicates the first post-MMU framebuffer
stores completed; red indicates an exception reached the direct vector. An
unchanged trace still cannot distinguish an exception before vector entry from
a display-path issue, but it removes the loader stack and C handler from the
exception signal.

The next candidate replaces the already-visible stage-2 marker at row 280 with
a solid black band only after the real EL1 MMU routine returns. This makes a
positive return observable as a change to an existing band rather than a new
stripe at row 560, which was unreliable in the standalone control. Build it
with `platforms/iphone7plus/build-mmu-return-overlay.sh`; the current flat
payload SHA-256 was
`017ad12d5515e34577ac8a32944c86cd0f8f19e31418cc2610b2bf18895f7994`. It built
and passed packaging. The user later supplied this hash, confirming the payload
was run, and sent a photo with the same trace; row 280 did not appear black.
This is evidence that stage 8 was not reached, though the display path still
prevents treating it as conclusive. Do not repeat this payload.

A follow-up result payload adds a solid-red row-280 marker in the EL1 exception
handler while keeping the solid-black stage-8 return marker. Thus black means
the EL1 routine returned; red means the loader's exception handler ran; the
original row means neither visible marker appeared. This still cannot distinguish
a failure before the vector is usable from a stale payload. The candidate built
and passed packaging with SHA-256
`52276f77e57356a01604bdaf40ba7c07c10bc48c09b3074061f7cc790542c84c`. The user
verified and ran this payload but reported the original row, with neither red
nor black visible.

The user verified and ran the magenta candidate with SHA-256
`e9f685e50c68b74bf5314df7ec0e3e0de934e2815353537e72382e9f52a980ff`, then
reported the same trace. The marker used the same pink/magenta color as an
existing band, so that result cannot say whether the post-MMU drawing call ran.
The contrasting black/green/red candidate was then verified and run with
SHA-256 `ba9d527a62d1595acfbd38441a74ce9470336621dc5a256a4e0667ab9bc5e83f`;
the user again reported the unchanged trace.

The user then verified and ran the contrasting black/green/red candidate with
SHA-256 `ba9d527a62d1595acfbd38441a74ce9470336621dc5a256a4e0667ab9bc5e83f` and
again reported the unchanged trace. The framebuffer range audit confirms the
buffer fits within L1 index 34; it does not explain the missing overlays.
Therefore the screen result remains inconclusive. Do not repeat the same marker
payload. Continue by auditing the compiled loader's actual framebuffer pointer,
mapping descriptor, and exception/write path against the standalone probe.

### 2026-10-05: compiled diagnostic audit

The user uploaded `b57f7b...` and `89ff2aeb...`, verifying each checksum, and
reported the same shortened trace ending in pink. Neither establishes a
successful MMU transition. The earlier claims that the latter binary contained
a stackless vector were incorrect: disassembly of the archived uploaded image
shows that the assembler never defined `BOARD_iphone7plus`, so it excluded both
the vector override and `pongo_mmu_fault_vector`. C builds do define that macro;
assembly must use the generated `CONFIG_PLAT_IPHONE7PLUS` instead.

The actual generic vector had a second confirmed fault: it used only `ADRP` for
`exception_register_state`, whose address is `0x810004020`. It therefore saved
registers at `0x810004000`, overwriting `pongo_framebuffer_mapped` and the cached
framebuffer pointer/stride before the C exception handler tried to draw. The
new address patch adds the missing low 12 bits. The direct-vector patch also
replaces the misleading color signal with a 128x128 black square at the
framebuffer origin. Both the immediate post-MMU checkpoint and the successful
MMU-off return paint a black band at row 280. Zero and all-ones pixels were
visible in prior numeric reports; other color names have not been calibrated.

The repaired diagnostic is SHA-256
`8ecb679527ae6d86ecb7d8d663faf16d65eb506108408d0975ff29af6d5f5d02`.
`verify-mmu-diagnostic.py` checks the compiled handler exists, all 16 vectors in
the packaged raw image branch to it, it avoids stack/calls, SCTLR/ISB precede
its framebuffer stores, the fault square uses zero/black at row zero, and code matches the
inspected ELF. This check passes the repaired image and rejects the archived
`89ff2aeb...` image. The original hardware stall remains undiagnosed; the new
image has not yet been tested on hardware. Do not interpret previous missing
exception colors as proof that no exception occurred.

### 2026-10-05: unchanged repaired image; offline execution and positive control

The user uploaded the pinned `diagnostics/8ecb6795/loader.bin` and again
reported the same colored trace with no black block/band. This does not confirm
the hardware PC or MMU state. The upload source and packaged machine code have
been checked; actual execution of each modified path is still unconfirmed.

`check-loader-emulation.py` now runs the exact raw payload at a synthetic Pongo
receive address, with the observed framebuffer geometry and an EL1 Cortex-A72
CPU. The loader relocates itself to `0x810000000`, populates the expected L1
descriptors (`0x800000711`, `0x880000601`), completes the MMU enable/disable
round trip, paints black at row 280, and waits at `0x810000154`. A separate
temporary SVC injection reaches the stackless fault wait at `0x8100027e4` and
paints the expected 128x128 black square. Both emulator cases pass. Reports are
under `build/iphone7plus/emulation/`; this is generic ARM validation, not an
Apple A10/cache/display/USB model or proof of hardware success.

The next hardware image is a positive control generated by
`build-entry-vector-control.py`. It changes only three instructions in a copy
of the verified image: the two entry pixel values become zero, and the call to
`arch_init` immediately after installing VBAR becomes an intentional `SVC
#0x51`. It must halt in the diagnostic vector before attempting the MMU test.
The original upload image is preserved. The exact control bytes passed the
emulator without further injection, with SHA-256
`59c47fcee73f5ff208e7b4cfc777fc9a9de8bcb78daef34d23519f0ebb7582a0`.
The image and a three-instruction audit manifest are in
`build/iphone7plus/diagnostics/entry-vector-control/`. Expected hardware output
is a black square covering the first few bands at the top left; a small entry
band without the full square would distinguish entry from exception handling.
The control has not yet been run on hardware. If old colors remain unchanged,
investigate execution/relocation/display observation before drawing conclusions
about the MMU transition.

The user then ran the entry positive control and observed a black square about
three row lengths tall, with no colored bands. That matches the intentional
pre-`arch_init` exception: hardware execution, monochrome drawing, and the
stackless exception handler are now positively observed at this early point.
It does not establish that the vector still works after later MMU setup.

The checkpoint generator now also brackets the enable instruction:

- `before-mmu-control/loader.bin`, SHA-256
  `e45f3c5d70c5cdaa87278c5de8984bdb1766315afe39c736167cab9cf640bd02`,
  replaces the `MSR SCTLR_EL1,x8` at `0x81000129c` with SVC. A black square
  here means the loader completed cache-disable, MAIR/TCR/TTBR setup, TLB
  invalidation and barriers, reaching the point immediately before enabling
  translation, and handled the deliberate exception there.
- `after-mmu-control/loader.bin`, SHA-256
  `756f8751620c1378d765cde0f9f0030797179ab1888374463f904208f26b9917`,
  replaces the instruction at `0x8100012a4`, immediately after the SCTLR write
  and ISB, with SVC. A black square additionally demonstrates post-enable
  instruction execution and working exception entry at that point.

Both exact images passed emulator exception tests without additional injected
changes. Neither has been run on hardware yet; run the before-M control next.
Unlike the early entry control, later controls can leave earlier colored bands
below the black square. Presence of the top square is the result to report.

The user reported black followed by a very thin yellow remainder, then white,
yellow, white, pink, white, red, white, pink for the before-M control. This
matches a 128-row black square covering the first 16 rows of the band starting
at row 112, leaving its last 8 yellow rows and the later pre-MMU bands. The
before-enable checkpoint is therefore positively observed on hardware; the
cache/table/TLB setup reaches the instruction at `0x81000129c`. Next run the
already-emulated `after-mmu-control` image. Its result will test execution after
the SCTLR write/ISB and subsequent exception entry, not full kernel startup.

### 2026-10-05: after-enable control stays at the short entry band

The user ran the after-MMU control and reported only the double-height black
entry band followed by the original red/yellow/pink sequence. The 128-row
exception square did not appear. Combined with the positive before-MMU
control, this narrows the loss of observable progress to enable/ISB and the
post-enable exception path. It does not distinguish instruction-fetch failure
from failure to enter or run the translated vector, or establish the final PC.

The next image is `diagnostics/mmu-register-report/loader.bin`, SHA-256
`7d63d9f50092ccd4a70df34ac3939a94682af26b01fc1d62e32704f9b617a9ad`.
`build-mmu-register-report.py` requires the exact audited `8ecb6795` input,
checks its ELF/code and all-NOP padding, replaces the enable write at
`0x81000129c` with a branch, and inserts a 648-byte stackless numeric report at
`0x810001400`. Original code/table addresses, payload size and source artifact
are preserved. The report reads registers and table bytes with translation
and caches off, then halts; it does not change MMU registers or try the switch.

| Row | Value |
| --- | --- |
| 0 | `004D4D5550524531` (`MMUPRE1` signature) |
| 1 | CurrentEL |
| 2 | SCTLR_EL1 before enable |
| 3 | TCR_EL1 |
| 4 | TTBR0_EL1 |
| 5 | TTBR1_EL1 |
| 6 | MAIR_EL1 |
| 7 | VBAR_EL1 |
| 8 | ID_AA64MMFR0_EL1 |
| 9 | SP |
| A | Proposed SCTLR value in x8 |
| B, C | Actual TTBR0 table entries 32 and 34 |
| D, E | Entries 32 and 34 at expected table `0x810009000` |
| F | ID_AA64MMFR1_EL1 |

B/C remain all ones if the actual TTBR0 address differs from the known table;
the report avoids following an unexpected pointer. Expected descriptors are
`0000000800000711` for code and `0000000880000601` for display. This checks
CPU-visible bytes, not successful hardware page-table walks.

The exact image passed QEMU without further injection. PC reached the report
wait, registers/descriptors matched expectations, and a pixel decoder matched
all 272 glyphs to captured values. The rendered image was visually inspected.
Baseline MMU-return emulation also still passes. These generic Cortex-A72
checks do not establish Apple A10 behavior. Hardware report pending; request
one photo containing all sixteen numbered rows.

### 2026-10-05: hardware register report identifies unsupported 4 KiB granule

Photo `codex-clipboard-9f434f31-9bfe-4d9b-97c8-becf08974a5a.png` reports:

| Row | Value |
| --- | --- |
| 0 | `004D4D5550524531` |
| 1 | `0000000000000004` |
| 2 | `0000000030D00800` |
| 3 | `00000001B0903019` |
| 4 | `0000000810009000` |
| 5 | `0000000810005000` |
| 6 | `000000FF440C0400` |
| 7 | `0000000810002000` |
| 8 | `00000000F0100001` |
| 9 | `0000000810003F70` |
| A | `0000000030D00801` |
| B, D | `0000000800000711` |
| C, E | `0000000880000601` |
| F | `0000000000111000` |

The expected code/display descriptors and table pointer are present. However,
MMFR0.TGran4[31:28]=F means 4 KiB unsupported; TGran16[23:20]=1 means 16 KiB
supported. TCR.TG0[15:14]=00 selects unsupported 4 KiB. ASIDBits=0 also explains
why TCR.AS read back zero rather than the written one. This is a concrete
configuration mismatch, not yet proof there are no other hardware issues.
Arm's granule definitions and table geometries are documented in its
[memory management guide](https://documentation-service.arm.com/static/6489c105153eb247a5450414).

The earlier standalone controls contained an independent false-positive bug:
SVC records the following instruction in ELR, but their handler added another
4 before ERET. In the relocated control this skips the SCTLR-value LDR; in the
call-frame control it skips `BL .Lel1_mmu_enable` entirely. A later return stripe
therefore did not prove translation was enabled. Removed that extra add in
all three standalone sources; preserved their existing binaries for history.
Do not use the old success claims to rule out granule incompatibility.

`build-mmu-register-report.py --test-16k` now creates a terminal test with a
16 KiB-aligned L2 root at `0x810004000`. It reuses old globals/upper-table RAM
only after the original C path is finished; it never returns to C or kernel.
Code/vectors/stack stay below this region. A 36-bit VA range starts at L2 with
32 MiB blocks: entry 0x408 maps the loader, and entries 0x45D/0x45E map both
blocks touched by the framebuffer. TG0=2/TG1=1, EPD1=1, eight-bit ASIDs and
hardware-derived IPS replace the old TCR. The same MAIR layout is retained.

Success requires SCTLR.M readback=1 and nonfaulting `AT S1E1R` results for code
and framebuffer. The numeric renderer runs with translation still enabled.
Row 0 is `0031364B50415353` (`16KPASS`); row 2 should be `0000000030D00801`.
Rows B/C/D show the three L2 descriptors; E/F show PAR for code/display.
The exception path instead disables translation and reports `16KFAIL`, with
D/E/F containing ESR/FAR/ELR. `16KSTOP` indicates an unsupported capability or
failed M/PAR check. All 16 original vector entries reach that terminal handler.

Artifact: `diagnostics/mmu-16k-transition/loader.bin`, SHA-256
`0bb5e2da00e82a4907f4f4814bed2b60de9fe1c8ab73139c19556f1de5bdf49b`.
The exact image passes QEMU's generic `max` CPU (16 KiB supported), with M=1,
correct identity PARs, expected descriptors and all 272 display glyphs checked.
The original numeric report still reproduces its original hash. Phone test
pending. This addresses only the terminal loader diagnostic: the current
seL4 headers define PageBits=12 and PageTableIndexBits=9, so a full 16 KiB port
requires kernel, ABI and Microkit tooling changes rather than only a TCR edit.

### 2026-10-05: 16 KiB transition confirmed on hardware

Photo `codex-clipboard-1863cc8b-089a-459d-8103-2430ba8bc2fb.png` shows the
expected terminal report for the `0bb5e2da...` test:

| Row | Value | Interpretation |
| --- | --- | --- |
| 0 | `0031364B50415353` | `16KPASS` |
| 1 | `0000000000000004` | EL1 |
| 2 | `0000000030D00801` | Translation enabled; caches disabled |
| 3 | `00000001709CB01C` | 16 KiB granules, 36-bit low VA, 36-bit PA |
| 4, 5 | `0000000810004000` | Aligned L2 root |
| 6 | `000000FF440C0400` | MAIR |
| 7 | `0000000810002000` | Existing vector base |
| 8 | `00000000F0100001` | MMFR0 |
| 9 | `0000000810003F70` | Stack pointer |
| A | `0000000030D00801` | Requested SCTLR |
| B | `0000000810000711` | Code L2 block |
| C | `00000008BA000601` | First display L2 block |
| D | `00000008BC000601` | Second display L2 block |
| E | `4400000810001B00` | Successful code translation; PA field `0x810001000` |
| F | `00000008BB80CB00` | Successful display translation; PA field `0x8BB80C000` |

Both PAR results have F=0 and the expected identity physical addresses. The
report-rendering path leaves M enabled; this is actual translated execution
and framebuffer access, not the old SVC-skipped false positive. The terminal
test intentionally does not enter seL4. Preserve this pinned artifact as the
known-working hardware control; there is no need to repeat the color probes.

Next implementation work is coordinated 16 KiB support: kernel translation
setup and descriptors, libsel4 page/table object constants and ABI, Microkit
loader tables and image-generation/alignment assumptions. The current kernel
constants are PageBits=12, LargePageBits=21, HugePageBits=30, PageTableBits=12,
PageTableIndexBits=9. The loader has 512-entry, 4 KiB-aligned tables. A 16 KiB
port must handle 2048-entry tables and the different block sizes/levels while
preserving the existing 4 KiB QEMU build. Do not promote the terminal test's
reuse of loader globals as a table into the full boot path.


## 2026-10-05: experimental full 16 KiB boot image

The previous hardware `16KPASS` result authorized moving beyond the terminal
test. Implemented board-scoped kernel geometry (14/25-bit pages, 11-bit table
indices, one-bit 48-bit VA root), aligned table objects, 8-bit hardware ASID
allocation, corrected architectural TLBI VA operand shift (always 12), and
36-bit device PA bounds. L1 huge-page mappings are rejected in this port.
The kernel allocates only reachable high-window L2 tables, avoiding a 32 MiB
static array. Default 4 KiB geometry is retained for other platforms.

The loader uses distinct aligned 16 KiB low/high tables, 32 MiB normal-memory
blocks and the two observed display blocks, 48-bit low/high VA TCR with
8-bit ASIDs, and proceeds to seL4 instead of the old terminal MMU halt.
The Rust Microkit tool, monitor stack, image/spec placement, PD stack defaults,
and linker segment alignment now follow the page geometry. The Rust seL4
initialiser had an additional hardcoded 4 KiB copy placeholder; emulation
caught its alignment panic and it was fixed in an isolated dependency copy.

The smoke PD maps the first MiB of the observed framebuffer and prints
`16K BOOT` / `USER OK`. The plain earlier idle PD was retained. The full image
is 848244 bytes, pinned at:

`build/iphone7plus/diagnostics/16k-boot-feb3da968639/loader.bin`

SHA-256: `feb3da9686393cb9799ac3fe8a73007e1597eb6506c2983d27d8c1e7d75cebb7`

The complete build script is `platforms/iphone7plus/build-16k-sdk.sh`. Exported
incremental patches and per-file hashes are kept in the repository, separate
from cache build products. Patch reconstruction was checked against every
recorded output hash. Existing known-working hardware control remains intact.

Checks completed:

- Microkit tool: 152 tests pass, including 16 KiB XML/geometry and rejection of
  4 KiB regions for a 16 KiB configuration.
- Kernel/loader/monitor/initialiser and full smoke image compile and link.
- Default 4 KiB QEMU kernel compiles and links (existing .bss alignment warning).
- QEMU hardware breakpoints reach kernel entry, `init_kernel`, initial address
  space creation, Rust initialiser, thread startup, and smoke screen completion.
- The last check compares task code and physical screen pixels; matching a
  virtual address alone was insufficient because the monitor shares that VA.
- QEMU counter frequency is explicitly 24 MHz to match this port. With its
  default frequency, the bootstrap budget exhausted and the timer-only Apple
  interrupt shim could not wake it on the generic virtual interrupt controller.

These are generic-emulator results. Full hardware boot, timer/FIQ delivery,
cache behavior, and sustained operation remain unverified. The memory fixture
is still based on prior measured free RAM; this is an experimental smoke port,
not a complete phone OS or a formally verified extension of seL4.


## 2026-10-05: full smoke task confirmed on iPhone 7 Plus

The user uploaded the pinned 16 KiB full boot image and supplied a clear
screen photo (`build/iphone7plus/diagnostics/16k-boot-feb3da968639/hardware-2026-10-05.png`).
It shows the protection domain's **`16K BOOT` / `USER OK`** white text on its
black region, below which earlier loader/kernel colored checkpoints remain.
Only the user task writes this text. This confirms that the full path reached
seL4, the Rust CapDL initialiser, Microkit's task startup, and a user-space
framebuffer write on the phone. The photo identifies behavior, rather than
cryptographically proving which file was uploaded; the preceding instruction
specified the `feb3da968639...` artifact.

The next separate milestone is timer-driven scheduling: this static screen
cannot show that A10 timer/FIQ delivery replenishes a runnable task, nor that
two protection domains exchange execution over time. Sustained runtime and
broader device integration remain open.


## 2026-10-05: timer-driven scheduling probe prepared

A separate Microkit image tests the next boundary. Protection domain
`spinner` has priority 100, a 20 ms budget and 200 ms period. It writes the
same two-line boot result, then runs an infinite userspace loop without a
system call or voluntary yield. Protection domain `observer` has priority 99
and writes `TIMER OK` at screen row 184. The lower-priority task cannot run
while the spinner remains runnable; the third line therefore tests budget
expiry and timer-driven scheduling. Both share the first MiB of the observed
Pongo framebuffer using separate mappings. No IRQ or timer register is exposed
to userspace in this probe.

Pinned image: `build/iphone7plus/diagnostics/16k-scheduler-ba37c2b5dbc3/loader.bin`

SHA-256: `ba37c2b5dbc3addb4a39aae638e5df07ba3a8dc765b97de13bc98d1264d5647d` (885740 bytes).

`platforms/iphone7plus/build-16k-scheduler-probe.sh` builds and tests it. QEMU
reaches the spinner and verifies the first two screen lines. The observer does
not run within the 8-second emulator window. QEMU's generic virtual interrupt
controller does not model the Apple A10 timer/FIQ routing used by this board
shim, so this result leaves hardware timer delivery open. The phone run is
needed to interpret the third line. The previously hardware-proven
`feb3da968639...` boot image has not changed.

The phone run on 2026-10-05 showed `16K BOOT` and `USER OK`, with no
`TIMER OK` (saved in the pinned diagnostic directory). This confirms the
spinner started but does not establish why the observer did not run. A
QEMU control image blocks the spinner on its input cap; the observer then
runs and draws a third line. A second QEMU diagnostic makes the spinner
enter the kernel periodically with `seL4_Yield`; after budget exhaustion,
the observer runs and draws `TEST OK`. Thus the task setup and budget
accounting work when there is a kernel entry. The phone's asynchronous
timer/FIQ delivery is the remaining question.

The syscall diagnostic is pinned at
`build/iphone7plus/diagnostics/16k-scheduler-syscall-1abee24c4de8/loader.bin`,
SHA-256 `1abee24c4de87ea44897dd2a7db2ce8c8cf677ff3a459081dada258441d6ca0d`
(885740 bytes). Steve reported `TEST OK` visible on the phone, with some
display bleed. The lack of `TIMER OK` in the non-yielding image therefore
implicates timer arming, FIQ delivery, or FIQ entry handling rather than
observer setup or budget accounting. The report is visual/verbal and does
not cryptographically identify the uploaded file. Repeating either image
adds no information.

### Timer enable lost during platform initialization

Source audit after the `TEST OK` phone result found a specific ordering bug.
`init_cpu()` calls `initTimer()` and enables `CNTV_CTL_EL0`. Then `init_plat()`
calls the iPhone `initIRQController()`, which previously wrote only `IMASK`
to that register, clearing `ENABLE`. `init_irqs()` subsequently unmasked the
timer by clearing `IMASK` but did not restore `ENABLE`. This leaves the timer
disabled throughout user execution and explains the absence of `TIMER OK`.

The board-specific fix reads `CNTV_CTL_EL0`, sets `IMASK`, and writes it back,
preserving `ENABLE`. It is included in
`patches/sel4-16.0-iphone7plus-timer-fiq.patch`. The kernel compiled, the
Microkit tool's 152 tests passed, and compiled `initIRQController()` contains
the expected `MRS CNTV_CTL_EL0`, `ORR #2`, `MSR CNTV_CTL_EL0` sequence. QEMU's
generic interrupt controller routes timer events differently from Apple's FIQ
path, and the full QEMU boot check stalled after kernel initialization with
the timer enabled; it cannot certify the phone outcome. The fixed image is
`build/iphone7plus/diagnostics/16k-timer-enable-2e6ad7b59d17/loader.bin`,
SHA-256 `2e6ad7b59d17d20aff38b2ffd7edd379e80583f06dfd57248b3384ce77f99314`
(885740 bytes). Steve reported `TIMER OK` present on the phone. The saved
photo shows faint pixels at the third-line location, but the line starts at
row 184: its first eight rows lie over the dark header and its remaining
white glyphs lie over a white background. This screen layout explains the
bleeding/incomplete text. The observer source now paints a dark background
under rows 184–239 for future builds; the pinned tested image is unchanged.
The result supports timer-driven scheduling of the observer at least once,
but does not demonstrate sustained periodic interrupts or cryptographically
bind the photograph to the uploaded binary.

## 2026-10-05: first NexatrodeOS service image ready for phone trial

The minimal phone profile at `platforms/iphone7plus/nexatrode-minimal.system`
contains two actual OS components: the existing Rust calculator service,
rebuilt against the 16 KiB SDK, and a C home client. The home client writes
`NEXATRODE OS` and `RUST` to the observed Pongo framebuffer, then sends a
Microkit protected call with operands 19 and 23 to the Rust service. A correct
reply draws `CALC 42`; a wrong reply draws `IPC FAIL`. If execution stops
before the call returns, only the first two lines remain. This establishes a
concrete cross-PD OS service milestone without another synthetic color probe.

Build with `sh platforms/iphone7plus/build-nexatrode-minimal.sh`. The 16 KiB
Rust target is `rust/targets/aarch64-sel4-microkit-iphone16k.json`. The build
passed Rust and C compilation, Microkit image generation, 16 KiB ELF segment
alignment checks, and framebuffer bounds checks. Its Microkit report lists
both tasks and the protected-call endpoint. The pinned image is
`build/iphone7plus/diagnostics/nexatrode-first-service-264c3f5d8972/loader.bin`,
SHA-256 `264c3f5d8972f122dc3a73a5c090d1a4bdc5b6e7f2d63986437989b2633c7a8c`
(916516 bytes). It has not yet run on the phone. The generic QEMU check reached
kernel address-space setup but stalled before the initialiser, consistent with
the known Apple FIQ versus QEMU GIC mismatch in this timer-enabled SDK; that
emulator attempt is not a phone boot result.

From a fresh Pongo session, upload the pinned image with
`./build/iphone7plus/load-pongo-raw-probe ./build/iphone7plus/diagnostics/nexatrode-first-service-264c3f5d8972/loader.bin`.
The existing upload helper can report `LIBUSB_ERROR_TIMEOUT` after the
transfer; decide whether this boot ran from the phone screen, as with the
earlier images. A `CALC 42` screen would prove that a real NexatrodeOS Rust
service and client communicate on hardware. It would not yet be the full
interactive QEMU profile: that profile still contains 4 KiB regions and
QEMU-only UART, RTC, VirtIO, graphics, and disk device assumptions. The
framebuffer address in this first phone profile is the previously measured
Pongo address and should eventually come from the boot handoff dynamically.
The 16 KiB Rust target and Microkit image are iPhone-specific; a future Pixel
4a profile should use its own board configuration and measured translation
granule. Do not change the shared QEMU/Pixel build to 16 KiB as part of this
iPhone bring-up.

Steve then reported that the phone displays `NEXATRODE OS`, `RUST`, and
`CALC 42`. `CALC 42` is written only after the home client receives the
expected protected-call reply from the Rust server. This is the first
hardware-observed NexatrodeOS two-component service boot. The report is
verbal and does not independently bind the displayed result to the pinned
image hash. A separate upload attempt from this workspace timed out on its
first Pongo USB control request after Steve had already observed the screen;
that attempt is not counted as the successful upload. The full interactive
profile and device drivers remain future work.

## 2026-10-05: app shell hardware trial

The next phone image replaces the calculator demo with the existing Rust
Settings and Phone application protection domains. Each application owns a
16 KiB view region and publishes its real `AppViewState` text. A small display
client invokes both applications through protected calls, reads the two
snapshots through read-only mappings, and renders them side by side. The
Settings build has an iPhone-only `iphone-shell` feature that says `VAULT WAIT`
because no vault is connected; the Phone panel is labeled `PHONE DEMO` because
there is no real modem or input driver yet. Neither QEMU MMIO nor a fictitious
storage/telephony backend is mapped.

Build: `sh platforms/iphone7plus/build-nexatrode-shell.sh`.
Pinned image: `build/iphone7plus/diagnostics/nexatrode-shell-75b8236d1263/loader.bin`.
SHA-256: `75b8236d12633fa13ea3471c660b5ce4167d24a904f2b3866c3390e67669ecc0`
(984980 bytes). The 16 KiB Rust binaries and Microkit image built; the
Settings formatter passed its three standalone tests both with and without
the iPhone feature, and screen bounds/font coverage were checked. Pongo
accepted the full payload transfer and the subsequent `bootr` request timed
out, as in earlier phone runs. Steve reported that the iPhone screen displayed
`NEXATRODE OS` with both `SETTINGS` and `PHONE DEMO` panels. Those panels are
drawn only after both Rust app protected calls reply and the shell reads both
app-owned snapshots, so this confirms the three-PD app shell on hardware. The
screen report is verbal and does not independently identify the uploaded file
hash. The next distinct milestone is real device input to switch and edit app
views; the existing QEMU input path is not an iPhone touch driver. Local source
references include the iPhone D11 device tree in Asahi Linux (with button GPIOs
but no touchscreen node) and Project Sandcastle's `hx-touchd` Z2/SPI touchscreen
support. Storage-backed Notes will also need an iPhone block-device and crypto
path; the QEMU vault currently expects VirtIO and must not be mapped to phone
hardware addresses by guesswork.

### Touch driver groundwork

The separate Project Sandcastle Linux kernel tree does contain a D11 device
tree with a touch node, unlike the earlier local Sandcastle utilities checkout
discussed above. At commit `0c2f7dda13d67bb7e06123df516f8bdb1000a79b`
it identifies SPI2, GPIOs 161/81/166, touch/SPI clocks, two power rails,
and the interrupt path. The kernel touch driver delegates firmware loading to
the `hx-touchd` daemon, which needs a model-specific `.mtprops` personality and
SysCfg calibration data. Both are now available locally. The source audit
and exact values are in `platforms/iphone7plus/touch/README.md`.

A bounded, transport-independent report decoder is now implemented in
`platforms/iphone7plus/touch/report.c`. A separate SPI reply parser in
`packet.c` checks the header, tag, size, and payload checksum before the
report parser is called. Host tests cover malformed and maximal packets,
multiple contacts, release state, signed coordinate normalization,
truncation, excess contacts, and invalid calibration. Both sources compile as
freestanding AArch64. They are not connected to the phone shell, and no touch
MMIO or power registers were changed. The newer HoolockLinux port reports
working touch on `iPhone9,4` / D111, whereas this phone is `iPhone9,2` / D11;
its firmware/calibration must not be presumed interchangeable. The next
hardware slice is an isolated controller service using verified D11 resources
and the correct firmware/calibration.

### D11 touch firmware identified from local IPSW

The supplied `iPhone_5.5_P3_15.8.8_19H422_Restore.ipsw` was inspected
read-only. Its BuildManifest contains both D11 and D111 identities. The D11
DeviceTree reports `iPhone9,2` / `D11AP`, `hid-transport,k1` on SPI2, and
firmware personality `C1F5E,2`. The OS filesystem contains
`usr/share/firmware/multitouch/D11.mtprops`. The extracted firmware is 116,503
bytes with SHA-256
`7f9b16b749dd0f3e12633ce5140220ea89317006dc0278b24cf2d815578e3ff6`.
Its personality matches the D11 tree and its calibration sequence requests
SysCfg `MtCl`, `OrbG`, `OFCl`, and `SDAC`. The local
`platforms/iphone7plus/touch/validate-assets.py` tool checks this pair and
rejects the D111 tree. The extracted Apple files are kept under ignored
`.cache/iphone7plus-touch-fw/`, not committed to the project.

The paired iOS 15.8.8 phone exposed `SysCfgData` through
`ideviceinfo -q com.apple.mobile.internal -k SysCfgData -x`. Two read-only
queries matched byte for byte. The bounded parser at
`platforms/iphone7plus/touch/syscfg-calibration.py` verified its `gfCS` index
and extracted the four private touch records: `MtCl` (852 bytes), `OrbG`
(992 bytes), `OFCl` (1,474 bytes), and `SDAC` (208 bytes). They are stored with
mode `0600` in ignored `.cache/iphone7plus-touch-fw/calibration/`; full SysCfg
copies were removed. This lockdown query worked independently of the
jailbreak's SSH service. Firmware and calibration availability do not imply a
working touch driver: power, clock, SPI, firmware upload, interrupt handling,
and app input routing remain to be implemented and hardware-tested.

### Prepared touch boot stream and SPI2 read probe

The D11 firmware's calibration sequence names exactly four providers. The
matching private SysCfg records fit their declared maximum sizes, including
the 1,474-byte `OFCl` under its 1,476-byte limit. The host-side
`platforms/iphone7plus/touch/build-boot-stream.py` tool transformed these
assets into a 16-operation, 80,230-byte private boot stream at
`.cache/iphone7plus-touch-fw/boot-stream.bin`. Its SHA-256 is
`a063e426b030fc106ebdd8939de601a9244f9570ccd81c5db2c8c6bd224a636a`.
The `NTFW` format keeps plist and XML parsing off the target; its freestanding
`boot_stream.c` reader validates the entire sequence and bounds before
handing records to a future SPI driver. Host tests and AArch64 compilation
pass. The stream has not been sent to the touch controller.

`platforms/iphone7plus/touch/spi2-read.system` is the next hardware check. It
maps only the D11 SPI2 register page at `0x20a088000` with read permission and
displays five register values on the known Pongo framebuffer. Its flat Pongo
image is built at `build/iphone7plus/spi2-read/loader.bin` (848,356 bytes).
The first upload reached `SPI2 READ CLKCFG` with no value. Its first SPI2
register read did not complete. A later Pongo control request timed out, so
a new DFU/Pongo session was needed. A fault-aware follow-up image at
`build/iphone7plus/spi2-diagnostic/loader.bin` gave the SPI2 reader a parent
fault handler. It displayed `FAULT`, type `6` (seL4 VMFault), address low word
`0A088000`, and FSR `92000018`. The AArch64 exception class is a data abort
from lower EL and DFSC `0x18` means a synchronous parity/ECC error. The
first diagnostic only displayed the low 32 bits of the fault address. The
rebuilt diagnostic confirmed pointer `0x41000000`, SPI2 clock-gate value
`0x00000A00` at `0x20e0801d8`, full fault address `0x20A088000`, and the same
FSR. In Sandcastle's PMGR driver, status bits 4–7 equal `0xF` when the gate is
enabled; the observed value has those bits zero. The next isolated image,
`build/iphone7plus/spi2-enable/loader.bin`, follows Sandcastle's bounded gate
enable sequence and reads SPI2 only after the enable status appears. It writes
the clock-gate register, but leaves SPI2 itself read-only and does not reset
touch power.

The first `spi2-enable` hardware run showed gate `BEFORE A00`, `AFTER FF`,
and all five SPI2 values without a fault: CLKCFG `0`, CONFIG `8`, STATUS `0`,
PIN `3`, CLKDIV `1`. This confirms that enabling the SPI2 clock gate made the
controller's mapped registers readable.

The touch port now has a freestanding, polling SPI2 PIO transport in
`platforms/iphone7plus/touch/spi2_transport.c`. It follows the Sandcastle
controller's 16-byte FIFO and register setup, uses the D11 24 MHz SPI2 parent
clock to select a rate no faster than the touch node's 3 MHz limit, and has
bounded timeout and stale FIFO checks. A host fake-controller test and an
AArch64 freestanding compile pass. No SPI data transfer or firmware upload
has yet been attempted on the phone. The remaining
hardware path is touch power over two I2C PMUs, touch clock and GPIO setup,
then a controlled short transfer and firmware boot stream.
An isolated `spi2-setup.system` image is built at
`build/iphone7plus/spi2-setup/loader.bin` (902,684 bytes, SHA-256
`7dc3563439a005405f477bc74a71ee4fe5246d598029f52e16d3c550f2426b30`).
It exercises only SPI2 controller configuration and register readback.
The first phone run reported BEFORE `0xA00`, AFTER `0xFF`, CLKCFG `0x1`,
CONFIG `0x0010401E`, STATUS `0x30`, PIN `0`, and CLKDIV `8`, with no fault.
The controller accepted the register setup and 24 MHz / 8 divider; CLKCFG
readback differs from the written `0xD`, and no SPI data transfer has yet
tested whether that difference matters.

The next image, `build/iphone7plus/touch-hw-probe/loader.bin` (886,524 bytes,
SHA-256 `55948371bf03343376310deac9e4e2a791c6adf74f6573fd707da01da474064c`),
is built from `touch-hw-probe.system`. It enables I2C0 and I2C1 controller
gates, reads both revision registers, and displays the current touch clock,
reset, chip-select, and IRQ GPIO registers. The two I2C blocks fit within one
16 KiB mapping. It does not transact with either PMU or alter touch power.
`hw_control.c` supplies tested PMGR/GPIO routines for the future touch service;
the on-device I2C/GPIO readback showed I2C0 and I2C1 gates `0xFF`, both
controller revisions `3`, touch clock `0x800002EE`, and GPIO reset/CS/IRQ
registers `0x78A02`, `0x78A21`, and `0x7CB80`, with no fault. The I2C pointer
was `0x43000000`.

`i2c_pmu.c` and its host test implement a bounded register-select/read
operation for the display PMU at I2C0 address `0x27` and the core PMU at I2C1
address `0x74`. The next image, `touch-pmu-probe.system`, configures those I2C
pins and reads the existing rail-control bytes without writing values. The
flat image is `build/iphone7plus/touch-pmu-probe/loader.bin` (886,412 bytes,
SHA-256 `d62645f0f79548daa082d9e88b926bfaaf0cc6093e4eef774427bf59b00a9ee6`).
The on-phone result was HV register `0x05 = 0x0F` and core register
`0x031F = 0x01`, with no ERR or FAULT. The touch HV enable bit `0x10` is off;
the core switch bit `0x01` is on. The display PMU's low nibble remains set,
so an HV enable write must preserve those bits.

The next standalone image, built with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh hv`, performs one
bounded touch-HV bit test. It stops before writing unless the core switch is
on and display-PMU register `0x05` is exactly `0x0F`; it holds touch reset
low, reads back the masked enable (`0x1F` expected), then attempts to clear
the touch bit and reads back the original `0x0F`. It sends no SPI data. The
image is `build/iphone7plus/touch-hv-probe/loader.bin` (886,412 bytes,
SHA-256 `c320b262e2b7b0ad891f6dc6b27814ea4f8387b5e2bfc70f4755644d5658e388`).
The phone showed `CORE 31F = 0x01`, `HV BEFORE = 0x0F`, `HV ON = 0x1F`, and
`HV OFF = 0x0F`. This verifies the masked PMU touch-HV control path and
successful restoration. The loader's Pongo `bootr` request timed out, but
the diagnostic clearly ran on the display.

`touch-startup-probe.system` is the first combined power, clock, reset, and
SPI2 image. It requires the previously observed core, HV, and disabled-clock
values, performs the Sandcastle-style reset and a four-byte zero SPI exchange,
displays the received bytes, then disables the clock and restores touch HV.
Its SPI transfer has a finite poll limit, and cleanup runs on success or
transfer failure. It sends no firmware. Build with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh startup`; the image
is `build/iphone7plus/touch-startup-probe/loader.bin` (886,636 bytes, SHA-256
`29fe429d9558f109ef15bb41566bb1a15d8a79772183bdb8dc0fe053da344cc2`).
The initial version displayed seL4 fault label `3` (UserException). It used
`CNTVCT_EL0` for delays, but `CONFIG_EXPORT_VCNT_USER` is disabled in this
kernel build; the counter access is the likely fault. The new image removes
all `CNTVCT_EL0`/`CNTFRQ_EL0` instructions and uses short conservative
instruction loops. The fault monitor now reports user-exception IP and
number if another fault occurs.
On the corrected phone run the clock read `0x000802DC` (enable bit set,
divider `0x2DC`), SPI2 reached `READY`, the four-byte receive value was
`0x1F011F01`, and the display-PMU HV byte returned to `0x0F`. No FAULT
appeared. This validates a bounded SPI2 transfer and normal cleanup, not
touch firmware boot or report delivery.

A separate `firmware` build now embeds the private, checksum-verified
80,230-byte D11 boot stream in an ignored image. `boot_execute.c` validates
all 16 records before use, transmits at most 16 KiB per SPI call while CS
remains asserted for a record, sends each two-byte ACK command, and releases
CS on errors. Host tests cover chunking and cleanup. The image repeats the
working startup, sends the stream, waits on the GPIO controller's latched
falling-edge status (or active-low pin level) for at most 500 ms, and
restores reset, clock, and HV afterward. `STREAM SENT` will not by itself prove report
delivery. Build with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh firmware`. The image
is `build/iphone7plus/touch-firmware-probe/loader.bin` (969,116 bytes,
SHA-256 `841e8cf1d13ff38ccbf15932b611ad23d0e83a579f9a75ad3f8b100d6113025f`).
The first on-phone firmware run completed the SPI records and stopped at
record `0xF`, type `3` (final `WAIT_IRQ`) with result `0xFFFFFFFD` (IRQ
timeout). Boot IRQ read `0x7CB80`, sync RX `0x1F011F01`, and HV restored to
`0x0F`. The revised image configures GPIO 166 for a falling-edge interrupt
and reads the Hx GPIO block's latched group-zero pending bit at offset
`0x814`, clearing the boot edge before firmware transmission. Its first
build faulted when drawing an extra result at row 240, beyond the mapped
framebuffer. The corrected image keeps that result inside the mapped area.
The corrected firmware image reached `FW RESULT 0`, latched falling-edge
`IRQ 1`, and `STREAM SENT`, with normal cleanup. The IRQ latch was cleared
before all stream records, so its edge could have arrived earlier than the
final wait. This verifies that records were sent and an edge occurred, but
does not yet prove the controller finished booting or produced a valid report.
The next `report` mode sends the generation-2 wake and D9 report-information
commands, checks controller status/length, reads the first 16 D9 bytes, and
displays the calibration bounds before cleaning up. The host-tested image is
`build/iphone7plus/touch-report-probe/loader.bin` (969,116 bytes, SHA-256
`5cdce73744741ac6a875543e51194c388da01fb592ac5247819012d91fbd6d57`).
The first phone run showed `FW IRQ 1`, but its D9 query stopped at stage `3`
after the second report-information command. On the follow-up run, the raw
response consisted entirely of
`4F814F81` repeated; D9 stage `3` reported status `0x4F` and length
`0x4F81`. HV was restored to `0x0F`. This is not a valid D9 report. The
next image prepared the SPI2 controller before every generation-2 message,
as Sandcastle's SPI driver does for `spi_sync_transfer`. The phone returned
the same D9 error and repeated bytes, ruling out that setup difference.
The next image recorded the first eight response bytes from wake, the first
E3/D9 command, and the second E3/D9 command separately. On the phone, both
E3/D9 responses repeated `4F814F81`, and most of the wake response already
had that pattern. The current image also records the first stream-record
boundary with a pending touch IRQ, pending state before the final wait, and
the final two-byte acknowledgement response. The phone reported `IRQ PRE 1`,
so its final wait consumed an IRQ already pending before that wait. The first
record index appeared to be zero but was clipped at the photo's edge; the
user read `ACK14` as `0x4AD1`. The current revision moves `FIRST` into a
fully visible row and shows `POST ACK`, the pending state immediately after
the boot IRQ is cleared. The phone showed `POST ACK 0`, `FIRST 3`, and
`ACK14 0x4AD1`; `D9 RESULT` remained `0xFFFFFFFD` with repeated
`4F814F81` RX. This confirms that the boot edge was cleared and a new edge
appeared during the large firmware write at record 2, before the final IRQ
wait. That edge alone does not establish successful firmware boot. Source
comparison found that Sandcastle prepares its SPI controller for every
16 KiB firmware transfer while our path only prepared it once at startup.
The current source corrects this and captures several per-record ACK
responses to evaluate the next phone run. The phone still returned
`D9 RESULT 0xFFFFFFFD` with repeated `4F814F81` control responses after
per-transfer SPI2 preparation; the early and late ACK responses were
nonzero and varied. The current image instead keeps the panel powered for
a longer interval and compares immediate and late D9 responses during one
boot to test whether the invalid reply is a readiness-timing issue.
The phone returned the same immediate and late D9 error `0xFFFFFFFD`,
status `0x4F`, length `0x4F81`, and repeated RX word `4F814F81`;
`LATE IRQ` was `1`. Waiting longer did not fix the control response.
Independent host comparison against Sandcastle's `mtfw.c` builder found
the real stream discrepancy: our XML parser did not resolve `IDREF` values,
so five boot-sequence register writes had zeroed masks or values. The parser
and regression test are fixed. The regenerated private stream is 80,230
bytes with SHA-256
`c4157ef64c0901f7e8aa14768e8173ab76ac406fc54e668f1f64b2eb3d74a0b3`
and matches Sandcastle's output byte for byte. The new ignored report loader
SHA-256 is
`479c1a1243e38e347a64a19a49065a583651f814d6e7365279e6e0ae4bc3d51a`
when tested on the phone;
host checks and build pass. The phone run showed `FW IRQ 1`, `D9 RESULT 0`,
`D9 STAGE 5`, `D9 STATUS 0`, and `D9 LENGTH 16`; the probe read the first
16 D9 bytes and restored HV to `0x0F`. The previously invalid repeated
response was caused by the bad stream. The next hardware milestone is a
contact IRQ and checksum-verified packet read, followed by live input routing.
An isolated event probe for that next milestone is built at
`build/iphone7plus/touch-event-probe/loader.bin`, SHA-256
`e0a899e61baf66fb022cef019c444fd8fd066eeabad32a030cc3bbf8c2be82ab`.
It sends the generation-2 `AF` report-enable command, waits while showing
`TAP SCREEN`, and validates a bounded packet before decoding contact position.
The first event image reached D9 again but showed `EVENT RESULT 0xFFFFFFFA`
(`NO_EVENT`), zero IRQs, and no packet after an overly long wait. The revised
image bounds the wait more tightly and checks both the GPIO's active-low line
and its latched falling-edge bit. Host tests pass. The revised image read a
checksum-valid 14-byte empty event packet: `D9 RESULT 0`, `EVENT RESULT 0`,
one IRQ, and zero contacts. The user was uncertain whether they tapped during
the capture, so this proves the event transport but does not yet prove active
touch contacts. A deliberate repeat showed why the display ended before the
user could tap: the first IRQ delivered a valid 14-byte status packet with a
12-byte payload. The probe incorrectly tried to decode it as a contact report
and stopped. The current image skips such short status packets and continues
waiting for a contact. The phone then showed `D9 RESULT 0`, `EVENT RESULT 0`,
three IRQs, a checksum-valid 56-byte frame with one contact, normalized
`X 968`, `Y 265`, and HV off at `0x0F`. The hardware touch path is proven
through contact decoding.
The first integrated shell image is built with
`sh platforms/iphone7plus/build-nexatrode-shell.sh touch`. It adds a
low-priority touch PD, a shared state page, and a notification channel to the
existing Rust Settings/Phone shell. The driver polls GPIO 166 because this
seL4 iPhone kernel currently exposes only its timer IRQ to user space. A
verified fresh contact selects the left Settings or right Phone demo view.
The private firmware stream remains ignored. The first touch-enabled image
had SHA-256
`47dc1e8c9264e00a9a256b8eafdbcac31c4928c21f016b8f1e42051f8cc43a38`;
host tests and the image build pass. On the iPhone, the shell showed the
Settings and Phone demo views and `TOUCH READY`. A left-half tap selected
the single Settings view; after release, a right-half tap selected the
single Phone demo view. This is the first confirmed live D11 touch-to-app
path in NexatrodeOS. The next touch shell expands the mapped framebuffer to
its full 1080 x 1920 dimensions and renders a launcher plus large Phone demo
keypad. Digit, CLR, and DEL buttons send routed key events to the Rust Phone
PD, which updates its own shared view. That first full-screen keypad image
had no modem PD and was 1,153,628 bytes,
SHA-256 `dd6ec8f1c7abacff9ad19acb6ed4d6ccf7b4afc9f573ae6ec59b31943d014258`.
On the iPhone, the launcher filled the display and showed `TOUCH READY`.
Opening Phone and tapping 5 showed `DIAL`, `5`, `RING 0 SMS 0`; DEL returned
to `IDLE`, and HOME and SETTINGS navigation worked. The current driver polls
the GPIO IRQ state; direct user-space external IRQ delivery and broader input
routing remain future work.
The next candidate adds the existing Rust fake-modem PD on a Phone-only dial
channel and a `CALL DEMO` button. It can only validate the protected-call
path and display `CALLING`; it has no cellular hardware access. The ignored
`build/iphone7plus/nexatrode-touch-shell/loader.bin` is now 1,221,676 bytes,
SHA-256 `23fb95b0fe314f10698587fecd3cdb93c2a5ae277b9de8966f49c636deeaa4bb`.
Static and touch builds pass. On the iPhone, the user entered `56` and
tapped `CALL DEMO`; the Rust Phone view changed to `CALLING`. This confirms
the touch shell -> Phone PD -> fake modem PD -> Phone view round trip on
hardware. It does not place a cellular call.

An isolated native Slint Notes image can now be built with
`sh platforms/iphone7plus/build-nexatrode-slint-notes.sh`. It combines a
`no_std` Slint Notes protection domain with the proven D11 touch service and
renders through Slint's line software renderer into the full 1080 x 1920 Pongo
framebuffer. The first phone run displayed the actual Slint screen, with a red
background and magenta editor. A second run with `RRGGBBAA` words showed a
strongly blue background and a visible Save button, but a tap left the status
at `SAVED LOCALLY`. The next image retained the last contact position for
Slint pointer release; the D11 service reports zero coordinates when a finger
lifts. On the phone, the user then saw `SAVED THROUGH TOUCH`, which only the
Save callback sets. The user was unsure of the exact tap location at that
point. The tested image is 1,738,580 bytes,
SHA-256 `950a1fc1cada41679e6cb5c05a54e59fdf8633f8ca718ab898c315887cf65531`.
The image is
separate from the working multi-app touch shell. This Notes screen has no
persistent storage or keyboard yet; its status text does not imply a saved file.
The user observed a visible top-to-bottom wipe while the first Notes frame
appeared, but not after Save. A candidate image with Slint reused-buffer
dirty-region rendering and non-volatile writes to framebuffer RAM is preserved
under `build/iphone7plus/diagnostics/41ccfd8d/`, SHA-256
`41ccfd8dfdb9794079279a687e482510e14b50bed0f59f1b24149848a5430089`.
The user missed the first paint on a successful boot, so its first-frame speed
has not yet been measured on hardware. The last confirmed Save callback image
is preserved under `build/iphone7plus/diagnostics/950a1fc1/`. The same boot
showed that Save responded when tapped slightly to the left, motivating a
coordinate check. A subsequent image overlaid a yellow marker at the received
touch position. The user deliberately tapped the center of Save; the marker
appeared on the button and the status read `SAVED THROUGH TOUCH`. This confirms
alignment at Save, so no calibration offset was added. The diagnostic image,
SHA-256 `560b2d111d21a780e5057556e13203405e1ab3f77dd382a1c6f71ab5df1339b0`,
is preserved under `build/iphone7plus/diagnostics/560b2d11/`. The normal
image no longer draws the marker.
The last marker-free image before staging is preserved under
`build/iphone7plus/diagnostics/87611ce9/`, SHA-256
`87611ce90ac0d24815537106f692725e0f8c0cac3dcffb841bfbc6565fbab619`.
An additional candidate rendered into a separate cacheable 1080 x 1920
stage buffer before copying the dirty region to the live Device-mapped
framebuffer with 32-bit stores. Its SHA-256 is
`c8aa19bd81187d1563958a6d6b3b2e1e8f528cf3aae9be8280aac247c4387246`.
On the phone, the user saw the same first-frame wipe and confirmed Save still
worked. The staged image is preserved under
`build/iphone7plus/diagnostics/c8aa19bd/`, while the normal build was
returned to the smaller direct-rendering image with SHA-256
`87611ce90ac0d24815537106f692725e0f8c0cac3dcffb841bfbc6565fbab619`.
The comparison indicates that the visible sweep is dominated by presenting
pixels to the Device-mapped live framebuffer, not by rasterizing Slint's scene.
Removing it likely requires a faster scanout write path or a framebuffer flip;
neither has been validated on this phone.

The next native Notes candidate addresses the very small text seen on the
phone. Slint's software renderer embeds bitmap glyphs at build time; the
previous image did not include glyphs at the physical sizes needed for the
iPhone's 2.6087 scale. The Notes build now embeds those sizes and gives the
heading, editor, status, and Save control larger type with higher contrast.
Host rendering and Save input checks pass at the iPhone resolution, at
300 x 400, and with a reserved keyboard area. The candidate is preserved at
`build/iphone7plus/diagnostics/2c8d8703/loader.bin`, SHA-256
`2c8d8703962c9eaf15d7836943a0f34e2690472381c3337bb66c91406074c657`.
The phone displayed the larger heading, note, status, and Save label without
clipping, and a tap changed the status to `SAVED THROUGH TOUCH`. This confirms
that the larger embedded glyphs and Save touch path work on hardware. The
first-frame wipe remains a separate scanout performance issue.
The user found the smaller `PERSONAL OS` and status labels somewhat faint.
A follow-up candidate uses pure white, larger type for those two labels;
its host preview and Save checks pass. The candidate is preserved at
`build/iphone7plus/diagnostics/16262479/loader.bin`, SHA-256
`16262479dcd2094173c55405a391c4deac69808555d26c9cfebd32c194003192`.
On the phone, the user found these mostly better but still called the smaller
tinted subtitle unclear. A third candidate renders the subtitle and footer
larger and white, retaining smaller sizes at 300 x 400 so text does not clip.
Host layout and Save checks pass. The image is preserved at
`build/iphone7plus/diagnostics/5d6e85d6/loader.bin`, SHA-256
`5d6e85d6475151a600d1104569169c9bec1dbd8f3430fbce2d1261eb6a324bff`,
and was booted on the phone. The user found only slight residual softness in
the smaller labels and confirmed Save still works. The latest phone photo
shows clear large text; broad vertical shapes appear to be glass reflections
and do not by themselves establish framebuffer corruption.

Sandcastle's Linux [`simplefb` driver](https://github.com/corellium/linux-sandcastle/blob/sandcastle-5.4/drivers/video/fbdev/simplefb.c#L465-L469)
maps its framebuffer with `ioremap_wc`,
whereas this seL4/Microkit profile maps the screen as `cached="false"`, which
becomes Device-nGnRnE in seL4's AArch64 page-table code. Sandcastle's Android
gralloc patch sets `NUM_BUFFERS` to one, so it does not establish a supported
page-flip path. The kernel MAIR has a Normal-NC slot, but the current seL4
cached boolean selects only Normal WB or Device. An isolated, screen-only
Normal-NC mapping is the next performance experiment to compare with the
preserved Device-mapped image; MMIO maps must stay Device. The effect on this
phone had not yet been tested when this candidate was prepared.

For that comparison, `patches/sel4-iphone7plus-screen-normal-nc-experiment.patch`
changes only the iPhone 7 Plus kernel's user-page mapping for the known Pongo
framebuffer interval `[0x8bb80c000, 0x8bc004000)`. It selects the existing
`NORMAL_NC` MAIR entry there while preserving Device-nGnRnE for all other
`cached="false"` mappings, including MMIO. The separate builder
`platforms/iphone7plus/build-screen-normal-nc-experiment.sh` clones the
prepared kernel source into a new cache directory, builds a separate SDK, and
packages the unchanged Slint Notes and touch-service protection domains. The
candidate raw image is
`build/iphone7plus/nexatrode-slint-notes-normal-nc/loader.bin`, SHA-256
`ff096c9cf2b410ad1181d92f80efc558c149f3454636809e64cee615f42cd8e0`.
The working Device-mapped baseline remains
`build/iphone7plus/diagnostics/5d6e85d6/loader.bin`. The Normal-NC kernel
and Microkit image build successfully. Generic AArch64 emulation reaches the
kernel entry but times out before the initializer for both this and the
baseline Slint image, so this check does not establish a speed or boot result.
The candidate was sent to the phone on 2026-10-06. Notes appeared and Save
still changed its status through touch. The user described the initial draw as
"less" visible but did not catch its duration, so this is a favorable
qualitative observation, not a measured speedup. Keep the Device-mapped image
as a known working comparison. The physical-address special case is an
experiment; a lasting solution would expose a precise memory type through the
mapping API.

An explicit mapping path is now built separately. The three patches
`patches/sel4-iphone7plus-explicit-normal-nc.patch`,
`patches/rust-sel4-iphone7plus-explicit-normal-nc.patch`, and
`patches/microkit-iphone7plus-explicit-normal-nc.patch` extend the AArch64 EL1
VM attribute, CapDL frame data, Microkit parser, and initialiser so a map can
request `normal_nc="true"` alongside `cached="false"`. The kernel chooses the
Normal-NC MAIR entry only when this bit is set; no physical-address check is
used. `platforms/iphone7plus/nexatrode-slint-notes-explicit-nc.system` sets
it only on the screen. The touch state and touch-controller MMIO maps remain
Device. `platforms/iphone7plus/build-explicit-normal-nc.sh` rebuilds the
separate SDK and image from prepared sources without changing the base SDK.
The tool's 153 tests pass, including a new check that rejects contradictory,
hypervisor, and non-AArch64 uses of the attribute. The seL4 kernel and
initialiser build, and packaging the same system without `normal_nc` changes
the payload hash, confirming the flag reaches the image. The explicit image
is `build/iphone7plus/nexatrode-slint-notes-explicit-nc/loader.bin`, SHA-256
`82a1a198fe1b9bb60fb2a0378ff9e078b206123b3378701c89efdf27ed7318e1`.
It was uploaded on 2026-10-06. The phone displayed Notes, Save changed its
status through touch, and the user found the first-frame wipe similar to the
earlier Normal-NC image. This confirms the explicit mapping path boots and
preserves the UI/touch behavior; it does not measure scanout throughput. The
earlier physical-range candidate and Device-mapped baseline are preserved.

For a measured first-draw comparison, a timing-only variant enables EL0 reads
of `CNTVCT_EL0` in a separate SDK and wraps the initial Slint
`render_by_line` call. The counter frequency was measured as 24 MHz on D11.
The on-screen `FRAME n MS` value includes rasterization and CPU writes to the
mapped framebuffer, but not panel scanout. `SAVE` still changes that status to
`SAVED THROUGH TOUCH`. The diagnostic source is gated behind the
`frame-timing` feature in `experiments/slint-notes/src/iphone.rs`; the ordinary
Notes build is unaffected. `platforms/iphone7plus/build-notes-frame-timing.sh`
builds matched Normal-NC and Device images from the same SDK and PD binary.
The timing-only kernel and SDK changes are in
`patches/sel4-iphone7plus-vcnt-timing.patch` and
`patches/microkit-iphone7plus-vcnt-timing.patch`. Their raw image hashes are
`a6a2cf92538ab3aa86edc301306c1d702de3e0fdf19fdfd6858f47eeae8f2c2e`
(Normal-NC) and
`055f57bb03d653486aaae6724d3b4183a9dd9f8d0c65e42737a0e733a7f5b9b4`
(Device). On the phone, the first Normal-NC run displayed `FRAME 796 MS`; the
matched Device run displayed `FRAME 1185 MS`. Normal-NC reduced the measured
initial render interval by 389 ms (32.8%) in this one-run comparison. Notes
appeared and `SAVE` changed the status to `SAVED THROUGH TOUCH` with both
mappings. This measures CPU rendering and framebuffer writes, not display
scanout or the visible wipe, and should be repeated before treating the
percentage as a stable performance estimate.

The next isolated diagnostic separates rasterization from presentation on
the Normal-NC mapping. With the `stage-timing` feature, the first Slint frame
is rendered into a cacheable 0x7f8000-byte RAM region, then copied into the
live framebuffer. The status displays `R n C n MS`: `R` is rasterization into
RAM and `C` is the complete framebuffer copy, each timed with `CNTVCT_EL0`.
Subsequent status and touch redraws use the proven direct path. The separate
system description and builder are
`platforms/iphone7plus/nexatrode-slint-notes-stage-timing.system` and
`platforms/iphone7plus/build-notes-stage-timing.sh`. The candidate image is
`build/iphone7plus/nexatrode-slint-notes-stage-timing/loader.bin`, SHA-256
`441dfb084e0c344d729e4df86bf11236262af5f2333046e2bceb96f59af5dac9`.
It built successfully, but the phone showed only the old color bars after
upload; Pongo remained enumerated yet did not answer a retry. This run gave no
timing result. It does not establish whether the extra RAM mapping, the image,
or the handoff failed.

A smaller diagnostic keeps the proven direct Normal-NC render and times each
Slint line callback and pixel-write loop, with no extra framebuffer mapping.
The first version displayed `R233 W599 T797` and Save worked, but the phase
sum exceeded the total, so its split is invalid. The corrected version uses
local counters for the single initial render and an instruction barrier at
each counter read. On the phone it displayed `R773 W19 T797`. These numbers
are internally consistent, and Save still worked. They do not prove Slint
rasterization is the bottleneck: Normal-NC writes can stall later work, moving
the cost into the next line's `R` interval. The corrected image is built by
`platforms/iphone7plus/build-notes-line-timing.sh`, SHA-256
`b6ad590472d9aa169bd5bc0fdd0270016155944e9bdf2298cf35687e28495df3`.

To bound the framebuffer write cost without Slint, the `fill-timing` candidate
writes a uniform dark blue 1080 x 1920 frame, executes `DSB SY` to wait for
the writes, and then renders Notes. It displays `F n T n MS` for the completed
fill and subsequent Notes render. The image from
`platforms/iphone7plus/build-notes-fill-timing.sh` has SHA-256
`090986caebac1c65fdbff7482c0e61ca34308e4e1c5bb46554a242225bd09cc0`.
On the phone it displayed `F14 T976 MS`, and Save worked. The completed
sequential 8.36 MB screen fill took 14 ms, whereas Slint's first Notes draw
took 976 ms in that boot. This rules out a simple full-frame sequential write
bandwidth limit as the sole cause of the slow draw. It does not isolate Slint
rasterization from stalls caused by its per-pixel Normal-NC writes.

The next candidate packs each Slint-rendered line into a 32-bit RAM row and
copies the row to the live Normal-NC framebuffer in one operation. It keeps the
same system map and touch service as the working image, and uses the existing
`FRAME n MS` first-draw timing. Builder:
`platforms/iphone7plus/build-notes-row-copy.sh`. Candidate image SHA-256:
`7b0c0f6c7010448e4ec1feb1f5cc98c317db047fa073effa1e73f46aaa540061`.
On the phone it displayed `FRAME 982 MS`, compared with `FRAME 796 MS` in the
earlier direct-write Normal-NC run. Save still worked. The runs are too few to
quantify the regression, but the row copy offers no observed improvement. Keep
the direct Normal-NC renderer as the working baseline; the row-copy candidate
remains an isolated image.

The follow-up raster-profile candidate times a full Slint draw into a
throwaway one-line RGB buffer without framebuffer writes, then requests a
second full draw to the live Normal-NC screen. It displays `C n L n MS` for
these two passes and switches back to reused-buffer partial redraws for touch.
It adds no memory region. Builder:
`platforms/iphone7plus/build-notes-raster-profile.sh`. Image SHA-256:
`f69cb68a684238cfe2d0709cbeeb01b666e64f0225a081a82cc9f8e2d46fb0c7`.
On the phone it displayed `C597 L976 MS`, and Save still changed the status
through touch. Slint spent 597 ms on a full 1080 x 1920 software raster pass
even without screen writes; a later live pass took 976 ms. The first draw can
warm Slint's glyph caches, so the two times are a direction-finding comparison
rather than a pure split of one identical draw. Both the renderer and live
presentation deserve attention; raw sequential fill bandwidth alone is not
the bottleneck.

The next boot-only experiment aims to hide the visible first-frame wipe
without requiring a display page flip. The host Slint renderer emits the
default Notes frame at 1080 x 1920 and scale 2.6086957; full-buffer,
line-by-line, and reused-buffer first renders match pixel-for-pixel. A
row-run-length encoder reduces that RGB frame to a 232,152-byte asset,
`experiments/slint-notes/assets/notes-1080x1920.rle`, SHA-256
`8b8aea8c4017f9a0db6f7108982ed3f53c0d2460079822dd1bbaf87d3917d0fd`.
The iPhone candidate decodes and copies each row to the live Normal-NC screen
before initializing Slint. It then lets Slint perform its first full render
into a discarded one-line buffer to establish reused-buffer state, and writes
only a changed status region to the live display. On-screen `S n C n MS`
reports snapshot presentation and off-screen Slint initialization. The
builder `platforms/iphone7plus/build-notes-snapshot-boot.sh` regenerates and
compares the host frames before packaging an isolated image,
`build/iphone7plus/nexatrode-slint-notes-snapshot-boot/loader.bin`, SHA-256
`2a9bad59a6d94ae01f9328004809fc93d8b5ae6e43be61dccba17b88aae7109e`.
On the phone it displayed `S17 C777 MS`, and tapping Save still changed the
status to `SAVED THROUGH TOUCH`. The snapshot reached the display in 17 ms;
Slint's first off-screen render still took 777 ms before the UI became
interactive. On a repeat upload, the user observed a slight delay before
Notes appeared, but said it drew faster than the earlier visible wipe. This
supports the snapshot as a perceived first-draw improvement, while leaving
the off-screen initialization time as the next performance target. The asset
must be regenerated whenever the default
Notes UI, embedded fonts, scale, or renderer changes.

The next isolated Slint image begins the multi-workspace port. The
`workspace-ui` feature compiles `experiments/slint-notes/ui/workspaces.slint`
instead of the original Notes screen. It has a Home launcher plus Notes and
Contacts views, visible Home/Notes/People tabs, and a Save touch callback that
reports `SAVE TAP RECEIVED`. The Contacts view says no phone contacts are
loaded, and the Notes view says no keyboard or storage is connected. A host
1080 x 1920 preview and a 300 x 400 preview render; simulated taps open Notes,
fire Save, switch to Contacts, and return Home. Full-buffer and line-by-line
host renders match pixel-for-pixel. The separate iPhone image, built with
`platforms/iphone7plus/build-slint-workspaces.sh` and the proven Normal-NC map,
is `build/iphone7plus/nexatrode-slint-workspaces/loader.bin`, SHA-256
`3e6688a48487ddc0069ebf3b02db6bb288652b4ee654735819d36beec384eafa`.
The first phone handoff timed out after checkm8 and before Pongo. A repeat
reached Pongo and transferred the image; its usual `bootr` USB request timed
out, but the user saw the Home/Workspaces screen and confirmed that Notes,
People/Contacts, and Home touch navigation all work on the phone. In Notes,
tapping Save changed `NOT STORED YET` to `SAVE TAP RECEIVED`, confirming the
callback in the multi-view image. This candidate deliberately
does not use the Notes-only
snapshot, whose pixels would not match the Home screen.

The full Slint suite is a further isolated feature, `full-suite-ui`, with the
eight QEMU-era destinations: Notes, Contacts, Calculator, Clock, Calendar,
Messages, Phone demo, and Settings. Its touch keyboard edits bounded in-memory
Notes, Contacts, Calendar, and Messages fields. The calculator handles the four
basic operations and reports division by zero. Phone digits update a demo
display; Call explicitly reports that no modem is connected. Clock reports
that UTC is unavailable, while Settings, Messages, and the text views label
their missing storage or network services. The Slint UI and interaction model
live in `experiments/slint-notes/ui/full_suite.slint` and
`experiments/slint-notes/src/suite.rs`; this does not connect the QEMU vault or
device-specific services to the iPhone.

The host 1080 x 1920 preview matches line-by-line rendering. A scripted host
touch sequence opened every workspace, typed into Notes, Contacts, Calendar,
and Messages, checked Calculator `7 + 5 = 12`, and checked the Phone demo.
The first iPhone image from `build-slint-full-suite.sh` has SHA-256
`69d8885b17288ad6cb54ea8e3a23a963f25a89b424b68ae99e7e36ac02f82db3`.
On the phone, Calculator worked, but the user reported on-screen `DRAW 787 MS`
and `DRAW 780 MS` when opening screens, and noticeable lag on a digit press.
A later reading on a Calculator digit was `DRAW 6 MS`, so the earlier values
should not be attributed to each digit redraw. A host dirty-region trace at
1080 x 1920 counted 1,660,075 pixels for opening Calculator and 162,692 for
a digit (about 8% of the screen). A separate full-resolution input-profile
image measured `GAP 940 DRAW 4 MS` for a quick digit tap: nearly all the
perceived delay lies between the D11 press and release reports, not in Slint's
digit raster. The system description gave both Slint and the always-polling
touch service a 20,000 us budget per 200,000 us period (10% each). With both
budgets raised to 150,000 us per 200,000 us in an isolated image, three normal
short digit taps all registered and the observed value became
`GAP 200 DRAW 4 MS`. This is strong evidence that scheduler throttling contributed to the
missed taps and delay. A second isolated variant combines the higher budget
with contact-activated buttons, completing each simple click at the first
touch report while ignoring the eventual release. That variant still missed
rapid taps at a 150,000 us budget. A subsequent image added a 256-entry
single-producer queue in the shared touch-state page so the Slint PD can drain
all reported contact transitions after drawing. At 150,000 us it still showed
only 2–3 of five rapid Calculator taps. With both PD budgets equal to their
200,000 us periods, all five rapid Calculator taps registered, and five
normal-paced Notes letters appeared in order on the phone. Opening Calculator
from Home then showed `PRESS DRAW 61 MS`, and the user reported that the
transition felt fast. This full-budget,
queued, contact-activated image is
`build/iphone7plus/nexatrode-slint-full-suite-queued-touch-b200000/loader.bin`,
SHA-256 `e8566a332f861429b0f1ca1df3c343bdc72000f90a0bee2a814c16cdd4f41dc2`.
The always-polling touch PD can now consume a full CPU budget while idle; an
IRQ-driven wait should replace that loop before treating this as a power- or
resource-optimized configuration. The existing Notes first-boot snapshot only
addresses first paint.

A separate candidate tests half-resolution Slint rasterization: render at
540 x 960 with the same logical 414 x 736 layout, then replicate every pixel
2 x 2 into the 1080 x 1920 Normal-NC framebuffer. Touch coordinates still map
to the original logical layout. Host 540 x 960 rendering and all scripted
interactions pass, and the image from `build-slint-full-suite-half-res.sh`
has SHA-256
`8ebc4e67fa8e5f01d1dd42be3b216bb1f47a6dcbf7116f7b0feb2fa9f0e3cde5`.
On the phone, FIRST and DRAW were around 200 ms and the 5 key registered at
the right position, but the text was too pixelated to keep. A 720 x 1280
two-thirds-resolution candidate still looked too pixelated and was slower:
FIRST 1168 ms and a Calculator page DRAW over 800 ms. The full-resolution
image remains the visual baseline.
