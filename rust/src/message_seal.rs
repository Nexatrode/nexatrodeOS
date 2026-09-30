//! PSL1 sealed-message codec (AES-256-GCM-SIV) for Messages and net demos.

use crate::record_crypto::{self, KEY_SIZE, NONCE_SIZE, TAG_SIZE};

pub const MAGIC: [u8; 4] = *b"PSL1";
pub const KEY_ID_SIZE: usize = 8;
pub const HEADER_SIZE: usize = 4 + KEY_ID_SIZE + 4 + NONCE_SIZE; // 28
pub const MAX_PLAINTEXT: usize = 32;
pub const MAX_PACKET: usize = HEADER_SIZE + MAX_PLAINTEXT + TAG_SIZE;

/// Fixed UDP demo key-id and AES key (non-production).
pub const DEMO_KEY_ID: [u8; KEY_ID_SIZE] = *b"DEADBEEF";
pub const DEMO_KEY: [u8; KEY_SIZE] = *b"PERSONAL-OS-UDP-SEAL-DEMO-KEY!!!"; // 32
pub const DEMO_NONCE: [u8; NONCE_SIZE] = [0x11; NONCE_SIZE];
pub const DEMO_PLAINTEXT: &[u8] = b"HELLO";

/// Messages demo key: domain prefix || 8-byte peer key id.
pub fn messages_demo_key(peer_key_id: &[u8; KEY_ID_SIZE]) -> [u8; KEY_SIZE] {
    let mut key = [0u8; KEY_SIZE];
    key[..24].copy_from_slice(b"PERSONAL-OS-MSG-DEMO-KEY");
    key[24..32].copy_from_slice(peer_key_id);
    key
}

fn aad(key_id: &[u8; KEY_ID_SIZE]) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[..4].copy_from_slice(&MAGIC);
    out[4..].copy_from_slice(key_id);
    out
}

/// Seal plaintext into `output`. Returns packet length.
pub fn seal(
    key: &[u8; KEY_SIZE],
    key_id: &[u8; KEY_ID_SIZE],
    nonce: &[u8; NONCE_SIZE],
    plaintext: &[u8],
    output: &mut [u8],
) -> Option<usize> {
    if plaintext.is_empty() || plaintext.len() > MAX_PLAINTEXT {
        return None;
    }
    let packet_len = HEADER_SIZE + plaintext.len() + TAG_SIZE;
    if output.len() < packet_len {
        return None;
    }
    output[..4].copy_from_slice(&MAGIC);
    output[4..12].copy_from_slice(key_id);
    output[12..16].copy_from_slice(&(plaintext.len() as u32).to_le_bytes());
    output[16..28].copy_from_slice(nonce);
    let cipher_end = 28 + plaintext.len();
    output[28..cipher_end].copy_from_slice(plaintext);
    let tag = record_crypto::encrypt(key, nonce, &aad(key_id), &mut output[28..cipher_end])?;
    output[cipher_end..cipher_end + TAG_SIZE].copy_from_slice(&tag);
    Some(packet_len)
}

/// Unseal a PSL1 packet in place. Returns plaintext length; plaintext sits at offset 28.
pub fn unseal(key: &[u8; KEY_SIZE], packet: &mut [u8]) -> Option<usize> {
    if packet.len() < HEADER_SIZE + TAG_SIZE {
        return None;
    }
    if packet[..4] != MAGIC {
        return None;
    }
    let mut key_id = [0u8; KEY_ID_SIZE];
    key_id.copy_from_slice(&packet[4..12]);
    let pt_len = u32::from_le_bytes(packet[12..16].try_into().ok()?) as usize;
    if pt_len == 0 || pt_len > MAX_PLAINTEXT {
        return None;
    }
    if packet.len() != HEADER_SIZE + pt_len + TAG_SIZE {
        return None;
    }
    let mut nonce = [0u8; NONCE_SIZE];
    nonce.copy_from_slice(&packet[16..28]);
    let cipher_end = 28 + pt_len;
    let mut tag = [0u8; TAG_SIZE];
    tag.copy_from_slice(&packet[cipher_end..cipher_end + TAG_SIZE]);
    if !record_crypto::decrypt(key, &nonce, &aad(&key_id), &mut packet[28..cipher_end], &tag) {
        return None;
    }
    Some(pt_len)
}

/// Build the fixed UDP demo packet used by host Python and the guest smoke.
pub fn demo_udp_packet(output: &mut [u8; MAX_PACKET]) -> usize {
    seal(
        &DEMO_KEY,
        &DEMO_KEY_ID,
        &DEMO_NONCE,
        DEMO_PLAINTEXT,
        output,
    )
    .expect("demo seal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_messages_demo_key() {
        let key_id = *b"BE12CD34";
        let key = messages_demo_key(&key_id);
        let mut packet = [0u8; MAX_PACKET];
        let len = seal(&key, &key_id, &[0x22; NONCE_SIZE], b"HI", &mut packet).unwrap();
        let pt_len = unseal(&key, &mut packet[..len]).unwrap();
        assert_eq!(&packet[28..28 + pt_len], b"HI");
    }

    #[test]
    fn demo_udp_packet_unseals_to_hello() {
        let mut packet = [0u8; MAX_PACKET];
        let len = demo_udp_packet(&mut packet);
        let pt_len = unseal(&DEMO_KEY, &mut packet[..len]).unwrap();
        assert_eq!(&packet[28..28 + pt_len], DEMO_PLAINTEXT);
        assert_eq!(&packet[4..12], &DEMO_KEY_ID);
    }
}
