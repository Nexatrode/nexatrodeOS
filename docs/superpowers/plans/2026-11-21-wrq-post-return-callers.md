# WRQ post-return caller attribution

User authorizes offline analysis through a verified image, with no automatic phone actions. Work directly on main. Preserve the measured Normal-NC SDK/mapping contract, all validation/ordering, and the IMG_9688 artifact.

The panel's40/77 count is successful Ring::sample calls, each containing status, cursor and acquire operations and their native pre/post checks. The saved screen retains aggregate costs but not callers or packet sequence. It cannot establish which37 extra calls occurred. WRQ return→registration queue improved92.798→62.303ms; queue→registration publication grew342.353→422.509ms; registration publication→OACK grew230.725→352.684ms. Table cost per sample improved about53%; inferring an exact traffic cause from those aggregates would be guessing.

Use observer-only caller attribution, rather than removing checks or guessing a scheduling change. An opt-in leaf inherits the Normal-NC image. Tag existing samples with source basename hash/line and validated RX occupancy using track_caller; propagate through gate/revalidate so service callers remain distinguishable. No extra FIFO reads or clocks. Aggregate the existing bounded history over return→registration queue, queue→registration publication and publication→OACK. Show the nine most expensive callers with per-window counts/time, zero-RX count and maximum validated RX bytes; report all omitted counts/time, overflow/loss/missing evidence. Keep current close/read/DATA/IPA/WLAN outcomes.

- [x] Test caller propagation, clipping/counts, zero/unknown RX, omitted-site accounting, capacity/eviction and freeze before implementation; add bounded metadata/aggregation and observational capture.
- [x] Build/render the panel and freeze source-line map. Replay identical protocol I/O/events with feature on/off, run native fault controls and owner/link/source/stack suites. Review independently; do not treat fake clocks as phone time.
- [x] Reuse pinned Normal-NC SDK without mutation; build separate image, audit exact maps/304 mutations, reproduce complete loader/CapDL/report, verify wrapped image and source/artifact hashes. Preserve originals, commit/push main, give manual cold temporary boot command.

The next boot identifies actual repeated callers and intervals, not a promised deadline fix. Existing source/reference notes explain ordering and checks; they do not reconstruct this handset's unrecorded dynamic call sequence.
