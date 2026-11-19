#![no_std]
#![no_main]
//! Standalone native SMMU experiment; implementation shared with IPA startup.
#[allow(dead_code)]
mod pixel_probe_display;
#[allow(dead_code)]
mod pixel_smmu_install;

use sel4_microkit::{memory_region_symbol, protection_domain};

#[protection_domain]
fn init() -> pixel_smmu_install::Probe {
    let framebuffer = memory_region_symbol!(splash_fb: *mut u32).as_ptr();
    // SAFETY: the standalone system and actual CapDL audit prove all retained
    // SMMU/frame/IRQ mappings. There is exactly one call per cold boot.
    unsafe { pixel_smmu_install::run(framebuffer) }
}
