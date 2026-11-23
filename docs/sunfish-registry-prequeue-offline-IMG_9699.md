# Offline review of the IMG_9699 registry prequeue interval

The8,165.430ms receive-to-queue interval for transaction4 is real, but REGISTRY WINDOW TWO does not record transaction3's full-publication time or the first service admission attempt after it. Saved counters from other boots cannot supply those timestamps for this boot. The successful1,000.586ms publication-before-older-completion overlap already rules out a requirement to wait for transaction3 RX_DONE on the successful second admission.

## Current source boundary

`RuntimeDomain::service_pass` calls `Owner::dispatch_service_registry_bounded` before the storage, ordinary IPA and WLAN service tail. The bounded dispatcher calls the normal registry step, then can call it once more immediately after successful queueing while the exact ELF request remains pending. There is no writer between these two calls.

When the first call queues transaction3, the second call can select transaction4 but must fail the same-peer gate: transaction3 is not fully published yet. The new window deliberately requires full publication, a real retained remote intent and reference-flow token, a local QRTR retirement condition and no second same-peer outstanding packet. The runtime then proceeds through storage and the rest of the service pass before its outer router continuation runs again. TFTP-specific nested router calls are separate paths and cannot be assumed to occur here.

After transaction3 fully publishes, the first otherwise-eligible ordinary registry dispatch can queue transaction4 immediately. The ordinary service path still performs its existing identity, native revalidation, request decoding, queue and credit checks. This is a concrete scheduling opportunity; it is not a measured attribution of all8.165seconds to storage. The storage burst itself is bounded to two steps, not an unbounded storage drain.

## Replay evidence

Private `registry-prequeue-offline-IMG_9699-v1` rebuilds the saved owner/router/RMTFS composition and GLINK writer pressure fixture against the current production sources and exact native feature set. Three focused tests pass across seven modeled cases:

- Four cases compare late older publication with early publication followed by later service entry, with idle or active storage. They produce identical captured new-queue/new-publication/old-completion timestamps and overlap proof. The current panel therefore cannot distinguish these histories. Model ticks are intentional event schedules, not handset latency estimates.
- Two owner/router cases insert exactly one ordinary checked writer dispatch before the storage tail. That turn publishes transaction3 in these cases; the next normal registry step queues4 before storage starts, without an older RX_DONE, changed older packet or reused busy intent. This identifies an implementable boundary rather than assuming a service dispatch transmits by itself.
- One actual GLINK pressure replay preserves backing advertisement, positive grant ACK, request3 RX_DONE, QRTR RESUME and reply3 before admitting reply4. Request4 RX_DONE then legitimately precedes reply4. Every selected write's blocked attempt retries identical bytes; the two replies use distinct intents and reverse completion does not free the older reply. Returning the QRTR-credit packet's intent does not complete transaction3.

An initial fixture assertion expected reply4 on the very next writer call and failed because the writer correctly returned request4 first. That expectation was corrected and the failed log retained. It is evidence against forcing the next DATA write or skipping receive completion, not a production failure.

## Reference comparison and limits

The pinned Linux and Android GLINK intent-request handlers were re-read. Linux's allocation path advertises a newly allocated receive intent before its positive ACK. Android can ACK an already-advertised reusable intent; its fresh-allocation path also advertises before ACK. The native replay keeps its fresh-grant ordering. The pinned Linux QRTR implementation uses low/high destination credit watermarks5/10, rather than requiring a per-reply QRTR resume. The current reference-flow token/counter checks remain necessary; the two-reply window does not replace them.

Linux's saved fast matching RX_DONE intervals do not establish the timing of native transaction3 publication or service dispatch in IMG_9699. Existing native publication pressure structures contain counters/masks, not an absolute older-publication timestamp; older photographs differ in feature set and packet availability. No web source can reconstruct an event that was not captured from this boot.

## Next concrete change to evaluate

A bounded checked registry handoff before the service tail can give the just-queued predecessor a normal writer turn, then retry ordinary second admission only if the exact predecessor has actually fully published. A grant, RX_DONE, credit, backpressure, other owner or fatal condition must retain the ordinary outcome and fallback; this must not become a drain loop, forced DATA selection or premature retirement. A subsequent image should capture predecessor publication and the first eligible retry so its benefit can be verified separately from total ELF time.

This offline review changes no production policy and builds no new image. No Pixel command is needed yet. Replay source/feature/compiler-command hashes, the original failed assertion, passing output, source/reference hashes and result metadata are retained privately. The previous delivered image and manifest remain unchanged.
