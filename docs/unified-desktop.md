# Current desktop build and integration boundary

Use `./dev desktop` for the current ARM64 UTM desktop. Its checked-in profile is
`platforms/utm-desktop.json`: four vCPUs, 2×, SimpleX, NexaWal, NetSurf, VirGL
composition/partial redraws, native desktop resizing and the
`handoff-prime-m1-100` wallet scanner. It calls the existing builders rather than
forking the applications or making a second graphics implementation.

```sh
./dev desktop build \
  --inputs /path/to/public/utm-accelerated \
  --storage build/utm-vault-key-storage \
  --shell-base build/desktop-shell/retina-ready-2x \
  --composer build/desktop-simplex/composer-large/target/release/microkit \
  --output build/desktop-apps/your-build
./dev desktop inspect --guest build/desktop-apps/your-build
./dev desktop run --guest build/desktop-apps/your-build \
  --template /path/to/public/template.plist --keep-running
./dev desktop check --guest build/desktop-apps/your-build --label windowed
```

`run` creates a new blank test disk and VM, and refuses to overwrite tracking
for an existing session. `install` updates only an existing
verified disposable session in that build directory, preserving its disposable
disk. It checks the VM UUID, drive name, bundle boundary, image/source digests
and development-key provenance before stopping that UUID. It rejects saved-data
sessions and linked disk images. It does not discover a regular VM by name or
offer a saved-data migration option. Use `--resume` on `build` only to rebuild
an existing marked staging directory. No VM is changed by building.

`check` runs twelve app-focus switches, held moves, minimize/restore, cursor
damage checks and six bounded host CPU workers. Normal builds additionally
measure 15 seconds of held dragging and three seconds idle. For independent
rendered-pixel and live application checks, build with `--diagnostic`, then:

```sh
./dev desktop install --guest build/desktop-apps/your-build
./dev desktop check --guest build/desktop-apps/your-build \
  --label live --live
```

The live check creates a fresh upstream SimpleX peer and an unfunded disposable
wallet. It checks relay messages/photos, wallet scanning and browser HTTPS
together. It needs a sufficiently large 2× viewport with 520-logical-pixel app
height, an unlocked Mac and Internet access. Diagnostic GPU readback is much
slower; its timings must not be compared with normal dragging measurements.
Peer identities, test recovery words, trace data and disks remain ignored build
artifacts. Personal wallets, chats and Pixel bring-up work are not inputs.

## Runtime inventory and baseline profile

Each three-app build now generates `runtime.json` from its actual system
description. It lists frontends, backend workers, private shared regions and
trusted peers that also map those regions. It rejects missing/unclassified
domains and cross-app private memory. The public clock/display-mode pages may
be shared read-only. The read-only PL031 RTC register mapping is also recorded
as a public readout, outside private memory ownership or future erasure ranges.
The inventory and system description are hashed alongside
the source and loader. This is a build consistency check, not a security proof.

That check exposed a legacy shared TCP page between the agent and wallet.
The three-app network profile now gives the agent, wallet/scan worker and
browser separate physical wire pages. Wallet page offsets and wider receive
buffer semantics stay the same. Endpoint identity fixes wallet/worker/browser
slots; the agent may select only its own slots 0/1. These changes are confined
to the desktop feature flags; the phone/Wi-Fi build and existing SimpleX lock
fixture retain their current network layouts.

The default development-key profile is a **static, unlocked service assembly**. Closing
windows retains services and memory. The generated inventory explicitly reports
that wrapped-key locking, private-memory cleanup on close and dynamic package
installation are absent. The stronger passphrase/lifecycle/Notes/SimpleX fixtures
remain regression tests and do not confer their guarantees on this image.

The wrapped-session profile below adds the integrated lock boundary, covering all three frontends, all eleven
listed backend workers, private shared pages and writable process state. It
also handles network cancellation, trusted-provider caches, retained display
textures, scheduling-context donation and interrupted work. The live checks below
exercise enrollment, wrong-password denial, manual/idle lock, locked reboot and
durable recovery in the combined image. A retained-byte failure control separately
tests the lifecycle engine on the host; no guest fault-injection result is claimed.
General dynamic installation remains a subsequent integration, outside the
fixed service restart group.

An opt-in [phone NetSurf profile](../experiments/mobile-browser/README.md) now
shares the native engine and HTTPS worker with the desktop and has passed
disposable UTM checks for touch, forms, images, JavaScript, HTTPS and app routing.
It remains a development-key assembly, outside the shared wrapped-session lock
profile; Pixel hardware integration is still pending. x86 virtual-machine
support and Mac mini hardware drivers remain later work. The common app/service
code remains shared; layouts, hardware transports and platform build descriptions
are adapters.

The [CoMaps-data experiment](../experiments/comaps-native/README.md) reads a pinned
Gibraltar `.mwm` in a native ARM64 protection domain and renders roads, areas and
labels with pan/zoom at 2×. In addition to its diskless standalone fixture,
`./dev desktop build --maps` now adds an optional fourth dock/window to the GPU
desktop. Maps receives a private input queue through the trusted manager; it has
no network endpoint, vault access, other app's memory, or raw global input.
The renderer remains a bounded CPU viewer, not the full CoMaps/Drape application.
The four-app profile deliberately rejects the three-app wrapped-session option:
Maps has not been added to that lock/cleanup group. The regular three-app profile
and saved-data VMs are unchanged. Storage-backed map management, search, routing
and GPS remain later milestones.

## November 25–26, 2026 verification

The diagnostic image passed all nine live three-app checks at 2624×1644, 2×.
SimpleX sent and received through a fresh upstream relay peer; two inline photos
completed the real receive/transcript/reader/decoder path and appeared in GPU
readback pixels. NexaWal restored an unfunded test identity at height 3,809,410
and fully synced against `rpc.nexawal.com` (tip at test start: 3,814,633).
NetSurf's packaged JavaScript and real HTTPS About page rendered, including
HTTPS while the wallet scanned. Minimize/restore, focus ownership and sending
after restoring the window passed. The diagnostic loader hash is
`54d8bdf19444ad9e14230de679770f2d8786a409f2a86e50a57fe9956b6af1d6`.

The normal build passed the shared loaded-interaction command: twelve focus
switches, six held moves, minimize/restore, cursor damage, then 15 seconds of
120 Hz input with six bounded CPU workers. At 2624×1644 it completed 50.62 guest
frames/sec; interval median/p95/max were 17.52/36.09/165.24 ms. Damage covered
29.82% of a full-frame equivalent during dragging. There were zero blocking GPU
waits and zero watchdog recoveries, followed by zero frames, IRQs or timer wakes
over three idle seconds. These are guest completion timings, not physical
display FPS, and host-load differences preclude a controlled speedup comparison.
Native UTM inspection showed the normal desktop with all three dock items and
the disposable wallet synced.

The canonical fresh-launch path also booted a separate blank-disk fixture,
verified both frontends and completed GPU frames, and unregistered that VM.
Its temporary directory was removed. The existing disposable 13014BA0 fixture
remains on the normal image, with only generated test data. Saved-data VMs and
the Pixel worktree were not installed, stopped, migrated or edited.

Fourteen Python checks cover the command's disposable-data/provenance guards
and the runtime inventory, including deliberately wrong VM IDs, saved-data
flags, linked disks, stale loaders/sources, missing workers, cross-app private
pages and writable public clock/RTC views. Actual TCP receive tests pass under
ASan/UBSan for legacy, wide and receive-ahead paths, the existing SimpleX lock
epoch logic and the new private-page routing/wide-buffer control. Rust wallet
wire compatibility checks pass too. Those baseline results do not test a combined desktop lock.

Local evidence is under ignored `build/desktop-apps/resize-verify/`: generated
`runtime.json`, the preserved `unified-diagnostic-*` artifacts,
`live-unified-live/validation.json`, `motion-unified-normal.json`,
`damage-unified-normal.json`, `damage-load-unified-normal.json` and
`unified-fresh-launch.json`. Build/install/test logs are `build/desktop-unified-*`.

## Wrapped three-app session experiment

The opt-in `--locked` profile connects the existing passphrase-wrapped vault to
all three frontends and eleven backend workers. It requires a fresh disposable
VM/disk, the managed 14-service composer, VirGL and desktop resizing. It cannot
be installed over the development-key profile's disk. Enrollment creates a
random vault key; no installation key or saved user data is imported.

```sh
python3 experiments/desktop-integrated/build_composer.py \
  --source /path/to/prepared/microkit-source \
  --output build/desktop-lock-trio/composer \
  --pool-bits 26 --desktop-session
# Use that command's printed composer path with the build command above, plus:
#   --locked --diagnostic --idle-ms 30000
./dev desktop check --guest build/desktop-apps/your-locked-build \
  --label lock-cycle --lock-cycle
```

The manager starts the curated app group suspended. Only the authenticator has
the enrollment/key-transfer endpoint; a separate unprivileged test process checks
that foreign unlock and prepare calls are denied. Authentication arms a bounded
current-epoch lease, prepares the wrapped key, then commits through the manager
before reconstructing the services. F9 and the trusted idle deadline trigger the
same lock boundary. Closing/minimizing a window still retains the live service.

A full lock stops all group threads, closes all five private TCP slots, evicts
the provider key and plaintext staging buffers, detaches donated scheduling
contexts, and erases/verifies writable ELF, stacks, IPC buffers, heaps, queues,
frontend observations and surfaces. Only initial image/stack/IPC boot pages are
snapshotted; shared queues and heaps are never restored from a boot snapshot.
The compositor independently covers both scanout targets and overwrites retained
app/decor textures before acknowledging the current lock epoch and display mode.
The manager reports completed lock only after those acknowledgements. Unlock
restarts processes with original entrypoints/stacks, empty input and notifications,
and original curated capabilities; committed data is reopened from the vault.

This remains a fixed-role software lifecycle experiment. It does not revoke a
general installed application's capability graph, guarantee delivery of an
in-flight save, erase every copy in kernel/network/authenticator stacks or SIMD
registers, wipe physical host GPU memory, resist disk rollback, or provide
hardware-backed retry enforcement. The debug kernel and host QMP interface are
trusted test tools, not production physical-access protections. GPU overwrite
acknowledgement describes logical resources, not forensic hardware erasure.

The November 26 wrapped image composes successfully. Host checks pass: 26 Python
readiness/provenance/inventory checks; ASan/UBSan restart tests for the existing
six-service and new fourteen-service groups, including a retained-byte negative
control; and the actual five-slot network-gate test. Assembled-system validation
checks every owned shared region is included once, the boot snapshot excludes
those regions, all child scheduling capabilities are present, and lifecycle PPC
priorities are valid. Private process/shared erasure ranges total 904,052,736
bytes. Three focused Rust tests cover RPC polling-alarm coalescing, preservation
of bulk/backoff timer ownership and elapsed-time connection deadlines.

The combined disposable UTM run passed on November 26. The diagnostic image
`e701e7267f33d7710d8c9e553ac1629d3e5169046b6ae69c83705055662cc903`
ran at 2624×1564, 2× scale, with four accelerated vCPUs. The checked workflow:

- Enrolled a fresh generated passphrase and random wrapped vault key. All
  fourteen services remained suspended until authenticated key commit;
  unprivileged unlock/prepare attempts were denied.
- Sent and received through a real SMP relay with a fresh upstream peer and
  displayed two incoming photos through the receive/transcript/thumbnail workers.
- Locked during an advancing scan, verified private-memory clearing and both
  GPU-cover acknowledgements, and rejected an incorrect passphrase. Unlock
  discarded an unsaved draft, restored committed chat/photo data, delivered a
  message queued during lock and resumed the saved scan checkpoint.
- Loaded browser HTML/JavaScript and HTTPS during scanning. NetSurf restarted
  at Home after lock; ordinary window close/minimize retained its loaded page.
- Fully synced the unfunded generated wallet from height 3,809,410 against
  `rpc.nexawal.com` (tip at test start: 3,815,324), crossing the legacy-output
  regression range. Trusted idle auto-lock and locked reboot then recovered
  chat, photos, live delivery and a fully synced wallet again.
- Finished locked with the vault key evicted, both frontend observations zero,
  private-service erasure verified and current-mode GPU cover acknowledged.
  The test passphrase and selected plaintext canaries were absent from the disk
  image; passphrase/draft canaries were also absent from serial output. This
  canary check is additional evidence, not a proof that every plaintext copy is
  erased.

The observed manual lock was 647 ms from host QMP input to completed cleanup and
GPU acknowledgements. Diagnostic wallet-tab checks measured 167 ms median and
320 ms p95; these include readback/reference verification and sampled polling,
and exclude physical host display latency. They are not normal-image frame-rate
or touch-latency measurements.

Two delays found during recovery were fixed in this opt-in profile. Cold
AgentStore initialization now runs below frontend priority. The wallet RPC state
machine uses an outstanding 2 ms polling alarm instead of thousands of CPU
yields that consumed its managed scheduling budget. Bulk 74-block batching,
M1 tuning, 100 ms healthy bulk pacing and rate-limit backoff are retained. A
focused checkpoint retest validated its anchor in about 111 ms after connection
and advanced in 23.6 seconds overall; approximately 21 seconds remained in the
older DNS path. The normal phone and Pixel build flags are unchanged.

Evidence is under `build/desktop-apps/trio-lock-final/live-full-lock/`:
`validation.json`, `lock-validation.json`, browser OCR/pixel captures and lock
screenshots. `build/desktop-trio-final-live.log` records the completed run. The
normal image `eaffedd03de68405e3e5e6ef7f66a51a749b9150c73f8103b111d9e6f2cc56ba`
also passed a QMP smoke test using only the earlier disposable test disk:
wrapped-key recovery, both frontend surfaces after unlock, manual cleanup,
incorrect-password rejection, correct unlock after the retry delay, idle
auto-lock and locked reboot/unlock. This checks the GPU path without diagnostic
readback; it is not an additional visible-drag or physical-input measurement.
The result is `build/desktop-apps/trio-lock/normal-lock-smoke.json` and the log is
`build/desktop-trio-normal-smoke.log`. The extra normal-image VM is stopped; the
final diagnostic fixture remains locked. No saved-data VM or Pixel checkout was
changed. Source, tests and this verification record are versioned; per-run VM
disks, credentials and build evidence remain ignored local artifacts.
