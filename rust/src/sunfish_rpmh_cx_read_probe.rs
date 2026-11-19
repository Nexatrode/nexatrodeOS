#![no_std]
#![no_main]

mod pixel_probe_display;
mod pixel_rpmh_controller;

use personal_os_rust_server::qcom_rpmh_read::{CX_ADDRESS, CxRead, READ_MESSAGE, Snapshot};
use pixel_probe_display::Display;
use pixel_rpmh_controller::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65_536; // A work bound, not milliseconds.

#[protection_domain]
fn init() -> CxReadProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "RPMH CX READ");
    display.line(1, "READ COMMAND ONLY");
    display.line(2, "PREFLIGHT START");
    display.line(27, "NO POWER VOTE");
    display.line(29, "RESTART WHEN DONE");
    display.flush();

    // The build checks the live-derived DT's Apps DRV2/active-first allocation.
    // This one-PD temporary image replaces Linux and owns the AP controller
    // endpoint; it never claims ownership merely from the IDLE register.
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc: *mut u32).as_ptr()) };
    let mut read = CxRead::new(WORK_LIMIT);
    let result = read.start(&mut controller).and_then(|()| {
        display.clear();
        display.line(0, "RPMH CX READ");
        display.line(1, "READ COMMAND ONLY");
        display.line(2, "READ IN FLIGHT");
        display.line(27, "NO POWER VOTE");
        display.line(29, "RESTART WHEN DONE");
        display.flush();
        loop {
            if read.poll(&mut controller)? {
                return Ok(());
            }
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        }
    });
    let before = read.initial();
    let after = Snapshot::read(&mut controller);
    display.clear();
    display.line(0, "RPMH CX READ");
    display.line(
        1,
        if result.is_ok() {
            "CX READ PASS"
        } else {
            "CX READ FAIL"
        },
    );
    if let Err(error) = result {
        display.hex(2, "ERROR", error.code());
    } else {
        display.line(2, "RESPONSE AND CLEANUP OK");
    }
    display.hex(3, "CX ADDRESS", CX_ADDRESS);
    display.hex(4, "READ MSGID", READ_MESSAGE);
    display.hex(5, "CX RESPONSE", read.response().unwrap_or(u32::MAX));
    display.hex(6, "RSC CONFIG", before.config);
    display.hex(7, "BEFORE IRQ EN", before.irq_enable);
    display.hex(8, "BEFORE IRQ ST", before.irq_status);
    display.hex(9, "BEFORE IDLE", before.idle);
    display.hex(10, "BEFORE ENABLE", before.enable);
    display.hex(11, "BEFORE WAIT", before.wait);
    display.hex(12, "BEFORE CONTROL", before.control);
    display.hex(13, "AFTER IRQ EN", after.irq_enable);
    display.hex(14, "AFTER IRQ ST", after.irq_status);
    display.hex(15, "AFTER IDLE", after.idle);
    display.hex(16, "AFTER ENABLE", after.enable);
    display.hex(17, "AFTER WAIT", after.wait);
    display.hex(18, "AFTER CONTROL", after.control);
    display.hex(19, "COMMAND STATUS", after.command_status);
    display.hex(20, "OTHER IDLE", after.other_idle);
    display.hex(21, "OTHER ENABLE", after.other_enable);
    display.hex(22, "OTHER WAIT", after.other_wait);
    display.hex(23, "OTHER CONTROL", after.other_control);
    display.hex(24, "WORK LIMIT", WORK_LIMIT);
    display.line(26, "NO MODEM START");
    display.line(27, "NO POWER VOTE");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!(
        "RPMH-CX-READ|RESULT|{:?} response={:?} before={:?} after={:?}",
        result,
        read.response(),
        before,
        after
    );
    CxReadProbe
}

struct CxReadProbe;
impl Handler for CxReadProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
