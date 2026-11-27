# IMG_9797: exact EFS overlap admitted; both receipts precede close

The settled **EFS REPLY OVERLAP** panel corresponds to the delivered `native-efs-reply-overlap-v1` profile, revision `4646447a229424019db598df279cc752068d2fcf`, image SHA256 `3e02e56b725613244482d64f9701e5c5d5c334861ceef3e1238b6e4bd900d00b`. This is a panel/manual-command association, not an independently measured handset image hash. Raw HEIC, readable crops and decoded data are saved in that profile's separate `device-result`; original delivered artifacts and manifest remain unchanged.

## The bounded admission change worked

PAIR`0001017F` means one offer, one successful overlap admission and flags0x7f: enabled, first and second requests captured, byte-identical full RRQ/peer match, original transmit bound, client closed, second queued. This confirms full request equality using the authoritative Core ledger, rather than inferring equality from the earlier numeric option projection. The extra normal service handoff ran once and returned ResponseQueued. The inherited early writer committed grant ACK8 then GLINK DATA9, with no RX veto or TX backpressure; this command projection does not separately identify the DATA's reservation. GLINK DATA here carries a TFTP ERROR, not a firmware data block.

Both EFS entries are peer0x22, ERROR1/19 bytes on local port0x5003, with flags0xbf. The first is reservation0x13; the second0x14, using watched remote intent2. Both ordinary admissions succeeded on their first attempt with zero FlowBusy and zero NoCapacity. The second queued **633.963ms before the first matching completion**, consistent with the admitted overlap retaining the original live buffer. Ruleset/EFS port roles differ from IMG_9776; comparisons use exact paths and within-boot identities.

| Event | First EFS | Second EFS |
| --- | --- | --- |
| Accepted | `00DE3C76` | `00EC0123` |
| Dispatch completed | `00DFA055` | `00ED56A7` |
| Queued | `00DFE773` | `00ED9DC4` |
| Published | `00E9EF71` | `00F08235` |
| Matching RX_DONE processed | `00F74A2F` | `00F89B60` |
| Client close processed | `01032C05` | `01032C05` |

First acceptance-to-dispatch 91.103 ms and acceptance-to-publication 766.715 ms remain close to IMG_9776's 91.766/772.137 ms. Its publication-to-completion interval is 875.198 ms; completion precedes observed close by **778.710ms**.

The second request dispatches in 87.428 ms and queues 18.205 ms later. Compared with IMG_9776, dispatch-to-queue falls from **687.540ms to 18.205 ms**, acceptance-to-publication from **1,052.392ms to 295.186 ms**, and publication-to-completion from **7,478.713ms to 530.731 ms**. The second receipt now precedes client close by **692.389ms**, whereas it previously followed close by 7,103.323 ms. These are individual boot observations, not a guaranteed proprietary application deadline or a controlled isolation of every scheduling cost.

## Second-reply FIFO visibility is now captured

The watch is correctly bound to reservation0x14/remote intent2 and finished without history evictions:

- Arm`00F0823D`; peer-tail crossing`00F09A34`, **6.143ms after publication**.
- Last receipt-absent sample`00F1FBEC`; first-visible sample`00F24592`, an 18.854 ms bracket.
- First visibility is 109.406 ms after observed consumption; matching completion is processed 415.182 ms after visibility.
- RX is empty at watch arm; 152 bytes precede the matching receipt at first visibility. Their identities are not decoded by this panel.

The long second-reply completion gap is absent in this boot. Most of the remaining 530.731 ms publication-to-processed-completion interval is after RX visibility. That does not assign all 415.182 ms to exclusive CPU work: other records and runtime work may interleave. Tail crossing bounds shared-FIFO consumption, not remote application acceptance. Close-watch fields are absent because the watch had already finished; the actual transaction clocks establish completion before close.

## WLAN has not advanced; move the next comparison beyond this EFS gate

Original peer0x1f sent DATA; its final ACK phase4 completed at`00D82457`, before close`00D97CA5`. Runtime error/detail0, no pending close, cleanup stage6. IPA complete1, PD UNINIT, **WLAN RRQ0 / WLFW0**. Global read count14, kind mask0x2e, ruleset/EFS counts4/2, omissions2/0. Both early EFS replies completing before close was not sufficient to produce a WLAN firmware request in this run.

ELF body match1, host-credit mask7 and completed-name mask0x75 are present. ELF clocks are RX`014A93D7`, queue`0156A2EE`, publication`01A2E7F0`, completion`01A7A125`: 790.295 ms receive-to-queue, **4,998.402ms queue-to-publication**, and 309.557 ms publication-to-completion. The current panel does not identify what occupied that queue interval or establish that it caused the missing WLAN startup.

The useful next offline comparison is the startup path after the completed EFS pair, including this ELF wait, against the saved Linux sequence and existing locator/queue notes. Do not repeat the old EFS serialization probe as though its result remained unknown: exact admission, separate matching receipts and before-close completion are demonstrated here. Retain the narrow guard and ownership tests, and reassess only if a later boot actually regresses. No additional Pixel command is needed to interpret this result.
