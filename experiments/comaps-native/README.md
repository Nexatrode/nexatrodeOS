# Offline CoMaps-data experiment

This is a native ARM64/seL4 viewer for pinned CoMaps map data. The embedded
profile uses Gibraltar (1,272,536 bytes); storage profiles add two more regions. It reads the pinned `.mwm` directly in the guest,
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

The magnifying-glass button opens offline **name search in the selected region**.
Desktop also accepts S, / or Ctrl+F. Type a place/road name, use arrows and Enter
or tap a result to center it and open its details. Matching folds ASCII case and
repeated spaces; exact and prefix matches precede other substrings. Repeated
named road segments share a result. The first eight matches are shown in a
scrollable list; dragging that list does not pan the map. Escape or × cancels.
On mobile, the existing on-screen keyboard edits the query; SHOW RESULTS hides
it, and OPEN FIRST opens the first result. Tap the query field to show it again.

Search uses a fixed-capacity list of eligible names and anchors, with a
48-byte UTF-8 query and eight result slots. It adds no network or vault access.
Only two read-only keyboard/search visibility flags are shared with the phone
shell. Hidden Maps receives no typing; its keyboard is dismissed on focus loss.
Changing regions rebuilds the index and clears query/results. Queries are not
written to disk. This is not CoMaps' full search engine: address/POI metadata,
search across unloaded regions and Unicode case/diacritic folding are absent.
Non-ASCII name bytes remain visible and match literally when entered.

### Whole-region names with bounded geometry

For chunked Liechtenstein, the build exports every supported named place and
road segment from the complete pinned MWM, independently of the rendering LOD.
The 3,275 records occupy 353,716 bytes at the fixed public-pack offset 48 MiB.
Each record contains a name, kind and original geometry midpoint. The catalogue
is hash-pinned in the guest, read asynchronously by `map_store`, structurally
validated, then retained in a fixed 8,192-record array (884,736 bytes). Oversize
builds fail explicitly. Gibraltar and San Marino continue using their fully
decoded region indexes. Queries, selection and private saved places do not enter
the public catalogue.

Selecting a catalogue result centers its anchor and asynchronously loads only
the required detail pages; the resulting original feature supplies the details
card. The catalogue survives pan/zoom without rebuilding or re-reading it.
Switching regions clears the query/results and selects the appropriate index.
A damaged catalogue fails the region load while retaining the prior map; it
never silently substitutes incomplete viewport-only results. Existing selection
journals, private ciphertext banks and map geometry offsets are unchanged.
Existing disposable packs need their new public catalogue extent installed
while stopped; this experiment is not a production disk-format migration.

`tests/comaps_region_search_host.py` checks full-source coverage and exact
anchors, SHA-256 pins, absence of a minor-road result from the overview LOD,
and sanitized malformed/truncated index rejection. The shared UTM search test
also selects Altneugut from overview, then Ruggell outside the loaded southern view,
with the fixture network disconnected. It checks camera/detail recovery on both
layouts. Address parsing, fuzzy/Unicode folding, routing and cross-region search
remain separate work.

`tests/comaps_search_test.c` checks the bounded engine under sanitizers.
`tests/comaps_search_utm.py --guest <stage> [--mobile]` disconnects only that
disposable VM's `net0` link, exercises place/road search, the real on-screen or
hardware keyboard, clear/backspace, list scrolling, result selection, capacity,
focus isolation and both region indexes, and restores the link afterward.
Results and composed screenshots are stored under `search-verification/`.

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

## Private saved places (opt-in disposable experiment)

Add `--map-private` to a `--maps --map-storage` mobile or desktop build, using a
fresh stage and disposable disks. Search for a place or road, open its details,
and choose **SAVE**. **SAVED** in the footer opens the list. First use enrolls a
**separate Maps passphrase**, with confirmation; it is not the device passphrase.
Save up to 16 named coordinates across the two regions, rename or delete them,
and reopen one to switch regions and center the map. Dragging the list, its
arrows/Enter and the mouse wheel navigate longer lists. **LOCK**, leaving Maps, input overflow and
60 seconds of inactivity close the private vault. Mobile device lock also removes
Maps focus. An unlock never happens merely because an app regains focus.

Maps reuses the existing allocation-free AES-256-GCM-SIV record helpers and
PBKDF2-HMAC-SHA256 (600,000 rounds) wrapped-key prototype. A fresh random 256-bit
key and independent envelope salt/nonce come from the guest's VirtIO RNG. The
passphrase wraps that key; it does not appear on disk. Bookmark counts, names,
regions and coordinates occupy a fixed 2 KiB encrypted payload. AAD covers the
record magic, generation, nonce and key envelope. The entire record must
authenticate before any bookmark is installed. Maps owns the key and plaintext;
its disk service sees only opaque ciphertext in a dedicated shared 8 KiB page.
Neither Maps component receives another app's vault namespace or key.

Two 4 KiB banks at 1 MiB in the otherwise public map pack are reserved for this
namespace. Saves write the other bank and require VirtIO FLUSH before reporting
success. Enrollment persists both banks before acknowledgement. Restart begins
locked, tries newest valid authenticated data first and can recover the older
bank after a torn newer write. Damaged nonempty banks never silently enroll a
new key. This region is separate from both the public selection journal and the
pinned map payload extents. Public map corruption/recovery tests must not reset
these private banks in a personal-data deployment; all current tests are disposable.

Lock zeroizes the live key/envelope, passphrase and confirmation buffers, pending
save, structured bookmarks and shared map surface (including unused rows). The
map returns to its public overview. The mobile shell and desktop compositor can
retain freed pixel copies; this is **not** a whole-RAM, GPU-cache, stack/register
or full-session erasure guarantee. The rest of the four-app profile still uses
its public development storage key. This is an app-scoped prototype, not the
shared three-app wrapped-key restart group, Titan M, secure boot or a production
security boundary. Retry delays are monotonic-session software delays and can
be reset by reboot; disk snapshots can be rolled back and attacked offline.

`tests/comaps_private_utm.py` validates saving, rename/delete, authentication,
focus loss, cold restart and damaged/torn banks with only the recorded disposable
UUID and public-map disk. It checks real composed pixels and verifies that a
private test name/passphrase do not appear in the raw disk. Crypto unit tests
cover envelope and record authentication, fallback and live-key zeroization;
`tests/comaps_private_contract_test.py` rejects cross-app authority additions.

## Larger region and bounded spatial loading

The three-region storage profile adds **Liechtenstein**, from the same pinned
CoMaps series/version: 4,511,920 source bytes, 58,130 features and 906,581 points.
The former whole-map reader hits its point capacity at record 19,863. Increasing
all map arrays would scale memory with region size. Instead, a build-time
streaming visitor uses the existing decoder's fixed workspace to export the
supported rendering subset into an overview and 64 spatial detail pages. The
source MWM stays in the public build cache, with its pinned SHA-256. Geometry
retained in detail pages is unchanged; the overview intentionally omits buildings,
minor roads and non-place points. This remains a CoMaps-data experiment, not the
complete CoMaps renderer, search index or an arbitrary-size region importer.

In the guest, the file buffer remains **8 MiB**, each candidate stays capped at
**32,768 features / 524,288 points**, and there are still two reusable decoded
candidates. Only the pages intersecting the viewport plus a 50% margin are read
at detail zoom (level 3 and above). Pages are checked individually against hashes
compiled from the pinned source, structurally validated and filtered into the
candidate. Each disk request remains at most 64 KiB; completions drive the next
request/page. The current map remains visible until the entire candidate is
valid. A failed chunk retains it. Dense views exceeding a fixed cap fail rather
than silently dropping features or allocating more memory. Search on this region
covers the whole selected region through the separate name catalogue described
below; it is not full-region address search.

Tap the current region name to open the shared **region manager**. It lists the
three installed regions, their runtime sizes, current selection and the chunked
format. Choose one, retry a failed region, or cancel an in-progress candidate.
Cancellation waits for the outstanding bounded disk operation, discards the
candidate and leaves the last successful selection journal unchanged. Progress is
reported per page. These are provisioned offline regions: runtime downloads,
remove/install and updates are not implemented by this manager.

The new disposable pack is **64 MiB**. Gibraltar and San Marino retain their
original offsets (8 and 16 MiB), while Liechtenstein's derived pages start at
24 MiB. Selection records remain at 0/4096; private saved-place ciphertext banks
remain at 1 MiB. Region IDs 0=Gibraltar and 1=San Marino are unchanged, and
2=Liechtenstein is appended. Builders require fresh disposable stages/disks for
this layout; they do not expand or migrate an installed personal-data disk.
Saved places use these stable IDs and absolute coordinates, independently of
which detail pages are loaded. A missing/corrupt region never deletes a bookmark.

`tests/comaps_spatial_host.py` profiles the source and checks page parsing under
ASan/UBSan, including truncations, viewport filtering, capacity exhaustion and
mutations. `tests/comaps_regions_utm.py --guest <stage> [--mobile] [--regression]`
checks manager pixels, overview/detail loading, repeated loaded-host panning,
cancel with a throttled disposable map disk, cold restart, corrupted overview
and detail pages, and recovery. Timings separate disk/DMA, hashing, page parsing
and end-to-end selection. They include scheduling/preemption; they do not measure
physical touch-to-display latency. Full private saved-place regressions remain
in `tests/comaps_private_utm.py`.

## I/O latency tracing

`tests/comaps_io_utm.py --guest <desktop stage> --output build/maps-io/run`
repeats the same Liechtenstein overview/detail viewport, samples the guest timing
page and the dedicated map disk's QEMU block statistics, and optionally adds
four bounded host CPU workers with `--load`. It does not edit a disk, reboot,
enroll a passphrase or import personal data. The recorded script and loader
hash distinguish each experiment.

The guest records doorbell-to-completion-handler waits, request setup/doorbell
calls, interrupt MMIO acknowledgements, DMA copying, service dispatch, and the
handoff back to the renderer, with a bounded wait histogram and worst-request
record. These are elapsed times and include preemption. A stalled doorbell call
is counted in both issue and wait time; **do not sum them**. QEMU's block times
cover its backend completion, not delivery of the guest IRQ. This comparison
locates delays around virtual I/O without claiming to distinguish every host
scheduler wait from a seL4 kernel wait.

Public reads now use up to **124 KiB** of the existing 128 KiB DMA allocation
(the first 4 KiB contains descriptors/control). This reduces MMIO round trips
without adding memory, outstanding requests or raw-disk authority. Empty
spatial cells are skipped. The portable SHA-256 rounds are unrolled in groups
of eight, retain the same pinned hashes and require no optional CPU crypto
instructions. The volatile byte-copy path remains for safe uncached DMA tails.

The public-map service runs below Maps rendering (priority 94 versus 95), with
an 8/10 ms reservation. The former 3/10 ms reservation stretched software
hashing across replenishments even when the UI was idle. The larger reservation
uses available CPU while the lower priority lets map drawing and interactive
frontends preempt it. This trades background throughput during active panning
for immediate drawing, rather than raising a hashing service above the UI.

`tests/comaps_loading_input.py --guest <desktop stage> --output
build/maps-io/drag-loading --load` checks twelve held-drag frames while the
fixture's map disk is throttled to 2 MiB/s and four host workers run. It restores
the disk rate and releases the pointer in cleanup. Both guest publication times
and injection-to-observed-frame times are recorded separately, alongside a
composed screenshot; neither is a physical touch-to-display measurement.
