#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
#[allow(unexpected_cfgs, dead_code)]
#[path = "../../../rust/src/wallet_scan_mailbox.rs"]
mod photo_mailbox;
#[cfg(target_os = "none")]
mod transcript_ipc;
#[cfg(target_os = "none")]
mod live_receive;
#[cfg(target_os = "none")]
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
#[cfg(target_os = "none")]
use transcript_ipc::{TranscriptMailbox, HEADER};
#[cfg(target_os = "none")]
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, MessageInfo, get_mr, memory_region_symbol, protection_domain, set_mr};
#[cfg(all(target_os = "none", feature = "utm-perf-trace"))]
use sel4_microkit::debug_println;

#[cfg(target_os = "none")]
struct Worker {
    mailbox: &'static TranscriptMailbox,
    live_mailbox: &'static photo_mailbox::Mailbox<1, 4096>,
    transfer: *mut BlobTransferPage,
}

#[cfg(target_os = "none")]
#[protection_domain]
fn init() -> Worker {
    let mailbox = memory_region_symbol!(transcript_mailbox: *mut TranscriptMailbox);
    let transfer = memory_region_symbol!(messages_worker_transfer: *mut BlobTransferPage);
    let live = memory_region_symbol!(live_receive_mailbox: *mut photo_mailbox::Mailbox<1, 4096>);
    Worker { mailbox: unsafe { mailbox.as_ref() }, live_mailbox: unsafe { live.as_ref() }, transfer: transfer.as_ptr() }
}

#[cfg(target_os = "none")]
impl Handler for Worker {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        let ui = Channel::new(0);
        let live_ui = Channel::new(2);
        if channels.contains(live_ui) {
            let transfer = self.transfer;
            #[cfg(feature = "utm-perf-trace")]
            let started = trace_time_us();
            let executed = self.live_mailbox.execute(|request, output| {
                if request.len() != 1 || request[0] > 1 { return None; }
                let page = unsafe { &mut *transfer };
                if !page.set_payload(&[]) { return None; }
                if request[0] == 1 {
                    let reply = Channel::new(1).pp_call(MessageInfo::new(0x35f, 0));
                    #[cfg(feature = "utm-perf-trace")]
                    debug_println!("PERF|LIVE_RECEIVE_START_REPLY|{}|{}", reply.label(), reply.count());
                    if reply.label() != 0x360 || reply.count() != 0 { return None; }
                    if !page.set_payload(&[]) { return None; }
                }
                let reply = Channel::new(1).pp_call(MessageInfo::new(0x399, 0));
                #[cfg(feature = "utm-perf-trace")]
                debug_println!("PERF|LIVE_RECEIVE_REPLY|{}|{}", reply.label(), reply.count());
                if reply.label() != 0x362 || reply.count() != 4 { return None; }
                let status = get_mr(0);
                let length = usize::try_from(get_mr(1)).ok()?;
                let owner = get_mr(3);
                let body = page.payload().filter(|body| body.len() == length)?;
                live_receive::encode(output, status, owner, core::str::from_utf8(body).ok()?)
            });
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|LIVE_RECEIVE_WORKER|{}|{}|{}", started, trace_time_us(), u8::from(executed));
            if executed { live_ui.notify(); }
        }
        if !channels.contains(ui) { return Ok(()); }
        let transfer = self.transfer;
        if self.mailbox.execute(|request, output| {
            let index = transcript_ipc::parse_request(request)?;
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|TRANSCRIPT_WORKER_INFO_START|{}", index);
            let reply = loop {
                set_mr(0, index as u64);
                let reply = Channel::new(1).pp_call(MessageInfo::new(0x395, 1));
                if reply.label() == 0x396 && reply.count() == 0 { continue; }
                break reply;
            };
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|TRANSCRIPT_WORKER_INFO_DONE|{}|{}|{}", index, reply.label(), reply.count());
            if reply.label() != 0x384 || reply.count() != 2 { return None; }
            let owner = get_mr(0);
            let revision = get_mr(1);
            if owner == 0 || owner == u64::MAX { return None; }
            output[..8].copy_from_slice(&owner.to_le_bytes());
            output[8..HEADER].copy_from_slice(&revision.to_le_bytes());
            let mut used = HEADER;
            let mut cursor = 0;
            loop {
                set_mr(0, owner);
                set_mr(1, revision);
                set_mr(2, cursor);
                let reply = Channel::new(1).pp_call(MessageInfo::new(0x397, 3));
                if reply.label() != 0x386 || reply.count() != 2 { return None; }
                let length = usize::try_from(get_mr(0)).ok()?;
                let next = get_mr(1);
                let payload = unsafe { &*transfer }.payload().filter(|p| p.len() == length)?;
                core::str::from_utf8(payload).ok()?;
                let end = used.checked_add(length)?;
                if end > output.len() { return None; }
                output[used..end].copy_from_slice(payload);
                used = end;
                if next == 0 { break; }
                if next <= cursor { return None; }
                cursor = next;
            }
            Some(used)
        }) { ui.notify(); }
        Ok(())
    }
}

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(all(target_os = "none", feature = "utm-perf-trace"))]
fn trace_time_us() -> u64 {
    let ticks: u64;
    let frequency: u64;
    unsafe {
        core::arch::asm!("isb", "mrs {v}, cntpct_el0", v = out(reg) ticks, options(nostack, preserves_flags));
        core::arch::asm!("mrs {v}, cntfrq_el0", v = out(reg) frequency, options(nostack, preserves_flags));
    }
    ticks / (frequency / 1_000_000).max(1)
}
