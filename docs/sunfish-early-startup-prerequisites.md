# Early startup comparison after IMG_9771

IMG_9771 completed its watched post-ELF MCFG reply before the client closed, without using an idle loan. IPA remained complete and no WLAN firmware request or WLFW server appeared. The watched peer was0x4b rather than IMG_9770's0x4a, so those are not asserted to be the same logical request. This changes the next investigation: the MCFG idle opportunity is no longer the best discriminator of the earlier WLAN divergence.

## What the saved Linux reference establishes

The successful cold Linux journal records the first server-check WRQ at177.023110s, then ruleset RRQs and a modem_efs RRQ at177.039398–177.040307s. The server explicitly rejects both missing paths. The first WLAN firmware RRQ follows at178.730030s; the first MCFG RRQ is187.027163s. Thus the later MCFG/digest exchange is not an established initial-WLAN prerequisite. Missing ruleset/modem_efs files also do not themselves prevent that reference boot from requesting WLAN firmware.

The separate locator reference associates a likely ELF request/reply/completion from sizes, order, service-name logs and GLINK intent identity. It is not a captured exact QMI payload, and its kernel/socket clocks must not be combined with journal clocks to derive a deadline. Native's current exact ELF-body comparison is useful independently of that association.

The existing notes already document native completion of the original server-check write before ELF (November23 POST ELF STARTUP photos), early ruleset/modem_efs reads, the correct locator body, and later MCFG activity. Those facts do not establish that each early negative reply completed before its own peer closed in the current boot. IMG_9771 shows only its later watched MCFG reply. Port numbers, request identity and timing cannot be imported from another boot.

The pinned Linux/LineageOS source audit was rerun, including `qcom_pd_mapper`, PDR, remoteproc, ath10k QMI, downstream ICNSS and Android init/device files. It preserves the existing conclusion: a locator mapping is not a host command to launch WLAN, and these sources do not expose the proprietary modem worker's complete startup decision. The successful reference reached firmware requests before ath10k attachment. There is no source-backed reason here to synthesize an MCFG file, add a speculative restart command or relax transport checks. The audited upstream/downstream source revisions are distinct from the exact working boot's kernel; the audit establishes selected API/ordering semantics, not byte identity of those kernels.

## New bounded observation

`sunfish-startup-prerequisites` adds observation only, independently of ELF completion or an MCFG watcher. It keeps the first two accepted RRQs for each exact path `/readwrite/ota_firewall/ruleset` and `/readwrite/modem_efs`, with per-path observation and omitted counts. Later requests cannot overwrite these entries. Duplicate hooks for a retained handle do not allocate another entry; omitted requests are not retained for deduplication. Each entry records the full opaque request handle privately, actual peer node/port, body length, numeric option projection including zero/presence, server result and reply opcode/code, normal queue attempts/waits, actual host-only reservation and slot, publication, exact remote-intent completion and validated same-node client close. Client ACK/ERROR projection is scoped to peer and transfer port; it is not assigned a separate opaque request identity.

Publication/completion require the original reservation, slot, reply route and length; completion also requires the original remote intent and prior publication. Reused slots or intents cannot update an unrelated entry. The capture never chooses a packet, authorizes I/O, retires an owner, changes an ACK or modifies queue/grant/credit priorities. No new shared-FIFO read, validation, cursor sample or client-close action is added. Unknown clocks remain unknown; optional existing clock calls and fixed metadata copying can perturb handset timing. A GLINK completion proves buffer return, not application-level acceptance of the reply. ERROR1 does not call for OACK/DATA.

## Reading EARLY STARTUP REPLIES

Rows2–7 show runtime/close state, the original peer and WRQ/OACK/DATA clocks, original final-ACK phase/publication/completion/close, ELF receive/queue/publication/completion, exact ELF-body match, ELF host-credit mask and locator queued/completed name masks, then IPA/PD/WLAN/WLFW outcomes. Host-credit bits1/2/4 mean requested, intent available and completed; name masks use the existing service-name codes.

Rows8–12 and13–17 are the first two ruleset RRQs; rows18–22 and23–27 are the first two modem_efs RRQs. Each five-row block shows:

- Kind1 ruleset or2 modem_efs, peer, flags and full reservation sequence split low/high.
- Request body bytes, rsize, numeric option-presence mask, server error result (zero means success), option-parse flags and client opcode/code projection.
- Request receive, server dispatch and successful queue clocks.
- Reply publication, matching GLINK completion and client close clocks.
- Reply opcode/code/body bytes/host transfer port, admission attempts, FlowBusy and NoCapacity counts. These compact fields use four hex digits except the eight-digit port; counters above that display width cannot be reconstructed from the panel.

Entry flag bits1/2/4/8/16/32/64/128 mean received, dispatched, response produced, queued, published, matching completion, client ACK-or-ERROR and closed;512 marks a missing transport reservation. A phase flag may be set with an unknown optional timestamp. The final row packs global read count/kind mask, per-path observations and per-path omitted counts. `FFFFFFFF` is unknown, not elapsed time.

Offline tests exercise the actual Owner's two early ERROR1 exchanges and real GLINK receipts; bounded recording before ELF exists; zero-rsize presence; dispatch failure; exact peer close; reset; and wrong reservation/intent rejection. Observer-on/off comparisons must retain the complete ordered Fake I/O and wire bytes for both actual early exchanges. Expected failing controls remove the receive hook or reservation check. Actual physical framebuffer rendering covers both buffers, absent entries, clocks and field widths. Broader fault, backpressure, grant, credit and close regressions remain required.

The next hardware question is specific: did each early negative reply complete before its client closed, while the original final ACK and correct ELF reply also completed? If yes, these exchanges can be deprioritized as WLAN blockers and the missing startup prerequisite must be sought elsewhere. If not, the first incomplete exchange is identified by exact identity and stage. This image diagnoses that distinction; it is not a new WLAN scheduling fix or a promise of firmware startup.
