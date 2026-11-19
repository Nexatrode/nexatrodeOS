//! Bounded WPA2-Personal station handshake and protected data gate.
//!
//! This controller joins the existing station, PMK/PTK, EAPOL, and CCMP
//! codecs. The caller supplies fresh SNonce material from its entropy service,
//! transports the returned Native-WiFi frames, and confirms actual key install
//! and EAPOL transmission. Protected data is unavailable until both confirms
//! succeed and message 3 carries the Secure bit. Retransmission timers,
//! association selection, deauthentication, PMF, rekeying, and the hardware
//! WLAN transport remain outside this module.
#![forbid(unsafe_code)]

use zeroize::{Zeroize, Zeroizing};

use crate::ath10k_nwifi_rx::{FrameHeaderError, FrameKind, inspect_frame};
use crate::ieee80211_ccmp::{
    CcmpError, CcmpReplayWindow, CcmpTxPacketNumber, decrypt_frame, encrypt_frame,
};
use crate::ieee80211_eapol::{
    EapolError, EapolKey, GroupKey, MAX_EAPOL_KEY_FRAME_BYTES, ReplayWindow, encode_message2,
    encode_message4_fields,
};
use crate::ieee80211_wpa2::{PairwiseKeys, Pmk, Wpa2Error, validate_rsn_ie};
use crate::wlan_key_confirmation::KeyInstallConfirmation;

const IEEE80211_HEADER_BYTES: usize = 24;
const LLC_SNAP_BYTES: [u8; 8] = [0xaa, 0xaa, 0x03, 0, 0, 0, 0x88, 0x8e];
const EAPOL_ETHERTYPE: [u8; 2] = [0x88, 0x8e];
const DATA_FRAME_CONTROL: u16 = 0x0008;
const FC_TO_DS: u16 = 0x0100;
const FC_FROM_DS: u16 = 0x0200;
const FC_PROTECTED: u16 = 0x4000;
const FC_QOS_SUBTYPE: u16 = 0x0080;
const FC_DS_MASK: u16 = FC_TO_DS | FC_FROM_DS;
const SEQUENCE_NUMBER_MAX: u16 = 0x0fff;
const CCMP_RX_QUEUES: usize = 17;
const NON_QOS_QUEUE: usize = 16;
const WPA2_PAIRWISE_KEY_INDEX: u8 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupplicantError {
    InvalidStationAddress,
    InvalidBssid,
    InvalidSNonce,
    InvalidRsnInformationElement,
    InvalidSequenceNumber,
    InvalidDataDirection,
    InvalidDataPeer,
    InvalidEapolFrame,
    NotEapolKey,
    EapolFrameTooLarge,
    OutputTooSmall,
    UnexpectedMessage,
    ConflictingMessage1,
    InvalidGroupRsc,
    KeysNotPending,
    KeysAlreadyConfirmed,
    KeyInstallNotConfirmed,
    Message4NotReady,
    Message4NotSubmitted,
    ProtectedDataNotAuthorized,
    GroupKeyUnavailable,
    EthernetFrame(crate::ieee80211_ethernet::Error),
    InvalidFrameHeader(FrameHeaderError),
    Eapol(EapolError),
    Wpa2(Wpa2Error),
    Ccmp(CcmpError),
}

impl From<EapolError> for SupplicantError {
    fn from(value: EapolError) -> Self {
        Self::Eapol(value)
    }
}

impl From<Wpa2Error> for SupplicantError {
    fn from(value: Wpa2Error) -> Self {
        Self::Wpa2(value)
    }
}

impl From<CcmpError> for SupplicantError {
    fn from(value: CcmpError) -> Self {
        Self::Ccmp(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupplicantPhase {
    AwaitingMessage1,
    AwaitingMessage3,
    KeysPending,
    Established,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message3Result {
    Accepted,
    Duplicate,
}

/// One selected WPA2-Personal/CCMP-128 association.
pub struct Wpa2Supplicant<'rsn> {
    phase: SupplicantPhase,
    pmk: Option<Pmk>,
    pairwise_keys: Option<PairwiseKeys>,
    station: [u8; 6],
    bssid: [u8; 6],
    snonce: Zeroizing<[u8; 32]>,
    rsn_ie: &'rsn [u8],
    message1: [u8; MAX_EAPOL_KEY_FRAME_BYTES],
    message1_len: usize,
    message3: [u8; MAX_EAPOL_KEY_FRAME_BYTES],
    message3_len: usize,
    eapol_replay: ReplayWindow,
    pending_keys: Option<PendingKeys>,
    keys_installed: bool,
    message4_encoded: bool,
    message4_submitted: bool,
    pairwise_tx_pn: CcmpTxPacketNumber,
    pairwise_rx_pn: [CcmpReplayWindow; CCMP_RX_QUEUES],
    group_rx_pn: [CcmpReplayWindow; CCMP_RX_QUEUES],
}

struct PendingKeys {
    replay_counter: [u8; 8],
    rsc: [u8; 8],
    secure: bool,
    group_key: Option<GroupKey>,
}

impl<'rsn> Wpa2Supplicant<'rsn> {
    /// Create a one-association handshake. `snonce` must come from the
    /// platform entropy service; it is erased when this controller is dropped.
    pub fn new(
        pmk: Pmk,
        station: [u8; 6],
        bssid: [u8; 6],
        snonce: [u8; 32],
        rsn_ie: &'rsn [u8],
    ) -> Result<Self, SupplicantError> {
        if !valid_unicast_address(&station) {
            return Err(SupplicantError::InvalidStationAddress);
        }
        if !valid_unicast_address(&bssid) {
            return Err(SupplicantError::InvalidBssid);
        }
        if snonce.iter().all(|byte| *byte == 0) {
            return Err(SupplicantError::InvalidSNonce);
        }
        validate_rsn_ie(rsn_ie).map_err(|error| match error {
            Wpa2Error::InvalidRsnInformationElement => {
                SupplicantError::InvalidRsnInformationElement
            }
            other => SupplicantError::Wpa2(other),
        })?;

        Ok(Self {
            phase: SupplicantPhase::AwaitingMessage1,
            pmk: Some(pmk),
            pairwise_keys: None,
            station,
            bssid,
            snonce: Zeroizing::new(snonce),
            rsn_ie,
            message1: [0; MAX_EAPOL_KEY_FRAME_BYTES],
            message1_len: 0,
            message3: [0; MAX_EAPOL_KEY_FRAME_BYTES],
            message3_len: 0,
            eapol_replay: ReplayWindow::new(),
            pending_keys: None,
            keys_installed: false,
            message4_encoded: false,
            message4_submitted: false,
            pairwise_tx_pn: CcmpTxPacketNumber::new(0).expect("zero is a valid CCMP PN"),
            pairwise_rx_pn: [CcmpReplayWindow::default(); CCMP_RX_QUEUES],
            group_rx_pn: [CcmpReplayWindow::default(); CCMP_RX_QUEUES],
        })
    }

    pub const fn phase(&self) -> SupplicantPhase {
        self.phase
    }

    pub const fn is_authorized(&self) -> bool {
        matches!(self.phase, SupplicantPhase::Established)
    }

    /// Process EAPOL-Key message 1 and encode message 2. An exact retransmitted
    /// message 1 is answered with the same SNonce/PTK. A different second M1
    /// requires a new association controller.
    pub fn process_message1(
        &mut self,
        eapol_frame: &[u8],
        message2_output: &mut [u8],
    ) -> Result<usize, SupplicantError> {
        if self.phase == SupplicantPhase::Failed {
            return Err(SupplicantError::UnexpectedMessage);
        }
        let message1 = EapolKey::parse(eapol_frame)?;
        message1.validate_message1()?;
        if eapol_frame.len() > self.message1.len() {
            return Err(SupplicantError::EapolFrameTooLarge);
        }

        match self.phase {
            SupplicantPhase::AwaitingMessage1 => {
                let pmk = self
                    .pmk
                    .as_ref()
                    .ok_or(SupplicantError::UnexpectedMessage)?;
                let keys = pmk.derive_ccmp128_ptk(
                    self.bssid,
                    self.station,
                    message1.nonce(),
                    &self.snonce,
                )?;
                let encoded = encode_message2(
                    message1.replay_counter(),
                    &self.snonce,
                    self.rsn_ie,
                    keys.kck(),
                    message2_output,
                )?;
                self.message1[..eapol_frame.len()].copy_from_slice(eapol_frame);
                self.message1_len = eapol_frame.len();
                self.pairwise_keys = Some(keys);
                self.pmk = None;
                self.phase = SupplicantPhase::AwaitingMessage3;
                Ok(encoded)
            }
            SupplicantPhase::AwaitingMessage3
                if eapol_frame == &self.message1[..self.message1_len] =>
            {
                let keys = self
                    .pairwise_keys
                    .as_ref()
                    .ok_or(SupplicantError::UnexpectedMessage)?;
                Ok(encode_message2(
                    message1.replay_counter(),
                    &self.snonce,
                    self.rsn_ie,
                    keys.kck(),
                    message2_output,
                )?)
            }
            SupplicantPhase::AwaitingMessage3 => Err(SupplicantError::ConflictingMessage1),
            SupplicantPhase::KeysPending
            | SupplicantPhase::Established
            | SupplicantPhase::Failed => Err(SupplicantError::UnexpectedMessage),
        }
    }

    /// Accept a Native-WiFi EAPOL-Key message 3. Bad unauthenticated packets
    /// leave the handshake pending. Once a valid MIC authenticates malformed
    /// key data, the association fails closed and erases its PTK.
    pub fn process_message3(
        &mut self,
        eapol_frame: &[u8],
        unwrap_buffer: &mut [u8],
    ) -> Result<Message3Result, SupplicantError> {
        if matches!(
            self.phase,
            SupplicantPhase::KeysPending | SupplicantPhase::Established
        ) {
            if eapol_frame == &self.message3[..self.message3_len] {
                return Ok(Message3Result::Duplicate);
            }
            return Err(SupplicantError::UnexpectedMessage);
        }
        if self.phase != SupplicantPhase::AwaitingMessage3 {
            return Err(SupplicantError::UnexpectedMessage);
        }
        if eapol_frame.len() > self.message3.len() {
            return Err(SupplicantError::EapolFrameTooLarge);
        }

        let message1 = EapolKey::parse(&self.message1[..self.message1_len])?;
        let message3 = EapolKey::parse(eapol_frame)?;
        let encrypted_key_data_len = message3.key_data().len();
        let unwrapped_key_data_len = encrypted_key_data_len
            .checked_sub(8)
            .ok_or(SupplicantError::Eapol(EapolError::InvalidEncryptedKeyData))?;
        if unwrap_buffer.len() < unwrapped_key_data_len {
            return Err(SupplicantError::Eapol(EapolError::OutputTooSmall));
        }
        let pairwise = self
            .pairwise_keys
            .as_ref()
            .ok_or(SupplicantError::UnexpectedMessage)?;
        let verified =
            message3.verify_message3(&message1, pairwise.kck(), &mut self.eapol_replay)?;
        let unwrapped = match verified.unwrap_key_data(pairwise.kek(), unwrap_buffer) {
            Ok(value) => value,
            Err(error) => {
                self.fail();
                return Err(SupplicantError::Eapol(error));
            }
        };
        let validated = match unwrapped.validate_rsn_key_data(self.rsn_ie) {
            Ok(value) => value,
            Err(error) => {
                self.fail();
                return Err(SupplicantError::Eapol(error));
            }
        };
        let (replay_counter, rsc, secure, group_key) = validated.into_parts();
        let group_start_pn = if group_key.is_some() {
            match decode_ccmp_rsc(&rsc) {
                Ok(value) => value,
                Err(error) => {
                    self.fail();
                    return Err(error);
                }
            }
        } else {
            0
        };
        if group_key.is_some() {
            for window in &mut self.group_rx_pn {
                // `decode_ccmp_rsc` already enforces the 48-bit boundary.
                window
                    .reset_to(group_start_pn)
                    .map_err(SupplicantError::Ccmp)?;
            }
        }

        self.message3[..eapol_frame.len()].copy_from_slice(eapol_frame);
        self.message3_len = eapol_frame.len();
        self.pending_keys = Some(PendingKeys {
            replay_counter,
            rsc,
            secure,
            group_key,
        });
        self.phase = SupplicantPhase::KeysPending;
        Ok(Message3Result::Accepted)
    }

    /// Process an inbound From-DS Native-WiFi EAPOL-Key data frame, requiring
    /// the selected AP and station addresses before parsing the EAPOL payload.
    pub fn process_message1_frame(
        &mut self,
        frame: &[u8],
        message2_sequence: u16,
        output: &mut [u8],
    ) -> Result<usize, SupplicantError> {
        if message2_sequence > SEQUENCE_NUMBER_MAX {
            return Err(SupplicantError::InvalidSequenceNumber);
        }
        let eapol = extract_eapol_frame(frame, self.station, self.bssid)?;
        if output.len() < IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len() {
            return Err(SupplicantError::OutputTooSmall);
        }
        let eapol_len = self.process_message1(
            eapol,
            &mut output[IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len()..],
        )?;
        let frame_len = IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len() + eapol_len;
        write_station_eapol_header(
            &mut output[..IEEE80211_HEADER_BYTES],
            self.station,
            self.bssid,
            message2_sequence,
        )?;
        output[IEEE80211_HEADER_BYTES..IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len()]
            .copy_from_slice(&LLC_SNAP_BYTES);
        Ok(frame_len)
    }

    /// Process an inbound From-DS message 3 frame.
    pub fn process_message3_frame(
        &mut self,
        frame: &[u8],
        unwrap_buffer: &mut [u8],
    ) -> Result<Message3Result, SupplicantError> {
        let eapol = extract_eapol_frame(frame, self.station, self.bssid)?;
        self.process_message3(eapol, unwrap_buffer)
    }

    /// Encode message 4 in a complete To-DS Native-WiFi data frame. Source
    /// supplicant ordering permits M4 submission before driver key install;
    /// the data port still remains closed until both operations are confirmed.
    pub fn encode_message4_frame(
        &mut self,
        sequence: u16,
        output: &mut [u8],
    ) -> Result<usize, SupplicantError> {
        if sequence > SEQUENCE_NUMBER_MAX {
            return Err(SupplicantError::InvalidSequenceNumber);
        }
        if self.phase != SupplicantPhase::KeysPending && self.phase != SupplicantPhase::Established
        {
            return Err(SupplicantError::Message4NotReady);
        }
        if output.len() < IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len() {
            return Err(SupplicantError::OutputTooSmall);
        }
        let keys = self
            .pairwise_keys
            .as_ref()
            .ok_or(SupplicantError::Message4NotReady)?;
        let pending = self
            .pending_keys
            .as_ref()
            .ok_or(SupplicantError::Message4NotReady)?;
        let eapol_len = encode_message4_fields(
            &pending.replay_counter,
            pending.secure,
            keys.kck(),
            &mut output[IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len()..],
        )?;
        let frame_len = IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len() + eapol_len;
        write_station_eapol_header(
            &mut output[..IEEE80211_HEADER_BYTES],
            self.station,
            self.bssid,
            sequence,
        )?;
        output[IEEE80211_HEADER_BYTES..IEEE80211_HEADER_BYTES + LLC_SNAP_BYTES.len()]
            .copy_from_slice(&LLC_SNAP_BYTES);
        self.message4_encoded = true;
        Ok(frame_len)
    }

    /// Borrow the exact keys and RSC that the caller must successfully install
    /// before it confirms key installation. The PTK KCK/KEK are never exposed.
    pub fn key_install_view(&self) -> Result<KeyInstallView<'_>, SupplicantError> {
        if self.phase != SupplicantPhase::KeysPending && self.phase != SupplicantPhase::Established
        {
            return Err(SupplicantError::KeysNotPending);
        }
        let pairwise = self
            .pairwise_keys
            .as_ref()
            .ok_or(SupplicantError::KeysNotPending)?;
        let pending = self
            .pending_keys
            .as_ref()
            .ok_or(SupplicantError::KeysNotPending)?;
        Ok(KeyInstallView { pairwise, pending })
    }

    /// Call only after the platform key installer confirms pairwise and any
    /// group key were installed. Failure must leave the data port closed.
    pub(crate) fn confirm_keys_installed_from_tracker(
        &mut self,
        tracker: &impl KeyInstallConfirmation,
        generation: u32,
    ) -> Result<(), SupplicantError> {
        if self.phase != SupplicantPhase::KeysPending {
            return Err(SupplicantError::KeysNotPending);
        }
        let group_key_index = self
            .key_install_view()?
            .group_key()
            .map(|group| group.key_id);
        if !tracker.confirms_keys_for_association(
            generation,
            self.bssid,
            WPA2_PAIRWISE_KEY_INDEX,
            group_key_index,
        ) {
            return Err(SupplicantError::KeyInstallNotConfirmed);
        }
        self.confirm_keys_installed()
    }

    pub(crate) fn confirm_keys_installed(&mut self) -> Result<(), SupplicantError> {
        if self.phase != SupplicantPhase::KeysPending {
            return Err(SupplicantError::KeysNotPending);
        }
        if self.keys_installed {
            return Err(SupplicantError::KeysAlreadyConfirmed);
        }
        self.keys_installed = true;
        self.update_authorization();
        Ok(())
    }

    /// Call only after the frame transport accepts the encoded M4. A TX error
    /// leaves authorization closed and allows the same M4 to be re-encoded.
    pub fn confirm_message4_submitted(&mut self) -> Result<(), SupplicantError> {
        if self.phase != SupplicantPhase::KeysPending {
            if self.phase == SupplicantPhase::Established && self.message4_submitted {
                return Ok(());
            }
            return Err(SupplicantError::Message4NotReady);
        }
        if !self.message4_encoded {
            return Err(SupplicantError::Message4NotSubmitted);
        }
        self.message4_submitted = true;
        self.update_authorization();
        Ok(())
    }

    /// Encrypt station-to-AP data with the installed pairwise temporal key.
    /// Linux supplicant policy disables GTK transmit use when a pairwise key is
    /// active, so station-originated multicast also uses the PTK here.
    pub fn encrypt_data_frame(
        &mut self,
        frame: &[u8],
        output: &mut [u8],
    ) -> Result<usize, SupplicantError> {
        if !self.is_authorized() {
            return Err(SupplicantError::ProtectedDataNotAuthorized);
        }
        validate_station_tx_frame(frame, self.station, self.bssid)?;
        let pairwise = self
            .pairwise_keys
            .as_ref()
            .ok_or(SupplicantError::ProtectedDataNotAuthorized)?;
        Ok(encrypt_frame(
            pairwise.temporal_key(),
            0,
            &mut self.pairwise_tx_pn,
            frame,
            output,
        )?)
    }

    /// Encapsulate and protect one sDDF-style Ethernet-II frame. The WPA2
    /// controlled-port gate is checked before copying the packet into scratch;
    /// scratch is cleared after every attempted encapsulation/encryption.
    /// EAPOL handshake frames use the dedicated message methods instead.
    pub fn encrypt_ethernet_frame(
        &mut self,
        ethernet: &[u8],
        sequence: u16,
        native_scratch: &mut [u8],
        output: &mut [u8],
    ) -> Result<usize, SupplicantError> {
        if !self.is_authorized() {
            return Err(SupplicantError::ProtectedDataNotAuthorized);
        }
        let frame_bytes = match crate::ieee80211_ethernet::encapsulate_ethernet_ii(
            ethernet,
            self.station,
            self.bssid,
            sequence,
            native_scratch,
        ) {
            Ok(frame_bytes) => frame_bytes,
            Err(error) => {
                native_scratch.zeroize();
                return Err(SupplicantError::EthernetFrame(error));
            }
        };
        let result = self.encrypt_data_frame(&native_scratch[..frame_bytes], output);
        native_scratch.zeroize();
        result
    }

    /// Authenticate and decrypt AP-to-station data. Unicast frames use the
    /// PTK; multicast frames use the installed GTK and its M3 RSC replay base.
    pub fn decrypt_data_frame(
        &mut self,
        frame: &[u8],
        output: &mut [u8],
    ) -> Result<usize, SupplicantError> {
        if !self.is_authorized() {
            return Err(SupplicantError::ProtectedDataNotAuthorized);
        }
        let tid = validate_station_rx_frame(frame, self.station, self.bssid)?;
        let group = frame[4] & 1 != 0;
        if group {
            let pending = self
                .pending_keys
                .as_ref()
                .ok_or(SupplicantError::GroupKeyUnavailable)?;
            let gtk = pending
                .group_key
                .as_ref()
                .ok_or(SupplicantError::GroupKeyUnavailable)?;
            Ok(decrypt_frame(
                gtk.temporal_key(),
                gtk.key_id(),
                &mut self.group_rx_pn[tid],
                frame,
                output,
            )?)
        } else {
            let pairwise = self
                .pairwise_keys
                .as_ref()
                .ok_or(SupplicantError::ProtectedDataNotAuthorized)?;
            Ok(decrypt_frame(
                pairwise.temporal_key(),
                0,
                &mut self.pairwise_rx_pn[tid],
                frame,
                output,
            )?)
        }
    }

    fn update_authorization(&mut self) {
        let secure = self
            .pending_keys
            .as_ref()
            .is_some_and(|pending| pending.secure);
        if secure && self.keys_installed && self.message4_submitted {
            self.phase = SupplicantPhase::Established;
        }
    }

    fn fail(&mut self) {
        self.pairwise_keys = None;
        self.pending_keys = None;
        self.phase = SupplicantPhase::Failed;
    }
}

impl Drop for Wpa2Supplicant<'_> {
    fn drop(&mut self) {
        self.message1.zeroize();
        self.message3.zeroize();
    }
}

pub struct KeyInstallView<'a> {
    pairwise: &'a PairwiseKeys,
    pending: &'a PendingKeys,
}

impl<'a> KeyInstallView<'a> {
    pub fn pairwise_temporal_key(&self) -> &'a [u8; 16] {
        self.pairwise.temporal_key()
    }

    pub fn receive_sequence_counter(&self) -> &'a [u8; 8] {
        &self.pending.rsc
    }

    pub fn group_key(&self) -> Option<GroupKeyInstallView<'a>> {
        self.pending
            .group_key
            .as_ref()
            .map(|key| GroupKeyInstallView {
                key_id: key.key_id(),
                transmit: key.transmit(),
                temporal_key: key.temporal_key(),
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupKeyInstallView<'a> {
    pub key_id: u8,
    pub transmit: bool,
    pub temporal_key: &'a [u8; 16],
}

/// Extract EAPOL from one unprotected AP-to-station 802.11 SNAP frame.
pub fn extract_eapol_frame(
    frame: &[u8],
    station: [u8; 6],
    bssid: [u8; 6],
) -> Result<&[u8], SupplicantError> {
    let view = inspect_frame(frame).map_err(SupplicantError::InvalidFrameHeader)?;
    if view.kind != FrameKind::Data || view.frame_control & FC_PROTECTED != 0 {
        return Err(SupplicantError::InvalidEapolFrame);
    }
    if view.frame_control & FC_DS_MASK != FC_FROM_DS {
        return Err(SupplicantError::InvalidDataDirection);
    }
    if view.header[4..10] != station
        || view.header[10..16] != bssid
        || view.header[16..22] != station
    {
        return Err(SupplicantError::InvalidDataPeer);
    }
    if view.body.len() < LLC_SNAP_BYTES.len()
        || view.body[..6] != LLC_SNAP_BYTES[..6]
        || view.body[6..8] != EAPOL_ETHERTYPE
    {
        return Err(SupplicantError::NotEapolKey);
    }
    let eapol = &view.body[LLC_SNAP_BYTES.len()..];
    if eapol.len() < 4 {
        return Err(SupplicantError::InvalidEapolFrame);
    }
    if eapol[1] != 3 {
        return Err(SupplicantError::NotEapolKey);
    }
    let declared = 4usize + usize::from(u16::from_be_bytes([eapol[2], eapol[3]]));
    if declared > eapol.len() || eapol[declared..].iter().any(|byte| *byte != 0) {
        return Err(SupplicantError::InvalidEapolFrame);
    }
    Ok(&eapol[..declared])
}

fn write_station_eapol_header(
    output: &mut [u8],
    station: [u8; 6],
    bssid: [u8; 6],
    sequence: u16,
) -> Result<(), SupplicantError> {
    if sequence > SEQUENCE_NUMBER_MAX {
        return Err(SupplicantError::InvalidSequenceNumber);
    }
    if output.len() < IEEE80211_HEADER_BYTES {
        return Err(SupplicantError::OutputTooSmall);
    }
    output[..IEEE80211_HEADER_BYTES].fill(0);
    output[..2].copy_from_slice(&(DATA_FRAME_CONTROL | FC_TO_DS).to_le_bytes());
    output[4..10].copy_from_slice(&bssid);
    output[10..16].copy_from_slice(&station);
    output[16..22].copy_from_slice(&bssid);
    output[22..24].copy_from_slice(&(sequence << 4).to_le_bytes());
    Ok(())
}

fn validate_station_tx_frame(
    frame: &[u8],
    station: [u8; 6],
    bssid: [u8; 6],
) -> Result<(), SupplicantError> {
    let view = inspect_frame(frame).map_err(SupplicantError::InvalidFrameHeader)?;
    if view.kind != FrameKind::Data || view.frame_control & FC_DS_MASK != FC_TO_DS {
        return Err(SupplicantError::InvalidDataDirection);
    }
    if view.frame_control & FC_PROTECTED != 0
        || view.header[4..10] != bssid
        || view.header[10..16] != station
    {
        return Err(SupplicantError::InvalidDataPeer);
    }
    Ok(())
}

fn validate_station_rx_frame(
    frame: &[u8],
    station: [u8; 6],
    bssid: [u8; 6],
) -> Result<usize, SupplicantError> {
    let view = inspect_frame(frame).map_err(SupplicantError::InvalidFrameHeader)?;
    if view.kind != FrameKind::Data || view.frame_control & FC_DS_MASK != FC_FROM_DS {
        return Err(SupplicantError::InvalidDataDirection);
    }
    if view.frame_control & FC_PROTECTED == 0 || view.header[10..16] != bssid {
        return Err(SupplicantError::InvalidDataPeer);
    }
    let group = view.header[4] & 1 != 0;
    if !group && view.header[4..10] != station {
        return Err(SupplicantError::InvalidDataPeer);
    }

    if view.frame_control & FC_QOS_SUBTYPE == 0 {
        return Ok(NON_QOS_QUEUE);
    }
    let four_address = view.frame_control & FC_DS_MASK == FC_DS_MASK;
    let qos_offset = 24 + if four_address { 6 } else { 0 };
    let tid = *view
        .header
        .get(qos_offset)
        .ok_or(SupplicantError::InvalidDataPeer)?
        & 0x0f;
    Ok(usize::from(tid))
}

fn decode_ccmp_rsc(rsc: &[u8; 8]) -> Result<u64, SupplicantError> {
    if rsc[6] != 0 || rsc[7] != 0 {
        return Err(SupplicantError::InvalidGroupRsc);
    }
    Ok(u64::from(rsc[0])
        | (u64::from(rsc[1]) << 8)
        | (u64::from(rsc[2]) << 16)
        | (u64::from(rsc[3]) << 24)
        | (u64::from(rsc[4]) << 32)
        | (u64::from(rsc[5]) << 40))
}

fn valid_unicast_address(address: &[u8; 6]) -> bool {
    address.iter().any(|byte| *byte != 0) && address[0] & 1 == 0
}

#[cfg(test)]
mod tests {
    use aes::cipher::{BlockCipherEncrypt, KeyInit};
    use aes::{Aes128, Block};
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    use super::{LLC_SNAP_BYTES, Message3Result, SupplicantError, SupplicantPhase, Wpa2Supplicant};
    use crate::ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use crate::ath10k_htt::{HTT_SECURITY_TYPE_AES_CCMP, HttSecurityIndication};
    use crate::ath10k_key::{
        CCMP128_KEY_BYTES, CcmpKeyInstall, CcmpKeyInstallTracker, KeyInstallAction, KeyKind,
        VDEV_INSTALL_KEY_FIXED_BYTES, queue_ccmp_install_key,
    };
    use crate::ath10k_wmi_tx::WmiCommandTxQueue;
    use crate::ieee80211_ccmp::{
        CcmpReplayWindow, CcmpTxPacketNumber, decrypt_frame, encrypt_frame,
    };
    use crate::ieee80211_wpa2::Pmk;
    use crate::sunfish_qcacld_key::{
        KeyKind as QcacldKeyKind, QcacldCcmpKeyInstallTracker, WMI_VDEV_INSTALL_KEY_BODY_BYTES,
        encode_ccmp_install_key,
    };
    use crate::sunfish_qcacld_key_join::next_wpa2_key_install_request_from_view;

    const STATION: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const BSSID: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
    const ANONCE: [u8; 32] = [0x22; 32];
    const SNONCE: [u8; 32] = [0x33; 32];
    const RSN_IE: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];
    const REPLAY_1: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const REPLAY_3: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 2];
    const EAPOL_MIC_START: usize = 81;
    const EAPOL_MIC_END: usize = 97;
    const EAPOL_KEY_DATA_LENGTH_END: usize = 97;
    const EAPOL_KEY_DATA_START: usize = 99;
    const EAPOL_FIXED_FRAME_BYTES: usize = 99;

    fn new_supplicant() -> Wpa2Supplicant<'static> {
        let pmk = Pmk::from_passphrase(b"correct horse battery", b"test-network").unwrap();
        Wpa2Supplicant::new(pmk, STATION, BSSID, SNONCE, &RSN_IE).unwrap()
    }

    fn complete_key_tracker(
        supplicant: &Wpa2Supplicant<'_>,
        generation: u32,
    ) -> CcmpKeyInstallTracker {
        let keys = supplicant.key_install_view().unwrap();
        let group = keys.group_key().unwrap();
        let mut tracker =
            CcmpKeyInstallTracker::new(generation, 1, 0x0042, BSSID, 0, Some(group.key_id))
                .unwrap();
        let endpoint = HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 256,
        };
        let policy = HtcServicePolicy {
            receive_allocation: 64,
            disable_credit_flow: false,
            credit_size: 8,
            initial_credits: 64,
        };
        let mut queue =
            WmiCommandTxQueue::<4>::new_wmi_control(endpoint, policy, false, 11).unwrap();
        let mut body = [0u8; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
        let mut frame = [0u8; 128];
        let pairwise = queue_ccmp_install_key(
            &mut queue,
            CcmpKeyInstall {
                vdev_id: 1,
                peer_address: BSSID,
                key_index: 0,
                kind: KeyKind::Pairwise,
                temporal_key: keys.pairwise_temporal_key(),
            },
            0x8000_3000,
            1,
            0x100,
            &mut body,
            &mut frame,
        )
        .unwrap();
        tracker.record_queued(generation, pairwise).unwrap();
        tracker.mark_published(generation, KeyKind::Pairwise, pairwise.transfer).unwrap();
        let group_install = queue_ccmp_install_key(
            &mut queue,
            CcmpKeyInstall {
                vdev_id: 1,
                peer_address: BSSID,
                key_index: group.key_id,
                kind: KeyKind::Group,
                temporal_key: group.temporal_key,
            },
            0x8000_4000,
            2,
            0x101,
            &mut body,
            &mut frame,
        )
        .unwrap();
        tracker.record_queued(generation, group_install).unwrap();
        tracker.mark_published(generation, KeyKind::Group, group_install.transfer).unwrap();
        assert_eq!(
            tracker.observe_security_indication(
                generation,
                HttSecurityIndication {
                    peer_id: 0x0042,
                    security_type: HTT_SECURITY_TYPE_AES_CCMP,
                    is_unicast: true,
                },
            ),
            KeyInstallAction::Installed {
                kind: KeyKind::Pairwise,
                all_required_installed: false,
            }
        );
        assert_eq!(
            tracker.observe_security_indication(
                generation,
                HttSecurityIndication {
                    peer_id: 0x0042,
                    security_type: HTT_SECURITY_TYPE_AES_CCMP,
                    is_unicast: false,
                },
            ),
            KeyInstallAction::Installed {
                kind: KeyKind::Group,
                all_required_installed: true,
            }
        );
        body.fill(0);
        frame.fill(0);
        tracker
    }

    fn message1() -> Vec<u8> {
        let mut frame = vec![0; EAPOL_FIXED_FRAME_BYTES];
        frame[..4].copy_from_slice(&[2, 3, 0, 95]);
        frame[4] = 2;
        frame[5..7].copy_from_slice(&0x008au16.to_be_bytes());
        frame[7..9].copy_from_slice(&16u16.to_be_bytes());
        frame[9..17].copy_from_slice(&REPLAY_1);
        frame[17..49].copy_from_slice(&ANONCE);
        frame
    }

    fn wrap_key_data(kek: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
        assert!(plaintext.len() >= 16 && plaintext.len() % 8 == 0);
        let cipher = Aes128::new_from_slice(kek).unwrap();
        let blocks = plaintext.len() / 8;
        let mut wrapped = vec![0; plaintext.len() + 8];
        wrapped[..8].fill(0xa6);
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
        wrapped
    }

    fn message3() -> Vec<u8> {
        let pmk = Pmk::from_passphrase(b"correct horse battery", b"test-network").unwrap();
        let keys = pmk
            .derive_ccmp128_ptk(BSSID, STATION, &ANONCE, &SNONCE)
            .unwrap();

        let mut plaintext = Vec::new();
        plaintext.extend_from_slice(&RSN_IE);
        plaintext.extend_from_slice(&[
            221, 22, 0x00, 0x0f, 0xac, 0x01, 0x06, 0x00, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
        ]);
        // RFC 3394 input is block-aligned. The RSN key-data parser accepts a
        // zero-length vendor padding element followed by zero padding bytes.
        plaintext.extend_from_slice(&[221, 0]);
        assert_eq!(plaintext.len() % 8, 0);
        let wrapped = wrap_key_data(keys.kek(), &plaintext);

        let frame_len = EAPOL_KEY_DATA_START + wrapped.len();
        let mut frame = vec![0; frame_len];
        frame[..4].copy_from_slice(&[2, 3, 0, (frame_len - 4) as u8]);
        frame[4] = 2;
        frame[5..7].copy_from_slice(&0x13cau16.to_be_bytes());
        frame[7..9].copy_from_slice(&16u16.to_be_bytes());
        frame[9..17].copy_from_slice(&REPLAY_3);
        frame[17..49].copy_from_slice(&ANONCE);
        frame[65] = 1; // GTK RSC = 1, little endian.
        frame[EAPOL_KEY_DATA_LENGTH_END..EAPOL_KEY_DATA_START]
            .copy_from_slice(&(wrapped.len() as u16).to_be_bytes());
        frame[EAPOL_KEY_DATA_START..].copy_from_slice(&wrapped);
        let mut mac = Hmac::<Sha1>::new_from_slice(keys.kck()).unwrap();
        mac.update(&frame[..EAPOL_MIC_START]);
        mac.update(&[0; 16]);
        mac.update(&frame[EAPOL_MIC_END..]);
        frame[EAPOL_MIC_START..EAPOL_MIC_END].copy_from_slice(&mac.finalize().into_bytes()[..16]);
        frame
    }

    fn from_ds_eapol(eapol: &[u8]) -> Vec<u8> {
        let mut frame = vec![0; 24 + LLC_SNAP_BYTES.len() + eapol.len()];
        frame[..2].copy_from_slice(&0x0208u16.to_le_bytes());
        frame[4..10].copy_from_slice(&STATION);
        frame[10..16].copy_from_slice(&BSSID);
        frame[16..22].copy_from_slice(&STATION);
        frame[24..32].copy_from_slice(&LLC_SNAP_BYTES);
        frame[32..].copy_from_slice(eapol);
        frame
    }

    fn from_ds_data(destination: [u8; 6], payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0; 24 + payload.len()];
        frame[..2].copy_from_slice(&0x0208u16.to_le_bytes());
        frame[4..10].copy_from_slice(&destination);
        frame[10..16].copy_from_slice(&BSSID);
        frame[16..22].copy_from_slice(&[0x02, 0, 0, 0, 0, 3]);
        frame[24..].copy_from_slice(payload);
        frame
    }

    fn ethernet_to_ap(destination: [u8; 6], payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0; 14 + payload.len()];
        frame[..6].copy_from_slice(&destination);
        frame[6..12].copy_from_slice(&STATION);
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14..].copy_from_slice(payload);
        frame
    }

    #[test]
    fn full_exchange_gates_data_until_key_install_and_m4_submission() {
        let mut supplicant = new_supplicant();
        assert_eq!(supplicant.phase(), SupplicantPhase::AwaitingMessage1);
        let data = [0x08, 0x01];
        assert_eq!(
            supplicant.encrypt_data_frame(&data, &mut [0; 64]),
            Err(SupplicantError::ProtectedDataNotAuthorized)
        );
        let network_packet = ethernet_to_ap(BSSID, b"closed port");
        let mut closed_scratch = [0xa5; 128];
        let mut closed_output = [0x5a; 128];
        assert_eq!(
            supplicant.encrypt_ethernet_frame(
                &network_packet,
                1,
                &mut closed_scratch,
                &mut closed_output,
            ),
            Err(SupplicantError::ProtectedDataNotAuthorized)
        );
        assert_eq!(closed_scratch, [0xa5; 128]);
        assert_eq!(closed_output, [0x5a; 128]);

        let m1 = message1();
        let mut m2_frame = [0; 700];
        let m2_len = supplicant
            .process_message1_frame(&from_ds_eapol(&m1), 7, &mut m2_frame)
            .unwrap();
        let m2 = &m2_frame[24 + LLC_SNAP_BYTES.len()..m2_len];
        let parsed_m2 = crate::ieee80211_eapol::EapolKey::parse(m2).unwrap();
        assert_eq!(parsed_m2.replay_counter(), &REPLAY_1);
        assert_eq!(supplicant.phase(), SupplicantPhase::AwaitingMessage3);

        let m3 = message3();
        let mut unwrapped = [0xa5; 400];
        assert_eq!(
            supplicant
                .process_message3_frame(&from_ds_eapol(&m3), &mut unwrapped)
                .unwrap(),
            Message3Result::Accepted
        );
        assert_eq!(supplicant.phase(), SupplicantPhase::KeysPending);
        assert_eq!(&unwrapped[..48], &[0; 48]);
        assert_eq!(
            supplicant.process_message3(&m3, &mut unwrapped).unwrap(),
            Message3Result::Duplicate
        );

        {
            let keys = supplicant.key_install_view().unwrap();
            assert_eq!(keys.receive_sequence_counter(), &[1, 0, 0, 0, 0, 0, 0, 0]);
            let gtk = keys.group_key().unwrap();
            assert_eq!(gtk.key_id, 2);
            assert!(gtk.transmit);
            assert_eq!(gtk.temporal_key, &[0x77; 16]);
        }

        let mut m4_frame = [0; 256];
        let m4_len = supplicant.encode_message4_frame(8, &mut m4_frame).unwrap();
        let m4 = &m4_frame[24 + LLC_SNAP_BYTES.len()..m4_len];
        assert_eq!(
            crate::ieee80211_eapol::EapolKey::parse(m4)
                .unwrap()
                .replay_counter(),
            &REPLAY_3
        );
        supplicant.confirm_message4_submitted().unwrap();
        assert!(!supplicant.is_authorized());
        let tracker = complete_key_tracker(&supplicant, 12);
        assert_eq!(
            supplicant.confirm_keys_installed_from_tracker(&tracker, 13),
            Err(SupplicantError::KeyInstallNotConfirmed)
        );
        assert!(!supplicant.is_authorized());
        supplicant
            .confirm_keys_installed_from_tracker(&tracker, 12)
            .unwrap();
        assert!(supplicant.is_authorized());
        assert_eq!(supplicant.phase(), SupplicantPhase::Established);

        let pairwise_key = *supplicant
            .key_install_view()
            .unwrap()
            .pairwise_temporal_key();
        let network_packet = ethernet_to_ap([0x02, 0, 0, 0, 0, 3], b"authorized station data");
        let mut native_scratch = [0xa5; 128];
        let mut short_output = [0x5a; 24 + 8 + 23 + 15];
        assert_eq!(
            supplicant.encrypt_ethernet_frame(
                &network_packet,
                0x123,
                &mut native_scratch,
                &mut short_output,
            ),
            Err(SupplicantError::Ccmp(
                crate::ieee80211_ccmp::CcmpError::OutputTooSmall
            ))
        );
        assert_eq!(native_scratch, [0; 128]);
        assert_eq!(short_output, [0x5a; 24 + 8 + 23 + 15]);
        assert_eq!(supplicant.pairwise_tx_pn.last_used(), 0);

        let mut protected_packet = [0x5a; 128];
        let protected_bytes = supplicant
            .encrypt_ethernet_frame(
                &network_packet,
                0x123,
                &mut native_scratch,
                &mut protected_packet,
            )
            .unwrap();
        assert_eq!(native_scratch, [0; 128]);
        assert_ne!(protected_packet[1] & 0x40, 0);
        assert_eq!(supplicant.pairwise_tx_pn.last_used(), 1);

        let mut expected_clear = [0; 128];
        let expected_bytes = crate::ieee80211_ethernet::encapsulate_ethernet_ii(
            &network_packet,
            STATION,
            BSSID,
            0x123,
            &mut expected_clear,
        )
        .unwrap();
        let mut tx_replay = CcmpReplayWindow::new(0).unwrap();
        let mut decrypted_network = [0; 128];
        let decrypted_bytes = decrypt_frame(
            &pairwise_key,
            0,
            &mut tx_replay,
            &protected_packet[..protected_bytes],
            &mut decrypted_network,
        )
        .unwrap();
        decrypted_network[1] &= !0x40;
        assert_eq!(
            &decrypted_network[..decrypted_bytes],
            &expected_clear[..expected_bytes]
        );

        let clear_unicast = from_ds_data(STATION, b"AP unicast");
        let mut protected_unicast = vec![0; clear_unicast.len() + 16];
        let mut ap_pairwise_pn = CcmpTxPacketNumber::new(0).unwrap();
        let protected_len = encrypt_frame(
            &pairwise_key,
            0,
            &mut ap_pairwise_pn,
            &clear_unicast,
            &mut protected_unicast,
        )
        .unwrap();
        let mut decrypted = vec![0; protected_len];
        let decrypted_len = supplicant
            .decrypt_data_frame(&protected_unicast[..protected_len], &mut decrypted)
            .unwrap();
        decrypted[1] &= !0x40; // CCMP decrypt preserves the 802.11 Protected bit.
        assert_eq!(&decrypted[..decrypted_len], &clear_unicast);

        let gtk = [0x77; 16];
        let clear_group = from_ds_data([0x01, 0, 0x5e, 0, 0, 1], b"AP group");
        let mut protected_group = vec![0; clear_group.len() + 16];
        let mut ap_group_pn = CcmpTxPacketNumber::new(1).unwrap();
        let protected_len = encrypt_frame(
            &gtk,
            2,
            &mut ap_group_pn,
            &clear_group,
            &mut protected_group,
        )
        .unwrap();
        let decrypted_len = supplicant
            .decrypt_data_frame(&protected_group[..protected_len], &mut decrypted)
            .unwrap();
        decrypted[1] &= !0x40;
        assert_eq!(&decrypted[..decrypted_len], &clear_group);
    }

    #[test]
    fn qcacld_key_selection_emits_pairwise_then_group_with_message3_rsc() {
        let mut supplicant = new_supplicant();
        let message1 = message1();
        let mut message2 = [0; 700];
        supplicant
            .process_message1_frame(&from_ds_eapol(&message1), 7, &mut message2)
            .unwrap();
        let mut unwrapped = [0xa5; 400];
        assert_eq!(
            supplicant
                .process_message3_frame(&from_ds_eapol(&message3()), &mut unwrapped)
                .unwrap(),
            Message3Result::Accepted
        );

        let mut tracker = QcacldCcmpKeyInstallTracker::new(12, 1, 0x42, BSSID, 0, Some(2)).unwrap();
        let pairwise = next_wpa2_key_install_request_from_view(
            supplicant.key_install_view().unwrap(),
            12,
            BSSID,
            &tracker,
        )
        .unwrap()
        .unwrap();
        assert_eq!(pairwise.kind, QcacldKeyKind::Pairwise);
        assert_eq!(pairwise.key_index, 0);
        assert_eq!(
            pairwise.temporal_key,
            supplicant
                .key_install_view()
                .unwrap()
                .pairwise_temporal_key()
        );
        assert_eq!(pairwise.group_receive_sequence_counter, &[0; 8]);

        let mut body = [0; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
        let pairwise_layout = encode_ccmp_install_key(pairwise, &mut body).unwrap();
        tracker.record_queued(12, pairwise_layout).unwrap();
        body.fill(0);

        let group = next_wpa2_key_install_request_from_view(
            supplicant.key_install_view().unwrap(),
            12,
            BSSID,
            &tracker,
        )
        .unwrap()
        .unwrap();
        assert_eq!(group.kind, QcacldKeyKind::Group);
        assert_eq!(group.key_index, 2);
        assert_eq!(group.temporal_key, &[0x77; 16]);
        assert_eq!(
            group.group_receive_sequence_counter,
            &[1, 0, 0, 0, 0, 0, 0, 0]
        );

        let group_layout = encode_ccmp_install_key(group, &mut body).unwrap();
        tracker.record_queued(12, group_layout).unwrap();
        body.fill(0);
        assert_eq!(
            next_wpa2_key_install_request_from_view(
                supplicant.key_install_view().unwrap(),
                12,
                BSSID,
                &tracker,
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn authorized_ethernet_ccmp_frame_composes_into_qcacld_ce4_plan() {
        use crate::qcom_ce_v3::RingCursor;
        use crate::qcom_ce_v3_native_io::TxPublishError;
        use crate::qcom_ce_v3_profile::{Profile, plan_rings};
        use crate::qcom_htc::HtcReady;
        use crate::qcom_wmi_ce::{Direction, DmaPool, ServiceCeMapping, TxBufferWindow};
        use crate::sunfish_qcacld_ce_tx::{CE4_DATA_DESCRIPTOR_COUNT, CeTxBuffers, CeTxPlan};
        use crate::sunfish_qcacld_htc::{HTT_DATA_SERVICE_ID, parse_connect_response};
        use crate::sunfish_qcacld_tx::TxMetadata;
        use crate::sunfish_wlan_net_source::{self, EthernetFrameSource};
        use crate::sunfish_wlan_tx_pipeline::{
            Error as PipelineError, MAX_PROTECTED_NATIVE_FRAME_BYTES,
            MAX_UNPROTECTED_NATIVE_FRAME_BYTES, QcacldTxPublisher, SDDF_NET_BUFFER_BYTES,
            StagingBuffers, TxPlanContext, plan_next_sddf_ethernet_tx,
            publish_next_sddf_ethernet_tx,
        };

        const POOL_BASE: u64 = 0x8000_0000;
        const POOL_BYTES: u64 = 0x0100_0000;

        struct EthernetQueueFixture {
            frame: Vec<u8>,
            delivered: bool,
        }

        #[derive(Default)]
        struct RecordingPublisher {
            plan: Option<CeTxPlan>,
            frame: Vec<u8>,
        }

        impl QcacldTxPublisher for RecordingPublisher {
            fn publish_qcacld_data_tx(
                &mut self,
                _generation: u64,
                _profile: Profile,
                _rings: &crate::qcom_ce_v3_profile::RingPlan,
                plan: CeTxPlan,
                frame: &[u8],
                cursor: &mut RingCursor,
            ) -> Result<(), TxPublishError> {
                let next = cursor
                    .commit_publish_batch(plan.first_ticket(), CE4_DATA_DESCRIPTOR_COUNT)
                    .map_err(TxPublishError::Cursor)?;
                if next != plan.next_producer_index() {
                    return Err(TxPublishError::InvalidPlan);
                }
                self.frame.extend_from_slice(frame);
                self.plan = Some(plan);
                Ok(())
            }
        }

        impl EthernetFrameSource for EthernetQueueFixture {
            fn next_frame(
                &mut self,
                output: &mut [u8],
            ) -> Result<Option<usize>, sunfish_wlan_net_source::Error> {
                if self.delivered {
                    return Ok(None);
                }
                if self.frame.len() > output.len() {
                    return Err(sunfish_wlan_net_source::Error::Bridge(7));
                }
                output[..self.frame.len()].copy_from_slice(&self.frame);
                self.delivered = true;
                Ok(Some(self.frame.len()))
            }
        }

        let mut supplicant = new_supplicant();
        let mut message2 = [0; 256];
        supplicant
            .process_message1(&message1(), &mut message2)
            .unwrap();
        let mut unwrapped = [0; 256];
        supplicant
            .process_message3(&message3(), &mut unwrapped)
            .unwrap();
        let mut message4 = [0; 256];
        supplicant.encode_message4_frame(1, &mut message4).unwrap();
        supplicant.confirm_message4_submitted().unwrap();
        let tracker = complete_key_tracker(&supplicant, 22);
        supplicant
            .confirm_keys_installed_from_tracker(&tracker, 22)
            .unwrap();
        assert!(supplicant.is_authorized());

        let network_packet = ethernet_to_ap(BSSID, b"authorized CE4 packet");
        let dma_pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
        let rings = plan_rings(Profile::Mission, POOL_BASE, POOL_BYTES).unwrap();
        let data_base = POOL_BASE + rings.total_bytes().div_ceil(4096) * 4096 + 4096;
        let buffers = CeTxBuffers {
            htc_htt_prefix: TxBufferWindow {
                iova: data_base,
                capacity: 64,
            },
        };
        let metadata = TxMetadata {
            fragment_table_iova: data_base + 0x1000,
            frame_iova: data_base + 0x2000,
            msdu_id: 3,
            vdev_id: 1,
            ext_tid: 0,
            hardware_encrypt: false,
            request_completion: false,
        };
        let mappings = [
            ServiceCeMapping {
                service_id: HTT_DATA_SERVICE_ID,
                direction: Direction::HostToTarget,
                engine: 4,
            },
            ServiceCeMapping {
                service_id: HTT_DATA_SERVICE_ID,
                direction: Direction::TargetToHost,
                engine: 1,
            },
        ];
        let endpoint_id = 5u8;
        let max_message_size = 512u16;
        let encoded_endpoint = (u32::from(endpoint_id) << 8) | (u32::from(max_message_size) << 16);
        let connect_response = [
            3,
            0,
            0,
            3,
            encoded_endpoint as u8,
            (encoded_endpoint >> 8) as u8,
            (encoded_endpoint >> 16) as u8,
            (encoded_endpoint >> 24) as u8,
            0,
            0,
            0,
            0,
        ];
        let endpoint = parse_connect_response(
            &connect_response,
            HtcReady {
                credit_count: 4,
                credit_size: 64,
                max_endpoints: 9,
                htc_version: 1,
                max_messages_per_bundle: 1,
                alternate_data_credit_size: 0,
            },
        )
        .unwrap();

        let context = TxPlanContext {
            mappings: &mappings,
            endpoint,
            frame_sequence: 0x321,
            htc_sequence: 0x7a,
            metadata,
            buffers,
            dma_pool,
            profile: Profile::Mission,
            rings: &rings,
        };
        let mut cursor = RingCursor::new(2048).unwrap();
        let wrong_route = [
            ServiceCeMapping {
                engine: 3,
                ..mappings[0]
            },
            mappings[1],
        ];
        let mut queue_fixture = EthernetQueueFixture {
            frame: network_packet,
            delivered: false,
        };
        let mut ethernet_scratch = [0; SDDF_NET_BUFFER_BYTES];
        let mut native_scratch = [0xa5; MAX_UNPROTECTED_NATIVE_FRAME_BYTES];
        let mut protected_frame = [0; MAX_PROTECTED_NATIVE_FRAME_BYTES];

        let mut unauthorized = new_supplicant();
        assert_eq!(
            plan_next_sddf_ethernet_tx(
                &mut queue_fixture,
                &mut unauthorized,
                StagingBuffers {
                    ethernet: &mut ethernet_scratch,
                    native: &mut native_scratch,
                    protected: &mut protected_frame,
                },
                context,
                cursor,
            ),
            Err(PipelineError::NotAuthorized)
        );
        assert!(!queue_fixture.delivered);

        // Static CE validation must fail before the shared queue callback can
        // retire the packet or the supplicant can advance its CCMP PN.
        let invalid_context = TxPlanContext {
            mappings: &wrong_route,
            ..context
        };
        assert_eq!(
            plan_next_sddf_ethernet_tx(
                &mut queue_fixture,
                &mut supplicant,
                StagingBuffers {
                    ethernet: &mut ethernet_scratch,
                    native: &mut native_scratch,
                    protected: &mut protected_frame,
                },
                invalid_context,
                cursor,
            ),
            Err(PipelineError::Plan(
                crate::sunfish_qcacld_ce_tx::Error::WrongRoute
            ))
        );
        assert!(!queue_fixture.delivered);

        let mut publisher = RecordingPublisher::default();
        assert_eq!(
            publish_next_sddf_ethernet_tx(
                &mut publisher,
                17,
                &mut queue_fixture,
                &mut supplicant,
                StagingBuffers {
                    ethernet: &mut ethernet_scratch,
                    native: &mut native_scratch,
                    protected: &mut protected_frame,
                },
                context,
                &mut cursor,
            ),
            Ok(true)
        );
        let plan = publisher.plan.unwrap();
        let protected_bytes = publisher.frame.len();
        assert_eq!(publisher.frame, protected_frame[..protected_bytes]);
        assert_eq!(cursor.pending(), CE4_DATA_DESCRIPTOR_COUNT);
        assert!(queue_fixture.delivered);
        assert!(native_scratch.iter().all(|byte| *byte == 0));
        assert_ne!(protected_frame[1] & 0x40, 0);
        assert_eq!(protected_frame[24], 1, "preflight must not consume CCMP PN");

        assert_eq!(
            plan_next_sddf_ethernet_tx(
                &mut queue_fixture,
                &mut supplicant,
                StagingBuffers {
                    ethernet: &mut ethernet_scratch,
                    native: &mut native_scratch,
                    protected: &mut protected_frame,
                },
                context,
                cursor,
            )
            .unwrap(),
            None,
            "an empty sDDF queue does not produce another CE plan"
        );

        assert_eq!(plan.frame_bytes(), protected_bytes as u16);
        assert_eq!(plan.endpoint_id(), endpoint_id);
        assert_eq!(plan.route().host_to_target_engine, 4);
        assert_eq!(plan.route().target_to_host_engine, 1);
        assert_eq!(plan.second_descriptor().address(), metadata.frame_iova);
        assert_ne!(plan.second_descriptor().nbytes(), 0);
        let htt_flags = u32::from_le_bytes(plan.htc_htt_prefix()[8..12].try_into().unwrap());
        assert_ne!(
            htt_flags & (1 << 10),
            0,
            "firmware must not encrypt CCMP again"
        );
        assert_eq!(
            u32::from_le_bytes(plan.fragment_descriptor()[24..28].try_into().unwrap()),
            metadata.frame_iova as u32
        );
    }

    #[test]
    fn small_unwrap_buffer_does_not_consume_message3_replay_state() {
        let mut supplicant = new_supplicant();
        let mut m2 = [0; 256];
        let m1 = message1();
        supplicant.process_message1(&m1, &mut m2).unwrap();
        let m3 = message3();
        assert_eq!(
            supplicant.process_message3(&m3, &mut [0; 1]),
            Err(SupplicantError::Eapol(
                crate::ieee80211_eapol::EapolError::OutputTooSmall
            ))
        );
        assert_eq!(supplicant.phase(), SupplicantPhase::AwaitingMessage3);
        assert_eq!(
            supplicant.process_message3(&m3, &mut [0; 400]),
            Ok(Message3Result::Accepted)
        );
    }
}
