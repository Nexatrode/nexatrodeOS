//! Composition of validated QCACLD startup evidence into WMI_INIT bytes.
//!
//! This module joins the generation-bound CE2 WMI event session to the
//! source-derived resource and HTC encoders. Its frame and descriptor planners
//! are offline; its publisher delegates to the existing NativeIo CE Gate.
#![forbid(unsafe_code)]

pub use crate::sunfish_qcacld_htc::wmi_control::{
    SunfishWmiControlSession, WmiControlPhase, WmiControlSessionError,
};

use crate::sunfish_qcacld_resource::{
    SunfishQcacldResourceInputs, SunfishQcacldWmiFrameError, SunfishQcacldWmiFramePlan,
    encode_sunfish_qcacld_wmi_init_frame,
};
use crate::sunfish_qcacld_wmi_rx::{StartupObservedWmiEvent, WmiStartupTransition};
use crate::sunfish_qcacld_wmi_rx::{SunfishWmiStartupSession, WmiStartupError, WmiStartupPhase};
use crate::sunfish_qcacld_wmi_rx_native::NativeWmiCe2EventContext;
use crate::{
    qcom_ce_v3::RingCursor,
    qcom_ce_v3_profile::{Profile, RingPlan},
    qcom_wmi_ce::{self, DmaPool, ServiceCeMapping, TxBufferWindow, WmiCeTxPlan},
};
use crate::{
    qcom_ce_v3_native_io::{Gate, NativeIo, TxPublishError},
    qcom_htc::{HtcCreditLedger, HtcError},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishQcacldWmiInitCeError {
    Frame(SunfishQcacldWmiFrameError),
    Ce(qcom_wmi_ce::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishQcacldWmiInitPublishError {
    PlanGeneration {
        planned: u64,
        active: u64,
    },
    Startup(WmiStartupError),
    CreditReserve(HtcError),
    Publish(TxPublishError),
    CreditCancelFailed {
        publish: TxPublishError,
        credit: HtcError,
    },
    PublicationUncertain,
    UncertainQuarantineFailed(WmiStartupError),
    PublishedSessionCommitFailed(WmiStartupError),
}

/// Caller-selected route and storage inputs for publishing the initial WMI_INIT
/// from the accepted SERVICE_READY/SERVICE_AVAILABLE snapshots. These values
/// describe a candidate command and CE window; the NativeIo Gate still decides
/// whether live publication is authorized.
pub struct SunfishQcacldWmiInitRuntime<'a> {
    pub inputs: SunfishQcacldResourceInputs,
    pub hardware_mode_index: Option<u32>,
    pub transfer_id: u16,
    pub mappings: &'a [ServiceCeMapping],
    pub dma_pool: DmaPool,
    pub buffer: TxBufferWindow,
    /// Live WMI_CONTROL CE3 source-ring cursor, including earlier HTC control
    /// publications from the same transport generation.
    pub tx_cursor: &'a mut RingCursor,
    pub wmi_body_output: &'a mut [u8],
    pub htc_frame_output: &'a mut [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishQcacldWmiInitRuntimeError {
    Plan(SunfishQcacldWmiInitCeError),
    Publish(SunfishQcacldWmiInitPublishError),
}

impl core::fmt::Display for SunfishQcacldWmiInitRuntimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Plan(_) => f.write_str("failed to plan the accepted WMI_INIT transaction"),
            Self::Publish(_) => f.write_str("failed to publish WMI_INIT through CE3"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishQcacldWmiInitRuntimeOutcome {
    NotReady,
    Published,
}

/// WMI_INIT bytes and the corresponding CE3 descriptor preflight. Both plans
/// remain unpublished and the CE cursor is unchanged.
pub struct SunfishQcacldWmiInitCePlan<'a> {
    pub generation: u64,
    pub frame: SunfishQcacldWmiFramePlan,
    pub ce: WmiCeTxPlan<'a>,
}

/// Publish WMI_INIT exactly when CE2 has accepted the SERVICE_AVAILABLE
/// transition. This joins the bounded receive dispatcher to the existing CE3
/// planner and NativeIo publisher. It sends no other event and grants no
/// authority beyond the supplied live Gate and caller-selected memory inputs.
pub fn publish_sunfish_qcacld_wmi_init_after_observe<G: Gate>(
    event: &StartupObservedWmiEvent<'_>,
    context: NativeWmiCe2EventContext<'_, G>,
    runtime: &mut SunfishQcacldWmiInitRuntime<'_>,
) -> Result<SunfishQcacldWmiInitRuntimeOutcome, SunfishQcacldWmiInitRuntimeError> {
    if event.startup_transition != Some(WmiStartupTransition::InitReady) {
        return Ok(SunfishQcacldWmiInitRuntimeOutcome::NotReady);
    }

    let NativeWmiCe2EventContext {
        io,
        generation,
        profile,
        rings,
        credits,
        startup,
        ..
    } = context;
    let plan = plan_sunfish_qcacld_wmi_init_ce_tx(
        startup,
        generation,
        runtime.inputs,
        runtime.hardware_mode_index,
        credits.endpoint_id(),
        runtime.transfer_id,
        usize::from(credits.max_message_size()),
        credits.credit_size(),
        credits.available(),
        runtime.mappings,
        runtime.dma_pool,
        runtime.buffer,
        profile,
        rings,
        *runtime.tx_cursor,
        runtime.wmi_body_output,
        runtime.htc_frame_output,
    )
    .map_err(SunfishQcacldWmiInitRuntimeError::Plan)?;
    publish_sunfish_qcacld_wmi_init(
        io,
        startup,
        generation,
        plan,
        credits,
        profile,
        rings,
        runtime.tx_cursor,
    )
    .map_err(SunfishQcacldWmiInitRuntimeError::Publish)?;
    Ok(SunfishQcacldWmiInitRuntimeOutcome::Published)
}

/// Build the initial WMI_INIT frame from accepted, owned startup evidence.
///
/// This is the path for callers whose CE2 receive scratch has already been
/// reused. It requires the startup session to be at `InitReady` and to match
/// the current transport generation, and that session must be bound to a
/// completed WMI_CONTROL_SVC handshake. Success only encodes protocol bytes and
/// an HTC credit plan; it does not submit CE3 descriptors or advance the
/// startup session. Use `publish_sunfish_qcacld_wmi_init` to reserve credits,
/// publish through the CE Gate, and commit the startup phase on success.
#[allow(clippy::too_many_arguments)]
pub fn encode_sunfish_qcacld_wmi_init_frame_from_session(
    startup: &SunfishWmiStartupSession,
    active_generation: u64,
    inputs: SunfishQcacldResourceInputs,
    hardware_mode_index: Option<u32>,
    endpoint_id: u8,
    max_message_size: usize,
    credit_size: u16,
    available_credits: u16,
    wmi_body_output: &mut [u8],
    htc_frame_output: &mut [u8],
) -> Result<SunfishQcacldWmiFramePlan, SunfishQcacldWmiFrameError> {
    if startup.generation() != active_generation {
        return Err(SunfishQcacldWmiFrameError::WrongGeneration {
            expected: startup.generation(),
            observed: active_generation,
        });
    }
    if startup.phase() != WmiStartupPhase::InitReady {
        return Err(SunfishQcacldWmiFrameError::StartupNotReady);
    }
    startup
        .require_control_endpoint(endpoint_id)
        .map_err(|error| match error {
            crate::sunfish_qcacld_wmi_rx::WmiControlEndpointError::SessionNotBound => {
                SunfishQcacldWmiFrameError::ControlSessionNotBound
            }
            crate::sunfish_qcacld_wmi_rx::WmiControlEndpointError::Mismatch {
                expected,
                observed,
            } => SunfishQcacldWmiFrameError::ControlEndpointMismatch { expected, observed },
        })?;
    let service_ready = startup
        .service_ready_snapshot()
        .ok_or(SunfishQcacldWmiFrameError::StartupEvidenceUnavailable)?;
    let service_available = startup
        .service_available_snapshot()
        .ok_or(SunfishQcacldWmiFrameError::StartupEvidenceUnavailable)?;
    encode_sunfish_qcacld_wmi_init_frame(
        &service_ready.view(),
        &service_available.view(),
        inputs,
        hardware_mode_index,
        endpoint_id,
        max_message_size,
        credit_size,
        available_credits,
        wmi_body_output,
        htc_frame_output,
    )
}

/// Compose the session-backed WMI_INIT frame with the existing CE3 source-ring
/// planner. This performs no DMA writes or MMIO and does not reserve credits or
/// advance the cursor; the caller still needs live ownership before publishing.
#[allow(clippy::too_many_arguments)]
pub fn plan_sunfish_qcacld_wmi_init_ce_tx<'a>(
    startup: &SunfishWmiStartupSession,
    active_generation: u64,
    inputs: SunfishQcacldResourceInputs,
    hardware_mode_index: Option<u32>,
    endpoint_id: u8,
    transfer_id: u16,
    max_message_size: usize,
    credit_size: u16,
    available_credits: u16,
    mappings: &[ServiceCeMapping],
    dma_pool: DmaPool,
    buffer: TxBufferWindow,
    profile: Profile,
    rings: &RingPlan,
    cursor: RingCursor,
    wmi_body_output: &mut [u8],
    htc_frame_output: &'a mut [u8],
) -> Result<SunfishQcacldWmiInitCePlan<'a>, SunfishQcacldWmiInitCeError> {
    let frame = encode_sunfish_qcacld_wmi_init_frame_from_session(
        startup,
        active_generation,
        inputs,
        hardware_mode_index,
        endpoint_id,
        max_message_size,
        credit_size,
        available_credits,
        wmi_body_output,
        htc_frame_output,
    )
    .map_err(SunfishQcacldWmiInitCeError::Frame)?;
    let ce = qcom_wmi_ce::plan_wmi_tx(
        mappings,
        &htc_frame_output[..frame.htc.frame_len],
        frame.htc,
        endpoint_id,
        transfer_id,
        dma_pool,
        buffer,
        profile,
        rings,
        cursor,
    )
    .map_err(SunfishQcacldWmiInitCeError::Ce)?;
    Ok(SunfishQcacldWmiInitCePlan {
        generation: active_generation,
        frame,
        ce,
    })
}

/// Publish one prepared WMI_INIT through the existing live CE3 Gate.
///
/// The transport generation and startup phase are checked before credits are
/// reserved. Definite pre-publication failures cancel that reservation. An
/// uncertain producer-index commit retains the credits and quarantines the
/// startup generation so this command cannot be replayed. Only confirmed CE3
/// publication advances the session to `AwaitReady`.
pub fn publish_sunfish_qcacld_wmi_init<G: Gate>(
    io: &mut NativeIo<G>,
    startup: &mut SunfishWmiStartupSession,
    active_generation: u64,
    plan: SunfishQcacldWmiInitCePlan<'_>,
    credits: &mut HtcCreditLedger,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
) -> Result<(), SunfishQcacldWmiInitPublishError> {
    submit_wmi_init_with(startup, active_generation, plan, credits, |ce| {
        io.publish_wmi_tx(active_generation, profile, rings, ce, cursor)
    })
}

fn submit_wmi_init_with(
    startup: &mut SunfishWmiStartupSession,
    active_generation: u64,
    plan: SunfishQcacldWmiInitCePlan<'_>,
    credits: &mut HtcCreditLedger,
    publish: impl FnOnce(WmiCeTxPlan<'_>) -> Result<(), TxPublishError>,
) -> Result<(), SunfishQcacldWmiInitPublishError> {
    if plan.generation != active_generation {
        return Err(SunfishQcacldWmiInitPublishError::PlanGeneration {
            planned: plan.generation,
            active: active_generation,
        });
    }
    if startup.generation() != active_generation {
        return Err(SunfishQcacldWmiInitPublishError::Startup(
            WmiStartupError::WrongGeneration {
                expected: startup.generation(),
                observed: active_generation,
            },
        ));
    }
    startup
        .require_control_endpoint(credits.endpoint_id())
        .map_err(|error| {
            SunfishQcacldWmiInitPublishError::Startup(match error {
                crate::sunfish_qcacld_wmi_rx::WmiControlEndpointError::SessionNotBound => {
                    WmiStartupError::ControlSessionNotBound
                }
                crate::sunfish_qcacld_wmi_rx::WmiControlEndpointError::Mismatch {
                    expected,
                    observed,
                } => WmiStartupError::ControlEndpointMismatch { expected, observed },
            })
        })?;
    if plan.ce.endpoint_id() != credits.endpoint_id() {
        return Err(SunfishQcacldWmiInitPublishError::Startup(
            WmiStartupError::ControlEndpointMismatch {
                expected: credits.endpoint_id(),
                observed: plan.ce.endpoint_id(),
            },
        ));
    }
    if startup.phase() != WmiStartupPhase::InitReady {
        return Err(SunfishQcacldWmiInitPublishError::Startup(
            WmiStartupError::InitSubmissionNotAllowed {
                phase: startup.phase(),
            },
        ));
    }

    let credit_plan = plan.frame.htc;
    credits
        .reserve(active_generation, credit_plan)
        .map_err(SunfishQcacldWmiInitPublishError::CreditReserve)?;

    match publish(plan.ce) {
        Ok(()) => startup
            .mark_init_submitted(active_generation)
            .map_err(SunfishQcacldWmiInitPublishError::PublishedSessionCommitFailed),
        Err(TxPublishError::Uncertain) => {
            startup
                .mark_init_submission_uncertain(active_generation)
                .map_err(SunfishQcacldWmiInitPublishError::UncertainQuarantineFailed)?;
            Err(SunfishQcacldWmiInitPublishError::PublicationUncertain)
        }
        Err(publish_error) => {
            credits
                .cancel_unpublished(active_generation, credit_plan.credits_required)
                .map_err(
                    |credit| SunfishQcacldWmiInitPublishError::CreditCancelFailed {
                        publish: publish_error,
                        credit,
                    },
                )?;
            Err(SunfishQcacldWmiInitPublishError::Publish(publish_error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcacld_wmi::QcacldPeerLimit;
    use crate::qcom_htc::{HtcCreditLedger, HtcReady};
    use crate::qcom_wmi_ce::{Direction, WMI_CONTROL_SERVICE_ID};
    use crate::sunfish_qcacld_htc::HttStartupSession;
    use crate::sunfish_qcacld_resource::SunfishQcacldResourceInputs;
    use crate::sunfish_qcacld_wmi_rx::{
        WMI_SERVICE_AVAILABLE_EVENT_ID, WMI_SERVICE_READY_EVENT_ID, receive_wmi_event,
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
        let response = [
            0, 0, 12, 0, 0, 0, 0, 0, 3, 0, 0, 3, 0, ENDPOINT, 0, 2, 0, 0, 0, 0,
        ];
        htt.accept_connect_response_frame(GENERATION, &response)
            .unwrap();
        htt
    }

    fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn append_tlv(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
        bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(value);
    }

    fn service_ready_body() -> Vec<u8> {
        let mut fixed = [0u8; 128];
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
            put_word(&mut fixed, index * 4, word);
        }
        let mut body = Vec::new();
        append_tlv(&mut body, 32, &fixed);
        append_tlv(&mut body, 33, &[0; 36]);
        append_tlv(&mut body, 16, &[0; 128]);
        append_tlv(&mut body, 18, &[]);
        body
    }

    fn service_available_body() -> Vec<u8> {
        let mut fixed = [0u8; 20];
        put_word(&mut fixed, 0, 128);
        let mut body = Vec::new();
        append_tlv(&mut body, 559, &fixed);
        body
    }

    fn event_frame(event_id: u32, body: &[u8]) -> Vec<u8> {
        let declared = 4 + body.len();
        let mut frame = vec![0u8; 8 + declared];
        let header = u32::from(ENDPOINT) | ((declared as u32) << 16);
        put_word(&mut frame, 0, header);
        put_word(&mut frame, 8, event_id);
        frame[12..].copy_from_slice(body);
        frame
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

    fn init_ready_session() -> SunfishWmiStartupSession {
        init_ready_session_with_binding(true)
    }

    fn init_ready_unbound_session() -> SunfishWmiStartupSession {
        init_ready_session_with_binding(false)
    }

    fn init_ready_session_with_binding(bind_control: bool) -> SunfishWmiStartupSession {
        let mut credits = HtcCreditLedger::new(
            GENERATION,
            ENDPOINT,
            CREDIT_COUNT,
            CREDIT_SIZE,
            MAX_MESSAGE_SIZE as u16,
        )
        .unwrap();
        let mut startup = if bind_control {
            startup_after_control()
        } else {
            SunfishWmiStartupSession::new(GENERATION)
        };
        for (event_id, body) in [
            (WMI_SERVICE_READY_EVENT_ID, service_ready_body()),
            (WMI_SERVICE_AVAILABLE_EVENT_ID, service_available_body()),
        ] {
            let frame = event_frame(event_id, &body);
            let event =
                receive_wmi_event(GENERATION, ENDPOINT, MAX_MESSAGE_SIZE, &mut credits, &frame)
                    .unwrap();
            startup.observe(&event).unwrap();
        }
        assert_eq!(startup.phase(), WmiStartupPhase::InitReady);
        startup
    }

    fn plan_init<'a>(
        startup: &SunfishWmiStartupSession,
        body_output: &mut [u8; 512],
        frame_output: &'a mut [u8; 512],
    ) -> SunfishQcacldWmiInitCePlan<'a> {
        let mappings = [
            ServiceCeMapping {
                service_id: WMI_CONTROL_SERVICE_ID,
                direction: Direction::HostToTarget,
                engine: 3,
            },
            ServiceCeMapping {
                service_id: WMI_CONTROL_SERVICE_ID,
                direction: Direction::TargetToHost,
                engine: 2,
            },
        ];
        let profile = Profile::Mission;
        let dma_pool = DmaPool::new(0x8000_0000, 0x40000).unwrap();
        let rings =
            crate::qcom_ce_v3_profile::plan_rings(profile, dma_pool.base(), dma_pool.bytes())
                .unwrap();
        let buffer = TxBufferWindow {
            iova: (dma_pool.base() + rings.total_bytes() + 4095) & !4095,
            capacity: 512,
        };
        let cursor = RingCursor::new(32).unwrap();
        plan_sunfish_qcacld_wmi_init_ce_tx(
            startup,
            GENERATION,
            SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32),
            None,
            ENDPOINT,
            7,
            MAX_MESSAGE_SIZE,
            CREDIT_SIZE,
            CREDIT_COUNT,
            &mappings,
            dma_pool,
            buffer,
            profile,
            &rings,
            cursor,
            body_output,
            frame_output,
        )
        .unwrap()
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

    #[test]
    fn wmi_init_encoding_and_publication_require_bound_control_endpoint() {
        let mut unbound = init_ready_unbound_session();
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        assert_eq!(
            encode_sunfish_qcacld_wmi_init_frame_from_session(
                &unbound,
                GENERATION,
                SunfishQcacldResourceInputs::lineage_sunfish_candidate(
                    crate::qcacld_wmi::QcacldPeerLimit::OtherTarget32,
                ),
                None,
                ENDPOINT,
                MAX_MESSAGE_SIZE,
                CREDIT_SIZE,
                CREDIT_COUNT,
                &mut body,
                &mut frame,
            ),
            Err(SunfishQcacldWmiFrameError::ControlSessionNotBound)
        );

        let bound = init_ready_session();
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        let plan = plan_init(&bound, &mut body, &mut frame);
        let mut credits = credit_ledger();
        assert_eq!(
            submit_wmi_init_with(&mut unbound, GENERATION, plan, &mut credits, |_| Ok(())),
            Err(SunfishQcacldWmiInitPublishError::Startup(
                WmiStartupError::ControlSessionNotBound
            ))
        );
        assert_eq!(credits.available(), CREDIT_COUNT);
        assert_eq!(credits.in_flight(), 0);

        // Keep `bound` live so the planned frame's accepted evidence remains
        // explicit in this test, even though publication is rejected above.
        assert_eq!(bound.control_endpoint_id(), Some(ENDPOINT));
    }

    #[test]
    fn confirmed_init_publication_retains_credits_and_advances_the_session() {
        let mut startup = init_ready_session();
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        let plan = plan_init(&startup, &mut body, &mut frame);
        let required = plan.frame.htc.credits_required;
        let mut credits = credit_ledger();

        submit_wmi_init_with(&mut startup, GENERATION, plan, &mut credits, |ce| {
            assert_eq!(ce.frame_bytes(), 324);
            assert_eq!(ce.descriptor().transfer_id(), 7);
            Ok(())
        })
        .unwrap();

        assert_eq!(startup.phase(), WmiStartupPhase::AwaitReady);
        assert_eq!(credits.in_flight(), required);
        assert_eq!(credits.available(), CREDIT_COUNT - required);
    }

    #[test]
    fn definite_init_publish_failure_cancels_credits_without_advancing() {
        let mut startup = init_ready_session();
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        let plan = plan_init(&startup, &mut body, &mut frame);
        let mut credits = credit_ledger();

        assert_eq!(
            submit_wmi_init_with(&mut startup, GENERATION, plan, &mut credits, |_| {
                Err(TxPublishError::LeaseLost)
            }),
            Err(SunfishQcacldWmiInitPublishError::Publish(
                TxPublishError::LeaseLost
            ))
        );
        assert_eq!(startup.phase(), WmiStartupPhase::InitReady);
        assert_eq!(credits.in_flight(), 0);
        assert_eq!(credits.available(), CREDIT_COUNT);
    }

    #[test]
    fn uncertain_init_publish_quarantines_generation_and_keeps_credits() {
        let mut startup = init_ready_session();
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        let plan = plan_init(&startup, &mut body, &mut frame);
        let required = plan.frame.htc.credits_required;
        let mut credits = credit_ledger();

        assert_eq!(
            submit_wmi_init_with(&mut startup, GENERATION, plan, &mut credits, |_| {
                Err(TxPublishError::Uncertain)
            }),
            Err(SunfishQcacldWmiInitPublishError::PublicationUncertain)
        );
        assert_eq!(startup.phase(), WmiStartupPhase::InitSubmissionUncertain);
        assert_eq!(credits.in_flight(), required);
        assert_eq!(
            startup.mark_init_submitted(GENERATION),
            Err(WmiStartupError::InitSubmissionNotAllowed {
                phase: WmiStartupPhase::InitSubmissionUncertain,
            })
        );
        startup.advance_generation(GENERATION + 1).unwrap();
        assert_eq!(startup.phase(), WmiStartupPhase::AwaitServiceReady);
    }

    #[test]
    fn stale_plan_generation_is_rejected_before_credit_reservation_or_publish() {
        let mut startup = init_ready_session();
        let mut body = [0u8; 512];
        let mut frame = [0u8; 512];
        let plan = plan_init(&startup, &mut body, &mut frame);
        let mut credits = credit_ledger();
        let mut called = false;

        assert_eq!(
            submit_wmi_init_with(&mut startup, GENERATION + 1, plan, &mut credits, |_| {
                called = true;
                Ok(())
            }),
            Err(SunfishQcacldWmiInitPublishError::PlanGeneration {
                planned: GENERATION,
                active: GENERATION + 1,
            })
        );
        assert!(!called);
        assert_eq!(credits.in_flight(), 0);
        assert_eq!(startup.phase(), WmiStartupPhase::InitReady);
    }
}
