# First OACK spare-buffer experiment

The latest handset proves the sole host-Available intent was already carrying the original client's exact WRQ retry. OACK was read before close arrival, but the later 108-byte grant ACK was not. Test whether a newly advertised receive buffer before OACK permits DATA without waiting for that grant.

User instructions authorize continuing offline through a useful verified cold-boot image without additional design questions. This is a bounded experimental policy, not a proven WLAN fix.

Approaches considered: keeping receive target 3 or 4 during the first transfer adds broad replenishment work and sometimes delays OACK. Reordering the 108-byte grant and OACK did not recover the measured window. Select one fresh advertisement on an ordinary writer selection specifically before the original port-0x1f OACK, then allow at most one normal checked writer call. The allocation, normal selector and all existing backend validations remain.

Use only an existing Advertising or Free slot in RX_SLOTS=8. Do not add a pool, release a held request, borrow a Control slot, alter receipts, change QRTR credit, or prime another peer. Stop priming after the first committed fresh advertisement and skip permanently after a 108-byte grant has been decoded during this first transfer. Existing forced FirstGrantReply opportunities retain their original DATA-only contract and cannot be consumed by this priming. A failed advertisement cannot open a follow-up. An uncertain effect still quarantines. The follow-up uses normal arbitration; it does not force DATA or loop after a blocked call. A processed original-client close prevents the extra turn.

Capture the exact primed ID, its advertisement head/read, OACK head/read, matching 108-byte ACK read, close-arrival bounds, use of that primed ID in already-pending RX, original DATA/final ACK and WLAN/WLFW outcomes. Do not conflate RX-tail consumption with a GLINK return. Label missing evidence. Preserve existing scheduling and pool sizes apart from the scoped advertisement and single follow-up.

Offline acceptance includes normal/blocked/uncertain/closed/full-pool cases and independent peer controls that can use existing buffers, require a fresh buffer, or wait for the 108-byte ACK. The fresh-buffer dependency is a fixture assumption. Its successful DATA/final-ACK exchange does not identify the modem dependency. Full target, payload, boot-layout, stack and 280-mutation capability verification are required before declaring an image ready.
