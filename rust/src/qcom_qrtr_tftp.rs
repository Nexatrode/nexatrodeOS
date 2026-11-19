//! Bounded TFTP transfer routing for the direct-peer QRTR transport.
//!
//! The caller supplies already-decoded QRTR routing fields and one opaque,
//! authorized image capability. This module binds each RRQ to its exact remote
//! `(node, port)` and a fresh local transfer port, then accepts only ACK or
//! ERROR packets addressed to that transfer port. It does not parse GLINK or
//! QRTR headers, publish the well-known TFTP service, access files, or manage
//! transport credits and retransmission timers.
#![forbid(unsafe_code)]

use crate::qcom_tftp::{
    self as tftp, AckOutcome, Error as TftpError, MAX_READ_SIZE, OP_ACK, OP_ERROR, OP_RRQ,
    ReadSession, ReadSource, RemoteErrorOutcome, TransferProgress,
};

pub const MAX_ACTIVE_TRANSFERS: usize = 4;
pub const MAX_RESERVED_PORTS: usize = 8;
pub const QRTR_CONTROL_PORT: u32 = 0xffff_fffe;
pub const QRTR_BROADCAST_NODE: u32 = 0xffff_ffff;

/// Opaque authority to serve one verified Sunfish WLAN image.
///
/// The opt-in private Sunfish input provider verifies the exact reference
/// image and device binding before constructing this capability. That path
/// is for the owner's authorized on-device experiment and grants no right to
/// redistribute firmware. Other providers still need their own verified
/// source/target/use policy. Test fixtures grant no device authority.
pub struct AuthorizedWlanImage<'a, S: ReadSource + ?Sized = [u8]> {
    source: &'a S,
}

impl<'a> AuthorizedWlanImage<'a, [u8]> {
    /// Only the private input provider calls this, after complete digest and
    /// device-binding checks. Retaining an immutable slice prevents content
    /// changes during transfer; no mutable storage lease is involved.
    #[cfg(feature = "sunfish-native-wifi-private-inputs")]
    pub(crate) const fn private_sunfish_reference(bytes: &'a [u8]) -> Self {
        Self { source: bytes }
    }

    #[cfg(test)]
    // This fixture is used by several integration-test crates; some test
    // harnesses compile this module without constructing a TFTP server.
    #[allow(dead_code)]
    pub(crate) const fn test_fixture(bytes: &'a [u8]) -> Self {
        Self { source: bytes }
    }
}

#[cfg(test)]
impl<'a, S: ReadSource + ?Sized> AuthorizedWlanImage<'a, S> {
    // This fixture is used by the TFTP integration suite; some suites compile
    // the generic implementation without constructing a streaming source.
    #[allow(dead_code)]
    pub(crate) const fn test_source_fixture(source: &'a S) -> Self {
        Self { source }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Peer {
    pub node: u32,
    pub port: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncomingRoute {
    pub destination_node: u32,
    pub destination_port: u32,
    pub source: Peer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyRoute {
    pub source_node: u32,
    pub source_port: u32,
    pub destination: Peer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferKey {
    pub local_port: u32,
    pub peer: Peer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidNode,
    InvalidServicePort,
    InvalidFirstTransferPort,
    InvalidTransferRange,
    TooManyReservedPorts,
    InvalidReservedPort,
    DuplicateReservedPort,
    ReservedPortConflict,
    ImageUnavailable,
    ImageTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IgnoreReason {
    WrongDestinationNode,
    WrongPeerNode,
    InvalidPeerPort,
    UnknownLocalPort,
    WrongPeerEndpoint,
    UnsupportedServicePacket,
    InvalidReadRequest,
    SourceReadFault,
    NoSessionCapacity,
    TransferPortExhausted,
    InvalidTransferPacket,
    DuplicateOrUnexpectedAck,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseReason {
    TransferComplete,
    EndOfTransfer,
    RemoteFailure { code: u16 },
    SourceReadFault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dispatch {
    Send {
        route: ReplyRoute,
        bytes: usize,
    },
    Closed {
        key: TransferKey,
        reason: CloseReason,
    },
    Ignored(IgnoreReason),
}

struct Transfer<'image, S: ReadSource + ?Sized> {
    key: TransferKey,
    session: ReadSession<'image, S>,
}

/// Fixed-capacity QRTR TFTP read-session table.
///
/// Transfer ports are allocated monotonically from `first_transfer_port` and
/// are never reused during this server's lifetime. The caller must provide all
/// other local QRTR ports that the allocator must skip. This avoids routing a
/// delayed datagram from an old transfer into a later session with the same
/// peer endpoint.
pub struct Server<'image, S: ReadSource + ?Sized = [u8]> {
    local_node: u32,
    trusted_peer_node: u32,
    service_port: u32,
    next_transfer_port: u32,
    last_transfer_port: u32,
    reserved_ports: [u32; MAX_RESERVED_PORTS],
    image: &'image S,
    image_len: u64,
    transfers: [Option<Transfer<'image, S>>; MAX_ACTIVE_TRANSFERS],
}

fn valid_port(port: u32) -> bool {
    port != 0 && port < QRTR_CONTROL_PORT
}

impl<'image, S: ReadSource + ?Sized> Server<'image, S> {
    /// Create a server over one authorized image and a bounded port policy.
    ///
    /// `reserved_ports` should include every other local QRTR endpoint in the
    /// owning transport. Consuming the opaque image capability prevents
    /// callers from supplying arbitrary bytes before source, integrity,
    /// target, and authorization checks are implemented by a trusted provider.
    pub fn new(
        local_node: u32,
        trusted_peer_node: u32,
        service_port: u32,
        first_transfer_port: u32,
        reserved_ports: &[u32],
        image: AuthorizedWlanImage<'image, S>,
    ) -> Result<Self, ConfigError> {
        if local_node == QRTR_BROADCAST_NODE || trusted_peer_node == QRTR_BROADCAST_NODE {
            return Err(ConfigError::InvalidNode);
        }
        if !valid_port(service_port) {
            return Err(ConfigError::InvalidServicePort);
        }
        if !valid_port(first_transfer_port) {
            return Err(ConfigError::InvalidFirstTransferPort);
        }
        if reserved_ports.len() > MAX_RESERVED_PORTS {
            return Err(ConfigError::TooManyReservedPorts);
        }
        let image_len = image
            .source
            .len()
            .map_err(|_| ConfigError::ImageUnavailable)?;
        if image_len > u64::from(MAX_READ_SIZE) {
            return Err(ConfigError::ImageTooLarge);
        }

        let mut ports = [0; MAX_RESERVED_PORTS];
        for (index, &port) in reserved_ports.iter().enumerate() {
            if !valid_port(port) {
                return Err(ConfigError::InvalidReservedPort);
            }
            if port == service_port {
                return Err(ConfigError::ReservedPortConflict);
            }
            if reserved_ports[..index].contains(&port) {
                return Err(ConfigError::DuplicateReservedPort);
            }
            ports[index] = port;
        }

        Ok(Self {
            local_node,
            trusted_peer_node,
            service_port,
            next_transfer_port: first_transfer_port,
            last_transfer_port: QRTR_CONTROL_PORT - 1,
            reserved_ports: ports,
            image: image.source,
            image_len,
            transfers: [None, None, None, None],
        })
    }

    /// Restrict a firmware server to a dedicated, finite local-port range.
    pub fn new_bounded(local_node: u32, trusted_peer_node: u32, service_port: u32,
        first_transfer_port: u32, last_transfer_port: u32, reserved_ports: &[u32],
        image: AuthorizedWlanImage<'image, S>) -> Result<Self, ConfigError> {
        if !valid_port(last_transfer_port) || first_transfer_port > last_transfer_port {
            return Err(ConfigError::InvalidTransferRange);
        }
        let mut server = Self::new(local_node, trusted_peer_node, service_port,
            first_transfer_port, reserved_ports, image)?;
        server.last_transfer_port = last_transfer_port;
        Ok(server)
    }

    /// Revalidate that the retained source is still available with the length
    /// that was authorized when this server was constructed.
    pub fn image_available(&self) -> bool {
        self.image.len() == Ok(self.image_len)
    }
    /// Local node, authenticated peer node and registered service port.
    /// Metadata only; callers must still retain and revalidate the image.
    pub fn route_binding(&self) -> (u32, u32, u32) {
        (self.local_node, self.trusted_peer_node, self.service_port)
    }

    pub fn active_transfers(&self) -> usize {
        self.transfers.iter().flatten().count()
    }

    /// Immutable snapshot for the serialized transport owner; refresh before
    /// polling peer-close events and after each server state transition.
    pub fn active_peers(&self) -> [Option<Peer>; MAX_ACTIVE_TRANSFERS] {
        core::array::from_fn(|index| self.transfers[index].as_ref().map(|t| t.key.peer))
    }

    pub fn owns_transfer(&self, peer: Peer, local_port: u32) -> bool {
        self.transfers.iter().flatten().any(|t| t.key.peer == peer && t.key.local_port == local_port)
    }

    /// Retire sessions only after the transport has safely handled outstanding
    /// replies for this exact peer. Transfer ports are never reused.
    pub fn close_peer(&mut self, peer: Peer) -> usize {
        let mut closed = 0;
        for transfer in &mut self.transfers {
            if transfer.as_ref().is_some_and(|transfer| transfer.key.peer == peer) {
                *transfer = None;
                closed += 1;
            }
        }
        closed
    }

    pub fn next_transfer_port(&self) -> u32 {
        self.next_transfer_port
    }

    /// Route one TFTP payload using its already-decoded QRTR source and
    /// destination addresses. Responses are written to caller-owned storage;
    /// this method never allocates or retains a packet buffer.
    pub fn receive(&mut self, route: IncomingRoute, packet: &[u8], output: &mut [u8]) -> Dispatch {
        if output.len() < tftp::MAX_REQUEST_BYTES {
            return Dispatch::Ignored(IgnoreReason::OutputTooSmall);
        }
        if route.destination_node != self.local_node {
            return Dispatch::Ignored(IgnoreReason::WrongDestinationNode);
        }
        if route.source.node != self.trusted_peer_node {
            return Dispatch::Ignored(IgnoreReason::WrongPeerNode);
        }
        if !valid_port(route.source.port) {
            return Dispatch::Ignored(IgnoreReason::InvalidPeerPort);
        }

        if route.destination_port == self.service_port {
            return self.receive_service_request(route.source, packet, output);
        }
        self.receive_transfer_packet(route.destination_port, route.source, packet, output)
    }

    fn receive_service_request(
        &mut self,
        peer: Peer,
        packet: &[u8],
        output: &mut [u8],
    ) -> Dispatch {
        if packet.len() < 2 || u16::from_be_bytes([packet[0], packet[1]]) != OP_RRQ {
            return Dispatch::Ignored(IgnoreReason::UnsupportedServicePacket);
        }
        let mut request = match tftp::decode_read_request(packet) {
            Ok(request) => request,
            Err(_) => return Dispatch::Ignored(IgnoreReason::InvalidReadRequest),
        };
        // This server emits one DATA per received ACK; it has no independent
        // window pump. Negotiate one packet so the peer will acknowledge each
        // packet rather than waiting for an unproduced second packet.
        request.options.window_size = 1;
        if let Some(transfer) = self.transfers.iter().flatten().find(|transfer| transfer.key.peer == peer) {
            return match transfer.session.repeat_initial_packet(request, output) {
                Ok(Some(bytes)) => Dispatch::Send {
                    route: ReplyRoute { source_node: self.local_node, source_port: transfer.key.local_port, destination: peer },
                    bytes,
                },
                Ok(None) => Dispatch::Ignored(IgnoreReason::InvalidReadRequest),
                Err(TftpError::SourceRead) => Dispatch::Ignored(IgnoreReason::SourceReadFault),
                Err(_) => Dispatch::Ignored(IgnoreReason::OutputTooSmall),
            };
        }
        let Some(slot) = self.transfers.iter().position(Option::is_none) else {
            return Dispatch::Ignored(IgnoreReason::NoSessionCapacity);
        };
        let Some(local_port) = self.allocate_transfer_port() else {
            return Dispatch::Ignored(IgnoreReason::TransferPortExhausted);
        };
        let mut session = match ReadSession::with_source(request, self.image) {
            Ok(session) => session,
            Err(TftpError::SourceRead) => {
                return Dispatch::Ignored(IgnoreReason::SourceReadFault);
            }
            Err(_) => return Dispatch::Ignored(IgnoreReason::InvalidReadRequest),
        };
        let progress = match session.next_packet(output) {
            Ok(progress) => progress,
            Err(TftpError::BufferTooSmall) => {
                return Dispatch::Ignored(IgnoreReason::OutputTooSmall);
            }
            Err(TftpError::SourceRead) => {
                return Dispatch::Ignored(IgnoreReason::SourceReadFault);
            }
            Err(_) => return Dispatch::Ignored(IgnoreReason::InvalidReadRequest),
        };
        let TransferProgress::Packet { bytes, .. } = progress else {
            return Dispatch::Ignored(IgnoreReason::InvalidReadRequest);
        };
        let key = TransferKey { local_port, peer };
        self.transfers[slot] = Some(Transfer { key, session });
        Dispatch::Send {
            route: ReplyRoute {
                source_node: self.local_node,
                source_port: local_port,
                destination: peer,
            },
            bytes,
        }
    }

    fn receive_transfer_packet(
        &mut self,
        local_port: u32,
        peer: Peer,
        packet: &[u8],
        output: &mut [u8],
    ) -> Dispatch {
        let Some(index) = self.transfers.iter().position(|transfer| {
            transfer
                .as_ref()
                .is_some_and(|t| t.key.local_port == local_port)
        }) else {
            return Dispatch::Ignored(IgnoreReason::UnknownLocalPort);
        };
        let transfer = self.transfers[index].as_mut().unwrap();
        if transfer.key.peer != peer {
            return Dispatch::Ignored(IgnoreReason::WrongPeerEndpoint);
        }

        let opcode = packet
            .get(..2)
            .map(|value| u16::from_be_bytes([value[0], value[1]]));
        match opcode {
            Some(OP_ACK) => match transfer.session.receive_ack(packet) {
                Ok(AckOutcome::Accepted) => match transfer.session.next_packet(output) {
                    Ok(TransferProgress::Packet { bytes, .. }) => Dispatch::Send {
                        route: ReplyRoute {
                            source_node: self.local_node,
                            source_port: local_port,
                            destination: peer,
                        },
                        bytes,
                    },
                    Ok(TransferProgress::Complete) => {
                        let key = transfer.key;
                        self.transfers[index] = None;
                        Dispatch::Closed {
                            key,
                            reason: CloseReason::TransferComplete,
                        }
                    }
                    Ok(TransferProgress::WaitingForAck(_)) => {
                        Dispatch::Ignored(IgnoreReason::InvalidTransferPacket)
                    }
                    Err(TftpError::BufferTooSmall) => {
                        Dispatch::Ignored(IgnoreReason::OutputTooSmall)
                    }
                    Err(TftpError::SourceRead) => {
                        let key = transfer.key;
                        self.transfers[index] = None;
                        Dispatch::Closed {
                            key,
                            reason: CloseReason::SourceReadFault,
                        }
                    }
                    Err(_) => Dispatch::Ignored(IgnoreReason::InvalidTransferPacket),
                },
                Ok(AckOutcome::Duplicate | AckOutcome::Unexpected) => {
                    Dispatch::Ignored(IgnoreReason::DuplicateOrUnexpectedAck)
                }
                Err(_) => Dispatch::Ignored(IgnoreReason::InvalidTransferPacket),
            },
            Some(OP_ERROR) => match transfer.session.receive_error(packet) {
                Ok(outcome) => {
                    let key = transfer.key;
                    self.transfers[index] = None;
                    let reason = match outcome {
                        RemoteErrorOutcome::EndOfTransfer => CloseReason::EndOfTransfer,
                        RemoteErrorOutcome::Failure { code } => CloseReason::RemoteFailure { code },
                    };
                    Dispatch::Closed { key, reason }
                }
                Err(_) => Dispatch::Ignored(IgnoreReason::InvalidTransferPacket),
            },
            _ => Dispatch::Ignored(IgnoreReason::InvalidTransferPacket),
        }
    }

    fn allocate_transfer_port(&mut self) -> Option<u32> {
        while valid_port(self.next_transfer_port) && self.next_transfer_port <= self.last_transfer_port {
            let candidate = self.next_transfer_port;
            self.next_transfer_port = candidate.saturating_add(1);
            if candidate != self.service_port
                && !self.reserved_ports.contains(&candidate)
                && !self
                    .transfers
                    .iter()
                    .flatten()
                    .any(|transfer| transfer.key.local_port == candidate)
            {
                return Some(candidate);
            }
        }
        None
    }
}
