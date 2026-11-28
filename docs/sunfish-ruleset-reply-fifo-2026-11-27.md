# November27 RULESET REPLY FIFO: prompt consumption, late matching receipts

The two settled desktop photos named `Photo on 11-27-26 at 11.23 PM.jpg` and its `#2` counterpart are complementary views of one outcome. The association is with delivered source `e33975ea1d634e88a5bfe106efc046563fecf7d0`, image SHA256 `f34f070063727df1a5719de65091e5d64e65ecd9bd6e77661a7ffa9deb3a2ef7`, by the delivered command and panel title. It is not an independently measured handset image hash. Raw originals, transcription, executable arithmetic, comparison and evidence hashes are saved separately in `native-ruleset-reply-fifo-v1/device-result/`. All161 original delivery artifacts were reverified unchanged. No device command or new image build was performed for this analysis.

## The new boundary is resolved

Both original watched ERROR1 replies were consumed promptly from shared TX. Their exact matching RX_DONE records were absent at processed client close and arrived in a much later shared-RX head advance. Therefore most of their publication-to-completion gap precedes receipt visibility; it is neither a multi-second wait for the modem's TX read cursor nor several seconds of host drain after a visible receipt.

| Original reply | Publication → observed TX-tail crossing | Tail → last receipt-absent sample | Tail → first receipt-visible sample | First visibility → exact Core completion |
| --- | ---: | ---: | ---: | ---: |
| Ruleset2, peer21/reservation15/intent1 | 6.157ms | **7,192.804ms** | 7,211.804ms | 306.877ms |
| Ruleset3, peer23/reservation17/intent3 | 6.152ms | **6,086.300ms** | 6,105.300ms | 567.683ms |

Both receipts map to the same retained head-advance bracket: last absent `016DDED2`, first visible `016E290A`, a19.000ms bracket. They have80 and96 bytes ahead respectively. These offsets do not identify the preceding or intervening records. Both watches finish, TX bytes remaining0, exact original flagsBF, visible-at-processed-close0. History evictions25/18 do not invalidate the retained numeric span for these exact receipts. The close field is the host's validated processing of that peer close, not its first arrival in the FIFO.

Ruleset2: RX `00F56673`, dispatch `00F64044`, queue `00FB461D`, publication `010005E1`, arm `010005EC`, observed tail `01001DEE`, exact completion `0172D7C7`, close `01087887`. RX unread at arm144, at close216. Receipt first visibility is6,664.323ms after processed close; completion is6,971.200ms after close. Total publication-to-completion7,524.838ms. Accepted request→dispatch55.761ms, dispatch→queue329.177ms, queue→publication311.236ms. Three admission attempts include two FlowBusy and no NoCapacity.

Ruleset3: RX `0109D504`, dispatch `010AACF8`, queue `010AF4F3`, publication `0110E82E`, arm `0110E837`, observed tail `01110036`, exact completion `0176D28D`, close `015D0687`. RX unread at arm0, at close72. Receipt first visibility is1,122.947ms after processed close; completion is1,690.630ms after close. Total publication-to-completion6,679.135ms. Accepted request→dispatch55.284ms, dispatch→queue18.427ms, queue→publication389.947ms. One admission attempt, no FlowBusy/NoCapacity.

These are19-byte opcode5/code1 replies, not OACKs awaiting DATA. Both85-byte requests offer1000ms timeout. Ruleset2 has options17/rsize4; ruleset3 options0F/rsize absent. The page does not establish complete request-byte identity, initial request FIFO visibility, proprietary worker deadlines, or application acceptance. A prompt TX-tail crossing only establishes transport consumption. The absent/visible bracket localizes arrival into shared RX; it does not by itself explain why the remote side produced that receipt late or exclude an indirect dependency on earlier host work. No synthetic completion or early reuse is justified.

## Successful boundaries and remaining backlog

| Exchange | Accepted → publication | Publication → matching completion | Matching completion relative to processed close |
| --- | ---: | ---: | ---: |
| Ruleset1, peer21/reservation13 | 781.839ms | 885.676ms | **938.633ms before** |
| Ruleset2 | 696.174ms | 7,524.838ms | 6,971.200ms after |
| Ruleset3 | 463.658ms | 6,679.135ms | 1,690.630ms after |
| Ruleset4, peer23/reservation1C | 2,238.520ms | 696.354ms | 2,578.979ms after |
| Retained EFS1, peer22/reservation12 | 735.548ms | 1,002.536ms | **1,004.285ms before** |
| Retained EFS2, peer22/reservation14 | 265.539ms | 597.741ms | **701.141ms before** |

Ruleset total4/omitted0. This child does not print EFS total/omitted counts, so the two retained entries are not claimed as exhaustive EFS coverage. Each displayed entry has flagsBF and ERROR1. Ruleset4 waits2,120.436ms accepted→queue; it queues73.912ms after the exact ruleset3 completion, consistent with the existing retained-flow gate, without proving every microsecond of that wait. It was already closed before queueing. Do not retire published ownership speculatively to clear this gate.

Unlike IMG_9821/9822, the inherited reverse-arrival handoff **did run**: service calls1/result2 (ResponseQueued), flags7F, writer-group calls1. First EFS queues122.956ms before first ruleset. The page does not print the group's committed commands, so its count alone cannot identify which packet consumed each writer turn. Different instrumented boots are not a controlled timing comparison. The new observer changed no scheduling policy.

Original peer1F sent DATA. Its final ACK publication `00DA24A4` → matching completion `00DA89D6` takes25.906ms; completion is89.188ms before close and10,042.947ms before ELF publication. This is an established successful original write, not a missing final ACK to debug again.

ELF accepted→queue2,430.450ms, queue→publication111.979ms, accepted→Owner completion3,471.930ms. Its observed TX read is6.123ms after publication. Receipt remains absent241.933ms after observed tail, first visible260.953ms after tail in a19.020ms bracket, with168 bytes ahead. First visibility→Core completion643.996ms, Core→Owner18.429ms. ELF's remaining host drain is real and separate from the ruleset receipts' multi-second pre-visibility gaps. Exact body match1, host-credit mask7. TX3 calls1/mask001803A0 alone does not enumerate the unprinted writer sequence.

Runtime error/detail0, close stage6, no pending close. Live IPA flags1F, registration1/driver1/publications4/response1, no pending message. Historical IPA complete1, PD7FFFFFFF, RAM MCFG writes1/file length1. **WLAN RRQ0 / WLFW servers0**. No WLAN milestone advanced.

## Source and saved-trace comparison; next offline target

The pinned successful Linux journal still shows ruleset and modem_efs missing-file rejections at177.039398–177.040307s, followed by the first WLAN firmware RRQ at178.730030s; first MCFG is later at187.027163s. The pinned `tqftpserv.c` failed-open path sends ERROR1 and returns. Missing ruleset content itself is not a demonstrated WLAN blocker and these ERROR replies do not call for OACK/DATA. Initial host acceptance here is not the journal's request-generation boundary and these clocks must not be mixed to derive a modem timeout.

The saved mainline GLINK source separates scheduling/transmission of receive-done notifications from receipt handling, and returns a remote intent to availability on real RX_DONE. The reviewed Linux/Android host implementation defines ownership/order, not the closed modem worker's entire startup decision. Current capture resembles IMG_9775's first EFS case: prompt consumption followed by seconds without its receipt in RX. Subsequent EFS changes produced before-close completions (IMG_9797), but did not establish WLAN startup. Thus this new ruleset result cannot be promoted to a known final Wi-Fi fix.

Next, compare the pending receive/control work and earlier request visibility around the shared ruleset receipt-arrival bracket with the existing EFS replays and Linux/native startup notes. Determine whether a host grant/credit/return dependency is evidenced, or whether the request was already late before host acceptance. Neither is established by this page. Only add a new capture if those saved records cannot distinguish the concrete alternatives. Repeating generic writer-turn, OACK or slot-occupancy probes would not recover the missing remote-worker decision. No additional Pixel command or unchanged cold boot is needed to interpret these photos.

Local reference paths: `.cache/sunfish-mobian-comparison/device-runs/cold-wifi-20261025T200752Z/journal-final.txt`; `.cache/sunfish-mobian-comparison/wifi-preparation/source/tqftpserv/tqftpserv.c`; `.cache/sunfish-linux-mainline-ref/drivers/rpmsg/qcom_glink_native.c`. Related notes: [observer semantics](sunfish-ruleset-reply-fifo.md), [prior frozen outcome](sunfish-ruleset-service-handoff-IMG_9821.md), [successful Linux startup ordering](sunfish-early-startup-prerequisites.md), [earlier EFS receipt gap](sunfish-efs-reply-fifo-IMG_9775.md), [EFS overlap outcome](sunfish-efs-reply-overlap-IMG_9797.md).
