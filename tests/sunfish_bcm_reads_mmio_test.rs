//! Real combined adapter against canary RAM, not physical hardware.
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_rpmh_bus.rs"]
mod pixel_rpmh_bus;
#[path = "../rust/src/qcom_rpmh_bus.rs"]
mod qcom_rpmh_bus;
#[path = "../rust/src/qcom_rpmh_handoff.rs"]
mod qcom_rpmh_handoff;
#[path = "../rust/src/qcom_rpmh_read.rs"]
mod qcom_rpmh_read;

#[test]
fn combined_adapter_submits_only_fixed_bus_reads_and_preserves_controller_tail() {
    use qcom_rpmh_read::{BcmRead, BcmResource, OBSERVED_CONFIG};
    for resource in BcmResource::ALL {
        let mut ram = [0xa5a5a5a5_u32; 0x3000 / 4];
        ram[0xc / 4] = OBSERVED_CONFIG;
        ram[0xd00 / 4] = 6;
        ram[0xd04 / 4] = 0;
        for bank in 0..8 {
            let base = 0xd00 + bank * 0x2a0;
            for (offset, value) in [(0x10, 0), (0x14, 0), (0x18, 1), (0x1c, 0)] {
                ram[(base + offset) / 4] = value;
            }
        }
        let before = ram;
        let mut io = unsafe { pixel_rpmh_bus::Controller::new(ram.as_mut_ptr()) };
        let captured = qcom_rpmh_handoff::capture_stable(&mut io, 1).unwrap();
        assert_eq!(captured.banks[7].idle, 1);
        assert_eq!(ram, before);
        BcmRead::new(resource, 1).start(&mut io).unwrap();
        for (index, value) in ram.iter().enumerate() {
            let expected = match index * 4 {
                0xd00 => 7,
                0xd10 => 0,
                0xd14 => 0x1010000,
                0xd1c => 1,
                0xd30 => 0x108,
                0xd34 => resource.address(),
                0xd38 => 0,
                _ => before[index],
            };
            assert_eq!(*value, expected, "unexpected WRITE at {:x}", index * 4);
        }
    }
}

#[test]
fn fixed_bus_writes_use_commit_wait_and_cannot_modify_sleep_wake_pages() {
    use qcom_rpmh_read::{ActiveTransaction, Command, MC0_REASSERT_WORD, OBSERVED_CONFIG};
    for (command, address, data) in [
        (Command::Mc0Reassert, 0x50000, MC0_REASSERT_WORD),
        (Command::Sn11PasFloor, 0x5006c, 0x613c5d02),
        (Command::Sn0PasFloor, 0x50078, 0x613c5d02),
        (Command::Ce0PasFloor, 0x500a0, 0x6fffffff),
    ] {
        let mut ram = [0xa5a5a5a5_u32; 0x3000 / 4];
        ram[0xc / 4] = OBSERVED_CONFIG;
        ram[0xd00 / 4] = 6;
        ram[0xd04 / 4] = 0;
        for bank in 0..8 {
            let base = 0xd00 + bank * 0x2a0;
            for (offset, value) in [(0x10, 0), (0x14, 0), (0x18, 1), (0x1c, 0)] {
                ram[(base + offset) / 4] = value;
            }
        }
        let before = ram;
        let mut io = unsafe { pixel_rpmh_bus::Controller::new(ram.as_mut_ptr()) };
        ActiveTransaction::new(command, 1).start(&mut io).unwrap();
        for (index, value) in ram.iter().enumerate() {
            let expected = match index * 4 {
                0xd00 => 7,
                0xd10 => 1,
                0xd14 => 0x1010000,
                0xd1c => 1,
                0xd30 => 0x10108,
                0xd34 => address,
                0xd38 => data,
                _ => before[index],
            };
            assert_eq!(*value, expected, "unexpected WRITE at {:x}", index * 4);
        }
    }
}
