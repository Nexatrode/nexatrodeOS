# First freed registry slot gives ELF a normal service turn

Follow-up to [IMG_9717](sunfish-elf-window-IMG_9717.md). Its exact receipt journal shows transaction3 freed one of two same-peer slots, then transaction4's receipt was processed before ELF was retried. This does not imply an unconditional wait for both completions. The earlier named-blocker handoff can miss a useful completion because it binds one historical blocker and a single pressure entry overwritten while walking transmits.

`sunfish-elf-first-slot-turn` replaces that handoff only in the diagnostic profile. After a normal validated registry receipt and normal retirement, Core retains one ephemeral `(opaque request handle, typed reply route)` witness. Every ingest, reset and attempted take clears it. The pending first ELF request remains Owner-selected by its real handle. A take requires the exact completed handle, live pending ticket and matching registry source/destination pair. Live same-peer metadata must show zero prior owners or exactly one fully published registry reply with its original remote slot/reference token and local QRTR retirement satisfied. An unpublished survivor, wrong peer/handle, stale event, invalid ingest or reset cannot lend the turn.

A successful take consumes one Owner lifetime exception and ends the current checked transport iteration. It retains the existing exhausted-budget continuation/IRQ behavior so the regular service pass can try ELF before another receive record. It does **not** queue or publish directly, bypass a pending FIFO record, change grant order, retire the other reply, reserve credit or skip any normal queue/writer/lifecycle checks. Fresh credit/capacity can still reject the ordinary queue attempt. Normal continuation is the fallback; there is no retry loop or device-clock deadline.

Four new Core cases exercise either receipt, an unpublished survivor, stale/wrong/invalid/reset witnesses, unrelated peer, fresh reference-credit rejection and blocked writer retry. The actual Owner case places both completions in the FIFO, verifies only the first is processed before returning, and admits ELF through ordinary service dispatch while the other original owner remains. Both completion orders are tested. Existing grant, close, fault, full validation and credit controls remain applicable.

The **ELF FIRST SLOT TURN** panel retains the prior admission/receipt/TX3 FIFO/TX4/MCFG fields. Row5 now reads **TRIES RECEIPTS OMIT SLOT TURN**; its fourth value is the consumed Owner exception (0/1), not proof that normal queue admission or publication succeeded. Compare the exact successful admission sequence/count with both receipt sequences. Queued-handoff flags on row4 are unchanged:0F means entered/returned,1F additionally means exact ELF published by its end. WLAN RRQ/WLFW are still the startup outcome, not inferred from ELF or MCFG success.

No new hardware read sites or optional clocks are introduced by this scheduling change. The previous observation still has its documented overhead and bounds. Host replays do not prove a handset deadline, every interleaving or WLAN startup. Pinned SDK reuse is checked against fresh native code and actual packed loader/capabilities; it is not a fresh SDK compilation.

## Verified delivery

Current/prior actual Owner suites pass590/589 tests (one pre-existing ignored case each); Core passes281/277; actual IRQ/Router613/612. Four new Core cases and the actual Owner two-order continuation test pass. Disabling only the continuation in a private negative control makes that Owner test fail as intended: two receipts are processed instead of one. The actual panel,24 paired Owner schedules, existing grant/credit/close/fault controls,304 negative capability checks and eight packed Normal-NC table maps pass. Synthetic Core and MCFG committed-byte comparisons preserve472 and816 bytes respectively; actual continuation scheduling is tested separately. The selected stack regression audit leaves853,744 bytes under its documented exclusions.

Image size **68,947,968 bytes**, SHA-256 `3e7d2c28e4dd5dae7b02c6bac593740360a89f4c422ffd50e648ae19b4ebde8c`. Native ELF SHA-256 `43448c0742d46ce86490f8c03fae1a71edeec458c49a30e625a209c54bc5d837`. The private manifest binds the final committed revision and verified artifacts. No device was booted during these checks.

After fully powering off and entering bootloader with Volume Down + Power:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-elf-first-slot-turn-v1/fastboot.img
```

Wait at least a minute and photograph the entire settled **ELF FIRST SLOT TURN** panel. The expected discriminator is an ordinary ELF admission after the first eligible receipt with one original owner retained; a blocked ordinary attempt can still fall back normally. This does not predict WLAN firmware loading or working WiFi.
