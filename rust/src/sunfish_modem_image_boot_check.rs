#![no_std]
#![no_main]
mod pixel_probe_display;
use personal_os_rust_server::qcom_modem_image::{FILE_BYTES, Plan, Source};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, memory_region_symbol, protection_domain,
};
include!(env!("SUNFISH_MODEM_SOURCES_RS_PATH"));
static MDT: &[u8] = include_bytes!(env!("SUNFISH_MODEM_MDT_PATH"));

#[protection_domain]
fn init() -> ImageBootCheck {
    // Retain the same build manifest and large-image geometry. These are
    // patched pointer values only: none of their mapped memory is accessed.
    core::hint::black_box([
        memory_region_symbol!(apps_rsc:*mut u32).as_ptr() as usize,
        memory_region_symbol!(aop_msgram:*mut u32).as_ptr() as usize,
        memory_region_symbol!(aop_doorbell:*mut u32).as_ptr() as usize,
        memory_region_symbol!(pas_metadata:*mut u8).as_ptr() as usize,
        memory_region_symbol!(modem_image_pool:*mut u8).as_ptr() as usize,
        memory_region_symbol!(modem_gap_snapshot:*mut u8).as_ptr() as usize,
        *sel4_microkit::var!(pas_metadata_paddr:usize=0),
    ]);
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    d.clear();
    d.line(0, "MODEM IMAGE BOOT");
    d.line(1, "BOOT CHECK START");
    d.line(27, "NO MODEM MEMORY ACCESS");
    d.line(28, "NO SECURE CALL");
    d.flush();
    if let Err(e) = Plan::new(MDT, core::hint::black_box(&FIRMWARE_SOURCES)) {
        d.clear();
        d.line(0, "MODEM IMAGE BOOT");
        d.line(1, "BOOT PLAN FAIL");
        d.hex(2, "ERROR", e.code());
        d.flush();
        return ImageBootCheck;
    }
    let mut bytes = 0u32;
    let mut sum = 0u32;
    for source in &FIRMWARE_SOURCES {
        for offset in 0..source.bytes.len() {
            // Volatile reads retain and exercise the entire pinned payload.
            sum = sum.wrapping_add(u32::from(unsafe {
                source.bytes.as_ptr().add(offset).read_volatile()
            }));
            bytes += 1;
        }
    }
    d.clear();
    d.line(0, "MODEM IMAGE BOOT");
    d.line(
        1,
        if bytes == FILE_BYTES as u32 && sum == FIRMWARE_BYTE_SUM {
            "BOOT CHECK PASS"
        } else {
            "BOOT CHECK FAIL"
        },
    );
    d.hex(2, "READ BYTES", bytes);
    d.hex(3, "PAYLOAD SUM", sum);
    d.line(7, "LARGE PAYLOAD READ");
    d.line(8, "NO MODEM MEMORY ACCESS");
    d.line(9, "NO SECURE CALL");
    d.line(10, "NO RESOURCE VOTES");
    d.line(11, "NO FIRMWARE START");
    d.line(15, "PLEASE READ STATUS");
    d.flush();
    ImageBootCheck
}
struct ImageBootCheck;
impl Handler for ImageBootCheck {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
