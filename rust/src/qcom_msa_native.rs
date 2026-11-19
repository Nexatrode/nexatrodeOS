//! Fixed native adapter for the two Sunfish WLAN MSA SCM assignments.
//!
//! This composes a live lifecycle gate, a private scratch writer, and one
//! serialized SCM capability. It never maps the WLAN MSA reservation itself.
#![forbid(unsafe_code)]

use crate::{
    qcom_modem_handlers::Fault,
    qcom_modem_pools::{ARG_BYTES, Call},
    qcom_msa::{HandoffPlan, Reservation},
    qcom_msa_handoff::{self, LiveObservation},
};

pub trait Lifecycle {
    fn revalidate(&mut self, generation: u64, identity: u64) -> Result<(), Fault>;
}

pub trait Scm {
    fn invoke(
        &mut self,
        call: &Call,
        lifecycle: &mut dyn FnMut() -> Result<(), Fault>,
    ) -> Result<[u64; 8], Fault>;
}

pub trait Scratch {
    fn stage(&mut self, bytes: &[u8; ARG_BYTES]) -> Result<(), Fault>;
    fn complete(&mut self) -> Result<(), Fault>;
}

/// The only `qcom_msa_handoff::Io` implementation intended for the native
/// owner. Each staged call must byte-match the sealed plan and exact order.
pub struct Native<L, S, B> {
    lifecycle: L,
    scm: S,
    scratch: B,
    reservation: Reservation,
    plan: HandoffPlan,
    staged: u8,
    attempted: u8,
    last_reply: Option<[u64; 8]>,
    failed: bool,
}

impl<L, S, B> Native<L, S, B> {
    pub fn new(
        plan: HandoffPlan,
        reservation: Reservation,
        lifecycle: L,
        scm: S,
        scratch: B,
    ) -> Result<Self, Fault> {
        if plan.generation() != reservation.generation()
            || plan.identity() != reservation.identity()
            || plan.identity() == 0
        {
            return Err(Fault::Failed);
        }
        for transfer in plan.transfers() {
            Call::for_msa_transfer(transfer).map_err(|_| Fault::Failed)?;
        }
        Ok(Self {
            lifecycle,
            scm,
            scratch,
            reservation,
            plan,
            staged: 0,
            attempted: 0,
            last_reply: None,
            failed: false,
        })
    }
}

impl<L: Lifecycle, S: Scm, B: Scratch> qcom_msa_handoff::Io for Native<L, S, B> {
    fn observe(&mut self) -> Result<LiveObservation, Fault> {
        if self.failed {
            return Err(Fault::Uncertain);
        }
        if let Err(error) = self
            .lifecycle
            .revalidate(self.reservation.generation(), self.reservation.identity())
        {
            self.failed = true;
            return Err(error);
        }
        Ok(LiveObservation {
            generation: self.reservation.generation(),
            identity: self.reservation.identity(),
            ready: true,
            // The native constructor is reachable only after the platform
            // validated the no-map MSA profile and the reserved SCM scratch.
            contract: true,
            initial_hlos: self.attempted == 0,
        })
    }

    fn stage(&mut self, call: &Call) -> Result<(), Fault> {
        if self.failed || self.staged != self.attempted || self.attempted >= 2 {
            self.failed = true;
            return Err(Fault::Failed);
        }
        let expected = Call::for_msa_transfer(self.plan.transfers()[self.attempted as usize])
            .map_err(|_| Fault::Failed)?;
        if call.words() != expected.words() || call.bytes() != expected.bytes() {
            self.failed = true;
            return Err(Fault::Failed);
        }
        if let Err(error) = self.scratch.stage(call.bytes()) {
            self.failed = true;
            return Err(error);
        }
        self.staged += 1;
        Ok(())
    }

    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault> {
        if self.failed || self.staged != self.attempted + 1 || self.last_reply.is_some() {
            self.failed = true;
            return Err(Fault::Failed);
        }
        // Record before entering the monitor so any error or non-return is
        // terminal and cannot cause an implicit replay.
        self.attempted += 1;
        let generation = self.reservation.generation();
        let identity = self.reservation.identity();
        let lifecycle = &mut self.lifecycle;
        let scm = &mut self.scm;
        let mut revalidate = || lifecycle.revalidate(generation, identity);
        match scm.invoke(call, &mut revalidate) {
            Ok(reply) => {
                self.last_reply = Some(reply);
                Ok(reply)
            }
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }

    fn complete(&mut self) -> Result<(), Fault> {
        if self.failed
            || self.attempted == 0
            || self.staged != self.attempted
            || !self
                .last_reply
                .is_some_and(|reply| reply[0] == 0 && reply[1] == 0)
        {
            self.failed = true;
            return Err(Fault::Uncertain);
        }
        if let Err(error) = self.scratch.complete() {
            self.failed = true;
            return Err(error);
        }
        self.last_reply = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{qcom_msa, qcom_msa_handoff::Handoff};
    use std::{cell::RefCell, rc::Rc, vec::Vec};

    #[derive(Clone)]
    struct Shared(Rc<RefCell<State>>);

    struct State {
        observations: usize,
        fail_observation: Option<usize>,
        stages: Vec<[u8; ARG_BYTES]>,
        completes: usize,
        replies: Vec<[u64; 8]>,
        calls: Vec<[u64; 8]>,
        fail_stage: bool,
    }

    impl Shared {
        fn new(replies: &[[u64; 8]]) -> Self {
            Self(Rc::new(RefCell::new(State {
                observations: 0,
                fail_observation: None,
                stages: Vec::new(),
                completes: 0,
                replies: replies.to_vec(),
                calls: Vec::new(),
                fail_stage: false,
            })))
        }

        fn plan() -> (Reservation, HandoffPlan) {
            let reservation = Reservation::validate_sunfish(7, 0xabc).unwrap();
            let regions = [
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
            ];
            let plan = qcom_msa::plan_handoff(reservation, &regions).unwrap();
            (reservation, plan)
        }
    }

    impl Lifecycle for Shared {
        fn revalidate(&mut self, generation: u64, identity: u64) -> Result<(), Fault> {
            let mut state = self.0.borrow_mut();
            state.observations += 1;
            if generation != 7
                || identity != 0xabc
                || state.fail_observation == Some(state.observations)
            {
                return Err(Fault::Failed);
            }
            Ok(())
        }
    }

    impl Scm for Shared {
        fn invoke(
            &mut self,
            call: &Call,
            lifecycle: &mut dyn FnMut() -> Result<(), Fault>,
        ) -> Result<[u64; 8], Fault> {
            lifecycle()?;
            let mut state = self.0.borrow_mut();
            state.calls.push(*call.words());
            if state.replies.is_empty() {
                return Err(Fault::Uncertain);
            }
            Ok(state.replies.remove(0))
        }
    }

    impl Scratch for Shared {
        fn stage(&mut self, bytes: &[u8; ARG_BYTES]) -> Result<(), Fault> {
            let mut state = self.0.borrow_mut();
            if state.fail_stage {
                return Err(Fault::Uncertain);
            }
            state.stages.push(*bytes);
            Ok(())
        }

        fn complete(&mut self) -> Result<(), Fault> {
            self.0.borrow_mut().completes += 1;
            Ok(())
        }
    }

    #[test]
    fn exact_two_transfer_sequence_stages_before_smc_and_requires_both_statuses() {
        let (reservation, plan) = Shared::plan();
        let shared = Shared::new(&[[0; 8], [0; 8]]);
        let io = Native::new(
            plan,
            reservation,
            shared.clone(),
            shared.clone(),
            shared.clone(),
        )
        .unwrap();
        let mut handoff = Handoff::new(plan, io).unwrap();
        assert!(!handoff.step().unwrap());
        assert!(handoff.step().unwrap());
        assert_eq!(handoff.status().completed, 2);
        let permit = handoff.into_msa_ready_permit().unwrap();
        assert_eq!(permit.generation(), 7);
        let state = shared.0.borrow();
        assert_eq!(state.calls.len(), 2);
        assert_eq!(state.stages.len(), 2);
        assert_eq!(state.completes, 2);
        assert_eq!(
            u64::from_le_bytes(state.stages[0][0..8].try_into().unwrap()),
            0x98d00000
        );
        assert_eq!(
            u64::from_le_bytes(state.stages[0][8..16].try_into().unwrap()),
            0x4000
        );
        assert_eq!(
            u64::from_le_bytes(state.stages[1][0..8].try_into().unwrap()),
            0x98d04000
        );
        assert_eq!(
            u64::from_le_bytes(state.stages[1][8..16].try_into().unwrap()),
            0x1fc000
        );
        assert!(
            state
                .stages
                .iter()
                .all(|bytes| bytes[280..].iter().all(|b| *b == 0))
        );
    }

    #[test]
    fn nonzero_secure_monitor_status_stops_before_completion_or_second_transfer() {
        for reply in [[1, 0, 0, 0, 0, 0, 0, 0], [0, 1, 0, 0, 0, 0, 0, 0]] {
            let (reservation, plan) = Shared::plan();
            let shared = Shared::new(&[reply, [0; 8]]);
            let io = Native::new(
                plan,
                reservation,
                shared.clone(),
                shared.clone(),
                shared.clone(),
            )
            .unwrap();
            let mut handoff = Handoff::new(plan, io).unwrap();
            assert_eq!(handoff.step(), Err(qcom_msa_handoff::Error::Monitor));
            assert_eq!(handoff.status().attempted, 1);
            assert_eq!(handoff.status().completed, 0);
            assert_eq!(
                handoff.status().ownership[0],
                qcom_msa_handoff::Ownership::Unknown
            );
            let state = shared.0.borrow();
            assert_eq!(state.calls.len(), 1);
            assert_eq!(state.completes, 0);
        }
    }

    #[test]
    fn lifecycle_change_after_scratch_stage_prevents_smc() {
        let (reservation, plan) = Shared::plan();
        let shared = Shared::new(&[[0; 8], [0; 8]]);
        shared.0.borrow_mut().fail_observation = Some(3);
        let io = Native::new(
            plan,
            reservation,
            shared.clone(),
            shared.clone(),
            shared.clone(),
        )
        .unwrap();
        let mut handoff = Handoff::new(plan, io).unwrap();
        assert_eq!(handoff.step(), Err(qcom_msa_handoff::Error::Lifecycle));
        let state = shared.0.borrow();
        assert_eq!(state.stages.len(), 1);
        assert!(state.calls.is_empty());
    }

    #[test]
    fn scratch_stage_failure_prevents_smc_and_replay() {
        let (reservation, plan) = Shared::plan();
        let shared = Shared::new(&[[0; 8], [0; 8]]);
        shared.0.borrow_mut().fail_stage = true;
        let io = Native::new(
            plan,
            reservation,
            shared.clone(),
            shared.clone(),
            shared.clone(),
        )
        .unwrap();
        let mut handoff = Handoff::new(plan, io).unwrap();
        assert_eq!(handoff.step(), Err(qcom_msa_handoff::Error::Stage));
        assert_eq!(handoff.status().attempted, 0);
        assert!(shared.0.borrow().calls.is_empty());
    }
}
