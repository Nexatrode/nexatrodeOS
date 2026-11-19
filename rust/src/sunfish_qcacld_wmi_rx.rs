//! Bounded QCACLD WMI receive dispatch on the negotiated HTC WMI endpoint.
//!
//! The caller supplies bytes copied from the CE2 receive ring. This module
//! validates the HTC/WMI envelope, applies its independent HTC credit reports
//! to the generation-bound WMI ledger, and decodes the first supported event
//! types. It has no DMA, MMIO, interrupt, or device authority.
#![forbid(unsafe_code)]

use crate::qcacld_wmi::{
    ServiceAvailableError, ServiceAvailableSnapshot, ServiceAvailableView, ServiceReadyError,
    ServiceReadySnapshot, ServiceReadyView, SunfishWmiStartupError, WmiReadyError, WmiReadyEvent,
};
use crate::qcom_ce_v3::RxCompletion;
use crate::qcom_htc::{self, HtcCreditLedger, HtcCreditReportTotals, HtcError};
use crate::sunfish_qcacld_htc::{
    HttEndpoint,
    wmi_control::{SunfishWmiControlSession, WmiControlEndpoint, WmiControlPhase},
};
use crate::sunfish_qcacld_mgmt_tx::{
    self, CompletedMgmtTx, Error as MgmtTxError, MgmtTxCompletion, MgmtTxOutstanding,
};

/// IDs from the pinned QCACLD `wmi_event_id` ABI.
pub const WMI_SERVICE_READY_EVENT_ID: u32 = 0x0001;
pub const WMI_READY_EVENT_ID: u32 = 0x0002;
pub const WMI_SERVICE_AVAILABLE_EVENT_ID: u32 = 0x0003;
pub const WMI_SCAN_EVENT_ID: u32 = 0x3001;
pub const WMI_MGMT_TX_COMPLETION_EVENT_ID: u32 = 0x7006;

/// The fixed scan-event TLV is tag 36 with seven little-endian words in its
/// value (`event` through `tsf_timestamp`).
pub const WMI_SCAN_EVENT_FIXED_PARAM_TLV_TAG: u16 = 36;
pub const WMI_SCAN_EVENT_FIXED_PARAM_VALUE_BYTES: usize = 28;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QcacldScanEvent {
    pub event: u32,
    pub reason: u32,
    pub channel_frequency_mhz: u32,
    pub requestor_id: u32,
    pub scan_id: u32,
    pub vdev_id: u32,
    pub tsf_timestamp: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiEvent<'a> {
    ServiceReady(ServiceReadyView<'a>),
    ServiceAvailable(ServiceAvailableView<'a>),
    Ready(WmiReadyEvent<'a>),
    Scan(QcacldScanEvent),
    MgmtTxCompletion(MgmtTxCompletion),
    /// Keep bounded, syntactically valid events visible so a future handler
    /// can be added without losing their HTC credit reports.
    Unknown {
        event_id: u32,
        body: &'a [u8],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceivedWmiEvent<'a> {
    pub generation: u64,
    pub endpoint_id: u8,
    pub event_id: u32,
    /// The validated event's TLV body, retained for stateful handlers that
    /// must validate it again when advancing a protocol session.
    pub tlv_body: &'a [u8],
    pub credit_returns: HtcCreditReportTotals,
    /// Credits returned to this WMI endpoint; reports for other endpoints
    /// remain available in `credit_returns` for their own ledgers.
    pub wmi_credits_returned: u16,
    pub event: WmiEvent<'a>,
}

/// Ordered stages of QCACLD's initial WMI control-plane handshake.
///
/// `Ready` means only that a successful WMI_READY event followed a submitted
/// WMI_INIT for this generation. It does not imply firmware download, WLAN
/// subsystem start, or radio operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiStartupPhase {
    AwaitServiceReady,
    AwaitServiceAvailable,
    InitReady,
    InitSubmissionUncertain,
    AwaitReady,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiStartupTransition {
    ServiceReadyAccepted,
    InitReady,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiStartupError {
    WrongGeneration {
        expected: u64,
        observed: u64,
    },
    GenerationNotAdvanced {
        current: u64,
        requested: u64,
    },
    UnexpectedEvent {
        phase: WmiStartupPhase,
        event_id: u32,
    },
    InitSubmissionNotAllowed {
        phase: WmiStartupPhase,
    },
    ServiceReady(ServiceReadyError),
    StartupPolicy(SunfishWmiStartupError),
    ServiceAvailable(ServiceAvailableError),
    Ready(WmiReadyError),
    ControlNotStarted {
        phase: WmiControlPhase,
    },
    ControlEndpointUnavailable,
    ControlSessionNotBound,
    ControlEndpointMismatch {
        expected: u8,
        observed: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiControlEndpointError {
    SessionNotBound,
    Mismatch { expected: u8, observed: u8 },
}

/// Generation-bound state for the initial QCACLD WMI handshake. It gates
/// event order and successful command submission; it does not build or send
/// WMI_INIT and grants no DMA or device authority.
#[derive(Debug, Eq, PartialEq)]
pub struct SunfishWmiStartupSession {
    generation: u64,
    phase: WmiStartupPhase,
    htt_endpoint: Option<HttEndpoint>,
    wmi_control_endpoint: Option<WmiControlEndpoint>,
    service_ready: Option<ServiceReadySnapshot>,
    service_available: Option<ServiceAvailableSnapshot>,
}

impl SunfishWmiStartupSession {
    pub const fn new(generation: u64) -> Self {
        Self {
            generation,
            phase: WmiStartupPhase::AwaitServiceReady,
            htt_endpoint: None,
            wmi_control_endpoint: None,
            service_ready: None,
            service_available: None,
        }
    }

    /// Create the WMI event session only after the source-ordered HTC
    /// WMI_CONTROL_SVC connection and SETUP_COMPLETE submission have reached
    /// `Started`. Both negotiated endpoints are then locked to this transport
    /// generation and cleared when that generation advances.
    pub fn after_wmi_control_started(
        control: &SunfishWmiControlSession,
    ) -> Result<Self, WmiStartupError> {
        if control.phase() != WmiControlPhase::Started {
            return Err(WmiStartupError::ControlNotStarted {
                phase: control.phase(),
            });
        }
        let endpoint = control
            .endpoint()
            .ok_or(WmiStartupError::ControlEndpointUnavailable)?;
        Ok(Self {
            generation: control.generation(),
            phase: WmiStartupPhase::AwaitServiceReady,
            htt_endpoint: Some(control.htt_endpoint()),
            wmi_control_endpoint: Some(endpoint),
            service_ready: None,
            service_available: None,
        })
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn phase(&self) -> WmiStartupPhase {
        self.phase
    }

    pub const fn control_endpoint_id(&self) -> Option<u8> {
        match self.wmi_control_endpoint {
            Some(endpoint) => Some(endpoint.endpoint_id()),
            None => None,
        }
    }

    pub const fn htt_endpoint(&self) -> Option<HttEndpoint> {
        self.htt_endpoint
    }

    pub const fn wmi_control_endpoint(&self) -> Option<WmiControlEndpoint> {
        self.wmi_control_endpoint
    }

    /// Check the endpoint selected by the CE2 receive path before it consumes
    /// a completion. Unbound sessions remain useful for offline event-parser
    /// tests; a bound session rejects a caller configured for another route.
    pub fn validate_observed_endpoint(&self, endpoint_id: u8) -> Result<(), WmiStartupError> {
        if let Some(expected) = self.control_endpoint_id() {
            if endpoint_id != expected {
                return Err(WmiStartupError::ControlEndpointMismatch {
                    expected,
                    observed: endpoint_id,
                });
            }
        }
        Ok(())
    }

    /// Require proof of the completed control-service handshake and the
    /// negotiated endpoint before planning or publishing WMI_INIT.
    pub fn require_control_endpoint(&self, endpoint_id: u8) -> Result<(), WmiControlEndpointError> {
        let expected = self
            .control_endpoint_id()
            .ok_or(WmiControlEndpointError::SessionNotBound)?;
        if endpoint_id != expected {
            return Err(WmiControlEndpointError::Mismatch {
                expected,
                observed: endpoint_id,
            });
        }
        Ok(())
    }

    /// Owned SERVICE_READY evidence remains available after the CE2 scratch
    /// buffer has been reused by later completions.
    pub const fn service_ready_snapshot(&self) -> Option<ServiceReadySnapshot> {
        self.service_ready
    }

    /// Owned SERVICE_AVAILABLE evidence remains available for WMI_INIT
    /// resource-policy evaluation after event dispatch returns.
    pub const fn service_available_snapshot(&self) -> Option<ServiceAvailableSnapshot> {
        self.service_available
    }

    /// Start a new transport/radio generation. Old or repeated generation
    /// numbers are rejected so delayed events cannot reset the session.
    pub fn advance_generation(&mut self, generation: u64) -> Result<(), WmiStartupError> {
        if generation <= self.generation {
            return Err(WmiStartupError::GenerationNotAdvanced {
                current: self.generation,
                requested: generation,
            });
        }
        self.generation = generation;
        self.phase = WmiStartupPhase::AwaitServiceReady;
        self.htt_endpoint = None;
        self.wmi_control_endpoint = None;
        self.service_ready = None;
        self.service_available = None;
        Ok(())
    }

    /// Validate and consume the next event in the source-matched startup
    /// order. The event body is reparsed so fabricated or mismatched enum
    /// variants cannot advance the session.
    pub fn observe(
        &mut self,
        received: &ReceivedWmiEvent<'_>,
    ) -> Result<WmiStartupTransition, WmiStartupError> {
        if received.generation != self.generation {
            return Err(WmiStartupError::WrongGeneration {
                expected: self.generation,
                observed: received.generation,
            });
        }
        self.validate_observed_endpoint(received.endpoint_id)?;

        let mut accepted_service_ready = None;
        let mut accepted_service_available = None;
        let next = match (self.phase, received.event_id) {
            (WmiStartupPhase::AwaitServiceReady, WMI_SERVICE_READY_EVENT_ID) => {
                let service_ready = ServiceReadyView::parse(received.tlv_body)
                    .map_err(WmiStartupError::ServiceReady)?;
                accepted_service_ready = Some(
                    service_ready
                        .sunfish_startup_snapshot()
                        .map_err(WmiStartupError::StartupPolicy)?,
                );
                WmiStartupPhase::AwaitServiceAvailable
            }
            (WmiStartupPhase::AwaitServiceAvailable, WMI_SERVICE_AVAILABLE_EVENT_ID) => {
                accepted_service_available = Some(
                    ServiceAvailableView::parse(received.tlv_body)
                        .map_err(WmiStartupError::ServiceAvailable)?
                        .snapshot(),
                );
                WmiStartupPhase::InitReady
            }
            (WmiStartupPhase::AwaitReady, WMI_READY_EVENT_ID) => {
                WmiReadyEvent::parse(received.tlv_body).map_err(WmiStartupError::Ready)?;
                WmiStartupPhase::Ready
            }
            (phase, event_id) => {
                return Err(WmiStartupError::UnexpectedEvent { phase, event_id });
            }
        };

        self.phase = next;
        if let Some(snapshot) = accepted_service_ready {
            self.service_ready = Some(snapshot);
        }
        if let Some(snapshot) = accepted_service_available {
            self.service_available = Some(snapshot);
        }
        Ok(match next {
            WmiStartupPhase::AwaitServiceAvailable => WmiStartupTransition::ServiceReadyAccepted,
            WmiStartupPhase::InitReady => WmiStartupTransition::InitReady,
            WmiStartupPhase::Ready => WmiStartupTransition::Ready,
            WmiStartupPhase::AwaitServiceReady
            | WmiStartupPhase::InitSubmissionUncertain
            | WmiStartupPhase::AwaitReady => {
                unreachable!("observe only advances through handshake events")
            }
        })
    }

    /// Mark WMI_INIT submitted only after the transport reports successful
    /// publication. READY events observed before this call are rejected.
    pub fn mark_init_submitted(&mut self, generation: u64) -> Result<(), WmiStartupError> {
        if generation != self.generation {
            return Err(WmiStartupError::WrongGeneration {
                expected: self.generation,
                observed: generation,
            });
        }
        if self.phase != WmiStartupPhase::InitReady {
            return Err(WmiStartupError::InitSubmissionNotAllowed { phase: self.phase });
        }
        self.phase = WmiStartupPhase::AwaitReady;
        Ok(())
    }

    /// Quarantine this startup generation when CE3 publication may have taken
    /// effect but its completion could not be confirmed. The frame must not be
    /// replayed; only an explicit generation advance can leave this phase.
    pub fn mark_init_submission_uncertain(
        &mut self,
        generation: u64,
    ) -> Result<(), WmiStartupError> {
        if generation != self.generation {
            return Err(WmiStartupError::WrongGeneration {
                expected: self.generation,
                observed: generation,
            });
        }
        if self.phase != WmiStartupPhase::InitReady {
            return Err(WmiStartupError::InitSubmissionNotAllowed { phase: self.phase });
        }
        self.phase = WmiStartupPhase::InitSubmissionUncertain;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedWmiEvent<'a> {
    pub completion: RxCompletion,
    pub received: ReceivedWmiEvent<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetiredWmiEvent<'a> {
    pub completion: RxCompletion,
    pub received: ReceivedWmiEvent<'a>,
    /// Present only for a validated management-TX completion. The matching
    /// frame lease is retired before this result is returned.
    pub retired_mgmt_tx: Option<CompletedMgmtTx>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartupObservedWmiEvent<'a> {
    pub retired: RetiredWmiEvent<'a>,
    /// Set only for SERVICE_READY, SERVICE_AVAILABLE, or READY events that
    /// advanced the generation-bound initial handshake.
    pub startup_transition: Option<WmiStartupTransition>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineError<C> {
    Consume(C),
    EmptyCompletion,
    CompletionOutOfRange {
        frame_bytes: usize,
        available: usize,
    },
    Wmi(Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetirementPipelineError<C> {
    Dispatch(PipelineError<C>),
    ManagementTxRetirement(MgmtTxError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupPipelineError<C> {
    Retirement(RetirementPipelineError<C>),
    Startup(WmiStartupError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Htc(HtcError),
    WrongCreditEndpoint { expected: u8, observed: u8 },
    ServiceReady(ServiceReadyError),
    ServiceAvailable(ServiceAvailableError),
    Ready(WmiReadyError),
    ScanEventTlv,
    MgmtTxCompletion(MgmtTxError),
}

/// Decode one copied CE2 receive frame and return HTC credits before
/// dispatching its WMI event. HTC trailer validity is independent of WMI
/// event validity, so a valid credit return is retained even if a known
/// event's body is malformed. Credit updates themselves are transactional:
/// a stale generation or an over-return leaves the ledger unchanged.
pub fn receive_wmi_event<'a>(
    generation: u64,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credit_ledger: &mut HtcCreditLedger,
    bytes: &'a [u8],
) -> Result<ReceivedWmiEvent<'a>, Error> {
    if credit_ledger.endpoint_id() != expected_endpoint_id {
        return Err(Error::WrongCreditEndpoint {
            expected: expected_endpoint_id,
            observed: credit_ledger.endpoint_id(),
        });
    }

    let frame = qcom_htc::parse_wmi_frame(bytes, expected_endpoint_id, max_message_size)
        .map_err(Error::Htc)?;
    let credit_returns = frame.credit_returns();
    let wmi_credits_returned = credit_ledger
        .apply_credit_returns(generation, credit_returns)
        .map_err(Error::Htc)?;

    let event = match frame.command_id {
        WMI_SERVICE_READY_EVENT_ID => WmiEvent::ServiceReady(
            ServiceReadyView::parse(frame.tlv_body).map_err(Error::ServiceReady)?,
        ),
        WMI_READY_EVENT_ID => {
            WmiEvent::Ready(WmiReadyEvent::parse(frame.tlv_body).map_err(Error::Ready)?)
        }
        WMI_SERVICE_AVAILABLE_EVENT_ID => WmiEvent::ServiceAvailable(
            ServiceAvailableView::parse(frame.tlv_body).map_err(Error::ServiceAvailable)?,
        ),
        WMI_SCAN_EVENT_ID => WmiEvent::Scan(parse_scan_event(frame.tlv_body)?),
        WMI_MGMT_TX_COMPLETION_EVENT_ID => WmiEvent::MgmtTxCompletion(
            sunfish_qcacld_mgmt_tx::parse_wmi_mgmt_tx_completion(frame.tlv_body)
                .map_err(Error::MgmtTxCompletion)?,
        ),
        event_id => WmiEvent::Unknown {
            event_id,
            body: frame.tlv_body,
        },
    };

    Ok(ReceivedWmiEvent {
        generation,
        endpoint_id: frame.endpoint_id,
        event_id: frame.command_id,
        tlv_body: frame.tlv_body,
        credit_returns,
        wmi_credits_returned,
        event,
    })
}

/// Compose the native CE2 copy callback with HTC credit return and WMI event
/// dispatch. The callback is normally `NativeIo::consume_wmi_rx` with the
/// current CE2 ring and buffer ledger; a failed or out-of-bounds completion
/// cannot mutate the HTC credit ledger.
pub fn consume_and_dispatch<'a, C>(
    generation: u64,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credit_ledger: &mut HtcCreditLedger,
    scratch: &'a mut [u8],
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, C>,
) -> Result<CompletedWmiEvent<'a>, PipelineError<C>> {
    let completion = consume(scratch).map_err(PipelineError::Consume)?;
    let frame_bytes = usize::from(completion.nbytes);
    if frame_bytes == 0 {
        return Err(PipelineError::EmptyCompletion);
    }
    let frame = scratch
        .get(..frame_bytes)
        .ok_or(PipelineError::CompletionOutOfRange {
            frame_bytes,
            available: scratch.len(),
        })?;
    let received = receive_wmi_event(
        generation,
        expected_endpoint_id,
        max_message_size,
        credit_ledger,
        frame,
    )
    .map_err(PipelineError::Wmi)?;
    Ok(CompletedWmiEvent {
        completion,
        received,
    })
}

/// Consume and dispatch one CE2 WMI event, then retire its matching
/// management-frame DMA lease when the event is a WMI management-TX
/// completion. A completion with an unknown descriptor or stale generation
/// fails closed and leaves the outstanding table unchanged. The caller must
/// quarantine uncertain native ring ownership after such a failure.
pub fn consume_dispatch_and_retire_mgmt_tx<'a, C, const N: usize>(
    generation: u64,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credit_ledger: &mut HtcCreditLedger,
    outstanding: &mut MgmtTxOutstanding<N>,
    scratch: &'a mut [u8],
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, C>,
) -> Result<RetiredWmiEvent<'a>, RetirementPipelineError<C>> {
    let completed = consume_and_dispatch(
        generation,
        expected_endpoint_id,
        max_message_size,
        credit_ledger,
        scratch,
        consume,
    )
    .map_err(RetirementPipelineError::Dispatch)?;

    let retired_mgmt_tx = match completed.received.event {
        WmiEvent::MgmtTxCompletion(completion) => Some(
            outstanding
                .complete(generation, completion)
                .map_err(RetirementPipelineError::ManagementTxRetirement)?,
        ),
        _ => None,
    };
    Ok(RetiredWmiEvent {
        completion: completed.completion,
        received: completed.received,
        retired_mgmt_tx,
    })
}

/// Consume one CE2 completion, dispatch its HTC/WMI contents, apply returned
/// credits, retire any matching management-TX lease, and advance the initial
/// startup session when this is a startup event. The supplied callback is
/// typically `NativeIo::consume_wmi_rx`; this function itself has no hardware
/// authority. Non-startup events remain dispatchable before WMI READY and do
/// not disturb the handshake tracker.
#[allow(clippy::too_many_arguments)]
pub fn consume_dispatch_and_observe_startup<'a, C, const N: usize>(
    generation: u64,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credit_ledger: &mut HtcCreditLedger,
    outstanding: &mut MgmtTxOutstanding<N>,
    startup: &mut SunfishWmiStartupSession,
    scratch: &'a mut [u8],
    consume: impl FnOnce(&mut [u8]) -> Result<RxCompletion, C>,
) -> Result<StartupObservedWmiEvent<'a>, StartupPipelineError<C>> {
    startup
        .validate_observed_endpoint(expected_endpoint_id)
        .map_err(StartupPipelineError::Startup)?;
    let retired = consume_dispatch_and_retire_mgmt_tx(
        generation,
        expected_endpoint_id,
        max_message_size,
        credit_ledger,
        outstanding,
        scratch,
        consume,
    )
    .map_err(StartupPipelineError::Retirement)?;

    let startup_transition = match retired.received.event_id {
        WMI_SERVICE_READY_EVENT_ID | WMI_SERVICE_AVAILABLE_EVENT_ID | WMI_READY_EVENT_ID => Some(
            startup
                .observe(&retired.received)
                .map_err(StartupPipelineError::Startup)?,
        ),
        _ => None,
    };
    Ok(StartupObservedWmiEvent {
        retired,
        startup_transition,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiCe2DrainStep {
    /// No completed CE2 descriptor is currently available.
    Empty,
    /// One descriptor was consumed and its WMI event was dispatched.
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiCe2DrainReport {
    pub completions: usize,
    /// True only if the service observed an empty ring and the caller's
    /// hardware-status acknowledgement succeeded.
    pub irq_acknowledged: bool,
    /// A full budget leaves CE2 masked and unacknowledged; the runtime must
    /// schedule another drain pass rather than waiting for a new IRQ.
    pub needs_deferred_drain: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiCe2DrainError<Service, Acknowledge> {
    ZeroBudget,
    Service(Service),
    Acknowledge(Acknowledge),
}

/// Drain and dispatch completed WMI CE2 descriptors before acknowledging the
/// latched device status. `service_one` must return `Completed` only
/// after one descriptor has been consumed and its event processed; `Empty`
/// means no completion remains. Callers should mask CE2 before entering. If
/// the finite budget is exhausted, this deliberately leaves the device status
/// unacknowledged so a deferred pass can finish draining before clearing the
/// interrupt.
pub fn drain_wmi_ce2_irq<Service, Acknowledge>(
    max_completions: usize,
    mut service_one: impl FnMut() -> Result<WmiCe2DrainStep, Service>,
    acknowledge: impl FnOnce() -> Result<(), Acknowledge>,
) -> Result<WmiCe2DrainReport, WmiCe2DrainError<Service, Acknowledge>> {
    if max_completions == 0 {
        return Err(WmiCe2DrainError::ZeroBudget);
    }
    let mut completions = 0;
    while completions < max_completions {
        match service_one().map_err(WmiCe2DrainError::Service)? {
            WmiCe2DrainStep::Empty => {
                acknowledge().map_err(WmiCe2DrainError::Acknowledge)?;
                return Ok(WmiCe2DrainReport {
                    completions,
                    irq_acknowledged: true,
                    needs_deferred_drain: false,
                });
            }
            WmiCe2DrainStep::Completed => completions += 1,
        }
    }
    Ok(WmiCe2DrainReport {
        completions,
        irq_acknowledged: false,
        needs_deferred_drain: true,
    })
}

fn parse_scan_event(bytes: &[u8]) -> Result<QcacldScanEvent, Error> {
    const HEADER_BYTES: usize = 4;
    const EVENT_BYTES: usize = HEADER_BYTES + WMI_SCAN_EVENT_FIXED_PARAM_VALUE_BYTES;
    const EXPECTED_TLV_HEADER: u32 = ((WMI_SCAN_EVENT_FIXED_PARAM_TLV_TAG as u32) << 16)
        | WMI_SCAN_EVENT_FIXED_PARAM_VALUE_BYTES as u32;

    if bytes.len() != EVENT_BYTES
        || u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != EXPECTED_TLV_HEADER
    {
        return Err(Error::ScanEventTlv);
    }
    let word = |offset: usize| {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    };
    Ok(QcacldScanEvent {
        event: word(4),
        reason: word(8),
        channel_frequency_mhz: word(12),
        requestor_id: word(16),
        scan_id: word(20),
        vdev_id: word(24),
        tsf_timestamp: word(28),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcacld_wmi::QcacldPeerLimit;
    use crate::qcom_htc::{
        HTC_FLAG_RECV_TRAILER, HTC_RECORD_CREDITS, HtcCreditLedger, HtcReady,
        encode_wmi_command_frame,
    };
    use crate::sunfish_qcacld_htc::HttStartupSession;
    use crate::sunfish_qcacld_resource::{
        SunfishQcacldResourceInputs, encode_sunfish_qcacld_wmi_init_frame,
    };
    use crate::sunfish_qcacld_wmi_startup::{
        encode_sunfish_qcacld_wmi_init_frame_from_session, plan_sunfish_qcacld_wmi_init_ce_tx,
    };

    const GENERATION: u64 = 7;
    const ENDPOINT: u8 = 2;
    const CREDIT_COUNT: u16 = 8;
    const CREDIT_SIZE: u16 = 64;
    const MAX_MESSAGE_SIZE: usize = 512;

    fn connected_htt() -> HttStartupSession {
        let ready = HtcReady {
            credit_count: CREDIT_COUNT,
            credit_size: CREDIT_SIZE,
            max_endpoints: 9,
            htc_version: 1,
            max_messages_per_bundle: 1,
            alternate_data_credit_size: 0,
        };
        let mut htt = HttStartupSession::new(GENERATION, ready).unwrap();
        let mut output = [0u8; 16];
        htt.stage_connect_request(GENERATION, &mut output).unwrap();
        htt.request_tx_submitted(GENERATION).unwrap();
        let response = [0, 0, 12, 0, 0, 0, 0, 0, 3, 0, 0, 3, 0, 5, 0, 2, 0, 0, 0, 0];
        htt.accept_connect_response_frame(GENERATION, &response)
            .unwrap();
        htt
    }

    fn startup_after_control() -> SunfishWmiStartupSession {
        let ready = [1, 0, 8, 0, 64, 0, 9, 0, 1, 1, 0, 0];
        let htt = connected_htt();
        let mut control =
            SunfishWmiControlSession::after_htt_connected(GENERATION, &ready, &htt).unwrap();
        let mut output = [0u8; 20];
        control
            .stage_connect_request(GENERATION, &mut output)
            .unwrap();
        control.control_tx_submitted(GENERATION).unwrap();
        let response = [
            0, 0, 12, 0, 0, 0, 0, 0, 3, 0, 0, 1, 0, ENDPOINT, 0, 2, 0, 0, 0, 0,
        ];
        control
            .accept_connect_response_frame(GENERATION, &response)
            .unwrap();
        control
            .stage_setup_complete(GENERATION, &mut output)
            .unwrap();
        control.control_tx_submitted(GENERATION).unwrap();
        SunfishWmiStartupSession::after_wmi_control_started(&control).unwrap()
    }

    fn credit_ledger() -> HtcCreditLedger {
        HtcCreditLedger::new(
            GENERATION,
            ENDPOINT,
            CREDIT_COUNT,
            CREDIT_SIZE,
            MAX_MESSAGE_SIZE as u16,
        )
        .unwrap()
    }

    fn reserve_one_credit(ledger: &mut HtcCreditLedger) {
        let mut output = [0u8; 32];
        let plan = encode_wmi_command_frame(
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            CREDIT_SIZE,
            ledger.available(),
            0x1234,
            &[0xaa],
            &mut output,
        )
        .unwrap();
        assert_eq!(plan.credits_required, 1);
        ledger.reserve(GENERATION, plan).unwrap();
    }

    fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn tlv(tag: u16, value: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + value.len());
        bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(value);
        bytes
    }

    fn rx_frame(event_id: u32, body: &[u8], credit_return: Option<(u8, u8)>) -> Vec<u8> {
        let wmi_payload_bytes = 4 + body.len();
        let trailer = credit_return.map_or_else(Vec::new, |(endpoint, credits)| {
            vec![HTC_RECORD_CREDITS, 4, 0, 0, endpoint, credits, 0, 0]
        });
        let declared = wmi_payload_bytes + trailer.len();
        let flags = if trailer.is_empty() {
            0
        } else {
            HTC_FLAG_RECV_TRAILER
        };
        let mut bytes = vec![0u8; 8 + declared];
        let first = u32::from(ENDPOINT) | (u32::from(flags) << 8) | ((declared as u32) << 16);
        put_word(&mut bytes, 0, first);
        bytes[4] = trailer.len() as u8;
        put_word(&mut bytes, 8, event_id);
        bytes[12..12 + body.len()].copy_from_slice(body);
        bytes[12 + body.len()..].copy_from_slice(&trailer);
        bytes
    }

    fn scan_body() -> Vec<u8> {
        let mut value = [0u8; WMI_SCAN_EVENT_FIXED_PARAM_VALUE_BYTES];
        for (index, word) in [1, 0, 2412, 4, 9, 2, 0x1234_5678].into_iter().enumerate() {
            put_word(&mut value, index * 4, word);
        }
        tlv(WMI_SCAN_EVENT_FIXED_PARAM_TLV_TAG, &value)
    }

    fn ready_body(status: u32) -> Vec<u8> {
        let mut value = [0u8; 60];
        for (index, word) in [0x0100_0000, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0]
            .into_iter()
            .enumerate()
        {
            put_word(&mut value, index * 4, word);
        }
        put_word(&mut value, 24, 0x3322_1102);
        put_word(&mut value, 28, 0x0000_5544);
        put_word(&mut value, 32, status);
        tlv(35, &value)
    }

    fn service_ready_body() -> Vec<u8> {
        let mut event = [0u8; 128];
        for (index, word) in [
            crate::qcacld_wmi::SUNFISH_WCN3998_FIRMWARE_BUILD,
            0x0100_0000,
            762,
            0x5f41_4351,
            0x0000_4c4d,
            0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            put_word(&mut event, index * 4, word);
        }
        let mut body = tlv(32, &event);
        body.extend_from_slice(&tlv(33, &[0; 36]));
        body.extend_from_slice(&tlv(16, &[0; 128]));
        body.extend_from_slice(&tlv(18, &[]));
        body
    }

    fn service_available_body() -> Vec<u8> {
        let mut fixed = [0u8; 20];
        put_word(&mut fixed, 0, 128);
        put_word(&mut fixed, 4, 0x0000_0001);
        tlv(559, &fixed)
    }

    fn mgmt_completion_body(descriptor_id: u32) -> Vec<u8> {
        let mut value = [0u8; 40];
        put_word(&mut value, 0, descriptor_id);
        put_word(&mut value, 4, 3);
        tlv(
            crate::sunfish_qcacld_mgmt_tx::WMI_MGMT_TX_COMPLETION_TLV_TAG,
            &value,
        )
    }

    #[test]
    fn ready_event_dispatches_and_applies_wmi_endpoint_credit_return() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let body = ready_body(0);
        let frame = rx_frame(WMI_READY_EVENT_ID, &body, Some((ENDPOINT, 1)));

        let received =
            receive_wmi_event(GENERATION, ENDPOINT, MAX_MESSAGE_SIZE, &mut ledger, &frame).unwrap();

        assert_eq!(received.event_id, WMI_READY_EVENT_ID);
        assert_eq!(received.wmi_credits_returned, 1);
        assert_eq!(received.credit_returns.for_endpoint(ENDPOINT), 1);
        assert_eq!(ledger.available(), CREDIT_COUNT);
        assert_eq!(ledger.in_flight(), 0);
        assert!(matches!(
            received.event,
            WmiEvent::Ready(WmiReadyEvent { total_peers: 0, .. })
        ));
    }

    #[test]
    fn ready_dispatch_rejects_the_short_ath10k_prefix_for_qcacld() {
        let mut ledger = credit_ledger();
        let mut short_fixed = [0u8; 36];
        put_word(&mut short_fixed, 32, 0);
        let frame = rx_frame(WMI_READY_EVENT_ID, &tlv(35, &short_fixed), None);

        assert_eq!(
            receive_wmi_event(GENERATION, ENDPOINT, MAX_MESSAGE_SIZE, &mut ledger, &frame),
            Err(Error::Ready(WmiReadyError::FixedParameterWrongLength))
        );
    }

    #[test]
    fn ce2_completion_is_bounded_before_credit_and_event_dispatch() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let body = ready_body(0);
        let frame = rx_frame(WMI_READY_EVENT_ID, &body, Some((ENDPOINT, 1)));
        let mut scratch = [0u8; 128];
        let frame_bytes = frame.len();
        let completed = consume_and_dispatch(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &mut scratch,
            |output| {
                output[..frame_bytes].copy_from_slice(&frame);
                Ok::<_, ()>(RxCompletion {
                    address: 0x9000,
                    nbytes: frame_bytes as u16,
                    transfer_id: 3,
                    byte_swapped: false,
                })
            },
        )
        .unwrap();
        assert_eq!(usize::from(completed.completion.nbytes), frame_bytes);
        assert!(matches!(completed.received.event, WmiEvent::Ready(_)));
        assert_eq!(completed.received.wmi_credits_returned, 1);
        assert_eq!(ledger.available(), CREDIT_COUNT);
        assert_eq!(ledger.in_flight(), 0);
    }

    #[test]
    fn failed_or_invalid_ce2_completion_does_not_change_credit_state() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let mut scratch = [0u8; 32];
        assert_eq!(
            consume_and_dispatch(
                GENERATION,
                ENDPOINT,
                MAX_MESSAGE_SIZE,
                &mut ledger,
                &mut scratch,
                |_| Err::<RxCompletion, _>("ce2 failed"),
            ),
            Err(PipelineError::Consume("ce2 failed"))
        );
        let scratch_bytes = scratch.len();
        assert_eq!(
            consume_and_dispatch(
                GENERATION,
                ENDPOINT,
                MAX_MESSAGE_SIZE,
                &mut ledger,
                &mut scratch,
                |_| {
                    Ok::<_, ()>(RxCompletion {
                        address: 0x9000,
                        nbytes: 0,
                        transfer_id: 1,
                        byte_swapped: false,
                    })
                },
            ),
            Err(PipelineError::EmptyCompletion)
        );
        assert_eq!(
            consume_and_dispatch(
                GENERATION,
                ENDPOINT,
                MAX_MESSAGE_SIZE,
                &mut ledger,
                &mut scratch,
                |_| {
                    Ok::<_, ()>(RxCompletion {
                        address: 0x9000,
                        nbytes: (scratch_bytes + 1) as u16,
                        transfer_id: 2,
                        byte_swapped: false,
                    })
                },
            ),
            Err(PipelineError::CompletionOutOfRange {
                frame_bytes: scratch_bytes + 1,
                available: scratch_bytes,
            })
        );
        assert_eq!(ledger.available(), CREDIT_COUNT - 1);
        assert_eq!(ledger.in_flight(), 1);
    }

    #[test]
    fn dispatches_service_ready_scan_and_management_completion() {
        let mut service_ledger = credit_ledger();
        let service = rx_frame(WMI_SERVICE_READY_EVENT_ID, &service_ready_body(), None);
        let service = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut service_ledger,
            &service,
        )
        .unwrap();
        assert!(matches!(service.event, WmiEvent::ServiceReady(_)));

        let mut scan_ledger = credit_ledger();
        let scan = rx_frame(WMI_SCAN_EVENT_ID, &scan_body(), None);
        let scan = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut scan_ledger,
            &scan,
        )
        .unwrap();
        assert_eq!(
            scan.event,
            WmiEvent::Scan(QcacldScanEvent {
                event: 1,
                reason: 0,
                channel_frequency_mhz: 2412,
                requestor_id: 4,
                scan_id: 9,
                vdev_id: 2,
                tsf_timestamp: 0x1234_5678,
            })
        );

        let mut available_ledger = credit_ledger();
        let available = rx_frame(
            WMI_SERVICE_AVAILABLE_EVENT_ID,
            &service_available_body(),
            None,
        );
        let available = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut available_ledger,
            &available,
        )
        .unwrap();
        assert!(matches!(
            available.event,
            WmiEvent::ServiceAvailable(view)
                if view.segment_offset() == 128 && view.service_enabled(128) == Some(true)
        ));

        let mut mgmt_ledger = credit_ledger();
        let mgmt = rx_frame(
            WMI_MGMT_TX_COMPLETION_EVENT_ID,
            &mgmt_completion_body(41),
            None,
        );
        let mgmt = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut mgmt_ledger,
            &mgmt,
        )
        .unwrap();
        assert!(matches!(
            mgmt.event,
            WmiEvent::MgmtTxCompletion(MgmtTxCompletion {
                descriptor_id: 41,
                status: mgmt_tx_status,
                ..
            }) if mgmt_tx_status == crate::sunfish_qcacld_mgmt_tx::MgmtTxCompletionStatus::CompleteNoAck
        ));
    }

    #[test]
    fn startup_gate_requires_service_ready_ext_init_submission_then_ready() {
        let mut session = startup_after_control();
        let mut ledger = credit_ledger();

        let service_body = service_ready_body();
        let service_frame = rx_frame(WMI_SERVICE_READY_EVENT_ID, &service_body, None);
        let service = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &service_frame,
        )
        .unwrap();
        assert_eq!(
            session.observe(&service),
            Ok(WmiStartupTransition::ServiceReadyAccepted)
        );
        assert_eq!(session.phase(), WmiStartupPhase::AwaitServiceAvailable);

        let available_body = service_available_body();
        let available_frame = rx_frame(WMI_SERVICE_AVAILABLE_EVENT_ID, &available_body, None);
        let available = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &available_frame,
        )
        .unwrap();
        assert_eq!(
            session.observe(&available),
            Ok(WmiStartupTransition::InitReady)
        );
        assert_eq!(session.phase(), WmiStartupPhase::InitReady);

        let ready_body = ready_body(0);
        let ready_frame = rx_frame(WMI_READY_EVENT_ID, &ready_body, None);
        let ready = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &ready_frame,
        )
        .unwrap();
        assert_eq!(
            session.observe(&ready),
            Err(WmiStartupError::UnexpectedEvent {
                phase: WmiStartupPhase::InitReady,
                event_id: WMI_READY_EVENT_ID,
            })
        );
        assert_eq!(session.phase(), WmiStartupPhase::InitReady);

        session.mark_init_submitted(GENERATION).unwrap();
        assert_eq!(session.phase(), WmiStartupPhase::AwaitReady);
        assert_eq!(session.observe(&ready), Ok(WmiStartupTransition::Ready));
        assert_eq!(session.phase(), WmiStartupPhase::Ready);
    }

    #[test]
    fn init_frame_uses_generation_bound_snapshots_after_event_buffers_are_reused() {
        let mut session = startup_after_control();
        let mut ledger = credit_ledger();
        let service_body = service_ready_body();
        let mut service_frame = rx_frame(WMI_SERVICE_READY_EVENT_ID, &service_body, None);
        let service = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &service_frame,
        )
        .unwrap();
        session.observe(&service).unwrap();
        service_frame.fill(0xa5);

        let available_body = service_available_body();
        let mut available_frame = rx_frame(WMI_SERVICE_AVAILABLE_EVENT_ID, &available_body, None);
        let available = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &available_frame,
        )
        .unwrap();
        session.observe(&available).unwrap();
        available_frame.fill(0x5a);
        assert_eq!(session.phase(), WmiStartupPhase::InitReady);

        let inputs =
            SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
        let service_view = ServiceReadyView::parse(&service_body).unwrap();
        let available_view = ServiceAvailableView::parse(&available_body).unwrap();
        let mut expected_body = [0u8; 512];
        let mut expected_frame = [0u8; 512];
        let expected = encode_sunfish_qcacld_wmi_init_frame(
            &service_view,
            &available_view,
            inputs,
            None,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            CREDIT_SIZE,
            CREDIT_COUNT,
            &mut expected_body,
            &mut expected_frame,
        )
        .unwrap();

        // The event views were tied to now-overwritten CE2 frames. Once
        // accepted, later buffer reuse must not change the WMI_INIT bytes.
        let mut actual_body = [0u8; 512];
        let mut actual_frame = [0u8; 512];
        let actual = encode_sunfish_qcacld_wmi_init_frame_from_session(
            &session,
            GENERATION,
            inputs,
            None,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            CREDIT_SIZE,
            CREDIT_COUNT,
            &mut actual_body,
            &mut actual_frame,
        )
        .unwrap();
        assert_eq!(actual.resource, expected.resource);
        assert_eq!(actual.body_bytes, expected.body_bytes);
        assert_eq!(actual.htc, expected.htc);
        assert_eq!(
            &actual_body[..actual.body_bytes],
            &expected_body[..expected.body_bytes]
        );
        assert_eq!(
            &actual_frame[..actual.htc.frame_len],
            &expected_frame[..expected.htc.frame_len]
        );

        let mappings = [
            crate::qcom_wmi_ce::ServiceCeMapping {
                service_id: crate::qcom_wmi_ce::WMI_CONTROL_SERVICE_ID,
                direction: crate::qcom_wmi_ce::Direction::HostToTarget,
                engine: 3,
            },
            crate::qcom_wmi_ce::ServiceCeMapping {
                service_id: crate::qcom_wmi_ce::WMI_CONTROL_SERVICE_ID,
                direction: crate::qcom_wmi_ce::Direction::TargetToHost,
                engine: 2,
            },
        ];
        let profile = crate::qcom_ce_v3_profile::Profile::Mission;
        let pool = crate::qcom_wmi_ce::DmaPool::new(0x8000_0000, 0x40000).unwrap();
        let rings =
            crate::qcom_ce_v3_profile::plan_rings(profile, pool.base(), pool.bytes()).unwrap();
        let tx_buffer = crate::qcom_wmi_ce::TxBufferWindow {
            iova: (pool.base() + rings.total_bytes() + 4095) & !4095,
            capacity: 512,
        };
        let cursor = crate::qcom_ce_v3::RingCursor::new(32).unwrap();
        let mut planned_body = [0u8; 512];
        let mut planned_frame = [0u8; 512];
        let ce_plan = plan_sunfish_qcacld_wmi_init_ce_tx(
            &session,
            GENERATION,
            inputs,
            None,
            ENDPOINT,
            7,
            MAX_MESSAGE_SIZE,
            CREDIT_SIZE,
            CREDIT_COUNT,
            &mappings,
            pool,
            tx_buffer,
            profile,
            &rings,
            cursor,
            &mut planned_body,
            &mut planned_frame,
        )
        .unwrap();
        assert_eq!(ce_plan.frame.body_bytes, 312);
        assert_eq!(ce_plan.frame.htc.frame_len, 324);
        assert_eq!(ce_plan.ce.route().host_to_target_engine, 3);
        assert_eq!(ce_plan.ce.route().target_to_host_engine, 2);
        assert_eq!(ce_plan.ce.descriptor().address(), tx_buffer.iova);
        assert_eq!(ce_plan.ce.frame_bytes(), 324);
        assert_eq!(ce_plan.ce.descriptor().transfer_id(), 7);
        assert_eq!(cursor.producer_index(), 0);
        assert_eq!(cursor.pending(), 0);
    }

    #[test]
    fn init_frame_builder_rejects_stale_generation_and_wrong_startup_phase() {
        let inputs =
            SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        let waiting = SunfishWmiStartupSession::new(GENERATION);
        let args = (
            inputs,
            None,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            CREDIT_SIZE,
            CREDIT_COUNT,
        );
        assert_eq!(
            encode_sunfish_qcacld_wmi_init_frame_from_session(
                &waiting,
                GENERATION + 1,
                args.0,
                args.1,
                args.2,
                args.3,
                args.4,
                args.5,
                &mut body,
                &mut frame,
            ),
            Err(
                crate::sunfish_qcacld_resource::SunfishQcacldWmiFrameError::WrongGeneration {
                    expected: GENERATION,
                    observed: GENERATION + 1,
                }
            )
        );
        assert_eq!(
            encode_sunfish_qcacld_wmi_init_frame_from_session(
                &waiting, GENERATION, args.0, args.1, args.2, args.3, args.4, args.5, &mut body,
                &mut frame,
            ),
            Err(crate::sunfish_qcacld_resource::SunfishQcacldWmiFrameError::StartupNotReady)
        );
    }

    #[test]
    fn ce2_pipeline_advances_startup_only_after_valid_events_and_init_submission() {
        let mut credits = credit_ledger();
        let mut outstanding = MgmtTxOutstanding::<4>::new(GENERATION).unwrap();
        let mut startup = SunfishWmiStartupSession::new(GENERATION);
        let mut scratch = [0u8; 512];

        macro_rules! consume_frame {
            ($frame:expr) => {{
                let frame = $frame;
                let frame_bytes = frame.len();
                consume_dispatch_and_observe_startup(
                    GENERATION,
                    ENDPOINT,
                    MAX_MESSAGE_SIZE,
                    &mut credits,
                    &mut outstanding,
                    &mut startup,
                    &mut scratch,
                    |output| {
                        output[..frame_bytes].copy_from_slice(&frame);
                        Ok::<_, ()>(RxCompletion {
                            address: 0x9000,
                            nbytes: frame_bytes as u16,
                            transfer_id: 1,
                            byte_swapped: false,
                        })
                    },
                )
            }};
        }

        let service_ready = rx_frame(WMI_SERVICE_READY_EVENT_ID, &service_ready_body(), None);
        let observed = consume_frame!(service_ready).unwrap();
        assert_eq!(
            observed.startup_transition,
            Some(WmiStartupTransition::ServiceReadyAccepted)
        );
        assert_eq!(startup.phase(), WmiStartupPhase::AwaitServiceAvailable);
        let expected_service_ready = ServiceReadyView::parse(&service_ready_body())
            .unwrap()
            .sunfish_startup_snapshot()
            .unwrap();
        assert_eq!(
            startup.service_ready_snapshot(),
            Some(expected_service_ready)
        );
        assert!(startup.service_available_snapshot().is_none());

        let service_available = rx_frame(
            WMI_SERVICE_AVAILABLE_EVENT_ID,
            &service_available_body(),
            None,
        );
        let observed = consume_frame!(service_available).unwrap();
        assert_eq!(
            observed.startup_transition,
            Some(WmiStartupTransition::InitReady)
        );
        assert_eq!(startup.phase(), WmiStartupPhase::InitReady);
        let expected_service_available = ServiceAvailableView::parse(&service_available_body())
            .unwrap()
            .snapshot();
        assert_eq!(
            startup.service_ready_snapshot(),
            Some(expected_service_ready)
        );
        assert_eq!(
            startup.service_available_snapshot(),
            Some(expected_service_available)
        );
        assert_eq!(
            expected_service_ready.view().service_enabled(99),
            Some(false)
        );
        assert_eq!(
            expected_service_available.view().service_enabled(128),
            Some(true)
        );

        let ready = rx_frame(WMI_READY_EVENT_ID, &ready_body(0), None);
        assert_eq!(
            consume_frame!(ready.clone()),
            Err(StartupPipelineError::Startup(
                WmiStartupError::UnexpectedEvent {
                    phase: WmiStartupPhase::InitReady,
                    event_id: WMI_READY_EVENT_ID,
                }
            ))
        );
        assert_eq!(startup.phase(), WmiStartupPhase::InitReady);

        // This transition is driven by the caller only after CE3 publication
        // reports success. The receive path cannot infer that from READY.
        startup.mark_init_submitted(GENERATION).unwrap();
        let observed = consume_frame!(ready).unwrap();
        assert_eq!(
            observed.startup_transition,
            Some(WmiStartupTransition::Ready)
        );
        assert_eq!(startup.phase(), WmiStartupPhase::Ready);
        assert_eq!(outstanding.len(), 0);
    }

    #[test]
    fn startup_gate_rejects_out_of_order_and_duplicate_events_without_advancing() {
        let mut session = SunfishWmiStartupSession::new(GENERATION);
        let mut ledger = credit_ledger();

        let available_body = service_available_body();
        let available_frame = rx_frame(WMI_SERVICE_AVAILABLE_EVENT_ID, &available_body, None);
        let available = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &available_frame,
        )
        .unwrap();
        assert_eq!(
            session.observe(&available),
            Err(WmiStartupError::UnexpectedEvent {
                phase: WmiStartupPhase::AwaitServiceReady,
                event_id: WMI_SERVICE_AVAILABLE_EVENT_ID,
            })
        );
        assert_eq!(session.phase(), WmiStartupPhase::AwaitServiceReady);

        let service_body = service_ready_body();
        let service_frame = rx_frame(WMI_SERVICE_READY_EVENT_ID, &service_body, None);
        let service = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &service_frame,
        )
        .unwrap();
        session.observe(&service).unwrap();
        assert_eq!(
            session.observe(&service),
            Err(WmiStartupError::UnexpectedEvent {
                phase: WmiStartupPhase::AwaitServiceAvailable,
                event_id: WMI_SERVICE_READY_EVENT_ID,
            })
        );
        assert_eq!(session.phase(), WmiStartupPhase::AwaitServiceAvailable);
        assert_eq!(
            session.mark_init_submitted(GENERATION),
            Err(WmiStartupError::InitSubmissionNotAllowed {
                phase: WmiStartupPhase::AwaitServiceAvailable,
            })
        );
    }

    #[test]
    fn startup_session_requires_started_control_handshake_and_binds_its_endpoint() {
        let ready = [1, 0, 8, 0, 64, 0, 9, 0, 1, 1, 0, 0];
        let htt = connected_htt();
        let mut control =
            SunfishWmiControlSession::after_htt_connected(GENERATION, &ready, &htt).unwrap();
        assert_eq!(
            SunfishWmiStartupSession::after_wmi_control_started(&control),
            Err(WmiStartupError::ControlNotStarted {
                phase: WmiControlPhase::Ready,
            })
        );

        let mut output = [0u8; 20];
        control
            .stage_connect_request(GENERATION, &mut output)
            .unwrap();
        control.control_tx_submitted(GENERATION).unwrap();
        let response = [
            0, 0, 12, 0, 0, 0, 0, 0, 3, 0, 0, 1, 0, ENDPOINT, 0, 2, 0, 0, 0, 0,
        ];
        control
            .accept_connect_response_frame(GENERATION, &response)
            .unwrap();
        control
            .stage_setup_complete(GENERATION, &mut output)
            .unwrap();
        control.control_tx_submitted(GENERATION).unwrap();

        let mut startup = SunfishWmiStartupSession::after_wmi_control_started(&control).unwrap();
        assert_eq!(startup.generation(), GENERATION);
        assert_eq!(
            startup.htt_endpoint().unwrap().endpoint_id(),
            htt.endpoint().unwrap().endpoint_id()
        );
        assert_eq!(
            startup.wmi_control_endpoint().unwrap().endpoint_id(),
            ENDPOINT
        );
        assert_eq!(startup.control_endpoint_id(), Some(ENDPOINT));
        assert_eq!(startup.validate_observed_endpoint(ENDPOINT), Ok(()));
        assert_eq!(
            startup.validate_observed_endpoint(ENDPOINT + 1),
            Err(WmiStartupError::ControlEndpointMismatch {
                expected: ENDPOINT,
                observed: ENDPOINT + 1,
            })
        );
        startup.advance_generation(GENERATION + 1).unwrap();
        assert_eq!(startup.htt_endpoint(), None);
        assert_eq!(startup.wmi_control_endpoint(), None);
        assert_eq!(startup.control_endpoint_id(), None);
        assert_eq!(
            startup.require_control_endpoint(ENDPOINT),
            Err(WmiControlEndpointError::SessionNotBound)
        );
    }

    #[test]
    fn bound_startup_rejects_wrong_ce2_route_before_consuming_a_completion() {
        let mut startup = startup_after_control();
        let mut credits = credit_ledger();
        let mut outstanding = MgmtTxOutstanding::<4>::new(GENERATION).unwrap();
        let mut scratch = [0u8; 32];
        let mut consumed = false;

        let result = consume_dispatch_and_observe_startup(
            GENERATION,
            ENDPOINT + 1,
            MAX_MESSAGE_SIZE,
            &mut credits,
            &mut outstanding,
            &mut startup,
            &mut scratch,
            |_| {
                consumed = true;
                Ok::<_, ()>(RxCompletion {
                    address: 0x9000,
                    nbytes: 1,
                    transfer_id: 1,
                    byte_swapped: false,
                })
            },
        );

        assert_eq!(
            result,
            Err(StartupPipelineError::Startup(
                WmiStartupError::ControlEndpointMismatch {
                    expected: ENDPOINT,
                    observed: ENDPOINT + 1,
                }
            ))
        );
        assert!(!consumed);
        assert_eq!(credits.available(), CREDIT_COUNT);
        assert_eq!(startup.phase(), WmiStartupPhase::AwaitServiceReady);
    }

    #[test]
    fn startup_gate_rejects_bad_target_evidence_and_stale_generations() {
        let mut session = SunfishWmiStartupSession::new(GENERATION);
        let mut ledger = credit_ledger();

        let mut bad_service_body = service_ready_body();
        // The first fixed-parameter word is the captured firmware build.
        put_word(&mut bad_service_body, 4, 0xdead_beef);
        let bad_frame = rx_frame(WMI_SERVICE_READY_EVENT_ID, &bad_service_body, None);
        let bad_service = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &bad_frame,
        )
        .unwrap();
        assert!(matches!(
            session.observe(&bad_service),
            Err(WmiStartupError::StartupPolicy(
                SunfishWmiStartupError::MemoryRequests(
                    crate::qcacld_wmi::EmptyMemoryRequestError::FirmwareBuildMismatch {
                        observed: 0xdead_beef,
                        ..
                    }
                )
            ))
        ));
        assert_eq!(session.phase(), WmiStartupPhase::AwaitServiceReady);

        let service_body = service_ready_body();
        let service_frame = rx_frame(WMI_SERVICE_READY_EVENT_ID, &service_body, None);
        let service = receive_wmi_event(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut ledger,
            &service_frame,
        )
        .unwrap();
        session.observe(&service).unwrap();
        assert!(session.service_ready_snapshot().is_some());
        session.advance_generation(GENERATION + 1).unwrap();
        assert_eq!(session.generation(), GENERATION + 1);
        assert_eq!(session.phase(), WmiStartupPhase::AwaitServiceReady);
        assert!(session.service_ready_snapshot().is_none());
        assert!(session.service_available_snapshot().is_none());
        assert_eq!(
            session.observe(&service),
            Err(WmiStartupError::WrongGeneration {
                expected: GENERATION + 1,
                observed: GENERATION,
            })
        );
        assert_eq!(session.phase(), WmiStartupPhase::AwaitServiceReady);
        assert_eq!(
            session.advance_generation(GENERATION + 1),
            Err(WmiStartupError::GenerationNotAdvanced {
                current: GENERATION + 1,
                requested: GENERATION + 1,
            })
        );
    }

    #[test]
    fn valid_management_completion_retires_only_its_matching_frame_lease() {
        let mut credits = credit_ledger();
        let mut outstanding = MgmtTxOutstanding::<2>::new(GENERATION).unwrap();
        let lease = crate::sunfish_qcacld_mgmt_tx::MgmtTxLease {
            generation: GENERATION,
            descriptor_id: 41,
            frame_iova: 0x4000,
            frame_bytes: 48,
        };
        outstanding.reserve(lease).unwrap();
        let frame = rx_frame(
            WMI_MGMT_TX_COMPLETION_EVENT_ID,
            &mgmt_completion_body(41),
            None,
        );
        let frame_bytes = frame.len();
        let mut scratch = [0u8; 128];

        let retired = consume_dispatch_and_retire_mgmt_tx(
            GENERATION,
            ENDPOINT,
            MAX_MESSAGE_SIZE,
            &mut credits,
            &mut outstanding,
            &mut scratch,
            |output| {
                output[..frame_bytes].copy_from_slice(&frame);
                Ok::<_, ()>(RxCompletion {
                    address: 0x9000,
                    nbytes: frame_bytes as u16,
                    transfer_id: 7,
                    byte_swapped: false,
                })
            },
        )
        .unwrap();

        assert_eq!(retired.retired_mgmt_tx.unwrap().lease, lease);
        assert_eq!(
            retired.retired_mgmt_tx.unwrap().status,
            crate::sunfish_qcacld_mgmt_tx::MgmtTxCompletionStatus::CompleteNoAck
        );
        assert!(matches!(
            retired.received.event,
            WmiEvent::MgmtTxCompletion(MgmtTxCompletion {
                descriptor_id: 41,
                ..
            })
        ));
        assert!(outstanding.is_empty());
    }

    #[test]
    fn unknown_management_completion_does_not_retire_another_frame_lease() {
        let mut credits = credit_ledger();
        reserve_one_credit(&mut credits);
        let mut outstanding = MgmtTxOutstanding::<2>::new(GENERATION).unwrap();
        outstanding
            .reserve(crate::sunfish_qcacld_mgmt_tx::MgmtTxLease {
                generation: GENERATION,
                descriptor_id: 99,
                frame_iova: 0x5000,
                frame_bytes: 48,
            })
            .unwrap();
        let frame = rx_frame(
            WMI_MGMT_TX_COMPLETION_EVENT_ID,
            &mgmt_completion_body(41),
            Some((ENDPOINT, 1)),
        );
        let frame_bytes = frame.len();
        let mut scratch = [0u8; 128];

        assert_eq!(
            consume_dispatch_and_retire_mgmt_tx(
                GENERATION,
                ENDPOINT,
                MAX_MESSAGE_SIZE,
                &mut credits,
                &mut outstanding,
                &mut scratch,
                |output| {
                    output[..frame_bytes].copy_from_slice(&frame);
                    Ok::<_, ()>(RxCompletion {
                        address: 0x9000,
                        nbytes: frame_bytes as u16,
                        transfer_id: 8,
                        byte_swapped: false,
                    })
                },
            ),
            Err(RetirementPipelineError::ManagementTxRetirement(
                MgmtTxError::UnknownDescriptorId { descriptor_id: 41 }
            ))
        );
        assert_eq!(outstanding.len(), 1);
        assert_eq!(credits.available(), CREDIT_COUNT);
        assert_eq!(credits.in_flight(), 0);
    }

    #[test]
    fn unknown_event_keeps_its_body_and_transport_credit_report() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let frame = rx_frame(0x1234, &[0xaa, 0xbb], Some((ENDPOINT, 1)));
        let received =
            receive_wmi_event(GENERATION, ENDPOINT, MAX_MESSAGE_SIZE, &mut ledger, &frame).unwrap();
        assert_eq!(
            received.event,
            WmiEvent::Unknown {
                event_id: 0x1234,
                body: &[0xaa, 0xbb],
            }
        );
        assert_eq!(received.wmi_credits_returned, 1);
    }

    #[test]
    fn malformed_known_event_does_not_lose_a_valid_transport_credit_return() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let frame = rx_frame(WMI_SCAN_EVENT_ID, &[0, 0, 0, 0], Some((ENDPOINT, 1)));
        assert_eq!(
            receive_wmi_event(GENERATION, ENDPOINT, MAX_MESSAGE_SIZE, &mut ledger, &frame,),
            Err(Error::ScanEventTlv)
        );
        assert_eq!(ledger.available(), CREDIT_COUNT);
        assert_eq!(ledger.in_flight(), 0);
    }

    #[test]
    fn invalid_credit_returns_fail_without_mutating_the_ledger() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let frame = rx_frame(WMI_READY_EVENT_ID, &ready_body(0), Some((ENDPOINT, 2)));
        assert_eq!(
            receive_wmi_event(GENERATION, ENDPOINT, MAX_MESSAGE_SIZE, &mut ledger, &frame,),
            Err(Error::Htc(HtcError::CreditReportExceedsOutstanding {
                reported: 2,
                outstanding: 1,
            }))
        );
        assert_eq!(ledger.available(), CREDIT_COUNT - 1);
        assert_eq!(ledger.in_flight(), 1);
    }

    #[test]
    fn stale_generation_and_wrong_endpoint_are_rejected_before_credit_mutation() {
        let mut ledger = credit_ledger();
        reserve_one_credit(&mut ledger);
        let frame = rx_frame(WMI_READY_EVENT_ID, &ready_body(0), Some((ENDPOINT, 1)));
        assert_eq!(
            receive_wmi_event(
                GENERATION + 1,
                ENDPOINT,
                MAX_MESSAGE_SIZE,
                &mut ledger,
                &frame,
            ),
            Err(Error::Htc(HtcError::WrongGeneration))
        );
        assert_eq!(ledger.available(), CREDIT_COUNT - 1);

        assert_eq!(
            receive_wmi_event(
                GENERATION,
                ENDPOINT + 1,
                MAX_MESSAGE_SIZE,
                &mut ledger,
                &frame,
            ),
            Err(Error::WrongCreditEndpoint {
                expected: ENDPOINT + 1,
                observed: ENDPOINT,
            })
        );
        assert_eq!(ledger.available(), CREDIT_COUNT - 1);
    }

    #[test]
    fn ce2_irq_drain_acknowledges_only_after_an_empty_poll() {
        use core::cell::Cell;

        let steps = [
            WmiCe2DrainStep::Completed,
            WmiCe2DrainStep::Completed,
            WmiCe2DrainStep::Empty,
        ];
        let index = Cell::new(0);
        let ack_order = Cell::new(0);
        let report = drain_wmi_ce2_irq(
            4,
            || {
                let next = index.get() + 1;
                index.set(next);
                Ok::<_, ()>(steps[next - 1])
            },
            || {
                ack_order.set(index.get() + 1);
                Ok::<_, ()>(())
            },
        )
        .unwrap();

        assert_eq!(index.get(), 3);
        assert_eq!(ack_order.get(), 4);
        assert_eq!(
            report,
            WmiCe2DrainReport {
                completions: 2,
                irq_acknowledged: true,
                needs_deferred_drain: false,
            }
        );
    }

    #[test]
    fn ce2_irq_drain_defers_ack_when_its_budget_is_exhausted() {
        let mut acked = false;
        let report = drain_wmi_ce2_irq(
            2,
            || Ok::<_, ()>(WmiCe2DrainStep::Completed),
            || {
                acked = true;
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        assert_eq!(
            report,
            WmiCe2DrainReport {
                completions: 2,
                irq_acknowledged: false,
                needs_deferred_drain: true,
            }
        );
        assert!(!acked);

        let next = drain_wmi_ce2_irq(
            2,
            || Ok::<_, ()>(WmiCe2DrainStep::Empty),
            || {
                acked = true;
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        assert_eq!(next.completions, 0);
        assert!(next.irq_acknowledged);
        assert!(!next.needs_deferred_drain);
        assert!(acked);
    }

    #[test]
    fn ce2_irq_drain_fails_closed_on_service_or_acknowledgement_error() {
        let mut acked = false;
        assert_eq!(
            drain_wmi_ce2_irq(
                2,
                || Err::<WmiCe2DrainStep, _>("consume/dispatch"),
                || {
                    acked = true;
                    Ok::<_, ()>(())
                },
            ),
            Err(WmiCe2DrainError::Service("consume/dispatch"))
        );
        assert!(!acked);

        assert_eq!(
            drain_wmi_ce2_irq(
                2,
                || Ok::<_, ()>(WmiCe2DrainStep::Empty),
                || Err::<(), _>("ack"),
            ),
            Err(WmiCe2DrainError::Acknowledge("ack"))
        );
        assert_eq!(
            drain_wmi_ce2_irq(
                0,
                || Ok::<_, ()>(WmiCe2DrainStep::Empty),
                || Ok::<_, ()>(()),
            ),
            Err(WmiCe2DrainError::ZeroBudget)
        );
    }
}
