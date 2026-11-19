#![no_std]
#![no_main]

mod pixel_probe_display;
mod pixel_rpmh_controller;

use personal_os_rust_server::qcom_rpmh_read::read_series;
use pixel_probe_display::Display;
use pixel_rpmh_controller::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65_536; // Per read, not milliseconds.

#[protection_domain]
fn init() -> ResourceReadsProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "RPMH RESOURCE READS");
    display.line(1, "TWO PASSES CX MSS MX XO");
    display.line(2, "READS IN PROGRESS");
    display.line(27, "NO POWER VOTES");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    // One PD replaces Linux as the standalone AP DRV2 controller owner.
    // Only its first 4 KiB is mapped, containing active banks 0 and 1.
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc: *mut u32).as_ptr()) };
    let report = read_series(&mut controller, WORK_LIMIT, || {
        for _ in 0..256 {
            unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
        }
    });

    display.clear();
    display.line(0, "RPMH RESOURCE READS");
    display.line(
        1,
        if report.passed() {
            "RESOURCE READS PASS"
        } else {
            "RESOURCE READS FAIL"
        },
    );
    display.hex(2, "READS COMPLETED", u32::from(report.completed));
    for (index, labels) in [
        ["CX FIRST", "CX SECOND"],
        ["MSS FIRST", "MSS SECOND"],
        ["MX FIRST", "MX SECOND"],
        ["XO FIRST", "XO SECOND"],
    ]
    .into_iter()
    .enumerate()
    {
        for pass in 0..2 {
            if let Some(response) = report.responses[pass][index] {
                display.hex(3 + index * 2 + pass, labels[pass], response);
            } else {
                // An absent result is never presented as a returned zero.
                display.line(3 + index * 2 + pass, "NOT COMPLETED");
            }
        }
    }
    display.hex(11, "COMMAND STATUS", report.after.command_status);
    display.hex(12, "BEFORE IRQ EN", report.before.irq_enable);
    display.hex(13, "AFTER IRQ EN", report.after.irq_enable);
    display.hex(14, "AFTER IRQ ST", report.after.irq_status);
    display.hex(15, "AFTER IDLE", report.after.idle);
    display.hex(16, "AFTER ENABLE", report.after.enable);
    display.hex(17, "AFTER WAIT", report.after.wait);
    display.hex(18, "AFTER CONTROL", report.after.control);
    display.hex(19, "OTHER IDLE", report.after.other_idle);
    display.hex(20, "OTHER ENABLE", report.after.other_enable);
    display.hex(21, "OTHER WAIT", report.after.other_wait);
    display.hex(22, "OTHER CONTROL", report.after.other_control);
    if let Some(error) = report.error {
        display.hex(23, "ERROR", error.code());
    }
    if let Some(resource) = report.failed_resource {
        display.hex(24, "FAILED ADDRESS", resource.address());
    }
    if let Some(pass) = report.failed_pass {
        display.hex(25, "FAILED PASS", u32::from(pass) + 1);
    }
    display.line(26, "NO MODEM START");
    display.line(27, "NO POWER VOTES");
    display.hex(28, "WORK EACH READ", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("RPMH-RESOURCE-READS|RESULT|{:?}", report);
    ResourceReadsProbe
}

struct ResourceReadsProbe;
impl Handler for ResourceReadsProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
