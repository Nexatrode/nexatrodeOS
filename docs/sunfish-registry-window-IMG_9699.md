# IMG_9699: registry overlap works; WLAN remains absent

The settled REGISTRY WINDOW TWO photo matches the profile built from `ff4edd21`, image SHA256 `877cc9ec1fcc1850cd18d940b9f00806776ecc8b3492e5075379c28b0fea2c2d`. Original photo, preview, complete27-field transcription and arithmetic are saved in private `native-registry-two-replies-v1/device-result`. The historical delivery manifest and frozen source snapshot are unchanged.

Runtime error is zero. WINDOW ADMITS is2. The exact captured pair is older transaction3/newer4 to peer0:9. PUB BEFORE DONE is1, the clock-independent proof that transaction4 fully published before transaction3's validated matching RX_DONE. The timestamps agree: new publication01C995CB, older completion01D8DA55, a1,000.586ms overlap. This confirms the intended bounded concurrency on hardware while retaining independent completion identities.

| Observation | Hex microseconds |
|---|---|
| Exact pair queue | 01C1FB0C |
| Exact pair newer publication | 01C995CB |
| Exact pair older completion | 01D8DA55 |
| Transaction4 receive | 014562D7 |
| Transaction4 queue | 01C1FB0D |
| Transaction4 publication | 01C995D3 |
| Transaction4 completion | 01DCE60F |
| ELF receive | 015004D8 |
| ELF queue | 01DBC2F2 |
| ELF publication | 01DEDFDA |
| ELF completion | 0218DBCF |
| Original final ACK publication | 00D65778 |
| Original final ACK completion | 00D6BBB9 |
| Exact peer0:1f close first visible | 00D66F38 |

The core publication observation precedes the owner's transaction4 publication observation by8us; its queue observation precedes the owner's queue observation by1us. They are separate observations of the same successful events, not a clock mismatch.

Transaction4 receive-to-queue is8,165.430ms; queue-to-publication498.374ms; publication-to-completion1,265.724ms. The new panel does not capture transaction3 publication or when its fully-published eligibility condition first became true. It cannot divide the8.165seconds between an unpublished predecessor, service-loop delay and other queue guards. It would be incorrect to attribute that entire interval to waiting for transaction3's RX_DONE: the hardware proof establishes admission/publication before that receipt.

ELF receive-to-queue is9,158.170ms, queue-to-publication204.008ms, publication-to-completion3,800.053ms; total13,162.231ms. ELF queues74.525ms before transaction4 completes, consistent with the second successful admission count. IMG_9697 had8,403.901ms prequeue and9,234.287ms total. These separate boots do not demonstrate an end-to-end speedup; this run is slower overall despite the specific overlap succeeding.

The original final ACK still completes: ACK TURN1, ACK PHASE4, one write complete. Publication precedes exact close visibility by6.080ms; matching completion follows it by19.585ms (25.665ms after publication). Close visibility is a host FIFO observation and does not establish the remote close decision time or application acceptance.

IPA COMPLETE is1. WLAN RRQ RX and WLFW SERVERS remain0. No WLAN firmware request or WLFW startup is demonstrated by this test. ELF completion is still only a service-registry locator response.

Next offline target: the prequeue path for transaction4, especially transaction3's publication/eligibility and the ordinary service-loop turns. Inspect existing publication/pressure histories and saved Linux/native traces before deciding whether another capture is needed. The existing owner path selects one pending registry request and applies normal checked reply admission; the bounded extra turn only follows successful queueing. Neither the current photo nor open-source protocol rules alone establish which condition consumed the remaining prequeue interval.

No additional Pixel command is needed to interpret this result. No device command was executed while recording it.
