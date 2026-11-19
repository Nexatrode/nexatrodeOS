//! Source-matched HTC binding and data framing for Sunfish QCACLD HTT.
//!
//! The HTT endpoint is assigned dynamically by the target. This module only
//! encodes the connect request, validates its response, and frames caller-owned
//! HTT bytes. It does not publish CE descriptors, allocate/map DMA, or access
//! WLAN hardware.
#![forbid(unsafe_code)]

#[path = "sunfish_qcacld_wmi_control.rs"]
pub mod wmi_control;

use crate::qcom_htc::{
    self, ConnectServiceRequest, ConnectServiceResponse, HTC_CONTROL_ENDPOINT,
    HTC_FRAME_HEADER_SIZE, HTC_MAX_FRAME_PAYLOAD, HtcCreditReportTotals, HtcError, HtcReady,
};

pub const HTT_DATA_SERVICE_ID: u16 = 0x0300;
pub const HTC_CONNECT_DISABLE_CREDIT_FLOW_CTRL: u16 = 1 << 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttStartupPhase {
    AwaitRequest,
    RequestStaged,
    AwaitResponse,
    SubmissionUncertain,
    Connected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttStartupError {
    InvalidGeneration,
    WrongGeneration {
        expected: u64,
        observed: u64,
    },
    InvalidState {
        expected: HttStartupPhase,
        observed: HttStartupPhase,
    },
    Htc(HtcError),
}

/// Generation-bound state for the initial HTT_DATA_MSG_SVC connection. The
/// pinned QCACLD attach path connects this HTC service before attaching the
/// HTT receive path; the target-selected endpoint is retained only after a
/// valid control-endpoint response has been received.
#[derive(Debug, Eq, PartialEq)]
pub struct HttStartupSession {
    generation: u64,
    ready: HtcReady,
    phase: HttStartupPhase,
    endpoint: Option<HttEndpoint>,
    request_frame: [u8; HTC_FRAME_HEADER_SIZE + 8],
    request_frame_len: u8,
}

impl HttStartupSession {
    pub fn new(generation: u64, ready: HtcReady) -> Result<Self, HttStartupError> {
        if generation == 0 {
            return Err(HttStartupError::InvalidGeneration);
        }
        if ready.credit_count == 0 {
            return Err(HttStartupError::Htc(HtcError::ZeroCreditCount));
        }
        if ready.credit_size == 0 {
            return Err(HttStartupError::Htc(HtcError::ZeroCreditSize));
        }
        if !(2..=qcom_htc::HTC_LOCAL_ENDPOINT_LIMIT).contains(&ready.max_endpoints) {
            return Err(HttStartupError::Htc(HtcError::InvalidEndpointLimit {
                observed: ready.max_endpoints,
            }));
        }
        if ready.htc_version > 1 {
            return Err(HttStartupError::Htc(HtcError::UnsupportedHtcVersion {
                observed: ready.htc_version,
            }));
        }
        Ok(Self {
            generation,
            ready,
            phase: HttStartupPhase::AwaitRequest,
            endpoint: None,
            request_frame: [0; HTC_FRAME_HEADER_SIZE + 8],
            request_frame_len: 0,
        })
    }

    pub fn from_ready_payload(
        generation: u64,
        ready_payload: &[u8],
    ) -> Result<Self, HttStartupError> {
        let ready = qcom_htc::parse_ready_ex(ready_payload).map_err(HttStartupError::Htc)?;
        Self::new(generation, ready)
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn ready(&self) -> HtcReady {
        self.ready
    }

    pub const fn phase(&self) -> HttStartupPhase {
        self.phase
    }

    pub const fn endpoint(&self) -> Option<HttEndpoint> {
        self.endpoint
    }

    /// Encode the source-matched HTT connect request inside an HTC control
    /// frame. State changes only after both bounded encoders succeed.
    pub fn stage_connect_request(
        &mut self,
        generation: u64,
        output: &mut [u8],
    ) -> Result<usize, HttStartupError> {
        self.require_generation(generation)?;
        self.require_phase(HttStartupPhase::AwaitRequest)?;
        let mut request = [0u8; 8];
        let request_len = encode_connect_request(&mut request).map_err(HttStartupError::Htc)?;
        let mut frame = [0u8; HTC_FRAME_HEADER_SIZE + 8];
        let frame_len = qcom_htc::encode_control_frame(&request[..request_len], &mut frame)
            .map_err(HttStartupError::Htc)?;
        if output.len() < frame_len {
            return Err(HttStartupError::Htc(HtcError::OutputTooSmall {
                required: frame_len,
                available: output.len(),
            }));
        }
        output[..frame_len].copy_from_slice(&frame[..frame_len]);
        self.request_frame = frame;
        self.request_frame_len = u8::try_from(frame_len).map_err(|_| {
            HttStartupError::Htc(HtcError::MessageTooLarge {
                limit: u8::MAX as usize,
                observed: frame_len,
            })
        })?;
        self.phase = HttStartupPhase::RequestStaged;
        Ok(frame_len)
    }

    /// Borrow the one staged request frame for the native CE0 publisher.
    /// The frame remains owned by this generation-bound session.
    pub fn staged_connect_request_frame(&self, generation: u64) -> Result<&[u8], HttStartupError> {
        self.require_generation(generation)?;
        self.require_phase(HttStartupPhase::RequestStaged)?;
        if usize::from(self.request_frame_len) != HTC_FRAME_HEADER_SIZE + 8 {
            return Err(HttStartupError::Htc(HtcError::UnexpectedLength {
                expected: HTC_FRAME_HEADER_SIZE + 8,
                observed: usize::from(self.request_frame_len),
            }));
        }
        Ok(&self.request_frame[..usize::from(self.request_frame_len)])
    }

    /// Confirm that CE accepted the staged control frame. A failed transport
    /// must call `request_tx_failed` so the transaction cannot be mistaken for
    /// an in-flight request.
    pub fn request_tx_submitted(&mut self, generation: u64) -> Result<(), HttStartupError> {
        self.require_generation(generation)?;
        self.require_phase(HttStartupPhase::RequestStaged)?;
        self.phase = HttStartupPhase::AwaitResponse;
        Ok(())
    }

    pub fn request_tx_failed(&mut self, generation: u64) -> Result<(), HttStartupError> {
        self.require_generation(generation)?;
        self.require_phase(HttStartupPhase::RequestStaged)?;
        self.phase = HttStartupPhase::AwaitRequest;
        Ok(())
    }

    /// Permanently quarantine this startup transaction when CE publication
    /// may have reached hardware but software cannot confirm cursor commit.
    pub fn request_tx_uncertain(&mut self, generation: u64) -> Result<(), HttStartupError> {
        self.require_generation(generation)?;
        self.require_phase(HttStartupPhase::RequestStaged)?;
        self.phase = HttStartupPhase::SubmissionUncertain;
        Ok(())
    }

    /// Accept exactly one response for the submitted HTT connection request.
    /// Malformed, wrong-service, or out-of-range responses do not bind an
    /// endpoint or advance the transaction.
    pub fn accept_connect_response_frame(
        &mut self,
        generation: u64,
        frame: &[u8],
    ) -> Result<HttEndpoint, HttStartupError> {
        self.require_generation(generation)?;
        self.require_phase(HttStartupPhase::AwaitResponse)?;
        let control = qcom_htc::parse_plain_frame(frame).map_err(HttStartupError::Htc)?;
        if control.endpoint_id != HTC_CONTROL_ENDPOINT {
            return Err(HttStartupError::Htc(HtcError::InvalidFrameEndpoint {
                observed: control.endpoint_id,
            }));
        }
        let endpoint =
            parse_connect_response(control.payload, self.ready).map_err(HttStartupError::Htc)?;
        self.endpoint = Some(endpoint);
        self.phase = HttStartupPhase::Connected;
        Ok(endpoint)
    }

    fn require_generation(&self, observed: u64) -> Result<(), HttStartupError> {
        if self.generation == observed {
            Ok(())
        } else {
            Err(HttStartupError::WrongGeneration {
                expected: self.generation,
                observed,
            })
        }
    }

    fn require_phase(&self, expected: HttStartupPhase) -> Result<(), HttStartupError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(HttStartupError::InvalidState {
                expected,
                observed: self.phase,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttEndpoint {
    endpoint_id: u8,
    max_message_size: u16,
}

impl HttEndpoint {
    pub const fn endpoint_id(self) -> u8 {
        self.endpoint_id
    }

    pub const fn max_message_size(self) -> usize {
        self.max_message_size as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataFramePlan {
    pub frame_len: usize,
    pub payload_len: usize,
    pub sequence: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceivedDataFrame<'a> {
    pub sequence: u8,
    pub htt_payload: &'a [u8],
    pub credit_returns: HtcCreditReportTotals,
}

/// Encode the Sunfish QCACLD HTT service connection. The source disables HTC
/// credit-flow control for this service and obtains the endpoint from the
/// target's connect response.
pub fn encode_connect_request(output: &mut [u8]) -> Result<usize, HtcError> {
    qcom_htc::encode_connect_service_request(
        ConnectServiceRequest {
            service_id: HTT_DATA_SERVICE_ID,
            connection_flags: HTC_CONNECT_DISABLE_CREDIT_FLOW_CTRL,
            look_ahead_v2: false,
            metadata: &[],
        },
        output,
    )
}

/// Parse the target-assigned HTT endpoint. The returned handle cannot be
/// created from an unrelated service response or an invalid endpoint ID.
pub fn parse_connect_response(payload: &[u8], ready: HtcReady) -> Result<HttEndpoint, HtcError> {
    let response = qcom_htc::parse_connect_service_response(payload, HTT_DATA_SERVICE_ID, ready)?;
    endpoint_from_response(response)
}

fn endpoint_from_response(response: ConnectServiceResponse<'_>) -> Result<HttEndpoint, HtcError> {
    if response.service_id != HTT_DATA_SERVICE_ID {
        return Err(HtcError::WrongServiceId {
            expected: HTT_DATA_SERVICE_ID,
            observed: response.service_id,
        });
    }
    if response.endpoint_id == HTC_CONTROL_ENDPOINT {
        return Err(HtcError::InvalidEndpoint {
            observed: response.endpoint_id,
        });
    }
    if response.max_message_size == 0
        || usize::from(response.max_message_size) > HTC_MAX_FRAME_PAYLOAD
    {
        return Err(HtcError::InvalidMaxMessageSize {
            observed: usize::from(response.max_message_size),
        });
    }
    Ok(HttEndpoint {
        endpoint_id: response.endpoint_id,
        max_message_size: response.max_message_size,
    })
}

/// Encode an HTC data frame on the dynamically assigned HTT endpoint.
/// `payload` is the HTT byte prefix selected for this download; the complete
/// MSDU remains in its separate QCACLD DMA fragment and is not flattened here.
pub fn encode_data_frame(
    endpoint: HttEndpoint,
    sequence: u8,
    payload: &[u8],
    output: &mut [u8],
) -> Result<DataFramePlan, HtcError> {
    if endpoint.endpoint_id == HTC_CONTROL_ENDPOINT {
        return Err(HtcError::InvalidEndpoint {
            observed: endpoint.endpoint_id,
        });
    }
    if endpoint.endpoint_id >= qcom_htc::HTC_LOCAL_ENDPOINT_LIMIT {
        return Err(HtcError::InvalidFrameEndpoint {
            observed: endpoint.endpoint_id,
        });
    }
    if payload.is_empty() {
        return Err(HtcError::Truncated {
            expected: 1,
            observed: 0,
        });
    }
    if payload.len() > endpoint.max_message_size() || payload.len() > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::MessageTooLarge {
            limit: endpoint.max_message_size().min(HTC_MAX_FRAME_PAYLOAD),
            observed: payload.len(),
        });
    }
    let frame_len = HTC_FRAME_HEADER_SIZE + payload.len();
    if output.len() < frame_len {
        return Err(HtcError::OutputTooSmall {
            required: frame_len,
            available: output.len(),
        });
    }

    let first = u32::from(endpoint.endpoint_id) | ((payload.len() as u32) << 16);
    output[..4].copy_from_slice(&first.to_le_bytes());
    // QCACLD puts the per-endpoint sequence number in ControlBytes1.
    output[4..8].copy_from_slice(&(u32::from(sequence) << 8).to_le_bytes());
    output[HTC_FRAME_HEADER_SIZE..frame_len].copy_from_slice(payload);

    Ok(DataFramePlan {
        frame_len,
        payload_len: payload.len(),
        sequence,
    })
}

/// Parse one received HTC data frame on the negotiated HTT endpoint. Unlike
/// the startup-control parser, the data frame's ControlBytes1 carries its
/// endpoint sequence number, so it must not be treated as reserved zero.
/// HTC bundling, sequence-check, CRC-check, and padding flags are unsupported.
/// Receive credit reports use the HTC trailer; receive flag bit 0 is not a
/// credit request and is rejected because that bundle mode is unsupported.
pub fn parse_data_frame<'a>(
    endpoint: HttEndpoint,
    bytes: &'a [u8],
) -> Result<ReceivedDataFrame<'a>, HtcError> {
    if endpoint.endpoint_id == HTC_CONTROL_ENDPOINT
        || endpoint.endpoint_id >= qcom_htc::HTC_LOCAL_ENDPOINT_LIMIT
    {
        return Err(HtcError::InvalidEndpoint {
            observed: endpoint.endpoint_id,
        });
    }

    let frame =
        qcom_htc::parse_rx_data_frame(bytes, endpoint.endpoint_id, endpoint.max_message_size())?;
    Ok(ReceivedDataFrame {
        sequence: frame.control_bytes1,
        htt_payload: frame.payload,
        credit_returns: frame.credit_returns(),
    })
}
