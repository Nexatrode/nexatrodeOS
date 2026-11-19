//! Generation-bound HTC control connection for the Sunfish WMI service.
//!
//! This protocol session stages only the QCACLD WMI_CONTROL_SVC connect and
//! HTC_SETUP_COMPLETE_EX frames. It has no DMA, MMIO, IRQ, or device authority.
#![forbid(unsafe_code)]

use super::{HttEndpoint, HttStartupPhase, HttStartupSession};
use crate::qcom_htc::{
    self, ConnectServiceResponse, HTC_CONTROL_ENDPOINT, HTC_FRAME_HEADER_SIZE,
    HTC_WMI_CONTROL_SERVICE_ID, HtcCreditLedger, HtcError, HtcStartup,
};

const HTC_CONNECT_SERVICE_REQUEST_BYTES: usize = 8;
const HTC_SETUP_COMPLETE_EX_BYTES: usize = 12;
const WMI_CONTROL_CONNECT_FRAME_BYTES: usize =
    HTC_FRAME_HEADER_SIZE + HTC_CONNECT_SERVICE_REQUEST_BYTES;
const HTC_SETUP_COMPLETE_FRAME_BYTES: usize = HTC_FRAME_HEADER_SIZE + HTC_SETUP_COMPLETE_EX_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiControlPhase {
    Ready,
    ConnectStaged,
    AwaitConnectResponse,
    SubmissionUncertain,
    ServiceConnected,
    SetupStaged,
    Started,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiControlSessionError {
    InvalidGeneration,
    WrongGeneration {
        expected: u64,
        observed: u64,
    },
    WrongHttGeneration {
        expected: u64,
        observed: u64,
    },
    HttNotConnected {
        observed: HttStartupPhase,
    },
    HttEndpointUnavailable,
    ReadyPayloadMismatch,
    InvalidState {
        expected: WmiControlPhase,
        observed: WmiControlPhase,
    },
    Htc(HtcError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiControlEndpoint {
    endpoint_id: u8,
    max_message_size: u16,
}

impl WmiControlEndpoint {
    pub const fn endpoint_id(self) -> u8 {
        self.endpoint_id
    }

    pub const fn max_message_size(self) -> u16 {
        self.max_message_size
    }
}

/// Generation-bound HTC control handshake for WMI_CONTROL_SVC. It models
/// QCACLD's WMI connect, response, and later HTC setup-complete ordering; only
/// its staged canonical frames can enter the native CE0 publisher.
#[derive(Debug, Eq, PartialEq)]
pub struct SunfishWmiControlSession {
    generation: u64,
    htt_endpoint: HttEndpoint,
    startup: HtcStartup,
    phase: WmiControlPhase,
    endpoint: Option<WmiControlEndpoint>,
    staged_frame: [u8; HTC_SETUP_COMPLETE_FRAME_BYTES],
    staged_frame_len: u8,
}

impl SunfishWmiControlSession {
    /// Begin the WMI control handshake only after QCACLD's earlier HTT service
    /// connection has completed on this same transport generation.
    pub fn after_htt_connected(
        generation: u64,
        ready_payload: &[u8],
        htt: &HttStartupSession,
    ) -> Result<Self, WmiControlSessionError> {
        if generation == 0 {
            return Err(WmiControlSessionError::InvalidGeneration);
        }
        if htt.generation() != generation {
            return Err(WmiControlSessionError::WrongHttGeneration {
                expected: htt.generation(),
                observed: generation,
            });
        }
        if htt.phase() != HttStartupPhase::Connected {
            return Err(WmiControlSessionError::HttNotConnected {
                observed: htt.phase(),
            });
        }
        let htt_endpoint = htt
            .endpoint()
            .ok_or(WmiControlSessionError::HttEndpointUnavailable)?;
        let observed_ready =
            qcom_htc::parse_ready_ex(ready_payload).map_err(WmiControlSessionError::Htc)?;
        if observed_ready != htt.ready() {
            return Err(WmiControlSessionError::ReadyPayloadMismatch);
        }
        let mut startup = HtcStartup::new();
        startup
            .accept_ready(ready_payload)
            .map_err(WmiControlSessionError::Htc)?;
        Ok(Self {
            generation,
            htt_endpoint,
            startup,
            phase: WmiControlPhase::Ready,
            endpoint: None,
            staged_frame: [0; HTC_SETUP_COMPLETE_FRAME_BYTES],
            staged_frame_len: 0,
        })
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn htt_endpoint(&self) -> HttEndpoint {
        self.htt_endpoint
    }

    pub const fn phase(&self) -> WmiControlPhase {
        self.phase
    }

    pub const fn endpoint(&self) -> Option<WmiControlEndpoint> {
        self.endpoint
    }

    pub fn wmi_credit_ledger(
        &self,
        generation: u64,
    ) -> Result<HtcCreditLedger, WmiControlSessionError> {
        self.require_generation(generation)?;
        self.require_phase(WmiControlPhase::Started)?;
        self.startup
            .wmi_credit_ledger(generation)
            .map_err(WmiControlSessionError::Htc)
    }

    pub fn stage_connect_request(
        &mut self,
        generation: u64,
        output: &mut [u8],
    ) -> Result<usize, WmiControlSessionError> {
        self.require_generation(generation)?;
        self.require_phase(WmiControlPhase::Ready)?;
        if output.len() < WMI_CONTROL_CONNECT_FRAME_BYTES {
            return Err(WmiControlSessionError::Htc(HtcError::OutputTooSmall {
                required: WMI_CONTROL_CONNECT_FRAME_BYTES,
                available: output.len(),
            }));
        }

        let mut payload = [0u8; HTC_CONNECT_SERVICE_REQUEST_BYTES];
        let payload_len = self
            .startup
            .stage_wmi_connect(&mut payload)
            .map_err(WmiControlSessionError::Htc)?;
        let mut frame = [0u8; WMI_CONTROL_CONNECT_FRAME_BYTES];
        let frame_len = qcom_htc::encode_control_frame(&payload[..payload_len], &mut frame)
            .map_err(WmiControlSessionError::Htc)?;
        output[..frame_len].copy_from_slice(&frame[..frame_len]);
        self.staged_frame[..frame_len].copy_from_slice(&frame[..frame_len]);
        self.staged_frame_len = frame_len as u8;
        self.phase = WmiControlPhase::ConnectStaged;
        Ok(frame_len)
    }

    pub fn staged_control_frame(&self, generation: u64) -> Result<&[u8], WmiControlSessionError> {
        self.require_generation(generation)?;
        if !matches!(
            self.phase,
            WmiControlPhase::ConnectStaged | WmiControlPhase::SetupStaged
        ) {
            return Err(WmiControlSessionError::InvalidState {
                expected: WmiControlPhase::ConnectStaged,
                observed: self.phase,
            });
        }
        let expected = match self.phase {
            WmiControlPhase::ConnectStaged => WMI_CONTROL_CONNECT_FRAME_BYTES,
            WmiControlPhase::SetupStaged => HTC_SETUP_COMPLETE_FRAME_BYTES,
            _ => unreachable!(),
        };
        if usize::from(self.staged_frame_len) != expected {
            return Err(WmiControlSessionError::Htc(HtcError::UnexpectedLength {
                expected,
                observed: usize::from(self.staged_frame_len),
            }));
        }
        Ok(&self.staged_frame[..expected])
    }

    pub fn control_tx_submitted(
        &mut self,
        generation: u64,
    ) -> Result<WmiControlPhase, WmiControlSessionError> {
        self.require_generation(generation)?;
        let next = match self.phase {
            WmiControlPhase::ConnectStaged => {
                self.startup
                    .control_tx_submitted()
                    .map_err(WmiControlSessionError::Htc)?;
                WmiControlPhase::AwaitConnectResponse
            }
            WmiControlPhase::SetupStaged => {
                self.startup
                    .control_tx_submitted()
                    .map_err(WmiControlSessionError::Htc)?;
                WmiControlPhase::Started
            }
            observed => {
                return Err(WmiControlSessionError::InvalidState {
                    expected: WmiControlPhase::ConnectStaged,
                    observed,
                });
            }
        };
        self.phase = next;
        Ok(next)
    }

    pub fn control_tx_failed(
        &mut self,
        generation: u64,
    ) -> Result<WmiControlPhase, WmiControlSessionError> {
        self.require_generation(generation)?;
        let next = match self.phase {
            WmiControlPhase::ConnectStaged => {
                self.startup
                    .control_tx_failed()
                    .map_err(WmiControlSessionError::Htc)?;
                WmiControlPhase::Ready
            }
            WmiControlPhase::SetupStaged => {
                self.startup
                    .control_tx_failed()
                    .map_err(WmiControlSessionError::Htc)?;
                WmiControlPhase::ServiceConnected
            }
            observed => {
                return Err(WmiControlSessionError::InvalidState {
                    expected: WmiControlPhase::ConnectStaged,
                    observed,
                });
            }
        };
        self.phase = next;
        self.staged_frame_len = 0;
        Ok(next)
    }

    pub fn control_tx_uncertain(&mut self, generation: u64) -> Result<(), WmiControlSessionError> {
        self.require_generation(generation)?;
        if !matches!(
            self.phase,
            WmiControlPhase::ConnectStaged | WmiControlPhase::SetupStaged
        ) {
            return Err(WmiControlSessionError::InvalidState {
                expected: WmiControlPhase::ConnectStaged,
                observed: self.phase,
            });
        }
        self.phase = WmiControlPhase::SubmissionUncertain;
        Ok(())
    }

    pub fn accept_connect_response_frame(
        &mut self,
        generation: u64,
        frame: &[u8],
    ) -> Result<WmiControlEndpoint, WmiControlSessionError> {
        self.require_generation(generation)?;
        self.require_phase(WmiControlPhase::AwaitConnectResponse)?;
        let response: ConnectServiceResponse<'_> = self
            .startup
            .accept_connect_response_frame(frame)
            .map_err(WmiControlSessionError::Htc)?;
        if response.service_id != HTC_WMI_CONTROL_SERVICE_ID
            || response.endpoint_id == HTC_CONTROL_ENDPOINT
        {
            return Err(WmiControlSessionError::Htc(HtcError::WrongServiceId {
                expected: HTC_WMI_CONTROL_SERVICE_ID,
                observed: response.service_id,
            }));
        }
        let endpoint = WmiControlEndpoint {
            endpoint_id: response.endpoint_id,
            max_message_size: response.max_message_size,
        };
        self.endpoint = Some(endpoint);
        self.phase = WmiControlPhase::ServiceConnected;
        Ok(endpoint)
    }

    pub fn stage_setup_complete(
        &mut self,
        generation: u64,
        output: &mut [u8],
    ) -> Result<usize, WmiControlSessionError> {
        self.require_generation(generation)?;
        self.require_phase(WmiControlPhase::ServiceConnected)?;
        if output.len() < HTC_SETUP_COMPLETE_FRAME_BYTES {
            return Err(WmiControlSessionError::Htc(HtcError::OutputTooSmall {
                required: HTC_SETUP_COMPLETE_FRAME_BYTES,
                available: output.len(),
            }));
        }
        let mut payload = [0u8; HTC_SETUP_COMPLETE_EX_BYTES];
        let payload_len = self
            .startup
            .stage_setup_complete(&mut payload)
            .map_err(WmiControlSessionError::Htc)?;
        let mut frame = [0u8; HTC_SETUP_COMPLETE_FRAME_BYTES];
        let frame_len = qcom_htc::encode_control_frame(&payload[..payload_len], &mut frame)
            .map_err(WmiControlSessionError::Htc)?;
        output[..frame_len].copy_from_slice(&frame[..frame_len]);
        self.staged_frame[..frame_len].copy_from_slice(&frame[..frame_len]);
        self.staged_frame_len = frame_len as u8;
        self.phase = WmiControlPhase::SetupStaged;
        Ok(frame_len)
    }

    fn require_generation(&self, observed: u64) -> Result<(), WmiControlSessionError> {
        if observed == self.generation {
            Ok(())
        } else {
            Err(WmiControlSessionError::WrongGeneration {
                expected: self.generation,
                observed,
            })
        }
    }

    fn require_phase(&self, expected: WmiControlPhase) -> Result<(), WmiControlSessionError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(WmiControlSessionError::InvalidState {
                expected,
                observed: self.phase,
            })
        }
    }
}
