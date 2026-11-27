# Native seL4 GPU feasibility checkpoint

The capability probe is an isolated, diskless Microkit image. It maps only the QEMU VirtIO MMIO
transport window and its own DMA allocation. It has no block/network driver,
wallet, chat database, installation key, application surface or clipboard.
It never changes an existing VM's display configuration.

The probe negotiates modern VirtIO, discovers GPU capability sets, retrieves
them with fenced requests, and creates a VirGL context, retained until device reset for scanout. It then creates a
64×64 render target, clears it through VirGL, and checks every BGRA pixel after
a fenced transfer back to guest memory. A checkerboard texture is uploaded once;
three overlapping textured rectangles are composed at sixteen positions and
each output is compared with a CPU reference. These use VirGL blits, not a
complete OpenGL implementation or a general shader API.

The current CPU desktop is unchanged. Passing this probe does **not** prove
physical host GPU identity, full-resolution performance, presentation latency,
Slint GPU rendering, a production compositor, or SDL/wgpu compatibility.

## Reproduce

```sh
python3 tests/desktop_gpu_probe_build.py \
  --inputs /path/to/public/utm-accelerated --output build/desktop-gpu/new-probe
caffeinate -di python3 tests/desktop_gpu_probe_utm.py \
  --guest build/desktop-gpu/new-probe \
  --template /path/to/public/utm-accelerated/template.plist \
  --device virtio-gpu-gl-device
```

Run the same loader with `--device virtio-gpu-device` as a 2D control. It should
report no VirGL context and no rendering test. Build a separate output with
`--skip-clear` and run the GL case as a negative control: it must fail pixel
verification rather than report rendering success. The runner exits nonzero for
that deliberate failure; inspect its `result.json` for the pixel failure and
successful cleanup. A rendering failure must never become a software-rendered
"GPU pass".

Each run creates a uniquely identified, diskless VM, saves serial output and
results outside the bundle, and stops/deletes only that VM in its cleanup path.
`--inspect-seconds 45` briefly holds it for native display inspection;
`--keep-running` explicitly retains that one disposable VM after texture
verification. It does not turn a failed capture into a successful test result.
Fresh output/case directories are required to avoid overwriting evidence. Loader
and source digests are verified before launch. No saved-data fixture is imported.

## Results on November 24–25, 2026

Installed UTM 5.0.8 accepted `virtio-gpu-gl-device` with the existing ARM64
Microkit SDK. The 2D control advertised no capability sets. The GL device exposed
capset IDs 1, 2 and 4, and accepted fenced context creation; an earlier capability-only case also
verified context destruction.
Clear/readback and the retained-texture test passed. Omitting the render command
failed on pixel zero, proving the check detects the absent draw. Every probe VM
was stopped and deleted. See ignored `build/desktop-gpu/` outputs for raw logs,
source/loader hashes and per-case results. The small readback experiment's
elapsed time includes command submission, polling, readback and verification;
it is not a desktop frame-rate benchmark.

The first render attempt exposed an uncached-memory alignment fault from merged
wide stores. The probe now uses `-mstrict-align` and explicit device barriers.
This is part of the tested compile contract, not a change to seL4 or Pixel code.

The full-resolution variant (`--desktop-size` on the builder) also passed
initial pixel verification over a 2560×1600 target and sixteen retained-texture
updates. However, scanout capture remained black after successful SET_SCANOUT
and RESOURCE_FLUSH responses, including after retaining the context and polling
for eight seconds. The full-resolution runner therefore correctly reports
`passed: false` with `GPU readback passed but scanout pixels did not match`.
Native UTM inspection subsequently showed the correct red surface and overlapping
checkerboards: GL presentation works. QMP's `screendump` captures a black CPU
surface in this setup. The automated probe deliberately continues to fail that
capture check; GPU readback and native display observation are separate evidence.
The captured pixels and QEMU device tree are retained for investigation.
No accelerated desktop default was enabled.

Next: identify the host rendering backend, test buffer lifetime/reset and collect
equivalent CPU/GPU timings. The capability result supports further GPU development;
it does not yet justify enabling accelerated desktop composition by default.

## Experimental three-app connection

`tests/desktop_apps_utm.py --browser --virgl` builds an opt-in trusted renderer
PD instead of the original GPU driver/virtualizer/client chain. The CPU image
remains the default and fallback. Apps still render with Slint/NetSurf in CPU
memory. The new PD reads their bounded surfaces, retains BGRA textures, composes
them with VirGL blits, and alternates two fenced scanout resources. GPU commands
and DMA mappings are never granted to app PDs. The manager forwards notifications
without copying desktop pixels or waiting for GPU completion.

The GPU shell exports the backdrop and three canonical window-decoration
surfaces separately. Geometry-only dragging is handled directly by the trusted
manager; it changes neither the backdrop nor the app pixels. The compositor
moves each cached decoration and app texture together. Decorations are refreshed
when focus changes. Rounded edges use bounded clipped blit spans; alpha-blended
composition remains future work. The original cyan arrow cursor is retained.
The renderer has a bounded 2 ms / 10 ms scheduling context above the shell, so
the shell cannot prevent it from presenting during a drag. Completion handling
clears only the response header rather than 64 KiB of uncached storage.

This is not a full OpenGL API, a Slint GPU backend, physical GPU identity
verification, or a refresh-synchronized frame scheduler. Dynamic sizing is
provided separately by the optional display-agent integration below.

Add `--gpu-verify` for diagnostic tests: each frame is read back, sampled against
an independent CPU layering reference, and exported through a seqlock-protected
RGB surface for host pixel checks. `tests/desktop_gpu_capture.py` captures this
readback instead of QMP's black CPU surface. These captures establish rendered
content, not physical display latency. Verification adds considerable overhead
and must not be used as the normal performance configuration.

The combined diagnostic test passed with a fresh disposable identity: real
two-way SimpleX relay traffic, two received photos (one during wallet scanning),
full wallet synchronization against `rpc.nexawal.com`, NetSurf local HTML/JS and
HTTPS during scanning, and app minimize/restore. Its wallet-tab median was
216.8 ms and p95 399.8 ms across 12 host samples, including diagnostic readback,
CPU-reference verification, file I/O and polling. This is not a normal-build
latency result or a physical-display measurement. Evidence is in ignored
`build/desktop-apps/virgl-verify/validation.json` and the associated captures.

The subsequent layer-separated diagnostic drag test completed twelve distinct
frames while the button was held. Its backdrop revision stayed at 4 for all
twelve samples. `gpu-held.png` captures the moved decoration and app content
before release. These tests use an explicitly mapped observation page at
`0x5fba0000`; ELF report object addresses were unsuitable for the initial
completion-counter experiment. One earlier run timed out on the GPU queue;
that failure is preserved in `build/desktop-gpu-drag-nav.log`. A fresh run
passed, but timeout recovery and host-backend stability remain release gates.

The normal build's twelve held-drag samples had a 155.5 ms median and a
3135.9 ms p95 (the maximum with twelve samples); three-app dock switching had
a 306.4 ms median and 481.3 ms p95. These include QMP submission, repeated
observation-page reads and polling. They prove completion before release, not
smooth 60 Hz motion. The large first-drag outlier and initial texture uploads
need further profiling. The CPU backend remains the default.

The layer-separated build also passed the full combined app regression:
bidirectional relay messaging, two received photos, NetSurf local JS/HTTPS,
window restore and a complete disposable wallet sync. An initial DNS timeout
recovered through the existing backoff. The photo assertion now waits for
visible image pixels after decode readiness, rather than racing asynchronous
frontend publication. Results are in `build/desktop-apps/virgl-drag-verify/`.

### Bounded frame submission

The subsequent held-motion profile found that unchanged app images took only
microseconds to inspect, while five serial command completions consumed nearly
the entire frame. The renderer now fits the complete three-window drawing
stream in one 16 KiB request and enqueues draw, scanout and flush together. Each
request has independent DMA and response storage. One frame remains in flight;
all fenced responses are checked before any storage or scanout target is reused.
Resource creation and resize retirement still use synchronous commands; normal
texture uploads now use the completion-driven phases below.
Ring indices wrap independently of the monotonically increasing 64-bit fences.
The renderer's scheduling budget and app authority have not changed.

The timing-only observation ring records the last 48 frames (snapshot,
composition, presentation and completion waits); it contains no app content or
key events. `tests/desktop_gpu_motion.py` drives a held drag and reads these guest
timestamps. This measures completed guest GPU work, including scheduling and
host command delays; it does not establish physical display latency or VSync.
`tests/desktop_gpu_transport_test.c` runs the production transport against a fake
device, including reversed completion order, full queues, DMA preservation,
ring/fence wrap, and six malformed-completion controls. Run it with AddressSanitizer
and UndefinedBehaviorSanitizer as well as validating pixels in diagnostic UTM.

The first two pre-batching runs had 96 and 123 ms median frame durations. Two
subsequent batched runs reached 46–52 completed frames/s with 15–17 ms median
frame durations, but another batched run stalled severely (1.8 frames/s). All
samples, including that failure to sustain smooth motion, are preserved in
`build/desktop-apps/resize-verify/motion-*.json`. These short sequential trials
ran on a variably loaded Mac; they are not a controlled idle/busy comparison or
a sustained 60 Hz claim. Remaining work includes completion-driven scheduling,
frame pacing and profiling the long-tail stalls. Do not enable expensive
`--gpu-verify` readback for normal interactive use.

After the queue/fence checks, the final normal build completed two further
four-second held-motion runs at 2624×1644: 58.10 and 58.49 completed frames/s.
Median frame work was 10.17 and 10.20 ms; completed-frame interval p95 was 32.85
and 28.25 ms. Earlier trials used 2560×1600 and host load varied, so retain the
separate modes and all outliers when comparing them. The diagnostic variant
passed independent layout checks and CPU-reference pixels at 2560×1564 and
2624×1644, dock switching, held dragging and search typing. Native UTM inspection
of the final normal build showed the complete moved window before button
release. Input for that held-frame check was QMP; it is a separate host display
observation, not a physical-input latency measurement. Evidence is in
`build/desktop-apps/resize-verify/motion-validation.json`. Disposable `13014BA0`
was reused; no additional VM, saved data import or Pixel change was needed.

### Frame pacing and completion wakeups

Normal VirGL frames now use an explicit preparation/upload/presentation state
machine. The renderer owns the former GPU driver's IRQ (79, local channel 1),
acknowledges VirtIO interrupt status and resumes only after validating completed
command leases. Texture uploads are drained in bounded groups of four before
drawing; draw/scanout/flush remain one bounded batch. Producers may update their
own pages while a frame is pending; the renderer coalesces the notifications and
reads the latest scene for the next frame. It never overwrites pending DMA or
recreates scanout resources before the old frame completes.

A dedicated timer endpoint schedules starts at least 1/60 second apart. Late
frames rebase the next start rather than emitting catch-up bursts. When idle,
the renderer requests no periodic frame timer; a stale one-shot watchdog may
still arrive. A 50 ms watchdog checks pending completions without spinning,
records recovery separately, and enforces a three-second pending-queue deadline.
This is software pacing, not host-display VSync or a physical latency guarantee.
Initialization, resize resource creation/retirement and diagnostic readback
remain synchronous; steady drawing and texture uploads do not busy-wait.

Observation counters distinguish IRQ wakeups, paced deferrals, coalesced
requests and watchdog recoveries. Timing samples separate total frame elapsed
time from elapsed handling spans across callbacks, excluding the time returned
to Microkit. Handling spans can still include CPU preemption; they are not a
hardware CPU-cycle accounting measurement. The normal motion test checks the
minimum start interval, bounded queue, zero blocking GPU waits and real IRQ
completions. Host tests cover a 1000 Hz notification storm, delayed completion,
no reuse/append while pending and skipped catch-up bursts.

The initial eight-second 120 Hz input run completed 454 held-drag frames at
56.76 frames/s: 452 frame completions arrived on IRQ and two were observed on
an already pending input wakeup. There were no watchdog recoveries or blocking
GPU waits. Median handling span was 0.044 ms; median total frame elapsed time
was 7.71 ms. Longer runs retained severe completion stalls up to about 1.6 s,
including a 20.44 frames/s 15-second run and a 25.11 frames/s eight-second run.
Their renderer handling spans remained around 0.045 ms at the median. The
transport was returning to Microkit while waiting for completions, but these
results do not establish consistently smooth presentation under the variable
host workload. All runs remain in `motion-paced-*.json` in the resize staging
directory; do not discard slow samples or infer a sustained 60 Hz result.

The final normal build also passed an eight-second 120 Hz input run at the
1600×1200 minimum desktop size: 55.83 completed frames/s, 17.54 ms median and
23.52 ms p95 completed-frame intervals. It had no blocking GPU waits or watchdog
recoveries, and median handling span was 0.044 ms. During the following three
seconds idle it submitted zero frames and received zero GPU IRQs or timer
wakeups. An earlier final-run attempt was rejected because UTM changed mode
during measurement, then repeated after the mode settled. Diagnostic pixels,
dock/held movement and typing passed at 2560×1564 and 2624×1644. A native CUA
drag moved the complete SimpleX window in the final normal build; separate
read-only scene observations confirmed geometry and completed presentation.
Source/loader provenance and all test scopes are in
`build/desktop-apps/resize-verify/paced-scheduling-validation.json`.

The original `virgl-drag` images use a fixed 2560×1600 scanout. The newer
`--desktop-resize` integration adds a display-only SPICE/VirtIO serial agent,
bounded 1600×1200–3840×2160 physical modes, and a constant 2× logical scale.
The manager owns layout and clamps windows; the GPU acknowledges a mode only
after completed scanout/flush. A size/stride change forces a full shell redraw
and invalidates incompatible retained app/decorations. The Slint shell uses
preferred dimensions rather than a fixed width/height binding. This avoids
reusing pixels at the old stride and allows the dock to follow the actual size.
The launcher sets `-spice agent-mouse=off`: the native display agent consumes
only monitor configuration, so native mouse input must stay on the VirtIO
tablet. Scanout textures use the current mode dimensions, not a cropped
maximum-sized texture, because SPICE normalizes tablet input using the backing
texture size. Mode changes fence scanout disable, resource detach and release,
then recreate the two targets within the existing maximum guest DMA reservation.
See the desktop-apps README for the native UTM test and its scope.

### Timestamped completion profiling

`desktop_apps_run.py --gpu-command-trace` creates an explicitly disposable
fixture with timestamped QEMU GPU events, without keyboard or app-content
logging. `desktop_gpu_stall_trace.py` correlates its fence IDs with the guest's
bounded timing history. Samples also record the longest interval between
completion polls. This distinguishes time inside the host command path from
time before dispatch or before the guest observes a finished command. The
residual includes dispatch, scheduling and observation; it is not a direct
CPU or GPU execution-time measurement. The motion harness now checks that the
Mac is unlocked before and after a visible-drag measurement and waits for a
stable display mode before sending input.

Steady-frame batches now fence only the final request, rather than each draw,
scanout and flush separately. All individual responses are still validated,
and every request/response/texture lease remains unavailable until the entire
batch and its final fence retire. Requests are finalized before publishing the
available-ring index, so the device cannot see a partially updated fence policy.
Initialization and resource retirement retain their synchronous fences. The
transport test deliberately delays the final fence after earlier unfenced
acknowledgements and rejects reuse or appending in that state.

The compositor also merges adjacent equal-inset rounded-corner strips. With
three visible windows and the cursor this reduces the draw stream from 132 to
84 blits (11,616 to 7,392 command bytes), while the independent CPU pixel
reference retains the original mask. Windowed and full-screen diagnostic
checks validate the complete moved content, dock and rounded edges. Diagnostic
readback follows the same presentation batch as the normal build; it is still
synchronous and must not be enabled for performance measurements.

The initial timestamped baseline did not reproduce a seconds-long stall, even
when the native window was minimized. Later baseline trials at 2624×1644 did:
one frame spent 1,699 ms from host draw dispatch to flush-fence response, then
had a 9,678 ms maximum gap between guest completion checks. Its total guest
frame took 11,250 ms. That trial overlapped a source rebuild; a separate trial
without the rebuild still had a 2,911 ms guest frame (1,258 ms in the measured
host span, with a 1,725 ms guest poll gap). These traces demonstrate both host
command-path delay and delayed guest observation. They do not establish that
an extra fence, occlusion or a single GPU operation caused every long stall.

Two optimized, timestamped 15-second runs at 2624×1644 completed 55.36 and
55.18 frames/s, with 17.74/17.54 ms median and 24.53/24.19 ms p95 frame
intervals. The maximum intervals were 152.54 and 107.14 ms. Across their 1,655
correlated frames, host draw-dispatch-to-flush-completion was 5.65 ms median,
10.61 ms p95 and 35.63 ms maximum. The longest total-frame outlier spent
135 ms in preparation, before the measured presentation batch. Neither run
used a blocking GPU wait or watchdog recovery; each submitted zero new frames,
IRQs and timer wakeups during the following three seconds idle. Host load varied
across these sequential trials, so these are observations, not a controlled
speedup ratio or proof that the old seconds-long tail is eliminated.

Keep failure evidence alongside successful runs: the diagnostic readback
timed out during a locked-Mac/full-screen transition. After restarting while
awake, windowed 2560×1564 and full-screen 2624×1644 transitions passed the
independent pixel and held-drag checks. Two subsequent normal restarts stalled
before graphics initialization. The original register sampler repeatedly read
CPU 0 despite its per-core labels: those captures show CPU 0 in CLH acquisition,
but do not establish that all four vCPUs were blocked. An unchanged-image retry
booted successfully. That startup issue must not be attributed to the
renderer's new final-fence policy. The tracing-only disposable fixture was retired after
its logs and manifests were copied into the local staging directory.

The final trace-free normal image, SHA-256
`9980a884bc9667eb66070c0a83d0b20febf89d69fc99468157e088c525aba7d7`,
completed 832 held-drag frames in a 15-second 120 Hz input run at 2624×1644:
55.37 completed frames/s, 17.60 ms median, 24.90 ms p95 and 81.02 ms maximum
completed-frame intervals. Blocking GPU waits and watchdog recoveries remained
zero. The following three seconds had one background frame, four GPU IRQs and
one timer wakeup, ending idle; this sample is retained alongside the earlier
zero-frame idle intervals. After returning to windowed mode, a separate QMP
held drag moved SimpleX by 40 logical pixels. Native UTM display observation
showed the complete moved window before release. CUA coordinate gestures were
unavailable (`noWindowsAvailable`) in this final native-window check, despite
successful AX menus and an unlocked host; do not label it a native-input timing
test. The existing disposable `13014BA0` is left running with this normal image.
Full provenance, rejected attempts and scope are recorded in
`build/desktop-apps/resize-verify/stall-profiling-validation.json`. The changes
remain local until an explicit commit/push request.

### Startup investigation and monitor ownership

The follow-up retained the same kernel and guest loader. Five ordinary cold
boots, eight with the old installer's early QMP polling, and eight with six
temporary host CPU-load processes all reached rendered readiness. This did not
reproduce a kernel deadlock. Early polling alone therefore did not establish
the cause of the earlier hangs. All temporary load processes were terminated.

One of those 21 rendered-ready snapshots had no `SLINT|FRAME_COPY_RAM_V1`
markers yet. It was an early capture, so it does not prove that the old
launcher's text check would have timed out. Serial output can also interleave;
removing its role in readiness is additional hardening. VirGL readiness now checks a stable GPU
timing publication, two completed frames, valid dimensions, and the two Slint
frontends' initialization words. It reads no app text. Fault text is still
retained, and actual missing GPU/frontend initialization still times out.

QMP FIFO clients now hold a nonblocking advisory lock for their complete
lifetime. A second cooperating reader fails before opening the shared reply
stream instead of consuming another request's response. This also covers
`MotionQmp` through its base class. The lock inode remains in place after close;
closing releases ownership, including repeated cleanup calls. A deliberately
competing client was rejected on the live disposable monitor while its owner
continued to query it successfully.

`desktop_boot_stress.py` records repeated cold starts and, on failure, pauses
only the disposable guest, captures kernel lock memory and explicitly sends
`cpu-index` with every register request. It verifies the returned CPU number.
An independent live capture confirmed distinct CPU#0–CPU#3 reports and granted
CLH request states in a healthy paused guest. These fixes improve startup
verification and future diagnosis; they are not a speculative kernel patch or
a claim that the original intermittent wait has been eliminated. The original
kernel SHA-256 remains
`b5b54db4c7ede5daefaa7918e919fb01f56429ee002151803a3b1679784b053c`.

The final five cold boots additionally checked both frontend initialization
words; each ran in a distinct QEMU process and passed (3.83 seconds median,
3.91 seconds maximum). The updated launcher independently booted and retired
a fresh blank-disk fixture. Five host tests cover torn/uninitialized readiness,
exclusive monitor ownership and incorrect vCPU selection. The final trace-free
15-second 120 Hz QMP drag run at 1600×1200 completed 54.14 frames/s, with
17.78 ms median, 27.05 ms p95 and 107.24 ms maximum completed-frame intervals.
It had no blocking GPU waits or watchdog recoveries, then submitted zero frames,
IRQs or timer wakeups over three seconds idle. Host display inspection showed
the complete SimpleX window moved by 40 logical pixels while the QMP button
was still held. These are regression checks of the unchanged guest image, not
evidence of a kernel performance improvement. Results and the correction to
the earlier CPU-label interpretation are retained in
`build/desktop-apps/resize-verify/kernel-startup-validation.json`.

## Partial redraw and decoration checkpoint — November 25, 2026

Composition now repairs one conservative rectangle per frame instead of
repainting the entire render target. Each alternating target keeps its own
last **completed** scene, source revisions and cursor bounds. Moving/hiding a
window includes its old and new bounds; stacking changes include changed
overlaps. Content/decoration changes include their visible window bounds.
Resize invalidates both target histories. All backdrop, rounded decoration,
content and cursor blits are clipped to that repair rectangle. A target's
history advances only after its final successful presentation fence.

The backdrop publishes an eight-generation dirty-rectangle journal under its
existing seqlock. The renderer unions every intervening update, snapshots only
that rectangle, and uploads it with the matching stride and byte offset. Each
target separately accumulates those updates until repaired. Missing, overwritten
or malformed history falls back to a full snapshot/upload/repair. The protocol
adds 160 bytes after the existing 16-byte backdrop header only in the VirGL
shell; CPU shell and decoration headers retain their previous layouts.

Focus no longer repeatedly changes the main fullscreen Slint render target into
a decoration target. A private, window-sized Slint adapter produces retained
decorations. Only the previously and newly active decorations are regenerated;
initialization or changed app height regenerates all three. The old image's
focus control republished revisions `[10,10,10]` as `[12,12,12]`. In the new live
checks, all twelve switches in each mode/build changed exactly the two affected
decorations and left the third revision unchanged. Dock hover/pressed feedback
still works and uses the backdrop's bounded damage rather than being disabled.

Two diagnostic runs at 2× (windowed 2560×1564, fullscreen 2624×1644) passed GPU
readback comparison with the CPU scene reference while six bounded CPU workers
ran. Each exercised twelve focus changes, six completed held-button moves,
minimize/restore and cursor damage. Independent resize/layout checks also passed
typing and held dragging in both modes. Native UTM inspection of the normal
build showed the entire SimpleX body and decoration moved while the QMP button
was still held. These inputs are QMP-injected; the native screenshot checks
presentation separately, not physical mouse latency.

The normal, trace-free image then completed 15-second 120 Hz input trials with
the same six workers and three overlapping apps:

| 2× mode | Completed guest frames/sec | Interval median / p95 / max | Damaged area / full-frame equivalent |
| --- | ---: | --- | ---: |
| Windowed, 2624×1564 | 54.99 | 17.64 / 26.11 / 99.52 ms | 31.37% |
| Fullscreen, 2624×1644 | 54.00 | 17.71 / 28.78 / 69.51 ms | 29.59% |

Both had zero blocking GPU waits, zero watchdog recoveries, a maximum of four
command leases, and zero frames/IRQs/timer wakes during the final three idle
seconds. The workers consumed about 106 and 101 child CPU-seconds over 26-second
trials. Cursor-only checks damaged 0.13% and 0.12% of a full-frame equivalent.
QMP-observed switch-to-first-completed-scene medians were 89.20 and 83.95 ms;
updated decorations were observed by 95.22 and 100.02 ms median. Polling and
command round trips are included, so these are not physical tap-to-photon
measurements. The diagnostic readback build is excluded from these timings.

This reduces composition area substantially, but the overlapping layers still
cause overdraw inside that area. Full scanout/host flush is retained: target
damage and the host's previously presented front-buffer damage are different
histories. There is no host-vsync feedback yet, and these runs still contain
70–100 ms interval outliers. Earlier full-redraw runs had different dimensions
and host load; the recorded frame rates are not a controlled speedup ratio.
No kernel, Pixel GPU or Wi-Fi change is part of this checkpoint.

Normal loader SHA-256:
`9bbb82c58de0c6797dfcc28e0b7c43a9f3e6fb474fc90a7e030ad1cf8770d213`.
Raw manifests/results are in ignored
`build/desktop-apps/resize-verify/{damage,motion,damage-load}-partial-*.json`,
with diagnostic/normal build and install logs under `build/desktop-damage-*`.
The diagnostic checks initially caught excessive backdrop invalidation and an
input-coordinate rounding error in the new harness; both were corrected before
the passing runs. Failed attempts remain in those logs.

Reproduce against an already-running **disposable** 2× fixture (normal or
`--gpu-verify` diagnostic build):

```sh
python3 tests/desktop_damage_load.py \
  --guest build/desktop-apps/your-disposable-fixture --label windowed
# Enter fullscreen through UTM, then:
python3 tests/desktop_damage_load.py \
  --guest build/desktop-apps/your-disposable-fixture --label fullscreen
```

The wrapper starts/reaps only its own six self-terminating load workers, records
their CPU time, and rejects expired load. Diagnostic builds perform pixel checks;
normal builds additionally run held-motion timing. It never provisions saved
data. The production clipping/damage headers also pass ASan/UBSan tests against
an independent per-pixel scene oracle, including alternate buffers, cursor
trails, z-order, hide/restore, source changes and resize. Deliberately omitting
old bounds or using the other target's history must fail that oracle. Focus
mask, GPU queue/pacing, three-app stacking and resize tests pass; the CPU-only
desktop shell also compiles with the shared platform-adapter change.

## Protocol references

- [VirtIO GPU protocol](https://docs.oasis-open.org/virtio/virtio/v1.3/virtio-v1.3.html)
- [VirGL command protocol](https://gitlab.freedesktop.org/virgl/virglrenderer/-/blob/main/src/virgl_protocol.h)
- [UTM display support](https://docs.getutm.app/settings-qemu/devices/display/)
