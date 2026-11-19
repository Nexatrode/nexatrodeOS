//! Bounded WPA2 EAPOL-Key parsing, message-2/message-4 encoding, and message-3 checks.
//!
//! This module covers only WPA2-Personal with the RSN EAPOL-Key descriptor
//! version 2 and CCMP-128. It provides a MIC-gated replay window but does not
//! run retransmission timers, install pairwise/group keys, or authorize
//! protected data traffic.
#![forbid(unsafe_code)]

use aes::cipher::{BlockCipherDecrypt, KeyInit};
use aes::{Aes128, Block};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

const EAPOL_VERSION: u8 = 2;
const EAPOL_TYPE_KEY: u8 = 3;
const RSN_KEY_DESCRIPTOR: u8 = 2;
const DESCRIPTOR_VERSION_HMAC_SHA1_AES: u16 = 2;
const CCMP_128_KEY_BYTES: u16 = 16;

const EAPOL_HEADER_BYTES: usize = 4;
const KEY_FIXED_BYTES: usize = 95;
const MIC_START: usize = 81;
const MIC_BYTES: usize = 16;
const MIC_END: usize = MIC_START + MIC_BYTES;
const KEY_DATA_LENGTH_END: usize = KEY_FIXED_BYTES + EAPOL_HEADER_BYTES;
const EAPOL_FIXED_FRAME_BYTES: usize = EAPOL_HEADER_BYTES + KEY_FIXED_BYTES;
pub const MAX_EAPOL_KEY_FRAME_BYTES: usize = 512;
const MAX_UNWRAPPED_KEY_DATA_BYTES: usize = 400;
const AES_WRAP_IV: [u8; 8] = [0xa6; 8];
const CCMP_128_GROUP_KEY_BYTES: usize = 16;
const RSN_ELEMENT_ID: u8 = 48;
const VENDOR_ELEMENT_ID: u8 = 221;
const RSN_GROUP_KEY_SELECTOR: [u8; 4] = [0x00, 0x0f, 0xac, 0x01];

const KEY_INFO_KEY_INDEX_MASK: u16 = 0x0030;
const KEY_INFO_PAIRWISE: u16 = 1 << 3;
const KEY_INFO_INSTALL: u16 = 1 << 6;
const KEY_INFO_ACK: u16 = 1 << 7;
const KEY_INFO_MIC: u16 = 1 << 8;
const KEY_INFO_SECURE: u16 = 1 << 9;
const KEY_INFO_ERROR: u16 = 1 << 10;
const KEY_INFO_REQUEST: u16 = 1 << 11;
const KEY_INFO_ENCRYPTED_KEY_DATA: u16 = 1 << 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EapolError {
    FrameTooShort,
    FrameTooLong,
    WrongVersion,
    WrongPacketType,
    InvalidFrameLength,
    WrongDescriptor,
    InvalidKeyDataLength,
    InvalidRsnElement,
    OutputTooSmall,
    NotMessage1,
    NotMessage3,
    InvalidReplayCounter,
    MissingMic,
    NonceMismatch,
    InvalidEncryptedKeyData,
    InvalidKeyWrapIntegrity,
    KeyDataUnderflow,
    MissingRsnElement,
    DuplicateRsnElement,
    RsnElementMismatch,
    UnsupportedKeyDataElement,
    DuplicateGroupKey,
    InvalidGroupKey,
    InvalidMic,
    AesInitialization,
    HmacInitialization,
}

/// A borrowed, exact-length EAPOL-Key frame.
pub struct EapolKey<'a> {
    frame: &'a [u8],
    key_info: u16,
    key_length: u16,
    replay_counter: &'a [u8; 8],
    nonce: &'a [u8; 32],
    rsc: &'a [u8; 8],
    mic: &'a [u8; MIC_BYTES],
    key_data: &'a [u8],
}

impl<'a> EapolKey<'a> {
    /// Parse a complete version-2 EAPOL-Key frame. Trailing Ethernet padding
    /// must be removed by the caller before parsing.
    pub fn parse(frame: &'a [u8]) -> Result<Self, EapolError> {
        if frame.len() < EAPOL_FIXED_FRAME_BYTES {
            return Err(EapolError::FrameTooShort);
        }
        if frame.len() > MAX_EAPOL_KEY_FRAME_BYTES {
            return Err(EapolError::FrameTooLong);
        }
        if frame[0] != EAPOL_VERSION {
            return Err(EapolError::WrongVersion);
        }
        if frame[1] != EAPOL_TYPE_KEY {
            return Err(EapolError::WrongPacketType);
        }
        if usize::from(u16::from_be_bytes([frame[2], frame[3]])) != frame.len() - EAPOL_HEADER_BYTES
        {
            return Err(EapolError::InvalidFrameLength);
        }
        if frame[EAPOL_HEADER_BYTES] != RSN_KEY_DESCRIPTOR {
            return Err(EapolError::WrongDescriptor);
        }

        let key_info = u16::from_be_bytes([frame[5], frame[6]]);
        if key_info & 0x0007 != DESCRIPTOR_VERSION_HMAC_SHA1_AES {
            return Err(EapolError::WrongDescriptor);
        }
        let key_length = u16::from_be_bytes([frame[7], frame[8]]);
        let replay_counter = frame[9..17]
            .try_into()
            .map_err(|_| EapolError::FrameTooShort)?;
        let nonce = frame[17..49]
            .try_into()
            .map_err(|_| EapolError::FrameTooShort)?;
        let rsc = frame[65..73]
            .try_into()
            .map_err(|_| EapolError::FrameTooShort)?;
        let mic = frame[MIC_START..MIC_END]
            .try_into()
            .map_err(|_| EapolError::FrameTooShort)?;
        let key_data_length = usize::from(u16::from_be_bytes([frame[MIC_END], frame[MIC_END + 1]]));
        let key_data_end = KEY_DATA_LENGTH_END
            .checked_add(key_data_length)
            .ok_or(EapolError::InvalidKeyDataLength)?;
        if key_data_end != frame.len() {
            return Err(EapolError::InvalidKeyDataLength);
        }

        Ok(Self {
            frame,
            key_info,
            key_length,
            replay_counter,
            nonce,
            rsc,
            mic,
            key_data: &frame[KEY_DATA_LENGTH_END..key_data_end],
        })
    }

    /// Validate the WPA2 four-way-handshake message 1 key-info shape.
    pub fn validate_message1(&self) -> Result<(), EapolError> {
        let required = DESCRIPTOR_VERSION_HMAC_SHA1_AES | KEY_INFO_PAIRWISE | KEY_INFO_ACK;
        let forbidden = KEY_INFO_KEY_INDEX_MASK
            | KEY_INFO_INSTALL
            | KEY_INFO_MIC
            | KEY_INFO_SECURE
            | KEY_INFO_ERROR
            | KEY_INFO_REQUEST
            | KEY_INFO_ENCRYPTED_KEY_DATA;
        if self.key_info & !forbidden != required || self.key_info & forbidden != 0 {
            return Err(EapolError::NotMessage1);
        }
        if self.key_length != CCMP_128_KEY_BYTES || self.nonce.iter().all(|byte| *byte == 0) {
            return Err(EapolError::NotMessage1);
        }
        if !self.key_data.is_empty() {
            return Err(EapolError::NotMessage1);
        }
        Ok(())
    }

    /// Verify message 3 against message 1, including its increasing replay
    /// counter, ANonce, required flags, AES-wrap input length, and MIC.
    pub fn verify_message3<'m1>(
        &self,
        message1: &EapolKey<'m1>,
        kck: &[u8; MIC_BYTES],
        replay_window: &mut ReplayWindow,
    ) -> Result<VerifiedMessage3<'a>, EapolError> {
        message1.validate_message1()?;
        let required = DESCRIPTOR_VERSION_HMAC_SHA1_AES
            | KEY_INFO_PAIRWISE
            | KEY_INFO_INSTALL
            | KEY_INFO_ACK
            | KEY_INFO_MIC
            | KEY_INFO_ENCRYPTED_KEY_DATA;
        let forbidden = KEY_INFO_KEY_INDEX_MASK | KEY_INFO_ERROR | KEY_INFO_REQUEST;
        if self.key_info & !forbidden & !KEY_INFO_SECURE != required {
            return Err(EapolError::NotMessage3);
        }
        if self.key_info & forbidden != 0 || self.key_length != CCMP_128_KEY_BYTES {
            return Err(EapolError::NotMessage3);
        }
        if self.nonce != message1.nonce {
            return Err(EapolError::NonceMismatch);
        }
        if self.replay_counter <= message1.replay_counter {
            return Err(EapolError::InvalidReplayCounter);
        }
        if self.key_data.len() < 16 || self.key_data.len() % 8 != 0 {
            return Err(EapolError::InvalidEncryptedKeyData);
        }
        replay_window.verify_mic_and_advance(self, kck)?;
        Ok(VerifiedMessage3 {
            replay_counter: self.replay_counter,
            rsc: self.rsc,
            secure: self.key_info & KEY_INFO_SECURE != 0,
            encrypted_key_data: self.key_data,
        })
    }

    /// Check the EAPOL-Key MIC using a constant-time comparison.
    pub fn verify_mic(&self, kck: &[u8; MIC_BYTES]) -> Result<(), EapolError> {
        let expected = calculate_mic(self.frame, kck)?;
        if bool::from(expected.ct_eq(self.mic)) {
            Ok(())
        } else {
            Err(EapolError::InvalidMic)
        }
    }

    pub fn key_info(&self) -> u16 {
        self.key_info
    }

    pub fn replay_counter(&self) -> &[u8; 8] {
        self.replay_counter
    }

    pub fn nonce(&self) -> &[u8; 32] {
        self.nonce
    }

    pub fn rsc(&self) -> &[u8; 8] {
        self.rsc
    }

    pub fn key_data(&self) -> &[u8] {
        self.key_data
    }
}

/// Strict monotonically increasing replay window for authenticated inbound
/// EAPOL-Key frames. The saved counter advances only after MIC verification.
/// Reset this value when beginning a new association.
#[derive(Default)]
pub struct ReplayWindow {
    last_authenticated: Option<[u8; 8]>,
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_authenticated(&self) -> Option<&[u8; 8]> {
        self.last_authenticated.as_ref()
    }

    pub fn clear(&mut self) {
        self.last_authenticated = None;
    }

    pub fn verify_mic_and_advance(
        &mut self,
        frame: &EapolKey<'_>,
        kck: &[u8; MIC_BYTES],
    ) -> Result<(), EapolError> {
        if frame.key_info & KEY_INFO_MIC == 0 {
            return Err(EapolError::MissingMic);
        }
        if self
            .last_authenticated
            .is_some_and(|last| frame.replay_counter <= &last)
        {
            return Err(EapolError::InvalidReplayCounter);
        }
        frame.verify_mic(kck)?;
        self.last_authenticated = Some(*frame.replay_counter);
        Ok(())
    }
}

/// Message-3 fields exposed only after its replay, nonce, and MIC checks pass.
pub struct VerifiedMessage3<'a> {
    replay_counter: &'a [u8; 8],
    rsc: &'a [u8; 8],
    secure: bool,
    encrypted_key_data: &'a [u8],
}

impl<'frame> VerifiedMessage3<'frame> {
    pub fn replay_counter(&self) -> &[u8; 8] {
        self.replay_counter
    }

    pub fn rsc(&self) -> &[u8; 8] {
        self.rsc
    }

    pub fn secure(&self) -> bool {
        self.secure
    }

    /// Still encrypted. It must be AES-unwrapped and validated before key use.
    pub fn encrypted_key_data(&self) -> &[u8] {
        self.encrypted_key_data
    }

    /// AES-unwrap the authenticated message-3 key data into a caller-owned,
    /// bounded buffer. The plaintext buffer is erased when the returned guard
    /// is dropped, including when RSN/KDE validation fails.
    pub fn unwrap_key_data<'out>(
        &'_ self,
        kek: &[u8; 16],
        output: &'out mut [u8],
    ) -> Result<UnwrappedMessage3KeyData<'out, 'frame>, EapolError> {
        let wrapped = self.encrypted_key_data;
        if wrapped.len() < 16 || wrapped.len() % 8 != 0 {
            return Err(EapolError::InvalidEncryptedKeyData);
        }
        let plaintext_len = wrapped
            .len()
            .checked_sub(8)
            .filter(|length| *length <= MAX_UNWRAPPED_KEY_DATA_BYTES)
            .ok_or(EapolError::InvalidEncryptedKeyData)?;
        if output.len() < plaintext_len {
            return Err(EapolError::OutputTooSmall);
        }

        let plaintext = &mut output[..plaintext_len];
        plaintext.copy_from_slice(&wrapped[8..]);
        let mut initial_block = [0; 8];
        initial_block.copy_from_slice(&wrapped[..8]);
        if let Err(error) = aes128_key_unwrap_in_place(kek, &mut initial_block, plaintext) {
            plaintext.zeroize();
            initial_block.zeroize();
            return Err(error);
        }
        initial_block.zeroize();
        Ok(UnwrappedMessage3KeyData {
            plaintext,
            plaintext_len,
            replay_counter: self.replay_counter,
            rsc: self.rsc,
            secure: self.secure,
        })
    }
}

/// Decrypted message-3 key data. The plaintext is erased on drop and must be
/// validated before any key is installed or used.
pub struct UnwrappedMessage3KeyData<'out, 'frame> {
    plaintext: &'out mut [u8],
    plaintext_len: usize,
    replay_counter: &'frame [u8; 8],
    rsc: &'frame [u8; 8],
    secure: bool,
}

impl<'out, 'frame> UnwrappedMessage3KeyData<'out, 'frame> {
    /// Require an exact match for the negotiated RSN IE and extract an
    /// optional CCMP-128 GTK. Unsupported key-data elements fail closed.
    pub fn validate_rsn_key_data(
        self,
        expected_rsn_ie: &[u8],
    ) -> Result<ValidatedMessage3KeyData<'frame>, EapolError> {
        if expected_rsn_ie.len() < 2
            || expected_rsn_ie[0] != RSN_ELEMENT_ID
            || usize::from(expected_rsn_ie[1]) + 2 != expected_rsn_ie.len()
        {
            return Err(EapolError::InvalidRsnElement);
        }

        let group_key = parse_rsn_key_data(&self.plaintext[..self.plaintext_len], expected_rsn_ie)?;
        Ok(ValidatedMessage3KeyData {
            replay_counter: self.replay_counter,
            rsc: self.rsc,
            secure: self.secure,
            group_key,
        })
    }
}

impl Drop for UnwrappedMessage3KeyData<'_, '_> {
    fn drop(&mut self) {
        self.plaintext[..self.plaintext_len].zeroize();
    }
}

/// Message-3 metadata and group key available only after MIC, AES-wrap,
/// negotiated-RSN, and key-data validation have all succeeded.
pub struct ValidatedMessage3KeyData<'a> {
    replay_counter: &'a [u8; 8],
    rsc: &'a [u8; 8],
    secure: bool,
    group_key: Option<GroupKey>,
}

impl ValidatedMessage3KeyData<'_> {
    pub fn replay_counter(&self) -> &[u8; 8] {
        self.replay_counter
    }

    pub fn rsc(&self) -> &[u8; 8] {
        self.rsc
    }

    pub fn secure(&self) -> bool {
        self.secure
    }

    pub fn group_key(&self) -> Option<&GroupKey> {
        self.group_key.as_ref()
    }

    /// Consume the validated M3 result into owned scalar metadata and its
    /// zeroizing GTK guard.
    pub fn into_parts(self) -> ([u8; 8], [u8; 8], bool, Option<GroupKey>) {
        (*self.replay_counter, *self.rsc, self.secure, self.group_key)
    }
}

/// CCMP-128 group temporal key extracted from a validated GTK KDE. The key is
/// erased on drop and is not installed by this module.
pub struct GroupKey {
    key_id: u8,
    transmit: bool,
    key: [u8; CCMP_128_GROUP_KEY_BYTES],
}

impl GroupKey {
    pub fn key_id(&self) -> u8 {
        self.key_id
    }

    pub fn transmit(&self) -> bool {
        self.transmit
    }

    pub fn temporal_key(&self) -> &[u8; CCMP_128_GROUP_KEY_BYTES] {
        &self.key
    }
}

impl Drop for GroupKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

fn parse_rsn_key_data(
    plaintext: &[u8],
    expected_rsn_ie: &[u8],
) -> Result<Option<GroupKey>, EapolError> {
    let mut offset = 0usize;
    let mut rsn_found = false;
    let mut group_key = None;

    while offset < plaintext.len() {
        let remaining = plaintext.len() - offset;
        if plaintext[offset] == VENDOR_ELEMENT_ID
            && (remaining == 1 || (remaining >= 2 && plaintext[offset + 1] == 0))
        {
            let padding_start = if remaining == 1 {
                offset + 1
            } else {
                offset + 2
            };
            if plaintext[padding_start..].iter().any(|byte| *byte != 0) {
                return Err(EapolError::UnsupportedKeyDataElement);
            }
            break;
        }
        if remaining < 2 {
            return Err(EapolError::KeyDataUnderflow);
        }
        let element_len = usize::from(plaintext[offset + 1]);
        let element_end = offset
            .checked_add(2)
            .and_then(|start| start.checked_add(element_len))
            .ok_or(EapolError::KeyDataUnderflow)?;
        if element_end > plaintext.len() {
            return Err(EapolError::KeyDataUnderflow);
        }
        let element = &plaintext[offset..element_end];
        let body = &element[2..];

        match element[0] {
            RSN_ELEMENT_ID => {
                if rsn_found {
                    return Err(EapolError::DuplicateRsnElement);
                }
                if element != expected_rsn_ie {
                    return Err(EapolError::RsnElementMismatch);
                }
                rsn_found = true;
            }
            VENDOR_ELEMENT_ID => {
                if body.len() < RSN_GROUP_KEY_SELECTOR.len()
                    || body[..RSN_GROUP_KEY_SELECTOR.len()] != RSN_GROUP_KEY_SELECTOR
                {
                    return Err(EapolError::UnsupportedKeyDataElement);
                }
                if group_key.is_some() {
                    return Err(EapolError::DuplicateGroupKey);
                }
                let gtk = &body[RSN_GROUP_KEY_SELECTOR.len()..];
                if gtk.len() != 2 + CCMP_128_GROUP_KEY_BYTES || gtk[0] & !0x07 != 0 || gtk[1] != 0 {
                    return Err(EapolError::InvalidGroupKey);
                }
                let mut key = [0; CCMP_128_GROUP_KEY_BYTES];
                key.copy_from_slice(&gtk[2..]);
                group_key = Some(GroupKey {
                    key_id: gtk[0] & 0x03,
                    transmit: gtk[0] & 0x04 != 0,
                    key,
                });
            }
            _ => return Err(EapolError::UnsupportedKeyDataElement),
        }
        offset = element_end;
    }

    if !rsn_found {
        return Err(EapolError::MissingRsnElement);
    }
    Ok(group_key)
}

fn aes128_key_unwrap_in_place(
    kek: &[u8; 16],
    initial_block: &mut [u8; 8],
    plaintext: &mut [u8],
) -> Result<(), EapolError> {
    if plaintext.len() < 8 || plaintext.len() % 8 != 0 {
        return Err(EapolError::InvalidEncryptedKeyData);
    }
    let blocks = plaintext.len() / 8;
    let cipher = Aes128::new_from_slice(kek).map_err(|_| EapolError::AesInitialization)?;
    let mut block = Block::default();

    for round in (0..6).rev() {
        for index in (1..=blocks).rev() {
            let t = (blocks * round + index) as u64;
            let a_xor_t = u64::from_be_bytes(*initial_block) ^ t;
            block[..8].copy_from_slice(&a_xor_t.to_be_bytes());
            let start = (index - 1) * 8;
            block[8..].copy_from_slice(&plaintext[start..start + 8]);
            cipher.decrypt_block(&mut block);
            initial_block.copy_from_slice(&block[..8]);
            plaintext[start..start + 8].copy_from_slice(&block[8..]);
        }
    }

    let integrity_ok = bool::from(initial_block.ct_eq(&AES_WRAP_IV));
    block.as_mut_slice().zeroize();
    initial_block.zeroize();
    if integrity_ok {
        Ok(())
    } else {
        plaintext.zeroize();
        Err(EapolError::InvalidKeyWrapIntegrity)
    }
}

/// Encode an EAPOL-Key message 2 using the supplicant's association RSN IE.
/// The output remains unchanged if validation or capacity checks fail.
pub fn encode_message2(
    replay_counter: &[u8; 8],
    snonce: &[u8; 32],
    rsn_ie: &[u8],
    kck: &[u8; MIC_BYTES],
    output: &mut [u8],
) -> Result<usize, EapolError> {
    if rsn_ie.len() < 2 || rsn_ie[0] != 48 || usize::from(rsn_ie[1]) + 2 != rsn_ie.len() {
        return Err(EapolError::InvalidRsnElement);
    }
    if snonce.iter().all(|byte| *byte == 0) {
        return Err(EapolError::NotMessage1);
    }
    let frame_bytes = EAPOL_FIXED_FRAME_BYTES
        .checked_add(rsn_ie.len())
        .ok_or(EapolError::InvalidKeyDataLength)?;
    if frame_bytes > MAX_EAPOL_KEY_FRAME_BYTES {
        return Err(EapolError::FrameTooLong);
    }
    if output.len() < frame_bytes {
        return Err(EapolError::OutputTooSmall);
    }

    write_eapol_key_header(
        output,
        frame_bytes,
        DESCRIPTOR_VERSION_HMAC_SHA1_AES | KEY_INFO_PAIRWISE | KEY_INFO_MIC,
        replay_counter,
        snonce,
        rsn_ie,
    );
    write_mic(&mut output[..frame_bytes], kck)?;
    Ok(frame_bytes)
}

/// Encode EAPOL-Key message 4 from validated message-3 data.
pub fn encode_message4(
    message3: &ValidatedMessage3KeyData<'_>,
    kck: &[u8; MIC_BYTES],
    output: &mut [u8],
) -> Result<usize, EapolError> {
    if output.len() < EAPOL_FIXED_FRAME_BYTES {
        return Err(EapolError::OutputTooSmall);
    }
    encode_message4_fields(message3.replay_counter, message3.secure, kck, output)
}

/// Encode message 4 from fields that were validated and retained by a
/// handshake controller. Key installation and port authorization remain the
/// controller's responsibility.
pub(crate) fn encode_message4_fields(
    replay_counter: &[u8; 8],
    secure: bool,
    kck: &[u8; MIC_BYTES],
    output: &mut [u8],
) -> Result<usize, EapolError> {
    if output.len() < EAPOL_FIXED_FRAME_BYTES {
        return Err(EapolError::OutputTooSmall);
    }
    write_eapol_key_header(
        output,
        EAPOL_FIXED_FRAME_BYTES,
        DESCRIPTOR_VERSION_HMAC_SHA1_AES
            | KEY_INFO_PAIRWISE
            | KEY_INFO_MIC
            | if secure { KEY_INFO_SECURE } else { 0 },
        replay_counter,
        &[0; 32],
        &[],
    );
    write_mic(&mut output[..EAPOL_FIXED_FRAME_BYTES], kck)?;
    Ok(EAPOL_FIXED_FRAME_BYTES)
}

fn write_eapol_key_header(
    output: &mut [u8],
    frame_bytes: usize,
    key_info: u16,
    replay_counter: &[u8; 8],
    nonce: &[u8; 32],
    key_data: &[u8],
) {
    output[..frame_bytes].fill(0);
    output[0] = EAPOL_VERSION;
    output[1] = EAPOL_TYPE_KEY;
    output[2..4].copy_from_slice(&((frame_bytes - EAPOL_HEADER_BYTES) as u16).to_be_bytes());
    output[4] = RSN_KEY_DESCRIPTOR;
    output[5..7].copy_from_slice(&key_info.to_be_bytes());
    output[7..9].copy_from_slice(&0u16.to_be_bytes()); // RSN key length.
    output[9..17].copy_from_slice(replay_counter);
    output[17..49].copy_from_slice(nonce);
    output[MIC_END..MIC_END + 2].copy_from_slice(&(key_data.len() as u16).to_be_bytes());
    output[KEY_DATA_LENGTH_END..KEY_DATA_LENGTH_END + key_data.len()].copy_from_slice(key_data);
}

fn write_mic(frame: &mut [u8], kck: &[u8; MIC_BYTES]) -> Result<(), EapolError> {
    let mic = calculate_mic(frame, kck)?;
    frame[MIC_START..MIC_END].copy_from_slice(&mic);
    Ok(())
}

fn calculate_mic(frame: &[u8], kck: &[u8; MIC_BYTES]) -> Result<[u8; MIC_BYTES], EapolError> {
    EapolKey::parse(frame)?;
    let mut mac = Hmac::<Sha1>::new_from_slice(kck).map_err(|_| EapolError::HmacInitialization)?;
    mac.update(&frame[..MIC_START]);
    mac.update(&[0; MIC_BYTES]);
    mac.update(&frame[MIC_END..]);
    let digest = mac.finalize().into_bytes();
    let mut mic = [0u8; MIC_BYTES];
    mic.copy_from_slice(&digest[..MIC_BYTES]);
    Ok(mic)
}

#[cfg(test)]
mod tests {
    use super::{
        DESCRIPTOR_VERSION_HMAC_SHA1_AES, EAPOL_FIXED_FRAME_BYTES, EapolError, EapolKey,
        KEY_DATA_LENGTH_END, KEY_INFO_ACK, KEY_INFO_ENCRYPTED_KEY_DATA, KEY_INFO_INSTALL,
        KEY_INFO_MIC, KEY_INFO_PAIRWISE, KEY_INFO_SECURE, ReplayWindow, aes128_key_unwrap_in_place,
        encode_message2, encode_message4,
    };
    use aes::cipher::{BlockCipherEncrypt, KeyInit};
    use aes::{Aes128, Block};
    use zeroize::Zeroize;

    const KCK: [u8; 16] = [0x11; 16];
    const REPLAY_1: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const REPLAY_3: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 2];
    const ANONCE: [u8; 32] = [0x22; 32];
    const SNONCE: [u8; 32] = [0x33; 32];
    const RSN_IE: [u8; 8] = [48, 6, 1, 0, 0, 15, 172, 4];
    const KEK: [u8; 16] = [0x44; 16];

    fn wrap_key_data(kek: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
        assert!(plaintext.len() >= 8 && plaintext.len() % 8 == 0);
        let cipher = Aes128::new_from_slice(kek).unwrap();
        let blocks = plaintext.len() / 8;
        let mut wrapped = vec![0; plaintext.len() + 8];
        wrapped[..8].copy_from_slice(&[0xa6; 8]);
        wrapped[8..].copy_from_slice(plaintext);
        let mut block = Block::default();

        for round in 0..6 {
            for index in 1..=blocks {
                let start = 8 + (index - 1) * 8;
                block[..8].copy_from_slice(&wrapped[..8]);
                block[8..].copy_from_slice(&wrapped[start..start + 8]);
                cipher.encrypt_block(&mut block);
                let t = (blocks * round + index) as u64;
                let a = u64::from_be_bytes(block[..8].try_into().unwrap()) ^ t;
                wrapped[..8].copy_from_slice(&a.to_be_bytes());
                wrapped[start..start + 8].copy_from_slice(&block[8..]);
            }
        }
        block.as_mut_slice().zeroize();
        wrapped
    }

    fn message3_with_key_data(key_data: &[u8]) -> Vec<u8> {
        let frame_bytes = EAPOL_FIXED_FRAME_BYTES + key_data.len();
        let mut frame = vec![0; frame_bytes];
        frame[0..2].copy_from_slice(&[2, 3]);
        frame[2..4].copy_from_slice(&((frame_bytes - 4) as u16).to_be_bytes());
        frame[4] = 2;
        frame[5..7].copy_from_slice(
            &(DESCRIPTOR_VERSION_HMAC_SHA1_AES
                | KEY_INFO_PAIRWISE
                | KEY_INFO_INSTALL
                | KEY_INFO_ACK
                | KEY_INFO_MIC
                | KEY_INFO_SECURE
                | KEY_INFO_ENCRYPTED_KEY_DATA)
                .to_be_bytes(),
        );
        frame[7..9].copy_from_slice(&16u16.to_be_bytes());
        frame[9..17].copy_from_slice(&REPLAY_3);
        frame[17..49].copy_from_slice(&ANONCE);
        frame[97..99].copy_from_slice(&(key_data.len() as u16).to_be_bytes());
        frame[99..].copy_from_slice(key_data);
        super::write_mic(&mut frame, &KCK).unwrap();
        frame
    }

    fn message1() -> [u8; EAPOL_FIXED_FRAME_BYTES] {
        let mut frame = [0; EAPOL_FIXED_FRAME_BYTES];
        frame[0] = 2;
        frame[1] = 3;
        frame[2..4].copy_from_slice(&95u16.to_be_bytes());
        frame[4] = 2;
        frame[5..7].copy_from_slice(
            &(DESCRIPTOR_VERSION_HMAC_SHA1_AES | KEY_INFO_PAIRWISE | KEY_INFO_ACK).to_be_bytes(),
        );
        frame[7..9].copy_from_slice(&16u16.to_be_bytes());
        frame[9..17].copy_from_slice(&REPLAY_1);
        frame[17..49].copy_from_slice(&ANONCE);
        frame
    }

    fn message3() -> [u8; 123] {
        let mut frame = [0; 123];
        frame[..4].copy_from_slice(&[2, 3, 0, 119]);
        frame[4] = 2;
        frame[5..7].copy_from_slice(
            &(DESCRIPTOR_VERSION_HMAC_SHA1_AES
                | KEY_INFO_PAIRWISE
                | KEY_INFO_INSTALL
                | KEY_INFO_ACK
                | KEY_INFO_MIC
                | KEY_INFO_SECURE
                | KEY_INFO_ENCRYPTED_KEY_DATA)
                .to_be_bytes(),
        );
        frame[7..9].copy_from_slice(&16u16.to_be_bytes());
        frame[9..17].copy_from_slice(&REPLAY_3);
        frame[17..49].copy_from_slice(&ANONCE);
        frame[KEY_DATA_LENGTH_END..].copy_from_slice(&[0x5a; 24]);
        frame[97..99].copy_from_slice(&24u16.to_be_bytes());
        super::write_mic(&mut frame, &KCK).unwrap();
        frame
    }

    #[test]
    fn parses_message1_and_encodes_message2_with_source_layout_and_mic() {
        let m1_bytes = message1();
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        m1.validate_message1().unwrap();

        let mut output = [0xa5; 120];
        let bytes = encode_message2(&REPLAY_1, &SNONCE, &RSN_IE, &KCK, &mut output).unwrap();
        assert_eq!(bytes, EAPOL_FIXED_FRAME_BYTES + RSN_IE.len());
        assert_eq!(&output[..4], &[2, 3, 0, 103]);
        assert_eq!(&output[4..9], &[2, 1, 10, 0, 0]);
        assert_eq!(&output[9..17], &REPLAY_1);
        assert_eq!(&output[17..49], &SNONCE);
        assert_eq!(&output[97..99], &8u16.to_be_bytes());
        assert_eq!(&output[99..107], &RSN_IE);
        assert_eq!(
            &output[81..97],
            &[
                0xb5, 0xd7, 0x00, 0xe5, 0x16, 0x7c, 0x32, 0x42, 0x08, 0x07, 0x39, 0xbf, 0x54, 0x71,
                0xb3, 0x68,
            ]
        );
        let m2 = EapolKey::parse(&output[..bytes]).unwrap();
        m2.verify_mic(&KCK).unwrap();
        assert!(output[bytes..].iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn verifies_message3_replay_nonce_mic_and_bounded_wrapped_data() {
        let m1_bytes = message1();
        let m3_bytes = message3();
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let m3 = EapolKey::parse(&m3_bytes).unwrap();
        let mut replay_window = ReplayWindow::new();
        let verified = m3.verify_message3(&m1, &KCK, &mut replay_window).unwrap();
        assert_eq!(verified.replay_counter(), &REPLAY_3);
        assert_eq!(verified.encrypted_key_data(), &[0x5a; 24]);

        let mut bad_mic = m3_bytes;
        bad_mic[81] ^= 1;
        let mut bad_mic_window = ReplayWindow::new();
        assert_eq!(
            EapolKey::parse(&bad_mic)
                .unwrap()
                .verify_message3(&m1, &KCK, &mut bad_mic_window)
                .err(),
            Some(EapolError::InvalidMic)
        );

        let mut stale = m3_bytes;
        stale[9..17].copy_from_slice(&REPLAY_1);
        super::write_mic(&mut stale, &KCK).unwrap();
        let mut stale_window = ReplayWindow::new();
        assert_eq!(
            EapolKey::parse(&stale)
                .unwrap()
                .verify_message3(&m1, &KCK, &mut stale_window)
                .err(),
            Some(EapolError::InvalidReplayCounter)
        );
        assert_eq!(replay_window.last_authenticated(), Some(&REPLAY_3));
    }

    #[test]
    fn replay_window_advances_only_after_a_valid_mic_and_rejects_replays() {
        let m1_bytes = message1();
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let first = message3();
        let mut replay_window = ReplayWindow::new();

        EapolKey::parse(&first)
            .unwrap()
            .verify_message3(&m1, &KCK, &mut replay_window)
            .unwrap();
        assert_eq!(replay_window.last_authenticated(), Some(&REPLAY_3));
        assert_eq!(
            EapolKey::parse(&first)
                .unwrap()
                .verify_message3(&m1, &KCK, &mut replay_window)
                .err(),
            Some(EapolError::InvalidReplayCounter)
        );

        let next_replay = [0, 0, 0, 0, 0, 0, 0, 3];
        let mut invalid_mic = message3();
        invalid_mic[9..17].copy_from_slice(&next_replay);
        super::write_mic(&mut invalid_mic, &KCK).unwrap();
        invalid_mic[81] ^= 1;
        assert_eq!(
            EapolKey::parse(&invalid_mic)
                .unwrap()
                .verify_message3(&m1, &KCK, &mut replay_window)
                .err(),
            Some(EapolError::InvalidMic)
        );
        assert_eq!(replay_window.last_authenticated(), Some(&REPLAY_3));

        let mut valid_next = message3();
        valid_next[9..17].copy_from_slice(&next_replay);
        super::write_mic(&mut valid_next, &KCK).unwrap();
        EapolKey::parse(&valid_next)
            .unwrap()
            .verify_message3(&m1, &KCK, &mut replay_window)
            .unwrap();
        assert_eq!(replay_window.last_authenticated(), Some(&next_replay));

        replay_window.clear();
        assert_eq!(replay_window.last_authenticated(), None);
    }

    #[test]
    fn aes_key_unwrap_matches_rfc3394_and_erases_bad_plaintext() {
        let kek = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let mut initial_block = [0x1f, 0xa6, 0x8b, 0x0a, 0x81, 0x12, 0xb4, 0x47];
        let mut plaintext = [
            0xae, 0xf3, 0x4b, 0xd8, 0xfb, 0x5a, 0x7b, 0x82, 0x9d, 0x3e, 0x86, 0x23, 0x71, 0xd2,
            0xcf, 0xe5,
        ];
        aes128_key_unwrap_in_place(&kek, &mut initial_block, &mut plaintext).unwrap();
        assert_eq!(initial_block, [0; 8]);
        assert_eq!(
            plaintext,
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        );

        let mut bad_initial = [0xa7; 8];
        let mut bad_plaintext = [0x55; 16];
        assert_eq!(
            aes128_key_unwrap_in_place(&kek, &mut bad_initial, &mut bad_plaintext),
            Err(EapolError::InvalidKeyWrapIntegrity)
        );
        assert_eq!(bad_plaintext, [0; 16]);
        assert_eq!(bad_initial, [0; 8]);
    }

    #[test]
    fn unwraps_and_validates_message3_rsn_and_ccmp_group_key_then_erases_plaintext() {
        let mut plaintext = Vec::new();
        plaintext.extend_from_slice(&RSN_IE);
        plaintext.extend_from_slice(&[221, 22, 0x00, 0x0f, 0xac, 0x01, 0x06, 0x00]);
        plaintext.extend_from_slice(&[0x77; 16]);
        let wrapped = wrap_key_data(&KEK, &plaintext);
        let m1_bytes = message1();
        let m3_bytes = message3_with_key_data(&wrapped);
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let m3 = EapolKey::parse(&m3_bytes).unwrap();
        let mut replay_window = ReplayWindow::new();
        let verified = m3.verify_message3(&m1, &KCK, &mut replay_window).unwrap();
        let mut output = [0xa5; 400];
        let unwrapped = verified.unwrap_key_data(&KEK, &mut output).unwrap();
        let validated = unwrapped.validate_rsn_key_data(&RSN_IE).unwrap();
        let group_key = validated.group_key().unwrap();
        assert_eq!(group_key.key_id(), 2);
        assert!(group_key.transmit());
        assert_eq!(group_key.temporal_key(), &[0x77; 16]);
        assert_eq!(&output[..plaintext.len()], &[0; 32]);
        assert!(output[plaintext.len()..].iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn key_data_rsn_mismatch_and_unwrap_integrity_failure_are_rejected_and_erased() {
        let mut plaintext = Vec::new();
        plaintext.extend_from_slice(&RSN_IE);
        plaintext.extend_from_slice(&[221, 22, 0x00, 0x0f, 0xac, 0x01, 0x00, 0x00]);
        plaintext.extend_from_slice(&[0x88; 16]);
        let wrapped = wrap_key_data(&KEK, &plaintext);
        let m1_bytes = message1();
        let m3_bytes = message3_with_key_data(&wrapped);
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let m3 = EapolKey::parse(&m3_bytes).unwrap();
        let mut replay_window = ReplayWindow::new();
        let verified = m3.verify_message3(&m1, &KCK, &mut replay_window).unwrap();
        let mut output = [0xa5; 400];
        assert_eq!(
            verified
                .unwrap_key_data(&KEK, &mut output)
                .unwrap()
                .validate_rsn_key_data(&[48, 6, 0, 0, 0, 0, 0, 0])
                .err(),
            Some(EapolError::RsnElementMismatch)
        );
        assert_eq!(&output[..plaintext.len()], &[0; 32]);

        let mut corrupted = wrapped;
        corrupted[0] ^= 1;
        let m3_bytes = message3_with_key_data(&corrupted);
        let m3 = EapolKey::parse(&m3_bytes).unwrap();
        let mut corrupted_window = ReplayWindow::new();
        let verified = m3
            .verify_message3(&m1, &KCK, &mut corrupted_window)
            .unwrap();
        output.fill(0xa5);
        assert_eq!(
            verified.unwrap_key_data(&KEK, &mut output).err(),
            Some(EapolError::InvalidKeyWrapIntegrity)
        );
        assert_eq!(&output[..plaintext.len()], &[0; 32]);
    }

    #[test]
    fn message4_is_signed_and_uses_message3_replay_counter() {
        let m1_bytes = message1();
        let wrapped = wrap_key_data(&KEK, &RSN_IE);
        let m3_bytes = message3_with_key_data(&wrapped);
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let m3 = EapolKey::parse(&m3_bytes).unwrap();
        let mut replay_window = ReplayWindow::new();
        let verified = m3.verify_message3(&m1, &KCK, &mut replay_window).unwrap();
        let mut key_data = [0; 400];
        let validated = verified
            .unwrap_key_data(&KEK, &mut key_data)
            .unwrap()
            .validate_rsn_key_data(&RSN_IE)
            .unwrap();
        let mut output = [0xa5; 110];
        let bytes = encode_message4(&validated, &KCK, &mut output).unwrap();
        assert_eq!(bytes, EAPOL_FIXED_FRAME_BYTES);
        assert_eq!(&output[5..7], &[3, 10]);
        assert_eq!(&output[9..17], &REPLAY_3);
        assert_eq!(&output[97..99], &[0, 0]);
        EapolKey::parse(&output[..bytes])
            .unwrap()
            .verify_mic(&KCK)
            .unwrap();
        assert!(output[bytes..].iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn message4_preserves_the_message3_secure_flag() {
        let m1_bytes = message1();
        let wrapped = wrap_key_data(&KEK, &RSN_IE);
        let mut m3_bytes = message3_with_key_data(&wrapped);
        let info = u16::from_be_bytes([m3_bytes[5], m3_bytes[6]]) & !KEY_INFO_SECURE;
        m3_bytes[5..7].copy_from_slice(&info.to_be_bytes());
        super::write_mic(&mut m3_bytes, &KCK).unwrap();

        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let m3 = EapolKey::parse(&m3_bytes).unwrap();
        let mut replay_window = ReplayWindow::new();
        let verified = m3.verify_message3(&m1, &KCK, &mut replay_window).unwrap();
        assert!(!verified.secure());
        let mut key_data = [0; 400];
        let validated = verified
            .unwrap_key_data(&KEK, &mut key_data)
            .unwrap()
            .validate_rsn_key_data(&RSN_IE)
            .unwrap();
        assert!(!validated.secure());

        let mut output = [0; EAPOL_FIXED_FRAME_BYTES];
        let bytes = encode_message4(&validated, &KCK, &mut output).unwrap();
        assert_eq!(bytes, EAPOL_FIXED_FRAME_BYTES);
        assert_eq!(&output[5..7], &[1, 10]);
        EapolKey::parse(&output[..bytes])
            .unwrap()
            .verify_mic(&KCK)
            .unwrap();
    }

    #[test]
    fn malformed_lengths_flags_and_rsn_elements_fail_closed() {
        let mut m1 = message1();
        m1[2..4].copy_from_slice(&94u16.to_be_bytes());
        assert_eq!(
            EapolKey::parse(&m1).err(),
            Some(EapolError::InvalidFrameLength)
        );

        let mut m1 = message1();
        m1[5] ^= 0x08;
        let parsed = EapolKey::parse(&m1).unwrap();
        assert_eq!(parsed.validate_message1(), Err(EapolError::NotMessage1));

        let mut m1 = message1();
        m1[7..9].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(
            EapolKey::parse(&m1).unwrap().validate_message1(),
            Err(EapolError::NotMessage1)
        );

        let m1_bytes = message1();
        let m1 = EapolKey::parse(&m1_bytes).unwrap();
        let mut m3 = message3();
        m3[7..9].copy_from_slice(&0u16.to_be_bytes());
        super::write_mic(&mut m3, &KCK).unwrap();
        assert_eq!(
            EapolKey::parse(&m3)
                .unwrap()
                .verify_message3(&m1, &KCK, &mut ReplayWindow::new())
                .err(),
            Some(EapolError::NotMessage3)
        );

        let mut output = [0xa5; 110];
        assert_eq!(
            encode_message2(&REPLAY_1, &SNONCE, &[48, 7, 0], &KCK, &mut output),
            Err(EapolError::InvalidRsnElement)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
        assert_eq!(
            encode_message2(&REPLAY_1, &SNONCE, &RSN_IE, &KCK, &mut output[..100]),
            Err(EapolError::OutputTooSmall)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
}
