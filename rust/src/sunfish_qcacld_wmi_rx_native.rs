//! Native CE2 IRQ servicing for the bounded QCACLD WMI receive pipeline.
//!
//! This composes safe WMI event handling with the capability-gated CE adapter.
//! It does not acknowledge the seL4 GIC capability or schedule deferred work.
#![forbid(unsafe_code)]

use core::cell::RefCell;
use core::fmt;

use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_native_io::{Gate, NativeIo, RxConsumeError, RxIrqError};
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_htc::HtcCreditLedger;
use crate::qcom_wmi_ce::RxBufferLedger;
use crate::sunfish_qcacld_mgmt_tx::MgmtTxOutstanding;
use crate::sunfish_qcacld_wmi_rx::{
    PipelineError, RetirementPipelineError, StartupObservedWmiEvent, StartupPipelineError,
    SunfishWmiStartupSession, WmiCe2DrainError, WmiCe2DrainReport, WmiCe2DrainStep,
    consume_dispatch_and_observe_startup, drain_wmi_ce2_irq,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWmiCe2PollError<Dispatch> {
    Pipeline(StartupPipelineError<RxConsumeError>),
    Dispatch(Dispatch),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWmiCe2ServiceError<Dispatch> {
    Disable(RxIrqError),
    Drain(WmiCe2DrainError<NativeWmiCe2PollError<Dispatch>, RxIrqError>),
    Reenable(RxIrqError),
}

/// Hardware and protocol state available to a startup-event action after the
/// CE2 completion has been consumed and the startup session has accepted it.
/// Actions run before CE2 status is acknowledged, so an action failure keeps
/// the interrupt masked and lets the owner fail closed.
pub struct NativeWmiCe2EventContext<'a, G: Gate> {
    pub io: &'a mut NativeIo<G>,
    pub generation: u64,
    pub profile: Profile,
    pub rings: &'a RingPlan,
    pub cursor: &'a mut RingCursor,
    pub credits: &'a mut HtcCreditLedger,
    pub startup: &'a mut SunfishWmiStartupSession,
}

impl<Dispatch> fmt::Display for NativeWmiCe2ServiceError<Dispatch> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disable(_) => f.write_str("failed to mask CE2 WMI interrupt"),
            Self::Drain(_) => f.write_str("failed to drain or acknowledge CE2 WMI completions"),
            Self::Reenable(_) => f.write_str("failed to re-enable CE2 WMI interrupt"),
        }
    }
}

/// Service one CE2 WMI IRQ using the native CE adapter and validated HTC/WMI
/// startup pipeline. CE2 is masked first, completions are drained and
/// dispatched up to `max_completions`, and device status is acknowledged only
/// after an empty poll. CE2 is re-enabled only while receive descriptors remain
/// posted. Budget exhaustion or service/acknowledgement failure leaves CE2
/// masked so the runtime can recover without losing the pending condition.
#[allow(clippy::too_many_arguments)]
pub fn service_native_wmi_ce2_irq<G: Gate, const N: usize, Dispatch>(
    io: &mut NativeIo<G>,
    generation: u64,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    rx_ledger: &mut RxBufferLedger,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credit_ledger: &mut HtcCreditLedger,
    outstanding: &mut MgmtTxOutstanding<N>,
    startup: &mut SunfishWmiStartupSession,
    scratch: &mut [u8],
    max_completions: usize,
    mut dispatch: impl FnMut(&StartupObservedWmiEvent<'_>) -> Result<(), Dispatch>,
) -> Result<WmiCe2DrainReport, NativeWmiCe2ServiceError<Dispatch>> {
    service_native_wmi_ce2_irq_with_action(
        io,
        generation,
        profile,
        rings,
        cursor,
        rx_ledger,
        expected_endpoint_id,
        max_message_size,
        credit_ledger,
        outstanding,
        startup,
        scratch,
        max_completions,
        |event, _context| dispatch(event),
    )
}

/// Service CE2 like [`service_native_wmi_ce2_irq`], with an additional
/// post-observation action hook. The hook runs after event parsing, HTC credit
/// accounting, management-TX retirement, and startup-session transition, but
/// before CE2's empty-ring check and status acknowledgement. This is the
/// boundary for publishing WMI_INIT in response to SERVICE_AVAILABLE.
#[allow(clippy::too_many_arguments)]
pub fn service_native_wmi_ce2_irq_with_action<G: Gate, const N: usize, Dispatch>(
    io: &mut NativeIo<G>,
    generation: u64,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    rx_ledger: &mut RxBufferLedger,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credit_ledger: &mut HtcCreditLedger,
    outstanding: &mut MgmtTxOutstanding<N>,
    startup: &mut SunfishWmiStartupSession,
    scratch: &mut [u8],
    max_completions: usize,
    mut dispatch: impl FnMut(
        &StartupObservedWmiEvent<'_>,
        NativeWmiCe2EventContext<'_, G>,
    ) -> Result<(), Dispatch>,
) -> Result<WmiCe2DrainReport, NativeWmiCe2ServiceError<Dispatch>> {
    if max_completions == 0 {
        return Err(NativeWmiCe2ServiceError::Drain(
            WmiCe2DrainError::ZeroBudget,
        ));
    }
    io.set_wmi_rx_irq(generation, profile, rings, cursor, false)
        .map_err(NativeWmiCe2ServiceError::Disable)?;

    let io = RefCell::new(io);
    let report = drain_wmi_ce2_irq(
        max_completions,
        || {
            let event = {
                let mut io = io.borrow_mut();
                consume_dispatch_and_observe_startup(
                    generation,
                    expected_endpoint_id,
                    max_message_size,
                    credit_ledger,
                    outstanding,
                    startup,
                    scratch,
                    |output| {
                        io.consume_wmi_rx(generation, profile, rings, cursor, rx_ledger, output)
                    },
                )
            };
            match event {
                Ok(event) => {
                    let mut io_ref = io.borrow_mut();
                    dispatch(
                        &event,
                        NativeWmiCe2EventContext {
                            io: &mut **io_ref,
                            generation,
                            profile,
                            rings,
                            cursor,
                            credits: credit_ledger,
                            startup,
                        },
                    )
                    .map_err(NativeWmiCe2PollError::Dispatch)?;
                    Ok(WmiCe2DrainStep::Completed)
                }
                Err(StartupPipelineError::Retirement(RetirementPipelineError::Dispatch(
                    PipelineError::Consume(RxConsumeError::NotReady),
                ))) => Ok(WmiCe2DrainStep::Empty),
                Err(error) => Err(NativeWmiCe2PollError::Pipeline(error)),
            }
        },
        || {
            io.borrow_mut()
                .acknowledge_wmi_rx_ce2_copy_complete(generation)
        },
    )
    .map_err(NativeWmiCe2ServiceError::Drain)?;

    if report.irq_acknowledged && cursor.pending() != 0 {
        io.borrow_mut()
            .set_wmi_rx_irq(generation, profile, rings, cursor, true)
            .map_err(NativeWmiCe2ServiceError::Reenable)?;
    }
    Ok(report)
}
