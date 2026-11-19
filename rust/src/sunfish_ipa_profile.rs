//! Source-derived Sunfish IPA v4.2 `INIT_MODEM_DRIVER` request profile.
//!
//! Static partition offsets come from the matched LineageOS 4.14
//! `ipa_4_2_mem_part` and QMI builder at commit
//! `28f9290ae067d3e7857126eb6312720dbe9c7c80`; the platform enum and endpoint
//! mapping come from that commit's `rmnet_ipa.c`, `ipa_qmi_service_v01.h`, and
//! `ipa_utils.c`. Register-derived SRAM geometry, the allocated APPS_WAN_CONS
//! endpoint, and uC-loaded state remain explicit runtime inputs. This module
//! builds a request description only; it does not read IPA registers, start
//! the AP-side IPA service, attach transport, or send QMI.
#![forbid(unsafe_code)]

use crate::qcom_ipa::{
    Error, InitRequestFields, MemoryBlock, MemorySpan, OffsetRange, PlatformType, RouteTable,
};

/// `IPA_MEM_PART(end_ofst)` in the IPA v4.2 memory partition.
pub const PARTITION_END_BYTES: u32 = 0x2000;
/// The IPA v4.2 endpoint table assigns APPS_WAN_CONS endpoint 9.
pub const APPS_WAN_CONS_ENDPOINT: u32 = 9;

const V4_ROUTE_OFFSET: u32 = 0x3a0;
const V6_ROUTE_OFFSET: u32 = 0x428;
const V4_FILTER_OFFSET: u32 = 0x290;
const V6_FILTER_OFFSET: u32 = 0x318;
const HEADER_OFFSET: u32 = 0x4a8;
const HEADER_SIZE: u32 = 0x140;
const HEADER_PROCESSING_OFFSET: u32 = 0x5f0;
const HEADER_PROCESSING_SIZE: u32 = 0x200;
const MODEM_MEMORY_OFFSET: u32 = 0xbf0;
const MODEM_MEMORY_SIZE: u32 = 0x140c;
const STATS_QUOTA_OFFSET: u32 = 0xa50;
const STATS_QUOTA_SIZE: u32 = 0x60;
const STATS_DROP_OFFSET: u32 = 0xbf0;
const STATS_DROP_SIZE: u32 = 0;
const MAX_REGISTER_UNITS: u32 = u16::MAX as u32;

/// Values decoded from `IPA_SHARED_MEM_SIZE` and the initialized IPA endpoint
/// map. The two register fields are in 8-byte units, as in the Linux driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeSnapshot {
    pub shared_mem_baddr_units: u32,
    pub shared_mem_size_units: u32,
    /// `None` means Linux reports `IPA_EP_NOT_ALLOCATED` and omits the TLV.
    pub apps_wan_cons_endpoint: Option<u32>,
    /// The source omits the SSR TLV on first boot and sends `true` after uC load.
    pub ipa_uc_loaded: bool,
}

/// Build the exact Sunfish IPA v4.2 request fields from live, caller-supplied
/// runtime state. All QMI SRAM offsets are absolute IPA-relative offsets, so
/// the validator limit is the exclusive end `baddr + available_size`.
pub fn init_request_fields(snapshot: RuntimeSnapshot) -> Result<InitRequestFields, Error> {
    if snapshot.shared_mem_baddr_units > MAX_REGISTER_UNITS
        || snapshot.shared_mem_size_units > MAX_REGISTER_UNITS
    {
        return Err(Error::InvalidMemoryRange);
    }
    let shared_mem_baddr_bytes = snapshot
        .shared_mem_baddr_units
        .checked_mul(8)
        .ok_or(Error::InvalidMemoryRange)?;
    let shared_mem_size_bytes = snapshot
        .shared_mem_size_units
        .checked_mul(8)
        .ok_or(Error::InvalidMemoryRange)?;
    if shared_mem_size_bytes < PARTITION_END_BYTES {
        return Err(Error::InvalidMemoryRange);
    }
    let shared_mem_end_exclusive = shared_mem_baddr_bytes
        .checked_add(shared_mem_size_bytes)
        .ok_or(Error::InvalidMemoryRange)?;

    if snapshot
        .apps_wan_cons_endpoint
        .is_some_and(|endpoint| endpoint != APPS_WAN_CONS_ENDPOINT)
    {
        return Err(Error::InvalidEndpoint);
    }

    let offset = |relative: u32| -> Result<u32, Error> {
        shared_mem_baddr_bytes
            .checked_add(relative)
            .ok_or(Error::InvalidMemoryRange)
    };
    let range = |relative: u32, size: u32| -> Result<OffsetRange, Error> {
        let start = offset(relative)?;
        let end_inclusive = start
            .checked_add(size)
            .and_then(|end| end.checked_sub(1))
            .ok_or(Error::InvalidMemoryRange)?;
        Ok(OffsetRange {
            start,
            end_inclusive,
        })
    };

    Ok(InitRequestFields {
        platform_type: PlatformType::MsmAndroid,
        ipa_shared_memory_end_exclusive: shared_mem_end_exclusive,
        header_table: Some(range(HEADER_OFFSET, HEADER_SIZE)?),
        v4_route: RouteTable {
            start: offset(V4_ROUTE_OFFSET)?,
            num_indices: 7,
        },
        v6_route: RouteTable {
            start: offset(V6_ROUTE_OFFSET)?,
            num_indices: 7,
        },
        v4_filter_start: offset(V4_FILTER_OFFSET)?,
        v6_filter_start: offset(V6_FILTER_OFFSET)?,
        modem_memory: Some(MemoryBlock {
            start: offset(MODEM_MEMORY_OFFSET)?,
            size: MODEM_MEMORY_SIZE,
        }),
        control_endpoint: snapshot.apps_wan_cons_endpoint,
        is_ssr_bootup: snapshot.ipa_uc_loaded.then_some(true),
        header_processing_table: Some(range(HEADER_PROCESSING_OFFSET, HEADER_PROCESSING_SIZE)?),
        compression_table: None,
        hash_tables: None,
        stats_quota: MemorySpan {
            start: offset(STATS_QUOTA_OFFSET)?,
            size: STATS_QUOTA_SIZE,
        },
        drop_stats: MemorySpan {
            start: offset(STATS_DROP_OFFSET)?,
            size: STATS_DROP_SIZE,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_ipa;

    fn snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot {
            shared_mem_baddr_units: 0x10,
            shared_mem_size_units: 0x400,
            apps_wan_cons_endpoint: Some(APPS_WAN_CONS_ENDPOINT),
            ipa_uc_loaded: false,
        }
    }

    fn find_tlv<'a>(mut bytes: &'a [u8], tag: u8) -> Option<&'a [u8]> {
        if bytes.len() < qcom_ipa::QMI_HEADER_BYTES {
            return None;
        }
        bytes = &bytes[qcom_ipa::QMI_HEADER_BYTES..];
        while bytes.len() >= 3 {
            let len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
            if bytes.len() < 3 + len {
                return None;
            }
            if bytes[0] == tag {
                return Some(&bytes[3..3 + len]);
            }
            bytes = &bytes[3 + len..];
        }
        None
    }

    #[test]
    fn builds_the_source_derived_sunfish_v42_profile() {
        let fields = init_request_fields(snapshot()).unwrap();
        assert_eq!(fields.platform_type, PlatformType::MsmAndroid);
        assert_eq!(fields.ipa_shared_memory_end_exclusive, 0x2080);
        assert_eq!(
            fields.header_table,
            Some(OffsetRange {
                start: 0x528,
                end_inclusive: 0x667,
            })
        );
        assert_eq!(
            fields.v4_route,
            RouteTable {
                start: 0x420,
                num_indices: 7,
            }
        );
        assert_eq!(
            fields.v6_route,
            RouteTable {
                start: 0x4a8,
                num_indices: 7,
            }
        );
        assert_eq!(fields.v4_filter_start, 0x310);
        assert_eq!(fields.v6_filter_start, 0x398);
        assert_eq!(
            fields.modem_memory,
            Some(MemoryBlock {
                start: 0xc70,
                size: 0x140c,
            })
        );
        assert_eq!(fields.control_endpoint, Some(9));
        assert_eq!(fields.is_ssr_bootup, None);
        assert_eq!(
            fields.header_processing_table,
            Some(OffsetRange {
                start: 0x670,
                end_inclusive: 0x86f,
            })
        );
        assert_eq!(fields.compression_table, None);
        assert_eq!(fields.hash_tables, None);
        assert_eq!(
            fields.stats_quota,
            MemorySpan {
                start: 0xad0,
                size: 0x60,
            }
        );
        assert_eq!(
            fields.drop_stats,
            MemorySpan {
                start: 0xc70,
                size: 0,
            }
        );
    }

    #[test]
    fn encodes_the_sunfish_request_without_hash_or_compression_tlvs() {
        let fields = init_request_fields(snapshot()).unwrap();
        let mut output = [0xa5; qcom_ipa::MAX_QMI_BYTES];
        let len = fields.encode(0x1234, &mut output).unwrap();
        assert_eq!(len, 118);
        assert_eq!(
            &output[..qcom_ipa::QMI_HEADER_BYTES],
            &[0, 0x34, 0x12, 0x21, 0, 0x6f, 0]
        );
        let expected_tags = [
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x1f, 0x20, 0x21, 0x22,
        ];
        let mut cursor = qcom_ipa::QMI_HEADER_BYTES;
        for tag in expected_tags {
            assert_eq!(output[cursor], tag);
            let value_len = u16::from_le_bytes([output[cursor + 1], output[cursor + 2]]) as usize;
            cursor += 3 + value_len;
        }
        assert_eq!(cursor, len);
        for tag in [0x18, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e] {
            assert_eq!(find_tlv(&output[..len], tag), None);
        }
        assert_eq!(
            find_tlv(&output[..len], 0x21),
            Some(&0xc70u32.to_le_bytes()[..])
        );
        assert_eq!(
            find_tlv(&output[..len], 0x22),
            Some(&0u32.to_le_bytes()[..])
        );
    }

    #[test]
    fn ssr_state_and_endpoint_presence_follow_runtime_snapshot() {
        let mut runtime = snapshot();
        runtime.ipa_uc_loaded = true;
        runtime.apps_wan_cons_endpoint = None;
        let fields = init_request_fields(runtime).unwrap();
        assert_eq!(fields.is_ssr_bootup, Some(true));
        assert_eq!(fields.control_endpoint, None);
        let mut output = [0; qcom_ipa::MAX_QMI_BYTES];
        let len = fields.encode(1, &mut output).unwrap();
        assert_eq!(find_tlv(&output[..len], 0x17), None);
        assert_eq!(find_tlv(&output[..len], 0x18), Some(&[1][..]));
    }

    #[test]
    fn rejects_invalid_runtime_geometry_and_endpoint_mappings() {
        for runtime in [
            RuntimeSnapshot {
                shared_mem_size_units: 0,
                ..snapshot()
            },
            RuntimeSnapshot {
                shared_mem_size_units: 0x3ff,
                ..snapshot()
            },
            RuntimeSnapshot {
                shared_mem_baddr_units: u32::MAX,
                ..snapshot()
            },
            RuntimeSnapshot {
                shared_mem_baddr_units: u32::MAX / 8,
                shared_mem_size_units: 0x400,
                ..snapshot()
            },
            RuntimeSnapshot {
                apps_wan_cons_endpoint: Some(8),
                ..snapshot()
            },
        ] {
            assert!(init_request_fields(runtime).is_err());
        }
    }
}
