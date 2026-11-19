import unittest

from platforms.sunfish.msa_ownership_model import Region, plan_msa_handoff
from platforms.sunfish.wlfw_protocol_model import (
    GLINK_CMD_OPEN,
    GLINK_CMD_TX_DATA,
    GLINK_CMD_TX_DATA_CONT,
    GlinkFrameError,
    GlinkIntent,
    GlinkReassembler,
    GlinkTransportGate,
    GlinkTransportGateError,
    GlinkTransportState,
    QMI_RESPONSE,
    QmiMessage,
    QmiFrameError,
    ServiceEndpoint,
    ServiceState,
    StartupStage,
    WlfwBrokerError,
    WlfwBrokerModel,
    WlfwPolicyQueue,
    WlfwPolicyQueueError,
    WlfwRequestTicket,
    WlfwServiceModel,
    WlfwStartupModel,
    decode_capability_response,
    decode_capability_details,
    decode_indication_registration_response,
    decode_indication_registration_details,
    decode_wlfw_status_response,
    decode_msa_info_response,
    decode_msa_ready_response,
    encode_capability_request,
    encode_glink_intent,
    encode_glink_intent_request,
    encode_glink_open,
    encode_glink_open_ack,
    encode_glink_version,
    encode_indication_registration_request,
    encode_msa_info_request,
    encode_msa_ready_request,
    encode_wlfw_qrtr_lookup,
    glink_ring_available,
    glink_ring_peek,
    parse_qrtr_packet,
    parse_qmi_message,
    parse_glink_data_chunk,
    parse_glink_intents,
    parse_glink_record,
    glink_record_size,
    parse_wlfw_qrtr_server_event,
)


def qmi_frame(message_type, transaction_id, message_id, payload):
    return (
        bytes([message_type])
        + transaction_id.to_bytes(2, "little")
        + message_id.to_bytes(2, "little")
        + len(payload).to_bytes(2, "little")
        + payload
    )


class QmiFramingTests(unittest.TestCase):
    def test_parse_message_and_multiple_tlvs(self):
        payload = b"\x02\x02\x00\x00\x00\x10\x03\x00abc"
        parsed = parse_qmi_message(qmi_frame(QMI_RESPONSE, 0x1234, 0x2D, payload))
        self.assertEqual(parsed.message_type, QMI_RESPONSE)
        self.assertEqual(parsed.transaction_id, 0x1234)
        self.assertEqual(parsed.message_id, 0x2D)
        self.assertEqual(parsed.tlvs, ((2, b"\x00\x00"), (0x10, b"abc")))

    def test_rejects_short_header(self):
        with self.assertRaisesRegex(QmiFrameError, "truncated QMI header"):
            parse_qmi_message(b"\x02\x00")

    def test_rejects_invalid_type(self):
        with self.assertRaisesRegex(QmiFrameError, "message type"):
            parse_qmi_message(qmi_frame(3, 1, 1, b""))

    def test_rejects_declared_length_mismatch(self):
        frame = qmi_frame(QMI_RESPONSE, 1, 1, b"abc")
        with self.assertRaisesRegex(QmiFrameError, "does not match"):
            parse_qmi_message(frame[:-1])

    def test_rejects_truncated_tlv_header(self):
        with self.assertRaisesRegex(QmiFrameError, "truncated QMI TLV"):
            parse_qmi_message(qmi_frame(QMI_RESPONSE, 1, 1, b"\x01\x00"))

    def test_rejects_tlv_overrun(self):
        with self.assertRaisesRegex(QmiFrameError, "extends beyond"):
            parse_qmi_message(qmi_frame(QMI_RESPONSE, 1, 1, b"\x01\x02\x00x"))

    def test_enforces_payload_limit(self):
        frame = qmi_frame(QMI_RESPONSE, 1, 1, b"abc")
        with self.assertRaisesRegex(QmiFrameError, "configured bound"):
            parse_qmi_message(frame, max_payload=2)

    def test_encode_msa_info_request(self):
        frame = encode_msa_info_request(0x1234, 0x98D00000, 0x200000)
        parsed = parse_qmi_message(frame)
        self.assertEqual((parsed.message_type, parsed.transaction_id, parsed.message_id), (0, 0x1234, 0x2D))
        self.assertEqual(
            parsed.tlvs,
            ((0x01, (0x98D00000).to_bytes(8, "little")), (0x02, (0x200000).to_bytes(4, "little"))),
        )

    def test_encode_msa_request_rejects_out_of_range_fields(self):
        with self.assertRaises(ValueError):
            encode_msa_info_request(0x10000, 0, 1)
        with self.assertRaises(ValueError):
            encode_msa_info_request(1, -1, 1)
        with self.assertRaises(ValueError):
            encode_msa_info_request(1, 0, 0x1_0000_0000)

    def test_encode_remaining_startup_requests(self):
        registration = parse_qmi_message(encode_indication_registration_request(5))
        self.assertEqual(registration.message_id, 0x20)
        self.assertEqual(
            registration.tlvs,
            (
                (0x10, b"\x01"),
                (0x13, b"\x01"),
                (0x14, b"\x01"),
                (0x15, (0x4B4E454C).to_bytes(4, "little")),
            ),
        )
        with_rejuvenate = parse_qmi_message(encode_indication_registration_request(6, rejuvenate=True))
        self.assertEqual(with_rejuvenate.tlvs[-1], (0x19, b"\x01\x00\x00\x00"))
        self.assertEqual(parse_qmi_message(encode_msa_ready_request(7)).message_id, 0x2E)
        self.assertEqual(parse_qmi_message(encode_capability_request(8)).message_id, 0x24)

    def test_decode_status_response_and_reject_duplicate_common_tlv(self):
        payload = b"\x02\x04\x00\x00\x00\x00\x00\x10\x02\x00xy"
        decoded = decode_wlfw_status_response(
            qmi_frame(QMI_RESPONSE, 11, 0x2E, payload),
            expected_transaction_id=11,
            expected_message_id=0x2E,
        )
        self.assertEqual((decoded.result, decoded.error), (0, 0))
        self.assertEqual(decoded.optional_tlvs, ((0x10, b"xy"),))
        duplicate = qmi_frame(QMI_RESPONSE, 11, 0x2E, payload[:7] + payload[:7])
        with self.assertRaisesRegex(QmiFrameError, "duplicate QMI common"):
            decode_wlfw_status_response(
                duplicate, expected_transaction_id=11, expected_message_id=0x2E
            )


    def test_decode_startup_response_types_with_schema_limits(self):
        ok = b"\x02\x04\x00\x00\x00\x00\x00"
        self.assertEqual(
            decode_msa_ready_response(
                qmi_frame(QMI_RESPONSE, 2, 0x2E, ok), expected_transaction_id=2
            ).result,
            0,
        )
        self.assertEqual(
            decode_indication_registration_response(
                qmi_frame(QMI_RESPONSE, 3, 0x20, ok), expected_transaction_id=3
            ).result,
            0,
        )
        self.assertEqual(
            decode_capability_response(
                qmi_frame(QMI_RESPONSE, 4, 0x24, ok), expected_transaction_id=4
            ).result,
            0,
        )
        oversized_ready = qmi_frame(QMI_RESPONSE, 2, 0x2E, ok + b"\x10\x01\x00\x00")
        with self.assertRaisesRegex(QmiFrameError, "configured bound"):
            decode_msa_ready_response(oversized_ready, expected_transaction_id=2)

    def test_decode_capability_optional_fields(self):
        common = b"\x02\x04\x00\x00\x00\x00\x00"
        payload = (
            common
            + b"\x10\x08\x00" + (0x3990).to_bytes(4, "little") + (0x1234).to_bytes(4, "little")
            + b"\x11\x04\x00" + (0x4A).to_bytes(4, "little")
            + b"\x12\x04\x00" + (0x123456).to_bytes(4, "little")
            + b"\x13\x09\x00" + (0x10203).to_bytes(4, "little") + b"\x04test"
            + b"\x14\x04\x00\x03abc"
            + b"\x15\x01\x00\x02"
            + b"\x16\x02\x00xy"
        )
        response = decode_capability_details(
            qmi_frame(QMI_RESPONSE, 21, 0x24, payload), expected_transaction_id=21
        )
        self.assertEqual(response.chip_info, (0x3990, 0x1234))
        self.assertEqual((response.board_id, response.soc_id), (0x4A, 0x123456))
        self.assertEqual((response.firmware_version, response.firmware_timestamp), (0x10203, "test"))
        self.assertEqual((response.firmware_build_id, response.number_of_macs), ("abc", 2))
        self.assertEqual(response.unknown_optional_tlvs, ((0x16, b"xy"),))

    def test_capability_rejects_bad_string_and_duplicate_optional(self):
        common = b"\x02\x04\x00\x00\x00\x00\x00"
        bad_string = qmi_frame(QMI_RESPONSE, 22, 0x24, common + b"\x14\x02\x00\x05x")
        with self.assertRaisesRegex(QmiFrameError, "string length"):
            decode_capability_details(bad_string, expected_transaction_id=22)
        duplicate = qmi_frame(
            QMI_RESPONSE, 22, 0x24, common + b"\x15\x01\x00\x01\x15\x01\x00\x02"
        )
        with self.assertRaisesRegex(QmiFrameError, "duplicate optional"):
            decode_capability_details(duplicate, expected_transaction_id=22)

    def test_indication_registration_fw_status_bits_are_decoded(self):
        common = b"\x02\x04\x00\x00\x00\x00\x00"
        fw_status = (0x06).to_bytes(8, "little")
        response = decode_indication_registration_details(
            qmi_frame(QMI_RESPONSE, 23, 0x20, common + b"\x10\x08\x00" + fw_status),
            expected_transaction_id=23,
        )
        self.assertEqual((response.result, response.fw_status), (0, 0x06))

    def test_decode_msa_info_response(self):
        records = (
            (0x98D00000).to_bytes(8, "little")
            + (0x4000).to_bytes(4, "little")
            + b"\x01"
            + (0x98D04000).to_bytes(8, "little")
            + (0x1FC000).to_bytes(4, "little")
            + b"\x00"
        )
        payload = b"\x02\x04\x00\x00\x00\x00\x00" + b"\x03\x1b\x00\x02" + records
        response = decode_msa_info_response(qmi_frame(QMI_RESPONSE, 7, 0x2D, payload), expected_transaction_id=7)
        self.assertEqual((response.result, response.error), (0, 0))
        self.assertEqual(
            [(region.address, region.size, region.secure) for region in response.regions],
            [(0x98D00000, 0x4000, True), (0x98D04000, 0x1FC000, False)],
        )
        # Keep wire decoding separate from policy; feed its result to the
        # offline planner that enforces pool bounds and non-overlap.
        plan = plan_msa_handoff(
            0x98D00000,
            0x200000,
            [Region(r.address, r.size, int(r.secure)) for r in response.regions],
        )
        self.assertFalse(plan["executes_scm"])
        self.assertEqual(len(plan["requests"]), 2)

    def test_msa_response_checks_transaction_and_region_flag(self):
        common = b"\x02\x04\x00\x00\x00\x00\x00"
        empty_regions = b"\x03\x01\x00\x00"
        response = qmi_frame(QMI_RESPONSE, 2, 0x2D, common + empty_regions)
        with self.assertRaisesRegex(QmiFrameError, "transaction id"):
            decode_msa_info_response(response, expected_transaction_id=3)

        record = (1).to_bytes(8, "little") + (1).to_bytes(4, "little") + b"\x02"
        invalid_flag = qmi_frame(QMI_RESPONSE, 2, 0x2D, common + b"\x03\x0e\x00\x01" + record)
        with self.assertRaisesRegex(QmiFrameError, "secure_flag"):
            decode_msa_info_response(invalid_flag, expected_transaction_id=2)

    def test_msa_response_rejects_too_many_regions(self):
        common = b"\x02\x04\x00\x00\x00\x00\x00"
        too_many = qmi_frame(QMI_RESPONSE, 2, 0x2D, common + b"\x03\x01\x00\x03")
        with self.assertRaisesRegex(QmiFrameError, "more MSA regions"):
            decode_msa_info_response(too_many, expected_transaction_id=2)

    def test_msa_failure_response_preserves_remote_error_without_regions(self):
        common_failure = b"\x02\x04\x00\x01\x00\x3a\x00"
        response = decode_msa_info_response(
            qmi_frame(QMI_RESPONSE, 9, 0x2D, common_failure), expected_transaction_id=9
        )
        self.assertEqual((response.result, response.error, response.regions), (1, 0x3A, ()))


class GlinkFramingTests(unittest.TestCase):
    def test_ring_snapshot_checks_indices_and_wraps_reads(self):
        fifo = bytes(range(16))
        self.assertEqual(glink_ring_available(len(fifo), head=4, tail=12), 8)
        self.assertEqual(glink_ring_peek(fifo, head=4, tail=12, offset=0, count=8),
                         bytes([12, 13, 14, 15, 0, 1, 2, 3]))
        with self.assertRaisesRegex(GlinkFrameError, "exceeds"):
            glink_ring_peek(fifo, head=4, tail=12, offset=7, count=2)
        with self.assertRaisesRegex(GlinkFrameError, "index"):
            glink_ring_available(len(fifo), head=17, tail=0)

    @staticmethod
    def data_record(command, channel_id, intent_id, chunk, left_size):
        raw = (
            command.to_bytes(2, "little")
            + channel_id.to_bytes(2, "little")
            + intent_id.to_bytes(4, "little")
            + len(chunk).to_bytes(4, "little")
            + left_size.to_bytes(4, "little")
            + chunk
        )
        return parse_glink_record(raw.ljust((len(raw) + 7) & ~7, b"\0"))

    def test_version_and_open_records(self):
        version = parse_glink_record(encode_glink_version(features=2))
        self.assertEqual((version.command, version.param1, version.param2), (0, 1, 2))
        ack = parse_glink_record(encode_glink_version(features=2, ack=True))
        self.assertEqual(ack.command, 1)
        open_record = parse_glink_record(encode_glink_open(7, "IPCRTR"))
        self.assertEqual((open_record.command, open_record.param1, open_record.param2), (GLINK_CMD_OPEN, 7, 7))
        self.assertEqual(open_record.body, b"IPCRTR\0")
        self.assertEqual(parse_glink_record(encode_glink_open_ack(9)).param1, 9)

    def test_stream_length_probe_handles_partial_and_coalesced_records(self):
        first = encode_glink_open(7, "IPCRTR")
        second = encode_glink_open_ack(9)
        stream = first + second
        self.assertIsNone(glink_record_size(stream[:5]))
        self.assertIsNone(glink_record_size(stream[: len(first) - 1]))
        self.assertEqual(glink_record_size(stream), len(first))
        self.assertEqual(glink_record_size(stream[len(first) :]), len(second))
        oversized_open = bytearray(first)
        oversized_open[4:8] = (0xFFFFFFFF).to_bytes(4, "little")
        with self.assertRaisesRegex(GlinkFrameError, "name length"):
            glink_record_size(bytes(oversized_open))

    def test_open_name_limits_and_malformed_records(self):
        with self.assertRaises(ValueError):
            encode_glink_open(1, "")
        with self.assertRaises(ValueError):
            encode_glink_open(1, "x" * 32)
        with self.assertRaises(GlinkFrameError):
            parse_glink_record(b"\x02\x00\x01\x00\x07\x00\x00\x00IPCRTR\0")
        bad_name = bytearray(encode_glink_open(1, "IPCRTR"))
        bad_name[14] = ord("X")
        with self.assertRaisesRegex(GlinkFrameError, "NUL-terminated"):
            parse_glink_record(bytes(bad_name))

    def test_intent_batch_roundtrip_and_limits(self):
        intents = (GlinkIntent(0x800, 1), GlinkIntent(0x4400, 9))
        record = parse_glink_record(encode_glink_intent(4, intents))
        self.assertEqual(record.param1, 4)
        self.assertEqual(parse_glink_intents(record), intents)
        request = parse_glink_record(encode_glink_intent_request(4, 0x2000))
        self.assertEqual((request.command, request.param1, request.param2), (7, 4, 0x2000))
        with self.assertRaises(ValueError):
            encode_glink_intent(1, ())
        malformed = bytearray(encode_glink_intent(1, intents))
        malformed[4:8] = (3).to_bytes(4, "little")
        with self.assertRaisesRegex(GlinkFrameError, "count"):
            parse_glink_record(bytes(malformed))

    def test_tx_data_chunk_roundtrip_and_bounds(self):
        payload = b"qrtr-packet"
        raw = (
            GLINK_CMD_TX_DATA.to_bytes(2, "little")
            + (3).to_bytes(2, "little")
            + (8).to_bytes(4, "little")
            + len(payload).to_bytes(4, "little")
            + (0).to_bytes(4, "little")
            + payload
        )
        raw = raw.ljust((len(raw) + 7) & ~7, b"\0")
        chunk = parse_glink_data_chunk(parse_glink_record(raw))
        self.assertEqual((chunk.channel_id, chunk.intent_id, chunk.chunk, chunk.left_size), (3, 8, payload, 0))
        self.assertFalse(chunk.continuation)
        bad = bytearray(raw)
        bad[8:12] = (0x2001).to_bytes(4, "little")
        with self.assertRaisesRegex(GlinkFrameError, "configured bound"):
            parse_glink_record(bytes(bad))
        with self.assertRaises(GlinkFrameError):
            parse_glink_data_chunk(parse_glink_record(raw), max_message=4)

    def test_bounded_data_reassembly_and_sequence_validation(self):
        assembler = GlinkReassembler(max_message=64)
        first = self.data_record(GLINK_CMD_TX_DATA, 3, 8, b"hello ", 5)
        final = self.data_record(GLINK_CMD_TX_DATA_CONT, 3, 8, b"world", 0)
        self.assertIsNone(assembler.feed(first))
        self.assertTrue(assembler.pending)
        self.assertEqual(assembler.feed(final), b"hello world")
        self.assertFalse(assembler.pending)
        with self.assertRaisesRegex(GlinkFrameError, "no initial"):
            assembler.feed(final)

        self.assertIsNone(assembler.feed(first))
        wrong_remainder = self.data_record(GLINK_CMD_TX_DATA_CONT, 3, 8, b"world", 1)
        with self.assertRaisesRegex(GlinkFrameError, "remaining-byte count"):
            assembler.feed(wrong_remainder)
        self.assertFalse(assembler.pending)

    def test_rejects_unknown_commands_and_wrong_record_alignment(self):
        with self.assertRaisesRegex(GlinkFrameError, "unknown"):
            parse_glink_record((99).to_bytes(2, "little") + bytes(6))
        with self.assertRaisesRegex(GlinkFrameError, "aligned"):
            parse_glink_record(bytes(9))


class GlinkOwnershipGateTests(unittest.TestCase):
    def test_startup_requires_exclusive_ownership_and_peer_ready_before_indices(self):
        gate = GlinkTransportGate()
        with self.assertRaisesRegex(GlinkTransportGateError, "exclusive AP ownership"):
            gate.claim_exclusive(exclusive_boot_verified=False)
        gate.claim_exclusive(exclusive_boot_verified=True)
        with self.assertRaisesRegex(GlinkTransportGateError, "reset/fresh-boot"):
            gate.confirm_peer_ready(reset_or_fresh_boot_verified=False)
        with self.assertRaises(GlinkTransportGateError):
            gate.initialize_indices()
        gate.confirm_peer_ready(reset_or_fresh_boot_verified=True)
        gate.initialize_indices()
        self.assertEqual(gate.state, GlinkTransportState.RING_READY)
        gate.negotiate_version(1)
        self.assertEqual(gate.state, GlinkTransportState.VERSION_NEGOTIATED)

    def test_version_ack_cannot_stand_in_for_ring_or_peer_ownership(self):
        gate = GlinkTransportGate()
        with self.assertRaisesRegex(GlinkTransportGateError, "ring must be ready"):
            gate.negotiate_version(1)
        gate.claim_exclusive(exclusive_boot_verified=True)
        gate.confirm_peer_ready(reset_or_fresh_boot_verified=True)
        gate.initialize_indices()
        with self.assertRaisesRegex(GlinkTransportGateError, "unsupported"):
            gate.negotiate_version(2)


class WlfwServiceLifecycleTests(unittest.TestCase):
    def test_qrtr_v1_and_v2_packet_headers(self):
        ctrl = b"".join(value.to_bytes(4, "little") for value in (4, 0x45, 1, 5, 700))
        v1 = b"".join(
            value.to_bytes(4, "little")
            for value in (1, 1, 5, 700, 1, len(ctrl), 0, 0xFFFFFFFE)
        ) + ctrl
        parsed_v1 = parse_qrtr_packet(v1)
        self.assertEqual((parsed_v1.version, parsed_v1.packet_type), (1, 1))
        self.assertEqual((parsed_v1.source_node, parsed_v1.source_port), (5, 700))
        self.assertEqual(parsed_v1.payload, ctrl)
        event = parse_wlfw_qrtr_server_event(parsed_v1.payload)
        self.assertEqual((event.service_id, event.node, event.port), (0x45, 5, 700))

        v2_header = (
            bytes([3, 1, 1, 4])
            + len(ctrl).to_bytes(4, "little")
            + (5).to_bytes(2, "little")
            + (700).to_bytes(2, "little")
            + (0).to_bytes(2, "little")
            + (0xFFFE).to_bytes(2, "little")
            + b"\x00" * 4
        )
        parsed_v2 = parse_qrtr_packet(v2_header + ctrl)
        self.assertEqual(parsed_v2.version, 3)
        self.assertTrue(parsed_v2.confirm_rx)
        self.assertEqual(parsed_v2.destination_port, 0xFFFFFFFE)
        self.assertEqual(parse_wlfw_qrtr_server_event(parsed_v2.payload).port, 700)

    def test_qrtr_packet_parser_rejects_malformed_headers_and_lengths(self):
        with self.assertRaisesRegex(QmiFrameError, "truncated QRTR v1"):
            parse_qrtr_packet(b"\x01\x00\x00\x00")
        with self.assertRaisesRegex(QmiFrameError, "unsupported QRTR"):
            parse_qrtr_packet(b"\x02" + b"\x00" * 31)
        v2_bad_flags = bytes([3, 1, 2, 0]) + b"\x00" * 12 + b"\x00" * 4
        with self.assertRaisesRegex(QmiFrameError, "unsupported flags"):
            parse_qrtr_packet(v2_bad_flags)
        v1_wrong_size = b"".join(
            value.to_bytes(4, "little")
            for value in (1, 1, 5, 700, 0, 20, 0, 0)
        )
        with self.assertRaisesRegex(QmiFrameError, "length does not match"):
            parse_qrtr_packet(v1_wrong_size)

    def test_qrtr_lookup_and_matching_new_server_control(self):
        lookup = [int.from_bytes(encode_wlfw_qrtr_lookup()[i : i + 4], "little") for i in range(0, 20, 4)]
        self.assertEqual(lookup, [10, 0x45, 1, 0, 0])
        event = b"".join(value.to_bytes(4, "little") for value in (4, 0x45, 1, 5, 700))
        parsed = parse_wlfw_qrtr_server_event(event)
        self.assertEqual((parsed.service_id, parsed.version, parsed.instance), (0x45, 1, 0))
        self.assertEqual((parsed.node, parsed.port), (5, 700))

    def test_qrtr_event_parser_rejects_wrong_service_command_and_length(self):
        bad_service = b"".join(value.to_bytes(4, "little") for value in (4, 0x46, 1, 5, 700))
        with self.assertRaisesRegex(QmiFrameError, "does not match"):
            parse_wlfw_qrtr_server_event(bad_service)
        wrong_command = b"".join(value.to_bytes(4, "little") for value in (5, 0x45, 1, 5, 700))
        with self.assertRaisesRegex(QmiFrameError, "not NEW_SERVER"):
            parse_wlfw_qrtr_server_event(wrong_command)
        with self.assertRaisesRegex(QmiFrameError, "exactly 20"):
            parse_wlfw_qrtr_server_event(b"\x00" * 19)

    def test_only_exact_wlfw_service_binds(self):
        model = WlfwServiceModel()
        self.assertFalse(model.service_arrived(0x44, 1, 0, 1, 20))
        self.assertFalse(model.service_arrived(0x45, 2, 0, 1, 20))
        self.assertFalse(model.service_arrived(0x45, 1, 1, 1, 20))
        self.assertEqual(model.state, ServiceState.SEARCHING)
        self.assertTrue(model.service_arrived(0x45, 1, 0, 4, 700))
        self.assertEqual(model.state, ServiceState.CONNECTED)
        self.assertEqual((model.endpoint.node, model.endpoint.port), (4, 700))

    def test_rejects_invalid_endpoint_and_ignores_stale_departure(self):
        model = WlfwServiceModel()
        self.assertFalse(model.service_arrived(0x45, 1, 0, 0, 700))
        self.assertFalse(model.service_arrived(0x45, 1, 0, 4, 0))
        self.assertTrue(model.service_arrived(0x45, 1, 0, 4, 700))
        self.assertFalse(model.service_departed(4, 701))
        self.assertEqual(model.state, ServiceState.CONNECTED)
        self.assertTrue(model.service_departed(4, 700))
        self.assertEqual(model.state, ServiceState.SEARCHING)
        self.assertIsNone(model.endpoint)


class WlfwStartupOrderTests(unittest.TestCase):
    def test_observed_startup_order_has_explicit_dma_gate(self):
        model = WlfwStartupModel()
        self.assertTrue(model.begin(service_connected=True, platform_power_ready=True))
        self.assertEqual(model.stage, StartupStage.INDICATION_REGISTRATION)
        self.assertTrue(model.accept_response(0x20, result=0))
        self.assertTrue(model.accept_response(0x2D, result=0, has_msa_regions=True))
        self.assertEqual(model.stage, StartupStage.MSA_OWNERSHIP)
        self.assertTrue(model.confirm_msa_ownership(
            pool_excluded=True, scm_owner_verified=True, smmu_isolated=True
        ))
        self.assertTrue(model.accept_response(0x2E, result=0))
        self.assertTrue(model.accept_response(0x24, result=0))
        self.assertEqual(model.stage, StartupStage.READY)

    def test_startup_stops_when_power_or_transport_is_not_ready(self):
        model = WlfwStartupModel()
        self.assertFalse(model.begin(service_connected=True, platform_power_ready=False))
        self.assertEqual(model.stage, StartupStage.FAILED)

    def test_startup_rejects_out_of_order_response(self):
        model = WlfwStartupModel()
        model.begin(service_connected=True, platform_power_ready=True)
        self.assertFalse(model.accept_response(0x2D, result=0, has_msa_regions=True))
        self.assertEqual(model.stage, StartupStage.FAILED)

    def test_startup_fails_closed_on_unverified_msa_ownership(self):
        model = WlfwStartupModel()
        model.begin(service_connected=True, platform_power_ready=True)
        model.accept_response(0x20, result=0)
        model.accept_response(0x2D, result=0, has_msa_regions=True)
        self.assertFalse(model.confirm_msa_ownership(
            pool_excluded=True, scm_owner_verified=True, smmu_isolated=False
        ))
        self.assertEqual(model.stage, StartupStage.FAILED)

    def test_startup_stops_on_failed_wlfw_result_or_empty_msa(self):
        failed = WlfwStartupModel()
        failed.begin(service_connected=True, platform_power_ready=True)
        self.assertFalse(failed.accept_response(0x20, result=1))
        empty = WlfwStartupModel()
        empty.begin(service_connected=True, platform_power_ready=True)
        empty.accept_response(0x20, result=0)
        self.assertFalse(empty.accept_response(0x2D, result=0, has_msa_regions=False))


class WlfwBrokerModelTests(unittest.TestCase):
    def connected(self):
        model = WlfwBrokerModel()
        self.assertTrue(model.service_arrived(0x45, 1, 0, 5, 700))
        return model

    def test_broker_emits_only_typed_startup_requests_and_fixed_msa_pool(self):
        model = self.connected()
        with self.assertRaisesRegex(WlfwBrokerError, "not connected"):
            WlfwBrokerModel().request_capability(now_ms=0)

        registration = model.request_indication_registration(now_ms=10)
        self.assertEqual(registration.transaction_id, 1)
        self.assertEqual(registration.message_id, 0x20)
        self.assertEqual((registration.endpoint.node, registration.endpoint.port), (5, 700))
        with self.assertRaisesRegex(WlfwBrokerError, "another WLFW transaction"):
            model.request_capability(now_ms=11)

        response = model.receive_response(
            5, 700, qmi_frame(QMI_RESPONSE, 1, 0x20, b""), now_ms=12
        )
        self.assertEqual((response.transaction_id, response.message_id), (1, 0x20))

        msa = model.request_msa_info(now_ms=13)
        parsed = parse_qmi_message(msa.wire_bytes)
        self.assertEqual(msa.transaction_id, 2)
        self.assertEqual(
            parsed.tlvs,
            ((1, (0x98D00000).to_bytes(8, "little")),
             (2, (0x200000).to_bytes(4, "little"))),
        )

    def test_broker_binds_responses_to_endpoint_and_transaction(self):
        model = self.connected()
        ticket = model.request_capability(now_ms=100)
        response = qmi_frame(QMI_RESPONSE, ticket.transaction_id, 0x24, b"")
        with self.assertRaisesRegex(WlfwBrokerError, "time is invalid"):
            model.receive_response(5, 700, response, now_ms=99)
        with self.assertRaisesRegex(WlfwBrokerError, "unexpected endpoint"):
            model.receive_response(6, 700, response, now_ms=101)
        with self.assertRaisesRegex(WlfwBrokerError, "does not match"):
            model.receive_response(5, 700, qmi_frame(QMI_RESPONSE, 9, 0x24, b""), now_ms=101)
        self.assertIsNotNone(model.pending)
        parsed = model.receive_response(5, 700, response, now_ms=102)
        self.assertEqual(parsed.message_id, 0x24)
        self.assertIsNone(model.pending)

    def test_broker_times_out_without_retry_and_discards_late_reply(self):
        model = self.connected()
        ticket = model.request_msa_ready(now_ms=20, timeout_ms=100)
        self.assertIsNone(model.expire(now_ms=119))
        expired = model.expire(now_ms=120)
        self.assertEqual(expired, ticket)
        self.assertIsNone(model.pending)
        with self.assertRaisesRegex(WlfwBrokerError, "no connected endpoint"):
            model.receive_response(
                5, 700, qmi_frame(QMI_RESPONSE, ticket.transaction_id, 0x2E, b""),
                now_ms=121,
            )

    def test_service_departure_cancels_pending_and_new_session_resets_ids(self):
        model = self.connected()
        old = model.request_indication_registration(now_ms=0)
        self.assertFalse(model.service_arrived(0x45, 1, 0, 9, 999))
        self.assertEqual(model.endpoint.node, old.endpoint.node)
        self.assertFalse(model.service_departed(5, 701))
        self.assertTrue(model.service_departed(5, 700))
        self.assertIsNone(model.pending)
        self.assertFalse(model.service_arrived(0x45, 1, 0, 0, 700))
        self.assertTrue(model.service_arrived(0x45, 1, 0, 8, 900))
        new = model.request_capability(now_ms=1)
        self.assertEqual(new.transaction_id, 1)
        with self.assertRaisesRegex(WlfwBrokerError, "stale or unexpected endpoint"):
            model.receive_response(
                old.endpoint.node, old.endpoint.port,
                qmi_frame(QMI_RESPONSE, 1, 0x24, b""), now_ms=2,
            )


class WlfwPolicyQueueTests(unittest.TestCase):
    endpoint = ServiceEndpoint(5, 700)

    @staticmethod
    def completion(transaction_id, *, message_id=0x20, started_ms=0):
        request = WlfwRequestTicket(
            ServiceEndpoint(5, 700), transaction_id, message_id,
            qmi_frame(0, transaction_id, message_id, b""),
            started_ms, started_ms + 100,
        )
        response = QmiMessage(
            QMI_RESPONSE, transaction_id, message_id, ((0x02, b"\0\0\0\0"),)
        )
        return request, response

    def test_queue_is_bounded_and_overflow_preserves_existing_entries(self):
        queue = WlfwPolicyQueue()
        queue.bind(self.endpoint)
        session = queue.session_id
        for transaction_id in range(1, queue.CAPACITY + 1):
            request, response = self.completion(transaction_id)
            queue.publish(self.endpoint, session, request, response, received_ms=1)
        self.assertEqual(queue.depth, queue.CAPACITY)
        request, response = self.completion(queue.CAPACITY + 1)
        with self.assertRaisesRegex(WlfwPolicyQueueError, "full"):
            queue.publish(self.endpoint, session, request, response, received_ms=1)
        self.assertEqual(queue.depth, queue.CAPACITY)
        self.assertEqual(
            [queue.receive().request.transaction_id for _ in range(queue.CAPACITY)],
            [1, 2, 3, 4],
        )
        self.assertIsNone(queue.receive())

    def test_queue_matches_endpoint_ticket_response_and_deadline(self):
        queue = WlfwPolicyQueue()
        queue.bind(self.endpoint)
        session = queue.session_id
        request, response = self.completion(1, started_ms=10)
        with self.assertRaisesRegex(WlfwPolicyQueueError, "stale endpoint"):
            queue.publish(ServiceEndpoint(6, 700), session, request, response, received_ms=11)
        with self.assertRaisesRegex(WlfwPolicyQueueError, "request window"):
            queue.publish(self.endpoint, session, request, response, received_ms=110)
        wrong_response = QmiMessage(QMI_RESPONSE, 2, 0x20, response.tlvs)
        with self.assertRaisesRegex(WlfwPolicyQueueError, "does not match"):
            queue.publish(self.endpoint, session, request, wrong_response, received_ms=11)
        queue.publish(self.endpoint, session, request, response, received_ms=11)
        item = queue.receive()
        self.assertEqual((item.request.transaction_id, item.response.transaction_id), (1, 1))
        with self.assertRaisesRegex(WlfwPolicyQueueError, "duplicate"):
            queue.publish(self.endpoint, session, request, response, received_ms=12)

    def test_queue_caps_each_typed_response_and_flushes_on_departure(self):
        queue = WlfwPolicyQueue()
        queue.bind(self.endpoint)
        session = queue.session_id
        request, response = self.completion(1)
        oversized = QmiMessage(
            QMI_RESPONSE, 1, 0x20,
            ((0x02, b"\0\0\0\0"), (0x10, bytes(12))),
        )
        with self.assertRaisesRegex(WlfwPolicyQueueError, "typed operation bound"):
            queue.publish(self.endpoint, session, request, oversized, received_ms=1)
        queue.publish(self.endpoint, session, request, response, received_ms=1)
        self.assertFalse(queue.service_departed(ServiceEndpoint(5, 701)))
        self.assertTrue(queue.service_departed(self.endpoint))
        self.assertEqual(queue.depth, 0)
        self.assertIsNone(queue.receive())
        queue.bind(ServiceEndpoint(8, 900))
        new_session = queue.session_id
        next_request, next_response = self.completion(1)
        next_request = WlfwRequestTicket(
            ServiceEndpoint(8, 900), next_request.transaction_id,
            next_request.message_id, next_request.wire_bytes,
            next_request.started_ms, next_request.deadline_ms,
        )
        queue.publish(
            ServiceEndpoint(8, 900), new_session, next_request, next_response,
            received_ms=1,
        )
        self.assertEqual(queue.depth, 1)
        with self.assertRaisesRegex(WlfwPolicyQueueError, "stale endpoint session"):
            queue.publish(
                ServiceEndpoint(8, 900), session, next_request, next_response,
                received_ms=1,
            )


if __name__ == "__main__":
    unittest.main()
