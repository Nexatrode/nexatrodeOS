//! Read-only BCM command-database metadata decoder.
//!
//! Layout: matched msm_bus_fabric_rpmh.c `struct bcm_db` (8 LE bytes).
//! This supplies no bus vote, controller command, ownership, or supported ABI
//! claim. In particular, table version/reserved bytes are reported verbatim.

use crate::qcom_cmd_db::{Error as DbError, Resource};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Database(DbError),
    Length,
    Unit,
    Width,
    ClockDomain,
}

impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::Database(error) => 0x100 + error.code(),
            Self::Length => 1,
            Self::Unit => 2,
            Self::Width => 3,
            Self::ClockDomain => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub id: [u8; 8],
    pub address: u32,
    pub unit_size: u32,
    pub width: u16,
    pub clock_domain: u8,
    pub reserved: u8,
    pub auxiliary_version: u16,
}

impl Metadata {
    pub fn from_resource(resource: &Resource<'_>) -> Result<Self, Error> {
        let address = resource.command_address(5).map_err(Error::Database)?;
        let data: &[u8; 8] = resource.auxiliary.try_into().map_err(|_| Error::Length)?;
        let unit_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let width = u16::from_le_bytes([data[4], data[5]]);
        // Observation is distinct from numeric voting policy. In particular,
        // native ACV metadata has width zero; retain it for source-specific
        // handling instead of hiding the other fields behind a generic guard.
        // The reference bus driver has 16 virtual-clock-domain lists.
        if data[6] >= 16 {
            return Err(Error::ClockDomain);
        }
        Ok(Self {
            id: resource.id,
            address,
            unit_size,
            width,
            clock_domain: data[6],
            reserved: data[7],
            auxiliary_version: resource.auxiliary_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(data: &[u8], address: u32) -> Resource<'_> {
        Resource {
            id: *b"MC0\0\0\0\0\0",
            address,
            slave: ((address >> 16) & 7) as u16,
            table_slave: 5,
            priority: [0; 2],
            auxiliary_version: 0x102,
            auxiliary: data,
        }
    }

    #[test]
    fn decodes_unaligned_little_endian_bytes_and_preserves_opaque_fields() {
        let bytes = [0xff, 0x78, 0x56, 0x34, 0x12, 0x23, 0x01, 15, 0xab];
        let decoded = Metadata::from_resource(&resource(&bytes[1..], 0x50004)).unwrap();
        assert_eq!(
            decoded,
            Metadata {
                id: *b"MC0\0\0\0\0\0",
                address: 0x50004,
                unit_size: 0x12345678,
                width: 0x123,
                clock_domain: 15,
                reserved: 0xab,
                auxiliary_version: 0x102,
            }
        );
    }

    #[test]
    fn rejects_short_and_extended_layouts() {
        let bytes = [1; 9];
        for len in 0..=9 {
            if len != 8 {
                assert_eq!(
                    Metadata::from_resource(&resource(&bytes[..len], 0x50000)),
                    Err(Error::Length)
                );
            }
        }
    }

    #[test]
    fn observes_zero_scaling_fields_but_refuses_out_of_range_domain() {
        let valid = [1, 0, 0, 0, 1, 0, 0, 0];
        let mut data = valid;
        data[0] = 0;
        assert_eq!(
            Metadata::from_resource(&resource(&data, 0x50000))
                .unwrap()
                .unit_size,
            0
        );
        data = valid;
        data[4] = 0;
        let mut acv = resource(&data, 0x50000);
        acv.id = *b"ACV\0\0\0\0\0";
        let decoded = Metadata::from_resource(&acv).unwrap();
        assert_eq!(decoded.width, 0);
        assert_eq!(decoded.id, acv.id);
        data = valid;
        for domain in 16..=255 {
            data[6] = domain;
            assert_eq!(
                Metadata::from_resource(&resource(&data, 0x50000)),
                Err(Error::ClockDomain)
            );
        }
    }

    #[test]
    fn selected_resource_must_have_nonzero_bcm_address() {
        let data = [1, 0, 0, 0, 1, 0, 0, 0];
        for address in [0, 0x30000, 0x40000, 0x60000] {
            assert_eq!(
                Metadata::from_resource(&resource(&data, address)),
                Err(Error::Database(DbError::SlaveId))
            );
        }
        let mut value = resource(&data, 0x50000);
        value.table_slave = 3; // Selected consumer validates address type, not table category.
        assert!(Metadata::from_resource(&value).is_ok());
    }
}
