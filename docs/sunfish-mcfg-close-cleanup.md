# Bounded MCFG client-close cleanup

IMG_9728 showed successful IPA and ELF completion, followed by MCFG reads whose captured OACKs appeared after the client's close. The offline replay in `sunfish-post-elf-late-reply-review.md` reproduced held and duplicate MCFG requests surviving DEL_CLIENT. This change fixes that lifecycle gap; it does not establish MCFG as the cause of WLAN remaining UNINIT. Linux's saved WLAN request preceded its MCFG/MBN requests.

The opt-in `sunfish-mcfg-close-cleanup` feature pauses the exact validated MCFG DEL_CLIENT at the transport Owner. An exclusive Owner/server handoff checks the generation, held request identities, retained replies, server peer/port, staged writer and matching transmit records. A fresh validated FIFO sample precedes the commit. Failures stop and quarantine the session. Normal FIFO checks, advertisement-before-ACK ordering, QRTR credits and interrupt rearming remain active.

Cleanup cancels held MCFG requests, retained replies and transmit records that have sent zero bytes and own no remote intent. Receive buffers return through the ordinary checked RX_DONE writer. Partially and fully published replies retain their original remote intent and completion identity; QRTR credit records also remain on their normal path. An unfinished write aborts its staging area without replacing committed MCFG bytes. Closed peer ports keep the existing generation fence and the server's reserved transfer port. A late packet cannot reopen a closed transfer. The MBN digest response behavior is unchanged.

Offline tests cover duplicate held requests, unsent and retained replies, partial publication and real completion, credit retention, staged-write cancellation, unrelated peers, mixed-service rejection, identity/server mismatch, readiness/snapshot/cursor faults, tombstone capacity, blocked and uncertain receive-return publication, and actual runtime IRQ routing. A negative control disables the close pause and must fail the held-request cleanup test. The full suites passed: Owner 602 (prior feature set 592), IRQ 630 (prior 619), and Core 293 in each configuration. Each Owner/IRQ suite retains one existing ignored test. All 11 focused cleanup/IRQ cases passed. Source/stack checks, renderer tests and native payload/capability audits accompany the private image. The selected native stack regression audit leaves 830,288 bytes of the 5 MiB stack; it excludes TLS, indirect calls and other descendants and is not a whole-program worst-case bound.

The panel title is **MCFG CLOSE CLEANUP**. `CLOSE OBS COMMIT HELD RETAINED` contains cumulative counts of observed closes, committed cleanups, canceled held/deferred requests and discarded retained replies. `CLOSE UNSENT KEPT PEER US` contains canceled unstarted transmit records, retained published/partial reply plus credit records, last successfully cleaned peer port and cleanup time in microseconds. KEPT is a count at cleanup, not the current outstanding count. The existing ELF, MCFG, journal, live IPA, WLAN and WLFW rows remain. The two option rows are replaced; their prior evidence remains in IMG_9728 and the saved notes.

With the Pixel fully powered off, enter bootloader with Volume Down + Power, then run this temporary boot manually:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-mcfg-close-cleanup-v1/fastboot.img
```

Wait at least one minute and photograph the full settled panel. No adb, fastboot or device operation is performed by the preparation scripts. Native execution, actual modem behavior and progress to WLAN still require that cold boot.
