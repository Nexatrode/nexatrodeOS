#![no_std]
#![no_main]
mod pixel_aop_mailbox;
mod pixel_aop_modem_on;
mod pixel_modem_prepare;
mod pixel_pas_metadata;
mod pixel_pas_smc;
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_pas_init::{InitOnce, Report};
use personal_os_rust_server::qcom_pas_metadata::{self as metadata, Inspection, Prepared};
use personal_os_rust_server::qcom_rpmh_proxy_hold::FloorReport;
use pixel_aop_mailbox::Mailbox;
use pixel_aop_modem_on::Probe;
use pixel_pas_metadata::Buffer;
use pixel_pas_smc::Smc;
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

static MDT: &[u8] = include_bytes!(env!("SUNFISH_MODEM_MDT_PATH"));

struct Screen<'a> {
    d: &'a Display,
    aop: &'a Probe<'a>,
    resources: &'a Option<FloorReport>,
    metadata: Option<Prepared>,
}
impl Screen<'_> {
    fn inspection(&self, error: u32, r: &Inspection, secure: &Report) {
        let d = self.d;
        d.clear();
        d.line(0, "PAS INIT BUFFER");
        d.line(
            1,
            if error == 0 {
                "BUFFER EXACT STABLE"
            } else {
                "BUFFER CHANGE FOUND"
            },
        );
        for (row, label, value) in [
            (2, "ERROR", error),
            (3, "GUARD LOW BAD", r.low_guard_bad),
            (4, "GUARD HIGH BAD", r.high_guard_bad),
            (5, "PADDING BAD", r.padding_bad),
            (6, "PAYLOAD DIFF", r.payload_diff),
            (7, "PAYLOAD ZERO", r.payload_zero),
            (8, "FIRST OFFSET", r.first_diff.unwrap_or(u32::MAX)),
            (9, "LAST OFFSET", r.last_diff.unwrap_or(u32::MAX)),
            (10, "EXPECT WORD0", r.expected_words[0]),
            (11, "ACTUAL WORD0", r.actual_words[0]),
            (12, "EXPECT WORD1", r.expected_words[1]),
            (13, "ACTUAL WORD1", r.actual_words[1]),
            (14, "INPUT CRC", r.input_crc32),
            (15, "AFTER CRC", r.actual_crc32),
            (16, "UNSTABLE BYTES", r.unstable_bytes),
            (17, "ACTUAL MDT ERR", r.actual_parse_error),
            (18, "MONITOR STATUS", 0),
            (19, "SERVICE STATUS", 0),
            (
                20,
                "MDT PHYS LO",
                self.metadata.map_or(0, |p| p.metadata_physical) as u32,
            ),
            (
                21,
                "MDT BYTES",
                self.metadata.map_or(0, |p| p.metadata_length) as u32,
            ),
            (22, "SMC RETURNED", secure.returned_mask),
            (23, "AOP MASK", self.aop.mask),
            (
                24,
                "RESOURCE ACK",
                self.resources
                    .as_ref()
                    .map_or(0, |r| r.controller.acknowledged_mask),
            ),
            (
                25,
                "REQUESTS",
                self.resources.as_ref().map_or(0, |r| r.controller.requests),
            ),
        ] {
            d.hex(row, label, value);
        }
        d.line(26, "NO SEGMENT FILES");
        d.line(27, "NO MEM SETUP");
        d.line(28, "NO RESET RELEASE");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
    }

    fn paint(&self, status: &str, error: u32, secure: Option<&Report>, after: Option<u32>) {
        let d = self.d;
        d.clear();
        d.line(0, "PAS METADATA INIT");
        d.line(1, status);
        d.hex(2, "ERROR", error);
        d.hex(3, "AOP MASK", self.aop.mask);
        let c = self.resources.as_ref().map(|r| &r.controller);
        d.hex(4, "RESOURCE ACK", c.map_or(0, |c| c.acknowledged_mask));
        d.hex(5, "REQUESTS", c.map_or(0, |c| c.requests));
        d.hex(6, "SMC ISSUED", secure.map_or(0, |r| r.issued_mask));
        d.hex(7, "SMC RETURNED", secure.map_or(0, |r| r.returned_mask));
        d.hex(
            8,
            "KERNEL ERROR",
            secure.and_then(|r| r.kernel_error).unwrap_or(0) as u32,
        );
        d.hex(
            9,
            "LAST FN",
            secure
                .and_then(|r| r.last_request)
                .map_or(0, |r| r.words()[0]) as u32,
        );
        if let Some(code) = after {
            d.hex(10, "MDT AFTER ERROR", code);
        } else {
            d.hex(
                10,
                "MDT PHYS LO",
                self.metadata.map_or(0, |p| p.metadata_physical) as u32,
            );
        }
        if let Some(r) = secure {
            // Show the last actually returned register set, including all high
            // halves. Returned/issued masks distinguish an invocation failure.
            if let Some(raw) = r.replies.iter().rev().flatten().next() {
                for (i, labels) in [
                    ["X0 HI", "X0 LO"],
                    ["X1 HI", "X1 LO"],
                    ["X2 HI", "X2 LO"],
                    ["X3 HI", "X3 LO"],
                    ["X4 HI", "X4 LO"],
                    ["X5 HI", "X5 LO"],
                    ["X6 HI", "X6 LO"],
                    ["X7 HI", "X7 LO"],
                ]
                .into_iter()
                .enumerate()
                {
                    d.hex(11 + 2 * i, labels[0], (raw[i] >> 32) as u32);
                    d.hex(12 + 2 * i, labels[1], raw[i] as u32);
                }
            }
        } else if let Some(c) = c {
            d.hex(11, "RESOURCE PHASE", c.phase.code());
            d.hex(12, "CTRL ERROR", c.controller_error.map_or(0, |e| e.code()));
            d.hex(13, "MC0 ERROR", c.mc0_error.map_or(0, |e| e.code()));
            d.hex(14, "GUARD ERROR", c.handoff_error.map_or(0, |e| e.code()));
            for (i, label) in [
                "MC0 INITIAL",
                "ACV INITIAL",
                "CE0 INITIAL",
                "SN11 INITIAL",
                "SN0 INITIAL",
                "SH0 INITIAL",
            ]
            .into_iter()
            .enumerate()
            {
                pixel_modem_prepare::sample(d, 15 + i, label, c.initial_bus[0][i]);
            }
        } else {
            d.line(11, self.aop.status);
        }
        d.line(27, "NO SEGMENT FILES");
        d.line(28, "NO MEM SETUP OR RESET");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
    }
}

#[protection_domain]
fn init() -> PasInitProbe {
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    pixel_modem_prepare::progress(&d, "PREPARE FOR PAS INIT");
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
    let preparation = pixel_modem_prepare::run(
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
    let screen = Screen {
        d: &d,
        aop: &aop,
        resources: &resources,
        metadata: prepared,
    };
    if let Err(e) = preparation {
        screen.paint("PREPARATION FAIL", e, None, None);
        return PasInitProbe;
    }
    let mut secure = match InitOnce::new(prepared.unwrap()) {
        Ok(s) => s,
        Err(e) => {
            screen.paint("METADATA PLAN FAIL", 0x50000 | e.code(), None, None);
            return PasInitProbe;
        }
    };
    let mut smc = Smc;
    let result = secure
        .check(&mut smc, |r| {
            screen.paint(r.phase.label(), 0, Some(r), None)
        })
        .map_err(|e| 0x50000 | e.code())
        .and_then(|()| {
            // Fresh readback immediately before the metadata handoff. It cannot
            // re-stage/repair memory, and remains owned until physical restart.
            let fresh =
                metadata::verify(&mut buffer, physical, MDT).map_err(|e| 0x60000 | e.code())?;
            secure
                .initialize(&mut smc, fresh, |r| {
                    screen.paint(r.phase.label(), 0, Some(r), None)
                })
                .map_err(|e| 0x50000 | e.code())
        });
    // Only a completed INIT transport may be followed by CPU buffer inspection.
    // An interrupted/BUSY/unknown monitor result is left untouched, not resumed.
    let after = if secure.report().replies[2].is_some_and(|r| r[0] == 0) {
        Some(metadata::inspect(&mut buffer, physical, MDT))
    } else {
        None
    };
    let after_error = after.map(|r| r.map_or_else(|e| e.code(), |r| r.strict_error()));
    let error = result.err().unwrap_or_else(|| match after_error {
        Some(0) => 0,
        code => 0x70000 | code.unwrap_or(0xff),
    });
    // Zero status is displayed separately from unchanged/stable memory. The
    // strict metadata postcondition is not weakened for an RW consumer.
    if let Some(Ok(inspection)) = after.filter(|_| secure.report().passed() && verified) {
        screen.inspection(error, &inspection, secure.report());
    } else {
        screen.paint(
            "INIT CALL STOPPED",
            error,
            Some(secure.report()),
            after_error,
        );
    }
    debug_println!(
        "PAS-INIT|error={:X} report={:X?} after={:?} resources={:X?}",
        error,
        secure.report(),
        after,
        resources
    );
    // No MEM_SETUP, executable segments, AUTH_AND_RESET or SHUTDOWN is supplied.
    PasInitProbe
}
struct PasInitProbe;
impl Handler for PasInitProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
