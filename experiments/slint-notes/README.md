# Slint Notes feasibility spike

This isolated experiment renders one Notes screen with Slint 1.18.1's CPU
software renderer at arbitrary physical dimensions and scale factors. It
exercises the current 300 x 400 UTM canvas and full-resolution portrait frames
for Pixel 4a (1080 x 2340, assumed scale 2.75) and iPhone 7 Plus (1080 x 1920,
native scale 2.6087). `--exercise-input` dispatches a touch into the editor,
types `hi` through Slint's key-event API, taps Save, and asserts that the field
and callback both respond. Pixel 4a's exact logical dimensions should come from
the device's Android display metrics when we connect real hardware.

Run it from the repository root:

```sh
cargo run --locked --manifest-path experiments/slint-notes/Cargo.toml \
  --bin personal-os-slint-notes-spike -- \
  300 400 build/slint-notes-300x400.ppm --exercise-input
cargo run --locked --manifest-path experiments/slint-notes/Cargo.toml \
  --bin personal-os-slint-notes-spike -- \
  440 900 build/slint-notes-440x900.ppm
cargo run --locked --manifest-path experiments/slint-notes/Cargo.toml \
  --bin personal-os-slint-notes-spike -- \
  1080 2340 build/slint-notes-pixel4a.ppm 2.75
cargo run --locked --manifest-path experiments/slint-notes/Cargo.toml \
  --bin personal-os-slint-notes-spike -- \
  1080 1920 build/slint-notes-iphone7plus.ppm 2.6086957
cargo run --locked --manifest-path experiments/slint-notes/Cargo.toml \
  --bin personal-os-slint-notes-spike -- \
  1080 2340 build/slint-notes-pixel4a-keyboard.ppm 2.75 \
  --keyboard-inset=280 --exercise-input
```

`--keyboard-inset=N` reserves N logical units at the bottom for the OS keyboard.
The editor shrinks and the status and Save action move above it; the preview
draws a simple keyboard-shaped placeholder in the reserved area. Pair it with
`--exercise-input` to verify the editor still receives text and Save remains
tappable. This tests layout and routing only; the OS keyboard itself is not part
of this app screen.

The dimensions are physical buffer pixels; the scale argument is pixels per
logical Slint unit. [Apple lists](https://developer.apple.com/library/archive/documentation/DeviceInformation/Reference/iOSDeviceCompatibility/Displays/Displays.html)
the iPhone 7 Plus UI as 414 x 736 points with native scale 2.608, while
[Android uses density-independent units](https://developer.android.com/design/ui/mobile/guides/layout-and-content/grids-and-units?hl=en)
rather than panel pixels. The Pixel 4a scale here is a working estimate, not a
readout from the handset.

The standalone library is `no_std` and uses Slint's software renderer with the
single-threaded runtime. Its host check is:

```sh
cargo check --locked --manifest-path experiments/slint-notes/Cargo.toml --lib
```

The custom seL4/Microkit target check, from the repository root, is:

```sh
cargo +nightly-2026-09-25 check --locked \
  --manifest-path experiments/slint-notes/Cargo.toml --lib \
  --target rust/targets/aarch64-sel4-microkit.json \
  -Z build-std=core,alloc -Z json-target-spec
```

The target check confirms that the UI component and renderer compile for the
custom target. The separate iPhone build below now also links a Microkit
protection domain and runs Slint on the iPhone 7 Plus; it does not change the
existing QEMU/UTM system image or graphics profile.

Build the isolated native iPhone Notes image from the repository root:

```sh
sh platforms/iphone7plus/build-nexatrode-slint-notes.sh
```

This produces `build/iphone7plus/nexatrode-slint-notes/loader.bin`. Its Notes
protection domain runs Slint's software renderer directly into the mapped
1080 x 1920 Pongo framebuffer and receives the existing D11 touch service's
shared contact state. It uses a bounded 4 MiB boot allocator that does not
reclaim allocations. The first phone run displayed the native Slint Notes UI
with a red background and magenta editor. A second run with `RRGGBBAA` words
showed a strongly blue background and a visible Save button, but the status did
not change on tap. The next image kept those `RRGGBBAA` pixel words and
released the Slint pointer at the last contact position: the touch service
publishes zero coordinates on release. On the phone, a tap then changed the
status to `SAVED THROUGH TOUCH`, which only the Save callback sets. The user
could not confirm the exact tap position. A subsequent diagnostic drew a yellow
marker at Slint's received touch coordinate; a deliberate center-of-Save tap
put the marker on the button and invoked the Save callback, confirming alignment
at that control. The marker was then removed from the normal image. The user
also observed the first frame visibly drawing from top to bottom; subsequent
Save taps did not repeat the wipe. The previous image used Slint's reused-buffer
dirty-region mode and ordinary writes to mapped framebuffer RAM. A later boot
was successful, but the user missed the first paint, so the speed change is
not yet measured on the phone.

A staged candidate rendered into a separate cacheable 1080 x 1920 buffer,
then copied the dirty region into the live Device-mapped framebuffer with
32-bit stores. The phone showed the same visible first-frame wipe and Save
still worked. Staging added about 8 MiB without helping this issue, so the
normal build again renders directly to the live framebuffer. The staged image
is preserved at `build/iphone7plus/diagnostics/c8aa19bd/loader.bin` for
comparison. The Device-mapped scanout write path and page-flip options need
further investigation before claiming a faster first paint.

The next candidate addresses the separate readability problem. Slint scales
logical layout coordinates to the iPhone's 2.6087 physical-pixel scale, but
its embedded bitmap renderer can only draw glyph sizes compiled into the
image. Previously the largest bundled glyph was much smaller than the
physical size requested by the UI, making the text look tiny on the handset.
`build.rs` now bundles the physical sizes used by the iPhone UI, and
`ui/notes.slint` gives the heading, note text, status, and Save control larger
type and stronger contrast. Host rendering and Save input checks pass at
1080 x 1920, at 300 x 400, and with a reserved keyboard area. The native
candidate is preserved at `build/iphone7plus/diagnostics/2c8d8703/loader.bin`
(SHA-256 `2c8d8703962c9eaf15d7836943a0f34e2690472381c3337bb66c91406074c657`).
On the phone, the larger heading, note, status, and Save label were all visible
without clipping; tapping Save changed the status to `SAVED THROUGH TOUCH`.
The user reported that the smaller `PERSONAL OS` and status labels remained
faint. A follow-up candidate makes those two labels pure white and larger;
host preview and Save input checks pass at iPhone and 300 x 400 sizes. It is
preserved at `build/iphone7plus/diagnostics/16262479/loader.bin` (SHA-256
`16262479dcd2094173c55405a391c4deac69808555d26c9cfebd32c194003192`).
On the phone, the user found these mostly improved, but the smaller tinted
subtitle remained less clear than the main heading. The following candidate
uses larger white type for the subtitle and footer, with a compact font size
on the 300 x 400 canvas. Host layout and Save checks pass; the phone image is
preserved at `build/iphone7plus/diagnostics/5d6e85d6/loader.bin` (SHA-256
`5d6e85d6475151a600d1104569169c9bec1dbd8f3430fbce2d1261eb6a324bff`)
and was booted on the phone. The user described only a little residual softness
in the smaller labels, and confirmed Save still works. The phone photo shows
the main title and note text clearly; large vertical shapes in the photo
appear to be reflections on the glass, so the image alone does not establish a
framebuffer corruption fault.

Sandcastle's Linux path offers a specific performance clue: its
[`simplefb` driver](https://github.com/corellium/linux-sandcastle/blob/sandcastle-5.4/drivers/video/fbdev/simplefb.c#L465-L469)
maps the framebuffer with `ioremap_wc` (write combining/Normal non-cacheable
on arm64). Its [Android gralloc patch](https://github.com/corellium/projectsandcastle/blob/master/android/sandcastle-aosp.diff)
explicitly reduces the
framebuffer count from two to one, so this is not evidence of a working page
flip on this phone. Our Microkit `cached="false"` screen mapping becomes seL4
Device-nGnRnE, while the kernel MAIR already defines Normal-NC but does not
expose it through the current cached boolean. A useful isolated next test is
to add a Normal-NC frame mapping for *only* the screen and compare first-paint
time against the preserved Device-mapped image. Do not change the touch/MMIO
regions' attributes. This is a hypothesis to measure, not a demonstrated
speedup yet.
The earlier working image remains preserved at
`build/iphone7plus/diagnostics/87611ce9/loader.bin`.

The status text is a UI
callback only: this experiment has no persistent Notes storage, text keyboard,
or full application launcher.

The separate `workspace-ui` feature now builds a Slint Home screen with Notes
and Contacts views. It keeps the working Notes-only image unchanged. Host
previews and touch-navigation checks use:

```sh
cargo run --locked --offline --release \
  --manifest-path experiments/slint-notes/Cargo.toml \
  --features workspace-ui --bin personal-os-slint-notes-spike -- \
  1080 1920 build/iphone7plus/workspaces-preview.ppm 2.6086957 \
  --exercise-workspaces
sh platforms/iphone7plus/build-slint-workspaces.sh
```

The builder packages `build/iphone7plus/nexatrode-slint-workspaces/loader.bin`
using the same D11 touch service and Normal-NC screen mapping as the working
Notes image. Home, Notes, and Contacts are Slint views; Contacts is explicitly
empty on this iPhone image, and Notes has neither a keyboard nor persistent
storage. The Save callback reports `SAVE TAP RECEIVED` rather than claiming a
file was saved. This workspace image does not use the Notes-only pre-rendered
boot snapshot, which would be pixel-incompatible with the new Home screen.
On the iPhone 7 Plus, touch navigation from Home to Notes and Contacts and
back to Home worked, and tapping Save changed `NOT STORED YET` to
`SAVE TAP RECEIVED`.

`full-suite-ui` is the next isolated phone UI: Home plus Notes, Contacts,
Calculator, Clock, Calendar, Messages, Phone demo, and Settings. Its shared
touch keyboard edits bounded, boot-local text in Notes, Contacts, Calendar,
and Messages. The four-function Calculator and Phone demo respond to touch.
Clock, vault persistence, messaging, and the modem remain disconnected and
are labeled accordingly. The host exercise checks every navigation target,
keyboard entry, Calculator `7 + 5 = 12`, and Phone demo input:

```sh
cargo run --locked --offline --release \
  --manifest-path experiments/slint-notes/Cargo.toml \
  --features full-suite-ui --bin personal-os-slint-notes-spike -- \
  1080 1920 build/iphone7plus/full-suite-home.ppm 2.6086957 \
  --exercise-suite --compare-line
sh platforms/iphone7plus/build-slint-full-suite.sh
```

The full-resolution phone image displays FIRST and DRAW times in its footer.
The first hardware comparison showed about 780 ms for page changes and visible
delay even on Calculator input; the later measured draw for a digit was only
6 ms. A host trace found that opening Calculator repainted 1,660,075 pixels,
while a digit repainted 162,692. An isolated `full-suite-half-res` image renders
at 540 x 960 and replicates each output pixel 2 x 2 into the 1080 x 1920
framebuffer while preserving the logical touch layout; build it with
`sh platforms/iphone7plus/build-slint-full-suite-half-res.sh`. It reduced
phone FIRST and DRAW to about 200 ms but was too pixelated. The 720 x 1280
middle-resolution candidate remained pixelated and drew a page in over
800 ms. The separate full-resolution input-profile builder
`platforms/iphone7plus/build-slint-full-suite-input-profile.sh` displays the
press-to-release gap alongside release draw time. A quick digit tap on the
phone measured `GAP 940 DRAW 4 MS`, so the delay is in release delivery rather
than the digit render. The `full-suite-press-click` variant completes each tap
on its press report; build it with
`SLINT_SUITE_FEATURE=full-suite-press-click sh platforms/iphone7plus/build-slint-full-suite-input-profile.sh`.
The original system gives both the UI and touch service only 10% CPU budget.
With `SLINT_SUITE_BUDGET=150000` (75% of the 200,000 us period), three normal
short taps all registered and the status showed `GAP 200 DRAW 4 MS`. That
budget can also be combined with `full-suite-press-click` for a separate test.
Rapid typing still missed taps with that 75% budget, even after adding a
256-entry touch event queue. At a full `SLINT_SUITE_BUDGET=200000`, five rapid
Calculator taps and five normal-paced Notes letters all registered on the
iPhone. Opening Calculator from Home showed `PRESS DRAW 61 MS` and felt fast
on the device. The working prototype image is reproducible with:

```sh
SLINT_SUITE_FEATURE=full-suite-queued-touch SLINT_SUITE_BUDGET=200000 \
  sh platforms/iphone7plus/build-slint-full-suite-input-profile.sh
```

The touch service still polls continuously. The full budget fixes this
prototype's missed taps but makes an IRQ-driven or blocked wait important
before power and CPU usage can be considered settled.

The first frame-bridge contract is now defined in `src/frame.rs` and mirrored
for the C GPU client in `tests/input/slint_frame.h`: a 300 x 400 RGB8 frame, a
20-byte header, fixed row stride, and an even/odd publication revision. The GPU
client has a checked RGB-to-BGRA copy path, and both Rust and C tests reject
invalid dimensions, formats, sizes, or mid-copy updates. This QEMU frame bridge
remains dormant: the separate iPhone Slint protection domain uses its mapped
framebuffer directly. Normal UTM and QEMU profiles still use the existing
app-view renderer.

The full-suite screen now has a darker ink background, a cyan system wordmark
and divider, muted secondary labels, and restrained cyan, violet, amber, and
coral app accents. It keeps all eight workspaces and the existing touch flows.
Host validation at the phone's 1080 x 1920 layout runs
`--exercise-suite --compare-line`, covering navigation, keyboard input,
calculator arithmetic, and the Phone demo while comparing both software
rendering paths pixel-for-pixel. `build/slint-aurora-contact-sheet.png` shows
all nine views at Pixel 4a resolution.

`./dev utm-slint-preview` builds a separate UTM profile with a 1080 x 2340
Slint-rendered Home frame. The headless guest GPU test confirms exact scanout,
but this is a static host-rendered image rather than an interactive Slint
protection domain. The current UTM run reported “Display output is not active,”
so the live UTM display path still needs debugging. The full interactive Slint
image for Pixel 4a builds with `sh platforms/sunfish/build-slint-display.sh`
and lands at `.cache/pixel-sunfish/slint-display/fastboot.img`; building it
does not flash or boot the phone.
The display image includes a reset task. Set `SLINT_RESET_OUTER` when building
to control its observation window. Use `SLINT_RESET_OUTER=0` to disable the
automatic reset and leave the temporary Slint preview running until a manual
restart; the reset task yields while idle. Positive values from 1 to 5000 set
the bounded busy-wait window (for example, `SLINT_RESET_OUTER=600 sh platforms/sunfish/build-slint-display.sh`).

The UI deliberately uses Slint's built-in `TextInput` with multi-line input
and custom-drawn controls instead of standard widgets. That keeps the
software-renderer experiment small and avoids adding a widget asset decoder to
the no-`std` binary. Before adopting Slint for the OS, check its license terms
for the intended distribution and test font coverage, memory bounds,
accessibility and keyboard behavior on the actual platform adapter.
