#![no_std]
#![no_main]
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_rpmh_bus::read_bcm_series;
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65536;

#[protection_domain]
fn init() -> BcmReadsProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    display.clear();
    display.line(0, "RPMH BCM READS");
    display.line(1, "CHECK FULL HANDOFF");
    display.line(27, "NO VOTES OR MODEM START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    let result = read_bcm_series(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |pass, resource| {
            display.clear();
            display.line(0, "RPMH BCM READS");
            display.line(1, "RESOURCE READ ONLY");
            display.hex(2, "ADDRESS", resource.address());
            display.hex(3, "PASS", u32::from(pass) + 1);
            display.line(27, "NO VOTES OR MODEM START");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    display.clear();
    display.line(0, "RPMH BCM READS");
    display.line(
        1,
        if result.passed() {
            "BCM READ PASS"
        } else {
            "BCM READ FAIL"
        },
    );
    display.hex(2, "READS COMPLETED", u32::from(result.completed));
    for (index, labels) in [
        ["MC0 FIRST", "MC0 SECOND"],
        ["ACV FIRST", "ACV SECOND"],
        ["CE0 FIRST", "CE0 SECOND"],
        ["SN11 FIRST", "SN11 SECOND"],
        ["SN0 FIRST", "SN0 SECOND"],
        ["SH0 FIRST", "SH0 SECOND"],
    ]
    .into_iter()
    .enumerate()
    {
        for pass in 0..2 {
            let row = 3 + index * 2 + pass;
            if let Some(value) = result.responses[pass][index] {
                display.hex(row, labels[pass], value);
            } else {
                display.line(row, "NOT COMPLETED");
            }
        }
    }
    display.hex(15, "SUBMITTED", u32::from(result.submitted));
    display.hex(16, "HANDOFF CHECKS", u32::from(result.handoff_checks));
    if let Some(state) = result.state {
        display.hex(17, "IRQ STATUS", state.irq_status);
        display.hex(18, "TCS IDLE", state.idle);
        display.hex(19, "TCS ENABLE", state.enable);
        display.hex(20, "TCS CONTROL", state.control);
        display.hex(21, "IRQ ENABLE", state.irq_enable);
    } else {
        display.line(17, "STATE NOT SAMPLED");
    }
    if let Some(error) = result.error {
        display.hex(22, "ERROR", error.code());
    } else {
        display.line(22, "BANKS PRESERVED");
    }
    if let Some(error) = result.controller_error {
        display.hex(23, "CTRL ERROR", error.code());
    } else if let Some(error) = result.handoff_error {
        display.hex(23, "HANDOFF ERROR", error.code());
    }
    if let Some(resource) = result.failed_resource {
        display.hex(24, "FAILED ADDRESS", resource.address());
    }
    if let Some(pass) = result.failed_pass {
        display.hex(25, "FAILED PASS", u32::from(pass) + 1);
    }
    display.line(26, "RESOURCE WRITE BIT ZERO");
    display.line(27, "NO VOTES OR MODEM START");
    display.hex(28, "WORK EACH", WORK_LIMIT);
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("BCM-READS|RESULT|{:?}", result);
    BcmReadsProbe
}
struct BcmReadsProbe;
impl Handler for BcmReadsProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
