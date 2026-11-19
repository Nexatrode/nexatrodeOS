//! Host tests for the source-derived UFS-to-RPMh resource map.

#[path = "../rust/src/qcom_cmd_db.rs"]
#[allow(dead_code)]
mod qcom_cmd_db;
#[path = "../rust/src/qcom_ufs_rpmh_map.rs"]
mod qcom_ufs_rpmh_map;

use qcom_cmd_db::{Database, HEADER_BYTES, MAGIC};
use qcom_ufs_rpmh_map::{Error, sunfish_supply_addresses};

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(resources: [(&str, u32); 4]) -> Vec<u8> {
    let mut bytes = vec![0; HEADER_BYTES + resources.len() * 24];
    put32(&mut bytes, 4, MAGIC);
    put16(&mut bytes, 8, 3);
    put16(&mut bytes, 10, 0);
    put16(&mut bytes, 14, resources.len() as u16);
    for (index, (name, address)) in resources.iter().enumerate() {
        let entry = HEADER_BYTES + index * 24;
        bytes[entry..entry + name.len()].copy_from_slice(name.as_bytes());
        put32(&mut bytes, entry + 16, *address);
    }
    bytes
}

const RESOURCES: [(&str, u32); 4] = [
    ("ldoa19", 0x30000),
    ("ldoa12", 0x30004),
    ("ldoa4", 0x30008),
    ("ldoc3", 0x3000c),
];
const ADDRESSES: [u32; 5] = [0x30000, 0x30004, 0x30008, 0x3000c, 0x3000c];

#[test]
fn resolves_all_ufs_clients_and_allows_only_the_known_shared_rail() {
    let bytes = fixture(RESOURCES);
    let database = Database::parse(&bytes).unwrap();
    assert_eq!(
        sunfish_supply_addresses(&database),
        Ok(ADDRESSES)
    );
}

#[test]
fn rejects_an_unexpected_alias_between_distinct_rails() {
    let bytes = fixture([
        ("ldoa19", 0x30000),
        ("ldoa12", 0x30004),
        ("ldoa4", 0x30000),
        ("ldoc3", 0x3000c),
    ]);
    let database = Database::parse(&bytes).unwrap();
    assert_eq!(
        sunfish_supply_addresses(&database),
        Err(Error::UnexpectedAlias {
            first_client: 0,
            second_client: 2,
        })
    );
}

#[test]
fn rejects_a_resource_that_is_not_a_vrm_command_address() {
    let bytes = fixture([
        ("ldoa19", 0x30000),
        ("ldoa12", 0x30004),
        ("ldoa4", 0x30008),
        ("ldoc3", 0x5000c),
    ]);
    let database = Database::parse(&bytes).unwrap();
    assert_eq!(
        sunfish_supply_addresses(&database),
        Err(Error::Database(qcom_cmd_db::Error::SlaveId))
    );
}

#[test]
fn fails_closed_when_a_source_resource_is_missing() {
    let bytes = fixture([
        ("ldoa19", 0x30000),
        ("ldoa12", 0x30004),
        ("ldoa4", 0x30008),
        ("other", 0x3000c),
    ]);
    let database = Database::parse(&bytes).unwrap();
    assert_eq!(
        sunfish_supply_addresses(&database),
        Err(Error::Database(qcom_cmd_db::Error::NotFound))
    );
}
