# WRQ post-return caller diagnostic

IMG_9688's faster Normal-NC tables did not get the original OACK ahead of the close. Compared with IMG_9687, return→registration queue improved from92.798 to62.303ms, while queue→registration publication grew from342.353 to422.509ms and publication→OACK grew from230.725 to352.684ms.

The displayed40/77 “checks” count is successful `Ring::sample` calls overlapping the post-return interval. Each sample performs status observation, cursor acquisition and acquire-barrier/observation, with the existing native pre/post domain validation around those operations. It is not a count of received packets or individual table scans. The old photo gives aggregate cost and completeness, but does not identify its additional37 sample callers. Source shows receive peeks/commits, write begin/readback/completion, publication admission and service revalidation as distinct producers; source alone cannot reconstruct which ran in that boot.

The next image adds source caller and validated RX-occupancy metadata to the existing bounded sample history. `track_caller` forwards through sample/gate/revalidate wrappers; the diagnostic records source basename hash and line. It does not add a FIFO read or clock read, remove validation, change quarantine/error ordering or modify protocol scheduling. The measured Normal-NC SDK and eight-table memory policy are reused without mutation. Dynamic RAM/code allocations remain subject to the candidate's full capability audit.

The **WRQ POST RETURN CALLERS** panel splits time into three intervals:

1. WRQ buffer return → registration reply queueing.
2. Queueing → registration reply publication.
3. Registration publication → original OACK publication.

For each interval it shows total duration, sampled validation time/count, RX/TX enclosing spans, endpoints and native completeness. The next nine rows rank retained callers by combined sampled time, showing their three counts/durations, total zero-RX observations and maximum validated RX bytes. Frozen `caller-line-map.json` and `caller-sources` resolve the displayed hexadecimal file IDs/lines. The map has15 collision-free source IDs and121 call locators.

Omitted-site counts/time, history loss, missing native sets, unknown site/RX and site-overflow are explicit. The32-site aggregation ranks only retained sites if overflow occurs; do not claim global top-nine attribution in that case. A sample crossing an interval boundary can count in both intervals, but its time is clipped. Validated zero RX means the recorded cursor pair was empty; it cannot prove that no data arrived afterward. Existing DATA, close/read, IPA, WLAN and WLFW outcomes remain visible.

Tests cover caller forwarding through the actual adapter gate, three-window clipping, RX metadata, unknown sites, capacity, history eviction and freeze. The full owner suite passes559 cases with one prior ignored test; the Normal-NC baseline suite passes554 with the same ignored test; link tests pass212. All24 paired replay schedules/48 executions/96 modes preserve exact FIFO I/O digests and peer events, and ten fault controls pass. The rendered panel verifies both framebuffers and non-overlapping small summary labels/data. These checks do not prove phone elapsed time or Normal-memory transient ordering.

The image profile is `.cache/sunfish-native-wifi/private/native-wrq-post-return-callers-v1`. It preserves the original IMG_9688 image and private evidence. The packed initializer inspector now takes its expected object count from the SHA-bound audited CapDL input, because instrumentation changes code-page count. The fixed eight NC table IDs/flags and complete loader/CapDL/report reproduction remain mandatory; wrong/missing expected-count controls are rejected.

This is a new attribution measurement, not another copy of the table-memory experiment or a claimed timeout fix. No phone commands are run automatically. After a fully powered-off entry into fastboot, use the temporary command supplied with the verified image and wait for the matching title plus **RESTART WHEN DONE**.

Verified image: **68,911,104 bytes**, SHA-256 `d7596d3d3efccee155ecd2ffb4d51498bc890be14f3c86b8a1d69f4eafcff2de`. The final authority audit rejects304 mutations; complete loader/CapDL/report reproduction passes, and the packed archive has24,538 objects with the same eight intended NC mapping IDs/flags. The pre-boot manifest records no device run.

Manual cold temporary boot, after fully powering off and entering bootloader fastboot:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-wrq-post-return-callers-v1/fastboot.img
```

Wait for **WRQ POST RETURN CALLERS** and **RESTART WHEN DONE**, then photograph the entire settled panel.
