//! Certificate-validated, bounded HTTPS transport for a guest TCP adapter.
//!
//! This standalone adapter is not yet connected to the wallet. A bare-metal
//! binary must install a getrandom custom backend backed by the entropy PD.
#![no_std]
extern crate alloc;

pub mod binary_response;

use alloc::{string::ToString, sync::Arc, vec, vec::Vec};
use core::{fmt::Debug, time::Duration};
use rustls::{
    client::UnbufferedClientConnection,
    pki_types::{ServerName, UnixTime},
    unbuffered::{ConnectionState, UnbufferedStatus},
    ClientConfig, RootCertStore,
};

/// A connected byte stream. `read` returning zero means EOF, never "pending".
pub trait Transport {
    type Error;
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Nonblocking connected stream. `None` means the operation would block;
/// `Some(0)` from `read` means EOF. A write must never report more than the
/// provided slice and may accept fewer bytes.
pub trait PollTransport {
    type Error;
    fn try_write(&mut self, bytes: &[u8]) -> Result<Option<usize>, Self::Error>;
    fn try_read(&mut self, bytes: &mut [u8]) -> Result<Option<usize>, Self::Error>;
}

#[derive(Debug)]
pub enum InitError {
    InvalidName,
    InvalidClock,
    RequestTooLarge,
    Configuration,
    Tls(rustls::Error),
}

pub enum Progress {
    Pending,
    Complete(Vec<u8>),
}

/// Receives newly decrypted records without retaining a second response buffer.
pub trait PlaintextSink {
    fn append(&mut self, bytes: &[u8]);
    fn complete(&self) -> bool;
}

/// An incrementally driven TLS connection. Each poll performs
/// at most one TCP operation, allowing the guest scheduler to run between
/// network events. Certificate verification is completed before application
/// bytes can be sent.
pub struct Session {
    client: UnbufferedClientConnection,
    incoming: Vec<u8>,
    incoming_used: usize,
    outgoing: Vec<u8>,
    outgoing_used: usize,
    outgoing_sent: usize,
    request: Vec<u8>,
    response: Vec<u8>,
    max_response: usize,
    sent: bool,
    finished: bool,
    wire_progress: u64,
}

impl Session {
    pub fn new(
        server_name: &str,
        unix_seconds: u64,
        request: &[u8],
        max_response: usize,
    ) -> Result<Self, InitError> {
        if unix_seconds == 0 {
            return Err(InitError::InvalidClock);
        }
        if request.len() > 16 * 1024 {
            return Err(InitError::RequestTooLarge);
        }
        let name =
            ServerName::try_from(server_name.to_string()).map_err(|_| InitError::InvalidName)?;
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let config = ClientConfig::builder_with_details(
            Arc::new(rustls::crypto::ring::default_provider()),
            Arc::new(ValidationTime(unix_seconds)),
        )
        .with_safe_default_protocol_versions()
        .map_err(|_| InitError::Configuration)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client =
            UnbufferedClientConnection::new(Arc::new(config), name).map_err(InitError::Tls)?;
        Ok(Self {
            client,
            incoming: vec![0; 64 * 1024],
            incoming_used: 0,
            outgoing: vec![0; 64 * 1024],
            outgoing_used: 0,
            outgoing_sent: 0,
            request: request.into(),
            response: Vec::new(),
            max_response,
            sent: false,
            finished: false,
            wire_progress: 0,
        })
    }

    /// Total ciphertext bytes transferred on this connection.
    pub fn wire_progress(&self) -> u64 { self.wire_progress }

    /// Whether the transport has accepted the entire encrypted application request.
    pub fn request_flushed(&self) -> bool {
        self.sent && self.outgoing_sent == self.outgoing_used
    }

    /// Begin a sequential request only at a clean, fully consumed response
    /// boundary. The HTTP caller also checks persistence and the peer identity.
    pub fn start_next_request(&mut self, request: &[u8]) -> bool {
        if !self.finished || !self.sent || self.incoming_used != 0
            || self.outgoing_sent != self.outgoing_used || request.len() > 16 * 1024 {
            return false;
        }
        self.request.clear();
        self.request.extend_from_slice(request);
        self.sent = false;
        self.finished = false;
        true
    }

    pub fn poll<T: PollTransport>(
        &mut self,
        stream: &mut T,
        complete: impl FnMut(&[u8]) -> bool,
    ) -> Result<Progress, Error<T::Error>> {
        self.poll_inner(stream, complete, None, true)
    }

    /// Drive the same authenticated TLS session with an incremental response sink.
    /// The sink owns the plaintext limit and is always called with new records only.
    pub fn poll_stream<T: PollTransport>(
        &mut self, stream: &mut T, sink: &mut impl PlaintextSink,
    ) -> Result<bool, Error<T::Error>> {
        self.poll_inner(stream, |_| false, Some(sink), true)
            .map(|progress| matches!(progress, Progress::Complete(_)))
    }

    /// Drive just the outgoing half of a reused connection. The caller can
    /// leave the response in TCP until it has storage available. Returns true
    /// once the complete encrypted request has been accepted by the transport.
    pub fn poll_request_write<T: PollTransport>(&mut self, stream: &mut T) -> Result<bool, Error<T::Error>> {
        if !self.request_flushed() {
            self.poll_inner(stream, |_| false, None, false)?;
        }
        Ok(self.request_flushed())
    }

    fn poll_inner<T: PollTransport>(
        &mut self, stream: &mut T, mut complete: impl FnMut(&[u8]) -> bool,
        mut sink: Option<&mut dyn PlaintextSink>, allow_read: bool,
    ) -> Result<Progress, Error<T::Error>> {
        if self.finished {
            return Err(Error::UnexpectedState);
        }
        // Ciphertext must be sent exactly once, even across partial writes.
        if self.outgoing_sent < self.outgoing_used {
            let pending = &self.outgoing[self.outgoing_sent..self.outgoing_used];
            match stream.try_write(pending).map_err(Error::Io)? {
                None => return Ok(Progress::Pending),
                Some(0) => return Err(Error::PrematureClose),
                Some(n) if n > pending.len() => return Err(Error::BufferLimit),
                Some(n) => {
                    self.outgoing_sent += n;
                    self.wire_progress = self.wire_progress.saturating_add(n as u64);
                }
            }
            return Ok(Progress::Pending);
        }
        for _ in 0..32 {
            let UnbufferedStatus { mut discard, state } = self
                .client
                .process_tls_records(&mut self.incoming[..self.incoming_used]);
            let mut needs_read = false;
            let mut complete_response = false;
            match state.map_err(Error::Tls)? {
                ConnectionState::EncodeTlsData(mut state) => {
                    if self.outgoing_sent == self.outgoing_used {
                        self.outgoing_sent = 0;
                        self.outgoing_used = 0;
                    }
                    let n = state
                        .encode(&mut self.outgoing[self.outgoing_used..])
                        .map_err(|_| Error::BufferLimit)?;
                    self.outgoing_used += n;
                }
                ConnectionState::TransmitTlsData(state) => {
                    // The encoded handshake bytes are flushed at the start of
                    // the next poll, before advancing this rustls state.
                    if self.outgoing_sent != self.outgoing_used {
                        return Ok(Progress::Pending);
                    }
                    self.outgoing_sent = 0;
                    self.outgoing_used = 0;
                    state.done();
                }
                ConnectionState::BlockedHandshake => needs_read = true,
                ConnectionState::WriteTraffic(mut state) => {
                    if !self.sent {
                        self.outgoing_sent = 0;
                        self.outgoing_used = state
                            .encrypt(&self.request, &mut self.outgoing)
                            .map_err(|_| Error::BufferLimit)?;
                        self.sent = true;
                    } else {
                        needs_read = true;
                    }
                }
                ConnectionState::ReadTraffic(mut state) => {
                    while let Some(record) = state.next_record() {
                        let record = record.map_err(Error::Tls)?;
                        if !self.sent && !record.payload.is_empty() {
                            return Err(Error::UnexpectedState);
                        }
                        discard = discard
                            .checked_add(record.discard)
                            .ok_or(Error::BufferLimit)?;
                        if let Some(receiver) = sink.as_deref_mut() {
                            receiver.append(record.payload);
                        } else {
                            if record.payload.len() > self.max_response.saturating_sub(self.response.len()) {
                                return Err(Error::BufferLimit);
                            }
                            self.response.extend_from_slice(record.payload);
                        }
                    }
                    complete_response = sink.as_ref().map_or_else(|| complete(&self.response), |receiver| receiver.complete());
                }
                ConnectionState::PeerClosed | ConnectionState::Closed => {
                    return Err(Error::PrematureClose)
                }
                _ => return Err(Error::UnexpectedState),
            }
            if discard > self.incoming_used {
                return Err(Error::UnexpectedState);
            }
            self.incoming.copy_within(discard..self.incoming_used, 0);
            self.incoming_used -= discard;
            if complete_response {
                self.finished = true;
                return Ok(Progress::Complete(core::mem::take(&mut self.response)));
            }
            if self.outgoing_sent < self.outgoing_used {
                return Ok(Progress::Pending);
            }
            if needs_read {
                if !allow_read {
                    return Ok(Progress::Pending);
                }
                if self.incoming_used == self.incoming.len() {
                    return Err(Error::BufferLimit);
                }
                let available = &mut self.incoming[self.incoming_used..];
                match stream.try_read(available).map_err(Error::Io)? {
                    None => return Ok(Progress::Pending),
                    Some(0) => return Err(Error::PrematureClose),
                    Some(n) if n > available.len() => return Err(Error::BufferLimit),
                    Some(n) => {
                        self.incoming_used += n;
                        self.wire_progress = self.wire_progress.saturating_add(n as u64);
                    }
                }
                return Ok(Progress::Pending);
            }
        }
        Ok(Progress::Pending)
    }
}

#[derive(Debug)]
struct ValidationTime(u64);
impl rustls::time_provider::TimeProvider for ValidationTime {
    fn current_time(&self) -> Option<UnixTime> {
        Some(UnixTime::since_unix_epoch(Duration::from_secs(self.0)))
    }
}

#[derive(Debug)]
pub enum Error<E> {
    InvalidName,
    InvalidClock,
    Configuration,
    Tls(rustls::Error),
    Io(E),
    BufferLimit,
    PrematureClose,
    UnexpectedState,
    IterationLimit,
}

/// Errors returned by the guest TCP service when polling ciphertext.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    Unavailable,
    InvalidReply,
}

/// Fixed diagnostic categories. They contain no peer-provided strings or bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    PeerClosed,
    Transport,
    Certificate,
    Protocol,
    Clock,
    Configuration,
    Limit,
}

impl FailureKind {
    pub fn transient(self) -> bool {
        matches!(self, Self::PeerClosed | Self::Transport)
    }
}

fn tls_failure(error: &rustls::Error) -> FailureKind {
    match error {
        rustls::Error::InvalidCertificate(_)
        | rustls::Error::InvalidCertRevocationList(_)
        | rustls::Error::NoCertificatesPresented => FailureKind::Certificate,
        rustls::Error::FailedToGetCurrentTime => FailureKind::Clock,
        _ => FailureKind::Protocol,
    }
}

impl InitError {
    pub fn kind(&self) -> FailureKind {
        match self {
            Self::InvalidName | Self::Configuration => FailureKind::Configuration,
            Self::InvalidClock => FailureKind::Clock,
            Self::RequestTooLarge => FailureKind::Limit,
            Self::Tls(error) => tls_failure(error),
        }
    }
}

impl Error<StreamError> {
    pub fn kind(&self) -> FailureKind {
        match self {
            Self::PrematureClose => FailureKind::PeerClosed,
            Self::Io(StreamError::Unavailable) => FailureKind::Transport,
            Self::Io(StreamError::InvalidReply) | Self::UnexpectedState => FailureKind::Protocol,
            Self::Tls(error) => tls_failure(error),
            Self::InvalidClock => FailureKind::Clock,
            Self::Configuration | Self::InvalidName => FailureKind::Configuration,
            Self::BufferLimit | Self::IterationLimit => FailureKind::Limit,
        }
    }
}

/// Perform one request using public WebPKI roots and hostname validation.
/// `complete` must only return true after the entire framed response arrives.
/// The caller supplies a trustworthy Unix time and must bound transport waits.
pub fn request<T: Transport>(
    stream: &mut T,
    server_name: &str,
    unix_seconds: u64,
    request: &[u8],
    max_response: usize,
    complete: impl Fn(&[u8]) -> bool,
) -> Result<Vec<u8>, Error<T::Error>> {
    if unix_seconds == 0 {
        return Err(Error::InvalidClock);
    }
    let name = ServerName::try_from(server_name.to_string()).map_err(|_| Error::InvalidName)?;
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = ClientConfig::builder_with_details(
        Arc::new(rustls::crypto::ring::default_provider()),
        Arc::new(ValidationTime(unix_seconds)),
    )
    .with_safe_default_protocol_versions()
    .map_err(|_| Error::Configuration)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut client = UnbufferedClientConnection::new(Arc::new(config), name).map_err(Error::Tls)?;
    let mut incoming = vec![0u8; 64 * 1024];
    let mut outgoing = vec![0u8; 64 * 1024];
    let mut incoming_used = 0usize;
    let mut outgoing_used = 0usize;
    let mut response = Vec::new();
    let mut sent = false;
    for _ in 0..100_000 {
        let UnbufferedStatus { mut discard, state } =
            client.process_tls_records(&mut incoming[..incoming_used]);
        match state.map_err(Error::Tls)? {
            ConnectionState::EncodeTlsData(mut state) => {
                let n = state
                    .encode(&mut outgoing[outgoing_used..])
                    .map_err(|_| Error::BufferLimit)?;
                outgoing_used += n;
            }
            ConnectionState::TransmitTlsData(state) => {
                stream
                    .write_all(&outgoing[..outgoing_used])
                    .map_err(Error::Io)?;
                outgoing_used = 0;
                state.done();
            }
            ConnectionState::BlockedHandshake => {
                if incoming_used == incoming.len() {
                    return Err(Error::BufferLimit);
                }
                let n = stream
                    .read(&mut incoming[incoming_used..])
                    .map_err(Error::Io)?;
                if n == 0 {
                    return Err(Error::PrematureClose);
                }
                if n > incoming.len() - incoming_used {
                    return Err(Error::BufferLimit);
                }
                incoming_used += n;
            }
            ConnectionState::WriteTraffic(mut state) => {
                if !sent {
                    let n = state
                        .encrypt(request, &mut outgoing)
                        .map_err(|_| Error::BufferLimit)?;
                    stream.write_all(&outgoing[..n]).map_err(Error::Io)?;
                    sent = true;
                } else {
                    if incoming_used == incoming.len() {
                        return Err(Error::BufferLimit);
                    }
                    let n = stream
                        .read(&mut incoming[incoming_used..])
                        .map_err(Error::Io)?;
                    if n == 0 {
                        return Err(Error::PrematureClose);
                    }
                    if n > incoming.len() - incoming_used {
                        return Err(Error::BufferLimit);
                    }
                    incoming_used += n;
                }
            }
            ConnectionState::ReadTraffic(mut state) => {
                while let Some(record) = state.next_record() {
                    let record = record.map_err(Error::Tls)?;
                    discard = discard
                        .checked_add(record.discard)
                        .ok_or(Error::BufferLimit)?;
                    if record.payload.len() > max_response.saturating_sub(response.len()) {
                        return Err(Error::BufferLimit);
                    }
                    response.extend_from_slice(record.payload);
                }
                if complete(&response) {
                    return Ok(response);
                }
            }
            ConnectionState::PeerClosed | ConnectionState::Closed => {
                return Err(Error::PrematureClose)
            }
            _ => return Err(Error::UnexpectedState),
        }
        if discard > incoming_used {
            return Err(Error::UnexpectedState);
        }
        incoming.copy_within(discard..incoming_used, 0);
        incoming_used -= discard;
    }
    Err(Error::IterationLimit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_tls_retry_distinguishes_stream_failures_from_invalid_data() {
        use FailureKind::*;
        for (error, kind, retry) in [
            (Error::PrematureClose, PeerClosed, true),
            (Error::Io(StreamError::Unavailable), Transport, true),
            (Error::Io(StreamError::InvalidReply), Protocol, false),
            (Error::Tls(rustls::Error::InvalidCertificate(rustls::CertificateError::Expired)), Certificate, false),
            (Error::Tls(rustls::Error::DecryptError), Protocol, false),
            (Error::BufferLimit, Limit, false),
            (Error::InvalidClock, Clock, false),
        ] {
            assert_eq!(error.kind(), kind);
            assert_eq!(error.kind().transient(), retry);
        }
    }

    #[test]
    fn peer_eof_and_read_error_after_client_hello_are_transient() {
        struct FailingStream(bool);
        impl PollTransport for FailingStream {
            type Error = StreamError;
            fn try_write(&mut self, bytes: &[u8]) -> Result<Option<usize>, StreamError> {
                Ok(Some(bytes.len()))
            }
            fn try_read(&mut self, _: &mut [u8]) -> Result<Option<usize>, StreamError> {
                if self.0 { Ok(Some(0)) } else { Err(StreamError::Unavailable) }
            }
        }
        for (closed, expected) in [(true, FailureKind::PeerClosed), (false, FailureKind::Transport)] {
            let mut session = Session::new("example.org", 1_700_000_000, b"request", 1024).unwrap();
            let mut stream = FailingStream(closed);
            let err = (0..16).find_map(|_| session.poll(&mut stream, |_| false).err()).unwrap();
            assert!(session.wire_progress() > 0);
            assert_eq!(err.kind(), expected);
            assert!(err.kind().transient());
        }
    }

    struct NoIo;
    impl Transport for NoIo {
        type Error = ();
        fn write_all(&mut self, _: &[u8]) -> Result<(), ()> {
            panic!("unexpected write")
        }
        fn read(&mut self, _: &mut [u8]) -> Result<usize, ()> {
            panic!("unexpected read")
        }
    }

    #[test]
    fn nonblocking_session_reports_wire_progress() {
        struct PartialWriter;
        impl PollTransport for PartialWriter {
            type Error = ();
            fn try_write(&mut self, _: &[u8]) -> Result<Option<usize>, ()> { Ok(Some(1)) }
            fn try_read(&mut self, _: &mut [u8]) -> Result<Option<usize>, ()> { Ok(None) }
        }
        let mut session = Session::new("rpc.nexawal.com", 1_700_000_000, b"GET / HTTP/1.0\r\n\r\n", 1024).unwrap();
        let mut stream = PartialWriter;
        assert_eq!(session.wire_progress(), 0);
        for _ in 0..16 {
            let _ = session.poll(&mut stream, |_| false).unwrap();
            if session.wire_progress() > 0 { break; }
        }
        assert!(session.wire_progress() > 0);
    }

    #[test]
    fn write_only_poll_flushes_ciphertext_once_across_blocked_and_partial_writes() {
        struct Writer { blocked: bool, bytes: Vec<u8> }
        impl PollTransport for Writer {
            type Error = ();
            fn try_write(&mut self, bytes: &[u8]) -> Result<Option<usize>, ()> {
                if self.blocked {
                    self.blocked = false;
                    return Ok(None);
                }
                self.bytes.push(bytes[0]);
                Ok(Some(1))
            }
            fn try_read(&mut self, _: &mut [u8]) -> Result<Option<usize>, ()> {
                panic!("request priming attempted to read a response")
            }
        }
        let mut session = Session::new("example.org", 1_700_000_000, b"request", 0).unwrap();
        // Exercise the state immediately after rustls encrypted the request.
        session.sent = true;
        session.outgoing[..2].copy_from_slice(b"ab");
        session.outgoing_used = 2;
        let mut stream = Writer { blocked: true, bytes: Vec::new() };
        for expected in [false, false, true, true] {
            assert_eq!(session.poll_request_write(&mut stream).unwrap(), expected);
        }
        assert_eq!(stream.bytes, b"ab");
        assert_eq!(session.wire_progress(), 2);
    }

    #[test]
    fn rejects_missing_clock_and_invalid_hostname_before_io() {
        assert!(matches!(
            Session::new("rpc.nexawal.com", 0, b"", 0),
            Err(InitError::InvalidClock)
        ));
        assert!(matches!(
            Session::new("bad/name", 1, b"", 0),
            Err(InitError::InvalidName)
        ));
        assert!(matches!(
            Session::new("rpc.nexawal.com", 1, &[0; 16 * 1024 + 1], 0),
            Err(InitError::RequestTooLarge)
        ));
        assert!(matches!(
            request(&mut NoIo, "rpc.nexawal.com", 0, b"", 0, |_| false),
            Err(Error::InvalidClock)
        ));
        assert!(matches!(
            request(&mut NoIo, "bad/name", 1, b"", 0, |_| false),
            Err(Error::InvalidName)
        ));
    }
}
