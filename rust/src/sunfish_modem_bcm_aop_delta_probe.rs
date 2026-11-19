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

#[protection_domain]
fn init() -> ModemBcmAopDeltaProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    show_start(&display, "STAGE MDT FIRST");
    let physical = *sel4_microkit::var!(pas_metadata_paddr:usize=0) as u64;
    let mut buffer = unsafe { Buffer::new(memory_region_symbol!(pas_metadata:*mut u8).as_ptr()) };
    let prepared = match metadata::prepare(&mut buffer, physical, MDT) {
        Ok(value) => value,
        Err(error) => {
            display.clear();
            display.line(0, "BCM AOP BEFORE AFTER");
            display.line(1, "MDT PREP FAIL");
            display.hex(2, "ERROR", error.code());
            no_start_lines(&display);
            display.flush();
            return ModemBcmAopDeltaProbe;
        }
    };

    let mailbox = unsafe {
        Mailbox::new(
            memory_region_symbol!(aop_msgram:*mut u32).as_ptr(),
            memory_region_symbol!(aop_doorbell:*mut u32).as_ptr(),
        )
    };
    let mut aop = Probe::new(&display, &mailbox);
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc:*mut u32).as_ptr()) };

    show_start(&display, "READ BEFORE AOP");
    let before = read_series(&display, &mut controller, "BEFORE AOP READ");
    if !before.passed() {
        let metadata_ok = metadata::verify(&mut buffer, physical, MDT) == Ok(prepared);
        paint_result(&display, &aop, prepared, metadata_ok, &before, None, None);
        debug_println!("MODEM-BCM-AOP-DELTA|before={:?}|aop=skipped", before);
        return ModemBcmAopDeltaProbe;
    }

    show_start(&display, "AOP ON BETWEEN READS");
    if let Err(error) = pixel_aop_modem_on::run(&mut aop) {
        let metadata_ok = metadata::verify(&mut buffer, physical, MDT) == Ok(prepared);
        paint_result(
            &display,
            &aop,
            prepared,
            metadata_ok,
            &before,
            None,
            Some(error),
        );
        debug_println!(
            "MODEM-BCM-AOP-DELTA|before={:?}|aop_error={:x}",
            before,
            error
        );
        return ModemBcmAopDeltaProbe;
    }

    show_start(&display, "READ AFTER AOP");
    let after = read_series(&display, &mut controller, "AFTER AOP READ");
    let metadata_ok = metadata::verify(&mut buffer, physical, MDT) == Ok(prepared);
    paint_result(
        &display,
        &aop,
        prepared,
        metadata_ok,
        &before,
        Some(&after),
        None,
    );
    debug_println!(
        "MODEM-BCM-AOP-DELTA|aop={:x}/{:x}|metadata={}|before={:?}|after={:?}",
        aop.mask,
        aop.notifies,
        metadata_ok,
        before,
        after
    );
    ModemBcmAopDeltaProbe
}

fn read_series(display: &Display, controller: &mut Controller, progress: &'static str) -> Report {
    qcom_rpmh_bus::read_bcm_series(
        controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |pass, resource| {
            display.clear();
            display.line(0, "BCM AOP BEFORE AFTER");
            display.line(1, progress);
            display.hex(2, "PASS", u32::from(pass) + 1);
            display.hex(3, "RESOURCE", resource.address());
            display.line(26, "NO BCM RESOURCE WRITE");
            display.line(27, "NO SECURE CALL");
            display.line(28, "NO MODEM OR WLAN START");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    )
}

fn show_start(display: &Display, status: &str) {
    display.clear();
    display.line(0, "BCM AOP BEFORE AFTER");
    display.line(1, status);
    no_start_lines(display);
    display.flush();
}

fn no_start_lines(display: &Display) {
    display.line(26, "NO BCM RESOURCE WRITE");
    display.line(27, "NO SECURE CALL");
    display.line(28, "NO MODEM OR WLAN START");
    display.line(29, "RESTART WHEN DONE");
}

fn stable_value(report: &Report, index: usize) -> Option<u32> {
    match (report.responses[0][index], report.responses[1][index]) {
        (Some(first), Some(second)) if first == second => Some(first),
        _ => None,
    }
}

fn pair_diff(report: &Report) -> u32 {
    (0..6).fold(0, |mask, index| {
        let changed = match (report.responses[0][index], report.responses[1][index]) {
            (Some(first), Some(second)) => first != second,
            _ => true,
        };
        mask | (u32::from(changed) << index)
    })
}

fn transition_diff(before: &Report, after: Option<&Report>) -> u32 {
    (0..6).fold(0, |mask, index| {
        let changed = match (
            stable_value(before, index),
            after.and_then(|r| stable_value(r, index)),
        ) {
            (Some(before), Some(after)) => before != after,
            _ => true,
        };
        mask | (u32::from(changed) << index)
    })
}

fn resource_line(
    display: &Display,
    row: usize,
    label: &str,
    before: Option<u32>,
    after: Option<u32>,
) {
    let mut line = [b' '; 24];
    let label = label.as_bytes();
    if label.len() > 4 {
        return;
    }
    line[..label.len()].copy_from_slice(label);
    line[label.len()] = b' ';
    line[label.len() + 1] = b'B';
    write_value(&mut line[label.len() + 2..label.len() + 10], before);
    line[label.len() + 10] = b' ';
    line[label.len() + 11] = b'A';
    write_value(&mut line[label.len() + 12..label.len() + 20], after);
    let end = label.len() + 20;
    if let Ok(text) = core::str::from_utf8(&line[..end]) {
        display.line(row, text);
    }
}

fn write_value(out: &mut [u8], value: Option<u32>) {
    if out.len() != 8 {
        return;
    }
    if let Some(value) = value {
        for (index, byte) in out.iter_mut().enumerate() {
            let shift = 28 - 4 * index;
            *byte = b"0123456789ABCDEF"[((value >> shift) & 0xf) as usize];
        }
    } else {
        out.copy_from_slice(b"MISSING ");
    }
}

fn paint_result(
    display: &Display,
    aop: &Probe<'_>,
    prepared: Prepared,
    metadata_ok: bool,
    before: &Report,
    after: Option<&Report>,
    aop_error: Option<u32>,
) {
    display.clear();
    display.line(0, "BCM AOP BEFORE AFTER");
    display.line(
        1,
        if before.passed() {
            "BEFORE PASSED"
        } else {
            "BEFORE FAILED"
        },
    );
    display.line(
        2,
        match after {
            Some(result) if result.passed() => "AFTER PASSED",
            Some(_) => "AFTER FAILED",
            None if aop_error.is_some() => "AOP FAILED",
            None => "AFTER SKIPPED",
        },
    );
    display.hex(3, "AOP ERROR", aop_error.unwrap_or(0));
    display.hex(4, "AOP PHASE", aop.mask);
    display.hex(5, "AOP NOTIFIES", aop.notifies);
    display.hex(6, "BEFORE READS", u32::from(before.completed));
    display.hex(
        7,
        "AFTER READS",
        after.map_or(0, |r| u32::from(r.completed)),
    );
    display.hex(8, "B PAIR DIFF", pair_diff(before));
    display.hex(9, "A PAIR DIFF", after.map_or(0x3f, pair_diff));
    display.hex(10, "AOP DELTA MASK", transition_diff(before, after));
    for (index, (resource, label)) in [
        (BcmResource::Mc0, "MC0"),
        (BcmResource::Acv, "ACV"),
        (BcmResource::Ce0, "CE0"),
        (BcmResource::Sn11, "SN11"),
        (BcmResource::Sn0, "SN0"),
        (BcmResource::Sh0, "SH0"),
    ]
    .into_iter()
    .enumerate()
    {
        resource_line(
            display,
            11 + index,
            label,
            stable_value(before, resource.index()),
            after.and_then(|r| stable_value(r, resource.index())),
        );
    }
    display.hex(17, "B READ ERROR", before.error.map_or(0, |e| e.code()));
    display.hex(
        18,
        "A READ ERROR",
        after.and_then(|r| r.error).map_or(0, |e| e.code()),
    );
    display.hex(
        19,
        "B CTRL ERROR",
        before.controller_error.map_or(0, |e| e.code()),
    );
    display.hex(
        20,
        "A CTRL ERROR",
        after
            .and_then(|r| r.controller_error)
            .map_or(0, |e| e.code()),
    );
    display.hex(
        21,
        "B HANDOFF ERR",
        before.handoff_error.map_or(0, |e| e.code()),
    );
    display.hex(
        22,
        "A HANDOFF ERR",
        after.and_then(|r| r.handoff_error).map_or(0, |e| e.code()),
    );
    display.hex(
        23,
        "MDT VERIFIED",
        u32::from(metadata_ok && prepared.metadata_length == MDT.len()),
    );
    no_start_lines(display);
    display.flush();
}

struct ModemBcmAopDeltaProbe;
impl Handler for ModemBcmAopDeltaProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
