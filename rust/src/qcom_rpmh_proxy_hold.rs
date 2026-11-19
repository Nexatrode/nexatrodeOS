//! Combine the observed MC0 reassertion and CX/MSS/XO hold experiments.
//!
//! This is a fixed active-resource experiment, not complete proxy power or a
//! bandwidth provider. It supplies no AOP-on, PAS, reset, firmware, DMA, retry,
//! or automatic unvote. Exclusive standalone AP DRV2 ownership is required.

use crate::qcom_bcm_vote::Decoded;
use crate::qcom_rpmh_bus_hold::{self as mc0, EXPECTED_READS};
use crate::qcom_rpmh_handoff::{self as handoff, ReadRegisters};
use crate::qcom_rpmh_power::EXPECTED_START;
use crate::qcom_rpmh_read::{
    ActiveTransaction, BcmResource, Command, Error as ControllerError, ReadResource, RegisterIo,
    Snapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Mc0,
    InitialRails,
    Cx,
    Mss,
    Xo,
    FinalBus,
    Held,
    Sn11Floor,
    Sn0Floor,
    Ce0Floor,
    FinalRails,
    FloorsHeld,
}
impl Phase {
    pub const fn code(self) -> u32 {
        self as u32
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mc0 => "CHECK AND HOLD MC0",
            Self::InitialRails => "CHECK STARTING RAILS",
            Self::Cx => "HOLD CX AND CHECK",
            Self::Mss => "HOLD MSS AND CHECK",
            Self::Xo => "HOLD XO AND CHECK",
            Self::FinalBus => "CHECK FINAL BUS REPLIES",
            Self::Held => "FOUR REQUESTS HELD",
            Self::Sn11Floor => "HOLD SN11 AND CHECK",
            Self::Sn0Floor => "HOLD SN0 AND CHECK",
            Self::Ce0Floor => "HOLD CE0 AND CHECK",
            Self::FinalRails => "CHECK RAILS AGAIN",
            Self::FloorsHeld => "SEVEN REQUESTS HELD",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Mc0,
    Capture,
    BankChanged,
    Controller,
    RailValues,
    BusValues,
    FloorValues,
    FinalRailValues,
    FloorStartValues,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub initial_bus: [[Option<u32>; 6]; 2],
    /// Initial, after CX, after MSS, after XO; two passes per stage.
    pub rails: [[[Option<u32>; 4]; 2]; 4],
    pub bus: [[Option<u32>; 6]; 2],
    /// Bits 0..3 are MC0/CX/MSS/XO; the floor extension adds SN11/SN0/CE0 at
    /// bits 4..6. Submission does not imply acknowledgement.
    pub submitted_mask: u32,
    pub acknowledged_mask: u32,
    pub requests: u32,
    pub checks: u32,
    pub phase: Phase,
    pub failed_request: Option<u32>,
    pub state: Option<Snapshot>,
    pub error: Option<Error>,
    pub mc0_error: Option<mc0::Error>,
    pub controller_error: Option<ControllerError>,
    pub handoff_error: Option<handoff::Error>,
    pub(crate) handoff: Option<handoff::Snapshot>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.phase == Phase::Held
            && self.submitted_mask == 15
            && self.acknowledged_mask == 15
            && self.requests == 72
            && self.checks == 144
            && self.rails.iter().flatten().flatten().all(Option::is_some)
            && self.bus.iter().flatten().all(Option::is_some)
    }

    /// Strict experimental postcondition, not a general invariant of rail changes.
    pub fn bus_difference_mask(&self) -> u32 {
        (0..6).fold(0, |mask, i| {
            let baseline = self.initial_bus[0][i];
            let stable_baseline = baseline.is_some() && baseline == self.initial_bus[1][i];
            let same = stable_baseline
                && (0..2).all(|pass| {
                    self.bus[pass][i].is_some_and(|raw| {
                        if i == 0 {
                            Decoded::parse(raw).is_ok_and(|v| {
                                v.same_request(Decoded::parse(EXPECTED_READS[0]).unwrap())
                            })
                        } else {
                            Some(raw) == baseline
                        }
                    })
                });
            mask | (u32::from(!same) << i)
        })
    }
}

fn checked_request<I: RegisterIo + ReadRegisters>(
    io: &mut I,
    baseline: handoff::Snapshot,
    command: Command,
    write_bit: u32,
    polls: u32,
    wait: &mut impl FnMut(),
    r: &mut Report,
) -> Result<Option<u32>, ()> {
    r.failed_request = Some(r.requests + 1);
    let before = handoff::capture_stable(io, 3).map_err(|e| {
        r.handoff_error = Some(e);
        r.error = Some(Error::Capture);
    })?;
    r.checks += 1;
    if before != baseline {
        r.error = Some(Error::BankChanged);
        return Err(());
    }
    let mut transaction = ActiveTransaction::new(command, polls);
    let result = transaction.start(io).and_then(|()| {
        r.requests += 1;
        r.submitted_mask |= write_bit;
        loop {
            if transaction.poll(io)? {
                return Ok(());
            }
            wait();
        }
    });
    r.state = Some(transaction.last());
    if let Err(e) = result {
        r.state = Some(Snapshot::read(io));
        r.controller_error = Some(e);
        r.error = Some(Error::Controller);
        return Err(());
    }
    r.acknowledged_mask |= write_bit;
    let after = handoff::capture_stable(io, 3).map_err(|e| {
        r.handoff_error = Some(e);
        r.error = Some(Error::Capture);
    })?;
    r.checks += 1;
    if after != baseline {
        r.error = Some(Error::BankChanged);
        return Err(());
    }
    r.failed_request = None;
    // A READ is published only after the full preservation check passes.
    Ok(transaction.response())
}

/// MC0 first, then fixed CX6/MSS7/XO3, preserving all eight programmed banks
/// around every request. Stop at the first fault, including after an applied
/// WRITE with lost acknowledgement; leave held requests for physical restart.
pub fn hold_proxy_resources<I: RegisterIo + ReadRegisters>(
    io: &mut I,
    polls: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(Phase),
) -> Report {
    progress(Phase::Mc0);
    let bus = mc0::hold_mc0(io, polls, &mut wait, |_| {});
    let mut r = Report {
        initial_bus: bus.before.responses,
        rails: [[[None; 4]; 2]; 4],
        bus: [[None; 6]; 2],
        submitted_mask: u32::from(bus.write_submitted),
        acknowledged_mask: u32::from(bus.write_acknowledged),
        requests: u32::from(bus.before.submitted)
            + u32::from(bus.write_submitted)
            + bus.after.map_or(0, |a| u32::from(a.submitted)),
        checks: bus.preserved_checks(),
        phase: Phase::Mc0,
        failed_request: None,
        state: bus.state,
        error: None,
        mc0_error: bus.error,
        controller_error: bus.controller_error,
        handoff_error: bus.handoff_error,
        handoff: bus.guard_after,
    };
    if !bus.passed() {
        r.error = Some(Error::Mc0);
        return r;
    }
    let baseline = bus.guard_after.unwrap();
    for (stage, phase) in [Phase::InitialRails, Phase::Cx, Phase::Mss, Phase::Xo]
        .into_iter()
        .enumerate()
    {
        r.phase = phase;
        progress(phase);
        if stage != 0 {
            let command = [Command::CxTurbo, Command::MssTurbo, Command::XoOn][stage - 1];
            if checked_request(io, baseline, command, 1 << stage, polls, &mut wait, &mut r).is_err()
            {
                return r;
            }
        }
        for pass in 0..2 {
            for resource in ReadResource::ALL {
                match checked_request(
                    io,
                    baseline,
                    Command::Read(resource),
                    0,
                    polls,
                    &mut wait,
                    &mut r,
                ) {
                    Ok(value) => r.rails[stage][pass][resource.index()] = value,
                    Err(()) => return r,
                }
            }
        }
        let cx = if stage == 0 { 5 } else { 6 };
        let mss = if stage < 2 { 0 } else { 7 };
        if !(0..2).all(|pass| {
            let values = r.rails[stage][pass];
            values[0] == Some(cx)
                && values[1] == Some(mss)
                && values[3] == Some(3)
                && (stage != 0 || values == EXPECTED_START.map(Some))
        }) {
            r.error = Some(Error::RailValues);
            return r;
        }
        // MX is observed after each step, without inventing a coupling rule.
    }
    r.phase = Phase::FinalBus;
    progress(r.phase);
    for pass in 0..2 {
        for resource in BcmResource::ALL {
            match checked_request(
                io,
                baseline,
                Command::ReadBcm(resource),
                0,
                polls,
                &mut wait,
                &mut r,
            ) {
                Ok(value) => r.bus[pass][resource.index()] = value,
                Err(()) => return r,
            }
        }
    }
    // A changed reply is retained for diagnosis. Do not compensate, infer a
    // rollback vote, or declare full modem readiness from these resource reads.
    if r.bus_difference_mask() != 0 {
        r.error = Some(Error::BusValues);
        return r;
    }
    r.phase = Phase::Held;
    progress(r.phase);
    r
}

#[derive(Clone, Copy, Debug)]
pub struct FloorReport {
    /// Counters, masks, phase and failure diagnostics continue from the base run.
    pub controller: Report,
    pub base_passed: bool,
    pub bus_stages: [[[Option<u32>; 6]; 2]; 3],
    pub final_rails: [[Option<u32>; 4]; 2],
}
impl FloorReport {
    pub fn passed(&self) -> bool {
        self.base_passed
            && self.controller.error.is_none()
            && self.controller.phase == Phase::FloorsHeld
            && self.controller.requests == 119
            && self.controller.checks == 238
            && self.controller.submitted_mask == 127
            && self.controller.acknowledged_mask == 127
            && self
                .bus_stages
                .iter()
                .flatten()
                .flatten()
                .all(Option::is_some)
            && self.final_rails.iter().flatten().all(Option::is_some)
    }
    pub fn bus_difference_mask(&self) -> u32 {
        floor_difference(self.bus_stages[2], 2, self.controller.initial_bus)
    }
}

fn floor_difference(
    responses: [[Option<u32>; 6]; 2],
    stage: usize,
    initial: [[Option<u32>; 6]; 2],
) -> u32 {
    use crate::qcom_rpmh_read::{CE0_PAS_FLOOR_WORD, SN_PAS_FLOOR_WORD};
    (0..6).fold(0, |mask, i| {
        let floor_value = match i {
            3 => Some(SN_PAS_FLOOR_WORD),
            4 if stage >= 1 => Some(SN_PAS_FLOOR_WORD),
            2 if stage >= 2 => Some(CE0_PAS_FLOOR_WORD),
            _ => None,
        };
        let baseline = initial[0][i];
        let stable_baseline = baseline.is_some() && baseline == initial[1][i];
        let same = stable_baseline
            && (0..2).all(|pass| {
                responses[pass][i].is_some_and(|raw| {
                    if i == 0 {
                        Decoded::parse(raw).is_ok_and(|v| {
                            v.same_request(Decoded::parse(EXPECTED_READS[0]).unwrap())
                        })
                    } else if let Some(expected) = floor_value {
                        Decoded::parse(raw)
                            .is_ok_and(|v| v.same_request(Decoded::parse(expected).unwrap()))
                    } else {
                        Some(raw) == baseline
                    }
                })
            });
        mask | (u32::from(!same) << i)
    })
}

/// After the observed combined hold, test three fixed nondecreasing bus floors
/// in downstream VCD3 -> upstream VCD6 order. Each is a separate committed
/// single-resource request (the legacy single-vote ABI), not an atomic batch.
/// CE0 saturates: PASS means request/readback/preservation, never sufficient
/// secure-loader bandwidth, complete aggregation/QoS, or PAS readiness.
pub fn hold_pas_bus_floor<I: RegisterIo + ReadRegisters>(
    io: &mut I,
    polls: u32,
    mut wait: impl FnMut(),
    mut progress: impl FnMut(Phase),
) -> FloorReport {
    let controller = hold_proxy_resources(io, polls, &mut wait, &mut progress);
    let mut r = FloorReport {
        base_passed: controller.passed(),
        controller,
        bus_stages: [[[None; 6]; 2]; 3],
        final_rails: [[None; 4]; 2],
    };
    if !r.base_passed {
        return r;
    }
    // The fixed SN/CE floor words were derived from this recorded starting
    // profile. Keep the no-reduction guarantee by requiring each target's
    // starting value to match in both reads before submitting any floor WRITE.
    let floor_targets = [2, 3, 4]; // CE0, SN11, SN0
    let fixed_floor_inputs_match = floor_targets.into_iter().all(|index| {
        (0..2).all(|pass| r.controller.initial_bus[pass][index] == Some(EXPECTED_READS[index]))
    });
    if !fixed_floor_inputs_match {
        r.controller.error = Some(Error::FloorStartValues);
        return r;
    }
    let baseline = r.controller.handoff.unwrap();
    for (stage, (phase, command)) in [
        (Phase::Sn11Floor, Command::Sn11PasFloor),
        (Phase::Sn0Floor, Command::Sn0PasFloor),
        (Phase::Ce0Floor, Command::Ce0PasFloor),
    ]
    .into_iter()
    .enumerate()
    {
        r.controller.phase = phase;
        progress(phase);
        if checked_request(
            io,
            baseline,
            command,
            1 << (stage + 4),
            polls,
            &mut wait,
            &mut r.controller,
        )
        .is_err()
        {
            return r;
        }
        for pass in 0..2 {
            for resource in BcmResource::ALL {
                match checked_request(
                    io,
                    baseline,
                    Command::ReadBcm(resource),
                    0,
                    polls,
                    &mut wait,
                    &mut r.controller,
                ) {
                    Ok(value) => r.bus_stages[stage][pass][resource.index()] = value,
                    Err(()) => return r,
                }
            }
        }
        if floor_difference(r.bus_stages[stage], stage, r.controller.initial_bus) != 0 {
            r.controller.error = Some(Error::FloorValues);
            return r;
        }
    }
    r.controller.phase = Phase::FinalRails;
    progress(r.controller.phase);
    for pass in 0..2 {
        for resource in ReadResource::ALL {
            match checked_request(
                io,
                baseline,
                Command::Read(resource),
                0,
                polls,
                &mut wait,
                &mut r.controller,
            ) {
                Ok(value) => r.final_rails[pass][resource.index()] = value,
                Err(()) => return r,
            }
        }
    }
    if !(0..2).all(|pass| {
        r.final_rails[pass][0] == Some(6)
            && r.final_rails[pass][1] == Some(7)
            && r.final_rails[pass][3] == Some(3)
    }) {
        r.controller.error = Some(Error::FinalRailValues);
        return r;
    }
    r.controller.phase = Phase::FloorsHeld;
    progress(r.controller.phase);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_rpmh_handoff::{BankField, MAPPING_BYTES, OBSERVED_CONFIG, Register as Guard};
    use crate::qcom_rpmh_read::{MC0_REASSERT_WORD, Register};
    use std::vec::Vec;

    struct Fake {
        words: [u32; MAPPING_BYTES / 4],
        rails: [u32; 4],
        bus: [u32; 6],
        requests: Vec<(u32, u32, u32, u32)>,
        stop_at: Option<usize>,
        change_bank_after: Option<(usize, usize)>,
        change_capture: Option<usize>,
        captures: usize,
        reject_write: Option<usize>,
        change_final_bus: Option<usize>,
        change_mx: bool,
        drop_mss_after_ce0: bool,
    }
    impl Fake {
        fn ready() -> Self {
            let mut f = Self {
                words: [0; MAPPING_BYTES / 4],
                rails: EXPECTED_START,
                bus: EXPECTED_READS,
                requests: Vec::new(),
                stop_at: None,
                change_bank_after: None,
                change_capture: None,
                captures: 0,
                reject_write: None,
                change_final_bus: None,
                change_mx: false,
                drop_mss_after_ce0: false,
            };
            f.words[0xc / 4] = OBSERVED_CONFIG;
            f.words[0xd00 / 4] = 1;
            for bank in 0..8 {
                f.words[Guard::bank(bank, BankField::Idle).unwrap().offset() / 4] = 1;
            }
            f
        }
        fn set(&mut self, r: Register, v: u32) {
            self.words[r.offset() / 4] = v;
        }
        fn bank_change(&mut self, bank: usize) {
            self.words[Guard::bank(bank, BankField::Wait).unwrap().offset() / 4] = 1;
        }
        fn run(&mut self) -> Report {
            hold_proxy_resources(self, 1, || {}, |_| {})
        }
    }
    impl RegisterIo for Fake {
        fn read(&mut self, r: Register) -> u32 {
            self.words[r.offset() / 4]
        }
        fn write(&mut self, r: Register, v: u32) {
            if r == Register::IrqClear {
                self.words[Register::IrqStatus.offset() / 4] &= !v;
                if let Some((at, bank)) = self.change_bank_after {
                    if self.requests.len() == at {
                        self.bank_change(bank);
                    }
                }
            } else {
                self.set(r, v);
            }
            if r == Register::Control && v == 0 {
                self.set(Register::CommandStatus, 0);
            }
            if r != Register::Control || v != 0x1010000 {
                return;
            }
            let message = self.words[0xd30 / 4];
            let address = self.words[0xd34 / 4];
            let data = self.words[0xd38 / 4];
            let wait = self.words[0xd10 / 4];
            self.requests.push((message, address, data, wait));
            let n = self.requests.len();
            let response;
            if message == 0x10108 {
                assert!(
                    [
                        (0x50000, MC0_REASSERT_WORD, 1),
                        (0x30000, 6, 0),
                        (0x30060, 7, 0),
                        (0x30090, 3, 0),
                        (0x5006c, 0x613c5d02, 1),
                        (0x50078, 0x613c5d02, 1),
                        (0x500a0, 0x6fffffff, 1)
                    ]
                    .contains(&(address, data, wait))
                );
                if self.reject_write != Some(n) {
                    if let Some(i) = ReadResource::ALL
                        .iter()
                        .position(|r| r.address() == address)
                    {
                        self.rails[i] = data;
                    } else if let Some(i) =
                        BcmResource::ALL.iter().position(|r| r.address() == address)
                    {
                        self.bus[i] = data & !(1 << 30);
                    }
                }
                if self.change_mx && address == 0x30000 {
                    self.rails[2] = 4;
                }
                if self.drop_mss_after_ce0 && address == 0x500a0 {
                    self.rails[1] = 0;
                }
                response = 0; // WRITEs do not publish a READ response.
            } else {
                assert_eq!((message, data, wait), (0x108, 0, 0));
                response = if let Some(i) = ReadResource::ALL
                    .iter()
                    .position(|r| r.address() == address)
                {
                    self.rails[i]
                } else {
                    let i = BcmResource::ALL
                        .iter()
                        .position(|r| r.address() == address)
                        .unwrap();
                    self.bus[i] ^ u32::from(n > 60 && self.change_final_bus == Some(i))
                };
            }
            self.set(Register::Idle, 0);
            // A WRITE may already be applied when the acknowledgement is lost.
            if self.stop_at == Some(n) {
                return;
            }
            self.set(Register::IrqStatus, 1);
            self.set(Register::CommandStatus, 0x10100);
            self.set(Register::Response, response);
            self.set(Register::Idle, 1);
        }
    }
    impl ReadRegisters for Fake {
        fn read(&mut self, r: Guard) -> u32 {
            if r == Guard::CONFIG {
                self.captures += 1;
                if self.change_capture == Some(self.captures) {
                    self.bank_change(7);
                }
            }
            self.words[r.offset() / 4]
        }
    }

    #[test]
    fn combines_exactly_four_writes_in_bus_cx_mss_xo_order_with_full_preservation() {
        let mut f = Fake::ready();
        let r = f.run();
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.requests, 72);
        assert_eq!(r.checks, 144);
        let writes: Vec<_> = f
            .requests
            .iter()
            .enumerate()
            .filter(|(_, (m, _, _, _))| *m == 0x10108)
            .map(|(i, v)| (i + 1, *v))
            .collect();
        assert_eq!(
            writes,
            [
                (13, (0x10108, 0x50000, 0x69917f75, 1)),
                (34, (0x10108, 0x30000, 6, 0)),
                (43, (0x10108, 0x30060, 7, 0)),
                (52, (0x10108, 0x30090, 3, 0)),
            ]
        );
        assert_eq!(r.rails[3], [[Some(6), Some(7), Some(3), Some(3)]; 2]);
        assert_eq!(r.bus, [EXPECTED_READS.map(Some); 2]);
    }

    #[test]
    fn every_timeout_is_terminal_even_when_a_write_was_applied() {
        for stop in 1..=72 {
            let mut f = Fake::ready();
            f.stop_at = Some(stop);
            let r = f.run();
            assert!(!r.passed());
            assert_eq!(f.requests.len(), stop);
            assert_eq!(r.requests as usize, stop);
            let submitted = [13, 34, 43, 52]
                .into_iter()
                .enumerate()
                .fold(0, |m, (i, n)| m | (u32::from(stop >= n) << i));
            let acknowledged = [13, 34, 43, 52]
                .into_iter()
                .enumerate()
                .fold(0, |m, (i, n)| m | (u32::from(stop > n) << i));
            assert_eq!(r.submitted_mask, submitted);
            assert_eq!(r.acknowledged_mask, acknowledged);
        }
    }

    #[test]
    fn each_bank_change_after_each_combined_transaction_stops_later_requests() {
        for request in 26..=72 {
            for bank in 0..8 {
                let mut f = Fake::ready();
                f.change_bank_after = Some((request, bank));
                let r = f.run();
                assert!(r.error.is_some());
                assert_eq!(f.requests.len(), request);
            }
        }
    }

    #[test]
    fn changed_handoff_between_transactions_blocks_next_submission() {
        // Two captures per guard, two guards per successful request.
        for request in 26..=72 {
            let mut f = Fake::ready();
            f.change_capture = Some((request - 1) * 4 + 1);
            let r = f.run();
            assert_eq!(r.error, Some(Error::BankChanged));
            assert_eq!(f.requests.len(), request - 1);
        }
    }

    #[test]
    fn wrong_starting_rails_allow_no_rail_write_and_failed_rail_readback_blocks_next_step() {
        for i in 0..4 {
            let mut f = Fake::ready();
            f.rails[i] ^= 1;
            let r = f.run();
            assert_eq!(r.error, Some(Error::RailValues));
            assert_eq!(r.submitted_mask, 1);
            assert_eq!(f.requests.len(), 33);
        }
        for (request, length) in [(34, 42), (43, 51)] {
            let mut f = Fake::ready();
            f.reject_write = Some(request);
            let r = f.run();
            assert_eq!(r.error, Some(Error::RailValues));
            assert_eq!(f.requests.len(), length);
        }
    }

    #[test]
    fn final_bus_changes_are_reported_without_any_compensation() {
        for i in 0..6 {
            let mut f = Fake::ready();
            f.change_final_bus = Some(i);
            let r = f.run();
            assert_eq!(r.error, Some(Error::BusValues));
            assert_eq!(r.bus_difference_mask(), 1 << i);
            assert_eq!(f.requests.len(), 72);
            assert_eq!(r.acknowledged_mask, 15);
        }
    }

    #[test]
    fn stable_non_target_boot_profile_is_preserved_across_combined_hold() {
        let mut f = Fake::ready();
        f.bus[5] = 0x240d5adf;
        let r = f.run();
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.initial_bus[0][5], Some(0x240d5adf));
        assert_eq!(r.initial_bus[1][5], Some(0x240d5adf));
        assert_eq!(r.bus[0][5], Some(0x240d5adf));
        assert_eq!(r.bus[1][5], Some(0x240d5adf));
        assert_eq!(r.bus_difference_mask(), 0);
    }

    #[test]
    fn mx_coupling_is_observed_without_an_invented_write_or_postcondition() {
        let mut f = Fake::ready();
        f.change_mx = true;
        let r = f.run();
        assert!(r.passed());
        assert_eq!(r.rails[3][1][2], Some(4));
        assert!(
            !f.requests
                .iter()
                .any(|(m, a, _, _)| *m == 0x10108 && *a == 0x30010)
        );
    }

    #[test]
    fn zero_budget_submits_nothing() {
        let mut f = Fake::ready();
        let r = hold_proxy_resources(&mut f, 0, || {}, |_| {});
        assert!(!r.passed());
        assert!(f.requests.is_empty());
    }

    #[test]
    fn pas_bus_floors_use_three_fixed_commits_without_reducing_inherited_fields() {
        let mut f = Fake::ready();
        let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.controller.requests, 119);
        assert_eq!(r.controller.checks, 238);
        let writes: Vec<_> = f
            .requests
            .iter()
            .enumerate()
            .filter(|(_, (m, _, _, _))| *m == 0x10108)
            .map(|(i, v)| (i + 1, *v))
            .collect();
        assert_eq!(
            &writes[4..],
            &[
                (73, (0x10108, 0x5006c, 0x613c5d02, 1)),
                (86, (0x10108, 0x50078, 0x613c5d02, 1)),
                (99, (0x10108, 0x500a0, 0x6fffffff, 1)),
            ]
        );
        assert_eq!(writes.len(), 7);
        assert_eq!(
            r.bus_stages[2],
            [[
                Some(0x29917f75),
                Some(0x20000008),
                Some(0x2fffffff),
                Some(0x213c5d02),
                Some(0x213c5d02),
                Some(0x24005adf)
            ]; 2]
        );
        assert_eq!(r.final_rails, [[Some(6), Some(7), Some(3), Some(3)]; 2]);
        assert_eq!(r.bus_difference_mask(), 0);
        for i in 0..6 {
            let before = Decoded::parse(EXPECTED_READS[i]).unwrap();
            let after = Decoded::parse(r.bus_stages[2][0][i].unwrap()).unwrap();
            assert!(after.vector.x >= before.vector.x && after.vector.y >= before.vector.y);
        }
    }

    #[test]
    fn pas_floors_preserve_stable_non_target_boot_profile() {
        let mut f = Fake::ready();
        f.bus[5] = 0x240d5adf;
        let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.controller.initial_bus[0][5], Some(0x240d5adf));
        assert_eq!(r.bus_stages[2][0][5], Some(0x240d5adf));
        assert_eq!(r.bus_stages[2][1][5], Some(0x240d5adf));
        assert_eq!(r.bus_difference_mask(), 0);
    }

    #[test]
    fn fixed_floor_words_are_refused_when_their_target_starting_value_differs() {
        for index in [2, 3, 4] {
            let mut f = Fake::ready();
            f.bus[index] ^= 1;
            let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
            assert!(r.base_passed, "{index}: {r:?}");
            assert_eq!(r.controller.error, Some(Error::FloorStartValues));
            assert_eq!(r.controller.requests, 72);
            assert_eq!(r.controller.acknowledged_mask, 15);
            assert_eq!(f.requests.len(), 72);
            assert!(!f.requests.iter().any(|(message, address, _, _)| {
                *message == 0x10108 && [0x500a0, 0x5006c, 0x50078].contains(address)
            }));
        }
    }

    #[test]
    fn every_added_floor_timeout_stops_without_retries_or_automatic_unvote() {
        for stop in 73..=119 {
            let mut f = Fake::ready();
            f.stop_at = Some(stop);
            let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
            assert!(!r.passed());
            assert_eq!(f.requests.len(), stop);
            assert_eq!(r.controller.failed_request, Some(stop as u32));
            let expected_submitted = [13, 34, 43, 52, 73, 86, 99]
                .into_iter()
                .enumerate()
                .fold(0, |m, (i, n)| m | (u32::from(stop >= n) << i));
            let expected_ack = [13, 34, 43, 52, 73, 86, 99]
                .into_iter()
                .enumerate()
                .fold(0, |m, (i, n)| m | (u32::from(stop > n) << i));
            assert_eq!(r.controller.submitted_mask, expected_submitted);
            assert_eq!(r.controller.acknowledged_mask, expected_ack);
        }
    }

    #[test]
    fn every_bank_change_after_each_floor_transaction_stops_further_commands() {
        for request in 73..=119 {
            for bank in 0..8 {
                let mut f = Fake::ready();
                f.change_bank_after = Some((request, bank));
                let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
                assert!(!r.passed());
                assert_eq!(f.requests.len(), request);
            }
        }
    }

    #[test]
    fn each_unapplied_bus_floor_blocks_the_next_write() {
        for request in [73, 86, 99] {
            let mut f = Fake::ready();
            f.reject_write = Some(request);
            let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
            assert_eq!(r.controller.error, Some(Error::FloorValues));
            assert_eq!(f.requests.len(), request + 12);
        }
    }

    #[test]
    fn lost_rail_hold_after_bus_writes_is_reported_without_a_compensating_write() {
        let mut f = Fake::ready();
        f.drop_mss_after_ce0 = true;
        let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
        assert_eq!(r.controller.error, Some(Error::FinalRailValues));
        assert_eq!(r.controller.acknowledged_mask, 127);
        assert_eq!(f.requests.len(), 119);
    }

    #[test]
    fn failed_base_experiment_never_attempts_an_additional_bus_floor() {
        for stop in [1, 13, 34, 43, 52, 72] {
            let mut f = Fake::ready();
            f.stop_at = Some(stop);
            let r = hold_pas_bus_floor(&mut f, 1, || {}, |_| {});
            assert!(!r.base_passed && !r.passed());
            assert_eq!(f.requests.len(), stop);
            assert_eq!(r.controller.submitted_mask & !15, 0);
        }
    }
}
