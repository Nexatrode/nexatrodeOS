//! Compose one CE1 HTT-data completion through RX_IND policy and Ethernet
//! delivery.
//!
//! The caller supplies the CE1 operation and all buffer/lease storage. The
//! receive closure is expected to be the generation-checked native CE1 copy;
//! this module itself has no MMIO, DMA, cache, or interrupt authority.
#![forbid(unsafe_code)]

use crate::qcom_ce_v3::RxCompletion;
use crate::qcom_htc::HtcCreditReportTotals;
use crate::sunfish_qcacld_htc::HttEndpoint;
use crate::sunfish_qcacld_rx::ParseOptions;
use crate::sunfish_qcacld_rx_delivery::{
    EthernetRxSink, RxDeliveryError, RxDeliverySyncError, RxIndicationDeliveryReport,
    RxIndicationRecycleReport, deliver_indication, deliver_indication_with_cpu_sync,
    deliver_indication_with_cpu_sync_and_collect,
};
use crate::sunfish_qcacld_rx_session::{
    Error as SessionError, RxBufferLease, RxMsduChain, SunfishQcacldRxSession,
};
use crate::sunfish_qcacld_rx_transport::{Error as TransportError, consume_and_decode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxPipelineReport {
    pub completion: RxCompletion,
    pub sequence: u8,
    pub credit_returns: HtcCreditReportTotals,
    pub delivery: RxIndicationDeliveryReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxPipelineRecycleReport {
    pub pipeline: RxPipelineReport,
    pub recycled_buffer_count: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RxPipelineError<C, S> {
    Transport(TransportError<C>),
    Delivery(RxDeliveryError<S>),
}

#[derive(Debug, Eq, PartialEq)]
pub enum RxPipelineSyncError<C, S, D> {
    Transport(TransportError<C>),
    Delivery(RxDeliverySyncError<S, D>),
}

/// Consume one validated CE1 HTT-data completion and deliver all supported
/// MSDUs described by its bounded RX indication.
#[allow(clippy::too_many_arguments)]
pub fn consume_and_deliver<const N: usize, S: EthernetRxSink, C>(
    endpoint: HttEndpoint,
    transport_scratch: &mut [u8],
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, C>,
) -> Result<RxPipelineReport, RxPipelineError<C, S::Error>> {
    let completed = consume_and_decode(endpoint, transport_scratch, consume)
        .map_err(RxPipelineError::Transport)?;
    let delivery = deliver_indication(
        session,
        buffers_by_id,
        options,
        completed.indication,
        msdus_output,
        leases_output,
        frame_scratch,
        sink,
    )
    .map_err(RxPipelineError::Delivery)?;

    Ok(RxPipelineReport {
        completion: completed.completion,
        sequence: completed.sequence,
        credit_returns: completed.credit_returns,
        delivery,
    })
}

/// CE1 completion pipeline variant that performs per-buffer CPU DMA
/// synchronization before HTT RX buffers are inspected. The callback is a
/// platform boundary; this module still has no MMIO, DMA, cache, or IRQ
/// authority of its own.
#[allow(clippy::too_many_arguments)]
pub fn consume_and_deliver_with_cpu_sync<const N: usize, S: EthernetRxSink, C, D>(
    endpoint: HttEndpoint,
    transport_scratch: &mut [u8],
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
    sync_for_cpu: impl FnMut(u32) -> Result<(), D>,
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, C>,
) -> Result<RxPipelineReport, RxPipelineSyncError<C, S::Error, D>> {
    let completed = consume_and_decode(endpoint, transport_scratch, consume)
        .map_err(RxPipelineSyncError::Transport)?;
    let delivery = deliver_indication_with_cpu_sync(
        session,
        buffers_by_id,
        options,
        completed.indication,
        msdus_output,
        leases_output,
        frame_scratch,
        sink,
        sync_for_cpu,
    )
    .map_err(RxPipelineSyncError::Delivery)?;

    Ok(RxPipelineReport {
        completion: completed.completion,
        sequence: completed.sequence,
        credit_returns: completed.credit_returns,
        delivery,
    })
}

/// CE1 pipeline variant that also returns the FIFO IDs released by packet
/// delivery. After this function returns, the caller's immutable buffer view
/// has ended and it can pass those IDs to `SunfishHttRxRingRefillState` for one
/// fill-N device publication.
#[allow(clippy::too_many_arguments)]
pub fn consume_and_deliver_with_cpu_sync_and_collect<const N: usize, S: EthernetRxSink, C, D>(
    endpoint: HttEndpoint,
    transport_scratch: &mut [u8],
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
    sync_for_cpu: impl FnMut(u32) -> Result<(), D>,
    recycled_buffer_ids: &mut [u32],
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, C>,
) -> Result<RxPipelineRecycleReport, RxPipelineRecycleError<C, S::Error, D>> {
    let required_slots = session.posted_count();
    for available in [
        msdus_output.len(),
        leases_output.len(),
        recycled_buffer_ids.len(),
    ] {
        if available < required_slots {
            return Err(RxPipelineRecycleError::Delivery(
                RxDeliverySyncError::Delivery(RxDeliveryError::Claim(
                    SessionError::OutputTooSmall {
                        needed: required_slots,
                        available,
                    },
                )),
            ));
        }
    }

    let completed = consume_and_decode(endpoint, transport_scratch, consume)
        .map_err(RxPipelineRecycleError::Transport)?;
    let collected = deliver_indication_with_cpu_sync_and_collect(
        session,
        buffers_by_id,
        options,
        completed.indication,
        msdus_output,
        leases_output,
        frame_scratch,
        sink,
        sync_for_cpu,
        recycled_buffer_ids,
    )
    .map_err(RxPipelineRecycleError::Delivery)?;
    let RxIndicationRecycleReport {
        delivery,
        recycled_buffer_count,
    } = collected;

    Ok(RxPipelineRecycleReport {
        pipeline: RxPipelineReport {
            completion: completed.completion,
            sequence: completed.sequence,
            credit_returns: completed.credit_returns,
            delivery,
        },
        recycled_buffer_count,
    })
}

#[derive(Debug, Eq, PartialEq)]
pub enum RxPipelineRecycleError<C, S, D> {
    Transport(TransportError<C>),
    Delivery(RxDeliverySyncError<S, D>),
}
