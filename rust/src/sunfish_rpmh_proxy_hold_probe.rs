#![no_std]
#![no_main]
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_rpmh_proxy_hold::hold_proxy_resources;
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65536;
fn sample(display: &Display, row: usize, label: &str, value: Option<u32>) {
    if let Some(value) = value {
        display.hex(row, label, value);
    } else {
        display.line(row, "NOT COMPLETED");
    }
}

#[protection_domain]
fn init() -> ProxyHoldProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    let r = hold_proxy_resources(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |phase| {
            display.clear();
            display.line(0, "RPMH COMBINED HOLD");
            display.line(1, phase.label());
            display.line(26, "NO AOP ON OR RESET");
            display.line(27, "NO AUTO UNVOTE");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    display.clear();
    display.line(0, "RPMH COMBINED HOLD");
    display.line(
        1,
        if r.passed() {
            "COMBINED HOLD PASS"
        } else {
            "COMBINED HOLD FAIL"
        },
    );
    if let Some(error) = r.error {
        display.hex(2, "ERROR", error.code());
    } else {
        display.line(2, "RESOURCE TEST ONLY");
    }
    display.hex(3, "PHASE", r.phase.code());
    for (row, label, value) in [
        (4, "CX BEFORE", r.rails[0][0][0]),
        (5, "CX FINAL ONE", r.rails[3][0][0]),
        (6, "CX FINAL TWO", r.rails[3][1][0]),
        (7, "MSS FINAL ONE", r.rails[3][0][1]),
        (8, "MSS FINAL TWO", r.rails[3][1][1]),
        (9, "MX FINAL", r.rails[3][0][2]),
        (10, "XO FINAL ONE", r.rails[3][0][3]),
        (11, "XO FINAL TWO", r.rails[3][1][3]),
        (12, "MC0 FINAL ONE", r.bus[0][0]),
        (13, "MC0 FINAL TWO", r.bus[1][0]),
        (14, "ACV FINAL", r.bus[0][1]),
        (15, "CE0 FINAL", r.bus[0][2]),
        (16, "SN11 FINAL", r.bus[0][3]),
        (17, "SN0 FINAL", r.bus[0][4]),
        (18, "SH0 FINAL", r.bus[0][5]),
    ] {
        sample(&display, row, label, value);
    }
    display.hex(19, "WRITES SENT MASK", r.submitted_mask);
    display.hex(20, "WRITE ACK MASK", r.acknowledged_mask);
    display.hex(21, "REQUESTS", r.requests);
    display.hex(22, "HANDOFF CHECKS", r.checks);
    display.hex(23, "BUS DIFF MASK", r.bus_difference_mask());
    if let Some(request) = r.failed_request {
        display.hex(24, "FAILED REQUEST", request);
    } else if let Some(state) = r.state {
        display.hex(24, "IRQ STATUS", state.irq_status);
    }
    if let Some(error) = r.controller_error {
        display.hex(25, "CTRL ERROR", error.code());
    } else if let Some(error) = r.handoff_error {
        display.hex(25, "GUARD ERROR", error.code());
    } else if let Some(error) = r.mc0_error {
        display.hex(25, "MC0 ERROR", error.code());
    } else if let Some(state) = r.state {
        display.hex(25, "TCS ENABLE", state.enable);
    }
    display.line(26, "NO AOP ON OR RESET");
    display.line(27, "NO AUTO UNVOTE");
    display.hex(28, "WORK EACH", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("PROXY-HOLD|RESULT|{:X?}", r);
    ProxyHoldProbe
}
struct ProxyHoldProbe;
impl Handler for ProxyHoldProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
