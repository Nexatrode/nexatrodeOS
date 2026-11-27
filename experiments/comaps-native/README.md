# Offline CoMaps-data experiment

This is a native ARM64/seL4 bring-up viewer for one real CoMaps map region,
Gibraltar (1,272,536 bytes). It reads the pinned `.mwm` directly in the guest,
including points, inner/outer polylines and triangle geometry. Geometry is decoded
once at startup, then retained for viewport culling and CPU rendering at
2560×1600, 2×. It is **not the full CoMaps app or its Drape renderer**.

Drag to pan; use + / − or the mouse wheel to zoom. Arrow keys pan, H/Home
restores the initial peninsula view, and Fit does the same. Zoom has six bounded
steps and panning is constrained to the region bounds. The simple style shows
coastline water, green areas, buildings, roads, railways and selected labels.
It does not reproduce CoMaps' full cartographic rules or label placement.

## Reproduce

Prepared public UTM SDK/driver inputs, the existing 2× display assembly and the
ordinary Microkit composer are prerequisites. No installed vault ELF, VM disk,
installation manifest, wallet or chat files are inputs. The pinned CoMaps source
and downloaded map stay in ignored `build/comaps/`; nothing is installed into a
saved-data VM or the Pixel checkout.

```sh
python3 experiments/comaps-native/prepare.py
python3 tests/comaps_native_host.py
python3 -m unittest discover -s tests -p 'comaps_native_*test.py'
python3 experiments/comaps-native/build.py \
  --inputs /path/to/public/utm-accelerated \
  --display build/desktop-shell/retina-ready-2x \
  --composer build/desktop-graphical/composer/target/release/microkit \
  --output build/comaps/guest
caffeinate -di python3 tests/comaps_native_utm.py \
  --guest build/comaps/guest \
  --template /path/to/public/template.plist --leave-running
```

The builder reuses the existing public display/input drivers and standalone
picolibc/stb dependencies prepared by the NetSurf experiment; no browser engine
or browser renderer is linked. Use `--resume` to rebuild a marked output, then
stop **only this experiment's disposable UUID** and use `--reuse` on the runner.
The runner checks source/system/loader hashes and its recorded diskless VM
identity before replacing the loader. It creates no disk and configures no
network adapter, shared folder or clipboard. Building alone changes no VM.

The map reader runs in its own protection domain with only its frame, heap,
input ring, pointer and focus pages. Raw input access is confined to this single-app test
VM; the desktop adapter below instead uses the trusted shell's private
input routing. This standalone image has no wallet, SimpleX, vault, GPS or network services.
It is not a lock-screen or private-storage test.

## Verification

On November 26, 2026 the native reader decoded 8,957 records and 140,685 points.
The host reference test compared 6,674 geometry blocks point for point with the
pinned, unmodified CoMaps C++ decoder. It compiles upstream geometry/point coding
and exception support solely as an independent host oracle; the guest is the
bounded C adapter. Address/undefined-behavior sanitizers cover both, including
truncations, invalid table/version/record/varint/geometry input and 300 deterministic
mutations. Capacity or unsupported-format failures reject the whole region;
partial map loads are not reported as success.

The disposable UTM test verifies actual scanout colours for water, greenery,
buildings and streets; visible zoom changes; held pointer dragging on both axes;
keyboard and wheel controls; zoom limits; identical map pixels after Fit; and no
map frame publication during two idle seconds. Timing results in
`build/comaps/guest/interaction/validation.json` measure guest drawing, including
scheduling delays, rather than physical display FPS or touch latency. The current proof path reuses
the earlier CPU display assembly; some loaded-host runs included multi-second
drawing/publication intervals. A functional pass does not establish smooth
60 Hz panning, and integration with the current shell/display path still needs
performance work.
Screenshots/OCR and the serial trace remain alongside that result. `build.json`
records source, map, dependency, system and loader hashes; `host-validation.json`
records the sanitizer/reference checks. These tests do not establish a complete
untrusted-map security audit.

## Scope and next steps

The reader deliberately supports a bounded subset of MWM DAT versions 0/1,
choosing the best available geometry. Metadata, search/routing indices, live
positioning, bookmarks, downloading/updating regions, multiple maps and general
filesystem access are not implemented. The initial camera is a fixed public
Gibraltar view, not an inferred user location. The prototype is tested against
one pinned map; other region/version combinations remain unverified.

The opt-in storage-backed experiment below adds two pinned regions. A full CoMaps port
would additionally need its C++ runtime/platform services, Drape graphics backend,
task/thread lifecycle, map management, search and routing. Existing VirGL blit
composition does not supply the full graphics API that Drape expects.

## Two storage-backed regions (disposable UTM only)

Named place labels and visible road labels can be tapped to open a details card.
The card shows the offline name, feature kind and the map geometry's anchor
coordinates (not a GPS fix or a guaranteed entrance/address). A cyan marker
identifies the selected map point. Use × to close, tap empty map space to clear,
or drag to pan. Movement of up to six logical pixels is treated as tap jitter;
crossing that threshold starts a pan with the full initial displacement. Card
touches do not pan the map underneath. Changing regions or using Fit clears the
selection, and stale label hit rectangles are invalidated after camera/viewport
changes. Details survive ordinary app switching but are not saved on disk.

`tests/comaps_interaction_test.c` checks the gesture threshold and cancellation
under sanitizers. `tests/comaps_details_utm.py --guest <stage> [--mobile]` checks
real place/road cards, jitter versus dragging, card input consumption, private
focus, both regions and stale-selection cleanup in a disposable fixture. The
second page of the public observation region exposes bounded selection/hit
metadata for those checks; no other app gets that mapping.

Add `--map-storage` to either the desktop or mobile `--maps` builder. This keeps
the existing embedded-region mode available, and creates a new 32 MiB public-map
pack with Gibraltar (1,272,536 bytes) and San Marino (1,980,495 bytes), pinned in
[regions.json](regions.json). Region bytes are absent from the storage-profile
app ELF. The runner copies the pack into a separate `maps.img` disk in its own
generated disposable VM; the fresh app-service `blank.img` remains separate.
No regular VM disk, wallet, chats, installation key or Pixel checkout is read.

Tap the region name at the upper left, then choose either region. Both layouts
share the loader and decoder. The current map remains usable while an isolated
`map_store` protection domain reads bounded 64 KiB chunks through an
interrupt-driven VirtIO block queue. The public file buffer is read-only in the
renderer; the shell, wallet, chat and browser receive no mappings or channels to
the storage service. Map parsing and checksum work run below the interactive
frontends' priorities. A candidate decoded map replaces the current one only
after the whole pinned SHA-256 and bounded decoder checks pass.

This is a small map-pack format with two fixed public-file extents, **not a
general filesystem**. Each file is limited to 8 MiB; the existing 32,768-feature,
524,288-point and 32,768-per-geometry limits remain. San Marino decodes 32,619
records and 383,542 points. Both regions were compared point for point with the
unmodified pinned CoMaps geometry decoder (6,674 / 18,389 blocks), and tested
under address/undefined-behavior sanitizers. Other regions/formats and larger
files are not automatically supported. There are no runtime map downloads.

After a successful load, the service writes the last selected region to the
older of two checksummed 4 KiB selection records, then requires a VirtIO FLUSH
before acknowledging the save. Cold boot chooses the newest intact record and
revalidates the selected region; it does not rewrite an unchanged selection.
A torn newest record falls back to the older record. A corrupt region displays
an error and never replaces a valid current map. If the selected region is
damaged at boot, another region can be chosen. A truncated pack is rejected by
capacity validation. Restoring valid public payloads allows recovery after a
restart. These checksums protect against damage, not malicious rollback of a
public region preference. There are no private bookmarks or camera-history
records, and this profile is still outside the wrapped-key lock lifecycle.

Run the dedicated checks after creating a boot-only, keep-running fixture:

```sh
python3 -m unittest discover -s tests -p 'map_storage_*test.py'
python3 tests/map_storage_utm.py --guest build/desktop-apps/maps-storage --regression
python3 tests/map_storage_utm.py --guest build/mobile-browser/maps-storage --mobile --regression
```

The guarded test modifies only that fixture's `maps.img` while its recorded UUID
is stopped. It exercises repeated switching, both real scanouts, cold restart,
an artificially torn selection, corrupted selected/unselected files, an
incomplete payload, a physically truncated pack and restoration. Optional
regressions exercise the existing four-app checks, phone browser groups and
loaded-host pan/switch checks. Evidence is under each stage's
`storage-verification/`; screenshots distinguish renderer state from actual
scanout. The first San Marino UTM trial exposed an unaligned libc tail read from
the uncached DMA mapping; explicit volatile byte copies fixed it. This remains
a QEMU experiment: device hot-unplug, missing completion interrupts, arbitrary
filesystem access and physical Pixel storage are outside its verification.

## Optional GPU desktop integration

The existing desktop builder now accepts `--maps` alongside
`--browser --virgl --desktop-resize`; `./dev desktop build --maps` supplies those
base flags. See the [desktop instructions](../desktop-apps/README.md) for the
large-page composer prerequisite and disposable build/run/check commands.
This adds a fourth dock item and retained 1120-pixel-wide RGB8 surface. Its height
tracks the bounded desktop mode at 2×. Map drawing remains CPU based; the trusted
VirGL compositor uploads and positions the retained texture and decorations.
Moving a window does not ask Maps to draw its contents again.

The Maps PD gets only its frame, 16 MiB heap, private input ring, timing page and
read-only display mode. The manager owns global pointer/focus and translates
events to local coordinates. Maps has no vault/network endpoint or other app
memory. Hidden Maps does not process other apps' keys or publish frames. Unlike
the diskless fixture, the four-app desktop has a fresh development-key disk for
the existing app services. It rejects the three-app wrapped-session lock option
until Maps is included in that lifecycle; do not use it with personal data.

`tests/desktop_maps_input.py` checks actual diagnostic GPU pixels, all four dock
actions, focus isolation, pan/zoom, retained minimize/restore and held window
movement. The normal build measures handler-to-publication phases and completed
GPU texture generations without expensive readback. Both use four bounded host
CPU workers. Phase times include preemption; QMP injection-to-observed completion
also includes polling and host scheduling, not physical display latency.

Profiling identified opaque area/triangle fills and reservation replenishment
as useful targets. Solid RGB rectangles now rasterize one clipped row and reuse
it, while text retains alpha blending. `tests/comaps_raster_test.c` compares
10,000 rectangles against the old pixel oracle under sanitizers. Maps keeps a
bounded 8/10 ms CPU reservation below the interactive frontends' priorities,
allowing typical frames to complete without the former 4/10 ms mid-frame wait.
This is a local experiment, not a consistent 60 Hz or full CoMaps performance
claim. Region parsing, attribution and the default three-app profile are shared
with the earlier experiment and unchanged in scope.

In the November 26, 2026 four-vCPU normal-compositor runs, the same 16 alternating
keyboard pans at 2560×1564 (1120×1040 map surface) with four bounded host CPU
workers recorded these handler-to-publication times:

| Configuration | Median | p95 / maximum (16 samples) |
| --- | ---: | ---: |
| Original fills, 4/10 ms reservation | 13.142 ms | 20.368 ms |
| Reusable RGB rows, 8/10 ms reservation | 6.728 ms | 26.344 ms |

The median improved, but the tail did not; host/guest preemption remains visible.
This was not a controlled idle-host experiment. Intervening runs with other
viewport heights are retained as separate observations and excluded from this
comparison. The final diagnostic run also passed map colours, pan/zoom, focus
isolation, minimize/restore, held-window pixels and 12 four-app switches under
load. It is kept separate from normal-compositor timings.

Evidence is in `build/desktop-apps/maps/maps-normal-before/`,
`maps-normal-budget/` and `maps-final-diagnostic/`, including the recorded build
manifests and traces. `resize-observations.json` records independent layout,
four-app dock, held-drag and typing checks after native UTM/SPICE windowed,
2624×1644 full-screen and restored-window changes. Native full-screen dock/zoom
input was also observed separately through UTM; automated measurements use QMP.

Upstream revision and the map's URL, version, size and SHA-256 are pinned in
[sources.json](sources.json). See [NOTICE](NOTICE) and [licenses](licenses) for
CoMaps Apache-2.0 provenance; font and stb notices are also exported with builds.
Map data is © OpenStreetMap contributors under ODbL. Attribution remains visible
in the viewer; source and map distributor links are in the notice.
