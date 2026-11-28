# IMG_9813 — original-return loan vetoed by ready QRTR credit

The settled TX3 OWN RETURN TURN photograph is associated with source `df9c96bb9dab5b74cd63247daf0e68c04573ee19`, image SHA256 `dafd1a8a4e17e775edfa647a5a6eccf4ae022177873e93c582cbe926171feb3c`. Original HEIC, readable JPEG, manually transcribed physical rows, executable decoder and hashes are archived separately under `.cache/sunfish-native-wifi/private/native-tx3-own-return-turn-v1/device-result/`. All 123 delivered artifacts were reverified unchanged. No device command or new image build was performed for this analysis. Times below are milliseconds converted from hexadecimal runtime microseconds.

## The new callback did not run; the new counters identify its last veto

TX3 frozen snapshot2, calls80=128 and maskE0AC contain neither own-return callback bits21/22 nor existing ready DATA bits19/20. New candidate gate7 is zero. Consequently no blocked or failed attempt of the new return was observed: it never reached its checked callback.

| New own-return first-failing gate | Hex count | Decimal |
| --- | ---: | ---: |
| 0 nonordinary call | 0 | 0 |
| 1 protected work/priority DATA/cooldown/active conversion | 13 | 19 |
| 2 no positive pending grant ACK | 2C | 44 |
| 3 no exact unadvertised fresh backing | 21 | 33 |
| 4 no eligible scoped/marked original pending return or spent opportunity | 0 | 0 |
| 5 normal DATA order/fit/outbound-wait/flow-predecessor selector differs | 0 | 0 |
| 6 ready QRTR credit | **20** | **32** |
| 7 candidate available | 0 | 0 |
| Total | 80 | 128 |

The helper checks gate6 after the preceding private live-state, normal DATA-order and fitting-intent conditions. Thus on32 actual checks the queued original TX3 return and the normal fitting reply candidate were eligible up to the ready-credit veto. This is stronger evidence than IMG_9812's earlier borrowed-gate counters, whose first failures hid these later conditions. It does not identify32 distinct credit packets, their owning flows or the exact credit bytes. The counter cannot tell whether one queued credit was observed repeatedly.

Credit priority is intentionally preserved. This result supports an offline replay of one normal ready credit first, followed by the bounded own return and existing reply opportunity if all guards remain valid. It does not authorize bypassing the credit, assuming its completion, draining arbitrary credits, or borrowing after an already advertised backing. It does not prove the whole12.667-second wait can be removed.

## Publication pressure and prompt receipts recur

TX3 Q/P/Core are01220992/01E35145/01E3B6B0: queue→publication **12,666.803ms**, 51.812ms longer than IMG_9812. The original priority counters repeat0 cooldown,9 protected TFTP,105 pending intent-control,13 own-return,0 other exclusions and1 registry selection. Existing borrowed-grant first failures also repeat19 no ACK,36 negative ACK,37 no backing and36 no yield/conversion opportunity. ACK progress is37 sent,0 blocked,32 returns ahead and36 advertisements ahead; do not substitute IMG_9812's34 returns.

| Current post-publication interval | TX3 | ELF |
| --- | ---: | ---: |
| Observed peer-tail crossing / RX_DONE first visible | 6.249 | 6.190 |
| Visibility → Core matching receipt | 19.714 | 19.754 |
| Publication → Core matching receipt | 25.963 | 25.944 |
| Core → Owner completion | Not separately displayed | 18.421 |
| Publication → Owner completion | Not separately displayed | 44.365 |

TAIL/VIS are the same first sample for each reply, with unavailable ABS, zero bytes ahead, finished1 and zero history evictions. This is prompt observed consumption/receipt; it does not measure an exact simultaneous modem read/return. ELF armed at01EC5CBD with zero unread bytes and remote intent0. No late-receipt pattern from IMG_9811 was reproduced.

ELF R/Q/P/Owner are014C9E91/01EB6C9C/01EC5CC5/01ED0A12: receive→queue10,407.435ms, queue→publication61.481ms, receive→Owner10,513.281ms. Its single ordinary writer call has maskA0, not paired-credit bits17–20. Completed span00009013 is36.883ms; subtracting independently converted first/last times gives36.884ms, consistent with1us conversion rounding. The panel again records one peer admission rejection and no full-pool/flow/credit/capability rejection. Last peer-failure snapshot01E596D7 precedes queueing by382.405ms and retains unpublished transaction4; this does not label the entire prequeue interval a continuously attempted rejection.

## Status and retained digest outcomes

Live IPA flags1F, no pending message, exact ELF host-credit mask7 and body match1 remain established. Original final ACK completion00DA4E6E is present. Its absolute timestamp differs substantially from IMG_9812; this panel does not supply the original WRQ/OACK timings to allocate that earlier startup difference. Historical IPA complete1, PD7FFFFFFF, **WLAN RRQ0 / WLFW0**, one RAM MCFG write/file byte and no full firmware read completion remain unchanged.

Both retained digest records name peer50 and transfer port500A, with distinct reservations21/22. The first has flagsBF and real publication/completion before close. The second has flags8F, no publication or completion and the same observed close; do not describe both as completed. This is consistent with retained unpublished close cleanup, not evidence of a second successful transfer. Total12/omitted10 prevents generalizing these two outcomes to every digest request.

No further Pixel run is required merely to interpret this capture. Next work is a bounded credit-first offline replay using actual request identity and the existing normal credit selector, preserving grant advertisement-before-ACK, blocked-write fallback, all FIFO checks and original receipt ownership.
