#![no_std]
#![no_main]
mod pixel_aop_mailbox;
mod pixel_aop_modem_on;
#[cfg(feature = "sunfish-modem-crash-probe")]
mod pixel_modem_crash;
mod pixel_modem_image_memory;
#[cfg(feature = "sunfish-modem-owner")]
mod pixel_modem_owner;
#[cfg(feature = "sunfish-modem-owner")]
mod pixel_modem_owner_gate;
#[cfg(feature = "sunfish-modem-owner")]
pub(crate) mod pixel_modem_pool_resources;
mod pixel_modem_status;
#[cfg(feature = "sunfish-modem-owner")]
static mut MODEM_OWNER: Option<pixel_modem_owner::Runtime> = None;
#[cfg(all(
    feature = "sunfish-modem-owner",
    any(
        feature = "sunfish-glink-rx-packet-probe",
        feature = "sunfish-modem-crash-probe"
    )
))]
compile_error!("modem owner mode consumes the channel handoff exclusively");
#[cfg(all(
    feature = "sunfish-modem-fatal-trace-probe",
    not(feature = "sunfish-modem-crash-probe")
))]
compile_error!("fatal-source trace requires the bounded modem crash snapshot mode");
#[cfg(any(
    feature = "sunfish-glink-boot-probe",
    feature = "sunfish-glink-version-probe"
))]
mod pixel_glink_boot;
#[cfg(feature = "sunfish-glink-channel-probe")]
mod pixel_glink_channel;
#[cfg(feature = "sunfish-qrtr-servreg-probe")]
mod pixel_glink_control;
#[cfg(feature = "sunfish-glink-done-probe")]
mod pixel_glink_done;
#[cfg(feature = "sunfish-glink-intents-probe")]
mod pixel_glink_intents;
mod pixel_glink_inventory;
#[cfg(feature = "sunfish-glink-open-probe")]
mod pixel_glink_open;
#[cfg(feature = "sunfish-glink-version-probe")]
mod pixel_glink_pending;
#[cfg(any(
    feature = "sunfish-glink-prepare-probe",
    feature = "sunfish-glink-boot-probe",
    feature = "sunfish-glink-version-probe"
))]
mod pixel_glink_prepare;
#[cfg(any(
    feature = "sunfish-glink-receive-probe",
    feature = "sunfish-glink-boot-probe",
    feature = "sunfish-glink-version-probe"
))]
mod pixel_glink_receive;
#[cfg(feature = "sunfish-glink-rx-packet-probe")]
mod pixel_glink_rx;
#[cfg(feature = "sunfish-glink-version-probe")]
mod pixel_glink_version;
#[cfg(feature = "sunfish-qrtr-discovery-probe")]
mod pixel_qrtr_discovery;
#[cfg(feature = "sunfish-qrtr-hello-probe")]
mod pixel_qrtr_hello;
#[cfg(feature = "sunfish-qrtr-next-probe")]
mod pixel_qrtr_next;
#[cfg(feature = "sunfish-qrtr-servreg-probe")]
mod pixel_qrtr_prequery;
#[cfg(feature = "sunfish-qrtr-servreg-probe")]
mod pixel_qrtr_servreg;
#[cfg(feature = "sunfish-qrtr-servreg-probe")]
mod pixel_qrtr_snapshot;
#[cfg(feature = "sunfish-qrtr-servreg-probe")]
mod pixel_servreg_wire;
mod pixel_smp2p_handshake;
#[cfg(feature = "sunfish-qrtr-discovery-probe")]
static mut GLINK_DISCOVERY_BUFFERS: [pixel_glink_rx::Buffer; 32] =
    [const { pixel_glink_rx::Buffer::new() }; 32];
#[cfg(feature = "sunfish-glink-rx-packet-probe")]
static mut GLINK_RX_BUFFER: pixel_glink_rx::Buffer = pixel_glink_rx::Buffer::new();
#[cfg(feature = "sunfish-qrtr-next-probe")]
static mut GLINK_NEXT_BUFFER: pixel_glink_rx::Buffer = pixel_glink_rx::Buffer::new();
#[cfg(any(
    all(
        feature = "sunfish-glink-receive-probe",
        feature = "sunfish-glink-prepare-probe"
    ),
    all(
        feature = "sunfish-glink-boot-probe",
        feature = "sunfish-glink-receive-probe"
    ),
    all(
        feature = "sunfish-glink-boot-probe",
        feature = "sunfish-glink-prepare-probe"
    ),
    all(
        feature = "sunfish-glink-version-probe",
        feature = "sunfish-glink-prepare-probe"
    ),
    all(
        feature = "sunfish-glink-version-probe",
        feature = "sunfish-glink-boot-probe"
    ),
    all(
        feature = "sunfish-glink-version-probe",
        feature = "sunfish-glink-receive-probe"
    )
))]
compile_error!("choose exactly one GLINK experimental mode");
mod pixel_modem_prepare;
mod pixel_pas_metadata;
mod pixel_pas_smc;
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_modem_image::{self as image, ImageOnce, Plan, Source};
use personal_os_rust_server::qcom_pas_auth::{AuthOnce, Report as AuthReport};
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
    Channel, ChannelSet, Handler, Infallible, MessageInfo, memory_region_symbol, protection_domain,
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
        d.line(28, "AUTH NOT CALLED YET");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
    }
}

#[protection_domain]
fn init() -> ModemImageProbe {
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    d.clear();
    d.line(0, "MODEM STATUS CHECK");
    d.line(1, "CHECK STATUS CLOCK");
    d.flush();
    let mut clock = match pixel_modem_status::PhysicalCounter::new() {
        Ok(c) => c,
        Err(e) => {
            show_status_error(&d, "CLOCK CHECK FAIL", e.code());
            return ModemImageProbe;
        }
    };
    let smem_mapping = memory_region_symbol!(modem_smem_status:*mut u8)
        .as_ptr()
        .cast_const();
    let lock_mapping = memory_region_symbol!(tcsr_smem_lock:*mut u32).as_ptr();
    #[cfg(feature = "sunfish-modem-owner")]
    let ap_page = memory_region_symbol!(smp2p_ap_page:*mut u8).as_ptr();
    #[cfg(feature = "sunfish-modem-owner")]
    let doorbell = memory_region_symbol!(aop_doorbell:*mut u32).as_ptr();
    #[cfg(feature = "sunfish-modem-owner")]
    let owner_tail = memory_region_symbol!(glink_prepare_tail:*mut u8).as_ptr();
    #[cfg(feature = "sunfish-modem-owner")]
    if personal_os_rust_server::qcom_modem_owner::Layout::validate_symbols(
        smem_mapping as usize,
        ap_page as usize,
        owner_tail as usize,
        lock_mapping as usize,
        doorbell as usize,
    )
    .is_err()
    {
        d.line(1, "OWNER MAPPING REJECTED");
        d.flush();
        return ModemImageProbe;
    }
    let mut observer = unsafe {
        pixel_modem_status::Window::new(
            // The Microkit symbol macro accepts *mut only; the manifest is RO and
            // the observer receives a const pointer with no SMEM write operation.
            smem_mapping,
            lock_mapping,
        )
    };
    d.line(1, "CHECK STATUS BASELINE");
    d.flush();
    let baseline = match observer.before_auth() {
        Ok(b) => b,
        Err(e) => {
            show_status_error(&d, "BASELINE STOPPED", e.code());
            return ModemImageProbe;
        }
    };
    if let Err(e) = pixel_smp2p_handshake::check_initial(&mut observer, &baseline) {
        show_status_error(&d, "AP BASELINE STOPPED", e.code());
        return ModemImageProbe;
    }
    #[cfg(feature = "sunfish-modem-crash-probe")]
    let crash_baseline = pixel_modem_crash::capture(&mut observer);
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
    #[cfg(not(feature = "sunfish-modem-owner"))]
    let doorbell = memory_region_symbol!(aop_doorbell:*mut u32).as_ptr();
    let mailbox = unsafe {
        Mailbox::new(
            memory_region_symbol!(aop_msgram:*mut u32).as_ptr(),
            doorbell,
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
        d.line(28, "AUTH NOT CALLED YET");
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
    if let Err(e) = observer.revalidate(&baseline) {
        show_status_error(&d, "BASELINE CHANGED", e.code());
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
    if !stage.report().passed() || post != Some(true) {
        return ModemImageProbe;
    }
    let fresh = match metadata::inspect(&mut buffer, physical, MDT) {
        Ok(r) => r,
        Err(_) => {
            paint_image(&d, stage.report(), Some(false));
            return ModemImageProbe;
        }
    };
    if let Err(e) = observer.revalidate(&baseline) {
        show_status_error(&d, "BASELINE CHANGED", e.code());
        return ModemImageProbe;
    }
    let mut auth = match AuthOnce::after_image(stage, prepared, &fresh) {
        Ok(s) => s,
        Err(e) => {
            d.clear();
            d.line(0, "MODEM AUTH TEST");
            d.line(1, "AUTH GATE STOPPED");
            d.hex(2, "ERROR", e.code());
            d.line(29, "RESTART WHEN DONE");
            d.flush();
            return ModemImageProbe;
        }
    };
    #[cfg(not(feature = "sunfish-modem-owner"))]
    let ap_page = memory_region_symbol!(smp2p_ap_page:*mut u8).as_ptr();
    let mut handshake = unsafe { pixel_smp2p_handshake::Handshake::new(ap_page, doorbell) };
    let result = auth.check(&mut smc, |r| paint_auth(&d, r));
    if result.is_ok() {
        match metadata::inspect(&mut buffer, physical, MDT) {
            Ok(fresh) => {
                let permit = match auth.status_prepare(&fresh) {
                    Ok(p) => p,
                    Err(e) => {
                        show_status_error(&d, "AP PERMIT STOPPED", e.code());
                        return ModemImageProbe;
                    }
                };
                if let Err(e) = handshake.prepare(&mut observer, &baseline, permit) {
                    paint_ap_prepare(&d, &handshake, "AP PREPARE STOPPED", e.code());
                    return ModemImageProbe;
                }
                paint_ap_prepare(&d, &handshake, "AP PREPARE PASS", 0);
                // Re-read consumed metadata after AP publication, immediately
                // before the single secure authentication/reset call.
                let fresh = match metadata::inspect(&mut buffer, physical, MDT) {
                    Ok(r) => r,
                    Err(e) => {
                        paint_ap_prepare(&d, &handshake, "AUTH METADATA FAIL", e.code());
                        return ModemImageProbe;
                    }
                };
                #[cfg(feature = "sunfish-glink-prepare-probe")]
                {
                    let permit = match auth.into_glink_memory_preparation(&fresh) {
                        Ok(p) => p,
                        Err(e) => {
                            paint_ap_prepare(&d, &handshake, "GLINK PERMIT STOPPED", e.code());
                            return ModemImageProbe;
                        }
                    };
                    let mut preparation = unsafe {
                        pixel_glink_prepare::Preparation::new(ap_page, {
                            #[cfg(feature = "sunfish-modem-owner")]
                            {
                                owner_tail
                            }
                            #[cfg(not(feature = "sunfish-modem-owner"))]
                            {
                                memory_region_symbol!(glink_prepare_tail:*mut u8).as_ptr()
                            }
                        })
                    };
                    let result = preparation.run(&mut observer, &handshake, permit);
                    paint_glink_prepare(&d, &result);
                    return ModemImageProbe;
                }
                #[cfg(any(
                    feature = "sunfish-glink-boot-probe",
                    feature = "sunfish-glink-version-probe"
                ))]
                {
                    let boot = match unsafe {
                        pixel_glink_boot::Boot::prepare(
                            &mut observer,
                            &handshake,
                            auth,
                            &fresh,
                            ap_page,
                            {
                                #[cfg(feature = "sunfish-modem-owner")]
                                {
                                    owner_tail
                                }
                                #[cfg(not(feature = "sunfish-modem-owner"))]
                                {
                                    memory_region_symbol!(glink_prepare_tail:*mut u8).as_ptr()
                                }
                            },
                        )
                    } {
                        Ok(b) => b,
                        Err(e) => {
                            paint_ap_prepare(&d, &handshake, "GLINK PREPARE STOPPED", e.code());
                            return ModemImageProbe;
                        }
                    };
                    d.clear();
                    d.line(0, "MODEM GLINK BOOT");
                    d.line(1, "GLINK PREPARE PASS");
                    d.hex(2, "FREE AFTER", boot.prepared().free_after);
                    d.flush();
                    let fresh = match metadata::inspect(&mut buffer, physical, MDT) {
                        Ok(r) => r,
                        Err(e) => {
                            paint_ap_prepare(&d, &handshake, "AUTH METADATA FAIL", e.code());
                            return ModemImageProbe;
                        }
                    };
                    let started =
                        match boot.authenticate(&mut observer, &handshake, &mut smc, &fresh, |r| {
                            paint_auth(&d, r)
                        }) {
                            Ok(s) => s,
                            Err(f) => {
                                paint_glink_boot_failure(&d, &f);
                                return ModemImageProbe;
                            }
                        };
                    #[cfg(feature = "sunfish-glink-boot-probe")]
                    let (auth, identity) = started.into_parts();
                    #[cfg(feature = "sunfish-glink-version-probe")]
                    let (auth, fresh) = started.into_fresh_parts();
                    let r = handshake.observe(&mut observer, baseline, auth, &mut clock, |r| {
                        paint_handshake(&d, r)
                    });
                    paint_handshake(&d, &r);
                    if r.status.ready_bit_seen {
                        #[cfg(feature = "sunfish-glink-boot-probe")]
                        {
                            let result = handshake.take_ready_observation().and_then(|ready| {
                                pixel_glink_boot::read(&mut observer, identity, ready)
                            });
                            paint_glink_boot(&d, &result);
                        }
                        #[cfg(feature = "sunfish-glink-version-probe")]
                        {
                            let ready = match handshake.take_ready_observation() {
                                Ok(r) => r,
                                Err(e) => {
                                    paint_ap_prepare(
                                        &d,
                                        &handshake,
                                        "READY TOKEN STOPPED",
                                        e.code(),
                                    );
                                    return ModemImageProbe;
                                }
                            };
                            let version = unsafe {
                                pixel_glink_version::VersionOnce::new(fresh, ap_page, doorbell)
                            };
                            #[cfg(not(feature = "sunfish-glink-open-probe"))]
                            let mut version = version;
                            #[cfg(not(feature = "sunfish-glink-pending-probe"))]
                            let result = version.run(&mut observer, ready, &mut clock, |r| {
                                paint_glink_version(&d, r)
                            });
                            #[cfg(all(
                                feature = "sunfish-glink-pending-probe",
                                not(feature = "sunfish-glink-open-probe")
                            ))]
                            let result =
                                version.run_pending(&mut observer, ready, &mut clock, |r| {
                                    paint_glink_version(&d, r)
                                });
                            #[cfg(not(feature = "sunfish-glink-pending-probe"))]
                            paint_glink_version(&d, &result);
                            #[cfg(all(
                                feature = "sunfish-glink-pending-probe",
                                not(feature = "sunfish-glink-open-probe")
                            ))]
                            paint_glink_pending(&d, &result);
                            #[cfg(feature = "sunfish-glink-open-probe")]
                            {
                                let checkpoint = match version.into_open_checkpoint(
                                    &mut observer,
                                    ready,
                                    &mut clock,
                                    |r| paint_glink_version(&d, r),
                                ) {
                                    Ok(s) => s,
                                    Err(r) => {
                                        paint_glink_pending(&d, &r);
                                        return ModemImageProbe;
                                    }
                                };
                                #[cfg(feature = "sunfish-glink-open-plan-snapshot")]
                                {
                                    let result = pixel_glink_open::snapshot_plan(checkpoint);
                                    paint_glink_plan_snapshot(&d, &result);
                                    return ModemImageProbe;
                                }
                                #[cfg(not(feature = "sunfish-glink-open-plan-snapshot"))]
                                {
                                    let open = unsafe {
                                        pixel_glink_open::OpenOnce::new(
                                            checkpoint, ap_page, doorbell,
                                        )
                                    };
                                    #[cfg(not(feature = "sunfish-glink-channel-probe"))]
                                    {
                                        let mut open = open;
                                        let result = open.run(&mut observer, &mut clock, |r| {
                                            paint_glink_open(&d, r)
                                        });
                                        paint_glink_open(&d, &result);
                                    }
                                    #[cfg(feature = "sunfish-glink-channel-probe")]
                                    {
                                        let checkpoint = match open.into_channel_checkpoint(
                                            &mut observer,
                                            &mut clock,
                                            |r| paint_glink_open(&d, r),
                                        ) {
                                            Ok(s) => s,
                                            Err(r) => {
                                                paint_glink_open(&d, &r);
                                                return ModemImageProbe;
                                            }
                                        };
                                        let channel = unsafe {
                                            pixel_glink_channel::ChannelOnce::new(
                                                checkpoint, ap_page, doorbell,
                                            )
                                        };
                                        #[cfg(feature = "sunfish-modem-owner")]
                                        {
                                            let checkpoint = match channel.into_receive_checkpoint(
                                                &mut observer,
                                                &mut clock,
                                                |r| paint_glink_channel(&d, r),
                                            ) {
                                                Ok(checkpoint) => checkpoint,
                                                Err(report) => {
                                                    paint_glink_channel(&d, &report);
                                                    return ModemImageProbe;
                                                }
                                            };
                                            // Transfer the one actual fresh authenticated channel;
                                            // no old polling writer remains after this return.
                                            let scm: Result<
                                                Option<pixel_modem_owner::ScmResources>,
                                                personal_os_rust_server::qcom_modem_owner::Error,
                                            > = {
                                                #[cfg(
                                                    feature = "sunfish-modem-servreg-state-probe"
                                                )]
                                                {
                                                    Ok(None)
                                                }
                                                #[cfg(not(
                                                    feature = "sunfish-modem-servreg-state-probe"
                                                ))]
                                                {
                                                    pixel_modem_pool_resources::injected(1, 0)
                                                .map_err(|_| personal_os_rust_server::qcom_modem_owner::Error::Mapping)
                                                .and_then(|_| pixel_modem_pool_resources::validated_scm_args()
                                                .map_err(|_| personal_os_rust_server::qcom_modem_owner::Error::Mapping)
                                                .and_then(|args| unsafe {
                                                    pixel_modem_owner::ScmResources::new(
                                                        args,
                                                        pixel_modem_pool_resources::invoke_scm,
                                                    )
                                                }))
                                                .map(Some)
                                                }
                                            };
                                            #[cfg(feature = "sunfish-modem-servreg-state-probe")]
                                            pixel_modem_owner::paint_servreg_owner_stage(
                                                &d,
                                                "ATTACHING NATIVE OWNER",
                                            );
                                            #[cfg(feature = "sunfish-modem-servreg-state-probe")]
                                            let mut attach_stage = "ATTACHING NATIVE OWNER";
                                            let runtime = scm.and_then(|scm| unsafe {
                                                    pixel_modem_owner::attach(
                                                        observer,
                                                        checkpoint,
                                                        smem_mapping,
                                                        ap_page,
                                                        doorbell,
                                                        d.pixels_ptr(),
                                                        scm,
                                                        |stage| {
                                                            #[cfg(feature = "sunfish-modem-servreg-state-probe")]
                                                            {
                                                                attach_stage = stage;
                                                                pixel_modem_owner::paint_servreg_owner_stage(&d, stage);
                                                            }
                                                            #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
                                                            let _ = stage;
                                                        },
                                                    )
                                                });
                                            match runtime {
                                                Ok(mut runtime) => {
                                                    #[cfg(
                                                        feature = "sunfish-modem-servreg-state-probe"
                                                    )]
                                                    pixel_modem_owner::paint_servreg_owner_stage(
                                                        &d,
                                                        "STARTING OWNER IRQ LOOP",
                                                    );
                                                    let result = runtime.start();
                                                    unsafe {
                                                        *core::ptr::addr_of_mut!(MODEM_OWNER) =
                                                            Some(runtime);
                                                    }
                                                    #[cfg(
                                                        feature = "sunfish-modem-servreg-state-probe"
                                                    )]
                                                    if let Some(owner) = unsafe {
                                                        (&mut *core::ptr::addr_of_mut!(MODEM_OWNER))
                                                            .as_mut()
                                                    } {
                                                        if result.is_err() {
                                                            owner.mark_servreg_query_failed();
                                                        }
                                                        owner.paint_servreg_query(&d);
                                                    }
                                                    #[cfg(not(
                                                        feature = "sunfish-modem-servreg-state-probe"
                                                    ))]
                                                    {
                                                        d.clear();
                                                        d.line(0, "MODEM OWNER IRQ MODE");
                                                        d.line(
                                                            1,
                                                            if result.is_ok() {
                                                                "OWNER ACTIVE"
                                                            } else {
                                                                "OWNER STOPPED RETAINED"
                                                            },
                                                        );
                                                        d.line(28, "NO STORAGE OR DMA BACKEND");
                                                        d.flush();
                                                    }
                                                }
                                                Err(error) => {
                                                    sel4_microkit::debug_println!(
                                                        "MODEM_OWNER|ATTACH STOPPED stage={} error={:?}",
                                                        {
                                                            #[cfg(
                                                                feature = "sunfish-modem-servreg-state-probe"
                                                            )]
                                                            {
                                                                attach_stage
                                                            }
                                                            #[cfg(not(
                                                                feature = "sunfish-modem-servreg-state-probe"
                                                            ))]
                                                            {
                                                                "ATTACH"
                                                            }
                                                        },
                                                        error
                                                    );
                                                    d.clear();
                                                    d.line(0, "SERVREG OWNER PATH");
                                                    d.line(1, "OWNER ATTACH STOPPED");
                                                    #[cfg(
                                                        feature = "sunfish-modem-servreg-state-probe"
                                                    )]
                                                    d.line(2, attach_stage);
                                                    d.line(4, "NO QUERY SENT");
                                                    d.line(5, "NO WLAN START");
                                                    d.line(6, "NO WLAN MSA GRANT");
                                                    d.line(28, "NO WLAN OR HOST DMA");
                                                    d.line(29, "RESTART WHEN DONE");
                                                    d.flush();
                                                }
                                            }
                                            return ModemImageProbe;
                                        }
                                        #[cfg(not(any(
                                            feature = "sunfish-glink-rx-packet-probe",
                                            feature = "sunfish-modem-owner"
                                        )))]
                                        {
                                            let mut channel = channel;
                                            let result =
                                                channel.run(&mut observer, &mut clock, |r| {
                                                    paint_glink_channel(&d, r)
                                                });
                                            paint_glink_channel(&d, &result);
                                        }
                                        #[cfg(feature = "sunfish-glink-rx-packet-probe")]
                                        {
                                            let checkpoint = match channel.into_receive_checkpoint(
                                                &mut observer,
                                                &mut clock,
                                                |r| paint_glink_channel(&d, r),
                                            ) {
                                                Ok(c) => c,
                                                Err(r) => {
                                                    paint_glink_channel(&d, &r);
                                                    return ModemImageProbe;
                                                }
                                            };
                                            let buffer = unsafe {
                                                &mut *core::ptr::addr_of_mut!(GLINK_RX_BUFFER)
                                            };
                                            let receiver = unsafe {
                                                pixel_glink_rx::ReceiveOnce::new(
                                                    checkpoint, buffer, ap_page, doorbell,
                                                )
                                            };
                                            #[cfg(not(feature = "sunfish-glink-done-probe"))]
                                            {
                                                let mut receiver = receiver;
                                                let result =
                                                    receiver.run(&mut observer, &mut clock, |r| {
                                                        paint_glink_rx(&d, r)
                                                    });
                                                paint_glink_rx(&d, &result);
                                                // Keep the unique static-buffer lease parked until full restart.
                                                core::mem::forget(receiver);
                                            }
                                            #[cfg(feature = "sunfish-glink-done-probe")]
                                            {
                                                let checkpoint = match receiver
                                                    .into_completion_checkpoint(
                                                        &mut observer,
                                                        &mut clock,
                                                        |r| paint_glink_rx(&d, r),
                                                    ) {
                                                    Ok(c) => c,
                                                    Err(stopped) => {
                                                        paint_glink_rx(&d, &stopped.report);
                                                        core::mem::forget(stopped);
                                                        return ModemImageProbe;
                                                    }
                                                };
                                                let completion = unsafe {
                                                    pixel_glink_done::CompleteOnce::new(
                                                        checkpoint, ap_page, doorbell,
                                                    )
                                                };
                                                #[cfg(not(feature = "sunfish-qrtr-hello-probe"))]
                                                {
                                                    let mut completion = completion;
                                                    let result = completion.run(
                                                        &mut observer,
                                                        &mut clock,
                                                        |r| paint_glink_done(&d, r),
                                                    );
                                                    paint_glink_done(&d, &result);
                                                    core::mem::forget(completion);
                                                }
                                                #[cfg(feature = "sunfish-qrtr-hello-probe")]
                                                {
                                                    let checkpoint = match completion
                                                        .into_send_checkpoint(
                                                            &mut observer,
                                                            &mut clock,
                                                            |r| paint_glink_done(&d, r),
                                                        ) {
                                                        Ok(c) => c,
                                                        Err(stopped) => {
                                                            paint_glink_done(&d, &stopped.report);
                                                            core::mem::forget(stopped);
                                                            return ModemImageProbe;
                                                        }
                                                    };
                                                    let hello = unsafe {
                                                        pixel_qrtr_hello::HelloOnce::new(
                                                            checkpoint, ap_page, doorbell,
                                                        )
                                                    };
                                                    #[cfg(not(
                                                        feature = "sunfish-qrtr-next-probe"
                                                    ))]
                                                    {
                                                        let mut hello = hello;
                                                        let result = hello.run(
                                                            &mut observer,
                                                            &mut clock,
                                                            |r| paint_qrtr_hello(&d, r),
                                                        );
                                                        paint_qrtr_hello(&d, &result);
                                                        core::mem::forget(hello);
                                                    }
                                                    #[cfg(feature = "sunfish-qrtr-next-probe")]
                                                    {
                                                        let checkpoint = match hello
                                                            .into_receive_checkpoint(
                                                                &mut observer,
                                                                &mut clock,
                                                                |r| paint_qrtr_hello(&d, r),
                                                            ) {
                                                            Ok(c) => c,
                                                            Err(stopped) => {
                                                                paint_qrtr_hello(
                                                                    &d,
                                                                    &stopped.report,
                                                                );
                                                                core::mem::forget(stopped);
                                                                return ModemImageProbe;
                                                            }
                                                        };
                                                        let buffer = unsafe {
                                                            &mut *core::ptr::addr_of_mut!(
                                                                GLINK_NEXT_BUFFER
                                                            )
                                                        };
                                                        let next = unsafe {
                                                            pixel_qrtr_next::NextOnce::new(
                                                                checkpoint, buffer, ap_page,
                                                                doorbell,
                                                            )
                                                        };
                                                        #[cfg(not(
                                                            feature = "sunfish-qrtr-discovery-probe"
                                                        ))]
                                                        {
                                                            let mut next = next;
                                                            let result = next.run(
                                                                &mut observer,
                                                                &mut clock,
                                                                |r| paint_qrtr_next(&d, r),
                                                            );
                                                            paint_qrtr_next(&d, &result);
                                                            core::mem::forget(next);
                                                        }
                                                        #[cfg(
                                                            feature = "sunfish-qrtr-discovery-probe"
                                                        )]
                                                        {
                                                            let checkpoint = match next
                                                                .into_discovery_checkpoint(
                                                                    &mut observer,
                                                                    &mut clock,
                                                                    |r| paint_qrtr_next(&d, r),
                                                                ) {
                                                                Ok(c) => c,
                                                                Err(stopped) => {
                                                                    paint_qrtr_next(
                                                                        &d,
                                                                        &stopped.report,
                                                                    );
                                                                    core::mem::forget(stopped);
                                                                    return ModemImageProbe;
                                                                }
                                                            };
                                                            let pool = unsafe {
                                                                &mut *core::ptr::addr_of_mut!(
                                                                    GLINK_DISCOVERY_BUFFERS
                                                                )
                                                            };
                                                            #[cfg(
                                                                feature = "sunfish-qrtr-servreg-probe"
                                                            )]
                                                            {
                                                                let mut servreg = unsafe {
                                                                    pixel_qrtr_servreg::ServregOnce::new(
                                                                        checkpoint,
                                                                        &mut pool[0],
                                                                        ap_page,
                                                                        doorbell,
                                                                    )
                                                                };
                                                                let result = servreg
                                                                    .run(&mut observer, &mut clock);
                                                                paint_qrtr_servreg(&d, &result);
                                                                core::mem::forget(servreg);
                                                            }
                                                            #[cfg(not(
                                                                feature = "sunfish-qrtr-servreg-probe"
                                                            ))]
                                                            {
                                                                #[cfg(not(
                                                                    feature = "sunfish-qrtr-credit-probe"
                                                                ))]
                                                                let mut discovery = unsafe {
                                                                    pixel_qrtr_discovery::DiscoveryOnce::new(checkpoint,pool,ap_page,doorbell)
                                                                };
                                                                #[cfg(all(
                                                                    feature = "sunfish-qrtr-credit-probe",
                                                                    not(
                                                                        feature = "sunfish-qrtr-wait-probe"
                                                                    )
                                                                ))]
                                                                let mut discovery = unsafe {
                                                                    pixel_qrtr_discovery::DiscoveryOnce::credit(checkpoint,pool,ap_page,doorbell)
                                                                };
                                                                #[cfg(
                                                                    feature = "sunfish-qrtr-wait-probe"
                                                                )]
                                                                let mut discovery = unsafe {
                                                                    pixel_qrtr_discovery::DiscoveryOnce::wait(checkpoint,pool,ap_page,doorbell)
                                                                };
                                                                let result = discovery.run(
                                                                    &mut observer,
                                                                    &mut clock,
                                                                    |r| paint_qrtr_discovery(&d, r),
                                                                );
                                                                paint_qrtr_discovery(&d, &result);
                                                                core::mem::forget(discovery);
                                                                #[cfg(
                                                                    feature = "sunfish-modem-crash-probe"
                                                                )]
                                                                {
                                                                    // Terminal protocol state. Retain every lease and perform only
                                                                    // diagnostic reads; no further RX/packet/index/doorbell action.
                                                                    let crash_after =
                                                                        pixel_modem_crash::capture(
                                                                            &mut observer,
                                                                        );
                                                                    let crash_status =
                                                                        pixel_modem_crash::status(
                                                                            &mut observer,
                                                                        );
                                                                    let crash_steps=(pixel_modem_crash::partition_stage(&mut observer),pixel_modem_crash::partition_stage(&mut observer));
                                                                    #[cfg(
                                                                        feature = "sunfish-modem-fatal-trace-probe"
                                                                    )]
                                                                    paint_modem_fatal_trace(
                                                                        &d,
                                                                        &result,
                                                                        &crash_baseline,
                                                                        &crash_after,
                                                                        &crash_status,
                                                                        crash_steps,
                                                                    );
                                                                    #[cfg(not(
                                                                        feature = "sunfish-modem-fatal-trace-probe"
                                                                    ))]
                                                                    paint_modem_crash(
                                                                        &d,
                                                                        &result,
                                                                        &crash_baseline,
                                                                        &crash_after,
                                                                        &crash_status,
                                                                        crash_steps,
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return ModemImageProbe;
                }
                #[cfg(not(any(
                    feature = "sunfish-glink-prepare-probe",
                    feature = "sunfish-glink-boot-probe",
                    feature = "sunfish-glink-version-probe"
                )))]
                let _ = auth.authenticate(&mut smc, &fresh, |r| paint_auth(&d, r));
            }
            Err(_) => {
                d.clear();
                d.line(0, "MODEM AUTH TEST");
                d.line(1, "FRESH METADATA FAIL");
                d.line(29, "RESTART WHEN DONE");
                d.flush();
                return ModemImageProbe;
            }
        }
    }
    paint_auth(&d, auth.report());
    if !auth.report().passed() {
        return ModemImageProbe;
    }
    let r = handshake.observe(&mut observer, baseline, auth, &mut clock, |r| {
        paint_handshake(&d, r)
    });
    paint_handshake(&d, &r);
    if r.status.ready_bit_seen {
        #[cfg(not(feature = "sunfish-glink-receive-probe"))]
        {
            let result = handshake
                .take_ready_observation()
                .and_then(|ready| pixel_glink_inventory::read(&mut observer, ready));
            paint_glink_inventory(&d, &result, &r);
        }
        #[cfg(feature = "sunfish-glink-receive-probe")]
        {
            let result = handshake
                .take_ready_observation()
                .and_then(|ready| pixel_glink_receive::read(&mut observer, ready));
            paint_glink_receive(&d, &result, &r);
        }
    }
    ModemImageProbe
}
#[cfg(any(
    feature = "sunfish-glink-boot-probe",
    feature = "sunfish-glink-version-probe"
))]
fn paint_glink_boot_failure(d: &Display, f: &pixel_glink_boot::Failure) {
    d.clear();
    d.line(0, "MODEM GLINK BOOT");
    d.line(1, "GLINK BOOT STOPPED");
    d.hex(2, "ERROR", f.error.code());
    d.hex(3, "AUTH ERROR", f.auth.error.map_or(0, |e| e.code()));
    d.hex(4, "ISSUED", f.auth.issued_mask);
    d.hex(5, "RETURNED", f.auth.returned_mask);
    d.hex(6, "KERNEL ERROR", f.auth.kernel_error.unwrap_or(0) as u32);
    if let Some(raw) = f.auth.replies[2] {
        d.hex(7, "MONITOR HI", (raw[0] >> 32) as u32);
        d.hex(8, "MONITOR LO", raw[0] as u32);
        d.hex(9, "SERVICE HI", (raw[1] >> 32) as u32);
        d.hex(10, "SERVICE LO", raw[1] as u32);
    }
    d.line(20, "NO GLINK PACKET SENT");
    d.line(21, "NO RETRY OR CLEANUP");
    d.line(23, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-version-probe")]
fn paint_glink_version(d: &Display, r: &pixel_glink_version::Report) {
    d.clear();
    d.line(0, "GLINK VERSION EXCHANGE");
    d.line(
        1,
        if r.passed() {
            "VERSION EXCHANGE PASS"
        } else if r.error.is_some() {
            "VERSION EXCHANGE STOPPED"
        } else {
            "WAITING FOR VERSION ACK"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "STAGING CHECK", r.staged as u32);
    d.hex(4, "HEAD STORE", r.head_published as u32);
    d.hex(5, "RX TAIL STORE", r.offer_consumed as u32);
    d.hex(6, "NOTIFICATIONS", r.notifications);
    d.hex(7, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(8, "ACK SEEN", r.ack_seen as u32);
    d.hex(9, "LAST TX TAIL", r.words[0]);
    d.hex(10, "LAST TX HEAD", r.words[1]);
    d.hex(11, "LAST RX TAIL", r.words[2]);
    d.hex(12, "LAST RX HEAD", r.words[3]);
    d.hex(13, "RX AVAILABLE", r.available);
    if let Some(h) = r.header {
        d.hex(14, "COMMAND", h.command as u32);
        d.hex(15, "PARAM ONE", h.param1 as u32);
        d.hex(16, "PARAM TWO", h.param2);
    }
    d.hex(17, "SAMPLES", r.samples);
    d.line(20, "TWO VERSION MESSAGES");
    d.line(21, "NO CHANNEL OPEN");
    d.line(22, "NO RETRY OR CLEANUP");
    d.line(23, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-pending-probe")]
fn paint_glink_pending(d: &Display, r: &pixel_glink_version::Report) {
    d.clear();
    d.line(0, "GLINK PENDING RX");
    d.line(
        1,
        if matches!(r.pending, Some(Ok(_))) {
            "RX SNAPSHOT PASS"
        } else {
            "RX SNAPSHOT STOPPED"
        },
    );
    d.hex(2, "VERSION ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(4, "ACK AT FRONT", r.ack_seen as u32);
    if let Some(result) = &r.pending {
        match result {
            Err(e) => d.hex(5, "SNAPSHOT ERROR", e.code()),
            Ok(s) => {
                d.hex(5, "SNAPSHOT ERROR", 0);
                d.hex(6, "RX AVAILABLE", s.available);
                d.hex(7, "RX CAPTURED", s.captured as u32);
                d.hex(8, "PARSED MESSAGES", s.parsed.count as u32);
                for i in 0..s.parsed.count.min(3) {
                    let p = &s.parsed.packets[i];
                    let row = 9 + i * 4;
                    d.hex(
                        row,
                        ["P0 COMMAND", "P1 COMMAND", "P2 COMMAND"][i],
                        p.header.command as u32,
                    );
                    d.hex(
                        row + 1,
                        ["P0 PARAM ONE", "P1 PARAM ONE", "P2 PARAM ONE"][i],
                        p.header.param1 as u32,
                    );
                    d.hex(
                        row + 2,
                        ["P0 PARAM TWO", "P1 PARAM TWO", "P2 PARAM TWO"][i],
                        p.header.param2,
                    );
                    if p.header.command == 2 {
                        // parse admits only printable ASCII and a final NUL.
                        // This fixed font displays capitals/digits only. Preserve
                        // character positions, uppercase letters and show other
                        // printable punctuation as spaces; fit the24-column row.
                        let n = p.name_len.min(21);
                        let mut line = [b' '; 24];
                        line[..3].copy_from_slice(b"P0 ");
                        line[1] = b'0' + i as u8;
                        for (j, b) in p.name[..n].iter().enumerate() {
                            line[3 + j] = b.to_ascii_uppercase();
                        }
                        if let Ok(name) = core::str::from_utf8(&line[..3 + n]) {
                            d.line(row + 3, name)
                        }
                    }
                }
                d.hex(21, "ACK IN SNAPSHOT", s.parsed.ack_in_prefix as u32);
                d.hex(22, "PARSE ERROR", s.parsed.error.map_or(0, |e| e.code()));
                d.hex(
                    23,
                    "QUEUE COMPLETE",
                    (s.parsed.error.is_none()
                        && !s.parsed.unknown_command
                        && s.parsed.parsed_bytes == s.captured
                        && s.captured as u32 == s.available) as u32,
                );
                d.hex(24, "UNKNOWN COMMAND", s.parsed.unknown_command as u32);
            }
        }
    }
    d.line(25, "RX TAIL UNCHANGED");
    d.line(26, "NO CHANNEL ACCEPTED");
    d.line(27, "NO EXTRA PACKET SENT");
    d.line(28, "NAMES UPPERCASE PREFIX");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-open-probe")]
fn paint_glink_open(d: &Display, r: &pixel_glink_open::Report) {
    d.clear();
    d.line(0, "IPCRTR OPEN REQUEST");
    d.line(
        1,
        if r.passed() {
            "IPCRTR REQUEST PASS"
        } else if r.error.is_some() {
            "IPCRTR REQUEST STOPPED"
        } else {
            "WAITING FOR OPEN ACK"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "STAGED", r.staged as u32);
    d.hex(4, "HEAD STORE", r.head_store as u32);
    d.hex(5, "NOTIFICATIONS", r.notifications);
    d.hex(6, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(7, "OPEN ACK SEEN", r.open_ack as u32);
    d.hex(8, "TX TAIL", r.words[0]);
    d.hex(9, "TX HEAD", r.words[1]);
    d.hex(10, "RX TAIL", r.words[2]);
    d.hex(11, "RX HEAD", r.words[3]);
    d.hex(12, "RX AVAILABLE", r.available);
    d.hex(13, "NEW RX BYTES", r.new_bytes);
    if let Some(p) = r.plan_shape {
        d.hex(14, "PLAN PACKET", p.packet);
        d.hex(15, "EXPECT OFFSET", p.expected_offset);
        d.hex(16, "SEEN OFFSET", p.offset);
    } else if let Some(h) = r.header {
        d.hex(14, "COMMAND", h.command as u32);
        d.hex(15, "PARAM ONE", h.param1 as u32);
        d.hex(16, "PARAM TWO", h.param2)
    }
    d.hex(17, "SAMPLES", r.samples);
    d.hex(18, "BYTE WRITES", r.byte_writes);
    d.hex(19, "GUARD STAGE", r.guard_stage);
    if let Some(p) = r.plan_shape {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for (index, packet) in p.packets.iter().enumerate() {
            let mut line = [b' '; 30];
            line[0] = b'P';
            line[1] = b'0' + index as u8;
            line[2] = b'@';
            for (at, value) in [
                (3, packet.offset),
                (7, packet.command),
                (11, packet.id),
                (15, packet.bytes),
            ] {
                line[at] = HEX[((value >> 4) & 0xf) as usize];
                line[at + 1] = HEX[(value & 0xf) as usize];
            }
            line[5] = b' ';
            line[6] = b'C';
            line[9] = b' ';
            line[10] = b'I';
            line[13] = b' ';
            line[14] = b'B';
            line[17] = b' ';
            let shown = (packet.name_len as usize).min(packet.name_prefix.len());
            line[18..18 + shown].copy_from_slice(&packet.name_prefix[..shown]);
            if let Ok(text) = core::str::from_utf8(&line) {
                d.line(20 + index, text);
            }
        }
        d.hex(24, "EXPECT ID", p.expected_id);
        d.hex(25, "SEEN ID", p.id);
        d.hex(26, "EXPECT BYTES", p.expected_name_bytes);
        d.hex(27, "SEEN BYTES", p.name_bytes);
    } else {
        d.line(20, "LOCAL CHANNEL 1");
        d.line(21, "REMOTE OFFER ID 2");
        d.line(22, "NO PEER OPEN ACK SENT");
        d.line(23, "NO RECEIVE CONSUMPTION");
        d.line(24, "NO INTENT ADVERTISED");
        d.line(25, "NO QRTR PAYLOAD");
        d.line(26, "NO WLAN OR HOST DMA");
        d.hex(27, "PLAN PACKET", 0);
    }
    d.hex(28, "PLAN DETAIL", r.plan_detail);
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-open-plan-snapshot")]
fn paint_glink_plan_snapshot(d: &Display, r: &pixel_glink_open::PlanSnapshotReport) {
    d.clear();
    d.line(0, "GLINK PLAN RX SNAPSHOT");
    d.line(1, "SNAPSHOT PASS");
    d.hex(2, "ERROR", 0);
    d.hex(3, "RX AVAILABLE", r.available);
    d.hex(4, "RX CAPTURED", r.captured);
    d.hex(5, "RX HEAD", r.words[3]);
    d.hex(6, "RX TAIL", r.words[2]);
    d.hex(7, "PLAN DETAIL", r.plan_detail);
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for row in 0..16 {
        let mut line = [b' '; 26];
        let offset = row * 8;
        line[0] = HEX[(offset >> 4) & 0xf];
        line[1] = HEX[offset & 0xf];
        for column in 0..8 {
            let at = offset + column;
            let start = 2 + column * 3;
            line[start] = b' ';
            if at < r.captured as usize {
                let value = r.bytes[at];
                line[start + 1] = HEX[(value >> 4) as usize];
                line[start + 2] = HEX[(value & 0xf) as usize];
            } else {
                line[start + 1] = b'-';
                line[start + 2] = b'-';
            }
        }
        if let Ok(text) = core::str::from_utf8(&line) {
            d.line(8 + row, text);
        }
    }
    d.line(24, "RX TAIL HELD");
    d.line(25, "NO OPEN TX WRITE");
    d.line(26, "NO OPEN HEAD STORE");
    d.line(27, "NO OPEN NOTIFY");
    d.line(28, "NO QMI OR DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(all(
    feature = "sunfish-glink-channel-probe",
    not(feature = "sunfish-glink-intents-probe")
))]
fn paint_glink_channel(d: &Display, r: &pixel_glink_channel::Report) {
    d.clear();
    d.line(0, "IPCRTR PEER ACK");
    d.line(
        1,
        if r.passed() {
            "IPCRTR ACK PASS"
        } else if r.error.is_some() {
            "IPCRTR ACK STOPPED"
        } else {
            "WAITING FOR ACK CONSUME"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "STAGED", r.staged as u32);
    d.hex(4, "HEAD STORE", r.head_store as u32);
    d.hex(5, "NOTIFICATIONS", r.notifications);
    d.hex(6, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(7, "TX TAIL", r.words[0]);
    d.hex(8, "TX HEAD", r.words[1]);
    d.hex(9, "RX TAIL", r.words[2]);
    d.hex(10, "RX HEAD", r.words[3]);
    d.hex(11, "RX AVAILABLE", r.available);
    d.hex(12, "NEW RX CAPTURED", r.new_bytes);
    if let Some(h) = r.header {
        d.hex(13, "COMMAND", h.command as u32);
        d.hex(14, "PARAM ONE", h.param1 as u32);
        d.hex(15, "PARAM TWO", h.param2)
    }
    d.hex(16, "CONTROL RECORDS", r.records);
    d.hex(17, "INTENT RECORDS", r.intent_records);
    d.hex(18, "INTENT ENTRIES", r.intent_entries);
    d.hex(19, "SAMPLES", r.samples);
    d.hex(20, "BYTE WRITES", r.byte_writes);
    d.hex(21, "GUARD STAGE", r.guard_stage);
    d.line(22, "ACK REMOTE CHANNEL 2");
    d.line(23, "RX TAIL UNCHANGED");
    d.line(24, "NO INTENT ADVERTISED");
    d.line(25, "NO INTENT IDS USED");
    d.line(26, "NO QRTR PAYLOAD");
    d.line(27, "NO WLAN OR HOST DMA");
    d.line(28, "BOUNDED CONTROL SNAPSHOT");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-intents-probe")]
fn paint_glink_channel(d: &Display, r: &pixel_glink_channel::Report) {
    let complete = r.new_bytes == r.available.saturating_sub(88);
    d.clear();
    d.line(0, "IPCRTR INTENT DETAILS");
    d.line(
        1,
        if r.passed() && r.intents.is_some() {
            if complete {
                "INTENT INVENTORY PASS"
            } else {
                "INTENT PREFIX PASS"
            }
        } else if r.error.is_some() {
            "INTENT SCAN STOPPED"
        } else {
            "CAPTURING INTENT DETAILS"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(4, "RX HEAD", r.words[3]);
    d.hex(5, "NEW RX PENDING", r.available.saturating_sub(88));
    d.hex(6, "NEW RX CAPTURED", r.new_bytes);
    d.hex(
        8,
        "QUEUE COMPLETE",
        (r.passed() && r.intents.is_some() && complete) as u32,
    );
    if let Some(s) = r.intent_scan {
        let i = s.inventory;
        d.hex(7, "PARSED BYTES", s.parsed_bytes as u32);
        d.hex(9, "CONTROL RECORDS", i.records);
        d.hex(10, "INTENT ENTRIES", i.entries);
        d.hex(11, "SIZE GROUPS", i.group_count as u32);
        d.hex(12, "INTENT REQUESTS", i.intent_requests);
        d.hex(13, "REQUEST SIZE", i.last_request_size);
        if s.error.is_some() {
            d.hex(14, "STOP DETAIL", s.detail);
            d.hex(15, "STOP OFFSET", s.stop_offset as u32);
            if let Some(h) = s.stop_header {
                d.hex(16, "COMMAND", h.command as u32);
                d.hex(17, "PARAM ONE", h.param1 as u32);
                d.hex(18, "PARAM TWO", h.param2)
            }
            if s.preview_words >= 3 {
                d.hex(19, "WORD08", s.stop_words[2])
            }
            if s.preview_words >= 4 {
                d.hex(20, "WORD0C", s.stop_words[3])
            }
        } else {
            d.hex(14, "MIN INTENT ID", i.min_id);
            d.hex(15, "MAX INTENT ID", i.max_id);
            d.hex(16, "DUPLICATE IDS", i.duplicate_ids);
            d.hex(17, "ZERO SIZES", i.zero_sizes);
            d.hex(18, "SIGNAL BITS", i.signal_bits);
            d.hex(19, "INTENT RECORDS", i.intent_records);
            d.hex(20, "READ NOTIFICATIONS", i.read_notifications);
        }
        for (n, g) in i.groups[..i.group_count.min(5)].iter().enumerate() {
            let mut line = *b"G0 00000000 N 00000000";
            line[1] = b'0' + n as u8;
            for (j, v) in [g.size, g.count].into_iter().enumerate() {
                for k in 0..8 {
                    line[if j == 0 { 3 + k } else { 14 + k }] =
                        b"0123456789ABCDEF"[((v >> (28 - 4 * k)) & 15) as usize];
                }
            }
            if let Ok(s) = core::str::from_utf8(&line) {
                d.line(21 + n, s)
            }
        }
    } else {
        d.hex(9, "GUARD STAGE", r.guard_stage);
        d.hex(10, "BYTE WRITES", r.byte_writes);
        d.hex(11, "HEAD STORE", r.head_store as u32);
        d.hex(12, "NOTIFICATIONS", r.notifications);
        if let Some(h) = r.header {
            d.hex(13, "FIRST COMMAND", h.command as u32);
            d.hex(14, "FIRST PARAM ONE", h.param1 as u32);
            d.hex(15, "FIRST PARAM TWO", h.param2)
        }
    }
    d.line(26, "RX TAIL8 UNCHANGED");
    d.line(27, "NO INTENT ID SELECTED");
    d.line(28, "NO RESPONSE OR QRTR DATA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-rx-packet-probe")]
fn paint_glink_rx(d: &Display, r: &pixel_glink_rx::Report) {
    d.clear();
    d.line(0, "IPCRTR FIRST RECEIVE");
    d.line(
        1,
        if r.passed() {
            "RECEIVE COPY PASS"
        } else if r.error.is_some() {
            "RECEIVE STOPPED"
        } else {
            "WAITING FOR ONE PACKET"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "GUARD STAGE", r.stage);
    d.hex(4, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(5, "TX HEAD", r.words[1]);
    d.hex(6, "RX HEAD", r.words[3]);
    d.hex(7, "RX AVAILABLE", r.available);
    if let Some(h) = r.header {
        d.hex(8, "DATA COMMAND", h.command as u32);
        d.hex(9, "REMOTE CID", h.param1 as u32);
        d.hex(10, "LOCAL INTENT", h.param2)
    }
    d.hex(11, "CHUNK BYTES", r.chunk);
    d.hex(12, "LEFT BYTES", r.left);
    d.hex(13, "COPIED BYTES", r.copied);
    d.hex(14, "BUFFER GUARDS", r.guards as u32);
    d.hex(15, "QRTR V1 SHAPE", r.qrtr_v1 as u32);
    if r.copied == 52 {
        for (i, label) in [
            "QRTR VERSION",
            "QRTR TYPE",
            "SRC NODE",
            "SRC PORT",
            "CONFIRM RX",
            "QRTR BODY BYTES",
            "DST NODE",
            "DST PORT",
        ]
        .into_iter()
        .enumerate()
        {
            d.hex(16 + i, label, r.qrtr_words[i]);
        }
        d.hex(24, "BODY WORD0", r.control_word);
    } else {
        d.hex(16, "BYTE WRITES", r.byte_writes);
        d.hex(17, "HEAD STORE", r.head_store as u32);
        d.hex(18, "NOTIFICATIONS", r.notifications);
        d.hex(19, "SAMPLES", r.samples);
    }
    d.line(25, "LOCAL BUFFER64 RESERVED");
    d.line(26, "RX TAIL8 UNCHANGED");
    d.line(27, "NO RX DONE OR QRTR TX");
    d.line(28, "NO HOST DMA OR RADIO");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-done-probe")]
fn paint_glink_done(d: &Display, r: &pixel_glink_done::Report) {
    d.clear();
    d.line(0, "IPCRTR RECEIVE DONE");
    d.line(
        1,
        if r.passed() {
            "RXDONE CONSUMED PASS"
        } else if r.error.is_some() {
            "RXDONE STOPPED"
        } else {
            "WAITING FOR RXDONE"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "GUARD STAGE", r.stage);
    d.hex(4, "BYTE WRITES", r.byte_writes);
    d.hex(5, "RX TAIL STORE", r.tail_store as u32);
    d.hex(6, "TX HEAD STORE", r.head_store as u32);
    d.hex(7, "NOTIFICATIONS", r.notifications);
    d.hex(8, "TX CONSUMED", r.tx_consumed as u32);
    for (i, label) in ["TX TAIL", "TX HEAD", "RX TAIL", "RX HEAD"]
        .into_iter()
        .enumerate()
    {
        d.hex(9 + i, label, r.words[i]);
    }
    d.hex(13, "RX AVAILABLE", r.available);
    d.hex(14, "BUFFER GUARDS", r.guards as u32);
    d.hex(15, "INTENT RETIRED", r.retired as u32);
    d.hex(16, "SAMPLES", r.samples);
    d.line(22, "ONE NONREUSABLE RXDONE");
    d.line(23, "PRIVATE BUFFER RESERVED");
    d.line(24, "NO NEW INTENT");
    d.line(25, "NO QRTR TX");
    d.line(26, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-qrtr-hello-probe")]
fn paint_qrtr_hello(d: &Display, r: &pixel_qrtr_hello::Report) {
    d.clear();
    d.line(0, "IPCRTR QRTR HELLO");
    d.line(
        1,
        if r.passed() {
            "HELLO TRANSPORT PASS"
        } else if r.error.is_some() {
            "HELLO TRANSPORT STOPPED"
        } else {
            "WAITING FOR HELLO DONE"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "GUARD STAGE", r.stage);
    d.hex(4, "PEER INTENT", 0);
    d.hex(5, "PEER CAPACITY", r.capacity);
    d.hex(6, "INTENT RESERVED", r.reserved as u32);
    d.hex(7, "BYTE WRITES", r.byte_writes);
    d.hex(8, "HEAD STORE", r.head_store as u32);
    d.hex(9, "NOTIFICATIONS", r.notifications);
    d.hex(10, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(11, "DONE SEEN", r.done_seen as u32);
    d.hex(12, "DONE COMMAND", r.done_command);
    for (i, label) in ["TX TAIL", "TX HEAD", "RX TAIL", "RX HEAD"]
        .into_iter()
        .enumerate()
    {
        d.hex(13 + i, label, r.words[i]);
    }
    d.hex(17, "RX AVAILABLE", r.available);
    d.hex(18, "RX CAPTURED", r.captured);
    d.hex(19, "QUEUE COMPLETE", r.complete as u32);
    d.hex(20, "BUFFER GUARDS", r.guards as u32);
    if r.error.is_some() {
        d.hex(21, "STOP OFFSET", r.stop_offset);
        if let Some(h) = r.header {
            d.hex(22, "COMMAND", h.command as u32);
            d.hex(23, "PARAM ONE", h.param1 as u32);
            d.hex(24, "PARAM TWO", h.param2)
        }
    } else {
        d.hex(21, "CONTROL RECORDS", r.records);
        d.hex(22, "INTENT REQUESTS", r.requests);
        d.hex(23, "REQUEST SIZE", r.request_size);
        d.hex(24, "NEW INTENT ENTRIES", r.advertised);
    }
    d.line(25, "RX TAIL290 UNCHANGED");
    d.line(26, "NO INTENT REUSE");
    d.line(27, "NO SERVICE OR WLAN QMI");
    d.line(28, "NO HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-qrtr-next-probe")]
fn paint_next_pair(d: &Display, row: usize, label: &[u8], one: u32, tag: u8, two: u32) {
    let mut line = [b' '; 30];
    let a = label.len() + 1;
    let b = a + 11;
    assert!(b + 8 <= line.len());
    line[..label.len()].copy_from_slice(label);
    line[a + 9] = tag;
    for i in 0..8 {
        line[a + i] = b"0123456789ABCDEF"[((one >> (28 - 4 * i)) & 15) as usize];
        line[b + i] = b"0123456789ABCDEF"[((two >> (28 - 4 * i)) & 15) as usize];
    }
    if let Ok(s) = core::str::from_utf8(&line[..b + 8]) {
        d.line(row, s)
    }
}
#[cfg(feature = "sunfish-qrtr-next-probe")]
fn paint_qrtr_next(d: &Display, r: &pixel_qrtr_next::Report) {
    d.clear();
    d.line(0, "QRTR NEXT RECEIVE");
    d.line(
        1,
        if r.passed() {
            "NEXT PACKET COPY PASS"
        } else if r.error.is_some() {
            "NEXT RECEIVE STOPPED"
        } else {
            "WAITING FOR NEXT PACKET"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "GUARD STAGE", r.stage);
    d.hex(4, "TX CONSUMED", r.tx_consumed as u32);
    d.hex(5, "TX HEAD", r.words[1]);
    d.hex(6, "RX HEAD", r.words[3]);
    d.hex(7, "RX AVAILABLE", r.available);
    if let Some(h) = r.header {
        d.hex(8, "DATA COMMAND", h.command as u32);
        d.hex(9, "REMOTE CID", h.param1 as u32);
        d.hex(10, "LOCAL INTENT", h.param2)
    }
    d.hex(11, "CHUNK BYTES", r.chunk);
    d.hex(12, "LEFT BYTES", r.left);
    d.hex(13, "COPIED BYTES", r.copied);
    d.hex(14, "BUFFER GUARDS", r.guards as u32);
    d.hex(15, "FIRST GUARDS", r.first_guards as u32);
    d.hex(16, "QRTR V1 SHAPE", r.qrtr_v1 as u32);
    if r.copied == 52 {
        paint_next_pair(d, 17, b"TYPE", r.qrtr_words[1], b'C', r.qrtr_words[4]);
        paint_next_pair(d, 18, b"SRC", r.qrtr_words[2], b'P', r.qrtr_words[3]);
        paint_next_pair(d, 19, b"DST", r.qrtr_words[6], b'P', r.qrtr_words[7]);
        d.hex(20, "QRTR BODY BYTES", r.qrtr_words[5]);
        for (i, label) in [
            "BODY WORD0",
            "BODY WORD1",
            "BODY WORD2",
            "BODY WORD3",
            "BODY WORD4",
        ]
        .into_iter()
        .enumerate()
        {
            d.hex(21 + i, label, r.body_words[i]);
        }
    } else {
        d.hex(17, "BYTE WRITES", r.byte_writes);
        d.hex(18, "HEAD STORE", r.head_store as u32);
        d.hex(19, "NOTIFICATIONS", r.notifications);
        d.hex(20, "SAMPLES", r.samples);
    }
    d.line(26, "RX TAIL290 UNCHANGED");
    d.line(27, "NO RXDONE OR INTENT REUSE");
    d.line(28, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-boot-probe")]
fn paint_glink_boot(
    d: &Display,
    result: &Result<pixel_glink_boot::Report, personal_os_rust_server::qcom_modem_status::Error>,
) {
    d.clear();
    d.line(0, "MODEM GLINK BOOT");
    d.line(
        1,
        if result.is_ok() {
            "BOOT MEMORY PASS"
        } else {
            "BOOT MEMORY STOPPED"
        },
    );
    d.hex(2, "ERROR", result.as_ref().err().map_or(0, |e| e.code()));
    if let Ok(r) = result {
        let s = &r.receive;
        d.hex(3, "PRESENT MASK", s.inventory.present_mask());
        d.hex(
            4,
            "SLAVE KERNEL",
            s.inventory.status.slave_kernel.unwrap_or(u32::MAX),
        );
        for (i, label) in ["TX TAIL", "TX HEAD", "RX TAIL", "RX HEAD"]
            .into_iter()
            .enumerate()
        {
            d.hex(5 + i, label, s.words[i]);
        }
        d.hex(9, "RX AVAILABLE", s.available);
        d.hex(10, "TX ZERO PASSES", r.tx_zero_passes);
        d.hex(11, "HEADER PRESENT", s.header.is_some() as u32);
        if let Some(h) = s.header {
            d.hex(12, "COMMAND", h.command as u32);
            d.hex(13, "PARAM ONE", h.param1 as u32);
            d.hex(14, "PARAM TWO", h.param2);
        }
        for (i, label) in ["DESC OFFSET", "TX OFFSET", "RX OFFSET"]
            .into_iter()
            .enumerate()
        {
            d.hex(
                15 + i,
                label,
                s.inventory.items[i].map_or(0, |v| v.offset as u32),
            );
        }
    }
    d.line(20, "NO GLINK PACKET SENT");
    d.line(21, "NO POST AUTH WRITES");
    d.line(22, "NO INDEX RESET");
    d.line(23, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-prepare-probe")]
fn paint_glink_prepare(
    d: &Display,
    result: &Result<pixel_glink_prepare::Report, personal_os_rust_server::qcom_modem_status::Error>,
) {
    d.clear();
    d.line(0, "MODEM GLINK PREPARE");
    d.line(
        1,
        if result.is_ok() {
            "GLINK MEMORY PREPARE PASS"
        } else {
            "GLINK MEMORY STOPPED"
        },
    );
    d.hex(2, "ERROR", result.as_ref().err().map_or(0, |e| e.code()));
    if let Ok(r) = result {
        d.hex(3, "DESC OFFSET", r.descriptor.offset as u32);
        d.hex(4, "DESC SIZE", r.descriptor.size as u32);
        d.hex(5, "TX OFFSET", r.tx.offset as u32);
        d.hex(6, "TX SIZE", r.tx.size as u32);
        d.hex(7, "FREE BEFORE", r.free_before);
        d.hex(8, "FREE AFTER", r.free_after);
        d.hex(9, "BYTES WRITTEN", r.written);
        d.hex(10, "GUARD BYTES", r.guard_bytes);
        d.hex(11, "VERIFY PASSES", r.verification_passes);
    }
    d.line(19, "AUTH SESSION CONSUMED");
    d.line(20, "NO AUTH OR RESET");
    d.line(21, "NO GLINK PACKET SENT");
    d.line(22, "NO MODEM START");
    d.line(23, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
fn show_status_error(d: &Display, label: &str, error: u32) {
    d.clear();
    d.line(0, "MODEM STATUS CHECK");
    d.line(1, label);
    d.hex(2, "ERROR", error);
    d.line(20, "AUTH NOT CALLED");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
fn paint_ap_prepare(d: &Display, h: &pixel_smp2p_handshake::Handshake, label: &str, error: u32) {
    d.clear();
    d.line(0, "MODEM AP STATUS PREPARE");
    d.line(1, label);
    d.hex(2, "ERROR", error);
    d.hex(3, "AP PUBLISHED", h.prepared() as u32);
    d.hex(4, "AP OFFSET", h.ap_offset());
    if let Some(dir) = h.last_directory() {
        d.hex(5, "PART OFFSET", dir.route.offset as u32);
        d.hex(6, "LAST FREE U", dir.free_uncached as u32);
        d.hex(7, "LAST FREE C", dir.free_cached as u32);
        d.hex(8, "PART SIZE", dir.route.size as u32);
    }
    d.line(20, "AUTH NOT CALLED");
    d.line(21, "NO STATUS NOTIFICATION");
    d.line(22, "ONE AP PAGE ONLY");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(not(feature = "sunfish-glink-receive-probe"))]
fn paint_glink_inventory(
    d: &Display,
    result: &Result<
        pixel_glink_inventory::Report,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    prior: &pixel_smp2p_handshake::Report,
) {
    d.clear();
    d.line(0, "MODEM GLINK DIRECTORY");
    d.line(
        1,
        if result.is_ok() {
            "DIRECTORY SNAPSHOT PASS"
        } else {
            "DIRECTORY SNAPSHOT FAIL"
        },
    );
    d.hex(2, "ERROR", result.as_ref().err().map_or(0, |e| e.code()));
    d.hex(3, "PRIOR READY", prior.status.ready_bit_seen as u32);
    d.hex(4, "AP PUBLISHED", prior.published as u32);
    d.hex(5, "AP OFFSET", prior.ap_offset);
    if let Ok(r) = result {
        d.hex(6, "PRESENT MASK", r.present_mask());
        d.hex(7, "METADATA PASSES", r.metadata_passes);
        d.hex(8, "FREE U", r.directory.free_uncached as u32);
        d.hex(9, "FREE C", r.directory.free_cached as u32);
        for (i, labels) in [
            ["DESC OFFSET", "DESC SIZE"],
            ["TX OFFSET", "TX SIZE"],
            ["RX OFFSET", "RX SIZE"],
        ]
        .into_iter()
        .enumerate()
        {
            d.hex(
                10 + 2 * i,
                labels[0],
                r.items[i].map_or(0, |v| v.offset as u32),
            );
            d.hex(
                11 + 2 * i,
                labels[1],
                r.items[i].map_or(0, |v| v.size as u32),
            );
        }
        d.hex(16, "VALID ENTRIES", r.status.valid_entries as u32);
        d.hex(
            17,
            "SLAVE KERNEL",
            r.status.slave_kernel.unwrap_or(u32::MAX),
        );
    }
    d.line(20, "GLINK PAYLOAD UNREAD");
    d.line(21, "NO GLINK WRITES");
    d.line(22, "NO WLAN QMI");
    d.line(23, "NO HOST DMA SETUP");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-glink-receive-probe")]
fn paint_glink_receive(
    d: &Display,
    result: &Result<pixel_glink_receive::Report, personal_os_rust_server::qcom_modem_status::Error>,
    prior: &pixel_smp2p_handshake::Report,
) {
    d.clear();
    d.line(0, "MODEM GLINK RECEIVE");
    d.line(
        1,
        match result {
            Ok(r) if r.header.is_some_and(|h| h.version_offer()) => "VERSION OFFER SEEN",
            Ok(r) if r.header.is_none() => "RX EMPTY SNAPSHOT PASS",
            Ok(_) => "RX HEADER SNAPSHOT PASS",
            Err(_) => "RX SNAPSHOT FAIL",
        },
    );
    d.hex(2, "ERROR", result.as_ref().err().map_or(0, |e| e.code()));
    d.hex(3, "PRIOR READY", prior.status.ready_bit_seen as u32);
    if let Ok(r) = result {
        d.hex(4, "PRESENT MASK", r.inventory.present_mask());
        d.hex(5, "TX TAIL", r.words[0]);
        d.hex(6, "TX HEAD", r.words[1]);
        d.hex(7, "RX TAIL", r.words[2]);
        d.hex(8, "RX HEAD", r.words[3]);
        d.hex(9, "RX AVAILABLE", r.available);
        d.hex(10, "HEADER PRESENT", r.header.is_some() as u32);
        if let Some(h) = r.header {
            d.hex(11, "COMMAND", h.command as u32);
            d.hex(12, "PARAM ONE", h.param1 as u32);
            d.hex(13, "PARAM TWO", h.param2);
        }
        d.hex(14, "CONTENT PASSES", r.content_passes);
        d.hex(
            15,
            "SLAVE KERNEL",
            r.inventory.status.slave_kernel.unwrap_or(u32::MAX),
        );
        d.hex(
            16,
            "DESC OFFSET",
            r.inventory.items[0].map_or(0, |v| v.offset as u32),
        );
        d.hex(
            17,
            "RX OFFSET",
            r.inventory.items[2].map_or(0, |v| v.offset as u32),
        );
    }
    d.line(20, "NO GLINK WRITES");
    d.line(21, "NO PACKET CONSUMED");
    d.line(22, "NO VERSION ACK SENT");
    d.line(23, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
fn paint_handshake(d: &Display, r: &pixel_smp2p_handshake::Report) {
    let s = &r.status;
    d.clear();
    d.line(0, "MODEM STATUS HANDSHAKE");
    d.line(
        1,
        if s.ready_bit_seen {
            "READY BIT SEEN"
        } else if s.error.is_some() {
            "HANDSHAKE STOPPED"
        } else {
            "WAITING FOR STATUS"
        },
    );
    d.hex(2, "ERROR", s.error.map_or(0, |e| e.code()));
    d.hex(3, "SAMPLES", s.samples);
    d.hex(4, "AUTH RETURNED", if s.auth_passed { 7 } else { 0 });
    d.hex(5, "AP PUBLISHED", r.published as u32);
    d.hex(6, "AP OFFSET", r.ap_offset);
    d.hex(7, "DOORBELLS", r.doorbells);
    d.hex(8, "PEER NEGOTIATED", r.peer_negotiated as u32);
    if let Some(dire) = s.directory {
        d.hex(9, "FREE U", dire.free_uncached as u32);
        d.hex(10, "FREE C", dire.free_cached as u32);
        d.hex(11, "ITEM OFFSET", dire.item.map_or(0, |i| i.offset as u32));
        d.hex(12, "ITEM SIZE", dire.item.map_or(0, |i| i.size as u32));
    }
    if let Some(status) = s.status {
        d.hex(13, "VALID ENTRIES", status.valid_entries as u32);
        d.hex(14, "FEATURES", status.features);
        d.hex(15, "FLAGS", status.flags);
        d.hex(16, "SLAVE KERNEL", status.slave_kernel.unwrap_or(u32::MAX));
    }
    d.hex(17, "ELAPSED LO", s.elapsed_ticks as u32);
    d.hex(18, "FREQUENCY", s.frequency as u32);
    d.line(20, "ONE AP STATUS ITEM");
    d.line(21, "SSR FEATURE DISABLED");
    d.line(22, "NO GLINK OR WLAN QMI");
    d.line(23, "NO HOST DMA SETUP");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
fn paint_auth(d: &Display, r: &AuthReport) {
    d.clear();
    d.line(0, "MODEM AUTH TEST");
    d.line(
        1,
        if r.phase == personal_os_rust_server::qcom_pas_auth::Phase::Returned && !r.passed() {
            "AUTH RESULT FAIL"
        } else {
            r.phase.label()
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "ISSUED", r.issued_mask);
    d.hex(4, "RETURNED", r.returned_mask);
    d.hex(
        5,
        "LAST FN",
        r.last_request.map_or(0, |r| r.words()[0]) as u32,
    );
    d.hex(6, "KERNEL ERROR", r.kernel_error.unwrap_or(0) as u32);
    d.hex(7, "IMAGE LOADED", r.image.loaded_mask);
    d.hex(8, "VERIFIED ONE", r.image.verified_masks[0]);
    d.hex(9, "VERIFIED TWO", r.image.verified_masks[1]);
    if let Some(raw) = r.replies.iter().rev().flatten().next() {
        d.hex(11, "MONITOR HI", (raw[0] >> 32) as u32);
        d.hex(12, "MONITOR LO", raw[0] as u32);
        d.hex(13, "SERVICE HI", (raw[1] >> 32) as u32);
        d.hex(14, "SERVICE LO", raw[1] as u32);
    }
    d.line(19, "SECURE RESULT ONLY");
    d.line(20, "MODEM READY UNCHECKED");
    d.line(21, "NO WLAN QMI");
    d.line(22, "NO HOST DMA SETUP");
    d.line(23, "HELD VOTES KEPT");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
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
    d.line(23, "AUTH NOT CALLED YET");
    d.line(24, "NO MODEM START");
    d.line(25, "NO DMA OR RADIO");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
struct ModemImageProbe;
impl Handler for ModemImageProbe {
    type Error = Infallible;
    // The library's default run() ACKs every IRQ after init unconditionally.
    // Owner.start() must control even that first unmask, including failed init.
    // This manifest is active and this handler uses no deferred kernel actions.
    #[cfg(feature = "sunfish-modem-owner")]
    fn run(&mut self) -> Result<sel4_microkit::Never, Self::Error> {
        use sel4_microkit::ipc::{self, Event};
        let mut reply = None;
        loop {
            let event = match reply.take() {
                Some(message) => ipc::reply_recv(message),
                None => ipc::recv(),
            };
            match event {
                Event::Notified(channels) => self.notified(channels)?,
                Event::Protected(channel, message) => {
                    reply = Some(self.protected(channel, message)?);
                }
                Event::Fault(child, message) => reply = self.fault(child, message)?,
            }
        }
    }
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        #[cfg(feature = "sunfish-modem-owner")]
        if let Some(owner) = unsafe { (&mut *core::ptr::addr_of_mut!(MODEM_OWNER)).as_mut() } {
            owner.notified(channels);
        }
        #[cfg(not(feature = "sunfish-modem-owner"))]
        let _ = channels;
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}

#[cfg(feature = "sunfish-qrtr-discovery-probe")]
fn paint_qrtr_discovery(d: &Display, r: &pixel_qrtr_discovery::Report) {
    d.clear();
    d.line(
        0,
        if cfg!(feature = "sunfish-qrtr-wait-probe") {
            "QRTR MEASURED WAIT"
        } else if cfg!(feature = "sunfish-qrtr-credit-probe") {
            "QRTR CREDIT INVENTORY"
        } else {
            "QRTR SERVICE INVENTORY"
        },
    );
    d.line(
        1,
        if r.passed() && r.found {
            "WLFW ANNOUNCEMENT SEEN"
        } else if r.passed() {
            "DISCOVERY LIMIT"
        } else if r.error.is_some() {
            "DISCOVERY STOPPED"
        } else {
            "DISCOVERY RUNNING"
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "GUARD STAGE", r.stage);
    d.hex(4, "RECORDS", r.count as u32);
    d.hex(5, "LATEST SERVICE", r.latest.service);
    d.hex(6, "LATEST INSTANCE", r.latest.instance);
    d.hex(7, "LATEST NODE", r.latest.node);
    d.hex(8, "LATEST PORT", r.latest.port);
    d.hex(9, "WLFW SEEN", r.found as u32);
    d.hex(10, "WLFW INSTANCE", r.target.instance);
    d.hex(11, "WLFW NODE", r.target.node);
    d.hex(12, "WLFW PORT", r.target.port);
    for (i, label) in ["TX TAIL", "TX HEAD", "RX TAIL", "RX HEAD"]
        .into_iter()
        .enumerate()
    {
        d.hex(13 + i, label, r.words[i]);
    }
    d.hex(17, "RX AVAILABLE", r.available);
    d.hex(18, "NEW COPIES", r.copied);
    d.hex(19, "DONE CONSUMED", r.done_consumed);
    d.hex(20, "FRESH INTENTS", r.advertised);
    if cfg!(feature = "sunfish-qrtr-credit-probe") {
        d.hex(21, "REQUEST ACKS", r.requests)
    } else {
        d.hex(21, "NOTIFICATIONS", r.notifications)
    }
    d.hex(22, "OLD GUARDS", r.old_guards as u32);
    d.hex(23, "POOL GUARDS", r.pool_guards as u32);
    if let Some(h) = r.pending {
        d.hex(24, "PENDING COMMAND", h.command as u32);
        d.hex(25, "PENDING PARAM1", h.param1 as u32);
        d.hex(26, "PENDING PARAM2", h.param2)
    } else if cfg!(feature = "sunfish-qrtr-wait-probe") {
        d.hex(24, "DATA POLLS", r.data_samples);
        d.hex(25, "CLOCK HZ", r.frequency);
        d.hex(26, "LAST SLAVE", r.slave_kernel)
    }
    if cfg!(feature = "sunfish-qrtr-wait-probe") {
        d.hex(27, "LAST POLL MS", r.elapsed_ms)
    } else {
        d.hex(27, "QRTR BODY COMMAND", r.body_command)
    }
    d.line(28, "NO SERVICE QMI OR DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}

#[cfg(feature = "sunfish-qrtr-servreg-probe")]
fn paint_qrtr_servreg(d: &Display, r: &pixel_qrtr_servreg::Report) {
    d.clear();
    if cfg!(feature = "sunfish-qrtr-servreg-pdr-followup") && r.followup_done_consumed {
        d.line(0, "QRTR PDR AFTER DONE");
        d.line(
            1,
            if r.error.is_some() {
                "NEXT RX STOPPED"
            } else {
                "RX DONE PASS"
            },
        );
        d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
        d.hex(3, "DONE INTENT", r.followup_done_intent);
        d.hex(4, "RX AVAILABLE", r.post_rx_available);
        d.hex(5, "RX CAPTURED", r.post_rx_captured);
        d.hex(6, "RX HEAD", r.post_rx_head);
        d.hex(7, "RX TAIL", r.rx_tail);
        d.line(8, "POST-DONE RX WORDS");
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for row in 0..8 {
            let first = row * 2;
            let second = first + 1;
            let mut line = [b' '; 23];
            line[0] = b'0' + (first / 10) as u8;
            line[1] = b'0' + (first % 10) as u8;
            line[2] = b' ';
            for digit in 0..8 {
                line[3 + digit] =
                    HEX[((r.post_rx_words[first] >> (28 - digit * 4)) & 0xf) as usize];
            }
            line[11] = b' ';
            line[12] = b'0' + (second / 10) as u8;
            line[13] = b'0' + (second % 10) as u8;
            line[14] = b' ';
            for digit in 0..8 {
                line[15 + digit] =
                    HEX[((r.post_rx_words[second] >> (28 - digit * 4)) & 0xf) as usize];
            }
            if let Ok(text) = core::str::from_utf8(&line) {
                d.line(9 + row, text);
            }
        }
        d.line(
            18,
            if r.post_rx_available == 0 {
                "NO NEXT RX BYTES"
            } else {
                "NEXT RX TAIL HELD"
            },
        );
        d.line(19, "NO THIRD QMI REPLY");
        d.line(20, "NO WLAN OR HOST DMA");
        d.line(21, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if cfg!(feature = "sunfish-qrtr-servreg-pdr-followup") && r.response_consumed {
        d.line(0, "QRTR PDR NEXT PACKET");
        d.line(
            1,
            if r.error.is_some() {
                "FOLLOW-UP STOPPED"
            } else if r.followup_response_consumed {
                "PDDUMP REPLY PASS"
            } else if r.followup_query_valid {
                "PDDUMP QUERY MATCH"
            } else if r.followup_packet_valid {
                "PACKET SNAPSHOT PASS"
            } else if r.post_rx_captured > 0 {
                "RX SNAPSHOT ONLY"
            } else if r.followup_tx_consumed {
                "INTENT PASS NO DATA"
            } else if r.followup_intent_offered {
                "INTENT OFFERED"
            } else {
                "CONTROL CHECK STOPPED"
            },
        );
        d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
        d.hex(3, "RESPONSE CONSUMED", r.response_consumed as u32);
        d.hex(4, "PDDUMP MATCH", r.followup_query_valid as u32);
        d.hex(5, "QMI TRANSACTION", r.followup_transaction);
        d.hex(6, "REQUEST PORT", r.followup_request_port);
        d.hex(7, "REPLY STAGED", r.followup_response_staged as u32);
        d.hex(8, "REPLY CONSUMED", r.followup_response_consumed as u32);
        d.hex(9, "RX AVAILABLE", r.post_rx_available);
        d.hex(10, "RX CAPTURED", r.post_rx_captured);
        d.hex(11, "RX TAIL", r.rx_tail);
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let word_rows = if r.followup_response_consumed {
            8
        } else if r.followup_packet_valid {
            10
        } else {
            2
        };
        let word_at = |index: usize| {
            if r.followup_response_consumed {
                r.post_rx_words.get(index).copied().unwrap_or(0)
            } else {
                r.followup_words.get(index).copied().unwrap_or(0)
            }
        };
        for row in 0..word_rows {
            let first = row * 2;
            let second = first + 1;
            let mut line = [b' '; 23];
            line[0] = b'0' + (first / 10) as u8;
            line[1] = b'0' + (first % 10) as u8;
            line[2] = b' ';
            for digit in 0..8 {
                line[3 + digit] = HEX[((word_at(first) >> (28 - digit * 4)) & 0xf) as usize];
            }
            line[11] = b' ';
            line[12] = b'0' + (second / 10) as u8;
            line[13] = b'0' + (second % 10) as u8;
            line[14] = b' ';
            for digit in 0..8 {
                line[15 + digit] = HEX[((word_at(second) >> (28 - digit * 4)) & 0xf) as usize];
            }
            if let Ok(text) = core::str::from_utf8(&line) {
                d.line(12 + row, text);
            }
        }
        if r.followup_response_consumed {
            d.line(
                20,
                if r.post_rx_captured > 0 {
                    "NEXT RX TAIL HELD"
                } else {
                    "NO NEXT RX BYTES"
                },
            );
        } else {
            d.hex(22, "CHUNK BYTES", r.followup_chunk_bytes);
            d.hex(23, "LEFT BYTES", r.followup_left_bytes);
        }
        d.line(24, "NO THIRD QMI REPLY");
        d.line(25, "NO WLAN OR HOST DMA");
        d.line(26, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if r.query_check_pass {
        d.line(0, "QRTR SERVREG QUERY");
        d.line(1, "QMI QUERY PASS");
        d.hex(2, "ERROR", 0);
        d.hex(3, "RX AVAILABLE", r.rx_available);
        d.hex(4, "RX CAPTURED", r.rx_captured);
        d.hex(5, "REMOTE PORT", r.request_port);
        d.hex(6, "QMI TRANSACTION", r.transaction);
        d.hex(
            7,
            "QMI MESSAGE ID",
            pixel_servreg_wire::GET_DOMAIN_LIST_MESSAGE_ID as u32,
        );
        d.hex(8, "QMI TYPE", 0);
        d.hex(9, "QRTR BYTES", pixel_servreg_wire::QUERY_BYTES as u32);
        d.hex(10, "QMI BYTES", 21);
        d.hex(11, "LOCAL INTENT", r.query_intent_id);
        d.hex(12, "RX TAIL", r.rx_tail);
        d.line(18, "NO QUERY CONSUME");
        d.line(19, "NO QMI RESPONSE");
        d.line(20, "NO WLAN OR HOST DMA");
        d.line(21, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if r.prequery_check_pass {
        d.line(0, "QRTR PREQUERY CHECK");
        d.line(1, "EXACT FRAME PASS");
        d.hex(2, "ERROR", 0);
        d.hex(3, "RX CAPTURED", r.rx_captured);
        d.hex(4, "SERVER SERVICE", r.server_service);
        d.hex(5, "SERVER INSTANCE", r.server_instance);
        d.hex(
            6,
            "EXPECTED INSTANCE",
            pixel_qrtr_prequery::EXPECTED_INSTANCE,
        );
        d.hex(7, "SERVER NODE", r.server_node);
        d.hex(8, "SERVER PORT", r.server_port);
        d.hex(9, "RETURNED INTENT", r.returned_intent);
        d.hex(10, "REQUESTED SIZE", r.requested_intent_size);
        d.hex(11, "RX TAIL", r.rx_tail);
        d.line(18, "NO PREQUERY CONSUME");
        d.line(19, "NO FOLLOW-UP INTENT");
        d.line(20, "NO SERVREG QMI RESPONSE");
        d.line(21, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if r.stage == 4 && r.prequery_handled && r.rx_captured == 0 && r.error.is_some() {
        d.line(0, "QRTR SERVREG PREQUERY");
        d.line(1, "PREQUERY HANDLED");
        d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
        d.hex(3, "SERVER SERVICE", r.server_service);
        d.hex(4, "SERVER INSTANCE", r.server_instance);
        d.hex(5, "SERVER NODE", r.server_node);
        d.hex(6, "SERVER PORT", r.server_port);
        d.hex(7, "RETURNED INTENT", r.returned_intent);
        d.hex(8, "REQUESTED SIZE", r.requested_intent_size);
        d.hex(9, "LOCAL INTENT", r.query_intent_id);
        d.hex(10, "BYTE WRITES", r.byte_writes);
        d.hex(11, "NOTIFICATIONS", r.notifications);
        d.hex(12, "TX TAIL", r.tx_words[0]);
        d.hex(13, "TX HEAD", r.tx_words[1]);
        d.hex(14, "RX TAIL", r.rx_tail);
        d.hex(15, "ELAPSED MS", r.elapsed_ms);
        d.line(18, "WAITING FOR SERVREG QMI");
        d.line(19, "NO QRTR RESPONSE SENT");
        d.line(20, "NO WLAN OR HOST DMA");
        d.line(21, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if r.stage == 4 && r.rx_captured > 0 && r.error.is_some() && r.rx_prequery_error_code == 9 {
        d.line(0, "QRTR INSTANCE CHECK");
        d.line(1, "INSTANCE REJECTED");
        d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
        d.hex(3, "PARSE DETAIL", r.rx_prequery_error_code);
        d.hex(4, "SERVER SERVICE", r.server_service);
        d.hex(5, "SERVER INSTANCE", r.server_instance);
        d.hex(
            6,
            "EXPECTED INSTANCE",
            pixel_qrtr_prequery::EXPECTED_INSTANCE,
        );
        d.hex(7, "SERVER NODE", r.server_node);
        d.hex(8, "SERVER PORT", r.server_port);
        d.hex(
            9,
            "INSTANCE MATCH",
            (r.server_instance == pixel_qrtr_prequery::EXPECTED_INSTANCE) as u32,
        );
        d.hex(10, "RX CAPTURED", r.rx_captured);
        d.line(22, "RX TAIL UNCHANGED");
        d.line(23, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if r.stage == 4 && r.rx_captured > 0 && r.error.is_some() {
        let w = &r.rx_words;
        let observed_tms_servreg_shape = r.rx_captured == 80
            && r.rx_available == 80
            && w[0] == 0x0002_0009
            && w[1] == 4
            && w[2] == 60
            && w[3] == 0
            && w[4] == 1
            && w[5] == 1
            && w[6] == 0
            && w[7] == 9
            && w[8] == 1
            && w[9] == 25
            && w[10] == 1
            && w[11] == 0x4000
            && w[12] == 0x2100_0400
            && w[13] == 0x0100_1200
            && w[14] == 0x6d74_0400
            && w[15] == 0x6440_7273
            && w[16] == 0x6e65_5f72
            && w[17] == 0x656c_6261
            && w[18] == 0x0000_0064
            && w[19] == 0;
        let observed_tms_pdr_enabled_shape = r.rx_captured == 80
            && r.rx_available == 80
            && w[0] == 0x0002_0009
            && w[1] == 4
            && w[2] == 60
            && w[3] == 0
            && w[4] == 1
            && w[5] == 1
            && w[6] == 0
            && w[7] == 9
            && w[8] == 0
            && w[9] == 25
            && w[10] == 1
            && w[11] == 0x4000
            && w[12] == 0x2100_0100
            && w[13] == 0x0100_1200
            && w[14] == 0x6d74_000f
            && w[15] == 0x6470_2f73
            && w[16] == 0x6e65_5f72
            && w[17] == 0x656c_6261
            && w[18] == 0x0000_0064
            && w[19] == 0;
        if observed_tms_servreg_shape || observed_tms_pdr_enabled_shape {
            let qmi_type = w[12] & 0xff;
            let qmi_transaction = (w[12] >> 8) & 0xffff;
            let qmi_message_id = ((w[12] >> 24) & 0xff) | ((w[13] & 0xff) << 8);
            let qmi_bytes = (w[13] >> 8) & 0xffff;
            let tlv_type = (w[13] >> 24) & 0xff;
            let tlv_bytes = w[14] & 0xffff;
            let mut data_prefix = [b' '; 20];
            data_prefix[..5].copy_from_slice(b"DATA ");
            for (index, slot) in data_prefix[5..].iter_mut().enumerate() {
                let byte_offset = 58 + index;
                let word = w[byte_offset / 4];
                let value = (word >> ((byte_offset % 4) * 8)) as u8;
                *slot = if value.is_ascii_graphic() {
                    value
                } else {
                    b'.'
                };
            }

            d.clear();
            d.line(0, "QRTR SERVREG RX");
            d.line(
                1,
                if observed_tms_pdr_enabled_shape {
                    "PDR QUERY DECODE"
                } else {
                    "QUERY CANDIDATE"
                },
            );
            d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
            d.hex(3, "RX AVAILABLE", r.rx_available);
            d.hex(4, "RX CAPTURED", r.rx_captured);
            d.hex(5, "GLINK CMD", w[0] & 0xffff);
            d.hex(6, "GLINK PARAM1", w[0] >> 16);
            d.hex(7, "GLINK PARAM2", w[1]);
            d.hex(8, "CHUNK SIZE", w[2]);
            d.hex(9, "LEFT SIZE", w[3]);
            d.hex(10, "QRTR VERSION", w[4]);
            d.hex(11, "QRTR TYPE", w[5]);
            d.hex(12, "SRC NODE", w[6]);
            d.hex(13, "SRC PORT", w[7]);
            d.hex(14, "QRTR CONFIRM", w[8]);
            d.hex(15, "QRTR BYTES", w[9]);
            d.hex(16, "DST NODE", w[10]);
            d.hex(17, "DST PORT", w[11]);
            d.hex(18, "QMI TYPE", qmi_type);
            d.hex(19, "QMI TXN", qmi_transaction);
            d.hex(20, "QMI MSG ID", qmi_message_id);
            d.hex(21, "QMI BYTES", qmi_bytes);
            d.hex(22, "TLV TYPE", tlv_type);
            d.hex(23, "TLV LEN", tlv_bytes);
            if let Ok(text) = core::str::from_utf8(&data_prefix) {
                d.line(24, text);
            }
            d.line(25, "RX TAIL UNCHANGED");
            d.line(26, "NO QMI RESPONSE");
            d.line(27, "NO WLAN OR HOST DMA");
            d.line(28, "RESTART WHEN DONE");
            d.flush();
            return;
        }
        d.line(0, "QRTR RX CHECK");
        d.line(
            1,
            if r.rx_prequery_shape {
                "NEW SERVER + GLINK CTRL"
            } else {
                "UNEXPECTED RX SHAPE"
            },
        );
        d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
        d.hex(3, "RX AVAILABLE", r.rx_available);
        d.hex(4, "RX CAPTURED", r.rx_captured);
        d.hex(5, "RX HEAD", r.rx_head);
        d.hex(6, "PREFIX BYTES", r.rx_prefix_bytes);
        d.hex(7, "OPAQUE BYTES", r.rx_suffix_bytes);
        d.hex(8, "PARSE DETAIL", r.rx_prequery_error_code);
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for pair in 0..16 {
            let first = pair * 2;
            if (first as u32) * 4 >= r.rx_captured {
                break;
            }
            let second = first + 1;
            let mut line = [b' '; 23];
            line[0] = b'0' + (first / 10) as u8;
            line[1] = b'0' + (first % 10) as u8;
            line[2] = b' ';
            for digit in 0..8 {
                line[3 + digit] = HEX[((r.rx_words[first] >> (28 - digit * 4)) & 0xf) as usize];
            }
            line[11] = b' ';
            line[12] = b'0' + (second / 10) as u8;
            line[13] = b'0' + (second % 10) as u8;
            line[14] = b' ';
            for digit in 0..8 {
                line[15 + digit] = HEX[((r.rx_words[second] >> (28 - digit * 4)) & 0xf) as usize];
            }
            if let Ok(text) = core::str::from_utf8(&line) {
                d.line(9 + pair, text);
            }
        }
        d.line(22, "RX TAIL UNCHANGED");
        d.line(23, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") && r.response_consumed {
        d.clear();
        d.line(0, "QRTR PDR FOLLOW-UP");
        d.line(
            1,
            if r.error.is_some() {
                "OBSERVATION STOPPED"
            } else if r.passed() {
                "EMPTY MAP PASS"
            } else {
                "FOLLOW-UP CHECK"
            },
        );
        d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
        d.hex(3, "RESPONSE CONSUMED", r.response_consumed as u32);
        d.hex(4, "POST RX AVAILABLE", r.post_rx_available);
        d.hex(5, "POST RX CAPTURED", r.post_rx_captured);
        d.hex(6, "POST RX HEAD", r.post_rx_head);
        d.hex(7, "TX HEAD", r.tx_words[1]);
        d.hex(8, "RX TAIL", r.rx_tail);
        d.line(9, "POST-REPLY RX WORDS");
        for (index, word) in r.post_rx_words.iter().enumerate() {
            if index * 4 >= r.post_rx_captured as usize {
                break;
            }
            let label = [b'W', b'0' + (index / 10) as u8, b'0' + (index % 10) as u8];
            if let Ok(label) = core::str::from_utf8(&label) {
                d.hex(10 + index, label, *word);
            }
        }
        d.line(26, "RX TAIL UNCHANGED");
        d.line(
            27,
            if r.post_rx_available == 0 {
                "NO FOLLOW-UP RX"
            } else if r.post_rx_available > r.post_rx_captured {
                "SNAPSHOT CAPPED 64B"
            } else {
                "RX SNAPSHOT STABLE"
            },
        );
        d.line(28, "NO SECOND REPLY SENT");
        d.line(29, "RESTART WHEN DONE");
        d.flush();
        return;
    }
    d.line(
        0,
        if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
            "QRTR PDR REPLY"
        } else {
            "QRTR SERVREG CHECK"
        },
    );
    d.line(
        1,
        if r.passed() {
            if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
                "EMPTY MAP REPLY PASS"
            } else {
                "SERVREG MAP PASS"
            }
        } else if r.error.is_some() {
            if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
                "PDR REPLY STOPPED"
            } else {
                "SERVREG MAP STOPPED"
            }
        } else {
            if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
                "PDR REPLY CHECK"
            } else {
                "SERVREG MAP CHECK"
            }
        },
    );
    d.hex(2, "ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "STAGE", r.stage);
    d.hex(4, "REQUEST SEEN", r.request_seen as u32);
    d.hex(5, "SERVICE ID", pixel_servreg_wire::SERVICE_ID);
    d.hex(6, "LOCAL PORT", pixel_servreg_wire::LOCAL_PORT);
    d.hex(7, "REMOTE PORT", r.request_port);
    d.hex(8, "QMI TRANSACTION", r.transaction);
    d.hex(9, "ANNOUNCED", r.announced as u32);
    d.hex(10, "RESPONSE STAGED", r.response_staged as u32);
    d.hex(11, "RESPONSE CONSUMED", r.response_consumed as u32);
    d.hex(12, "TX TAIL", r.tx_words[0]);
    d.hex(13, "TX HEAD", r.tx_words[1]);
    d.hex(14, "RX TAIL", r.rx_tail);
    d.hex(15, "BYTE WRITES", r.byte_writes);
    d.hex(16, "NOTIFICATIONS", r.notifications);
    d.hex(17, "ELAPSED MS", r.elapsed_ms);
    if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
        d.line(19, "TMS/PDR_ENABLED EMPTY");
        d.hex(20, "TOTAL DOMAINS", 0);
        d.hex(21, "DB REVISION", 1);
    } else {
        d.line(19, "DOMAIN MSM/MODEM/ROOT_PD");
        d.hex(20, "QMI INSTANCE", pixel_servreg_wire::DOMAIN_INSTANCE);
    }
    d.line(25, "NO FOLLOW-ON QMI");
    d.line(26, "NO WLAN OR HOST DMA");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}

#[cfg(feature = "sunfish-modem-crash-probe")]
fn paint_modem_crash(
    d: &Display,
    r: &pixel_qrtr_discovery::Report,
    before: &Result<
        personal_os_rust_server::qcom_modem_crash::Snapshot,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    after: &Result<
        personal_os_rust_server::qcom_modem_crash::Snapshot,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    status: &Result<
        Option<personal_os_rust_server::qcom_modem_status::Status>,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    steps: (u32, u32),
) {
    use personal_os_rust_server::qcom_modem_crash as crash;
    d.clear();
    d.line(0, "MODEM FAILURE REASON");
    d.line(
        1,
        if after.is_ok() {
            "REASON SNAPSHOT PASS"
        } else {
            "REASON SNAPSHOT STOPPED"
        },
    );
    d.hex(2, "DISC ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "LAST POLL MS", r.elapsed_ms);
    d.hex(
        4,
        "SLAVE STATUS",
        status
            .as_ref()
            .ok()
            .and_then(|s| s.and_then(|s| s.slave_kernel))
            .unwrap_or(u32::MAX),
    );
    d.hex(
        5,
        "STATUS ERROR",
        status.as_ref().err().map_or(0, |e| e.code()),
    );
    d.hex(
        6,
        "BASE ERROR",
        before.as_ref().err().map_or(0, |e| e.code()),
    );
    d.hex(
        7,
        "REASON ERROR",
        after.as_ref().err().map_or(0, |e| e.code()),
    );
    d.hex(
        8,
        "PREFIX CHANGED",
        before
            .as_ref()
            .ok()
            .zip(after.as_ref().ok())
            .map_or(u32::MAX, |(b, a)| u32::from(crash::changed(b, a))),
    );
    d.hex(9, "CHECK STAGES", (steps.0 << 16) | steps.1);
    if let Ok(s) = after {
        d.hex(
            10,
            "ITEM OFFSET",
            s.directory.item.map_or(u32::MAX, |i| i.offset as u32),
        );
        d.hex(
            11,
            "ITEM BYTES",
            s.directory.item.map_or(0, |i| i.size as u32),
        );
        d.hex(12, "TEXT BYTES", s.text_len as u32);
        d.hex(
            13,
            "NUL AND CAPPED",
            u32::from(s.terminated) | (u32::from(s.truncated) << 1),
        );
        for row in 0..11 {
            let text = crash::text_row(s, row);
            d.line(
                14 + row,
                core::str::from_utf8(&text).unwrap_or("TEXT ENCODING ERROR"),
            );
        }
    }
    d.hex(25, "OLD GUARDS", r.old_guards as u32);
    d.hex(26, "POOL GUARDS", r.pool_guards as u32);
    d.hex(27, "WLFW SEEN", r.found as u32);
    d.line(28, "REASON READ ONLY");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}

#[cfg(feature = "sunfish-modem-fatal-trace-probe")]
fn paint_modem_fatal_trace(
    d: &Display,
    r: &pixel_qrtr_discovery::Report,
    before: &Result<
        personal_os_rust_server::qcom_modem_crash::Snapshot,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    after: &Result<
        personal_os_rust_server::qcom_modem_crash::Snapshot,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    status: &Result<
        Option<personal_os_rust_server::qcom_modem_status::Status>,
        personal_os_rust_server::qcom_modem_status::Error,
    >,
    steps: (u32, u32),
) {
    use personal_os_rust_server::qcom_modem_crash as crash;
    let terminal = if r.first_fatal_status != u32::MAX {
        r.first_fatal_status
    } else {
        status
            .as_ref()
            .ok()
            .and_then(|s| s.as_ref())
            .and_then(|s| s.slave_kernel)
            .unwrap_or(u32::MAX)
    };
    d.clear();
    d.line(0, "MODEM FATAL TRACE");
    d.line(
        1,
        if r.first_fatal_status != u32::MAX {
            "FATAL TRACE CAPTURED"
        } else {
            "FATAL TRACE INCOMPLETE"
        },
    );
    d.hex(2, "DISC ERROR", r.error.map_or(0, |e| e.code()));
    d.hex(3, "FATAL AT MS", r.first_fatal_ms);
    d.hex(4, "FIRST STATUS", r.first_status);
    d.hex(5, "FATAL STATUS", terminal);
    d.hex(6, "STATUS SAMPLES", r.status_samples);
    d.hex(7, "STATUS CHANGES", r.status_changes);
    d.hex(8, "ERR FATAL BIT", u32::from(terminal & 1 != 0));
    d.hex(9, "ERR READY BIT", u32::from(terminal & 2 != 0));
    d.hex(10, "PROXY UNVOTE", u32::from(terminal & 4 != 0));
    d.hex(11, "STOP ACK BIT", u32::from(terminal & 8 != 0));
    d.hex(12, "SHUTDOWN BIT", u32::from(terminal & 128 != 0));
    d.hex(
        13,
        "REASON ERROR",
        after.as_ref().err().map_or(0, |e| e.code()),
    );
    if let Ok(s) = after {
        d.hex(
            14,
            "ITEM OFFSET",
            s.directory.item.map_or(u32::MAX, |i| i.offset as u32),
        );
        d.hex(
            15,
            "ITEM BYTES",
            s.directory.item.map_or(0, |i| i.size as u32),
        );
        for row in 0..4 {
            let text = crash::text_row(s, row);
            d.line(
                16 + row,
                core::str::from_utf8(&text).unwrap_or("TEXT ENCODING ERROR"),
            );
        }
    }
    d.hex(
        20,
        "PREFIX CHANGED",
        before
            .as_ref()
            .ok()
            .zip(after.as_ref().ok())
            .map_or(u32::MAX, |(b, a)| u32::from(crash::changed(b, a))),
    );
    d.hex(21, "CHECK STAGES", (steps.0 << 16) | steps.1);
    d.hex(
        22,
        "GUARDS",
        u32::from(r.old_guards) | (u32::from(r.pool_guards) << 1),
    );
    d.hex(23, "ELAPSED MS", r.elapsed_ms);
    d.hex(24, "DATA POLLS", r.data_samples);
    d.hex(25, "WLFW SEEN", r.found as u32);
    d.line(26, "NO GIC ACCESS");
    d.line(27, "STATUS/REASON READ ONLY");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}
