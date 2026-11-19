//! One-flight WLFW startup client over bounded QRTR/QMI bytes.
//!
//! This module validates one in-flight client transaction at a time and keeps
//! the ath10k BDF/calibration flow separate from ICNSS's default rejuvenation,
//! dynamic-feature-mask, and native WLAN enable path. It does not own GLINK,
//! peer lifecycle, power, SCM, SMMU, firmware loading, or radio state.
//! Production BDF token construction remains disabled until verified Sunfish
//! board-data bytes are matched to the live capability.
#![forbid(unsafe_code)]

use crate::qcom_msa::{self, HandoffPlan, MsaReadyPermit, Reservation as MsaReservation};
use crate::qcom_qmi_router::Address;
use crate::qcom_qrtr_control::{self as control, Server};
use crate::qcom_wlfw_bdf::{self, VerifiedBdfImage};
use crate::wlfw_protocol as protocol;

pub const QRTR_HEADER_BYTES: usize = 32;
/// Pinned ICNSS `ICNSS_DEFAULT_FEATURE_MASK`; best-effort capability policy.
pub const ICNSS_DEFAULT_DYNAMIC_FEATURE_MASK: u64 = 0x01;
// A maximum BDF_DOWNLOAD QMI request uses 4,064 bytes and fills one bounded
// QRTR datagram. Capability replies remain much smaller.
pub const MAX_QMI_BYTES: usize = 4064;
pub const MAX_DATAGRAM_BYTES: usize = QRTR_HEADER_BYTES + ((MAX_QMI_BYTES + 3) & !3);
const SMALL_QMI_BYTES: usize = 214;
const WLAN_CFG_QMI_BYTES: usize = 7 + protocol::WLFW_WLAN_CFG_MAX_PAYLOAD_BYTES;
const QRTR_VERSION_V1: u32 = 1;
const QRTR_PACKET_DATA: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    StaleGeneration,
    InvalidLocalAddress,
    InvalidServer,
    EndpointConflict,
    NoEndpoint,
    Busy,
    AlreadyRegistered,
    Failed,
    TransactionExhausted,
    OutputTooSmall,
    InvalidQrtr,
    WrongEndpoint,
    NoPendingRequest,
    MsaReadyPermitMismatch,
    BdfPrerequisite,
    BdfImageMismatch,
    CalReportPrerequisite,
    DynamicFeatureMaskPrerequisite,
    PowerPrerequisite,
    FirmwareNotReady,
    WlanConfigPrerequisite,
    WlanModePrerequisite,
    FirmwareAlreadyReady,
    ServerAlreadyRegistered,
    MsaPolicy(qcom_msa::Error),
    Protocol(protocol::ProtocolError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Searching,
    ReadyToRegister,
    RegistrationPending,
    IcnssAlreadyRegistered,
    MsaInfoReady,
    MsaInfoPending,
    MsaOwnershipRequired,
    MsaReadyPending,
    CapabilityReady,
    CapabilityPending,
    CapabilitiesAccepted,
    BdfPending,
    BdfReady,
    BdfComplete,
    CalReportPending,
    CalReportAccepted,
    DynamicFeatureMaskPending,
    WlanConfigPending,
    WlanModeReady,
    WlanModePending,
    WlanModeAccepted,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    generation: u64,
    endpoint: Address,
    transaction_id: u16,
    message_id: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BdfProgress {
    generation: u64,
    identity: u64,
    asset_id: u64,
    board_id: u32,
    total_bytes: usize,
    next_offset: usize,
    next_segment_id: u32,
    pending_bytes: usize,
    pending_end: bool,
    accepted_segments: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundRequest {
    pub generation: u64,
    pub source: Address,
    pub destination: Address,
    pub transaction_id: u16,
    pub message_id: u16,
    pub bytes: [u8; MAX_DATAGRAM_BYTES],
    pub len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationOutcome {
    Accepted {
        firmware_status: Option<u64>,
        confirm_rx: bool,
    },
    Rejected {
        result: u16,
        error: u16,
        confirm_rx: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsaInfoOutcome {
    pub result: u16,
    pub error: u16,
    pub regions: [protocol::MsaRegion; protocol::WLFW_MAX_MSA_REGIONS],
    pub region_count: usize,
    pub confirm_rx: bool,
    pub handoff_plan: Option<HandoffPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsaReadyOutcome {
    pub result: u16,
    pub error: u16,
    pub confirm_rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndicationOutcome {
    pub indication: protocol::WlfwIndication,
    pub confirm_rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityOutcome<'a> {
    pub details: protocol::WlfwCapabilityResponse<'a>,
    pub confirm_rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilitySummary {
    pub result: u16,
    pub error: u16,
    pub confirm_rx: bool,
    pub chip_info: Option<(u32, u32)>,
    pub board_id: Option<u32>,
    pub soc_id: Option<u32>,
    pub firmware_version: Option<u32>,
    pub firmware_timestamp: [u8; 32],
    pub firmware_timestamp_len: usize,
    pub firmware_build_id: [u8; 128],
    pub firmware_build_id_len: usize,
    pub number_of_macs: Option<u8>,
    pub unknown_optional_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BdfSegmentOutcome {
    pub result: u16,
    pub error: u16,
    pub confirm_rx: bool,
    pub completed: bool,
    pub bytes_accepted: usize,
    pub segments_accepted: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalReportOutcome {
    pub result: u16,
    pub error: u16,
    pub confirm_rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlanConfigOutcome {
    pub result: u16,
    pub error: u16,
    pub confirm_rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynamicFeatureMaskOutcome {
    pub result: u16,
    pub error: u16,
    pub previous_mask: Option<u64>,
    pub current_mask: Option<u64>,
    pub confirm_rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlanModeOutcome {
    pub result: u16,
    pub error: u16,
    pub confirm_rx: bool,
}

impl CapabilityOutcome<'_> {
    fn summary(self) -> CapabilitySummary {
        let mut summary = CapabilitySummary {
            result: self.details.result,
            error: self.details.error,
            confirm_rx: self.confirm_rx,
            chip_info: self.details.chip_info,
            board_id: self.details.board_id,
            soc_id: self.details.soc_id,
            firmware_version: self.details.firmware_version,
            firmware_timestamp: [0; 32],
            firmware_timestamp_len: 0,
            firmware_build_id: [0; 128],
            firmware_build_id_len: 0,
            number_of_macs: self.details.number_of_macs,
            unknown_optional_count: self.details.unknown_optional_count,
        };
        if let Some(value) = self.details.firmware_timestamp {
            summary.firmware_timestamp_len = value.len();
            summary.firmware_timestamp[..value.len()].copy_from_slice(value.as_bytes());
        }
        if let Some(value) = self.details.firmware_build_id {
            summary.firmware_build_id_len = value.len();
            summary.firmware_build_id[..value.len()].copy_from_slice(value.as_bytes());
        }
        summary
    }
}

/// State for the narrow first WLFW transaction. The local QRTR address is
/// supplied by trusted owner configuration and must be reserved separately
/// from locally advertised services.
pub struct Client {
    local: Address,
    generation: u64,
    endpoint: Option<Address>,
    pending: Option<Pending>,
    next_transaction_id: u32,
    state: State,
    icnss_registration: bool,
    outcome: Option<RegistrationOutcome>,
    msa_reservation: Option<MsaReservation>,
    msa_info_outcome: Option<MsaInfoOutcome>,
    capability_summary: Option<CapabilitySummary>,
    last_indication: Option<IndicationOutcome>,
    firmware_ready_seen: bool,
    firmware_ready_indication_seen: bool,
    msa_ready_indication_seen: bool,
    dynamic_feature_mask_done: bool,
    bdf_progress: Option<BdfProgress>,
}

fn valid_node(node: u32) -> bool {
    node < control::CONTROL_PORT
}

fn valid_port(port: u32) -> bool {
    port != 0 && port < control::CONTROL_PORT
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

impl Client {
    pub fn new(local: Address, generation: u64) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if !valid_node(local.node) || !valid_port(local.port) {
            return Err(Error::InvalidLocalAddress);
        }
        Ok(Self {
            local,
            generation,
            endpoint: None,
            pending: None,
            next_transaction_id: 1,
            state: State::Searching,
            icnss_registration: false,
            outcome: None,
            msa_reservation: None,
            msa_info_outcome: None,
            capability_summary: None,
            last_indication: None,
            firmware_ready_seen: false,
            firmware_ready_indication_seen: false,
            msa_ready_indication_seen: false,
            dynamic_feature_mask_done: false,
            bdf_progress: None,
        })
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn endpoint(&self) -> Option<Address> {
        self.endpoint
    }

    /// Trusted local QRTR address configured by the native owner.
    pub fn local_address(&self) -> Address {
        self.local
    }

    pub fn outcome(&self) -> Option<RegistrationOutcome> {
        self.outcome
    }

    pub fn msa_info_outcome(&self) -> Option<MsaInfoOutcome> {
        self.msa_info_outcome
    }

    pub fn capability_summary(&self) -> Option<CapabilitySummary> {
        self.capability_summary
    }

    pub fn dynamic_feature_mask_done(&self) -> bool {
        self.dynamic_feature_mask_done
    }

    pub fn last_indication(&self) -> Option<IndicationOutcome> {
        self.last_indication
    }

    /// True only after the bound WLFW endpoint reports FW_READY either in the
    /// registration status or as an authenticated startup indication.
    pub fn firmware_ready_seen(&self) -> bool {
        self.firmware_ready_seen
    }

    /// True only after an FW_READY indication, as required by native ICNSS
    /// before it probes QCACLD and sends WLAN_CFG/WLAN_MODE.
    pub fn firmware_ready_indication_seen(&self) -> bool {
        self.firmware_ready_indication_seen
    }

    pub fn pending_message_id(&self) -> Option<u16> {
        self.pending.map(|pending| pending.message_id)
    }

    /// Bind only a validated exact WLFW NEW_SERVER record. The caller should
    /// obtain this record through the authenticated peer controller's unique
    /// endpoint lookup; a second distinct endpoint is rejected fail-closed.
    pub fn observe_server(&mut self, generation: u64, server: Server) -> Result<bool, Error> {
        self.check_generation(generation)?;
        if server.service != control::WLFW_SERVICE_ID
            || server.instance != control::WLFW_VERSION_INSTANCE
            || !valid_node(server.address.node)
            || !valid_port(server.address.port)
        {
            return Err(Error::InvalidServer);
        }
        match self.endpoint {
            Some(current) if current == server.address => Ok(false),
            Some(_) => Err(Error::EndpointConflict),
            None => {
                self.endpoint = Some(server.address);
                self.state = State::ReadyToRegister;
                Ok(true)
            }
        }
    }

    /// Remove only the matching server. Stale removals do not invalidate a
    /// newer endpoint; a real removal cancels the outstanding transaction.
    pub fn remove_server(&mut self, generation: u64, server: Server) -> Result<bool, Error> {
        self.check_generation(generation)?;
        if self.endpoint != Some(server.address)
            || server.service != control::WLFW_SERVICE_ID
            || server.instance != control::WLFW_VERSION_INSTANCE
        {
            return Ok(false);
        }
        self.endpoint = None;
        self.pending = None;
        self.state = State::Searching;
        self.icnss_registration = false;
        self.outcome = None;
        self.msa_reservation = None;
        self.msa_info_outcome = None;
        self.capability_summary = None;
        self.last_indication = None;
        self.firmware_ready_seen = false;
        self.firmware_ready_indication_seen = false;
        self.msa_ready_indication_seen = false;
        self.dynamic_feature_mask_done = false;
        self.bdf_progress = None;
        Ok(true)
    }

    /// Prepare exactly the ICNSS-default indication registration request.
    /// Output is a QRTR-v1 data datagram containing one QMI request; its final
    /// padding bytes are zero and excluded from the QRTR payload length.
    pub fn begin_indication_registration(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.begin_indication_registration_with_rejuvenate(generation, output_capacity, false)
    }

    /// Prepare the native ICNSS registration, including its default
    /// FW_REJUVENATE subscription.
    pub fn begin_icnss_indication_registration(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.begin_indication_registration_with_rejuvenate(generation, output_capacity, true)
    }

    fn begin_indication_registration_with_rejuvenate(
        &mut self,
        generation: u64,
        output_capacity: usize,
        rejuvenate: bool,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::ReadyToRegister {
            return Err(Error::AlreadyRegistered);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len =
            protocol::encode_indication_registration_request(transaction_id, rejuvenate, &mut qmi)
                .map_err(Error::Protocol)?;
        let padded_payload = align4(qmi_len);
        let datagram_len = QRTR_HEADER_BYTES + padded_payload;
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            self.local.node,
            self.local.port,
            0,
            qmi_len as u32,
            endpoint.node,
            endpoint.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut bytes, index * 4, *value);
        }
        bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi_len].copy_from_slice(&qmi[..qmi_len]);

        // Commit only after the complete bounded datagram has been prepared.
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_IND_REGISTER_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.icnss_registration = rejuvenate;
        self.state = State::RegistrationPending;
        Ok(OutboundRequest {
            generation,
            source: self.local,
            destination: endpoint,
            transaction_id,
            message_id: protocol::WLFW_IND_REGISTER_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Consume one response addressed to this client port from the exact bound
    /// peer endpoint. Malformed, stale, unrelated, or mismatched responses
    /// leave the pending request intact so a later valid response can resolve it.
    pub fn accept_indication_registration_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<RegistrationOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation {
            return Err(Error::StaleGeneration);
        }
        if datagram.len() < QRTR_HEADER_BYTES || datagram.len() & 3 != 0 {
            return Err(Error::InvalidQrtr);
        }
        let source = Address {
            node: word(datagram, 8),
            port: word(datagram, 12),
        };
        let destination = Address {
            node: word(datagram, 24),
            port: word(datagram, 28),
        };
        if source != pending.endpoint || destination != self.local {
            return Err(Error::WrongEndpoint);
        }
        let payload_len = word(datagram, 20) as usize;
        if word(datagram, 0) != QRTR_VERSION_V1
            || word(datagram, 4) != QRTR_PACKET_DATA
            || word(datagram, 16) > 1
            || payload_len > MAX_QMI_BYTES
            || datagram.len() != QRTR_HEADER_BYTES + align4(payload_len)
        {
            return Err(Error::InvalidQrtr);
        }
        if datagram[QRTR_HEADER_BYTES + payload_len..]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(Error::InvalidQrtr);
        }
        let decoded = protocol::decode_indication_registration_details(
            &datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + payload_len],
            pending.transaction_id,
        )
        .map_err(Error::Protocol)?;
        let confirm_rx = word(datagram, 16) == 1;
        self.pending = None;
        if decoded.result == 0 {
            self.firmware_ready_seen |= decoded
                .fw_status
                .is_some_and(|status| status & protocol::WLFW_FW_READY_STATUS_BIT != 0);
            let already_registered = self.icnss_registration
                && decoded.fw_status.is_some_and(|status| {
                    status & protocol::WLFW_ALREADY_REGISTERED_STATUS_BIT != 0
                });
            self.state = if already_registered {
                State::IcnssAlreadyRegistered
            } else {
                State::MsaInfoReady
            };
            let outcome = RegistrationOutcome::Accepted {
                firmware_status: decoded.fw_status,
                confirm_rx,
            };
            self.outcome = Some(outcome);
            Ok(outcome)
        } else {
            self.state = State::Failed;
            let outcome = RegistrationOutcome::Rejected {
                result: decoded.result,
                error: decoded.error,
                confirm_rx,
            };
            self.outcome = Some(outcome);
            Ok(outcome)
        }
    }

    /// Begin the ath10k reference MSA_INFO exchange for a fixed, validated
    /// pool. This describes the reserved range; it does not transfer ownership.
    pub fn begin_msa_info(
        &mut self,
        generation: u64,
        reservation: MsaReservation,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.begin_msa_info_from(generation, reservation, output_capacity, false)
    }

    /// Begin ICNSS's MSA_INFO even when registration reports FW_READY. The
    /// pinned ICNSS path keys WLAN readiness on the FW_READY indication and
    /// continues MSA setup unless the server reports ALREADY_REGISTERED.
    pub fn begin_icnss_msa_info(
        &mut self,
        generation: u64,
        reservation: MsaReservation,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if matches!(
            self.outcome,
            Some(RegistrationOutcome::Accepted {
                firmware_status: Some(status),
                ..
            }) if status & protocol::WLFW_ALREADY_REGISTERED_STATUS_BIT != 0
        ) {
            return Err(Error::ServerAlreadyRegistered);
        }
        self.begin_msa_info_from(generation, reservation, output_capacity, true)
    }

    fn begin_msa_info_from(
        &mut self,
        generation: u64,
        reservation: MsaReservation,
        output_capacity: usize,
        continue_after_registration_fw_ready: bool,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.firmware_ready_seen && !continue_after_registration_fw_ready {
            return Err(Error::FirmwareAlreadyReady);
        }
        if self.state != State::MsaInfoReady {
            return Err(Error::AlreadyRegistered);
        }
        if reservation.generation() != generation {
            return Err(Error::StaleGeneration);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len = protocol::encode_msa_info_request(
            transaction_id,
            reservation.base(),
            reservation.bytes(),
            &mut qmi,
        )
        .map_err(Error::Protocol)?;
        let datagram_len = QRTR_HEADER_BYTES + align4(qmi_len);
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            self.local.node,
            self.local.port,
            0,
            qmi_len as u32,
            endpoint.node,
            endpoint.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut bytes, index * 4, *value);
        }
        bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi_len].copy_from_slice(&qmi[..qmi_len]);

        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_MSA_INFO_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.msa_reservation = Some(reservation);
        self.msa_info_outcome = None;
        self.state = State::MsaInfoPending;
        Ok(OutboundRequest {
            generation,
            source: self.local,
            destination: endpoint,
            transaction_id,
            message_id: protocol::WLFW_MSA_INFO_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Accept one exact MSA_INFO response, checking every successful region
    /// against the fixed reservation before exposing it to ownership policy.
    pub fn accept_msa_info_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<MsaInfoOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation {
            return Err(Error::StaleGeneration);
        }
        if pending.message_id != protocol::WLFW_MSA_INFO_MESSAGE_ID
            || self.state != State::MsaInfoPending
        {
            return Err(Error::NoPendingRequest);
        }
        if datagram.len() < QRTR_HEADER_BYTES || datagram.len() & 3 != 0 {
            return Err(Error::InvalidQrtr);
        }
        let source = Address {
            node: word(datagram, 8),
            port: word(datagram, 12),
        };
        let destination = Address {
            node: word(datagram, 24),
            port: word(datagram, 28),
        };
        if source != pending.endpoint || destination != self.local {
            return Err(Error::WrongEndpoint);
        }
        let payload_len = word(datagram, 20) as usize;
        if word(datagram, 0) != QRTR_VERSION_V1
            || word(datagram, 4) != QRTR_PACKET_DATA
            || word(datagram, 16) > 1
            || payload_len > MAX_QMI_BYTES
            || datagram.len() != QRTR_HEADER_BYTES + align4(payload_len)
        {
            return Err(Error::InvalidQrtr);
        }
        if datagram[QRTR_HEADER_BYTES + payload_len..]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(Error::InvalidQrtr);
        }
        let decoded = protocol::decode_msa_info_response(
            &datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + payload_len],
            pending.transaction_id,
        )
        .map_err(Error::Protocol)?;
        let confirm_rx = word(datagram, 16) == 1;
        let mut handoff_plan = None;
        if decoded.result == 0 {
            let pool = self.msa_reservation.ok_or(Error::NoPendingRequest)?;
            if decoded.region_count == 0
                || decoded.regions[..decoded.region_count]
                    .iter()
                    .any(|region| !pool.contains(region.address, region.size))
            {
                return Err(Error::Protocol(protocol::ProtocolError::InvalidMsaRegions));
            }
            for i in 0..decoded.region_count {
                let a = decoded.regions[i];
                let a_end = a
                    .address
                    .checked_add(u64::from(a.size))
                    .ok_or(Error::Protocol(protocol::ProtocolError::InvalidMsaRegions))?;
                for b in decoded.regions.iter().take(i) {
                    let b_end = b
                        .address
                        .checked_add(u64::from(b.size))
                        .ok_or(Error::Protocol(protocol::ProtocolError::InvalidMsaRegions))?;
                    if a.address < b_end && b.address < a_end {
                        return Err(Error::Protocol(protocol::ProtocolError::InvalidMsaRegions));
                    }
                }
            }
            let mut policy_regions = [qcom_msa::MsaRegion {
                base: 0,
                bytes: 0,
                secure: false,
            }; protocol::WLFW_MAX_MSA_REGIONS];
            for (destination, source) in policy_regions
                .iter_mut()
                .zip(decoded.regions.iter().take(decoded.region_count))
            {
                *destination = qcom_msa::MsaRegion {
                    base: source.address,
                    bytes: source.size,
                    secure: source.secure,
                };
            }
            match qcom_msa::plan_handoff(pool, &policy_regions[..decoded.region_count]) {
                Ok(plan) => handoff_plan = Some(plan),
                Err(error) => {
                    let outcome = MsaInfoOutcome {
                        result: decoded.result,
                        error: decoded.error,
                        regions: decoded.regions,
                        region_count: decoded.region_count,
                        confirm_rx,
                        handoff_plan: None,
                    };
                    self.pending = None;
                    self.state = State::Failed;
                    self.msa_info_outcome = Some(outcome);
                    return Err(Error::MsaPolicy(error));
                }
            }
        }
        let outcome = MsaInfoOutcome {
            result: decoded.result,
            error: decoded.error,
            regions: decoded.regions,
            region_count: decoded.region_count,
            confirm_rx,
            handoff_plan,
        };
        self.pending = None;
        if decoded.result == 0 {
            self.state = State::MsaOwnershipRequired;
        } else {
            self.state = State::Failed;
        }
        self.msa_info_outcome = Some(outcome);
        Ok(outcome)
    }

    /// Begin MSA_READY only with the one-shot token minted by the exact-once
    /// handoff coordinator after both planned ranges received exact successful
    /// SCM acknowledgements.
    pub fn begin_msa_ready(
        &mut self,
        generation: u64,
        permit: &mut MsaReadyPermit,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::MsaOwnershipRequired {
            return Err(Error::NoPendingRequest);
        }
        let reservation = self.msa_reservation.ok_or(Error::NoPendingRequest)?;
        let plan = self
            .msa_info_outcome
            .and_then(|outcome| outcome.handoff_plan)
            .ok_or(Error::NoPendingRequest)?;
        if permit.is_consumed()
            || permit.generation() != generation
            || permit.identity() != reservation.identity()
            || permit.generation() != plan.generation()
            || permit.identity() != plan.identity()
        {
            return Err(Error::MsaReadyPermitMismatch);
        }

        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len = protocol::encode_msa_ready_request(transaction_id, &mut qmi)
            .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_MSA_READY_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        permit.consume();
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_MSA_READY_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.state = State::MsaReadyPending;
        Ok(request)
    }

    pub fn accept_msa_ready_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<MsaReadyOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_MSA_READY_MESSAGE_ID
            || self.state != State::MsaReadyPending
        {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let decoded = protocol::decode_msa_ready_response(payload, pending.transaction_id)
            .map_err(Error::Protocol)?;
        if decoded.optional_count != 0 {
            return Err(Error::Protocol(protocol::ProtocolError::UnknownOptionalTlv));
        }
        let outcome = MsaReadyOutcome {
            result: decoded.result,
            error: decoded.error,
            confirm_rx,
        };
        self.pending = None;
        self.state = if decoded.result == 0 {
            State::CapabilityReady
        } else {
            State::Failed
        };
        Ok(outcome)
    }

    pub fn begin_capability_request(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::CapabilityReady {
            return Err(Error::NoPendingRequest);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len = protocol::encode_capability_request(transaction_id, &mut qmi)
            .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_CAP_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_CAP_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.state = State::CapabilityPending;
        Ok(request)
    }

    pub fn accept_capability_response<'a>(
        &mut self,
        generation: u64,
        datagram: &'a [u8],
    ) -> Result<CapabilityOutcome<'a>, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_CAP_MESSAGE_ID
            || self.state != State::CapabilityPending
        {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let details = protocol::decode_capability_details(payload, pending.transaction_id)
            .map_err(Error::Protocol)?;
        self.capability_summary = Some(
            CapabilityOutcome {
                details,
                confirm_rx,
            }
            .summary(),
        );
        self.pending = None;
        self.state = if details.result == 0 {
            State::CapabilitiesAccepted
        } else {
            State::Failed
        };
        Ok(CapabilityOutcome {
            details,
            confirm_rx,
        })
    }

    /// Send the optional-mask transaction used by the downstream ICNSS
    /// default profile after capability discovery. This remains separate from
    /// ath10k's BDF/calibration continuation.
    pub fn begin_dynamic_feature_mask(
        &mut self,
        generation: u64,
        mask: u64,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::CapabilitiesAccepted || self.dynamic_feature_mask_done {
            return Err(Error::DynamicFeatureMaskPrerequisite);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len = protocol::encode_dynamic_feature_mask_request(transaction_id, mask, &mut qmi)
            .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.state = State::DynamicFeatureMaskPending;
        Ok(request)
    }

    /// Consume a valid dynamic-mask response. ICNSS preserves WLAN startup
    /// progress even when this best-effort request returns a nonzero QMI status.
    pub fn accept_dynamic_feature_mask_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<DynamicFeatureMaskOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID
            || self.state != State::DynamicFeatureMaskPending
        {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let decoded =
            protocol::decode_dynamic_feature_mask_response(payload, pending.transaction_id)
                .map_err(Error::Protocol)?;
        self.pending = None;
        self.dynamic_feature_mask_done = true;
        self.state = State::CapabilitiesAccepted;
        Ok(DynamicFeatureMaskOutcome {
            result: decoded.result,
            error: decoded.error,
            previous_mask: decoded.previous_mask,
            current_mask: decoded.current_mask,
            confirm_rx,
        })
    }

    /// Complete ICNSS's best-effort mask step after the transport reports its
    /// QMI timeout. The caller must supply an observed timeout, not elapsed
    /// wall time guessed by the protocol layer.
    pub fn complete_dynamic_feature_mask_timeout(&mut self, generation: u64) -> Result<(), Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID
            || self.state != State::DynamicFeatureMaskPending
        {
            return Err(Error::NoPendingRequest);
        }
        self.pending = None;
        self.dynamic_feature_mask_done = true;
        self.state = State::CapabilitiesAccepted;
        Ok(())
    }

    /// Accept the exact empty MSA_READY or FW_READY indication from the bound
    /// WLFW server. This does not consume or replace the one-flight request.
    pub fn accept_startup_indication(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<IndicationOutcome, Error> {
        self.check_generation(generation)?;
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let (payload, confirm_rx) = self.endpoint_payload(endpoint, datagram)?;
        let outcome = IndicationOutcome {
            indication: protocol::decode_startup_indication(payload).map_err(Error::Protocol)?,
            confirm_rx,
        };
        if outcome.indication == protocol::WlfwIndication::MsaReady {
            self.msa_ready_indication_seen = true;
        } else if outcome.indication == protocol::WlfwIndication::FirmwareReady {
            self.firmware_ready_seen = true;
            self.firmware_ready_indication_seen = true;
        }
        self.last_indication = Some(outcome);
        Ok(outcome)
    }

    /// Queue the first segmented BDF_DOWNLOAD only after successful capability
    /// discovery and the exact unsolicited MSA_READY indication. The image
    /// token is opaque and must be issued by the future source verifier.
    pub fn begin_bdf_download(
        &mut self,
        generation: u64,
        image: &VerifiedBdfImage<'_>,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::CapabilitiesAccepted || !self.msa_ready_indication_seen {
            return Err(Error::BdfPrerequisite);
        }
        let board_id = self
            .capability_summary
            .and_then(|summary| summary.board_id)
            .ok_or(Error::BdfPrerequisite)?;
        let identity = self
            .msa_reservation
            .ok_or(Error::BdfPrerequisite)?
            .identity();
        if image.is_empty()
            || image.len() > qcom_wlfw_bdf::MAX_BDF_IMAGE_BYTES
            || image.board_id() != board_id
            || image.generation() != generation
            || image.identity() != identity
            || image.asset_id() == 0
        {
            return Err(Error::BdfImageMismatch);
        }
        let progress = BdfProgress {
            generation,
            identity,
            asset_id: image.asset_id(),
            board_id,
            total_bytes: image.len(),
            next_offset: 0,
            next_segment_id: 0,
            pending_bytes: 0,
            pending_end: false,
            accepted_segments: 0,
        };
        let (request, progress, pending) =
            self.build_bdf_segment(generation, image, progress, output_capacity)?;
        self.bdf_progress = Some(progress);
        self.pending = Some(pending);
        self.next_transaction_id += 1;
        self.state = State::BdfPending;
        Ok(request)
    }

    /// Queue the next segment only after the preceding segment received an
    /// exact successful response. The sealed image token must match the same
    /// device identity, capability board ID, image identity, and full length.
    pub fn continue_bdf_download(
        &mut self,
        generation: u64,
        image: &VerifiedBdfImage<'_>,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::BdfReady {
            return Err(Error::NoPendingRequest);
        }
        let progress = self.bdf_progress.ok_or(Error::BdfPrerequisite)?;
        self.validate_bdf_image(generation, image, progress)?;
        let (request, progress, pending) =
            self.build_bdf_segment(generation, image, progress, output_capacity)?;
        self.bdf_progress = Some(progress);
        self.pending = Some(pending);
        self.next_transaction_id += 1;
        self.state = State::BdfPending;
        Ok(request)
    }

    pub fn bdf_progress(&self) -> Option<(usize, u32)> {
        self.bdf_progress
            .map(|progress| (progress.next_offset, progress.accepted_segments))
    }

    pub fn accept_bdf_download_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<BdfSegmentOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID
            || self.state != State::BdfPending
        {
            return Err(Error::NoPendingRequest);
        }
        let progress = self.bdf_progress.ok_or(Error::BdfPrerequisite)?;
        if progress.pending_bytes == 0 {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let decoded = protocol::decode_bdf_download_response(payload, pending.transaction_id)
            .map_err(Error::Protocol)?;
        if decoded.optional_count != 0 {
            return Err(Error::Protocol(protocol::ProtocolError::UnknownOptionalTlv));
        }
        if decoded.result != 0 {
            self.pending = None;
            self.state = State::Failed;
            return Ok(BdfSegmentOutcome {
                result: decoded.result,
                error: decoded.error,
                confirm_rx,
                completed: false,
                bytes_accepted: progress.next_offset,
                segments_accepted: progress.accepted_segments,
            });
        }
        let mut progress = progress;
        progress.next_offset = progress
            .next_offset
            .checked_add(progress.pending_bytes)
            .ok_or(Error::BdfImageMismatch)?;
        progress.next_segment_id = progress
            .next_segment_id
            .checked_add(1)
            .ok_or(Error::BdfImageMismatch)?;
        progress.accepted_segments = progress
            .accepted_segments
            .checked_add(1)
            .ok_or(Error::BdfImageMismatch)?;
        progress.pending_bytes = 0;
        let completed = progress.pending_end;
        progress.pending_end = false;
        self.pending = None;
        self.bdf_progress = Some(progress);
        self.state = if completed {
            State::BdfComplete
        } else {
            State::BdfReady
        };
        Ok(BdfSegmentOutcome {
            result: decoded.result,
            error: decoded.error,
            confirm_rx,
            completed,
            bytes_accepted: progress.next_offset,
            segments_accepted: progress.accepted_segments,
        })
    }

    /// Report the reference client's currently empty calibration metadata list
    /// after all BDF segments have been acknowledged. Populating this list is
    /// intentionally unsupported until verified calibration data is available.
    pub fn begin_empty_cal_report(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::BdfComplete {
            return Err(Error::CalReportPrerequisite);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len = protocol::encode_empty_cal_report_request(transaction_id, &mut qmi)
            .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_CAL_REPORT_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_CAL_REPORT_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.state = State::CalReportPending;
        Ok(request)
    }

    pub fn accept_cal_report_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<CalReportOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_CAL_REPORT_MESSAGE_ID
            || self.state != State::CalReportPending
        {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let decoded = protocol::decode_cal_report_response(payload, pending.transaction_id)
            .map_err(Error::Protocol)?;
        if decoded.optional_count != 0 {
            return Err(Error::Protocol(protocol::ProtocolError::UnknownOptionalTlv));
        }
        self.pending = None;
        self.state = if decoded.result == 0 {
            State::CalReportAccepted
        } else {
            State::Failed
        };
        Ok(CalReportOutcome {
            result: decoded.result,
            error: decoded.error,
            confirm_rx,
        })
    }

    /// Begin the ath10k reference WLAN_CFG exchange after its BDF/calibration
    /// path and FW_READY indication.
    pub fn begin_wlan_config(
        &mut self,
        generation: u64,
        config: protocol::WlanConfig<'_>,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.begin_wlan_config_from(
            generation,
            config,
            output_capacity,
            State::CalReportAccepted,
            false,
        )
    }

    /// Begin the native ICNSS WLAN_CFG exchange after capability discovery,
    /// the default dynamic-feature-mask step, and FW_READY. An
    /// ALREADY_REGISTERED server skips those new-server steps and can enter
    /// here after its FW_READY indication. Neither path requires ath10k's
    /// BDF_DOWNLOAD or CAL_REPORT transactions.
    pub fn begin_icnss_wlan_config(
        &mut self,
        generation: u64,
        config: protocol::WlanConfig<'_>,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.begin_wlan_config_from(
            generation,
            config,
            output_capacity,
            State::CapabilitiesAccepted,
            true,
        )
    }

    fn begin_wlan_config_from(
        &mut self,
        generation: u64,
        config: protocol::WlanConfig<'_>,
        output_capacity: usize,
        required_state: State,
        require_dynamic_feature_mask: bool,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        let icnss_existing_session =
            require_dynamic_feature_mask && self.state == State::IcnssAlreadyRegistered;
        if self.state != required_state && !icnss_existing_session {
            return Err(Error::WlanConfigPrerequisite);
        }
        if require_dynamic_feature_mask
            && !icnss_existing_session
            && !self.dynamic_feature_mask_done
        {
            return Err(Error::DynamicFeatureMaskPrerequisite);
        }
        if require_dynamic_feature_mask && !self.firmware_ready_indication_seen {
            return Err(Error::FirmwareNotReady);
        }
        if !self.firmware_ready_seen {
            return Err(Error::FirmwareNotReady);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; WLAN_CFG_QMI_BYTES];
        let qmi_len = protocol::encode_wlan_cfg_request(transaction_id, config, &mut qmi)
            .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_WLAN_CFG_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_WLAN_CFG_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.state = State::WlanConfigPending;
        Ok(request)
    }

    pub fn accept_wlan_config_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<WlanConfigOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_WLAN_CFG_MESSAGE_ID
            || self.state != State::WlanConfigPending
        {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let decoded = protocol::decode_wlan_cfg_response(payload, pending.transaction_id)
            .map_err(Error::Protocol)?;
        if decoded.optional_count != 0 {
            return Err(Error::Protocol(protocol::ProtocolError::UnknownOptionalTlv));
        }
        self.pending = None;
        self.state = if decoded.result == 0 {
            State::WlanModeReady
        } else {
            State::Failed
        };
        Ok(WlanConfigOutcome {
            result: decoded.result,
            error: decoded.error,
            confirm_rx,
        })
    }

    /// Send the normal mission-mode request after WLAN_CFG succeeds. CE
    /// allocation and hardware initialization remain outside this client.
    pub fn begin_wlan_mode(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.state == State::Failed {
            return Err(Error::Failed);
        }
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::WlanModeReady || !self.firmware_ready_seen {
            return Err(Error::WlanModePrerequisite);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; SMALL_QMI_BYTES];
        let qmi_len = protocol::encode_wlan_mode_request(
            transaction_id,
            protocol::WlanMode::Mission,
            &mut qmi,
        )
        .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_WLAN_MODE_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        self.pending = Some(Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_WLAN_MODE_MESSAGE_ID,
        });
        self.next_transaction_id += 1;
        self.state = State::WlanModePending;
        Ok(request)
    }

    pub fn accept_wlan_mode_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<WlanModeOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation
            || pending.message_id != protocol::WLFW_WLAN_MODE_MESSAGE_ID
            || self.state != State::WlanModePending
        {
            return Err(Error::NoPendingRequest);
        }
        let (payload, confirm_rx) = self.response_payload(pending, datagram)?;
        let decoded = protocol::decode_wlan_mode_response(payload, pending.transaction_id)
            .map_err(Error::Protocol)?;
        if decoded.optional_count != 0 {
            return Err(Error::Protocol(protocol::ProtocolError::UnknownOptionalTlv));
        }
        self.pending = None;
        self.state = if decoded.result == 0 {
            State::WlanModeAccepted
        } else {
            State::Failed
        };
        Ok(WlanModeOutcome {
            result: decoded.result,
            error: decoded.error,
            confirm_rx,
        })
    }

    fn validate_bdf_image(
        &self,
        generation: u64,
        image: &VerifiedBdfImage<'_>,
        progress: BdfProgress,
    ) -> Result<(), Error> {
        if image.len() != progress.total_bytes
            || image.board_id() != progress.board_id
            || image.generation() != progress.generation
            || image.identity() != progress.identity
            || image.asset_id() != progress.asset_id
            || generation != progress.generation
        {
            return Err(Error::BdfImageMismatch);
        }
        Ok(())
    }

    fn build_bdf_segment(
        &self,
        generation: u64,
        image: &VerifiedBdfImage<'_>,
        mut progress: BdfProgress,
        output_capacity: usize,
    ) -> Result<(OutboundRequest, BdfProgress, Pending), Error> {
        self.validate_bdf_image(generation, image, progress)?;
        if progress.pending_bytes != 0 || progress.next_offset >= progress.total_bytes {
            return Err(Error::NoPendingRequest);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let data_end = core::cmp::min(
            progress.next_offset + protocol::WLFW_BDF_QRTR_MAX_DATA_BYTES,
            progress.total_bytes,
        );
        let data = &image.bytes()[progress.next_offset..data_end];
        let end = data_end == progress.total_bytes;
        let mut qmi = [0u8; MAX_QMI_BYTES];
        let qmi_len = protocol::encode_bdf_download_request(
            transaction_id,
            progress.total_bytes as u32,
            progress.next_segment_id,
            data,
            end,
            &mut qmi,
        )
        .map_err(Error::Protocol)?;
        let request = self.build_request(
            generation,
            endpoint,
            transaction_id,
            protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
            &qmi[..qmi_len],
            output_capacity,
        )?;
        progress.pending_bytes = data.len();
        progress.pending_end = end;
        let pending = Pending {
            generation,
            endpoint,
            transaction_id,
            message_id: protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
        };
        Ok((request, progress, pending))
    }

    fn build_request(
        &self,
        generation: u64,
        endpoint: Address,
        transaction_id: u16,
        message_id: u16,
        qmi: &[u8],
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        if qmi.len() > MAX_QMI_BYTES {
            return Err(Error::OutputTooSmall);
        }
        let datagram_len = QRTR_HEADER_BYTES + align4(qmi.len());
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            self.local.node,
            self.local.port,
            0,
            qmi.len() as u32,
            endpoint.node,
            endpoint.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut bytes, index * 4, *value);
        }
        bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi.len()].copy_from_slice(qmi);
        Ok(OutboundRequest {
            generation,
            source: self.local,
            destination: endpoint,
            transaction_id,
            message_id,
            bytes,
            len: datagram_len,
        })
    }

    fn response_payload<'a>(
        &self,
        pending: Pending,
        datagram: &'a [u8],
    ) -> Result<(&'a [u8], bool), Error> {
        self.endpoint_payload(pending.endpoint, datagram)
    }

    fn endpoint_payload<'a>(
        &self,
        endpoint: Address,
        datagram: &'a [u8],
    ) -> Result<(&'a [u8], bool), Error> {
        if datagram.len() < QRTR_HEADER_BYTES || datagram.len() & 3 != 0 {
            return Err(Error::InvalidQrtr);
        }
        let source = Address {
            node: word(datagram, 8),
            port: word(datagram, 12),
        };
        let destination = Address {
            node: word(datagram, 24),
            port: word(datagram, 28),
        };
        if source != endpoint || destination != self.local {
            return Err(Error::WrongEndpoint);
        }
        let payload_len = word(datagram, 20) as usize;
        if word(datagram, 0) != QRTR_VERSION_V1
            || word(datagram, 4) != QRTR_PACKET_DATA
            || word(datagram, 16) > 1
            || payload_len > MAX_QMI_BYTES
            || datagram.len() != QRTR_HEADER_BYTES + align4(payload_len)
        {
            return Err(Error::InvalidQrtr);
        }
        if datagram[QRTR_HEADER_BYTES + payload_len..]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(Error::InvalidQrtr);
        }
        Ok((
            &datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + payload_len],
            word(datagram, 16) == 1,
        ))
    }

    /// A transport reset must be externally proven before the caller advances
    /// this generation. Transaction IDs are not reset, preventing stale QMI
    /// replies from matching a later logical session.
    pub fn begin_generation(&mut self, generation: u64) -> Result<(), Error> {
        if generation <= self.generation {
            return Err(Error::InvalidGeneration);
        }
        self.generation = generation;
        self.endpoint = None;
        self.pending = None;
        self.state = State::Searching;
        self.icnss_registration = false;
        self.outcome = None;
        self.msa_reservation = None;
        self.msa_info_outcome = None;
        self.capability_summary = None;
        self.last_indication = None;
        self.firmware_ready_seen = false;
        self.firmware_ready_indication_seen = false;
        self.msa_ready_indication_seen = false;
        self.dynamic_feature_mask_done = false;
        self.bdf_progress = None;
        Ok(())
    }

    fn check_generation(&self, generation: u64) -> Result<(), Error> {
        if generation != self.generation {
            Err(Error::StaleGeneration)
        } else {
            Ok(())
        }
    }
}
