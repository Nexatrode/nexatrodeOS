//! Bounded sDDF Ethernet-to-CCMP-to-QCACLD-CE4 transmit path.
//!
//! This composes queue polling, the WPA2 controlled-port gate, software CCMP,
//! the source-derived HTT/CE4 planner, and the native gated CE publisher.
#![forbid(unsafe_code)]

use crate::ieee80211_supplicant::{SupplicantError, Wpa2Supplicant};
use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_native_io::{Gate, NativeIo, TxPublishError};
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_wmi_ce::{DmaPool, ServiceCeMapping};
use crate::sunfish_qcacld_ce_tx::{self, CeTxBuffers, CeTxPlan};
use crate::sunfish_qcacld_htc::HttEndpoint;
use crate::sunfish_qcacld_tx::TxMetadata;
use crate::sunfish_wlan_net_source::{self, EthernetFrameSource};

/// Pinned sDDF `NET_BUFFER_SIZE` from the network headers used by the bridge.
pub const SDDF_NET_BUFFER_BYTES: usize = 2048;
const ETHERNET_HEADER_BYTES: usize = 14;
const NATIVE_WIFI_HEADER_BYTES: usize = 24;
const LLC_SNAP_BYTES: usize = 8;
const CCMP_OVERHEAD_BYTES: usize = 16;
pub const MAX_UNPROTECTED_NATIVE_FRAME_BYTES: usize =
    SDDF_NET_BUFFER_BYTES - ETHERNET_HEADER_BYTES + NATIVE_WIFI_HEADER_BYTES + LLC_SNAP_BYTES;
pub const MAX_PROTECTED_NATIVE_FRAME_BYTES: usize =
    MAX_UNPROTECTED_NATIVE_FRAME_BYTES + CCMP_OVERHEAD_BYTES;

const fn preflight_frame() -> [u8; MAX_PROTECTED_NATIVE_FRAME_BYTES] {
    let mut frame = [0; MAX_PROTECTED_NATIVE_FRAME_BYTES];
    // Little-endian 802.11 Data + To-DS frame control. The payload is never
    // transmitted; it lets the planner conservatively validate the full
    // protected-frame IOVA span before dequeuing a network packet.
    frame[0] = 0x08;
    frame[1] = 0x01;
    frame
}

static PREFLIGHT_FRAME: [u8; MAX_PROTECTED_NATIVE_FRAME_BYTES] = preflight_frame();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotAuthorized,
    InvalidStagingCapacity,
    InvalidFrameSequence,
    FirmwareEncryptionConflict,
    Source(sunfish_wlan_net_source::Error),
    Supplicant(SupplicantError),
    Plan(sunfish_qcacld_ce_tx::Error),
}

pub struct StagingBuffers<'a> {
    /// Private copy target for the shared sDDF TX frame.
    pub ethernet: &'a mut [u8],
    /// Temporary unprotected Native-WiFi frame; the supplicant clears it.
    pub native: &'a mut [u8],
    /// Private CCMP output staging. The guarded CE publisher copies it to DMA.
    pub protected: &'a mut [u8],
}

#[derive(Clone, Copy)]
pub struct PreparedTx<'a> {
    plan: CeTxPlan,
    frame: &'a [u8],
}

impl<'a> PreparedTx<'a> {
    pub const fn plan(self) -> CeTxPlan {
        self.plan
    }

    pub const fn frame(self) -> &'a [u8] {
        self.frame
    }
}

/// Authority-bearing CE publisher used by the composed packet path.
///
/// The production implementation delegates to `NativeIo`, which revalidates
/// live CE, endpoint, DMA, SMMU, ring, and generation state under its Gate.
pub trait QcacldTxPublisher {
    fn publish_qcacld_data_tx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: CeTxPlan,
        frame: &[u8],
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError>;
}

impl<G: Gate> QcacldTxPublisher for NativeIo<G> {
    fn publish_qcacld_data_tx(
        &mut self,
        generation: u64,
        profile: Profile,
        rings: &RingPlan,
        plan: CeTxPlan,
        frame: &[u8],
        cursor: &mut RingCursor,
    ) -> Result<(), TxPublishError> {
        NativeIo::publish_qcacld_data_tx(self, generation, profile, rings, plan, frame, cursor)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransmitError {
    Prepare(Error),
    Publish(TxPublishError),
}

#[derive(Clone, Copy)]
pub struct TxPlanContext<'a> {
    pub mappings: &'a [ServiceCeMapping],
    pub endpoint: HttEndpoint,
    /// 12-bit 802.11 sequence number for the encrypted data frame.
    pub frame_sequence: u16,
    /// HTC sequence number for the data transport frame.
    pub htc_sequence: u8,
    pub metadata: TxMetadata,
    pub buffers: CeTxBuffers,
    pub dma_pool: DmaPool,
    pub profile: Profile,
    pub rings: &'a RingPlan,
}

/// Poll and prepare at most one protected data packet, leaving CE publication
/// to the caller.
///
/// Authorization, staging capacities, software-encryption policy, and the
/// largest possible protected frame's CE4/DMA plan are checked before the
/// source callback can dequeue a shared network buffer. The returned frame
/// borrows the private `protected` staging buffer.
pub fn plan_next_sddf_ethernet_tx(
    source: &mut impl EthernetFrameSource,
    supplicant: &mut Wpa2Supplicant<'_>,
    staging: StagingBuffers<'_>,
    context: TxPlanContext<'_>,
    cursor: RingCursor,
) -> Result<Option<CeTxPlan>, Error> {
    Ok(
        prepare_next_sddf_ethernet_tx(source, supplicant, staging, context, cursor)?
            .map(PreparedTx::plan),
    )
}

/// Prepare one packet and borrow its protected staging bytes for a publisher.
pub fn prepare_next_sddf_ethernet_tx<'a>(
    source: &mut impl EthernetFrameSource,
    supplicant: &mut Wpa2Supplicant<'_>,
    staging: StagingBuffers<'a>,
    context: TxPlanContext<'_>,
    cursor: RingCursor,
) -> Result<Option<PreparedTx<'a>>, Error> {
    if !supplicant.is_authorized() {
        return Err(Error::NotAuthorized);
    }
    if context.frame_sequence > 0x0fff {
        return Err(Error::InvalidFrameSequence);
    }
    if context.metadata.hardware_encrypt {
        return Err(Error::FirmwareEncryptionConflict);
    }
    if staging.ethernet.len() < SDDF_NET_BUFFER_BYTES
        || staging.native.len() < MAX_UNPROTECTED_NATIVE_FRAME_BYTES
        || staging.protected.len() < MAX_PROTECTED_NATIVE_FRAME_BYTES
    {
        return Err(Error::InvalidStagingCapacity);
    }

    // Validate all static CE route, endpoint, ring, overlap, and maximum-frame
    // IOVA constraints before asking the sDDF bridge to retire a packet.
    plan_native_frame(
        &PREFLIGHT_FRAME,
        context,
        cursor,
        TxMetadata {
            hardware_encrypt: false,
            ..context.metadata
        },
    )
    .map_err(Error::Plan)?;

    let ethernet_bytes = source.next_frame(staging.ethernet).map_err(Error::Source)?;
    let Some(ethernet_bytes) = ethernet_bytes else {
        return Ok(None);
    };
    if !(ETHERNET_HEADER_BYTES..=staging.ethernet.len()).contains(&ethernet_bytes)
        || ethernet_bytes > SDDF_NET_BUFFER_BYTES
    {
        return Err(Error::Source(
            sunfish_wlan_net_source::Error::InvalidFrameLength {
                observed: ethernet_bytes,
                capacity: staging.ethernet.len().min(SDDF_NET_BUFFER_BYTES),
            },
        ));
    }

    let protected_bytes = supplicant
        .encrypt_ethernet_frame(
            &staging.ethernet[..ethernet_bytes],
            context.frame_sequence,
            staging.native,
            staging.protected,
        )
        .map_err(Error::Supplicant)?;
    let metadata = TxMetadata {
        hardware_encrypt: false,
        ..context.metadata
    };
    let plan = plan_native_frame(
        &staging.protected[..protected_bytes],
        context,
        cursor,
        metadata,
    )
    .map_err(Error::Plan)?;
    Ok(Some(PreparedTx {
        plan,
        frame: &staging.protected[..protected_bytes],
    }))
}

/// Prepare and publish at most one sDDF Ethernet frame.
///
/// If the live CE lease is lost after the shared sDDF buffer has been copied,
/// publication fails closed and that packet is dropped. A `Uncertain` error
/// must quarantine the queue; callers must never retry the same plan.
pub fn publish_next_sddf_ethernet_tx(
    publisher: &mut impl QcacldTxPublisher,
    generation: u64,
    source: &mut impl EthernetFrameSource,
    supplicant: &mut Wpa2Supplicant<'_>,
    staging: StagingBuffers<'_>,
    context: TxPlanContext<'_>,
    cursor: &mut RingCursor,
) -> Result<bool, TransmitError> {
    let prepared = prepare_next_sddf_ethernet_tx(source, supplicant, staging, context, *cursor)
        .map_err(TransmitError::Prepare)?;
    let Some(prepared) = prepared else {
        return Ok(false);
    };
    publisher
        .publish_qcacld_data_tx(
            generation,
            context.profile,
            context.rings,
            prepared.plan,
            prepared.frame,
            cursor,
        )
        .map_err(TransmitError::Publish)?;
    Ok(true)
}

fn plan_native_frame(
    frame: &[u8],
    context: TxPlanContext<'_>,
    cursor: RingCursor,
    metadata: TxMetadata,
) -> Result<CeTxPlan, sunfish_qcacld_ce_tx::Error> {
    sunfish_qcacld_ce_tx::plan_native_wifi_tx(
        context.mappings,
        context.endpoint,
        context.htc_sequence,
        frame,
        metadata,
        context.buffers,
        context.dma_pool,
        context.profile,
        context.rings,
        cursor,
    )
}
