#![no_std]
#![no_main]

mod pixel_probe_display;
mod pixel_rpmh_controller;

use personal_os_rust_server::qcom_rpmh_power::{Phase, hold_cx_turbo};
use pixel_probe_display::Display;
use pixel_rpmh_controller::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65_536;

fn sample(display: &Display, row: usize, label: &str, value: Option<u32>) {
    if let Some(value) = value {
        display.hex(row, label, value);
    } else {
        display.line(row, "NOT COMPLETED");
    }
}

#[protection_domain]
fn init() -> CxTurboHoldProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc: *mut u32).as_ptr()) };
    let report = hold_cx_turbo(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |phase| {
            display.clear();
            display.line(0, "RPMH CX TURBO HOLD");
            display.line(
                1,
                match phase {
                    Phase::BeforeReads => "CHECK STARTING VALUES",
                    Phase::CxVote => "CX TURBO REQUEST",
                    Phase::AfterReads => "CHECK AFTER REQUEST",
                    Phase::Held => "CX REQUEST HELD",
                },
            );
            display.line(2, "CX SELECTOR 6 ONLY");
            display.line(26, "NO MODEM START");
            display.line(27, "NO AUTO UNVOTE");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    display.clear();
    display.line(0, "RPMH CX TURBO HOLD");
    display.line(
        1,
        if report.passed() {
            "CX TURBO HOLD PASS"
        } else {
            "CX TURBO HOLD FAIL"
        },
    );
    if let Some(error) = report.error {
        display.hex(2, "ERROR", error.code());
    } else {
        display.line(2, "ACK AND TWO READBACKS OK");
    }
    sample(&display, 3, "CX BEFORE", report.before.responses[0][0]);
    sample(
        &display,
        4,
        "CX AFTER FIRST",
        report.after.and_then(|a| a.responses[0][0]),
    );
    sample(
        &display,
        5,
        "CX AFTER SECOND",
        report.after.and_then(|a| a.responses[1][0]),
    );
    for (index, row, labels) in [
        (1, 6, ["MSS BEFORE", "MSS AFTER FIRST", "MSS AFTER 2"]),
        (2, 9, ["MX BEFORE", "MX AFTER FIRST", "MX AFTER 2"]),
        (3, 12, ["XO BEFORE", "XO AFTER FIRST", "XO AFTER 2"]),
    ] {
        sample(&display, row, labels[0], report.before.responses[0][index]);
        sample(
            &display,
            row + 1,
            labels[1],
            report.after.and_then(|a| a.responses[0][index]),
        );
        sample(
            &display,
            row + 2,
            labels[2],
            report.after.and_then(|a| a.responses[1][index]),
        );
    }
    display.hex(15, "WRITE SUBMITTED", u32::from(report.write_submitted));
    display.hex(16, "WRITE ACK", u32::from(report.write_completed));
    display.hex(17, "IRQ STATUS", report.state.irq_status);
    display.hex(18, "TCS IDLE", report.state.idle);
    display.hex(19, "TCS ENABLE", report.state.enable);
    display.hex(20, "TCS WAIT", report.state.wait);
    display.hex(21, "TCS CONTROL", report.state.control);
    display.hex(22, "OTHER IDLE", report.state.other_idle);
    if let Some(error) = report.controller_error {
        display.hex(23, "CTRL ERROR", error.code());
    } else {
        display.hex(23, "IRQ ENABLE", report.state.irq_enable);
    }
    if let Some(resource) = report
        .after
        .and_then(|a| a.failed_resource)
        .or(report.before.failed_resource)
    {
        display.hex(24, "FAILED ADDRESS", resource.address());
    }
    display.line(
        25,
        if report.passed() {
            "CX TURBO REQUEST HELD"
        } else if report.write_submitted {
            "CX VOTE STATE UNKNOWN"
        } else {
            "VOTE NOT SENT"
        },
    );
    display.line(26, "NO MODEM START");
    display.line(27, "NO AUTO UNVOTE");
    display.hex(28, "WORK LIMIT", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("RPMH-CX-TURBO-HOLD|RESULT|{:?}", report);
    CxTurboHoldProbe
}

struct CxTurboHoldProbe;
impl Handler for CxTurboHoldProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
