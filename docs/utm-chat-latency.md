# UTM chat-opening latency

An accelerated four-vCPU UTM guest was tested with the same offline copy of a
saved 13-message conversation. The copy had three stored-photo references;
two decoded successfully in the baseline, while one failed to decode. A
subsequent offline diagnostic read all 254,008 bytes of the failing JPEG. Its
header reported 1,333 × 2,370 pixels (3,159,210 pixels), exceeding the current
2,500,000-pixel preview decoder limit; its JPEG end marker was present. This
identifies the configured limit as the reason the current preview rejects it,
not proof that the full image is otherwise undamaged. The guest's networking
was restricted and its disposable disk was discarded.

The instrumented baseline spent 5.35–5.41 seconds preparing bubbles on the
first open. Most of that time was spent reading the three photo references.
The revised guest creates the message rows first and requests a photo preview
only when its row is visible, reading one bounded chunk per timer turn.
Original attachment IDs remain the source for Save. A preview is used when
one exists; older attachments without a separate preview may still require
reading their original bytes.

| Host condition | Baseline first chat header | Revised first chat header | Baseline later opens | Revised later opens |
| --- | ---: | ---: | ---: | ---: |
| Busier, 1-minute host load 39–49 | 5,919 ms | 704 ms | 478, 461 ms | 655, 575 ms |
| Lighter, 1-minute host load 17–18 | 6,340 ms | 556 ms | 445, 479 ms | 424, 440 ms |

Each cell is from a single fresh guest or two subsequent openings, not a
statistical distribution. The timer began before the QMP input command and
ended when ScreenCaptureKit observed the chat header in the UTM window.
It includes the test's press/release pacing and capture sampling; it does not
establish physical-display latency or prove that every photo was loaded.
The host was still doing other work during the lighter run. Diagnostic serial
logging also affects timings. These results are not a comparison of live
network activity or concurrent wallet synchronization.

The offline UTM test also verified that a visible saved photo decoded and that
an older off-screen photo became eligible and loaded after scrolling. The
input service now forwards held-pointer movement to Slint; chat-thread vertical
drags are delivered to the scroll view while the existing edge and home
gesture paths remain deferred. A header repaint check compared the rendered
header before and after a photo update.

## Large photo preview worker

The accelerated UTM build now sends visible JPEGs to a separate bounded
worker on vCPU 3. It returns a preview of at most 480 × 480 pixels and the UI
retains the original attachment as the Save target. The worker accepts images
up to 3.5 million pixels and at most 2,560 pixels per side; larger images
still need a separate thumbnail strategy. A changed conversation or transcript
cannot receive a completion belonging to another conversation.

In a restricted-network, disposable UTM copy of the saved chat, the 254,008
byte photo decoded and appeared. In one instrumented run, decoding occupied
359 ms between worker start and completion. A keyboard tap and guest frame were
published during that interval, with about 208 ms between the injected pointer
down event and frame publication; the simulated tap included about 80 ms
between down and up. These are guest/automated-input timings from one run,
not physical-display latency or a guarantee under live wallet/network load.
Leaving and reopening the chat during decoding retained the resulting photo.
The non-instrumented source rebuild also displayed the photo in the offline VM.

## Background reads and session thumbnail cache

The accelerated guest has an active reader on vCPU 0 with its own transfer
page and a read-only, owner-checked vault endpoint. It reads authenticated
3,072-byte chunks without delaying the UI between requests. The vault keeps
validating the committed attachment and its owner on each chunk. The UI sends
only a stable owner and attachment ID, then hands the completed bytes to the
existing decoder worker. Results for a changed chat, changed transcript, or
cleared cache generation are discarded.

Decoded previews are retained in a 4 MiB, least-recently-used in-memory cache
keyed by owner and attachment generation. The cache is cleared when a chat is
deleted and is not persisted to disk. The current conversation's Slint model
can retain its own image references separately from the cache; releasing those
on scroll and virtualizing long lists remains future work.

In one restricted-network UTM test against the same saved snapshot, the first
chat frame preceded the large photo frame by about 2.26 seconds (previous
instrumented run: roughly 9 seconds). After opening another chat and returning,
the large image was served from the cache with no vault read, and the guest
published a frame about 0.28 seconds after the injected release event. In a
separate run a keyboard frame was published during the 1.82-second background
read. These are individual instrumented guest runs, not end-to-end display
latency statistics or a live wallet/network workload. A wrong-owner read was
also rejected by the vault without returning a payload.
