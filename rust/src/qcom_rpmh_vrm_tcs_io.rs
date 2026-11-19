//! Bounded active-TCS0 transaction and IRQ-completion state machine.
//!
//! This consumes only a plan produced by `qcom_rpmh_vrm_tcs`. The caller must
//! provide a fresh stable snapshot and an IO implementation that validates
//! the live owner lease before every access. No raw MMIO implementation is
//! provided here. Ambiguous failures are terminal to the broker; this module
//! never retries a register write or the final trigger.

use crate::qcom_rpmh_handoff::{BankField, Register as SnapshotRegister, Snapshot};
use crate::qcom_rpmh_vrm_tcs::{Plan, Register, Write};

const TCS_BASE: usize = 0xd00;
const IRQ_STATUS: usize = TCS_BASE + 0x04;
const IRQ_CLEAR: usize = TCS_BASE + 0x08;
const COMMAND_STATUS: usize = TCS_BASE + 0x3c;
const COMMAND_STRIDE: usize = 20;
const IRQ_BIT: u32 = 1;
const STATUS_ISSUED: u32 = 1 << 8;
const STATUS_COMPLETE: u32 = 1 << 16;
const STATUS_REQUIRED: u32 = STATUS_ISSUED | STATUS_COMPLETE;
pub const MAX_ATTEMPTS: u8 = 8;

/// Error from one lease-bound register access.
#[derive(Debug, Eq, PartialEq)]
pub enum AccessError<E> {
    LeaseLost,
    Io(E),
}

/// Every implementation must validate the supplied lease, identity, and
/// generation in the same non-preemptible owner critical section as each
/// read/write. Keeping the authority arguments on the access methods prevents
/// callers from splitting validation from MMIO. A native adapter must also
/// enforce the uncached mapping and exact register allowlist before volatile
/// access.
pub trait GuardedRegisters<L> {
    type Error;

    fn validate(&mut self, lease: &L, identity: u64, generation: u64) -> bool;
    fn read32(
        &mut self,
        lease: &L,
        identity: u64,
        generation: u64,
        offset: usize,
    ) -> Result<u32, AccessError<Self::Error>>;
    fn write32(
        &mut self,
        lease: &L,
        identity: u64,
        generation: u64,
        offset: usize,
        value: u32,
    ) -> Result<(), AccessError<Self::Error>>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error<E> {
    InvalidBudget,
    StaleSnapshot,
    InvalidPlan,
    LeaseLost,
    Io(E),
    ReadbackMismatch {
        offset: usize,
        expected: u32,
        observed: u32,
    },
    IrqTimeout,
    UnexpectedIrq(u32),
    ControllerBusy(u32),
    CommandIncomplete {
        slot: u8,
        status: u32,
    },
    IrqNotCleared,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    identity: u64,
    generation: u64,
    ticket: u64,
    command_count: usize,
}

impl Completion {
    pub const fn identity(self) -> u64 {
        self.identity
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn ticket(self) -> u64 {
        self.ticket
    }

    pub const fn command_count(self) -> usize {
        self.command_count
    }
}

fn read<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
    offset: usize,
) -> Result<u32, Error<E>> {
    io.read32(lease, plan.identity(), plan.generation(), offset)
        .map_err(map_access_error)
}

fn write<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
    offset: usize,
    value: u32,
) -> Result<(), Error<E>> {
    io.write32(lease, plan.identity(), plan.generation(), offset, value)
        .map_err(map_access_error)
}

fn map_access_error<E>(error: AccessError<E>) -> Error<E> {
    match error {
        AccessError::LeaseLost => Error::LeaseLost,
        AccessError::Io(error) => Error::Io(error),
    }
}

fn write_checked<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
    register: Register,
    value: u32,
    attempts: u8,
) -> Result<(), Error<E>> {
    let offset = register.offset();
    write(io, lease, plan, offset, value)?;
    let mut observed = 0;
    for _ in 0..attempts {
        observed = read(io, lease, plan, offset)?;
        if observed == value {
            return Ok(());
        }
    }
    Err(Error::ReadbackMismatch {
        offset,
        expected: value,
        observed,
    })
}

fn wait_irq<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
    attempts: u8,
) -> Result<(), Error<E>> {
    for _ in 0..attempts {
        let status = read(io, lease, plan, IRQ_STATUS)?;
        if status & !IRQ_BIT != 0 {
            return Err(Error::UnexpectedIrq(status));
        }
        if status & IRQ_BIT != 0 {
            return Ok(());
        }
    }
    Err(Error::IrqTimeout)
}

fn wait_idle<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
    attempts: u8,
) -> Result<(), Error<E>> {
    let offset = SnapshotRegister::bank(0, BankField::Idle).unwrap().offset();
    let mut observed = 0;
    for _ in 0..attempts {
        observed = read(io, lease, plan, offset)?;
        if observed != 0 {
            return Ok(());
        }
    }
    Err(Error::ControllerBusy(observed))
}

fn verify_command_status<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
) -> Result<(), Error<E>> {
    for slot in 0..plan.command_count() {
        let offset = COMMAND_STATUS + slot * COMMAND_STRIDE;
        let status = read(io, lease, plan, offset)?;
        if status & STATUS_REQUIRED != STATUS_REQUIRED {
            return Err(Error::CommandIncomplete {
                slot: slot as u8,
                status,
            });
        }
    }
    Ok(())
}

fn cleanup<L, E>(
    io: &mut impl GuardedRegisters<L, Error = E>,
    lease: &L,
    plan: Plan,
    attempts: u8,
) -> Result<(), Error<E>> {
    write_checked(io, lease, plan, Register::Enable, 0, attempts)?;
    write_checked(io, lease, plan, Register::Wait, 0, attempts)?;
    // IRQ_CLEAR is W1C: write the observed TCS0 bit exactly once and never
    // read the clear register itself.
    write(io, lease, plan, IRQ_CLEAR, IRQ_BIT)?;
    for _ in 0..attempts {
        let status = read(io, lease, plan, IRQ_STATUS)?;
        if status & !IRQ_BIT != 0 {
            return Err(Error::UnexpectedIrq(status));
        }
        if status & IRQ_BIT == 0 {
            return Ok(());
        }
    }
    Err(Error::IrqNotCleared)
}

/// Execute one planned active-TCS0 request with bounded readback and IRQ work.
///
/// The supplied snapshot must be freshly captured under the same live lease;
/// it must equal the exact snapshot used to build the plan. Each staged write
/// is issued once and checked with a bounded number of reads. The final trigger
/// is issued once with no readback. A successful completion requires the IRQ,
/// idle status, issued+complete bits for every command, and exact cleanup.
pub fn execute<L, E>(
    plan: Plan,
    fresh_snapshot: Snapshot,
    lease: &L,
    io: &mut impl GuardedRegisters<L, Error = E>,
    readback_attempts: u8,
    irq_attempts: u8,
) -> Result<Completion, Error<E>> {
    if readback_attempts == 0
        || readback_attempts > MAX_ATTEMPTS
        || irq_attempts == 0
        || irq_attempts > MAX_ATTEMPTS
    {
        return Err(Error::InvalidBudget);
    }
    if fresh_snapshot != plan.snapshot() {
        return Err(Error::StaleSnapshot);
    }
    if plan.command_count() == 0
        || plan.command_count() > crate::qcom_rpmh_vrm_tcs::MAX_TCS_COMMANDS
        || plan.write_count() < 1
    {
        return Err(Error::InvalidPlan);
    }

    let mut trigger_seen = false;
    for index in 0..plan.write_count() {
        let item: Write = plan.write(index).ok_or(Error::InvalidPlan)?;
        let offset = item.register().offset();
        if let Some(expected) = item.expected_readback() {
            if trigger_seen {
                return Err(Error::InvalidPlan);
            }
            if expected != item.value() {
                return Err(Error::InvalidPlan);
            }
            write_checked(
                io,
                lease,
                plan,
                item.register(),
                item.value(),
                readback_attempts,
            )?;
        } else {
            if trigger_seen || index + 1 != plan.write_count() {
                return Err(Error::InvalidPlan);
            }
            write(io, lease, plan, offset, item.value())?;
            trigger_seen = true;
        }
    }
    if !trigger_seen {
        return Err(Error::InvalidPlan);
    }

    wait_irq(io, lease, plan, irq_attempts)?;
    wait_idle(io, lease, plan, irq_attempts)?;
    let command_status = verify_command_status(io, lease, plan);
    cleanup(io, lease, plan, readback_attempts)?;
    command_status?;

    Ok(Completion {
        identity: plan.identity(),
        generation: plan.generation(),
        ticket: plan.ticket(),
        command_count: plan.command_count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_handoff::{Bank, OBSERVED_CONFIG, TCS_COUNT};
    use crate::qcom_rpmh_vote_aggregate::ClientVote;
    use crate::qcom_rpmh_vote_broker::{
        AggregatePhase, Broker, Commit, CompleteVoteSet, ExactPhaseTransport, UpdatePlan,
    };
    use crate::qcom_rpmh_vrm_command::{Batch, build_batch};
    use crate::qcom_rpmh_vrm_tcs::{MAX_TCS_COMMANDS, build_bcm_command_plan, build_plan};
    use crate::qcom_rpmh_vrm_tcs_transport::{Backend, DeferredCache, RpmhTcsTransport};
    use std::vec::Vec;

    const MODE: u32 = 1 << 16;
    const TRIGGER: u32 = 1 << 24;
    const COMMAND_ENABLE: usize = TCS_BASE + 0x1c;
    const COMMAND_WAIT: usize = TCS_BASE + 0x10;
    const CONTROL: usize = TCS_BASE + 0x14;
    const STATUS: usize = TCS_BASE + 0x18;

    struct Fake {
        words: [u32; 0x3000 / 4],
        snapshot: Snapshot,
        writes: Vec<(usize, u32)>,
        validations: Vec<(u64, u64)>,
        valid: bool,
        lose_on_validation: Option<usize>,
        lose_after_trigger: bool,
        fail_write_at: Option<usize>,
        mismatch_offset: Option<usize>,
        complete_on_trigger: bool,
        incomplete_slot: Option<usize>,
        trigger_writes: usize,
    }

    impl Fake {
        fn from_snapshot(snapshot: Snapshot, commands: usize) -> Self {
            let mut fake = Self {
                words: [0; 0x3000 / 4],
                snapshot,
                writes: Vec::new(),
                validations: Vec::new(),
                valid: true,
                lose_on_validation: None,
                lose_after_trigger: false,
                fail_write_at: None,
                mismatch_offset: None,
                complete_on_trigger: true,
                incomplete_slot: None,
                trigger_writes: 0,
            };
            fake.words[0x0c / 4] = snapshot.config;
            fake.words[IRQ_STATUS / 4] = snapshot.irq_status;
            fake.words[TCS_BASE / 4] = snapshot.irq_enable;
            for bank in 0..TCS_COUNT {
                let base = TCS_BASE + bank * 0x2a0;
                let view = snapshot.banks[bank];
                fake.words[(base + 0x10) / 4] = view.wait;
                fake.words[(base + 0x14) / 4] = view.control;
                fake.words[(base + 0x18) / 4] = view.idle;
                fake.words[(base + 0x1c) / 4] = view.enable;
                for slot in 0..16 {
                    if let Some(command) = view.commands[slot] {
                        let command_base = base + slot * COMMAND_STRIDE;
                        fake.words[(command_base + 0x30) / 4] = command.message;
                        fake.words[(command_base + 0x34) / 4] = command.address;
                        fake.words[(command_base + 0x38) / 4] = command.data;
                    }
                }
            }
            if commands != 0 {
                fake.words[STATUS / 4] = 1;
            }
            fake
        }

        fn active_update() -> (Broker, UpdatePlan) {
            let initial_vote = ClientVote {
                set_active: true,
                set_sleep: false,
                valid_mask: 0,
                registers: [0; 4],
            };
            let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture(
                0x30000,
                0x51,
                9,
                &[(1, initial_vote)],
            ))
            .unwrap();
            let update = broker
                .begin_update(
                    1,
                    ClientVote {
                        set_active: true,
                        set_sleep: false,
                        valid_mask: 0b0011,
                        registers: [640, 1, 0, 0],
                    },
                )
                .unwrap();
            (broker, update)
        }

        fn active_batch() -> Batch {
            let (_broker, update) = Self::active_update();
            build_batch(update, AggregatePhase::Active).unwrap()
        }

        fn idle_snapshot() -> Snapshot {
            let mut snapshot = Snapshot {
                config: OBSERVED_CONFIG,
                irq_enable: 0x21,
                ..Snapshot::default()
            };
            snapshot.banks[0] = Bank {
                idle: 1,
                ..Bank::default()
            };
            snapshot.banks[1] = Bank {
                idle: 1,
                ..Bank::default()
            };
            snapshot
        }

        fn standard() -> (Plan, Snapshot, Self) {
            let batch = Self::active_batch();
            let snapshot = Self::idle_snapshot();
            let plan = build_plan(snapshot, batch).unwrap();
            let fake = Self::from_snapshot(snapshot, plan.command_count());
            (plan, snapshot, fake)
        }

        fn standard_batch() -> (Batch, Snapshot, Self) {
            let batch = Self::active_batch();
            let snapshot = Self::idle_snapshot();
            let fake = Self::from_snapshot(snapshot, batch.len());
            (batch, snapshot, fake)
        }

        fn word(&self, offset: usize) -> u32 {
            self.words[offset / 4]
        }

        fn trigger_count(&self) -> usize {
            self.trigger_writes
        }
    }

    impl GuardedRegisters<()> for Fake {
        type Error = &'static str;

        fn validate(&mut self, _lease: &(), identity: u64, generation: u64) -> bool {
            self.validations.push((identity, generation));
            if self.lose_on_validation == Some(self.validations.len()) {
                self.valid = false;
            }
            self.valid && !(self.lose_after_trigger && self.trigger_writes != 0)
        }

        fn read32(
            &mut self,
            lease: &(),
            identity: u64,
            generation: u64,
            offset: usize,
        ) -> Result<u32, AccessError<Self::Error>> {
            if !self.validate(lease, identity, generation) {
                return Err(AccessError::LeaseLost);
            }
            if offset / 4 >= self.words.len() {
                return Err(AccessError::Io("read outside mapping"));
            }
            let value = self.words[offset / 4];
            if self.mismatch_offset == Some(offset) {
                Ok(value ^ 1)
            } else {
                Ok(value)
            }
        }

        fn write32(
            &mut self,
            lease: &(),
            identity: u64,
            generation: u64,
            offset: usize,
            value: u32,
        ) -> Result<(), AccessError<Self::Error>> {
            if !self.validate(lease, identity, generation) {
                return Err(AccessError::LeaseLost);
            }
            let index = self.writes.len();
            self.writes.push((offset, value));
            if self.fail_write_at == Some(index) {
                return Err(AccessError::Io("write failed"));
            }
            if offset == CONTROL && value == MODE | TRIGGER {
                self.trigger_writes += 1;
                self.words[offset / 4] = value;
                if self.complete_on_trigger {
                    self.words[IRQ_STATUS / 4] |= IRQ_BIT;
                    for slot in 0..crate::qcom_rpmh_vrm_tcs::MAX_TCS_COMMANDS {
                        let status = if self.incomplete_slot == Some(slot) {
                            STATUS_ISSUED
                        } else {
                            STATUS_REQUIRED
                        };
                        self.words[(COMMAND_STATUS + slot * COMMAND_STRIDE) / 4] = status;
                    }
                }
                return Ok(());
            }
            if offset == IRQ_CLEAR && value == IRQ_BIT {
                self.words[IRQ_STATUS / 4] &= !IRQ_BIT;
                return Ok(());
            }
            if offset < 0x3000 {
                self.words[offset / 4] = value;
                Ok(())
            } else {
                Err(AccessError::Io("write outside mapping"))
            }
        }
    }

    impl Backend<()> for Fake {
        fn capture_stable(
            &mut self,
            lease: &(),
            identity: u64,
            generation: u64,
        ) -> Result<Snapshot, Self::Error> {
            if self.validate(lease, identity, generation) {
                Ok(self.snapshot)
            } else {
                Err("lease lost during snapshot")
            }
        }
    }

    #[derive(Default)]
    struct TestCache {
        staged: Vec<(u64, AggregatePhase, u64)>,
    }

    impl DeferredCache<()> for TestCache {
        type Error = &'static str;

        fn stage(&mut self, lease: &(), batch: Batch) -> Result<(), Self::Error> {
            if *lease != () {
                return Err("wrong lease");
            }
            self.staged
                .push((batch.identity, batch.phase, batch.ticket));
            Ok(())
        }
    }

    #[test]
    fn successful_transaction_triggers_once_checks_all_commands_and_cleans_up() {
        let (plan, snapshot, mut fake) = Fake::standard();
        let completion = execute(plan, snapshot, &(), &mut fake, 3, 4).unwrap();
        assert_eq!(completion.identity(), 0x51);
        assert_eq!(completion.generation(), 9);
        assert_eq!(completion.ticket(), 1);
        assert_eq!(completion.command_count(), 2);
        assert_eq!(fake.trigger_count(), 1);
        assert_eq!(fake.word(COMMAND_ENABLE), 0);
        assert_eq!(fake.word(COMMAND_WAIT), 0);
        assert_eq!(fake.word(IRQ_STATUS), 0);
        assert_eq!(fake.writes.last(), Some(&(IRQ_CLEAR, IRQ_BIT)));
        assert!(fake.validations.iter().all(|pair| *pair == (0x51, 9)));
    }

    #[test]
    fn bcm_executor_completes_and_cleans_all_sixteen_tcs_slots() {
        let commands: [crate::qcom_bcm_vote::Encoded; MAX_TCS_COMMANDS] =
            core::array::from_fn(|slot| {
                let complete = slot + 1 == MAX_TCS_COMMANDS;
                crate::qcom_bcm_vote::Encoded {
                    address: 0x50000 + slot as u32 * 4,
                    data: 0x2000_0000 | (u32::from(complete) << 30) | slot as u32,
                    clock_domain: 0,
                    complete,
                    saturated: false,
                }
            });
        let snapshot = Fake::idle_snapshot();
        let plan = build_bcm_command_plan(snapshot, 0x51, 9, 3, &commands).unwrap();
        let mut fake = Fake::from_snapshot(snapshot, MAX_TCS_COMMANDS);

        let completion = execute(plan, snapshot, &(), &mut fake, 3, 4).unwrap();

        assert_eq!(completion.command_count(), MAX_TCS_COMMANDS);
        assert_eq!(fake.trigger_count(), 1);
        assert_eq!(fake.word(COMMAND_ENABLE), 0);
        assert_eq!(fake.word(COMMAND_WAIT), 0);
        assert_eq!(fake.word(IRQ_STATUS), 0);
        for slot in 0..MAX_TCS_COMMANDS {
            assert_eq!(
                fake.word(Register::CommandAddress(slot as u8).offset()),
                0x50000 + slot as u32 * 4
            );
            assert_eq!(
                fake.word(Register::CommandData(slot as u8).offset()),
                0x2000_0000 | (u32::from(slot + 1 == MAX_TCS_COMMANDS) << 30) | slot as u32
            );
        }
    }

    #[test]
    fn stale_snapshot_and_invalid_budgets_perform_no_io() {
        let (plan, snapshot, mut fake) = Fake::standard();
        let mut stale = snapshot;
        stale.irq_enable ^= 1;
        assert_eq!(
            execute(plan, stale, &(), &mut fake, 3, 4),
            Err(Error::StaleSnapshot)
        );
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 0, 4),
            Err(Error::InvalidBudget)
        );
        assert!(fake.writes.is_empty());
        assert!(fake.validations.is_empty());
    }

    #[test]
    fn readback_failure_stops_before_trigger_without_retrying_a_write() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.mismatch_offset = Some(Register::CommandMessage(0).offset());
        assert!(matches!(
            execute(plan, snapshot, &(), &mut fake, 2, 3),
            Err(Error::ReadbackMismatch { .. })
        ));
        assert_eq!(fake.trigger_count(), 0);
        assert_eq!(
            fake.writes
                .iter()
                .filter(|(offset, _)| *offset == Register::CommandMessage(0).offset())
                .count(),
            1
        );
    }

    #[test]
    fn trigger_failure_after_effect_is_not_retried_or_followed_by_cleanup() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.lose_after_trigger = true;
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 4),
            Err(Error::LeaseLost)
        );
        assert_eq!(fake.trigger_count(), 1);
        assert_ne!(fake.word(IRQ_STATUS) & IRQ_BIT, 0);
        assert!(!fake.writes.iter().any(|(offset, _)| *offset == IRQ_CLEAR));
    }

    #[test]
    fn ambiguous_trigger_io_error_is_not_replayed() {
        let (plan, snapshot, mut fake) = Fake::standard();
        let trigger_index = plan.write_count() - 1;
        fake.fail_write_at = Some(trigger_index);
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 4),
            Err(Error::Io("write failed"))
        );
        assert_eq!(fake.trigger_count(), 0);
        assert_eq!(
            fake.writes
                .iter()
                .filter(|(offset, value)| *offset == CONTROL && *value == MODE | TRIGGER)
                .count(),
            1
        );
    }

    #[test]
    fn missing_completion_times_out_without_cleanup_or_replay() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.complete_on_trigger = false;
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 2),
            Err(Error::IrqTimeout)
        );
        assert_eq!(fake.trigger_count(), 1);
        assert!(!fake.writes.iter().any(|(offset, _)| *offset == IRQ_CLEAR));
    }

    #[test]
    fn other_tcs_irq_is_never_acknowledged_as_this_request() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.words[IRQ_STATUS / 4] = 1 << 1;
        fake.complete_on_trigger = false;
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 2),
            Err(Error::UnexpectedIrq(2))
        );
        assert_eq!(fake.trigger_count(), 1);
        assert!(!fake.writes.iter().any(|(offset, _)| *offset == IRQ_CLEAR));
    }

    #[test]
    fn incomplete_command_status_is_reported_after_bounded_cleanup() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.complete_on_trigger = true;
        fake.incomplete_slot = Some(0);
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 3),
            Err(Error::CommandIncomplete {
                slot: 0,
                status: STATUS_ISSUED,
            })
        );
        assert_eq!(fake.word(COMMAND_ENABLE), 0);
        assert_eq!(fake.word(COMMAND_WAIT), 0);
        assert_eq!(fake.word(IRQ_STATUS), 0);
        assert_eq!(fake.trigger_count(), 1);
    }

    #[test]
    fn lost_lease_stops_all_access_and_never_cleans_up_by_guessing() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.valid = false;
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 3),
            Err(Error::LeaseLost)
        );
        assert!(fake.writes.is_empty());
        assert!(fake.validations.iter().all(|pair| *pair == (0x51, 9)));
    }

    #[test]
    fn lease_lost_between_staged_write_and_readback_stops_before_later_io() {
        let (plan, snapshot, mut fake) = Fake::standard();
        fake.lose_on_validation = Some(2);
        assert_eq!(
            execute(plan, snapshot, &(), &mut fake, 3, 3),
            Err(Error::LeaseLost)
        );
        assert_eq!(fake.writes.len(), 1);
        assert_eq!(fake.trigger_count(), 0);
    }

    #[test]
    fn broker_active_phase_commits_only_after_tcs_completion() {
        let (mut broker, update) = Fake::active_update();
        let (batch, _snapshot, backend) = Fake::standard_batch();
        assert_eq!(batch.phase, AggregatePhase::Active);
        let mut transport = RpmhTcsTransport::new(TestCache::default(), backend, 3, 4).unwrap();
        assert_eq!(
            broker.execute(update, &(), &mut transport),
            Ok(Commit::Committed)
        );
        let (cache, backend) = transport.into_parts();
        assert!(cache.staged.is_empty());
        assert_eq!(backend.trigger_count(), 1);
        assert_eq!(backend.word(IRQ_STATUS), 0);
        assert!(backend.validations.iter().all(|pair| *pair == (0x51, 9)));
    }

    #[test]
    fn deferred_phases_stage_in_cache_without_any_tcs_access() {
        let (mut batch, snapshot, backend) = Fake::standard_batch();
        let mut transport = RpmhTcsTransport::new(TestCache::default(), backend, 3, 4).unwrap();
        for phase in [AggregatePhase::Sleep, AggregatePhase::Wake] {
            batch.phase = phase;
            ExactPhaseTransport::accept_exact_phase(&mut transport, &(), batch).unwrap();
        }
        let (cache, backend) = transport.into_parts();
        assert_eq!(cache.staged.len(), 2);
        assert_eq!(cache.staged[0], (0x51, AggregatePhase::Sleep, batch.ticket));
        assert_eq!(cache.staged[1], (0x51, AggregatePhase::Wake, batch.ticket));
        assert!(backend.writes.is_empty());
        assert!(backend.validations.is_empty());
        assert_eq!(snapshot, backend.snapshot);
    }
}
