# Exact ELF prequeue phases, admission owners and first capacity release

[IMG_9798](sunfish-post-efs-elf-writer-IMG_9798.md) puts **10,652.966 ms before ELF queueing**, followed by a clean first writer pass: 61.315 ms queue-to-publication and 44.193 ms publication-to-completion. The previous boot had 790.295 ms prequeue and 4,998.402 ms queued. The current writer screen cannot name the prior admission blocker. It is not evidence that the earlier queued stall is fixed or that all prequeue time is CPU work.

## What the saved evidence already distinguishes

- [IMG_9701](sunfish-registry-pressure-IMG_9701.md): repeated grant-control work held an older registry reply unpublished. Transaction4 and ELF waited behind it. Suitable remote intents and successful controls were recorded, not a selected write stuck in FIFO backpressure. Its owner identities and counts belong to that historical boot.
- [IMG_9717](sunfish-elf-window-IMG_9717.md): first rejection had transaction4 unpublished; the latest had two fully published owners. Exact receipt3 made room, but no admission attempt occurred until after receipt4. That motivated the existing bounded first-receipt continuation, subsequently demonstrated in [IMG_9718](sunfish-elf-first-slot-IMG_9718.md).
- [IMG_9724](sunfish-elf-third-phases-IMG_9724.md): the ordinary eight-slot pool, reason2, blocked admission even though the two earlier registry replies satisfied the current third-reply policy. The accepted-to-receive-end phase was mostly accounted for by measured I/O within that enabled scope. The phase-I/O counters did not cover every later dispatch and cannot stand for total elapsed work.
- [IMG_9725](sunfish-elf-tx-slots-IMG_9725.md): five published owners and three queued packets filled the pool. The first freed slot was refilled by another packet before ELF tried successfully. There was no already-GLINK-complete owner waiting only for local QRTR retirement to release speculatively.
- [IMG_9726](sunfish-elf-capacity-retry-IMG_9726.md): the bounded capacity continuation admitted ELF in the first actually freed slot, preserving the other owners. It did not remove the real wait for that release or demonstrate WLAN startup.

These mechanisms are distinct. The current 10.653-second interval cannot be assigned to one by substituting slot numbers, peer ports, transaction timing, or request patterns from a different boot. Current IMG_9798 has one captured EFS request; IMG_9797 exercised the exact second-request overlap and both real completions before close. Keep that verified guard, and do not infer an EFS regression from the missing second request.

The successful saved Linux journal still reaches WLAN firmware before the later MCFG read. tqftpserv supplies a connected transfer socket and an explicit missing-file ERROR; Linux/Lineage GLINK maintains real intent ownership and matching returns, while QRTR credits are separate. The earlier local launch-path audit supplies no source-backed speculative WLAN launch RPC. Nothing in this comparison authorizes early buffer retirement, a larger queue, fabricated completion, bypassed credit or a firmware restart. Separate kernel, socket and journal clocks are not combined into one timing trace.

## Incremental change

`sunfish-elf-prequeue-owners` inherits `sunfish-post-efs-elf-writer` and replaces only the panel with **ELF PREQUEUE OWNERS**. All relevant producers already exist in the current selected feature closure: Owner receive/dispatch/select timings, preselection runs, exact Core admission/receipt window, and opening/latest full-pool/first-release snapshots. This change adds no probe hook, clock call, shared-FIFO read, counter, queue attempt, writer turn, service handoff, admission rule or lifecycle mutation. No trace field authorizes I/O. Display formatting may still perturb wall-clock timings.

The only existing source edited is the previous read-only renderer: two gated blocks declare and route to the new child renderer. Removing those blocks reproduces the source at `737501ff` exactly; Cargo adds one child feature. Runtime, Core, Owner, FIFO, existing ELF producers, grants, credits, faults, close cleanup and EFS admission are unchanged. The integrated main desktop/wallet work precedes this baseline and is retained.

## Panel decoding

Numbers are hexadecimal. Times are converted microseconds since runtime start; unavailable/clockless times are `FFFFFFFF`. Zero-valued intent IDs are valid. Missing records remain unknown; counts are not elapsed-time partitions. Data rows contain at most six eight-digit values, within both physical framebuffers.

| Row / label | Meaning |
| --- | --- |
| 2 ERROR DETAIL IPA PENDING CLOSE STAGE | Error, detail, live IPA flags, pending IPA message ID, pending MCFG-close port, cleanup stage. |
| 3 ELF RX QUEUE PUB DONE | Exact first accepted ELF request, normal queue success, full publication, matching Owner completion. |
| 4 RX END REG ENTER SELECT PASSES | End of the receive pass after acceptance; first registry dispatch after acceptance; first selection of exact ELF; retained queued-window writer-pass count. Subtract these boundaries and queue time to distinguish receive-tail, entry, preselection, and postselection/admission waits. |
| 5 TX3 Q PUB TAIL DONE RXVIS AHEAD | Existing named predecessor queue/publication, sampled peer consumption, Owner completion; first matching receipt visibility and bytes ahead. These are different observation sites, not modem application-acceptance timestamps. History-eviction/absence fields are not displayed here, so no lossless FIFO reconstruction follows. |
| 6 TX4 RX QUEUE PUB DONE | Existing exact transaction4 timeline. These ages can identify whether the later request waited for an earlier publication or completion. |
| 7 WAITS NONE PEER TXCAP FLOW CREDIT CAP | Owner-recorded reasons0..5: absent/unknown pressure, same-peer gate, no ordinary transmit slot, legacy flow capacity, reference-flow busy, reference-flow capacity. These count failed exact ELF attempts, not every service loop. |
| 8 PRESELECT CALLS OMIT RUN0..3 | Completed registry dispatches before ELF is first selected, omitted run observations, first four compressed runs. A run is `transaction<<16 | name_code<<12 | outcome<<8 | count`. Outcomes1 queued,2 FlowBusy,3 NoCapacity,4 other error. With a recorded selection clock, runs stop at first exact ELF selection and do not account for later failed ELF admissions. An unavailable producer clock can leave that boundary unrecorded, so clockless run scope must not be assumed. |
| 9 / 11 FIRST / LAST FAIL | Failure time, `sequence.min(0xffffff)<<8 | reason`, same-peer owner count, derived gate mask. The actual reason determines which gate rejected; the derived mask summarizes occupants and is not authority. |
| 10 / 12 OWNERS A / B | Up to two retained same-peer entries, each `transaction<<16 | flags<<8 | kind`, followed by full remote intent ID. Count may exceed retained entries. Missing entry is FFFFFFFF. |
| 13 ADMIT US SEQ N GATES LASTRX TRIES | Exact successful Core admission time, observation sequence, remaining earlier same-peer count, derived gates, last retained receipt transaction before admission, total admission attempts. Owner queue time is a later boundary. |
| 14 RX0 RX1 / 15 RX2 RX3 | Two receipts per row, each three values: time, packed transaction/state, full remote intent ID. `TXNSTATE=transaction<<16 | remaining<<8 | target*2 | matched_last_failure`. The match binds the real prior opaque request, not a reused numeric intent alone. |
| 16 RECEIPTS OMIT SEQ0..3 | Total receipt count, omitted middle count, full observation sequence of each retained record. Compare these sequences with admission, including in clockless captures. First two/latest two receipts are retained after overflow. |
| 17 POOL FIRST LAST FAILS CHANGED ADMIT END | Opening/latest reason2 snapshots, count of full-pool failures, changed-slot mask, exact pool observer's successful admission time, finished flag. Absent pool stays unknown; it is not evidence of eight occupied slots. |
| 18..25 SLOT META SRC DST ID LENSENT TXNMSG | Latest blocked slots0..7 in order. Full source/destination ports and intent ID; length high16/sent low16; QMI transaction high16/message low16. Metadata is kind high8/flags next8/op low16. Nodes for these eight rows are not displayed; the underlying capture retains them. |
| 26 FREE US SLOTCAUSEMASK META SRC DST ID | First actual ordinary-slot clear after opening full capture: time, slot high16/cause next8/empty mask low8, original opening owner's metadata, full ports and intent ID. This is the opening identity, not the later occupant in the latest snapshot. |
| 27 FREE LEN QMI NODES ADMIT SLOT EMPTY | That original owner's length/sent, transaction/message, full source/destination nodes; ELF's ultimately assigned slot and post-admission empty mask. The first freed slot and final ELF slot need not coincide. |
| 28 IPA PD WLAN WLFW ACK PAIR | Historical IPA complete, host PD-query state, WLAN firmware RRQ count, WLFW servers, original final ACK completion, retained EFS overlap pair flags/admissions/offers. |

Same-peer entry flags are started1, fully published2, GLINK-done4, local QRTR retirement8, remote slot16, reference token32. Derived gates: multiple owners1, non-registry owner2, different local endpoint4, unpublished8, no remote slot16, GLINK already done32, local QRTR incomplete64, missing token128. **Multiple owners alone does not establish a rejection under the inherited third-ELF policy.** Reason1 identifies actual same-peer rejection; reason2 follows that gate and occurs before reference-credit reservation, so it does not prove credit was available at the failed check.

Slot kinds:1 QRTR RESUME,2 QRTR control,3 RMTFS,4 registry,5 other QMI reply,6 TFTP,7 IPA,8 other native client. Slot flags differ from the entry flags: occupied1/started2/full4/GLINK-done8/local-QRTR16/remote-owned32/reference-token64. TFTP op is the big-endian opcode; QMI op is its kind; other op is the QRTR type. Non-QMI transaction/message is FFFFFFFF. An ERROR opcode alone does not identify filename or error code.

First-free causes:1 exact GLINK retirement,2 matching QRTR RESUME retirement,3 validated unpublished firmware-close cancellation,4 completed RMTFS close cleanup,5 queued TFTP retry cancellation,6 unpublished original-OACK cancellation. All are observations after existing valid release paths; none grants permission to retire a still-owned buffer. Opening/latest captures omit intermediate pool states; first free can precede the latest full capture if another packet refills it.

Live IPA flag bits0..9 are modem-ready/uC-ready/indication-requested/indication-sent/protocol-complete/failed/pending-record/record-queued/held-response/initial-boot. Pair packs flags low8/admissions next8/offers high16; its existing flags are enabled1/first2/second4/exact8/original-bound16/closed32/second-queued64. The settled live state does not reconstruct earlier startup delays.

## Verification and cold temporary boot

The actual renderer test covers distinct first/latest owner states, two-owner admission reasons, exact receipt identities/sequences, all eight distinct slots, full-width ports/intent IDs, a changed slot with a different original first-free identity, missing/clockless data, and both physical framebuffers. Source-boundary checks cover unchanged producers and protocol paths. Full selected/prior Owner, Core and IRQ suites, inherited grant/credit/backpressure/retry/close/fault schedules, packet parity and negative controls remain required. Native selected-stack, 304 capability mutations, actual packed Normal-NC archive mappings and loader/report/CapDL reproduction bind the final delivery. The pinned SDK is reused, not recompiled. No handset timing is proven offline.

Profile `native-elf-prequeue-owners-v1`. After fully powering off, enter bootloader using Volume Down + Power and manually run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-elf-prequeue-owners-v1/fastboot.img
```

Wait at least one minute and send the full settled **ELF PREQUEUE OWNERS** panel. The discriminator is which prequeue phase and real admission owner/gate held this boot, and which normal release or dispatch made queueing possible. It may differ from the prior boot; a shorter delay is not itself proof of a fix. No device action or working WLAN is claimed before the boot.
