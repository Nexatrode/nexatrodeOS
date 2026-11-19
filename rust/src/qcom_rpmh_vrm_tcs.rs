//! Bounded Apps DRV2 active-TCS0 plans for VRM and source-derived bus batches.
//!
//! This converts exact command/address/data inputs into the source-matched
//! active-TCS0 register sequence. It performs no MMIO or IRQ handling and does
//! not establish controller ownership or report command completion. Deferred
//! sleep/wake phases remain owner-cache operations and are not planned here.

use crate::qcom_rpmh_handoff::{OBSERVED_CONFIG, Snapshot};
use crate::qcom_rpmh_vote_broker::AggregatePhase;
use crate::qcom_rpmh_vrm_command::{Batch, MAX_COMMANDS};

pub const ACTIVE_TCS: usize = 0;
/// The captured Sunfish DRV2 RSC has 16 command slots per TCS, matching the
/// pinned kernel's `MAX_RPMH_PAYLOAD`. VRM batches remain limited separately.
pub const MAX_TCS_COMMANDS: usize = 16;
pub const MAX_WRITES: usize = MAX_TCS_COMMANDS * 3 + 6;
pub const WRITE_MESSAGE_ID: u32 = 8 | (1 << 8) | (1 << 16);

const TCS_BASE: usize = 0xd00;
const WAIT_OFFSET: usize = 0x10;
const CONTROL_OFFSET: usize = 0x14;
const ENABLE_OFFSET: usize = 0x1c;
const COMMAND_STRIDE: usize = 20;
const MESSAGE_OFFSET: usize = 0x30;
const ADDRESS_OFFSET: usize = 0x34;
const DATA_OFFSET: usize = 0x38;
const MODE: u32 = 1 << 16;
const TRIGGER: u32 = 1 << 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Wait,
    Enable,
    Control,
    CommandMessage(u8),
    CommandAddress(u8),
    CommandData(u8),
}

impl Register {
    pub const fn offset(self) -> usize {
        match self {
            Self::Wait => TCS_BASE + WAIT_OFFSET,
            Self::Enable => TCS_BASE + ENABLE_OFFSET,
            Self::Control => TCS_BASE + CONTROL_OFFSET,
            Self::CommandMessage(slot) => {
                TCS_BASE + slot as usize * COMMAND_STRIDE + MESSAGE_OFFSET
            }
            Self::CommandAddress(slot) => {
                TCS_BASE + slot as usize * COMMAND_STRIDE + ADDRESS_OFFSET
            }
            Self::CommandData(slot) => TCS_BASE + slot as usize * COMMAND_STRIDE + DATA_OFFSET,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Write {
    register: Register,
    value: u32,
    expected_readback: Option<u32>,
}

impl Write {
    pub const fn register(self) -> Register {
        self.register
    }

    pub const fn value(self) -> u32 {
        self.value
    }

    /// `None` is reserved for the final hardware trigger, which must never be
    /// retried or read back as a way to infer whether it was accepted.
    pub const fn expected_readback(self) -> Option<u32> {
        self.expected_readback
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    identity: u64,
    generation: u64,
    ticket: u64,
    snapshot: Snapshot,
    command_count: usize,
    writes: [Option<Write>; MAX_WRITES],
    write_count: usize,
}

/// Internal command shape shared by the VRM and BCM plan constructors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TcsCommand {
    address: u32,
    data: u32,
}

impl Plan {
    pub const fn identity(self) -> u64 {
        self.identity
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn ticket(self) -> u64 {
        self.ticket
    }

    pub const fn snapshot(self) -> Snapshot {
        self.snapshot
    }

    pub const fn command_count(self) -> usize {
        self.command_count
    }

    pub const fn write_count(self) -> usize {
        self.write_count
    }

    pub fn write(&self, index: usize) -> Option<Write> {
        if index < self.write_count {
            self.writes[index]
        } else {
            None
        }
    }

    fn push(&mut self, register: Register, value: u32, verify: bool) {
        self.writes[self.write_count] = Some(Write {
            register,
            value,
            expected_readback: if verify { Some(value) } else { None },
        });
        self.write_count += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    DeferredPhase,
    InvalidIdentity,
    EmptyBatch,
    TooManyCommands,
    InvalidCommand,
    Geometry,
    IrqNotEnabled,
    Busy,
    UnknownControl,
}

/// Plan one checked active-TCS0 write from a stable all-bank snapshot.
///
/// The command message requests an accelerator response for the whole batch;
/// every command slot therefore has `WRITE_MESSAGE_ID`, `WAIT` remains zero,
/// and only the used low slots are enabled. The caller must still validate its
/// live lifecycle lease and recheck the snapshot before performing any write.
pub fn build_plan(snapshot: Snapshot, batch: Batch) -> Result<Plan, Error> {
    if batch.phase != AggregatePhase::Active {
        return Err(Error::DeferredPhase);
    }
    if batch.identity == 0 || batch.generation == 0 || batch.ticket == 0 {
        return Err(Error::InvalidIdentity);
    }
    if batch.is_empty() {
        return Err(Error::EmptyBatch);
    }
    if batch.len() > MAX_COMMANDS {
        return Err(Error::TooManyCommands);
    }
    let mut commands = [TcsCommand::default(); MAX_COMMANDS];
    for slot in 0..batch.len() {
        let command = batch.command(slot).ok_or(Error::InvalidCommand)?;
        if command.register as usize >= MAX_COMMANDS {
            return Err(Error::InvalidCommand);
        }
        let expected_address = batch
            .resource_address
            .checked_add(command.register as u32 * 4)
            .ok_or(Error::InvalidCommand)?;
        if command.address != expected_address
            || (0..slot).any(|previous| {
                batch
                    .command(previous)
                    .is_some_and(|other| other.address == command.address)
            })
        {
            return Err(Error::InvalidCommand);
        }
        commands[slot] = TcsCommand {
            address: command.address,
            data: command.data,
        };
    }

    build_raw_plan(
        snapshot,
        batch.identity,
        batch.generation,
        batch.ticket,
        &commands[..batch.len()],
    )
}

/// Plan a BCM active-TCS0 batch. Only slave-ID 5 BCM word addresses and valid
/// BCM packet words are accepted, keeping this path separate from VRM rails
/// and arbitrary RPMh resources. This creates no MMIO authority; execution
/// still requires the guarded controller lease and exact stable snapshot.
pub fn build_bcm_command_plan(
    snapshot: Snapshot,
    identity: u64,
    generation: u64,
    ticket: u64,
    commands: &[crate::qcom_bcm_vote::Encoded],
) -> Result<Plan, Error> {
    if commands.is_empty() {
        return Err(Error::EmptyBatch);
    }
    if commands.len() > MAX_TCS_COMMANDS {
        return Err(Error::TooManyCommands);
    }
    let mut tcs_commands = [TcsCommand::default(); MAX_TCS_COMMANDS];
    for (index, command) in commands.iter().enumerate() {
        let ends_clock_domain =
            index + 1 == commands.len() || commands[index + 1].clock_domain != command.clock_domain;
        if command.address == 0
            || command.address & 0xfff8_0000 != 0
            || (command.address >> 16) & 7 != 5
            || command.address & 3 != 0
            || command.clock_domain >= 16
            || (index > 0 && commands[index - 1].clock_domain > command.clock_domain)
            || command.complete != ends_clock_domain
            || command.data & !0x6fff_ffff != 0
            || command.complete != (command.data & (1 << 30) != 0)
            || commands[..index]
                .iter()
                .any(|previous| previous.address == command.address)
        {
            return Err(Error::InvalidCommand);
        }
        tcs_commands[index] = TcsCommand {
            address: command.address,
            data: command.data,
        };
    }
    build_raw_plan(
        snapshot,
        identity,
        generation,
        ticket,
        &tcs_commands[..commands.len()],
    )
}

/// Plan the internal exact address/data form after a typed caller has
/// validated the resource class. This helper is not exposed outside the
/// module; the VRM path calls it only after register/address validation.
fn build_raw_plan(
    snapshot: Snapshot,
    identity: u64,
    generation: u64,
    ticket: u64,
    commands: &[TcsCommand],
) -> Result<Plan, Error> {
    if identity == 0 || generation == 0 || ticket == 0 {
        return Err(Error::InvalidIdentity);
    }
    if commands.is_empty() {
        return Err(Error::EmptyBatch);
    }
    if commands.len() > MAX_TCS_COMMANDS {
        return Err(Error::TooManyCommands);
    }
    if snapshot.config != OBSERVED_CONFIG {
        return Err(Error::Geometry);
    }
    if snapshot.irq_status != 0 {
        return Err(Error::Busy);
    }
    // Linux enables the active-TCS IRQ mask at controller setup/resume, not
    // per transaction. Require TCS0's bit and leave the shared mask untouched.
    if snapshot.irq_enable & 1 == 0 {
        return Err(Error::IrqNotEnabled);
    }
    for bank in &snapshot.banks[..2] {
        if bank.idle != 1 || bank.enable != 0 || bank.wait != 0 {
            return Err(Error::Busy);
        }
        if bank.control & !(MODE | TRIGGER) != 0 {
            return Err(Error::UnknownControl);
        }
    }

    let mut plan = Plan {
        identity,
        generation,
        ticket,
        snapshot,
        command_count: commands.len(),
        writes: [None; MAX_WRITES],
        write_count: 0,
    };
    for (slot, command) in commands.iter().enumerate() {
        // RPMh command addresses are `slv_id:18:16 | offset:15:0`; reject
        // reserved high bits, the null address, and duplicate resources.
        if command.address == 0
            || command.address & 0xfff8_0000 != 0
            || commands[..slot]
                .iter()
                .any(|previous| previous.address == command.address)
        {
            return Err(Error::InvalidCommand);
        }
        let command_slot = slot as u8;
        plan.push(
            Register::CommandMessage(command_slot),
            WRITE_MESSAGE_ID,
            true,
        );
        plan.push(
            Register::CommandAddress(command_slot),
            command.address,
            true,
        );
        plan.push(Register::CommandData(command_slot), command.data, true);
    }

    let enable_mask = (1u32 << commands.len()) - 1;
    plan.push(Register::Wait, 0, true);
    plan.push(Register::Enable, enable_mask, true);

    let control = snapshot.banks[ACTIVE_TCS].control;
    let without_trigger = control & !TRIGGER;
    plan.push(Register::Control, without_trigger, true);
    plan.push(Register::Control, without_trigger & !MODE, true);
    plan.push(Register::Control, MODE, true);
    plan.push(Register::Control, MODE | TRIGGER, false);
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_handoff::Bank;
    use crate::qcom_rpmh_vote_aggregate::ClientVote;
    use crate::qcom_rpmh_vote_broker::{Broker, CompleteVoteSet};
    use crate::qcom_rpmh_vrm_command::build_batch;

    fn active_batch() -> Batch {
        let current = ClientVote {
            set_active: true,
            set_sleep: false,
            valid_mask: 0,
            registers: [0; 4],
        };
        let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture(
            0x30000,
            0x51,
            9,
            &[(1, current)],
        ))
        .unwrap();
        let plan = broker
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
        build_batch(plan, AggregatePhase::Active).unwrap()
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

    #[test]
    fn active_batch_maps_exact_slots_then_checked_trigger() {
        let plan = build_plan(idle_snapshot(), active_batch()).unwrap();
        assert_eq!(plan.identity(), 0x51);
        assert_eq!(plan.generation(), 9);
        assert_eq!(plan.ticket(), 1);
        assert_eq!(plan.command_count(), 2);
        assert_eq!(plan.write_count(), 12);
        assert_eq!(
            plan.write(0),
            Some(Write {
                register: Register::CommandMessage(0),
                value: WRITE_MESSAGE_ID,
                expected_readback: Some(WRITE_MESSAGE_ID),
            })
        );
        assert_eq!(WRITE_MESSAGE_ID, 0x0001_0108);
        assert_eq!(
            plan.write(1).unwrap().register(),
            Register::CommandAddress(0)
        );
        assert_eq!(plan.write(1).unwrap().value(), 0x30000);
        assert_eq!(plan.write(2).unwrap().register(), Register::CommandData(0));
        assert_eq!(plan.write(2).unwrap().value(), 640);
        assert_eq!(
            plan.write(3).unwrap().register(),
            Register::CommandMessage(1)
        );
        assert_eq!(
            plan.write(4).unwrap().register(),
            Register::CommandAddress(1)
        );
        assert_eq!(plan.write(4).unwrap().value(), 0x30004);
        assert_eq!(plan.write(5).unwrap().register(), Register::CommandData(1));
        assert_eq!(plan.write(5).unwrap().value(), 1);
        assert_eq!(plan.write(6).unwrap().register(), Register::Wait);
        assert_eq!(plan.write(6).unwrap().value(), 0);
        assert_eq!(plan.write(7).unwrap().register(), Register::Enable);
        assert_eq!(plan.write(7).unwrap().value(), 0b11);
        assert_eq!(plan.write(8).unwrap().value(), 0);
        assert_eq!(plan.write(9).unwrap().value(), 0);
        assert_eq!(plan.write(10).unwrap().value(), MODE);
        assert_eq!(plan.write(11).unwrap().value(), MODE | TRIGGER);
        assert_eq!(plan.write(11).unwrap().expected_readback(), None);
        assert_eq!(plan.write_count(), 12);
        assert_eq!(plan.write(12), None);
        assert_eq!(Register::Wait.offset(), 0xd10);
        assert_eq!(Register::Control.offset(), 0xd14);
        assert_eq!(Register::Enable.offset(), 0xd1c);
        assert_eq!(Register::CommandMessage(1).offset(), 0xd44);
        assert_eq!(Register::CommandAddress(1).offset(), 0xd48);
        assert_eq!(Register::CommandData(1).offset(), 0xd4c);
    }

    #[test]
    fn bcm_plan_maps_all_sixteen_source_supported_slots() {
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
        let plan = build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands).unwrap();

        assert_eq!(plan.command_count(), 16);
        assert_eq!(plan.write_count(), MAX_WRITES);
        assert_eq!(
            plan.write(0).unwrap().register(),
            Register::CommandMessage(0)
        );
        assert_eq!(plan.write(1).unwrap().value(), 0x50000);
        assert_eq!(plan.write(2).unwrap().value(), 0x2000_0000);
        let last_slot = 15 * 3;
        assert_eq!(
            plan.write(last_slot).unwrap().register(),
            Register::CommandMessage(15)
        );
        assert_eq!(plan.write(last_slot + 1).unwrap().value(), 0x5003c);
        assert_eq!(plan.write(last_slot + 2).unwrap().value(), 0x6000_000f);
        assert_eq!(plan.write(48).unwrap().register(), Register::Wait);
        assert_eq!(plan.write(49).unwrap().register(), Register::Enable);
        assert_eq!(plan.write(51).unwrap().register(), Register::Control);
        assert_eq!(plan.write(53).unwrap().value(), MODE | TRIGGER);
        assert_eq!(plan.write(53).unwrap().expected_readback(), None);
        assert_eq!(Register::CommandMessage(15).offset(), 0xe5c);
        assert_eq!(Register::CommandAddress(15).offset(), 0xe60);
        assert_eq!(Register::CommandData(15).offset(), 0xe64);
    }

    #[test]
    fn source_bcm_phase_word_composes_into_the_tcs_plan() {
        use crate::qcom_bcm::Metadata;
        use crate::qcom_bcm_vote::{BcmRequest, RpmhRequestState, Vector, Vectors, plan_tcs};

        let source_plan = plan_tcs(
            [BcmRequest {
                metadata: Metadata {
                    id: *b"MC0\0\0\0\0\0",
                    address: 0x50000,
                    unit_size: 1,
                    width: 1,
                    clock_domain: 0,
                    reserved: 0,
                    auxiliary_version: 0x1000,
                },
                vectors: Vectors {
                    active: Vector { x: 1, y: 2 },
                    sleep: Vector::default(),
                },
            }],
            false,
            RpmhRequestState::ActiveOnly,
        )
        .unwrap();
        let encoded = source_plan.active.commands[0];
        assert_eq!(encoded.address, 0x50000);
        assert_eq!(encoded.data, 0x6000_4002);
        let controller_plan =
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 4, &[encoded]).unwrap();
        assert_eq!(controller_plan.command_count(), 1);
        assert_eq!(controller_plan.write(2).unwrap().value(), encoded.data);
    }

    #[test]
    fn bcm_plan_rejects_oversize_invalid_and_duplicate_commands() {
        let mut commands: [crate::qcom_bcm_vote::Encoded; MAX_TCS_COMMANDS + 1] =
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
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands),
            Err(Error::TooManyCommands)
        );

        commands[0].address |= 0x8000_0000;
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands[..MAX_TCS_COMMANDS]),
            Err(Error::InvalidCommand)
        );
        commands[0].address = 0x40000;
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands[..MAX_TCS_COMMANDS]),
            Err(Error::InvalidCommand)
        );
        commands[0].address = 0x50002;
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands[..MAX_TCS_COMMANDS]),
            Err(Error::InvalidCommand)
        );
        commands[0].address = commands[1].address;
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands[..MAX_TCS_COMMANDS]),
            Err(Error::InvalidCommand)
        );
        commands[0].address = 0x50000;
        commands[0].data |= 1 << 31;
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &commands[..MAX_TCS_COMMANDS]),
            Err(Error::InvalidCommand)
        );
    }

    #[test]
    fn bcm_plan_requires_ordered_vcd_groups_and_one_terminal_commit_each() {
        let first = crate::qcom_bcm_vote::Encoded {
            address: 0x50000,
            data: 0x2000_0001,
            clock_domain: 0,
            complete: false,
            saturated: false,
        };
        let second = crate::qcom_bcm_vote::Encoded {
            address: 0x50004,
            data: 0x6000_0002,
            clock_domain: 0,
            complete: true,
            saturated: false,
        };
        let third = crate::qcom_bcm_vote::Encoded {
            address: 0x50008,
            data: 0x6000_0003,
            clock_domain: 3,
            complete: true,
            saturated: false,
        };
        assert!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &[first, second, third]).is_ok()
        );

        let premature_commit = crate::qcom_bcm_vote::Encoded {
            data: 0x6000_0001,
            complete: true,
            ..first
        };
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &[premature_commit, second]),
            Err(Error::InvalidCommand)
        );

        let missing_final_commit = crate::qcom_bcm_vote::Encoded {
            data: 0x2000_0003,
            complete: false,
            ..third
        };
        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &[third, missing_final_commit]),
            Err(Error::InvalidCommand)
        );

        assert_eq!(
            build_bcm_command_plan(idle_snapshot(), 0x51, 9, 2, &[third, first]),
            Err(Error::InvalidCommand)
        );
    }

    #[test]
    fn deferred_phases_and_invalid_controller_snapshots_are_refused() {
        let active = active_batch();
        let mut sleep_plan = active;
        sleep_plan.phase = AggregatePhase::Sleep;
        assert_eq!(
            build_plan(idle_snapshot(), sleep_plan),
            Err(Error::DeferredPhase)
        );

        let mut invalid = idle_snapshot();
        invalid.config ^= 0x1000;
        assert_eq!(build_plan(invalid, active), Err(Error::Geometry));
        invalid = idle_snapshot();
        invalid.irq_status = 1;
        assert_eq!(build_plan(invalid, active), Err(Error::Busy));
        invalid = idle_snapshot();
        invalid.irq_enable = 0x20;
        assert_eq!(build_plan(invalid, active), Err(Error::IrqNotEnabled));
        invalid = idle_snapshot();
        invalid.banks[0].enable = 1;
        assert_eq!(build_plan(invalid, active), Err(Error::Busy));
        invalid = idle_snapshot();
        invalid.banks[1].idle = 0;
        assert_eq!(build_plan(invalid, active), Err(Error::Busy));
        invalid = idle_snapshot();
        invalid.banks[0].control = 1 << 15;
        assert_eq!(build_plan(invalid, active), Err(Error::UnknownControl));
    }
}
