#![no_std]
#![no_main]

mod pixel_probe_display;
mod pixel_rpmh_bus;

use personal_os_rust_server::{
    qcom_bcm::Metadata,
    qcom_cmd_db::Database,
    qcom_rpmh_bus::{IPA_PATH_BCM_COUNT, read_ipa_path_bcms},
    qcom_rpmh_read::{BcmTarget, BcmTargetName},
};
use pixel_probe_display::Display;
use pixel_rpmh_bus::Controller;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const SNAPSHOT_BYTES: usize = 0x2000;
const WORK_LIMIT: u32 = 65536;

fn read_word(base: *const u32, offset: usize) -> u32 {
    let value = u32::from_le(unsafe { base.add(offset / 4).read_volatile() });
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    value
}

fn dictionary(base: *const u32) -> (u32, u32) {
    (read_word(base, 0x0c), read_word(base, 0x10))
}

fn resolve_targets(display: &Display) -> Option<[BcmTarget; IPA_PATH_BCM_COUNT]> {
    let dictionary_page = memory_region_symbol!(cmd_db_dictionary: *mut u32)
        .as_ptr()
        .cast_const();
    let expected = (0x85f2_0000, 0x1078);
    if dictionary(dictionary_page) != expected || dictionary(dictionary_page) != expected {
        display.line(1, "DICTIONARY REFUSED");
        return None;
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
            return None;
        }
    }
    if dictionary(dictionary_page) != expected {
        display.line(1, "DICTIONARY CHANGED");
        return None;
    }
    let db = match Database::parse(&bytes[..length]) {
        Ok(db) => db,
        Err(error) => {
            display.hex(1, "DATABASE ERROR", error.code());
            return None;
        }
    };
    debug_println!(
        "IPA-BCM|DB|version={:08X} entries={} stable=true",
        db.version(),
        db.entry_count()
    );
    let mut targets = [None; IPA_PATH_BCM_COUNT];
    for (index, name) in BcmTargetName::IPA_PATHS.into_iter().enumerate() {
        let resolved = db
            .lookup(name.as_str())
            .map_err(|error| error.code())
            .and_then(|resource| {
                if resource.table_slave != 5 {
                    return Err(0x200);
                }
                Metadata::from_resource(&resource).map_err(|error| error.code())
            })
            .and_then(|metadata| {
                BcmTarget::new(name, metadata.address).map_err(|error| error.code())
            });
        match resolved {
            Ok(target) => {
                debug_println!("IPA-BCM|TARGET|{}|{:08X}", name.as_str(), target.address());
                targets[index] = Some(target);
            }
            Err(code) => {
                display.line(1, "TARGET RESOLVE FAIL");
                display.hex(2, "TARGET ERROR", code);
                debug_println!("IPA-BCM|TARGET-ERROR|{}|{:08X}", name.as_str(), code);
                return None;
            }
        }
    }
    Some(core::array::from_fn(|index| targets[index].unwrap()))
}

#[protection_domain]
fn init() -> IpaPathBcmProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "IPA PATH BCM READS");
    display.line(1, "RESOLVE LIVE CMD DB");
    display.line(27, "NO BUS VOTE OR MODEM START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();

    let Some(targets) = resolve_targets(&display) else {
        display.flush();
        debug_println!("IPA-BCM|STOP|CMD-DB target resolution failed");
        return IpaPathBcmProbe;
    };
    let mut controller =
        unsafe { Controller::new(memory_region_symbol!(apps_rsc: *mut u32).as_ptr()) };
    let result = read_ipa_path_bcms(
        &mut controller,
        targets,
        WORK_LIMIT,
        || {
            for _ in 0..256 {
                unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) };
            }
        },
        |pass, target| {
            display.clear();
            display.line(0, "IPA PATH BCM READS");
            display.line(1, "RESOURCE READ ONLY");
            display.line(2, target.name().as_str());
            display.hex(3, "ADDRESS", target.address());
            display.hex(4, "PASS", u32::from(pass) + 1);
            display.line(27, "NO BUS VOTE OR MODEM START");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
        },
    );

    display.clear();
    display.line(0, "IPA PATH BCM READS");
    display.line(
        1,
        if result.passed() {
            "READS PASS"
        } else {
            "READS STOPPED"
        },
    );
    display.hex(2, "COMPLETED", u32::from(result.completed));
    for (index, target) in targets.iter().copied().enumerate() {
        for pass in 0..2 {
            let row = 3 + pass * IPA_PATH_BCM_COUNT + index;
            if let Some(value) = result.responses[pass][index] {
                display.hex(row, target.name().as_str(), value);
            } else {
                display.line(row, target.name().as_str());
            }
        }
        debug_println!(
            "IPA-BCM|RESULT|{}|first={:08X}|second={:08X}",
            target.name().as_str(),
            result.responses[0][index].unwrap_or(u32::MAX),
            result.responses[1][index].unwrap_or(u32::MAX)
        );
    }
    display.hex(25, "SUBMITTED", u32::from(result.submitted));
    display.hex(26, "HANDOFF CHECKS", u32::from(result.handoff_checks));
    if let Some(error) = result.error {
        display.hex(27, "STOP ERROR", error.code());
    } else {
        display.line(27, "ALL BANKS PRESERVED");
    }
    if let Some(error) = result.controller_error {
        display.hex(28, "CTRL ERROR", error.code());
    } else if let Some(error) = result.handoff_error {
        display.hex(28, "HANDOFF ERROR", error.code());
    } else {
        display.line(28, "NO VOTE OR WLAN START");
    }
    display.line(29, "RESTART WHEN DONE");
    display.flush();
    debug_println!("IPA-BCM|DONE|{:?}", result);
    IpaPathBcmProbe
}

struct IpaPathBcmProbe;
impl Handler for IpaPathBcmProbe {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
