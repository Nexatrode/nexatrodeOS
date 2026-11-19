#![no_std]
#![no_main]
mod pixel_pas_metadata;
mod pixel_probe_display;

use personal_os_rust_server::qcom_pas_metadata::{self as metadata, CAPACITY, MAPPING_SIZE, PAGE};
use pixel_pas_metadata::Buffer;
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

static MDT: &[u8] = include_bytes!(env!("SUNFISH_MODEM_MDT_PATH"));

#[protection_domain]
fn init() -> MetadataProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    display.clear();
    display.line(0, "PAS METADATA PREPARE");
    display.line(1, "CHECK AND STAGE MDT");
    display.line(26, "NO SECURE CALL");
    display.line(27, "NO FIRMWARE START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    let physical = *sel4_microkit::var!(pas_metadata_paddr:usize = 0) as u64;
    let base = memory_region_symbol!(pas_metadata:*mut u8).as_ptr();
    let mut buffer = unsafe { Buffer::new(base) };
    let result = metadata::prepare(&mut buffer, physical, MDT);
    display.clear();
    display.line(0, "PAS METADATA PREPARE");
    display.line(
        1,
        if result.is_ok() {
            "METADATA STAGE PASS"
        } else {
            "METADATA STAGE FAIL"
        },
    );
    if let Err(error) = result {
        display.hex(2, "ERROR", error.code());
    } else {
        display.line(2, "COPY AND GUARDS OK");
    }
    display.hex(3, "BUFFER PHYS HI", (physical >> 32) as u32);
    display.hex(4, "BUFFER PHYS LO", physical as u32);
    display.hex(5, "REGION BYTES", MAPPING_SIZE as u32);
    display.hex(6, "GUARD BYTES", PAGE as u32);
    display.hex(7, "METADATA LIMIT", CAPACITY as u32);
    display.hex(8, "INPUT BYTES", MDT.len() as u32);
    if let Ok(prepared) = result {
        display.hex(9, "MDT PHYS HI", (prepared.metadata_physical >> 32) as u32);
        display.hex(10, "MDT PHYS LO", prepared.metadata_physical as u32);
        display.hex(11, "STAGED BYTES", prepared.metadata_length as u32);
        display.hex(12, "PROGRAM HEADERS", prepared.program_headers as u32);
        display.hex(13, "LOAD SEGMENTS", prepared.loadable_segments as u32);
        display.hex(14, "LINKED START", prepared.linked_start);
        display.hex(15, "LINKED END", prepared.linked_end);
        display.line(16, "ALL LOADS RELOCATABLE");
        display.line(17, "NO SEGMENT OVERLAP");
        display.line(18, "MDT READBACK PARSED");
        display.line(19, "BOTH GUARD PAGES OK");
        display.line(20, "UNUSED PADDING ZERO");
    }
    display.line(21, "BYTE VOLATILE ACCESS");
    display.line(22, "OWNED RAM DEVICE NX");
    display.line(23, "MODEM POOL NOT MAPPED");
    display.line(24, "NO SEGMENT FILES LOADED");
    display.line(25, "NO RESOURCE VOTES");
    display.line(26, "NO SECURE CALL");
    display.line(27, "NO FIRMWARE START");
    display.line(28, "NO RESET RELEASE");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("PAS-METADATA|physical={:X} result={:X?}", physical, result);
    MetadataProbe
}
struct MetadataProbe;
impl Handler for MetadataProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
