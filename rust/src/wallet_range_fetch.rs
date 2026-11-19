//! One authenticated binary range request on the wallet-owned TCP slot.
use super::wallet_tcp::WalletTcp;
use alloc::format;
use nexatrode_guest_https::{binary_response::BinaryResponse, FailureKind, Session};
use nexatrode_guest_https::binary_response::BodyStorage;
use walletcore_core::{NodeEndpoint, RangeBody};

struct WalletBody(RangeBody);

impl BodyStorage for WalletBody {
    fn body_len(&self) -> usize {
        self.0.len()
    }

    fn reserve(&mut self, additional: usize, _: usize) -> Result<(), RangeError> {
        self.0.reserve(additional).map_err(|_| RangeError::TooLarge)
    }

    fn append_bytes(&mut self, bytes: &[u8]) -> Result<(), RangeError> {
        self.0.append(bytes).map_err(|_| RangeError::TooLarge)
    }
}

pub use nexatrode_guest_https::binary_response::RangeError;

pub const MAX_RANGE_BYTES: usize = walletcore_core::MAX_RANGE_BODY;
const POLLS_PER_STEP: usize = 64;
const MAX_IDLE_MS: u64 = 20_000;
const MAX_ELAPSED_MS: u64 = 120_000;

pub(super) struct RangeConnection {
    session: Session,
    peer: (NodeEndpoint, [u8; 4]),
}

impl RangeConnection {
    pub(super) fn from_preflight(session: Session, node: NodeEndpoint, ip: [u8; 4]) -> Self {
        Self { session, peer: (node, ip) }
    }
}

struct Active {
    connecting: bool,
    connection: RangeConnection,
    initial_wire: u64,
    response: BinaryResponse<WalletBody>,
}

pub struct RangeFetch {
    pub start_height: u64,
    pub count: usize,
    started_ms: u64,
    last_progress_ms: u64,
    wire_bytes: u64,
    body_bytes: usize,
    #[cfg(feature = "wallet-range-profile")]
    phases: FetchPhases,
    pub(super) reused: bool,
    active: Option<Active>,
}

#[cfg(feature = "wallet-range-profile")]
#[derive(Default)]
struct FetchPhases {
    started: u64,
    connected: Option<u64>,
    request_flushed: Option<u64>,
    first_body: Option<u64>,
}

impl RangeFetch {
    /// The caller provides the encoded EPEE body and owns its requested range
    /// across retries. DNS resolution and the choice of node belong to the wallet.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        tcp: &mut WalletTcp,
        node: NodeEndpoint,
        ip: [u8; 4],
        start: u64,
        count: usize,
        encoded_request_body: &[u8],
        unix_seconds: Option<u64>,
        now_ms: u64,
        idle: &mut Option<RangeConnection>,
    ) -> Result<Self, RangeError> {
        if node.port != 443 {
            return Err(RangeError::Tls(FailureKind::Configuration));
        }
        if count == 0
            || start.checked_add(count as u64).is_none()
            || encoded_request_body.is_empty()
        {
            return Err(RangeError::Framing);
        }
        if encoded_request_body.len() > 1024 {
            return Err(RangeError::TooLarge);
        }
        #[cfg(feature = "wallet-range-profile")]
        let started = super::wallet_bulk::clock_us();
        let unix_seconds = unix_seconds.ok_or(RangeError::Tls(FailureKind::Clock))?;
        let mut request = format!(
            "POST /get_blocks.bin HTTP/1.1\r\nHost: {}\r\nContent-Type: application/octet-stream\r\nAccept-Encoding: identity\r\nContent-Length: {}\r\n\r\n",
            node.authority(), encoded_request_body.len(),
        ).into_bytes();
        request.extend_from_slice(encoded_request_body);
        // Streaming keeps plaintext only in BinaryResponse. Session owns the
        // bounded TLS record buffers and validates the configured host and time.
        let peer = (node, ip);
        let connection = idle.take().filter(|conn| conn.peer == peer)
            .and_then(|mut conn| conn.session.start_next_request(&request).then_some(conn));
        let reused = connection.is_some();
        let connection = match connection {
            Some(conn) => conn,
            None => {
                tcp.reset().map_err(|_| RangeError::Connect)?;
                let session = Session::new(node.host(), unix_seconds, &request, 0)
                    .map_err(|error| RangeError::Tls(error.kind()))?;
                if tcp.open_fresh(ip, node.port).is_err() {
                    let _ = tcp.reset();
                    return Err(RangeError::Connect);
                }
                RangeConnection { session, peer }
            }
        };
        let initial_wire = connection.session.wire_progress();
        Ok(Self {
            start_height: start,
            count,
            started_ms: now_ms,
            last_progress_ms: now_ms,
            wire_bytes: 0,
            body_bytes: 0,
            reused,
            #[cfg(feature = "wallet-range-profile")]
            phases: FetchPhases {
                started, connected: reused.then_some(started), ..Default::default()
            },
            active: Some(Active {
                connecting: !reused,
                connection,
                initial_wire,
                response: BinaryResponse::with_body(MAX_RANGE_BYTES, WalletBody(RangeBody::new(start, count))),
            }),
        })
    }

    /// Cumulative TCP ciphertext read or written by this fetch, including TLS handshake.
    pub fn wire_progress(&self) -> u64 {
        self.wire_bytes
    }

    /// Bytes of the next range body decrypted and framed so far.
    pub fn body_progress(&self) -> usize {
        self.body_bytes
    }

    pub fn partial_body(&mut self) -> Option<&mut RangeBody> {
        self.active.as_mut().map(|active| &mut active.response.body_mut().0)
    }

    /// Give a previously authenticated connection one nonblocking write turn.
    /// A primed range has no response body allocation until ordinary polling.
    pub fn prime_request(&mut self, tcp: &mut WalletTcp, now_ms: u64) -> Result<bool, RangeError> {
        if !self.reused { return Ok(false); }
        let active = self.active.as_mut().ok_or(RangeError::Inactive)?;
        let session = &mut active.connection.session;
        let flushed = session.poll_request_write(tcp).map_err(|error| RangeError::Tls(error.kind()))?;
        #[cfg(feature = "wallet-range-profile")]
        if flushed && self.phases.request_flushed.is_none() {
            self.phases.request_flushed = Some(super::wallet_bulk::clock_us());
        }
        let wire = session.wire_progress() - active.initial_wire;
        let advanced = wire != self.wire_bytes;
        if advanced {
            self.wire_bytes = wire;
            self.last_progress_ms = now_ms;
        }
        Ok(flushed && advanced)
    }

    /// At most one connect-status query and 64 TLS polls. Re-arm the wallet's
    /// short timer on Pending; an empty poll immediately yields to the caller.
    pub fn step(
        &mut self,
        tcp: &mut WalletTcp,
        now_ms: u64,
        idle: &mut Option<RangeConnection>,
    ) -> Result<Option<RangeBody>, RangeError> {
        let Some(active) = self.active.as_mut() else {
            return Err(RangeError::Inactive);
        };
        let finished = (|| {
            if now_ms < self.last_progress_ms {
                return Err(RangeError::Tls(FailureKind::Clock));
            }
            if now_ms - self.started_ms >= MAX_ELAPSED_MS
                || now_ms - self.last_progress_ms >= MAX_IDLE_MS
            {
                return Err(RangeError::Timeout);
            }
            if active.connecting {
                if !tcp.poll_connected().map_err(|_| RangeError::Connect)? {
                    return Ok(false);
                }
                active.connecting = false;
                #[cfg(feature = "wallet-range-profile")]
                { self.phases.connected = Some(super::wallet_bulk::clock_us()); }
                self.last_progress_ms = now_ms;
            }
            for _ in 0..POLLS_PER_STEP {
                let done = tcp.with_range_rx(|tcp| active.connection.session.poll_stream(tcp, &mut active.response))
                    .map_err(|error| RangeError::Tls(error.kind()))?;
                self.body_bytes = active.response.body_progress();
                #[cfg(feature = "wallet-range-profile")]
                {
                    if self.phases.request_flushed.is_none() && active.connection.session.request_flushed() {
                        self.phases.request_flushed = Some(super::wallet_bulk::clock_us());
                    }
                    if self.phases.first_body.is_none() && self.body_bytes != 0 {
                        self.phases.first_body = Some(super::wallet_bulk::clock_us());
                    }
                }
                let current = active.connection.session.wire_progress() - active.initial_wire;
                let progressed = current != self.wire_bytes;
                if progressed {
                    self.wire_bytes = current;
                    self.last_progress_ms = now_ms;
                }
                if done || !progressed {
                    return Ok(done);
                }
            }
            Ok(false)
        })();
        match finished {
            Ok(false) => Ok(None),
            Ok(true) => {
                #[cfg(feature = "wallet-range-profile")]
                if let (Some(connected), Some(flushed), Some(first)) =
                    (self.phases.connected, self.phases.request_flushed, self.phases.first_body)
                {
                    let received = super::wallet_bulk::clock_us();
                    sel4_microkit::debug_println!(
                        "UIAPP|WALLET|RANGE|ACQUIRE start={} tcp_us={} tls_send_us={} until_body_us={} stream_us={}",
                        self.start_height, connected - self.phases.started, flushed - connected,
                        first - flushed, received - first
                    );
                }
                let active = self.active.take().ok_or(RangeError::Inactive)?;
                let reuse = active.response.can_reuse();
                let body = active.response.finish();
                if reuse {
                    *idle = Some(active.connection);
                } else {
                    let release = tcp.release().map_err(|_| RangeError::Connect);
                    let body = body?;
                    release?;
                    return Ok(Some(body.0));
                }
                let body = body?;
                Ok(Some(body.0))
            }
            Err(error) => {
                self.cancel(tcp);
                Err(error)
            }
        }
    }

    /// Safe after completion; an inactive fetch cannot cancel a subsequent owner.
    pub fn cancel(&mut self, tcp: &mut WalletTcp) {
        if self.active.take().is_some() {
            let _ = tcp.reset();
        }
    }
}
