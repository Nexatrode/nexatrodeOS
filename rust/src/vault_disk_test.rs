#![no_std]
#![no_main]
#![allow(long_running_const_eval)]

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
    PHOTO_CHUNK, PHOTO_MAX, REPLY_NO_BLOB, REPLY_OK, REPLY_DENIED, LIBRARY_OWNER,
    REQUEST_LIBRARY_CLONE, REQUEST_LIBRARY_NEXT, REQUEST_LIBRARY_DELETE, REQUEST_LIBRARY_IMPORT, REQUEST_LIBRARY_COPY_TO_CHAT, REQUEST_ATTACHMENT_COMMIT,
    REQUEST_FILE_DELETE, REQUEST_FILE_READ, REQUEST_FILE_SIZE, REQUEST_FILE_SYNC, REQUEST_FILE_WRITE,
    REQUEST_HISTORY_APPEND, REQUEST_HISTORY_END, REQUEST_HISTORY_READ, REQUEST_HISTORY_READ_BATCH, REQUEST_PHOTO_COMMIT,
    REQUEST_PHOTO_PURGE_CHAT, REQUEST_PHOTO_READ,
    REQUEST_PHOTO_STAGE, REQUEST_READ, REQUEST_READ_BLOB, REQUEST_TEST_ADVANCE,
    REQUEST_WRITE, REQUEST_WRITE_BLOB, TEST_VALUE, REPLY_CONFLICT,
    REQUEST_WALLET_IDENTITY_CREATE, REQUEST_WALLET_IDENTITY_READ, WALLET_IDENTITY_SIZE,
    REQUEST_WALLET_TX_STAGE, REQUEST_WALLET_TX_COMMIT, REQUEST_WALLET_TX_READ,
    WALLET_TX_CHUNK, WALLET_TX_MAX,
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
    let wallet_transfer = memory_region_symbol!(wallet_identity_transfer: *mut BlobTransferPage);
    run_wallet_identity_smoke(unsafe { &mut *wallet_transfer.as_ptr() });
    run_wallet_pin_smoke(unsafe { &mut *wallet_transfer.as_ptr() });
    run_wallet_transaction_smoke(unsafe { &mut *wallet_transfer.as_ptr() });
    run_wallet_node_smoke(unsafe { &mut *wallet_transfer.as_ptr() });
    run_file_smoke(unsafe { &mut *messages_transfer.as_ptr() });
    run_history_smoke(unsafe { &mut *messages_transfer.as_ptr() });
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

fn run_wallet_pin_smoke(transfer: &mut BlobTransferPage) {
    const WALLET: Channel = Channel::new(8);
    const ENVELOPE: [u8; personal_os_rust_server::wallet_pin::ENVELOPE_SIZE] =
        [0x4a; personal_os_rust_server::wallet_pin::ENVELOPE_SIZE];
    let read = || MessageInfo::new(vault_protocol::REQUEST_WALLET_PIN_READ, 0);
    let create = || MessageInfo::new(vault_protocol::REQUEST_WALLET_PIN_CREATE, 0);
    if MESSAGES_ENDPOINT.pp_call(read()).label() == REPLY_OK {
        fail("Messages reached the wallet PIN endpoint");
    }
    let initial = WALLET.pp_call(read()).label();
    if initial == REPLY_NO_BLOB {
        transfer.set_payload(&ENVELOPE);
        if WALLET.pp_call(create()).label() != REPLY_OK { fail("PIN envelope commit failed"); }
    } else if initial != REPLY_OK { fail("PIN envelope read failed"); }
    transfer.set_payload(&ENVELOPE);
    if WALLET.pp_call(create()).label() != REPLY_CONFLICT { fail("PIN envelope overwrite accepted"); }
    if WALLET.pp_call(read()).label() != REPLY_OK || transfer.payload() != Some(&ENVELOPE[..]) {
        fail("PIN envelope did not reopen");
    }
    transfer.set_payload(&[]);
    debug_println!("STORAGE|WALLET_PIN_{}", if initial == REPLY_NO_BLOB { "COMMIT_OK" } else { "REBOOT_OK" });
}

fn run_wallet_identity_smoke(transfer: &mut BlobTransferPage) {
    const WALLET: Channel = Channel::new(8);
    const IDENTITY: [u8; WALLET_IDENTITY_SIZE] = [0x5b; WALLET_IDENTITY_SIZE];
    let read = || MessageInfo::new(REQUEST_WALLET_IDENTITY_READ, 0);
    let create = || MessageInfo::new(REQUEST_WALLET_IDENTITY_CREATE, 0);
    if MESSAGES_ENDPOINT.pp_call(read()).label() == REPLY_OK {
        fail("Messages reached the wallet identity endpoint");
    }
    let initial = WALLET.pp_call(read()).label();
    if initial == REPLY_NO_BLOB {
        transfer.set_payload(&IDENTITY);
        if WALLET.pp_call(create()).label() != REPLY_OK || transfer.payload() != Some(&[]) {
            fail("wallet identity commit or transfer scrub failed");
        }
    } else if initial != REPLY_OK {
        fail("wallet identity could not be read at boot");
    }
    transfer.set_payload(&[0x6c; WALLET_IDENTITY_SIZE]);
    if WALLET.pp_call(create()).label() != REPLY_CONFLICT || transfer.payload() != Some(&[]) {
        fail("wallet identity overwrite was accepted or left staged bytes");
    }
    if WALLET.pp_call(read()).label() != REPLY_OK || transfer.payload() != Some(&IDENTITY[..]) {
        fail("wallet identity did not authenticate and reopen");
    }
    transfer.set_payload(&[]);
    if initial == REPLY_NO_BLOB {
        debug_println!("STORAGE|WALLET_IDENTITY_COMMIT_OK");
    } else {
        debug_println!("STORAGE|WALLET_IDENTITY_REBOOT_OK");
    }
}

fn run_wallet_node_smoke(transfer: &mut BlobTransferPage) {
    const WALLET: Channel = Channel::new(8);
    const NODE: &[u8] = b"127.0.0.1:38081";
    const REPLACEMENT: &[u8] = b"stage.example:38081";
    let read = || MessageInfo::new(vault_protocol::REQUEST_WALLET_NODE_READ, 0);
    let write = || MessageInfo::new(vault_protocol::REQUEST_WALLET_NODE_WRITE, 0);
    if MESSAGES_ENDPOINT.pp_call(read()).label() == REPLY_OK { fail("messages reached wallet node storage"); }
    let initial = WALLET.pp_call(read()).label();
    if initial == REPLY_NO_BLOB {
        for node in [NODE, REPLACEMENT] {
            transfer.set_payload(node);
            if WALLET.pp_call(write()).label() != REPLY_OK { fail("wallet node save failed"); }
            if WALLET.pp_call(read()).label() != REPLY_OK || transfer.payload() != Some(node) {
                fail("wallet node save did not read back");
            }
        }
        debug_println!("STORAGE|WALLET_NODE_COMMIT_OK");
    } else if initial == REPLY_OK && transfer.payload() == Some(REPLACEMENT) {
        debug_println!("STORAGE|WALLET_NODE_REBOOT_OK");
    } else { fail("wallet node did not persist"); }
    transfer.set_payload(&[]);
}

fn run_wallet_transaction_smoke(transfer: &mut BlobTransferPage) {
    const WALLET: Channel = Channel::new(8);
    const TRANSACTION: [u8; WALLET_TX_MAX] = [0x69; WALLET_TX_MAX];
    static REPLACEMENT: [u8; WALLET_TX_MAX] = [0x42; WALLET_TX_MAX];
    set_mr(0, 0);
    if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_WALLET_TX_READ, 1)).label() == REPLY_OK {
        fail("Messages reached wallet transaction storage");
    }
    set_mr(0, 0);
    let initial = WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_READ, 1)).label();
    if initial == REPLY_NO_BLOB {
        for (part, chunk) in TRANSACTION.chunks(WALLET_TX_CHUNK).enumerate() {
            transfer.set_payload(chunk);
            set_mr(0, (part * WALLET_TX_CHUNK) as u64);
            if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_STAGE, 1)).label() != REPLY_OK {
                fail("wallet transaction staging failed");
            }
        }
        set_mr(0, TRANSACTION.len() as u64);
        if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_COMMIT, 1)).label() != REPLY_OK {
            fail("wallet transaction commit failed");
        }
    } else if initial != REPLY_OK {
        fail("wallet transaction could not be read");
    }
    if initial == REPLY_NO_BLOB {
        // Two interrupted attempts advance the nonce reservation twice. The
        // second attempt must not overwrite chunks of the committed bank.
        for (part, chunk) in REPLACEMENT.chunks(WALLET_TX_CHUNK).enumerate() {
            transfer.set_payload(chunk);
            set_mr(0, (part * WALLET_TX_CHUNK) as u64);
            if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_STAGE, 1)).label() != REPLY_OK {
                fail("wallet transaction interruption staging failed");
            }
        }
        for _ in 0..2 {
            set_mr(0, REPLACEMENT.len() as u64);
            if WALLET.pp_call(MessageInfo::new(0x444, 1)).label() != REPLY_OK {
                fail("wallet transaction interruption simulation failed");
            }
        }
        debug_println!("STORAGE|WALLET_TX_INTERRUPTED_TWICE");
    }
    for (part, chunk) in TRANSACTION.chunks(WALLET_TX_CHUNK).enumerate() {
        set_mr(0, part as u64);
        let reply = WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_READ, 1));
        if reply.label() != REPLY_OK
            || reply.count() != if part == 0 { 1 } else { 0 }
            || (part == 0 && get_mr(0) != TRANSACTION.len() as u64)
            || transfer.payload() != Some(chunk)
        {
            fail("wallet transaction did not reopen");
        }
    }
    if initial == REPLY_NO_BLOB {
        // The staged replacement survived the simulated failures. A later
        // commit must work, then another commit restores the reboot baseline.
        set_mr(0, REPLACEMENT.len() as u64);
        if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_COMMIT, 1)).label() != REPLY_OK {
            fail("wallet transaction commit after interruptions failed");
        }
        for (part, chunk) in REPLACEMENT.chunks(WALLET_TX_CHUNK).enumerate() {
            set_mr(0, part as u64);
            if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_READ, 1)).label() != REPLY_OK
                || transfer.payload() != Some(chunk) {
                fail("wallet transaction replacement did not reopen");
            }
        }
        for (part, chunk) in TRANSACTION.chunks(WALLET_TX_CHUNK).enumerate() {
            transfer.set_payload(chunk);
            set_mr(0, (part * WALLET_TX_CHUNK) as u64);
            if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_STAGE, 1)).label() != REPLY_OK {
                fail("wallet transaction baseline restaging failed");
            }
        }
        set_mr(0, TRANSACTION.len() as u64);
        if WALLET.pp_call(MessageInfo::new(REQUEST_WALLET_TX_COMMIT, 1)).label() != REPLY_OK {
            fail("wallet transaction baseline recommit failed");
        }
        debug_println!("STORAGE|WALLET_TX_REPLACEMENT_OK");
    }
    transfer.set_payload(&[]);
    if initial == REPLY_NO_BLOB {
        debug_println!("STORAGE|WALLET_TX_COMMIT_OK");
    } else {
        debug_println!("STORAGE|WALLET_TX_REBOOT_OK");
    }
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

fn run_history_smoke(transfer: &mut BlobTransferPage) {
    const CHAT: u64 = 17;
    const FIRST: [u8; 1200] = [b'a'; 1200];
    const SECOND: [u8; 1800] = [b'b'; 1800];
    let end = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_END, 0));
    if end.label() != REPLY_OK || end.count() != 1 {
        fail("history high-water read failed");
    }
    let last = get_mr(0);
    if last == 0 {
        for (index, event) in [&FIRST[..], &SECOND[..]].into_iter().enumerate() {
            if !transfer.set_payload(event) {
                fail("could not stage history event");
            }
            set_mr(0, CHAT);
            let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_APPEND, 1));
            if reply.label() != REPLY_OK || reply.count() != 1 || get_mr(0) != index as u64 + 1 {
                fail("history append did not return the next record ID");
            }
        }
        debug_println!("STORAGE|HISTORY_COMMIT_OK");
    } else if last != 6 {
        fail("history high-water did not survive reboot");
    }
    for (index, event) in [&FIRST[..], &SECOND[..]].into_iter().enumerate() {
        set_mr(0, index as u64 + 1);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_READ, 1));
        if reply.label() != REPLY_OK || reply.count() != 2 || get_mr(0) != CHAT
            || get_mr(1) as usize != event.len() || transfer.payload() != Some(event)
        {
            fail("history event did not reopen with its owner and original bytes");
        }
    }
    set_mr(0, 1);
    set_mr(1, 2);
    let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_READ_BATCH, 2));
    if reply.label() != REPLY_OK || reply.count() != 2 || get_mr(0) != 2 {
        fail("batched history read failed");
    }
    let length = get_mr(1) as usize;
    let payload = transfer.payload().filter(|payload| payload.len() == length)
        .unwrap_or_else(|| fail("batched history payload is invalid"));
    let mut entries = personal_os_rust_server::history_batch::decode(payload, 2)
        .unwrap_or_else(|| fail("batched history framing is invalid"));
    if entries.next() != Some(personal_os_rust_server::history_batch::Entry { owner: CHAT, event: Some(&FIRST[..]) })
        || entries.next() != Some(personal_os_rust_server::history_batch::Entry { owner: CHAT, event: Some(&SECOND[..]) })
        || entries.next().is_some() {
        fail("batched history did not preserve owners and event bytes");
    }
    drop(entries);
    set_mr(0, 0);
    set_mr(1, 2);
    if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_READ_BATCH, 2)).label() == REPLY_OK {
        fail("batched history accepted an invalid range");
    }
    debug_println!("STORAGE|HISTORY_BATCH_OK");
    run_photo_reuse_smoke(transfer, last == 0);
    run_library_smoke(transfer, last == 0);
    if last == 6 {
        debug_println!("STORAGE|HISTORY_REBOOT_OK");
    }
}

fn run_library_smoke(transfer: &mut BlobTransferPage, first_boot: bool) {
    const OWNER: u64 = 21;
    const NAME: &[u8] = b"large-test.jpg";
    const DATA: [u8; PHOTO_MAX] = {
        let mut bytes = [0u8; PHOTO_MAX];
        let mut i = 0;
        while i < PHOTO_MAX { bytes[i] = (i + i / PHOTO_CHUNK) as u8; i += 1; }
        bytes[0] = 0xff; bytes[1] = 0xd8;
        bytes
    };
    if first_boot {
        for (part, chunk) in DATA.chunks(PHOTO_CHUNK).enumerate() {
            if !transfer.set_payload(chunk) { fail("library source stage page failed"); }
            set_mr(0, (part * PHOTO_CHUNK) as u64);
            if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_STAGE, 1)).label() != REPLY_OK {
                fail("library source stage failed");
            }
        }
        set_mr(0, OWNER); set_mr(1, PHOTO_MAX as u64);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_COMMIT, 2));
        if reply.label() != REPLY_OK || reply.count() != 1 { fail("library source commit failed"); }
        let source = get_mr(0);
        set_mr(0, source);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_DELETE, 1)).label() != REPLY_DENIED {
            fail("library delete accepted a chat attachment");
        }
        if !transfer.set_payload(NAME) { fail("library filename failed"); }
        set_mr(0, source); set_mr(1, OWNER + 1);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_CLONE, 2)).label() != REPLY_DENIED {
            fail("library clone accepted another chat owner");
        }
        if !transfer.set_payload(b"../bad.jpg") { fail("bad filename setup failed"); }
        set_mr(0, source); set_mr(1, OWNER);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_CLONE, 2)).label() == REPLY_OK {
            fail("library accepted path traversal");
        }
        if !transfer.set_payload(NAME) { fail("library filename failed"); }
        set_mr(0, source); set_mr(1, OWNER);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_CLONE, 2));
        if reply.label() != REPLY_OK || reply.count() != 1 { fail("library clone failed"); }
        if !transfer.set_payload(b"deleted.jpg") { fail("deleted filename failed"); }
        set_mr(0, source); set_mr(1, OWNER);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_CLONE, 2));
        if reply.label() != REPLY_OK || reply.count() != 1 { fail("second library clone failed"); }
        let deleted = get_mr(0);
        set_mr(0, deleted); set_mr(1, 22);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_COPY_TO_CHAT, 2));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            debug_println!("STORAGE|COPY_TO_CHAT_ERROR|{}", reply.label());
            fail("copy to chat failed");
        }
        let chat_copy = get_mr(0);
        if !transfer.set_payload(&chat_copy.to_le_bytes()) { fail("copy id staging failed"); }
        set_mr(0, 22);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_APPEND, 1)).label() != REPLY_OK {
            fail("copy id history failed");
        }
        set_mr(0, deleted);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_DELETE, 1)).label() != REPLY_OK {
            fail("library deletion failed");
        }
        set_mr(0, deleted); set_mr(1, 0);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2)).label() == REPLY_OK {
            fail("deleted library file remained readable");
        }
        // Import arbitrary bytes through the same staged interface used by
        // the UTM-only host fixture, then verify and remove the import.
        for (part, chunk) in DATA.chunks(PHOTO_CHUNK).enumerate() {
            if !transfer.set_payload(chunk) { fail("import stage page failed"); }
            set_mr(0, (part * PHOTO_CHUNK) as u64);
            if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_STAGE, 1)).label() != REPLY_OK {
                fail("import stage failed");
            }
        }
        if !transfer.set_payload(b"fixture.bin") { fail("import name failed"); }
        set_mr(0, DATA.len() as u64);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_IMPORT, 1));
        if reply.label() != REPLY_OK || reply.count() != 1 { fail("library import failed"); }
        let imported = get_mr(0);
        for (part, chunk) in DATA.chunks(PHOTO_CHUNK).enumerate() {
            set_mr(0, imported); set_mr(1, part as u64);
            if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2)).label() != REPLY_OK
                || transfer.payload() != Some(chunk) { fail("imported bytes changed"); }
        }
        set_mr(0, imported);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_DELETE, 1)).label() != REPLY_OK {
            fail("import cleanup failed");
        }
        debug_println!("STORAGE|LIBRARY_IMPORT_DELETE_1M_OK");
        set_mr(0, OWNER);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_PURGE_CHAT, 1)).label() != REPLY_OK {
            fail("library source chat purge failed");
        }
    }
    // The chat-owned copy must remain readable after deleting its source and
    // after every restart of this same test disk.
    set_mr(0, 5);
    let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_HISTORY_READ, 1));
    if reply.label() != REPLY_OK || get_mr(0) != 22 { fail("chat copy id missing"); }
    let Some(copy_bytes) = transfer.payload() else { fail("chat copy id payload missing"); };
    let Ok(id_bytes) = <[u8; 8]>::try_from(copy_bytes) else { fail("chat copy id invalid"); };
    let chat_copy = u64::from_le_bytes(id_bytes);
    for (part, chunk) in DATA.chunks(PHOTO_CHUNK).enumerate() {
        set_mr(0, chat_copy); set_mr(1, part as u64);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
        if reply.label() != REPLY_OK || (part == 0 && get_mr(0) != 22) || transfer.payload() != Some(chunk) {
            fail("message copy did not survive source deletion");
        }
    }
    debug_println!("STORAGE|MESSAGE_COPY_AFTER_DELETE_{}", if first_boot { "OK" } else { "REBOOT_OK" });
    set_mr(0, 0);
    let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_NEXT, 1));
    if reply.label() != REPLY_OK || reply.count() != 3 || get_mr(2) as usize != PHOTO_MAX
        || transfer.payload() != Some(NAME) { fail("library entry not found"); }
    let cursor = get_mr(0);
    let id = get_mr(1);
    for (part, chunk) in DATA.chunks(PHOTO_CHUNK).enumerate() {
        set_mr(0, id); set_mr(1, part as u64);
        let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
        if reply.label() != REPLY_OK || reply.count() != if part == 0 { 2 } else { 0 }
            || (part == 0 && (get_mr(0) != LIBRARY_OWNER || get_mr(1) as usize != PHOTO_MAX))
            || transfer.payload() != Some(chunk) { fail("library file contents changed"); }
    }
    set_mr(0, cursor);
    if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_LIBRARY_NEXT, 1)).label() != REPLY_NO_BLOB {
        fail("library unexpectedly returned another entry");
    }
    debug_println!("STORAGE|LIBRARY_1M_{}", if first_boot { "OK" } else { "REBOOT_OK" });
}

fn run_photo_reuse_smoke(transfer: &mut BlobTransferPage, first_boot: bool) {
    const REMOVED: &[u8] = &[0xff, 0xd8, 1, 2];
    const KEPT: &[u8] = &[0xff, 0xd8, 3, 4];
    const REPLACEMENT: &[u8] = &{
        // Keep the independent reuse fixture small so the 1 MiB copy tests
        // exercise the production-sized 14 MiB partition without filling it.
        let mut bytes = [0; 48 * 1024];
        let mut index = 0;
        while index < bytes.len() {
            bytes[index] = (index + index / PHOTO_CHUNK) as u8;
            index += 1;
        }
        bytes[0] = 0xff;
        bytes[1] = 0xd8;
        bytes
    };
    const OLD_ID: u64 = (1 << 32) | 1;
    const OTHER_ID: u64 = (2 << 32) | 2;
    // Slot 2 remains occupied, so the four-slot replacement must start at 3.
    const NEW_ID: u64 = (3 << 32) | 3;

    if first_boot {
        for (owner, id, bytes) in [(17, OLD_ID, REMOVED), (18, OTHER_ID, KEPT), (19, NEW_ID, REPLACEMENT)] {
            if owner == 19 {
                set_mr(0, 17);
                if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_PURGE_CHAT, 1)).label() != REPLY_OK {
                    fail("purging a chat's photos failed");
                }
            }
            for (part, chunk) in bytes.chunks(PHOTO_CHUNK).enumerate() {
                if !transfer.set_payload(chunk) {
                    fail("staging attachment payload failed");
                }
                set_mr(0, (part * PHOTO_CHUNK) as u64);
                let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_STAGE, 1));
                if reply.label() != REPLY_OK {
                    fail("staging attachment chunk failed");
                }
            }
            set_mr(0, owner);
            set_mr(1, bytes.len() as u64);
            let commit = if owner == 19 { REQUEST_ATTACHMENT_COMMIT } else { REQUEST_PHOTO_COMMIT };
            let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(commit, 2));
            if reply.label() != REPLY_OK || reply.count() != 1 || get_mr(0) != id {
                fail("attachment did not use expected physical slot and unique ID");
            }
        }
    }
    set_mr(0, OLD_ID);
    set_mr(1, 0);
    if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2)).label() == REPLY_OK {
        fail("stale photo reference opened a replacement");
    }
    for (owner, id, bytes) in [(18, OTHER_ID, KEPT), (19, NEW_ID, REPLACEMENT)] {
        for (part, chunk) in bytes.chunks(PHOTO_CHUNK).enumerate() {
            set_mr(0, id);
            set_mr(1, part as u64);
            let reply = MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
            if reply.label() != REPLY_OK
                || reply.count() != if part == 0 { 2 } else { 0 }
                || (part == 0 && (get_mr(0) != owner || get_mr(1) != bytes.len() as u64))
                || transfer.payload() != Some(chunk)
            {
                fail("attachment did not reopen with the expected owner and bytes");
            }
        }
    }
    if first_boot {
        // Stage and delete a second large item. A stale ID must not expose a
        // continuation chunk left on disk after its first block was purged.
        for (part, chunk) in REPLACEMENT.chunks(PHOTO_CHUNK).enumerate() {
            if !transfer.set_payload(chunk) { fail("large purge fixture staging failed"); }
            set_mr(0, (part * PHOTO_CHUNK) as u64);
            if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_STAGE, 1)).label() != REPLY_OK {
                fail("large purge fixture chunk failed");
            }
        }
        set_mr(0, 20);
        set_mr(1, REPLACEMENT.len() as u64);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_COMMIT, 2)).label() != REPLY_OK {
            fail("large purge fixture commit failed");
        }
        let stale = get_mr(0);
        set_mr(0, 20);
        if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_PURGE_CHAT, 1)).label() != REPLY_OK {
            fail("large purge fixture delete failed");
        }
        for part in [0, 1, 4, 5, 15] {
            set_mr(0, stale);
            set_mr(1, part);
            if MESSAGES_ENDPOINT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2)).label() == REPLY_OK {
                fail("purged attachment exposed a stale chunk");
            }
        }
    }
    if first_boot {
        debug_println!("STORAGE|PHOTO_REUSE_OK");
    } else {
        debug_println!("STORAGE|PHOTO_REUSE_REBOOT_OK");
    }
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
