# Identify the remaining TFTP traffic and ELF milestone

IMG_9710 demonstrated announcement-buffer reuse and a shorter transaction-3 completion interval (7.382 to 4.604 seconds in those separate boots). It also recorded a 72-byte GLINK transfer packet to port 0x49 after that port's close. Its compact QRTR/QMI IDs do not identify the TFTP opcode or filename. Historical port-0x49 server-check captures cannot identify this different boot's port role.

Offline execution of the actual current ServerCheck produces a **39-byte server-check OACK**, which occupies **88 GLINK wire bytes** after the QRTR header and padding. The supported missing-file error is **19 bytes**, occupying **72 wire bytes**. Native DATA chunks are bounded at 256 bytes, so the fixed server-check OACK is unfragmented. Thus this late 72-byte record cannot be that fixed OACK. Missing-file error fits, but other negotiated read replies can vary in length; size alone does not identify the packet. The saved successful Linux boot reads ota_firewall/ruleset and modem_efs, receives negative replies for unavailable files, and proceeds to WLAN firmware requests. Linux/node port numbers vary across boots.

## Capture

The optional `sunfish-registry-tftp-wire` diagnostic retains the successful announcement reuse and its checked empty-RX ACK gate. It changes no FIFO validation, normal grant order, QRTR credit rule, dispatch method or request lifetime. It adds a small scalar projection from the already copied complete GLINK record; it performs no device reads and adds no timestamp sites. Header count/capacity and exact opaque transaction-3 completion freeze remain unchanged (first 40, latest 39, separate matching completion).

Panel: **REGISTRY TFTP IDENTITY**. Existing header columns remain:

```
US DIR CMD CH PARAM WIRE_BYTES QRTR SRC_PORT DST_PORT TAG DETAIL_A DETAIL_B
```

Tag **A** identifies raw TFTP IDs in the known local native TFTP namespace. DETAIL_A high 16 bits are opcode (1 RRQ, 2 WRQ, 3 DATA, 4 ACK, 5 ERROR, 6 OACK). Low 16 bits are block/error code for DATA/ACK/ERROR, or FFFF when that field does not exist. DETAIL_B high byte is the request-path category; low 24 bits are the declared TFTP body length. Categories: 1 server_check.txt, 2 mcfg.tmp, 3 ota_firewall/ruleset, 4 modem_efs, 5 wlanmdsp.mbn suffix, 6 another nonempty request path, 0 unavailable/not a request. These are metadata hints, not successful TFTP service dispatch or application acceptance. No payload or filename text is retained.

Examples: a missing-file response appears as `A 00050001 00000013`; the fixed server-check OACK as `A 0006FFFF 00000027`. Tag D retains DEL_CLIENT named node/port; QMI raw flags 0/2/4 and tag F retain their prior meanings. Direction selects the local port; a modem port that happens to fall in the host namespace cannot misclassify QMI at host port 0x4003. Only complete, declared QRTR-v1 DATA in a single initial GLINK record is summarized; truncated/fragmented/unknown packets retain unavailable details.

The previous tail row becomes **TAIL ELF R Q P D**, followed by five compact timestamps in that order: transaction-3 peer-tail crossing, ELF request receive, ELF reply queue, full publication and completion. These use already retained CPU-side owner observations. ELF values remain live after the header window freezes. Completion means the service-registry ELF-locator reply completed its existing retirement conditions, not WLAN firmware loading. Missing clocks/events display FFFFFFFF. All 80 header positions and WLAN/WLFW counters remain visible.

## Offline validation and next handset result

The actual server/catalog and projection test covers server-check WRQ/OACK, missing-file RRQ/ERROR, DATA/ACK/error numbers, local-versus-peer namespace, malformed lengths, unknown opcodes, fragments and absent clocks. It confirms 39/88 and 19/72 payload/wire lengths without assuming a role for the phone's packet. The actual renderer checks tag A alongside QMI/DEL_CLIENT/unknown records, all 80 positions, the five tail/ELF timestamps, missing ELF/window/clocks and both framebuffers. Existing reuse/ACK-race tests and full driver/Core suites remain required.

The native image additionally requires selected-stack, capability mutation, packed initializer/Normal-NC maps, generated physical mapping, compiled table-pair and byte-for-byte loader reproduction checks. Pinned SDK components are reused, not recompiled. These checks do not establish handset timing or all interleavings.

The next physical result should resolve which request and reply used port 0x49, whether the incoming packet was RRQ/WRQ/DATA, and where ELF reached after the faster transaction-3 completion. This is a targeted classification/outcome capture, not another speculative writer-priority change. Private profile: `native-registry-tftp-identity-v1`. No handset action is performed during preparation.
