# GPU-first desktop and shared-storage implementation

Accepted choices: investigate GPU composition first; share the private file API
between phone and desktop; full privacy lock pauses protected apps and evicts
keys. Baseline: the tested three-app desktop at e91f8994. All experiments use
disposable UTM data. Saved wallets/chats and Pixel Wi-Fi work are excluded.

## Delivery order and status

1. **GPU feasibility — in progress.** The native diskless probe now discovers
   VirGL capabilities and verifies rendered clears and retained-texture blits.
   Full-resolution GPU readback also passes, but scanout capture is black and
   fails the presentation gate. Host-backend identification, lifetime/reset checks
   and equivalent CPU/GPU measurements are still required. A failed feasibility
   gate retains the CPU backend and records the exact unsupported requirement;
   it does not silently expand into a Vulkan/Mesa port.
2. **Retained compositor and scheduling — pending.** Separate wallpaper, chrome,
   decorations and app content. Bound pending frames, coalesce drag geometry
   without losing button/key transitions, and release buffers on completion.
   Upload only changed content; CPU fallback repaints old/new damaged regions.
   Measure input wait, rendering, composition, copy/upload and completion
   separately from QMP/host-display observations. Keep the existing appearance.
3. **Resizing — pending.** Negotiate dimensions, scale and generation. Support
   display-mode changes, resize and maximize/restore, with checked allocations,
   replacement acknowledgements and old-surface retention until release.
   Defaults: 2×; integer 1×/2× support; maximum 3840×2160 physical; minimum
   desktop 800×600 logical and app 360×360 logical. Narrow SimpleX uses its
   single-pane view below 520 logical pixels; NetSurf reflows its viewport.
4. **Shared files — pending.** Add a Rust capability-scoped file broker with
   private app roots, quotas, document handles, read/write-at-offset, stat,
   directory, rename, unlink and sync operations. Reject traversal and
   unsupported link operations. Prototype ext4 in a separate lwext4 service
   pinned to b2d8fe025940cf4091fc40334aedd64d56034cdc, preserving its licenses
   and supported-feature constraints. Keep keys in the vault; private content
   and directory metadata use authenticated encrypted objects. Keep the old
   provider as a compatibility path. Gate adoption on interrupted-write,
   corruption, isolation and equivalent-encryption/durability benchmarks.
   No Btrfs port or general POSIX/Linux ABI is part of this delivery.
5. **Integrated unlock — pending.** Boot into the trusted authenticator; require
   authenticated key unwrap and epoch-bound storage activation before starting
   protected apps. Cover the display on lock, gate requests, checkpoint with
   bounded timeouts, stop all protected frontends/workers, invalidate handles,
   clear owned private memory/caches and evict keys. Recreate on unlock and
   reconnect SimpleX afterward. Include NexaWal scanning, SimpleX receive/photo/
   transcript, NetSurf HTTPS and private-storage workers. Reuse the existing
   disposable PBKDF2/AES-GCM-SIV envelope initially; production KDF selection,
   including Argon2id measurements, is a separate promotion gate.
6. **Combined verification — pending.** Exercise all three apps during scanning,
   relay messages/photos, drag/resize and lock/unlock. Use deliberate failed
   controls for redraw, memory erasure and storage recovery. Persist source and
   loader hashes, timings, CPU/memory/upload measurements and recovery evidence.
   Publish tested milestone commits through this worktree while preserving
   concurrent main/Wi-Fi commits. Retire only fixtures created by these tests.

## Interfaces and lifecycle

Surface configure/submit/damage/completion carry bounded dimensions and a
resource generation. Apps cannot submit raw GPU commands or name another app's
buffers. File identity comes from trusted endpoints; lock and app termination
invalidate instance-scoped handles. A selected-document capability is separate
from wallet keys and private databases. Existing formats remain accessible
through explicit adapters, never by silently reinterpreting their bytes.

Screen blanking may leave an unlocked session working. Full privacy lock stops
protected work and evicts keys. Hardware suspend is a separate feature whose
future resume policy starts locked. The current three-app desktop remains a
static disposable integration and does not inherit the separate dynamic lock
fixtures' guarantees just because those fixtures pass.

No personal-data migration is authorized by these experiments. Backup/restore
and migration need a separate verified design and rollout. Titan M, verified
boot, hardware retry enforcement, rollback protection and real Pixel storage/
power/GPU validation need hardware evidence outside this UTM implementation.

## Later app portability: SDL3 and wgpu

Preserve native window, event, timer, file and lifecycle interfaces so libraries
can implement platform backends without granting apps global resources. An
initial SDL3 port should use native software surfaces and event callbacks;
audio/camera and other absent services remain explicitly unsupported. Accelerated
rendering can follow the graphics backend. SDL3 support is not already present.

wgpu additionally needs an actual supported graphics API backend and its runtime
requirements. A VirGL command probe is neither an OpenGL API implementation nor
a Vulkan driver and cannot satisfy wgpu by itself. UTM's host Metal backend does
not expose Apple's Metal frameworks inside our seL4 guest. A later wgpu spike
must select and prove a guest API implementation before claiming compatibility.
See [SDL's porting guidance](https://wiki.libsdl.org/SDL3/README-porting) and
[wgpu's backend overview](https://wgpu.rs/).
