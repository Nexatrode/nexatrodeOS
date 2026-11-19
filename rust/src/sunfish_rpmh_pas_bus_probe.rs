#![no_std]
#![no_main]
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_rpmh_proxy_hold::hold_pas_bus_floor;
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
fn init() -> PasBusProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    let r = hold_pas_bus_floor(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |phase| {
            display.clear();
            display.line(0, "RPMH PAS BUS FLOOR");
            display.line(1, phase.label());
            display.line(2, "CE0 FIELDS SATURATE");
            display.line(26, "NO AOP ON OR RESET");
            display.line(27, "NO AUTO UNVOTE");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    let c = r.controller;
    display.clear();
    display.line(0, "RPMH PAS BUS FLOOR");
    display.line(
        1,
        if r.passed() {
            "BUS FLOOR HOLD PASS"
        } else {
            "BUS FLOOR HOLD FAIL"
        },
    );
    if let Some(error) = c.error {
        display.hex(2, "ERROR", error.code());
    } else {
        display.line(2, "CE0 FIELDS SATURATED");
    }
    display.hex(3, "PHASE", c.phase.code());
    let last_bus = r
        .bus_stages
        .iter()
        .rev()
        .find(|stage| stage.iter().flatten().any(Option::is_some))
        .copied()
        .unwrap_or([[None; 6]; 2]);
    for (i, name) in [
        "MC0 FINAL",
        "ACV FINAL",
        "CE0 FINAL",
        "SN11 FINAL",
        "SN0 FINAL",
        "SH0 FINAL",
    ]
    .into_iter()
    .enumerate()
    {
        // Keep a single latest partial stage on failure; never mix responses
        // from before and after different writes into one apparent snapshot.
        sample(&display, 4 + i, name, last_bus[0][i]);
    }
    for (row, label, value) in [
        (10, "MC0 SECOND", r.bus_stages[2][1][0]),
        (11, "CE0 SECOND", r.bus_stages[2][1][2]),
        (12, "SN11 SECOND", r.bus_stages[2][1][3]),
        (13, "SN0 SECOND", r.bus_stages[2][1][4]),
        (14, "CX FINAL", r.final_rails[0][0]),
        (15, "MSS FINAL", r.final_rails[0][1]),
        (16, "MX FINAL", r.final_rails[0][2]),
        (17, "XO FINAL", r.final_rails[0][3]),
    ] {
        sample(&display, row, label, value);
    }
    display.hex(18, "WRITE SENT MASK", c.submitted_mask);
    display.hex(19, "WRITE ACK MASK", c.acknowledged_mask);
    display.hex(20, "REQUESTS", c.requests);
    display.hex(21, "HANDOFF CHECKS", c.checks);
    display.hex(22, "CE0 PLAN SAT", 1);
    display.hex(23, "BUS DIFF MASK", r.bus_difference_mask());
    if let Some(request) = c.failed_request {
        display.hex(24, "FAILED REQUEST", request);
    } else if let Some(state) = c.state {
        display.hex(24, "IRQ STATUS", state.irq_status);
    }
    if let Some(error) = c.controller_error {
        display.hex(25, "CTRL ERROR", error.code());
    } else if let Some(error) = c.handoff_error {
        display.hex(25, "GUARD ERROR", error.code());
    } else if let Some(error) = c.mc0_error {
        display.hex(25, "MC0 ERROR", error.code());
    } else if let Some(state) = c.state {
        display.hex(25, "TCS ENABLE", state.enable);
    }
    display.line(26, "NO AOP ON OR RESET");
    display.line(27, "NO AUTO UNVOTE");
    display.hex(28, "WORK EACH", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("PAS-BUS-FLOOR|RESULT|{:X?}", r);
    PasBusProbe
}
struct PasBusProbe;
impl Handler for PasBusProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
