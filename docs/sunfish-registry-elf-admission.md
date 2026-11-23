# Registry / ELF admission after repaint deferral

IMG_9706 confirmed the presentation improvement: transaction4 queue-to-first-writer fell from3,029.378ms to113.447ms and queue-to-publication from3,501.811ms to585.028ms. IPA completed, but WLAN RRQ/WLFW remained0. Transaction4 publication preceded ELF queueing by4,203.597ms. The panel omitted ELF arrival/selection and matching transaction3/4 receipts, so that separation cannot be assigned to a particular blocker or assumed to be time when ELF was already pending.

## Saved traces and offline checks

The saved IMG_9699 hardware trace already shows ELF queueing74.525ms before transaction4's matching RX_DONE. An unconditional requirement to wait for4 before admitting ELF is therefore ruled out. Source and actual existing replays retain the bounded same-peer window: one additional registry reply may queue while one earlier reply remains fully published and owned; a third simultaneous same-peer packet is rejected. A matching receipt retires only its original transmit. Publication alone is not retirement, and neither a close nor another buffer's receipt can free the older intent.

Fresh runs of the bounded-window, exact blocked-retry/closed-peer, receipt identity/clockless and real owner credit-limit cases pass. They show that returning one of two earlier replies can allow the next queue while the other is still owned, and that reference-flow credit can still block admission independently. The pinned Linux C credit oracle and10 Rust flow cases also pass. Model timing is not Pixel timing; these tests cannot reconstruct the missing events from IMG_9706.

One diagnostic limitation matters: `record_registry_pressure` iterates all active transmits and overwrites its peer fields for each matching destination. Consequently LAST PEER TXN is the last matching peer in that snapshot, not necessarily a unique blocker or the cause of every wait. Global slot counts are also not a count of matching registry peers. Do not retrospectively blame4 alone when both3 and4 may be owned.

## Next image: existing observations on one screen

Opt-in `sunfish-registry-elf-admission-panel` changes only the rendered panel to **REGISTRY ELF ADMISSION**. It retains the successful repaint deferral. No new driver state, capture hooks, hardware reads, timestamp calls, validation boundary or scheduling policy is introduced. Eight relevant source files and seven Runtime service/dispatch/paint function bodies are byte-identical to `e3f87fe8`; hashes and the comparison proof are retained privately.

The27 fields are:

- Runtime error; exact transaction3 publication/matching RX_DONE; transaction4 queue/publication/matching RX_DONE.
- ELF host receive, receive-pass end, first registry entry, first ELF selection, queue, publication and matching completion.
- WAIT PEER / WAIT TX FULL / WAIT CREDIT, the retained failed ELF admission counts for reasons1/2/4.
- LAST REASON, LAST PEER TXN, LAST PEER FLAGS and TX COUNTS from the last failed ELF admission, which remain historical after success.
- OLD RX ABS / VIS / AHEAD from the exact predecessor receipt's existing FIFO-visibility observation.
- PAINT DEFERRED, IPA completion, WLAN RRQ and WLFW servers.

ELF SELECT is recorded before request decoding and queue validation, not at the end of a first queue check. ELF ENTER is the first retained registry entry after that request exists; other pending registry requests can still be selected before ELF. RX times are host validated receive observations, not the modem's original emission time. Owner transaction4 queue/publication timestamps can be a few microseconds later than the Core observations used on earlier panels.

LAST REASON values:1 same-peer gate,2 all TX slots occupied,3 legacy flow-table capacity,4 reference-flow credit blocked,5 reference-flow table capacity. Missing pressure stays unknown; zero wait counts are meaningful only when the ELF trace exists. LAST PEER FLAGS bits0/1/2/3 mean bytes sent / fully published / GLINK done / per-transmit QRTR retirement condition. The last condition does not establish remaining destination credit. TX COUNTS packs four bytes low-to-high: all used slots, unpublished slots, published-awaiting-GLINK slots, GLINK-done-awaiting-QRTR slots. These are global counts. The three displayed wait counters do not enumerate reasons3/5; LAST REASON can still show them.

OLD RX fields refer to the predecessor's exact decoded matching receipt. ABS/VIS bound its appearance in validated FIFO snapshots and AHEAD counts bytes before it. Missing/lost-history fields remain unknown. They do not prove remote application acceptance or let unrelated receipts retire a reply. All absolute times use the runtime clock; `FFFFFFFF` denotes missing/unknown where applicable, or a saturated counter. ELF completion is a service-registry locator milestone, not WLAN firmware loading.

This view distinguishes late ELF arrival/selection from failed admission, and can place transaction3/4 receipts relative to successful queueing. It does not change retirement or raise the outstanding window on a guess. If no failed queue was observed, LAST fields remain unknown rather than inventing a blocker.

## Verification and delivery

Current/prior feature profiles each pass580 Owner tests with one existing ignored, and260 Core tests. Existing grant/credit/backpressure/fault controls, three prearm Core cases, three prearm Owner cases and presentation tests remain enabled. The actual27-field renderer is checked with packed slot bytes, absent ELF/pressure/peer transaction/clocks, both framebuffers and the physical15-character numeric-label limit. Prior panel renderers remain tested. Twenty-four paired Owner schedules retain full protocol/I/O digests and peer event order; these finite controls are not hardware deadline or all-interleaving proofs.

The actual native build, selected-stack audit, fresh CapDL authority with304 mutation rejections, compiled table-pair helper, Normal-NC initializer maps and whole loader/report/CapDL reproduction are verified before delivery. The SDK/tool is pinned reuse, not fresh compilation. All3,637 named protocol frame addresses match the preceding image. Selected stack remaining is917,120bytes of5MiB, excluding TLS, indirect calls and other descendants; this is not a whole-program worst-case proof. Review is self-review without an independent agent.

Private profile: `.cache/sunfish-native-wifi/private/native-registry-elf-admission-v1`. Image is68,923,392bytes, SHA256 `b0093080d03debb2aabe99a3fec782a9094f44f1f6181f1c22f933fa9f2e0ca9`. Final source/artifact/input hashes and preserved older images are bound by the delivery manifest. Prior handset evidence and manifests are not rewritten. No device command was run while preparing this image; this panel's handset outcome is untested.

## Manual cold temporary boot

Power completely off, then enter bootloader with Volume Down + Power. On the Mac:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-elf-admission-v1/fastboot.img
```

Wait for the settled **REGISTRY ELF ADMISSION / RESTART WHEN DONE** panel and photograph the whole screen. Routine repaint deferral remains enabled while the router is busy; errors remain immediately visible.
