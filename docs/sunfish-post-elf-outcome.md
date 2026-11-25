# Post-ELF outcome after the successful capacity retry

IMG_9726 confirms the intended normal-slot admission: the first freed slot1 was used by ELF, flags0F/result1. Receive-to-Owner completion was2,997.977ms, with WLAN/WLFW still zero. The latest full-pool snapshot preceded that success and cannot describe terminal packet ownership.

The offline review checks the actual selected source and Owner/FIFO tests. `ColdSession::protocol_complete` requires modem-ready, QMI uC-ready, the requested final indication and exit from initial boot. QMI uC-ready is set by committing the driver-complete response's opaque real transport receipt; the final indication requires its own receipt. Runtime's IPA COMPLETE is a historical latch of that report. Thus at least one complete handshake really finished. The snapshot's unsent0035 response does not establish a permanent IPA stall. A later duplicate response can coexist with completed handshake state, so the historical flag alone cannot identify an arbitrary response transaction. Two added actual Owner tests verify both boundaries, including QRTR credit without GLINK completion.

Pinned Linux fd78d17954a86ea6ceb0c535e618ea129e09f8dd sends0035's response before setting its uC-ready flag. Android28f9290ae067d3e7857126eb6312720dbe9c7c80 sets its modem-init flag before sending its response. Neither source supports inventing a missing native readiness operation merely from this frozen pool snapshot. The saved Linux QRTR-header boot identifies request txn2/message35 at248.461410, response header248.462369, GLINK publication248.462382 and matching RX_DONE248.462471; the final0022 indication publishes248.462550 and completes248.462608. Those headers identify messages, unlike a packet-size guess.

The separate saved ELF-locator boot associates ELF RX138.272783, reply138.273077 and matching completion138.273157 by names, size, order and intent, not captured QMI payload. Its first WLAN socket receive follows by691.852us. Its journal shows the earlier server-check write and negative modem_efs/ruleset reads. The successful cold-wifi journal requests WLAN178.730030, before absent MCFG187.027163. Do not mix these boots or printk/socket clocks into a single causal timeline. MCFG presence is not a demonstrated initial-WLAN prerequisite.

ELF reply completion does not imply completion of the separate host QRTR credit. Existing tests complete ELF first and its exact host-credit packet afterward. Matching endpoint pairs alone cannot bind a frozen RESUME_TX to one of several registry transactions. The existing `ElfHostCredit` binds that exact request and records requested/publication/completion plus state at ELF publication. These current fields were collected on IMG_9726 but were not displayed and cannot be retrieved from its photograph.

## Next image: existing observations, unchanged transport

Feature `sunfish-post-elf-outcome` retains the successful capacity-retry feature and changes only the selected renderer. No receive, queue, writer, completion, scheduling or device access method changes. The title is **POST ELF OUTCOME**. It displays:

- ELF acceptance/queue/publication/completion and reference-body match.
- Exact ELF host credit: confirmation requested, actual publication, matching completion and remote intent. HOST BEFORE bits1/2 describe publication/completion already observed when ELF published; FFFFFFFF means no observation.
- Live IPA protocol flags, registration/driver request counts, committed publications, pending message, accepted responses and failure state. FLAGS bits0..9 are modem-ready, QMI uC-ready, indication-requested, indication-sent, currently-complete, failed, pending-record, queued-record, held-response and initial-boot. The bottom IPA field is the separate historical Runtime latch.
- The existing early ELF retry result, original final ACK, first accepted post-ELF service request, first post-ELF MCFG result/reply/client ending, first other-path category, cumulative read mask and WLAN/WLFW.
- The existing bounded journal: first8/latest8 records, with omitted-middle count. The journal starts when ELF is accepted, not when it completes; only records whose clocks actually follow ELF completion are post-completion observations. It excludes IPA endpoint traffic and is not an exhaustive GLINK log.

POST PATH values1..7 are server-check, MCFG, ruleset, modem_efs, WLAN image, other and unknown. OTHER KIND additionally identifies exact mbn_hw.dig as8. First-post selection means host acceptance after matching ELF completion, not modem generation time. MCFG CLIENT FIRST/LAST encode opcode in high16 and ACK block/ERROR number in low16. ERROR9 can be a normal stat/end-of-transfer ending; it is not inferred when absent. OFFER/REPLY rows display selected option values, not every raw byte. No physical outcome is claimed before boot.

The renderer is exercised against the real dual framebuffer with a pending duplicate versus completed IPA handshake, separately outstanding credit, all seven post-path categories, absent/rejected server results, zero-size file, omitted journal and clockless records. Target validation checks the boot envelope, selected stack and actual capability/mapping audit. Pinned Normal-NC SDK reuse is distinguished from recompilation.

After a full power-off, enter fastboot and temporarily boot:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-post-elf-outcome-v1/fastboot.img
```

Wait at least one minute and photograph the entire settled panel. No automatic Pixel action is performed.
