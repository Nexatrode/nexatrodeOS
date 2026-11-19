//! Direct-peer QRTR-v1 control plane. Fixed tables, no forwarding or allocation.
//!
//! Wire layout: LineageOS kernel 28f9290ae067d3e7857126eb6312720dbe9c7c80
//! include/uapi/linux/qrtr.h and net/qrtr/qrtr.c. Announcements use the QMI
//! descriptor's (instance << 8) | version, as in linux-msm/qrtr lib/qrtr.c.
//! Readiness is a trusted backend assertion, never inferred from a peer packet.
#![forbid(unsafe_code)]

use crate::qcom_modem_services::Service;
use crate::qcom_qmi_router::Address;

pub const CONTROL_PORT: u32 = 0xffff_fffe;
pub const BROADCAST_NODE: u32 = 0xffff_ffff;
pub const DATAGRAM_BYTES: usize = 52;
pub const MAX_SERVERS: usize = 64;
pub const RECEIVE_TARGET: usize = 2;
pub const WLFW_SERVICE_ID: u32 = 0x45;
/// QRTR v1 stores `(instance << 8) | version`; WLFW is version 1, instance 0.
pub const WLFW_VERSION_INSTANCE: u32 = 0x0000_0001;
pub const IPA_SERVICE_ID: u32 = 0x31;
/// The modem-side IPA endpoint is service 0x31, version 1, instance 2.
pub const IPA_MODEM_VERSION_INSTANCE: u32 = (2 << 8) | 1;
pub const TFTP_SERVICE_ID: u32 = 4096;
/// The Sunfish Lineage `tftp_server` advertises version 1, instance 0.
pub const TFTP_VERSION_INSTANCE: u32 = 0x0000_0001;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Length,
    Header,
    Address,
    Payload,
    Unsupported,
    NotStarted,
    PeerNotReady,
    PeerGone,
    ClientGone,
    NoCapacity,
    ServerConflict,
    Busy,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Server {
    pub service: u32,
    pub instance: u32,
    pub address: Address,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Hello,
    Bye,
    NewServer(Server),
    DelServer(Server),
    DelClient(u32),
    Ignored(u32),
    DelProc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Hello {
        first: bool,
    },
    Server {
        server: Server,
        added: bool,
        changed: bool,
    },
    ClientRemoved(u32),
    Ignored(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outbound {
    Hello,
    NewServer(Service),
    DelServer(Service),
    NewTftpServer,
    DelTftpServer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub started: bool,
    pub peer_hello: bool,
    pub hello_sent: bool,
    pub servers: usize,
    pub ready_services: u8,
    pub announced_services: u8,
}

#[derive(Clone, Copy)]
struct Local {
    port: u32,
    ready: bool,
    announced: bool,
    queued: bool,
}
impl Local {
    const EMPTY: Self = Self {
        port: 0,
        ready: false,
        announced: false,
        queued: false,
    };
}

pub struct Controller {
    local: u32,
    peer: u32,
    started: bool,
    gone: bool,
    peer_hello: bool,
    hello_sent: bool,
    hello_queued: bool,
    services: [Local; 4],
    tftp: Local,
    servers: [Option<Server>; MAX_SERVERS],
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn port(value: u32) -> bool {
    value != 0 && value < CONTROL_PORT
}

impl Controller {
    pub fn new(local: u32, peer: u32) -> Self {
        // The enclosing router validates the two node identities.
        Self {
            local,
            peer,
            started: false,
            gone: false,
            peer_hello: false,
            hello_sent: false,
            hello_queued: false,
            services: [Local::EMPTY; 4],
            tftp: Local::EMPTY,
            servers: [None; MAX_SERVERS],
        }
    }
    fn check(&self) -> Result<(), Error> {
        if self.gone {
            Err(Error::PeerGone)
        } else if !self.started {
            Err(Error::NotStarted)
        } else {
            Ok(())
        }
    }
    pub fn start(&mut self) -> Result<(), Error> {
        if self.gone {
            return Err(Error::PeerGone);
        }
        if self.started {
            return Err(Error::Busy);
        }
        self.started = true;
        Ok(())
    }
    pub fn started(&self) -> bool {
        self.started
    }
    pub fn status(&self) -> Status {
        let mut status = Status {
            started: self.started,
            peer_hello: self.peer_hello,
            hello_sent: self.hello_sent,
            servers: self.servers.iter().flatten().count(),
            ready_services: 0,
            announced_services: 0,
        };
        for (index, service) in self.services.iter().enumerate() {
            let bit = if index < 3 { index } else { index + 1 };
            status.ready_services |= u8::from(service.ready) << bit;
            status.announced_services |= u8::from(service.announced) << bit;
        }
        status.ready_services |= u8::from(self.tftp.ready) << 3;
        status.announced_services |= u8::from(self.tftp.announced) << 3;
        status
    }
    pub fn server(&self, index: usize) -> Option<Server> {
        self.servers.get(index).copied().flatten()
    }
    /// Return the unique WLFW endpoint already announced by the authenticated
    /// direct peer. QRTR NEW_LOOKUP is a local name-service operation, so it is
    /// deliberately not forwarded as a packet to the modem.
    pub fn wlfw_endpoint(&self) -> Option<Address> {
        if !self.started || self.gone || !self.hello_sent || !self.peer_hello {
            return None;
        }
        let mut matches = self.servers.iter().flatten().filter(|server| {
            server.service == WLFW_SERVICE_ID && server.instance == WLFW_VERSION_INSTANCE
        });
        let first = matches.next()?.address;
        matches.next().is_none().then_some(first)
    }
    /// Return the unique modem IPA endpoint after direct-peer HELLO. This is
    /// distinct from the local AP service at IPA instance 1.
    pub fn ipa_modem_endpoint(&self) -> Option<Address> {
        if !self.started || self.gone || !self.hello_sent || !self.peer_hello {
            return None;
        }
        let mut matches = self.servers.iter().flatten().filter(|server| {
            server.service == IPA_SERVICE_ID && server.instance == IPA_MODEM_VERSION_INSTANCE
        });
        let first = matches.next()?.address;
        matches.next().is_none().then_some(first)
    }
    /// Configure a local server before direct-peer control starts. This stages
    /// readiness only; the service cannot be announced or accept requests until
    /// HELLO completes and the server record is published.
    pub fn prepare_available(&mut self, service: Service, at: u32) -> Result<(), Error> {
        if self.gone {
            return Err(Error::PeerGone);
        }
        if self.started {
            return Err(Error::Busy);
        }
        if !port(at) {
            return Err(Error::Address);
        }
        let local = &mut self.services[service.index()];
        if local.port != 0 || local.ready || local.announced || local.queued {
            return Err(Error::Busy);
        }
        local.port = at;
        local.ready = true;
        Ok(())
    }
    /// Backend must stay alive through withdrawal and all outstanding requests.
    /// A queued change must finish publication before another change is accepted.
    pub fn set_available(&mut self, service: Service, at: u32, ready: bool) -> Result<(), Error> {
        self.check()?;
        if !port(at) {
            return Err(Error::Address);
        }
        let local = &mut self.services[service.index()];
        if local.queued {
            return Err(Error::Busy);
        }
        if local.port != 0 && local.port != at {
            return Err(Error::Address);
        }
        local.port = at;
        local.ready = ready;
        Ok(())
    }
    pub fn accepts(&self, service: Service) -> bool {
        let local = self.services[service.index()];
        !self.gone && self.peer_hello && self.hello_sent && local.ready && local.announced
    }

    /// Stage the raw QRTR TFTP endpoint before control starts. TFTP packets are
    /// not QMI requests and must be dispatched by a separate bounded handler.
    pub fn prepare_tftp_available(&mut self, at: u32) -> Result<(), Error> {
        if self.gone {
            return Err(Error::PeerGone);
        }
        if self.started {
            return Err(Error::Busy);
        }
        if !port(at) {
            return Err(Error::Address);
        }
        if self.tftp.port != 0 || self.tftp.ready || self.tftp.announced || self.tftp.queued {
            return Err(Error::Busy);
        }
        self.tftp.port = at;
        self.tftp.ready = true;
        Ok(())
    }

    /// Check that a TFTP endpoint is staged at the exact port while the
    /// controller is still pristine and the endpoint has not been announced.
    pub fn tftp_server_prepared(&self, at: u32) -> bool {
        !self.gone
            && !self.started
            && self.tftp.port == at
            && self.tftp.ready
            && !self.tftp.announced
            && !self.tftp.queued
    }

    /// The TFTP observer/server must remain alive through withdrawal and any
    /// outstanding transfers. No packet is answered by changing readiness.
    pub fn set_tftp_available(&mut self, at: u32, ready: bool) -> Result<(), Error> {
        self.check()?;
        if !port(at) {
            return Err(Error::Address);
        }
        if self.tftp.queued {
            return Err(Error::Busy);
        }
        if self.tftp.port != 0 && self.tftp.port != at {
            return Err(Error::Address);
        }
        self.tftp.port = at;
        self.tftp.ready = ready;
        Ok(())
    }

    pub fn accepts_tftp(&self) -> bool {
        !self.gone && self.peer_hello && self.hello_sent && self.tftp.ready && self.tftp.announced
    }

    pub fn tftp_port(&self) -> Option<u32> {
        (self.tftp.port != 0).then_some(self.tftp.port)
    }

    /// Decode a complete, unpadded datagram before changing any state. Lookup
    /// commands are local name-service APIs, not accepted from the modem link.
    pub fn decode(&self, bytes: &[u8]) -> Result<Command, Error> {
        self.check()?;
        if bytes.len() != DATAGRAM_BYTES {
            return Err(Error::Length);
        }
        let kind = word(bytes, 4);
        if word(bytes, 0) != 1
            || word(bytes, 16) != 0
            || word(bytes, 20) != 20
            || word(bytes, 32) != kind
        {
            return Err(Error::Header);
        }
        let source_port = word(bytes, 12);
        if word(bytes, 8) != self.peer
            || ![self.local, BROADCAST_NODE].contains(&word(bytes, 24))
            || word(bytes, 28) != CONTROL_PORT
            || (source_port != CONTROL_PORT && kind != 6)
        {
            return Err(Error::Address);
        }
        match kind {
            // Linux QRTR treats these commands using only `cmd`; their
            // 16-byte union storage is not a reserved-zero field. In
            // particular, HELLO is answered from the packet source address.
            // Keep the fixed 20-byte control record and strict outer address
            // checks, while ignoring fields the protocol does not define for
            // these commands.
            2 | 3 | 8 | 9 => Ok(match kind {
                2 => Command::Hello,
                3 => Command::Bye,
                _ => Command::Ignored(kind),
            }),
            4 | 5 => {
                let server = Server {
                    service: word(bytes, 36),
                    instance: word(bytes, 40),
                    address: Address {
                        node: word(bytes, 44),
                        port: word(bytes, 48),
                    },
                };
                if server.service == 0
                    || server.address.node != self.peer
                    || !port(server.address.port)
                {
                    return Err(Error::Payload);
                }
                Ok(if kind == 4 {
                    Command::NewServer(server)
                } else {
                    Command::DelServer(server)
                })
            }
            6 => {
                let at = word(bytes, 40);
                if word(bytes, 36) != self.peer
                    || !port(at)
                    || (source_port != CONTROL_PORT && source_port != at)
                {
                    return Err(Error::Payload);
                }
                Ok(Command::DelClient(at))
            }
            // Matched kernel's DEL_PROC magic; unrelated/forwarded nodes are
            // rejected. A direct peer loss never mints a new generation.
            13 => {
                if word(bytes, 36) != 0xe111
                    || word(bytes, 40) != self.peer
                    || bytes[44..] != [0; 8]
                {
                    return Err(Error::Payload);
                }
                Ok(Command::DelProc)
            }
            _ => Err(Error::Unsupported),
        }
    }
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Event, Error> {
        let command = self.decode(bytes)?;
        if !matches!(command, Command::Hello | Command::Bye | Command::DelProc) && !self.peer_hello
        {
            return Err(Error::PeerNotReady);
        }
        match command {
            Command::Hello => {
                let first = !self.peer_hello;
                self.peer_hello = true;
                Ok(Event::Hello { first })
            }
            Command::Bye | Command::DelProc => {
                self.gone = true;
                Err(Error::PeerGone)
            }
            Command::NewServer(server) => {
                let index = self
                    .servers
                    .iter()
                    .position(|s| s.is_some_and(|s| s.address == server.address))
                    .or_else(|| self.servers.iter().position(Option::is_none))
                    .ok_or(Error::NoCapacity)?;
                let changed = self.servers[index] != Some(server);
                self.servers[index] = Some(server);
                Ok(Event::Server {
                    server,
                    added: true,
                    changed,
                })
            }
            Command::DelServer(server) => {
                let index = self
                    .servers
                    .iter()
                    .position(|s| s.is_some_and(|s| s.address == server.address));
                if index.is_some_and(|i| self.servers[i] != Some(server)) {
                    return Err(Error::ServerConflict);
                }
                if let Some(index) = index {
                    self.servers[index] = None;
                }
                Ok(Event::Server {
                    server,
                    added: false,
                    changed: index.is_some(),
                })
            }
            Command::DelClient(at) => {
                for server in &mut self.servers {
                    if server.is_some_and(|s| s.address.port == at) {
                        *server = None;
                    }
                }
                Ok(Event::ClientRemoved(at))
            }
            Command::Ignored(kind) => Ok(Event::Ignored(kind)),
        }
    }

    /// At most one pending HELLO and three unsent local service transitions.
    /// Published records still occupy the existing bounded TX completion slots.
    /// These flags coalesce input instead of constructing an unbounded queue.
    pub(crate) fn next(&self) -> Option<Outbound> {
        if self.check().is_err() {
            return None;
        }
        if !self.hello_sent {
            return if self.hello_queued {
                None
            } else {
                Some(Outbound::Hello)
            };
        }
        if !self.peer_hello {
            return None;
        }
        // LOC must be discoverable before the data services: the modem may
        // resolve protection-domain ownership during its own initialization.
        for service in [Service::ServiceRegistry, Service::IpaServer] {
            let local = self.services[service.index()];
            if !local.queued && local.ready != local.announced {
                return Some(if local.ready {
                    Outbound::NewServer(service)
                } else {
                    Outbound::DelServer(service)
                });
            }
        }
        if !self.tftp.queued && self.tftp.ready != self.tftp.announced {
            return Some(if self.tftp.ready {
                Outbound::NewTftpServer
            } else {
                Outbound::DelTftpServer
            });
        }
        for service in [Service::RemoteStorage, Service::HeapMemory] {
            let local = self.services[service.index()];
            if !local.queued && local.ready != local.announced {
                return Some(if local.ready {
                    Outbound::NewServer(service)
                } else {
                    Outbound::DelServer(service)
                });
            }
        }
        None
    }
    pub(crate) fn encode(&self, token: Outbound) -> [u8; DATAGRAM_BYTES] {
        let mut bytes = [0; DATAGRAM_BYTES];
        let kind = match token {
            Outbound::Hello => 2,
            Outbound::NewServer(_) | Outbound::NewTftpServer => 4,
            Outbound::DelServer(_) | Outbound::DelTftpServer => 5,
        };
        for (index, value) in [
            1,
            kind,
            self.local,
            CONTROL_PORT,
            0,
            20,
            self.peer,
            CONTROL_PORT,
            kind,
        ]
        .iter()
        .enumerate()
        {
            put(&mut bytes, index * 4, *value);
        }
        match token {
            Outbound::NewServer(service) | Outbound::DelServer(service) => {
                let (id, version, instance) = service.descriptor();
                put(&mut bytes, 36, id);
                put(&mut bytes, 40, (instance << 8) | version as u32);
                put(&mut bytes, 44, self.local);
                put(&mut bytes, 48, self.services[service.index()].port);
            }
            Outbound::NewTftpServer | Outbound::DelTftpServer => {
                put(&mut bytes, 36, TFTP_SERVICE_ID);
                put(&mut bytes, 40, TFTP_VERSION_INSTANCE);
                put(&mut bytes, 44, self.local);
                put(&mut bytes, 48, self.tftp.port);
            }
            Outbound::Hello => {}
        }
        bytes
    }
    pub(crate) fn queued(&mut self, token: Outbound) {
        match token {
            Outbound::Hello => self.hello_queued = true,
            Outbound::NewServer(service) | Outbound::DelServer(service) => {
                self.services[service.index()].queued = true
            }
            Outbound::NewTftpServer | Outbound::DelTftpServer => self.tftp.queued = true,
        }
    }
    /// Called only after the writer commits the whole GLINK datagram. GLINK
    /// completion still owns its TX slot and remote intent until RX_DONE.
    pub(crate) fn published(&mut self, token: Outbound) {
        match token {
            Outbound::Hello => {
                self.hello_sent = true;
                self.hello_queued = false;
            }
            Outbound::NewServer(service) | Outbound::DelServer(service) => {
                let local = &mut self.services[service.index()];
                local.announced = matches!(token, Outbound::NewServer(_));
                local.queued = false;
            }
            Outbound::NewTftpServer | Outbound::DelTftpServer => {
                self.tftp.announced = matches!(token, Outbound::NewTftpServer);
                self.tftp.queued = false;
            }
        }
    }
}
