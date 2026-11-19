//! Bounded selection of a compatible WPA2-Personal access point from scan results.
//!
//! The caller supplies the scan frames, RSSI measurements, approved channel
//! list, and optional pinned BSSID. This module does not start a scan, choose
//! regulatory policy, associate, or transmit frames.
#![forbid(unsafe_code)]

use crate::ath10k_nwifi_rx::{BssAdvertisement, parse_bss_advertisement};
use crate::ieee80211_wpa2::validate_rsn_ie;

const MAX_SCAN_CANDIDATES: usize = 128;
const MAX_CANDIDATE_FRAME_BYTES: usize = 4096;
const MAX_ALLOWED_CHANNELS: usize = 255;
const CAPABILITY_ESS: u16 = 1 << 0;
const CAPABILITY_IBSS: u16 = 1 << 1;
const CAPABILITY_PRIVACY: u16 = 1 << 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BssSelectionError {
    InvalidSsid,
    NoAllowedChannels,
    TooManyAllowedChannels,
    InvalidAllowedChannel,
    DuplicateAllowedChannel,
    InvalidPinnedBssid,
    TooManyCandidates,
    CandidateFrameTooLarge,
    NoCompatibleBss,
}

/// One borrowed beacon or probe response and the RSSI measured for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanCandidate<'a> {
    pub frame: &'a [u8],
    pub rssi_dbm: i8,
    /// WMI RX channel, when available. If the beacon advertises a primary
    /// channel too, both sources must agree.
    pub observed_channel: Option<u8>,
}

/// Selection constraints supplied by the scan caller and network settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssSelectionPolicy<'a> {
    pub ssid: &'a [u8],
    /// If present, only this access point may be selected.
    pub pinned_bssid: Option<[u8; 6]>,
    /// Caller-approved primary channels; the selector never expands this list.
    pub allowed_channels: &'a [u8],
    pub minimum_rssi_dbm: i8,
}

/// A selected, borrowed advertisement plus the scan's channel and RSSI data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedBss<'a> {
    pub advertisement: BssAdvertisement<'a>,
    pub channel: u8,
    pub rssi_dbm: i8,
}

/// Select the strongest exact-SSID WPA2-PSK/CCMP AP on an approved channel.
///
/// Malformed or incompatible scan records are skipped. An exact RSSI tie is
/// resolved by the lowest BSSID, then the lowest channel, so results do not
/// depend on scan-result ordering. Fully identical records retain the first.
pub fn select_bss<'a>(
    candidates: &[ScanCandidate<'a>],
    policy: BssSelectionPolicy<'_>,
) -> Result<SelectedBss<'a>, BssSelectionError> {
    if !(1..=32).contains(&policy.ssid.len()) {
        return Err(BssSelectionError::InvalidSsid);
    }
    if policy.allowed_channels.is_empty() {
        return Err(BssSelectionError::NoAllowedChannels);
    }
    if policy.allowed_channels.len() > MAX_ALLOWED_CHANNELS {
        return Err(BssSelectionError::TooManyAllowedChannels);
    }
    for (index, channel) in policy.allowed_channels.iter().enumerate() {
        if *channel == 0 {
            return Err(BssSelectionError::InvalidAllowedChannel);
        }
        if policy.allowed_channels[..index].contains(channel) {
            return Err(BssSelectionError::DuplicateAllowedChannel);
        }
    }
    if policy
        .pinned_bssid
        .is_some_and(|bssid| !valid_unicast_bssid(&bssid))
    {
        return Err(BssSelectionError::InvalidPinnedBssid);
    }
    if candidates.len() > MAX_SCAN_CANDIDATES {
        return Err(BssSelectionError::TooManyCandidates);
    }

    let mut best: Option<SelectedBss<'a>> = None;
    for candidate in candidates {
        if candidate.frame.len() > MAX_CANDIDATE_FRAME_BYTES {
            return Err(BssSelectionError::CandidateFrameTooLarge);
        }
        if candidate.rssi_dbm < policy.minimum_rssi_dbm {
            continue;
        }
        let Ok(advertisement) = parse_bss_advertisement(candidate.frame) else {
            continue;
        };
        if advertisement.ssid != Some(policy.ssid)
            || advertisement.supported_rates.is_none()
            || advertisement.capabilities & CAPABILITY_ESS == 0
            || advertisement.capabilities & CAPABILITY_IBSS != 0
            || advertisement.capabilities & CAPABILITY_PRIVACY == 0
            || policy
                .pinned_bssid
                .is_some_and(|pinned| advertisement.bssid != pinned)
        {
            continue;
        }
        let channel = match (candidate.observed_channel, advertisement.primary_channel) {
            (Some(observed), Some(advertised)) if observed != advertised => continue,
            (Some(observed), _) => observed,
            (None, Some(advertised)) => advertised,
            (None, None) => continue,
        };
        if !policy.allowed_channels.contains(&channel) {
            continue;
        }
        let Some(rsn_ie) = advertisement.rsn_ie else {
            continue;
        };
        if validate_rsn_ie(rsn_ie).is_err() {
            continue;
        }

        let selected = SelectedBss {
            advertisement,
            channel,
            rssi_dbm: candidate.rssi_dbm,
        };
        if best.is_none_or(|current| ranks_before(selected, current)) {
            best = Some(selected);
        }
    }
    best.ok_or(BssSelectionError::NoCompatibleBss)
}

fn ranks_before(candidate: SelectedBss<'_>, current: SelectedBss<'_>) -> bool {
    candidate.rssi_dbm > current.rssi_dbm
        || (candidate.rssi_dbm == current.rssi_dbm
            && (candidate.advertisement.bssid < current.advertisement.bssid
                || (candidate.advertisement.bssid == current.advertisement.bssid
                    && candidate.channel < current.channel)))
}

fn valid_unicast_bssid(bssid: &[u8; 6]) -> bool {
    bssid.iter().any(|byte| *byte != 0) && bssid[0] & 1 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSN_WPA2_PSK_CCMP: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];
    const RSN_WPA2_EAP_CCMP: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 1, 0, 0,
    ];

    fn beacon(bssid: [u8; 6], ssid: &[u8], channel: u8, rsn_ie: &[u8]) -> ([u8; 128], usize) {
        let length = 36 + 2 + ssid.len() + 3 + 3 + rsn_ie.len();
        let mut frame = [0; 128];
        frame[..2].copy_from_slice(&0x0080u16.to_le_bytes());
        frame[16..22].copy_from_slice(&bssid);
        frame[34..36].copy_from_slice(&0x0011u16.to_le_bytes());
        let mut offset = 36;
        frame[offset..offset + 2].copy_from_slice(&[0, ssid.len() as u8]);
        offset += 2;
        frame[offset..offset + ssid.len()].copy_from_slice(ssid);
        offset += ssid.len();
        frame[offset..offset + 3].copy_from_slice(&[1, 1, 0x82]);
        offset += 3;
        frame[offset..offset + 3].copy_from_slice(&[3, 1, channel]);
        offset += 3;
        frame[offset..offset + rsn_ie.len()].copy_from_slice(rsn_ie);
        (frame, length)
    }

    fn candidate<'a>(frame: &'a [u8], rssi_dbm: i8) -> ScanCandidate<'a> {
        ScanCandidate {
            frame,
            rssi_dbm,
            observed_channel: None,
        }
    }

    fn policy<'a>(ssid: &'a [u8], allowed_channels: &'a [u8]) -> BssSelectionPolicy<'a> {
        BssSelectionPolicy {
            ssid,
            pinned_bssid: None,
            allowed_channels,
            minimum_rssi_dbm: -80,
        }
    }

    #[test]
    fn selects_strongest_matching_wpa2_ap_only_from_approved_channels() {
        let (weak_frame, weak_len) = beacon([2, 0, 0, 0, 0, 2], b"home", 1, &RSN_WPA2_PSK_CCMP);
        let (strong_frame, strong_len) = beacon([2, 0, 0, 0, 0, 3], b"home", 6, &RSN_WPA2_PSK_CCMP);
        let (wrong_ssid_frame, wrong_ssid_len) =
            beacon([2, 0, 0, 0, 0, 1], b"guest", 1, &RSN_WPA2_PSK_CCMP);
        let (unapproved_channel_frame, unapproved_channel_len) =
            beacon([2, 0, 0, 0, 0, 4], b"home", 11, &RSN_WPA2_PSK_CCMP);
        let candidates = [
            candidate(&weak_frame[..weak_len], -55),
            candidate(&wrong_ssid_frame[..wrong_ssid_len], -20),
            candidate(&unapproved_channel_frame[..unapproved_channel_len], -30),
            candidate(&strong_frame[..strong_len], -42),
        ];

        let selected = select_bss(&candidates, policy(b"home", &[1, 6])).unwrap();
        assert_eq!(selected.advertisement.bssid, [2, 0, 0, 0, 0, 3]);
        assert_eq!(selected.channel, 6);
        assert_eq!(selected.rssi_dbm, -42);
        assert_eq!(selected.advertisement.rsn_ie, Some(&RSN_WPA2_PSK_CCMP[..]));
    }

    #[test]
    fn rejects_other_akm_and_honors_optional_bssid_pin() {
        let (eap_frame, eap_len) = beacon([2, 0, 0, 0, 0, 2], b"home", 1, &RSN_WPA2_EAP_CCMP);
        let (psk_frame, psk_len) = beacon([2, 0, 0, 0, 0, 3], b"home", 1, &RSN_WPA2_PSK_CCMP);
        let candidates = [
            candidate(&eap_frame[..eap_len], -30),
            candidate(&psk_frame[..psk_len], -60),
        ];
        let mut pinned = policy(b"home", &[1]);
        pinned.pinned_bssid = Some([2, 0, 0, 0, 0, 3]);

        assert_eq!(
            select_bss(&candidates, pinned).unwrap().advertisement.bssid,
            [2, 0, 0, 0, 0, 3]
        );
        pinned.pinned_bssid = Some([2, 0, 0, 0, 0, 9]);
        assert_eq!(
            select_bss(&candidates, pinned),
            Err(BssSelectionError::NoCompatibleBss)
        );
    }

    #[test]
    fn requires_an_infrastructure_privacy_capable_bss() {
        let (mut frame, length) = beacon([2, 0, 0, 0, 0, 2], b"home", 1, &RSN_WPA2_PSK_CCMP);
        frame[34..36].copy_from_slice(&0x0012u16.to_le_bytes()); // IBSS + privacy
        let candidates = [candidate(&frame[..length], -35)];
        assert_eq!(
            select_bss(&candidates, policy(b"home", &[1])),
            Err(BssSelectionError::NoCompatibleBss)
        );
    }

    #[test]
    fn breaks_equal_rssi_ties_by_bssid_independent_of_scan_order() {
        let (higher_frame, higher_len) = beacon([2, 0, 0, 0, 0, 9], b"home", 1, &RSN_WPA2_PSK_CCMP);
        let (lower_frame, lower_len) = beacon([2, 0, 0, 0, 0, 3], b"home", 1, &RSN_WPA2_PSK_CCMP);
        let high = candidate(&higher_frame[..higher_len], -40);
        let low = candidate(&lower_frame[..lower_len], -40);

        assert_eq!(
            select_bss(&[high, low], policy(b"home", &[1]))
                .unwrap()
                .advertisement
                .bssid,
            [2, 0, 0, 0, 0, 3]
        );
        assert_eq!(
            select_bss(&[low, high], policy(b"home", &[1]))
                .unwrap()
                .advertisement
                .bssid,
            [2, 0, 0, 0, 0, 3]
        );
    }

    #[test]
    fn validates_policy_before_scanning() {
        assert_eq!(
            select_bss(&[], policy(b"", &[1])),
            Err(BssSelectionError::InvalidSsid)
        );
        assert_eq!(
            select_bss(&[], policy(b"home", &[])),
            Err(BssSelectionError::NoAllowedChannels)
        );
        assert_eq!(
            select_bss(&[], policy(b"home", &[1, 1])),
            Err(BssSelectionError::DuplicateAllowedChannel)
        );
        assert_eq!(
            select_bss(&[], policy(b"home", &[0])),
            Err(BssSelectionError::InvalidAllowedChannel)
        );
        let mut invalid_pin = policy(b"home", &[1]);
        invalid_pin.pinned_bssid = Some([0xff; 6]);
        assert_eq!(
            select_bss(&[], invalid_pin),
            Err(BssSelectionError::InvalidPinnedBssid)
        );
    }
}
