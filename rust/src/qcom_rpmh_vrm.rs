//! Fixed Sunfish ICNSS regulator actions and shared-rail consumer reduction.
//!
//! The ICNSS action codec translates only pinned voltage and enable
//! operations. The separate shared-rail reducer combines the known ICNSS and
//! WCN3990 consumer requests into one regulator-device vote using Linux
//! regulator-core semantics. Both preserve caller-owned fields outside their
//! bounded scope. Neither proves a complete runtime consumer ledger or decides
//! what reaches hardware; a trusted complete ledger and shared broker remain
//! required. Clock operations and arbitrary load requests are not executable
//! through this codec.

use crate::qcom_rpmh_vote_aggregate::{
    ClientVote, ENABLE_REGISTER, MODE_REGISTER, REGISTER_COUNT, VOLTAGE_REGISTER,
};
use crate::qcom_wlan_power::{Action, Rail, SUNFISH_RAILS};

pub const VRM_MAX_MICROVOLTS: u32 = 8_191_000;

const VALID_REGISTER_MASK: u8 = (1 << REGISTER_COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidVoteMask,
    InvalidActiveSleepSelection,
    NotWlanRail,
    UnsupportedSharedRail,
    VoltageRequestMismatch,
    InvalidVoltageRange,
    NoVoltageSetBeforeEnable,
    LoadOverflow,
    UnsupportedLoad,
    NotVrmAction,
}

/// A Bluetooth-controlled supply that shares an RPMh regulator with ICNSS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wcn3990SharedRail {
    Xtal,
    Core,
    Pa,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wcn3990BtState {
    NeverStarted,
    Enabled,
    Stopped,
}

/// ICNSS's per-consumer state for a regulator shared with Bluetooth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcNssRegulatorState {
    NeverStarted,
    Enabled,
    Stopped,
}

/// One Bluetooth consumer's regulator-core requests. `voltage_range` and
/// `load_microamps` stay at the consumer API level: the captured Bluetooth
/// and ICNSS supply properties resolve to the same regulator device, so the
/// regulator core combines client voltage/load/enable state before the RPMh
/// provider emits its one hardware vote. In particular, do not independently
/// max-aggregate a BT `set_load` result as if it were a separate mode vote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Wcn3990BtRegulatorRequest {
    pub voltage_range_microvolts: Option<(u32, u32)>,
    pub load_microamps: Option<u32>,
    pub enabled: Option<bool>,
}

/// Return the source-derived WCN3990 request for one of the three regulators
/// shared with ICNSS. `Stopped` represents the post-disable requests made by
/// `bluetooth-power.c`; `NeverStarted` has not made consumer requests yet.
pub fn wcn3990_bt_regulator_request(
    rail: Wcn3990SharedRail,
    state: Wcn3990BtState,
) -> Wcn3990BtRegulatorRequest {
    let (min_microvolts, max_microvolts) = match rail {
        Wcn3990SharedRail::Xtal => (1_700_000, 1_900_000),
        Wcn3990SharedRail::Core => (1_245_000, 1_350_000),
        Wcn3990SharedRail::Pa => (3_200_000, 3_400_000),
    };
    match state {
        Wcn3990BtState::NeverStarted => Wcn3990BtRegulatorRequest {
            voltage_range_microvolts: None,
            load_microamps: None,
            enabled: None,
        },
        Wcn3990BtState::Enabled => Wcn3990BtRegulatorRequest {
            voltage_range_microvolts: Some((min_microvolts, max_microvolts)),
            load_microamps: Some(1),
            enabled: Some(true),
        },
        Wcn3990BtState::Stopped => Wcn3990BtRegulatorRequest {
            voltage_range_microvolts: Some((0, max_microvolts)),
            load_microamps: Some(0),
            enabled: Some(false),
        },
    }
}

/// Source-derived ICNSS consumer state for a Sunfish regulator. The upstream
/// enable path skips `regulator_set_load` because each ICNSS load is zero; the
/// stop path explicitly sets load to zero and clears the minimum voltage.
pub fn icnss_regulator_request(
    rail: Rail,
    state: IcNssRegulatorState,
) -> Result<Wcn3990BtRegulatorRequest, Error> {
    let spec = spec(rail)?;
    Ok(match state {
        IcNssRegulatorState::NeverStarted => Wcn3990BtRegulatorRequest {
            voltage_range_microvolts: None,
            load_microamps: None,
            enabled: None,
        },
        IcNssRegulatorState::Enabled => Wcn3990BtRegulatorRequest {
            voltage_range_microvolts: Some((spec.min_microvolts, spec.max_microvolts)),
            load_microamps: None,
            enabled: Some(true),
        },
        IcNssRegulatorState::Stopped => Wcn3990BtRegulatorRequest {
            voltage_range_microvolts: Some((0, spec.max_microvolts)),
            load_microamps: Some(0),
            enabled: Some(false),
        },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SharedRegulatorProfile {
    icnss_rail: Rail,
    bt_rail: Wcn3990SharedRail,
    mode_threshold_microamps: u32,
    low_power_mode: u32,
    high_power_mode: u32,
}

const SUNFISH_SHARED_REGULATOR_PROFILES: [SharedRegulatorProfile; 3] = [
    SharedRegulatorProfile {
        icnss_rail: Rail::Xo18,
        bt_rail: Wcn3990SharedRail::Xtal,
        mode_threshold_microamps: 10_000,
        low_power_mode: 4,
        high_power_mode: 7,
    },
    SharedRegulatorProfile {
        icnss_rail: Rail::Rfa13,
        bt_rail: Wcn3990SharedRail::Core,
        mode_threshold_microamps: 10_000,
        low_power_mode: 4,
        high_power_mode: 7,
    },
    SharedRegulatorProfile {
        icnss_rail: Rail::Ch0_33,
        bt_rail: Wcn3990SharedRail::Pa,
        mode_threshold_microamps: 10_000,
        low_power_mode: 4,
        high_power_mode: 7,
    },
];

/// Combine the two known consumer requests into the single regulator-device
/// request that RPMh sees. This follows regulator-core voltage intersection,
/// enable reference counting, and summed-load semantics; it does not grant
/// hardware authority or prove that no additional runtime consumers exist.
pub fn combine_sunfish_shared_regulator_request(
    base_vote: ClientVote,
    rail: Rail,
    icnss_state: IcNssRegulatorState,
    bluetooth_state: Wcn3990BtState,
    system_load_microamps: u32,
) -> Result<ClientVote, Error> {
    if base_vote.valid_mask & !VALID_REGISTER_MASK != 0 {
        return Err(Error::InvalidVoteMask);
    }
    if !base_vote.set_active || !base_vote.set_sleep {
        return Err(Error::InvalidActiveSleepSelection);
    }
    let profile = SUNFISH_SHARED_REGULATOR_PROFILES
        .iter()
        .find(|profile| profile.icnss_rail == rail)
        .ok_or(Error::UnsupportedSharedRail)?;
    let icnss_request = icnss_regulator_request(rail, icnss_state)?;
    let bluetooth_request = wcn3990_bt_regulator_request(profile.bt_rail, bluetooth_state);
    combine_regulator_consumers(
        base_vote,
        icnss_request,
        bluetooth_request,
        profile,
        system_load_microamps,
    )
}

fn combine_regulator_consumers(
    mut vote: ClientVote,
    icnss: Wcn3990BtRegulatorRequest,
    bluetooth: Wcn3990BtRegulatorRequest,
    profile: &SharedRegulatorProfile,
    system_load_microamps: u32,
) -> Result<ClientVote, Error> {
    let mut min_microvolts = 0;
    let mut max_microvolts = u32::MAX;
    let mut has_voltage_request = false;
    for (minimum, maximum) in [
        icnss.voltage_range_microvolts,
        bluetooth.voltage_range_microvolts,
    ]
    .into_iter()
    .flatten()
    {
        if minimum > maximum {
            return Err(Error::InvalidVoltageRange);
        }
        min_microvolts = min_microvolts.max(minimum);
        max_microvolts = max_microvolts.min(maximum);
        has_voltage_request = true;
    }
    if has_voltage_request {
        vote.registers[VOLTAGE_REGISTER] =
            round_up_vrm_request_millivolts(min_microvolts, max_microvolts)?;
        vote.valid_mask |= 1 << VOLTAGE_REGISTER;
    }

    let enabled = [icnss.enabled, bluetooth.enabled];
    if enabled.iter().any(Option::is_some) {
        vote.registers[ENABLE_REGISTER] = u32::from(enabled.contains(&Some(true)));
        vote.valid_mask |= 1 << ENABLE_REGISTER;
    }

    let loads = [icnss.load_microamps, bluetooth.load_microamps];
    // regulator_enable() invokes the regulator core's DRMS update before the
    // enable operation, even when this consumer has not called set_load. Treat
    // absent per-consumer load requests as their current zero load, then fold
    // in system load and any load requests from the modeled sibling client.
    if loads.iter().any(Option::is_some) || enabled.iter().any(Option::is_some) {
        let total = loads
            .into_iter()
            .flatten()
            .fold(Some(system_load_microamps), |sum, load| {
                sum.and_then(|sum| sum.checked_add(load))
            })
            .ok_or(Error::LoadOverflow)?;
        vote.registers[MODE_REGISTER] = if total >= profile.mode_threshold_microamps {
            profile.high_power_mode
        } else {
            profile.low_power_mode
        };
        vote.valid_mask |= 1 << MODE_REGISTER;
    }
    Ok(vote)
}

/// Apply one source-plan operation to this client's regulator vote. The
/// voltage request must exactly match the verified Sunfish plan so callers
/// cannot use this codec to vote a different voltage or rail.
pub fn apply_icnss_action(mut vote: ClientVote, action: Action) -> Result<ClientVote, Error> {
    if vote.valid_mask & !VALID_REGISTER_MASK != 0 {
        return Err(Error::InvalidVoteMask);
    }
    match action {
        Action::SetVoltage {
            rail,
            min_microvolts,
            max_microvolts,
        } => {
            let spec = spec(rail)?;
            if (min_microvolts, max_microvolts) != (spec.min_microvolts, spec.max_microvolts) {
                return Err(Error::VoltageRequestMismatch);
            }
            let millivolts = round_up_millivolts(min_microvolts, max_microvolts)?;
            vote.registers[VOLTAGE_REGISTER] = millivolts;
            vote.valid_mask |= 1 << VOLTAGE_REGISTER;
            Ok(vote)
        }
        Action::EnableRail(rail) => {
            spec(rail)?;
            if vote.valid_mask & (1 << VOLTAGE_REGISTER) == 0 {
                return Err(Error::NoVoltageSetBeforeEnable);
            }
            vote.registers[ENABLE_REGISTER] = 1;
            vote.valid_mask |= 1 << ENABLE_REGISTER;
            Ok(vote)
        }
        Action::SetLoad { microamps, .. } if microamps != 0 => Err(Error::UnsupportedLoad),
        Action::SetLoad { .. } => Err(Error::UnsupportedLoad),
        Action::DelayMicroseconds { .. } | Action::EnableCxoRefClock => Err(Error::NotVrmAction),
    }
}

fn spec(rail: Rail) -> Result<&'static crate::qcom_wlan_power::RailSpec, Error> {
    SUNFISH_RAILS
        .iter()
        .find(|candidate| candidate.rail == rail)
        .ok_or(Error::NotWlanRail)
}

fn round_up_millivolts(min_microvolts: u32, max_microvolts: u32) -> Result<u32, Error> {
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
    if rounded > max_microvolts || rounded > VRM_MAX_MICROVOLTS {
        return Err(Error::InvalidVoltageRange);
    }
    Ok(millivolts)
}

/// Encode the regulator-core intersection's minimum for the VRM provider.
/// Zero is valid after every consumer has cleared its minimum-voltage request.
fn round_up_vrm_request_millivolts(min_microvolts: u32, max_microvolts: u32) -> Result<u32, Error> {
    if min_microvolts > max_microvolts {
        return Err(Error::InvalidVoltageRange);
    }
    if min_microvolts == 0 {
        return Ok(0);
    }
    round_up_millivolts(min_microvolts, max_microvolts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_vote_aggregate::{HEADROOM_REGISTER, MODE_REGISTER};

    fn vote() -> ClientVote {
        ClientVote {
            set_active: true,
            set_sleep: true,
            valid_mask: (1 << MODE_REGISTER) | (1 << HEADROOM_REGISTER),
            registers: [0, 0, 7, 55],
        }
    }

    #[test]
    fn voltage_request_encodes_millivolts_and_preserves_other_client_fields() {
        let encoded = apply_icnss_action(
            vote(),
            Action::SetVoltage {
                rail: Rail::Rfa13,
                min_microvolts: 1_304_000,
                max_microvolts: 1_304_000,
            },
        )
        .unwrap();
        assert_eq!(encoded.registers, [1304, 0, 7, 55]);
        assert_eq!(encoded.valid_mask, 0b1101);
        assert!(encoded.set_active && encoded.set_sleep);
    }

    #[test]
    fn enable_requires_prior_voltage_and_sets_only_the_enable_register() {
        assert_eq!(
            apply_icnss_action(vote(), Action::EnableRail(Rail::Xo18)),
            Err(Error::NoVoltageSetBeforeEnable)
        );
        let voltage = apply_icnss_action(
            vote(),
            Action::SetVoltage {
                rail: Rail::Xo18,
                min_microvolts: 1_800_000,
                max_microvolts: 1_800_000,
            },
        )
        .unwrap();
        let enabled = apply_icnss_action(voltage, Action::EnableRail(Rail::Xo18)).unwrap();
        assert_eq!(enabled.registers, [1800, 1, 7, 55]);
        assert_eq!(enabled.valid_mask, 0b1111);
    }

    #[test]
    fn only_exact_profile_rail_voltage_pairs_are_accepted() {
        assert_eq!(
            apply_icnss_action(
                vote(),
                Action::SetVoltage {
                    rail: Rail::Ch0_33,
                    min_microvolts: 3_300_000,
                    max_microvolts: 3_312_000,
                }
            ),
            Err(Error::VoltageRequestMismatch)
        );
        assert_eq!(
            apply_icnss_action(
                vote(),
                Action::SetVoltage {
                    rail: Rail::Ch0_33,
                    min_microvolts: 3_312_000,
                    max_microvolts: 3_312_000,
                }
            )
            .unwrap()
            .registers[VOLTAGE_REGISTER],
            3312
        );
    }

    #[test]
    fn non_vrm_actions_and_unmodeled_loads_are_rejected() {
        assert_eq!(
            apply_icnss_action(vote(), Action::EnableCxoRefClock),
            Err(Error::NotVrmAction)
        );
        assert_eq!(
            apply_icnss_action(
                vote(),
                Action::SetLoad {
                    rail: Rail::CxMx,
                    microamps: 1,
                }
            ),
            Err(Error::UnsupportedLoad)
        );
    }

    #[test]
    fn wcn3990_shared_regulator_requests_match_source_states() {
        let profiles = [
            (Wcn3990SharedRail::Xtal, (1_700_000, 1_900_000)),
            (Wcn3990SharedRail::Core, (1_245_000, 1_350_000)),
            (Wcn3990SharedRail::Pa, (3_200_000, 3_400_000)),
        ];
        for (rail, voltage_range_microvolts) in profiles {
            let never_started = wcn3990_bt_regulator_request(rail, Wcn3990BtState::NeverStarted);
            assert_eq!(
                never_started,
                Wcn3990BtRegulatorRequest {
                    voltage_range_microvolts: None,
                    load_microamps: None,
                    enabled: None,
                }
            );

            let enabled = wcn3990_bt_regulator_request(rail, Wcn3990BtState::Enabled);
            assert_eq!(
                enabled,
                Wcn3990BtRegulatorRequest {
                    voltage_range_microvolts: Some(voltage_range_microvolts),
                    load_microamps: Some(1),
                    enabled: Some(true),
                }
            );

            let stopped = wcn3990_bt_regulator_request(rail, Wcn3990BtState::Stopped);
            assert_eq!(
                stopped,
                Wcn3990BtRegulatorRequest {
                    voltage_range_microvolts: Some((0, voltage_range_microvolts.1)),
                    load_microamps: Some(0),
                    enabled: Some(false),
                }
            );
        }
    }

    #[test]
    fn shared_regulator_combines_voltage_enable_and_mode_before_rpmh_aggregation() {
        assert_eq!(
            icnss_regulator_request(Rail::Xo18, IcNssRegulatorState::NeverStarted),
            Ok(Wcn3990BtRegulatorRequest {
                voltage_range_microvolts: None,
                load_microamps: None,
                enabled: None,
            })
        );

        let enabled = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Rfa13,
            IcNssRegulatorState::Enabled,
            Wcn3990BtState::Enabled,
            0,
        )
        .unwrap();
        // ICNSS's fixed 1.304 V range intersects BT's 1.245–1.35 V range.
        assert_eq!(enabled.registers, [1304, 1, 4, 55]);
        assert_eq!(enabled.valid_mask, 0b1111);
        assert!(enabled.set_active && enabled.set_sleep);

        let bluetooth_only = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Rfa13,
            IcNssRegulatorState::Stopped,
            Wcn3990BtState::Enabled,
            0,
        )
        .unwrap();
        assert_eq!(bluetooth_only.registers, [1245, 1, 4, 55]);

        let stopped = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Rfa13,
            IcNssRegulatorState::Stopped,
            Wcn3990BtState::Stopped,
            0,
        )
        .unwrap();
        assert_eq!(stopped.registers, [0, 0, 4, 55]);

        let not_started = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Rfa13,
            IcNssRegulatorState::Enabled,
            Wcn3990BtState::NeverStarted,
            0,
        )
        .unwrap();
        // ICNSS skips set_load for its zero configured load, but regulator_enable
        // still recalculates DRMS and selects LPM at a zero total load.
        assert_eq!(not_started.registers, [1304, 1, 4, 55]);
        assert_eq!(not_started.valid_mask, 0b1111);
    }

    #[test]
    fn enable_recalculates_mode_from_system_load_even_without_client_set_load() {
        let below_threshold = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Xo18,
            IcNssRegulatorState::Enabled,
            Wcn3990BtState::NeverStarted,
            9_999,
        )
        .unwrap();
        assert_eq!(below_threshold.registers[MODE_REGISTER], 4);

        let at_threshold = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Xo18,
            IcNssRegulatorState::Enabled,
            Wcn3990BtState::NeverStarted,
            10_000,
        )
        .unwrap();
        assert_eq!(at_threshold.registers[MODE_REGISTER], 7);
    }

    #[test]
    fn never_started_clients_without_a_transition_preserve_the_prior_mode() {
        let unchanged = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Xo18,
            IcNssRegulatorState::NeverStarted,
            Wcn3990BtState::NeverStarted,
            0,
        )
        .unwrap();
        assert_eq!(unchanged.registers[MODE_REGISTER], 7);
    }

    #[test]
    fn all_captured_shared_rails_use_their_intersected_voltage_ranges() {
        let profiles = [
            (Rail::Xo18, 1800),
            (Rail::Rfa13, 1304),
            (Rail::Ch0_33, 3312),
        ];
        for (rail, expected_millivolts) in profiles {
            let combined = combine_sunfish_shared_regulator_request(
                vote(),
                rail,
                IcNssRegulatorState::Enabled,
                Wcn3990BtState::Enabled,
                0,
            )
            .unwrap();
            assert_eq!(combined.registers[VOLTAGE_REGISTER], expected_millivolts);
            assert_eq!(combined.registers[ENABLE_REGISTER], 1);
        }
    }

    #[test]
    fn shared_regulator_sums_system_and_consumer_loads_with_checked_threshold() {
        let below = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Xo18,
            IcNssRegulatorState::Enabled,
            Wcn3990BtState::Enabled,
            9_998,
        )
        .unwrap();
        assert_eq!(below.registers[MODE_REGISTER], 4);

        let at_threshold = combine_sunfish_shared_regulator_request(
            vote(),
            Rail::Xo18,
            IcNssRegulatorState::Enabled,
            Wcn3990BtState::Enabled,
            9_999,
        )
        .unwrap();
        assert_eq!(at_threshold.registers[MODE_REGISTER], 7);

        assert_eq!(
            combine_sunfish_shared_regulator_request(
                vote(),
                Rail::Xo18,
                IcNssRegulatorState::Stopped,
                Wcn3990BtState::Enabled,
                u32::MAX,
            ),
            Err(Error::LoadOverflow)
        );
    }

    #[test]
    fn shared_regulator_rejects_conflicting_consumer_voltage_ranges() {
        let profile = SUNFISH_SHARED_REGULATOR_PROFILES[0];
        assert_eq!(
            combine_regulator_consumers(
                vote(),
                Wcn3990BtRegulatorRequest {
                    voltage_range_microvolts: Some((1_900_001, 2_000_000)),
                    load_microamps: None,
                    enabled: None,
                },
                Wcn3990BtRegulatorRequest {
                    voltage_range_microvolts: Some((1_700_000, 1_900_000)),
                    load_microamps: None,
                    enabled: None,
                },
                &profile,
                0,
            ),
            Err(Error::InvalidVoltageRange)
        );
    }

    #[test]
    fn shared_regulator_rejects_unsupported_rail_and_incomplete_vote_scope() {
        assert_eq!(
            combine_sunfish_shared_regulator_request(
                vote(),
                Rail::Ch1_33,
                IcNssRegulatorState::Enabled,
                Wcn3990BtState::NeverStarted,
                0,
            ),
            Err(Error::UnsupportedSharedRail)
        );

        let mut active_only = vote();
        active_only.set_sleep = false;
        assert_eq!(
            combine_sunfish_shared_regulator_request(
                active_only,
                Rail::Xo18,
                IcNssRegulatorState::Enabled,
                Wcn3990BtState::NeverStarted,
                0,
            ),
            Err(Error::InvalidActiveSleepSelection)
        );
    }

    #[test]
    fn malformed_vote_masks_and_voltage_ranges_are_rejected() {
        let mut malformed = vote();
        malformed.valid_mask = 0x80;
        assert_eq!(
            apply_icnss_action(
                malformed,
                Action::SetVoltage {
                    rail: Rail::CxMx,
                    min_microvolts: 640_000,
                    max_microvolts: 640_000,
                }
            ),
            Err(Error::InvalidVoteMask)
        );
        assert_eq!(round_up_millivolts(1_304_001, 1_305_000), Ok(1305));
        assert_eq!(round_up_vrm_request_millivolts(0, 1_800_000), Ok(0));
        assert_eq!(round_up_vrm_request_millivolts(0, 0), Ok(0));
        assert_eq!(
            round_up_vrm_request_millivolts(1_800_001, 1_800_000),
            Err(Error::InvalidVoltageRange)
        );
        assert_eq!(
            round_up_millivolts(1_304_001, 1_304_001),
            Err(Error::InvalidVoltageRange)
        );
        assert_eq!(
            round_up_millivolts(u32::MAX, u32::MAX),
            Err(Error::InvalidVoltageRange)
        );
    }
}
