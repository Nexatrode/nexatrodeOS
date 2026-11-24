# Shared phone and desktop development

The accepted direction keeps the curated phone on Microkit and develops native
desktop app installation through a separate seL4 process manager. UTM is the
integration environment. Slint and the existing CPU Skia and VirtIO display
work remain the rendering foundation.

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
Real app workers, their installation-key migration and protected file services
remain separate integration boundaries.

## Implementation order

| Milestone | Completion requirement | Current state |
| --- | --- | --- |
| Reproducible baseline | Repeat app switching, typing, photos, scanning and receiving with stage timings and source/build identities | Existing timing tools and new root-task build hashes; combined workload remains |
| App lifecycle | Fresh identity, memory and permissions on launch; reclaim after exit, fault and interrupted creation | 100 small-process launches and 13 graphical instances tested; general graphical-constructor rollback remains |
| Independent surfaces | Separate app renderers, bounded submissions, focus routing and trusted lock coverage | Dynamic close/fault/restart with a surviving peer, plus a separate passphrase-gated one-app fixture with trusted full-screen lock coverage; multiple dynamic windows remain |
| Frame presentation | Explicit buffer ownership/release, retained damage, bounded queues, presentation feedback | Existing band redraw and frame-copy work plus the new last-good surface snapshots; swapchain remains |
| Protected storage and lock | Private app storage, scoped documents, recoverable writes, key eviction and recreated apps | Disposable wrapped-key eviction, leased unlock commit, encrypted-note recovery and fresh app memory tested together; real app checkpointing/migration, general documents and complete secret lifecycle remain |
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
