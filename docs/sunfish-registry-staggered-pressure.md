# Staggered registry arrivals and retained writer pressure

IMG_9700 measured10,022.099ms from transaction3 queueing to full publication. Transaction4 was accepted2,499.245ms after that queue and queued55.229ms after publication. The accepted request order explains why the prior handoff did not arm: it required a successful first queue followed by an immediate waiting second dispatch with ELF already pending. It did not handle a predecessor queued on an earlier service pass.

## Offline replay

Private `registry-staggered-offline-IMG_9700-v1` rebuilds the saved owner/router/session fixtures against current production sources and the exact delivered native feature set. Two focused tests pass:

1. A real owner/router receives and queues3 first, then accepts4 and ELF on later passes while3 is unpublished. Initially the peer advertises only64-byte receive intents; the actual manifest-backed avs/audio reply needs88bytes. The current bounded dispatcher returns WaitingReply for4 with no handoff attempt. Its actual publication snapshot has calls5, mask0x47 and five no-candidate gates. Supplying two real fitting intent advertisements and the corresponding positive intent-request ACK allows the ordinary writer to publish3, and the next normal registry service queues4 without receiving3's RX_DONE. This demonstrates no-fit pressure and the trigger miss; it does not identify the Pixel's buffer availability.
2. A real GLINK writer has fitting buffers but queues3 before receiving4 while a grant is pending. The snapshot instead shows control selection and an ACK gate. It preserves backing advertisement, positive grant ACK, request3 RX_DONE, a real QRTR RESUME and reply3. Request4 RX_DONE then precedes reply4. Selected FIFO-blocked writes retry identical bytes; reply intents remain distinct and a credit-packet return does not complete3. This fixture uses a minimal48-byte success reply to isolate transport behavior, not the native88-byte locator payload.

The actual owner credit-limit test also passes again: pending count10 rejects second admission until a real RESUME arrives. The replays do not weaken credits, grant order or ownership. Model ticks are deliberate event ordering, not predicted handset latency.

An initial owner fixture assumed one pending grant would keep3 unpublished, but the actual owner pass can process up to16 receive/write iterations and completed that grant before publishing3. That failed expectation was retained and replaced by explicit fitting-intent pressure. A pending grant alone cannot be asserted as the physical cause from the screenshot.

## What remains unknown

The existing native snapshot does not retain transaction3's writer history after its TX slot retires. Earlier photos and saved Linux/Android source establish possible gates and required ordering, but cannot reconstruct which buffers or packets were live during this boot. A fixed extra writer turn cannot supply a missing remote intent or bypass a pending grant/credit safely.

The next image therefore preserves scheduling policy and retains the already-maintained `RegistryPublication` counters for the exact transaction3/peer0:9 request. This reuses existing instrumentation rather than guessing a new priority rule. It will show actual calls, selection/no-fit/wait masks, first applicable priority gates, completed/blocked ACKs, receive returns and advertisements ahead of a pending ACK, and maximum completed ACK age.

Counters count calls/events, not elapsed time in mutually exclusive causes. A mask bit records occurrence at least once. Gate counts name the first priority exclusion, not complete ordinary-writer eligibility. Mixed pressure can overlap, and a long queue-to-publication interval with few writer calls suggests a service/receive interval outside this publisher. The capture does not promise attribution of every microsecond.

## Observation and image

The optional `sunfish-registry-predecessor-pressure` feature adds no scheduler decision, hardware operation, clock read or resource retirement. While the exact predecessor remains queued, the getter returns a read-only copy of its current counters. After a successful full publication it freezes those counters by opaque owner/generation/request identity; they survive matching completion and slot retirement, and transport reset clears the trace. The live/frozen state is independent of clock presence. Other replies and unrelated completions cannot overwrite the frozen record.

The `REGISTRY TX3 PRESSURE` panel has27fields. OLD SNAP STATE0means no predecessor,1means not frozen,2means full-publication counters frozen. Missing pressure fields remainFFFFFFFF. Paired fields pack label-left count in the high16bits and label-right in the low16bits, each clipped at65535. A present snapshot can therefore showFFFF as a saturated count; it is not missing evidence. ACK MAX AGE MS converts counter ticks to milliseconds; other US fields are hex microseconds relative to runtime start.

OLD MASK bits0through7 mean: no suitable intent observed; outbound intent request outstanding; control selected; other DATA selected; this reply selected but FIFO blocked; this reply written; outbound intent request selected; registry-priority publication committed. COOL/TFTP, ACK/RXDONE, NOCAND/OTHER and CREDIT/SELECT show the eight existing first-gate counters. DONE/ADS AHEAD count actual receive returns/advertisements committed while an ACK was pending. These are exact retained transaction3 counters, not transaction4's final pressure sample or a different boot's traffic.

The prior handoff trigger remains unchanged, including its staggered-arrival limitation. No retry is forced without a fitting buffer, no grant/credit ordering is bypassed, and no completion is invented. This image gathers the missing evidence needed to choose the next bounded change.

## Verification

Candidate and delivered-profile owner suites each pass575tests with one prior ignored test. Link suites pass223/222. A new two-clock-mode lifecycle test covers live reads, an exact blocked write, successful full-publication freezing, other reply activity, unrelated completion, retirement and reset. The actual pressure renderer covers all27fields, live/frozen/missing states, counter packing/saturation, duration units, clock absence and original-ACK identity on both framebuffers. Earlier panels and all handoff/credit/fault controls remain enabled.

The selected nested stack audit measures4,296,480bytes in the same5MiB mapping, leaving946,400bytes. Its scope excludes TLS, indirect calls and other descendants; it is not whole-program worst-case stack analysis. Final capability/SDK/packed-payload/source and delivery results are recorded after completion below.

## Manual cold temporary boot

Fully power off the Pixel, then hold Volume Down + Power for cold bootloader fastboot. After delivery verification run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-predecessor-pressure-v1/fastboot.img
```

Wait for `REGISTRY TX3 PRESSURE` and `RESTART WHEN DONE`, then send the whole settled panel. This is temporary boot with no partition flash. No device command was executed during preparation; WLAN outcome remains untested for this image.

## Verified delivery

Profile `native-registry-predecessor-pressure-v1` is ready for manual cold temporary boot. Image size68,915,200bytes; SHA256 `6e5743d5477b30e39da086bea37ee9c7ea2e8d4aa56df922bbb697fdd7e60f15`.

The final capability audit rejected304mutations. The unchanged pinned SDK reproduces the complete loader, CapDL and report byte-for-byte. The actual packed initializer archive has24,539objects and eight audited RW/NX/uncached Normal-NC maps; correct count passes while wrong/missing count controls fail. All3,637 named protocol frame physical bindings are unchanged from `native-registry-handoff-v1`. The actual paired-load validator still checks both words across4,096bytes with its null/alignment guards and no helper calls.

Twenty-four paired pressure schedules retain exact full I/O and peer-event order; the existing dedicated handoff, credit, close and fault tests also pass. Actual PD text, wrapped/LZ4 kernel, Android boot envelope, caller snapshots, source/input/artifact hashes and prior delivered image hashes are verified. `manifest.json` and `git-revision.txt` bind the final checkpoint. Self-review found no remaining important issue; no independent agent review was used. The new image has no handset result yet.
