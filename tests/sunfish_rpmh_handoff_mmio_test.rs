//! Actual read-only adapter against RAM: no hardware state transitions modeled.
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_rpmh_handoff.rs"]
mod pixel_rpmh_handoff;
#[path = "../rust/src/qcom_rpmh_handoff.rs"]
mod qcom_rpmh_handoff;

use qcom_rpmh_handoff::{
    BankField, Command, CommandField, MAPPING_BYTES, OBSERVED_CONFIG, Register, capture_stable,
};

#[test]
fn actual_read_only_adapter_preserves_every_word_and_reads_last_wake_slot() {
    let mut ram = [0x5a5a5a5a_u32; MAPPING_BYTES / 4];
    ram[Register::CONFIG.offset() / 4] = OBSERVED_CONFIG;
    for bank in 0..8 {
        ram[Register::bank(bank, BankField::Enable).unwrap().offset() / 4] = 0;
        ram[Register::bank(bank, BankField::Wait).unwrap().offset() / 4] = 0;
    }
    ram[Register::bank(7, BankField::Enable).unwrap().offset() / 4] = 1 << 15;
    ram[Register::command(7, 15, CommandField::Message)
        .unwrap()
        .offset()
        / 4] = 0x10108;
    ram[Register::command(7, 15, CommandField::Address)
        .unwrap()
        .offset()
        / 4] = 0x50000;
    ram[Register::command(7, 15, CommandField::Data)
        .unwrap()
        .offset()
        / 4] = 0x60003dcb;
    let before = ram;
    let mut io = unsafe { pixel_rpmh_handoff::Controller::new(ram.as_ptr()) };
    let snapshot = capture_stable(&mut io, 1).unwrap();
    assert_eq!(ram, before);
    assert_eq!(
        snapshot.banks[7].commands[15],
        Some(Command {
            message: 0x10108,
            address: 0x50000,
            data: 0x60003dcb
        })
    );
    assert_eq!(snapshot.banks[7].commands[..15], [None; 15]);
    assert_eq!(snapshot.irq_status, 0x5a5a5a5a);
}
