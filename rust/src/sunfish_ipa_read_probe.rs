#![no_std]
#![no_main]

//! Candidate read-only Sunfish IPA v4.2 / GSI v2.2 register snapshot.
//!
//! The pinned Linux driver submits an interconnect vote and enables the IPA
//! clock before its first `IPA_VERSION` read. This candidate has no such
//! provider; the packaging script refuses to produce a bootable image until
//! one exists.

#[path = "freestanding.rs"]
mod freestanding;
mod pixel_probe_display;

use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const IPA_BYTES: usize = 0x34000;
const IPA_VERSION: usize = 0x034;
const IPA_COMP_CFG: usize = 0x03c;
const IPA_CLKON_CFG: usize = 0x044;
const IPA_SHARED_MEM_SIZE: usize = 0x054;
const IPA_QSB_MAX_WRITES: usize = 0x074;
const IPA_QSB_MAX_READS: usize = 0x078;
const IPA_FILT_ROUT_HASH_EN: usize = 0x148;
const IPA_BCR: usize = 0x1d0;
const IPA_COUNTER_CFG: usize = 0x1f0;
const IPA_TX_CFG: usize = 0x1fc;

// Atoll DTS places GSI at IPA base + 0x4000. The IPA mapping contains the
// entire GSI range, so this probe uses one read-only device mapping and no
// writable alias.
const GSI_BASE_OFFSET: usize = 0x4000;
const GSI_EE0_STATUS: usize = GSI_BASE_OFFSET + 0x12000;
const GSI_STATUS_ENABLED_MASK: u32 = 0x0000_0001;
const GSI_V2_2_AP_HW_PARAM_2: usize = GSI_BASE_OFFSET + 0x1f040;
const GSI_NUM_EVENT_RINGS_MASK: u32 = 0x0000_1f00;
const GSI_NUM_EVENT_RINGS_SHIFT: u32 = 8;

const OFFSETS: [usize; 12] = [
    IPA_VERSION,
    IPA_SHARED_MEM_SIZE,
    IPA_TX_CFG,
    IPA_COMP_CFG,
    IPA_CLKON_CFG,
    IPA_QSB_MAX_WRITES,
    IPA_QSB_MAX_READS,
    IPA_COUNTER_CFG,
    IPA_BCR,
    IPA_FILT_ROUT_HASH_EN,
    GSI_EE0_STATUS,
    GSI_V2_2_AP_HW_PARAM_2,
];

fn read32(base: *const u32, offset: usize) -> u32 {
    debug_assert!(offset.is_multiple_of(4));
    debug_assert!(offset + 4 <= IPA_BYTES);
    // SAFETY: the System XML gives this exact register window read-only device
    // permissions; every offset is source-pinned and checked to fit the map.
    u32::from_le(unsafe { base.add(offset / 4).read_volatile() })
}

fn render_read_checkpoint(display: &Display, sample: u32, offset: usize) {
    display.line(
        1,
        if sample == 1 {
            "SAMPLE ONE IN PROGRESS"
        } else {
            "SAMPLE TWO IN PROGRESS"
        },
    );
    display.line(2, "READ OFFSET");
    display.hex(3, "REGISTER", offset as u32);
    display.flush();
}

fn snapshot(base: *const u32, display: &Display, sample: u32) -> Option<[u32; OFFSETS.len()]> {
    let mut values = [0; OFFSETS.len()];
    render_read_checkpoint(display, sample, IPA_VERSION);
    let version = read32(base, IPA_VERSION);
    if version == 0 {
        return None;
    }
    values[0] = version;
    for (value, offset) in values.iter_mut().zip(OFFSETS).skip(1) {
        render_read_checkpoint(display, sample, offset);
        *value = read32(base, offset);
    }
    // Order the device reads before the second sample.
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    Some(values)
}

fn render_version_zero(display: &Display, sample: u32) {
    display.clear();
    display.line(0, "IPA V42 READ ONLY");
    display.line(1, "IPA VERSION ZERO");
    display.hex(2, "IPA VERSION", 0);
    display.line(18, "NO GSI WRITE OR DMA");
    display.line(19, "NO PAS OR WLAN START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("IPA-READ|VERSION_ZERO|sample={}", sample);
}

#[protection_domain]
fn init() -> IpaReadProbe {
    // SAFETY: splash is the system's dedicated display mapping.
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    // The Microkit symbol macro exposes a NonNull<*mut T> even for an XML
    // read-only mapping; immediately narrow the pointer before any access.
    let ipa = memory_region_symbol!(ipa_mmio: *mut u32)
        .as_ptr()
        .cast_const();

    display.clear();
    display.line(0, "IPA V42 READ ONLY");
    display.line(1, "SNAPSHOT IN PROGRESS");
    display.line(27, "NO GSI WRITE OR DMA");
    display.line(28, "NO PAS OR WLAN START");
    display.flush();

    let Some(first) = snapshot(ipa, &display, 1) else {
        render_version_zero(&display, 1);
        return IpaReadProbe;
    };
    let Some(second) = snapshot(ipa, &display, 2) else {
        render_version_zero(&display, 2);
        return IpaReadProbe;
    };
    let stable = first == second;
    let values = second;
    let shared_mem = values[1];
    let gsi_status = values[10];
    let gsi_hw_param_2 = values[11];
    let event_rings = (gsi_hw_param_2 & GSI_NUM_EVENT_RINGS_MASK) >> GSI_NUM_EVENT_RINGS_SHIFT;

    display.clear();
    display.line(0, "IPA V42 READ ONLY");
    display.line(
        1,
        if stable {
            "SNAPSHOT STABLE"
        } else {
            "SNAPSHOT CHANGED"
        },
    );
    display.hex(2, "IPA VERSION", values[0]);
    display.hex(3, "SHMEM BASE U8", shared_mem >> 16);
    display.hex(4, "SHMEM SIZE U8", shared_mem & 0xffff);
    display.hex(5, "TX CFG", values[2]);
    display.hex(6, "COMP CFG", values[3]);
    display.hex(7, "CLKON CFG", values[4]);
    display.hex(8, "QSB WRITES", values[5]);
    display.hex(9, "QSB READS", values[6]);
    display.hex(10, "COUNTER CFG", values[7]);
    display.hex(11, "BCR", values[8]);
    display.hex(12, "HASH ENABLE", values[9]);
    display.hex(13, "GSI STATUS", gsi_status);
    display.hex(
        14,
        "GSI ENABLED",
        u32::from(gsi_status & GSI_STATUS_ENABLED_MASK != 0),
    );
    display.hex(15, "GSI PARAM 2", gsi_hw_param_2);
    display.hex(16, "GSI EVT RINGS", event_rings);
    display.line(17, "MMIO MAPPED READ ONLY");
    display.line(18, "NO GSI WRITE OR DMA");
    display.line(19, "NO PAS OR WLAN START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();

    debug_println!(
        "IPA-READ|DONE|stable={}|version={:08X}|shared_mem={:08X}|gsi_status={:08X}|gsi_enabled={}|gsi_hw_param_2={:08X}|event_rings={}",
        stable,
        values[0],
        shared_mem,
        gsi_status,
        gsi_status & GSI_STATUS_ENABLED_MASK != 0,
        gsi_hw_param_2,
        event_rings
    );
    IpaReadProbe
}

struct IpaReadProbe;

impl Handler for IpaReadProbe {
    type Error = Infallible;

    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }

    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
