# IMG_9805: fast registry path, ordinary ELF capacity retry

Settled **READY HANDOFF POOL**, delivered source `e51b4109d96af5c7ecd24b15f2a5261be7f39ff3`, image SHA256 `f21e07d69ab9df396b2a4798c755f5636084bcd521259bc8e2ef53586f125e48`. Raw HEIC, full PNG, readable PNG, manual row transcription, decoder and hashes are stored separately under the immutable profile's `device-result/`. All86 delivered artifact hashes were reverified unchanged. No device commands were run. Runtime error/detail0; close cleanup stage6.

TX3 queue/publication/Core completion `012F849B / 01338588 / 01724695`: queue to publication **262.381ms**. Both TX3 and TX4 have frozen snapshots2, publication masks `3FA4`, calls3 and4. They used the existing converted-grant path; no new bits19/20. The separate exact-blocker handoff flags/arms/receipts/services/result are all0. Neither new opportunity was exercised. This fast path resembles IMG_9802 and must not be credited as a controlled improvement caused by the new handoffs.

ELF receive/Owner queue/publication/Owner completion `014BB3DE / 016B2B69 / 016D2834 / 017C49E1`: prequeue2,062.219ms, queue to publication130.251ms, publication to completion991.661ms, total **3,184.131ms**. These are host observations, not modem execution times. Three actual full-pool captures; only slot7 differs between first/latest full snapshots (`changed=80`). First exact release `01699397`, slot1/cause1 GLINK RX_DONE/empty mask02. It released the fully published TFTP ERROR from host5003 to modem21, intent0. ELF used the same slot1; Owner queue followed release by104.402ms. Capacity attempt flags0F/result1/arms1/full skips2. Original lifetime completion turn used0, new handoff0: normal capacity retry worked.

## Eight owners at the latest full snapshot

Ports/IDs hexadecimal. These are retained blocked snapshots, not settled ownership. First-free route nodes are explicitly host1→modem0. Other routes are identified by local service ports, not reconstructed from a displayed node field.

| Slot | Packet | Source→destination port | Bytes sent/length | Remote intent |
| --- | --- | --- | --- | --- |
| 0 | QRTR RESUME_TX | 5000→22 | 52/52 | 2 |
| 1 | TFTP ERROR opcode5 | 5003→21 | 52/52 | 0 |
| 2 | TFTP ERROR opcode5 | 5002→22 | 52/52 | 1 |
| 3 | TFTP ERROR opcode5 | 5004→23 | 52/52 | 3 |
| 4 | IPA response transaction2/message0035 | 4002→20 | 0/48 | unavailable |
| 5 | Registry response transaction3/message0021 | 4003→9 | 88/88 | 4 |
| 6 | Registry response transaction4/message0021 | 4003→9 | 224/224 | 10 |
| 7 | QRTR RESUME_TX | 4003→9 | 52/52 | 5 |

Slot7 was initially unsent without a remote intent. All other opening/latest records match. Seven packets are published at the latest snapshot, one IPA response unstarted. Local QRTR retirement conditions are set; no record is GLINK-complete and merely waiting for the local condition. Published owners cannot safely retire based on peer-tail consumption alone. ERROR opcode5 does not identify the filename/error code. The current capture does not prove that the IPA0035 response remained outstanding after this snapshot; settled pending IPA message is unavailable and the historical completion latch is1.

Original final ACK completed `00DA82FB`. EFS pair `0001017F` records an overlap offered/admitted, not its completion time. WLAN RRQ0/WLFW0 and PD7FFFFFFF. No WLAN startup milestone changed.

## Reconcile before probing again

[IMG_9726](sunfish-elf-capacity-retry-IMG_9726.md) already exposed an unsent IPA0035 response in an equivalent eight-owner snapshot. [IMG_9728](sunfish-post-elf-outcome-IMG_9728.md) then showed that response was transient: live IPA flags1F, one registration/driver request, four publications, no pending/held/failed record; ELF host credit completed. Its post-ELF request was MCFG, not WLAN. This is evidence from another boot, not proof of the current boot's eventual state.

The next image should retain the ready-handoff policy and reuse existing outcome captures to connect **this boot's early scheduling** to live IPA completion, original ACK, both EFS endings, exact ELF credit and first accepted post-ELF request. Do not repeat the eight-slot inventory or assume early retirement is necessary. It is a diagnostic integration, not another scheduling fix or a promise of working Wi-Fi.
