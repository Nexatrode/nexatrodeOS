#![no_std]
#![no_main]

use core::ptr;

mod entropy_protocol;
mod vault_protocol;

use entropy_protocol::{
    REPLY_OK as ENTROPY_OK, REQUEST_BYTES as ENTROPY_REQUEST, SAMPLE_BYTES as ENTROPY_SAMPLE_BYTES,
};
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, get_mr, memory_region_symbol, protection_domain, set_mr,
};
use vault_protocol::{
    REPLY_NO_BLOB, REPLY_OK, REQUEST_FILE_DELETE, REQUEST_FILE_READ, REQUEST_FILE_SIZE,
    REQUEST_FILE_SYNC, REQUEST_FILE_WRITE, REQUEST_READ, REQUEST_READ_BLOB, REQUEST_TEST_ADVANCE,
    REQUEST_WRITE, REQUEST_WRITE_BLOB, TEST_VALUE,
};

const VAULT_ENDPOINT: Channel = Channel::new(1);
const NOTES_ENDPOINT: Channel = Channel::new(3);
const MESSAGES_ENDPOINT: Channel = Channel::new(6);
const ENTROPY_ENDPOINT: Channel = Channel::new(2);
const INTERRUPTION_VALUE: MessageRegisterValue = 0x5253_544f_5245;
const BASELINE_VALUE: MessageRegisterValue = TEST_VALUE;
const BLOB_BASELINE: &[u8] = b"NOTES BASELINE";
const BLOB_INTERRUPTION: &[u8] = b"NOTES COMMIT OK";
const FILE_BASELINE: &[u8] = b"AGENT DB FILE PERSISTENCE CHECK v1\n";

#[used]
#[unsafe(link_section = ".storage_test_config")]
static STORAGE_TEST_INTERRUPTION_POINT: u32 = 0;
const STAGE_MARKERS: [&str; 4] = [
    "STORAGE|MISSING_COMMIT_STAGED",
    "STORAGE|PARTIAL_BODY_STAGED",
    "STORAGE|BAD_AUTH_TAG_STAGED",
    "STORAGE|TORN_COMMIT_STAGED",
];

#[protection_domain]
fn init() -> StorageTest {
    let interruption_point =
        unsafe { ptr::read_volatile(ptr::addr_of!(STORAGE_TEST_INTERRUPTION_POINT)) };
    let blob_transfer = memory_region_symbol!(blob_transfer: *mut BlobTransferPage);
    let messages_transfer = memory_region_symbol!(messages_transfer: *mut BlobTransferPage);
    unsafe { blob_transfer.as_ptr().write(BlobTransferPage::new()) };
    unsafe { messages_transfer.as_ptr().write(BlobTransferPage::new()) };
    let Some(entropy_fingerprint) = entropy_smoke_test() else {
        fail("VirtIO RNG did not return fresh samples");
    };
    debug_println!("STORAGE|ENTROPY|FINGERPRINT:{:016x}", entropy_fingerprint);
    debug_println!("STORAGE|ENTROPY|PASS: fresh samples delivered");
    if (5..=8).contains(&interruption_point) {
        run_blob_flush_interruption(interruption_point, unsafe { &mut *blob_transfer.as_ptr() });
    } else if interruption_point != 0 {
        run_flush_interruption(interruption_point);
    }

    let first = read_record();
    match first {
        Some((0, 0)) => {
            debug_println!("STORAGE|FIRST_BOOT_OK: blank partition read as generation 0");
            let write = write_record(REQUEST_WRITE, 0, TEST_VALUE);
            if write != Some(1) || read_record() != Some((1, TEST_VALUE)) {
                fail("first disk commit did not read back");
            }
            debug_println!("STORAGE|COMMIT_OK: generation 1 read back from vault");
        }
        Some((1, TEST_VALUE)) => {
            debug_println!("STORAGE|REBOOT_RECOVERY_OK: generation 1 survived malformed slot");
        }
        _ => fail("unexpected persistent record"),
    }
    run_file_smoke(unsafe { &mut *messages_transfer.as_ptr() });
    let Some((stage, generation)) = advance_test() else {
        fail("vault did not advance the interruption scenario");
    };
    if generation != 2 && stage != 5 {
        fail("test interruption did not target generation 2");
    }
    if stage == 5 {
        debug_println!("STORAGE|ALL_CASES_RECOVERED: generation 1 survived every interrupted slot");
    } else if (1..=4).contains(&stage) {
        debug_println!(
            "{}: generation 2 is deliberately invalid",
            STAGE_MARKERS[(stage - 1) as usize]
        );
    } else {
        fail("vault returned an unknown interruption stage");
    }
    StorageTest
}

fn run_file_smoke(transfer: &mut BlobTransferPage) {
    let db_exists = file_state(0).unwrap_or_else(|| fail("database file stat failed"));
    if !db_exists.1 {
        if !transfer.set_payload(FILE_BASELINE) || !write_file(0, 0, FILE_BASELINE.len(), transfer)
        {
            fail("could not write initial agent database file");
        }
        set_mr(0, 0);
        if MESSAGES_ENDPOINT
            .pp_call(MessageInfo::new(REQUEST_FILE_SYNC, 1))
            .label()
            != REPLY_OK
        {
            fail("could not sync initial agent database file");
        }
        debug_println!("STORAGE|FILE_COMMIT_OK: initial app-scoped database file synced");
    } else if db_exists.0 != FILE_BASELINE.len() {
        fail("agent database file length did not survive restart");
    }

    if !transfer.set_payload(&[]) {
        fail("could not clear file transfer page before read");
    }
    set_mr(0, 0);
    set_mr(1, 0);
    set_mr(2, FILE_BASELINE.len() as MessageRegisterValue);
    let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_FILE_READ, 3));
    let Some(payload) = transfer.payload() else {
        fail("vault returned an invalid file read length");
    };
    if reply.label() != REPLY_OK
        || get_mr(0) as usize != FILE_BASELINE.len()
        || payload != FILE_BASELINE
    {
        fail("agent database file contents did not survive restart");
    }
    debug_println!("STORAGE|FILE_REOPEN_OK: app-scoped file read after boot");

    if !transfer.set_payload(b"JOURNAL") || !write_file(1, 0, 7, transfer) {
        fail("could not write rollback-journal file");
    }
    set_mr(0, 1);
    if MESSAGES_ENDPOINT
        .pp_call(MessageInfo::new(REQUEST_FILE_SYNC, 1))
        .label()
        != REPLY_OK
    {
        fail("could not sync rollback-journal file");
    }
    set_mr(0, 1);
    if MESSAGES_ENDPOINT
        .pp_call(MessageInfo::new(REQUEST_FILE_DELETE, 1))
        .label()
        != REPLY_OK
    {
        fail("could not persist rollback-journal deletion");
    }
    let journal_state = file_state(1).unwrap_or_else(|| fail("journal stat failed"));
    if journal_state.0 != 0 || journal_state.1 {
        fail("deleted rollback journal still appears present");
    }
    debug_println!("STORAGE|JOURNAL_DELETE_OK: journal metadata committed");
}

fn file_state(file: u32) -> Option<(usize, bool)> {
    set_mr(0, file as MessageRegisterValue);
    let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_FILE_SIZE, 1));
    if reply.label() != REPLY_OK || reply.count() != 2 {
        return None;
    }
    Some((get_mr(0) as usize, get_mr(1) != 0))
}

fn write_file(file: u32, offset: usize, length: usize, transfer: &BlobTransferPage) -> bool {
    if transfer
        .payload()
        .is_none_or(|payload| payload.len() != length)
    {
        return false;
    }
    set_mr(0, file as MessageRegisterValue);
    set_mr(1, offset as MessageRegisterValue);
    set_mr(2, length as MessageRegisterValue);
    let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_FILE_WRITE, 3));
    reply.label() == REPLY_OK
}

fn entropy_smoke_test() -> Option<u64> {
    let mut samples = [[0u8; ENTROPY_SAMPLE_BYTES]; 4];
    for sample in &mut samples {
        let reply = ENTROPY_ENDPOINT.pp_call(MessageInfo::new(ENTROPY_REQUEST, 0));
        if reply.label() != ENTROPY_OK || reply.count() != 2 {
            return None;
        }
        sample[..8].copy_from_slice(&get_mr(0).to_le_bytes());
        sample[8..].copy_from_slice(&get_mr(1).to_le_bytes());
        if sample.iter().all(|byte| *byte == 0) {
            return None;
        }
    }
    for index in 0..samples.len() {
        if samples[..index].contains(&samples[index]) {
            return None;
        }
    }
    Some(u64::from_le_bytes(samples[0][..8].try_into().ok()?))
}

fn run_flush_interruption(point: u32) -> ! {
    if !(1..=4).contains(&point) {
        fail("unknown flush-interruption point");
    }
    match read_record() {
        Some((0, 0)) => {
            if write_record(REQUEST_WRITE, 0, BASELINE_VALUE) != Some(1) {
                fail("could not commit interruption-test baseline");
            }
            debug_println!("STORAGE|INTERRUPTION_BASELINE_OK: point={}", point);
            set_mr(0, point as MessageRegisterValue);
            let reply = VAULT_ENDPOINT.pp_call(MessageInfo::new(REQUEST_TEST_ADVANCE, 1));
            if reply.label() == REPLY_OK {
                fail("vault returned without the requested QMP reset");
            }
            fail("vault rejected the requested flush interruption");
        }
        Some((generation, value)) => {
            let recovered_old = generation == 1 && value == BASELINE_VALUE;
            let recovered_new = generation == 2 && value == INTERRUPTION_VALUE;
            let accepted = match point {
                1 | 2 => recovered_old,
                3 => recovered_old || recovered_new,
                4 => recovered_new,
                _ => false,
            };
            if !accepted {
                fail("flush interruption recovered an unexpected record");
            }
            debug_println!(
                "STORAGE|INTERRUPTION_RECOVERY_OK: point={} generation={}",
                point,
                generation
            );
            loop {
                sel4::r#yield();
            }
        }
        None => fail("flush interruption left no readable vault record"),
    }
}

fn run_blob_flush_interruption(point: u32, transfer: &mut BlobTransferPage) -> ! {
    if !(5..=8).contains(&point) {
        fail("unknown blob flush-interruption point");
    }
    let boundary = point - 4;
    match read_blob_record(transfer) {
        Ok(None) => {
            if !transfer.set_payload(BLOB_BASELINE) || write_blob_record(transfer, 0) != Some(1) {
                fail("could not commit blob interruption-test baseline");
            }
            if !transfer.set_payload(BLOB_INTERRUPTION) {
                fail("could not stage blob interruption-test payload");
            }
            debug_println!("STORAGE|BLOB_INTERRUPTION_BASELINE_OK: point={}", point);
            set_mr(0, boundary as MessageRegisterValue);
            let reply = NOTES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_TEST_ADVANCE, 1));
            if reply.label() == REPLY_OK {
                fail("vault returned without the requested blob QMP reset");
            }
            fail("vault rejected the requested blob flush interruption");
        }
        Ok(Some((generation, length))) => {
            let payload = &transfer.payload[..length];
            let recovered_old = generation == 1 && payload == BLOB_BASELINE;
            let recovered_new = generation == 2 && payload == BLOB_INTERRUPTION;
            let accepted = match boundary {
                1 | 2 => recovered_old,
                3 => recovered_old || recovered_new,
                4 => recovered_new,
                _ => false,
            };
            if !accepted {
                fail("blob flush interruption recovered unexpected note data");
            }
            debug_println!(
                "STORAGE|INTERRUPTION_RECOVERY_OK: point={} generation={}",
                point,
                generation
            );
            loop {
                sel4::r#yield();
            }
        }
        Err(()) => fail("could not read the blob interruption-test record"),
    }
}

struct StorageTest;

impl Handler for StorageTest {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("FAIL: unexpected storage test notification");
        let _ = channels;
        Ok(())
    }
}

fn read_record() -> Option<(MessageRegisterValue, MessageRegisterValue)> {
    let reply = VAULT_ENDPOINT.pp_call(MessageInfo::new(REQUEST_READ, 0));
    if reply.label() != REPLY_OK || reply.count() != 2 {
        return None;
    }
    Some((get_mr(0), get_mr(1)))
}

fn write_record(
    label: MessageLabel,
    generation: MessageRegisterValue,
    value: MessageRegisterValue,
) -> Option<MessageRegisterValue> {
    set_mr(0, generation);
    set_mr(1, value);
    let reply = VAULT_ENDPOINT.pp_call(MessageInfo::new(label, 2));
    if reply.label() != REPLY_OK || reply.count() != 1 {
        return None;
    }
    Some(get_mr(0))
}

fn read_blob_record(
    transfer: &mut BlobTransferPage,
) -> Result<Option<(MessageRegisterValue, usize)>, ()> {
    let reply = NOTES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
    if reply.label() == REPLY_NO_BLOB && reply.count() == 0 {
        return Ok(None);
    }
    if reply.label() != REPLY_OK || reply.count() != 2 {
        return Err(());
    }
    let generation = get_mr(0);
    let length = get_mr(1) as usize;
    if transfer.payload().map(|payload| payload.len()) != Some(length) {
        return Err(());
    }
    Ok(Some((generation, length)))
}

fn write_blob_record(
    transfer: &mut BlobTransferPage,
    generation: MessageRegisterValue,
) -> Option<MessageRegisterValue> {
    let length = transfer.payload()?.len();
    set_mr(0, generation);
    set_mr(1, length as MessageRegisterValue);
    let reply = NOTES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_WRITE_BLOB, 2));
    if reply.label() != REPLY_OK || reply.count() != 1 {
        return None;
    }
    Some(get_mr(0))
}

fn advance_test() -> Option<(MessageRegisterValue, MessageRegisterValue)> {
    let reply = VAULT_ENDPOINT.pp_call(MessageInfo::new(REQUEST_TEST_ADVANCE, 0));
    if reply.label() != REPLY_OK || reply.count() != 2 {
        return None;
    }
    Some((get_mr(0), get_mr(1)))
}

fn fail(reason: &str) -> ! {
    debug_println!("FAIL: storage test: {}", reason);
    loop {
        sel4::r#yield();
    }
}
