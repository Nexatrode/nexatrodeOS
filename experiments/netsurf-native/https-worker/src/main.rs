// SPDX-License-Identifier: BSD-2-Clause
#![no_std]
#![no_main]
extern crate alloc;
#[path = "../../../../rust/src/freestanding.rs"]
mod freestanding;
mod http;
mod tcp;
use core::{
    ptr::{self, NonNull},
    sync::atomic::{AtomicU32, Ordering},
};
use nexatrode_guest_https::{PlaintextSink, Session};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};
const CLIENT: Channel = Channel::new(0);
const ENTROPY: Channel = Channel::new(2);
const TIMER: Channel = Channel::new(3);
const MAGIC: u32 = 0x31535448;
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();
getrandom::register_custom_getrandom!(random);
fn random(out: &mut [u8]) -> Result<(), getrandom::Error> {
    for chunk in out.chunks_mut(16) {
        let reply = ENTROPY.pp_call(MessageInfo::new(0x401, 0));
        if reply.label() != 0x400 || reply.count() != 2 {
            out.fill(0);
            return Err(getrandom::Error::from(
                core::num::NonZeroU32::new(getrandom::Error::CUSTOM_START).unwrap(),
            ));
        }
        let mut sample = [0u8; 16];
        sample[..8].copy_from_slice(&get_mr(0).to_le_bytes());
        sample[8..].copy_from_slice(&get_mr(1).to_le_bytes());
        chunk.copy_from_slice(&sample[..chunk.len()]);
        sample.fill(0);
    }
    Ok(())
}
#[repr(C)]
struct Request {
    magic: u32,
    seq: AtomicU32,
    command: u32,
    length: u32,
    url: [u8; 2048],
}
#[repr(C)]
struct Reply {
    magic: u32,
    seq: u32,
    state: AtomicU32,
    error: u32,
    status: u32,
    headers_len: u32,
    body_len: u32,
    reserved: u32,
    headers: [u8; 16384],
    body: [u8; http::BODY_MAX],
}
const _: () = assert!(core::mem::size_of::<Reply>() <= 0x205000);
const _: () = assert!(core::mem::size_of::<Request>() <= 4096);
struct Job {
    seq: u32,
    url: http::Url,
    phase: u8,
    tls: Option<Session>,
    response: http::Response,
    started: u64,
    progress: u64,
    wire: u64,
}
struct Worker {
    request: NonNull<Request>,
    reply: NonNull<Reply>,
    rtc: NonNull<u32>,
    tcp: tcp::Tcp,
    seen: u32,
    job: Option<Job>,
}
impl PlaintextSink for http::Response {
    fn append(&mut self, b: &[u8]) {
        self.append(b)
    }
    fn complete(&self) -> bool {
        self.complete()
    }
}
fn ms() -> u64 {
    let ticks: u64;
    let freq: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0",out(reg)ticks);
        core::arch::asm!("mrs {}, cntfrq_el0",out(reg)freq);
    }
    ticks / freq * 1000 + (ticks % freq) * 1000 / freq
}
#[protection_domain]
fn init() -> Worker {
    let heap = memory_region_symbol!(https_heap:*mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(
        heap.as_ptr(),
        0x1000000,
    ))
    .unwrap();
    let reply = memory_region_symbol!(https_response:*mut Reply);
    unsafe {
        ptr::write_bytes(
            reply.as_ptr().cast::<u8>(),
            0,
            core::mem::size_of::<Reply>(),
        );
        (*reply.as_ptr()).magic = MAGIC;
    }
    debug_println!("HTTPS|READY|rustls|verified certificates|bounded GET|no storage");
    Worker {
        request: memory_region_symbol!(https_request:*mut Request),
        reply,
        rtc: memory_region_symbol!(https_rtc:*mut u32),
        tcp: tcp::Tcp::new(),
        seen: 0,
        job: None,
    }
}
impl Worker {
    fn reply(&mut self, seq: u32, error: u32, status: u16, headers: &[u8], body: &[u8]) {
        let r = unsafe { self.reply.as_mut() };
        r.state.store(0, Ordering::Release);
        r.magic = MAGIC;
        r.seq = seq;
        r.error = error;
        r.status = status as u32;
        r.headers_len = headers.len() as u32;
        r.body_len = body.len() as u32;
        r.headers[..headers.len()].copy_from_slice(headers);
        r.body[..body.len()].copy_from_slice(body);
        r.state
            .store(if error == 0 { 1 } else { 2 }, Ordering::Release);
        CLIENT.notify();
    }
    fn incoming(&mut self) {
        let request = unsafe { self.request.as_ref() };
        let seq = request.seq.load(Ordering::Acquire);
        if seq == 0 || seq == self.seen {
            return;
        }
        self.seen = seq;
        self.job = None;
        self.tcp.cancel();
        // Snapshot before parsing. The untrusted frontend can never select a TCP slot,
        // root store, TLS time, filesystem path, or executable in this broker.
        let magic = unsafe { ptr::read_volatile(ptr::addr_of!(request.magic)) };
        let command = unsafe { ptr::read_volatile(ptr::addr_of!(request.command)) };
        let n = unsafe { ptr::read_volatile(ptr::addr_of!(request.length)) } as usize;
        if command == 2 {
            return;
        }
        if magic != MAGIC || command != 1 || n == 0 || n > 2047 {
            self.reply(seq, 1, 0, b"Invalid HTTPS request", b"");
            return;
        }
        let mut bytes = [0u8; 2048];
        unsafe {
            ptr::copy_nonoverlapping(request.url.as_ptr(), bytes.as_mut_ptr(), n);
        }
        let url = core::str::from_utf8(&bytes[..n])
            .ok()
            .and_then(|s| http::Url::parse(s).ok());
        let Some(url) = url else {
            self.reply(seq, 1, 0, b"Only valid HTTPS URLs are supported", b"");
            return;
        };
        debug_println!("HTTPS|START|id={}|host={}", seq, url.host);
        let now = ms();
        self.job = Some(Job {
            seq,
            url,
            phase: 0,
            tls: None,
            response: http::Response::new(),
            started: now,
            progress: now,
            wire: 0,
        });
    }
    fn step(&mut self, job: &mut Job) -> Result<bool, (u32, &'static str)> {
        let now = ms();
        if now - job.started > 45000 || now - job.progress > 15000 {
            return Err((3, "HTTPS request timed out"));
        }
        if job.phase == 0 {
            if self
                .tcp
                .dns_start(&job.url.host)
                .map_err(|_| (4, "DNS lookup failed"))?
            {
                job.phase = 1;
            }
            return Ok(false);
        }
        if job.phase == 1 {
            if let Some(ip) = self.tcp.dns_poll().map_err(|_| (4, "DNS lookup failed"))? {
                self.tcp
                    .open(ip, job.url.port)
                    .map_err(|_| (5, "TCP connection failed"))?;
                job.phase = 2;
                job.progress = now;
            }
            return Ok(false);
        }
        if job.phase == 2 {
            if !self
                .tcp
                .connected()
                .map_err(|_| (5, "TCP connection failed"))?
            {
                return Ok(false);
            }
            let seconds = unsafe { ptr::read_volatile(self.rtc.as_ptr()) } as u64;
            if seconds < 1_735_689_600 {
                return Err((6, "TLS clock unavailable"));
            }
            debug_println!("HTTPS|TLS|id={}|utc={}", job.seq, seconds);
            job.tls = Some(
                {
                #[cfg(not(feature = "test-fixtures"))]
                let session = Session::new(
                    &job.url.host,
                    seconds,
                    &job.url.request(),
                    http::BODY_MAX + http::HEADER_MAX,
                );
                #[cfg(feature = "test-fixtures")]
                let session = Session::new_browser_fixture(&job.url.host,seconds,
                    &job.url.request(),http::BODY_MAX+http::HEADER_MAX,
                    include_bytes!(env!("NETSURF_TEST_CA_DER")));
                session.map_err(|_| (6, "TLS initialization failed"))?
                },
            );
            job.phase = 3;
            job.progress = now;
        }
        let tls = job.tls.as_mut().unwrap();
        for _ in 0..64 {
            match tls.poll_stream(&mut self.tcp, &mut job.response) {
                Ok(true) => {
                    if job.response.error.is_some() {
                        return Err((7, "Unsupported or invalid HTTP response"));
                    }
                    return Ok(true);
                }
                Ok(false) => {}
                Err(nexatrode_guest_https::Error::PrematureClose) => {
                    job.response.eof(tls.peer_closed_cleanly());
                    if job.response.error.is_none() {
                        return Ok(true);
                    }
                    return Err((8, "HTTPS response was truncated"));
                }
                Err(e) => {
                    debug_println!("HTTPS|TLS_ERROR|id={}|{:?}", job.seq, e);
                    return Err(
                        if e.kind() == nexatrode_guest_https::FailureKind::Certificate {
                            (9, "TLS certificate rejected")
                        } else {
                            (2, "TLS verification or transport failed")
                        },
                    );
                }
            }
            let wire = tls.wire_progress();
            if wire == job.wire {
                break;
            }
            job.wire = wire;
            job.progress = now;
        }
        Ok(false)
    }
    fn tick(&mut self) {
        self.incoming();
        if let Some(mut job) = self.job.take() {
            match self.step(&mut job) {
                Ok(true) => {
                    debug_println!(
                        "HTTPS|DONE|id={}|host={}|status={}|bytes={}|ms={}",
                        job.seq,
                        job.url.host,
                        job.response.status,
                        job.response.body.len(),
                        ms() - job.started
                    );
                    self.reply(
                        job.seq,
                        0,
                        job.response.status,
                        &job.response.headers,
                        &job.response.body,
                    );
                    self.tcp.cancel();
                }
                Err((code, message)) => {
                    debug_println!(
                        "HTTPS|ERROR|id={}|host={}|code={}|{}",
                        job.seq,
                        job.url.host,
                        code,
                        message
                    );
                    self.reply(job.seq, code, 0, message.as_bytes(), b"");
                    self.tcp.cancel();
                }
                Ok(false) => {
                    self.job = Some(job);
                    set_mr(0, 2_000_000);
                    TIMER.pp_call(MessageInfo::new(1, 1));
                }
            }
        }
    }
}
impl Handler for Worker {
    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel == Channel::new(4) && request.label() == 0x205 && request.count() == 0 {
            set_mr(0, unsafe { ptr::read_volatile(self.rtc.as_ptr()) } as u64);
            return Ok(MessageInfo::new(0x100, 1));
        }
        Ok(MessageInfo::new(0x2ff, 0))
    }

    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        self.tick();
        Ok(())
    }
}
