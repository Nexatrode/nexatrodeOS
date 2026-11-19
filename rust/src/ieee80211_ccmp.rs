//! Bounded IEEE 802.11 CCMP-128 frame protection.
//!
//! Implements the AES-CCM transform used by WPA2-Personal CCMP-128 for
//! ordinary 802.11 data frames. The caller owns key installation, association
//! state, key selection, scheduling, and per-TID replay-window lifetime. This
//! module does not provide retransmission policy, fragmentation/reassembly,
//! management-frame protection, hardware offload, or a production data path.
#![forbid(unsafe_code)]

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use aes::{Aes128, Block};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::ath10k_nwifi_rx::{FrameHeaderError, FrameKind, inspect_frame};

pub const CCMP_HEADER_BYTES: usize = 8;
pub const CCMP_MIC_BYTES: usize = 8;
pub const MAX_CCMP_FRAME_BYTES: usize = 8192;
pub const MAX_PACKET_NUMBER: u64 = (1u64 << 48) - 1;

const FC_TO_DS: u16 = 0x0100;
const FC_FROM_DS: u16 = 0x0200;
const FC_RETRY: u16 = 0x0800;
const FC_POWER_MGMT: u16 = 0x1000;
const FC_MORE_DATA: u16 = 0x2000;
const FC_PROTECTED: u16 = 0x4000;
const FC_ORDER: u16 = 0x8000;
const FC_QOS_SUBTYPE: u16 = 0x0080;
const FC_SUBTYPE_BITS_4_TO_6: u16 = 0x0070;
const CCMP_EXT_IV: u8 = 0x20;
const CCM_NONCE_BYTES: usize = 13;
const CCM_LENGTH_BYTES: usize = 2;
const CCM_TAG_BYTES: usize = CCMP_MIC_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CcmpError {
    InvalidFrame(FrameHeaderError),
    NotDataFrame,
    ProtectedInputFrame,
    FrameTooLarge,
    PayloadTooLong,
    OutputTooSmall,
    InvalidKeyId,
    InvalidPacketNumber,
    PacketNumberExhausted,
    InvalidCcmpHeader,
    UnexpectedKeyId,
    Replay,
    AuthenticationFailed,
    AesInitialization,
}

/// Monotonic CCMP transmitter PN state. Start at zero for a fresh key; the
/// first call to `next` returns PN 1. A new key requires a new counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CcmpTxPacketNumber {
    last_used: u64,
}

impl CcmpTxPacketNumber {
    pub const fn new(last_used: u64) -> Result<Self, CcmpError> {
        if last_used > MAX_PACKET_NUMBER {
            Err(CcmpError::InvalidPacketNumber)
        } else {
            Ok(Self { last_used })
        }
    }

    pub const fn last_used(&self) -> u64 {
        self.last_used
    }

    fn next(&mut self) -> Result<u64, CcmpError> {
        let next = self
            .last_used
            .checked_add(1)
            .filter(|value| *value <= MAX_PACKET_NUMBER)
            .ok_or(CcmpError::PacketNumberExhausted)?;
        self.last_used = next;
        Ok(next)
    }
}

/// One monotonic receive replay window. Keep independent instances for each
/// peer/key/TID queue. A PN is committed only after CCM authentication passes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CcmpReplayWindow {
    last_authenticated: u64,
}

impl CcmpReplayWindow {
    pub const fn new(last_authenticated: u64) -> Result<Self, CcmpError> {
        if last_authenticated > MAX_PACKET_NUMBER {
            Err(CcmpError::InvalidPacketNumber)
        } else {
            Ok(Self { last_authenticated })
        }
    }

    pub const fn last_authenticated(&self) -> u64 {
        self.last_authenticated
    }

    pub fn clear(&mut self) {
        self.last_authenticated = 0;
    }

    pub fn reset_to(&mut self, packet_number: u64) -> Result<(), CcmpError> {
        if packet_number > MAX_PACKET_NUMBER {
            return Err(CcmpError::InvalidPacketNumber);
        }
        self.last_authenticated = packet_number;
        Ok(())
    }
}

/// Encrypt an unprotected data frame and append the CCMP header and MIC.
/// `output` must have room for `frame.len() + 16` bytes. `key_id` is 0..=3.
/// The transmit PN advances only after all frame and output validation passes.
pub fn encrypt_frame(
    key: &[u8; 16],
    key_id: u8,
    packet_number: &mut CcmpTxPacketNumber,
    frame: &[u8],
    output: &mut [u8],
) -> Result<usize, CcmpError> {
    if key_id > 3 {
        return Err(CcmpError::InvalidKeyId);
    }
    if frame.len() > MAX_CCMP_FRAME_BYTES {
        return Err(CcmpError::FrameTooLarge);
    }
    let view = inspect_frame(frame).map_err(CcmpError::InvalidFrame)?;
    if view.kind != FrameKind::Data {
        return Err(CcmpError::NotDataFrame);
    }
    if view.frame_control & FC_PROTECTED != 0 {
        return Err(CcmpError::ProtectedInputFrame);
    }
    let payload = view.body;
    if payload.len() > u16::MAX as usize {
        return Err(CcmpError::PayloadTooLong);
    }

    let output_len = frame
        .len()
        .checked_add(CCMP_HEADER_BYTES + CCMP_MIC_BYTES)
        .ok_or(CcmpError::FrameTooLarge)?;
    if output.len() < output_len {
        return Err(CcmpError::OutputTooSmall);
    }

    let pn = packet_number.next()?;
    let context = FrameCryptoContext::new(view.header, view.frame_control, pn)?;
    let header_len = view.header.len();
    output[..header_len].copy_from_slice(view.header);
    set_protected_bit(&mut output[..header_len]);

    let ccmp_start = header_len;
    write_ccmp_header(
        &mut output[ccmp_start..ccmp_start + CCMP_HEADER_BYTES],
        pn,
        key_id,
    );
    let body_start = ccmp_start + CCMP_HEADER_BYTES;
    let body_end = body_start + payload.len();
    output[body_start..body_end].copy_from_slice(payload);
    let tag_end = body_end + CCMP_MIC_BYTES;
    let cipher = Aes128::new_from_slice(key).map_err(|_| CcmpError::AesInitialization)?;
    let (body_output, tag_output) = output[body_start..tag_end].split_at_mut(payload.len());
    ccm_encrypt_in_place(
        &cipher,
        &context.nonce,
        &context.aad[2..context.aad_len],
        body_output,
        tag_output,
    );
    Ok(output_len)
}

/// Authenticate and decrypt a CCMP-protected data frame.
///
/// The returned frame excludes the eight-byte CCMP header and eight-byte MIC,
/// matching mac80211's software decryption transform. The Protected bit is
/// preserved for the caller's 802.11 receive policy. `replay` is unchanged on
/// malformed input, replay, or failed authentication; output plaintext is
/// erased on authentication failure.
pub fn decrypt_frame(
    key: &[u8; 16],
    expected_key_id: u8,
    replay: &mut CcmpReplayWindow,
    frame: &[u8],
    output: &mut [u8],
) -> Result<usize, CcmpError> {
    if expected_key_id > 3 {
        return Err(CcmpError::InvalidKeyId);
    }
    if frame.len() > MAX_CCMP_FRAME_BYTES + CCMP_HEADER_BYTES + CCMP_MIC_BYTES {
        return Err(CcmpError::FrameTooLarge);
    }
    let view = inspect_frame(frame).map_err(CcmpError::InvalidFrame)?;
    if view.kind != FrameKind::Data {
        return Err(CcmpError::NotDataFrame);
    }
    if view.frame_control & FC_PROTECTED == 0 {
        return Err(CcmpError::InvalidCcmpHeader);
    }
    let header_len = view.header.len();
    let minimum_len = header_len + CCMP_HEADER_BYTES + CCMP_MIC_BYTES;
    if frame.len() < minimum_len {
        return Err(CcmpError::InvalidCcmpHeader);
    }
    let ccmp = &frame[header_len..header_len + CCMP_HEADER_BYTES];
    let (pn, key_id) = parse_ccmp_header(ccmp)?;
    if key_id != expected_key_id {
        return Err(CcmpError::UnexpectedKeyId);
    }
    if pn <= replay.last_authenticated {
        return Err(CcmpError::Replay);
    }

    let plaintext_len = frame.len() - header_len - CCMP_HEADER_BYTES - CCMP_MIC_BYTES;
    if plaintext_len > u16::MAX as usize {
        return Err(CcmpError::PayloadTooLong);
    }
    let output_len = header_len + plaintext_len;
    if output.len() < output_len {
        return Err(CcmpError::OutputTooSmall);
    }

    let context = FrameCryptoContext::new(view.header, view.frame_control, pn)?;
    output[..header_len].copy_from_slice(view.header);
    let cipher = Aes128::new_from_slice(key).map_err(|_| CcmpError::AesInitialization)?;
    let body_start = header_len + CCMP_HEADER_BYTES;
    let body_end = body_start + plaintext_len;
    ccm_decrypt_in_place(
        &cipher,
        &context.nonce,
        &frame[body_start..body_end],
        &mut output[header_len..output_len],
    );

    // Recompute the CCM tag over the authenticated plaintext. Decryption above
    // intentionally stages plaintext in caller memory; erase it before return
    // if the supplied MIC does not match.
    let mut expected_tag = ccm_tag(
        &cipher,
        &context.nonce,
        &context.aad[2..context.aad_len],
        &output[header_len..output_len],
    );
    let tag_ok = bool::from(expected_tag.ct_eq(&frame[body_end..body_end + CCMP_MIC_BYTES]));
    expected_tag.zeroize();
    if !tag_ok {
        output[..output_len].zeroize();
        return Err(CcmpError::AuthenticationFailed);
    }
    replay.last_authenticated = pn;
    Ok(output_len)
}

#[derive(Clone, Copy)]
struct FrameCryptoContext {
    aad: [u8; 34],
    aad_len: usize,
    nonce: [u8; CCM_NONCE_BYTES],
}

impl FrameCryptoContext {
    fn new(header: &[u8], frame_control: u16, pn: u64) -> Result<Self, CcmpError> {
        if pn == 0 || pn > MAX_PACKET_NUMBER {
            return Err(CcmpError::InvalidPacketNumber);
        }
        let mut aad = [0; 34];
        let mut cursor = 2;
        let qos = frame_control & FC_QOS_SUBTYPE != 0;
        let four_address = frame_control & (FC_TO_DS | FC_FROM_DS) == (FC_TO_DS | FC_FROM_DS);

        let mut masked_fc = frame_control;
        masked_fc &= !(FC_RETRY | FC_POWER_MGMT | FC_MORE_DATA | FC_SUBTYPE_BITS_4_TO_6);
        masked_fc |= FC_PROTECTED;
        if qos {
            masked_fc &= !FC_ORDER;
        }
        aad[cursor..cursor + 2].copy_from_slice(&masked_fc.to_le_bytes());
        cursor += 2;
        aad[cursor..cursor + 18].copy_from_slice(&header[4..22]);
        cursor += 18;
        aad[cursor] = header[22] & 0x0f;
        aad[cursor + 1] = 0;
        cursor += 2;

        if four_address {
            aad[cursor..cursor + 6].copy_from_slice(&header[24..30]);
            cursor += 6;
        }

        let mut tid = 0;
        if qos {
            let qos_offset = 24 + if four_address { 6 } else { 0 };
            tid = *header.get(qos_offset).ok_or(CcmpError::InvalidCcmpHeader)? & 0x0f;
            aad[cursor] = tid;
            aad[cursor + 1] = 0;
            cursor += 2;
        }
        let aad_bytes = cursor - 2;
        aad[..2].copy_from_slice(&(aad_bytes as u16).to_be_bytes());

        let pn_bytes = packet_number_bytes(pn);
        let mut nonce = [0; CCM_NONCE_BYTES];
        nonce[0] = tid;
        nonce[1..7].copy_from_slice(&header[10..16]);
        nonce[7..13].copy_from_slice(&pn_bytes);
        Ok(Self {
            aad,
            aad_len: cursor,
            nonce,
        })
    }
}

fn set_protected_bit(header: &mut [u8]) {
    let fc = u16::from_le_bytes([header[0], header[1]]) | FC_PROTECTED;
    header[..2].copy_from_slice(&fc.to_le_bytes());
}

fn packet_number_bytes(packet_number: u64) -> [u8; 6] {
    [
        (packet_number >> 40) as u8,
        (packet_number >> 32) as u8,
        (packet_number >> 24) as u8,
        (packet_number >> 16) as u8,
        (packet_number >> 8) as u8,
        packet_number as u8,
    ]
}

fn write_ccmp_header(output: &mut [u8], packet_number: u64, key_id: u8) {
    let pn = packet_number_bytes(packet_number);
    output.copy_from_slice(&[
        pn[5],
        pn[4],
        0,
        CCMP_EXT_IV | (key_id << 6),
        pn[3],
        pn[2],
        pn[1],
        pn[0],
    ]);
}

fn parse_ccmp_header(header: &[u8]) -> Result<(u64, u8), CcmpError> {
    if header.len() != CCMP_HEADER_BYTES || header[2] != 0 || header[3] & 0x3f != CCMP_EXT_IV {
        return Err(CcmpError::InvalidCcmpHeader);
    }
    let key_id = header[3] >> 6;
    let pn = u64::from(header[0])
        | (u64::from(header[1]) << 8)
        | (u64::from(header[4]) << 16)
        | (u64::from(header[5]) << 24)
        | (u64::from(header[6]) << 32)
        | (u64::from(header[7]) << 40);
    if pn == 0 {
        return Err(CcmpError::InvalidPacketNumber);
    }
    Ok((pn, key_id))
}

fn ccm_encrypt_in_place(
    cipher: &Aes128,
    nonce: &[u8; CCM_NONCE_BYTES],
    aad: &[u8],
    plaintext_ciphertext: &mut [u8],
    tag: &mut [u8],
) {
    let mut calculated_tag = ccm_tag(cipher, nonce, aad, plaintext_ciphertext);
    ccm_crypt_in_place(cipher, nonce, plaintext_ciphertext);
    tag.copy_from_slice(&calculated_tag);
    calculated_tag.zeroize();
}

fn ccm_decrypt_in_place(
    cipher: &Aes128,
    nonce: &[u8; CCM_NONCE_BYTES],
    ciphertext: &[u8],
    plaintext: &mut [u8],
) {
    plaintext.copy_from_slice(ciphertext);
    ccm_crypt_in_place(cipher, nonce, plaintext);
}

fn ccm_tag(
    cipher: &Aes128,
    nonce: &[u8; CCM_NONCE_BYTES],
    aad: &[u8],
    plaintext: &[u8],
) -> [u8; CCM_TAG_BYTES] {
    let mut state = [0u8; 16];
    let mut block = [0u8; 16];
    let aad_flag = if aad.is_empty() { 0 } else { 0x40 };
    let flags = aad_flag | (((CCM_TAG_BYTES - 2) / 2) as u8) << 3 | (CCM_LENGTH_BYTES as u8 - 1);
    block[0] = flags;
    block[1..1 + CCM_NONCE_BYTES].copy_from_slice(nonce);
    block[14..16].copy_from_slice(&(plaintext.len() as u16).to_be_bytes());
    aes_encrypt_block(cipher, &mut block);
    state.copy_from_slice(&block);

    if !aad.is_empty() {
        let mut encoded = [0u8; 2 + 34];
        encoded[..2].copy_from_slice(&(aad.len() as u16).to_be_bytes());
        encoded[2..2 + aad.len()].copy_from_slice(aad);
        for chunk in encoded[..2 + aad.len()].chunks(16) {
            mac_chunk(cipher, &mut state, chunk);
        }
        encoded.zeroize();
    }

    for chunk in plaintext.chunks(16) {
        mac_chunk(cipher, &mut state, chunk);
    }

    let mut counter_zero = [0u8; 16];
    counter_zero[0] = CCM_LENGTH_BYTES as u8 - 1;
    counter_zero[1..1 + CCM_NONCE_BYTES].copy_from_slice(nonce);
    aes_encrypt_block(cipher, &mut counter_zero);
    let mut tag = [0u8; CCM_TAG_BYTES];
    for index in 0..CCM_TAG_BYTES {
        tag[index] = state[index] ^ counter_zero[index];
    }
    state.zeroize();
    block.zeroize();
    counter_zero.zeroize();
    tag
}

fn mac_chunk(cipher: &Aes128, state: &mut [u8; 16], bytes: &[u8]) {
    let mut block = [0u8; 16];
    block[..bytes.len()].copy_from_slice(bytes);
    for index in 0..16 {
        block[index] ^= state[index];
    }
    aes_encrypt_block(cipher, &mut block);
    state.copy_from_slice(&block);
    block.zeroize();
}

fn ccm_crypt_in_place(cipher: &Aes128, nonce: &[u8; CCM_NONCE_BYTES], bytes: &mut [u8]) {
    for (index, chunk) in bytes.chunks_mut(16).enumerate() {
        let mut counter = [0u8; 16];
        counter[0] = CCM_LENGTH_BYTES as u8 - 1;
        counter[1..1 + CCM_NONCE_BYTES].copy_from_slice(nonce);
        let counter_value = (index as u16) + 1;
        counter[14..16].copy_from_slice(&counter_value.to_be_bytes());
        aes_encrypt_block(cipher, &mut counter);
        for (byte, key_byte) in chunk.iter_mut().zip(counter.iter()) {
            *byte ^= *key_byte;
        }
        counter.zeroize();
    }
}

fn aes_encrypt_block(cipher: &Aes128, bytes: &mut [u8; 16]) {
    let mut block = Block::default();
    block.copy_from_slice(bytes);
    cipher.encrypt_block(&mut block);
    bytes.copy_from_slice(&block);
    block.as_mut_slice().zeroize();
}

#[cfg(test)]
mod tests {
    use super::{
        CCMP_HEADER_BYTES, CCMP_MIC_BYTES, CcmpError, CcmpReplayWindow, CcmpTxPacketNumber,
        FrameCryptoContext, MAX_PACKET_NUMBER, ccm_encrypt_in_place, decrypt_frame, encrypt_frame,
    };
    use aes::Aes128;
    use aes::cipher::KeyInit;

    const KEY: [u8; 16] = [
        0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce,
        0xcf,
    ];

    fn rfc3610_vector_one() -> (Vec<u8>, Vec<u8>) {
        let nonce = [
            0x00, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
        ];
        let aad = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        let plaintext = [
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15,
            0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        ];
        let expected = [
            0x58, 0x8c, 0x97, 0x9a, 0x61, 0xc6, 0x63, 0xd2, 0xf0, 0x66, 0xd0, 0xc2, 0xc0, 0xf9,
            0x89, 0x80, 0x6d, 0x5f, 0x6b, 0x61, 0xda, 0xc3, 0x84, 0x17, 0xe8, 0xd1, 0x2c, 0xfd,
            0xf9, 0x26, 0xe0,
        ];
        (
            plaintext.to_vec(),
            [nonce.as_slice(), aad.as_slice(), expected.as_slice()].concat(),
        )
    }

    fn data_frame(qos: bool) -> Vec<u8> {
        let fc = if qos { 0x0288u16 } else { 0x0208u16 };
        let mut frame = vec![0u8; if qos { 26 } else { 24 }];
        frame[..2].copy_from_slice(&fc.to_le_bytes());
        frame[2..4].copy_from_slice(&0x1234u16.to_le_bytes());
        frame[4..10].copy_from_slice(&[0x02, 0, 0, 0, 0, 1]);
        frame[10..16].copy_from_slice(&[0x02, 0, 0, 0, 0, 2]);
        frame[16..22].copy_from_slice(&[0x02, 0, 0, 0, 0, 3]);
        frame[22..24].copy_from_slice(&0xabcdu16.to_le_bytes());
        if qos {
            frame[24] = 7;
            frame[25] = 0xa5;
        }
        frame.extend_from_slice(b"bounded test payload");
        frame
    }

    #[test]
    fn matches_rfc_3610_ccm_vector_one() {
        let (mut ciphertext, metadata) = rfc3610_vector_one();
        let nonce: [u8; 13] = metadata[..13].try_into().unwrap();
        let aad = &metadata[13..21];
        let expected = &metadata[21..];
        let cipher = Aes128::new_from_slice(&KEY).unwrap();
        let mut tag = [0u8; 8];
        ccm_encrypt_in_place(&cipher, &nonce, aad, &mut ciphertext, &mut tag);
        assert_eq!([ciphertext, tag.to_vec()].concat(), expected);
    }

    #[test]
    fn frame_round_trip_preserves_header_and_encodes_pn_keyid_and_tid() {
        for qos in [false, true] {
            let frame = data_frame(qos);
            let mut encrypted = vec![0; frame.len() + CCMP_HEADER_BYTES + CCMP_MIC_BYTES];
            let mut tx_pn = CcmpTxPacketNumber::new(0).unwrap();
            let encrypted_len =
                encrypt_frame(&[0x42; 16], 2, &mut tx_pn, &frame, &mut encrypted).unwrap();
            assert_eq!(tx_pn.last_used(), 1);
            assert_eq!(encrypted[1] & 0x40, 0x40);
            let header_len = if qos { 26 } else { 24 };
            assert_eq!(
                &encrypted[header_len..header_len + 8],
                &[1, 0, 0, 0xa0, 0, 0, 0, 0]
            );

            let mut replay = CcmpReplayWindow::new(0).unwrap();
            let mut plaintext = vec![0; frame.len()];
            let plaintext_len = decrypt_frame(
                &[0x42; 16],
                2,
                &mut replay,
                &encrypted[..encrypted_len],
                &mut plaintext,
            )
            .unwrap();
            assert_eq!(plaintext_len, frame.len());
            let mut expected = frame;
            let fc = u16::from_le_bytes([expected[0], expected[1]]) | 0x4000;
            expected[..2].copy_from_slice(&fc.to_le_bytes());
            assert_eq!(&plaintext[..plaintext_len], expected);
            assert_eq!(replay.last_authenticated(), 1);
        }
    }

    #[test]
    fn failed_mic_erases_staged_plaintext_and_does_not_advance_replay_window() {
        let frame = data_frame(true);
        let mut encrypted = vec![0; frame.len() + 16];
        let mut tx_pn = CcmpTxPacketNumber::new(0).unwrap();
        let encrypted_len =
            encrypt_frame(&[0x42; 16], 1, &mut tx_pn, &frame, &mut encrypted).unwrap();
        encrypted[encrypted_len - 1] ^= 1;
        let mut replay = CcmpReplayWindow::new(0).unwrap();
        let mut plaintext = vec![0xaa; frame.len()];
        assert_eq!(
            decrypt_frame(&[0x42; 16], 1, &mut replay, &encrypted, &mut plaintext),
            Err(CcmpError::AuthenticationFailed)
        );
        assert!(plaintext.iter().all(|byte| *byte == 0));
        assert_eq!(replay.last_authenticated(), 0);
    }

    #[test]
    fn rejects_replay_and_bad_key_id_without_committing_pn() {
        let frame = data_frame(false);
        let mut encrypted = vec![0; frame.len() + 16];
        let mut tx_pn = CcmpTxPacketNumber::new(0).unwrap();
        let len = encrypt_frame(&[0x42; 16], 1, &mut tx_pn, &frame, &mut encrypted).unwrap();
        let mut replay = CcmpReplayWindow::new(0).unwrap();
        let mut plaintext = vec![0; frame.len()];
        decrypt_frame(
            &[0x42; 16],
            1,
            &mut replay,
            &encrypted[..len],
            &mut plaintext,
        )
        .unwrap();
        assert_eq!(
            decrypt_frame(
                &[0x42; 16],
                1,
                &mut replay,
                &encrypted[..len],
                &mut plaintext
            ),
            Err(CcmpError::Replay)
        );
        let mut another_window = CcmpReplayWindow::new(0).unwrap();
        assert_eq!(
            decrypt_frame(
                &[0x42; 16],
                2,
                &mut another_window,
                &encrypted[..len],
                &mut plaintext
            ),
            Err(CcmpError::UnexpectedKeyId)
        );
    }

    #[test]
    fn qos_four_address_aad_matches_linux_field_order_and_masks() {
        let mut frame = vec![0u8; 32];
        let fc = 0x0388u16 | 0x0800 | 0x1000 | 0x2000 | 0x8000;
        frame[..2].copy_from_slice(&fc.to_le_bytes());
        frame[4..10].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        frame[10..16].copy_from_slice(&[7, 8, 9, 10, 11, 12]);
        frame[16..22].copy_from_slice(&[13, 14, 15, 16, 17, 18]);
        frame[22..24].copy_from_slice(&0xabcdu16.to_le_bytes());
        frame[24..30].copy_from_slice(&[19, 20, 21, 22, 23, 24]);
        frame[30] = 0x0b;
        let context = FrameCryptoContext::new(&frame, fc, 1).unwrap();
        assert_eq!(&context.aad[..2], &[0, 30]);
        assert_eq!(&context.aad[2..4], &[0x88, 0x43]);
        assert_eq!(&context.aad[4..22], &frame[4..22]);
        assert_eq!(&context.aad[22..24], &[0x0d, 0]);
        assert_eq!(&context.aad[24..30], &frame[24..30]);
        assert_eq!(&context.aad[30..32], &[0x0b, 0]);
        assert_eq!(context.nonce[0], 0x0b);
        assert_eq!(&context.nonce[1..7], &frame[10..16]);
        assert_eq!(&context.nonce[7..], &[0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn packet_number_exhaustion_never_reuses_nonce() {
        let mut tx = CcmpTxPacketNumber::new(MAX_PACKET_NUMBER).unwrap();
        assert_eq!(tx.next(), Err(CcmpError::PacketNumberExhausted));
        assert_eq!(tx.last_used(), MAX_PACKET_NUMBER);
        assert_eq!(
            CcmpTxPacketNumber::new(MAX_PACKET_NUMBER + 1),
            Err(CcmpError::InvalidPacketNumber)
        );
    }
}
