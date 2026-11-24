# Transaction 3 receipt — IMG_9708

The settled REGISTRY TX3 RECEIPT panel reports no runtime error, IPA complete, zero WLAN firmware RRQs and zero WLFW servers. The private result is preserved under `native-registry-tx3-receipt-v1/device-result`; the prior delivery manifest remains unchanged.

| Event | Runtime microseconds (hex) |
| --- | --- |
| Transaction 3 queue | 011AA221 |
| Full publication | 011F7490 |
| Peer tail crossing | 011F8C9B |
| Last sample without matching receipt | 019051B9 |
| First sample with matching receipt | 01909B78 |
| Matching decoded RX_DONE | 019A6E37 |
| First converted grant reservation | 011E9D9D |
| Grant advertisement | 01204B57 |
| Grant ACK | 0121A5CE |
| Transaction 4 publication / completion | 015D9820 / 019EAA47 |
| ELF request / selection | 014CDE95 / 0154E16E |
| ELF queue / publication / completion | 019D8706 / 019F519A / 01AB24EF |

The modem consumed the reply 6,155 µs after publication. Matching receipt first became visible 7,415,528 µs after publication, or 7,409,373 µs after consumption. Host decoding followed visibility by 643,775 µs. This separates a post-consumption gap from the already resolved original publication delay; transport consumption does not establish application acceptance.

The first converted grant advertisement followed reply publication by 54,983 µs; its ACK followed by 88,695 µs. Those recorded events alone do not explain the remaining gap. The scalar history lost 24 records and reports 96 bytes ahead of the matching receipt. It cannot identify all intervening traffic, later grant obligations, or which side caused the post-consumption delay.

Transaction 3 occupied 88 payload bytes, the exact receipt latch matched, and eight repaints were deferred. ELF queued 202,959 µs after transaction 3 completed and 74,561 µs before transaction 4 completed. These observations support capturing bounded accepted RX and committed TX header IDs over transaction 3's publication-to-receipt window, without another scheduling change.
