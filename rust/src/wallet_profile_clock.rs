//! Read the exported architectural counter without a cross-PD timer call.
//! This measures elapsed guest time and includes time descheduled by the host.
pub(super) fn now_us() -> u64 {
    let ticks: u64;
    let freq: u64;
    unsafe {
        if sel4::sel4_cfg_bool!(EXPORT_PCNT_USER) {
            core::arch::asm!("isb", "mrs {}, cntpct_el0", out(reg) ticks, options(nostack, preserves_flags));
        } else if sel4::sel4_cfg_bool!(EXPORT_VCNT_USER) {
            core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) ticks, options(nostack, preserves_flags));
        } else {
            // Some SDKs expose neither counter. They cannot time worker phases.
            return 0;
        }
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack, preserves_flags));
    }
    ticks / freq * 1_000_000 + ticks % freq * 1_000_000 / freq
}
