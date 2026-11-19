//! Bounded WPA2 station authentication, association, and key-handshake flow.
//!
//! This joins the existing open-system management codecs to the WPA2
//! supplicant transaction. The caller still selects a BSS and legal channel,
//! schedules/transmits Native-WiFi frames, reports successful key installation,
//! and confirms transport submission. This module does not scan or touch radio
//! hardware.
#![forbid(unsafe_code)]

use zeroize::Zeroizing;

use crate::ieee80211_bss::SelectedBss;
use crate::ieee80211_station::{
    AssociationRequest, StationFrameError, StationIdentity, encode_association_request,
    encode_open_authentication_request, parse_association_response,
    parse_open_authentication_response,
};
use crate::ieee80211_supplicant::{
    KeyInstallView, Message3Result, SupplicantError, SupplicantPhase, Wpa2Supplicant,
};
use crate::ieee80211_wpa2::{Pmk, Wpa2Error, validate_rsn_ie};
use crate::wlan_key_confirmation::KeyInstallConfirmation;

const MAX_SEQUENCE_NUMBER: u16 = 0x0fff;
const MAX_ASSOCIATION_FRAME_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinError {
    InvalidSsid,
    InvalidPassphrase,
    InvalidSNonce,
    InvalidRsnInformationElement,
    InvalidInitialSequence,
    InvalidAssociationGeneration,
    InvalidSelectedBss,
    UnexpectedPhase,
    InvalidResponseTimeout,
    ResponseDeadlineOverflow,
    ProtectedDataNotAuthorized,
    KeyInstallNotConfirmed,
    AuthenticationRejected(u16),
    AssociationRejected(u16),
    Station(StationFrameError),
    Supplicant(SupplicantError),
    Wpa2(Wpa2Error),
}

impl From<StationFrameError> for JoinError {
    fn from(value: StationFrameError) -> Self {
        Self::Station(value)
    }
}

impl From<SupplicantError> for JoinError {
    fn from(value: SupplicantError) -> Self {
        Self::Supplicant(value)
    }
}

impl From<Wpa2Error> for JoinError {
    fn from(value: Wpa2Error) -> Self {
        Self::Wpa2(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinPhase {
    Ready,
    AwaitingAuthenticationResponse,
    AwaitingAssociationResponse,
    AwaitingMessage1,
    AwaitingMessage3,
    KeysPending,
    Established,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResponseDeadline {
    phase: JoinPhase,
    at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinTimeoutAction {
    None,
    TimedOut { phase: JoinPhase },
}

/// Configuration for one already-selected WPA2-Personal BSS.
pub struct JoinConfig<'a> {
    /// Owner generation for rejecting late key events from an earlier attempt.
    pub association_generation: u32,
    pub station: [u8; 6],
    pub bssid: [u8; 6],
    pub initial_sequence_number: u16,
    pub ssid: &'a [u8],
    pub passphrase: &'a [u8],
    /// Complete RSN IE, including ID and length bytes.
    pub rsn_ie: &'a [u8],
    /// Supported Rates IE value selected from the BSS.
    pub supported_rates: &'a [u8],
    /// Optional Extended Supported Rates IE value selected from the BSS.
    pub extended_supported_rates: &'a [u8],
    /// Other already-ordered association IEs, excluding SSID/rates/RSN.
    pub additional_ies: &'a [u8],
    pub capability_info: u16,
    pub listen_interval: u16,
    /// Fresh entropy from the platform RNG, retained only until M1 derives PTK.
    pub snonce: [u8; 32],
}

/// Station-owned settings applied after a compatible BSS has been selected.
pub struct JoinSettings<'a> {
    /// Fresh owner generation for this association attempt; zero is reserved.
    pub association_generation: u32,
    pub station: [u8; 6],
    pub initial_sequence_number: u16,
    pub passphrase: &'a [u8],
    /// Already-ordered association IEs excluding SSID, rates, and RSN.
    pub additional_ies: &'a [u8],
    /// The station's own capability bits, not the AP's advertised bits.
    pub capability_info: u16,
    pub listen_interval: u16,
    /// Fresh platform RNG output for this association attempt.
    pub snonce: [u8; 32],
}

impl<'a> JoinConfig<'a> {
    /// Bind station settings to the BSS selector's retained advertisement.
    pub fn from_selected_bss(
        selected: SelectedBss<'a>,
        settings: JoinSettings<'a>,
    ) -> Result<Self, JoinError> {
        let ssid = selected
            .advertisement
            .ssid
            .filter(|ssid| !ssid.is_empty())
            .ok_or(JoinError::InvalidSelectedBss)?;
        let rsn_ie = selected
            .advertisement
            .rsn_ie
            .ok_or(JoinError::InvalidSelectedBss)?;
        let supported_rates = selected
            .advertisement
            .supported_rates
            .ok_or(JoinError::InvalidSelectedBss)?;

        Ok(Self {
            association_generation: settings.association_generation,
            station: settings.station,
            bssid: selected.advertisement.bssid,
            initial_sequence_number: settings.initial_sequence_number,
            ssid,
            passphrase: settings.passphrase,
            rsn_ie,
            supported_rates,
            extended_supported_rates: selected.advertisement.extended_rates.unwrap_or(&[]),
            additional_ies: settings.additional_ies,
            capability_info: settings.capability_info,
            listen_interval: settings.listen_interval,
            snonce: settings.snonce,
        })
    }
}

/// The selected BSS through WPA2 key authorization, before any radio driver.
pub struct Wpa2Join<'a> {
    phase: JoinPhase,
    association_generation: u32,
    station: [u8; 6],
    bssid: [u8; 6],
    next_sequence_number: u16,
    ssid: &'a [u8],
    rsn_ie: &'a [u8],
    supported_rates: &'a [u8],
    extended_supported_rates: &'a [u8],
    additional_ies: &'a [u8],
    capability_info: u16,
    listen_interval: u16,
    pmk: Option<Pmk>,
    snonce: Option<Zeroizing<[u8; 32]>>,
    supplicant: Option<Wpa2Supplicant<'a>>,
    association_id: Option<u16>,
    response_deadline: Option<ResponseDeadline>,
}

impl<'a> Wpa2Join<'a> {
    /// Validate and prepare one WPA2-PSK/CCMP-128 association.
    ///
    /// The caller must have selected the BSS and filtered its channel through
    /// the platform's regulatory policy. No frame is produced until `begin`.
    pub fn new(config: JoinConfig<'a>) -> Result<Self, JoinError> {
        if config.association_generation == 0 {
            return Err(JoinError::InvalidAssociationGeneration);
        }
        if config.ssid.is_empty() || config.ssid.len() > 32 {
            return Err(JoinError::InvalidSsid);
        }
        if config.snonce.iter().all(|byte| *byte == 0) {
            return Err(JoinError::InvalidSNonce);
        }
        if config.initial_sequence_number > MAX_SEQUENCE_NUMBER {
            return Err(JoinError::InvalidInitialSequence);
        }
        if config.station == config.bssid {
            return Err(JoinError::Wpa2(Wpa2Error::InvalidPeerAddress));
        }
        validate_rsn_ie(config.rsn_ie).map_err(|error| match error {
            Wpa2Error::InvalidRsnInformationElement => JoinError::InvalidRsnInformationElement,
            other => JoinError::Wpa2(other),
        })?;
        let pmk =
            Pmk::from_passphrase(config.passphrase, config.ssid).map_err(|error| match error {
                Wpa2Error::InvalidPassphrase => JoinError::InvalidPassphrase,
                other => JoinError::Wpa2(other),
            })?;

        let join = Self {
            phase: JoinPhase::Ready,
            association_generation: config.association_generation,
            station: config.station,
            bssid: config.bssid,
            next_sequence_number: config.initial_sequence_number,
            ssid: config.ssid,
            rsn_ie: config.rsn_ie,
            supported_rates: config.supported_rates,
            extended_supported_rates: config.extended_supported_rates,
            additional_ies: config.additional_ies,
            capability_info: config.capability_info,
            listen_interval: config.listen_interval,
            pmk: Some(pmk),
            snonce: Some(Zeroizing::new(config.snonce)),
            supplicant: None,
            association_id: None,
            response_deadline: None,
        };

        // Exercise the complete association input validation before the first
        // authentication frame can be sent. No caller buffer is modified.
        let mut validation_output = [0; MAX_ASSOCIATION_FRAME_BYTES];
        join.encode_association(&mut validation_output)?;
        Ok(join)
    }

    pub const fn phase(&self) -> JoinPhase {
        self.phase
    }

    pub const fn association_id(&self) -> Option<u16> {
        self.association_id
    }

    pub const fn association_generation(&self) -> u32 {
        self.association_generation
    }

    pub const fn bssid(&self) -> [u8; 6] {
        self.bssid
    }

    pub const fn is_authorized(&self) -> bool {
        matches!(self.phase, JoinPhase::Established)
    }

    /// Revoke this association after the transport owner has verified loss
    /// of its final mapped peer. The owner must also stop and safely retire
    /// any packets already handed to the transport.
    pub fn peer_lost(&mut self, generation: u32, bssid: [u8; 6]) -> bool {
        if generation != self.association_generation
            || bssid != self.bssid
            || self.phase == JoinPhase::Failed
        {
            return false;
        }
        self.fail();
        true
    }

    /// Arm a caller-budgeted deadline for the current response phase.
    ///
    /// The timeout duration is policy input; this module does not assume a
    /// platform timer frequency or choose a retry policy. Call this only after
    /// the corresponding request has been accepted by the frame transport
    /// (or after association success, when waiting for EAPOL message 1). The
    /// owner must call `poll_response_timeout` before dispatching each incoming
    /// response so a late frame cannot revive an expired association.
    pub fn arm_response_timeout(&mut self, now_ms: u64, timeout_ms: u32) -> Result<(), JoinError> {
        if timeout_ms == 0
            || !matches!(
                self.phase,
                JoinPhase::AwaitingAuthenticationResponse
                    | JoinPhase::AwaitingAssociationResponse
                    | JoinPhase::AwaitingMessage1
                    | JoinPhase::AwaitingMessage3
            )
        {
            return Err(JoinError::InvalidResponseTimeout);
        }
        let at_ms = now_ms
            .checked_add(u64::from(timeout_ms))
            .ok_or(JoinError::ResponseDeadlineOverflow)?;
        self.response_deadline = Some(ResponseDeadline {
            phase: self.phase,
            at_ms,
        });
        Ok(())
    }

    /// Expire the current response wait and erase the one-shot join state.
    ///
    /// Returns the phase that timed out once. The transaction becomes terminal
    /// and rejects all later authentication, association, or EAPOL frames.
    /// Key installation in `KeysPending` has its own transport owner and is
    /// intentionally not covered by this response timer.
    pub fn poll_response_timeout(&mut self, now_ms: u64) -> JoinTimeoutAction {
        let Some(deadline) = self.response_deadline else {
            return JoinTimeoutAction::None;
        };
        if deadline.phase != self.phase || now_ms < deadline.at_ms {
            return JoinTimeoutAction::None;
        }
        self.fail();
        JoinTimeoutAction::TimedOut {
            phase: deadline.phase,
        }
    }

    /// Emit the open-system authentication request and reserve the response
    /// phase only after the output buffer is large enough.
    pub fn begin(&mut self, output: &mut [u8]) -> Result<usize, JoinError> {
        if self.phase != JoinPhase::Ready {
            return Err(JoinError::UnexpectedPhase);
        }
        let bytes =
            encode_open_authentication_request(self.identity(self.next_sequence_number), output)?
                .frame_bytes;
        self.advance_sequence();
        self.phase = JoinPhase::AwaitingAuthenticationResponse;
        Ok(bytes)
    }

    /// Validate the selected AP's auth response, then emit the association
    /// request. A rejected response terminates this one-shot join attempt.
    pub fn process_authentication_response(
        &mut self,
        frame: &[u8],
        output: &mut [u8],
    ) -> Result<usize, JoinError> {
        if self.phase != JoinPhase::AwaitingAuthenticationResponse {
            return Err(JoinError::UnexpectedPhase);
        }
        let response =
            parse_open_authentication_response(frame, self.identity(self.next_sequence_number))?;
        if response.status_code != 0 {
            self.fail();
            return Err(JoinError::AuthenticationRejected(response.status_code));
        }
        let bytes = self.encode_association(output)?;
        self.advance_sequence();
        self.clear_response_timeout();
        self.phase = JoinPhase::AwaitingAssociationResponse;
        Ok(bytes)
    }

    /// Validate association success and create the WPA2 supplicant transaction.
    pub fn process_association_response(&mut self, frame: &[u8]) -> Result<u16, JoinError> {
        if self.phase != JoinPhase::AwaitingAssociationResponse {
            return Err(JoinError::UnexpectedPhase);
        }
        let response = parse_association_response(frame, self.identity(self.next_sequence_number))?;
        if response.status_code != 0 {
            self.fail();
            return Err(JoinError::AssociationRejected(response.status_code));
        }
        let association_id = response.association_id.ok_or(JoinError::UnexpectedPhase)?;
        let pmk = self.pmk.take().ok_or(JoinError::UnexpectedPhase)?;
        let snonce = self.snonce.take().ok_or(JoinError::UnexpectedPhase)?;
        let supplicant =
            match Wpa2Supplicant::new(pmk, self.station, self.bssid, *snonce, self.rsn_ie) {
                Ok(supplicant) => supplicant,
                Err(error) => {
                    self.fail();
                    return Err(error.into());
                }
            };
        self.association_id = Some(association_id);
        self.supplicant = Some(supplicant);
        self.clear_response_timeout();
        self.phase = JoinPhase::AwaitingMessage1;
        Ok(association_id)
    }

    /// Accept EAPOL M1 and return a complete To-DS Native-WiFi M2 frame.
    pub fn process_message1_frame(
        &mut self,
        frame: &[u8],
        output: &mut [u8],
    ) -> Result<usize, JoinError> {
        if self.phase != JoinPhase::AwaitingMessage1 && self.phase != JoinPhase::AwaitingMessage3 {
            return Err(JoinError::UnexpectedPhase);
        }
        let supplicant = self.supplicant.as_mut().ok_or(JoinError::UnexpectedPhase)?;
        let bytes = supplicant.process_message1_frame(frame, self.next_sequence_number, output)?;
        self.advance_sequence();
        self.clear_response_timeout();
        self.phase = JoinPhase::AwaitingMessage3;
        Ok(bytes)
    }

    /// Accept EAPOL M3 after M1/M2, leaving the driver to install keys and
    /// submit the separately encoded M4.
    pub fn process_message3_frame(
        &mut self,
        frame: &[u8],
        unwrap_buffer: &mut [u8],
    ) -> Result<Message3Result, JoinError> {
        if self.phase == JoinPhase::KeysPending || self.phase == JoinPhase::Established {
            return Ok(self
                .supplicant
                .as_mut()
                .ok_or(JoinError::UnexpectedPhase)?
                .process_message3_frame(frame, unwrap_buffer)?);
        }
        if self.phase != JoinPhase::AwaitingMessage3 {
            return Err(JoinError::UnexpectedPhase);
        }
        let result = self
            .supplicant
            .as_mut()
            .ok_or(JoinError::UnexpectedPhase)?
            .process_message3_frame(frame, unwrap_buffer);
        match result {
            Ok(result) => {
                if result == Message3Result::Accepted {
                    self.clear_response_timeout();
                    self.phase = JoinPhase::KeysPending;
                }
                Ok(result)
            }
            Err(error) => {
                if self
                    .supplicant
                    .as_ref()
                    .is_some_and(|supplicant| supplicant.phase() == SupplicantPhase::Failed)
                {
                    self.fail();
                }
                Err(error.into())
            }
        }
    }

    pub fn encode_message4_frame(&mut self, output: &mut [u8]) -> Result<usize, JoinError> {
        if self.phase != JoinPhase::KeysPending && self.phase != JoinPhase::Established {
            return Err(JoinError::UnexpectedPhase);
        }
        let sequence = self.next_sequence_number;
        let bytes = self
            .supplicant
            .as_mut()
            .ok_or(JoinError::UnexpectedPhase)?
            .encode_message4_frame(sequence, output)?;
        self.advance_sequence();
        Ok(bytes)
    }

    pub fn key_install_view(&self) -> Result<KeyInstallView<'_>, JoinError> {
        if self.phase != JoinPhase::KeysPending && self.phase != JoinPhase::Established {
            return Err(JoinError::UnexpectedPhase);
        }
        Ok(self
            .supplicant
            .as_ref()
            .ok_or(JoinError::UnexpectedPhase)?
            .key_install_view()?)
    }

    pub fn confirm_keys_installed(
        &mut self,
        tracker: &impl KeyInstallConfirmation,
    ) -> Result<(), JoinError> {
        if self.phase != JoinPhase::KeysPending {
            return Err(JoinError::UnexpectedPhase);
        }
        let supplicant = self.supplicant.as_mut().ok_or(JoinError::UnexpectedPhase)?;
        supplicant
            .confirm_keys_installed_from_tracker(tracker, self.association_generation)
            .map_err(|error| match error {
                SupplicantError::KeyInstallNotConfirmed => JoinError::KeyInstallNotConfirmed,
                other => JoinError::Supplicant(other),
            })?;
        self.sync_phase();
        Ok(())
    }

    pub fn confirm_message4_submitted(&mut self) -> Result<(), JoinError> {
        if self.phase != JoinPhase::KeysPending && self.phase != JoinPhase::Established {
            return Err(JoinError::UnexpectedPhase);
        }
        let supplicant = self.supplicant.as_mut().ok_or(JoinError::UnexpectedPhase)?;
        supplicant.confirm_message4_submitted()?;
        self.sync_phase();
        Ok(())
    }

    pub fn encrypt_data_frame(
        &mut self,
        frame: &[u8],
        output: &mut [u8],
    ) -> Result<usize, JoinError> {
        if !self.is_authorized() {
            return Err(JoinError::ProtectedDataNotAuthorized);
        }
        Ok(self
            .supplicant
            .as_mut()
            .ok_or(JoinError::UnexpectedPhase)?
            .encrypt_data_frame(frame, output)?)
    }

    pub fn decrypt_data_frame(
        &mut self,
        frame: &[u8],
        output: &mut [u8],
    ) -> Result<usize, JoinError> {
        if !self.is_authorized() {
            return Err(JoinError::ProtectedDataNotAuthorized);
        }
        Ok(self
            .supplicant
            .as_mut()
            .ok_or(JoinError::UnexpectedPhase)?
            .decrypt_data_frame(frame, output)?)
    }

    fn encode_association(&self, output: &mut [u8]) -> Result<usize, JoinError> {
        let rsn_value = self
            .rsn_ie
            .get(2..)
            .ok_or(JoinError::InvalidRsnInformationElement)?;
        Ok(encode_association_request(
            AssociationRequest {
                identity: self.identity(self.next_sequence_number),
                capability_info: self.capability_info,
                listen_interval: self.listen_interval,
                ssid: self.ssid,
                supported_rates: self.supported_rates,
                extended_supported_rates: self.extended_supported_rates,
                rsn: Some(rsn_value),
                additional_ies: self.additional_ies,
            },
            output,
        )?
        .frame_bytes)
    }

    fn identity(&self, sequence_number: u16) -> StationIdentity {
        StationIdentity {
            station: self.station,
            bssid: self.bssid,
            sequence_number,
        }
    }

    fn advance_sequence(&mut self) {
        self.next_sequence_number = self.next_sequence_number.wrapping_add(1) & MAX_SEQUENCE_NUMBER;
    }

    fn sync_phase(&mut self) {
        if self
            .supplicant
            .as_ref()
            .is_some_and(Wpa2Supplicant::is_authorized)
        {
            self.phase = JoinPhase::Established;
        }
    }

    fn clear_response_timeout(&mut self) {
        self.response_deadline = None;
    }

    fn fail(&mut self) {
        self.pmk = None;
        self.snonce = None;
        self.supplicant = None;
        self.association_id = None;
        self.clear_response_timeout();
        self.phase = JoinPhase::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::{JoinConfig, JoinError, JoinPhase, JoinSettings, JoinTimeoutAction, Wpa2Join};
    use crate::ieee80211_bss::{BssSelectionPolicy, ScanCandidate, select_bss};

    const STATION: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const BSSID: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
    const RSN_IE: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];
    const RATES: [u8; 4] = [0x82, 0x84, 0x8b, 0x96];
    const SSID: &[u8] = b"test-network";

    fn config() -> JoinConfig<'static> {
        JoinConfig {
            association_generation: 1,
            station: STATION,
            bssid: BSSID,
            initial_sequence_number: 0x0fff,
            ssid: SSID,
            passphrase: b"correct horse battery",
            rsn_ie: &RSN_IE,
            supported_rates: &RATES,
            extended_supported_rates: &[],
            additional_ies: &[],
            capability_info: 0x0011,
            listen_interval: 10,
            snonce: [0x33; 32],
        }
    }

    fn join() -> Wpa2Join<'static> {
        Wpa2Join::new(config()).unwrap()
    }

    #[test]
    fn join_requires_a_nonzero_association_generation() {
        let mut config = config();
        config.association_generation = 0;
        assert!(matches!(
            Wpa2Join::new(config),
            Err(JoinError::InvalidAssociationGeneration)
        ));
        assert_eq!(join().association_generation(), 1);
    }

    #[test]
    fn response_deadline_expires_once_and_rejects_late_association_frames() {
        let mut join = join();
        let mut output = [0; 512];
        join.begin(&mut output).unwrap();
        join.arm_response_timeout(1_000, 10_000).unwrap();
        assert_eq!(join.poll_response_timeout(10_999), JoinTimeoutAction::None);
        assert_eq!(
            join.poll_response_timeout(11_000),
            JoinTimeoutAction::TimedOut {
                phase: JoinPhase::AwaitingAuthenticationResponse,
            }
        );
        assert_eq!(join.phase(), JoinPhase::Failed);
        assert!(join.pmk.is_none());
        assert!(join.snonce.is_none());
        assert!(join.supplicant.is_none());
        assert_eq!(join.poll_response_timeout(20_000), JoinTimeoutAction::None);
        assert_eq!(
            join.process_authentication_response(&authentication_response(0), &mut output),
            Err(JoinError::UnexpectedPhase)
        );
    }

    #[test]
    fn response_deadline_tracks_each_wait_phase_without_covering_key_install() {
        let mut join = join();
        let mut output = [0; 512];
        join.begin(&mut output).unwrap();
        join.arm_response_timeout(100, 1_000).unwrap();
        assert_eq!(join.poll_response_timeout(1_099), JoinTimeoutAction::None);
        join.process_authentication_response(&authentication_response(0), &mut output)
            .unwrap();
        assert_eq!(join.poll_response_timeout(5_000), JoinTimeoutAction::None);
        join.arm_response_timeout(5_000, 1_000).unwrap();
        assert_eq!(join.poll_response_timeout(5_999), JoinTimeoutAction::None);
        join.process_association_response(&association_response(0))
            .unwrap();
        assert_eq!(join.phase(), JoinPhase::AwaitingMessage1);
        assert_eq!(join.poll_response_timeout(10_000), JoinTimeoutAction::None);
        join.arm_response_timeout(10_000, 1_000).unwrap();
        assert_eq!(join.poll_response_timeout(10_999), JoinTimeoutAction::None);
        join.process_message1_frame(&message1_frame(), &mut output)
            .unwrap();
        assert_eq!(join.phase(), JoinPhase::AwaitingMessage3);
        assert_eq!(join.poll_response_timeout(20_000), JoinTimeoutAction::None);
        join.arm_response_timeout(20_000, 1_000).unwrap();
        assert_eq!(join.poll_response_timeout(20_999), JoinTimeoutAction::None);
        assert_eq!(
            join.poll_response_timeout(21_000),
            JoinTimeoutAction::TimedOut {
                phase: JoinPhase::AwaitingMessage3,
            }
        );
        assert_eq!(join.phase(), JoinPhase::Failed);
        assert_eq!(
            join.process_message3_frame(&[], &mut []),
            Err(JoinError::UnexpectedPhase)
        );

        let mut join = Wpa2Join::new(config()).unwrap();
        join.phase = JoinPhase::KeysPending;
        assert_eq!(
            join.arm_response_timeout(0, 1),
            Err(JoinError::InvalidResponseTimeout)
        );
        assert_eq!(
            join.poll_response_timeout(u64::MAX),
            JoinTimeoutAction::None
        );
    }

    #[test]
    fn response_deadline_rejects_zero_duration_and_overflow_without_replacing_old_deadline() {
        let mut join = join();
        let mut output = [0; 512];
        join.begin(&mut output).unwrap();
        join.arm_response_timeout(10, 100).unwrap();
        assert_eq!(
            join.arm_response_timeout(0, 0),
            Err(JoinError::InvalidResponseTimeout)
        );
        assert_eq!(
            join.arm_response_timeout(u64::MAX, 1),
            Err(JoinError::ResponseDeadlineOverflow)
        );
        assert_eq!(join.poll_response_timeout(109), JoinTimeoutAction::None);
        assert_eq!(
            join.poll_response_timeout(110),
            JoinTimeoutAction::TimedOut {
                phase: JoinPhase::AwaitingAuthenticationResponse,
            }
        );
    }

    fn authentication_response(status: u16) -> Vec<u8> {
        let mut frame = vec![0; 30];
        frame[..2].copy_from_slice(&0x00b0u16.to_le_bytes());
        frame[4..10].copy_from_slice(&STATION);
        frame[10..16].copy_from_slice(&BSSID);
        frame[16..22].copy_from_slice(&BSSID);
        frame[24..30].copy_from_slice(&[0, 0, 2, 0, status as u8, (status >> 8) as u8]);
        frame
    }

    fn association_response(status: u16) -> Vec<u8> {
        let mut frame = vec![0; 30];
        frame[..2].copy_from_slice(&0x0010u16.to_le_bytes());
        frame[4..10].copy_from_slice(&STATION);
        frame[10..16].copy_from_slice(&BSSID);
        frame[16..22].copy_from_slice(&BSSID);
        frame[24..30].copy_from_slice(&[0x11, 0, status as u8, (status >> 8) as u8, 1, 0xc0]);
        frame
    }

    fn message1_frame() -> Vec<u8> {
        let mut frame = vec![0; 24 + 8 + 99];
        frame[..2].copy_from_slice(&0x0208u16.to_le_bytes());
        frame[4..10].copy_from_slice(&STATION);
        frame[10..16].copy_from_slice(&BSSID);
        frame[16..22].copy_from_slice(&STATION);
        frame[24..32].copy_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0, 0x88, 0x8e]);
        frame[32..36].copy_from_slice(&[2, 3, 0, 95]);
        frame[36] = 2;
        frame[37..39].copy_from_slice(&0x008au16.to_be_bytes());
        frame[39..41].copy_from_slice(&16u16.to_be_bytes());
        frame[41..49].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        frame[49..81].fill(0x22);
        frame
    }

    #[test]
    fn selected_scan_result_builds_the_wpa2_join_configuration() {
        let mut frame = [0; 128];
        frame[..2].copy_from_slice(&0x0080u16.to_le_bytes());
        frame[16..22].copy_from_slice(&BSSID);
        frame[34..36].copy_from_slice(&0x0011u16.to_le_bytes());
        let mut offset = 36;
        frame[offset..offset + 2].copy_from_slice(&[0, SSID.len() as u8]);
        offset += 2;
        frame[offset..offset + SSID.len()].copy_from_slice(SSID);
        offset += SSID.len();
        frame[offset..offset + 2].copy_from_slice(&[1, RATES.len() as u8]);
        offset += 2;
        frame[offset..offset + RATES.len()].copy_from_slice(&RATES);
        offset += RATES.len();
        frame[offset..offset + 3].copy_from_slice(&[3, 1, 6]);
        offset += 3;
        frame[offset..offset + 2].copy_from_slice(&[50, 2]);
        offset += 2;
        frame[offset..offset + 2].copy_from_slice(&[0x0c, 0x12]);
        offset += 2;
        frame[offset..offset + RSN_IE.len()].copy_from_slice(&RSN_IE);
        offset += RSN_IE.len();

        let candidate = ScanCandidate {
            frame: &frame[..offset],
            rssi_dbm: -42,
            observed_channel: Some(6),
        };
        let selected = select_bss(
            &[candidate],
            BssSelectionPolicy {
                ssid: SSID,
                pinned_bssid: Some(BSSID),
                allowed_channels: &[6],
                minimum_rssi_dbm: -80,
            },
        )
        .unwrap();
        let config = JoinConfig::from_selected_bss(
            selected,
            JoinSettings {
                association_generation: 9,
                station: STATION,
                initial_sequence_number: 7,
                passphrase: b"correct horse battery",
                additional_ies: &[],
                capability_info: 0x0011,
                listen_interval: 20,
                snonce: [0x44; 32],
            },
        )
        .unwrap();

        assert_eq!(config.station, STATION);
        assert_eq!(config.association_generation, 9);
        assert_eq!(config.bssid, BSSID);
        assert_eq!(config.ssid, SSID);
        assert_eq!(config.rsn_ie, &RSN_IE);
        assert_eq!(config.supported_rates, &RATES);
        assert_eq!(config.extended_supported_rates, &[0x0c, 0x12]);
        assert_eq!(config.initial_sequence_number, 7);
        assert_eq!(config.capability_info, 0x0011);
        assert_eq!(config.listen_interval, 20);
        assert_eq!(config.snonce, [0x44; 32]);
        let join = Wpa2Join::new(config).unwrap();
        assert_eq!(join.phase(), JoinPhase::Ready);
        assert_eq!(join.association_generation(), 9);
    }

    #[test]
    fn peer_loss_is_generation_and_bssid_bound_and_clears_join_state() {
        let mut join = join();
        join.begin(&mut [0; 30]).unwrap();
        join.process_authentication_response(&authentication_response(0), &mut [0; 256]).unwrap();
        join.process_association_response(&association_response(0)).unwrap();
        assert!(join.supplicant.is_some());
        assert_eq!(join.association_id(), Some(1));
        // Exercise the established-state revocation path independently of the
        // separate handshake tests; this does not simulate firmware keys.
        join.phase = JoinPhase::Established;
        assert!(join.is_authorized());
        assert!(!join.peer_lost(2, BSSID));
        assert!(!join.peer_lost(1, [0x02, 0, 0, 0, 0, 9]));
        assert_eq!(join.phase(), JoinPhase::Established);
        assert!(join.peer_lost(1, BSSID));
        assert_eq!(join.phase(), JoinPhase::Failed);
        assert!(!join.is_authorized());
        assert!(join.association_id().is_none());
        assert!(join.supplicant.is_none());
        assert!(join.pmk.is_none());
        assert!(join.snonce.is_none());
        assert!(!join.peer_lost(1, BSSID));
        assert!(join.process_message1_frame(&message1_frame(), &mut [0; 256]).is_err());
    }

    #[test]
    fn joins_auth_association_and_enters_the_eapol_state_machine() {
        let mut join = join();
        let mut auth = [0xa5; 30];
        let auth_len = join.begin(&mut auth).unwrap();
        assert_eq!(auth_len, 30);
        assert_eq!(&auth[..2], &0x00b0u16.to_le_bytes());
        assert_eq!(u16::from_le_bytes([auth[22], auth[23]]), 0xfff0);
        assert_eq!(join.phase(), JoinPhase::AwaitingAuthenticationResponse);

        let mut assoc = [0xa5; 256];
        let assoc_len = join
            .process_authentication_response(&authentication_response(0), &mut assoc)
            .unwrap();
        assert!(assoc_len > 24);
        assert_eq!(&assoc[..2], &0x0000u16.to_le_bytes());
        assert_eq!(u16::from_le_bytes([assoc[22], assoc[23]]), 0);
        assert_eq!(join.phase(), JoinPhase::AwaitingAssociationResponse);
        assert_eq!(assoc[24..26], [0x11, 0]);

        let association_id = join
            .process_association_response(&association_response(0))
            .unwrap();
        assert_eq!(association_id, 1);
        assert_eq!(join.association_id(), Some(1));
        assert_eq!(join.phase(), JoinPhase::AwaitingMessage1);

        let mut message2 = [0xa5; 256];
        let message2_len = join
            .process_message1_frame(&message1_frame(), &mut message2)
            .unwrap();
        assert_eq!(&message2[..2], &0x0108u16.to_le_bytes());
        assert_eq!(join.phase(), JoinPhase::AwaitingMessage3);
        assert!(message2_len > 24 + 8);
        assert!(!join.is_authorized());
    }

    #[test]
    fn output_shortage_and_wrong_peer_do_not_advance_join_state() {
        let mut join = join();
        assert_eq!(
            join.begin(&mut [0; 29]),
            Err(JoinError::Station(StationFrameError::OutputTooSmall))
        );
        assert_eq!(join.phase(), JoinPhase::Ready);
        let mut auth = [0; 30];
        join.begin(&mut auth).unwrap();

        let mut wrong_peer = authentication_response(0);
        wrong_peer[10] ^= 2;
        assert!(matches!(
            join.process_authentication_response(&wrong_peer, &mut [0; 256]),
            Err(JoinError::Station(StationFrameError::WrongPeer))
        ));
        assert_eq!(join.phase(), JoinPhase::AwaitingAuthenticationResponse);

        assert_eq!(
            join.process_authentication_response(&authentication_response(0), &mut [0; 32]),
            Err(JoinError::Station(StationFrameError::OutputTooSmall))
        );
        assert_eq!(join.phase(), JoinPhase::AwaitingAuthenticationResponse);
    }

    #[test]
    fn rejected_authentication_or_association_is_terminal() {
        let mut auth_rejected = join();
        auth_rejected.begin(&mut [0; 30]).unwrap();
        assert_eq!(
            auth_rejected
                .process_authentication_response(&authentication_response(13), &mut [0; 256],),
            Err(JoinError::AuthenticationRejected(13))
        );
        assert_eq!(auth_rejected.phase(), JoinPhase::Failed);

        let mut assoc_rejected = join();
        assoc_rejected.begin(&mut [0; 30]).unwrap();
        assoc_rejected
            .process_authentication_response(&authentication_response(0), &mut [0; 256])
            .unwrap();
        assert_eq!(
            assoc_rejected.process_association_response(&association_response(17)),
            Err(JoinError::AssociationRejected(17))
        );
        assert_eq!(assoc_rejected.phase(), JoinPhase::Failed);
        assert_eq!(assoc_rejected.association_id(), None);
    }

    use crate::ieee80211_station::StationFrameError;
}
