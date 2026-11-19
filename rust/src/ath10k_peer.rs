//! Host-side HTT peer-map tracking; no hardware publication or radio access.
#![forbid(unsafe_code)]

use crate::ath10k_htt::{HttPeerMap, HttPeerUnmap};
use crate::ath10k_vdev::{self, PeerType, QueueError, StationCommand};
use crate::ath10k_wmi_tx::{QueuedTransfer, WmiCommandTxQueue};

/// Linux `ATH10K_MAX_NUM_PEER_IDS`, bounded by the HTT RX descriptor field.
pub const MAX_PEER_IDS: usize = 2048;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerError {
    InvalidGeneration,
    InvalidMac,
    WrongGeneration,
    AlreadyUsed,
    NotQueued,
    WrongTransfer,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerAction {
    Ignored,
    Mapped { peer_id: u16, first: bool },
    Duplicate { peer_id: u16 },
    Unmapped { peer_id: u16, remaining: bool },
    InvalidPeerId(u16),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Idle,
    Queued { slot: usize, transfer_id: u16 },
    Awaiting,
    Mapped,
    Unmapped,
    Failed,
}

/// One peer in one transport generation. The caller must enforce global peer-ID
/// uniqueness across peers and tag incoming events with their real generation.
#[derive(Debug, Eq, PartialEq)]
pub struct PeerTracker {
    generation: u64,
    vdev_id: u8,
    mac: [u8; 6],
    phase: Phase,
    ids: [u64; 32],
    count: u16,
}
impl PeerTracker {
    pub(crate) fn matches_identity(&self, generation: u64, vdev_id: u8, mac: [u8; 6]) -> bool {
        self.generation == generation && self.vdev_id == vdev_id && self.mac == mac
    }
    pub fn new(generation: u64, vdev_id: u8, mac: [u8; 6]) -> Result<Self, PeerError> {
        if generation == 0 {
            return Err(PeerError::InvalidGeneration);
        }
        if mac == [0; 6] || mac[0] & 1 != 0 {
            return Err(PeerError::InvalidMac);
        }
        Ok(Self {
            generation,
            vdev_id,
            mac,
            phase: Phase::Idle,
            ids: [0; 32],
            count: 0,
        })
    }
    fn can_queue(&self, generation: u64) -> Result<(), PeerError> {
        if generation != self.generation {
            return Err(PeerError::WrongGeneration);
        }
        if self.phase != Phase::Idle {
            return Err(PeerError::AlreadyUsed);
        }
        Ok(())
    }
    /// Called by the transport owner after publishing this exact CE slot.
    pub fn mark_published(
        &mut self,
        generation: u64,
        slot: usize,
        transfer_id: u16,
    ) -> Result<(), PeerError> {
        if generation != self.generation {
            return Err(PeerError::WrongGeneration);
        }
        match self.phase {
            Phase::Queued {
                slot: s,
                transfer_id: id,
            } if slot == s && transfer_id == id => {
                self.phase = Phase::Awaiting;
                Ok(())
            }
            Phase::Queued { .. } => Err(PeerError::WrongTransfer),
            _ => Err(PeerError::NotQueued),
        }
    }
    /// A matching peer-map event is required; a queue entry is not readiness.
    pub fn contains_peer_id(&self, generation: u64, peer_id: u16) -> bool {
        let id = usize::from(peer_id);
        generation == self.generation
            && self.phase == Phase::Mapped
            && id < MAX_PEER_IDS
            && self.ids[id / 64] & (1u64 << (id % 64)) != 0
    }
    /// A caller-established creation timeout consumes this generation's attempt.
    pub fn timeout(&mut self, generation: u64) -> Result<(), PeerError> {
        if generation != self.generation {
            return Err(PeerError::WrongGeneration);
        }
        if self.phase != Phase::Awaiting {
            return Err(PeerError::NotQueued);
        }
        self.phase = Phase::Failed;
        Ok(())
    }
    pub fn on_map(&mut self, generation: u64, event: HttPeerMap) -> PeerAction {
        if generation != self.generation
            || !matches!(self.phase, Phase::Awaiting | Phase::Mapped)
            || event.vdev_id != self.vdev_id
            || event.mac != self.mac
        {
            return PeerAction::Ignored;
        }
        let id = usize::from(event.peer_id);
        if id >= MAX_PEER_IDS {
            self.phase = Phase::Failed;
            return PeerAction::InvalidPeerId(event.peer_id);
        }
        let bit = 1u64 << (id % 64);
        let index = id / 64;
        if self.ids[index] & bit != 0 {
            return PeerAction::Duplicate {
                peer_id: event.peer_id,
            };
        }
        let first = self.count == 0;
        self.ids[index] |= bit;
        self.count += 1;
        self.phase = Phase::Mapped;
        PeerAction::Mapped {
            peer_id: event.peer_id,
            first,
        }
    }
    pub fn on_unmap(&mut self, generation: u64, event: HttPeerUnmap) -> PeerAction {
        if generation != self.generation || self.phase != Phase::Mapped {
            return PeerAction::Ignored;
        }
        let id = usize::from(event.peer_id);
        if id >= MAX_PEER_IDS {
            return PeerAction::Ignored;
        }
        let bit = 1u64 << (id % 64);
        let index = id / 64;
        if self.ids[index] & bit == 0 {
            return PeerAction::Ignored;
        }
        self.ids[index] &= !bit;
        self.count -= 1;
        let remaining = self.count != 0;
        if !remaining {
            self.phase = Phase::Unmapped;
        }
        PeerAction::Unmapped {
            peer_id: event.peer_id,
            remaining,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerQueueError {
    State(PeerError),
    Queue(QueueError),
}
/// Reserve a software WMI create command; map events remain gated until the
/// hardware owner reports publication of its returned descriptor slot.
pub fn queue_peer_create_tracked<const N: usize>(
    queue: &mut WmiCommandTxQueue<N>,
    tracker: &mut PeerTracker,
    generation: u64,
    kind: PeerType,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    output: &mut [u8],
) -> Result<QueuedTransfer, PeerQueueError> {
    tracker
        .can_queue(generation)
        .map_err(PeerQueueError::State)?;
    let command = StationCommand::PeerCreate {
        vdev_id: u32::from(tracker.vdev_id),
        mac: tracker.mac,
        kind,
    };
    let queued =
        ath10k_vdev::queue_station_command(queue, command, ce_iova, transfer_id, context, output)
            .map_err(PeerQueueError::Queue)?;
    tracker.phase = Phase::Queued {
        slot: queued.slot,
        transfer_id,
    };
    Ok(queued)
}
