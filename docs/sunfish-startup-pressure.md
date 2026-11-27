# Adaptive startup pressure display

[IMG_9803](sunfish-elf-pool-release-IMG_9803.md) followed the slow transaction3/unpublished-peer path, while IMG_9802 exercised the ordinary full transmit pool. The existing producers retain both alternatives, but choosing just one detailed panel loses the other when arrivals change. The next image changes presentation only.

Feature`sunfish-startup-pressure` inherits`sunfish-elf-pool-release` and every prior observer/policy. If `owner.elf_tx_slots()` returns an actual retained capture, the unchanged eight-slot display runs with title **STARTUP POOL PRESSURE**. Otherwise the unchanged transaction3/retry display runs with title **STARTUP REPLY PRESSURE**. Wait counters or timestamps do not choose a view. A clockless capture still selects the pool view. Early startup can change to the pool view when a real rejection later occurs; photograph the settled page.

The reply view retains all frozen transaction3 publication calls/mask, ordinary priority gates0–7, borrowed grant gates0–B, ACK/return/advertisement counters, converted-grant clocks, independent transaction4 timeline/snapshot, first ELF latest-failure owners, exact Core receipts and successful admission, Owner receipt/work-return/registry-entry/selection spans and actual consumed-slot identity. Its detailed field definitions remain in [the transaction3/retry note](sunfish-tx3-elf-retry.md). Row2 IPA is the live flags word, while its final IPA is historical completion. FFFFFFFF is unknown/unavailable, not zero.

The pool view retains every first/latest slot pair and first-release/admitted metadata from [the pool display](sunfish-elf-pool-release.md), with its original row meanings. In that view row2 IPA is historical completion. The different titles explicitly distinguish the two schemas. Slot rows0–7 remain ordered pairs FIRST then LAST. Neither view establishes remote application acceptance, missing intermediate events, or the private WLAN launch condition.

Four gated presentation blocks (three in the transaction3 renderer and one in the pool renderer) and one Cargo line are the entire production delta. Removing them recovers all parent Rust sources at`ca12cc96` exactly. Core, Adapter, Owner, FIFO validation, service budgets, fitting-intent selection, own RX_DONE prerequisite, grant advertisement-before-ACK and QRTR credit checks remain byte-identical. No producer, clock, validation call, queue attempt, earlier retirement, new permission or scheduling exception is added. Display code/layout can still perturb wall timing. Both earlier diagnostic features retain their earlier selection when the child feature is disabled.

Offline tests run the actual selected renderer and both physical framebuffers through no-pool reply mode, genuine captured-pool mode, transition between views, changed first/latest identities, distinct first-release/admitted slots, zero/missing/clockless values and width bounds. A nonzero capacity wait counter without a typed capture does not select pool mode. All parent Rust source is compared after normalization. Prior renderer tests, source-boundary tests, complete Owner/Core/IRQ suites, selected native stack, fresh capability rejection audit, full boot-layout checks and actual packed Normal-NC payload reproduction bind the delivered image. The pinned SDK is reused, not recompiled.

This image should distinguish the current writer gate and retry/service delay if the slow path recurs, while preserving useful capacity evidence if that path recurs instead. It is not a timing fix or a promise of WLAN startup.

Fully power off, enter the bootloader with Volume Down + Power, then manually run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-startup-pressure-v1/fastboot.img
```

Wait at least one minute, then send the full settled **STARTUP REPLY PRESSURE** or **STARTUP POOL PRESSURE** panel. No automatic device command or partition flash is performed.
