#![no_std]
#![no_main]

mod pixel_probe_display;
mod pixel_rpmh_controller;

use personal_os_rust_server::qcom_rpmh_power::{Phase, ResourceHoldPhase, hold_modem_resources};
use pixel_probe_display::Display;
use pixel_rpmh_controller::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65_536;

fn sample(display: &Display, row: usize, label: &str, value: Option<u32>) {
    if let Some(v) = value {
        display.hex(row, label, v);
    } else {
        display.line(row, "NOT COMPLETED");
    }
}

#[protection_domain]
fn init() -> ResourcesHoldProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    let r = hold_modem_resources(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |phase| {
            display.clear();
            display.line(0, "RPMH RESOURCE HOLD");
            display.line(
                1,
                match phase {
                    ResourceHoldPhase::Cx(Phase::BeforeReads) => "CHECK STARTING VALUES",
                    ResourceHoldPhase::Cx(Phase::CxVote) => "CX TURBO REQUEST",
                    ResourceHoldPhase::Cx(_) => "CHECK CX REQUEST",
                    ResourceHoldPhase::MssVote => "MSS TURBO REQUEST",
                    ResourceHoldPhase::MssReads => "CHECK MSS REQUEST",
                    ResourceHoldPhase::XoVote => "XO ON REQUEST",
                    ResourceHoldPhase::XoReads => "CHECK XO REQUEST",
                    ResourceHoldPhase::Held => "REQUESTS HELD",
                },
            );
            display.line(2, "CX6 MSS7 XO3 ONLY");
            display.line(26, "NO PAS OR RESET");
            display.line(27, "NO AUTO UNVOTE");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    display.clear();
    display.line(0, "RPMH RESOURCE HOLD");
    display.line(
        1,
        if r.passed() {
            "RESOURCE HOLD PASS"
        } else {
            "RESOURCE HOLD FAIL"
        },
    );
    if let Some(e) = r.error {
        display.hex(2, "ERROR", e.code());
    } else {
        display.line(2, "ACK AND READBACKS OK");
    }
    for (i, label) in ["CX ACK", "MSS ACK", "XO ACK"].into_iter().enumerate() {
        display.hex(3 + i, label, u32::from(r.completed[i]));
    }
    sample(&display, 6, "CX BEFORE", r.cx.before.responses[0][0]);
    sample(
        &display,
        7,
        "CX FINAL FIRST",
        r.after_xo.and_then(|a| a.responses[0][0]),
    );
    sample(
        &display,
        8,
        "CX FINAL SECOND",
        r.after_xo.and_then(|a| a.responses[1][0]),
    );
    sample(&display, 9, "MSS BEFORE", r.cx.before.responses[0][1]);
    sample(
        &display,
        10,
        "MSS AFTER VOTE",
        r.after_mss.and_then(|a| a.responses[0][1]),
    );
    sample(
        &display,
        11,
        "MSS FINAL FIRST",
        r.after_xo.and_then(|a| a.responses[0][1]),
    );
    sample(
        &display,
        12,
        "MSS FINAL 2",
        r.after_xo.and_then(|a| a.responses[1][1]),
    );
    sample(&display, 13, "MX BEFORE", r.cx.before.responses[0][2]);
    sample(
        &display,
        14,
        "MX FINAL FIRST",
        r.after_xo.and_then(|a| a.responses[0][2]),
    );
    sample(
        &display,
        15,
        "MX FINAL SECOND",
        r.after_xo.and_then(|a| a.responses[1][2]),
    );
    sample(
        &display,
        16,
        "XO FINAL FIRST",
        r.after_xo.and_then(|a| a.responses[0][3]),
    );
    sample(
        &display,
        17,
        "XO FINAL SECOND",
        r.after_xo.and_then(|a| a.responses[1][3]),
    );
    display.hex(18, "IRQ STATUS", r.state.irq_status);
    display.hex(19, "TCS IDLE", r.state.idle);
    display.hex(20, "TCS ENABLE", r.state.enable);
    display.hex(21, "TCS CONTROL", r.state.control);
    if let Some(e) = r.controller_error {
        display.hex(22, "CTRL ERROR", e.code());
    } else {
        display.hex(22, "IRQ ENABLE", r.state.irq_enable);
    }
    if let Some(e) = r.cx.error {
        display.hex(23, "CX ERROR", e.code());
    } else {
        display.hex(23, "OTHER IDLE", r.state.other_idle);
    }
    let mask = r
        .submitted
        .iter()
        .enumerate()
        .fold(0, |mask, (i, s)| mask | (u32::from(*s) << i));
    display.hex(24, "SUBMITTED MASK", mask);
    display.line(
        25,
        if r.passed() {
            "REQUESTS HELD"
        } else if mask != 0 {
            "REQUESTS MAY BE HELD"
        } else {
            "NO REQUESTS SENT"
        },
    );
    display.line(26, "NO PAS OR RESET");
    display.line(27, "NO AUTO UNVOTE");
    display.hex(28, "WORK EACH", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("RPMH-RESOURCES-HOLD|RESULT|{:?}", r);
    ResourcesHoldProbe
}

struct ResourcesHoldProbe;
impl Handler for ResourcesHoldProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
