//! Native bounded CE1 service for the shared QCACLD RX_IND/SEC_IND endpoint.
//!
//! The owner masks CE1, consumes and synchronously dispatches bounded HTT
//! events, acknowledges only after the ring is empty, and rearms only while
//! receive descriptors remain posted. It does not acknowledge a GIC capability
//! or schedule the deferred drain itself.
#![forbid(unsafe_code)]

use core::cell::RefCell;

use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_native_io::{Gate, NativeIo, RxConsumeError, RxIrqError};
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_wmi_ce::RxBufferLedger;
use crate::sunfish_qcacld_htc::HttEndpoint;
use crate::sunfish_qcacld_htt_event::{
    CompletedEvent, ConsumeError, HttCe1DrainError, HttCe1DrainReport, HttCe1DrainStep,
    consume_and_dispatch, drain_htt_ce1_irq,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHttCe1PollError<Dispatch> {
    Event(ConsumeError<RxConsumeError>),
    Dispatch(Dispatch),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHttCe1ServiceError<Dispatch> {
    Disable(RxIrqError),
    Drain(HttCe1DrainError<NativeHttCe1PollError<Dispatch>, RxIrqError>),
    Reenable(RxIrqError),
}

/// Drain one CE1 copy-complete interrupt. `dispatch` runs synchronously while
/// the scratch-backed event is valid; it must consume the full `DecodedFrame`
/// including HTC credit reports. RX_IND packet delivery and SEC_IND key
/// tracking are separate policy callbacks. Any parse/dispatch failure keeps
/// CE1 masked and unacknowledged for fail-closed owner recovery.
#[allow(clippy::too_many_arguments)]
pub fn service_native_htt_ce1_irq<G: Gate, Dispatch>(
    io: &mut NativeIo<G>,
    generation: u64,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    rx_ledger: &mut RxBufferLedger,
    endpoint: HttEndpoint,
    scratch: &mut [u8],
    max_completions: usize,
    mut dispatch: impl FnMut(&CompletedEvent<'_>) -> Result<(), Dispatch>,
) -> Result<HttCe1DrainReport, NativeHttCe1ServiceError<Dispatch>> {
    if max_completions == 0 {
        return Err(NativeHttCe1ServiceError::Drain(
            HttCe1DrainError::ZeroBudget,
        ));
    }
    io.set_htt_rx_ce1_irq(generation, profile, rings, cursor, false)
        .map_err(NativeHttCe1ServiceError::Disable)?;

    let io = RefCell::new(io);
    let report = drain_htt_ce1_irq(
        max_completions,
        || {
            let event = {
                let mut io = io.borrow_mut();
                consume_and_dispatch(endpoint, scratch, |output| {
                    io.consume_htt_rx_ce1(generation, profile, rings, cursor, rx_ledger, output)
                })
            };
            match event {
                Ok(event) => {
                    dispatch(&event).map_err(NativeHttCe1PollError::Dispatch)?;
                    Ok(HttCe1DrainStep::Completed)
                }
                Err(ConsumeError::Consume(RxConsumeError::NotReady)) => Ok(HttCe1DrainStep::Empty),
                Err(error) => Err(NativeHttCe1PollError::Event(error)),
            }
        },
        || {
            io.borrow_mut()
                .acknowledge_htt_rx_ce1_copy_complete(generation)
        },
    )
    .map_err(NativeHttCe1ServiceError::Drain)?;

    if report.irq_acknowledged && cursor.pending() != 0 {
        io.borrow_mut()
            .set_htt_rx_ce1_irq(generation, profile, rings, cursor, true)
            .map_err(NativeHttCe1ServiceError::Reenable)?;
    }
    Ok(report)
}
