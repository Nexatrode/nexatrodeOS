//! IPA QMI wire profile and cold-handshake bookkeeping for the working Linux
//! reference (2267e248204e7e051e0f7355298949a6440de579).
//!
//! This is separate from the downstream IPA v3 service. Plans and protocol
//! flags are data, not hardware readiness or authority to boot MPSS. A native
//! owner must retain the real IPA startup owners, publish both QRTR endpoints,
//! validate peer discovery/generation and confirm complete transport records.
//! No transport, MMIO, firmware, reset, DMA or network operation occurs here.
#![forbid(unsafe_code)]

use crate::qcom_qmi::{self as qmi, Kind, Message};
use crate::sunfish_ipa_linux_memory as memory;
use core::sync::atomic::{AtomicU64, Ordering};

pub const SERVICE: u32 = 0x31;
pub const VERSION: u32 = 1;
pub const HOST_INSTANCE: u32 = 1;
pub const MODEM_INSTANCE: u32 = 2;
pub const HOST_VERSION_INSTANCE: u32 = (HOST_INSTANCE << 8) | VERSION;
pub const MODEM_VERSION_INSTANCE: u32 = (MODEM_INSTANCE << 8) | VERSION;
pub const INDICATION_REGISTER: u16 = 0x20;
pub const INIT_DRIVER: u16 = 0x21;
pub const INIT_COMPLETE: u16 = 0x22;
/// Vendor IPA CONFIG is not handled by the pinned Linux reference server.
pub const CONFIG_REQUEST: u16 = 0x27;
pub const DRIVER_INIT_COMPLETE: u16 = 0x35;
pub const INIT_DRIVER_TIMEOUT_MS: u32 = 60_000;
pub const INIT_REQUEST_BYTES: usize = 108;
pub const COMMON_BYTES: usize = 14;
pub const MODEM_ROUTE_COUNT: u32 = 8;
pub const CONTROL_ENDPOINT: u32 = 9;
const CONTROL_PORT: u32 = 0xffff_fffe;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Geometry,
    InvalidGeneration,
    InvalidPeer,
    OwnerExhausted,
    SequenceExhausted,
    StaleGeneration,
    WrongPeer,
    WrongTransaction,
    UnsupportedMessage,
    TooLarge,
    Busy,
    AlreadyStarted,
    NotPublished,
    NoPendingPublication,
    WrongPublication,
    Failed,
    RemoteFailure { result: u16, error: u16 },
    Wire(qmi::Error),
}
impl From<qmi::Error> for Error {
    fn from(value: qmi::Error) -> Self {
        Self::Wire(value)
    }
}

/// Byte-exact INIT_DRIVER request parameters for the measured v4.2 aperture.
/// In particular, route ends are *indices*, and skip_uc_load is always present.
/// The boolean here is an encoding input, not proof that firmware is loaded.
pub struct InitRequest {
    bytes: [u8; INIT_REQUEST_BYTES],
}
impl InitRequest {
    pub fn new(
        transaction: u16,
        shared_memory_register: u32,
        hardware_uc_loaded: bool,
    ) -> Result<Self, Error> {
        let geometry = memory::geometry(shared_memory_register).map_err(|_| Error::Geometry)?;
        if geometry.offset != 0 || geometry.usable_bytes != 0x2000 {
            return Err(Error::Geometry);
        }
        let mut bytes = [0; INIT_REQUEST_BYTES];
        header(
            &mut bytes,
            0,
            transaction,
            INIT_DRIVER,
            INIT_REQUEST_BYTES - 7,
        );
        let mut at = 7;
        let mut append = |tag: u8, value: &[u8]| {
            bytes[at] = tag;
            bytes[at + 1..at + 3].copy_from_slice(&(value.len() as u16).to_le_bytes());
            bytes[at + 3..at + 3 + value.len()].copy_from_slice(value);
            at += 3 + value.len();
        };
        let region = |name| {
            memory::REGIONS
                .iter()
                .find(|r| r.name == name)
                .ok_or(Error::Geometry)
        };
        let bounds = |name| -> Result<[u8; 8], Error> {
            let r = region(name)?;
            if r.size == 0 || r.offset + r.size > geometry.usable_bytes {
                return Err(Error::Geometry);
            }
            Ok(pair(r.offset as u32, (r.offset + r.size - 1) as u32))
        };
        append(0x10, &3u32.to_le_bytes()); // MSM_ANDROID, as in the reference.
        append(0x11, &bounds("MODEM_HEADER")?);
        for (tag, name) in [(0x12, "V4_ROUTE"), (0x13, "V6_ROUTE")] {
            let r = region(name)?;
            if MODEM_ROUTE_COUNT == 0 || MODEM_ROUTE_COUNT as usize * 8 > r.size {
                return Err(Error::Geometry);
            }
            append(tag, &pair(r.offset as u32, MODEM_ROUTE_COUNT - 1));
        }
        for (tag, name) in [(0x14, "V4_FILTER"), (0x15, "V6_FILTER")] {
            append(tag, &(region(name)?.offset as u32).to_le_bytes());
        }
        let modem = region("MODEM")?;
        append(0x16, &pair(modem.offset as u32, modem.size as u32));
        append(0x17, &CONTROL_ENDPOINT.to_le_bytes());
        append(0x18, &[u8::from(hardware_uc_loaded)]);
        append(0x19, &bounds("MODEM_PROC_CTX")?);
        // v4.2 defines neither compression, hashed tables nor drop statistics.
        // The original factory omits these fields; it does not send zero TLVs.
        let quota = region("STATS_QUOTA_MODEM")?;
        append(0x1f, &(quota.offset as u32).to_le_bytes());
        append(0x20, &(quota.size as u32).to_le_bytes());
        if at != INIT_REQUEST_BYTES {
            return Err(Error::Geometry);
        }
        Ok(Self { bytes })
    }
    pub const fn bytes(&self) -> &[u8; INIT_REQUEST_BYTES] {
        &self.bytes
    }
}
fn pair(first: u32, second: u32) -> [u8; 8] {
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&first.to_le_bytes());
    bytes[4..].copy_from_slice(&second.to_le_bytes());
    bytes
}
fn header(output: &mut [u8], kind: u8, transaction: u16, id: u16, payload: usize) {
    output[0] = kind;
    output[1..3].copy_from_slice(&transaction.to_le_bytes());
    output[3..5].copy_from_slice(&id.to_le_bytes());
    output[5..7].copy_from_slice(&(payload as u16).to_le_bytes());
}
fn common(kind: u8, transaction: u16, id: u16) -> [u8; INIT_REQUEST_BYTES] {
    let mut bytes = [0; INIT_REQUEST_BYTES];
    header(&mut bytes, kind, transaction, id, 7);
    bytes[7..10].copy_from_slice(&[2, 4, 0]);
    // Standard success result and no error are already zero.
    bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address {
    pub node: u32,
    pub port: u32,
}
impl Address {
    fn valid(self) -> bool {
        self.node < CONTROL_PORT && self.port != 0 && self.port < CONTROL_PORT
    }
}
/// Linux uses two local QMI handles. Replies/indications leave the host server;
/// the INIT_DRIVER exchange uses the client. Their local ports may differ.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Socket {
    Client,
    Server,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitResponse {
    pub control_endpoint: Option<u32>,
    pub default_endpoint: Option<u32>,
    pub driver_init_pending: Option<u8>,
}
fn init_response(bytes: &[u8]) -> Result<(u16, InitResponse), Error> {
    if bytes.len() > 7 + 25 {
        return Err(Error::TooLarge);
    }
    let m = Message::decode(bytes)?;
    if m.kind != Kind::Response || m.message_id != INIT_DRIVER {
        return Err(Error::UnsupportedMessage);
    }
    m.check_fields(&[2])?;
    let (result, error) = match m.required(2)? {
        [a, b, c, d] => (u16::from_le_bytes([*a, *b]), u16::from_le_bytes([*c, *d])),
        _ => return Err(qmi::Error::InvalidTlvLength(2).into()),
    };
    // Linux's callback ignores the decoded result. Native code deliberately
    // requires explicit success before reporting the protocol milestone.
    if result != 0 || error != 0 {
        return Err(Error::RemoteFailure { result, error });
    }
    Ok((
        m.transaction,
        InitResponse {
            control_endpoint: m.optional_u32(0x10)?,
            default_endpoint: m.optional_u32(0x11)?,
            driver_init_pending: m.optional_u8(0x12)?,
        },
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Init { transaction: u16 },
    Registration,
    DriverComplete { status: u8 },
    Indication,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    sequence: u64,
    operation: Operation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(feature = "sunfish-linux-ipa-qmi")]
pub(crate) struct PublicationId {
    pub(crate) owner: u64,
    pub(crate) generation: u64,
    pub(crate) sequence: u64,
}
/// No Clone or public fields: another owner/generation cannot commit this
/// record. Preparing it has no publication or remote-readiness side effect.
#[derive(Debug)]
pub struct Prepared {
    owner: u64,
    generation: u64,
    sequence: u64,
    peer: Address,
    socket: Socket,
    bytes: [u8; INIT_REQUEST_BYTES],
    len: usize,
}
impl Prepared {
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub(crate) const fn publication_id(&self) -> PublicationId {
        PublicationId {
            owner: self.owner,
            generation: self.generation,
            sequence: self.sequence,
        }
    }
    pub const fn peer(&self) -> Address {
        self.peer
    }
    pub const fn socket(&self) -> Socket {
        self.socket
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
fn server_operation(m: &Message<'_>) -> Result<Operation, Error> {
    if m.as_bytes().len() > 7 + 20 {
        return Err(Error::TooLarge);
    }
    if m.kind != Kind::Request {
        return Err(Error::UnsupportedMessage);
    }
    match m.message_id {
        INDICATION_REGISTER => {
            m.check_fields(&[])?;
            for tag in 0x10..=0x14 {
                m.optional_u8(tag)?;
            }
            Ok(Operation::Registration)
        }
        DRIVER_INIT_COMPLETE => {
            if m.as_bytes().len() > 7 + 4 {
                return Err(Error::TooLarge);
            }
            m.check_fields(&[1])?;
            Ok(Operation::DriverComplete { status: m.u8(1)? })
        }
        _ => Err(Error::UnsupportedMessage),
    }
}
/// Wire validation for the explicitly selected Linux IPA router profile.
/// Raw optional/status bytes are not interpreted by the original handlers.
pub fn validate_server_request(message: &Message<'_>) -> Result<(), qmi::Error> {
    server_operation(message)
        .map(|_| ())
        .map_err(|error| match error {
            Error::Wire(error) => error,
            Error::TooLarge => qmi::Error::TooLarge,
            _ => qmi::Error::UnsupportedMessage,
        })
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitPhase {
    Idle,
    Prepared,
    Published(u16),
    Complete,
}
/// Diagnostic protocol flags only. `qmi_uc_ready` comes from the modem's QMI
/// request and is not the UC hardware interrupt or ipa->uc_loaded flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct State {
    pub modem_ready: bool,
    pub qmi_uc_ready: bool,
    pub indication_requested: bool,
    pub indication_sent: bool,
    pub initial_boot: bool,
    pub failed: bool,
}
pub struct ColdSession {
    owner: u64,
    generation: u64,
    modem: Address,
    next_sequence: u64,
    pending: Option<Pending>,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    server_pending: Option<Pending>,
    init_phase: InitPhase,
    state: State,
    response: Option<InitResponse>,
    driver_status: Option<u8>,
}
impl ColdSession {
    pub fn new(generation: u64, modem_service: Address) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if !modem_service.valid() {
            return Err(Error::InvalidPeer);
        }
        let mut owner = NEXT_OWNER.load(Ordering::Relaxed);
        loop {
            let next = owner.checked_add(1).ok_or(Error::OwnerExhausted)?;
            match NEXT_OWNER.compare_exchange_weak(
                owner,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => owner = current,
            }
        }
        Ok(Self {
            owner,
            generation,
            modem: modem_service,
            next_sequence: 1,
            pending: None,
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_pending: None,
            init_phase: InitPhase::Idle,
            state: State {
                modem_ready: false,
                qmi_uc_ready: false,
                indication_requested: false,
                indication_sent: false,
                initial_boot: true,
                failed: false,
            },
            response: None,
            driver_status: None,
        })
    }
    pub const fn state(&self) -> State {
        self.state
    }
    pub const fn response(&self) -> Option<InitResponse> {
        self.response
    }
    /// Only an INIT_DRIVER with its real publication committed can accept a
    /// response, independent of any subsequent server-side publication.
    #[cfg(feature = "sunfish-ipa-independent-response-experiment")]
    pub const fn init_response_can_accept(&self) -> bool {
        matches!(self.init_phase, InitPhase::Published(_)) && !self.state.failed
    }
    pub const fn driver_status(&self) -> Option<u8> {
        self.driver_status
    }
    pub const fn protocol_complete(&self) -> bool {
        !self.state.failed
            && !self.state.initial_boot
            && self.state.modem_ready
            && self.state.qmi_uc_ready
            && self.state.indication_sent
    }
    fn check(&self, generation: u64) -> Result<(), Error> {
        if generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        if self.state.failed {
            return Err(Error::Failed);
        }
        Ok(())
    }
    fn prepare(
        &mut self,
        operation: Operation,
        peer: Address,
        socket: Socket,
        bytes: [u8; INIT_REQUEST_BYTES],
        len: usize,
    ) -> Result<Prepared, Error> {
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let pending = match socket { Socket::Client => &mut self.pending, Socket::Server => &mut self.server_pending };
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let pending = &mut self.pending;
        if pending.is_some() {
            return Err(Error::Busy);
        }
        let sequence = self.next_sequence;
        self.next_sequence = sequence.checked_add(1).ok_or(Error::SequenceExhausted)?;
        *pending = Some(Pending {
            sequence,
            operation,
        });
        Ok(Prepared {
            owner: self.owner,
            generation: self.generation,
            sequence,
            peer,
            socket,
            bytes,
            len,
        })
    }
    pub fn prepare_init_driver(
        &mut self,
        generation: u64,
        transaction: u16,
        shared_memory_register: u32,
    ) -> Result<Prepared, Error> {
        self.check(generation)?;
        if self.init_phase != InitPhase::Idle {
            return Err(Error::AlreadyStarted);
        }
        // This owner is cold-only. A hardware UC interrupt is not substituted
        // for the modem's QMI completion request later in this handshake.
        let bytes = InitRequest::new(transaction, shared_memory_register, false)?.bytes;
        let prepared = self.prepare(
            Operation::Init { transaction },
            self.modem,
            Socket::Client,
            bytes,
            INIT_REQUEST_BYTES,
        )?;
        self.init_phase = InitPhase::Prepared;
        Ok(prepared)
    }
    pub fn prepare_response(
        &mut self,
        generation: u64,
        peer: Address,
        bytes: &[u8],
    ) -> Result<Prepared, Error> {
        self.check(generation)?;
        if !peer.valid() || peer.node != self.modem.node {
            return Err(Error::WrongPeer);
        }
        let m = Message::decode(bytes)?;
        let operation = server_operation(&m)?;
        self.prepare(
            operation,
            peer,
            Socket::Server,
            common(2, m.transaction, m.message_id),
            COMMON_BYTES,
        )
    }
    /// The trusted server-side transaction allocator supplies this ID. Linux
    /// allocates a transaction even for an indication; zero is legal but is
    /// not a fixed indication header value.
    pub fn prepare_indication(
        &mut self,
        generation: u64,
        transaction: u16,
    ) -> Result<Option<Prepared>, Error> {
        self.check(generation)?;
        let s = self.state;
        if !s.modem_ready || !s.qmi_uc_ready || !s.indication_requested || s.indication_sent {
            return Ok(None);
        }
        self.prepare(
            Operation::Indication,
            self.modem,
            Socket::Server,
            common(4, transaction, INIT_COMPLETE),
            COMMON_BYTES,
        )
        .map(Some)
    }
    fn pending(&self, prepared: &Prepared) -> Result<Pending, Error> {
        if prepared.owner != self.owner || prepared.generation != self.generation {
            return Err(Error::WrongPublication);
        }
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let pending = match prepared.socket { Socket::Client => self.pending, Socket::Server => self.server_pending };
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let pending = self.pending;
        let pending = pending.ok_or(Error::NoPendingPublication)?;
        if pending.sequence != prepared.sequence {
            return Err(Error::WrongPublication);
        }
        Ok(pending)
    }
    /// The transport must confirm the complete record (including its required
    /// GLINK/QRTR completion work) before calling this. The call is not itself
    /// transport proof and creates no hardware-ready capability.
    pub fn commit_publication(
        &mut self,
        generation: u64,
        prepared: &Prepared,
    ) -> Result<(), Error> {
        self.check(generation)?;
        let pending = self.pending(prepared)?;
        match pending.operation {
            Operation::Init { transaction } => self.init_phase = InitPhase::Published(transaction),
            Operation::Registration => self.state.indication_requested = true,
            Operation::DriverComplete { status } => {
                self.state.qmi_uc_ready = true;
                self.driver_status = Some(status);
            }
            Operation::Indication => self.state.indication_sent = true,
        }
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        match prepared.socket { Socket::Client => self.pending = None, Socket::Server => self.server_pending = None }
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        { self.pending = None; }
        self.observe_ready();
        Ok(())
    }
    pub fn receive_init_response(
        &mut self,
        generation: u64,
        peer: Address,
        bytes: &[u8],
    ) -> Result<(), Error> {
        self.check(generation)?;
        if peer != self.modem {
            return Err(Error::WrongPeer);
        }
        let transaction = match self.init_phase {
            InitPhase::Published(t) => t,
            _ => return Err(Error::NotPublished),
        };
        // Correlation is checked before a remote error may terminate the owner.
        // Unrelated/stale responses cannot alter even failure bookkeeping.
        let envelope = Message::decode(bytes)?;
        if envelope.kind != Kind::Response || envelope.message_id != INIT_DRIVER {
            return Err(Error::UnsupportedMessage);
        }
        if envelope.transaction != transaction {
            return Err(Error::WrongTransaction);
        }
        let response = match init_response(bytes) {
            Ok((_, response)) => response,
            Err(error @ Error::RemoteFailure { .. }) => {
                self.state.failed = true;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        self.response = Some(response);
        self.init_phase = InitPhase::Complete;
        self.state.modem_ready = true;
        self.observe_ready();
        Ok(())
    }
    fn observe_ready(&mut self) {
        if self.state.modem_ready && self.state.qmi_uc_ready && self.state.indication_sent {
            self.state.initial_boot = false;
        }
    }
    /// A partial/uncertain transport record consumes this session. No retry,
    /// rollback or ready state is inferred from a possibly delivered message.
    pub fn publication_uncertain(
        &mut self,
        generation: u64,
        prepared: &Prepared,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.pending(prepared)?;
        self.state.failed = true;
        Ok(())
    }
    /// A timeout, transport loss or native guard failure is terminal. Retained
    /// IPA owners and a new trusted generation are handled outside this codec.
    pub fn fail(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.state.failed = true;
        Ok(())
    }
    pub fn peer_lost(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.state.modem_ready = false;
        self.state.indication_requested = false;
        self.state.indication_sent = false;
        self.state.failed = true;
        // As in Linux's BYE handler, QMI UC readiness and initial_boot survive;
        // this cold-only owner still cannot perform an automatic SSR restart.
        Ok(())
    }
}
