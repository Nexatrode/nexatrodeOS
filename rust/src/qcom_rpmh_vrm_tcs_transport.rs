//! Exact RPMh phase bridge to bounded TCS0 execution.
//!
//! This composes the broker's prepared batch with either an owner-cache stage
//! for deferred sleep/wake phases or the active-TCS transaction engine. The
//! cache remains an injected platform obligation; this module does not
//! implement global `rpmh_flush`, shared-resource discovery, or MMIO.

use crate::qcom_rpmh_handoff::Snapshot;
use crate::qcom_rpmh_vote_broker::{AggregatePhase, ExactPhaseTransport};
use crate::qcom_rpmh_vrm_command::Batch;
use crate::qcom_rpmh_vrm_tcs::{self, Plan};
use crate::qcom_rpmh_vrm_tcs_io::{self, Completion, GuardedRegisters};

/// The backend must capture two stable snapshots under the supplied live
/// lease and validate the same identity/generation before every register
/// access. A native implementation must additionally enforce exact uncached
/// mappings and register access classes.
pub trait Backend<L>: GuardedRegisters<L> {
    fn capture_stable(
        &mut self,
        lease: &L,
        identity: u64,
        generation: u64,
    ) -> Result<Snapshot, Self::Error>;
}

/// Owner-managed sleep/wake cache. Implementations must validate the lease,
/// bound capacity, preserve exact resource identity, and never claim TCS
/// completion. A system-wide flush is a separate operation.
pub trait DeferredCache<L> {
    type Error;

    fn stage(&mut self, lease: &L, batch: Batch) -> Result<(), Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidBudget,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error<CacheError, BackendError> {
    Cache(CacheError),
    Snapshot(BackendError),
    Plan(qcom_rpmh_vrm_tcs::Error),
    Execute(qcom_rpmh_vrm_tcs_io::Error<BackendError>),
}

/// Production authority is deliberately not manufactured by this value; its
/// cache and backend must already be tied to the platform's live owner lease.
pub struct RpmhTcsTransport<C, B> {
    cache: C,
    backend: B,
    readback_attempts: u8,
    irq_attempts: u8,
}

impl<C, B> RpmhTcsTransport<C, B> {
    pub fn new(
        cache: C,
        backend: B,
        readback_attempts: u8,
        irq_attempts: u8,
    ) -> Result<Self, ConfigError> {
        if readback_attempts == 0
            || readback_attempts > qcom_rpmh_vrm_tcs_io::MAX_ATTEMPTS
            || irq_attempts == 0
            || irq_attempts > qcom_rpmh_vrm_tcs_io::MAX_ATTEMPTS
        {
            return Err(ConfigError::InvalidBudget);
        }
        Ok(Self {
            cache,
            backend,
            readback_attempts,
            irq_attempts,
        })
    }

    pub fn into_parts(self) -> (C, B) {
        (self.cache, self.backend)
    }
}

impl<L, C, B> ExactPhaseTransport<L> for RpmhTcsTransport<C, B>
where
    C: DeferredCache<L>,
    B: Backend<L>,
{
    type Error = Error<C::Error, B::Error>;

    fn accept_exact_phase(&mut self, lease: &L, batch: Batch) -> Result<(), Self::Error> {
        match batch.phase {
            AggregatePhase::Sleep | AggregatePhase::Wake => {
                self.cache.stage(lease, batch).map_err(Error::Cache)
            }
            AggregatePhase::Active => {
                if !self
                    .backend
                    .validate(lease, batch.identity, batch.generation)
                {
                    return Err(Error::Execute(qcom_rpmh_vrm_tcs_io::Error::LeaseLost));
                }
                let snapshot = self
                    .backend
                    .capture_stable(lease, batch.identity, batch.generation)
                    .map_err(Error::Snapshot)?;
                let plan: Plan = tcs_vrm_plan(snapshot, batch).map_err(Error::Plan)?;
                qcom_rpmh_vrm_tcs_io::execute(
                    plan,
                    snapshot,
                    lease,
                    &mut self.backend,
                    self.readback_attempts,
                    self.irq_attempts,
                )
                .map(|_receipt: Completion| ())
                .map_err(Error::Execute)
            }
        }
    }
}

fn tcs_vrm_plan(snapshot: Snapshot, batch: Batch) -> Result<Plan, qcom_rpmh_vrm_tcs::Error> {
    qcom_rpmh_vrm_tcs::build_plan(snapshot, batch)
}
