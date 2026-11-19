#![no_std]
#![no_main]

//! One-shot read-only UFSHCI register snapshot. No UFS command, power, reset,
//! interrupt, DMA, SMMU, or secure-monitor operation is available.

#[path = "freestanding.rs"]
mod freestanding;
#[allow(dead_code)]
mod pixel_probe_display;

use personal_os_rust_server::qcom_ufs_host::{
    REG_CAPABILITIES, REG_CONTROLLER_STATUS, REG_VERSION,
};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

fn read_register(base: *const u32, offset: u32) -> u32 {
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    let value = unsafe { base.add((offset / 4) as usize).read_volatile() };
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    value
}

fn checkpoint_read(display: &Display, base: *const u32, offset: u32, label: &str) -> u32 {
    display.replace_line(1, label);
    display.flush();
    let value = read_register(base, offset);
    display.hex(4, label, value);
    display.flush();
    value
}

#[protection_domain]
fn init() -> UfsHciReadProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "UFS HCI READ ONLY");
    display.line(1, "SNAPSHOT START");
    display.flush();

    display.replace_line(1, "MAP HCI MMIO");
    display.flush();
    let hci_vaddr = *sel4_microkit::var!(ufs_hci_mmio: usize = 0);
    display.hex(2, "HCI VADDR", hci_vaddr as u32);
    if hci_vaddr != 0x6400_0000 {
        display.line(3, "ADDR INVALID");
        display.line(4, "NO HCI READ");
        display.line(29, "RESTART WHEN DONE");
        display.flush();
        return UfsHciReadProbe;
    }
    display.line(3, "HCI MAPPED");
    display.line(4, "HCI READ ONLY");
    display.flush();

    let base = hci_vaddr as *const u32;
    let capabilities = checkpoint_read(&display, base, REG_CAPABILITIES, "READ CAP");
    let version = checkpoint_read(&display, base, REG_VERSION, "READ VERSION");
    let controller_status = checkpoint_read(&display, base, REG_CONTROLLER_STATUS, "READ HCS");

    display.clear();
    display.line(0, "UFS HCI READ ONLY");
    display.line(1, "READ COMPLETE");
    display.hex(2, "CAPABILITIES", capabilities);
    display.hex(3, "HCI VERSION", version);
    display.hex(4, "HCI STATUS", controller_status);
    display.line(12, "NO UFS COMMAND");
    display.line(13, "NO IRQ OR DMA");
    display.line(14, "NO SMMU OR SMC");
    display.line(15, "NO POWER OR RESET");
    display.line(29, "RESTART WHEN DONE");
    display.flush();

    debug_println!(
        "UFS-HCI-RO|DONE|cap={:08X}|version={:08X}|status={:08X}",
        capabilities,
        version,
        controller_status
    );
    UfsHciReadProbe
}

struct UfsHciReadProbe;

impl Handler for UfsHciReadProbe {
    type Error = Infallible;

    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }

    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
