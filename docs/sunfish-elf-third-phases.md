# Third-reply ELF queue phases

[IMG_9722](sunfish-elf-third-IMG_9722.md) confirms the selected ELF reply admitted with two older replies still owned and published before both matching older completions. Its remaining receive-to-queue interval is 2,870.161 ms, while queue-to-publication is 92.876 ms. WLAN RRQ and WLFW remain zero. This does not establish the phase or blocker responsible for the residual wait.

Feature `sunfish-elf-third-phases` retains the verified third-reply policy and selects only a different display module. It reuses existing request acceptance, receive-pass-end, registry-entry/selection, first/last admission, older publication, receipt and native I/O snapshots. No new producer hooks, counters, clocks, device reads, grants, writer attempts, queue changes or retirement rules are added. Runtime method bodies and the complete Owner/Core/FIFO/native-I/O sources remain identical to the delivered third-reply source; only the display module path changes.

The panel **ELF THIRD QUEUE PHASES** includes:

- Exact ELF receive/queue/publication/completion and the existing queued handoff.
- Both earlier reply publications, transaction 3's FIFO observation and transaction 4's timing.
- Receive end, registry entry, first ELF selection and the four disjoint elapsed intervals to queue.
- First/latest rejected admission, both occupants' transaction/flags/intent/type, successful admission and exact receipt sequence.
- Three existing I/O counter deltas: acceptance to receive end, receive end to queue, queue to publication. Columns are **OBS / DESC / READ / OTHER**, in microseconds. These four categories are disjoint; their nested barrier/domain details are not separately added. Missing, regressed or saturated counters remain FFFFFFFF.
- MCFG target/result summary, original ACK and IPA/PD/WLAN/WLFW outcomes.

Reason1 is same-peer ownership pressure, reason2 normal TX capacity, reason3 legacy flow capacity, reason4 reference credit blocked and reason5 reference table capacity. Aggregate gate bits describe both occupants and do not prove which operand rejected admission. In the retained third-reply mode, gate mask `0x01` (two older owners) alone is permitted; either older unpublished/not-owned/incorrectly-routed owner still blocks. First/latest snapshots omit intermediate checks; elapsed selection-to-queue time is not proof of pure remote waiting. Counter snapshots cannot assign all non-I/O service work, and an optional clock's absence is not zero cost.

The renderer is tested against actual dual framebuffers, representative older owners, phase deltas, missing/regressed counters, absent captures and zero frequency. Existing protocol/fault/transfer suites and the third-admission negative control remain required. Native build, selected stack, actual capability mutation audit, boot envelope and packed initializer mappings are verified. The previously rebuilt pinned Normal-NC SDK is reused and reproduced; no fresh SDK compilation is claimed. Offline schedules and selected stack analysis do not establish all modem deadlines or whole-program worst-case stack use.

Cold temporary boot only:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-elf-third-phases-v1/fastboot.img
```

Fully power off before entering fastboot. Wait at least one minute, then send the entire settled panel. No rerun of the prior image is needed. This capture distinguishes the remaining prequeue phases under the now-demonstrated third-reply policy; it does not promise WLAN firmware or working Wi-Fi on this boot.
