//! Shared observed metadata -> AOP ON -> strict resource hold -> verify path.
use crate::pixel_aop_modem_on::{self, Probe};
use crate::pixel_pas_metadata::Buffer;
use crate::pixel_probe_display::Display;
use crate::pixel_rpmh_bus::Controller;
use personal_os_rust_server::qcom_pas_metadata::{self as metadata, Prepared};
use personal_os_rust_server::qcom_rpmh_proxy_hold::{FloorReport, hold_pas_bus_floor};
const WORK_LIMIT: u32 = 65_536;
pub fn sample(d: &Display, row: usize, label: &str, value: Option<u32>) {
    if let Some(value) = value {
        d.hex(row, label, value);
    } else {
        d.line(row, "NOT COMPLETED");
    }
}

pub fn progress(d: &Display, label: &str) {
    d.clear();
    d.line(0, "MODEM PREPARE");
    d.line(1, label);
    d.line(26, "NO SECURE CALL");
    d.line(27, "NO SEGMENT FILES");
    d.line(28, "NO RESET RELEASE");
    d.line(29, "RESTART WHEN DONE");
    d.flush();
}

/// The sequence stops at its first fault. An uncertain/applied ON or resource
/// request is never retried or compensated; physical restart recovers the test.
pub fn run(
    d: &Display,
    buffer: &mut Buffer,
    physical: u64,
    mdt: &[u8],
    aop: &mut Probe<'_>,
    controller: &mut Controller,
    prepared: &mut Option<Prepared>,
    resources: &mut Option<FloorReport>,
    verified: &mut bool,
) -> Result<(), u32> {
    // All input/address checks and the guarded CPU copy precede platform writes.
    progress(d, "CHECK AND STAGE MDT");
    *prepared = Some(metadata::prepare(buffer, physical, mdt).map_err(|e| 0x10000 | e.code())?);
    pixel_aop_modem_on::run(aop).map_err(|e| 0x20000 | e)?;
    *resources = Some(hold_pas_bus_floor(
        controller,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |phase| progress(d, phase.label()),
    ));
    let r = resources.as_ref().unwrap();
    if !r.passed() {
        return Err(0x30000 | r.controller.error.map_or(0xff, |e| e.code()));
    }
    progress(d, "VERIFY MDT AND GUARDS");
    // Read-only verification: never repairs or re-stages after platform writes.
    let checked = metadata::verify(buffer, physical, mdt).map_err(|e| 0x40000 | e.code())?;
    if Some(checked) != *prepared {
        return Err(0x400ff);
    }
    *verified = true;
    Ok(())
}
