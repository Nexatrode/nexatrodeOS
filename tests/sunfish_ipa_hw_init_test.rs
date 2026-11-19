#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/sunfish_ipa_hw_init.rs"]
mod sunfish_ipa_hw_init;

use sunfish_ipa_hw_init::{
    Error, ExecuteError, REG_BCR, REG_CLKON_CFG, REG_COMP_CFG, REG_COUNTER_CFG,
    REG_FILT_ROUT_HASH_EN, REG_QSB_MAX_READS, REG_QSB_MAX_WRITES, REG_TX_CFG, REG_VERSION,
    RegisterIo, SUNFISH_IPA_HW_V4_2, Snapshot, execute, plan,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Read(u32),
    Write(u32, u32),
}

#[derive(Default)]
struct FakeRegisters {
    values: std::collections::BTreeMap<u32, u32>,
    operations: Vec<Operation>,
    fail_read_at: Option<u32>,
    fail_write_at: Option<u32>,
}

impl RegisterIo for FakeRegisters {
    type Error = &'static str;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error> {
        self.operations.push(Operation::Read(offset));
        if self.fail_read_at == Some(offset) {
            return Err("read failed");
        }
        self.values
            .get(&offset)
            .copied()
            .ok_or("uninitialized register")
    }

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error> {
        self.operations.push(Operation::Write(offset, value));
        if self.fail_write_at == Some(offset) {
            return Err("write failed");
        }
        self.values.insert(offset, value);
        Ok(())
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        hardware_type: SUNFISH_IPA_HW_V4_2,
        version: 1,
        tx_cfg: 0xffff_ffff,
        comp_cfg: 0x0005_4321,
    }
}

#[test]
fn plan_matches_the_source_order_and_ipa_v42_values() {
    let plan = plan(snapshot()).unwrap();
    let writes = plan.writes();
    assert_eq!(
        writes.map(|write| write.offset),
        [
            REG_BCR,
            REG_CLKON_CFG,
            REG_TX_CFG,
            REG_QSB_MAX_WRITES,
            REG_QSB_MAX_READS,
            REG_COUNTER_CFG,
            REG_COMP_CFG,
            REG_COMP_CFG,
            REG_FILT_ROUT_HASH_EN,
        ]
    );
    assert_eq!(
        writes.map(|write| write.value),
        [
            0,
            0x3000_0000,
            0x0001_effc,
            8,
            12,
            0x0000_00f0,
            0x0005_43a1,
            0x0005_c3a1,
            0,
        ]
    );
}

#[test]
fn plan_requires_the_v42_profile_and_a_reachable_version_register() {
    let mut input = snapshot();
    input.hardware_type = 15;
    assert_eq!(plan(input), Err(Error::WrongHardwareType));

    let mut input = snapshot();
    input.version = 0;
    assert_eq!(plan(input), Err(Error::VersionUnavailable));
}

#[test]
fn reconstructed_fields_clear_unmodeled_bits_and_disable_pa_mask() {
    let plan = plan(snapshot()).unwrap();
    let writes = plan.writes();
    let tx_cfg = writes
        .iter()
        .find(|write| write.offset == REG_TX_CFG)
        .unwrap();
    let comp_cfg = writes
        .iter()
        .filter(|write| write.offset == REG_COMP_CFG)
        .last()
        .unwrap();
    assert_eq!(tx_cfg.value & 0x1000, 0);
    assert_eq!(tx_cfg.value & !0x0001_fffc, 0);
    assert_eq!(comp_cfg.value & 0x0000_8180, 0x0000_8180);
    assert_eq!(comp_cfg.value & !0x001f_ffff, 0);
}

#[test]
fn executor_matches_linux_register_read_write_order() {
    let mut io = FakeRegisters::default();
    io.values.insert(REG_VERSION, 1);
    io.values.insert(REG_TX_CFG, 0xffff_ffff);
    io.values.insert(REG_COMP_CFG, 0x0005_4321);

    let receipt = execute(&mut io, SUNFISH_IPA_HW_V4_2).unwrap();
    assert_eq!(receipt.version, 1);
    assert_eq!(receipt.confirmed_writes, 9);
    assert_eq!(receipt.comp_cfg_readback, 0x0005_c3a1);
    assert_eq!(
        io.operations,
        [
            Operation::Read(REG_VERSION),
            Operation::Write(REG_BCR, 0),
            Operation::Write(REG_CLKON_CFG, 0x3000_0000),
            Operation::Read(REG_TX_CFG),
            Operation::Write(REG_TX_CFG, 0x0001_effc),
            Operation::Write(REG_QSB_MAX_WRITES, 8),
            Operation::Write(REG_QSB_MAX_READS, 12),
            Operation::Write(REG_COUNTER_CFG, 0x0000_00f0),
            Operation::Read(REG_COMP_CFG),
            Operation::Write(REG_COMP_CFG, 0x0005_43a1),
            Operation::Read(REG_COMP_CFG),
            Operation::Write(REG_COMP_CFG, 0x0005_c3a1),
            Operation::Read(REG_COMP_CFG),
            Operation::Write(REG_FILT_ROUT_HASH_EN, 0),
        ]
    );
}

#[test]
fn executor_rejects_wrong_hardware_before_mmio_and_zero_version_before_writes() {
    let mut io = FakeRegisters::default();
    assert_eq!(
        execute(&mut io, SUNFISH_IPA_HW_V4_2 - 1),
        Err(ExecuteError::WrongHardwareType)
    );
    assert!(io.operations.is_empty());

    io.values.insert(REG_VERSION, 0);
    assert_eq!(
        execute(&mut io, SUNFISH_IPA_HW_V4_2),
        Err(ExecuteError::VersionUnavailable)
    );
    assert_eq!(io.operations, [Operation::Read(REG_VERSION)]);
}

#[test]
fn executor_stops_on_partial_write_failure_and_reports_confirmed_prefix() {
    let mut io = FakeRegisters::default();
    io.values.insert(REG_VERSION, 1);
    io.values.insert(REG_TX_CFG, 0xffff_ffff);
    io.fail_write_at = Some(REG_QSB_MAX_WRITES);

    assert_eq!(
        execute(&mut io, SUNFISH_IPA_HW_V4_2),
        Err(ExecuteError::Write {
            offset: REG_QSB_MAX_WRITES,
            confirmed_writes: 3,
            source: "write failed",
        })
    );
    assert_eq!(
        io.operations.last(),
        Some(&Operation::Write(REG_QSB_MAX_WRITES, 8))
    );
    assert_eq!(io.values.get(&REG_BCR), Some(&0));
    assert_eq!(io.values.get(&REG_CLKON_CFG), Some(&0x3000_0000));
}
