#![no_std]
#![no_main]

mod pixel_aop_mailbox;
mod pixel_probe_display;

use personal_os_rust_server::qcom_qmp::{Descriptor, MODEM_NOTIFICATION_BYTES};
use personal_os_rust_server::qcom_qmp_link::{LinkHandshake, Stage, Step};
use personal_os_rust_server::qcom_qmp_tx::{OffNotification, TransferProgress};
use pixel_aop_mailbox::{Mailbox, Snapshot};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const MSGRAM_BYTES: u32 = 4096;
const POLLS_PER_STAGE: u32 = 65_536; // Work bound, not elapsed milliseconds.

#[derive(Default)]
struct Progress {
    link: bool,
    open: bool,
    published: bool,
    consumed: bool,
    closed: bool,
}

struct Probe<'a> {
    display: &'a Display,
    mailbox: &'a Mailbox,
    progress: Progress,
    last: Snapshot,
    status: &'static str,
    notifies: u32,
}

impl Probe<'_> {
    fn report(&self, error: Option<u32>) {
        let d = self.last.descriptor;
        let p = &self.progress;
        self.display.clear();
        self.display.line(0, "AOP PACKET TEST");
        self.display.line(
            1,
            if error.is_some() {
                "PACKET FAIL"
            } else {
                self.status
            },
        );
        if let Some(code) = error {
            self.display.hex(2, "ERROR", code);
        } else {
            self.display.line(2, "MODEM OFF ONLY");
        }
        for (row, passed, yes, no) in [
            (3, p.link, "LINK PASS", "LINK WAIT"),
            (4, p.open, "OPEN PASS", "OPEN WAIT"),
            (5, p.published, "OFF PUBLISHED", "OFF NOT CONFIRMED"),
            (6, p.consumed, "OFF CONSUMED", "CONSUMPTION WAIT"),
            (7, p.closed, "CLOSE PASS", "CLOSE WAIT"),
        ] {
            self.display.line(row, if passed { yes } else { no });
        }
        if error.is_some() {
            self.display.line(8, self.status);
        }
        self.display.hex(9, "REMOTE LINK", d.remote.link);
        self.display.hex(10, "REMOTE ACK", d.remote.link_ack);
        self.display.hex(11, "AP LINK", d.local.link);
        self.display.hex(12, "AP ACK", d.local.link_ack);
        self.display.hex(13, "REMOTE CH", d.remote.channel);
        self.display.hex(14, "REMOTE CH ACK", d.remote.channel_ack);
        self.display.hex(15, "AP CH", d.local.channel);
        self.display.hex(16, "AP CH ACK", d.local.channel_ack);
        self.display.hex(18, "RX BYTES", self.last.receive_length);
        self.display.hex(19, "TX BYTES", self.last.transmit_length);
        self.display.hex(20, "NOTIFIES", self.notifies);
        self.display
            .hex(21, "PAYLOAD BYTES", MODEM_NOTIFICATION_BYTES as u32);
        self.display.hex(22, "WORK LIMIT", POLLS_PER_STAGE);
        self.display.line(24, "NO MODEM START");
        self.display.line(26, "RESTART WHEN DONE");
        self.display.flush();
        debug_println!(
            "AOP-QMP|PACKET|status={} error={:?} published={} consumed={} close={} notifies={}",
            self.status,
            error,
            p.published,
            p.consumed,
            p.closed,
            self.notifies
        );
        debug_println!(
            "AOP-QMP|STATE|remote={:08X}/{:08X}/{:08X}/{:08X} local={:08X}/{:08X}/{:08X}/{:08X} rx={} tx={}",
            d.remote.link,
            d.remote.link_ack,
            d.remote.channel,
            d.remote.channel_ack,
            d.local.link,
            d.local.link_ack,
            d.local.channel,
            d.local.channel_ack,
            self.last.receive_length,
            self.last.transmit_length
        );
    }

    fn status(&mut self, status: &'static str) {
        self.status = status;
        self.report(None);
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

fn run(probe: &mut Probe<'_>) -> Result<(), u32> {
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
                            probe.progress.link = true;
                            probe.status("WAIT CHANNEL OPEN");
                        }
                        _ => return Err(0x32),
                    }
                }
            }
            Step::Wait => delay(),
            Step::Connected => {
                probe.progress.open = true;
                break;
            }
            Step::Complete => return Err(0x32),
        }
    }

    probe.status("OFF WRITE START");
    let snapshot = probe.read()?;
    let mut transfer = OffNotification::new(
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
    probe.mailbox.publish_off(&packet)?;
    probe.notifies += 1;
    probe.progress.published = true;
    probe.read()?;
    probe.status("WAIT OFF CONSUMPTION");
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
                probe.progress.consumed = true;
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
                probe.progress.closed = true;
                probe.status("PACKET PASS");
                return Ok(());
            }
            Step::Connected => return Err(0x32),
        }
    }
}

#[protection_domain]
fn init() -> AopPacketProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "AOP PACKET TEST");
    display.line(1, "READ START");
    display.line(2, "MODEM OFF ONLY");
    display.flush();
    let mailbox = unsafe {
        Mailbox::new(
            memory_region_symbol!(aop_msgram: *mut u32).as_ptr(),
            memory_region_symbol!(aop_doorbell: *mut u32).as_ptr(),
        )
    };
    let mut probe = Probe {
        display: &display,
        mailbox: &mailbox,
        progress: Progress::default(),
        status: "READ START",
        notifies: 0,
        last: Snapshot {
            descriptor: Descriptor::from_words(&[0; 16]),
            receive_length: u32::MAX,
            transmit_length: u32::MAX,
        },
    };
    if let Err(error) = run(&mut probe) {
        probe.report(Some(error));
    }
    debug_println!("AOP-QMP|DONE|hold screen until physical restart");
    AopPacketProbe
}

struct AopPacketProbe;
impl Handler for AopPacketProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
