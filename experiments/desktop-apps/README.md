# SimpleX, NexaWal and NetSurf desktop experiment

This opt-in Microkit image runs SimpleX and NexaWal, plus windowed NetSurf with
`--browser`. Each app has a 560×520 logical-pixel desktop window, rendered at
1120×1040 physical pixels. The trusted landscape shell and compositor own
the wallpaper, dock, titlebars,
stacking order, focus and coordinate translation. The display remains
1280×800 logical / 2560×1600 physical.

SimpleX uses a desktop chat-list/conversation layout with native Slint text
input. It reuses the real Haskell agent, Rust messages service and asynchronous
receive, transcript, photo-reader and thumbnail workers. NexaWal reuses its
existing wallet cards and setup, with scrolling and fixed bottom navigation.
It retains the four-core, 74-block, prefetch, M1-tuned, healthy-100ms scanner and
verified HTTPS transport. The backend default node is `rpc.nexawal.com:443`.
The agent/messages services have active scheduling contexts, with the same
priority ordering as the earlier live SimpleX fixture. This unlocked static
image builds networking without `SIMPLEX_LOCK_LIFECYCLE`: importing the lock
fixture's initially-closed network gate without its unlock manager prevents
relay connections. The separate lock-gated fixtures are unchanged.
The original Compose SimpleX desktop and local NexaWal Slint sources informed
the layout; this does not run their macOS/JVM application binaries.

## Build and test

Use public prepared SDK/driver inputs and the existing large-page composer.
The latter is needed because mapping the combined app ELF images exclusively
with small pages exhausts the stock initializer's capability slots. The composer
is an experimental build dependency, not a new kernel or hardware change.

```sh
python3 tests/desktop_apps_utm.py --browser \
  --inputs /path/to/public/utm-accelerated \
  --storage build/utm-vault-key-storage \
  --shell-base build/desktop-shell/retina-ready-2x \
  --composer build/desktop-simplex/composer-large/target/release/microkit \
  --output build/desktop-apps/fresh-build

caffeinate -di python3 tests/desktop_apps_run.py \
  --guest build/desktop-apps/fresh-build \
  --template /path/to/public/utm-accelerated/template.plist \
  --leave-running
```

The builder does not read `installation.json`, a provisioned vault ELF, saved
wallets/chats, or regular VM disks. It source-builds the frontends, vault,
messages service, clock, entropy service, wallet/scanning workers and network
service. The public prepared agent and hardware drivers are code/config inputs.
The public block-client configuration is relocated to this image's vault map;
its virtual addresses must agree with the SDF.

### Experimental GPU composition

Add `--virgl` to the three-app (`--browser`) build to select the isolated
retained-texture VirGL compositor. The original CPU image remains the default.
Slint and NetSurf still rasterize app content on the CPU; composition and scanout
use VirGL. GPU DMA and MMIO are mapped only into the trusted renderer, and app
surface mappings in that renderer are read-only. The manager does not wait on
GPU fences or perform full-frame composition in this configuration.

Use `--gpu-verify` on the builder for full app tests with diagnostic GPU readback
and sampled CPU-reference comparison. Without it, use `--gpu-navigation` on the
runner to test window dragging and three-app dock switching through completed
scanout/flush commands. Add `--leave-running` to leave that disposable VM open
for inspection. Diagnostic readback and physical-display observations are
different evidence; QMP's ordinary screenshot path is black with this GL setup.
See [the native GPU experiment](../desktop-gpu/README.md) for boundaries.

The runner creates its own UUID, disk, GPT partition, private upstream SimpleX
peer and files. Network egress is enabled for real relay traffic and the wallet
node. The combined fixture is explicitly **disposable-data-only**: its vault
uses the development key and the vault's `storage-test` identity API. No test
uses the installation key or real funds. The host-only `disposable_wallet`
example generates a fresh, unfunded identity using OS entropy, writes it only
to a newly created private test file, and never prints the recovery words.
New-wallet creation is exercised and canceled before persistence; a separate
fresh identity is restored with an earlier birthday to produce an actual scan
workload. The phone's rule that new wallets begin at today's tip is preserved.
`--boot-only` skips the relay and wallet exercises. `--resume` on the builder
only accepts a worktree build directory carrying its disposable-build marker.

The optional `desktop-apps-observe` feature publishes test state to bounded
observation pages in this disposable image. It is separate from the app-window
features and provides no command interface. It is not enabled by the regular
phone or desktop shell builds.

`tests/desktop_apps_manifest.py` checks the actual generated SDF and network
ELF: separate frontend authorities/private mappings, scan-worker affinity,
relocated block buffers and absence of the lock fixture's network gate.
`tests/desktop_apps_surface_test.c` checks frame validation, window-local input,
2× coordinates and bounded-ring overflow reset. The existing shell composition
and geometry tests cover trusted chrome and the unchanged demo paths. The live
runner additionally checks a fresh relay connection, bidirectional typing,
actual presented photo pixels, a fresh wallet scan and focus/minimize/restore.
Tab timings include host QMP input/observation overhead and sampled framebuffer
presentation; they are not physical Mac-display latency measurements.

## Current boundary

This is a static, curated two-app desktop integration. Each frontend has its own
heap, surface and input queue. SimpleX's frontend has no wallet-service endpoint;
NexaWal's frontend has no messages-service endpoint. Shared backend services
remain provisioned in the system image. Closing or minimizing a window hides it
and removes input ownership; it deliberately retains the running service.

This combined image does not yet connect these windows to the separate dynamic
app lock, wrapped-key eviction or process destruction/scrubbing fixtures. Those
previously tested lifecycle experiments remain separate. Do not treat window
close as a security lock or use this fixture with saved personal data. The next
integration step is a trusted desktop session lock and service lifecycle before
installing these apps into a personal desktop VM.

## Disposable UTM results (2026-11-24)

The combined four-vCPU, 2× image passed native typing, actual SMP delivery in
both directions, two distinct incoming photos, input ownership while minimized,
window close/restore and twelve Wallet↔Settings changes without restarting.
The scan advanced from 3,809,410 to 3,809,760 during the background-message
exercise. A follow-up on the same VM confirmed both photos decoded; the second
photo contributed 87,616 matching pixels in the presented chat region. The
first cold DNS lookup retried and recovered automatically.

The **broader wallet scan did not finish**. A later batch hit the existing
`OwnershipError::LegacyOutputs` guard, and the UI accurately displayed that
sync stopped and balance/history could be incomplete. That build's range ownership path assumed RingCT output-index continuity and rejected V1 outputs. Do not
remove that guard or skip those transactions to claim a successful sync;
legacy output handling needs a separate core implementation and correctness
checks.

The twelve QMP-observed tab changes had a 266 ms median and a 1.35 s maximum.
These sampled framebuffer timings include input/observation transport and the
combined workload; they exclude the physical host display. Responsiveness is
still uneven. This result does not establish a system-wide latency percentile.

Local evidence is in `build/desktop-apps/run05/validation.json`, `serial.log`,
`desktop-photo.png`, `desktop-concurrent.png` and `desktop-terminal.png`.
Functional/concurrency results and `wallet_full_sync_completed: false` are
recorded separately. The separate dynamic lock and Pixel Wi-Fi work were not
changed, and no saved wallet or conversations were used.

## Legacy-output scan implementation

The range decoder retains exact per-transaction output indices for blocks with
V1 outputs, validating each clear-amount bucket separately. RingCT-only blocks
keep the compact contiguous span. The ownership scanner uses these indices for
transparent outputs instead of skipping V1 transactions. Worker jobs use the
versioned `NXJ2` envelope; all scan PDs must be rebuilt together.

Owned legacy receipts retain their clear-amount bucket in serialized wallet
receipts and worker results. Existing RingCT receipt encoding is unchanged; old
readers reject the new legacy flags rather than interpreting their indices as
RingCT indices. The coordinator checks returned receipt keys, indices, amount
buckets and clear amounts against the authenticated source transactions before
ordered commit. Full V1 transaction hash evidence is still required by the
range decoder when pruned data omits signatures.

This adds scanning, not legacy spending. The current RingCT decoy/signing path
rejects legacy inputs. The compact single-block fallback, which lacks exact
legacy index vectors, also remains fail-closed. A wallet cannot declare a full
scan after silently skipping a legacy transaction.

The disposable regression begins at height 3,809,410 (or earlier), crosses the
previous failure range at 3,812,148, and requires terminal `SCAN OK`/completion.
It also exercises real SimpleX relay traffic and incoming photos during the
scan. Success and failures are recorded separately from the UI checks.

### Legacy scan regression result (2026-11-24)

`build/desktop-apps/run06` completed the full disposable birthday-to-tip scan:
**3,809,410 → node height 3,813,869**, 4,459 blocks in 35.747 seconds
(about 124.7 blocks/sec). This is the wallet range timer through its final
chain anchor, excluding enrollment and the earlier DNS retry; it is a combined
real desktop/relay workload, not the fixed 222-block performance benchmark.
The previously failing range at 3,812,148 was crossed. Both machine status
(`SCAN OK | SEND NOT READY`) and the presented UI (`Scan complete`) confirmed
completion. This unfunded random test wallet had no owned on-chain outputs;
owned V1 amounts, bucket indices, timelocks, receipt serialization and malformed
metadata are covered separately by deterministic published-key host fixtures.

Actual keyboard input, messages in both directions, two incoming photos and
minimize/restore passed on the same guest. The 12 sampled QMP-observed tab
changes had a 271 ms median and a 357 ms maximum; these include host control
and framebuffer sampling overhead. Cold DNS retried and recovered without a
VM restart. The run used no saved personal data. Evidence includes
`validation.json`, `session.json`, `serial.log` and `desktop-terminal.png`.

The host suite passed 100 wallet-core/range/pool tests and 20 vendored wallet
library tests. The UTM image uses the same tested source hashes recorded in
`build.json`. This does not enable legacy spending or add index vectors to
the compact single-block fallback; that fallback continues to fail closed.

## Three-app desktop

Add `--browser` to `tests/desktop_apps_utm.py` to include NetSurf as a third dock
item and 560×520 logical / 1120×1040 physical window. Prepare the pinned
JavaScript-enabled NetSurf dependencies first using the native browser's build
instructions; the combined builder reuses those public engine libraries,
source-built frontend and exported license inventory. It builds a separate
HTTPS broker with `desktop-tcp` and excludes test hooks and the fixture CA.
The existing two-app and Notes/Calculator demo images retain their original
48-byte shell ABI. The three-app shell uses a 64-byte state with an explicit
stacking order. Titlebars, dragging, focus and all three dock launch actions
remain owned by the trusted shell/manager.

In the three-app image the manager commits dock, minimize and close actions in
input order. Keyboard focus no longer waits for a Slint paint callback. The
shell still draws the chrome and handles dragging; the original two-app demo
keeps its existing command path. A canceled click or input-ring reset does not
launch a window. The live test observes committed shell state before typing,
rather than assuming that a fixed delay is sufficient under host load.

The browser maps only its own heap, frame, input queue and HTTP request/result
pages. It has no raw driver input, global screen, wallet or messages endpoint.
Its HTTPS broker uses a private TCP wire page and fixed session 4; the wallet
and scan worker retain sessions 2 and 3. The UTM-only network option
`DESKTOP_BROWSER_NETWORK` adds this endpoint without changing the default
four-session build. The broker does not map the legacy wallet/agent TCP page.
The shared network service, timer and entropy drivers remain trusted services.

Use the usual public SDK, storage-fixture, display and large-page composer
arguments, choosing a new output directory:

```sh
caffeinate -di python3 tests/desktop_apps_utm.py --browser \
  --inputs /path/to/public/utm-accelerated \
  --storage build/utm-vault-key-storage \
  --shell-base build/desktop-shell/retina-ready-2x \
  --composer build/desktop-simplex/composer-large/target/release/microkit \
  --output build/desktop-apps/three-apps
caffeinate -di python3 tests/desktop_apps_run.py \
  --guest build/desktop-apps/three-apps \
  --template /path/to/public/utm-accelerated/template.plist --leave-running
```

The runner opens and restores the browser through the dock, types an HTTPS
address after switching keyboard ownership, checks actual window scanout, and
loads a real page while a fresh wallet scans and SimpleX receives a message
and photo. The separate `browser-validation.json` records these checks. A
three-app C fixture checks stacking, titlebar protection, invalid-frame
rejection and suppression of held-key repeats after focus changes.

This is still a disposable static integration. Minimizing/closing retains app
services and does not implement session locking, key eviction or destruction.
Do not substitute a saved wallet/chat disk into this development-key fixture.
The standalone browser and the phone launcher remain separate build profiles.

### Fresh three-app result (2026-11-24)

The clean `build/desktop-apps/current` build passed all nine live checks in
four-vCPU accelerated UTM. SimpleX exchanged messages through a real relay and
presented two incoming photos, including one received while the wallet scanned.
NetSurf opened its packaged JavaScript page, loaded the real NetSurf About page
over certificate-checked HTTPS using private TCP slot 4 during scanning, and
retained the page across close/restore. Typing into NexaWal while the browser
was minimized did not contaminate the browser address.

The fresh unfunded wallet scanned height 3,809,410 through node height 3,813,918:
4,508 blocks in 31,763 ms of verified scanning (about 142 blocks/s). Cold startup
DNS retried first; this is a combined workload, not the fixed 222-block speed
benchmark. The 12 sampled wallet tab changes had a 247 ms median and 350 ms
maximum, including QMP input, polling and presented-pixel observation overhead.
The standalone JavaScript/HTTPS browser build, three-app compositor test,
two-app surface test, shell composition at 1×/2× and geometry checks also passed.
Local evidence is in `build.json`, `validation.json`, `browser-validation.json`,
`serial.log` and `three-apps-restored.png` in that build directory.

Only the current disposable desktop VM was retained from this chat's desktop
and browser trials. Ten retired disposable VM registrations/bundles and two
clean temporary integration worktrees were removed. Older data-bearing VMs,
the regular saved-data VM, backup branches and the Pixel Wi-Fi worktrees were
retained. Reuse one staging directory with the builder's `--resume` option for
incremental builds; retire its stopped disposable VM before running the launcher
again, since each test intentionally creates a new UUID and blank disk.

### Display and storage boundaries

The default CPU image and older GPU images use a fixed 2560×1600 physical /
1280×800 logical display. The optional `--desktop-resize` GPU build below
negotiates display sizes and relays bounded viewport changes to the apps.
App windows can move, minimize and close. Individual window-edge resizing is
not implemented.

The fixture uses a raw 64 MiB disk image with a GPT partition. Inside it is the
custom vault block/record format, not ext4, FAT or APFS. App namespaces and fixed
file extents hold authenticated encrypted records, the SimpleX SQLite database
and rollback journal, wallet state and photos. The vault's AES-256-GCM-SIV
records and commit/recovery protocols are not a general directory filesystem.
This disposable build uses the public development key; the separate wrapped-key
and installation-key fixtures are not enabled here.

### Native UTM desktop resizing (experimental GPU path)

Build the three-app GPU image with `--browser --virgl --desktop-resize`. This
adds a native, display-only SPICE agent over VirtIO PCI serial; it does not add
clipboard sharing, file transfer, a command shell, or saved-data access. The
runner enables UTM dynamic resolution and native Retina resolution only for
these disposable fixtures, with a fresh blank disk.

The manager accepts a single-monitor request, bounds physical dimensions to
1600×1200–3840×2160, rounds down to even dimensions, and keeps a 2× logical
scale. The dock follows the desktop size; windows are clamped back into view.
Apps keep their 560-point width, with a 380–520-point viewport height to fit
shorter desktops. NetSurf schedules reflow and the Slint frontends resize their
windows. Guest DMA reservations stay at bounded maximum capacity. The two host
scanout textures are recreated at the active dimensions after fenced detach and
release of the previous resources; SPICE uses their dimensions to normalize
tablet coordinates. The agent replies only after the GPU has
completed scanout and flush for the requested mode.

For readback/reference validation add `--gpu-verify`, boot using
`tests/desktop_apps_run.py --boot-only --leave-running`, resize the **native UTM
window** or enter/exit full screen, then run:

```sh
python3 tests/desktop_resize_input.py --guest build/desktop-apps/resize-verify \
  --label windowed --exercise
```

Repeat with `--changed --label fullscreen` and `--changed --label restored`.
The validator checks the actual SPICE request, manager mode, completed GPU
acknowledgment, window bounds, dock focus, held dragging and sampled GPU pixels
against the CPU reference. It never fabricates a host monitor request. The
capture is diagnostic GPU readback, not a measurement of physical display
latency. Sizes outside the supported range are bounded and UTM may scale or
letterbox the result. This does not implement resizing individual app windows.

Host checks for resize clamps, invalid requests, input cancellation, and bounded
frame growth/shrinkage:

```sh
clang -O0 -fno-builtin -I tests/surface_stub tests/desktop_resize_test.c \
  -o /tmp/desktop-resize-test
/tmp/desktop-resize-test
rustc --edition 2021 --test --cfg 'feature="desktop-real-app"' \
  tests/desktop_resize_frame_test.rs -o /tmp/desktop-resize-frame-test
/tmp/desktop-resize-frame-test
```

The corrected resize build was tested on November 25, 2026 with a blank disk
and no saved wallet or chats. Native UTM window sizing produced 1600×1564 and
1600×1200 modes; full screen produced 2624×1644; returning to windowed mode
restored 1600×1200. At each recorded size, the GPU reference/readback, independent
dock and edge pixel checks, dock focus, held guest-window drag and search typing
passed. The first run exposed stale shell pixels after stride changes; full
redraw on mode changes fixed that. Automated input now waits for the mode to
settle and uses ceiling conversion to avoid one-pixel absolute-input rounding
errors. Failed runs are retained separately from passing observations.

Evidence: `build/desktop-apps/resize-verify/resize-observations.json` and the
`resize-*.png` captures. These are diagnostic GPU readbacks. Native UTM captures
also showed the clean full-screen and restored layouts. QMP-injected input is
verified by those initial observations; native input failed in the original
`05A90E91` fixture and required the follow-up fixes below. Those QMP observations
must not be treated as evidence of native keyboard or mouse support.

Native-input investigation found two independent defects. SPICE routed mouse
events to the display-only agent rather than the VirtIO tablet, despite the
agent advertising no mouse capability. The launcher now sets
`-spice agent-mouse=off`, preserving ordinary device input and display-agent
monitor configuration. Maximum-sized scanout textures then caused the tablet
coordinates to be normalized against the wrong dimensions; recreating the two
scanout textures at the active mode fixed native targeting. Clipboard and
directory sharing remain disabled.

The corrected `0F7D4AFB` diagnostic fixture received native UTM/SPICE clicks and
keyboard input: search showed `n` in windowed mode and `nf` after entering
2624×1644 full screen. These actions used native computer-use input, without QMP
input injection in that fixture. Its temporary input tracing is diagnostic only;
normal fixtures omit it. Both intermediate input-trace fixtures were retired
after preserving the observations. The normal build, without expensive GPU
reference verification or key tracing, booted successfully as disposable
`13014BA0-53ED-4DCB-9F28-E36FD5E4231F`. Its native reopen check remains pending
while the Mac is locked. Scope and source/loader hashes are recorded in
`build/desktop-apps/resize-verify/native-input-validation.json`. Saved
installation data and Pixel Wi-Fi were untouched.

The user subsequently confirmed native input worked. The same disposable VM
now contains the bounded GPU frame-submission improvement described in the
desktop-gpu README. The normal build omits GPU reference readback and input
tracing; its final short held-motion runs reached about 58 completed frames/s,
with remaining long-tail stalls recorded rather than discarded. See
`build/desktop-apps/resize-verify/motion-validation.json` for the tested loader,
pixel checks and separate native host display observation.

The same disposable now runs the completion-driven, software-paced GPU build.
Drawing and texture uploads yield to Microkit until their validated completion;
normal starts are capped at 60 Hz and idle work stops. The final short minimum-size
run completed about 56 frames/s, but longer runs retained severe completion
stalls, so this is not a consistently smooth 60 Hz claim. Normal/diagnostic,
native/QMP and handler/wait timings are distinguished in
`build/desktop-apps/resize-verify/paced-scheduling-validation.json` and the
desktop-gpu README. Saved wallet/chats and Pixel work remain untouched.

The subsequent timestamped GPU investigation reduced steady batches to one
final fence and merged identical rounded-edge strips (132 to 84 blits with
three visible apps and the cursor). The final trace-free 2624×1644 held-drag
run completed 55.37 frames/s, with 17.60 ms median and 24.90 ms p95 intervals;
its maximum was 81.02 ms. The host displayed complete moved content while the
QMP drag remained held. Long host/guest scheduling stalls were reproduced in
older-image controls, and intermittent early SMP boot hangs remain unresolved.
See the desktop-gpu README and
`build/desktop-apps/resize-verify/stall-profiling-validation.json` for all
outliers, diagnostic failures and native-input limitations. The tracing-only
fixture was retired; `13014BA0` runs the normal build without GPU or key tracing.
