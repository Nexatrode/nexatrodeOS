//! One native IPA PAS15 boot, stopping before host GSI commands or modem/WLAN.
use super::*;
use personal_os_rust_server::{
    sunfish_ipa_linux_buffers as buffers, sunfish_ipa_linux_loader as loader,
    sunfish_ipa_linux_loader_native::Window,
    sunfish_ipa_linux_pas::LinuxTransport,
    sunfish_ipa_linux_resources as resources, sunfish_ipa_pas as pas,
    sunfish_scm_linux_power::{self as scm, HeldResources},
};

const FIRMWARE: &[u8] = include_bytes!(env!("SUNFISH_IPA_FIRMWARE_PATH"));
#[used]
#[unsafe(no_mangle)]
static mut ipa_pas_metadata_paddr: u64 = 0;
#[used]
#[unsafe(no_mangle)]
static mut ipa_fw_target_paddr: u64 = 0;
pub(super) type NativeLoader = loader::Loader<'static, Window<'static>>;

fn error_code(error: loader::Error) -> u32 {
    use loader::Error::*;
    match error {
        Identity => 1,
        Image => 2,
        Geometry => 3,
        Consumed => 4,
        DirtyMetadata => 5,
        Metadata => 6,
        Io => 7,
        Guard => 8,
        Pas(e) => 0x100 + e as u32,
        Stage => 9,
        Readback => 10,
    }
}
fn frame(d: &Display, status: &str) {
    d.clear();
    d.replace_line(0, "NATIVE IPA FIRMWARE");
    d.replace_line(1, status);
    d.replace_line(23, "IPA PAS 15 ONLY");
    d.replace_line(24, "POWER HOLDS UNTIL REBOOT");
    d.replace_line(25, "NO HOST GSI OR DMA COMMAND");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "NO FLASH OR RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
fn guard(
    domain: &pixel_smmu_install::Probe,
    pool: &buffers::Pool<
        personal_os_rust_server::sunfish_ipa_linux_buffers_native::NativeRam<'static>,
    >,
    registers: IrqRegisters,
    expected: personal_os_rust_server::sunfish_ipa_linux_config::Snapshot,
) -> Result<(), loader::Error> {
    if !domain.retained_quiet() || pool.buffer(buffers::Kind::Zero).is_none() {
        return Err(loader::Error::Guard);
    }
    read_retained_irq_snapshot(registers.ipa, registers.gsi, expected, 12)
        .map_err(|_| loader::Error::Guard)?;
    if word(registers.ipa, 0x3008)
        & !personal_os_rust_server::sunfish_ipa_linux_irq::ALLOWED_DISABLED_STATUS
        != 0
    {
        return Err(loader::Error::Guard);
    }
    for _ in 0..2 {
        for spec in resources::PROFILE {
            if word(registers.ipa, spec.offset) != spec.value {
                return Err(loader::Error::Guard);
            }
        }
    }
    Ok(())
}
fn stop(d: &Display, state: &mut Startup, label: &str, code: u32) {
    dispatch_uc(state, true); // one mask, no clear/ACK/retry, retain every owner
    frame(d, label);
    d.hex(2, "NATIVE ERROR", code);
    show_report(d, state);
    d.flush();
}
fn show_report(d: &Display, state: &Startup) {
    let hold = state._scm_hold.report();
    d.hex(3, "SCM SENT", hold.submitted as u32);
    d.hex(4, "SCM DONE", hold.completed as u32);
    if let Some(e) = hold.error {
        d.hex(5, "SCM ERROR", e.code() as u32);
    }
    if let Some(f) = state._firmware.as_ref() {
        let r = f.report();
        let p = f.pas_report();
        d.hex(6, "LOAD PHASE", r.phase as u32);
        d.hex(7, "PAS SENT", p.issued_mask);
        d.hex(8, "PAS RETURNED", p.returned_mask);
        d.hex(9, "METADATA BYTES", r.metadata_length as u32);
        d.hex(10, "TARGET SENT", r.target_writes as u32);
        d.hex(11, "TARGET VERIFIED", r.target_verified as u32);
        if let Some(e) = r.error {
            d.hex(12, "LOADER ERROR", error_code(e));
        }
        if let Some(e) = p.error {
            d.hex(13, "PAS ERROR", e as u32 + 1);
        }
        if let Some(v) = p.replies[5] {
            d.hex(14, "AUTH MONITOR", v[0] as u32);
            d.hex(15, "AUTH SERVICE", v[1] as u32);
        }
        if let Some(v) = p.kernel_error {
            d.hex(16, "SMC KERNEL ERR", v as u32);
        }
    }
    if let Some(reg) = state._irq_registers {
        d.hex(17, "GSI STATUS", word(reg.gsi, 0x1f000));
    }
    if let Some(domain) = state._domain.as_ref() {
        if let Some((g, c)) = domain.fault_status() {
            d.hex(18, "GLOBAL FSR", g);
            d.hex(19, "CONTEXT FSR", c);
        }
    }
    if let Some(uc) = state._uc.as_ref() {
        d.hex(20, "UC INIT SEEN", uc.report().init_observed);
    }
}
pub(super) fn show_runtime_result(d: &Display, state: &Startup) {
    let Some(runtime) = state._uc.as_ref() else {
        return;
    };
    let r = runtime.report();
    frame(
        d,
        if r.error.is_some() {
            "IPA UC STOP"
        } else if r.init_observed > 0 {
            "IPA UC INIT OBSERVED"
        } else {
            "IPA UC IRQ OBSERVED"
        },
    );
    show_report(d, state);
    d.hex(2, "UC NOTICES", r.notifications);
    d.hex(16, "UC ACKS", r.acknowledge_completed);
    if let Some(v) = r.last_event {
        d.hex(13, "UC LAST EVENT", v as u32);
    }
    if let Some(v) = r.last_response {
        d.hex(21, "UC LAST RSP", v as u32);
    }
    d.hex(22, "UC ERROR", r.error.map_or(0, |e| e.code()));
    if let Some(e) = r.containment_error {
        d.hex(12, "CONTAIN ERROR", e.code());
    }
    d.flush();
}
pub(super) fn start(
    d: &Display,
    state: &mut Startup,
    controller: pixel_rpmh_bus::Controller,
    dict: *const u32,
    db: *const u32,
) {
    // Retain actual controller and memory owners before any new write/SMC.
    state._firmware_rsc = Some(controller);
    frame(d, "VERIFY PRIVATE IPA IMAGE");
    let image = match loader::Image::private_reference(FIRMWARE) {
        Ok(v) => v,
        Err(e) => {
            stop(d, state, "IPA IMAGE STOP", error_code(e));
            return;
        }
    };
    let memory = memory_region_symbol!(ipa_pas_metadata:*mut u8).as_ptr();
    let physical = unsafe { core::ptr::read_volatile(&raw const ipa_pas_metadata_paddr) };
    let target = memory_region_symbol!(ipa_fw_target:*mut u8).as_ptr();
    let target_physical = unsafe { core::ptr::read_volatile(&raw const ipa_fw_target_paddr) };
    let Some(second) = physical.checked_add(4096) else {
        stop(d, state, "IPA MAPPING STOP", 1);
        return;
    };
    if memory as usize != 0x75601000 || target as usize != 0x75800000 {
        stop(d, state, "IPA MAPPING STOP", 2);
        return;
    }
    // SAFETY: firmware-profile actual CapDL audit proves these two allocated
    // contiguous private pages and the exact reserved target, uncached RW/NX,
    // without any frame/CPU/reserved alias. No TZMem bridge is registered.
    // This PD retains the views until reboot and never retries a PAS attempt.
    let window = match unsafe {
        Window::new(memory, [physical, second], target, target_physical, 0x10000)
    } {
        Ok(v) => v,
        Err(e) => {
            stop(d, state, "IPA WINDOW STOP", error_code(e));
            return;
        }
    };
    state._firmware = match NativeLoader::new(image, window) {
        Ok(v) => Some(v),
        Err(e) => {
            stop(d, state, "IPA OWNER STOP", error_code(e));
            return;
        }
    };
    let (Some(domain), Some(pool), Some(expected), Some(registers)) = (
        state._domain.as_ref(),
        state._buffers.as_ref(),
        state._config_report.as_ref().and_then(|r| r.after),
        state._irq_registers,
    ) else {
        stop(d, state, "IPA PRECONDITION STOP", 3);
        return;
    };
    if guard(domain, pool, registers, expected).is_err() {
        stop(d, state, "IPA PRECONDITION STOP", 4);
        return;
    }
    let metadata = match metadata_named(dict, db, scm::NAMES) {
        Ok(v) => v,
        Err(e) => {
            stop(d, state, "SCM CMD DB STOP", e);
            return;
        }
    };
    frame(d, "HOLD SCM CRYPTO PATH");
    let result = state._scm_hold.enable(
        state._firmware_rsc.as_mut().unwrap(),
        metadata,
        65536,
        core::hint::spin_loop,
        |resource| {
            d.hex(2, "SCM RESOURCE", resource.index() as u32);
            d.flush();
        },
    );
    if let Err(e) = result {
        stop(d, state, "SCM HOLD STOP", e.code() as u32);
        return;
    }
    frame(d, "PREPARE IPA METADATA");
    let result = state._firmware.as_mut().unwrap().prepare();
    if let Err(e) = result {
        stop(d, state, "IPA METADATA STOP", error_code(e));
        return;
    }
    // Disjoint retained borrows remain live through the whole PAS sequence.
    let mut secure = pixel_pas_smc::Smc;
    let hold_report = state._scm_hold.report();
    let mut resources = match HeldResources::new(
        &state._scm_hold,
        state._firmware_rsc.as_mut().unwrap(),
        65536,
    ) {
        Ok(v) => v,
        Err(e) => {
            stop(d, state, "SCM OWNER STOP", e.code() as u32);
            return;
        }
    };
    let result = state._firmware.as_mut().unwrap().start(
        &mut LinuxTransport::new(&mut secure, &mut resources),
        |_| guard(domain, pool, registers, expected),
        |p| {
            d.replace_line(
                1,
                match p.phase {
                    pas::Phase::InitPending => "CALL IPA PAS INIT",
                    pas::Phase::MemSetupPending => "CALL IPA MEM SETUP",
                    pas::Phase::AuthPending => "CALL IPA AUTH RESET",
                    _ => "QUERY IPA PAS",
                },
            );
            d.hex(7, "PAS SENT", p.issued_mask);
            d.hex(8, "PAS RETURNED", p.returned_mask);
            d.hex(3, "SCM SENT", hold_report.submitted as u32);
            d.hex(4, "SCM DONE", hold_report.completed as u32);
            if let Some(stage) = p.stage_report {
                d.hex(9, "SEGMENTS", stage.segments_staged as u32);
                d.hex(10, "FILE BYTES", stage.file_bytes_staged as u32);
                d.hex(11, "BSS BYTES", stage.bss_bytes_zeroed as u32);
            }
            d.flush();
        },
    );
    // Call leases do not unvote; the retained hold remains until reboot.
    drop(resources);
    if let Err(e) = result {
        stop(d, state, "IPA PAS STOP", error_code(e));
        return;
    }
    if read_retained_post_auth_snapshot(registers.ipa, registers.gsi, expected).is_err() {
        stop(d, state, "IPA POST AUTH STOP", 5);
        return;
    }
    frame(d, "IPA AUTH RETURNED");
    show_report(d, state);
    d.replace_line(21, "WAITING FOR UC INIT");
    d.replace_line(22, "NO READY TOKEN MINTED");
    d.flush();
}
