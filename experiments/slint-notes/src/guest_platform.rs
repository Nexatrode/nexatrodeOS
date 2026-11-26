use alloc::{boxed::Box, rc::Rc, collections::VecDeque};
use core::{time::Duration,cell::RefCell};
use slint::platform::{Platform, WindowAdapter, software_renderer::MinimalSoftwareWindow};

use crate::frame_clock::{elapsed, next_wake_delay};

pub fn counter_ticks() -> u64 {
    let ticks: u64;
    // The shipping phone/VM profiles export an architectural counter. Older
    // iPhone boot diagnostics export neither and must not execute a trapping MRS.
    unsafe {
        if sel4_config::sel4_cfg_bool!(EXPORT_VCNT_USER) {
            core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) ticks, options(nostack, preserves_flags));
        } else if sel4_config::sel4_cfg_bool!(EXPORT_PCNT_USER) {
            core::arch::asm!("isb", "mrs {}, cntpct_el0", out(reg) ticks, options(nostack, preserves_flags));
        } else {
            return 0;
        }
    }
    ticks
}

struct GuestPlatform {
    window: Rc<MinimalSoftwareWindow>,
    adapters: RefCell<VecDeque<Rc<MinimalSoftwareWindow>>>,
    origin: u64,
    frequency: u32,
}

impl Platform for GuestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.adapters.borrow_mut().pop_front().unwrap_or_else(||self.window.clone()))
    }

    fn duration_since_start(&self) -> Duration {
        elapsed(counter_ticks().wrapping_sub(self.origin), self.frequency)
    }
}

pub fn counter_frequency() -> u32 {
    let frequency: u64;
    if sel4_config::sel4_cfg_bool!(EXPORT_VCNT_USER)
        || sel4_config::sel4_cfg_bool!(EXPORT_PCNT_USER)
    {
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) frequency, options(nomem, nostack, preserves_flags));
        }
        assert!(frequency != 0 && frequency <= u32::MAX as u64);
    } else {
        // This branch is confined to historical counter-less bring-up images.
        frequency = 1;
    }
    frequency as u32
}

pub fn install(window: &Rc<MinimalSoftwareWindow>) {
    install_with_windows(window,&[]);
}

/// The primary adapter is consumed first, followed by explicitly supplied
/// private render targets. Ordinary one-window callers retain their behavior.
pub fn install_with_windows(window: &Rc<MinimalSoftwareWindow>,additional:&[Rc<MinimalSoftwareWindow>]) {
    let mut adapters=VecDeque::new();adapters.push_back(window.clone());
    adapters.extend(additional.iter().cloned());
    slint::platform::set_platform(Box::new(GuestPlatform {
        window: window.clone(),
        adapters: RefCell::new(adapters),
        origin: counter_ticks(),
        frequency: counter_frequency(),
    }))
    .expect("install Slint guest platform");
}

pub fn next_wake(window: &MinimalSoftwareWindow) -> Option<Duration> {
    next_wake_delay(
        window.window().has_active_animations(),
        slint::platform::duration_until_next_timer_update(),
    )
}

/// Publish demand to the read-only clock client in the phone's input poller.
///
/// # Safety
/// A supplied pointer must address a mapped, writable, aligned atomic word,
/// with this PD as its sole writer.
pub unsafe fn arm_phone_frame(
    window: &MinimalSoftwareWindow,
    request: Option<core::ptr::NonNull<core::sync::atomic::AtomicU64>>,
) {
    if let Some(request) = request {
        crate::frame_wake::publish(
            unsafe { request.as_ref() },
            counter_ticks(),
            counter_frequency(),
            next_wake(window),
        );
    }
}
