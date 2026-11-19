#!/usr/bin/env python3
"""Pin the single observed GLINK request and bounded follow-up response."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "rust/src/pixel_qrtr_servreg.rs").read_text()
WIRE = (ROOT / "rust/src/pixel_servreg_wire.rs").read_text()
DISPLAY = (ROOT / "rust/src/sunfish_modem_smp2p_probe.rs").read_text()

required = [
    "const FOLLOWUP_INTENT_ID: u32 = 5;",
    "const FOLLOWUP_INTENT_BYTES: usize = 64;",
    "const FOLLOWUP_QUERY_GLINK_BYTES: usize = 16 + FOLLOWUP_INTENT_BYTES;",
    "const FOLLOWUP_CAPTURE_BYTES: usize = 96;",
    "const FOLLOWUP_WAIT_MS: u64 = 10_000;",
    "const POST_REPLY_WINDOW_MS: u64 = 10_000;",
    "const POST_REPLY_POLL_LIMIT: usize = 1_200;",
    "pub const PDDUMP_QUERY_NAME: &[u8] = b\"tms/pddump_disabled\";",
    "pub fn parse_pddump_query(bytes: &[u8]) -> Option<Query>",
    "words[..4] == [0x0002_000e, 2, 0x0002_0007, FOLLOWUP_INTENT_BYTES as u32]",
    "FOLLOWUP_INTENT_BYTES as u8",
    "fn answer_followup_intent(",
    "fn capture_followup_packet(",
    "fn answer_followup_query(",
    "fn parse_followup_query_frame(bytes: &[u8]) -> Option<wire::Query>",
    "report.followup_controls_consumed = true;",
    "report.followup_intent_offered = true;",
    "report.followup_tx_consumed = consumed.words[0] == new_head;",
    "report.followup_packet_valid = true;",
    "report.followup_query_valid = true;",
    "report.followup_response_staged = true;",
    "report.followup_response_consumed = consumed.words[0] == new_head;",
    "fn consume_followup_reply_done(",
    "pixel_glink_control::parse_pdr_reply_done(",
    "report.followup_done_consumed = true;",
    "report.followup_done_intent = intent;",
    "consume_followup_reply_done(",
    "d.line(0, \"QRTR PDR AFTER DONE\");",
    "d.line(18,",
    "if cfg!(feature = \"sunfish-qrtr-servreg-pdr-followup\") && r.response_consumed",
    "d.line(24, \"NO THIRD QMI REPLY\");",
    "d.hex(4, \"PDDUMP MATCH\", r.followup_query_valid as u32);",
    "let word_rows = if r.followup_response_consumed {",
    "wire::pdr_empty_response(query, &mut qrtr_response);",
    "observe_after_reply(",
]
missing = [token for token in required if token not in SOURCE + WIRE + DISPLAY]
if missing:
    raise SystemExit("missing follow-up safety invariant(s): " + ", ".join(missing))

proof = {
    "accepted_rx_control_words": ["0x0002000e", "0x00000002", "0x00020007", "0x00000040"],
    "rx_control_bytes_consumed": 16,
    "advertised_local_channel": 1,
    "advertised_remote_request_channel": 2,
    "fresh_local_intent_id": 5,
    "intent_capacity_bytes": 64,
    "followup_rx_snapshot_cap_bytes": 96,
    "followup_wait_ms": 10000,
    "accepted_followup_service_name": "tms/pddump_disabled",
    "accepted_followup_qmi_message_id": "0x0021",
    "accepted_followup_transaction_preserved": True,
    "followup_glink_request_bytes_consumed": 80,
    "followup_empty_map_response_bytes": 72,
    "followup_qmi_response_count": 1,
    "third_qmi_response": False,
    "post_response_observation_ms": 10000,
    "post_response_poll_limit": 1200,
    "post_response_rx_snapshot_cap_bytes": 64,
    "pddump_request_rx_tail_advance_bytes": 80,
    "post_reply_rx_done_rx_tail_advance_bytes": 8,
    "rx_tail_after_post_done_observation_unchanged": True,
    "post_pddump_reply_control": {
        "record": "RX_DONE_W_REUSE",
        "channel": 2,
        "remote_intent": 2,
        "bytes_consumed": 8,
        "consume_only_after_double_copy_and_exact_match": True,
    },
    "post_done_rx_snapshot_cap_bytes": 64,
    "post_done_rx_tail_held": True,
    "private_payload_capacity_bytes": 80,
    "private_guard_bytes_each_side": 16,
}
print(json.dumps(proof, indent=2))
