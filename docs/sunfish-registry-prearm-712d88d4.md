# Settled REGISTRY PREARM IO result: delay precedes the writer

The photo `712d88d4` is the settled panel from source `fe3ba2b3`, image SHA256 `4fcc8d8ebf1d3ae72923d8bc0acb0e530c9c05c4d51fb62215e0c0ec3103a0da`. The original PNG, all 27 decoded fields, arithmetic and interpretation limits are saved privately in `native-registry-prearm-io-v1/device-result`. The delivery manifest is preserved unchanged. Runtime error is zero.

## Main finding

Transaction 4 waited **3,029.378 ms from the post-queue observation to first Adapter writer entry**. Only **119.735 ms** of that interval was inside the newly scoped native transport I/O calls. Approximately **2,909.643 ms** therefore lies outside those enclosing calls. This narrows the investigation to the queue-to-runtime/dispatch path, service work, rendering, or other unprofiled elapsed work; it does not assign the time to any particular cause or establish that it was idle time.

From first Adapter entry to arming took another **130.277 ms**, with **130.119 ms** inside the profiled I/O calls. Almost all of the unassigned interval is consequently before the first writer entry, rather than in that writer's fresh FIFO sample or checked ACK write.

| Interval | Elapsed |
| --- | ---: |
| Queue observation → first Adapter entry | 3,029.378 ms |
| First Adapter entry → first Core entry | 14.372 ms |
| First Core entry → first Core result/end observation | 28.667 ms |
| First end observation → arming observation | 87.238 ms |
| Queue observation → arming observation | 3,159.655 ms |
| Arming observation → transaction 4 publication | 342.156 ms |
| Queue observation → transaction 4 publication | 3,501.811 ms |

The queue/publication wait roughly reproduces IMG_9705's 3,497.619 ms. The 4.192 ms difference is not a controlled performance comparison: this panel adds instrumentation and uses Adapter queue/arm observations slightly later than the previous panel's Core events. This was an observation change, not another scheduling fix.

## Buffer and first-turn evidence

QUEUE FLAGS and FIRST FLAGS are both `0x122E`, bits 1,2,3,5,9,12. The exact live reply and its receive identity exist, a fitting/owned remote GLINK intent exists, a positive grant ACK is pending, and ordinary class-1 QRTR credit is ready. Bit 0 is clear, so the registry request's own local RX_DONE was already completed at queueing. OWN DONE remains `FFFFFFFF` because the new trace records that return only if it occurs after queueing; this is expected missing evidence, not proof that a return was lost.

PASSES GATE `00020002` means two prearm Core entries and first borrowed-grant eligibility gate 2: no unadvertised backing remained. This does not mean no fitting reply buffer. Source clears `intent_ack_backing` after successful matching advertisement publication while leaving the positive ACK pending.

FIRST TXN CMD `FFFF0008`, parameter 1, result 1 records a successfully checked positive GLINK intent-request ACK. The first writer therefore took the already ordered grant ACK rather than publishing transaction 4. The ACK's originating request size/identity is not on this panel and must not be assumed to be the later converted grant. The second prearm entry reached conversion arming; the first-only record does not identify every later packet.

Queue-to-arm outer I/O totals were status **55.806 ms**, cursor **55.824 ms**, read/readback **16.746 ms**, and other I/O **121.476 ms**. Their individually rounded sum is 249.852 ms versus the directly converted combined total 249.854 ms. The 2 µs difference is within the three-microsecond bound from separately rounding four tick totals. Do not add the total and its components again.

## ELF and WLAN

ELF locator receive → queue took **7,477.177 ms**; queue → publication **205.166 ms**; publication → matching completion **629.796 ms**. Total receive → completion was **8,312.139 ms**. ELF queued **3,460.242 ms after transaction 4 publication**. The panel has no transaction 4 remote receipt timestamp, so it cannot establish whether that receipt blocked ELF queueing or compare ELF speed with the preceding panel that omitted these fields.

IPA COMPLETE is 1. WLAN firmware RRQ and WLFW server counters remain zero. This panel does not display a first-client TFTP DATA timeline; it cannot establish that client's outcome. ELF completion is a service-registry locator milestone, not firmware loading or working Wi-Fi.

## Offline direction before another handset run

Source review confirms that `Owner::queue_reply_inner` queues the reply and schedules a continuation; `Router::schedule` sets a flag and does not synchronously call the writer. `RuntimeDomain::service_pass` can continue registry, storage, IPA and WLAN work before returning to ordinary owner dispatch, and `service_owner` has progress/rendering work at bounded-slice boundaries. These are concrete paths to review, not measured assignments of the 2.910 seconds. Router liveness itself checks software error/busy state rather than performing a native transport I/O validation.

Next reconstruct this exact post-queue path through continuation flags, remaining service work and display/flush boundaries to the first Adapter entry, using existing runtime phase evidence and saved source/notes before adding another handset probe. Also retain the separate ELF prequeue interval and distinguish its admission dependencies from dispatch scheduling. Keep FIFO validation, advertisement-before-ACK ordering and QRTR credits unchanged; this photo does not justify weakening them.

No additional Pixel command is needed to interpret this result or begin that offline review. No device command was executed while recording it.
