#![no_std]
#![no_main]
mod pixel_aop_mailbox;
mod pixel_aop_modem_on;
mod pixel_modem_prepare;
mod pixel_pas_metadata;
mod pixel_probe_display;
mod pixel_rpmh_bus;

use pixel_aop_mailbox::Mailbox;
use pixel_aop_modem_on::Probe;
use pixel_modem_prepare::{progress, run, sample};
use pixel_pas_metadata::Buffer;
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

static MDT: &[u8] = include_bytes!(env!("SUNFISH_MODEM_MDT_PATH"));

#[protection_domain]
fn init() -> ModemPrepareProbe {
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    progress(&d, "PREPARE START");
    let physical = *sel4_microkit::var!(pas_metadata_paddr:usize = 0) as u64;
    let mut buffer = unsafe { Buffer::new(memory_region_symbol!(pas_metadata:*mut u8).as_ptr()) };
    let mailbox = unsafe {
        Mailbox::new(
            memory_region_symbol!(aop_msgram:*mut u32).as_ptr(),
            memory_region_symbol!(aop_doorbell:*mut u32).as_ptr(),
        )
    };
    let mut aop = Probe::new(&d, &mailbox);
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    let mut prepared = None;
    let mut resources = None;
    let mut verified = false;
    let result = run(
        &d,
        &mut buffer,
        physical,
        MDT,
        &mut aop,
        &mut controller,
        &mut prepared,
        &mut resources,
        &mut verified,
    );
    d.clear();
    d.line(0, "MODEM PREPARE");
    d.line(
        1,
        if result.is_ok() {
            "PREPARE PASS"
        } else {
            "PREPARE FAIL"
        },
    );
    d.hex(2, "ERROR", result.err().unwrap_or(0));
    d.hex(3, "AOP PHASE MASK", aop.mask);
    d.hex(4, "AOP NOTIFIES", aop.notifies);
    if let Some(r) = resources.as_ref() {
        let c = &r.controller;
        d.hex(5, "RSC PHASE", c.phase.code());
        d.hex(6, "WRITE ACK MASK", c.acknowledged_mask);
        d.hex(7, "REQUESTS", c.requests);
        d.hex(8, "HANDOFF CHECKS", c.checks);
        // Keep the latest partial stage intact on failure, including the first
        // bus series if AOP ON changed the old baseline. Do not mix stages.
        let rails = if r.final_rails.iter().flatten().any(Option::is_some) {
            r.final_rails
        } else {
            c.rails
                .iter()
                .rev()
                .find(|s| s.iter().flatten().any(Option::is_some))
                .copied()
                .unwrap_or([[None; 4]; 2])
        };
        let bus = r
            .bus_stages
            .iter()
            .rev()
            .find(|s| s.iter().flatten().any(Option::is_some))
            .copied()
            .unwrap_or(if c.bus.iter().flatten().any(Option::is_some) {
                c.bus
            } else {
                c.initial_bus
            });
        for (i, label) in ["CX LATEST", "MSS LATEST", "MX LATEST", "XO LATEST"]
            .into_iter()
            .enumerate()
        {
            sample(&d, 9 + i, label, rails[0][i]);
        }
        for (i, label) in [
            "MC0 LATEST",
            "ACV LATEST",
            "CE0 LATEST",
            "SN11 LATEST",
            "SN0 LATEST",
            "SH0 LATEST",
        ]
        .into_iter()
        .enumerate()
        {
            sample(&d, 13 + i, label, bus[0][i]);
        }
        d.hex(22, "CTRL ERROR", c.controller_error.map_or(0, |e| e.code()));
        d.hex(
            23,
            "GUARD MC0 ERROR",
            c.handoff_error.map_or_else(
                || c.mc0_error.map_or(0, |e| e.code()),
                |e| 0x10000 | e.code(),
            ),
        );
    } else {
        d.line(5, "RSC NOT STARTED");
        d.line(6, aop.status);
    }
    d.hex(
        19,
        "MDT PHYS LO",
        prepared.map_or(0, |p| p.metadata_physical as u32),
    );
    d.hex(
        20,
        "MDT BYTES",
        prepared.map_or(0, |p| p.metadata_length as u32),
    );
    d.hex(21, "MDT VERIFIED", u32::from(verified));
    d.hex(24, "RX BYTES", aop.last.receive_length);
    d.hex(25, "TX BYTES", aop.last.transmit_length);
    d.line(26, "NO SECURE CALL");
    d.line(27, "NO SEGMENT FILES");
    d.line(28, "NO RESET RELEASE");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
    // Presentation completes before UART debugging, which may be unavailable.
    debug_println!(
        "MODEM-PREPARE|result={:X?} aop={:X}/{:X} metadata={:X?} verified={} resources={:X?}",
        result,
        aop.mask,
        aop.notifies,
        prepared,
        verified,
        resources
    );
    ModemPrepareProbe
}
struct ModemPrepareProbe;
impl Handler for ModemPrepareProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
