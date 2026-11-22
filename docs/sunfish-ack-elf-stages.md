# Original DATA ACK and ELF admission stages

IMG_9695 demonstrated local completion of the original server-check write and
matching GLINK completion of its final ACK. It also exposed two remaining
intervals:

| Interval | Native IMG_9695 |
|---|---:|
| Original DATA receipt to final ACK publication | 1,046.324 ms |
| ACK publication to matching GLINK completion | 295.664 ms |
| ELF locator request receipt to queue admission | 16,165.554 ms |
| ELF queue admission to publication | 79.328 ms |
| ELF publication to matching GLINK completion | 44.034 ms |

The saved Linux first transfer received its nine-byte DATA packet at
248.451115737 and sent its four-byte ACK at 248.451221362 in the metadata-hook
clock: 105.625 microseconds. In the saved `tqftpserv.c`, the write path checks
the sender and DATA opcode, writes the payload, calls `tftp_send_ack`, then
reports whether another block is needed. Linux does not defer ACK construction
to a subsequent service loop. This does not prove that our larger delay is
server execution: native receipt, dispatch, checked buffer release, admission
and writer publication are separate boundaries; Linux hooks also differ from
native observations. Both servers still need valid transport and credit state.

The native combined server retains the generated ACK in a bounded per-peer
reply record, observes server completion, calls `finish_tftp_request` (including
checked receive release and scheduling), then attempts `flush_peer_tftp_reply`.
Each queue attempt keeps the existing generation/identity checks, transmit
capacity and same-destination flow gates. Successful admission is followed by
ordinary scheduling and checked writer selection. The previous image recorded
only receipt and full ACK publication, so it cannot attribute the 1.046 seconds
to any one of these stages. Publication before host close visibility also does
not timestamp the modem application's timeout decision or ACK consumption.

For ELF, saved native notes distinguish long receive passes, preselection
registry retries, same-peer queue rejection, and post-admission writer delay.
IMG_9595 recorded a 12.049-second preselection interval with transaction 4
returning FlowBusy twelve times, then two ELF same-peer waits behind that exact
transaction. Another older trace identified transaction 3 holding transaction
4. The current 16.166-second interval does not identify either transaction as
its cause. The saved Linux locator comparison places the likely ELF request,
reply and RX_DONE at 138.272783/138.273077/138.273157 on its GLINK clock, using
locator logs, packet sizes and order for attribution. That receipt boundary is
not identical to native request acceptance. Existing native ELF phase records
already provide the next necessary separation; no wider packet capture is
needed here.

The private profile `native-ack-elf-stages-v1` enables
`sunfish-ack-elf-phase-trace` on the previous table-pair/metadata-batch/stack-split
profile. Its settled panel is **ACK ELF STAGES**:

- DATA RX, DATA DISP, ACK READY, DATA RETURN and ACK QUEUE are distinct runtime
  timestamps. RETURN means successful publication of that DATA's GLINK RX_DONE,
  not merely a request to release it. ACK queueing/publication can precede this
  return in a valid offline schedule; do not assume all displayed stages are
  monotonically ordered.
- ACK PUB/DONE/PHASE are shown only after the exact first DATA was accepted as
  final and that DATA's request-aware ACK queue operation succeeded. Matching
  bytes, peer, transfer port and block alone are insufficient. An unavailable
  clock leaves its timestamp unknown without invalidating identity proof.
- RX_END, REG_ENTRY, ELF_SELECT, ELF_QUEUE, ELF_PUB and ELF_DONE divide the ELF
  receive/dispatch/admission/writer path. REG_RUN0/1 are the initial compressed
  transaction/name/outcome/count runs, not a complete transaction history;
  REG_OMIT counts records beyond the four stored runs. Runs 2/3 are retained in
  memory but are not shown on this compact panel. ELF_PEER_WAITS counts reason-1
  same-destination queue rejects. BLOCK_TXN/PUB/DONE refer to the first exact
  unpublished registry reply observed blocking ELF directly.
- CLOSE_VIS is a validated FIFO observation for the exact modem0:0x1f client
  and is hidden if the original trace has a different address or no matched
  identity. Missing observations are FFFFFFFF; zero can be a real count/time.
- WLAN_RRQ and WLFW_SERVERS remain outcome indicators. ELF_DONE is a locator
  reply's GLINK completion, not firmware loading or working Wi-Fi.

Only diagnostic state, optional bounded clock observations and the display
change. No writer priority, packet, FIFO sample, validation boundary, grant
order, retry, credit or fault policy changes. Stage hooks follow the existing
bounded original/post-ELF trace iterator; the latter can add an observation for
an undisplayed later transfer, and the same event can have two clock samples
if both views match. These measurements include their own observation overhead
and possible preemption; fake-peer parity is not a handset timing guarantee.

Review found and fixed two identity errors before delivery. Return matching now
uses the opaque datagram ID (`tftp_datagram_id`), which differs from the wire
intent after WRQ-buffer reuse. A regression blocks the actual eight-byte
RX_DONE, checks its wire ID, accepts the identical record on retry and verifies
that only successful commit records return. Queue matching moved out of the
generic transmit insertion path to successful request-aware queueing. It also
requires completed final-DATA acceptance independently of the clock. Regression
cases cover generic queueing, wrong owner/generation/request, wrong port/block,
a rejected first DATA, and absent clocks. The actual panel fixture hides
unproven ACK phases and tests missing identity/endpoints, full-address close
matching, clock conversion and identical dual framebuffers.

The earlier unrelated SMMU handoff model test still expects 78 SMR groups where
the current source declares 128; it is documented in the predecessor profile's
`unrelated-model-baseline.json`. This diagnostic does not change that policy or
claim that unrelated model test passes. Native corruption/fault checks and the
image's full capability audit remain required delivery gates.

No Pixel command is run by the build workflow. This profile is for manual cold
temporary `fastboot boot`; no partition is flashed. No working WLAN/Wi-Fi
outcome is claimed before its handset result.

Delivery verification for `native-ack-elf-stages-v1`: 570 owner cases passed
with one prior ignored case; the previous profile passed 567 with one ignored;
218 link cases passed. All 24 paired replay schedules (48 executions/96 mode
runs) preserved exact packet/cursor publications, full fake-peer I/O digests and
event times. Ten fault controls, both profiles' 4,096-word native corruption
checks and 16 domain-fault cases, 32,768 primitive bit corruptions, actual
renderer tests and stack/source contracts passed. The selected compiled stack
budget leaves 950,416 bytes above the measured 4,292,464-byte peak in the same
5 MiB mapping; this is not whole-program worst-case stack analysis.

The exact-image capability audit rejected 304 mutations. The unchanged pinned
SDK reproduced the entire new loader, CapDL and report byte-for-byte. The
actual serialized initializer archive has 24,536 objects and exactly the eight
expected Normal-NC RW/NX/uncached table caps; incorrect and missing archive
counts were rejected. One extra PD code page shifts 544 named protocol RAM
allocations relative to the previous image, which were audited rather than
assumed unchanged. AArch64 code inclusion, compressed kernel, Android boot v2
layout, wrapper and source/reference/private-input hashes passed. No device
command was executed.

The temporary image is 68,907,008 bytes, SHA-256
`f2dd2e9aaf9e728f3b7930274a54d3e195cac29c2880277c177cc7646a952539`.
Its private manifest, source hashes, actual disassembly, stack report, negative
controls, replay logs and manual capture instructions are retained beside it.
