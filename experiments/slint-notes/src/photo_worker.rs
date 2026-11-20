#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#[cfg(target_os = "none")]
extern crate alloc;
#[cfg(target_os = "none")]
mod photo_ipc;
#[cfg(target_os = "none")]
#[allow(unexpected_cfgs, dead_code)]
#[path = "../../../rust/src/wallet_scan_mailbox.rs"]
mod photo_mailbox;
#[cfg(target_os = "none")]
mod photo_preview;
#[cfg(target_os = "none")]
use photo_ipc::PhotoMailbox;
#[cfg(target_os = "none")]
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, debug_println, memory_region_symbol,
    protection_domain,
};

#[cfg(target_os = "none")]
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();
#[cfg(target_os = "none")]
struct Worker {
    slot: &'static PhotoMailbox,
}
#[cfg(target_os = "none")]
#[protection_domain]
fn init() -> Worker {
    let heap = memory_region_symbol!(photo_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(
        heap.as_ptr(),
        0x4000000,
    ))
    .unwrap();
    let mailbox = memory_region_symbol!(photo_mailbox_region: *mut PhotoMailbox);
    debug_println!("SLINT PHOTO WORKER READY");
    Worker {
        slot: unsafe { mailbox.as_ref() },
    }
}
#[cfg(target_os = "none")]
impl Handler for Worker {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        let channel = Channel::new(0);
        if channels.contains(channel) {
            #[cfg(feature = "utm-perf-trace")]
            let before = trace_time_us();
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|WORKER_START|{}", before);
            let executed = self.slot.execute(photo_ipc::encode_preview);
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|WORKER_END|{}|{}", trace_time_us(), u8::from(executed));
            if executed {
                channel.notify();
            }
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
