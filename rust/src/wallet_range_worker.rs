#![no_std]
#![no_main]
mod wallet_scan_mailbox;
#[cfg(feature = "wallet-range-profile")]
mod wallet_profile_clock;

use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, debug_println, memory_region_symbol,
    protection_domain,
};
use wallet_scan_mailbox::Mailbox;
use walletcore_core::{MAX_SCAN_JOB, MAX_SCAN_RESULT};
#[cfg(not(feature = "wallet-range-profile"))]
use walletcore_core::execute_scan_job;

type ScanMailbox = Mailbox<MAX_SCAN_JOB, MAX_SCAN_RESULT>;
const _: () = assert!(core::mem::size_of::<ScanMailbox>() <= 0x400000);

#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();

struct Worker {
    slot: &'static ScanMailbox,
}

#[protection_domain]
fn init() -> Worker {
    let heap = memory_region_symbol!(range_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(
        heap.as_ptr(),
        0x1000000,
    ))
    .unwrap();
    let slot = memory_region_symbol!(range_mailbox: *mut ScanMailbox);
    debug_println!(
        "UIAPP|WALLET|WORKER|READY {}",
        sel4_microkit::pd_name().unwrap_or("?")
    );
    Worker {
        slot: unsafe { slot.as_ref() },
    }
}

impl Handler for Worker {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(Channel::new(0)) {
            #[cfg(not(feature = "wallet-range-profile"))]
            self.slot.execute(|job, result| execute_scan_job(job, result).ok());
            #[cfg(feature = "wallet-range-profile")]
            self.slot.execute(|job, result| {
                let start = wallet_profile_clock::now_us();
                let mut boundaries = [start; 2];
                let mut marks = boundaries.iter_mut();
                let len = walletcore_core::execute_profiled_scan_job(job, result, || {
                    *marks.next().unwrap() = wallet_profile_clock::now_us();
                }).ok()?;
                let end = wallet_profile_clock::now_us();
                self.slot.set_worker_times([boundaries[0] - start, boundaries[1] - boundaries[0], end - boundaries[1]]);
                Some(len)
            });
        }
        Ok(())
    }
}
