//! Bounded AP-side IPA QMI service state for Sunfish.
//!
//! Wire and ordering reference: LineageOS sunfish kernel
//! 28f9290ae067d3e7857126eb6312720dbe9c7c80,
//! `drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c` and
//! `include/uapi/linux/ipa_qmi_service_v01.h`. This module prepares responses
//! and the master-init indication but does not publish QRTR services, own a
//! transport, or infer IPA hardware readiness. Callers commit state only
//! after the complete response or indication is confirmed published.
#![forbid(unsafe_code)]

use crate::qcom_qmi::{self as qmi, Kind, Message};
use crate::qcom_qmi_router::Address;

pub const SERVICE_ID: u32 = 0x31;
pub const SERVICE_VERSION: u32 = 1;
pub const AP_INSTANCE: u32 = 1;
/// QRTR QMI service descriptor: `(instance << 8) | version`.
pub const QRTR_VERSION_INSTANCE: u32 = (AP_INSTANCE << 8) | SERVICE_VERSION;
pub const INDICATION_REGISTER_MESSAGE_ID: u16 = 0x0020;
pub const INIT_MODEM_DRIVER_COMPLETE_MESSAGE_ID: u16 = 0x0035;
pub const MASTER_DRIVER_INIT_COMPLETE_MESSAGE_ID: u16 = 0x0022;
pub const QMI_COMMON_RESPONSE_TLV: u8 = 0x02;
pub const QMI_MESSAGE_BYTES: usize = 14;
pub const MAX_INDICATION_REGISTER_QMI_BYTES: usize = qmi::HEADER_BYTES + 20;
pub const MAX_MODEM_COMPLETE_QMI_BYTES: usize = qmi::HEADER_BYTES + 4;

const TLV_MODEM_STATUS: u8 = 0x01;
const TLV_MASTER_INIT_COMPLETE: u8 = 0x10;
const TLV_DATA_USAGE_QUOTA: u8 = 0x11;
const TLV_MHI_READY: u8 = 0x12;
const TLV_ENDPOINT_DESC: u8 = 0x13;
const TLV_BW_CHANGE: u8 = 0x14;
const CONTROL_PORT: u32 = 0xffff_fffe;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    StaleGeneration,
    InvalidPeer,
    WrongPeer,
    Busy,
    NoPendingResponse,
    NoPendingIndication,
    TokenExhausted,
    OutputTooSmall,
    InputTooLarge,
    Indeterminate,
    UnsupportedMessage,
    InvalidBoolean(u8),
    Qmi(qmi::Error),
}

impl From<qmi::Error> for Error {
    fn from(error: qmi::Error) -> Self {
        Self::Qmi(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registration {
    pub master_driver_init_complete: Option<bool>,
    pub data_usage_quota_reached: Option<bool>,
    pub mhi_ready: Option<bool>,
    pub endpoint_desc: Option<bool>,
    pub bandwidth_change: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Request {
    IndicationRegistration(Registration),
    ModemInitComplete { status: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedResponse {
    peer: Address,
    bytes: [u8; QMI_MESSAGE_BYTES],
    len: usize,
    token: u64,
    request: Request,
}

impl PreparedResponse {
    pub fn peer(&self) -> Address {
        self.peer
    }

    /// Opaque transaction token, committed only after the transport confirms
    /// both GLINK receive completion and QRTR flow-control completion.
    pub fn token(&self) -> u64 {
        self.token
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedIndication {
    peer: Address,
    bytes: [u8; QMI_MESSAGE_BYTES],
    len: usize,
    token: u64,
    generation: u64,
}

/// Opaque, crate-private proof that the ordered Sunfish IPA post-init
/// coordinator completed every hardware stage for one owner generation. The
/// native runtime passes this token through `sunfish_ipa_post_init::Ready`;
/// external callers cannot construct it or invoke the permit constructors.
pub(crate) struct PostInitProof {
    generation: u64,
}

impl PostInitProof {
    /// Crate-private token factory. Keep its production call in the one-shot
    /// post-init coordinator, after operations and snapshot validation pass.
    #[allow(dead_code)] // The production post-init coordinator is not yet wired to hardware.
    pub(crate) const fn from_coordinator(generation: u64) -> Self {
        Self { generation }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn test_after_post_init(generation: u64) -> Self {
        Self { generation }
    }
}

/// Opaque proof that the trusted AP IPA provider completed its source-matched
/// post-init path for one modem-owner generation.
pub struct ApReadyPermit {
    generation: u64,
}

impl ApReadyPermit {
    /// Minted by the Linux receive owner only after its trusted platform
    /// backend has revalidated the retained setup for this generation.
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    #[allow(dead_code)] // Standalone QMI tests do not include the receive owner.
    pub(crate) fn after_linux_setup(generation: u64) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        Ok(Self { generation })
    }
    /// Mint only after the trusted runtime has observed successful
    /// `ipa3_post_init()` completion, including the SRAM, GSI, interrupt, and
    /// AP-pipe checks. This constructor stays crate-private so transport users
    /// cannot assert hardware readiness themselves.
    #[allow(dead_code)] // The native IPA provider that consumes this is not implemented yet.
    pub(crate) fn after_post_init(proof: &PostInitProof) -> Result<Self, Error> {
        let generation = proof.generation();
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        Ok(Self { generation })
    }

    pub(crate) fn check(&self, generation: u64) -> Result<(), Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if self.generation != generation {
            return Err(Error::StaleGeneration);
        }
        Ok(())
    }

    /// Boot generation bound to the trusted AP IPA post-init proof.
    #[allow(dead_code)] // Shared test crates include this module without the native owner.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    #[allow(dead_code)] // Shared service module is included by more than one test crate.
    pub(crate) fn test_after_post_init(generation: u64) -> Self {
        let proof = PostInitProof::test_after_post_init(generation);
        Self::after_post_init(&proof).expect("nonzero test generation")
    }
}

impl PreparedIndication {
    pub fn peer(&self) -> Address {
        self.peer
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn token(&self) -> u64 {
        self.token
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingResponse {
    token: u64,
    request: Request,
}

pub struct Server {
    generation: u64,
    expected_peer: Address,
    next_token: u64,
    pending_response: Option<PendingResponse>,
    pending_indication: Option<u64>,
    registration: Option<Registration>,
    modem_init_status: Option<u8>,
    ap_init_complete: bool,
    indication_sent: bool,
    indeterminate: bool,
}

fn valid_address(address: Address) -> bool {
    address.node < CONTROL_PORT && address.port != 0 && address.port < CONTROL_PORT
}

fn boolean(message: &Message<'_>, tag: u8) -> Result<Option<bool>, Error> {
    match message.get(tag) {
        None => Ok(None),
        Some([0]) => Ok(Some(false)),
        Some([1]) => Ok(Some(true)),
        Some([value]) => Err(Error::InvalidBoolean(*value)),
        Some(_) => Err(qmi::Error::InvalidTlvLength(tag).into()),
    }
}

fn decode_request(message: &Message<'_>) -> Result<Request, Error> {
    if message.kind != Kind::Request {
        return Err(qmi::Error::UnsupportedKind.into());
    }
    Ok(match message.message_id {
        INDICATION_REGISTER_MESSAGE_ID => {
            if message.as_bytes().len() > MAX_INDICATION_REGISTER_QMI_BYTES {
                return Err(Error::InputTooLarge);
            }
            message.check_fields(&[])?;
            Request::IndicationRegistration(Registration {
                master_driver_init_complete: boolean(&message, TLV_MASTER_INIT_COMPLETE)?,
                data_usage_quota_reached: boolean(&message, TLV_DATA_USAGE_QUOTA)?,
                mhi_ready: boolean(&message, TLV_MHI_READY)?,
                endpoint_desc: boolean(&message, TLV_ENDPOINT_DESC)?,
                bandwidth_change: boolean(&message, TLV_BW_CHANGE)?,
            })
        }
        INIT_MODEM_DRIVER_COMPLETE_MESSAGE_ID => {
            if message.as_bytes().len() > MAX_MODEM_COMPLETE_QMI_BYTES {
                return Err(Error::InputTooLarge);
            }
            message.check_fields(&[TLV_MODEM_STATUS])?;
            Request::ModemInitComplete {
                status: message.u8(TLV_MODEM_STATUS)?,
            }
        }
        _ => return Err(Error::UnsupportedMessage),
    })
}

/// Validate the supported AP-side IPA request without changing server state.
/// The router uses this before lending a request to the serialized owner.
pub fn validate_request(message: &Message<'_>) -> Result<(), qmi::Error> {
    decode_request(message)
        .map(|_| ())
        .map_err(|error| match error {
            Error::UnsupportedMessage => qmi::Error::UnsupportedMessage,
            Error::Qmi(error) => error,
            _ => qmi::Error::InvalidValue,
        })
}

fn common_response(message_id: u16, transaction: u16) -> [u8; QMI_MESSAGE_BYTES] {
    let mut bytes = [0; QMI_MESSAGE_BYTES];
    bytes[0] = 2; // QMI response
    bytes[1..3].copy_from_slice(&transaction.to_le_bytes());
    bytes[3..5].copy_from_slice(&message_id.to_le_bytes());
    bytes[5..7].copy_from_slice(&7u16.to_le_bytes());
    bytes[7] = QMI_COMMON_RESPONSE_TLV;
    bytes[8..10].copy_from_slice(&4u16.to_le_bytes());
    // The pinned Linux handlers return QMI success for both supported requests.
    bytes[10..14].copy_from_slice(&[0, 0, 0, 0]);
    bytes
}

fn master_init_indication() -> [u8; QMI_MESSAGE_BYTES] {
    let mut bytes = common_response(MASTER_DRIVER_INIT_COMPLETE_MESSAGE_ID, 0);
    bytes[0] = 4; // QMI indication
    bytes
}

impl Server {
    pub fn new(expected_peer: Address, generation: u64) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if !valid_address(expected_peer) {
            return Err(Error::InvalidPeer);
        }
        Ok(Self {
            generation,
            expected_peer,
            next_token: 1,
            pending_response: None,
            pending_indication: None,
            registration: None,
            modem_init_status: None,
            ap_init_complete: false,
            indication_sent: false,
            indeterminate: false,
        })
    }

    pub fn registration(&self) -> Option<Registration> {
        self.registration
    }

    /// Raw modem status is retained because the pinned v3 Linux handler records
    /// completion and responds success without interpreting this byte.
    pub fn modem_init_status(&self) -> Option<u8> {
        self.modem_init_status
    }

    pub fn ap_init_complete(&self) -> bool {
        self.ap_init_complete
    }

    pub fn indication_sent(&self) -> bool {
        self.indication_sent
    }

    fn check(&self, generation: u64) -> Result<(), Error> {
        if self.indeterminate {
            return Err(Error::Indeterminate);
        }
        if generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        Ok(())
    }

    /// Prepare one bounded QMI response. State changes only after the caller
    /// confirms that the complete response record was published.
    pub fn prepare_response(
        &mut self,
        generation: u64,
        peer: Address,
        request_bytes: &[u8],
        output_capacity: usize,
    ) -> Result<PreparedResponse, Error> {
        self.check(generation)?;
        if peer != self.expected_peer {
            return Err(Error::WrongPeer);
        }
        if self.pending_response.is_some() {
            return Err(Error::Busy);
        }
        if output_capacity < QMI_MESSAGE_BYTES {
            return Err(Error::OutputTooSmall);
        }
        let message = Message::decode(request_bytes)?;
        let transaction = message.transaction;
        let request = decode_request(&message)?;
        let message_id = match request {
            Request::IndicationRegistration(_) => INDICATION_REGISTER_MESSAGE_ID,
            Request::ModemInitComplete { .. } => INIT_MODEM_DRIVER_COMPLETE_MESSAGE_ID,
        };
        let token = self.next_token;
        let next = token.checked_add(1).ok_or(Error::TokenExhausted)?;
        let prepared = PreparedResponse {
            peer,
            bytes: common_response(message_id, transaction),
            len: QMI_MESSAGE_BYTES,
            token,
            request,
        };
        self.next_token = next;
        self.pending_response = Some(PendingResponse { token, request });
        Ok(prepared)
    }

    /// Commit only after the full QMI response has been confirmed published.
    pub fn commit_response(
        &mut self,
        generation: u64,
        response: &PreparedResponse,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if response.peer != self.expected_peer {
            return Err(Error::WrongPeer);
        }
        let pending = self.pending_response.ok_or(Error::NoPendingResponse)?;
        if pending.token != response.token || pending.request != response.request {
            return Err(Error::NoPendingResponse);
        }
        match pending.request {
            Request::IndicationRegistration(registration) => {
                self.registration = Some(registration);
                self.indication_sent = false;
            }
            Request::ModemInitComplete { status } => {
                self.modem_init_status.get_or_insert(status);
            }
        }
        self.pending_response = None;
        Ok(())
    }

    /// An uncertain transport write may have reached the modem. Quarantine
    /// this generation rather than retransmitting a response or indication.
    pub fn mark_response_uncertain(
        &mut self,
        generation: u64,
        response: &PreparedResponse,
    ) -> Result<(), Error> {
        self.check(generation)?;
        let pending = self.pending_response.ok_or(Error::NoPendingResponse)?;
        if pending.token != response.token {
            return Err(Error::NoPendingResponse);
        }
        self.indeterminate = true;
        Ok(())
    }

    /// Called only by a trusted IPA owner after local initialization completes.
    /// This state is independent of the modem's QMI completion request.
    pub fn note_ap_init_complete(
        &mut self,
        generation: u64,
        readiness: &ApReadyPermit,
    ) -> Result<(), Error> {
        self.check(generation)?;
        readiness.check(generation)?;
        self.ap_init_complete = true;
        Ok(())
    }

    /// Prepare the source-matched `0x0022` indication after AP init and modem
    /// indication registration have both completed. Publication is committed
    /// separately to avoid reporting it sent when transport backpressure occurs.
    pub fn prepare_master_init_indication(
        &mut self,
        generation: u64,
        output_capacity: usize,
    ) -> Result<Option<PreparedIndication>, Error> {
        self.check(generation)?;
        if self.registration.is_none() || !self.ap_init_complete || self.indication_sent {
            return Ok(None);
        }
        if self.pending_indication.is_some() {
            return Err(Error::Busy);
        }
        if output_capacity < QMI_MESSAGE_BYTES {
            return Err(Error::OutputTooSmall);
        }
        let token = self.next_token;
        let next = token.checked_add(1).ok_or(Error::TokenExhausted)?;
        self.next_token = next;
        self.pending_indication = Some(token);
        Ok(Some(PreparedIndication {
            peer: self.expected_peer,
            bytes: master_init_indication(),
            len: QMI_MESSAGE_BYTES,
            token,
            generation,
        }))
    }

    pub fn commit_master_init_indication(
        &mut self,
        generation: u64,
        indication: &PreparedIndication,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if indication.generation != generation
            || indication.peer != self.expected_peer
            || self.pending_indication != Some(indication.token)
        {
            return Err(Error::NoPendingIndication);
        }
        self.pending_indication = None;
        self.indication_sent = true;
        Ok(())
    }

    pub fn mark_indication_uncertain(
        &mut self,
        generation: u64,
        indication: &PreparedIndication,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if indication.generation != generation
            || indication.peer != self.expected_peer
            || self.pending_indication != Some(indication.token)
        {
            return Err(Error::NoPendingIndication);
        }
        self.indeterminate = true;
        Ok(())
    }
}
