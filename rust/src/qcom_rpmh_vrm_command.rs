//! Bounded RPMh VRM command batches for one broker aggregate phase.
//!
//! The command shape follows the pinned rpmh-regulator driver: each valid VRM
//! register is written at `resource address + register * 4` with the selected
//! active, sleep, or wake aggregate. Wake-only commands use the active
//! aggregate when the source driver requires a future wake set. This planner
//! performs no RSC access and claims no submission or completion.

use crate::qcom_rpmh_vote_aggregate::REGISTER_COUNT;
use crate::qcom_rpmh_vote_broker::{AggregatePhase, UpdatePlan};

pub const MAX_COMMANDS: usize = REGISTER_COUNT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command {
    pub register: u8,
    pub address: u32,
    pub data: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Batch {
    pub resource_address: u32,
    pub identity: u64,
    pub generation: u64,
    pub ticket: u64,
    pub phase: AggregatePhase,
    commands: [Option<Command>; MAX_COMMANDS],
    count: usize,
}

impl Batch {
    pub fn command(&self, index: usize) -> Option<Command> {
        if index < self.count {
            self.commands[index]
        } else {
            None
        }
    }

    pub const fn len(&self) -> usize {
        self.count
    }

    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    PhaseNotRequired,
    InvalidRegisterMask,
    VoteRemovalUnsupported,
    AddressOverflow,
}

/// Create a finite command list for the exact changed fields in one phase.
/// The first differing sleep request resends the full active/wake set, as the
/// source regulator does. Clearing a previously valid field is rejected until
/// the platform owner implements source-matched removal behavior.
pub fn build_batch(plan: UpdatePlan, phase: AggregatePhase) -> Result<Batch, Error> {
    let required = match phase {
        AggregatePhase::Active => plan.active_required(),
        AggregatePhase::Sleep => plan.sleep_required(),
        AggregatePhase::Wake => plan.wake_required(),
    };
    if !required {
        return Err(Error::PhaseNotRequired);
    }
    if plan.removed_mask(phase) != 0 {
        return Err(Error::VoteRemovalUnsupported);
    }

    let aggregate = plan.desired();
    let (valid_mask, registers) = match phase {
        AggregatePhase::Active => (aggregate.active_valid_mask, aggregate.active_registers),
        AggregatePhase::Sleep => (aggregate.sleep_valid_mask, aggregate.sleep_registers),
        AggregatePhase::Wake => (aggregate.active_valid_mask, aggregate.active_registers),
    };
    if valid_mask & !((1 << REGISTER_COUNT) - 1) != 0 {
        return Err(Error::InvalidRegisterMask);
    }
    let command_mask = plan.command_mask(phase);
    if command_mask & !valid_mask != 0 {
        return Err(Error::InvalidRegisterMask);
    }

    let mut batch = Batch {
        resource_address: plan.resource_address(),
        identity: plan.identity(),
        generation: plan.generation(),
        ticket: plan.ticket(),
        phase,
        commands: [None; MAX_COMMANDS],
        count: 0,
    };
    for (register, data) in registers.iter().copied().enumerate() {
        if command_mask & (1 << register) == 0 {
            continue;
        }
        let offset = u32::try_from(register)
            .ok()
            .and_then(|index| index.checked_mul(4))
            .ok_or(Error::AddressOverflow)?;
        let address = batch
            .resource_address
            .checked_add(offset)
            .ok_or(Error::AddressOverflow)?;
        batch.commands[batch.count] = Some(Command {
            register: register as u8,
            address,
            data,
        });
        batch.count += 1;
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_vote_aggregate::ClientVote;
    use crate::qcom_rpmh_vote_broker::{Broker, CompleteVoteSet};

    fn make_broker(address: u32, vote: ClientVote) -> Broker {
        Broker::from_complete_set(CompleteVoteSet::test_fixture(
            address,
            0x51,
            9,
            &[(1, vote)],
        ))
        .unwrap_or_else(|_| panic!("valid fixture"))
    }

    fn make_broker_with_sleep_cache(address: u32, vote: ClientVote) -> Broker {
        Broker::from_complete_set(CompleteVoteSet::test_fixture_with_state(
            address,
            0x51,
            9,
            &[(1, vote)],
            true,
            false,
        ))
        .unwrap_or_else(|_| panic!("valid fixture"))
    }

    fn vote(active: bool, sleep: bool, mask: u8, registers: [u32; 4]) -> ClientVote {
        ClientVote {
            set_active: active,
            set_sleep: sleep,
            valid_mask: mask,
            registers,
        }
    }

    #[test]
    fn batch_encodes_changed_registers_for_each_required_phase() {
        let initial = vote(true, true, 0b1111, [640, 0, 7, 55]);
        let mut broker = make_broker_with_sleep_cache(0x30000, initial);
        let next = vote(true, true, 0b1111, [640, 1, 7, 55]);
        let plan = broker.begin_update(1, next).unwrap();

        let sleep = build_batch(plan, AggregatePhase::Sleep).unwrap();
        assert_eq!(sleep.resource_address, 0x30000);
        assert_eq!(sleep.identity, 0x51);
        assert_eq!(sleep.generation, 9);
        assert_eq!(sleep.ticket, plan.ticket());
        assert_eq!(sleep.phase, AggregatePhase::Sleep);
        assert!(!sleep.is_empty());
        assert_eq!(sleep.len(), 1);
        assert_eq!(
            sleep.command(0),
            Some(Command {
                register: 1,
                address: 0x30004,
                data: next.registers[1],
            })
        );
        assert_eq!(sleep.command(1), None);

        let active = build_batch(plan, AggregatePhase::Active).unwrap();
        assert_eq!(active.phase, AggregatePhase::Active);
        assert_eq!(active.len(), 1);
        assert_eq!(active.command(0).unwrap().register, 1);

        let wake = build_batch(plan, AggregatePhase::Wake).unwrap();
        assert_eq!(wake.phase, AggregatePhase::Wake);
        assert_eq!(wake.len(), 1);
        assert_eq!(wake.command(0).unwrap().data, next.registers[1]);
    }

    #[test]
    fn unchanged_phase_is_not_emitted() {
        let initial = vote(true, false, 0b0011, [640, 0, 0, 0]);
        let mut broker = make_broker(0x30000, initial);
        let plan = broker
            .begin_update(1, vote(true, false, 0b0011, [640, 1, 0, 0]))
            .unwrap();
        assert_eq!(
            build_batch(plan, AggregatePhase::Sleep),
            Err(Error::PhaseNotRequired)
        );
    }

    #[test]
    fn vote_removal_and_address_overflow_fail_closed() {
        let initial = vote(true, true, 0b1111, [640, 0, 7, 55]);
        let mut broker = make_broker(0x30000, initial);
        let removal = broker
            .begin_update(1, vote(true, true, 0b0111, [640, 0, 7, 0]))
            .unwrap();
        assert_eq!(
            build_batch(removal, AggregatePhase::Active),
            Err(Error::VoteRemovalUnsupported)
        );

        let mut overflowing = make_broker(u32::MAX - 3, initial);
        let update = overflowing
            .begin_update(1, vote(true, false, 0b1111, [640, 0, 7, 56]))
            .unwrap();
        assert_eq!(
            build_batch(update, AggregatePhase::Active),
            Err(Error::AddressOverflow)
        );
    }

    #[test]
    fn first_deferred_cache_batch_ignores_unstaged_logical_fields() {
        let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture_with_state(
            0x30000,
            0x51,
            9,
            &[
                (1, vote(true, true, 0b0011, [7, 1, 0, 0])),
                (2, vote(true, false, 0b0011, [9, 2, 0, 0])),
            ],
            false,
            false,
        ))
        .unwrap();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0001, [8, 0, 0, 0]))
            .unwrap();

        let sleep = build_batch(plan, AggregatePhase::Sleep).unwrap();
        assert_eq!(sleep.len(), 1);
        assert_eq!(sleep.command(0).unwrap().register, 0);
        let wake = build_batch(plan, AggregatePhase::Wake).unwrap();
        assert_eq!(wake.len(), 2);
        assert_eq!(wake.command(0).unwrap().data, 9);
        assert_eq!(wake.command(1).unwrap().data, 2);
        let active = build_batch(plan, AggregatePhase::Active).unwrap();
        assert_eq!(active.len(), 2);
        assert_eq!(active.command(0).unwrap().data, 9);
        assert_eq!(active.command(1).unwrap().data, 2);
    }
}
