# UTM frame handoff investigation

Phoc retains damage across output buffers and schedules redraws from output
frame events. The UTM Slint guest already renders partial regions, but reduces
them to a full-width horizontal band. The shared header contains only the most
recent band. If the graphics client skips an even revision, it cannot infer
all of the earlier damage from that header.

The graphics client now copies the complete shared RGB image if a revision was
skipped, the initial frame is incomplete, or an earlier copy raced the writer.
It resumes band copies after a stable full copy. An interrupted copy leaves the
last presented frame in place while the client retries. The producer publishes
its odd revision before exposing the changed metadata and pixels. This remains
a single shared image and is not a display-synchronised swapchain.

`tests/input/slint_frame_test.c` covers skipped disjoint updates, consecutive
band updates, recovery, an in-progress revision, and revision wrap. The
accelerated builder compiles the graphics client from the current checkout so
the fix is not lost to an older prepared ELF.

## Measurements and limits

In November 2026, three Home/Settings cycles per build used a four-vCPU
accelerated UTM VM, a blank temporary disk and restricted networking. The Mac
was busy with other work. Optional serial instrumentation recorded guest
stages, and repeated QMP screendumps sampled the guest framebuffer. It did not
measure physical-display presentation; logging and sampling also perturb the
run. These few sequential samples do not establish a reliable overall tap
latency improvement.

The UI copies full 1080 × 2340 RGB pictures between its rendering buffer,
page cache and shared frame. Its freestanding `memcpy` uses volatile byte
accesses. A local ordinary-RAM copy routine uses aligned 64-bit or 32-bit
accesses when both pointers permit them, retaining byte copies for other
alignments. It is not for MMIO and does not replace the system-wide libc
functions. In the instrumented samples the median cached-page store fell from
16.27 ms (8 observed copies) to 1.00 ms (6), and cached-frame publication from
17.59 ms (10) to 1.71 ms (5). Cached-page restore fell from 17.39 ms (11) to
5.06 ms (4); the latter included a 196 ms outlier. Counts are individual
instrumented operations, not independent full navigation trials.

QMP-sampled Home transitions had medians of 3.37 s before and 3.72 s after;
Settings transitions had medians of 3.79 s and 1.11 s. The three samples in
each group varied widely, including multi-second gaps outside the measured
copy loops. They cannot distinguish guest scheduling, host contention and
instrumentation overhead. More controlled tests with first-visible host frames
and active wallet/chat workloads are needed before adjusting scheduling or
adding animations. The fallback 200 ms graphics poll applies when normal
notifications are missed; it is not a fixed delay for every input.

A subsequent non-tracing optimized build completed two Home/Settings cycles
on the same kind of blank guest. QMP framebuffer-change samples were 444 and
162 ms for Settings and 167 and 165 ms for Home. There is no matching
non-tracing baseline, so these are not a before/after comparison. A separate
navigation smoke test reached Tools, Settings, SimpleX and NexaWal.
