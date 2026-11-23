# Bounded registry publication/admission handoff

IMG_9699 confirmed transaction4 publication before transaction3's matching RX_DONE, but still measured8,165.430ms from transaction4 receipt to queueing. Offline review found an unhelpful service boundary: the bounded registry dispatcher queues3 and immediately retries4 before any writer has published3. The runtime then runs storage and other service work before the outer writer continuation. The photo cannot establish how much of its8.165seconds that boundary consumed.

The new `sunfish-registry-handoff-turn` feature preserves the two-reply window and earlier original final-ACK turn. After the existing bounded registry dispatch actually queues a reply and its immediate second dispatch waits, an opaque one-shot candidate records those two actual request handles only if live private state identifies the same registry endpoint/peer and one unpublished predecessor. No display snapshot or timestamp gates scheduling.

Before the storage tail, the router can consume that candidate and perform exactly one ordinary checked owner dispatch. Pending other-owner work defers it. The normal writer can choose a grant advertisement, ACK, receive completion, credit, other packet or blocked outcome; it is never forced to send the registry DATA. The candidate is consumed before dispatch. There is no drain loop or repeated priority attempt for the same candidate.

A retry runs at most once, only if the exact predecessor now is fully published and still owns its matching slot/reference token, the waiting handle is still a live registry request, and ordinary validated selection still chooses that exact request. It invokes the normal service dispatcher and queue checks. Full transmit slots, reference-credit pressure, native validation faults, close handling and uncertain writes retain their existing results. The previous reply and its intent remain owned until their actual matching completions. A newly selected request is never bypassed. A later successful queue may create a new bounded candidate; every service pass still reaches the ordinary service tail rather than draining the registry indefinitely.

Firmware or TFTP client closes encountered during the nested router pass defer the retry and run the existing validated close handlers before the runtime proceeds to storage. Fatal non-TFTP client closes retain fail-closed behavior and quarantine; no receipt is invented.

## Capture

The `REGISTRY HANDOFF` panel shows27 bounded fields. Exact target metadata observes older transaction3/newer4 to peer0:9; `HANDOFFS` counts all valid consumed candidates. `HANDOFF MATCH` distinguishes a captured target from missing evidence. Predecessor queue/publication/completion are matched by opaque request identity even before a second reply is admitted. Writer start/end and retry-ready/start/end use the existing runtime clock. They are observations only, and missing timestamps remainFFFFFFFF.

RETRY READY means the exact predecessor/publication metadata allowed checking the retry; it is not a claim that capacity or QRTR credit admission already succeeded. RETRY RESULT0means not retried/deferred,1queued,2waiting on ordinary admission,3idle,4progress,5fatal. The result exists independently of the clock. Transaction4 and ELF receive/queue/publication/completion, publication-before-older-completion proof, original final ACK phase, IPA completion, WLAN RRQs, WLFW servers and completed-write count remain visible. ELF completion remains a locator reply, not a WLAN firmware transfer.

## Offline validation

Candidate owner suite:575passed/1priorignored. Previous image's profile:569passed/1priorignored. Candidate/prior link suites:222/221passed. The actual handoff tests cover14cases across six tests: idle/active storage, one-shot consumption, other-owner deferral, pending grant and valid receive backlog, cursor sampling failure, uncertain store, fatal status, wrong generation, advertisement-head failure, non-TFTP peer close, stale completed/queued pairs, a newly selected earlier queue slot, and real QRTR credit exhaustion/resumption. The last test fills the real destination count to10; the handoff returns WaitingReply/reason4 and only an actual RESUME permits normal admission. No old intent is returned merely to make room.

The new live-metadata test covers clock absent/present, wrong owner, route mismatch, wrong receive state, completed/retired predecessor and transport reset. Existing exact blocked-write and grant-order window tests, independent receipt identity, firmware-close faults, normal FIFO faults, full native4096-word corruption controls and all prior final-ACK tests remain enabled. Incoming control/backlog can legitimately defer the one-shot or leave it able to queue; the test checks actual outcomes and retained ownership rather than demanding one particular scheduler selection.

Twenty-four paired saved pressure schedules retain exact full I/O and peer-event order (48executions/96modes). These fixtures do not call the new runtime handoff boundary; the actual owner/router lifecycle tests above cover it. Fake peer timing does not predict handset milliseconds. The actual panel renderer and clock conversion were tested with all27fields, six outcomes, absent capture/clocks, original ACK identity and both framebuffers; prior panel renderers also pass.

The compiler inlined `init` into its exact `FnOnce::call_once` wrapper in this build. The stack audit was extended to select that actual implementation, verify the persistent entry's call, reject ambiguous/closure-only symbols, and accept an outlined wrapper only as a reviewed zero-frame two-instruction tail forwarder. Ten parser tests pass, and the previous image retains its original4,293,760-byte result. The new selected nested frame budget is4,295,536bytes in the same5MiB mapping, leaving947,344bytes. This is not whole-program worst-case stack analysis; TLS, indirect calls and other descendants remain outside its scope.

Linux/Android grant handlers and the pinned Linux QRTR credit model were rechecked during the preceding offline investigation; actual reference flow limits remain5/10. The focused pinned C oracle and ten Rust flow-model tests pass again. They do not supply missing native event timestamps or prove a WLAN launch trigger.

## Verification and cold temporary boot

The final image and capability/SDK/packed-payload checks are recorded below after completion. All firmware, private fixtures, images and source/input/artifact hashes remain under `.cache/sunfish-native-wifi/private/native-registry-handoff-v1`. Production changes are confined to the optional handoff, metadata observation, panel and audit support. No device command is executed during preparation.

After delivery verification, fully power off the Pixel and hold Volume Down + Power for cold bootloader fastboot. Run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-handoff-v1/fastboot.img
```

Wait for `REGISTRY HANDOFF` and `RESTART WHEN DONE`, then send the whole settled panel. Temporary boot does not flash a partition. The test should show whether the bounded retry actually ran, when3published relative to4admission, and whether ELF/WLAN progressed; it does not promise Wi-Fi startup.

## Verified delivery

Profile `native-registry-handoff-v1` is ready for manual cold temporary boot. Image size68,911,104bytes; SHA256 `b37b1f8961519668ed508b7d664cd78b1b484d104481f2823b65362653353d34`.

The final capability audit rejected304mutations. The unchanged pinned SDK reproduces the complete loader, CapDL and report byte-for-byte. The actual packed initializer archive has24,539objects and eight audited RW/NX/uncached Normal-NC table maps; correct-count audit passes while wrong/missing count controls fail. All3,637 named protocol frame bindings were compared by name;544 physical bindings moved with image growth, with authority and permissions checked separately. The actual AArch64 paired-load validator still checks both words over4,096bytes with null/alignment guards and no helper calls.

The actual PD text is in the loader, its wrapper/LZ4 kernel matches the Android boot envelope, and boot layout/input/source/artifact hashes pass. Caller source snapshots, frozen FIFO/native validation function bodies and earlier delivered image hashes are retained. `manifest.json` and `git-revision.txt` bind the final checkpoint and evidence. Self-review found no remaining important issue; no independent agent review was used. There is no handset result for this new profile yet.
