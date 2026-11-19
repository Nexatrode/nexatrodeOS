#![no_std]
#![no_main]
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_rpmh_bus_hold::{Phase, hold_mc0};
use personal_os_rust_server::qcom_rpmh_read::MC0_REASSERT_WORD;
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
fn init() -> Mc0HoldProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    let r = hold_mc0(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |phase| {
            display.clear();
            display.line(0, "RPMH MC0 HOLD");
            display.line(
                1,
                match phase {
                    Phase::BeforeReads => "CHECK SIX START VALUES",
                    Phase::Mc0Write => "REASSERT OBSERVED MC0",
                    Phase::AfterCapture => "CHECK BANK PRESERVATION",
                    Phase::AfterReads => "CHECK SIX READBACKS",
                    Phase::Held => "MC0 REQUEST HELD",
                },
            );
            display.hex(3, "FIXED MC0 WORD", MC0_REASSERT_WORD);
            display.line(26, "NO PAS OR RESET");
            display.line(27, "NO AUTO UNVOTE");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    display.clear();
    display.line(0, "RPMH MC0 HOLD");
    display.line(
        1,
        if r.passed() {
            "MC0 HOLD PASS"
        } else {
            "MC0 HOLD FAIL"
        },
    );
    if let Some(error) = r.error {
        display.hex(2, "ERROR", error.code());
    } else {
        display.line(2, "ACK AND READBACKS OK");
    }
    for (index, labels) in [
        ["MC0 BEFORE", "MC0 AFTER"],
        ["ACV BEFORE", "ACV AFTER"],
        ["CE0 BEFORE", "CE0 AFTER"],
        ["SN11 BEFORE", "SN11 AFTER"],
        ["SN0 BEFORE", "SN0 AFTER"],
        ["SH0 BEFORE", "SH0 AFTER"],
    ]
    .into_iter()
    .enumerate()
    {
        sample(
            &display,
            3 + index * 2,
            labels[0],
            r.before.responses[0][index],
        );
        sample(
            &display,
            4 + index * 2,
            labels[1],
            r.after.and_then(|after| after.responses[0][index]),
        );
    }
    sample(
        &display,
        15,
        "MC0 AFTER TWO",
        r.after.and_then(|after| after.responses[1][0]),
    );
    display.hex(16, "WRITE SUBMITTED", u32::from(r.write_submitted));
    display.hex(17, "WRITE ACK", u32::from(r.write_acknowledged));
    display.hex(18, "HANDOFF CHECKS", r.preserved_checks());
    if let Some(state) = r.state {
        display.hex(19, "IRQ STATUS", state.irq_status);
        display.hex(20, "TCS IDLE", state.idle);
        display.hex(21, "TCS ENABLE", state.enable);
        display.hex(22, "TCS CONTROL", state.control);
        if r.controller_error.is_none() && r.handoff_error.is_none() {
            display.hex(23, "IRQ ENABLE", state.irq_enable);
        }
    }
    if let Some(error) = r.controller_error {
        display.hex(23, "CTRL ERROR", error.code());
    } else if let Some(error) = r.handoff_error {
        display.hex(23, "GUARD ERROR", error.code());
    }
    display.line(
        24,
        if r.passed() {
            "MC0 REQUEST HELD"
        } else if r.write_submitted {
            "MC0 MAY BE HELD"
        } else {
            "NO RESOURCE WRITE SENT"
        },
    );
    if r.passed() {
        display.line(25, "OTHER FIVE UNCHANGED");
    } else {
        let mask = (0..6).fold(0, |mask, index| {
            mask | (((0..2).any(|pass| {
                r.before.responses[pass][index]
                    != Some(personal_os_rust_server::qcom_rpmh_bus_hold::EXPECTED_READS[index])
            }) as u32)
                << index)
        });
        display.hex(25, "START DIFF MASK", mask);
    }
    display.line(26, "NO PAS OR RESET");
    display.line(27, "NO AUTO UNVOTE");
    display.hex(28, "WORK EACH", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!(
        "MC0-HOLD|RESULT|error={:?} submitted={} ack={} before={:X?} after={:X?}",
        r.error,
        r.write_submitted,
        r.write_acknowledged,
        r.before.responses,
        r.after.map(|after| after.responses)
    );
    Mc0HoldProbe
}
struct Mc0HoldProbe;
impl Handler for Mc0HoldProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
