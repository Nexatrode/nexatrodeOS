//! Standalone host helper: emit the fixed PSL1 UDP demo datagram.
use aes_gcm_siv::{
    Aes256GcmSiv, Nonce,
    aead::{Aead, KeyInit},
};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const MAGIC: &[u8] = b"PSL1";
const KEY_ID: &[u8] = b"DEADBEEF";
const KEY: &[u8; 32] = b"PERSONAL-OS-UDP-SEAL-DEMO-KEY!!!";
const NONCE: [u8; 12] = [0x11; 12];
const PLAINTEXT: &[u8] = b"HELLO";

fn main() {
    let cipher = Aes256GcmSiv::new_from_slice(KEY).expect("key");
    let nonce = Nonce::from_slice(&NONCE);
    let mut aad = [0u8; 12];
    aad[..4].copy_from_slice(MAGIC);
    aad[4..].copy_from_slice(KEY_ID);
    let body = cipher
        .encrypt(nonce, aes_gcm_siv::aead::Payload {
            msg: PLAINTEXT,
            aad: &aad,
        })
        .expect("encrypt");
    let mut packet = Vec::with_capacity(28 + body.len());
    packet.extend_from_slice(MAGIC);
    packet.extend_from_slice(KEY_ID);
    packet.extend_from_slice(&(PLAINTEXT.len() as u32).to_le_bytes());
    packet.extend_from_slice(&NONCE);
    packet.extend_from_slice(&body);
    match env::args().nth(1).as_deref() {
        None | Some("-") => io::stdout().write_all(&packet).unwrap(),
        Some(path) => {
            if let Err(error) = fs::write(path, &packet) {
                eprintln!("{error}");
                process::exit(1);
            }
        }
    }
}
