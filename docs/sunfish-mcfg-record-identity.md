# MCFG trace identity after cancellation

IMG_9762 confirms that all five observed closes committed, ELF and its independent host credit completed, and IPA completed without pending work or failure. WLAN firmware requests and WLFW services remained zero. Its apparent MCFG publication/completion after close is not reliable: an actual Core replay canceled an unstarted response, reused its slot for a different peer, and reproduced false publication/completion of the old trace.

The opt-in `sunfish-mcfg-record-identity` feature corrects that observer. At successful queueing it copies the existing host-only QRTR reservation identity. Publication and completion observations must match that exact token, TFTP route and body length. The reservation includes owner/generation/sequence/destination; an identical packet on a later reservation is a different record. The copied value is never used to mutate the flow table or authorize I/O.

After successful unstarted-record cancellation, a diagnostic hook clears the tracked slot and marks bit `0x100` (canceled). Queue time remains valid, but canceled records acquire no later publication or completion from slot reuse. The original record sequence remains visible. Synthetic journal kind `FD` denotes cancellation, distinct from a real packet and from synthetic completion kind `FE`. Cancellation time is optional; flags and identity remain valid with clocks disabled. Bit `0x200` means a queued trace lacked an identity and later publication/completion is not attributed.

The protocol cleanup transaction, writer selection, FIFO/domain checks, grant advertisement/ACK order, QRTR credits, file provider and native runtime handoffs remain unchanged. The only cleanup-commit addition is a void observation hook after successful `fail_unpublished` and before clearing the transmit record. Published/partial records remain owned until their real completions. Diagnostic timing can perturb execution cost; it is not authority for any protocol decision.

Tests cover cancellation followed by foreign slot reuse, real blocked/publication/completion, close with an already published reply, uncertain writes, transport reset and matching addresses/body with a different reservation. Existing cleanup/fault and current/prior feature suites also run. An on/off actual Core replay compares committed bytes across cancellation and slot reuse. Negative controls remove the cancellation hook or defeat the token comparison and must fail the corresponding regression.

The Linux/native review remains separate from the observer fix. The successful Linux cold journal requested WLAN at 178.730030 seconds, before MCFG at 187.027163 and the digest path at 187.041263. A fresh current-native Owner fixture still consumes the exact 93-byte `mbn_hw.dig` request without replying; the WLAN decoder rejects that name. Linux returns ERROR 1 on failed open. This is a real bounded negative-response gap, but the saved ordering does not establish it as the trigger for initial WLAN startup. This image leaves that response policy unchanged.

Panel: **MCFG RECORD IDENTITY**. All existing cleanup and startup outcomes remain. The MCFG flags show cancellation explicitly. `MCFG CLOSE CANCEL RECORD LO HI` replaces the client-ending summary with close time, cancellation time and low/high 32 bits of the original reservation sequence. `FFFFFFFF` means absent for those timestamps/identity fields. The journal still retains first 8/latest 8 records and omits its middle; client packet details remain in the retained capture but are not all displayed. Do not infer a real reply completion from a reused slot.

Verification: current/prior Owner suites passed 616/610 cases and current/prior IRQ suites passed 651/645, with one ignored case in each suite. Core suites passed 294/293. Six identity cases and two observer-boundary checks passed; both deliberate regressions failed as required. The cancellation/slot-reuse replay committed the same 496 bytes with the observer enabled or disabled. The native selected nested-frame budget leaves 829,632 bytes of the 5 MiB stack; this excludes TLS, indirect calls and other descendants and is not whole-program worst-case stack analysis.

The packed image also requires the 304-mutation capability audit and reproduction of the actual loader, report and CapDL, including eight Normal-NC capabilities. The pinned previously rebuilt SDK is reused; the native program is rebuilt. Artifact verification records these results privately and performs no device commands.

Cold temporary boot manually:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-mcfg-record-identity-v1/fastboot.img
```

Wait at least one minute and send the full settled panel. This is a diagnostic identity correction accompanying the previously successful cleanup behavior, not a new writer-priority experiment or a promise of WLAN startup. Preparation performs no device commands.
