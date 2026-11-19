//! Bounded SERVREG_NOTIF listener / one-shot PD-restart client.
//!
//! This targets the exact Sunfish SERVREG_NOTIF service instance. It follows
//! the pinned Linux order: register the listener, use its returned current
//! state, acknowledge matching state-update indications, and permit at most
//! one RESTART_PD only when the latest observed state is UNINIT. An optional
//! explicit state query remains supported for diagnostics. A caller must
//! still gate transport publication on the live modem-owner lifecycle.
#![forbid(unsafe_code)]

use crate::qcom_qmi_router::Address;
use crate::qcom_qrtr_control::{self as control, Server};
use crate::qcom_servreg_notif as codec;

pub const QRTR_HEADER_BYTES: usize = 32;
pub const QRTR_VERSION_V1: u32 = 1;
pub const QRTR_PACKET_DATA: u32 = 1;
pub const VERSION_INSTANCE: u32 = 0x0000_b401; // (instance 180 << 8) | version 1
pub const MAX_DATAGRAM_BYTES: usize =
    QRTR_HEADER_BYTES + ((codec::WLAN_PD_REGISTER_LISTENER_BYTES + 3) & !3);
/// Fixed datagram size for the SERVREG query and restart requests.
pub const QUERY_DATAGRAM_BYTES: usize = QRTR_HEADER_BYTES + ((codec::WLAN_PD_QUERY_BYTES + 3) & !3);
const MAX_QMI_RESPONSE_BYTES: usize = codec::QMI_HEADER_BYTES + 14;
const MAX_QMI_INDICATION_BYTES: usize = codec::WLAN_PD_STATE_UPDATED_INDICATION_BYTES;
pub const MAX_TRACE_BYTES: usize =
    if codec::WLAN_PD_STATE_UPDATED_INDICATION_BYTES > codec::WLAN_PD_SET_ACK_REQUEST_BYTES {
        codec::WLAN_PD_STATE_UPDATED_INDICATION_BYTES
    } else {
        if codec::WLAN_PD_REGISTER_LISTENER_BYTES > codec::WLAN_PD_SET_ACK_REQUEST_BYTES {
            codec::WLAN_PD_REGISTER_LISTENER_BYTES
        } else {
            codec::WLAN_PD_SET_ACK_REQUEST_BYTES
        }
    };

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    StaleGeneration,
    InvalidLocalAddress,
    InvalidServer,
    EndpointConflict,
    NoEndpoint,
    Busy,
    AlreadyComplete,
    TransactionExhausted,
    OutputTooSmall,
    InvalidDatagram,
    WrongEndpoint,
    WrongServiceName,
    NoPendingRequest,
    NoPendingIndication,
    ListenerRequired,
    RestartRequiresUninitialized,
    Wire(codec::Error),
}

impl From<codec::Error> for Error {
    fn from(error: codec::Error) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Searching,
    Ready,
    Pending,
    Complete,
    ListenerPending,
    ListenerComplete,
    AckPending,
    AckFailed,
    RestartPending,
    RestartComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Query,
    RegisterListener,
    SetAck,
    RestartPd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    generation: u64,
    endpoint: Address,
    transaction: u16,
    operation: Operation,
    trace: WireTrace,
}

/// Last transaction's bounded QMI payloads and validated QRTR endpoints.
/// This is diagnostic evidence only; it never changes request policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireTrace {
    pub source: Address,
    pub destination: Address,
    pub transaction: u16,
    pub message_id: u16,
    pub request: [u8; MAX_TRACE_BYTES],
    pub request_len: u8,
    pub response: [u8; MAX_TRACE_BYTES],
    pub response_len: u8,
}

impl WireTrace {
    const EMPTY: Self = Self {
        source: Address { node: 0, port: 0 },
        destination: Address { node: 0, port: 0 },
        transaction: 0,
        message_id: 0,
        request: [0; MAX_TRACE_BYTES],
        request_len: 0,
        response: [0; MAX_TRACE_BYTES],
        response_len: 0,
    };

    fn indication(source: Address, destination: Address, transaction: u16, bytes: &[u8]) -> Self {
        let mut trace = Self {
            source,
            destination,
            transaction,
            message_id: codec::STATE_UPDATED_INDICATION_MESSAGE_ID,
            ..Self::EMPTY
        };
        let len = bytes.len().min(MAX_TRACE_BYTES);
        trace.response[..len].copy_from_slice(&bytes[..len]);
        trace.response_len = len as u8;
        trace
    }

    fn request(
        source: Address,
        destination: Address,
        transaction: u16,
        message_id: u16,
        bytes: &[u8],
    ) -> Self {
        let mut trace = Self {
            source,
            destination,
            transaction,
            message_id,
            ..Self::EMPTY
        };
        let len = bytes.len().min(MAX_TRACE_BYTES);
        trace.request[..len].copy_from_slice(&bytes[..len]);
        trace.request_len = len as u8;
        trace
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundQuery {
    pub generation: u64,
    pub source: Address,
    pub destination: Address,
    pub transaction: u16,
    pub message_id: u16,
    pub bytes: [u8; MAX_DATAGRAM_BYTES],
    pub len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Outcome {
    /// Present only when the optional explicit QUERY_STATE operation ran.
    pub query: Option<codec::QueryStateResponse>,
    /// Present after matching REGISTER_LISTENER completes.
    pub listener: Option<codec::RegisterListenerResponse>,
    /// Present only after the one-shot RESTART_PD response is received.
    pub restart: Option<codec::RestartPdResponse>,
    /// Most recent validated WLAN state update received after listener setup.
    pub state_update: Option<StateUpdate>,
    /// Result of the matching SET_ACK request, if one was sent.
    pub set_ack: Option<codec::SetAckResponse>,
    /// The QRTR peer requested a separate receive confirmation for this packet.
    pub confirm_rx: bool,
    /// Raw bounded request/reply and the exact QRTR endpoint pair.
    pub trace: WireTrace,
}
impl Outcome {
    pub fn current_state(&self) -> Option<codec::ServiceState> {
        self.state_update
            .map(|update| update.state)
            .or_else(|| {
                self.listener
                    .filter(|response| response.result == 0 && response.error == 0)
                    .and_then(|response| response.state)
            })
            .or_else(|| {
                self.query
                    .filter(|response| response.result == 0 && response.error == 0)
                    .and_then(|response| response.state)
            })
    }
    pub fn current_result(&self) -> Option<(u16, u16)> {
        self.listener
            .map(|response| (response.result, response.error))
            .or_else(|| self.query.map(|response| (response.result, response.error)))
    }

    /// True only after a received RESTART_PD response explicitly succeeds.
    /// A queued request or a successful listener response is insufficient.
    pub fn restart_succeeded(&self) -> bool {
        self.restart
            .is_some_and(|response| response.result == 0 && response.error == 0)
    }

    pub fn state_update_acknowledged(&self) -> bool {
        self.set_ack
            .is_some_and(|response| response.result == 0 && response.error == 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateUpdate {
    pub qmi_transaction: u16,
    pub state: codec::ServiceState,
    pub indication_transaction: u16,
    pub confirm_rx: bool,
}

pub struct Client {
    local: Address,
    generation: u64,
    endpoint: Option<Address>,
    pending: Option<Pending>,
    pending_indication: Option<StateUpdate>,
    next_transaction: u32,
    state: State,
    outcome: Option<Outcome>,
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

const fn align4(value: usize) -> usize {
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
            pending_indication: None,
            next_transaction: 1,
            state: State::Searching,
            outcome: None,
        })
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn endpoint(&self) -> Option<Address> {
        self.endpoint
    }

    pub fn local_address(&self) -> Address {
        self.local
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    /// Bind only a SERVREG_NOTIF NEW_SERVER record from the authenticated QRTR
    /// control plane. A distinct second endpoint is rejected fail-closed.
    pub fn observe_server(&mut self, generation: u64, server: Server) -> Result<bool, Error> {
        self.check_generation(generation)?;
        if server.service != codec::SERVICE_ID
            || server.instance != VERSION_INSTANCE
            || !valid_node(server.address.node)
            || server.address.node == self.local.node
            || !valid_port(server.address.port)
        {
            return Err(Error::InvalidServer);
        }
        match self.endpoint {
            Some(current) if current == server.address => Ok(false),
            Some(_) => Err(Error::EndpointConflict),
            None => {
                self.endpoint = Some(server.address);
                self.state = State::Ready;
                Ok(true)
            }
        }
    }

    /// Remove only the matching service instance. A real removal invalidates
    /// the pending transaction and its result; stale removals do nothing.
    pub fn remove_server(&mut self, generation: u64, server: Server) -> Result<bool, Error> {
        self.check_generation(generation)?;
        if server.service != codec::SERVICE_ID
            || server.instance != VERSION_INSTANCE
            || self.endpoint != Some(server.address)
        {
            return Ok(false);
        }
        self.endpoint = None;
        self.pending = None;
        self.pending_indication = None;
        self.outcome = None;
        self.state = State::Searching;
        Ok(true)
    }

    /// Advance the client only after the modem owner supplies a newer trusted
    /// generation. Old endpoints and replies cannot survive lifecycle reset.
    pub fn begin_generation(&mut self, generation: u64) -> Result<(), Error> {
        if generation <= self.generation {
            return Err(Error::InvalidGeneration);
        }
        self.generation = generation;
        self.endpoint = None;
        self.pending = None;
        self.pending_indication = None;
        self.outcome = None;
        self.state = State::Searching;
        Ok(())
    }

    /// Build one exact QRTR-v1 datagram carrying the bounded QMI query. State
    /// and transaction counters change only after the full frame fits.
    pub fn begin_query(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundQuery, Error> {
        self.check_generation(generation)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        if self.state != State::Ready {
            return Err(Error::AlreadyComplete);
        }
        let transaction =
            u16::try_from(self.next_transaction).map_err(|_| Error::TransactionExhausted)?;
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        let mut qmi = [0u8; codec::WLAN_PD_QUERY_BYTES];
        let qmi_len = codec::encode_query(transaction, codec::WLAN_PD_DOMAIN_NAME, &mut qmi)
            .map_err(Error::Wire)?;
        let datagram_len = QRTR_HEADER_BYTES + align4(qmi_len);
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
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
            transaction,
            operation: Operation::Query,
            trace: WireTrace::request(
                self.local,
                endpoint,
                transaction,
                codec::QUERY_STATE_MESSAGE_ID,
                &qmi[..qmi_len],
            ),
        });
        self.next_transaction += 1;
        self.outcome = None;
        self.state = State::Pending;
        Ok(OutboundQuery {
            generation,
            source: self.local,
            destination: endpoint,
            transaction,
            message_id: codec::QUERY_STATE_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Register for SERVREG state notifications and obtain the state at
    /// registration time. The pinned Lineage PDR-restart path performs this
    /// step before sending RESTART_PD.
    pub fn begin_register_listener(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundQuery, Error> {
        self.check_generation(generation)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        let prior = self.outcome;
        if !matches!(self.state, State::Ready | State::Complete)
            || prior.is_some_and(|value| {
                value.listener.is_some()
                    || value.restart.is_some()
                    || value.query.is_some_and(|query| {
                        query.result != 0
                            || query.error != 0
                            || query.state != Some(codec::ServiceState::Uninit)
                    })
            })
        {
            return Err(Error::RestartRequiresUninitialized);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction =
            u16::try_from(self.next_transaction).map_err(|_| Error::TransactionExhausted)?;
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        let mut qmi = [0u8; codec::WLAN_PD_REGISTER_LISTENER_BYTES];
        let qmi_len = codec::encode_register_listener(
            transaction,
            true,
            codec::WLAN_PD_DOMAIN_NAME,
            &mut qmi,
        )
        .map_err(Error::Wire)?;
        let datagram_len = QRTR_HEADER_BYTES + align4(qmi_len);
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
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
            transaction,
            operation: Operation::RegisterListener,
            trace: WireTrace::request(
                self.local,
                endpoint,
                transaction,
                codec::REGISTER_LISTENER_MESSAGE_ID,
                &qmi[..qmi_len],
            ),
        });
        self.next_transaction += 1;
        self.outcome = prior;
        self.state = State::ListenerPending;
        Ok(OutboundQuery {
            generation,
            source: self.local,
            destination: endpoint,
            transaction,
            message_id: codec::REGISTER_LISTENER_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Accept one exact WLAN state-update indication after listener
    /// registration. The caller remains responsible for the QRTR receive
    /// confirmation and must publish the returned SET_ACK request once.
    pub fn accept_state_updated_indication(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<StateUpdate, Error> {
        self.check_generation(generation)?;
        if self.pending.is_some() || self.pending_indication.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::ListenerComplete {
            return Err(Error::ListenerRequired);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        if datagram.len() < QRTR_HEADER_BYTES || datagram.len() & 3 != 0 {
            return Err(Error::InvalidDatagram);
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
        let confirm_rx = word(datagram, 16);
        if word(datagram, 0) != QRTR_VERSION_V1
            || word(datagram, 4) != QRTR_PACKET_DATA
            || confirm_rx > 1
            || payload_len > MAX_QMI_INDICATION_BYTES
            || datagram.len() != QRTR_HEADER_BYTES + align4(payload_len)
            || datagram[QRTR_HEADER_BYTES + payload_len..]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(Error::InvalidDatagram);
        }
        let payload = &datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + payload_len];
        let indication = codec::decode_state_updated_indication(payload).map_err(Error::Wire)?;
        if indication.service_name != codec::WLAN_PD_DOMAIN_NAME {
            return Err(Error::WrongServiceName);
        }
        if !self.outcome.is_some_and(|outcome| {
            outcome
                .listener
                .is_some_and(|listener| listener.result == 0 && listener.error == 0)
        }) {
            return Err(Error::ListenerRequired);
        }
        let update = StateUpdate {
            qmi_transaction: indication.qmi_transaction,
            state: indication.state,
            indication_transaction: indication.indication_transaction,
            confirm_rx: confirm_rx == 1,
        };
        let prior = self.outcome;
        self.outcome = Some(Outcome {
            query: prior.and_then(|value| value.query),
            listener: prior.and_then(|value| value.listener),
            restart: prior.and_then(|value| value.restart),
            state_update: Some(update),
            set_ack: None,
            confirm_rx: update.confirm_rx,
            trace: WireTrace::indication(source, destination, indication.qmi_transaction, payload),
        });
        self.pending_indication = Some(update);
        Ok(update)
    }

    /// Build one SET_ACK for the exact accepted state-update transaction.
    /// The QMI request uses its own transaction ID and is never replayed by
    /// this client after publication or an ambiguous response.
    pub fn begin_state_updated_ack(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundQuery, Error> {
        self.check_generation(generation)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::ListenerComplete {
            return Err(Error::ListenerRequired);
        }
        let indication = self.pending_indication.ok_or(Error::NoPendingIndication)?;
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction =
            u16::try_from(self.next_transaction).map_err(|_| Error::TransactionExhausted)?;
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        let mut qmi = [0u8; codec::WLAN_PD_SET_ACK_REQUEST_BYTES];
        let qmi_len = codec::encode_state_updated_ack(
            transaction,
            codec::WLAN_PD_DOMAIN_NAME,
            indication.indication_transaction,
            &mut qmi,
        )
        .map_err(Error::Wire)?;
        let datagram_len = QRTR_HEADER_BYTES + align4(qmi_len);
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
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
            transaction,
            operation: Operation::SetAck,
            trace: WireTrace::request(
                self.local,
                endpoint,
                transaction,
                codec::SET_ACK_MESSAGE_ID,
                &qmi[..qmi_len],
            ),
        });
        self.next_transaction += 1;
        self.state = State::AckPending;
        Ok(OutboundQuery {
            generation,
            source: self.local,
            destination: endpoint,
            transaction,
            message_id: codec::SET_ACK_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Queue one RESTART_PD only after the listener confirms `wlan_pd` is
    /// UNINIT. If QUERY_STATE was also used, it must agree. This operation is
    /// terminal for the client generation; callers must not retry it.
    pub fn begin_restart_pd(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<OutboundQuery, Error> {
        self.check_generation(generation)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        let prior = self.outcome.ok_or(Error::RestartRequiresUninitialized)?;
        if self.state != State::ListenerComplete
            || prior.restart.is_some()
            || prior.state_update.is_some_and(|update| {
                update.state != codec::ServiceState::Uninit || !prior.state_update_acknowledged()
            })
            || !prior.listener.is_some_and(|listener| {
                listener.result == 0
                    && listener.error == 0
                    && listener.state == Some(codec::ServiceState::Uninit)
            })
            || prior.query.is_some_and(|query| {
                query.result != 0
                    || query.error != 0
                    || query.state != Some(codec::ServiceState::Uninit)
            })
        {
            return Err(Error::RestartRequiresUninitialized);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction =
            u16::try_from(self.next_transaction).map_err(|_| Error::TransactionExhausted)?;
        let mut bytes = [0u8; MAX_DATAGRAM_BYTES];
        let mut qmi = [0u8; codec::WLAN_PD_RESTART_PD_BYTES];
        let qmi_len = codec::encode_restart_pd(transaction, codec::WLAN_PD_DOMAIN_NAME, &mut qmi)
            .map_err(Error::Wire)?;
        let datagram_len = QRTR_HEADER_BYTES + align4(qmi_len);
        if datagram_len > output_capacity {
            return Err(Error::OutputTooSmall);
        }
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
            transaction,
            operation: Operation::RestartPd,
            trace: WireTrace::request(
                self.local,
                endpoint,
                transaction,
                codec::RESTART_PD_MESSAGE_ID,
                &qmi[..qmi_len],
            ),
        });
        self.next_transaction += 1;
        self.state = State::RestartPending;
        Ok(OutboundQuery {
            generation,
            source: self.local,
            destination: endpoint,
            transaction,
            message_id: codec::RESTART_PD_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Accept only a padded QRTR-v1 DATA response from the pending endpoint to
    /// this local port. Invalid replies leave the request pending for diagnosis.
    pub fn accept_response(&mut self, generation: u64, datagram: &[u8]) -> Result<Outcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation {
            return Err(Error::StaleGeneration);
        }
        if datagram.len() < QRTR_HEADER_BYTES || datagram.len() & 3 != 0 {
            return Err(Error::InvalidDatagram);
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
            || payload_len > MAX_QMI_RESPONSE_BYTES
            || datagram.len() != QRTR_HEADER_BYTES + align4(payload_len)
            || datagram[QRTR_HEADER_BYTES + payload_len..]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(Error::InvalidDatagram);
        }
        let payload = &datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + payload_len];
        let confirm_rx = word(datagram, 16) == 1;
        let mut trace = pending.trace;
        trace.response_len = payload_len as u8;
        trace.response[..payload_len].copy_from_slice(payload);
        let outcome = match pending.operation {
            Operation::Query => Outcome {
                query: Some(
                    codec::decode_response(payload, pending.transaction).map_err(Error::Wire)?,
                ),
                listener: None,
                restart: None,
                state_update: None,
                set_ack: None,
                confirm_rx,
                trace,
            },
            Operation::RegisterListener => {
                let prior = self.outcome;
                Outcome {
                    query: prior.and_then(|value| value.query),
                    listener: Some(
                        codec::decode_register_listener_response(payload, pending.transaction)
                            .map_err(Error::Wire)?,
                    ),
                    restart: None,
                    state_update: prior.and_then(|value| value.state_update),
                    set_ack: prior.and_then(|value| value.set_ack),
                    confirm_rx,
                    trace,
                }
            }
            Operation::RestartPd => {
                let prior = self.outcome.ok_or(Error::RestartRequiresUninitialized)?;
                Outcome {
                    query: prior.query,
                    listener: prior.listener,
                    restart: Some(
                        codec::decode_restart_pd_response(payload, pending.transaction)
                            .map_err(Error::Wire)?,
                    ),
                    state_update: prior.state_update,
                    set_ack: prior.set_ack,
                    confirm_rx,
                    trace,
                }
            }
            Operation::SetAck => {
                let prior = self.outcome.ok_or(Error::NoPendingIndication)?;
                Outcome {
                    query: prior.query,
                    listener: prior.listener,
                    restart: prior.restart,
                    state_update: prior.state_update,
                    set_ack: Some(
                        codec::decode_state_updated_ack_response(payload, pending.transaction)
                            .map_err(Error::Wire)?,
                    ),
                    confirm_rx,
                    trace,
                }
            }
        };
        self.pending = None;
        let ack_succeeded =
            pending.operation == Operation::SetAck && outcome.state_update_acknowledged();
        if ack_succeeded {
            self.pending_indication = None;
        }
        self.outcome = Some(outcome);
        self.state = match pending.operation {
            Operation::Query => State::Complete,
            Operation::RegisterListener => State::ListenerComplete,
            Operation::SetAck if ack_succeeded => State::ListenerComplete,
            Operation::SetAck => State::AckFailed,
            Operation::RestartPd => State::RestartComplete,
        };
        Ok(outcome)
    }

    fn check_generation(&self, generation: u64) -> Result<(), Error> {
        if generation == 0 {
            Err(Error::InvalidGeneration)
        } else if generation != self.generation {
            Err(Error::StaleGeneration)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_servreg_notif::ServiceState;

    const LOCAL: Address = Address {
        node: 1,
        port: 0x4002,
    };
    const SERVER: Server = Server {
        service: codec::SERVICE_ID,
        instance: VERSION_INSTANCE,
        address: Address { node: 0, port: 3 },
    };

    fn client() -> Client {
        Client::new(LOCAL, 1).unwrap()
    }

    fn response(
        transaction: u16,
        state: Option<i32>,
        source: Address,
        destination: Address,
    ) -> Vec<u8> {
        let mut qmi = vec![2];
        qmi.extend_from_slice(&transaction.to_le_bytes());
        qmi.extend_from_slice(&codec::QUERY_STATE_MESSAGE_ID.to_le_bytes());
        qmi.extend_from_slice(&[0, 0]);
        qmi.extend_from_slice(&[0x02, 4, 0, 0, 0, 0, 0]);
        if let Some(state) = state {
            qmi.extend_from_slice(&[0x10, 4, 0]);
            qmi.extend_from_slice(&state.to_le_bytes());
        }
        let payload_len = (qmi.len() - codec::QMI_HEADER_BYTES) as u16;
        qmi[5..7].copy_from_slice(&payload_len.to_le_bytes());

        let mut datagram = vec![0; QRTR_HEADER_BYTES + align4(qmi.len())];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            source.node,
            source.port,
            1,
            qmi.len() as u32,
            destination.node,
            destination.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut datagram, index * 4, *value);
        }
        datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi.len()].copy_from_slice(&qmi);
        datagram
    }

    fn restart_response(
        transaction: u16,
        result: u16,
        error: u16,
        source: Address,
        destination: Address,
    ) -> Vec<u8> {
        let mut qmi = vec![2];
        qmi.extend_from_slice(&transaction.to_le_bytes());
        qmi.extend_from_slice(&codec::RESTART_PD_MESSAGE_ID.to_le_bytes());
        qmi.extend_from_slice(&[0, 0]);
        qmi.extend_from_slice(&[
            0x02,
            4,
            0,
            result as u8,
            (result >> 8) as u8,
            error as u8,
            (error >> 8) as u8,
        ]);
        let payload_len = (qmi.len() - codec::QMI_HEADER_BYTES) as u16;
        qmi[5..7].copy_from_slice(&payload_len.to_le_bytes());
        let mut datagram = vec![0; QRTR_HEADER_BYTES + align4(qmi.len())];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            source.node,
            source.port,
            1,
            qmi.len() as u32,
            destination.node,
            destination.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut datagram, index * 4, *value);
        }
        datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi.len()].copy_from_slice(&qmi);
        datagram
    }

    fn listener_response(
        transaction: u16,
        result: u16,
        error: u16,
        state: Option<i32>,
        source: Address,
        destination: Address,
    ) -> Vec<u8> {
        let mut qmi = vec![2];
        qmi.extend_from_slice(&transaction.to_le_bytes());
        qmi.extend_from_slice(&codec::REGISTER_LISTENER_MESSAGE_ID.to_le_bytes());
        qmi.extend_from_slice(&[0, 0]);
        qmi.extend_from_slice(&[
            0x02,
            4,
            0,
            result as u8,
            (result >> 8) as u8,
            error as u8,
            (error >> 8) as u8,
        ]);
        if let Some(state) = state {
            qmi.extend_from_slice(&[0x10, 4, 0]);
            qmi.extend_from_slice(&state.to_le_bytes());
        }
        let payload_len = (qmi.len() - codec::QMI_HEADER_BYTES) as u16;
        qmi[5..7].copy_from_slice(&payload_len.to_le_bytes());
        let mut datagram = vec![0; QRTR_HEADER_BYTES + align4(qmi.len())];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            source.node,
            source.port,
            1,
            qmi.len() as u32,
            destination.node,
            destination.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut datagram, index * 4, *value);
        }
        datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi.len()].copy_from_slice(&qmi);
        datagram
    }

    fn state_updated_indication(
        transaction: u16,
        indication_transaction: u16,
        state: i32,
        service_name: &[u8],
        source: Address,
        destination: Address,
    ) -> Vec<u8> {
        let mut qmi = vec![4];
        qmi.extend_from_slice(&transaction.to_le_bytes());
        qmi.extend_from_slice(&codec::STATE_UPDATED_INDICATION_MESSAGE_ID.to_le_bytes());
        qmi.extend_from_slice(&[0, 0]);
        qmi.extend_from_slice(&[0x01, 4, 0]);
        qmi.extend_from_slice(&state.to_le_bytes());
        qmi.push(0x02);
        qmi.extend_from_slice(&(service_name.len() as u16).to_le_bytes());
        qmi.extend_from_slice(service_name);
        qmi.extend_from_slice(&[0x03, 2, 0]);
        qmi.extend_from_slice(&indication_transaction.to_le_bytes());
        let payload_len = (qmi.len() - codec::QMI_HEADER_BYTES) as u16;
        qmi[5..7].copy_from_slice(&payload_len.to_le_bytes());

        let mut datagram = vec![0; QRTR_HEADER_BYTES + align4(qmi.len())];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            source.node,
            source.port,
            1,
            qmi.len() as u32,
            destination.node,
            destination.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut datagram, index * 4, *value);
        }
        datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi.len()].copy_from_slice(&qmi);
        datagram
    }

    fn set_ack_response(
        transaction: u16,
        result: u16,
        error: u16,
        source: Address,
        destination: Address,
    ) -> Vec<u8> {
        let mut qmi = vec![2];
        qmi.extend_from_slice(&transaction.to_le_bytes());
        qmi.extend_from_slice(&codec::SET_ACK_MESSAGE_ID.to_le_bytes());
        qmi.extend_from_slice(&[0, 0]);
        qmi.extend_from_slice(&[
            0x02,
            4,
            0,
            result as u8,
            (result >> 8) as u8,
            error as u8,
            (error >> 8) as u8,
        ]);
        let payload_len = (qmi.len() - codec::QMI_HEADER_BYTES) as u16;
        qmi[5..7].copy_from_slice(&payload_len.to_le_bytes());
        let mut datagram = vec![0; QRTR_HEADER_BYTES + align4(qmi.len())];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            source.node,
            source.port,
            0,
            qmi.len() as u32,
            destination.node,
            destination.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut datagram, index * 4, *value);
        }
        datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi.len()].copy_from_slice(&qmi);
        datagram
    }

    #[test]
    fn accepts_only_exact_servreg_notif_server_instance() {
        let mut client = client();
        assert_eq!(client.state(), State::Searching);
        assert_eq!(client.observe_server(1, SERVER), Ok(true));
        assert_eq!(client.observe_server(1, SERVER), Ok(false));
        assert_eq!(client.endpoint(), Some(SERVER.address));
        assert_eq!(client.state(), State::Ready);

        let wrong_service = Server {
            service: 0x45,
            ..SERVER
        };
        assert_eq!(
            client.observe_server(1, wrong_service),
            Err(Error::InvalidServer)
        );
        let wrong_instance = Server {
            instance: 1,
            ..SERVER
        };
        assert_eq!(
            client.observe_server(1, wrong_instance),
            Err(Error::InvalidServer)
        );
        let local_endpoint = Server {
            address: LOCAL,
            ..SERVER
        };
        assert_eq!(
            client.observe_server(1, local_endpoint),
            Err(Error::InvalidServer)
        );
        let other = Server {
            address: Address { node: 0, port: 4 },
            ..SERVER
        };
        assert_eq!(
            client.observe_server(1, other),
            Err(Error::EndpointConflict)
        );
    }

    #[test]
    fn builds_one_exact_wlan_domain_query_and_commits_after_capacity_check() {
        let mut client = client();
        assert_eq!(
            client.begin_query(1, QUERY_DATAGRAM_BYTES),
            Err(Error::NoEndpoint)
        );
        client.observe_server(1, SERVER).unwrap();
        assert_eq!(
            client.begin_query(1, QUERY_DATAGRAM_BYTES - 1),
            Err(Error::OutputTooSmall)
        );
        assert_eq!(client.state(), State::Ready);
        let query = client.begin_query(1, QUERY_DATAGRAM_BYTES).unwrap();
        assert_eq!(
            (
                query.generation,
                query.source,
                query.destination,
                query.transaction,
                query.message_id
            ),
            (1, LOCAL, SERVER.address, 1, codec::QUERY_STATE_MESSAGE_ID)
        );
        assert_eq!(query.len, QUERY_DATAGRAM_BYTES);
        assert_eq!(word(&query.bytes, 0), QRTR_VERSION_V1);
        assert_eq!(word(&query.bytes, 4), QRTR_PACKET_DATA);
        assert_eq!(word(&query.bytes, 20), codec::WLAN_PD_QUERY_BYTES as u32);
        assert_eq!(
            &query.bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + codec::WLAN_PD_QUERY_BYTES],
            b"\x00\x01\x00\x21\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd"
        );
        assert_eq!(
            &query.bytes[QRTR_HEADER_BYTES + codec::WLAN_PD_QUERY_BYTES..query.len],
            &[0]
        );
        assert_eq!(client.state(), State::Pending);
        assert_eq!(
            client.begin_query(1, QUERY_DATAGRAM_BYTES),
            Err(Error::Busy)
        );
    }

    #[test]
    fn consumes_only_matching_response_and_preserves_invalid_pending_request() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client.begin_query(1, QUERY_DATAGRAM_BYTES).unwrap();

        let wrong_source = response(1, Some(0x1fff_ffff), Address { node: 0, port: 9 }, LOCAL);
        assert_eq!(
            client.accept_response(1, &wrong_source),
            Err(Error::WrongEndpoint)
        );
        let wrong_transaction = response(2, Some(0x1fff_ffff), SERVER.address, LOCAL);
        assert_eq!(
            client.accept_response(1, &wrong_transaction),
            Err(Error::Wire(codec::Error::WrongTransaction))
        );
        assert_eq!(client.state(), State::Pending);

        let valid = response(1, Some(0x1fff_ffff), SERVER.address, LOCAL);
        let outcome = client.accept_response(1, &valid).unwrap();
        assert_eq!(
            outcome.query,
            Some(codec::QueryStateResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Up)
            })
        );
        assert_eq!(outcome.confirm_rx, true);
        assert_eq!(outcome.trace.transaction, 1);
        assert_eq!(outcome.trace.message_id, codec::QUERY_STATE_MESSAGE_ID);
        assert_eq!(outcome.trace.source, LOCAL);
        assert_eq!(outcome.trace.destination, SERVER.address);
        assert_eq!(client.state(), State::Complete);
        assert_eq!(
            client.outcome().unwrap().current_state(),
            Some(ServiceState::Up)
        );
        assert_eq!(
            client.begin_query(1, QUERY_DATAGRAM_BYTES),
            Err(Error::AlreadyComplete)
        );
        assert_eq!(
            client.accept_response(1, &valid),
            Err(Error::NoPendingRequest)
        );
    }

    #[test]
    fn restart_pd_is_one_shot_and_requires_successful_query_and_listener() {
        let mut up_client = client();
        up_client.observe_server(1, SERVER).unwrap();
        up_client.begin_query(1, QUERY_DATAGRAM_BYTES).unwrap();
        let up = response(1, Some(0x1fff_ffff), SERVER.address, LOCAL);
        up_client.accept_response(1, &up).unwrap();
        assert_eq!(
            up_client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
        assert_eq!(
            up_client.begin_register_listener(1, MAX_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );

        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client.begin_query(1, QUERY_DATAGRAM_BYTES).unwrap();
        let uninit = response(1, Some(0x7fff_ffff), SERVER.address, LOCAL);
        client.accept_response(1, &uninit).unwrap();
        assert_eq!(
            client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
        assert_eq!(client.state(), State::Complete);

        assert_eq!(
            client.begin_register_listener(1, MAX_DATAGRAM_BYTES - 1),
            Err(Error::OutputTooSmall)
        );
        let listener = client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        assert_eq!(listener.transaction, 2);
        assert_eq!(listener.message_id, codec::REGISTER_LISTENER_MESSAGE_ID);
        assert_eq!(listener.len, MAX_DATAGRAM_BYTES);
        assert_eq!(
            &listener.bytes
                [QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + codec::WLAN_PD_REGISTER_LISTENER_BYTES],
            b"\x00\x02\x00\x20\x00\x18\x00\x01\x01\x00\x01\x02\x11\x00msm/modem/wlan_pd"
        );
        let listener_reply = listener_response(2, 0, 0, Some(0x7fff_ffff), SERVER.address, LOCAL);
        let registered = client.accept_response(1, &listener_reply).unwrap();
        assert_eq!(client.state(), State::ListenerComplete);
        assert_eq!(
            registered.listener,
            Some(codec::RegisterListenerResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Uninit),
            })
        );

        let restart = client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES).unwrap();
        assert_eq!(restart.transaction, 3);
        assert_eq!(restart.message_id, codec::RESTART_PD_MESSAGE_ID);
        assert_eq!(restart.len, QUERY_DATAGRAM_BYTES);
        assert_eq!(
            &restart.bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + codec::WLAN_PD_RESTART_PD_BYTES],
            b"\x00\x03\x00\x24\x00\x14\x00\x01\x11\x00msm/modem/wlan_pd"
        );
        assert_eq!(client.state(), State::RestartPending);
        assert_eq!(
            client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::Busy)
        );

        let reply = restart_response(3, 0, 0, SERVER.address, LOCAL);
        let outcome = client.accept_response(1, &reply).unwrap();
        assert_eq!(
            outcome.query,
            Some(codec::QueryStateResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Uninit),
            })
        );
        assert_eq!(
            outcome.listener,
            Some(codec::RegisterListenerResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Uninit),
            })
        );
        assert_eq!(
            outcome.restart,
            Some(codec::RestartPdResponse {
                result: 0,
                error: 0
            })
        );
        assert_eq!(outcome.trace.message_id, codec::RESTART_PD_MESSAGE_ID);
        assert_eq!(client.state(), State::RestartComplete);
        assert_eq!(
            client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
    }

    #[test]
    fn listener_first_uses_its_state_for_one_shot_restart() {
        let mut client = Client::new(LOCAL, 1).unwrap();
        client.observe_server(1, SERVER).unwrap();
        let listener = client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        assert_eq!(listener.transaction, 1);
        assert_eq!(listener.message_id, codec::REGISTER_LISTENER_MESSAGE_ID);
        let reply = listener_response(1, 0, 0, Some(0x7fff_ffff), SERVER.address, LOCAL);
        let outcome = client.accept_response(1, &reply).unwrap();
        assert_eq!(client.state(), State::ListenerComplete);
        assert_eq!(outcome.query, None);
        assert_eq!(outcome.current_result(), Some((0, 0)));
        assert_eq!(outcome.current_state(), Some(ServiceState::Uninit));

        let restart = client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES).unwrap();
        assert_eq!(restart.transaction, 2);
        assert_eq!(restart.message_id, codec::RESTART_PD_MESSAGE_ID);
        let reply = restart_response(2, 0, 0, SERVER.address, LOCAL);
        let outcome = client.accept_response(1, &reply).unwrap();
        assert_eq!(client.state(), State::RestartComplete);
        assert_eq!(outcome.query, None);
        assert_eq!(outcome.listener.unwrap().state, Some(ServiceState::Uninit));
        assert_eq!(outcome.restart.unwrap().result, 0);
        assert!(outcome.restart_succeeded());
        assert_eq!(
            client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
    }

    #[test]
    fn state_update_is_acknowledged_once_and_latest_state_gates_restart() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        client
            .accept_response(
                1,
                &listener_response(1, 0, 0, Some(0x7fff_ffff), SERVER.address, LOCAL),
            )
            .unwrap();

        let indication = state_updated_indication(
            0x55aa,
            0x1234,
            0x1fff_ffff,
            codec::WLAN_PD_DOMAIN_NAME,
            SERVER.address,
            LOCAL,
        );
        let update = client
            .accept_state_updated_indication(1, &indication)
            .unwrap();
        assert_eq!(
            update,
            StateUpdate {
                qmi_transaction: 0x55aa,
                state: ServiceState::Up,
                indication_transaction: 0x1234,
                confirm_rx: true,
            }
        );
        let observed = client.outcome().unwrap();
        assert_eq!(observed.current_state(), Some(ServiceState::Up));
        assert_eq!(
            observed.trace.message_id,
            codec::STATE_UPDATED_INDICATION_MESSAGE_ID
        );
        assert_eq!(observed.trace.transaction, 0x55aa);
        assert_eq!(
            &observed.trace.response[..usize::from(observed.trace.response_len)],
            &indication[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + word(&indication, 20) as usize]
        );

        assert_eq!(
            client.begin_state_updated_ack(1, MAX_DATAGRAM_BYTES - 1),
            Err(Error::OutputTooSmall)
        );
        assert_eq!(client.state(), State::ListenerComplete);
        let ack = client
            .begin_state_updated_ack(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        assert_eq!(ack.transaction, 2);
        assert_eq!(ack.message_id, codec::SET_ACK_MESSAGE_ID);
        assert_eq!(ack.len, MAX_DATAGRAM_BYTES);
        let ack_message = crate::qcom_qmi::Message::decode(
            &ack.bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + codec::WLAN_PD_SET_ACK_REQUEST_BYTES],
        )
        .unwrap();
        assert_eq!(ack_message.kind, crate::qcom_qmi::Kind::Request);
        assert_eq!(ack_message.transaction, 2);
        assert_eq!(ack_message.message_id, codec::SET_ACK_MESSAGE_ID);
        assert_eq!(
            ack_message.required(0x01).unwrap(),
            codec::WLAN_PD_DOMAIN_NAME
        );
        assert_eq!(
            ack_message.required(0x02).unwrap(),
            &0x1234u16.to_le_bytes()
        );
        assert_eq!(client.state(), State::AckPending);
        assert_eq!(
            client.begin_state_updated_ack(1, MAX_DATAGRAM_BYTES),
            Err(Error::Busy)
        );

        let acknowledged = client
            .accept_response(1, &set_ack_response(2, 0, 0, SERVER.address, LOCAL))
            .unwrap();
        assert_eq!(client.state(), State::ListenerComplete);
        assert!(acknowledged.state_update_acknowledged());
        assert_eq!(acknowledged.current_state(), Some(ServiceState::Up));
        assert_eq!(
            acknowledged.set_ack,
            Some(codec::SetAckResponse {
                result: 0,
                error: 0,
            })
        );
        assert_eq!(acknowledged.trace.message_id, codec::SET_ACK_MESSAGE_ID);
        assert_eq!(
            client.begin_state_updated_ack(1, MAX_DATAGRAM_BYTES),
            Err(Error::NoPendingIndication)
        );
        assert_eq!(
            client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
    }

    #[test]
    fn invalid_or_unregistered_state_updates_are_not_accepted() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        let indication = state_updated_indication(
            1,
            2,
            0x7fff_ffff,
            codec::WLAN_PD_DOMAIN_NAME,
            SERVER.address,
            LOCAL,
        );
        assert_eq!(
            client.accept_state_updated_indication(1, &indication),
            Err(Error::ListenerRequired)
        );

        client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        client
            .accept_response(
                1,
                &listener_response(1, 1, 0x20, Some(0x7fff_ffff), SERVER.address, LOCAL),
            )
            .unwrap();
        assert_eq!(
            client.accept_state_updated_indication(1, &indication),
            Err(Error::ListenerRequired)
        );
        assert_eq!(client.state(), State::ListenerComplete);

        let mut second_client = Client::new(LOCAL, 1).unwrap();
        second_client.observe_server(1, SERVER).unwrap();
        second_client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        second_client
            .accept_response(
                1,
                &listener_response(1, 0, 0, Some(0x7fff_ffff), SERVER.address, LOCAL),
            )
            .unwrap();
        let wrong_endpoint = state_updated_indication(
            2,
            3,
            0x7fff_ffff,
            codec::WLAN_PD_DOMAIN_NAME,
            Address { node: 0, port: 4 },
            LOCAL,
        );
        assert_eq!(
            second_client.accept_state_updated_indication(1, &wrong_endpoint),
            Err(Error::WrongEndpoint)
        );
        let wrong_name = state_updated_indication(
            2,
            3,
            0x7fff_ffff,
            b"msm/modem/other_pd",
            SERVER.address,
            LOCAL,
        );
        assert_eq!(
            second_client.accept_state_updated_indication(1, &wrong_name),
            Err(Error::WrongServiceName)
        );
        let mut bad_padding = indication;
        *bad_padding.last_mut().unwrap() = 0xff;
        assert_eq!(
            second_client.accept_state_updated_indication(1, &bad_padding),
            Err(Error::InvalidDatagram)
        );
        assert_eq!(second_client.outcome().unwrap().state_update, None);
        assert_eq!(second_client.state(), State::ListenerComplete);
    }

    #[test]
    fn failed_state_update_ack_is_not_retried_or_followed_by_another_update() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        client
            .accept_response(
                1,
                &listener_response(1, 0, 0, Some(0x7fff_ffff), SERVER.address, LOCAL),
            )
            .unwrap();
        let indication = state_updated_indication(
            2,
            3,
            0x7fff_ffff,
            codec::WLAN_PD_DOMAIN_NAME,
            SERVER.address,
            LOCAL,
        );
        client
            .accept_state_updated_indication(1, &indication)
            .unwrap();
        client
            .begin_state_updated_ack(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        let failed = client
            .accept_response(1, &set_ack_response(2, 1, 0x2d, SERVER.address, LOCAL))
            .unwrap();
        assert_eq!(client.state(), State::AckFailed);
        assert!(!failed.state_update_acknowledged());
        assert_eq!(
            client.begin_state_updated_ack(1, MAX_DATAGRAM_BYTES),
            Err(Error::ListenerRequired)
        );
        assert_eq!(
            client.accept_state_updated_indication(1, &indication),
            Err(Error::Busy)
        );
        assert_eq!(
            client.begin_restart_pd(1, QUERY_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
    }

    #[test]
    fn msa_followup_gate_requires_an_explicit_successful_restart_response() {
        let mut outcome = Outcome {
            query: None,
            listener: Some(codec::RegisterListenerResponse {
                result: 0,
                error: 0,
                state: Some(ServiceState::Uninit),
            }),
            restart: None,
            state_update: None,
            set_ack: None,
            confirm_rx: false,
            trace: WireTrace::EMPTY,
        };
        assert!(!outcome.restart_succeeded());
        outcome.restart = Some(codec::RestartPdResponse {
            result: 0,
            error: 0,
        });
        assert!(outcome.restart_succeeded());
        outcome.restart = Some(codec::RestartPdResponse {
            result: 1,
            error: 9,
        });
        assert!(!outcome.restart_succeeded());
    }

    #[test]
    fn invalid_listener_handle_and_changed_state_block_pd_restart() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client.begin_query(1, MAX_DATAGRAM_BYTES).unwrap();
        let uninit = response(1, Some(0x7fff_ffff), SERVER.address, LOCAL);
        client.accept_response(1, &uninit).unwrap();
        client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();

        // The peer may include a state TLV even on a failed common response.
        // Preserve the raw bytes in `listener`, but do not let this value
        // replace the earlier successful QUERY_STATE result.
        let rejected = listener_response(
            2,
            1,
            0x20,
            Some(0xc04f_d938u32 as i32),
            SERVER.address,
            LOCAL,
        );
        let outcome = client.accept_response(1, &rejected).unwrap();
        assert_eq!(client.state(), State::ListenerComplete);
        assert_eq!(
            outcome.listener,
            Some(codec::RegisterListenerResponse {
                result: 1,
                error: 0x20,
                state: Some(ServiceState::Unknown(0xc04f_d938u32 as i32)),
            })
        );
        assert_eq!(outcome.current_result(), Some((1, 0x20)));
        assert_eq!(outcome.current_state(), Some(ServiceState::Uninit));
        assert_eq!(
            client.begin_restart_pd(1, MAX_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
    }

    #[test]
    fn invalid_data_format_listener_reply_retains_exact_wire_trace_and_blocks_restart() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        let request = client
            .begin_register_listener(1, MAX_DATAGRAM_BYTES)
            .unwrap();
        assert_eq!(request.transaction, 1);
        assert_eq!(request.message_id, codec::REGISTER_LISTENER_MESSAGE_ID);

        let reply = listener_response(1, 1, 0x2d, None, SERVER.address, LOCAL);
        let qmi_reply_len = word(&reply, 20) as usize;
        let outcome = client.accept_response(1, &reply).unwrap();
        assert_eq!(outcome.current_result(), Some((1, 0x2d)));
        assert_eq!(outcome.current_state(), None);
        assert_eq!(outcome.trace.source, LOCAL);
        assert_eq!(outcome.trace.destination, SERVER.address);
        assert_eq!(outcome.trace.transaction, 1);
        assert_eq!(
            outcome.trace.message_id,
            codec::REGISTER_LISTENER_MESSAGE_ID
        );
        assert_eq!(
            &outcome.trace.request[..usize::from(outcome.trace.request_len)],
            b"\x00\x01\x00\x20\x00\x18\x00\x01\x01\x00\x01\x02\x11\x00msm/modem/wlan_pd"
        );
        assert_eq!(
            &outcome.trace.response[..usize::from(outcome.trace.response_len)],
            &reply[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + qmi_reply_len]
        );
        assert_eq!(
            client.begin_restart_pd(1, MAX_DATAGRAM_BYTES),
            Err(Error::RestartRequiresUninitialized)
        );
    }

    #[test]
    fn failed_servreg_response_never_exposes_its_optional_state_as_current() {
        let outcome = Outcome {
            query: None,
            listener: Some(codec::RegisterListenerResponse {
                result: 1,
                error: 0x20,
                state: Some(ServiceState::Unknown(0xc04f_d938u32 as i32)),
            }),
            restart: None,
            state_update: None,
            set_ack: None,
            confirm_rx: false,
            trace: WireTrace::EMPTY,
        };
        assert_eq!(outcome.current_result(), Some((1, 0x20)));
        assert_eq!(outcome.current_state(), None);
    }

    #[test]
    fn malformed_qrtr_and_qmi_replies_do_not_consume_the_query() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client.begin_query(1, QUERY_DATAGRAM_BYTES).unwrap();
        let mut invalid = response(1, None, SERVER.address, LOCAL);
        invalid[47] = 1; // Nonzero QRTR padding.
        assert_eq!(
            client.accept_response(1, &invalid),
            Err(Error::InvalidDatagram)
        );
        assert_eq!(client.state(), State::Pending);

        let mut invalid = response(1, None, SERVER.address, LOCAL);
        put(&mut invalid, 4, 4); // Not a QRTR DATA packet.
        assert_eq!(
            client.accept_response(1, &invalid),
            Err(Error::InvalidDatagram)
        );
        assert_eq!(client.state(), State::Pending);
    }

    #[test]
    fn service_removal_and_generation_change_invalidate_pending_work() {
        let mut client = client();
        client.observe_server(1, SERVER).unwrap();
        client.begin_query(1, QUERY_DATAGRAM_BYTES).unwrap();
        let stale_remove = Server {
            address: Address { node: 0, port: 4 },
            ..SERVER
        };
        assert_eq!(client.remove_server(1, stale_remove), Ok(false));
        assert_eq!(client.state(), State::Pending);
        assert_eq!(client.begin_generation(1), Err(Error::InvalidGeneration));
        client.begin_generation(2).unwrap();
        assert_eq!(client.state(), State::Searching);
        assert_eq!(client.accept_response(1, &[]), Err(Error::StaleGeneration));
        assert_eq!(client.endpoint(), None);

        client.observe_server(2, SERVER).unwrap();
        client.begin_query(2, QUERY_DATAGRAM_BYTES).unwrap();
        assert_eq!(client.remove_server(2, SERVER), Ok(true));
        assert_eq!(client.state(), State::Searching);
        assert_eq!(client.outcome(), None);
    }
}
