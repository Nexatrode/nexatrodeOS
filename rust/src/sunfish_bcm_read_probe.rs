#![no_std]
#![no_main]

mod pixel_probe_display;

use personal_os_rust_server::{
    qcom_bcm::{Error, Metadata},
    qcom_cmd_db::Database,
};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const SNAPSHOT_BYTES: usize = 0x2000;
// The build validates these names against the final Sunfish modem/PAS routes.
const RESOURCES: [(&str, [&str; 4]); 6] = [
    (
        "MC0",
        ["MC0 ADDR", "MC0 UNIT", "MC0 WIDTH VCD", "MC0 VER RSV"],
    ),
    (
        "ACV",
        ["ACV ADDR", "ACV UNIT", "ACV WIDTH VCD", "ACV VER RSV"],
    ),
    (
        "CE0",
        ["CE0 ADDR", "CE0 UNIT", "CE0 WIDTH VCD", "CE0 VER RSV"],
    ),
    (
        "SN11",
        ["SN11 ADDR", "SN11 UNIT", "SN11 WIDTH VCD", "SN11 VER RSV"],
    ),
    (
        "SN0",
        ["SN0 ADDR", "SN0 UNIT", "SN0 WIDTH VCD", "SN0 VER RSV"],
    ),
    (
        "SH0",
        ["SH0 ADDR", "SH0 UNIT", "SH0 WIDTH VCD", "SH0 VER RSV"],
    ),
];

fn read_word(base: *const u32, offset: usize) -> u32 {
    let value = u32::from_le(unsafe { base.add(offset / 4).read_volatile() });
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    value
}

fn dictionary(base: *const u32) -> (u32, u32) {
    (read_word(base, 0x0c), read_word(base, 0x10))
}

fn read_metadata(display: &Display) {
    let dictionary_page = memory_region_symbol!(cmd_db_dictionary: *mut u32)
        .as_ptr()
        .cast_const();
    let expected = (0x85f2_0000, 0x1078);
    if dictionary(dictionary_page) != expected || dictionary(dictionary_page) != expected {
        display.line(1, "DICTIONARY REFUSED");
        return;
    }
    let base = memory_region_symbol!(cmd_db_snapshot: *mut u32)
        .as_ptr()
        .cast_const();
    let length = expected.1 as usize;
    let mut bytes = [0; SNAPSHOT_BYTES];
    for index in 0..length / 4 {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&read_word(base, index * 4).to_le_bytes());
    }
    for index in 0..length / 4 {
        if bytes[index * 4..index * 4 + 4] != read_word(base, index * 4).to_le_bytes() {
            display.line(1, "SNAPSHOT UNSTABLE");
            return;
        }
    }
    if dictionary(dictionary_page) != expected {
        display.line(1, "DICTIONARY CHANGED");
        return;
    }
    let db = match Database::parse(&bytes[..length]) {
        Ok(db) => db,
        Err(error) => {
            display.hex(1, "DATABASE ERROR", error.code());
            return;
        }
    };
    debug_println!(
        "BCM-READ|DB|version={:08X} entries={} stable=true",
        db.version(),
        db.entry_count()
    );
    let mut valid = true;
    for (index, (name, labels)) in RESOURCES.iter().enumerate() {
        let row = 3 + index * 4;
        match db
            .lookup(name)
            .map_err(Error::Database)
            .and_then(|resource| Metadata::from_resource(&resource))
        {
            Ok(metadata) => {
                display.hex(row, labels[0], metadata.address);
                display.hex(row + 1, labels[1], metadata.unit_size);
                display.hex(
                    row + 2,
                    labels[2],
                    (u32::from(metadata.width) << 8) | u32::from(metadata.clock_domain),
                );
                display.hex(
                    row + 3,
                    labels[3],
                    (u32::from(metadata.auxiliary_version) << 16) | u32::from(metadata.reserved),
                );
                debug_println!("BCM-READ|{}|{:?}", name, metadata);
            }
            Err(error) => {
                valid = false;
                display.line(row, name);
                display.hex(row + 1, "METADATA ERROR", error.code());
                debug_println!("BCM-READ|{}|ERROR {:?}", name, error);
            }
        }
    }
    display.line(
        1,
        if valid {
            "METADATA PASS"
        } else {
            "METADATA FAIL"
        },
    );
    // PASS means a stable copy and structural decode, never a bus vote.
}

#[protection_domain]
fn init() -> BcmReadProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "RPMH BUS METADATA");
    display.line(2, "READ ONLY");
    display.line(27, "NO RSC MAPPING");
    display.line(28, "NO VOTES OR MODEM START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    read_metadata(&display);
    display.flush();
    debug_println!("BCM-READ|DONE|hold until physical restart");
    BcmReadProbe
}

struct BcmReadProbe;
impl Handler for BcmReadProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
