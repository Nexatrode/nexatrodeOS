#![no_std]

extern crate alloc;

slint::include_modules!();

pub mod frame;
pub mod pixel_touch;

#[cfg(feature = "full-suite-ui")]
pub mod suite;
