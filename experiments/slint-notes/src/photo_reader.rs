#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#[cfg(target_os = "none")]
extern crate alloc;
#[cfg(target_os = "none")]
#[allow(unexpected_cfgs, dead_code)]
#[path = "../../../rust/src/wallet_scan_mailbox.rs"]
mod photo_mailbox;
#[cfg(target_os = "none")]
mod photo_read;
#[cfg(target_os = "none")]
mod photo_reader_ipc;
#[cfg(target_os = "none")]
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
#[cfg(target_os = "none")]
use photo_reader_ipc::PhotoReadMailbox;
#[cfg(target_os = "none")]
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};

#[cfg(target_os = "none")]
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();
#[cfg(target_os = "none")]
struct Reader {
    slot: &'static PhotoReadMailbox,
    transfer: *mut BlobTransferPage,
}
#[cfg(target_os = "none")]
#[protection_domain]
fn init() -> Reader {
    let heap = memory_region_symbol!(photo_read_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(
        heap.as_ptr(),
        0x200000,
    ))
    .unwrap();
    let mailbox = memory_region_symbol!(photo_read_mailbox: *mut PhotoReadMailbox);
    let transfer = memory_region_symbol!(photo_read_transfer: *mut BlobTransferPage);
    debug_println!("SLINT PHOTO READER READY");
    Reader {
        slot: unsafe { mailbox.as_ref() },
        transfer: transfer.as_ptr(),
    }
}
#[cfg(target_os = "none")]
impl Handler for Reader {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        let ui = Channel::new(0);
        if !channels.contains(ui) {
            return Ok(());
        }
        let transfer = self.transfer;
        #[cfg(feature = "utm-perf-trace")]
        let start = trace_time_us();
        let executed = self.slot.execute(|request, output| {
            let (owner, id) = photo_reader_ipc::parse_request(request)?;
            let mut total = 0usize;
            photo_read::read_photo(owner, id, output, |owner, id, part, dst| {
                set_mr(0, id);
                set_mr(1, part as u64);
                set_mr(2, owner);
                let reply = Channel::new(1).pp_call(MessageInfo::new(0x438, 3));
                if reply.label() != 0x300 || reply.count() != if part == 0 { 2 } else { 0 } {
                    return None;
                }
                #[cfg(feature = "utm-perf-trace")]
                let mut first_owner = 0;
                if part == 0 {
                    let actual_owner = get_mr(0);
                    #[cfg(feature = "utm-perf-trace")]
                    {
                        first_owner = actual_owner;
                    }
                    if actual_owner != owner && actual_owner != u64::MAX {
                        return None;
                    }
                    total = get_mr(1) as usize;
                }
                let page = unsafe { &mut *transfer };
                let payload = page.payload()?;
                if payload.len() > dst.len() {
                    return None;
                }
                let n = payload.len();
                dst[..n].copy_from_slice(payload);
                page.set_payload(&[]);
                #[cfg(feature = "utm-perf-trace")]
                if part == 0 && first_owner != u64::MAX {
                    let wrong = if owner == 1 { 2 } else { 1 };
                    set_mr(0, id);
                    set_mr(1, 0);
                    set_mr(2, wrong);
                    let denied = Channel::new(1).pp_call(MessageInfo::new(0x438, 3));
                    let empty =
                        unsafe { (&*transfer).payload().is_some_and(|bytes| bytes.is_empty()) };
                    if denied.label() != 0x304 || !empty {
                        debug_println!("PERF|PHOTO_ACCESS|FAIL");
                        return None;
                    }
                    debug_println!("PERF|PHOTO_ACCESS|PASS");
                }
                Some((total, n))
            })
        });
        #[cfg(feature = "utm-perf-trace")]
        debug_println!(
            "PERF|READER|{}|{}|{}",
            start,
            trace_time_us(),
            u8::from(executed)
        );
        if executed {
            ui.notify();
        }
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
