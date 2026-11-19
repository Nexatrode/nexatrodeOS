//! Bounded station authentication and association management-frame codecs.
//!
//! The caller supplies the selected BSS parameters and security IEs. This
//! module only encodes and validates the open-system authentication and
//! association exchange; it does not choose a network, derive/install keys,
//! send frames, change channels, or claim that protected data traffic is ready.
#![forbid(unsafe_code)]

use crate::ath10k_nwifi_rx::{FrameKind, inspect_frame};

const MANAGEMENT_HEADER_BYTES: usize = 24;
const AUTH_FIXED_BYTES: usize = 6;
const ASSOC_REQUEST_FIXED_BYTES: usize = 4;
const ASSOC_RESPONSE_FIXED_BYTES: usize = 6;

const FC_TYPE_SUBTYPE_MASK: u16 = 0x00fc;
const FC_AUTH: u16 = 0x00b0;
const FC_ASSOC_REQUEST: u16 = 0x0000;
const FC_ASSOC_RESPONSE: u16 = 0x0010;
const FC_PROTECTED: u16 = 0x4000;

const AUTH_ALGORITHM_OPEN_SYSTEM: u16 = 0;
const AUTH_TRANSACTION_REQUEST: u16 = 1;
const AUTH_TRANSACTION_RESPONSE: u16 = 2;

const WLAN_EID_SSID: u8 = 0;
const WLAN_EID_SUPPORTED_RATES: u8 = 1;
const WLAN_EID_RSN: u8 = 48;
const WLAN_EID_EXTENDED_SUPPORTED_RATES: u8 = 50;

const CAPABILITY_ESS: u16 = 1 << 0;
const CAPABILITY_IBSS: u16 = 1 << 1;
const CAPABILITY_PRIVACY: u16 = 1 << 4;

const MAX_SSID_BYTES: usize = 32;
const MAX_SUPPORTED_RATES_BYTES: usize = 8;
const MAX_SEQUENCE_NUMBER: u16 = 0x0fff;
const MAX_ASSOCIATION_ID: u16 = 2007;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationFrameError {
    InvalidStationAddress,
    InvalidBssid,
    SequenceNumberOutOfRange,
    SsidTooLong,
    InvalidSupportedRates,
    ExtendedRatesTooLong,
    InvalidRsnElement,
    InvalidAdditionalElements,
    DuplicateCoreElement(u8),
    InvalidCapability,
    OutputTooSmall,
    FrameHeader,
    WrongFrameKind,
    WrongFrameSubtype,
    ProtectedManagementFrame,
    WrongPeer,
    TruncatedBody,
    UnsupportedAuthenticationAlgorithm,
    WrongAuthenticationTransaction,
    InvalidAssociationId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationIdentity {
    pub station: [u8; 6],
    pub bssid: [u8; 6],
    /// 12-bit sequence number, with fragment number zero.
    pub sequence_number: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssociationRequest<'a> {
    pub identity: StationIdentity,
    /// Capability bits selected by the caller. ESS is required, IBSS is
    /// rejected, and the privacy bit must agree with presence of `rsn`.
    pub capability_info: u16,
    pub listen_interval: u16,
    pub ssid: &'a [u8],
    pub supported_rates: &'a [u8],
    pub extended_supported_rates: &'a [u8],
    /// RSN information-element value, excluding its 2-byte ID/length prefix.
    pub rsn: Option<&'a [u8]>,
    /// Already-ordered, complete IEs such as HT/VHT/WMM capabilities. SSID,
    /// rates, and RSN elements are rejected here to prevent duplicates.
    pub additional_ies: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedStationFrame {
    pub frame_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticationResponse {
    pub status_code: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssociationResponse {
    pub capability_info: u16,
    pub status_code: u16,
    /// Present only for a successful association response.
    pub association_id: Option<u16>,
}

fn valid_unicast_address(address: &[u8; 6]) -> bool {
    address.iter().any(|byte| *byte != 0) && address[0] & 1 == 0
}

fn validate_identity(identity: StationIdentity) -> Result<(), StationFrameError> {
    if !valid_unicast_address(&identity.station) {
        return Err(StationFrameError::InvalidStationAddress);
    }
    if !valid_unicast_address(&identity.bssid) {
        return Err(StationFrameError::InvalidBssid);
    }
    if identity.sequence_number > MAX_SEQUENCE_NUMBER {
        return Err(StationFrameError::SequenceNumberOutOfRange);
    }
    Ok(())
}

fn write_management_header(output: &mut [u8], frame_control: u16, identity: StationIdentity) {
    output[..2].copy_from_slice(&frame_control.to_le_bytes());
    output[2..4].fill(0); // Duration.
    output[4..10].copy_from_slice(&identity.bssid);
    output[10..16].copy_from_slice(&identity.station);
    output[16..22].copy_from_slice(&identity.bssid);
    output[22..24].copy_from_slice(&(identity.sequence_number << 4).to_le_bytes());
}

/// Encode an open-system authentication request without partial output on
/// failure. Shared-key authentication is intentionally unsupported.
pub fn encode_open_authentication_request(
    identity: StationIdentity,
    output: &mut [u8],
) -> Result<EncodedStationFrame, StationFrameError> {
    validate_identity(identity)?;
    let frame_bytes = MANAGEMENT_HEADER_BYTES + AUTH_FIXED_BYTES;
    if output.len() < frame_bytes {
        return Err(StationFrameError::OutputTooSmall);
    }

    write_management_header(output, FC_AUTH, identity);
    output[24..26].copy_from_slice(&AUTH_ALGORITHM_OPEN_SYSTEM.to_le_bytes());
    output[26..28].copy_from_slice(&AUTH_TRANSACTION_REQUEST.to_le_bytes());
    output[28..30].copy_from_slice(&0u16.to_le_bytes());
    Ok(EncodedStationFrame { frame_bytes })
}

fn parse_management_response<'a>(
    frame: &'a [u8],
    identity: StationIdentity,
    expected_subtype: u16,
) -> Result<&'a [u8], StationFrameError> {
    let view = inspect_frame(frame).map_err(|_| StationFrameError::FrameHeader)?;
    if view.kind != FrameKind::Management {
        return Err(StationFrameError::WrongFrameKind);
    }
    if view.frame_control & FC_TYPE_SUBTYPE_MASK != expected_subtype {
        return Err(StationFrameError::WrongFrameSubtype);
    }
    if view.frame_control & FC_PROTECTED != 0 {
        return Err(StationFrameError::ProtectedManagementFrame);
    }
    if view.header.len() != MANAGEMENT_HEADER_BYTES {
        return Err(StationFrameError::FrameHeader);
    }
    let address1 = &view.header[4..10];
    let address2 = &view.header[10..16];
    let address3 = &view.header[16..22];
    if address1 != identity.station || address2 != identity.bssid || address3 != identity.bssid {
        return Err(StationFrameError::WrongPeer);
    }
    Ok(view.body)
}

/// Validate an open-system authentication response from the selected AP.
/// The status code is returned as wire data; a nonzero value is not success.
pub fn parse_open_authentication_response(
    frame: &[u8],
    identity: StationIdentity,
) -> Result<AuthenticationResponse, StationFrameError> {
    validate_identity(identity)?;
    let body = parse_management_response(frame, identity, FC_AUTH)?;
    if body.len() < AUTH_FIXED_BYTES {
        return Err(StationFrameError::TruncatedBody);
    }
    if u16::from_le_bytes([body[0], body[1]]) != AUTH_ALGORITHM_OPEN_SYSTEM {
        return Err(StationFrameError::UnsupportedAuthenticationAlgorithm);
    }
    if u16::from_le_bytes([body[2], body[3]]) != AUTH_TRANSACTION_RESPONSE {
        return Err(StationFrameError::WrongAuthenticationTransaction);
    }
    Ok(AuthenticationResponse {
        status_code: u16::from_le_bytes([body[4], body[5]]),
    })
}

fn validate_rates(rates: &[u8], max: usize) -> bool {
    !rates.is_empty() && rates.len() <= max && rates.iter().all(|rate| rate & 0x7f != 0)
}

fn validate_ie_stream(bytes: &[u8], reject_core: bool) -> Result<(), StationFrameError> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let header_end = offset
            .checked_add(2)
            .ok_or(StationFrameError::InvalidAdditionalElements)?;
        if header_end > bytes.len() {
            return Err(StationFrameError::InvalidAdditionalElements);
        }
        let id = bytes[offset];
        let value_bytes = usize::from(bytes[offset + 1]);
        let end = header_end
            .checked_add(value_bytes)
            .ok_or(StationFrameError::InvalidAdditionalElements)?;
        if end > bytes.len() {
            return Err(StationFrameError::InvalidAdditionalElements);
        }
        if reject_core
            && matches!(
                id,
                WLAN_EID_SSID
                    | WLAN_EID_SUPPORTED_RATES
                    | WLAN_EID_RSN
                    | WLAN_EID_EXTENDED_SUPPORTED_RATES
            )
        {
            return Err(StationFrameError::DuplicateCoreElement(id));
        }
        offset = end;
    }
    Ok(())
}

/// Encode an association request with caller-selected capability and IEs.
/// Security suites and key installation remain the responsibility of the
/// caller; carrying an RSN IE does not mark the connection authorized.
pub fn encode_association_request(
    request: AssociationRequest<'_>,
    output: &mut [u8],
) -> Result<EncodedStationFrame, StationFrameError> {
    validate_identity(request.identity)?;
    if request.ssid.len() > MAX_SSID_BYTES {
        return Err(StationFrameError::SsidTooLong);
    }
    if !validate_rates(request.supported_rates, MAX_SUPPORTED_RATES_BYTES) {
        return Err(StationFrameError::InvalidSupportedRates);
    }
    if !request.extended_supported_rates.is_empty()
        && !validate_rates(request.extended_supported_rates, u8::MAX as usize)
    {
        return Err(StationFrameError::ExtendedRatesTooLong);
    }
    if let Some(rsn) = request.rsn {
        if rsn.len() < 2 || rsn.len() > u8::MAX as usize {
            return Err(StationFrameError::InvalidRsnElement);
        }
    }
    validate_ie_stream(request.additional_ies, true)?;

    let capability = request.capability_info;
    if capability & CAPABILITY_ESS == 0
        || capability & CAPABILITY_IBSS != 0
        || (capability & CAPABILITY_PRIVACY != 0) != request.rsn.is_some()
    {
        return Err(StationFrameError::InvalidCapability);
    }

    let mut body_bytes = ASSOC_REQUEST_FIXED_BYTES;
    for value_bytes in [
        Some(request.ssid.len()),
        Some(request.supported_rates.len()),
        (!request.extended_supported_rates.is_empty())
            .then_some(request.extended_supported_rates.len()),
        request.rsn.map(<[u8]>::len),
    ]
    .into_iter()
    .flatten()
    {
        body_bytes = body_bytes
            .checked_add(2 + value_bytes)
            .ok_or(StationFrameError::InvalidAdditionalElements)?;
    }
    body_bytes = body_bytes
        .checked_add(request.additional_ies.len())
        .ok_or(StationFrameError::InvalidAdditionalElements)?;
    let frame_bytes = MANAGEMENT_HEADER_BYTES
        .checked_add(body_bytes)
        .ok_or(StationFrameError::InvalidAdditionalElements)?;
    if output.len() < frame_bytes {
        return Err(StationFrameError::OutputTooSmall);
    }

    write_management_header(output, FC_ASSOC_REQUEST, request.identity);
    output[24..26].copy_from_slice(&capability.to_le_bytes());
    output[26..28].copy_from_slice(&request.listen_interval.to_le_bytes());
    let mut offset = 28;
    offset = write_ie(output, offset, WLAN_EID_SSID, request.ssid);
    offset = write_ie(
        output,
        offset,
        WLAN_EID_SUPPORTED_RATES,
        request.supported_rates,
    );
    if !request.extended_supported_rates.is_empty() {
        offset = write_ie(
            output,
            offset,
            WLAN_EID_EXTENDED_SUPPORTED_RATES,
            request.extended_supported_rates,
        );
    }
    if let Some(rsn) = request.rsn {
        offset = write_ie(output, offset, WLAN_EID_RSN, rsn);
    }
    output[offset..offset + request.additional_ies.len()].copy_from_slice(request.additional_ies);
    debug_assert_eq!(offset + request.additional_ies.len(), frame_bytes);
    Ok(EncodedStationFrame { frame_bytes })
}

fn write_ie(output: &mut [u8], offset: usize, id: u8, value: &[u8]) -> usize {
    output[offset] = id;
    output[offset + 1] = value.len() as u8;
    output[offset + 2..offset + 2 + value.len()].copy_from_slice(value);
    offset + 2 + value.len()
}

/// Parse a response from the selected AP. AID is absent when the AP rejects
/// the request; success also validates the IEEE 802.11 association-ID range.
pub fn parse_association_response(
    frame: &[u8],
    identity: StationIdentity,
) -> Result<AssociationResponse, StationFrameError> {
    validate_identity(identity)?;
    let body = parse_management_response(frame, identity, FC_ASSOC_RESPONSE)?;
    if body.len() < ASSOC_RESPONSE_FIXED_BYTES {
        return Err(StationFrameError::TruncatedBody);
    }
    let capability_info = u16::from_le_bytes([body[0], body[1]]);
    let status_code = u16::from_le_bytes([body[2], body[3]]);
    if status_code != 0 {
        return Ok(AssociationResponse {
            capability_info,
            status_code,
            association_id: None,
        });
    }
    let association_id = u16::from_le_bytes([body[4], body[5]]) & 0x3fff;
    if !(1..=MAX_ASSOCIATION_ID).contains(&association_id) {
        return Err(StationFrameError::InvalidAssociationId);
    }
    Ok(AssociationResponse {
        capability_info,
        status_code,
        association_id: Some(association_id),
    })
}
