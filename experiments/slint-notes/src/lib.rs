#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

slint::include_modules!();

pub mod frame;
pub mod chat_photo_loader;
pub mod photo_preview;
#[cfg(feature = "utm-photo-worker")]
pub mod photo_cache;
#[cfg(feature = "utm-photo-worker")]
pub mod photo_rows;
#[cfg(feature = "utm-photo-worker")]
pub mod photo_read;
#[cfg(feature = "utm-photo-worker")]
pub mod photo_reader_ipc;
#[cfg(feature = "utm-photo-worker")]
#[allow(unexpected_cfgs)]
#[path = "../../../rust/src/wallet_scan_mailbox.rs"]
pub mod photo_mailbox;
#[cfg(feature = "utm-photo-worker")]
pub mod photo_ipc;
pub mod chat_drag;
pub mod frame_clock;
pub mod frame_wake;
#[cfg(target_os = "none")]
pub mod guest_platform;
pub mod pixel_touch;
pub mod qr;
pub mod wallet_status;

#[cfg(feature = "full-suite-ui")]
pub mod suite;
