//! WPA2-PSK PMK and CCMP-128 PTK derivation.
//!
//! This module provides key derivation and validation for one strict WPA2-PSK
//! / CCMP-128 RSN profile. It does not implement the EAPOL handshake, install
//! keys, encrypt frames, or authorize radio activity. Passphrases must be
//! printable ASCII with length 8..=63.
#![forbid(unsafe_code)]

use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use zeroize::{Zeroize, Zeroizing};

const PMK_BYTES: usize = 32;
const PTK_BYTES: usize = 48;
const KCK_BYTES: usize = 16;
const KEK_BYTES: usize = 16;
const TK_BYTES: usize = 16;
const PBKDF2_ROUNDS: u32 = 4096;
const WPA2_LABEL: &[u8] = b"Pairwise key expansion";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wpa2Error {
    InvalidPassphrase,
    InvalidSsid,
    InvalidPeerAddress,
    InvalidRsnInformationElement,
    UnsupportedRsnSuite,
    HmacInitialization,
}

/// Accept only the WPA2-Personal / CCMP-128 RSN profile implemented here.
/// The input is the complete element, including its ID and length bytes.
pub fn validate_rsn_ie(ie: &[u8]) -> Result<(), Wpa2Error> {
    const RSN_ID: u8 = 48;
    const CCMP_128: [u8; 4] = [0x00, 0x0f, 0xac, 0x04];
    const PSK_AKM: [u8; 4] = [0x00, 0x0f, 0xac, 0x02];
    const MFPR: u16 = 1 << 6;
    const MFPC: u16 = 1 << 7;

    if ie.len() < 2 || ie[0] != RSN_ID || usize::from(ie[1]) + 2 != ie.len() {
        return Err(Wpa2Error::InvalidRsnInformationElement);
    }
    let body = &ie[2..];
    let mut offset = 0usize;
    if take(body, &mut offset, 2)? != [1, 0] {
        return Err(Wpa2Error::UnsupportedRsnSuite);
    }
    if take(body, &mut offset, 4)? != CCMP_128 {
        return Err(Wpa2Error::UnsupportedRsnSuite);
    }
    if take(body, &mut offset, 2)? != [1, 0] || take(body, &mut offset, 4)? != CCMP_128 {
        return Err(Wpa2Error::UnsupportedRsnSuite);
    }
    if take(body, &mut offset, 2)? != [1, 0] || take(body, &mut offset, 4)? != PSK_AKM {
        return Err(Wpa2Error::UnsupportedRsnSuite);
    }

    if offset < body.len() {
        let capabilities = take(body, &mut offset, 2)?;
        let capabilities = u16::from_le_bytes([capabilities[0], capabilities[1]]);
        if capabilities & (MFPR | MFPC) != 0 {
            // 802.11w protected management frames are outside this profile.
            return Err(Wpa2Error::UnsupportedRsnSuite);
        }
    }
    if offset < body.len() {
        let pmkid_count = take(body, &mut offset, 2)?;
        if pmkid_count != [0, 0] || offset != body.len() {
            return Err(Wpa2Error::UnsupportedRsnSuite);
        }
    }
    if offset != body.len() {
        return Err(Wpa2Error::UnsupportedRsnSuite);
    }
    Ok(())
}

fn take<'a>(body: &'a [u8], offset: &mut usize, bytes: usize) -> Result<&'a [u8], Wpa2Error> {
    let end = offset
        .checked_add(bytes)
        .filter(|end| *end <= body.len())
        .ok_or(Wpa2Error::InvalidRsnInformationElement)?;
    let value = &body[*offset..end];
    *offset = end;
    Ok(value)
}

/// WPA2-Personal pairwise master key, erased when dropped.
pub struct Pmk([u8; PMK_BYTES]);

impl Pmk {
    /// Derive the WPA2-PSK PMK from an ASCII passphrase and SSID.
    ///
    /// Raw 256-bit hexadecimal PSKs are deliberately not accepted by this
    /// entry point; callers must not accidentally treat their 64 hex digits
    /// as a passphrase.
    pub fn from_passphrase(passphrase: &[u8], ssid: &[u8]) -> Result<Self, Wpa2Error> {
        if !(8..=63).contains(&passphrase.len())
            || !passphrase.iter().all(|byte| (0x20..=0x7e).contains(byte))
        {
            return Err(Wpa2Error::InvalidPassphrase);
        }
        if !(1..=32).contains(&ssid.len()) {
            return Err(Wpa2Error::InvalidSsid);
        }

        let mut bytes = [0; PMK_BYTES];
        pbkdf2_hmac::<Sha1>(passphrase, ssid, PBKDF2_ROUNDS, &mut bytes);
        Ok(Self(bytes))
    }

    /// Derive the CCMP-128 KCK, KEK, and temporal key for a four-way handshake.
    ///
    /// Addresses and nonces are sorted as required by the WPA pairwise key
    /// hierarchy, so the result is independent of argument ordering. Nonces
    /// are borrowed and not retained; the caller must erase its SNonce after
    /// the handshake no longer needs it.
    pub fn derive_ccmp128_ptk(
        &self,
        authenticator: [u8; 6],
        supplicant: [u8; 6],
        anonce: &[u8; 32],
        snonce: &[u8; 32],
    ) -> Result<PairwiseKeys, Wpa2Error> {
        if !valid_unicast_address(&authenticator)
            || !valid_unicast_address(&supplicant)
            || authenticator == supplicant
        {
            return Err(Wpa2Error::InvalidPeerAddress);
        }

        let (address_low, address_high) = if authenticator < supplicant {
            (authenticator, supplicant)
        } else {
            (supplicant, authenticator)
        };
        let (nonce_low, nonce_high) = if anonce <= snonce {
            (anonce, snonce)
        } else {
            (snonce, anonce)
        };

        let mut context = Zeroizing::new([0u8; 76]);
        context[..6].copy_from_slice(&address_low);
        context[6..12].copy_from_slice(&address_high);
        context[12..44].copy_from_slice(nonce_low);
        context[44..76].copy_from_slice(nonce_high);

        let mut ptk = Zeroizing::new([0u8; PTK_BYTES]);
        for counter in 0u8..3 {
            let mut mac =
                Hmac::<Sha1>::new_from_slice(&self.0).map_err(|_| Wpa2Error::HmacInitialization)?;
            mac.update(WPA2_LABEL);
            mac.update(&[0]);
            mac.update(&*context);
            mac.update(&[counter]);
            let mut block = mac.finalize().into_bytes();
            let start = usize::from(counter) * 20;
            let end = (start + block.len()).min(PTK_BYTES);
            ptk[start..end].copy_from_slice(&block[..end - start]);
            block.as_mut_slice().zeroize();
        }
        context.zeroize();

        let mut keys = PairwiseKeys {
            kck: [0; KCK_BYTES],
            kek: [0; KEK_BYTES],
            tk: [0; TK_BYTES],
        };
        keys.kck.copy_from_slice(&ptk[..KCK_BYTES]);
        keys.kek
            .copy_from_slice(&ptk[KCK_BYTES..KCK_BYTES + KEK_BYTES]);
        keys.tk
            .copy_from_slice(&ptk[KCK_BYTES + KEK_BYTES..KCK_BYTES + KEK_BYTES + TK_BYTES]);
        ptk.zeroize();
        Ok(keys)
    }

    #[cfg(test)]
    fn bytes_for_test(&self) -> &[u8; PMK_BYTES] {
        &self.0
    }
}

impl Drop for Pmk {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// CCMP-128 pairwise keys. Each key is erased when this value is dropped.
pub struct PairwiseKeys {
    kck: [u8; KCK_BYTES],
    kek: [u8; KEK_BYTES],
    tk: [u8; TK_BYTES],
}

impl PairwiseKeys {
    /// EAPOL-Key MIC key (KCK), borrowed for the handshake implementation.
    pub fn kck(&self) -> &[u8; KCK_BYTES] {
        &self.kck
    }

    /// EAPOL-Key encryption key (KEK), borrowed for GTK unwrap.
    pub fn kek(&self) -> &[u8; KEK_BYTES] {
        &self.kek
    }

    /// CCMP-128 temporal key, borrowed by the protected data path.
    pub fn temporal_key(&self) -> &[u8; TK_BYTES] {
        &self.tk
    }
}

impl Drop for PairwiseKeys {
    fn drop(&mut self) {
        self.kck.zeroize();
        self.kek.zeroize();
        self.tk.zeroize();
    }
}

fn valid_unicast_address(address: &[u8; 6]) -> bool {
    address.iter().any(|byte| *byte != 0) && address[0] & 1 == 0
}

#[cfg(test)]
mod tests {
    use super::{Pmk, Wpa2Error, validate_rsn_ie};

    const RSN_WPA2_PSK_CCMP: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];

    #[test]
    fn accepts_only_the_complete_wpa2_psk_ccmp128_profile() {
        validate_rsn_ie(&RSN_WPA2_PSK_CCMP).unwrap();

        let without_capabilities = [
            48, 18, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2,
        ];
        validate_rsn_ie(&without_capabilities).unwrap();

        let with_empty_pmkid_list = [
            48, 22, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0, 0, 0,
        ];
        validate_rsn_ie(&with_empty_pmkid_list).unwrap();
    }

    #[test]
    fn rejects_malformed_unsupported_and_pmf_required_rsn_profiles() {
        assert_eq!(
            validate_rsn_ie(&RSN_WPA2_PSK_CCMP[..RSN_WPA2_PSK_CCMP.len() - 1]),
            Err(Wpa2Error::InvalidRsnInformationElement)
        );
        let mut wrong_pairwise = RSN_WPA2_PSK_CCMP;
        wrong_pairwise[13] = 2;
        assert_eq!(
            validate_rsn_ie(&wrong_pairwise),
            Err(Wpa2Error::UnsupportedRsnSuite)
        );
        let mut pmf_required = RSN_WPA2_PSK_CCMP;
        pmf_required[20] = 0x40;
        assert_eq!(
            validate_rsn_ie(&pmf_required),
            Err(Wpa2Error::UnsupportedRsnSuite)
        );
    }

    #[test]
    fn derives_the_known_wpa2_pmk_vector() {
        let pmk = Pmk::from_passphrase(b"password", b"IEEE").unwrap();
        assert_eq!(
            pmk.bytes_for_test(),
            &[
                0xf4, 0x2c, 0x6f, 0xc5, 0x2d, 0xf0, 0xeb, 0xef, 0x9e, 0xbb, 0x4b, 0x90, 0xb3, 0x8a,
                0x5f, 0x90, 0x2e, 0x83, 0xfe, 0x1b, 0x13, 0x5a, 0x70, 0xe2, 0x3a, 0xed, 0x76, 0x2e,
                0x97, 0x10, 0xa1, 0x2e,
            ]
        );
    }

    #[test]
    fn rejects_invalid_passphrases_ssids_and_peer_addresses() {
        assert_eq!(
            Pmk::from_passphrase(b"short", b"IEEE").err(),
            Some(Wpa2Error::InvalidPassphrase)
        );
        assert_eq!(
            Pmk::from_passphrase(&[b'a'; 64], b"IEEE").err(),
            Some(Wpa2Error::InvalidPassphrase)
        );
        assert_eq!(
            Pmk::from_passphrase(b"password", b"").err(),
            Some(Wpa2Error::InvalidSsid)
        );
        assert_eq!(
            Pmk::from_passphrase(b"password", &[b'a'; 33]).err(),
            Some(Wpa2Error::InvalidSsid)
        );

        let pmk = Pmk::from_passphrase(b"password", b"IEEE").unwrap();
        assert_eq!(
            pmk.derive_ccmp128_ptk([0; 6], [2, 0, 0, 0, 0, 1], &[1; 32], &[2; 32],)
                .err(),
            Some(Wpa2Error::InvalidPeerAddress)
        );
    }

    #[test]
    fn derives_the_known_ccmp128_ptk_and_sorts_both_inputs() {
        let pmk = Pmk::from_passphrase(b"password", b"IEEE").unwrap();
        let ap = [0x02, 0, 0, 0, 0, 2];
        let station = [0x02, 0, 0, 0, 0, 1];
        let anonce = core::array::from_fn(|i| i as u8);
        let snonce = core::array::from_fn(|i| (i + 32) as u8);
        let keys = pmk
            .derive_ccmp128_ptk(ap, station, &anonce, &snonce)
            .unwrap();
        let swapped = pmk
            .derive_ccmp128_ptk(station, ap, &snonce, &anonce)
            .unwrap();

        assert_eq!(
            keys.kck(),
            &[
                0xdd, 0xbd, 0x69, 0x93, 0xdd, 0xce, 0xf2, 0xe5, 0x5f, 0x87, 0xa6, 0x20, 0xa1, 0x90,
                0xea, 0x20,
            ]
        );
        assert_eq!(
            keys.kek(),
            &[
                0xf5, 0xcc, 0x99, 0x9d, 0x44, 0xa9, 0x95, 0x9e, 0xe8, 0x83, 0xd5, 0xbd, 0xf5, 0xe1,
                0x79, 0x8e,
            ]
        );
        assert_eq!(
            keys.temporal_key(),
            &[
                0x74, 0x95, 0xc0, 0xaa, 0x22, 0x58, 0x82, 0x88, 0xf3, 0xb7, 0x58, 0xfb, 0x43, 0x18,
                0xd0, 0x0a,
            ]
        );
        assert_eq!(keys.kck(), swapped.kck());
        assert_eq!(keys.kek(), swapped.kek());
        assert_eq!(keys.temporal_key(), swapped.temporal_key());
    }
}
