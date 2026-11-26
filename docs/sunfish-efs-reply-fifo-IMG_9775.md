# IMG_9775: first EFS ERROR consumed promptly, RX_DONE appears late

The settled **MODEM EFS REPLY FIFO** panel is associated with source `97b9b9f96a02945367c79333173719d85626f65a`, image `3d674f92a5e58c63872837461435e412e3b6ef570eff430ea22c25177a881dc5`. Raw photo, readable views and decoded data are saved separately in that profile's `device-result`. This association is not an independently measured handset image hash. Delivered artifacts remain unchanged.

The exact first modem_efs ERROR is peer0x22, reservation0x13, remote intent0, opcode5/code1/19 bytes. It queued on its first attempt with no FlowBusy/NoCapacity and retained flags0xbf. The first/second arrival order remains fixed.

| Event | Same-clock hex microseconds |
| --- | --- |
| Request accepted |`00E132E1`|
| Server dispatch |`00EF7344`|
| Reply queued |`00EFBB3D`|
| Reply published |`00F7FAE2`|
| FIFO watch armed |`00F7FAEB`|
| Peer tail crossed reply |`00F8131A`|
| Last sample excluding receipt |`016CE001`|
| First sample containing receipt |`016D2A31`|
| Matching receipt processed |`016FABE4`|
| Client close processed |`0108A0B3`|

The modem's TX-tail crossing was observed **6.200ms after publication**. This bounds shared-FIFO consumption; it does not prove that the remote TFTP client accepted the error. RX was empty at arm. The exact matching receipt remained absent through **7,654.631ms after that observed tail crossing**, first appeared18.992ms later, and was processed **164.275ms after first visibility**. Total publication-to-processed-completion is7,844.098ms. Most of that interval therefore precedes the receipt's shared-RX visibility; it is not an eight-second host receive backlog or a slow modem read of this reply.

The receipt was not visible at accepted close; the first visible bound is6,588.798ms after close. The visible sample has **72 bytes ahead** of the matching8-byte RX_DONE. Their identities are not shown and cannot be imported from earlier captures. History evictions27 do not erase the numeric bounds retained for this exact matching receipt. The watch is finished and correctly bound to reservation0x13/intent0.

The host still spends **933.987ms receive-to-dispatch**,18.425ms dispatch-to-queue, and **540.581ms queue-to-publication**: **1,492.993ms from accepted RRQ to ERROR publication**. Publication precedes the host's close observation by1,091.025ms, but that does not establish an application deadline. A late response to an already timing-out worker remains possible; delayed receipt generation is not proof of a modem bug or of a missing host ACK. The negative reply body matches the existing Linux-compatible missing-file policy; no OACK/DATA follows ERROR1.

The second EFS request waits behind the first published flow: reservation0x1a,12 admission attempts,10 FlowBusy,1 NoCapacity, then success. Runtime error/detail0, no pending close, stage6. Original final ACK phase4, matching completion `00DB07F0`; ELF `014D4D72 / 0171441B / 0173299D / 0180038E`, body match1/host-credit mask7. IPA complete1, PD UNINIT, **WLAN RRQ0 / WLFW0**. No WLAN milestone moved.

The next offline question is whether the already-admitted exact first EFS ERROR can borrow a bounded ordinary writer turn immediately after queue admission, while keeping a fresh RX veto, advertisement before grant ACK, same-peer flow and real completion checks. The existing MCFG checked-turn pattern is relevant, but this must have separate EFS scope, one-shot accounting and default-off behavior in tests. It cannot claim to solve the933ms service wait or guarantee a proprietary deadline. Do not remove checks or invent a receipt to clear the flow. No Pixel command is needed for this interpretation.
