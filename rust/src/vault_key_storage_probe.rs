#![no_std]
#![no_main]

//! Disposable test keys only. No production key enrollment or migration.
mod vault_protocol;

use core::ptr;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use personal_os_rust_server::vault_key_prototype::{open, seal};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};
use vault_protocol::{
    REPLY_NO_BLOB, REPLY_OK, REQUEST_READ_BLOB, REQUEST_TEST_ADVANCE, REQUEST_WRITE_BLOB,
};

const NOTES: Channel = Channel::new(3);
const OLD_PASSWORD: &[u8] = b"disposable old passphrase";
const NEW_PASSWORD: &[u8] = b"disposable new passphrase";
const TEST_KEY: [u8; 32] = [42; 32];

// Set with llvm-objcopy in a private build directory; 0 means normal reboot,
// 1..=4 select the existing storage-test flush interruption points.
#[used]
#[unsafe(link_section = ".storage_test_config")]
static INTERRUPTION_POINT: u32 = 0;

#[protection_domain]
fn init() -> Probe {
    let point = unsafe { ptr::read_volatile(ptr::addr_of!(INTERRUPTION_POINT)) };
    let page = memory_region_symbol!(blob_transfer: *mut BlobTransferPage);
    unsafe { page.as_ptr().write(BlobTransferPage::new()) };
    run(point, unsafe { &mut *page.as_ptr() });
    Probe
}

fn run(point: u32, page: &mut BlobTransferPage) {
    if point > 4 {
        fail("invalid interruption point");
    }
    let old = seal(OLD_PASSWORD, &[7; 28], &TEST_KEY).unwrap_or_else(|| fail("seal old"));
    let new = seal(NEW_PASSWORD, &[8; 28], &TEST_KEY).unwrap_or_else(|| fail("seal new"));
    let (generation, exists) = read(page);
    if !exists {
        if !page.set_payload(&old) {
            fail("stage baseline");
        }
        write(0);
        let (generation, exists) = read(page);
        if !exists || generation != 1 {
            fail("baseline readback");
        }
        verify(page, &old, OLD_PASSWORD, NEW_PASSWORD);
        debug_println!("KEY_STORAGE|BASELINE_OK");
        if point == 0 {
            return;
        }
        if !page.set_payload(&new) {
            fail("stage replacement");
        }
        set_mr(0, point as u64);
        let _ = NOTES.pp_call(MessageInfo::new(REQUEST_TEST_ADVANCE, 1));
        fail("interruption returned before reset");
    }
    if point == 0 {
        if generation == 1 {
            verify(page, &old, OLD_PASSWORD, NEW_PASSWORD);
            if !page.set_payload(&new) {
                fail("stage replacement");
            }
            write(1);
            let (generation, exists) = read(page);
            if !exists || generation != 2 {
                fail("replacement readback");
            }
            verify(page, &new, NEW_PASSWORD, OLD_PASSWORD);
            debug_println!("KEY_STORAGE|REPLACED_OK");
        } else if generation == 2 {
            verify(page, &new, NEW_PASSWORD, OLD_PASSWORD);
            debug_println!("KEY_STORAGE|NORMAL_REOPEN_OK");
        } else {
            fail("unexpected normal generation");
        }
        return;
    }
    match (point, generation) {
        (1 | 2 | 3, 1) => verify(page, &old, OLD_PASSWORD, NEW_PASSWORD),
        (3 | 4, 2) => verify(page, &new, NEW_PASSWORD, OLD_PASSWORD),
        _ => fail("unexpected recovered generation"),
    }
    debug_println!(
        "KEY_STORAGE|RECOVERY_OK|point={}|generation={}",
        point,
        generation
    );
}

fn read(page: &BlobTransferPage) -> (u64, bool) {
    let reply = NOTES.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
    if reply.label() == REPLY_NO_BLOB && reply.count() == 0 {
        return (0, false);
    }
    if reply.label() != REPLY_OK || reply.count() != 2 {
        fail("read response");
    }
    let generation = get_mr(0);
    if page.payload().map(|p| p.len()) != Some(get_mr(1) as usize) {
        fail("read length");
    }
    (generation, true)
}

fn write(expected: u64) {
    set_mr(0, expected);
    set_mr(
        1,
        personal_os_rust_server::vault_key_prototype::ENVELOPE_SIZE as u64,
    );
    let reply = NOTES.pp_call(MessageInfo::new(REQUEST_WRITE_BLOB, 2));
    if reply.label() != REPLY_OK || reply.count() != 1 || get_mr(0) != expected + 1 {
        fail("write response");
    }
}

fn verify(page: &BlobTransferPage, expected: &[u8], good: &[u8], bad: &[u8]) {
    let bytes = page.payload().unwrap_or_else(|| fail("invalid payload"));
    if bytes != expected
        || open(good, bytes).as_deref() != Some(&TEST_KEY)
        || open(bad, bytes).is_some()
    {
        fail("envelope recovery or authentication");
    }
}

fn fail(reason: &str) -> ! {
    debug_println!("KEY_STORAGE|FAIL|{}", reason);
    loop {
        sel4::r#yield();
    }
}

struct Probe;
impl Handler for Probe {
    type Error = Infallible;
    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
}
