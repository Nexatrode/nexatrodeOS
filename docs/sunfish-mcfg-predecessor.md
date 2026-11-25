# Follow the unsent MCFG predecessor and grant context

IMG_9764 identifies an earlier, wholly unsent same-peer TFTP ERROR (`0x1d`, length 52, sent 0) as the last queue blocker for original MCFG response `0x1e`. The target later had a fitting, ready remote buffer, but its only recorded Core writer pass committed RX_DONE `0x56` while a negative grant ACK was pending. The target was then canceled. The earlier synthetic fully-sent predecessor explanation does not describe that last phone snapshot.

The existing full-pool-denial replay shows why a receive return can be necessary before a fresh advertisement and positive ACK. It checks the committed return, a blocked advertisement retry and advertisement-before-ACK ordering. The photo does not expose retry-full/yield state or denial size, so it cannot identify that branch uniquely or justify moving the target ahead of it. The 27.707 ms to host-observed close is not a proven FIFO or modem deadline.

The opt-in `sunfish-mcfg-predecessor-trace` extends the existing observer without changing Core/FIFO/Owner sources, writer arbitration, grant ordering, QRTR credits, native validation, file semantics or close handling. All existing observation entry points remain unchanged. There are no added transport-register/FIFO accesses; existing optional clocks and fixed host-memory scans may perturb timing.

On the last failed queue attempt the observer copies the blocking record's full existing reservation identity, separate from the target's identity. Before target admission, writer capture can follow that exact unsent predecessor, including actual publication and matching real completion. After admission it follows the target as before. A different reservation reusing the slot cannot acquire those observations. If the last failure changes to a different blocking identity, predecessor publication/completion clocks reset; each retained writer record displays its own sequence, so records from different identities remain distinguishable. A predecessor already published before capture gets no fabricated publication timestamp. Clocks can be absent independently of identity and flags. Cancellation/reset retain the previous isolation rules.

Panel **MCFG PREDECESSOR TURN** uses the same eight-entry first-four/latest-four writer capture. Row 9 is `BLOCK PUB DONE PASSES`, retaining last failed-attempt time and predecessor publication/completion. Row 13 packs independent ELF host-credit requested/published/completed in bits 0..2, followed by MCFG file presence at dispatch and server result. Row 27 retains read total/mask and writer passes/omissions. Original final ACK, ELF clocks, IPA pending/failure/completion, close commits, file progress, WLAN and WLFW remain visible. The filtered packet journal remains collected but is not displayed.

Each writer's first line is `start end gates command`. Its second line is `parameter QRTR-kind source destination result last-grant-size sequence-low sequence-high`. The original gates retain their meanings in `sunfish-mcfg-queue-writer.md`. New bits 10/11/12 mean retry-full-intent, intent-ACK yield, and predecessor phase; low 16 bits are entry state, high 16 bits are callback/exit state. Bit 25 identifies selection of the exact tracked record for that phase. A predecessor-marked record must not be described as a writer attempt for the later target. The displayed sequence names the watched record for that phase; it names the selected packet only when the exact-selection bit is set. The last requested grant size is diagnostic context, not proof that a grant is currently pending; the ACK gates disambiguate that. None of these observations authorize publication.

Offline cases cover an unsent predecessor through its real publication/completion and then the original target; a predecessor already sent before capture without an invented publication clock; and foreign slot reuse after cancellation. Existing writer/identity, backpressure, faults, reset and clockless cases remain active. Strict source checks compare the complete Core, FIFO, Owner, file, credit, runtime and existing ELF-observer files with `39860731`. The actual Core denial replay and on/off committed-byte comparison accompany the image. Native stack, capability and packed SDK/payload verification are required before readiness.

Verification: current/prior Owner suites passed 624/621 and current/prior IRQ suites 659/656, with one existing ignored test in each. Core suites passed 294/294. Three predecessor cases, five prior writer cases and six identity cases passed, along with two new source checks and the real full-pool-denial replay. Both deliberate predecessor regressions failed as required. The on/off unsent-predecessor/target replay committed the same 584 bytes. The selected native nested-frame budget leaves 820,480 bytes of its 5 MiB stack; this excludes TLS, indirect calls and other descendants and is not whole-program worst-case stack analysis. The pinned previously rebuilt SDK is reused while the native program is rebuilt; fresh capability and packed-payload checks accompany readiness.

This is a diagnostic extension, not a new priority or file-response policy. The saved successful Linux boot requests WLAN before its MCFG/digest probes; this exchange remains unproven as the initial WLAN startup prerequisite. No Pixel commands run during preparation.

Manual cold temporary boot after verification reports ready:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-mcfg-predecessor-v1/fastboot.img
```

Wait at least one minute and photograph the full settled **MCFG PREDECESSOR TURN** panel.
