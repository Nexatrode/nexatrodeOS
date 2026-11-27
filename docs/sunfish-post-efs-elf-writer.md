# Post-EFS startup and the exact ELF writer window

IMG_9797 demonstrates the bounded second-EFS admission working: both exact ERROR replies complete before their shared client closes. The second queues 633.963 ms before the first completion, publishes 295.186 ms after acceptance, and completes 692.389 ms before close. Retain that change and its ownership guards. Neither a WLAN firmware request nor WLFW appears. See [the saved result](sunfish-efs-reply-overlap-IMG_9797.md).

The next unresolved interval is the exact ELF reply: accepted `014A93D7`, queued `0156A2EE`, published `01A2E7F0`, completed `01A7A125`. That is 790.295 ms before queueing, **4,998.402 ms queued before publication**, and 309.557 ms to matching completion. The EFS panel did not display the already-retained writer choices. It cannot assign this queue interval to an intent shortage, a grant, other packets, or service-loop work, nor establish it as the cause of missing WLAN startup.

## Offline comparison and limits

The saved Linux successful cold journal places server-check WRQ at 177.023110 s, ruleset/EFS RRQs at 177.039398 s, WLAN firmware RRQ at 178.730030 s, and MCFG RRQ at 187.027163 s. Missing ruleset/EFS files receive explicit ERROR1 responses. That journal contains one EFS request, not an equivalent second same-peer request. Native's exact two-request overlap result therefore resolves a native serialization problem; it is not a byte-for-byte reproduction of that Linux journal.

Linux tqftpserv opens a connected QRTR socket for each transfer and replies to missing files. GLINK receive intents and exact RX_DONE ownership are handled below userspace; QRTR credits remain a separate gate. Saved locator traffic shows a sub-millisecond receive/reply/receipt exchange, but its association uses names, size, and ordering rather than a complete exact QMI payload. The locator capture, userspace socket trace, and cold journal are separate clocks/boots and must not be spliced into one causal timeline. The pinned Linux/Lineage launch-path audit supplies no source-backed speculative WLAN launch RPC. Firmware-private startup decisions remain unobserved.

Existing notes already distinguish full-pool admission from queued publication. IMG_9725/9726 show actual in-flight owners that must retain their buffers until real matching receipts; no speculative retirement follows from a QRTR credit. Those older slot identities do not identify current owners. Here ELF already passed queue admission before the five-second interval.

[IMG_9718](sunfish-elf-first-slot-IMG_9718.md) had a 4,139.576 ms queued interval. The next observation-only boot, [IMG_9719](sunfish-elf-writer-IMG_9719.md), published in its first writer pass, 93.500 ms after queueing. Thus the new boot may not reproduce a five-second wait. A clean writer result is useful evidence and must not be presented as explaining the earlier stall.

## Change: display existing records only

`sunfish-post-efs-elf-writer` adds **POST EFS ELF WRITER**, a read-only renderer routed ahead of the existing startup panel. It inherits the verified EFS overlap profile. No Core/Owner/FIFO producer, queue admission, grant ordering, QRTR credit, receive lifetime, service handoff, writer scheduling, validation, fault handling, clock hook, or shared device read changes. Removing the two feature-gated renderer routing blocks reproduces the prior source exactly. Display formatting can still perturb wall-clock timing.

The existing writer capture binds the exact accepted first ELF request, starts after normal queue admission, retains the first four/latest four completed records, and freezes at full publication. The ordinary success path appends the final pass record before commit freezes the trace. Uncertain/error paths may retain an active entry without a completed record; the new screen exposes the existing active flag rather than fabricating an end or callback result. No new producer or current-record accessor is added.

## Panel decoding

Values are hexadecimal. Ages and durations are microseconds after conversion; missing or clockless times are `FFFFFFFF`. Counter zero remains distinct from missing time. Each data row has at most six eight-digit values and fits both physical framebuffers.

| Label | Values |
| --- | --- |
| ERROR DETAIL CLOSE STAGE | Runtime error, detail, pending MCFG-close port or FFFFFFFF, cleanup stage. |
| ELF RX QUEUE PUB DONE | Exact first ELF accepted, queued, fully published, matching Owner completion. |
| EFS1 PUB DONE CLOSE FLAGS | First EFS publication, completion, client close, retained stages. |
| EFS2 PUB DONE CLOSE PAIR | Second EFS same clocks; pair packs flags in low byte, admissions next byte, offers high16. |
| BODY CREDIT IPA FLAGS PENDING | ELF reference-body match; host-credit mask requested1/remote-intent-present2/completed4; live IPA flags; pending IPA message ID. |
| POST PATH OP PEER BYTES | First postmatching-ELF TFTP request: path kind, opcode, source peer port, body bytes. Path1 server-check,2 MCFG,3 ruleset,4 EFS,5 WLAN,6 other,7 unknown. |
| QUEUED TURN ENTER END FLAGS TX | Existing bounded queued handoff entry/end, flags, ordinary publication-progress count. Flags armed1/consumed2/entered4/returned8/exact ELF published by end16/failed32. |
| WRITER FIRST LAST PASSES OMIT | First Core writer entry, last completed observation, total entries, omitted middle records. |
| WRITER US GAP TIMED FLAGS | Sum of timed pass spans, maximum prior-end to next-entry gap, timed count, flags finished1/active2/regressed4. |
| PUB CALLS MASK TXN BYTES | Exact target's frozen publication callback count, accumulated outcome mask, QMI transaction, queued bytes. |
| TURN GATES / GRANT GATES | Existing first-failing-condition counts, indexed by the labels. |
| START END CMDKFTXN PARAM RESULT | Eight retained records: pass start/end, packed command/kind/flags/transaction, full GLINK parameter, actual callback result. |
| ACK SENT BLOCKED DONE ADS AGE | ACK commits, blocked ACKs, receive returns ahead, advertisements ahead, maximum pending-ACK age while exact ELF unpublished. |
| MCFG PEER FLAGS FILE RESULT CLIENT CLOSE | Existing first captured post-ELF MCFG peer/stages, file state before dispatch, server result, last client opcode, observed close. This is not necessarily the first TFTP request above. |
| IPA PD STATE WLAN WLFW | Historical IPA-complete latch, host PD-query state, WLAN firmware RRQ count, WLFW server count. |
| ORIG ACK READ MASK LOC Q DONE | Original writer's final ACK completion; read count high16/kind mask low16; queued/completed locator-name masks. |

Live IPA flags bits0..9 are modem-ready, QMI uC-ready, indication-requested, indication-sent, protocol-complete, failed, pending-record, record-queued, held-response, initial-boot. Historical IPA COMPLETE1 alone does not establish that every response has retired. If the report/protocol is absent, its live flags are unknown.

`CMDKFTXN=(command.min(255)<<24)|(kind.min(15)<<20)|((flags&15)<<16)|transaction`. Record flags fitting-intent1/exact-target-selected2/callback-blocked4/callback-uncertain8. Registry kind is4. Actual callback RESULT0 means no observed callback,1 committed,2 WouldBlock,3 Uncertain. Commands5/6/7/8/9 are intent advertisement/RX_DONE/intent request/grant ACK/DATA. A command without RESULT1 does not establish a committed write. No-header command0/parameterFFFFFFFF differs from a synthetic normal-refusal command0/parameter0. Missing records are all FFFFFFFF.

Priority turn gates0..7: cooldown, ready TFTP, pending intent ACK, own receive return, no candidate, different candidate, priority/credit unavailable, selected. Grant gates0..B: no pending ACK, denial, no backing, no yield/conversion eligibility, cooldown, TFTP, WRQ return, own return, opportunity already used, candidate mismatch, priority mismatch, selected. Counts aggregate all observed attempts; omitted middle records remain unknown.

Publication mask bits0..7: no suitable intent, outstanding request, control selected, other DATA selected, selected-but-blocked, successful DATA callback, outbound intent request selected, successful registry priority. Higher inherited experimental bits retain their existing meanings described in [the original writer capture](sunfish-elf-writer-publication.md). They do not authorize extra I/O.

Subtract ELF queue from first writer entry and last observed end from Owner publication to separate outer gaps from observed pass costs. Pass spans and maximum gaps are not exclusive CPU accounting; queued-handoff duration can overlap the writer and continue after publication. Do not add overlapping spans or assign an active unclosed callback to pure service-loop delay. GLINK receipt is buffer return, not remote application acceptance; peer-tail crossing is shared transport consumption, not application acceptance.

## Verification and next boot

The actual renderer is exercised with distinct opening/tail command/result records, missing and clockless fields, active/regressed flags, live pending IPA state, both EFS timelines, and both physical framebuffers. Source-boundary checks cover the unchanged runtime, transport, queue/flow, EFS admission, and existing writer producer. Full Owner/Core/IRQ suites, inherited ownership/grant/credit/backpressure/fault tests and negative controls remain required. The native selected-stack audit, 304 capability mutations, actual packed archive Normal-NC caps and loader/report/CapDL reproduction are recorded in the private delivery manifest. The pinned SDK is reused, not newly compiled. These are finite offline checks, not a hardware deadline proof.

Profile `native-post-efs-elf-writer-v1`. Fully power off, then enter bootloader with Volume Down + Power and manually run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-post-efs-elf-writer-v1/fastboot.img
```

Wait at least a minute and photograph the full settled **POST EFS ELF WRITER** panel. No automatic device command is performed. This compares the post-EFS/ELF window; it does not claim that WLAN or WLFW is now working.
