//! Bounded, read-only TFTP request policy for the Qualcomm QRTR file service.
//!
//! This module is the protocol core only. It does not create QRTR sockets,
//! publish service 4096, or provide firmware bytes. A caller must supply a
//! separately authorized immutable file source before it can serve anything.
#![forbid(unsafe_code)]

use core::cell::RefCell;

pub const MAX_QRTR_DATAGRAM_BYTES: usize = 4096;
pub const QRTR_HEADER_BYTES: usize = 32;
/// Maximum raw TFTP packet that fits after the QRTR v1 header.
pub const MAX_REQUEST_BYTES: usize = MAX_QRTR_DATAGRAM_BYTES - QRTR_HEADER_BYTES;
pub const MAX_OBSERVED_PATH_BYTES: usize = 128;
pub const MAX_OBSERVED_MODE_BYTES: usize = 16;
pub const MIN_BLOCK_SIZE: u16 = 8;
/// Largest DATA payload that fits in one QRTR datagram after QRTR and TFTP headers.
pub const MAX_BLOCK_SIZE: u16 = (MAX_REQUEST_BYTES - 4) as u16;
/// RFC 2348's maximum requested blksize; negotiated plans may choose less.
pub const MAX_TFTP_BLOCK_SIZE: u16 = 65_464;
pub const MAX_TIMEOUT_MS: u32 = 255_000;
pub const MAX_READ_SIZE: u32 = 100 * 1024 * 1024;

pub const OP_RRQ: u16 = 1;
pub const OP_WRQ: u16 = 2;
pub const OP_DATA: u16 = 3;
pub const OP_ACK: u16 = 4;
pub const OP_ERROR: u16 = 5;
pub const OP_OACK: u16 = 6;

pub const ERROR_ACCESS: u16 = 2;
pub const ERROR_ILLEGAL_OPERATION: u16 = 4;
pub const ERROR_OPTION_NEGOTIATION: u16 = 8;
/// Qualcomm's TFTP reference uses this custom ERROR code to end a size probe.
pub const ERROR_END_OF_TRANSFER: u16 = 9;

const OPT_BLKSIZE: u8 = 1 << 0;
const OPT_TIMEOUTMS: u8 = 1 << 1;
const OPT_TSIZE: u8 = 1 << 2;
const OPT_RSIZE: u8 = 1 << 3;
const OPT_WSIZE: u8 = 1 << 4;
const OPT_SEEK: u8 = 1 << 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    ShortPacket,
    PacketTooLarge,
    InvalidAck,
    UnsupportedOpcode,
    WriteDenied,
    UnterminatedField,
    EmptyField,
    UnsupportedMode,
    UnavailablePath,
    MalformedOptions,
    InvalidOptionValue,
    DuplicateOption,
    SeekPastEnd,
    BufferTooSmall,
    InvalidTransferRange,
    InvalidErrorPacket,
    SourceRead,
}

/// Bounded random-access read interface for an immutable firmware source.
///
/// `len` must describe the same retained object for the whole transfer.
/// `read_exact_at` must fill the entire destination or return an error; the
/// transfer never retries a partial or uncertain read. Implementations backed
/// by storage must revalidate their live read-only lease on each call.
pub trait ReadSource {
    fn len(&self) -> Result<u64, ()>;
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ()>;
}

/// Mutable backing-I/O interface for a single-owner storage provider.
///
/// Implementations should revalidate their live read-only lease on every
/// method. `len` is sampled when a [`ReadSourceCell`] is created and before
/// and after each nonempty read; a changed length or uncertain operation
/// fails the enclosing TFTP transfer closed.
pub trait ReadSourceIo {
    fn len(&mut self) -> Result<u64, ()>;
    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()>;
}

/// No-allocation, runtime-borrow-checked adapter from mutable storage I/O to
/// the shared-reference [`ReadSource`] contract used by transfer sessions.
///
/// The owner must serialize calls; overlapping or reentrant reads return an
/// error instead of panicking. This wrapper supplies no image authorization.
pub struct ReadSourceCell<T: ReadSourceIo> {
    source: RefCell<T>,
    len: u64,
}

impl<T: ReadSourceIo> ReadSourceCell<T> {
    pub fn new(mut source: T) -> Result<Self, ()> {
        let len = source.len()?;
        Ok(Self {
            source: RefCell::new(source),
            len,
        })
    }
}

impl<T: ReadSourceIo> ReadSource for ReadSourceCell<T> {
    fn len(&self) -> Result<u64, ()> {
        let mut source = self.source.try_borrow_mut().map_err(|_| ())?;
        if source.len()? != self.len {
            return Err(());
        }
        Ok(self.len)
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        let end = offset.checked_add(out.len() as u64).ok_or(())?;
        if end > self.len {
            return Err(());
        }
        let mut source = self.source.try_borrow_mut().map_err(|_| ())?;
        if source.len()? != self.len {
            return Err(());
        }
        if !out.is_empty() {
            source.read_exact_at(offset, out)?;
            if source.len()? != self.len {
                return Err(());
            }
        }
        Ok(())
    }
}

impl ReadSource for [u8] {
    fn len(&self) -> Result<u64, ()> {
        Ok(<[u8]>::len(self) as u64)
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        let source = self.get(start..end).ok_or(())?;
        out.copy_from_slice(source);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    /// Firmware strings identify these aliases for the modem's WLAN PD loader.
    /// The enum conveys path policy only; it does not prove a file is available
    /// or that its redistribution is authorized.
    WlanMdspMbn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    pub block_size: u16,
    pub timeout_ms: u32,
    pub read_size: Option<u32>,
    pub window_size: u16,
    pub seek: u64,
    pub request_file_size: bool,
    offered: u8,
}

impl Options {
    const fn defaults() -> Self {
        Self {
            block_size: 512,
            timeout_ms: 1000,
            read_size: None,
            window_size: 1,
            seek: 0,
            request_file_size: false,
            offered: 0,
        }
    }

    /// An OACK is only needed when at least one supported option was offered.
    pub const fn has_supported_options(self) -> bool {
        self.offered != 0
    }

    /// Bit mask of the bounded options parsed from the request.
    pub const fn offered_mask(self) -> u8 {
        self.offered
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadRequest {
    pub resource: Resource,
    pub options: Options,
}

/// Payload-free summary of one raw TFTP datagram for an observation run.
/// Text fields contain printable ASCII only; no DATA bytes or backing slice is
/// retained. Unknown paths and denied operations remain observable as errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestObservation {
    pub opcode: u16,
    pub path: [u8; MAX_OBSERVED_PATH_BYTES],
    pub path_len: u8,
    pub mode: [u8; MAX_OBSERVED_MODE_BYTES],
    pub mode_len: u8,
    /// Bytes after the mode terminator; metadata only, not validated options.
    pub option_bytes: u16,
    /// Bounded raw option prefix, captured only for RRQ/WRQ metadata.
    pub option_prefix: [u8; 40],
    pub option_prefix_len: u8,
    pub resource: Option<Resource>,
    pub options: Option<Options>,
    pub error: Option<Error>,
}

impl RequestObservation {
    pub fn path_text(&self) -> &str {
        let len = usize::from(self.path_len).min(self.path.len());
        core::str::from_utf8(&self.path[..len]).unwrap_or("")
    }

    pub fn mode_text(&self) -> &str {
        let len = usize::from(self.mode_len).min(self.mode.len());
        core::str::from_utf8(&self.mode[..len]).unwrap_or("")
    }

    pub fn path_part(&self, index: usize) -> &str {
        let start = index.saturating_mul(32);
        let len = usize::from(self.path_len).min(self.path.len());
        if start >= len {
            return "";
        }
        let end = (start + 32).min(len);
        core::str::from_utf8(&self.path[start..end]).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadPlan {
    pub file_size: u64,
    pub start_offset: u64,
    pub transfer_size: u64,
    pub block_size: u16,
    pub window_size: u16,
}

/// One packet or wait-state from a single read-only TFTP transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferProgress {
    Packet {
        bytes: usize,
        ack_after: Option<u16>,
    },
    WaitingForAck(u16),
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AckOutcome {
    Accepted,
    Duplicate,
    Unexpected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteErrorOutcome {
    /// Qualcomm's stat-like size probe is intentionally ended by the client.
    EndOfTransfer,
    Failure {
        code: u16,
    },
}

/// Bounded per-peer read-transfer state over one immutable caller-owned image.
///
/// This type never opens files, allocates, publishes a QRTR service, or sends
/// packets. The transport must bind ACKs to the original peer/endpoint and
/// handle timeout/retransmission policy outside this state machine. Supply
/// bytes only from an independently authorized immutable firmware source.
pub struct ReadSession<'a, S: ReadSource + ?Sized = [u8]> {
    request: ReadRequest,
    plan: ReadPlan,
    source: &'a S,
    started: bool,
    awaiting_oack_ack: bool,
    pending_ack: Option<u16>,
    last_ack: Option<u16>,
    offset: u64,
    next_block: u64,
    window_packets: u16,
    final_sent: bool,
    complete: bool,
    source_faulted: bool,
}

impl<'a> ReadSession<'a, [u8]> {
    pub fn new(request: ReadRequest, source: &'a [u8]) -> Result<Self, Error> {
        Self::with_source(request, source)
    }
}

impl<'a, S: ReadSource + ?Sized> ReadSession<'a, S> {
    /// Recreate an unacknowledged initial response without advancing the session.
    pub fn repeat_initial_packet(&self, request: ReadRequest, out: &mut [u8]) -> Result<Option<usize>, Error> {
        if request != self.request || self.last_ack.is_some() {
            return Ok(None);
        }
        if self.source_faulted || self.source.len() != Ok(self.plan.file_size) {
            return Err(Error::SourceRead);
        }
        if self.awaiting_oack_ack && self.pending_ack == Some(0) {
            return encode_oack(self.request, self.plan, out).map(Some);
        }
        if !self.request.options.has_supported_options() && self.started && self.pending_ack == Some(1) {
            let payload_len = usize::try_from(self.plan.transfer_size.min(u64::from(self.plan.block_size)))
                .map_err(|_| Error::InvalidTransferRange)?;
            let len = payload_len.checked_add(4).ok_or(Error::InvalidTransferRange)?;
            if out.len() < len { return Err(Error::BufferTooSmall); }
            self.source.read_exact_at(self.plan.start_offset, &mut out[4..len]).map_err(|_| Error::SourceRead)?;
            out[..4].copy_from_slice(&[0, OP_DATA as u8, 0, 1]);
            return Ok(Some(len));
        }
        Ok(None)
    }

    /// Create a transfer over a retained random-access source without loading
    /// the complete file into memory.
    pub fn with_source(request: ReadRequest, source: &'a S) -> Result<Self, Error> {
        let file_size = source.len().map_err(|_| Error::SourceRead)?;
        let plan = ReadPlan::new(request, file_size)?;
        Ok(Self {
            request,
            plan,
            source,
            started: false,
            awaiting_oack_ack: false,
            pending_ack: None,
            last_ack: None,
            offset: plan.start_offset,
            next_block: 1,
            window_packets: 0,
            final_sent: false,
            complete: false,
            source_faulted: false,
        })
    }

    /// Produce the next OACK/DATA packet into caller-owned storage.
    ///
    /// DATA packets are returned one at a time so the transport can apply its
    /// own work budget. `ack_after` is set on OACK and at the end of each data
    /// window, or immediately for a short final data block.
    pub fn next_packet(&mut self, out: &mut [u8]) -> Result<TransferProgress, Error> {
        if self.source_faulted {
            return Err(Error::SourceRead);
        }
        if self.complete {
            return Ok(TransferProgress::Complete);
        }
        if let Some(block) = self.pending_ack {
            return Ok(TransferProgress::WaitingForAck(block));
        }
        if self.source.len() != Ok(self.plan.file_size) {
            self.source_faulted = true;
            return Err(Error::SourceRead);
        }

        if !self.started && self.request.options.has_supported_options() {
            let bytes = encode_oack(self.request, self.plan, out)?;
            self.started = true;
            self.awaiting_oack_ack = true;
            self.pending_ack = Some(0);
            return Ok(TransferProgress::Packet {
                bytes,
                ack_after: Some(0),
            });
        }
        self.started = true;

        let transfer_end = self
            .plan
            .start_offset
            .checked_add(self.plan.transfer_size)
            .ok_or(Error::InvalidTransferRange)?;
        if self.offset > transfer_end {
            return Err(Error::InvalidTransferRange);
        }
        let remaining = transfer_end - self.offset;
        let payload_len = usize::try_from(remaining.min(u64::from(self.plan.block_size)))
            .map_err(|_| Error::InvalidTransferRange)?;
        let packet_len = 4usize
            .checked_add(payload_len)
            .ok_or(Error::InvalidTransferRange)?;
        if out.len() < packet_len {
            return Err(Error::BufferTooSmall);
        }
        let payload_end = 4usize
            .checked_add(payload_len)
            .ok_or(Error::InvalidTransferRange)?;
        if self
            .source
            .read_exact_at(self.offset, &mut out[4..payload_end])
            .is_err()
        {
            self.source_faulted = true;
            return Err(Error::SourceRead);
        }
        let block = self.next_block as u16;
        out[..2].copy_from_slice(&OP_DATA.to_be_bytes());
        out[2..4].copy_from_slice(&block.to_be_bytes());

        self.offset = self
            .offset
            .checked_add(payload_len as u64)
            .ok_or(Error::InvalidTransferRange)?;
        self.next_block = self.next_block.wrapping_add(1);
        self.window_packets = self.window_packets.saturating_add(1);
        self.final_sent = payload_len < usize::from(self.plan.block_size);
        if self.final_sent || self.window_packets >= self.plan.window_size {
            self.pending_ack = Some(block);
        }

        Ok(TransferProgress::Packet {
            bytes: packet_len,
            ack_after: self.pending_ack,
        })
    }

    /// Accept only the ACK needed to advance this session. The caller must
    /// first verify that the datagram arrived from this session's QRTR peer.
    pub fn receive_ack(&mut self, packet: &[u8]) -> Result<AckOutcome, Error> {
        let block = decode_ack(packet)?;
        if self.source_faulted {
            return Ok(AckOutcome::Unexpected);
        }
        if self.complete {
            return Ok(AckOutcome::Unexpected);
        }
        if let Some(expected) = self.pending_ack {
            if block == expected {
                self.pending_ack = None;
                self.last_ack = Some(block);
                if self.awaiting_oack_ack {
                    self.awaiting_oack_ack = false;
                    self.window_packets = 0;
                } else if self.final_sent {
                    self.complete = true;
                } else {
                    self.window_packets = 0;
                }
                return Ok(AckOutcome::Accepted);
            }
        }
        if self.last_ack == Some(block) {
            Ok(AckOutcome::Duplicate)
        } else {
            Ok(AckOutcome::Unexpected)
        }
    }

    /// Accept a well-formed remote ERROR and terminate this read session.
    /// Code 9 is Qualcomm's non-failure end-of-transfer marker used after a
    /// stat-like `tsize=0` probe; other codes terminate with a remote failure.
    pub fn receive_error(&mut self, packet: &[u8]) -> Result<RemoteErrorOutcome, Error> {
        let (code, _) = decode_error(packet)?;
        if self.complete {
            return Err(Error::InvalidTransferRange);
        }
        self.pending_ack = None;
        self.awaiting_oack_ack = false;
        self.complete = true;
        if code == ERROR_END_OF_TRANSFER {
            Ok(RemoteErrorOutcome::EndOfTransfer)
        } else {
            Ok(RemoteErrorOutcome::Failure { code })
        }
    }
}

fn ascii_eq_ignore_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn read_cstring<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a [u8], Error> {
    let tail = bytes.get(*at..).ok_or(Error::UnterminatedField)?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Error::UnterminatedField)?;
    if end == 0 {
        return Err(Error::EmptyField);
    }
    *at += end + 1;
    Ok(&tail[..end])
}

fn decimal(bytes: &[u8]) -> Result<u64, Error> {
    if bytes.is_empty() {
        return Err(Error::InvalidOptionValue);
    }
    bytes.iter().try_fold(0u64, |value, byte| {
        if !byte.is_ascii_digit() {
            return Err(Error::InvalidOptionValue);
        }
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(byte - b'0')))
            .ok_or(Error::InvalidOptionValue)
    })
}

fn set_once(options: &mut Options, bit: u8) -> Result<(), Error> {
    if options.offered & bit != 0 {
        return Err(Error::DuplicateOption);
    }
    options.offered |= bit;
    Ok(())
}

fn parse_options(bytes: &[u8], options: &mut Options) -> Result<(), Error> {
    let mut at = 0;
    while at < bytes.len() {
        let name = read_cstring(bytes, &mut at).map_err(|_| Error::MalformedOptions)?;
        let value = read_cstring(bytes, &mut at).map_err(|_| Error::MalformedOptions)?;
        match name {
            b"blksize" => {
                set_once(options, OPT_BLKSIZE)?;
                let value = decimal(value)?;
                if !(u64::from(MIN_BLOCK_SIZE)..=u64::from(MAX_TFTP_BLOCK_SIZE)).contains(&value) {
                    return Err(Error::InvalidOptionValue);
                }
                options.block_size = value as u16;
            }
            b"timeoutms" => {
                set_once(options, OPT_TIMEOUTMS)?;
                let value = decimal(value)?;
                if !(1..=u64::from(MAX_TIMEOUT_MS)).contains(&value) {
                    return Err(Error::InvalidOptionValue);
                }
                options.timeout_ms = value as u32;
            }
            b"tsize" => {
                set_once(options, OPT_TSIZE)?;
                if decimal(value)? != 0 {
                    return Err(Error::InvalidOptionValue);
                }
                options.request_file_size = true;
            }
            b"rsize" => {
                set_once(options, OPT_RSIZE)?;
                let value = decimal(value)?;
                if !(1..=u64::from(MAX_READ_SIZE)).contains(&value) {
                    return Err(Error::InvalidOptionValue);
                }
                options.read_size = Some(value as u32);
            }
            b"wsize" => {
                set_once(options, OPT_WSIZE)?;
                let value = decimal(value)?;
                if !(1..=u64::from(u16::MAX)).contains(&value) {
                    return Err(Error::InvalidOptionValue);
                }
                options.window_size = value as u16;
            }
            b"seek" => {
                set_once(options, OPT_SEEK)?;
                options.seek = decimal(value)?;
            }
            // Like the pinned reference server, unknown options are ignored.
            _ => {}
        }
    }
    Ok(())
}

/// Decode an RRQ in octet mode. Only the three firmware-observed WLAN image
/// aliases are eligible; path traversal, arbitrary files, and all writes are
/// denied. No file is opened by this function.
pub fn decode_read_request(bytes: &[u8]) -> Result<ReadRequest, Error> {
    if bytes.len() < 2 {
        return Err(Error::ShortPacket);
    }
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Error::PacketTooLarge);
    }
    let opcode = u16::from_be_bytes([bytes[0], bytes[1]]);
    if opcode == OP_WRQ {
        return Err(Error::WriteDenied);
    }
    if opcode != OP_RRQ {
        return Err(Error::UnsupportedOpcode);
    }

    let mut at = 2;
    let path = read_cstring(bytes, &mut at)?;
    let resource = match path {
        b"/readonly/firmware/image/wlanmdsp.mbn"
        | b"/readonly/vendor/firmware_mnt/image/wlanmdsp.mbn"
        | b"/readonly/vendor/firmware/wlanmdsp.mbn" => Resource::WlanMdspMbn,
        _ => return Err(Error::UnavailablePath),
    };
    let mode = read_cstring(bytes, &mut at)?;
    if !ascii_eq_ignore_case(mode, b"octet") {
        return Err(Error::UnsupportedMode);
    }

    let mut options = Options::defaults();
    if at < bytes.len() {
        parse_options(&bytes[at..], &mut options)?;
    }
    Ok(ReadRequest { resource, options })
}

/// Capture RRQ/WRQ metadata without preserving a packet payload or authorizing
/// any file operation. This is for a bounded diagnostic observer; a production
/// server must separately validate policy and supply an authorized file source.
pub fn observe_request(bytes: &[u8]) -> Result<RequestObservation, Error> {
    if bytes.len() < 2 {
        return Err(Error::ShortPacket);
    }
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Error::PacketTooLarge);
    }
    let opcode = u16::from_be_bytes([bytes[0], bytes[1]]);
    let mut observation = RequestObservation {
        opcode,
        path: [0; MAX_OBSERVED_PATH_BYTES],
        path_len: 0,
        mode: [0; MAX_OBSERVED_MODE_BYTES],
        mode_len: 0,
        option_bytes: 0,
        option_prefix: [0; 40],
        option_prefix_len: 0,
        resource: None,
        options: None,
        error: None,
    };
    if !matches!(opcode, OP_RRQ | OP_WRQ) {
        observation.error = Some(if opcode == OP_ACK {
            Error::InvalidAck
        } else {
            Error::UnsupportedOpcode
        });
        return Ok(observation);
    }

    let mut at = 2;
    let path = read_cstring(bytes, &mut at)?;
    let mode = read_cstring(bytes, &mut at)?;
    if path.len() > observation.path.len() || mode.len() > observation.mode.len() {
        return Err(Error::PacketTooLarge);
    }
    for (dst, src) in observation.path.iter_mut().zip(path) {
        *dst = if (0x20..=0x7e).contains(src) {
            *src
        } else {
            b'?'
        };
    }
    for (dst, src) in observation.mode.iter_mut().zip(mode) {
        *dst = if (0x20..=0x7e).contains(src) {
            *src
        } else {
            b'?'
        };
    }
    observation.path_len = path.len() as u8;
    observation.mode_len = mode.len() as u8;
    observation.option_bytes = (bytes.len() - at) as u16;
    let option_len = (bytes.len() - at).min(observation.option_prefix.len());
    observation.option_prefix[..option_len].copy_from_slice(&bytes[at..at + option_len]);
    observation.option_prefix_len = option_len as u8;

    match decode_read_request(bytes) {
        Ok(request) => {
            observation.resource = Some(request.resource);
            observation.options = Some(request.options);
        }
        Err(error) => {
            observation.error = Some(error);
            if opcode == OP_RRQ && ascii_eq_ignore_case(mode, b"octet") && at < bytes.len() {
                let mut options = Options::defaults();
                if parse_options(&bytes[at..], &mut options).is_ok() {
                    observation.options = Some(options);
                }
            }
        }
    }
    Ok(observation)
}

#[cfg(feature = "sunfish-tftp-mcfg-ram")]
pub(crate) fn decode_mcfg_read_options(bytes: &[u8]) -> Result<Options, Error> {
    let mut at = 2;
    if bytes.len() > 512 || !bytes.starts_with(&OP_RRQ.to_be_bytes())
        || read_cstring(bytes, &mut at)? != b"/readwrite/mcfg.tmp"
    {
        return Err(Error::UnavailablePath);
    }
    if !ascii_eq_ignore_case(read_cstring(bytes, &mut at)?, b"octet") {
        return Err(Error::UnsupportedMode);
    }
    let mut options = Options::defaults();
    parse_options(&bytes[at..], &mut options)?;
    Ok(options)
}

pub fn decode_ack(bytes: &[u8]) -> Result<u16, Error> {
    if bytes.len() != 4 {
        return Err(Error::InvalidAck);
    }
    if u16::from_be_bytes([bytes[0], bytes[1]]) != OP_ACK {
        return Err(Error::InvalidAck);
    }
    Ok(u16::from_be_bytes([bytes[2], bytes[3]]))
}

/// Decode a TFTP ERROR packet, validating its single NUL-terminated message.
pub fn decode_error(bytes: &[u8]) -> Result<(u16, &[u8]), Error> {
    if bytes.len() < 5
        || bytes.len() > MAX_REQUEST_BYTES
        || u16::from_be_bytes([bytes[0], bytes[1]]) != OP_ERROR
        || bytes.last() != Some(&0)
        || bytes[4..bytes.len() - 1].contains(&0)
    {
        return Err(Error::InvalidErrorPacket);
    }
    Ok((
        u16::from_be_bytes([bytes[2], bytes[3]]),
        &bytes[4..bytes.len() - 1],
    ))
}

pub const fn error_code(error: Error) -> u16 {
    match error {
        Error::WriteDenied | Error::UnavailablePath => ERROR_ACCESS,
        Error::MalformedOptions | Error::InvalidOptionValue | Error::DuplicateOption => {
            ERROR_OPTION_NEGOTIATION
        }
        Error::UnsupportedOpcode | Error::UnsupportedMode => ERROR_ILLEGAL_OPERATION,
        Error::ShortPacket
        | Error::PacketTooLarge
        | Error::InvalidAck
        | Error::UnterminatedField
        | Error::EmptyField
        | Error::SeekPastEnd
        | Error::BufferTooSmall
        | Error::InvalidTransferRange
        | Error::InvalidErrorPacket
        | Error::SourceRead => ERROR_ILLEGAL_OPERATION,
    }
}

impl ReadPlan {
    /// Bind a parsed request to a trusted file length. `transfer_size` is
    /// clipped to EOF and to an optional `rsize`, matching read-only semantics.
    pub fn new(request: ReadRequest, file_size: u64) -> Result<Self, Error> {
        Self::for_options(request.options, file_size)
    }

    pub(crate) fn for_options(options: Options, file_size: u64) -> Result<Self, Error> {
        if options.seek > file_size {
            return Err(Error::SeekPastEnd);
        }
        let available = file_size - options.seek;
        let transfer_size = options
            .read_size
            .map_or(available, |limit| available.min(u64::from(limit)));
        Ok(Self {
            file_size,
            start_offset: options.seek,
            transfer_size,
            block_size: options.block_size.min(MAX_BLOCK_SIZE),
            window_size: options.window_size,
        })
    }
}

struct PacketWriter<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl<'a> PacketWriter<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Self { out, len: 0 }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let end = self
            .len
            .checked_add(bytes.len())
            .ok_or(Error::BufferTooSmall)?;
        let target = self
            .out
            .get_mut(self.len..end)
            .ok_or(Error::BufferTooSmall)?;
        target.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    fn cstring(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.push(bytes)?;
        self.push(&[0])
    }

    fn number(&mut self, value: u64) -> Result<(), Error> {
        let mut digits = [0u8; 20];
        let mut value = value;
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.cstring(&digits[at..])
    }
}

/// Encode an OACK containing only supported options that were requested.
pub fn encode_oack(request: ReadRequest, plan: ReadPlan, out: &mut [u8]) -> Result<usize, Error> {
    encode_options(request.options, plan, out)
}

pub(crate) fn encode_options(options: Options, plan: ReadPlan, out: &mut [u8]) -> Result<usize, Error> {
    if !options.has_supported_options() {
        return Err(Error::MalformedOptions);
    }
    let mut writer = PacketWriter::new(out);
    writer.push(&OP_OACK.to_be_bytes())?;
    let offered = options.offered;
    for (bit, name, value) in [
        (OPT_BLKSIZE, &b"blksize"[..], u64::from(plan.block_size)),
        (
            OPT_TIMEOUTMS,
            &b"timeoutms"[..],
            u64::from(options.timeout_ms),
        ),
        (OPT_TSIZE, &b"tsize"[..], plan.file_size),
        (
            OPT_RSIZE,
            &b"rsize"[..],
            u64::from(options.read_size.unwrap_or(0)),
        ),
        (OPT_WSIZE, &b"wsize"[..], u64::from(plan.window_size)),
        (OPT_SEEK, &b"seek"[..], options.seek),
    ] {
        if offered & bit != 0 {
            writer.cstring(name)?;
            writer.number(value)?;
        }
    }
    Ok(writer.len)
}

pub fn encode_data(block: u16, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    if payload.len() > usize::from(MAX_BLOCK_SIZE) {
        return Err(Error::PacketTooLarge);
    }
    let len = 4usize
        .checked_add(payload.len())
        .ok_or(Error::BufferTooSmall)?;
    let packet = out.get_mut(..len).ok_or(Error::BufferTooSmall)?;
    packet[..2].copy_from_slice(&OP_DATA.to_be_bytes());
    packet[2..4].copy_from_slice(&block.to_be_bytes());
    packet[4..].copy_from_slice(payload);
    Ok(len)
}

pub fn encode_error(code: u16, message: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    if message.contains(&0) {
        return Err(Error::EmptyField);
    }
    let len = 5usize
        .checked_add(message.len())
        .ok_or(Error::BufferTooSmall)?;
    let packet = out.get_mut(..len).ok_or(Error::BufferTooSmall)?;
    packet[..2].copy_from_slice(&OP_ERROR.to_be_bytes());
    packet[2..4].copy_from_slice(&code.to_be_bytes());
    packet[4..4 + message.len()].copy_from_slice(message);
    packet[len - 1] = 0;
    Ok(len)
}
