"""Offline-only GLINK, QRTR, QMI framing and WLFW service lifecycle model.

This module deliberately has no socket, MMIO, SMC, or shared-memory access.
It models synthetic GLINK records, QRTR control payloads and QMI messages after
AF_QIPCRTR has demultiplexed the outer QRTR packet.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from enum import Enum, auto


WLFW_SERVICE_ID = 0x45
WLFW_SERVICE_VERSION = 1
WLFW_SERVICE_INSTANCE = 0
QMI_REQUEST = 0
QMI_RESPONSE = 2
QMI_INDICATION = 4
QMI_HEADER_SIZE = 7
MAX_QMI_PAYLOAD = 0xFFFF
WLFW_MSA_INFO_MESSAGE_ID = 0x002D
WLFW_IND_REGISTER_MESSAGE_ID = 0x0020
WLFW_MSA_READY_MESSAGE_ID = 0x002E
WLFW_CAP_MESSAGE_ID = 0x0024
WLFW_CLIENT_ID = 0x4B4E454C
WLFW_MAX_MSA_REGIONS = 2
QRTR_TYPE_NEW_SERVER = 4
QRTR_TYPE_NEW_LOOKUP = 10
QRTR_CTRL_PACKET_SIZE = 20
QRTR_PROTO_VER_1 = 1
QRTR_PROTO_VER_2 = 3
QRTR_V1_HEADER_SIZE = 32
QRTR_V2_HEADER_SIZE = 16
QRTR_PORT_CTRL = 0xFFFFFFFE
QRTR_PORT_CTRL_V2 = 0xFFFE
GLINK_VERSION = 1
GLINK_HEADER_SIZE = 8
GLINK_FIFO_ALIGNMENT = 8
GLINK_NAME_SIZE = 32
GLINK_MAX_TX_CHUNK = 0x2000
GLINK_MAX_MESSAGE = 0x100000
GLINK_CMD_VERSION = 0
GLINK_CMD_VERSION_ACK = 1
GLINK_CMD_OPEN = 2
GLINK_CMD_CLOSE = 3
GLINK_CMD_OPEN_ACK = 4
GLINK_CMD_INTENT = 5
GLINK_CMD_RX_DONE = 6
GLINK_CMD_RX_INTENT_REQ = 7
GLINK_CMD_RX_INTENT_REQ_ACK = 8
GLINK_CMD_TX_DATA = 9
GLINK_CMD_CLOSE_ACK = 11
GLINK_CMD_TX_DATA_CONT = 12
GLINK_CMD_READ_NOTIF = 13
GLINK_CMD_RX_DONE_W_REUSE = 14
GLINK_CMD_SIGNALS = 15


class QmiFrameError(ValueError):
    """Malformed or unsupported QMI wire frame."""


class GlinkFrameError(ValueError):
    """Malformed or unsupported GLINK native-protocol record."""


class GlinkTransportGateError(RuntimeError):
    """A GLINK transport action was attempted before its ownership gates."""


class GlinkTransportState(Enum):
    UNCLAIMED = auto()
    EXCLUSIVE = auto()
    PEER_READY = auto()
    RING_READY = auto()
    VERSION_NEGOTIATED = auto()


class GlinkTransportGate:
    """Policy-only model of the required exclusive-ownership startup gates.

    This does not prove the supplied evidence or touch hardware. A production
    broker must obtain these facts from a trusted boot/reset authority.
    """

    def __init__(self):
        self.state = GlinkTransportState.UNCLAIMED

    def claim_exclusive(self, *, exclusive_boot_verified: bool) -> None:
        if self.state is not GlinkTransportState.UNCLAIMED:
            raise GlinkTransportGateError("GLINK ownership was already claimed")
        if exclusive_boot_verified is not True:
            raise GlinkTransportGateError("exclusive AP ownership is not verified")
        self.state = GlinkTransportState.EXCLUSIVE

    def confirm_peer_ready(self, *, reset_or_fresh_boot_verified: bool) -> None:
        if self.state is not GlinkTransportState.EXCLUSIVE:
            raise GlinkTransportGateError("exclusive GLINK ownership is required first")
        if reset_or_fresh_boot_verified is not True:
            raise GlinkTransportGateError("modem reset/fresh-boot state is not verified")
        self.state = GlinkTransportState.PEER_READY

    def initialize_indices(self) -> None:
        if self.state is not GlinkTransportState.PEER_READY:
            raise GlinkTransportGateError("verified modem peer state is required before ring initialization")
        self.state = GlinkTransportState.RING_READY

    def negotiate_version(self, version: int) -> None:
        if self.state is not GlinkTransportState.RING_READY:
            raise GlinkTransportGateError("GLINK ring must be ready before VERSION negotiation")
        if version != GLINK_VERSION:
            raise GlinkTransportGateError("unsupported GLINK protocol version")
        self.state = GlinkTransportState.VERSION_NEGOTIATED


@dataclass(frozen=True)
class GlinkRecord:
    command: int
    param1: int
    param2: int
    body: bytes


@dataclass(frozen=True)
class GlinkIntent:
    size: int
    intent_id: int


@dataclass(frozen=True)
class GlinkDataChunk:
    channel_id: int
    intent_id: int
    chunk: bytes
    left_size: int
    continuation: bool


def _align_glink(value: int) -> int:
    return (value + GLINK_FIFO_ALIGNMENT - 1) & ~(GLINK_FIFO_ALIGNMENT - 1)


def glink_ring_available(fifo_size: int, *, head: int, tail: int) -> int:
    """Return bytes available in a synthetic circular RX FIFO snapshot.

    This mirrors the matched SMEM backend's head/tail arithmetic but accepts
    only caller-provided integers; it does not read shared indices.
    """
    if fifo_size <= 0 or not 0 <= head <= fifo_size or not 0 <= tail <= fifo_size:
        raise GlinkFrameError("GLINK FIFO size or index is outside its bounds")
    return fifo_size - tail + head if head < tail else head - tail


def glink_ring_peek(fifo: bytes, *, head: int, tail: int,
                    offset: int, count: int) -> bytes:
    """Copy a bounded range from a synthetic ring snapshot without mutation."""
    available = glink_ring_available(len(fifo), head=head, tail=tail)
    if offset < 0 or count < 0 or offset + count > available:
        raise GlinkFrameError("GLINK ring read exceeds the available snapshot")
    start = (tail + offset) % len(fifo)
    first = min(count, len(fifo) - start)
    return fifo[start:start + first] + fifo[:count - first]


def _glink_header(command: int, param1: int, param2: int) -> bytes:
    if not 0 <= command <= 0xFFFF:
        raise ValueError("GLINK command is outside its 16-bit field")
    if not 0 <= param1 <= 0xFFFF:
        raise ValueError("GLINK param1 is outside its 16-bit field")
    if not 0 <= param2 <= 0xFFFFFFFF:
        raise ValueError("GLINK param2 is outside its 32-bit field")
    return command.to_bytes(2, "little") + param1.to_bytes(2, "little") + param2.to_bytes(4, "little")


def encode_glink_version(*, features: int = 0, ack: bool = False) -> bytes:
    """Encode the v1 version exchange record; it is not sent to hardware."""
    if not 0 <= features <= 0xFFFFFFFF:
        raise ValueError("GLINK feature bitmap is outside its 32-bit field")
    command = GLINK_CMD_VERSION_ACK if ack else GLINK_CMD_VERSION
    return _glink_header(command, GLINK_VERSION, features)


def encode_glink_open(channel_id: int, name: str) -> bytes:
    """Encode a bounded named OPEN request, including FIFO alignment bytes."""
    try:
        name_bytes = name.encode("ascii") + b"\0"
    except UnicodeEncodeError as exc:
        raise ValueError("GLINK channel name must be ASCII") from exc
    if not name or len(name_bytes) > GLINK_NAME_SIZE:
        raise ValueError("GLINK channel name is empty or exceeds 31 bytes")
    raw = _glink_header(GLINK_CMD_OPEN, channel_id, len(name_bytes)) + name_bytes
    return raw.ljust(_align_glink(len(raw)), b"\0")


def encode_glink_open_ack(channel_id: int) -> bytes:
    return _glink_header(GLINK_CMD_OPEN_ACK, channel_id, 0)


def encode_glink_intent(channel_id: int, intents: tuple[GlinkIntent, ...]) -> bytes:
    """Encode GLINK INTENT records, each as little-endian size and ID."""
    if not intents or len(intents) > 32:
        raise ValueError("GLINK intent count must be between 1 and 32")
    body = bytearray()
    for intent in intents:
        if not 0 < intent.size <= GLINK_MAX_MESSAGE:
            raise ValueError("GLINK intent size is outside the configured bound")
        if not 0 < intent.intent_id <= 0xFFFFFFFF:
            raise ValueError("GLINK intent ID must be a nonzero u32")
        body += intent.size.to_bytes(4, "little")
        body += intent.intent_id.to_bytes(4, "little")
    raw = _glink_header(GLINK_CMD_INTENT, channel_id, len(intents)) + body
    return raw.ljust(_align_glink(len(raw)), b"\0")


def encode_glink_intent_request(channel_id: int, size: int) -> bytes:
    if not 0 < size <= GLINK_MAX_MESSAGE:
        raise ValueError("GLINK requested intent size is outside the configured bound")
    return _glink_header(GLINK_CMD_RX_INTENT_REQ, channel_id, size)


def glink_record_size(data: bytes, *, max_message: int = GLINK_MAX_MESSAGE) -> int | None:
    """Return one record's aligned byte length, or None for an incomplete prefix.

    This is useful to a future ring parser, but intentionally does not read
    from or advance an SMEM FIFO.
    """
    if not 0 < max_message <= GLINK_MAX_MESSAGE:
        raise ValueError("GLINK message bound is invalid")
    if len(data) < GLINK_HEADER_SIZE:
        return None
    command = int.from_bytes(data[0:2], "little")
    param2 = int.from_bytes(data[4:8], "little")
    if command == GLINK_CMD_OPEN:
        if not 1 <= param2 <= GLINK_NAME_SIZE:
            raise GlinkFrameError("GLINK OPEN name length is outside the bound")
        raw_size = GLINK_HEADER_SIZE + param2
    elif command == GLINK_CMD_INTENT:
        if not 1 <= param2 <= 32:
            raise GlinkFrameError("GLINK intent count is outside the bound")
        raw_size = GLINK_HEADER_SIZE + param2 * 8
    elif command in (GLINK_CMD_TX_DATA, GLINK_CMD_TX_DATA_CONT):
        if len(data) < 16:
            return None
        chunk_size = int.from_bytes(data[8:12], "little")
        left_size = int.from_bytes(data[12:16], "little")
        if not chunk_size or chunk_size > GLINK_MAX_TX_CHUNK:
            raise GlinkFrameError("GLINK data chunk exceeds configured bound")
        if left_size > max_message or chunk_size + left_size > max_message:
            raise GlinkFrameError("GLINK reassembled message exceeds configured bound")
        raw_size = 16 + chunk_size
    elif command in {
        GLINK_CMD_VERSION, GLINK_CMD_VERSION_ACK, GLINK_CMD_CLOSE,
        GLINK_CMD_OPEN_ACK, GLINK_CMD_RX_DONE, GLINK_CMD_RX_INTENT_REQ,
        GLINK_CMD_RX_INTENT_REQ_ACK, GLINK_CMD_CLOSE_ACK,
        GLINK_CMD_READ_NOTIF, GLINK_CMD_RX_DONE_W_REUSE, GLINK_CMD_SIGNALS,
    }:
        raw_size = GLINK_HEADER_SIZE
    else:
        raise GlinkFrameError("unknown GLINK command")
    required = _align_glink(raw_size)
    return required if len(data) >= required else None


def parse_glink_record(data: bytes, *, max_message: int = GLINK_MAX_MESSAGE) -> GlinkRecord:
    """Parse exactly one aligned native GLINK record from a synthetic byte buffer.

    This deliberately does not read a ring or interpret SMEM indices. Callers
    must provide the complete record, including any FIFO alignment bytes.
    """
    if not 0 < max_message <= GLINK_MAX_MESSAGE:
        raise ValueError("GLINK message bound is invalid")
    if len(data) < GLINK_HEADER_SIZE or len(data) & (GLINK_FIFO_ALIGNMENT - 1):
        raise GlinkFrameError("GLINK record must contain a header and be 8-byte aligned")
    command = int.from_bytes(data[0:2], "little")
    param1 = int.from_bytes(data[2:4], "little")
    param2 = int.from_bytes(data[4:8], "little")
    if command not in {
        GLINK_CMD_VERSION, GLINK_CMD_VERSION_ACK, GLINK_CMD_OPEN,
        GLINK_CMD_CLOSE, GLINK_CMD_OPEN_ACK, GLINK_CMD_INTENT,
        GLINK_CMD_RX_DONE, GLINK_CMD_RX_INTENT_REQ,
        GLINK_CMD_RX_INTENT_REQ_ACK, GLINK_CMD_TX_DATA,
        GLINK_CMD_CLOSE_ACK, GLINK_CMD_TX_DATA_CONT,
        GLINK_CMD_READ_NOTIF, GLINK_CMD_RX_DONE_W_REUSE,
        GLINK_CMD_SIGNALS,
    }:
        raise GlinkFrameError("unknown GLINK command")

    if command == GLINK_CMD_OPEN:
        if not 1 <= param2 <= GLINK_NAME_SIZE:
            raise GlinkFrameError("GLINK OPEN name length is outside the bound")
        end = GLINK_HEADER_SIZE + param2
        if end > len(data) or _align_glink(end) != len(data):
            raise GlinkFrameError("GLINK OPEN payload length does not match record")
        name = data[GLINK_HEADER_SIZE:end]
        if not name[:-1] or name[-1:] != b"\0" or b"\0" in name[:-1]:
            raise GlinkFrameError("GLINK OPEN name is not a single NUL-terminated string")
        try:
            name[:-1].decode("ascii")
        except UnicodeDecodeError as exc:
            raise GlinkFrameError("GLINK OPEN name is not ASCII") from exc
        body = name
    elif command in (GLINK_CMD_TX_DATA, GLINK_CMD_TX_DATA_CONT):
        if len(data) < 16:
            raise GlinkFrameError("truncated GLINK data header")
        chunk_size = int.from_bytes(data[8:12], "little")
        left_size = int.from_bytes(data[12:16], "little")
        if chunk_size > GLINK_MAX_TX_CHUNK or left_size > max_message:
            raise GlinkFrameError("GLINK data chunk exceeds configured bound")
        end = 16 + chunk_size
        if end > len(data) or _align_glink(end) != len(data):
            raise GlinkFrameError("GLINK data length does not match record")
        if chunk_size + left_size > max_message:
            raise GlinkFrameError("GLINK reassembled message exceeds configured bound")
        body = data[8:end]
    elif command == GLINK_CMD_INTENT:
        if not 1 <= param2 <= 32:
            raise GlinkFrameError("GLINK intent count is outside the bound")
        end = GLINK_HEADER_SIZE + param2 * 8
        if end > len(data) or _align_glink(end) != len(data):
            raise GlinkFrameError("GLINK INTENT payload length does not match count")
        body = data[GLINK_HEADER_SIZE:end]
    else:
        if len(data) != GLINK_HEADER_SIZE:
            raise GlinkFrameError("GLINK control record has unexpected payload")
        body = b""

    return GlinkRecord(command, param1, param2, body)


def parse_glink_intents(record: GlinkRecord) -> tuple[GlinkIntent, ...]:
    if record.command != GLINK_CMD_INTENT:
        raise GlinkFrameError("record is not GLINK INTENT")
    if len(record.body) != record.param2 * 8:
        raise GlinkFrameError("GLINK INTENT count does not match body")
    intents = tuple(
        GlinkIntent(
            int.from_bytes(record.body[offset : offset + 4], "little"),
            int.from_bytes(record.body[offset + 4 : offset + 8], "little"),
        )
        for offset in range(0, len(record.body), 8)
    )
    if any(not 0 < item.size <= GLINK_MAX_MESSAGE or item.intent_id == 0 for item in intents):
        raise GlinkFrameError("GLINK INTENT entry is outside the configured bound")
    return intents


def parse_glink_data_chunk(record: GlinkRecord, *, max_message: int = GLINK_MAX_MESSAGE) -> GlinkDataChunk:
    if record.command not in (GLINK_CMD_TX_DATA, GLINK_CMD_TX_DATA_CONT):
        raise GlinkFrameError("record is not GLINK TX_DATA")
    if len(record.body) < 8:
        raise GlinkFrameError("truncated GLINK data chunk")
    chunk_size = int.from_bytes(record.body[0:4], "little")
    left_size = int.from_bytes(record.body[4:8], "little")
    chunk = record.body[8:]
    if chunk_size != len(chunk) or not chunk_size or chunk_size > GLINK_MAX_TX_CHUNK:
        raise GlinkFrameError("GLINK data chunk size does not match payload")
    if left_size > max_message or chunk_size + left_size > max_message:
        raise GlinkFrameError("GLINK reassembled message exceeds configured bound")
    return GlinkDataChunk(record.param1, record.param2, chunk, left_size, record.command == GLINK_CMD_TX_DATA_CONT)


class GlinkReassembler:
    """Bounded, per-channel TX_DATA fragment assembler for synthetic records."""

    def __init__(self, *, max_message: int = GLINK_MAX_MESSAGE):
        if not 0 < max_message <= GLINK_MAX_MESSAGE:
            raise ValueError("GLINK reassembly bound is invalid")
        self.max_message = max_message
        self._channel_id: int | None = None
        self._intent_id: int | None = None
        self._remaining = 0
        self._buffer = bytearray()

    @property
    def pending(self) -> bool:
        return self._channel_id is not None

    def feed(self, record: GlinkRecord) -> bytes | None:
        chunk = parse_glink_data_chunk(record, max_message=self.max_message)
        if not self.pending:
            if chunk.continuation:
                raise GlinkFrameError("GLINK continuation has no initial data chunk")
            if len(chunk.chunk) + chunk.left_size > self.max_message:
                raise GlinkFrameError("GLINK reassembled message exceeds configured bound")
            if chunk.left_size == 0:
                return chunk.chunk
            self._channel_id = chunk.channel_id
            self._intent_id = chunk.intent_id
            self._remaining = chunk.left_size
            self._buffer.extend(chunk.chunk)
            return None

        if not chunk.continuation:
            self.reset()
            raise GlinkFrameError("new GLINK data message arrived before prior message completed")
        if (chunk.channel_id, chunk.intent_id) != (self._channel_id, self._intent_id):
            self.reset()
            raise GlinkFrameError("GLINK continuation channel or intent ID changed")
        if len(chunk.chunk) + chunk.left_size != self._remaining:
            self.reset()
            raise GlinkFrameError("GLINK continuation remaining-byte count is inconsistent")

        self._buffer.extend(chunk.chunk)
        if len(self._buffer) > self.max_message:
            self.reset()
            raise GlinkFrameError("GLINK reassembled message exceeds configured bound")
        self._remaining = chunk.left_size
        if self._remaining:
            return None
        message = bytes(self._buffer)
        self.reset()
        return message

    def reset(self) -> None:
        self._channel_id = None
        self._intent_id = None
        self._remaining = 0
        self._buffer.clear()


@dataclass(frozen=True)
class QmiMessage:
    message_type: int
    transaction_id: int
    message_id: int
    tlvs: tuple[tuple[int, bytes], ...]


@dataclass(frozen=True)
class MsaRegion:
    address: int
    size: int
    secure: bool


@dataclass(frozen=True)
class MsaInfoResponse:
    result: int
    error: int
    regions: tuple[MsaRegion, ...]


@dataclass(frozen=True)
class WlfwResponseStatus:
    result: int
    error: int
    optional_tlvs: tuple[tuple[int, bytes], ...]


@dataclass(frozen=True)
class WlfwIndicationRegistrationResponse:
    result: int
    error: int
    fw_status: int | None


@dataclass(frozen=True)
class WlfwCapabilityResponse:
    result: int
    error: int
    chip_info: tuple[int, int] | None
    board_id: int | None
    soc_id: int | None
    firmware_version: int | None
    firmware_timestamp: str | None
    firmware_build_id: str | None
    number_of_macs: int | None
    unknown_optional_tlvs: tuple[tuple[int, bytes], ...]


@dataclass(frozen=True)
class QrtrServerEvent:
    service_id: int
    version: int
    instance: int
    node: int
    port: int


@dataclass(frozen=True)
class QrtrPacket:
    version: int
    packet_type: int
    source_node: int
    source_port: int
    destination_node: int
    destination_port: int
    confirm_rx: bool
    payload: bytes


def parse_qrtr_packet(data: bytes, *, max_payload: int = 65535) -> QrtrPacket:
    """Parse a bounded version-1 or version-2 QRTR packet with 4-byte padding."""
    if not data or len(data) & 3:
        raise QmiFrameError("QRTR packet must be nonempty and 4-byte aligned")
    version = data[0]
    if version == QRTR_PROTO_VER_1:
        if len(data) < QRTR_V1_HEADER_SIZE:
            raise QmiFrameError("truncated QRTR v1 header")
        fields = [int.from_bytes(data[offset : offset + 4], "little") for offset in range(0, 32, 4)]
        if fields[0] != QRTR_PROTO_VER_1:
            raise QmiFrameError("invalid QRTR v1 version field")
        _, packet_type, source_node, source_port, confirm_rx, size, dest_node, dest_port = fields
        header_size = QRTR_V1_HEADER_SIZE
    elif version == QRTR_PROTO_VER_2:
        if len(data) < QRTR_V2_HEADER_SIZE:
            raise QmiFrameError("truncated QRTR v2 header")
        packet_type, flags, optional_size = data[1], data[2], data[3]
        if flags & ~0x01:
            raise QmiFrameError("QRTR v2 header has unsupported flags")
        size = int.from_bytes(data[4:8], "little")
        source_node = int.from_bytes(data[8:10], "little")
        source_port = int.from_bytes(data[10:12], "little")
        dest_node = int.from_bytes(data[12:14], "little")
        dest_port = int.from_bytes(data[14:16], "little")
        header_size = QRTR_V2_HEADER_SIZE + optional_size
        if header_size > len(data):
            raise QmiFrameError("QRTR v2 optional header exceeds packet")
        if header_size & 3:
            raise QmiFrameError("QRTR v2 header length is not 4-byte aligned")
        if source_port == QRTR_PORT_CTRL_V2:
            source_port = QRTR_PORT_CTRL
        if dest_port == QRTR_PORT_CTRL_V2:
            dest_port = QRTR_PORT_CTRL
        confirm_rx = flags & 1
    else:
        raise QmiFrameError("unsupported QRTR protocol version")

    if confirm_rx not in (0, 1):
        raise QmiFrameError("QRTR confirm_rx field is not boolean")
    if size > max_payload:
        raise QmiFrameError("QRTR payload exceeds configured bound")
    if len(data) != header_size + ((size + 3) & ~3):
        raise QmiFrameError("QRTR payload length does not match packet")
    payload = data[header_size : header_size + size]
    return QrtrPacket(
        version,
        packet_type,
        source_node,
        source_port,
        dest_node,
        dest_port,
        bool(confirm_rx),
        payload,
    )


def encode_wlfw_qrtr_lookup() -> bytes:
    """Encode the QRTR control payload registered by ICNSS qmi_add_lookup."""
    version_instance = WLFW_SERVICE_VERSION | (WLFW_SERVICE_INSTANCE << 8)
    return b"".join(
        value.to_bytes(4, "little")
        for value in (QRTR_TYPE_NEW_LOOKUP, WLFW_SERVICE_ID, version_instance, 0, 0)
    )


def parse_wlfw_qrtr_server_event(data: bytes) -> QrtrServerEvent:
    """Parse a QRTR NEW_SERVER control payload after AF_QIPCRTR demultiplexing."""
    if len(data) != QRTR_CTRL_PACKET_SIZE:
        raise QmiFrameError("QRTR control packet must be exactly 20 bytes")
    command, service, version_instance, node, port = (
        int.from_bytes(data[offset : offset + 4], "little")
        for offset in range(0, QRTR_CTRL_PACKET_SIZE, 4)
    )
    if command != QRTR_TYPE_NEW_SERVER:
        raise QmiFrameError("QRTR control packet is not NEW_SERVER")
    version = version_instance & 0xFF
    instance = (version_instance >> 8) & 0xFF
    if version_instance != (version | (instance << 8)):
        raise QmiFrameError("QRTR service version/instance has reserved bits set")
    if (service, version, instance) != (
        WLFW_SERVICE_ID,
        WLFW_SERVICE_VERSION,
        WLFW_SERVICE_INSTANCE,
    ):
        raise QmiFrameError("QRTR NEW_SERVER does not match the WLFW service lookup")
    if node == 0 or port == 0:
        raise QmiFrameError("QRTR NEW_SERVER has an invalid endpoint")
    return QrtrServerEvent(service, version, instance, node, port)


def parse_qmi_message(data: bytes, *, max_payload: int = MAX_QMI_PAYLOAD) -> QmiMessage:
    """Parse the 7-byte QMI header and bounded TLVs; reject trailing/truncated data."""
    if len(data) < QMI_HEADER_SIZE:
        raise QmiFrameError("truncated QMI header")

    message_type = data[0]
    if message_type not in (QMI_REQUEST, QMI_RESPONSE, QMI_INDICATION):
        raise QmiFrameError("unsupported QMI message type")

    transaction_id = int.from_bytes(data[1:3], "little")
    message_id = int.from_bytes(data[3:5], "little")
    payload_size = int.from_bytes(data[5:7], "little")
    if payload_size > max_payload:
        raise QmiFrameError("QMI payload exceeds configured bound")
    if len(data) != QMI_HEADER_SIZE + payload_size:
        raise QmiFrameError("QMI payload length does not match frame")

    tlvs: list[tuple[int, bytes]] = []
    offset = QMI_HEADER_SIZE
    end = len(data)
    while offset < end:
        if end - offset < 3:
            raise QmiFrameError("truncated QMI TLV header")
        tlv_type = data[offset]
        tlv_size = int.from_bytes(data[offset + 1 : offset + 3], "little")
        offset += 3
        if tlv_size > end - offset:
            raise QmiFrameError("QMI TLV extends beyond message")
        tlvs.append((tlv_type, data[offset : offset + tlv_size]))
        offset += tlv_size

    return QmiMessage(message_type, transaction_id, message_id, tuple(tlvs))


def _encode_request(transaction_id: int, message_id: int, payload: bytes) -> bytes:
    if not 0 <= transaction_id <= 0xFFFF:
        raise ValueError("transaction id is outside QMI's 16-bit range")
    if not 0 <= message_id <= 0xFFFF:
        raise ValueError("message id is outside QMI's 16-bit range")
    if len(payload) > MAX_QMI_PAYLOAD:
        raise ValueError("QMI payload exceeds its 16-bit length field")
    return (
        bytes([QMI_REQUEST])
        + transaction_id.to_bytes(2, "little")
        + message_id.to_bytes(2, "little")
        + len(payload).to_bytes(2, "little")
        + payload
    )


def encode_msa_info_request(transaction_id: int, address: int, size: int) -> bytes:
    """Encode the two required WLFW MSA_INFO_REQ TLVs from the matched IDL."""
    if not 0 <= address <= 0xFFFFFFFFFFFFFFFF:
        raise ValueError("MSA address is outside the QMI u64 range")
    if not 0 <= size <= 0xFFFFFFFF:
        raise ValueError("MSA size is outside the QMI u32 range")
    payload = (
        b"\x01\x08\x00"
        + address.to_bytes(8, "little")
        + b"\x02\x04\x00"
        + size.to_bytes(4, "little")
    )
    return _encode_request(transaction_id, WLFW_MSA_INFO_MESSAGE_ID, payload)


def encode_indication_registration_request(
    transaction_id: int, *, rejuvenate: bool = False
) -> bytes:
    """Encode the ICNSS indication subscriptions from its matched source."""
    for field_name, value in (("rejuvenate", rejuvenate),):
        if not isinstance(value, bool):
            raise ValueError(f"{field_name} must be a boolean")
    # ICNSS marks only FW-ready, MSA-ready, pin-connect-result, and client ID
    # as valid in its default request. Rejuvenate is conditional on a quirk.
    payload = b"\x10\x01\x00\x01\x13\x01\x00\x01\x14\x01\x00\x01"
    payload += b"\x15\x04\x00" + WLFW_CLIENT_ID.to_bytes(4, "little")
    if rejuvenate:
        payload += b"\x19\x04\x00\x01\x00\x00\x00"
    return _encode_request(transaction_id, WLFW_IND_REGISTER_MESSAGE_ID, payload)


def encode_msa_ready_request(transaction_id: int) -> bytes:
    return _encode_request(transaction_id, WLFW_MSA_READY_MESSAGE_ID, b"")


def encode_capability_request(transaction_id: int) -> bytes:
    return _encode_request(transaction_id, WLFW_CAP_MESSAGE_ID, b"")


def decode_wlfw_status_response(
    data: bytes, *, expected_transaction_id: int, expected_message_id: int, max_payload: int = 207
) -> WlfwResponseStatus:
    """Decode common QMI result and retain bounded optional WLFW response TLVs."""
    message = parse_qmi_message(data, max_payload=max_payload)
    if message.message_type != QMI_RESPONSE:
        raise QmiFrameError("WLFW status frame is not a QMI response")
    if message.transaction_id != expected_transaction_id:
        raise QmiFrameError("WLFW response transaction id mismatch")
    if message.message_id != expected_message_id:
        raise QmiFrameError("unexpected WLFW response message id")

    common: bytes | None = None
    optional: list[tuple[int, bytes]] = []
    for tlv_type, value in message.tlvs:
        if tlv_type == 0x02:
            if common is not None:
                raise QmiFrameError("duplicate QMI common result TLV")
            if len(value) != 4:
                raise QmiFrameError("invalid QMI common response TLV size")
            common = value
        elif tlv_type < 0x10:
            raise QmiFrameError("unknown mandatory WLFW response TLV")
        else:
            optional.append((tlv_type, value))
    if common is None:
        raise QmiFrameError("WLFW response is missing the common result TLV")
    return WlfwResponseStatus(
        int.from_bytes(common[:2], "little"),
        int.from_bytes(common[2:], "little"),
        tuple(optional),
    )


def decode_msa_ready_response(data: bytes, *, expected_transaction_id: int) -> WlfwResponseStatus:
    return decode_wlfw_status_response(
        data,
        expected_transaction_id=expected_transaction_id,
        expected_message_id=WLFW_MSA_READY_MESSAGE_ID,
        max_payload=7,
    )


def decode_indication_registration_response(
    data: bytes, *, expected_transaction_id: int
) -> WlfwResponseStatus:
    return decode_wlfw_status_response(
        data,
        expected_transaction_id=expected_transaction_id,
        expected_message_id=WLFW_IND_REGISTER_MESSAGE_ID,
        max_payload=18,
    )


def decode_capability_response(data: bytes, *, expected_transaction_id: int) -> WlfwResponseStatus:
    return decode_wlfw_status_response(
        data,
        expected_transaction_id=expected_transaction_id,
        expected_message_id=WLFW_CAP_MESSAGE_ID,
        max_payload=207,
    )


def _optional_tlv_map(tlvs: tuple[tuple[int, bytes], ...]) -> dict[int, bytes]:
    result: dict[int, bytes] = {}
    for tlv_type, value in tlvs:
        if tlv_type in result:
            raise QmiFrameError("duplicate optional WLFW response TLV")
        result[tlv_type] = value
    return result


def decode_indication_registration_details(
    data: bytes, *, expected_transaction_id: int
) -> WlfwIndicationRegistrationResponse:
    status = decode_indication_registration_response(
        data, expected_transaction_id=expected_transaction_id
    )
    optional = _optional_tlv_map(status.optional_tlvs)
    fw_status = optional.pop(0x10, None)
    if fw_status is not None and len(fw_status) != 8:
        raise QmiFrameError("invalid WLFW indication-registration fw_status TLV")
    if optional:
        raise QmiFrameError("unknown indication-registration response TLV")
    return WlfwIndicationRegistrationResponse(
        status.result,
        status.error,
        int.from_bytes(fw_status, "little") if fw_status is not None else None,
    )


def _decode_qmi_string(value: bytes, *, max_bytes: int, label: str) -> str:
    if not value:
        raise QmiFrameError(f"{label} is missing its string length")
    size = value[0]
    if size > max_bytes or len(value) != size + 1:
        raise QmiFrameError(f"{label} string length is invalid")
    try:
        return value[1:].decode("ascii")
    except UnicodeDecodeError as error:
        raise QmiFrameError(f"{label} is not ASCII") from error


def decode_capability_details(
    data: bytes, *, expected_transaction_id: int
) -> WlfwCapabilityResponse:
    status = decode_capability_response(data, expected_transaction_id=expected_transaction_id)
    optional = _optional_tlv_map(status.optional_tlvs)

    chip_info = optional.pop(0x10, None)
    if chip_info is not None:
        if len(chip_info) != 8:
            raise QmiFrameError("invalid WLFW chip-info TLV")
        chip_info = (
            int.from_bytes(chip_info[:4], "little"),
            int.from_bytes(chip_info[4:], "little"),
        )

    board_info = optional.pop(0x11, None)
    if board_info is not None and len(board_info) != 4:
        raise QmiFrameError("invalid WLFW board-info TLV")
    soc_info = optional.pop(0x12, None)
    if soc_info is not None and len(soc_info) != 4:
        raise QmiFrameError("invalid WLFW SoC-info TLV")

    firmware_version = None
    firmware_timestamp = None
    fw_version_info = optional.pop(0x13, None)
    if fw_version_info is not None:
        if len(fw_version_info) < 5:
            raise QmiFrameError("invalid WLFW firmware-version TLV")
        firmware_version = int.from_bytes(fw_version_info[:4], "little")
        firmware_timestamp = _decode_qmi_string(
            fw_version_info[4:], max_bytes=32, label="firmware timestamp"
        )

    firmware_build_id = None
    fw_build_id = optional.pop(0x14, None)
    if fw_build_id is not None:
        firmware_build_id = _decode_qmi_string(
            fw_build_id, max_bytes=128, label="firmware build ID"
        )

    number_of_macs = optional.pop(0x15, None)
    if number_of_macs is not None:
        if len(number_of_macs) != 1:
            raise QmiFrameError("invalid WLFW number-of-MACs TLV")
        number_of_macs = number_of_macs[0]

    return WlfwCapabilityResponse(
        result=status.result,
        error=status.error,
        chip_info=chip_info,
        board_id=int.from_bytes(board_info, "little") if board_info is not None else None,
        soc_id=int.from_bytes(soc_info, "little") if soc_info is not None else None,
        firmware_version=firmware_version,
        firmware_timestamp=firmware_timestamp,
        firmware_build_id=firmware_build_id,
        number_of_macs=number_of_macs,
        unknown_optional_tlvs=tuple(sorted(optional.items())),
    )


def decode_msa_info_response(data: bytes, *, expected_transaction_id: int) -> MsaInfoResponse:
    """Decode the MSA_INFO_RESP common result and at most two returned ranges."""
    message = parse_qmi_message(data, max_payload=37)
    if message.message_type != QMI_RESPONSE:
        raise QmiFrameError("MSA info frame is not a QMI response")
    if message.transaction_id != expected_transaction_id:
        raise QmiFrameError("MSA info response transaction id mismatch")
    if message.message_id != WLFW_MSA_INFO_MESSAGE_ID:
        raise QmiFrameError("unexpected WLFW message id for MSA info response")

    values: dict[int, bytes] = {}
    for tlv_type, value in message.tlvs:
        if tlv_type not in (0x02, 0x03):
            if tlv_type < 0x10:
                raise QmiFrameError("unknown mandatory MSA response TLV")
            continue
        if tlv_type in values:
            raise QmiFrameError("duplicate MSA response TLV")
        values[tlv_type] = value
    if 0x02 not in values:
        raise QmiFrameError("MSA response is missing the common result TLV")
    if len(values[0x02]) != 4:
        raise QmiFrameError("invalid QMI common response TLV size")

    result = int.from_bytes(values[0x02][0:2], "little")
    error = int.from_bytes(values[0x02][2:4], "little")
    if result != 0:
        if 0x03 in values:
            raise QmiFrameError("failed MSA response unexpectedly includes memory regions")
        return MsaInfoResponse(result, error, ())
    if 0x03 not in values:
        raise QmiFrameError("successful MSA response is missing its region TLV")

    region_tlv = values[0x03]
    if not region_tlv:
        raise QmiFrameError("MSA region TLV is missing its array count")
    count = region_tlv[0]
    if count > WLFW_MAX_MSA_REGIONS:
        raise QmiFrameError("WLFW reported more MSA regions than supported")
    record_size = 13  # u64 address, u32 size, u8 secure_flag
    if len(region_tlv) != 1 + count * record_size:
        raise QmiFrameError("MSA region array length is inconsistent")

    regions = []
    offset = 1
    for _ in range(count):
        address = int.from_bytes(region_tlv[offset : offset + 8], "little")
        size = int.from_bytes(region_tlv[offset + 8 : offset + 12], "little")
        secure_flag = region_tlv[offset + 12]
        if secure_flag not in (0, 1):
            raise QmiFrameError("WLFW MSA secure_flag must be 0 or 1")
        regions.append(MsaRegion(address, size, bool(secure_flag)))
        offset += record_size

    return MsaInfoResponse(
        result=result,
        error=error,
        regions=tuple(regions),
    )


class ServiceState(Enum):
    SEARCHING = auto()
    CONNECTED = auto()


@dataclass(frozen=True)
class ServiceEndpoint:
    node: int
    port: int


class StartupStage(Enum):
    WAITING = auto()
    INDICATION_REGISTRATION = auto()
    MSA_INFO = auto()
    MSA_OWNERSHIP = auto()
    MSA_READY = auto()
    CAPABILITY = auto()
    READY = auto()
    FAILED = auto()


class WlfwStartupModel:
    """Offline fail-closed model of ICNSS's observed WLFW bring-up order."""

    def __init__(self) -> None:
        self.stage = StartupStage.WAITING
        self.failure: str | None = None

    def begin(self, *, service_connected: bool, platform_power_ready: bool) -> bool:
        if self.stage is not StartupStage.WAITING:
            return self._fail("startup already began")
        if not service_connected or not platform_power_ready:
            return self._fail("transport or platform power is not ready")
        self.stage = StartupStage.INDICATION_REGISTRATION
        return True

    def accept_response(self, message_id: int, *, result: int, has_msa_regions: bool = False) -> bool:
        expected = {
            StartupStage.INDICATION_REGISTRATION: WLFW_IND_REGISTER_MESSAGE_ID,
            StartupStage.MSA_INFO: WLFW_MSA_INFO_MESSAGE_ID,
            StartupStage.MSA_READY: WLFW_MSA_READY_MESSAGE_ID,
            StartupStage.CAPABILITY: WLFW_CAP_MESSAGE_ID,
        }.get(self.stage)
        if expected is None or message_id != expected:
            return self._fail("response arrived out of startup order")
        if result != 0:
            return self._fail("WLFW rejected startup request")
        if self.stage is StartupStage.INDICATION_REGISTRATION:
            self.stage = StartupStage.MSA_INFO
        elif self.stage is StartupStage.MSA_INFO:
            if not has_msa_regions:
                return self._fail("successful MSA response contains no regions")
            self.stage = StartupStage.MSA_OWNERSHIP
        elif self.stage is StartupStage.MSA_READY:
            self.stage = StartupStage.CAPABILITY
        else:
            self.stage = StartupStage.READY
        return True

    def confirm_msa_ownership(
        self, *, pool_excluded: bool, scm_owner_verified: bool, smmu_isolated: bool
    ) -> bool:
        if self.stage is not StartupStage.MSA_OWNERSHIP:
            return self._fail("MSA ownership confirmation arrived out of order")
        if not (pool_excluded and scm_owner_verified and smmu_isolated):
            return self._fail("MSA ownership or DMA isolation is unverified")
        self.stage = StartupStage.MSA_READY
        return True

    def _fail(self, reason: str) -> bool:
        self.stage = StartupStage.FAILED
        self.failure = reason
        return False


class WlfwServiceModel:
    """Fail-closed host model for matching a single WLFW QRTR service."""

    def __init__(self) -> None:
        self.state = ServiceState.SEARCHING
        self.endpoint: ServiceEndpoint | None = None

    def service_arrived(
        self, service_id: int, version: int, instance: int, node: int, port: int
    ) -> bool:
        if (service_id, version, instance) != (
            WLFW_SERVICE_ID,
            WLFW_SERVICE_VERSION,
            WLFW_SERVICE_INSTANCE,
        ):
            return False
        if not 0 <= node <= 0xFFFFFFFF or not 0 <= port <= 0xFFFFFFFF:
            return False
        if node == 0 or port == 0:
            return False
        endpoint = ServiceEndpoint(node, port)
        self.endpoint = endpoint
        self.state = ServiceState.CONNECTED
        return True

    def service_departed(self, node: int, port: int) -> bool:
        """Drop only the currently bound endpoint; stale removals are ignored."""
        if self.endpoint != ServiceEndpoint(node, port):
            return False
        self.endpoint = None
        self.state = ServiceState.SEARCHING
        return True


class WlfwBrokerError(RuntimeError):
    """The host-only WLFW transaction broker rejected an operation."""


@dataclass(frozen=True)
class WlfwRequestTicket:
    endpoint: ServiceEndpoint
    transaction_id: int
    message_id: int
    wire_bytes: bytes
    started_ms: int
    deadline_ms: int


@dataclass(frozen=True)
class WlfwPolicyCompletion:
    session_id: int
    request: WlfwRequestTicket
    response: QmiMessage
    received_ms: int


class WlfwPolicyQueueError(RuntimeError):
    """The bounded host-only broker-to-policy queue rejected a completion."""


class WlfwPolicyQueue:
    """Bounded SPSC-style completion queue model with explicit disconnect flush.

    Queue entries are already parsed QMI responses tied to the broker's request
    ticket. This models the client boundary only; it does not schedule work or
    own a transport. Overflow rejects the newest completion instead of silently
    overwriting an older one.
    """

    CAPACITY = 4
    REQUEST_WIRE_LIMIT = 64
    RESPONSE_PAYLOAD_LIMITS = {
        WLFW_IND_REGISTER_MESSAGE_ID: 18,
        WLFW_MSA_INFO_MESSAGE_ID: 37,
        WLFW_MSA_READY_MESSAGE_ID: 7,
        WLFW_CAP_MESSAGE_ID: 207,
    }

    def __init__(self) -> None:
        self.endpoint: ServiceEndpoint | None = None
        self._items: deque[WlfwPolicyCompletion] = deque()
        self._last_transaction_id = 0
        self._session_id = 0

    @property
    def session_id(self) -> int:
        return self._session_id

    def bind(self, endpoint: ServiceEndpoint) -> None:
        if not (0 <= endpoint.node <= 0xFFFFFFFF and 0 <= endpoint.port <= 0xFFFFFFFF):
            raise WlfwPolicyQueueError("service endpoint is outside its wire fields")
        if self.endpoint is not None and self.endpoint != endpoint:
            raise WlfwPolicyQueueError("another service endpoint is already bound")
        if self.endpoint is None:
            if self._items:
                raise WlfwPolicyQueueError("unbound queue contains stale completions")
            self.endpoint = endpoint
            self._last_transaction_id = 0
            self._session_id += 1

    @property
    def depth(self) -> int:
        return len(self._items)

    def publish(
        self,
        endpoint: ServiceEndpoint,
        session_id: int,
        request: WlfwRequestTicket,
        response: QmiMessage,
        *,
        received_ms: int,
    ) -> None:
        if session_id != self._session_id:
            raise WlfwPolicyQueueError("completion belongs to a stale endpoint session")
        if self.endpoint is None or endpoint != self.endpoint or request.endpoint != endpoint:
            raise WlfwPolicyQueueError("completion belongs to an unbound or stale endpoint")
        if received_ms < request.started_ms or received_ms >= request.deadline_ms:
            raise WlfwPolicyQueueError("completion time is outside the request window")
        if not 1 <= request.transaction_id <= 0xFFFF:
            raise WlfwPolicyQueueError("request transaction ID is invalid")
        if len(request.wire_bytes) > self.REQUEST_WIRE_LIMIT:
            raise WlfwPolicyQueueError("request ticket exceeds the fixed queue bound")
        try:
            request_message = parse_qmi_message(
                request.wire_bytes, max_payload=self.REQUEST_WIRE_LIMIT - QMI_HEADER_SIZE
            )
        except QmiFrameError as error:
            raise WlfwPolicyQueueError("request ticket contains malformed QMI") from error
        if (request_message.message_type != QMI_REQUEST or
                request_message.transaction_id != request.transaction_id or
                request_message.message_id != request.message_id):
            raise WlfwPolicyQueueError("request ticket does not match its encoded request")
        if request.transaction_id <= self._last_transaction_id:
            raise WlfwPolicyQueueError("duplicate or out-of-order completion")
        if (response.message_type != QMI_RESPONSE or
                response.transaction_id != request.transaction_id or
                response.message_id != request.message_id):
            raise WlfwPolicyQueueError("response does not match its request ticket")
        payload_size = sum(3 + len(value) for _, value in response.tlvs)
        limit = self.RESPONSE_PAYLOAD_LIMITS.get(request.message_id)
        if limit is None or payload_size > limit:
            raise WlfwPolicyQueueError("response exceeds the typed operation bound")
        if len(self._items) >= self.CAPACITY:
            raise WlfwPolicyQueueError("policy completion queue is full")

        frozen_response = QmiMessage(
            response.message_type,
            response.transaction_id,
            response.message_id,
            tuple((tlv_type, bytes(value)) for tlv_type, value in response.tlvs),
        )
        self._items.append(WlfwPolicyCompletion(
            session_id, request, frozen_response, received_ms
        ))
        self._last_transaction_id = request.transaction_id

    def receive(self) -> WlfwPolicyCompletion | None:
        return self._items.popleft() if self._items else None

    def service_departed(self, endpoint: ServiceEndpoint) -> bool:
        if self.endpoint != endpoint:
            return False
        self._items.clear()
        self.endpoint = None
        self._last_transaction_id = 0
        self._session_id += 1
        return True


class WlfwBrokerModel:
    """Offline single-flight WLFW request broker; it has no transport access.

    The public methods are typed to the four startup operations. It binds every
    reply to the currently discovered endpoint and one transaction, drops
    outstanding work on endpoint loss, and never retries timed-out requests.
    """

    MAX_TIMEOUT_MS = 30_000
    MSA_POOL_BASE = 0x98D00000
    MSA_POOL_SIZE = 0x200000
    ALLOWED_MESSAGE_IDS = frozenset({
        WLFW_IND_REGISTER_MESSAGE_ID,
        WLFW_MSA_INFO_MESSAGE_ID,
        WLFW_MSA_READY_MESSAGE_ID,
        WLFW_CAP_MESSAGE_ID,
    })

    def __init__(self) -> None:
        self.service = WlfwServiceModel()
        self.pending: WlfwRequestTicket | None = None
        self._next_transaction_id = 1

    @property
    def endpoint(self) -> ServiceEndpoint | None:
        return self.service.endpoint

    def service_arrived(
        self, service_id: int, version: int, instance: int, node: int, port: int
    ) -> bool:
        previous = self.service.endpoint
        if previous is not None and previous != ServiceEndpoint(node, port):
            return False
        if not self.service.service_arrived(service_id, version, instance, node, port):
            return False
        if self.service.endpoint != previous:
            self.pending = None
            self._next_transaction_id = 1
        return True

    def service_departed(self, node: int, port: int) -> bool:
        if not self.service.service_departed(node, port):
            return False
        self.pending = None
        return True

    def request_indication_registration(self, *, now_ms: int, timeout_ms: int = 5000) -> WlfwRequestTicket:
        return self._begin(
            WLFW_IND_REGISTER_MESSAGE_ID,
            encode_indication_registration_request(self._peek_transaction_id()),
            now_ms=now_ms,
            timeout_ms=timeout_ms,
        )

    def request_msa_info(self, *, now_ms: int, timeout_ms: int = 5000) -> WlfwRequestTicket:
        return self._begin(
            WLFW_MSA_INFO_MESSAGE_ID,
            encode_msa_info_request(
                self._peek_transaction_id(), self.MSA_POOL_BASE, self.MSA_POOL_SIZE
            ),
            now_ms=now_ms,
            timeout_ms=timeout_ms,
        )

    def request_msa_ready(self, *, now_ms: int, timeout_ms: int = 5000) -> WlfwRequestTicket:
        return self._begin(
            WLFW_MSA_READY_MESSAGE_ID,
            encode_msa_ready_request(self._peek_transaction_id()),
            now_ms=now_ms,
            timeout_ms=timeout_ms,
        )

    def request_capability(self, *, now_ms: int, timeout_ms: int = 5000) -> WlfwRequestTicket:
        return self._begin(
            WLFW_CAP_MESSAGE_ID,
            encode_capability_request(self._peek_transaction_id()),
            now_ms=now_ms,
            timeout_ms=timeout_ms,
        )

    def _peek_transaction_id(self) -> int:
        if self._next_transaction_id > 0xFFFF:
            raise WlfwBrokerError("transaction ID space exhausted for this endpoint session")
        return self._next_transaction_id

    def _begin(
        self, message_id: int, wire_bytes: bytes, *, now_ms: int, timeout_ms: int
    ) -> WlfwRequestTicket:
        endpoint = self.service.endpoint
        if endpoint is None:
            raise WlfwBrokerError("WLFW service is not connected")
        if message_id not in self.ALLOWED_MESSAGE_IDS:
            raise WlfwBrokerError("QMI message is outside the WLFW startup allowlist")
        if self.pending is not None:
            raise WlfwBrokerError("another WLFW transaction is outstanding")
        if now_ms < 0 or not 0 < timeout_ms <= self.MAX_TIMEOUT_MS:
            raise WlfwBrokerError("transaction time or timeout is outside the bound")

        parsed = parse_qmi_message(wire_bytes, max_payload=64)
        transaction_id = self._peek_transaction_id()
        if (parsed.message_type != QMI_REQUEST or
                parsed.transaction_id != transaction_id or
                parsed.message_id != message_id):
            raise WlfwBrokerError("encoded request does not match the broker operation")
        ticket = WlfwRequestTicket(
            endpoint, transaction_id, message_id, bytes(wire_bytes), now_ms,
            now_ms + timeout_ms,
        )
        self.pending = ticket
        self._next_transaction_id += 1
        return ticket

    def receive_response(
        self, source_node: int, source_port: int, wire_bytes: bytes, *, now_ms: int
    ) -> QmiMessage:
        endpoint = self.service.endpoint
        ticket = self.pending
        if endpoint is None or ticket is None:
            raise WlfwBrokerError("no connected endpoint has an outstanding request")
        if endpoint != ServiceEndpoint(source_node, source_port) or endpoint != ticket.endpoint:
            raise WlfwBrokerError("response came from a stale or unexpected endpoint")
        if now_ms < ticket.started_ms:
            raise WlfwBrokerError("response time is invalid")
        if now_ms >= ticket.deadline_ms:
            self.pending = None
            raise WlfwBrokerError("WLFW request timed out; response was discarded")
        response = parse_qmi_message(wire_bytes, max_payload=2048)
        if response.message_type != QMI_RESPONSE:
            raise WlfwBrokerError("WLFW reply is not a QMI response")
        if (response.transaction_id, response.message_id) != (
            ticket.transaction_id, ticket.message_id
        ):
            raise WlfwBrokerError("WLFW response does not match the outstanding request")
        self.pending = None
        return response

    def expire(self, *, now_ms: int) -> WlfwRequestTicket | None:
        """Discard an expired request. The caller may not blindly retry it."""
        if now_ms < 0:
            raise WlfwBrokerError("expiration time is invalid")
        if self.pending is None:
            return None
        if now_ms < self.pending.started_ms:
            raise WlfwBrokerError("expiration time moved backwards")
        if now_ms < self.pending.deadline_ms:
            return None
        expired = self.pending
        self.pending = None
        return expired
