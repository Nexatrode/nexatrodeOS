#![allow(dead_code)]
#[cfg(mpss_power_tests)]
#[path = "sunfish_mpss_linux_power_test.rs"]
mod mpss;
#[path = "../rust/src/qcom_bcm.rs"]
mod qcom_bcm;
#[path = "../rust/src/qcom_bcm_vote.rs"]
mod qcom_bcm_vote;
#[path = "../rust/src/qcom_cmd_db.rs"]
mod qcom_cmd_db;
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;
#[cfg(mpss_power_tests)]
#[path = "../rust/src/qcom_qmp.rs"]
mod qcom_qmp;
#[cfg(mpss_power_tests)]
#[path = "../rust/src/qcom_qmp_link.rs"]
mod qcom_qmp_link;
#[path = "../rust/src/qcom_rpmh_bus.rs"]
mod qcom_rpmh_bus;
#[path = "../rust/src/qcom_rpmh_handoff.rs"]
mod qcom_rpmh_handoff;
#[path = "../rust/src/qcom_rpmh_read.rs"]
mod qcom_rpmh_read;
#[path = "../rust/src/sunfish_ipa_fws.rs"]
mod sunfish_ipa_fws;
#[path = "../rust/src/sunfish_ipa_linux_pas.rs"]
mod sunfish_ipa_linux_pas;
#[path = "../rust/src/sunfish_ipa_pas.rs"]
mod sunfish_ipa_pas;
#[cfg(mpss_power_tests)]
#[path = "../rust/src/sunfish_mpss_linux_power.rs"]
mod sunfish_mpss_linux_power;
#[cfg(mpss_power_tests)]
#[path = "../rust/src/sunfish_mpss_linux_power_native.rs"]
mod sunfish_mpss_linux_power_native;
#[path = "../rust/src/sunfish_scm_linux_power.rs"]
mod sunfish_scm_linux_power;
use qcom_rpmh_handoff::{ReadRegisters, Register as GuardRegister};
use qcom_rpmh_read::{BcmResource, Register, RegisterIo};
use sunfish_ipa_linux_pas::Resources;
use sunfish_scm_linux_power::*;
fn metadata() -> [qcom_bcm::Metadata; 6] {
    core::array::from_fn(|i| {
        let mut id = [0; 8];
        id[..NAMES[i].len()].copy_from_slice(NAMES[i].as_bytes());
        qcom_bcm::Metadata {
            id,
            address: BcmResource::ALL[i].address(),
            unit_size: 1000,
            width: if i == 1 { 0 } else { 8 },
            clock_domain: (i % 4) as u8,
            reserved: 0,
            auxiliary_version: 0x100,
        }
    })
}
struct Fake {
    words: [u32; 0x3000 / 4],
    values: [u32; 6],
    requests: Vec<(u32, u32, bool)>,
    writes: usize,
    timeout_at: Option<usize>,
    suppress: bool,
    rails: [u32; 4],
    started: Option<std::rc::Rc<std::cell::Cell<bool>>>,
}
impl Fake {
    fn new() -> Self {
        let mut s = Self {
            words: [0; 0x3000 / 4],
            values: [0x20004002; 6],
            requests: vec![],
            writes: 0,
            timeout_at: None,
            suppress: false,
            rails: [5, 0, 3, 3],
            started: None,
        };
        s.words[0xc / 4] = qcom_rpmh_read::OBSERVED_CONFIG;
        s.words[0xd00 / 4] = 6;
        for bank in 0..8 {
            s.words[(0xd18 + bank * 0x2a0) / 4] = 1;
        }
        s
    }
}
impl RegisterIo for Fake {
    fn read(&mut self, r: Register) -> u32 {
        self.words[r.offset() / 4]
    }
    fn write(&mut self, r: Register, v: u32) {
        if let Some(started) = self.started.as_ref() {
            assert!(started.get(), "RSC effect preceded AOP ON");
        }
        self.writes += 1;
        let off = r.offset();
        self.words[off / 4] = v;
        if off == 0xd08 {
            self.words[0xd04 / 4] &= !v;
        }
        if off == 0xd14 && v & (1 << 24) != 0 {
            let address = self.words[0xd34 / 4];
            let data = self.words[0xd38 / 4];
            let write = self.words[0xd30 / 4] & (1 << 16) != 0;
            let index = BcmResource::ALL.iter().position(|r| r.address() == address);
            let values: &mut [u32] = if index.is_some() {
                &mut self.values
            } else {
                &mut self.rails
            };
            let index = index.unwrap_or_else(|| {
                qcom_rpmh_read::ReadResource::ALL
                    .iter()
                    .position(|r| r.address() == address)
                    .unwrap()
            });
            let count = self.requests.len();
            self.requests.push((address, data, write));
            if self.timeout_at == Some(count) {
                return;
            }
            if write && !self.suppress {
                values[index] = data & !(1 << 30);
            }
            self.words[0xd40 / 4] = values[index];
            self.words[0xd3c / 4] = (1 << 8) | (1 << 16);
            self.words[0xd04 / 4] |= 1;
        }
        if off == 0xd20 && v == 0 {
            self.words[0xd3c / 4] = 0;
        }
    }
}
impl ReadRegisters for Fake {
    fn read(&mut self, r: GuardRegister) -> u32 {
        self.words[r.offset() / 4]
    }
}
#[test]
fn source_max_peak_preserves_x_and_mask_and_saturates_only_numeric_y() {
    let before = [0x20004002; 6];
    let floors = plan(metadata(), before).unwrap();
    for f in floors {
        assert_eq!(f.vector.x, 1);
        assert_eq!(
            f.vector.y,
            if f.resource == BcmResource::Acv {
                10
            } else {
                0x3fff
            }
        );
        assert_eq!(f.data, 0x60004000 | f.vector.y as u32);
    }
    let mut bad = metadata();
    bad[0].unit_size = 0;
    assert_eq!(plan(bad, before), Err(Error::Metadata));
    let mut bad = metadata();
    bad[2].address += 4;
    assert_eq!(plan(bad, before), Err(Error::Metadata));
    let mut bad = metadata();
    bad[2].clock_domain = 16;
    assert_eq!(plan(bad, before), Err(Error::Metadata));
    let mut bad = metadata();
    bad[2].width = 0;
    assert_eq!(plan(bad, before), Err(Error::Metadata));
    let mut bad = before;
    bad[0] = 1;
    assert_eq!(plan(metadata(), bad), Err(Error::Baseline));
}
#[test]
fn real_transaction_path_holds_all_six_and_call_leases_never_unvote() {
    let mut io = Fake::new();
    let mut hold = HoldOnce::new();
    assert!(HeldResources::new(&hold, &mut io, 8).is_err());
    hold.enable(&mut io, metadata(), 8, || {}, |_| {}).unwrap();
    assert_eq!(
        (hold.report().submitted, hold.report().completed),
        (0x3f, 0x3f)
    );
    assert_eq!(io.requests.iter().filter(|r| r.2).count(), 6);
    let before = io.values;
    {
        let mut provider = HeldResources::new(&hold, &mut io, 8).unwrap();
        for _ in 0..3 {
            let lease = provider.acquire().unwrap();
            provider.release(lease).unwrap();
        }
    }
    assert_eq!(io.values, before);
    assert_eq!(io.requests.iter().filter(|r| r.2).count(), 6);
    let count = io.writes;
    assert_eq!(
        hold.enable(&mut io, metadata(), 8, || {}, |_| {}),
        Err(Error::Consumed)
    );
    assert_eq!(io.writes, count);
}
#[test]
fn readback_or_uncertain_transaction_failure_retains_partial_state_without_retry() {
    for timeout in [true, false] {
        let mut io = Fake::new();
        if timeout {
            io.timeout_at = Some(12);
        } else {
            io.suppress = true;
        }
        let mut hold = HoldOnce::new();
        assert!(hold.enable(&mut io, metadata(), 8, || {}, |_| {}).is_err());
        assert_eq!((hold.report().submitted, hold.report().completed), (1, 0));
        assert!(HeldResources::new(&hold, &mut io, 8).is_err());
        let count = io.writes;
        assert_eq!(
            hold.enable(&mut io, metadata(), 8, || {}, |_| {}),
            Err(Error::Consumed)
        );
        assert_eq!(io.writes, count);
    }
}
#[test]
fn live_handoff_change_or_lost_floor_blocks_lease_before_secure_call() {
    for handoff in [true, false] {
        let mut io = Fake::new();
        let mut hold = HoldOnce::new();
        hold.enable(&mut io, metadata(), 8, || {}, |_| {}).unwrap();
        if handoff {
            io.words[(0xd1c + 0x2a0) / 4] = 1;
        } else {
            io.values[2] = 0x20000000;
        }
        let count = io.requests.len();
        let expected = if handoff {
            0
        } else {
            plan(metadata(), [0x20004002; 6])
                .unwrap()
                .iter()
                .position(|f| f.resource == BcmResource::Ce0)
                .unwrap()
                + 1
        };
        let mut provider = HeldResources::new(&hold, &mut io, 8).unwrap();
        assert!(provider.acquire().is_err());
        assert!(provider.acquire().is_err());
        drop(provider);
        assert_eq!(io.requests.len() - count, expected);
        // Reconstructing a proxy cannot clear the owner's terminal failure.
        assert!(HeldResources::new(&hold, &mut io, 8).is_err());
    }
}

#[test]
fn dropping_a_proxy_with_an_outstanding_lease_cannot_reset_lease_identity() {
    let mut io = Fake::new();
    let mut hold = HoldOnce::new();
    hold.enable(&mut io, metadata(), 8, || {}, |_| {}).unwrap();
    let mut provider = HeldResources::new(&hold, &mut io, 8).unwrap();
    let lease = provider.acquire().unwrap();
    drop(provider);
    assert!(HeldResources::new(&hold, &mut io, 8).is_err());
    drop(lease); // no hidden unvote or lease reset on Drop
    assert!(HeldResources::new(&hold, &mut io, 8).is_err());
}
