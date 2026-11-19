//! Restricted BCM READ transactions with full TCS programming preservation.
//!
//! Uses the observed six BCM addresses and single-command RPMh READ protocol.
//! No resource WRITE, vote, retry, cancellation, or inferred rollback. Caller
//! must own the standalone AP DRV2 endpoint; an empty snapshot is not authority.

use crate::qcom_rpmh_handoff::{self as handoff, ReadRegisters};
use crate::qcom_rpmh_read::{
    BcmRead, BcmResource, BcmTarget, BcmTargetName, Error as ControllerError, RegisterIo, Snapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    WorkLimit,
    BeforeCapture,
    Handoff,
    Controller,
    AfterCapture,
    Preservation,
    TargetList,
}
impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::WorkLimit => 1,
            Self::BeforeCapture => 2,
            Self::Handoff => 3,
            Self::Controller => 4,
            Self::AfterCapture => 5,
            Self::Preservation => 6,
            Self::TargetList => 7,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub responses: [[Option<u32>; 6]; 2],
    pub submitted: u8,
    pub completed: u8,
    pub handoff_checks: u8,
    pub before: Option<handoff::Snapshot>,
    pub after: Option<handoff::Snapshot>,
    pub state: Option<Snapshot>,
    pub error: Option<Error>,
    pub controller_error: Option<ControllerError>,
    pub handoff_error: Option<handoff::Error>,
    pub failed_resource: Option<BcmResource>,
    pub failed_pass: Option<u8>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.completed == 12
            && self.submitted == 12
            && self.handoff_checks == 24
            && self.before.is_some()
            && self.before == self.after
            && self.responses.iter().flatten().all(Option::is_some)
    }
}

fn empty(state: handoff::Snapshot) -> bool {
    state.irq_status == 0
        && state
            .banks
            .iter()
            .all(|bank| bank.enable == 0 && bank.wait == 0 && bank.control == 0 && bank.idle == 1)
}

/// Two fixed passes. Before each READ, require the same stable empty handoff;
/// publish its response only after transaction cleanup and a matching full
/// snapshot. First failure is terminal, even if firmware replies later.
pub fn read_bcm_series<I: RegisterIo + ReadRegisters>(
    io: &mut I,
    polls: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(u8, BcmResource),
) -> Report {
    let mut report = Report {
        responses: [[None; 6]; 2],
        submitted: 0,
        completed: 0,
        handoff_checks: 0,
        before: None,
        after: None,
        state: None,
        error: None,
        controller_error: None,
        handoff_error: None,
        failed_resource: None,
        failed_pass: None,
    };
    if polls == 0 {
        report.error = Some(Error::WorkLimit);
        return report;
    }
    for pass in 0..2 {
        for resource in BcmResource::ALL {
            report.failed_pass = Some(pass as u8);
            report.failed_resource = Some(resource);
            let before = match handoff::capture_stable(io, 3) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    report.error = Some(Error::BeforeCapture);
                    report.handoff_error = Some(error);
                    return report;
                }
            };
            report.handoff_checks += 1;
            report.state = Some(Snapshot::read(io));
            let initial = *report.before.get_or_insert(before);
            report.after = Some(before);
            if !empty(before) || initial != before {
                report.error = Some(Error::Handoff);
                return report;
            }
            progress(pass as u8, resource);
            let mut read = BcmRead::new(resource, polls);
            let transaction = read.start(io).and_then(|()| {
                report.submitted += 1;
                loop {
                    if read.poll(io)? {
                        return Ok(());
                    }
                    wait();
                }
            });
            report.state = Some(read.last());
            if let Err(error) = transaction {
                report.state = Some(Snapshot::read(io)); // Non-clearing diagnostics only.
                report.error = Some(Error::Controller);
                report.controller_error = Some(error);
                return report;
            }
            let after = match handoff::capture_stable(io, 3) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    report.error = Some(Error::AfterCapture);
                    report.handoff_error = Some(error);
                    return report;
                }
            };
            report.handoff_checks += 1;
            report.after = Some(after);
            if after != before {
                report.error = Some(Error::Preservation);
                return report;
            }
            report.responses[pass][resource.index()] = read.response();
            report.completed += 1;
        }
    }
    report.failed_resource = None;
    report.failed_pass = None;
    report
}

pub const IPA_PATH_BCM_COUNT: usize = BcmTargetName::IPA_PATHS.len();

/// Result of two complete read-only passes over the eleven BCMs selected by
/// the source-validated IPA routes. Values remain opaque raw RPMh responses.
#[derive(Clone, Copy, Debug)]
pub struct IpaBcmReport {
    pub responses: [[Option<u32>; IPA_PATH_BCM_COUNT]; 2],
    pub submitted: u8,
    pub completed: u8,
    pub handoff_checks: u8,
    pub before: Option<handoff::Snapshot>,
    pub after: Option<handoff::Snapshot>,
    pub state: Option<Snapshot>,
    pub error: Option<Error>,
    pub controller_error: Option<ControllerError>,
    pub handoff_error: Option<handoff::Error>,
    pub failed_target: Option<BcmTargetName>,
    pub failed_pass: Option<u8>,
}

impl IpaBcmReport {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && usize::from(self.completed) == IPA_PATH_BCM_COUNT * 2
            && usize::from(self.submitted) == IPA_PATH_BCM_COUNT * 2
            && self.handoff_checks as usize == IPA_PATH_BCM_COUNT * 4
            && self.before.is_some()
            && self.before == self.after
            && self.responses.iter().flatten().all(Option::is_some)
    }
}

/// Read the route-selected BCMs twice through the existing Apps-RSC READ
/// transaction. The CMD_DB-derived targets must be complete and in the
/// fixed source-validated order before the first controller write.
pub fn read_ipa_path_bcms<I: RegisterIo + ReadRegisters>(
    io: &mut I,
    targets: [BcmTarget; IPA_PATH_BCM_COUNT],
    polls: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(u8, BcmTarget),
) -> IpaBcmReport {
    let mut report = IpaBcmReport {
        responses: [[None; IPA_PATH_BCM_COUNT]; 2],
        submitted: 0,
        completed: 0,
        handoff_checks: 0,
        before: None,
        after: None,
        state: None,
        error: None,
        controller_error: None,
        handoff_error: None,
        failed_target: None,
        failed_pass: None,
    };
    if polls == 0 {
        report.error = Some(Error::WorkLimit);
        return report;
    }
    if targets
        .iter()
        .zip(BcmTargetName::IPA_PATHS)
        .any(|(target, expected)| target.name() != expected)
        || targets.iter().enumerate().any(|(index, target)| {
            targets[index + 1..]
                .iter()
                .any(|other| other.address() == target.address())
        })
    {
        report.error = Some(Error::TargetList);
        return report;
    }
    for pass in 0..2 {
        for (index, target) in targets.iter().copied().enumerate() {
            report.failed_pass = Some(pass as u8);
            report.failed_target = Some(target.name());
            let before = match handoff::capture_stable(io, 3) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    report.error = Some(Error::BeforeCapture);
                    report.handoff_error = Some(error);
                    return report;
                }
            };
            report.handoff_checks += 1;
            report.state = Some(Snapshot::read(io));
            let initial = *report.before.get_or_insert(before);
            report.after = Some(before);
            if !empty(before) || initial != before {
                report.error = Some(Error::Handoff);
                return report;
            }
            progress(pass as u8, target);
            let mut read = BcmRead::for_target(target, polls);
            let transaction = read.start(io).and_then(|()| {
                report.submitted += 1;
                loop {
                    if read.poll(io)? {
                        return Ok(());
                    }
                    wait();
                }
            });
            report.state = Some(read.last());
            if let Err(error) = transaction {
                report.state = Some(Snapshot::read(io));
                report.error = Some(Error::Controller);
                report.controller_error = Some(error);
                return report;
            }
            let after = match handoff::capture_stable(io, 3) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    report.error = Some(Error::AfterCapture);
                    report.handoff_error = Some(error);
                    return report;
                }
            };
            report.handoff_checks += 1;
            report.after = Some(after);
            if after != before {
                report.error = Some(Error::Preservation);
                return report;
            }
            report.responses[pass][index] = read.response();
            report.completed += 1;
        }
    }
    report.failed_target = None;
    report.failed_pass = None;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_handoff::{
        BankField, MAPPING_BYTES, OBSERVED_CONFIG, Register as GuardRegister,
    };
    use crate::qcom_rpmh_read::{BcmTargetName, READ_MESSAGE, Register};
    use std::vec::Vec;

    struct Fake {
        words: [u32; MAPPING_BYTES / 4],
        writes: Vec<(Register, u32)>,
        requests: Vec<u32>,
        stop_at: Option<usize>,
        change_after: Option<usize>,
        captures: usize,
        change_before_capture: Option<usize>,
        unstable: bool,
    }
    impl Fake {
        fn ready() -> Self {
            let mut f = Self {
                words: [0; MAPPING_BYTES / 4],
                writes: Vec::new(),
                requests: Vec::new(),
                stop_at: None,
                change_after: None,
                captures: 0,
                change_before_capture: None,
                unstable: false,
            };
            f.words[0xc / 4] = OBSERVED_CONFIG;
            f.words[0xd00 / 4] = 1;
            for bank in 0..8 {
                f.words[GuardRegister::bank(bank, BankField::Idle).unwrap().offset() / 4] = 1;
            }
            f
        }
        fn set(&mut self, r: Register, v: u32) {
            self.words[r.offset() / 4] = v;
        }
    }

    fn ipa_targets() -> [BcmTarget; IPA_PATH_BCM_COUNT] {
        core::array::from_fn(|index| {
            BcmTarget::new(BcmTargetName::IPA_PATHS[index], 0x50000 + index as u32 * 4).unwrap()
        })
    }
    impl RegisterIo for Fake {
        fn read(&mut self, r: Register) -> u32 {
            self.words[r.offset() / 4]
        }
        fn write(&mut self, r: Register, v: u32) {
            self.writes.push((r, v));
            if r == Register::IrqClear {
                self.words[Register::IrqStatus.offset() / 4] &= !v;
                if self.change_after == Some(self.requests.len()) {
                    self.words[GuardRegister::bank(7, BankField::Control).unwrap().offset() / 4] =
                        0x10000;
                }
            } else {
                self.set(r, v);
            }
            if r == Register::Control && v == 0 {
                self.set(Register::CommandStatus, 0);
            }
            if r == Register::Control && v == 0x1010000 {
                assert_eq!(self.words[Register::Message.offset() / 4], READ_MESSAGE);
                assert_eq!(self.words[Register::Data.offset() / 4], 0);
                let address = self.words[Register::Address.offset() / 4];
                assert!(
                    BcmResource::ALL.iter().any(|r| r.address() == address)
                        || (0x50000..=0x5fffc).contains(&address) && address & 3 == 0
                );
                self.requests.push(address);
                self.set(Register::Idle, 0);
                if self.stop_at == Some(self.requests.len()) {
                    return;
                }
                self.set(Register::IrqStatus, 1);
                self.set(Register::CommandStatus, 0x10100);
                self.set(
                    Register::Response,
                    if self.requests.len() == 1 {
                        0
                    } else {
                        0xa0000000 | self.requests.len() as u32
                    },
                );
                self.set(Register::Idle, 1);
            }
        }
    }

    #[test]
    fn ipa_route_targets_are_read_twice_only_after_full_handoff_preservation() {
        let targets = ipa_targets();
        let mut f = Fake::ready();
        let result = read_ipa_path_bcms(&mut f, targets, 2, || {}, |_, _| {});
        assert!(result.passed());
        assert_eq!(result.completed, 22);
        assert_eq!(result.submitted, 22);
        assert_eq!(result.handoff_checks, 44);
        assert_eq!(result.responses[0][0], Some(0));
        assert_eq!(result.responses[0][1], Some(0xa0000002));
        assert_eq!(result.responses[1][10], Some(0xa0000016));
        let expected = targets.map(BcmTarget::address);
        assert_eq!(f.requests, expected.repeat(2));
        assert!(
            f.writes
                .iter()
                .filter(|(r, _)| *r == Register::Message)
                .all(|(_, value)| *value == READ_MESSAGE)
        );
        assert_eq!(result.before, result.after);
    }

    #[test]
    fn invalid_or_reordered_live_targets_are_rejected_before_rsc_writes() {
        assert_eq!(
            BcmTarget::new(BcmTargetName::Sn11, 0x40000),
            Err(ControllerError::InvalidTarget)
        );
        assert_eq!(
            BcmTarget::new(BcmTargetName::Sn11, 0x50001),
            Err(ControllerError::InvalidTarget)
        );
        let mut targets = ipa_targets();
        targets.swap(0, 1);
        let mut f = Fake::ready();
        let result = read_ipa_path_bcms(&mut f, targets, 2, || {}, |_, _| {});
        assert_eq!(result.error, Some(Error::TargetList));
        assert!(f.writes.is_empty());

        let mut targets = ipa_targets();
        targets[1] = BcmTarget::new(BcmTargetName::Sn0, targets[0].address()).unwrap();
        let result = read_ipa_path_bcms(&mut f, targets, 2, || {}, |_, _| {});
        assert_eq!(result.error, Some(Error::TargetList));
        assert!(f.writes.is_empty());
    }
    impl ReadRegisters for Fake {
        fn read(&mut self, r: GuardRegister) -> u32 {
            if r == GuardRegister::CONFIG {
                self.captures += 1;
                if self.unstable || self.change_before_capture == Some(self.captures) {
                    self.words[0xd00 / 4] += 2;
                }
            }
            self.words[r.offset() / 4]
        }
    }

    #[test]
    fn two_bus_passes_publish_only_verified_raw_responses_and_preserve_all_banks() {
        let mut f = Fake::ready();
        let result = read_bcm_series(&mut f, 2, || {}, |_, _| {});
        assert!(result.passed());
        assert_eq!(
            result.responses[0],
            [
                Some(0),
                Some(0xa0000002),
                Some(0xa0000003),
                Some(0xa0000004),
                Some(0xa0000005),
                Some(0xa0000006)
            ]
        );
        assert_eq!(result.responses[1][5], Some(0xa000000c));
        assert_eq!(
            f.requests,
            BcmResource::ALL.map(BcmResource::address).repeat(2)
        );
        assert!(
            f.writes
                .iter()
                .filter(|(r, _)| *r == Register::Message)
                .all(|(_, v)| *v == 0x108)
        );
        assert_eq!(result.handoff_checks, 24);
        assert_eq!(result.before, result.after);
    }

    #[test]
    fn every_timeout_stops_without_retry_cleanup_or_any_resource_write() {
        for stop in 1..=12 {
            let mut f = Fake::ready();
            f.stop_at = Some(stop);
            let r = read_bcm_series(&mut f, 1, || {}, |_, _| {});
            assert_eq!(r.error, Some(Error::Controller));
            assert_eq!(r.controller_error, Some(ControllerError::PollLimit));
            assert_eq!(r.completed as usize, stop - 1);
            assert_eq!(r.submitted as usize, stop);
            assert_eq!(f.requests.len(), stop);
            assert_eq!(f.writes.last(), Some(&(Register::Control, 0x1010000)));
        }
    }

    #[test]
    fn every_bank_must_start_empty_idle_and_untriggered_before_any_write() {
        for bank in 0..8 {
            for (field, value) in [
                (BankField::Enable, 1),
                (BankField::Wait, 1),
                (BankField::Control, 0x10000),
                (BankField::Idle, 0),
            ] {
                let mut f = Fake::ready();
                f.words[GuardRegister::bank(bank, field).unwrap().offset() / 4] = value;
                let r = read_bcm_series(&mut f, 1, || {}, |_, _| {});
                assert_eq!(r.error, Some(Error::Handoff));
                assert!(f.writes.is_empty());
            }
        }
    }

    #[test]
    fn changed_wake_programming_blocks_response_publication_and_later_requests() {
        for stop in 1..=12 {
            let mut f = Fake::ready();
            f.change_after = Some(stop);
            let r = read_bcm_series(&mut f, 1, || {}, |_, _| {});
            assert_eq!(r.error, Some(Error::Preservation));
            assert_eq!(r.completed as usize, stop - 1);
            assert_eq!(f.requests.len(), stop);
            assert_eq!(r.responses[(stop - 1) / 6][(stop - 1) % 6], None);
        }
    }

    #[test]
    fn later_before_snapshot_must_match_initial_even_if_empty_and_stable() {
        let mut f = Fake::ready();
        f.change_before_capture = Some(5); // Second request's preflight.
        let r = read_bcm_series(&mut f, 1, || {}, |_, _| {});
        assert_eq!(r.error, Some(Error::Handoff));
        assert_eq!(r.completed, 1);
        assert_eq!(f.requests.len(), 1);
    }

    #[test]
    fn zero_budget_and_unstable_snapshot_cannot_issue_a_request() {
        let mut f = Fake::ready();
        assert_eq!(
            read_bcm_series(&mut f, 0, || {}, |_, _| {}).error,
            Some(Error::WorkLimit)
        );
        assert!(f.writes.is_empty());
        assert_eq!(f.captures, 0);
        f.unstable = true;
        let r = read_bcm_series(&mut f, 1, || {}, |_, _| {});
        assert_eq!(r.error, Some(Error::BeforeCapture));
        assert_eq!(r.handoff_error, Some(handoff::Error::Unstable));
        assert!(f.writes.is_empty());
        assert_eq!(f.captures, 6);
    }
}
