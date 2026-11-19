#![no_std]
#![no_main]

mod pixel_probe_display;
mod pixel_rpmh_handoff;

use personal_os_rust_server::qcom_rpmh_handoff::capture_stable;
use pixel_probe_display::Display;
use pixel_rpmh_handoff::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

#[protection_domain]
fn init() -> HandoffProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "RPMH TCS HANDOFF");
    display.line(28, "NO VOTES OR MODEM START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    let mut controller = unsafe {
        Controller::new(
            memory_region_symbol!(apps_rsc_state: *mut u32)
                .as_ptr()
                .cast_const(),
        )
    };
    match capture_stable(&mut controller, 3) {
        Ok(snapshot) => {
            display.line(1, "READ ONLY STABLE");
            for (index, labels) in [
                ["A0 EN WAIT", "A0 CONTROL", "A0 IDLE"],
                ["A1 EN WAIT", "A1 CONTROL", "A1 IDLE"],
                ["S2 EN WAIT", "S2 CONTROL", "S2 IDLE"],
                ["S3 EN WAIT", "S3 CONTROL", "S3 IDLE"],
                ["S4 EN WAIT", "S4 CONTROL", "S4 IDLE"],
                ["W5 EN WAIT", "W5 CONTROL", "W5 IDLE"],
                ["W6 EN WAIT", "W6 CONTROL", "W6 IDLE"],
                ["W7 EN WAIT", "W7 CONTROL", "W7 IDLE"],
            ]
            .into_iter()
            .enumerate()
            {
                let bank = snapshot.banks[index];
                display.hex(2 + index * 3, labels[0], (bank.enable << 16) | bank.wait);
                display.hex(3 + index * 3, labels[1], bank.control);
                display.hex(4 + index * 3, labels[2], bank.idle);
            }
            display.hex(26, "RSC CONFIG", snapshot.config);
            display.hex(27, "IRQ STATUS", snapshot.irq_status);
            display.flush();
            debug_println!(
                "RSC-HANDOFF|STABLE|irq_enable={:08X} irq_status={:08X}",
                snapshot.irq_enable,
                snapshot.irq_status
            );
            for (index, bank) in snapshot.banks.iter().enumerate() {
                debug_println!(
                    "RSC-HANDOFF|BANK {}|enable={:08X} wait={:08X} control={:08X} idle={:08X}",
                    index,
                    bank.enable,
                    bank.wait,
                    bank.control,
                    bank.idle
                );
                for (slot, command) in bank.commands.iter().enumerate() {
                    if let Some(command) = command {
                        debug_println!("RSC-HANDOFF|CMD {} {}|{:?}", index, slot, command);
                    }
                }
            }
        }
        Err(error) => {
            display.line(1, "SNAPSHOT REFUSED");
            display.hex(2, "SNAPSHOT ERROR", error.code());
            debug_println!("RSC-HANDOFF|REFUSED|{:?}", error);
        }
    }
    display.flush();
    HandoffProbe
}

struct HandoffProbe;
impl Handler for HandoffProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
