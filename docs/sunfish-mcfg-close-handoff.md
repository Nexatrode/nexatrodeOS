# MCFG close handoff after the queued ELF writer

IMG_9729 stopped with runtime error `0x73`, one MCFG close observed and zero cleanups committed. The original write's final ACK and the ELF host credit completed, but the published ELF reply had no recorded completion. WLAN requests and WLFW services were zero. The selected panel omitted the nested runtime error detail, so the exact physical failure path is not proven by that photo alone.

An actual Router/Owner replay now reproduces the missing boundary: the queued ELF dispatch receives an MCFG close and pauses the Owner. The runtime then enters an ordinary service without resolving that close, and the service returns `Stopped`. The initial regression test failed with the paused close still present; the original unwrapped path remains a passing characterization of the failure. This is a host scheduling reproduction, not a reconstructed modem deadline.

The opt-in `sunfish-mcfg-close-handoff` feature replaces that one runtime call with `dispatch_queued_elf_mcfg_handoff`. It preserves live-router, generation/identity and other-owner checks, runs the existing bounded queued ELF dispatch, and resolves any pending MCFG close through the existing exclusive Owner/server cleanup before returning to ordinary services. If a close was already pending, it resolves without re-entering normal dispatch. Cleanup faults remain terminal; there is no extra writer, retry loop or reduced validation. With no pending close, an actual fixture compares identical FIFO operations and bytes against the original dispatch.

Offline verification passed six new nested-handoff cases and four source-boundary checks. The full suites passed: Owner 602 in both configurations, IRQ 636 with the fix and 630 in the prior configuration, and Core 293 in both. Each Owner/IRQ suite retains one existing ignored test. The positive nested-close fixture checks actual publication and real matched receipts with optional timestamps absent; held and already-published MCFG replies are both exercised. Other-owner priority, generation rejection and fresh-gate faults remain covered. A negative control removes only the new wrapper cleanup and fails the nested-close regression. The selected native stack regression audit leaves 830,224 bytes of its 5 MiB stack; this excludes TLS, indirect calls and other descendants.

The MCFG cleanup commit, FIFO access code, Core GLINK scheduling, QRTR flow code, RAM server behavior, published-buffer identity and normal completion path are unchanged. Partially or fully published replies and QRTR credits still require their real completions. MBN digest behavior is unchanged.

The earlier IMG_9729 review also flagged registry retry as a possible gap. Further review found that `retry_registry_handoff` already returns without retrying whenever an exclusive firmware/MCFG close is pending. That path is already guarded and is not changed here. The new source checks bind the native runtime to the exact helper exercised by the host fixtures.

The panel is **MCFG CLOSE HANDOFF**. The top row now reads `RUNTIME DETAIL CLOSE STAGE`: outer runtime error, nested runtime detail, currently pending MCFG peer port (`FFFFFFFF` if none), and cleanup stage. Stages are 0 not attempted, 1 entry, 2 server preflight passed, 3 Owner preflight passed/before Core commit, 4 Core commit passed, 5 server close passed, 6 full cleanup committed. These are diagnostic counters; they do not authorize progress. Existing cleanup counts and ELF/MCFG/IPA/WLAN/WLFW outcomes remain visible.

Cold temporary boot manually:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-mcfg-close-handoff-v1/fastboot.img
```

Wait at least one minute and send the full settled panel. No adb or fastboot action is run by the preparation. This fixes a reproduced runtime handoff defect; it does not establish the cause of WLAN remaining unavailable or promise firmware/radio startup on the next boot.
