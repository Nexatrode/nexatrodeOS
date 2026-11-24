# IMG_9692: original server-check DATA observed

The settled `WRQ TABLE PAIR READS` panel from the user is the first result in
this comparison with a non-absent FIRST DATA timestamp. Runtime error is zero,
and LOST/MISS/SITE/RX/OVER are all zero. The private original HEIC, rendered
preview, source SHA and decoded JSON are archived under
`.cache/sunfish-native-wifi/private/native-wrq-table-pairs-v1/device-result`.

| Event | Runtime microseconds, hexadecimal |
|---|---|
| Original WRQ buffer return | 00C0B31C |
| IPA registration queued | 00C17392 |
| Registration published | 00C5D677 |
| Original OACK published | 00C8BAF0 |
| Modem read OACK | 00C90C30 |
| First DATA accepted into transport | 00CAB459 |
| Exact close first visible | 00DEF74D |

The post-return intervals contain 8/33/21 successful samples, or 62 total.
Their measured check times are 49.191/203.059/129.198 ms, or 381.448 ms total.
WRQ return to OACK publication is 526.292 ms, versus 799.887 ms in IMG_9691.
That is about 274 ms less in this boot. Sample count was 72 and checked time
562.933 ms in IMG_9691. These are single-boot observations with a different
protocol reaction, not an isolated repeated benchmark of table-read speed.

The modem read OACK 20.800 ms after publication, then DATA appeared 108.585 ms
later. OACK read preceded close visibility by 1,436.445 ms; DATA preceded it by
1,327.860 ms. Close visibility is a validated local FIFO observation, not the
exact instant the remote client decided to close.

Identity was checked against the frozen source: `server_check_oacks()[0]`
selects the earliest accepted WRQ trace. `observe_server_check_rx` records
FIRST DATA only when the peer address matches that trace, the local transfer
port selects its slot, and the packet has TFTP opcode DATA (3). This is the
original transfer's DATA observation, rather than an unrelated completed peer.
It does not itself prove application payload acceptance, final ACK or a fully
completed transfer; those fields are not displayed on this panel.

IPA COMPLETE is 1, WLAN RRQ RX is 0 and WLFW SERVERS is 0. The next offline
question is whether this DATA was dispatched and its final ACK reached the
client before close, then how that completion relates to the ELF locator
reply and the first WLAN firmware read. Do not claim WLAN or working Wi-Fi.
