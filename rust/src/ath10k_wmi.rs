//! Host-testable WMI wire primitives for the ath10k protocol model.
//!
//! The module keeps the WMI-TLV and legacy WMI 10.4 event layouts distinct.
//! It does not service firmware memory requests, allocate DMA buffers, or
//! access the device.

use crate::ath10k_firmware::Ath10kFirmwareMetadata;

const WMI_TLV_HEADER_SIZE: usize = 4;
const WMI_TLV_ARRAY_UINT32: u16 = 16;
const WMI_TLV_ARRAY_STRUCT: u16 = 18;
const WMI_TLV_SERVICE_READY_EVENT: u16 = 32;
const WMI_TLV_WLAN_HOST_MEM_REQ: u16 = 34;
const WMI_TLV_READY_EVENT: u16 = 35;
const WMI_SERVICE_READY_FIXED_SIZE: usize = 104;
const WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET: usize = 72;
const WMI_HOST_MEM_REQ_FIXED_SIZE: usize = 16;
const WMI_10_4_SERVICE_READY_FIXED_SIZE: usize = 160;
const WMI_10_4_SERVICE_BITMAP_OFFSET: usize = 20;
const WMI_10_4_SERVICE_BITMAP_BYTES: usize = 16 * 4;
const WMI_10_4_SERVICE_READY_NUM_MEM_REQS_OFFSET: usize = 156;
const WMI_10_4_READY_EVENT_FIXED_SIZE: usize = 20;
const WMI_10_4_CMD_HEADER_SIZE: usize = 4;
/// The pinned `enum wmi_10_4_cmd_id` assigns INIT immediately after the
/// `0x9fff` end marker, so the command header carries `0xa000`.
pub const WMI_10_4_INIT_CMD_ID: u32 = 0xa000;
/// Pinned ath10k `WMI_MAX_MEM_REQS`; bound both event parsing and fixed-size
/// planning to the same number of request descriptors as Linux.
pub const WMI_MAX_HOST_MEMORY_REQUESTS: usize = 16;
/// Sunfish SMMU page size used to isolate each host-memory chunk in the pool.
pub const WMI_HOST_MEMORY_IOVA_ALIGNMENT: u64 = 4096;
const WMI_HOST_MEMORY_CHUNK_ARRAY_HEADER_BYTES: usize = 4;
const WMI_HOST_MEMORY_CHUNK_BYTES: usize = 12;
/// `struct wmi_resource_config_10_4` has 42 packed little-endian words in
/// the pinned ath10k ABI.
pub const WMI_10_4_RESOURCE_CONFIG_WORDS: usize = 42;
pub const WMI_10_4_RESOURCE_CONFIG_BYTES: usize = WMI_10_4_RESOURCE_CONFIG_WORDS * 4;
// The pinned C ABI has a one-item union in wmi_host_mem_chunks so older
// firmware receives storage for at least one item even when count is zero.
const WMI_10_4_INIT_BASE_BYTES: usize = WMI_10_4_RESOURCE_CONFIG_BYTES
    + WMI_HOST_MEMORY_CHUNK_ARRAY_HEADER_BYTES
    + WMI_HOST_MEMORY_CHUNK_BYTES;
const WMI_READY_EVENT_FIXED_SIZE: usize = 36;
const WMI_NUM_UNITS_IS_NUM_VDEVS: u32 = 1 << 0;
const WMI_NUM_UNITS_IS_NUM_PEERS: u32 = 1 << 1;
const WMI_NUM_UNITS_IS_NUM_ACTIVE_PEERS: u32 = 1 << 2;
/// Raw TLV service-bitmap position of `WMI_TLV_SERVICE_RX_FULL_REORDER` in
/// the pinned ath10k `wmi-tlv.h` enum.
pub const WMI_TLV_SERVICE_RX_FULL_REORDER_INDEX: usize = 65;
const WMI_10_4_SERVICE_PEER_CACHING_INDEX: usize = 37;
const TARGET_10_4_NUM_VDEVS: u32 = 16;
const TARGET_HL_TLV_NUM_PEERS: u32 = 33;
const TARGET_10_4_NUM_QCACHE_PEERS_MAX: u32 = 512;
const TARGET_10_4_QCACHE_ACTIVE_PEERS: u32 = 50;
const TARGET_10_4_QCACHE_ACTIVE_PEERS_PFC: u32 = 35;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiTlvError {
    HeaderTruncated,
    ValueTruncated,
    LengthOverflow,
}

/// One borrowed WMI-TLV record. On wire, the little-endian length precedes
/// the little-endian tag, followed immediately by `value` with no padding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiTlvRecord<'a> {
    pub tag: u16,
    pub value: &'a [u8],
}

pub struct WmiTlvIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiServiceReadyError {
    Tlv(WmiTlvError),
    ServiceReadyMissing,
    ServiceReadyDuplicate,
    ServiceReadyTooShort,
    ServiceBitmapMissing,
    ServiceBitmapMalformed,
    MemoryRequestsMissing,
    MemoryRequestsDuplicate,
    MemoryRequestTagUnexpected,
    MemoryRequestTooShort,
    MemoryRequestCountMismatch,
    TooManyMemoryRequests,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiHostMemoryRequestError {
    WrongTag,
    WrongLength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wmi10_4ServiceReadyError {
    EventTooShort,
    TooManyMemoryRequests,
    MemoryRequestsTruncated,
    LengthOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wmi10_4ReadyError {
    EventTooShort,
}

/// Wire-level fields from one WMI host-memory request.
///
/// `num_unit_info` and `num_units` intentionally remain uninterpreted: Linux
/// assigns meaning to these values using firmware flags and runtime resource
/// configuration. This decoder cannot turn a request into an allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiHostMemoryRequest {
    pub request_id: u32,
    pub unit_size: u32,
    pub num_unit_info: u32,
    pub num_units: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiHostMemoryCountHint {
    Fixed,
    Vdevs,
    Peers,
    ActivePeers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiHostMemoryPolicy {
    /// Request ID and size expected from the selected firmware profile.
    pub request_id: u32,
    pub unit_size: u32,
    /// Count mode accepted by this profile.
    pub count_hint: WmiHostMemoryCountHint,
    /// Count fixed by trusted driver configuration, never by firmware.
    pub unit_count: u32,
    /// Per-request memory budget from the same trusted profile.
    pub max_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiHostMemoryPlan {
    pub request_id: u32,
    pub unit_count: u32,
    pub rounded_unit_size: u32,
    pub byte_len: u32,
}

/// Fixed-capacity allocation plan for one complete service-ready event.
/// Plans preserve firmware wire order after matching against trusted policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiHostMemoryBatchPlan<const N: usize> {
    pub requests: [WmiHostMemoryPlan; N],
    pub total_bytes: u64,
}

/// One WMI INIT host-memory chunk. `iova` is the DMA address placed on wire;
/// the backing frames and translated SMMU mapping must be supplied by a
/// separate authorized platform allocator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiHostMemoryChunk {
    pub request_id: u32,
    pub iova: u32,
    pub byte_len: u32,
}

/// Bounded IOVA placement for all chunks in one validated batch. Each chunk
/// occupies a disjoint, page-aligned span; its wire size excludes final-page
/// padding. Fields stay private so callers cannot forge an encoded assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiHostMemoryAssignment<const N: usize> {
    chunks: [WmiHostMemoryChunk; N],
    reserved_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiHostMemoryAssignmentError {
    TooManyChunks,
    PoolMisaligned,
    InvalidPlan,
    DuplicateRequestId,
    TotalSizeMismatch,
    AddressOverflow,
    AddressOutOfRange,
    PoolTooSmall,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiInit10_4EncodeError {
    ResourceConfigWrongSize { expected: usize, observed: usize },
    OutputTooSmall { required: usize, available: usize },
    Assignment(WmiHostMemoryAssignmentError),
}

/// Complete WCN3990 WMI 10.4 INIT resource block from the pinned ath10k
/// policy. The peer-flow-control choice must come from validated API-5
/// firmware metadata; this profile does not establish firmware compatibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiWcn3990ResourceConfig10_4 {
    bytes: [u8; WMI_10_4_RESOURCE_CONFIG_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiWcn3990ResourceConfigError {
    WmiOperationVersionMissing,
    UnsupportedWmiOperationVersion(u32),
}

impl WmiWcn3990ResourceConfig10_4 {
    /// Build the WCN3990 profile from parsed API metadata and SERVICE_READY.
    ///
    /// The pinned core starts with 16 vdevs and a WCN3990 hardware override of
    /// 33 peers. SERVICE_READY may replace peer/TID counts when the firmware
    /// advertises peer caching: 512 cached peers plus vdevs, and either 50 or
    /// 35 active peers plus vdevs depending on `peer-flow-ctrl`. The firmware
    /// feature also selects 1424 or 2500 MSDU descriptors. All other words
    /// follow `TARGET_10_4_*` and the WCN3990 chain masks (`0x7`).
    ///
    /// Rejects missing/unsupported WMI versions before producing an INIT
    /// resource block. The WMI 10.4 parser always exposes its fixed 16-word
    /// service bitmap, so a missing capability bit means disabled.
    pub fn from_firmware_metadata(
        metadata: &Ath10kFirmwareMetadata,
        service_ready: &Wmi10_4ServiceReadyView<'_>,
    ) -> Result<Self, WmiWcn3990ResourceConfigError> {
        match metadata.wmi_op_version() {
            Some(6) => {
                let peer_caching =
                    service_ready.service_enabled(WMI_10_4_SERVICE_PEER_CACHING_INDEX);
                Ok(Self::from_features(
                    metadata.has_peer_flow_control(),
                    peer_caching,
                ))
            }
            Some(version) => {
                Err(WmiWcn3990ResourceConfigError::UnsupportedWmiOperationVersion(version))
            }
            None => Err(WmiWcn3990ResourceConfigError::WmiOperationVersionMissing),
        }
    }

    fn from_features(peer_flow_control: bool, peer_caching: bool) -> Self {
        let num_msdu_desc = if peer_flow_control { 2500 } else { 1424 };
        let (num_peers, num_active_peers, num_tids) = if peer_caching {
            let active_peers = (if peer_flow_control {
                TARGET_10_4_QCACHE_ACTIVE_PEERS_PFC
            } else {
                TARGET_10_4_QCACHE_ACTIVE_PEERS
            }) + TARGET_10_4_NUM_VDEVS;
            (
                TARGET_10_4_NUM_QCACHE_PEERS_MAX + TARGET_10_4_NUM_VDEVS,
                active_peers,
                active_peers * 2,
            )
        } else {
            (TARGET_HL_TLV_NUM_PEERS, 0, 96)
        };
        let words: [u32; WMI_10_4_RESOURCE_CONFIG_WORDS] = [
            TARGET_10_4_NUM_VDEVS, // num_vdevs
            num_peers,             // num_peers; service-ready may enable qcache
            num_active_peers,      // num_active_peers
            0,                     // num_offload_peers
            0,                     // num_offload_reorder_buffs
            2,                     // num_peer_keys
            num_tids,              // num_tids; qcache sets this to active peers * 2
            32,                    // ast_skid_limit
            7,                     // tx_chain_mask
            7,                     // rx_chain_mask
            100,                   // rx_timeout_pri[0]
            100,                   // rx_timeout_pri[1]
            100,                   // rx_timeout_pri[2]
            40,                    // rx_timeout_pri[3]
            1,                     // rx_decap_mode: ATH10K_HW_TXRX_NATIVE_WIFI
            4,                     // scan_max_pending_req
            3,                     // bmiss_offload_max_vdev
            3,                     // roam_offload_max_vdev
            8,                     // roam_offload_max_ap_profiles
            0,                     // num_mcast_groups
            0,                     // num_mcast_table_elems
            0,                     // mcast2ucast_mode
            1024,                  // tx_dbg_log_size
            32,                    // num_wds_entries
            1,                     // dma_burst_size
            0,                     // mac_aggr_delim
            1,                     // rx_skip_defrag_timeout_dup_detection_check
            0,                     // vow_config
            3,                     // gtk_offload_max_vdev
            num_msdu_desc,         // num_msdu_desc
            2,                     // max_frag_entries
            16,                    // max_peer_ext_stats
            0,                     // smart_ant_cap
            0,                     // bk_minfree
            0,                     // be_minfree
            0,                     // vi_minfree
            0,                     // vo_minfree
            1,                     // rx_batchmode
            0,                     // tt_support
            0,                     // atf_config
            1,                     // iphdr_pad_config
            0,                     // qwrap_config
        ];
        let mut bytes = [0; WMI_10_4_RESOURCE_CONFIG_BYTES];
        for (index, word) in words.into_iter().enumerate() {
            let offset = index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        Self { bytes }
    }

    pub const fn as_bytes(&self) -> &[u8; WMI_10_4_RESOURCE_CONFIG_BYTES] {
        &self.bytes
    }

    /// Return Linux ath10k's allocation count for a dynamic host-memory hint.
    /// It adds the target's extra self-peer/vdev slot. For an active-peer
    /// request with zero active peers, the pinned driver falls back to the
    /// configured peer count. Fixed-count requests remain firmware values and
    /// are validated separately against a trusted policy.
    pub fn host_memory_units_for_hint(
        &self,
        hint: WmiHostMemoryCountHint,
    ) -> Result<Option<u32>, WmiHostMemoryPolicyError> {
        let resource_word = |index: usize| {
            let offset = index * 4;
            u32::from_le_bytes([
                self.bytes[offset],
                self.bytes[offset + 1],
                self.bytes[offset + 2],
                self.bytes[offset + 3],
            ])
        };
        let configured_count = match hint {
            WmiHostMemoryCountHint::Fixed => return Ok(None),
            WmiHostMemoryCountHint::Vdevs => resource_word(0),
            WmiHostMemoryCountHint::Peers => resource_word(1),
            WmiHostMemoryCountHint::ActivePeers => {
                let active_peers = resource_word(2);
                if active_peers == 0 {
                    resource_word(1)
                } else {
                    active_peers
                }
            }
        };
        configured_count
            .checked_add(1)
            .map(Some)
            .ok_or(WmiHostMemoryPolicyError::SizeOverflow)
    }
}

impl From<WmiHostMemoryAssignmentError> for WmiInit10_4EncodeError {
    fn from(error: WmiHostMemoryAssignmentError) -> Self {
        Self::Assignment(error)
    }
}

impl<const N: usize> WmiHostMemoryBatchPlan<N> {
    /// Place the complete plan inside one bounded, already-authorized IOVA
    /// pool. Each chunk begins on a 4 KiB boundary, and the entire address
    /// interval must fit the 32-bit pointer field in the ath10k WMI ABI.
    /// This creates no mapping or physical backing and grants no DMA access.
    pub fn assign_iova_pool(
        &self,
        pool_base_iova: u64,
        pool_bytes: u64,
    ) -> Result<WmiHostMemoryAssignment<N>, WmiHostMemoryAssignmentError> {
        if N > WMI_MAX_HOST_MEMORY_REQUESTS {
            return Err(WmiHostMemoryAssignmentError::TooManyChunks);
        }
        if pool_base_iova % WMI_HOST_MEMORY_IOVA_ALIGNMENT != 0 {
            return Err(WmiHostMemoryAssignmentError::PoolMisaligned);
        }
        let address_space_end = u64::from(u32::MAX) + 1;
        let pool_end = pool_base_iova
            .checked_add(pool_bytes)
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        if pool_base_iova > address_space_end || pool_end > address_space_end {
            return Err(WmiHostMemoryAssignmentError::AddressOutOfRange);
        }

        let empty = WmiHostMemoryChunk {
            request_id: 0,
            iova: 0,
            byte_len: 0,
        };
        let mut chunks = [empty; N];
        let mut cursor = pool_base_iova;
        let mut total_bytes = 0u64;

        for (index, plan) in self.requests.iter().copied().enumerate() {
            if plan.unit_count == 0
                || plan.rounded_unit_size == 0
                || plan.rounded_unit_size % 4 != 0
                || plan
                    .rounded_unit_size
                    .checked_mul(plan.unit_count)
                    .is_none_or(|size| size != plan.byte_len)
            {
                return Err(WmiHostMemoryAssignmentError::InvalidPlan);
            }
            if self.requests[..index]
                .iter()
                .any(|prior| prior.request_id == plan.request_id)
            {
                return Err(WmiHostMemoryAssignmentError::DuplicateRequestId);
            }

            let chunk_bytes = u64::from(plan.byte_len);
            total_bytes = total_bytes
                .checked_add(chunk_bytes)
                .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
            let reserved_bytes = chunk_bytes
                .checked_add(WMI_HOST_MEMORY_IOVA_ALIGNMENT - 1)
                .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?
                & !(WMI_HOST_MEMORY_IOVA_ALIGNMENT - 1);
            let chunk_end = cursor
                .checked_add(chunk_bytes)
                .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
            let next_cursor = cursor
                .checked_add(reserved_bytes)
                .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
            if chunk_end > address_space_end || next_cursor > pool_end {
                return Err(WmiHostMemoryAssignmentError::PoolTooSmall);
            }

            chunks[index] = WmiHostMemoryChunk {
                request_id: plan.request_id,
                iova: u32::try_from(cursor)
                    .map_err(|_| WmiHostMemoryAssignmentError::AddressOutOfRange)?,
                byte_len: plan.byte_len,
            };
            cursor = next_cursor;
        }

        if total_bytes != self.total_bytes {
            return Err(WmiHostMemoryAssignmentError::TotalSizeMismatch);
        }
        Ok(WmiHostMemoryAssignment {
            chunks,
            reserved_bytes: cursor - pool_base_iova,
        })
    }
}

impl<const N: usize> WmiHostMemoryAssignment<N> {
    pub const fn chunks(&self) -> &[WmiHostMemoryChunk; N] {
        &self.chunks
    }

    pub const fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }

    /// Encode the count followed by source-layout `{req_id, ptr, size}`
    /// records. Output capacity is checked before any byte is modified.
    pub fn encode_chunk_array(
        &self,
        output: &mut [u8],
    ) -> Result<usize, WmiHostMemoryAssignmentError> {
        if N > WMI_MAX_HOST_MEMORY_REQUESTS {
            return Err(WmiHostMemoryAssignmentError::TooManyChunks);
        }
        let record_bytes = N
            .checked_mul(WMI_HOST_MEMORY_CHUNK_BYTES)
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        let required = WMI_HOST_MEMORY_CHUNK_ARRAY_HEADER_BYTES
            .checked_add(record_bytes)
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        if output.len() < required {
            return Err(WmiHostMemoryAssignmentError::OutputTooSmall);
        }

        output[..required].fill(0);
        output[..4].copy_from_slice(&(N as u32).to_le_bytes());
        for (index, chunk) in self.chunks.iter().copied().enumerate() {
            let start =
                WMI_HOST_MEMORY_CHUNK_ARRAY_HEADER_BYTES + index * WMI_HOST_MEMORY_CHUNK_BYTES;
            output[start..start + 4].copy_from_slice(&chunk.request_id.to_le_bytes());
            output[start + 4..start + 8].copy_from_slice(&chunk.iova.to_le_bytes());
            output[start + 8..start + 12].copy_from_slice(&chunk.byte_len.to_le_bytes());
        }
        Ok(required)
    }

    /// Encode the pinned ath10k `wmi_init_cmd_10_4` body. The 168-byte
    /// resource-config block is already policy-validated caller input. The
    /// body includes the C ABI's one-item minimum union slot plus the flexible
    /// array sized by the actual chunk count; the unused slot remains zero.
    /// The WMI command ID and HTC framing are separate transport data. This
    /// creates no backing pages, SMMU mappings, or DMA authority.
    pub fn encode_init_body_10_4(
        &self,
        resource_config: &[u8],
        output: &mut [u8],
    ) -> Result<usize, WmiInit10_4EncodeError> {
        if N > WMI_MAX_HOST_MEMORY_REQUESTS {
            return Err(WmiInit10_4EncodeError::Assignment(
                WmiHostMemoryAssignmentError::TooManyChunks,
            ));
        }
        if resource_config.len() != WMI_10_4_RESOURCE_CONFIG_BYTES {
            return Err(WmiInit10_4EncodeError::ResourceConfigWrongSize {
                expected: WMI_10_4_RESOURCE_CONFIG_BYTES,
                observed: resource_config.len(),
            });
        }
        let chunk_array_bytes = WMI_HOST_MEMORY_CHUNK_ARRAY_HEADER_BYTES
            .checked_add(
                N.checked_mul(WMI_HOST_MEMORY_CHUNK_BYTES)
                    .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?,
            )
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        let required = WMI_10_4_INIT_BASE_BYTES
            .checked_add(
                N.checked_mul(WMI_HOST_MEMORY_CHUNK_BYTES)
                    .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?,
            )
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        if output.len() < required {
            return Err(WmiInit10_4EncodeError::OutputTooSmall {
                required,
                available: output.len(),
            });
        }

        output[..required].fill(0);
        output[..WMI_10_4_RESOURCE_CONFIG_BYTES].copy_from_slice(resource_config);
        let written = self
            .encode_chunk_array(&mut output[WMI_10_4_RESOURCE_CONFIG_BYTES..required])
            .map_err(WmiInit10_4EncodeError::Assignment)?;
        debug_assert_eq!(written, chunk_array_bytes);
        Ok(required)
    }

    /// Encode the WMI 10.4 INIT command header and body, as passed to
    /// `ath10k_wmi_cmd_send_nowait`. HTC transport framing is still separate.
    /// Validation finishes before writing the command ID; no DMA or transport
    /// operation is performed here.
    pub fn encode_init_command_10_4(
        &self,
        resource_config: &[u8],
        output: &mut [u8],
    ) -> Result<usize, WmiInit10_4EncodeError> {
        if resource_config.len() != WMI_10_4_RESOURCE_CONFIG_BYTES {
            return Err(WmiInit10_4EncodeError::ResourceConfigWrongSize {
                expected: WMI_10_4_RESOURCE_CONFIG_BYTES,
                observed: resource_config.len(),
            });
        }
        if N > WMI_MAX_HOST_MEMORY_REQUESTS {
            return Err(WmiInit10_4EncodeError::Assignment(
                WmiHostMemoryAssignmentError::TooManyChunks,
            ));
        }
        let body_required = WMI_10_4_INIT_BASE_BYTES
            .checked_add(
                N.checked_mul(WMI_HOST_MEMORY_CHUNK_BYTES)
                    .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?,
            )
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        let required = WMI_10_4_CMD_HEADER_SIZE
            .checked_add(body_required)
            .ok_or(WmiHostMemoryAssignmentError::AddressOverflow)?;
        if output.len() < required {
            return Err(WmiInit10_4EncodeError::OutputTooSmall {
                required,
                available: output.len(),
            });
        }

        self.encode_init_body_10_4(
            resource_config,
            &mut output[WMI_10_4_CMD_HEADER_SIZE..required],
        )?;
        output[..WMI_10_4_CMD_HEADER_SIZE].copy_from_slice(&WMI_10_4_INIT_CMD_ID.to_le_bytes());
        Ok(required)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiHostMemoryBatchError {
    TooManyPolicies,
    RequestCountMismatch,
    DuplicatePolicyId,
    UnexpectedTlvTag,
    Tlv(WmiTlvError),
    Request(WmiHostMemoryRequestError),
    UnknownRequestId,
    DuplicateRequestId,
    MissingRequestId,
    Policy(WmiHostMemoryPolicyError),
    TotalSizeOverflow,
    ExceedsAggregateBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiHostMemoryPolicyError {
    RequestIdMismatch,
    CountHintMismatch,
    UnitSizeMismatch,
    UnitCountMismatch,
    EmptyAllocation,
    SizeOverflow,
    ExceedsBudget,
}

impl WmiHostMemoryRequest {
    pub fn parse(record: WmiTlvRecord<'_>) -> Result<Self, WmiHostMemoryRequestError> {
        if record.tag != WMI_TLV_WLAN_HOST_MEM_REQ {
            return Err(WmiHostMemoryRequestError::WrongTag);
        }
        if record.value.len() != WMI_HOST_MEM_REQ_FIXED_SIZE {
            return Err(WmiHostMemoryRequestError::WrongLength);
        }

        let word = |offset: usize| {
            u32::from_le_bytes([
                record.value[offset],
                record.value[offset + 1],
                record.value[offset + 2],
                record.value[offset + 3],
            ])
        };
        Ok(Self {
            request_id: word(0),
            unit_size: word(4),
            num_unit_info: word(8),
            num_units: word(12),
        })
    }

    /// Decode one inline `wlan_host_mem_req` from the non-TLV WMI 10.4
    /// SERVICE_READY event body.
    pub fn parse_10_4(bytes: &[u8]) -> Result<Self, WmiHostMemoryRequestError> {
        if bytes.len() != WMI_HOST_MEM_REQ_FIXED_SIZE {
            return Err(WmiHostMemoryRequestError::WrongLength);
        }
        let word = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        Ok(Self {
            request_id: word(0),
            unit_size: word(4),
            num_unit_info: word(8),
            num_units: word(12),
        })
    }

    /// Interpret only the known single-bit count modes from the ath10k ABI.
    ///
    /// `None` means the flags are unknown or combined. Callers still need a
    /// firmware-specific policy to decide whether any returned mode is safe.
    pub const fn count_hint(&self) -> Option<WmiHostMemoryCountHint> {
        match self.num_unit_info {
            0 => Some(WmiHostMemoryCountHint::Fixed),
            WMI_NUM_UNITS_IS_NUM_VDEVS => Some(WmiHostMemoryCountHint::Vdevs),
            WMI_NUM_UNITS_IS_NUM_PEERS => Some(WmiHostMemoryCountHint::Peers),
            WMI_NUM_UNITS_IS_NUM_ACTIVE_PEERS => Some(WmiHostMemoryCountHint::ActivePeers),
            _ => None,
        }
    }

    /// Produce a checked size plan using only a caller-supplied trusted rule.
    /// This computes no physical address and performs no allocation or mapping.
    pub fn checked_plan(
        &self,
        policy: WmiHostMemoryPolicy,
    ) -> Result<WmiHostMemoryPlan, WmiHostMemoryPolicyError> {
        if self.request_id != policy.request_id {
            return Err(WmiHostMemoryPolicyError::RequestIdMismatch);
        }
        if self.count_hint() != Some(policy.count_hint) {
            return Err(WmiHostMemoryPolicyError::CountHintMismatch);
        }
        if self.unit_size != policy.unit_size {
            return Err(WmiHostMemoryPolicyError::UnitSizeMismatch);
        }

        let expected_wire_count = match policy.count_hint {
            WmiHostMemoryCountHint::Fixed => policy.unit_count,
            WmiHostMemoryCountHint::Vdevs
            | WmiHostMemoryCountHint::Peers
            | WmiHostMemoryCountHint::ActivePeers => 0,
        };
        if self.num_units != expected_wire_count {
            return Err(WmiHostMemoryPolicyError::UnitCountMismatch);
        }
        if policy.unit_count == 0 || self.unit_size == 0 {
            return Err(WmiHostMemoryPolicyError::EmptyAllocation);
        }

        let rounded_unit_size = self
            .unit_size
            .checked_add(3)
            .ok_or(WmiHostMemoryPolicyError::SizeOverflow)?
            & !3;
        let byte_len = rounded_unit_size
            .checked_mul(policy.unit_count)
            .ok_or(WmiHostMemoryPolicyError::SizeOverflow)?;
        if byte_len > policy.max_bytes {
            return Err(WmiHostMemoryPolicyError::ExceedsBudget);
        }

        Ok(WmiHostMemoryPlan {
            request_id: self.request_id,
            unit_count: policy.unit_count,
            rounded_unit_size,
            byte_len,
        })
    }
}

fn plan_host_memory_requests<const N: usize, I>(
    declared_count: u32,
    policies: &[WmiHostMemoryPolicy; N],
    max_total_bytes: u64,
    requests: I,
) -> Result<WmiHostMemoryBatchPlan<N>, WmiHostMemoryBatchError>
where
    I: Iterator<Item = Result<WmiHostMemoryRequest, WmiHostMemoryBatchError>>,
{
    if N > WMI_MAX_HOST_MEMORY_REQUESTS {
        return Err(WmiHostMemoryBatchError::TooManyPolicies);
    }
    if N != declared_count as usize {
        return Err(WmiHostMemoryBatchError::RequestCountMismatch);
    }
    for left in 0..N {
        if policies[left + 1..]
            .iter()
            .any(|right| right.request_id == policies[left].request_id)
        {
            return Err(WmiHostMemoryBatchError::DuplicatePolicyId);
        }
    }

    let empty = WmiHostMemoryPlan {
        request_id: 0,
        unit_count: 0,
        rounded_unit_size: 0,
        byte_len: 0,
    };
    let mut plans = [empty; N];
    let mut seen = [false; N];
    let mut total_bytes = 0u64;
    let mut output_index = 0usize;

    for request in requests {
        let request = request?;
        if output_index >= N {
            return Err(WmiHostMemoryBatchError::RequestCountMismatch);
        }
        let policy_index = policies
            .iter()
            .position(|policy| policy.request_id == request.request_id)
            .ok_or(WmiHostMemoryBatchError::UnknownRequestId)?;
        if seen[policy_index] {
            return Err(WmiHostMemoryBatchError::DuplicateRequestId);
        }

        let plan = request
            .checked_plan(policies[policy_index])
            .map_err(WmiHostMemoryBatchError::Policy)?;
        total_bytes = total_bytes
            .checked_add(u64::from(plan.byte_len))
            .ok_or(WmiHostMemoryBatchError::TotalSizeOverflow)?;
        if total_bytes > max_total_bytes {
            return Err(WmiHostMemoryBatchError::ExceedsAggregateBudget);
        }

        plans[output_index] = plan;
        output_index += 1;
        seen[policy_index] = true;
    }

    if seen.iter().any(|is_seen| !is_seen) {
        return Err(WmiHostMemoryBatchError::MissingRequestId);
    }
    Ok(WmiHostMemoryBatchPlan {
        requests: plans,
        total_bytes,
    })
}

impl<'a> Wmi10_4ServiceReadyView<'a> {
    /// Parse the fixed, non-TLV WMI 10.4 SERVICE_READY body used by the
    /// ath10k `wmi_10_4_ops` selected from API-5 metadata version 6.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Wmi10_4ServiceReadyError> {
        if bytes.len() < WMI_10_4_SERVICE_READY_FIXED_SIZE {
            return Err(Wmi10_4ServiceReadyError::EventTooShort);
        }
        let count_at = WMI_10_4_SERVICE_READY_NUM_MEM_REQS_OFFSET;
        let declared_host_memory_request_count = u32::from_le_bytes([
            bytes[count_at],
            bytes[count_at + 1],
            bytes[count_at + 2],
            bytes[count_at + 3],
        ]);
        if declared_host_memory_request_count as usize > WMI_MAX_HOST_MEMORY_REQUESTS {
            return Err(Wmi10_4ServiceReadyError::TooManyMemoryRequests);
        }
        let request_bytes = (declared_host_memory_request_count as usize)
            .checked_mul(WMI_HOST_MEM_REQ_FIXED_SIZE)
            .ok_or(Wmi10_4ServiceReadyError::LengthOverflow)?;
        let requests_start = WMI_10_4_SERVICE_READY_FIXED_SIZE;
        let requests_end = requests_start
            .checked_add(request_bytes)
            .ok_or(Wmi10_4ServiceReadyError::LengthOverflow)?;
        if bytes.len() < requests_end {
            return Err(Wmi10_4ServiceReadyError::MemoryRequestsTruncated);
        }
        let bitmap_end = WMI_10_4_SERVICE_BITMAP_OFFSET + WMI_10_4_SERVICE_BITMAP_BYTES;
        Ok(Self {
            event: bytes,
            service_bitmap: &bytes[WMI_10_4_SERVICE_BITMAP_OFFSET..bitmap_end],
            host_memory_requests: &bytes[requests_start..requests_end],
            trailing: &bytes[requests_end..],
            declared_host_memory_request_count,
        })
    }

    pub fn service_word(&self, index: usize) -> Option<u32> {
        let start = index.checked_mul(4)?;
        let word = self.service_bitmap.get(start..start.checked_add(4)?)?;
        Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    }

    /// The WMI 10.4 peer-caching capability is raw service bit 37. Linux maps
    /// it to its internal `WMI_SERVICE_PEER_CACHING` bit 78 afterward.
    pub fn service_enabled(&self, service_index: usize) -> bool {
        let word = self.service_word(service_index / 32).unwrap_or(0);
        word & (1u32 << (service_index % 32)) != 0
    }

    pub fn host_memory_request_records(&self) -> impl Iterator<Item = &'a [u8]> {
        self.host_memory_requests
            .chunks_exact(WMI_HOST_MEM_REQ_FIXED_SIZE)
    }

    pub fn plan_host_memory<const N: usize>(
        &self,
        policies: &[WmiHostMemoryPolicy; N],
        max_total_bytes: u64,
    ) -> Result<WmiHostMemoryBatchPlan<N>, WmiHostMemoryBatchError> {
        let requests = self.host_memory_request_records().map(|record| {
            WmiHostMemoryRequest::parse_10_4(record).map_err(WmiHostMemoryBatchError::Request)
        });
        plan_host_memory_requests(
            self.declared_host_memory_request_count,
            policies,
            max_total_bytes,
            requests,
        )
    }

    pub fn plan_host_memory_for_resource_config<const N: usize>(
        &self,
        policies: &[WmiHostMemoryPolicy; N],
        resource_config: &WmiWcn3990ResourceConfig10_4,
        max_total_bytes: u64,
    ) -> Result<WmiHostMemoryBatchPlan<N>, WmiHostMemoryBatchError> {
        for policy in policies {
            let expected_units = resource_config
                .host_memory_units_for_hint(policy.count_hint)
                .map_err(WmiHostMemoryBatchError::Policy)?;
            if expected_units.is_some_and(|expected| expected != policy.unit_count) {
                return Err(WmiHostMemoryBatchError::Policy(
                    WmiHostMemoryPolicyError::UnitCountMismatch,
                ));
            }
        }
        self.plan_host_memory(policies, max_total_bytes)
    }
}

impl Wmi10_4ReadyEvent<'_> {
    /// Parse the non-TLV WMI 10.4 READY body (WMI command header already
    /// removed), preserving the reserved MAC bytes and any trailing payload.
    pub fn parse(bytes: &[u8]) -> Result<Wmi10_4ReadyEvent<'_>, Wmi10_4ReadyError> {
        if bytes.len() < WMI_10_4_READY_EVENT_FIXED_SIZE {
            return Err(Wmi10_4ReadyError::EventTooShort);
        }
        let mut mac_address = [0u8; 6];
        mac_address.copy_from_slice(&bytes[8..14]);
        Ok(Wmi10_4ReadyEvent {
            software_version: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            abi_version: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            mac_address,
            mac_reserved: [bytes[14], bytes[15]],
            status: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            trailing: &bytes[WMI_10_4_READY_EVENT_FIXED_SIZE..],
        })
    }
}

impl<'a> WmiServiceReadyView<'a> {
    /// Validate the complete firmware request set against trusted per-ID
    /// policies and one aggregate byte budget. No addresses are produced and
    /// no memory is allocated. Unknown, duplicate, missing, or mismatched
    /// requests reject the whole batch.
    pub fn plan_host_memory<const N: usize>(
        &self,
        policies: &[WmiHostMemoryPolicy; N],
        max_total_bytes: u64,
    ) -> Result<WmiHostMemoryBatchPlan<N>, WmiHostMemoryBatchError> {
        let requests = self.host_memory_request_records().map(|record| {
            let record = record.map_err(WmiHostMemoryBatchError::Tlv)?;
            if record.tag != WMI_TLV_WLAN_HOST_MEM_REQ {
                return Err(WmiHostMemoryBatchError::UnexpectedTlvTag);
            }
            WmiHostMemoryRequest::parse(record).map_err(WmiHostMemoryBatchError::Request)
        });
        plan_host_memory_requests(
            self.declared_host_memory_request_count,
            policies,
            max_total_bytes,
            requests,
        )
    }

    /// Validate dynamic unit counts against the source-derived WCN3990 INIT
    /// resource profile before applying the trusted per-request policies.
    /// Fixed-count requests remain controlled by their explicit policy.
    pub fn plan_host_memory_for_resource_config<const N: usize>(
        &self,
        policies: &[WmiHostMemoryPolicy; N],
        resource_config: &WmiWcn3990ResourceConfig10_4,
        max_total_bytes: u64,
    ) -> Result<WmiHostMemoryBatchPlan<N>, WmiHostMemoryBatchError> {
        for policy in policies {
            let expected_units = resource_config
                .host_memory_units_for_hint(policy.count_hint)
                .map_err(WmiHostMemoryBatchError::Policy)?;
            if expected_units.is_some_and(|expected| expected != policy.unit_count) {
                return Err(WmiHostMemoryBatchError::Policy(
                    WmiHostMemoryPolicyError::UnitCountMismatch,
                ));
            }
        }
        self.plan_host_memory(policies, max_total_bytes)
    }
}

/// A borrowed, validated subset of a WMI service-ready event.
///
/// The event and service bitmap are left encoded exactly as received. The
/// host-memory-request array is only framed as a sequence of tag-34 records;
/// their payloads remain opaque and cannot cause allocations through this API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiServiceReadyView<'a> {
    pub event: &'a [u8],
    pub service_bitmap: &'a [u8],
    pub host_memory_requests: &'a [u8],
    pub declared_host_memory_request_count: u32,
}

/// Borrowed WMI 10.4 SERVICE_READY event body. This is the non-TLV layout
/// selected by ath10k when API-5 metadata advertises WMI operation version
/// 10.4. The fixed service bitmap and inline 16-byte memory requests are
/// validated before the view is returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Wmi10_4ServiceReadyView<'a> {
    pub event: &'a [u8],
    pub service_bitmap: &'a [u8],
    pub host_memory_requests: &'a [u8],
    pub trailing: &'a [u8],
    pub declared_host_memory_request_count: u32,
}

/// Fixed fields from the non-TLV WMI 10.4 READY event. The two reserved bytes
/// in the 8-byte wire MAC address are preserved for diagnostic fidelity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Wmi10_4ReadyEvent<'a> {
    pub software_version: u32,
    pub abi_version: u32,
    pub mac_address: [u8; 6],
    pub mac_reserved: [u8; 2],
    pub status: u32,
    pub trailing: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiReadyError {
    Tlv(WmiTlvError),
    ReadyMissing,
    ReadyDuplicate,
    ReadyTooShort,
}

/// Fixed WMI READY payload fields, decoded from a borrowed TLV event.
///
/// ABI values and firmware status are returned as wire-level integers. Callers
/// must compare them with a firmware-specific policy before advancing startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiReadyEvent {
    pub abi_versions: [u32; 6],
    pub mac_address: [u8; 6],
    pub status: u32,
}

impl WmiReadyEvent {
    pub fn parse(bytes: &[u8]) -> Result<Self, WmiReadyError> {
        let mut event = None;
        for record in WmiTlvIter::new(bytes) {
            let record = record.map_err(WmiReadyError::Tlv)?;
            if record.tag != WMI_TLV_READY_EVENT {
                continue;
            }
            if event.replace(record.value).is_some() {
                return Err(WmiReadyError::ReadyDuplicate);
            }
            if record.value.len() < WMI_READY_EVENT_FIXED_SIZE {
                return Err(WmiReadyError::ReadyTooShort);
            }
        }

        let event = event.ok_or(WmiReadyError::ReadyMissing)?;
        let mut abi_versions = [0u32; 6];
        for (index, abi_version) in abi_versions.iter_mut().enumerate() {
            let start = index * 4;
            *abi_version = u32::from_le_bytes([
                event[start],
                event[start + 1],
                event[start + 2],
                event[start + 3],
            ]);
        }
        let mut mac_address = [0u8; 6];
        mac_address.copy_from_slice(&event[24..30]);
        let status = u32::from_le_bytes([event[32], event[33], event[34], event[35]]);

        Ok(Self {
            abi_versions,
            mac_address,
            status,
        })
    }
}

impl<'a> WmiServiceReadyView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, WmiServiceReadyError> {
        let mut event = None;
        let mut service_bitmap = None;
        let mut host_memory_requests = None;

        for record in WmiTlvIter::new(bytes) {
            let record = record.map_err(WmiServiceReadyError::Tlv)?;
            match record.tag {
                WMI_TLV_SERVICE_READY_EVENT => {
                    if event.replace(record.value).is_some() {
                        return Err(WmiServiceReadyError::ServiceReadyDuplicate);
                    }
                    if record.value.len() < WMI_SERVICE_READY_FIXED_SIZE {
                        return Err(WmiServiceReadyError::ServiceReadyTooShort);
                    }
                }
                WMI_TLV_ARRAY_UINT32 if service_bitmap.is_none() => {
                    if record.value.is_empty() || record.value.len() % 4 != 0 {
                        return Err(WmiServiceReadyError::ServiceBitmapMalformed);
                    }
                    service_bitmap = Some(record.value);
                }
                WMI_TLV_ARRAY_UINT32 => {
                    // Linux uses a later ARRAY_UINT32 for optional DBS modes.
                    // Leave it untouched; only the first array is the service map.
                }
                WMI_TLV_ARRAY_STRUCT => {
                    if host_memory_requests.replace(record.value).is_some() {
                        return Err(WmiServiceReadyError::MemoryRequestsDuplicate);
                    }
                }
                _ => {}
            }
        }

        let event = event.ok_or(WmiServiceReadyError::ServiceReadyMissing)?;
        let service_bitmap = service_bitmap.ok_or(WmiServiceReadyError::ServiceBitmapMissing)?;
        let host_memory_requests =
            host_memory_requests.ok_or(WmiServiceReadyError::MemoryRequestsMissing)?;
        let declared_host_memory_request_count = u32::from_le_bytes([
            event[WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET],
            event[WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET + 1],
            event[WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET + 2],
            event[WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET + 3],
        ]);
        if declared_host_memory_request_count as usize > WMI_MAX_HOST_MEMORY_REQUESTS {
            return Err(WmiServiceReadyError::TooManyMemoryRequests);
        }

        let mut actual_count = 0u32;
        for record in WmiTlvIter::new(host_memory_requests) {
            let record = record.map_err(WmiServiceReadyError::Tlv)?;
            if record.tag != WMI_TLV_WLAN_HOST_MEM_REQ {
                return Err(WmiServiceReadyError::MemoryRequestTagUnexpected);
            }
            if record.value.len() < WMI_HOST_MEM_REQ_FIXED_SIZE {
                return Err(WmiServiceReadyError::MemoryRequestTooShort);
            }
            actual_count = actual_count
                .checked_add(1)
                .ok_or(WmiServiceReadyError::MemoryRequestCountMismatch)?;
            if actual_count as usize > WMI_MAX_HOST_MEMORY_REQUESTS {
                return Err(WmiServiceReadyError::TooManyMemoryRequests);
            }
        }
        if actual_count != declared_host_memory_request_count {
            return Err(WmiServiceReadyError::MemoryRequestCountMismatch);
        }

        Ok(Self {
            event,
            service_bitmap,
            host_memory_requests,
            declared_host_memory_request_count,
        })
    }

    /// Read one little-endian service-bitmap word without allocating.
    pub fn service_word(&self, index: usize) -> Option<u32> {
        let start = index.checked_mul(4)?;
        let word = self.service_bitmap.get(start..start.checked_add(4)?)?;
        Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    }

    /// Return a service bit if its complete bitmap word was supplied. `None`
    /// distinguishes an absent word from an advertised, disabled service.
    pub fn service_enabled(&self, service_index: usize) -> Option<bool> {
        let Some(word) = self.service_word(service_index / 32) else {
            return None;
        };
        Some(word & (1u32 << (service_index % 32)) != 0)
    }

    /// Whether this firmware advertises full-reorder RX, which selects the
    /// explicit-IOVA HTT completion path rather than FIFO slot completion.
    pub fn rx_full_reorder_enabled(&self) -> Option<bool> {
        self.service_enabled(WMI_TLV_SERVICE_RX_FULL_REORDER_INDEX)
    }

    /// Iterate the opaque host-memory-request TLVs after validation.
    pub fn host_memory_request_records(&self) -> WmiTlvIter<'a> {
        WmiTlvIter::new(self.host_memory_requests)
    }
}

impl<'a> WmiTlvIter<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl<'a> Iterator for WmiTlvIter<'a> {
    type Item = Result<WmiTlvRecord<'a>, WmiTlvError>;

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining == 0 {
            return None;
        }
        if remaining < WMI_TLV_HEADER_SIZE {
            self.offset = self.bytes.len();
            return Some(Err(WmiTlvError::HeaderTruncated));
        }

        let header_start = self.offset;
        let value_start = match header_start.checked_add(WMI_TLV_HEADER_SIZE) {
            Some(value_start) => value_start,
            None => {
                self.offset = self.bytes.len();
                return Some(Err(WmiTlvError::LengthOverflow));
            }
        };
        let value_len = usize::from(u16::from_le_bytes([
            self.bytes[header_start],
            self.bytes[header_start + 1],
        ]));
        let value_end = match value_start.checked_add(value_len) {
            Some(value_end) => value_end,
            None => {
                self.offset = self.bytes.len();
                return Some(Err(WmiTlvError::LengthOverflow));
            }
        };
        if value_end > self.bytes.len() {
            self.offset = self.bytes.len();
            return Some(Err(WmiTlvError::ValueTruncated));
        }

        let tag = u16::from_le_bytes([self.bytes[header_start + 2], self.bytes[header_start + 3]]);
        self.offset = value_end;
        Some(Ok(WmiTlvRecord {
            tag,
            value: &self.bytes[value_start..value_end],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append_tlv(out: &mut Vec<u8>, tag: u16, value: &[u8]) {
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(value);
    }

    fn append_firmware_ie(out: &mut Vec<u8>, id: u32, value: &[u8]) {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    fn firmware_metadata(
        peer_flow_control: bool,
        wmi_op_version: Option<u32>,
    ) -> Ath10kFirmwareMetadata {
        let mut bytes = Vec::from(*b"QCA-ATH10K\0");
        bytes.push(0);
        let mut features = [0u8; 3];
        if peer_flow_control {
            features[1] |= 1 << 5;
        }
        append_firmware_ie(&mut bytes, 2, &features);
        if let Some(version) = wmi_op_version {
            append_firmware_ie(&mut bytes, 5, &version.to_le_bytes());
        }
        Ath10kFirmwareMetadata::parse(&bytes).unwrap()
    }

    fn service_ready_fixture(mem_requests: &[u8]) -> Vec<u8> {
        service_ready_fixture_with_count(mem_requests, 1)
    }

    fn service_ready_fixture_with_count(mem_requests: &[u8], request_count: u32) -> Vec<u8> {
        service_ready_fixture_with_bitmap(mem_requests, request_count, &[0x1234, 0x5678])
    }

    fn service_ready_fixture_with_bitmap(
        mem_requests: &[u8],
        request_count: u32,
        service_words: &[u32],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut event = [0u8; WMI_SERVICE_READY_FIXED_SIZE];
        event[WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET..WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET + 4]
            .copy_from_slice(&request_count.to_le_bytes());
        append_tlv(&mut bytes, WMI_TLV_SERVICE_READY_EVENT, &event);
        let mut service_bitmap = Vec::new();
        for word in service_words {
            service_bitmap.extend_from_slice(&word.to_le_bytes());
        }
        append_tlv(&mut bytes, WMI_TLV_ARRAY_UINT32, &service_bitmap);
        append_tlv(&mut bytes, WMI_TLV_ARRAY_STRUCT, mem_requests);
        bytes
    }

    fn service_ready_10_4_fixture(
        mem_requests: &[u8],
        request_count: u32,
        service_words: &[u32; 16],
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; WMI_10_4_SERVICE_READY_FIXED_SIZE];
        for (index, word) in service_words.iter().copied().enumerate() {
            let offset = WMI_10_4_SERVICE_BITMAP_OFFSET + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[WMI_10_4_SERVICE_READY_NUM_MEM_REQS_OFFSET
            ..WMI_10_4_SERVICE_READY_NUM_MEM_REQS_OFFSET + 4]
            .copy_from_slice(&request_count.to_le_bytes());
        bytes.extend_from_slice(mem_requests);
        bytes
    }

    #[test]
    fn wcn3990_10_4_resource_config_matches_reference_profile() {
        let profile = |peer_flow_control: bool, peer_caching: bool| {
            let mut service_words = [0u32; 16];
            if peer_caching {
                service_words[WMI_10_4_SERVICE_PEER_CACHING_INDEX / 32] |=
                    1 << (WMI_10_4_SERVICE_PEER_CACHING_INDEX % 32);
            }
            let event = service_ready_10_4_fixture(&[], 0, &service_words);
            let service_ready = Wmi10_4ServiceReadyView::parse(&event).unwrap();
            WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
                &firmware_metadata(peer_flow_control, Some(6)),
                &service_ready,
            )
            .unwrap()
        };
        let standard = profile(false, false);
        let peer_flow_control = profile(true, false);
        let peer_caching = profile(false, true);
        let peer_caching_flow_control = profile(true, true);
        let words = |config: &WmiWcn3990ResourceConfig10_4| {
            core::array::from_fn::<_, WMI_10_4_RESOURCE_CONFIG_WORDS, _>(|index| {
                let offset = index * 4;
                u32::from_le_bytes([
                    config.as_bytes()[offset],
                    config.as_bytes()[offset + 1],
                    config.as_bytes()[offset + 2],
                    config.as_bytes()[offset + 3],
                ])
            })
        };
        let expected: [u32; WMI_10_4_RESOURCE_CONFIG_WORDS] = [
            16, 33, 0, 0, 0, 2, 96, 32, 7, 7, 100, 100, 100, 40, 1, 4, 3, 3, 8, 0, 0, 0, 1024, 32,
            1, 0, 1, 0, 3, 1424, 2, 16, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0,
        ];
        assert_eq!(words(&standard), expected);
        let mut expected_peer_flow_control = expected;
        expected_peer_flow_control[29] = 2500;
        assert_eq!(words(&peer_flow_control), expected_peer_flow_control);
        let peer_caching_words = words(&peer_caching);
        assert_eq!(peer_caching_words[0], 16);
        assert_eq!(peer_caching_words[1], 528);
        assert_eq!(peer_caching_words[2], 66);
        assert_eq!(peer_caching_words[6], 132);
        assert_eq!(peer_caching_words[29], 1424);
        let peer_caching_pfc_words = words(&peer_caching_flow_control);
        assert_eq!(peer_caching_pfc_words[1], 528);
        assert_eq!(peer_caching_pfc_words[2], 51);
        assert_eq!(peer_caching_pfc_words[6], 102);
        assert_eq!(peer_caching_pfc_words[29], 2500);
        assert_eq!(
            standard.host_memory_units_for_hint(WmiHostMemoryCountHint::Fixed),
            Ok(None)
        );
        assert_eq!(
            standard.host_memory_units_for_hint(WmiHostMemoryCountHint::Vdevs),
            Ok(Some(17))
        );
        assert_eq!(
            standard.host_memory_units_for_hint(WmiHostMemoryCountHint::Peers),
            Ok(Some(34))
        );
        assert_eq!(
            standard.host_memory_units_for_hint(WmiHostMemoryCountHint::ActivePeers),
            Ok(Some(34))
        );
        assert_eq!(
            peer_caching.host_memory_units_for_hint(WmiHostMemoryCountHint::Peers),
            Ok(Some(529))
        );
        assert_eq!(
            peer_caching.host_memory_units_for_hint(WmiHostMemoryCountHint::ActivePeers),
            Ok(Some(67))
        );
        assert_eq!(
            peer_caching_flow_control
                .host_memory_units_for_hint(WmiHostMemoryCountHint::ActivePeers),
            Ok(Some(52))
        );

        let empty_batch = WmiHostMemoryBatchPlan::<0> {
            requests: [],
            total_bytes: 0,
        };
        let assignment = empty_batch.assign_iova_pool(0x8000_0000, 0).unwrap();
        let mut body = [0xa5; WMI_10_4_INIT_BASE_BYTES];
        assert_eq!(
            assignment.encode_init_body_10_4(standard.as_bytes(), &mut body),
            Ok(WMI_10_4_INIT_BASE_BYTES)
        );
        assert_eq!(&body[..WMI_10_4_RESOURCE_CONFIG_BYTES], standard.as_bytes());
    }

    #[test]
    fn wcn3990_resource_profile_requires_wmi_10_4_metadata() {
        let no_peer_caching = service_ready_10_4_fixture(&[], 0, &[0; 16]);
        let service_ready = Wmi10_4ServiceReadyView::parse(&no_peer_caching).unwrap();
        assert_eq!(
            WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
                &firmware_metadata(false, None),
                &service_ready,
            ),
            Err(WmiWcn3990ResourceConfigError::WmiOperationVersionMissing)
        );
        assert_eq!(
            WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
                &firmware_metadata(false, Some(3)),
                &service_ready
            ),
            Err(WmiWcn3990ResourceConfigError::UnsupportedWmiOperationVersion(3))
        );

        assert_eq!(
            Wmi10_4ServiceReadyView::parse(
                &no_peer_caching[..WMI_10_4_SERVICE_READY_FIXED_SIZE - 1]
            ),
            Err(Wmi10_4ServiceReadyError::EventTooShort)
        );
    }

    #[test]
    fn wmi_10_4_service_ready_uses_fixed_layout_raw_service_bits_and_inline_requests() {
        let mut service_words = [0u32; 16];
        service_words[WMI_10_4_SERVICE_PEER_CACHING_INDEX / 32] |=
            1 << (WMI_10_4_SERVICE_PEER_CACHING_INDEX % 32);
        let mut request = Vec::new();
        append_host_memory_request_10_4(&mut request, 9, 64, 0, 2);
        let mut event = service_ready_10_4_fixture(&request, 1, &service_words);
        event.extend_from_slice(&[0xaa, 0xbb]);

        let view = Wmi10_4ServiceReadyView::parse(&event).unwrap();
        assert_eq!(view.event.len(), event.len());
        assert_eq!(view.service_bitmap.len(), 64);
        assert_eq!(view.service_word(1), Some(1 << 5));
        assert!(view.service_enabled(37));
        assert!(!view.service_enabled(78));
        assert_eq!(view.host_memory_request_records().count(), 1);
        assert_eq!(view.trailing, &[0xaa, 0xbb]);
        assert_eq!(
            WmiHostMemoryRequest::parse_10_4(view.host_memory_request_records().next().unwrap()),
            Ok(WmiHostMemoryRequest {
                request_id: 9,
                unit_size: 64,
                num_unit_info: 0,
                num_units: 2,
            })
        );

        let raw_cache = service_ready_10_4_fixture(&[], 0, &service_words);
        let raw_cache = Wmi10_4ServiceReadyView::parse(&raw_cache).unwrap();
        let raw_cache_config = WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
            &firmware_metadata(false, Some(6)),
            &raw_cache,
        )
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(raw_cache_config.as_bytes()[4..8].try_into().unwrap()),
            528
        );

        let mut only_linux_normalized_bit = [0u32; 16];
        only_linux_normalized_bit[78 / 32] |= 1 << (78 % 32);
        let event = service_ready_10_4_fixture(&[], 0, &only_linux_normalized_bit);
        let view = Wmi10_4ServiceReadyView::parse(&event).unwrap();
        let config = WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
            &firmware_metadata(false, Some(6)),
            &view,
        )
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(config.as_bytes()[4..8].try_into().unwrap()),
            TARGET_HL_TLV_NUM_PEERS
        );
    }

    #[test]
    fn wmi_10_4_service_ready_rejects_truncated_inline_memory_requests() {
        let event = service_ready_10_4_fixture(&[0; 15], 1, &[0; 16]);
        assert_eq!(
            Wmi10_4ServiceReadyView::parse(&event),
            Err(Wmi10_4ServiceReadyError::MemoryRequestsTruncated)
        );
        let event = service_ready_10_4_fixture(&[], 17, &[0; 16]);
        assert_eq!(
            Wmi10_4ServiceReadyView::parse(&event),
            Err(Wmi10_4ServiceReadyError::TooManyMemoryRequests)
        );
    }

    #[test]
    fn wmi_10_4_service_ready_plans_and_encodes_one_complete_init_command() {
        use crate::ath10k_ce::{
            HtcControlMessage, HtcCreditLedger, HtcRxHeader, HtcServicePolicy, HtcSession,
            HtcTxHeader, Wcn3990Descriptor,
        };

        let mut requests = Vec::new();
        // Keep the firmware's request order different from the policy order;
        // INIT must preserve the accepted event order.
        append_host_memory_request_10_4(&mut requests, 7, 64, 0, 2);
        append_host_memory_request_10_4(&mut requests, 3, 32, 0, 3);
        let event = service_ready_10_4_fixture(&requests, 2, &[0; 16]);
        let service_ready = Wmi10_4ServiceReadyView::parse(&event).unwrap();
        let policies = [
            WmiHostMemoryPolicy {
                request_id: 3,
                unit_size: 32,
                count_hint: WmiHostMemoryCountHint::Fixed,
                unit_count: 3,
                max_bytes: 96,
            },
            WmiHostMemoryPolicy {
                request_id: 7,
                unit_size: 64,
                count_hint: WmiHostMemoryCountHint::Fixed,
                unit_count: 2,
                max_bytes: 128,
            },
        ];
        let resource_config = WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
            &firmware_metadata(false, Some(6)),
            &service_ready,
        )
        .unwrap();
        let batch = service_ready
            .plan_host_memory_for_resource_config(&policies, &resource_config, 224)
            .unwrap();
        assert_eq!(batch.total_bytes, 224);
        assert_eq!(batch.requests[0].request_id, 7);
        assert_eq!(batch.requests[1].request_id, 3);

        let assignment = batch.assign_iova_pool(0x8000_0000, 0x2000).unwrap();
        let required = WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_INIT_BASE_BYTES + 2 * 12;
        let mut command = vec![0xa5; required];
        assert_eq!(
            assignment.encode_init_command_10_4(resource_config.as_bytes(), &mut command),
            Ok(required)
        );
        assert_eq!(
            &command[..WMI_10_4_CMD_HEADER_SIZE],
            &WMI_10_4_INIT_CMD_ID.to_le_bytes()
        );
        assert_eq!(
            &command[WMI_10_4_CMD_HEADER_SIZE
                ..WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_RESOURCE_CONFIG_BYTES],
            resource_config.as_bytes()
        );
        let chunks_start = WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_RESOURCE_CONFIG_BYTES;
        assert_eq!(
            &command[chunks_start..chunks_start + 4],
            &2u32.to_le_bytes()
        );
        assert_eq!(
            &command[chunks_start + 4..chunks_start + 28],
            &[
                7, 0, 0, 0, 0, 0, 0, 0x80, 128, 0, 0, 0, 3, 0, 0, 0, 0, 0x10, 0, 0x80, 96, 0, 0, 0,
            ]
        );
        assert_eq!(&command[chunks_start + 28..], &[0; 12]);

        // Follow the encoded WMI INIT through the source-defined HTC service
        // connection, dynamically assigned endpoint, credit accounting, and
        // CE descriptor. This serializes a fixture only; it submits no DMA.
        const WMI_CONTROL_SERVICE_ID: u16 = 0x0100;
        let ready_message = [1, 0, 24, 0, 0, 1, 8, 0];
        let HtcControlMessage::Ready(ready) = HtcControlMessage::parse(&ready_message).unwrap()
        else {
            panic!("HTC READY fixture decoded as another message");
        };
        let mut session = HtcSession::new(ready).unwrap();
        let policy = HtcServicePolicy::derive(WMI_CONTROL_SERVICE_ID, ready).unwrap();
        let connect_request = policy.connect_request(WMI_CONTROL_SERVICE_ID).encode();
        assert_eq!(connect_request, [2, 0, 0, 1, 0, 0x18, 0, 0]);

        let mut connect_frame = [0u8; 16];
        assert_eq!(
            HtcTxHeader::write_frame(0, &connect_request, 0, false, &mut connect_frame),
            Ok(16)
        );
        assert_eq!(&connect_frame[..8], &[0, 0, 8, 0, 0, 0, 0, 0]);

        let response_message = [3, 0, 0, 1, 0, 3, 0, 8];
        let HtcControlMessage::ConnectServiceResponse(response) =
            HtcControlMessage::parse(&response_message).unwrap()
        else {
            panic!("HTC response fixture decoded as another message");
        };
        let (endpoint, connected_policy) =
            session.connect(WMI_CONTROL_SERVICE_ID, response).unwrap();
        assert_eq!(endpoint.endpoint_id, 3);
        assert_eq!(endpoint.max_message_size, 2048);
        assert!(connected_policy.flow_control_enabled());

        let mut htc_frame = [0u8; 512];
        let frame_len = HtcTxHeader::write_frame(
            endpoint.endpoint_id,
            &command,
            1,
            connected_policy.flow_control_enabled(),
            &mut htc_frame,
        )
        .unwrap();
        assert_eq!(frame_len, HtcRxHeader::HEADER_SIZE + command.len());
        let htc_header = HtcRxHeader::parse(&htc_frame[..frame_len], frame_len).unwrap();
        assert_eq!(htc_header.endpoint_id, 3);
        assert_eq!(htc_header.flags, 1);
        assert_eq!(htc_header.payload_len as usize, command.len());
        assert_eq!(&htc_frame[HtcRxHeader::HEADER_SIZE..frame_len], command);

        let mut credits: HtcCreditLedger = session.credits();
        assert_eq!(credits.consume(endpoint.endpoint_id, frame_len), Ok(1));
        assert_eq!(credits.available(endpoint.endpoint_id), Ok(23));
        let descriptor =
            Wcn3990Descriptor::encode(0x8000_0000, frame_len, 0x321, false, false).unwrap();
        assert_eq!(
            u32::from_le_bytes(descriptor.as_bytes()[0..4].try_into().unwrap()),
            0x8000_0000
        );
        assert_eq!(
            u16::from_le_bytes(descriptor.as_bytes()[8..10].try_into().unwrap()) as usize,
            frame_len
        );
        assert_eq!(
            u16::from_le_bytes(descriptor.as_bytes()[10..12].try_into().unwrap()),
            0x3210
        );
    }

    #[test]
    fn wmi_10_4_ready_event_uses_fixed_20_byte_layout() {
        let mut event = [0u8; WMI_10_4_READY_EVENT_FIXED_SIZE + 1];
        event[0..4].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        event[4..8].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        event[8..16].copy_from_slice(&[1, 2, 3, 4, 5, 6, 0xaa, 0xbb]);
        event[16..20].copy_from_slice(&0x99aabbccu32.to_le_bytes());
        event[20] = 0xdd;
        let ready = Wmi10_4ReadyEvent::parse(&event).unwrap();
        assert_eq!(ready.software_version, 0x1122_3344);
        assert_eq!(ready.abi_version, 0x5566_7788);
        assert_eq!(ready.mac_address, [1, 2, 3, 4, 5, 6]);
        assert_eq!(ready.mac_reserved, [0xaa, 0xbb]);
        assert_eq!(ready.status, 0x99aabbcc);
        assert_eq!(ready.trailing, &[0xdd]);
        assert_eq!(
            Wmi10_4ReadyEvent::parse(&event[..19]),
            Err(Wmi10_4ReadyError::EventTooShort)
        );
    }

    #[test]
    fn host_memory_dynamic_units_match_the_wcn3990_resource_profile() {
        let mut requests = Vec::new();
        append_host_memory_request_10_4(&mut requests, 7, 64, WMI_NUM_UNITS_IS_NUM_ACTIVE_PEERS, 0);
        let mut service_words = [0u32; 16];
        service_words[WMI_10_4_SERVICE_PEER_CACHING_INDEX / 32] |=
            1 << (WMI_10_4_SERVICE_PEER_CACHING_INDEX % 32);
        let event = service_ready_10_4_fixture(&requests, 1, &service_words);
        let service_ready = Wmi10_4ServiceReadyView::parse(&event).unwrap();
        let resource_config = WmiWcn3990ResourceConfig10_4::from_firmware_metadata(
            &firmware_metadata(true, Some(6)),
            &service_ready,
        )
        .unwrap();
        let policy = WmiHostMemoryPolicy {
            request_id: 7,
            unit_size: 64,
            count_hint: WmiHostMemoryCountHint::ActivePeers,
            unit_count: 52,
            max_bytes: 52 * 64,
        };
        let plan = service_ready
            .plan_host_memory_for_resource_config(&[policy], &resource_config, 52 * 64)
            .unwrap();
        assert_eq!(plan.requests[0].unit_count, 52);

        let wrong_policy = WmiHostMemoryPolicy {
            unit_count: 51,
            max_bytes: 51 * 64,
            ..policy
        };
        assert_eq!(
            service_ready.plan_host_memory_for_resource_config(
                &[wrong_policy],
                &resource_config,
                51 * 64,
            ),
            Err(WmiHostMemoryBatchError::Policy(
                WmiHostMemoryPolicyError::UnitCountMismatch,
            ))
        );
    }

    fn append_host_memory_request(
        out: &mut Vec<u8>,
        request_id: u32,
        unit_size: u32,
        num_unit_info: u32,
        num_units: u32,
    ) {
        let mut request = [0u8; WMI_HOST_MEM_REQ_FIXED_SIZE];
        request[0..4].copy_from_slice(&request_id.to_le_bytes());
        request[4..8].copy_from_slice(&unit_size.to_le_bytes());
        request[8..12].copy_from_slice(&num_unit_info.to_le_bytes());
        request[12..16].copy_from_slice(&num_units.to_le_bytes());
        append_tlv(out, WMI_TLV_WLAN_HOST_MEM_REQ, &request);
    }

    fn append_host_memory_request_10_4(
        out: &mut Vec<u8>,
        request_id: u32,
        unit_size: u32,
        num_unit_info: u32,
        num_units: u32,
    ) {
        out.extend_from_slice(&request_id.to_le_bytes());
        out.extend_from_slice(&unit_size.to_le_bytes());
        out.extend_from_slice(&num_unit_info.to_le_bytes());
        out.extend_from_slice(&num_units.to_le_bytes());
    }

    #[test]
    fn iterator_parses_little_endian_records_without_alignment_padding() {
        let bytes = [
            3, 0, 0x34, 0x12, 0xaa, 0xbb, 0xcc, // three-byte value
            1, 0, 0x78, 0x56, 0xdd, // immediately follows at offset 7
        ];
        let records: Result<Vec<_>, _> = WmiTlvIter::new(&bytes).collect();
        assert_eq!(
            records,
            Ok(vec![
                WmiTlvRecord {
                    tag: 0x1234,
                    value: &[0xaa, 0xbb, 0xcc],
                },
                WmiTlvRecord {
                    tag: 0x5678,
                    value: &[0xdd],
                },
            ])
        );
    }

    #[test]
    fn iterator_accepts_zero_length_values_and_empty_input() {
        assert_eq!(WmiTlvIter::new(&[]).next(), None);
        assert_eq!(
            WmiTlvIter::new(&[0, 0, 0x34, 0x12]).next(),
            Some(Ok(WmiTlvRecord {
                tag: 0x1234,
                value: &[],
            }))
        );
    }

    #[test]
    fn iterator_rejects_partial_header_and_truncated_value_then_stops() {
        let mut partial_header = WmiTlvIter::new(&[1, 0, 2]);
        assert_eq!(
            partial_header.next(),
            Some(Err(WmiTlvError::HeaderTruncated))
        );
        assert_eq!(partial_header.next(), None);

        let mut short_value = WmiTlvIter::new(&[4, 0, 2, 0, 0xaa, 0xbb]);
        assert_eq!(short_value.next(), Some(Err(WmiTlvError::ValueTruncated)));
        assert_eq!(short_value.next(), None);
    }

    #[test]
    fn service_ready_keeps_memory_request_payload_opaque_and_borrowed() {
        let mut requests = Vec::new();
        let mut opaque_request = [0u8; WMI_HOST_MEM_REQ_FIXED_SIZE];
        opaque_request[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        opaque_request[4..8].copy_from_slice(&0x1000u32.to_le_bytes());
        opaque_request[8..12].copy_from_slice(&0xfeed_beefu32.to_le_bytes());
        opaque_request[12..16].copy_from_slice(&7u32.to_le_bytes());
        append_tlv(&mut requests, WMI_TLV_WLAN_HOST_MEM_REQ, &opaque_request);
        let bytes = service_ready_fixture(&requests);
        let view = WmiServiceReadyView::parse(&bytes).unwrap();

        assert_eq!(view.declared_host_memory_request_count, 1);
        assert_eq!(view.service_word(0), Some(0x1234));
        assert_eq!(view.service_word(1), Some(0x5678));
        assert_eq!(view.service_word(2), None);
        let request = view.host_memory_request_records().next().unwrap().unwrap();
        assert_eq!(request.tag, WMI_TLV_WLAN_HOST_MEM_REQ);
        assert_eq!(request.value, &opaque_request);
        assert_eq!(
            WmiHostMemoryRequest::parse(request),
            Ok(WmiHostMemoryRequest {
                request_id: 0x1234_5678,
                unit_size: 0x1000,
                num_unit_info: 0xfeed_beef,
                num_units: 7,
            })
        );
        assert_eq!(
            WmiHostMemoryRequest::parse(request).unwrap().count_hint(),
            None
        );
        assert!(core::ptr::eq(view.event.as_ptr(), bytes[4..].as_ptr()));
    }

    #[test]
    fn host_memory_request_count_hint_accepts_only_known_single_modes() {
        let modes = [
            (0, Some(WmiHostMemoryCountHint::Fixed)),
            (
                WMI_NUM_UNITS_IS_NUM_VDEVS,
                Some(WmiHostMemoryCountHint::Vdevs),
            ),
            (
                WMI_NUM_UNITS_IS_NUM_PEERS,
                Some(WmiHostMemoryCountHint::Peers),
            ),
            (
                WMI_NUM_UNITS_IS_NUM_ACTIVE_PEERS,
                Some(WmiHostMemoryCountHint::ActivePeers),
            ),
            (
                WMI_NUM_UNITS_IS_NUM_PEERS | WMI_NUM_UNITS_IS_NUM_VDEVS,
                None,
            ),
            (1 << 31, None),
        ];

        for (num_unit_info, expected) in modes {
            let request = WmiHostMemoryRequest {
                request_id: 1,
                unit_size: 64,
                num_unit_info,
                num_units: 3,
            };
            assert_eq!(request.count_hint(), expected);
        }
    }

    #[test]
    fn host_memory_request_policy_uses_trusted_count_and_checked_size() {
        let dynamic_request = WmiHostMemoryRequest {
            request_id: 9,
            unit_size: 65,
            num_unit_info: WMI_NUM_UNITS_IS_NUM_PEERS,
            num_units: 0,
        };
        let policy = WmiHostMemoryPolicy {
            request_id: 9,
            unit_size: 65,
            count_hint: WmiHostMemoryCountHint::Peers,
            unit_count: 4,
            max_bytes: 272,
        };
        assert_eq!(
            dynamic_request.checked_plan(policy),
            Ok(WmiHostMemoryPlan {
                request_id: 9,
                unit_count: 4,
                rounded_unit_size: 68,
                byte_len: 272,
            })
        );
        assert_eq!(
            dynamic_request.checked_plan(WmiHostMemoryPolicy {
                max_bytes: 271,
                ..policy
            }),
            Err(WmiHostMemoryPolicyError::ExceedsBudget)
        );

        let fixed_request = WmiHostMemoryRequest {
            request_id: 10,
            unit_size: 0xffff_fffe,
            num_unit_info: 0,
            num_units: 2,
        };
        let fixed_policy = WmiHostMemoryPolicy {
            request_id: 10,
            unit_size: 0xffff_fffe,
            count_hint: WmiHostMemoryCountHint::Fixed,
            unit_count: 2,
            max_bytes: u32::MAX,
        };
        assert_eq!(
            fixed_request.checked_plan(fixed_policy),
            Err(WmiHostMemoryPolicyError::SizeOverflow)
        );
        assert_eq!(
            WmiHostMemoryRequest {
                num_units: 1,
                ..dynamic_request
            }
            .checked_plan(policy),
            Err(WmiHostMemoryPolicyError::UnitCountMismatch)
        );
        assert_eq!(
            dynamic_request.checked_plan(WmiHostMemoryPolicy {
                unit_count: 0,
                ..policy
            }),
            Err(WmiHostMemoryPolicyError::EmptyAllocation)
        );
    }

    #[test]
    fn service_ready_caps_memory_request_count_at_the_ath10k_limit() {
        let mut requests = Vec::new();
        for request_id in 0..=WMI_MAX_HOST_MEMORY_REQUESTS {
            append_host_memory_request(&mut requests, request_id as u32, 64, 0, 1);
        }
        assert_eq!(
            WmiServiceReadyView::parse(&service_ready_fixture_with_count(
                &requests,
                (WMI_MAX_HOST_MEMORY_REQUESTS + 1) as u32,
            )),
            Err(WmiServiceReadyError::TooManyMemoryRequests)
        );

        let empty_event = service_ready_fixture_with_count(&[], 0);
        let view = WmiServiceReadyView::parse(&empty_event).unwrap();
        let policy = WmiHostMemoryPolicy {
            request_id: 0,
            unit_size: 4,
            count_hint: WmiHostMemoryCountHint::Fixed,
            unit_count: 1,
            max_bytes: 4,
        };
        let too_many_policies = [policy; WMI_MAX_HOST_MEMORY_REQUESTS + 1];
        assert_eq!(
            view.plan_host_memory(&too_many_policies, 4),
            Err(WmiHostMemoryBatchError::TooManyPolicies)
        );
    }

    #[test]
    fn host_memory_batch_plan_matches_every_request_and_caps_aggregate_bytes() {
        let mut requests = Vec::new();
        // Deliberately use wire order opposite to policy order; the plan must
        // retain the firmware order while matching by trusted request ID.
        append_host_memory_request(&mut requests, 10, 128, 0, 2);
        append_host_memory_request(&mut requests, 9, 65, WMI_NUM_UNITS_IS_NUM_PEERS, 0);
        let event = service_ready_fixture_with_count(&requests, 2);
        let view = WmiServiceReadyView::parse(&event).unwrap();
        let policies = [
            WmiHostMemoryPolicy {
                request_id: 9,
                unit_size: 65,
                count_hint: WmiHostMemoryCountHint::Peers,
                unit_count: 4,
                max_bytes: 272,
            },
            WmiHostMemoryPolicy {
                request_id: 10,
                unit_size: 128,
                count_hint: WmiHostMemoryCountHint::Fixed,
                unit_count: 2,
                max_bytes: 256,
            },
        ];

        assert_eq!(
            view.plan_host_memory(&policies, 528),
            Ok(WmiHostMemoryBatchPlan {
                requests: [
                    WmiHostMemoryPlan {
                        request_id: 10,
                        unit_count: 2,
                        rounded_unit_size: 128,
                        byte_len: 256,
                    },
                    WmiHostMemoryPlan {
                        request_id: 9,
                        unit_count: 4,
                        rounded_unit_size: 68,
                        byte_len: 272,
                    },
                ],
                total_bytes: 528,
            })
        );
        assert_eq!(
            view.plan_host_memory(&policies, 527),
            Err(WmiHostMemoryBatchError::ExceedsAggregateBudget)
        );
    }

    #[test]
    fn host_memory_assignment_places_and_encodes_bounded_wmi_chunks() {
        let batch = WmiHostMemoryBatchPlan {
            requests: [
                WmiHostMemoryPlan {
                    request_id: 10,
                    unit_count: 2,
                    rounded_unit_size: 128,
                    byte_len: 256,
                },
                WmiHostMemoryPlan {
                    request_id: 9,
                    unit_count: 4,
                    rounded_unit_size: 68,
                    byte_len: 272,
                },
            ],
            total_bytes: 528,
        };
        let assignment = batch.assign_iova_pool(0x8000_0000, 0x2000).unwrap();
        assert_eq!(assignment.reserved_bytes(), 0x2000);
        assert_eq!(
            assignment.chunks(),
            &[
                WmiHostMemoryChunk {
                    request_id: 10,
                    iova: 0x8000_0000,
                    byte_len: 256,
                },
                WmiHostMemoryChunk {
                    request_id: 9,
                    iova: 0x8000_1000,
                    byte_len: 272,
                },
            ]
        );

        let mut encoded = [0xaau8; 28];
        assert_eq!(assignment.encode_chunk_array(&mut encoded), Ok(28));
        assert_eq!(
            encoded,
            [
                2, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0x80, 0, 1, 0, 0, 9, 0, 0, 0, 0, 0x10, 0, 0x80,
                0x10, 1, 0, 0,
            ]
        );

        let mut short = [0xaau8; 27];
        assert_eq!(
            assignment.encode_chunk_array(&mut short),
            Err(WmiHostMemoryAssignmentError::OutputTooSmall)
        );
        assert_eq!(short, [0xaa; 27]);
    }

    #[test]
    fn host_memory_assignment_rejects_bad_pool_plan_and_address_width() {
        let batch = WmiHostMemoryBatchPlan {
            requests: [WmiHostMemoryPlan {
                request_id: 7,
                unit_count: 1,
                rounded_unit_size: 256,
                byte_len: 256,
            }],
            total_bytes: 256,
        };
        assert_eq!(
            batch.assign_iova_pool(0x8000_0001, 0x1000),
            Err(WmiHostMemoryAssignmentError::PoolMisaligned)
        );
        assert_eq!(
            batch.assign_iova_pool(0x8000_0000, 0x0fff),
            Err(WmiHostMemoryAssignmentError::PoolTooSmall)
        );
        assert_eq!(
            batch.assign_iova_pool(0xffff_f000, 0x2000),
            Err(WmiHostMemoryAssignmentError::AddressOutOfRange)
        );

        let duplicate = WmiHostMemoryBatchPlan {
            requests: [batch.requests[0], batch.requests[0]],
            total_bytes: 512,
        };
        assert_eq!(
            duplicate.assign_iova_pool(0x8000_0000, 0x2000),
            Err(WmiHostMemoryAssignmentError::DuplicateRequestId)
        );

        let mismatched_total = WmiHostMemoryBatchPlan {
            total_bytes: 257,
            ..batch
        };
        assert_eq!(
            mismatched_total.assign_iova_pool(0x8000_0000, 0x1000),
            Err(WmiHostMemoryAssignmentError::TotalSizeMismatch)
        );
    }

    #[test]
    fn init_10_4_body_matches_packed_resource_and_chunk_layout() {
        let batch = WmiHostMemoryBatchPlan {
            requests: [
                WmiHostMemoryPlan {
                    request_id: 10,
                    unit_count: 2,
                    rounded_unit_size: 128,
                    byte_len: 256,
                },
                WmiHostMemoryPlan {
                    request_id: 9,
                    unit_count: 4,
                    rounded_unit_size: 68,
                    byte_len: 272,
                },
            ],
            total_bytes: 528,
        };
        let assignment = batch.assign_iova_pool(0x8000_0000, 0x2000).unwrap();
        let resource_config: [u8; WMI_10_4_RESOURCE_CONFIG_BYTES] =
            core::array::from_fn(|index| index as u8);
        let mut output = [0xa5; 208];

        assert_eq!(
            assignment.encode_init_body_10_4(&resource_config, &mut output),
            Ok(208)
        );
        assert_eq!(&output[..WMI_10_4_RESOURCE_CONFIG_BYTES], &resource_config);
        assert_eq!(&output[168..172], &[2, 0, 0, 0]);
        assert_eq!(
            &output[172..196],
            &[
                10, 0, 0, 0, 0, 0, 0, 0x80, 0, 1, 0, 0, 9, 0, 0, 0, 0, 0x10, 0, 0x80, 0x10, 1, 0,
                0,
            ]
        );
        assert_eq!(&output[196..208], &[0; 12]);

        let mut short = [0xa5; 207];
        assert_eq!(
            assignment.encode_init_body_10_4(&resource_config, &mut short),
            Err(WmiInit10_4EncodeError::OutputTooSmall {
                required: 208,
                available: 207,
            })
        );
        assert_eq!(short, [0xa5; 207]);

        let mut wrong_config = [0xa5; 208];
        assert_eq!(
            assignment.encode_init_body_10_4(&resource_config[..167], &mut wrong_config),
            Err(WmiInit10_4EncodeError::ResourceConfigWrongSize {
                expected: 168,
                observed: 167,
            })
        );
        assert_eq!(wrong_config, [0xa5; 208]);
    }

    #[test]
    fn init_10_4_zero_request_body_retains_one_zeroed_union_slot() {
        let empty_batch = WmiHostMemoryBatchPlan::<0> {
            requests: [],
            total_bytes: 0,
        };
        let assignment = empty_batch.assign_iova_pool(0x8000_0000, 0).unwrap();
        let resource_config = [0x5a; WMI_10_4_RESOURCE_CONFIG_BYTES];
        let mut output = [0xa5; WMI_10_4_INIT_BASE_BYTES];

        assert_eq!(
            assignment.encode_init_body_10_4(&resource_config, &mut output),
            Ok(WMI_10_4_INIT_BASE_BYTES)
        );
        assert_eq!(&output[..WMI_10_4_RESOURCE_CONFIG_BYTES], &resource_config);
        assert_eq!(&output[168..], &[0; 16]);
    }

    #[test]
    fn init_10_4_command_prepends_source_derived_command_id_atomically() {
        let empty_batch = WmiHostMemoryBatchPlan::<0> {
            requests: [],
            total_bytes: 0,
        };
        let assignment = empty_batch.assign_iova_pool(0x8000_0000, 0).unwrap();
        let resource_config = [0x5a; WMI_10_4_RESOURCE_CONFIG_BYTES];
        let required = WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_INIT_BASE_BYTES;
        let mut output = [0xa5; WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_INIT_BASE_BYTES];
        assert_eq!(
            assignment.encode_init_command_10_4(&resource_config, &mut output),
            Ok(required)
        );
        assert_eq!(
            &output[..WMI_10_4_CMD_HEADER_SIZE],
            &WMI_10_4_INIT_CMD_ID.to_le_bytes()
        );
        assert_eq!(
            &output[WMI_10_4_CMD_HEADER_SIZE
                ..WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_RESOURCE_CONFIG_BYTES],
            &resource_config
        );
        assert_eq!(&output[WMI_10_4_CMD_HEADER_SIZE + 168..], &[0; 16]);

        let mut short = [0xa5; WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_INIT_BASE_BYTES - 1];
        assert_eq!(
            assignment.encode_init_command_10_4(&resource_config, &mut short),
            Err(WmiInit10_4EncodeError::OutputTooSmall {
                required,
                available: required - 1,
            })
        );
        assert_eq!(
            short,
            [0xa5; WMI_10_4_CMD_HEADER_SIZE + WMI_10_4_INIT_BASE_BYTES - 1]
        );
    }

    #[test]
    fn host_memory_batch_plan_rejects_unknown_and_duplicate_ids() {
        let policies = [
            WmiHostMemoryPolicy {
                request_id: 1,
                unit_size: 64,
                count_hint: WmiHostMemoryCountHint::Fixed,
                unit_count: 1,
                max_bytes: 64,
            },
            WmiHostMemoryPolicy {
                request_id: 2,
                unit_size: 64,
                count_hint: WmiHostMemoryCountHint::Fixed,
                unit_count: 1,
                max_bytes: 64,
            },
        ];

        let mut unknown = Vec::new();
        append_host_memory_request(&mut unknown, 3, 64, 0, 1);
        append_host_memory_request(&mut unknown, 2, 64, 0, 1);
        let unknown_event = service_ready_fixture_with_count(&unknown, 2);
        let unknown_view = WmiServiceReadyView::parse(&unknown_event).unwrap();
        assert_eq!(
            unknown_view.plan_host_memory(&policies, 128),
            Err(WmiHostMemoryBatchError::UnknownRequestId)
        );

        let mut duplicate = Vec::new();
        append_host_memory_request(&mut duplicate, 1, 64, 0, 1);
        append_host_memory_request(&mut duplicate, 1, 64, 0, 1);
        let duplicate_event = service_ready_fixture_with_count(&duplicate, 2);
        let duplicate_view = WmiServiceReadyView::parse(&duplicate_event).unwrap();
        assert_eq!(
            duplicate_view.plan_host_memory(&policies, 128),
            Err(WmiHostMemoryBatchError::DuplicateRequestId)
        );

        let duplicate_policies = [policies[0], policies[0]];
        assert_eq!(
            duplicate_view.plan_host_memory(&duplicate_policies, 128),
            Err(WmiHostMemoryBatchError::DuplicatePolicyId)
        );
    }

    #[test]
    fn host_memory_request_decoder_rejects_unknown_tag_and_non_fixed_length() {
        assert_eq!(
            WmiHostMemoryRequest::parse(WmiTlvRecord {
                tag: 0x1234,
                value: &[0; WMI_HOST_MEM_REQ_FIXED_SIZE],
            }),
            Err(WmiHostMemoryRequestError::WrongTag)
        );
        assert_eq!(
            WmiHostMemoryRequest::parse(WmiTlvRecord {
                tag: WMI_TLV_WLAN_HOST_MEM_REQ,
                value: &[0; WMI_HOST_MEM_REQ_FIXED_SIZE + 1],
            }),
            Err(WmiHostMemoryRequestError::WrongLength)
        );
    }

    #[test]
    fn service_ready_requires_all_three_records_and_a_complete_event() {
        assert_eq!(
            WmiServiceReadyView::parse(&[]),
            Err(WmiServiceReadyError::ServiceReadyMissing)
        );

        let mut short_event = Vec::new();
        append_tlv(&mut short_event, WMI_TLV_SERVICE_READY_EVENT, &[0; 8]);
        assert_eq!(
            WmiServiceReadyView::parse(&short_event),
            Err(WmiServiceReadyError::ServiceReadyTooShort)
        );

        let mut no_bitmap = Vec::new();
        append_tlv(
            &mut no_bitmap,
            WMI_TLV_SERVICE_READY_EVENT,
            &[0; WMI_SERVICE_READY_FIXED_SIZE],
        );
        append_tlv(&mut no_bitmap, WMI_TLV_ARRAY_STRUCT, &[]);
        assert_eq!(
            WmiServiceReadyView::parse(&no_bitmap),
            Err(WmiServiceReadyError::ServiceBitmapMissing)
        );

        let mut no_memory_requests = Vec::new();
        append_tlv(
            &mut no_memory_requests,
            WMI_TLV_SERVICE_READY_EVENT,
            &[0; WMI_SERVICE_READY_FIXED_SIZE],
        );
        append_tlv(&mut no_memory_requests, WMI_TLV_ARRAY_UINT32, &[1, 0, 0, 0]);
        assert_eq!(
            WmiServiceReadyView::parse(&no_memory_requests),
            Err(WmiServiceReadyError::MemoryRequestsMissing)
        );
    }

    #[test]
    fn service_ready_rejects_malformed_bitmap_request_array_and_count() {
        let mut malformed_bitmap = Vec::new();
        append_tlv(
            &mut malformed_bitmap,
            WMI_TLV_SERVICE_READY_EVENT,
            &[0; WMI_SERVICE_READY_FIXED_SIZE],
        );
        append_tlv(&mut malformed_bitmap, WMI_TLV_ARRAY_UINT32, &[1, 2, 3]);
        append_tlv(&mut malformed_bitmap, WMI_TLV_ARRAY_STRUCT, &[]);
        assert_eq!(
            WmiServiceReadyView::parse(&malformed_bitmap),
            Err(WmiServiceReadyError::ServiceBitmapMalformed)
        );

        let mut bad_request = Vec::new();
        append_tlv(&mut bad_request, 0x1234, &[1]);
        assert_eq!(
            WmiServiceReadyView::parse(&service_ready_fixture(&bad_request)),
            Err(WmiServiceReadyError::MemoryRequestTagUnexpected)
        );

        let mut short_request = Vec::new();
        append_tlv(&mut short_request, WMI_TLV_WLAN_HOST_MEM_REQ, &[0; 15]);
        assert_eq!(
            WmiServiceReadyView::parse(&service_ready_fixture(&short_request)),
            Err(WmiServiceReadyError::MemoryRequestTooShort)
        );

        assert_eq!(
            WmiServiceReadyView::parse(&service_ready_fixture(&[])),
            Err(WmiServiceReadyError::MemoryRequestCountMismatch)
        );

        let mut truncated_request = Vec::new();
        truncated_request.extend_from_slice(&4u16.to_le_bytes());
        truncated_request.extend_from_slice(&WMI_TLV_WLAN_HOST_MEM_REQ.to_le_bytes());
        truncated_request.extend_from_slice(&[0xaa]);
        assert_eq!(
            WmiServiceReadyView::parse(&service_ready_fixture(&truncated_request)),
            Err(WmiServiceReadyError::Tlv(WmiTlvError::ValueTruncated))
        );
    }

    #[test]
    fn service_ready_rejects_ambiguous_duplicate_required_records() {
        let mut requests = Vec::new();
        append_tlv(
            &mut requests,
            WMI_TLV_WLAN_HOST_MEM_REQ,
            &[0; WMI_HOST_MEM_REQ_FIXED_SIZE],
        );
        let mut duplicate_event = service_ready_fixture(&requests);
        append_tlv(
            &mut duplicate_event,
            WMI_TLV_SERVICE_READY_EVENT,
            &[0; WMI_SERVICE_READY_FIXED_SIZE],
        );
        assert_eq!(
            WmiServiceReadyView::parse(&duplicate_event),
            Err(WmiServiceReadyError::ServiceReadyDuplicate)
        );

        let mut duplicate_requests = service_ready_fixture(&requests);
        append_tlv(&mut duplicate_requests, WMI_TLV_ARRAY_STRUCT, &requests);
        assert_eq!(
            WmiServiceReadyView::parse(&duplicate_requests),
            Err(WmiServiceReadyError::MemoryRequestsDuplicate)
        );
    }

    #[test]
    fn service_ready_uses_first_uint32_array_as_bitmap() {
        let mut requests = Vec::new();
        let mut event = [0u8; WMI_SERVICE_READY_FIXED_SIZE];
        event[WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET..WMI_SERVICE_READY_NUM_MEM_REQS_OFFSET + 4]
            .copy_from_slice(&0u32.to_le_bytes());
        append_tlv(&mut requests, WMI_TLV_SERVICE_READY_EVENT, &event);
        append_tlv(&mut requests, WMI_TLV_ARRAY_UINT32, &[0x34, 0x12, 0, 0]);
        append_tlv(
            &mut requests,
            WMI_TLV_ARRAY_UINT32,
            &[0xff, 0xff, 0xff, 0xff],
        );
        append_tlv(&mut requests, WMI_TLV_ARRAY_STRUCT, &[]);

        let view = WmiServiceReadyView::parse(&requests).unwrap();
        assert_eq!(view.service_word(0), Some(0x1234));
        assert_eq!(view.service_word(1), None);
    }

    #[test]
    fn full_reorder_capability_uses_the_pinned_tlv_service_bit() {
        let disabled_event = service_ready_fixture_with_bitmap(&[], 0, &[0, 0, 0]);
        let disabled = WmiServiceReadyView::parse(&disabled_event).unwrap();
        assert_eq!(disabled.rx_full_reorder_enabled(), Some(false));

        let enabled_event = service_ready_fixture_with_bitmap(
            &[],
            0,
            &[0, 0, 1 << (WMI_TLV_SERVICE_RX_FULL_REORDER_INDEX % 32)],
        );
        let enabled = WmiServiceReadyView::parse(&enabled_event).unwrap();
        assert_eq!(enabled.rx_full_reorder_enabled(), Some(true));

        let short_event = service_ready_fixture_with_bitmap(&[], 0, &[0, 0]);
        let short = WmiServiceReadyView::parse(&short_event).unwrap();
        assert_eq!(short.rx_full_reorder_enabled(), None);
    }

    #[test]
    fn ready_event_decodes_abi_mac_and_status_without_interpreting_them() {
        let mut payload = [0u8; WMI_READY_EVENT_FIXED_SIZE];
        for index in 0..6 {
            let start = index * 4;
            payload[start..start + 4].copy_from_slice(&(0x1020_3040 + index as u32).to_le_bytes());
        }
        payload[24..30].copy_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        payload[30..32].copy_from_slice(&[0xaa, 0xbb]);
        payload[32..36].copy_from_slice(&0x8765_4321u32.to_le_bytes());

        let mut bytes = Vec::new();
        append_tlv(&mut bytes, 0x7777, &[9]);
        append_tlv(&mut bytes, WMI_TLV_READY_EVENT, &payload);
        let ready = WmiReadyEvent::parse(&bytes).unwrap();
        assert_eq!(
            ready.abi_versions,
            [
                0x1020_3040,
                0x1020_3041,
                0x1020_3042,
                0x1020_3043,
                0x1020_3044,
                0x1020_3045,
            ]
        );
        assert_eq!(ready.mac_address, [0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(ready.status, 0x8765_4321);
    }

    #[test]
    fn ready_event_rejects_missing_duplicate_short_and_truncated_records() {
        assert_eq!(WmiReadyEvent::parse(&[]), Err(WmiReadyError::ReadyMissing));

        let mut short = Vec::new();
        append_tlv(
            &mut short,
            WMI_TLV_READY_EVENT,
            &[0; WMI_READY_EVENT_FIXED_SIZE - 1],
        );
        assert_eq!(
            WmiReadyEvent::parse(&short),
            Err(WmiReadyError::ReadyTooShort)
        );

        let mut duplicate = Vec::new();
        append_tlv(
            &mut duplicate,
            WMI_TLV_READY_EVENT,
            &[0; WMI_READY_EVENT_FIXED_SIZE],
        );
        append_tlv(
            &mut duplicate,
            WMI_TLV_READY_EVENT,
            &[0; WMI_READY_EVENT_FIXED_SIZE],
        );
        assert_eq!(
            WmiReadyEvent::parse(&duplicate),
            Err(WmiReadyError::ReadyDuplicate)
        );

        let mut truncated = Vec::new();
        truncated.extend_from_slice(&(WMI_READY_EVENT_FIXED_SIZE as u16).to_le_bytes());
        truncated.extend_from_slice(&WMI_TLV_READY_EVENT.to_le_bytes());
        truncated.extend_from_slice(&[0; 3]);
        assert_eq!(
            WmiReadyEvent::parse(&truncated),
            Err(WmiReadyError::Tlv(WmiTlvError::ValueTruncated))
        );
    }
}
