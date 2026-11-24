# Settled ELF QUEUED HANDOFF — IMG_9713

Physical result supplied November 24, 2026 for verified `native-elf-queued-handoff-v1`, image SHA-256 `3ef6fac6b2570e63534254f55609124c7c6c5f9441b7c3652ba09d85c68330d9`. The original HEIC and decoded fields are retained in that profile's private `device-result/`. The delivered verification manifest and source revision remain unchanged.

## The queued handoff worked, with no WLAN advance

Flags are **0x1F**: the exact ELF reply armed the one-shot, the capability was consumed, normal dispatch entered and returned, and the exact reply published by the end. Entry `017152BB`, publication `0172BE64`, end `0172BE66` place publication inside that turn. It reports one received record, one publication-progress event and no IRQ acknowledgement. The direct raw duration displays `00016BAA` = **93.098 ms**. Subtracting independently rounded displayed ages gives 93.099 ms; that one-microsecond difference is normal integer conversion, not a different event.

| Current exact ELF boundary | Hex microseconds | Seconds from runtime start |
| --- | --- | --- |
| Request accepted | 0139A019 | 20.553753 |
| Receive pass ended | 014748B5 | 21.448885 |
| First registry entry | 0147786C | 21.461100 |
| First ELF selection | 01481FB7 | 21.503927 |
| Reply queued / handoff armed | 017152B9 | 24.203961 |
| Reply fully published | 0172BE64 | 24.297060 |
| Matching GLINK completion | 017D054F | 24.970575 |

Queue-to-publication is **93.099 ms**, compared with **443.446 ms** in IMG_9712: that interval is 350.347 ms shorter, about 79%. These are different boots, not a controlled benchmark. Overall receive-to-publication is **3,743.307 ms**, versus **3,729.709 ms** previously. Thus this boot establishes an isolated post-queue scheduling improvement, not an overall ELF/startup speedup.

## The larger delay precedes queueing

The existing phase projection partitions receive-to-queue into:

| Phase | Time |
| --- | ---: |
| Request acceptance → receive-pass end | 895.132 ms |
| Receive-pass end → registry entry | 12.215 ms |
| Registry entry → ELF selection | 42.827 ms |
| ELF selection → queue admission | **2,700.034 ms** |

ELF recorded **eight same-peer FlowBusy rejections**, with zero transmit-slot-capacity, legacy-flow-capacity or reference-credit/table-capacity rejections. Its first named blocker is transaction 4, flags `0x08`: the local reference retirement condition was set, but it had not started publication at that first observation. This bit does not establish receipt of a QRTR RESUME. The sole preselection run `00041101` means transaction 4, `tms/servreg`, successfully queued once. The larger later selection-to-admission interval is not part of that preselection run.

Transaction 4 receive/queue/publication/completion are `0133D084 / 0147EFEB / 01529AE4 / 0173E20A` (20.172932 / 21.491691 / 22.190820 / 24.371722 s). Its queue-to-publication interval is 699.129 ms. ELF queues **2,013.141 ms after transaction 4 publishes**, while transaction 4's completion is later than ELF publication.

The latter ordering is permitted by the existing bounded two-registry-reply window. It does not indicate a fabricated completion or a new handoff bypass: normal queue admission allows one fully published older reply to remain in flight with its own intent and credit. It refuses a third same-peer reply. Therefore transaction 3 still occupying the other window slot is one possible explanation for the later admission pressure; this panel does not show its exact retirement. The first-blocker snapshot alone cannot prove transaction 4 was the sole blocker for all 2.700034 seconds. Validation and other service work also lie within that interval. The unchanged Core admission logic is in `qcom_glink_qmi.rs::queue_reply` and is documented in [the bounded-window notes](sunfish-bounded-registry-window.md).

ELF still returns the expected body and completes. Its peer is 9, offset 0, returned rows/total domains 1/1. Host-credit flags `0x0F` again establish requested confirmation RESUME publication and completion before ELF publication. Publication-to-matching-completion takes 673.515 ms. All five observed locator requests queued and completed (both masks `0x75`); name counts are tms/servreg 1, GPS 0, ELF 1, wlan/fw 0, PDR 1, pddump 1 and other 1, with avs/audio count 1.

## Other startup observations and capture limits

The original write's final ACK completed at `00D8F34C` (14.218060 s), 6.335693 seconds before this ELF lookup. IPA completed. **WLAN RRQ=0, WLFW servers=0, PD state=0x7FFFFFFF**, with runtime error zero. Neither WLAN firmware loading nor a new domain-state milestone is established.

This boot's first captured post-ELF MCFG target is **peer 0x4A**, not IMG_9712's peer 0x4B. Its flags are **0x83**: received, server dispatched and closed, with **no nonempty reply prepared**, queue, publication, matching completion or client ACK/ERROR. It arrived at `0182F198` and closed at `018A59F6`. The new summary panel omits the retained numeric server result and offered options; `FFFFFFFF` in its ERROR field is unavailable, not error zero or success. We cannot assign this outcome to zero-rsize, capacity, route state or another rejection from the photo. The earlier successful missing-file reply was a different request/boot and must not be substituted here.

The settled MCFG counters remain one write / one committed byte / zero completed reads. The first other accepted path is exact `mbn_hw.dig`, peer 0x4E, 93-byte RRQ. The bounded journal saw 44 relevant records, omits 28 middle records and reports zero malformed candidates. It cannot reconstruct all intervening exchanges.

## Next offline discriminator

Keep the demonstrated queue handoff and the existing checks. Examine the two-slot registry window across transactions 3, 4 and ELF, particularly the older matching completion versus ELF's same-peer admission attempts, using the saved window/FIFO traces and source. Do not simply enlarge the window, retire a buffer without its real receipt, or call all of the elapsed time a remote wait. Separately inspect the MCFG dispatch-result paths; its new target has no prepared reply, but its actual options/result are not visible on this summary.

No rerun of this image is needed to establish the successful handoff or the unchanged WLAN outcome. A further hardware capture should expose a named missing boundary or result, rather than repeat queue-publication timing alone.
