#![no_std]
#![no_main]
mod pixel_aop_mailbox;
mod pixel_aop_modem_on;
mod pixel_pas_metadata;
mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::qcom_pas_metadata::{self as metadata, Prepared};
use personal_os_rust_server::qcom_rpmh_bus::{self, Report};
use personal_os_rust_server::qcom_rpmh_read::BcmResource;
use pixel_aop_mailbox::Mailbox;
use pixel_aop_modem_on::Probe;
use pixel_pas_metadata::Buffer;
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const WORK_LIMIT: u32 = 65_536;
static MDT: &[u8] = include_bytes!(env!("SUNFISH_MODEM_MDT_PATH"));
const EXPECTED: [u32; 6] = [
    0x2991_7f75,
    0x2000_0008,
    0,
    0x213c_44f1,
    0x213c_44f1,
    0x2400_5adf,
];

#[protection_domain]
fn init() -> ModemBcmAfterAopProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    show_start(&display, "STAGE MDT FIRST");
    let physical = *sel4_microkit::var!(pas_metadata_paddr:usize=0) as u64;
    let mut buffer = unsafe { Buffer::new(memory_region_symbol!(pas_metadata:*mut u8).as_ptr()) };
    let prepared = match metadata::prepare(&mut buffer, physical, MDT) {
        Ok(value) => value,
        Err(error) => {
            display.clear();
            display.line(0, "MODEM BCM AFTER AOP");
            display.line(1, "MDT PREP FAIL");
            display.hex(2, "ERROR", error.code());
            no_start_lines(&display);
            display.flush();
            return ModemBcmAfterAopProbe;
        }
    };

    let mailbox = unsafe {
        Mailbox::new(
            memory_region_symbol!(aop_msgram:*mut u32).as_ptr(),
            memory_region_symbol!(aop_doorbell:*mut u32).as_ptr(),
        )
    };
    let mut aop = Probe::new(&display, &mailbox);
    show_start(&display, "AOP ON START");
    if let Err(error) = pixel_aop_modem_on::run(&mut aop) {
        display.clear();
        display.line(0, "MODEM BCM AFTER AOP");
        display.line(1, "AOP ON STOPPED");
        display.hex(2, "ERROR", error);
        display.hex(3, "AOP PHASE MASK", aop.mask);
        display.hex(4, "AOP NOTIFIES", aop.notifies);
        no_start_lines(&display);
        display.flush();
        return ModemBcmAfterAopProbe;
    }

    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };
    show_start(&display, "READ BCM TWICE");
    let result = qcom_rpmh_bus::read_bcm_series(
        &mut controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |pass, resource| {
            display.clear();
            display.line(0, "MODEM BCM AFTER AOP");
            display.hex(1, "PASS", u32::from(pass) + 1);
            display.hex(2, "RESOURCE", resource.address());
            display.hex(3, "AOP PHASE MASK", aop.mask);
            display.line(26, "NO BCM RESOURCE WRITE");
            display.line(27, "NO SECURE CALL");
            display.line(28, "NO MODEM OR WLAN START");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );
    let metadata_ok = metadata::verify(&mut buffer, physical, MDT) == Ok(prepared);
    paint_result(&display, &aop, prepared, metadata_ok, &result);
    debug_println!(
        "MODEM-BCM-AOP|aop={:x}/{:x}|metadata={}|rsc={:?}",
        aop.mask,
        aop.notifies,
        metadata_ok,
        result
    );
    ModemBcmAfterAopProbe
}

fn show_start(display: &Display, status: &str) {
    display.clear();
    display.line(0, "MODEM BCM AFTER AOP");
    display.line(1, status);
    display.line(26, "NO BCM RESOURCE WRITE");
    display.line(27, "NO SECURE CALL");
    display.line(28, "NO MODEM OR WLAN START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
}

fn no_start_lines(display: &Display) {
    display.line(26, "NO BCM RESOURCE WRITE");
    display.line(27, "NO SECURE CALL");
    display.line(28, "NO MODEM OR WLAN START");
    display.line(29, "RESTART WHEN DONE");
}

fn paint_result(
    display: &Display,
    aop: &Probe<'_>,
    prepared: Prepared,
    metadata_ok: bool,
    r: &Report,
) {
    display.clear();
    display.line(0, "MODEM BCM AFTER AOP");
    display.line(
        1,
        if !r.passed() {
            "BCM READ FAIL"
        } else if pair_diff(r) != 0 {
            "TWO PASSES DIFFER"
        } else {
            "TWO PASSES MATCH"
        },
    );
    display.hex(2, "READ ERROR", r.error.map_or(0, |e| e.code()));
    display.hex(3, "AOP PHASE MASK", aop.mask);
    display.hex(4, "AOP NOTIFIES", aop.notifies);
    display.hex(5, "READS DONE", u32::from(r.completed));
    display.hex(6, "HANDOFF CHECKS", u32::from(r.handoff_checks));
    for (i, (resource, labels)) in [
        (BcmResource::Mc0, ["MC0 PASS0", "MC0 PASS1"]),
        (BcmResource::Acv, ["ACV PASS0", "ACV PASS1"]),
        (BcmResource::Ce0, ["CE0 PASS0", "CE0 PASS1"]),
        (BcmResource::Sn11, ["SN11 PASS0", "SN11 PASS1"]),
        (BcmResource::Sn0, ["SN0 PASS0", "SN0 PASS1"]),
        (BcmResource::Sh0, ["SH0 PASS0", "SH0 PASS1"]),
    ]
    .into_iter()
    .enumerate()
    {
        for pass in 0..2 {
            let row = 7 + i * 2 + pass;
            if let Some(value) = r.responses[pass][resource.index()] {
                display.hex(row, labels[pass], value);
            } else {
                display.line(row, "NOT COMPLETED");
            }
        }
    }
    display.hex(19, "PAIR DIFF MASK", pair_diff(r));
    display.hex(20, "EXPECTED DIFF", expected_diff(r));
    display.hex(21, "CTRL ERROR", r.controller_error.map_or(0, |e| e.code()));
    display.hex(22, "HANDOFF ERROR", r.handoff_error.map_or(0, |e| e.code()));
    display.hex(
        23,
        "FAILED PASS",
        r.failed_pass.map_or(0, |p| u32::from(p) + 1),
    );
    display.hex(
        24,
        "FAILED ADDRESS",
        r.failed_resource.map_or(0, BcmResource::address),
    );
    display.hex(
        25,
        "MDT VERIFIED",
        u32::from(metadata_ok && prepared.metadata_length == MDT.len()),
    );
    display.line(26, "NO BCM RESOURCE WRITE");
    display.line(27, "NO SECURE CALL");
    display.line(28, "NO MODEM OR WLAN START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
}

fn pair_diff(r: &Report) -> u32 {
    (0..6).fold(0, |mask, i| {
        mask | (u32::from(r.responses[0][i] != r.responses[1][i]) << i)
    })
}

fn expected_diff(r: &Report) -> u32 {
    (0..6).fold(0, |mask, i| {
        let matches = (0..2).all(|pass| r.responses[pass][i] == Some(EXPECTED[i]));
        mask | (u32::from(!matches) << i)
    })
}

struct ModemBcmAfterAopProbe;
impl Handler for ModemBcmAfterAopProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
