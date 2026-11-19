//! Bounded inbound QRTR-v1 routing to the fixed Sunfish modem-service decoders.
//!
//! The caller supplies complete, unpadded datagrams and the trusted transport
//! generation. The generation is never taken from peer-controlled wire data.
//! This module neither publishes services nor sends replies. A reply route
//! identifies a request; it is not evidence an operation succeeded. A future
//! transport must recheck its generation before any reply is published.
#![forbid(unsafe_code)]

use crate::qcom_modem_services::{self as services, Request, Service};
use crate::qcom_qmi::{self as qmi, Kind, Message};
use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_DATAGRAM_BYTES: usize = 4096;
pub const QRTR_HEADER_BYTES: usize = 32;
pub const MAX_ENDPOINTS: usize = 4;
pub const MAX_IN_FLIGHT: usize = 8;
const CONTROL_PORT: u32 = 0xffff_fffe;
static NEXT_ROUTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(qmi::Error),
    InvalidGeneration,
    StaleGeneration,
    InvalidNode,
    InvalidPort,
    UnsupportedVersion,
    UnsupportedPacket,
    InvalidConfirmRx,
    WrongDestination,
    WrongPeer,
    NoEndpoint,
    EndpointBusy,
    NotRequest,
    DuplicateTransaction,
    InFlightFull,
    TicketExhausted,
    RouterIdsExhausted,
    InvalidTicket,
}

impl From<qmi::Error> for Error {
    fn from(error: qmi::Error) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address {
    pub node: u32,
    pub port: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub service: Service,
    pub address: Address,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Context {
    pub generation: u64,
    pub service: Service,
    pub peer: Address,
    pub local: Address,
    pub transaction: u16,
    pub message_id: u16,
    pub confirm_rx: bool,
}

/// Private fields and no Copy/Clone prevent construction or normal reuse by
/// callers. Dropped tickets retain their slot until generation reset.
#[derive(Debug)]
pub struct Ticket {
    owner: u64,
    generation: u64,
    serial: u64,
    slot: usize,
}

#[derive(Debug)]
pub struct RoutedRequest<'a> {
    pub context: Context,
    pub request: Request<'a>,
    pub ticket: Ticket,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyRoute {
    pub generation: u64,
    pub service: Service,
    pub source: Address,
    pub destination: Address,
    pub transaction: u16,
    pub message_id: u16,
}

#[derive(Clone, Copy)]
struct Pending {
    context: Context,
    serial: u64,
}

pub struct Router {
    owner: u64,
    local_node: u32,
    peer_node: u32,
    generation: u64,
    endpoints: [Option<Endpoint>; MAX_ENDPOINTS],
    pending: [Option<Pending>; MAX_IN_FLIGHT],
    next_serial: u64,
}

fn valid_port(port: u32) -> bool {
    port != 0 && port < CONTROL_PORT
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl Router {
    pub fn new(local_node: u32, peer_node: u32, generation: u64) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if local_node >= CONTROL_PORT || peer_node >= CONTROL_PORT || local_node == peer_node {
            return Err(Error::InvalidNode);
        }
        let owner = NEXT_ROUTER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Error::RouterIdsExhausted)?;
        Ok(Self {
            owner,
            local_node,
            peer_node,
            generation,
            endpoints: [None; MAX_ENDPOINTS],
            pending: [None; MAX_IN_FLIGHT],
            next_serial: 1,
        })
    }

    /// Local configuration only: this does not advertise a QRTR service.
    pub fn register(&mut self, service: Service, port: u32) -> Result<Endpoint, Error> {
        if !valid_port(port) {
            return Err(Error::InvalidPort);
        }
        if self.endpoints[service.index()].is_some()
            || self
                .endpoints
                .iter()
                .flatten()
                .any(|endpoint| endpoint.address.port == port)
        {
            return Err(Error::EndpointBusy);
        }
        let endpoint = Endpoint {
            service,
            address: Address {
                node: self.local_node,
                port,
            },
        };
        self.endpoints[service.index()] = Some(endpoint);
        Ok(endpoint)
    }

    /// Reset requires a strictly newer locally managed generation. Old endpoint
    /// registrations and pending tickets are invalidated together; serials are
    /// never reset, so reusing a slot cannot revive an old ticket.
    pub fn begin_generation(&mut self, generation: u64) -> Result<(), Error> {
        if generation <= self.generation {
            return Err(Error::InvalidGeneration);
        }
        self.generation = generation;
        self.endpoints = [None; MAX_ENDPOINTS];
        self.pending = [None; MAX_IN_FLIGHT];
        Ok(())
    }

    pub fn in_flight(&self) -> usize {
        self.pending.iter().flatten().count()
    }

    pub fn endpoint(&self, service: Service) -> Option<Endpoint> {
        self.endpoints[service.index()]
    }

    pub fn route<'a>(
        &mut self,
        generation: u64,
        bytes: &'a [u8],
    ) -> Result<RoutedRequest<'a>, Error> {
        self.route_profile(generation, bytes, false)
    }

    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub(crate) fn route_linux_ipa<'a>(
        &mut self,
        generation: u64,
        bytes: &'a [u8],
    ) -> Result<RoutedRequest<'a>, Error> {
        self.route_profile(generation, bytes, true)
    }

    fn route_profile<'a>(
        &mut self,
        generation: u64,
        bytes: &'a [u8],
        _linux_ipa: bool,
    ) -> Result<RoutedRequest<'a>, Error> {
        if generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        if bytes.len() > MAX_DATAGRAM_BYTES {
            return Err(qmi::Error::TooLarge.into());
        }
        if bytes.len() < QRTR_HEADER_BYTES {
            return Err(qmi::Error::ShortHeader.into());
        }
        if word(bytes, 0) != 1 {
            return Err(Error::UnsupportedVersion);
        }
        if word(bytes, 4) != 1 {
            return Err(Error::UnsupportedPacket);
        }
        let peer = Address {
            node: word(bytes, 8),
            port: word(bytes, 12),
        };
        let local = Address {
            node: word(bytes, 24),
            port: word(bytes, 28),
        };
        if peer.node != self.peer_node {
            return Err(Error::WrongPeer);
        }
        if local.node != self.local_node {
            return Err(Error::WrongDestination);
        }
        if !valid_port(peer.port) || !valid_port(local.port) {
            return Err(Error::InvalidPort);
        }
        let confirm_rx = match word(bytes, 16) {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidConfirmRx),
        };
        if word(bytes, 20) as u64 != (bytes.len() - QRTR_HEADER_BYTES) as u64 {
            return Err(qmi::Error::LengthMismatch.into());
        }
        let endpoint = self
            .endpoints
            .iter()
            .flatten()
            .find(|endpoint| endpoint.address == local)
            .ok_or(Error::NoEndpoint)?;
        let service = endpoint.service;
        let message = Message::decode(&bytes[QRTR_HEADER_BYTES..])?;
        if message.kind != Kind::Request {
            return Err(Error::NotRequest);
        }
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        let request = if _linux_ipa && service == Service::IpaServer {
            crate::sunfish_ipa_linux_qmi::validate_server_request(&message)?;
            Request::IpaServer {
                message_id: message.message_id,
            }
        } else {
            services::decode(service, &message)?
        };
        #[cfg(not(feature = "sunfish-linux-ipa-qmi"))]
        let request = services::decode(service, &message)?;
        let context = Context {
            generation,
            service,
            peer,
            local,
            transaction: message.transaction,
            message_id: message.message_id,
            confirm_rx,
        };
        // Validation above is side-effect free. No malformed request can
        // consume capacity, even when the table is already full.
        if self.pending.iter().flatten().any(|pending| {
            pending.context.peer == peer
                && pending.context.local == local
                && pending.context.transaction == context.transaction
        }) {
            return Err(Error::DuplicateTransaction);
        }
        let slot = self
            .pending
            .iter()
            .position(Option::is_none)
            .ok_or(Error::InFlightFull)?;
        let serial = self.next_serial;
        let next_serial = serial.checked_add(1).ok_or(Error::TicketExhausted)?;
        self.pending[slot] = Some(Pending { context, serial });
        self.next_serial = next_serial;
        Ok(RoutedRequest {
            context,
            request,
            ticket: Ticket {
                owner: self.owner,
                generation,
                serial,
                slot,
            },
        })
    }

    /// Inspect the sealed reply identity without releasing the transaction key.
    /// Queued transports retain the ticket until their completion conditions hold.
    pub fn reply_route(&self, ticket: &Ticket) -> Result<ReplyRoute, Error> {
        if ticket.owner != self.owner {
            return Err(Error::InvalidTicket);
        }
        if ticket.generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        let pending = self
            .pending
            .get(ticket.slot)
            .copied()
            .flatten()
            .filter(|pending| pending.serial == ticket.serial)
            .ok_or(Error::InvalidTicket)?;
        let context = pending.context;
        Ok(ReplyRoute {
            generation: context.generation,
            service: context.service,
            source: context.local,
            destination: context.peer,
            transaction: context.transaction,
            message_id: context.message_id,
        })
    }

    /// Retire one validated request without claiming its action succeeded.
    pub fn complete(&mut self, ticket: Ticket) -> Result<ReplyRoute, Error> {
        let route = self.reply_route(&ticket)?;
        self.pending[ticket.slot] = None;
        Ok(route)
    }

    pub fn cancel(&mut self, ticket: Ticket) -> Result<(), Error> {
        self.complete(ticket).map(|_| ())
    }
}
