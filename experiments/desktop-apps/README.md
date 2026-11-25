# SimpleX and NexaWal desktop experiment

This opt-in Microkit image runs the existing phone app services in two 560×520
logical-pixel desktop windows, rendered at 1120×1040 physical pixels each. The
trusted landscape shell and compositor own the wallpaper, dock, titlebars,
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
python3 tests/desktop_apps_utm.py \
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
