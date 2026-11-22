//! Incremental HTTP/1 response framing for a certificate-validated binary RPC.
use crate::{FailureKind, PlaintextSink};
use alloc::vec::Vec;

const HEADER_LIMIT: usize = 8 * 1024;
const CHUNK_LINE_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeError {
    Connect,
    Timeout,
    Truncated,
    TooLarge,
    Framing,
    Http(u16),
    Tls(FailureKind),
    Inactive,
}

impl RangeError {
    pub fn transient(self) -> bool {
        matches!(
            self,
            Self::Connect
                | Self::Timeout
                | Self::Truncated
                | Self::Http(408 | 429 | 500 | 502 | 503 | 504)
        ) || matches!(self, Self::Tls(kind) if kind.transient())
    }
    pub fn rate_limited(self) -> bool {
        self == Self::Http(429)
    }
    pub fn too_large(self) -> bool {
        matches!(self, Self::TooLarge | Self::Http(413))
    }
    pub fn unsupported(self) -> bool {
        matches!(self, Self::Http(404 | 405 | 501))
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum State {
    Headers,
    Fixed(usize),
    ChunkSize,
    Chunk(usize),
    ChunkCr,
    ChunkLf,
    Trailers,
    CloseDelimited,
    Done,
}

/// Owns the append-only bytes accepted by the HTTP framer. A specialized sink
/// can validate prefixes in place while the response is still arriving.
pub trait BodyStorage {
    fn body_len(&self) -> usize;
    fn reserve(&mut self, additional: usize, limit: usize) -> Result<(), RangeError>;
    fn append_bytes(&mut self, data: &[u8]) -> Result<(), RangeError>;
}

impl BodyStorage for Vec<u8> {
    fn body_len(&self) -> usize {
        self.len()
    }

    fn reserve(&mut self, additional: usize, limit: usize) -> Result<(), RangeError> {
        let required = self
            .len()
            .checked_add(additional)
            .filter(|&n| n <= limit)
            .ok_or(RangeError::TooLarge)?;
        if required > self.capacity() {
            let target = required.max(self.capacity().saturating_mul(2)).min(limit);
            self.try_reserve_exact(target - self.len())
                .map_err(|_| RangeError::TooLarge)?;
        }
        Ok(())
    }

    fn append_bytes(&mut self, data: &[u8]) -> Result<(), RangeError> {
        self.extend_from_slice(data);
        Ok(())
    }
}

pub struct BinaryResponse<B = Vec<u8>> {
    state: State,
    line: Vec<u8>,
    metadata_bytes: usize,
    status: Option<u16>,
    persistent: bool,
    content_length: Option<usize>,
    chunked: bool,
    body: B,
    limit: usize,
    error: Option<RangeError>,
}

/// Checks the growing buffer returned by Session::poll, feeding each byte to
/// the HTTP framer once. A close-delimited JSON response keeps the wallet's
/// existing completion rule and cannot supply the next request's connection.
pub struct BufferedRpcResponse {
    framed: BinaryResponse,
    consumed: usize,
}

impl BufferedRpcResponse {
    pub fn new(limit: usize) -> Self {
        Self { framed: BinaryResponse::new(limit), consumed: 0 }
    }

    pub fn complete(&mut self, response: &[u8], legacy_complete: impl FnOnce(&[u8]) -> bool) -> bool {
        self.framed.append(&response[self.consumed..]);
        self.consumed = response.len();
        if self.close_delimited() { legacy_complete(response) } else { self.framed.complete() }
    }

    pub fn close_delimited(&self) -> bool { self.framed.state == State::CloseDelimited }

    pub fn finish(self) -> Result<(Vec<u8>, bool), RangeError> {
        let persistent = self.framed.can_reuse();
        self.framed.finish().map(|body| (body, persistent))
    }
}

impl BinaryResponse {
    pub fn new(limit: usize) -> Self {
        Self::with_body(limit, Vec::new())
    }
}

impl<B: BodyStorage> BinaryResponse<B> {
    pub fn with_body(limit: usize, body: B) -> Self {
        Self {
            state: State::Headers,
            line: Vec::new(),
            metadata_bytes: 0,
            status: None,
            persistent: false,
            content_length: None,
            chunked: false,
            body,
            limit,
            error: None,
        }
    }

    /// Bytes of authenticated application body accepted so far, excluding framing.
    pub fn body_progress(&self) -> usize {
        self.body.body_len()
    }

    pub fn body_mut(&mut self) -> &mut B {
        &mut self.body
    }

    pub fn can_reuse(&self) -> bool {
        self.persistent && self.state == State::Done && self.error.is_none()
    }

    pub fn finish(self) -> Result<B, RangeError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.state != State::Done {
            return Err(RangeError::Truncated);
        }
        Ok(self.body)
    }

    fn consume(&mut self, mut data: &[u8]) -> Result<(), RangeError> {
        while !data.is_empty() {
            match self.state {
                State::Headers | State::Trailers | State::ChunkSize => {
                    let count = data
                        .iter()
                        .position(|b| *b == b'\n')
                        .map_or(data.len(), |i| i + 1);
                    let (part, rest) = data.split_at(count);
                    data = rest;
                    if self.state == State::ChunkSize {
                        if self.line.len() + count > CHUNK_LINE_LIMIT {
                            return Err(RangeError::Framing);
                        }
                    } else {
                        self.metadata_bytes += count;
                        if self.metadata_bytes > HEADER_LIMIT {
                            return Err(RangeError::Framing);
                        }
                    }
                    self.line.extend_from_slice(part);
                    if part.last() == Some(&b'\n') {
                        let mut line = core::mem::take(&mut self.line);
                        let content = line.strip_suffix(b"\r\n").ok_or(RangeError::Framing)?;
                        self.parse_line(content)?;
                        line.clear();
                        self.line = line;
                    }
                }
                State::Fixed(left) | State::Chunk(left) => {
                    let count = data.len().min(left);
                    self.body.append_bytes(&data[..count])?;
                    data = &data[count..];
                    self.state = match (self.state, left - count) {
                        (State::Fixed(_), 0) => State::Done,
                        (State::Fixed(_), n) => State::Fixed(n),
                        (_, 0) => State::ChunkCr,
                        (_, n) => State::Chunk(n),
                    };
                }
                State::ChunkCr | State::ChunkLf => {
                    let cr = self.state == State::ChunkCr;
                    if data[0] != if cr { b'\r' } else { b'\n' } {
                        return Err(RangeError::Framing);
                    }
                    data = &data[1..];
                    self.state = if cr { State::ChunkLf } else { State::ChunkSize };
                }
                State::CloseDelimited | State::Done => return Err(RangeError::Framing),
            }
        }
        Ok(())
    }

    fn parse_line(&mut self, line: &[u8]) -> Result<(), RangeError> {
        let line = core::str::from_utf8(line).map_err(|_| RangeError::Framing)?;
        if line.bytes().any(|b| (b < 32 && b != b'\t') || b == 127) {
            return Err(RangeError::Framing);
        }
        if self.state == State::ChunkSize {
            let digits = line.split(';').next().ok_or(RangeError::Framing)?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(RangeError::Framing);
            }
            let size = usize::from_str_radix(digits, 16).map_err(|_| RangeError::TooLarge)?;
            if size == 0 {
                self.state = State::Trailers;
                self.metadata_bytes = 0;
            } else {
                self.reserve_body(size)?;
                self.state = State::Chunk(size);
            }
        } else if self.status.is_none() {
            let mut parts = line.splitn(3, ' ');
            self.persistent = match parts.next() {
                Some("HTTP/1.1") => true,
                Some("HTTP/1.0") => false,
                _ => return Err(RangeError::Framing),
            };
            let code = parts.next().ok_or(RangeError::Framing)?;
            if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(RangeError::Framing);
            }
            self.status = Some(code.parse().map_err(|_| RangeError::Framing)?);
            if !matches!(self.status, Some(100..=599)) {
                return Err(RangeError::Framing);
            }
        } else if line.is_empty() {
            if self.state == State::Trailers {
                self.state = State::Done;
            } else {
                let code = self.status.ok_or(RangeError::Framing)?;
                if (100..200).contains(&code) && code != 101 {
                    self.status = None;
                    return Ok(());
                }
                if code != 200 {
                    return Err(RangeError::Http(code));
                }
                self.state = match (self.chunked, self.content_length) {
                    (false, Some(0)) => State::Done,
                    (false, Some(n)) => {
                        self.reserve_body(n)?;
                        State::Fixed(n)
                    }
                    (true, None) => State::ChunkSize,
                    (false, None) => {
                        self.state = State::CloseDelimited;
                        return Err(RangeError::Framing);
                    }
                    _ => return Err(RangeError::Framing),
                };
            }
        } else {
            let (name, value) = line.split_once(':').ok_or(RangeError::Framing)?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            {
                return Err(RangeError::Framing);
            }
            if self.status != Some(200) {
                return Ok(());
            }
            let value = value.trim_matches([' ', '\t']);
            if name.eq_ignore_ascii_case("connection") {
                if value.split(',').any(|token| token.trim().eq_ignore_ascii_case("close")) {
                    self.persistent = false;
                }
            } else if name.eq_ignore_ascii_case("content-length") {
                if self.state == State::Trailers
                    || self.content_length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(RangeError::Framing);
                }
                self.content_length = Some(value.parse().map_err(|_| RangeError::TooLarge)?);
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                if self.state == State::Trailers
                    || self.chunked
                    || !value.eq_ignore_ascii_case("chunked")
                {
                    return Err(RangeError::Framing);
                }
                self.chunked = true;
            } else if name.eq_ignore_ascii_case("content-encoding")
                && (self.state == State::Trailers || !value.eq_ignore_ascii_case("identity"))
            {
                return Err(RangeError::Framing);
            }
        }
        Ok(())
    }

    fn reserve_body(&mut self, additional: usize) -> Result<(), RangeError> {
        self.body
            .body_len()
            .checked_add(additional)
            .filter(|&n| n <= self.limit)
            .ok_or(RangeError::TooLarge)?;
        self.body.reserve(additional, self.limit)
    }
}

impl<B: BodyStorage> PlaintextSink for BinaryResponse<B> {
    fn append(&mut self, data: &[u8]) {
        if self.error.is_none() {
            self.error = self.consume(data).err();
        }
    }

    fn complete(&self) -> bool {
        self.error.is_some() || self.state == State::Done
    }
}
