# TFTP peer metadata validation plan

User requests offline investigation through a verified cold temporary boot image, on main, preserving FIFO checks and grant ordering. They have opted out of repeated approval questions. Use native execution and one final independent review.

## Design and evidence

The restored caller panel reports 77 samples (8/38/31), about618ms of838ms from WRQ return to OACK publication. Calls0A85 and0A52 repeatedly gate held-packet metadata lookup and firmware-peer assignment. Neither operation reads device packet data: packets are already owned in Session receive slots. All live FIFO transaction checks remain necessary.

Chosen approach: add opt-in Adapter::refresh_tftp_peer_snapshot(generation, held_handles, retained_entries). One fresh gate precedes a bounded, callback-free metadata transaction. Check every held handle against current generation/owner/slot, decode local bytes, build owner and firmware lists in temporaries, then install both and return the diagnostic owner snapshot. Retained entries carry peer, original kind2/4/8 and firmware classification. Keep exact request-before-retained ordering. Reject capacity/invalid handles without partial metadata updates. No validation snapshot survives the call. poll_receive, consume, publish and write_checked remain unchanged with fresh samples. Legacy path stays available.

Rejected alternatives: skip unchanged updates across work-loop iterations (requires cache invalidation across closes/slot reuse); remove FIFO samples or persist successful health validation (weakens transaction boundaries). Grouping local metadata is smaller and easier to prove. Estimated savings are tens of milliseconds, not a promise to beat the entire observed deadline.

## Tasks

- [x] Add failing batch tests for exact lists/order, one fresh sample/no writes, two calls/two samples, stale/foreign/retired handles, capacity, and fault quarantine with atomic state. Implement bounded helper plus opt-in owner branch; compare existing full owner/link/fault behavior and semantic FIFO publication parity with feature off.
- [x] Build separate native-wrq-peer-metadata-v1, retaining handoff split/Normal-NC policy/caller panel with distinct title. Audit selected stack budget and frozen source map; test display,304 capability mutations, packed payload/SDK reproduction and hashes. Preserve original images.
- [x] Final independent review, bind verified artifacts and notes, commit/push main; provide manual temporary cold fastboot command. No device commands by agent.

Review focus: retained reply firmware classification; slot/handle reuse and foreign identity; capacity failure commits neither list; validation failure quarantines before metadata mutation; no stale snapshot reused for subsequent FIFO I/O. All receive/consume/transmit source bodies must be identical to baseline.
