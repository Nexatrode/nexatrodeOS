# Current ELF pool ownership and first release

[IMG_9802](sunfish-tx3-elf-retry-IMG_9802.md) did not reproduce the earlier unpublished-transaction4 retry episode. Transactions3/4 used the retained converted-grant reply path. Exact ELF instead encountered three ordinary transmit-capacity rejections and admitted before either registry receipt, with both older owners retained. Its2.064-second prequeue interval cannot be partitioned from those counters alone.

## Offline review

The required capture already exists in `qcom_elf_tx_slots.rs`: opening/latest full eight-slot snapshots for the exact first ELF handle, the first actual ordinary-slot clear, and the eventual admitted slot. No additional producer, clock, FIFO access or scheduling exception is required. The settled IMG_9802 photo does not contain those records, and host source cannot recover their handset values retrospectively. A new boot can expose its own values; it cannot reconstruct IMG_9802's individual packet identities.

Normal `retire_transmit` requires both `glink_done` and `qrtr_done`, completes any router ticket/typed completion bookkeeping, and only then clears the slot and observes the release. Validated unpublished cancellation paths retain their separate restrictions. Matching RX_DONE alone is insufficient when the local QRTR retirement condition remains outstanding. Invalid receipts, WouldBlock and uncertain publication do not grant capacity. Published packets cannot be retired speculatively. Existing mixed-pool tests cover first release/reuse, unrelated and invalid receipts, GLINK-before-QRTR retention, later exact RESUME, blocked/uncertain writes, reset, clockless and frozen captures; the selected full Owner/Core suites include these cases and the bounded capacity retry.

The saved Linux reference `drivers/rpmsg/qcom_glink_native.c:qcom_glink_handle_rx_done` resolves the channel and exact remote intent, clears `in_use` on the real completion, and removes non-reusable intents or wakes reuse waiters. The saved Lineage kernel has the same exact-intent return boundary. Linux QRTR's `net/qrtr/af_qrtr.c` manages per-destination flow control separately through confirm/RESUME. Those sources support keeping completion and flow ownership distinct. Linux has no direct equivalent of this native fixed eight-record registry/TFTP/IPA pool, and the code does not identify the current handset occupants or the proprietary WLAN launch condition. No web lookup or device action was needed for this source review. Input hashes are retained with the private build evidence.

Earlier [IMG_9726](sunfish-elf-capacity-retry-IMG_9726.md) exposed a full-pool release in a different startup schedule. Its five/six published owners, filenames or first-free slot must not be imported into this boot. This image deliberately reuses that instrumentation alongside the current transaction3/4 and EFS changes; it adds no new scheduling fix.

## Incremental display only

Feature `sunfish-elf-pool-release` inherits `sunfish-tx3-elf-retry`. Two gated renderer blocks select **ELF POOL FIRST RELEASE**. Removing those blocks and the one Cargo feature line recovers the parent Rust sources exactly at `6f1d64bc`. All transport, Core, Adapter, Owner, Runtime service, validation, grant and credit source remains byte-for-byte unchanged. The older observer normalization also removes the child feature line before its existing baseline comparison. All prior optional observations remain selected; this increment adds none. Formatting, code layout and physical allocation can still perturb timing.

All values are hexadecimal. FFFFFFFF means absent/unavailable, not zero. Rows7–22 are eight pairs, slot0 through7 in order: white FIRST followed by cyan LAST. Each record contains:

`kind<<24 | flags<<16 | op`, full source port, destination port, remote intent ID, `length<<16 | bytes_sent`, `transaction<<16 | QMI message`.

Kinds:1 RESUME,2 QRTR control,3 RMTFS,4 registry,5 other typed QMI reply,6 TFTP,7 IPA,8 other client. Flags:01 occupied,02 started,04 full publication,08 GLINK done,10 local QRTR retirement satisfied,20 remote slot owned,40 reference-flow reservation retained. These flags differ from the two-owner admission flags. TFTP op is its big-endian opcode; QMI op is kind; other packets use QRTR type. Non-QMI transaction/message is FFFF. Intent0 is valid. The eight pairs omit node IDs; the first-free original has its full nodes and QRTR type separately.

| Row | Fields |
| --- | --- |
| 2 | Runtime error, detail, historical IPA complete, pending IPA message, pending MCFG close port, cleanup stage. IPA here is not the older live flags word. |
| 3 | TX3 queue/publication/Core receipt, TX4 accepted/queue/publication. |
| 4 | ELF accepted/queue/publication/Owner completion. |
| 5 | First/latest full times, failure count, complete-record changed mask, admission time, frozen/end Boolean. |
| 6 | First-free time, `slot<<16 | cause<<8 | empty_mask`, ELF admitted slot/remaining empty mask, existing completion-slot Boolean, separate capacity-retry flags. |
| 7–22 | FIRST/LAST records, slots0–7 in order. |
| 23 | First-freed index's original opening record, same six-word encoding. Its intent/sent values are from opening, not necessarily from release. |
| 24 | That original record's source/destination nodes, QRTR type, TX4 Owner completion, capacity retry's admitted slot. |
| 25 | Capacity retry armed/entered/ended clocks, result, arm count, full-skip count. |
| 26 | ELF wait reasons: none, peer, transmit capacity, flow, credit, other capacity. |
| 27 | Independent TX3 and TX4 snapshot states/calls/publication masks. State2 is frozen. |
| 28 | Historical IPA complete, PD state, WLAN firmware RRQs, WLFW servers, original final-ACK completion, EFS pair flags/admissions/offers. |

Release causes:1 exact GLINK receipt retirement;2 matching QRTR RESUME retirement;3 validated unpublished firmware-close cancellation;4 completed RMTFS close cleanup;5 queued TFTP retry cancellation;6 unpublished original OACK cancellation. These labels observe existing clear sites and never authorize them. The initial full pool means the first cleared index belongs to the opening owner. A later full snapshot can contain a different packet in that slot if another queue claimed it before ELF retried; compare the opening original, latest snapshot and eventual admission separately. A changed mask0 compares only those two samples and does not exclude intervening or later progress.

## Verification and next cold boot

The actual selected renderer runs against heterogeneous first/latest eight-slot fixtures, a changed/reused slot, the retained original first-free record, separate admitted index, absent and clockless captures, packed IDs/lengths, width bounds and both physical framebuffers. The preceding renderer still runs with the child feature off. Source comparison covers every parent `rust/src` file and Cargo feature removal. Native checks bind the rebuilt AArch64 text to loader and boot image, selected stack headroom, capability mutation audit, boot layout and actual serialized Normal-NC mappings using the pinned SDK. SDK reuse is not a fresh SDK compilation. Private verification logs and source/input hashes identify the exact delivered build.

This test should identify the packets filling the current queue, their publication/completion states, the first valid release and whether ELF used it. It does not promise a WLAN change or by itself partition every millisecond between observations.

Fully power off, enter bootloader with Volume Down + Power, then manually run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-elf-pool-release-v1/fastboot.img
```

Wait at least one minute and photograph the full settled **ELF POOL FIRST RELEASE** panel. This temporarily boots the image. No partition flash or automatic device action is performed.
