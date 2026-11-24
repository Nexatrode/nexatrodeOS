# Registry queue-to-first-arm I/O diagnostic

This follows the settled IMG_9705 **REGISTRY CREDIT TURN** result recorded in `sunfish-registry-credit-turn-IMG_9705.md`. The bounded credit write and later registry write ran on the handset. Queue-to-publication fell from 6.199 s to 3.498 s; 3.157 s of that remaining wait preceded the first conversion arming. That panel cannot say whether the remaining interval was the reply's own receive return, validation work, service work, or a later grant opportunity. IPA completed, while WLAN firmware RRQ and WLFW counters remained zero.

The next image preserves the successful converted-credit policy from `9280fa90`. It adds observations rather than another speculative scheduling change. No new diagnostic state, flags, counters, or clock values determine writer eligibility, ordering, credit use, or fault handling.

## Boundaries and screen

The panel is **REGISTRY PREARM IO**. Rows retain the original opaque owner/generation/request identity of registry transaction 4 paired with transaction 3, rather than relying on a reused transmit index. The first eligible 3/4 pair is retained until transport reset.

- **QUEUE OBS US** is the Adapter observation after its successful, normally gated queue operation, paired with a counter snapshot. It is slightly later than the Core's queue event.
- **ADAPT FIRST US** precedes the first Adapter writer's normal fresh FIFO sample. **CORE FIRST US** follows that sample. **FIRST END US** is after the first Core publisher result and before remaining Adapter postprocessing. A validation failure or early Adapter yield can leave Core/end evidence absent; it is not a completed callback.
- **OWN DONE US** records the successful local RX_DONE for the exact registry request after queueing. It is not the modem's receipt for the reply. If the request buffer was already returned at queueing, this row is absent and queue flag bit 0 is clear.
- **ARM OBS US** is the Adapter observation after the Core first arms a converted grant. Core prearm observations stop after first arming or full reply publication. The dedicated I/O window stops at first observed arming or transport reset; if no arming occurs, arm timing stays absent.
- **PASSES GATE** packs the saturating first 65,535 prearm Core entries in the upper half and the first entry's actual borrowed-grant eligibility gate in the lower half. A missing gate is `FFFF`. This gate is not a description of every queue check.
- **FIRST TXN CMD**, **FIRST PARAM**, **FIRST RESULT** retain the first selected GLINK record after queueing. If the first Core entry selected nothing, its first packet can be later. Transaction occupies the upper half (`FFFF` for a control record), command the lower half. Parameter is the GLINK record parameter, not necessarily a QRTR port. Outcome: 0 selected without callback, 1 checked success, 2 WouldBlock, 3 uncertain, 4 callback begun without a returned outcome. A later retry never overwrites this first record.
- **IO TO FIRST US** sums the four outer I/O categories from the queue snapshot to first Adapter entry. **IO TO ARM US** sums queue to first arming. **STATUS**, **CURSOR**, **READ**, **OTHER IO** show those four category deltas to arming. They are disjoint enclosing calls, including their full validation work; READ includes RX reads and TX readbacks. OTHER includes writes, cursor stores, barriers, and notification. Do not add a total to its component categories or add nested validation/domain timings again.
- The final rows retain ELF locator receive/queue/publication/completion, transaction 4 publication, IPA completion, WLAN RRQ, and WLFW server observations. ELF is the service-registry locator reply, not evidence that WLAN firmware was loaded.

`FFFFFFFF` means missing, invalid, saturated, or unavailable evidence where applicable. Optional clocks can perturb timings and include preemption. Host fixture operation counts are synthetic costs, not handset microseconds. The interval outside measured I/O cannot automatically be called idle time or assigned to a particular service.

## Flag and gate decoding

QUEUE FLAGS / FIRST FLAGS bits:

| Bit | Observation |
| --- | --- |
| 0 | Exact request's local RX_DONE is pending |
| 1 | Transmit has an owned or fitting remote GLINK intent |
| 2 / 3 | Pending intent ACK / positive ACK |
| 4 | Unadvertised grant backing exists |
| 5 | Ordinary class-1 QRTR credit is ready |
| 6 | Registry-turn cooldown is set |
| 7 / 8 | Registry borrowed attempt / credit borrowed attempt spent |
| 9 | Matching live transmit exists |
| 10 | Outbound intent request is waiting |
| 11 | Unsent same-flow predecessor exists |
| 12 | Matching receive exists with original owner/generation |

Gate values follow the existing first-failing conjunction: 0 no pending grant ACK; 1 negative ACK; 2 no backing; 3 no matching converted token/yield; 4 cooldown; 5 protected TFTP ready; 6 protected WRQ return; 7 own request return pending; 8 borrowed registry attempt spent; 9 ordinary registry candidate differs; 10 selected registry priority differs (which can include the ready credit); 11 conjunction admitted. None of these observations changes the conjunction.

## Timing scope review

The old ELF I/O counters are enabled during owner dispatch. Using them alone would miss service-side I/O outside that scope and could make a real wait look idle. The new native window has a separate bounded four-category accumulator. It enables optional start/end clock samples around the same existing native operations from exact queueing through arming, independent of the old owner enable flag. Old owner/detail/domain counters retain their prior scope, including nested barrier-domain clock sampling. Counter snapshots and diagnostic getters are CPU-only; there are no extra MMIO or cursor reads. Missing clocks, counter overflow/saturation or regression remain unknown.

Existing Ring read/peek/consume/write checks, fault quarantine, full SMMU validation boundaries, advertisement-before-ACK ordering, protected WRQ/TFTP turns and QRTR credit selection remain intact.

## Verification and artifact

Private profile: `.cache/sunfish-native-wifi/private/native-registry-prearm-io-v1`. The source and artifact manifest is generated after all checks, and device evidence will be stored separately without rewriting this delivery manifest.

The native image is 68,927,488 bytes, SHA256 `4fcc8d8ebf1d3ae72923d8bc0acb0e530c9c05c4d51fb62215e0c0ec3103a0da`.

Offline verification includes 580 passing current Owner tests (one existing ignored), 577 passing prior-profile Owner tests (same ignored), 260 current and 257 prior-profile Core tests, the six retained credit cases, and six new prearm tests across Core, Adapter and timing counters. New cases cover clockless latching, exact local return, first selection/outcome retention, WouldBlock/uncertainty/unwind, transport reset, validation failure, getter no-I/O, timing independent of owner scope, and invalid/saturated deltas. The actual 27-field renderer is exercised with missing clocks/evidence and both framebuffers; each numeric label must fit the physical 15-character limit. Twenty-four paired Owner replay schedules preserve complete protocol/I/O digests and peer event order; they are finite controls, not a timing or all-interleavings proof.

Native stack, actual packed initializer/Normal-NC maps, generated physical mappings, capability mutation controls, pinned SDK reuse and loader/payload reproduction are audited on this image. The selected stack path has 917,168 bytes remaining in the 5 MiB mapping; it excludes TLS, indirect calls and other descendants and is not a whole-program worst-case proof. The additional resident frame moves 544 generated data-pool frame addresses; all 3,637 named protocol frames are compared and table/MMIO bindings stay pinned. Source, binary, private inputs and preserved older images are hash-bound by the final manifest. Review is self-review; no independent agent was used.

No device command was run while preparing the image. A handset outcome is still required. This diagnostic does not claim working Wi-Fi or a known number of remaining boots.

## Manual cold temporary boot

Power the Pixel fully off, then enter bootloader with Volume Down + Power. On the Mac:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-prearm-io-v1/fastboot.img
```

Wait for the settled **REGISTRY PREARM IO** panel / **RESTART WHEN DONE** and photograph the whole screen. This uses temporary `fastboot boot`.
