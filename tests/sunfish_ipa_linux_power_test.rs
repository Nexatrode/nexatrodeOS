#[path = "../rust/src/qcom_bcm.rs"]
pub mod qcom_bcm;
#[path = "../rust/src/qcom_bcm_vote.rs"]
pub mod qcom_bcm_vote;
#[path = "../rust/src/qcom_cmd_db.rs"]
pub mod qcom_cmd_db;
#[path = "../rust/src/qcom_rpmh_bus.rs"]
pub mod qcom_rpmh_bus;
#[path = "../rust/src/qcom_rpmh_handoff.rs"]
pub mod qcom_rpmh_handoff;
#[allow(dead_code)] // Legacy power commands are exercised by their own suites.
#[path = "../rust/src/qcom_rpmh_read.rs"]
pub mod qcom_rpmh_read;
#[path = "../rust/src/sunfish_ipa_linux_power.rs"]
pub mod sunfish_ipa_linux_power;
use qcom_rpmh_handoff::{ReadRegisters, Register as SnapshotRegister};
use qcom_rpmh_read::{BcmTargetName, Register, RegisterIo};
use sunfish_ipa_linux_power::*;

fn metadata() -> [qcom_bcm::Metadata; COUNT] {
    core::array::from_fn(|i| {
        let name = BcmTargetName::IPA_PATHS[i].as_str();
        let mut id = [0; 8];
        id[..name.len()].copy_from_slice(name.as_bytes());
        qcom_bcm::Metadata {
            id,
            address: 0x50000 + i as u32 * 4,
            unit_size: 1000,
            width: if name == "ACV" { 0 } else { 8 },
            clock_domain: (i % 4) as u8,
            reserved: 0,
            auxiliary_version: 0x100,
        }
    })
}
struct Fake {
    words: [u32; 0x3000 / 4],
    values: [u32; COUNT],
    writes: Vec<(usize, u32)>,
    requests: Vec<(u32, u32, bool)>,
    timeout: bool,
    suppress_vote: Option<usize>,
}
impl Fake {
    fn new() -> Self {
        let mut s = Self {
            words: [0; 0x3000 / 4],
            values: [0x20000002; COUNT],
            writes: vec![],
            requests: vec![],
            timeout: false,
            suppress_vote: None,
        };
        s.words[0xc / 4] = qcom_rpmh_read::OBSERVED_CONFIG;
        s.words[0xd00 / 4] = 6;
        for b in 0..8 {
            s.words[(0xd18 + b * 0x2a0) / 4] = 1;
        }
        s
    }
}
impl RegisterIo for Fake {
    fn read(&mut self, r: Register) -> u32 {
        self.words[r.offset() / 4]
    }
    fn write(&mut self, r: Register, v: u32) {
        let off = r.offset();
        self.writes.push((off, v));
        self.words[off / 4] = v;
        if off == 0xd08 {
            self.words[0xd04 / 4] &= !v;
        }
        if off == 0xd14 && v & (1 << 24) != 0 && !self.timeout {
            let addr = self.words[0xd34 / 4];
            let data = self.words[0xd38 / 4];
            let write = self.words[0xd30 / 4] & (1 << 16) != 0;
            let i = ((addr - 0x50000) / 4) as usize;
            self.requests.push((addr, data, write));
            if write && self.suppress_vote != Some(i) {
                self.values[i] = data & !(1 << 30);
            }
            self.words[0xd40 / 4] = self.values[i];
            self.words[0xd3c / 4] = (1 << 8) | (1 << 16);
            self.words[0xd04 / 4] |= 1;
        }
    }
}
impl ReadRegisters for Fake {
    fn read(&mut self, r: SnapshotRegister) -> u32 {
        self.words[r.offset() / 4]
    }
}

#[test]
fn linux_ipa_units_and_route_scaling_differ_from_legacy_profile() {
    let p = plan(metadata(), [0; COUNT]).unwrap();
    let find = |n| p.iter().find(|f| f.target().name() == n).unwrap().vector();
    assert_eq!(
        find(BcmTargetName::Ip0),
        qcom_bcm_vote::Vector { x: 0, y: 100 }
    ); // 100 MHz / (1000 kHz)
    assert_eq!(
        find(BcmTargetName::Acv),
        qcom_bcm_vote::Vector { x: 0, y: 8 }
    );
    assert_eq!(
        find(BcmTargetName::Sh0),
        qcom_bcm_vote::Vector { x: 16383, y: 16383 }
    ); // explicit source saturation
    let mut m = metadata();
    for x in &mut m {
        x.unit_size = 1_000_000;
    }
    let p = plan(m, [0; COUNT]).unwrap();
    let sh = p
        .iter()
        .find(|f| f.target().name() == BcmTargetName::Sh0)
        .unwrap();
    assert_eq!(sh.vector(), qcom_bcm_vote::Vector { x: 20, y: 232 });
    assert_eq!(p[COUNT - 1].target().name(), BcmTargetName::Ip0);
}

#[test]
fn native_floor_never_reduces_observed_vectors_or_acv_bits() {
    let mut before = [0x200a0020; COUNT];
    before[COUNT - 1] = 0x200000ff;
    let p = plan(metadata(), before).unwrap();
    for f in p {
        let i = BcmTargetName::IPA_PATHS
            .iter()
            .position(|n| *n == f.target().name())
            .unwrap();
        let old = qcom_bcm_vote::Decoded::parse(before[i]).unwrap().vector;
        assert!(f.vector().x >= old.x);
        if f.target().name() == BcmTargetName::Acv {
            assert_eq!(f.vector().y & old.y, old.y);
        } else {
            assert!(f.vector().y >= old.y);
        }
    }
}

#[test]
fn coordinator_executes_actual_controller_sequence_clock_last_and_retains_tail() {
    let mut io = Fake::new();
    let tail = io.words[0x1000 / 4..].to_vec();
    let mut power = PowerOnce::new();
    let r = power.enable(&mut io, metadata(), 4, || {}, |_, _| {});
    assert_eq!(r.error, None);
    assert_eq!(r.completed, 0x7ff);
    let writes: Vec<_> = io.requests.iter().filter(|r| r.2).collect();
    assert_eq!(writes.len(), COUNT);
    assert_eq!(writes.last().unwrap().0, metadata()[COUNT - 1].address);
    assert_eq!(io.words[0xd00 / 4], 6);
    assert_eq!(io.words[0xd04 / 4], 0);
    assert_eq!(&io.words[0x1000 / 4..], &tail);
    let writes = io.writes.len();
    assert_eq!(
        power.enable(&mut io, metadata(), 4, || {}, |_, _| {}).error,
        Some(Error::AlreadyStarted)
    );
    assert_eq!(io.writes.len(), writes);
}

#[test]
fn invalid_last_metadata_and_baseline_fail_before_any_resource_write() {
    for bad_baseline in [false, true] {
        let mut io = Fake::new();
        let mut m = metadata();
        if bad_baseline {
            io.values[COUNT - 1] = 0x20004001;
        } else {
            m[COUNT - 1].id[0] = b'X';
        }
        let r = PowerOnce::new().enable(&mut io, m, 4, || {}, |_, _| {});
        assert!(r.error.is_some());
        assert!(!io.requests.iter().any(|r| r.2));
    }
}

#[test]
fn short_clock_readback_stops_once_and_never_replays_a_failed_vote() {
    let mut io = Fake::new();
    io.suppress_vote = Some(COUNT - 1);
    let mut p = PowerOnce::new();
    let r = p.enable(&mut io, metadata(), 4, || {}, |_, _| {});
    assert_eq!(r.error, Some(Error::Readback));
    assert_eq!(r.failed_resource, Some(BcmTargetName::Ip0));
    assert_eq!(
        io.requests
            .iter()
            .filter(|r| r.2 && r.0 == metadata()[COUNT - 1].address)
            .count(),
        1
    );
    let count = io.writes.len();
    p.enable(&mut io, metadata(), 4, || {}, |_, _| {});
    assert_eq!(count, io.writes.len());
}
