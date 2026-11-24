# Transaction 4 conversion lifetime after the REGISTRY POST REPLY result

The settled `8f485584` result establishes prompt consumption of transaction 3's reply (6.138 ms), late arrival of its matching RX_DONE (10,252.828–10,271.948 ms after publication), and another 647.595 ms before host processing. The tracked converted backing advertisement and ACK committed within 144.476 ms. Transaction 4 still waited 7,100.521 ms, with an armed conversion marker but no borrowed-callback attempt or commit. Its aggregate gates contained only no-ACK, denial, no-unadvertised-backing and no-yield observations. Those aggregates do not identify how the armed opportunity ended.

## What the offline review establishes

The exact conversion token is created only after a successful receive-buffer return converts a full-pool denial into a positive grant. It binds a fresh backing intent and opaque owner/generation/request identity. An ordinary registry candidate still requires its own RX_DONE, normal send ordering and a fitting modem intent; ready QRTR credits and protected TFTP/WRQ work retain priority. A borrowed callback spends the opportunity before the writer, including blocked, uncertain or unwound outcomes. The matching backing advertisement, an ACK or a subsequent conversion can also end the token. Reset destroys the window and token together.

A replay with transaction 3 still outstanding and both host-only registry entries occupied sends transaction 4 at the next ordinary entry when the token and ordinary conditions remain eligible. A separate own-request-return history sends the pending negative ACK first and never arms that same conversion; it is not a replay of the Pixel's armed marker. The saved private two-history replay records these alternatives rather than treating the fixture's original incorrect ordering assumption as a driver defect.

Seven dedicated current-Core tests cover ordinary success with/without clocks, blocked callback and byte-identical fallback, ready QRTR credit and advertisement-before-ACK, a bounded follow-up veto, uncertain write/reset, later-arm isolation, and unwind distinct from a veto. They verify the original transaction identities and keep transaction 3 outstanding where relevant. These schedules show how specific histories behave; they do not reconstruct the missing handset sequence or assign real milliseconds to fake clocks.

The Linux and Android reference handlers still establish the same advertisement-before-positive-ACK contract. They do not reveal which native publisher entry saw this token or which packet ended it. An observation is needed before another scheduling exception is justified.

## Observation only: first exact transaction 4 opportunity

`sunfish-registry-conversion-lifetime-trace` retains the first armed token for the exact bounded startup pair (transactions 3/4, peer 0:9). Later arms increment a saturating count without overwriting the first history. All latches and outcome matching work without clocks. The observer adds no device access or protocol exception and changes no FIFO, fit, credit, grant or reply-window checks. Optional timestamps and bounded metadata add overhead.

The first publisher entry is captured after the normal session/generation check and before receive maintenance or selector work. Raw flags describe that entry; bits 11–15 are filled later from the existing selector results. The actual gate is recorded by the existing grant-gate function. The first selected fixed record is retained, including an early outbound-intent request; a bounded follow-up may select a record and veto it before invoking the writer. Selection can therefore occur on a different call from the retained first entry. Uncertainty and unwind preserve the existing quarantine and cannot be retried as success.

The first token ends with a distinct reason. Matching grant advertisement/ACK observations can continue after that end without allowing an unrelated later grant to overwrite them. Publication counters still freeze at full transaction 4 publication, not at token end.

## Panel: REGISTRY TX4 TURN LIFE

There are 27 hexadecimal fields, followed by RESTART WHEN DONE. Timestamps are microseconds on the same runtime clock. `FFFFFFFF` denotes unavailable evidence; it is not a duration. ARMS zero means no matching conversion was observed in this boot, not that no receive-intent requests occurred.

| Field | Meaning |
|---|---|
| TX4 QUEUE / PUB | Existing transaction 4 queue and full-publication times |
| ARMS | Number of matching conversion opportunities; only the first history is retained |
| ARM US / BACKING / REQUEST | Reservation time, actual fresh backing ID and original local request intent ID |
| ARM LEN SENT | High 16 bits: full reply length; low 16 bits: bytes already sent at arming |
| ENTRY US / FLAGS / GATE | First checked publisher entry, state/selector bits and the actual first-condition gate |
| ENTRY BACKING / TOKEN REQ | Live backing and token request IDs at that first entry |
| CHOICE US / TXN CMD / PARAM | First selected fixed packet; QMI transaction high 16 bits (`FFFF` if absent), GLINK command low 16 bits, and actual command parameter |
| WRITE RESULT | 0 selected without callback; 1 committed; 2 WouldBlock; 3 uncertain; 4 callback entered without a returned result, such as unwind |
| EXIT US / CAUSE | First token end: 1 borrowed callback starts, 2 backing advertisement commits, 3 ACK commits, 4 another conversion replaces the opportunity; 0 no observed end |
| GRANT AD / ACK | Matching backing advertisement and positive ACK times, retained independently of the scheduling token |
| TX4 CALLS / MASK / SNAP STATE | Existing publication pressure; snapshot state 0 absent, 1 live, 2 full publication frozen |
| IPA COMPLETE / WLAN RRQ RX / WLFW SERVERS | Startup outcomes, without claiming that transport completion means firmware startup |

ENTRY FLAGS:

* Bits 0–5: ordinary entry; ACK present; ACK positive; live backing matches the first token; complete token identity matches; first backing still Advertising.
* Bits 6–10: own RX_DONE pending; registry cooldown; borrowed attempt already used; matching live transmit found; it is already fully published.
* Bits 11–15: any converted registry candidate; any registry priority candidate; protected TFTP; protected WRQ return; normal registry candidate exists but ready QRTR credit excludes priority.
* Bits 16–18: ordinary ACK-yield flag; fitting/owned remote intent found; unsent same-flow predecessor present.

GATE is the existing first-condition index: 0 no ACK, 1 denial, 2 no unadvertised backing, 3 no yield/exact conversion, 4 cooldown, 5 protected TFTP, 6 protected WRQ, 7 own return, 8 borrowed attempt used, 9 no exact normal candidate, 10 credit, 11 selected borrowed candidate. Earlier conditions hide later ones. A missing gate can mean the first entry exited before reaching this observation.

An EXIT of 1 marks an attempt, not a commit. CHOICE can remain the first vetoed packet even if a later entry borrows the reply; inspect WRITE RESULT, EXIT and TX4 publication together. The trace is deliberately bounded to the first opportunity and is not a complete packet history.

## Verified delivery

Private profile: `.cache/sunfish-native-wifi/private/native-registry-tx4-lifetime-v1`.

* Image size: **68,923,392 bytes**.
* Image SHA256: **d16c88eb4e6e4597c544c2899a4ebac53010fba3b7be2f1f80c07d6ebebdb748**.
* Current/prior Owner: **577/577 passed**, one existing ignored test each. Current/prior Core/link: **251/244 passed**. Seven dedicated lifetime tests pass.
* The actual renderer checks all 27 fields, no-clock/no-gate/unarmed cases, packed transaction/length fields, a veto versus callback result and both framebuffers. Twenty-four paired Owner schedules retain exact full I/O and peer-event order; this is finite control coverage. Existing uncertain write, advertisement, close, cursor, table corruption and domain-fault controls remain active. The pinned Linux C credit oracle and ten Rust flow-model tests remain passing reference controls.
* The native selected-stack audit measures **4,320,784 bytes**, leaving **922,096 bytes** in the 5 MiB stack. It excludes TLS, indirect calls and other descendants and is not whole-program worst-case stack analysis.
* The current CapDL has **24,542 objects**. Relative to the preceding image, 544 data-pool frames move 8,192 bytes higher and use generated physical-address symbols. The other 3,093 named protocol bindings, including all eight Normal-NC table frames and MMIO, remain pinned.
* Packaging binds the actual ELF text, loader, compressed Android boot payload, audit input hashes and 304 capability mutation rejections. Whole loader/CapDL/report reproduction and the actual packed initializer archive verify unchanged pinned SDK/tool components and eight Normal-NC maps. SDK components are reused, not claimed freshly compiled. Private manifests bind the exact source and output hashes.

No device command is executed by the offline checks. This is a diagnostic continuation of the successful early reply policy; it does not predict WLAN/WLFW will start.

Fully power off, enter the bootloader with Volume Down + Power, then run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-tx4-lifetime-v1/fastboot.img
```

Wait for **REGISTRY TX4 TURN LIFE** and **RESTART WHEN DONE**, then capture the entire panel.
