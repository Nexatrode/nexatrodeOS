// SPDX-License-Identifier: BSD-2-Clause
//! Browser-private bounded HTTP/1 framing, independent of wallet RPC policy.
extern crate alloc;
use alloc::{string::String, vec::Vec};
pub const HEADER_MAX: usize = 16384;
pub const BODY_MAX: usize = 2 * 1024 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Url,
    Headers,
    Framing,
    Limit,
    Encoding,
    Truncated,
}
#[derive(Debug)]
pub struct Url {
    pub host: String,
    pub authority: String,
    pub port: u16,
    pub target: String,
}
impl Url {
    pub fn parse(input: &str) -> Result<Self, Error> {
        let rest = input.strip_prefix("https://").ok_or(Error::Url)?;
        if input.len() > 2047 || input.bytes().any(|b| b <= 32 || b >= 127 || b == b'\\') {
            return Err(Error::Url);
        }
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let (host, port) = match authority.split_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().map_err(|_| Error::Url)?),
            None => (authority, 443),
        };
        if host.is_empty()
            || host.len() > 127
            || port == 0
            || host.split('.').any(|s| {
                s.is_empty()
                    || s.starts_with('-')
                    || s.ends_with('-')
                    || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(Error::Url);
        }
        let tail = rest[end..].split('#').next().unwrap_or("");
        let target = if tail.starts_with('/') {
            String::from(tail)
        } else {
            alloc::format!("/{tail}")
        };
        Ok(Self {
            host: String::from(host),
            authority: String::from(authority),
            port,
            target,
        })
    }
    pub fn request(&self) -> Vec<u8> {
        alloc::format!("GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: NetSurf-NexatrodeOS/0.1\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n", self.target, self.authority).into_bytes()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Headers,
    Fixed(usize),
    ChunkSize,
    Chunk(usize),
    ChunkCr,
    ChunkLf,
    Trailers,
    Close,
    Done,
}
pub struct Response {
    state: State,
    line: Vec<u8>,
    metadata: usize,
    pub status: u16,
    length: Option<usize>,
    chunked: bool,
    pub headers: Vec<u8>,
    pub body: Vec<u8>,
    pub error: Option<Error>,
}
impl Response {
    pub fn new() -> Self {
        Self {
            state: State::Headers,
            line: Vec::new(),
            metadata: 0,
            status: 0,
            length: None,
            chunked: false,
            headers: Vec::new(),
            body: Vec::new(),
            error: None,
        }
    }
    pub fn complete(&self) -> bool {
        self.state == State::Done || self.error.is_some()
    }
    /// Only authenticated TLS close_notify may terminate an unframed response.
    pub fn eof(&mut self, authenticated: bool) {
        if authenticated && self.state == State::Close {
            self.state = State::Done;
        } else if self.state != State::Done {
            self.error = Some(Error::Truncated);
        }
    }
    pub fn append(&mut self, data: &[u8]) {
        if self.error.is_none() {
            self.error = self.consume(data).err();
        }
    }
    fn consume(&mut self, mut data: &[u8]) -> Result<(), Error> {
        while !data.is_empty() {
            match self.state {
                State::Headers | State::ChunkSize | State::Trailers => {
                    let n = data
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(data.len(), |i| i + 1);
                    self.metadata = self.metadata.checked_add(n).ok_or(Error::Limit)?;
                    if self.metadata > HEADER_MAX || self.line.len() + n > 8192 {
                        return Err(Error::Limit);
                    }
                    self.line.extend_from_slice(&data[..n]);
                    data = &data[n..];
                    if self.line.last() != Some(&b'\n') {
                        continue;
                    }
                    if !self.line.ends_with(b"\r\n") {
                        return Err(Error::Framing);
                    }
                    let line = core::mem::take(&mut self.line);
                    self.line(&line[..line.len() - 2])?;
                }
                State::Fixed(left) | State::Chunk(left) => {
                    let chunk = matches!(self.state, State::Chunk(_));
                    let n = left.min(data.len());
                    self.store(&data[..n])?;
                    data = &data[n..];
                    self.state = if left > n {
                        if chunk {
                            State::Chunk(left - n)
                        } else {
                            State::Fixed(left - n)
                        }
                    } else if chunk {
                        State::ChunkCr
                    } else {
                        State::Done
                    };
                }
                State::ChunkCr => {
                    if data[0] != b'\r' {
                        return Err(Error::Framing);
                    }
                    data = &data[1..];
                    self.state = State::ChunkLf;
                }
                State::ChunkLf => {
                    if data[0] != b'\n' {
                        return Err(Error::Framing);
                    }
                    data = &data[1..];
                    self.state = State::ChunkSize;
                }
                State::Close => {
                    self.store(data)?;
                    return Ok(());
                }
                State::Done => return Err(Error::Framing),
            }
        }
        Ok(())
    }
    fn store(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.len() > BODY_MAX.saturating_sub(self.body.len()) {
            return Err(Error::Limit);
        }
        self.body
            .try_reserve(data.len())
            .map_err(|_| Error::Limit)?;
        self.body.extend_from_slice(data);
        Ok(())
    }
    fn line(&mut self, raw: &[u8]) -> Result<(), Error> {
        let text = core::str::from_utf8(raw).map_err(|_| Error::Headers)?;
        if self.state == State::ChunkSize {
            let size = text.split(';').next().ok_or(Error::Framing)?;
            if size.is_empty() || size.len() > 8 || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Framing);
            }
            let n = usize::from_str_radix(size, 16).map_err(|_| Error::Framing)?;
            if n > BODY_MAX.saturating_sub(self.body.len()) {
                return Err(Error::Limit);
            }
            self.state = if n == 0 {
                State::Trailers
            } else {
                State::Chunk(n)
            };
            return Ok(());
        }
        if self.state == State::Trailers {
            if raw.is_empty() {
                self.state = State::Done;
                return Ok(());
            }
            // Trailer fields cannot change routing, framing or content metadata.
            let (name, _) = text.split_once(':').ok_or(Error::Framing)?;
            if !valid_name(name)
                || [
                    "content-length",
                    "transfer-encoding",
                    "content-encoding",
                    "location",
                    "content-type",
                ]
                .iter()
                .any(|x| name.eq_ignore_ascii_case(x))
            {
                return Err(Error::Framing);
            }
            return Ok(());
        }
        if self.status == 0 {
            let mut p = text.splitn(3, ' ');
            if !matches!(p.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
                return Err(Error::Headers);
            }
            let code = p.next().ok_or(Error::Headers)?;
            if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::Headers);
            }
            self.status = code.parse().map_err(|_| Error::Headers)?;
            if !(100..=599).contains(&self.status) || self.status == 101 {
                return Err(Error::Headers);
            }
        } else if raw.is_empty() {
            if self.status < 200 {
                self.status = 0;
                self.headers.clear();
                self.length = None;
                self.chunked = false;
                return Ok(());
            }
            if self.chunked && self.length.is_some() {
                return Err(Error::Framing);
            }
            self.state = if matches!(self.status, 204 | 304) {
                State::Done
            } else if self.chunked {
                State::ChunkSize
            } else if let Some(n) = self.length {
                if n == 0 { State::Done } else { State::Fixed(n) }
            } else {
                State::Close
            };
        } else {
            let (name, value) = text.split_once(':').ok_or(Error::Headers)?;
            if !valid_name(name) || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127) {
                return Err(Error::Headers);
            }
            let value = value.trim_matches([' ', '\t']);
            if name.eq_ignore_ascii_case("content-length") {
                if self.length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(Error::Framing);
                }
                let n = value.parse().map_err(|_| Error::Limit)?;
                if n > BODY_MAX {
                    return Err(Error::Limit);
                }
                self.length = Some(n);
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                if self.chunked || !value.eq_ignore_ascii_case("chunked") {
                    return Err(Error::Framing);
                }
                self.chunked = true;
            } else if name.eq_ignore_ascii_case("content-encoding")
                && !value.eq_ignore_ascii_case("identity")
            {
                return Err(Error::Encoding);
            }
            // The frontend receives decoded bytes, so framing headers are replaced.
            if ![
                "content-length",
                "transfer-encoding",
                "connection",
                "content-encoding",
            ]
            .iter()
            .any(|x| name.eq_ignore_ascii_case(x))
            {
                self.headers.extend_from_slice(raw);
                self.headers.extend_from_slice(b"\r\n");
            }
        }
        Ok(())
    }
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls_and_injection() {
        let u = Url::parse("https://example.com:443/a?b=c#x").unwrap();
        assert_eq!(u.target, "/a?b=c");
        assert_eq!(u.port, 443);
        for u in [
            "http://example.com/",
            "https://user@example.com/",
            "https://a/\r\nX: yes",
            "https://a:0/",
            "https://a\\b/",
            "https://a..b/",
        ] {
            assert_eq!(Url::parse(u).unwrap_err(), Error::Url);
        }
        assert!(
            String::from_utf8(u.request())
                .unwrap()
                .contains("Accept-Encoding: identity\r\n")
        );
    }
    fn fragmented(wire: &[u8]) -> Response {
        let mut r = Response::new();
        for b in wire {
            r.append(core::slice::from_ref(b));
        }
        r
    }
    #[test]
    fn fixed_fragmented() {
        let r = fragmented(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5\r\n\r\nhello",
        );
        assert!(r.complete());
        assert!(r.error.is_none());
        assert_eq!(r.body, b"hello");
    }
    #[test]
    fn chunks_and_trailers() {
        let r=fragmented(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;x=y\r\nabc\r\n2\r\nde\r\n0\r\nX-End: yes\r\n\r\n");
        assert!(r.complete());
        assert!(r.error.is_none());
        assert_eq!(r.body, b"abcde");
    }
    #[test]
    fn redirect_and_interim() {
        let r=fragmented(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 302 Found\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(r.status, 302);
        assert!(r.complete());
        assert!(r.headers.starts_with(b"Location:"));
    }
    #[test]
    fn rejects_bad_framing() {
        for wire in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\nx",
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\nx",
            b"HTTP/1.1 200 OK\r\nContent-Length: 99999999\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\naZ",
        ] {
            assert!(fragmented(wire).error.is_some());
        }
    }
    #[test]
    fn close_requires_authentication_and_fixed_is_never_truncated() {
        let mut r = fragmented(b"HTTP/1.0 200 OK\r\n\r\nbody");
        r.eof(false);
        assert_eq!(r.error, Some(Error::Truncated));
        let mut r = fragmented(b"HTTP/1.0 200 OK\r\n\r\nbody");
        r.eof(true);
        assert!(r.complete());
        assert!(r.error.is_none());
        let mut r = fragmented(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\nbody");
        r.eof(true);
        assert_eq!(r.error, Some(Error::Truncated));
    }
    #[test]
    fn metadata_limit() {
        let mut r = Response::new();
        r.append(b"HTTP/1.1 200 OK\r\n");
        r.append(&alloc::vec![b'x';HEADER_MAX]);
        assert_eq!(r.error, Some(Error::Limit));
    }
}
