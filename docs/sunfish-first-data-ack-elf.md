# Original DATA, final ACK and ELF lookup timeline

IMG_9692 established original-transfer DATA reception, 108.585 ms after the
OACK peer-read observation and 1,327.860 ms before exact close visibility.
It did not display server dispatch result, final ACK or ELF lookup timing.
Those unphotographed handset fields cannot be reconstructed from source alone.

Offline review found existing observations for the next stages. The original
first-write trace retains DATA block, full TFTP packet byte count and dispatch
result, final accepted block, and ACK phases: 0 absent, 1 prepared, 2 queued,
3 fully published, 4 matching GLINK RX_DONE. Queue matching checks peer, local
transfer port and ACK block, then tracks the transmit slot. The matching
completion clears that slot, preserving the completed observation across reuse.
ELF lookup receive/queue/publication/GLINK-completion timestamps already exist.

The legacy first-final-ACK trace is unsuitable as unconditional proof of this
original transfer: it records the first locally completed write. Its optional
first-transfer filter is not enabled in the current IPA-before-OACK runtime
composition. Another peer can finish first. The new read-only
`original_tftp_write_trace` instead correlates the earliest accepted OACK with
the first-write record by Session owner, generation, opaque original request
intent, full peer address and local transfer port. It returns no match rather
than substituting an unrelated completed transfer. This is historical metadata,
not validation of a currently held receive handle or a new authority token.

`FIRST DATA ACK ELF` presents those existing observations on the same runtime
clock. It adds no protocol state field, I/O operation, clock read or scheduling
turn. The pair reads, metadata batch, handoff stack split and Normal-NC mappings
remain active. Drawing a different panel has ordinary CPU cost; no claim is
made that a diagnostic screen has zero effect on elapsed handset timing.

Interpretation:

- TRACE MATCH must be 1 before associating dispatch/ACK fields with the original
  transfer. FIRST PEER and TRANSFER PORT identify it.
- DATA RX is transport reception. DATA RESULT 0 means successful dispatch of
  that first DATA packet; an absent result is FFFFFFFF. DATA BYTES includes the
  four-byte opcode/block header, not just application bytes.
- FINAL BLOCK is set after the server accepts a terminal DATA block and prepares
  its ACK. WRITES COMPLETE counts all locally completed server writes; it cannot
  identify this transfer alone.
- ACK PUB and ACK DONE distinguish publication from GLINK buffer return.
  Phase 4 is a transport receipt, not TFTP application acceptance. RX_DONE can
  arrive after client close or cleanup, so its lateness alone proves no failure.
- CLOSE VIS is the checked FIFO visibility of exact modem node 0, port 0x1f.
  CLOSE RX is local ingestion of this transfer's close. The visibility/read
  observations are attached only when original identity matches; visibility is
  unavailable for a differently addressed client. ACK BEF CLOSE compares ACK
  DONE with CLOSE VIS: 1 strictly before, 0 otherwise, FFFFFFFF if unavailable.
- ELF fields concern the `kernel/elf_loader` service-registry locator reply,
  rather than loading a firmware ELF. WLAN RRQ and WLFW are separate later
  outcomes, with their own first-observed timestamps.

The saved Linux metadata for modem peer 0:0x1e records a 73-byte WRQ, 39-byte
OACK, nine-byte DATA packet and four-byte ACK. The hook's DATA receive and ACK
send timestamps differ by about 106 microseconds. These are userspace metadata
timestamps, not modem application-acceptance times. The corresponding QRTR
window then records client removal. Saved path logs show WLAN firmware requests
later in that successful reference boot. Port numbers differ between Linux and
native; sequence and identity within each boot matter, not equality of port IDs
between boots. All three input files and the extracted records are hash-bound
in the private candidate. This sequence is evidence of order, not proof that
one event alone causes WLAN startup.

Three identity tests cover exact matching, unrelated completed peers, absent
records and owner/generation/intent/address/port mismatches; a panic clock proves
the getter reads no new timestamp. The existing full owner DATA/ACK/ELF fixture
also asserts the new view matches the original completed transfer. The actual
renderer and existing clock conversion are compiled in a host fixture with
the real dual-framebuffer Display. Missing identity/endpoints, wrong close
node/port, equal/late completion and invalid clock conversion are checked.

Independent review found an important attribution edge case in the first panel
version: OACK READ and CLOSE VIS could still refer to the first observed WRQ
when the accepted-transfer match failed. Both now require the same identity
match. The regression was observed RED to GREEN with populated underlying
timestamps and missing identity. No protocol or transport test changes were
needed for this rendering correction.

The decision after the next boot is deliberately bounded. If DATA remains
undispatched or has a server error, examine that dispatch. If ACK remains
prepared/queued, examine the corresponding queue/writer stage. If publication
and transport completion succeed, follow the ELF reply and later WLAN request
instead of treating a completed TFTP stage as the continuing bottleneck. A
missing field or mismatched identity is unavailable evidence, not a zero delay.

The private image profile is
`.cache/sunfish-native-wifi/private/native-first-data-ack-elf-v1`. Required gates
include 567 owner tests versus 564 in the preceding profile, 215 link tests,
24 paired full-I/O replay schedules, native table/fault tests, stack budget,
304 capability mutations, eight packed Normal-NC maps and whole payload/SDK
reproduction. The prior unrelated SMMU model 78-versus-128 fixture mismatch
remains documented separately and is not claimed fixed or passing.

The image is 68,902,912 bytes, SHA-256
`324fdbd3f9ad18763d899258dac4eaed70b593683149d2caac88f61535b58c86`.
No new handset outcome is claimed before the cold temporary boot.
