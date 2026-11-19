//! Source-matched filtering and bounded best-BSS retention for WMI scan RX.
//!
//! This consumes already-decoded ath10k management-RX events. The accumulator
//! can be used directly, or through `ScanWorkflow`, which correlates one queued
//! scan with its events and results. It never transmits or changes radio state.
#![forbid(unsafe_code)]

use crate::ath10k_mgmt_rx::{
    ManagementRxEvent, WMI_RX_STATUS_ERR_CRC, WMI_RX_STATUS_ERR_DECRYPT,
    WMI_RX_STATUS_ERR_KEY_CACHE_MISS,
};
use crate::ath10k_nwifi_rx::{FrameKind, inspect_frame};
use crate::ath10k_scan::{
    Error as ScanError, QueueError, QueuedScanStart, QueuedScanStop, ScanEvent, ScanEventAction,
    ScanPhase, ScanSession, StartScanRequest, queue_start_scan, queue_stop_scan,
};
use crate::ath10k_wmi_tx::WmiCommandTxQueue;
use crate::ieee80211_bss::{
    BssSelectionError, BssSelectionPolicy, ScanCandidate, SelectedBss, select_bss,
};

const ATH10K_DEFAULT_NOISE_FLOOR_DBM: i32 = -95;
const MAX_SCAN_FRAME_BYTES: usize = 4096;
const ATH10K_MAX_5G_CHANNEL: u8 = 173;
const BEACON_SUBTYPE: u8 = 8;
const PROBE_RESPONSE_SUBTYPE: u8 = 5;
const RX_ERROR_MASK: u32 =
    WMI_RX_STATUS_ERR_CRC | WMI_RX_STATUS_ERR_DECRYPT | WMI_RX_STATUS_ERR_KEY_CACHE_MISS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanResultError {
    InvalidPolicy(BssSelectionError),
    FrameBufferTooSmall,
    CandidateFrameTooLarge,
    InvalidSignal,
    ScanNotStartingForDeadline,
    StartResponseDeadlineAlreadyArmed,
    InvalidStartResponseTimeout,
    StartResponseDeadlineOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanResultDisposition {
    StoredBest,
    ReplacedBest,
    KeptCurrent,
    IgnoredDuringCac,
    IgnoredRxError,
    IgnoredUnsupportedChannel,
    IgnoredNonBssFrame,
    IgnoredMalformedFrame,
    IgnoredNoMatch,
    IgnoredOutsideActiveScan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanWorkflowState {
    Idle,
    Starting,
    Running,
    Aborting,
    StartTimedOut,
    Completed { reason: u32 },
    Aborted { reason: u32 },
    StartFailed { reason: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanWatchdogAction {
    None,
    StartResponseTimedOut,
    StopTimedOutScan,
}

/// One-scan accumulator backed entirely by caller-owned storage.
///
/// It retains only the strongest currently compatible beacon/probe response,
/// which is enough for a single-SSID connection attempt and avoids allocating
/// an unbounded scan table. Create a fresh accumulator for each scan.
pub struct ScanResultAccumulator<'storage, 'policy> {
    policy: BssSelectionPolicy<'policy>,
    storage: &'storage mut [u8],
    best_len: Option<usize>,
    best_bssid: [u8; 6],
    best_channel: u8,
    best_rssi_dbm: i8,
}

impl<'storage, 'policy> ScanResultAccumulator<'storage, 'policy> {
    pub fn new(
        policy: BssSelectionPolicy<'policy>,
        storage: &'storage mut [u8],
    ) -> Result<Self, ScanResultError> {
        match select_bss(&[], policy) {
            Err(BssSelectionError::NoCompatibleBss) => {}
            Err(error) => return Err(ScanResultError::InvalidPolicy(error)),
            Ok(_) => {
                return Err(ScanResultError::InvalidPolicy(
                    BssSelectionError::NoCompatibleBss,
                ));
            }
        }
        Ok(Self {
            policy,
            storage,
            best_len: None,
            best_bssid: [0; 6],
            best_channel: 0,
            best_rssi_dbm: i8::MIN,
        })
    }

    /// Consider one decoded management-RX event.
    ///
    /// CAC and CRC/decrypt/key-cache failures follow the pinned ath10k WMI
    /// handler. SNR becomes dBm using ath10k's `-95` dBm noise floor. MIC
    /// status is intentionally not in the drop mask, matching the source.
    pub fn observe(
        &mut self,
        event: ManagementRxEvent<'_>,
        cac_running: bool,
    ) -> Result<ScanResultDisposition, ScanResultError> {
        if cac_running {
            return Ok(ScanResultDisposition::IgnoredDuringCac);
        }
        if event.status & RX_ERROR_MASK != 0 {
            return Ok(ScanResultDisposition::IgnoredRxError);
        }
        if event.frame.len() > MAX_SCAN_FRAME_BYTES {
            return Err(ScanResultError::CandidateFrameTooLarge);
        }
        let channel = match u8::try_from(event.channel_number) {
            Ok(channel) if supported_channel(channel) => channel,
            _ => return Ok(ScanResultDisposition::IgnoredUnsupportedChannel),
        };
        let signal = i32::try_from(event.snr)
            .ok()
            .and_then(|snr| snr.checked_add(ATH10K_DEFAULT_NOISE_FLOOR_DBM))
            .and_then(|signal| i8::try_from(signal).ok())
            .ok_or(ScanResultError::InvalidSignal)?;

        let view = match inspect_frame(event.frame) {
            Ok(view) => view,
            Err(_) => return Ok(ScanResultDisposition::IgnoredMalformedFrame),
        };
        if view.kind != FrameKind::Management
            || !matches!(view.subtype, BEACON_SUBTYPE | PROBE_RESPONSE_SUBTYPE)
        {
            return Ok(ScanResultDisposition::IgnoredNonBssFrame);
        }
        let candidate = ScanCandidate {
            frame: event.frame,
            rssi_dbm: signal,
            observed_channel: Some(channel),
        };
        let selected = match select_bss(core::slice::from_ref(&candidate), self.policy) {
            Ok(selected) => selected,
            Err(BssSelectionError::NoCompatibleBss) => {
                return Ok(ScanResultDisposition::IgnoredNoMatch);
            }
            Err(error) => return Err(ScanResultError::InvalidPolicy(error)),
        };
        if self.storage.len() < event.frame.len() {
            return Err(ScanResultError::FrameBufferTooSmall);
        }

        let disposition = match self.best_len {
            None => ScanResultDisposition::StoredBest,
            Some(_)
                if ranks_before(
                    selected.rssi_dbm,
                    selected.advertisement.bssid,
                    selected.channel,
                    self.best_rssi_dbm,
                    self.best_bssid,
                    self.best_channel,
                ) =>
            {
                ScanResultDisposition::ReplacedBest
            }
            Some(_) => return Ok(ScanResultDisposition::KeptCurrent),
        };

        self.storage[..event.frame.len()].copy_from_slice(event.frame);
        self.best_len = Some(event.frame.len());
        self.best_bssid = selected.advertisement.bssid;
        self.best_channel = selected.channel;
        self.best_rssi_dbm = selected.rssi_dbm;
        Ok(disposition)
    }

    /// Return the current best result, borrowing its frame from caller storage.
    pub fn selected(&self) -> Result<Option<SelectedBss<'_>>, BssSelectionError> {
        let Some(frame_len) = self.best_len else {
            return Ok(None);
        };
        let candidate = ScanCandidate {
            frame: &self.storage[..frame_len],
            rssi_dbm: self.best_rssi_dbm,
            observed_channel: Some(self.best_channel),
        };
        select_bss(core::slice::from_ref(&candidate), self.policy).map(Some)
    }

    pub const fn is_empty(&self) -> bool {
        self.best_len.is_none()
    }
}

/// One scan request connected to its management-RX results and BSS selection.
///
/// The workflow owns the same `ScanSession` used by WMI queue submission. RX
/// candidates are accepted only while that exact request is running/aborting,
/// and a selected BSS is exposed only after non-aborted completion.
pub struct ScanWorkflow<'storage, 'policy> {
    session: ScanSession,
    results: ScanResultAccumulator<'storage, 'policy>,
    state: ScanWorkflowState,
    max_scan_ms: u32,
    scan_deadline_ms: Option<u64>,
    start_response_deadline_ms: Option<u64>,
}

impl<'storage, 'policy> ScanWorkflow<'storage, 'policy> {
    pub fn new(
        policy: BssSelectionPolicy<'policy>,
        storage: &'storage mut [u8],
    ) -> Result<Self, ScanResultError> {
        Ok(Self {
            session: ScanSession::new(),
            results: ScanResultAccumulator::new(policy, storage)?,
            state: ScanWorkflowState::Idle,
            max_scan_ms: 0,
            scan_deadline_ms: None,
            start_response_deadline_ms: None,
        })
    }

    /// Queue one source-matched scan command and bind its identifiers to RX.
    pub fn queue_start<const RING_SIZE: usize>(
        &mut self,
        queue: &mut WmiCommandTxQueue<RING_SIZE>,
        request: &StartScanRequest<'_>,
        ce_iova: u64,
        transfer_id: u16,
        context: u32,
        htc_frame_output: &mut [u8],
    ) -> Result<QueuedScanStart, QueueError> {
        if self.state != ScanWorkflowState::Idle {
            return Err(QueueError::Scan(ScanError::ScanAlreadyActive));
        }
        let queued = queue_start_scan(
            &mut self.session,
            queue,
            request,
            ce_iova,
            transfer_id,
            context,
            htc_frame_output,
        )?;
        self.max_scan_ms = request.max_scan_ms;
        self.scan_deadline_ms = None;
        self.start_response_deadline_ms = None;
        self.state = ScanWorkflowState::Starting;
        Ok(queued)
    }

    /// Arm the response deadline immediately after the native WMI transport
    /// accepts the queued START_SCAN command. The earlier `queue_start` call
    /// only stages a command in software and is not a firmware-visible submit.
    /// Poll this before dispatching every scan event. Expiry is terminal for
    /// this workflow because a late event has no generation field; do not
    /// retry or reuse its scan identifiers until the WMI generation resets.
    pub fn arm_start_response_deadline(
        &mut self,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Result<(), ScanResultError> {
        if self.state != ScanWorkflowState::Starting {
            return Err(ScanResultError::ScanNotStartingForDeadline);
        }
        if self.start_response_deadline_ms.is_some() {
            return Err(ScanResultError::StartResponseDeadlineAlreadyArmed);
        }
        if timeout_ms == 0 {
            return Err(ScanResultError::InvalidStartResponseTimeout);
        }
        let deadline = now_ms
            .checked_add(timeout_ms)
            .ok_or(ScanResultError::StartResponseDeadlineOverflow)?;
        self.start_response_deadline_ms = Some(deadline);
        Ok(())
    }

    /// Expire a START_SCAN command that firmware has not acknowledged.
    /// This is one-shot and quarantines the scan identifiers against late
    /// STARTED/COMPLETED events. A fresh workflow belongs to a fresh WMI
    /// generation, not an automatic scan retry.
    pub fn poll_start_response_deadline(&mut self, now_ms: u64) -> ScanWatchdogAction {
        if self.state != ScanWorkflowState::Starting
            || !self
                .start_response_deadline_ms
                .is_some_and(|deadline| now_ms >= deadline)
        {
            return ScanWatchdogAction::None;
        }
        // Keep the workflow unchanged if a future caller violates the
        // session/workflow phase invariant.
        if self.session.start_response_timed_out().is_err() {
            return ScanWatchdogAction::None;
        }
        self.start_response_deadline_ms = None;
        self.scan_deadline_ms = None;
        self.state = ScanWorkflowState::StartTimedOut;
        ScanWatchdogAction::StartResponseTimedOut
    }

    /// Queue the matching stop command for the active scan.
    pub fn queue_stop<const RING_SIZE: usize>(
        &mut self,
        queue: &mut WmiCommandTxQueue<RING_SIZE>,
        ce_iova: u64,
        transfer_id: u16,
        context: u32,
        htc_frame_output: &mut [u8],
    ) -> Result<QueuedScanStop, QueueError> {
        if self.state != ScanWorkflowState::Running {
            return Err(QueueError::Scan(ScanError::ScanNotRunning));
        }
        let queued = queue_stop_scan(
            &mut self.session,
            queue,
            ce_iova,
            transfer_id,
            context,
            htc_frame_output,
        )?;
        self.scan_deadline_ms = None;
        self.state = ScanWorkflowState::Aborting;
        Ok(queued)
    }

    /// Correlate one decoded WMI event and latch its terminal outcome.
    pub fn observe_scan_event(&mut self, event: ScanEvent) -> ScanEventAction {
        self.observe_scan_event_inner(event, None)
    }

    /// Correlate one WMI event and arm or clear the maximum-duration watchdog.
    /// `now_ms` must come from a monotonic clock. The watchdog is armed only
    /// after firmware confirms that the scan started; on expiry the caller
    /// should queue the existing STOP_SCAN command. It never sends on its own.
    pub fn observe_scan_event_at(&mut self, event: ScanEvent, now_ms: u64) -> ScanEventAction {
        self.observe_scan_event_inner(event, Some(now_ms))
    }

    fn observe_scan_event_inner(
        &mut self,
        event: ScanEvent,
        now_ms: Option<u64>,
    ) -> ScanEventAction {
        let action = self.session.observe_event(event);
        match action {
            ScanEventAction::Started => {
                self.state = ScanWorkflowState::Running;
                self.start_response_deadline_ms = None;
                self.scan_deadline_ms = now_ms.and_then(|now| {
                    (self.max_scan_ms != 0).then(|| now.saturating_add(u64::from(self.max_scan_ms)))
                });
            }
            ScanEventAction::StartFailed { reason } => {
                self.start_response_deadline_ms = None;
                self.scan_deadline_ms = None;
                self.state = ScanWorkflowState::StartFailed { reason };
            }
            ScanEventAction::Completed {
                reason,
                aborted: false,
            } => {
                self.start_response_deadline_ms = None;
                self.scan_deadline_ms = None;
                self.state = ScanWorkflowState::Completed { reason };
            }
            ScanEventAction::Completed {
                reason,
                aborted: true,
            } => {
                self.start_response_deadline_ms = None;
                self.scan_deadline_ms = None;
                self.state = ScanWorkflowState::Aborted { reason };
            }
            _ => {}
        }
        action
    }

    /// Poll the monotonic maximum-duration deadline. Once expired, this keeps
    /// requesting a stop until `queue_stop` succeeds; a rejected software
    /// enqueue therefore cannot silently lose the timeout.
    pub fn watchdog_action(&self, now_ms: u64) -> ScanWatchdogAction {
        if self.state == ScanWorkflowState::Running
            && self
                .scan_deadline_ms
                .is_some_and(|deadline| now_ms >= deadline)
        {
            ScanWatchdogAction::StopTimedOutScan
        } else {
            ScanWatchdogAction::None
        }
    }

    /// Feed management RX only while the correlated WMI scan is active.
    pub fn observe_management_rx(
        &mut self,
        event: ManagementRxEvent<'_>,
        cac_running: bool,
    ) -> Result<ScanResultDisposition, ScanResultError> {
        if !matches!(
            self.session.phase(),
            ScanPhase::Running | ScanPhase::Aborting
        ) {
            return Ok(ScanResultDisposition::IgnoredOutsideActiveScan);
        }
        self.results.observe(event, cac_running)
    }

    pub const fn state(&self) -> ScanWorkflowState {
        self.state
    }

    /// A successful, non-aborted scan is required before exposing a BSS.
    pub fn selected(&self) -> Result<Option<SelectedBss<'_>>, BssSelectionError> {
        if !matches!(self.state, ScanWorkflowState::Completed { .. }) {
            return Ok(None);
        }
        self.results.selected()
    }
}

fn supported_channel(channel: u8) -> bool {
    (1..=14).contains(&channel) || (36..=ATH10K_MAX_5G_CHANNEL).contains(&channel)
}

fn ranks_before(
    candidate_rssi: i8,
    candidate_bssid: [u8; 6],
    candidate_channel: u8,
    current_rssi: i8,
    current_bssid: [u8; 6],
    current_channel: u8,
) -> bool {
    candidate_rssi > current_rssi
        || (candidate_rssi == current_rssi
            && (candidate_bssid < current_bssid
                || (candidate_bssid == current_bssid && candidate_channel < current_channel)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use crate::ath10k_scan::{StartScanRequest, WMI_SCAN_EVENT_COMPLETED, WMI_SCAN_EVENT_STARTED};
    use crate::ieee80211_bss::BssSelectionPolicy;

    const RSN_WPA2_PSK_CCMP: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];

    fn beacon(bssid: [u8; 6], ssid: &[u8], channel: u8) -> ([u8; 128], usize) {
        let length = 36 + 2 + ssid.len() + 3 + 3 + RSN_WPA2_PSK_CCMP.len();
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
        frame[offset..offset + RSN_WPA2_PSK_CCMP.len()].copy_from_slice(&RSN_WPA2_PSK_CCMP);
        (frame, length)
    }

    fn event<'a>(frame: &'a [u8], channel: u32, snr: u32, status: u32) -> ManagementRxEvent<'a> {
        ManagementRxEvent {
            channel_number: channel,
            snr,
            rssi_control: [0; 4],
            rate: 0,
            phy_mode: 0,
            status,
            frame,
            rx_mac_timestamp: None,
        }
    }

    fn policy<'a>(ssid: &'a [u8]) -> BssSelectionPolicy<'a> {
        BssSelectionPolicy {
            ssid,
            pinned_bssid: None,
            allowed_channels: &[1, 6],
            minimum_rssi_dbm: -80,
        }
    }

    fn wmi_queue() -> WmiCommandTxQueue<4> {
        WmiCommandTxQueue::new_wmi_control(
            HtcEndpoint {
                service_id: HTC_WMI_CONTROL_SERVICE_ID,
                endpoint_id: 3,
                max_message_size: 512,
            },
            HtcServicePolicy {
                receive_allocation: 32,
                disable_credit_flow: false,
                credit_size: 16,
                initial_credits: 32,
            },
            false,
            9,
        )
        .unwrap()
    }

    fn workflow_event(event_type: u32) -> ScanEvent {
        ScanEvent {
            event_type,
            reason: 0,
            channel_frequency_mhz: 0,
            requestor_id: 0xa001,
            scan_id: 0xa000,
            vdev_id: 4,
        }
    }

    #[test]
    fn filters_cac_and_bad_rx_status_then_retains_borrowed_best_bss() {
        let (frame, frame_len) = beacon([2, 0, 0, 0, 0, 2], b"home", 1);
        let mut storage = [0xa5; 128];
        {
            let mut results = ScanResultAccumulator::new(policy(b"home"), &mut storage).unwrap();

            assert_eq!(
                results.observe(event(&frame[..frame_len], 1, 55, 0), true),
                Ok(ScanResultDisposition::IgnoredDuringCac)
            );
            assert_eq!(
                results.observe(
                    event(&frame[..frame_len], 1, 55, WMI_RX_STATUS_ERR_CRC),
                    false
                ),
                Ok(ScanResultDisposition::IgnoredRxError)
            );
            assert!(results.is_empty());
            assert_eq!(
                results.observe(event(&frame[..frame_len], 1, 55, 0), false),
                Ok(ScanResultDisposition::StoredBest)
            );
            let selected = results.selected().unwrap().unwrap();
            assert_eq!(selected.rssi_dbm, -40); // 55 dB SNR plus the -95 dBm floor.
            assert_eq!(selected.channel, 1);
            assert_eq!(selected.advertisement.bssid, [2, 0, 0, 0, 0, 2]);
        }
        assert!(storage[..frame_len] == frame[..frame_len]);
        assert!(storage[frame_len..].iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn updates_strongest_candidate_and_rejects_wrong_channel_or_network() {
        let (first, first_len) = beacon([2, 0, 0, 0, 0, 5], b"home", 1);
        let (stronger, stronger_len) = beacon([2, 0, 0, 0, 0, 3], b"home", 6);
        let (wrong_ssid, wrong_ssid_len) = beacon([2, 0, 0, 0, 0, 2], b"guest", 1);
        let mut storage = [0; 128];
        let mut results = ScanResultAccumulator::new(policy(b"home"), &mut storage).unwrap();

        assert_eq!(
            results.observe(event(&first[..first_len], 1, 45, 0), false),
            Ok(ScanResultDisposition::StoredBest)
        );
        assert_eq!(
            results.observe(event(&stronger[..stronger_len], 6, 60, 0), false),
            Ok(ScanResultDisposition::ReplacedBest)
        );
        assert_eq!(
            results.observe(event(&wrong_ssid[..wrong_ssid_len], 1, 80, 0), false),
            Ok(ScanResultDisposition::IgnoredNoMatch)
        );
        assert_eq!(
            results.observe(event(&first[..first_len], 6, 75, 0), false),
            Ok(ScanResultDisposition::IgnoredNoMatch)
        );
        assert_eq!(
            results.selected().unwrap().unwrap().advertisement.bssid,
            [2, 0, 0, 0, 0, 3]
        );
    }

    #[test]
    fn refuses_invalid_policy_and_small_or_oversized_storage_inputs() {
        assert!(matches!(
            ScanResultAccumulator::new(
                BssSelectionPolicy {
                    ssid: b"",
                    pinned_bssid: None,
                    allowed_channels: &[1],
                    minimum_rssi_dbm: -80,
                },
                &mut [0; 128],
            ),
            Err(ScanResultError::InvalidPolicy(
                BssSelectionError::InvalidSsid
            ))
        ));

        let (frame, frame_len) = beacon([2, 0, 0, 0, 0, 2], b"home", 1);
        let mut small_storage = [0; 16];
        let mut results = ScanResultAccumulator::new(policy(b"home"), &mut small_storage).unwrap();
        assert_eq!(
            results.observe(event(&frame[..frame_len], 1, 55, 0), false),
            Err(ScanResultError::FrameBufferTooSmall)
        );

        let large_frame = [0; MAX_SCAN_FRAME_BYTES + 1];
        assert_eq!(
            results.observe(event(&large_frame, 1, 55, 0), false),
            Err(ScanResultError::CandidateFrameTooLarge)
        );
        assert!(results.is_empty());
    }

    #[test]
    fn matches_the_pinned_maximum_5ghz_channel() {
        let (frame, frame_len) = beacon([2, 0, 0, 0, 0, 2], b"home", 173);
        let mut storage = [0; 128];
        let mut channel_policy = policy(b"home");
        channel_policy.allowed_channels = &[173];
        let mut results = ScanResultAccumulator::new(channel_policy, &mut storage).unwrap();
        assert_eq!(
            results.observe(event(&frame[..frame_len], 173, 55, 0), false),
            Ok(ScanResultDisposition::StoredBest)
        );
        assert_eq!(
            results.observe(event(&frame[..frame_len], 174, 55, 0), false),
            Ok(ScanResultDisposition::IgnoredUnsupportedChannel)
        );
    }

    #[test]
    fn queued_scan_correlates_rx_and_exposes_bss_only_after_completion() {
        let channels = [2412];
        let ssid = b"home";
        let ssids: [&[u8]; 1] = [ssid];
        let request = StartScanRequest::linux_reference_defaults(4, &channels, &ssids, &[]);
        let (frame, frame_len) = beacon([2, 0, 0, 0, 0, 2], ssid, 1);
        let mut storage = [0; 128];
        let mut workflow = ScanWorkflow::new(policy(ssid), &mut storage).unwrap();
        assert_eq!(workflow.state(), ScanWorkflowState::Idle);
        assert_eq!(
            workflow.observe_management_rx(event(&frame[..frame_len], 1, 55, 0), false),
            Ok(ScanResultDisposition::IgnoredOutsideActiveScan)
        );

        let mut queue = wmi_queue();
        let mut htc_frame = [0; 512];
        workflow
            .queue_start(&mut queue, &request, 0x1234_5000, 3, 0x1234, &mut htc_frame)
            .unwrap();
        assert_eq!(workflow.state(), ScanWorkflowState::Starting);
        assert_eq!(
            workflow.observe_scan_event(workflow_event(WMI_SCAN_EVENT_STARTED)),
            ScanEventAction::Started
        );
        assert_eq!(workflow.state(), ScanWorkflowState::Running);
        assert_eq!(
            workflow.observe_management_rx(event(&frame[..frame_len], 1, 55, 0), false),
            Ok(ScanResultDisposition::StoredBest)
        );
        assert_eq!(workflow.selected().unwrap(), None);

        assert_eq!(
            workflow.observe_scan_event(workflow_event(WMI_SCAN_EVENT_COMPLETED)),
            ScanEventAction::Completed {
                reason: 0,
                aborted: false,
            }
        );
        assert_eq!(workflow.state(), ScanWorkflowState::Completed { reason: 0 });
        let selected = workflow.selected().unwrap().unwrap();
        assert_eq!(selected.advertisement.bssid, [2, 0, 0, 0, 0, 2]);
        assert_eq!(selected.channel, 1);
        assert_eq!(selected.rssi_dbm, -40);
    }

    #[test]
    fn queued_scan_stop_keeps_results_hidden_after_abort_completion() {
        let channels = [2412];
        let ssid = b"home";
        let ssids: [&[u8]; 1] = [ssid];
        let request = StartScanRequest::linux_reference_defaults(4, &channels, &ssids, &[]);
        let (frame, frame_len) = beacon([2, 0, 0, 0, 0, 2], ssid, 1);
        let mut storage = [0; 128];
        let mut workflow = ScanWorkflow::new(policy(ssid), &mut storage).unwrap();
        let mut queue = wmi_queue();
        let mut htc_frame = [0; 512];

        workflow
            .queue_start(&mut queue, &request, 0x1234_5000, 3, 0x1234, &mut htc_frame)
            .unwrap();
        workflow.observe_scan_event(workflow_event(WMI_SCAN_EVENT_STARTED));
        workflow
            .observe_management_rx(event(&frame[..frame_len], 1, 55, 0), false)
            .unwrap();
        let stop = workflow
            .queue_stop(&mut queue, 0x1234_6000, 4, 0x5678, &mut htc_frame)
            .unwrap();
        assert_eq!(
            stop.command.command_id,
            crate::ath10k_scan::WMI_10_4_STOP_SCAN_COMMAND_ID
        );
        assert_eq!(workflow.state(), ScanWorkflowState::Aborting);

        assert_eq!(
            workflow.observe_scan_event(ScanEvent {
                reason: 1,
                ..workflow_event(WMI_SCAN_EVENT_COMPLETED)
            }),
            ScanEventAction::Completed {
                reason: 1,
                aborted: true,
            }
        );
        assert_eq!(workflow.state(), ScanWorkflowState::Aborted { reason: 1 });
        assert_eq!(workflow.selected().unwrap(), None);
    }

    #[test]
    fn watchdog_requests_stop_at_the_firmware_scan_limit_and_disarms_on_queue() {
        let channels = [2412];
        let ssid = b"home";
        let ssids: [&[u8]; 1] = [ssid];
        let mut request = StartScanRequest::linux_reference_defaults(4, &channels, &ssids, &[]);
        request.max_scan_ms = 400;
        let mut storage = [0; 128];
        let mut workflow = ScanWorkflow::new(policy(ssid), &mut storage).unwrap();
        let mut queue = wmi_queue();
        let mut htc_frame = [0; 512];

        workflow
            .queue_start(&mut queue, &request, 0x1234_5000, 3, 0x1234, &mut htc_frame)
            .unwrap();
        assert_eq!(workflow.watchdog_action(50_000), ScanWatchdogAction::None);
        assert_eq!(
            workflow.observe_scan_event_at(workflow_event(WMI_SCAN_EVENT_STARTED), 1_000),
            ScanEventAction::Started
        );
        assert_eq!(workflow.watchdog_action(1_399), ScanWatchdogAction::None);
        assert_eq!(
            workflow.watchdog_action(1_400),
            ScanWatchdogAction::StopTimedOutScan
        );

        workflow
            .queue_stop(&mut queue, 0x1234_6000, 4, 0x5678, &mut htc_frame)
            .unwrap();
        assert_eq!(workflow.state(), ScanWorkflowState::Aborting);
        assert_eq!(workflow.watchdog_action(20_000), ScanWatchdogAction::None);
        assert_eq!(
            workflow.observe_scan_event(ScanEvent {
                reason: 1,
                ..workflow_event(WMI_SCAN_EVENT_COMPLETED)
            }),
            ScanEventAction::Completed {
                reason: 1,
                aborted: true,
            }
        );
        assert_eq!(workflow.watchdog_action(u64::MAX), ScanWatchdogAction::None);
    }

    #[test]
    fn watchdog_ignores_a_zero_firmware_scan_limit() {
        let channels = [2412];
        let ssid = b"home";
        let ssids: [&[u8]; 1] = [ssid];
        let mut request = StartScanRequest::linux_reference_defaults(4, &channels, &ssids, &[]);
        request.max_scan_ms = 0;
        let mut storage = [0; 128];
        let mut workflow = ScanWorkflow::new(policy(ssid), &mut storage).unwrap();
        let mut queue = wmi_queue();
        let mut htc_frame = [0; 512];

        workflow
            .queue_start(&mut queue, &request, 0x1234_5000, 3, 0x1234, &mut htc_frame)
            .unwrap();
        workflow.observe_scan_event_at(workflow_event(WMI_SCAN_EVENT_STARTED), 1_000);
        assert_eq!(workflow.watchdog_action(u64::MAX), ScanWatchdogAction::None);
    }

    #[test]
    fn start_response_timeout_is_one_shot_and_quarantines_late_events() {
        let channels = [2412];
        let ssid = b"home";
        let ssids: [&[u8]; 1] = [ssid];
        let request = StartScanRequest::linux_reference_defaults(4, &channels, &ssids, &[]);
        let mut storage = [0; 128];
        let mut workflow = ScanWorkflow::new(policy(ssid), &mut storage).unwrap();
        let mut queue = wmi_queue();
        let mut htc_frame = [0; 512];

        workflow
            .queue_start(&mut queue, &request, 0x1234_5000, 3, 0x1234, &mut htc_frame)
            .unwrap();
        workflow.arm_start_response_deadline(2_000, 300).unwrap();
        assert_eq!(
            workflow.poll_start_response_deadline(2_299),
            ScanWatchdogAction::None
        );
        assert_eq!(
            workflow.poll_start_response_deadline(2_300),
            ScanWatchdogAction::StartResponseTimedOut
        );
        assert_eq!(workflow.state(), ScanWorkflowState::StartTimedOut);
        assert_eq!(
            workflow.poll_start_response_deadline(u64::MAX),
            ScanWatchdogAction::None
        );
        assert_eq!(
            workflow.observe_scan_event(workflow_event(WMI_SCAN_EVENT_STARTED)),
            ScanEventAction::UnexpectedPhase
        );
        assert_eq!(workflow.state(), ScanWorkflowState::StartTimedOut);
        assert_eq!(
            workflow.queue_start(&mut queue, &request, 0x1234_6000, 4, 0x5678, &mut htc_frame),
            Err(QueueError::Scan(ScanError::ScanAlreadyActive))
        );
    }

    #[test]
    fn start_response_deadline_rejects_invalid_or_repeated_arming() {
        let channels = [2412];
        let ssid = b"home";
        let ssids: [&[u8]; 1] = [ssid];
        let request = StartScanRequest::linux_reference_defaults(4, &channels, &ssids, &[]);
        let mut storage = [0; 128];
        let mut workflow = ScanWorkflow::new(policy(ssid), &mut storage).unwrap();
        assert_eq!(
            workflow.arm_start_response_deadline(1, 10),
            Err(ScanResultError::ScanNotStartingForDeadline)
        );

        let mut queue = wmi_queue();
        let mut htc_frame = [0; 512];
        workflow
            .queue_start(&mut queue, &request, 0x1234_5000, 3, 0x1234, &mut htc_frame)
            .unwrap();
        assert_eq!(
            workflow.arm_start_response_deadline(1, 0),
            Err(ScanResultError::InvalidStartResponseTimeout)
        );
        assert_eq!(
            workflow.arm_start_response_deadline(u64::MAX, 1),
            Err(ScanResultError::StartResponseDeadlineOverflow)
        );
        workflow.arm_start_response_deadline(1, 10).unwrap();
        assert_eq!(
            workflow.arm_start_response_deadline(2, 10),
            Err(ScanResultError::StartResponseDeadlineAlreadyArmed)
        );
        assert_eq!(
            workflow.observe_scan_event(workflow_event(WMI_SCAN_EVENT_STARTED)),
            ScanEventAction::Started
        );
        assert_eq!(
            workflow.poll_start_response_deadline(u64::MAX),
            ScanWatchdogAction::None
        );
        assert_eq!(
            workflow.arm_start_response_deadline(3, 10),
            Err(ScanResultError::ScanNotStartingForDeadline)
        );
    }
}
