//! Bounded QMI service-locator replies for the source-derived Sunfish map.
//!
//! The endpoint serves only the manifest-backed `GET_DOMAIN_LIST` operation.
//! It has no transport, domain-start, power, DMA, or ownership side effects.
#![forbid(unsafe_code)]

use crate::qcom_qmi::{self as qmi, Kind, Message};

pub const SERVICE_ID: u32 = 0x40;
pub const SERVICE_VERSION: u8 = 1;
pub const SERVICE_INSTANCE: u32 = 1;
// Keep this distinct from the remote-storage (0x4000), heap-memory (0x4001),
// and SERVREG_NOTIF client (0x4200) ports in the native modem owner.
pub const LOCAL_PORT: u32 = 0x4003;
pub const GET_DOMAIN_LIST: u16 = 0x0021;
pub const DB_REVISION: u16 = 1;
pub const MAX_NAME_BYTES: usize = 64;
#[cfg(feature = "sunfish-servreg-sm7150-topology")]
pub const MAX_DOMAIN_ROWS: usize = 6;
#[cfg(not(feature = "sunfish-servreg-sm7150-topology"))]
pub const MAX_DOMAIN_ROWS: usize = 5;

/// Stable diagnostic identifiers for the bounded Sunfish service names.
pub const NAME_TMS_SERVREG: u32 = 1;
pub const NAME_GPS_SERVICE: u32 = 2;
pub const NAME_ELF_LOADER: u32 = 3;
pub const NAME_WLAN_FW: u32 = 4;
pub const NAME_TMS_PDR_ENABLED: u32 = 5;
pub const NAME_TMS_PDDUMP_DISABLED: u32 = 6;
pub const NAME_UNKNOWN: u32 = u32::MAX;

/// Stable indexes for the fixed-size per-name counters in the owner trace.
pub const NAME_COUNTER_TMS_SERVREG: usize = 0;
pub const NAME_COUNTER_GPS_SERVICE: usize = 1;
pub const NAME_COUNTER_ELF_LOADER: usize = 2;
pub const NAME_COUNTER_WLAN_FW: usize = 3;
pub const NAME_COUNTER_TMS_PDR_ENABLED: usize = 4;
pub const NAME_COUNTER_TMS_PDDUMP_DISABLED: usize = 5;
pub const NAME_COUNTER_OTHER: usize = 6;
pub const NAME_COUNTER_COUNT: usize = 7;

pub const fn name_counter_index(name_code: u32) -> usize {
    match name_code {
        NAME_TMS_SERVREG => NAME_COUNTER_TMS_SERVREG,
        NAME_GPS_SERVICE => NAME_COUNTER_GPS_SERVICE,
        NAME_ELF_LOADER => NAME_COUNTER_ELF_LOADER,
        NAME_WLAN_FW => NAME_COUNTER_WLAN_FW,
        NAME_TMS_PDR_ENABLED => NAME_COUNTER_TMS_PDR_ENABLED,
        NAME_TMS_PDDUMP_DISABLED => NAME_COUNTER_TMS_PDDUMP_DISABLED,
        _ => NAME_COUNTER_OTHER,
    }
}

pub const fn service_name_code(name: &[u8]) -> u32 {
    match name {
        b"tms/servreg" => NAME_TMS_SERVREG,
        b"gps/gps_service" => NAME_GPS_SERVICE,
        b"kernel/elf_loader" => NAME_ELF_LOADER,
        b"wlan/fw" => NAME_WLAN_FW,
        b"tms/pdr_enabled" => NAME_TMS_PDR_ENABLED,
        b"tms/pddump_disabled" => NAME_TMS_PDDUMP_DISABLED,
        _ => NAME_UNKNOWN,
    }
}

// The Linux QMI encoder uses a u8 length for nested strings whose
// descriptor maximum fits in u8. SERVREG's maximum is 65 bytes.
#[cfg(feature = "sunfish-servreg-linux-string-length")]
const DOMAIN_LENGTH_BYTES: usize = 1;
#[cfg(not(feature = "sunfish-servreg-linux-string-length"))]
const DOMAIN_LENGTH_BYTES: usize = 2;
const MAX_ENTRY_BYTES: usize = DOMAIN_LENGTH_BYTES + MAX_NAME_BYTES + 4 + 1 + 4;
const MAX_OPTIONAL_TLV_BYTES: usize = 5 + 5 + 3 + 1 + MAX_DOMAIN_ROWS * MAX_ENTRY_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Query<'a> {
    pub service_name: &'a [u8],
    pub domain_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Mapping {
    service: &'static [u8],
    domain: &'static [u8],
    instance: u32,
}

// The modem entries come from the pinned modemr.jsn/modemuw.jsn manifests.
// The SM7150 locator also lists the ADSP and CDSP domains under tms/servreg,
// in the order of sm7150_domains in the reference Linux qcom_pd_mapper.c.
const MAPPINGS: &[Mapping] = &[
    #[cfg(feature = "sunfish-servreg-sm7150-topology")]
    Mapping {
        service: b"tms/servreg",
        domain: b"msm/adsp/audio_pd",
        instance: 74,
    },
    #[cfg(feature = "sunfish-servreg-sm7150-topology")]
    Mapping {
        service: b"tms/servreg",
        domain: b"msm/adsp/root_pd",
        instance: 74,
    },
    #[cfg(feature = "sunfish-servreg-sm7150-topology")]
    Mapping {
        service: b"tms/servreg",
        domain: b"msm/adsp/sensor_pd",
        instance: 74,
    },
    #[cfg(feature = "sunfish-servreg-sm7150-topology")]
    Mapping {
        service: b"tms/servreg",
        domain: b"msm/cdsp/root_pd",
        instance: 76,
    },
    Mapping {
        service: b"tms/servreg",
        domain: b"msm/modem/root_pd",
        instance: 180,
    },
    Mapping {
        service: b"tms/servreg",
        domain: b"msm/modem/wlan_pd",
        instance: 180,
    },
    #[cfg(feature = "sunfish-servreg-avs-audio")]
    Mapping {
        service: b"avs/audio",
        domain: b"msm/adsp/audio_pd",
        instance: 74,
    },
    Mapping {
        service: b"gps/gps_service",
        domain: b"msm/modem/root_pd",
        instance: 180,
    },
    Mapping {
        service: b"kernel/elf_loader",
        domain: b"msm/modem/wlan_pd",
        instance: 180,
    },
    Mapping {
        service: b"wlan/fw",
        domain: b"msm/modem/wlan_pd",
        instance: 180,
    },
];

/// Decode a QMI SERVREG LOC GET_DOMAIN_LIST request. The offset is optional in
/// the schema; malformed strings and unsupported mandatory TLVs are rejected.
pub fn decode<'a>(message: &Message<'a>) -> Result<Query<'a>, qmi::Error> {
    if message.kind != Kind::Request || message.message_id != GET_DOMAIN_LIST {
        return Err(qmi::Error::UnsupportedMessage);
    }
    message.check_fields(&[0x01, 0x10])?;
    let service_name = message.required(0x01)?;
    if service_name.is_empty()
        || service_name.len() > MAX_NAME_BYTES
        || !service_name.iter().all(u8::is_ascii_graphic)
    {
        return Err(qmi::Error::InvalidValue);
    }
    Ok(Query {
        service_name,
        domain_offset: message.optional_u32(0x10)?.unwrap_or(0),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Response {
    bytes: [u8; MAX_OPTIONAL_TLV_BYTES],
    len: usize,
}

impl Response {
    fn new() -> Self {
        Self {
            bytes: [0; MAX_OPTIONAL_TLV_BYTES],
            len: 0,
        }
    }

    fn push_tlv(&mut self, kind: u8, value: &[u8]) {
        let end = self.len + 3 + value.len();
        debug_assert!(end <= self.bytes.len());
        self.bytes[self.len] = kind;
        self.bytes[self.len + 1..self.len + 3].copy_from_slice(&(value.len() as u16).to_le_bytes());
        self.bytes[self.len + 3..end].copy_from_slice(value);
        self.len = end;
    }

    pub fn optional_tlvs(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn total_domains(&self) -> u16 {
        self.find_tlv(0x10)
            .filter(|value| value.len() == 2)
            .map(|value| u16::from_le_bytes([value[0], value[1]]))
            .unwrap_or(0)
    }

    pub fn domain_list_tlv(&self) -> Option<&[u8]> {
        self.find_tlv(0x12)
    }

    fn find_tlv(&self, wanted: u8) -> Option<&[u8]> {
        let mut at = 0;
        while at + 3 <= self.len {
            let kind = self.bytes[at];
            let size = u16::from_le_bytes([self.bytes[at + 1], self.bytes[at + 2]]) as usize;
            let end = at.checked_add(3 + size)?;
            if end > self.len {
                return None;
            }
            if kind == wanted {
                return Some(&self.bytes[at + 3..end]);
            }
            at = end;
        }
        None
    }
}

/// Build success, total-count and revision TLVs, plus one bounded page of
/// manifest-backed domain rows. Empty and unknown maps omit the optional list.
pub fn respond(query: Query<'_>) -> Response {
    let total = MAPPINGS
        .iter()
        .filter(|mapping| mapping.service == query.service_name)
        .count();
    let mut response = Response::new();

    response.push_tlv(0x10, &(total as u16).to_le_bytes());
    response.push_tlv(0x11, &DB_REVISION.to_le_bytes());

    if query.domain_offset >= total as u32 {
        return response;
    }

    let mut list = [0u8; 1 + MAX_DOMAIN_ROWS * MAX_ENTRY_BYTES];
    let mut list_len = 1;
    let mut count = 0u8;
    for mapping in MAPPINGS
        .iter()
        .filter(|mapping| mapping.service == query.service_name)
        .skip(query.domain_offset as usize)
    {
        let name_len = mapping.domain.len();
        let name_start = list_len + DOMAIN_LENGTH_BYTES;
        let end = name_start + name_len + 4 + 1 + 4;
        debug_assert!(name_len <= MAX_NAME_BYTES && end <= list.len());
        let length = (name_len as u16).to_le_bytes();
        list[list_len..name_start].copy_from_slice(&length[..DOMAIN_LENGTH_BYTES]);
        list[name_start..name_start + name_len].copy_from_slice(mapping.domain);
        let mut at = name_start + name_len;
        list[at..at + 4].copy_from_slice(&mapping.instance.to_le_bytes());
        at += 4;
        list[at] = 0; // service_data_valid: the manifests publish no service data.
        at += 1;
        list[at..at + 4].fill(0);
        list_len = end;
        count += 1;
    }
    list[0] = count;
    response.push_tlv(0x12, &list[..list_len]);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_codes_map_to_stable_counter_indexes() {
        assert_eq!(
            name_counter_index(NAME_TMS_SERVREG),
            NAME_COUNTER_TMS_SERVREG
        );
        assert_eq!(
            name_counter_index(NAME_GPS_SERVICE),
            NAME_COUNTER_GPS_SERVICE
        );
        assert_eq!(name_counter_index(NAME_ELF_LOADER), NAME_COUNTER_ELF_LOADER);
        assert_eq!(name_counter_index(NAME_WLAN_FW), NAME_COUNTER_WLAN_FW);
        assert_eq!(
            name_counter_index(NAME_TMS_PDR_ENABLED),
            NAME_COUNTER_TMS_PDR_ENABLED
        );
        assert_eq!(
            name_counter_index(NAME_TMS_PDDUMP_DISABLED),
            NAME_COUNTER_TMS_PDDUMP_DISABLED
        );
        assert_eq!(name_counter_index(NAME_UNKNOWN), NAME_COUNTER_OTHER);
        assert_eq!(name_counter_index(0), NAME_COUNTER_OTHER);
    }

    fn message(service: &[u8], offset: Option<u32>) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.push(1);
        payload.extend_from_slice(&(service.len() as u16).to_le_bytes());
        payload.extend_from_slice(service);
        if let Some(offset) = offset {
            payload.push(0x10);
            payload.extend_from_slice(&4u16.to_le_bytes());
            payload.extend_from_slice(&offset.to_le_bytes());
        }
        let mut bytes = vec![0, 7, 0, 0x21, 0, 0, 0];
        bytes[5..7].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    #[test]
    fn source_manifest_maps_and_instance_ids_are_returned() {
        let bytes = message(b"tms/servreg", None);
        let request = qmi::Message::decode(&bytes).unwrap();
        let query = decode(&request).unwrap();
        let response = respond(query);
        assert_eq!(response.optional_tlvs()[0], 0x10);
        let domains: &[(&[u8], u32)] = &[
            #[cfg(feature = "sunfish-servreg-sm7150-topology")]
            (b"msm/adsp/audio_pd", 74),
            #[cfg(feature = "sunfish-servreg-sm7150-topology")]
            (b"msm/adsp/root_pd", 74),
            #[cfg(feature = "sunfish-servreg-sm7150-topology")]
            (b"msm/adsp/sensor_pd", 74),
            #[cfg(feature = "sunfish-servreg-sm7150-topology")]
            (b"msm/cdsp/root_pd", 76),
            (b"msm/modem/root_pd", 180),
            (b"msm/modem/wlan_pd", 180),
        ];
        let mut expected = vec![domains.len() as u8];
        for &(domain, instance) in domains {
            expected.extend_from_slice(&(domain.len() as u16).to_le_bytes()[..DOMAIN_LENGTH_BYTES]);
            expected.extend_from_slice(domain);
            expected.extend_from_slice(&instance.to_le_bytes());
            expected.extend_from_slice(&[0; 5]);
        }
        assert_eq!(response.total_domains(), domains.len() as u16);
        assert_eq!(response.domain_list_tlv(), Some(expected.as_slice()));
    }

    #[test]
    fn offset_pages_are_bounded_and_keep_total_count() {
        let total = if cfg!(feature = "sunfish-servreg-sm7150-topology") { 6 } else { 2 };
        let bytes = message(b"tms/servreg", Some(total - 1));
        let request = qmi::Message::decode(&bytes).unwrap();
        let response = respond(decode(&request).unwrap());
        let list = response.domain_list_tlv().unwrap();
        assert_eq!(response.total_domains(), total as u16);
        assert_eq!(list[0], 1);
        assert_eq!(
            &list[1 + DOMAIN_LENGTH_BYTES..1 + DOMAIN_LENGTH_BYTES + b"msm/modem/wlan_pd".len()],
            b"msm/modem/wlan_pd"
        );

        let bytes = message(b"tms/servreg", Some(u32::MAX));
        let request = qmi::Message::decode(&bytes).unwrap();
        let response = respond(decode(&request).unwrap());
        assert_eq!(response.total_domains(), total as u16);
        assert_eq!(response.domain_list_tlv(), None);
    }

    #[test]
    fn all_captured_service_maps_and_empty_maps_are_exact() {
        for (service, expected) in [
            (
                b"gps/gps_service".as_slice(),
                b"msm/modem/root_pd".as_slice(),
            ),
            (
                b"kernel/elf_loader".as_slice(),
                b"msm/modem/wlan_pd".as_slice(),
            ),
            (b"wlan/fw".as_slice(), b"msm/modem/wlan_pd".as_slice()),
        ] {
            let bytes = message(service, None);
            let request = qmi::Message::decode(&bytes).unwrap();
            let response = respond(decode(&request).unwrap());
            assert_eq!(response.total_domains(), 1);
            assert!(
                response
                    .domain_list_tlv()
                    .unwrap()
                    .windows(expected.len())
                    .any(|window| window == expected)
            );
        }
        for service in [
            b"tms/pdr_enabled".as_slice(),
            b"unmapped/service".as_slice(),
            b"AVS/AUDIO".as_slice(),
        ] {
            let bytes = message(service, None);
            let request = qmi::Message::decode(&bytes).unwrap();
            let response = respond(decode(&request).unwrap());
            assert_eq!(response.total_domains(), 0);
            assert_eq!(response.domain_list_tlv(), None);
        }
    }

    #[test]
    fn avs_audio_reply_matches_the_pinned_sm7150_locator() {
        let bytes = message(b"avs/audio", None);
        let request = qmi::Message::decode(&bytes).unwrap();
        let response = respond(decode(&request).unwrap());
        let expected: &[u8] = if cfg!(feature = "sunfish-servreg-avs-audio") {
            b"\x10\x02\x00\x01\x00\x11\x02\x00\x01\x00\x12\x1c\x00\x01\x11msm/adsp/audio_pd\x4a\x00\x00\x00\x00\x00\x00\x00\x00"
        } else {
            b"\x10\x02\x00\x00\x00\x11\x02\x00\x01\x00"
        };
        assert_eq!(response.optional_tlvs(), expected);
    }

    #[test]
    fn service_name_codes_are_stable_and_unknown_names_stay_unknown() {
        assert_eq!(service_name_code(b"tms/servreg"), NAME_TMS_SERVREG);
        assert_eq!(service_name_code(b"gps/gps_service"), NAME_GPS_SERVICE);
        assert_eq!(service_name_code(b"kernel/elf_loader"), NAME_ELF_LOADER);
        assert_eq!(service_name_code(b"wlan/fw"), NAME_WLAN_FW);
        assert_eq!(service_name_code(b"tms/pdr_enabled"), NAME_TMS_PDR_ENABLED);
        assert_eq!(
            service_name_code(b"tms/pddump_disabled"),
            NAME_TMS_PDDUMP_DISABLED
        );
        assert_eq!(service_name_code(b"tms/other"), NAME_UNKNOWN);
    }

    #[test]
    fn malformed_and_unsupported_queries_fail_closed() {
        let bytes = message(b"", None);
        let request = qmi::Message::decode(&bytes).unwrap();
        assert_eq!(decode(&request), Err(qmi::Error::InvalidValue));

        let bytes = message(b"tms/servreg", None);
        let request = qmi::Message::decode(&bytes).unwrap();
        assert!(decode(&request).is_ok());
        let mut malformed = message(b"tms/servreg", None);
        malformed[0] = 2;
        let malformed = qmi::Message::decode(&malformed).unwrap();
        assert_eq!(decode(&malformed), Err(qmi::Error::UnsupportedMessage));

        let invalid_name = message(b"bad name", None);
        let invalid_name = qmi::Message::decode(&invalid_name).unwrap();
        assert_eq!(decode(&invalid_name), Err(qmi::Error::InvalidValue));

        let mut duplicate = message(b"tms/servreg", Some(0));
        duplicate.extend_from_slice(&[0x10, 4, 0, 1, 0, 0, 0]);
        let payload_len = (duplicate.len() - 7) as u16;
        duplicate[5..7].copy_from_slice(&payload_len.to_le_bytes());
        assert_eq!(
            qmi::Message::decode(&duplicate),
            Err(qmi::Error::DuplicateTlv(0x10))
        );
    }
}
