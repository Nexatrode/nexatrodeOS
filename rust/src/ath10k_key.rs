//! Bounded WMI 10.4 CCMP key-install encoding for the ath10k host path.
//!
//! This serializes the legacy WMI command used by the pinned Sunfish ath10k
//! source. It does not transfer key ownership, publish DMA, transmit the
//! command, wait for the firmware security indication, or authorize data.
#![forbid(unsafe_code)]

use crate::ath10k_htt::{HTT_SECURITY_TYPE_AES_CCMP, HttSecurityIndication};
use crate::ath10k_wmi_tx::{Error as WmiTxError, QueuedTransfer, WmiCommandTxQueue};
use crate::wlan_key_confirmation::KeyInstallConfirmation;

pub const WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID: u32 = 0x901d;
pub const VDEV_INSTALL_KEY_FIXED_BYTES: usize = 92;
pub const CCMP128_KEY_BYTES: usize = 16;
pub const WMI_MAX_KEY_INDEX: u8 = 3;

const KEY_FLAGS_PAIRWISE: u32 = 0;
const KEY_FLAGS_GROUP: u32 = 1;
const CIPHER_AES_CCM: u32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyKind {
    Pairwise,
    Group,
}

/// Require the expected peer, CCMP security type, and unicast/group indication
/// before treating an HTT security indication as a match for this key kind.
/// This predicate alone does not correlate a queued command; use
/// `CcmpKeyInstallTracker` for that, and keep the data gate closed until every
/// required key and M4 are confirmed.
pub const fn security_indication_confirms_ccmp_install(
    indication: HttSecurityIndication,
    expected_peer_id: u16,
    kind: KeyKind,
) -> bool {
    let expected_unicast = matches!(kind, KeyKind::Pairwise);
    indication.peer_id == expected_peer_id
        && indication.security_type == HTT_SECURITY_TYPE_AES_CCMP
        && indication.is_unicast == expected_unicast
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CcmpKeyInstall<'a> {
    pub vdev_id: u32,
    pub peer_address: [u8; 6],
    pub key_index: u8,
    pub kind: KeyKind,
    pub temporal_key: &'a [u8; CCMP128_KEY_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    KeyIndexOutOfRange,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLayout {
    pub command_id: u32,
    pub body_bytes: usize,
    pub vdev_id: u32,
    pub peer_address: [u8; 6],
    pub key_index: u8,
    pub kind: KeyKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    NotWmiControlEndpoint,
    Key(Error),
    Transport(WmiTxError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedKeyInstall {
    pub command: CommandLayout,
    pub transfer: QueuedTransfer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerError {
    InvalidKeyIndex,
    StaleGeneration,
    CommandEncodingMismatch,
    CommandIdentityMismatch,
    KeyIndexMismatch,
    UnexpectedKeyKind,
    DuplicateQueuedCommand,
    NotQueuedForPublication,
    PublicationMismatch,
    DuplicatePublication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyInstallAction {
    IgnoredStaleGeneration,
    IgnoredMismatch,
    IgnoredUnexpectedKind,
    IgnoredBeforeQueue,
    IgnoredBeforePublication,
    Duplicate {
        kind: KeyKind,
    },
    Installed {
        kind: KeyKind,
        all_required_installed: bool,
    },
}

/// Correlates one association generation's queued CCMP key commands with the
/// firmware SEC_IND events that confirm installation. It does not install
/// keys, publish DMA, or authorize the WPA2 data path by itself.
#[derive(Debug, Eq, PartialEq)]
pub struct CcmpKeyInstallTracker {
    generation: u32,
    vdev_id: u32,
    peer_id: u16,
    peer_address: [u8; 6],
    pairwise_key_index: u8,
    group_key_index: Option<u8>,
    pairwise_queued: bool,
    group_queued: bool,
    pairwise_transfer: Option<QueuedTransfer>,
    group_transfer: Option<QueuedTransfer>,
    pairwise_published: bool,
    group_published: bool,
    pairwise_installed: bool,
    group_installed: bool,
}

impl CcmpKeyInstallTracker {
    pub(crate) fn matches_peer(
        &self,
        generation: u32,
        vdev_id: u8,
        peer_id: u16,
        peer_address: [u8; 6],
    ) -> bool {
        self.generation == generation
            && self.vdev_id == u32::from(vdev_id)
            && self.peer_id == peer_id
            && self.peer_address == peer_address
    }
    pub fn new(
        generation: u32,
        vdev_id: u32,
        peer_id: u16,
        peer_address: [u8; 6],
        pairwise_key_index: u8,
        group_key_index: Option<u8>,
    ) -> Result<Self, TrackerError> {
        if pairwise_key_index > WMI_MAX_KEY_INDEX
            || group_key_index.is_some_and(|index| index > WMI_MAX_KEY_INDEX)
        {
            return Err(TrackerError::InvalidKeyIndex);
        }
        Ok(Self {
            generation,
            vdev_id,
            peer_id,
            peer_address,
            pairwise_key_index,
            group_key_index,
            pairwise_queued: false,
            group_queued: false,
            pairwise_transfer: None,
            group_transfer: None,
            pairwise_published: false,
            group_published: false,
            pairwise_installed: false,
            group_installed: false,
        })
    }

    /// Record only a successfully queued command with the exact association
    /// identity and planned key index. A failed queue operation leaves this
    /// tracker unchanged.
    pub fn record_queued(
        &mut self,
        generation: u32,
        queued: QueuedKeyInstall,
    ) -> Result<(), TrackerError> {
        if generation != self.generation {
            return Err(TrackerError::StaleGeneration);
        }
        if queued.command.command_id != WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID
            || queued.command.body_bytes != VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES
        {
            return Err(TrackerError::CommandEncodingMismatch);
        }
        if queued.command.vdev_id != self.vdev_id
            || queued.command.peer_address != self.peer_address
        {
            return Err(TrackerError::CommandIdentityMismatch);
        }

        match queued.command.kind {
            KeyKind::Pairwise => {
                if queued.command.key_index != self.pairwise_key_index {
                    return Err(TrackerError::KeyIndexMismatch);
                }
                if self.pairwise_queued {
                    return Err(TrackerError::DuplicateQueuedCommand);
                }
                self.pairwise_queued = true;
                self.pairwise_transfer = Some(queued.transfer);
            }
            KeyKind::Group => {
                let expected = self
                    .group_key_index
                    .ok_or(TrackerError::UnexpectedKeyKind)?;
                if queued.command.key_index != expected {
                    return Err(TrackerError::KeyIndexMismatch);
                }
                if self.group_queued {
                    return Err(TrackerError::DuplicateQueuedCommand);
                }
                self.group_queued = true;
                self.group_transfer = Some(queued.transfer);
            }
        }
        Ok(())
    }

    /// Called only by the transport owner after it publishes this exact
    /// queued descriptor; recording the software queue alone is insufficient.
    pub fn mark_published(
        &mut self,
        generation: u32,
        kind: KeyKind,
        transfer: QueuedTransfer,
    ) -> Result<(), TrackerError> {
        if generation != self.generation {
            return Err(TrackerError::StaleGeneration);
        }
        let (expected, published) = match kind {
            KeyKind::Pairwise => (&self.pairwise_transfer, &mut self.pairwise_published),
            KeyKind::Group => (&self.group_transfer, &mut self.group_published),
        };
        let Some(expected) = expected else {
            return Err(TrackerError::NotQueuedForPublication);
        };
        if *expected != transfer {
            return Err(TrackerError::PublicationMismatch);
        }
        if *published {
            return Err(TrackerError::DuplicatePublication);
        }
        *published = true;
        Ok(())
    }

    /// Accept a security indication only for a published key in this exact
    /// generation. Stale, unrelated, and duplicate indications do not advance
    /// readiness. The caller must still keep WPA2 data closed until this
    /// tracker reports all required keys installed and M4 has been submitted.
    pub fn observe_security_indication(
        &mut self,
        generation: u32,
        indication: HttSecurityIndication,
    ) -> KeyInstallAction {
        if generation != self.generation {
            return KeyInstallAction::IgnoredStaleGeneration;
        }

        let kind = if indication.is_unicast {
            KeyKind::Pairwise
        } else {
            KeyKind::Group
        };
        if !security_indication_confirms_ccmp_install(indication, self.peer_id, kind) {
            return KeyInstallAction::IgnoredMismatch;
        }

        let (queued, published, installed) = match kind {
            KeyKind::Pairwise => (
                self.pairwise_queued,
                self.pairwise_published,
                &mut self.pairwise_installed,
            ),
            KeyKind::Group => {
                if self.group_key_index.is_none() {
                    return KeyInstallAction::IgnoredUnexpectedKind;
                }
                (
                    self.group_queued,
                    self.group_published,
                    &mut self.group_installed,
                )
            }
        };
        if !queued {
            return KeyInstallAction::IgnoredBeforeQueue;
        }
        if !published {
            return KeyInstallAction::IgnoredBeforePublication;
        }
        if *installed {
            return KeyInstallAction::Duplicate { kind };
        }
        *installed = true;
        KeyInstallAction::Installed {
            kind,
            all_required_installed: self.all_required_installed(),
        }
    }

    pub const fn all_required_installed(&self) -> bool {
        self.pairwise_installed && (self.group_key_index.is_none() || self.group_installed)
    }

    /// Verify that this completed key set belongs to the named association
    /// and carries its planned pairwise/group key indexes.
    pub fn confirms_keys_for_association(
        &self,
        generation: u32,
        peer_address: [u8; 6],
        pairwise_key_index: u8,
        group_key_index: Option<u8>,
    ) -> bool {
        self.generation == generation
            && self.peer_address == peer_address
            && self.pairwise_key_index == pairwise_key_index
            && self.group_key_index == group_key_index
            && self.all_required_installed()
    }
}

impl KeyInstallConfirmation for CcmpKeyInstallTracker {
    fn confirms_keys_for_association(
        &self,
        generation: u32,
        peer_address: [u8; 6],
        pairwise_key_index: u8,
        group_key_index: Option<u8>,
    ) -> bool {
        CcmpKeyInstallTracker::confirms_keys_for_association(
            self,
            generation,
            peer_address,
            pairwise_key_index,
            group_key_index,
        )
    }
}

/// Encode one CCMP-128 WMI 10.4 install-key body.
///
/// The fixed prefix is the pinned 92-byte `wmi_vdev_install_key_cmd`. Linux
/// zeroes the whole skb before filling it, including the RSC/TSC counters and
/// MAC alignment bytes, so this encoder does the same. The WPA supplicant's
/// receive sequence counter remains available to the RX replay filter; the
/// pinned ath10k WMI argument does not carry it into this command.
pub fn encode_ccmp_install_key(
    request: CcmpKeyInstall<'_>,
    output: &mut [u8],
) -> Result<CommandLayout, Error> {
    if request.key_index > WMI_MAX_KEY_INDEX {
        return Err(Error::KeyIndexOutOfRange);
    }
    let body_bytes = VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES;
    if output.len() < body_bytes {
        return Err(Error::OutputTooSmall);
    }

    let body = &mut output[..body_bytes];
    body.fill(0);
    body[0..4].copy_from_slice(&request.vdev_id.to_le_bytes());
    body[4..10].copy_from_slice(&request.peer_address);
    body[12..16].copy_from_slice(&u32::from(request.key_index).to_le_bytes());
    let key_flags = match request.kind {
        KeyKind::Pairwise => KEY_FLAGS_PAIRWISE,
        KeyKind::Group => KEY_FLAGS_GROUP,
    };
    body[16..20].copy_from_slice(&key_flags.to_le_bytes());
    body[20..24].copy_from_slice(&CIPHER_AES_CCM.to_le_bytes());
    body[80..84].copy_from_slice(&(CCMP128_KEY_BYTES as u32).to_le_bytes());
    // TX/RX MIC lengths are zero for CCMP. Bytes 24..80 and 84..92 are the
    // source-defined zeroed sequence counters and MIC lengths.
    body[VDEV_INSTALL_KEY_FIXED_BYTES..body_bytes].copy_from_slice(request.temporal_key);

    Ok(CommandLayout {
        command_id: WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID,
        body_bytes,
        vdev_id: request.vdev_id,
        peer_address: request.peer_address,
        key_index: request.key_index,
        kind: request.kind,
    })
}

/// Encode and place one install-key command in the already-negotiated
/// WMI_CONTROL software queue. The CE descriptor and ring slot are a plan;
/// the function does not publish DMA or indicate that firmware installed the
/// key. Callers must await their platform's key-install completion path before
/// confirming keys to the WPA2 state machine. Both output buffers can contain
/// plaintext key bytes, including when queue submission fails; their owner is
/// responsible for protecting and clearing them.
#[allow(clippy::too_many_arguments)]
pub fn queue_ccmp_install_key<const RING_SIZE: usize>(
    queue: &mut WmiCommandTxQueue<RING_SIZE>,
    request: CcmpKeyInstall<'_>,
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    command_body_output: &mut [u8],
    htc_frame_output: &mut [u8],
) -> Result<QueuedKeyInstall, QueueError> {
    if !queue.is_wmi_control_bound() {
        return Err(QueueError::NotWmiControlEndpoint);
    }

    let command = encode_ccmp_install_key(request, command_body_output).map_err(QueueError::Key)?;
    let transfer = queue
        .queue_transfer(
            command.command_id,
            &command_body_output[..command.body_bytes],
            ce_iova,
            transfer_id,
            context,
            false,
            false,
            htc_frame_output,
        )
        .map_err(QueueError::Transport)?;

    Ok(QueuedKeyInstall { command, transfer })
}
