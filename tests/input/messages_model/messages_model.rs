#![allow(dead_code)]

// Path-include the shared codec + Messages model so `#[cfg(test)]` modules
// inside those files run under a host Cargo package with `aes-gcm-siv`.
#[path = "../../../rust/src/record_crypto.rs"]
mod record_crypto;
#[path = "../../../rust/src/message_seal.rs"]
mod message_seal;
#[path = "../../../rust/src/vault_crypto.rs"]
mod vault_crypto;
#[path = "../../../rust/src/vault_record.rs"]
mod vault_record;
#[path = "../../../rust/src/vault_blob_record.rs"]
mod vault_blob_record;
#[path = "../../../rust/src/messages.rs"]
mod messages;
