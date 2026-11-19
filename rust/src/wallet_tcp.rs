//! Shared dhcp_client haskell_tcp page client (wallet slot 2 / worker slot 3).
#![allow(dead_code)]

use core::ptr::NonNull;
use sel4_microkit::{Channel, MessageInfo, debug_println, memory_region_symbol};

const TCP_MAGIC: u32 = 0x3150_4354;
const TCP_MAX: usize = 128;
/// Wallet PD → dhcp channel id and session slot.
pub const WALLET_DHCP_CHANNEL: Channel = Channel::new(2);
pub const WALLET_TCP_SLOT: u32 = 2;
/// scan_worker PD → dhcp channel id and session slot.
pub const WORKER_DHCP_CHANNEL: Channel = Channel::new(1);
pub const WORKER_TCP_SLOT: u32 = 3;

const OPEN: u64 = 0x421;
const STATUS: u64 = 0x422;
const SEND: u64 = 0x423;
const RECV: u64 = 0x424;
const RECV_WIDE: u64 = 0x427;
const QUERY_WIDE: u64 = 0x428;
const WIDE_READY: u64 = 0x438;
const WIDE_OFFSET: usize = 2048;
const LEGACY_WIDE_LEN: usize = 1024;
const WIDE_LEN: usize = 2048;
const CLOSE: u64 = 0x425;
const CANCEL: u64 = 0x426;
const DNS_START: u64 = 0x440;
const DNS_POLL: u64 = 0x441;

const ACCEPTED: u64 = 0x430;
const PENDING: u64 = 0x431;
const CONNECTED: u64 = 0x432;
const DATA: u64 = 0x433;
const CLOSED: u64 = 0x434;
const BUSY: u64 = 0x435;
const ERROR: u64 = 0x436;
const NOT_READY: u64 = 0x437;

#[repr(C)]
struct TcpPage {
    magic: u32,
    request_len: u32,
    response_len: u32,
    reserved: u32,
    dest_ip: [u8; 4],
    dest_port: u16,
    reserved2: u16,
    request: [u8; TCP_MAX],
    response: [u8; TCP_MAX],
}

const _: () = assert!(core::mem::size_of::<TcpPage>() == 280);
const _: () = assert!(4 * core::mem::size_of::<TcpPage>() <= WIDE_OFFSET && WIDE_OFFSET + WIDE_LEN <= 4096);

pub struct WalletTcp {
    rx_capacity: Option<usize>,
    range_rx: bool,
    page: NonNull<TcpPage>,
    dhcp: Channel,
    slot: u32,
}

#[cfg(feature = "wallet-https")]
impl nexatrode_guest_https::PollTransport for WalletTcp {
    type Error = nexatrode_guest_https::StreamError;

    fn try_write(&mut self, bytes: &[u8]) -> Result<Option<usize>, Self::Error> {
        self.try_send_raw(bytes)
    }

    fn try_read(&mut self, bytes: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        self.try_recv_raw(bytes)
    }
}

impl WalletTcp {
    pub fn map() -> Self {
        Self::map_with(WALLET_DHCP_CHANNEL, WALLET_TCP_SLOT)
    }

    pub fn map_worker() -> Self {
        Self::map_with(WORKER_DHCP_CHANNEL, WORKER_TCP_SLOT)
    }

    fn map_with(dhcp: Channel, slot: u32) -> Self {
        // Per-slot wire page inside shared haskell_tcp_io (wallet=2, worker=3).
        let base = memory_region_symbol!(haskell_tcp_page: *mut TcpPage);
        let page = unsafe { NonNull::new_unchecked(base.as_ptr().add(slot as usize)) };
        unsafe {
            page.as_ptr().write(TcpPage {
                magic: TCP_MAGIC,
                request_len: 0,
                response_len: 0,
                reserved: slot,
                dest_ip: [0; 4],
                dest_port: 0,
                reserved2: 0,
                request: [0; TCP_MAX],
                response: [0; TCP_MAX],
            });
        }
        Self { page, dhcp, slot, rx_capacity: None, range_rx: false }
    }

    fn page_mut(&mut self) -> &mut TcpPage {
        unsafe { self.page.as_mut() }
    }

    fn stamp_slot(&mut self) {
        let slot = self.slot;
        let page = self.page_mut();
        page.magic = TCP_MAGIC;
        page.reserved = slot;
    }

    fn call(&self, label: u64) -> u64 {
        // microkit patches this bitmask into the PD image; bit N means channel N is a PP client.
        unsafe extern "C" {
            static microkit_pps: usize;
        }
        let pps = unsafe { core::ptr::addr_of!(microkit_pps).read_volatile() };
        // Keep verbose CALL_IN/OUT for open/send/recv; STATUS spam hides progress.
        // SEND/RECV are called for every 128-byte chunk. Logging each PPC
        // overwhelms the serial console during block scans and delays input.
        let verbose = !matches!(label, STATUS | SEND | RECV | RECV_WIDE);
        if verbose {
            debug_println!(
                "UIAPP|TCP|CALL_IN 0x{:x} ch={} slot={} pps={:#x} bit={}",
                label,
                self.dhcp.index(),
                self.slot,
                pps,
                (pps >> self.dhcp.index()) & 1
            );
        }
        let reply = self.dhcp.pp_call(MessageInfo::new(label, 0)).label();
        if verbose {
            debug_println!("UIAPP|TCP|CALL_OUT 0x{:x} -> 0x{:x}", label, reply);
        }
        reply
    }

    /// Agent NativeNetwork uses threadDelay 10ms between PENDING polls so dhcp
    /// can process RX/timer. Hammering STATUS via PPC starves notified() and
    /// aborts peer connects (seen on canary 1.1.1.1:80). Stay off-CPU long.
    pub fn pause_poll(&self) {
        // Longer off-CPU window so gpu_client (prio 1) can scan out between
        // TCP STATUS polls on 1-core TCG.
        for _ in 0..2_000 {
            sel4::r#yield();
        }
    }

    fn wait_not_pending(&mut self, label: u64) -> u64 {
        // Fewer STATUS PPCs, longer gaps — match agent poll cadence.
        for i in 0..600u32 {
            self.stamp_slot();
            let status = self.call(label);
            if status != PENDING && status != BUSY {
                return status;
            }
            if i % 25 == 0 {
                debug_println!("UIAPP|WALLET|TCP|WAIT 0x{:x} pending i={}", label, i);
            }
            self.pause_poll();
        }
        debug_println!("UIAPP|WALLET|TCP|WAIT_TIMEOUT 0x{:x}", label);
        ERROR
    }

    pub fn dns_lookup(&mut self, host: &str) -> Result<[u8; 4], ()> {
        let bytes = host.as_bytes();
        if bytes.is_empty() || bytes.len() >= TCP_MAX {
            return Err(());
        }
        {
            let slot = self.slot;
            let page = self.page_mut();
            page.magic = TCP_MAGIC;
            page.reserved = slot;
            page.request[..bytes.len()].copy_from_slice(bytes);
            page.request_len = bytes.len() as u32;
        }
        let start = self.call(DNS_START);
        if start != ACCEPTED && start != DATA {
            debug_println!("UIAPP|WALLET|DNS_START|{}", start);
            return Err(());
        }
        for _ in 0..600u32 {
            self.stamp_slot();
            let poll = self.call(DNS_POLL);
            if poll == DATA {
                let page = self.page_mut();
                if page.response_len < 4 {
                    return Err(());
                }
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&page.response[..4]);
                return Ok(ip);
            }
            if poll != PENDING && poll != BUSY {
                debug_println!("UIAPP|WALLET|DNS_POLL|{}", poll);
                return Err(());
            }
            self.pause_poll();
        }
        debug_println!("UIAPP|WALLET|DNS_POLL|timeout");
        Err(())
    }

    /// Single STATUS PPC used to prove the wallet→dhcp channel replies.
    pub fn call_status_probe(&mut self) -> u64 {
        self.stamp_slot();
        self.call(STATUS)
    }

    /// Issue OPEN only; caller polls `poll_connected` across notifies so dhcp
    /// can run its RX/timer path (agent-style, no STATUS spin).
    pub fn open(&mut self, ip: [u8; 4], port: u16) -> Result<(), ()> {
        let open = self.start_open(ip, port);
        // BUSY = already CONNECTING/CONNECTED on this slot. Do NOT CANCEL —
        // dump ethfix: a second OPEN aborted a live Cake TCP (slot=2
        // Connection aborted) and left WaitConnect spinning on ERROR.
        // Exception: PEER_CLOSED with unread rx also returns BUSY while STATUS
        // is CLOSED — WaitConnect then OPEN_BUSY forever (tip768 park). Clear
        // the stale session and retry OPEN once.
        if open == BUSY {
            self.stamp_slot();
            let st = self.call(STATUS);
            if st == PENDING || st == BUSY || st == CONNECTED || st == DATA {
                debug_println!("UIAPP|WALLET|TCP|OPEN_BUSY");
                return Ok(());
            }
            debug_println!("UIAPP|WALLET|TCP|OPEN_BUSY_STALE st={}", st);
            self.close();
            let open2 = self.start_open(ip, port);
            if open2 == BUSY {
                debug_println!("UIAPP|WALLET|TCP|OPEN_BUSY_AFTER_CLEAR");
                return Ok(());
            }
            if open2 != ACCEPTED && open2 != CONNECTED {
                debug_println!("UIAPP|WALLET|TCP_OPEN|{}", open2);
                return Err(());
            }
            debug_println!("UIAPP|WALLET|TCP|OPEN_OK_AFTER_CLEAR");
            return Ok(());
        }
        if open != ACCEPTED && open != CONNECTED {
            debug_println!("UIAPP|WALLET|TCP_OPEN|{}", open);
            return Err(());
        }
        debug_println!("UIAPP|WALLET|TCP|OPEN_OK");
        Ok(())
    }

    fn start_open(&mut self, ip: [u8; 4], port: u16) -> u64 {
        let slot = self.slot;
        let page = self.page_mut();
        page.magic = TCP_MAGIC;
        page.reserved = slot;
        page.dest_ip = ip;
        page.dest_port = port;
        page.request_len = 0;
        page.response_len = 0;
        self.call(OPEN)
    }

    /// A fresh TLS session may never adopt a prior connection reported as BUSY.
    pub fn open_fresh(&mut self, ip: [u8; 4], port: u16) -> Result<(), ()> {
        match self.start_open(ip, port) {
            ACCEPTED | CONNECTED => Ok(()),
            _ => Err(()),
        }
    }

    /// Detach the PCB and discard any remaining RX bytes on this slot.
    pub fn reset(&mut self) -> Result<(), ()> {
        self.stamp_slot();
        if self.call(CANCEL) == CLOSED { Ok(()) } else { Err(()) }
    }

    /// The daemon can return BUSY on graceful close while a PCB remains live.
    pub fn release(&mut self) -> Result<(), ()> {
        self.stamp_slot();
        if self.call(CLOSE) == CLOSED { Ok(()) } else { self.reset() }
    }

    /// One STATUS PPC. Ok(true)=connected, Ok(false)=still pending, Err=failed.
    pub fn poll_connected(&mut self) -> Result<bool, ()> {
        self.stamp_slot();
        let status = self.call(STATUS);
        if status == CONNECTED {
            debug_println!("UIAPP|WALLET|TCP|CONNECTED");
            return Ok(true);
        }
        if status == PENDING || status == BUSY {
            return Ok(false);
        }
        debug_println!("UIAPP|WALLET|TCP_STATUS|{}", status);
        Err(())
    }

    pub fn connect(&mut self, ip: [u8; 4], port: u16) -> Result<(), ()> {
        self.open(ip, port)?;
        let status = self.wait_not_pending(STATUS);
        if status != CONNECTED {
            debug_println!("UIAPP|WALLET|TCP_STATUS|{}", status);
            return Err(());
        }
        debug_println!("UIAPP|WALLET|TCP|CONNECTED");
        Ok(())
    }

    /// Attempt one raw TCP write without spinning. `None` means retry after
    /// the network service has had a chance to run. This preserves the 128-byte
    /// IPC boundary and is suitable for incremental TLS handshakes.
    #[cfg(feature = "wallet-https")]
    fn try_send_raw(&mut self, bytes: &[u8]) -> Result<Option<usize>, nexatrode_guest_https::StreamError> {
        if bytes.is_empty() { return Ok(Some(0)); }
        let count = bytes.len().min(TCP_MAX);
        {
            let slot = self.slot;
            let page = self.page_mut();
            page.magic = TCP_MAGIC;
            page.reserved = slot;
            page.request[..count].copy_from_slice(&bytes[..count]);
            page.request_len = count as u32;
        }
        self.stamp_slot();
        match self.call(SEND) {
            ACCEPTED => Ok(Some(count)),
            CLOSED => Ok(Some(0)),
            BUSY | PENDING => Ok(None),
            ERROR | NOT_READY => Err(nexatrode_guest_https::StreamError::Unavailable),
            _ => Err(nexatrode_guest_https::StreamError::InvalidReply),
        }
    }

    /// An empty wide read may arm a one-shot RX notification for the range
    /// coordinator. Ordinary RPCs and legacy services keep their timer path.
    #[cfg(feature = "wallet-https")]
    pub(super) fn with_range_rx<T>(&mut self, poll: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.range_rx;
        self.range_rx = !cfg!(feature = "wallet-handoff-prime");
        let result = poll(self);
        self.range_rx = previous;
        result
    }

    /// Attempt one raw TCP read without interpreting ciphertext as HTTP.
    /// `Some(0)` is an orderly peer close; `None` means no bytes are ready.
    #[cfg(feature = "wallet-https")]
    fn try_recv_raw(&mut self, out: &mut [u8]) -> Result<Option<usize>, nexatrode_guest_https::StreamError> {
        if out.is_empty() { return Err(nexatrode_guest_https::StreamError::InvalidReply); }
        let limit = if cfg!(feature = "wallet-wide-rx") && self.slot == WALLET_TCP_SLOT && out.len() > TCP_MAX {
            match self.rx_capacity {
                Some(limit) => limit,
                None => {
                    self.stamp_slot();
                    let reply = self.dhcp.pp_call(MessageInfo::new(QUERY_WIDE, 0));
                    let limit = match (reply.label(), reply.count()) {
                        // The original wide service returns only a label.
                        (WIDE_READY, 0) => LEGACY_WIDE_LEN,
                        (WIDE_READY, 1) => (sel4_microkit::get_mr(0) as usize).clamp(TCP_MAX, WIDE_LEN),
                        _ => TCP_MAX,
                    };
                    self.rx_capacity = Some(limit);
                    debug_println!("UIAPP|WALLET|TCP_RX|capacity={}", limit);
                    limit
                }
            }
        } else { TCP_MAX };
        let count = out.len().min(limit);
        let wide = count > TCP_MAX;
        {
            let slot = self.slot;
            let page = self.page_mut();
            page.magic = TCP_MAGIC;
            page.reserved = slot;
            page.request_len = count as u32;
            page.response_len = 0;
        }
        let result = if wide && self.range_rx {
            sel4_microkit::set_mr(0, 1); // WALLET_RX_WAKE_ONCE
            self.dhcp.pp_call(MessageInfo::new(RECV_WIDE, 1)).label()
        } else {
            self.call(if wide { RECV_WIDE } else { RECV })
        };
        match result {
            DATA => {
                let page = self.page_mut();
                let n = page.response_len as usize;
                if n == 0 { return Ok(None); }
                if n > count { return Err(nexatrode_guest_https::StreamError::InvalidReply); }
                if wide {
                    // All four legacy pages precede this wallet-only RX tail.
                    let base = unsafe { self.page.as_ptr().sub(WALLET_TCP_SLOT as usize).cast::<u8>() };
                    let source = unsafe { core::slice::from_raw_parts(base.add(WIDE_OFFSET), n) };
                    out[..n].copy_from_slice(source);
                } else {
                    out[..n].copy_from_slice(&page.response[..n]);
                }
                Ok(Some(n))
            }
            PENDING | BUSY => Ok(None),
            CLOSED => Ok(Some(0)),
            ERROR | NOT_READY => Err(nexatrode_guest_https::StreamError::Unavailable),
            _ => Err(nexatrode_guest_https::StreamError::InvalidReply),
        }
    }

    pub fn send_all(&mut self, data: &[u8]) -> Result<(), ()> {
        let mut offset = 0;
        while offset < data.len() {
            let chunk = core::cmp::min(TCP_MAX, data.len() - offset);
            {
                let slot = self.slot;
                let page = self.page_mut();
                page.magic = TCP_MAGIC;
                page.reserved = slot;
                page.request[..chunk].copy_from_slice(&data[offset..offset + chunk]);
                page.request_len = chunk as u32;
            }
            let mut accepted = false;
            for _ in 0..600u32 {
                self.stamp_slot();
                let status = self.call(SEND);
                if status == ACCEPTED {
                    accepted = true;
                    break;
                }
                if status != BUSY && status != PENDING {
                    debug_println!("UIAPP|WALLET|TCP_SEND|{}", status);
                    return Err(());
                }
                self.pause_poll();
            }
            if !accepted {
                debug_println!("UIAPP|WALLET|TCP_SEND|timeout");
                return Err(());
            }
            offset += chunk;
        }
        Ok(())
    }

    /// One RECV PPC. Ok(n)=bytes appended (0 if pending); Err after hard failure.
    /// `done` is set when the HTTP response looks complete or the peer closed.
    pub fn poll_recv(&mut self, out: &mut [u8], filled: &mut usize) -> Result<bool, ()> {
        {
            let slot = self.slot;
            let page = self.page_mut();
            page.magic = TCP_MAGIC;
            page.reserved = slot;
            page.request_len = TCP_MAX as u32;
            page.response_len = 0;
        }
        let status = self.call(RECV);
        if status == DATA {
            let page = self.page_mut();
            let n = page.response_len as usize;
            if n == 0 {
                return Ok(false);
            }
            if *filled + n > out.len() {
                return Err(());
            }
            out[*filled..*filled + n].copy_from_slice(&page.response[..n]);
            *filled += n;
            return Ok(http_response_complete(&out[..*filled]));
        }
        if status == PENDING || status == BUSY {
            // Even with no new bytes, a prior chunk may already hold a full
            // JSON-RPC body (HTTP/1.1 keep-alive without FIN).
            if *filled > 0 && http_response_complete(&out[..*filled]) {
                return Ok(true);
            }
            return Ok(false);
        }
        if status == CLOSED {
            // tip768: Cake FIN before full Content-Length made poll_recv report
            // done → handle_rpc_body NO_BLOB → TIP_HANG_SKIP (walked tip without
            // scanning). Only finish on FIN when the HTTP body is whole; else Err
            // so sync_retry_recv reconnects instead of stall→hang-skip.
            if *filled > 0 && http_response_complete(&out[..*filled]) {
                return Ok(true);
            }
            if *filled > 0 {
                debug_println!(
                    "UIAPP|WALLET|TCP_RECV|EARLY_FIN filled={}",
                    *filled
                );
                return Err(());
            }
            // Empty FIN used to return Ok(true) → sync BAD_HTTP on filled=0 and
            // burn tip GetTxBatch retries (deposit H3803776 peers never decoded).
            debug_println!("UIAPP|WALLET|TCP_RECV|EMPTY_FIN");
            return Err(());
        }
        if *filled > 0 {
            // Non-CLOSED terminal: only finish if the HTTP body is whole.
            if http_response_complete(&out[..*filled]) {
                return Ok(true);
            }
            debug_println!(
                "UIAPP|WALLET|TCP_RECV|INCOMPLETE_TERM st={} filled={}",
                status,
                *filled
            );
            return Err(());
        }
        debug_println!("UIAPP|WALLET|TCP_RECV|{}", status);
        Err(())
    }

    pub fn recv_until_closed(&mut self, out: &mut [u8]) -> Result<usize, ()> {
        let mut filled = 0usize;
        for _ in 0..1200u32 {
            match self.poll_recv(out, &mut filled)? {
                true => return Ok(filled),
                false => self.pause_poll(),
            }
        }
        debug_println!("UIAPP|WALLET|TCP_RECV|spin filled={}", filled);
        if filled > 0 {
            Ok(filled)
        } else {
            Err(())
        }
    }

    pub fn close(&mut self) {
        self.stamp_slot();
        let _ = self.call(CLOSE);
    }
}

/// Build a HTTP/1.0 POST for Monero JSON-RPC (connection close; easy framing).
pub fn build_json_rpc_http(host: &str, body: &[u8], out: &mut [u8]) -> Option<usize> {
    json_rpc_http(host, body, out, false)
}

pub fn build_scan_preflight_http(host: &str, body: &[u8], out: &mut [u8]) -> Option<usize> {
    json_rpc_http(host, body, out, true)
}

fn json_rpc_http(host: &str, body: &[u8], out: &mut [u8], persistent: bool) -> Option<usize> {
    let mut n = 0usize;
    macro_rules! push {
        ($bytes:expr) => {{
            let b: &[u8] = $bytes;
            if n + b.len() > out.len() {
                return None;
            }
            out[n..n + b.len()].copy_from_slice(b);
            n += b.len();
        }};
    }
    if persistent {
        push!(b"POST /json_rpc HTTP/1.1\r\nHost: ");
    } else {
        push!(b"POST /json_rpc HTTP/1.0\r\nHost: ");
    }
    push!(host.as_bytes());
    push!(b"\r\nContent-Type: application/json\r\nContent-Length: ");
    let mut len_buf = [0u8; 10];
    let mut len = body.len();
    let mut i = len_buf.len();
    if len == 0 {
        i -= 1;
        len_buf[i] = b'0';
    } else {
        while len > 0 {
            i -= 1;
            len_buf[i] = b'0' + (len % 10) as u8;
            len /= 10;
        }
    }
    push!(&len_buf[i..]);
    if persistent {
        push!(b"\r\nAccept-Encoding: identity\r\n\r\n");
    } else {
        push!(b"\r\nConnection: close\r\n\r\n");
    }
    push!(body);
    Some(n)
}

pub fn http_body(response: &[u8]) -> Option<&[u8]> {
    if let Some(at) = http_header_end(response) {
        return Some(&response[at..]);
    }
    // Some stacks deliver HTTP/1.x with odd line endings; fall back to the
    // first JSON object after a status line.
    if response.starts_with(b"HTTP/") {
        if let Some(i) = response.iter().position(|&b| b == b'{') {
            return Some(&response[i..]);
        }
    }
    None
}

fn http_header_end(response: &[u8]) -> Option<usize> {
    // Prefer CRLF; also accept LF-only separators from some proxies.
    for sep in [b"\r\n\r\n".as_slice(), b"\n\n".as_slice()] {
        let mut i = 0;
        while i + sep.len() <= response.len() {
            if &response[i..i + sep.len()] == sep {
                return Some(i + sep.len());
            }
            i += 1;
        }
    }
    None
}

fn http_content_length(headers: &[u8]) -> Option<usize> {
    const KEY: &[u8] = b"content-length:";
    let mut i = 0;
    while i + KEY.len() <= headers.len() {
        let window = &headers[i..i + KEY.len()];
        let mut matched = true;
        for (a, b) in window.iter().zip(KEY.iter()) {
            if a.to_ascii_lowercase() != *b {
                matched = false;
                break;
            }
        }
        if matched {
            let mut j = i + KEY.len();
            while j < headers.len() && headers[j] == b' ' {
                j += 1;
            }
            let start = j;
            while j < headers.len() && headers[j].is_ascii_digit() {
                j += 1;
            }
            if j == start {
                return None;
            }
            let mut value = 0usize;
            for &digit in &headers[start..j] {
                value = value.saturating_mul(10).saturating_add((digit - b'0') as usize);
            }
            return Some(value);
        }
        i += 1;
    }
    None
}

pub fn json_object_complete(body: &[u8]) -> bool {
    if !body.starts_with(b"{") {
        return false;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for &b in body {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    false
}

/// True when headers are present and the body matches Content-Length (or any body if CL absent).
pub fn http_response_complete(response: &[u8]) -> bool {
    let (headers, body) = if let Some(body_at) = http_header_end(response) {
        (&response[..body_at], &response[body_at..])
    } else if response.starts_with(b"HTTP/") {
        // No clean header/body sep — still finish once the JSON object is whole.
        let Some(i) = response.iter().position(|&b| b == b'{') else {
            return false;
        };
        return json_object_complete(&response[i..]);
    } else {
        return false;
    };
    match http_content_length(headers) {
        // Require the full Content-Length. Accepting json_object_complete early
        // (or any non-empty body) truncated tip get_block mid-blob → SCAN NO BLOB
        // (dump127) before tx_hashes arrived.
        Some(len) => body.len() >= len,
        // No Content-Length: only finish once the JSON object is whole.
        None => json_object_complete(body),
    }
}

