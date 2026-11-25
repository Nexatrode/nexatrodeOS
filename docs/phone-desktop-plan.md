# Shared phone and desktop development

The accepted direction keeps the curated phone on Microkit and develops native
desktop app installation through a separate seL4 process manager. UTM is the
integration environment. Slint and the existing CPU Skia and VirtIO display
work remain the rendering foundation.

The accepted GPU-first delivery sequence and current checkpoint status are in
[the implementation record](desktop-integration-execution.md). Native VirGL
rendering/readback now works in a separate diskless probe; the presentation
gate is still failing, so the three-app desktop retains its CPU compositor.

The laboratory implementation now includes a graphical lifecycle image. A
trusted Microkit-hosted manager and compositor creates a Slint process after
boot from a bounded resource pool. It clears that window's caches and input
ownership, revokes its mappings and objects, and recreates a fresh instance.
A second Slint window stays in a predeclared domain to test peer survival.
The dedicated root-task and two-static-window systems remain regression tests.
A separate disposable lock fixture now gates launch on passphrase verification,
destroys the dynamic app on manual or idle lock, and erases its private user
frames before revocation. Its optional wrapped-key mode synchronously evicts
the random disposable vault key and commits unlock before launching an app.
A scoped broker now gives that dynamic Notes app a private encrypted document,
with lock/unlock and interrupted-save recovery tested in disposable UTM.
A separate disposable live-relay fixture now also gates the full SimpleX
frontend, agent and receive/photo/transcript workers on the wrapped-key lock.
Manual/auto lock, a locked reboot, live delivery and actual photo presentation
passed; see [the SimpleX lifecycle evidence](simplex-dynamic-lock.md).
Regular installation-key migration, NexaWal lifecycle integration and general
file services remain separate boundaries.

A separate [native NetSurf guest](../experiments/netsurf-native/README.md)
now runs packaged HTML/CSS/BMP/PNG/JPEG documents and real forms, links/history and
scrolling at 2x in disposable UTM. Its opt-in HTTPS worker also fetches and
renders real NetSurf/IANA/kernel.org pages using guest DNS/TCP and validated
TLS. Its standalone profile remains available; the opt-in three-app desktop now
also provides a native window and dock item alongside SimpleX and NexaWal.
Browser persistence and combined desktop lock integration remain pending. This experiment
supplies no Linux binary ABI.

## Landscape desktop shell

The opt-in `DESKTOP_SHELL` image adds a 1280×800 logical workspace to the
graphical lifecycle experiment. The default desktop build now renders at
2560×1600 physical pixels (2×); `--scale 1` builds the comparison mode. It boots
directly to the existing aurora wallpaper, a
Mac-local clock in the top bar, and a two-app dock. Notes and Calculator use
separate Slint renderers, input queues and content surfaces. The trusted manager
owns window positions, stacking, keyboard focus and the management endpoint;
app content cannot paint over a higher window's title bar.

The dock launches or restores windows. Title bars support moving, minimizing
and closing. Notes is constructed after boot and destroyed on close; reopening
creates a fresh process, input ownership and document. This shell's note is
explicitly session-only and its UI says that closing discards it. Calculator
uses a predeclared isolated PD; closing hides it and resets its arithmetic state,
rather than reclaiming that PD. The separate encrypted Notes and wrapped-lock
fixtures remain available; this desktop image does not yet combine them.

Reproduce from this worktree, using the same prepared public driver inputs and
private composer as the graphical tests:

```sh
caffeinate -di python3 tests/desktop_shell_utm.py \
  --inputs /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --base build/desktop-graphical/final-utm \
  --sddf /Volumes/untitled/sel4-personal-os/.cache/sddf-personal-os \
  --composer build/desktop-simplex/composer-large/target/release/microkit \
  --output build/desktop-shell/new-run
```

`new-run` must not already exist. The desktop shell and both app renderers use
the same fixed density. Slint pre-rasterizes glyphs and SVG resources at that
density, and receives the matching scale factor and physical window size.
Window placement, hit testing and pointer events remain in logical coordinates;
composition, strides, damage bands and the cursor use physical coordinates.
The 2× output and shell frame regions are 16 MiB and app surfaces are 4 MiB;
phone defaults retain their existing 8 MiB output contract.

Both desktop UIs explicitly import the pinned Roboto variable font in
`experiments/slint-notes/assets/fonts/Roboto-Variable.ttf`. Its source commit and
license are adjacent. The builder rejects a fallback family or missing weights
400/600/700 and checks that the 15-logical-pixel label has a 30-physical-pixel
glyph at 2×. It also copies the font's OFL notice into the staging directory.
This fixes the initial shell's accidental Helvetica-regular fallback.

The builder compiles the shell, app interfaces, manager and landscape GPU client from source. It reuses the prepared kernel,
VirtIO/input drivers and clock service. The runner creates its own VM UUID and
blank disk, restricts networking, disables host sharing and removes that VM on
exit. It never opens the regular VM disk or installation key. No wallet, vault,
SimpleX database or Wi-Fi driver is part of the landscape image.

Verification on 2026-11-24 uses actual guest mouse/keyboard input, presented
1280×800 and 2560×1600 frames, OCR and serial diagnostics: dock launch, typed note, calculator
arithmetic, independent focus, minimize/restore, title-bar drag and Notes
close/recreation. Host tests cover geometry/clamping, overlapping window chrome,
malformed surfaces, closed input and cache cleanup, under ASan/UBSan. Existing
portrait surface and frame-copy tests also pass. The run stores `build.json`,
`shell-result.json`, serial output and screenshots in its own ignored directory.
The actual scanout tests also sample typing and minimize/restore 12 times each.
`responsiveness.json` measures QMP input submission through first changed scanout
capture, including capture and file overhead. Full-frame readback itself grows
with pixel count, so those values must not be interpreted as UI-only latency.
The optional runner flag `--desktop-shell-frame-profile` adds a smaller,
sequence-checked readback of the completed public RGB frame. Its
`frame-publication.json` still includes QMP overhead and excludes GPU transfer
and the physical host display. It reads only the disposable guest's declared
output pages; private app memory and real-data disks are outside the probe.

Run `tests/desktop_shell_sharpness.py --one <1x-output> --two <2x-output>` to
compare the same Roboto title at both densities. The test requires independent
physical samples within logical text pixels at 2×, rather than a stretched 1×
bitmap, and produces native crops for visual review. Scale selection is fixed
per build; live monitor-DPI changes remain future work.

The 2026-11-24 comparison in `build/desktop-shell/retina-ready-{1x,2x}` passed
all interaction checks. The same title uses Roboto 700 at 15 physical pixels in
1× and 30 in 2×. The native crop has 375 logical text blocks with independent
2× pixel samples; the nearest-enlarged 1× crop has none.

Observed medians for the smaller coherent-frame probe, 12 samples per action:

| Action | 1× | 2× |
| --- | ---: | ---: |
| Typing | 126 ms | 138 ms |
| Minimize | 126 ms | 177 ms |
| Restore | 140 ms | 188 ms |

The three-readback capture itself had medians of 74 and 76 ms. These values
include test overhead, exclude GPU/physical-display latency, and were collected
under the Mac's uncontrolled current workload. They establish functioning
interactions and a measurable cost for 2× window updates, not a controlled
measurement of user-perceived latency or a frame-rate guarantee.

This is an initial desktop shell. It does not add a desktop login, durable
session saving, resize/maximize, multiple dynamic windows, package installation,
a general file manager or GPU drawing acceleration. The Notes lifecycle fixture
has a bounded 60-instance generation space per boot; hitting the limit rejects
another launch without restarting the manager. Broader process admission and
construction-failure rollback remain part of the lifecycle milestone below.
Phone rendering dimensions and the regular accelerated VM build stay unchanged.

## Implementation order

| Milestone | Completion requirement | Current state |
| --- | --- | --- |
| Reproducible baseline | Repeat app switching, typing, photos, scanning and receiving with stage timings and source/build identities | Existing timing tools and new root-task build hashes; combined workload remains |
| App lifecycle | Fresh identity, memory and permissions on launch; reclaim after exit, fault and interrupted creation | 100 small-process launches and 13 graphical instances tested; general graphical-constructor rollback remains |
| Independent surfaces | Separate app renderers, bounded submissions, focus routing and trusted lock coverage | Dynamic close/fault/restart with a surviving peer, plus a separate passphrase-gated one-app fixture with trusted full-screen lock coverage; multiple dynamic windows remain |
| Frame presentation | Explicit buffer ownership/release, retained damage, bounded queues, presentation feedback | Existing band redraw and frame-copy work plus the new last-good surface snapshots; swapchain remains |
| Protected storage and lock | Private app storage, scoped documents, recoverable writes, key eviction and recreated apps | Dynamic Notes now uses scoped encrypted storage with stale-handle denial, key eviction and all four interrupted-save boundaries tested; real app checkpointing/migration, general documents and complete secret lifecycle remain |
| Complete interface | Shared controls and navigation, useful Files and Settings, measured transitions | Existing shell, lock screen, SimpleX and NexaWal work; consistent independent app interfaces remain |
| Native installation | Load packages from disk, owner approval, stable identity, permissions, update/rollback/uninstall | Embedded bounded native and Slint ELF loading demonstrated; disk packages and installer remain |
| Network resilience | App recovery and fail-closed required routing across disconnects and retries | Existing wallet and SimpleX recovery; OS egress policy remains |
| Physical devices and power | Verified storage, time, input and display; screen off, idle, suspend/resume tested independently | Separate hardware work; Pixel integration follows verified drivers |

The next stronger passphrase-lock milestone must pause SimpleX and NexaWal.
Its implementation must block new requests, checkpoint protected data, stop secret-bearing workers,
clear private memory and graphical caches, and evict vault keys. Unlock creates
fresh instances. Screen blanking can permit background work while unlocked.

The phone starts with a curated app set. The desktop permits owner-built native
apps, including unsigned local builds assigned a local identity. Source
signatures identify provenance. Updates must preserve the intended identity
and require approval for increased authority. Build community source in a
disposable environment with pinned inputs and no personal files or signing keys.

Keep app-level lifecycle, storage, surface, input, time and network interfaces
small and versioned. Caller identity comes from trusted endpoints and
capabilities. Bound resource consumption and invalidate every instance-scoped
handle on termination. Provide document access through a trusted chooser that
grants a selected document; wallet keys and private app databases stay outside
document browsing.

The initial supported renderer remains the CPU backend. GPU work starts with
a platform feasibility test of commands, memory ownership, synchronisation,
failure recovery and the same measured workload. Driver implementation and
Titan M, verified boot and rollback protection each need hardware evidence.

## Verification and rollout

Every experimental image uses disposable data and a separate output directory.
Capture the loader/kernel hashes, relevant source hashes, serial markers and
framebuffer evidence. Report QMP framebuffer observations with their scope.
The initial interaction targets are 100 ms p95 for visible tap feedback and
250 ms p95 for switching to an already loaded app in the standard UTM workload;
these are targets pending controlled measurements.

Test cross-app memory, storage, input and surface access, stale authority,
faults, repeated launch/exit, allocation failures, malformed messages, parser
bounds and interrupted storage updates. Deliberate failure cases must cause
the corresponding tests to fail. Test redraw correctness before adding
transitions, and measure CPU and memory use under the combined workload.

Promotion proceeds from isolated tests to a disposable full-system UTM image
and then a staged regular-VM build. Real-data migration needs a verified backup,
authenticated format handling and a tested recovery procedure. Experimental
images do not migrate the regular wallet or chats.

The current worktree owns these changes. Pixel Wi-Fi development continues
separately. Linux, Android, iOS and macOS app compatibility remain later
projects.

See [the app architecture and experiments](desktop-app-direction.md),
[the frame handoff](utm-frame-handoff.md), and
[the wrapped-key prototype](vault-key-prototype.md) for current evidence.
The [dynamic app lock experiment](dynamic-app-lock.md) records the separate
passphrase/lifecycle test and its security limits.

The [desktop apps experiment](../experiments/desktop-apps/README.md) now runs
SimpleX and NexaWal together in 2× landscape UTM with fresh data, with `--browser`
adding NetSurf as a third window and dock item. Live messages,
photos, native typing, background receipt and window restoration passed. Tab
changes remain measurable under load. After adding indexed legacy-output
scanning, a disposable wallet completed 4,459 blocks through node height
3,813,869. Legacy spending and the compact single-block fallback remain
separate limitations. This static integration still needs the separate desktop
session-lock/service lifecycle before any personal-data rollout.

The fresh three-app image passed real browser HTTPS loading while the wallet
scanned and SimpleX received a message and photo. Dock focus commits in input
order independently of shell painting. The display and app-window dimensions
are still fixed; UTM host-window resizing does not trigger guest relayout.
Storage remains the custom encrypted vault with bounded app file extents,
rather than a general directory filesystem.
