#![no_std]
#![no_main]

mod pixel_probe_display;

use personal_os_rust_server::qcom_qmp::{Descriptor, QMP_DESCRIPTOR_WORDS};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const MSGRAM_BYTES: u32 = 4096;

fn read_descriptor(base: *const u32) -> [u32; QMP_DESCRIPTOR_WORDS] {
    let mut words = [0u32; QMP_DESCRIPTOR_WORDS];
    for (index, word) in words.iter_mut().enumerate() {
        *word = u32::from_le(unsafe { base.add(index).read_volatile() });
    }
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    words
}

#[protection_domain]
fn init() -> AopProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "AOP MAILBOX");
    display.line(1, "READ START");
    display.line(2, "READ ONLY");
    display.flush();
    debug_println!("AOP-QMP|READ_START|read-only MSGRAM descriptor");

    let base = memory_region_symbol!(aop_msgram: *mut u32)
        .as_ptr()
        .cast_const();
    let mut words = read_descriptor(base);
    let mut stable = false;
    for _ in 0..4 {
        let next = read_descriptor(base);
        stable = words == next;
        words = next;
        if stable {
            break;
        }
    }
    let descriptor = Descriptor::from_words(&words);
    let layout = descriptor.validate(MSGRAM_BYTES);
    debug_println!("AOP-QMP|SNAPSHOT|stable={}", stable);
    for (index, value) in words.iter().enumerate() {
        debug_println!("AOP-QMP|DESC|{}|{:08X}", index, value);
    }

    display.clear();
    display.line(0, "AOP MAILBOX");
    display.line(1, "READ ONLY SNAPSHOT");
    display.line(2, if stable { "STABLE YES" } else { "STABLE NO" });
    display.hex(3, "MAGIC", descriptor.magic);
    display.hex(4, "VERSION", descriptor.version);
    display.hex(5, "FEATURES", descriptor.features);
    match layout {
        Ok(_) => display.line(6, "LAYOUT VALID"),
        Err(error) => display.hex(6, "LAYOUT ERROR", error.code()),
    }
    display.hex(7, "REMOTE LINK", descriptor.remote.link);
    display.hex(8, "REMOTE ACK", descriptor.remote.link_ack);
    display.hex(9, "AP LINK", descriptor.local.link);
    display.hex(10, "AP ACK", descriptor.local.link_ack);
    display.hex(11, "REMOTE CH", descriptor.remote.channel);
    display.hex(12, "REMOTE CH ACK", descriptor.remote.channel_ack);
    display.hex(13, "AP CH", descriptor.local.channel);
    display.hex(14, "AP CH ACK", descriptor.local.channel_ack);
    display.hex(15, "RX OFFSET", descriptor.remote.mailbox_offset);
    display.hex(16, "RX SIZE", descriptor.remote.mailbox_size);
    display.hex(17, "TX OFFSET", descriptor.local.mailbox_offset);
    display.hex(18, "TX SIZE", descriptor.local.mailbox_size);
    if stable {
        if let Ok(layout) = layout {
            // Read only the length words, after checking both mailbox ranges.
            // No payload, shared state, or AP/AOP acknowledgement is changed.
            let receive_length = u32::from_le(unsafe {
                base.add(layout.receive.offset() as usize / 4)
                    .read_volatile()
            });
            let transmit_length = u32::from_le(unsafe {
                base.add(layout.transmit.offset() as usize / 4)
                    .read_volatile()
            });
            display.hex(19, "RX BYTES", receive_length);
            display.hex(20, "TX BYTES", transmit_length);
            let lengths_valid = layout
                .receive
                .validate_packet_length(receive_length)
                .is_ok()
                && layout
                    .transmit
                    .validate_packet_length(transmit_length)
                    .is_ok();
            display.line(
                21,
                if lengths_valid {
                    "LENGTHS VALID"
                } else {
                    "LENGTHS INVALID"
                },
            );
            debug_println!(
                "AOP-QMP|LENGTHS|rx={} tx={} valid={}",
                receive_length,
                transmit_length,
                lengths_valid
            );
        }
    }
    display.line(23, "NO COMMAND SENT");
    display.flush();
    debug_println!("AOP-QMP|DONE|hold screen until physical restart");
    AopProbe
}

struct AopProbe;

impl Handler for AopProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
