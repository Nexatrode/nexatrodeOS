//! Bounded codecs for SERVREG_NOTIF queries, state notifications, and PD restart operations.
//!
//! The schema is pinned to LineageOS Sunfish kernel commit
//! `28f9290ae067d3e7857126eb6312720dbe9c7c80`,
//! `drivers/soc/qcom/service-notifier-private.h`. This module only encodes a
//! QMI payloads and decodes replies. It performs no transport, registration,
//! or protection-domain lifecycle action; callers must enforce their own
//! one-shot policy before publishing RESTART_PD.
#![forbid(unsafe_code)]

use crate::qcom_qmi::{self as qmi, Kind, Message};

pub const SERVICE_ID: u32 = 0x42;
pub const SERVICE_VERSION: u8 = 1;
pub const REGISTER_LISTENER_MESSAGE_ID: u16 = 0x0020;
pub const QUERY_STATE_MESSAGE_ID: u16 = 0x0021;
pub const STATE_UPDATED_INDICATION_MESSAGE_ID: u16 = 0x0022;
pub const SET_ACK_MESSAGE_ID: u16 = 0x0023;
pub const RESTART_PD_MESSAGE_ID: u16 = 0x0024;
pub const MAX_SERVICE_NAME_BYTES: usize = 64;
/// Domain path returned by SERVREG_LOC for the `wlan/fw` service.
/// The pinned Linux client registers and restarts this domain path, not the
/// service alias used in the GET_DOMAIN_LIST query.
pub const WLAN_PD_DOMAIN_NAME: &[u8] = b"msm/modem/wlan_pd";
pub const QMI_HEADER_BYTES: usize = qmi::HEADER_BYTES;
pub const MAX_QUERY_BYTES: usize = QMI_HEADER_BYTES + 3 + MAX_SERVICE_NAME_BYTES;
pub const WLAN_PD_QUERY_BYTES: usize = QMI_HEADER_BYTES + 3 + WLAN_PD_DOMAIN_NAME.len();
pub const WLAN_PD_REGISTER_LISTENER_BYTES: usize =
    QMI_HEADER_BYTES + 4 + 3 + WLAN_PD_DOMAIN_NAME.len();
pub const WLAN_PD_STATE_UPDATED_INDICATION_BYTES: usize =
    QMI_HEADER_BYTES + 3 + 4 + 3 + MAX_SERVICE_NAME_BYTES + 3 + 2;
pub const WLAN_PD_SET_ACK_REQUEST_BYTES: usize =
    QMI_HEADER_BYTES + 3 + WLAN_PD_DOMAIN_NAME.len() + 3 + 2;
pub const WLAN_PD_RESTART_PD_BYTES: usize = QMI_HEADER_BYTES + 3 + WLAN_PD_DOMAIN_NAME.len();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(qmi::Error),
    InvalidServiceName,
    OutputTooSmall,
    WrongMessage,
    WrongTransaction,
    MissingResponse,
    InvalidResponseLength,
    InvalidStateLength,
    InvalidIndicationTransactionLength,
}

impl From<qmi::Error> for Error {
    fn from(error: qmi::Error) -> Self {
        Self::Wire(error)
    }
}

/// Encode QMI SERVREG_NOTIF QUERY_STATE for a bounded ASCII service name.
/// The returned slice is a QMI message, ready to be placed inside a QRTR
/// datagram after a live endpoint has been discovered and validated.
pub fn encode_query(
    transaction: u16,
    service_name: &[u8],
    output: &mut [u8],
) -> Result<usize, Error> {
    if service_name.is_empty()
        || service_name.len() > MAX_SERVICE_NAME_BYTES
        || !service_name.iter().all(u8::is_ascii_graphic)
    {
        return Err(Error::InvalidServiceName);
    }
    let payload_bytes = 3 + service_name.len();
    let total_bytes = QMI_HEADER_BYTES + payload_bytes;
    if output.len() < total_bytes {
        return Err(Error::OutputTooSmall);
    }

    output[0] = 0; // QMI request
    output[1..3].copy_from_slice(&transaction.to_le_bytes());
    output[3..5].copy_from_slice(&QUERY_STATE_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(payload_bytes as u16).to_le_bytes());
    output[7] = 0x01; // service_name QMI_STRING
    output[8..10].copy_from_slice(&(service_name.len() as u16).to_le_bytes());
    output[10..total_bytes].copy_from_slice(service_name);
    Ok(total_bytes)
}

/// Encode the Lineage SERVREG_NOTIF listener registration request. Enabling
/// the listener is required by the pinned kernel client's PD-restart path.
pub fn encode_register_listener(
    transaction: u16,
    enable: bool,
    service_name: &[u8],
    output: &mut [u8],
) -> Result<usize, Error> {
    if service_name.is_empty()
        || service_name.len() > MAX_SERVICE_NAME_BYTES
        || !service_name.iter().all(u8::is_ascii_graphic)
    {
        return Err(Error::InvalidServiceName);
    }
    let payload_bytes = 4 + 3 + service_name.len();
    let total_bytes = QMI_HEADER_BYTES + payload_bytes;
    if output.len() < total_bytes {
        return Err(Error::OutputTooSmall);
    }

    output[0] = 0; // QMI request
    output[1..3].copy_from_slice(&transaction.to_le_bytes());
    output[3..5].copy_from_slice(&REGISTER_LISTENER_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(payload_bytes as u16).to_le_bytes());
    output[7] = 0x01; // enable
    output[8..10].copy_from_slice(&1u16.to_le_bytes());
    output[10] = u8::from(enable);
    output[11] = 0x02; // service_name QMI_STRING
    output[12..14].copy_from_slice(&(service_name.len() as u16).to_le_bytes());
    output[14..total_bytes].copy_from_slice(service_name);
    Ok(total_bytes)
}

/// Encode QMI SERVREG_NOTIF RESTART_PD for a bounded ASCII service name.
/// The returned slice is a QMI request, ready to be wrapped in QRTR-v1 DATA.
pub fn encode_restart_pd(
    transaction: u16,
    service_name: &[u8],
    output: &mut [u8],
) -> Result<usize, Error> {
    if service_name.is_empty()
        || service_name.len() > MAX_SERVICE_NAME_BYTES
        || !service_name.iter().all(u8::is_ascii_graphic)
    {
        return Err(Error::InvalidServiceName);
    }
    let payload_bytes = 3 + service_name.len();
    let total_bytes = QMI_HEADER_BYTES + payload_bytes;
    if output.len() < total_bytes {
        return Err(Error::OutputTooSmall);
    }

    output[0] = 0; // QMI request
    output[1..3].copy_from_slice(&transaction.to_le_bytes());
    output[3..5].copy_from_slice(&RESTART_PD_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(payload_bytes as u16).to_le_bytes());
    output[7] = 0x01; // service_name QMI_STRING, pinned Sunfish schema
    output[8..10].copy_from_slice(&(service_name.len() as u16).to_le_bytes());
    output[10..total_bytes].copy_from_slice(service_name);
    Ok(total_bytes)
}

/// Encode the source-defined SERVREG_NOTIF SET_ACK request for one received
/// STATE_UPDATED_IND. The QMI transaction is local to this request; the
/// indication transaction is carried separately in TLV 0x02.
pub fn encode_state_updated_ack(
    transaction: u16,
    service_name: &[u8],
    indication_transaction: u16,
    output: &mut [u8],
) -> Result<usize, Error> {
    if service_name.is_empty()
        || service_name.len() > MAX_SERVICE_NAME_BYTES
        || !service_name.iter().all(u8::is_ascii_graphic)
    {
        return Err(Error::InvalidServiceName);
    }
    let payload_bytes = 3 + service_name.len() + 3 + 2;
    let total_bytes = QMI_HEADER_BYTES + payload_bytes;
    if output.len() < total_bytes {
        return Err(Error::OutputTooSmall);
    }

    output[0] = 0; // QMI request
    output[1..3].copy_from_slice(&transaction.to_le_bytes());
    output[3..5].copy_from_slice(&SET_ACK_MESSAGE_ID.to_le_bytes());
    output[5..7].copy_from_slice(&(payload_bytes as u16).to_le_bytes());
    output[7] = 0x01; // service_name QMI_STRING
    output[8..10].copy_from_slice(&(service_name.len() as u16).to_le_bytes());
    output[10..10 + service_name.len()].copy_from_slice(service_name);
    let transaction_tlv = 10 + service_name.len();
    output[transaction_tlv] = 0x02; // indication transaction_id
    output[transaction_tlv + 1..transaction_tlv + 3].copy_from_slice(&2u16.to_le_bytes());
    output[transaction_tlv + 3..transaction_tlv + 5]
        .copy_from_slice(&indication_transaction.to_le_bytes());
    Ok(total_bytes)
}

/// A SERVREG state is a signed 32-bit enum in the pinned QMI schema. Preserve
/// unknown values so a newer modem enum remains observable without guessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Down,
    Up,
    EarlyDown,
    Uninit,
    Unknown(i32),
}

impl From<i32> for ServiceState {
    fn from(value: i32) -> Self {
        match value {
            0x0fff_ffff => Self::Down,
            0x1fff_ffff => Self::Up,
            0x2fff_ffff => Self::EarlyDown,
            0x7fff_ffff => Self::Uninit,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryStateResponse {
    /// QMI common response result (0 success, 1 failure), retained verbatim.
    pub result: u16,
    /// QMI common response error code, retained verbatim.
    pub error: u16,
    /// Optional current-state field from TLV 0x10.
    pub state: Option<ServiceState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterListenerResponse {
    pub result: u16,
    pub error: u16,
    pub state: Option<ServiceState>,
}

impl QueryStateResponse {
    pub const fn is_success(self) -> bool {
        self.result == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestartPdResponse {
    /// QMI common response result (0 success, 1 failure), retained verbatim.
    pub result: u16,
    /// QMI common response error code, retained verbatim.
    pub error: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateUpdatedIndication<'a> {
    /// QMI envelope transaction, retained without assuming a value for IND.
    pub qmi_transaction: u16,
    pub state: ServiceState,
    pub service_name: &'a [u8],
    /// SERVREG transaction that must be echoed in SET_ACK TLV 0x02.
    pub indication_transaction: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetAckResponse {
    pub result: u16,
    pub error: u16,
}

/// Decode one complete QMI response and bind it to the expected transaction.
/// QRTR source/destination addresses must be checked by the transport layer.
pub fn decode_response(
    bytes: &[u8],
    expected_transaction: u16,
) -> Result<QueryStateResponse, Error> {
    let message = Message::decode(bytes)?;
    if message.kind != Kind::Response || message.message_id != QUERY_STATE_MESSAGE_ID {
        return Err(Error::WrongMessage);
    }
    if message.transaction != expected_transaction {
        return Err(Error::WrongTransaction);
    }
    message.check_fields(&[0x02, 0x10])?;
    let common = message.get(0x02).ok_or(Error::MissingResponse)?;
    if common.len() != 4 {
        return Err(Error::InvalidResponseLength);
    }
    let result = u16::from_le_bytes([common[0], common[1]]);
    let error = u16::from_le_bytes([common[2], common[3]]);
    let state = message
        .get(0x10)
        .map(|value| {
            if value.len() != 4 {
                return Err(Error::InvalidStateLength);
            }
            Ok(ServiceState::from(i32::from_le_bytes([
                value[0], value[1], value[2], value[3],
            ])))
        })
        .transpose()?;
    Ok(QueryStateResponse {
        result,
        error,
        state,
    })
}

/// Decode the common response and current service state from REGISTER_LISTENER.
pub fn decode_register_listener_response(
    bytes: &[u8],
    expected_transaction: u16,
) -> Result<RegisterListenerResponse, Error> {
    let message = Message::decode(bytes)?;
    if message.kind != Kind::Response || message.message_id != REGISTER_LISTENER_MESSAGE_ID {
        return Err(Error::WrongMessage);
    }
    if message.transaction != expected_transaction {
        return Err(Error::WrongTransaction);
    }
    message.check_fields(&[0x02, 0x10])?;
    let common = message.get(0x02).ok_or(Error::MissingResponse)?;
    if common.len() != 4 {
        return Err(Error::InvalidResponseLength);
    }
    let state = message
        .get(0x10)
        .map(|value| {
            if value.len() != 4 {
                return Err(Error::InvalidStateLength);
            }
            Ok(ServiceState::from(i32::from_le_bytes([
                value[0], value[1], value[2], value[3],
            ])))
        })
        .transpose()?;
    Ok(RegisterListenerResponse {
        result: u16::from_le_bytes([common[0], common[1]]),
        error: u16::from_le_bytes([common[2], common[3]]),
        state,
    })
}

/// Decode only the common response TLV for a matching RESTART_PD response.
pub fn decode_restart_pd_response(
    bytes: &[u8],
    expected_transaction: u16,
) -> Result<RestartPdResponse, Error> {
    let message = Message::decode(bytes)?;
    if message.kind != Kind::Response || message.message_id != RESTART_PD_MESSAGE_ID {
        return Err(Error::WrongMessage);
    }
    if message.transaction != expected_transaction {
        return Err(Error::WrongTransaction);
    }
    message.check_fields(&[0x02])?;
    let common = message.get(0x02).ok_or(Error::MissingResponse)?;
    if common.len() != 4 {
        return Err(Error::InvalidResponseLength);
    }
    Ok(RestartPdResponse {
        result: u16::from_le_bytes([common[0], common[1]]),
        error: u16::from_le_bytes([common[2], common[3]]),
    })
}

/// Decode the Lineage SERVREG_NOTIF STATE_UPDATED_IND schema. The exact
/// service path and the obligation to ACK it remain caller policy.
pub fn decode_state_updated_indication(bytes: &[u8]) -> Result<StateUpdatedIndication<'_>, Error> {
    let message = Message::decode(bytes)?;
    if message.kind != Kind::Indication || message.message_id != STATE_UPDATED_INDICATION_MESSAGE_ID
    {
        return Err(Error::WrongMessage);
    }
    message.check_fields(&[0x01, 0x02, 0x03])?;
    let state = match message.required(0x01)? {
        [a, b, c, d] => ServiceState::from(i32::from_le_bytes([*a, *b, *c, *d])),
        _ => return Err(Error::InvalidStateLength),
    };
    let service_name = message.required(0x02)?;
    if service_name.is_empty()
        || service_name.len() > MAX_SERVICE_NAME_BYTES
        || !service_name.iter().all(u8::is_ascii_graphic)
    {
        return Err(Error::InvalidServiceName);
    }
    let indication_transaction = match message.required(0x03)? {
        [a, b] => u16::from_le_bytes([*a, *b]),
        _ => return Err(Error::InvalidIndicationTransactionLength),
    };
    Ok(StateUpdatedIndication {
        qmi_transaction: message.transaction,
        state,
        service_name,
        indication_transaction,
    })
}

/// Decode the common response to SET_ACK and bind it to the request
/// transaction. QRTR peer/address correlation belongs to the transport.
pub fn decode_state_updated_ack_response(
    bytes: &[u8],
    expected_transaction: u16,
) -> Result<SetAckResponse, Error> {
    let message = Message::decode(bytes)?;
    if message.kind != Kind::Response || message.message_id != SET_ACK_MESSAGE_ID {
        return Err(Error::WrongMessage);
    }
    if message.transaction != expected_transaction {
        return Err(Error::WrongTransaction);
    }
    message.check_fields(&[0x02])?;
    let common = message.get(0x02).ok_or(Error::MissingResponse)?;
    if common.len() != 4 {
        return Err(Error::InvalidResponseLength);
    }
    Ok(SetAckResponse {
        result: u16::from_le_bytes([common[0], common[1]]),
        error: u16::from_le_bytes([common[2], common[3]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qmi(kind: u8, transaction: u16, message_id: u16, fields: &[(u8, &[u8])]) -> Vec<u8> {
        let mut bytes = vec![kind];
        bytes.extend_from_slice(&transaction.to_le_bytes());
        bytes.extend_from_slice(&message_id.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        for (kind, value) in fields {
            bytes.push(*kind);
            bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
            bytes.extend_from_slice(value);
        }
        let payload = (bytes.len() - QMI_HEADER_BYTES) as u16;
        bytes[5..7].copy_from_slice(&payload.to_le_bytes());
        bytes
    }

    #[test]
    fn wlan_domain_query_matches_the_pinned_qmi_string_schema() {
        let mut bytes = [0xa5; WLAN_PD_QUERY_BYTES];
        let length = encode_query(0x1234, WLAN_PD_DOMAIN_NAME, &mut bytes).unwrap();
        assert_eq!(length, WLAN_PD_QUERY_BYTES);
        assert_eq!(
            &bytes,
            b"\x00\x34\x12\x21\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd"
        );
        let message = Message::decode(&bytes).unwrap();
        assert_eq!(message.kind, Kind::Request);
        assert_eq!(message.transaction, 0x1234);
        assert_eq!(message.message_id, QUERY_STATE_MESSAGE_ID);
        assert_eq!(message.required(0x01), Ok(WLAN_PD_DOMAIN_NAME));
    }

    #[test]
    fn wlan_domain_restart_pd_matches_the_pinned_request_schema() {
        let mut bytes = [0xa5; WLAN_PD_RESTART_PD_BYTES];
        let length = encode_restart_pd(2, WLAN_PD_DOMAIN_NAME, &mut bytes).unwrap();
        assert_eq!(length, WLAN_PD_RESTART_PD_BYTES);
        assert_eq!(
            &bytes,
            b"\x00\x02\x00\x24\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd"
        );
        let message = Message::decode(&bytes).unwrap();
        assert_eq!(message.kind, Kind::Request);
        assert_eq!(message.transaction, 2);
        assert_eq!(message.message_id, RESTART_PD_MESSAGE_ID);
        assert_eq!(message.required(0x01), Ok(WLAN_PD_DOMAIN_NAME));
    }

    #[test]
    fn wlan_domain_listener_registration_matches_the_pinned_qmi_schema() {
        let mut bytes = [0xa5; WLAN_PD_REGISTER_LISTENER_BYTES];
        let length = encode_register_listener(2, true, WLAN_PD_DOMAIN_NAME, &mut bytes).unwrap();
        assert_eq!(length, WLAN_PD_REGISTER_LISTENER_BYTES);
        assert_eq!(
            &bytes,
            b"\x00\x02\x00\x20\x00\x18\x00\x01\x01\x00\x01\x02\x11\x00msm/modem/wlan_pd"
        );
        let message = Message::decode(&bytes).unwrap();
        assert_eq!(message.kind, Kind::Request);
        assert_eq!(message.transaction, 2);
        assert_eq!(message.message_id, REGISTER_LISTENER_MESSAGE_ID);
        assert_eq!(message.required(0x01), Ok(&[1][..]));
        assert_eq!(message.required(0x02), Ok(WLAN_PD_DOMAIN_NAME));
    }

    #[test]
    fn decodes_listener_registration_state_and_failure() {
        let common = [0, 0, 0, 0];
        let state = 0x7fff_ffffi32.to_le_bytes();
        let success = qmi(
            2,
            3,
            REGISTER_LISTENER_MESSAGE_ID,
            &[(0x02, &common), (0x10, &state)],
        );
        assert_eq!(
            decode_register_listener_response(&success, 3),
            Ok(RegisterListenerResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Uninit),
            })
        );
        let failure = [1, 0, 9, 0];
        let failed = qmi(2, 4, REGISTER_LISTENER_MESSAGE_ID, &[(0x02, &failure)]);
        assert_eq!(
            decode_register_listener_response(&failed, 4),
            Ok(RegisterListenerResponse {
                result: 1,
                error: 9,
                state: None,
            })
        );
    }

    #[test]
    fn decodes_restart_pd_common_response_and_rejects_wrong_identity() {
        let common = [0, 0, 0, 0];
        let success = qmi(2, 7, RESTART_PD_MESSAGE_ID, &[(0x02, &common)]);
        assert_eq!(
            decode_restart_pd_response(&success, 7),
            Ok(RestartPdResponse {
                result: 0,
                error: 0
            })
        );
        assert_eq!(
            decode_restart_pd_response(&success, 8),
            Err(Error::WrongTransaction)
        );
        let wrong_id = qmi(2, 7, QUERY_STATE_MESSAGE_ID, &[(0x02, &common)]);
        assert_eq!(
            decode_restart_pd_response(&wrong_id, 7),
            Err(Error::WrongMessage)
        );
        let failure = [1, 0, 0x15, 0];
        let failed = qmi(2, 9, RESTART_PD_MESSAGE_ID, &[(0x02, &failure)]);
        assert_eq!(
            decode_restart_pd_response(&failed, 9),
            Ok(RestartPdResponse {
                result: 1,
                error: 0x15
            })
        );
    }

    #[test]
    fn state_updated_indication_matches_lineage_schema_and_ack_echoes_its_identity() {
        let state = 0x1fff_ffffi32.to_le_bytes();
        let indication_transaction = 0x1234u16.to_le_bytes();
        let indication = qmi(
            4,
            0xbeef,
            STATE_UPDATED_INDICATION_MESSAGE_ID,
            &[
                (0x01, &state),
                (0x02, WLAN_PD_DOMAIN_NAME),
                (0x03, &indication_transaction),
            ],
        );
        let decoded = decode_state_updated_indication(&indication).unwrap();
        assert_eq!(decoded.qmi_transaction, 0xbeef);
        assert_eq!(decoded.state, ServiceState::Up);
        assert_eq!(decoded.service_name, WLAN_PD_DOMAIN_NAME);
        assert_eq!(decoded.indication_transaction, 0x1234);

        let mut ack = [0xa5; WLAN_PD_SET_ACK_REQUEST_BYTES];
        let length = encode_state_updated_ack(
            0x5678,
            decoded.service_name,
            decoded.indication_transaction,
            &mut ack,
        )
        .unwrap();
        assert_eq!(length, 32);
        assert_eq!(
            &ack,
            b"\x00\x78\x56\x23\x00\x19\x00\x01\x11\x00msm/modem/wlan_pd\x02\x02\x00\x34\x12"
        );
        let ack_message = Message::decode(&ack).unwrap();
        assert_eq!(ack_message.kind, Kind::Request);
        assert_eq!(ack_message.transaction, 0x5678);
        assert_eq!(ack_message.message_id, SET_ACK_MESSAGE_ID);
        assert_eq!(ack_message.required(0x01), Ok(WLAN_PD_DOMAIN_NAME));
        assert_eq!(ack_message.required(0x02), Ok(&0x1234u16.to_le_bytes()[..]));
    }

    #[test]
    fn set_ack_response_preserves_common_result_and_binds_transaction() {
        let success = qmi(2, 0x5678, SET_ACK_MESSAGE_ID, &[(0x02, &[0, 0, 0, 0])]);
        assert_eq!(
            decode_state_updated_ack_response(&success, 0x5678),
            Ok(SetAckResponse {
                result: 0,
                error: 0
            })
        );
        assert_eq!(
            decode_state_updated_ack_response(&success, 0x5679),
            Err(Error::WrongTransaction)
        );
        let failed = qmi(2, 0x5678, SET_ACK_MESSAGE_ID, &[(0x02, &[1, 0, 0x2a, 0])]);
        assert_eq!(
            decode_state_updated_ack_response(&failed, 0x5678),
            Ok(SetAckResponse {
                result: 1,
                error: 0x2a
            })
        );
    }

    #[test]
    fn state_updated_indication_rejects_wrong_identity_and_malformed_fields() {
        let state = 0x1fff_ffffi32.to_le_bytes();
        let name = WLAN_PD_DOMAIN_NAME;
        let indication_transaction = 1u16.to_le_bytes();
        let wrong_kind = qmi(
            0,
            1,
            STATE_UPDATED_INDICATION_MESSAGE_ID,
            &[
                (0x01, &state),
                (0x02, name),
                (0x03, &indication_transaction),
            ],
        );
        assert_eq!(
            decode_state_updated_indication(&wrong_kind),
            Err(Error::WrongMessage)
        );
        let wrong_id = qmi(
            4,
            1,
            QUERY_STATE_MESSAGE_ID,
            &[
                (0x01, &state),
                (0x02, name),
                (0x03, &indication_transaction),
            ],
        );
        assert_eq!(
            decode_state_updated_indication(&wrong_id),
            Err(Error::WrongMessage)
        );
        let missing_transaction = qmi(
            4,
            1,
            STATE_UPDATED_INDICATION_MESSAGE_ID,
            &[(0x01, &state), (0x02, name)],
        );
        assert_eq!(
            decode_state_updated_indication(&missing_transaction),
            Err(Error::Wire(qmi::Error::MissingTlv(0x03)))
        );
        let short_state = [0xff; 3];
        let malformed_state = qmi(
            4,
            1,
            STATE_UPDATED_INDICATION_MESSAGE_ID,
            &[
                (0x01, &short_state),
                (0x02, name),
                (0x03, &indication_transaction),
            ],
        );
        assert_eq!(
            decode_state_updated_indication(&malformed_state),
            Err(Error::InvalidStateLength)
        );
        let short_transaction = [1u8];
        let malformed_transaction = qmi(
            4,
            1,
            STATE_UPDATED_INDICATION_MESSAGE_ID,
            &[(0x01, &state), (0x02, name), (0x03, &short_transaction)],
        );
        assert_eq!(
            decode_state_updated_indication(&malformed_transaction),
            Err(Error::InvalidIndicationTransactionLength)
        );
        let invalid_name = qmi(
            4,
            1,
            STATE_UPDATED_INDICATION_MESSAGE_ID,
            &[
                (0x01, &state),
                (0x02, b"bad\0path"),
                (0x03, &indication_transaction),
            ],
        );
        assert_eq!(
            decode_state_updated_indication(&invalid_name),
            Err(Error::InvalidServiceName)
        );
    }

    #[test]
    fn set_ack_encoder_checks_name_and_capacity_before_writing() {
        let mut output = [0xa5; WLAN_PD_SET_ACK_REQUEST_BYTES];
        assert_eq!(
            encode_state_updated_ack(1, b"", 2, &mut output),
            Err(Error::InvalidServiceName)
        );
        assert_eq!(
            encode_state_updated_ack(1, b"wlan/fw\0", 2, &mut output),
            Err(Error::InvalidServiceName)
        );
        assert_eq!(
            encode_state_updated_ack(1, WLAN_PD_DOMAIN_NAME, 2, &mut output[..31]),
            Err(Error::OutputTooSmall)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn query_name_and_output_bounds_are_checked_before_writes() {
        let mut output = [0xa5; MAX_QUERY_BYTES];
        assert_eq!(
            encode_query(1, b"", &mut output),
            Err(Error::InvalidServiceName)
        );
        assert_eq!(
            encode_query(1, b"wlan/fw\0", &mut output),
            Err(Error::InvalidServiceName)
        );
        assert_eq!(
            encode_query(1, &[b'a'; MAX_SERVICE_NAME_BYTES + 1], &mut output),
            Err(Error::InvalidServiceName)
        );
        assert_eq!(
            encode_query(1, b"wlan/fw", &mut output[..16]),
            Err(Error::OutputTooSmall)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn decodes_common_success_and_optional_wlan_state() {
        let common = [0, 0, 0, 0];
        let state = 0x1fff_ffffi32.to_le_bytes();
        let bytes = qmi(
            2,
            9,
            QUERY_STATE_MESSAGE_ID,
            &[(0x02, &common), (0x10, &state)],
        );
        assert_eq!(
            decode_response(&bytes, 9),
            Ok(QueryStateResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Up),
            })
        );
    }

    #[test]
    fn accepts_success_without_optional_state_and_preserves_failure_code() {
        let common_success = [0, 0, 0, 0];
        let success = qmi(2, 2, QUERY_STATE_MESSAGE_ID, &[(0x02, &common_success)]);
        assert_eq!(
            decode_response(&success, 2).unwrap(),
            QueryStateResponse {
                result: 0,
                error: 0,
                state: None
            }
        );

        let common_failure = [1, 0, 0x2a, 0];
        let failure = qmi(2, 3, QUERY_STATE_MESSAGE_ID, &[(0x02, &common_failure)]);
        let response = decode_response(&failure, 3).unwrap();
        assert_eq!(
            (response.result, response.error, response.state),
            (1, 42, None)
        );
        assert!(!response.is_success());
    }

    #[test]
    fn reports_known_and_unknown_signed_state_values_without_coercion() {
        assert_eq!(ServiceState::from(0x0fff_ffff), ServiceState::Down);
        assert_eq!(ServiceState::from(0x2fff_ffff), ServiceState::EarlyDown);
        assert_eq!(ServiceState::from(0x7fff_ffff), ServiceState::Uninit);
        assert_eq!(ServiceState::from(-1), ServiceState::Unknown(-1));
    }

    #[test]
    fn rejects_wrong_identity_missing_fields_and_malformed_lengths() {
        let common = [0, 0, 0, 0];
        let wrong_kind = qmi(0, 1, QUERY_STATE_MESSAGE_ID, &[(0x02, &common)]);
        assert_eq!(decode_response(&wrong_kind, 1), Err(Error::WrongMessage));

        let wrong_id = qmi(2, 1, 0x20, &[(0x02, &common)]);
        assert_eq!(decode_response(&wrong_id, 1), Err(Error::WrongMessage));

        let correct = qmi(2, 1, QUERY_STATE_MESSAGE_ID, &[(0x02, &common)]);
        assert_eq!(decode_response(&correct, 2), Err(Error::WrongTransaction));
        assert_eq!(
            decode_response(&qmi(2, 1, QUERY_STATE_MESSAGE_ID, &[]), 1),
            Err(Error::MissingResponse)
        );

        let short_common = [0, 0, 0];
        assert_eq!(
            decode_response(
                &qmi(2, 1, QUERY_STATE_MESSAGE_ID, &[(0x02, &short_common)]),
                1
            ),
            Err(Error::InvalidResponseLength)
        );
        let short_state = [0, 0, 0];
        assert_eq!(
            decode_response(
                &qmi(
                    2,
                    1,
                    QUERY_STATE_MESSAGE_ID,
                    &[(0x02, &common), (0x10, &short_state)]
                ),
                1
            ),
            Err(Error::InvalidStateLength)
        );
    }

    #[test]
    fn rejects_duplicate_and_unknown_mandatory_fields() {
        let common = [0, 0, 0, 0];
        let duplicate = qmi(
            2,
            1,
            QUERY_STATE_MESSAGE_ID,
            &[(0x02, &common), (0x02, &common)],
        );
        assert_eq!(
            decode_response(&duplicate, 1),
            Err(Error::Wire(qmi::Error::DuplicateTlv(0x02)))
        );
        let extra_mandatory = [7];
        assert_eq!(
            decode_response(
                &qmi(
                    2,
                    1,
                    QUERY_STATE_MESSAGE_ID,
                    &[(0x02, &common), (0x03, &extra_mandatory)]
                ),
                1
            ),
            Err(Error::Wire(qmi::Error::UnknownMandatoryTlv(0x03)))
        );
    }
}
