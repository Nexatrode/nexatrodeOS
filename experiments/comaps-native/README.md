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
VM; a future desktop/mobile integration should use the trusted shell's private
input routing. This image has no wallet, SimpleX, vault, GPS or network services.
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

The next useful milestone is integrating this surface and private input with
the shell, followed by bounded storage-backed map reads. A full CoMaps port
would additionally need its C++ runtime/platform services, Drape graphics backend,
task/thread lifecycle, map management, search and routing. Existing VirGL blit
composition does not supply the full graphics API that Drape expects.

Upstream revision and the map's URL, version, size and SHA-256 are pinned in
[sources.json](sources.json). See [NOTICE](NOTICE) and [licenses](licenses) for
CoMaps Apache-2.0 provenance; font and stb notices are also exported with builds.
Map data is © OpenStreetMap contributors under ODbL. Attribution remains visible
in the viewer; source and map distributor links are in the notice.
