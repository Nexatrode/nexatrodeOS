//! Compose a completed CE1 copy with the negotiated QCACLD HTC/HTT decoder.
//!
//! The caller supplies the transport operation so this module stays testable
//! without MMIO or DMA authority. Only the byte count returned by the CE
//! completion is exposed to the decoder; unused scratch-buffer capacity is
//! never parsed as part of the frame.
#![forbid(unsafe_code)]

use crate::qcom_ce_v3::RxCompletion;
use crate::qcom_htc::HtcCreditReportTotals;
use crate::sunfish_qcacld_htc::HttEndpoint;
use crate::sunfish_qcacld_rx_ind::{self, FrameError, HttRxIndication};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    Consume(E),
    EmptyCompletion,
    CompletionOutOfRange {
        frame_bytes: usize,
        available: usize,
    },
    Frame(FrameError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedDataFrame<'a> {
    pub completion: RxCompletion,
    pub sequence: u8,
    pub credit_returns: HtcCreditReportTotals,
    pub indication: HttRxIndication<'a>,
}

/// Consume and parse exactly one completed CE1 HTT-data frame.
///
/// `consume` is normally `NativeIo::consume_htt_rx_ce1` with the current
/// runtime generation, CE1 ring plan/cursor, and RX buffer ledger. The
/// closure must return only after copying a validated completion into
/// `output`. This function performs no retry, interrupt acknowledgement,
/// additional ring consumption, or low-latency MSDU-ring ownership change.
pub fn consume_and_decode<'a, E>(
    endpoint: HttEndpoint,
    output: &'a mut [u8],
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, E>,
) -> Result<CompletedDataFrame<'a>, Error<E>> {
    let completion = consume(output).map_err(Error::Consume)?;
    let frame_bytes = usize::from(completion.nbytes);
    if frame_bytes == 0 {
        return Err(Error::EmptyCompletion);
    }
    let frame = output
        .get(..frame_bytes)
        .ok_or(Error::CompletionOutOfRange {
            frame_bytes,
            available: output.len(),
        })?;
    let (sequence, credit_returns, indication) =
        sunfish_qcacld_rx_ind::decode_htc_data_frame(endpoint, frame).map_err(Error::Frame)?;
    Ok(CompletedDataFrame {
        completion,
        sequence,
        credit_returns,
        indication,
    })
}
