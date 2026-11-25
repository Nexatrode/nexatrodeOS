# Native NetSurf experiment

This separate ARM64 Microkit guest runs the real upstream NetSurf HTML parser,
DOM, CSS cascade/layout, form handling, document history and BMP/PNG/JPEG decoders.
It uses a new C frontend with CPU plotters and packaged Roboto font rendering
through stb_truetype. It is not a Linux guest, a remote browser, a screenshot
viewer. JavaScript is an optional build mode described below.

The default offline mode packages its resources into the executable: HTML documents, an external CSS
file, a deterministic BMP, NetSurf's user-agent stylesheets and English messages.
The displayed `resource:` URL is a local packaged resource, not an Internet URL.
The form uses NetSurf's actual GET form encoding and loads a local result page.
It does not echo submitted data into HTML or store it.

## Scope

- Standalone full-screen browser at 1280x800 logical / 2560x1600 physical pixels.
- Local links, Back/Home/Reload/Stop controls, text input, backspace, checkbox, GET form
  navigation, mouse-wheel scrolling and keyboard scrolling.
- A dedicated single-threaded browser protection domain, separate from the
  display/input/timer drivers. This initial full-screen frontend publishes
  directly to its own display plane; it is not yet a window in the desktop dock.
- No network service, filesystem, wallet, vault, chat or physical Pixel driver
  is included in the default offline mode. Its disposable VM has a new blank
  16 MiB disk and restricted host networking, without guest storage/network
  services.
- SVG/path painting, general site browsing, uploads/downloads,
  profile persistence, browser lock integration and site-per-process isolation
  are outside the initial offline experiment. The opt-in HTTPS mode below
  adds real websites without storage; `--javascript` additionally enables
  Duktape and NetSurf's existing DOM bindings.

Picolibc supplies portable C-library operations. The frontend implements a
bounded heap and monotonic scheduling, with explicit failure for unavailable
file/socket operations. The experiment does not supply a general POSIX or Linux
application ABI. It uses the accelerated VM's exported architectural counter;
the fixed offline wall-clock base is not a trusted source for online TLS.

## Build

Use this chat's separate worktree. The existing SDK, driver images and template
below are read-only public build inputs; the installation key and regular VM
disk are never opened. No physical hardware or Wi-Fi build is involved.

```sh
python3 experiments/netsurf-native/build.py --guest \
  --inputs /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --display build/desktop-shell/retina-ready-2x
```

The Mac host needs LLVM/lld, CMake, Meson, Ninja, Make, Python, Perl,
pkg-config and ImageMagick. The image validation also uses NumPy. The builder downloads only the pinned public source archives
in `sources.json`, verifies SHA-256 hashes and extracts them under this
worktree's ignored `build/netsurf-native/` directory. It cross-builds Picolibc
without semihosting or threading, and the required NetSurf libraries.
Host generators run on the Mac; the browser and its libraries are ARM64 ELF.

Small changes in the staged NetSurf tree add the frontend build target, disable
unsupported OS features, remove the file fetcher, and add the local resources
to the resource fetcher's whitelist. The builder contains the exact changes;
no upstream checkout is edited. `--engine-only` can reuse the prepared dependency
builds during frontend development. A matching prepared 1x display may be
selected with `--scale 1`; the initial successful interaction run is at 2x.

The staging directory `build/netsurf-native/guest-2x/` contains the source and
artifact hashes, capability inventory, compiler/composer logs, licenses and
loader. No generated object, archive, VM or downloaded implementation source
should be committed.

## Disposable UTM test

```sh
caffeinate -di python3 tests/netsurf_native_utm.py \
  --guest build/netsurf-native/guest-2x \
  --template /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated/template.plist \
  --leave-running
```

The runner creates a new VM UUID, copies only the experiment loader, creates a
blank disk, disables host sharing and starts UTM in disposable mode. It checks
the loader digest and explicit offline/HTTPS build flags. Without `--leave-running`, it stops
and unregisters only its own VM. On failure it records the disposable serial
log and cleans up that VM. It never controls the regular VM or the existing
desktop trial.

Tests send actual QMP keyboard/tablet events and check actual guest scanout
with OCR, a decoded image pixel landmark, and the engine's form-navigation
callback. They verify a typed value after backspace, checkbox inclusion in the
GET query, local result content, scrolling to the footer and back, a new wheel
scroll, a link to another document and browser Back. They finish at an empty
form. This is functional evidence, not a physical-display latency benchmark.

`interaction-result.json`, serial output and screenshots are stored beside the
build manifest. `session.json` identifies only this newly created disposable VM.

## Native HTTPS mode

Add `--https` to the build command. The separate output is
`build/netsurf-native/guest-https-2x/`; the offline image remains available.
The Rust worker is a separate Cargo package with a committed lockfile. Reuse
prepared engine dependencies with `--engine-only` when appropriate.

```sh
python3 experiments/netsurf-native/build.py --engine-only --guest --https \
  --inputs /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --display build/desktop-shell/retina-ready-2x
caffeinate -di python3 tests/netsurf_native_utm.py --https \
  --guest build/netsurf-native/guest-https-2x \
  --template /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated/template.plist \
  --leave-running
```

The explicit runner flag enables QEMU user NAT only for this new browser-only
VM. There is no host-side fetch proxy. Guest lwIP performs DNS and TCP over
VirtIO Ethernet; the Rust broker uses the existing `nexatrode-guest-https`
rustls adapter. The pinned WebPKI roots verify the certificate chain and DNS
name at the current PL031 RTC UTC time. Fresh TLS sessions use the guest VirtIO
entropy service. The C runtime's fixed offline epoch is never used for TLS.

The UI has no raw TCP, entropy or RTC mapping. It writes a 4 KiB request page;
the worker has that page read-only. The worker writes a separate result region
that the UI can only read. Sequence numbers match jobs, cancellation replaces
the current job, and the worker owns its exclusive TCP slot in this otherwise
app-free network image. Slot 2 reuses the tested wire protocol, not a live wallet
session. No wallet, vault, SimpleX, storage driver, installation key or saved
app data is present. The image includes only display/input/timer, network,
entropy, browser and HTTPS services. The regular VM is never controlled.

The worker supports GET, current TLS certificate validation, bounded headers,
Content-Length, chunked responses and authenticated TLS close-delimited
responses. It rejects ambiguous framing, truncation, unsupported encodings,
oversized results and malformed URLs. It requests identity encoding. Each
response is limited to 2 MiB with 16 KiB metadata; the UI feeds at most 16 KiB
to NetSurf per fetch poll. TLS work runs outside the UI with timer-driven
progress, a 15-second idle timeout and 45-second overall timeout. The frontend
has a 50-second broker watchdog. The serialized queue allows 32 fetch contexts;
redirect handling uses NetSurf's existing redirect-count limit. There is no
HTTP fetcher or silent HTTPS-to-HTTP fallback. Cookies, authentication,
uploads, POST and persistent profiles are not implemented. Responses are
buffered before delivery; connection reuse and progressive body delivery are
future improvements.

Home opens a packaged page with real HTTPS links. Clicking the address bar
starts a fresh URL entry; Enter navigates and Escape leaves editing. Failure
text is visible in the bar. F5 reloads the intended main document and its
resources; Escape outside address editing stops current fetches and queued
layout. Reload after Stop retries the stopped destination, even while the old
page is still visible. Stop does not disable JavaScript or its future timers.

The bar follows main-document loading, resource loading, completion, failure
and cancellation. Its HTTP code belongs to the main document, including cached
pages and redirects. A missing image cannot replace a successful page's code
with 404, and old-page callbacks cannot mark a new navigation complete. Local
navigation clears stale online errors. The HTTP status is not a claim that all
page features or images are supported.

Validation on 2026-11-24 in 4-vCPU accelerated UTM at 2x:

| Real page | Native main-document download | Observed scanout |
| --- | --- | --- |
| `https://www.netsurf-browser.org/` | 10,212 bytes, 610 ms | Page text and layout visible |
| `https://www.iana.org/domains/reserved/` | 11,009 bytes, 378 ms | Reserved-domains text and tables visible |
| `https://www.kernel.org/` | 20,436 bytes, 110 ms | Release table and page navigation visible |

These are individual DNS/connect/TLS/body timings, not full-page load times,
medians or responsiveness benchmarks. External CSS/subresources add more work.
The automated test checks actual scanout OCR, rejects both
`expired.badssl.com` and `wrong.host.badssl.com` without delivering a body,
returns Home during an unresolved request, and then loads a new NetSurf About
page in the same boot. `https-result.json`, screenshots, manifest and serial log
are retained under the HTTPS stage. The local HTML/CSS/BMP/forms/history/scroll
suite also passed again in a separate offline disposable VM.

The standalone HTTP tests cover fragmented fixed/chunked bodies, trailers,
redirect/interim responses, size bounds, invalid framing, URL injection and
unauthenticated/truncated EOF. Run with:

```sh
rustc --edition 2024 --test experiments/netsurf-native/https-worker/src/http.rs \
  -o build/netsurf-native/http-tests
build/netsurf-native/http-tests
cargo test --manifest-path experiments/guest-https/Cargo.toml --locked --offline
```

The current example.com document downloads and parses its title, but its body
appears blank in this port. That is an observed rendering limitation, not a
successful visual website test. SVG/WebP/GIF support, some CSS effects and glyphs outside Roboto's coverage
remain missing. PNG and JPEG support is described below. Tested sites can therefore have empty
image slots, overlapping elements or missing characters. This experiment does
not establish compatibility with arbitrary sites or modern web applications.

## PNG and JPEG

The browser uses NetSurf's upstream image handlers with pinned libpng 1.6.59
and libjpeg-turbo 3.2.1. Both libraries cross-build as static ARM64 libraries
with NEON enabled, using the same freestanding C runtime as the browser. There
is no host image decoder or host file service in the guest. The builder verifies
that both image handlers survived linking; changing feature flags invalidates
the staged engine objects so old registration code cannot silently disable them.
The explicit `sources.json` URLs, versions and SHA-256 values cover both new
libraries; their original license texts are copied to the stage.

The PNG path includes transparency, palette/grayscale conversions and Adam7
interlacing. The JPEG path includes ordinary baseline, progressive, grayscale
and CMYK images. Rendering uses the existing RGBA bitmap/CPU plotter pipeline
at native 2x density, including alpha blending and resizing. No GPU drawing
API is required for this addition.

Each decoded image is bounded to 4096 pixels per side and 4,194,304 total pixels
(16 MiB RGBA). JPEG dimensions are checked after reading the header and before
starting decompression; PNG dimensions/area are checked before speculative
bitmap allocation. libpng also bounds side lengths, chunk caching and individual
chunk allocation. The existing 2 MiB HTTPS response limit and 64 MiB browser
heap remain in force. Huge photos are rejected rather than automatically
scaled down during decoding; efficient scaled decoding and decode scheduling
are future work.

The build packages synthetic PNG/JPEG patterns and a `resource:images.html`
page. It exercises RGBA transparency, interlaced PNG, baseline/progressive/CMYK
JPEG, damaged files, and valid image headers declaring 3000x3000 pixels above
the decoded-pixel budget. The image test compares each distinct rendered cell
with its reference, checks the rest of the page remains visible and navigates
Home after rejected images. It also compares real guest screen pixels with
public NetSurf logo PNG, W3C PNG/JPEG image-test references, and an embedded
JPEG photograph on the W3C Berners-Lee page. The NetSurf logo
is checked inside its real website page, alongside page text. Public reference
files are downloaded only as host-side pixel oracles under ignored build output;
they are never packaged or copied to the guest. Guest DNS/TCP/TLS fetches the
website images independently.

The normal HTTPS UTM test runs this image suite after its existing website,
certificate rejection and recovery tests. `--https --images-only` isolates the
image checks during development. `image-result.json` and the `images-*.png`
screenshots record the outcome. These are functional image/rendering checks,
not an image throughput or input-latency benchmark. No remote website image is
committed to the repository.

## Optional JavaScript

Add `--javascript` alongside `--https` to both build and test commands:

```sh
python3 experiments/netsurf-native/build.py --engine-only --guest --https --javascript \
  --inputs /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --display build/desktop-shell/retina-ready-2x
caffeinate -di python3 tests/netsurf_native_utm.py --https --javascript \
  --guest build/netsurf-native/guest-https-js-2x \
  --template /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated/template.plist \
  --leave-running
```

This separate image links the Duktape 2.7.0 source bundled in the pinned NetSurf
revision and its generated WebIDL bindings. Pinned nsgenbind 0.9 runs only on
the build host (requires Bison and Flex); it is not a guest process. The toolbar
shows `JS on`. The default build still omits JavaScript.

Inline and external scripts, DOM access, event listeners, `setTimeout` and
`Date.now()` use the upstream engine. UTC comes from a read-only RTC query to
the HTTPS worker; the UI does not gain raw RTC or storage mappings. Script
interrupt checks use the guest monotonic architectural counter and a 250 ms
per-call execution budget. This interrupts a looping JavaScript call; it is
not a whole-page CPU quota or a hard real-time guarantee for native DOM/image
operations. Memory still shares the browser's bounded 64 MiB heap.

The builder serializes WebIDL generation, fixes the pinned libDOM traversal
that dispatched a target's click listener twice, and synchronizes JavaScript
`input.value` / `textarea.value` setters with NetSurf's existing form widget
and damage path. These patches are reproducible from `build.py`; generated
bindings and downloaded engine code remain under ignored build output.

**JavaScript support is partial.** The native deferred DOM reflow path below
now paints post-load changes to text, page structure and supported CSS. There
is no claim of modern application compatibility, JavaScript modules,
WebAssembly, media, or a complete modern browser API surface.
The experiment still has no persistent site storage, wallet, vault or chats,
and one browser process handles all pages.

The JavaScript suite checks actual guest scanout for inline/external script
results, a timer-updated form field, UTC time and exactly one counter increment
per real pointer click. It verifies that an infinite loop is interrupted and
Home still works, then visits NetSurf, kernel.org and QuirksMode over native
HTTPS. The QuirksMode check requires script-generated `navigator` output, rather
than treating a successful static page download as proof of scripting.
`javascript-result.json`, `js-*.png` and serial output record outcomes and
observed site errors. `--javascript-only` isolates this suite during development;
the normal command also runs HTTPS/certificate, PNG/JPEG and dynamic DOM layout regression checks.

## Deferred DOM redraw and reflow

The native integration batches DOM text, attribute and subtree invalidations
for a scheduled update after the JavaScript/event stack returns. It constructs
a fresh box tree and CSS selection context from the **existing DOM**. It does
not reload the page, reparse the original HTML, rerun scripts or reset their
closures. Initial document conversion still uses upstream scheduling.

A rebuild clears stale CSS node data and box backlinks, recreates form widgets
from current DOM values, refreshes images/image maps and lays out the new tree.
Old render resources are released only after successful conversion **and final
layout**. Allocator failures, including upstream code paths that otherwise
continue with incomplete content, cause rollback to the prior render state and
a visible update-failure notice. A subsequent mutation or Reload can recover.
Conditional backlink retirement
covers detached nodes retained by JavaScript as well as floats/list markers.
Unowned form widgets are released during rebuild and normal document teardown.
The pinned libDOM class-token cache is refreshed when class attributes change;
the JavaScript `CharacterData.data` binding is now implemented too.

Typing preserves the field value, character-index caret and keyboard focus when
the same field survives. Page text selection is cleared. The frontend preserves
the current scroll offset when a URL notification is unchanged and clamps it
when the new document becomes shorter. Home/End keys are forwarded to focused
editors and still scroll ordinary pages. Pending layout work is cancelled on
close/destruction and rearmed on reopening a dirty cached document.

This first implementation uses a **full synchronous layout transaction**, not
incremental per-node layout. Mutations coalesce across a 16 ms scheduling
interval, with the first queued deadline preserved while more changes arrive;
rebuilding waits for initial conversion, changed stylesheets and non-image
resources to settle. Pending images do not block it: the new tree attaches its
image handles before old handles are released, with pending-resource accounting
restored on rollback. No event-loop turn occurs during the swap. A preflight
limit of 20,000 DOM nodes / depth 512
bounds eligible documents. Dynamic frames/iframes are deliberately excluded;
unsupported documents retain the last render tree and show an update-failure notice.
Synchronous geometry reads within the script that made a change do not force
an immediate layout flush. The existing 64 MiB heap and incomplete CSS/browser
APIs still apply. These bounds are checked after DOM mutation. The separate
fragment limits below now protect `innerHTML`; other native DOM operations can
still monopolise the UI before the reflow guard runs. The JavaScript bytecode
deadline cannot interrupt an ongoing C DOM operation.

`tests/netsurf_native_reflow_input.py` checks actual scanout for `textContent`,
`CharacterData.data`, `innerHTML`, insertion/removal/reinsertion, class and
inline-style changes, stylesheet editing/removal/reinsertion, `display:none`
and image `src` changes. It verifies wrapping moves subsequent content, checks
stylesheet/image pixels, repeats 21 further updates, types across a timer-driven
reflow, checks that a continuous 5 ms mutation stream still produces intermediate
layouts, grows/scrolls/shrinks the document and navigates away with pending work.
It records guest rebuild times and live heap use separately from host-visible
input/display latency. `--https --javascript --reflow-only` isolates these
checks; the normal JavaScript-enabled runner includes all browser suites.
Results are in `reflow-result.json` and `reflow-*.png` beside the build manifest.

## Recovery tests

`--test-hooks` builds a separate `guest-https-js-test-2x` image. Only this image
contains allocation fault controls (F8 for successive constructor allocations,
F9 for the first final-layout allocation). Normal images assert that the fault
control symbol is absent. Real allocator failures are detected in both modes.

The test image also trusts an ephemeral, seven-day CA **only for `10.0.2.2`**,
the QEMU host-loopback address. The test server uses actual guest TCP/TLS, with
certificate chain, IP-name and time verification still enabled. Public names
always use the ordinary WebPKI roots. Fixture private keys remain under ignored
`build/`; neither the extra trust anchor nor its controls enter normal browser,
wallet or chat images. The build asserts that the fixture CA is absent from the
normal HTTPS worker. Certificates nearing expiry are regenerated for a rebuild.

```sh
python3 experiments/netsurf-native/build.py --engine-only --guest --https \
  --javascript --test-hooks \
  --inputs /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --display build/desktop-shell/retina-ready-2x
caffeinate -di python3 tests/netsurf_native_utm.py --https --javascript \
  --test-hooks --recovery-only \
  --guest build/netsurf-native/guest-https-js-test-2x \
  --template /Volumes/untitled/sel4-personal-os/.cache/utm-accelerated/template.plist
```

The runner requires matching explicit test flags. Recovery checks cover failed
construction/layout allocations, retained scanout, subsequent recovery, bounded
DOM updates, reflow while an image is downloading, main versus subresource HTTP
status, redirects, cached Back, Reload, Stop and ignored late responses. They
record actual scanout and native diagnostics in `recovery-result.json` and PNGs.
`--recovery-group reflow` or `network` isolates a subgroup during development;
the default is all recovery checks. Normal builds omit `--test-hooks` and run
the existing HTTPS, image, JavaScript and reflow suites together.

## Bounded native fragments

`innerHTML` parses a detached fragment before removing existing live children.
The pinned libDOM Hubbub adapter now has opt-in, parser-internal limits; ordinary
main-document parsing retains its original behavior. Only these dynamic
fragments enable a 128 KiB input cap, 4,096-node budget, depth 256 and a 100 ms
parsing deadline. The parser checks node creation, insertion/reparenting work
and the deadline inside its native callbacks. A final tree walk validates the
accepted fragment too. Limits are declared in `include/native_dom_work.h`.

A limit failure destroys the staged fragment, raises a JavaScript `RangeError`
after C resources have been released, and retains the previous DOM and render
tree. The page shows its existing update-failure notice; a later small change
can recover. Detached insertions no longer activate page styles, scripts or
images or schedule visible reflow. Accepted fragments activate through their
normal live insertion callbacks.

These are deliberate compatibility limits: sites that replace larger fragments
receive an error. The deadline is checked between parser operations, so it is
not a hard execution-time guarantee. Live transfer and cleanup remain synchronous
and are bounded by the tree sizes rather than a preemptive timer. Other DOM
APIs and main-document parsing still need separate work.

The native-work suite replaces 500- and 2,000-element flat lists, rejects larger,
deeper and node-heavy fragments, verifies retained DOM/text and scanout, then
types and returns Home in the same boot. Test images additionally use F10 to
expire the next parser deadline through the actual guard; the control is absent
from normal binaries. Run the suite with `--javascript --https --dom-work-only`
and the usual guest/template arguments, adding `--test-hooks` for its explicit
test image. Full JavaScript-enabled runs also include the normal native-work
checks after the existing suites.

`dom-work-result.json` records parsing, live transfer and cleanup separately.
These are individual guest timings, excluding host input/display latency. The
diagnostics log only sizes, counters and timing, without fragment contents.

## Current limitations

This is a new frontend, not an audited browser security boundary for arbitrary
web content. Plotters, font support, navigation/input handling and resource
limits are intentionally limited. The browser has a 64 MiB heap; its initial
ELF is about 2.6 MiB, including resources and the font. No peak-memory or
page-load benchmark has been established. Mouse wheel and keyboard scrolling
are tested; full touch gestures and drag-selection behaviour need more work.

The static callback table must outlive `init`: NetSurf retains the table pointer.
The first integration boot caught that lifetime requirement; the interaction
test exercises asynchronous completion and repeated navigation after `init`
returns. Inspect the upstream revision and third-party notices before modifying
or distributing this build.
