#![no_std]
#![no_main]

mod pixel_probe_display;

use personal_os_rust_server::qcom_cmd_db::{Database, Fault, HEADER_BYTES};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const SNAPSHOT_BYTES: usize = 0x2000;
const EXPECTED_DB_BASE: u32 = 0x85f2_0000;
const TURBO_CONSUMER_LEVEL: u32 = 385;

fn read_word(base: *const u32, offset: usize) -> u32 {
    let word = u32::from_le(unsafe { base.add(offset / 4).read_volatile() });
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    word
}

fn dictionary(base: *const u32) -> (u32, u32) {
    (read_word(base, 0x0c), read_word(base, 0x10))
}

fn report_dictionary(display: &Display, physical: u32, length: u32) {
    display.clear();
    display.line(0, "RPMH RESOURCES");
    display.line(1, "READ ONLY");
    display.hex(2, "DICT BASE", physical);
    display.hex(3, "DICT SIZE", length);
    display.line(27, "NO VOTES SENT");
    display.line(29, "RESTART WHEN DONE");
}

fn copied_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([value[0], value[1]]))
}

fn copied_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn report_fault(display: &Display, physical: u32, bytes: &[u8], fault: Fault) {
    // All detail comes from the bounded RAM copy, not new MMIO accesses.
    report_dictionary(display, physical, bytes.len() as u32);
    display.line(1, "READ ONLY STABLE");
    display.hex(4, "DB VERSION", copied_u32(bytes, 0).unwrap_or(u32::MAX));
    display.hex(5, "DB ERROR", fault.error.code());
    display.hex(
        6,
        "FAULT SLOT",
        fault.slot.map(u32::from).unwrap_or(u32::MAX),
    );
    display.hex(
        7,
        "FAULT ENTRY",
        fault.entry.map(u32::from).unwrap_or(u32::MAX),
    );
    for (slot, labels) in [
        ["T0 SID COUNT", "T0 HDR AUX"],
        ["T1 SID COUNT", "T1 HDR AUX"],
        ["T2 SID COUNT", "T2 HDR AUX"],
        ["T3 SID COUNT", "T3 HDR AUX"],
        ["T4 SID COUNT", "T4 HDR AUX"],
        ["T5 SID COUNT", "T5 HDR AUX"],
        ["T6 SID COUNT", "T6 HDR AUX"],
        ["T7 SID COUNT", "T7 HDR AUX"],
    ]
    .into_iter()
    .enumerate()
    {
        let start = 8 + slot * 16;
        if let (Some(slave), Some(count), Some(header), Some(auxiliary)) = (
            copied_u16(bytes, start),
            copied_u16(bytes, start + 6),
            copied_u16(bytes, start + 2),
            copied_u16(bytes, start + 4),
        ) {
            display.hex(
                8 + slot * 2,
                labels[0],
                (u32::from(slave) << 16) | u32::from(count),
            );
            display.hex(
                9 + slot * 2,
                labels[1],
                (u32::from(header) << 16) | u32::from(auxiliary),
            );
        }
    }
    if let (Some(slot), Some(entry)) = (fault.slot, fault.entry) {
        if let Some(header) = copied_u16(bytes, 8 + usize::from(slot) * 16 + 2) {
            let start = HEADER_BYTES + usize::from(header) + usize::from(entry) * 24;
            if let (Some(low), Some(high)) =
                (copied_u32(bytes, start), copied_u32(bytes, start + 4))
            {
                display.hex(26, "ENTRY ID LOW", low);
                display.hex(28, "ENTRY ID HIGH", high);
            }
            if let (Some(address), Some(length), Some(offset)) = (
                copied_u32(bytes, start + 16),
                copied_u16(bytes, start + 20),
                copied_u16(bytes, start + 22),
            ) {
                display.hex(24, "ENTRY ADDR", address);
                display.hex(
                    25,
                    "ENTRY LEN OFF",
                    (u32::from(length) << 16) | u32::from(offset),
                );
            }
        }
    }
    display.flush();
    debug_println!("RPMH-READ|PARSE_FAULT|{:?}", fault);
}

fn snapshot(base: *const u32, bytes: &mut [u8], length: usize) -> bool {
    for _ in 0..3 {
        for index in 0..length / 4 {
            bytes[index * 4..index * 4 + 4]
                .copy_from_slice(&read_word(base, index * 4).to_le_bytes());
        }
        let mut stable = true;
        for index in 0..length / 4 {
            if bytes[index * 4..index * 4 + 4] != read_word(base, index * 4).to_le_bytes() {
                stable = false;
                break;
            }
        }
        if stable {
            return true;
        }
    }
    false
}

fn show_arc(
    display: &Display,
    db: &Database<'_>,
    name: &str,
    row: usize,
    labels: [&str; 3],
) -> bool {
    let resource = match db.lookup(name) {
        Ok(resource) => resource,
        Err(error) => {
            display.hex(row, "LOOKUP ERROR", error.code());
            return false;
        }
    };
    display.hex(row, labels[0], resource.address);
    debug_println!(
        "RPMH-READ|RESOURCE|{} addr={:08X} priority={:08X}/{:08X} aux_version={:04X} aux={:02X?}",
        name,
        resource.address,
        resource.priority[0],
        resource.priority[1],
        resource.auxiliary_version,
        resource.auxiliary
    );
    match resource
        .arc_levels()
        .and_then(|levels| levels.select_at_least(TURBO_CONSUMER_LEVEL))
    {
        Ok(selection) => {
            display.hex(row + 1, labels[1], u32::from(selection.selector));
            display.hex(row + 2, labels[2], selection.consumer_level);
            true
        }
        Err(error) => {
            display.hex(row + 1, "ARC ERROR", error.code());
            false
        }
    }
}

fn read_resources(display: &Display) {
    let dict = memory_region_symbol!(cmd_db_dictionary: *mut u32)
        .as_ptr()
        .cast_const();
    let (physical, length) = dictionary(dict);
    if dictionary(dict) != (physical, length) {
        report_dictionary(display, physical, length);
        display.line(5, "DICTIONARY UNSTABLE");
        display.flush();
        return;
    }
    report_dictionary(display, physical, length);
    if physical != EXPECTED_DB_BASE
        || length < HEADER_BYTES as u32
        || length as usize > SNAPSHOT_BYTES
        || length % 4 != 0
    {
        display.line(5, "DICTIONARY REFUSED");
        display.flush();
        return;
    }
    display.line(5, "DB READ START");
    display.flush();
    let base = memory_region_symbol!(cmd_db_snapshot: *mut u32)
        .as_ptr()
        .cast_const();
    let mut bytes = [0; SNAPSHOT_BYTES];
    let stable = snapshot(base, &mut bytes, length as usize);
    display.line(6, if stable { "STABLE YES" } else { "STABLE NO" });
    if !stable {
        display.flush();
        return;
    }
    // Recheck the dictionary after the copy as well; never follow a new pointer.
    if dictionary(dict) != (physical, length) {
        report_dictionary(display, physical, length);
        display.line(5, "DICTIONARY CHANGED");
        display.flush();
        return;
    }
    let db = match Database::parse_detailed(&bytes[..length as usize]) {
        Ok(db) => db,
        Err(fault) => {
            report_fault(display, physical, &bytes[..length as usize], fault);
            return;
        }
    };
    // Clear the temporary stage marker before drawing the version/entry count.
    display.clear();
    display.line(0, "RPMH RESOURCES");
    display.line(1, "READ ONLY");
    display.hex(2, "DICT BASE", physical);
    display.hex(3, "DICT SIZE", length);
    display.hex(4, "DB VERSION", db.version());
    display.hex(5, "DB ENTRIES", db.entry_count() as u32);
    display.line(6, "STABLE YES");
    debug_println!(
        "RPMH-READ|DATABASE|base={:08X} size={:08X} version={:08X} checksum={:08X} standalone={} entries={}",
        physical,
        length,
        db.version(),
        db.checksum(),
        db.standalone(),
        db.entry_count()
    );
    let mut valid = true;
    for (name, row, labels) in [
        ("cx.lvl", 7, ["CX ADDR", "CX TURBO SEL", "CX LEVEL"]),
        ("mss.lvl", 10, ["MSS ADDR", "MSS TURBO SEL", "MSS LEVEL"]),
        ("mx.lvl", 13, ["MX ADDR", "MX TURBO SEL", "MX LEVEL"]),
    ] {
        valid &= show_arc(display, &db, name, row, labels);
    }
    match db.lookup("xo.lvl").and_then(|resource| {
        resource.command_address(3)?;
        Ok(resource)
    }) {
        Ok(resource) => {
            display.hex(16, "XO ADDR", resource.address);
            display.hex(17, "XO AUX VERSION", u32::from(resource.auxiliary_version));
            debug_println!(
                "RPMH-READ|RESOURCE|xo.lvl addr={:08X} priority={:08X}/{:08X} aux_version={:04X}",
                resource.address,
                resource.priority[0],
                resource.priority[1],
                resource.auxiliary_version
            );
        }
        Err(error) => {
            display.hex(16, "XO ERROR", error.code());
            valid = false;
        }
    }
    display.line(18, "RSC READ START");
    display.flush();
    let rsc = memory_region_symbol!(apps_rsc_state: *mut u32)
        .as_ptr()
        .cast_const();
    let config = read_word(rsc, 0x0c);
    let tcs_count = (config >> 12) & 0x3f; // DT-selected DRV ID 2; 6 bits per DRV.
    let commands = (config >> 27) & 0x1f;
    display.hex(19, "RSC CONFIG", config);
    display.hex(20, "RSC TCS COUNT", tcs_count);
    display.hex(21, "RSC CMDS EACH", commands);
    if tcs_count >= 2 && tcs_count <= 12 && commands > 0 && commands <= 16 {
        // The DT orders ACTIVE(2), SLEEP(3), WAKE(3), CONTROL(1).
        // Read only the first two (active) TCS status registers, never an IRQ
        // clear/control/enable register, and never infer ownership from IDLE.
        display.hex(22, "RSC IRQ STATUS", read_word(rsc, 0xd00 + 4));
        display.hex(23, "ACTIVE0 STATUS", read_word(rsc, 0xd00 + 0x18));
        display.hex(24, "ACTIVE1 STATUS", read_word(rsc, 0xd00 + 672 + 0x18));
        display.line(
            25,
            if valid {
                "READ ONLY DONE"
            } else {
                "RESOURCE CHECK FAILED"
            },
        );
    } else {
        display.line(25, "RSC GEOMETRY REFUSED");
    }
    display.line(27, "NO VOTES SENT");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
}

#[protection_domain]
fn init() -> RpmhReadProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "RPMH RESOURCES");
    display.line(1, "READ ONLY");
    display.line(2, "DICTIONARY READ START");
    display.line(27, "NO VOTES SENT");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    read_resources(&display);
    debug_println!("RPMH-READ|DONE|hold screen until physical restart");
    RpmhReadProbe
}

struct RpmhReadProbe;
impl Handler for RpmhReadProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
