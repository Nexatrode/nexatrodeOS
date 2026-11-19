//! Exact-once execution policy for the fixed Sunfish WLAN MSA handoff.
//!
//! The matched Linux driver treats a successful MEM_PROT_ASSIGN return as the
//! ownership result and has no independent ownership-query API. This
//! coordinator therefore records only an exact zero-status SMC acknowledgement
//! after verifying the live lifecycle and fixed no-map/exclusive profile; it
//! never labels that state as independently read back.
#![forbid(unsafe_code)]

use crate::{
    qcom_modem_handlers::Fault,
    qcom_modem_pools::{Call, Error as CallError},
    qcom_msa::{HandoffPlan, MsaReadyPermit},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ownership {
    Host,
    Transferred,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveObservation {
    pub generation: u64,
    pub identity: u64,
    pub ready: bool,
    /// True only when the trusted owner has established the matched SCM64
    /// return contract, exclusive no-map reservation, and serialized SMC use.
    pub contract: bool,
    /// Fresh-boot fixed MSA range is still owned by HLOS before any call.
    /// This is a boot-profile precondition, not an SCM readback.
    pub initial_hlos: bool,
}

/// `observe()` supplies only live lifecycle and fresh-boot profile evidence.
/// Transition state is advanced only after the matched SCM call returns exact
/// zero in both status registers; this API has no independent ownership query.
pub trait Io {
    fn observe(&mut self) -> Result<LiveObservation, Fault>;
    /// Stage only the exact call bytes in private uncached scratch and finish
    /// all required cache maintenance/barriers; this must not invoke SCM.
    fn stage(&mut self, call: &Call) -> Result<(), Fault>;
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault>;
    /// Finish ownership/cache ordering before the subsequent live readback.
    fn complete(&mut self) -> Result<(), Fault>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Lifecycle,
    Ownership,
    CallEncoding,
    Stage,
    Monitor,
    Completion,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub ownership: [Ownership; 2],
    pub attempted: u8,
    pub completed: u8,
    pub error: Option<Error>,
}

/// A serial two-transfer handoff. An ambiguous monitor result permanently
/// stops the object and marks the current range unknown. There is no transfer
/// replay, rollback, or implicit reclaim. The native SCM adapter may perform
/// only its bounded source-matched transport retries inside one transfer. The
/// local ledger records a successful SCM acknowledgement, not readback.
pub struct Handoff<T: Io> {
    io: T,
    plan: HandoffPlan,
    status: Status,
    busy: bool,
}

impl<T: Io> Handoff<T> {
    pub fn new(plan: HandoffPlan, mut io: T) -> Result<Self, Error> {
        let observation = io.observe().map_err(|_| Error::Lifecycle)?;
        if observation.generation != plan.generation()
            || observation.identity != plan.identity()
            || observation.identity == 0
            || !observation.ready
            || !observation.contract
            || !observation.initial_hlos
        {
            return Err(Error::Lifecycle);
        }
        Ok(Self {
            io,
            plan,
            status: Status {
                ownership: [Ownership::Host; 2],
                attempted: 0,
                completed: 0,
                error: None,
            },
            busy: false,
        })
    }

    pub fn status(&self) -> Status {
        self.status
    }

    /// Consume the coordinator and mint the one-shot protocol permit only
    /// after both transfers completed with exact successful SCM acknowledgments.
    pub fn into_msa_ready_permit(self) -> Result<MsaReadyPermit, Error> {
        if self.status.error.is_some() {
            return Err(Error::Stopped);
        }
        if self.status.completed != 2
            || self.status.attempted != 2
            || self.status.ownership != [Ownership::Transferred; 2]
            || self.busy
        {
            return Err(Error::Ownership);
        }
        Ok(MsaReadyPermit::from_verified_handoff(
            self.plan.generation(),
            self.plan.identity(),
        ))
    }

    /// Execute at most one fixed transfer. Returns true after both exact SCM
    /// success acknowledgments and lifecycle rechecks.
    pub fn step(&mut self) -> Result<bool, Error> {
        if self.status.error.is_some() {
            return Err(Error::Stopped);
        }
        if self.busy {
            return Err(self.stop(Error::Stopped));
        }
        if self.status.completed == 2 {
            return Ok(true);
        }
        self.revalidate()?;

        let index = usize::from(self.status.completed);
        let transfer = self.plan.transfers()[index];
        let call = Call::for_msa_transfer(transfer)
            .map_err(|_: CallError| self.stop(Error::CallEncoding))?;
        self.busy = true;
        if self.io.stage(&call).is_err() {
            return Err(self.stop(Error::Stage));
        }
        self.busy = false;

        // Revalidate after scratch staging and immediately before crossing the
        // secure monitor boundary.
        self.revalidate()?;
        self.status.ownership[index] = Ownership::Unknown;
        self.status.attempted += 1; // The plan bounds this to two invocations.
        self.busy = true;
        let result = match self.io.invoke(&call) {
            Ok(result) => result,
            Err(_) => return Err(self.stop(Error::Monitor)),
        };
        // The matched Linux `__qcom_scm_assign_mem()` returns qcom_scm_call's
        // transport status (SMC a0) unless that is zero, then returns the SCM
        // command status (SMC a1). Accept only exact zero/zero success.
        if result[0] != 0 || result[1] != 0 {
            return Err(self.stop(Error::Monitor));
        }
        if self.io.complete().is_err() {
            return Err(self.stop(Error::Completion));
        }
        self.busy = false;

        self.revalidate()?;
        self.status.ownership[index] = Ownership::Transferred;
        self.status.completed += 1;
        Ok(self.status.completed == 2)
    }

    fn revalidate(&mut self) -> Result<(), Error> {
        self.busy = true;
        let observation = match self.io.observe() {
            Ok(observation) => observation,
            Err(_) => return Err(self.stop(Error::Lifecycle)),
        };
        self.busy = false;
        if observation.generation != self.plan.generation()
            || observation.identity != self.plan.identity()
            || !observation.ready
            || !observation.contract
        {
            self.status.ownership = [Ownership::Unknown; 2];
            return Err(self.stop(Error::Lifecycle));
        }
        Ok(())
    }

    fn stop(&mut self, error: Error) -> Error {
        self.status.error = Some(error);
        self.busy = true;
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_msa;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Mode {
        Pass,
        StageFail,
        RevokeAfterStage,
        RevokeAfterCompleteFirst,
        InvokeFailFirst,
        NonzeroFirst,
        NonzeroSecond,
        CompleteFailFirst,
        InvokeFailSecond,
    }

    struct State {
        observation: LiveObservation,
        mode: Mode,
        stages: usize,
        invokes: usize,
        completes: usize,
        calls: Vec<([u64; 8], [u8; crate::qcom_modem_pools::ARG_BYTES])>,
    }

    #[derive(Clone)]
    struct Fake(Rc<RefCell<State>>);

    impl Fake {
        fn new(mode: Mode) -> Self {
            Self(Rc::new(RefCell::new(State {
                observation: LiveObservation {
                    generation: 7,
                    identity: 0xabc,
                    ready: true,
                    contract: true,
                    initial_hlos: true,
                },
                mode,
                stages: 0,
                invokes: 0,
                completes: 0,
                calls: Vec::new(),
            })))
        }

        fn state(&self) -> std::cell::Ref<'_, State> {
            self.0.borrow()
        }
    }

    impl Io for Fake {
        fn observe(&mut self) -> Result<LiveObservation, Fault> {
            Ok(self.0.borrow().observation)
        }

        fn stage(&mut self, call: &Call) -> Result<(), Fault> {
            let mut state = self.0.borrow_mut();
            state.stages += 1;
            state.calls.push((*call.words(), *call.bytes()));
            match state.mode {
                Mode::StageFail => Err(Fault::Failed),
                Mode::RevokeAfterStage => {
                    state.observation.ready = false;
                    Ok(())
                }
                _ => Ok(()),
            }
        }

        fn invoke(&mut self, _call: &Call) -> Result<[u64; 8], Fault> {
            let mut state = self.0.borrow_mut();
            state.invokes += 1;
            match state.mode {
                Mode::InvokeFailFirst if state.invokes == 1 => Err(Fault::Uncertain),
                Mode::InvokeFailSecond if state.invokes == 2 => Err(Fault::Uncertain),
                Mode::NonzeroFirst if state.invokes == 1 => Ok([1, 0, 0, 0, 0, 0, 0, 0]),
                Mode::NonzeroSecond if state.invokes == 1 => Ok([0, 1, 0, 0, 0, 0, 0, 0]),
                _ => Ok([0; 8]),
            }
        }

        fn complete(&mut self) -> Result<(), Fault> {
            let mut state = self.0.borrow_mut();
            state.completes += 1;
            if state.mode == Mode::CompleteFailFirst && state.completes == 1 {
                Err(Fault::Uncertain)
            } else {
                if state.mode == Mode::RevokeAfterCompleteFirst && state.completes == 1 {
                    state.observation.ready = false;
                }
                Ok(())
            }
        }
    }

    fn plan() -> HandoffPlan {
        let reservation = qcom_msa::Reservation::validate(
            7,
            0xabc,
            qcom_msa::SUNFISH_MSA_BASE,
            qcom_msa::SUNFISH_MSA_BYTES,
            true,
            true,
            &[],
        )
        .unwrap();
        qcom_msa::plan_handoff(
            reservation,
            &[
                qcom_msa::MsaRegion {
                    base: qcom_msa::SUNFISH_MSA_BASE,
                    bytes: 0x4000,
                    secure: true,
                },
                qcom_msa::MsaRegion {
                    base: qcom_msa::SUNFISH_MSA_BASE + 0x4000,
                    bytes: 0x1fc000,
                    secure: false,
                },
            ],
        )
        .unwrap()
    }

    #[test]
    fn serializes_both_ranges_and_records_exact_scm_acknowledgements() {
        let peer = Fake::new(Mode::Pass);
        let mut handoff = Handoff::new(plan(), peer.clone()).unwrap();
        assert!(!handoff.step().unwrap());
        assert_eq!(
            handoff.status(),
            Status {
                ownership: [Ownership::Transferred, Ownership::Host],
                attempted: 1,
                completed: 1,
                error: None,
            }
        );
        assert!(handoff.step().unwrap());
        assert!(handoff.step().unwrap());
        assert_eq!(
            handoff.status(),
            Status {
                ownership: [Ownership::Transferred; 2],
                attempted: 2,
                completed: 2,
                error: None,
            }
        );
        let state = peer.state();
        assert_eq!((state.stages, state.invokes, state.completes), (2, 2, 2));
        assert_eq!(state.calls.len(), 2);
        assert_eq!(
            u64::from_le_bytes(state.calls[0].1[..8].try_into().unwrap()),
            qcom_msa::SUNFISH_MSA_BASE
        );
        assert_eq!(
            u64::from_le_bytes(state.calls[1].1[..8].try_into().unwrap()),
            qcom_msa::SUNFISH_MSA_BASE + 0x4000
        );
        let transfers = plan().transfers();
        assert_eq!(
            [transfers[0].region(), transfers[1].region()],
            [
                qcom_msa::Range {
                    base: qcom_msa::SUNFISH_MSA_BASE,
                    bytes: 0x4000
                },
                qcom_msa::Range {
                    base: qcom_msa::SUNFISH_MSA_BASE + 0x4000,
                    bytes: 0x1fc000
                }
            ]
        );
    }

    #[test]
    fn preflight_rejects_stale_contract_and_missing_fresh_hlos_profile() {
        for mutate in [0, 1, 2, 3] {
            let peer = Fake::new(Mode::Pass);
            {
                let mut state = peer.0.borrow_mut();
                match mutate {
                    0 => state.observation.contract = false,
                    1 => state.observation.ready = false,
                    2 => state.observation.identity += 1,
                    _ => state.observation.initial_hlos = false,
                }
            }
            let result = Handoff::new(plan(), peer.clone());
            assert!(matches!(result, Err(Error::Lifecycle)));
            let state = peer.state();
            assert_eq!((state.stages, state.invokes), (0, 0));
        }
    }

    #[test]
    fn failures_stop_without_retry_or_implicit_reclaim() {
        for (mode, expected, attempted, ownership) in [
            (Mode::StageFail, Error::Stage, 0, [Ownership::Host; 2]),
            (
                Mode::RevokeAfterStage,
                Error::Lifecycle,
                0,
                [Ownership::Unknown; 2],
            ),
            (
                Mode::RevokeAfterCompleteFirst,
                Error::Lifecycle,
                1,
                [Ownership::Unknown; 2],
            ),
            (
                Mode::InvokeFailFirst,
                Error::Monitor,
                1,
                [Ownership::Unknown, Ownership::Host],
            ),
            (
                Mode::NonzeroFirst,
                Error::Monitor,
                1,
                [Ownership::Unknown, Ownership::Host],
            ),
            (
                Mode::NonzeroSecond,
                Error::Monitor,
                1,
                [Ownership::Unknown, Ownership::Host],
            ),
            (
                Mode::CompleteFailFirst,
                Error::Completion,
                1,
                [Ownership::Unknown, Ownership::Host],
            ),
        ] {
            let peer = Fake::new(mode);
            let mut handoff = Handoff::new(plan(), peer.clone()).unwrap();
            assert_eq!(handoff.step(), Err(expected), "{mode:?}");
            assert_eq!(handoff.step(), Err(Error::Stopped), "{mode:?}");
            let status = handoff.status();
            assert_eq!(status.error, Some(expected));
            assert_eq!(status.attempted, attempted);
            assert_eq!(status.ownership, ownership);
            let state = peer.state();
            assert_eq!(state.invokes, usize::from(attempted != 0), "{mode:?}");
        }
    }

    #[test]
    fn second_range_failure_retains_verified_first_handoff_and_marks_second_unknown() {
        let peer = Fake::new(Mode::InvokeFailSecond);
        let mut handoff = Handoff::new(plan(), peer.clone()).unwrap();
        assert!(!handoff.step().unwrap());
        assert_eq!(handoff.step(), Err(Error::Monitor));
        assert_eq!(handoff.step(), Err(Error::Stopped));
        assert_eq!(
            handoff.status(),
            Status {
                ownership: [Ownership::Transferred, Ownership::Unknown],
                attempted: 2,
                completed: 1,
                error: Some(Error::Monitor),
            }
        );
        let state = peer.state();
        assert_eq!((state.stages, state.invokes, state.completes), (2, 2, 1));
    }
}
