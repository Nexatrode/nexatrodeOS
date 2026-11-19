//! Bounded ath10k WMI 10.4 scan command encoders.
//!
//! These emit command bodies used by the pinned WMI 10.4 path. They do not
//! select regulatory channels, send over hardware, or control the radio.
#![forbid(unsafe_code)]

use crate::ath10k_nwifi_rx::InformationElementIter;
use crate::ath10k_wmi_tx::{Error as WmiTxError, QueuedTransfer, WmiCommandTxQueue};

pub const WMI_10_4_START_SCAN_COMMAND_ID: u32 = 0x9000;
pub const WMI_10_4_STOP_SCAN_COMMAND_ID: u32 = 0x9001;
pub const WMI_SCAN_STOP_ONE: u32 = 0;
pub const WMI_SCAN_EVENT_BYTES: usize = 24;
pub const STOP_SCAN_COMMAND_BYTES: usize = 16;
pub const MAX_SCAN_CHANNELS: usize = 64;
pub const MAX_SCAN_SSIDS: usize = 16;
pub const MAX_SCAN_BSSIDS: usize = 4;
pub const MAX_SCAN_IE_BYTES: usize = 256;
pub const MAX_SSID_BYTES: usize = 32;

const HOST_SCAN_ID_PREFIX: u32 = 0xa000;
const MAX_HOST_SCAN_ID: u16 = 0x0fff;
const MAX_SCAN_PRIORITY: u32 = 4;
const CHANNEL_LIST_TAG: u32 = 1;
const SSID_LIST_TAG: u32 = 2;
const BSSID_LIST_TAG: u32 = 3;
const IE_TAG: u32 = 4;

const COMMON_WORDS: usize = 15;
const COMMON_BYTES: usize = COMMON_WORDS * 4;
const FIXED_COMMAND_BYTES: usize = COMMON_BYTES + 4;
const LIST_HEADER_BYTES: usize = 8;
const CHANNEL_ENTRY_BYTES: usize = 4;
const SSID_ENTRY_BYTES: usize = 4 + MAX_SSID_BYTES;
const MAC_ADDRESS_BYTES: usize = 8;
/// Maximum bounded WMI 10.4 start-scan command body for all supported lists.
pub const MAX_START_SCAN_COMMAND_BYTES: usize = FIXED_COMMAND_BYTES
    + LIST_HEADER_BYTES
    + MAX_SCAN_CHANNELS * CHANNEL_ENTRY_BYTES
    + LIST_HEADER_BYTES
    + MAX_SCAN_SSIDS * SSID_ENTRY_BYTES
    + LIST_HEADER_BYTES
    + MAX_SCAN_BSSIDS * MAC_ADDRESS_BYTES
    + LIST_HEADER_BYTES
    + MAX_SCAN_IE_BYTES;

pub const WMI_SCAN_EVENT_STARTED: u32 = 1 << 0;
pub const WMI_SCAN_EVENT_COMPLETED: u32 = 1 << 1;
pub const WMI_SCAN_EVENT_BSS_CHANNEL: u32 = 1 << 2;
pub const WMI_SCAN_EVENT_FOREIGN_CHANNEL: u32 = 1 << 3;
pub const WMI_SCAN_EVENT_DEQUEUED: u32 = 1 << 4;
pub const WMI_SCAN_EVENT_PREEMPTED: u32 = 1 << 5;
pub const WMI_SCAN_EVENT_START_FAILED: u32 = 1 << 6;
pub const WMI_SCAN_EVENT_RESTARTED: u32 = 1 << 7;
pub const WMI_SCAN_EVENT_FOREIGN_CHANNEL_EXIT: u32 = 1 << 8;
pub const WMI_SCAN_FLAG_PASSIVE: u32 = 0x01;
pub const WMI_SCAN_CHAN_STAT_EVENT: u32 = 0x10;
pub const LINUX_SCAN_REQUESTOR_ID: u16 = 1;
pub const LINUX_SCAN_ID: u16 = 0;
pub const LINUX_SCAN_PRIORITY_LOW: u32 = 1;
pub const LINUX_SCAN_NOTIFY_EVENTS: u32 = WMI_SCAN_EVENT_STARTED
    | WMI_SCAN_EVENT_COMPLETED
    | WMI_SCAN_EVENT_BSS_CHANNEL
    | WMI_SCAN_EVENT_FOREIGN_CHANNEL
    | WMI_SCAN_EVENT_DEQUEUED
    | WMI_SCAN_EVENT_FOREIGN_CHANNEL_EXIT;
const BROADCAST_BSSID_LIST: [[u8; 6]; 1] = [[0xff; 6]];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanEventKind {
    Started,
    Completed,
    BssChannel,
    ForeignChannel,
    Dequeued,
    Preempted,
    StartFailed,
    Restarted,
    ForeignChannelExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanEvent {
    /// Raw WMI event type; unknown values are preserved for forward compatibility.
    pub event_type: u32,
    /// Raw completion reason; meaningful for completion and start-failure events.
    pub reason: u32,
    /// Channel frequency in MHz when the event is `ForeignChannel`.
    pub channel_frequency_mhz: u32,
    pub requestor_id: u32,
    pub scan_id: u32,
    pub vdev_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanPhase {
    Idle,
    Starting,
    Running,
    Aborting,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanEventAction {
    Started,
    StartFailed { reason: u32 },
    Completed { reason: u32, aborted: bool },
    BssChannel,
    ForeignChannel { frequency_mhz: u32 },
    Ignored,
    Unmatched,
    UnexpectedPhase,
}

/// Tracks one host-owned scan request. It never sends commands or controls the radio.
///
/// The three-identifier match is deliberately strict: Linux ath10k uses one global
/// scan state and does not filter these event IDs, but this session refuses stale or
/// cross-vdev events before they can affect host state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanSession {
    phase: ScanPhase,
    requestor_id: u32,
    scan_id: u32,
    vdev_id: u32,
}

impl ScanSession {
    pub const fn new() -> Self {
        Self {
            phase: ScanPhase::Idle,
            requestor_id: 0,
            scan_id: 0,
            vdev_id: 0,
        }
    }

    pub const fn phase(&self) -> ScanPhase {
        self.phase
    }

    /// Reserve the sole scan slot before a caller attempts transport submission.
    pub fn reserve_start(&mut self, request: &StartScanRequest<'_>) -> Result<(), Error> {
        start_scan_payload_len(request)?;
        if self.phase != ScanPhase::Idle {
            return Err(Error::ScanAlreadyActive);
        }

        self.requestor_id = HOST_SCAN_ID_PREFIX | u32::from(request.requestor_id);
        self.scan_id = HOST_SCAN_ID_PREFIX | u32::from(request.scan_id);
        self.vdev_id = request.vdev_id;
        self.phase = ScanPhase::Starting;
        Ok(())
    }

    /// Release a reserved start if command submission itself fails.
    pub fn submission_failed(&mut self) -> Result<(), Error> {
        if self.phase != ScanPhase::Starting {
            return Err(Error::ScanNotStarting);
        }
        self.finish();
        Ok(())
    }

    /// Quarantine a start whose firmware acknowledgement deadline expired.
    /// Keep the scan identifiers so late events cannot be mistaken for a new
    /// scan in this WMI generation. Recover by resetting the WMI generation and
    /// constructing a new session; do not retry with these identifiers.
    pub fn start_response_timed_out(&mut self) -> Result<(), Error> {
        if self.phase != ScanPhase::Starting {
            return Err(Error::ScanNotStarting);
        }
        self.phase = ScanPhase::TimedOut;
        Ok(())
    }

    /// Mark an already-running scan as aborting before the caller sends stop.
    pub fn mark_abort_requested(&mut self) -> Result<(), Error> {
        match self.phase {
            ScanPhase::Running => {
                self.phase = ScanPhase::Aborting;
                Ok(())
            }
            ScanPhase::Aborting => Err(Error::ScanAlreadyAborting),
            ScanPhase::Idle | ScanPhase::Starting | ScanPhase::TimedOut => {
                Err(Error::ScanNotRunning)
            }
        }
    }

    /// Restore a queued-stop reservation when software transport rejected it
    /// before a CE slot was committed. A queued stop that may reach hardware
    /// must remain `Aborting` until a terminal scan event arrives.
    pub fn stop_submission_failed(&mut self) -> Result<(), Error> {
        if self.phase != ScanPhase::Aborting {
            return Err(Error::ScanNotAborting);
        }
        self.phase = ScanPhase::Running;
        Ok(())
    }

    /// Apply a decoded event only when it belongs to this exact active request.
    pub fn observe_event(&mut self, event: ScanEvent) -> ScanEventAction {
        if self.phase == ScanPhase::Idle
            || event.requestor_id != self.requestor_id
            || event.scan_id != self.scan_id
            || event.vdev_id != self.vdev_id
        {
            return ScanEventAction::Unmatched;
        }

        match event.event_type {
            WMI_SCAN_EVENT_STARTED if self.phase == ScanPhase::Starting => {
                self.phase = ScanPhase::Running;
                ScanEventAction::Started
            }
            WMI_SCAN_EVENT_START_FAILED if self.phase == ScanPhase::Starting => {
                self.finish();
                ScanEventAction::StartFailed {
                    reason: event.reason,
                }
            }
            WMI_SCAN_EVENT_COMPLETED
                if matches!(self.phase, ScanPhase::Running | ScanPhase::Aborting) =>
            {
                let aborted = self.phase == ScanPhase::Aborting;
                self.finish();
                ScanEventAction::Completed {
                    reason: event.reason,
                    aborted,
                }
            }
            WMI_SCAN_EVENT_BSS_CHANNEL
                if matches!(self.phase, ScanPhase::Running | ScanPhase::Aborting) =>
            {
                ScanEventAction::BssChannel
            }
            WMI_SCAN_EVENT_FOREIGN_CHANNEL
                if matches!(self.phase, ScanPhase::Running | ScanPhase::Aborting) =>
            {
                ScanEventAction::ForeignChannel {
                    frequency_mhz: event.channel_frequency_mhz,
                }
            }
            WMI_SCAN_EVENT_STARTED
            | WMI_SCAN_EVENT_START_FAILED
            | WMI_SCAN_EVENT_COMPLETED
            | WMI_SCAN_EVENT_BSS_CHANNEL
            | WMI_SCAN_EVENT_FOREIGN_CHANNEL => ScanEventAction::UnexpectedPhase,
            _ => ScanEventAction::Ignored,
        }
    }

    fn finish(&mut self) {
        self.phase = ScanPhase::Idle;
        self.requestor_id = 0;
        self.scan_id = 0;
        self.vdev_id = 0;
    }
}

impl Default for ScanSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanEvent {
    pub const fn kind(self) -> Option<ScanEventKind> {
        match self.event_type {
            WMI_SCAN_EVENT_STARTED => Some(ScanEventKind::Started),
            WMI_SCAN_EVENT_COMPLETED => Some(ScanEventKind::Completed),
            WMI_SCAN_EVENT_BSS_CHANNEL => Some(ScanEventKind::BssChannel),
            WMI_SCAN_EVENT_FOREIGN_CHANNEL => Some(ScanEventKind::ForeignChannel),
            WMI_SCAN_EVENT_DEQUEUED => Some(ScanEventKind::Dequeued),
            WMI_SCAN_EVENT_PREEMPTED => Some(ScanEventKind::Preempted),
            WMI_SCAN_EVENT_START_FAILED => Some(ScanEventKind::StartFailed),
            WMI_SCAN_EVENT_RESTARTED => Some(ScanEventKind::Restarted),
            WMI_SCAN_EVENT_FOREIGN_CHANNEL_EXIT => Some(ScanEventKind::ForeignChannelExit),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    ScanIdOutOfRange,
    RequestorIdOutOfRange,
    PriorityOutOfRange,
    TooManyChannels,
    InvalidChannelFrequency,
    TooManySsids,
    SsidTooLong,
    TooManyBssids,
    TooManyInformationElementBytes,
    MalformedInformationElements,
    ScanEventTooShort,
    LengthOverflow,
    OutputTooSmall,
    ScanAlreadyActive,
    ScanNotStarting,
    ScanNotRunning,
    ScanAlreadyAborting,
    ScanNotAborting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartScanRequest<'a> {
    /// Host scan ID; the source-defined `0xA000` prefix is added by the encoder.
    pub scan_id: u16,
    /// Host requestor ID; the source-defined `0xA000` prefix is added by the encoder.
    pub requestor_id: u16,
    pub vdev_id: u32,
    /// WMI scan priority in the inclusive range 0 through 4.
    pub priority: u32,
    pub notify_scan_events: u32,
    pub active_dwell_ms: u32,
    pub passive_dwell_ms: u32,
    pub min_rest_ms: u32,
    pub max_rest_ms: u32,
    pub repeat_probe_ms: u32,
    pub probe_spacing_ms: u32,
    pub idle_ms: u32,
    pub max_scan_ms: u32,
    pub probe_delay_ms: u32,
    pub scan_control_flags: u32,
    /// Frequencies in MHz, already filtered by the caller's regulatory policy.
    pub channels_mhz: &'a [u16],
    pub ssids: &'a [&'a [u8]],
    pub bssids: &'a [[u8; 6]],
    /// A complete sequence of 802.11 information elements.
    pub extra_information_elements: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartScanLayout {
    pub command_id: u32,
    pub payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopScanLayout {
    pub command_id: u32,
    pub payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    NotWmiControlEndpoint,
    Scan(Error),
    Transport(WmiTxError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedScanStart {
    pub command: StartScanLayout,
    pub transfer: QueuedTransfer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedScanStop {
    pub command: StopScanLayout,
    pub transfer: QueuedTransfer,
}

impl<'a> StartScanRequest<'a> {
    /// Use the common values selected by pinned upstream ath10k.
    ///
    /// The caller supplies regulatory-approved channel frequencies and SSIDs;
    /// an empty SSID list requests passive scanning as Linux does. This only
    /// constructs policy data and does not start a scan.
    pub const fn linux_reference_defaults(
        vdev_id: u32,
        channels_mhz: &'a [u16],
        ssids: &'a [&'a [u8]],
        extra_information_elements: &'a [u8],
    ) -> Self {
        let scan_control_flags = if ssids.is_empty() {
            WMI_SCAN_CHAN_STAT_EVENT | WMI_SCAN_FLAG_PASSIVE
        } else {
            WMI_SCAN_CHAN_STAT_EVENT
        };
        Self {
            scan_id: LINUX_SCAN_ID,
            requestor_id: LINUX_SCAN_REQUESTOR_ID,
            vdev_id,
            priority: LINUX_SCAN_PRIORITY_LOW,
            notify_scan_events: LINUX_SCAN_NOTIFY_EVENTS,
            active_dwell_ms: 50,
            passive_dwell_ms: 150,
            min_rest_ms: 50,
            max_rest_ms: 500,
            repeat_probe_ms: 0,
            probe_spacing_ms: 0,
            idle_ms: 0,
            max_scan_ms: 20_000,
            probe_delay_ms: 5,
            scan_control_flags,
            channels_mhz,
            ssids,
            bssids: &BROADCAST_BSSID_LIST,
            extra_information_elements,
        }
    }
}

/// Decode the fixed WMI 10.4 scan event prefix, preserving unknown enum values.
/// Any trailing bytes are ignored, matching ath10k's pull routine.
pub fn parse_scan_event(input: &[u8]) -> Result<ScanEvent, Error> {
    if input.len() < WMI_SCAN_EVENT_BYTES {
        return Err(Error::ScanEventTooShort);
    }
    Ok(ScanEvent {
        event_type: read_u32(input, 0),
        reason: read_u32(input, 4),
        channel_frequency_mhz: read_u32(input, 8),
        requestor_id: read_u32(input, 12),
        scan_id: read_u32(input, 16),
        vdev_id: read_u32(input, 20),
    })
}

/// Validate the request and return the exact variable-length command size.
pub fn start_scan_payload_len(request: &StartScanRequest<'_>) -> Result<usize, Error> {
    if request.scan_id > MAX_HOST_SCAN_ID {
        return Err(Error::ScanIdOutOfRange);
    }
    if request.requestor_id > MAX_HOST_SCAN_ID {
        return Err(Error::RequestorIdOutOfRange);
    }
    if request.priority > MAX_SCAN_PRIORITY {
        return Err(Error::PriorityOutOfRange);
    }
    if request.channels_mhz.len() > MAX_SCAN_CHANNELS {
        return Err(Error::TooManyChannels);
    }
    if request.channels_mhz.iter().any(|frequency| *frequency == 0) {
        return Err(Error::InvalidChannelFrequency);
    }
    if request.ssids.len() > MAX_SCAN_SSIDS {
        return Err(Error::TooManySsids);
    }
    if request.ssids.iter().any(|ssid| ssid.len() > MAX_SSID_BYTES) {
        return Err(Error::SsidTooLong);
    }
    if request.bssids.len() > MAX_SCAN_BSSIDS {
        return Err(Error::TooManyBssids);
    }
    let ie_bytes = request.extra_information_elements.len();
    if ie_bytes > MAX_SCAN_IE_BYTES {
        return Err(Error::TooManyInformationElementBytes);
    }
    if InformationElementIter::new(request.extra_information_elements)
        .any(|element| element.is_err())
    {
        return Err(Error::MalformedInformationElements);
    }

    let mut length = FIXED_COMMAND_BYTES;
    if !request.channels_mhz.is_empty() {
        length = add(length, LIST_HEADER_BYTES)?;
        length = add(
            length,
            multiply(request.channels_mhz.len(), CHANNEL_ENTRY_BYTES)?,
        )?;
    }
    if !request.ssids.is_empty() {
        length = add(length, LIST_HEADER_BYTES)?;
        length = add(length, multiply(request.ssids.len(), SSID_ENTRY_BYTES)?)?;
    }
    if !request.bssids.is_empty() {
        length = add(length, LIST_HEADER_BYTES)?;
        length = add(length, multiply(request.bssids.len(), MAC_ADDRESS_BYTES)?)?;
    }
    if ie_bytes != 0 {
        length = add(length, LIST_HEADER_BYTES)?;
        length = add(length, round_up_four(ie_bytes)?)?;
    }
    Ok(length)
}

/// Encode one WMI 10.4 start-scan command body. `output` is unchanged on error.
pub fn encode_start_scan(
    request: &StartScanRequest<'_>,
    output: &mut [u8],
) -> Result<StartScanLayout, Error> {
    let payload_bytes = start_scan_payload_len(request)?;
    if output.len() < payload_bytes {
        return Err(Error::OutputTooSmall);
    }

    let command = &mut output[..payload_bytes];
    command.fill(0);
    let mut offset = 0;
    for value in [
        HOST_SCAN_ID_PREFIX | u32::from(request.scan_id),
        HOST_SCAN_ID_PREFIX | u32::from(request.requestor_id),
        request.vdev_id,
        request.priority,
        request.notify_scan_events,
        request.active_dwell_ms,
        request.passive_dwell_ms,
        request.min_rest_ms,
        request.max_rest_ms,
        request.repeat_probe_ms,
        request.probe_spacing_ms,
        request.idle_ms,
        request.max_scan_ms,
        request.probe_delay_ms,
        request.scan_control_flags,
        0, // ath10k_wmi_op_gen_start_scan sets burst_duration_ms to zero.
    ] {
        write_u32(command, &mut offset, value);
    }

    if !request.channels_mhz.is_empty() {
        write_list_header(
            command,
            &mut offset,
            CHANNEL_LIST_TAG,
            request.channels_mhz.len(),
        );
        for frequency in request.channels_mhz {
            command[offset..offset + 2].copy_from_slice(&frequency.to_le_bytes());
            offset += CHANNEL_ENTRY_BYTES; // PHY mode and reserved bytes stay zero.
        }
    }

    if !request.ssids.is_empty() {
        write_list_header(command, &mut offset, SSID_LIST_TAG, request.ssids.len());
        for ssid in request.ssids {
            write_u32(command, &mut offset, ssid.len() as u32);
            command[offset..offset + ssid.len()].copy_from_slice(ssid);
            offset += MAX_SSID_BYTES;
        }
    }

    if !request.bssids.is_empty() {
        write_list_header(command, &mut offset, BSSID_LIST_TAG, request.bssids.len());
        for bssid in request.bssids {
            command[offset..offset + bssid.len()].copy_from_slice(bssid);
            offset += MAC_ADDRESS_BYTES; // Two word-padding bytes remain zero.
        }
    }

    if !request.extra_information_elements.is_empty() {
        write_list_header(
            command,
            &mut offset,
            IE_TAG,
            request.extra_information_elements.len(),
        );
        command[offset..offset + request.extra_information_elements.len()]
            .copy_from_slice(request.extra_information_elements);
        offset += round_up_four(request.extra_information_elements.len())?;
    }

    debug_assert_eq!(offset, payload_bytes);
    Ok(StartScanLayout {
        command_id: WMI_10_4_START_SCAN_COMMAND_ID,
        payload_bytes,
    })
}

/// Encode and queue one start-scan command through a negotiated WMI_CONTROL
/// endpoint. The scan slot is reserved before queue submission and released if
/// the software transport cannot accept the command. CE/IOVA fields remain a
/// plan; this function does not publish DMA or start hardware scanning.
#[allow(clippy::too_many_arguments)]
pub fn queue_start_scan<const RING_SIZE: usize>(
    session: &mut ScanSession,
    queue: &mut WmiCommandTxQueue<RING_SIZE>,
    request: &StartScanRequest<'_>,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    htc_frame_output: &mut [u8],
) -> Result<QueuedScanStart, QueueError> {
    if !queue.is_wmi_control_bound() {
        return Err(QueueError::NotWmiControlEndpoint);
    }
    session.reserve_start(request).map_err(QueueError::Scan)?;

    let mut command_body = [0u8; MAX_START_SCAN_COMMAND_BYTES];
    let command = match encode_start_scan(request, &mut command_body) {
        Ok(command) => command,
        Err(error) => {
            // `reserve_start` performs the same input validation. Keep the
            // rollback explicit so future encoder checks cannot strand a slot.
            if let Err(rollback_error) = session.submission_failed() {
                return Err(QueueError::Scan(rollback_error));
            }
            return Err(QueueError::Scan(error));
        }
    };
    let transfer = match queue.queue_transfer(
        command.command_id,
        &command_body[..command.payload_bytes],
        ce_iova,
        transfer_id,
        context,
        false,
        false,
        htc_frame_output,
    ) {
        Ok(transfer) => transfer,
        Err(error) => {
            if let Err(rollback_error) = session.submission_failed() {
                return Err(QueueError::Scan(rollback_error));
            }
            return Err(QueueError::Transport(error));
        }
    };

    Ok(QueuedScanStart { command, transfer })
}

/// Encode the four-word WMI 10.4 `STOP_SCAN` body for the session currently
/// marked aborting. The one-scan form uses the active requestor, scan, and
/// vdev IDs; output is unchanged on error.
pub fn encode_stop_scan(session: &ScanSession, output: &mut [u8]) -> Result<StopScanLayout, Error> {
    if session.phase != ScanPhase::Aborting {
        return Err(Error::ScanNotRunning);
    }
    if output.len() < STOP_SCAN_COMMAND_BYTES {
        return Err(Error::OutputTooSmall);
    }
    for (index, value) in [
        session.requestor_id,
        session.scan_id,
        WMI_SCAN_STOP_ONE,
        session.vdev_id,
    ]
    .into_iter()
    .enumerate()
    {
        output[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(StopScanLayout {
        command_id: WMI_10_4_STOP_SCAN_COMMAND_ID,
        payload_bytes: STOP_SCAN_COMMAND_BYTES,
    })
}

/// Queue the exact matching `STOP_SCAN` request on WMI_CONTROL. A transport
/// rejection restores the session to `Running`; successful software enqueue
/// leaves it `Aborting` until the matching terminal event. This plans a CE
/// transfer only; it does not publish DMA or stop a live radio scan.
#[allow(clippy::too_many_arguments)]
pub fn queue_stop_scan<const RING_SIZE: usize>(
    session: &mut ScanSession,
    queue: &mut WmiCommandTxQueue<RING_SIZE>,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    htc_frame_output: &mut [u8],
) -> Result<QueuedScanStop, QueueError> {
    if !queue.is_wmi_control_bound() {
        return Err(QueueError::NotWmiControlEndpoint);
    }
    session.mark_abort_requested().map_err(QueueError::Scan)?;

    let mut command_body = [0u8; STOP_SCAN_COMMAND_BYTES];
    let command = match encode_stop_scan(session, &mut command_body) {
        Ok(command) => command,
        Err(error) => {
            if let Err(rollback_error) = session.stop_submission_failed() {
                return Err(QueueError::Scan(rollback_error));
            }
            return Err(QueueError::Scan(error));
        }
    };
    let transfer = match queue.queue_transfer(
        command.command_id,
        &command_body[..command.payload_bytes],
        ce_iova,
        transfer_id,
        context,
        false,
        false,
        htc_frame_output,
    ) {
        Ok(transfer) => transfer,
        Err(error) => {
            if let Err(rollback_error) = session.stop_submission_failed() {
                return Err(QueueError::Scan(rollback_error));
            }
            return Err(QueueError::Transport(error));
        }
    };

    Ok(QueuedScanStop { command, transfer })
}

fn write_list_header(output: &mut [u8], offset: &mut usize, tag: u32, count: usize) {
    write_u32(output, offset, tag);
    write_u32(output, offset, count as u32);
}

fn write_u32(output: &mut [u8], offset: &mut usize, value: u32) {
    output[*offset..*offset + 4].copy_from_slice(&value.to_le_bytes());
    *offset += 4;
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn round_up_four(value: usize) -> Result<usize, Error> {
    value
        .checked_add(3)
        .map(|aligned| aligned & !3)
        .ok_or(Error::LengthOverflow)
}

fn add(left: usize, right: usize) -> Result<usize, Error> {
    left.checked_add(right).ok_or(Error::LengthOverflow)
}

fn multiply(left: usize, right: usize) -> Result<usize, Error> {
    left.checked_mul(right).ok_or(Error::LengthOverflow)
}
