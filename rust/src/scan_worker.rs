//! Tip-scan helper PD: fetches get_block over its own dhcp TCP slot.
#![no_std]
#![no_main]

extern crate alloc;

#[cfg(target_os = "none")]
#[path = "freestanding.rs"]
mod freestanding;

mod wallet_scan_protocol;
mod wallet_tcp;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};
use wallet_scan_protocol::{
    REPLY_WORKER_BAD, REPLY_WORKER_OK, REQUEST_WORKER_ARM_BLOCK, REQUEST_WORKER_PING,
    SCAN_JOB_MAGIC, ScanJobPage, WORKER_FROM_WALLET_CHANNEL,
};
use wallet_tcp::{WalletTcp, http_body};
use walletcore_core::{build_get_block_body, build_http_post, json_string_array};

// get_block JSON for busy tip blocks needs ~512KiB recv; keep headroom for
// tx_hashes parse copies. Bump allocator never frees.
const HEAP_BYTES: usize = 768 * 1024;
#[repr(align(16))]
struct HeapMem([u8; HEAP_BYTES]);
static mut HEAP_MEM: HeapMem = HeapMem([0; HEAP_BYTES]);
static HEAP_USED: AtomicUsize = AtomicUsize::new(0);

struct BootHeap;
unsafe impl GlobalAlloc for BootHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = core::ptr::addr_of_mut!(HEAP_MEM.0) as *mut u8 as usize;
        let align = layout.align();
        let size = layout.size();
        let mut used = HEAP_USED.load(Ordering::Relaxed);
        loop {
            let aligned = (used + align - 1) & !(align - 1);
            let next = aligned.saturating_add(size);
            if next > HEAP_BYTES {
                return core::ptr::null_mut();
            }
            match HEAP_USED.compare_exchange(used, next, Ordering::SeqCst, Ordering::Relaxed) {
                Ok(_) => return (base + aligned) as *mut u8,
                Err(cur) => used = cur,
            }
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BootHeap = BootHeap;

const DAEMON_HOST: &str = "xmr-node.cakewallet.com";
const DAEMON_PORT: u16 = 18081;
const POLL_MAX: u32 = 2000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    WaitConnect,
    Send,
    Recv,
}

#[protection_domain]
fn init() -> ScanWorker {
    let job = memory_region_symbol!(scan_job: *mut ScanJobPage);
    unsafe {
        job.as_ptr().write(ScanJobPage {
            magic: SCAN_JOB_MAGIC,
            height: 0,
            status: ScanJobPage::STATUS_IDLE,
            outs: 0,
        });
    }
    debug_println!("UIAPP|SCAN_WORKER|READY");
    ScanWorker {
        job,
        tcp: None,
        phase: Phase::Idle,
        polls: 0,
        req: [0u8; 2048],
        req_len: 0,
        resp: Vec::new(),
        resp_len: 0,
        height: 0,
    }
}

struct ScanWorker {
    job: NonNull<ScanJobPage>,
    tcp: Option<WalletTcp>,
    phase: Phase,
    polls: u32,
    req: [u8; 2048],
    req_len: usize,
    resp: Vec<u8>,
    resp_len: usize,
    height: u64,
}

impl ScanWorker {
    fn job_mut(&mut self) -> &mut ScanJobPage {
        unsafe { self.job.as_mut() }
    }

    fn tcp_mut(&mut self) -> &mut WalletTcp {
        if self.tcp.is_none() {
            self.tcp = Some(WalletTcp::map_worker());
        }
        self.tcp.as_mut().unwrap()
    }

    fn fail(&mut self, why: &str) {
        debug_println!("UIAPP|SCAN_WORKER|FAIL {}", why);
        let page = self.job_mut();
        page.status = ScanJobPage::STATUS_FAIL;
        page.outs = 0;
        if let Some(tcp) = self.tcp.as_mut() {
            tcp.close();
        }
        self.phase = Phase::Idle;
    }

    fn ping(&mut self) -> MessageInfo {
        self.job_mut().magic = SCAN_JOB_MAGIC;
        debug_println!("UIAPP|SCAN_WORKER|PING");
        MessageInfo::new(REPLY_WORKER_OK, 0)
    }

    fn arm_block(&mut self) -> MessageInfo {
        let height = get_mr(0);
        if height == 0 {
            return MessageInfo::new(REPLY_WORKER_BAD, 0);
        }
        let mut body = [0u8; 256];
        let Some(body_len) = build_get_block_body(height, &mut body) else {
            return MessageInfo::new(REPLY_WORKER_BAD, 0);
        };
        let mut http = [0u8; 2048];
        let Some(http_len) =
            build_http_post(DAEMON_HOST, "/json_rpc", &body[..body_len], &mut http)
        else {
            return MessageInfo::new(REPLY_WORKER_BAD, 0);
        };
        self.req[..http_len].copy_from_slice(&http[..http_len]);
        self.req_len = http_len;
        self.height = height;
        // Drop the old Vec so the next resize isn't layered on exhausted bump.
        self.resp = Vec::new();
        self.resp_len = 0;
        self.polls = 0;
        HEAP_USED.store(0, Ordering::SeqCst);
        {
            let page = self.job_mut();
            page.magic = SCAN_JOB_MAGIC;
            page.height = height;
            page.outs = 0;
            page.status = ScanJobPage::STATUS_ARMED;
        }
        let ip = [173, 255, 212, 53];
        if self.tcp_mut().open(ip, DAEMON_PORT).is_err() {
            self.fail("CONNECT");
            return MessageInfo::new(REPLY_WORKER_BAD, 0);
        }
        self.phase = Phase::WaitConnect;
        debug_println!("UIAPP|SCAN_WORKER|ARM h={}", height);
        MessageInfo::new(REPLY_WORKER_OK, 0)
    }

    fn advance(&mut self) {
        match self.phase {
            Phase::Idle => {}
            Phase::WaitConnect => match self.tcp_mut().poll_connected() {
                Ok(true) => {
                    self.polls = 0;
                    self.phase = Phase::Send;
                    debug_println!("UIAPP|SCAN_WORKER|CONNECTED");
                }
                Ok(false) => {
                    self.polls = self.polls.saturating_add(1);
                    if self.polls >= POLL_MAX {
                        self.fail("CONNECT_TIMEOUT");
                    }
                }
                Err(()) => self.fail("CONNECT"),
            },
            Phase::Send => {
                let n = self.req_len;
                let mut copy = [0u8; 2048];
                copy[..n].copy_from_slice(&self.req[..n]);
                if self.tcp_mut().send_all(&copy[..n]).is_err() {
                    self.fail("SEND");
                    return;
                }
                self.resp.clear();
                self.resp.resize(512 * 1024, 0);
                self.resp_len = 0;
                self.polls = 0;
                self.phase = Phase::Recv;
                debug_println!("UIAPP|SCAN_WORKER|SENT {}", n);
            }
            Phase::Recv => {
                let mut filled = self.resp_len;
                let status = {
                    let buf = &mut self.resp[..];
                    self.tcp.as_mut().unwrap().poll_recv(buf, &mut filled)
                };
                match status {
                    Ok(true) => {
                        self.resp_len = filled;
                        self.tcp_mut().close();
                        let body = {
                            let Some(json) = http_body(&self.resp[..filled]) else {
                                self.fail("BAD_HTTP");
                                return;
                            };
                            json.to_vec()
                        };
                        let txs = json_string_array(&body, b"tx_hashes").unwrap_or_default();
                        let height = self.height;
                        let outs = txs.len() as u32;
                        let page = self.job_mut();
                        page.height = height;
                        page.outs = outs;
                        page.status = ScanJobPage::STATUS_DONE;
                        self.phase = Phase::Idle;
                        debug_println!(
                            "UIAPP|SCAN_WORKER|BLOCK_OK h={} txs={}",
                            self.height,
                            txs.len()
                        );
                    }
                    Ok(false) => {
                        self.resp_len = filled;
                        self.polls = self.polls.saturating_add(1);
                        if self.polls >= POLL_MAX {
                            self.fail("RECV_TIMEOUT");
                        }
                    }
                    Err(()) => self.fail("RECV"),
                }
            }
        }
    }
}

impl Handler for ScanWorker {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(WORKER_FROM_WALLET_CHANNEL) && self.phase != Phase::Idle {
            self.advance();
        }
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != WORKER_FROM_WALLET_CHANNEL {
            return Ok(MessageInfo::new(REPLY_WORKER_BAD, 0));
        }
        Ok(match request.label() {
            REQUEST_WORKER_PING if request.count() == 0 => self.ping(),
            REQUEST_WORKER_ARM_BLOCK if request.count() == 1 => self.arm_block(),
            _ => MessageInfo::new(REPLY_WORKER_BAD, 0),
        })
    }
}
