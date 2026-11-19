#![no_std]
#![no_main]
mod pixel_aop_mailbox;
mod pixel_aop_modem_on;
mod pixel_modem_image_memory;
mod pixel_modem_prepare;
mod pixel_pas_metadata;
mod pixel_pas_smc;
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_modem_image::{self as image, ImageOnce, Plan, Source};
use personal_os_rust_server::qcom_pas_init::{InitOnce, Report as InitReport};
use personal_os_rust_server::qcom_pas_mem_setup::{
    self as setup, MemSetupOnce, Report as MemReport,
};
use personal_os_rust_server::qcom_pas_metadata::{self as metadata, Inspection, Prepared};
use personal_os_rust_server::qcom_rpmh_proxy_hold::FloorReport;
use pixel_aop_mailbox::Mailbox;
use pixel_aop_modem_on::Probe;
use pixel_modem_image_memory::ImageMemory;
use pixel_pas_metadata::Buffer;
use pixel_pas_smc::Smc;
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

include!(env!("SUNFISH_MODEM_SOURCES_RS_PATH"));
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
        d.line(0, "MODEM IMAGE STAGE");
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
        d.line(27, "NO DATA WRITTEN YET");
        d.line(28, "NO AUTH OR RESET");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
    }
}

#[protection_domain]
fn init() -> ModemImageProbe {
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    pixel_modem_prepare::progress(&d, "VALIDATE IMAGE PLAN");
    let plan = match Plan::new(MDT, &FIRMWARE_SOURCES) {
        Ok(p) => p,
        Err(e) => {
            d.clear();
            d.line(0, "MODEM IMAGE STAGE");
            d.line(1, "IMAGE PLAN STOPPED");
            d.hex(2, "ERROR", e.code());
            d.line(27, "NO SECURE CALL");
            d.line(28, "NO POOL ACCESS");
            d.line(29, "RESTART WHEN DONE");
            d.flush();
            return ModemImageProbe;
        }
    };
    pixel_modem_prepare::progress(&d, "PREPARE FOR IMAGE STAGE");
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
        d.line(0, "MODEM IMAGE STAGE");
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
        return ModemImageProbe;
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
            return ModemImageProbe;
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
        return ModemImageProbe;
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
            return ModemImageProbe;
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
            return ModemImageProbe;
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
    if !passed {
        screen.paint(
            "MEM SETUP STOPPED",
            error,
            None,
            Some(mem.report()),
            after.as_ref().and_then(|r| r.as_ref().ok()),
        );
        return ModemImageProbe;
    }
    // The only modem-pool access is below both successful PAS calls and the
    // final exact consumed-buffer check. No pool reads in any earlier stage.
    let mut stage = match ImageOnce::after_setup(mem, plan) {
        Ok(s) => s,
        Err(e) => {
            screen.paint("DATA PLAN STOPPED", 0xb0000 | e.code(), None, None, None);
            return ModemImageProbe;
        }
    };
    let mut memory = unsafe {
        ImageMemory::new(
            memory_region_symbol!(modem_image_pool:*mut u8).as_ptr(),
            memory_region_symbol!(modem_gap_snapshot:*mut u8).as_ptr(),
        )
    };
    let _result = stage.run(&mut memory, |r| paint_image(&d, r, None));
    let post = if stage.report().passed() {
        Some(
            metadata::inspect(&mut buffer, physical, MDT)
                .is_ok_and(|r| setup::observed_consumed_pattern(&r, prepared)),
        )
    } else {
        None
    };
    paint_image(&d, stage.report(), post);
    debug_println!(
        "MODEM-IMAGE|report={:X?} metadata_post={:?}",
        stage.report(),
        post
    );
    ModemImageProbe
}
fn paint_image(d: &Display, r: &image::Report, post: Option<bool>) {
    d.clear();
    d.line(0, "MODEM IMAGE STAGE");
    d.line(
        1,
        if post == Some(false) {
            "METADATA POST FAIL"
        } else {
            r.phase.label()
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(
        3,
        "SEGMENT INDEX",
        r.current_index.map_or(u32::MAX, u32::from),
    );
    d.hex(4, "LOADED MASK", r.loaded_mask);
    d.hex(5, "VERIFIED ONE", r.verified_masks[0]);
    d.hex(6, "VERIFIED TWO", r.verified_masks[1]);
    d.hex(7, "FILE WRITES", r.file_writes);
    d.hex(8, "ZERO WRITES", r.zero_writes);
    d.hex(9, "PAYLOAD BAD", r.payload_bad);
    d.hex(10, "ZERO TAIL BAD", r.zero_bad);
    d.hex(11, "GAP BAD", r.gap_bad);
    d.hex(12, "FIRST BAD", r.first_bad.unwrap_or(u32::MAX));
    d.hex(13, "CHECKED ONE", r.checked_bytes[0]);
    d.hex(14, "CHECKED TWO", r.checked_bytes[1]);
    d.hex(15, "GAP SNAPSHOT", r.snapshot_bytes);
    d.hex(16, "GAPS CHECKED", r.gap_checked);
    d.hex(
        17,
        "METADATA POST",
        post.map_or(u32::MAX, |ok| u32::from(!ok)),
    );
    d.hex(18, "INIT RETURNED", r.memory_setup.init.returned_mask);
    d.hex(19, "MEM RETURNED", r.memory_setup.returned_mask);
    d.line(22, "MODEM MAPPING NX");
    d.line(23, "NO AUTH OR RESET");
    d.line(24, "NO MODEM START");
    d.line(25, "NO DMA OR RADIO");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
struct ModemImageProbe;
impl Handler for ModemImageProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
