# IMG_9818 — all three bounded TX3 stages committed; no ELF full pool

Settled **ELF POOL RX TAIL**, associated with preceding manual boot instruction for source `ad3b49a780f8b03721915c334c86d2eea26ce638`, image SHA256 `372438d4d30cdfcf373426787541150a6a0fee4c38b7fcaceeebc6b774a41d06`. Association is by instruction and panel, not an independently read handset hash. Original HEIC, readable JPEG/crops, all displayed numeric rows and32 header projections, executable decoder and hashes are archived separately in the delivered profile's `device-result/`. All128 delivered artifacts were reverified unchanged. No device command or new build was performed for this analysis.

## The complete credit/own-return/reply opportunity ran

TX3 has5 calls, mask **007E23AC**. Credit attempt/commit17/18, own receive-return attempt/commit21/22, and ready reply attempt/commit19/20 are all set. The converted-grant reply bits11/12 are absent. This is physical evidence for the new three-stage path, unlike IMG_9815/9817's corrected3FA4 mask. The optional renderer introduces no scheduling change; the inherited credit-return implementation is the behavior tested here.

Own-return first failures0..7 are `0,2,1,1,0,0,0,1`, summing5: two protected/priority/cooldown/conversion exclusions, one no-positive-ACK, one no-fresh-backing and one candidate. There is no ready-credit veto at gate6. Candidate count and callback bits are separate evidence. The mask confirms actual successful callbacks, not merely an arm or eligible snapshot. Existing checked scheduling/replay establishes credit→own return→reply order; the wire trace arms at reply publication and does not independently timestamp the preceding two stages.

Queue→publication is **383.080ms**, compared with262.627ms in IMG_9815/9817 and12,666.803ms in IMG_9813. This confirms the new path was exercised, not a monotonic performance improvement over the immediately preceding fast converted path. The opening wire response is104 total bytes/88 QRTR bytes, local port4003→peer9, QMI response transaction3/message21, remote intent2. Its timestamp precedes the full-publication observation by4us at their different hook sites. The next displayed host advertisement and positive ACK occur54.938ms and143.480ms after publication. The compact advertisement header shows a count, not backing ID/size; do not reconstruct an undisplayed backing from it. First converted-grant fields are unavailable, consistent with the absence of that callback path.

## No pool failure to decode in this boot

POOL failures0, absent first/latest/free/admission snapshots and ELF waits `0,0,0,0,0,0` show no recorded exact ELF transmit-capacity rejection. Thus the unavailable slot/free fields do not mean eight empty slots or a failed capture. The fallback correctly shows current own-return/priority/grant counters instead. IMG_9815/9817's four capacity failures and missing eight owner identities cannot be copied into this boot.

ELF receive→queue **935.712ms**, queue→publication204.557ms, publication→Owner2,172.070ms, total **3,312.339ms**. That is63.001ms longer than IMG_9815/9817's3,249.338ms despite the smaller prequeue interval. All-zero admission wait counters do not partition or explain the935.712ms before queueing; service/preparation time and opportunities to attempt admission are distinct from failed attempts. TX4 queue→publication321.898ms and publication→Owner3,160.277ms.

## Prompt consumption, delayed matching receipts and host processing

| Interval | TX3 | ELF |
| --- | ---: | ---: |
| Publication → observed peer-tail crossing | 6.117ms | 6.078ms |
| Publication → exact receipt first visible sample | 3,892.378ms | 1,329.649ms |
| First visible sample → exact Core completion | 478.361ms | 824.127ms |
| Publication → exact Core completion | 4,370.739ms | 2,153.776ms |

Both retained visibility bounds use last-absent01608036/first-visible0160C9E8, an18.866ms bracket. They are observations, not exact firmware event clocks. Reported bytes ahead are48 for TX3 and168 for ELF; history evictions2/0 respectively. Counts belong to each exact receipt's retained first-visible sample and do not alone identify preceding records. ELF Owner follows its Core completion by18.294ms. Peer consumption does not establish application acceptance or release GLINK ownership.

## What the retained wire tail identifies

79 ordinary headers retained,2 producer omissions,0 unparsed; the page skips48 additional retained headers and prints opening4/latest27 plus exact matching receipt. It is not a complete traffic log.

The visible late startup sequence contains:

- Two85-byte RRQs from peer48 to5000 with raw category3, which the pinned parser maps to exact `/readwrite/ota_firewall/ruleset`. A19-byte ERROR1 reply is committed from5004→48 at0156399D,554.863ms after the first displayed read. DEL_CLIENT names node0/port48 at0158525D,137.408ms after that reply. This is a missing-file reply, not an OACK or firmware DATA.
- Two73-byte RRQs from peer49 to5000 at015BD5B0/01611CEA,345.914ms apart. Category2 maps to exact `/readwrite/mcfg.tmp`. Same category/length/ports do not prove equality of undisplayed option bytes. No peer49 response or close appears in the displayed tail before the TX3 capture freezes803.031ms after the first read. That does not prove no later response or close occurred, or no earlier omitted reply.
- Fresh intent requests of120,52 and108 bytes interleaved with host receive returns, advertisements and positive ACKs. These operations are retained normal protocol work; the log does not justify bypassing their ordering or checks.
- Accepted reusable RX_DONE records for intent0 and1 before the separately matched original TX3 intent2 receipt at01681687. Numeric IDs0/1 alone do not identify those owners as TX4 or ELF: IDs can be reused and no matching opaque transaction witness is displayed for those two records. The exact TX3 Core observation precedes its header timestamp by6us.

Headers show accepted/committed times, not first FIFO arrival positions. They must not be summed indiscriminately across producer/presentation omissions to label the entire48/168-byte prefix. The trace freezes at exact TX3 completion and does not cover the later ELF matching receipt. Current evidence supports offline correlation of MCFG service/response timing and the validated receive path with existing notes/replays; it does not establish a missing GLINK message or the modem's internal WLAN prerequisite.

## Startup result

Runtime error/detail0, no pending MCFG close, cleanup stage6. Original final ACK00D8413A is present. Historical IPA complete1, live pending IPA message unavailable/none and exact ELF host-credit mask7. **WLAN firmware RRQ0 / WLFW servers0** remain unchanged. The full bounded handoff is now demonstrated on the Pixel, but it has not produced a new WLAN milestone. No extra Pixel command or rerun is needed to interpret this photograph; published buffers still require their real matching completions.
