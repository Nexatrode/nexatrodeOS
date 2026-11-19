//! One fixed MC0 reassertion experiment; not a complete interconnect provider.
//!
//! Requires the observed MC0 request plus a stable two-pass snapshot of all six
//! resources, then reissues only MC0's existing numeric fields with commit.
//! Other resources are compared against this run's baseline because their raw
//! replies include unrelated clients and can vary across boots. Does not reduce
//! MC0's observed fields, touch other resources, retry, or automatically
//! restore/unvote. Raw response identity is not complete per-client ownership.

use crate::qcom_bcm_vote::Decoded;
use crate::qcom_rpmh_bus::{self as bus, read_bcm_series};
use crate::qcom_rpmh_handoff::{self as handoff, ReadRegisters};
use crate::qcom_rpmh_read::{
    ActiveTransaction, Command, Error as ControllerError, MC0_OBSERVED_READ, RegisterIo, Snapshot,
};

/// Historical complete tuple used by probe fixtures only. Runtime validation
/// pins MC0 to its observed request and uses the stable current-boot values
/// for the other five resources.
pub const EXPECTED_READS: [u32; 6] = [
    MC0_OBSERVED_READ,
    0x20000008,
    0,
    0x213c44f1,
    0x213c44f1,
    0x24005adf,
];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BeforeReads,
    StartingValues,
    BeforeCapture,
    BeforeChanged,
    Write,
    AfterCapture,
    AfterChanged,
    AfterReads,
    AfterValues,
}
impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::BeforeReads => 1,
            Self::StartingValues => 2,
            Self::BeforeCapture => 3,
            Self::BeforeChanged => 4,
            Self::Write => 5,
            Self::AfterCapture => 6,
            Self::AfterChanged => 7,
            Self::AfterReads => 8,
            Self::AfterValues => 9,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Phase {
    BeforeReads,
    Mc0Write,
    AfterCapture,
    AfterReads,
    Held,
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub before: bus::Report,
    pub after: Option<bus::Report>,
    pub guard_before: Option<handoff::Snapshot>,
    pub guard_after: Option<handoff::Snapshot>,
    pub write_submitted: bool,
    pub write_acknowledged: bool,
    pub state: Option<Snapshot>,
    pub error: Option<Error>,
    pub controller_error: Option<ControllerError>,
    pub handoff_error: Option<handoff::Error>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.write_submitted
            && self.write_acknowledged
            && self.before.passed()
            && self.guard_before.is_some()
            && self.guard_before == self.guard_after
            && self
                .after
                .is_some_and(|after| after.passed() && after_values(after, self.before.responses))
    }
    pub fn preserved_checks(&self) -> u32 {
        u32::from(self.before.handoff_checks)
            + u32::from(self.guard_before.is_some())
            + u32::from(self.guard_after.is_some())
            + self.after.map_or(0, |r| u32::from(r.handoff_checks))
    }
}

fn starting_values_match(before: &bus::Report) -> bool {
    let expected = Decoded::parse(MC0_OBSERVED_READ).unwrap();
    if !(0..2).all(|pass| {
        before.responses[pass][0]
            .and_then(|value| Decoded::parse(value).ok())
            .is_some_and(|actual| actual.same_request(expected))
    }) {
        return false;
    }
    (1..6).all(|resource| {
        before.responses[0][resource].is_some()
            && before.responses[0][resource] == before.responses[1][resource]
    })
}

fn after_values(after: bus::Report, before: [[Option<u32>; 6]; 2]) -> bool {
    let expected_mc0 = Decoded::parse(MC0_OBSERVED_READ).unwrap();
    (0..2).all(|pass| {
        (0..6).all(|resource| {
            let Some(value) = after.responses[pass][resource] else {
                return false;
            };
            if resource == 0 {
                Decoded::parse(value).is_ok_and(|actual| actual.same_request(expected_mc0))
            } else {
                before[0][resource].is_some()
                    && before[0][resource] == before[1][resource]
                    && Some(value) == before[0][resource]
            }
        })
    })
}

/// Requires exclusive standalone AP DRV2 ownership and the observed metadata.
/// The sole WRITE is a fixed constant, never caller-constructed response data.
/// On failure an applied-but-unacknowledged request may remain held until reset.
pub fn hold_mc0<I: RegisterIo + ReadRegisters>(
    io: &mut I,
    polls: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(Phase),
) -> Report {
    progress(Phase::BeforeReads);
    let before = read_bcm_series(io, polls, &mut wait, |_, _| {});
    let mut r = Report {
        before,
        after: None,
        guard_before: None,
        guard_after: None,
        write_submitted: false,
        write_acknowledged: false,
        state: before.state,
        error: None,
        controller_error: before.controller_error,
        handoff_error: before.handoff_error,
    };
    if !before.passed() {
        r.error = Some(Error::BeforeReads);
        return r;
    }
    if !starting_values_match(&before) {
        r.error = Some(Error::StartingValues);
        return r;
    }
    progress(Phase::Mc0Write);
    let guard = match handoff::capture_stable(io, 3) {
        Ok(guard) => guard,
        Err(e) => {
            r.error = Some(Error::BeforeCapture);
            r.handoff_error = Some(e);
            return r;
        }
    };
    r.guard_before = Some(guard);
    if Some(guard) != before.before {
        r.error = Some(Error::BeforeChanged);
        return r;
    }
    let mut write = ActiveTransaction::new(Command::Mc0Reassert, polls);
    let outcome = write.start(io).and_then(|()| {
        r.write_submitted = true;
        loop {
            if write.poll(io)? {
                return Ok(());
            }
            wait();
        }
    });
    r.state = Some(write.last());
    if let Err(e) = outcome {
        r.state = Some(Snapshot::read(io));
        r.error = Some(Error::Write);
        r.controller_error = Some(e);
        return r;
    }
    r.write_acknowledged = true;
    progress(Phase::AfterCapture);
    let after_guard = match handoff::capture_stable(io, 3) {
        Ok(guard) => guard,
        Err(e) => {
            r.error = Some(Error::AfterCapture);
            r.handoff_error = Some(e);
            return r;
        }
    };
    r.guard_after = Some(after_guard);
    if after_guard != guard {
        r.error = Some(Error::AfterChanged);
        return r;
    }
    progress(Phase::AfterReads);
    let after = read_bcm_series(io, polls, &mut wait, |_, _| {});
    r.after = Some(after);
    r.state = after.state;
    if !after.passed() {
        r.error = Some(Error::AfterReads);
        r.controller_error = after.controller_error;
        r.handoff_error = after.handoff_error;
        return r;
    }
    if after.before != before.before {
        r.error = Some(Error::AfterChanged);
        return r;
    }
    if !after_values(after, before.responses) {
        r.error = Some(Error::AfterValues);
        return r;
    }
    progress(Phase::Held);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_handoff::{
        BankField, MAPPING_BYTES, OBSERVED_CONFIG, Register as GuardRegister,
    };
    use crate::qcom_rpmh_read::{BcmResource, MC0_REASSERT_WORD, Register};
    use std::vec::Vec;
    struct Fake {
        words: [u32; MAPPING_BYTES / 4],
        values: [u32; 6],
        requests: Vec<(u32, u32, u32, u32)>,
        stop_at: Option<usize>,
        corrupt_wake_after_write: bool,
        corrupt_before_write: bool,
        change_readback: bool,
        mutate_after_read: Option<usize>,
        read_count: usize,
        sticky_commit: bool,
        captures: usize,
    }
    impl Fake {
        fn ready() -> Self {
            let mut f = Self {
                words: [0; MAPPING_BYTES / 4],
                values: EXPECTED_READS,
                requests: Vec::new(),
                stop_at: None,
                corrupt_wake_after_write: false,
                corrupt_before_write: false,
                change_readback: false,
                mutate_after_read: None,
                read_count: 0,
                sticky_commit: false,
                captures: 0,
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
    impl RegisterIo for Fake {
        fn read(&mut self, r: Register) -> u32 {
            self.words[r.offset() / 4]
        }
        fn write(&mut self, r: Register, v: u32) {
            if r == Register::IrqClear {
                self.words[Register::IrqStatus.offset() / 4] &= !v;
                if self.corrupt_wake_after_write && self.requests.len() == 13 {
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
                let message = self.words[0xd30 / 4];
                let address = self.words[0xd34 / 4];
                let data = self.words[0xd38 / 4];
                let wait = self.words[0xd10 / 4];
                self.requests.push((message, address, data, wait));
                let index = BcmResource::ALL
                    .iter()
                    .position(|r| r.address() == address)
                    .unwrap();
                if message == 0x10108 {
                    assert_eq!((address, data, wait), (0x50000, MC0_REASSERT_WORD, 1));
                    // Applied even if the acknowledgement below is lost.
                    self.values[0] = if self.sticky_commit {
                        MC0_REASSERT_WORD
                    } else {
                        MC0_OBSERVED_READ
                    };
                    if self.change_readback {
                        self.values[1] ^= 1;
                    }
                } else {
                    assert_eq!((message, data, wait), (0x108, 0, 0));
                    self.read_count += 1;
                    if self.mutate_after_read == Some(self.read_count) {
                        self.values[5] ^= 1;
                    }
                }
                self.set(Register::Idle, 0);
                if self.stop_at == Some(self.requests.len()) {
                    return;
                }
                self.set(Register::IrqStatus, 1);
                self.set(Register::CommandStatus, 0x10100);
                self.set(Register::Response, self.values[index]);
                self.set(Register::Idle, 1);
            }
        }
    }
    impl ReadRegisters for Fake {
        fn read(&mut self, r: GuardRegister) -> u32 {
            if r == GuardRegister::CONFIG {
                self.captures += 1;
                if self.corrupt_before_write && self.captures == 49 {
                    self.words[GuardRegister::bank(5, BankField::Wait).unwrap().offset() / 4] = 1;
                }
            }
            self.words[r.offset() / 4]
        }
    }
    #[test]
    fn reasserts_one_fixed_word_with_commit_wait_and_50_preservation_checks() {
        let mut f = Fake::ready();
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert!(r.passed());
        assert_eq!(f.requests.len(), 25);
        assert_eq!(r.preserved_checks(), 50);
        assert_eq!(f.requests[12], (0x10108, 0x50000, 0x69917f75, 1));
        assert_eq!(
            f.requests
                .iter()
                .filter(|(m, _, _, _)| *m == 0x10108)
                .count(),
            1
        );
        assert_eq!(r.before.responses, r.after.unwrap().responses);
    }
    #[test]
    fn every_timeout_is_terminal_with_no_second_write_or_guessed_unvote() {
        for stop in 1..=25 {
            let mut f = Fake::ready();
            f.stop_at = Some(stop);
            let r = hold_mc0(&mut f, 1, || {}, |_| {});
            assert!(!r.passed());
            assert_eq!(f.requests.len(), stop);
            assert_eq!(
                f.requests
                    .iter()
                    .filter(|(m, _, _, _)| *m == 0x10108)
                    .count(),
                usize::from(stop >= 13)
            );
            assert_eq!(r.write_submitted, stop >= 13);
            assert_eq!(r.write_acknowledged, stop > 13);
        }
    }
    #[test]
    fn mc0_request_is_pinned_but_stable_other_resource_baselines_are_dynamic() {
        let mut f = Fake::ready();
        f.values[0] ^= 1;
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert_eq!(r.error, Some(Error::StartingValues));
        assert!(!r.write_submitted);
        assert_eq!(f.requests.len(), 12);
        assert!(f.requests.iter().all(|(m, _, _, _)| *m == 0x108));

        for index in 1..6 {
            let mut f = Fake::ready();
            f.values[index] ^= 1;
            let r = hold_mc0(&mut f, 1, || {}, |_| {});
            assert!(r.passed(), "resource {index}: {r:?}");
            assert!(
                r.before
                    .responses
                    .iter()
                    .all(|pass| { pass[index] == Some(EXPECTED_READS[index] ^ 1) })
            );
            assert!(
                r.after
                    .unwrap()
                    .responses
                    .iter()
                    .all(|pass| { pass[index] == Some(EXPECTED_READS[index] ^ 1) })
            );
            assert_eq!(f.requests.len(), 25);
            assert_eq!(
                f.requests
                    .iter()
                    .filter(|(message, _, _, _)| *message == 0x10108)
                    .count(),
                1
            );
        }
    }
    #[test]
    fn a_value_change_between_read_passes_blocks_the_mc0_write() {
        let mut f = Fake::ready();
        // The first pass is stable and matches the recorded tuple. Drift the
        // final resource immediately before pass 1 starts, as can happen when
        // an external RPMh client changes its request between snapshots.
        f.mutate_after_read = Some(7);
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert_eq!(r.error, Some(Error::StartingValues));
        let before = r.before.responses;
        assert_eq!(before[0], EXPECTED_READS.map(Some));
        assert_eq!(before[1][5], Some(EXPECTED_READS[5] ^ 1));
        assert!(!r.write_submitted);
        assert_eq!(f.requests.len(), 12);
        assert!(
            f.requests
                .iter()
                .all(|(message, _, _, _)| *message == 0x108)
        );
    }
    #[test]
    fn changed_sleep_or_wake_programming_stops_before_or_after_the_one_write() {
        let mut f = Fake::ready();
        f.corrupt_before_write = true;
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert_eq!(r.error, Some(Error::BeforeChanged));
        assert_eq!(f.requests.len(), 12);
        let mut f = Fake::ready();
        f.corrupt_wake_after_write = true;
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert_eq!(r.error, Some(Error::AfterChanged));
        assert!(r.write_acknowledged);
        assert!(r.after.is_none());
        assert_eq!(f.requests.len(), 13);
    }
    #[test]
    fn other_resource_readback_change_fails_without_any_compensating_write() {
        let mut f = Fake::ready();
        f.change_readback = true;
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert_eq!(r.error, Some(Error::AfterValues));
        assert!(r.write_acknowledged);
        assert_eq!(f.requests.len(), 25);
        assert_eq!(
            f.requests
                .iter()
                .filter(|(m, _, _, _)| *m == 0x10108)
                .count(),
            1
        );
    }
    #[test]
    fn commit_control_flag_may_be_reported_without_changing_numeric_request() {
        let mut f = Fake::ready();
        f.sticky_commit = true;
        let r = hold_mc0(&mut f, 1, || {}, |_| {});
        assert!(r.passed());
        assert_eq!(r.after.unwrap().responses[0][0], Some(0x69917f75));
    }
    #[test]
    fn zero_work_budget_never_submits_a_command() {
        let mut f = Fake::ready();
        let r = hold_mc0(&mut f, 0, || {}, |_| {});
        assert!(!r.passed());
        assert!(f.requests.is_empty());
        assert!(!r.write_submitted);
    }
}
