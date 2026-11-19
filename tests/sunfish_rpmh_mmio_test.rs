//! Exercise the real RSC adapter on canary-filled RAM, not physical hardware.
//! W1C, idle/completion transitions, and ordering need separate device evidence.

extern crate self as personal_os_rust_server;

#[path = "../rust/src/pixel_rpmh_controller.rs"]
mod pixel_rpmh_controller;
#[path = "../rust/src/qcom_rpmh_read.rs"]
mod qcom_rpmh_read;

use pixel_rpmh_controller::Controller;
use qcom_rpmh_read::{
    CxRead, Error, OBSERVED_CONFIG, ReadResource, Register, RegisterIo, ResourceRead,
};

#[test]
fn actual_adapter_fixed_votes_have_only_the_selected_command_footprint() {
    use qcom_rpmh_read::{ActiveTransaction, Command};
    for (command, resource, data) in [
        (Command::CxTurbo, ReadResource::Cx, 6),
        (Command::MssTurbo, ReadResource::Mss, 7),
        (Command::XoOn, ReadResource::Xo, 3),
    ] {
        let mut ram = ready();
        let before = ram;
        let mut io = unsafe { Controller::new(ram.as_mut_ptr()) };
        ActiveTransaction::new(command, 1).start(&mut io).unwrap();
        for index in 0..1024 {
            let expected = match index * 4 {
                0xd00 => 7,
                0xd10 => 0,
                0xd14 => 0x1010000,
                0xd1c => 1,
                0xd30 => 0x10108,
                0xd34 => resource.address(),
                0xd38 => data,
                _ => before[index],
            };
            assert_eq!(
                ram[index],
                expected,
                "unexpected CX vote write {:x}",
                index * 4
            );
        }
    }
}

fn ready() -> [u32; 1024] {
    let mut ram = [0xa5a5_a5a5; 1024];
    for (r, v) in [
        (Register::Config, OBSERVED_CONFIG),
        (Register::IrqEnable, 0x6),
        (Register::IrqStatus, 0),
        (Register::Idle, 1),
        (Register::OtherIdle, 1),
        (Register::Enable, 0),
        (Register::OtherEnable, 0),
        (Register::Wait, 0),
        (Register::OtherWait, 0),
        (Register::Control, 0),
        (Register::OtherControl, 0),
    ] {
        ram[r.offset() / 4] = v;
    }
    ram
}

#[test]
fn actual_adapter_submits_only_allowlisted_reads_in_first_active_bank() {
    for resource in ReadResource::ALL {
        let mut ram = ready();
        let before = ram;
        let mut io = unsafe { Controller::new(ram.as_mut_ptr()) };
        ResourceRead::for_resource(resource, 1)
            .start(&mut io)
            .unwrap();
        for index in 0..1024 {
            let expected = match index * 4 {
                0xd00 => 7, // Set only active0 IRQ bit, preserve other bits.
                0xd10 => 0,
                0xd14 => 0x1010000,
                0xd1c => 1,
                0xd30 => 0x108, // WRITE bit clear.
                0xd34 => resource.address(),
                0xd38 => 0,
                _ => before[index],
            };
            assert_eq!(
                ram[index],
                expected,
                "unexpected write at byte offset {:x}",
                index * 4
            );
        }
    }
}

#[test]
fn actual_adapter_refuses_dirty_bank_without_modifying_any_word() {
    for (r, v) in [
        (Register::Enable, 1),
        (Register::OtherEnable, 1),
        (Register::IrqStatus, 1),
        (Register::Idle, 0),
        (Register::OtherControl, 2),
    ] {
        let mut ram = ready();
        ram[r.offset() / 4] = v;
        let before = ram;
        let mut io = unsafe { Controller::new(ram.as_mut_ptr()) };
        assert_eq!(CxRead::new(1).start(&mut io), Err(Error::Handoff));
        assert_eq!(ram, before);
    }
}

#[test]
fn actual_adapter_timeout_does_not_retrigger_disable_or_clear_an_uncertain_command() {
    let mut ram = ready();
    let mut io = unsafe { Controller::new(ram.as_mut_ptr()) };
    let mut read = CxRead::new(1);
    read.start(&mut io).unwrap();
    let submitted = ram;
    assert_eq!(read.poll(&mut io), Ok(false));
    assert_eq!(read.poll(&mut io), Err(Error::PollLimit));
    assert_eq!(ram, submitted);
    assert_eq!(read.response(), None);
}

#[test]
fn actual_adapter_only_clears_its_completion_bit_and_refuses_unverified_ram_cleanup() {
    let mut ram = ready();
    let mut io = unsafe { Controller::new(ram.as_mut_ptr()) };
    let mut read = CxRead::new(1);
    read.start(&mut io).unwrap();
    ram[Register::IrqStatus.offset() / 4] = 5;
    ram[Register::CommandStatus.offset() / 4] = 0x10100;
    ram[Register::Response.offset() / 4] = 7;
    let submitted = ram;
    // Ordinary RAM cannot implement W1C; cleanup must fail, not report PASS.
    assert_eq!(read.poll(&mut io), Err(Error::Cleanup));
    assert_eq!(read.response(), None);
    for index in 0..1024 {
        let expected = match index * 4 {
            0xd00 => 6,
            0xd08 => 1,
            0xd10 | 0xd14 | 0xd1c => 0,
            _ => submitted[index],
        };
        assert_eq!(
            ram[index],
            expected,
            "unexpected cleanup write {:x}",
            index * 4
        );
    }
    assert_eq!(io.read(Register::IrqStatus), 5);
}
