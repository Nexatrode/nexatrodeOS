# Bounded registry reply window after IMG_9697

IMG_9697 completed the original server-check DATA/final-ACK path and showed transaction4 last blocked by a fully published transaction3 reply (`reason1`,kind4,flags0B). The flag0x08 is the reference SERVREG local retirement bit; it does not establish a received QRTR RESUME. The panel did not distinguish a remote-late RX_DONE from a completion already in the FIFO but not yet handled. Its7.220-second registry-entry-to-ELF-selection interval is not a measured7.220-second remote wait.

The next profile reuses existing `sunfish-wlan-registry-second-reply` rather than forcing completion or releasing an outstanding intent. It permits at most two same-peer ServiceRegistry replies from the same local endpoint. The older reply must already be fully published, still own its remote slot and reference-flow token, lack matching GLINK completion, and satisfy the existing local retirement condition. The next reply still needs a free transmit slot, valid encoded QMI, a separately available remote intent for publication, and real reference-flow credit. A third reply waits. Each completion retires only its matching request/intent. The earlier bounded original final-ACK turn remains enabled.

The new `sunfish-registry-window-trace` feature observes actual successful second admissions, not eligibility attempts. A fixed first pair matches older transaction3/newer4 to peer0:9 and retains opaque owner/generation/request identities rather than TX-slot indices. Queue, full-publication and exact older-completion timestamps share the runtime clock. `PUB BEFORE DONE` captures event ordering independently of a clock:1means newer4 published before older3 returned;0means older completed first;FFFFFFFFmeans no matching full-publication proof. Reset clears the capture. No observer field controls admission, scheduling or retirement.

The `REGISTRY WINDOW TWO` panel also retains transaction4/first-ELF queue/publication/completion, original final ACK and exact peer0:1f close visibility, IPA completion, WLAN RRQs and WLFW server counts. An ELF completion is a service-registry locator reply, not a WLAN firmware transfer. The panel is bounded to27 fields and both actual framebuffers were checked.

## Source and trace comparison

Saved Linux locator events identify successive avs/audio, tms/servreg and kernel/elf_loader lookups. Matching by order and reply size, the one-domain reply returns its GLINK intent86us after TX and the six-domain reply201us afterward. The88-byte ELF reply returns after80us. Those are ftrace intervals; journald locator timestamps are not interchangeable clock boundaries. Native transaction3's service name was not captured on IMG_9697, so its identity is not inferred solely from transaction number.

The domain mappings and pinned avs/audio reply fixture match the local reference source. The focused pinned Linux C `qrtr_tx_wait` oracle and ten pure Rust reference-flow tests pass. This focused runner does not claim the original broader script passed: its unrelated strict compiler/deprecation failures are preserved in the offline comparison directory. Current full candidate and prior-profile owner/link suites are separate validation.

The saved references do not prove why native transaction3's completion was late. This experiment tests whether bounded permitted concurrency removes head-of-line waiting without inventing a receipt. A firmware request, successful firmware transfer, WLFW startup and usable network traffic remain hardware milestones.

## Offline verification

Current owner suites:569passed/1priorignored with the window and574passed/1priorignored with the previous policy. Current link suites:221passed with the window and220passed with the previous policy. Five earlier tests explicitly describe a single outstanding reply and remain enabled for the previous policy. Their assumptions are replaced on the candidate by actual3/4/5 startup-chain tests, rather than weakened assertions.

The startup-chain owner test covers clock present/absent and completions in either order, with distinct intents and unchanged bytes for the still-outstanding reply. Core tests cover at-most-two, no admission behind an unpublished reply, actual credit saturation, exact blocked-write retry, advertisement-before-positive-ACK, both receipt orders and current fail-closed non-TFTP client-close behavior. Closed-peer published buffers remain owned under quarantine; this change does not silently convert that safety path into graceful retirement. Additional observer tests cover failed admission, absent clock, completion before/after publication, unrelated newer completion and reset.

Twenty-four paired saved pressure schedules preserve exact full I/O and peer-event order. These schedules do not establish registry-window overlap or invoke the runtime final-ACK boundary: the dedicated startup owner/core tests establish the new window lifecycle, and the existing four bounded ACK boundary schedules establish that retained runtime behavior. Fake peer schedules are not handset timing predictions.

The temporary host check runner initially omitted the established16MiB test-thread stack and aborted in large fixtures; it was corrected to the same stack setting as the full suites. This is not a newly observed device stack fault. The selected native construction/begin-transfer stack audit measures4,293,760bytes in the existing5MiB mapping, leaving949,120bytes. This regression budget excludes TLS, indirect calls and other descendants and is not whole-program worst-case stack analysis.

Final capability, packed initializer, payload/source hash and delivery results are recorded below only after completion. All output artifacts and source snapshots remain in the private profile; modem firmware and packet payloads are not added to Git.

## Manual cold temporary boot

After the delivery checks finish, fully power off the Pixel, then hold Volume Down + Power for cold bootloader fastboot. From the repository root:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-two-replies-v1/fastboot.img
```

Wait for `REGISTRY WINDOW TWO` and `RESTART WHEN DONE`, then send the whole settled panel. This is temporary boot, with no partition flash. No device command was executed while preparing it.

## Verified delivery

Profile `.cache/sunfish-native-wifi/private/native-registry-two-replies-v1` is ready for manual cold temporary boot. Image size68,911,104bytes; SHA256 `877cc9ec1fcc1850cd18d940b9f00806776ecc8b3492e5075379c28b0fea2c2d`.

The final audit rejected304 capability mutations. Actual loader/CapDL/report reproduction is byte-identical using the unchanged pinned SDK; the real packed initializer archive contains24,538objects and exactly eight audited RW/NX/uncached Normal-NC table maps. Correct object count passes; wrong count and missing count fail. All3,637 named protocol frame bindings were compared by name:544 moved after image growth, with permissions/authority checked independently by the final audits. The AArch64 validation helper still uses paired loads over all4,096bytes, checks both words, enforces null/alignment guards and has no helper call. Native4096-word corruption and fault controls pass in both feature profiles.

The actual PD text is present in the loader, which matches the wrapped/LZ4 Android boot image; boot-layout and source/input/artifact hashes pass. Caller-source snapshots and safety function body hashes are current. Previous delivered images retain their hashes. Self-review found no remaining important issue; no independent agent review was performed for this change. Private `manifest.json` binds the final image and evidence, and `git-revision.txt` records the committed source checkpoint. There has been no handset test of this image yet.
