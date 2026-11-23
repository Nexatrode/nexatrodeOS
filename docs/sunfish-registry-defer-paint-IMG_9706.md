# IMG_9706: repaint deferral removes the pre-writer stall

The settled **REGISTRY DEFER PAINT** panel is from source `e3f87fe8`, image SHA256 `2db3517ae7e3e233d62fdbe41393a76bc960c1046fa04660599109dea40185b7`. Runtime error is zero. Original HEIC, PNG preview, all 27 decoded fields and arithmetic are stored privately in `native-registry-defer-paint-v1/device-result`. The delivery manifest is preserved unchanged.

## Measured improvement

| Interval | Previous 712d88d4 | IMG_9706 |
| --- | ---: | ---: |
| Queue observation → first Adapter writer entry | 3,029.378 ms | **113.447 ms** |
| Measured transport I/O before first entry | 119.735 ms | 113.361 ms |
| Elapsed time outside those I/O calls | 2,909.643 ms | **0.086 ms** |
| Queue observation → arming observation | 3,159.655 ms | **243.437 ms** |
| Arming observation → transaction 4 publication | 342.156 ms | 341.591 ms |
| Queue observation → transaction 4 publication | 3,501.811 ms | **585.028 ms** |

Queue-to-publication is 2,916.783 ms shorter, an 83.29% reduction, about 5.99× faster in this comparison. Almost all of the improvement is before first writer entry; post-arming publication time is essentially unchanged.

PAINT DEFERRED is 8 across the runtime. PAINT GAP is zero in the completed queue-to-first-writer window. The formerly approximately 2.910 seconds outside profiled I/O has essentially disappeared. Together with the presentation-only source change and unchanged captured first-turn metadata, this strongly supports debug redraw contention as the removed source of delay. Separate boots do not establish an exact exclusive CPU cost or all-interleavings guarantee, and this boot's zero rendering time cannot retrospectively measure each redraw in the previous boot. The count is not necessarily eight full redraws inside this specific window.

Remaining prearm time is almost entirely inside the retained checks and transport calls: queue-to-arm elapsed is 243.437 ms versus 243.246 ms measured I/O. Categories are status53.744 / cursor53.816 / read16.710 / other118.975 ms. The one-microsecond difference between their individually rounded sum and directly converted combined total is expected.

## Same captured first turn

Queue and first flags remain `0x122E`, PASSES GATE remains `00020002`, and the first selected packet remains `FFFF0008`, parameter1, result1. Thus the live reply/fitting intent exists, its own local RX_DONE was already complete at queueing, and the first checked writer successfully sends the pending positive GLINK grant ACK. The missing OWN DONE timestamp is expected because that exact local return preceded queueing. Gate2 means no unadvertised backing, not a missing fitting reply buffer or a failed ACK. The panel does not identify that ACK's originating grant request size. Two Core entries occur before arming, as before.

The new ordinary redraw gate therefore produced a concrete handset improvement while retaining the captured grant/credit path. This is a performance milestone; transaction 4 publication is not its matching modem RX_DONE or application acceptance.

## ELF and WLAN outcome

ELF QUEUE is `019F12FC` (27.202300s on the runtime clock), DONE is `01ACBB83` (28.097411s), and transaction 4 publication is `015EEEAF` (22.998703s). ELF completion is 3.101790s earlier on that clock than in 712d88d4, with the usual separate-boot/start-boundary caveat.

Transaction 4 publication precedes ELF queueing by **4,203.597 ms**. ELF queue → completion is **895.111 ms**. The preceding panel had 3,460.242ms publication-to-ELF-queue separation; faster transaction 4 publication does not imply that every later interval improved.

IPA COMPLETE is1. WLAN firmware RRQ and WLFW server counters remain0. No first-client TFTP DATA or firmware-transfer timeline is displayed here. ELF completion remains the service-registry locator milestone, not loaded WLAN firmware or working Wi-Fi.

## Next offline target

Review what occurred between transaction 4 publication and ELF admission, retaining the repaint improvement. The current panel replaces ELF receive/publication with paint rows and has no matching transaction3/4 remote receipt timestamps or ELF first-admission result. The 4.204s separation alone therefore cannot establish that ELF was already pending throughout it, which request blocked it, or whether ingress, service scheduling, full-slot pressure or QRTR credit/retirement was responsible.

Source review confirms that the normal same-peer registry window permits one additional reply only when exactly one predecessor is fully published with its retained intent/reference-flow state and QRTR retirement condition. A third same-peer outstanding packet fails FlowBusy; global transmit exhaustion separately fails NoCapacity. Publication alone does not release the predecessor. Before changing scheduling or retirement, compare the saved completion/admission traces and identify the exact pending request, matching RX_DONE/credit state, and first normal ELF queue attempt. Keep FIFO, grant ordering and credit checks unchanged.

No additional Pixel command is needed to interpret this result or start that offline comparison. No device command was executed while recording it.
