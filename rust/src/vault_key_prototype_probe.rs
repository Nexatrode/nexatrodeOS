#![no_std]
#![no_main]

//! Disposable-data Microkit probe. Never use these test inputs for a vault.

use personal_os_rust_server::vault_key_prototype::{open, seal};
use sel4_microkit::{ChannelSet, Handler, Infallible, debug_println, protection_domain};

#[protection_domain]
fn init() -> Probe {
    let key = [42; 32];
    let Some(envelope) = seal(b"disposable passphrase", &[7; 28], &key) else {
        debug_println!("KEY_PROBE|FAIL|seal");
        return Probe;
    };
    if open(b"disposable passphrase", &envelope).as_deref() != Some(&key) {
        debug_println!("KEY_PROBE|FAIL|roundtrip");
        return Probe;
    }
    if open(b"incorrect passphrase", &envelope).is_some() {
        debug_println!("KEY_PROBE|FAIL|wrong_password");
        return Probe;
    }
    let mut corrupted = envelope;
    corrupted[44] ^= 1;
    if open(b"disposable passphrase", &corrupted).is_some() {
        debug_println!("KEY_PROBE|FAIL|corruption");
        return Probe;
    }
    debug_println!("KEY_PROBE|PASS|roundtrip_wrong_password_tamper");
    Probe
}

struct Probe;

impl Handler for Probe {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
}
