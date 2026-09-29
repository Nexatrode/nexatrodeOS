#![no_std]
#![no_main]

mod vault_protocol;

use core::{
    ptr,
    sync::atomic::{Ordering, fence},
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, get_mr, memory_region_symbol, protection_domain, set_mr,
};
use vault_protocol::{
    OWNER_CHANNEL, REPLY_BAD_REQUEST, REPLY_CONFLICT, REPLY_CORRUPT, REPLY_DENIED, REPLY_OK,
    REQUEST_READ, REQUEST_WRITE, TEST_VALUE,
};

const SLOT_COUNT: usize = 2;
const COMMIT_SALT: MessageRegisterValue = 0x6a09_e667_f3bc_c909;
const CHECKSUM_SALT: MessageRegisterValue = 0xbb67_ae85_84ca_a73b;

#[repr(C)]
#[derive(Clone, Copy)]
struct RecordSlot {
    generation: MessageRegisterValue,
    value: MessageRegisterValue,
    checksum: MessageRegisterValue,
    committed: MessageRegisterValue,
}

impl RecordSlot {
    fn valid(self) -> bool {
        self.committed == (self.generation ^ COMMIT_SALT)
            && self.checksum == checksum(self.generation, self.value)
    }
}

#[repr(C)]
struct VaultState {
    slots: [RecordSlot; SLOT_COUNT],
}

#[protection_domain]
fn init() -> Vault {
    let state = memory_region_symbol!(vault_private_start: *mut VaultState).as_ptr();
    let initial = RecordSlot {
        generation: 0,
        value: vault_protocol::INITIAL_VALUE,
        checksum: checksum(0, vault_protocol::INITIAL_VALUE),
        committed: COMMIT_SALT,
    };
    // Boot with a committed old version and a torn, uncommitted newer version.
    // This makes recovery selection part of every QEMU boot test.
    let interrupted = RecordSlot {
        generation: 1,
        value: TEST_VALUE ^ 0xffff,
        checksum: checksum(1, TEST_VALUE ^ 0xffff),
        committed: 0,
    };
    unsafe {
        ptr::write_volatile(
            state,
            VaultState {
                slots: [initial, interrupted],
            },
        );
    }
    debug_println!("VAULT: RAM record ready; staged incomplete version for recovery test");
    Vault { state }
}

struct Vault {
    // This address is mapped only in the vault domain by system.system.
    state: *mut VaultState,
}

impl Vault {
    fn read_slot(&self, index: usize) -> RecordSlot {
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.state).slots[index])) }
    }

    fn write_slot_field(&mut self, index: usize, field: SlotField, value: MessageRegisterValue) {
        unsafe {
            let slot = ptr::addr_of_mut!((*self.state).slots[index]);
            let field = match field {
                SlotField::Generation => ptr::addr_of_mut!((*slot).generation),
                SlotField::Value => ptr::addr_of_mut!((*slot).value),
                SlotField::Checksum => ptr::addr_of_mut!((*slot).checksum),
                SlotField::Committed => ptr::addr_of_mut!((*slot).committed),
            };
            ptr::write_volatile(field, value);
        }
    }

    fn active_slot(&self) -> Option<(usize, RecordSlot)> {
        let first = self.read_slot(0);
        let second = self.read_slot(1);
        match (first.valid(), second.valid()) {
            (true, true) if second.generation > first.generation => Some((1, second)),
            (true, _) => Some((0, first)),
            (false, true) => Some((1, second)),
            (false, false) => None,
        }
    }

    fn write_record(
        &mut self,
        expected_generation: MessageRegisterValue,
        value: MessageRegisterValue,
    ) -> Result<MessageRegisterValue, MessageLabel> {
        let Some((active_index, active)) = self.active_slot() else {
            return Err(REPLY_CORRUPT);
        };
        if active.generation != expected_generation {
            return Err(REPLY_CONFLICT);
        }
        let Some(next_generation) = active.generation.checked_add(1) else {
            return Err(REPLY_CONFLICT);
        };
        let target = 1 - active_index;

        // Invalidate the old slot first, stage the new record, then publish its
        // commit marker last. This gives the service a simple recovery rule;
        // actual power-loss durability still requires a block device and flush.
        self.write_slot_field(target, SlotField::Committed, 0);
        fence(Ordering::Release);
        self.write_slot_field(target, SlotField::Generation, next_generation);
        self.write_slot_field(target, SlotField::Value, value);
        self.write_slot_field(
            target,
            SlotField::Checksum,
            checksum(next_generation, value),
        );
        fence(Ordering::Release);
        self.write_slot_field(target, SlotField::Committed, next_generation ^ COMMIT_SALT);
        Ok(next_generation)
    }
}

#[derive(Clone, Copy)]
enum SlotField {
    Generation,
    Value,
    Checksum,
    Committed,
}

impl Handler for Vault {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("FAIL: unexpected vault notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        // Channel identity is assigned by the kernel configuration. There is
        // deliberately no caller ID in the request payload for a client to spoof.
        if channel != OWNER_CHANNEL {
            return Ok(MessageInfo::new(REPLY_DENIED, 0));
        }

        match (request.label(), request.count()) {
            (REQUEST_READ, 0) => match self.active_slot() {
                Some((_index, record)) => {
                    set_mr(0, record.generation);
                    set_mr(1, record.value);
                    Ok(MessageInfo::new(REPLY_OK, 2))
                }
                None => Ok(MessageInfo::new(REPLY_CORRUPT, 0)),
            },
            (REQUEST_WRITE, 2) => {
                let expected_generation = get_mr(0);
                let value = get_mr(1);
                match self.write_record(expected_generation, value) {
                    Ok(generation) => {
                        set_mr(0, generation);
                        Ok(MessageInfo::new(REPLY_OK, 1))
                    }
                    Err(label) => Ok(MessageInfo::new(label, 0)),
                }
            }
            _ => Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0)),
        }
    }
}

fn checksum(generation: MessageRegisterValue, value: MessageRegisterValue) -> MessageRegisterValue {
    generation.rotate_left(17) ^ value.rotate_right(9) ^ CHECKSUM_SALT
}
