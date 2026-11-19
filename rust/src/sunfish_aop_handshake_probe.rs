#![no_std]
#![no_main]

mod pixel_aop_mailbox;
mod pixel_probe_display;

use personal_os_rust_server::qcom_qmp_link::{LinkHandshake, Stage, Step};
use pixel_aop_mailbox::{Mailbox, Snapshot};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const MSGRAM_BYTES: u32 = 4096;
// These are finite work limits, not milliseconds. EL0 counter access is not
// available on this Pixel configuration. No IRQ or system-wide timer is used.
const POLLS_PER_STAGE: u32 = 65_536;

fn report(display: &Display, stage: Stage, error: Option<u32>, snapshot: Snapshot, notifies: u32) {
    display.clear();
    display.line(0, "AOP HANDSHAKE");
    display.line(
        1,
        if error.is_some() {
            "HANDSHAKE FAIL"
        } else {
            match stage {
                Stage::Start => "CHECK HANDOFF",
                Stage::LinkAck => "WAIT LINK ACK",
                Stage::ChannelAck => "WAIT CHANNEL OPEN",
                Stage::Connected => "CHANNEL OPEN",
                Stage::DisconnectAck => "WAIT CHANNEL CLOSE",
                Stage::Complete => "HANDSHAKE PASS",
                Stage::Failed => "HANDSHAKE FAIL",
            }
        },
    );
    if let Some(code) = error {
        display.hex(2, "ERROR", code);
    } else {
        display.line(2, "NO PAYLOAD SENT");
    }
    display.line(
        3,
        match stage {
            Stage::ChannelAck | Stage::Connected | Stage::DisconnectAck | Stage::Complete => {
                "LINK PASS"
            }
            _ => "LINK NOT VERIFIED",
        },
    );
    display.line(
        4,
        match stage {
            Stage::Connected | Stage::DisconnectAck | Stage::Complete => "OPEN PASS",
            _ => "OPEN NOT VERIFIED",
        },
    );
    display.line(
        5,
        if stage == Stage::Complete {
            "CLOSE PASS"
        } else {
            "CLOSE NOT VERIFIED"
        },
    );
    let d = snapshot.descriptor;
    display.hex(7, "REMOTE LINK", d.remote.link);
    display.hex(8, "REMOTE ACK", d.remote.link_ack);
    display.hex(9, "AP LINK", d.local.link);
    display.hex(10, "AP ACK", d.local.link_ack);
    display.hex(11, "REMOTE CH", d.remote.channel);
    display.hex(12, "REMOTE CH ACK", d.remote.channel_ack);
    display.hex(13, "AP CH", d.local.channel);
    display.hex(14, "AP CH ACK", d.local.channel_ack);
    display.hex(16, "RX BYTES", snapshot.receive_length);
    display.hex(17, "TX BYTES", snapshot.transmit_length);
    display.hex(18, "NOTIFIES", notifies);
    display.hex(19, "WORK LIMIT", POLLS_PER_STAGE);
    display.line(21, "NO MODEM COMMAND");
    display.line(23, "RESTART WHEN DONE");
    display.flush();
    debug_println!(
        "AOP-QMP|HANDSHAKE|stage={:?} error={:?} notifies={}",
        stage,
        error,
        notifies
    );
    debug_println!(
        "AOP-QMP|STATE|remote={:08X}/{:08X}/{:08X}/{:08X} local={:08X}/{:08X}/{:08X}/{:08X}",
        d.remote.link,
        d.remote.link_ack,
        d.remote.channel,
        d.remote.channel_ack,
        d.local.link,
        d.local.link_ack,
        d.local.channel,
        d.local.channel_ack
    );
    debug_println!(
        "AOP-QMP|LENGTHS|rx={} tx={}",
        snapshot.receive_length,
        snapshot.transmit_length
    );
}

fn run(display: &Display, mailbox: &Mailbox) {
    let mut snapshot = match mailbox.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            // A sentinel length means the length was not read or not stable.
            report(
                display,
                Stage::Start,
                Some(error.code),
                Snapshot {
                    descriptor: error.descriptor,
                    receive_length: u32::MAX,
                    transmit_length: u32::MAX,
                },
                0,
            );
            return;
        }
    };
    let mut handshake = match LinkHandshake::new(
        snapshot.descriptor,
        MSGRAM_BYTES,
        POLLS_PER_STAGE,
        snapshot.receive_length,
        snapshot.transmit_length,
    ) {
        Ok(handshake) => handshake,
        Err(error) => {
            report(display, Stage::Start, Some(error.code()), snapshot, 0);
            return;
        }
    };
    let mut notifies = 0;
    loop {
        let stage_before = handshake.stage();
        let step = match handshake.advance(
            snapshot.descriptor,
            snapshot.receive_length,
            snapshot.transmit_length,
        ) {
            Ok(step) => step,
            Err(error) => {
                report(
                    display,
                    stage_before,
                    Some(error.code()),
                    snapshot,
                    notifies,
                );
                return;
            }
        };
        // Render only when changing stage. This also leaves a visible marker
        // before the first descriptor write or doorbell access can fault.
        if handshake.stage() != stage_before {
            report(display, handshake.stage(), None, snapshot, notifies);
        }
        match step {
            Step::Notify(writes) => {
                mailbox.notify(writes);
                notifies += 1;
            }
            Step::Wait => {
                for _ in 0..4096 {
                    unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
                }
            }
            Step::Complete => return,
            Step::Connected => {
                report(display, Stage::Connected, Some(0x13), snapshot, notifies);
                return;
            }
        }
        snapshot = match mailbox.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                report(
                    display,
                    handshake.stage(),
                    Some(error.code),
                    Snapshot {
                        descriptor: error.descriptor,
                        receive_length: u32::MAX,
                        transmit_length: u32::MAX,
                    },
                    notifies,
                );
                return;
            }
        };
    }
}

#[protection_domain]
fn init() -> AopHandshakeProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "AOP HANDSHAKE");
    display.line(1, "READ START");
    display.line(2, "NO PAYLOAD SENT");
    display.flush();
    let mailbox = unsafe {
        Mailbox::new(
            memory_region_symbol!(aop_msgram: *mut u32).as_ptr(),
            memory_region_symbol!(aop_doorbell: *mut u32).as_ptr(),
        )
    };
    run(&display, &mailbox);
    debug_println!("AOP-QMP|DONE|hold screen until physical restart");
    AopHandshakeProbe
}

struct AopHandshakeProbe;

impl Handler for AopHandshakeProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
