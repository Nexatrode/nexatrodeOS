#![no_std]
#![no_main]

mod entropy_protocol;
mod vault_protocol;

use entropy_protocol::{
    CLIENT_CHANNEL as ENTROPY_CHANNEL, REPLY_OK as ENTROPY_OK, REQUEST_BYTES as ENTROPY_REQUEST,
    SAMPLE_BYTES as ENTROPY_SAMPLE_BYTES,
};
use sel4::{Fault, UserContext};
use sel4_microkit::{
    Channel, ChannelSet, Child, Handler, Infallible, MessageInfo, MessageLabel,
    MessageRegisterValue, debug_print, debug_println, get_mr, protection_domain, set_mr,
};
use vault_protocol::{
    CLIENT_CHANNEL as VAULT, REPLY_CONFLICT as VAULT_CONFLICT,
    REPLY_NAMESPACE_TEST as VAULT_NAMESPACE_TEST, REPLY_OK as VAULT_OK,
    REQUEST_NAMESPACE_TEST as VAULT_NAMESPACE_TEST_REQUEST, REQUEST_READ as VAULT_READ,
    REQUEST_WRITE as VAULT_WRITE, TEST_VALUE as VAULT_TEST_VALUE,
};

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../build/crasher_entry.rs"
));

const SERVICE: Channel = Channel::new(0);
const NOTES_APP: Channel = Channel::new(7);
const CONTACTS_APP: Channel = Channel::new(8);
const C_REFERENCE: Channel = Channel::new(4);
const START_CRASHER: Channel = Channel::new(1);
const CONSOLE_TO_CLIENT: Channel = Channel::new(2);
const CLIENT_TO_CONSOLE: Channel = Channel::new(3);
const CONSOLE_START_CRASHER: Channel = Channel::new(5);
const CRASHER_CHILD: Child = Child::new(1);
const ENTROPY: Channel = Channel::new(ENTROPY_CHANNEL);

const REQUEST_ADD: MessageLabel = 1;
const REQUEST_COUNT: MessageLabel = 2;
const REQUEST_GUARD: MessageLabel = 3;
const REPLY_OK: MessageLabel = 0x100;
const REPLY_BAD_REQUEST: MessageLabel = 0x101;
const REPLY_OVERFLOW: MessageLabel = 0x102;
const SERVER_GUARD_VALUE: MessageRegisterValue = 0x51A7E123;
const SERVER_PRIVATE_ADDRESS: MessageRegisterValue = 0x4000_0000;

const REQUEST_CONSOLE_CALC: MessageLabel = 0x201;
const REQUEST_CONSOLE_STATUS: MessageLabel = 0x202;
const REQUEST_CONSOLE_CRASH: MessageLabel = 0x203;
const REPLY_CONSOLE_ERROR: MessageLabel = 0x2ff;

// seL4's AArch64 fault tag and VMFault message length.
const VM_FAULT_LABEL: MessageLabel = 6;
const VM_FAULT_WORDS: usize = 4;

#[protection_domain]
fn init() -> Supervisor {
    let mut supervisor = Supervisor {
        failures: 0,
        checks: 0,
        crash_armed: false,
        crash_inflight: false,
        crash_request_pending: false,
        handled_faults: 0,
        counter_before_crash: 0,
        vault_generation: 0,
        vault_value: 0,
    };
    supervisor.run_baseline();
    supervisor
}

struct Supervisor {
    failures: usize,
    checks: usize,
    crash_armed: bool,
    crash_inflight: bool,
    crash_request_pending: bool,
    handled_faults: MessageRegisterValue,
    counter_before_crash: MessageRegisterValue,
    vault_generation: MessageRegisterValue,
    vault_value: MessageRegisterValue,
}

impl Supervisor {
    fn record(&mut self, passed: bool, name: &str) {
        self.checks += 1;
        if !passed {
            self.failures += 1;
        }
        debug_print!("{}{}\n", if passed { "PASS: " } else { "FAIL: " }, name);
    }

    fn check(
        &mut self,
        reply: MessageInfo,
        expected_label: MessageLabel,
        expected_count: usize,
        expected_value: MessageRegisterValue,
        name: &str,
    ) {
        let passed = reply.label() == expected_label
            && reply.count() == expected_count
            && (expected_count == 0 || get_mr(0) == expected_value);
        self.record(passed, name);
    }

    fn check_vault_record(
        &mut self,
        expected_generation: MessageRegisterValue,
        expected_value: MessageRegisterValue,
        name: &str,
    ) {
        let passed = self.read_vault_record() == Some((expected_generation, expected_value));
        self.record(passed, name);
    }

    fn read_vault_record(&mut self) -> Option<(MessageRegisterValue, MessageRegisterValue)> {
        let reply = VAULT.pp_call(MessageInfo::new(VAULT_READ, 0));
        if reply.label() != VAULT_OK || reply.count() != 2 {
            return None;
        }
        Some((get_mr(0), get_mr(1)))
    }

    fn write_vault_record(
        &mut self,
        expected_generation: MessageRegisterValue,
        value: MessageRegisterValue,
    ) -> MessageInfo {
        set_mr(0, expected_generation);
        set_mr(1, value);
        VAULT.pp_call(MessageInfo::new(VAULT_WRITE, 2))
    }

    fn run_baseline(&mut self) {
        debug_println!("CLIENT: sending requests through capability channel 0");

        let entropy_passed = entropy_smoke_test();
        self.record(entropy_passed, "QEMU VirtIO RNG delivers fresh samples");
        if entropy_passed {
            debug_println!("ENTROPY|PASS: four distinct 128-bit samples received");
        }

        set_mr(0, 19);
        set_mr(1, 23);
        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_ADD, 2)),
            REPLY_OK,
            1,
            42,
            "Rust server computes 19 + 23 = 42",
        );

        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_ADD, 0)),
            REPLY_BAD_REQUEST,
            0,
            0,
            "Rust server rejects missing operands",
        );

        self.check(
            SERVICE.pp_call(MessageInfo::new(99, 0)),
            REPLY_BAD_REQUEST,
            0,
            0,
            "Rust server rejects unknown operation",
        );

        set_mr(0, MessageRegisterValue::MAX);
        set_mr(1, 1);
        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_ADD, 2)),
            REPLY_OVERFLOW,
            0,
            0,
            "Rust server rejects integer overflow",
        );

        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_COUNT, 0)),
            REPLY_OK,
            1,
            1,
            "rejected requests leave Rust server state unchanged",
        );

        set_mr(0, 7);
        set_mr(1, 8);
        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_ADD, 2)),
            REPLY_OK,
            1,
            15,
            "Rust server responds after malformed requests",
        );

        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_GUARD, 0)),
            REPLY_OK,
            1,
            SERVER_GUARD_VALUE,
            "Rust server private page initialized",
        );

        set_mr(0, 19);
        set_mr(1, 23);
        self.check(
            C_REFERENCE.pp_call(MessageInfo::new(REQUEST_ADD, 2)),
            REPLY_OK,
            1,
            42,
            "C reference server answers protected call",
        );

        self.check(
            C_REFERENCE.pp_call(MessageInfo::new(REQUEST_ADD, 1)),
            REPLY_BAD_REQUEST,
            0,
            0,
            "C reference server rejects wrong message length",
        );

        self.check(
            C_REFERENCE.pp_call(MessageInfo::new(99, 2)),
            REPLY_BAD_REQUEST,
            0,
            0,
            "C reference server rejects unknown operation",
        );

        set_mr(0, MessageRegisterValue::MAX);
        set_mr(1, 1);
        self.check(
            C_REFERENCE.pp_call(MessageInfo::new(REQUEST_ADD, 2)),
            REPLY_OVERFLOW,
            0,
            0,
            "C reference server rejects unsigned overflow",
        );

        let current_record = self.read_vault_record();
        self.record(
            current_record.is_some(),
            "vault reads latest committed record",
        );
        let (current_generation, _) = current_record.unwrap_or((0, 0));
        let next_generation = current_generation.checked_add(1).unwrap_or(0);
        let write_reply = self.write_vault_record(current_generation, VAULT_TEST_VALUE);
        self.check(
            write_reply,
            VAULT_OK,
            1,
            next_generation,
            "vault accepts versioned owner update",
        );
        self.vault_generation = next_generation;
        self.vault_value = VAULT_TEST_VALUE;
        self.check_vault_record(
            next_generation,
            VAULT_TEST_VALUE,
            "vault reads back committed version",
        );
        let stale_reply = self.write_vault_record(current_generation, 0xBAD);
        self.check(
            stale_reply,
            VAULT_CONFLICT,
            0,
            0,
            "vault rejects stale version",
        );
        self.check_vault_record(
            next_generation,
            VAULT_TEST_VALUE,
            "vault keeps latest version after stale write",
        );

        self.check(
            NOTES_APP.pp_call(MessageInfo::new(VAULT_NAMESPACE_TEST_REQUEST, 0)),
            VAULT_NAMESPACE_TEST,
            0,
            0,
            "Notes app is confined to its own vault namespace",
        );
        self.check(
            CONTACTS_APP.pp_call(MessageInfo::new(VAULT_NAMESPACE_TEST_REQUEST, 0)),
            VAULT_NAMESPACE_TEST,
            0,
            0,
            "Contacts app is confined to its own vault namespace",
        );

        if self.failures != 0 {
            debug_println!("FAIL: baseline failed; fault test not started");
            return;
        }

        self.crash_armed = true;
        self.crash_inflight = true;
        self.counter_before_crash = 2;
        debug_println!("CLIENT: baseline passed; triggering isolated crash");
        #[cfg(feature = "budget-pressure")]
        CLIENT_TO_CONSOLE.notify();
        START_CRASHER.notify();
    }

    fn read_addition_count(&self) -> Option<MessageRegisterValue> {
        let reply = SERVICE.pp_call(MessageInfo::new(REQUEST_COUNT, 0));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            return None;
        }
        Some(get_mr(0))
    }

    fn handle_fault(&mut self, child: Child, message: MessageInfo) -> bool {
        let vm_fault = if message.label() == VM_FAULT_LABEL && message.count() == VM_FAULT_WORDS {
            match message.fault() {
                Fault::VmFault(fault) => Some((fault.addr(), fault.is_prefetch(), fault.fsr())),
                _ => None,
            }
        } else {
            None
        };

        let expected = self.crash_armed
            && self.crash_inflight
            && child == CRASHER_CHILD
            && vm_fault.is_some_and(|(address, prefetch, syndrome)| {
                address == SERVER_PRIVATE_ADDRESS
                    && !prefetch
                    // AArch64: lower-EL data abort, write access, translation fault.
                    && ((syndrome >> 26) & 0x3f) == 0x24
                    && syndrome & (1 << 6) != 0
                    && syndrome & 0x3c == 0x04
            });

        if self.handled_faults == 0 {
            self.record(expected, "kernel blocked crasher write at 0x40000000");
        } else {
            debug_print!(
                "{}kernel blocked crasher write at 0x40000000\n",
                if expected { "PASS: " } else { "FAIL: " }
            );
            if !expected {
                self.failures += 1;
            }
        }

        if !expected {
            debug_println!("FAIL: unexpected child fault; isolation test aborted");
            debug_print!(
                "FAULT DIAGNOSTIC: child={} armed={} inflight={} label={} words={}",
                child.index(),
                self.crash_armed as usize,
                self.crash_inflight as usize,
                message.label(),
                message.count()
            );
            if let Some((address, prefetch, syndrome)) = vm_fault {
                debug_print!(
                    " address={:#x} syndrome={:#x} prefetch={}",
                    address,
                    syndrome,
                    prefetch as usize
                );
            }
            debug_println!("");
            return false;
        }

        self.crash_armed = false;
        self.crash_inflight = false;
        self.handled_faults = self.handled_faults.wrapping_add(1);
        child.tcb().tcb_suspend().unwrap();
        debug_println!("CLIENT: crasher stopped; checking independent server");

        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_GUARD, 0)),
            REPLY_OK,
            1,
            SERVER_GUARD_VALUE,
            "server private page unchanged after fault",
        );
        let count_reply = SERVICE.pp_call(MessageInfo::new(REQUEST_COUNT, 0));
        if self.handled_faults == 1 {
            self.check(
                count_reply,
                REPLY_OK,
                1,
                self.counter_before_crash,
                "server counter preserved after fault",
            );
        } else {
            let preserved = count_reply.label() == REPLY_OK
                && count_reply.count() == 1
                && get_mr(0) == self.counter_before_crash;
            if !preserved {
                self.failures += 1;
            }
            debug_println!(
                "{}server counter preserved after fault",
                if preserved { "PASS: " } else { "FAIL: " }
            );
        }

        self.check_vault_record(
            self.vault_generation,
            self.vault_value,
            "vault record unchanged after denied child write",
        );

        set_mr(0, 100);
        set_mr(1, 23);
        self.check(
            SERVICE.pp_call(MessageInfo::new(REQUEST_ADD, 2)),
            REPLY_OK,
            1,
            123,
            "independent server computes 100 + 23 after fault",
        );

        if self.handled_faults == 1 {
            self.check(
                SERVICE.pp_call(MessageInfo::new(REQUEST_COUNT, 0)),
                REPLY_OK,
                1,
                self.counter_before_crash.wrapping_add(1),
                "independent server state advances after fault",
            );
            debug_println!(
                "{}",
                if self.failures == 0 && self.checks == 25 {
                    "ISOLATION COMPLETE: 25 checks passed; crasher stopped, services alive"
                } else {
                    "FAIL: isolation checks failed"
                }
            );
            CLIENT_TO_CONSOLE.notify();
        } else {
            let after_reply = SERVICE.pp_call(MessageInfo::new(REQUEST_COUNT, 0));
            let advanced = after_reply.label() == REPLY_OK
                && after_reply.count() == 1
                && get_mr(0) == self.counter_before_crash.wrapping_add(1);
            if !advanced {
                self.failures += 1;
            }
            debug_println!(
                "{}independent server state advances after fault",
                if advanced { "PASS: " } else { "FAIL: " }
            );
            debug_println!(
                "{}",
                if self.failures == 0 {
                    "CRASH COMPLETE: crasher stopped; server remains active"
                } else {
                    "FAIL: crash recovery checks failed"
                }
            );
        }

        true
    }

    fn restart_crasher(&self) {
        let tcb = CRASHER_CHILD.tcb();
        let mut registers = UserContext::default();
        *registers.pc_mut() = CRASHER_RESTART_ENTRY as MessageRegisterValue;
        // As in microkit_pd_restart, write only the PC and resume the child.
        tcb.tcb_write_registers(true, 1, &mut registers).unwrap();
    }
}

fn entropy_smoke_test() -> bool {
    let mut samples = [[0u8; ENTROPY_SAMPLE_BYTES]; 4];
    for sample in &mut samples {
        let reply = ENTROPY.pp_call(MessageInfo::new(ENTROPY_REQUEST, 0));
        if reply.label() != ENTROPY_OK || reply.count() != 2 {
            return false;
        }
        let first = get_mr(0).to_le_bytes();
        let second = get_mr(1).to_le_bytes();
        sample[..8].copy_from_slice(&first);
        sample[8..].copy_from_slice(&second);
        if sample.iter().all(|byte| *byte == 0) {
            return false;
        }
    }
    for index in 0..samples.len() {
        if samples[..index].contains(&samples[index]) {
            return false;
        }
    }
    true
}

impl Handler for Supervisor {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(CONSOLE_START_CRASHER)
            && self.crash_request_pending
            && !self.crash_inflight
        {
            self.crash_request_pending = false;
            self.crash_armed = true;
            self.crash_inflight = true;
            self.restart_crasher();
            return Ok(());
        }

        debug_println!("FAIL: unexpected client notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        let operation = request.label();
        let count = request.count();

        if channel != CONSOLE_TO_CLIENT {
            return Ok(MessageInfo::new(REPLY_CONSOLE_ERROR, 0));
        }
        if operation == REQUEST_CONSOLE_STATUS && count == 0 {
            set_mr(0, self.handled_faults);
            return Ok(MessageInfo::new(REPLY_OK, 1));
        }
        if operation == REQUEST_CONSOLE_CALC && count == 2 {
            let left = get_mr(0);
            let right = get_mr(1);
            set_mr(0, left);
            set_mr(1, right);
            let reply = SERVICE.pp_call(MessageInfo::new(REQUEST_ADD, 2));
            if reply.label() == REPLY_OK && reply.count() == 1 {
                let result = get_mr(0);
                set_mr(0, result);
                return Ok(MessageInfo::new(REPLY_OK, 1));
            }
            return Ok(reply);
        }
        if operation == REQUEST_CONSOLE_CRASH && count == 0 {
            if self.crash_inflight || self.crash_request_pending || self.handled_faults == 0 {
                return Ok(MessageInfo::new(REPLY_CONSOLE_ERROR, 0));
            }
            let Some(counter) = self.read_addition_count() else {
                return Ok(MessageInfo::new(REPLY_CONSOLE_ERROR, 0));
            };
            self.counter_before_crash = counter;
            self.crash_request_pending = true;
            return Ok(MessageInfo::new(REPLY_OK, 0));
        }
        Ok(MessageInfo::new(REPLY_CONSOLE_ERROR, 0))
    }

    fn fault(
        &mut self,
        child: Child,
        message: MessageInfo,
    ) -> Result<Option<MessageInfo>, Self::Error> {
        self.handle_fault(child, message);
        // Deliberately leave the faulting thread stopped; never resume its store.
        Ok(None)
    }
}
