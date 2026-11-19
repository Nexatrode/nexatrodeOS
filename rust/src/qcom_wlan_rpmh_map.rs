//! Resolve the pinned Sunfish ICNSS regulator supplies to RPMh command IDs.
//!
//! These are source-derived command-DB keys, ordered like `SUNFISH_RAILS`.
//! Resolving a key proves only that a bounded database contains one matching
//! VRM address; it does not authenticate that database or grant RSC ownership.

use crate::qcom_cmd_db::{Database, Error as DatabaseError};
use crate::qcom_wlan_power::RAIL_COUNT;

pub const VRM_SLAVE_ID: u16 = 3;

/// `SUNFISH_RAILS` order: CX/MX, XO18, RFA13, CH1 3.3 V, CH0 3.3 V.
pub const SUNFISH_RAIL_RESOURCE_KEYS: [&str; RAIL_COUNT] =
    ["ldoa9", "ldoc1", "ldoc2", "ldoc11", "ldoc10"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Database(DatabaseError),
    DuplicateAddress { first_rail: u8, second_rail: u8 },
}

/// Resolve and validate all five regulator command addresses from a copied
/// command database. Missing/ambiguous keys and non-VRM addresses fail closed.
pub fn sunfish_rail_addresses(db: &Database<'_>) -> Result<[u32; RAIL_COUNT], Error> {
    let mut addresses = [0; RAIL_COUNT];
    for (index, key) in SUNFISH_RAIL_RESOURCE_KEYS.iter().enumerate() {
        let resource = db.lookup(key).map_err(Error::Database)?;
        let address = resource
            .command_address(VRM_SLAVE_ID)
            .map_err(Error::Database)?;
        if let Some(first) = addresses[..index]
            .iter()
            .position(|candidate| *candidate == address)
        {
            return Err(Error::DuplicateAddress {
                first_rail: first as u8,
                second_rail: index as u8,
            });
        }
        addresses[index] = address;
    }
    Ok(addresses)
}
