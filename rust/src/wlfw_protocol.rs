//! Bounded, allocation-free QRTR/QMI framing for the Pixel WLFW path.
//!
//! This module only parses caller-provided bytes. It does not own a socket,
//! GLINK ring, shared-memory mapping, modem lifecycle, or hardware resource.

#![allow(dead_code)]

pub const WLFW_SERVICE_ID: u32 = 0x45;
pub const WLFW_SERVICE_VERSION: u8 = 1;
pub const WLFW_SERVICE_INSTANCE: u8 = 0;
pub const WLFW_MSA_INFO_MESSAGE_ID: u16 = 0x002d;
pub const WLFW_IND_REGISTER_MESSAGE_ID: u16 = 0x0020;
pub const WLFW_CAP_MESSAGE_ID: u16 = 0x0024;
pub const WLFW_MSA_READY_MESSAGE_ID: u16 = 0x002e;
pub const WLFW_BDF_DOWNLOAD_MESSAGE_ID: u16 = 0x0025;
pub const WLFW_CAL_REPORT_MESSAGE_ID: u16 = 0x0026;
pub const WLFW_WLAN_MODE_MESSAGE_ID: u16 = 0x0022;
pub const WLFW_WLAN_CFG_MESSAGE_ID: u16 = 0x0023;
pub const WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID: u16 = 0x003b;
pub const WLFW_FW_READY_IND_MESSAGE_ID: u16 = 0x0021;
pub const WLFW_MSA_READY_IND_MESSAGE_ID: u16 = 0x002b;
/// `fw_status` bit returned by indication registration when firmware is ready.
pub const WLFW_FW_READY_STATUS_BIT: u64 = 0x02;
/// `fw_status` bit returned when the remote WLFW server already has a client.
pub const WLFW_ALREADY_REGISTERED_STATUS_BIT: u64 = 0x01;
pub const WLFW_CLIENT_ID: u32 = 0x4b4e_454c;
pub const WLFW_MAX_MSA_REGIONS: usize = 2;
pub const WLFW_MAX_CE_TARGET_CFG: usize = 12;
pub const WLFW_MAX_CE_SERVICE_CFG: usize = 24;
pub const WLFW_MAX_SHADOW_REG_CFG: usize = 24;
pub const WLFW_MAX_SHADOW_REG_V2_CFG: usize = 36;
/// Maximum non-NUL bytes in a WLFW host-version string (QMI schema v01).
pub const WLFW_MAX_HOST_VERSION_BYTES: usize = 16;
pub const WLFW_WLAN_CFG_MAX_PAYLOAD_BYTES: usize = 796;
pub const WLFW_DYNAMIC_FEATURE_MASK_MAX_PAYLOAD_BYTES: usize = 29;
pub const WLFW_BDF_SCHEMA_MAX_DATA_BYTES: usize = 6144;
/// Largest BDF segment that fits one 4096-byte QRTR datagram with the
/// source-derived required QMI TLVs and the local QMI header.
pub const WLFW_BDF_QRTR_MAX_DATA_BYTES: usize = 4023;
pub const QRTR_TYPE_NEW_SERVER: u32 = 4;
pub const QRTR_TYPE_NEW_LOOKUP: u32 = 10;
pub const QRTR_CTRL_PACKET_SIZE: usize = 20;
pub const QRTR_PORT_CTRL: u32 = 0xffff_fffe;
pub const GLINK_FIFO_ALIGNMENT: usize = 8;
pub const GLINK_HEADER_SIZE: usize = 8;
pub const GLINK_NAME_SIZE: usize = 32;
pub const GLINK_MAX_TX_CHUNK: usize = 0x2000;
pub const GLINK_MAX_MESSAGE: usize = 0x100000;
pub const GLINK_CMD_VERSION: u16 = 0;
pub const GLINK_CMD_VERSION_ACK: u16 = 1;
pub const GLINK_CMD_OPEN: u16 = 2;
pub const GLINK_CMD_CLOSE: u16 = 3;
pub const GLINK_CMD_OPEN_ACK: u16 = 4;
pub const GLINK_CMD_INTENT: u16 = 5;
pub const GLINK_CMD_RX_DONE: u16 = 6;
pub const GLINK_CMD_RX_INTENT_REQ: u16 = 7;
pub const GLINK_CMD_RX_INTENT_REQ_ACK: u16 = 8;
pub const GLINK_CMD_TX_DATA: u16 = 9;
pub const GLINK_CMD_CLOSE_ACK: u16 = 11;
pub const GLINK_CMD_TX_DATA_CONT: u16 = 12;
pub const GLINK_CMD_READ_NOTIF: u16 = 13;
pub const GLINK_CMD_RX_DONE_W_REUSE: u16 = 14;
pub const GLINK_CMD_SIGNALS: u16 = 15;

const QRTR_V1_HEADER_SIZE: usize = 32;
const QRTR_V2_HEADER_SIZE: usize = 16;
const QMI_HEADER_SIZE: usize = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    EmptyOrUnalignedQrtr,
    TruncatedQrtrHeader,
    UnsupportedQrtrVersion,
    UnsupportedQrtrFlags,
    InvalidQrtrHeader,
    InvalidQrtrLength,
    QrtrPayloadTooLarge,
    InvalidQrtrControl,
    WrongQrtrService,
    InvalidQrtrEndpoint,
    TruncatedQmiHeader,
    UnsupportedQmiType,
    QmiPayloadTooLarge,
    InvalidQmiLength,
    TruncatedTlvHeader,
    TlvOverrun,
    WrongQmiType,
    WrongTransactionId,
    WrongMessageId,
    MissingCommonResult,
    InvalidCommonResult,
    DuplicateTlv,
    UnknownMandatoryTlv,
    TooManyMsaRegions,
    MissingMsaRegions,
    InvalidMsaRegions,
    InvalidSecureFlag,
    FailedResponseHasRegions,
    TooManyOptionalTlvs,
    InvalidWlfwString,
    InvalidWlfwField,
    UnknownOptionalTlv,
    OutputTooSmall,
    InvalidGlinkAlignment,
    UnknownGlinkCommand,
    InvalidGlinkRecordLength,
    InvalidGlinkOpenName,
    InvalidGlinkIntentCount,
    InvalidGlinkDataChunk,
    GlinkMessageTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlinkRecord<'a> {
    pub command: u16,
    pub param1: u16,
    pub param2: u32,
    /// Command-specific bytes, excluding the fixed GLINK header and padding.
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlinkIntent {
    pub size: u32,
    pub intent_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlinkDataChunk<'a> {
    pub channel_id: u16,
    pub intent_id: u32,
    pub bytes: &'a [u8],
    pub left_size: u32,
    pub continuation: bool,
}

pub fn parse_glink_record(
    data: &[u8],
    max_message: usize,
) -> Result<GlinkRecord<'_>, ProtocolError> {
    if data.len() < GLINK_HEADER_SIZE || data.len() & (GLINK_FIFO_ALIGNMENT - 1) != 0 {
        return Err(ProtocolError::InvalidGlinkAlignment);
    }
    if max_message == 0 || max_message > GLINK_MAX_MESSAGE {
        return Err(ProtocolError::GlinkMessageTooLarge);
    }
    let command = u16::from_le_bytes([data[0], data[1]]);
    let param1 = u16::from_le_bytes([data[2], data[3]]);
    let param2 = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let body = match command {
        GLINK_CMD_OPEN => {
            let name_len = param2 as usize;
            let end = GLINK_HEADER_SIZE
                .checked_add(name_len)
                .ok_or(ProtocolError::InvalidGlinkRecordLength)?;
            if !(1..=GLINK_NAME_SIZE).contains(&name_len)
                || end > data.len()
                || align_glink(end) != data.len()
            {
                return Err(ProtocolError::InvalidGlinkOpenName);
            }
            let name = &data[GLINK_HEADER_SIZE..end];
            if name.len() < 2
                || name[name.len() - 1] != 0
                || name[..name.len() - 1].contains(&0)
                || !name[..name.len() - 1].is_ascii()
            {
                return Err(ProtocolError::InvalidGlinkOpenName);
            }
            name
        }
        GLINK_CMD_INTENT => {
            if !(1..=32).contains(&param2) {
                return Err(ProtocolError::InvalidGlinkIntentCount);
            }
            let end = GLINK_HEADER_SIZE + (param2 as usize) * 8;
            if end > data.len() || align_glink(end) != data.len() {
                return Err(ProtocolError::InvalidGlinkRecordLength);
            }
            &data[GLINK_HEADER_SIZE..end]
        }
        GLINK_CMD_TX_DATA | GLINK_CMD_TX_DATA_CONT => {
            if data.len() < 16 {
                return Err(ProtocolError::InvalidGlinkRecordLength);
            }
            let chunk_size = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
            let left_size = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
            let end = 16usize
                .checked_add(chunk_size)
                .ok_or(ProtocolError::InvalidGlinkRecordLength)?;
            if chunk_size == 0
                || chunk_size > GLINK_MAX_TX_CHUNK
                || left_size > max_message
                || chunk_size > max_message
                || chunk_size + left_size > max_message
            {
                return Err(ProtocolError::GlinkMessageTooLarge);
            }
            if end > data.len() || align_glink(end) != data.len() {
                return Err(ProtocolError::InvalidGlinkRecordLength);
            }
            &data[8..end]
        }
        GLINK_CMD_VERSION
        | GLINK_CMD_VERSION_ACK
        | GLINK_CMD_CLOSE
        | GLINK_CMD_OPEN_ACK
        | GLINK_CMD_RX_DONE
        | GLINK_CMD_RX_INTENT_REQ
        | GLINK_CMD_RX_INTENT_REQ_ACK
        | GLINK_CMD_CLOSE_ACK
        | GLINK_CMD_READ_NOTIF
        | GLINK_CMD_RX_DONE_W_REUSE
        | GLINK_CMD_SIGNALS => {
            if data.len() != GLINK_HEADER_SIZE {
                return Err(ProtocolError::InvalidGlinkRecordLength);
            }
            &data[GLINK_HEADER_SIZE..]
        }
        _ => return Err(ProtocolError::UnknownGlinkCommand),
    };
    Ok(GlinkRecord {
        command,
        param1,
        param2,
        body,
    })
}

pub fn parse_glink_intents(
    record: GlinkRecord<'_>,
) -> Result<([GlinkIntent; 32], usize), ProtocolError> {
    if record.command != GLINK_CMD_INTENT
        || record.param2 == 0
        || record.param2 > 32
        || record.body.len() != record.param2 as usize * 8
    {
        return Err(ProtocolError::InvalidGlinkIntentCount);
    }
    let mut intents = [GlinkIntent {
        size: 0,
        intent_id: 0,
    }; 32];
    let count = record.param2 as usize;
    for (index, intent) in intents.iter_mut().take(count).enumerate() {
        let offset = index * 8;
        let size = u32::from_le_bytes(record.body[offset..offset + 4].try_into().unwrap());
        let id = u32::from_le_bytes(record.body[offset + 4..offset + 8].try_into().unwrap());
        if size == 0 || size as usize > GLINK_MAX_MESSAGE || id == 0 {
            return Err(ProtocolError::InvalidGlinkIntentCount);
        }
        *intent = GlinkIntent {
            size,
            intent_id: id,
        };
    }
    Ok((intents, count))
}

pub fn parse_glink_data_chunk<'a>(
    record: GlinkRecord<'a>,
    max_message: usize,
) -> Result<GlinkDataChunk<'a>, ProtocolError> {
    if !matches!(record.command, GLINK_CMD_TX_DATA | GLINK_CMD_TX_DATA_CONT)
        || record.body.len() < 8
        || max_message == 0
        || max_message > GLINK_MAX_MESSAGE
    {
        return Err(ProtocolError::InvalidGlinkDataChunk);
    }
    let size = u32::from_le_bytes(record.body[0..4].try_into().unwrap()) as usize;
    let left = u32::from_le_bytes(record.body[4..8].try_into().unwrap());
    let bytes = &record.body[8..];
    if size == 0
        || size > GLINK_MAX_TX_CHUNK
        || size != bytes.len()
        || left as usize > max_message
        || size > max_message
        || size + left as usize > max_message
    {
        return Err(ProtocolError::InvalidGlinkDataChunk);
    }
    Ok(GlinkDataChunk {
        channel_id: record.param1,
        intent_id: record.param2,
        bytes,
        left_size: left,
        continuation: record.command == GLINK_CMD_TX_DATA_CONT,
    })
}

fn align_glink(size: usize) -> usize {
    (size + GLINK_FIFO_ALIGNMENT - 1) & !(GLINK_FIFO_ALIGNMENT - 1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QrtrPacket<'a> {
    pub version: u8,
    pub packet_type: u32,
    pub source_node: u32,
    pub source_port: u32,
    pub destination_node: u32,
    pub destination_port: u32,
    pub confirm_rx: bool,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QrtrServerEvent {
    pub service_id: u32,
    pub version: u8,
    pub instance: u8,
    pub node: u32,
    pub port: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QmiMessage<'a> {
    pub message_type: u8,
    pub transaction_id: u16,
    pub message_id: u16,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QmiTlv<'a> {
    pub kind: u8,
    pub value: &'a [u8],
}

pub const WLFW_MAX_OPTIONAL_TLVS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlfwStatusResponse<'a> {
    pub result: u16,
    pub error: u16,
    pub optional_tlvs: [Option<QmiTlv<'a>>; WLFW_MAX_OPTIONAL_TLVS],
    pub optional_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeTargetPipeConfig {
    pub pipe_num: u32,
    pub pipe_dir: u32,
    pub entries: u32,
    pub max_bytes: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeServicePipeConfig {
    pub service_id: u32,
    pub pipe_dir: u32,
    pub pipe_num: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShadowRegisterConfig {
    pub id: u16,
    pub offset: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShadowRegisterV2Config {
    pub address: u32,
}

/// Borrowed WLFW CE profile. The caller owns the platform-specific values;
/// this value validates only the v01 schema and never authorizes hardware use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlanConfig<'a> {
    /// Optional top-level `host_version` TLV (tag 0x10).
    pub host_version: Option<&'a str>,
    pub target_pipes: &'a [CeTargetPipeConfig],
    pub service_pipes: &'a [CeServicePipeConfig],
    pub shadow_registers: &'a [ShadowRegisterConfig],
    /// `None` omits shadow_reg_v2; `Some(&[])` encodes it as valid and empty.
    pub shadow_registers_v2: Option<&'a [ShadowRegisterV2Config]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicFeatureMaskResponse {
    pub result: u16,
    pub error: u16,
    pub previous_mask: Option<u64>,
    pub current_mask: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WlanMode {
    Mission,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlfwIndicationRegistrationResponse {
    pub result: u16,
    pub error: u16,
    pub fw_status: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WlfwIndication {
    FirmwareReady,
    MsaReady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlfwCapabilityResponse<'a> {
    pub result: u16,
    pub error: u16,
    pub chip_info: Option<(u32, u32)>,
    pub board_id: Option<u32>,
    pub soc_id: Option<u32>,
    pub firmware_version: Option<u32>,
    pub firmware_timestamp: Option<&'a str>,
    pub firmware_build_id: Option<&'a str>,
    pub number_of_macs: Option<u8>,
    pub unknown_optional_tlvs: [Option<QmiTlv<'a>>; WLFW_MAX_OPTIONAL_TLVS],
    pub unknown_optional_count: usize,
}

pub struct QmiTlvIter<'a> {
    remaining: &'a [u8],
}

impl<'a> Iterator for QmiTlvIter<'a> {
    type Item = Result<QmiTlv<'a>, ProtocolError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        if self.remaining.len() < 3 {
            self.remaining = &[];
            return Some(Err(ProtocolError::TruncatedTlvHeader));
        }
        let kind = self.remaining[0];
        let len = u16::from_le_bytes([self.remaining[1], self.remaining[2]]) as usize;
        if len > self.remaining.len() - 3 {
            self.remaining = &[];
            return Some(Err(ProtocolError::TlvOverrun));
        }
        let value = &self.remaining[3..3 + len];
        self.remaining = &self.remaining[3 + len..];
        Some(Ok(QmiTlv { kind, value }))
    }
}

impl<'a> QmiMessage<'a> {
    pub fn tlvs(&self) -> QmiTlvIter<'a> {
        QmiTlvIter {
            remaining: self.payload,
        }
    }
}

pub fn parse_qrtr_packet(data: &[u8], max_payload: usize) -> Result<QrtrPacket<'_>, ProtocolError> {
    if data.is_empty() || data.len() & 3 != 0 {
        return Err(ProtocolError::EmptyOrUnalignedQrtr);
    }
    let (
        version,
        packet_type,
        source_node,
        source_port,
        destination_node,
        destination_port,
        confirm_rx,
        size,
        header_size,
    ) = match data[0] {
        1 => {
            if data.len() < QRTR_V1_HEADER_SIZE {
                return Err(ProtocolError::TruncatedQrtrHeader);
            }
            let f = [
                read_u32(data, 0)?,
                read_u32(data, 4)?,
                read_u32(data, 8)?,
                read_u32(data, 12)?,
                read_u32(data, 16)?,
                read_u32(data, 20)?,
                read_u32(data, 24)?,
                read_u32(data, 28)?,
            ];
            if f[0] != 1 || f[4] > 1 {
                return Err(ProtocolError::InvalidQrtrHeader);
            }
            (
                1,
                f[1],
                f[2],
                f[3],
                f[6],
                f[7],
                f[4] != 0,
                f[5] as usize,
                QRTR_V1_HEADER_SIZE,
            )
        }
        3 => {
            if data.len() < QRTR_V2_HEADER_SIZE {
                return Err(ProtocolError::TruncatedQrtrHeader);
            }
            let flags = data[2];
            if flags & !1 != 0 {
                return Err(ProtocolError::UnsupportedQrtrFlags);
            }
            let optional = data[3] as usize;
            let header_size = QRTR_V2_HEADER_SIZE + optional;
            if header_size > data.len() || header_size & 3 != 0 {
                return Err(ProtocolError::InvalidQrtrHeader);
            }
            let mut source_port = u16::from_le_bytes([data[10], data[11]]) as u32;
            let mut destination_port = u16::from_le_bytes([data[14], data[15]]) as u32;
            if source_port == 0xfffe {
                source_port = QRTR_PORT_CTRL;
            }
            if destination_port == 0xfffe {
                destination_port = QRTR_PORT_CTRL;
            }
            (
                3,
                data[1] as u32,
                u16::from_le_bytes([data[8], data[9]]) as u32,
                source_port,
                u16::from_le_bytes([data[12], data[13]]) as u32,
                destination_port,
                flags & 1 != 0,
                read_u32(data, 4)? as usize,
                header_size,
            )
        }
        _ => return Err(ProtocolError::UnsupportedQrtrVersion),
    };
    if size > max_payload {
        return Err(ProtocolError::QrtrPayloadTooLarge);
    }
    let padded_size = size
        .checked_add(3)
        .ok_or(ProtocolError::InvalidQrtrLength)?
        & !3;
    if data.len() != header_size + padded_size {
        return Err(ProtocolError::InvalidQrtrLength);
    }
    Ok(QrtrPacket {
        version,
        packet_type,
        source_node,
        source_port,
        destination_node,
        destination_port,
        confirm_rx,
        payload: &data[header_size..header_size + size],
    })
}

pub fn encode_wlfw_qrtr_lookup(output: &mut [u8]) -> Result<usize, ProtocolError> {
    if output.len() < QRTR_CTRL_PACKET_SIZE {
        return Err(ProtocolError::OutputTooSmall);
    }
    let version_instance = WLFW_SERVICE_VERSION as u32 | ((WLFW_SERVICE_INSTANCE as u32) << 8);
    for (index, value) in [
        QRTR_TYPE_NEW_LOOKUP,
        WLFW_SERVICE_ID,
        version_instance,
        0,
        0,
    ]
    .iter()
    .enumerate()
    {
        output[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(QRTR_CTRL_PACKET_SIZE)
}

pub fn parse_wlfw_qrtr_server_event(data: &[u8]) -> Result<QrtrServerEvent, ProtocolError> {
    if data.len() != QRTR_CTRL_PACKET_SIZE {
        return Err(ProtocolError::InvalidQrtrControl);
    }
    let command = read_u32(data, 0)?;
    let service = read_u32(data, 4)?;
    let version_instance = read_u32(data, 8)?;
    let node = read_u32(data, 12)?;
    let port = read_u32(data, 16)?;
    let version = version_instance as u8;
    let instance = (version_instance >> 8) as u8;
    if command != QRTR_TYPE_NEW_SERVER {
        return Err(ProtocolError::InvalidQrtrControl);
    }
    if version_instance != version as u32 | ((instance as u32) << 8)
        || (service, version, instance)
            != (WLFW_SERVICE_ID, WLFW_SERVICE_VERSION, WLFW_SERVICE_INSTANCE)
    {
        return Err(ProtocolError::WrongQrtrService);
    }
    if node == 0 || port == 0 {
        return Err(ProtocolError::InvalidQrtrEndpoint);
    }
    Ok(QrtrServerEvent {
        service_id: service,
        version,
        instance,
        node,
        port,
    })
}

pub fn parse_qmi_message(data: &[u8], max_payload: usize) -> Result<QmiMessage<'_>, ProtocolError> {
    if data.len() < QMI_HEADER_SIZE {
        return Err(ProtocolError::TruncatedQmiHeader);
    }
    let message_type = data[0];
    if !matches!(message_type, 0 | 2 | 4) {
        return Err(ProtocolError::UnsupportedQmiType);
    }
    let transaction_id = u16::from_le_bytes([data[1], data[2]]);
    let message_id = u16::from_le_bytes([data[3], data[4]]);
    let payload_size = u16::from_le_bytes([data[5], data[6]]) as usize;
    if payload_size > max_payload {
        return Err(ProtocolError::QmiPayloadTooLarge);
    }
    if data.len() != QMI_HEADER_SIZE + payload_size {
        return Err(ProtocolError::InvalidQmiLength);
    }
    let payload = &data[QMI_HEADER_SIZE..];
    let message = QmiMessage {
        message_type,
        transaction_id,
        message_id,
        payload,
    };
    for tlv in message.tlvs() {
        tlv?;
    }
    Ok(message)
}

/// Decode only the two empty startup indications used by the matched ath10k
/// path. They are unsolicited QMI messages with transaction ID zero.
pub fn decode_startup_indication(data: &[u8]) -> Result<WlfwIndication, ProtocolError> {
    let message = parse_qmi_message(data, 0)?;
    if message.message_type != 4 {
        return Err(ProtocolError::WrongQmiType);
    }
    if message.transaction_id != 0 {
        return Err(ProtocolError::WrongTransactionId);
    }
    match message.message_id {
        WLFW_FW_READY_IND_MESSAGE_ID => Ok(WlfwIndication::FirmwareReady),
        WLFW_MSA_READY_IND_MESSAGE_ID => Ok(WlfwIndication::MsaReady),
        _ => Err(ProtocolError::WrongMessageId),
    }
}

fn encode_wlfw_request(
    transaction_id: u16,
    message_id: u16,
    payload: &[u8],
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    let frame_size = QMI_HEADER_SIZE
        .checked_add(payload.len())
        .ok_or(ProtocolError::OutputTooSmall)?;
    if payload.len() > u16::MAX as usize || output.len() < frame_size {
        return Err(ProtocolError::OutputTooSmall);
    }
    output[0] = 0;
    output[1..3].copy_from_slice(&transaction_id.to_le_bytes());
    output[3..5].copy_from_slice(&message_id.to_le_bytes());
    output[5..7].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    output[7..frame_size].copy_from_slice(payload);
    Ok(frame_size)
}

/// Encode WLFW indication subscriptions. ICNSS enables `rejuvenate` under its
/// default `FW_REJUVENATE_ENABLE` quirk; other stacks may omit that TLV.
pub fn encode_indication_registration_request(
    transaction_id: u16,
    rejuvenate: bool,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    const DEFAULT: [u8; 19] = [
        0x10, 1, 0, 1, 0x13, 1, 0, 1, 0x14, 1, 0, 1, 0x15, 4, 0, 0x4c, 0x45, 0x4e, 0x4b,
    ];
    const REJUVENATE: [u8; 7] = [0x19, 4, 0, 1, 0, 0, 0];
    let extra = if rejuvenate { REJUVENATE.len() } else { 0 };
    let mut payload = [0u8; DEFAULT.len() + REJUVENATE.len()];
    payload[..DEFAULT.len()].copy_from_slice(&DEFAULT);
    if rejuvenate {
        payload[DEFAULT.len()..DEFAULT.len() + extra].copy_from_slice(&REJUVENATE);
    }
    encode_wlfw_request(
        transaction_id,
        WLFW_IND_REGISTER_MESSAGE_ID,
        &payload[..DEFAULT.len() + extra],
        output,
    )
}

pub fn encode_msa_ready_request(
    transaction_id: u16,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    encode_wlfw_request(transaction_id, WLFW_MSA_READY_MESSAGE_ID, &[], output)
}

pub fn encode_capability_request(
    transaction_id: u16,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    encode_wlfw_request(transaction_id, WLFW_CAP_MESSAGE_ID, &[], output)
}

/// Encode the ICNSS dynamic-feature-mask request (optional mask TLV 0x10).
pub fn encode_dynamic_feature_mask_request(
    transaction_id: u16,
    mask: u64,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    let mut payload = [0u8; 11];
    payload[..3].copy_from_slice(&[0x10, 8, 0]);
    payload[3..].copy_from_slice(&mask.to_le_bytes());
    encode_wlfw_request(
        transaction_id,
        WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
        &payload,
        output,
    )
}

/// Decode the common status plus the two optional ICNSS mask TLVs.
pub fn decode_dynamic_feature_mask_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<DynamicFeatureMaskResponse, ProtocolError> {
    let status = decode_wlfw_status_response(
        data,
        expected_transaction_id,
        WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
        WLFW_DYNAMIC_FEATURE_MASK_MAX_PAYLOAD_BYTES,
    )?;
    let mut previous_mask = None;
    let mut current_mask = None;
    for tlv in status
        .optional_tlvs
        .iter()
        .take(status.optional_count)
        .flatten()
    {
        if tlv.value.len() != 8 {
            return Err(ProtocolError::InvalidWlfwField);
        }
        let mask = u64::from_le_bytes(tlv.value.try_into().unwrap());
        let slot = match tlv.kind {
            0x10 => &mut previous_mask,
            0x11 => &mut current_mask,
            _ => return Err(ProtocolError::UnknownOptionalTlv),
        };
        if slot.replace(mask).is_some() {
            return Err(ProtocolError::DuplicateTlv);
        }
    }
    Ok(DynamicFeatureMaskResponse {
        result: status.result,
        error: status.error,
        previous_mask,
        current_mask,
    })
}

/// Encode one BDF_DOWNLOAD segment using the exact required ath10k TLVs.
/// The transport-specific chunk ceiling keeps the complete QMI frame inside a
/// 4096-byte QRTR datagram; `end` must be derived from the verified source
/// length and current segment offset by the caller.
pub fn encode_bdf_download_request(
    transaction_id: u16,
    total_size: u32,
    segment_id: u32,
    data: &[u8],
    end: bool,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    if total_size == 0
        || data.is_empty()
        || data.len() > WLFW_BDF_QRTR_MAX_DATA_BYTES
        || data.len() > WLFW_BDF_SCHEMA_MAX_DATA_BYTES
    {
        return Err(ProtocolError::InvalidWlfwField);
    }
    let payload_len = 34usize
        .checked_add(data.len())
        .ok_or(ProtocolError::OutputTooSmall)?;
    let frame_len = QMI_HEADER_SIZE
        .checked_add(payload_len)
        .ok_or(ProtocolError::OutputTooSmall)?;
    if output.len() < frame_len {
        return Err(ProtocolError::OutputTooSmall);
    }

    output[0] = 0;
    output[1..3].copy_from_slice(&transaction_id.to_le_bytes());
    output[3..5].copy_from_slice(&WLFW_BDF_DOWNLOAD_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(payload_len as u16).to_le_bytes());
    let mut at = QMI_HEADER_SIZE;
    output[at..at + 4].copy_from_slice(&[0x01, 1, 0, 1]);
    at += 4;
    output[at..at + 7].copy_from_slice(&[0x10, 4, 0, 0, 0, 0, 0]);
    at += 7;
    output[at..at + 3].copy_from_slice(&[0x11, 4, 0]);
    output[at + 3..at + 7].copy_from_slice(&total_size.to_le_bytes());
    at += 7;
    output[at..at + 3].copy_from_slice(&[0x12, 4, 0]);
    output[at + 3..at + 7].copy_from_slice(&segment_id.to_le_bytes());
    at += 7;
    let data_tlv_len = u16::try_from(data.len() + 2).map_err(|_| ProtocolError::OutputTooSmall)?;
    output[at..at + 3].copy_from_slice(&[0x13, data_tlv_len as u8, (data_tlv_len >> 8) as u8]);
    output[at + 3..at + 5].copy_from_slice(&(data.len() as u16).to_le_bytes());
    output[at + 5..at + 5 + data.len()].copy_from_slice(data);
    at += 5 + data.len();
    output[at..at + 4].copy_from_slice(&[0x14, 1, 0, u8::from(end)]);
    at += 4;
    debug_assert_eq!(at, frame_len);
    Ok(frame_len)
}

pub fn decode_bdf_download_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(
        data,
        expected_transaction_id,
        WLFW_BDF_DOWNLOAD_MESSAGE_ID,
        7,
    )
}

/// Encode the reference driver's empty calibration metadata report. The
/// ath10k Sunfish QMI object has no calibration-data population path, so it
/// reports a zero-length metadata array. Non-empty reports need verified
/// calibration objects and are deliberately rejected by this API.
pub fn encode_empty_cal_report_request(
    transaction_id: u16,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    const PAYLOAD: [u8; 4] = [0x01, 0x01, 0x00, 0x00];
    encode_wlfw_request(transaction_id, WLFW_CAL_REPORT_MESSAGE_ID, &PAYLOAD, output)
}

/// Encode a WLAN_CFG request with source-compatible optional TLV presence.
/// The supplied CE values remain platform policy inputs; this function does
/// not authorize a device operation.
pub fn encode_wlan_cfg_request(
    transaction_id: u16,
    config: WlanConfig<'_>,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    if config.target_pipes.is_empty()
        || config.target_pipes.len() > WLFW_MAX_CE_TARGET_CFG
        || config.service_pipes.is_empty()
        || config.service_pipes.len() > WLFW_MAX_CE_SERVICE_CFG
        || config.shadow_registers.len() > WLFW_MAX_SHADOW_REG_CFG
        || config
            .shadow_registers_v2
            .is_some_and(|registers| registers.len() > WLFW_MAX_SHADOW_REG_V2_CFG)
    {
        return Err(ProtocolError::InvalidWlfwField);
    }

    if let Some(version) = config.host_version {
        if version.len() > WLFW_MAX_HOST_VERSION_BYTES
            || !version.is_ascii()
            || version.as_bytes().contains(&0)
        {
            return Err(ProtocolError::InvalidWlfwString);
        }
    }

    let target_bytes = 4 + config.target_pipes.len() * 20;
    let service_bytes = 4 + config.service_pipes.len() * 12;
    let shadow_bytes = 4 + config.shadow_registers.len() * 4;
    let shadow_v2_bytes = config
        .shadow_registers_v2
        .map(|registers| 4 + registers.len() * 4);
    let mut payload_len = (3 + target_bytes)
        .checked_add(3 + service_bytes)
        .and_then(|value| value.checked_add(3 + shadow_bytes))
        .ok_or(ProtocolError::OutputTooSmall)?;
    if let Some(version) = config.host_version {
        payload_len = payload_len
            .checked_add(3 + version.len())
            .ok_or(ProtocolError::OutputTooSmall)?;
    }
    if let Some(shadow_v2_bytes) = shadow_v2_bytes {
        payload_len = payload_len
            .checked_add(3 + shadow_v2_bytes)
            .ok_or(ProtocolError::OutputTooSmall)?;
    }
    if payload_len > WLFW_WLAN_CFG_MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::InvalidWlfwField);
    }
    let frame_len = QMI_HEADER_SIZE
        .checked_add(payload_len)
        .ok_or(ProtocolError::OutputTooSmall)?;
    if output.len() < frame_len {
        return Err(ProtocolError::OutputTooSmall);
    }

    output[0] = 0;
    output[1..3].copy_from_slice(&transaction_id.to_le_bytes());
    output[3..5].copy_from_slice(&WLFW_WLAN_CFG_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(payload_len as u16).to_le_bytes());
    let mut at = QMI_HEADER_SIZE;

    if let Some(version) = config.host_version {
        write_tlv_header(output, &mut at, 0x10, version.len())?;
        output[at..at + version.len()].copy_from_slice(version.as_bytes());
        at += version.len();
    }

    write_tlv_header(output, &mut at, 0x11, target_bytes)?;
    write_u32(output, &mut at, config.target_pipes.len() as u32);
    for pipe in config.target_pipes {
        for value in [
            pipe.pipe_num,
            pipe.pipe_dir,
            pipe.entries,
            pipe.max_bytes,
            pipe.flags,
        ] {
            write_u32(output, &mut at, value);
        }
    }

    write_tlv_header(output, &mut at, 0x12, service_bytes)?;
    write_u32(output, &mut at, config.service_pipes.len() as u32);
    for pipe in config.service_pipes {
        for value in [pipe.service_id, pipe.pipe_dir, pipe.pipe_num] {
            write_u32(output, &mut at, value);
        }
    }

    write_tlv_header(output, &mut at, 0x13, shadow_bytes)?;
    write_u32(output, &mut at, config.shadow_registers.len() as u32);
    for register in config.shadow_registers {
        output[at..at + 2].copy_from_slice(&register.id.to_le_bytes());
        output[at + 2..at + 4].copy_from_slice(&register.offset.to_le_bytes());
        at += 4;
    }

    if let Some(registers) = config.shadow_registers_v2 {
        let value_len = 4 + registers.len() * 4;
        write_tlv_header(output, &mut at, 0x14, value_len)?;
        write_u32(output, &mut at, registers.len() as u32);
        for register in registers {
            write_u32(output, &mut at, register.address);
        }
    }

    debug_assert_eq!(at, frame_len);
    Ok(frame_len)
}

/// Encode the fixed mission-mode request. Debug mode is explicitly disabled,
/// matching the normal ath10k SNOC configuration.
pub fn encode_wlan_mode_request(
    transaction_id: u16,
    mode: WlanMode,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    const PAYLOAD_BYTES: usize = 11;
    let frame_len = QMI_HEADER_SIZE + PAYLOAD_BYTES;
    if output.len() < frame_len {
        return Err(ProtocolError::OutputTooSmall);
    }
    output[0] = 0;
    output[1..3].copy_from_slice(&transaction_id.to_le_bytes());
    output[3..5].copy_from_slice(&WLFW_WLAN_MODE_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(PAYLOAD_BYTES as u16).to_le_bytes());
    let mode_value = match mode {
        WlanMode::Mission => 0i32,
    };
    output[7..10].copy_from_slice(&[0x01, 4, 0]);
    output[10..14].copy_from_slice(&mode_value.to_le_bytes());
    output[14..18].copy_from_slice(&[0x10, 1, 0, 0]);
    Ok(frame_len)
}

fn write_tlv_header(
    output: &mut [u8],
    at: &mut usize,
    kind: u8,
    value_len: usize,
) -> Result<(), ProtocolError> {
    let value_len = u16::try_from(value_len).map_err(|_| ProtocolError::OutputTooSmall)?;
    output[*at..*at + 3].copy_from_slice(&[kind, value_len as u8, (value_len >> 8) as u8]);
    *at += 3;
    Ok(())
}

fn write_u32(output: &mut [u8], at: &mut usize, value: u32) {
    output[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
    *at += 4;
}

pub fn decode_wlan_cfg_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(data, expected_transaction_id, WLFW_WLAN_CFG_MESSAGE_ID, 7)
}

pub fn decode_wlan_mode_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(data, expected_transaction_id, WLFW_WLAN_MODE_MESSAGE_ID, 7)
}

pub fn decode_cal_report_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(data, expected_transaction_id, WLFW_CAL_REPORT_MESSAGE_ID, 7)
}

fn decode_wlfw_status_response(
    data: &[u8],
    expected_transaction_id: u16,
    expected_message_id: u16,
    max_payload: usize,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    let message = parse_qmi_message(data, max_payload)?;
    if message.message_type != 2 {
        return Err(ProtocolError::WrongQmiType);
    }
    if message.transaction_id != expected_transaction_id {
        return Err(ProtocolError::WrongTransactionId);
    }
    if message.message_id != expected_message_id {
        return Err(ProtocolError::WrongMessageId);
    }
    let mut common = None;
    let mut optional_tlvs = [None; WLFW_MAX_OPTIONAL_TLVS];
    let mut optional_count = 0;
    for item in message.tlvs() {
        let tlv = item?;
        match tlv.kind {
            0x02 => {
                if common.is_some() {
                    return Err(ProtocolError::DuplicateTlv);
                }
                if tlv.value.len() != 4 {
                    return Err(ProtocolError::InvalidCommonResult);
                }
                common = Some((
                    u16::from_le_bytes([tlv.value[0], tlv.value[1]]),
                    u16::from_le_bytes([tlv.value[2], tlv.value[3]]),
                ));
            }
            kind if kind < 0x10 => return Err(ProtocolError::UnknownMandatoryTlv),
            _ => {
                if optional_count == optional_tlvs.len() {
                    return Err(ProtocolError::TooManyOptionalTlvs);
                }
                optional_tlvs[optional_count] = Some(tlv);
                optional_count += 1;
            }
        }
    }
    let (result, error) = common.ok_or(ProtocolError::MissingCommonResult)?;
    Ok(WlfwStatusResponse {
        result,
        error,
        optional_tlvs,
        optional_count,
    })
}

pub fn decode_msa_ready_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(data, expected_transaction_id, WLFW_MSA_READY_MESSAGE_ID, 7)
}

pub fn decode_indication_registration_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(
        data,
        expected_transaction_id,
        WLFW_IND_REGISTER_MESSAGE_ID,
        18,
    )
}

pub fn decode_capability_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwStatusResponse<'_>, ProtocolError> {
    decode_wlfw_status_response(data, expected_transaction_id, WLFW_CAP_MESSAGE_ID, 207)
}

pub fn decode_indication_registration_details(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwIndicationRegistrationResponse, ProtocolError> {
    let status = decode_indication_registration_response(data, expected_transaction_id)?;
    let mut fw_status = None;
    for tlv in status
        .optional_tlvs
        .iter()
        .take(status.optional_count)
        .flatten()
    {
        if tlv.kind != 0x10 || fw_status.is_some() {
            return Err(ProtocolError::UnknownOptionalTlv);
        }
        if tlv.value.len() != 8 {
            return Err(ProtocolError::InvalidWlfwField);
        }
        fw_status = Some(u64::from_le_bytes(tlv.value.try_into().unwrap()));
    }
    Ok(WlfwIndicationRegistrationResponse {
        result: status.result,
        error: status.error,
        fw_status,
    })
}

fn decode_wlfw_string(value: &[u8], max_len: usize) -> Result<&str, ProtocolError> {
    let Some((&len, bytes)) = value.split_first() else {
        return Err(ProtocolError::InvalidWlfwString);
    };
    let len = len as usize;
    if len > max_len || bytes.len() != len || !bytes.is_ascii() {
        return Err(ProtocolError::InvalidWlfwString);
    }
    core::str::from_utf8(bytes).map_err(|_| ProtocolError::InvalidWlfwString)
}

pub fn decode_capability_details(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<WlfwCapabilityResponse<'_>, ProtocolError> {
    let status = decode_capability_response(data, expected_transaction_id)?;
    let mut response = WlfwCapabilityResponse {
        result: status.result,
        error: status.error,
        chip_info: None,
        board_id: None,
        soc_id: None,
        firmware_version: None,
        firmware_timestamp: None,
        firmware_build_id: None,
        number_of_macs: None,
        unknown_optional_tlvs: [None; WLFW_MAX_OPTIONAL_TLVS],
        unknown_optional_count: 0,
    };
    let mut seen = [false; 6];
    for tlv in status
        .optional_tlvs
        .iter()
        .take(status.optional_count)
        .flatten()
    {
        let slot = match tlv.kind {
            0x10..=0x15 => (tlv.kind - 0x10) as usize,
            _ => {
                let index = response.unknown_optional_count;
                response.unknown_optional_tlvs[index] = Some(*tlv);
                response.unknown_optional_count += 1;
                continue;
            }
        };
        if seen[slot] {
            return Err(ProtocolError::DuplicateTlv);
        }
        seen[slot] = true;
        match tlv.kind {
            0x10 if tlv.value.len() == 8 => {
                response.chip_info = Some((
                    u32::from_le_bytes(tlv.value[..4].try_into().unwrap()),
                    u32::from_le_bytes(tlv.value[4..].try_into().unwrap()),
                ));
            }
            0x11 if tlv.value.len() == 4 => {
                response.board_id = Some(u32::from_le_bytes(tlv.value.try_into().unwrap()));
            }
            0x12 if tlv.value.len() == 4 => {
                response.soc_id = Some(u32::from_le_bytes(tlv.value.try_into().unwrap()));
            }
            0x13 if tlv.value.len() >= 5 => {
                response.firmware_version =
                    Some(u32::from_le_bytes(tlv.value[..4].try_into().unwrap()));
                response.firmware_timestamp = Some(decode_wlfw_string(&tlv.value[4..], 32)?);
            }
            0x14 => response.firmware_build_id = Some(decode_wlfw_string(tlv.value, 128)?),
            0x15 if tlv.value.len() == 1 => response.number_of_macs = Some(tlv.value[0]),
            _ => return Err(ProtocolError::InvalidWlfwField),
        }
    }
    Ok(response)
}

pub fn encode_msa_info_request(
    transaction_id: u16,
    address: u64,
    size: u32,
    output: &mut [u8],
) -> Result<usize, ProtocolError> {
    const FRAME_SIZE: usize = 25;
    if output.len() < FRAME_SIZE {
        return Err(ProtocolError::OutputTooSmall);
    }
    output[0] = 0;
    output[1..3].copy_from_slice(&transaction_id.to_le_bytes());
    output[3..5].copy_from_slice(&WLFW_MSA_INFO_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&18u16.to_le_bytes());
    output[7..10].copy_from_slice(&[0x01, 8, 0]);
    output[10..18].copy_from_slice(&address.to_le_bytes());
    output[18..21].copy_from_slice(&[0x02, 4, 0]);
    output[21..25].copy_from_slice(&size.to_le_bytes());
    Ok(FRAME_SIZE)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsaRegion {
    pub address: u64,
    pub size: u32,
    pub secure: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsaInfoResponse {
    pub result: u16,
    pub error: u16,
    pub regions: [MsaRegion; WLFW_MAX_MSA_REGIONS],
    pub region_count: usize,
}

pub fn decode_msa_info_response(
    data: &[u8],
    expected_transaction_id: u16,
) -> Result<MsaInfoResponse, ProtocolError> {
    let message = parse_qmi_message(data, 37)?;
    if message.message_type != 2 {
        return Err(ProtocolError::WrongQmiType);
    }
    if message.transaction_id != expected_transaction_id {
        return Err(ProtocolError::WrongTransactionId);
    }
    if message.message_id != WLFW_MSA_INFO_MESSAGE_ID {
        return Err(ProtocolError::WrongMessageId);
    }
    let mut common = None;
    let mut region_data = None;
    for item in message.tlvs() {
        let tlv = item?;
        match tlv.kind {
            0x02 => {
                if common.is_some() {
                    return Err(ProtocolError::DuplicateTlv);
                }
                if tlv.value.len() != 4 {
                    return Err(ProtocolError::InvalidCommonResult);
                }
                common = Some((
                    u16::from_le_bytes([tlv.value[0], tlv.value[1]]),
                    u16::from_le_bytes([tlv.value[2], tlv.value[3]]),
                ));
            }
            0x03 => {
                if region_data.is_some() {
                    return Err(ProtocolError::DuplicateTlv);
                }
                region_data = Some(tlv.value);
            }
            kind if kind < 0x10 => return Err(ProtocolError::UnknownMandatoryTlv),
            _ => {}
        }
    }
    let (result, error) = common.ok_or(ProtocolError::MissingCommonResult)?;
    let empty = [MsaRegion {
        address: 0,
        size: 0,
        secure: false,
    }; WLFW_MAX_MSA_REGIONS];
    if result != 0 {
        if region_data.is_some() {
            return Err(ProtocolError::FailedResponseHasRegions);
        }
        return Ok(MsaInfoResponse {
            result,
            error,
            regions: empty,
            region_count: 0,
        });
    }
    let raw = region_data.ok_or(ProtocolError::MissingMsaRegions)?;
    if raw.is_empty() {
        return Err(ProtocolError::InvalidMsaRegions);
    }
    let count = raw[0] as usize;
    if count > WLFW_MAX_MSA_REGIONS {
        return Err(ProtocolError::TooManyMsaRegions);
    }
    if raw.len() != 1 + count * 13 {
        return Err(ProtocolError::InvalidMsaRegions);
    }
    let mut regions = empty;
    for (index, region) in regions.iter_mut().take(count).enumerate() {
        let offset = 1 + index * 13;
        let secure = raw[offset + 12];
        if secure > 1 {
            return Err(ProtocolError::InvalidSecureFlag);
        }
        *region = MsaRegion {
            address: u64::from_le_bytes(raw[offset..offset + 8].try_into().unwrap()),
            size: u32::from_le_bytes(raw[offset + 8..offset + 12].try_into().unwrap()),
            secure: secure != 0,
        };
    }
    Ok(MsaInfoResponse {
        result,
        error,
        regions,
        region_count: count,
    })
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, ProtocolError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(ProtocolError::TruncatedQrtrHeader)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qmi_frame(message_type: u8, tx: u16, id: u16, payload: &[u8]) -> std::vec::Vec<u8> {
        let mut frame = std::vec::Vec::from([message_type]);
        frame.extend_from_slice(&tx.to_le_bytes());
        frame.extend_from_slice(&id.to_le_bytes());
        frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn parses_qrtr_v1_and_v2_and_server_event() {
        let event = [4u32, 0x45, 1, 5, 700]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<std::vec::Vec<_>>();
        let mut v1 = [1u32, 1, 5, 700, 1, 20, 0, QRTR_PORT_CTRL]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<std::vec::Vec<_>>();
        v1.extend_from_slice(&event);
        let parsed = parse_qrtr_packet(&v1, 64).unwrap();
        assert_eq!(
            (parsed.source_node, parsed.source_port, parsed.confirm_rx),
            (5, 700, true)
        );
        assert_eq!(
            parse_wlfw_qrtr_server_event(parsed.payload).unwrap().port,
            700
        );

        let mut v2 = [3, 1, 1, 4].to_vec();
        v2.extend_from_slice(&20u32.to_le_bytes());
        v2.extend_from_slice(&[5, 0, 0xbc, 2, 0, 0, 0xfe, 0xff]);
        v2.extend_from_slice(&[0; 4]);
        v2.extend_from_slice(&event);
        let parsed = parse_qrtr_packet(&v2, 64).unwrap();
        assert_eq!(parsed.destination_port, QRTR_PORT_CTRL);
        assert_eq!(
            parse_wlfw_qrtr_server_event(parsed.payload).unwrap().node,
            5
        );
    }

    #[test]
    fn startup_indications_require_exact_empty_unsolicited_frames() {
        let msa_ready = qmi_frame(4, 0, WLFW_MSA_READY_IND_MESSAGE_ID, &[]);
        assert_eq!(
            decode_startup_indication(&msa_ready),
            Ok(WlfwIndication::MsaReady)
        );
        let firmware_ready = qmi_frame(4, 0, WLFW_FW_READY_IND_MESSAGE_ID, &[]);
        assert_eq!(
            decode_startup_indication(&firmware_ready),
            Ok(WlfwIndication::FirmwareReady)
        );
        assert_eq!(
            decode_startup_indication(&qmi_frame(2, 0, WLFW_MSA_READY_IND_MESSAGE_ID, &[])),
            Err(ProtocolError::WrongQmiType)
        );
        assert_eq!(
            decode_startup_indication(&qmi_frame(4, 1, WLFW_MSA_READY_IND_MESSAGE_ID, &[])),
            Err(ProtocolError::WrongTransactionId)
        );
        assert_eq!(
            decode_startup_indication(&qmi_frame(4, 0, 0x7fff, &[])),
            Err(ProtocolError::WrongMessageId)
        );
        assert_eq!(
            decode_startup_indication(&qmi_frame(
                4,
                0,
                WLFW_MSA_READY_IND_MESSAGE_ID,
                &[1, 1, 0, 1]
            )),
            Err(ProtocolError::QmiPayloadTooLarge)
        );
    }

    #[test]
    fn bdf_segments_fit_one_bounded_qrtr_datagram_and_match_the_source_schema() {
        let bytes = [0xa5; WLFW_BDF_QRTR_MAX_DATA_BYTES];
        let mut frame = [0u8; 4096];
        let len = encode_bdf_download_request(0x1234, 9000, 0, &bytes, false, &mut frame).unwrap();
        assert_eq!(len, 4064);
        let parsed = parse_qmi_message(&frame[..len], 4096).unwrap();
        assert_eq!(
            (
                parsed.message_type,
                parsed.transaction_id,
                parsed.message_id,
                parsed.payload.len()
            ),
            (0, 0x1234, WLFW_BDF_DOWNLOAD_MESSAGE_ID, 4057)
        );
        let tlvs = parsed
            .tlvs()
            .collect::<Result<std::vec::Vec<_>, _>>()
            .unwrap();
        assert_eq!(tlvs.len(), 6);
        assert_eq!((tlvs[0].kind, tlvs[0].value), (0x01, &[1][..]));
        assert_eq!((tlvs[1].kind, tlvs[1].value), (0x10, &[0, 0, 0, 0][..]));
        assert_eq!(
            (
                tlvs[2].kind,
                u32::from_le_bytes(tlvs[2].value.try_into().unwrap())
            ),
            (0x11, 9000)
        );
        assert_eq!(
            (
                tlvs[3].kind,
                u32::from_le_bytes(tlvs[3].value.try_into().unwrap())
            ),
            (0x12, 0)
        );
        assert_eq!(tlvs[4].kind, 0x13);
        assert_eq!(
            u16::from_le_bytes(tlvs[4].value[..2].try_into().unwrap()) as usize,
            bytes.len()
        );
        assert_eq!(&tlvs[4].value[2..], &bytes);
        assert_eq!((tlvs[5].kind, tlvs[5].value), (0x14, &[0][..]));
        assert_eq!(
            decode_bdf_download_response(
                &qmi_frame(
                    2,
                    0x1234,
                    WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                    &[2, 4, 0, 0, 0, 0, 0]
                ),
                0x1234
            )
            .unwrap()
            .result,
            0
        );

        assert_eq!(
            encode_bdf_download_request(1, 9, 0, &[], true, &mut frame),
            Err(ProtocolError::InvalidWlfwField)
        );
        assert_eq!(
            encode_bdf_download_request(
                1,
                9,
                0,
                &[1; WLFW_BDF_QRTR_MAX_DATA_BYTES + 1],
                true,
                &mut frame
            ),
            Err(ProtocolError::InvalidWlfwField)
        );
        assert_eq!(
            encode_bdf_download_request(1, 9, 0, &[1], true, &mut [0; 41]),
            Err(ProtocolError::OutputTooSmall)
        );
    }

    #[test]
    fn empty_cal_report_matches_the_pinned_ath10k_schema() {
        let mut request = [0u8; 11];
        let len = encode_empty_cal_report_request(0x1234, &mut request).unwrap();
        assert_eq!(&request[..len], &[0, 0x34, 0x12, 0x26, 0, 4, 0, 1, 1, 0, 0]);
        let parsed = parse_qmi_message(&request[..len], 7).unwrap();
        assert_eq!(
            (
                parsed.message_type,
                parsed.transaction_id,
                parsed.message_id
            ),
            (0, 0x1234, WLFW_CAL_REPORT_MESSAGE_ID)
        );
        assert_eq!(
            parsed.tlvs().next().unwrap().unwrap(),
            QmiTlv {
                kind: 1,
                value: &[0]
            }
        );
        assert_eq!(
            decode_cal_report_response(
                &qmi_frame(
                    2,
                    0x1234,
                    WLFW_CAL_REPORT_MESSAGE_ID,
                    &[2, 4, 0, 0, 0, 0, 0]
                ),
                0x1234
            )
            .unwrap()
            .result,
            0
        );
        assert_eq!(
            decode_cal_report_response(
                &qmi_frame(
                    2,
                    0x1235,
                    WLFW_CAL_REPORT_MESSAGE_ID,
                    &[2, 4, 0, 0, 0, 0, 0]
                ),
                0x1234
            ),
            Err(ProtocolError::WrongTransactionId)
        );
        assert_eq!(
            encode_empty_cal_report_request(1, &mut [0; 10]),
            Err(ProtocolError::OutputTooSmall)
        );
    }

    #[test]
    fn qrtr_lookup_and_bad_packets() {
        let mut out = [0u8; QRTR_CTRL_PACKET_SIZE];
        encode_wlfw_qrtr_lookup(&mut out).unwrap();
        assert_eq!(
            out,
            [10u32, 0x45, 1, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<std::vec::Vec<_>>()[..]
        );
        assert_eq!(
            parse_qrtr_packet(&[1, 0, 0, 0], 1),
            Err(ProtocolError::TruncatedQrtrHeader)
        );
        let mut bad = [3, 1, 2, 0].to_vec();
        bad.extend_from_slice(&[0; 12]);
        assert_eq!(
            parse_qrtr_packet(&bad, 10),
            Err(ProtocolError::UnsupportedQrtrFlags)
        );
    }

    #[test]
    fn qmi_tlvs_are_bounded_and_msa_request_matches_wire_layout() {
        let mut request = [0u8; 25];
        let len = encode_msa_info_request(0x1234, 0x98d00000, 0x200000, &mut request).unwrap();
        assert_eq!(len, 25);
        let parsed = parse_qmi_message(&request, 64).unwrap();
        assert_eq!(
            (
                parsed.message_type,
                parsed.transaction_id,
                parsed.message_id
            ),
            (0, 0x1234, WLFW_MSA_INFO_MESSAGE_ID)
        );
        let tlvs = parsed
            .tlvs()
            .collect::<Result<std::vec::Vec<_>, _>>()
            .unwrap();
        assert_eq!(tlvs[0].value, &0x98d00000u64.to_le_bytes());
        assert_eq!(tlvs[1].value, &0x200000u32.to_le_bytes());
        assert_eq!(
            parse_qmi_message(&[2, 0], 64),
            Err(ProtocolError::TruncatedQmiHeader)
        );
        let bad = qmi_frame(2, 1, 2, &[1, 2]);
        assert_eq!(
            parse_qmi_message(&bad, 64),
            Err(ProtocolError::TruncatedTlvHeader)
        );
    }

    #[test]
    fn encodes_startup_requests_with_matched_wlfw_layouts() {
        let mut frame = [0u8; 40];
        let len = encode_indication_registration_request(5, false, &mut frame).unwrap();
        let parsed = parse_qmi_message(&frame[..len], 64).unwrap();
        assert_eq!((parsed.message_id, parsed.transaction_id), (0x20, 5));
        let tlvs = parsed
            .tlvs()
            .collect::<Result<std::vec::Vec<_>, _>>()
            .unwrap();
        assert_eq!(tlvs[0].value, &[1]);
        assert_eq!(tlvs[1].kind, 0x13);
        assert_eq!(tlvs[2].kind, 0x14);
        assert_eq!(tlvs[3].value, &WLFW_CLIENT_ID.to_le_bytes());
        assert_eq!(tlvs.len(), 4);

        let len = encode_indication_registration_request(6, true, &mut frame).unwrap();
        let parsed = parse_qmi_message(&frame[..len], 64).unwrap();
        assert_eq!(parsed.tlvs().last().unwrap().unwrap().kind, 0x19);

        let len = encode_msa_ready_request(7, &mut frame).unwrap();
        let parsed = parse_qmi_message(&frame[..len], 64).unwrap();
        assert_eq!((parsed.message_id, parsed.payload), (0x2e, &[][..]));
        let len = encode_capability_request(8, &mut frame).unwrap();
        let parsed = parse_qmi_message(&frame[..len], 64).unwrap();
        assert_eq!((parsed.message_id, parsed.payload), (0x24, &[][..]));
        assert_eq!(
            encode_capability_request(8, &mut [0u8; 6]),
            Err(ProtocolError::OutputTooSmall)
        );
    }

    #[test]
    fn decodes_startup_status_and_capability_details_with_schema_bounds() {
        let common = [2, 4, 0, 0, 0, 0, 0];
        let frame = qmi_frame(2, 11, 0x2e, &common);
        assert_eq!(decode_msa_ready_response(&frame, 11).unwrap().result, 0);
        let frame = qmi_frame(2, 12, 0x20, &common);
        assert_eq!(
            decode_indication_registration_response(&frame, 12)
                .unwrap()
                .result,
            0
        );
        let frame = qmi_frame(2, 13, 0x24, &common);
        assert_eq!(decode_capability_response(&frame, 13).unwrap().result, 0);

        let mut payload = common.to_vec();
        payload.extend_from_slice(&[0x10, 8, 0]);
        payload.extend_from_slice(&0x3990u32.to_le_bytes());
        payload.extend_from_slice(&0x1234u32.to_le_bytes());
        payload.extend_from_slice(&[0x11, 4, 0]);
        payload.extend_from_slice(&0x4au32.to_le_bytes());
        payload.extend_from_slice(&[0x12, 4, 0]);
        payload.extend_from_slice(&0x123456u32.to_le_bytes());
        payload.extend_from_slice(&[0x13, 9, 0]);
        payload.extend_from_slice(&0x10203u32.to_le_bytes());
        payload.extend_from_slice(b"\x04test");
        payload.extend_from_slice(b"\x14\x04\x00\x03abc\x15\x01\x00\x02\x16\x02\x00xy");
        let frame = qmi_frame(2, 21, 0x24, &payload);
        let decoded = decode_capability_details(&frame, 21).unwrap();
        assert_eq!(decoded.chip_info, Some((0x3990, 0x1234)));
        assert_eq!(
            (decoded.board_id, decoded.soc_id),
            (Some(0x4a), Some(0x123456))
        );
        assert_eq!(decoded.firmware_version, Some(0x10203));
        assert_eq!(decoded.firmware_timestamp, Some("test"));
        assert_eq!(decoded.firmware_build_id, Some("abc"));
        assert_eq!(decoded.number_of_macs, Some(2));
        assert_eq!(decoded.unknown_optional_count, 1);

        let mut indication = common.to_vec();
        indication.extend_from_slice(&[0x10, 8, 0]);
        indication.extend_from_slice(&6u64.to_le_bytes());
        let frame = qmi_frame(2, 22, 0x20, &indication);
        assert_eq!(
            decode_indication_registration_details(&frame, 22)
                .unwrap()
                .fw_status,
            Some(6)
        );

        let oversized = qmi_frame(2, 11, 0x2e, &[0; 8]);
        assert_eq!(
            decode_msa_ready_response(&oversized, 11),
            Err(ProtocolError::QmiPayloadTooLarge)
        );
        let mut bad = common.to_vec();
        bad.extend_from_slice(b"\x14\x02\x00\x05x");
        let frame = qmi_frame(2, 23, 0x24, &bad);
        assert_eq!(
            decode_capability_details(&frame, 23),
            Err(ProtocolError::InvalidWlfwString)
        );
    }

    #[test]
    fn decodes_msa_regions_and_rejects_invalid_secure_flags() {
        let records = [
            (0x98d00000u64.to_le_bytes(), 0x4000u32.to_le_bytes(), 1u8),
            (0x98d04000u64.to_le_bytes(), 0x1fc000u32.to_le_bytes(), 0u8),
        ];
        let mut regions = vec![2];
        for (address, size, secure) in records {
            regions.extend_from_slice(&address);
            regions.extend_from_slice(&size);
            regions.push(secure);
        }
        let mut payload = vec![2, 4, 0, 0, 0, 0, 0, 3, 27, 0];
        payload.extend_from_slice(&regions);
        let frame = qmi_frame(2, 7, WLFW_MSA_INFO_MESSAGE_ID, &payload);
        let decoded = decode_msa_info_response(&frame, 7).unwrap();
        assert_eq!(decoded.region_count, 2);
        assert_eq!(
            decoded.regions[0],
            MsaRegion {
                address: 0x98d00000,
                size: 0x4000,
                secure: true
            }
        );
        assert_eq!(
            decoded.regions[1],
            MsaRegion {
                address: 0x98d04000,
                size: 0x1fc000,
                secure: false
            }
        );

        let mut bad_payload = vec![2, 4, 0, 0, 0, 0, 0, 3, 14, 0, 1];
        bad_payload.extend_from_slice(&1u64.to_le_bytes());
        bad_payload.extend_from_slice(&1u32.to_le_bytes());
        bad_payload.push(2);
        let bad = qmi_frame(2, 7, WLFW_MSA_INFO_MESSAGE_ID, &bad_payload);
        assert_eq!(
            decode_msa_info_response(&bad, 7),
            Err(ProtocolError::InvalidSecureFlag)
        );
    }

    fn glink_record(command: u16, param1: u16, param2: u32, body: &[u8]) -> std::vec::Vec<u8> {
        let mut bytes = command.to_le_bytes().to_vec();
        bytes.extend_from_slice(&param1.to_le_bytes());
        bytes.extend_from_slice(&param2.to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.resize(align_glink(bytes.len()), 0);
        bytes
    }

    #[test]
    fn parses_glink_control_open_intent_and_data_records() {
        let version_bytes = glink_record(GLINK_CMD_VERSION, 1, 0, &[]);
        let version = parse_glink_record(&version_bytes, 4096).unwrap();
        assert_eq!(
            (version.command, version.param1, version.param2),
            (GLINK_CMD_VERSION, 1, 0)
        );

        let open_bytes = glink_record(GLINK_CMD_OPEN, 3, 5, b"IPCR\0");
        let open = parse_glink_record(&open_bytes, 4096).unwrap();
        assert_eq!(open.body, b"IPCR\0");

        let mut intent_body = 0x800u32.to_le_bytes().to_vec();
        intent_body.extend_from_slice(&7u32.to_le_bytes());
        let intent_bytes = glink_record(GLINK_CMD_INTENT, 3, 1, &intent_body);
        let intents = parse_glink_record(&intent_bytes, 4096).unwrap();
        let (intents, count) = parse_glink_intents(intents).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            intents[0],
            GlinkIntent {
                size: 0x800,
                intent_id: 7
            }
        );

        let mut data_body = 3u32.to_le_bytes().to_vec();
        data_body.extend_from_slice(&0u32.to_le_bytes());
        data_body.extend_from_slice(b"abc");
        let data_bytes = glink_record(GLINK_CMD_TX_DATA, 3, 7, &data_body);
        let record = parse_glink_record(&data_bytes, 4096).unwrap();
        let chunk = parse_glink_data_chunk(record, 4096).unwrap();
        assert_eq!(
            (
                chunk.channel_id,
                chunk.intent_id,
                chunk.bytes,
                chunk.left_size
            ),
            (3, 7, b"abc".as_slice(), 0)
        );
        assert!(!chunk.continuation);
    }

    #[test]
    fn rejects_malformed_glink_lengths_and_bounds() {
        let malformed_open = glink_record(GLINK_CMD_OPEN, 1, 3, b"abc\0");
        assert_eq!(
            parse_glink_record(&malformed_open, 4096),
            Err(ProtocolError::InvalidGlinkOpenName)
        );
        assert_eq!(
            parse_glink_record(&glink_record(99, 0, 0, &[]), 4096),
            Err(ProtocolError::UnknownGlinkCommand)
        );
        let mut bad_intent = 1u32.to_le_bytes().to_vec();
        bad_intent.extend_from_slice(&0u32.to_le_bytes());
        let bad_intent_bytes = glink_record(GLINK_CMD_INTENT, 1, 1, &bad_intent);
        let record = parse_glink_record(&bad_intent_bytes, 4096).unwrap();
        assert_eq!(
            parse_glink_intents(record),
            Err(ProtocolError::InvalidGlinkIntentCount)
        );
    }
}
