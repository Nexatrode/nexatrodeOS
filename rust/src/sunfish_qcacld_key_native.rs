//! Native CE3 publisher for the source-matched Sunfish QCACLD key command.
//!
//! The publisher requires an accepted WMI_READY session and exact HTC/CE
//! ownership. It reserves credits and key-buffer lifetime before CE3 commit,
//! erases the host staging frame on every return path, and retains/quarantines
//! uncertain publications for read-index-based DMA scrubbing.
#![forbid(unsafe_code)]

use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_native_io::{Gate, NativeIo, TxPublishError};
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_htc::{HtcCreditLedger, HtcError};
use crate::qcom_wmi_ce::{self, SensitiveTxBufferLedger};
use crate::sunfish_qcacld_key::{
    Error as KeyPlanError, QcacldCcmpKeyInstallTracker, QcacldKeyInstallCePlan, TrackerError,
};
use crate::sunfish_qcacld_wmi_rx::{
    SunfishWmiStartupSession, WmiControlEndpointError, WmiStartupError, WmiStartupPhase,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyInstallPublishError {
    PlanGeneration { planned: u64, active: u64 },
    Startup(WmiStartupError),
    StartupNotReady { phase: WmiStartupPhase },
    EndpointMismatch { planned: u8, credit_ledger: u8 },
    Tracker(TrackerError),
    Preflight(KeyPlanError),
    CreditReserve(HtcError),
    SensitiveLedgerReserve(qcom_wmi_ce::Error),
    CreditCancel(HtcError),
    Publish(TxPublishError),
    UnpublishedBufferScrub(TxPublishError),
    PublicationUncertain,
    TrackerQuarantine(TrackerError),
    PublishedTrackerCommit(TrackerError),
}

/// Publish one pending CCMP key only after this WMI generation reached READY,
/// the control endpoint and credit plan match, and the association tracker
/// accepts the command identity. The host staging frame is erased on every
/// return path. A DMA copy remains ledgered until CE3's read index permits
/// `NativeIo::reclaim_sensitive_wmi_tx` to scrub it.
#[allow(clippy::too_many_arguments)]
pub fn publish_qcacld_key_install<G: Gate, const N: usize>(
    io: &mut NativeIo<G>,
    mut plan: QcacldKeyInstallCePlan<'_>,
    active_generation: u64,
    association_generation: u32,
    startup: &SunfishWmiStartupSession,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    credits: &mut HtcCreditLedger,
    sensitive: &mut SensitiveTxBufferLedger<N>,
    tracker: &mut QcacldCcmpKeyInstallTracker,
) -> Result<(), KeyInstallPublishError> {
    let result = publish_qcacld_key_install_inner(
        io,
        &plan,
        active_generation,
        association_generation,
        startup,
        profile,
        rings,
        cursor,
        credits,
        sensitive,
        tracker,
    );
    plan.wipe_frame();
    result
}

#[allow(clippy::too_many_arguments)]
fn publish_qcacld_key_install_inner<G: Gate, const N: usize>(
    io: &mut NativeIo<G>,
    plan: &QcacldKeyInstallCePlan<'_>,
    active_generation: u64,
    association_generation: u32,
    startup: &SunfishWmiStartupSession,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    credits: &mut HtcCreditLedger,
    sensitive: &mut SensitiveTxBufferLedger<N>,
    tracker: &mut QcacldCcmpKeyInstallTracker,
) -> Result<(), KeyInstallPublishError> {
    if plan.generation() != active_generation {
        return Err(KeyInstallPublishError::PlanGeneration {
            planned: plan.generation(),
            active: active_generation,
        });
    }
    if startup.generation() != active_generation {
        return Err(KeyInstallPublishError::Startup(
            WmiStartupError::WrongGeneration {
                expected: startup.generation(),
                observed: active_generation,
            },
        ));
    }
    if startup.phase() != WmiStartupPhase::Ready {
        return Err(KeyInstallPublishError::StartupNotReady {
            phase: startup.phase(),
        });
    }
    startup
        .require_control_endpoint(plan.endpoint_id())
        .map_err(|error| {
            KeyInstallPublishError::Startup(match error {
                WmiControlEndpointError::SessionNotBound => WmiStartupError::ControlSessionNotBound,
                WmiControlEndpointError::Mismatch { expected, observed } => {
                    WmiStartupError::ControlEndpointMismatch { expected, observed }
                }
            })
        })?;
    if credits.endpoint_id() != plan.endpoint_id() {
        return Err(KeyInstallPublishError::EndpointMismatch {
            planned: plan.endpoint_id(),
            credit_ledger: credits.endpoint_id(),
        });
    }
    tracker
        .validate_command(association_generation, plan.command())
        .map_err(KeyInstallPublishError::Tracker)?;

    let ce = plan.ce_plan().map_err(KeyInstallPublishError::Preflight)?;
    let lease = plan
        .sensitive_buffer_lease_from_ce(ce)
        .map_err(KeyInstallPublishError::Preflight)?;
    let credit_plan = plan.htc_plan();
    credits
        .reserve(active_generation, credit_plan)
        .map_err(KeyInstallPublishError::CreditReserve)?;
    if let Err(error) = sensitive.reserve(lease, *cursor, plan.dma_pool()) {
        credits
            .cancel_unpublished(active_generation, credit_plan.credits_required)
            .map_err(KeyInstallPublishError::CreditCancel)?;
        return Err(KeyInstallPublishError::SensitiveLedgerReserve(error));
    }

    match io.publish_wmi_tx(active_generation, profile, rings, ce, cursor) {
        Ok(()) => tracker
            .record_queued(association_generation, plan.command())
            .map_err(|error| {
                match tracker.quarantine_uncertain_publication(association_generation) {
                    Ok(()) => KeyInstallPublishError::PublishedTrackerCommit(error),
                    Err(quarantine_error) => {
                        KeyInstallPublishError::TrackerQuarantine(quarantine_error)
                    }
                }
            }),
        Err(TxPublishError::Uncertain) => {
            tracker
                .quarantine_uncertain_publication(association_generation)
                .map_err(KeyInstallPublishError::TrackerQuarantine)?;
            Err(KeyInstallPublishError::PublicationUncertain)
        }
        Err(publish_error) => {
            if let Err(scrub_error) = io.scrub_unpublished_sensitive_wmi_tx(
                active_generation,
                profile,
                rings,
                cursor,
                sensitive,
                lease,
            ) {
                tracker
                    .quarantine_uncertain_publication(association_generation)
                    .map_err(KeyInstallPublishError::TrackerQuarantine)?;
                return Err(KeyInstallPublishError::UnpublishedBufferScrub(scrub_error));
            }
            credits
                .cancel_unpublished(active_generation, credit_plan.credits_required)
                .map_err(KeyInstallPublishError::CreditCancel)?;
            Err(KeyInstallPublishError::Publish(publish_error))
        }
    }
}
