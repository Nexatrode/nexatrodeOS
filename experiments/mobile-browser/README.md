# Mobile NetSurf integration

This opt-in UTM profile adds NetSurf to the existing phone launcher alongside
SimpleX and NexaWal. It uses the same native NetSurf engine and isolated HTTPS
worker as the desktop experiment, with a phone toolbar, touch scrolling and the
shell's on-screen keyboard. The page is rendered at 360 CSS pixels and 3× scale
inside the 1080×2340 phone display. Normal phone and desktop builds are unchanged.

## Build and run with disposable data

Prerequisites are the prepared public accelerated-UTM SDK/code inputs, block
driver configuration, pinned `build/netsurf-native` dependencies and the ordinary
Microkit composer. Use the non-managed composer below, rather than the desktop
lock/lifecycle composer. The builder compiles a new development-key vault; it
does not read an installed vault ELF, installation manifest or personal disk.

```sh
python3 experiments/mobile-browser/build.py \
  --inputs /path/to/public/utm-accelerated \
  --storage build/utm-vault-key-storage \
  --composer build/desktop-graphical/composer/target/release/microkit \
  --output build/mobile-browser/phone-ready --observe
python3 tests/mobile_browser_utm.py \
  --guest build/mobile-browser/phone-ready \
  --template /path/to/public/template.plist --leave-running
```

Use `--resume` to rebuild a marked staging directory and `--reuse` on the test
runner to update only its recorded disposable VM. The runner verifies source
and loader hashes, VM identity and the blank development disk before replacing
the loader. It refuses personal-data sessions and linked disk images. Building
alone changes no VM. `--observe` enables test-only UI observation and must only
be used with disposable data; it does not enable native browser fault hooks.

The NetSurf Home tile opens the browser; its trusted HOME button returns to the
launcher. Tap the URL field or a page text field to show the keyboard. HIDE
restores the full page height; GO submits the address or form. Drag the page to
scroll. Contacts remains available from Tools in this opt-in profile.

## Verified November 26, 2026

The canonical disposable UTM run passed all nine scenario groups:

- Home launch, phone-sized toolbar and native page viewport.
- Local HTML/CSS and visible JavaScript DOM changes.
- URL entry, on-screen keys, deletion and touch GO with a resized viewport.
- Separate PNG and JPEG images with rendered colour landmarks.
- Touch-focused text input, checkbox and an exact native GET form submission.
- Drag scrolling to the document footer and returning to the top.
- The shared image decoder page at phone width.
- Real HTTPS fetching and rejection of a wrong-host TLS certificate, without
  delivering an HTTP response body from that host.
- Home/SimpleX/NexaWal navigation, browser reopening and no browser input while
  it is hidden.

The tested loader SHA-256 is
`51c8c32e0e79fbb899ce2035ee3efd7b4b924fc6598a80bfbf02124f853d9815`.
Generated `build.json`, `session.json`, serial trace and
`interaction/validation.json` record provenance and results. Screenshots and
OCR evidence are in the same `interaction` directory. These are QMP-driven
functional checks, not physical touch-to-display latency measurements. They do
not test live SimpleX messaging or wallet syncing in this phone assembly.

Six host contract tests reject unintended vault mappings, writable browser
surfaces/results, writable RTC access and overlapping UI mappings. The shared
desktop frontend also rebuilt byte-for-byte identically to its previously
tested ELF. These checks are regression coverage, not a security proof.

## Boundaries

The browser and HTTPS worker receive no wallet, chat or vault memory mappings.
The worker has its own TCP wire page and slot 4; wallet and agent wire pages are
separate. Backgrounding cancels browser activity and releases the shell's page
image, while retaining the native browser process and heap.

This is a development-key assembly, not the shared wrapped-key desktop lock
profile. The existing phone lock gates shell input and hides the browser, but
this experiment does not establish browser heap erasure or full-session key
eviction. It must not be used to claim encrypted browser storage or to migrate
personal data.

Page rendering remains on the CPU. NetSurf's web compatibility limits remain;
this does not add a modern Chromium-class engine. Text entry is ASCII, browser
history/settings are not persisted, and uploads/downloads are not integrated.
Pixel hardware, its Wi-Fi integration and saved-data VMs are outside this test.

## Optional offline Maps

Add `--maps --observe` when building a new disposable stage such as
`build/mobile-browser/phone-maps`. Use the existing large-page composer
`build/desktop-simplex/composer-large/target/release/microkit`; the expanded
phone assembly exceeds the smaller packager's object budget. The same runner
recognizes the Maps profile and uses `tests/mobile_maps_input.py`. Rebuild with
`--resume` and run with `--reuse` to update only that stage's recorded disposable
VM rather than create additional fixtures.

Home has SimpleX, NetSurf, NexaWal and Maps tiles, plus smaller Tools and Settings
cards. Maps uses the same bounded native Gibraltar reader and renderer as the
desktop. The private native surface is 1080 pixels wide at 3× scale; its height
matches the phone viewport. Drag to pan, tap +/− to zoom and Fit to restore the
overview. HOME belongs to the trusted shell. The on-screen keyboard is not
needed for these map controls; hardware arrows and zoom keys also work.

Labels use real glyph bounds, shadow/padding reservations and duplicate-name
suppression, with places preceding roads and at most 24 labels. The font-bounds
test covers both 2× and 3× rasterization. The disposable observation page exports
only public map camera/timing and bounded label rectangles; tests independently
check that the rectangles fit the viewport and do not intersect.

Maps receives only its private frame, heap, input queue and observation page.
It cannot read the vault, chat/wallet data, raw global input, the browser's
network slot or any other app heap. The shell maps the map pixels read-only,
forwards input only while Maps is active/unlocked and releases its image when
backgrounded. Queue backpressure reserves space for focus/reset controls.
This does not add full-session heap erasure or wrapped-key lock integration.
The normal phone profile is unchanged; this is an opt-in UTM assembly with a
fresh development-key disk. There is no GPS, offline place search, routing or
region downloading yet. Pixel hardware and Wi-Fi remain separate work.

### Maps verification — November 27, 2026

The disposable four-app phone passed real scanout colour landmarks, touch
pan/zoom/Fit, HOME navigation, hidden-map input/frame suppression, returning to
the same camera and access to SimpleX, NexaWal, NetSurf, Tools and Settings.
Five captured map states exported 10–24 label rectangles each; independent
checks found no intersections or viewport clipping. The same VM then passed
all nine existing mobile browser groups, including JavaScript, PNG/JPEG, forms,
keyboard/scrolling, HTTPS and wrong-host certificate rejection.

Twelve acknowledged map pans with four bounded host CPU workers measured
8.958 ms median / 14.179 ms maximum from guest map handler to frame publication.
This excludes shell image copy, host presentation and physical touch latency.
The tested phone loader is
`f58570eecbc29916713361ad02f9c4c84da7cb578640206c80211b007e7d65cd`.
Evidence, source/loader hashes and the browser regression result are under
`build/mobile-browser/phone-maps/interaction/`. An initial missing HOME position
was fixed; a subsequent full-page OCR miss was resolved with an independent
focused header check. Failed observations remain separately recorded.
Native UTM inspection confirmed the rendered screen, but native pointer
automation was inconclusive. It is recorded separately in
`interaction/native-input-observation.json`, not counted as a native mouse pass.
The VM was returned to its normal window with cursor capture released.
