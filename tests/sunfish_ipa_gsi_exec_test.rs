#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/sunfish_ipa_gsi_exec.rs"]
mod sunfish_ipa_gsi_exec;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;

use std::collections::BTreeMap;
use sunfish_ipa_gsi_exec::{
    complete_next_ap_channel_allocation, complete_next_ap_channel_start,
    complete_next_ap_event_ring_allocation, execute_ap_context, issue_next_ap_channel_allocation,
    issue_next_ap_channel_start, issue_next_ap_event_ring_allocation, prime_next_ap_event_ring,
    read_ap_gsi_channel_snapshot, ApGsiChannelSnapshotError, ChannelAllocationCommandError,
    ChannelAllocationControlError, ChannelControlError, ChannelControlIo, ChannelStartCommandError,
    EventControlError, EventControlIo, EventRingCommandError, EventRingPrimeError, ExecuteError,
    ExecuteReceipt, RegisterIo, SUNFISH_GSI_AP_STATUS_ENABLED_BIT, SUNFISH_GSI_AP_STATUS_OFFSET,
    SUNFISH_GSI_CHANNEL_COMMAND_OFFSET, SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET,
    SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT, SUNFISH_GSI_CHANNEL_DOORBELL1_OFFSET,
    SUNFISH_GSI_CHANNEL_DOORBELL_STRIDE, SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET,
    SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, SUNFISH_GSI_CONTEXT_STRIDE,
    SUNFISH_GSI_EVENT_CONTEXT0_OFFSET, SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT,
    SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET, SUNFISH_GSI_EVENT_DOORBELL0_OFFSET,
    SUNFISH_GSI_EVENT_DOORBELL1_OFFSET, SUNFISH_GSI_EVENT_DOORBELL_STRIDE,
    SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET,
};
use sunfish_ipa_gsi_profile::{
    plan_ap_dma_rings, plan_ap_gsi_context_writes, ApEventRingAllocator, ApEventRingCapacity,
    ApGsiChannelAllocator, ApGsiChannelCapacity, ApGsiChannelIds, DmaPool,
    AP_GSI_CHANNEL_ALLOCATE_ORDER, LINUX_IPA_AP_IOVA_START, SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET,
    SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET,
    SUNFISH_GSI_VERSION_2_2,
};

#[derive(Default)]
struct FakeRegisters {
    attempts: Vec<(u32, u32)>,
    committed: Vec<(u32, u32)>,
    fail_at: Option<usize>,
    registers: BTreeMap<u32, u32>,
    read_attempts: Vec<u32>,
    fail_read_at: Option<usize>,
}

impl RegisterIo for FakeRegisters {
    type Error = usize;

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error> {
        let attempt = self.attempts.len();
        self.attempts.push((offset, value));
        if self.fail_at == Some(attempt) {
            return Err(attempt);
        }
        self.committed.push((offset, value));
        if offset == SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET {
            if let Some(status) = self.registers.get_mut(&SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET) {
                *status &= !value;
                if *status == 0 {
                    if let Some(kind) = self
                        .registers
                        .get_mut(&SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET)
                    {
                        *kind &= !SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT;
                    }
                }
            }
        } else if offset == SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET {
            if let Some(status) = self
                .registers
                .get_mut(&SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET)
            {
                *status &= !value;
                if *status == 0 {
                    if let Some(kind) = self
                        .registers
                        .get_mut(&SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET)
                    {
                        *kind &= !SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT;
                    }
                }
            }
        }
        Ok(())
    }
}

impl EventControlIo for FakeRegisters {
    type Error = usize;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error> {
        let attempt = self.read_attempts.len();
        self.read_attempts.push(offset);
        if self.fail_read_at == Some(attempt) {
            return Err(attempt);
        }
        Ok(self.registers.get(&offset).copied().unwrap_or(0))
    }

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error> {
        RegisterIo::write32(self, offset, value)
    }
}

impl ChannelControlIo for FakeRegisters {
    type Error = usize;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error> {
        EventControlIo::read32(self, offset)
    }

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error> {
        RegisterIo::write32(self, offset, value)
    }
}

fn context_plan() -> sunfish_ipa_gsi_profile::ApGsiContextPlan {
    let rings = plan_ap_dma_rings(DmaPool {
        base_iova: LINUX_IPA_AP_IOVA_START,
        bytes: 0xe000,
    })
    .unwrap();
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut io = FakeRegisters::default();
    io.registers
        .insert(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET, 31 << 3);
    io.registers.insert(
        SUNFISH_GSI_AP_STATUS_OFFSET,
        SUNFISH_GSI_AP_STATUS_ENABLED_BIT,
    );
    let snapshot = read_ap_gsi_channel_snapshot(&mut io).unwrap();
    let mut channel_allocator = ApGsiChannelAllocator::from_snapshot(snapshot).unwrap();
    let channel_reservation = channel_allocator.reserve_ap_channels().unwrap();
    let channel_ids = loop {
        let channel_id = channel_allocator
            .next_channel_id(channel_reservation)
            .unwrap()
            .unwrap();
        issue_next_ap_channel_allocation(&mut io, &mut channel_allocator, channel_reservation)
            .unwrap();
        io.registers.insert(
            SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
            SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
        );
        io.registers
            .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1u32 << channel_id);
        io.registers.insert(
            SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET
                + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(channel_id),
            1 << 20,
        );
        if let Some(ids) = complete_next_ap_channel_allocation(
            &mut io,
            &mut channel_allocator,
            channel_reservation,
        )
        .unwrap()
        {
            break ids;
        }
    };
    assert_eq!(
        io.committed
            .iter()
            .filter(|(offset, _)| *offset == SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET)
            .map(|(_, value)| (*value & 0xff) as u8)
            .collect::<Vec<_>>(),
        AP_GSI_CHANNEL_ALLOCATE_ORDER
    );
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let ids = loop {
        let event_id = allocator.next_event_id(reservation).unwrap().unwrap();
        issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channel_ids)
            .unwrap();
        io.registers.insert(
            SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
            SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT,
        );
        io.registers
            .insert(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET, 1u32 << event_id);
        io.registers.insert(
            SUNFISH_GSI_EVENT_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(event_id),
            1 << 20,
        );
        if let Some(ids) =
            complete_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation).unwrap()
        {
            break ids;
        }
    };
    plan_ap_gsi_context_writes(&rings, ids, channel_ids, SUNFISH_GSI_VERSION_2_2, capacity).unwrap()
}

fn allocated_ap_channel_ids() -> ApGsiChannelIds {
    let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
    let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_channels().unwrap();
    let mut completed = None;
    for channel_id in AP_GSI_CHANNEL_ALLOCATE_ORDER {
        allocator
            .mark_command_issued(reservation, channel_id)
            .unwrap();
        completed = allocator
            .confirm_channel_allocated(reservation, 1u32 << channel_id, 1)
            .unwrap();
    }
    completed.expect("all source-required AP channels are allocated")
}

fn primed_context_receipt() -> ExecuteReceipt {
    let mut context_io = FakeRegisters::default();
    let mut receipt = execute_ap_context(&mut context_io, context_plan()).unwrap();
    let mut prime_io = FakeRegisters::default();
    for expected_id in [0, 1, 2] {
        assert_eq!(
            prime_next_ap_event_ring(&mut prime_io, &mut receipt),
            Ok(expected_id)
        );
    }
    assert_eq!(receipt.next_event_ring_to_prime(), None);
    receipt
}

#[test]
fn event_command_and_control_offsets_match_the_pinned_gsi_v2_register_map() {
    assert_eq!(SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 0x12010);
    assert_eq!(SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET, 0x12080);
    assert_eq!(SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT, 0x2);
    assert_eq!(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET, 0x12094);
    assert_eq!(SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, 0x120a4);
    assert_eq!(SUNFISH_GSI_EVENT_CONTEXT0_OFFSET, 0x10000);
    assert_eq!(SUNFISH_GSI_CONTEXT_STRIDE, 0x80);
}

#[test]
fn channel_allocate_command_and_completion_are_one_shot_and_source_ordered() {
    assert_eq!(SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET, 0x12008);
    let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
    let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_channels().unwrap();
    let mut io = FakeRegisters::default();

    let command = issue_next_ap_channel_allocation(&mut io, &mut allocator, reservation).unwrap();
    assert_eq!(command.channel_id, 5);
    assert_eq!(command.command_word, 5); // GSI_CH_ALLOCATE opcode is zero.
    assert_eq!(io.committed, [(SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET, 5)]);
    assert_eq!(
        complete_next_ap_channel_allocation(&mut io, &mut allocator, reservation),
        Ok(None)
    );

    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    io.registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 5);
    io.registers.insert(
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * 5,
        1 << 20,
    );
    assert_eq!(
        complete_next_ap_channel_allocation(&mut io, &mut allocator, reservation),
        Ok(None)
    );
    assert_eq!(allocator.next_channel_id(reservation), Ok(Some(0)));
}

#[test]
fn uncertain_channel_allocate_write_quarantines_every_reserved_id() {
    let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
    let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_channels().unwrap();
    let mut io = FakeRegisters {
        fail_at: Some(0),
        ..FakeRegisters::default()
    };
    assert_eq!(
        issue_next_ap_channel_allocation(&mut io, &mut allocator, reservation),
        Err(ChannelAllocationCommandError::Write {
            channel_id: 5,
            source: 0,
            quarantine_error: None,
        })
    );
    let expected_mask = AP_GSI_CHANNEL_ALLOCATE_ORDER
        .into_iter()
        .fold(0u32, |mask, channel_id| mask | (1u32 << channel_id));
    assert_eq!(allocator.quarantined_mask(), expected_mask);
    assert_eq!(allocator.occupied_mask(), expected_mask);
}

#[test]
fn unexpected_channel_allocate_irq_quarantines_without_acknowledging_it() {
    let capacity = ApGsiChannelCapacity::from_v2_2_hw_param_2(31 << 3).unwrap();
    let mut allocator = ApGsiChannelAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_channels().unwrap();
    let mut io = FakeRegisters::default();
    issue_next_ap_channel_allocation(&mut io, &mut allocator, reservation).unwrap();
    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    io.registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 0);
    assert!(matches!(
        complete_next_ap_channel_allocation(&mut io, &mut allocator, reservation),
        Err(ChannelAllocationControlError::UnexpectedBitmap {
            expected: 0x20,
            observed: 0x1,
            quarantine_error: None,
        })
    ));
    assert!(io
        .committed
        .iter()
        .all(|(offset, _)| *offset == SUNFISH_GSI_AP_CHANNEL_COMMAND_OFFSET));
    assert_eq!(allocator.quarantined_mask(), expected_ap_channel_mask());
}

#[test]
fn channel_snapshot_reads_capacity_and_all_source_channels_without_writes() {
    let mut io = FakeRegisters::default();
    io.registers
        .insert(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET, 31 << 3);
    io.registers.insert(
        SUNFISH_GSI_AP_STATUS_OFFSET,
        SUNFISH_GSI_AP_STATUS_ENABLED_BIT,
    );
    for channel_id in AP_GSI_CHANNEL_ALLOCATE_ORDER {
        io.registers.insert(
            SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET
                + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(channel_id),
            0,
        );
    }
    io.registers
        .insert(SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET, 1 << 20);

    let snapshot = read_ap_gsi_channel_snapshot(&mut io).unwrap();
    assert_eq!(snapshot.capacity().count(), 31);
    assert_eq!(snapshot.state(0), Some(1));
    assert_eq!(snapshot.state(5), Some(0));
    assert_eq!(snapshot.occupied_mask(), 1);
    let expected_reads = [
        SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET,
        SUNFISH_GSI_AP_STATUS_OFFSET,
    ]
    .into_iter()
    .chain(AP_GSI_CHANNEL_ALLOCATE_ORDER.into_iter().map(|channel_id| {
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(channel_id)
    }))
    .collect::<Vec<_>>();
    assert_eq!(io.read_attempts, expected_reads);
    assert!(io.committed.is_empty());
}

#[test]
fn channel_snapshot_rejects_disabled_manager_before_reading_channel_contexts() {
    let mut io = FakeRegisters::default();
    io.registers
        .insert(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET, 31 << 3);

    assert_eq!(
        read_ap_gsi_channel_snapshot(&mut io),
        Err(ApGsiChannelSnapshotError::ManagerNotEnabled { observed: 0 })
    );
    assert_eq!(
        io.read_attempts,
        [
            SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET,
            SUNFISH_GSI_AP_STATUS_OFFSET,
        ]
    );
    assert!(io.committed.is_empty());
}

#[test]
fn channel_snapshot_rejects_short_capacity_before_reading_contexts() {
    let mut io = FakeRegisters::default();
    io.registers
        .insert(SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET, 9 << 3);
    assert_eq!(
        read_ap_gsi_channel_snapshot(&mut io),
        Err(ApGsiChannelSnapshotError::ChannelOutsideCapacity {
            channel_id: 9,
            reported_count: 9,
        })
    );
    assert_eq!(io.read_attempts, [SUNFISH_GSI_V2_2_AP_HW_PARAM_2_OFFSET]);
    assert!(io.committed.is_empty());
}

fn expected_ap_channel_mask() -> u32 {
    AP_GSI_CHANNEL_ALLOCATE_ORDER
        .into_iter()
        .fold(0u32, |mask, channel_id| mask | (1u32 << channel_id))
}

#[test]
fn channel_start_offsets_and_state_fields_match_the_pinned_gsi_v2_register_map() {
    assert_eq!(SUNFISH_GSI_AP_STATUS_OFFSET, 0x12000);
    assert_eq!(SUNFISH_GSI_AP_STATUS_ENABLED_BIT, 0x1);
    assert_eq!(SUNFISH_GSI_CHANNEL_COMMAND_OFFSET, 0x12008);
    assert_eq!(SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT, 0x1);
    assert_eq!(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 0x12090);
    assert_eq!(SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET, 0x120a0);
    assert_eq!(SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET, 0xf000);
    assert_eq!(SUNFISH_GSI_CHANNEL_DOORBELL1_OFFSET, 0x11004);
    assert_eq!(SUNFISH_GSI_CHANNEL_DOORBELL_STRIDE, 8);
}

#[test]
fn event_ring_prime_offsets_match_the_pinned_gsi_v2_doorbell_map() {
    assert_eq!(SUNFISH_GSI_EVENT_DOORBELL0_OFFSET, 0x11100);
    assert_eq!(SUNFISH_GSI_EVENT_DOORBELL1_OFFSET, 0x11104);
    assert_eq!(SUNFISH_GSI_EVENT_DOORBELL_STRIDE, 8);
}

#[test]
fn executor_writes_the_complete_source_ordered_context_plan() {
    let plan = context_plan();
    let expected: Vec<_> = plan
        .writes()
        .iter()
        .map(|write| (write.offset, write.value))
        .collect();
    let mut io = FakeRegisters::default();

    let receipt = execute_ap_context(&mut io, plan).unwrap();

    assert_eq!(receipt.confirmed_writes as usize, expected.len());
    assert_eq!(receipt.next_event_ring_to_prime(), Some(0));
    assert_eq!(receipt.next_channel_id(), None);
    assert!(!receipt.all_channels_started());
    assert_eq!(expected.len(), 45);
    assert_eq!(io.attempts, expected);
    assert_eq!(io.committed, expected);
}

#[test]
fn event_rings_are_primed_msb_then_lsb_before_channel_start_is_available() {
    let mut context_io = FakeRegisters::default();
    let mut receipt = execute_ap_context(&mut context_io, context_plan()).unwrap();
    let mut io = FakeRegisters::default();

    assert_eq!(
        issue_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelStartCommandError::EventRingsNotPrimed)
    );
    for expected_id in [0, 1, 2] {
        assert_eq!(
            prime_next_ap_event_ring(&mut io, &mut receipt),
            Ok(expected_id)
        );
    }
    assert_eq!(receipt.next_event_ring_to_prime(), None);
    assert_eq!(receipt.next_channel_id(), Some(1));
    assert_eq!(
        io.committed,
        [
            (SUNFISH_GSI_EVENT_DOORBELL1_OFFSET, 0),
            (SUNFISH_GSI_EVENT_DOORBELL0_OFFSET, 0x2000_7bf0),
            (
                SUNFISH_GSI_EVENT_DOORBELL1_OFFSET + SUNFISH_GSI_EVENT_DOORBELL_STRIDE,
                0,
            ),
            (
                SUNFISH_GSI_EVENT_DOORBELL0_OFFSET + SUNFISH_GSI_EVENT_DOORBELL_STRIDE,
                0x2000_8ff0,
            ),
            (
                SUNFISH_GSI_EVENT_DOORBELL1_OFFSET + SUNFISH_GSI_EVENT_DOORBELL_STRIDE * 2,
                0,
            ),
            (
                SUNFISH_GSI_EVENT_DOORBELL0_OFFSET + SUNFISH_GSI_EVENT_DOORBELL_STRIDE * 2,
                0x2000_aff0,
            ),
        ]
    );
}

#[test]
fn uncertain_event_ring_prime_write_quarantines_and_blocks_channel_start() {
    let mut context_io = FakeRegisters::default();
    let mut receipt = execute_ap_context(&mut context_io, context_plan()).unwrap();
    let mut io = FakeRegisters {
        fail_at: Some(1),
        ..FakeRegisters::default()
    };

    assert_eq!(
        prime_next_ap_event_ring(&mut io, &mut receipt),
        Err(EventRingPrimeError::Write {
            event_id: 0,
            offset: SUNFISH_GSI_EVENT_DOORBELL0_OFFSET,
            write_index: 1,
            source: 1,
        })
    );
    assert_eq!(receipt.next_event_ring_to_prime(), None);
    assert_eq!(receipt.next_channel_id(), None);
    assert_eq!(
        issue_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelStartCommandError::Quarantined)
    );
    assert_eq!(
        io.attempts.len(),
        2,
        "an uncertain doorbell was not retried"
    );
}

#[test]
fn channel_start_command_is_one_shot_and_waits_for_control_irq_and_started_state() {
    let mut receipt = primed_context_receipt();
    let mut io = FakeRegisters::default();

    let command = issue_next_ap_channel_start(&mut io, &mut receipt).unwrap();
    assert_eq!(command.channel_id, 1);
    assert_eq!(command.command_word, 0x0100_0001);
    assert_eq!(
        io.attempts,
        [(SUNFISH_GSI_CHANNEL_COMMAND_OFFSET, 0x0100_0001)]
    );
    assert_eq!(
        issue_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelStartCommandError::CommandAlreadyIssued)
    );
    assert_eq!(
        complete_next_ap_channel_start(&mut io, &mut receipt),
        Ok(None)
    );

    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    io.registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 1);
    io.registers.insert(
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE,
        2 << 20,
    );
    let completed = complete_next_ap_channel_start(&mut io, &mut receipt)
        .unwrap()
        .unwrap();

    assert_eq!(completed.channel_id, 1);
    assert_eq!(completed.state, 2);
    assert_eq!(completed.doorbell_msb, 0);
    assert_eq!(
        io.committed,
        [
            (SUNFISH_GSI_CHANNEL_COMMAND_OFFSET, 0x0100_0001),
            (SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET, 1 << 1),
            (
                SUNFISH_GSI_CHANNEL_DOORBELL1_OFFSET + SUNFISH_GSI_CHANNEL_DOORBELL_STRIDE,
                0,
            ),
        ]
    );
    assert_eq!(receipt.next_channel_id(), Some(2));

    let second = issue_next_ap_channel_start(&mut io, &mut receipt).unwrap();
    assert_eq!(second.channel_id, 2);
    assert_eq!(second.command_word, 0x0100_0002);
}

#[test]
fn channel_start_accepts_source_driver_flow_control_state() {
    let mut receipt = primed_context_receipt();
    let mut io = FakeRegisters::default();
    issue_next_ap_channel_start(&mut io, &mut receipt).unwrap();
    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    io.registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 1);
    io.registers.insert(
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE,
        5 << 20,
    );

    let completion = complete_next_ap_channel_start(&mut io, &mut receipt)
        .unwrap()
        .unwrap();
    assert_eq!(completion.state, 5);
    assert_eq!(receipt.next_channel_id(), Some(2));
}

#[test]
fn unexpected_channel_start_irq_quarantines_without_acknowledging_other_channels() {
    let mut receipt = primed_context_receipt();
    let mut io = FakeRegisters::default();
    issue_next_ap_channel_start(&mut io, &mut receipt).unwrap();
    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    io.registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 2);

    assert_eq!(
        complete_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelControlError::UnexpectedBitmap {
            expected: 1 << 1,
            observed: 1 << 2,
        })
    );
    assert_eq!(receipt.next_channel_id(), None);
    assert_eq!(io.attempts.len(), 1, "unexpected IRQ was not acknowledged");
    assert_eq!(
        issue_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelStartCommandError::Quarantined)
    );
}

#[test]
fn uncertain_channel_start_command_write_quarantines_without_retry() {
    let mut receipt = primed_context_receipt();
    let mut io = FakeRegisters {
        fail_at: Some(0),
        ..FakeRegisters::default()
    };

    assert_eq!(
        issue_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelStartCommandError::Write {
            channel_id: 1,
            source: 0,
        })
    );
    assert_eq!(
        io.attempts,
        [(SUNFISH_GSI_CHANNEL_COMMAND_OFFSET, 0x0100_0001)]
    );
    assert_eq!(receipt.next_channel_id(), None);
    assert_eq!(
        issue_next_ap_channel_start(&mut io, &mut receipt),
        Err(ChannelStartCommandError::Quarantined)
    );
}

#[test]
fn failed_channel_ack_or_post_ack_state_read_quarantines_the_generation() {
    let mut receipt = primed_context_receipt();
    let mut ack_io = FakeRegisters {
        fail_at: Some(1),
        ..FakeRegisters::default()
    };
    issue_next_ap_channel_start(&mut ack_io, &mut receipt).unwrap();
    ack_io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    ack_io
        .registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 1);
    assert_eq!(
        complete_next_ap_channel_start(&mut ack_io, &mut receipt),
        Err(ChannelControlError::Acknowledge {
            channel_id: 1,
            source: 1,
        })
    );
    assert_eq!(receipt.next_channel_id(), None);

    let mut receipt = primed_context_receipt();
    let mut read_io = FakeRegisters {
        fail_read_at: Some(2),
        ..FakeRegisters::default()
    };
    issue_next_ap_channel_start(&mut read_io, &mut receipt).unwrap();
    read_io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    read_io
        .registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 1);
    assert_eq!(
        complete_next_ap_channel_start(&mut read_io, &mut receipt),
        Err(ChannelControlError::ContextRead {
            channel_id: 1,
            source: 2,
        })
    );
    assert_eq!(receipt.next_channel_id(), None);
    assert_eq!(
        read_io.committed,
        [
            (SUNFISH_GSI_CHANNEL_COMMAND_OFFSET, 0x0100_0001),
            (SUNFISH_GSI_CHANNEL_IRQ_CLEAR_OFFSET, 1 << 1),
        ]
    );
}

#[test]
fn unexpected_channel_state_or_initial_doorbell_failure_quarantines() {
    let mut receipt = primed_context_receipt();
    let mut state_io = FakeRegisters::default();
    issue_next_ap_channel_start(&mut state_io, &mut receipt).unwrap();
    state_io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    state_io
        .registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 1);
    state_io.registers.insert(
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE,
        1 << 20,
    );
    assert_eq!(
        complete_next_ap_channel_start(&mut state_io, &mut receipt),
        Err(ChannelControlError::UnexpectedState {
            channel_id: 1,
            state: 1,
        })
    );
    assert_eq!(receipt.next_channel_id(), None);

    let mut receipt = primed_context_receipt();
    let mut doorbell_io = FakeRegisters {
        fail_at: Some(2),
        ..FakeRegisters::default()
    };
    issue_next_ap_channel_start(&mut doorbell_io, &mut receipt).unwrap();
    doorbell_io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_CHANNEL_CONTROL_IRQ_BIT,
    );
    doorbell_io
        .registers
        .insert(SUNFISH_GSI_CHANNEL_IRQ_SOURCE_OFFSET, 1 << 1);
    doorbell_io.registers.insert(
        SUNFISH_GSI_CHANNEL_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE,
        2 << 20,
    );
    assert_eq!(
        complete_next_ap_channel_start(&mut doorbell_io, &mut receipt),
        Err(ChannelControlError::DoorbellWrite {
            channel_id: 1,
            source: 2,
        })
    );
    assert_eq!(receipt.next_channel_id(), None);
}

#[test]
fn event_allocation_issues_exact_command_once_and_waits_for_external_completion() {
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let channels = allocated_ap_channel_ids();
    let mut io = FakeRegisters::default();

    let receipt =
        issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels)
            .unwrap();

    assert_eq!(receipt.event_id, 0);
    assert_eq!(receipt.command_word, 0);
    assert_eq!(io.attempts, [(SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 0)]);
    assert_eq!(allocator.next_event_id(reservation), Ok(Some(0)));
    assert_eq!(
        issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels),
        Err(EventRingCommandError::Allocation(
            sunfish_ipa_gsi_profile::EventRingAllocationError::CommandAlreadyIssued
        ))
    );
    assert_eq!(io.attempts.len(), 1);
}

#[test]
fn uncertain_event_command_write_quarantines_ids_and_cannot_be_retried() {
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let channels = allocated_ap_channel_ids();
    let mut io = FakeRegisters {
        fail_at: Some(0),
        ..FakeRegisters::default()
    };

    assert_eq!(
        issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels),
        Err(EventRingCommandError::Write {
            event_id: 0,
            source: 0,
            quarantine_error: None,
        })
    );
    assert_eq!(io.attempts, [(SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 0)]);
    assert_eq!(allocator.occupied_mask(), 0b111);
    assert_eq!(allocator.quarantined_mask(), 0b111);
    assert_eq!(
        allocator.next_event_id(reservation),
        Err(sunfish_ipa_gsi_profile::EventRingAllocationError::StaleReservation)
    );
    let next = allocator.reserve_ap_system_rings().unwrap();
    assert_eq!(allocator.next_event_id(next), Ok(Some(3)));
}

#[test]
fn event_control_irq_and_context_state_gate_each_ring_before_ids_are_minted() {
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let channels = allocated_ap_channel_ids();
    let mut io = FakeRegisters::default();
    let mut final_ids = None;

    for event_id in 0..3u8 {
        let command =
            issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels)
                .unwrap();
        assert_eq!(command.event_id, event_id);
        if event_id == 0 {
            assert_eq!(
                complete_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation),
                Ok(None)
            );
        }
        io.registers.insert(
            SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
            SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT,
        );
        io.registers
            .insert(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET, 1u32 << event_id);
        io.registers.insert(
            SUNFISH_GSI_EVENT_CONTEXT0_OFFSET + SUNFISH_GSI_CONTEXT_STRIDE * u32::from(event_id),
            1 << 20,
        );
        final_ids = complete_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation)
            .unwrap()
            .or(final_ids);
    }

    let ids = final_ids.expect("the third confirmed event allocation should mint IDs");
    assert_eq!(ids.common(), 0);
    assert_eq!(ids.apps_command(), 1);
    assert_eq!(ids.apps_lan_receive(), 2);
    assert_eq!(
        io.committed,
        [
            (SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 0),
            (SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, 1),
            (SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 1),
            (SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, 2),
            (SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 2),
            (SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, 4),
        ]
    );
}

#[test]
fn unexpected_event_irq_bitmap_stops_and_quarantines_the_reserved_ids() {
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let channels = allocated_ap_channel_ids();
    let mut io = FakeRegisters::default();
    issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels).unwrap();
    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT,
    );
    io.registers.insert(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET, 2);

    assert_eq!(
        complete_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation),
        Err(EventControlError::UnexpectedBitmap {
            expected: 1,
            observed: 2,
            quarantine_error: None,
        })
    );
    assert_eq!(allocator.quarantined_mask(), 0b111);
    assert_eq!(io.attempts.len(), 1, "unexpected IRQ bitmap was not acked");
}

#[test]
fn ambiguous_event_irq_acknowledgement_quarantines_without_retry() {
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let channels = allocated_ap_channel_ids();
    let mut io = FakeRegisters {
        fail_at: Some(1),
        ..FakeRegisters::default()
    };
    issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels).unwrap();
    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT,
    );
    io.registers.insert(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET, 1);

    assert_eq!(
        complete_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation),
        Err(EventControlError::Acknowledge {
            event_id: 0,
            source: 1,
            quarantine_error: None,
        })
    );
    assert_eq!(allocator.quarantined_mask(), 0b111);
    assert_eq!(io.attempts.len(), 2);
}

#[test]
fn context_read_failure_after_irq_acknowledgement_quarantines_ids() {
    let capacity = ApEventRingCapacity::from_v2_2_hw_param_2(24 << 8).unwrap();
    let mut allocator = ApEventRingAllocator::new(capacity, 0).unwrap();
    let reservation = allocator.reserve_ap_system_rings().unwrap();
    let channels = allocated_ap_channel_ids();
    let mut io = FakeRegisters {
        fail_read_at: Some(2),
        ..FakeRegisters::default()
    };
    issue_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation, channels).unwrap();
    io.registers.insert(
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_OFFSET,
        SUNFISH_GSI_EVENT_CONTROL_TYPE_IRQ_BIT,
    );
    io.registers.insert(SUNFISH_GSI_EVENT_IRQ_SOURCE_OFFSET, 1);

    assert_eq!(
        complete_next_ap_event_ring_allocation(&mut io, &mut allocator, reservation),
        Err(EventControlError::ContextRead {
            event_id: 0,
            source: 2,
            quarantine_error: None,
        })
    );
    assert_eq!(allocator.quarantined_mask(), 0b111);
    assert_eq!(
        io.committed,
        [
            (SUNFISH_GSI_AP_EVENT_COMMAND_OFFSET, 0),
            (SUNFISH_GSI_EVENT_IRQ_CLEAR_OFFSET, 1),
        ]
    );
}

#[test]
fn failed_write_reports_confirmed_prefix_and_is_not_retried() {
    let plan = context_plan();
    let mut io = FakeRegisters {
        fail_at: Some(7),
        ..FakeRegisters::default()
    };

    let expected = plan.writes()[7];
    let result = execute_ap_context(&mut io, plan);

    assert_eq!(
        result,
        Err(ExecuteError::Write {
            index: 7,
            offset: expected.offset,
            confirmed_writes: 7,
            source: 7,
        })
    );
    assert_eq!(io.attempts.len(), 8);
    assert_eq!(io.committed.len(), 7);
}
