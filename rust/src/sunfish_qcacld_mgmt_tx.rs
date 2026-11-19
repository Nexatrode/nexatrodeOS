//! Bounded Sunfish QCACLD WMI management-frame TX-by-reference codec.
//!
//! This matches the pinned QCACLD `send_mgmt_cmd_tlv()` path. It emits only
//! open authentication and association-request commands, requires the
//! generation's WMI management-TX service capability, and describes a frame
//! already placed in an authorized DMA pool. It does not map DMA, publish CE,
//! or authorize a live radio operation.
#![forbid(unsafe_code)]

use crate::qcacld_wmi::ServiceReadyView;
use crate::qcom_ce_v3::MAX_CE_IOVA;
use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_htc::{self, WmiTxPlan};
use crate::qcom_wmi_ce::{self, DmaPool, ServiceCeMapping, TxBufferWindow, WmiCeTxPlan};

pub const WMI_MGMT_TX_WMI_SERVICE_INDEX: usize = 76;
pub const WMI_MGMT_TX_SEND_COMMAND_ID: u32 = 0x7008;
pub const WMI_MGMT_TX_COMPLETION_EVENT_ID: u32 = 0x7006;
pub const WMI_MGMT_TX_FIXED_PARAM_TAG: u16 = 422;
/// `wmi_mgmt_tx_compl_event_fixed_param` immediately follows send-command tag
/// 422 in the pinned WMI TLV tag enum.
pub const WMI_MGMT_TX_COMPLETION_TLV_TAG: u16 = 423;
pub const WMI_MGMT_TX_COMPLETION_VALUE_BYTES: usize = 40;
pub const WMI_ARRAY_BYTE_TAG: u16 = 17;
pub const MGMT_TX_INLINE_FRAME_BYTES: usize = 64;
pub const MAX_JOIN_MANAGEMENT_FRAME_BYTES: usize = 512;

const WMI_MGMT_TX_FIXED_PARAM_BYTES: usize = 36;
const WMI_TLV_HEADER_BYTES: usize = 4;
const MAX_WMI_MGMT_TX_BODY_BYTES: usize =
    WMI_MGMT_TX_FIXED_PARAM_BYTES + WMI_TLV_HEADER_BYTES + MGMT_TX_INLINE_FRAME_BYTES;
pub const MAX_HTC_WMI_MGMT_TX_FRAME_BYTES: usize = qcom_htc::HTC_FRAME_HEADER_SIZE
    + qcom_htc::HTC_WMI_COMMAND_HEADER_SIZE
    + MAX_WMI_MGMT_TX_BODY_BYTES;
const MGMT_TYPE: u8 = 0;
const ASSOCIATION_REQUEST_SUBTYPE: u8 = 0;
const AUTHENTICATION_SUBTYPE: u8 = 11;
const SUNFISH_MAX_VDEVS: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    ServiceBitmapMissing,
    ServiceUnavailable,
    WrongGeneration,
    InvalidDmaPool,
    InvalidFrameLength { observed: usize },
    UnsupportedFrameControl,
    InvalidAuthenticationRequest,
    InvalidVdevId { observed: u8 },
    InvalidChannelFrequency,
    WrongDestination,
    WrongSource,
    InvalidFrameAddress,
    FrameOutsideDmaPool,
    FrameOverlapsDmaRegion,
    OutputTooSmall { required: usize, available: usize },
    InvalidCompletionLength { observed: usize },
    InvalidCompletionTlv,
    InvalidCompletionStatus { observed: u32 },
    InvalidOutstandingCapacity,
    InvalidLease,
    DuplicateDescriptorId { descriptor_id: u32 },
    OutstandingFull,
    UnknownDescriptorId { descriptor_id: u32 },
    Htc(qcom_htc::HtcError),
    Ce(qcom_wmi_ce::Error),
}

/// Generation-bound evidence that the firmware advertised WMI management TX.
///
/// The initial implementation intentionally has no HTT fallback. Linux uses
/// HTT when service 76 is absent; Sunfish's separate HTT management path has
/// not been implemented here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiMgmtTxCapability {
    generation: u64,
}

impl WmiMgmtTxCapability {
    pub fn from_service_ready(
        generation: u64,
        service_ready: &ServiceReadyView<'_>,
    ) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        match service_ready.service_enabled(WMI_MGMT_TX_WMI_SERVICE_INDEX) {
            None => Err(Error::ServiceBitmapMissing),
            Some(false) => Err(Error::ServiceUnavailable),
            Some(true) => Ok(Self { generation }),
        }
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// One unprotected association-management request to serialize by reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MgmtTxRequest<'a> {
    pub vdev_id: u8,
    pub descriptor_id: u16,
    pub channel_frequency_mhz: u16,
    pub station: [u8; 6],
    pub bssid: [u8; 6],
    pub frame_iova: u64,
    pub frame: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedMgmtTx {
    generation: u64,
    descriptor_id: u16,
    frame_iova: u64,
    frame_bytes: u16,
    inline_bytes: u16,
    body_bytes: u16,
}

/// Firmware's WMI management-TX completion status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MgmtTxCompletionStatus {
    CompleteOk,
    Discard,
    Inspect,
    CompleteNoAck,
}

/// One fixed-parameter QCACLD management-TX completion event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MgmtTxCompletion {
    pub descriptor_id: u32,
    pub status: MgmtTxCompletionStatus,
    pub pdev_id: u32,
    pub ppdu_id: u32,
    pub ack_rssi: u32,
    pub tx_rate: u32,
    pub peer_phymode: u32,
    pub retries_count: u32,
    pub tx_tsf: u64,
}

/// A management frame's DMA address remains leased until its WMI completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MgmtTxLease {
    pub generation: u64,
    pub descriptor_id: u32,
    pub frame_iova: u64,
    pub frame_bytes: u16,
}

/// Returned only after a matching firmware event retires the DMA lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedMgmtTx {
    pub lease: MgmtTxLease,
    pub status: MgmtTxCompletionStatus,
}

/// Bounded in-flight descriptor table for by-reference management frames.
///
/// A source-ring completion is not sufficient to release these leases: the
/// firmware may read the separately mapped frame after it consumes the WMI
/// command. Only the matching WMI management-TX completion retires a slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MgmtTxOutstanding<const N: usize> {
    generation: u64,
    leases: [Option<MgmtTxLease>; N],
    len: usize,
}

/// Complete HTC/WMI command staged for the dynamically assigned WMI endpoint.
/// The value owns its bytes; it does not copy them into DMA or publish a CE
/// descriptor. `plan_wmi_mgmt_tx_ce` can validate this frame against the
/// selected CE3 route and ring before a separate native owner copies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedWmiMgmtTx {
    command: EncodedMgmtTx,
    wmi: WmiTxPlan,
    bytes: [u8; MAX_HTC_WMI_MGMT_TX_FRAME_BYTES],
}

impl EncodedWmiMgmtTx {
    pub const fn command(self) -> EncodedMgmtTx {
        self.command
    }

    pub const fn wmi_plan(self) -> WmiTxPlan {
        self.wmi
    }

    pub fn frame(&self) -> &[u8] {
        &self.bytes[..self.wmi.frame_len]
    }

    /// Verify the complete by-reference frame matches the request encoded in
    /// this command, including the IOVA, byte length, and inline prefix.
    pub fn references_frame(&self, frame_iova: u64, frame: &[u8]) -> bool {
        let command = self.command;
        let bytes = self.frame();
        if command.frame_iova != frame_iova
            || usize::from(command.frame_bytes) != frame.len()
            || command.inline_bytes as usize != frame.len().min(MGMT_TX_INLINE_FRAME_BYTES)
            || bytes.len() < 52 + usize::from(command.inline_bytes)
            || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) != WMI_MGMT_TX_SEND_COMMAND_ID
            || u32::from_le_bytes(bytes[12..16].try_into().unwrap())
                != tlv_header(WMI_MGMT_TX_FIXED_PARAM_TAG, 32)
            || u32::from_le_bytes(bytes[28..32].try_into().unwrap()) != frame_iova as u32
            || u32::from_le_bytes(bytes[32..36].try_into().unwrap())
                != ((frame_iova >> 32) as u32 & 0x1f)
            || u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize != frame.len()
            || u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize
                != usize::from(command.inline_bytes)
        {
            return false;
        }
        let inline_bytes = usize::from(command.inline_bytes);
        let padded_inline_bytes = (inline_bytes + 3) & !3;
        if u32::from_le_bytes(bytes[48..52].try_into().unwrap())
            != tlv_header(WMI_ARRAY_BYTE_TAG, padded_inline_bytes)
            || bytes[52..52 + inline_bytes] != frame[..inline_bytes]
            || bytes[52 + inline_bytes..52 + padded_inline_bytes]
                .iter()
                .any(|byte| *byte != 0)
        {
            return false;
        }
        true
    }
}

impl EncodedMgmtTx {
    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn descriptor_id(self) -> u16 {
        self.descriptor_id
    }

    pub const fn frame_iova(self) -> u64 {
        self.frame_iova
    }

    pub const fn frame_bytes(self) -> u16 {
        self.frame_bytes
    }

    pub const fn inline_bytes(self) -> u16 {
        self.inline_bytes
    }

    pub const fn body_bytes(self) -> u16 {
        self.body_bytes
    }
}

impl<const N: usize> MgmtTxOutstanding<N> {
    pub fn new(generation: u64) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if N == 0 {
            return Err(Error::InvalidOutstandingCapacity);
        }
        Ok(Self {
            generation,
            leases: [None; N],
            len: 0,
        })
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reserve a descriptor ID and DMA frame address until firmware completes
    /// the corresponding management-TX request.
    pub fn reserve(&mut self, lease: MgmtTxLease) -> Result<(), Error> {
        if lease.generation != self.generation {
            return Err(Error::WrongGeneration);
        }
        let Some(frame_end) = lease.frame_iova.checked_add(u64::from(lease.frame_bytes)) else {
            return Err(Error::InvalidLease);
        };
        if lease.frame_iova == 0
            || !lease.frame_iova.is_multiple_of(8)
            || lease.frame_bytes == 0
            || usize::from(lease.frame_bytes) > MAX_JOIN_MANAGEMENT_FRAME_BYTES
            || frame_end > MAX_CE_IOVA
        {
            return Err(Error::InvalidLease);
        }
        if self
            .leases
            .iter()
            .flatten()
            .any(|active| active.descriptor_id == lease.descriptor_id)
        {
            return Err(Error::DuplicateDescriptorId {
                descriptor_id: lease.descriptor_id,
            });
        }
        let slot = self
            .leases
            .iter_mut()
            .find(|active| active.is_none())
            .ok_or(Error::OutstandingFull)?;
        *slot = Some(lease);
        self.len += 1;
        Ok(())
    }

    /// Undo a reservation only when the native publisher proves that it did
    /// not publish the CE producer index. A possibly visible descriptor must
    /// remain leased until a matching firmware completion or queue recovery.
    pub fn cancel_unpublished(
        &mut self,
        generation: u64,
        descriptor_id: u32,
    ) -> Result<MgmtTxLease, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if generation != self.generation {
            return Err(Error::WrongGeneration);
        }
        let index = self
            .leases
            .iter()
            .position(|active| active.is_some_and(|lease| lease.descriptor_id == descriptor_id))
            .ok_or(Error::UnknownDescriptorId { descriptor_id })?;
        let lease = self.leases[index]
            .take()
            .ok_or(Error::UnknownDescriptorId { descriptor_id })?;
        self.len -= 1;
        Ok(lease)
    }

    /// Retire exactly one matching DMA lease after a validated WMI event.
    /// Malformed, stale-generation, or unknown completions leave the table
    /// unchanged so the caller can quarantine rather than reuse uncertain
    /// memory.
    pub fn complete(
        &mut self,
        generation: u64,
        completion: MgmtTxCompletion,
    ) -> Result<CompletedMgmtTx, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if generation != self.generation {
            return Err(Error::WrongGeneration);
        }
        let index = self
            .leases
            .iter()
            .position(|active| {
                active.is_some_and(|lease| lease.descriptor_id == completion.descriptor_id)
            })
            .ok_or(Error::UnknownDescriptorId {
                descriptor_id: completion.descriptor_id,
            })?;
        let lease = self.leases[index]
            .take()
            .ok_or(Error::UnknownDescriptorId {
                descriptor_id: completion.descriptor_id,
            })?;
        self.len -= 1;
        Ok(CompletedMgmtTx {
            lease,
            status: completion.status,
        })
    }
}

fn tlv_header(tag: u16, value_bytes: usize) -> u32 {
    (u32::from(tag) << 16) | value_bytes as u32
}

fn ranges_overlap(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    let Some(a_end) = a.checked_add(a_bytes) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_bytes) else {
        return true;
    };
    a < b_end && b < a_end
}

/// Decode the fixed parameter for one already-demultiplexed
/// `WMI_MGMT_TX_COMPLETION_EVENTID` event. The caller must first validate the
/// enclosing HTC/WMI frame and event command ID.
pub fn parse_wmi_mgmt_tx_completion(body: &[u8]) -> Result<MgmtTxCompletion, Error> {
    const TLV_BYTES: usize = 4;
    const EVENT_BYTES: usize = TLV_BYTES + WMI_MGMT_TX_COMPLETION_VALUE_BYTES;
    if body.len() != EVENT_BYTES {
        return Err(Error::InvalidCompletionLength {
            observed: body.len(),
        });
    }
    let parsed_header = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    if parsed_header
        != tlv_header(
            WMI_MGMT_TX_COMPLETION_TLV_TAG,
            WMI_MGMT_TX_COMPLETION_VALUE_BYTES,
        )
    {
        return Err(Error::InvalidCompletionTlv);
    }
    let read = |offset: usize| {
        u32::from_le_bytes([
            body[offset],
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
        ])
    };
    let status = match read(8) {
        0 => MgmtTxCompletionStatus::CompleteOk,
        1 => MgmtTxCompletionStatus::Discard,
        2 => MgmtTxCompletionStatus::Inspect,
        3 => MgmtTxCompletionStatus::CompleteNoAck,
        observed => return Err(Error::InvalidCompletionStatus { observed }),
    };
    Ok(MgmtTxCompletion {
        descriptor_id: read(4),
        status,
        pdev_id: read(12),
        ppdu_id: read(16),
        ack_rssi: read(20),
        tx_rate: read(24),
        peer_phymode: read(28),
        retries_count: read(32),
        tx_tsf: u64::from(read(36)) | (u64::from(read(40)) << 32),
    })
}

fn write_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn checked_request(
    capability: WmiMgmtTxCapability,
    generation: u64,
    pool: DmaPool,
    request: MgmtTxRequest<'_>,
) -> Result<(usize, usize), Error> {
    if generation == 0 {
        return Err(Error::InvalidGeneration);
    }
    if capability.generation != generation {
        return Err(Error::WrongGeneration);
    }
    if request.vdev_id >= SUNFISH_MAX_VDEVS {
        return Err(Error::InvalidVdevId {
            observed: request.vdev_id,
        });
    }
    if !(24..=MAX_JOIN_MANAGEMENT_FRAME_BYTES).contains(&request.frame.len()) {
        return Err(Error::InvalidFrameLength {
            observed: request.frame.len(),
        });
    }
    let frame_control = request.frame[0];
    let subtype = frame_control >> 4;
    if frame_control & 0x0f != MGMT_TYPE
        || !matches!(
            subtype,
            ASSOCIATION_REQUEST_SUBTYPE | AUTHENTICATION_SUBTYPE
        )
        || request.frame[1] & 0x43 != 0
    {
        return Err(Error::UnsupportedFrameControl);
    }
    match subtype {
        ASSOCIATION_REQUEST_SUBTYPE => {
            if request.frame.len() < 28 {
                return Err(Error::InvalidFrameLength {
                    observed: request.frame.len(),
                });
            }
        }
        AUTHENTICATION_SUBTYPE => {
            if request.frame.len() < 30 {
                return Err(Error::InvalidFrameLength {
                    observed: request.frame.len(),
                });
            }
            // The WPA2 join path uses only open-system authentication request
            // (algorithm 0, transaction 1, status 0). Do not reinterpret an
            // auth response or a different authentication method as a request.
            if request.frame[24..26] != [0, 0]
                || request.frame[26..28] != [1, 0]
                || request.frame[28..30] != [0, 0]
            {
                return Err(Error::InvalidAuthenticationRequest);
            }
        }
        _ => return Err(Error::UnsupportedFrameControl),
    }
    if request.station == request.bssid || request.station[0] & 1 != 0 || request.bssid[0] & 1 != 0
    {
        return Err(Error::WrongSource);
    }
    if request.frame[4..10] != request.bssid {
        return Err(Error::WrongDestination);
    }
    if request.frame[10..16] != request.station {
        return Err(Error::WrongSource);
    }
    if request.frame[16..22] != request.bssid {
        return Err(Error::WrongDestination);
    }
    if request.channel_frequency_mhz == 0 {
        return Err(Error::InvalidChannelFrequency);
    }
    if request.frame_iova == 0 || !request.frame_iova.is_multiple_of(8) {
        return Err(Error::InvalidFrameAddress);
    }
    let frame_len = request.frame.len() as u64;
    let end = request
        .frame_iova
        .checked_add(frame_len)
        .ok_or(Error::FrameOutsideDmaPool)?;
    if end > MAX_CE_IOVA {
        return Err(Error::FrameOutsideDmaPool);
    }
    let pool_end = pool
        .base()
        .checked_add(pool.bytes())
        .ok_or(Error::InvalidDmaPool)?;
    if request.frame_iova < pool.base() || end > pool_end {
        return Err(Error::FrameOutsideDmaPool);
    }

    let inline_bytes = request.frame.len().min(MGMT_TX_INLINE_FRAME_BYTES);
    let padded_inline_bytes = inline_bytes
        .checked_add(3)
        .ok_or(Error::InvalidFrameLength {
            observed: request.frame.len(),
        })?
        & !3;
    Ok((inline_bytes, padded_inline_bytes))
}

/// Encode one by-reference WMI management TX command body.
///
/// The command carries the first 64 frame bytes inline, while `frame_iova`
/// points to the complete frame. The caller must keep that DMA allocation
/// owned and immutable until the matching WMI management-TX completion arrives.
/// No output bytes are changed on validation or capacity failure.
pub fn encode_wmi_mgmt_tx(
    capability: WmiMgmtTxCapability,
    generation: u64,
    pool: DmaPool,
    request: MgmtTxRequest<'_>,
    output: &mut [u8],
) -> Result<EncodedMgmtTx, Error> {
    let (inline_bytes, padded_inline_bytes) =
        checked_request(capability, generation, pool, request)?;
    let body_bytes = WMI_MGMT_TX_FIXED_PARAM_BYTES
        .checked_add(WMI_TLV_HEADER_BYTES)
        .and_then(|fixed| fixed.checked_add(padded_inline_bytes))
        .ok_or(Error::InvalidFrameLength {
            observed: request.frame.len(),
        })?;
    if body_bytes > MAX_WMI_MGMT_TX_BODY_BYTES || output.len() < body_bytes {
        return Err(Error::OutputTooSmall {
            required: body_bytes,
            available: output.len(),
        });
    }

    // Stage locally so the caller's output stays unchanged unless the entire
    // fixed structure and bounded inline frame are ready.
    let mut encoded = [0u8; MAX_WMI_MGMT_TX_BODY_BYTES];
    write_u32(&mut encoded, 0, tlv_header(WMI_MGMT_TX_FIXED_PARAM_TAG, 32));
    write_u32(&mut encoded, 4, u32::from(request.vdev_id));
    write_u32(&mut encoded, 8, u32::from(request.descriptor_id));
    write_u32(&mut encoded, 12, u32::from(request.channel_frequency_mhz));
    write_u32(&mut encoded, 16, request.frame_iova as u32);
    write_u32(&mut encoded, 20, ((request.frame_iova >> 32) as u32) & 0x1f);
    write_u32(&mut encoded, 24, request.frame.len() as u32);
    write_u32(&mut encoded, 28, inline_bytes as u32);
    write_u32(&mut encoded, 32, 0); // no TX parameter extension for join frames
    write_u32(
        &mut encoded,
        WMI_MGMT_TX_FIXED_PARAM_BYTES,
        tlv_header(WMI_ARRAY_BYTE_TAG, padded_inline_bytes),
    );
    let inline_start = WMI_MGMT_TX_FIXED_PARAM_BYTES + WMI_TLV_HEADER_BYTES;
    encoded[inline_start..inline_start + inline_bytes]
        .copy_from_slice(&request.frame[..inline_bytes]);
    output[..body_bytes].copy_from_slice(&encoded[..body_bytes]);

    Ok(EncodedMgmtTx {
        generation,
        descriptor_id: request.descriptor_id,
        frame_iova: request.frame_iova,
        frame_bytes: request.frame.len() as u16,
        inline_bytes: inline_bytes as u16,
        body_bytes: body_bytes as u16,
    })
}

/// Build a complete by-reference WMI management-TX command in an HTC frame.
///
/// This is still a byte-planning operation: it reserves no credit, copies no
/// bytes to DMA, and publishes no descriptor. The dynamic endpoint and
/// available HTC credits must come from the live, generation-matched WMI/HTC
/// owner. The returned frame can then be checked by the existing CE3 planner.
pub fn encode_wmi_mgmt_tx_frame(
    capability: WmiMgmtTxCapability,
    generation: u64,
    pool: DmaPool,
    request: MgmtTxRequest<'_>,
    endpoint_id: u8,
    max_message_size: usize,
    credit_size: u16,
    available_credits: u16,
) -> Result<EncodedWmiMgmtTx, Error> {
    let mut body = [0u8; MAX_WMI_MGMT_TX_BODY_BYTES];
    let command = encode_wmi_mgmt_tx(capability, generation, pool, request, &mut body)?;
    let mut bytes = [0u8; MAX_HTC_WMI_MGMT_TX_FRAME_BYTES];
    let wmi = qcom_htc::encode_wmi_command_frame(
        endpoint_id,
        max_message_size,
        credit_size,
        available_credits,
        WMI_MGMT_TX_SEND_COMMAND_ID,
        &body[..usize::from(command.body_bytes)],
        &mut bytes,
    )
    .map_err(Error::Htc)?;
    Ok(EncodedWmiMgmtTx {
        command,
        wmi,
        bytes,
    })
}

/// Validate a staged management command against the selected WMI CE route,
/// profile, DMA pool, source ring, and cursor. This does not mutate the cursor
/// or perform cache, DMA, MMIO, or lifecycle operations.
pub fn plan_wmi_mgmt_tx_ce<'a>(
    encoded: &'a EncodedWmiMgmtTx,
    mappings: &[ServiceCeMapping],
    endpoint_id: u8,
    transfer_id: u16,
    dma_pool: DmaPool,
    buffer: TxBufferWindow,
    profile: Profile,
    rings: &RingPlan,
    cursor: RingCursor,
) -> Result<WmiCeTxPlan<'a>, Error> {
    let command = encoded.command();
    let frame_bytes = u64::from(command.frame_bytes());
    let frame_end = command
        .frame_iova()
        .checked_add(frame_bytes)
        .ok_or(Error::FrameOutsideDmaPool)?;
    let pool_end = dma_pool
        .base()
        .checked_add(dma_pool.bytes())
        .ok_or(Error::InvalidDmaPool)?;
    if command.frame_iova() < dma_pool.base() || frame_end > pool_end {
        return Err(Error::FrameOutsideDmaPool);
    }
    if ranges_overlap(
        command.frame_iova(),
        frame_bytes,
        buffer.iova,
        u64::from(buffer.capacity),
    ) || rings.allocations().iter().any(|allocation| {
        ranges_overlap(
            command.frame_iova(),
            frame_bytes,
            allocation.descriptor_iova,
            u64::from(allocation.allocation_bytes),
        )
    }) {
        return Err(Error::FrameOverlapsDmaRegion);
    }
    qcom_wmi_ce::plan_wmi_tx(
        mappings,
        encoded.frame(),
        encoded.wmi,
        endpoint_id,
        transfer_id,
        dma_pool,
        buffer,
        profile,
        rings,
        cursor,
    )
    .map_err(Error::Ce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const STATION: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    const BSSID: [u8; 6] = [0x10, 0x21, 0x32, 0x43, 0x54, 0x65];
    fn pool() -> DmaPool {
        DmaPool::new(0x8000_0000, 0x1_0000).expect("valid fixture DMA pool")
    }

    fn append_tlv(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
        bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(value);
    }

    fn service_ready(enabled: bool) -> Vec<u8> {
        let fixed = [0; 128];
        let mut bitmap = [0u8; 128];
        if enabled {
            // QCACLD's pinned service bitmap uses a four-service-byte stride.
            bitmap[19 * 4..20 * 4].copy_from_slice(&1u32.to_le_bytes());
        }
        let mut event = Vec::new();
        append_tlv(&mut event, 32, &fixed);
        append_tlv(&mut event, 33, &[0; 36]);
        append_tlv(&mut event, 16, &bitmap);
        // A zero memory-request count requires no ARRAY_STRUCT record.
        event
    }

    fn capability(enabled: bool) -> WmiMgmtTxCapability {
        let event = service_ready(enabled);
        let parsed = ServiceReadyView::parse(&event).unwrap();
        WmiMgmtTxCapability::from_service_ready(7, &parsed).unwrap_or_else(|error| {
            if enabled {
                panic!("enabled capability rejected: {error:?}")
            }
            panic!("disabled service must be asserted by the caller")
        })
    }

    fn auth_frame() -> Vec<u8> {
        let mut frame = vec![0; 30];
        frame[0] = 0xb0; // open-system authentication request
        frame[4..10].copy_from_slice(&BSSID);
        frame[10..16].copy_from_slice(&STATION);
        frame[16..22].copy_from_slice(&BSSID);
        frame[24..26].copy_from_slice(&0u16.to_le_bytes());
        frame[26..28].copy_from_slice(&1u16.to_le_bytes());
        frame
    }

    fn request(frame: &[u8], frame_iova: u64) -> MgmtTxRequest<'_> {
        MgmtTxRequest {
            vdev_id: 1,
            descriptor_id: 0x1234,
            channel_frequency_mhz: 2412,
            station: STATION,
            bssid: BSSID,
            frame_iova,
            frame,
        }
    }

    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn completion_event(descriptor_id: u32, status: u32) -> [u8; 44] {
        let mut event = [0u8; 44];
        event[..4].copy_from_slice(
            &tlv_header(
                WMI_MGMT_TX_COMPLETION_TLV_TAG,
                WMI_MGMT_TX_COMPLETION_VALUE_BYTES,
            )
            .to_le_bytes(),
        );
        for (index, value) in [
            descriptor_id,
            status,
            2,           // pdev_id
            0x1234_5678, // ppdu_id
            0xffff_ff9c, // ack RSSI, retained as source-defined u32
            600_000,     // tx rate in kbps
            7,           // peer phy mode
            3,           // retry count
            0x89ab_cdef, // TSF low
            0x0123_4567, // TSF high
        ]
        .into_iter()
        .enumerate()
        {
            let offset = 4 + index * 4;
            event[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        event
    }

    #[test]
    fn service_ready_bit_76_is_required_and_generation_bound() {
        let event = service_ready(false);
        let parsed = ServiceReadyView::parse(&event).unwrap();
        assert_eq!(
            WmiMgmtTxCapability::from_service_ready(7, &parsed),
            Err(Error::ServiceUnavailable)
        );
        assert_eq!(
            WmiMgmtTxCapability::from_service_ready(0, &parsed),
            Err(Error::InvalidGeneration)
        );

        let event = service_ready(true);
        let parsed = ServiceReadyView::parse(&event).unwrap();
        let permit = WmiMgmtTxCapability::from_service_ready(7, &parsed).unwrap();
        assert_eq!(permit.generation(), 7);
        assert_eq!(
            encode_wmi_mgmt_tx(
                permit,
                8,
                pool(),
                request(&auth_frame(), 0x8000_1000),
                &mut [0; 104]
            ),
            Err(Error::WrongGeneration)
        );
    }

    #[test]
    fn authentication_request_matches_qcacld_wmi_reference_layout() {
        let frame = auth_frame();
        let permit = capability(true);
        let mut output = [0xa5; 112];
        let plan = encode_wmi_mgmt_tx(permit, 7, pool(), request(&frame, 0x8000_1000), &mut output)
            .unwrap();

        assert_eq!(plan.descriptor_id(), 0x1234);
        assert_eq!(plan.frame_iova(), 0x8000_1000);
        assert_eq!(plan.frame_bytes(), frame.len() as u16);
        assert_eq!(plan.inline_bytes(), frame.len() as u16);
        assert_eq!(plan.body_bytes(), 72);
        assert_eq!(word(&output, 0), 0x01a6_0020);
        assert_eq!(word(&output, 4), 1);
        assert_eq!(word(&output, 8), 0x1234);
        assert_eq!(word(&output, 12), 2412);
        assert_eq!(word(&output, 16), 0x8000_1000);
        assert_eq!(word(&output, 20), 0);
        assert_eq!(word(&output, 24), 30);
        assert_eq!(word(&output, 28), 30);
        assert_eq!(word(&output, 32), 0);
        assert_eq!(word(&output, 36), 0x0011_0020);
        assert_eq!(&output[40..70], frame.as_slice());
        assert_eq!(&output[70..72], &[0, 0]);
        assert_eq!(&output[72..], &[0xa5; 40]);
    }

    #[test]
    fn management_tx_is_composed_as_wmi_0x7008_inside_dynamic_htc_endpoint() {
        let frame = auth_frame();
        let encoded = encode_wmi_mgmt_tx_frame(
            capability(true),
            7,
            pool(),
            request(&frame, 0x8000_1000),
            5,
            256,
            64,
            8,
        )
        .unwrap();
        let bytes = encoded.frame();
        assert_eq!(bytes.len(), 84);
        assert_eq!(&bytes[..8], &[5, 0, 76, 0, 0, 0, 0, 0]);
        assert_eq!(&bytes[8..12], &WMI_MGMT_TX_SEND_COMMAND_ID.to_le_bytes());
        assert_eq!(word(bytes, 12), 0x01a6_0020);
        assert_eq!(word(bytes, 48), 0x0011_0020);
        assert_eq!(&bytes[52..82], frame.as_slice());
        assert_eq!(&bytes[82..84], &[0, 0]);
        assert_eq!(encoded.command().frame_iova(), 0x8000_1000);
        assert_eq!(encoded.wmi_plan().credits_required, 2);
    }

    #[test]
    fn encoded_management_tx_preflights_on_the_selected_wmi_ce3_route() {
        let frame = auth_frame();
        let dma_pool = DmaPool::new(0x8000_0000, 0x4_0000).unwrap();
        let encoded = encode_wmi_mgmt_tx_frame(
            capability(true),
            7,
            dma_pool,
            request(&frame, 0x8003_0000),
            5,
            256,
            64,
            8,
        )
        .unwrap();
        let rings = crate::qcom_ce_v3_profile::plan_rings(
            Profile::Mission,
            dma_pool.base(),
            dma_pool.bytes(),
        )
        .unwrap();
        let buffer_iova = (dma_pool.base() + rings.total_bytes() + 4095) & !4095;
        let buffer = TxBufferWindow {
            iova: buffer_iova,
            capacity: 512,
        };
        let mappings = [
            ServiceCeMapping {
                service_id: qcom_wmi_ce::WMI_CONTROL_SERVICE_ID,
                direction: qcom_wmi_ce::Direction::HostToTarget,
                engine: qcom_wmi_ce::WMI_CONTROL_SOURCE_CE,
            },
            ServiceCeMapping {
                service_id: qcom_wmi_ce::WMI_CONTROL_SERVICE_ID,
                direction: qcom_wmi_ce::Direction::TargetToHost,
                engine: qcom_wmi_ce::WMI_CONTROL_DESTINATION_CE,
            },
        ];
        let cursor = RingCursor::new(32).unwrap();
        let plan = plan_wmi_mgmt_tx_ce(
            &encoded,
            &mappings,
            5,
            9,
            dma_pool,
            buffer,
            Profile::Mission,
            &rings,
            cursor,
        )
        .unwrap();

        assert_eq!(plan.route().host_to_target_engine, 3);
        assert_eq!(plan.route().target_to_host_engine, 2);
        assert_eq!(plan.descriptor().address(), buffer.iova);
        assert_eq!(
            usize::from(plan.descriptor().nbytes()),
            encoded.frame().len()
        );
        assert_eq!(plan.descriptor().transfer_id(), 9);
        assert_eq!(plan.ticket().index(), 0);
        assert_eq!(cursor.producer_index(), 0);
        assert_eq!(cursor.pending(), 0);

        let ring_iova = rings.allocations()[0].descriptor_iova;
        let aliased = encode_wmi_mgmt_tx_frame(
            capability(true),
            7,
            dma_pool,
            request(&frame, ring_iova),
            5,
            256,
            64,
            8,
        )
        .unwrap();
        assert!(matches!(
            plan_wmi_mgmt_tx_ce(
                &aliased,
                &mappings,
                5,
                10,
                dma_pool,
                buffer,
                Profile::Mission,
                &rings,
                cursor,
            ),
            Err(Error::FrameOverlapsDmaRegion)
        ));
    }

    #[test]
    fn completion_event_parser_checks_fixed_tlv_and_preserves_source_fields() {
        let event = completion_event(0x1234, 3);
        let parsed = parse_wmi_mgmt_tx_completion(&event).unwrap();
        assert_eq!(parsed.descriptor_id, 0x1234);
        assert_eq!(parsed.status, MgmtTxCompletionStatus::CompleteNoAck);
        assert_eq!(parsed.pdev_id, 2);
        assert_eq!(parsed.ppdu_id, 0x1234_5678);
        assert_eq!(parsed.ack_rssi, 0xffff_ff9c);
        assert_eq!(parsed.tx_rate, 600_000);
        assert_eq!(parsed.peer_phymode, 7);
        assert_eq!(parsed.retries_count, 3);
        assert_eq!(parsed.tx_tsf, 0x0123_4567_89ab_cdef);

        assert_eq!(
            parse_wmi_mgmt_tx_completion(&event[..43]),
            Err(Error::InvalidCompletionLength { observed: 43 })
        );
        let mut wrong_tag = event;
        wrong_tag[2] ^= 1;
        assert_eq!(
            parse_wmi_mgmt_tx_completion(&wrong_tag),
            Err(Error::InvalidCompletionTlv)
        );
        let mut wrong_value_length = event;
        wrong_value_length[0] -= 4;
        assert_eq!(
            parse_wmi_mgmt_tx_completion(&wrong_value_length),
            Err(Error::InvalidCompletionTlv)
        );
        let invalid_status = completion_event(0x1234, 4);
        assert_eq!(
            parse_wmi_mgmt_tx_completion(&invalid_status),
            Err(Error::InvalidCompletionStatus { observed: 4 })
        );
    }

    #[test]
    fn completion_retires_only_the_matching_generation_bound_frame_lease() {
        let mut outstanding = MgmtTxOutstanding::<2>::new(7).unwrap();
        let first = MgmtTxLease {
            generation: 7,
            descriptor_id: 0x1234,
            frame_iova: 0x8000_3000,
            frame_bytes: 30,
        };
        let second = MgmtTxLease {
            descriptor_id: 0x1235,
            ..first
        };
        outstanding.reserve(first).unwrap();
        assert_eq!(outstanding.len(), 1);
        assert_eq!(
            outstanding.reserve(first),
            Err(Error::DuplicateDescriptorId {
                descriptor_id: first.descriptor_id
            })
        );
        outstanding.reserve(second).unwrap();
        assert_eq!(
            outstanding.reserve(MgmtTxLease {
                descriptor_id: 0x1236,
                ..first
            }),
            Err(Error::OutstandingFull)
        );

        let completion = parse_wmi_mgmt_tx_completion(&completion_event(0x1234, 3)).unwrap();
        assert_eq!(
            outstanding.complete(8, completion),
            Err(Error::WrongGeneration)
        );
        assert_eq!(
            outstanding.complete(
                7,
                MgmtTxCompletion {
                    descriptor_id: 0xffff,
                    ..completion
                }
            ),
            Err(Error::UnknownDescriptorId {
                descriptor_id: 0xffff
            })
        );
        assert_eq!(outstanding.len(), 2);

        let retired = outstanding.complete(7, completion).unwrap();
        assert_eq!(retired.lease, first);
        assert_eq!(retired.status, MgmtTxCompletionStatus::CompleteNoAck);
        assert_eq!(outstanding.len(), 1);
        assert_eq!(
            outstanding.reserve(MgmtTxLease {
                generation: 8,
                descriptor_id: 0x1236,
                ..first
            }),
            Err(Error::WrongGeneration)
        );
    }

    #[test]
    fn outstanding_ledger_rejects_invalid_frame_ranges_and_zero_capacity() {
        assert_eq!(
            MgmtTxOutstanding::<0>::new(7),
            Err(Error::InvalidOutstandingCapacity)
        );
        let mut outstanding = MgmtTxOutstanding::<1>::new(7).unwrap();
        for invalid in [
            MgmtTxLease {
                frame_iova: 0,
                ..lease_fixture()
            },
            MgmtTxLease {
                frame_iova: 0x8000_3001,
                ..lease_fixture()
            },
            MgmtTxLease {
                frame_bytes: 0,
                ..lease_fixture()
            },
            MgmtTxLease {
                frame_bytes: (MAX_JOIN_MANAGEMENT_FRAME_BYTES + 1) as u16,
                ..lease_fixture()
            },
            MgmtTxLease {
                frame_iova: MAX_CE_IOVA - 8,
                frame_bytes: 30,
                ..lease_fixture()
            },
        ] {
            assert_eq!(outstanding.reserve(invalid), Err(Error::InvalidLease));
            assert!(outstanding.is_empty());
        }
    }

    fn lease_fixture() -> MgmtTxLease {
        MgmtTxLease {
            generation: 7,
            descriptor_id: 0x1234,
            frame_iova: 0x8000_3000,
            frame_bytes: 30,
        }
    }

    #[test]
    fn rejects_non_request_authentication_and_invalid_vdev() {
        let permit = capability(true);
        let mut response = auth_frame();
        response[26..28].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            encode_wmi_mgmt_tx(
                permit,
                7,
                pool(),
                request(&response, 0x8000_1000),
                &mut [0; 104]
            ),
            Err(Error::InvalidAuthenticationRequest)
        );
        let valid = auth_frame();
        let mut invalid_vdev = request(&valid, 0x8000_1000);
        invalid_vdev.vdev_id = SUNFISH_MAX_VDEVS;
        assert_eq!(
            encode_wmi_mgmt_tx(permit, 7, pool(), invalid_vdev, &mut [0; 104]),
            Err(Error::InvalidVdevId {
                observed: SUNFISH_MAX_VDEVS
            })
        );
    }

    #[test]
    fn association_request_uses_full_dma_length_and_only_first_64_inline_bytes() {
        let mut frame = vec![0; 80];
        frame[0] = 0x00; // association request
        frame[4..10].copy_from_slice(&BSSID);
        frame[10..16].copy_from_slice(&STATION);
        frame[16..22].copy_from_slice(&BSSID);
        for (index, byte) in frame[24..].iter_mut().enumerate() {
            *byte = (index as u8).wrapping_add(1);
        }
        let mut output = [0; 104];
        let plan = encode_wmi_mgmt_tx(
            capability(true),
            7,
            pool(),
            request(&frame, 0x8000_2000),
            &mut output,
        )
        .unwrap();
        assert_eq!(plan.frame_bytes(), 80);
        assert_eq!(plan.inline_bytes(), 64);
        assert_eq!(plan.body_bytes(), 104);
        assert_eq!(word(&output, 24), 80);
        assert_eq!(word(&output, 28), 64);
        assert_eq!(word(&output, 36), 0x0011_0040);
        assert_eq!(&output[40..104], &frame[..64]);
    }

    #[test]
    fn rejects_wrong_frame_kind_protection_addresses_and_dma_ranges_atomically() {
        let permit = capability(true);
        let valid = auth_frame();
        let mut output = [0xa5; 104];

        let mut wrong_kind = valid.clone();
        wrong_kind[0] = 0x08;
        assert_eq!(
            encode_wmi_mgmt_tx(
                permit,
                7,
                pool(),
                request(&wrong_kind, 0x8000_1000),
                &mut output
            ),
            Err(Error::UnsupportedFrameControl)
        );
        let mut protected = valid.clone();
        protected[1] = 0x40;
        assert_eq!(
            encode_wmi_mgmt_tx(
                permit,
                7,
                pool(),
                request(&protected, 0x8000_1000),
                &mut output
            ),
            Err(Error::UnsupportedFrameControl)
        );
        let mut wrong_peer = valid.clone();
        wrong_peer[10] ^= 2;
        assert_eq!(
            encode_wmi_mgmt_tx(
                permit,
                7,
                pool(),
                request(&wrong_peer, 0x8000_1000),
                &mut output
            ),
            Err(Error::WrongSource)
        );
        assert_eq!(
            encode_wmi_mgmt_tx(permit, 7, pool(), request(&valid, 0x8001_0000), &mut output),
            Err(Error::FrameOutsideDmaPool)
        );
        assert_eq!(output, [0xa5; 104]);
    }

    #[test]
    fn insufficient_output_is_non_mutating() {
        let frame = auth_frame();
        let mut output = [0xa5; 71];
        assert_eq!(
            encode_wmi_mgmt_tx(
                capability(true),
                7,
                pool(),
                request(&frame, 0x8000_1000),
                &mut output,
            ),
            Err(Error::OutputTooSmall {
                required: 72,
                available: 71,
            })
        );
        assert_eq!(output, [0xa5; 71]);
    }
}
