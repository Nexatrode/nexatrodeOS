# Close handling for absent-MCFG error responses

IMG_9731 and IMG_9747 showed identical retained state: runtime `0x73`, nested detail `0x64`, peer `0x49`, cleanup stage 1, no cleanup commit, and ELF not yet queued. An isolated actual Owner/FIFO/server replay reproduced stage 1 when an absent MCFG read created an ERROR1 transfer and a retained retry was followed by DEL_CLIENT. That transfer had `missing_kind=1` but neither RAM read nor write state, so the previous MCFG preflight rejected it. The photo does not expose the server slot's exact contents; this is a matching reproduced software case, not a recovered phone call stack.

The opt-in `sunfish-mcfg-missing-close` feature extends the existing close classification to that exact error-response state. It requires the MCFG missing-path discriminator 1, no RAM read/write state, zero buffered length, zero last length/block and no completed transfer. Cache refresh and exclusive close preflight use the same predicate. Abandoned peers remain excluded. Other missing paths, server-check transfers and mixed state remain rejected. The existing server cleanup then reserves the closed peer/port without creating an MCFG file or aborting another peer's writer. An error-response transfer remains identifiable even if another peer has since committed the MCFG file.

Core GLINK/FIFO code, QRTR credits, grant advertisement/ACK order, native runtime handoff, file storage implementation and request/reply parsing are unchanged. The existing close transaction cancels only held requests, retained replies and zero-byte unstarted transmit records. Published replies retain their original remote intent and complete only against real matching RX_DONE. Normal receive returns, fresh FIFO validation, terminal fault handling and closed-peer fences remain active. MBN digest behavior is unchanged.

Offline cases cover unsent errors, published errors and real completions, held retries, retained duplicates, foreign server-check/ruleset/modem-EFS rejection, later file creation, fresh readiness/snapshot/cursor faults, and RAM read/write classification. An actual nested ELF Router/Owner case combines the missing-file close with both ERROR1 and ELF completions. A negative control disables only the missing-state predicate and must fail the held-retry cleanup case. All nine focused missing-close cases passed, including the nested ELF case. The full suites passed: Owner 610 (prior configuration 602), IRQ 645 (prior 636), and Core 293 in both configurations. Each Owner/IRQ suite retains one existing ignored test. Two new source-boundary tests confirm unchanged transport and request/reply policy; existing framebuffer, source and fault checks also pass. The selected native stack regression audit leaves 830,224 bytes of its 5 MiB stack; it excludes TLS, indirect calls and other descendants. Actual packed-image capability/payload audits accompany the image.

Panel: **MCFG MISSING CLOSE**. The top `RUNTIME DETAIL CLOSE STAGE` fields retain their meanings. Stage 6 denotes a committed cleanup. `CLOSE UNSENT KEPT PEER KIND` replaces the cleanup timestamp with a classification: 0 no existing server transfer, 1 RAM read, 2 RAM write, 3 absent-MCFG error response, `FFFFFFFF` not successfully classified. KIND is the last attempted cleanup's classification; PEER remains the last committed cleanup's peer. The pending peer is shown at the top on failure. KEPT counts published/partial reply and QRTR-credit records retained at successful cleanup, not the current outstanding count.

Cold temporary boot manually:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-mcfg-missing-close-v1/fastboot.img
```

Wait at least one minute and photograph the full settled panel. No device commands run during preparation. The intended result is cleanup committing without the stage-1 stop, followed by continued ELF/IPA startup. WLAN firmware requests and WLFW discovery remain physical outcomes to check; this change does not establish MCFG as the WLAN startup prerequisite.
