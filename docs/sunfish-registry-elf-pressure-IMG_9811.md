# IMG_9811 — earlier registry/ELF publication, paired ELF turn exercised

The settled REGISTRY ELF PRESSURE result belongs to source revision `162a8abd4139606444bac027fb1284621b0b37f5`, image SHA256 `d68ff4a52d256d06ef44bd0d4f151892e701205bcae98db9dfb500efffc7734a`. The original HEIC, readable JPEG, selected manually transcribed physical rows, executable decoder, comparison and hashes are archived separately under `.cache/sunfish-native-wifi/private/native-registry-elf-pressure-v1/device-result/`. All 110 delivered artifacts were verified unchanged before adding results. Panel values are hexadecimal; converted durations below are milliseconds.

| Interval | IMG_9810 | IMG_9811 |
| --- | ---: | ---: |
| TX3 queue → publication | 12,694.940 | 260.962 |
| TX3 publication → Core completion | 25.947 | 4,119.376 |
| ELF receive → queue | 10,204.397 | 1,024.823 |
| ELF queue → publication | 61.444 | 170.963 |
| ELF publication → Owner completion | 44.344 | 1,803.327 |
| ELF receive → Owner completion | 10,310.185 | 2,999.113 |

TX3 Q/P/Core timestamps are 013023F7/01341F59/0172FAA9. TX4 Q/P/Owner are 014534CD/0147D5FF/01749531. ELF R/Q/P/Owner are 014977A3/01591ADA/015BB6AD/01773AEC. ELF queued 9.623685 seconds earlier and completed 7.755183 seconds earlier on the runtime clock. Its receive-to-completion duration improved by 7.311072 seconds. Queue-to-publication and post-publication completion were individually slower, so overall progress must not be described as improvement in every phase.

The retained TX3 publication is frozen, with three calls and mask 00003FA4. Bits 11/12 show the existing converted-grant DATA attempt/commit; bits 19/20 are absent. Its three priority observations see pending intent ACKs; grant counters include one negative ACK, one missing backing and one eligible selection. This does not physically isolate the new queue-time ready-marker fix, whose separate offline replay remains the evidence for that omission.

ELF has three calls and mask 001E27AC: bits 17/18 and 19/20 show both successful stages of the paired normal-ready credit then ELF DATA loan. This is the first confirmation in this comparison that the paired ELF turn ran on the Pixel; IMG_9810's clean ordinary pass did not exercise it. The mask is a retained observation, not scheduling authority. The paired credit need not be ELF's own flow credit. Live ELF host-credit completion mask 7 is recorded separately.

Writer first/last are 01597A25/015BB6A1, three completed passes, zero omitted, total completed spans 0001AD45 and maximum inter-call gap 0000479D. Flags 1 mean finished, inactive and no clock regression. Total completed spans sum callback time; they do not equal first-to-last elapsed time when multiple passes have gaps. The timeline adds 24.395ms queue-to-first, 146.556ms first-to-last and 12us last-to-publication to yield 170.963ms. ELF admission failure counters are zero and last failure is unavailable; do not invent an admission rejection to explain the 1.025-second receive-to-queue interval.

ELF queued before either older reply's recorded GLINK completion. The panel therefore does not show ELF waiting for those two actual receipts before admission. Matching receipt ownership and published-buffer retention remain intact; the longer TX3/ELF post-publication waits require comparison against prior FIFO observations before assigning modem-versus-host delay.

Live IPA flags are 0000001F, pending message unavailable/none, ELF host-credit mask 7 and reference-body match 1. Original final ACK completion is 0009207C. Historical IPA complete is 1, PD state 7FFFFFFF, WLAN RRQ count 0 and WLFW servers 0. RAM MCFG writes/file bytes are 1/1; full firmware read completion count remains 0. No new WLAN startup milestone is demonstrated.

Next offline question: compare the longer TX3 publication-to-Core receipt and ELF publication-to-Owner completion with the saved FIFO/header and registry traces. This panel does not provide the exact peer-tail and first-visible times for those replies. Further Pixel action is not required merely to interpret this result.
