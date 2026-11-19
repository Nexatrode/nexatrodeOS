//! Resolve the pinned Sunfish UFS regulator clients to RPMh VRM resources.
//!
//! The reference-clock pad and PHY PLL clients intentionally resolve to the
//! same PMIC rail. This maps resource identities only; it does not issue votes
//! or grant RSC ownership.

use crate::qcom_cmd_db::{Database, Error as DatabaseError};

pub const VRM_SLAVE_ID: u16 = 3;

/// Ordered as the UFS platform's five independently managed regulator clients.
/// `vddp-ref-clk` is a second client on the `vdda-pll` physical rail.
pub const UFS_SUPPLY_RESOURCE_KEYS: [&str; 5] = ["ldoa19", "ldoa12", "ldoa4", "ldoc3", "ldoc3"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Database(DatabaseError),
    UnexpectedAlias { first_client: u8, second_client: u8 },
}

/// Resolve UFS client addresses from a bounded copy of the command database.
/// Only the documented PHY-PLL/reference-clock pair may share an address.
pub fn sunfish_supply_addresses(db: &Database<'_>) -> Result<[u32; 5], Error> {
    let mut addresses = [0; 5];
    for (index, key) in UFS_SUPPLY_RESOURCE_KEYS.iter().enumerate() {
        let resource = db.lookup(key).map_err(Error::Database)?;
        let address = resource
            .command_address(VRM_SLAVE_ID)
            .map_err(Error::Database)?;
        if let Some(first) = addresses[..index]
            .iter()
            .position(|candidate| *candidate == address)
        {
            if (first, index) != (3, 4) {
                return Err(Error::UnexpectedAlias {
                    first_client: first as u8,
                    second_client: index as u8,
                });
            }
        }
        addresses[index] = address;
    }
    Ok(addresses)
}
