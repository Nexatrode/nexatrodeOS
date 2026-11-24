# Exact transaction3 peer consumption and receipt timing

IMG_9707 is recorded in `sunfish-registry-elf-IMG_9707.md`. ELF arrived and was selected early, then encountered9same-peer admission failures with0full-slot/reference-credit failures. It queued203.120ms after transaction3's matching RX_DONE and74.592ms before transaction4's matching RX_DONE. The repaint improvement remained: transaction4 queue-to-publication583.460ms. IPA completed, while WLAN RRQ/WLFW stayed0.

The dominant unresolved observation is transaction3 publication→matching receipt processing8,061.110ms. Its receipt was first visible7,417.253ms after publication, with96bytes ahead, then processed643.857ms later. The panel omitted the already retained predecessor peer-tail observation. Without it we cannot distinguish delayed consumption of the original reply from delayed RX_DONE after consumption, or assign all8.061s to a particular service. Scalar history cannot identify those96bytes.

## Next panel

`sunfish-registry-tx3-receipt-panel` selects **REGISTRY TX3 RECEIPT**. It reuses existing exact predecessor queue/publication, peer-tail, matching receipt visibility/processing, history-loss and converted-grant observations. It also retains transaction4 publication/completion, ELF receive/selection/queue/publication/completion, peer-wait count, IPA and WLAN/WLFW status. No new protocol state, trace hook, hardware read, timestamp call, grant/credit rule or dispatch policy is added. The successful redraw deferral remains enabled. The same eight relevant source files and seven Runtime service/paint/dispatch bodies remain unchanged from552ad335; proof hashes are retained privately.

TX3 TAIL records the validated shared-FIFO peer tail crossing the tracked publication. This means transport consumption, not acceptance by the remote QMI service. It is an observation at a host sample, not the modem's exact read instant. Comparing publication→TAIL→receipt visibility→processing divides the large interval without guessing that publication retired the buffer. Current same-peer admission still requires the original completion/flow conditions.

TX3 RX ABS/VIS bound the exact matching receipt's FIFO appearance. RX AHEAD is preceding byte count, not packet identity. RX MATCHED is the clock-independent finished flag of the exact predecessor FIFO observation. RX HISTORY LOST counts evicted RX-advance history entries; unknown individual bounds stay unknown. History loss is not a transport fault or permission to skip records. TX3 BYTES comes from the predecessor's retained publication snapshot and may be absent until captured.

GRANT CONV/AD/ACK/INTENT show the first converted-grant opportunity bound to that same original request. They do not enumerate every ordinary or later grant. Missing fields do not establish that no grant occurred. If retained, compare their actual ordering to peer-tail and receipt timing; do not assume a particular requested size or that the grant caused the delay.

All timestamps share the runtime clock. `FFFFFFFF` means unavailable/unknown where applicable, or saturated counters. ELF SELECT is before decoding/queue checks. Queue/publication is not a matching remote completion. ELF completion is the service-registry locator milestone, not WLAN firmware loading. The next handset result is required before a new timing or WLAN outcome is claimed.

## Verification and artifact

Current and previous feature profiles each pass580 Owner tests with one existing ignored, and260 Core tests. The actual27-field renderer is tested for exact predecessor/grant labels, absent grant/publication/clocks, unfinished and evicted receipt history, both framebuffers and the physical15-character numeric-label limit. Previous panel renderers and the retained buffer identity, blocked retry, grant ordering, credit and fault tests remain checked. Twenty-four paired Owner schedules preserve complete protocol/I/O digests and peer order; finite host controls do not establish handset deadlines or all interleavings.

Native build, selected-stack, CapDL304mutation rejection, Normal-NC packed initializer maps, actual table-pair machine code and whole loader/report/CapDL reproduction are verified before delivery. SDK/tool bytes are pinned reuse, not fresh compilation. All3,637 named protocol frame addresses match the prior image. Selected stack remaining is917,120bytes of5MiB, excluding TLS, indirect calls and other descendants; this is not a whole-program worst-case proof. Review is self-review without an independent agent.

Private profile: `.cache/sunfish-native-wifi/private/native-registry-tx3-receipt-v1`. Image is68,923,392bytes, SHA256 `d4a5edf04ce390835ed06ab22243e80dd3a60d5e705d5e6f333be7a212832935`. The delivery manifest binds source/artifact/input hashes and preserved older images. Device results will be saved separately. No device command was executed while preparing it.

## Manual cold temporary boot

Power fully off, then enter bootloader with Volume Down + Power. Run on the Mac:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-tx3-receipt-v1/fastboot.img
```

Wait for **REGISTRY TX3 RECEIPT / RESTART WHEN DONE** and photograph the whole panel. Repaint deferral can pause ordinary display updates while the router is busy; errors remain immediately visible.
