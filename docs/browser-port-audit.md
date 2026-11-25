# Native browser feasibility audit

Date: 2026-11-24. Scope: source-level comparison of upstream NetSurf, MacSurf
2.3 and the local Ladybird source against NexatrodeOS. No browser was compiled,
run in UTM or tested against live websites for this audit. Effort rankings below
are engineering judgments, not measured build times or performance results.

Follow-up: the separate [native NetSurf experiment](../experiments/netsurf-native/README.md)
now cross-builds and passes local HTML/CSS/BMP, form input/submission, navigation
and scrolling checks in disposable accelerated UTM at native 2x. The original
audit below remains a source comparison. An opt-in native HTTPS mode now also
passes real NetSurf/IANA/kernel.org rendering, certificate rejection and
recovery checks. That limited evidence does not establish arbitrary website
compatibility or Ladybird feasibility through a running port.

The opt-in JavaScript image now links NetSurf's bundled Duktape 2.7.0 and DOM
bindings. Disposable UTM checks verified inline/external scripts, timer-driven
form repainting, exactly one counter increment per click, current RTC-based
`Date.now()` and recovery from an infinite loop. NetSurf and kernel.org rendered
with scripting enabled. QuirksMode's browser-detection page displayed its
script-generated navigator listing, while its shared script reported a syntax
error at line 76 (the inspected script uses `let` there). This is useful partial
JavaScript evidence, not modern-web compatibility. The initial JavaScript port
exposed static-layout limitations. A follow-up native layout transaction now
redraws/reflows supported DOM text, structure and CSS mutations, with UTM
checks for wrapping, forms, images, repeated updates and navigation. See the
experiment README for its resource, subresource and dynamic-frame limitations.
The experiment records scanout, serial diagnostics and per-site errors in the
ignored `guest-https-js-2x` stage; no saved app data or storage service is used.
In the wider HTTPS regression run, the IANA page still rendered its checked
content, but its script reported `TypeError: cannot read property 'length' of
undefined`. Successful text/image rendering is recorded separately from full
script compatibility.

The final JavaScript-enabled run passed the HTTPS/certificate, PNG/JPEG pixel
and JavaScript suites together in one disposable 4-vCPU, 2x UTM boot. The tests
accept an already fetched page from NetSurf's in-memory cache only alongside
visible page content and native HTTPS evidence from that boot. Certificate
negative checks retry a transport failure at most twice and still require an
actual verifier rejection; a connection reset alone cannot pass the check.

The DOM reflow follow-up passed all four suites together in a fresh disposable
UTM boot, including a 60-update continuous mutation stream, input/caret
preservation, page growth/shrink scrolling and cancellation of queued work on
navigation. The final small-page fixture measured a 1 ms median / 7 ms maximum
guest rebuild time across 21 repeated updates; 32 intermediate rebuilds occurred
before the continuous stream ended. These are guest layout timings, not host
display latency or a general website benchmark. The implementation uses full
render-tree transactions and excludes frame/iframe documents from this new path.

The recovery follow-up passed a separate disposable UTM fault suite: six
constructor allocation failures plus a final-layout allocation failure retained
the old scanout and recovered through a subsequent mutation or Reload. Final
layout now completes inside the transaction before old resources are freed.
Depth/node-count rejection also retained the previous layout and recovered after
restoring a small DOM. Pending images no longer block DOM updates; rollback and
Stop were exercised with an image still downloading over real guest TLS.

Main-document status is now independent of image/favicon results. Tests covered
a successful document with a missing image, actual main-document 404, redirects,
cached Back, loading feedback, Reload/F5, Stop/Escape, late responses and an old
page resource finishing during a new navigation. The fixture CA is scoped to
QEMU loopback and compiled only into the explicit test image; normal builds
assert that both the extra CA and failure-injection controls are absent. The
JavaScript-disabled build passed its offline UTM interaction suite as well.
The normal JavaScript-enabled build then passed all four existing suites in
one new 4-vCPU, 2x disposable UTM boot. Its 21 repeated small-page rebuilds
measured a 1 ms median / 4 ms maximum, with 278,512 bytes of live heap growth
over that sequence; these remain guest-work measurements, not display latency.
Source, loader and test hashes, blank-disk size and disabled host sharing were
checked against the running VM. Evidence is recorded in the ignored
`build/netsurf-native/hardening-validation.json` and each guest stage.

The stress work also exposed a remaining limitation: a very wide native DOM
parse can stall before the post-mutation reflow guard runs. The bytecode timeout
does not interrupt C DOM operations. This recovery work is not evidence of
bounded execution for arbitrary hostile pages.

The next follow-up added parser-internal limits for dynamic `innerHTML`
fragments: 128 KiB, 4,096 nodes, depth 256 and a soft 100 ms parsing deadline.
Rejected fragments are discarded before live children are removed; callers get
a `RangeError`. Detached staging nodes no longer activate page-side work.
The initial main-document parser and other DOM APIs retain their prior behavior.

Disposable UTM checks verified size, node-count, depth and forced-deadline
rejection, retained DOM and scanout, subsequent typing/navigation, and 96 repeated
node-heavy refusals. The normal build passed the HTTPS, image, JavaScript,
reflow and native-work suites together, and the JavaScript-disabled build passed
its interaction suite. The updated reflow-failure and network-recovery suite
passed separately with explicit test hooks. The normal image excludes those
hooks and the temporary fixture CA.

In the normal build's individual samples, 500- and 2,000-element replacements
took 21 ms and 88 ms of guest native work. The oversized 11,000-element input
was refused before parsing. After the wider browser workload, cleanup dominated
some node-limit refusals; the maximum across 96 repetitions was 579 ms. Cleanup
and live transfer are still synchronous, so the parsing deadline is not a
frame-time guarantee. These are guest-work timings, excluding host input and
display latency. Source/loader/test hashes and the running disposable VM were
verified in `build/netsurf-native/dom-fragment-validation.json`.

## Recommendation

Try **upstream NetSurf with a NexatrodeOS frontend** first, as a bounded native
port experiment. Use MacSurf as a reference and a possible source of selected
engine improvements, rather than importing its Classic Mac application.
Keep Ladybird as the larger candidate if the required websites exceed NetSurf's
capabilities. A successful NetSurf port would validate OS interfaces; it would
not establish that NetSurf is the final browser for this OS.

Neither option requires Linux binary compatibility or a general POSIX layer.
Both require real equivalents for the OS operations they use. A small C runtime
and browser-private adapters may have familiar C/POSIX-shaped interfaces without
making the OS a Unix environment. Unsupported operations must fail explicitly;
success-returning stubs are not a valid implementation of security or threading.

## Source provenance and limits

| Source | Inspected revision | Qualification |
| --- | --- | --- |
| NexatrodeOS | `327e5b8a2b991f075398bc65ac6eb59441671d9d` | Our separate `codex/simplex-main-work` worktree; the starting tree was clean. |
| Upstream NetSurf mirror | `170d89462442c286f80c0604f30e4c7f2a801265`, committed 2026-11-18 | Downloaded pinned source from the project's GitHub mirror. |
| MacSurf | Published `v2.3` implementation source archive | SHA-256 `f2260f57731baa614cd8f1625521df0c21afcb41a46385d875cc3e68f27aba11`, checked against GitHub's release-asset digest. |
| Ladybird | `162412c81a44f37b9aa5686a431c1c5b0f9dd31c` | Local checkout has pre-existing edits; decisive runtime/build files used here were separately checked to match HEAD. No edits were made there. |

Downloads, provenance and 14 source checks are stored locally under the ignored
`build/browser-audit/2026-11-24/` directory in `sources.json` and
`audit-evidence.json`. No downloaded build scripts were executed. The source
archives are not vendored into this repository.

The MacSurf URL redirects to `ptrickyy/macsurf`; that accessible repository is
currently archived. This is a maintenance concern for choosing a base, not proof
that every related project has ceased development. Its future canonical
development location was not established by this audit.

Source entry points:

- [NetSurf pinned source](https://github.com/netsurf-browser/netsurf/tree/170d89462442c286f80c0604f30e4c7f2a801265)
- [MacSurf release and implementation archive](https://github.com/ptrickyy/macsurf/releases/tag/v2.3)
- [Ladybird pinned porting guide](https://github.com/LadybirdBrowser/ladybird/blob/162412c81a44f37b9aa5686a431c1c5b0f9dd31c/Documentation/Porting.md)
- [Current desktop capabilities and boundaries](phone-desktop-plan.md)

## Comparison

| Concern | Upstream NetSurf | MacSurf 2.3 | Ladybird at the inspected revision |
| --- | --- | --- | --- |
| First native port | Smallest integration surface of these candidates; still substantial work. | More platform and fork integration than an upstream NetSurf frontend. | Largest runtime and dependency gap. |
| Interface boundary | C callback tables for windows, bitmaps, font layout, scheduling, fetching and storage. | NetSurf interfaces plus extensive Mac-specific and fork-specific changes. | Separate UI and platform ports, principally LibWebView, AK, LibCore and LibIPC. |
| Drawing | CPU plotters can target our bounded pixel surfaces. | QuickDraw drawing and Mac font integration need replacement. | CPU Skia path exists; full compatible Skia/text dependencies still need a port. |
| Concurrency | An initial single-tab engine can use a cooperative event loop in one isolated process. | Cooperative Classic Mac event loop, with additional Mac networking/thread dependencies. | Real threads, helper processes, IPC, shared memory and synchronization are central. |
| Networking | libcurl is the default, but fetchers are replaceable. | Open Transport and macTLS integration must be replaced or adapted. | RequestServer, libcurl/OpenSSL and the event loop need native integration or replacement. |
| JavaScript | Duktape bindings; a supported no-JavaScript build exists. | Adapted QuickJS plus custom DOM/browser bindings. | LibJS and the surrounding DOM/platform implementation. |
| Modern-site goal | Compatibility must be checked against actual target sites. | Additional CSS/DOM/JS work is promising but not a compatibility guarantee. | Broader modern-web ambition, with a much larger port; inspected source still identifies itself as pre-alpha. |
| Maintenance | Pinned mirror has recent activity; keep local adapter patches small. | Accessible source repository is archived; fork changes span several libraries. | Larger evolving upstream and dependency stack to track. |

These are not host-browser speed comparisons. No blocks-per-second-style metric,
page-load measurement or memory-use estimate was produced.

## What NexatrodeOS already supplies, and what remains

| Service | Existing foundation | Browser work still required |
| --- | --- | --- |
| Display and input | Isolated RGB surfaces, mouse/keyboard routing, focus, damage handling and native 2x shell rendering. | Browser content adapter, caret/selection, scrolling, font metrics, clipping and surface lifetime. General resize/maximize is still absent. |
| Scheduling and clocks | seL4 scheduling, monotonic timers and UTC/local display-time services. | An engine event loop with deadlines, cancellation and wakeups; expensive parsing must stay outside the shell process. |
| Network and TLS | Working app-specific TCP, DNS, certificate-validated guest TLS, entropy and clock adapters. | Browser-owned connections, asynchronous fetch, cancellation and backpressure. Existing fixed app slots cannot simply be reassigned to an untrusted browser. |
| Persistent data | Scoped vault and encrypted Notes experiments with interrupted-save recovery. | Browser profile storage, cookies, cache, downloads and file grants. Existing record APIs are not a general filesystem or SQLite VFS. |
| Lifecycle | Dynamic app creation/destruction and lock cleanup demonstrated in separate fixtures. | A browser-specific launch contract, resource admission, fault recovery and grants; general multiple dynamic apps/tabs remain work. |
| C/C++ support | Freestanding C and a restricted C++/Skia runtime. | Real library semantics for the selected engine. This is not an existing general libc/pthreads environment. |

Concrete boundaries in our current code:

- `experiments/slint-notes/skia/runtime.cc:306-340` has single-thread-oriented
  synchronization shims; `pthread_create` returns `EAGAIN` and condition waiting
  does not wait. This runtime must not be presented as thread-safe to Ladybird.
- In that file, `mmap` at line 391 allocates heap memory, `mprotect` at line 410
  returns success without changing page rights, and `fopen` at line 412 is
  unavailable. These are restricted renderer scaffolding, not browser services.
- `tests/net/include/haskell_tcp_protocol.h:13` declares four session slots,
  already assigned to existing apps/workers. Its base payload is 128 bytes, with
  a separate wallet-specific wide receive path. A browser needs its own broker
  and bounded buffering, not unrestricted use of those channels.
- `experiments/desktop-graphical/image.h:7-8` limits executable packages to 4 MiB
  and load addresses to `0x200000..0x1000000`. The graphical manager currently
  maps an 8 MiB dynamic-app heap. These are fixture policies, not seL4 limits;
  neither candidate has been shown to fit them.
- `experiments/desktop-shell/protocol.h` describes two fixed-size app windows.
  It is not a general tab/process/window allocation API.

The current landscape shell also restricts networking and does not include the
real app/vault integration. Reusing the working phone network implementation
still requires explicit integration into a disposable browser image.

## NetSurf native port

The useful boundary is `desktop/gui_table.h`, registered through
`netsurf_register`, rather than the complete existing framebuffer application.
The framebuffer frontend is a reference for the callback implementation; its
event handling and library dependencies are not automatically freestanding.

Implement a new NexatrodeOS frontend providing:

1. `gui_window_table` for dimensions, invalidation, scroll state, title, URL,
   caret and pointer changes. Route input from our manager into the browser.
2. `gui_bitmap_table`, `plotter_table` and `gui_layout_table` for images, clipped
   drawing and consistent text measurement/rendering. Slint's pre-rasterized UI
   font is not by itself a general web-font/shaping service. Use one font path
   for both layout and painting; test non-ASCII text and 1x/2x coordinates.
3. `gui_misc_table.schedule` backed by monotonic deadlines and notifications,
   rather than copying a busy polling loop. Keep the engine in an isolated
   process so its layout work cannot block the trusted shell.
4. An HTTP(S) `fetcher_operation_table` with asynchronous setup/start/poll/abort
   and defined response ownership. This avoids requiring libcurl's socket API
   in the first port, but requires implementing correct fetch behaviour.
5. Resource loading from packaged assets and a private storage adapter. Exclude
   or replace the default file fetcher; its sources are included unconditionally
   in the inspected build. Unsupported URL schemes must be rejected.

NetSurf's build explicitly permits `NETSURF_USE_CURL=NO`,
`NETSURF_USE_DUKTAPE=NO` and `NETSURF_FS_BACKING_STORE=NO`. These are useful for an
offline first milestone; they do not eliminate all C runtime or filesystem
references. Audit linked objects and undefined symbols before guest admission.

The C dependency set includes libcss, libdom, their parser/string dependencies
such as Hubbub/ParserUtils/Wapcaplet, libnsutils, character conversion and
selected image libraries. Use pinned compatible revisions, cross-build the
required pieces and separate build-host generators from guest code. Supply
memory/string/formatting/math/time functions with correct semantics. Start with
packaged fonts and a small deliberate image-format set.

Even without libcurl, `content/fetchers.h` references `fd_set` through
`utils/inet.h`. The native event-loop/fetch adapter must replace or contain that
descriptor-shaped interface too; disabling a build option alone does not make
the engine freestanding.

For HTTPS, our TLS transport can be reused as a component, not as a complete
browser HTTP implementation. Status/headers, redirects, body framing,
compression, uploads, cancellation and connection reuse need defined handling.
Cookies and origin policy belong to the browser too. Do not solve certificate
failures by disabling verification or silently downgrading to plaintext.

## Why a direct MacSurf port is a different task

The archive contains implementation source, including forked NetSurf, libcss,
libdom and `browser/libquickjs`. It is not just browser controls over stock
NetSurf. For example, `libcss/src/select/computed.c` owns fork-specific grid and
gradient fields, while additional rendering/DOM work appears in both core and
`frontends/macos9` code.

Replace these platform dependencies:

- Carbon window/control and text-editing structures in `macos9.h`.
- QuickDraw plotters, bitmap ownership and Mac font metrics.
- `WaitNextEvent` and the `TickCount`-based scheduler with our event/deadline API.
- Open Transport, Classic Mac Thread Manager and file APIs in the fetch path.
- macTLS's platform transport, entropy, time and resource/CA-bundle integration,
  or replace it with our validated TLS service.
- PowerPC/CodeWarrior-specific prefix headers and build definitions. The
  QuickJS prefix explicitly selects PowerPC and Classic Mac; an ARM64 native
  target needs a separate build configuration and type/alignment checks.

QuickJS parses JavaScript, but modern website support also depends on DOM,
events, fetch, storage, CSS and the binding code. Copying QuickJS into upstream
NetSurf would not reproduce MacSurf's behaviour. First identify a coherent,
testable subset of the fork, with its library versions and notices, rather than
merging large frontend files and compiler shims wholesale.

The source notices identify the browser as GPLv2 with separately licensed
components. Preserve the actual component notices if code is reused. This audit
does not determine redistribution obligations for a future combined build.

## Ladybird native port

Ladybird's documented split between UI and platform ports is useful, but our
present runtime is too small for an unchanged engine build.

Required work includes:

- **Runtime and memory:** a suitable C/C++ runtime, allocator, real virtual-memory
  operations, thread-local storage, stack bounds for garbage collection,
  synchronization and thread lifecycle. `AK/StackInfo.cpp` demonstrates why
  returning guessed stack information is unsafe. Adapt or replace mimalloc's
  platform allocation path and other library assumptions.
- **Event loop:** replace `EventLoopImplementationUnix`, including its
  `poll`/wake-pipe/thread-local behaviour, with native notifications and timers.
- **Process lifecycle:** implement `Core::Process` and helper spawning through
  the trusted manager. Creating a fresh process directly can replace
  fork/exec/spawn semantics; implementing Unix fork is not inherently necessary.
- **IPC and shared buffers:** replace socket pairs, descriptor transfer,
  `AnonymousBuffer` mappings and the IPC I/O thread. `TransportSocket.cpp`
  currently combines these mechanisms. Native endpoint/buffer grants must be
  authenticated and bounded; an integer supplied by an app is not authority.
- **Network:** adapt RequestServer and libcurl/OpenSSL, or supply equivalent
  request operations. A new drawing frontend does not solve this dependency.
- **Rendering/text:** port the required Skia, HarfBuzz/FreeType or equivalent
  font backend, ICU, image codecs and their data files. The existing reduced
  Skia renderer is not a proven substitute. A CPU painting option exists in
  `Services/WebContent/main.cpp`, so GPU support is not the first blocker.
- **Build graph:** the inspected default dependency checks require, among
  others, CURL, OpenSSL, ICU, SQLite, SDL3, Threads, ANGLE and several media/image
  libraries. LibWeb also links Rust components. Omitting video/WebGL at first
  needs explicit build/feature work; it is not an existing universal switch
  that removes these dependencies.
- **Frontend and storage:** implement the LibWebView UI contract using Slint
  controls and our surfaces, plus private profile/resource access and a suitable
  SQLite VFS or other supported persistence integration.

Some pieces can be staged or replaced. This list describes dependencies of the
inspected source, not a requirement that NexatrodeOS implement every Linux API.
The cost is a browser-specific native platform port and ongoing upstream
maintenance, rather than a one-time window adapter.

Ladybird's source labels the core BSD-2-Clause; its dependencies retain their
own licenses and notices.

## Isolation and resource policy

Keep browser content outside the shell and outside wallet/chat address spaces.
Start with one isolated renderer/tab with no wallet, chat, raw-disk or device
capabilities. Grant network/profile access through separately scoped services.
The initial renderer may share a process with its own browser logic; this does
not establish isolation between different websites.

Before enabling untrusted scripts, enforce memory and execution limits, allow
the manager to terminate a stuck renderer, and prove teardown/cancellation cannot
leave callbacks, input ownership, stale buffer grants or requests attached to a
successor. A multi-process engine's original Unix sandbox does not transfer to
seL4 merely by recompiling it. Conversely, seL4 does not implement web same-origin
policy, cookie rules or permission prompts for us.

File access should use explicit grants; disable downloads/uploads until that
path exists. A lock must revoke access to decrypted profile state and clean up
workers according to a tested browser policy. Rendering success is not evidence
of those protections.

## First experiment and decision gates

Use a new disposable ARM64 accelerated UTM image and blank disk. Do not use the
regular VM, installation key, wallet, chat database or Pixel hardware/build.

1. **Offline engine:** cross-link pinned upstream NetSurf and the minimum C
   dependencies. Package HTML/CSS/fonts/images. Draw a document with links,
   editable forms and scrolling through the existing compositor at 1x and 2x.
   Verify undefined symbols, actual binary size and peak allocations before
   changing the fixture's resource limits. Leave scripts and persistence off.
2. **Responsiveness and containment:** type, scroll, move/minimize the window
   during large-page layout; measure shell/input and renderer publication
   separately. Force allocation failure and renderer faults; close/reopen
   repeatedly and verify grants, input and memory are reclaimed.
3. **Native HTTPS:** add a dedicated scoped fetch broker with queued bounded
   connections, partial I/O, cancellation and retries. Verify bad certificates,
   incorrect time, redirects, truncated/chunked/compressed responses and stalled
   connections. Share network infrastructure without borrowing wallet slots.
4. **Compatibility:** run the same recorded site/test set on host baselines and
   the guest: local CSS/forms, an article/docs site, a forum, a JS-heavy site,
   and the actual workflows the owner wants. Record versions, missing browser
   APIs and content restrictions separately from transport failures.
5. **Engine decision:** enable bounded JavaScript only with interruption and
   cleanup tests. If required sites fail because of fundamental DOM/CSS/JS
   gaps, evaluate Ladybird before investing in a broad MacSurf merge. If the
   target set is satisfied, keep a small NetSurf adapter and review selected
   MacSurf changes individually.

This ordering produces reusable surface, network, storage and lifecycle work
even if NetSurf is later replaced. It also preserves the distinction between a
native browser demonstration, a useful limited browser and a browser suitable
for arbitrary modern websites.
