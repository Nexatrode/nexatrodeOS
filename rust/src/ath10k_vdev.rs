//! WMI 10.4 station-vdev and peer command bodies for the ath10k host path.
//!
//! Channel words must come from a validated channel and regulatory policy.
//! These encoders do not submit commands, create a peer, or start a radio.
#![forbid(unsafe_code)]

use crate::ath10k_wmi_tx::{Error as TxError, QueuedTransfer, WmiCommandTxQueue};

pub const WMI_10_4_VDEV_CREATE: u32 = 0x9013;
pub const WMI_10_4_VDEV_DELETE: u32 = 0x9014;
pub const WMI_10_4_VDEV_START: u32 = 0x9015;
pub const WMI_10_4_VDEV_UP: u32 = 0x9017;
pub const WMI_10_4_VDEV_STOP: u32 = 0x9018;
pub const WMI_10_4_VDEV_DOWN: u32 = 0x9019;
pub const WMI_10_4_PEER_CREATE: u32 = 0x9022;
pub const WMI_10_4_PEER_DELETE: u32 = 0x9023;
pub const WMI_10_4_PEER_ASSOC: u32 = 0x9026;
pub const WMI_10_4_VDEV_START_RESPONSE: u32 = 0x9004;
pub const STATION_START_BYTES: usize = 128;
pub const PEER_ASSOC_BYTES: usize = 356;
pub const MAX_PEER_RATES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    OutputTooSmall,
    InvalidMac,
    TruncatedEvent,
    TooManyRates,
    InvalidMpduDensity,
}

/// Exact six little-endian words of Linux `struct wmi_channel`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelWords {
    pub frequency: u32,
    pub center1: u32,
    pub center2: u32,
    pub flags: u32,
    pub regulatory0: u32,
    pub regulatory1: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PeerType {
    Default = 0,
    Bss = 1,
    Tdls = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartResponse {
    pub vdev_id: u32,
    pub requestor_id: u32,
    pub response_type: u32,
    pub status: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartError {
    InvalidGeneration,
    WrongGeneration,
    WrongVdev,
    AlreadyUsed,
    NotQueued,
    WrongTransfer,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartAction {
    Ignored,
    Started,
    Failed(u32),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartPhase {
    Idle,
    Queued { slot: usize, transfer_id: u16 },
    Pending,
    Started,
    Failed,
}

/// Correlates one start within a transport generation. The owner must ensure
/// events carry their actual transport generation and must not create another
/// tracker for the same vdev in that generation: the wire response has no
/// generation or unique host transaction token.
#[derive(Debug, Eq, PartialEq)]
pub struct StartTracker {
    generation: u64,
    vdev_id: u32,
    phase: StartPhase,
}
impl StartTracker {
    pub fn new(generation: u64, vdev_id: u32) -> Result<Self, StartError> {
        if generation == 0 {
            return Err(StartError::InvalidGeneration);
        }
        Ok(Self {
            generation,
            vdev_id,
            phase: StartPhase::Idle,
        })
    }
    fn can_queue(&self, generation: u64, vdev_id: u32) -> Result<(), StartError> {
        if generation != self.generation {
            return Err(StartError::WrongGeneration);
        }
        if vdev_id != self.vdev_id {
            return Err(StartError::WrongVdev);
        }
        if self.phase != StartPhase::Idle {
            return Err(StartError::AlreadyUsed);
        }
        Ok(())
    }
    /// The transport owner calls this after publishing the matching CE entry.
    /// A software-queue reservation alone must not enable response acceptance.
    pub fn mark_published(
        &mut self,
        generation: u64,
        slot: usize,
        transfer_id: u16,
    ) -> Result<(), StartError> {
        if generation != self.generation {
            return Err(StartError::WrongGeneration);
        }
        match self.phase {
            StartPhase::Queued {
                slot: expected_slot,
                transfer_id: expected_id,
            } if slot == expected_slot && transfer_id == expected_id => {
                self.phase = StartPhase::Pending;
                Ok(())
            }
            StartPhase::Queued { .. } => Err(StartError::WrongTransfer),
            _ => Err(StartError::NotQueued),
        }
    }
    pub fn on_response(
        &mut self,
        generation: u64,
        event_id: u32,
        response: StartResponse,
    ) -> StartAction {
        if self.phase != StartPhase::Pending
            || generation != self.generation
            || event_id != WMI_10_4_VDEV_START_RESPONSE
            || response.vdev_id != self.vdev_id
            || response.requestor_id != 0
            || response.response_type != 0
        {
            return StartAction::Ignored;
        }
        if response.status == 0 {
            self.phase = StartPhase::Started;
            StartAction::Started
        } else {
            self.phase = StartPhase::Failed;
            StartAction::Failed(response.status)
        }
    }
    /// Mark a caller-established timeout terminal for this attempt.
    pub fn timeout(&mut self, generation: u64) -> Result<(), StartError> {
        if generation != self.generation {
            return Err(StartError::WrongGeneration);
        }
        if self.phase != StartPhase::Pending {
            return Err(StartError::NotQueued);
        }
        self.phase = StartPhase::Failed;
        Ok(())
    }
}

/// Fields must be derived from the negotiated station, AP and local policy;
/// the wire encoder does not decide which capabilities may be advertised.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerAssoc<'a> {
    pub mac: [u8; 6],
    pub vdev_id: u32,
    pub reassoc: bool,
    pub association_id: u16,
    pub flags: u32,
    pub capabilities: u16,
    pub listen_interval: u32,
    pub ht_capabilities: u32,
    pub max_mpdu: u32,
    pub mpdu_density: u32,
    pub rate_capabilities: u32,
    pub legacy_rates: &'a [u8],
    pub ht_rates: &'a [u8],
    pub spatial_streams: u32,
    pub vht_capabilities: u32,
    pub phy_mode: u32,
    pub vht_rates: [u32; 4],
    pub bandwidth_rxnss_override: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationCommand {
    Create {
        vdev_id: u32,
        mac: [u8; 6],
    },
    Start {
        vdev_id: u32,
        channel: ChannelWords,
        beacon_interval: u32,
        dtim_period: u32,
        pmf: bool,
    },
    PeerCreate {
        vdev_id: u32,
        mac: [u8; 6],
        kind: PeerType,
    },
    PeerDelete {
        vdev_id: u32,
        mac: [u8; 6],
    },
    Up {
        vdev_id: u32,
        association_id: u32,
        bssid: [u8; 6],
    },
    Down {
        vdev_id: u32,
    },
    Stop {
        vdev_id: u32,
    },
    Delete {
        vdev_id: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    NotWmiControl,
    Encode(Error),
    Transport(TxError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedStartError {
    NotStart,
    State(StartError),
    Queue(QueueError),
}

/// Reserve the host start attempt only after the software queue accepts it.
pub fn queue_tracked_start<const N: usize>(
    queue: &mut WmiCommandTxQueue<N>,
    tracker: &mut StartTracker,
    generation: u64,
    command: StationCommand,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    output: &mut [u8],
) -> Result<QueuedTransfer, TrackedStartError> {
    let vdev_id = match command {
        StationCommand::Start { vdev_id, .. } => vdev_id,
        _ => return Err(TrackedStartError::NotStart),
    };
    tracker
        .can_queue(generation, vdev_id)
        .map_err(TrackedStartError::State)?;
    let queued = queue_station_command(queue, command, ce_iova, transfer_id, context, output)
        .map_err(TrackedStartError::Queue)?;
    // An accepted software slot is not evidence that a descriptor was published.
    tracker.phase = StartPhase::Queued {
        slot: queued.slot,
        transfer_id,
    };
    Ok(queued)
}

/// Encode WMI 10.4's common peer body, 10.2 info0 and 10.4 bandwidth word.
pub fn encode_peer_assoc(request: PeerAssoc<'_>, out: &mut [u8]) -> Result<usize, Error> {
    if !unicast(request.mac) {
        return Err(Error::InvalidMac);
    }
    if request.legacy_rates.len() > MAX_PEER_RATES || request.ht_rates.len() > MAX_PEER_RATES {
        return Err(Error::TooManyRates);
    }
    if request.mpdu_density > 16 {
        return Err(Error::InvalidMpduDensity);
    }
    require(out, PEER_ASSOC_BYTES)?;
    let body = &mut out[..PEER_ASSOC_BYTES];
    body.fill(0);
    body[..6].copy_from_slice(&request.mac);
    for (at, value) in [
        (8, request.vdev_id),
        (12, u32::from(!request.reassoc)),
        (16, u32::from(request.association_id)),
        (20, request.flags),
        (24, u32::from(request.capabilities)),
        (28, request.listen_interval),
        (32, request.ht_capabilities),
        (36, request.max_mpdu),
        (40, request.mpdu_density),
        (44, request.rate_capabilities),
        (48, request.legacy_rates.len() as u32),
        (184, request.ht_rates.len() as u32),
        (320, request.spatial_streams),
        (324, request.vht_capabilities),
        (328, request.phy_mode),
        (348, 0xff),
        (352, request.bandwidth_rxnss_override),
    ] {
        word(body, at, value);
    }
    body[52..52 + request.legacy_rates.len()].copy_from_slice(request.legacy_rates);
    body[188..188 + request.ht_rates.len()].copy_from_slice(request.ht_rates);
    for (index, value) in request.vht_rates.into_iter().enumerate() {
        word(body, 332 + index * 4, value);
    }
    Ok(PEER_ASSOC_BYTES)
}

/// Queue a peer-association body on a negotiated WMI_CONTROL endpoint.
pub fn queue_peer_assoc<const N: usize>(
    queue: &mut WmiCommandTxQueue<N>,
    request: PeerAssoc<'_>,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    output: &mut [u8],
) -> Result<QueuedTransfer, QueueError> {
    if !queue.is_wmi_control_bound() {
        return Err(QueueError::NotWmiControl);
    }
    let mut body = [0u8; PEER_ASSOC_BYTES];
    let length = encode_peer_assoc(request, &mut body).map_err(QueueError::Encode)?;
    queue
        .queue_transfer(
            WMI_10_4_PEER_ASSOC,
            &body[..length],
            ce_iova,
            transfer_id,
            context,
            false,
            false,
            output,
        )
        .map_err(QueueError::Transport)
}

/// Prepare an HTC/CE software queue entry. Hardware publication and response
/// correlation are separate operations owned by the driver.
pub fn queue_station_command<const N: usize>(
    queue: &mut WmiCommandTxQueue<N>,
    command: StationCommand,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    output: &mut [u8],
) -> Result<QueuedTransfer, QueueError> {
    if !queue.is_wmi_control_bound() {
        return Err(QueueError::NotWmiControl);
    }
    let mut body = [0u8; STATION_START_BYTES];
    let (id, length) = match command {
        StationCommand::Create { vdev_id, mac } => (
            WMI_10_4_VDEV_CREATE,
            encode_station_create(vdev_id, mac, &mut body),
        ),
        StationCommand::Start {
            vdev_id,
            channel,
            beacon_interval,
            dtim_period,
            pmf,
        } => (
            WMI_10_4_VDEV_START,
            encode_station_start(
                vdev_id,
                channel,
                beacon_interval,
                dtim_period,
                pmf,
                &mut body,
            ),
        ),
        StationCommand::PeerCreate { vdev_id, mac, kind } => (
            WMI_10_4_PEER_CREATE,
            encode_peer_create(vdev_id, mac, kind, &mut body),
        ),
        StationCommand::PeerDelete { vdev_id, mac } => (
            WMI_10_4_PEER_DELETE,
            encode_peer_delete(vdev_id, mac, &mut body),
        ),
        StationCommand::Up {
            vdev_id,
            association_id,
            bssid,
        } => (
            WMI_10_4_VDEV_UP,
            encode_vdev_up(vdev_id, association_id, bssid, &mut body),
        ),
        StationCommand::Down { vdev_id } => {
            (WMI_10_4_VDEV_DOWN, encode_vdev_id(vdev_id, &mut body))
        }
        StationCommand::Stop { vdev_id } => {
            (WMI_10_4_VDEV_STOP, encode_vdev_id(vdev_id, &mut body))
        }
        StationCommand::Delete { vdev_id } => {
            (WMI_10_4_VDEV_DELETE, encode_vdev_id(vdev_id, &mut body))
        }
    };
    let length = length.map_err(QueueError::Encode)?;
    queue
        .queue_transfer(
            id,
            &body[..length],
            ce_iova,
            transfer_id,
            context,
            false,
            false,
            output,
        )
        .map_err(QueueError::Transport)
}

fn unicast(mac: [u8; 6]) -> bool {
    mac[0] & 1 == 0 && mac != [0; 6]
}
fn word(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn require(out: &[u8], size: usize) -> Result<(), Error> {
    if out.len() < size {
        Err(Error::OutputTooSmall)
    } else {
        Ok(())
    }
}

pub fn encode_station_create(vdev_id: u32, mac: [u8; 6], out: &mut [u8]) -> Result<usize, Error> {
    if !unicast(mac) {
        return Err(Error::InvalidMac);
    }
    require(out, 20)?;
    let body = &mut out[..20];
    body.fill(0);
    word(body, 0, vdev_id);
    word(body, 4, 2); // WMI_VDEV_TYPE_STA
    body[12..18].copy_from_slice(&mac);
    Ok(20)
}

/// Linux's station start uses zero SSID, NOA, beacon TX, and requestor fields.
pub fn encode_station_start(
    vdev_id: u32,
    channel: ChannelWords,
    beacon_interval: u32,
    dtim_period: u32,
    pmf: bool,
    out: &mut [u8],
) -> Result<usize, Error> {
    require(out, STATION_START_BYTES)?;
    let body = &mut out[..STATION_START_BYTES];
    body.fill(0);
    for (i, value) in [
        channel.frequency,
        channel.center1,
        channel.center2,
        channel.flags,
        channel.regulatory0,
        channel.regulatory1,
    ]
    .into_iter()
    .enumerate()
    {
        word(body, i * 4, value);
    }
    word(body, 24, vdev_id);
    word(body, 32, beacon_interval);
    word(body, 36, dtim_period);
    word(body, 40, if pmf { 2 } else { 0 });
    Ok(STATION_START_BYTES)
}

pub fn encode_peer_create(
    vdev_id: u32,
    mac: [u8; 6],
    kind: PeerType,
    out: &mut [u8],
) -> Result<usize, Error> {
    if !unicast(mac) {
        return Err(Error::InvalidMac);
    }
    require(out, 16)?;
    let body = &mut out[..16];
    body.fill(0);
    word(body, 0, vdev_id);
    body[4..10].copy_from_slice(&mac);
    word(body, 12, kind as u32);
    Ok(16)
}

pub fn encode_peer_delete(vdev_id: u32, mac: [u8; 6], out: &mut [u8]) -> Result<usize, Error> {
    if !unicast(mac) {
        return Err(Error::InvalidMac);
    }
    require(out, 12)?;
    let body = &mut out[..12];
    body.fill(0);
    word(body, 0, vdev_id);
    body[4..10].copy_from_slice(&mac);
    Ok(12)
}

/// Shared packed body for VDEV_DOWN, VDEV_STOP and VDEV_DELETE.
pub fn encode_vdev_id(vdev_id: u32, out: &mut [u8]) -> Result<usize, Error> {
    require(out, 4)?;
    word(out, 0, vdev_id);
    Ok(4)
}

pub fn encode_vdev_up(
    vdev_id: u32,
    association_id: u32,
    bssid: [u8; 6],
    out: &mut [u8],
) -> Result<usize, Error> {
    if !unicast(bssid) {
        return Err(Error::InvalidMac);
    }
    require(out, 16)?;
    let body = &mut out[..16];
    body.fill(0);
    word(body, 0, vdev_id);
    word(body, 4, association_id);
    body[8..14].copy_from_slice(&bssid);
    Ok(16)
}

/// Decode the event body; the caller must correlate event ID, vdev, type and status.
pub fn decode_start_response(event: &[u8]) -> Result<StartResponse, Error> {
    if event.len() < 16 {
        return Err(Error::TruncatedEvent);
    }
    let read = |at| u32::from_le_bytes(event[at..at + 4].try_into().unwrap());
    Ok(StartResponse {
        vdev_id: read(0),
        requestor_id: read(4),
        response_type: read(8),
        status: read(12),
    })
}
