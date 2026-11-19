//! Bounded dispatcher for the Sunfish QCACLD HTT target-to-host events used
//! by the native receive and WPA key-install paths.
//!
//! The pinned QCACLD handler dispatches `RX_IND` and `SEC_IND` from the same
//! CE1 HTT-data endpoint. Security indications are reduced to peer, cipher,
//! and unicast/group fields; their Michael-key and WAPI-RSC bytes are never
//! copied into the returned event. This module has no MMIO or DMA authority.
#![forbid(unsafe_code)]

use crate::ath10k_htt::{
    HTT_10_4_T2H_SECURITY_INDICATION, HttSecurityIndication, HttSecurityIndicationError,
};
use crate::qcom_ce_v3::RxCompletion;
use crate::qcom_htc::{HtcCreditReportTotals, HtcError};
use crate::sunfish_qcacld_htc::{HttEndpoint, parse_data_frame};
use crate::sunfish_qcacld_rx_ind::{self, HTT_T2H_MSG_TYPE_RX_IND, HttRxIndication};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    RxIndication(HttRxIndication<'a>),
    SecurityIndication(HttSecurityIndication),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Htc(HtcError),
    EmptyHttMessage,
    UnsupportedMessageType(u8),
    RxIndication(sunfish_qcacld_rx_ind::Error),
    SecurityIndication(HttSecurityIndicationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedFrame<'a> {
    pub sequence: u8,
    pub credit_returns: HtcCreditReportTotals,
    pub event: Event<'a>,
}

/// Parse one HTC frame and dispatch only the source-verified Sunfish HTT
/// `RX_IND` and `SEC_IND` messages. HTC credit reports are retained for both
/// event kinds so callers can return credits after handling the event.
pub fn decode_data_frame<'a>(
    endpoint: HttEndpoint,
    bytes: &'a [u8],
) -> Result<DecodedFrame<'a>, DecodeError> {
    let frame = parse_data_frame(endpoint, bytes).map_err(DecodeError::Htc)?;
    let Some(&message_type) = frame.htt_payload.first() else {
        return Err(DecodeError::EmptyHttMessage);
    };
    let event = match message_type {
        HTT_T2H_MSG_TYPE_RX_IND => Event::RxIndication(
            HttRxIndication::decode(frame.htt_payload).map_err(DecodeError::RxIndication)?,
        ),
        HTT_10_4_T2H_SECURITY_INDICATION => Event::SecurityIndication(
            HttSecurityIndication::parse(frame.htt_payload)
                .map_err(DecodeError::SecurityIndication)?,
        ),
        observed => return Err(DecodeError::UnsupportedMessageType(observed)),
    };

    Ok(DecodedFrame {
        sequence: frame.sequence,
        credit_returns: frame.credit_returns,
        event,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsumeError<E> {
    Consume(E),
    EmptyCompletion,
    CompletionOutOfRange {
        frame_bytes: usize,
        available: usize,
    },
    Frame(DecodeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedEvent<'a> {
    pub completion: RxCompletion,
    pub decoded: DecodedFrame<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttCe1DrainStep {
    Completed,
    Empty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttCe1DrainReport {
    pub completions: usize,
    pub irq_acknowledged: bool,
    pub needs_deferred_drain: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttCe1DrainError<Service, Acknowledge> {
    ZeroBudget,
    Service(Service),
    Acknowledge(Acknowledge),
}

/// Drain CE1 events up to a fixed budget. The caller masks CE1 first and may
/// acknowledge its copy-complete status only after an empty poll. Budget
/// exhaustion or service failure leaves the status unacknowledged.
pub fn drain_htt_ce1_irq<Service, Acknowledge>(
    max_completions: usize,
    mut service_one: impl FnMut() -> Result<HttCe1DrainStep, Service>,
    acknowledge: impl FnOnce() -> Result<(), Acknowledge>,
) -> Result<HttCe1DrainReport, HttCe1DrainError<Service, Acknowledge>> {
    if max_completions == 0 {
        return Err(HttCe1DrainError::ZeroBudget);
    }
    let mut completions = 0;
    while completions < max_completions {
        match service_one().map_err(HttCe1DrainError::Service)? {
            HttCe1DrainStep::Empty => {
                acknowledge().map_err(HttCe1DrainError::Acknowledge)?;
                return Ok(HttCe1DrainReport {
                    completions,
                    irq_acknowledged: true,
                    needs_deferred_drain: false,
                });
            }
            HttCe1DrainStep::Completed => completions += 1,
        }
    }
    Ok(HttCe1DrainReport {
        completions,
        irq_acknowledged: false,
        needs_deferred_drain: true,
    })
}

/// Consume and dispatch exactly one bounded CE1 HTT-data completion.
///
/// `consume` is normally `NativeIo::consume_htt_rx_ce1` with the active
/// generation, ring plan/cursor, and RX-buffer ledger. This wrapper does not
/// retry, acknowledge interrupts, refill rings, update key state, or deliver
/// packets; it returns one validated event and its HTC credit summary.
pub fn consume_and_dispatch<'a, E>(
    endpoint: HttEndpoint,
    output: &'a mut [u8],
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, E>,
) -> Result<CompletedEvent<'a>, ConsumeError<E>> {
    let completion = consume(output).map_err(ConsumeError::Consume)?;
    let frame_bytes = usize::from(completion.nbytes);
    if frame_bytes == 0 {
        return Err(ConsumeError::EmptyCompletion);
    }
    let frame = output
        .get(..frame_bytes)
        .ok_or(ConsumeError::CompletionOutOfRange {
            frame_bytes,
            available: output.len(),
        })?;
    let decoded = decode_data_frame(endpoint, frame).map_err(ConsumeError::Frame)?;
    Ok(CompletedEvent {
        completion,
        decoded,
    })
}

#[cfg(test)]
mod drain_tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn ce1_drain_acknowledges_only_after_empty_and_reports_budget_exhaustion() {
        let services = Cell::new(0usize);
        let acknowledgements = Cell::new(0usize);
        let report = drain_htt_ce1_irq(
            3,
            || {
                let service = services.get();
                services.set(service + 1);
                Ok::<_, ()>(if service == 2 {
                    HttCe1DrainStep::Empty
                } else {
                    HttCe1DrainStep::Completed
                })
            },
            || {
                acknowledgements.set(acknowledgements.get() + 1);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        assert_eq!(
            report,
            HttCe1DrainReport {
                completions: 2,
                irq_acknowledged: true,
                needs_deferred_drain: false,
            }
        );
        assert_eq!(acknowledgements.get(), 1);

        let acknowledgements = Cell::new(0usize);
        let report = drain_htt_ce1_irq(
            2,
            || Ok::<_, ()>(HttCe1DrainStep::Completed),
            || {
                acknowledgements.set(acknowledgements.get() + 1);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        assert_eq!(
            report,
            HttCe1DrainReport {
                completions: 2,
                irq_acknowledged: false,
                needs_deferred_drain: true,
            }
        );
        assert_eq!(acknowledgements.get(), 0);
    }

    #[test]
    fn ce1_drain_rejects_zero_budget_and_keeps_service_failures_unacknowledged() {
        let acknowledgements = Cell::new(0usize);
        assert_eq!(
            drain_htt_ce1_irq(
                0,
                || Ok::<_, ()>(HttCe1DrainStep::Empty),
                || {
                    acknowledgements.set(acknowledgements.get() + 1);
                    Ok::<_, ()>(())
                },
            ),
            Err(HttCe1DrainError::ZeroBudget)
        );
        assert_eq!(
            drain_htt_ce1_irq(
                1,
                || Err::<HttCe1DrainStep, _>(7u8),
                || {
                    acknowledgements.set(acknowledgements.get() + 1);
                    Ok::<_, ()>(())
                },
            ),
            Err(HttCe1DrainError::Service(7))
        );
        assert_eq!(acknowledgements.get(), 0);
    }
}
