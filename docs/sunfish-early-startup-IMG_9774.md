# IMG_9774: arrival order fixed, first modem_efs completion still late

The settled **EARLY TFTP ARRIVAL ORDER** panel is associated with source `eb8dde530aaf39c5e74775a52e1757395d323300`, image `dd74767d7f18db0d50635716af3f01b1c7b734e67e56f969b5fb4ba2de88ac2c`. Raw photo, views and decoded clocks are saved separately under that profile's `device-result`. Association is not an independently measured handset image hash. Earlier delivered artifacts remain unchanged.

The first modem_efs RRQ is now dispatched, queued and published **before** the second. Peer0x22's first request receives reservation0x13, second0x1a; in IMG_9773 the first accepted EFS request was assigned the later reservation. This is the intended device ordering outcome. Paths, handles and clocks are scoped to this boot; port numbers across boots are not stable logical identities.

| modem_efs event | First accepted | Second accepted |
| --- | --- | --- |
| Receive |`00E04AC3`|`00F0D255`|
| Dispatch |`00EE865A`|`010423A6`|
| Queue |`00EECE40`|`0173E19C`|
| Publication |`00F67B48`|`0177DB9A`|
| Matching completion |`016E686B`|`018644DC`|
| Close |`0106EC74`|`0106EC74`|

The first request's receive-to-dispatch delay is**932.759ms**, compared with3,911.462ms in IMG_9773. These are separate instrumented boots, not a controlled timing benchmark. Dispatch-to-queue is18.406ms and queue-to-publication503.048ms, giving **1,454.213ms receive-to-publication**. It queues on its first attempt with no FlowBusy/NoCapacity. Its ERROR1 publishes **1,077.548ms before accepted close**, but matching RX_DONE is **7,859.491ms after publication**, or **6,781.943ms after close**. Publication before the host observes close does not prove application receipt before a private client deadline.

The second request instead sees ten admission attempts (eight FlowBusy, one NoCapacity), queues7,323.126ms after dispatch, and publishes7,401.254ms after close. This is consistent with the later response now waiting behind the original published flow; the experiment did not bypass same-peer flow checks or fabricate completion. The actual modem FIFO read time and RX_DONE visibility time are absent from this panel, so late processed completion cannot distinguish remote read delay, peer receipt-generation delay and host receive processing.

All four retained early replies remain ERROR1,19 bytes, flags0xbf, server result0, no client ACK/ERROR captured. First ruleset peer0x21 completes354.609ms before close; second ruleset publishes149.831ms after close and completes7,888.113ms afterward. Four ruleset observations have two retained/two omitted; both EFS observations are retained. Global read count16, kind mask0x2e, without WLAN kind. Ruleset request option presence0x17/rsize4 and EFS0x0f/rsize absent are unchanged projections, not proof of byte-identical retries.

The original server-check transfer still completes: peer0x1f, WRQ `00C0C891`, OACK `00CBBC4C`, DATA `00CD7D71`, final ACK phase4/publication/completion `00D9BCC1 / 00DA21AF`. Matching ACK completion precedes ELF request acceptance by7.467748s. ELF receive/queue/publication/completion `014C1493 / 01700024 / 0171E4EE / 017EBB61`; body match1, host-credit mask7, locator queued/completed name masks0x75. Runtime error/detail0, no pending close, cleanup stage6, IPA complete1, PD UNINIT, **WLAN RRQ0 / WLFW0**.

The arrival-order defect has a positive device result, but no new WLAN milestone. The next offline target is the exact first EFS ERROR's FIFO consumption and matching receipt visibility, reusing the existing checked cursor-history observers with original reservation/intent identity. Such observation must add no shared-memory read, grant/credit priority, lifecycle or retirement change. Keep arrival ordering and the first/second request identities. Another generic MCFG/OACK probe or treating RX_DONE as application acceptance would not resolve this gap.
