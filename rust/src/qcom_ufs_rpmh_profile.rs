//! Source-derived Sunfish UFS contributions to RPMh VRM rail votes.
//!
//! The profile represents only the UFS consumer requests after the source
//! driver has applied its pre-CAP voltage, load, and enable operations. It is
//! not a complete per-rail client ledger and cannot be submitted to an RSC.

use crate::qcom_rpmh_vote_aggregate::{
    ClientVote, ENABLE_REGISTER, MODE_REGISTER, VOLTAGE_REGISTER,
};

pub const SUPPLY_COUNT: usize = 5;
pub const ACTIVE_SLEEP_SET: u8 = 3;
pub const PMIC5_LDO_LPM_MODE: u32 = 4;
pub const PMIC5_LDO_HPM_MODE: u32 = 7;
pub const HPM_MIN_LOAD_UA: u32 = 1;
pub const CLIENT_VOTE_VALID_MASK: u8 =
    (1 << VOLTAGE_REGISTER) | (1 << ENABLE_REGISTER) | (1 << MODE_REGISTER);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SupplyClient {
    Vcc = 0,
    Vccq2 = 1,
    VddaPhy = 2,
    VddaPll = 3,
    VddpRefClk = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupplySpec {
    pub resource_key: &'static str,
    pub min_microvolts: u32,
    pub max_microvolts: u32,
    pub max_load_microamps: u32,
    /// `qcom,set = RPMH_REGULATOR_SET_ALL` selects active and sleep sets.
    pub set_mask: u8,
}

/// Runtime-DT consumer values and PHY voltage limits from the pinned Linux
/// source, ordered like the five UFS regulator clients.
pub const UFS_SUPPLY_SPECS: [SupplySpec; SUPPLY_COUNT] = [
    SupplySpec {
        resource_key: "ldoa19",
        min_microvolts: 0x2d0370,
        max_microvolts: 0x2d2a80,
        max_load_microamps: 0x927c0,
        set_mask: ACTIVE_SLEEP_SET,
    },
    SupplySpec {
        resource_key: "ldoa12",
        min_microvolts: 0x1b7740,
        max_microvolts: 0x1dc130,
        max_load_microamps: 0x927c0,
        set_mask: ACTIVE_SLEEP_SET,
    },
    SupplySpec {
        resource_key: "ldoa4",
        min_microvolts: 800_000,
        max_microvolts: 925_000,
        max_load_microamps: 0xf5b4,
        set_mask: ACTIVE_SLEEP_SET,
    },
    SupplySpec {
        resource_key: "ldoc3",
        min_microvolts: 1_200_000,
        max_microvolts: 1_800_000,
        max_load_microamps: 0x477c,
        set_mask: ACTIVE_SLEEP_SET,
    },
    SupplySpec {
        resource_key: "ldoc3",
        min_microvolts: 1_200_000,
        max_microvolts: 1_200_000,
        max_load_microamps: 0x64,
        set_mask: ACTIVE_SLEEP_SET,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidVoltageRange,
    InvalidClient,
    UnsupportedSet,
}

/// Match the pinned VRM driver's `DIV_ROUND_UP(min_uv, 1000)` request, while
/// enforcing the driver's max-voltage bound before creating a vote.
pub fn requested_millivolts(min_microvolts: u32, max_microvolts: u32) -> Result<u32, Error> {
    if min_microvolts == 0 || min_microvolts > max_microvolts {
        return Err(Error::InvalidVoltageRange);
    }
    let millivolts = min_microvolts
        .checked_add(999)
        .ok_or(Error::InvalidVoltageRange)?
        / 1000;
    let rounded = millivolts
        .checked_mul(1000)
        .ok_or(Error::InvalidVoltageRange)?;
    if rounded > max_microvolts {
        return Err(Error::InvalidVoltageRange);
    }
    Ok(millivolts)
}

/// Build one bounded UFS consumer contribution after its pre-CAP enable.
/// Every current source profile has a nonzero load, selecting PMIC5 LDO HPM
/// from the pinned `<0, 1>` load-threshold list. This vote is not sufficient to
/// instantiate a production broker without all other clients and caches.
pub fn enabled_client_vote(client: SupplyClient) -> Result<ClientVote, Error> {
    let spec = UFS_SUPPLY_SPECS
        .get(client as usize)
        .ok_or(Error::InvalidClient)?;
    if spec.set_mask != ACTIVE_SLEEP_SET {
        return Err(Error::UnsupportedSet);
    }
    let voltage_mv = requested_millivolts(spec.min_microvolts, spec.max_microvolts)?;
    let mode = if spec.max_load_microamps >= HPM_MIN_LOAD_UA {
        PMIC5_LDO_HPM_MODE
    } else {
        PMIC5_LDO_LPM_MODE
    };
    Ok(ClientVote {
        set_active: true,
        set_sleep: true,
        valid_mask: CLIENT_VOTE_VALID_MASK,
        registers: [voltage_mv, 1, mode, 0],
    })
}
