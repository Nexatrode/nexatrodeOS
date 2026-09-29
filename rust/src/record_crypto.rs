//! Small, allocation-free AEAD helpers shared by the disk vault and host tests.

use aes_gcm_siv::{
    Aes256GcmSiv, Nonce, Tag,
    aead::{AeadInOut, KeyInit, inout::InOutBuf},
};

pub const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

pub(crate) fn encrypt(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    associated_data: &[u8],
    data: &mut [u8],
) -> Option<[u8; TAG_SIZE]> {
    let cipher = Aes256GcmSiv::new_from_slice(key).ok()?;
    let nonce = <&Nonce>::try_from(nonce.as_slice()).ok()?;
    let tag = cipher
        .encrypt_inout_detached(nonce, associated_data, InOutBuf::from(data))
        .ok()?;
    let mut result = [0; TAG_SIZE];
    result.copy_from_slice(tag.as_slice());
    Some(result)
}

pub(crate) fn decrypt(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    associated_data: &[u8],
    data: &mut [u8],
    tag: &[u8; TAG_SIZE],
) -> bool {
    let Ok(cipher) = Aes256GcmSiv::new_from_slice(key) else {
        return false;
    };
    let Ok(nonce) = <&Nonce>::try_from(nonce.as_slice()) else {
        return false;
    };
    let Ok(tag) = <&Tag>::try_from(tag.as_slice()) else {
        return false;
    };
    cipher
        .decrypt_inout_detached(nonce, associated_data, InOutBuf::from(data), tag)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; KEY_SIZE] = [0x42; KEY_SIZE];

    #[test]
    fn encrypts_and_decrypts_in_place() {
        let nonce = [0x31; NONCE_SIZE];
        let aad = b"namespace=notes;generation=7";
        let original = *b"record42";
        let mut data = original;
        let tag = encrypt(&TEST_KEY, &nonce, aad, &mut data).expect("encryption succeeds");
        assert_ne!(data, original);
        assert!(decrypt(&TEST_KEY, &nonce, aad, &mut data, &tag));
        assert_eq!(data, original);
    }

    #[test]
    fn rejects_ciphertext_tag_and_context_tampering() {
        let nonce = [0x31; NONCE_SIZE];
        let aad = b"namespace=notes;generation=7";
        let original = *b"record42";
        let mut ciphertext = original;
        let tag = encrypt(&TEST_KEY, &nonce, aad, &mut ciphertext).expect("encryption succeeds");

        let mut changed_ciphertext = ciphertext;
        changed_ciphertext[0] ^= 1;
        assert!(!decrypt(
            &TEST_KEY,
            &nonce,
            aad,
            &mut changed_ciphertext,
            &tag
        ));

        let mut changed_tag = tag;
        changed_tag[0] ^= 1;
        let mut tag_test = ciphertext;
        assert!(!decrypt(
            &TEST_KEY,
            &nonce,
            aad,
            &mut tag_test,
            &changed_tag
        ));

        let mut changed_context = ciphertext;
        assert!(!decrypt(
            &TEST_KEY,
            &nonce,
            b"namespace=contacts;generation=7",
            &mut changed_context,
            &tag
        ));
    }
}
