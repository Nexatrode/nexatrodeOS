//! Concrete pre-auth SMEM extension of the retained native IPA/RMTFS image.
//! No additional SMC, peer notification, modem authentication or GLINK start.
use super::*;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use personal_os_rust_server::sunfish_mpss_linux_smem::{self as memory, Phase};
// One image-lifetime attempt. Only state1 permits extension I/O; readiness of
// the actual retained owners is checked before entering every bounded step.
static STATE: AtomicU8 = AtomicU8::new(0);
static ERROR: AtomicU32 = AtomicU32::new(0);
const GENERATION: u64 = 1;
fn live(generation: u64) -> Result<(), memory::Error> {
    if STATE.load(Ordering::Acquire) != 1
        || !pixel_linux_rmtfs::preboot_generation(generation)
        || !pixel_smmu_install::preboot_faults_clear()
    {
        return Err(memory::Error::Guard);
    }
    Ok(())
}
fn healthy(state: &Startup) -> bool {
    pixel_ipa_receive::healthy(state)
        && state._domain.as_ref().is_some_and(|d| d.retained_quiet())
        && state
            ._linux_rmtfs
            .as_ref()
            .is_some_and(|f| f.phase() == pixel_linux_rmtfs::Phase::Prepared)
        && pixel_linux_rmtfs::preboot_generation(GENERATION)
}
pub(super) fn fault(state: &mut Startup, code: u32) {
    ERROR
        .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire)
        .ok();
    STATE.store(3, Ordering::Release);
    if let Some(f) = state._linux_rmtfs.as_mut() {
        if f.phase() != pixel_linux_rmtfs::Phase::Failed {
            f.fault(code);
        }
    }
    // Keep all actual owners/mappings and any partial publication retained.
    // No retry, unvote, automatic cleanup or extra IRQ ACK is performed here.
}
pub(super) fn show(d: &Display, state: &Startup) {
    d.clear();
    d.replace_line(0, "MODEM HOST SMEM");
    d.replace_line(
        1,
        match STATE.load(Ordering::Acquire) {
            0 => "NOT STARTED",
            1 => "PREPARING FRESH REGIONS",
            2 => "HOST SMEM PREPARED",
            _ => "HOST SMEM STOP",
        },
    );
    d.hex(2, "NATIVE ERROR", ERROR.load(Ordering::Relaxed));
    if let Some(r) = state._domain.as_ref().and_then(|d| d.modem_smem_report()) {
        d.hex(4, "PHASE", r.phase as u32);
        d.hex(5, "ALLOCATIONS", u32::from(r.allocations));
        d.hex(6, "STORES SENT", r.submitted);
        d.hex(7, "STORES DONE", r.completed);
        d.hex(8, "ZEROED BYTES", r.zeroed as u32);
        d.hex(9, "VERIFIED BYTES", r.verified as u32);
        d.hex(10, "LOCK ATTEMPTS", r.lock_attempts);
        d.hex(11, "LOCKS", r.locks);
        d.hex(12, "RELEASES", r.releases);
        d.hex(13, "LAST SMEM OFFSET", r.last_offset as u32);
        d.hex(14, "LAST VALUE", r.last_value);
        d.hex(
            15,
            "CORE ERROR",
            r.error.map_or(0, |e| match e {
                memory::Error::Consumed => 1,
                memory::Error::Predecessor => 2,
                memory::Error::Guard => 3,
                memory::Error::LockBusy => 4,
                memory::Error::LockLost => 5,
                memory::Error::Directory(e) => 0x100 + e.code(),
                memory::Error::Layout => 6,
                memory::Error::Unstable => 7,
                memory::Error::Store => 8,
                memory::Error::Readback => 9,
            }),
        );
    }
    for (row, label, value) in [
        (16, "SMP2P PA", 0x8611f040),
        (17, "GLINK DESC PA", 0x8611f1a8),
        (18, "GLINK TX PA", 0x8611f1d8),
    ] {
        d.hex(row, label, value);
    }
    d.hex(
        20,
        "IPA RX HEALTHY",
        u32::from(pixel_ipa_receive::healthy(state)),
    );
    d.hex(
        21,
        "PREP RECEIPT",
        u32::from(state._modem_smem_prepared.is_some()),
    );
    d.replace_line(23, "IPA OWNER RETAINED");
    d.replace_line(24, "NO SMP2P OR GLINK START");
    d.replace_line(25, "NO MPSS OR WLAN START");
    d.replace_line(27, "NO FLASH OR RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
pub(super) fn prepare(d: &Display, state: &mut Startup) {
    // The provider stage already rendered the specific preparation failure.
    // Preserve its screen, diagnostic and retained owners if it did not finish.
    if state
        ._linux_rmtfs
        .as_ref()
        .is_none_or(|f| f.phase() != pixel_linux_rmtfs::Phase::Prepared)
    {
        return;
    }
    if STATE
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        fault(state, 1);
        show(d, state);
        return;
    }
    if !healthy(state) {
        fault(state, 2);
        show(d, state);
        return;
    }
    // The real domain already retains the extended Window, IPA allocation and
    // all injected views. No copied address or independent allocator is used.
    if state
        ._domain
        .as_mut()
        .unwrap()
        .attach_modem_smem(GENERATION, live)
        .is_err()
    {
        fault(state, 3);
        show(d, state);
        return;
    }
    show(d, state);
    for _ in 0..32 {
        pixel_ipa_receive::poll_pending(state);
        if !healthy(state) {
            fault(state, 4);
            show(d, state);
            return;
        }
        let phase = match state._domain.as_mut().unwrap().modem_smem_step() {
            Ok(p) => p,
            Err(_) => {
                fault(state, 5);
                show(d, state);
                return;
            }
        };
        show(d, state);
        if phase == Phase::Prepared {
            pixel_ipa_receive::poll_pending(state);
            if !healthy(state) {
                fault(state, 6);
                show(d, state);
                return;
            }
            match state._domain.as_mut().unwrap().take_modem_smem_prepared() {
                Ok(p) if p.generation() == GENERATION => state._modem_smem_prepared = Some(p),
                _ => {
                    fault(state, 7);
                    show(d, state);
                    return;
                }
            }
            STATE.store(2, Ordering::Release);
            show(d, state);
            return;
        }
    }
    fault(state, 8);
    show(d, state);
}
