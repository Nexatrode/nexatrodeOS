//! Host-only checks for the pure RPMh shared-vote model.

#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
mod qcom_rpmh_vote_aggregate;

use qcom_rpmh_vote_aggregate::{
    aggregate, ClientVote, Error, ENABLE_REGISTER, HEADROOM_REGISTER, MAX_CLIENTS, MODE_REGISTER,
    REGISTER_COUNT, VOLTAGE_REGISTER,
};

fn vote(
    set_active: bool,
    set_sleep: bool,
    valid_mask: u8,
    registers: [u32; REGISTER_COUNT],
) -> ClientVote {
    ClientVote {
        set_active,
        set_sleep,
        valid_mask,
        registers,
    }
}

#[test]
fn active_and_sleep_sets_are_max_aggregated_independently() {
    let result = aggregate(&[
        vote(true, true, 0b0011, [6, 1, 2, 0]),
        vote(true, false, 0b0111, [7, 0, 3, 0]),
        vote(false, true, 0b1001, [99, 0, 0, 4]),
    ])
    .unwrap();

    assert_eq!(result.active_valid_mask, 0b0111);
    assert_eq!(result.active_registers, [7, 1, 3, 0]);
    assert_eq!(result.sleep_valid_mask, 0b1011);
    assert_eq!(result.sleep_registers, [99, 1, 0, 4]);
}

#[test]
fn inactive_register_values_do_not_contribute_without_a_valid_bit() {
    let result = aggregate(&[
        vote(true, true, 1 << VOLTAGE_REGISTER, [4, 99, 99, 99]),
        vote(true, true, 1 << ENABLE_REGISTER, [99, 1, 99, 99]),
        vote(true, true, 1 << MODE_REGISTER, [99, 99, 2, 99]),
        vote(true, true, 1 << HEADROOM_REGISTER, [99, 99, 99, 3]),
    ])
    .unwrap();

    assert_eq!(result.active_valid_mask, 0b1111);
    assert_eq!(result.active_registers, [4, 1, 2, 3]);
    assert_eq!(result.sleep_registers, [4, 1, 2, 3]);
}

#[test]
fn empty_vote_set_is_empty_and_overflow_is_rejected() {
    assert_eq!(aggregate(&[]).unwrap().active_valid_mask, 0);
    let votes = [vote(false, false, 0, [0; REGISTER_COUNT]); MAX_CLIENTS + 1];
    assert_eq!(aggregate(&votes), Err(Error::TooManyClients));
}

#[test]
fn malformed_register_mask_is_rejected_even_for_inactive_client() {
    assert_eq!(
        aggregate(&[vote(false, false, 0x10, [0; REGISTER_COUNT])]),
        Err(Error::InvalidRegisterMask)
    );
}
