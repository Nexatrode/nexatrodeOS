//! Decode the bounded Sunfish IPA v4.2 shared-memory snapshot.
//!
//! The matched IPA driver's read-only `gen_reg` debugfs file prints the
//! decoded `IPA_SHARED_MEM_SIZE` fields in 8-byte units. A native IPA owner
//! can instead pass a raw register value to `decode_shared_mem_size_register`
//! after its trusted clock/bus-vote provider has made the register readable.
//! This module only parses or decodes supplied values; it does not map or read
//! MMIO, access debugfs, start the IPA service, or send QMI.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_profile::{self, RuntimeSnapshot};

pub const SUNFISH_IPA_HW_TYPE_V4_2: u32 = 16;
pub const IPA_SHARED_MEM_SIZE_OFFSET: u32 = 0x054;
pub const IPA_SHARED_MEM_BADDR_MASK: u32 = 0xffff_0000;
pub const IPA_SHARED_MEM_BADDR_SHIFT: u32 = 16;
pub const IPA_SHARED_MEM_SIZE_MASK: u32 = 0x0000_ffff;
pub const IPA_SHARED_MEM_SIZE_SHIFT: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    MissingField,
    DuplicateField,
    InvalidValue,
    WrongHardwareType,
    InvalidGeometry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub hardware_type: u32,
    pub shared_mem_baddr_units: u32,
    pub shared_mem_size_units: u32,
}

/// Decode the source-defined `IPA_SHARED_MEM_SIZE` register word. Both fields
/// are in 8-byte units. The caller must obtain the value through the trusted
/// IPA owner after establishing the source-required clock and bus-vote lease;
/// this pure decoder carries no hardware authority.
pub fn decode_shared_mem_size_register(
    hardware_type: u32,
    register_value: u32,
) -> Result<Snapshot, Error> {
    if hardware_type != SUNFISH_IPA_HW_TYPE_V4_2 {
        return Err(Error::WrongHardwareType);
    }
    validate_snapshot(Snapshot {
        hardware_type,
        shared_mem_baddr_units: (register_value & IPA_SHARED_MEM_BADDR_MASK)
            >> IPA_SHARED_MEM_BADDR_SHIFT,
        shared_mem_size_units: (register_value & IPA_SHARED_MEM_SIZE_MASK)
            >> IPA_SHARED_MEM_SIZE_SHIFT,
    })
}

impl Snapshot {
    /// Add state that the capture does not contain. These remain explicit so
    /// callers cannot mistake register geometry for a complete runtime state.
    pub const fn runtime_snapshot(
        self,
        apps_wan_cons_endpoint: Option<u32>,
        ipa_uc_loaded: bool,
    ) -> RuntimeSnapshot {
        RuntimeSnapshot {
            shared_mem_baddr_units: self.shared_mem_baddr_units,
            shared_mem_size_units: self.shared_mem_size_units,
            apps_wan_cons_endpoint,
            ipa_uc_loaded,
        }
    }
}

/// Parse `/sys/kernel/debug/ipa/hw_type` and `gen_reg` output from the
/// existing Linux driver. The source exposes `hw_type` as a decimal u32 and
/// prints the shared-memory fields as hexadecimal, already-decoded 8-byte
/// units. All unrelated register lines are ignored.
pub fn parse_debugfs(hw_type_text: &[u8], gen_reg_text: &[u8]) -> Result<Snapshot, Error> {
    let hardware_type = parse_decimal(trim_ascii(hw_type_text))?;
    if hardware_type != SUNFISH_IPA_HW_TYPE_V4_2 {
        return Err(Error::WrongHardwareType);
    }

    let shared_mem_baddr_units =
        parse_hex(field_value(gen_reg_text, b"IPA_SHARED_MEM_RESTRICTED")?)?;
    let shared_mem_size_units = parse_hex(field_value(gen_reg_text, b"IPA_SHARED_MEM_SIZE")?)?;

    validate_snapshot(Snapshot {
        hardware_type,
        shared_mem_baddr_units,
        shared_mem_size_units,
    })
}

fn validate_snapshot(snapshot: Snapshot) -> Result<Snapshot, Error> {
    let runtime = snapshot.runtime_snapshot(None, false);
    sunfish_ipa_profile::init_request_fields(runtime).map_err(|_| Error::InvalidGeometry)?;
    Ok(snapshot)
}

fn field_value<'a>(text: &'a [u8], name: &[u8]) -> Result<&'a [u8], Error> {
    let mut found = None;
    for line in text.split(|byte| *byte == b'\n') {
        let Some(value) = line
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(b"="))
        else {
            continue;
        };
        if found.is_some() {
            return Err(Error::DuplicateField);
        }
        found = Some(trim_ascii(value));
    }
    found.ok_or(Error::MissingField)
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn parse_decimal(bytes: &[u8]) -> Result<u32, Error> {
    let text = core::str::from_utf8(bytes).map_err(|_| Error::InvalidValue)?;
    u32::from_str_radix(text, 10).map_err(|_| Error::InvalidValue)
}

fn parse_hex(bytes: &[u8]) -> Result<u32, Error> {
    let digits = bytes
        .strip_prefix(b"0x")
        .or_else(|| bytes.strip_prefix(b"0X"))
        .ok_or(Error::InvalidValue)?;
    let text = core::str::from_utf8(digits).map_err(|_| Error::InvalidValue)?;
    u32::from_str_radix(text, 16).map_err(|_| Error::InvalidValue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sunfish_ipa_profile::{self, APPS_WAN_CONS_ENDPOINT};

    const GEN_REG: &[u8] = b"IPA_VERSION=0x1\nIPA_COMP_HW_VERSION=0x10\nIPA_SHARED_MEM_RESTRICTED=0x10\nIPA_SHARED_MEM_SIZE=0x400\n";

    #[test]
    fn parses_v42_debugfs_geometry_in_eight_byte_units() {
        let snapshot = parse_debugfs(b"16\n", GEN_REG).unwrap();
        assert_eq!(snapshot.hardware_type, SUNFISH_IPA_HW_TYPE_V4_2);
        assert_eq!(snapshot.shared_mem_baddr_units, 0x10);
        assert_eq!(snapshot.shared_mem_size_units, 0x400);

        let runtime = snapshot.runtime_snapshot(Some(APPS_WAN_CONS_ENDPOINT), false);
        let fields = sunfish_ipa_profile::init_request_fields(runtime).unwrap();
        assert_eq!(fields.ipa_shared_memory_end_exclusive, 0x2080);
        assert_eq!(fields.control_endpoint, Some(9));
        assert_eq!(fields.is_ssr_bootup, None);
    }

    #[test]
    fn decodes_the_raw_v42_shared_memory_register() {
        let snapshot = decode_shared_mem_size_register(16, 0x0010_0400).unwrap();
        assert_eq!(snapshot.hardware_type, SUNFISH_IPA_HW_TYPE_V4_2);
        assert_eq!(snapshot.shared_mem_baddr_units, 0x10);
        assert_eq!(snapshot.shared_mem_size_units, 0x400);
        assert_eq!(
            snapshot,
            parse_debugfs(
                b"16\n",
                b"IPA_SHARED_MEM_RESTRICTED=0x10\nIPA_SHARED_MEM_SIZE=0x400\n",
            )
            .unwrap()
        );
    }

    #[test]
    fn raw_register_decoder_rejects_wrong_hardware_and_geometry() {
        assert_eq!(
            decode_shared_mem_size_register(15, 0x0010_0400),
            Err(Error::WrongHardwareType)
        );
        assert_eq!(
            decode_shared_mem_size_register(16, 0x0010_03ff),
            Err(Error::InvalidGeometry)
        );
    }

    #[test]
    fn rejects_a_non_v42_driver_snapshot() {
        assert_eq!(
            parse_debugfs(b"15\n", GEN_REG),
            Err(Error::WrongHardwareType)
        );
    }

    #[test]
    fn requires_each_memory_geometry_field_exactly_once() {
        assert_eq!(
            parse_debugfs(b"16", b"IPA_SHARED_MEM_SIZE=0x400\n"),
            Err(Error::MissingField)
        );
        assert_eq!(
            parse_debugfs(
                b"16",
                b"IPA_SHARED_MEM_RESTRICTED=0x10\nIPA_SHARED_MEM_RESTRICTED=0x10\nIPA_SHARED_MEM_SIZE=0x400\n",
            ),
            Err(Error::DuplicateField)
        );
    }

    #[test]
    fn rejects_malformed_or_out_of_range_geometry() {
        assert_eq!(
            parse_debugfs(
                b"16",
                b"IPA_SHARED_MEM_RESTRICTED=0xgg\nIPA_SHARED_MEM_SIZE=0x400\n",
            ),
            Err(Error::InvalidValue)
        );
        assert_eq!(
            parse_debugfs(
                b"16",
                b"IPA_SHARED_MEM_RESTRICTED=0x10\nIPA_SHARED_MEM_SIZE=0x3ff\n",
            ),
            Err(Error::InvalidGeometry)
        );
        assert_eq!(
            parse_debugfs(
                b"16",
                b"IPA_SHARED_MEM_RESTRICTED=0x10000\nIPA_SHARED_MEM_SIZE=0x400\n",
            ),
            Err(Error::InvalidGeometry)
        );
    }
}
