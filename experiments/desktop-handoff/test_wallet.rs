#![no_std]
#![no_main]

#[path = "../../rust/src/vault_protocol.rs"]
mod vault_protocol;

use personal_os_rust_server::{
    device_lock::{Credential, LockSettings},
    vault_blob_transfer::BlobTransferPage,
    vault_file::VaultFileId,
    vault_key_prototype as wrapping, wrapped_vault_bootstrap as bootstrap,
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};
use vault_protocol::*;

const MANAGER: Channel = Channel::new(0);
const LOCK: Channel = Channel::new(10);
const WALLET: Channel = Channel::new(8);
const PASSWORD: &[u8] = b"throwaway fixture passphrase";
const CONTENT: &[u8] = b"disposable wallet state";

struct Wallet {
    file: &'static mut BlobTransferPage,
}

fn require(condition: bool, operation: &str) {
    if !condition {
        debug_println!("HANDOFF|FAIL|wallet {}", operation);
        loop {
            sel4::r#yield();
        }
    }
}

fn file_request(operation: sel4_microkit::MessageLabel, count: usize) -> MessageInfo {
    set_mr(0, VaultFileId::WalletState as u64);
    set_mr(1, 0);
    set_mr(2, CONTENT.len() as u64);
    WALLET.pp_call(MessageInfo::new(operation, count))
}

impl Wallet {
    fn verify(&mut self) {
        self.file.set_payload(&[]);
        let result = file_request(REQUEST_FILE_READ, 3);
        require(
            result.label() == REPLY_OK
                && result.count() == 1
                && get_mr(0) == CONTENT.len() as u64
                && self.file.payload() == Some(CONTENT),
            "file readback",
        );
    }
}

#[protection_domain]
fn init() -> Wallet {
    let lock = unsafe { &mut *memory_region_symbol!(lock_page: *mut BlobTransferPage).as_ptr() };
    let file = unsafe { &mut *memory_region_symbol!(wallet_page: *mut BlobTransferPage).as_ptr() };
    let mut wallet = Wallet { file };
    require(
        file_request(REQUEST_FILE_READ, 3).label() == REPLY_DENIED,
        "locked read",
    );
    let initial = LOCK.pp_call(MessageInfo::new(REQUEST_DEVICE_LOCK_READ, 0));
    require(initial.label() == REPLY_NO_BLOB, "blank disk");
    // Public test inputs, used only with a newly created disk.
    let key = [0x42; 32];
    let envelope = wrapping::seal(PASSWORD, &[0x37; 28], &key).unwrap();
    let settings =
        LockSettings::enabled(Credential::create(PASSWORD, [0x29; 16]).unwrap(), 60_000).unwrap();
    let record =
        bootstrap::encode(&settings.encode(), &envelope, &wrapping::key_hash(&key)).unwrap();
    require(lock.set_payload(&record), "stage enrollment");
    set_mr(0, 0);
    require(
        LOCK.pp_call(MessageInfo::new(REQUEST_DEVICE_LOCK_WRITE, 1))
            .label()
            == REPLY_OK,
        "enroll",
    );
    require(
        LOCK.pp_call(MessageInfo::new(REQUEST_DEVICE_LOCK_READ, 0))
            .label()
            == REPLY_OK,
        "read enrollment",
    );
    let (_, saved, _) = bootstrap::decode(lock.payload().unwrap()).unwrap();
    let recovered = wrapping::open(PASSWORD, &saved).unwrap();
    require(lock.set_payload(recovered.as_slice()), "stage unlock");
    require(
        LOCK.pp_call(MessageInfo::new(REQUEST_DEVICE_VAULT_UNLOCK, 0))
            .label()
            == REPLY_OK,
        "unlock",
    );
    require(wallet.file.set_payload(CONTENT), "stage file");
    require(
        file_request(REQUEST_FILE_WRITE, 3).label() == REPLY_OK,
        "file write",
    );
    require(
        file_request(REQUEST_FILE_SYNC, 1).label() == REPLY_OK,
        "file sync",
    );
    wallet.verify();
    debug_println!("HANDOFF|PASS|wallet unlocked and readable");
    MANAGER.notify();
    wallet
}

impl Handler for Wallet {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(MANAGER) {
            self.verify();
            debug_println!("HANDOFF|PASS|wallet still readable");
            MANAGER.notify();
        }
        Ok(())
    }
}
