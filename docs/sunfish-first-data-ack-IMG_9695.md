# IMG_9695: original write accepted, ACK and locator reply completed

The settled FIRST DATA ACK ELF panel has runtime error 0, original peer
0:0x1f, transfer port 0x5001 and TRACE MATCH 1. Original DATA dispatch returned
0 for block 1, with nine TFTP packet bytes (five application bytes). FINAL
BLOCK is 1, ACK PHASE is 4, and WRITES COMPLETE is 1. This proves original
local write completion and a matching GLINK receipt of its final ACK. It does
not prove that the remote TFTP application accepted that ACK before timing out.

| Event | Runtime microseconds, hexadecimal |
|---|---|
| OACK published | 00C85928 |
| OACK peer read | 00C870F4 |
| DATA received | 00CA176C |
| Final ACK published | 00DA0EA0 |
| Final ACK GLINK completion | 00DE9190 |
| Exact close first visible | 00DE44F6 |
| Exact close ingested | 00E24462 |
| ELF lookup received | 014A21D2 |
| ELF reply queued | 0240CC84 |
| ELF reply published | 02420264 |
| ELF reply GLINK completion | 0242AE66 |

OACK peer read followed publication by 6.092 ms; DATA followed that read by
108.152 ms. DATA receipt to ACK publication took 1,046.324 ms. This is a
remaining substantial delay, around the advertised 1,000 ms timeout interval;
the panel does not timestamp the client's exact timer start or ACK read.

ACK publication preceded close visibility by 276.054 ms. Its GLINK completion
was 19.610 ms after visibility but 242.386 ms before close ingestion. Therefore
ACK BEF CLOSE=0 describes that particular completion/visibility comparison;
it is not, by itself, a TFTP failure verdict. Visibility is a local validated
FIFO observation, not the exact remote decision to close.

ELF lookup receipt to queue admission took 16,165.554 ms. Queue admission to
publication took 79.328 ms, and publication to GLINK completion took 44.034 ms.
The total locator request-to-receipt interval was 16,288.916 ms, dominated by
work before admission. ELF completion concerns the service-registry locator
reply, not WLAN firmware loading.

PD QUERY STATE remains 0x7fffffff, WLAN RRQ and WLFW SERVERS remain 0, and their
first timestamps are absent. The next offline comparison should cover both
the original DATA-to-ACK path and the long ELF prequeue interval. Earlier
registry traces identified transaction 4 and same-peer transaction 3 waits in
some profiles, but matching duration is insufficient to assign that cause to
this boot. Reuse those notes and available service-phase observations before
designing another capture. Do not continue treating the original OACK/no-DATA
condition as the only known stage.

The original HEIC, rendered preview, SHA-256 and decoded arithmetic are under
`.cache/sunfish-native-wifi/private/native-first-data-ack-elf-v1/device-result`.
No additional phone command or driver change was needed to interpret this
panel. Working WLAN or Wi-Fi has not been demonstrated.
