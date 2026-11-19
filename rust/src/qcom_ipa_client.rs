//! One-flight IPA `INIT_MODEM_DRIVER` client over bounded QRTR/QMI bytes.
//!
//! The caller supplies the exact discovered modem service and an already
//! validated IPA resource snapshot. This module only builds and correlates one
//! transaction. A received QMI success is not evidence that WLAN is ready, and
//! this client owns no transport, IPA hardware, memory, power, or radio state.
#![forbid(unsafe_code)]

use crate::qcom_ipa::{self as ipa, InitRequestFields, InitResponse};
use crate::qcom_ipa_service::PostInitProof;
use crate::qcom_qmi_router::Address;
use crate::qcom_qrtr_control::{self as control, Server};

pub const QRTR_HEADER_BYTES: usize = 32;
pub const MAX_QMI_BYTES: usize = ipa::MAX_QMI_BYTES;
pub const MAX_DATAGRAM_BYTES: usize = QRTR_HEADER_BYTES + ((MAX_QMI_BYTES + 3) & !3);

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
    AlreadyRequested,
    TransactionExhausted,
    OutputTooSmall,
    InvalidQrtr,
    WrongEndpoint,
    NoPendingRequest,
    UncertainOutcome,
    Codec(ipa::Error),
}

impl From<ipa::Error> for Error {
    fn from(error: ipa::Error) -> Self {
        Self::Codec(error)
    }
}

/// Generation-bound proof that the trusted AP IPA provider completed its
/// source-matched post-init path and captured the runtime fields for the
/// modem's `INIT_MODEM_DRIVER` request. The native runtime must obtain this
/// from that provider; a parsed or caller-constructed profile is not enough.
pub struct InitPermit {
    generation: u64,
    fields: InitRequestFields,
}

impl InitPermit {
    /// Mint only from the coordinator's proof of completed `ipa3_post_init()`
    /// and the runtime fields captured for that same generation.
    #[allow(dead_code)] // The native IPA provider is not implemented yet.
    pub(crate) fn after_post_init(
        proof: &PostInitProof,
        fields: InitRequestFields,
    ) -> Result<Self, Error> {
        let generation = proof.generation();
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        let mut encoded = [0; MAX_QMI_BYTES];
        fields.encode(1, &mut encoded)?;
        Ok(Self { generation, fields })
    }

    /// Read the validated fields only for the permit's matching owner
    /// generation. Construction remains crate-private to the IPA provider.
    pub fn fields(&self, generation: u64) -> Result<&InitRequestFields, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        Ok(&self.fields)
    }

    #[cfg(test)]
    pub(crate) fn test_after_post_init(generation: u64) -> Self {
        let fields = crate::sunfish_ipa_profile::init_request_fields(
            crate::sunfish_ipa_profile::RuntimeSnapshot {
                shared_mem_baddr_units: 0x10,
                shared_mem_size_units: 0x400,
                apps_wan_cons_endpoint: Some(crate::sunfish_ipa_profile::APPS_WAN_CONS_ENDPOINT),
                ipa_uc_loaded: false,
            },
        )
        .expect("valid synthetic IPA test snapshot");
        let proof = PostInitProof::test_after_post_init(generation);
        Self::after_post_init(&proof, fields).expect("nonzero test generation")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Searching,
    Ready,
    Pending,
    ResponseReceived,
    /// The modem accepted `INIT_MODEM_DRIVER` but requested its separate
    /// `INIT_MODEM_DRIVER_CMPLT` exchange. This client cannot complete that
    /// server-side transaction, so the IPA handshake remains explicitly open.
    ModemDriverCompletionPending,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    generation: u64,
    endpoint: Address,
    transaction_id: u16,
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
pub struct ResponseOutcome {
    /// The response's result/error are preserved, including remote failures.
    pub response: InitResponse,
    /// The transport caller remains responsible for QRTR receive confirmation.
    pub confirm_rx: bool,
}

pub struct Client {
    local: Address,
    generation: u64,
    endpoint: Option<Address>,
    pending: Option<Pending>,
    next_transaction_id: u32,
    request_used: bool,
    state: State,
    outcome: Option<ResponseOutcome>,
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
    /// The local QRTR address must come from trusted owner configuration and
    /// be reserved separately from any locally advertised service endpoint.
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
            request_used: false,
            state: State::Searching,
            outcome: None,
        })
    }

    pub fn local_address(&self) -> Address {
        self.local
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn endpoint(&self) -> Option<Address> {
        self.endpoint
    }

    pub fn pending_transaction_id(&self) -> Option<u16> {
        self.pending.map(|pending| pending.transaction_id)
    }

    pub fn outcome(&self) -> Option<ResponseOutcome> {
        self.outcome
    }

    /// Bind only the exact IPA service/version/instance announcement. The
    /// caller should pass a record from the trusted QRTR controller's unique
    /// endpoint lookup; a second distinct server is rejected.
    pub fn observe_server(&mut self, generation: u64, server: Server) -> Result<bool, Error> {
        self.check_generation(generation)?;
        if self.state == State::Indeterminate {
            return Err(Error::UncertainOutcome);
        }
        if server.service != ipa::QRTR_SERVICE_ID
            || server.instance != ipa::QRTR_VERSION_INSTANCE
            || !valid_node(server.address.node)
            || !valid_port(server.address.port)
            || server.address == self.local
        {
            return Err(Error::InvalidServer);
        }
        match self.endpoint {
            Some(current) if current == server.address => Ok(false),
            Some(_) => Err(Error::EndpointConflict),
            None => {
                self.endpoint = Some(server.address);
                if !self.request_used {
                    self.state = State::Ready;
                }
                Ok(true)
            }
        }
    }

    /// Remove only the matching server. Losing it while a request is pending
    /// leaves the outcome ambiguous, so this generation cannot retry.
    pub fn remove_server(&mut self, generation: u64, server: Server) -> Result<bool, Error> {
        self.check_generation(generation)?;
        if server.service != ipa::QRTR_SERVICE_ID
            || server.instance != ipa::QRTR_VERSION_INSTANCE
            || self.endpoint != Some(server.address)
        {
            return Ok(false);
        }
        self.endpoint = None;
        if self.pending.take().is_some() {
            self.outcome = None;
            self.state = State::Indeterminate;
        } else if !self.request_used {
            self.state = State::Searching;
        }
        Ok(true)
    }

    /// Encode and prepare one IPA request. Caller-supplied fields must come
    /// from a source-validated live IPA snapshot; this function invents none.
    /// No transaction state changes until the whole datagram fits.
    pub fn begin_init_modem_driver(
        &mut self,
        generation: u64,
        fields: &InitRequestFields,
        output_capacity: usize,
    ) -> Result<OutboundRequest, Error> {
        self.check_generation(generation)?;
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        if self.request_used {
            return Err(Error::AlreadyRequested);
        }
        if self.state == State::ResponseReceived {
            return Err(Error::AlreadyRequested);
        }
        if self.state != State::Ready {
            return Err(Error::NoEndpoint);
        }
        let endpoint = self.endpoint.ok_or(Error::NoEndpoint)?;
        let transaction_id =
            u16::try_from(self.next_transaction_id).map_err(|_| Error::TransactionExhausted)?;
        let mut qmi = [0u8; MAX_QMI_BYTES];
        let qmi_len = fields.encode(transaction_id, &mut qmi)?;
        let padded_qmi_len = align4(qmi_len);
        let datagram_len = QRTR_HEADER_BYTES + padded_qmi_len;
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
        });
        self.request_used = true;
        self.next_transaction_id += 1;
        self.outcome = None;
        self.state = State::Pending;
        Ok(OutboundRequest {
            generation,
            source: self.local,
            destination: endpoint,
            transaction_id,
            message_id: ipa::INIT_MODEM_DRIVER_MESSAGE_ID,
            bytes,
            len: datagram_len,
        })
    }

    /// Accept only a padded QRTR-v1 data response from the exact pending peer
    /// to this local endpoint. Invalid or unrelated packets leave the request
    /// pending. QMI result/error are returned unchanged and never imply WLAN
    /// readiness.
    pub fn accept_init_modem_driver_response(
        &mut self,
        generation: u64,
        datagram: &[u8],
    ) -> Result<ResponseOutcome, Error> {
        self.check_generation(generation)?;
        let pending = self.pending.ok_or(Error::NoPendingRequest)?;
        if pending.generation != generation {
            return Err(Error::StaleGeneration);
        }
        if datagram.len() < QRTR_HEADER_BYTES
            || datagram.len() > MAX_DATAGRAM_BYTES
            || datagram.len() & 3 != 0
        {
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
        let response = ipa::decode_init_response(
            &datagram[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + payload_len],
            pending.transaction_id,
        )?;
        let outcome = ResponseOutcome {
            response,
            confirm_rx: word(datagram, 16) == 1,
        };
        self.pending = None;
        self.outcome = Some(outcome);
        self.state = if outcome.response.result == 0
            && outcome.response.error == 0
            && outcome.response.modem_driver_init_pending == Some(true)
        {
            State::ModemDriverCompletionPending
        } else {
            State::ResponseReceived
        };
        Ok(outcome)
    }

    /// Advance only after an external owner proves transport reset/quiescence.
    /// Transaction IDs never reset, preventing stale replies from matching a
    /// later logical generation.
    pub fn begin_generation(&mut self, generation: u64) -> Result<(), Error> {
        if generation <= self.generation {
            return Err(Error::InvalidGeneration);
        }
        self.generation = generation;
        self.endpoint = None;
        self.pending = None;
        self.outcome = None;
        self.request_used = false;
        self.state = State::Searching;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sunfish_ipa_profile::{self as sunfish_ipa, RuntimeSnapshot};

    const GENERATION: u64 = 7;
    const LOCAL: Address = Address {
        node: 1,
        port: 16_450,
    };
    const REMOTE: Address = Address { node: 0, port: 27 };

    fn server(address: Address) -> Server {
        Server {
            service: ipa::QRTR_SERVICE_ID,
            instance: ipa::QRTR_VERSION_INSTANCE,
            address,
        }
    }

    fn client() -> Client {
        Client::new(LOCAL, GENERATION).unwrap()
    }

    #[test]
    fn init_permit_binds_validated_runtime_fields_to_one_generation() {
        let permit = InitPermit::test_after_post_init(GENERATION);
        let fields = permit.fields(GENERATION).unwrap();
        assert_eq!(fields.control_endpoint, Some(9));
        assert_eq!(fields.ipa_shared_memory_end_exclusive, 0x2080);
        assert_eq!(permit.fields(GENERATION + 1), Err(Error::StaleGeneration));
        assert!(matches!(
            InitPermit::after_post_init(&PostInitProof::test_after_post_init(0), *fields,),
            Err(Error::InvalidGeneration)
        ));
    }

    fn fields() -> InitRequestFields {
        InitRequestFields {
            platform_type: ipa::PlatformType::MsmAndroid,
            ipa_shared_memory_end_exclusive: 0x10000,
            header_table: Some(ipa::OffsetRange {
                start: 0x100,
                end_inclusive: 0x1ff,
            }),
            v4_route: ipa::RouteTable {
                start: 0x200,
                num_indices: 8,
            },
            v6_route: ipa::RouteTable {
                start: 0x300,
                num_indices: 12,
            },
            v4_filter_start: 0x400,
            v6_filter_start: 0x500,
            modem_memory: Some(ipa::MemoryBlock {
                start: 0x600,
                size: 0x800,
            }),
            control_endpoint: Some(7),
            is_ssr_bootup: Some(false),
            header_processing_table: Some(ipa::OffsetRange {
                start: 0x1000,
                end_inclusive: 0x10ff,
            }),
            compression_table: Some(ipa::OffsetRange {
                start: 0x1100,
                end_inclusive: 0x11ff,
            }),
            hash_tables: Some(ipa::HashTables {
                v4_route: ipa::RouteTable {
                    start: 0x1200,
                    num_indices: 8,
                },
                v6_route: ipa::RouteTable {
                    start: 0x1300,
                    num_indices: 12,
                },
                v4_filter_start: 0x1400,
                v6_filter_start: 0x1500,
            }),
            stats_quota: ipa::MemorySpan {
                start: 0x1600,
                size: 0x100,
            },
            drop_stats: ipa::MemorySpan {
                start: 0x1700,
                size: 0,
            },
        }
    }

    fn response_datagram(
        local: Address,
        remote: Address,
        transaction: u16,
        result: u16,
        error: u16,
        confirm_rx: u32,
    ) -> [u8; QRTR_HEADER_BYTES + 16] {
        let mut bytes = [0u8; QRTR_HEADER_BYTES + 16];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            remote.node,
            remote.port,
            confirm_rx,
            14,
            local.node,
            local.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut bytes, index * 4, *value);
        }
        bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + 7].copy_from_slice(&[
            2,
            transaction as u8,
            (transaction >> 8) as u8,
            ipa::INIT_MODEM_DRIVER_MESSAGE_ID as u8,
            (ipa::INIT_MODEM_DRIVER_MESSAGE_ID >> 8) as u8,
            7,
            0,
        ]);
        bytes[QRTR_HEADER_BYTES + 7..QRTR_HEADER_BYTES + 14].copy_from_slice(&[
            0x02,
            4,
            0,
            result as u8,
            (result >> 8) as u8,
            error as u8,
            (error >> 8) as u8,
        ]);
        bytes
    }

    fn response_datagram_with_modem_pending(
        local: Address,
        remote: Address,
        transaction: u16,
    ) -> [u8; QRTR_HEADER_BYTES + 20] {
        let mut bytes = [0u8; QRTR_HEADER_BYTES + 20];
        for (index, value) in [
            QRTR_VERSION_V1,
            QRTR_PACKET_DATA,
            remote.node,
            remote.port,
            1,
            18,
            local.node,
            local.port,
        ]
        .iter()
        .enumerate()
        {
            put(&mut bytes, index * 4, *value);
        }
        bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + 7].copy_from_slice(&[
            2,
            transaction as u8,
            (transaction >> 8) as u8,
            ipa::INIT_MODEM_DRIVER_MESSAGE_ID as u8,
            (ipa::INIT_MODEM_DRIVER_MESSAGE_ID >> 8) as u8,
            11,
            0,
        ]);
        bytes[QRTR_HEADER_BYTES + 7..QRTR_HEADER_BYTES + 14]
            .copy_from_slice(&[0x02, 4, 0, 0, 0, 0, 0]);
        bytes[QRTR_HEADER_BYTES + 14..QRTR_HEADER_BYTES + 18].copy_from_slice(&[0x12, 1, 0, 1]);
        bytes
    }

    fn ready_client() -> Client {
        let mut client = client();
        assert!(client.observe_server(GENERATION, server(REMOTE)).unwrap());
        client
    }

    #[test]
    fn sunfish_v42_profile_composes_into_one_bounded_qrtr_request() {
        let fields = sunfish_ipa::init_request_fields(RuntimeSnapshot {
            shared_mem_baddr_units: 0x10,
            shared_mem_size_units: 0x400,
            apps_wan_cons_endpoint: Some(sunfish_ipa::APPS_WAN_CONS_ENDPOINT),
            ipa_uc_loaded: false,
        })
        .unwrap();
        let mut client = ready_client();
        let request = client
            .begin_init_modem_driver(GENERATION, &fields, MAX_DATAGRAM_BYTES)
            .unwrap();

        assert_eq!(request.len, QRTR_HEADER_BYTES + 120);
        assert_eq!(word(&request.bytes, 20), 118);
        let qmi = &request.bytes[QRTR_HEADER_BYTES..QRTR_HEADER_BYTES + 118];
        assert_eq!(u16::from_le_bytes([qmi[5], qmi[6]]), 111);
        let expected_tags = [
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x1f, 0x20, 0x21, 0x22,
        ];
        let mut cursor = ipa::QMI_HEADER_BYTES;
        for tag in expected_tags {
            assert_eq!(qmi[cursor], tag);
            let value_len = u16::from_le_bytes([qmi[cursor + 1], qmi[cursor + 2]]) as usize;
            cursor += 3 + value_len;
        }
        assert_eq!(cursor, qmi.len());
        assert_eq!(
            &request.bytes[QRTR_HEADER_BYTES + 118..request.len],
            &[0, 0]
        );
        assert_eq!(client.state(), State::Pending);
    }

    #[test]
    fn lookup_requires_exact_service_version_and_instance() {
        assert_eq!(ipa::QRTR_SERVICE_ID, 49);
        assert_eq!(ipa::QRTR_VERSION_INSTANCE, 0x201);
        let mut client = client();
        let mut wrong = server(REMOTE);
        wrong.instance = 0x200;
        assert_eq!(
            client.observe_server(GENERATION, wrong),
            Err(Error::InvalidServer)
        );
        assert_eq!(client.state(), State::Searching);
        assert!(client.observe_server(GENERATION, server(REMOTE)).unwrap());
        assert!(!client.observe_server(GENERATION, server(REMOTE)).unwrap());
        assert_eq!(client.endpoint(), Some(REMOTE));
        assert_eq!(
            client.observe_server(GENERATION, server(Address { node: 0, port: 28 })),
            Err(Error::EndpointConflict)
        );
    }

    #[test]
    fn request_uses_exact_endpoint_and_does_not_commit_if_capacity_is_short() {
        let mut client = ready_client();
        assert_eq!(
            client.begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES - 1),
            Err(Error::OutputTooSmall)
        );
        assert_eq!(client.state(), State::Ready);
        assert_eq!(client.pending_transaction_id(), None);

        let request = client
            .begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        assert_eq!(request.source, LOCAL);
        assert_eq!(request.destination, REMOTE);
        assert_eq!(request.transaction_id, 1);
        assert_eq!(request.message_id, ipa::INIT_MODEM_DRIVER_MESSAGE_ID);
        assert_eq!(request.len, MAX_DATAGRAM_BYTES);
        assert_eq!(word(&request.bytes, 0), QRTR_VERSION_V1);
        assert_eq!(word(&request.bytes, 4), QRTR_PACKET_DATA);
        assert_eq!(word(&request.bytes, 8), LOCAL.node);
        assert_eq!(word(&request.bytes, 12), LOCAL.port);
        assert_eq!(word(&request.bytes, 16), 0);
        assert_eq!(word(&request.bytes, 20), ipa::MAX_QMI_BYTES as u32);
        assert_eq!(word(&request.bytes, 24), REMOTE.node);
        assert_eq!(word(&request.bytes, 28), REMOTE.port);
        assert_eq!(request.bytes[QRTR_HEADER_BYTES], 0);
        assert_eq!(
            &request.bytes[QRTR_HEADER_BYTES + 3..QRTR_HEADER_BYTES + 5],
            &ipa::INIT_MODEM_DRIVER_MESSAGE_ID.to_le_bytes()
        );
        assert_eq!(client.state(), State::Pending);
        assert_eq!(client.pending_transaction_id(), Some(1));
    }

    #[test]
    fn response_correlation_preserves_qmi_result_and_confirmation_flag() {
        let mut client = ready_client();
        client
            .begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        let bytes = response_datagram(LOCAL, REMOTE, 1, 0, 0, 1);
        let outcome = client
            .accept_init_modem_driver_response(GENERATION, &bytes)
            .unwrap();
        assert_eq!(outcome.response.result, 0);
        assert_eq!(outcome.response.error, 0);
        assert!(outcome.confirm_rx);
        assert_eq!(client.state(), State::ResponseReceived);
        assert_eq!(client.outcome(), Some(outcome));
        assert_eq!(client.pending_transaction_id(), None);
        assert!(client.remove_server(GENERATION, server(REMOTE)).unwrap());
        assert!(client.observe_server(GENERATION, server(REMOTE)).unwrap());
        assert_eq!(client.state(), State::ResponseReceived);
        assert_eq!(
            client.begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES),
            Err(Error::AlreadyRequested)
        );
    }

    #[test]
    fn successful_response_with_modem_pending_stays_incomplete() {
        let mut client = ready_client();
        client
            .begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        let bytes = response_datagram_with_modem_pending(LOCAL, REMOTE, 1);
        let outcome = client
            .accept_init_modem_driver_response(GENERATION, &bytes)
            .unwrap();
        assert_eq!(outcome.response.result, 0);
        assert_eq!(outcome.response.error, 0);
        assert_eq!(outcome.response.modem_driver_init_pending, Some(true));
        assert_eq!(client.state(), State::ModemDriverCompletionPending);
        assert_eq!(client.outcome(), Some(outcome));
        assert_eq!(client.pending_transaction_id(), None);
        assert_eq!(
            client.begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES),
            Err(Error::AlreadyRequested)
        );
    }

    #[test]
    fn remote_failure_is_recorded_without_becoming_readiness() {
        let mut client = ready_client();
        client
            .begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        let bytes = response_datagram(LOCAL, REMOTE, 1, 1, 0x17, 0);
        let outcome = client
            .accept_init_modem_driver_response(GENERATION, &bytes)
            .unwrap();
        assert_eq!((outcome.response.result, outcome.response.error), (1, 0x17));
        assert_eq!(client.state(), State::ResponseReceived);
    }

    #[test]
    fn unrelated_or_malformed_responses_leave_the_request_pending() {
        let mut client = ready_client();
        client
            .begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        let mut bytes = response_datagram(LOCAL, REMOTE, 1, 0, 0, 0);
        put(&mut bytes, 8, 0);
        put(&mut bytes, 12, 28);
        assert_eq!(
            client.accept_init_modem_driver_response(GENERATION, &bytes),
            Err(Error::WrongEndpoint)
        );
        let bytes = response_datagram(LOCAL, REMOTE, 2, 0, 0, 0);
        assert!(matches!(
            client.accept_init_modem_driver_response(GENERATION, &bytes),
            Err(Error::Codec(ipa::Error::InvalidResponse))
        ));
        let mut malformed = response_datagram(LOCAL, REMOTE, 1, 0, 0, 0);
        malformed[QRTR_HEADER_BYTES + 14] = 1;
        assert_eq!(
            client.accept_init_modem_driver_response(GENERATION, &malformed),
            Err(Error::InvalidQrtr)
        );
        assert_eq!(client.state(), State::Pending);
        assert_eq!(client.pending_transaction_id(), Some(1));
    }

    #[test]
    fn server_removal_and_new_generation_invalidate_pending_work() {
        let mut client = ready_client();
        client
            .begin_init_modem_driver(GENERATION, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        assert!(
            !client
                .remove_server(GENERATION, server(Address { node: 0, port: 28 }))
                .unwrap()
        );
        assert!(client.remove_server(GENERATION, server(REMOTE)).unwrap());
        assert_eq!(client.state(), State::Indeterminate);
        assert_eq!(client.pending_transaction_id(), None);
        assert_eq!(
            client.observe_server(GENERATION, server(REMOTE)),
            Err(Error::UncertainOutcome)
        );
        assert_eq!(
            client.accept_init_modem_driver_response(
                GENERATION,
                &response_datagram(LOCAL, REMOTE, 1, 0, 0, 0)
            ),
            Err(Error::NoPendingRequest)
        );
        assert_eq!(
            client.begin_generation(GENERATION),
            Err(Error::InvalidGeneration)
        );
        client.begin_generation(GENERATION + 1).unwrap();
        assert_eq!(client.generation(), GENERATION + 1);
        assert_eq!(client.state(), State::Searching);
        assert!(
            client
                .observe_server(GENERATION + 1, server(REMOTE))
                .unwrap()
        );
        let retry = client
            .begin_init_modem_driver(GENERATION + 1, &fields(), MAX_DATAGRAM_BYTES)
            .unwrap();
        assert_eq!(retry.transaction_id, 2);
    }

    #[test]
    fn stale_generation_and_invalid_local_addresses_fail_closed() {
        assert_eq!(
            Client::new(LOCAL, 0).map(|_| ()),
            Err(Error::InvalidGeneration)
        );
        assert_eq!(
            Client::new(Address { node: 1, port: 0 }, GENERATION).map(|_| ()),
            Err(Error::InvalidLocalAddress)
        );
        let mut client = client();
        assert_eq!(
            client.observe_server(GENERATION + 1, server(REMOTE)),
            Err(Error::StaleGeneration)
        );
    }
}
