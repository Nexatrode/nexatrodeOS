//! Bounded QCACLD TLV encoding for WPA2 CCMP key installation.
//!
//! This is the downstream Sunfish `WMI_VDEV_INSTALL_KEY_CMDID` format, not
//! ath10k's WMI 10.4 layout. It builds the fixed 104-byte TLV structure, HTC
//! frame, and CE3 descriptor plan. It has no device authority. The native
//! publisher and DMA-lifetime handler live in `sunfish_qcacld_key_native`.
#![forbid(unsafe_code)]

use crate::ath10k_htt::{HTT_SECURITY_TYPE_AES_CCMP, HttSecurityIndication};
use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_htc::{self, WmiTxPlan};
use crate::qcom_wmi_ce::{self, DmaPool, ServiceCeMapping, TxBufferWindow, WmiCeTxPlan};
use crate::wlan_key_confirmation::KeyInstallConfirmation;

pub const WMI_VDEV_INSTALL_KEY_COMMAND_ID: u32 = 0x5009;
pub const WMI_VDEV_INSTALL_KEY_FIXED_TAG: u16 = 96;
pub const WMI_ARRAY_BYTE_TAG: u16 = 17;
pub const WMI_VDEV_INSTALL_KEY_FIXED_BYTES: usize = 104;
pub const WMI_TLV_HEADER_BYTES: usize = 4;
pub const CCMP128_KEY_BYTES: usize = 16;
pub const WMI_VDEV_INSTALL_KEY_BODY_BYTES: usize =
    WMI_VDEV_INSTALL_KEY_FIXED_BYTES + WMI_TLV_HEADER_BYTES + CCMP128_KEY_BYTES;
pub const WMI_MAX_KEY_INDEX: u8 = 3;

const KEY_FLAGS_GROUP: u32 = 1;
const CIPHER_AES_CCM: u32 = 4;

const OFFSET_VDEV_ID: usize = 4;
const OFFSET_PEER_MAC: usize = 8;
const OFFSET_KEY_INDEX: usize = 16;
const OFFSET_KEY_FLAGS: usize = 20;
const OFFSET_KEY_CIPHER: usize = 24;
const OFFSET_KEY_RSC: usize = 28;
const OFFSET_KEY_LENGTH: usize = 84;
const OFFSET_KEY_ARRAY_TLV: usize = WMI_VDEV_INSTALL_KEY_FIXED_BYTES;
const OFFSET_KEY_DATA: usize = OFFSET_KEY_ARRAY_TLV + WMI_TLV_HEADER_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyKind {
    Pairwise,
    Group,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CcmpInstallKey<'a> {
    /// QCACLD's `set_key_params::vdev_id` is an 8-bit host identifier even
    /// though the WMI fixed-parameter structure serializes it as a u32.
    pub vdev_id: u8,
    pub peer_address: [u8; 6],
    pub key_index: u8,
    pub kind: KeyKind,
    pub temporal_key: &'a [u8; CCMP128_KEY_BYTES],
    /// QCACLD copies the GTK RSC into `key_rsc_counter`; pairwise WPA2 keys
    /// use zero because their sequence state is not the EAPOL GTK RSC.
    pub group_receive_sequence_counter: &'a [u8; 8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPeerAddress,
    KeyIndexOutOfRange { observed: u8 },
    OutputTooSmall { required: usize, available: usize },
    InvalidGeneration,
    Htc(qcom_htc::HtcError),
    Ce(qcom_wmi_ce::Error),
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

/// Encoded command metadata, its exclusive key-bearing HTC staging buffer, and
/// the inputs needed to reconstruct a CE3 plan. The mutable frame reference
/// lets the publisher erase host staging immediately after its DMA copy.
#[derive(Debug)]
pub struct QcacldKeyInstallCePlan<'a> {
    generation: u64,
    command: CommandLayout,
    htc: WmiTxPlan,
    endpoint_id: u8,
    transfer_id: u16,
    mappings: &'a [ServiceCeMapping],
    dma_pool: DmaPool,
    buffer: TxBufferWindow,
    profile: Profile,
    rings: RingPlan,
    cursor: RingCursor,
    frame: &'a mut [u8],
}

impl<'a> QcacldKeyInstallCePlan<'a> {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn command(&self) -> CommandLayout {
        self.command
    }

    pub const fn htc_plan(&self) -> WmiTxPlan {
        self.htc
    }

    pub fn ce_plan(&self) -> Result<WmiCeTxPlan<'_>, Error> {
        qcom_wmi_ce::plan_wmi_tx(
            self.mappings,
            &self.frame[..self.htc.frame_len],
            self.htc,
            self.endpoint_id,
            self.transfer_id,
            self.dma_pool,
            self.buffer,
            self.profile,
            &self.rings,
            self.cursor,
        )
        .map_err(Error::Ce)
    }

    pub const fn frame_bytes(&self) -> usize {
        self.htc.frame_len
    }

    pub const fn endpoint_id(&self) -> u8 {
        self.endpoint_id
    }

    pub fn wipe_frame(&mut self) {
        self.frame[..self.htc.frame_len].fill(0);
    }

    /// Create the pre-publication lifetime token for the key-bearing DMA
    /// buffer. Reserve it in `SensitiveTxBufferLedger` before CE3 publication.
    pub fn sensitive_buffer_lease(&self) -> Result<qcom_wmi_ce::SensitiveTxBufferLease, Error> {
        let ce = self.ce_plan()?;
        self.sensitive_buffer_lease_from_ce(ce)
    }

    pub(crate) fn sensitive_buffer_lease_from_ce(
        &self,
        ce: WmiCeTxPlan<'_>,
    ) -> Result<qcom_wmi_ce::SensitiveTxBufferLease, Error> {
        let bytes = u32::try_from(self.frame_bytes()).map_err(|_| {
            Error::Htc(qcom_htc::HtcError::MessageTooLarge {
                limit: qcom_htc::HTC_MAX_FRAME_PAYLOAD,
                observed: self.frame_bytes(),
            })
        })?;
        qcom_wmi_ce::SensitiveTxBufferLease::new(
            self.generation,
            ce.ticket(),
            TxBufferWindow {
                iova: ce.descriptor().address(),
                capacity: bytes,
            },
        )
        .map_err(Error::Ce)
    }

    #[allow(dead_code)] // Read by the native publisher; absent from the no-unsafe model harness.
    pub(crate) const fn dma_pool(&self) -> DmaPool {
        self.dma_pool
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerError {
    InvalidKeyIndex,
    StaleGeneration,
    PublicationUncertain,
    CommandEncodingMismatch,
    CommandIdentityMismatch,
    KeyIndexMismatch,
    UnexpectedKeyKind,
    DuplicateQueuedCommand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyInstallAction {
    IgnoredStaleGeneration,
    IgnoredUncertainPublication,
    IgnoredMismatch,
    IgnoredUnexpectedKind,
    IgnoredBeforeQueue,
    Duplicate {
        kind: KeyKind,
    },
    Installed {
        kind: KeyKind,
        all_required_installed: bool,
    },
}

/// Tracks the one pairwise and optional group CCMP command for one association.
/// Call `record_queued` only after the QCACLD WMI owner commits the command to
/// its CE3 transmit ring. The native publisher records that commit, and the
/// native WPA2 event dispatcher correlates CE1 `SEC_IND` messages with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QcacldCcmpKeyInstallTracker {
    generation: u32,
    vdev_id: u8,
    peer_id: u16,
    peer_address: [u8; 6],
    pairwise_key_index: u8,
    group_key_index: Option<u8>,
    pairwise_queued: bool,
    group_queued: bool,
    pairwise_installed: bool,
    group_installed: bool,
    publication_uncertain: bool,
}

impl QcacldCcmpKeyInstallTracker {
    pub fn new(
        generation: u32,
        vdev_id: u8,
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
            pairwise_installed: false,
            group_installed: false,
            publication_uncertain: false,
        })
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }

    pub const fn publication_uncertain(&self) -> bool {
        self.publication_uncertain
    }

    pub const fn vdev_id(&self) -> u8 {
        self.vdev_id
    }

    pub const fn peer_id(&self) -> u16 {
        self.peer_id
    }

    pub const fn peer_address(&self) -> [u8; 6] {
        self.peer_address
    }

    pub const fn pairwise_key_index(&self) -> u8 {
        self.pairwise_key_index
    }

    pub const fn group_key_index(&self) -> Option<u8> {
        self.group_key_index
    }

    /// Return the next key command that may be queued for this association.
    /// Pairwise is deliberately ordered before GTK; uncertain publication
    /// remains a terminal error until a new association generation is made.
    pub fn next_unqueued_kind(&self, generation: u32) -> Result<Option<KeyKind>, TrackerError> {
        if generation != self.generation {
            return Err(TrackerError::StaleGeneration);
        }
        if self.publication_uncertain {
            return Err(TrackerError::PublicationUncertain);
        }
        if !self.pairwise_queued {
            return Ok(Some(KeyKind::Pairwise));
        }
        if self.group_key_index.is_some() && !self.group_queued {
            return Ok(Some(KeyKind::Group));
        }
        Ok(None)
    }

    pub fn quarantine_uncertain_publication(
        &mut self,
        generation: u32,
    ) -> Result<(), TrackerError> {
        if generation != self.generation {
            return Err(TrackerError::StaleGeneration);
        }
        self.publication_uncertain = true;
        Ok(())
    }

    pub fn validate_command(
        &self,
        generation: u32,
        command: CommandLayout,
    ) -> Result<(), TrackerError> {
        if generation != self.generation {
            return Err(TrackerError::StaleGeneration);
        }
        if self.publication_uncertain {
            return Err(TrackerError::PublicationUncertain);
        }
        if command.command_id != WMI_VDEV_INSTALL_KEY_COMMAND_ID
            || command.body_bytes != WMI_VDEV_INSTALL_KEY_BODY_BYTES
        {
            return Err(TrackerError::CommandEncodingMismatch);
        }
        if command.vdev_id != u32::from(self.vdev_id) || command.peer_address != self.peer_address {
            return Err(TrackerError::CommandIdentityMismatch);
        }
        match command.kind {
            KeyKind::Pairwise => {
                if command.key_index != self.pairwise_key_index {
                    return Err(TrackerError::KeyIndexMismatch);
                }
                if self.pairwise_queued {
                    return Err(TrackerError::DuplicateQueuedCommand);
                }
            }
            KeyKind::Group => {
                let expected = self
                    .group_key_index
                    .ok_or(TrackerError::UnexpectedKeyKind)?;
                if command.key_index != expected {
                    return Err(TrackerError::KeyIndexMismatch);
                }
                if self.group_queued {
                    return Err(TrackerError::DuplicateQueuedCommand);
                }
            }
        }
        Ok(())
    }

    pub fn record_queued(
        &mut self,
        generation: u32,
        command: CommandLayout,
    ) -> Result<(), TrackerError> {
        self.validate_command(generation, command)?;
        match command.kind {
            KeyKind::Pairwise => {
                self.pairwise_queued = true;
            }
            KeyKind::Group => {
                self.group_queued = true;
            }
        }
        Ok(())
    }

    pub fn observe_security_indication(
        &mut self,
        generation: u32,
        indication: HttSecurityIndication,
    ) -> KeyInstallAction {
        if generation != self.generation {
            return KeyInstallAction::IgnoredStaleGeneration;
        }
        if self.publication_uncertain {
            return KeyInstallAction::IgnoredUncertainPublication;
        }
        let kind = if indication.is_unicast {
            KeyKind::Pairwise
        } else {
            KeyKind::Group
        };
        if indication.peer_id != self.peer_id
            || indication.security_type != HTT_SECURITY_TYPE_AES_CCMP
        {
            return KeyInstallAction::IgnoredMismatch;
        }

        let (queued, installed) = match kind {
            KeyKind::Pairwise => (&mut self.pairwise_queued, &mut self.pairwise_installed),
            KeyKind::Group => {
                if self.group_key_index.is_none() {
                    return KeyInstallAction::IgnoredUnexpectedKind;
                }
                (&mut self.group_queued, &mut self.group_installed)
            }
        };
        if !*queued {
            return KeyInstallAction::IgnoredBeforeQueue;
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
        !self.publication_uncertain
            && self.pairwise_installed
            && (self.group_key_index.is_none() || self.group_installed)
    }
}

impl KeyInstallConfirmation for QcacldCcmpKeyInstallTracker {
    fn confirms_keys_for_association(
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

/// Encode only the QCACLD TLV body. Its WMI command ID is
/// `WMI_VDEV_INSTALL_KEY_COMMAND_ID`; the transport adds that ID to its WMI
/// command envelope. No bytes are written on validation or capacity failure.
pub fn encode_ccmp_install_key(
    request: CcmpInstallKey<'_>,
    output: &mut [u8],
) -> Result<CommandLayout, Error> {
    if request.peer_address == [0; 6] || request.peer_address[0] & 1 != 0 {
        return Err(Error::InvalidPeerAddress);
    }
    if request.key_index > WMI_MAX_KEY_INDEX {
        return Err(Error::KeyIndexOutOfRange {
            observed: request.key_index,
        });
    }
    if output.len() < WMI_VDEV_INSTALL_KEY_BODY_BYTES {
        return Err(Error::OutputTooSmall {
            required: WMI_VDEV_INSTALL_KEY_BODY_BYTES,
            available: output.len(),
        });
    }

    let body = &mut output[..WMI_VDEV_INSTALL_KEY_BODY_BYTES];
    body.fill(0);
    write_tlv_header(
        body,
        0,
        WMI_VDEV_INSTALL_KEY_FIXED_TAG,
        (WMI_VDEV_INSTALL_KEY_FIXED_BYTES - WMI_TLV_HEADER_BYTES) as u16,
    );
    write_u32(body, OFFSET_VDEV_ID, u32::from(request.vdev_id));
    // QCACLD's WMI_CHAR_ARRAY_TO_MAC_ADDR stores MAC bytes 0..3 in the first
    // little-endian word and bytes 4..5 in the low half of the next word.
    // `body.fill(0)` leaves that second word's upper half zero-padded.
    body[OFFSET_PEER_MAC..OFFSET_PEER_MAC + 6].copy_from_slice(&request.peer_address);
    write_u32(body, OFFSET_KEY_INDEX, u32::from(request.key_index));
    write_u32(
        body,
        OFFSET_KEY_FLAGS,
        u32::from(matches!(request.kind, KeyKind::Group)) * KEY_FLAGS_GROUP,
    );
    write_u32(body, OFFSET_KEY_CIPHER, CIPHER_AES_CCM);
    if request.kind == KeyKind::Group {
        body[OFFSET_KEY_RSC..OFFSET_KEY_RSC + 8]
            .copy_from_slice(request.group_receive_sequence_counter);
    }
    write_u32(body, OFFSET_KEY_LENGTH, CCMP128_KEY_BYTES as u32);
    // Both MIC lengths and multi-group-key fields remain zero for CCMP-128.
    write_tlv_header(
        body,
        OFFSET_KEY_ARRAY_TLV,
        WMI_ARRAY_BYTE_TAG,
        CCMP128_KEY_BYTES as u16,
    );
    body[OFFSET_KEY_DATA..OFFSET_KEY_DATA + CCMP128_KEY_BYTES]
        .copy_from_slice(request.temporal_key);

    Ok(CommandLayout {
        command_id: WMI_VDEV_INSTALL_KEY_COMMAND_ID,
        body_bytes: WMI_VDEV_INSTALL_KEY_BODY_BYTES,
        vdev_id: u32::from(request.vdev_id),
        peer_address: request.peer_address,
        key_index: request.key_index,
        kind: request.kind,
    })
}

/// Clear the serialized key command after transport completion. This clears
/// the complete body, including the temporal key, and leaves any trailing
/// caller-owned bytes untouched.
pub fn wipe_ccmp_install_key_body(output: &mut [u8]) -> Result<(), Error> {
    if output.len() < WMI_VDEV_INSTALL_KEY_BODY_BYTES {
        return Err(Error::OutputTooSmall {
            required: WMI_VDEV_INSTALL_KEY_BODY_BYTES,
            available: output.len(),
        });
    }
    output[..WMI_VDEV_INSTALL_KEY_BODY_BYTES].fill(0);
    Ok(())
}

/// Compose one key command with the source-derived HTC envelope and CE3 TX
/// route. This is a planner only; the native publisher separately validates
/// the READY session, reserves credits, and tracks the translated DMA copy
/// until CE3's read index permits scrubbing.
#[allow(clippy::too_many_arguments)]
pub fn plan_qcacld_key_install_ce_tx<'a>(
    generation: u64,
    request: CcmpInstallKey<'_>,
    endpoint_id: u8,
    max_message_size: usize,
    credit_size: u16,
    available_credits: u16,
    mappings: &'a [ServiceCeMapping],
    transfer_id: u16,
    dma_pool: DmaPool,
    buffer: TxBufferWindow,
    profile: Profile,
    rings: &RingPlan,
    cursor: RingCursor,
    wmi_body_output: &mut [u8],
    htc_frame_output: &'a mut [u8],
) -> Result<QcacldKeyInstallCePlan<'a>, Error> {
    if generation == 0 {
        wmi_body_output.fill(0);
        htc_frame_output.fill(0);
        return Err(Error::InvalidGeneration);
    }

    let command = match encode_ccmp_install_key(request, wmi_body_output) {
        Ok(command) => command,
        Err(error) => {
            wmi_body_output.fill(0);
            htc_frame_output.fill(0);
            return Err(error);
        }
    };
    let htc = qcom_htc::encode_wmi_command_frame(
        endpoint_id,
        max_message_size,
        credit_size,
        available_credits,
        command.command_id,
        &wmi_body_output[..command.body_bytes],
        htc_frame_output,
    );
    // The WMI body contains the temporal key. Erase it regardless of whether
    // HTC framing succeeded; the framed copy is now the sole planning input.
    if let Err(error) = wipe_ccmp_install_key_body(wmi_body_output) {
        htc_frame_output.fill(0);
        return Err(error);
    }
    let htc = match htc {
        Ok(plan) => plan,
        Err(error) => {
            htc_frame_output.fill(0);
            return Err(Error::Htc(error));
        }
    };
    let checked = qcom_wmi_ce::plan_wmi_tx(
        mappings,
        &htc_frame_output[..htc.frame_len],
        htc,
        endpoint_id,
        transfer_id,
        dma_pool,
        buffer,
        profile,
        rings,
        cursor,
    )
    .map(|_| ())
    .map_err(Error::Ce);
    if let Err(error) = checked {
        htc_frame_output.fill(0);
        return Err(error);
    }

    Ok(QcacldKeyInstallCePlan {
        generation,
        command,
        htc,
        endpoint_id,
        transfer_id,
        mappings,
        dma_pool,
        buffer,
        profile,
        rings: *rings,
        cursor,
        frame: htc_frame_output,
    })
}

/// Erase the key-bearing HTC staging frame after a caller has finished using
/// its CE plan. This does not scrub the separate DMA-pool copy made by the
/// native publisher; that copy requires a source-read-index completion check.
pub fn wipe_qcacld_key_install_frame(output: &mut [u8], frame_bytes: usize) -> Result<(), Error> {
    if frame_bytes > output.len() {
        return Err(Error::OutputTooSmall {
            required: frame_bytes,
            available: output.len(),
        });
    }
    output[..frame_bytes].fill(0);
    Ok(())
}

fn write_tlv_header(output: &mut [u8], offset: usize, tag: u16, len: u16) {
    let word = (u32::from(tag) << 16) | u32::from(len);
    output[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn write_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 16] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f,
    ];
    const RSC: [u8; 8] = [1, 2, 3, 4, 5, 6, 0, 0];
    const BSSID: [u8; 6] = [0x02, 0x12, 0x34, 0x56, 0x78, 0x9a];

    fn request(kind: KeyKind, key_index: u8) -> CcmpInstallKey<'static> {
        CcmpInstallKey {
            vdev_id: 1,
            peer_address: BSSID,
            key_index,
            kind,
            temporal_key: &KEY,
            group_receive_sequence_counter: &RSC,
        }
    }

    #[test]
    fn pairwise_command_matches_qcacld_fixed_and_key_array_tlvs() {
        let mut output = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
        let layout = encode_ccmp_install_key(request(KeyKind::Pairwise, 0), &mut output).unwrap();

        assert_eq!(layout.command_id, 0x5009);
        assert_eq!(layout.body_bytes, 124);
        assert_eq!(&output[..4], &[0x64, 0x00, 0x60, 0x00]);
        assert_eq!(&output[4..8], &1u32.to_le_bytes());
        assert_eq!(&output[8..14], &BSSID);
        assert_eq!(&output[14..16], &[0, 0]);
        assert_eq!(&output[16..24], &[0; 8]);
        assert_eq!(&output[24..28], &4u32.to_le_bytes());
        assert_eq!(&output[28..36], &[0; 8]);
        assert_eq!(&output[84..88], &16u32.to_le_bytes());
        assert_eq!(&output[88..104], &[0; 16]);
        assert_eq!(&output[104..108], &[0x10, 0x00, 0x11, 0x00]);
        assert_eq!(&output[108..124], &KEY);
    }

    #[test]
    fn group_command_sets_group_usage_and_copies_only_gtk_rsc() {
        let mut output = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
        let layout = encode_ccmp_install_key(request(KeyKind::Group, 2), &mut output).unwrap();

        assert_eq!(layout.key_index, 2);
        assert_eq!(layout.kind, KeyKind::Group);
        assert_eq!(&output[16..20], &2u32.to_le_bytes());
        assert_eq!(&output[20..24], &1u32.to_le_bytes());
        assert_eq!(&output[24..28], &4u32.to_le_bytes());
        assert_eq!(&output[28..36], &RSC);
        assert_eq!(&output[108..124], &KEY);
    }

    #[test]
    fn invalid_requests_and_short_output_leave_buffer_untouched() {
        let cases = [
            (
                CcmpInstallKey {
                    peer_address: [0xff; 6],
                    ..request(KeyKind::Pairwise, 0)
                },
                Error::InvalidPeerAddress,
            ),
            (
                request(KeyKind::Pairwise, 4),
                Error::KeyIndexOutOfRange { observed: 4 },
            ),
        ];
        for (request, expected) in cases {
            let mut output = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
            assert_eq!(encode_ccmp_install_key(request, &mut output), Err(expected));
            assert_eq!(output, [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES]);
        }

        let mut short = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES - 1];
        assert_eq!(
            encode_ccmp_install_key(request(KeyKind::Pairwise, 0), &mut short),
            Err(Error::OutputTooSmall {
                required: WMI_VDEV_INSTALL_KEY_BODY_BYTES,
                available: WMI_VDEV_INSTALL_KEY_BODY_BYTES - 1,
            })
        );
        assert_eq!(short, [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES - 1]);
    }

    #[test]
    fn wipe_clears_key_material_and_preserves_trailing_caller_storage() {
        let mut output = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES + 4];
        encode_ccmp_install_key(request(KeyKind::Pairwise, 0), &mut output).unwrap();
        assert_eq!(&output[108..124], &KEY);

        wipe_ccmp_install_key_body(&mut output).unwrap();

        assert_eq!(
            &output[..WMI_VDEV_INSTALL_KEY_BODY_BYTES],
            &[0; WMI_VDEV_INSTALL_KEY_BODY_BYTES]
        );
        assert_eq!(&output[WMI_VDEV_INSTALL_KEY_BODY_BYTES..], &[0xa5; 4]);
    }

    fn indication(peer_id: u16, security_type: u8, is_unicast: bool) -> HttSecurityIndication {
        HttSecurityIndication {
            peer_id,
            security_type,
            is_unicast,
        }
    }

    fn encoded_layout(kind: KeyKind, key_index: u8) -> CommandLayout {
        let mut output = [0u8; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
        encode_ccmp_install_key(request(kind, key_index), &mut output).unwrap()
    }

    #[test]
    fn tracker_correlates_queued_pairwise_and_group_keys_with_sec_ind() {
        let mut tracker = QcacldCcmpKeyInstallTracker::new(17, 1, 0x42, BSSID, 0, Some(2)).unwrap();
        assert_eq!(tracker.next_unqueued_kind(17), Ok(Some(KeyKind::Pairwise)));
        let pairwise = indication(0x42, HTT_SECURITY_TYPE_AES_CCMP, true);
        assert_eq!(
            tracker.observe_security_indication(17, pairwise),
            KeyInstallAction::IgnoredBeforeQueue
        );
        assert_eq!(
            tracker.observe_security_indication(16, pairwise),
            KeyInstallAction::IgnoredStaleGeneration
        );
        assert_eq!(
            tracker.observe_security_indication(
                17,
                indication(0x43, HTT_SECURITY_TYPE_AES_CCMP, true)
            ),
            KeyInstallAction::IgnoredMismatch
        );

        tracker
            .record_queued(17, encoded_layout(KeyKind::Pairwise, 0))
            .unwrap();
        assert_eq!(tracker.next_unqueued_kind(17), Ok(Some(KeyKind::Group)));
        assert_eq!(
            tracker.observe_security_indication(17, pairwise),
            KeyInstallAction::Installed {
                kind: KeyKind::Pairwise,
                all_required_installed: false,
            }
        );
        assert_eq!(
            tracker.observe_security_indication(17, pairwise),
            KeyInstallAction::Duplicate {
                kind: KeyKind::Pairwise,
            }
        );
        tracker
            .record_queued(17, encoded_layout(KeyKind::Group, 2))
            .unwrap();
        assert_eq!(tracker.next_unqueued_kind(17), Ok(None));
        assert_eq!(
            tracker.observe_security_indication(
                17,
                indication(0x42, HTT_SECURITY_TYPE_AES_CCMP, false),
            ),
            KeyInstallAction::Installed {
                kind: KeyKind::Group,
                all_required_installed: true,
            }
        );
        assert!(tracker.confirms_keys_for_association(17, BSSID, 0, Some(2)));
        assert!(!tracker.confirms_keys_for_association(16, BSSID, 0, Some(2)));
    }

    #[test]
    fn uncertain_key_publication_blocks_selection_until_new_association() {
        let mut tracker = QcacldCcmpKeyInstallTracker::new(17, 1, 0x42, BSSID, 0, Some(2)).unwrap();
        tracker.quarantine_uncertain_publication(17).unwrap();
        assert_eq!(
            tracker.next_unqueued_kind(17),
            Err(TrackerError::PublicationUncertain)
        );
        assert_eq!(
            tracker.next_unqueued_kind(16),
            Err(TrackerError::StaleGeneration)
        );
    }

    #[test]
    fn tracker_rejects_mismatched_and_duplicate_commands() {
        let mut tracker = QcacldCcmpKeyInstallTracker::new(17, 1, 0x42, BSSID, 0, Some(2)).unwrap();
        let mut wrong = encoded_layout(KeyKind::Pairwise, 0);
        wrong.command_id += 1;
        assert_eq!(
            tracker.record_queued(17, wrong),
            Err(TrackerError::CommandEncodingMismatch)
        );
        assert_eq!(
            tracker.record_queued(16, encoded_layout(KeyKind::Pairwise, 0)),
            Err(TrackerError::StaleGeneration)
        );
        let mut wrong_identity = encoded_layout(KeyKind::Pairwise, 0);
        wrong_identity.vdev_id = 2;
        assert_eq!(
            tracker.record_queued(17, wrong_identity),
            Err(TrackerError::CommandIdentityMismatch)
        );
        let pairwise = encoded_layout(KeyKind::Pairwise, 0);
        tracker.record_queued(17, pairwise).unwrap();
        assert_eq!(
            tracker.record_queued(17, pairwise),
            Err(TrackerError::DuplicateQueuedCommand)
        );
    }

    #[test]
    fn pairwise_only_association_does_not_require_a_group_indication() {
        let mut tracker = QcacldCcmpKeyInstallTracker::new(17, 1, 0x42, BSSID, 0, None).unwrap();
        tracker
            .record_queued(17, encoded_layout(KeyKind::Pairwise, 0))
            .unwrap();
        assert_eq!(
            tracker.observe_security_indication(
                17,
                indication(0x42, HTT_SECURITY_TYPE_AES_CCMP, false),
            ),
            KeyInstallAction::IgnoredUnexpectedKind
        );
        assert_eq!(
            tracker.observe_security_indication(
                17,
                indication(0x42, HTT_SECURITY_TYPE_AES_CCMP, true),
            ),
            KeyInstallAction::Installed {
                kind: KeyKind::Pairwise,
                all_required_installed: true,
            }
        );
        assert!(tracker.confirms_keys_for_association(17, BSSID, 0, None));
    }
}
