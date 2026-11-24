# Post-ELF startup outcome panel

The [offline comparison](sunfish-mcfg-post-elf-review.md) confirms the successful Linux cold boot reached WLAN while MCFG was absent. IMG_9711 identifies a ruleset read/error and MCFG reads/error before ELF completion, but its transaction-3 header window stops before ELF queues. Older MCFG-only post-ELF observations cannot supply this boot's missing sequence.

Optional feature `sunfish-post-elf-startup-panel` replaces only the final diagnostic renderer. It inherits the delivered TFTP identity / announcement-reuse feature closure. It adds no receive buffer, history, timestamp, device read, service-loop call, write turn, file contents or protocol policy. Existing GLINK/FIFO checks, grant advertisement-before-ACK order, credits, firmware capability and reply identities remain unchanged. Repaint deferral remains enabled. Rendering and changed native code layout can still perturb timings.

Panel title: **POST ELF STARTUP**. This is a bounded startup outcome summary, not a new complete packet capture.

| Field | Meaning |
| --- | --- |
| ELF R Q P D | Four small timestamps: accepted ELF request, reply queue, full publication, normal reply retirement |
| ELF BODY MATCH | Existing exact expected QMI response comparison: 1 matches, 0 differs, FFFFFFFF no observation |
| ORIG WRQ PEER / WRQ US / OACK US / DATA US | Original immutable server-check transfer identity and its receive/OACK publication/first DATA observations |
| ACK PHASE BLOCK | High 16 bits: original final ACK phase; low 16: final block or FFFF unavailable |
| ORIG ACK PUB US / ACK DONE / CLOSE US | Original final ACK publication, matching GLINK completion and first accepted exact peer close |
| WRITES DONE PEER FLAGS | Three small values: completed server-check count, first locally completed write's peer, its final ACK flags |
| POST ELF PATH | Path category of first RRQ/WRQ accepted after ELF's matching GLINK completion |
| POST OP PEER DST LEN | Four small values: that request's opcode, modem peer port, host destination port, TFTP body bytes |
| ELF COMPLETED | Existing exact ELF transport-completion latch |
| MCFG W FILE READ | Three small values: MCFG write-session count, committed RAM file length, completed read count |
| READ COUNT MASK | High 16 bits read-event count, low 16 bits cumulative read-kind mask; saturating packing |
| POST QRTR META SRC DST | First four complete incoming QRTR header projections following ELF GLINK completion, in capture order |
| READ US PEER KIND | First five global service-read observations: accepted time, peer port, read-kind number |
| QUERY PHASE / PD QUERY STATE | Existing one-shot WLAN domain query, including valid UNINIT 7FFFFFFF |
| IPA COMPLETE / WLAN RRQ RX / WLFW SERVERS | Separate protocol and discovery outcomes |

Original ACK phases are 0 absent, 1 prepared, 2 queued, 3 fully published, 4 matching GLINK RX_DONE. The separate first-completed-write flags are DATA accepted 1, queued 2, started 4, published 8, matching GLINK RX_DONE 16. The original and first-completed peers may differ. A later transfer's ACK must never substitute for the original's ACK. All-ones timestamps/identities mean unavailable, not timeout or failure.

POST ELF PATH categories are **1 server-check, 2 MCFG, 3 ruleset, 4 modem_efs, 5 wlanmdsp.mbn, 6 other path, 7 decode/other classification error**. These reuse the existing exact-name observations. POST OP is 1 RRQ or 2 WRQ. The first post-ELF latch has no separate timestamp: it records host acceptance after matching completion, not the modem's generation time. Do not assign a later read's time to an earlier request by port alone.

The global read inventory uses its existing different numbering: **1 ruleset, 2 modem_efs, 3 MCFG, 4 WLAN image, 5 other**; its kind mask uses `1 << kind`. It retains only the first five reads, while the count and mask continue updating. This inventory is not reset at ELF and can fill before ELF completes. QRTR META high byte is version, next byte packet type, low 16 bits padded QRTR length. The four headers contain no new QMI or TFTP payload projection; blanks/all-ones remain unknown.

MCFG FILE FFFFFFFF means no committed file; **00000000 means an existing empty file**. Write-session admission alone is not a completed file. Native commits only after final DATA; this panel does not change that behavior. ELF retirement and GLINK RX_DONE do not establish WLAN firmware loading or remote application acceptance.

## Validation and handset comparison

The actual new renderer is compiled in a host fixture with the real physical two-buffer display. Distinct original/first-completed peers, original ACK phase/block and clocks, all seven post-ELF path codes, all five read positions with distinct kind/peer identities, absent versus empty MCFG, absent traces, clockless observations and both framebuffers are checked. The AArch64 build type-checks the renderer against the actual owner/Core structures. Protocol/FIFO files and the seven hot runtime methods are compared byte-for-byte with the previous delivered implementation.

Required delivery checks remain the current/prior owner and Core suites, full I/O/event parity across 24 paired schedules, existing grant/receipt/fault controls, staggered registry/credit tests, selected native stack budget, exact-runtime capability mutation audit, packed initializer Normal-NC mappings, generated physical frame mappings, actual table-pair instructions, boot layout, compressed payload and byte-for-byte loader/report/CapDL reproduction. Pinned SDK components are reused after their bindings are verified; this is not a fresh SDK build or an all-interleavings proof.

The handset result should establish whether the original write's final ACK completes before ELF, which service request follows ELF on the current faster build, and whether MCFG becomes present or WLAN/WLFW advances. It can reproduce the older MCFG-only branch or identify a different first post-ELF exchange without another scheduling experiment. Profile: `native-post-elf-startup-v1`. Temporary boot only; no automatic handset actions.

The verified image is 68,931,584 bytes, SHA-256 `faf903b3a4452afa4f0abd701a9d5751909e9e3dd8f6cd9fc9163a8864345f5d`. Current and prior owner suites each passed 585 tests with one existing ignored case; both Core suites passed 269. All 24 paired schedules preserved full I/O/event order. The actual-runtime capability audit rejected 304 mutations, and the fresh packed archive contains eight Normal-NC maps. Loader/report/CapDL reproduction and compressed payload checks passed. The selected native stack path retains 872,144 bytes in the five-MiB mapping; the audit excludes TLS, indirect calls and other descendants and is not a whole-program worst-case bound. Physical boot remains untested until the next handset run.
