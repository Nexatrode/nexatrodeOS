# Post-ELF MCFG request, reply and client trace

The `sunfish-post-elf-mcfg-trace` image displays **POST ELF MCFG TRACE**. It observes the first accepted `/readwrite/mcfg.tmp` RRQ after the exact ELF reply's matching GLINK RX_DONE. It starts its bounded journal at the accepted ELF lookup. It does not change request admission, TFTP responses, receive-slot ownership, FIFO validation, grant advertisement-before-ACK order, QRTR credits or firmware selection.

The preceding physical boot completed peer 0x1f's original write and final ACK before the ELF lookup, then accepted a 73-byte MCFG RRQ. IPA completed; WLAN firmware requests and WLFW announcements remained zero. See [the result](sunfish-post-elf-startup-20261123.md) and [the reference comparison](sunfish-mcfg-probe-comparison.md). Linux can terminate a successful size probe with ERROR 9 without a completed read, so the existing 1/1/0 MCFG counters do not identify a failure. Two synthetic 73-byte requests also demonstrate different admission outcomes; they are alternatives, not reconstructed phone bytes.

## Reading the panel

All values are hexadecimal. Clock values are microseconds relative to runtime start. `FFFFFFFF` is unknown or absent, not elapsed time; a zero option is a real value when its presence bit is set. Stage bits still advance without a clock.

- **ELF R Q P D:** original accepted lookup, queue, full publication and exact GLINK completion.
- **ELF BODY DONE CREDIT CONF:** reference body match; completed; host credit bits (requested=1, completed=2, published before ELF publication=4, completed before ELF publication=8); ELF QRTR confirmation request.
- **MCFG PEER LEN FLAGS FILE:** peer port, request body length, stage bits, committed file length immediately before actual server dispatch. File absence is meaningful once stage bit 2 is set.
- **OPT MASK FLAGS UNK KEY:** present bits for blksize=1, timeoutms=2, wsize=4, tsize=8, rsize=16, seek=32, unlink=64; parser flags; unknown count; first four unknown key bytes in big-endian ASCII. `unlink` is named for observation but remains unsupported by the native request validator. Parser flags are malformed=1, bad numeric value=2, duplicate=4, unknown=8, numeric overflow=16, oversized=32, non-octet mode=64. Observation scans at most 512 bytes; it never admits a request.
- **BLK TIME WIN TSIZE / RSIZE SEEK UNLINK:** actual recognized numeric request options. Presence bits distinguish absent values from an explicit zero or valid u32 MAX. Unknown option text beyond its four-byte key prefix and payload contents are not retained.
- **SERVER RESULT OP NUM LEN:** actual server result (0 success, otherwise the native server error code), prepared reply opcode, block/error number when applicable, body length. A failed dispatch or zero-length result has no prepared reply. An OACK's number is absent.
- **REPLY MASK FLAGS PORT ID:** reply option presence/parse flags, actual transfer source port, matching remote GLINK intent when fully published. The two following rows show actual prepared OACK values in the same option order.
- **RX QUEUE PUB DONE US:** target request accepted, reply queued, fully committed to GLINK TX FIFO, matching validated GLINK RX_DONE. These do not establish application acceptance or a peer-tail crossing.
- **CLIENT FIRST LAST COUNTS:** first/last valid ACK or ERROR as `opcode<<16|number`, ACK count in high 16 bits and ERROR count in low 16 bits, same-peer RRQ count after the original. **CLIENT FIRST LAST CLOSE** holds first/last client times, exact validated DEL_CLIENT time, peer port. Other peers and transfer ports do not advance the ending. Close freezes subsequent client-ending observations, but a late reply completion remains visible.
- **JOURNAL N OMIT BAD QWAIT:** retained-window total, omitted middle records, undecodable complete QRTR candidates, target queue waits (FlowBusy high16 / NoCapacity low16).
- **ORIG ACK ELF DONE:** original final ACK's GLINK completion and ELF completion, separate identities.
- **MCFG W FILE READ IPA / PD STATE WLAN WLFW:** existing outcomes; committed-file length zero differs from absence. Read count zero can be normal for a size-only probe. WLAN and WLFW remain the milestones needed to establish startup progress.
- **OTHER KIND PEER LEN OP:** first accepted post-ELF request outside temporary/server-check names: 5 WLAN path, 6 other, 7 unclassified path, 8 exact `mbn_hw.dig`; then peer, body length and opcode. This is host acceptance order, not modem generation time.

Stage bits: accepted request=1, server dispatched=2, nonempty reply prepared=4, queued=8, fully published=16, exact RX_DONE=32, valid same-peer ACK/ERROR=64, exact DEL_CLIENT=128. The original opaque request handle binds dispatch and queueing. The normal validated transmit index plus remote intent binds publication/completion; completion detaches the observation from the reusable slot. A same-peer duplicate does not replace the original identity.

## Journal

Sixteen rows preserve the opening eight and latest eight relevant records; the omitted count exposes saturation. Each row is `us tx kind srcPort dstPort tag a b`. `tx=1` means a fully committed outgoing QRTR datagram; `tx=0` a completely assembled incoming datagram. Incoming header projection precedes normal service validation and is raw metadata, not a statement of application acceptance. Relevant traffic comprises the original ELF endpoint pair, the local TFTP namespace, QRTR HELLO/DEL_CLIENT/BYE/RESUME_TX and IPA/locator/WLFW server records.

Tags: 0/2/4 raw QMI request/reply/indication (`a=transaction`, `b=message`); A TFTP (`a=opcode<<16|block-or-error`, `b=path-category<<24|bodyBytes`); B server announcement (`a=service`, `b=instance`); C BYE or D DEL_CLIENT (`a=node`, `b=port`); E synthetic exact GLINK completion (`a=intent`, `b=0 ELF or 1 target MCFG`); F unknown. Synthetic completion has kind FE. TFTP category 1 server-check, 2 MCFG, 3 firewall ruleset, 4 modem_efs, 5 WLAN, 6 other, 7 malformed path, 8 exact mbn_hw.dig. Raw QMI framing is recognized only with a matching declared length; unknowns are not inferred from packet size.

## Offline evidence and limits

The actual Core/Owner tests cover pre-ELF requests, opaque duplicate identity, same-peer queue pressure, a blocked write that cannot mark publication, exact completions, transmit-slot reuse, clockless stages, wrong-peer endings, ERROR 9, exact close, reset and real server dispatch. Existing native probes cover missing-file, zero-rsize rejection, size-probe termination and real data-read completion; the pinned Linux C oracle exercises its actual parser/OACK/read path. The on/off identity replay compares every committed outgoing byte. Existing paired schedules compare complete I/O and peer-event order, with all FIFO checks retained. The actual renderer test checks both framebuffers, widths, zero/presence, missing state, rejection, clockless state and head/tail journal placement.

The capture adds bounded CPU parsing/copies, optional diagnostic clock calls and repaint-change detection under the existing display gate. It can perturb timing. No diagnostic field or clock decides scheduling/admission, and no additional hardware-read site is introduced. Native stack/capability/payload verification is required for the new image. Selected stack analysis is not whole-program worst-case proof; host replays do not prove handset deadlines. No physical success is claimed before this image is booted.

## Manual cold boot

Only use the verified `native-post-elf-mcfg-trace-v1` artifact. Fully power off, enter bootloader with Volume Down + Power, and temporarily boot it with the command in the delivery response. Do not flash. Wait at least a minute, then photograph the entire settled panel, especially request/reply options, client ending and journal rows. No device command is needed during offline preparation.
