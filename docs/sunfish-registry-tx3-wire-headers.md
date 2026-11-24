# Transaction 3 bidirectional header capture

Profile: `native-registry-tx3-wire-v1`. Panel: **REGISTRY TX3 WIRE LOG**. This diagnostic follows IMG_9708: the modem consumed transaction 3 promptly, but the matching receipt became visible about 7.4 seconds later. It records the missing intervening protocol sequence rather than changing writer priority.

The optional feature `sunfish-registry-tx3-wire-headers` retains the first 40 and latest 39 accepted/committed headers plus a separate matching receipt. The trace arms at the original Core predecessor's exact full-publication transition and freezes on the normal decoded opaque completion event. Another transaction or a raw matching parameter cannot close it. A generation reset clears the trace. Missing clocks do not prevent capture or freezing.

Rows 8–27 contain four one-line records each (80 positions). Each line is:

```
US DIR CMD CH PARAM BYTES QRTR SOURCE_PORT DEST_PORT TAG DETAIL_A DETAIL_B
```

DIR 1 is committed host TX; 0 is accepted RX. TAG D means DEL_CLIENT named node and port; other decoded tags 0/2/4 are raw QMI flags, followed by transaction and message IDs. TAG F and all-ones fields mean unavailable, not zero. QRTR source/destination ports remain distinct from the node/port named inside DEL_CLIENT. The row OMIT UNPARSED packs omitted-middle count in the high 16 bits and unavailable projections in the low 16 bits, saturating each half. RX may be observed before its normal consume operation; these timestamps are host observations, not wire arrival times.

The observer adds no device reads, validation, FIFO peeks, credit borrowing or scheduler decisions. It reuses the already checked TX record or normally accepted RX scratch buffer, existing header parser and optional CPU clock. CPU work and counter reads can perturb timing. Compile-time bounds limit a scalar header to 128 bytes and the trace to 12 KiB. Selected native stack audit is required again; it is not whole-program worst-case stack proof.

Only successful checked TX and normally accepted RX are retained; failures, rejected/malformed records and earlier partial-publication fragments are outside this history. For a fragmented reply, the opening record is the fragment committing full publication. The compact parser inherits the existing QRTR v1 / bounded-fragment limitations; unknown details stay unavailable. No firmware payload or filenames are retained.

Offline coverage includes actual Adapter matching identity across another transaction, ring wrap, clockless capture, reset, getter without I/O, head/tail loss and freeze, projection boundaries, and the actual dual-buffer renderer. Paired full-I/O replay checks observation-only behavior on finite schedules. Repaint deferral, FIFO checks, advertisement-before-ACK ordering, Core selectors and QRTR credit policy remain unchanged.

For the handset result, send the whole settled panel including its bottom line. The diagnostic does not assert WLAN startup or Wi-Fi functionality before a physical test.
