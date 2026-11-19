//! Restricted power-resource experiments, not complete modem power sequencing.
//!
//! Only the observed starting tuple CX/MSS/MX/XO = 5/0/3/3 is accepted, in two
//! checked read passes. One fixed active CX TURBO write (selector 6) is then
//! acknowledged and checked by two more read passes. No automatic unvote or
//! restoration is attempted: read responses are not proven previous local
//! votes. A named extension requests MSS7 and already-on XO3 in that order,
//! with readback gates between each step. Successful requests are held until
//! physical restart; no bus, PAS, reset, firmware, or DMA operation is supplied.

use crate::qcom_rpmh_read::{
    ActiveTransaction, Command, Error, ReadSeries, RegisterIo, Snapshot, read_series,
};

pub const EXPECTED_START: [u32; 4] = [5, 0, 3, 3];
pub use crate::qcom_rpmh_read::CX_TURBO_SELECTOR;
use crate::qcom_rpmh_read::{MSS_TURBO_SELECTOR, XO_ON_VALUE};

struct WriteAttempt {
    submitted: bool,
    state: Snapshot,
    error: Option<Error>,
}

fn write_once(
    io: &mut impl RegisterIo,
    command: Command,
    polls: u32,
    wait: &mut impl FnMut(),
) -> WriteAttempt {
    let mut transaction = ActiveTransaction::new(command, polls);
    let mut submitted = false;
    let result = transaction.start(io).and_then(|()| {
        submitted = true;
        loop {
            if transaction.poll(io)? {
                return Ok(());
            }
            wait();
        }
    });
    WriteAttempt {
        submitted,
        state: if result.is_ok() {
            transaction.last()
        } else {
            Snapshot::read(io)
        },
        error: result.err(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerError {
    BeforeReads,
    StartingValues,
    Vote,
    AfterReads,
    Readback,
}

impl PowerError {
    pub const fn code(self) -> u32 {
        match self {
            Self::BeforeReads => 1,
            Self::StartingValues => 2,
            Self::Vote => 3,
            Self::AfterReads => 4,
            Self::Readback => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    BeforeReads,
    CxVote,
    AfterReads,
    Held,
}

#[derive(Clone, Copy, Debug)]
pub struct HoldReport {
    pub before: ReadSeries,
    pub after: Option<ReadSeries>,
    pub write_submitted: bool,
    pub write_completed: bool,
    pub state: Snapshot,
    pub error: Option<PowerError>,
    pub controller_error: Option<Error>,
}

impl HoldReport {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.write_completed
            && self.after.is_some_and(|after| {
                after.passed()
                    && after.responses[0][0] == Some(CX_TURBO_SELECTOR)
                    && after.responses[1][0] == Some(CX_TURBO_SELECTOR)
            })
    }
}

/// Requires the same exclusive standalone AP DRV2 ownership as the read probe.
/// `wait` and `progress` must return and must not access this controller.
/// After any failure, stop: never issue a compensating vote from a guessed
/// baseline, retry a timed-out write, start the modem, or release its reset.
pub fn hold_cx_turbo(
    io: &mut impl RegisterIo,
    polls_per_request: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(Phase),
) -> HoldReport {
    progress(Phase::BeforeReads);
    let before = read_series(io, polls_per_request, &mut wait);
    let mut report = HoldReport {
        before,
        after: None,
        write_submitted: false,
        write_completed: false,
        state: before.after,
        error: None,
        controller_error: None,
    };
    if !before.passed() {
        report.error = Some(PowerError::BeforeReads);
        report.controller_error = before.error;
        return report;
    }
    if before.responses != [EXPECTED_START.map(Some); 2] {
        report.error = Some(PowerError::StartingValues);
        return report;
    }

    progress(Phase::CxVote);
    let write = write_once(io, Command::CxTurbo, polls_per_request, &mut wait);
    report.write_submitted = write.submitted;
    report.state = write.state;
    if let Some(error) = write.error {
        report.error = Some(PowerError::Vote);
        report.controller_error = Some(error);
        return report;
    }
    report.write_completed = true;

    progress(Phase::AfterReads);
    let after = read_series(io, polls_per_request, &mut wait);
    report.after = Some(after);
    report.state = after.after;
    if !after.passed() {
        report.error = Some(PowerError::AfterReads);
        report.controller_error = after.error;
        return report;
    }
    if after.responses[0][0] != Some(CX_TURBO_SELECTOR)
        || after.responses[1][0] != Some(CX_TURBO_SELECTOR)
    {
        report.error = Some(PowerError::Readback);
        return report;
    }
    progress(Phase::Held);
    report
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceHoldError {
    Cx,
    AfterCxValues,
    MssVote,
    MssReads,
    MssValues,
    XoVote,
    XoReads,
    XoValues,
}
impl ResourceHoldError {
    pub const fn code(self) -> u32 {
        match self {
            Self::Cx => 1,
            Self::AfterCxValues => 2,
            Self::MssVote => 3,
            Self::MssReads => 4,
            Self::MssValues => 5,
            Self::XoVote => 6,
            Self::XoReads => 7,
            Self::XoValues => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceHoldPhase {
    Cx(Phase),
    MssVote,
    MssReads,
    XoVote,
    XoReads,
    Held,
}

#[derive(Clone, Copy, Debug)]
pub struct ResourceHoldReport {
    pub cx: HoldReport,
    pub after_mss: Option<ReadSeries>,
    pub after_xo: Option<ReadSeries>,
    pub submitted: [bool; 3],
    pub completed: [bool; 3],
    pub error: Option<ResourceHoldError>,
    pub controller_error: Option<Error>,
    pub state: Snapshot,
}
impl ResourceHoldReport {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.cx.passed()
            && self.completed == [true; 3]
            && self
                .after_xo
                .is_some_and(|r| resource_values(r, CX_TURBO_SELECTOR, MSS_TURBO_SELECTOR))
    }
}

fn resource_values(reads: ReadSeries, cx: u32, mss: u32) -> bool {
    reads.passed()
        && (0..2).all(|pass| {
            reads.responses[pass][0] == Some(cx)
                && reads.responses[pass][1] == Some(mss)
                && reads.responses[pass][3] == Some(XO_ON_VALUE)
        })
    // MX is displayed but not written or used to invent a coupling rule.
}

/// Restricted resource experiment, not the complete modem proxy sequence.
/// Reuse the proven CX hold, then request only MSS7 and already-on XO3. Each
/// write has checked acknowledgement/cleanup and two subsequent read passes.
/// No bus vote, AOP on, PAS, reset, firmware, DMA or automatic unvote is supplied.
pub fn hold_modem_resources(
    io: &mut impl RegisterIo,
    polls: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(ResourceHoldPhase),
) -> ResourceHoldReport {
    let cx = hold_cx_turbo(io, polls, &mut wait, |p| progress(ResourceHoldPhase::Cx(p)));
    let mut report = ResourceHoldReport {
        cx,
        after_mss: None,
        after_xo: None,
        submitted: [cx.write_submitted, false, false],
        completed: [cx.write_completed, false, false],
        error: None,
        controller_error: cx.controller_error,
        state: cx.state,
    };
    if !cx.passed() {
        report.error = Some(ResourceHoldError::Cx);
        return report;
    }
    if !cx
        .after
        .is_some_and(|r| resource_values(r, CX_TURBO_SELECTOR, 0))
    {
        report.error = Some(ResourceHoldError::AfterCxValues);
        return report;
    }
    progress(ResourceHoldPhase::MssVote);
    let mss = write_once(io, Command::MssTurbo, polls, &mut wait);
    report.submitted[1] = mss.submitted;
    report.state = mss.state;
    if let Some(error) = mss.error {
        report.error = Some(ResourceHoldError::MssVote);
        report.controller_error = Some(error);
        return report;
    }
    report.completed[1] = true;
    progress(ResourceHoldPhase::MssReads);
    let reads = read_series(io, polls, &mut wait);
    report.after_mss = Some(reads);
    report.state = reads.after;
    if !reads.passed() {
        report.error = Some(ResourceHoldError::MssReads);
        report.controller_error = reads.error;
        return report;
    }
    if !resource_values(reads, CX_TURBO_SELECTOR, MSS_TURBO_SELECTOR) {
        report.error = Some(ResourceHoldError::MssValues);
        return report;
    }
    progress(ResourceHoldPhase::XoVote);
    let xo = write_once(io, Command::XoOn, polls, &mut wait);
    report.submitted[2] = xo.submitted;
    report.state = xo.state;
    if let Some(error) = xo.error {
        report.error = Some(ResourceHoldError::XoVote);
        report.controller_error = Some(error);
        return report;
    }
    report.completed[2] = true;
    progress(ResourceHoldPhase::XoReads);
    let reads = read_series(io, polls, &mut wait);
    report.after_xo = Some(reads);
    report.state = reads.after;
    if !reads.passed() {
        report.error = Some(ResourceHoldError::XoReads);
        report.controller_error = reads.error;
        return report;
    }
    if !resource_values(reads, CX_TURBO_SELECTOR, MSS_TURBO_SELECTOR) {
        report.error = Some(ResourceHoldError::XoValues);
        return report;
    }
    progress(ResourceHoldPhase::Held);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_read::{
        CX_ADDRESS, OBSERVED_CONFIG, READ_MESSAGE, ReadResource, Register,
    };

    struct Fake {
        words: [u32; 1024],
        values: [u32; 4],
        requests: std::vec::Vec<(u32, u32, u32)>,
        stop_at: Option<usize>,
        corrupt_read_at: Option<usize>,
        ignore_vote: bool,
        ignore_vote_at: Option<usize>,
        mx_on_mss: Option<u32>,
    }
    impl Fake {
        fn ready() -> Self {
            let mut f = Self {
                words: [0; 1024],
                values: EXPECTED_START,
                requests: std::vec::Vec::new(),
                stop_at: None,
                corrupt_read_at: None,
                ignore_vote: false,
                ignore_vote_at: None,
                mx_on_mss: None,
            };
            for (r, v) in [
                (Register::Config, OBSERVED_CONFIG),
                (Register::IrqEnable, 1),
                (Register::Idle, 1),
                (Register::OtherIdle, 1),
            ] {
                f.set(r, v);
            }
            f
        }
        fn set(&mut self, r: Register, v: u32) {
            self.words[r.offset() / 4] = v;
        }
        fn writes(&self) -> std::vec::Vec<(u32, u32)> {
            self.requests
                .iter()
                .filter_map(|(m, a, d)| (m & (1 << 16) != 0).then_some((*a, *d)))
                .collect()
        }
    }
    impl RegisterIo for Fake {
        fn read(&mut self, r: Register) -> u32 {
            self.words[r.offset() / 4]
        }
        fn write(&mut self, r: Register, v: u32) {
            if r == Register::IrqClear {
                self.words[Register::IrqStatus.offset() / 4] &= !v;
            } else {
                self.set(r, v);
            }
            if r == Register::Control && v == 0 {
                self.set(Register::CommandStatus, 0);
            }
            if r == Register::Control && v == 0x1010000 {
                let message = self.read(Register::Message);
                let address = self.read(Register::Address);
                let data = self.read(Register::Data);
                self.requests.push((message, address, data));
                self.set(Register::Idle, 0);
                // A write may have applied even when the acknowledgement times out.
                if message & (1 << 16) != 0 {
                    assert_eq!(message, 0x10108);
                    let index = match address {
                        CX_ADDRESS => {
                            assert_eq!(data, 6);
                            0
                        }
                        0x30060 => {
                            assert_eq!(data, 7);
                            1
                        }
                        0x30090 => {
                            assert_eq!(data, 3);
                            3
                        }
                        _ => panic!("non-allowlisted WRITE"),
                    };
                    if !self.ignore_vote && self.ignore_vote_at != Some(self.requests.len()) {
                        self.values[index] = data;
                        if index == 1 {
                            if let Some(mx) = self.mx_on_mss {
                                self.values[2] = mx;
                            }
                        }
                    }
                }
                if self.stop_at == Some(self.requests.len()) {
                    return;
                }
                let index = ReadResource::ALL
                    .iter()
                    .position(|r| r.address() == address)
                    .unwrap();
                let response = if self.corrupt_read_at == Some(self.requests.len()) {
                    0xf
                } else {
                    self.values[index]
                };
                self.set(Register::Response, response);
                self.set(Register::CommandStatus, 0x10100);
                self.set(Register::IrqStatus, 1);
                self.set(Register::Idle, 1);
            }
        }
    }

    #[test]
    fn fixed_raise_ack_readback_and_hold_never_unvotes_or_writes_other_resources() {
        let mut f = Fake::ready();
        let mut phases = std::vec::Vec::new();
        let report = hold_cx_turbo(&mut f, 2, || {}, |p| phases.push(p));
        assert!(report.passed());
        assert!(report.write_submitted && report.write_completed);
        assert_eq!(f.requests.len(), 17);
        assert_eq!(f.writes(), [(CX_ADDRESS, 6)]);
        assert_eq!(
            phases,
            [
                Phase::BeforeReads,
                Phase::CxVote,
                Phase::AfterReads,
                Phase::Held
            ]
        );
        assert_eq!(
            report.after.unwrap().responses,
            [[Some(6), Some(0), Some(3), Some(3)]; 2]
        );
        assert_eq!(report.state.irq_enable, 1);
        assert_eq!(report.state.enable, 0);
        assert_eq!(f.values, [6, 0, 3, 3]);
        assert_eq!(f.requests[8], (0x10108, CX_ADDRESS, 6));
        assert!(
            f.requests
                .iter()
                .enumerate()
                .all(|(i, (m, _, d))| i == 8 || (*m == READ_MESSAGE && *d == 0))
        );
    }

    #[test]
    fn any_different_starting_response_refuses_the_vote_including_already_higher_cx() {
        for index in 0..4 {
            for value in [0, 1, 4, 6, 7, 15, u32::MAX] {
                if value == EXPECTED_START[index] {
                    continue;
                }
                let mut f = Fake::ready();
                f.values[index] = value;
                let report = hold_cx_turbo(&mut f, 1, || {}, |_| {});
                assert_eq!(report.error, Some(PowerError::StartingValues));
                assert!(!report.write_submitted);
                assert!(f.writes().is_empty());
            }
        }
        let mut f = Fake::ready();
        f.corrupt_read_at = Some(5); // Second CX sample differs.
        assert_eq!(
            hold_cx_turbo(&mut f, 1, || {}, |_| {}).error,
            Some(PowerError::StartingValues)
        );
        assert!(f.writes().is_empty());
    }

    #[test]
    fn first_fault_stops_everywhere_without_replaying_write_or_guessing_a_restore() {
        for failed_request in 1..=17 {
            let mut f = Fake::ready();
            f.stop_at = Some(failed_request);
            let report = hold_cx_turbo(&mut f, 2, || {}, |_| {});
            assert!(!report.passed());
            assert_eq!(f.requests.len(), failed_request);
            assert_eq!(report.controller_error, Some(Error::PollLimit));
            assert_eq!(
                report.error,
                Some(if failed_request < 9 {
                    PowerError::BeforeReads
                } else if failed_request == 9 {
                    PowerError::Vote
                } else {
                    PowerError::AfterReads
                })
            );
            assert_eq!(report.write_submitted, failed_request >= 9);
            assert_eq!(report.write_completed, failed_request > 9);
            assert_eq!(
                f.writes(),
                if failed_request < 9 {
                    std::vec![]
                } else {
                    std::vec![(CX_ADDRESS, 6)]
                }
            );
        }
    }

    #[test]
    fn acknowledged_write_without_changed_readback_does_not_report_success_or_restore() {
        let mut f = Fake::ready();
        f.ignore_vote = true;
        let report = hold_cx_turbo(&mut f, 1, || {}, |_| {});
        assert_eq!(report.error, Some(PowerError::Readback));
        assert!(report.write_completed);
        assert!(!report.passed());
        assert_eq!(f.writes(), [(CX_ADDRESS, 6)]);
    }

    #[test]
    fn zero_budget_never_submits_a_command() {
        let mut f = Fake::ready();
        let report = hold_cx_turbo(&mut f, 0, || {}, |_| {});
        assert_eq!(report.error, Some(PowerError::BeforeReads));
        assert!(f.requests.is_empty());
        assert!(!report.write_submitted);
    }

    #[test]
    fn resource_hold_issues_only_three_fixed_votes_in_order_with_intermediate_readbacks() {
        let mut f = Fake::ready();
        let mut phases = std::vec::Vec::new();
        let report = hold_modem_resources(&mut f, 1, || {}, |p| phases.push(p));
        assert!(report.passed());
        assert_eq!(report.completed, [true; 3]);
        assert_eq!(f.requests.len(), 35);
        assert_eq!(f.writes(), [(CX_ADDRESS, 6), (0x30060, 7), (0x30090, 3)]);
        assert_eq!(f.requests[8], (0x10108, CX_ADDRESS, 6));
        assert_eq!(f.requests[17], (0x10108, 0x30060, 7));
        assert_eq!(f.requests[26], (0x10108, 0x30090, 3));
        assert_eq!(
            report.after_mss.unwrap().responses,
            [[Some(6), Some(7), Some(3), Some(3)]; 2]
        );
        assert_eq!(phases.last(), Some(&ResourceHoldPhase::Held));
    }

    #[test]
    fn resource_hold_stops_at_each_timeout_including_ambiguous_mss_and_xo_writes() {
        for failed in 1..=35 {
            let mut f = Fake::ready();
            f.stop_at = Some(failed);
            let report = hold_modem_resources(&mut f, 2, || {}, |_| {});
            assert!(!report.passed());
            assert_eq!(f.requests.len(), failed);
            assert_eq!(report.controller_error, Some(Error::PollLimit));
            let expected = [(CX_ADDRESS, 6), (0x30060, 7), (0x30090, 3)];
            let count =
                usize::from(failed >= 9) + usize::from(failed >= 18) + usize::from(failed >= 27);
            assert_eq!(f.writes(), expected[..count]);
            assert_eq!(report.submitted, [failed >= 9, failed >= 18, failed >= 27]);
            assert_eq!(report.completed, [failed > 9, failed > 18, failed > 27]);
            assert!(report.error.is_some());
        }
    }

    #[test]
    fn missing_mss_readback_or_changed_xo_blocks_clock_vote_without_restore() {
        for changed_read in [None, Some(22)] {
            let mut f = Fake::ready();
            if changed_read.is_none() {
                f.ignore_vote_at = Some(18);
            } else {
                f.corrupt_read_at = changed_read;
            } // XO after MSS is unknown.
            let report = hold_modem_resources(&mut f, 1, || {}, |_| {});
            assert_eq!(report.error, Some(ResourceHoldError::MssValues));
            assert_eq!(report.completed, [true, true, false]);
            assert_eq!(f.writes(), [(CX_ADDRESS, 6), (0x30060, 7)]);
        }
    }

    #[test]
    fn mx_response_changes_are_reported_without_inventing_a_parent_vote() {
        let mut f = Fake::ready();
        f.mx_on_mss = Some(4);
        let report = hold_modem_resources(&mut f, 1, || {}, |_| {});
        assert!(report.passed());
        assert_eq!(report.after_xo.unwrap().responses[0][2], Some(4));
        assert_eq!(f.writes(), [(CX_ADDRESS, 6), (0x30060, 7), (0x30090, 3)]);
    }
}
