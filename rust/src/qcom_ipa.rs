//! Bounded IPA `INIT_MODEM_DRIVER` QMI client codec for Sunfish.
//!
//! The Linux 4.14 Sunfish IPA v3 driver looks up service 0x31/version 1,
//! modem instance 2, and sends message 0x0021 using live IPA memory-partition,
//! endpoint, hash-table, and uC/SSR state. This module encodes that transaction
//! from an already validated platform snapshot; it does not discover or invent
//! those resource values, implement the AP-side IPA provider, or touch hardware.
#![forbid(unsafe_code)]

use crate::qcom_qmi::{self, Kind, Message};

pub const SERVICE_ID: u16 = 0x31;
pub const SERVICE_VERSION: u16 = 1;
pub const MODEM_INSTANCE: u8 = 2;
/// QRTR service descriptor encoding: `(instance << 8) | version`.
pub const QRTR_SERVICE_ID: u32 = SERVICE_ID as u32;
pub const QRTR_VERSION_INSTANCE: u32 = ((MODEM_INSTANCE as u32) << 8) | SERVICE_VERSION as u32;
pub const INIT_MODEM_DRIVER_MESSAGE_ID: u16 = 0x0021;
pub const MAX_TLV_BYTES: usize = 162;
pub const QMI_HEADER_BYTES: usize = 7;
pub const MAX_QMI_BYTES: usize = QMI_HEADER_BYTES + MAX_TLV_BYTES;
pub const MAX_IPA_ENDPOINT: u32 = 19;
pub const MAX_ROUTE_INDICES: u32 = 31;

const TLV_PLATFORM_TYPE: u8 = 0x10;
const TLV_HEADER_TABLE: u8 = 0x11;
const TLV_V4_ROUTE: u8 = 0x12;
const TLV_V6_ROUTE: u8 = 0x13;
const TLV_V4_FILTER: u8 = 0x14;
const TLV_V6_FILTER: u8 = 0x15;
const TLV_MODEM_MEMORY: u8 = 0x16;
const TLV_CONTROL_ENDPOINT: u8 = 0x17;
const TLV_SSR_BOOT: u8 = 0x18;
const TLV_HEADER_PROCESSING: u8 = 0x19;
const TLV_COMPRESSION: u8 = 0x1a;
const TLV_V4_HASH_ROUTE: u8 = 0x1b;
const TLV_V6_HASH_ROUTE: u8 = 0x1c;
const TLV_V4_HASH_FILTER: u8 = 0x1d;
const TLV_V6_HASH_FILTER: u8 = 0x1e;
const TLV_STATS_QUOTA_BASE: u8 = 0x1f;
const TLV_STATS_QUOTA_SIZE: u8 = 0x20;
const TLV_DROP_STATS_BASE: u8 = 0x21;
const TLV_DROP_STATS_SIZE: u8 = 0x22;
const TLV_RESPONSE: u8 = 0x02;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    OutputTooSmall,
    InvalidMemoryRange,
    InvalidRouteCount,
    InvalidEndpoint,
    InvalidResponse,
    InvalidValue,
    Qmi(qcom_qmi::Error),
}

impl From<qcom_qmi::Error> for Error {
    fn from(value: qcom_qmi::Error) -> Self {
        Self::Qmi(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum PlatformType {
    Tn = 1,
    Le = 2,
    MsmAndroid = 3,
    MsmWindows = 4,
    MsmQnx = 5,
    LeMhi = 6,
}

/// An already resolved inclusive offset range within IPA shared memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OffsetRange {
    pub start: u32,
    pub end_inclusive: u32,
}

/// A modem-owned block within IPA shared memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryBlock {
    pub start: u32,
    pub size: u32,
}

/// A shared-memory base/size pair that may be empty when the source platform
/// still marks the corresponding QMI fields valid with a zero size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemorySpan {
    pub start: u32,
    pub size: u32,
}

/// Routing-table base and number of indices assigned to the modem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteTable {
    pub start: u32,
    pub num_indices: u32,
}

/// The optional hashable table fields are present together when IPA supports
/// hash routing/filtering; the Linux client omits all four when it does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HashTables {
    pub v4_route: RouteTable,
    pub v6_route: RouteTable,
    pub v4_filter_start: u32,
    pub v6_filter_start: u32,
}

/// Resource-dependent request fields captured from an initialized IPA host.
/// Addresses are shared-memory offsets, not physical addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitRequestFields {
    pub platform_type: PlatformType,
    /// Exclusive byte limit for QMI offsets in the IPA SRAM aperture. When
    /// sourced from `IPA_SHARED_MEM_SIZE`, this is `baddr_bytes + size_bytes`.
    pub ipa_shared_memory_end_exclusive: u32,
    pub header_table: Option<OffsetRange>,
    pub v4_route: RouteTable,
    pub v6_route: RouteTable,
    pub v4_filter_start: u32,
    pub v6_filter_start: u32,
    pub modem_memory: Option<MemoryBlock>,
    pub control_endpoint: Option<u32>,
    /// `None` means Linux has not loaded the IPA uC state and therefore omits
    /// the SSR TLV; `Some(false/true)` is the observed first/SSR boot value.
    pub is_ssr_bootup: Option<bool>,
    pub header_processing_table: Option<OffsetRange>,
    pub compression_table: Option<OffsetRange>,
    pub hash_tables: Option<HashTables>,
    pub stats_quota: MemorySpan,
    pub drop_stats: MemorySpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitResponse {
    pub result: u16,
    pub error: u16,
    pub control_endpoint: Option<u32>,
    pub default_endpoint: Option<u32>,
    pub modem_driver_init_pending: Option<bool>,
}

impl InitRequestFields {
    /// Encode one complete QMI request. The caller must supply the live IPA
    /// resource snapshot; output remains unchanged on validation/capacity
    /// errors. Request fields follow the pinned `ipa_qmi_service_v01.c` TLV
    /// order and the Linux client's 162-byte maximum.
    pub fn encode(&self, transaction: u16, output: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;

        let mut payload = [0u8; MAX_TLV_BYTES];
        let mut payload_len = 0usize;
        append_tlv(
            &mut payload,
            &mut payload_len,
            TLV_PLATFORM_TYPE,
            &((self.platform_type as i32).to_le_bytes()),
        )?;
        if let Some(range) = self.header_table {
            append_range(&mut payload, &mut payload_len, TLV_HEADER_TABLE, range)?;
        }
        append_route(&mut payload, &mut payload_len, TLV_V4_ROUTE, self.v4_route)?;
        append_route(&mut payload, &mut payload_len, TLV_V6_ROUTE, self.v6_route)?;
        append_u32(
            &mut payload,
            &mut payload_len,
            TLV_V4_FILTER,
            self.v4_filter_start,
        )?;
        append_u32(
            &mut payload,
            &mut payload_len,
            TLV_V6_FILTER,
            self.v6_filter_start,
        )?;
        if let Some(block) = self.modem_memory {
            append_block(&mut payload, &mut payload_len, TLV_MODEM_MEMORY, block)?;
        }
        if let Some(endpoint) = self.control_endpoint {
            append_u32(
                &mut payload,
                &mut payload_len,
                TLV_CONTROL_ENDPOINT,
                endpoint,
            )?;
        }
        if let Some(is_ssr) = self.is_ssr_bootup {
            append_tlv(
                &mut payload,
                &mut payload_len,
                TLV_SSR_BOOT,
                &[u8::from(is_ssr)],
            )?;
        }
        if let Some(range) = self.header_processing_table {
            append_range(&mut payload, &mut payload_len, TLV_HEADER_PROCESSING, range)?;
        }
        if let Some(range) = self.compression_table {
            append_range(&mut payload, &mut payload_len, TLV_COMPRESSION, range)?;
        }
        if let Some(hash) = self.hash_tables {
            append_route(
                &mut payload,
                &mut payload_len,
                TLV_V4_HASH_ROUTE,
                hash.v4_route,
            )?;
            append_route(
                &mut payload,
                &mut payload_len,
                TLV_V6_HASH_ROUTE,
                hash.v6_route,
            )?;
            append_u32(
                &mut payload,
                &mut payload_len,
                TLV_V4_HASH_FILTER,
                hash.v4_filter_start,
            )?;
            append_u32(
                &mut payload,
                &mut payload_len,
                TLV_V6_HASH_FILTER,
                hash.v6_filter_start,
            )?;
        }
        append_u32(
            &mut payload,
            &mut payload_len,
            TLV_STATS_QUOTA_BASE,
            self.stats_quota.start,
        )?;
        append_u32(
            &mut payload,
            &mut payload_len,
            TLV_STATS_QUOTA_SIZE,
            self.stats_quota.size,
        )?;
        append_u32(
            &mut payload,
            &mut payload_len,
            TLV_DROP_STATS_BASE,
            self.drop_stats.start,
        )?;
        append_u32(
            &mut payload,
            &mut payload_len,
            TLV_DROP_STATS_SIZE,
            self.drop_stats.size,
        )?;

        let message_len = QMI_HEADER_BYTES + payload_len;
        if output.len() < message_len {
            return Err(Error::OutputTooSmall);
        }
        let mut message = [0u8; MAX_QMI_BYTES];
        message[0] = 0; // QMI request
        message[1..3].copy_from_slice(&transaction.to_le_bytes());
        message[3..5].copy_from_slice(&INIT_MODEM_DRIVER_MESSAGE_ID.to_le_bytes());
        message[5..7].copy_from_slice(&(payload_len as u16).to_le_bytes());
        message[QMI_HEADER_BYTES..message_len].copy_from_slice(&payload[..payload_len]);
        output[..message_len].copy_from_slice(&message[..message_len]);
        Ok(message_len)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.ipa_shared_memory_end_exclusive == 0 {
            return Err(Error::InvalidMemoryRange);
        }
        validate_route(self.v4_route, self.ipa_shared_memory_end_exclusive)?;
        validate_route(self.v6_route, self.ipa_shared_memory_end_exclusive)?;
        validate_offset(self.v4_filter_start, self.ipa_shared_memory_end_exclusive)?;
        validate_offset(self.v6_filter_start, self.ipa_shared_memory_end_exclusive)?;
        if let Some(range) = self.header_table {
            validate_range(range, self.ipa_shared_memory_end_exclusive)?;
        }
        if let Some(block) = self.modem_memory {
            validate_block(block, self.ipa_shared_memory_end_exclusive)?;
        }
        if self
            .control_endpoint
            .is_some_and(|value| value > MAX_IPA_ENDPOINT)
        {
            return Err(Error::InvalidEndpoint);
        }
        if let Some(range) = self.header_processing_table {
            validate_range(range, self.ipa_shared_memory_end_exclusive)?;
        }
        if let Some(range) = self.compression_table {
            validate_range(range, self.ipa_shared_memory_end_exclusive)?;
        }
        if let Some(hash) = self.hash_tables {
            validate_route(hash.v4_route, self.ipa_shared_memory_end_exclusive)?;
            validate_route(hash.v6_route, self.ipa_shared_memory_end_exclusive)?;
            validate_offset(hash.v4_filter_start, self.ipa_shared_memory_end_exclusive)?;
            validate_offset(hash.v6_filter_start, self.ipa_shared_memory_end_exclusive)?;
        }
        validate_span(self.stats_quota, self.ipa_shared_memory_end_exclusive)?;
        validate_span(self.drop_stats, self.ipa_shared_memory_end_exclusive)?;
        Ok(())
    }
}

/// Decode only the matching `INIT_MODEM_DRIVER` response. A failure result is
/// preserved for the caller; it is never converted into readiness.
pub fn decode_init_response(
    bytes: &[u8],
    expected_transaction: u16,
) -> Result<InitResponse, Error> {
    let message = Message::decode(bytes).map_err(Error::Qmi)?;
    if message.kind != Kind::Response
        || message.transaction != expected_transaction
        || message.message_id != INIT_MODEM_DRIVER_MESSAGE_ID
    {
        return Err(Error::InvalidResponse);
    }
    message.check_fields(&[TLV_RESPONSE]).map_err(Error::Qmi)?;
    let common = message.required(TLV_RESPONSE).map_err(Error::Qmi)?;
    if common.len() != 4 {
        return Err(Error::Qmi(qcom_qmi::Error::InvalidTlvLength(TLV_RESPONSE)));
    }
    let control_endpoint = message.optional_u32(0x10).map_err(Error::Qmi)?;
    let default_endpoint = message.optional_u32(0x11).map_err(Error::Qmi)?;
    if control_endpoint
        .into_iter()
        .chain(default_endpoint)
        .any(|endpoint| endpoint > MAX_IPA_ENDPOINT)
    {
        return Err(Error::InvalidEndpoint);
    }
    let modem_driver_init_pending = match message.get(0x12) {
        None => None,
        Some([0]) => Some(false),
        Some([1]) => Some(true),
        Some(_) => return Err(Error::InvalidValue),
    };
    Ok(InitResponse {
        result: u16::from_le_bytes([common[0], common[1]]),
        error: u16::from_le_bytes([common[2], common[3]]),
        control_endpoint,
        default_endpoint,
        modem_driver_init_pending,
    })
}

fn validate_offset(offset: u32, memory_bytes: u32) -> Result<(), Error> {
    if offset >= memory_bytes {
        Err(Error::InvalidMemoryRange)
    } else {
        Ok(())
    }
}

fn validate_range(range: OffsetRange, memory_bytes: u32) -> Result<(), Error> {
    if range.start > range.end_inclusive || range.end_inclusive >= memory_bytes {
        Err(Error::InvalidMemoryRange)
    } else {
        Ok(())
    }
}

fn validate_block(block: MemoryBlock, memory_bytes: u32) -> Result<(), Error> {
    if block.size == 0
        || block
            .start
            .checked_add(block.size)
            .is_none_or(|end| end > memory_bytes)
    {
        Err(Error::InvalidMemoryRange)
    } else {
        Ok(())
    }
}

fn validate_span(span: MemorySpan, memory_bytes: u32) -> Result<(), Error> {
    validate_offset(span.start, memory_bytes)?;
    if span
        .start
        .checked_add(span.size)
        .is_none_or(|end| end > memory_bytes)
    {
        Err(Error::InvalidMemoryRange)
    } else {
        Ok(())
    }
}

fn validate_route(route: RouteTable, memory_bytes: u32) -> Result<(), Error> {
    validate_offset(route.start, memory_bytes)?;
    if route.num_indices > MAX_ROUTE_INDICES {
        return Err(Error::InvalidRouteCount);
    }
    Ok(())
}

fn append_range(
    payload: &mut [u8; MAX_TLV_BYTES],
    payload_len: &mut usize,
    tag: u8,
    range: OffsetRange,
) -> Result<(), Error> {
    let mut value = [0u8; 8];
    value[..4].copy_from_slice(&range.start.to_le_bytes());
    value[4..].copy_from_slice(&range.end_inclusive.to_le_bytes());
    append_tlv(payload, payload_len, tag, &value)
}

fn append_block(
    payload: &mut [u8; MAX_TLV_BYTES],
    payload_len: &mut usize,
    tag: u8,
    block: MemoryBlock,
) -> Result<(), Error> {
    let mut value = [0u8; 8];
    value[..4].copy_from_slice(&block.start.to_le_bytes());
    value[4..].copy_from_slice(&block.size.to_le_bytes());
    append_tlv(payload, payload_len, tag, &value)
}

fn append_route(
    payload: &mut [u8; MAX_TLV_BYTES],
    payload_len: &mut usize,
    tag: u8,
    route: RouteTable,
) -> Result<(), Error> {
    let mut value = [0u8; 8];
    value[..4].copy_from_slice(&route.start.to_le_bytes());
    value[4..].copy_from_slice(&route.num_indices.to_le_bytes());
    append_tlv(payload, payload_len, tag, &value)
}

fn append_u32(
    payload: &mut [u8; MAX_TLV_BYTES],
    payload_len: &mut usize,
    tag: u8,
    value: u32,
) -> Result<(), Error> {
    append_tlv(payload, payload_len, tag, &value.to_le_bytes())
}

fn append_tlv(
    payload: &mut [u8; MAX_TLV_BYTES],
    payload_len: &mut usize,
    tag: u8,
    value: &[u8],
) -> Result<(), Error> {
    let next_len = payload_len
        .checked_add(3 + value.len())
        .filter(|len| *len <= MAX_TLV_BYTES)
        .ok_or(Error::OutputTooSmall)?;
    payload[*payload_len] = tag;
    payload[*payload_len + 1..*payload_len + 3]
        .copy_from_slice(&(value.len() as u16).to_le_bytes());
    payload[*payload_len + 3..next_len].copy_from_slice(value);
    *payload_len = next_len;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_request() -> InitRequestFields {
        InitRequestFields {
            platform_type: PlatformType::MsmAndroid,
            ipa_shared_memory_end_exclusive: 0x10000,
            header_table: Some(OffsetRange {
                start: 0x100,
                end_inclusive: 0x1ff,
            }),
            v4_route: RouteTable {
                start: 0x200,
                num_indices: 8,
            },
            v6_route: RouteTable {
                start: 0x300,
                num_indices: 12,
            },
            v4_filter_start: 0x400,
            v6_filter_start: 0x500,
            modem_memory: Some(MemoryBlock {
                start: 0x600,
                size: 0x800,
            }),
            control_endpoint: Some(7),
            is_ssr_bootup: Some(false),
            header_processing_table: Some(OffsetRange {
                start: 0x1000,
                end_inclusive: 0x10ff,
            }),
            compression_table: Some(OffsetRange {
                start: 0x1100,
                end_inclusive: 0x11ff,
            }),
            hash_tables: Some(HashTables {
                v4_route: RouteTable {
                    start: 0x1200,
                    num_indices: 8,
                },
                v6_route: RouteTable {
                    start: 0x1300,
                    num_indices: 12,
                },
                v4_filter_start: 0x1400,
                v6_filter_start: 0x1500,
            }),
            stats_quota: MemorySpan {
                start: 0x1600,
                size: 0x100,
            },
            drop_stats: MemorySpan {
                start: 0x1700,
                size: 0,
            },
        }
    }

    fn append_response_tlv(out: &mut Vec<u8>, tag: u8, value: &[u8]) {
        out.push(tag);
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
        out.extend_from_slice(value);
    }

    fn response(transaction: u16, message_id: u16, tlvs: &[(u8, &[u8])]) -> Vec<u8> {
        let mut payload = Vec::new();
        for (tag, value) in tlvs {
            append_response_tlv(&mut payload, *tag, value);
        }
        let mut out = vec![2];
        out.extend_from_slice(&transaction.to_le_bytes());
        out.extend_from_slice(&message_id.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn request_tlv_count(bytes: &[u8]) -> usize {
        assert!(bytes.len() >= QMI_HEADER_BYTES);
        assert_eq!(bytes[0], 0);
        let payload_len = u16::from_le_bytes([bytes[5], bytes[6]]) as usize;
        assert_eq!(payload_len, bytes.len() - QMI_HEADER_BYTES);
        let mut cursor = QMI_HEADER_BYTES;
        let mut count = 0;
        while cursor < bytes.len() {
            assert!(bytes.len() - cursor >= 3);
            let len = u16::from_le_bytes([bytes[cursor + 1], bytes[cursor + 2]]) as usize;
            cursor = cursor.checked_add(3 + len).unwrap();
            assert!(cursor <= bytes.len());
            count += 1;
        }
        count
    }

    fn request_tlv(bytes: &[u8], wanted: u8) -> Option<&[u8]> {
        let mut cursor = QMI_HEADER_BYTES;
        while cursor < bytes.len() {
            let tag = bytes[cursor];
            let len = u16::from_le_bytes([bytes[cursor + 1], bytes[cursor + 2]]) as usize;
            let start = cursor + 3;
            let end = start + len;
            assert!(end <= bytes.len());
            if tag == wanted {
                return Some(&bytes[start..end]);
            }
            cursor = end;
        }
        None
    }

    fn request_tlv_tags(bytes: &[u8]) -> Vec<u8> {
        let mut cursor = QMI_HEADER_BYTES;
        let mut tags = Vec::new();
        while cursor < bytes.len() {
            let tag = bytes[cursor];
            let len = u16::from_le_bytes([bytes[cursor + 1], bytes[cursor + 2]]) as usize;
            tags.push(tag);
            cursor += 3 + len;
        }
        tags
    }

    #[test]
    fn service_and_message_ids_match_the_pinned_sunfish_ipa_v3_source() {
        assert_eq!((SERVICE_ID, SERVICE_VERSION, MODEM_INSTANCE), (0x31, 1, 2));
        assert_eq!(INIT_MODEM_DRIVER_MESSAGE_ID, 0x0021);
        assert_eq!(MAX_TLV_BYTES, 162);
    }

    #[test]
    fn encodes_the_full_source_schema_with_exact_maximum_size() {
        let mut output = [0xa5; MAX_QMI_BYTES];
        let len = full_request().encode(0x1234, &mut output).unwrap();
        assert_eq!(len, MAX_QMI_BYTES);
        assert_eq!(output[0], 0);
        assert_eq!(u16::from_le_bytes([output[1], output[2]]), 0x1234);
        assert_eq!(
            u16::from_le_bytes([output[3], output[4]]),
            INIT_MODEM_DRIVER_MESSAGE_ID
        );
        assert_eq!(request_tlv_count(&output[..len]), 19);
        assert_eq!(
            request_tlv_tags(&output[..len]),
            (TLV_PLATFORM_TYPE..=TLV_DROP_STATS_SIZE).collect::<Vec<_>>()
        );
        assert_eq!(
            request_tlv(&output[..len], TLV_PLATFORM_TYPE),
            Some(&3i32.to_le_bytes()[..])
        );
        assert_eq!(
            request_tlv(&output[..len], TLV_HEADER_TABLE),
            Some(&[0x00, 0x01, 0, 0, 0xff, 0x01, 0, 0][..])
        );
        assert_eq!(
            request_tlv(&output[..len], TLV_V4_ROUTE),
            Some(&[0x00, 0x02, 0, 0, 8, 0, 0, 0][..])
        );
        assert_eq!(
            request_tlv(&output[..len], TLV_MODEM_MEMORY),
            Some(&[0x00, 0x06, 0, 0, 0x00, 0x08, 0, 0][..])
        );
        assert_eq!(request_tlv(&output[..len], TLV_SSR_BOOT), Some(&[0][..]));
        assert_eq!(
            request_tlv(&output[..len], TLV_DROP_STATS_SIZE),
            Some(&0u32.to_le_bytes()[..])
        );
    }

    #[test]
    fn accepts_zero_sized_stats_span_only_with_an_in_range_base() {
        let mut fields = full_request();
        fields.stats_quota.size = 0;
        fields.drop_stats.size = 0;
        let mut output = [0u8; MAX_QMI_BYTES];
        let len = fields.encode(1, &mut output).unwrap();
        assert_eq!(
            request_tlv(&output[..len], TLV_STATS_QUOTA_SIZE),
            Some(&0u32.to_le_bytes()[..])
        );
        assert_eq!(
            request_tlv(&output[..len], TLV_DROP_STATS_SIZE),
            Some(&0u32.to_le_bytes()[..])
        );

        fields.drop_stats.start = fields.ipa_shared_memory_end_exclusive;
        assert_eq!(
            fields.encode(1, &mut output),
            Err(Error::InvalidMemoryRange)
        );
    }

    #[test]
    fn omits_only_source_optional_fields_that_are_unavailable() {
        let mut fields = full_request();
        fields.header_table = None;
        fields.modem_memory = None;
        fields.control_endpoint = None;
        fields.is_ssr_bootup = None;
        fields.header_processing_table = None;
        fields.compression_table = None;
        fields.hash_tables = None;
        let mut output = [0u8; MAX_QMI_BYTES];
        let len = fields.encode(1, &mut output).unwrap();
        assert_eq!(request_tlv_count(&output[..len]), 9);
        for tag in [
            TLV_HEADER_TABLE,
            TLV_MODEM_MEMORY,
            TLV_CONTROL_ENDPOINT,
            TLV_SSR_BOOT,
            TLV_HEADER_PROCESSING,
            TLV_COMPRESSION,
            TLV_V4_HASH_ROUTE,
            TLV_V6_HASH_ROUTE,
            TLV_V4_HASH_FILTER,
            TLV_V6_HASH_FILTER,
        ] {
            assert_eq!(
                request_tlv(&output[..len], tag),
                None,
                "unexpected optional TLV {tag:#x}"
            );
        }
        assert_eq!(
            request_tlv(&output[..len], TLV_PLATFORM_TYPE),
            Some(&3i32.to_le_bytes()[..])
        );
        assert_eq!(request_tlv(&output[..len], TLV_V4_ROUTE).unwrap().len(), 8);
        assert_eq!(
            request_tlv(&output[..len], TLV_STATS_QUOTA_BASE)
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn rejects_each_untrusted_platform_range_before_output_mutation() {
        let cases = [
            {
                let mut value = full_request();
                value.ipa_shared_memory_end_exclusive = 0;
                value
            },
            {
                let mut value = full_request();
                value.header_table = Some(OffsetRange {
                    start: 0x200,
                    end_inclusive: 0x10000,
                });
                value
            },
            {
                let mut value = full_request();
                value.modem_memory = Some(MemoryBlock {
                    start: 0xffff,
                    size: 2,
                });
                value
            },
            {
                let mut value = full_request();
                value.v4_route.num_indices = 32;
                value
            },
            {
                let mut value = full_request();
                value.control_endpoint = Some(20);
                value
            },
            {
                let mut value = full_request();
                value.stats_quota = MemorySpan {
                    start: 0x10000,
                    size: 0,
                };
                value
            },
        ];
        for fields in cases {
            let mut output = [0xa5; MAX_QMI_BYTES];
            let before = output;
            assert!(fields.encode(1, &mut output).is_err());
            assert_eq!(output, before);
        }
    }

    #[test]
    fn output_capacity_is_checked_before_mutation() {
        let mut output = [0x5a; MAX_QMI_BYTES - 1];
        let before = output;
        assert_eq!(
            full_request().encode(1, &mut output),
            Err(Error::OutputTooSmall)
        );
        assert_eq!(output, before);
    }

    #[test]
    fn decodes_matching_success_or_failure_without_promoting_failure() {
        let bytes = response(
            9,
            INIT_MODEM_DRIVER_MESSAGE_ID,
            &[
                (TLV_RESPONSE, &[0, 0, 0, 0]),
                (0x10, &7u32.to_le_bytes()),
                (0x11, &8u32.to_le_bytes()),
                (0x12, &[1]),
            ],
        );
        assert_eq!(
            decode_init_response(&bytes, 9),
            Ok(InitResponse {
                result: 0,
                error: 0,
                control_endpoint: Some(7),
                default_endpoint: Some(8),
                modem_driver_init_pending: Some(true),
            })
        );
        let failed = response(
            9,
            INIT_MODEM_DRIVER_MESSAGE_ID,
            &[(TLV_RESPONSE, &[1, 0, 3, 0])],
        );
        assert_eq!(
            decode_init_response(&failed, 9),
            Ok(InitResponse {
                result: 1,
                error: 3,
                control_endpoint: None,
                default_endpoint: None,
                modem_driver_init_pending: None,
            })
        );
    }

    #[test]
    fn rejects_wrong_response_identity_malformed_result_and_invalid_optional_values() {
        assert_eq!(
            decode_init_response(
                &response(
                    8,
                    INIT_MODEM_DRIVER_MESSAGE_ID,
                    &[(TLV_RESPONSE, &[0, 0, 0, 0])]
                ),
                9,
            ),
            Err(Error::InvalidResponse)
        );
        assert_eq!(
            decode_init_response(&response(9, 0x0022, &[(TLV_RESPONSE, &[0, 0, 0, 0])]), 9),
            Err(Error::InvalidResponse)
        );
        assert!(decode_init_response(&response(9, INIT_MODEM_DRIVER_MESSAGE_ID, &[]), 9).is_err());
        assert_eq!(
            decode_init_response(
                &response(
                    9,
                    INIT_MODEM_DRIVER_MESSAGE_ID,
                    &[(TLV_RESPONSE, &[0, 0, 0, 0]), (0x12, &[2])]
                ),
                9,
            ),
            Err(Error::InvalidValue)
        );
        assert_eq!(
            decode_init_response(
                &response(
                    9,
                    INIT_MODEM_DRIVER_MESSAGE_ID,
                    &[(TLV_RESPONSE, &[0, 0, 0, 0]), (0x10, &20u32.to_le_bytes())]
                ),
                9,
            ),
            Err(Error::InvalidEndpoint)
        );
    }
}
