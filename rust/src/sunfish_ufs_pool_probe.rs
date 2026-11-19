#![no_std]
#![no_main]

//! Temporary pool-only test for the dynamically allocated UFS backing RAM.
//! The system grants no UFS MMIO, interrupt, SMMU, SMC, modem, or WLAN access.

#[path = "freestanding.rs"]
mod freestanding;
#[allow(dead_code)]
mod pixel_probe_display;

use personal_os_rust_server::qcom_ufs_platform::{DmaBackingPool, UFS_DMA_POOL_BYTES};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

#[used]
#[unsafe(no_mangle)]
static mut ufs_dma_pool_paddr: u64 = 0;

fn test_each_page(base: *mut u8, pages: usize) -> usize {
    for page in 0..pages {
        // Every page index is bounded by the validated 16 MiB profile token.
        let word = unsafe { base.add(page * 4096).cast::<u64>() };
        let pattern = 0x5546_5350_0000_0000u64 | page as u64;
        let original = unsafe { word.read_volatile() };
        unsafe { word.write_volatile(pattern) };
        // Make the test check an observed write, then restore the fresh pool.
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
        let observed = unsafe { word.read_volatile() };
        unsafe { word.write_volatile(original) };
        // Make the restore visible even on the early failure path.
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
        if observed != pattern {
            return page;
        }
    }
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    pages
}

#[protection_domain]
fn init() -> UfsPoolProbe {
    // SAFETY: this system maps the dedicated splash framebuffer for this PD.
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    let pool_base = memory_region_symbol!(ufs_dma_pool: *mut u8).as_ptr();
    let physical_base = unsafe { core::ptr::read_volatile(&raw const ufs_dma_pool_paddr) };

    display.clear();
    display.line(0, "UFS POOL ONLY");
    display.line(1, "PROFILE CHECK");
    display.flush();

    let pool =
        match DmaBackingPool::from_profile(pool_base as u64, physical_base, UFS_DMA_POOL_BYTES) {
            Ok(pool) => pool,
            Err(_) => {
                display.clear();
                display.line(0, "UFS POOL ONLY");
                display.line(1, "PROFILE INVALID");
                display.hex(2, "POOL VIRT", pool_base as u32);
                display.hex(3, "POOL PHYS", physical_base as u32);
                display.line(15, "NO UFS MMIO OR DMA");
                display.line(16, "NO SMC MODEM WLAN");
                display.line(29, "RESTART WHEN DONE");
                display.flush();
                debug_println!(
                    "UFS-POOL|INVALID|vaddr={:08X}|paddr={:08X}",
                    pool_base as u32,
                    physical_base as u32
                );
                return UfsPoolProbe;
            }
        };

    let pages = pool.page_count();
    let scanned = test_each_page(pool_base, pages);
    let valid = scanned == pages
        && pool.page_physical(pages - 1).is_ok_and(|last| {
            last.checked_add(4096) == pool.physical_base().checked_add(pool.bytes())
        });

    display.clear();
    display.line(0, "UFS POOL ONLY");
    display.line(
        1,
        if valid {
            "PROFILE VALID"
        } else {
            "POOL TEST FAIL"
        },
    );
    display.hex(2, "POOL VIRT", pool.virtual_base() as u32);
    display.hex(3, "POOL PHYS", pool.physical_base() as u32);
    display.hex(4, "POOL BYTES", pool.bytes() as u32);
    display.hex(5, "PAGES CHECKED", scanned as u32);
    display.line(
        6,
        if valid {
            "PAGE RW PASS"
        } else {
            "PAGE RW FAIL"
        },
    );
    display.line(15, "NO UFS MMIO OR DMA");
    display.line(16, "NO SMMU OR SMC");
    display.line(17, "NO MODEM OR WLAN");
    display.line(29, "RESTART WHEN DONE");
    display.flush();

    debug_println!(
        "UFS-POOL|DONE|valid={}|vaddr={:08X}|paddr={:08X}|bytes={:08X}|pages={}/{}",
        valid,
        pool.virtual_base() as u32,
        pool.physical_base() as u32,
        pool.bytes() as u32,
        scanned,
        pages
    );
    UfsPoolProbe
}

struct UfsPoolProbe;

impl Handler for UfsPoolProbe {
    type Error = Infallible;

    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }

    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
