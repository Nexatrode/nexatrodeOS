#![no_std]
#![no_main]
mod pixel_aop_mailbox;
mod pixel_aop_modem_on;
mod pixel_modem_prepare;
mod pixel_pas_metadata;
mod pixel_pas_smc;
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_pas_init::{InitOnce, Report as InitReport};
use personal_os_rust_server::qcom_pas_mem_setup::{
    self as setup, MemSetupOnce, Report as MemReport,
};
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
    prepared: Prepared,
}
impl Screen<'_> {
    fn paint(
        &self,
        status: &str,
        error: u32,
        init: Option<&InitReport>,
        mem: Option<&MemReport>,
        inspection: Option<&Inspection>,
    ) {
        let d = self.d;
        d.clear();
        d.line(0, "PAS MODEM MEMORY");
        d.line(1, status);
        d.hex(2, "ERROR", error);
        let init = mem.map(|r| &r.init).or(init);
        let request = mem
            .and_then(|r| r.last_request)
            .or_else(|| init.and_then(|r| r.last_request));
        d.hex(3, "LAST FN", request.map_or(0, |r| r.words()[0]) as u32);
        d.hex(4, "INIT RETURNED", init.map_or(0, |r| r.returned_mask));
        d.hex(5, "MEM ISSUED", mem.map_or(0, |r| r.issued_mask));
        d.hex(6, "MEM RETURNED", mem.map_or(0, |r| r.returned_mask));
        d.hex(
            7,
            "KERNEL ERROR",
            mem.and_then(|r| r.kernel_error)
                .or_else(|| init.and_then(|r| r.kernel_error))
                .unwrap_or(0) as u32,
        );
        d.hex(
            8,
            "CONSUMED CHECK",
            inspection.map_or(u32::MAX, |r| {
                u32::from(!setup::observed_consumed_pattern(r, self.prepared))
            }),
        );
        d.hex(9, "MODEM BASE", setup::MODEM_BASE as u32);
        d.hex(10, "MODEM BYTES", setup::MODEM_BYTES as u32);
        let raw = mem
            .and_then(|r| r.replies.iter().rev().flatten().next())
            .or_else(|| init.and_then(|r| r.replies.iter().rev().flatten().next()));
        if let Some(raw) = raw {
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
        } else {
            d.hex(11, "AOP MASK", self.aop.mask);
            if let Some(r) = self.resources.as_ref() {
                d.hex(12, "RESOURCE ACK", r.controller.acknowledged_mask);
                d.hex(13, "REQUESTS", r.controller.requests);
                d.hex(
                    14,
                    "CTRL ERROR",
                    r.controller.controller_error.map_or(0, |e| e.code()),
                );
                d.hex(
                    15,
                    "GUARD ERROR",
                    r.controller.handoff_error.map_or(0, |e| e.code()),
                );
            }
        }
        d.line(27, "NO SEGMENT FILES");
        d.line(28, "NO AUTH OR RESET");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
    }
}

#[protection_domain]
fn init() -> PasMemSetupProbe {
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    pixel_modem_prepare::progress(&d, "PREPARE FOR MEM SETUP");
    let physical = *sel4_microkit::var!(pas_metadata_paddr:usize=0) as u64;
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
    // A failed preparation has no secure call and never touches the modem pool.
    if let Err(error) = preparation {
        d.clear();
        d.line(0, "PAS MODEM MEMORY");
        d.line(1, "PREPARATION FAILED");
        d.hex(2, "ERROR", error);
        d.hex(3, "AOP MASK", aop.mask);
        if let Some(r) = resources {
            d.hex(4, "RESOURCE ACK", r.controller.acknowledged_mask);
            d.hex(5, "REQUESTS", r.controller.requests);
        }
        d.line(27, "NO SECURE CALL");
        d.line(28, "NO AUTH OR RESET");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
        return PasMemSetupProbe;
    }
    let prepared = prepared.unwrap();
    let screen = Screen {
        d: &d,
        aop: &aop,
        resources: &resources,
        prepared,
    };
    let mut init = match InitOnce::new(prepared) {
        Ok(s) => s,
        Err(e) => {
            screen.paint("METADATA PLAN FAIL", 0x50000 | e.code(), None, None, None);
            return PasMemSetupProbe;
        }
    };
    let mut smc = Smc;
    let init_result = init
        .check(&mut smc, |r| {
            screen.paint(r.phase.label(), 0, Some(r), None, None)
        })
        .map_err(|e| 0x50000 | e.code())
        .and_then(|()| {
            let fresh =
                metadata::verify(&mut buffer, physical, MDT).map_err(|e| 0x60000 | e.code())?;
            init.initialize(&mut smc, fresh, |r| {
                screen.paint(r.phase.label(), 0, Some(r), None, None)
            })
            .map_err(|e| 0x50000 | e.code())
        });
    if let Err(error) = init_result {
        screen.paint("INIT STOPPED", error, Some(init.report()), None, None);
        return PasMemSetupProbe;
    }
    let consumed = match metadata::inspect(&mut buffer, physical, MDT) {
        Ok(r) => r,
        Err(e) => {
            screen.paint(
                "INIT INSPECT FAIL",
                0x70000 | e.code(),
                Some(init.report()),
                None,
                None,
            );
            return PasMemSetupProbe;
        }
    };
    let init_report = *init.report();
    let mut mem = match MemSetupOnce::after_init(init, &consumed) {
        Ok(s) => s,
        Err(e) => {
            screen.paint(
                "CONSUMED GATE FAIL",
                0x80000 | e.code(),
                Some(&init_report),
                None,
                Some(&consumed),
            );
            return PasMemSetupProbe;
        }
    };
    let result = mem
        .check(&mut smc, |r| {
            screen.paint(r.phase.label(), 0, None, Some(r), Some(&consumed))
        })
        .map_err(|e| 0x80000 | e.code())
        .and_then(|()| {
            let fresh =
                metadata::inspect(&mut buffer, physical, MDT).map_err(|e| 0x90000 | e.code())?;
            mem.setup(&mut smc, &fresh, |r| {
                screen.paint(r.phase.label(), 0, None, Some(r), Some(&fresh))
            })
            .map_err(|e| 0x80000 | e.code())
        });
    // Do not inspect or make further calls after an interrupted/BUSY/unknown
    // monitor response. Keep every allocation and held request until restart.
    let after = if mem.report().replies[1].is_some_and(|r| r[0] == 0) {
        Some(metadata::inspect(&mut buffer, physical, MDT))
    } else {
        None
    };
    let post_ok =
        after.is_some_and(|r| r.is_ok_and(|r| setup::observed_consumed_pattern(&r, prepared)));
    let error = result.err().unwrap_or_else(|| {
        if post_ok {
            0
        } else {
            0xa0000 | after.and_then(Result::err).map_or(0xff, |e| e.code())
        }
    });
    let passed = error == 0 && verified && mem.report().passed();
    screen.paint(
        if passed {
            "MEM SETUP PASS"
        } else {
            "MEM SETUP STOPPED"
        },
        error,
        None,
        Some(mem.report()),
        after.as_ref().and_then(|r| r.as_ref().ok()),
    );
    debug_println!(
        "PAS-MEM-SETUP|error={:X} report={:X?} after={:X?}",
        error,
        mem.report(),
        after
    );
    PasMemSetupProbe
}
struct PasMemSetupProbe;
impl Handler for PasMemSetupProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
