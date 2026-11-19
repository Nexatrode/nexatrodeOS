//! Narrow RAM-only model of the observed Sunfish server-check TFTP write.
//! Optional exact-name negative replies for three absent temporary files.
//! No filesystem, firmware, device I/O, scheduling or QRTR publication.
#![forbid(unsafe_code)]

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
#[path = "qcom_mcfg_ram.rs"]
mod mcfg;
#[cfg(feature = "sunfish-tftp-mcfg-ram")]
use crate::qcom_tftp as tftp;

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
#[derive(Clone, Copy)]
struct McfgRead {
    options: tftp::Options,
    plan: tftp::ReadPlan,
    block: u16,
    complete: bool,
}

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
impl McfgRead {
    fn packet(&self, block: u16, snapshot: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        if block == 0 {
            return tftp::encode_options(self.options, self.plan, out).map_err(|_| Error::OutputTooSmall);
        }
        let size = usize::from(self.plan.block_size);
        let start = usize::from(block - 1) * size;
        let data = snapshot.get(start..).ok_or(Error::UnexpectedPacket)?;
        tftp::encode_data(block, &data[..data.len().min(size)], out).map_err(|_| Error::OutputTooSmall)
    }
}

#[cfg(feature = "sunfish-tftp-mcfg-trace")]
pub(crate) use mcfg::WRITE_OACK as MCFG_WRITE_OACK;

pub const SERVICE_PORT: u32 = 0x5000;
pub const TRANSFER_PORT: u32 = 0x5001;
#[cfg(not(feature = "sunfish-tftp-extended-sessions-experiment"))]
pub const TRANSFER_COUNT: usize = 4;
#[cfg(feature = "sunfish-tftp-extended-sessions-experiment")]
pub const TRANSFER_COUNT: usize = 32;
pub const fn is_transfer_port(port: u32) -> bool { port >= TRANSFER_PORT && port < TRANSFER_PORT + TRANSFER_COUNT as u32 }
pub const MAX_DATA: usize = 512;
pub const RAM_CAPACITY: usize = 8192;
pub const REQUEST: &[u8] = b"\0\x02/readwrite/server_check.txt\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0";
pub const OACK: &[u8] = b"\0\x06blksize\07680\0timeoutms\01000\0wsize\010\0";
/// Exact pinned tqftpserv failed-open reply; no contents are served.
#[cfg(feature = "sunfish-tftp-missing-temp-replies")]
pub const MISSING_REPLY: &[u8] = b"\x00\x05\x00\x01file not found\x00";

/// Only absent names in this boot's restricted RAM namespace. This is not
/// filesystem lookup or a general policy for arbitrary modem files.
#[cfg(feature = "sunfish-tftp-missing-temp-replies")]
fn missing_path(packet: &[u8]) -> Option<u8> {
    if packet.len() > 512 || !packet.starts_with(&[0, 1]) { return None; }
    let mut parts = packet[2..].split(|b| *b == 0);
    let path = parts.next()?;
    let kind = match path {
        b"/readwrite/mcfg.tmp" => 1,
        b"/readwrite/ota_firewall/ruleset" => 2,
        b"/readwrite/modem_efs" => 3,
        _ => return None,
    };
    if !parts.next()?.eq_ignore_ascii_case(b"octet") || packet.last() != Some(&0) { return None; }
    // No option negotiation is needed for a missing file. Still require
    // bounded, nonempty printable key/value pairs and their terminators.
    loop {
        let key = parts.next()?;
        if key.is_empty() { return parts.next().is_none().then_some(kind); }
        let value = parts.next()?;
        if value.is_empty() || !key.iter().chain(value).all(|b| (0x20..=0x7e).contains(b)) { return None; }
    }
}

pub(crate) const REQUEST_PREFIX: &[u8] = b"\0\x02/readwrite/server_check.txt\0";
const REQUEST_OPTIONS: &[u8] = b"\0blksize\07680\0timeoutms\01000\0wsize\010\0";

pub(crate) fn matches_request(packet: &[u8]) -> bool {
    packet.strip_prefix(REQUEST_PREFIX).is_some_and(|tail| {
        tail.len() == 5 + REQUEST_OPTIONS.len()
            && tail[..5].eq_ignore_ascii_case(b"octet")
            && &tail[5..] == REQUEST_OPTIONS
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address { pub node: u32, pub port: u32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route { pub source: Address, pub destination: Address }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { WrongRoute, UnexpectedRequest, UnexpectedPacket, Oversize, Capacity, ChangedRetransmission, OutputTooSmall,
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    McfgBusy,
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    McfgMalformed,
}

impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::WrongRoute => 1, Self::UnexpectedRequest => 2, Self::UnexpectedPacket => 3,
            Self::Oversize => 4, Self::Capacity => 5, Self::ChangedRetransmission => 6,
            Self::OutputTooSmall => 7,
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            Self::McfgBusy => 8,
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            Self::McfgMalformed => 9,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reply {
    pub route: Route, pub len: usize, pub completed: bool,
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub missing_kind: u8,
}

/// Each slot owns its peer, transfer port, and RAM; slots are never reused in
/// this boot, so a late packet cannot become another peer's write.
struct Transfer {
    #[cfg(feature = "sunfish-tftp-mcfg-ram")]
    mcfg_write: Option<mcfg::WriteTransfer>,
    #[cfg(feature = "sunfish-tftp-mcfg-ram")]
    mcfg_read: Option<McfgRead>,
    peer: Option<Address>,
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    abandoned: bool,
    bytes: [u8; RAM_CAPACITY],
    len: usize,
    last_len: usize,
    last_block: u16,
    complete: bool,
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    missing_kind: u8,
}
impl Transfer {
    const fn new() -> Self { Self {
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        mcfg_write: None,
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        mcfg_read: None,
        peer: None, #[cfg(feature = "sunfish-tftp-peer-close-experiment")] abandoned: false, bytes: [0; RAM_CAPACITY], len: 0, last_len: 0, last_block: 0, complete: false, #[cfg(feature = "sunfish-tftp-missing-temp-replies")] missing_kind: 0 } }
}

#[cfg(feature = "sunfish-tftp-rejection-capture")]
#[derive(Clone, Copy)]
pub struct RejectionCapture { pub route: Route, pub opcode: u32, pub length: u32, pub captured: u32, pub bytes: [u8; 60] }

pub struct ServerCheck {
    #[cfg(feature = "sunfish-tftp-mcfg-ram")]
    mcfg_file: mcfg::McfgRam,
    #[cfg(feature = "sunfish-tftp-mcfg-ram")]
    mcfg_reads_completed: u32,
    #[cfg(feature = "sunfish-tftp-rejection-capture")]
    rejection_capture: Option<RejectionCapture>,
    transfers: [Transfer; TRANSFER_COUNT],
    first_rejection: Option<(Error, u32)>,
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    missing_requests: u32,
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    first_missing_kind: u8,
}
impl Default for ServerCheck { fn default() -> Self { Self::new() } }
impl ServerCheck {
    pub const fn new() -> Self { Self {
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        mcfg_file: mcfg::McfgRam::new(),
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        mcfg_reads_completed: 0,
        #[cfg(feature = "sunfish-tftp-rejection-capture")] rejection_capture: None, transfers: [const { Transfer::new() }; TRANSFER_COUNT], first_rejection: None, #[cfg(feature = "sunfish-tftp-missing-temp-replies")] missing_requests: 0, #[cfg(feature = "sunfish-tftp-missing-temp-replies")] first_missing_kind: 0 } }
    /// The original first-transfer view remains available to existing callers.
    pub fn contents(&self) -> &[u8] { &self.transfers[0].bytes[..self.transfers[0].len] }
    pub const fn complete(&self) -> bool { self.transfers[0].complete }
    pub const fn last_block(&self) -> u16 { self.transfers[0].last_block }
    pub const fn first_rejection(&self) -> Option<(Error, u32)> { self.first_rejection }
    #[cfg(feature = "sunfish-tftp-rejection-capture")]
    pub const fn rejection_capture(&self) -> Option<RejectionCapture> { self.rejection_capture }
    pub fn active_count(&self) -> usize { self.transfers.iter().filter(|t| t.peer.is_some()).count() }
    pub fn total_bytes(&self) -> usize { self.transfers.iter().map(|t| t.len).sum() }
    pub fn completed_count(&self) -> usize { self.transfers.iter().filter(|t| t.complete).count() }
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub const fn missing_requests(&self) -> u32 { self.missing_requests }
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub const fn first_missing_kind(&self) -> u8 { self.first_missing_kind }


    #[cfg(feature = "sunfish-tftp-mcfg-ram")]
    pub fn mcfg_progress(&self) -> (u32, Option<usize>, u32) {
        let writes = self.transfers.iter().filter(|slot| slot.mcfg_write.is_some()).count() as u32;
        (writes, self.mcfg_file.len(), self.mcfg_reads_completed)
    }

    /// Check whether an exact peer has an empty, incomplete transfer. The
    /// caller must hold exclusive access across preflight and any later commit.
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    pub fn can_abandon_empty_peer(&self, peer: Address) -> Result<(), Error> {
        let transfer = self.transfers.iter().find(|t| t.peer == Some(peer)).ok_or(Error::WrongRoute)?;
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        if transfer.missing_kind != 0 { return Err(Error::UnexpectedPacket); }
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        if transfer.mcfg_write.is_some() || transfer.mcfg_read.is_some() { return Err(Error::UnexpectedPacket); }
        if transfer.abandoned || transfer.len != 0 || transfer.last_block != 0 || transfer.complete {
            return Err(Error::UnexpectedPacket);
        }
        Ok(())
    }

    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    pub fn can_abandon_empty_peer_at(&self, peer: Address, transfer_port: u32) -> Result<(), Error> {
        self.can_abandon_empty_peer(peer)?;
        let index = self.transfers.iter().position(|t| t.peer == Some(peer)).ok_or(Error::WrongRoute)?;
        if transfer_port != TRANSFER_PORT + index as u32 { return Err(Error::WrongRoute); }
        Ok(())
    }

    /// Retire only an existing empty, incomplete server-check transfer.
    /// Keep its peer and port reserved so late packets cannot attach to a
    /// different transfer. No state changes on an unmatched or unsafe peer.
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    pub fn abandon_empty_peer(&mut self, peer: Address) -> Result<(), Error> {
        self.can_abandon_empty_peer(peer)?;
        let transfer = self.transfers.iter_mut().find(|t| t.peer == Some(peer)).ok_or(Error::WrongRoute)?;
        transfer.abandoned = true;
        Ok(())
    }

    /// Input route is from the authenticated peer to the local endpoint.
    /// Output is retained by the caller until publication succeeds.
    pub fn receive(&mut self, route: Route, packet: &[u8], out: &mut [u8]) -> Result<Reply, Error> {
        let result = self.receive_inner(route, packet, out);
        if let Err(error) = result {
            if self.first_rejection.is_none() {
                self.first_rejection = Some((error, packet.len().min(u32::MAX as usize) as u32));
                #[cfg(feature = "sunfish-tftp-rejection-capture")]
                {
                    let opcode = if packet.len() >= 2 { u16::from_be_bytes([packet[0], packet[1]]) as u32 } else { u32::MAX };
                    let mut bytes = [0; 60];
                    // Capture request metadata only, never DATA or arbitrary payloads.
                    let n = if opcode == 1 || opcode == 2 { packet.len().min(bytes.len()) } else { 0 };
                    bytes[..n].copy_from_slice(&packet[..n]);
                    self.rejection_capture = Some(RejectionCapture { route, opcode, length: packet.len().min(u32::MAX as usize) as u32, captured: n as u32, bytes });
                }
            }
        }
        result
    }

    fn receive_inner(&mut self, route: Route, packet: &[u8], out: &mut [u8]) -> Result<Reply, Error> {
        if route.source.node != 0 || route.destination.node != 1 || route.source.port == 0
            || route.source.port >= 0xffff_fffe { return Err(Error::WrongRoute); }
        if route.destination.port == SERVICE_PORT {
            #[cfg(feature = "sunfish-tftp-mcfg-ram")]
            if let Some(reply) = self.mcfg_request(route, packet, out)? { return Ok(reply); }
            #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
            if let Some(kind) = missing_path(packet) {
                if out.len() < MISSING_REPLY.len() { return Err(Error::OutputTooSmall); }
                let index = match self.transfers.iter().position(|t| t.peer == Some(route.source)) {
                    Some(index) => {
                        if self.transfers[index].missing_kind != kind { return Err(Error::UnexpectedRequest); }
                        index
                    }
                    None => self.transfers.iter().position(|t| t.peer.is_none()).ok_or(Error::Capacity)?,
                };
                self.transfers[index].peer = Some(route.source);
                self.transfers[index].missing_kind = kind;
                self.missing_requests = self.missing_requests.saturating_add(1);
                if self.first_missing_kind == 0 { self.first_missing_kind = kind; }
                out[..MISSING_REPLY.len()].copy_from_slice(MISSING_REPLY);
                return Ok(Reply { route: Route { source: Address { node: 1, port: TRANSFER_PORT + index as u32 }, destination: route.source },
                    len: MISSING_REPLY.len(), completed: false, missing_kind: kind });
            }
            if !matches_request(packet) { return Err(Error::UnexpectedRequest); }
            if out.len() < OACK.len() { return Err(Error::OutputTooSmall); }
            let index = match self.transfers.iter().position(|t| t.peer == Some(route.source)) {
                Some(index) => {
                    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
                    if self.transfers[index].missing_kind != 0 { return Err(Error::UnexpectedRequest); }
                    #[cfg(feature = "sunfish-tftp-mcfg-ram")]
                    if self.transfers[index].mcfg_write.is_some() || self.transfers[index].mcfg_read.is_some() {
                        return Err(Error::UnexpectedRequest);
                    }
                    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
                    if self.transfers[index].abandoned { return Err(Error::UnexpectedRequest); }
                    if self.transfers[index].last_block != 0 { return Err(Error::UnexpectedRequest); }
                    index
                }
                None => self.transfers.iter().position(|t| t.peer.is_none()).ok_or(Error::Capacity)?,
            };
            self.transfers[index].peer = Some(route.source);
            out[..OACK.len()].copy_from_slice(OACK);
            return Ok(Reply { route: Route { source: Address { node: 1, port: TRANSFER_PORT + index as u32 }, destination: route.source }, len: OACK.len(), completed: false, #[cfg(feature = "sunfish-tftp-missing-temp-replies")] missing_kind: 0 });
        }
        if !is_transfer_port(route.destination.port) { return Err(Error::WrongRoute); }
        let index = (route.destination.port - TRANSFER_PORT) as usize;
        let transfer = &mut self.transfers[index];
        if transfer.peer != Some(route.source) { return Err(Error::WrongRoute); }
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        if let Some(write) = transfer.mcfg_write.as_mut() {
            if tftp::decode_error(packet).is_ok() {
                write.cancel(&mut self.mcfg_file).map_err(mcfg_error)?;
                return Ok(mcfg_reply(route.destination, route.source, 0, false));
            }
            let len = write.data(&mut self.mcfg_file, route.source.port, packet, out)
                .map_err(mcfg_error)?;
            return Ok(mcfg_reply(route.destination, route.source, len, write.complete()));
        }
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        if let Some(read) = transfer.mcfg_read.as_mut() {
            if tftp::decode_error(packet).is_ok() {
                read.complete = true;
                return Ok(mcfg_reply(route.destination, route.source, 0, false));
            }
            let ack = tftp::decode_ack(packet).map_err(|_| Error::UnexpectedPacket)?;
            let snapshot = &transfer.bytes[..read.plan.transfer_size as usize];
            let len = if read.complete {
                if ack != read.block { return Err(Error::UnexpectedPacket); }
                0
            } else if ack == read.block {
                let size = usize::from(read.plan.block_size);
                if ack != 0 && usize::from(ack) * size > snapshot.len() {
                    read.complete = true;
                    self.mcfg_reads_completed = self.mcfg_reads_completed.saturating_add(1);
                    0
                } else {
                    let next = ack.checked_add(1).ok_or(Error::UnexpectedPacket)?;
                    let len = read.packet(next, snapshot, out)?;
                    read.block = next;
                    len
                }
            } else if ack.checked_add(1) == Some(read.block) {
                read.packet(read.block, snapshot, out)?
            } else {
                return Err(Error::UnexpectedPacket);
            };
            return Ok(mcfg_reply(route.destination, route.source, len, false));
        }
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        if transfer.missing_kind != 0 { return Err(Error::UnexpectedPacket); }
        #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
        if transfer.abandoned { return Err(Error::UnexpectedPacket); }
        if packet.len() < 4 || packet[0..2] != [0, 3] { return Err(Error::UnexpectedPacket); }
        let block = u16::from_be_bytes([packet[2], packet[3]]);
        let data = &packet[4..];
        if data.len() > MAX_DATA { return Err(Error::Oversize); }
        if out.len() < 4 { return Err(Error::OutputTooSmall); }
        if block == transfer.last_block && block != 0 {
            if data.len() != transfer.last_len || transfer.bytes[transfer.len - transfer.last_len..transfer.len] != *data {
                return Err(Error::ChangedRetransmission);
            }
        } else {
            if transfer.complete || transfer.last_block.checked_add(1) != Some(block) { return Err(Error::UnexpectedPacket); }
            let end = transfer.len.checked_add(data.len()).filter(|end| *end <= RAM_CAPACITY).ok_or(Error::Capacity)?;
            transfer.bytes[transfer.len..end].copy_from_slice(data);
            transfer.len = end;
            transfer.last_len = data.len();
            transfer.last_block = block;
            transfer.complete = data.len() < MAX_DATA;
        }
        out[..4].copy_from_slice(&[0, 4, packet[2], packet[3]]);
        Ok(Reply { route: Route { source: route.destination, destination: route.source }, len: 4, completed: transfer.complete, #[cfg(feature = "sunfish-tftp-missing-temp-replies")] missing_kind: 0 })
    }
}

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
fn mcfg_reply(source: Address, destination: Address, len: usize, completed: bool) -> Reply {
    Reply { route: Route { source, destination }, len, completed, missing_kind: 0 }
}

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
fn mcfg_error(error: mcfg::Error) -> Error {
    match error {
        mcfg::Error::Capacity => Error::Capacity,
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        mcfg::Error::Busy => Error::McfgBusy,
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        mcfg::Error::BadRequest => Error::McfgMalformed,
        mcfg::Error::OutputTooSmall => Error::OutputTooSmall,
        mcfg::Error::ChangedRetry => Error::ChangedRetransmission,
        _ => Error::UnexpectedPacket,
    }
}

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
impl ServerCheck {
    fn mcfg_request(&mut self, route: Route, packet: &[u8], out: &mut [u8]) -> Result<Option<Reply>, Error> {
        let write = packet.starts_with(b"\0\x02/readwrite/mcfg.tmp\0");
        let read = packet.starts_with(b"\0\x01/readwrite/mcfg.tmp\0");
        if !write && !read { return Ok(None); }
        let file_len = self.mcfg_file.len();
        if read && file_len.is_none() { return Ok(None); }

        let existing = self.transfers.iter().position(|slot| slot.peer == Some(route.source));
        let index = existing.or_else(|| self.transfers.iter().position(|slot| slot.peer.is_none()))
            .ok_or(Error::Capacity)?;
        let slot = &mut self.transfers[index];
        let source = Address { node: 1, port: TRANSFER_PORT + index as u32 };
        let len = if write {
            if existing.is_some() && !slot.mcfg_write.as_ref().is_some_and(|write| !write.complete()) {
                return Err(Error::UnexpectedRequest);
            }
            let (transfer, len) = mcfg::WriteTransfer::start(&mut self.mcfg_file, route.source.port, packet, out)
                .map_err(mcfg_error)?;
            slot.mcfg_write = Some(transfer);
            len
        } else {
            let options = tftp::decode_mcfg_read_options(packet).map_err(|_| Error::UnexpectedRequest)?;
            if existing.is_some() {
                let state = slot.mcfg_read.filter(|state| !state.complete && state.options == options)
                    .ok_or(Error::UnexpectedRequest)?;
                // A repeated RRQ may recover the initial reply, but cannot rewind an active transfer.
                let initial = if options.has_supported_options() { 0 } else { 1 };
                if state.block != initial { return Err(Error::UnexpectedRequest); }
                state.packet(initial, &slot.bytes[..state.plan.transfer_size as usize], out)?
            } else {
                let mut plan = tftp::ReadPlan::for_options(options, file_len.ok_or(Error::UnexpectedRequest)? as u64)
                    .map_err(|_| Error::UnexpectedRequest)?;
                plan.block_size = plan.block_size.min(512);
                plan.window_size = 1;
                let snapshot = &mut slot.bytes[..plan.transfer_size as usize];
                self.mcfg_file.read(plan.start_offset as usize, snapshot).map_err(mcfg_error)?;
                let block = if options.has_supported_options() { 0 } else { 1 };
                let state = McfgRead { options, plan, block, complete: false };
                let len = state.packet(block, snapshot, out)?;
                slot.mcfg_read = Some(state);
                len
            }
        };
        slot.peer = Some(route.source);
        Ok(Some(mcfg_reply(source, route.source, len, false)))
    }
}
