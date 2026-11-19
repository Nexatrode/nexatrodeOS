//! Fixed ON notification using the proven bounded link/packet/close sequence.
//! No automatic OFF, retry, RX clearing, resources, PAS operation or reset.
use crate::pixel_aop_mailbox;
use crate::pixel_probe_display;
use personal_os_rust_server::qcom_qmp::Descriptor;
use personal_os_rust_server::qcom_qmp_link::{LinkHandshake, Stage, Step};
use personal_os_rust_server::qcom_qmp_tx::{OnNotification, TransferProgress};
use pixel_aop_mailbox::{Mailbox, Snapshot};
use pixel_probe_display::Display;

const MSGRAM_BYTES: u32 = 4096;
const POLLS_PER_STAGE: u32 = 65_536; // Work bound, not elapsed milliseconds.

/// Mask bits: link acknowledged, channel open, ON published, consumed, closed.
/// Consumption is transport evidence only, not semantic acceptance or readiness.
pub struct Probe<'a> {
    display: &'a Display,
    pub mailbox: &'a Mailbox,
    pub mask: u32,
    pub last: Snapshot,
    pub status: &'static str,
    pub notifies: u32,
}

impl<'a> Probe<'a> {
    pub fn new(display: &'a Display, mailbox: &'a Mailbox) -> Self {
        Self {
            display,
            mailbox,
            mask: 0,
            notifies: 0,
            status: "AOP READ START",
            last: Snapshot {
                descriptor: Descriptor::from_words(&[0; 16]),
                receive_length: u32::MAX,
                transmit_length: u32::MAX,
            },
        }
    }

    fn status(&mut self, status: &'static str) {
        self.status = status;
        self.display.clear();
        self.display.line(0, "MODEM PREPARE");
        self.display.line(1, status);
        self.display.hex(3, "AOP PHASE MASK", self.mask);
        self.display.hex(4, "AOP NOTIFIES", self.notifies);
        self.display.line(26, "NO SECURE CALL");
        self.display.line(27, "NO SEGMENT FILES");
        self.display.line(28, "NO RESET RELEASE");
        self.display.line(29, "RESTART WHEN DONE");
        self.display.flush();
    }

    fn read(&mut self) -> Result<Snapshot, u32> {
        match self.mailbox.snapshot() {
            Ok(snapshot) => {
                self.last = snapshot;
                Ok(snapshot)
            }
            Err(error) => {
                self.last = Snapshot {
                    descriptor: error.descriptor,
                    receive_length: u32::MAX,
                    transmit_length: u32::MAX,
                };
                Err(error.code)
            }
        }
    }

    fn notify(&mut self, writes: [Option<personal_os_rust_server::qcom_qmp_link::ApWrite>; 2]) {
        self.mailbox.notify(writes);
        self.notifies += 1;
    }
}

fn delay() {
    for _ in 0..4096 {
        unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
    }
}

pub fn run(probe: &mut Probe<'_>) -> Result<(), u32> {
    let snapshot = probe.read()?;
    let mut link = LinkHandshake::open(
        snapshot.descriptor,
        MSGRAM_BYTES,
        POLLS_PER_STAGE,
        snapshot.receive_length,
        snapshot.transmit_length,
    )
    .map_err(|e| e.code())?;
    probe.status("LINK START");
    loop {
        let snapshot = probe.read()?;
        let before = link.stage();
        let step = link
            .advance(
                snapshot.descriptor,
                snapshot.receive_length,
                snapshot.transmit_length,
            )
            .map_err(|e| e.code())?;
        match step {
            Step::Notify(writes) => {
                // Keep presentation outside the snapshot-to-write interval.
                probe.notify(writes);
                if link.stage() != before {
                    match link.stage() {
                        Stage::LinkAck => probe.status("WAIT LINK ACK"),
                        Stage::ChannelAck => {
                            probe.mask |= 1;
                            probe.status("WAIT CHANNEL OPEN");
                        }
                        _ => return Err(0x32),
                    }
                }
            }
            Step::Wait => delay(),
            Step::Connected => {
                probe.mask |= 2;
                break;
            }
            Step::Complete => return Err(0x32),
        }
    }

    probe.status("AOP ON WRITE START");
    let snapshot = probe.read()?;
    let mut transfer = OnNotification::new(
        snapshot.descriptor,
        MSGRAM_BYTES,
        POLLS_PER_STAGE,
        snapshot.receive_length,
        snapshot.transmit_length,
    )
    .map_err(|e| e.code())?;
    let packet = transfer
        .start(
            snapshot.descriptor,
            snapshot.receive_length,
            snapshot.transmit_length,
        )
        .map_err(|e| e.code())?;
    probe.mailbox.publish_on(&packet)?;
    probe.notifies += 1;
    probe.mask |= 4;
    probe.read()?;
    probe.status("WAIT ON CONSUMPTION");
    loop {
        let snapshot = probe.read()?;
        match transfer
            .poll(
                snapshot.descriptor,
                snapshot.receive_length,
                snapshot.transmit_length,
            )
            .map_err(|e| e.code())?
        {
            TransferProgress::Wait => delay(),
            TransferProgress::Consumed => {
                probe.mask |= 8;
                break;
            }
        }
    }

    probe.status("CHANNEL CLOSE START");
    let snapshot = probe.read()?;
    let step = link
        .disconnect(
            snapshot.descriptor,
            snapshot.receive_length,
            snapshot.transmit_length,
        )
        .map_err(|e| e.code())?;
    match step {
        Step::Notify(writes) => probe.notify(writes),
        _ => return Err(0x32),
    }
    loop {
        let snapshot = probe.read()?;
        let step = link
            .advance(
                snapshot.descriptor,
                snapshot.receive_length,
                snapshot.transmit_length,
            )
            .map_err(|e| e.code())?;
        match step {
            Step::Notify(writes) => probe.notify(writes),
            Step::Wait => delay(),
            Step::Complete => {
                probe.mask |= 16;
                probe.status("AOP ON TRANSPORT PASS");
                return Ok(());
            }
            Step::Connected => return Err(0x32),
        }
    }
}
