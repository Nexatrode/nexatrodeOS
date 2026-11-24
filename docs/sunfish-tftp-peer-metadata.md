# TFTP peer metadata validation batch

The `WRQ CALLERS STACK SPLIT` handset result restored runtime startup, with
runtime error zero and complete caller attribution. Its post-return intervals
contained 8, 38 and 31 successful Ring samples: 77 total, costing about 618 ms
within 838 ms from WRQ buffer return to OACK publication. Registration published
about 134 ms before the exact close became visible, but OACK publication and
peer read followed visibility by about 219 ms and 245 ms. IPA completed;
first DATA, WLAN RRQ and WLFW service remained absent. This does not establish
that the earlier stalled screen was a CPU stack fault.

The leading caller sites included Adapter::tftp_datagram and
Adapter::set_tftp_firmware_peers. Owner::work rebuilt close-fence metadata every
receive-loop iteration: each held request received a fresh gate, then installing
the firmware peer list received another. These operations inspect packet bytes
already copied into Session-owned receive slots. No device read or callback
intervenes between their metadata reads and list installation.

The opt-in `sunfish-tftp-peer-metadata-batch` feature groups this work in
Adapter::refresh_tftp_peer_snapshot. It takes one fresh gate, validates every
opaque handle against the current Session owner, generation, slot state and
datagram identity, and builds both peer lists in bounded temporaries. Capacity
or handle errors leave both lists unchanged. It then installs the lists and
returns the existing diagnostic snapshot. No cached health or cursor sample
survives this call.

Request peers precede retained replies. The legacy pending reply retains kind2
and its original exclusion from the firmware list; per-peer replies retain
kind4 and classification by local ports0x5100..0x51ff; active firmware peers
retain kind8. Held packets keep the original RRQ/local-port classification.
The feature-off implementation remains intact.

The bodies of Ring::sample, read_rx_at, peek, consume, write_checked_inner,
Adapter::poll_receive and publish_inner are identical to the prior commit.
Every subsequent FIFO operation takes its existing fresh checks. GLINK intent
advertisement-before-ACK, RX_DONE handling, QRTR credit ordering and fault
quarantine are unchanged. This groups pure metadata validation; it does not
permit reusing a validation across external I/O or across receive-loop turns.

Four actual-adapter tests cover list order/classification, one fresh sample per
call, no packet writes, fresh validation on repeated calls, stale generation,
foreign and retired handles, capacity failure, empty-list clearing and sample
failure quarantine. The retired-handle fixture runs before a new slot occupant;
the unchanged Session identity checks enforce slot reuse safety. Independent
review found no Critical or Important issues.

Offline verification includes563 owner tests with batching,559 with the prior
feature profile,212 link tests, ten fault controls, nine handoff/parser tests,
and24 paired schedules/48 executions/96 modes. The replay requires exact
packet reads/readbacks/writes, head/tail publications and notifications, plus
identical peer-event order. Health/cursor sample totals and sample-clock event
timestamps intentionally differ; these are not claimed as full I/O parity or
handset deadline proofs. Selected stack demand remains4,290,304 bytes with
952,576 bytes remaining in the existing5MiB mapping; this is not whole-program
worst-case stack analysis.

A broad test-discovery command also encountered the pre-existing
sunfish_smmu_handoff_model_test mapping-group assertion: the old snapshot
expects78 while SUNFISH_MAX_SMR_SLOTS is128. Both files are unchanged by this
work. The failure is recorded separately and not counted as a pass; no SMMU
route-policy change is bundled here. The current image's authority and packed
memory mapping audits remain required independently.

The private candidate is
`.cache/sunfish-native-wifi/private/native-wrq-peer-metadata-v1`, titled
`WRQ PEER METADATA BATCH`. It retains the stack split, startup markers1/2/3,
Normal-NC SDK and eight table maps. The image is68,911,104 bytes, SHA-256
`2326f4c29e39a5d039d1ec64fa500b089071543c1574dc3023523aea76e43c52`.
The private manifest binds the frozen caller sources, inputs and verification
artifacts. Prior images remain unchanged. The expected savings are tens of
milliseconds; no claim is made that this alone recovers the full deadline or
starts Wi-Fi. Actual timing and outcomes require the next cold temporary boot.
