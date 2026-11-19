#![no_std]

extern crate alloc;

slint::include_modules!();

pub mod frame;
pub mod frame_clock;
pub mod frame_wake;
#[cfg(target_os = "none")]
pub mod guest_platform;
pub mod pixel_touch;
pub mod qr;
pub mod wallet_status;

#[cfg(feature = "full-suite-ui")]
pub mod suite;
